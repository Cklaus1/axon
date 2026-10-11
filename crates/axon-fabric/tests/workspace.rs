//! B261 — WorkspaceVersion: the cross-language identity vector, one negative
//! per import-refusal class (each asserting the store is left EMPTY), the
//! write-once content-addressed store, G28 (a hash-only observation is never
//! materialized), per-trial caches, and `check_target` accepting a manifest
//! ref through the real submit path.

mod common;
use common::*;

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};

use axon_fabric::submit;
use axon_fabric::workspace::{
    EntryKind, ImportRefusal, Omission, Quota, StoreError, TreeEntry, TrialCache,
    WorkspaceProjection, WorkspaceStore, WorkspaceTree, MAX_BYTES, MAX_DEPTH, MAX_ENTRIES,
};
use axon_loop_contracts::{Acf1Ref, TrialId};
use serde_json::Value;

fn vector() -> Value {
    serde_json::from_str(include_str!("fixtures/workspace_version_vector.json")).unwrap()
}

fn set_exec(p: &Path, exec: bool) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        p,
        std::fs::Permissions::from_mode(if exec { 0o755 } else { 0o644 }),
    )
    .unwrap();
}

/// Write the vector's `tree` under `root`.
fn materialize_vector(root: &Path, v: &Value) {
    for e in v["tree"].as_array().unwrap() {
        let p = root.join(e["path"].as_str().unwrap());
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        match e["kind"].as_str().unwrap() {
            "file" => {
                std::fs::write(&p, e["content"].as_str().unwrap()).unwrap();
                set_exec(&p, e["executable"].as_bool().unwrap());
            }
            "symlink" => std::os::unix::fs::symlink(e["target"].as_str().unwrap(), &p).unwrap(),
            k => panic!("unknown kind {k}"),
        }
    }
}

fn files_under(p: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    rd.flatten()
        .map(|e| {
            let m = std::fs::symlink_metadata(e.path()).unwrap();
            if m.is_dir() {
                files_under(&e.path())
            } else {
                1
            }
        })
        .sum()
}

// ── the cross-language vector ───────────────────────────────────────────────

#[test]
fn the_cross_language_vector_reproduces_byte_for_byte() {
    let v = vector();
    assert_eq!(v["schema"], "micode.workspace-version-vector/1");
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    materialize_vector(&root, &v);
    // Top-level .git / .micode are omitted — and the omission is RECORDED.
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/HEAD"), "ref: x\n").unwrap();
    std::fs::create_dir_all(root.join(".micode")).unwrap();
    std::fs::write(root.join(".micode/state"), "s").unwrap();
    // An empty directory is invisible.
    std::fs::create_dir_all(root.join("empty-dir/deeper")).unwrap();

    let t = WorkspaceTree::import_dir(&root, &Quota::default()).unwrap();
    let manifest = String::from_utf8(t.manifest()).unwrap();
    assert_eq!(manifest, v["manifest"].as_str().unwrap(), "manifest bytes");
    let msha = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(manifest.as_bytes()))
    };
    assert_eq!(msha, v["manifest_sha256"].as_str().unwrap());
    let identity = axon_cortex::runner::acf1_canonical_bytes(&[
        ("manifest_sha256", &msha),
        ("schema", "axon.workspace-version/1"),
    ]);
    assert_eq!(
        String::from_utf8(identity).unwrap(),
        v["identity_object"].as_str().unwrap()
    );
    assert_eq!(t.reference().as_str(), v["reference"].as_str().unwrap());
    assert_eq!(
        t.omissions(),
        &[
            Omission {
                path: ".git".into(),
                reason: "top-level repository/runtime state (recipe §1)".into()
            },
            Omission {
                path: ".micode".into(),
                reason: "top-level repository/runtime state (recipe §1)".into()
            },
        ]
    );

    // The same tree as an explicit entry list gives the same reference.
    let listed: Vec<TreeEntry> = v["tree"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| match e["kind"].as_str().unwrap() {
            "file" => TreeEntry {
                path: e["path"].as_str().unwrap().into(),
                kind: EntryKind::File {
                    executable: e["executable"].as_bool().unwrap(),
                },
                content: e["content"].as_str().unwrap().as_bytes().to_vec(),
            },
            _ => TreeEntry {
                path: e["path"].as_str().unwrap().into(),
                kind: EntryKind::Symlink,
                content: e["target"].as_str().unwrap().as_bytes().to_vec(),
            },
        })
        .collect();
    let t2 = WorkspaceTree::from_entries(listed, vec![], &Quota::default()).unwrap();
    assert_eq!(t2.reference(), t.reference());

    // One-byte flip: XOR the first byte of README.md with 0x01.
    let flip = root.join(v["one_byte_flip"]["path"].as_str().unwrap());
    let mut b = std::fs::read(&flip).unwrap();
    b[0] ^= 0x01;
    std::fs::write(&flip, b).unwrap();
    let t3 = WorkspaceTree::import_dir(&root, &Quota::default()).unwrap();
    assert_ne!(t3.reference(), t.reference(), "one byte must move the ref");
    assert_ne!(t3.reference().as_str(), v["reference"].as_str().unwrap());
}

