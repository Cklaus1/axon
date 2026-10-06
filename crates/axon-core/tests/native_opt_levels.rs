//! Native optimisation levels and `--emit-obj` (AX-17, AX-21, AX-22, AX-23).
//!
//! `axon build` runs LLVM's `default<On>` IR pipeline for the selected level
//! (`--opt-level 0|1|2|3|s|z`, `--release` = 2), gives every program function
//! except `main` internal linkage, and `--emit-obj` writes the program object
//! without linking. Optimisation must never change what a program prints: the
//! interpreter is the reference.
//!
//! Every test returns early when the binary was built without the `codegen`
//! feature (the gate's `--no-default-features` stage), like the native tests in
//! `cli_run.rs`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn axon() -> Command {
    Command::new(env!("CARGO_BIN_EXE_axon"))
}

fn codegen_absent(out: &Output) -> bool {
    let all = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    all.contains("requires building axon with the `codegen` feature")
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("axon_optlvl_{}_{name}", std::process::id()))
}

fn write_src(name: &str, src: &str) -> PathBuf {
    let f = tmp(&format!("{name}.ax"));
    std::fs::write(&f, src).expect("write temp source");
    f
}

fn build(args: &[&str], src: &Path, out: &Path) -> Output {
    axon()
        .arg("build")
        .args(args)
        .arg(src)
        .arg("-o")
        .arg(out)
        .output()
        .expect("spawn axon build")
}

