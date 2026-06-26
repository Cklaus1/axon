//! R21 §5.2 — the `axon-os` command surface: explain / run / verify / replay.
//! Thin I/O shell over the pure core + the `Runtime`. Output is human-legible
//! (not just exit codes); every subcommand has `--help`.

use crate::gate::{admit, Admission};
use crate::grant::{Budget, ExecPolicy, Grant, Label};
use crate::manifest::{parse as parse_manifest, JobManifest};
use crate::record::{from_json, to_json, verify};
use crate::runtime::{AxonCoreRuntime, Runtime};
use crate::{replay, supervisor};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
axon-os — run untrusted Axon programs under a proven capability bound

USAGE:
    axon-os explain <job.axjob>
    axon-os run     <job.axjob> [--run-id ID] [--out DIR]
    axon-os verify  <record.json>
    axon-os replay  <run-id> [--store DIR]

Every subcommand accepts --help. Exit codes: 0 ok, 2 usage/malformed,
6 refine, 7 budget, 8 capability/denied, 9 tamper/divergence.";

/// The supervisor's own authority. Default is broad (the manifest is the bound);
/// `"".into()` is the universal path ancestor and `"*"` the universal host.
fn broad_supervisor_grant() -> Grant {
    Grant {
        fs_read: vec!["".into()],
        fs_write: vec!["".into()],
        net: vec!["*".into()],
        exec: ExecPolicy::Any,
        max_label: Label::Secret,
        budget: Budget {
            calls: i64::MAX,
            tokens: i64::MAX,
            cost_micro: i64::MAX,
        },
    }
}

/// The legible, quantified grant rendering (R21 §4.4 / §5.2) — the human-facing
/// "may / may NOT / budget / ceiling" block.
fn legible_grant(g: &Grant) -> String {
    let mut may = Vec::new();
    let mut not = Vec::new();
    if g.fs_read.is_empty() {
        not.push("read files".to_string());
    } else {
        may.push(format!("read {}", g.fs_read.join(", ")));
    }
    if g.fs_write.is_empty() {
        not.push("write files".to_string());
    } else {
        may.push(format!("write {}", g.fs_write.join(", ")));
    }
    if g.net.is_empty() {
        not.push("use the network".to_string());
    } else {
        may.push(format!("reach {}", g.net.join(", ")));
    }
    if matches!(g.exec, ExecPolicy::Any) {
        may.push("spawn processes".to_string());
    } else {
        not.push("spawn processes".to_string());
    }
    format!(
        "  This program MAY: {}\n  It may NOT: {}\n  Budget: \u{2264} {} calls / {} tokens / {} \u{b5}$\n  Confidentiality ceiling: {}",
        if may.is_empty() { "(nothing)".into() } else { may.join("; ") },
        if not.is_empty() { "(no restrictions)".into() } else { not.join(", ") },
        g.budget.calls,
        g.budget.tokens,
        g.budget.cost_micro,
        g.max_label.as_str(),
    )
}

fn read_manifest(path: &Path) -> Result<JobManifest, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let base = path.parent().unwrap_or(Path::new("."));
    let mut m = parse_manifest(&src, base).map_err(|v| v.legible())?;
    // Absolutize the program path so the run event + saved manifest + replay
    // all reference the SAME path regardless of cwd (deterministic replay).
    m.program = std::fs::canonicalize(&m.program).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|d| d.join(&m.program))
            .unwrap_or(m.program.clone())
    });
    Ok(m)
}

