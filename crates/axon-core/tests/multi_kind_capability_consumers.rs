//! Every consumer of the capability tables must see a multi-kind builtin.
//!
//! `file_copy` reads one path and writes another, so it is classified by the
//! PER-ARGUMENT table (`classify_call_paths`) and NOT by `classify_call`,
//! which yields one kind per call. Three consumers asked the single-kind
//! function and silently saw nothing:
//!
//!   * the @[agent] action log — an @[agent] fn that read, copied and renamed
//!     a file produced ONE row (the read) while `moved.txt` existed on disk,
//!     in the log R4 Section 4.3 calls un-opt-out-able;
//!   * `collect_caps_expr`, hence `program_capabilities` — an imported module
//!     calling `write_file` is E1203 against a contained importer, while the
//!     same module calling `file_copy` (strictly MORE authority) passed with
//!     exit 0. That set also feeds the module audit verdict and R10's G2
//!     capability-monotonicity gate;
//!   * `cap_to_effect_row` matched `"fs"`, which `cap_label` never produces —
//!     the labels are `"fs:read"`/`"fs:write"` — so every filesystem agent
//!     action logged `effect_row:"Other"` next to `caps_used:"fs:read"`.
//!
//! Found by an adversarial review of the commit that fixed the FIRST two
//! consumers (the static value-ref check and the R28 ledger) and stopped
//! there.

use std::path::Path;
use std::process::Command;

fn axon() -> &'static str {
    env!("CARGO_BIN_EXE_axon")
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("axon_mk_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn an_agent_fn_logs_a_copy_and_a_rename_with_a_filesystem_effect_row() {
    let d = tmp("agent");
    std::fs::write(d.join("src.txt"), "data\n").unwrap();
    let prog = d.join("a.ax");
    std::fs::write(
        &prog,
        format!(
            "@[agent]\nfn act() -> i64 {{\n\
             \x20   let _ = read_file(\"{0}/src.txt\")\n\
             \x20   let _ = file_copy(\"{0}/src.txt\", \"{0}/dst.txt\")\n\
             \x20   let _ = file_rename(\"{0}/dst.txt\", \"{0}/moved.txt\")\n\
             \x20   0\n}}\nfn main() {{ let _ = act() }}\n",
            d.display()
        ),
    )
    .unwrap();
    let cache = d.join("cache");
    let out = Command::new(axon())
        .arg("run")
        .arg(&prog)
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "premise: the program must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    // Premise: the effect really happened. Without this the test would pass
    // on a build where the copy silently did nothing.
    assert!(
        d.join("moved.txt").exists(),
        "premise: the file must actually have been copied and renamed"
    );

    let log = std::fs::read_to_string(cache.join("axon").join("provenance.jsonl"))
        .expect("provenance written");
    let actions: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("\"event\":\"agent_action\""))
        .collect();
    for op in ["read_file", "file_copy", "file_rename"] {
        assert!(
            actions
                .iter()
                .any(|l| l.contains(&format!("\"action\":\"{op}\""))),
            "no agent_action row for `{op}` — an @[agent] fn touched the \
             filesystem with nothing in the log:\n{log}"
        );
    }
    assert!(
        !actions
            .iter()
            .any(|l| l.contains("\"effect_row\":\"Other\"")),
        "a filesystem agent action logged effect_row \"Other\":\n{}",
        actions.join("\n")
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The import-edge gate must see a copy as at least as much authority as a
/// write. `file_copy` is a read AND a write; `write_file` is a write.
#[test]
fn the_import_edge_gate_sees_a_copy_as_a_capability() {
    let d = tmp("e1203");
    let lib = d.join("lib");
    std::fs::create_dir_all(&lib).unwrap();

    for (name, call) in [
        ("evilwr", "write_file(a, b)"),
        ("evilcp", "file_copy(a, b)"),
        ("evilmv", "file_rename(a, b)"),
    ] {
        std::fs::write(
            lib.join(format!("{name}.ax")),
            format!("fn doit(a: str, b: str) -> i64 {{ let _ = {call}  0 }}\n"),
        )
        .unwrap();
        // The importer declares containment but never CALLS the import, so
        // the helper-follow check cannot fire and only the import edge can.
        let imp = d.join(format!("imp_{name}.ax"));
        std::fs::write(
            &imp,
            format!(
                "mod {name}\nuse {name}.{{doit}}\n\
                 @[contained(fs: [read(\"./\")], net: [], exec: none)]\n\
                 fn local_only() -> i64 {{ 0 }}\nfn main() {{ let _ = local_only() }}\n"
            ),
        )
        .unwrap();
        let out = Command::new(axon())
            .arg("check")
            .arg(&imp)
            .env("AXON_PATH", &lib)
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).to_string()
            + &String::from_utf8_lossy(&out.stderr);
        assert_ne!(
            out.status.code(),
            Some(0),
            "importing a module that calls `{call}` was permitted by a \
             contained importer that grants no write:\n{text}"
        );
        assert!(
            text.contains("E1203"),
            "refused, but not on the import edge — a refusal for another \
             reason is not this gate working:\n{text}"
        );
    }

    // CONTROL: a module that exercises nothing beyond the ceiling must still
    // import cleanly, or the gate is just refusing every import.
    std::fs::write(
        lib.join("benign.ax"),
        // Must TYPE-CHECK: read_file returns Result<str, str>, and a fixture
        // that fails to compile refuses for its own reason, not the gate's.
        "fn doit(a: str) -> i64 { let _ = read_file(a)  0 }\n",
    )
    .unwrap();
    let imp = d.join("imp_benign.ax");
    std::fs::write(
        &imp,
        "mod benign\nuse benign.{doit}\n\
         @[contained(fs: [read(\"./\")], net: [], exec: none)]\n\
         fn local_only() -> i64 { 0 }\nfn main() { let _ = local_only() }\n",
    )
    .unwrap();
    let out = Command::new(axon())
        .arg("check")
        .arg(&imp)
        .env("AXON_PATH", &lib)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "a within-ceiling import must still be permitted: {}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// `cap_to_effect_row` is not directly reachable from a test, so this pins the
/// property through the artifact that carries it: no agent action may report
/// the tag that means "unknown" for a capability the same row names.
#[test]
fn no_agent_action_reports_an_unknown_effect_row_for_a_known_capability() {
    let d = tmp("row");
    std::fs::write(d.join("src.txt"), "x\n").unwrap();
    let prog = d.join("a.ax");
    std::fs::write(
        &prog,
        format!(
            "@[agent]\nfn act() -> i64 {{ let _ = read_file(\"{}/src.txt\")  0 }}\n\
             fn main() {{ let _ = act() }}\n",
            d.display()
        ),
    )
    .unwrap();
    let cache = d.join("cache");
    let out = Command::new(axon())
        .arg("run")
        .arg(&prog)
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let log = std::fs::read_to_string(cache.join("axon").join("provenance.jsonl")).unwrap();
    let row = log
        .lines()
        .find(|l| l.contains("\"event\":\"agent_action\""))
        .expect("an agent_action row");
    assert!(row.contains("\"caps_used\":\"fs:read\""), "premise: {row}");
    assert!(
        row.contains("\"effect_row\":\"FS\""),
        "a row naming capability fs:read reported a non-FS effect row: {row}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

fn _unused(_: &Path) {}