#[test]
fn the_executable_bit_and_link_targets_are_identity() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("t");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("x"), "x").unwrap();
    set_exec(&root.join("x"), false);
    let a = WorkspaceTree::import_dir(&root, &Quota::default())
        .unwrap()
        .reference();
    set_exec(&root.join("x"), true);
    let b = WorkspaceTree::import_dir(&root, &Quota::default())
        .unwrap()
        .reference();
    assert_ne!(a, b, "mode 100644 vs 100755");
    std::os::unix::fs::symlink("x", root.join("l")).unwrap();
    let c = WorkspaceTree::import_dir(&root, &Quota::default())
        .unwrap()
        .reference();
    std::fs::remove_file(root.join("l")).unwrap();
    std::fs::write(root.join("l"), "x").unwrap(); // same BYTES, not a link
    let d = WorkspaceTree::import_dir(&root, &Quota::default())
        .unwrap()
        .reference();
    assert_ne!(c, d, "a symlink is not a file holding its target");
}

// ── refusals: one per class, each leaving the store empty ───────────────────

struct Case {
    _dir: tempfile::TempDir,
    root: PathBuf,
    state: PathBuf,
}

fn case() -> Case {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("ok.txt"), "fine\n").unwrap();
    let state = dir.path().join("state");
    Case {
        root,
        state,
        _dir: dir,
    }
}

/// Import through the store and assert it refused with `class` and
/// published NOTHING (no blob, no manifest).
fn refused(c: &Case, quota: &Quota, class: &str) -> ImportRefusal {
    let store = WorkspaceStore::open(&c.state, &tenant()).unwrap();
    let err = store.import_dir(&c.root, quota).unwrap_err();
    let StoreError::Refused(r) = err else {
        panic!("expected a refusal, got {err}")
    };
    assert_eq!(r.class(), class, "{r}");
    assert_eq!(
        files_under(&c.state),
        0,
        "a refused import publishes nothing"
    );
    r
}

fn entries_refused(entries: Vec<TreeEntry>, class: &str) {
    let r = WorkspaceTree::from_entries(entries, vec![], &Quota::default()).unwrap_err();
    assert_eq!(r.class(), class, "{r}");
}

fn file(path: &str) -> TreeEntry {
    TreeEntry {
        path: path.into(),
        kind: EntryKind::File { executable: false },
        content: b"x".to_vec(),
    }
}

#[test]
fn refuses_traversal() {
    for p in ["a/../b", "../x", "./x", "a//b", "a/", "."] {
        entries_refused(vec![file(p)], "traversal");
    }
}

#[test]
fn refuses_absolute_paths() {
    entries_refused(vec![file("/etc/passwd")], "absolute");
}

#[test]
fn refuses_symlinks_leaving_the_root() {
    let c = case();
    std::fs::create_dir_all(c.root.join("src")).unwrap();
    std::os::unix::fs::symlink("../../outside", c.root.join("src/esc")).unwrap();
    refused(&c, &Quota::default(), "escaping_symlink");

    let c = case();
    std::os::unix::fs::symlink("/etc/passwd", c.root.join("abs")).unwrap();
    refused(&c, &Quota::default(), "escaping_symlink");

    // Pops above the root part-way, even though it comes back down.
    let c = case();
    std::os::unix::fs::symlink("../tree/ok.txt", c.root.join("sneaky")).unwrap();
    refused(&c, &Quota::default(), "escaping_symlink");

    // Inside the root is fine (the vector has `src/up -> ../README.md`).
    let c = case();
    std::fs::create_dir_all(c.root.join("src")).unwrap();
    std::os::unix::fs::symlink("../ok.txt", c.root.join("src/up")).unwrap();
    WorkspaceStore::open(&c.state, &tenant())
        .unwrap()
        .import_dir(&c.root, &Quota::default())
        .unwrap();
}

#[test]
fn refuses_devices_fifos_and_sockets() {
    let c = case();
    let p = std::ffi::CString::new(c.root.join("fifo").to_str().unwrap()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(p.as_ptr(), 0o644) }, 0);
    refused(&c, &Quota::default(), "special_file");

    let c = case();
    let _l = std::os::unix::net::UnixListener::bind(c.root.join("sock")).unwrap();
    refused(&c, &Quota::default(), "special_file");
}

#[test]
fn refuses_non_utf8_names() {
    use std::os::unix::ffi::OsStrExt;
    let c = case();
    std::fs::write(
        c.root.join(std::ffi::OsStr::from_bytes(b"bad\xffname")),
        "x",
    )
    .unwrap();
    refused(&c, &Quota::default(), "non_utf8");
}

#[test]
fn refuses_control_characters() {
    let c = case();
    std::fs::write(c.root.join("a\nb"), "x").unwrap();
    refused(&c, &Quota::default(), "control_character");
    entries_refused(vec![file("a\u{7f}b")], "control_character");
}

#[test]
fn refuses_duplicates() {
    entries_refused(vec![file("a"), file("b"), file("a")], "duplicate");
}

#[test]
fn refuses_case_and_nfc_collisions() {
    let c = case();
    std::fs::write(c.root.join("README"), "1").unwrap();
    std::fs::write(c.root.join("readme"), "2").unwrap();
    refused(&c, &Quota::default(), "collision");

    let c = case();
    std::fs::write(c.root.join("caf\u{e9}.txt"), "nfc").unwrap();
    std::fs::write(c.root.join("cafe\u{301}.txt"), "nfd").unwrap();
    refused(&c, &Quota::default(), "collision");

    // Directories collide too.
    let c = case();
    std::fs::create_dir_all(c.root.join("Src")).unwrap();
    std::fs::create_dir_all(c.root.join("src")).unwrap();
    std::fs::write(c.root.join("Src/a"), "1").unwrap();
    std::fs::write(c.root.join("src/b"), "2").unwrap();
    refused(&c, &Quota::default(), "collision");

    // A path that is both a file and a directory (explicit lists only).
    entries_refused(vec![file("a"), file("a/b")], "collision");
}

