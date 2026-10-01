//! The mutation and paired-disable harnesses stamp their evidence with a
//! COMMIT, so they must run on exactly that commit and say which registry they
//! ran (C9 round 4, EQUIVALENCE (5)).
//!
//! Round 4 executed the hole: with uncommitted edits to
//! scripts/v022_freeze_manifest.py (M650's guarded file) and
//! profiles/linux-microvm/guest-init.sh, `v022_g01_mutations.py --only=M650`
//! wrote commit 1b687d95, all_killed true, because the dirty check looked at
//! crates/ only -- and 24 rows guard files outside crates/, and the registry
//! and markers live in scripts/. Shards recorded no registry, so --merge and
//! --join could not tell a run made from a locally edited registry.
//!
//! Each test runs the REAL harness script, copied from this tree into a
//! scratch repository, through its command line.

mod script_spawn;
use script_spawn::{repo_root, Bins};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const HARNESS: [&str; 4] = [
    "scripts/v022_g01_mutations.py",
    "scripts/v022_attack_markers.py",
    "scripts/v022_paired_disable.py",
    "scripts/lib_bounded_run.sh",
];

fn scratch(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "axon-harness-integrity-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

fn git(r: &Path, args: &[&str]) -> String {
    let o = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(r)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {o:?}");
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// A committed scratch repository holding the harness and a guarded file
/// outside crates/ (as guest-init.sh is for ten rows).
fn repo(tag: &str) -> PathBuf {
    let r = scratch(tag);
    git(&r, &["init", "-q", "-b", "main"]);
    for f in HARNESS {
        write(
            &r.join(f),
            &std::fs::read_to_string(repo_root().join(f)).unwrap(),
        );
    }
    write(
        &r.join("profiles/linux-microvm/guest-init.sh"),
        "#!/bin/sh\n",
    );
    write(&r.join(".gitignore"), "__pycache__/\n/target/\n");
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "candidate"]);
    r
}

/// `python3 <harness> <args>` in `r`.
fn harness(r: &Path, script: &str, args: &[&str]) -> Output {
    harness_cmd(r, script, args).output().unwrap()
}

fn harness_cmd(r: &Path, script: &str, args: &[&str]) -> Command {
    let mut c = script_spawn::script("python3", r.join(script), Bins::BuildsItsOwn);
    c.args(args)
        .current_dir(r)
        .env("PYTHONDONTWRITEBYTECODE", "1");
    c
}

