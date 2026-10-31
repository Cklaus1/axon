//! R22 §5.2 — the `axon-intent` command surface: compile / review / approve /
//! emit. Thin I/O shell over the pure core, the check→repair loop
//! (`crate::genloop`) and its `crate::generator::Generator`. Output is
//! human-legible (not just exit codes); every subcommand has `--help`.

use crate::approval::{build_token_raw, Risk};
use crate::emit;
use crate::generator;
use crate::genloop::{self, LoopConfig};
use crate::intent::{self, Intent};
use crate::pipeline::{self, GenMode, Resolved};
use crate::refusal::{Decision, Refusal};
use crate::synth::MockSynth;
use axon_os::manifest::{self, JobManifest};
use axon_os::runtime::{AxonCoreRuntime, Runtime};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
axon-intent — turn prose intent into a proven-bounded job (R22)

USAGE:
    axon-intent compile <intent.md> [--out DIR] [--seed N] [--risk LEVEL] [GEN]
    axon-intent review  <name.axjob> [--program <name.ax>]
    axon-intent approve <name.axjob> --by OP [--accept|--reject] [--approvers N]
    axon-intent emit    <intent.md> --by OP [--out DIR] [--yes] [GEN]
    axon-intent authorize-repair ...   (NH-R1 repair envelope; see authorize-repair --help)
    axon-intent audit-runs --spent-store D --run-ledger D [--envelopes DIR] [--json]

GEN: [--generator cmd:PATH|ai:MODEL] [--generate] [--max-rounds N<=3] [--transcript PATH]
An intent's fenced ```axon program is lifted and checked (axon check, the gates,
its examples); otherwise --generator runs the check->repair loop. --generate
generates even when a fenced program is present. emit --yes never approves a
generated program: a human runs `approve` on it.

Every subcommand accepts --help. Exit codes: 0 ok, 1 audit-runs findings,
2 usage/malformed, 3 rejected, 5 low-confidence refusal,
8 not-admissible/grant-exceeds-ceiling.
Set AXON_AI_MOCK=1 for the deterministic, offline mock synthesizer.";

/// Entry point. Returns the process exit code.
pub fn run(args: Vec<String>) -> ExitCode {
    let a: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    match a.as_slice() {
        [] | ["--help"] | ["-h"] | ["help"] => {
            println!("{USAGE}");
            ExitCode::from(0)
        }
        ["compile", "--help"] => help(
            "compile <intent.md> [--out DIR] [--seed N] [--risk LEVEL] [--generator cmd:PATH|ai:MODEL] [--generate] [--max-rounds N] [--transcript PATH]  — lift the fenced program or generate one (check→repair loop) → check → least-privilege grant → admissible → examples → score; writes <name>.ax + <name>.axjob",
        ),
        ["review", "--help"] => {
            help("review <name.axjob> [--program <name.ax>]  — print the legible approval request; no side effects")
        }
        ["approve", "--help"] => help(
            "approve <name.axjob> --by OP [--accept|--reject] [--approvers N]  — record the decision; on --accept write <name>.approval",
        ),
        ["emit", "--help"] => help(
            "emit <intent.md> --by OP [--out DIR] [--yes] [--generator cmd:PATH|ai:MODEL] [--generate] [--max-rounds N] [--transcript PATH]  — compile → review → approve in one step; --yes refuses a generated program (approve it yourself)",
        ),
        ["compile", rest @ ..] => cmd_compile(rest),
        ["review", rest @ ..] => cmd_review(rest),
        ["approve", rest @ ..] => cmd_approve(rest),
        ["emit", rest @ ..] => cmd_emit(rest),
        ["authorize-repair", rest @ ..] => crate::authorize::run(rest),
        ["audit-runs", rest @ ..] => crate::audit_runs::run(rest),
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

/// The generation flags `compile` and `emit` share (IG5).
#[derive(Default)]
struct GenFlags {
    generator: Option<String>,
    generate: bool,
    max_rounds: Option<u32>,
    transcript: Option<PathBuf>,
}

impl GenFlags {
    /// Consume the generation flag at `rest[i]`, if it is one: `Ok(n)` = the
    /// number of arguments taken (0 = not a generation flag).
    fn take(&mut self, rest: &[&str], i: usize) -> Result<usize, String> {
        let value = rest.get(i + 1).copied();
        match (rest[i], value) {
            ("--generator", Some(v)) => {
                self.generator = Some(v.to_string());
                Ok(2)
            }
            ("--max-rounds", Some(v)) => match v.parse::<u32>() {
                Ok(n) if n <= genloop::MAX_REPAIR_ROUNDS => {
                    self.max_rounds = Some(n);
                    Ok(2)
                }
                _ => Err(format!(
                    "--max-rounds must be 0..={}",
                    genloop::MAX_REPAIR_ROUNDS
                )),
            },
            ("--transcript", Some(v)) => {
                self.transcript = Some(PathBuf::from(v));
                Ok(2)
            }
            ("--generate", _) => {
                self.generate = true;
                Ok(1)
            }
            _ => Ok(0),
        }
    }
}

/// Why `produce` did not return a program: a usage error (exit 2, message on
/// stderr) or a refusal (its own exit code, legible on stdout).
enum Produce {
    Usage(String),
    Refused(Refusal),
}

/// The `axon` binary that checks and runs candidates: `AXON_BIN` (made
/// absolute), else `target/debug/axon` — the runtime's own default.
fn axon_bin() -> PathBuf {
    let p = std::env::var_os("AXON_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/debug/axon"));
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p)
    }
}