/// Entry point. Returns the process exit code.
pub fn run(args: Vec<String>) -> ExitCode {
    let a: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    match a.as_slice() {
        [] | ["--help"] | ["-h"] | ["help"] => {
            println!("{USAGE}");
            ExitCode::from(0)
        }
        ["explain", "--help"] => {
            help("explain <job.axjob>  — show the legible grant + gate verdict; no execution")
        }
        ["run", "--help"] => help("run <job.axjob> [--run-id ID] [--out DIR]  — gate→run→record"),
        ["verify", "--help"] => {
            help("verify <record.json>  — recompute the hash chain; detect tamper")
        }
        ["replay", "--help"] => {
            help("replay <run-id> [--store DIR]  — verify + re-run + assert identical")
        }
        ["explain", job] => cmd_explain(Path::new(job)),
        ["run", rest @ ..] => cmd_run(rest),
        ["verify", record] => cmd_verify(Path::new(record)),
        ["replay", rest @ ..] => cmd_replay(rest),
        _ => {
            eprintln!("axon-os: unrecognized invocation\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn help(line: &str) -> ExitCode {
    println!("axon-os {line}");
    ExitCode::from(0)
}

fn cmd_explain(job: &Path) -> ExitCode {
    let manifest = match read_manifest(job) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("axon-os: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = AxonCoreRuntime::from_env();
    let declared = rt.declared_effects(&manifest.program);
    let eff = manifest.grant.intersect(&broad_supervisor_grant());
    println!("Intent: {}", manifest.intent);
    println!("{}", legible_grant(&eff));
    match admit(&declared, &eff) {
        Admission::Admit => {
            println!("  Gate: \u{2713} ADMIT (declared effects are within the grant)");
            ExitCode::from(0)
        }
        Admission::Deny { reason, axis } => {
            println!("  Gate: \u{26a0} DENY ({reason}) [axis: {axis}]");
            ExitCode::from(8)
        }
    }
}

fn cmd_run(rest: &[&str]) -> ExitCode {
    let mut job: Option<&str> = None;
    let mut run_id = "run".to_string();
    let mut out = PathBuf::from(".");
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--run-id" if i + 1 < rest.len() => {
                run_id = rest[i + 1].to_string();
                i += 2;
            }
            "--out" if i + 1 < rest.len() => {
                out = PathBuf::from(rest[i + 1]);
                i += 2;
            }
            s if !s.starts_with("--") && job.is_none() => {
                job = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("axon-os run: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(job) = job else {
        eprintln!("axon-os run: missing <job.axjob>");
        return ExitCode::from(2);
    };
    let job_path = Path::new(job);
    let manifest = match read_manifest(job_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("axon-os: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = AxonCoreRuntime::from_env();
    let sup = broad_supervisor_grant();
    let rec = supervisor::run(&manifest, &sup, &run_id, &rt);

    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("axon-os run: cannot create {}: {e}", out.display());
        return ExitCode::from(2);
    }
    let rec_path = out.join(format!("{run_id}.json"));
    let _ = std::fs::write(&rec_path, to_json(&rec));
    // Save a manifest copy with the ABSOLUTE program path so `replay` reproduces
    // it regardless of the store directory (deterministic replay).
    let _ = std::fs::write(
        out.join(format!("{run_id}.axjob")),
        crate::manifest::to_axjob(&manifest),
    );
    println!(
        "{}  (run-id: {run_id}, record: {})",
        rec.verdict.legible(),
        rec_path.display()
    );
    ExitCode::from(rec.verdict.exit_code() as u8)
}

fn cmd_verify(record: &Path) -> ExitCode {
    let src = match std::fs::read_to_string(record) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("axon-os verify: cannot read {}: {e}", record.display());
            return ExitCode::from(2);
        }
    };
    let rec = match from_json(&src) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("axon-os verify: {}", e.detail);
            return ExitCode::from(2);
        }
    };
    match verify(&rec) {
        Ok(()) => {
            println!(
                "\u{2713} intact ({} events, digest {})",
                rec.events.len(),
                rec.record_digest
            );
            ExitCode::from(0)
        }
        Err(e) => {
            println!("\u{2717} TAMPERED: {}", e.detail);
            ExitCode::from(9)
        }
    }
}

fn cmd_replay(rest: &[&str]) -> ExitCode {
    let mut run_id: Option<&str> = None;
    let mut store = PathBuf::from(".");
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--store" if i + 1 < rest.len() => {
                store = PathBuf::from(rest[i + 1]);
                i += 2;
            }
            s if !s.starts_with("--") && run_id.is_none() => {
                run_id = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("axon-os replay: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(run_id) = run_id else {
        eprintln!("axon-os replay: missing <run-id>");
        return ExitCode::from(2);
    };
    let rec_path = store.join(format!("{run_id}.json"));
    let man_path = store.join(format!("{run_id}.axjob"));
    let stored = match std::fs::read_to_string(&rec_path)
        .ok()
        .and_then(|s| from_json(&s).ok())
    {
        Some(r) => r,
        None => {
            eprintln!("axon-os replay: no valid record at {}", rec_path.display());
            return ExitCode::from(2);
        }
    };
    let manifest = match read_manifest(&man_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("axon-os replay: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = AxonCoreRuntime::from_env();
    match replay::replay(&stored, &manifest, &broad_supervisor_grant(), &rt) {
        Ok(_) => {
            println!("\u{2713} replay identical (deterministic; record verified)");
            ExitCode::from(0)
        }
        Err(e) => {
            println!("\u{2717} {}", e.detail);
            ExitCode::from(9)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legible_grant_lists_may_and_may_not() {
        let g = Grant {
            fs_read: vec!["./data/".into()],
            fs_write: vec![],
            net: vec![],
            exec: ExecPolicy::None,
            max_label: Label::Internal,
            budget: Budget {
                calls: 100,
                tokens: 50000,
                cost_micro: 1000000,
            },
        };
        let s = legible_grant(&g);
        assert!(s.contains("read ./data/"));
        assert!(s.contains("may NOT") && s.contains("use the network"));
        assert!(s.contains("internal"));
    }

    #[test]
    fn broad_grant_is_a_superset_of_a_narrow_job() {
        let job = Grant {
            fs_read: vec!["./data/".into()],
            fs_write: vec!["./out/".into()],
            net: vec!["a.x.com".into()],
            exec: ExecPolicy::None,
            max_label: Label::Internal,
            budget: Budget {
                calls: 5,
                tokens: 5,
                cost_micro: 5,
            },
        };
        let eff = job.intersect(&broad_supervisor_grant());
        // The broad supervisor grant leaves the job's narrower bound intact.
        assert_eq!(eff.fs_read, vec!["./data/".to_string()]);
        assert_eq!(eff.net, vec!["a.x.com".to_string()]);
        assert!(eff.is_subset_of(&job) && job.is_subset_of(&eff));
    }
}
