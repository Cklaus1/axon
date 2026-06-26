//! Object-file emission and linking for axon-compiled programs.
//!
//! Extracted from the historic monolithic `codegen.rs` as the first
//! step of the §7.5 module split.  This file contains only **free
//! functions** (no `Codegen<'ctx>` methods), so the move is mechanically
//! safe — no field-access changes, no impl-block surgery.
//!
//! See `ROADMAP.md` §7.5 for the full split plan.  Subsequent splits
//! (types, expr, stmt, builtins, asi) involve methods on `Codegen` and
//! will require careful pub(super) decisions for fields and helpers;
//! they should be done on a faster machine where each step can be
//! validated by a full `cargo build -p axon-core`.
//!
//! Public surface:
//!   * `compile_bitcode_to_binary` — load LLVM bitcode + link to a binary
//!
//! Crate-private surface (visible to `super::Codegen`):
//!   * `emit_object_and_link` — IR module → object file → linked binary
//!   * `build_axon_rt`        — build axon-rt staticlib for linking
//!   * `build_axon_ai`        — build axon-ai staticlib for linking
//!   * `read_cross_linker`    — parse `~/.config/axon/cross.toml`

use std::path::Path;
use std::process::Command;

use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::OptimizationLevel;

// ── R14 Android cross-link support ────────────────────────────────────────────

/// True when `triple` names an Android (bionic) target.
///
/// R14 slice 1: Android is the Linux-buildable mobile target. iOS
/// (`*-apple-ios`) is specified but gated on a macOS host (R14 §4 / Q4) and is
/// NOT handled here — it would need the Xcode `ld` + iOS SDK.
pub(super) fn is_android_triple(triple: &str) -> bool {
    triple.contains("-android")
}

/// Map an Android LLVM triple to (its NDK clang basename, the cargo target env
/// infix used by `CARGO_TARGET_<INFIX>_LINKER` / `CC_<rust_target>`).
///
/// Returns `None` for an unrecognized android arch.
fn android_ndk_clang(triple: &str, api: u32) -> Option<(String, String)> {
    // The NDK toolchain ships per-(arch,api) clang wrappers, e.g.
    // `aarch64-linux-android34-clang`. armv7 uses the `armv7a-linux-androideabi`
    // clang basename but the rust target is `armv7-linux-androideabi`.
    let (clang_prefix, rust_target) = if triple.starts_with("aarch64") {
        ("aarch64-linux-android", "aarch64-linux-android")
    } else if triple.starts_with("x86_64") {
        ("x86_64-linux-android", "x86_64-linux-android")
    } else if triple.starts_with("armv7") || triple.starts_with("arm-") {
        ("armv7a-linux-androideabi", "armv7-linux-androideabi")
    } else if triple.starts_with("i686") {
        ("i686-linux-android", "i686-linux-android")
    } else {
        return None;
    };
    Some((
        format!("{clang_prefix}{api}-clang"),
        rust_target.to_string(),
    ))
}

/// Locate the Android NDK toolchain `bin/` directory.
///
/// Order: `$ANDROID_NDK_HOME`, `$ANDROID_NDK_ROOT`, then the highest-numbered
/// `$ANDROID_HOME/ndk/<version>`. Returns the absolute `…/bin` path.
fn android_ndk_bin() -> Option<std::path::PathBuf> {
    let host_tag = "linux-x86_64"; // this build host is Linux/WSL2 (R14 §1).
    let try_root = |root: &Path| -> Option<std::path::PathBuf> {
        let bin = root
            .join("toolchains/llvm/prebuilt")
            .join(host_tag)
            .join("bin");
        if bin.is_dir() {
            Some(bin)
        } else {
            None
        }
    };
    for var in ["ANDROID_NDK_HOME", "ANDROID_NDK_ROOT", "NDK_HOME"] {
        if let Some(p) = std::env::var_os(var) {
            if let Some(bin) = try_root(Path::new(&p)) {
                return Some(bin);
            }
        }
    }
    // $ANDROID_HOME/ndk/<version> — pick the lexically-highest version dir.
    let sdk = std::env::var_os("ANDROID_HOME").or_else(|| std::env::var_os("ANDROID_SDK_ROOT"))?;
    let ndk_root = Path::new(&sdk).join("ndk");
    let mut versions: Vec<std::path::PathBuf> = std::fs::read_dir(&ndk_root)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    versions.sort();
    for v in versions.into_iter().rev() {
        if let Some(bin) = try_root(&v) {
            return Some(bin);
        }
    }
    None
}

