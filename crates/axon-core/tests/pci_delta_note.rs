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

/// `pci_delta.py --emit ARGS...`: whether it passed, stdout, stderr.
fn emit(args: &[&str]) -> (bool, String, String) {
    let r = repo_root();
    let o = script(
        "python3",
        r.join("scripts/pci_delta.py"),
        Bins::NoWorkspaceBinary,
    )
    .arg("--emit")
    .args(args)
    .current_dir(&r)
    .env("PYTHONDONTWRITEBYTECODE", "1")
    .env_remove("PYTHONPATH")
    .output()
    .unwrap();
    (
        o.status.success(),
        String::from_utf8_lossy(&o.stdout).into_owned(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// `git ARGS...` in the repository (with `env`, and `input` on stdin), trimmed stdout; panics on failure.
fn git(args: &[&str], env: &[(&str, &str)], input: Option<&[u8]>) -> String {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let mut c = Command::new("git");
    c.args(args)
        .current_dir(repo_root())
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut ch = c.spawn().unwrap();
    ch.stdin
        .take()
        .unwrap()
        .write_all(input.unwrap_or(b""))
        .unwrap();
    let o = ch.wait_with_output().unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

/// A merge with no themes file is classified by the pure-join rule only when git merges its parents
/// cleanly to the same crates/axon-core/src: 558885e5 did (dropping its file classifies it by the
/// rule); 9453352e resolved conflicts in that tree, and 1adbe2b4 conflicted (in scripts/pci_delta.py)
/// though its crates/axon-core/src is git's merge: dropping either file leaves it UNCLASSIFIED.
#[test]
fn a_merge_without_a_themes_file_is_classified_only_when_it_is_a_pure_join() {
    let (ok, out, err) = emit(&["HEAD", "--drop", "558885e5"]);
    assert!(ok, "a pure join was refused:\n{err}");
    assert!(
        out.contains("\n| 558885e5 | r51/int merge | pure join: crates/axon-core/src equals "),
        "{out}"
    );
    for merge in ["9453352e", "1adbe2b4"] {
        let (ok, _, err) = emit(&["HEAD", "--drop", merge]);
        assert!(!ok, "{merge}, a merge that conflicted, was classified");
        assert!(err.contains("UNCLASSIFIED") && err.contains(merge), "{err}");
    }
}

/// An evil merge: 558885e5's parents joined to its tree plus one line under crates/axon-core/src.
/// It has no themes file and is UNCLASSIFIED; the same merge with 558885e5's own tree is a pure
/// join (the control: the refusal is the added line's, not the scratch commit's).
#[test]
fn an_evil_merge_is_unclassified() {
    let who: &[(&str, &str)] = &[
        ("GIT_AUTHOR_NAME", "pci"),
        ("GIT_AUTHOR_EMAIL", "pci@invalid"),
        ("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z"),
        ("GIT_COMMITTER_NAME", "pci"),
        ("GIT_COMMITTER_EMAIL", "pci@invalid"),
        ("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
    ];
    let merge = |tree: &str| {
        git(
            &[
                "commit-tree",
                tree,
                "-p",
                "558885e5^1",
                "-p",
                "558885e5^2",
                "-m",
                "Merge branch 'scratch'",
            ],
            who,
            None,
        )
    };
    let pure = merge(&git(&["rev-parse", "558885e5^{tree}"], &[], None));
    let index = std::env::temp_dir().join(format!("pci-evil-{}.index", std::process::id()));
    let ix = index.to_str().unwrap();
    let with_ix: &[(&str, &str)] = &[("GIT_INDEX_FILE", ix)];
    git(&["read-tree", "558885e5"], with_ix, None);
    let file = "crates/axon-core/src/lib.rs";
    let mut text = git(&["show", &format!("558885e5:{file}")], &[], None).into_bytes();
    text.extend_from_slice(b"\n// evil\n");
    let blob = git(&["hash-object", "-w", "--stdin"], &[], Some(&text));
    git(
        &[
            "update-index",
            "--cacheinfo",
            &format!("100644,{blob},{file}"),
        ],
        with_ix,
        None,
    );
    let evil = merge(&git(&["write-tree"], with_ix, None));
    let _ = std::fs::remove_file(&index);

    let (ok, out, err) = emit(&[&pure]);
    assert!(ok, "the control merge was refused:\n{err}");
    assert!(
        out.contains(&format!("\n| {} | scratch merge | pure join: ", &pure[..8])),
        "{out}"
    );
    let (ok, _, err) = emit(&[&evil]);
    assert!(!ok, "an evil merge was classified");
    assert!(
        err.contains(&format!(
            "UNCLASSIFIED: {} Merge branch 'scratch'",
            &evil[..8]
        )),
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
