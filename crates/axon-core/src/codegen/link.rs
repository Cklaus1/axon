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
    let module = ctx
        .create_module_from_ir(buf)
        .map_err(|e| format!("[E0906] cached bitcode could not be loaded: {}", e.to_string()))?;
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
        .create_target_machine(&triple, "generic", "", opt, RelocMode::PIC, CodeModel::Default)
        .ok_or_else(|| format!("[E0904] could not create target machine for '{target_triple}'"))?;
    module.set_triple(&triple);
    machine
        .write_to_file(module, FileType::Object, Path::new(output_path))
        .map_err(|e| format!("wasm object emit: {e}"))?;
    Ok(())
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
            .create_target_machine(&triple, "generic", "", opt, RelocMode::PIC, CodeModel::Default)
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
        let target =
            Target::from_triple(&triple).map_err(|e| format!("get native target: {e}"))?;
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
    let mut link_args: Vec<&str> = vec![&obj_path, "-o", output_path, "-lpthread", "-no-pie", "-lm"];
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
        Err(format!("linker ({}) exited with {}", linker.display(), status))
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
            let val = rest
                .trim_start_matches([' ', '\t', '='])
                .trim_matches('"');
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
    let cargo = std::env::var("CARGO").ok().unwrap_or_else(|| "cargo".into());
    let profile = if release { "release" } else { "debug" };

    // Locate the workspace root relative to this binary.
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .map(|d| format!("{d}/../../../Cargo.toml"))
        .unwrap_or_else(|_| "Cargo.toml".into());

    let status = Command::new(&cargo)
        .args(["build", "-p", "axon-rt", "--manifest-path", &manifest])
        .args(if release { &["--release"][..] } else { &[][..] })
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if !status.success() {
        return None;
    }

    // Resolve the target directory from CARGO_TARGET_DIR or adjacent to manifest.
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| {
            // Walk up from CARGO_MANIFEST_DIR to find <workspace>/target.
            std::env::var("CARGO_MANIFEST_DIR")
                .map(|d| format!("{d}/../../../target"))
                .unwrap_or_else(|_| "target".into())
        });

    let lib_path = format!("{target_dir}/{profile}/libaxon_rt.a");
    if std::path::Path::new(&lib_path).exists() {
        Some(lib_path)
    } else {
        None
    }
}

// ── axon-ai build helper ─────────────────────────────────────────────────────

/// Build `axon-ai` as a static library and return the path to `libaxon_ai.a`.
///
/// Silently returns `None` if cargo is not found or the build fails, so that
/// the linker step still attempts to proceed (ai_complete will be a missing
/// symbol only when actually called).
pub(super) fn build_axon_ai(release: bool) -> Option<String> {
    let cargo = std::env::var("CARGO").ok().unwrap_or_else(|| "cargo".into());
    let profile = if release { "release" } else { "debug" };

    // Locate the workspace root relative to this binary.
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .map(|d| format!("{d}/../../../Cargo.toml"))
        .unwrap_or_else(|_| "Cargo.toml".into());

    let status = Command::new(&cargo)
        .args(["build", "-p", "axon-ai", "--manifest-path", &manifest])
        .args(if release { &["--release"][..] } else { &[][..] })
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if !status.success() {
        return None;
    }

    // Resolve the target directory from CARGO_TARGET_DIR or adjacent to manifest.
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| {
            std::env::var("CARGO_MANIFEST_DIR")
                .map(|d| format!("{d}/../../../target"))
                .unwrap_or_else(|_| "target".into())
        });

    let lib_path = format!("{target_dir}/{profile}/libaxon_ai.a");
    if std::path::Path::new(&lib_path).exists() {
        Some(lib_path)
    } else {
        None
    }
}
