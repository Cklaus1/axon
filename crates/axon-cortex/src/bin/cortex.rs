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
//! | 25 | no target | no candidate could be established, or the named symbol does not exist |
//! | 26 | check cannot witness | `--check` already passes; it would accept the file unchanged |
//!
//! Note what 0 means here: a hidden check the generator never saw accepted the
//! result. It is not "the loop finished", and no other outcome is rounded up
//! to it.

use axon_cortex::action::SymbolRef;
use axon_cortex::runner::{EditGrant, EpisodeOutcome, Runner};

const USAGE: &str = "\
cortex repair --file PATH --check NAME [--symbol NAME] [options]
cortex locate --file PATH --check NAME [--workspace DIR] [--axon PATH] [--json]

  --workspace DIR        directory to operate in (default: .)
  --file PATH            workspace-relative file holding the symbol
  --symbol NAME          the function whose body may be replaced. OPTIONAL:
                         without it, Cortex ranks the candidates from the
                         checks that fail and tries the top --candidates of
                         them in order, restoring the file between attempts.
                         A test is never a candidate, and the check named by
                         --check can never be patched.
  --check NAME           the check that adjudicates the claim. It is never
                         shown to the generator: a result graded by something
                         the author could read is not evidence.
  --write-prefix P       path prefix the episode may write (repeatable).
                         NO default: with none given nothing may be written,
                         matching Cortex's convention where \"\" denies.
  --principal NAME       who is acting (default: agent)
  --budget N             maximum steps per candidate (default: 8)
  --candidates N         how many ranked candidates to try (default: 3). Top-1
                         is right 57.5% of the time on the real corpus and
                         top-3 covers 90%; each attempt starts from the state
                         this run found, and the hidden check adjudicates every
                         one of them.
  --generator SPEC       none (default) | ai:MODEL | cmd:PATH | literal:BODY
                         cmd: runs YOUR program — the prompt on its stdin, the
                         proposed body on its stdout. No credentials enter this
                         process and no provider is baked in. A model still
                         chooses nothing: you name the command here, before any
                         model is involved, and what it returns is validated,
                         authorized and adjudicated like any other proposal.
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

