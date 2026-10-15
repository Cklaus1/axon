//! R22 handoff (R22 §3.5) — verify an ApprovalToken at the run boundary.
//!
//! axon-intent produces a content-binding `<name>.approval` token; this is the
//! enforcement side: axon-os recomputes the digests from the ACTUAL program
//! source + grant (plus the token's own metadata) and refuses to run a job whose
//! program or grant was edited after approval, or whose decision wasn't
//! "approved". A one-byte edit to either invalidates the token.
//!
//! `canonical_grant` here is the SINGLE SOURCE of the grant encoding — axon-intent
//! delegates to it — so the two stacks can never drift on the digest.

use crate::grant::Grant;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::path::Path;

const UNIT: char = '\u{1f}';

fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{:x}", h.finalize())
}

/// The operator approval key: hex, at least 16 bytes. A DEDICATED variable,
/// not `AXON_ATTEST_KEY`: that key belongs to `axon-vm`, which falls back to an
/// ephemeral one, and reusing it would silently flip axon-os to refusing for
/// any operator who set it for axon-vm. There is deliberately no ephemeral
/// fallback here either: a per-process key cannot verify what a previous
/// process wrote.
///
/// Threat model: the key is SYMMETRIC, so every holder — the approving operator
/// and the enforcing host alike — can mint tokens and seal records. It proves
/// "made by a key holder", not "made by this approver": `approved_by` stays
/// unauthenticated (per-approver signatures are out of scope).
///
/// Setting the key is an opt-in that invalidates unkeyed artifacts: a keyed
/// verifier refuses unkeyed records (downgrade protection), so a run archived
/// before the key was set can no longer be verified or replayed under it.
pub const APPROVAL_KEY_ENV: &str = "AXON_APPROVAL_KEY";

/// The two subkeys derived from the operator approval key, one per use, so a
/// MAC made for a token is never valid as a record seal or vice versa:
/// `HMAC-SHA256(op_key, "axon-approval token v1")` and
/// `HMAC-SHA256(op_key, "axon-os record v1")`. `Debug` is redacted.
#[derive(Clone)]
pub struct ApprovalKey {
    token: [u8; 32],
    record: [u8; 32],
}

impl std::fmt::Debug for ApprovalKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ApprovalKey(<redacted>)")
    }
}

impl ApprovalKey {
    /// Derive both subkeys from operator key bytes (>= 16 bytes).
    pub fn derive(operator_key: &[u8]) -> Result<ApprovalKey, String> {
        if operator_key.len() < 16 {
            return Err(format!(
                "{APPROVAL_KEY_ENV} is shorter than 16 bytes: refusing to key approvals with it"
            ));
        }
        Ok(ApprovalKey {
            token: axon_attest::hmac_sha256(operator_key, b"axon-approval token v1"),
            record: axon_attest::hmac_sha256(operator_key, b"axon-os record v1"),
        })
    }

    /// `HMAC-SHA256(token_subkey, data)` — the approval-token MAC.
    pub fn token_mac(&self, data: &[u8]) -> [u8; 32] {
        axon_attest::hmac_sha256(&self.token, data)
    }

    /// `HMAC-SHA256(record_subkey, data)` — the run-record seal MAC.
    pub fn record_mac(&self, data: &[u8]) -> [u8; 32] {
        axon_attest::hmac_sha256(&self.record, data)
    }

    /// The key from [`APPROVAL_KEY_ENV`]: unset or blank ⇒ `None` (unkeyed);
    /// set but not hex, or shorter than 16 bytes ⇒ `Err`, never a silent fall
    /// back to unkeyed.
    pub fn from_env() -> Result<Option<ApprovalKey>, String> {
        Self::from_value(std::env::var(APPROVAL_KEY_ENV))
    }

    fn from_value(v: Result<String, std::env::VarError>) -> Result<Option<ApprovalKey>, String> {
        match v {
            Ok(v) if !v.trim().is_empty() => {
                let bytes = decode_hex(v.trim())
                    .ok_or_else(|| format!("{APPROVAL_KEY_ENV} is not valid hex"))?;
                ApprovalKey::derive(&bytes).map(Some)
            }
            Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                Err(format!("{APPROVAL_KEY_ENV} is not valid hex"))
            }
        }
    }
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The token schema `axon-intent approve` emits: `axon-approval/3` binds the
/// program's import closure (`imports_digest`) beside the entry file and the
/// grant. `/2` is reserved by R24 (multi-party approval).
pub const TOKEN_SCHEMA: &str = "axon-approval/3";