/// Resolve the NDK clang to use as the Android linker for `triple`.
///
/// Precedence: an explicit `[target.<triple>] linker = …` in
/// `~/.config/axon/cross.toml` wins; otherwise auto-detect the NDK
/// (`android_ndk_bin`) and pick the per-arch clang at API `api`.
fn android_linker(triple: &str, api: u32) -> Result<std::path::PathBuf, String> {
    if let Some(l) = read_cross_linker(triple) {
        return Ok(std::path::PathBuf::from(l));
    }
    let (clang_name, _rust) = android_ndk_clang(triple, api).ok_or_else(|| {
        format!("[E1710] mobile target '{triple}' is not a recognized Android arch")
    })?;
    let bin = android_ndk_bin().ok_or_else(|| {
        format!(
            "[E1710] mobile target '{triple}' requires the Android NDK; not found \
             (set ANDROID_NDK_HOME or ANDROID_HOME, or add [target.{triple}] linker=… \
             to ~/.config/axon/cross.toml)"
        )
    })?;
    let clang = bin.join(&clang_name);
    if clang.exists() {
        Ok(clang)
    } else {
        Err(format!(
            "[E1710] mobile target '{triple}' requires '{}' in the NDK toolchain ({}); not found",
            clang_name,
            bin.display()
        ))
    }
}

// ── Public surface ────────────────────────────────────────────────────────────

/// Load LLVM bitcode bytes into a fresh context and compile to a binary.
///
/// Used by the incremental cache on a cache hit: the IR emission stages are
/// skipped; we go directly from cached bitcode to object file → binary.
pub fn compile_bitcode_to_binary(
    bitcode: &[u8],
    output_path: &str,
    release: bool,
    target_triple: Option<&str>,
) -> Result<(), String> {
    use inkwell::memory_buffer::MemoryBuffer;
    let ctx = inkwell::context::Context::create();
    let buf = MemoryBuffer::create_from_memory_range(bitcode, "cached_bitcode");
    let module = ctx.create_module_from_ir(buf).map_err(|e| {
        format!(
            "[E0906] cached bitcode could not be loaded: {}",
            e.to_string()
        )
    })?;
    emit_object_and_link(&module, output_path, release, target_triple)
}

/// R7 Slice B (AOT wasm, object half): emit a WebAssembly **object file** for
/// `module` at `output_path` via the inkwell `wasm32` backend, WITHOUT linking.
///
/// This is the real IR→wasm codegen step the spec (R7 §3.2) deferred behind
/// E0907. The *link* into a runnable `.wasm` needs a wasm libc sysroot +
/// `wasm-ld` (the documented remaining gap, §12), which is environment-fragile;
/// emitting and validating the object is the verifiable, in-tree half. The
/// emitted file starts with the wasm magic `\0asm` (0x00 0x61 0x73 0x6d), which
/// the caller checks to prove the backend produced genuine wasm, not a stub.
pub fn emit_wasm_object(
    module: &inkwell::module::Module<'_>,
    output_path: &str,
    release: bool,
    target_triple: &str,
) -> Result<(), String> {
    let opt = if release {
        OptimizationLevel::Default
    } else {
        OptimizationLevel::None
    };
    Target::initialize_all(&InitializationConfig::default());
    let triple = TargetTriple::create(target_triple);
    let target = Target::from_triple(&triple).map_err(|e| {
        format!("[E0904] target '{target_triple}' not supported by this LLVM build: {e}")
    })?;
    let machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            opt,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| format!("[E0904] could not create target machine for '{target_triple}'"))?;
    module.set_triple(&triple);
    machine
        .write_to_file(module, FileType::Object, Path::new(output_path))
        .map_err(|e| format!("wasm object emit: {e}"))?;
    Ok(())
}

