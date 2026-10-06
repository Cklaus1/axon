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
