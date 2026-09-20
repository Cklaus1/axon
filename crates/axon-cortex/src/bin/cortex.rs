//! `cortex repair` — the production caller for the repair episode.
//!
//! Until this existed, every capability in `axon-cortex` was reachable only
//! from its own tests: observation, typed actions, authority, execution,
//! verification and the episode loop that drives them. A mechanism with tests
//! and no caller is not built — it ships in the binary and reads as done.
//!
//! What this binary is NOT is a general agent runner. It runs ONE episode
//! against ONE symbol and reports how it ended, because that is the unit Cortex
//! has contracts for. The model contributes a function body and nothing else.
//!
//! ## Exit codes
//!
//! Distinct per outcome, because the remedies differ and a single non-zero
//! would collapse "I need a patch body" into "the checker would not run":
//!
//! | code | outcome | what to do about it |
//! |---|---|---|
//! | 0 | verified done | nothing — a hidden check confirmed the repair |
//! | 2 | usage error | fix the command line; NOTHING was decided or run |
//! | 20 | budget exhausted | raise `--budget`, or the task is too hard for this loop |
//! | 21 | no progress | the loop is going in circles; the generator is not helping |
//! | 22 | blocked | the environment is wrong — the checker could not run |
//! | 23 | refused | authority is wrong — widen `--write-prefix`, deliberately |
//! | 24 | needs input | no usable proposal — supply or fix `--generator` |
//!
//! Note what 0 means here: a hidden check the generator never saw accepted the
//! result. It is not "the loop finished", and no other outcome is rounded up
//! to it.

use axon_cortex::action::SymbolRef;
use axon_cortex::runner::{EditGrant, EpisodeOutcome, Runner};

const USAGE: &str = "\
cortex repair --file PATH --symbol NAME --check NAME [options]

  --workspace DIR        directory to operate in (default: .)
  --file PATH            workspace-relative file holding the symbol
  --symbol NAME          the function whose body may be replaced
  --check NAME           the check that adjudicates the claim. It is never
                         shown to the generator: a result graded by something
                         the author could read is not evidence.
  --write-prefix P       path prefix the episode may write (repeatable).
                         NO default: with none given nothing may be written,
                         matching Cortex's convention where \"\" denies.
  --principal NAME       who is acting (default: agent)
  --budget N             maximum steps (default: 8)
  --generator SPEC       none (default) | ai:MODEL | literal:BODY
                         literal: applies a body you already know through the
                         same grant check and the same hidden check a model's
                         would face. Knowing the answer is a reason to skip the
                         model, not the adjudication.
  --axon PATH            the axon binary to check with (default: axon)
  --json                 machine-readable outcome on stdout
";

