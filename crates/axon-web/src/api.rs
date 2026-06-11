use std::fs;
use std::process::Command;

pub fn intent_compile(body: &str, axon_bin: &str) -> String {
    let req = parse_content(body);
    let ext = if req.content.trim_start().starts_with('#') { "md" } else { "ax" };
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
    let trace_json: serde_json::Value = match Command::new(axon_bin).args(["trace", "--json"]).output() {
        Ok(o) => {
            let s = String::from_utf8_lossy(&o.stdout).into_owned();
            serde_json::from_str(&s).unwrap_or(serde_json::Value::Array(vec![]))
        }
        Err(_) => serde_json::Value::Array(vec![]),
    };
    // Keep only entries with multiple evals (active adaptive fns).
    let trajectory: Vec<serde_json::Value> = if let serde_json::Value::Array(entries) = &trace_json {
        entries.iter()
            .filter(|e| e["evals"].as_u64().unwrap_or(0) > 1)
            .cloned()
            .collect()
    } else {
        vec![]
    };
    // Filter run stderr: skip warnings/run-id, keep meaningful output.
    let run_message: String = run_stderr.lines()
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
    }).to_string()
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
    run_json(axon_bin, &args)
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
            let json_line = stdout.lines().rev()
                .find(|l| l.trim_start().starts_with('{')
                      && serde_json::from_str::<serde_json::Value>(l).is_ok());
            // Prose = everything except the JSON line itself.
            let prose: String = stdout.lines()
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
                            obj.insert("run_output".to_string(), serde_json::Value::String(prose.clone()));
                            // Surface prominent status lines as "message" for easy UI display.
                            let msg: String = prose.lines()
                                .filter(|l| l.contains("FAILED") || l.contains("CAUGHT") || l.contains("BLOCKED") || l.contains("passed"))
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
            }).to_string()
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

fn err_json(msg: &str) -> String {
    serde_json::json!({"error": msg}).to_string()
}

fn write_temp(content: &str, ext: &str) -> String {
    let dir = std::env::temp_dir();
    let name = format!(
        "axon_web_{}.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0),
        ext
    );
    let path = dir.join(name);
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
        ContentReq { content: body.to_string() }
    }
}

fn parse_deploy(body: &str) -> DeployReq {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(body) {
        let content = v["content"].as_str().unwrap_or(body).to_string();
        let risk = v["risk"].as_str().map(|s| s.to_string());
        DeployReq { content, risk }
    } else {
        DeployReq { content: body.to_string(), risk: None }
    }
}
