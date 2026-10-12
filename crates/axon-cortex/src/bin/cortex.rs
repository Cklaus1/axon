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
//! | 27 | adjudicated, not clean | the check passed and nothing regressed, but the file was already failing others — THE PATCH IS KEPT |
//!
//! Note what 0 means here: a hidden check the generator never saw accepted the
//! result. It is not "the loop finished", and no other outcome is rounded up
//! to it.

use axon_cortex::action::SymbolRef;
use axon_cortex::runner::{
    CheckRegistry, EditGrant, EpisodeOutcome, FabricDispatch, FabricSubmitExecutor,
    LocalInterpreterExecutor, Runner, LOCAL_AXON_TEST_ID,
};

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
  --candidates N         how many ranked candidates to try (default: 3). On
                         the measured corpus the top candidate is the sole
                         most-suspicious one about a third of the time, while
                         the truth reaches rank 3 in roughly nine cases in ten
                         — which is why the run walks rather than stops at one.
                         Each attempt starts from the state this run found, and
                         the hidden check adjudicates every one of them. See
                         crates/axon-cortex/benchmarks/ for the current
                         figures; they are not quoted here because a number
                         embedded in --help outlives the measurement it came
                         from.
  --ranker SPEC          spectrum | openrouter:MODEL | decision:MODEL. Reorders
                         the --candidates names above before the walk; it can
                         never add or drop one. Default: env CORTEX_RANKER,
                         else openrouter:anthropic/claude-haiku-5.5, which
                         needs OPENROUTER_API_KEY and a `--features ai` build.
                         Any failure (no key, HTTP error, 20 s timeout, an
                         answer that is not a permutation) keeps the spectrum
                         order and prints `ranker SPEC unavailable: REASON`.
                         The model sees the file without the --check function,
                         the visible failing checks and the candidates — never
                         the check that adjudicates. Ignored with --symbol.
                         After ranking, one `escalation:` line (and
                         `escalation` in --json) gives cheap-vs-strong advice
                         for the generator, derived from the ranking with no
                         extra call: `cheap` whenever a ranker answered,
                         `unavailable` when none did. Advice only.
  --audit SPEC           off | openrouter:MODEL | decision:MODEL. After a
                         VERIFIED fix, asks one model whether the patch games
                         the tests (special-cases their inputs, hardcodes
                         their outputs, weakens a check) and prints `audit
                         SPEC: clean|suspect (...)` or `audit SPEC
                         unavailable: REASON`. Review only: it never changes
                         the verdict, the exit code or the file. Default: env
                         CORTEX_AUDIT, else
                         openrouter:anthropic/claude-haiku-5.5. The model sees
                         the file as found without the --check function, the
                         visible failing checks and the diff.
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
  --axon PATH            the axon binary to check with (default: axon). It is
                         resolved and its sha256 PINNED at startup; a check is
                         refused, before anything is spawned, if those bytes
                         change during the run.
  --fabric-journal FILE  route EVERY check through the Fabric submit path: each
                         dispatch becomes an acf-compute-request/1 handed to the
                         `axon-fabric` binary named (by path AND sha256) in
                         --check-registry, which rechecks the authority epoch,
                         journals intent/launch/terminal to FILE and returns an
                         acf-execution-receipt/1. Requires --check-registry,
                         --fabric-store, --fabric-tenant, --fabric-family,
                         --fabric-epoch, --fabric-grant-registry,
                         --fabric-principal, --fabric-grant-ref and
                         --fabric-policy-digest (none is defaulted). Receipts
                         appear under `receipts` in --json. A refused or
                         verdict-less dispatch is exit 22.
  --fabric-store DIR     the axon-loop store holding the authority epoch
  --fabric-tenant T / --fabric-family F   the scope within it
  --fabric-epoch N       the epoch this run was authorized under
  --fabric-grant-registry FILE  the operator's axon-fabric-grant-registry/1
  --fabric-principal P / --fabric-grant-ref G   who the checks run as, and
                         the registry grant (bound to P) they run under
  --fabric-policy-digest acf1:HEX   the governing policy (all-zero refused)
  --check-registry FILE  a `cortex-check-registry/1` JSON file naming the
                         checker by path AND sha256 (entry id
                         `axon-test-local`). Verified on load; a mismatch or a
                         missing entry exits 22 with nothing run. When given,
                         the registered binary replaces --axon.
  --label-log PATH       append ONE `cortex-label/1` JSON line per run that
                         reached the walk: the file's sha256 as found, the
                         check, the spectrum ranking with scores, the ranker's
                         order, every attempt, the outcome and exit code, and
                         `truth` = the repaired symbol (null unless
                         verified_done). Default: env CORTEX_LABEL_LOG, else
                         off. Written after the verdict: a write failure is
                         one stderr line and never changes the exit code.
  --label-source         also record the file's text as found in that line.
                         Off by default: source can be sensitive.
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

