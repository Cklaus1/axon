//! DEFAULTS READ AS A VALUE (C9 round 12, eqgate8, amendment 110): the drift tests.
//!
//! The behavioural tests of each default live beside the code (`custodian::tests`, `psv::class_tests`,
//! `backend::tests::accept_b263_defaults_are_observed_and_fail_closed`, `submit::tests`). These tests pin the
//! SHAPE that makes them sufficient: the mode a peer's reply names is read through ONE function, so no reader
//! can reintroduce `Mode::parse(..).unwrap_or(Mode::Dev)` (or `Protected`) without this file failing.

use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The non-test lines of a source file: everything before its `#[cfg(test)]` module.
fn production(text: &str) -> Vec<(usize, &str)> {
    let mut v = Vec::new();
    for (i, l) in text.lines().enumerate() {
        if l.trim() == "#[cfg(test)]" {
            break;
        }
        if !l.trim_start().starts_with("//") {
            v.push((i + 1, l));
        }
    }
    v
}

#[test]
fn a_replys_mode_is_read_only_through_mode_from_reply() {
    let mut files = Vec::new();
    rs_files(&root().join("src"), &mut files);
    let mut parses = Vec::new();
    let mut from_reply = std::collections::BTreeMap::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        for (n, l) in production(&text) {
            if l.contains("unwrap_or(Mode::") || l.contains("unwrap_or(crate::custodian::Mode::") {
                panic!("ATTACK: a mode default: {name}:{n} reads a mode with a default: {l}");
            }
            if l.contains("Mode::parse(") {
                parses.push(format!("{name}: {}", l.trim()));
            }
            if l.contains("Mode::from_reply(") {
                *from_reply.entry(name.clone()).or_insert(0usize) += 1;
            }
        }
    }
    parses.sort();
    assert_eq!(
        parses,
        [
            "custodian.rs: if r.schema != REPLY_SCHEMA || Mode::parse(&r.mode).is_none() {",
            "custodian.rs: Mode::parse(s).ok_or_else(|| {",
        ],
        "ATTACK: a new reader of a reply's mode bypasses Mode::from_reply"
    );
    // the four readers of a reply's mode, each through the one function
    assert_eq!(
        from_reply.get("custodian.rs").copied(),
        Some(3),
        "custodian.rs: issue, check and spend read the mode through Mode::from_reply: {from_reply:?}"
    );
    assert_eq!(
        from_reply.get("observer_service.rs").copied(),
        Some(1),
        "observer_service.rs reads the observer's mode through Mode::from_reply: {from_reply:?}"
    );
}

#[test]
fn the_protected_profile_name_has_one_definition() {
    let mut files = Vec::new();
    rs_files(&root().join("..").join("axon-fabric").join("src"), &mut files);
    rs_files(&root().join("..").join("axon-psv").join("src"), &mut files);
    rs_files(&root().join("..").join("axon-loop-contracts").join("src"), &mut files);
    let mut defs = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        for (n, l) in production(&text) {
            if l.contains("const PROTECTED_PROFILE:") {
                defs.push(format!("{}:{n}", f.file_name().unwrap().to_string_lossy()));
            }
        }
    }
    assert_eq!(
        defs,
        ["lib.rs:27"],
        "ATTACK: the protected profile's name is defined more than once (it was in readiness.rs and psv)"
    );
    assert_eq!(axon_psv::PROTECTED_PROFILE, "linux-microvm-protected");
    assert_eq!(axon_fabric::readiness::PROTECTED_PROFILE, axon_psv::PROTECTED_PROFILE);
    assert_eq!(
        axon_loop_contracts::PROTECTED_PROFILES,
        [axon_psv::PROTECTED_PROFILE],
        "the loop's list of protected backends is the PSV profile, no other"
    );
}
