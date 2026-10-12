//! R51 §11 S0: every test ignored as `#[ignore = "R51 Sn: <reason>"]` is listed
//! in `governance/status/r51-deferred-tests.json` with the same file, slice and
//! reason, and every listed test carries that ignore. A deferral the registry
//! does not name, or a registry entry whose test already runs (or is gone),
//! fails here: the set of tests a slice owes is exactly what the registry says.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        if p.is_dir() {
            if name != "target" && name != ".git" {
                rust_files(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

type Entry = (String, String, String, String); // (file, name, slice, reason)

/// `(file, test fn, slice, reason)` for every R51 ignore in `text`.
fn ignores_in(file: &str, text: &str) -> Vec<Entry> {
    const OPEN: &str = "#[ignore = \"R51 ";
    let lines: Vec<&str> = text.lines().collect();
    let mut out = vec![];
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim();
        let Some(rest) = t.strip_prefix(OPEN) else {
            continue;
        };
        let rest = rest.strip_suffix("\"]").unwrap_or_else(|| {
            panic!(
                "{file}:{}: an R51 ignore must be one line `{OPEN}Sn: <reason>\"]`",
                i + 1
            )
        });
        let (slice, reason) = rest
            .split_once(": ")
            .unwrap_or_else(|| panic!("{file}:{}: `R51 Sn: <reason>`, got {rest:?}", i + 1));
        let name = lines[i + 1..]
            .iter()
            .map(|l| l.trim())
            .find_map(|l| {
                let l = l.strip_prefix("pub ").unwrap_or(l);
                let l = l.strip_prefix("async ").unwrap_or(l);
                l.strip_prefix("fn ")
                    .map(|n| n.split(['(', '<']).next().unwrap().to_string())
            })
            .unwrap_or_else(|| panic!("{file}:{}: an R51 ignore with no fn after it", i + 1));
        out.push((
            file.to_string(),
            name,
            slice.to_string(),
            reason.to_string(),
        ));
    }
    out
}

fn ignored_in_tree(root: &Path) -> BTreeSet<Entry> {
    let mut files = vec![];
    rust_files(&root.join("crates"), &mut files);
    let mut set = BTreeSet::new();
    for f in files {
        // This file names the attribute it scans for; it defers nothing.
        if f.ends_with("crates/axon-core/tests/r51_deferred_tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        let rel = f.strip_prefix(root).unwrap().to_string_lossy().into_owned();
        for e in ignores_in(&rel, &text) {
            assert!(set.insert(e.clone()), "the same deferral twice: {e:?}");
        }
    }
    set
}

fn registry(root: &Path) -> BTreeSet<Entry> {
    let p = root.join("governance/status/r51-deferred-tests.json");
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    let mut set = BTreeSet::new();
    for t in v["tests"].as_array().expect("`tests` is an array") {
        let s = |k: &str| {
            t[k].as_str()
                .unwrap_or_else(|| panic!("{t}: no `{k}`"))
                .to_string()
        };
        let e = (s("file"), s("name"), s("slice"), s("reason"));
        assert!(
            ["S0b", "S1", "S2", "S3", "S4", "S5", "S6"].contains(&e.2.as_str()),
            "{e:?}: not an R51 slice that runs after S0"
        );
        assert!(set.insert(e.clone()), "listed twice: {e:?}");
    }
    let count = v["count"].as_u64().expect("`count`") as usize;
    assert_eq!(count, set.len(), "`count` disagrees with the list");
    set
}

#[test]
fn the_r51_deferred_test_registry_is_exactly_the_ignored_set() {
    let root = repo_root();
    let tree = ignored_in_tree(&root);
    let listed = registry(&root);
    let unlisted: Vec<_> = tree.difference(&listed).collect();
    let stale: Vec<_> = listed.difference(&tree).collect();
    assert!(
        unlisted.is_empty() && stale.is_empty(),
        "R51 deferred-test registry drift\n  ignored but not listed (or listed differently): {unlisted:#?}\n  \
         listed but not ignored: {stale:#?}"
    );
}

/// The scanner itself: a deferral is read with its slice and reason, and an
/// ignore without the R51 prefix is not one.
#[test]
fn the_scanner_reads_a_deferral_and_nothing_else() {
    let src = "#[test]\n#[ignore = \"R51 S3: needs the seal edge\"]\nfn attack() {}\n\
               #[test]\n#[ignore = \"slow\"]\nfn other() {}\n";
    assert_eq!(
        ignores_in("f.rs", src),
        vec![(
            "f.rs".to_string(),
            "attack".to_string(),
            "S3".to_string(),
            "needs the seal edge".to_string()
        )]
    );
}
