use std::fs;
use std::process::Command;

pub fn intent_compile(body: &str, axon_bin: &str) -> String {
    let req = parse_content(body);
    let ext = if req.content.trim_start().starts_with('#') {
        "md"
    } else {
        "ax"
    };
    let tmp = write_temp(&req.content, ext);
    run_json(axon_bin, &["intent", "compile", "--json", &tmp])
}

pub fn ast_review(body: &str, axon_bin: &str) -> String {
    let req = parse_content(body);
    let tmp = write_temp(&req.content, "ax");
    run_json(axon_bin, &["ast", "review", "--json", &tmp])
}

pub fn ast_approve(body: &str, axon_bin: &str) -> String {
    let req = parse_content(body);
    let tmp = write_temp(&req.content, "ax");
    let out = Command::new(axon_bin)
        .args(["ast", "approve", &tmp])
        .output();
    match out {
        Ok(o) => {
            let approved_path = format!("{tmp}.approved");
            serde_json::json!({
                "ok": o.status.success(),
                "approved_path": approved_path,
                "stdout": String::from_utf8_lossy(&o.stdout),
                "stderr": String::from_utf8_lossy(&o.stderr),
            })
            .to_string()
        }
        Err(e) => err_json(&e.to_string()),
    }
}

pub fn redteam(body: &str, axon_bin: &str) -> String {
    let req = parse_content(body);
    let tmp = write_temp(&req.content, "ax");
    run_json_merged(axon_bin, &["redteam", "--json", &tmp])
}

pub fn goal_improve(body: &str, axon_bin: &str) -> String {
    let req = parse_content(body);
    let tmp = write_temp(&req.content, "ax");
    // Run the goal (executes goal_run internally).
    let run_out = Command::new(axon_bin).args(["run", &tmp]).output();
    let (run_ok, run_stdout, run_stderr) = match run_out {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(e) => (false, String::new(), e.to_string()),
    };
    // Fetch score trajectory after the run.
    let trace_json: serde_json::Value =
        match Command::new(axon_bin).args(["trace", "--json"]).output() {
            Ok(o) => {
                let s = String::from_utf8_lossy(&o.stdout).into_owned();
                serde_json::from_str(&s).unwrap_or(serde_json::Value::Array(vec![]))
            }
            Err(_) => serde_json::Value::Array(vec![]),
        };
    // Keep only entries with multiple evals (active adaptive fns).
    let trajectory: Vec<serde_json::Value> = if let serde_json::Value::Array(entries) = &trace_json
    {
        entries
            .iter()
            .filter(|e| e["evals"].as_u64().unwrap_or(0) > 1)
            .cloned()
            .collect()
    } else {
        vec![]
    };
    // Filter run stderr: skip warnings/run-id, keep meaningful output.
    let run_message: String = run_stderr
        .lines()
        .filter(|l| !l.starts_with("warning:") && !l.contains("run-id "))
        .collect::<Vec<_>>()
        .join("\n");
    let best_score = extract_best_score(&run_stdout);
    serde_json::json!({
        "schema": "axon-goal-improve/1",
        "ok": run_ok,
        "run_output": run_stdout,
        "run_message": run_message,
        "best_score": best_score,
        "trajectory": trajectory,
    })
    .to_string()
}

pub fn deploy(body: &str, axon_bin: &str) -> String {
    let req = parse_deploy(body);
    let tmp = write_temp(&req.content, "ax");
    let mut args = vec!["deploy", "--json"];
    if let Some(ref r) = req.risk {
        args.push("--risk");
        args.push(r.as_str());
    }
    args.push(&tmp);
    // AUDIT T50 (P4-PROD-09). This used `run_json`, which parses stdout as ONE
    // JSON document and falls back to a `{ok, exit_code, stdout, stderr}`
    // wrapper otherwise. A deployed program prints its own output before the
    // report, so the fallback was the normal case and `status` / `approved` /
    // `failed_reason` never appeared at the top level — leaving the UI nothing
    // to gate on but `ok`, which is true whenever the CLI ran at all.
    //
    // `run_json_merged` (already used by /api/redteam) lifts the report object
    // and keeps the prose as `run_output`, so the schema's own fields are where
    // the caller expects them.
    run_json_merged(axon_bin, &args)
}

