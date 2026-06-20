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

    /// Filter `records` by what `caller` is allowed to see.
    ///
    /// - If RBAC is disabled (no admins configured): all records visible.
    /// - If caller is an admin: all records visible.
    /// - Otherwise: only records whose `principal` ends with the caller email
    ///   (e.g. "agent:alice@example.com" or "git:alice@example.com").
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
            .filter(|r| {
                r.principal.ends_with(caller)
                    || r.principal == caller
            })
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
            .filter(|r| r.principal.ends_with(caller) || r.principal == caller)
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
    use serde_json::json;
    use tempfile::tempdir;
    use crate::model::Effect;

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
        let config = RbacConfig { admins: vec!["alice@example.com".to_string()] };
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("agent:bob@example.com");
        let visible = config.filter_owned(vec![r1, r2], Some("alice@example.com"));
        assert_eq!(visible.len(), 2);
    }

    #[test]
    fn test_member_sees_only_own_records() {
        let config = RbacConfig { admins: vec!["admin@example.com".to_string()] };
        let r1 = make_record("agent:alice@example.com");
        let r2 = make_record("agent:bob@example.com");
        let visible = config.filter_owned(vec![r1, r2], Some("alice@example.com"));
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].principal, "agent:alice@example.com");
    }

    #[test]
    fn test_no_caller_sees_anonymous_only() {
        let config = RbacConfig { admins: vec!["admin@example.com".to_string()] };
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