/// R14 (mobile): emit a relocatable **object file** for an arbitrary device
/// `target_triple` WITHOUT linking. LLVM cross-emits the AArch64/x86_64 object
/// for an Apple-iOS or Android triple even on a Linux host — only the *link*
/// into `.a`/`.xcframework` (iOS) or `.so` (Android) needs the platform
/// toolchain. This is the verifiable in-tree half on Linux; the link is the
/// macOS/NDK host's job (spec §4 / §12 Q4). PIC reloc + the default code model,
/// matching a normal mobile static-/shared-lib object.
pub fn emit_object_for_triple(
    module: &inkwell::module::Module<'_>,
    output_path: &str,
    release: bool,
    target_triple: &str,
) -> Result<(), String> {
    let opt = if release {
        OptimizationLevel::Default
    } else {
        OptimizationLevel::None
    };
    Target::initialize_all(&InitializationConfig::default());
    let triple = TargetTriple::create(target_triple);
    let target = Target::from_triple(&triple).map_err(|e| {
        format!("[E0904] target '{target_triple}' not supported by this LLVM build: {e}")
    })?;
    let machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            opt,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| format!("[E0904] could not create target machine for '{target_triple}'"))?;
    module.set_triple(&triple);
    machine
        .write_to_file(module, FileType::Object, Path::new(output_path))
        .map_err(|e| format!("mobile object emit: {e}"))
}

// ── Crate-private surface (callable from super::Codegen) ─────────────────────

/// Initialize LLVM targets, create a `TargetMachine`, emit an object file, and
/// link it into a binary at `output_path`.
///
/// When `target_triple` is `None` the native host triple is used.  When it is
/// `Some(triple)` all LLVM backends are initialized and the specified triple is
/// used (cross-compilation).
pub(super) fn emit_object_and_link(
    module: &inkwell::module::Module<'_>,
    output_path: &str,
    release: bool,
    target_triple: Option<&str>,
) -> Result<(), String> {
    let opt = if release {
        OptimizationLevel::Default
    } else {
        OptimizationLevel::None
    };

    let (triple, machine) = if let Some(triple_str) = target_triple {
        // Cross-compilation: initialise every backend so any target is reachable.
        Target::initialize_all(&InitializationConfig::default());
        let triple = TargetTriple::create(triple_str);
        let target = Target::from_triple(&triple).map_err(|e| {
            format!(
                "[E0904] target '{}' not supported by this LLVM build: {}",
                triple_str, e
            )
        })?;
        let machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                opt,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or_else(|| {
                format!(
                    "[E0904] could not create target machine for '{}'",
                    triple_str
                )
            })?;
        (triple, machine)
    } else {
        // Native compilation.
        Target::initialize_native(&InitializationConfig::default())
            .map_err(|e| format!("LLVM native target init: {e}"))?;
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|e| format!("get native target: {e}"))?;
        let machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                opt,
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| "failed to create native target machine".to_string())?;
        (triple, machine)
    };

    // Update the module's target triple so the emitted object is correct.
    module.set_triple(&triple);

    // Emit object file to a temporary path.
    let obj_path = format!("{output_path}.o");
    machine
        .write_to_file(module, FileType::Object, Path::new(&obj_path))
        .map_err(|e| format!("object emit: {e}"))?;

    // R14 slice 1/2: Android (bionic) cross-link via the NDK clang. Android
    // ELFs are PIE and link bionic libc, not glibc — the host `-no-pie`/`-lm`
    // recipe does not apply. The axon-rt staticlib must be the ANDROID-triple
    // cross-build, not the host one. Handled in a dedicated path so the host
    // link recipe below stays exactly as it was.
    if let Some(triple_str) = target_triple {
        if is_android_triple(triple_str) {
            let res = android_link(&obj_path, output_path, release, triple_str, false);
            let _ = std::fs::remove_file(&obj_path);
            return res;
        }
    }

    // Build axon-rt static library so channel/spawn builtins are available.
    let rt_lib = build_axon_rt(release);
    // Build axon-ai static library so ai_complete is available.
    let ai_lib = build_axon_ai(release);

    // Determine linker: prefer the cross.toml override, else probe the host.
    let linker_override = target_triple.and_then(read_cross_linker);
    let linker = if let Some(ref l) = linker_override {
        std::path::PathBuf::from(l)
    } else {
        which::which("cc")
            .or_else(|_| which::which("clang"))
            .or_else(|_| which::which("gcc"))
            .map_err(|_| "no C compiler found (tried cc, clang, gcc)".to_string())?
    };

    // `-no-pie`: our emitted object uses non-PIC relocations (R_X86_64_32S),
    // so the default PIE link fails ("can not be used when making a PIE
    // object"). Link non-PIE. (R1: surfaced once the native build actually
    // produced objects — see BUILD_RESOLVED.md.)
    // `-no-pie`: our emitted object uses non-PIC relocations (R_X86_64_32S),
    // so the default PIE link fails. `-lm`: axon-rt's math builtins
    // (`__axon_pow` etc.) reference libm. (R1: both surfaced once the native
    // build actually produced objects — see BUILD_RESOLVED.md.)
    // `-Wl,--allow-multiple-definition` (BUG_HUNT #43): both `libaxon_rt.a` and
    // `libaxon_ai.a` are Rust `staticlib`s, so each embeds its own copy of the
    // Rust `core`/`std` symbols (e.g. `core::fmt::builders`). In a release link
    // the duplicate `core` symbols are a fatal "multiple definition" error
    // (debug happens to dedup via weak/comdat). Both copies come from the SAME
    // rustc `core`, so taking the first is safe — this is the standard remedy
    // for linking two Rust staticlibs into one binary.
    let mut link_args: Vec<&str> = vec![
        &obj_path,
        "-o",
        output_path,
        "-lpthread",
        "-no-pie",
        "-lm",
        "-Wl,--allow-multiple-definition",
    ];
    if let Some(ref lib) = rt_lib {
        link_args.push(lib.as_str());
    }
    if let Some(ref lib) = ai_lib {
        link_args.push(lib.as_str());
    }

    let status = Command::new(&linker)
        .args(&link_args)
        .status()
        .map_err(|e| format!("linker spawn: {e}"))?;

    let _ = std::fs::remove_file(&obj_path);

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "linker ({}) exited with {}",
            linker.display(),
            status
        ))
    }
}

