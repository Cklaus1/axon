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
}
