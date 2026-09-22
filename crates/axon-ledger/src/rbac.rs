/// RBAC for axon-ledger.
///
/// Two roles:
///   admin  — can view all records for all engineers
///   member — can only view their own records (principal matching their email)
///
/// Config lives in `<ledger-dir>/rbac.json`:
/// ```json
/// {
///   "admins": ["alice@example.com", "bob@example.com"]
/// }
/// ```
///
/// The "caller" identity is determined by `--as <email>` CLI flag or the
/// `AXON_PRINCIPAL` env var. If neither is set, the principal is "unknown"
/// and only unfiltered (single-engineer self-use) records are visible.
///
/// When RBAC is disabled (no rbac.json), all records are visible to everyone
/// (the original single-engineer behaviour).
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::LedgerRecord;

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct RbacConfig {
    #[serde(default)]
    pub admins: Vec<String>,
    /// OS identities that hold ADMIN AUTHORITY, as opposed to `admins`, which
    /// names claimed/display identities and confers nothing on its own.
    ///
    /// Entries are matched against [`authenticated_principal`] — an OS
    /// username, or `uid:<n>` when the uid has no passwd entry. Deliberately a
    /// SEPARATE list rather than reusing `admins` with a local-part heuristic:
    /// matching the OS user `alice` against the admin entry
    /// `alice@some-corp.example` would silently grant authority to whoever
    /// happens to hold that local account, which is the kind of accidental
    /// grant this whole change exists to remove. An operator says which OS
    /// identities are admins, explicitly, or none are.
    #[serde(default)]
    pub authenticated_admins: Vec<String>,
}

