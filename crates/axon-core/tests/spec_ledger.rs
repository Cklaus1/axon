//! `scripts/spec_ledger.py` (docs-truth DT-6, O-GAP-11) refuses a ledger whose
//! spec status lines, closure stamps, CHANGELOG and ROADMAP disagree.
//!
//! Shipped specs kept their DRAFT status and fixed opportunities stayed open
//! three times (L015, O-HI-02, O-GAP-11). The script is a full-gate par stage;
//! these tests plant a minimal tree per rule, so each refusal is a judgement
//! and the pass on the consistent tree is not a printed constant.

mod script_spawn;
use script_spawn::{repo_root, script, Bins};
use std::path::{Path, PathBuf};

/// An ancestor of every head this test runs at (checker-soundness, 2026-10-27).
const ANCESTOR: &str = "49b576fa";

/// A consistent tree: one BUILT spec citing a stamped entry, one DRAFT spec.
fn base() -> Vec<(&'static str, String)> {
    vec![
        (
            "tasks/a-spec.md",
            format!(
                "# a - spec\n\n**Status: BUILT 2026-10-29 on `main` {ANCESTOR} (A-1).**\n\n\
                 Source: `tasks/opportunities.md` O-T-01.\n\n## A-1 A thing\n\nBody.\n"
            ),
        ),
        ("tasks/b-spec.md", "# b - spec\n\n**Status: DRAFT.**\n\n## B-1 Other\n\nBody.\n".into()),
        (
            "tasks/opportunities.md",
            format!(
                "# Opportunities\n\n### O-T-01 - a thing\nSPECCED in tasks/a-spec.md.\n\
                 `[BUILT 2026-10-29 on main {ANCESTOR}: a A-1.]`\n\n### O-T-02 - an open thing\nNot specced.\n"
            ),
        ),
        ("CHANGELOG.md", "# Changelog\n\n- **A thing (A-1, O-T-01).**\n".into()),
        ("ROADMAP.md", "# Roadmap\n\nBuild specs: `python3 scripts/spec_ledger.py --list`.\n".into()),
    ]
}

