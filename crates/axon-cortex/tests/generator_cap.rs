//! C9 round 7, EQGATE3 (amendment 91): the command generator's output is read
//! through a cap (`take(MAX_GENERATOR_READ)`), on stdout and stderr alike. The
//! cap is a size bound that builds no `Err`; removed, a generator that writes
//! gigabytes makes the loop buffer them and abort on allocation failure,
//! taking the episode record with it, and every suite stayed green.

mod common;
use axon_cortex::action::SymbolRef;
use axon_cortex::generate::{CommandGenerator, PatchConstraints, PatchGenerator};
use axon_cortex::Observation;

fn peak_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .expect("VmHWM")
}

fn propose_with(script_body: &str) -> (u64, Result<usize, String>) {
    let d = std::env::temp_dir().join(format!("cortex-gen-cap-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let exe = d.join(format!("gen-{}.sh", script_body.len()));
    common::write_executable(&exe, format!("#!/bin/sh\n{script_body}\n"), 0o755);
    let obs = Observation {
        observation_id: "o".into(),
        snapshot_id: "s".into(),
        diagnostics: vec![],
        warnings: vec![],
        facts: vec![],
        omission_report: vec![],
    };
    let sym = SymbolRef {
        path: "f.ax".into(),
        symbol: "f".into(),
    };
    let c = PatchConstraints {
        symbol: sym.clone(),
        max_bytes: 1 << 30,
        current_body: "fn f() {}".into(),
        rejected: vec![],
    };
    let before = peak_rss_kb();
    let got = CommandGenerator::new(exe.display().to_string())
        .propose(&obs, &sym, &c)
        .map(|p| p.body.len())
        .map_err(|e| format!("{e:?}"));
    (peak_rss_kb().saturating_sub(before), got)
}

/// C9 round 7 (eqgate3): run `test` again ALONE in a child of this test binary
/// and say whether THIS process is that child. Peak RSS (`VmHWM`) is a property
/// of the whole process, and a sibling test thread that allocates would be read
/// as the growth under test: a suite run found the flood test failing on a
/// neighbour's memory. Usage: `if !alone("name") { return; }` first.
fn alone(test: &str) -> bool {
    if std::env::var("AXON_EQ_ALONE").as_deref() == Ok(test) {
        return true;
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--test-threads=1", "--nocapture"])
        .env("AXON_EQ_ALONE", test)
        .output()
        .unwrap();
    assert!(
        o.status.success() && String::from_utf8_lossy(&o.stdout).contains("1 passed"),
        "{test} failed when run alone:\n{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    false
}

#[test]
fn a_generators_stdout_is_never_buffered_past_its_cap() {
    if !alone("a_generators_stdout_is_never_buffered_past_its_cap") {
        return;
    }
    let (_, small) = propose_with("echo fn_body");
    assert_eq!(small, Ok(8), "control: a small answer is read whole");
    let (grew, _) = propose_with("head -c 400000000 /dev/zero");
    assert!(
        grew < 120_000,
        "ATTACK: the loop buffered a generator's stdout flood ({grew} kB of peak growth)"
    );
}

#[test]
fn a_generators_stderr_is_never_buffered_past_its_cap() {
    if !alone("a_generators_stderr_is_never_buffered_past_its_cap") {
        return;
    }
    let (grew, _) = propose_with("head -c 400000000 /dev/zero >&2; exit 1");
    assert!(
        grew < 120_000,
        "ATTACK: the loop buffered a generator's stderr flood ({grew} kB of peak growth)"
    );
}