/// Build the Runner with its check executor REGISTERED before any check runs
/// (B264). The checker is resolved from a trusted registry — the operator's
/// `--check-registry` file, whose digests are stated up front, or else the
/// operator's `--axon`, resolved once and pinned by sha256 now. Nothing a
/// check request carries can choose the program.
///
/// A registration failure exits 22 (environment) with nothing spawned: the
/// same code an unrunnable checker has always had, because it is the same
/// fact — no verdict can be produced.
/// `--fabric-*` flags. All operator input.
#[derive(Default)]
struct FabricArgs {
    journal: Option<std::path::PathBuf>,
    store: Option<std::path::PathBuf>,
    tenant: Option<String>,
    family: Option<String>,
    epoch: Option<u64>,
    grant_registry: Option<std::path::PathBuf>,
    principal: Option<String>,
    grant_ref: Option<String>,
    policy_digest: Option<String>,
}

impl FabricArgs {
    /// Consume one `--fabric-*` flag; false if `flag` is not one.
    fn take(&mut self, flag: &str, val: &mut dyn FnMut(&str) -> String) -> bool {
        match flag {
            "--fabric-journal" => self.journal = Some(val(flag).into()),
            "--fabric-store" => self.store = Some(val(flag).into()),
            "--fabric-tenant" => self.tenant = Some(val(flag)),
            "--fabric-family" => self.family = Some(val(flag)),
            "--fabric-grant-registry" => self.grant_registry = Some(val(flag).into()),
            "--fabric-principal" => self.principal = Some(val(flag)),
            "--fabric-grant-ref" => self.grant_ref = Some(val(flag)),
            "--fabric-policy-digest" => self.policy_digest = Some(val(flag)),
            "--fabric-epoch" => {
                let raw = val(flag);
                self.epoch = Some(raw.parse().unwrap_or_else(|_| {
                    usage(&format!("--fabric-epoch must be a number, got `{raw}`"))
                }))
            }
            _ => return false,
        }
        true
    }
}