/// R14: emit an object and link it as a loadable shared library (`.so`).
///
/// Currently wired for the Android triples (the `--host mobile` jniLibs path).
/// A non-Android shared-lib request is refused with a clear error rather than
/// silently producing a host-shaped `.so`.
pub(super) fn emit_shared_lib(
    module: &inkwell::module::Module<'_>,
    output_path: &str,
    release: bool,
    target_triple: Option<&str>,
) -> Result<(), String> {
    let triple_str = target_triple.ok_or_else(|| {
        "[E1710] --host mobile requires an Android --target (e.g. aarch64-linux-android)"
            .to_string()
    })?;
    if !is_android_triple(triple_str) {
        return Err(format!(
            "[E1710] --host mobile shared-lib output is only wired for Android triples; \
             '{triple_str}' is not Android (iOS is gated on a macOS host, R14 §4/Q4)"
        ));
    }

    let opt = if release {
        OptimizationLevel::Default
    } else {
        OptimizationLevel::None
    };
    Target::initialize_all(&InitializationConfig::default());
    let triple = TargetTriple::create(triple_str);
    let target = Target::from_triple(&triple).map_err(|e| {
        format!("[E0904] target '{triple_str}' not supported by this LLVM build: {e}")
    })?;
    let machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            opt,
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| format!("[E0904] could not create target machine for '{triple_str}'"))?;
    module.set_triple(&triple);

    let obj_path = format!("{output_path}.o");
    machine
        .write_to_file(module, FileType::Object, Path::new(&obj_path))
        .map_err(|e| format!("object emit: {e}"))?;

    let res = android_link(&obj_path, output_path, release, triple_str, true);
    let _ = std::fs::remove_file(&obj_path);
    res
}

