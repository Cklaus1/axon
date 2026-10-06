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

#[test]
fn a_generators_stdout_is_never_buffered_past_its_cap() {
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
    let (grew, _) = propose_with("head -c 400000000 /dev/zero >&2; exit 1");
    assert!(
        grew < 120_000,
        "ATTACK: the loop buffered a generator's stderr flood ({grew} kB of peak growth)"
    );
}
