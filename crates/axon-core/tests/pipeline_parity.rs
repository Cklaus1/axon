//! `lib::check_pipeline` and the CLI's `run_check_pipeline_located` are two
//! implementations of the same check. The CLI one carries a note saying they
//! "must stay in sync"; nothing enforced it, and on warnings they were not.
//!
//! `lib::check_pipeline` is what the browser playground (`axon-wasm`) renders,
//! so a diagnostic present only in the CLI is one a playground user never sees
//! on source that the CLI rejects. Measured over the cases below, four were
//! missing: W0006 (unused binding), W0002 (shadowing), W0003 (user fn shadows a
//! builtin) and W2001 (vague `@[goal]`) — because `check_pipeline` read
//! `resolve_result.errors` and never `.warnings`.
use std::process::Command;

fn cli_codes(src: &str, path: &std::path::Path) -> Vec<String> {
    std::fs::write(path, src).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_axon"))
        .args(["check", path.to_str().unwrap()])
        .output()
        .unwrap();
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    let mut v: Vec<String> = all
        .lines()
        .filter_map(|l| {
            let i = l.find("\"code\":\"")? + 8;
            let rest = &l[i..];
            let j = rest.find('"')?;
            Some(rest[..j].to_string())
        })
        .collect();
    v.sort();
    v.dedup();
    v
}

fn lib_codes(src: &str) -> Vec<String> {
    let mut v: Vec<String> = axon_core::check_pipeline(src, "probe.ax")
        .into_iter()
        .map(|d| d.code)
        .collect();
    v.sort();
    v.dedup();
    v
}

#[test]
fn both_check_pipelines_report_the_same_diagnostics() {
    let dir = std::env::temp_dir().join(format!("axon_pipediv_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("t.ax");
    let cases: &[(&str, &str)] = &[
        ("floor div", "fn main() { let x = 7 // 2\nprintln(to_str(x)) }\n"),
        ("mut", "fn main() { let mut c = 0\nprintln(to_str(c)) }\n"),
        ("unused", "fn main() { let x = 1\nprintln(\"a\") }\n"),
        ("unknown name", "fn main() { println(to_str(y)) }\n"),
        ("unknown type", "fn g(x: int) -> i64 { 1 }\nfn main() { println(to_str(1)) }\n"),
        ("not false", "fn main() { if not false { println(\"a\") } }\n"),
        ("hash comment", "fn main() { # hi\nprintln(\"x\") }\n"),
        ("bad arity", "fn main() { assert(true, \"m\") }\n"),
        ("contained net", "@[contained(fs: [], net: [], exec: none)]\nfn g() -> i64 { let _r = http_get(\"http://evil.com\", \"\") 0 }\nfn main() { println(to_str(g())) }\n"),
        ("ret mismatch", "fn g() -> i64 { \"s\" }\nfn main() { println(to_str(g())) }\n"),
        ("div by zero", "fn main() { let b = 1 / 0\nprintln(to_str(b)) }\n"),
        ("shadow builtin", "fn len(x: i64) -> i64 { x }\nfn main() { println(to_str(len(1))) }\n"),
        ("shadow local", "fn main() { let x = 1\nlet x = 2\nprintln(to_str(x)) }\n"),
        ("unreachable arm", "fn main() { let v = match 1 { 1 => 10  1 => 20  _ => 0 }\nprintln(to_str(v)) }\n"),
        ("unreachable code", "fn g() -> i64 { return 1\nprintln(\"never\")\n2 }\nfn main() { println(to_str(g())) }\n"),
        ("dropped Result", "fn main() { parse_int(\"1\")\nprintln(\"a\") }\n"),
        ("deferred attr", "@[agent]\nfn g() -> i64 { 1 }\nfn main() { println(to_str(g())) }\n"),
        ("vague goal", "@[goal(\"max\")]\nfn g() -> i64 { 1 }\nfn main() { println(to_str(g())) }\n"),
        ("ai no policy", "fn g() -> i64 { match ai_complete(\"x\") { Ok(r) => str_len(r)  Err(_) => 0 } }\nfn main() { println(to_str(g())) }\n"),
    ];
    let mut diffs = Vec::new();
    let mut saw_any = false;
    for (label, src) in cases {
        let c = cli_codes(src, &p);
        let l = lib_codes(src);
        saw_any |= !c.is_empty();
        if c != l {
            diffs.push(format!("{label}: cli={c:?} lib={l:?}"));
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
    // A run where the CLI reported nothing at all would "agree" with a library
    // pipeline that also reported nothing, and prove neither works.
    assert!(
        saw_any,
        "precondition: these programs must produce diagnostics at all"
    );
    assert!(
        diffs.is_empty(),
        "the library pipeline (what the browser playground shows) and `axon \
         check` disagree:\n  {}",
        diffs.join("\n  ")
    );
}