#[test]
fn the_quota_defaults_are_operator_decision_d11() {
    assert_eq!(
        Quota::default(),
        Quota {
            entries: 20_000,
            bytes: 256 * 1024 * 1024,
            depth: 32
        }
    );
    assert_eq!(
        (MAX_ENTRIES, MAX_BYTES, MAX_DEPTH),
        (20_000, 268_435_456, 32)
    );
}

#[test]
fn refuses_entry_quota_overflow() {
    let c = case();
    for i in 0..3 {
        std::fs::write(c.root.join(format!("f{i}")), "x").unwrap();
    }
    // 4 entries against a limit of 3; exactly at the limit is accepted.
    refused(
        &c,
        &Quota {
            entries: 3,
            ..Quota::default()
        },
        "quota_entries",
    );
    std::fs::remove_file(c.root.join("f2")).unwrap();
    WorkspaceStore::open(&c.state, &tenant())
        .unwrap()
        .import_dir(
            &c.root,
            &Quota {
                entries: 3,
                ..Quota::default()
            },
        )
        .unwrap();
}

#[test]
fn refuses_byte_quota_overflow() {
    let c = case(); // ok.txt = 5 bytes
    std::fs::write(c.root.join("big"), vec![b'x'; 6]).unwrap();
    refused(
        &c,
        &Quota {
            bytes: 10,
            ..Quota::default()
        },
        "quota_bytes",
    );
    // An explicit entry list is held to the same byte quota.
    let r = WorkspaceTree::from_entries(
        vec![file("a"), file("b"), file("c")],
        vec![],
        &Quota {
            bytes: 2,
            ..Quota::default()
        },
    )
    .unwrap_err();
    assert_eq!(r.class(), "quota_bytes");
    // Symlink targets count as content.
    let c = case();
    std::os::unix::fs::symlink("ok.txt", c.root.join("l")).unwrap(); // +6
    refused(
        &c,
        &Quota {
            bytes: 10,
            ..Quota::default()
        },
        "quota_bytes",
    );
}

#[test]
fn refuses_depth_quota_overflow_at_the_default() {
    let c = case();
    let deep: Vec<String> = (0..MAX_DEPTH).map(|i| format!("d{i}")).collect();
    // 32 components (31 dirs + file) is accepted …
    let ok = c.root.join(deep[..MAX_DEPTH - 1].join("/"));
    std::fs::create_dir_all(&ok).unwrap();
    std::fs::write(ok.join("f"), "x").unwrap();
    WorkspaceTree::import_dir(&c.root, &Quota::default()).unwrap();
    // … 33 is refused.
    let too = c.root.join(deep.join("/"));
    std::fs::create_dir_all(&too).unwrap();
    std::fs::write(too.join("f"), "x").unwrap();
    refused(&c, &Quota::default(), "quota_depth");
}

// ── the store ───────────────────────────────────────────────────────────────

#[test]
fn publish_is_write_once_and_materialize_round_trips() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    materialize_vector(&root, &vector());
    let store = WorkspaceStore::open(&dir.path().join("state"), &tenant()).unwrap();
    let r = store.import_dir(&root, &Quota::default()).unwrap();
    assert_eq!(r.as_str(), vector()["reference"].as_str().unwrap());
    let mpath = ws_root(&dir.path().join("state"))
        .join("versions")
        .join(format!("{}.manifest", &r.as_str()[5..]));
    let ino = std::fs::metadata(&mpath).unwrap().ino();
    assert_eq!(store.import_dir(&root, &Quota::default()).unwrap(), r);
    assert_eq!(
        std::fs::metadata(&mpath).unwrap().ino(),
        ino,
        "a second publish of the same version never replaces the first"
    );
    let v = store.load(&r).unwrap();
    assert_eq!(
        v.manifest,
        vector()["manifest"].as_str().unwrap().as_bytes()
    );

    let out = dir.path().join("out");
    store.materialize(&r, &out, false).unwrap();
    assert_eq!(
        WorkspaceTree::import_dir(&out, &Quota::default())
            .unwrap()
            .reference(),
        r,
        "materialization reproduces the version exactly (modes and links)"
    );
    // An existing destination is refused, never merged into.
    assert!(matches!(
        store.materialize(&r, &out, false),
        Err(StoreError::DestinationExists(_))
    ));
    // Read-only materialization: files 0444/0555, dirs 0555.
    let ro = dir.path().join("ro");
    store.materialize(&r, &ro, true).unwrap();
    assert_eq!(
        std::fs::metadata(ro.join("README.md")).unwrap().mode() & 0o777,
        0o444
    );
    assert_eq!(
        std::fs::metadata(ro.join("bin/run.sh")).unwrap().mode() & 0o777,
        0o555
    );
    assert_eq!(
        std::fs::metadata(ro.join("src")).unwrap().mode() & 0o777,
        0o555
    );
    axon_fabric::workspace::remove_tree(&ro).unwrap();
}

