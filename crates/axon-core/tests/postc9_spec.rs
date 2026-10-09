//! `governance/specs/post-c9-hardening.md` collects what C9 did not close. Its traceability table must
//! carry every finding of the round-14 loop's triage table, its backlog must list every item, and the
//! PSV-1 non-claims section must point at it (`scripts/check_postc9_spec.py`). A spec that silently
//! lost a finding would be a non-claim nobody tracks.

mod script_spawn;
use script_spawn::{repo_root, script, Bins};

fn run(args: &[&str]) -> (bool, String) {
    let r = repo_root();
    let o = script(
        "python3",
        r.join("scripts/check_postc9_spec.py"),
        Bins::NoWorkspaceBinary,
    )
    .args(args)
    .current_dir(&r)
    .env("PYTHONDONTWRITEBYTECODE", "1")
    .env_remove("PYTHONPATH")
    .output()
    .unwrap();
    let out = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o.status.success(), out)
}

#[test]
fn the_post_c9_spec_traces_every_finding_and_item() {
    let (ok, out) = run(&["--check"]);
    assert!(ok, "the post-C9 spec drifted:\n{out}");
    assert!(out.contains("check_postc9_spec: PASS"), "{out}");
}

/// Each rule is shown to be able to fail: the unplanted check passes (above), and every plant below
/// is refused with the reason the rule gives.
#[test]
fn a_missing_signature_a_wrong_decision_a_lost_item_or_a_dropped_pointer_is_refused() {
    let (ok, out) = run(&["--check"]);
    assert!(
        ok,
        "the unplanted check fails, so no plant can be judged:\n{out}"
    );
    for (what, plant, fragment) in [
        (
            "a signature missing from the traceability table",
            "spec:| 52 | `width-rule-top-level-sizedint-only-soft-wrapper-hides-width` | fixed in amendment 117 | - |\n=>",
            "is not in the spec's traceability table",
        ),
        (
            "a NARROW-CLAIM finding traced as fixed",
            "spec:| 21 | `dispatch-operator-type-exempt-no-val-taint` | narrowed | PH-A1 |=>| 21 | `dispatch-operator-type-exempt-no-val-taint` | fixed in amendment 117 | - |",
            "is NARROW-CLAIM in the triage table",
        ),
        (
            "a narrowed finding naming no existing item",
            "spec:| 7 | `static-oracle-global-initializer-merged-check` | narrowed | PH-A12 |=>| 7 | `static-oracle-global-initializer-merged-check` | narrowed | PH-A99 |",
            "not a PH-A item heading",
        ),
        (
            "a finding narrowed in the triage table only",
            "triage:| FIX | b. control-taint gaps | a match guard=>| NARROW-CLAIM | b. control-taint gaps | a match guard",
            "is NARROW-CLAIM in the triage table",
        ),
        (
            "an item removed from the backlog table",
            "spec:| PH-D10 | PSV-6 timing; PSV-3 doc minors |=>| PH-D10x | PSV-6 timing; PSV-3 doc minors |",
            "is not in the Part F backlog table",
        ),
        (
            "the pointer from the PSV-1 non-claims dropped",
            "verdict:governance/specs/post-c9-hardening.md=>governance/specs/other.md",
            "does not reference governance/specs/post-c9-hardening.md",
        ),
    ] {
        let (ok, out) = run(&["--check", "--plant", plant]);
        assert!(
            !ok && out.contains(fragment),
            "ATTACK: {what} was accepted by the post-C9 spec check:\n{out}"
        );
    }
}