fn usage(msg: &str) -> ! {
    // Exit 2 with NO outcome. A malformed command line means no episode ran,
    // and printing an outcome here would report a typo as a repair verdict.
    eprintln!("{msg}\n\n{USAGE}");
    std::process::exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("repair") => {}
        Some("--help" | "-h") | None => {
            println!("{USAGE}");
            return;
        }
        Some(other) => usage(&format!("unknown command `{other}`")),
    }

    let mut workspace = std::path::PathBuf::from(".");
    let mut file = String::new();
    let mut symbol = String::new();
    let mut check = String::new();
    let mut principal = "agent".to_string();
    let mut budget: usize = 8;
    let mut generator_spec = "none".to_string();
    let mut axon_bin = std::path::PathBuf::from("axon");
    let mut write_prefixes: Vec<String> = Vec::new();
    let mut json = false;

    while let Some(a) = args.next() {
        let mut val = |flag: &str| -> String {
            args.next()
                .unwrap_or_else(|| usage(&format!("{flag} needs a value")))
        };
        match a.as_str() {
            "--workspace" => workspace = std::path::PathBuf::from(val("--workspace")),
            "--file" => file = val("--file"),
            "--symbol" => symbol = val("--symbol"),
            "--check" => check = val("--check"),
            "--principal" => principal = val("--principal"),
            "--generator" => generator_spec = val("--generator"),
            "--axon" => axon_bin = std::path::PathBuf::from(val("--axon")),
            "--write-prefix" => write_prefixes.push(val("--write-prefix")),
            "--budget" => {
                let raw = val("--budget");
                budget = raw
                    .parse()
                    .unwrap_or_else(|_| usage(&format!("--budget must be a number, got `{raw}`")));
                if budget == 0 {
                    usage("--budget must be at least 1");
                }
            }
            "--json" => json = true,
            other => usage(&format!("unknown argument `{other}`")),
        }
    }
    for (name, v) in [
        ("--file", &file),
        ("--symbol", &symbol),
        ("--check", &check),
    ] {
        if v.is_empty() {
            usage(&format!("{name} is required"));
        }
    }

    // Resolved here and not inside the loop: an unknown generator spec is a
    // command-line error, and discovering it three steps in would report a typo
    // as NeedsInput — a verdict about the task rather than about the request.
    #[allow(unused_variables)]
    let generator: Option<Box<dyn axon_cortex::generate::PatchGenerator>> =
        match generator_spec.as_str() {
            "none" => None,
            // `literal:BODY` applies a body the operator already knows,
            // through the full pipeline rather than around it.
            spec if spec.starts_with("literal:") => Some(Box::new(
                axon_cortex::generate::LiteralGenerator::new(&spec["literal:".len()..]),
            )),
            spec => match spec.strip_prefix("ai:") {
                #[cfg(feature = "ai")]
                Some(model) if !model.is_empty() => {
                    Some(Box::new(axon_cortex::ai::AiPatchGenerator::new(model)))
                }
                #[cfg(not(feature = "ai"))]
                Some(_) => usage(
                    "this build has no model-backed generator: rebuild with \
                     `--features ai`. Refusing to run with `none` instead, \
                     which would report a build without a generator as a task \
                     that needed no repair.",
                ),
                _ => usage(&format!(
                    "unknown --generator `{spec}` (expected `none` or `ai:MODEL`)"
                )),
            },
        };

    let mut runner = Runner::new(&axon_bin, &workspace);
    let snap = match runner.snapshot(&[file.as_str()]) {
        Ok(s) => s,
        Err(e) => usage(&format!("cannot read {}/{file}: {e}", workspace.display())),
    };
    // The grant is pinned to the state it was issued over. That is what makes
    // a stale-authority refusal possible at all, and it is taken here rather
    // than inside the loop so the loop cannot re-issue authority to itself.
    let grant = EditGrant {
        grant_id: "cortex-repair".to_string(),
        principal: principal.clone(),
        snapshot_id: snap.snapshot_id.clone(),
        write_prefixes,
    };

    let outcome = runner.run_episode(
        &SymbolRef {
            path: file.clone(),
            symbol: symbol.clone(),
        },
        Some(&grant),
        &principal,
        &check,
        budget,
        generator.as_deref(),
    );

    let (code, kind, detail, steps) = match &outcome {
        EpisodeOutcome::VerifiedDone { steps } => (0, "verified_done", String::new(), *steps),
        EpisodeOutcome::BudgetExhausted { steps } => {
            (20, "budget_exhausted", String::new(), *steps)
        }
        EpisodeOutcome::NoProgress { steps, action } => (21, "no_progress", action.clone(), *steps),
        EpisodeOutcome::Blocked { steps, reason } => (22, "blocked", reason.clone(), *steps),
        EpisodeOutcome::Refused { steps, reason } => (23, "refused", reason.clone(), *steps),
        EpisodeOutcome::NeedsInput { steps, what } => (24, "needs_input", what.clone(), *steps),
    };

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": "cortex-repair/1",
                "outcome": kind,
                "detail": detail,
                "steps": steps,
                "exit_code": code,
                // The episode, verbatim. Every check that ran, every patch
                // applied with its before/after digests, and which generator
                // proposed it. The verdict above is a summary OF this, not a
                // substitute for it.
                "episode": format!("{:?}", runner.episode),
            })
        );
    } else {
        println!("{kind} after {steps} step(s)");
        if !detail.is_empty() {
            println!("  {detail}");
        }
    }
    std::process::exit(code);
}