/// An inline Python program run in `r` with the harness modules importable.
fn py(r: &Path, program: &str) -> String {
    let o = Command::new("python3")
        .args(["-c", program])
        .current_dir(r)
        .env("PYTHONPATH", r.join("scripts"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(o.status.success(), "setup program failed: {o:?}");
    String::from_utf8(o.stdout).unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The ways a tree can differ from its commit outside crates/.
/// A named way to make the tree dirty.
type DirtyCase = (&'static str, Box<dyn Fn()>);

fn dirty_cases(r: &Path) -> Vec<DirtyCase> {
    let (a, b, c) = (r.to_path_buf(), r.to_path_buf(), r.to_path_buf());
    vec![
        (
            "an uncommitted edit to a guarded file in profiles/",
            Box::new(move || {
                write(
                    &a.join("profiles/linux-microvm/guest-init.sh"),
                    "#!/bin/sh\n# edited\n",
                )
            }),
        ),
        (
            "an uncommitted edit to the attack-marker registry",
            Box::new(move || {
                let p = b.join("scripts/v022_attack_markers.py");
                let s = std::fs::read_to_string(&p).unwrap();
                write(&p, &format!("{s}\n# edited\n"));
            }),
        ),
        (
            "an untracked file in scripts/",
            Box::new(move || write(&c.join("scripts/extra.py"), "x = 1\n")),
        ),
    ]
}

/// A mutation run refuses any uncommitted change in the tree, not only under
/// crates/. Control: the clean tree gets past the check (and then stops on
/// the missing workspace, which is not this check).
#[test]
fn a_mutation_run_refuses_a_tree_with_an_uncommitted_change_outside_crates() {
    let args = ["--scope=psv", "--only=M650", "out.json"];
    let clean = repo("g01-clean");
    let o = harness(&clean, HARNESS[0], &args);
    assert!(
        !text(&o).contains("uncommitted changes"),
        "control: a clean tree is not refused as dirty: {}",
        text(&o)
    );
    for i in 0..3 {
        let r = repo("g01-dirty");
        let (what, make) = dirty_cases(&r).remove(i);
        make();
        let o = harness(&r, HARNESS[0], &args);
        assert!(
            !o.status.success() && text(&o).contains("uncommitted changes in the tree"),
            "ATTACK: a mutation run proceeded on a tree with an uncommitted change outside \
             crates ({what}):\n{}",
            text(&o)
        );
        let _ = std::fs::remove_dir_all(&r);
    }
    let _ = std::fs::remove_dir_all(&clean);
}

/// The same for a paired-disable run (and each of its shards).
#[test]
fn a_paired_disable_run_refuses_a_tree_with_an_uncommitted_change_outside_crates() {
    let args = ["--shard=0/1", "out.json"];
    let clean = repo("pd-clean");
    let o = harness(&clean, HARNESS[2], &args);
    assert!(
        !text(&o).contains("uncommitted changes"),
        "control: a clean tree is not refused as dirty: {}",
        text(&o)
    );
    for i in 0..3 {
        let r = repo("pd-dirty");
        let (what, make) = dirty_cases(&r).remove(i);
        make();
        let o = harness(&r, HARNESS[2], &args);
        assert!(
            !o.status.success() && text(&o).contains("uncommitted changes in the tree"),
            "ATTACK: a paired-disable run proceeded on a tree with an uncommitted change \
             outside crates ({what}):\n{}",
            text(&o)
        );
        let _ = std::fs::remove_dir_all(&r);
    }
    let _ = std::fs::remove_dir_all(&clean);
}

/// Mutation shards of the `binding` scope, as a run at HEAD writes them;
/// shard 1 claims `blobs` as its registry when given.
const MERGE_SHARDS: &str = r#"
import json, sys, v022_g01_mutations as m
head = sys.argv[1]; mode = sys.argv[2]; alt = mode == "alt"; out = sys.argv[3]
rows = [r for r in m.MUTATIONS if m.in_scope(r[0], "binding")]
for k in (0, 1):
    blobs = m.registry_blobs()
    if alt and k == 1:
        blobs = {f: "0" * 40 for f in blobs}
    d = {"schema": "axon-v022-mutation-run/3", "gate": "binding", "scope": "binding",
         "commit": head, "registry_blobs": blobs, "tree_clean": True, "toolchain": {},
         "shard": {"index": k, "of": 2}, "only": None, "all_killed": True,
         "mutations": [{"id": r[0], **m.row_digest(r), "result": "killed", "baseline": "passed"}
                       for i, r in enumerate(rows) if i % 2 == k]}
    if k == 1 and mode == "dirty":
        d["tree_clean"] = False
    if k == 1 and mode == "edits":
        d["mutations"][0]["old_sha256"] = "0" * 64
    json.dump(d, open(f"{out}/s{k}.json", "w"))
"#;

/// --merge refuses a shard made from a registry other than this tree's.
/// Control: two shards of this registry merge.
#[test]
fn a_merge_refuses_a_shard_made_from_another_registry() {
    let r = repo("merge");
    // Shards and output OUTSIDE the tree, which must stay clean.
    let out = scratch("merge-out");
    let head = git(&r, &["rev-parse", "HEAD"]);
    let p = |f: &str| out.join(f).display().to_string();
    let run = |alt: &str| {
        let prog = format!(
            "import sys; sys.argv = ['x', {head:?}, {alt:?}, {:?}]\n{MERGE_SHARDS}",
            out.display().to_string()
        );
        py(&r, &prog);
        harness(
            &r,
            HARNESS[0],
            &["--merge", &p("merged.json"), &p("s0.json"), &p("s1.json")],
        )
    };
    let o = run("alt");
    assert!(
        !o.status.success() && text(&o).contains("registry/marker blobs"),
        "ATTACK: --merge accepted a shard made from another registry:\n{}",
        text(&o)
    );
    let _ = std::fs::remove_file(out.join("merged.json"));
    let o = run("same");
    // (The merged verdict itself fails here: the scratch tree holds none of
    // the guards the retirement records name. The merge is what is judged.)
    assert!(
        out.join("merged.json").exists() && !text(&o).contains("refused:"),
        "control: shards of this registry merge: {}",
        text(&o)
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(&out);
}

/// Paired-disable shards, as a run at HEAD writes them.
const JOIN_SHARDS: &str = r#"
import json, sys, v022_g01_mutations as m, v022_paired_disable as pd
head = sys.argv[1]; mode = sys.argv[2]; alt = mode == "alt"; out = sys.argv[3]
u = sorted(set(m.EQUIVALENT_DID) | set(m.STALE_REFACTORED), key=lambda r: int(r[1:]))
for k in (0, 1):
    blobs = m.registry_blobs()
    if alt and k == 1:
        blobs = {f: "0" * 40 for f in blobs}
    sel = [x for i, x in enumerate(u) if i % 2 == k]
    d = {"schema": "axon-v022-paired-disable/2", "commit": head, "all_hold": True,
         "shard": f"{k}/2", "selected": sel, "registry_blobs": blobs, "tree_clean": True,
         "records": [{"mutation": x, "holds": True, "commit": head,
                      "edits_sha256": pd.current_edits_digest(x)} for x in sel]}
    if k == 1 and mode == "dirty":
        d["tree_clean"] = False
    if k == 1 and mode == "edits":
        d["records"][0]["edits_sha256"] = "0" * 64
    json.dump(d, open(f"{out}/j{k}.json", "w"))
"#;

/// --join refuses a shard made from a registry other than this tree's.
/// Control: two shards of this registry join.
#[test]
fn a_join_refuses_a_shard_made_from_another_registry() {
    let r = repo("join");
    let out = scratch("join-out");
    let head = git(&r, &["rev-parse", "HEAD"]);
    let p = |f: &str| out.join(f).display().to_string();
    let run = |alt: &str| {
        let prog = format!(
            "import sys; sys.argv = ['x', {head:?}, {alt:?}, {:?}]\n{JOIN_SHARDS}",
            out.display().to_string()
        );
        py(&r, &prog);
        harness(
            &r,
            HARNESS[2],
            &["--join", &p("joined.json"), &p("j0.json"), &p("j1.json")],
        )
    };
    let o = run("alt");
    assert!(
        !o.status.success() && text(&o).contains("registry/marker blobs"),
        "ATTACK: --join accepted a shard made from another registry:\n{}",
        text(&o)
    );
    let _ = std::fs::remove_file(out.join("joined.json"));
    let o = run("same");
    assert!(
        o.status.success() && out.join("joined.json").exists(),
        "control: shards of this registry join: {}",
        text(&o)
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(&out);
}

/// Shards written by `program` in `mode`, merged (`HARNESS[0] --merge`) or
/// joined (`HARNESS[2] --join`) by the real harness in a scratch clone.
fn combine(tag: &str, program: &str, join: bool, mode: &str) -> (Output, bool) {
    let r = repo(tag);
    let out = scratch(&format!("{tag}-out"));
    let head = git(&r, &["rev-parse", "HEAD"]);
    let p = |f: &str| out.join(f).display().to_string();
    let prog = format!(
        "import sys; sys.argv = ['x', {head:?}, {mode:?}, {:?}]\n{program}",
        out.display().to_string()
    );
    py(&r, &prog);
    let o = if join {
        harness(
            &r,
            HARNESS[2],
            &["--join", &p("joined.json"), &p("j0.json"), &p("j1.json")],
        )
    } else {
        harness(
            &r,
            HARNESS[0],
            &["--merge", &p("merged.json"), &p("s0.json"), &p("s1.json")],
        )
    };
    let written = out
        .join(if join { "joined.json" } else { "merged.json" })
        .exists();
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(&out);
    (o, written)
}

/// --merge refuses a shard that does not record a clean tree. Control:
/// a_merge_refuses_a_shard_made_from_another_registry's.
#[test]
fn a_merge_refuses_a_shard_from_a_dirty_tree() {
    let (o, written) = combine("merge-dirty", MERGE_SHARDS, false, "dirty");
    assert!(
        !written && text(&o).contains("does not record a clean tree"),
        "ATTACK: --merge accepted a shard made in a dirty tree:\n{}",
        text(&o)
    );
}

/// --merge refuses a shard whose row ran with an edit that is not this
/// registry's row (same registry blobs, another old/new text).
#[test]
fn a_merge_refuses_a_row_run_with_another_edit() {
    let (o, written) = combine("merge-edits", MERGE_SHARDS, false, "edits");
    assert!(
        !written && text(&o).contains("with an edit that is not this registry's row"),
        "ATTACK: --merge accepted a row executed with another edit:\n{}",
        text(&o)
    );
}

/// --join refuses a shard that does not record a clean tree.
#[test]
fn a_join_refuses_a_shard_from_a_dirty_tree() {
    let (o, written) = combine("join-dirty", JOIN_SHARDS, true, "dirty");
    assert!(
        !written && text(&o).contains("does not record a clean tree"),
        "ATTACK: --join accepted a shard made in a dirty tree:\n{}",
        text(&o)
    );
}

/// --join refuses a record executed with edits that are not this registry's.
#[test]
fn a_join_refuses_a_record_run_with_other_edits() {
    let (o, written) = combine("join-edits", JOIN_SHARDS, true, "edits");
    assert!(
        !written && text(&o).contains("with edits that are not"),
        "ATTACK: --join accepted a record executed with other edits:\n{}",
        text(&o)
    );
}

/// A kept paired-disable record is stale once ANY file of its owner package
/// changes, not only the files a list named (C9 round 4: M602's argument rests
/// on bin/axon-custodian.rs, M186's on backend.rs, neither on the list).
/// Control: at its own commit the record is current.
#[test]
fn a_kept_record_is_stale_once_its_owner_package_changes() {
    let r = repo("stale");
    // M245's guard is in axon-loop: a one-crate workspace of that name.
    write(
        &r.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/axon-loop\"]\nresolver = \"2\"\n",
    );
    write(
        &r.join("crates/axon-loop/Cargo.toml"),
        "[package]\nname = \"axon-loop\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    write(&r.join("crates/axon-loop/src/lib.rs"), "// v1\n");
    write(&r.join("crates/axon-loop/src/helper.rs"), "// v1\n");
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "owner package"]);
    let at = git(&r, &["rev-parse", "HEAD"]);
    let status = r.parent().unwrap().join(format!(
        "{}-status.json",
        r.file_name().unwrap().to_string_lossy()
    ));
    let prog = format!(
        "import json, v022_paired_disable as pd\n\
         json.dump({{'commit': {at:?}, 'records': [{{'mutation': 'M245', 'commit': {at:?}, \
         'holds': True, 'edits_sha256': pd.current_edits_digest('M245')}}]}}, \
         open({:?}, 'w'))\n",
        status.display().to_string()
    );
    py(&r, &prog);
    let check = || harness(&r, HARNESS[2], &["--check-stale", status.to_str().unwrap()]);
    let o = check();
    assert!(
        o.status.success() && text(&o).contains("current M245"),
        "control: at its own commit the record is current: {}",
        text(&o)
    );
    // A file of the owner package the old list never named.
    write(
        &r.join("crates/axon-loop/src/helper.rs"),
        "// v2: the all-paths argument rested on this\n",
    );
    git(&r, &["commit", "-q", "-am", "helper changed"]);
    let o = check();
    assert!(
        !o.status.success() && text(&o).contains("STALE M245"),
        "ATTACK: a kept paired-disable record stayed current after its owner package \
         changed:\n{}",
        text(&o)
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_file(&status);
}

// ── the harnesses' cells, run for real on a miniature workspace ─────────────

/// The registry's old text of `id` (what the harness expects to edit).
fn old_text(r: &Path, id: &str) -> String {
    py(
        r,
        &format!(
            "import sys, v022_g01_mutations as m\n\
             sys.stdout.write({{x[0]: x for x in m.MUTATIONS}}[{id:?}][3])"
        ),
    )
}

/// What every miniature test does when it runs: plant an executable in the
/// WORKSPACE target dir (as a script built from a mutated tree would) and log
/// the AXON_BIN it was given.
const PROBE: &str = r##"
fn probe() {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    if let Some(p) = std::env::var_os("FAKE_PLANT") {
        let p = std::path::PathBuf::from(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    if let Some(l) = std::env::var_os("FAKE_ENV_LOG") {
        let v = std::env::var(concat!("AXON", "_BIN")).unwrap_or_else(|_| "unset".into());
        let mut f = std::fs::OpenOptions::new().create(true).append(true).open(l).unwrap();
        writeln!(f, "{v}").unwrap();
    }
}
"##;

fn package(r: &Path, name: &str, extra: &str) {
    write(
        &r.join(format!("crates/{name}/Cargo.toml")),
        &format!(
            "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
             [features]\ndefault = []\n{extra}"
        ),
    );
    write(&r.join(format!("crates/{name}/src/lib.rs")), "\n");
}

/// A committed miniature workspace with the real harnesses: axon-core
/// (`axon`, and a `harness_binaries` test that runs scripts, holding M722's
/// guarded text), axon-loop (M245/M264's guarded file and their
/// `protected_class` tests, plus a root-only test that skips), and the
/// prerequisites paired-disable builds (cortex bins, psv_dev).
fn miniature(tag: &str) -> PathBuf {
    let r = repo(tag);
    write(
        &r.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/axon-core\", \"crates/axon-loop\", \
         \"crates/axon-cortex\", \"crates/axon-psv\"]\nresolver = \"2\"\n",
    );
    package(
        &r,
        "axon-core",
        "\n[[bin]]\nname = \"axon\"\npath = \"src/main.rs\"\n",
    );
    write(&r.join("crates/axon-core/src/main.rs"), "fn main() {}\n");
    write(
        &r.join("crates/axon-core/tests/script_spawn/mod.rs"),
        &format!("// guarded text\n{}", old_text(&r, "M722")),
    );
    write(
        &r.join("crates/axon-core/tests/harness_binaries.rs"),
        &format!(
            "// This test runs repository scripts through script_spawn::script(...).\n{PROBE}\n\
             #[test]\nfn an_ambient_binary_variable_never_reaches_a_script() {{\n    probe();\n    \
             assert!(!include_str!(\"script_spawn/mod.rs\").contains(\"let _ = v;\"), \
             \"ATTACK: an ambient binary-naming variable reached a script\");\n}}\n"
        ),
    );
    package(&r, "axon-loop", "");
    write(
        &r.join("crates/axon-loop/src/admission.rs"),
        &format!(
            "// guarded text\n{}\n{}\n",
            old_text(&r, "M245"),
            old_text(&r, "M264")
        ),
    );
    write(
        &r.join("crates/axon-loop/tests/protected_class.rs"),
        &format!(
            "{PROBE}\n#[test]\nfn a_key_revoked_at_the_operator_root_no_longer_counts() {{ probe(); }}\n\
             #[test]\nfn a_forged_unsigned_clearance_clears_nothing() {{ probe(); }}\n\
             #[test]\nfn a_root_only_test() {{ eprintln!(\"skipped: needs root (fixture)\"); }}\n"
        ),
    );
    package(
        &r,
        "axon-cortex",
        "\n[[bin]]\nname = \"cortex\"\npath = \"src/main.rs\"\n",
    );
    write(&r.join("crates/axon-cortex/src/main.rs"), "fn main() {}\n");
    package(&r, "axon-psv", "");
    write(
        &r.join("crates/axon-psv/examples/psv_dev.rs"),
        "fn main() {}\n",
    );
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "miniature workspace"]);
    r
}

/// Run `script` with `args` on the miniature workspace `r`, under an ambient
/// AXON_BIN, with the probe's plant/log paths. Returns (output, plant path,
/// the AXON_BIN values the cells saw).
fn run_cells(r: &Path, script: &str, args: &[&str]) -> (Output, PathBuf, Vec<String>) {
    let side = scratch("cells-side");
    // Where scripts' builds land: the WORKSPACE target dir, never the run's.
    let plant = r
        .join("target")
        .join("debug")
        .join("left-by-a-mutated-cell");
    let log = side.join("env.log");
    let o = harness_cmd(r, script, args)
        .env("CARGO_TARGET_DIR", side.join("tgt"))
        .env("AXON_BIN", "/ambient/axon-named-by-the-callers-shell")
        .env("FAKE_PLANT", &plant)
        .env("FAKE_ENV_LOG", &log)
        .env("V022_MUT_MEM", "4G")
        .output()
        .unwrap();
    let seen = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect();
    let _ = std::fs::remove_dir_all(&side);
    (o, plant, seen)
}

/// A mutation run's cell that ran scripts leaves no binary built from the
/// MUTATED tree in the workspace target dir (scrub after the cell), and no
/// cell sees the AXON_BIN of the shell that launched the run (C9 round 4,
/// EQUIVALENCE (6d) and the skip/environment minor). Through the real
/// harness, on one row, in a miniature workspace.
#[test]
fn a_mutation_run_leaves_no_mutant_binary_and_hides_the_callers_binary() {
    let r = miniature("mut-cells");
    let out = scratch("mut-cells-out").join("run.json");
    let (o, plant, seen) = run_cells(
        &r,
        HARNESS[0],
        &["--scope=all", "--only=M722", out.to_str().unwrap()],
    );
    let doc = std::fs::read_to_string(&out).unwrap_or_default();
    assert!(
        doc.contains("\"killed\""),
        "setup: the miniature row was not run and killed: {}\n{doc}",
        text(&o)
    );
    assert!(
        !plant.exists(),
        "ATTACK: a mutation run left a binary built from a mutated tree in the workspace \
         target dir ({})",
        plant.display()
    );
    assert!(
        !seen.is_empty() && seen.iter().all(|v| v == "unset"),
        "ATTACK: a mutation cell saw the caller's AXON_BIN: {seen:?}"
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(out.parent().unwrap());
}

/// A paired-disable run leaves no binary built from a mutated tree in the
/// workspace target dir, records a test that SKIPPED in a full-suite cell as
/// a skip (not a silent pass), and hides the caller's AXON_BIN from its cells.
/// Through the real harness, on one record, in a miniature workspace.
#[test]
fn a_paired_disable_run_scrubs_counts_skips_and_hides_the_callers_binary() {
    let r = miniature("pd-cells");
    let out = scratch("pd-cells-out").join("status.json");
    let (o, plant, seen) = run_cells(&r, HARNESS[2], &["--only=M245", out.to_str().unwrap()]);
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap_or_else(|_| "{}".into()))
            .unwrap_or_default();
    let rec = doc["records"]
        .as_array()
        .and_then(|rs| rs.iter().find(|x| x["mutation"] == "M245"))
        .cloned()
        .unwrap_or_default();
    assert!(
        rec["matrix"]["retired_guard_full_suite"].is_string(),
        "setup: the miniature record was not executed: {}\n{doc}",
        text(&o)
    );
    assert!(
        !plant.exists(),
        "ATTACK: a paired-disable run left a binary built from a mutated tree in the \
         workspace target dir ({})",
        plant.display()
    );
    let skipped = rec["matrix"]["retired_guard_full_suite_skipped"].to_string();
    assert!(
        skipped.contains("a_root_only_test"),
        "ATTACK: a full-suite cell counted a skipped test as a pass (recorded skips: {skipped})"
    );
    assert!(
        !seen.is_empty() && seen.iter().all(|v| v == "unset"),
        "ATTACK: a paired-disable cell saw the caller's AXON_BIN: {seen:?}"
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(out.parent().unwrap());
}

// ── a cell's build is judged by its own cargo invocation (amendment 59) ─────

/// What a test prints when a workspace binary it builds itself
/// (`script_spawn::workspace_bin`) fails to build: the nested cargo's
/// diagnostics, on the stderr the test process inherited (libtest does not
/// capture a child's output). The test itself is unaffected and passes.
const NESTED_NOISE: &str = r##"
fn nested_build_noise() {
    let _ = std::process::Command::new("sh")
        .args(["-c", "echo 'error[E0425]: cannot find value in this scope' >&2; echo 'error: could not compile `nested` (lib) due to 1 previous error' >&2"])
        .status();
}
"##;

/// A test file of the miniature `r` rewritten with `edit`, committed.
fn recommit(r: &Path, file: &str, edit: impl FnOnce(String) -> String) {
    let p = r.join(file);
    let s = std::fs::read_to_string(&p).unwrap();
    write(&p, &edit(s));
    git(r, &["add", "-A"]);
    git(r, &["commit", "-q", "-m", "fixture edit"]);
}

/// Every test of `file` prints a nested build's failure and still passes.
fn noisy(r: &Path, file: &str) {
    recommit(r, file, |s| {
        format!(
            "{NESTED_NOISE}\n{}",
            s.replace("probe();", "probe(); nested_build_noise();")
        )
    });
}

/// `file` no longer compiles.
fn broken(r: &Path, file: &str) {
    recommit(r, file, |s| {
        format!("{s}\n#[allow(dead_code)]\nfn broken() -> i64 {{ \"not an integer\" }}\n")
    })
}

/// The paths a harness printed as `output kept: <path>`.
fn kept(o: &Output) -> Vec<PathBuf> {
    text(o)
        .lines()
        .filter_map(|l| l.trim().strip_prefix("output kept: "))
        .map(PathBuf::from)
        .collect()
}

/// A nested build's failure printed INSIDE a passing test is not the row's
/// compile error: the mutation harness judged "compile_error" from any
/// `could not compile` / `error[E` anywhere in the output, so M278's baseline
/// (whose test builds the interpreter through workspace_bin) read
/// compile_error in a sharded run (C9 round 4). Control: the row is killed.
#[test]
fn a_mutation_run_does_not_take_a_tests_nested_build_output_for_a_compile_error() {
    let r = miniature("mut-noise");
    noisy(&r, "crates/axon-core/tests/harness_binaries.rs");
    let out = scratch("mut-noise-out").join("run.json");
    let (o, _, _) = run_cells(
        &r,
        HARNESS[0],
        &["--scope=all", "--only=M722", out.to_str().unwrap()],
    );
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap_or_else(|_| "{}".into()))
            .unwrap_or_default();
    let row = doc["mutations"][0].clone();
    assert!(
        row["id"] == "M722",
        "setup: the miniature row was not run: {}\n{doc}",
        text(&o)
    );
    assert_eq!(
        (row["baseline"].as_str(), row["result"].as_str()),
        (Some("passed"), Some("killed")),
        "ATTACK: a nested build's failure printed inside a passing test was taken for the \
         row's own compile error: {}\n{row}",
        text(&o)
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(out.parent().unwrap());
}

/// The same for paired-disable: a nested build's failure printed inside a
/// passing test made a full-suite (or consumer) cell read COMPILE_ERROR, and
/// a consumer baseline CONSUMER_BASELINE_BROKEN (M58, C9 round 4).
#[test]
fn a_paired_disable_run_does_not_take_a_tests_nested_build_output_for_a_compile_error() {
    let r = miniature("pd-noise");
    noisy(&r, "crates/axon-loop/tests/protected_class.rs");
    // The record's own test judges its guard set (M245, sibling M264): its
    // attack succeeds only in the joint cell, as a real record's does, so
    // that cell FAILS and must keep its output.
    let (m245, m264) = (old_text(&r, "M245"), old_text(&r, "M264"));
    recommit(&r, "crates/axon-loop/tests/protected_class.rs", |s| {
        s.replace(
            "fn a_key_revoked_at_the_operator_root_no_longer_counts() { probe(); nested_build_noise(); }",
            &format!(
                "fn a_key_revoked_at_the_operator_root_no_longer_counts() {{ probe(); \
                 nested_build_noise();\n    let s = include_str!(\"../src/admission.rs\");\n    \
                 assert!(s.contains(r###\"{m245}\"###) || s.contains(r###\"{m264}\"###), \
                 \"ATTACK: monitor: activated on a revoked key\"); }}"
            ),
        )
    });
    let out = scratch("pd-noise-out").join("status.json");
    let (o, _, _) = run_cells(&r, HARNESS[2], &["--only=M245", out.to_str().unwrap()]);
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap_or_else(|_| "{}".into()))
            .unwrap_or_default();
    let rec = doc["records"]
        .as_array()
        .and_then(|rs| rs.iter().find(|x| x["mutation"] == "M245"))
        .cloned()
        .unwrap_or_default();
    assert!(
        rec["matrix"]["retired_guard_full_suite"].is_string(),
        "setup: the miniature record was not executed: {}\n{doc}",
        text(&o)
    );
    assert_eq!(
        (
            rec["matrix"]["baseline"].as_str(),
            rec["matrix"]["retired_guard_full_suite"].as_str()
        ),
        (Some("ATTACK_REFUSED"), Some("SUITE_OK")),
        "ATTACK: a nested build's failure printed inside a passing test was taken for a \
         broken build: {}\n{}",
        text(&o),
        rec["matrix"]
    );
    // The cell where the attack succeeds did not pass: its output is kept.
    assert!(
        kept(&o).iter().any(|p| p
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("pd-cell-"))
            && std::fs::read_to_string(p)
                .unwrap_or_default()
                .contains("ATTACK:")),
        "ATTACK: a paired-disable cell whose test failed kept no output (kept {:?})",
        kept(&o)
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(out.parent().unwrap());
}

/// A baseline that does not build keeps its whole output, and says where:
/// the mutation record carried only the label `compile_error`, so M278's
/// environmental failure left nothing to diagnose (C9 round 4).
#[test]
fn a_mutation_baseline_that_does_not_build_keeps_its_output() {
    let r = miniature("mut-broken");
    broken(&r, "crates/axon-core/tests/harness_binaries.rs");
    let out = scratch("mut-broken-out").join("run.json");
    let (o, _, _) = run_cells(
        &r,
        HARNESS[0],
        &["--scope=all", "--only=M722", out.to_str().unwrap()],
    );
    let doc: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&out).unwrap_or_else(|_| "{}".into()))
            .unwrap_or_default();
    let row = doc["mutations"][0].clone();
    assert_eq!(
        row["baseline"].as_str(),
        Some("compile_error"),
        "setup: the broken baseline did not read compile_error: {}\n{doc}",
        text(&o)
    );
    let path = row["baseline_output"].as_str().map(PathBuf::from);
    let body = path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();
    assert!(
        body.contains("error[E0308]") && kept(&o).contains(path.as_ref().unwrap()),
        "ATTACK: a baseline that did not build kept no output (record {row}; printed {:?})",
        kept(&o)
    );
    let cell = row["cell_output"].as_str().map(PathBuf::from);
    assert!(
        cell.as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .is_some_and(|b| b.contains("error[E0308]"))
            && kept(&o).contains(cell.as_ref().unwrap()),
        "ATTACK: a mutated cell that did not pass kept no output (record {row})"
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(out.parent().unwrap());
}

/// A paired-disable cell that does not build keeps its whole output: the
/// full-suite cell returned on a compile error BEFORE its keep-the-output
/// branch, so M58's CONSUMER_BASELINE_BROKEN left nothing (C9 round 4).
#[test]
fn a_paired_disable_cell_that_does_not_build_keeps_its_output() {
    let r = miniature("pd-broken");
    broken(&r, "crates/axon-loop/tests/protected_class.rs");
    let out = scratch("pd-broken-out").join("status.json");
    let (o, _, _) = run_cells(&r, HARNESS[2], &["--only=M245", out.to_str().unwrap()]);
    // A cell's own test build (`run_test`) and a full-suite build
    // (`full_suite_ok`) each keep theirs.
    for (kind, what) in [
        (
            "pd-cell-build-",
            "a paired-disable cell whose test did not build",
        ),
        (
            "pd-suite-build-",
            "a paired-disable full-suite cell that did not build",
        ),
    ] {
        let builds: Vec<PathBuf> = kept(&o)
            .into_iter()
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(kind))
            })
            .collect();
        assert!(
            !builds.is_empty()
                && builds.iter().all(|p| std::fs::read_to_string(p)
                    .unwrap_or_default()
                    .contains("error[E0308]")),
            "ATTACK: {what} kept no output (kept {:?}): {}",
            kept(&o),
            text(&o)
        );
    }
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(out.parent().unwrap());
}