/// Emit just the object file for a freestanding build (no linking step).
/// Used by `axon build --freestanding --emit-obj` to let the caller supply
/// a boot stub object and run the final link manually.
pub(super) fn emit_freestanding_obj(
    module: &inkwell::module::Module<'_>,
    output_path: &str,
    release: bool,
    target_triple: Option<&str>,
) -> Result<(), String> {
    let triple_str = target_triple.unwrap_or("x86_64-unknown-none");
    let opt = if release {
        OptimizationLevel::Default
    } else {
        OptimizationLevel::None
    };
    Target::initialize_all(&InitializationConfig::default());
    let triple = TargetTriple::create(triple_str);
    let target = Target::from_triple(&triple).map_err(|e| {
        format!("[E0904] target '{triple_str}' not supported by this LLVM build: {e}")
    })?;
    // R25 (Zephyr/ARM): the `Kernel` code model is x86-64-specific and is
    // rejected by the ARM/thumb backend. A bare-metal ARM Cortex-M object that
    // links into a Zephyr app uses the default (small) code model. Select the
    // code model by target architecture.
    let (reloc, code_model) = freestanding_reloc_codemodel(triple_str);
    let machine = target
        .create_target_machine(&triple, "generic", "", opt, reloc, code_model)
        .ok_or_else(|| format!("[E0904] could not create target machine for '{triple_str}'"))?;
    module.set_triple(&triple);
    machine
        .write_to_file(module, FileType::Object, Path::new(output_path))
        .map_err(|e| format!("freestanding object emit: {e}"))
}

/// R25: select `(RelocMode, CodeModel)` for a freestanding target by its triple.
///
/// x86_64 bare-metal kernels load at a high fixed address and use the `Kernel`
/// code model with static relocations (the R17 default). ARM/thumb targets
/// (Cortex-M, e.g. a Zephyr app object) reject the `Kernel` code model — they
/// use the default (small) code model. Both stay static (no PIC) for a no-host
/// image.
fn freestanding_reloc_codemodel(triple_str: &str) -> (RelocMode, CodeModel) {
    let is_arm = triple_str.starts_with("thumb")
        || triple_str.starts_with("arm")
        || triple_str.starts_with("aarch64");
    if is_arm {
        (RelocMode::Static, CodeModel::Default)
    } else {
        (RelocMode::Static, CodeModel::Kernel)
    }
}

/// Emit an object file and link it as a freestanding (bare-metal) ELF binary.
///
/// Differences from the hosted path:
///   - Target defaults to `x86_64-unknown-none`; `RelocMode::Static`, `CodeModel::Kernel`.
///   - No axon-rt, no axon-ai, no libc/pthreads/libm.
///   - Linker is `ld` (or `x86_64-elf-ld`/`ld.bfd`); falls back to `cc -nostdlib`.
///   - `--entry <fn>` sets the ELF entry symbol from the `@[entry]`-annotated fn.
pub(super) fn emit_freestanding_binary(
    module: &inkwell::module::Module<'_>,
    output_path: &str,
    release: bool,
    target_triple: Option<&str>,
    entry_fn: Option<&str>,
    linker_script: Option<&str>,
) -> Result<(), String> {
    let triple_str = target_triple.unwrap_or("x86_64-unknown-none");
    let opt = if release {
        OptimizationLevel::Default
    } else {
        OptimizationLevel::None
    };

    Target::initialize_all(&InitializationConfig::default());
    let triple = TargetTriple::create(triple_str);
    let target = Target::from_triple(&triple).map_err(|e| {
        format!("[E0904] target '{triple_str}' not supported by this LLVM build: {e}")
    })?;
    let (reloc, code_model) = freestanding_reloc_codemodel(triple_str);
    let machine = target
        .create_target_machine(&triple, "generic", "", opt, reloc, code_model)
        .ok_or_else(|| format!("[E0904] could not create target machine for '{triple_str}'"))?;

    module.set_triple(&triple);

    let obj_path = format!("{output_path}.o");
    machine
        .write_to_file(module, FileType::Object, Path::new(&obj_path))
        .map_err(|e| format!("freestanding object emit: {e}"))?;

    // Prefer a bare-metal ld; fall back to cc with -nostdlib flags.
    let linker = which::which("x86_64-elf-ld")
        .or_else(|_| which::which("ld.bfd"))
        .or_else(|_| which::which("ld"));

    let result = if let Ok(ld) = linker {
        let mut args: Vec<String> = vec![
            obj_path.clone(),
            "-o".into(),
            output_path.into(),
            "-static".into(),
            "--no-dynamic-linker".into(),
        ];
        if let Some(entry) = entry_fn {
            args.push("--entry".into());
            args.push(entry.into());
        }
        if let Some(script) = linker_script {
            args.push("-T".into());
            args.push(script.into());
        }
        Command::new(&ld)
            .args(&args)
            .status()
            .map_err(|e| format!("freestanding ld spawn: {e}"))?
    } else {
        // Fallback: cc with -nostdlib/-static (works on most Linux hosts for x86-64).
        let cc = which::which("cc")
            .or_else(|_| which::which("gcc"))
            .or_else(|_| which::which("clang"))
            .map_err(|_| {
                "no linker found (tried x86_64-elf-ld, ld.bfd, ld, cc, gcc, clang)".to_string()
            })?;
        let mut args: Vec<String> = vec![
            obj_path.clone(),
            "-o".into(),
            output_path.into(),
            "-nostdlib".into(),
            "-static".into(),
            "-no-pie".into(),
        ];
        if let Some(entry) = entry_fn {
            args.push(format!("-Wl,--entry,{entry}"));
        }
        if let Some(script) = linker_script {
            args.push(format!("-Wl,-T,{script}"));
        }
        Command::new(&cc)
            .args(&args)
            .status()
            .map_err(|e| format!("freestanding cc spawn: {e}"))?
    };

    let _ = std::fs::remove_file(&obj_path);

    if result.success() {
        Ok(())
    } else {
        Err(format!("freestanding linker exited with {result}"))
    }
}