#[test]
fn a_tampered_blob_is_refused_and_nothing_is_materialized() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    materialize_vector(&root, &vector());
    let store = WorkspaceStore::open(&dir.path().join("state"), &tenant()).unwrap();
    let r = store.import_dir(&root, &Quota::default()).unwrap();
    // README.md's blob.
    let blob = ws_root(&dir.path().join("state"))
        .join("blobs/bad18e717145fcf190f9144d635a3295ab55ffc8cddcfc94b6c4b94b00093b42");
    std::fs::write(&blob, "hello workspacE\n").unwrap();
    let out = dir.path().join("out");
    assert!(matches!(
        store.materialize(&r, &out, false),
        Err(StoreError::Corrupt(_))
    ));
    assert!(!out.exists(), "no partial materialization");
}

// ── G28: a workspace observation is not a workspace ─────────────────────────

#[test]
fn a_hash_only_observation_cannot_be_materialized() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    materialize_vector(&root, &vector());
    let store = WorkspaceStore::open(&dir.path().join("state"), &tenant()).unwrap();
    // A snapshot the observer HASHED — the vector's own reference — but never
    // published: the store holds no content for it.
    let snapshot = vector()["reference"].as_str().unwrap().to_string();
    let hash_only = WorkspaceProjection {
        snapshot_ref: snapshot.clone(),
        version_ref: None,
        omissions: vec![],
        observed_at_ms: 1,
    };
    let out = dir.path().join("out");
    assert!(matches!(
        store.materialize_projection(&hash_only, &out, false),
        Err(StoreError::HashOnly(_))
    ));
    assert!(!out.exists());
    // Naming the hash AS a version does not make it content.
    let named = WorkspaceProjection {
        version_ref: Some(Acf1Ref::new(snapshot).unwrap()),
        ..hash_only.clone()
    };
    assert!(matches!(
        store.materialize_projection(&named, &out, false),
        Err(StoreError::NotPublished(_))
    ));
    assert!(!out.exists());
    // Once published, the same projection materializes, and it carries the
    // version's omissions explicitly.
    std::fs::create_dir_all(root.join(".git")).unwrap();
    let r = store.import_dir(&root, &Quota::default()).unwrap();
    let v = store.load(&r).unwrap();
    let proj = WorkspaceProjection {
        version_ref: Some(r.clone()),
        omissions: v.omissions.clone(),
        ..hash_only
    };
    assert_eq!(proj.omissions.len(), 1, "the .git omission is explicit");
    store.materialize_projection(&proj, &out, false).unwrap();
    assert_eq!(
        WorkspaceTree::import_dir(&out, &Quota::default())
            .unwrap()
            .reference(),
        r
    );
}

// ── per-trial caches ────────────────────────────────────────────────────────

#[test]
fn two_trials_caches_do_not_see_each_others_writes() {
    let dir = tempfile::tempdir().unwrap();
    let a =
        TrialCache::for_trial(dir.path(), &tenant(), &TrialId::new("trial-a").unwrap()).unwrap();
    let b =
        TrialCache::for_trial(dir.path(), &tenant(), &TrialId::new("trial-b").unwrap()).unwrap();
    assert_ne!(a.root, b.root);
    for ((ka, va), (kb, vb)) in a.env().iter().zip(b.env().iter()) {
        assert_eq!(ka, kb);
        assert_ne!(va, vb, "{ka} is per-trial");
        std::fs::write(Path::new(va).join("marker"), "a").unwrap();
        assert!(!Path::new(vb).join("marker").exists());
    }
    // Keyed by TrialId: the same trial gets the same directories back.
    let a2 =
        TrialCache::for_trial(dir.path(), &tenant(), &TrialId::new("trial-a").unwrap()).unwrap();
    assert_eq!(a2, a);
}

const CACHE_PROBE: &str = r#"
@[test]
fn t_write_home() {
    match env_var("HOME") {
        Ok(h) => {
            let _ = write_file("{h}/marker", "written")
            assert(file_exists("{h}/marker"))
        }
        Err(e) => assert(false)
    }
}

@[test]
fn t_home_clean() {
    match env_var("HOME") {
        Ok(h) => assert(!file_exists("{h}/marker"))
        Err(e) => assert(false)
    }
}
"#;

#[test]
fn through_submit_a_trials_home_is_its_own() {
    let env = Env::new();
    std::fs::write(env.ws.join("f.ax"), CACHE_PROBE).unwrap();
    let cfg = env.cfg(0);
    let mut ra = request(&env, "op-a1", "t_write_home");
    ra["trial_id"] = "trial-a".into();
    let s = submit(&ra.to_string(), &cfg).unwrap();
    assert_eq!(
        s.check_report.as_ref().unwrap()["passed"][0],
        "t_write_home"
    );
    // Same trial, next attempt: sees its own marker.
    let mut ra2 = request(&env, "op-a2", "t_home_clean");
    ra2["trial_id"] = "trial-a".into();
    let s = submit(&ra2.to_string(), &cfg).unwrap();
    assert_eq!(
        s.check_report.as_ref().unwrap()["failed"][0],
        "t_home_clean"
    );
    // Another trial: does not.
    let mut rb = request(&env, "op-b1", "t_home_clean");
    rb["trial_id"] = "trial-b".into();
    let s = submit(&rb.to_string(), &cfg).unwrap();
    assert_eq!(
        s.check_report.as_ref().unwrap()["passed"][0],
        "t_home_clean"
    );
    // And the operator's workspace was not the cache.
    assert!(!env.ws.join("marker").exists());
}

