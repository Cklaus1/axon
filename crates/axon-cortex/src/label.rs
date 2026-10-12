//! `cortex repair --label-log PATH`: one labelled row per repair run.
//!
//! A verified repair is a label nobody had to write: the hidden check named
//! the function whose new body it accepted. Recording that next to what the
//! run was given (the file's digest, the visible failing checks, the spectrum
//! ranking, the ranker's order, every attempt) turns ordinary `cortex repair`
//! use into localization data that is graded by the same adjudicator as the
//! repair itself. `benchmarks/labels/to_items.py` turns rows into LocRank
//! replay items and DecisionBench `loc` items.
//!
//! What a row is NOT allowed to do is change the run. It is written after the
//! verdict, the exit code and the file are all settled, and a write failure is
//! one stderr line, never a different exit code.
//!
//! The pre-run source is recorded only when asked (`--label-source`): a file
//! under repair may hold anything, and a log that silently copied it would
//! move it somewhere its owner did not choose.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The environment variable `--label-log` falls back to.
pub const LABEL_ENV: &str = "CORTEX_LABEL_LOG";

/// The row schema. A reader refuses any other value.
pub const SCHEMA: &str = "cortex-label/1";

/// Where rows go: the flag when given, else the environment, else nowhere.
///
/// An empty environment value is OFF, as an unset one is: `CORTEX_LABEL_LOG=`
/// is how a shell clears a variable. An empty FLAG is a usage error, because
/// on the command line it can only be a mistake.
pub fn select(flag: Option<&str>, env: Option<&str>) -> Result<Option<PathBuf>, String> {
    match flag {
        Some("") => Err("--label-log needs a non-empty path".to_string()),
        Some(p) => Ok(Some(PathBuf::from(p))),
        None => Ok(env.filter(|v| !v.is_empty()).map(PathBuf::from)),
    }
}

/// Lowercase hex sha256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// The sha256 of the running executable, or `None` when it cannot be read
/// (deleted while running, no `/proc`). Identifies the build a row came from
/// independently of the version string, which does not change per commit.
pub fn build_sha256() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    std::fs::read(exe).ok().map(|b| sha256_hex(&b))
}

/// `YYYY-MM-DDTHH:MM:SSZ` in UTC.
pub fn utc_timestamp(at: std::time::SystemTime) -> String {
    let secs = at
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rem) = (secs / 86_400, secs % 86_400);
    // Civil date from days since 1970-01-01 (H. Hinnant, `civil_from_days`).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

/// The localization a run did before its walk. Absent with `--symbol`.
#[derive(Clone, Debug)]
pub struct Localization {
    /// Visible failing checks (the adjudicating one excluded).
    pub failing: Vec<String>,
    /// Visible passing checks (the adjudicating one excluded).
    pub passing: Vec<String>,
    /// The full spectrum ranking, most suspicious first, with scores.
    pub ranked: Vec<(String, f64)>,
    /// `--candidates`.
    pub candidates: usize,
    /// The spectrum's top `candidates`: the set the ranker may only permute.
    pub eligible: Vec<String>,
}

/// What one candidate cost, across every resumption of its episode.
#[derive(Clone, Debug)]
pub struct CandidateCost {
    pub symbol: String,
    pub proposals: usize,
    pub steps: usize,
}

/// One run, as a label. Built by `cortex repair` after the verdict is fixed.
#[derive(Clone, Debug)]
pub struct LabelRow<'a> {
    pub ts: String,
    pub cortex_sha256: Option<String>,
    /// Workspace-relative, as given to `--file`.
    pub file: &'a str,
    /// The file as the run found it; its sha256 is always recorded, the text
    /// itself only when `include_source`.
    pub source: &'a str,
    pub include_source: bool,
    pub check: &'a str,
    /// `--symbol`, when the operator named the target.
    pub symbol: Option<&'a str>,
    /// The `--generator` spec, with a `literal:` body withheld.
    pub generator: &'a str,
    pub budget: usize,
    pub localization: Option<&'a Localization>,
    pub ranker: Option<Value>,
    pub attempted: &'a [String],
    pub outcome: &'a str,
    pub exit_code: i32,
    pub steps: usize,
    /// The repaired symbol, exactly when the outcome is `verified_done`.
    pub truth: Option<&'a str>,
    /// Per walked candidate, in walk order.
    pub costs: Vec<CandidateCost>,
    pub audit: Option<Value>,
}

