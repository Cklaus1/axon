//! The durable store is a filesystem effect and must be treated as one.
//!
//! `dstore_open` / `dstore_apply` / `dstore_clear` read, append to and delete
//! a log under the cache directory using `std::fs` directly. They appeared in
//! NEITHER `classify_call` nor `builtin_effect_row`, so every mechanism that
//! asks those tables what a call costs was told: nothing. Reproduced, all four
//! at once:
//!
//!   * `@[contained(fs: [], net: [], exec: none)]` — `axon check` exit 0, and
//!     the run wrote `$XDG_CACHE_HOME/axon/stores/pwned.ndjson`;
//!   * `AXON_ALLOWED_EFFECTS=Pure` — exit 0, same file written;
//!   * `sandbox_create_scoped(p, "IO", "", "", "")` (fs_write deny-all) —
//!     exit 0, same file written;
//!   * `AXON_RECORD` — journal 0 lines, so a run that wrote a file was
//!     indistinguishable from one that touched nothing.
//!
//! The two table-driven guards could not catch it: one asks "does a builtin
//! with a Net/Exec ROW get classified", the other "does a CLASSIFIED builtin
//! declare a row". A builtin missing from both is invisible to each.

use std::path::Path;
use std::process::Command;

fn axon() -> &'static str {
    env!("CARGO_BIN_EXE_axon")
}

fn write(dir: &Path, name: &str, src: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, src).unwrap();
    p
}

fn adaptive_rows(cache: &std::path::Path) -> usize {
    std::fs::read_to_string(cache.join("axon").join("provenance.jsonl"))
        .map(|t| t.matches("\"event\":\"adaptive_return\"").count())
        .unwrap_or(0)
}

fn tmp(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("axon_dstore_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn wrote(cache: &Path, key: &str) -> bool {
    cache
        .join("axon")
        .join("stores")
        .join(format!("{key}.ndjson"))
        .exists()
}

const CONTAINED: &str = r#"
@[contained(fs: [], net: [], exec: none)]
fn touch_disk() -> i64 {
    let h = dstore_open("k1", 0)
    dstore_apply(h, 1, 7)
}
fn main() { let _ = touch_disk() }
"#;

const PLAIN: &str = r#"
fn main() {
    let h = dstore_open("k2", 0)
    let _ = dstore_apply(h, 1, 7)
}
"#;

#[test]
fn contained_refuses_the_durable_store_statically() {
    let d = tmp("contained");
    let f = write(&d, "c.ax", CONTAINED);
    let out = Command::new(axon()).arg("check").arg(&f).output().unwrap();
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert_ne!(out.status.code(), Some(0), "check accepted it:\n{text}");
    assert!(text.contains("E1001"), "wrong diagnostic:\n{text}");
    // The message must name the builtin the author wrote. Classifying the
    // store KEY as a path made this say `read_file("k1")` and advise
    // `fs: [read("k1")]` — a grant naming one thing while permitting a write
    // to `$XDG_CACHE_HOME/axon/stores/k1.ndjson`, and one that cannot be
    // satisfied anyway because `dstore_apply`'s first argument is a handle.
    assert!(
        text.contains("dstore_open"),
        "the diagnostic names the wrong builtin:\n{text}"
    );
    assert!(
        !text.contains("read_file(\"k1\")"),
        "the diagnostic still reports a store key as a path:\n{text}"
    );
}

/// `is_impure_builtin` is a THIRD table, and it disagreed with the other two:
/// `@[pure]` accepted a function that wrote to disk (`axon check` exit 0, the
/// .ndjson written). The same table gates refinement-predicate purity, so a
/// `where` clause could have called it too. Found by the existing
/// cross-table test, not by me.
#[test]
fn a_pure_function_may_not_use_the_durable_store() {
    let d = tmp("pure");
    let f = write(
        &d,
        "pure.ax",
        r#"
@[pure]
fn p() -> i64 {
    let h = dstore_open("k5", 0)
    dstore_apply(h, 1, 7)
}
fn main() { println(to_str(p())) }
"#,
    );
    let cache = d.join("cache");
    let out = Command::new(axon())
        .arg("check")
        .arg(&f)
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stderr).to_string() + &String::from_utf8_lossy(&out.stdout);
    assert_ne!(
        out.status.code(),
        Some(0),
        "@[pure] accepted a disk write:\n{text}"
    );
    assert!(text.contains("E1207"), "wrong diagnostic:\n{text}");
    assert!(!wrote(&cache, "k5"));
}