impl RbacConfig {
    /// Load rbac.json from the ledger directory.
    /// Returns a default (empty admins list) if the file does not exist.
    pub fn load(ledger_dir: &Path) -> anyhow::Result<RbacConfig> {
        let path = ledger_dir.join("rbac.json");
        if !path.exists() {
            return Ok(RbacConfig::default());
        }
        let text = std::fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self, ledger_dir: &Path) -> anyhow::Result<()> {
        let path = ledger_dir.join("rbac.json");
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn add_admin(&mut self, email: &str) {
        if !self.admins.iter().any(|a| a == email) {
            self.admins.push(email.to_string());
        }
    }

    pub fn remove_admin(&mut self, email: &str) {
        self.admins.retain(|a| a != email);
    }

    /// Grant PRIVILEGED authority to an OS identity.
    pub fn add_authenticated_admin(&mut self, who: &str) {
        if !self.authenticated_admins.iter().any(|a| a == who) {
            self.authenticated_admins.push(who.to_string());
        }
    }

    pub fn remove_authenticated_admin(&mut self, who: &str) {
        self.authenticated_admins.retain(|a| a != who);
    }

    /// Admin by CLAIMED identity. Confers nothing on its own — see
    /// [`RbacConfig::is_admin_authenticated`], which is what privileged
    /// operations consume.
    pub fn is_admin(&self, email: &str) -> bool {
        self.admins.iter().any(|a| a == email)
    }

    /// Is the RBAC gate armed at all?
    ///
    /// ONE definition, because the CLI and the MCP server both need it and a
    /// second copy would drift. Armed when EITHER list is non-empty: a ledger
    /// that lists only `authenticated_admins` is configured, and treating it
    /// as unconfigured made every record readable by everyone.
    pub fn gate_is_armed(&self) -> bool {
        !self.admins.is_empty() || !self.authenticated_admins.is_empty()
    }

    /// Admin by AUTHENTICATED identity: the only predicate that may authorize
    /// a privileged operation.
    ///
    /// Takes an [`AuthenticatedPrincipal`] rather than a `&str` so that a
    /// caller-supplied string cannot reach it by mistake. That is the whole
    /// point: the previous gate compared `rbac.admins` against whatever
    /// `--as` said, so `--as alice@example.com prune --yes` — run by anyone —
    /// deleted the entire ledger.
    pub fn is_admin_authenticated(&self, who: &AuthenticatedPrincipal) -> bool {
        let name = who.as_str();
        self.authenticated_admins.iter().any(|a| a == name)
    }

    /// Does `principal` belong to `caller`?
    ///
    /// ONE predicate, used by both filters. They were duplicated, so the same
    /// defect had to be fixed twice and could be fixed once.
    ///
    /// The rule is namespace-stripped EXACT match, not a suffix test. Records
    /// genuinely carry an optional `<namespace>:` prefix — `git:alice@…` from
    /// git ingest, `agent:alice@…` from a session with `--engineer`, and bare
    /// emails after `rewrite_principals` — so plain equality would break real
    /// members. But `ends_with(caller)` compares no delimiter, and two ways:
    ///
    ///   REPRODUCED: caller `ob@example.com` sees `agent:bob@example.com`,
    ///   because "…bob@example.com".ends_with("ob@example.com") is true.
    ///
    ///   REPRODUCED, worse: caller `""` sees EVERY record, because
    ///   `str::ends_with("")` is true for every string. An empty `--as` or
    ///   `AXON_PRINCIPAL=` granted a non-admin the whole ledger — 2 of 2
    ///   records where the legitimate member saw 1.
    ///
    /// Splitting on the FIRST colon matches the grammar the rest of the crate
    /// already assumes when it strips prefixes for display, and it keeps
    /// `signal:session:…` intact as one identifier rather than re-splitting it.
    fn owns(principal: &str, caller: &str) -> bool {
        // An empty caller is not an identity. Handled here rather than at the
        // call sites so no future caller can reintroduce the hole.
        if caller.trim().is_empty() {
            return false;
        }
        if principal == caller {
            return true;
        }
        match principal.split_once(':') {
            Some((_ns, ident)) => ident == caller,
            None => false,
        }
    }

    /// Filter `records` by what `caller` is allowed to see.
    ///
    /// - If RBAC is disabled (no admins configured): all records visible.
    /// - If caller is an admin: all records visible.
    /// - Otherwise: only records the caller owns, per [`RbacConfig::owns`].
    /// A SECOND filter, applied on top of an already-filtered handle.
    ///
    /// Its one caller (`search` in main.rs) reads through `Store::open_as`,
    /// whose `all()` has already applied `filter_owned` — so by the time
    /// records arrive here they are filtered, and mutating this function's
    /// admin bypass has NO observable effect through any surface. Verified by
    /// mutation: reverting this bypass to the claimed identity leaves the
    /// whole suite green, while the same mutation to `filter_owned` is caught
    /// immediately.
    ///
    /// It is corrected to use `authority` for consistency, not because a hole
    /// was reachable through it. Recorded here so the next reader does not
    /// mistake untested for untried, and so that if this ever gains a caller
    /// with an unfiltered handle, the bypass is already right.
    pub fn filter_visible<'a>(
        &self,
        records: Vec<&'a LedgerRecord>,
        caller: Option<&str>,
        authority: &Authority,
    ) -> Vec<&'a LedgerRecord> {
        // RBAC disabled: neither list configured. Testing only `admins` meant
        // a ledger listing only `authenticated_admins` — exactly what the
        // refusal message tells an operator to write — read as unconfigured,
        // and every caller saw everything.
        if !self.gate_is_armed() {
            return records;
        }
        if authority.is_admin(self) {
            return records;
        }
        let Some(caller) = caller else {
            // No caller identity — return only untagged / anonymous records
            return records
                .into_iter()
                .filter(|r| r.principal == "unknown" || r.principal == "root")
                .collect();
        };
        // Member: filter to their own records
        records
            .into_iter()
            .filter(|r| Self::owns(&r.principal, caller))
            .collect()
    }

    /// Owned-record variant of filter_visible.
    /// Filter to what `caller` may see.
    ///
    /// TWO SEPARATE QUESTIONS, and only one of them may trust the claim.
    /// "Which records are MINE" narrows the view and is fine to answer from a
    /// claimed name. "May I see EVERYONE's" is admin authority and must come
    /// from `authority`. REPRODUCED before this split: `--as
    /// alice@example.com stats` reported 2 records to a caller entitled to 1,
    /// and the same through `$AXON_PRINCIPAL`, the MCP `ledger_stats` tool
    /// and axon-signal. The write verbs had been moved to authenticated
    /// identity and the read bypass had not.
    pub fn filter_owned(
        &self,
        records: Vec<LedgerRecord>,
        caller: Option<&str>,
        authority: &Authority,
    ) -> Vec<LedgerRecord> {
        if !self.gate_is_armed() {
            return records;
        }
        if authority.is_admin(self) {
            return records;
        }
        let Some(caller) = caller else {
            return records
                .into_iter()
                .filter(|r| r.principal == "unknown" || r.principal == "root")
                .collect();
        };
        records
            .into_iter()
            .filter(|r| Self::owns(&r.principal, caller))
            .collect()
    }
}

/// Determine the caller's identity: --as flag > AXON_PRINCIPAL env var > None.
pub fn resolve_caller(as_flag: Option<&str>) -> Option<String> {
    if let Some(a) = as_flag {
        return Some(a.to_string());
    }
    std::env::var("AXON_PRINCIPAL").ok()
}