fn set(tree: &mut Vec<(&'static str, String)>, rel: &'static str, text: String) {
    tree.retain(|(r, _)| *r != rel);
    tree.push((rel, text));
}

fn get(tree: &[(&'static str, String)], rel: &str) -> String {
    tree.iter().find(|(r, _)| *r == rel).unwrap().1.clone()
}

/// Runs the ledger on the planted tree: (exit code, stdout + stderr).
fn ledger(tag: &str, tree: &[(&'static str, String)]) -> (Option<i32>, String) {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("axon_spec_ledger_{}_{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, text) in tree {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
    }
    let r = repo_root();
    let o = script("python3", r.join("scripts/spec_ledger.py"), Bins::NoWorkspaceBinary)
        .arg(&dir)
        .current_dir(Path::new(&r))
        .env("SPEC_LEDGER_GIT", &r)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("PYTHONPATH")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let out = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    (o.status.code(), out)
}

/// The planted tree is refused with exactly this problem line.
fn refused(tag: &str, tree: &[(&'static str, String)], line: &str, attack: &str) {
    let (code, out) = ledger(tag, tree);
    assert!(
        code == Some(1) && out.lines().any(|l| l == format!("spec_ledger: {line}")),
        "ATTACK: {attack} was accepted (rc {code:?}, wanted the line `spec_ledger: {line}`):\n{out}"
    );
}

#[test]
fn a_consistent_planted_tree_passes() {
    let (code, out) = ledger("ok", &base());
    assert_eq!(code, Some(0), "the consistent tree was refused:\n{out}");
    assert!(out.contains("spec_ledger: PASS (0 problem(s), 2 specs)"), "{out}");
}

#[test]
fn a_spec_without_a_status_line_is_refused() {
    let mut t = base();
    set(&mut t, "tasks/b-spec.md", "# b - spec\n\nStatus: DRAFT\n\n## B-1 Other\n".into());
    refused(
        "nostatus",
        &t,
        "tasks/b-spec.md: no `**Status: <WORD>` line in its first 15 lines",
        "a spec without a status line",
    );
}

#[test]
fn a_built_spec_citing_an_unstamped_entry_is_refused() {
    let mut t = base();
    let opp = get(&t, "tasks/opportunities.md");
    let stamp = format!("`[BUILT 2026-10-29 on main {ANCESTOR}: a A-1.]`\n");
    set(&mut t, "tasks/opportunities.md", opp.replace(&stamp, ""));
    refused(
        "unstamped",
        &t,
        "tasks/a-spec.md: status BUILT, but O-T-01 carries no closure stamp",
        "a built spec citing an unstamped entry",
    );
}

#[test]
fn an_entry_specced_in_a_built_spec_without_a_stamp_is_refused() {
    let mut t = base();
    let opp = get(&t, "tasks/opportunities.md");
    set(
        &mut t,
        "tasks/opportunities.md",
        opp.replace("Not specced.", "SPECCED in tasks/a-spec.md (A-1 covers it)."),
    );
    refused(
        "specced",
        &t,
        "O-T-02: specced in a-spec.md (BUILT) but carries no closure stamp",
        "an entry specced in a built spec without a stamp",
    );
}

#[test]
fn a_draft_spec_whose_items_shipped_is_refused() {
    let mut t = base();
    set(&mut t, "CHANGELOG.md", "# Changelog\n\n- **A thing (A-1, O-T-01).**\n- **Other (B-1).**\n".into());
    refused(
        "draftshipped",
        &t,
        "tasks/b-spec.md: status DRAFT, but CHANGELOG.md names its items ['B-1']",
        "a draft spec whose items shipped",
    );
}

#[test]
fn a_built_status_naming_a_non_ancestor_commit_is_refused() {
    let mut t = base();
    let a = get(&t, "tasks/a-spec.md");
    set(&mut t, "tasks/a-spec.md", a.replace(ANCESTOR, "deadbee"));
    refused(
        "nonancestor",
        &t,
        "tasks/a-spec.md: status BUILT names ['deadbee'], none an ancestor of HEAD",
        "a built status naming a non-ancestor commit",
    );
}

#[test]
fn a_roadmap_without_the_ledger_pointer_is_refused() {
    let mut t = base();
    set(&mut t, "ROADMAP.md", "# Roadmap\n\nBuild specs: see tasks/.\n".into());
    refused(
        "roadmap",
        &t,
        "ROADMAP.md: does not point at `scripts/spec_ledger.py --list`",
        "a roadmap without the ledger pointer",
    );
}

/// A DRAFT spec that QUOTES shipped headings (a CHANGELOG excerpt, another
/// spec) inside fences does not own them: a 4-backtick block holding a
/// 3-backtick block, and a `~~~` block. The prototype's false refusal on the
/// docs-truth spec itself, and Step 1's DTA-4 planted cases.
#[test]
fn a_heading_inside_a_fence_is_not_an_item() {
    let mut t = base();
    set(
        &mut t,
        "tasks/b-spec.md",
        "# b - spec\n\n**Status: DRAFT.**\n\n## B-1 Other\n\n\
         ````markdown\n```\nquoted code\n```\n## SF-1 a quoted heading\n````\n\n\
         ~~~\n## SF-2 another quoted heading\n~~~\n"
            .into(),
    );
    set(
        &mut t,
        "CHANGELOG.md",
        "# Changelog\n\n- **A thing (A-1, O-T-01).**\n- **SF-1 and SF-2 shipped.**\n".into(),
    );
    let (code, out) = ledger("fence", &t);
    assert!(
        code == Some(0) && out.contains("spec_ledger: PASS (0 problem(s), 2 specs)"),
        "ATTACK: a fenced heading was read as a shipped item (rc {code:?}):\n{out}"
    );
}

#[test]
fn a_duplicate_item_id_is_refused() {
    let mut t = base();
    t.push(("tasks/c-spec.md", "# c - spec\n\n**Status: DRAFT.**\n\n## B-1 Same id\n".into()));
    refused(
        "dup",
        &t,
        "tasks/c-spec.md: item B-1 also defined in tasks/b-spec.md",
        "a duplicate item id",
    );
}

/// Rule 2 reads only the bold status span: a sha in later prose on the same
/// line (a fix commit, a review note) is not the ship commit.
#[test]
fn a_sha_outside_the_status_span_does_not_count() {
    let mut t = base();
    let a = get(&t, "tasks/a-spec.md");
    set(
        &mut t,
        "tasks/a-spec.md",
        a.replace(
            &format!("**Status: BUILT 2026-10-29 on `main` {ANCESTOR} (A-1).**"),
            &format!("**Status: BUILT 2026-10-29 on `main` (A-1).** Review fix in {ANCESTOR}."),
        ),
    );
    refused(
        "span",
        &t,
        "tasks/a-spec.md: status BUILT names no commit",
        "a sha outside the status span",
    );
}

/// Rule 5 reads the whole Source paragraph, not its first physical line.
#[test]
fn a_wrapped_source_paragraph_is_read_whole() {
    let mut t = base();
    let a = get(&t, "tasks/a-spec.md");
    set(
        &mut t,
        "tasks/a-spec.md",
        a.replace(
            "Source: `tasks/opportunities.md` O-T-01.\n",
            "Source: `tasks/opportunities.md` O-T-01,\nand O-T-02 (wrapped).\n",
        ),
    );
    refused(
        "wrapped",
        &t,
        "tasks/a-spec.md: status BUILT, but O-T-02 carries no closure stamp",
        "a wrapped source paragraph read as one line",
    );
}
