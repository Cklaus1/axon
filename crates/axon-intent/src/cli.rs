//! R22 §5.2 — the `axon-intent` command surface: compile / review / approve /
//! emit. Thin I/O shell over the pure core + the `Synthesizer`. Output is
//! human-legible (not just exit codes); every subcommand has `--help`.

use crate::approval::{from_json, verify_token, ApprovalToken, Risk};
use crate::emit;
use crate::intent::{self, Intent};
use crate::pipeline::{self, GenMode, Resolved};
use crate::refusal::{Decision, Refusal};
use crate::synth::{MockSynthOrReal, RealSynthesizer};
use axon_os::manifest::{self, JobManifest};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
axon-intent — turn prose intent into a proven-bounded job (R22)

USAGE:
    axon-intent compile <intent.md> [--out DIR] [--seed N] [--risk LEVEL]
    axon-intent review  <name.axjob> [--program <name.ax>]
    axon-intent approve <name.axjob> --by OP [--accept|--reject] [--approvers N]
    axon-intent emit    <intent.md> --by OP [--out DIR] [--yes]

Every subcommand accepts --help. Exit codes: 0 ok, 2 usage/malformed,
3 rejected, 5 low-confidence refusal, 8 not-admissible/grant-exceeds-ceiling.
Set AXON_AI_MOCK=1 for deterministic, offline synthesis.";