pub fn trace(axon_bin: &str) -> String {
    run_json(axon_bin, &["trace", "--json"])
}

fn extract_best_score(stdout: &str) -> Option<i64> {
    for line in stdout.lines() {
        if line.starts_with("best score:") {
            if let Some(n_str) = line.split_whitespace().nth(2) {
                if let Ok(n) = n_str.parse::<i64>() {
                    return Some(n);
                }
            }
        }
    }
    None
}

// Like run_json but also surfaces the prose output as "message" and "run_output" fields.
// Commands like `axon redteam --json` write prose + JSON to stdout; this splits them.
fn run_json_merged(axon_bin: &str, args: &[&str]) -> String {
    match Command::new(axon_bin).args(args).output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            // Find the last line that is valid JSON (the structured report).
            let json_line = stdout.lines().rev().find(|l| {
                l.trim_start().starts_with('{')
                    && serde_json::from_str::<serde_json::Value>(l).is_ok()
            });
            // Prose = everything except the JSON line itself.
            let prose: String = stdout
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    !(t.starts_with('{') && serde_json::from_str::<serde_json::Value>(t).is_ok())
                })
                .collect::<Vec<_>>()
                .join("\n");
            if let Some(json_str) = json_line {
                if let Ok(mut v) = serde_json::from_str::<serde_json::Value>(json_str) {
                    if let Some(obj) = v.as_object_mut() {
                        if !prose.is_empty() {
                            obj.insert(
                                "run_output".to_string(),
                                serde_json::Value::String(prose.clone()),
                            );
                            // Surface prominent status lines as "message" for easy UI display.
                            let msg: String = prose
                                .lines()
                                .filter(|l| {
                                    l.contains("FAILED")
                                        || l.contains("CAUGHT")
                                        || l.contains("BLOCKED")
                                        || l.contains("passed")
                                })
                                .collect::<Vec<_>>()
                                .join("\n");
                            if !msg.is_empty() {
                                obj.insert("message".to_string(), serde_json::Value::String(msg));
                            }
                        }
                    }
                    return v.to_string();
                }
            }
            // Fallback: no JSON line found.
            serde_json::json!({
                "ok": out.status.success(),
                "exit_code": out.status.code(),
                "stdout": stdout,
            })
            .to_string()
        }
        Err(e) => err_json(&e.to_string()),
    }
}

fn run_json(axon_bin: &str, args: &[&str]) -> String {
    match Command::new(axon_bin).args(args).output() {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
            if serde_json::from_str::<serde_json::Value>(&stdout).is_ok() {
                stdout
            } else {
                serde_json::json!({
                    "ok": out.status.success(),
                    "exit_code": out.status.code(),
                    "stdout": stdout,
                    "stderr": stderr,
                })
                .to_string()
            }
        }
        Err(e) => err_json(&e.to_string()),
    }
}

// ── Safety API (R26 attestation, R27 kill-switch, R28 ledger) ────────────────