#[test]
fn an_ambient_pure_ceiling_refuses_the_durable_store() {
    let d = tmp("ceiling");
    let f = write(&d, "p.ax", PLAIN);
    let cache = d.join("cache");
    let out = Command::new(axon())
        .arg("run")
        .arg(&f)
        .env("XDG_CACHE_HOME", &cache)
        .env("AXON_ALLOWED_EFFECTS", "Pure")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(8), "expected a sandbox violation");
    assert!(
        !wrote(&cache, "k2"),
        "a Pure ceiling permitted a disk write"
    );
}

#[test]
fn a_scoped_sandbox_checks_the_real_store_directory() {
    // DENY: the store carries no path argument, so the per-argument scope loop
    // saw nothing to check and the call fell through to "no restriction".
    let d = tmp("scoped");
    let deny = write(
        &d,
        "deny.ax",
        r#"
fn inner(x: i64) -> i64 {
    let h = dstore_open("k3", 0)
    dstore_apply(h, 1, x)
}
fn main() {
    let p = principal_root("p", false, false, false, 100)
    let s = sandbox_create_scoped(p, "IO", "", "", "")
    println(to_str(sandbox_run(s, "inner", 3)))
}
"#,
    );
    let cache = d.join("deny_cache");
    let out = Command::new(axon())
        .arg("run")
        .arg(&deny)
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(8),
        "fs_write deny-all permitted a write"
    );
    assert!(!wrote(&cache, "k3"));

    // ALLOW: the same program under a scope that DOES grant the directory must
    // still work, or the fix is "refuse everything" rather than "enforce".
    let allow = write(
        &d,
        "allow.ax",
        r#"
fn inner(x: i64) -> i64 {
    let h = dstore_open("k4", 0)
    dstore_apply(h, 1, x)
}
fn main() {
    let p = principal_root("p", false, true, false, 100)
    let s = sandbox_create_scoped(p, "IO", "*", "*", "")
    println(to_str(sandbox_run(s, "inner", 3)))
}
"#,
    );
    let cache2 = d.join("allow_cache");
    let out2 = Command::new(axon())
        .arg("run")
        .arg(&allow)
        .env("XDG_CACHE_HOME", &cache2)
        .output()
        .unwrap();
    assert_eq!(
        out2.status.code(),
        Some(0),
        "a granting scope must still permit the store: {}",
        String::from_utf8_lossy(&out2.stderr)
    );
    assert!(wrote(&cache2, "k4"));
}

#[test]
fn the_durable_store_refuses_to_run_under_replay_and_says_so_under_record() {
    let d = tmp("journal");
    let f = write(&d, "p.ax", PLAIN);
    let j = d.join("run.journal");

    // RECORD: the run proceeds — recording is observation — but the journal
    // cannot represent these calls, and an incomplete journal that says
    // nothing is a recording that lies by omission.
    let rec = Command::new(axon())
        .arg("run")
        .arg(&f)
        .env("XDG_CACHE_HOME", d.join("rc"))
        .env("AXON_RECORD", &j)
        .output()
        .unwrap();
    assert_eq!(rec.status.code(), Some(0));
    let rec_err = String::from_utf8_lossy(&rec.stderr);
    assert!(
        rec_err.contains("INCOMPLETE"),
        "an unrecordable write was recorded silently:\n{rec_err}"
    );

    // REPLAY: must refuse. Serving the rest of the run from a journal while
    // these calls read live state is the exact hazard replay removes.
    let cache = d.join("rp");
    let rep = Command::new(axon())
        .arg("run")
        .arg(&f)
        .env("XDG_CACHE_HOME", &cache)
        .env("AXON_REPLAY", &j)
        .output()
        .unwrap();
    assert_ne!(rep.status.code(), Some(0), "replay consulted live state");
    assert!(
        String::from_utf8_lossy(&rep.stderr).contains("AXON_REPLAY"),
        "refusal does not say why: {}",
        String::from_utf8_lossy(&rep.stderr)
    );
    assert!(
        !wrote(&cache, "k2"),
        "a replayed run wrote to the live store"
    );
}