/// The program for `intent`, gated and ready for approval (IG5). Exactly one
/// source, never a fallback:
/// - `--generator`: the check→repair loop (a fenced program is lifted instead
///   unless `--generate`);
/// - `AXON_AI_MOCK`: the deterministic mock (tests and audit);
/// - a fenced program: lifted, then `axon check`, the gates and the examples;
/// - none of these: exit 2 naming `--generator`.
fn produce(
    intent: &Intent,
    flags: &GenFlags,
    risk_floor: Option<Risk>,
) -> Result<Resolved, Produce> {
    let mock = std::env::var_os("AXON_AI_MOCK").is_some();
    let lift = intent.fenced_axon.is_some() && !flags.generate;
    if mock && flags.generator.is_some() {
        return Err(Produce::Usage(
            "--generator and AXON_AI_MOCK are exclusive: unset AXON_AI_MOCK to generate".into(),
        ));
    }
    if mock {
        let mode = if flags.generate {
            GenMode::ForceGenerate
        } else {
            GenMode::Auto
        };
        return pipeline::resolve(intent, &MockSynth::new(), mode, risk_floor)
            .map_err(Produce::Refused);
    }
    let bin = axon_bin();
    let resolved = if lift {
        let source = intent.fenced_axon.as_deref().unwrap_or_default();
        let cfg = LoopConfig::new(bin, String::new());
        let (rep, resolved) = genloop::evaluate_lifted(intent, source, &cfg);
        // No generation ran, so there is no `axon-intent-gen/1` transcript:
        // the lifted program's own report (check, gates, every example run)
        // is written instead. The genviab benchmark grades hidden examples
        // with it.
        if let Some(path) = &flags.transcript {
            let doc = serde_json::json!({ "schema": genloop::LIFT_REPORT_SCHEMA, "report": &rep });
            let text = serde_json::to_string_pretty(&doc).expect("the lift report serialises");
            if let Err(e) = std::fs::write(path, text) {
                return Err(Produce::Usage(format!(
                    "cannot write the transcript {}: {e}",
                    path.display()
                )));
            }
        }
        match resolved {
            Some(r) => r,
            None => return Err(Produce::Refused(rep.refusal())),
        }
    } else {
        let Some(spec) = flags.generator.as_deref() else {
            return Err(Produce::Usage(
                "the intent has no fenced ```axon program: pass --generator cmd:PATH|ai:MODEL \
                 to generate one (or AXON_AI_MOCK=1 for the deterministic mock)"
                    .into(),
            ));
        };
        let gen =
            generator::parse_spec(spec, generator::timeout_from_env()).map_err(Produce::Usage)?;
        let card = genloop::load_card(&bin, genloop::CHECK_TIMEOUT)
            .map_err(|e| Produce::Usage(format!("the language card is unavailable: {e}")))?;
        let mut cfg = LoopConfig::new(bin, card);
        if let Some(n) = flags.max_rounds {
            cfg.max_repair_rounds = n;
        }
        let out = genloop::run(intent, gen.as_ref(), &cfg);
        if let Some(path) = &flags.transcript {
            if let Err(e) = std::fs::write(path, out.transcript.to_json()) {
                return Err(Produce::Usage(format!(
                    "cannot write the transcript {}: {e}",
                    path.display()
                )));
            }
        }
        out.resolved.map_err(Produce::Refused)?
    };
    match risk_floor {
        // The loop gated without a floor; the floor only raises the request.
        Some(_) => {
            pipeline::resolve_draft(intent, resolved.draft, risk_floor).map_err(Produce::Refused)
        }
        None => Ok(resolved),
    }
}

