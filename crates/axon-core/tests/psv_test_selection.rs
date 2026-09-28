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
fn fresh(name: &str) -> std::path::PathBuf {
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