/// Exit 25: no target could be established.
///
/// Its own code, separate from the usage error above and from every episode
/// outcome below. The command line was fine and no episode ran, so reporting
/// it as either would send an operator to fix the wrong thing.
fn no_target(reason: &str) -> ! {
    eprintln!("could not localize a repair target: {reason}");
    std::process::exit(25)
}

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("repair") => {}
        // `locate` answers "what is broken?" without repairing anything. It
        // exists because the RANKING is the part whose quality has to be
        // measurable: a verdict that collapses the list to one name cannot be
        // scored for whether the right answer was second.
        Some("locate") => return locate_only(args),
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
    let mut candidates: usize = 3;

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
            "--candidates" => {
                let raw = val("--candidates");
                candidates = raw.parse().unwrap_or_else(|_| {
                    usage(&format!("--candidates must be a number, got `{raw}`"))
                });
                if candidates == 0 {
                    usage("--candidates must be at least 1");
                }
            }
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
    // --symbol is absent from this list on purpose: it is optional, and an
    // absent one is localized below rather than rejected.
    for (name, v) in [("--file", &file), ("--check", &check)] {
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
            // `cmd:PATH` — the operator's own program. Prompt on stdin, body
            // on stdout. A model still chooses nothing: the command is named
            // here, on the command line, before any model is involved.
            spec if spec.starts_with("cmd:") => Some(Box::new(
                axon_cortex::generate::CommandGenerator::new(&spec["cmd:".len()..]),
            )),
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

    // The adjudicating check must EXIST before anything runs.
    //
    // A check that matches nothing exits 0 and reports zero tests, which the
    // runner already refuses to call a pass — so a typo here does not produce
    // a false success. What it produces is worse to debug: the claim is
    // refused every time, the loop patches and re-patches, and the operator
    // gets exit 21 "going in circles" for a mistyped flag. The loop would be
    // reporting, accurately, on an experiment that could never have concluded.
    // It judges ONLY what it can establish by reading the file. An unreadable
    // file or an unrunnable checker is an ENVIRONMENT failure, and the episode
    // reports those as Blocked (22) with the observer's own reason. Answering
    // them here as a usage error would tell an operator their command line was
    // wrong when their toolchain is.
    if let Ok(src) = std::fs::read_to_string(workspace.join(&file)) {
        if !src.contains(&format!("fn {check}(")) {
            usage(&format!(
                "no check named `{check}` in {file}: a check that matches \
                 nothing can never accept a repair, so the episode would run \
                 its whole budget and report going in circles"
            ));
        }
    }

    // THE ADJUDICATOR MUST BE ABLE TO WITNESS THE REPAIR.
    //
    // Exit 0 means "a hidden check accepted the result". That is worth nothing
    // if the check would have accepted the file BEFORE any repair — and
    // measured on real code, that was the common case rather than an edge one:
    // 54 of 80 runs reported verified_done, most of them at step 1, having
    // changed nothing at all. The check passed because it never exercised the
    // broken function.
    //
    // A check that passes on the unrepaired file is not a weak adjudicator; it
    // is not an adjudicator. Refusing here is the same discipline the runner
    // applies when a filter matches zero tests: an absent verdict must not be
    // reported as a favourable one.
    // Naming the adjudicator as the repair target is a REQUEST error, caught
    // before anything runs. The episode refuses it too, but saying so here
    // names the mistake instead of reporting it as a refused action.
    if symbol == check {
        usage(&format!(
            "--symbol and --check are both `{check}`: patching the check that \
             decides whether the work is done would grade the repair against \
             bytes the repair just wrote"
        ));
    }

    match runner.run_hidden_check(&file, &check) {
        Err(e) => {
            eprintln!("the adjudicating check could not be run: {e}");
            std::process::exit(22);
        }
        Ok((true, _)) => {
            eprintln!(
                "`{check}` already passes on {file}, so it cannot witness a \
                 repair: it would accept this file unchanged. Name a check that \
                 currently FAILS, or there is nothing here to prove."
            );
            std::process::exit(26);
        }
        Ok((false, 0)) => {
            eprintln!(
                "`{check}` matched no test in {file}. A filter matching nothing \
                 exits 0 and reports ok, which is why this is checked rather \
                 than inferred from the exit code."
            );
            std::process::exit(2);
        }
        Ok((false, _)) => {}
    }

    // The ordered list of functions this run may try.
    //
    // An explicit --symbol is an INSTRUCTION, not a hypothesis to second-guess:
    // the operator may know something the failing checks do not show. Absent
    // one, the candidates come from the spectrum ranking.
    //
    // Measured on the real corpus — a defect injected into each function of
    // `examples/**.ax`, one at a time — the top-ranked candidate is right 57.5%
    // of the time and the top THREE cover 90%. Stopping at the first throws
    // away a third of the cases the evidence could already decide, so the run
    // walks the list. The hidden check adjudicates every attempt, so walking
    // further trades budget for coverage and never trades away correctness.
    let targets: Vec<String> = if !symbol.is_empty() {
        vec![symbol.clone()]
    } else {
        let src = std::fs::read_to_string(workspace.join(&file)).unwrap_or_default();
        let (failing, passing) = match runner.check_outcomes(&file, &check) {
            Ok(v) => v,
            // An unrunnable checker is an environment failure with its own
            // code, never an empty spectrum: no failing check is what a
            // HEALTHY file looks like.
            Err(e) => {
                eprintln!("the checks could not be run: {e}");
                std::process::exit(22);
            }
        };
        if failing.is_empty() {
            no_target(
                "no check fails, so there is nothing to localize; pass --symbol \
                 to attempt a repair anyway",
            );
        }
        let ranked = axon_cortex::locate::rank(&src, &failing, &passing);
        if ranked.is_empty() {
            no_target(&format!(
                "check(s) {} fail but reach no function defined in {file}; the \
                 defect may be in a callee, a builtin, or the check itself",
                failing.join(", ")
            ));
        }
        // Show the working, WITH the scores. A target arrived at silently
        // cannot be told from a guess, and the spread between first and second
        // is what says whether the evidence decided anything at all.
        let shown: Vec<String> = ranked
            .iter()
            .take(candidates)
            .map(|(n, sc)| format!("{n} ({sc:.2})"))
            .collect();
        eprintln!(
            "from failing check(s) {}: trying {}",
            failing.join(", "),
            shown.join(" then ")
        );
        ranked
            .into_iter()
            .take(candidates)
            .map(|(n, _)| n)
            .collect()
    };

    // Each attempt starts from the state this run FOUND, not from whatever the
    // previous attempt left behind. Without the restore, a compiling-but-wrong
    // patch to candidate 1 is still in the file when candidate 2 is tried, so
    // the second attempt is graded against code the first one damaged — and a
    // success there would not mean what it says.
    let original = std::fs::read_to_string(workspace.join(&file))
        .unwrap_or_else(|e| usage(&format!("cannot read {}/{file}: {e}", workspace.display())));

    let mut outcome = EpisodeOutcome::NeedsInput {
        steps: 0,
        what: "no candidate was attempted".to_string(),
    };
    let mut attempted: Vec<String> = Vec::new();
    for (i, target) in targets.iter().enumerate() {
        if i > 0 {
            if let Err(e) = std::fs::write(workspace.join(&file), &original) {
                eprintln!("cannot restore {file} between attempts: {e}");
                std::process::exit(22);
            }
            eprintln!("`{}` did not repair it; trying `{target}`", targets[i - 1]);
        }
        let snap = match runner.snapshot(&[file.as_str()]) {
            Ok(s) => s,
            Err(e) => usage(&format!("cannot read {}/{file}: {e}", workspace.display())),
        };
        // A fresh grant per attempt, pinned to the state that attempt starts
        // from. Reusing one across attempts would hand the second attempt
        // authority issued over a workspace the first one changed.
        let grant = EditGrant {
            grant_id: format!("cortex-repair-{i}"),
            principal: principal.clone(),
            snapshot_id: snap.snapshot_id.clone(),
            write_prefixes: write_prefixes.clone(),
        };
        attempted.push(target.clone());
        outcome = runner.run_episode(
            &SymbolRef {
                path: file.clone(),
                symbol: target.clone(),
            },
            Some(&grant),
            &principal,
            &check,
            budget,
            generator.as_deref(),
        );
        match &outcome {
            // The only success. Everything else is a reason to try the next
            // candidate, and running out of candidates reports the LAST reason
            // rather than inventing a summary of all of them.
            EpisodeOutcome::VerifiedDone { .. } => break,
            // Authority and environment failures are not about this candidate.
            // Another one would fail identically and burn the budget proving
            // it.
            EpisodeOutcome::Refused { .. } | EpisodeOutcome::Blocked { .. } => break,
            _ => {}
        }
    }

    let (code, kind, detail, steps) = match &outcome {
        EpisodeOutcome::VerifiedDone { steps } => (0, "verified_done", String::new(), *steps),
        EpisodeOutcome::BudgetExhausted { steps } => {
            (20, "budget_exhausted", String::new(), *steps)
        }
        EpisodeOutcome::NoProgress { steps, action } => (21, "no_progress", action.clone(), *steps),
        EpisodeOutcome::Blocked { steps, reason } => (22, "blocked", reason.clone(), *steps),
        EpisodeOutcome::Refused { steps, reason } => (23, "refused", reason.clone(), *steps),
        EpisodeOutcome::NeedsInput { steps, what } => (24, "needs_input", what.clone(), *steps),
        EpisodeOutcome::NoSuchSymbol {
            steps,
            symbol,
            path,
        } => (
            25,
            "no_such_symbol",
            format!("`{symbol}` is not defined in {path}"),
            *steps,
        ),
    };
    // Nothing worked: the file is left exactly as it was found. A run that
    // reports failure while having rewritten a function is reporting on a
    // workspace nobody asked for.
    if code != 0 {
        let _ = std::fs::write(workspace.join(&file), &original);
    }
    let repaired = (code == 0).then(|| attempted.last().cloned().unwrap_or_default());

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": "cortex-repair/1",
                "outcome": kind,
                "detail": detail,
                "steps": steps,
                // WHICH candidate worked, and which were tried before it. With
                // a walked ranking that is no longer implied by the command
                // line.
                "symbol": repaired,
                "attempted": attempted,
                "exit_code": code,
                // The episode, verbatim. Every check that ran, every patch
                // applied with its before/after digests, and which generator
                // proposed it. The verdict above is a summary OF this, not a
                // substitute for it.
                "episode": format!("{:?}", runner.episode),
            })
        );
    } else {
        match &repaired {
            Some(sym) => println!("{kind} after {steps} step(s) — repaired `{sym}`"),
            None => println!("{kind} after {steps} step(s)"),
        }
        if !detail.is_empty() {
            println!("  {detail}");
        }
    }
    std::process::exit(code);
}