/// Look up the configured linker for `target` in `~/.config/axon/cross.toml`.
///
/// Returns `None` if the file is absent or the target section has no `linker`
/// key — in which case the caller falls through to the host linker (which may
/// fail for truly cross-compiled targets, emitting E0905 guidance).
pub(super) fn read_cross_linker(target: &str) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let config_path = std::path::PathBuf::from(home)
        .join(".config")
        .join("axon")
        .join("cross.toml");
    let content = std::fs::read_to_string(config_path).ok()?;

    // Minimal TOML section parser: find [target.<triple>] then scan key = "value" lines.
    let section_header = format!("[target.{target}]");
    let pos = content.find(&section_header)?;
    let after = &content[pos + section_header.len()..];

    for line in after.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            break; // reached the next section
        }
        if let Some(rest) = trimmed.strip_prefix("linker") {
            // Accept: linker = "value"  or  linker="value"
            let val = rest.trim_start_matches([' ', '\t', '=']).trim_matches('"');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

// ── axon-rt build helper ─────────────────────────────────────────────────────

/// Build `axon-rt` as a static library and return the path to `libaxon_rt.a`.
///
/// Silently returns `None` if cargo is not found or the build fails, so that
/// the linker step still attempts to proceed (channel/spawn functions will
/// simply be missing symbols if the rt wasn't linked).
pub(super) fn build_axon_rt(release: bool) -> Option<String> {
    build_crate_staticlib("axon-rt", "libaxon_rt.a", release, None)
}

/// Build a workspace staticlib crate (`axon-rt`/`axon-ai`) and return the path
/// to the produced `.a`. When `target` is `Some(triple)` the crate is
/// cross-built for that triple (R14: the Android cross-build of axon-rt) and the
/// artifact is read from `target/<triple>/<profile>/`.
fn build_crate_staticlib(
    pkg: &str,
    libname: &str,
    release: bool,
    target: Option<&str>,
) -> Option<String> {
    let cargo = std::env::var("CARGO")
        .ok()
        .unwrap_or_else(|| "cargo".into());
    let profile = if release { "release" } else { "debug" };

    // Locate the workspace root relative to this binary.
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .map(|d| format!("{d}/../../../Cargo.toml"))
        .unwrap_or_else(|_| "Cargo.toml".into());

    let mut cmd = Command::new(&cargo);
    cmd.args(["build", "-p", pkg, "--manifest-path", &manifest])
        .args(if release { &["--release"][..] } else { &[][..] });
    if let Some(t) = target {
        cmd.args(["--target", t]);
        // R14: when cross-building for Android, point cargo + cc-rs at the NDK
        // clang/ar so the staticlib's C shims and the rust object both target
        // bionic. Only set vars the caller hasn't already overridden.
        if is_android_triple(t) {
            let api: u32 = std::env::var("AXON_ANDROID_API")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(34);
            if let (Some((clang_name, rust_target)), Some(bin)) =
                (android_ndk_clang(t, api), android_ndk_bin())
            {
                let clang = bin.join(&clang_name);
                let ar = bin.join("llvm-ar");
                let upper = rust_target.to_uppercase().replace('-', "_");
                let set = |cmd: &mut Command, k: String, v: &std::ffi::OsStr| {
                    if std::env::var_os(&k).is_none() {
                        cmd.env(k, v);
                    }
                };
                set(
                    &mut cmd,
                    format!("CARGO_TARGET_{upper}_LINKER"),
                    clang.as_os_str(),
                );
                set(&mut cmd, format!("CC_{rust_target}"), clang.as_os_str());
                set(&mut cmd, format!("AR_{rust_target}"), ar.as_os_str());
            }
        }
    }
    let status = cmd
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if !status.success() {
        return None;
    }

    // Resolve the target directory from CARGO_TARGET_DIR or adjacent to manifest.
    let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| {
        // Walk up from CARGO_MANIFEST_DIR to find <workspace>/target.
        std::env::var("CARGO_MANIFEST_DIR")
            .map(|d| format!("{d}/../../../target"))
            .unwrap_or_else(|_| "target".into())
    });

    // Cross-built artifacts live under target/<triple>/<profile>/.
    let lib_path = match target {
        Some(t) => format!("{target_dir}/{t}/{profile}/{libname}"),
        None => format!("{target_dir}/{profile}/{libname}"),
    };
    if std::path::Path::new(&lib_path).exists() {
        Some(lib_path)
    } else {
        None
    }
}

