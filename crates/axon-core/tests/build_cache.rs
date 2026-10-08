//! The native build cache (AX-34).
//!
//! A cache entry is the hosted program's relocatable object AFTER the
//! opt-level IR pipeline and the backend, so a hit only links. It used to be
//! the pre-optimisation bitcode, and a hit re-ran the whole O2 pipeline and
//! instruction selection: a warm `big-compile --release` cost 104.9 G
//! instructions against 105.6 G with `--no-cache`.
//!
//! Every test returns early when the binary was built without the `codegen`
//! feature (the gate's `--no-default-features` stage), like the native tests in
//! `cli_run.rs`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn axon() -> Command {
    Command::new(env!("CARGO_BIN_EXE_axon"))
}

fn codegen_absent(out: &Output) -> bool {
    all_output(out).contains("requires building axon with the `codegen` feature")
}

fn all_output(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// A fresh scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon_buildcache_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn hello(dir: &Path, name: &str, text: &str) -> PathBuf {
    let f = dir.join(format!("{name}.ax"));
    std::fs::write(&f, format!("fn main() {{\n  println(\"{text}\")\n}}\n")).unwrap();
    f
}

fn build(src: &Path, out: &Path, cache: &Path, extra: &[&str]) -> Output {
    axon()
        .arg("build")
        .args(extra)
        .arg(src)
        .arg("-o")
        .arg(out)
        .arg("--cache-dir")
        .arg(cache)
        .output()
        .expect("spawn axon build")
}