// ── The goal_* exemption's justification, pinned ────────────────────────────

/// `goal_*` is EXEMPT_IO because its filesystem write goes to a path the
/// program cannot address. That is the whole argument for exempting it rather
/// than classifying it, so it is a test, not a comment.
///
/// MEASURED, and stated plainly because this IS a gap, not a non-issue: a
/// `@[contained(fs: [], net: [], exec: none)]` fn calling `goal_run` exits 0
/// and appends ~20 program-derived lines to the provenance log; a scoped
/// sandbox with `fs_write = ""` does not stop it either. It is exempted
/// because classifying `goal_*` as `fs:write` would refuse every contained
/// optimiser — a policy change — and because the path is fixed. If the path
/// ever becomes program-influenced, that argument collapses and this fails.
#[test]
fn the_provenance_path_is_not_program_addressable() {
    let d = tmp("prov");
    let cache = d.join("cache");

    // Two programs whose only difference is a string an attacker would
    // control if the path were derived from program input.
    let run = |name: &str, key: &str| -> Vec<String> {
        let prog = d.join(format!("{name}.ax"));
        std::fs::write(
            &prog,
            format!(
                "@[adaptive]\nfn {key}(x: f64) -> f64 {{ 0.0 - (x - 7.0) * (x - 7.0) }}\n\
                 fn main() {{ let _ = goal_run(\"{key}\", 0.0, 5) }}\n"
            ),
        )
        .unwrap();
        let out = Command::new(axon())
            .arg("run")
            .arg(&prog)
            .env("XDG_CACHE_HOME", cache.join(name))
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "premise: the optimiser must run: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        let root = cache.join(name);
        let mut found = Vec::new();
        let mut stack = vec![root.clone()];
        while let Some(p) = stack.pop() {
            if let Ok(rd) = std::fs::read_dir(&p) {
                for e in rd.flatten() {
                    let q = e.path();
                    if q.is_dir() {
                        stack.push(q);
                    } else {
                        found.push(q.strip_prefix(&root).unwrap().to_string_lossy().to_string());
                    }
                }
            }
        }
        found.sort();
        found
    };

    let a = run("a", "score");
    let b = run("b", "zzzz_other_name");
    assert!(
        !a.is_empty(),
        "premise: the optimiser must write something, or this proves nothing"
    );
    assert_eq!(
        a, b,
        "the set of files written changed with a program-chosen name, so the \
         provenance path IS program-addressable and the goal_* EXEMPT_IO \
         entry's justification no longer holds"
    );
    assert!(
        a.iter().any(|f| f.ends_with("provenance.jsonl")),
        "expected the provenance log among {a:?}"
    );

    let _ = std::fs::remove_dir_all(&d);
}