// ── check_target accepts a manifest ref ─────────────────────────────────────

fn publish_ws(env: &Env) -> Acf1Ref {
    let cfg = env.cfg(0);
    std::fs::write(env.ws.join("notes.md"), "not a check\n").unwrap();
    WorkspaceStore::open(&cfg.state_dir, &tenant())
        .unwrap()
        .import_dir(&env.ws, &Quota::default())
        .unwrap()
}

#[test]
fn a_published_workspace_version_is_accepted_and_journalled() {
    let env = Env::new();
    let r = publish_ws(&env);
    let mut req = request(&env, "op-v", "t_ok");
    req["workspace_version_ref"] = r.as_str().into();
    let s = submit(&req.to_string(), &env.cfg(0)).unwrap();
    assert_eq!(
        s.receipt.verification,
        axon_loop_contracts::ReceiptVerification::Passed
    );
    assert_eq!(s.receipt.input_workspace_ref, r);
    assert!(
        env.journal_text()
            .contains(&format!("\"workspace_version_ref\":\"{r}\"")),
        "the journal records which version the run read"
    );
    // The per-op materialization is cleaned up.
    assert_eq!(
        std::fs::read_dir(env.dir.path().join("fabric-state/runs"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn a_version_without_the_argv_file_or_an_unpublished_ref_launches_nothing() {
    let env = Env::new();
    let r = publish_ws(&env);
    let mut req = request(&env, "op-missing", "t_ok");
    req["workspace_version_ref"] = r.as_str().into();
    req["argv"] = serde_json::json!(["other.ax", "t_ok"]);
    let e = submit(&req.to_string(), &env.cfg(0)).unwrap_err();
    assert_eq!(e.kind(), "malformed", "{e}");

    let mut req = request(&env, "op-unpub", "t_ok");
    req["workspace_version_ref"] = format!("acf1:{}", "7".repeat(64)).into();
    let e = submit(&req.to_string(), &env.cfg(0)).unwrap_err();
    assert_eq!(e.kind(), "conflict", "{e}");
    assert_eq!(spawn_count(&env.spawns), 0);
    assert_eq!(env.launch_records(), 0);
}

const FIXTURE_BROKEN: &str = "\
fn double(n: i64) -> i64 { n * 3 }

@[test]
fn t_ok() { assert_eq(double(2), 4) }
";

fn swap_workspace_file(cfg: &axon_fabric::SubmitConfig) {
    std::fs::write(cfg.workspace.join("f.ax"), FIXTURE_BROKEN).unwrap();
}

#[test]
fn a_one_file_version_judges_the_bytes_it_hashed_not_the_live_file() {
    let env = Env::new();
    let bytes = std::fs::read(env.ws.join("f.ax")).unwrap();
    let r = axon_cortex::runner::single_file_workspace_version_ref("f.ax", &bytes);
    let mut req = request(&env, "op-1f", "t_ok");
    req["workspace_version_ref"] = r.clone().into();
    let mut cfg = env.cfg(0);
    // The live file is replaced after the ref is checked, before launch.
    cfg.pre_launch_hook = Some(swap_workspace_file);
    let s = submit(&req.to_string(), &cfg).unwrap();
    assert_eq!(
        s.receipt.verification,
        axon_loop_contracts::ReceiptVerification::Passed,
        "the stored copy was judged, not the swapped live file: {:?}",
        s.check_report
    );
    assert_eq!(s.receipt.input_workspace_ref.as_str(), r);
    assert!(WorkspaceStore::open(&cfg.state_dir, &tenant())
        .unwrap()
        .contains(&Acf1Ref::new(r).unwrap()));
}

#[test]
fn the_historical_single_file_ref_still_resolves() {
    let env = Env::new();
    let s = submit(&request(&env, "op-legacy", "t_ok").to_string(), &env.cfg(0)).unwrap();
    assert_eq!(
        s.receipt.verification,
        axon_loop_contracts::ReceiptVerification::Passed
    );
    assert!(env
        .journal_text()
        .contains("\"legacy_single_file\":\"f.ax\""));
}

fn tenant() -> axon_loop_contracts::TenantId {
    axon_loop_contracts::TenantId::new("tenant-t").unwrap()
}

/// Where `tenant()`'s store lives under a state dir.
fn ws_root(state: &Path) -> PathBuf {
    use sha2::{Digest, Sha256};
    let key = format!("{:x}", Sha256::digest(tenant().as_str().as_bytes()));
    state.join("tenants").join(&key[..32]).join("workspaces")
}

// ── tenancy ─────────────────────────────────────────────────────────────────

#[test]
fn another_tenants_version_does_not_resolve_and_launches_nothing() {
    let env = Env::new();
    let cfg = env.cfg(0);
    let other = axon_loop_contracts::TenantId::new("tenant-other").unwrap();
    std::fs::write(env.ws.join("notes.md"), "x\n").unwrap();
    let r = WorkspaceStore::open(&cfg.state_dir, &other)
        .unwrap()
        .import_dir(&env.ws, &Quota::default())
        .unwrap();
    // Positive control: in its own tenant it resolves.
    assert!(WorkspaceStore::open(&cfg.state_dir, &other)
        .unwrap()
        .contains(&r));
    assert!(!WorkspaceStore::open(&cfg.state_dir, &tenant())
        .unwrap()
        .contains(&r));
    let mut req = request(&env, "op-xt", "t_ok");
    req["workspace_version_ref"] = r.as_str().into();
    let e = submit(&req.to_string(), &cfg).unwrap_err();
    assert_eq!(e.kind(), "conflict", "{e}");
    assert_eq!(spawn_count(&env.spawns), 0);
    assert_eq!(env.launch_records(), 0);
    // Caches are per tenant too, even for an equal TrialId.
    let t = TrialId::new("trial-1").unwrap();
    assert_ne!(
        TrialCache::for_trial(&cfg.state_dir, &other, &t).unwrap(),
        TrialCache::for_trial(&cfg.state_dir, &tenant(), &t).unwrap()
    );
}

// ── omissions are a set of observations (found by the paired G3 gate) ──────

/// The same version imported from a repository (`.git` omitted) and from a
/// plain copy (nothing omitted) is ONE version with two observations — not a
/// corrupt store. Before the fix the second publish failed "exists with
/// different bytes".
#[test]
fn the_same_version_imported_with_different_omissions_is_one_version() {
    let c = case();
    let store = WorkspaceStore::open(&c.state, &tenant()).unwrap();
    let plain = store.import_dir(&c.root, &Quota::default()).unwrap();
    std::fs::create_dir_all(c.root.join(".git")).unwrap();
    std::fs::write(c.root.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let with_git = store.import_dir(&c.root, &Quota::default()).unwrap();
    assert_eq!(plain, with_git, "omissions are not part of the reference");
    // Idempotent on a repeat of either observation.
    assert_eq!(store.import_dir(&c.root, &Quota::default()).unwrap(), plain);
    let v = store.load(&plain).unwrap();
    assert!(
        v.omissions.iter().any(|o| o.path == ".git"),
        "every recorded observation's omissions are kept: {:?}",
        v.omissions
    );
    // The content still round-trips.
    let dest = c.state.parent().unwrap().join("out");
    store.materialize(&plain, &dest, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dest.join("ok.txt")).unwrap(),
        "fine\n"
    );
}

/// A stored omission record whose bytes do not hash to its name is corrupt.
#[test]
fn a_tampered_omission_record_is_corrupt() {
    let c = case();
    let store = WorkspaceStore::open(&c.state, &tenant()).unwrap();
    let r = store.import_dir(&c.root, &Quota::default()).unwrap();
    let rec = walk_find(&c.state, ".omissions").expect("an omissions set directory");
    let file = std::fs::read_dir(&rec)
        .unwrap()
        .flatten()
        .next()
        .unwrap()
        .path();
    std::fs::write(&file, br#"[{"path":"x","reason":"forged"}]"#).unwrap();
    assert!(matches!(store.load(&r), Err(StoreError::Corrupt(_))));
}

fn walk_find(p: &Path, suffix: &str) -> Option<PathBuf> {
    for e in std::fs::read_dir(p).ok()?.flatten() {
        let q = e.path();
        if q.is_dir() {
            if q.to_string_lossy().ends_with(suffix) {
                return Some(q);
            }
            if let Some(f) = walk_find(&q, suffix) {
                return Some(f);
            }
        }
    }
    None
}

/// End to end: a check over a version imported from a REAL repository (with a
/// `.git` to omit) passes through submit — the post-run candidate re-check
/// re-imports a materialized copy with nothing to omit, and must not report
/// the store corrupt. Before the fix: receipt `not_run`, "candidate
/// unreadable after the run".
#[test]
fn a_check_over_a_version_from_a_real_repository_passes() {
    let env = Env::new();
    std::fs::create_dir_all(env.ws.join(".git")).unwrap();
    std::fs::write(env.ws.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    let r = publish_ws(&env);
    let mut req = request(&env, "op-repo", "t_ok");
    req["workspace_version_ref"] = r.as_str().into();
    let s = submit(&req.to_string(), &env.cfg(0)).unwrap();
    assert_eq!(
        s.receipt.verification,
        axon_loop_contracts::ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    assert_eq!(s.receipt.input_workspace_ref, r);
}

/// B282 / G03-r22-joint-bypass — the LEGACY single-file fallback reads the live
/// workspace in place. A file swapped after the request's digest was taken
/// must not yield a verdict that names the ORIGINAL bytes: either the verdict
/// is about the bytes the receipt names, or there is no verdict.
#[test]
fn a_legacy_single_file_swapped_before_launch_never_yields_a_verdict_on_the_original() {
    let env = Env::new();
    let named = request(&env, "op-legacy-swap", "t_ok");
    let mut cfg = env.cfg(0);
    cfg.pre_launch_hook = Some(swap_workspace_file);
    let s = submit(&named.to_string(), &cfg).unwrap();
    // The run judged the swapped file; the receipt names the original digest.
    // So there is NO verdict — neither passed nor failed — and the reason says why.
    assert_eq!(
        s.receipt.verification,
        axon_loop_contracts::ReceiptVerification::Unknown,
        "{:?}",
        s.reason
    );
    assert!(
        s.reason
            .as_deref()
            .is_some_and(|r| r.contains("the run changed the candidate")),
        "{:?}",
        s.reason
    );
}

/// The protected guest re-digests its read-only inputs with
/// `axon_workspace_recipe::tree_version_ref` (v022-psv-protocol.md §4). That
/// digest IS the store's reference for the same tree — the cross-language
/// vector included — and any byte, mode or link change moves it.
#[test]
fn the_guest_tree_digest_is_the_store_reference() {
    use axon_workspace_recipe::tree_version_ref;
    let v = vector();
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    materialize_vector(&root, &v);
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/HEAD"), "ref: x\n").unwrap();
    let q = Quota::default();
    let guest = tree_version_ref(&root, &q).unwrap();
    assert_eq!(guest, v["reference"].as_str().unwrap());
    assert_eq!(
        guest,
        WorkspaceTree::import_dir(&root, &q)
            .unwrap()
            .reference()
            .as_str()
    );

    // One byte.
    let f = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| std::fs::symlink_metadata(p).unwrap().is_file())
        .expect("a top-level file in the vector");
    let orig = std::fs::read(&f).unwrap();
    let mut changed = orig.clone();
    changed.push(b'!');
    std::fs::write(&f, &changed).unwrap();
    assert_ne!(tree_version_ref(&root, &q).unwrap(), guest, "content");
    std::fs::write(&f, &orig).unwrap();
    assert_eq!(tree_version_ref(&root, &q).unwrap(), guest);
    // One mode bit.
    let exec = std::fs::symlink_metadata(&f).unwrap().permissions().mode() & 0o111 != 0;
    set_exec(&f, !exec);
    assert_ne!(tree_version_ref(&root, &q).unwrap(), guest, "mode");
    set_exec(&f, exec);
    // A new link.
    std::os::unix::fs::symlink("x", root.join("new-link")).unwrap();
    assert_ne!(tree_version_ref(&root, &q).unwrap(), guest, "link");
}

// ── C9 round 4b, rows4b (amendment 62): the store never materializes bytes
// other than the version's. Two checks refuse each defect alone (the blob
// re-verified against its name, the tree re-derived to the reference; the
// manifest hashed to the reference), so these accept either refusal and fail
// only on the materialization (four-cell records). On the protected route the
// guest re-walks what it was given (M159) as a further layer.

#[test]
fn a_blob_holding_other_bytes_never_materializes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    materialize_vector(&root, &vector());
    let store = WorkspaceStore::open(&dir.path().join("state"), &tenant()).unwrap();
    let r = store.import_dir(&root, &Quota::default()).unwrap();
    let out = dir.path().join("out-control");
    store.materialize(&r, &out, false).expect("control");
    let blob = ws_root(&dir.path().join("state"))
        .join("blobs/bad18e717145fcf190f9144d635a3295ab55ffc8cddcfc94b6c4b94b00093b42");
    std::fs::write(&blob, "hello workspacE\n").unwrap();
    let out = dir.path().join("out");
    let m = store.materialize(&r, &out, false);
    assert!(
        matches!(m, Err(StoreError::Corrupt(_))),
        "ATTACK: a blob holding other bytes than its name was materialized as version {r}: {m:?}"
    );
    assert!(!out.exists(), "no partial materialization");
}

#[test]
fn a_manifest_that_is_not_its_versions_never_materializes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    materialize_vector(&root, &vector());
    let store = WorkspaceStore::open(&dir.path().join("state"), &tenant()).unwrap();
    let r = store.import_dir(&root, &Quota::default()).unwrap();
    let versions = ws_root(&dir.path().join("state")).join("versions");
    let manifest = std::fs::read_dir(&versions)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|x| x == "manifest"))
        .unwrap();
    let text = std::fs::read_to_string(&manifest).unwrap();
    assert!(text.contains("README.md"), "{text}");
    // Another name for the same blob: a well-formed manifest of ANOTHER tree.
    std::fs::write(&manifest, text.replace("README.md", "READMX.md")).unwrap();
    let out = dir.path().join("out");
    let m = store.materialize(&r, &out, false);
    assert!(
        matches!(m, Err(StoreError::Corrupt(_))),
        "ATTACK: a manifest of another tree was materialized as version {r}: {m:?}"
    );
    assert!(!out.exists(), "no partial materialization");
}

/// C9 round 4b, rows4b (M1090): a stored version whose manifest names ONE
/// path twice, with two contents, is never materialized; the reference would
/// cover both while the run reads one. The manifest hashes to its own name
/// (so load() accepts it) and both blobs are genuine: only the duplicate rule
/// of the tree's re-validation refuses it. Control: each content alone
/// materializes.
#[test]
fn a_version_naming_one_path_twice_never_materializes() {
    use axon_cortex::runner::{workspace_manifest_bytes, workspace_version_ref};
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let store = WorkspaceStore::open(&state, &tenant()).unwrap();
    let mut entries = vec![];
    for (i, body) in ["fn a() {}\n", "fn b() {}\n"].iter().enumerate() {
        let root = dir.path().join(format!("t{i}"));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("f.ax"), body).unwrap();
        let r = store.import_dir(&root, &Quota::default()).unwrap();
        store
            .materialize(&r, &dir.path().join(format!("ok{i}")), false)
            .expect("control");
        entries.extend(store.load(&r).unwrap().entries);
    }
    assert_eq!(entries.len(), 2);
    let m = workspace_manifest_bytes(&entries);
    let r = Acf1Ref::new(workspace_version_ref(&m)).unwrap();
    let hex = r.as_str().strip_prefix("acf1:").unwrap();
    let versions = ws_root(&state).join("versions");
    std::fs::write(versions.join(format!("{hex}.manifest")), &m).unwrap();
    let om = versions.join(format!("{hex}.omissions"));
    std::fs::create_dir_all(&om).unwrap();
    let none = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(b"[]"))
    };
    std::fs::write(om.join(format!("{none}.json")), "[]").unwrap();
    assert!(
        store.load(&r).is_ok(),
        "setup: the manifest hashes to its name"
    );
    let out = dir.path().join("out");
    let got = store.materialize(&r, &out, false);
    assert!(
        got.is_err(),
        "ATTACK: a version naming two contents for one path was materialized ({:?})",
        std::fs::read_to_string(out.join("f.ax"))
    );
}