/// POST /api/safety/attest — run `axon-vm attest` or return a mock report.
/// Mock mode activates when AXON_CI_NO_KVM=1 or the kernel image is absent.
pub fn safety_attest() -> String {
    let kernel_path = "dist/guest/vmlinuz";
    let use_mock = std::env::var("AXON_CI_NO_KVM").as_deref() == Ok("1")
        || !std::path::Path::new(kernel_path).exists();

    if use_mock {
        let ts_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        return serde_json::json!({
            "ok": true,
            "schema": "axon-vm-attest/1",
            "mode": "mock",
            "attested": true,
            "kernel": "mock",
            "hash": "sha256:mock0000000000000000000000000000000000000000000000000000000000000000",
            "report": {"pcr0": "0000000000000000", "pcr1": "0000000000000000"},
            "timestamp_secs": ts_secs,
        })
        .to_string();
    }

    match Command::new("axon-vm")
        .args(["attest", "--kernel", kernel_path])
        .output()
    {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
            if serde_json::from_str::<serde_json::Value>(&stdout).is_ok() {
                stdout
            } else {
                serde_json::json!({
                    "ok": out.status.success(),
                    "stdout": stdout,
                    "stderr": String::from_utf8_lossy(&out.stderr),
                })
                .to_string()
            }
        }
        Err(e) => err_json(&e.to_string()),
    }
}

/// POST /api/safety/kill — write the R27 kill-file for a running job.
/// Body: {"run_id": "..."}  (run_id defaults to "current" when omitted).
/// Safe to call even if no job is running — just writes the file.
pub fn safety_kill(body: &str) -> String {
    let run_id = if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        v["run_id"].as_str().unwrap_or("current").to_string()
    } else {
        "current".to_string()
    };

    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let runs_dir = std::path::Path::new(&home).join(".axon").join("runs");
    if let Err(e) = fs::create_dir_all(&runs_dir) {
        return serde_json::json!({"ok": false, "error": e.to_string()}).to_string();
    }
    // Trip EVERY latch that exists for this run, and SAY whether one did.
    //
    // This wrote `<run_id>.kill` and returned `latch: "tripped"` unconditionally
    // — the same defect AUDIT T26 fixed in `axon-os kill`, which used to write
    // one path by naming convention and "printed the tripped banner and exited
    // 0, leaving the operator believing a run had been killed when it had not".
    //
    // Two ways that happens here. A run started with `--killable --monitor` has
    // `<run_id>.monitor.kill` and NO `<run_id>.kill` (axon-os cli.rs requires
    // `monitor_effects.is_none()` for the latter), so writing only the first
    // misses the live one. And `axon-os run` defaults `--out` to `.`, while R32
    // names `~/.axon/runs/<run_id>.kill` as the canonical latch — so a
    // supervisor started without `--out ~/.axon/runs` is not polling this
    // directory at all.
    //
    // The endpoint cannot know the supervisor's `--out`, so it does not pretend
    // to: `armed` reports whether a latch for this run already existed HERE.
    // `armed: false` means the write is a pre-arm that a not-yet-started run
    // will pick up — not a confirmed kill.
    let candidates = [
        runs_dir.join(format!("{run_id}.kill")),
        runs_dir.join(format!("{run_id}.monitor.kill")),
    ];
    let existing: Vec<&std::path::PathBuf> = candidates.iter().filter(|p| p.exists()).collect();
    let armed = !existing.is_empty();
    let targets: Vec<&std::path::PathBuf> = if armed {
        existing
    } else {
        vec![&candidates[0]]
    };

    let kill_content = r#"{"latch":"tripped","reason":"operator shutdown"}"#;
    let mut written: Vec<String> = Vec::new();
    for t in &targets {
        if let Err(e) = fs::write(t, kill_content) {
            return serde_json::json!({"ok": false, "error": e.to_string()}).to_string();
        }
        written.push(t.to_string_lossy().to_string());
    }

    serde_json::json!({
        "ok": true,
        "run_id": run_id,
        "kill_file": written[0],
        "kill_files": written,
        "latch": "tripped",
        "armed": armed,
        "note": if armed {
            "tripped an existing latch for this run"
        } else {
            "no latch existed here — wrote the canonical R32 path, which a run              armed at it will pick up. A supervisor started without              `--out ~/.axon/runs` polls a different directory."
        },
        "reason": "operator shutdown",
    })
    .to_string()
}

