//! `governance/notes/v022-pci-delta.md` lists what changed under
//! `crates/axon-core/src` since the certified interpreter (31413ca7). That
//! list is GENERATED (`scripts/pci_delta.py`), so a reader never trusts a
//! hand-kept account of "the delta" (the first version of the note omitted
//! `lib.rs`, `parser.rs`, `ast.rs`, `error.rs` and three commits).
//!
//! This test fails when the note's block is not what git says at the head it
//! names, when a commit in that range is not classified, and when a commit
//! after the pinned head touches the interpreter (the note is then stale).

mod script_spawn;
use script_spawn::{repo_root, script, Bins};

#[test]
fn the_pci_delta_note_is_what_git_says() {
    let r = repo_root();
    let o = script(
        "python3",
        r.join("scripts/pci_delta.py"),
        Bins::NoWorkspaceBinary,
    )
    .arg("--check")
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
    assert!(o.status.success(), "the PCI delta note drifted:\n{out}");
    assert!(out.contains("pci_delta: PASS"), "{out}");
}

/// The control: forgetting one classification makes the generator refuse, so
/// the pass above is a judgement and not a printed constant.
#[test]
fn an_unclassified_interpreter_commit_is_refused() {
    let r = repo_root();
    let o = script(
        "python3",
        r.join("scripts/pci_delta.py"),
        Bins::NoWorkspaceBinary,
    )
    .args(["--emit", "HEAD", "--drop", "e8537726"])
    .current_dir(&r)
    .env("PYTHONDONTWRITEBYTECODE", "1")
    .env_remove("PYTHONPATH")
    .output()
    .unwrap();
    assert!(!o.status.success(), "an unclassified commit was accepted");
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(
        err.contains("UNCLASSIFIED") && err.contains("e8537726"),
        "{err}"
    );
}

/// `pci_delta.py --check` with one in-memory change (`--plant KEY:OLD=>NEW`): the output, and
/// whether it passed. The unplanted check passes (above), so each failure below is the plant's.
fn check_with_plant(plant: &str) -> (bool, String) {
    let r = repo_root();
    let o = script(
        "python3",
        r.join("scripts/pci_delta.py"),
        Bins::NoWorkspaceBinary,
    )
    .args(["--check", "--plant", plant])
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

/// A planted failure means something only if the unplanted check passes: otherwise a note that
/// drifted for another reason would make every control below pass without judging its plant.
fn the_unplanted_check_passes() {
    let r = repo_root();
    let o = script(
        "python3",
        r.join("scripts/pci_delta.py"),
        Bins::NoWorkspaceBinary,
    )
    .arg("--check")
    .current_dir(&r)
    .env("PYTHONDONTWRITEBYTECODE", "1")
    .env_remove("PYTHONPATH")
    .output()
    .unwrap();
    assert!(
        o.status.success(),
        "the unplanted check fails, so no plant can be judged:\n{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}

/// The note's gate table is compared with the gate script on label, package/target AND the
/// result each row prints, not on labels alone (round-13 PSV-3: `PASS 1/1` -> `PASS 2/2`, and
/// `axon-core/lib` -> `axon-psv/lib` on an am108 row, both passed the check that read labels).
#[test]
fn a_wrong_result_or_target_in_the_notes_gate_table_is_refused() {
    the_unplanted_check_passes();
    let row =
        "| am108 a native registry a sealed frame wrote is tainted | axon-core/lib | PASS 1/1 |";
    for (what, new) in [
        ("result", row.replace("PASS 1/1", "PASS 2/2")),
        ("target", row.replace("axon-core/lib", "axon-psv/lib")),
    ] {
        let (ok, out) = check_with_plant(&format!("note:{row}=>{new}"));
        assert!(
            !ok && out.contains("coverage table"),
            "ATTACK: a wrong {what} in the note's gate table was accepted:\n{out}"
        );
    }
}

/// The claim's two amendment lists are derived, not retyped: dropping am108 and am114 from the
/// delta list, or naming only am100 and am102 as the amendments whose arms are verified, fails.
#[test]
fn a_stale_amendment_list_in_the_claim_is_refused() {
    the_unplanted_check_passes();
    for (key, plant, fragment) in [
        (
            "the delta list",
            "verdict:/102/106/108/114/117/121 (the delta=>/102/106 (the delta",
            "delta amendment list",
        ),
        (
            "the arms list",
            "verdict:am100, am102, am106, am108, am114, am117 and am121 (=>am100 and am102 (",
            "amendments whose arms are verified",
        ),
    ] {
        let (ok, out) = check_with_plant(plant);
        assert!(
            !ok && out.contains(fragment),
            "ATTACK: a stale claim list ({key}) was accepted:\n{out}"
        );
    }
}

/// A mutation row's test is run by a gate row only if a row names it under the SAME package and
/// target, by its exact name; a substring of another test's name, or the right name under the
/// wrong package, is no gate.
#[test]
fn a_test_the_gate_runs_under_another_package_or_name_is_no_gate() {
    the_unplanted_check_passes();
    let name = "operator_side_control_flow_on_candidate_data_never_selects_operator_code";
    let right = format!("|axon-psv|sealed_frames|{name}\"");
    for (what, wrong) in [
        (
            "another package",
            format!("|axon-core|sealed_frames|{name}\""),
        ),
        (
            "a name that only contains it",
            format!("|axon-psv|sealed_frames|x_{name}\""),
        ),
    ] {
        let (ok, out) = check_with_plant(&format!("gates:{right}=>{wrong}"));
        assert!(
            !ok && out.contains("is run by no gate row"),
            "ATTACK: a gate row with {what} counted as running the test:\n{out}"
        );
    }
}

/// Amendment 117: the claim's list of what is NOT claimed names every finding the loop's triage
/// table decided NARROW-CLAIM, and only those. Dropping one entry, narrowing one more finding in
/// the table, or un-narrowing one the claim still lists, each fails.
#[test]
fn every_narrow_claim_finding_of_the_loop_is_in_the_non_claim_list_and_no_other_is() {
    the_unplanted_check_passes();
    for (what, plant, fragment) in [
        (
            "an entry dropped from the claim",
            "verdict:    - `dangling-operator-reference-resolved-from-candidate`\n=>",
            "is not in the claim's non-claim list",
        ),
        (
            "a finding narrowed in the table only",
            "triage:| FIX | b. control-taint gaps | a match guard=>| NARROW-CLAIM | b. control-taint gaps | a match guard",
            "is not in the claim's non-claim list",
        ),
        (
            "a finding un-narrowed in the table only",
            "triage:| NARROW-CLAIM | o. other | a module, fn, type or constant the SUITE names=>| FIX | o. other | a module, fn, type or constant the SUITE names",
            "which the triage table does not mark NARROW-CLAIM",
        ),
    ] {
        let (ok, out) = check_with_plant(plant);
        assert!(
            !ok && out.contains(fragment),
            "ATTACK: {what} was accepted by the claim drift check:\n{out}"
        );
    }
}
