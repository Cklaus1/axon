use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result};
use serde_json::json;

use crate::hash::record_id;
use crate::model::{Effect, LedgerRecord};
use crate::store::Store;

/// Ingest git commits into the ledger.
///
/// `since`: if provided, passed directly to `git log --after=<since>`.
/// git accepts ISO 8601 dates ("2026-01-01"), relative times ("30 days ago"),
/// or Unix epoch prefixed with "@".
pub fn ingest_git(
    repo_path: &Path,
    store: &mut Store,
    since: Option<&str>,
    repo_name: Option<&str>,
) -> Result<usize> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(repo_path)
        .arg("log")
        .arg("--all")
        .arg("--format=COMMIT_START%n%H|%ae|%at|%s")
        .arg("--name-only");
    if let Some(s) = since {
        cmd.arg(format!("--after={s}"));
    }
    let output = cmd.output().context("Failed to run git log")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git log failed: {}", stderr);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let commits = parse_git_log(&stdout);

    // Load existing commit SHAs for dedup
    let existing = store.find_by_effect(&Effect::GitCommit)?;
    let existing_shas: std::collections::HashSet<String> = existing
        .iter()
        .filter_map(|r| {
            r.payload
                .get("sha")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .collect();

    let mut added = 0;
    for commit in commits {
        let sha = commit.sha.clone();
        if existing_shas.contains(&sha) {
            continue;
        }

        let payload = json!({
            "sha": commit.sha,
            "message": commit.message,
            "author": commit.author,
            "files": commit.files,
        });

        let ts_ms = commit.unix_ts * 1000;
        let id = record_id(
            &format!("git:{}", commit.author),
            &Effect::GitCommit,
            ts_ms,
            &payload,
        );

        let record = LedgerRecord {
            id,
            principal: format!("git:{}", commit.author),
            effect: Effect::GitCommit,
            causal_parent: None,
            ts_ms,
            payload,
            repo: repo_name.map(String::from),
        };

        store.append(&record)?;
        added += 1;
    }

    Ok(added)
}

struct CommitInfo {
    sha: String,
    author: String,
    unix_ts: u64,
    message: String,
    files: Vec<String>,
}

fn parse_git_log(output: &str) -> Vec<CommitInfo> {
    let mut commits = Vec::new();
    let mut lines = output.lines().peekable();

    while let Some(line) = lines.next() {
        if line != "COMMIT_START" {
            continue;
        }
        // Next line is the header: SHA|email|unix_ts|subject
        let header = match lines.next() {
            Some(h) => h,
            None => break,
        };
        let parts: Vec<&str> = header.splitn(4, '|').collect();
        if parts.len() < 4 {
            continue;
        }
        let sha = parts[0].to_string();
        let author = parts[1].to_string();
        let unix_ts: u64 = parts[2].parse().unwrap_or(0);
        let message = parts[3].to_string();

        // Collect file names until next COMMIT_START or EOF
        let mut files = Vec::new();
        loop {
            match lines.peek() {
                None => break,
                Some(&"COMMIT_START") => break,
                Some(_) => {
                    let f = lines.next().unwrap();
                    let f = f.trim();
                    if !f.is_empty() {
                        files.push(f.to_string());
                    }
                }
            }
        }

        commits.push(CommitInfo {
            sha,
            author,
            unix_ts,
            message,
            files,
        });
    }

    commits
}