/// GET /api/safety/ledger — read the R28 audit ledger (last 10 entries).
/// Returns graceful fallback when R28 is not yet available.
pub fn safety_ledger() -> String {
    let ledger_path = std::env::var("AXON_AUDIT_LEDGER").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
        format!("{home}/.axon/runs/last_run.ledger.jsonl")
    });

    let path = std::path::Path::new(&ledger_path);
    if !path.exists() {
        // "no ledger at this path", NOT "the feature is unavailable". This said
        // "R28 not available", which blames the audit subsystem for what is
        // usually just a run that has not written a ledger yet — and sends a
        // reader looking for a missing feature instead of a missing file.
        return serde_json::json!({
            "ok": false,
            "reason": "no ledger file at this path",
            "ledger_path": ledger_path,
            "entries": [],
        })
        .to_string();
    }

    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => return serde_json::json!({"ok": false, "error": e.to_string()}).to_string(),
    };

    // Parse JSONL, take last 10 entries — and COUNT what will not parse.
    //
    // This was `filter_map(|l| from_str(l).ok())`, which silently dropped any
    // line it could not read. On an AUDIT ledger that is the wrong failure: a
    // truncated write or a tampered line rendered as a clean, shorter list, and
    // the only trace was a gap in the `seq` numbers that a reader had to notice
    // unaided. Measured on a 4-line ledger with one corrupt line, the endpoint
    // returned `ok: true` and entries 1, 2, 4.
    //
    // Unreadable lines are still excluded from `entries` — half a record is not
    // evidence — but the COUNT is reported, so "this ledger has a hole in it" is
    // something the caller is told rather than something it must infer.
    let non_empty: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    let total_lines = non_empty.len();
    let parsed: Vec<serde_json::Value> = non_empty
        .iter()
        .filter_map(|l| serde_json::from_str(l).ok())
        .collect();
    let unreadable = total_lines - parsed.len();
    let entries: Vec<serde_json::Value> = parsed
        .into_iter()
        .rev()
        .take(10)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();

    serde_json::json!({
        // `ok` stays true — the ledger was read — but it is no longer the whole
        // story, so the counts travel with it. A caller that wants to refuse a
        // holed ledger can; one that does not is at least not misled.
        "ok": true,
        "entries": entries,
        "ledger_path": ledger_path,
        "total_lines": total_lines,
        "unreadable_lines": unreadable,
    })
    .to_string()
}

/// GET /api/safety/status — aggregate safety status across all three layers.
/// Returns {ok, attested, killable, ledger_ok, coalition_ok, coalition_principals, coalition_max}.
pub fn safety_status() -> String {
    let attest_val: serde_json::Value =
        serde_json::from_str(&safety_attest()).unwrap_or(serde_json::Value::Null);
    let attested = attest_val["attested"]
        .as_bool()
        .or_else(|| attest_val["ok"].as_bool())
        .unwrap_or(false);
    // CARRY THE QUALIFIER. `safety_attest` enters MOCK when no guest kernel
    // image exists — file absence, not only an explicit opt-in — and returns
    // attested:true with a literal digest. Dropping `mode` here left the
    // aggregate with no field at all that distinguishes a measured kernel from
    // a stubbed one, and the UI then printed "live" from its own fallback.
    let attest_mode = attest_val["mode"].as_str().unwrap_or("unreported").to_string();

    let ledger_val: serde_json::Value =
        serde_json::from_str(&safety_ledger()).unwrap_or(serde_json::Value::Null);
    let ledger_ok = ledger_val["ok"].as_bool().unwrap_or(false);

    // WHAT THIS ACTUALLY ESTABLISHES: that the run store is writable. It is
    // NOT "this job can be stopped" — nothing here consults a latch, a
    // supervisor, or the `<run_id>.kill.ptr` channel that `axon-os kill`
    // resolves. The field is named for what it measures.
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    let runs_dir = std::path::Path::new(&home).join(".axon").join("runs");
    let run_store_writable = runs_dir.exists() || fs::create_dir_all(&runs_dir).is_ok();

    // The comment said ".kill files"; the code counted EVERY directory entry —
    // .json records, .axjob copies, .audit.jsonl, .kill.ptr — so a store with
    // 12 run artifacts rendered as "12 / 3 principals". Count what was meant.
    let kill_files = if runs_dir.exists() {
        fs::read_dir(&runs_dir)
            .map(|d| {
                d.filter_map(|e| e.ok())
                    .filter(|e| e.file_name().to_string_lossy().ends_with(".kill"))
                    .count()
            })
            .unwrap_or(0)
    } else {
        0
    };

    serde_json::json!({
        "ok": true,
        "attested": attested,
        // Never dropped again: a consumer can now tell a measured attestation
        // from a mocked one without reading the attest endpoint separately.
        "attest_mode": attest_mode,
        "run_store_writable": run_store_writable,
        "ledger_ok": ledger_ok,
        // `coalition_ok` was the literal `true`. Nothing computed it, and the
        // dashboard rendered a green tick for an axis that had never been
        // evaluated. Null means UNEVALUATED, which is the honest answer until
        // something measures it — a tick that is always shown carries no
        // information, and a reader cannot tell it from one that was earned.
        "coalition_ok": serde_json::Value::Null,
        "coalition_note": "not evaluated — no coalition-bound check is wired to \
                           this endpoint",
        "coalition_kill_files": kill_files,
        "coalition_max": 3,
    })
    .to_string()
}

