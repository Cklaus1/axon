//! The release verification manifest must cover the workspace, exactly.
//!
//! WHY THIS EXISTS. Release-critical coverage used to live in shell fragments
//! inside gate.sh, and the result was measurable: the strict gate COMPILED 20
//! of 22 crates (clippy --all-targets) while RUNNING the tests of only 5.
//! `axon-ledger` — which holds the RBAC model, record attribution, the MCP
//! serving surface and webhook egress, and where every authority fix of this
//! hardening pass landed — was among the crates whose tests never ran. A
//! passing gate therefore said nothing about them, and nothing anywhere
//! noticed the omission, because no single place stated what coverage was
//! supposed to be.
//!
//! Compiling a test is not running it. This pins the intent in one
//! machine-readable place so a new crate cannot arrive unclassified.

use std::collections::BTreeSet;

fn root() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Workspace members, read from the root Cargo.toml.
fn workspace_members() -> BTreeSet<String> {
    let text = std::fs::read_to_string(root().join("Cargo.toml")).expect("root Cargo.toml");
    let block = text
        .split_once("members")
        .and_then(|(_, r)| r.split_once('['))
        .and_then(|(_, r)| r.split_once(']'))
        .map(|(inner, _)| inner.to_string())
        .expect("a members = [...] list");
    block
        .split(',')
        .filter_map(|s| {
            let s = s.trim().trim_matches('"').trim();
            if s.is_empty() {
                return None;
            }
            // "crates/axon-core" -> "axon-core"
            s.rsplit('/').next().map(|n| n.to_string())
        })
        .collect()
}

fn manifest() -> serde_json::Value {
    let p = root().join("governance/release-verification.json");
    let text = std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("release manifest missing at {}: {e}", p.display()));
    serde_json::from_str(&text).expect("release manifest is valid JSON")
}

#[test]
fn release_manifest_covers_every_workspace_member() {
    let members = workspace_members();
    assert!(
        members.len() >= 20,
        "parsed only {} workspace members — the parser has drifted and this \
         guard would be vacuous: {members:?}",
        members.len()
    );

    let m = manifest();
    let classified: BTreeSet<String> = m["crates"]
        .as_object()
        .expect("crates object")
        .keys()
        .cloned()
        .collect();

    let unclassified: Vec<&String> = members.difference(&classified).collect();
    assert!(
        unclassified.is_empty(),
        "these workspace crates have no release classification: {unclassified:?}\n\
         Add each to governance/release-verification.json with a class (A/B/C/D), \
         a `why`, and — for A and B — the commands that must be green before a \
         release claim. A new production or security crate must not be able to \
         arrive without one."
    );

    // The reverse direction too: a manifest entry for a crate that no longer
    // exists is a stale promise, and would quietly reduce coverage while
    // still looking complete.
    let ghosts: Vec<&String> = classified.difference(&members).collect();
    assert!(
        ghosts.is_empty(),
        "the release manifest names crates that are not workspace members: {ghosts:?}"
    );
}

#[test]
fn every_security_or_runtime_critical_crate_has_a_required_command() {
    let m = manifest();
    let mut missing = Vec::new();
    for (name, spec) in m["crates"].as_object().expect("crates") {
        let class = spec["class"].as_str().unwrap_or("?");
        if class != "A" && class != "B" {
            continue;
        }
        let required = spec["required"].as_array().map(|a| a.len()).unwrap_or(0);
        if required == 0 {
            missing.push(name.clone());
        }
    }
    assert!(
        missing.is_empty(),
        "these crates are classified security- or runtime-critical but list no \
         required verification command, so a release could claim them green \
         without running anything: {missing:?}"
    );
}

/// A class-C crate that opts out of host tests must say what DOES verify it,
/// rather than silently having no coverage at all.
#[test]
fn a_crate_with_no_required_command_names_what_verifies_it() {
    let m = manifest();
    let mut silent = Vec::new();
    for (name, spec) in m["crates"].as_object().expect("crates") {
        let required = spec["required"].as_array().map(|a| a.len()).unwrap_or(0);
        if required == 0 && spec.get("verified_by").and_then(|v| v.as_str()).is_none() {
            silent.push(name.clone());
        }
    }
    assert!(
        silent.is_empty(),
        "these crates require no command and name no alternative verification — \
         'not tested' must be a stated decision, not an absence: {silent:?}"
    );
}

