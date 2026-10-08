//! Native optimisation levels and `--emit-obj` (AX-17, AX-21, AX-22, AX-23,
//! AX-37).
//!
//! `axon build` runs LLVM's `default<On>` IR pipeline for the selected level
//! (`--opt-level 0|1|2|3|s|z`, `--release` = 2; `0` runs only `globaldce`),
//! gives every program function except `main` internal linkage, and
//! `--emit-obj` writes the program object without linking. Optimisation must
//! never change what a program prints: the interpreter is the reference.
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

/// One ELF64 little-endian symbol: (name, is a function, is defined here).
fn elf_symbols(path: &Path) -> Vec<(String, bool, bool)> {
    let b = std::fs::read(path).expect("read object");
    let u16_at = |o: usize| u16::from_le_bytes(b[o..o + 2].try_into().unwrap()) as usize;
    let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
    let u64_at = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap()) as usize;
    assert_eq!(b[4], 2, "{} is not ELF64", path.display());
    let (shoff, shentsize, shnum) = (u64_at(0x28), u16_at(0x3a), u16_at(0x3c));
    let section = |i: usize| shoff + i * shentsize;
    let symtab = (0..shnum)
        .map(section)
        .find(|&s| u32_at(s + 4) == 2) // SHT_SYMTAB
        .unwrap_or_else(|| panic!("{} has no symbol table", path.display()));
    let strtab = u64_at(section(u32_at(symtab + 0x28)) + 0x18);
    let (off, size, entsize) = (
        u64_at(symtab + 0x18),
        u64_at(symtab + 0x20),
        u64_at(symtab + 0x38),
    );
    (off..off + size)
        .step_by(entsize)
        .skip(1) // the null symbol
        .map(|e| {
            let name_at = strtab + u32_at(e);
            let len = b[name_at..].iter().position(|&c| c == 0).unwrap();
            let name = String::from_utf8_lossy(&b[name_at..name_at + len]).into_owned();
            (name, b[e + 4] & 0xf == 2, u16_at(e + 6) != 0) // STT_FUNC, !SHN_UNDEF
        })
        .collect()
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