/// Report a `produce` failure the way every subcommand does.
fn produce_failed(cmd: &str, p: Produce) -> ExitCode {
    match p {
        Produce::Usage(m) => {
            eprintln!("axon-intent {cmd}: {m}");
            ExitCode::from(2)
        }
        Produce::Refused(r) => {
            println!("{}", r.legible());
            ExitCode::from(r.exit_code() as u8)
        }
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
    // `## Examples` fixtures written `from <file>` resolve under the intent
    // file's own directory, never outside it (IG1).
    let base = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    let mut i = intent::parse_with_base(&src, Some(base)).map_err(|r| r.legible())?;
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
    let mut flags = GenFlags::default();
    let mut i = 0;
    while i < rest.len() {
        match flags.take(rest, i) {
            Ok(0) => {}
            Ok(n) => {
                i += n;
                continue;
            }
            Err(m) => {
                eprintln!("axon-intent compile: {m}");
                return ExitCode::from(2);
            }
        }
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
    let resolved = match produce(&intent, &flags, risk_floor) {
        Ok(r) => r,
        Err(p) => return produce_failed("compile", p),
    };
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
    if decision == Decision::Rejected {
        println!("\u{2717} rejected by {by}");
        return ExitCode::from(3);
    }
    let key = match axon_os::ApprovalKey::from_env() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("axon-intent approve: {e}");
            return ExitCode::from(2);
        }
    };
    // G1e: the closure the program loads, resolved by the interpreter in the
    // SAME environment shape axon-os gives the job's run (env_clear + the
    // reproducible-branch forwarding + the program's directory as cwd), so
    // the digest approved is the digest the run is pinned to.
    let prog_path = program.unwrap_or_else(|| manifest.program.clone());
    let imports = match AxonCoreRuntime::from_env().imports_digest(&prog_path, &manifest.grant) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "axon-intent approve: cannot determine the program's import closure, so there \
                 is nothing to bind it to: {e}"
            );
            return ExitCode::from(2);
        }
    };
    // Build the token directly from the on-disk (program, grant) — the content we
    // bind is EXACTLY what is on disk, so a later one-byte edit invalidates it.
    let risk = crate::approval::derive_risk(&manifest.grant);
    let pd = crate::approval::program_digest(&prog_src);
    let gd = crate::approval::grant_digest(&manifest.grant);
    let token = build_token_raw(&pd, &gd, &imports, by, decision, risk, key.as_ref());

    let name = name_of(Path::new(job));
    let out = Path::new(job).parent().unwrap_or(Path::new("."));
    let path = out.join(format!("{name}.approval"));
    if let Err(e) = std::fs::write(&path, crate::approval::to_json(&token)) {
        eprintln!("axon-intent approve: cannot write {}: {e}", path.display());
        return ExitCode::from(2);
    }
    println!(
        "\u{2713} approved by {by} \u{2014} token bound to program {} grant {} imports {}",
        short(&pd),
        short(&gd),
        short(&imports)
    );
    println!(
        "  {}",
        if key.is_some() {
            "keyed (axmac1): only an AXON_APPROVAL_KEY holder could have made it"
        } else {
            "unkeyed (axtok3): integrity only — anyone can mint one; set AXON_APPROVAL_KEY to key it"
        }
    );
    println!("  Wrote {}", path.display());
    ExitCode::from(0)
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
    let mut flags = GenFlags::default();
    let mut i = 0;
    while i < rest.len() {
        match flags.take(rest, i) {
            Ok(0) => {}
            Ok(n) => {
                i += n;
                continue;
            }
            Err(m) => {
                eprintln!("axon-intent emit: {m}");
                return ExitCode::from(2);
            }
        }
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
    let resolved = match produce(&intent, &flags, None) {
        Ok(r) => r,
        Err(p) => return produce_failed("emit", p),
    };
    let name = name_of(path);
    let triple = match emit::write_compiled(&resolved, &intent, &out, &name) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("axon-intent emit: cannot write triple: {e}");
            return ExitCode::from(2);
        }
    };
    print_resolved(&resolved);
    if !yes {
        eprintln!("axon-intent emit: pass --yes to approve non-interactively");
        return ExitCode::from(2);
    }
    if resolved.draft.generated {
        // A generated program is approved by a human who read it (IG5): the
        // triple stays unapproved, so it cannot run until `approve`.
        eprintln!(
            "axon-intent emit: --yes never approves a generated program; review it and run \
             `axon-intent approve {} --by {by} --accept`",
            triple.manifest.display()
        );
        return ExitCode::from(2);
    }
    let key = match axon_os::ApprovalKey::from_env() {
        Ok(k) => k,
        Err(e) => {
            eprintln!("axon-intent emit: {e}");
            return ExitCode::from(2);
        }
    };
    // The closure of the program as WRITTEN, resolved in the run's environment
    // shape (see `approve`).
    let imports = match AxonCoreRuntime::from_env().imports_digest(&triple.program, &resolved.grant)
    {
        Ok(d) => d,
        Err(e) => {
            eprintln!("axon-intent emit: cannot determine the program's import closure: {e}");
            return ExitCode::from(2);
        }
    };
    match emit::write_approval(
        &resolved,
        &out,
        &name,
        &imports,
        by,
        Decision::Approved,
        key.as_ref(),
    ) {
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
/// triple on disk, check that its approval token binds what would run under
/// the environment's `AXON_APPROVAL_KEY`: the program and grant, AND the
/// import closure. It runs axon-os's one authorization
/// ([`axon_os::approval::authorize`], the `/1` empty-closure rule included)
/// and then compares the closure the token binds with the one the program
/// loads now (`axon imports`, via [`AxonCoreRuntime::from_env`]).
///
/// So it starts the interpreter for every token, and returns `Err` when the
/// closure cannot be determined (no interpreter, a module that does not
/// resolve) or has changed since approval. A triple with no token is `Err`.
/// This is the substance R21's `run` calls (see the CROSS-CRATE HOOK).
pub fn verify_triple(job: &Path) -> Result<(), String> {
    verify_triple_with(job, &AxonCoreRuntime::from_env())
}

/// [`verify_triple`] against the runtime `rt`, which reads the program
/// (once: the token is verified against that text) and resolves its import
/// closure.
pub fn verify_triple_with(job: &Path, rt: &impl Runtime) -> Result<(), String> {
    let src =
        std::fs::read_to_string(job).map_err(|e| format!("cannot read {}: {e}", job.display()))?;
    let base = job.parent().unwrap_or(Path::new("."));
    let manifest = manifest::parse(&src, base).map_err(|v| v.legible())?;
    let prog_src = rt
        .load_program(&manifest.program)
        .map_err(|e| format!("cannot read program {}: {e}", manifest.program.display()))?;
    let key = axon_os::ApprovalKey::from_env()?;
    let auth = axon_os::approval::authorize(job, &manifest, Some(&prog_src), key.as_ref(), rt)?;
    if auth.token.is_none() {
        return Err(format!(
            "no approval token at {}",
            job.with_extension("approval").display()
        ));
    }
    if let Some(bound) = &auth.required_imports {
        let loaded = rt
            .imports_digest(&manifest.program, &manifest.grant)
            .map_err(|e| {
                format!("the program's imports could not be determined (imports_digest): {e}")
            })?;
        if &loaded != bound {
            return Err(format!(
                "imports changed since approval: the token binds imports_digest {bound}, the \
                 program loads {loaded}"
            ));
        }
    }
    Ok(())
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