fn err_json(msg: &str) -> String {
    serde_json::json!({"error": msg}).to_string()
}

/// Stage `content` at a stable, CONTENT-ADDRESSED temp path.
///
/// AUDIT T50 (finding P4-PROD-10). This used to name the file from the current
/// clock (`subsec_nanos()`), so every request minted a NEW path. `ast approve`
/// therefore wrote `<tmp-A>.approved` while `deploy` ran `<tmp-B>` and looked
/// for `<tmp-B>.approved`, which never existed. Reproduced against the running
/// server:
///
/// ```text
/// POST /api/ast/approve -> "approved_path": "/tmp/axon_web_400484501.ax.approved"
/// POST /api/deploy      -> "path": "/tmp/axon_web_416046457.ax", "approved": false
/// ```
///
/// Every approval a user clicked was silently discarded, and the deploy pane
/// still reported success — the UI's sign-off step was decorative.
///
/// Hashing the content fixes it without any session plumbing, and gives exactly
/// the security semantics wanted: the same program text resolves to the same
/// path (so its approval is found), while text edited after approval resolves
/// elsewhere (so it is NOT approved). That is the same property `axon ast
/// approve` enforces internally — T10 made the approval bind the program text
/// rather than the filename — so the two now agree instead of merely coexisting.
#[cfg(test)]
pub(crate) fn stage_for_test(content: &str, ext: &str) -> String {
    write_temp(content, ext)
}

fn write_temp(content: &str, ext: &str) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(content.as_bytes());
    // 32 hex chars (128 bits) keeps the filename readable and is far beyond any
    // accidental collision for a scratch directory.
    let hex = format!("{digest:x}");
    let path = std::env::temp_dir().join(format!("axon_web_{}.{}", &hex[..32], ext));
    fs::write(&path, content).ok();
    path.to_string_lossy().into_owned()
}

struct ContentReq {
    content: String,
}

struct DeployReq {
    content: String,
    risk: Option<String>,
}

fn parse_content(body: &str) -> ContentReq {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let content = v["content"].as_str().unwrap_or(body).to_string();
        ContentReq { content }
    } else {
        ContentReq {
            content: body.to_string(),
        }
    }
}

fn parse_deploy(body: &str) -> DeployReq {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let content = v["content"].as_str().unwrap_or(body).to_string();
        let risk = v["risk"].as_str().map(|s| s.to_string());
        DeployReq { content, risk }
    } else {
        DeployReq {
            content: body.to_string(),
            risk: None,
        }
    }
}
