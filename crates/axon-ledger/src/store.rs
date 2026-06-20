use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use thiserror::Error;

use crate::model::{Effect, LedgerRecord};

#[derive(Error, Debug)]
pub enum StoreError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub struct Store {
    events_path: PathBuf,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        fs::create_dir_all(path)?;
        let events_path = path.join("events.ndjson");
        if !events_path.exists() {
            File::create(&events_path)?;
        }
        Ok(Store { events_path })
    }

    pub fn append(&mut self, record: &LedgerRecord) -> Result<()> {
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.events_path)?;
        let line = serde_json::to_string(record)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    pub fn all(&self) -> Result<Vec<LedgerRecord>> {
        let file = File::open(&self.events_path)?;
        let reader = BufReader::new(file);
        let mut records = Vec::new();
        for line in reader.lines() {
            let line = line?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let record: LedgerRecord = serde_json::from_str(trimmed)?;
            records.push(record);
        }
        Ok(records)
    }

    pub fn find_by_id(&self, id: &str) -> Result<Option<LedgerRecord>> {
        let records = self.all()?;
        Ok(records.into_iter().find(|r| r.id == id))
    }

    pub fn find_by_effect(&self, effect: &Effect) -> Result<Vec<LedgerRecord>> {
        let records = self.all()?;
        Ok(records.into_iter().filter(|r| &r.effect == effect).collect())
    }

    /// Filter records to a specific repo name tag.
    /// `None` or an empty filter returns all records (single-repo behaviour).
    pub fn find_by_repo(&self, repo: Option<&str>) -> Result<Vec<LedgerRecord>> {
        let records = self.all()?;
        match repo {
            None => Ok(records),
            Some(r) => Ok(records.into_iter().filter(|rec| rec.repo.as_deref() == Some(r)).collect()),
        }
    }

    pub fn find_by_payload_field(&self, key: &str, value: &str) -> Result<Vec<LedgerRecord>> {
        let records = self.all()?;
        Ok(records
            .into_iter()
            .filter(|r| {
                r.payload
                    .get(key)
                    .and_then(|v| v.as_str())
                    .map(|s| s == value)
                    .unwrap_or(false)
            })
            .collect())
    }

    /// Delete all records older than `cutoff_ms` (Unix epoch milliseconds).
    /// Records whose `causal_parent` points to a surviving record are kept
    /// even if they are older — orphaning a causal chain would corrupt the graph.
    ///
    /// Returns `(kept, pruned)` counts.
    pub fn prune(&mut self, cutoff_ms: u64) -> Result<(usize, usize)> {
        let all = self.all()?;

        // IDs of records recent enough to keep.
        let kept_ids: std::collections::HashSet<String> = all.iter()
            .filter(|r| r.ts_ms >= cutoff_ms)
            .map(|r| r.id.clone())
            .collect();

        // Also keep any record whose id is a causal_parent of a kept record —
        // prevents dangling edges that reference pruned ancestors.
        let parent_ids: std::collections::HashSet<String> = all.iter()
            .filter(|r| kept_ids.contains(&r.id))
            .filter_map(|r| r.causal_parent.clone())
            .collect();

        let to_keep: Vec<LedgerRecord> = all.into_iter()
            .filter(|r| kept_ids.contains(&r.id) || parent_ids.contains(&r.id))
            .collect();

        let pruned = {
            let all_count = self.all()?.len();
            all_count - to_keep.len()
        };

        // Rewrite the store atomically: write to .tmp then rename
        let tmp_path = self.events_path.with_extension("ndjson.tmp");
        {
            let mut f = File::create(&tmp_path)?;
            for r in &to_keep {
                writeln!(f, "{}", serde_json::to_string(r)?)?;
            }
        }
        fs::rename(&tmp_path, &self.events_path)?;

        Ok((to_keep.len(), pruned))
    }

    /// Replace a single record by id. Returns `true` if a record was replaced.
    /// Writes atomically via a `.tmp` rename.
    pub fn replace_record(&mut self, old_id: &str, new_record: &LedgerRecord) -> Result<bool> {
        let all = self.all()?;
        let mut replaced = false;
        let rewritten: Vec<LedgerRecord> = all.into_iter().map(|r| {
            if r.id == old_id {
                replaced = true;
                new_record.clone()
            } else {
                r
            }
        }).collect();
        if !replaced {
            return Ok(false);
        }
        let tmp_path = self.events_path.with_extension("ndjson.tmp");
        {
            let mut f = File::create(&tmp_path)?;
            for r in &rewritten {
                writeln!(f, "{}", serde_json::to_string(r)?)?;
            }
        }
        fs::rename(&tmp_path, &self.events_path)?;
        Ok(true)
    }

    /// Rewrite all records whose principal matches `old_prefix` to use `new_principal`.
    ///
    /// Used for engineer-backfill: sessions ingested before `--engineer` was available
    /// store `agent:<uuid>` as principal; this replaces them with a real email address.
    ///
    /// Returns `(total, updated)` counts. Writes atomically via a `.tmp` rename.
    pub fn rewrite_principals(&mut self, old_prefix: &str, new_principal: &str) -> Result<(usize, usize)> {
        let all = self.all()?;
        let total = all.len();
        let mut updated = 0usize;
        let rewritten: Vec<LedgerRecord> = all.into_iter().map(|mut r| {
            if r.principal.starts_with(old_prefix) {
                r.principal = new_principal.to_string();
                updated += 1;
            }
            r
        }).collect();
        let tmp_path = self.events_path.with_extension("ndjson.tmp");
        {
            let mut f = File::create(&tmp_path)?;
            for r in &rewritten {
                writeln!(f, "{}", serde_json::to_string(r)?)?;
            }
        }
        fs::rename(&tmp_path, &self.events_path)?;
        Ok((total, updated))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Effect;
    use serde_json::json;
    use tempfile::tempdir;

    fn make_record(id: &str, ts_ms: u64, parent: Option<&str>) -> LedgerRecord {
        LedgerRecord {
            id: id.to_string(),
            principal: "test".to_string(),
            effect: Effect::GitCommit,
            causal_parent: parent.map(String::from),
            ts_ms,
            payload: json!({}),
            repo: None,
        }
    }

    #[test]
    fn test_prune_removes_old_records() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.append(&make_record("r1", 1000, None)).unwrap();
        store.append(&make_record("r2", 5000, None)).unwrap();
        store.append(&make_record("r3", 9000, None)).unwrap();

        // Prune everything older than ts_ms=5000
        let (kept, pruned) = store.prune(5000).unwrap();
        assert_eq!(pruned, 1, "r1 should be pruned");
        assert_eq!(kept, 2, "r2 and r3 should remain");
        let remaining = store.all().unwrap();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.iter().any(|r| r.id == "r2"));
        assert!(remaining.iter().any(|r| r.id == "r3"));
    }

    #[test]
    fn test_prune_keeps_causal_children_of_survivors() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        // r1 is old but r2 (new) has r1 as causal_parent — r1 must survive
        store.append(&make_record("r1", 1000, None)).unwrap();
        store.append(&make_record("r2", 9000, Some("r1"))).unwrap();

        let (kept, pruned) = store.prune(5000).unwrap();
        assert_eq!(pruned, 0, "r1 must be kept because r2 references it");
        assert_eq!(kept, 2);
    }

    #[test]
    fn test_prune_nothing_when_all_recent() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.append(&make_record("r1", 9000, None)).unwrap();
        store.append(&make_record("r2", 9001, None)).unwrap();

        let (kept, pruned) = store.prune(5000).unwrap();
        assert_eq!(pruned, 0);
        assert_eq!(kept, 2);
    }

    #[test]
    fn test_find_by_repo_filters_correctly() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let mut r1 = make_record("r1", 1000, None);
        r1.repo = Some("api".to_string());
        let mut r2 = make_record("r2", 2000, None);
        r2.repo = Some("frontend".to_string());
        let r3 = make_record("r3", 3000, None); // untagged
        store.append(&r1).unwrap();
        store.append(&r2).unwrap();
        store.append(&r3).unwrap();

        let api = store.find_by_repo(Some("api")).unwrap();
        assert_eq!(api.len(), 1);
        assert_eq!(api[0].id, "r1");

        let all = store.find_by_repo(None).unwrap();
        assert_eq!(all.len(), 3);
    }

    #[test]
    fn test_prune_empty_store() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let (kept, pruned) = store.prune(9999).unwrap();
        assert_eq!(kept, 0);
        assert_eq!(pruned, 0);
    }

    #[test]
    fn test_rewrite_principals_updates_matching() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let mut r1 = make_record("r1", 1000, None);
        r1.principal = "agent:abc123".to_string();
        let mut r2 = make_record("r2", 2000, None);
        r2.principal = "agent:def456".to_string();
        let mut r3 = make_record("r3", 3000, None);
        r3.principal = "chris@example.com".to_string(); // already a real email, should not change
        store.append(&r1).unwrap();
        store.append(&r2).unwrap();
        store.append(&r3).unwrap();

        let (total, updated) = store.rewrite_principals("agent:", "chris@example.com").unwrap();
        assert_eq!(total, 3);
        assert_eq!(updated, 2);

        let all = store.all().unwrap();
        assert!(all.iter().all(|r| r.principal == "chris@example.com"));
    }

    #[test]
    fn test_rewrite_principals_no_match() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        store.append(&make_record("r1", 1000, None)).unwrap(); // principal="test"
        let (total, updated) = store.rewrite_principals("agent:", "chris@example.com").unwrap();
        assert_eq!(total, 1);
        assert_eq!(updated, 0);
    }
}
