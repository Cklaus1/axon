//! NH-R1 §5.1 (A2): `cortex grant --envelope-preview`, the minter's source
//! for every value a repair envelope pins. Each value must be what the
//! unattended runner recomputes at verification (`auth::phase_a`/`phase_b`),
//! so the test recomputes them through those library functions and compares.
#![cfg(feature = "unattended")]

mod common;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn axon_bin() -> PathBuf {
    script_spawn::workspace_bin(
        "AXON_BIN",
        &[
            "build",
            "-p",
            "axon-core",
            "--no-default-features",
            "--bin",
            "axon",
        ],
        "axon",
    )
}

/// `<tmp>/<name>/src/broken.ax`, canonical.
fn workspace(name: &str) -> PathBuf {
    let ws = std::env::temp_dir().join(format!("cortex_preview_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(ws.join("src")).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/broken.ax");
    std::fs::copy(src, ws.join("src/broken.ax")).unwrap();
    std::fs::canonicalize(&ws).unwrap()
}

fn preview(ws: &Path, extra: &[&str]) -> (i32, String, String) {
    let axon = axon_bin();
    let out = common::cortex()
        .args(["grant", "--envelope-preview", "--workspace"])
        .arg(ws)
        .args(["--file", "src/broken.ax", "--symbol", "double"])
        .args(["--check", "hidden_completion", "--write-prefix", "src"])
        .arg("--axon")
        .arg(&axon)
        .args(extra)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The preview's values are exactly the runner's: the pinned fence and its
/// sha256, the gate rows, the target grant, the import closure and the
/// check digest over the `--axon` bytes.
#[test]
fn the_preview_pins_what_the_runner_recomputes() {
    let ws = workspace("same");
    let (code, out, err) = preview(&ws, &["--generator", "ai:claude-x"]);
    assert_eq!(
        code, 0,
        "ATTACK: the envelope preview was not produced: {err}"
    );
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    assert_eq!(v["schema"], "cortex-envelope-preview/1", "{v}");

    let axon = axon_bin();
    let wall = Duration::from_secs(60);
    let file = ws.join("src/broken.ax");
    let caps = axon_cortex::fence::caps_of_file(&axon, &file, None, wall).unwrap();
    let fence = axon_cortex::fence::Fence::from_pristine(&caps);
    let pins = axon_cortex::fence::ScopePins {
        fs_read: vec![],
        fs_write: vec![],
        net: vec![],
    };
    let (rows, pinned) = axon_cortex::auth::gate_rows(&fence, &caps, &pins, "ai");
    let pinned = pinned.expect("a pure file's E1 row admits");
    let sha = pinned.sha256();
    let imports = axon_cortex::auth::imports_digest(&axon, &ws, "src/broken.ax", wall).unwrap();
    let checker = axon_cortex::label::sha256_hex(&std::fs::read(&axon).unwrap());
    let digest =
        axon_cortex::auth::check_digest("hidden_completion", &checker, None, &sha, &imports);
    let grant =
        axon_cortex::auth::target_grant(&axon, &ws, "src/broken.ax", "double", wall).unwrap();
    let (provider, host) = axon_cortex::auth::resolved_generator();
    let pristine = axon_cortex::label::sha256_hex(&std::fs::read(&file).unwrap());

    assert_eq!(
        v["fence_sha256"], sha,
        "ATTACK: the preview's fence is not the runner's pinned fence"
    );
    assert_eq!(v["fence"], pinned.to_json(&[]), "{v}");
    assert_eq!(
        v["check_digest"], digest,
        "ATTACK: the preview's check digest is not the runner's"
    );
    assert_eq!(
        v["check_digest_input"],
        axon_cortex::auth::check_digest_input("hidden_completion", &checker, None, &sha, &imports),
        "{v}"
    );
    assert_eq!(v["imports_digest"], imports, "{v}");
    assert_eq!(
        v["target_grant"], grant,
        "ATTACK: the preview's target grant is not the runner's"
    );
    let want_rows: Vec<Value> = rows.iter().map(|r| r.to_json()).collect();
    assert_eq!(v["gate"], Value::Array(want_rows), "{v}");
    assert!(
        v["gate"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "admit"),
        "{v}"
    );
    assert_eq!(
        v["task"],
        serde_json::json!({
            "workspace": ws.to_str().unwrap(),
            "file": "src/broken.ax",
            "symbol": "double",
            "pristine_sha256": pristine,
        })
    );
    assert_eq!(v["check_name"], "hidden_completion");
    assert_eq!(v["write_prefixes"], serde_json::json!(["src"]));
    assert_eq!(
        v["scopes"],
        serde_json::json!({"fs_read": [], "fs_write": [], "net": []})
    );
    assert_eq!(
        v["generator"],
        serde_json::json!({
            "kind": "ai", "model": "claude-x", "provider": provider, "endpoint_host": host,
        })
    );
    let _ = std::fs::remove_dir_all(&ws);
}

/// A file that USES a scope kind: the preview's fence is the scope-pinned
/// one phase B compares (`gate_rows`' E1 fence), never the pristine fence.
#[test]
fn a_scope_using_file_previews_the_pinned_fence() {
    let ws = workspace("pinned");
    std::fs::write(
        ws.join("src/broken.ax"),
        "fn peek() -> i64 {\n    match read_file(\"/x\") {\n        Ok(_) => 1,\n        Err(_) => 0,\n    }\n}\n\n\
         fn double(n: i64) -> i64 {\n    n + 2\n}\n\n\
         @[test]\nfn hidden_completion() {\n    assert_eq(double(7), 14)\n}\n\n\
         fn main() {\n    println(to_str(double(1)))\n}\n",
    )
    .unwrap();
    let axon = axon_bin();
    let wall = Duration::from_secs(60);
    let caps =
        axon_cortex::fence::caps_of_file(&axon, &ws.join("src/broken.ax"), None, wall).unwrap();
    let used = |k: &str| caps.capabilities.iter().any(|c| c == k);
    assert!(used("fs:read"), "setup: dstore reads a store: {caps:?}");
    let scope = std::env::temp_dir().join("axon-preview-scope");
    let scope = scope.to_str().unwrap().to_string();
    let pick = |k: &str| if used(k) { vec![scope.clone()] } else { vec![] };
    let pins = axon_cortex::fence::ScopePins {
        fs_read: pick("fs:read"),
        fs_write: pick("fs:write"),
        net: if used("net") {
            vec!["api.example.com".into()]
        } else {
            vec![]
        },
    };
    let mut extra = vec!["--generator".to_string(), "ai:m".to_string()];
    for (flag, vals) in [
        ("--read-prefix", &pins.fs_read),
        ("--fs-write-scope", &pins.fs_write),
        ("--net-host", &pins.net),
    ] {
        for v in vals {
            extra.push(flag.into());
            extra.push(v.clone());
        }
    }
    let extra: Vec<&str> = extra.iter().map(String::as_str).collect();
    let (code, out, err) = preview(&ws, &extra);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    let pristine = axon_cortex::fence::Fence::from_pristine(&caps);
    let (_, pinned) = axon_cortex::auth::gate_rows(&pristine, &caps, &pins, "ai");
    let pinned = pinned.expect("every used kind is pinned");
    assert_ne!(
        pinned.sha256(),
        pristine.sha256(),
        "setup: a pin changes the fence"
    );
    assert_eq!(
        v["fence_sha256"],
        pinned.sha256(),
        "ATTACK: the preview's fence is not the runner's pinned fence"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

/// A generator outside the v1 admission set (E9) is a refused gate row the
/// minter reads, never an admitted one.
#[test]
fn a_non_ai_generator_is_a_refused_gate_row() {
    let ws = workspace("e9");
    let (code, out, err) = preview(&ws, &["--generator", "cmd:./gen.sh"]);
    assert_eq!(code, 0, "{err}");
    let v: Value = serde_json::from_str(out.trim()).unwrap();
    let e9 = v["gate"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["id"] == "E9")
        .cloned()
        .unwrap();
    assert_eq!(
        e9["status"], "refuse",
        "ATTACK: a cmd: generator was admitted by the preview's gate: {v}"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

/// Phase A's symlink walk runs in the preview too: a symlinked task file
/// or a symlink under a write prefix produces no preview at all.
#[test]
fn a_symlink_on_the_task_or_a_write_prefix_produces_no_preview() {
    let ws = workspace("link");
    std::os::unix::fs::symlink("/etc", ws.join("src/etc")).unwrap();
    let (code, out, err) = preview(&ws, &["--generator", "ai:m"]);
    assert_eq!(
        code, 2,
        "ATTACK: a symlink under a write prefix was previewed: {out}"
    );
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("symlink"), "{err}");
    let _ = std::fs::remove_dir_all(&ws);

    let ws = workspace("linkfile");
    std::fs::rename(ws.join("src/broken.ax"), ws.join("real.ax")).unwrap();
    std::os::unix::fs::symlink(ws.join("real.ax"), ws.join("src/broken.ax")).unwrap();
    let (code, out, err) = preview(&ws, &["--generator", "ai:m"]);
    assert_eq!(
        code, 2,
        "ATTACK: a symlinked task file was previewed: {out}"
    );
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("symlink"), "{err}");
    let _ = std::fs::remove_dir_all(&ws);

    // The task file alone is the symlink, outside every write prefix: only
    // the task-file walk sees it.
    let ws = workspace("linkonly");
    std::fs::rename(ws.join("src/broken.ax"), ws.join("real.ax")).unwrap();
    std::os::unix::fs::symlink(ws.join("real.ax"), ws.join("src/broken.ax")).unwrap();
    let out = common::cortex()
        .args(["grant", "--envelope-preview", "--workspace"])
        .arg(&ws)
        .args(["--file", "src/broken.ax", "--symbol", "double"])
        .args(["--check", "hidden_completion", "--write-prefix", "gen"])
        .args(["--generator", "ai:m", "--axon"])
        .arg(axon_bin())
        .output()
        .unwrap();
    let so = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(2),
        "ATTACK: a symlinked task file was previewed: {so}"
    );
    assert!(so.is_empty(), "{so}");
    let _ = std::fs::remove_dir_all(&ws);
}

/// A check `fs_write` pin over the task file (§5.4) produces no preview.
#[test]
fn an_fs_write_pin_over_the_task_file_produces_no_preview() {
    let ws = workspace("fswrite");
    let pin = ws.join("src").to_str().unwrap().to_string();
    let (code, out, err) = preview(&ws, &["--generator", "ai:m", "--fs-write-scope", &pin]);
    assert_eq!(
        code, 2,
        "ATTACK: an fs_write pin over the task file was previewed: {out}"
    );
    assert!(out.is_empty(), "{out}");
    assert!(err.contains("covers the task file"), "{err}");
    let _ = std::fs::remove_dir_all(&ws);
}

/// Security review F4: a relative `--read-prefix` / `--fs-write-scope` pin
/// produces no preview. It would resolve against the check child's cwd (the
/// workspace) and skip the symlink walk: here `out` links to `/etc`.
#[test]
fn a_relative_fs_pin_produces_no_preview() {
    for flag in ["--read-prefix", "--fs-write-scope"] {
        let ws = workspace(&format!("relpin{flag}"));
        std::fs::write(
            ws.join("src/broken.ax"),
            "fn touch() -> i64 {\n    let _ = write_file(\"/x\", \"y\")\n    match read_file(\"/x\") {\n        \
             Ok(_) => 1,\n        Err(_) => 0,\n    }\n}\n\n\
             fn double(n: i64) -> i64 {\n    n + 2\n}\n\n\
             @[test]\nfn hidden_completion() {\n    assert_eq(double(7), 14)\n}\n",
        )
        .unwrap();
        std::os::unix::fs::symlink("/etc", ws.join("out")).unwrap();
        let other = if flag == "--read-prefix" {
            "--fs-write-scope"
        } else {
            "--read-prefix"
        };
        let (code, out, err) = preview(
            &ws,
            &[
                "--generator",
                "ai:m",
                flag,
                "out",
                other,
                "/var/empty-axon-scope",
            ],
        );
        assert_eq!(
            code, 2,
            "ATTACK: NH-R1 security F4 a relative {flag} pin was previewed: {out}"
        );
        assert!(out.is_empty(), "{out}");
        assert!(err.contains("is relative: a pin must be absolute"), "{err}");
        let _ = std::fs::remove_dir_all(&ws);
    }
}

/// Security review F7: a bare `--axon axon` is a PATH lookup for every child
/// the preview runs, so the checker sha256 it pins is that file's - not the
/// bytes of an `axon` that happens to sit in the current directory.
#[test]
fn a_bare_axon_pins_the_binary_path_runs() {
    let ws = workspace("bare_axon");
    let cwd = ws.with_extension("cwd");
    let bin = ws.with_extension("bin");
    let _ = std::fs::remove_dir_all(&cwd);
    let _ = std::fs::remove_dir_all(&bin);
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(cwd.join("axon"), b"a decoy that never runs\n").unwrap();
    std::os::unix::fs::symlink(axon_bin(), bin.join("axon")).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = common::cortex()
        .args(["grant", "--envelope-preview", "--workspace"])
        .arg(&ws)
        .args(["--file", "src/broken.ax", "--symbol", "double"])
        .args(["--check", "hidden_completion", "--write-prefix", "src"])
        .args(["--axon", "axon", "--generator", "ai:m"])
        .env("PATH", &path)
        .current_dir(&cwd)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_str(stdout.trim()).unwrap();
    let genuine = axon_cortex::label::sha256_hex(&std::fs::read(axon_bin()).unwrap());
    assert_eq!(
        v["check_digest_input"]["checker_exe_sha256"].as_str(),
        Some(genuine.as_str()),
        "ATTACK: NH-R1 security F7 the preview pinned an interpreter other than the one PATH runs"
    );
    for d in [&ws, &cwd, &bin] {
        let _ = std::fs::remove_dir_all(d);
    }
}

/// E7: the preview pins the local interpreter only; a check registry is a
/// usage error. The preview's inputs are refused without the preview flag,
/// so plain `cortex grant` keeps its exact surface.
#[test]
fn preview_inputs_need_the_preview_and_refuse_a_check_registry() {
    let ws = workspace("flags");
    let (code, out, _) = preview(&ws, &["--generator", "ai:m", "--check-registry", "r.json"]);
    assert_eq!(
        code, 2,
        "ATTACK: a check registry was accepted with --envelope-preview: {out}"
    );
    let out = common::cortex()
        .args(["grant", "--workspace"])
        .arg(&ws)
        .args(["--file", "src/broken.ax", "--check", "hidden_completion"])
        .arg("--axon")
        .arg(axon_bin())
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.code(),
        Some(2),
        "ATTACK: a preview input was accepted by plain grant: {err}"
    );
    assert!(
        err.contains("only accepted with --envelope-preview"),
        "{err}"
    );
    let _ = std::fs::remove_dir_all(&ws);
}
