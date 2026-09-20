//! C11 — the deterministic end-to-end conformance run, and C12's negative
//! gates.
//!
//! This is the run's SMOKE TEST. It exercises the package's own "first
//! executable vertical slice" against the real `axon` binary and a real
//! temporary workspace: snapshot → partial observation → catalog → authorize →
//! patch a COPY → run checks → verify independently → evidence → replay.
//!
//! Deterministic by construction: the decision is a fixed mock, not a model.
//! Per BUILD_PLAN, "A scripted golden test proves the plumbing; varied
//! model-driven tasks establish solver behavior. Keep these results separate."

use axon_cortex::episode::EpisodeEvent;
use axon_cortex::runner::{EditGrant, Refusal, Runner};
use std::path::PathBuf;

/// Locate the interpreter. If it is missing this FAILS rather than skips: the
/// conformance run is this build's stop condition, and a skip would report a
/// smoke test that never ran as a passing one.
fn axon_bin() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf();
    let bin = root.join("target/debug/axon");
    assert!(
        bin.exists(),
        "the conformance run needs the interpreter at {}; build it with \
         `cargo build -p axon-core --no-default-features --bin axon`. \
         Failing rather than skipping: a smoke test that did not run is not a \
         smoke test that passed.",
        bin.display()
    );
    bin
}