/// The pre-import-binding schema. Its `axtok1:` digest is unkeyed and binds no
/// imports, so it is refused under a key and, without one, accepted only for a
/// program whose import closure is empty (see [`authorize`]).
const LEGACY_TOKEN_SCHEMA: &str = "axon-approval/1";

/// The token schemas axon-os knows how to verify. Anything else is refused: a
/// field the verifier does not know the meaning of is not evidence.
const KNOWN_TOKEN_SCHEMAS: &[&str] = &[TOKEN_SCHEMA, LEGACY_TOKEN_SCHEMA];

/// The `imports_digest` of a program that imports nothing (`axon imports
/// --json` of an import-free file): SHA-256 of the empty string, as axon-core's
/// `lockfile::imports_digest(&[])` defines it.
pub const EMPTY_IMPORTS_DIGEST: &str =
    "axsha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

/// The fields an `axon-approval/3` token binds, in their serialized (JSON)
/// form. THE token formula: `axon-intent` mints with it and axon-os verifies
/// with it, so the two can never drift.
pub struct TokenFields<'a> {
    pub program_digest: &'a str,
    pub grant_digest: &'a str,
    pub imports_digest: &'a str,
    pub approved_by: &'a str,
    pub decision: &'a str,
    pub risk: &'a str,
}

impl TokenFields<'_> {
    /// `schema ␟ pd ␟ gd ␟ imports ␟ by ␟ decision ␟ risk`.
    fn canonical(&self) -> String {
        format!(
            "{TOKEN_SCHEMA}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}",
            self.program_digest,
            self.grant_digest,
            self.imports_digest,
            self.approved_by,
            self.decision,
            self.risk
        )
    }

    /// The token digest: keyed `axmac1:` + hex HMAC-SHA256 under the token
    /// subkey of [`ApprovalKey`]; unkeyed `axtok3:` + hex SHA-256. Both over
    /// the same canonical fields.
    pub fn digest(&self, key: Option<&ApprovalKey>) -> String {
        let canon = self.canonical();
        match key {
            Some(k) => format!("axmac1:{}", hex_lower(&k.token_mac(canon.as_bytes()))),
            None => format!("axtok3:{}", sha256_hex(canon.as_bytes())),
        }
    }
}