/// Unbounded recursion must stay a graceful exit 101 at every level, like the
/// interpreter's recursion-limit panic. With tail-recursion elimination the
/// optimiser turned this into a loop spinning ~2^63 times (a hang), so the
/// native run is bounded by a deadline here.
#[test]
fn optimised_unbounded_recursion_still_overflows_gracefully() {
    let src = write_src(
        "rec",
        "fn rec(n: i64) -> i64 { rec(n + 1) }\nfn main() -> i64 { rec(0) }\n",
    );
    let interp = axon().arg("run").arg(&src).output().expect("spawn run");
    assert_eq!(interp.status.code(), Some(101), "{}", stderr_of(&interp));

    for level in ["0", "1", "2", "3", "s", "z"] {
        let bin = tmp(&format!("rec_bin_{level}"));
        let _ = std::fs::remove_file(&bin);
        let b = build(&["--no-cache", "--opt-level", level], &src, &bin);
        if codegen_absent(&b) {
            let _ = std::fs::remove_file(&src);
            return;
        }
        assert_eq!(
            b.status.code(),
            Some(0),
            "build O{level}:\n{}",
            stderr_of(&b)
        );
        let mut child = Command::new(&bin)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("run native");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while child.try_wait().expect("poll native").is_none() {
            if std::time::Instant::now() > deadline {
                let _ = child.kill();
                panic!("O{level}: unbounded recursion hung instead of overflowing");
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let out = child.wait_with_output().expect("collect native");
        let _ = std::fs::remove_file(&bin);
        assert_eq!(
            out.status.code(),
            Some(101),
            "O{level}: {}",
            stderr_of(&out)
        );
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("stack overflow"),
            "O{level}: {}",
            stderr_of(&out)
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
    assert!(
        fib0.contains("alloca"),
        "O0 fib should be unoptimised:\n{fib0}"
    );
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
    assert_eq!(
        cached_obj.status.code(),
        Some(0),
        "{}",
        stderr_of(&cached_obj)
    );
    assert_eq!(elf_type(&obj), 1, "--emit-obj with a warm cache");
    let _ = std::fs::remove_file(&obj);

    let ll = tmp("emitobj_cached.ll");
    let cached_ir = build(
        &["--release", "--cache-dir", &cache_arg, "--emit-llvm"],
        &src,
        &ll,
    );
    assert_eq!(
        cached_ir.status.code(),
        Some(0),
        "{}",
        stderr_of(&cached_ir)
    );
    let text = std::fs::read_to_string(&ll).expect("--emit-llvm with a warm cache writes IR text");
    assert!(text.contains("define"), "not LLVM IR:\n{text}");
    let _ = std::fs::remove_file(&ll);
    let _ = std::fs::remove_dir_all(&cache);
    let _ = std::fs::remove_file(&src);
}

/// AX-37: an `O0` object holds only what `main` reaches. `declare_builtins`
/// emits ~70 builtin wrappers into every module; with no `globaldce` at `O0`
/// all of them were compiled, each pulling its `__axon_*` runtime code into the
/// binary, and `--emit-obj` (which skipped the AI-wrapper pruning) referenced
/// `__axon_ai_*`, so a debug object did not link against `libaxon_rt.a`.
#[test]
fn o0_object_keeps_only_reachable_functions_and_links_against_the_base_runtime() {
    let src = write_src("o0dce", "fn main() {\n    println(\"hi\")\n}\n");
    let obj = tmp("o0dce.o");
    let bin = tmp("o0dce_bin");
    let _ = std::fs::remove_file(&obj);
    let b = build(&["--no-cache", "--emit-obj"], &src, &obj);
    if codegen_absent(&b) {
        let _ = std::fs::remove_file(&src);
        return;
    }
    assert_eq!(b.status.code(), Some(0), "{}", stderr_of(&b));
    let syms = elf_symbols(&obj);
    let mut defined: Vec<&str> = syms
        .iter()
        .filter(|(_, func, def)| *func && *def)
        .map(|(n, _, _)| n.as_str())
        .collect();
    defined.sort_unstable();
    assert_eq!(
        defined,
        ["main", "println"],
        "the O0 object must define only the functions main reaches"
    );
    let ai: Vec<&str> = syms
        .iter()
        .map(|(n, _, _)| n.as_str())
        .filter(|n| n.starts_with("__axon_ai_"))
        .collect();
    assert!(ai.is_empty(), "O0 --emit-obj references {ai:?}");

    // The object a normal O0 build links: it must link against the debug
    // `libaxon_rt.a` alone. The normal build makes sure that library is built.
    let _ = std::fs::remove_file(&bin);
    let normal = build(&["--no-cache"], &src, &bin);
    assert_eq!(normal.status.code(), Some(0), "{}", stderr_of(&normal));
    let exe_target = Path::new(env!("CARGO_BIN_EXE_axon"))
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let rt = std::env::var_os("AXON_RUNTIME_DIR")
        .map(|d| PathBuf::from(d).join("libaxon_rt.a"))
        .into_iter()
        .chain(
            std::env::var_os("CARGO_TARGET_DIR")
                .map(|d| PathBuf::from(d).join("debug/libaxon_rt.a")),
        )
        .chain(exe_target.map(|t| t.join("debug/libaxon_rt.a")))
        .find(|p| p.is_file())
        .expect("the O0 build above links libaxon_rt.a from the target dir");
    let _ = std::fs::remove_file(&bin);
    let link = Command::new("cc")
        .arg(&obj)
        .arg(&rt)
        .arg("-o")
        .arg(&bin)
        .args(["-no-pie", "-Wl,--gc-sections", "-lpthread", "-lm"])
        .output()
        .expect("spawn cc");
    let _ = std::fs::remove_file(&obj);
    let _ = std::fs::remove_file(&src);
    assert!(
        link.status.success(),
        "O0 object + {} must link:\n{}",
        rt.display(),
        stderr_of(&link)
    );
    let run = Command::new(&bin).output().expect("run linked O0 object");
    let _ = std::fs::remove_file(&bin);
    assert_eq!(String::from_utf8_lossy(&run.stdout), "hi\n");
}