/// Entry point. Returns the process exit code.
pub fn run(args: Vec<String>) -> ExitCode {
    let a: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    match a.as_slice() {
        [] | ["--help"] | ["-h"] | ["help"] => {
            println!("{USAGE}");
            ExitCode::from(0)
        }
        ["compile", "--help"] => help(
            "compile <intent.md> [--out DIR] [--seed N] [--risk LEVEL]  — synthesize → infer least-privilege grant → prove admissible → score; writes <name>.ax + <name>.axjob",
        ),
        ["review", "--help"] => {
            help("review <name.axjob> [--program <name.ax>]  — print the legible approval request; no side effects")
        }
        ["approve", "--help"] => help(
            "approve <name.axjob> --by OP [--accept|--reject] [--approvers N]  — record the decision; on --accept write <name>.approval",
        ),
        ["emit", "--help"] => {
            help("emit <intent.md> --by OP [--out DIR] [--yes]  — compile → review → approve in one step")
        }
        ["compile", rest @ ..] => cmd_compile(rest),
        ["review", rest @ ..] => cmd_review(rest),
        ["approve", rest @ ..] => cmd_approve(rest),
        ["emit", rest @ ..] => cmd_emit(rest),
        _ => {
            eprintln!("axon-intent: unrecognized invocation\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn help(line: &str) -> ExitCode {
    println!("axon-intent {line}");
    ExitCode::from(0)
}

/// The synthesizer to use. `AXON_AI_MOCK=1` selects the deterministic mock
/// (offline, reproducible); otherwise the real subprocess synthesizer.
fn synthesizer() -> MockSynthOrReal {
    if std::env::var_os("AXON_AI_MOCK").is_some() {
        MockSynthOrReal::Mock(crate::synth::MockSynth::new())
    } else {
        MockSynthOrReal::Real(RealSynthesizer::from_env())
    }
}

/// Stem (file name without extension) used to name the emitted triple.
fn name_of(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .map(|s| s.trim_end_matches(".intent").to_string())
        .unwrap_or_else(|| "job".to_string())
}

fn read_intent(path: &Path, seed_override: Option<u64>) -> Result<Intent, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let mut i = intent::parse(&src).map_err(|r| r.legible())?;
    if let Some(s) = seed_override {
        i.seed = s;
    }
    Ok(i)
}

// ── compile ──────────────────────────────────────────────────────────────────
fn cmd_compile(rest: &[&str]) -> ExitCode {
    let mut file: Option<&str> = None;
    let mut out = PathBuf::from(".");
    let mut seed: Option<u64> = None;
    let mut risk_floor: Option<Risk> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--out" if i + 1 < rest.len() => {
                out = PathBuf::from(rest[i + 1]);
                i += 2;
            }
            "--seed" if i + 1 < rest.len() => {
                match rest[i + 1].parse::<u64>() {
                    Ok(n) => seed = Some(n),
                    Err(_) => {
                        eprintln!("axon-intent compile: --seed must be a u64");
                        return ExitCode::from(2);
                    }
                }
                i += 2;
            }
            "--risk" if i + 1 < rest.len() => {
                match Risk::parse(rest[i + 1]) {
                    Some(r) => risk_floor = Some(r),
                    None => {
                        eprintln!("axon-intent compile: --risk must be low|medium|high|critical");
                        return ExitCode::from(2);
                    }
                }
                i += 2;
            }
            s if !s.starts_with("--") && file.is_none() => {
                file = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("axon-intent compile: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(file) = file else {
        eprintln!("axon-intent compile: missing <intent.md>");
        return ExitCode::from(2);
    };
    let path = Path::new(file);
    let intent = match read_intent(path, seed) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("axon-intent: {e}");
            return ExitCode::from(2);
        }
    };
    let synth = synthesizer();
    match pipeline::resolve(&intent, &synth, gen_mode(&intent), risk_floor) {
        Ok(resolved) => {
            let name = name_of(path);
            match emit::write_compiled(&resolved, &intent, &out, &name) {
                Ok(triple) => {
                    print_resolved(&resolved);
                    println!(
                        "  Wrote {} + {}",
                        triple.program.display(),
                        triple.manifest.display()
                    );
                    println!(
                        "ADMISSIBLE, confidence {}/100, risk {}",
                        resolved.request.confidence,
                        resolved.request.risk.as_str()
                    );
                    ExitCode::from(0)
                }
                Err(e) => {
                    eprintln!("axon-intent compile: cannot write triple: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Err(r) => {
            println!("{}", r.legible());
            ExitCode::from(r.exit_code() as u8)
        }
    }
}

/// `AXON_INTENT_GEN=1` forces generation even when a fenced block is present.
fn gen_mode(_intent: &Intent) -> GenMode {
    if std::env::var_os("AXON_INTENT_GEN").is_some() {
        GenMode::ForceGenerate
    } else {
        GenMode::Auto
    }
}

fn print_resolved(r: &Resolved) {
    println!("Intent: {}", r.request.intent_goal);
    println!("{}", r.request.legible);
    if !r.request.uncertainty.is_empty() {
        println!("  Uncertainty: {}", r.request.uncertainty.join("; "));
    }
}

// ── review ───────────────────────────────────────────────────────────────────
fn cmd_review(rest: &[&str]) -> ExitCode {
    let mut job: Option<&str> = None;
    let mut program: Option<PathBuf> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--program" if i + 1 < rest.len() => {
                program = Some(PathBuf::from(rest[i + 1]));
                i += 2;
            }
            s if !s.starts_with("--") && job.is_none() => {
                job = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("axon-intent review: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(job) = job else {
        eprintln!("axon-intent review: missing <name.axjob>");
        return ExitCode::from(2);
    };
    let (manifest, _src) = match load_job(Path::new(job), program.as_deref()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("axon-intent: {e}");
            return ExitCode::from(2);
        }
    };
    let risk = crate::approval::derive_risk(&manifest.grant);
    println!("Intent: {}", manifest.intent);
    println!(
        "{}",
        crate::approval::legible_bound(&manifest.grant, risk, 0)
    );
    ExitCode::from(0)
}

/// Load a `.axjob` + its program source (`--program` or the manifest's program
/// path resolved relative to the manifest dir).
fn load_job(job: &Path, program: Option<&Path>) -> Result<(JobManifest, String), String> {
    let src =
        std::fs::read_to_string(job).map_err(|e| format!("cannot read {}: {e}", job.display()))?;
    let base = job.parent().unwrap_or(Path::new("."));
    let manifest = manifest::parse(&src, base).map_err(|v| v.legible())?;
    let prog_path = match program {
        Some(p) => p.to_path_buf(),
        None => manifest.program.clone(),
    };
    let prog_src = std::fs::read_to_string(&prog_path)
        .map_err(|e| format!("cannot read program {}: {e}", prog_path.display()))?;
    Ok((manifest, prog_src))
}

// ── approve ──────────────────────────────────────────────────────────────────
fn cmd_approve(rest: &[&str]) -> ExitCode {
    let mut job: Option<&str> = None;
    let mut by: Option<&str> = None;
    let mut decision = Decision::Approved;
    let mut program: Option<PathBuf> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--by" if i + 1 < rest.len() => {
                by = Some(rest[i + 1]);
                i += 2;
            }
            "--program" if i + 1 < rest.len() => {
                program = Some(PathBuf::from(rest[i + 1]));
                i += 2;
            }
            "--accept" => {
                decision = Decision::Approved;
                i += 1;
            }
            "--reject" => {
                decision = Decision::Rejected;
                i += 1;
            }
            "--approvers" if i + 1 < rest.len() => {
                let n: u32 = rest[i + 1].parse().unwrap_or(1);
                if n > 1 {
                    eprintln!("axon-intent approve: multi-party approval is R24 (--approvers N>1 not supported)");
                    return ExitCode::from(2);
                }
                i += 2;
            }
            s if !s.starts_with("--") && job.is_none() => {
                job = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("axon-intent approve: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(job) = job else {
        eprintln!("axon-intent approve: missing <name.axjob>");
        return ExitCode::from(2);
    };
    let Some(by) = by else {
        eprintln!("axon-intent approve: --by <operator-id> is required");
        return ExitCode::from(2);
    };
    let (manifest, prog_src) = match load_job(Path::new(job), program.as_deref()) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("axon-intent: {e}");
            return ExitCode::from(2);
        }
    };
    // Build the token directly from the on-disk (program, grant) — the content we
    // bind is EXACTLY what is on disk, so a later one-byte edit invalidates it.
    let risk = crate::approval::derive_risk(&manifest.grant);
    let pd = crate::approval::program_digest(&prog_src);
    let gd = crate::approval::grant_digest(&manifest.grant);
    let token = build_token_raw(&pd, &gd, by, decision, risk);

    if decision == Decision::Rejected {
        println!("\u{2717} rejected by {by}");
        return ExitCode::from(3);
    }

    let name = name_of(Path::new(job));
    let out = Path::new(job).parent().unwrap_or(Path::new("."));
    let path = out.join(format!("{name}.approval"));
    if let Err(e) = std::fs::write(&path, crate::approval::to_json(&token)) {
        eprintln!("axon-intent approve: cannot write {}: {e}", path.display());
        return ExitCode::from(2);
    }
    println!(
        "\u{2713} approved by {by} \u{2014} token bound to program {} grant {}",
        short(&pd),
        short(&gd)
    );
    println!("  Wrote {}", path.display());
    ExitCode::from(0)
}

