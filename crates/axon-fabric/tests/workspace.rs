//! B261 — WorkspaceVersion: the cross-language identity vector, one negative
//! per import-refusal class (each asserting the store is left EMPTY), the
//! write-once content-addressed store, G28 (a hash-only observation is never
//! materialized), per-trial caches, and `check_target` accepting a manifest
//! ref through the real submit path.

mod common;
use common::*;

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
    let store = WorkspaceStore::open(&c.state).unwrap();
    let err = store.import_dir(&c.root, quota).unwrap_err();
    let StoreError::Refused(r) = err else {
        panic!("expected a refusal, got {err}")
    };
    assert_eq!(r.class(), class, "{r}");
    assert_eq!(
        files_under(&c.state.join("workspaces")),
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
    WorkspaceStore::open(&c.state)
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
    WorkspaceStore::open(&c.state)
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
    let store = WorkspaceStore::open(&dir.path().join("state")).unwrap();
    let r = store.import_dir(&root, &Quota::default()).unwrap();
    assert_eq!(r.as_str(), vector()["reference"].as_str().unwrap());
    let mpath = dir
        .path()
        .join("state/workspaces/versions")
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
    let store = WorkspaceStore::open(&dir.path().join("state")).unwrap();
    let r = store.import_dir(&root, &Quota::default()).unwrap();
    // README.md's blob.
    let blob = dir.path().join(
        "state/workspaces/blobs/bad18e717145fcf190f9144d635a3295ab55ffc8cddcfc94b6c4b94b00093b42",
    );
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
    let store = WorkspaceStore::open(&dir.path().join("state")).unwrap();
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
    let a = TrialCache::for_trial(dir.path(), &TrialId::new("trial-a").unwrap()).unwrap();
    let b = TrialCache::for_trial(dir.path(), &TrialId::new("trial-b").unwrap()).unwrap();
    assert_ne!(a.root, b.root);
    for ((ka, va), (kb, vb)) in a.env().iter().zip(b.env().iter()) {
        assert_eq!(ka, kb);
        assert_ne!(va, vb, "{ka} is per-trial");
        std::fs::write(Path::new(va).join("marker"), "a").unwrap();
        assert!(!Path::new(vb).join("marker").exists());
    }
    // Keyed by TrialId: the same trial gets the same directories back.
    let a2 = TrialCache::for_trial(dir.path(), &TrialId::new("trial-a").unwrap()).unwrap();
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
    WorkspaceStore::open(&cfg.state_dir)
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
    assert!(WorkspaceStore::open(&cfg.state_dir)
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