fn entries(cache: &Path) -> Vec<PathBuf> {
    let mut v: Vec<_> = std::fs::read_dir(cache)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "axc"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// The single entry one build wrote.
fn only_entry(cache: &Path) -> PathBuf {
    let mut v = entries(cache);
    assert_eq!(v.len(), 1, "one build must write one entry: {v:?}");
    v.pop().unwrap()
}

fn run(bin: &Path) -> String {
    let o = Command::new(bin).output().expect("run built binary");
    String::from_utf8_lossy(&o.stdout).into_owned()
}

/// The object stored in a format-2 entry (see `cache.rs`).
fn stored_object(entry: &[u8]) -> &[u8] {
    assert_eq!(&entry[..8], b"AXONCACH");
    assert_eq!(u32::from_le_bytes(entry[8..12].try_into().unwrap()), 2);
    let id_len = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
    &entry[16 + id_len + 1 + 8 + 32..]
}

/// A hit links the STORED object: nothing is recompiled from the source and
/// no IR pipeline runs. Proved by swapping the entry for another program's —
/// a hit that still compiled the source would print the source's text.
#[test]
fn a_cache_hit_links_the_stored_object_without_recompiling() {
    let dir = scratch("hit");
    let (cache_a, cache_b) = (dir.join("cache_a"), dir.join("cache_b"));
    let a = hello(&dir, "app", "FROM_A");
    let b_dir = dir.join("b");
    std::fs::create_dir_all(&b_dir).unwrap();
    let b = hello(&b_dir, "app", "FROM_B");
    let bin = dir.join("bin");

    let cold = build(&a, &bin, &cache_a, &["--release"]);
    if codegen_absent(&cold) {
        return;
    }
    assert_eq!(cold.status.code(), Some(0), "{}", all_output(&cold));
    let cold_bin = std::fs::read(&bin).unwrap();
    let entry_a = only_entry(&cache_a);

    // The entry is a relocatable ELF object (ET_REL), i.e. post-backend: a hit
    // has nothing left to optimise or select instructions for.
    let stored = std::fs::read(&entry_a).unwrap();
    let obj = stored_object(&stored);
    assert_eq!(&obj[..4], b"\x7fELF", "the cache must store an object");
    assert_eq!(u16::from_le_bytes([obj[16], obj[17]]), 1, "ET_REL");

    // Warm: same bytes as the cold build and as `--no-cache`.
    let warm = build(&a, &bin, &cache_a, &["--release"]);
    assert_eq!(warm.status.code(), Some(0), "{}", all_output(&warm));
    assert!(
        !all_output(&warm).contains("warning"),
        "{}",
        all_output(&warm)
    );
    assert_eq!(std::fs::read(&bin).unwrap(), cold_bin, "hit != cold build");
    let nc = build(&a, &bin, &cache_a, &["--release", "--no-cache"]);
    assert_eq!(nc.status.code(), Some(0), "{}", all_output(&nc));
    assert_eq!(
        std::fs::read(&bin).unwrap(),
        cold_bin,
        "hit != --no-cache build"
    );

    // Swap in B's entry (valid, same compiler) under A's key.
    let ob = build(&b, &dir.join("bin_b"), &cache_b, &["--release"]);
    assert_eq!(ob.status.code(), Some(0), "{}", all_output(&ob));
    let entry_b = only_entry(&cache_b);
    std::fs::copy(&entry_b, &entry_a).unwrap();
    let swapped = build(&a, &bin, &cache_a, &["--release"]);
    assert_eq!(swapped.status.code(), Some(0), "{}", all_output(&swapped));
    assert_eq!(
        run(&bin),
        "FROM_B\n",
        "a hit must link the stored object, not recompile the source"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The opt level changes the object, so it is part of the key: a build at
/// another level misses, and `--release` hits the `--opt-level 2` entry.
#[test]
fn changing_the_opt_level_misses_the_cache() {
    let dir = scratch("optlevel");
    let cache = dir.join("cache");
    let src = hello(&dir, "app", "hi");
    let bin = dir.join("bin");

    let o1 = build(&src, &bin, &cache, &["--opt-level", "1"]);
    if codegen_absent(&o1) {
        return;
    }
    assert_eq!(o1.status.code(), Some(0), "{}", all_output(&o1));
    assert_eq!(entries(&cache).len(), 1);

    let o2 = build(&src, &bin, &cache, &["--opt-level", "2"]);
    assert_eq!(o2.status.code(), Some(0), "{}", all_output(&o2));
    assert_eq!(entries(&cache).len(), 2, "O2 must not reuse the O1 entry");
    assert_eq!(run(&bin), "hi\n");

    let release = build(&src, &bin, &cache, &["--release"]);
    assert_eq!(release.status.code(), Some(0), "{}", all_output(&release));
    assert_eq!(entries(&cache).len(), 2, "--release is O2: same entry");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A damaged or old-format entry is detected before anything is linked,
/// reported (E0906), rebuilt and overwritten — the next build hits again.
#[test]
fn a_corrupt_or_old_format_entry_is_rebuilt_not_linked() {
    let dir = scratch("corrupt");
    let cache = dir.join("cache");
    let src = hello(&dir, "app", "intact");
    let bin = dir.join("bin");

    let first = build(&src, &bin, &cache, &[]);
    if codegen_absent(&first) {
        return;
    }
    assert_eq!(first.status.code(), Some(0), "{}", all_output(&first));
    let entry = only_entry(&cache);
    let good = std::fs::read(&entry).unwrap();

    // One flipped byte inside the object's code: still the right length, still
    // an ELF, and linkable — only the checksum can tell.
    let mut flipped = good.clone();
    let at = good.len() - good.len() / 3;
    flipped[at] ^= 0xff;
    // A format-1 entry: identity length where the format version now is.
    let mut v1 = b"AXONCACH".to_vec();
    v1.extend_from_slice(&5u32.to_le_bytes());
    v1.extend_from_slice(b"0.1.0BC\xc0\xde");

    for (label, bytes, why) in [
        ("bit flip", flipped, "checksum"),
        ("old format", v1, "cache format"),
    ] {
        std::fs::write(&entry, &bytes).unwrap();
        let _ = std::fs::remove_file(&bin);
        let o = build(&src, &bin, &cache, &[]);
        let msg = all_output(&o);
        assert_eq!(o.status.code(), Some(0), "{label}: {msg}");
        assert!(
            msg.contains("warning[E0906]") && msg.contains(why),
            "{label}: the unusable entry must be reported: {msg}"
        );
        assert_eq!(run(&bin), "intact\n", "{label}");
        assert_eq!(
            std::fs::read(&entry).unwrap(),
            good,
            "{label}: the rebuild must overwrite the entry"
        );
        let again = build(&src, &bin, &cache, &[]);
        assert!(!all_output(&again).contains("E0906"), "{label}: now a hit");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
