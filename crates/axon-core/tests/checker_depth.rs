//! Front-end depth and complexity regressions (compilebench AX-63, AX-64, AX-66),
//! end to end through the `axon` binary.
//!
//! Each program is generated and flat: no line nests deeper than a couple of
//! levels, but the checker's walks over it are as deep (or as long) as the
//! program is long. Every run has a deadline, so a regression to quadratic time
//! or memory fails here instead of hanging the suite: before the fixes a
//! 20,000-link `@[total]` chain took 272 s (debug), a 100,000-term `+` chain
//! exhausted memory, and a refinement cycle aborted the process with a stack
//! overflow.
//!
//! The wasm32 half (traps under wasmtime's default stack) is
//! `scripts/wasm_checker_depth_parity.sh`, run by `wasm_checker_depth_ax63` /
//! `wasm_checker_depth_ax66` in `cli_run.rs`.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a case may take. The slowest runs in well under 5 s in a debug
/// build; the quadratic/cubic shapes this guards take minutes to hours.
const DEADLINE: Duration = Duration::from_secs(90);

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// `axon <sub> <generated source>` with a deadline: a run past it is killed and
/// the test fails.
fn axon(sub: &str, name: &str, src: &str) -> Run {
    // One directory per case: the cases run in parallel threads of one process.
    let dir =
        std::env::temp_dir().join(format!("axon_checker_depth_{}_{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join(format!("{name}.ax"));
    std::fs::write(&file, src).unwrap();
    let out = std::fs::File::create(dir.join(format!("{name}.out"))).unwrap();
    let err = std::fs::File::create(dir.join(format!("{name}.err"))).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_axon"))
        .args([sub, file.to_str().unwrap()])
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err))
        .spawn()
        .unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if start.elapsed() > DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            panic!("`axon {sub} {name}` took longer than {DEADLINE:?}: quadratic (or worse) again");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let read = |ext: &str| {
        String::from_utf8_lossy(&std::fs::read(dir.join(format!("{name}.{ext}"))).unwrap())
            .into_owned()
    };
    let run = Run {
        code: status.code(),
        stdout: read("out"),
        stderr: read("err"),
    };
    let _ = std::fs::remove_dir_all(&dir);
    run
}

const MAIN: &str = "fn main() -> i64 { println(\"ok\") 0 }\n";

fn assert_clean(r: &Run, what: &str) {
    assert_eq!(
        (r.code, r.stderr.as_str()),
        (Some(0), ""),
        "{what}: {}",
        r.stdout
    );
}

// ── AX-64: operator chains ────────────────────────────────────────────────────

#[test]
fn plus_chain_of_100k_terms_checks_ax64() {
    // `1 + 1 + ... + 1`, 100,000 terms: the checker built a path string per
    // level (`O(N²)` bytes live, `O(N³)` time to build); this ran out of memory.
    let terms = vec!["1"; 100_000].join(" + ");
    let src =
        format!("fn main() -> i64 {{\n    let x = {terms}\n    println(to_str(x))\n    0\n}}\n");
    let r = axon("check", "plus100k", &src);
    assert_clean(&r, "100,000-term `+` chain");
}

#[test]
fn string_concat_chain_checks_ax64() {
    // The `+` on strings goes through the concatenation-chain walk, which also
    // derived a path per level.
    let terms = vec!["\"a\""; 30_000].join(" + ");
    let src = format!("fn main() -> i64 {{\n    let x = {terms}\n    println(x)\n    0\n}}\n");
    let r = axon("check", "concat30k", &src);
    assert_clean(&r, "30,000-term `str` chain");
}

#[test]
fn mixed_operator_chain_checks_ax64() {
    // Every binary-operator arm is on the spine: arithmetic, comparison, logic,
    // bitwise, in one left-nested chain 40,000 deep.
    let mut e = String::from("1");
    for i in 0..10_000 {
        e.push_str(match i % 4 {
            0 => " + 2 * 3",
            1 => " - 4",
            2 => " | 5",
            _ => " ^ 6",
        });
    }
    let src = format!(
        "fn main() -> i64 {{\n    let x = {e}\n    let _y = x > 1 && x < 99999999999\n    println(\"ok\")\n    0\n}}\n"
    );
    let r = axon("check", "mixed", &src);
    assert_clean(&r, "mixed operator chain");
}

