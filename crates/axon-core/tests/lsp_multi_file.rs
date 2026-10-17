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
/// shut down; return every message the server wrote. The server runs in `cwd`.
fn session_in(
    cwd: &Path,
    axon_path: &Path,
    main_uri: &str,
    main_src: &str,
    extra: &[Value],
) -> Vec<Value> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_axon"))
        .arg("lsp")
        .current_dir(cwd)
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

fn session(axon_path: &Path, main_uri: &str, main_src: &str, extra: &[Value]) -> Vec<Value> {
    session_in(&std::env::temp_dir(), axon_path, main_uri, main_src, extra)
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

#[test]
fn a_relative_module_dir_still_yields_an_absolute_file_uri() {
    // `AXON_PATH=mods` (relative, the repo's usual `AXON_PATH=examples/stdlib`
    // style) stored `mods/lib.ax`, which became `file://mods/lib.ax`: host
    // `mods`, path `/lib.ax` — a jump to the wrong file.
    let dir = module_dir("rel");
    let parent = dir.parent().unwrap();
    let rel = Path::new(dir.file_name().unwrap());
    let main_uri = format!("file://{}/main.ax", dir.display()).replace(' ', "%20");
    let req = json!({"jsonrpc":"2.0","id":7,"method":"textDocument/definition",
        "params":{"textDocument":{"uri":main_uri},"position":{"line":5,"character":6}}});
    let msgs = session_in(parent, rel, &main_uri, MAIN, &[req]);
    let _ = std::fs::remove_dir_all(&dir);

    let resp = msgs
        .iter()
        .find(|m| m["id"] == 7)
        .expect("definition reply");
    assert_eq!(resp["result"]["uri"], lib_uri(&dir).as_str(), "{resp:#}");
    let publish = msgs
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .expect("diagnostics published");
    let rel_uri = &publish["params"]["diagnostics"][0]["relatedInformation"][0]["location"]["uri"];
    assert_eq!(rel_uri, lib_uri(&dir).as_str(), "{publish:#}");
}

/// A fresh module dir holding `files` (relative path, text); a relative path
/// with a `/` makes the nested directory.
fn modules(tag: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("axon lsp {tag} {}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    for (rel, text) in files {
        let f = dir.join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(f, text).unwrap();
    }
    dir
}

fn file_uri(p: &Path) -> String {
    format!("file://{}", p.display()).replace(' ', "%20")
}

/// Open `main` against the module dir; every diagnostic published must be the
/// imported module `module`'s (relatedInformation at its URI and line
/// `module_line`), anchored on main's line `use_line`, from column 0 to
/// `use_end` (the `use` declaration's closing brace).
fn assert_anchored(
    case: &str,
    dir: &Path,
    main: &str,
    module: &str,
    module_line: u64,
    use_line: u64,
    use_end: u64,
) {
    let main_uri = file_uri(&dir.join("main.ax"));
    let msgs = session(dir, &main_uri, main, &[]);
    let _ = std::fs::remove_dir_all(dir);

    let publish = msgs
        .iter()
        .find(|m| m["method"] == "textDocument/publishDiagnostics")
        .expect("diagnostics published");
    assert_eq!(publish["params"]["uri"], main_uri.as_str());
    let diags = publish["params"]["diagnostics"].as_array().unwrap();
    assert!(
        !diags.is_empty(),
        "{case}: {module}'s type error must be published"
    );
    for d in diags {
        // relatedInformation still names the module's own file and line.
        let rel = &d["relatedInformation"][0];
        assert_eq!(
            rel["location"]["uri"],
            file_uri(&dir.join(module)).as_str(),
            "{case}: only {module}'s diagnostics are published, each pointing into it: {diags:#?}"
        );
        assert_eq!(
            rel["location"]["range"]["start"]["line"], module_line,
            "{case}: {d:#}"
        );
        assert_eq!(
            d["range"]["start"]["line"], use_line,
            "MISANCHOR ({case}): {module}'s diagnostic is not on main.ax's line {use_line}: {d:#}"
        );
        assert_eq!(
            d["range"],
            json!({"start": {"line": use_line, "character": 0},
                   "end": {"line": use_line, "character": use_end}}),
            "{case}: the anchor spans the `use` declaration: {d:#}"
        );
    }
}

/// A type error on line 2 of the module that holds it.
const BROKEN_C: &str = "fn fc() -> i64 { 3 }\n// pad\nfn bad(a: i64) -> i64 { a + \"s\" }\n";

#[test]
fn a_transitive_module_anchors_on_the_use_that_reached_it() {
    // c.ax is reached only through b.ax; `use b` is main's SECOND `use`.
    let dir = modules(
        "transitive",
        &[
            ("a.ax", "fn fa() -> i64 { 1 }\n"),
            ("b.ax", "mod c\nuse c.{fc}\nfn fb() -> i64 { fc() }\n"),
            ("c.ax", BROKEN_C),
        ],
    );
    let main = "mod a\nmod b\nuse a.{fa}\nuse b.{fb}\nfn main() -> i64 { fa() + fb() }\n";
    assert_anchored("transitive", &dir, main, "c.ax", 2, 3, 10);
}

#[test]
fn a_use_inside_a_block_comment_is_never_the_anchor() {
    let dir = modules("comment", &[("lib.ax", BROKEN_C)]);
    let main = "mod lib\n/*\nuse lib.{fc}\n*/\nuse lib.{fc}\nfn main() -> i64 { fc() }\n";
    assert_anchored("block comment", &dir, main, "lib.ax", 2, 4, 12);
}

#[test]
fn a_module_path_that_ends_like_another_is_not_its_use() {
    // `util/lib.ax` ends with `/lib.ax`; only `use util.lib` imports it.
    let dir = modules(
        "suffix",
        &[
            ("lib.ax", "fn ok_one() -> i64 { 1 }\n"),
            ("util/lib.ax", BROKEN_C),
        ],
    );
    let main = "mod lib\nmod util\nuse lib.{ok_one}\nuse util.lib.{fc}\n\
                fn main() -> i64 { ok_one() + fc() }\n";
    assert_anchored("path suffix", &dir, main, "util/lib.ax", 2, 3, 17);
}

#[test]
fn a_module_the_entry_imports_directly_anchors_on_its_own_use() {
    // The loader merges c.ax while loading `use a` (a.ax imports it first);
    // the entry's own `use c` is still where c.ax's diagnostic belongs.
    let dir = modules(
        "direct",
        &[
            ("a.ax", "use c.{fc}\nfn fa() -> i64 { fc() }\n"),
            ("c.ax", BROKEN_C),
        ],
    );
    let main = "mod a\nmod c\nuse a.{fa}\nuse c.{fc}\nfn main() -> i64 { fa() + fc() }\n";
    assert_anchored("direct import", &dir, main, "c.ax", 2, 3, 10);
}