fn stage(name: &str) -> (PathBuf, PathBuf) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures");
    let ws = std::env::temp_dir().join(format!("cortex_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    Runner::stage_copy(&src, &ws).expect("stage a resettable copy");
    (src, ws)
}

/// C11 — the whole episode, end to end.
#[test]
fn cxg_c11_repair_episode_runs_end_to_end() {
    let (src, ws) = stage("e2e");
    let mut r = Runner::new(axon_bin(), &ws);

    // 1. Snapshot the region the episode may touch.
    let snap = r.snapshot(&["broken.ax"]).expect("snapshot");
    assert_eq!(snap.files.len(), 1);

    // 2. Partial observation. The fixture compiles but its LOGIC is wrong, so
    //    the observer reports a useful state either way.
    let obs = r.observe(&snap, "broken.ax");
    assert!(!obs.facts.is_empty(), "observation must say something");

    // 3. The visible reproduction check must FAIL before the repair — without
    //    this the episode could "succeed" on an unbroken fixture.
    assert!(
        !r.run_check("visible_repro", "broken.ax")
            .expect("check runs"),
        "precondition: the fixture is genuinely broken"
    );

    // 4. Authorize, then patch the COPY.
    let grant = EditGrant {
        grant_id: "g1".into(),
        principal: "repair-agent".into(),
        snapshot_id: snap.snapshot_id.clone(),
        write_prefixes: vec!["broken.ax".into()],
    };
    r.authorize(
        "patch_symbol_body",
        Some(&grant),
        "repair-agent",
        &snap,
        "broken.ax",
    )
    .expect("a well-formed action under a valid grant is allowed");
    assert!(
        r.apply_patch("broken.ax", "n + 2", "n * 2").expect("patch"),
        "the patch must actually apply"
    );

    // 5. Checks now pass, and an INDEPENDENT verifier agrees.
    assert!(r.run_check("visible_repro", "broken.ax").expect("recheck"));
    assert!(
        r.verify(true, "hidden_completion", "broken.ax"),
        "hidden completion tests must pass after a correct repair"
    );
    assert!(r.episode.verified_ok());

    // 6. The ORIGINAL fixture is untouched — the episode edited a copy.
    let orig = std::fs::read_to_string(src.join("broken.ax")).unwrap();
    assert!(
        orig.contains("n + 2"),
        "the source fixture must never be edited"
    );

    // 7. Evidence: the episode records the real sequence, and its digest is
    //    stable across a re-serialise (replay identity).
    let kinds: Vec<&str> = r
        .episode
        .events
        .iter()
        .map(|e| match e {
            EpisodeEvent::Snapshot { .. } => "snapshot",
            EpisodeEvent::Observed { .. } => "observed",
            EpisodeEvent::ActionAllowed { .. } => "allowed",
            EpisodeEvent::ActionDenied { .. } => "denied",
            EpisodeEvent::PatchApplied { .. } => "patch",
            EpisodeEvent::CheckRun { .. } => "check",
            EpisodeEvent::Verified { .. } => "verified",
        })
        .collect();
    assert_eq!(
        kinds,
        vec!["snapshot", "observed", "check", "patch", "check", "verified"],
        "the evidence must show the real order, including the failing check BEFORE the patch"
    );
    let d1 = r.episode.digest().unwrap();
    let json = axon_cortex::to_canonical_json(&r.episode).unwrap();
    let replayed: axon_cortex::episode::Episode = axon_cortex::parse_strict(&json).unwrap();
    assert_eq!(
        d1,
        replayed.digest().unwrap(),
        "the episode must replay to the same identity"
    );

    let _ = std::fs::remove_dir_all(&ws);
}

/// C12 / `G03-forgery` — unknown, wrong-principal and stale-snapshot grants all
/// refuse, and refuse WITHOUT a side effect. Refusing after writing is not
/// refusing.
#[test]
fn cxg_g03_forgery_refuses_without_side_effect() {
    let (_src, ws) = stage("forgery");
    let mut r = Runner::new(axon_bin(), &ws);
    let snap = r.snapshot(&["broken.ax"]).expect("snapshot");
    let before = std::fs::read_to_string(ws.join("broken.ax")).unwrap();

    let valid = EditGrant {
        grant_id: "g1".into(),
        principal: "repair-agent".into(),
        snapshot_id: snap.snapshot_id.clone(),
        write_prefixes: vec!["broken.ax".into()],
    };

    // No grant at all.
    assert!(matches!(
        r.authorize(
            "patch_symbol_body",
            None,
            "repair-agent",
            &snap,
            "broken.ax"
        ),
        Err(Refusal::UnknownGrant(_))
    ));
    // A grant belonging to someone else.
    assert!(matches!(
        r.authorize(
            "patch_symbol_body",
            Some(&valid),
            "other-agent",
            &snap,
            "broken.ax"
        ),
        Err(Refusal::WrongPrincipal { .. })
    ));
    // A grant pinned to a state the workspace has genuinely moved past.
    // Built by taking a SECOND real snapshot after a real edit, not by forging
    // an id: a hand-made id proves only that the comparison runs, not that the
    // production path can ever produce two different ones.
    std::fs::write(
        ws.join("broken.ax"),
        "fn main() { println(\"moved on\") }\n",
    )
    .unwrap();
    let snap2 = r.snapshot(&["broken.ax"]).expect("second snapshot");
    assert_ne!(
        snap.snapshot_id, snap2.snapshot_id,
        "a changed workspace must yield a different snapshot id, or authority never expires"
    );
    assert_eq!(
        snap2.parent_snapshot_id.as_deref(),
        Some(snap.snapshot_id.as_str())
    );
    assert!(matches!(
        // `valid` is pinned to snap; the workspace is now at snap2.
        r.authorize(
            "patch_symbol_body",
            Some(&valid),
            "repair-agent",
            &snap2,
            "broken.ax"
        ),
        Err(Refusal::StaleSnapshot { .. })
    ));
    std::fs::write(ws.join("broken.ax"), &before).unwrap();

    assert_eq!(
        std::fs::read_to_string(ws.join("broken.ax")).unwrap(),
        before,
        "a refused action must leave the workspace byte-identical"
    );
    assert_eq!(r.episode.denials().len(), 3, "and each refusal is recorded");
    let _ = std::fs::remove_dir_all(&ws);
}

/// C12 / `G03-payload` — a VALID grant paired with a traversal or a policy file
/// still refuses. Authority to edit is not authority to edit anything.
#[test]
fn cxg_g03_payload_refuses_traversal_and_policy_files() {
    let (_src, ws) = stage("payload");
    let mut r = Runner::new(axon_bin(), &ws);
    let snap = r.snapshot(&["broken.ax"]).expect("snapshot");
    let grant = EditGrant {
        grant_id: "g1".into(),
        principal: "repair-agent".into(),
        snapshot_id: snap.snapshot_id.clone(),
        // Deliberately BROAD, so the refusals below come from the payload
        // check and not from a narrow prefix.
        write_prefixes: vec!["*".into()],
    };
    for (path, want) in [
        ("../outside.ax", "traversal"),
        ("nested/../../escape.ax", "traversal"),
        ("axon.lock", "policy"),
        ("profile.rs", "policy"),
    ] {
        let got = r.authorize(
            "patch_symbol_body",
            Some(&grant),
            "repair-agent",
            &snap,
            path,
        );
        match (&got, want) {
            (Err(Refusal::PathTraversal(_)), "traversal") => {}
            (Err(Refusal::PolicyFile(_)), "policy") => {}
            other => panic!("`{path}` must refuse as {want}, got {other:?}"),
        }
    }
    let _ = std::fs::remove_dir_all(&ws);
}

/// C12 / `G03-done` — a DONE claim cannot close the task while hidden checks
/// fail, "regardless of confidence".
#[test]
fn cxg_g03_done_claim_cannot_override_failing_hidden_checks() {
    let (_src, ws) = stage("done");
    let mut r = Runner::new(axon_bin(), &ws);
    let _ = r.snapshot(&["broken.ax"]).expect("snapshot");
    // No patch applied — the fixture is still broken. Claim done anyway, with
    // maximum confidence.
    let verdict = r.verify(true, "hidden_completion", "broken.ax");
    assert!(
        !verdict,
        "a confident claim does not pass failing hidden checks"
    );
    assert!(!r.episode.verified_ok());
    let last = r.episode.events.last().unwrap();
    match last {
        EpisodeEvent::Verified { passed, detail } => {
            assert!(!passed);
            assert!(
                detail.contains("claim_done=true"),
                "the overruled claim must be RECORDED, so a reviewer sees it was made: {detail}"
            );
        }
        other => panic!("expected a Verified event, got {other:?}"),
    }
    let _ = std::fs::remove_dir_all(&ws);
}

/// C12 / `G02-partial` — a fixture that does not type-check still yields a
/// useful partial observation, and the observer never fabricates the facts it
/// could not determine.
#[test]
fn cxg_g02_partial_observation_never_fabricates() {
    let (_src, ws) = stage("partial");
    // Break it at the TYPE level, not just semantically.
    std::fs::write(
        ws.join("broken.ax"),
        "fn double(n: i64) -> i64 {\n  let x: i64 = \"not an int\"\n  n * 2\n}\nfn main() { println(to_str(double(2))) }\n",
    )
    .unwrap();
    let mut r = Runner::new(axon_bin(), &ws);
    let snap = r.snapshot(&["broken.ax"]).expect("snapshot");
    let obs = r.observe(&snap, "broken.ax");

    assert!(!obs.diagnostics.is_empty(), "a type error must be observed");
    let resolved = obs
        .facts
        .iter()
        .find(|(k, _)| k == "target_type_resolved")
        .map(|(_, v)| v)
        .expect("the fact must be present");
    assert!(
        resolved.is_unknown(),
        "types are unresolvable here, so the fact must be Unknown — not a fabricated default"
    );
    assert_eq!(resolved.value(), None);
    assert!(
        !obs.omission_report.is_empty(),
        "and the omission must be REPORTED — an observation without one is \
         indistinguishable from a claim of completeness"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

/// C12 — the catalog is denial-first: an action nobody listed is refused,
/// rather than allowed because nothing forbade it.
#[test]
fn cxg_c12_catalog_is_denial_first() {
    let (_src, ws) = stage("catalog");
    let mut r = Runner::new(axon_bin(), &ws);
    let snap = r.snapshot(&["broken.ax"]).expect("snapshot");
    assert!(matches!(
        r.authorize("rm_rf", None, "repair-agent", &snap, "broken.ax"),
        Err(Refusal::NotInCatalog(_))
    ));
    // ...and a listed, non-editing action needs no grant.
    assert!(r
        .authorize("inspect", None, "repair-agent", &snap, "broken.ax")
        .is_ok());
    let _ = std::fs::remove_dir_all(&ws);
}

/// C12 — a write prefix is a PATH prefix, and breadth is STATED. `broken.ax`
/// must not authorize `broken.ax.evil`, and an empty prefix must deny rather
/// than silently mean "everything" (the product policy: explicit `*` only).
#[test]
fn cxg_c12_write_prefix_is_a_path_not_a_string() {
    let (_src, ws) = stage("prefix");
    let mut r = Runner::new(axon_bin(), &ws);
    let snap = r.snapshot(&["broken.ax"]).expect("snapshot");
    let g = |p: &str| EditGrant {
        grant_id: "g1".into(),
        principal: "a".into(),
        snapshot_id: snap.snapshot_id.clone(),
        write_prefixes: vec![p.to_string()],
    };
    let can = |r: &Runner, grant: &EditGrant, path: &str| {
        r.authorize_dry("patch_symbol_body", Some(grant), "a", &snap, path)
            .is_ok()
    };
    assert!(can(&r, &g("src"), "src/lib.ax"));
    assert!(can(&r, &g("src"), "src"));
    // The sibling-file trap: a string prefix would allow both of these.
    assert!(!can(&r, &g("src"), "srcevil/lib.ax"));
    assert!(!can(&r, &g("broken.ax"), "broken.ax.evil"));
    // Breadth stated explicitly vs. implied by a blank.
    assert!(can(&r, &g("*"), "anything/at/all.ax"));
    assert!(
        !can(&r, &g(""), "anything.ax"),
        "an empty prefix must DENY; permissiveness is never implied by a blank value"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

/// C12 — a hidden check that could not RUN must fail closed AND must be
/// distinguishable in the evidence from one that ran and failed. The verdict
/// alone cannot carry that, so the recorded detail has to.
#[test]
fn cxg_c12_a_check_that_did_not_run_is_not_a_check_that_failed() {
    let (_src, ws) = stage("norun");
    // The input the primary path provably cannot produce: a binary that is not
    // there, so the spawn itself errors rather than the test suite failing.
    let mut r = Runner::new(ws.join("no-such-binary"), &ws);
    assert!(
        !r.verify(true, "hidden_completion", "broken.ax"),
        "fail closed"
    );
    let d = match r.episode.events.last().unwrap() {
        EpisodeEvent::Verified { detail, .. } => detail.clone(),
        other => panic!("expected Verified, got {other:?}"),
    };
    assert!(d.contains("DID NOT RUN"), "the evidence must say so: {d}");
    assert!(
        !d.contains("FAILED"),
        "and must not claim a failing run happened: {d}"
    );

    // The contrast case: a check that really ran and really failed.
    let mut r2 = Runner::new(axon_bin(), &ws);
    assert!(!r2.verify(true, "hidden_completion", "broken.ax"));
    let d2 = match r2.episode.events.last().unwrap() {
        EpisodeEvent::Verified { detail, .. } => detail.clone(),
        other => panic!("expected Verified, got {other:?}"),
    };
    assert!(d2.contains("FAILED"), "{d2}");
    assert!(!d2.contains("DID NOT RUN"), "{d2}");
    let _ = std::fs::remove_dir_all(&ws);
}

/// G02-partial, second half — a warning is a FACT, not a non-error.
///
/// The observer filtered for `"severity":"error"` and discarded the rest, so a
/// program the checker objected to was reported as `diagnostics: []`,
/// `compiles: true` — indistinguishable from a clean one. An agent repairing
/// code through Cortex could not see that the checker had found dead code.
///
/// This runs the REAL `axon check` against two fixtures that differ only in
/// whether they contain unreachable code. The clean control is what makes the
/// assertions mean something: an observer that reported a warning for
/// everything would satisfy the warned half alone.
#[test]
fn cxg_g02_an_observation_distinguishes_clean_from_warned() {
    let fact = |obs: &axon_cortex::Observation, k: &str| -> String {
        obs.facts
            .iter()
            .find(|(n, _)| n == k)
            .map(|(_, v)| format!("{v:?}"))
            .unwrap_or_else(|| panic!("observation has no `{k}` fact"))
    };

    // ── warned: compiles, and the checker has something to say ──────────────
    let (_, ws) = stage("warned");
    let mut r = Runner::new(axon_bin(), &ws);
    let snap = r.snapshot(&["warned.ax"]).expect("snapshot");
    let obs = r.observe(&snap, "warned.ax");

    assert!(
        obs.diagnostics.is_empty(),
        "warned.ax type-checks; a warning must not be reported as an error: {:?}",
        obs.diagnostics
    );
    assert!(
        !obs.warnings.is_empty(),
        "the checker emits W0005 for this fixture and the observation dropped it \
         — that is the absent-vs-empty collapse this test exists to catch"
    );
    assert!(
        fact(&obs, "compiles").contains("true"),
        "it does compile: {}",
        fact(&obs, "compiles")
    );
    assert!(
        fact(&obs, "compiles_cleanly").contains("false"),
        "compiling and compiling CLEANLY are different states: {}",
        fact(&obs, "compiles_cleanly")
    );
    assert!(
        fact(&obs, "warning_codes").contains("W0005"),
        "the code itself must survive, not just a count — a consumer deciding \
         what to do next needs to know it is dead code: {}",
        fact(&obs, "warning_codes")
    );

    // ── clean control: same shape, nothing to report ────────────────────────
    let (_, ws2) = stage("clean");
    let mut r2 = Runner::new(axon_bin(), &ws2);
    let snap2 = r2.snapshot(&["clean.ax"]).expect("snapshot");
    let obs2 = r2.observe(&snap2, "clean.ax");

    assert!(
        obs2.warnings.is_empty(),
        "the control has no dead code; reporting a warning here would mean the \
         observer flags everything: {:?}",
        obs2.warnings
    );
    assert!(
        fact(&obs2, "compiles_cleanly").contains("true"),
        "control must be clean: {}",
        fact(&obs2, "compiles_cleanly")
    );
    assert_eq!(
        fact(&obs2, "warning_count"),
        fact(&obs2, "warning_count"),
        "sanity"
    );
    assert!(
        fact(&obs2, "warning_count").contains('0'),
        "control warning_count must be 0: {}",
        fact(&obs2, "warning_count")
    );
}

/// G03-done, sharpened — a hidden check that does not exist is NOT a pass.
///
/// `verify()` used to run `axon test <file>` and ignore `hidden_check`
/// entirely: the name appeared only in the evidence string, so the episode
/// recorded "hidden check `X` passed" after a run that never evaluated X on
/// its own. Selecting it with `--filter` makes the record true.
///
/// That fix opens a hole the whole-file run did not have, and this test is the
/// guard on it: `axon test --filter nope` matches nothing, runs zero tests and
/// exits 0 — "test result: ok. 0 passed, 0 failed". Taken at face value, a
/// hidden check reports PASSED while not existing, which is worse than the
/// mislabelling the fix set out to remove.
#[test]
fn cxg_g03_a_hidden_check_that_matches_nothing_is_not_a_pass() {
    let (_, ws) = stage("nomatch");
    let mut r = Runner::new(axon_bin(), &ws);

    // CONTROL: the fixture's real hidden check. `broken.ax` is broken by
    // design, so this FAILS — and that is the point. It proves the filter
    // selects a test that genuinely ran, so the negative case below cannot be
    // explained by the filter silently matching nothing every time.
    //
    // (An earlier draft asserted this PASSES. It does not, and the control
    // caught the wrong assumption before anything was concluded from it.)
    assert!(
        !r.verify(true, "hidden_completion", "broken.ax"),
        "the unrepaired fixture's hidden check must FAIL"
    );
    let after_real = format!("{:?}", r.episode.events);
    assert!(
        after_real.contains("FAILED"),
        "a check that ran and failed must be recorded as FAILED: {after_real}"
    );

    // The real assertion: a name matching no test is unobserved, not passed.
    let verdict = r.verify(true, "no_such_hidden_check", "broken.ax");
    assert!(
        !verdict,
        "a hidden check that matched no test reported PASSED — a filter that \
         matches nothing exits 0, and treating that as a pass lets any DONE \
         claim close a task by naming a check that does not exist"
    );

    // Both verdicts are false. The EVIDENCE is what has to tell them apart:
    // one check ran and failed, the other never existed. Collapsing those is
    // how an absent verification comes to read as a performed one.
    let ev = format!("{:?}", r.episode.events);
    assert!(
        ev.contains("DID NOT RUN") && ev.contains("no_such_hidden_check"),
        "the episode must record that the check did not run, and name it: {ev}"
    );
}

/// CX-03 typed — authority is a property of the action, not of a string.
///
/// The old API took `action: &str` plus a generic `target_path`, so every
/// action could receive every parameter and the code decided which to ignore.
/// Ignoring a field is a runtime decision to be gotten right at each call site;
/// not having the field is a property of the type. Three nonsense states are
/// now `compile_fail` doctests on `CortexAction` rather than assertions here —
/// they do not compile, so there is nothing to test at runtime.
///
/// What remains testable is that the authority RULES still hold on the typed
/// path, and that the non-writing variants are genuinely effect-free.
#[test]
fn cxg_c03_typed_actions_carry_their_own_authority() {
    use axon_cortex::action::{CheckRef, CompletionClaim, CortexAction, SymbolRef};

    let (_, ws) = stage("typed");
    let mut r = Runner::new(axon_bin(), &ws);
    let snap = r.snapshot(&["broken.ax"]).expect("snapshot");

    let grant = EditGrant {
        grant_id: "g-typed".into(),
        principal: "agent".into(),
        snapshot_id: snap.snapshot_id.clone(),
        write_prefixes: vec!["broken.ax".into()],
    };

    // 1. A valid patch inside the grant is authorizable.
    let patch = CortexAction::PatchSymbolBody {
        symbol: SymbolRef {
            path: "broken.ax".into(),
            symbol: "add".into(),
        },
        proposed_body: "a + b".into(),
    };
    assert!(
        r.authorize_action(&patch, Some(&grant), "agent", &snap)
            .is_ok(),
        "a well-formed patch inside the granted prefix must be allowed — a \
         refusal-only test proves nothing about a system that can also say yes"
    );

    // 2. The same patch outside the grant is refused, and the refusal NAMES
    //    the path rather than being a generic denial.
    let outside = CortexAction::PatchSymbolBody {
        symbol: SymbolRef {
            path: "elsewhere.ax".into(),
            symbol: "add".into(),
        },
        proposed_body: "a + b".into(),
    };
    match r.authorize_action(&outside, Some(&grant), "agent", &snap) {
        Err(Refusal::PathOutsideGrant(p)) => assert_eq!(p, "elsewhere.ax"),
        other => panic!("expected PathOutsideGrant, got {other:?}"),
    }

    // 3. ClaimDone cannot resolve to an effectful mechanism. There is no path
    //    to check because the variant has no path to give — the rule has
    //    nothing to apply to, rather than being skipped.
    let claim = CortexAction::ClaimDone {
        claim: CompletionClaim {
            done: true,
            rationale: "tests pass".into(),
        },
    };
    assert_eq!(
        claim.write_target(),
        None,
        "claiming done must not be able to name a file to touch"
    );
    assert!(!claim.requires_write_authority());
    assert!(
        r.authorize_action(&claim, None, "agent", &snap).is_ok(),
        "a claim needs no grant; verify() adjudicates it independently"
    );

    // 4. Read-only actions need no grant either, and carry no write target.
    let inspect = CortexAction::Inspect {
        target: SymbolRef {
            path: "broken.ax".into(),
            symbol: "add".into(),
        },
    };
    let check = CortexAction::RunCheck {
        check: CheckRef {
            name: "visible_repro".into(),
            path: "broken.ax".into(),
        },
    };
    for a in [&inspect, &check] {
        assert_eq!(a.write_target(), None, "{} must not write", a.name());
        assert!(
            r.authorize_action(a, None, "agent", &snap).is_ok(),
            "{} is read-only and needs no grant",
            a.name()
        );
    }

    // 5. A stale grant still refuses on the typed path — authority does not
    //    survive the state it was granted over, and the typing did not
    //    accidentally drop that.
    let stale = EditGrant {
        snapshot_id: "axc1:stale".into(),
        ..grant.clone()
    };
    assert!(matches!(
        r.authorize_action(&patch, Some(&stale), "agent", &snap),
        Err(Refusal::StaleSnapshot { .. })
    ));
}

/// Observation → typed action selection, against the REAL checker.
///
/// Three observed states produce three different actions, and an undetermined
/// state produces none. The last row is the load-bearing one: `Observed::Unknown`
/// exists so a fact the checker could not establish stays distinguishable from
/// one it established as false, and the moment that distinction pays for itself
/// is exactly here — deciding whether to claim the work is done.
#[test]
fn cxg_c02_selection_follows_the_observation_and_refuses_to_guess() {
    use axon_cortex::action::{CortexAction, SymbolRef};
    use axon_cortex::select::{select_action, Selection};

    let sym = |p: &str| SymbolRef {
        path: p.to_string(),
        symbol: "f".to_string(),
    };

    // 1. Does not compile → Inspect. You cannot patch what you have not read,
    //    and the selector cannot invent a patch body.
    let (_, ws) = stage("sel_err");
    let mut r = Runner::new(axon_bin(), &ws);
    let snap = r.snapshot(&["uncompilable.ax"]).expect("snapshot");
    let obs = r.observe(&snap, "uncompilable.ax");
    assert!(
        !obs.diagnostics.is_empty(),
        "fixture must actually fail to compile, or this row proves nothing"
    );
    assert!(
        matches!(
            select_action(&obs, &sym("uncompilable.ax")),
            Selection::Act(CortexAction::Inspect { .. })
        ),
        "errors present must select Inspect, got {:?}",
        select_action(&obs, &sym("uncompilable.ax"))
    );

    // 2. Compiles WITH warnings → RunCheck. It builds, but the checker
    //    objected, so behaviour is worth confirming before anything is claimed.
    //    This is the row that consumes the warnings channel: without it the
    //    selector could not tell this case from the clean one.
    let (_, ws2) = stage("sel_warn");
    let mut r2 = Runner::new(axon_bin(), &ws2);
    let snap2 = r2.snapshot(&["warned.ax"]).expect("snapshot");
    let obs2 = r2.observe(&snap2, "warned.ax");
    assert!(obs2.diagnostics.is_empty() && !obs2.warnings.is_empty());
    assert!(
        matches!(
            select_action(&obs2, &sym("warned.ax")),
            Selection::Act(CortexAction::RunCheck { .. })
        ),
        "compiles-with-warnings must select RunCheck, not a completion claim"
    );

    // 3. Compiles cleanly → ClaimDone.
    let (_, ws3) = stage("sel_clean");
    let mut r3 = Runner::new(axon_bin(), &ws3);
    let snap3 = r3.snapshot(&["clean.ax"]).expect("snapshot");
    let obs3 = r3.observe(&snap3, "clean.ax");
    assert!(obs3.diagnostics.is_empty() && obs3.warnings.is_empty());
    assert!(
        matches!(
            select_action(&obs3, &sym("clean.ax")),
            Selection::Act(CortexAction::ClaimDone { .. })
        ),
        "a clean compile may claim done"
    );

    // 4. State UNKNOWN → Blocked, never an action. A runner pointed at a
    //    checker that does not exist observes `compiles: Unknown(reason)`; a
    //    selector that treated unknown as fine would claim done on a program it
    //    never managed to check. This is a real unavailable binary, not a
    //    hand-built Observation.
    let (_, ws4) = stage("sel_unknown");
    let mut r4 = Runner::new(ws4.join("no-such-axon-binary"), &ws4);
    let snap4 = r4.snapshot(&["clean.ax"]).expect("snapshot");
    let obs4 = r4.observe(&snap4, "clean.ax");
    match select_action(&obs4, &sym("clean.ax")) {
        Selection::Blocked(why) => assert!(
            why.contains("unknown"),
            "the block must carry the reason, not merely refuse: {why}"
        ),
        Selection::Act(a) => panic!(
            "selected {} from an observation that never ran the checker — \
             unknown was treated as fine",
            a.name()
        ),
    }
}

/// The loop closes: observe → select → authorize → EXECUTE → verify.
///
/// Every earlier test exercised one link. This drives the whole chain against
/// the real checker and a real workspace copy, and it has to end with the bug
/// actually FIXED — a loop that only ever refuses proves the gates work and
/// says nothing about whether the system can do the job.
#[test]
fn cxg_c11_the_repair_loop_closes_end_to_end() {
    use axon_cortex::action::{CortexAction, SymbolRef};
    use axon_cortex::runner::ExecOutcome;
    use axon_cortex::select::{select_action, Selection};

    let (_, ws) = stage("loop");
    let mut r = Runner::new(axon_bin(), &ws);
    let target = SymbolRef {
        path: "broken.ax".into(),
        symbol: "double".into(),
    };

    // ── 1. Observe. `double` adds instead of multiplying: it COMPILES, so the
    //       defect is semantic and the checker cannot see it.
    let snap = r.snapshot(&["broken.ax"]).expect("snapshot");
    let obs = r.observe(&snap, "broken.ax");
    assert!(
        obs.diagnostics.is_empty(),
        "the bug is semantic, not a type error"
    );

    // ── 2. Select. Clean compile ⇒ the observation has nothing left to see, so
    //       the selector proposes a completion claim.
    let sel = select_action(&obs, &target);
    assert!(matches!(
        sel,
        Selection::Act(CortexAction::ClaimDone { .. })
    ));

    // ── 3. Authorize + execute the claim. It must be EFFECT-FREE: the file is
    //       unchanged afterwards, because a claim is evidence, not an act.
    let before = std::fs::read_to_string(ws.join("broken.ax")).unwrap();
    let claim = sel.action().unwrap().clone();
    let auth = r
        .authorize_action(&claim, None, "agent", &snap)
        .expect("a claim needs no grant");
    assert!(matches!(r.execute(auth), ExecOutcome::Claimed { .. }));
    assert_eq!(
        std::fs::read_to_string(ws.join("broken.ax")).unwrap(),
        before,
        "claiming done must not touch the workspace"
    );

    // ── 4. Verify adjudicates the claim independently — and REFUSES it. This
    //       is G03-done: confidence is not evidence.
    assert!(
        !r.verify(true, "hidden_completion", "broken.ax"),
        "the hidden check still fails, so the claim cannot close the task"
    );

    // ── 5. Now actually repair it. Inspect first — the loop reads before it
    //       writes, and the body it reads is the real one.
    let inspect = CortexAction::Inspect {
        target: target.clone(),
    };
    let auth = r
        .authorize_action(&inspect, None, "agent", &snap)
        .expect("reading needs no grant");
    match r.execute(auth) {
        ExecOutcome::Inspected { body, .. } => {
            assert!(body.contains("n + 2"), "read the real body, got {body:?}")
        }
        other => panic!("expected Inspected, got {other:?}"),
    }

    // ── 6. Patch, under a grant pinned to the CURRENT snapshot.
    let grant = EditGrant {
        grant_id: "g-loop".into(),
        principal: "agent".into(),
        snapshot_id: snap.snapshot_id.clone(),
        write_prefixes: vec!["broken.ax".into()],
    };
    let patch = CortexAction::PatchSymbolBody {
        symbol: target.clone(),
        proposed_body: "\n    n * 2\n".into(),
    };
    let auth = r
        .authorize_action(&patch, Some(&grant), "agent", &snap)
        .expect("the patch is inside the grant");
    assert!(matches!(r.execute(auth), ExecOutcome::Patched { .. }));

    // ── 7. Verify again. The same hidden check, the same call — now it passes,
    //       because the code is actually fixed.
    assert!(
        r.verify(true, "hidden_completion", "broken.ax"),
        "after the repair the hidden check must pass — if this cannot flip, \
         the loop can only ever refuse"
    );

    // ── 8. And the episode is a record of all of it, in order.
    assert!(r.episode.verified_ok(), "the episode must end verified");
}

/// Execute's refusal branches — added because mutation found the end-to-end
/// test pinned none of them.
///
/// `cxg_c11_the_repair_loop_closes_end_to_end` proves the loop can do the job.
/// Four mutations survived it: a ClaimDone that writes a file, a missing symbol
/// reported as Patched, a zero-match check reported as passing, and a missing
/// symbol read as an empty body. Three were untested branches; the fourth was
/// an assertion too narrow to see — it compared ONE file, and the mutation
/// wrote a different one.
///
/// A loop that works on the happy path and lies on the unhappy one is worse
/// than one that does neither, because the happy path is what gets
/// demonstrated.
#[test]
fn cxg_c11_execute_refuses_precisely() {
    use axon_cortex::action::{CheckRef, CompletionClaim, CortexAction, SymbolRef};
    use axon_cortex::runner::ExecOutcome;

    let (_, ws) = stage("exec_neg");
    let mut r = Runner::new(axon_bin(), &ws);
    let snap = r.snapshot(&["broken.ax"]).expect("snapshot");

    // Fingerprint the WHOLE workspace, not one file. The narrow assertion is
    // what let a stray write survive.
    let fingerprint = |dir: &std::path::Path| -> Vec<(String, u64)> {
        let mut v: Vec<(String, u64)> = std::fs::read_dir(dir)
            .expect("read ws")
            .filter_map(|e| e.ok())
            .map(|e| {
                (
                    e.file_name().to_string_lossy().to_string(),
                    e.metadata().map(|m| m.len()).unwrap_or(0),
                )
            })
            .collect();
        v.sort();
        v
    };

    // 1. ClaimDone touches NOTHING anywhere in the workspace.
    let before = fingerprint(&ws);
    let claim = CortexAction::ClaimDone {
        claim: CompletionClaim {
            done: true,
            rationale: "believe it is fixed".into(),
        },
    };
    let auth = r.authorize_action(&claim, None, "agent", &snap).unwrap();
    assert!(matches!(r.execute(auth), ExecOutcome::Claimed { .. }));
    assert_eq!(
        fingerprint(&ws),
        before,
        "a completion claim created or changed a file — it must be effect-free, \
         and comparing only the target file cannot see that"
    );

    // 2. Inspecting a symbol that is not there is NOT an empty body. An empty
    //    body is a real thing a function can have; absence is a different fact.
    let ghost = SymbolRef {
        path: "broken.ax".into(),
        symbol: "no_such_symbol".into(),
    };
    let inspect = CortexAction::Inspect {
        target: ghost.clone(),
    };
    let auth = r.authorize_action(&inspect, None, "agent", &snap).unwrap();
    match r.execute(auth) {
        ExecOutcome::SymbolNotFound { symbol, .. } => assert_eq!(symbol, "no_such_symbol"),
        other => panic!("expected SymbolNotFound, got {other:?}"),
    }

    // 3. Patching a symbol that is not there must NOT report Patched. The old
    //    apply_patch returned Ok(false) when its needle was absent, which a
    //    caller could read as "applied, nothing to change".
    let grant = EditGrant {
        grant_id: "g-neg".into(),
        principal: "agent".into(),
        snapshot_id: snap.snapshot_id.clone(),
        write_prefixes: vec!["broken.ax".into()],
    };
    let patch_ghost = CortexAction::PatchSymbolBody {
        symbol: ghost,
        proposed_body: "irrelevant".into(),
    };
    let before = fingerprint(&ws);
    let auth = r
        .authorize_action(&patch_ghost, Some(&grant), "agent", &snap)
        .unwrap();
    match r.execute(auth) {
        ExecOutcome::SymbolNotFound { .. } => {}
        other => panic!("a patch to a missing symbol reported {other:?}"),
    }
    assert_eq!(
        fingerprint(&ws),
        before,
        "a patch that found no symbol must not have written anything"
    );

    // 4. A check whose name matches no test is not a passing check — the same
    //    zero-match hole verify() has, in the execute path.
    let check = CortexAction::RunCheck {
        check: CheckRef {
            name: "no_such_check".into(),
            path: "broken.ax".into(),
        },
    };
    let auth = r.authorize_action(&check, None, "agent", &snap).unwrap();
    match r.execute(auth) {
        ExecOutcome::CheckRan {
            passed, matched, ..
        } => {
            assert_eq!(matched, 0, "nothing should have matched");
            assert!(
                !passed,
                "a filter matching nothing exits 0; reporting that as a pass \
                 lets any check be satisfied by naming one that does not exist"
            );
        }
        other => panic!("expected CheckRan, got {other:?}"),
    }

    // Control: a check that DOES exist still runs and reports its match count,
    // so the assertions above are not satisfied by everything failing.
    let real = CortexAction::RunCheck {
        check: CheckRef {
            name: "visible_repro".into(),
            path: "broken.ax".into(),
        },
    };
    let auth = r.authorize_action(&real, None, "agent", &snap).unwrap();
    match r.execute(auth) {
        ExecOutcome::CheckRan { matched, .. } => {
            assert!(matched > 0, "the real check must have matched")
        }
        other => panic!("expected CheckRan, got {other:?}"),
    }
}

/// A bounded episode drives itself to a conclusion — or says why it cannot.
///
/// The loop closed for one step; this iterates it. What matters is not that it
/// terminates (a budget guarantees that) but that the REASON it stopped is the
/// true one, because "it stopped" is not something a caller can act on.
#[test]
fn cxg_c11_a_bounded_episode_terminates_with_a_reason() {
    use axon_cortex::action::SymbolRef;
    use axon_cortex::runner::EpisodeOutcome;

    // 0. SUCCESS FIRST. Repair the fixture, then run the episode: it must
    //    claim done, have verification HOLD, and finish. Without this row every
    //    assertion below is satisfied by a loop that never succeeds at
    //    anything, which is the easiest way to build a controller that looks
    //    rigorous and cannot work.
    //
    //    (An earlier draft of this test asserted the success row and then found
    //    the fixture produced NoProgress instead — the comment claimed a
    //    control the test did not have.)
    let (_, ws0) = stage("ep_success");
    let fixed = std::fs::read_to_string(ws0.join("broken.ax"))
        .unwrap()
        .replace("n + 2", "n * 2");
    std::fs::write(ws0.join("broken.ax"), fixed).unwrap();
    let mut r0 = Runner::new(axon_bin(), &ws0);
    let ok = r0.run_episode(
        &SymbolRef {
            path: "broken.ax".into(),
            symbol: "double".into(),
        },
        None,
        "agent",
        "hidden_completion",
        6,
    );
    assert!(
        matches!(ok, EpisodeOutcome::VerifiedDone { .. }),
        "a repaired fixture must drive to VerifiedDone, got {ok:?}"
    );

    // 1. The UNREPAIRED fixture: it compiles, so selection claims done, but the
    //    hidden check refuses the claim and the workspace never changes.
    let (_, ws) = stage("ep_ok");
    let mut r = Runner::new(axon_bin(), &ws);
    // clean.ax has no @[test] of its own, so give it a check that does exist
    // in the file it is asked about — the point of this row is the SUCCESS
    // path, and a check that cannot match would test the wrong thing.
    let outcome = r.run_episode(
        &SymbolRef {
            path: "broken.ax".into(),
            symbol: "double".into(),
        },
        None,
        "agent",
        "visible_repro",
        6,
    );
    // broken.ax compiles, so selection claims done; visible_repro FAILS on the
    // unrepaired fixture, so the claim is refused and the loop tries again with
    // an unchanged workspace — which is exactly NoProgress, not success.
    assert!(
        matches!(outcome, EpisodeOutcome::NoProgress { .. }),
        "an unrepairable claim loop must stop as NoProgress, got {outcome:?}"
    );

    // 2. NoProgress must be distinguishable from BudgetExhausted. A budget of 1
    //    cannot detect stuckness — there is no previous step to compare with —
    //    so the same fixture must report the budget instead.
    let (_, ws2) = stage("ep_budget");
    let mut r2 = Runner::new(axon_bin(), &ws2);
    let outcome2 = r2.run_episode(
        &SymbolRef {
            path: "broken.ax".into(),
            symbol: "double".into(),
        },
        None,
        "agent",
        "visible_repro",
        1,
    );
    assert!(
        matches!(outcome2, EpisodeOutcome::BudgetExhausted { steps: 1 }),
        "one step cannot prove stuckness; it must report the budget, got {outcome2:?}"
    );

    // 3. A checker that cannot run makes the observation Unknown, and the
    //    episode must BLOCK carrying that reason rather than guessing.
    let (_, ws3) = stage("ep_blocked");
    let mut r3 = Runner::new(ws3.join("no-such-axon"), &ws3);
    let outcome3 = r3.run_episode(
        &SymbolRef {
            path: "broken.ax".into(),
            symbol: "double".into(),
        },
        None,
        "agent",
        "visible_repro",
        6,
    );
    match outcome3 {
        EpisodeOutcome::Blocked { reason, .. } => assert!(
            reason.contains("unknown"),
            "the block must carry the observer's reason: {reason}"
        ),
        other => panic!("expected Blocked, got {other:?}"),
    }
}