/// R14 slice 1/2: link an Android object into a runnable bionic ELF executable.
///
/// Uses the NDK clang as the linker (cross.toml override or NDK auto-detect),
/// links the Android-triple cross-build of axon-rt, and produces a PIE ELF (the
/// Android ABI requirement). Produces an E1710 if the NDK is absent and E1712
/// if the link fails.
fn android_link(
    obj_path: &str,
    output_path: &str,
    release: bool,
    triple: &str,
    shared: bool,
) -> Result<(), String> {
    // Default NDK API level. The minSdk for Axon mobile artifacts; overridable
    // via $AXON_ANDROID_API.
    let api: u32 = std::env::var("AXON_ANDROID_API")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(34);

    let linker = android_linker(triple, api)?;

    // Cross-build axon-rt + axon-ai for the Android triple. Channel/spawn/
    // gfx-mock builtins resolve to axon-rt; ai_complete/ai_extract_* resolve to
    // axon-ai. Without them the link fails with a clear "undefined symbol
    // __axon_*" rather than a silent wrong result.
    let rt_lib = build_crate_staticlib("axon-rt", "libaxon_rt.a", release, Some(triple));
    let ai_lib = build_crate_staticlib("axon-ai", "libaxon_ai.a", release, Some(triple));

    // The NDK clang already knows the bionic sysroot, PIE, and libm. We do NOT
    // pass -no-pie (Android requires PIE) and let clang supply libc.
    let mut args: Vec<String> = vec![
        obj_path.to_string(),
        "-o".to_string(),
        output_path.to_string(),
        "-lm".to_string(),
        // Both axon-rt and axon-ai are Rust staticlibs that each embed their own
        // copy of `core`; allow the duplicate to resolve to the first (same
        // rustc core) — the host link uses the same remedy.
        "-Wl,--allow-multiple-definition".to_string(),
    ];
    if shared {
        // R14: a loadable JNI shared object. Keep the Axon entry symbols
        // (`main`/on_start/…) globally visible so the Kotlin wrapper can bind
        // them; the staticlibs are linked whole so __axon_* resolve.
        args.push("-shared".to_string());
        args.push("-Wl,--export-dynamic".to_string());
    }
    if let Some(ref lib) = rt_lib {
        args.push(lib.clone());
    }
    if let Some(ref lib) = ai_lib {
        args.push(lib.clone());
    }

    let status = Command::new(&linker)
        .args(&args)
        .status()
        .map_err(|e| format!("[E1712] mobile link failed for '{triple}': linker spawn: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "[E1712] mobile link failed for '{triple}': {} exited with {status}",
            linker.display()
        ))
    }
}

// ── axon-ai build helper ─────────────────────────────────────────────────────

/// Build `axon-ai` as a static library and return the path to `libaxon_ai.a`.
///
/// Silently returns `None` if cargo is not found or the build fails, so that
/// the linker step still attempts to proceed (ai_complete will be a missing
/// symbol only when actually called).
pub(super) fn build_axon_ai(release: bool) -> Option<String> {
    build_crate_staticlib("axon-ai", "libaxon_ai.a", release, None)
}