/// The Fabric dispatch, if one was asked for. `Arc` so the CLI can read the
/// receipts back after the Runner has used the executor.
fn fabric_executor(
    f: &FabricArgs,
    check_registry: Option<&std::path::Path>,
) -> Option<std::sync::Arc<FabricSubmitExecutor>> {
    let journal = f.journal.clone()?;
    let need = |v: Option<String>, name: &str| {
        v.unwrap_or_else(|| usage(&format!("--fabric-journal requires {name}")))
    };
    let cfg = FabricDispatch {
        registry_file: check_registry
            .unwrap_or_else(|| usage("--fabric-journal requires --check-registry"))
            .to_path_buf(),
        journal,
        store: f
            .store
            .clone()
            .unwrap_or_else(|| usage("--fabric-journal requires --fabric-store")),
        tenant: need(f.tenant.clone(), "--fabric-tenant"),
        family: need(f.family.clone(), "--fabric-family"),
        expected_epoch: f
            .epoch
            .unwrap_or_else(|| usage("--fabric-journal requires --fabric-epoch")),
        // Authority is OPERATOR input, never defaulted: this used to hard-code
        // principal `cortex:repair` and an all-zero policy digest (D-016).
        grant_registry: f
            .grant_registry
            .clone()
            .unwrap_or_else(|| usage("--fabric-journal requires --fabric-grant-registry")),
        principal_ref: need(f.principal.clone(), "--fabric-principal"),
        grant_ref: need(f.grant_ref.clone(), "--fabric-grant-ref"),
        policy_digest: need(f.policy_digest.clone(), "--fabric-policy-digest"),
        task_id: "cortex-repair".into(),
    };
    match FabricSubmitExecutor::new(cfg) {
        Ok(x) => Some(std::sync::Arc::new(x)),
        Err(e) => {
            eprintln!("the checks could not be run: {e}");
            std::process::exit(22);
        }
    }
}

/// Adapter so the Runner can own a handle while the CLI keeps another.
struct SharedFabric(std::sync::Arc<FabricSubmitExecutor>);

impl axon_cortex::runner::CheckExecutor for SharedFabric {
    fn id(&self) -> String {
        self.0.id()
    }
    fn run_checks(
        &self,
        req: &axon_cortex::runner::CheckRequest<'_>,
    ) -> std::io::Result<axon_cortex::runner::CheckReport> {
        self.0.run_checks(req)
    }
}