/// A program must not gain an otherwise-forbidden durable write by reaching
/// the provenance machinery.
///
/// REPRODUCED before the fix — each of these wrote a program-derived row to
/// `$XDG_CACHE_HOME/axon/provenance.jsonl` and exited 0: a direct
/// `@[adaptive]` call under `@[contained(fs: [], net: [], exec: none)]`; the
/// same under `AXON_ALLOWED_EFFECTS=Pure`; and inside
/// `sandbox_run(sandbox_create(p, ""), …)`, whose ceiling is deny-all.
/// `goal_run` amplified it: `max_evals <= 0` means unlimited, so a contained
/// function could drive an unbounded number of rows whose `score`, `input`
/// and `payload` it chose — a general durable store reached through the audit
/// machinery.
///
/// The write is not a builtin call, so it never passed `pre_effect_gate`
/// where the ceiling is enforced. It now asks the same question through the
/// same predicate, and SUPPRESSES the write rather than refusing the call:
/// refusing would abort every contained optimiser, and the optimiser does not
/// need the write — `goal_run` reads the in-memory store, which is untouched.
/// That is verified by the companion test below, without which "suppress"
/// could not be told from "break".
#[test]
fn a_restricted_ceiling_suppresses_the_provenance_write_on_every_path() {
    let d = tmp("prov_ceiling");
    let adaptive = "@[adaptive]\nfn metric(x: i64) -> i64 { 10 - (x - 3) * (x - 3) }\n";

    let cases: &[(&str, &str)] = &[
        ("direct", "fn main() { let _ = metric(3) }\n"),
        (
            "nested",
            "fn wrapper(x: i64) -> i64 { metric(x) }\nfn main() { let _ = wrapper(3) }\n",
        ),
        (
            "contained",
            "@[contained(fs: [], net: [], exec: none)]\n\
             fn boxed(x: i64) -> i64 { metric(x) }\nfn main() { let _ = boxed(3) }\n",
        ),
    ];

    for (name, body) in cases {
        let prog = d.join(format!("{name}.ax"));
        std::fs::write(&prog, format!("{adaptive}{body}")).unwrap();

        // PREMISE: with no ceiling the row IS written. Without this the
        // suppression assertion below could pass against a build that never
        // writes provenance at all.
        let open_cache = d.join(format!("{name}_open"));
        let out = Command::new(axon())
            .arg("run")
            .arg(&prog)
            .env("XDG_CACHE_HOME", &open_cache)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(0), "{name}: baseline run failed");
        assert!(
            adaptive_rows(&open_cache) >= 1,
            "{name}: premise — an unrestricted run must write a provenance row"
        );

        // Under a ceiling that grants no IO, nothing is persisted, and the
        // program still completes: suppression, not refusal.
        let shut_cache = d.join(format!("{name}_shut"));
        let out = Command::new(axon())
            .arg("run")
            .arg(&prog)
            .env("XDG_CACHE_HOME", &shut_cache)
            .env("AXON_ALLOWED_EFFECTS", "Pure")
            .output()
            .unwrap();
        assert_eq!(
            out.status.code(),
            Some(0),
            "{name}: the program must still run — the remedy is suppression, not \
             refusal: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(
            adaptive_rows(&shut_cache),
            0,
            "{name}: a durable provenance row was written under a ceiling that \
             grants no filesystem effect"
        );
    }

    // The sharpest case: an explicit deny-all scoped sandbox.
    let prog = d.join("sbx.ax");
    std::fs::write(
        &prog,
        format!(
            "{adaptive}fn main() {{\n    \
             let p = principal_root(\"p\", false, false, false, 100)\n    \
             let s = sandbox_create(p, \"\")\n    \
             let _ = sandbox_run(s, \"metric\", 3)\n}}\n"
        ),
    )
    .unwrap();
    let cache = d.join("sbx_cache");
    let out = Command::new(axon())
        .arg("run")
        .arg(&prog)
        .env("XDG_CACHE_HOME", &cache)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "sandbox_run case did not complete"
    );
    assert_eq!(
        adaptive_rows(&cache),
        0,
        "a deny-all sandbox still persisted a provenance row"
    );

    let _ = std::fs::remove_dir_all(&d);
}