/// `cortex locate --file F --check C [--json]`
fn locate_only(mut args: impl Iterator<Item = String>) {
    let mut workspace = std::path::PathBuf::from(".");
    let (mut file, mut check) = (String::new(), String::new());
    let mut axon_bin = std::path::PathBuf::from("axon");
    let mut json = false;
    while let Some(a) = args.next() {
        let mut val = |flag: &str| -> String {
            args.next()
                .unwrap_or_else(|| usage(&format!("{flag} needs a value")))
        };
        match a.as_str() {
            "--workspace" => workspace = std::path::PathBuf::from(val("--workspace")),
            "--file" => file = val("--file"),
            "--check" => check = val("--check"),
            "--axon" => axon_bin = std::path::PathBuf::from(val("--axon")),
            "--json" => json = true,
            other => usage(&format!("unknown argument `{other}`")),
        }
    }
    if file.is_empty() {
        usage("--file is required");
    }
    // Required, not optional. Omitted, `check_outcomes` excludes nothing and
    // the ADJUDICATING test becomes evidence for choosing what to repair —
    // which makes the target a function of the answer, the one thing this
    // module exists to prevent. The JSON gave no sign the spectrum was
    // contaminated.
    if check.is_empty() {
        usage(
            "--check is required: without it the adjudicating test is counted \
             as evidence, and the target becomes a function of the answer",
        );
    }
    let runner = Runner::new(&axon_bin, &workspace);
    let src = std::fs::read_to_string(workspace.join(&file))
        .unwrap_or_else(|e| usage(&format!("cannot read {file}: {e}")));
    // An unrunnable checker is reported as such, never as an empty spectrum:
    // no failing check is what a HEALTHY file looks like.
    let (failing, passing) = match runner.check_outcomes(&file, &check) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("the checks could not be run: {e}");
            std::process::exit(22);
        }
    };
    let ranked = axon_cortex::locate::rank(&src, &failing, &passing);
    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": "cortex-locate/1",
                "failing": failing,
                "passing": passing,
                // Every candidate WITH its score, not just the winner. The
                // spread between first and second is what says whether the
                // evidence actually decided anything.
                "ranked": ranked.iter()
                    .map(|(n, s)| serde_json::json!({"symbol": n, "score": s}))
                    .collect::<Vec<_>>(),
            })
        );
    } else if ranked.is_empty() {
        println!("no candidate: {} failing check(s)", failing.len());
    } else {
        for (n, sc) in &ranked {
            println!("{sc:.4}  {n}");
        }
    }
}