fn registered_runner(
    axon_bin: &std::path::Path,
    check_registry: Option<&std::path::Path>,
    workspace: &std::path::Path,
    fabric: Option<&std::sync::Arc<FabricSubmitExecutor>>,
) -> Runner {
    let reg = match check_registry {
        Some(file) => CheckRegistry::load(file),
        None => {
            let mut r = CheckRegistry::new();
            r.register_pinned(LOCAL_AXON_TEST_ID, axon_bin)
                .map(|()| r)
                .map_err(|e| e.to_string())
        }
    };
    let exec = reg.and_then(|r| {
        let exe = r
            .get(LOCAL_AXON_TEST_ID)
            .map_err(|e| e.to_string())?
            .path
            .clone();
        LocalInterpreterExecutor::from_registry(&r)
            .map(|x| (exe, x))
            .map_err(|e| e.to_string())
    });
    match exec {
        Ok((exe, _)) if fabric.is_some() => Runner::with_check_executor(
            exe,
            workspace,
            Box::new(SharedFabric(std::sync::Arc::clone(fabric.expect("some")))),
        ),
        Ok((exe, x)) => Runner::with_check_executor(exe, workspace, Box::new(x)),
        Err(e) => {
            eprintln!("the checks could not be run: {e}");
            std::process::exit(22);
        }
    }
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
    let mut check_registry: Option<std::path::PathBuf> = None;
    let mut fabric_args = FabricArgs::default();
    let mut write_prefixes: Vec<String> = Vec::new();
    let mut json = false;
    let mut candidates: usize = 3;
    let mut ranker_flag: Option<String> = None;
    let mut audit_flag: Option<String> = None;
    let mut label_log_flag: Option<String> = None;
    let mut label_source = false;

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
            "--ranker" => ranker_flag = Some(val("--ranker")),
            "--audit" => audit_flag = Some(val("--audit")),
            "--label-log" => label_log_flag = Some(val("--label-log")),
            "--label-source" => label_source = true,
            "--axon" => axon_bin = std::path::PathBuf::from(val("--axon")),
            "--check-registry" => {
                check_registry = Some(std::path::PathBuf::from(val("--check-registry")))
            }
            f if f.starts_with("--fabric-") => {
                if !fabric_args.take(f, &mut val) {
                    usage(&format!("unknown argument `{f}`"))
                }
            }
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
    // Also before anything runs: a mistyped ranker is a command-line error,
    // not a fallback to spectrum that a reader would take for a choice.
    let ranker_spec = axon_cortex::rank::select(
        ranker_flag.as_deref(),
        std::env::var(axon_cortex::rank::RANKER_ENV).ok().as_deref(),
    )
    .unwrap_or_else(|e| usage(&e));
    let mut ranker_report: Option<axon_cortex::rank::RankReport> = None;
    let mut escalation: Option<axon_cortex::rank::Escalation> = None;
    let audit_spec = axon_cortex::audit::select(
        audit_flag.as_deref(),
        std::env::var(axon_cortex::audit::AUDIT_ENV).ok().as_deref(),
    )
    .unwrap_or_else(|e| usage(&e));
    let label_log = axon_cortex::label::select(
        label_log_flag.as_deref(),
        std::env::var(axon_cortex::label::LABEL_ENV).ok().as_deref(),
    )
    .unwrap_or_else(|e| usage(&e));
    if label_source && label_log.is_none() {
        usage("--label-source needs a label log: pass --label-log PATH or set CORTEX_LABEL_LOG");
    }
    // What localization found, kept only for the label row.
    let mut localization: Option<axon_cortex::label::Localization> = None;

    let fabric = fabric_executor(&fabric_args, check_registry.as_deref());
    let mut runner = registered_runner(
        &axon_bin,
        check_registry.as_deref(),
        &workspace,
        fabric.as_ref(),
    );
    // (A grep for `fn NAME(` used to stand here as a cheaper first pass. It
    // was redundant with the check below and strictly worse: it accepted any
    // FUNCTION, so a `--check` naming an ordinary function passed it and was
    // then caught downstream anyway. Two mechanisms behind one question, and
    // the weaker one first — which also made the stronger one's own case
    // impossible to test, because both produced exit 2 for the same input.)

    // THE ADJUDICATOR MUST EXIST, MUST BE A TEST, AND MUST CURRENTLY FAIL.
    //
    // Exit 0 means "a hidden check accepted the result", which is worth
    // nothing if the check would have accepted the file BEFORE any repair —
    // measured on real code, 54 of 80 runs reported verified_done, most at
    // step 1, having changed nothing.
    //
    // An earlier version also grepped the source for `fn NAME(`. That accepts
    // any function, which is precisely the hole: a non-test name passed the
    // grep, matched no test, and the run proceeded. Running the check answers
    // all three questions at once.
    match runner.run_hidden_check(&file, &check) {
        Err(e) => {
            eprintln!("the adjudicating check could not be run: {e}");
            std::process::exit(22);
        }
        // It ran and PASSED. It would accept this file unchanged, so it cannot
        // witness a repair of it.
        Ok(Some(true)) => {
            eprintln!(
                "`{check}` already passes on {file}, so it cannot witness a \
                 repair: it would accept this file unchanged. Name a check \
                 that currently FAILS, or there is nothing here to prove."
            );
            std::process::exit(26);
        }
        // NO TEST BY THAT NAME RAN. Distinct from "it failed", and treating
        // the two alike is what let a `--check` naming an ordinary function
        // through: the run proceeded and then graded itself against a set that
        // never contained the name.
        Ok(None) => usage(&format!(
            "no test named `{check}` ran in {file}. `--check` must name an \
             @[test] function: a name matching nothing can never accept a \
             repair, and a name that is merely a FUNCTION is not an \
             adjudicator at all."
        )),
        Ok(Some(false)) => {}
    }

    // The ordered list of functions this run may try.
    //
    // An explicit --symbol is an INSTRUCTION, not a hypothesis to second-guess:
    // the operator may know something the failing checks do not show. Absent
    // one, the candidates come from the spectrum ranking.
    //
    // Measured on the real corpus — one defect injected per function across
    // `examples/**.ax`, five defect classes — the top-ranked candidate is the
    // sole most-suspicious one far less often than the truth reaches the top
    // THREE. Stopping at the first therefore throws away cases the evidence
    // could already decide, so the run walks the list. The hidden check
    // adjudicates every attempt, so walking further trades budget for coverage
    // and never trades away correctness.
    //
    // The figures live in `benchmarks/` and are deliberately not repeated
    // here: two earlier versions of this comment outlived the measurement they
    // quoted, and one of them was still advertising a number the benchmark
    // README had explicitly withdrawn.
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
        // THE RANKING STEP. It may only permute these names: the eligible set
        // is fixed above, by evidence, before any model is consulted. Every
        // failure keeps the spectrum order and says so on one line.
        let located = ranked.len();
        let eligible: Vec<String> = ranked
            .iter()
            .take(candidates)
            .map(|(n, _)| n.clone())
            .collect();
        let report = axon_cortex::rank::Ranker::from_env(ranker_spec.clone()).rank(
            &axon_cortex::rank::RankInput {
                file: &file,
                source: &src,
                hidden_check: &check,
                failing: &failing,
                eligible: &eligible,
            },
        );
        if let Some(line) = report.fallback_line() {
            eprintln!("{line}");
        } else if report.reason.is_none() && report.used != "spectrum" {
            eprintln!(
                "ranker {}: trying {} ({} ms, ${})",
                report.used,
                report.order.join(" then "),
                report.latency_ms.unwrap_or(0),
                report
                    .cost_usd
                    .map_or_else(|| "?".to_string(), |c| format!("{c:.6}"))
            );
        }
        // Advice for the operator's next choice of generator, from the
        // ranking alone. It changes nothing in this run.
        let advice = axon_cortex::rank::escalation(&report, located);
        eprintln!("{}", advice.line());
        escalation = Some(advice);
        let order = report.order.clone();
        ranker_report = Some(report);
        if label_log.is_some() {
            localization = Some(axon_cortex::label::Localization {
                failing,
                passing,
                ranked,
                candidates,
                eligible,
            });
        }
        order
    };

    // Each attempt starts from the state this run FOUND, not from whatever the
    // previous attempt left behind. Without the restore, a compiling-but-wrong
    // patch to candidate 1 is still in the file when candidate 2 is tried, so
    // the second attempt is graded against code the first one damaged — and a
    // success there would not mean what it says.
    let original = std::fs::read_to_string(workspace.join(&file))
        .unwrap_or_else(|e| usage(&format!("cannot read {}/{file}: {e}", workspace.display())));

    // BREADTH-FIRST: every candidate gets ONE proposal before any gets a
    // second.
    //
    // The walk used to be depth-first — each candidate ran its whole budget
    // before the next was tried. Measured with a real model, the slow runs
    // were the ones whose truth ranked below the top: every wrong candidate
    // ahead of it spent several long proposals before giving up, and then the
    // true candidate succeeded on its FIRST. Breadth lets a first-proposal
    // success at rank k cost k proposals instead of k-1 whole episodes plus
    // one. (The figures are in the CHANGELOG entry, not here; see the note on
    // the candidate list above.)
    //
    // Pass 2 resumes, in rank order, every candidate pass 1 PAUSED, each with
    // the rest of its own step budget and the history its episode already
    // has (`CandidateProgress`). So no candidate is ever given more proposals
    // or steps than the depth-first walk gave it, and a candidate pass 1
    // stopped for a reason of its own — going in circles, no such symbol, no
    // generator — is not tried again. One target (an explicit --symbol, or a
    // ranking with one name) is never paused, so it runs exactly as before.
    let mut outcome = EpisodeOutcome::NeedsInput {
        steps: 0,
        what: "no candidate was attempted".to_string(),
    };
    let mut progress: Vec<axon_cortex::runner::CandidateProgress> =
        targets.iter().map(|_| Default::default()).collect();
    // Every attempt, in the order it ran. A candidate resumed in pass 2
    // appears twice.
    let mut attempted: Vec<String> = Vec::new();
    let mut prev: Option<usize> = None;
    let mut context_reported = vec![false; targets.len()];
    'walk: for pass in 1..=2 {
        for (i, target) in targets.iter().enumerate() {
            if pass == 2 && !progress[i].paused() {
                continue;
            }
            progress[i].proposal_limit = (pass == 1 && targets.len() > 1).then_some(1);
            if let Some(p) = prev {
                if let Err(e) = std::fs::write(workspace.join(&file), &original) {
                    eprintln!("cannot restore {file} between attempts: {e}");
                    std::process::exit(22);
                }
                if pass == 1 {
                    eprintln!("`{}` did not repair it; trying `{target}`", targets[p]);
                } else {
                    eprintln!(
                        "`{}` did not repair it; resuming `{target}` with {} of its {budget} \
                         step(s) left",
                        targets[p],
                        budget - progress[i].steps()
                    );
                }
            }
            let snap = match runner.snapshot(&[file.as_str()]) {
                Ok(s) => s,
                Err(e) => usage(&format!("cannot read {}/{file}: {e}", workspace.display())),
            };
            // A fresh grant per attempt, pinned to the state that attempt
            // starts from. Reusing one across attempts would hand the second
            // attempt authority issued over a workspace the first one changed.
            let grant = EditGrant {
                grant_id: format!("cortex-repair-{}", attempted.len()),
                principal: principal.clone(),
                snapshot_id: snap.snapshot_id.clone(),
                write_prefixes: write_prefixes.clone(),
            };
            attempted.push(target.clone());
            prev = Some(i);
            outcome = runner.run_episode_resumable(
                &SymbolRef {
                    path: file.clone(),
                    symbol: target.clone(),
                },
                Some(&grant),
                &principal,
                &check,
                budget,
                generator.as_deref(),
                &mut progress[i],
            );
            // A generator shown less than the view is said so, once per
            // candidate, when it happens — never left for a reader to infer
            // from a worse result.
            if !context_reported[i] {
                if let Some(c) = progress[i].generator_context() {
                    context_reported[i] = true;
                    if let Some(reason) = c.fallback_reason() {
                        eprintln!(
                            "generator context for `{target}` unavailable: {reason}; \
                             using the body-only prompt"
                        );
                    }
                }
            }
            match &outcome {
                // The only success. Everything else is a reason to try the
                // next attempt, and running out of attempts reports the LAST
                // reason rather than inventing a summary of all of them.
                EpisodeOutcome::VerifiedDone { .. } => break 'walk,
                // Authority and environment failures are not about this
                // candidate. Another one would fail identically and burn the
                // budget proving it.
                EpisodeOutcome::Refused { .. } | EpisodeOutcome::Blocked { .. } => break 'walk,
                _ => {}
            }
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
        EpisodeOutcome::AdjudicatedNotClean {
            steps,
            still_failing,
        } => (
            27,
            "adjudicated_not_clean",
            format!(
                "`{check}` passed and nothing regressed, but {} check(s) that were already \
                 failing still fail: {}",
                still_failing.len(),
                still_failing.join(", ")
            ),
            *steps,
        ),
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
    // Exit 27 KEEPS its patch: the adjudicator accepted it and it broke
    // nothing, so discarding it would throw away work that succeeded on the
    // only question this run was asked.
    if code != 0 && code != 27 {
        let _ = std::fs::write(workspace.join(&file), &original);
    }
    let repaired = (code == 0).then(|| attempted.last().cloned().unwrap_or_default());

    // THE AUDIT, after the verdict, the exit code and the file are all
    // settled: it reads them and can change none of them. Only a verified fix
    // is audited; anything else has nothing to review.
    let audit = repaired.as_ref().map(|sym| {
        let failing: &[String] = prev
            .and_then(|i| progress[i].baseline_failing())
            .unwrap_or(&[]);
        let report = match std::fs::read_to_string(workspace.join(&file)) {
            Ok(patched) => axon_cortex::audit::Auditor::from_env(audit_spec.clone()).audit(
                &axon_cortex::audit::AuditInput {
                    file: &file,
                    original: &original,
                    patched: &patched,
                    symbol: sym,
                    hidden_check: &check,
                    failing,
                },
            ),
            Err(e) => axon_cortex::audit::AuditReport {
                requested: audit_spec.label(),
                used: None,
                verdict: axon_cortex::audit::Verdict::Unavailable,
                p: None,
                latency_ms: None,
                cost_usd: None,
                reason: Some(format!("cannot read the patched {file}: {e}")),
            },
        };
        if let Some(line) = report.line() {
            eprintln!("{line}");
        }
        report
    });

    // THE LABEL ROW, after the verdict, the exit code, the file and the audit
    // are settled. It reads them and changes none of them: a failed append is
    // one stderr line and the run exits exactly as it would have.
    if let Some(path) = &label_log {
        let costs = targets
            .iter()
            .zip(&progress)
            .filter(|(t, _)| attempted.contains(t))
            .map(|(t, p)| axon_cortex::label::CandidateCost {
                symbol: t.clone(),
                proposals: p.proposals(),
                steps: p.steps(),
            })
            .collect();
        let row = axon_cortex::label::LabelRow {
            ts: axon_cortex::label::utc_timestamp(std::time::SystemTime::now()),
            cortex_sha256: axon_cortex::label::build_sha256(),
            file: &file,
            source: &original,
            include_source: label_source,
            check: &check,
            symbol: (!symbol.is_empty()).then_some(symbol.as_str()),
            generator: &generator_spec,
            budget,
            localization: localization.as_ref(),
            ranker: ranker_report.as_ref().map(|r| r.to_json()),
            attempted: &attempted,
            outcome: kind,
            exit_code: code,
            steps,
            truth: repaired.as_deref(),
            costs,
            audit: audit.as_ref().map(|a| a.to_json()),
        };
        if let Err(e) = axon_cortex::label::append(path, &row.to_json()) {
            eprintln!(
                "label log {}: cannot append ({e}); the verdict and exit code are unaffected",
                path.display()
            );
        }
    }

    if json {
        println!(
            "{}",
            serde_json::json!({
                "schema": "cortex-repair/1",
                "outcome": kind,
                "detail": detail,
                "steps": steps,
                // WHICH candidate worked, and every attempt before it, in the
                // order they ran. With a walked ranking that is no longer
                // implied by the command line. Under the breadth-first walk a
                // candidate resumed in pass 2 appears a second time, so this
                // is an attempt log, not a set of candidates.
                "symbol": repaired,
                "attempted": attempted,
                "exit_code": code,
                // The episode, verbatim. Every check that ran, every patch
                // applied with its before/after digests, and which generator
                // proposed it. The verdict above is a summary OF this, not a
                // substitute for it.
                // THE EPISODE AS DATA, not as a Rust `Debug` string.
                //
                // It was emitted as `format!("{:?}", …)` embedded in a JSON
                // string, so the one artifact shipped to a consumer could not
                // be parsed by `parse_strict`, its digest could not be
                // recomputed, and `verified_ok()` could not be re-evaluated —
                // while `episode.rs` describes it as append-only and
                // replayable "so a replay can prove it re-ran the same episode
                // rather than a similar one". Nothing in the output supported
                // that claim.
                "episode": serde_json::to_value(&runner.episode)
                    .unwrap_or(serde_json::Value::Null),
                // The digest travels WITH it, so a reader can recompute and
                // compare rather than trusting the transport. An error here is
                // reported rather than swallowed: a digest that could not be
                // computed is not a digest that matched.
                "episode_digest": match runner.episode.digest() {
                    Ok(d) => serde_json::Value::String(d),
                    Err(e) => serde_json::Value::String(format!("<undigestable: {e}>")),
                },
                // Present only under --fabric-journal: the acf-execution-receipt/1
                // of every check this run dispatched, in order.
                "receipts": fabric.as_ref().map(|f| f.receipts()),
                // The ranking step: what was asked for, what ordered the walk,
                // and why they differ when they do. `null` with --symbol,
                // where nothing was ranked.
                "ranker": ranker_report.as_ref().map(|r| r.to_json()),
                // Cheap-vs-strong advice derived from that ranking. `null`
                // where nothing was ranked.
                "escalation": escalation.as_ref().map(|e| e.to_json()),
                // The review-only tamper audit of a verified fix. `null`
                // when nothing was verified.
                "audit": audit.as_ref().map(|a| a.to_json()),
                // What each candidate's generator was shown: `view` (the file
                // without the hidden check, the visible failing checks and
                // their output) or `body_only`, with the reason. Only
                // candidates a generator was asked about appear.
                "generator_context": targets
                    .iter()
                    .zip(&progress)
                    .filter_map(|(t, p)| {
                        p.generator_context().map(|c| {
                            serde_json::json!({
                                "symbol": t,
                                "used": if c.fallback_reason().is_some() {
                                    "body_only"
                                } else {
                                    "view"
                                },
                                "reason": c.fallback_reason(),
                            })
                        })
                    })
                    .collect::<Vec<_>>(),
            })
        );
    } else {
        match &repaired {
            Some(sym) => println!("{kind} after {steps} step(s) — repaired `{sym}`"),
            None => println!("{kind} after {steps} step(s)"),
        }
        // WHAT THE WHOLE RUN DID, not only its last episode.
        //
        // The walk tries several candidates and the verdict above describes
        // the FINAL one — so a defect that no single-symbol patch can repair
        // came back as "going in circles", a statement about one episode's
        // stuck-detection rather than about the run. Measured on 16 defects
        // deliberately placed outside what one `PatchSymbolBody` can express,
        // that was the only thing an operator was told.
        //
        // Restricted to the outcomes where episodes actually RAN and failed:
        // saying "tried N candidates" after an authority refusal or a missing
        // target would describe attempts that never happened, which is the
        // same overstatement pointing the other way. Exit 27 is excluded too —
        // there a candidate DID satisfy the check, and its own detail says
        // what remained.
        //
        // CANDIDATES, not attempts: the breadth-first walk resumes a candidate
        // in its second pass, so `attempted` can name one twice. The attempt
        // count is added only when it differs, so a run that never resumed
        // reads exactly as it always did.
        if matches!(code, 20 | 21) && !attempted.is_empty() {
            let mut distinct: Vec<&str> = Vec::new();
            for a in &attempted {
                if !distinct.contains(&a.as_str()) {
                    distinct.push(a);
                }
            }
            let attempts = if attempted.len() > distinct.len() {
                format!(" in {} attempt(s)", attempted.len())
            } else {
                String::new()
            };
            println!(
                "  tried {} candidate(s) ({}){attempts}; none satisfied `{check}`, so no \
                 single-symbol repair among them was enough",
                distinct.len(),
                distinct.join(", ")
            );
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
    let mut check_registry: Option<std::path::PathBuf> = None;
    let mut fabric_args = FabricArgs::default();
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
            "--check-registry" => {
                check_registry = Some(std::path::PathBuf::from(val("--check-registry")))
            }
            f if f.starts_with("--fabric-") => {
                if !fabric_args.take(f, &mut val) {
                    usage(&format!("unknown argument `{f}`"))
                }
            }
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
    let fabric = fabric_executor(&fabric_args, check_registry.as_deref());
    let runner = registered_runner(
        &axon_bin,
        check_registry.as_deref(),
        &workspace,
        fabric.as_ref(),
    );
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
                "receipts": fabric.as_ref().map(|f| f.receipts()),
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
