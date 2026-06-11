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
    run_json(axon_bin, &["redteam", "--json", &tmp])
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