/// No release-path script may launch a build or test whose exit status is
/// then discarded.
///
/// THE FAILURE THIS GUARDS, which happened in this repo's own release
/// workflow rather than in a script: a long `cargo test` was launched with
/// `nohup … &`, the launching shell exited 0, the child was killed partway
/// through its largest suite, and the partial tally (704 of 1542 tests) was
/// read as a pass. Two defects at once — a launcher's status standing in for
/// the job's, and a partial run counted as complete.
///
/// The scripts were already clean; this keeps them that way, because the
/// cheapest place for that pattern to reappear is a one-line "just run it in
/// the background" edit to a gate.
#[test]
fn release_path_scripts_never_discard_a_build_or_test_status() {
    let release_scripts = [
        "scripts/gate.sh",
        "scripts/claims_gate.sh",
        "scripts/run_managed.sh",
    ];
    let mut offenders = Vec::new();
    let mut scanned = 0usize;

    for rel in release_scripts {
        let path = root().join(rel);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("release script {rel} unreadable: {e}"));
        for (i, line) in text.lines().enumerate() {
            // Strip comments before judging: every current hit is prose ABOUT
            // this failure, and a scanner that flags its own documentation
            // gets switched off.
            let code = line.split('#').next().unwrap_or(line);
            if !code.contains("cargo ") {
                continue;
            }
            // A cargo command named inside a MESSAGE is not an invocation.
            // `claims_gate.sh` tells the operator "build it first: cargo build
            // …", and flagging that would make the scanner cry wolf on its own
            // help text — which is how a guard gets switched off.
            let emits_a_message = ["echo ", "printf ", "bad ", "die ", "fail ", "warn "]
                .iter()
                .any(|k| code.trim_start().starts_with(k));
            if emits_a_message {
                continue;
            }
            if !["test", "build", "clippy", "nextest"].iter().any(|v| {
                code.contains(&format!("cargo {v}")) || code.contains(&format!("cargo nextest {v}"))
            }) {
                continue;
            }
            scanned += 1;
            // Safe shapes: refused explicitly, continued onto the next line,
            // captured in a substitution, guarded by if/&&, or its pipeline
            // status preserved.
            let guarded = code.contains("|| fail")
                || code.contains("|| die")
                || code.trim_end().ends_with('\\')
                || code.contains("$(")
                || code.contains("PIPESTATUS")
                || code.trim_start().starts_with("if ")
                || code.contains("&&");
            if !guarded {
                offenders.push(format!("{rel}:{}: {}", i + 1, code.trim()));
            }
        }
    }

    assert!(
        scanned >= 10,
        "only {scanned} cargo invocations found across the release scripts — \
         the scanner's premise has drifted and this guard is vacuous"
    );
    assert!(
        offenders.is_empty(),
        "these release-path cargo invocations do not propagate failure, so the \
         gate could pass while they failed:\n  {}",
        offenders.join("\n  ")
    );
}

/// The inventory's own `gate_tests` field must match the rule the gate
/// actually implements, or the inventory is documentation that drifts.
///
/// Found by hand-maintaining it once: `axon-gfx-mock` is class C and has a
/// required command, and the field said the gate runs it. The gate stage
/// runs A and B only, so it does not.
#[test]
fn the_inventory_says_what_the_gate_actually_runs() {
    let m = manifest();
    let mut wrong = Vec::new();
    for (name, spec) in m["crates"].as_object().expect("crates") {
        let class = spec["class"].as_str().unwrap_or("?");
        let has_cmd = spec["required"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false);
        // The rule the gate stage implements: run the required commands of
        // class A and B crates.
        let gate_runs_it = has_cmd && (class == "A" || class == "B");
        let claimed = spec["gate_tests"].as_bool().unwrap_or(false);
        if claimed != gate_runs_it {
            wrong.push(format!(
                "{name}: says gate_tests={claimed}, gate runs it={gate_runs_it}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "the crate inventory claims coverage the gate does not provide (or \
         omits coverage it does):\n  {}",
        wrong.join("\n  ")
    );
}