#[test]
fn a_type_error_deep_in_a_chain_keeps_its_diagnostic_ax64() {
    // The diagnostic for the operand is the one the recursive walk gave: same
    // code, line, column and message, wherever in the chain the bad term is.
    let mut terms = vec!["1"; 3000];
    terms[1500] = "\"a\"";
    let src = format!(
        "fn main() -> i64 {{\n    let x = {}\n    println(to_str(x))\n    0\n}}\n",
        terms.join(" + ")
    );
    let r = axon("check", "badterm", &src);
    assert_eq!(r.code, Some(2), "{}{}", r.stdout, r.stderr);
    let diag = r.stderr.lines().next().unwrap_or_default();
    assert!(
        diag.contains("\"code\":\"E0301\"")
            && diag.contains("\"line\":2,\"col\":5")
            && diag.contains("arithmetic operand has non-numeric type str"),
        "{}",
        r.stderr
    );
}

#[test]
fn a_long_chain_still_runs_ax64() {
    // 3,000 terms through the whole pipeline, the interpreter included.
    let terms = vec!["1"; 3000].join(" + ");
    let src =
        format!("fn main() -> i64 {{\n    let x = {terms}\n    println(to_str(x))\n    0\n}}\n");
    let r = axon("run", "run3k", &src);
    assert_eq!(
        (r.code, r.stdout.as_str()),
        (Some(0), "3000\n"),
        "{}",
        r.stderr
    );
}

// ── AX-66: call-chain and refinement-chain walks ──────────────────────────────

fn chain(attr: &str, n: usize) -> String {
    let mut s = format!("{attr}fn f0() -> i64 {{ 0 }}\n");
    for i in 1..n {
        s.push_str(&format!("{attr}fn f{i}() -> i64 {{ f{}() }}\n", i - 1));
    }
    s
}

#[test]
fn contained_helper_chain_checks_ax66() {
    // `@[contained]` fn -> 20,000 helpers, each calling the one before: the
    // capability walk followed the chain by recursion (a wasm32 trap at 1,500).
    let src = format!(
        "{}@[contained]\nfn g() -> i64 {{ f19999() }}\n{MAIN}",
        chain("", 20_000)
    );
    let r = axon("check", "contained", &src);
    assert_clean(&r, "20,000-helper @[contained] chain");
}

#[test]
fn contained_helper_chain_still_refuses_a_forbidden_call_at_its_end_ax66() {
    let mut src = String::from("fn f0() -> str { read_file(\"/etc/passwd\") }\n");
    for i in 1..2000 {
        src.push_str(&format!("fn f{i}() -> str {{ f{}() }}\n", i - 1));
    }
    src.push_str("@[contained(fs: [], net: [], exec: none)]\nfn g() -> str { f1999() }\n");
    src.push_str(MAIN);
    let r = axon("check", "contained_bad", &src);
    assert_eq!(r.code, Some(2), "{}{}", r.stdout, r.stderr);
    let refusals: Vec<&str> = r
        .stderr
        .lines()
        .filter(|l| l.contains("\"code\":\"E1001\""))
        .collect();
    assert_eq!(refusals.len(), 1, "{}", r.stderr);
    assert!(
        refusals[0].contains("`read_file(\\\"/etc/passwd\\\")` is not permitted"),
        "{}",
        refusals[0]
    );
}

#[test]
fn total_chain_checks_ax66() {
    // `@[total]` chain of 20,000: the mutual-recursion probe walked the call
    // graph by recursion (a wasm32 trap at 3,000) and, once per fn, rebuilt its
    // visited set from the start: 272 s on this machine before the fix.
    let src = format!("{}{MAIN}", chain("@[total] ", 20_000));
    let r = axon("check", "total", &src);
    assert_clean(&r, "20,000-fn @[total] chain");
}

