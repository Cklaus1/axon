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

    pub fn is_admin(&self, email: &str) -> bool {
        self.admins.iter().any(|a| a == email)
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
    pub fn filter_visible<'a>(
        &self,
        records: Vec<&'a LedgerRecord>,
        caller: Option<&str>,
    ) -> Vec<&'a LedgerRecord> {
        // RBAC disabled: no admins configured
        if self.admins.is_empty() {
            return records;
        }
        let Some(caller) = caller else {
            // No caller identity — return only untagged / anonymous records
            return records
                .into_iter()
                .filter(|r| r.principal == "unknown" || r.principal == "root")
                .collect();
        };
        if self.is_admin(caller) {
            return records;
        }
        // Member: filter to their own records
        records
            .into_iter()
            .filter(|r| Self::owns(&r.principal, caller))
            .collect()
    }

    /// Owned-record variant of filter_visible.
    pub fn filter_owned(
        &self,
        records: Vec<LedgerRecord>,
        caller: Option<&str>,
    ) -> Vec<LedgerRecord> {
        if self.admins.is_empty() {
            return records;
        }
        let Some(caller) = caller else {
            return records
                .into_iter()
                .filter(|r| r.principal == "unknown" || r.principal == "root")
                .collect();
        };
        if self.is_admin(caller) {
            return records;
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Effect;
    use serde_json::json;
    use tempfile::tempdir;

    fn make_record(principal: &str) -> LedgerRecord {
        LedgerRecord {
            id: format!("id-{}", principal),
            principal: principal.to_string(),
            effect: Effect::AgentSession,
            causal_parent: None,
            ts_ms: 1000,
            payload: json!({}),
            repo: None,
        }
    }

    #[test]
    fn test_rbac_disabled_shows_all() {
        let config = RbacConfig::default();
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("agent:bob@example.com");
        let visible = config.filter_owned(vec![r1, r2], Some("alice@example.com"));
        assert_eq!(visible.len(), 2); // RBAC off
    }

    #[test]
    fn test_admin_sees_all() {
        let config = RbacConfig {
            admins: vec!["alice@example.com".to_string()],
        };
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("agent:bob@example.com");
        let visible = config.filter_owned(vec![r1, r2], Some("alice@example.com"));
        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn test_member_sees_only_own_records() {
        let config = RbacConfig {
            admins: vec!["admin@example.com".to_string()],
        };
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("agent:bob@example.com");
        let visible = config.filter_owned(vec![r1, r2], Some("alice@example.com"));
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].principal, "agent:alice@example.com");
    }

    #[test]
    fn test_no_caller_sees_anonymous_only() {
        let config = RbacConfig {
            admins: vec!["admin@example.com".to_string()],
        };
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("unknown");
        let visible = config.filter_owned(vec![r1, r2], None);
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
        let seen = c.filter_owned(recs, Some("ob@example.com"));
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
            let seen = c.filter_owned(recs, Some(caller));
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
        let seen = c.filter_owned(recs, Some("bob@example.com"));
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
        let seen = c.filter_owned(recs, Some("bob@example.com"));
        assert!(
            seen.is_empty(),
            "a principal merely CONTAINING the caller must not be owned by them"
        );
    }

    /// Control: the fix must not be "nobody owns anything".
    #[test]
    fn an_admin_still_sees_everything_and_a_member_still_sees_their_own() {
        let c = active();
        let all = || vec![rec("agent:alice@example.com"), rec("git:bob@example.com")];
        assert_eq!(c.filter_owned(all(), Some("admin@example.com")).len(), 2);
        assert_eq!(c.filter_owned(all(), Some("bob@example.com")).len(), 1);
    }
}
