//! PSV review wf_d725935a-7ed, B1 and the sibling MINOR: what `axon test`
//! collects when the protected runner invokes it.
//!
//! * Under `--seal`, a `@[test]` defined in a SEALED module (the candidate)
//!   is never collected: candidate bytes neither add a check nor supply the
//!   registered one. Control: without `--seal` the same test IS collected.
//! * `--exact` selects only the test with exactly the filter's name.
//!   Control: without it the substring sibling runs.

use std::path::Path;
use std::process::Command;

/// A fresh directory for one test (no tempfile dev-dependency here).
/// Directories left by EARLIER test processes (whose pid is gone) are swept:
/// each run left up to a dozen, one holding a copy of the axon binary, and the
/// accumulation filled a 12 GB /tmp mid-suite (C9 round 1b: native linker
/// "No space left on device" in unrelated harnesses).
fn fresh(name: &str) -> std::path::PathBuf {
    if let Ok(rd) = std::fs::read_dir(std::env::temp_dir()) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            let pid = n
                .strip_prefix("psv-sel-")
                .and_then(|r| r.split('-').next())
                .and_then(|p| p.parse::<u32>().ok());
            if let Some(pid) = pid {
                if !Path::new(&format!("/proc/{pid}")).exists() {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
    let d = std::env::temp_dir().join(format!("psv-sel-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fixture(d: &Path) {
    std::fs::create_dir_all(d.join("cand")).unwrap();
    std::fs::create_dir_all(d.join("suite")).unwrap();
    std::fs::write(
        d.join("cand/f.ax"),
        "fn double(x: i64) -> i64 { x * 2 }\n\n@[test]\nfn t_cand_probe() { assert(true) }\n",
    )
    .unwrap();
    std::fs::write(
        d.join("suite/accept.ax"),
        "mod f\nuse f.{double}\n\n@[test]\nfn t_ok() { assert_eq(double(21), 42) }\n\n@[test]\nfn t_ok_edge() { assert_eq(double(1), 3) }\n",
    )
    .unwrap();
}

/// The JSON result lines' names, and the summary's total.
fn run(d: &Path, filter: &str, exact: bool, seal: bool) -> (Vec<String>, u64) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_axon"));
    c.current_dir(d.join("suite"))
        .arg("test")
        .arg(d.join("suite/accept.ax"))
        .args(["--json", "--filter", filter])
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env(
            "AXON_PATH",
            format!("{}:{}", d.join("suite").display(), d.join("cand").display()),
        )
        .env("AXON_PATH_EXCLUSIVE", "1");
    if exact {
        c.arg("--exact");
    }
    if seal {
        c.arg("--seal").arg(d.join("cand"));
    }
    let out = c.output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let mut names = vec![];
    let mut total = 0;
    for l in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(l) else {
            continue;
        };
        if v["type"] == "summary" {
            total = v["total"].as_u64().unwrap_or(0);
        } else if let Some(n) = v["name"].as_str() {
            names.push(n.to_string());
        }
    }
    (names, total)
}

#[test]
fn a_sealed_candidates_own_test_is_never_collected() {
    let d = fresh("sealed");
    fixture(&d);
    // Control: unsealed, the candidate's test IS collected.
    let (names, _) = run(&d, "t_cand_probe", false, false);
    assert_eq!(names, vec!["t_cand_probe".to_string()], "control");
    // Sealed: not collected — not even when the filter names it exactly.
    let (names, total) = run(&d, "t_cand_probe", true, true);
    assert!(names.is_empty(), "{names:?}");
    assert_eq!(total, 0);
    let (names, _) = run(&d, "t_", false, true);
    assert!(!names.iter().any(|n| n == "t_cand_probe"), "{names:?}");
}

#[test]
fn exact_selects_only_the_named_test() {
    let d = fresh("exact");
    fixture(&d);
    // Control: a substring filter also runs the sibling.
    let (names, _) = run(&d, "t_ok", false, true);
    assert!(
        names.contains(&"t_ok_edge".to_string()),
        "control: {names:?}"
    );
    let (names, total) = run(&d, "t_ok", true, true);
    assert_eq!(names, vec!["t_ok".to_string()]);
    assert_eq!(total, 1);
}

fn hmac_hex(key: &[u8], msg: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut k = [0u8; 64];
    k[..key.len()].copy_from_slice(key);
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Review wf_1bc28496-38e (PSV-4): under a completion key, a FAILURE the
/// interpreter decided carries its own keyed token, in a domain distinct from
/// a pass's — so a printed "failed" line without it is not a verdict.
#[test]
fn a_failure_is_keyed_in_its_own_domain() {
    use std::io::Write;
    let d = fresh("keyed");
    fixture(&d);
    let key = [0x0bu8; 32];
    let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
    let line_for = |test: &str| -> serde_json::Value {
        let mut c = Command::new(env!("CARGO_BIN_EXE_axon"))
            .current_dir(d.join("suite"))
            .arg("test")
            .arg(d.join("suite/accept.ax"))
            .args([
                "--json",
                "--filter",
                test,
                "--exact",
                "--completion-key-stdin",
            ])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env(
                "AXON_PATH",
                format!("{}:{}", d.join("suite").display(), d.join("cand").display()),
            )
            .env("AXON_PATH_EXCLUSIVE", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(c.stdin.take().unwrap(), "{hex}").unwrap();
        let out = c.wait_with_output().unwrap();
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .find(|v| v["name"] == test)
            .unwrap_or_default()
    };
    let token = |domain: &str, test: &str| {
        let mut m = domain.as_bytes().to_vec();
        m.push(0);
        m.extend_from_slice(test.as_bytes());
        hmac_hex(&key, &m)
    };
    let f = line_for("t_ok_edge");
    assert_eq!(f["status"], "failed", "{f}");
    assert_eq!(
        f["completion"].as_str(),
        Some(token("axon-test-failed/1", "t_ok_edge").as_str()),
        "{f}"
    );
    // Control: a pass is keyed in the completion domain, not the failure one.
    let p = line_for("t_ok");
    assert_eq!(p["status"], "ok", "{p}");
    assert_eq!(
        p["completion"].as_str(),
        Some(token("axon-test-completion/1", "t_ok").as_str()),
        "{p}"
    );
}

/// Review wf_ecfcd666-6c9 (PSV-3): in the guest, under an Exec grant, a
/// candidate spawned a helper that read K out of this process's memory. While
/// it holds a completion key, `axon test` spawns NOTHING, whatever the ceiling
/// granted. Control: the same program without a key spawns.
#[test]
fn holding_a_completion_key_spawns_nothing() {
    use std::io::Write;
    let d = fresh("spawn");
    std::fs::write(
        d.join("spawn.ax"),
        "@[test]\nfn t_spawn() {\n    match exec(\"/bin/echo\", [\"SPAWNED\"]) {\n        Ok(o) => println(\"OUT:{o}\")\n        Err(e) => println(\"ERR:{e}\")\n    }\n}\n",
    )
    .unwrap();
    let run = |key: bool, ceiling: Option<&str>| -> String {
        let mut c = Command::new(env!("CARGO_BIN_EXE_axon"));
        c.current_dir(&d)
            .arg("test")
            .arg(d.join("spawn.ax"))
            .args(["--json", "--filter", "t_spawn", "--exact"])
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped());
        if let Some(v) = ceiling {
            c.env("AXON_ALLOWED_EFFECTS", v);
        }
        if key {
            c.arg("--completion-key-stdin");
        }
        let mut ch = c.spawn().unwrap();
        writeln!(ch.stdin.take().unwrap(), "{}", "0b".repeat(32)).unwrap();
        String::from_utf8_lossy(&ch.wait_with_output().unwrap().stdout).into_owned()
    };
    assert!(
        run(false, Some("IO,Exec")).contains("OUT:SPAWNED"),
        "control"
    );
    for ceiling in [Some("IO,Exec"), None] {
        let out = run(true, ceiling);
        assert!(!out.contains("SPAWNED"), "{ceiling:?}: {out}");
        assert!(out.contains("requires effect `Exec`"), "{ceiling:?}: {out}");
    }
}

/// Review wf_ecfcd666-6c9 (PSV-3): while it holds a completion key, `axon
/// test` is NON-DUMPABLE: an unprivileged process — even its own parent,
/// which the host's ptrace policy would otherwise allow — cannot open its
/// memory. Control: without a key the same parent can. Needs root (to run the
/// parent as nobody); otherwise this is not exercised and says so.
#[test]
fn holding_a_completion_key_makes_the_process_non_dumpable() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::process::CommandExt;
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("NOT EXERCISED: needs root to run the probing parent as nobody");
        return;
    }
    let d = fresh("dump");
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o777)).unwrap();
    let axon = d.join("axon");
    std::fs::copy(env!("CARGO_BIN_EXE_axon"), &axon).unwrap();
    std::fs::set_permissions(&axon, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(
        d.join("sleep.ax"),
        "@[test]\nfn t_sleep() { sleep_ms(2500) }\n",
    )
    .unwrap();
    std::fs::set_permissions(d.join("sleep.ax"), std::fs::Permissions::from_mode(0o644)).unwrap();
    let probe = |flag: &str| -> String {
        let script = format!(
            "cd {dir}\necho {key} | ./axon test sleep.ax --json --filter t_sleep --exact {flag} >/dev/null 2>&1 &\n\
             P=$!\nsleep 1\nif {{ :; }} 3</proc/$P/mem 2>/dev/null; then echo OPENED; else echo DENIED; fi\nwait $P\n",
            dir = d.display(),
            key = "0b".repeat(32),
        );
        let out = Command::new("/bin/sh")
            .arg("-c")
            .arg(script)
            .uid(65534)
            .gid(65534)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    assert_eq!(probe(""), "OPENED", "control: a dumpable child");
    assert_eq!(probe("--completion-key-stdin"), "DENIED");
}

/// Review wf_293dfdb6-9d8 (PSV-1, executed to a keyed PASS): a SUITE module
/// that exists but cannot be read (a non-UTF-8 byte; a directory named like
/// the module) must never let a later search directory — the candidate's —
/// define it. The candidate plants `helper.ax` with the answer the suite's
/// test wants; the run must not pass, and must name the unreadable module.
#[test]
fn an_unreadable_suite_module_never_falls_through_to_the_candidate() {
    use std::io::Write;
    for (case, plant_suite) in [("latin1", true), ("dir", false)] {
        let d = fresh(&format!("fall-{case}"));
        std::fs::create_dir_all(d.join("cand")).unwrap();
        std::fs::create_dir_all(d.join("suite")).unwrap();
        std::fs::write(
            d.join("suite/accept.ax"),
            "mod helper\nuse helper.{want}\n\n@[test]\nfn t_helper() { assert_eq(want(), 42) }\n",
        )
        .unwrap();
        if plant_suite {
            // The operator's helper can never pass (7 != 42), and one Latin-1
            // byte makes it unreadable as UTF-8.
            std::fs::write(
                d.join("suite/helper.ax"),
                b"fn want() -> i64 { 7 }\n// caf\xe9\n",
            )
            .unwrap();
        } else {
            std::fs::create_dir_all(d.join("suite/helper.ax")).unwrap();
        }
        std::fs::write(d.join("cand/helper.ax"), "fn want() -> i64 { 42 }\n").unwrap();
        let mut c = Command::new(env!("CARGO_BIN_EXE_axon"))
            .current_dir(d.join("suite"))
            .arg("test")
            .arg(d.join("suite/accept.ax"))
            .args([
                "--json",
                "--filter",
                "t_helper",
                "--exact",
                "--completion-key-stdin",
            ])
            .arg("--seal")
            .arg(d.join("cand"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env(
                "AXON_PATH",
                format!("{}:{}", d.join("suite").display(), d.join("cand").display()),
            )
            .env("AXON_PATH_EXCLUSIVE", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(c.stdin.take().unwrap(), "{}", "0b".repeat(32)).unwrap();
        let out = c.wait_with_output().unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stdout.contains("\"status\":\"ok\""), "{case}: {stdout}");
        assert!(
            stderr.contains("E0901") && stderr.contains("does not fall through"),
            "{case}: {stderr}"
        );
    }
}

/// Review round wf_336353cb-a2b (PSV-1, executed to a keyed PASS): a sealed
/// candidate module's own `use` must not pull an operator module that the
/// entry does not import into the program. Here the suite tree ships a
/// reference module the entry never imports; the candidate defines nothing
/// and only imports it. Control: the entry's OWN import of a candidate module
/// still works (the honest candidate passes).
#[test]
fn a_sealed_modules_use_never_reaches_an_unimported_suite_module() {
    use std::io::Write;
    let run = |cand_f: &str| -> (String, String) {
        let d = fresh(&format!("sealuse-{}", cand_f.len()));
        std::fs::create_dir_all(d.join("cand")).unwrap();
        std::fs::create_dir_all(d.join("suite")).unwrap();
        std::fs::write(
            d.join("suite/accept.ax"),
            "mod f\nuse f.{double}\n\n@[test]\nfn t_ok() { assert_eq(double(21), 42) }\n",
        )
        .unwrap();
        std::fs::write(
            d.join("suite/reference.ax"),
            "fn double(x: i64) -> i64 { x * 2 }\n",
        )
        .unwrap();
        std::fs::write(d.join("cand/f.ax"), cand_f).unwrap();
        let mut c = Command::new(env!("CARGO_BIN_EXE_axon"))
            .current_dir(d.join("suite"))
            .arg("test")
            .arg(d.join("suite/accept.ax"))
            .args([
                "--json",
                "--filter",
                "t_ok",
                "--exact",
                "--completion-key-stdin",
            ])
            .arg("--seal")
            .arg(d.join("cand"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env(
                "AXON_PATH",
                format!("{}:{}", d.join("suite").display(), d.join("cand").display()),
            )
            .env("AXON_PATH_EXCLUSIVE", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(c.stdin.take().unwrap(), "{}", "0b".repeat(32)).unwrap();
        let out = c.wait_with_output().unwrap();
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let (ok, _) = run("fn double(x: i64) -> i64 { x * 2 }\n");
    assert!(
        ok.contains("\"status\":\"ok\""),
        "control: an honest candidate passes: {ok}"
    );
    let (out, err) = run("mod reference\nuse reference\n");
    assert!(!out.contains("\"status\":\"ok\""), "{out}\n{err}");
}

/// Dev review round wf_7cb5856d-806 (a regression of the round-1 fix,
/// executed there to a keyed PASS): one file per module name, suite first,
/// whoever asks. The suite imports a candidate module BEFORE its own
/// `rubric`; the candidate's module `use`s `rubric` and ships a same-named
/// copy. That copy must never become the rubric. Control: the honest
/// candidate passes against the operator's rubric.
///
/// C9 round 1b: the suite-first case alone cannot tell this guard is there —
/// the search reaches `suite/rubric.ax` before the candidate's copy, so with
/// the guard removed the operator's rubric still loads and the attack fails
/// for a reason that is not this guard. The ONLY-GUARD route is an operator
/// module in a directory searched AFTER the candidate's: the operator's
/// library (`~/.axon/lib`, which a non-exclusive run searches after every
/// `AXON_PATH` entry). There the candidate's copy is the first file found,
/// and the sealed-`use` check is the one thing that stops it defining the
/// rubric.
#[test]
fn a_sealed_module_never_supplies_an_operator_modules_name() {
    use std::io::Write;
    // `in_lib`: the operator's rubric lives in the operator's library under
    // HOME (searched after the candidate), not in the suite tree.
    let run = |cand: &[(&str, &str)], in_lib: bool| -> (String, String) {
        let d = fresh(&format!("claim-{}-{in_lib}", cand.len()));
        std::fs::create_dir_all(d.join("cand")).unwrap();
        std::fs::create_dir_all(d.join("suite")).unwrap();
        std::fs::create_dir_all(d.join("home/.axon/lib")).unwrap();
        std::fs::write(
            d.join("suite/accept.ax"),
            "mod f\nuse f.{double}\nmod rubric\nuse rubric.{expected}\n\n@[test]\nfn t_ok() { assert_eq(double(21), expected()) }\n",
        )
        .unwrap();
        let rubric_dir = if in_lib { "home/.axon/lib" } else { "suite" };
        std::fs::write(
            d.join(rubric_dir).join("rubric.ax"),
            "fn expected() -> i64 { 42 }\n",
        )
        .unwrap();
        for (n, src) in cand {
            std::fs::write(d.join("cand").join(n), src).unwrap();
        }
        let mut c = Command::new(env!("CARGO_BIN_EXE_axon"));
        c.env_clear();
        if !in_lib {
            // The PSV runner's shape: the named directories and nothing else.
            c.env("AXON_PATH_EXCLUSIVE", "1");
        }
        let mut c = c
            .current_dir(d.join("suite"))
            .arg("test")
            .arg(d.join("suite/accept.ax"))
            .args([
                "--json",
                "--filter",
                "t_ok",
                "--exact",
                "--completion-key-stdin",
            ])
            .arg("--seal")
            .arg(d.join("cand"))
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", d.join("home"))
            .env(
                "AXON_PATH",
                format!("{}:{}", d.join("suite").display(), d.join("cand").display()),
            )
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(c.stdin.take().unwrap(), "{}", "0b".repeat(32)).unwrap();
        let out = c.wait_with_output().unwrap();
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let honest = [("f.ax", "fn double(x: i64) -> i64 { x * 2 }\n")];
    let attack = [
        (
            "f.ax",
            "use rubric.{expected}\nfn double(x: i64) -> i64 { 999 }\n",
        ),
        ("rubric.ax", "fn expected() -> i64 { 999 }\n"),
    ];
    for in_lib in [true, false] {
        let (ok, _) = run(&honest, in_lib);
        assert!(
            ok.contains("\"status\":\"ok\""),
            "control (in_lib={in_lib}): an honest candidate passes: {ok}"
        );
    }
    // The only-guard route first, so a missing guard fails HERE, on the
    // attack getting through.
    let (out, err) = run(&attack, true);
    assert!(
        !out.contains("\"status\":\"ok\""),
        "ATTACK: a sealed module's copy of the operator library's `rubric` defined the rubric: {out}\n{err}"
    );
    // No reason check on this route: the nested use judged as sealed (M260)
    // and the first-match rule (M436) each refuse it alone (M260's four-cell
    // record), with different diagnostics.
    let (out, err) = run(&attack, false);
    assert!(!out.contains("\"status\":\"ok\""), "{out}\n{err}");
    assert!(err.contains("may not supply it"), "{err}");
}

/// C9 round 1b (PSV-1, class b: the sealed-`use` rule covered one importer
/// only). One file per module name, suite first, WHOEVER asks: the operator
/// suite's OWN `use rubric`, resolved before the candidate's module loads,
/// must never land on the candidate's `rubric.ax` while the operator's library
/// holds the name. A non-exclusive `--seal` run searches `~/.axon/lib` after
/// every `AXON_PATH` entry, so the candidate directory came first and the
/// candidate defined the rubric (keyed pass, executed on the unmutated tree).
#[test]
fn an_operator_import_never_resolves_to_a_sealed_modules_copy() {
    use std::io::Write;
    let run = |cand: &[(&str, &str)]| -> (String, String) {
        let d = fresh(&format!("opimport-{}", cand.len()));
        std::fs::create_dir_all(d.join("cand")).unwrap();
        std::fs::create_dir_all(d.join("suite")).unwrap();
        std::fs::create_dir_all(d.join("home/.axon/lib")).unwrap();
        // The operator imports its rubric FIRST, before the candidate's module.
        std::fs::write(
            d.join("suite/accept.ax"),
            "mod rubric\nuse rubric.{expected}\nmod f\nuse f.{double}\n\n@[test]\nfn t_ok() { assert_eq(double(21), expected()) }\n",
        )
        .unwrap();
        std::fs::write(
            d.join("home/.axon/lib/rubric.ax"),
            "fn expected() -> i64 { 42 }\n",
        )
        .unwrap();
        for (n, src) in cand {
            std::fs::write(d.join("cand").join(n), src).unwrap();
        }
        let mut c = Command::new(env!("CARGO_BIN_EXE_axon"))
            .env_clear()
            .current_dir(d.join("suite"))
            .arg("test")
            .arg(d.join("suite/accept.ax"))
            .args([
                "--json",
                "--filter",
                "t_ok",
                "--exact",
                "--completion-key-stdin",
            ])
            .arg("--seal")
            .arg(d.join("cand"))
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", d.join("home"))
            .env(
                "AXON_PATH",
                format!("{}:{}", d.join("suite").display(), d.join("cand").display()),
            )
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(c.stdin.take().unwrap(), "{}", "0b".repeat(32)).unwrap();
        let out = c.wait_with_output().unwrap();
        (
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    };
    let (ok, err) = run(&[("f.ax", "fn double(x: i64) -> i64 { x * 2 }\n")]);
    assert!(
        ok.contains("\"status\":\"ok\""),
        "control: an honest candidate passes: {ok}\n{err}"
    );
    let (out, err) = run(&[
        ("f.ax", "fn double(x: i64) -> i64 { 999 }\n"),
        ("rubric.ax", "fn expected() -> i64 { 999 }\n"),
    ]);
    assert!(
        !out.contains("\"status\":\"ok\""),
        "ATTACK: the operator's own `use rubric` loaded the candidate's copy, which defined the rubric: {out}\n{err}"
    );
    assert!(err.contains("may not supply it"), "{err}");
}

/// Dev review round wf_bf757240-925 (PSV-1): a sealed candidate cannot reseed
/// the process RNG the operator's test draws from. The candidate reseeds to a
/// fixed seed at load and returns the draw it can then PREDICT. Its srand
/// reseeds only its OWN kernel's stream, so it guesses wrong. Control: an honest candidate that
/// echoes the value it is given passes.
#[test]
fn a_sealed_candidate_cannot_reseed_the_rng() {
    use std::io::Write;
    // The first `random_i64(0, 1000000)` after `srand(7)` (computed from the
    // interpreter's xorshift): a predicted value only a reseed makes knowable.
    const PREDICTED: i64 = 888327;
    let run = |cand: &str| -> String {
        let d = fresh(&format!("rng-{}", cand.len()));
        std::fs::create_dir_all(d.join("cand")).unwrap();
        std::fs::create_dir_all(d.join("suite")).unwrap();
        std::fs::write(
            d.join("suite/accept.ax"),
            "mod f
use f.{guess}

@[test]
fn t_ok() {
    let x = random_i64(0, 1000000)
    assert_eq(guess(x), x)
}
",
        )
        .unwrap();
        std::fs::write(d.join("cand/f.ax"), cand).unwrap();
        let mut c = Command::new(env!("CARGO_BIN_EXE_axon"))
            .current_dir(d.join("suite"))
            .arg("test")
            .arg(d.join("suite/accept.ax"))
            .args([
                "--json",
                "--filter",
                "t_ok",
                "--exact",
                "--completion-key-stdin",
            ])
            .arg("--seal")
            .arg(d.join("cand"))
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("AXON_ALLOWED_EFFECTS", "IO,Random")
            .env(
                "AXON_PATH",
                format!("{}:{}", d.join("suite").display(), d.join("cand").display()),
            )
            .env("AXON_PATH_EXCLUSIVE", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        writeln!(c.stdin.take().unwrap(), "{}", "0b".repeat(32)).unwrap();
        let out = c.wait_with_output().unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    // The attack: reseed at load, then return the value the operator's draw
    // will now take. A genuine pass only if the reseed steered the RNG.
    let attack = format!(
        "let _S = srand(7)
fn guess(x: i64) -> i64 {{ {PREDICTED} }}
"
    );
    let a = run(&attack);
    assert!(
        !a.contains("\"status\":\"ok\""),
        "the reseed must not pass: {a}"
    );
    // Control: an honest candidate that returns what it is given passes.
    let honest = "fn guess(x: i64) -> i64 { x }
";
    assert!(
        run(honest).contains("\"status\":\"ok\""),
        "control: honest candidate passes"
    );
}

/// Run one registered test of `suite` with `cand` sealed, in the runner's shape.
fn run_sealed(tag: &str, suite: &str, cand: &str, test: &str, env: &[(&str, &str)]) -> String {
    use std::io::Write;
    let d = fresh(tag);
    std::fs::create_dir_all(d.join("cand")).unwrap();
    std::fs::create_dir_all(d.join("suite")).unwrap();
    std::fs::write(d.join("suite/accept.ax"), suite).unwrap();
    std::fs::write(d.join("cand/f.ax"), cand).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_axon"));
    cmd.current_dir(d.join("suite"))
        .arg("test")
        .arg(d.join("suite/accept.ax"))
        .args([
            "--json",
            "--filter",
            test,
            "--exact",
            "--completion-key-stdin",
        ])
        .arg("--seal")
        .arg(d.join("cand"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("AXON_ALLOWED_EFFECTS", "IO,Random,AI,Net")
        .env(
            "AXON_PATH",
            format!("{}:{}", d.join("suite").display(), d.join("cand").display()),
        )
        .env("AXON_PATH_EXCLUSIVE", "1")
        .env("XDG_CACHE_HOME", d.join("cache"))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut c = cmd.spawn().unwrap();
    writeln!(c.stdin.take().unwrap(), "{}", "0b".repeat(32)).unwrap();
    let out = c.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// PSV-1, certifying review wf_ae3a5a74-41e. A sealed candidate cannot move
/// the operator's RNG stream — by ANY builtin. Every draw comes from the
/// running frame's kernel, and the candidate's kernel has its own stream. The
/// operator seeds, draws `x`, reseeds, lets the candidate churn, draws `y`:
/// `x == y`. Two earlier fixes guarded a LIST (three builtin names, then the
/// `Random` effect row) and each missed a route — the samplers, then the
/// `goal_*` searches, whose row says `{AI,Net,IO}`. So the churn is not a
/// hand-picked list for the Random-row builtins: it is checked against
/// `builtins::BUILTINS` at test time, and a new `Random` builtin fails this
/// test until the churn exercises it from a sealed frame.
#[test]
fn sealed_rng_activity_never_moves_the_operators_stream() {
    use axon_core::builtins::{builtin_effect_row, BUILTINS};
    // How to call each RNG-reaching builtin from the candidate.
    let calls: &[(&str, &str)] = &[
        ("random_i64", "random_i64(0, 1000000)"),
        ("random_f64", "random_f64()"),
        ("srand", "srand(7)"),
        ("gaussian_sample", "gaussian_sample(0.0, 1.0)"),
        ("beta_sample", "beta_sample(2.0, 5.0)"),
        ("categorical_sample", "categorical_sample([0.5, 0.5])"),
        // Not Random-row, yet they draw (the route the review executed).
        (
            "goal_run_random",
            "goal_run_random(\"probe\", 1000000.0, 5, 0, 1000000)",
        ),
        (
            "goal_run_multistart",
            "goal_run_multistart(\"probe\", 1000000.0, 2, 3, 0, 1000000)",
        ),
        (
            "goal_run_categorical",
            "goal_run_categorical(\"cprobe\", 4, 100.0, 5)",
        ),
    ];
    let covered: Vec<&str> = calls.iter().map(|(n, _)| *n).collect();
    let random_row: Vec<&str> = BUILTINS
        .iter()
        .map(|b| b.name)
        .filter(|n| builtin_effect_row(n).contains(&"Random"))
        .collect();
    assert!(
        !random_row.is_empty(),
        "drift test found no Random builtins at all"
    );
    for n in &random_row {
        assert!(
            covered.contains(n),
            "`{n}` carries the Random effect but is not exercised from a sealed frame \
             here — add it to `calls` so its draws are proven not to reach the operator's stream"
        );
    }
    let body: String = calls
        .iter()
        .enumerate()
        .map(|(i, (_, c))| format!("    let _r{i} = {c}\n"))
        .collect();
    let cand = format!(
        "@[adaptive]\nfn probe(a: i64) -> i64 {{ a }}\n@[adaptive]\nfn cprobe(c: i64) -> i64 {{ c }}\n\
         pub fn churn() -> i64 {{\n{body}    0\n}}\n"
    );
    let suite = "mod f
use f.{churn}

@[test]
fn t_ok() {
    srand(20260929)
    let x = random_i64(0, 1000000)
    srand(20260929)
    let _ = churn()
    let y = random_i64(0, 1000000)
    assert_eq(x, y)
}

@[test]
fn t_moves() {
    srand(20260929)
    let x = random_i64(0, 1000000)
    srand(20260929)
    let _ = churn()
    let _own = random_i64(0, 1000000)
    let y = random_i64(0, 1000000)
    assert_eq(x, y)
}
";
    let out = run_sealed("rng-churn", suite, &cand, "t_ok", &[]);
    assert!(
        out.contains("\"status\":\"ok\""),
        "the candidate's RNG activity moved the operator's stream: {out}"
    );
    // Control: the SAME check detects a stream that really moved (an operator
    // draw between x and y), so the pass above is the property, not a no-op.
    let moved = run_sealed("rng-churn-ctl", suite, &cand, "t_moves", &[]);
    assert!(
        moved.contains("\"status\":\"failed\""),
        "control: an operator-side draw must change y: {moved}"
    );
}

/// PSV-1: drawing from its own stream tells a candidate nothing about the
/// operator's. Under a fixed `AXON_SEED` both kernels start from the same
/// operator seed; the sealed kernel's is passed through a one-way derivation,
/// so the candidate's first draw is NOT the operator's first draw.
#[test]
fn a_sealed_candidates_own_stream_reveals_nothing_of_the_operators() {
    let suite = "mod f
use f.{peek}

@[test]
fn t_ok() {
    let x = random_i64(0, 1000000)
    assert_eq(peek(), x)
}
";
    let cand = "pub fn peek() -> i64 { random_i64(0, 1000000) }\n";
    let out = run_sealed("rng-peek", suite, cand, "t_ok", &[("AXON_SEED", "424242")]);
    assert!(
        out.contains("\"status\":\"failed\""),
        "the candidate's first draw equalled the operator's — its stream mirrors the operator seed: {out}"
    );
}
