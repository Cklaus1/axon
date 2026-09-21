//! The durable store is a filesystem effect and must be treated as one.
//!
//! `dstore_open` / `dstore_apply` / `dstore_clear` read, append to and delete
//! a log under the cache directory using `std::fs` directly. They appeared in
//! NEITHER `classify_call` nor `builtin_effect_row`, so every mechanism that
//! asks those tables what a call costs was told: nothing. Reproduced, all four
//! at once:
//!
//!   * `@[contained(fs: [], net: [], exec: none)]` — `axon check` exit 0, and
//!     the run wrote `$XDG_CACHE_HOME/axon/stores/pwned.ndjson`;
//!   * `AXON_ALLOWED_EFFECTS=Pure` — exit 0, same file written;
//!   * `sandbox_create_scoped(p, "IO", "", "", "")` (fs_write deny-all) —
//!     exit 0, same file written;
//!   * `AXON_RECORD` — journal 0 lines, so a run that wrote a file was
//!     indistinguishable from one that touched nothing.
//!
//! The two table-driven guards could not catch it: one asks "does a builtin
//! with a Net/Exec ROW get classified", the other "does a CLASSIFIED builtin
//! declare a row". A builtin missing from both is invisible to each.

use std::path::Path;
use std::process::Command;

fn axon() -> &'static str {
    env!("CARGO_BIN_EXE_axon")
}

fn write(dir: &Path, name: &str, src: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, src).unwrap();
    p
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("axon_dstore_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn wrote(cache: &Path, key: &str) -> bool {
    cache
        .join("axon")
        .join("stores")
        .join(format!("{key}.ndjson"))
        .exists()
}

const CONTAINED: &str = r#"
@[contained(fs: [], net: [], exec: none)]
fn touch_disk() -> i64 {
    let h = dstore_open("k1", 0)
    dstore_apply(h, 1, 7)
}
fn main() { let _ = touch_disk() }
"#;

const PLAIN: &str = r#"
fn main() {
    let h = dstore_open("k2", 0)
    let _ = dstore_apply(h, 1, 7)
}
"#;

#[test]
fn contained_refuses_the_durable_store_statically() {
    let d = tmp("contained");
    let f = write(&d, "c.ax", CONTAINED);
    let out = Command::new(axon()).arg("check").arg(&f).output().unwrap();
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert_ne!(out.status.code(), Some(0), "check accepted it:\n{text}");
    assert!(text.contains("E1001"), "wrong diagnostic:\n{text}");
    // The message must name the builtin the author wrote. Classifying the
    // store KEY as a path made this say `read_file("k1")` and advise
    // `fs: [read("k1")]` — a grant naming one thing while permitting a write
    // to `$XDG_CACHE_HOME/axon/stores/k1.ndjson`, and one that cannot be
    // satisfied anyway because `dstore_apply`'s first argument is a handle.
    assert!(
        text.contains("dstore_open"),
        "the diagnostic names the wrong builtin:\n{text}"
    );
    assert!(
        !text.contains("read_file(\"k1\")"),
        "the diagnostic still reports a store key as a path:\n{text}"
    );
}

/// `is_impure_builtin` is a THIRD table, and it disagreed with the other two:
/// `@[pure]` accepted a function that wrote to disk (`axon check` exit 0, the
/// .ndjson written). The same table gates refinement-predicate purity, so a
/// `where` clause could have called it too. Found by the existing
/// cross-table test, not by me.
#[test]
fn a_pure_function_may_not_use_the_durable_store() {
    let d = tmp("pure");
    let f = write(
        &d,
        "pure.ax",
        r#"
@[pure]
fn p() -> i64 {
    let h = dstore_open("k5", 0)
    dstore_apply(h, 1, 7)
}
fn main() { println(to_str(p())) }
"#,
    );
    let cache = d.join("cache");
    let out = Command::new(axon())
        .arg("check")
        .arg(&f)
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert_ne!(
        out.status.code(),
        Some(0),
        "@[pure] accepted a disk write:\n{text}"
    );
    assert!(text.contains("E1207"), "wrong diagnostic:\n{text}");
    assert!(!wrote(&cache, "k5"));
}