fn hex_lower(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The canonical, semantic encoding of a grant (fixed field order; formatting
/// cannot change the digest). The one definition both axon-os and axon-intent use.
///
/// EXHAUSTIVE DESTRUCTURING IS THE POINT. A struct pattern without `..` must
/// bind every field, so adding a field to `Grant` is a COMPILE ERROR here until
/// it is encoded or explicitly bound with a stated reason. `..` must never be
/// introduced; that is the whole guarantee.
///
/// It was previously a `format!` over named fields, which encoded 6 of 7 and
/// silently omitted `reproducible`. REPRODUCED: two manifests differing ONLY by
/// `profile = "hermetic"` vs `"restricted"`, one hand-minted token copied
/// byte-identically to both, and BOTH printed "Approval: required and verified
/// (program + grant unedited since sign-off)". The restricted run then loaded
/// and executed operator-ambient code through `AXON_PATH` — a module search
/// path, i.e. a code-injection channel — which `runtime.rs` strips under the
/// hermetic posture along with AXON_AUDIT_LEDGER, AXON_AI_REPLAY and
/// AXON_AI_MOCK.
///
/// The omission was justified in `grant.rs` as "it constrains HOW a run
/// executes, not WHAT it may touch". That is contradicted by `Grant::intersect`
/// in the same file, which ORs `reproducible` precisely because taking the AND
/// "would let a broad supervisor grant relax a hermetic job, which is the
/// direction an intersection must never go". A field an intersection treats as
/// authority is authority, and the AXON_PATH channel settles it: it governs
/// what code the job may LOAD.
///
/// SCHEMA BUMP, deliberately breaking. Including the field changes every
/// existing `grant_digest`, so tokens in flight stop verifying. A stable digest
/// over an incomplete statement is worth less than a breaking change, and
/// accepting `axon-approval/1` as legacy would leave the hole reachable by
/// downgrade — the same reasoning that makes the intersection OR rather than
/// AND.
pub fn canonical_grant(g: &Grant) -> String {
    let Grant {
        fs_read,
        fs_write,
        net,
        exec,
        max_label,
        budget,
        reproducible,
    } = g;
    format!(
        "fs_read={}{UNIT}fs_write={}{UNIT}net={}{UNIT}exec={}{UNIT}max_label={}{UNIT}\
         budget={},{},{}{UNIT}reproducible={}",
        fs_read.join(","),
        fs_write.join(","),
        net.join(","),
        exec.as_str(),
        max_label.as_str(),
        budget.calls,
        budget.tokens,
        budget.cost_micro,
        reproducible,
    )
}

/// The subset of an approval token axon-os needs to verify (structural JSON
/// deserialization — no shared type required with axon-intent).
#[derive(Deserialize)]
struct TokenView {
    schema: String,
    program_digest: String,
    grant_digest: String,
    /// `/3` only; a `/1` token has none.
    #[serde(default)]
    imports_digest: Option<String>,
    approved_by: String,
    decision: String,
    risk: String,
    token_digest: String,
}

/// What a verified token establishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedToken {
    /// `true`: an `axmac1:` token checked under `AXON_APPROVAL_KEY` (made by a
    /// key holder). `false`: an unkeyed digest — integrity only, anyone can
    /// mint one.
    pub keyed: bool,
    /// The import closure the token binds. `None` for a legacy
    /// `axon-approval/1` token, which binds none.
    pub imports_digest: Option<String>,
}

/// Verify an approval token against the actual program source + grant, under
/// the verifier's key (`None` = no `AXON_APPROVAL_KEY`).
///
/// `Ok` iff: the schema is known, decision == "approved", the program digest
/// matches the source, the grant digest matches the grant, AND the token's own
/// digest recomputes from its fields. The KEY decides which digest is
/// acceptable: with a key only `axmac1:` (an unkeyed `axtok3:`/`axtok1:` token
/// is refused — setting the key is the operator's opt-in to refuse what anyone
/// could mint); without one only the unkeyed digest of the token's schema (an
/// `axmac1:` token cannot be checked, so it is refused). The import closure is
/// NOT checked here: the caller hands [`VerifiedToken::imports_digest`] to the
/// process that loads the modules (`AXON_REQUIRE_IMPORTS_DIGEST`).
pub fn verify_approval(
    token_json: &str,
    program_src: &str,
    grant: &Grant,
    key: Option<&ApprovalKey>,
) -> Result<VerifiedToken, String> {
    let t: TokenView =
        serde_json::from_str(token_json).map_err(|e| format!("malformed approval token: {e}"))?;
    if !KNOWN_TOKEN_SCHEMAS.contains(&t.schema.as_str()) {
        return Err(format!(
            "approval token schema `{}` is not one axon-os verifies (known: {})",
            t.schema,
            KNOWN_TOKEN_SCHEMAS.join(", ")
        ));
    }

    if t.decision != "approved" {
        return Err(format!(
            "approval decision is `{}`, not approved",
            t.decision
        ));
    }
    let pd = format!("axsha256:{}", sha256_hex(program_src.as_bytes()));
    if pd != t.program_digest {
        return Err("program was edited after approval (program digest mismatch)".into());
    }
    let gd = format!("axsha256:{}", sha256_hex(canonical_grant(grant).as_bytes()));
    if gd != t.grant_digest {
        return Err("grant was edited after approval (grant digest mismatch)".into());
    }
    let (td, verified) = if t.schema == TOKEN_SCHEMA {
        let imports = t
            .imports_digest
            .as_deref()
            .ok_or("an axon-approval/3 token without an imports_digest binds no import closure")?;
        let fields = TokenFields {
            program_digest: &t.program_digest,
            grant_digest: &t.grant_digest,
            imports_digest: imports,
            approved_by: &t.approved_by,
            decision: &t.decision,
            risk: &t.risk,
        };
        let keyed = t.token_digest.starts_with("axmac1:");
        if key.is_some() && !keyed {
            return Err(format!(
                "unkeyed approval token refused: {APPROVAL_KEY_ENV} is set, so only an `axmac1:` \
                 token minted under it is accepted — re-approve with the key set"
            ));
        }
        if key.is_none() && keyed {
            return Err(format!(
                "keyed approval token (`axmac1:`) cannot be checked: {APPROVAL_KEY_ENV} is not set"
            ));
        }
        let verified = VerifiedToken {
            keyed,
            imports_digest: Some(imports.to_string()),
        };
        (fields.digest(key), verified)
    } else {
        // axon-approval/1: an unkeyed digest that binds no imports.
        if key.is_some() {
            return Err(format!(
                "axon-approval/1 token refused: {APPROVAL_KEY_ENV} is set and a /1 token is \
                 unkeyed — re-approve: this token predates import binding"
            ));
        }
        let canon = format!(
            "{}{UNIT}{}{UNIT}{}{UNIT}{}{UNIT}{}",
            t.program_digest, t.grant_digest, t.approved_by, t.decision, t.risk
        );
        let verified = VerifiedToken {
            keyed: false,
            imports_digest: None,
        };
        (format!("axtok1:{}", sha256_hex(canon.as_bytes())), verified)
    };
    // Internal consistency: the token's own digest must recompute from its
    // fields (under the key, for `axmac1:`), compared in constant time.
    if !axon_attest::constant_time_eq(td.as_bytes(), t.token_digest.as_bytes()) {
        return Err("approval token digest mismatch (tampered metadata)".into());
    }
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grant::{Budget, ExecPolicy, Label};

    fn grant() -> Grant {
        Grant {
            reproducible: false,
            fs_read: vec!["./data/".into()],
            fs_write: vec!["./out/".into()],
            net: vec![],
            exec: ExecPolicy::None,
            max_label: Label::Internal,
            budget: Budget {
                calls: 100,
                tokens: 50000,
                cost_micro: 1000000,
            },
        }
    }

    const LIB_IMPORTS: &str =
        "axsha256:1111111111111111111111111111111111111111111111111111111111111111";

    fn key(byte: u8) -> ApprovalKey {
        ApprovalKey::derive(&[byte; 16]).unwrap()
    }

    // A real `/3` token (the format axon-intent emits), minted through the one
    // formula both crates use.
    fn token_for(program_src: &str, g: &Grant, imports: &str, k: Option<&ApprovalKey>) -> String {
        let pd = format!("axsha256:{}", sha256_hex(program_src.as_bytes()));
        let gd = format!("axsha256:{}", sha256_hex(canonical_grant(g).as_bytes()));
        let (approved_by, decision, risk) = ("alice", "approved", "medium");
        let td = TokenFields {
            program_digest: &pd,
            grant_digest: &gd,
            imports_digest: imports,
            approved_by,
            decision,
            risk,
        }
        .digest(k);
        serde_json::json!({"schema":"axon-approval/3","program_digest":pd,"grant_digest":gd,
            "imports_digest":imports,"approved_by":approved_by,"decision":decision,
            "risk":risk,"token_digest":td})
        .to_string()
    }

    // A legacy `/1` token, hand-minted with the old `axtok1:` formula.
    fn legacy_token_for(program_src: &str, g: &Grant) -> String {
        let pd = format!("axsha256:{}", sha256_hex(program_src.as_bytes()));
        let gd = format!("axsha256:{}", sha256_hex(canonical_grant(g).as_bytes()));
        let (approved_by, decision, risk) = ("alice", "approved", "medium");
        let canon = format!("{pd}{UNIT}{gd}{UNIT}{approved_by}{UNIT}{decision}{UNIT}{risk}");
        let td = format!("axtok1:{}", sha256_hex(canon.as_bytes()));
        format!(
            "{{\"schema\":\"axon-approval/1\",\"program_digest\":\"{pd}\",\"grant_digest\":\"{gd}\",\"approved_by\":\"{approved_by}\",\"decision\":\"{decision}\",\"risk\":\"{risk}\",\"token_digest\":\"{td}\"}}"
        )
    }

    const SRC: &str = "fn main() { 0 }";

    #[test]
    fn approved_unedited_token_verifies() {
        let v = verify_approval(
            &token_for(SRC, &grant(), LIB_IMPORTS, None),
            SRC,
            &grant(),
            None,
        )
        .unwrap();
        assert_eq!(
            v,
            VerifiedToken {
                keyed: false,
                imports_digest: Some(LIB_IMPORTS.into())
            }
        );
        let k = key(7);
        let tok = token_for(SRC, &grant(), LIB_IMPORTS, Some(&k));
        assert!(tok.contains("\"token_digest\":\"axmac1:"), "{tok}");
        let v = verify_approval(&tok, SRC, &grant(), Some(&k)).unwrap();
        assert!(v.keyed);
        assert_eq!(v.imports_digest.as_deref(), Some(LIB_IMPORTS));
    }

    #[test]
    fn edited_program_is_rejected() {
        let tok = token_for(SRC, &grant(), LIB_IMPORTS, None);
        assert!(verify_approval(&tok, "fn main() { 1 }", &grant(), None).is_err());
    }

    #[test]
    fn edited_grant_is_rejected() {
        let tok = token_for(SRC, &grant(), LIB_IMPORTS, None);
        let mut widened = grant();
        widened.net = vec!["evil.com".into()]; // grant edited after approval
        assert!(verify_approval(&tok, SRC, &widened, None).is_err());
    }

    #[test]
    fn rejected_decision_is_refused() {
        let tok = token_for(SRC, &grant(), LIB_IMPORTS, None).replace("approved", "rejected");
        assert!(verify_approval(&tok, SRC, &grant(), None).is_err());
    }

    // G1e: the import closure is bound by the digest. An attacker who rewrote
    // a module and set the token's `imports_digest` to the new closure's is
    // refused, keyed and unkeyed alike.
    #[test]
    fn an_imports_digest_edited_after_the_digest_is_refused() {
        let swapped = "axsha256:2222222222222222222222222222222222222222222222222222222222222222";
        for k in [None, Some(key(7))] {
            let tok =
                token_for(SRC, &grant(), LIB_IMPORTS, k.as_ref()).replace(LIB_IMPORTS, swapped);
            let e = verify_approval(&tok, SRC, &grant(), k.as_ref())
                .expect_err("ATTACK: a token whose imports_digest was rewritten verified");
            assert!(e.contains("token digest mismatch"), "{e}");
        }
        // A /3 token that binds NO closure — minted consistently, the field
        // then absent — is refused, never read as some default closure.
        let no_imports = token_for(SRC, &grant(), EMPTY_IMPORTS_DIGEST, None).replace(
            &format!("\"imports_digest\":\"{EMPTY_IMPORTS_DIGEST}\","),
            "",
        );
        let e = verify_approval(&no_imports, SRC, &grant(), None)
            .expect_err("ATTACK: an axon-approval/3 token binding no import closure verified");
        assert!(e.contains("imports_digest"), "{e}");
    }

    // G5a: the verifier's key decides which digest it accepts.
    #[test]
    fn the_key_decides_which_token_digest_is_accepted() {
        let k = key(7);
        // Forged without the key (unkeyed axtok3), presented to a keyed verifier.
        let unkeyed = token_for(SRC, &grant(), LIB_IMPORTS, None);
        let e = verify_approval(&unkeyed, SRC, &grant(), Some(&k))
            .expect_err("ATTACK: an unkeyed axtok3 token passed a keyed verifier");
        assert!(e.contains("unkeyed approval token refused"), "{e}");
        // Forged under ANOTHER key: an axmac1 the verifier's key did not make.
        let other = token_for(SRC, &grant(), LIB_IMPORTS, Some(&key(9)));
        let e = verify_approval(&other, SRC, &grant(), Some(&k))
            .expect_err("ATTACK: an axmac1 token made under another key verified");
        assert!(e.contains("token digest mismatch"), "{e}");
        // A legacy axtok1 token, presented to a keyed verifier.
        let e = verify_approval(&legacy_token_for(SRC, &grant()), SRC, &grant(), Some(&k))
            .expect_err("ATTACK: an axtok1 token passed a keyed verifier");
        assert!(e.contains("predates import binding"), "{e}");
        // An axmac1 token cannot be checked without the key.
        let keyed = token_for(SRC, &grant(), LIB_IMPORTS, Some(&k));
        let e = verify_approval(&keyed, SRC, &grant(), None)
            .expect_err("an axmac1 token was accepted by a verifier holding no key");
        assert!(e.contains("cannot be checked"), "{e}");
    }

    // `/1`: keyless, it verifies and binds no imports (the caller requires an
    // empty closure); its digest is still checked.
    #[test]
    fn a_legacy_token_verifies_only_keyless_and_binds_no_imports() {
        let v = verify_approval(&legacy_token_for(SRC, &grant()), SRC, &grant(), None).unwrap();
        assert_eq!(
            v,
            VerifiedToken {
                keyed: false,
                imports_digest: None
            }
        );
        let tampered = legacy_token_for(SRC, &grant()).replace("alice", "mallory");
        assert!(verify_approval(&tampered, SRC, &grant(), None).is_err());
    }

    // G5b: a token whose schema the verifier does not know is refused, even
    // when every digest in it is internally consistent.
    #[test]
    fn unknown_token_schema_is_refused() {
        // The attack the schema check alone stops: a token whose digest DOES
        // recompute (the legacy /1 formula binds no schema tag), relabelled
        // with a schema axon-os does not know. Without the check it would be
        // verified under the /1 rules.
        let relabelled =
            legacy_token_for(SRC, &grant()).replace("axon-approval/1", "axon-approval/9");
        let e = verify_approval(&relabelled, SRC, &grant(), None)
            .expect_err("ATTACK: a token of an unknown schema was verified");
        assert!(
            e.contains("axon-approval/9"),
            "the refusal names the schema: {e}"
        );
        let tok = token_for(SRC, &grant(), LIB_IMPORTS, None)
            .replace("axon-approval/3", "axon-approval/9");
        assert!(
            verify_approval(&tok, SRC, &grant(), None).is_err(),
            "a relabelled /3 token must be refused"
        );
        let no_schema = token_for(SRC, &grant(), LIB_IMPORTS, None)
            .replace("\"schema\":\"axon-approval/3\",", "");
        assert!(
            verify_approval(&no_schema, SRC, &grant(), None).is_err(),
            "a token with no schema at all must be refused"
        );
    }

    // Known answer for the token formula, computed independently with
    // Python's hashlib/hmac: canonical = "axon-approval/3␟pd␟gd␟imp␟alice␟
    // approved␟low"; unkeyed = sha256(canonical); keyed = HMAC(HMAC(op_key,
    // "axon-approval token v1"), canonical) with op_key = 00..0f.
    #[test]
    fn token_formula_known_answer() {
        let f = TokenFields {
            program_digest: "axsha256:pd",
            grant_digest: "axsha256:gd",
            imports_digest: "axsha256:imp",
            approved_by: "alice",
            decision: "approved",
            risk: "low",
        };
        assert_eq!(
            f.digest(None),
            "axtok3:5b9abad93af267ec7fea2e73b4946196fe68d2a10731a1ed55a5308a6195938c",
            "signing KAT approval token formula (unkeyed)"
        );
        let k = ApprovalKey::derive(&(0u8..16).collect::<Vec<_>>()).unwrap();
        assert_eq!(
            f.digest(Some(&k)),
            "axmac1:3aae1c71981d089eb185be16616268b2e31a6aebb4388e76cd48c8847fc9c4c6",
            "signing KAT approval token formula (keyed)"
        );
    }

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    // Known answer: op key 00..0f; each subkey is HMAC(op_key, domain), checked
    // through the MAC it keys (HMAC(subkey, "x")), computed independently with
    // Python's hmac module. Pins both domains and that they differ.
    #[test]
    fn approval_key_derivation_known_answer() {
        let k = ApprovalKey::derive(&(0u8..16).collect::<Vec<_>>()).unwrap();
        assert_eq!(
            hex(&k.token_mac(b"x")),
            "8fd45308a38214c9c818302f427aa059121e0a5069b0620de1680be37229ace3",
            "signing KAT approval token subkey derivation label"
        );
        assert_eq!(
            hex(&k.record_mac(b"x")),
            "81d692c315dd0baec42afb3ca84a5c0dc938050c90706da85541609dd0aff7ca",
            "signing KAT approval record subkey derivation label"
        );
        assert_eq!(format!("{k:?}"), "ApprovalKey(<redacted>)");
    }

    #[test]
    fn approval_key_parsing_refuses_malformed_and_short() {
        use std::env::VarError;
        let parse = |s: &str| ApprovalKey::from_value(Ok(s.to_string()));
        assert!(parse("").unwrap().is_none(), "blank ⇒ unkeyed");
        assert!(parse("   ").unwrap().is_none(), "blank ⇒ unkeyed");
        assert!(ApprovalKey::from_value(Err(VarError::NotPresent))
            .unwrap()
            .is_none());
        let e = parse("zz00112233445566778899aabbccddeeff").unwrap_err();
        assert!(e.contains("not valid hex"), "{e}");
        let e = parse("0011223").unwrap_err();
        assert!(e.contains("not valid hex"), "odd length: {e}");
        let e = parse("00112233445566778899aabbccddee").unwrap_err();
        assert!(e.contains("shorter than 16 bytes"), "15 bytes: {e}");
        assert!(parse("00112233445566778899aabbccddeeff").unwrap().is_some());
    }

    /// A job dir holding SRC as the program and `token` (if any) as the
    /// `.approval` sibling; returns (job path, manifest).
    fn job_with(
        tag: &str,
        token: Option<String>,
    ) -> (std::path::PathBuf, crate::manifest::JobManifest) {
        let d = std::env::temp_dir().join(format!("axon-os-authz-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let program = d.join("main.ax");
        std::fs::write(&program, SRC).unwrap();
        let job = d.join("main.axjob");
        let _ = std::fs::remove_file(job.with_extension("approval"));
        if let Some(t) = token {
            std::fs::write(job.with_extension("approval"), t).unwrap();
        }
        let m = crate::manifest::JobManifest {
            require_approval: true,
            program,
            intent: "t".into(),
            seed: 1,
            grant: grant(),
        };
        (job, m)
    }

    fn mock(imports: Result<&str, &str>) -> crate::runtime::MockRuntime {
        let mut rt = crate::runtime::MockRuntime::new(
            crate::gate::DeclaredEffects::unknown(),
            crate::runtime::RunOutcome {
                events: vec![],
                verdict: crate::verdict::Verdict::Completed { value: 0 },
            },
        );
        rt.imports = imports.map(str::to_string).map_err(str::to_string);
        rt
    }

    // G1e: a `/3` token's closure is what the run is required to load —
    // authorize hands it on, without consulting the runtime's closure.
    #[test]
    fn authorize_hands_the_tokens_closure_to_the_run() {
        let (job, m) = job_with("v3", Some(token_for(SRC, &grant(), LIB_IMPORTS, None)));
        let a = authorize(&job, &m, None, &mock(Err("not consulted"))).unwrap();
        assert_eq!(a.status, ApprovalStatus::VerifiedRequired);
        assert_eq!(a.required_imports.as_deref(), Some(LIB_IMPORTS));
        let k = key(7);
        let (job, m) = job_with("v3k", Some(token_for(SRC, &grant(), LIB_IMPORTS, Some(&k))));
        let a = authorize(&job, &m, Some(&k), &mock(Err("not consulted"))).unwrap();
        assert!(a.token.unwrap().keyed);
        assert_eq!(a.required_imports.as_deref(), Some(LIB_IMPORTS));
    }

    // The /1 rule: keyless, a legacy token is accepted only for a program
    // whose closure the job's interpreter shows to be empty — and the run is
    // pinned to that empty closure. An importing program, an undeterminable
    // closure, or any key: refused, naming re-approval.
    #[test]
    fn a_legacy_token_is_authorized_only_for_an_import_free_program() {
        let (job, m) = job_with("v1", Some(legacy_token_for(SRC, &grant())));
        let a = authorize(&job, &m, None, &mock(Ok(EMPTY_IMPORTS_DIGEST))).unwrap();
        assert_eq!(a.required_imports.as_deref(), Some(EMPTY_IMPORTS_DIGEST));
        for (what, rt, k) in [
            ("an importing program", mock(Ok(LIB_IMPORTS)), None),
            ("an undeterminable closure", mock(Err("boom")), None),
            (
                "a keyed check",
                mock(Ok(EMPTY_IMPORTS_DIGEST)),
                Some(key(7)),
            ),
        ] {
            let e = match authorize(&job, &m, k.as_ref(), &rt) {
                Ok(a) => panic!("ATTACK: a /1 token was authorized for {what}: {a:?}"),
                Err(e) => e,
            };
            assert!(
                e.contains("re-approve: this token predates import binding"),
                "{what}: the refusal names re-approval: {e}"
            );
        }
    }
}

/// The three authorization outcomes, recorded so a reader can tell them apart
/// afterward. `RunRecord` previously carried no approval field at all, so an
/// unapproved execution and an approved one archived identically.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalStatus {
    /// The job does not require sign-off and no token was present.
    NotRequired,
    /// A valid token was present; the job required it.
    VerifiedRequired,
    /// A valid token was present; the job did not require it.
    VerifiedNotRequired,
    /// Authorization FAILED — the token was missing when required, or present
    /// and invalid. Distinct from `NotRequired`, which the denial branch used
    /// to stamp: the record of a run refused for want of sign-off then said
    /// the job did not need sign-off, contradicting the verdict beside it.
    Denied,
    /// The run was refused BEFORE authorization ran (the isolation check), so
    /// no approval decision exists. Distinct from `unknown`, the default for a
    /// record built outside the supervisor.
    NotChecked,
}

impl ApprovalStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            ApprovalStatus::NotRequired => "not_required",
            ApprovalStatus::VerifiedRequired => "verified_required",
            ApprovalStatus::VerifiedNotRequired => "verified_not_required",
            ApprovalStatus::Denied => "denied",
            ApprovalStatus::NotChecked => "not_checked",
        }
    }
}