/// Build a token from raw digests (the approve path binds on-disk content).
fn build_token_raw(
    program_digest: &str,
    grant_digest: &str,
    by: &str,
    decision: Decision,
    risk: Risk,
) -> ApprovalToken {
    let req = crate::approval::ApprovalRequest {
        program_digest: program_digest.to_string(),
        grant: empty_grant(),
        grant_digest: grant_digest.to_string(),
        intent_goal: String::new(),
        risk,
        confidence: 0,
        legible: String::new(),
        uncertainty: Vec::new(),
    };
    crate::approval::build_token(&req, by, decision)
}

fn empty_grant() -> axon_os::grant::Grant {
    axon_os::grant::Grant {
        fs_read: vec![],
        fs_write: vec![],
        net: vec![],
        exec: axon_os::grant::ExecPolicy::None,
        max_label: axon_os::grant::Label::Public,
        budget: axon_os::grant::Budget {
            calls: 0,
            tokens: 0,
            cost_micro: 0,
        },
    }
}

fn short(digest: &str) -> String {
    let h = digest.rsplit(':').next().unwrap_or(digest);
    h.chars().take(12).collect()
}

// ── emit (compile → review → approve in one step) ────────────────────────────
fn cmd_emit(rest: &[&str]) -> ExitCode {
    let mut file: Option<&str> = None;
    let mut by: Option<&str> = None;
    let mut out = PathBuf::from(".");
    let mut yes = false;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--by" if i + 1 < rest.len() => {
                by = Some(rest[i + 1]);
                i += 2;
            }
            "--out" if i + 1 < rest.len() => {
                out = PathBuf::from(rest[i + 1]);
                i += 2;
            }
            "--yes" => {
                yes = true;
                i += 1;
            }
            s if !s.starts_with("--") && file.is_none() => {
                file = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("axon-intent emit: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(file) = file else {
        eprintln!("axon-intent emit: missing <intent.md>");
        return ExitCode::from(2);
    };
    let Some(by) = by else {
        eprintln!("axon-intent emit: --by <operator-id> is required");
        return ExitCode::from(2);
    };
    let path = Path::new(file);
    let intent = match read_intent(path, None) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("axon-intent: {e}");
            return ExitCode::from(2);
        }
    };
    let synth = synthesizer();
    let resolved = match pipeline::resolve(&intent, &synth, gen_mode(&intent), None) {
        Ok(r) => r,
        Err(r) => {
            println!("{}", r.legible());
            return ExitCode::from(r.exit_code() as u8);
        }
    };
    let name = name_of(path);
    if let Err(e) = emit::write_compiled(&resolved, &intent, &out, &name) {
        eprintln!("axon-intent emit: cannot write triple: {e}");
        return ExitCode::from(2);
    }
    print_resolved(&resolved);
    if !yes {
        eprintln!("axon-intent emit: pass --yes to approve non-interactively");
        return ExitCode::from(2);
    }
    match emit::write_approval(&resolved, &out, &name, by, Decision::Approved) {
        Ok(p) => {
            println!("\u{2713} approved by {by} \u{2014} wrote {}", p.display());
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!("axon-intent emit: cannot write approval: {e}");
            ExitCode::from(2)
        }
    }
}