/// Suppression must not break the optimiser, or the fix is just "stop
/// working under a ceiling".
///
/// `goal_run`/`goal_best_score` read the IN-MEMORY store, which the
/// suppression does not touch — only the durable append is gated. With `IO`
/// granted the rows persist AND the best score is right; the point of the
/// pairing is that the optimiser's answer never depended on the file.
#[test]
fn suppressing_the_durable_write_does_not_break_optimisation() {
    let d = tmp("prov_optim");
    let prog = d.join("m.ax");
    std::fs::write(
        &prog,
        "@[adaptive]\nfn metric(x: i64) -> i64 { 10 - (x - 3) * (x - 3) }\n\
         fn main() {\n    let _ = metric(1)\n    let _ = metric(3)\n    let _ = metric(5)\n    \
         eprintln(\"best=\" + to_str(goal_best_score(\"metric\", 100.0)))\n}\n",
    )
    .unwrap();

    for (label, effects) in [("no ceiling", None), ("IO granted", Some("Pure,IO"))] {
        let cache = d.join(label.replace(' ', "_"));
        let mut c = Command::new(axon());
        c.arg("run").arg(&prog).env("XDG_CACHE_HOME", &cache);
        if let Some(e) = effects {
            c.env("AXON_ALLOWED_EFFECTS", e);
        }
        let out = c.output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(0), "{label}: run failed: {err}");
        assert!(
            err.contains("best=10"),
            "{label}: the optimiser must still find the best score: {err}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// The OTHER provenance writers are on the same ceiling.
///
/// The first pass guarded only the `@[adaptive]` writer. A disproof review
/// found two more into the same file: `append_agent_action_jsonl`, emitted at
/// the TOP of `pre_effect_gate` before the ceiling is consulted, and
/// `append_ai_call_jsonl`. MEASURED under `AXON_ALLOWED_EFFECTS=Net,AI` (IO
/// denied, so `write_file` is refused): an `@[agent]` fn looping 8 times over
/// `ai_complete` produced 8 `agent_action` plus 8 `ai_call` rows — 16 durable
/// program-derived lines, with an attacker-chosen function name and a
/// SHA-256 of an attacker-chosen prompt, in a run that had no filesystem
/// effect at all. Loop count arbitrary, so the volume was unbounded.
///
/// The cost of closing it is stated plainly: a run denied filesystem effects
/// now leaves NO provenance audit trail, because persisting one IS the
/// capability it was denied. Auditing a sandboxed run requires granting the
/// runtime a channel it may write.
#[test]
fn every_provenance_writer_honours_the_ceiling() {
    let d = tmp("prov_writers");
    let prog = d.join("leak.ax");
    std::fs::write(
        &prog,
        "@[agent]\nfn LEAK_ROW(i: i64) -> i64 {\n    \
         let _ = ai_complete(\"payload-\" + to_str(i))\n    i\n}\n\
         fn main() {\n    let i = 0\n    while i < 8 {\n        \
         let _ = LEAK_ROW(i)\n        i = i + 1\n    }\n}\n",
    )
    .unwrap();

    let rows = |cache: &std::path::Path, event: &str| -> usize {
        std::fs::read_to_string(cache.join("axon").join("provenance.jsonl"))
            .map(|t| t.matches(&format!("\"event\":\"{event}\"")).count())
            .unwrap_or(0)
    };

    // PREMISE: with IO granted, both writers fire. Without this the
    // suppression below could pass against a build that never writes at all.
    let open_cache = d.join("granted");
    let out = Command::new(axon())
        .arg("run")
        .arg(&prog)
        .env("XDG_CACHE_HOME", &open_cache)
        .env("AXON_AI_MOCK", "1")
        .env("AXON_ALLOWED_EFFECTS", "Net,AI,IO")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "premise run failed: {}",
               String::from_utf8_lossy(&out.stderr));
    assert!(rows(&open_cache, "agent_action") >= 8, "premise: agent_action rows");
    assert!(rows(&open_cache, "ai_call") >= 8, "premise: ai_call rows");

    // IO denied: neither writer may persist.
    let shut_cache = d.join("denied");
    let out = Command::new(axon())
        .arg("run")
        .arg(&prog)
        .env("XDG_CACHE_HOME", &shut_cache)
        .env("AXON_AI_MOCK", "1")
        .env("AXON_ALLOWED_EFFECTS", "Net,AI")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0), "the program must still run");
    assert_eq!(
        rows(&shut_cache, "agent_action"),
        0,
        "agent_action rows were persisted under a ceiling granting no IO"
    );
    assert_eq!(
        rows(&shut_cache, "ai_call"),
        0,
        "ai_call rows were persisted under a ceiling granting no IO"
    );

    // Only `run_start` may remain: it is written at the CLI boundary before
    // the program executes, carries a generated run-id, the seed and the
    // operator's chosen path, and is one row per process — no program-chosen
    // content and no volume to amplify. It is the replay handle, so guarding
    // it would cost the run its identity for nothing.
    let total = std::fs::read_to_string(shut_cache.join("axon").join("provenance.jsonl"))
        .map(|t| t.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);
    assert_eq!(
        total,
        rows(&shut_cache, "run_start"),
        "something other than run_start was persisted under a denied ceiling"
    );
    let _ = std::fs::remove_dir_all(&d);
}
