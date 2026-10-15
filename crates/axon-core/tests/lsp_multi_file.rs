//! `axon lsp` with imported modules: locations must name the file they are in.
//!
//! Every span used to be converted against the OPEN document's text, so an
//! error in an imported `lib.ax` was reported at a clamped, meaningless spot in
//! `main.ax`, and go-to-definition of an imported function jumped to the wrong
//! file. These drive the real server over stdio, with `AXON_PATH` naming a
//! module directory, exactly as an editor would.

#![cfg(feature = "serde-json")]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

fn frame(v: &Value) -> Vec<u8> {
    let body = serde_json::to_string(v).unwrap();
    format!("Content-Length: {}\r\n\r\n{}", body.len(), body).into_bytes()
}

fn parse_frames(mut raw: &[u8]) -> Vec<Value> {
    let mut out = Vec::new();
    while let Some(hdr_end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
        let hdr = std::str::from_utf8(&raw[..hdr_end]).unwrap();
        let len: usize = hdr
            .lines()
            .find_map(|l| l.strip_prefix("Content-Length: "))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        let body = &raw[hdr_end + 4..hdr_end + 4 + len];
        out.push(serde_json::from_slice(body).unwrap());
        raw = &raw[hdr_end + 4 + len..];
    }
    out
}

/// Run one session: open `main_uri` with `main_src`, send `extra` requests,
/// shut down; return every message the server wrote.
fn session(axon_path: &Path, main_uri: &str, main_src: &str, extra: &[Value]) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_axon"))
        .arg("lsp")
        .env("AXON_PATH", axon_path)
        .env("AXON_PATH_EXCLUSIVE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn axon lsp");
    let mut input = Vec::new();
    input.extend(frame(
        &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}),
    ));
    input.extend(frame(&json!({"jsonrpc":"2.0","method":"textDocument/didOpen",
        "params":{"textDocument":{"uri":main_uri,"languageId":"axon","version":1,"text":main_src}}})));
    for e in extra {
        input.extend(frame(e));
    }
    input.extend(frame(&json!({"jsonrpc":"2.0","id":99,"method":"shutdown"})));
    input.extend(frame(&json!({"jsonrpc":"2.0","method":"exit"})));
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&input).unwrap();
    drop(stdin);
    let mut raw = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut raw).unwrap();
    child.wait().unwrap();
    parse_frames(&raw)
}

/// A module dir whose name needs percent-encoding, holding a `lib.ax` with a
/// TYPE error on line 4 (parse errors carry no span) and `ok_one` on line 0.
fn module_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("axon lsp {tag} {}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("lib.ax"),
        "fn ok_one() -> i64 { 1 }\n// pad 1\n// pad 2\n// pad 3\n\
         fn broken(a: i64) -> i64 { a + \"not a number\" }\n",
    )
    .unwrap();
    std::fs::write(dir.join("other.ax"), "fn two() -> i64 { 2 }\n").unwrap();
    dir
}

/// `lib` is the SECOND import, so its `use` is line 3, not merely the first one.
const MAIN: &str = "mod other\nmod lib\nuse other.{two}\nuse lib.{ok_one}\n\
                    fn main() -> i64 {\n    ok_one() + two()\n}\n";

fn lib_uri(dir: &Path) -> String {
    // Spaces are the only byte in these paths that needs escaping.
    format!(
        "file://{}",
        dir.join("lib.ax").display().to_string().replace(' ', "%20")
    )
}

#[test]
fn imported_module_error_lands_on_the_use_line_with_related_location() {
    let dir = module_dir("diag");
    let main_uri = format!("file://{}/main.ax", dir.display()).replace(' ', "%20");
    let msgs = session(&dir, &main_uri, MAIN, &[]);
    let _ = std::fs::remove_dir_all(&dir);

    let publish = msgs
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .expect("diagnostics published");
    assert_eq!(publish["params"]["uri"], main_uri.as_str());
    let diags = publish["params"]["diagnostics"].as_array().unwrap();
    let foreign: Vec<&Value> = diags
        .iter()
        .filter(|d| d["relatedInformation"].is_array())
        .collect();
    assert!(
        !foreign.is_empty(),
        "lib.ax's type error must arrive with relatedInformation: {diags:#?}"
    );
    for d in &foreign {
        assert_eq!(
            d["range"]["start"]["line"], 3,
            "reported on main.ax's `use` line: {d:#}"
        );
        let rel = &d["relatedInformation"][0];
        assert_eq!(rel["location"]["uri"], lib_uri(&dir).as_str(), "{d:#}");
        assert_eq!(
            rel["location"]["range"]["start"]["line"], 4,
            "related location is lib.ax's own line: {d:#}"
        );
        assert!(!rel["message"].as_str().unwrap_or("").is_empty());
    }
    // No diagnostic may sit anywhere in main.ax except where it belongs: this
    // program is otherwise clean.
    assert_eq!(foreign.len(), diags.len(), "{diags:#?}");
}

#[test]
fn definition_of_an_imported_fn_jumps_into_its_own_file() {
    let dir = module_dir("def");
    let main_uri = format!("file://{}/main.ax", dir.display()).replace(' ', "%20");
    let req = json!({"jsonrpc":"2.0","id":7,"method":"textDocument/definition",
        "params":{"textDocument":{"uri":main_uri},"position":{"line":5,"character":6}}});
    let msgs = session(&dir, &main_uri, MAIN, &[req]);
    let _ = std::fs::remove_dir_all(&dir);

    let resp = msgs
        .iter()
        .find(|m| m["id"] == 7)
        .expect("definition reply");
    let loc = &resp["result"];
    assert_eq!(loc["uri"], lib_uri(&dir).as_str(), "{resp:#}");
    assert_eq!(loc["range"]["start"]["line"], 0, "{resp:#}");
    assert_eq!(loc["range"]["start"]["character"], 0, "{resp:#}");
}