/// C9 round 4b, rows4b (M1091, retired EQUIVALENT_DID against M1081 and
/// M1082): a blob planted under another content's name BEFORE publication
/// is never what a published version materializes. Two checks refuse it,
/// each alone: publication's comparison with the existing file, and the
/// re-verification of every blob when the tree is read.
#[test]
fn a_blob_planted_before_publication_never_materializes() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let store = WorkspaceStore::open(&state, &tenant()).unwrap();
    let real = "fn judged() { 1 }\n";
    let planted = "fn other() { 2 }\n";
    let blobs = ws_root(&state).join("blobs");
    std::fs::create_dir_all(&blobs).unwrap();
    let sha = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(real.as_bytes()))
    };
    std::fs::write(blobs.join(&sha), planted).unwrap();
    let root = dir.path().join("tree");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("f.ax"), real).unwrap();
    let out = dir.path().join("out");
    let got = store
        .import_dir(&root, &Quota::default())
        .ok()
        .and_then(|r| store.materialize(&r, &out, false).ok())
        .and_then(|()| std::fs::read_to_string(out.join("f.ax")).ok());
    assert_ne!(
        got.as_deref(),
        Some(planted),
        "ATTACK: a blob planted under another content's name before publication was materialized"
    );
}

/// C9 round 4c, ADMIT (amendment 76): the receipt is the verdict, so a verdict
/// over bytes the run did not judge is a false verdict. The same attack as
/// `a_legacy_single_file_swapped_before_launch_never_yields_a_verdict_on_the_original`
/// (the file is swapped under the run), judged on what the RECEIPT claims: not
/// passed and not failed, whichever way the swapped file's own test went.
#[test]
fn a_verdict_over_bytes_the_run_did_not_judge_is_never_receipted() {
    use axon_loop_contracts::ReceiptVerification as V;
    let env = Env::new();
    let mut cfg = env.cfg(0);
    cfg.pre_launch_hook = Some(swap_workspace_file);
    let s = submit(&request(&env, "op-swap-v", "t_ok").to_string(), &cfg).unwrap();
    if matches!(s.receipt.verification, V::Passed | V::Failed) {
        panic!(
            "ATTACK: a {:?} verdict was receipted over bytes the run did not judge ({:?})",
            s.receipt.verification, s.reason
        );
    }
    assert_eq!(s.receipt.verification, V::Unknown, "{:?}", s.reason);
}

