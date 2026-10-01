//! Every check that execs a built binary runs the binary built from the tree
//! under test, never a stale one (C9 round 4, EQUIVALENCE (6)).
//!
//! Round 4 executed both halves of the hole: `clock_parity.sh` ran a planted
//! `$REPO/target/debug/axon` and reported on it, and the R17 IR/QEMU gates ran
//! a planted `axon` from PATH and PASSED. The drift test that was meant to
//! prevent it counted literal `Command::new("bash")` in ONE file.
//!
//! The rule is enforced at two primitives, and these tests hold them:
//! * a SCRIPT runs the binary its caller names or the one it built itself
//!   (`scripts/lib/axon_bin.sh`: `named_bin` refuses with none named,
//!   `use_built` resolves the path cargo built to);
//! * a TEST runs a script only through `tests/script_spawn`, which strips every
//!   ambient binary-naming variable and refuses a script that guesses.
//!
//! Each attack runs the PRODUCTION script (a copy of it, in a scratch tree
//! where a stale binary is planted), through the production spawn helper.

mod script_spawn;
use script_spawn::{
    binary_choice_violations, binary_resolution_violations, caller_binary_vars, repo_root,
    spawn_violations, Bins, BINARY_VARS,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

fn scratch(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "axon-harness-binaries-{tag}-{}-{}",
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

/// An executable that records it ran by creating `marker`, then fails.
fn planted(p: &Path, marker: &Path) {
    write(
        p,
        &format!("#!/bin/sh\necho ran > '{}'\nexit 3\n", marker.display()),
    );
    let st = std::process::Command::new("chmod")
        .arg("0755")
        .arg(p)
        .status()
        .unwrap();
    assert!(st.success());
}

/// A copy of the named repository files under `root`, at the same paths.
fn copy_from_repo(root: &Path, rels: &[&str]) {
    for rel in rels {
        let text = std::fs::read_to_string(repo_root().join(rel)).unwrap();
        write(&root.join(rel), &text);
    }
}

fn walk(dir: &Path, keep: &dyn Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name == "target" || name == ".git" || name == "node_modules" || name == "dist" {
            continue;
        }
        if p.is_dir() {
            walk(&p, keep, out);
        } else if keep(&p) {
            out.push(p);
        }
    }
}

// ── a script never runs a binary its caller did not name ───────────────────

/// A harness that builds nothing (`replay_host_gate.sh`), run with NO binary
/// named, in a tree where a stale `target/debug/axon` sits and an `axon` is on
/// PATH. Neither may run: the harness refuses. Control: the binary the caller
/// names is exactly the one that runs.
#[test]
fn a_harness_that_builds_nothing_runs_no_binary_its_caller_did_not_name() {
    let root = scratch("named");
    copy_from_repo(
        &root,
        &["scripts/replay_host_gate.sh", "scripts/lib/axon_bin.sh"],
    );
    let stale = root.join("stale-ran");
    let on_path = root.join("path-ran");
    planted(&root.join("target").join("debug").join("axon"), &stale);
    planted(&root.join("fakebin/axon"), &on_path);
    let path = format!(
        "{}:{}",
        root.join("fakebin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = script_spawn::script(
        "bash",
        root.join("scripts/replay_host_gate.sh"),
        Bins::Named(&[]),
    )
    .env("PATH", &path)
    .output()
    .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !stale.exists() && !on_path.exists(),
        "ATTACK: a harness ran a binary its caller did not name (stale target/: {}, PATH: {}):\n{text}",
        stale.exists(),
        on_path.exists()
    );
    assert!(
        !out.status.success() && text.contains("REFUSED"),
        "a harness with no binary named must refuse, not skip or pass (exit {:?}):\n{text}",
        out.status.code()
    );
    // Control: the named binary is the one that runs.
    let named_ran = root.join("named-ran");
    let named = root.join("named/axon");
    planted(&named, &named_ran);
    let _ = script_spawn::script(
        "bash",
        root.join("scripts/replay_host_gate.sh"),
        Bins::Named(&[("AXON", named.to_str().unwrap())]),
    )
    .env("PATH", &path)
    .output()
    .unwrap();
    assert!(
        named_ran.exists() && !stale.exists() && !on_path.exists(),
        "control: the harness runs exactly the binary its caller names"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A harness that builds its own binary (`clock_parity.sh`) runs the file
/// CARGO built, wherever cargo put it: here a config sends the build to
/// `tgt/`, and a stale `target/debug/axon` sits where the old harness looked.
#[test]
fn a_building_harness_runs_the_binary_cargo_built_not_one_left_in_target() {
    let root = scratch("built");
    copy_from_repo(
        &root,
        &["scripts/clock_parity.sh", "scripts/lib/axon_bin.sh"],
    );
    let built_ran = root.join("built-ran");
    let stale_ran = root.join("stale-ran");
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"axon-core\"]\nresolver = \"2\"\n",
    );
    write(
        &root.join("axon-core/Cargo.toml"),
        "[package]\nname = \"axon-core\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [[bin]]\nname = \"axon\"\npath = \"main.rs\"\n",
    );
    write(
        &root.join("axon-core/main.rs"),
        &format!(
            "fn main() {{ std::fs::write({:?}, \"ran\").unwrap(); std::process::exit(3) }}\n",
            built_ran.display().to_string()
        ),
    );
    write(
        &root.join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"tgt\"\n",
    );
    planted(&root.join("target").join("debug").join("axon"), &stale_ran);
    let out = script_spawn::script(
        "bash",
        root.join("scripts/clock_parity.sh"),
        Bins::BuildsItsOwn,
    )
    .env("CARGO_NET_OFFLINE", "true")
    .output()
    .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        !stale_ran.exists(),
        "ATTACK: a building harness ran a binary cargo did not build (a stale target/debug/axon):\n{text}"
    );
    assert!(
        built_ran.exists(),
        "control: the harness runs the binary its own build produced (in tgt/):\n{text}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A binary-naming variable in the environment that launched `cargo test`
/// (a developer's `AXON`, a harness's `AXON_BIN`) never reaches a script:
/// only what the test itself names does.
#[test]
fn an_ambient_binary_variable_never_reaches_a_script() {
    let root = scratch("ambient");
    let probe = root.join("scripts/probe.sh");
    let mut body = String::from("#!/bin/sh\n");
    for v in BINARY_VARS {
        body.push_str(&format!("echo \"{v}=${{{v}:-unset}}\"\n"));
    }
    write(&probe, &body);
    let planted_path = root.join("planted-bin");
    for v in BINARY_VARS {
        // Only this test sets these, and every spawn in this binary goes
        // through the helper, which removes them again.
        std::env::set_var(v, &planted_path);
    }
    let out = script_spawn::script("sh", &probe, Bins::BuildsItsOwn)
        .output()
        .unwrap();
    let named = script_spawn::script("sh", &probe, Bins::Named(&[("AXON", "/the/named/axon")]))
        .output()
        .unwrap();
    for v in BINARY_VARS {
        std::env::remove_var(v);
    }
    let got = String::from_utf8_lossy(&out.stdout).to_string();
    let leaked: Vec<&&str> = BINARY_VARS
        .iter()
        .filter(|v| !got.contains(&format!("{v}=unset")))
        .collect();
    assert!(
        leaked.is_empty(),
        "ATTACK: an ambient binary-naming variable reached a script: {leaked:?}\n{got}"
    );
    let n = String::from_utf8_lossy(&named.stdout).to_string();
    assert!(
        n.contains("AXON=/the/named/axon") && n.contains("AXON_BIN=unset"),
        "control: exactly what the test names reaches the script:\n{n}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

// ── the drift gates: no route around the two primitives ────────────────────

/// Every script under the repository picks its binaries only through
/// `scripts/lib/axon_bin.sh` (or a target dir it chose and built into).
#[test]
fn no_script_picks_a_binary_it_neither_built_nor_was_given() {
    // The scanner itself: each of these guesses must be caught...
    let attacks = [
        concat!(
            "AXON=\"${AXON:-./target/",
            "debug/axon}\"\n\"$AXON\" run x.ax\n"
        ),
        concat!("for c in \"$REPO/target/", "release/axon\"; do :; done\n"),
        concat!("AXON_BIN=\"$(command -v ", "axon)\"\n"),
        concat!(
            "BIN=\"${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-musl/",
            "release/axon\"\n"
        ),
        concat!(
            "AXON = Path(__file__).parent / \"target",
            "\" / \"debug\" / \"axon\"\n"
        ),
    ];
    for a in attacks {
        assert!(
            !binary_choice_violations(a).is_empty(),
            "ATTACK: a script that guesses its binary went unflagged:\n{a}"
        );
    }
    // ...and none of these honest forms may be.
    let honest = [
        ". scripts/lib/axon_bin.sh\nuse_built AXON axon\n\"$AXON\" run x.ax\n",
        concat!(
            "CARGO_TARGET_DIR=\"$WORK/t\" cargo build -q -p axon-core --bin axon\n",
            "AXON=\"$WORK/t/",
            "debug/axon\"\n"
        ),
        concat!("echo \"build it: target/", "debug/axon is stale\"\n"),
        concat!("# old: AXON=target/", "debug/axon\n"),
    ];
    for h in honest {
        let v = binary_choice_violations(h);
        assert!(
            v.is_empty(),
            "control: an honest form was flagged: {v:?}\n{h}"
        );
    }

    let root = repo_root();
    let mut files = vec![];
    let is_script = |p: &Path| {
        matches!(
            p.extension().and_then(|e| e.to_str()),
            Some("sh") | Some("py") | Some("bash")
        )
    };
    walk(&root.join("scripts"), &is_script, &mut files);
    walk(&root.join("examples"), &is_script, &mut files);
    walk(&root.join("profiles"), &is_script, &mut files);
    for e in std::fs::read_dir(&root).unwrap().flatten() {
        if e.path().is_file() && is_script(&e.path()) {
            files.push(e.path());
        }
    }
    assert!(files.len() > 100, "found only {} scripts", files.len());
    let mut bad = vec![];
    for f in &files {
        let Ok(text) = std::fs::read_to_string(f) else {
            continue;
        };
        for v in binary_choice_violations(&text) {
            bad.push(format!(
                "{}: {v}",
                f.strip_prefix(&root).unwrap_or(f).display()
            ));
        }
        for v in caller_binary_vars(&text) {
            if !BINARY_VARS.contains(&v.as_str()) {
                bad.push(format!(
                    "{}: accepts a binary from its caller in {v}, which the spawn helper does \
                     not strip (add it to script_spawn::BINARY_VARS)",
                    f.strip_prefix(&root).unwrap_or(f).display()
                ));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "scripts that pick a binary their caller did not name and they did not build:\n  {}",
        bad.join("\n  ")
    );
}

/// Every test, in EVERY crate, runs a repository script only through
/// `tests/script_spawn` — whether through bash, sh, python3 or by executing
/// the file directly.
#[test]
fn every_script_spawn_in_the_workspace_goes_through_the_helper() {
    // The scanner itself: each of these must be caught... (spelled in pieces
    // so this file's own scan does not read its fixtures as spawns)
    let attacks = [
        concat!(
            "let out = Command::",
            "new(\"bash\")\n    .arg(&script)\n    .output();\n"
        ),
        concat!(
            "let s = PathBuf::from(\"../../scri",
            "pts/x.py\");\n",
            "let o = Command::",
            "new(\"python3\").arg(&s).output();\n"
        ),
        concat!(
            "let launcher = Path::new(env!(\"CARGO_MANIFEST_DIR\")).join(\"../../scri",
            "pts/fc.sh\");\n",
            "let o = Command::",
            "new(&launcher).output();\n"
        ),
        concat!(
            "let o = Command::",
            "new(\"/usr/bin/sh\").arg(root.join(\"scri",
            "pts/a.sh\")).status();\n"
        ),
        concat!(
            "let o = Command::",
            "new(concat!(\"ba\", \"sh\")).arg(&p).status();\n"
        ),
    ];
    for a in attacks {
        assert!(
            !spawn_violations(a).is_empty(),
            "ATTACK: a script spawn outside the helper went unflagged:\n{a}"
        );
    }
    let honest = [
        concat!(
            "let o = Command::",
            "new(\"sh\").arg(\"-c\").arg(&block).output();\n"
        ),
        concat!(
            "let o = Command::",
            "new(\"python3\").args([\"-c\", \"print(1)\"]).output();\n"
        ),
        "let o = script_spawn::script(\"bash\", &script, Bins::BuildsItsOwn).output();\n",
        concat!(
            "let o = Command::",
            "new(env!(\"CARGO_BIN_EXE_axon\")).arg(\"run\").output();\n"
        ),
    ];
    for h in honest {
        let v = spawn_violations(h);
        assert!(
            v.is_empty(),
            "control: an honest spawn was flagged: {v:?}\n{h}"
        );
    }

    let root = repo_root();
    let helper = root.join("crates/axon-core/tests/script_spawn/mod.rs");
    let mut files = vec![];
    let crates_dir = root.join("crates");
    for e in std::fs::read_dir(&crates_dir).unwrap().flatten() {
        walk(
            &e.path().join("tests"),
            &|p: &Path| p.extension().and_then(|x| x.to_str()) == Some("rs"),
            &mut files,
        );
    }
    assert!(files.len() > 50, "found only {} test sources", files.len());
    let mut bad = vec![];
    for f in files {
        if f.canonicalize().ok() == helper.canonicalize().ok() {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap();
        for v in binary_resolution_violations(&text) {
            bad.push(format!(
                "{}: {v}",
                f.strip_prefix(&root).unwrap_or(&f).display()
            ));
        }
        for v in spawn_violations(&text) {
            bad.push(format!(
                "{}: {v}",
                f.strip_prefix(&root).unwrap_or(&f).display()
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "tests that run a script around tests/script_spawn (it inherits ambient binaries and \
         never checks the script's choice):\n  {}",
        bad.join("\n  ")
    );
}

/// The spawn helper reads a script BEFORE running it: one that picks its
/// binary by a guessed `target/` path is refused, and never runs. Control: an
/// honest script runs.
#[test]
fn the_spawn_helper_refuses_a_script_that_guesses_its_binary() {
    let root = scratch("guess");
    let ran = root.join("guess-ran");
    let guess = root.join("scripts/guess.sh");
    write(
        &guess,
        &format!(
            "#!/bin/sh\n: > '{}'\nAXON=\"${{AXON:-./target/{}/axon}}\"\n",
            ran.display(),
            "debug"
        ),
    );
    let g = guess.clone();
    let got = std::panic::catch_unwind(move || {
        script_spawn::script("sh", &g, Bins::BuildsItsOwn)
            .output()
            .unwrap()
    });
    assert!(
        got.is_err() && !ran.exists(),
        "ATTACK: the spawn helper ran a script that guesses its binary"
    );
    let honest = root.join("scripts/honest.sh");
    let ok_ran = root.join("honest-ran");
    write(&honest, &format!("#!/bin/sh\n: > '{}'\n", ok_ran.display()));
    let o = script_spawn::script("sh", &honest, Bins::NoWorkspaceBinary)
        .output()
        .unwrap();
    assert!(
        o.status.success() && ok_ran.exists(),
        "control: an honest script runs"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// A binary a harness names for a test to exec is refused when it is older
/// than the sources cargo would rebuild it from: the PSV-1 attack test PASSED
/// its attack on a `debug/axon` that predated the fix (C9 round 4,
/// integration). Control: a binary newer than its sources is used as named.
#[test]
fn a_named_binary_older_than_its_sources_is_refused() {
    let root = scratch("stale-named");
    let stale = root.join("axon-stale");
    write(&stale, "#!/bin/sh\n");
    std::fs::File::options()
        .write(true)
        .open(&stale)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(86_400))
        .unwrap();
    // Only this test reads this variable.
    std::env::set_var("AXON_TEST_NAMED_BIN", &stale);
    let build = [
        "build",
        "-p",
        "axon-core",
        "--no-default-features",
        "--bin",
        "axon",
    ];
    let got = std::panic::catch_unwind(|| {
        script_spawn::workspace_bin("AXON_TEST_NAMED_BIN", &build, "axon")
    });
    assert!(
        got.is_err(),
        "ATTACK: a named binary older than its sources was handed to a test to exec"
    );
    let fresh = root.join("axon-fresh");
    write(&fresh, "#!/bin/sh\n");
    std::env::set_var("AXON_TEST_NAMED_BIN", &fresh);
    let got = script_spawn::workspace_bin("AXON_TEST_NAMED_BIN", &build, "axon");
    std::env::remove_var("AXON_TEST_NAMED_BIN");
    assert_eq!(
        got, fresh,
        "control: a current named binary is used as named"
    );
    let _ = std::fs::remove_dir_all(&root);
}