fn stderr_of(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

/// ELF `e_type` of `path` (1 = ET_REL relocatable, 2 = ET_EXEC executable).
fn elf_type(path: &Path) -> u16 {
    let bytes = std::fs::read(path).expect("read output");
    assert!(
        bytes.len() > 18 && &bytes[..4] == b"\x7fELF",
        "{} is not an ELF file",
        path.display()
    );
    u16::from_le_bytes([bytes[16], bytes[17]])
}

const FIB: &str = "fn fib(n: i64) -> i64 { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }\n\nfn main() -> i64 {\n    println(to_str(fib(20)))\n    0\n}\n";

/// Recursion, a float escape-time loop, an array fold through a closure,
/// string building, struct fields and an f64 printed through the runtime's
/// libm-backed formatter — enough surface that a pass rewriting it wrongly
/// shows up in the output.
const MIXED: &str = r#"type P = { x: i64, y: f64 }

fn fib(n: i64) -> i64 { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }

fn mandel(w: i64, h: i64, max_iter: i64) -> i64 {
    let total = 0
    for py in 0..h {
        let ci = -1.2 + (2.4 * i64_to_f64(py)) / i64_to_f64(h)
        for px in 0..w {
            let cr = -2.0 + (3.0 * i64_to_f64(px)) / i64_to_f64(w)
            let zr = 0.0
            let zi = 0.0
            let i = 0
            while i < max_iter && zr * zr + zi * zi <= 4.0 {
                let t = zr * zr - zi * zi + cr
                zi = 2.0 * zr * zi + ci
                zr = t
                i = i + 1
            }
            total = total + i
        }
    }
    total
}

fn main() -> i64 {
    println(to_str(fib(22)))
    println(to_str(mandel(60, 40, 50)))
    let xs = arr_range(0, 100)
    println(to_str(arr_fold(xs, 0, |acc: i64, x: i64| acc + x * x)))
    let s = "a"
    for i in 0..5 {
        s = s + to_str(i)
    }
    println(s)
    let p = P { x: 7, y: 2.5 }
    println(to_str(i64_to_f64(p.x) * p.y))
    0
}
"#;

#[test]
fn every_opt_level_prints_what_the_interpreter_prints() {
    let src = write_src("mixed", MIXED);
    let interp = axon().arg("run").arg(&src).output().expect("spawn run");
    assert_eq!(
        interp.status.code(),
        Some(0),
        "`axon run` must work: {}",
        stderr_of(&interp)
    );
    let want = String::from_utf8_lossy(&interp.stdout).to_string();
    assert!(!want.trim().is_empty(), "interpreter printed nothing");

    let levels: [&[&str]; 7] = [
        &[],
        &["--release"],
        &["--opt-level", "0"],
        &["--opt-level", "1"],
        &["--opt-level", "3"],
        &["--opt-level", "s"],
        &["--opt-level", "z"],
    ];
    for args in levels {
        let bin = tmp(&format!("mixed_bin_{}", args.join("").replace('-', "")));
        let _ = std::fs::remove_file(&bin);
        let mut full = vec!["--no-cache"];
        full.extend_from_slice(args);
        let b = build(&full, &src, &bin);
        if codegen_absent(&b) {
            let _ = std::fs::remove_file(&src);
            return;
        }
        assert_eq!(
            b.status.code(),
            Some(0),
            "build {args:?} failed:\n{}",
            stderr_of(&b)
        );
        let run = Command::new(&bin).output().expect("run native");
        let _ = std::fs::remove_file(&bin);
        assert_eq!(run.status.code(), Some(0), "native {args:?} exit status");
        assert_eq!(
            String::from_utf8_lossy(&run.stdout),
            want,
            "native build {args:?} disagrees with the interpreter"
        );
    }
    let _ = std::fs::remove_file(&src);
}

#[test]
fn release_ir_is_optimised_and_program_functions_are_internal() {
    let src = write_src("fib_ir", FIB);
    let o2 = tmp("fib_o2.ll");
    let o0 = tmp("fib_o0.ll");
    let b2 = build(&["--release", "--no-cache", "--emit-llvm"], &src, &o2);
    if codegen_absent(&b2) {
        let _ = std::fs::remove_file(&src);
        return;
    }
    let b0 = build(&["--no-cache", "--emit-llvm"], &src, &o0);
    let _ = std::fs::remove_file(&src);
    assert_eq!(b2.status.code(), Some(0), "{}", stderr_of(&b2));
    assert_eq!(b0.status.code(), Some(0), "{}", stderr_of(&b0));
    let ir2 = std::fs::read_to_string(&o2).expect("O2 IR");
    let ir0 = std::fs::read_to_string(&o0).expect("O0 IR");
    let _ = std::fs::remove_file(&o2);
    let _ = std::fs::remove_file(&o0);

    let fib_body = |ir: &str| -> String {
        let start = ir
            .lines()
            .position(|l| l.starts_with("define") && l.contains("@fib("))
            .unwrap_or_else(|| panic!("no fib definition in IR:\n{ir}"));
        ir.lines()
            .skip(start)
            .take_while(|l| *l != "}")
            .collect::<Vec<_>>()
            .join("\n")
    };
    let fib2 = fib_body(&ir2);
    let fib0 = fib_body(&ir0);
    // AX-21: O0 spills the parameter to a stack slot; the O2 pipeline
    // (mem2reg/SROA) removes it.
    assert!(fib0.contains("alloca"), "O0 fib should be unoptimised:\n{fib0}");
    assert!(
        !fib2.contains("alloca"),
        "--release must run the IR pipeline (no allocas left in fib):\n{fib2}"
    );
    // AX-22: non-`main` functions are internal (direct, dso_local calls);
    // `main` stays external for the C runtime.
    assert!(
        fib2.lines().next().unwrap().starts_with("define internal"),
        "fib must have internal linkage:\n{fib2}"
    );
    assert!(
        ir2.lines()
            .any(|l| l.starts_with("define") && !l.contains("internal") && l.contains("@main(")),
        "main must keep external linkage:\n{ir2}"
    );
    // Builtin helpers nobody calls are gone once they are internal.
    assert!(
        !ir2.contains("@str_reverse("),
        "unused internal helpers must be dropped at O2"
    );
}

#[test]
fn invalid_opt_level_is_rejected() {
    let src = write_src("badlvl", FIB);
    let bin = tmp("badlvl_bin");
    let b = build(&["--no-cache", "--opt-level", "4"], &src, &bin);
    let _ = std::fs::remove_file(&src);
    let _ = std::fs::remove_file(&bin);
    assert_ne!(b.status.code(), Some(0), "--opt-level 4 must be refused");
    let msg = stderr_of(&b);
    assert!(
        msg.contains("--opt-level") && msg.contains("'4'"),
        "the refusal must name the flag and value:\n{msg}"
    );
}

#[test]
fn hosted_emit_obj_writes_a_relocatable_object_not_a_binary() {
    let src = write_src("emitobj", FIB);
    let obj = tmp("emitobj.o");
    let _ = std::fs::remove_file(&obj);
    let b = build(&["--release", "--no-cache", "--emit-obj"], &src, &obj);
    if codegen_absent(&b) {
        let _ = std::fs::remove_file(&src);
        return;
    }
    assert_eq!(b.status.code(), Some(0), "{}", stderr_of(&b));
    assert_eq!(
        elf_type(&obj),
        1,
        "--emit-obj must write an ELF relocatable (ET_REL), not a linked binary"
    );
    let _ = std::fs::remove_file(&obj);

    // A warm cache must not turn `--emit-obj` / `--emit-llvm` back into a link:
    // populate the cache with a normal build, then ask for the object and IR.
    let cache = tmp("emitobj_cache");
    let _ = std::fs::remove_dir_all(&cache);
    let bin = tmp("emitobj_bin");
    let cache_arg = cache.to_string_lossy().to_string();
    let warm = build(&["--release", "--cache-dir", &cache_arg], &src, &bin);
    assert_eq!(warm.status.code(), Some(0), "{}", stderr_of(&warm));
    assert_eq!(elf_type(&bin), 2, "a normal build links an executable");
    let _ = std::fs::remove_file(&bin);

    let cached_obj = build(
        &["--release", "--cache-dir", &cache_arg, "--emit-obj"],
        &src,
        &obj,
    );
    assert_eq!(cached_obj.status.code(), Some(0), "{}", stderr_of(&cached_obj));
    assert_eq!(elf_type(&obj), 1, "--emit-obj with a warm cache");
    let _ = std::fs::remove_file(&obj);

    let ll = tmp("emitobj_cached.ll");
    let cached_ir = build(
        &["--release", "--cache-dir", &cache_arg, "--emit-llvm"],
        &src,
        &ll,
    );
    assert_eq!(cached_ir.status.code(), Some(0), "{}", stderr_of(&cached_ir));
    let text = std::fs::read_to_string(&ll).expect("--emit-llvm with a warm cache writes IR text");
    assert!(text.contains("define"), "not LLVM IR:\n{text}");
    let _ = std::fs::remove_file(&ll);
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_file(&src);
}
