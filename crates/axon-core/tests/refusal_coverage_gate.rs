//! The refusal-site coverage gate's own guards (C9 round 4b, integrate,
//! amendment 64). `scripts/v022_refusal_coverage.py` is the check every
//! protected decision file's evidence rests on, and the freeze consults it:
//! a guard of the gate that nothing exercises is a gate that can be switched
//! off unnoticed.
//!
//! Each test runs the REAL gate, through its command line, over a scratch
//! copy of this tree's in-scope sources, registry and markers, edited in one
//! named way (the ATTACK); the CONTROL is the unedited copy, on which the gate
//! holds.

mod script_spawn;
use script_spawn::Bins;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};

const GATE: &str = "scripts/v022_refusal_coverage.py";
/// What the gate reads: its registry, the markers the registry imports, and
/// every in-scope source (the rule's SCOPE_DIRS, SCOPE_FILES, and the files
/// of SCOPE_FN_REGIONS / REGIONS).
const COPY: [&str; 10] = [
    GATE,
    "scripts/v022_g01_mutations.py",
    "scripts/v022_attack_markers.py",
    "crates/axon-fabric/src",
    "crates/axon-loop/src",
    "crates/axon-loop-contracts/src",
    "crates/axon-psv/src",
    "crates/axon-core/src/interp.rs",
    "crates/axon-core/src/interp/conform.rs",
    "crates/axon-core/src/interp/eval.rs",
];
/// A scanned file every refusal site of which has a row (tasks.rs).
const SCANNED: &str = "crates/axon-loop/src/tasks.rs";
const INTERP: &str = "crates/axon-core/src/interp.rs";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn copy(from: &Path, to: &Path) {
    if from.is_dir() {
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            copy(&e.path(), &to.join(e.file_name()));
        }
    } else {
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(from, to).unwrap();
    }
}

/// A scratch copy of everything the gate reads.
fn tree(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "axon-refusal-coverage-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    for f in COPY {
        copy(&repo_root().join(f), &d.join(f));
    }
    d
}

fn edit(r: &Path, f: &str, old: &str, new: &str) {
    let p = r.join(f);
    let s = std::fs::read_to_string(&p).unwrap();
    assert_eq!(s.matches(old).count(), 1, "setup: {old:?} once in {f}");
    std::fs::write(&p, s.replacen(old, new, 1)).unwrap();
}

/// List `f` in the copy's NOT_YET_SCANNED with `count` (every other entry
/// is kept as this tree has it).
fn not_yet_scanned(r: &Path, f: &str, count: usize) {
    edit(
        r,
        GATE,
        "\nSITE = re.compile(",
        &format!("\nNOT_YET_SCANNED[{f:?}] = {count}\nSITE = re.compile("),
    );
}

fn gate(r: &Path, args: &[&str]) -> Output {
    // Through the workspace's script helper (harness_binaries'
    // every_script_spawn_in_the_workspace_goes_through_the_helper): the gate
    // runs no binary this workspace builds.
    script_spawn::script("python3", r.join(GATE), Bins::NoWorkspaceBinary)
        .args(args)
        .current_dir(r)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("PYTHONPATH")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn holds(r: &Path, args: &[&str], what: &str) {
    let o = gate(r, args);
    assert!(
        o.status.success(),
        "control ({what}): the gate must hold: {}",
        text(&o)
    );
}

/// The gate refuses with `why` in its report; ATTACK if it held.
fn refuses(r: &Path, args: &[&str], why: &str, attack: &str) {
    let o = gate(r, args);
    let t = text(&o);
    if o.status.success() {
        panic!("ATTACK: {attack}: the gate held: {t}");
    }
    assert!(t.contains(why), "{attack}: expected {why:?}: {t}");
}

/// A refusal site nothing rows or exempts: a new function in `f` (before its
/// test module, so it is code the gate reads).
const NEW_SITE: &str = "\npub fn integrate_gate_probe(x: u64) -> Result<(), String> {\n    if x > 7 {\n        return Err(format!(\"probe {x}\"));\n    }\n    Ok(())\n}\n";

fn add_site(r: &Path, f: &str) {
    let p = r.join(f);
    let s = std::fs::read_to_string(&p).unwrap();
    let at = s.find("\n#[cfg(test)]\nmod tests").unwrap_or(s.len());
    std::fs::write(&p, format!("{}{NEW_SITE}{}", &s[..at], &s[at..])).unwrap();
}

/// Amendment 61's re-measured count: a NOT YET SCANNED file's listed count
/// is re-measured on every run, so a refusal site added to such a file is
/// reported (the count no longer matches) rather than hidden behind the
/// listing. Control: the listing that matches the measurement holds.
#[test]
fn a_new_site_in_a_not_yet_scanned_file_is_reported() {
    let r = tree("count");
    not_yet_scanned(&r, SCANNED, 0);
    holds(
        &r,
        &[],
        "a NOT YET SCANNED listing whose count is the measured one",
    );
    add_site(&r, SCANNED);
    refuses(
        &r,
        &[],
        "NOT_YET_SCANNED says 0 uncovered sites, measured 1",
        "a refusal site added to a NOT YET SCANNED file went unreported behind a stale count",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 61: under --freeze, a non-empty NOT_YET_SCANNED is itself a
/// failure, so no freeze binds evidence over a decision file nobody scanned.
/// Control: the same tree holds without --freeze (the count is right).
#[test]
fn a_freeze_reading_refuses_a_not_yet_scanned_file() {
    let r = tree("freeze");
    not_yet_scanned(&r, SCANNED, 0);
    holds(&r, &[], "the same listing outside a freeze");
    refuses(
        &r,
        &["--freeze"],
        "NOT YET SCANNED at a freeze",
        "the freeze reading of the gate held with a protected file NOT YET SCANNED",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 64: interp.rs is scanned over the UNION of the seal|conform|cast
/// functions and the anchored region (REGIONS). A refusal added in the region
/// but in a function whose name the rule does not select (rng_guard's
/// neighbourhood) is still a site. Control: the unedited copy holds.
#[test]
fn a_site_in_the_anchored_region_outside_a_seal_fn_is_scanned() {
    let r = tree("region");
    holds(&r, &[], "the unedited copy");
    edit(
        &r,
        INTERP,
        "    fn rng_guard(&self) -> Result<(), Flow> {\n",
        "    fn rng_guard(&self) -> Result<(), Flow> {\n        if self.sealed_frames.get() > 99 {\n            return panic(\"integrate gate probe\");\n        }\n",
    );
    refuses(
        &r,
        &[],
        "refusal site with no row and no exemption",
        "an unrowed refusal in the anchored seal region was not scanned",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 64: a REGIONS anchor must name one place; a second copy of it
/// would let the region start (or end) wherever the first copy is, silently.
/// Control: the unedited copy holds.
#[test]
fn a_region_anchor_that_is_not_unique_is_refused() {
    let r = tree("anchor");
    holds(&r, &[], "the unedited copy");
    let start = "    /// Whether `f` was defined in a sealed (candidate) module.\n";
    edit(
        &r,
        INTERP,
        "    fn rng_guard(&self) -> Result<(), Flow> {\n",
        &format!("{start}    fn rng_guard(&self) -> Result<(), Flow> {{\n"),
    );
    refuses(
        &r,
        &[],
        "a REGIONS anchor is not in the file exactly once",
        "a REGIONS anchor that is not unique was accepted",
    );
    let _ = std::fs::remove_dir_all(&r);
}