// ── C9 round 6, EQGATE2 (amendment 87): permission modes of the store ────────

/// A materialized tree carries the modes the version records (files 0644 /
/// 0755, read-only 0444 / 0555, directories 0555 when read-only), a trial's
/// cache root is 0700, and a read-only tree can still be removed. Each is a
/// `set_permissions` / `set_mode` call that builds no `Err`; each was
/// weakenable alone with the whole suite green (round 6: the cache root 0700
/// -> 0755 left 743 tests passing).
#[test]
fn the_materialized_modes_and_the_trial_cache_root_are_what_the_store_says() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("tree");
    materialize_vector(&root, &vector());
    let store = WorkspaceStore::open(&dir.path().join("state"), &tenant()).unwrap();
    let r = store.import_dir(&root, &Quota::default()).unwrap();
    use std::os::unix::fs::MetadataExt as _;
    let mode = |p: &Path| std::fs::metadata(p).unwrap().mode() & 0o777;
    let rw = dir.path().join("rw");
    store.materialize(&r, &rw, false).unwrap();
    assert_eq!(
        mode(&rw.join("README.md")),
        0o644,
        "ATTACK: a plain file of a writable materialization is not 0644"
    );
    assert_eq!(
        mode(&rw.join("bin/run.sh")),
        0o755,
        "ATTACK: an executable file of a writable materialization is not 0755"
    );
    let ro = dir.path().join("ro");
    store.materialize(&r, &ro, true).unwrap();
    assert_eq!(
        mode(&ro.join("README.md")),
        0o444,
        "ATTACK: a plain file of a read-only materialization is writable"
    );
    assert_eq!(
        mode(&ro.join("bin/run.sh")),
        0o555,
        "ATTACK: an executable file of a read-only materialization is writable"
    );
    assert_eq!(
        mode(&ro.join("src")),
        0o555,
        "ATTACK: a directory of a read-only materialization is writable"
    );
    let removed = axon_fabric::workspace::remove_tree(&ro);
    assert!(
        removed.is_ok() && !ro.exists(),
        "ATTACK: a read-only materialized tree cannot be removed (its directories were not \
         unlocked): {removed:?}"
    );
    let cache = TrialCache::for_trial(dir.path(), &tenant(), &TrialId::new("trial-modes").unwrap())
        .unwrap();
    assert_eq!(
        mode(&cache.root),
        0o700,
        "ATTACK: a trial's cache root is not 0700: another uid can read or plant in it"
    );
}