#[test]
fn total_ring_is_still_a_mutual_recursion_refusal_ax66() {
    let n = 1000;
    let mut src = String::new();
    for i in 0..n {
        src.push_str(&format!(
            "@[total] fn f{i}() -> i64 {{ f{}() }}\n",
            (i + 1) % n
        ));
    }
    src.push_str(MAIN);
    let r = axon("check", "ring", &src);
    assert_eq!(r.code, Some(2), "{}{}", r.stdout, r.stderr);
    let refused: Vec<&str> = r
        .stderr
        .lines()
        .filter(|l| l.contains("\"code\":\"E1208\""))
        .collect();
    assert_eq!(refused.len(), n, "every fn on the ring is refused");
    assert!(
        refused[0].contains("`@[total]` function `f0` is part of a mutual-recursion cycle"),
        "{}",
        refused[0]
    );
}

#[test]
fn refinement_chain_checks_and_runs_ax66() {
    // `type A{i} = A{i-1} where true`, 20,000 links, then a fn taking the last:
    // resolving the name recursed once per link (a wasm32 trap at 5,000).
    let mut src = String::from("type A0 = i64 where true\n");
    for i in 1..20_000 {
        src.push_str(&format!("type A{i} = A{} where true\n", i - 1));
    }
    src.push_str("fn h(x: A19999) -> i64 { 7 }\nfn main() -> i64 { println(to_str(h(3))) 0 }\n");
    let r = axon("run", "refine", &src);
    assert_eq!(
        (r.code, r.stdout.as_str()),
        (Some(0), "7\n"),
        "{}",
        r.stderr
    );
}

#[test]
fn refinement_cycle_is_refused_not_a_stack_overflow_ax66() {
    // `type A = B ...; type B = A ...` overflowed the native stack (SIGABRT, no
    // diagnostic). Each member of the cycle now says so, at its own `type`.
    let src = format!(
        "type A = B where true\ntype B = A where true\ntype S = S where true\nfn f(x: A) -> i64 {{ 1 }}\n{MAIN}"
    );
    let r = axon("check", "cycle", &src);
    assert_eq!(r.code, Some(2), "{}{}", r.stdout, r.stderr);
    let lines: Vec<&str> = r.stderr.lines().collect();
    assert_eq!(lines.len(), 3, "{}", r.stderr);
    for (line, want) in lines.iter().zip([
        "type 'A' is defined in terms of itself: A -> B -> A",
        "type 'B' is defined in terms of itself: B -> A -> B",
        "type 'S' is defined in terms of itself: S -> S",
    ]) {
        assert!(
            line.contains("\"code\":\"E0308\"") && line.contains(want),
            "{want}: {line}"
        );
    }
}

// ── AX-63: deep types on native ───────────────────────────────────────────────

#[test]
fn a_deep_inferred_type_checks_on_native_ax63() {
    // `let a{i} = [a{i-1}]`: `a{3000}` has a 3,000-level type from shallow
    // source. The wasm32 build refuses past 224 levels (E0301, see the parity
    // script); native keeps its large front-end stack and no limit.
    let mut src = String::from("fn f() -> i64 {\n    let a0 = [1]\n");
    for i in 1..3000 {
        src.push_str(&format!("    let a{i} = [a{}]\n", i - 1));
    }
    src.push_str("    7\n}\nfn main() -> i64 { println(to_str(f())) 0 }\n");
    let r = axon("run", "deeptype", &src);
    // `a0..a2998` are never read, so each draws W0006 (a warning, exit 0).
    assert_eq!(
        (r.code, r.stdout.as_str()),
        (Some(0), "7\n"),
        "{}",
        r.stderr
    );
    assert!(
        !r.stderr.contains("\"severity\":\"error\""),
        "{}",
        r.stderr.lines().next().unwrap_or_default()
    );
}
