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

/// `git merge-tree --write-tree` of `a` and `b` (plain: no note driver): the tree and the conflicted paths.
fn merge_tree(a: &str, b: &str) -> (String, Vec<String>) {
    let o = std::process::Command::new("git")
        .args([
            "merge-tree",
            "--write-tree",
            "-z",
            "--name-only",
            "--no-messages",
            a,
            b,
        ])
        .current_dir(repo_root())
        .output()
        .unwrap();
    assert!(
        matches!(o.status.code(), Some(0 | 1)),
        "git merge-tree {a} {b}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    let out = String::from_utf8(o.stdout).unwrap();
    let mut parts = out.split('\0').filter(|p| !p.is_empty()).map(String::from);
    (parts.next().unwrap(), parts.collect())
}

/// Fixed identity and dates for the scratch commits below (unreferenced objects in the repository's
/// object store, which `git gc` drops).
const WHO: &[(&str, &str)] = &[
    ("GIT_AUTHOR_NAME", "pci"),
    ("GIT_AUTHOR_EMAIL", "pci@invalid"),
    ("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z"),
    ("GIT_COMMITTER_NAME", "pci"),
    ("GIT_COMMITTER_EMAIL", "pci@invalid"),
    ("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z"),
];

/// A scratch commit of `tree` with `parents`.
fn commit(tree: &str, parents: &[&str], msg: &str) -> String {
    let mut a = vec!["commit-tree", tree, "-m", msg];
    for p in parents {
        a.extend(["-p", p]);
    }
    git(&a, WHO, None)
}

/// The blob of `bytes`.
fn blob(bytes: &[u8]) -> String {
    git(&["hash-object", "-w", "--stdin"], &[], Some(bytes))
}

/// `base`'s tree with each path set to a blob (`Some`) or removed (`None`), through a private index.
fn tree_with(base: &str, edits: &[(&str, Option<String>)]) -> String {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let index = std::env::temp_dir().join(format!(
        "pci-scratch-{}-{}.index",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let ix: &[(&str, &str)] = &[("GIT_INDEX_FILE", index.to_str().unwrap())];
    git(&["read-tree", base], ix, None);
    for (path, b) in edits {
        match b {
            Some(b) => git(
                &["update-index", "--cacheinfo", &format!("100644,{b},{path}")],
                ix,
                None,
            ),
            None => git(&["update-index", "--force-remove", path], ix, None),
        };
    }
    let tree = git(&["write-tree"], ix, None);
    let _ = std::fs::remove_file(&index);
    tree
}

/// `rev`'s file `path` with `line` appended.
fn appended(rev: &str, path: &str, line: &str) -> String {
    let mut text = git(&["show", &format!("{rev}:{path}")], &[], None).into_bytes();
    text.extend_from_slice(format!("\n{line}\n").as_bytes());
    blob(&text)
}

/// A merge with no themes file is classified by the pure-join rule only when git's merge of its
/// parents leaves no conflict under crates/axon-core/src and gives the merge's crates/axon-core/src:
/// 558885e5 merged cleanly, 1adbe2b4 conflicted only in scripts/pci_delta.py (dropping either file
/// classifies it by the rule); 9453352e resolved conflicts in that tree (dropping its file leaves it
/// UNCLASSIFIED).
#[test]
fn a_merge_without_a_themes_file_is_classified_only_when_it_is_a_pure_join() {
    for (merge, theme) in [("558885e5", "r51/int merge"), ("1adbe2b4", "merge")] {
        let (ok, out, err) = emit(&["HEAD", "--drop", merge]);
        assert!(ok, "{merge}, a pure join, was refused:\n{err}");
        assert!(
            out.contains(&format!(
                "\n| {merge} | {theme} | pure join: crates/axon-core/src equals "
            )),
            "{out}"
        );
    }
    let (ok, _, err) = emit(&["HEAD", "--drop", "9453352e"]);
    assert!(
        !ok,
        "9453352e, a merge that conflicted in the interpreter, was classified"
    );
    assert!(
        err.contains("UNCLASSIFIED") && err.contains("9453352e"),
        "{err}"
    );
}

/// An evil merge: 558885e5's parents joined to its tree plus one line under crates/axon-core/src.
/// It has no themes file and is UNCLASSIFIED; the same merge with 558885e5's own tree is a pure
/// join (the control: the refusal is the added line's, not the scratch commit's).
#[test]
fn an_evil_merge_is_unclassified() {
    let parents = ["558885e5^1", "558885e5^2"];
    let pure = commit(
        &git(&["rev-parse", "558885e5^{tree}"], &[], None),
        &parents,
        "Merge branch 'scratch'",
    );
    let file = "crates/axon-core/src/lib.rs";
    let evil = commit(
        &tree_with(
            "558885e5",
            &[(file, Some(appended("558885e5", file, "// evil")))],
        ),
        &parents,
        "Merge branch 'scratch'",
    );

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

/// A merge that conflicted under crates/axon-core/src stays UNCLASSIFIED however it was resolved,
/// even to one side's text: (a) 9453352e's parents, its two conflicted interpreter files taken from
/// the first parent, the rest git's merge; (b) one side deletes error.rs (and edits lib.rs), the
/// other edits error.rs, and the merge keeps the edit, which IS the tree git's merge records: only
/// the conflict, not the tree comparison, refuses (b).
#[test]
fn a_merge_that_conflicted_in_the_interpreter_is_unclassified_however_resolved() {
    let src = "crates/axon-core/src/";
    let p = git(&["show", "-s", "--format=%P", "9453352e"], &[], None);
    let (p1, p2) = p.split_once(' ').unwrap();
    let (mtree, listed) = merge_tree(p1, p2);
    let ours: Vec<(&str, Option<String>)> = listed
        .iter()
        .filter(|c| c.starts_with(src))
        .map(|c| {
            (
                c.as_str(),
                Some(git(&["rev-parse", &format!("{p1}:{c}")], &[], None)),
            )
        })
        .collect();
    assert!(
        !ours.is_empty(),
        "9453352e's parents no longer conflict under {src}"
    );
    let one_side = commit(
        &tree_with(&mtree, &ours),
        &[p1, p2],
        "Merge branch 'scratch'",
    );

    let (x, y) = (
        "crates/axon-core/src/error.rs",
        "crates/axon-core/src/lib.rs",
    );
    let base = "558885e5";
    let del = commit(
        &tree_with(
            base,
            &[(x, None), (y, Some(appended(base, y, "// side one")))],
        ),
        &[base],
        "scratch: delete error.rs, edit lib.rs",
    );
    let edit = commit(
        &tree_with(base, &[(x, Some(appended(base, x, "// side two")))]),
        &[base],
        "scratch: edit error.rs",
    );
    let (mtree, listed) = merge_tree(&del, &edit);
    assert!(listed.iter().any(|c| c == x), "{listed:?}");
    let kept = commit(&mtree, &[&del, &edit], "Merge branch 'scratch'");

    for m in [one_side, kept] {
        let (ok, out, _) = emit(&[&m]);
        assert!(!ok);
        assert!(
            out.contains(&format!("\n| {} | UNCLASSIFIED |  |\n", &m[..8])),
            "{out}"
        );
    }
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