/// Authorize a job for execution, from the job file's own `.approval` sibling.
///
/// THE SINGLE AUTHORIZATION SITE. This logic lived in `cmd_run`, so `axon-os
/// replay` — which reaches `supervisor::run` by a different route — executed
/// without it. REPRODUCED: a stored job marked `require_approval = true` with
/// no token anywhere was refused by `run` with exit 8 and no side effect, and
/// re-executed by `replay` with exit 0, the side-effect file's mtime advancing.
/// The public `axon_os::supervise` re-export was a third route with no gate at
/// all.
///
/// It is called from inside `supervisor::run` — the point every execution path
/// converges on, and which already hosts the one check (`gate::admit`) that no
/// caller can skip. A check in a CALLER is opt-in per call site; a check here
/// is not.
///
/// The token's import closure travels on in [`Authorization::required_imports`]
/// to the process that loads the modules (G1e). A legacy `axon-approval/1`
/// token binds none, so it is accepted only when the program's closure — as
/// the job's own interpreter environment resolves it, [`Runtime::imports_digest`]
/// — is empty, and the run is then pinned to the empty closure.
///
/// [`Runtime::imports_digest`]: crate::runtime::Runtime::imports_digest
pub fn authorize(
    job_path: &Path,
    manifest: &crate::manifest::JobManifest,
    key: Option<&ApprovalKey>,
    rt: &impl crate::runtime::Runtime,
) -> Result<Authorization, String> {
    let approval_path = job_path.with_extension("approval");
    if approval_path.exists() {
        let token = std::fs::read_to_string(&approval_path).unwrap_or_default();
        let program_src = std::fs::read_to_string(&manifest.program).unwrap_or_default();
        // An INVALID token is a failure whether or not policy required one.
        let verified = verify_approval(&token, &program_src, &manifest.grant, key)?;
        let required_imports = match &verified.imports_digest {
            Some(d) => d.clone(),
            None => legacy_closure_is_empty(manifest, rt)?,
        };
        Ok(Authorization {
            status: if manifest.require_approval {
                ApprovalStatus::VerifiedRequired
            } else {
                ApprovalStatus::VerifiedNotRequired
            },
            token: Some(verified),
            required_imports: Some(required_imports),
        })
    } else if manifest.require_approval {
        Err(format!(
            "approval required but missing — this job sets `require_approval = true` \
             and there is no token at {}",
            approval_path.display()
        ))
    } else {
        Ok(Authorization {
            status: ApprovalStatus::NotRequired,
            token: None,
            required_imports: None,
        })
    }
}

/// A legacy `/1` token is as strong as before only for a program that imports
/// nothing (the entry digest then fixes the whole program). Returns the empty
/// closure the run is pinned to, or the refusal.
fn legacy_closure_is_empty(
    manifest: &crate::manifest::JobManifest,
    rt: &impl crate::runtime::Runtime,
) -> Result<String, String> {
    let loaded = rt
        .imports_digest(&manifest.program, &manifest.grant)
        .map_err(|e| {
            format!(
                "re-approve: this token predates import binding (axon-approval/1), and the \
                 program's import closure could not be determined: {e}"
            )
        })?;
    if loaded != EMPTY_IMPORTS_DIGEST {
        return Err(format!(
            "re-approve: this token predates import binding (axon-approval/1) and the program \
             imports modules (imports_digest {loaded})"
        ));
    }
    Ok(loaded)
}

/// The outcome of [`authorize`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    pub status: ApprovalStatus,
    /// The verified token, when one was present.
    pub token: Option<VerifiedToken>,
    /// The `imports_digest` the run must load (`AXON_REQUIRE_IMPORTS_DIGEST`
    /// on the child), when a token was present.
    pub required_imports: Option<String>,
}