// ── Authenticated identity ──────────────────────────────────────────────────
//
// THE INVARIANT: a privileged decision must be made from a principal the
// calling process cannot choose for itself; a caller-supplied identity string
// may name who is CLAIMED, never what is AUTHORIZED.
//
// [`resolve_caller`] above returns exactly what the caller asked to be called:
// `--as <anything>`, or `$AXON_PRINCIPAL`. Both are set by the caller, so
// neither can carry authority. REPRODUCED before this existed, on a
// two-principal ledger with `admins = ["alice@example.com"]`: any caller
// running `--as alice@example.com prune --older-than 2099-01-01 --yes`
// destroyed every record, and `--as alice@example.com diff --json` returned
// every principal's payload.

/// An identity established by the operating system rather than asserted by the
/// caller.
///
/// Constructed only by [`authenticated_principal`], so a `&str` from a flag
/// cannot be passed where one of these is required — the type is the
/// enforcement, not a convention someone has to remember.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedPrincipal(String);

impl AuthenticatedPrincipal {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AuthenticatedPrincipal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The OS identity of the process, from the real uid.
///
/// `getuid()` is the one identity here the process cannot change for itself:
/// an unprivileged process cannot become another uid without going through a
/// mechanism that authenticates first. `$USER` and `$LOGNAME` are NOT
/// equivalent — a caller sets those freely (`env USER=alice axon-ledger …`),
/// which would make them exactly as forgeable as the `--as` flag this
/// replaces.
///
/// Resolves the uid to a passwd name, falling back to `uid:<n>` when there is
/// no passwd entry (minimal containers). Both forms are stable and
/// unforgeable, and either may be listed in `authenticated_admins`.
pub fn authenticated_principal() -> AuthenticatedPrincipal {
    // SAFETY: getuid() is always successful and takes no arguments.
    let uid = unsafe { libc::getuid() };
    if let Some(name) = passwd_name_for_uid(uid) {
        return AuthenticatedPrincipal(name);
    }
    AuthenticatedPrincipal(format!("uid:{uid}"))
}

/// Look up a uid in `/etc/passwd`. Parsed directly rather than through
/// `getpwuid_r` to keep the unsafe surface to the single `getuid()` call.
fn passwd_name_for_uid(uid: u32) -> Option<String> {
    let text = std::fs::read_to_string("/etc/passwd").ok()?;
    for line in text.lines() {
        let mut f = line.split(':');
        let name = f.next()?;
        let _passwd = f.next()?;
        let entry_uid: u32 = f.next()?.parse().ok()?;
        if entry_uid == uid && !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

/// Whether the explicit development-only impersonation escape is armed.
///
/// A caller-asserted identity may carry authority ONLY when the operator has
/// deliberately turned this on. It is off unless the variable is exactly `1`,
/// so production is the default and the escape cannot be entered by accident,
/// by a typo, or by an empty value.
pub const DEV_IMPERSONATE_VAR: &str = "AXON_LEDGER_DEV_IMPERSONATE";

pub fn dev_impersonation_armed() -> bool {
    std::env::var(DEV_IMPERSONATE_VAR).ok().as_deref() == Some("1")
}

/// How a privileged decision should be made for this invocation.
#[derive(Debug, Clone)]
pub enum Authority {
    /// Authority comes from the OS identity. The normal case.
    Authenticated(AuthenticatedPrincipal),
    /// The operator armed [`DEV_IMPERSONATE_VAR`], so the claimed identity is
    /// treated as authoritative. Carries the real OS identity too, because an
    /// impersonated action still has someone actually performing it.
    Impersonated {
        claimed: String,
        real: AuthenticatedPrincipal,
    },
}

impl Authority {
    /// Resolve for this process. `claim` is the caller-supplied `--as` value.
    pub fn resolve(claim: Option<&str>) -> Authority {
        let real = authenticated_principal();
        match claim {
            Some(c) if dev_impersonation_armed() => Authority::Impersonated {
                claimed: c.to_string(),
                real,
            },
            _ => Authority::Authenticated(real),
        }
    }

    /// Does this authority hold admin rights under `rbac`?
    pub fn is_admin(&self, rbac: &RbacConfig) -> bool {
        match self {
            Authority::Authenticated(p) => rbac.is_admin_authenticated(p),
            // Under the armed dev escape the claim is what is being tested —
            // that is the entire purpose of the escape — but the real OS
            // identity still counts, so an actual admin need not impersonate.
            Authority::Impersonated { claimed, real } => {
                rbac.is_admin(claimed) || rbac.is_admin_authenticated(real)
            }
        }
    }

    /// The identity to ATTRIBUTE actions to: what the caller asked to be
    /// called when impersonating, otherwise the OS identity.
    pub fn attributed_name(&self) -> &str {
        match self {
            Authority::Authenticated(p) => p.as_str(),
            Authority::Impersonated { claimed, .. } => claimed,
        }
    }

    /// The identity that ACTUALLY performed the action, always the OS one.
    /// Distinct from [`Authority::attributed_name`] so an impersonated write
    /// still records who really made it.
    pub fn real_name(&self) -> &str {
        match self {
            Authority::Authenticated(p) => p.as_str(),
            Authority::Impersonated { real, .. } => real.as_str(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Effect;
    use serde_json::json;
    use tempfile::tempdir;

    /// A claim with NO authenticated authority — what an ordinary caller has.
    ///
    /// These tests used to pass a bare `&str` and the admin bypass keyed off
    /// it, so `test_admin_sees_all` asserted that CLAIMING an admin's name
    /// showed everything. That is the behaviour that was removed; the test is
    /// now split into the claim (sees only its own) and the authenticated
    /// admin (sees all).
    fn claim_only() -> Authority {
        Authority::Impersonated {
            claimed: "\u{0}not-an-admin".to_string(),
            real: AuthenticatedPrincipal("\u{0}not-an-admin".to_string()),
        }
    }

    /// An authority that IS the named authenticated admin.
    fn authenticated_as(who: &str) -> Authority {
        Authority::Authenticated(AuthenticatedPrincipal(who.to_string()))
    }

    fn make_record(principal: &str) -> LedgerRecord {
        LedgerRecord {
            id: format!("id-{}", principal),
            principal: principal.to_string(),
            effect: Effect::AgentSession,
            causal_parent: None,
            ts_ms: 1000,
            payload: json!({}),
            repo: None,
            recorded_by: None,
        }
    }

    #[test]
    fn test_rbac_disabled_shows_all() {
        let config = RbacConfig::default();
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("agent:bob@example.com");
        let visible = config.filter_owned(vec![r1, r2], Some("alice@example.com"), &claim_only());
        assert_eq!(visible.len(), 2); // RBAC off
    }

    #[test]
    fn test_authenticated_admin_sees_all() {
        let config = RbacConfig {
            authenticated_admins: vec!["alice-os".to_string()],
            admins: vec!["alice@example.com".to_string()],
        };
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("agent:bob@example.com");
        let visible = config.filter_owned(vec![r1, r2], None, &authenticated_as("alice-os"));
        assert_eq!(visible.len(), 2);
    }

    /// The other half of the split: CLAIMING an admin's name shows only what
    /// that name owns. This is the behaviour change — the bypass moved to
    /// authenticated identity.
    #[test]
    fn test_claimed_admin_does_not_see_all() {
        let config = RbacConfig {
            authenticated_admins: vec!["someone-else".to_string()],
            admins: vec!["alice@example.com".to_string()],
        };
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("agent:bob@example.com");
        let visible = config.filter_owned(vec![r1, r2], Some("alice@example.com"), &claim_only());
        assert_eq!(visible.len(), 1, "a claimed admin must not get the bypass");
        assert_eq!(visible[0].principal, "agent:alice@example.com");
    }

    #[test]
    fn test_member_sees_only_own_records() {
        let config = RbacConfig {
            authenticated_admins: vec![],
            admins: vec!["admin@example.com".to_string()],
        };
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("agent:bob@example.com");
        let visible = config.filter_owned(vec![r1, r2], Some("alice@example.com"), &claim_only());
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].principal, "agent:alice@example.com");
    }

    #[test]
    fn test_no_caller_sees_anonymous_only() {
        let config = RbacConfig {
            authenticated_admins: vec![],
            admins: vec!["admin@example.com".to_string()],
        };
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("unknown");
        let visible = config.filter_owned(vec![r1, r2], None, &claim_only());
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].principal, "unknown");
    }

    #[test]
    fn test_load_save_roundtrip() {
        let dir = tempdir().unwrap();
        let mut config = RbacConfig::default();
        config.add_admin("alice@example.com");
        config.save(dir.path()).unwrap();
        let loaded = RbacConfig::load(dir.path()).unwrap();
        assert!(loaded.is_admin("alice@example.com"));
        assert!(!loaded.is_admin("bob@example.com"));
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let dir = tempdir().unwrap();
        let config = RbacConfig::load(dir.path()).unwrap();
        assert!(config.admins.is_empty());
    }
}
#[cfg(test)]
mod identity_boundary_tests {
    /// Same helper as the sibling module: a claim carrying no authenticated
    /// authority, which is what an ordinary caller has.
    fn claim_only() -> super::Authority {
        super::Authority::Impersonated {
            claimed: "\u{0}not-an-admin".to_string(),
            real: super::AuthenticatedPrincipal("\u{0}not-an-admin".to_string()),
        }
    }

    use super::*;
    use crate::model::{Effect, LedgerRecord};

    fn rec(principal: &str) -> LedgerRecord {
        LedgerRecord {
            id: format!("r-{principal}"),
            principal: principal.to_string(),
            effect: Effect::GitCommit,
            causal_parent: None,
            ts_ms: 1,
            payload: serde_json::json!({}),
            repo: None,
            recorded_by: None,
        }
    }

    fn active() -> RbacConfig {
        // RBAC is INERT with no admins configured, so every test here must
        // configure one or it would pass vacuously against any predicate.
        let mut c = RbacConfig::default();
        c.add_admin("admin@example.com");
        c
    }

    /// REPRODUCED before the fix: "agent:bob@example.com".ends_with(
    /// "ob@example.com") is true, so a caller named `ob@example.com` read
    /// bob's records. Verified against the CLI: `--as ob@example.com stats`
    /// reported 1 record.
    #[test]
    fn a_suffix_collision_is_not_ownership() {
        let c = active();
        let recs = vec![rec("agent:bob@example.com")];
        let seen = c.filter_owned(recs, Some("ob@example.com"), &claim_only());
        assert!(
            seen.is_empty(),
            "a caller whose name is a SUFFIX of another principal must own nothing"
        );
    }

    /// REPRODUCED before the fix, and worse than the suffix case:
    /// `str::ends_with("")` is true for EVERY string, so an empty `--as` or
    /// `AXON_PRINCIPAL=` granted a non-admin the entire ledger. Verified
    /// against the CLI: `--as ""` reported 2 of 2 records where the legitimate
    /// member saw 1.
    #[test]
    fn an_empty_caller_owns_nothing_rather_than_everything() {
        let c = active();
        for caller in ["", "   ", "\t"] {
            let recs = vec![rec("agent:alice@example.com"), rec("git:bob@example.com")];
            let seen = c.filter_owned(recs, Some(caller), &claim_only());
            assert!(
                seen.is_empty(),
                "caller {caller:?} must own nothing; an empty identity is not an identity"
            );
        }
    }

    /// The legitimate case the suffix match existed to serve. A strict
    /// whole-string equality fix would have broken this, which is why the rule
    /// is namespace-stripped equality rather than plain `==`.
    #[test]
    fn a_member_owns_every_namespace_form_of_their_own_identity() {
        let c = active();
        let recs = vec![
            rec("agent:bob@example.com"),
            rec("git:bob@example.com"),
            rec("bob@example.com"),
            rec("agent:alice@example.com"),
        ];
        let seen = c.filter_owned(recs, Some("bob@example.com"), &claim_only());
        assert_eq!(
            seen.len(),
            3,
            "bob must own his agent:, git: and bare records, and only those: {:?}",
            seen.iter().map(|r| &r.principal).collect::<Vec<_>>()
        );
    }

    /// The mirror of the suffix hole: a fix that special-cased suffixes while
    /// still matching substrings would pass the test above and fail this one.
    #[test]
    fn a_prefix_collision_is_not_ownership() {
        let c = active();
        let recs = vec![rec("agent:bob@example.com.evil.test")];
        let seen = c.filter_owned(recs, Some("bob@example.com"), &claim_only());
        assert!(
            seen.is_empty(),
            "a principal merely CONTAINING the caller must not be owned by them"
        );
    }

    /// Control: the fix must not be "nobody owns anything".
    #[test]
    fn an_admin_still_sees_everything_and_a_member_still_sees_their_own() {
        let mut c = active();
        // The admin half needs an AUTHENTICATED authority now. It previously
        // passed a claimed "admin@example.com" and asserted 2 — which is the
        // behaviour that was removed, so the assertion had to move with it
        // rather than be deleted.
        c.add_authenticated_admin("admin-os");
        let all = || vec![rec("agent:alice@example.com"), rec("git:bob@example.com")];
        assert_eq!(
            c.filter_owned(
                all(),
                Some("admin@example.com"),
                &Authority::Authenticated(AuthenticatedPrincipal("admin-os".to_string()))
            )
            .len(),
            2
        );
        // And the claim alone does not.
        assert_eq!(
            c.filter_owned(all(), Some("admin@example.com"), &claim_only())
                .len(),
            0,
            "a claimed admin must see only what that name owns, which is nothing here"
        );
        assert_eq!(
            c.filter_owned(all(), Some("bob@example.com"), &claim_only())
                .len(),
            1
        );
    }
}