impl LabelRow<'_> {
    pub fn to_json(&self) -> Value {
        // A `literal:` body is the operator's own text and may be as sensitive
        // as the source; the kind is what a reader needs.
        let generator = if self.generator.starts_with("literal:") {
            "literal"
        } else {
            self.generator
        };
        json!({
            "schema": SCHEMA,
            "ts": self.ts,
            "cortex": {
                "version": env!("CARGO_PKG_VERSION"),
                "sha256": self.cortex_sha256,
            },
            "file": self.file,
            "file_sha256": sha256_hex(self.source.as_bytes()),
            "check": self.check,
            "symbol": self.symbol,
            "generator": generator,
            "budget": self.budget,
            "localization": self.localization.map(|l| json!({
                "failing": l.failing,
                "passing": l.passing,
                "ranked": l.ranked.iter()
                    .map(|(s, sc)| json!({"symbol": s, "score": sc}))
                    .collect::<Vec<_>>(),
                "candidates": l.candidates,
                "eligible": l.eligible,
            })),
            "ranker": self.ranker,
            "attempted": self.attempted,
            "outcome": self.outcome,
            "exit_code": self.exit_code,
            "steps": self.steps,
            "truth": self.truth,
            "proposals": self.costs.iter()
                .map(|c| json!({"symbol": c.symbol, "proposals": c.proposals, "steps": c.steps}))
                .collect::<Vec<_>>(),
            "audit": self.audit,
            "source": self.include_source.then_some(self.source),
        })
    }
}

/// Append `row` to `path` as ONE line, in ONE `write(2)` on an `O_APPEND`
/// descriptor, so concurrent runs sharing a log never interleave within a
/// line. A new file is created owner-only (0600): rows may carry source.
///
/// A short write is reported, not retried: a second write could land after
/// another process's line and split this one in two.
pub fn append(path: &Path, row: &Value) -> Result<(), String> {
    use std::io::Write;
    let mut line = serde_json::to_string(row).map_err(|e| e.to_string())?;
    line.push('\n');
    let mut opts = std::fs::OpenOptions::new();
    opts.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path).map_err(|e| e.to_string())?;
    let n = f.write(line.as_bytes()).map_err(|e| e.to_string())?;
    if n != line.len() {
        return Err(format!("short write: {n} of {} bytes", line.len()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flag_beats_env_and_empty_env_is_off() {
        assert_eq!(
            select(Some("a.jsonl"), Some("b.jsonl")).unwrap(),
            Some(PathBuf::from("a.jsonl"))
        );
        assert_eq!(
            select(None, Some("b.jsonl")).unwrap(),
            Some(PathBuf::from("b.jsonl"))
        );
        assert_eq!(select(None, Some("")).unwrap(), None);
        assert_eq!(select(None, None).unwrap(), None);
        assert!(select(Some(""), Some("b.jsonl")).is_err());
    }

    #[test]
    fn timestamps_are_utc_civil_dates() {
        let at = |s: u64| std::time::UNIX_EPOCH + std::time::Duration::from_secs(s);
        assert_eq!(utc_timestamp(at(0)), "1970-01-01T00:00:00Z");
        // 2000-02-29 (a leap day in a century leap year) 12:34:56.
        assert_eq!(utc_timestamp(at(951_827_696)), "2000-02-29T12:34:56Z");
        assert_eq!(utc_timestamp(at(1_791_763_200)), "2026-10-12T00:00:00Z");
    }

    fn row<'a>(attempted: &'a [String], loc: &'a Localization) -> LabelRow<'a> {
        LabelRow {
            ts: "2026-10-12T00:00:00Z".into(),
            cortex_sha256: None,
            file: "f.ax",
            source: "fn a() {}\n",
            include_source: false,
            check: "hidden",
            symbol: None,
            generator: "literal:secret body",
            budget: 8,
            localization: Some(loc),
            ranker: None,
            attempted,
            outcome: "verified_done",
            exit_code: 0,
            steps: 2,
            truth: Some("a"),
            costs: vec![CandidateCost {
                symbol: "a".into(),
                proposals: 1,
                steps: 2,
            }],
            audit: None,
        }
    }

    #[test]
    fn source_and_literal_bodies_stay_out_unless_asked() {
        let attempted = vec!["a".to_string()];
        let loc = Localization {
            failing: vec!["t".into()],
            passing: vec![],
            ranked: vec![("a".into(), 1.0)],
            candidates: 3,
            eligible: vec!["a".into()],
        };
        let mut r = row(&attempted, &loc);
        let v = r.to_json();
        assert_eq!(v["schema"], SCHEMA);
        assert_eq!(v["source"], Value::Null);
        assert_eq!(v["generator"], "literal");
        assert!(!v.to_string().contains("secret body"));
        assert_eq!(v["file_sha256"], sha256_hex(b"fn a() {}\n"));
        assert_eq!(v["localization"]["ranked"][0]["symbol"], "a");
        r.include_source = true;
        assert_eq!(r.to_json()["source"], "fn a() {}\n");
    }

    #[test]
    fn append_writes_whole_lines_and_reports_failure() {
        let dir = std::env::temp_dir().join(format!("cortex_label_unit_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("l.jsonl");
        append(&p, &json!({"n": 1})).unwrap();
        append(&p, &json!({"n": 2})).unwrap();
        assert_eq!(
            std::fs::read_to_string(&p).unwrap(),
            "{\"n\":1}\n{\"n\":2}\n"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&p).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(append(&dir.join("missing/l.jsonl"), &json!({})).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