#[test]
fn an_ambient_pure_ceiling_refuses_the_durable_store() {
    let d = tmp("ceiling");
    let f = write(&d, "p.ax", PLAIN);
    let cache = d.join("cache");
    let out = Command::new(axon())
        .arg("run")
        .arg(&f)
        .env("XDG_CACHE_HOME", &cache)
        .env("AXON_ALLOWED_EFFECTS", "Pure")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(8), "expected a sandbox violation");
    assert!(
        !wrote(&cache, "k2"),
        "a Pure ceiling permitted a disk write"
    );
}

#[test]
fn a_scoped_sandbox_checks_the_real_store_directory() {
    // DENY: the store carries no path argument, so the per-argument scope loop
    // saw nothing to check and the call fell through to "no restriction".
    let d = tmp("scoped");
    let deny = write(
        &d,
        "deny.ax",
        r#"
fn inner(x: i64) -> i64 {
    let h = dstore_open("k3", 0)
    dstore_apply(h, 1, x)
}
fn main() {
    let p = principal_root("p", false, false, false, 100)
    let s = sandbox_create_scoped(p, "IO", "", "", "")
    println(to_str(sandbox_run(s, "inner", 3)))
}
"#,
    );
    let cache = d.join("deny_cache");
    let out = Command::new(axon())
        .arg("run")
        .arg(&deny)
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(8),
        "fs_write deny-all permitted a write"
    );
    assert!(!wrote(&cache, "k3"));

    // ALLOW: the same program under a scope that DOES grant the directory must
    // still work, or the fix is "refuse everything" rather than "enforce".
    let allow = write(
        &d,
        "allow.ax",
        r#"
fn inner(x: i64) -> i64 {
    let h = dstore_open("k4", 0)
    dstore_apply(h, 1, x)
}
fn main() {
    let p = principal_root("p", false, true, false, 100)
    let s = sandbox_create_scoped(p, "IO", "*", "*", "")
    println(to_str(sandbox_run(s, "inner", 3)))
}
"#,
    );
    let cache2 = d.join("allow_cache");
    let out2 = Command::new(axon())
        .arg("run")
        .arg(&allow)
        .env("XDG_CACHE_HOME", &cache2)
        .output()
        .unwrap();
    assert_eq!(
        out2.status.code(),
        Some(0),
        "a granting scope must still permit the store: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(wrote(&cache2, "k4"));
}

#[test]
fn the_durable_store_refuses_to_run_under_replay_and_says_so_under_record() {
    let d = tmp("journal");
    let f = write(&d, "p.ax", PLAIN);
    let j = d.join("run.journal");

    // RECORD: the run proceeds — recording is observation — but the journal
    // cannot represent these calls, and an incomplete journal that says
    // nothing is a recording that lies by omission.
    let rec = Command::new(axon())
        .arg("run")
        .arg(&f)
        .env("XDG_CACHE_HOME", d.join("rc"))
        .env("AXON_RECORD", &j)
        .output()
        .unwrap();
    assert_eq!(rec.status.code(), Some(0));
    let rec_err = String::from_utf8_lossy(&rec.stderr);
    assert!(
        rec_err.contains("INCOMPLETE"),
        "an unrecordable write was recorded silently:\n{rec_err}"
    );

    // REPLAY: must refuse. Serving the rest of the run from a journal while
    // these calls read live state is the exact hazard replay removes.
    let cache = d.join("rp");
    let rep = Command::new(axon())
        .arg("run")
        .arg(&f)
        .env("XDG_CACHE_HOME", &cache)
        .env("AXON_REPLAY", &j)
        .output()
        .unwrap();
    assert_ne!(rep.status.code(), Some(0), "replay consulted live state");
    assert!(
        String::from_utf8_lossy(&rep.stderr).contains("AXON_REPLAY"),
        "refusal does not say why: {}",
        String::from_utf8_lossy(&rep.stderr)
    );
    assert!(
        !wrote(&cache, "k2"),
        "a replayed run wrote to the live store"
    );
}