/// Library helper for the R21-handoff contract (R22 §3.5): given an emitted
/// triple on disk, verify the approval token binds the actual (program, grant).
/// This is the substance R21's `run` calls (see the CROSS-CRATE HOOK).
pub fn verify_triple(job: &Path) -> Result<(), String> {
    let name = name_of(job);
    let dir = job.parent().unwrap_or(Path::new("."));
    let approval_path = dir.join(format!("{name}.approval"));
    let token_src = std::fs::read_to_string(&approval_path)
        .map_err(|e| format!("cannot read approval {}: {e}", approval_path.display()))?;
    let token: ApprovalToken = from_json(&token_src).map_err(|m| m.detail)?;
    let (manifest, prog_src) = load_job(job, None)?;
    verify_token(&token, &prog_src, &manifest.grant).map_err(|m| m.detail)
}

// keep the Refusal type referenced so the legible() rendering is exercised.
#[allow(dead_code)]
fn _ref(_: &Refusal) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_of_strips_intent_md() {
        assert_eq!(name_of(Path::new("summarize.intent.md")), "summarize");
        assert_eq!(name_of(Path::new("foo.axjob")), "foo");
    }

    #[test]
    fn short_truncates_a_digest() {
        assert_eq!(short("axsha256:abcdef0123456789").len(), 12);
    }
}
