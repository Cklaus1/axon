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
//!   * `HostedObject` / `link_hosted_object` — a hosted program's optimised
//!     object (what the build cache stores) and the link that turns it into a
//!     binary; a cache hit runs only the latter
//!   * `OptLevel`                  — `axon build --opt-level` (IR pipeline + backend level)
//!
//! Crate-private surface (visible to `super::Codegen`):
//!   * `emit_hosted_object_bytes` — IR module → optimised program object, in memory
//!   * `prune_unreachable_ai_callers` / `Runtime` — pick the ONE runtime
//!     staticlib a binary links (`libaxon_rt.a` or `libaxon_rt_ai.a`)
//!   * `emit_hosted_object`   — IR module → program object file (no link)
//!   * `optimize_for_ir_dump` — run the build's optimisation on a module for `--emit-llvm`
//!   * `read_cross_linker`    — parse `~/.config/axon/cross.toml`

use std::path::Path;
use std::process::Command;

use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::module::{Linkage, Module};
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::values::{AnyValueEnum, BasicValue, BasicValueEnum};
use inkwell::OptimizationLevel;

// ── Optimisation levels (AX-17 / AX-21) ──────────────────────────────────────

/// Optimisation level of a native build (`axon build --opt-level`).
///
/// A level selects BOTH the LLVM new-pass-manager IR pipeline that runs before
/// emission (`default<On>`: mem2reg/SROA, inlining, GVN, LICM, loop passes, …)
/// and the `TargetMachine` backend level (instruction selection, scheduling,
/// register allocation). The size levels pair their `default<Os>`/`default<Oz>`
/// pipelines with the `Default` backend level, as clang does. `O0` runs no IR
/// passes at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptLevel {
    O0,
    O1,
    O2,
    O3,
    Os,
    Oz,
}

impl OptLevel {
    /// Parse the `--opt-level` spelling: `0`, `1`, `2`, `3`, `s` or `z`.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "0" => Ok(Self::O0),
            "1" => Ok(Self::O1),
            "2" => Ok(Self::O2),
            "3" => Ok(Self::O3),
            "s" => Ok(Self::Os),
            "z" => Ok(Self::Oz),
            other => Err(format!(
                "invalid opt level '{other}' (expected one of: 0, 1, 2, 3, s, z)"
            )),
        }
    }

    /// The level implied by `--release` alone: `O2` for a release build, `O0`
    /// otherwise.
    pub fn from_release(release: bool) -> Self {
        if release {
            Self::O2
        } else {
            Self::O0
        }
    }

    /// The `--opt-level` spelling of this level (inverse of [`OptLevel::parse`]).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::O0 => "0",
            Self::O1 => "1",
            Self::O2 => "2",
            Self::O3 => "3",
            Self::Os => "s",
            Self::Oz => "z",
        }
    }

    /// Any level above `O0`. Optimised builds link the release-profile
    /// axon-rt/axon-ai staticlibs; `O0` links the debug profile.
    pub fn is_optimized(self) -> bool {
        self != Self::O0
    }

    fn backend(self) -> OptimizationLevel {
        match self {
            Self::O0 => OptimizationLevel::None,
            Self::O1 => OptimizationLevel::Less,
            Self::O2 | Self::Os | Self::Oz => OptimizationLevel::Default,
            Self::O3 => OptimizationLevel::Aggressive,
        }
    }

    fn pipeline(self) -> Option<&'static str> {
        match self {
            Self::O0 => None,
            Self::O1 => Some("default<O1>"),
            Self::O2 => Some("default<O2>"),
            Self::O3 => Some("default<O3>"),
            Self::Os => Some("default<Os>"),
            Self::Oz => Some("default<Oz>"),
        }
    }
}

/// AX-21: run the IR pass pipeline for `opt` on `module`, verifying the module
/// before and after. The pipeline is target-aware, so the caller sets the
/// module triple first; the data layout is taken from `machine` here (codegen
/// emits none, and the passes would otherwise lay out types with LLVM's
/// target-neutral default, e.g. 4-byte-aligned i64). No-op at `O0`.
///
/// Every definition is marked `"disable-tail-calls"="true"` first, so each
/// Axon call keeps its stack frame. Unbounded recursion must stay a graceful
/// "stack overflow" exit 101 (the runtime's guard-page handler), matching the
/// interpreter's recursion-limit panic; tail-recursion elimination would turn
/// `fn rec(n: i64) -> i64 { rec(n + 1) }` into a loop spinning ~2^63 times,
/// and backend sibling calls would do the same to mutual recursion. The
/// attribute switches off both.
fn optimize_module(
    module: &Module<'_>,
    machine: &TargetMachine,
    opt: OptLevel,
) -> Result<(), String> {
    let Some(pipeline) = opt.pipeline() else {
        return Ok(());
    };
    mark_definitions(module, "disable-tail-calls", "true");
    module.set_data_layout(&machine.get_target_data().get_data_layout());
    module.verify().map_err(|e| {
        format!(
            "IR verification failed before `{pipeline}`: {}",
            e.to_string()
        )
    })?;
    module
        .run_passes(pipeline, machine, PassBuilderOptions::create())
        .map_err(|e| format!("LLVM pass pipeline `{pipeline}` failed: {}", e.to_string()))?;
    module.verify().map_err(|e| {
        format!(
            "IR verification failed after `{pipeline}`: {}",
            e.to_string()
        )
    })
}

/// Freestanding output links no libc, so the optimiser must not synthesise
/// libc calls (loop-idiom recognition turning a fill/copy loop into
/// `memset`/`memcpy`, printf→puts, …). `"no-builtins"` on every definition is
/// what clang's `-ffreestanding` emits for the same reason.
fn mark_no_builtins(module: &Module<'_>) {
    mark_definitions(module, "no-builtins", "");
}

/// Add the string function attribute `key`=`value` to every function
/// definition in `module` (declarations are left alone).
fn mark_definitions(module: &Module<'_>, key: &str, value: &str) {
    let attr = module.get_context().create_string_attribute(key, value);
    let mut next = module.get_first_function();
    while let Some(func) = next {
        next = func.get_next_function();
        if func.count_basic_blocks() > 0 {
            func.add_attribute(AttributeLoc::Function, attr);
        }
    }
}

/// AX-22: give every function DEFINITION except `main` internal linkage, for
/// output that is a whole hosted program (executable, or its program object).
///
/// Such a program is one LLVM module; the only caller from outside it is the C
/// runtime calling `main` (axon-rt/axon-ai import no program symbols). With
/// default external linkage every call left the module's control — through the
/// PLT under PIC — and the IPO passes could not specialise, merge or delete a
/// function. Internal linkage is dso_local by construction, so calls (including
/// self-recursion) are direct, and GlobalDCE drops the unused builtin helpers.
/// Declarations (runtime/libc externs) are untouched. `naked` functions keep
/// external linkage: their callers can be inline-asm text naming the symbol.
///
/// Not applied to freestanding, shared-lib, wasm or mobile-object output: their
/// consumers (boot stubs, linker scripts, JNI, wasm exports) reach functions by
/// name.
fn internalize_program_functions(module: &Module<'_>) {
    let naked = Attribute::get_named_enum_kind_id("naked");
    let mut next = module.get_first_function();
    while let Some(func) = next {
        next = func.get_next_function();
        let is_definition = func.count_basic_blocks() > 0;
        if !is_definition
            || func.get_linkage() != Linkage::External
            || func.get_name().to_bytes() == b"main"
            || func
                .get_enum_attribute(AttributeLoc::Function, naked)
                .is_some()
        {
            continue;
        }
        func.set_linkage(Linkage::Internal);
    }
}

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

/// A hosted program compiled to its relocatable object: module triple set,
/// program functions internalised (AX-22), the `opt` IR pipeline (AX-21) and
/// the backend run. Linking it needs nothing from LLVM, which is why it is
/// what the build cache stores (AX-34): a hit only links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostedObject {
    /// The ELF (or target-format) relocatable object bytes.
    pub bytes: Vec<u8>,
    /// Whether the object still calls an `__axon_ai_*` builtin after pruning
    /// and optimisation, i.e. links `libaxon_rt_ai.a` rather than
    /// `libaxon_rt.a` (`Runtime`). The object alone does not say so cheaply,
    /// and the link must pick the same runtime a fresh build would.
    pub links_ai_runtime: bool,
}

/// Link a hosted program object into a binary at `output_path`, against the
/// runtime staticlib it needs. `opt` selects the runtime profile (optimised
/// levels link the release staticlibs and strip debug info); `target_triple`
/// selects the cross/Android link recipe. Writes the object to
/// `<output_path>.o` for the linker and removes it afterwards.
pub fn link_hosted_object(
    obj: &HostedObject,
    output_path: &str,
    opt: OptLevel,
    target_triple: Option<&str>,
) -> Result<(), String> {
    let obj_path = format!("{output_path}.o");
    std::fs::write(&obj_path, &obj.bytes).map_err(|e| format!("object emit: {e}"))?;
    let rt = if obj.links_ai_runtime {
        Runtime::WithAi
    } else {
        Runtime::Base
    };
    let res = link_object_file(&obj_path, output_path, opt, target_triple, rt);
    let _ = std::fs::remove_file(&obj_path);
    res
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
    opt: OptLevel,
    target_triple: &str,
) -> Result<(), String> {
    let (triple, machine) = pic_target_machine(target_triple, opt)?;
    module.set_triple(&triple);
    optimize_module(module, &machine, opt)?;
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
    opt: OptLevel,
    target_triple: &str,
) -> Result<(), String> {
    let (triple, machine) = pic_target_machine(target_triple, opt)?;
    module.set_triple(&triple);
    optimize_module(module, &machine, opt)?;
    machine
        .write_to_file(module, FileType::Object, Path::new(output_path))
        .map_err(|e| format!("mobile object emit: {e}"))
}

/// `TargetMachine` for an explicit (cross/device) triple: all backends
/// initialised, PIC relocations, default code model.
fn pic_target_machine(
    triple_str: &str,
    opt: OptLevel,
) -> Result<(TargetTriple, TargetMachine), String> {
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
            opt.backend(),
            RelocMode::PIC,
            CodeModel::Default,
        )
        .ok_or_else(|| format!("[E0904] could not create target machine for '{triple_str}'"))?;
    Ok((triple, machine))
}

/// `TargetMachine` for a hosted program: the native host when `target_triple`
/// is `None`, else the cross triple (PIC).
fn hosted_target_machine(
    target_triple: Option<&str>,
    opt: OptLevel,
) -> Result<(TargetTriple, TargetMachine), String> {
    if let Some(triple_str) = target_triple {
        return pic_target_machine(triple_str, opt);
    }
    Target::initialize_native(&InitializationConfig::default())
        .map_err(|e| format!("LLVM native target init: {e}"))?;
    let triple = TargetMachine::get_default_triple();
    let target = Target::from_triple(&triple).map_err(|e| format!("get native target: {e}"))?;
    let machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            opt.backend(),
            RelocMode::Default,
            CodeModel::Default,
        )
        .ok_or_else(|| "failed to create native target machine".to_string())?;
    Ok((triple, machine))
}

// ── Crate-private surface (callable from super::Codegen) ─────────────────────

/// Prepare a hosted program module for object emission: module triple set,
/// program functions internalised (AX-22), the `opt` IR pipeline run (AX-21).
/// Returns the target machine that emits the object.
///
/// When `target_triple` is `None` the native host triple is used.  When it is
/// `Some(triple)` all LLVM backends are initialized and the specified triple is
/// used (cross-compilation).
fn prepare_hosted_module(
    module: &inkwell::module::Module<'_>,
    opt: OptLevel,
    target_triple: Option<&str>,
) -> Result<TargetMachine, String> {
    let (triple, machine) = hosted_target_machine(target_triple, opt)?;
    // Update the module's target triple so the emitted object is correct.
    module.set_triple(&triple);
    internalize_program_functions(module);
    optimize_module(module, &machine, opt)?;
    Ok(machine)
}

/// Emit a hosted program's object file at `obj_path`, without linking
/// (`axon build --emit-obj`, AX-23). The module is prepared exactly as for
/// [`emit_hosted_object_bytes`].
pub(super) fn emit_hosted_object(
    module: &inkwell::module::Module<'_>,
    obj_path: &str,
    opt: OptLevel,
    target_triple: Option<&str>,
) -> Result<(), String> {
    let machine = prepare_hosted_module(module, opt, target_triple)?;
    machine
        .write_to_file(module, FileType::Object, Path::new(obj_path))
        .map_err(|e| format!("object emit: {e}"))
}

/// The hosted program's object, in memory, plus the runtime it links. The
/// runtime is read off the module AFTER the pipeline, so a call the optimiser
/// proved dead does not pull in the AI runtime. Run
/// [`prune_unreachable_ai_callers`] first.
pub(super) fn emit_hosted_object_bytes(
    module: &inkwell::module::Module<'_>,
    opt: OptLevel,
    target_triple: Option<&str>,
) -> Result<HostedObject, String> {
    let machine = prepare_hosted_module(module, opt, target_triple)?;
    let buf = machine
        .write_to_memory_buffer(module, FileType::Object)
        .map_err(|e| format!("object emit: {e}"))?;
    Ok(HostedObject {
        bytes: buf.as_slice().to_vec(),
        links_ai_runtime: Runtime::for_module(module) == Runtime::WithAi,
    })
}

/// `--emit-llvm`: apply to `module` the transformations the object-emitting
/// path for this build would apply before code generation (target triple and
/// data layout, internalisation for a hosted program, the `opt` IR pipeline),
/// so the dumped IR is the IR that gets compiled. `O0` leaves the module as
/// emitted.
pub(super) fn optimize_for_ir_dump(
    module: &inkwell::module::Module<'_>,
    opt: OptLevel,
    target_triple: Option<&str>,
    freestanding: bool,
    shared: bool,
) -> Result<(), String> {
    if !opt.is_optimized() {
        return Ok(());
    }
    let (triple, machine) = if freestanding {
        freestanding_target_machine(target_triple.unwrap_or("x86_64-unknown-none"), opt)?
    } else if shared {
        let triple_str = target_triple.ok_or_else(|| {
            "[E1710] --host mobile requires an Android --target (e.g. aarch64-linux-android)"
                .to_string()
        })?;
        pic_target_machine(triple_str, opt)?
    } else {
        hosted_target_machine(target_triple, opt)?
    };
    module.set_triple(&triple);
    if freestanding {
        mark_no_builtins(module);
    } else if !shared {
        internalize_program_functions(module);
    }
    optimize_module(module, &machine, opt)
}

/// Link the hosted program object at `obj_path` into a binary at
/// `output_path` against the runtime staticlib `rt`.
fn link_object_file(
    obj_path: &str,
    output_path: &str,
    opt: OptLevel,
    target_triple: Option<&str>,
    rt: Runtime,
) -> Result<(), String> {
    // Optimised builds link the release-profile runtime staticlibs.
    let release = opt.is_optimized();

    // R14 slice 1/2: Android (bionic) cross-link via the NDK clang. Android
    // ELFs are PIE and link bionic libc, not glibc — the host `-no-pie`/`-lm`
    // recipe does not apply. The axon-rt staticlib must be the ANDROID-triple
    // cross-build, not the host one. Handled in a dedicated path so the host
    // link recipe below stays exactly as it was.
    if let Some(triple_str) = target_triple {
        if is_android_triple(triple_str) {
            return android_link(obj_path, output_path, release, triple_str, false, rt);
        }
    }

    // The native runtime: ONE staticlib, the AI-capable one only when the
    // program calls an AI builtin (`Runtime`). Resolved before the linker runs
    // so a missing runtime is one clear error naming where it was looked for,
    // not a page of `undefined reference to __axon_*` (AX-09).
    let rt_lib = runtime_staticlib(rt, release, None)?;

    // Determine linker: prefer the cross.toml override, else probe the host.
    let linker_override = target_triple.and_then(read_cross_linker);
    let linker = if let Some(l) = &linker_override {
        std::path::PathBuf::from(l)
    } else {
        which::which("cc")
            .or_else(|_| which::which("clang"))
            .or_else(|_| which::which("gcc"))
            .map_err(|_| "no C compiler found (tried cc, clang, gcc)".to_string())?
    };

    // `-no-pie`: our emitted object uses non-PIC relocations (R_X86_64_32S),
    // so the default PIE link fails ("can not be used when making a PIE
    // object"). (R1: surfaced once the native build actually produced objects
    // — see BUILD_RESOLVED.md.)
    // `-lpthread -lm` AFTER the runtime: its threads and math builtins
    // (`__axon_pow` → `pow`) are what reference them, and a static link
    // resolves left to right.
    // `-Wl,--gc-sections`: the runtime is compiled one section per function,
    // so this keeps only what the program reaches instead of every builtin
    // and all of std's formatting/unwinding machinery they could pull in.
    // `--release` also drops debug info (`--strip-debug`, which keeps the
    // symbol table for profilers and backtraces).
    let mut link_args: Vec<&str> = vec![
        obj_path,
        rt_lib.as_str(),
        "-o",
        output_path,
        "-no-pie",
        "-Wl,--gc-sections",
        "-lpthread",
        "-lm",
    ];
    if release {
        link_args.push("-Wl,--strip-debug");
    }

    let status = Command::new(&linker)
        .args(&link_args)
        .status()
        .map_err(|e| format!("linker spawn: {e}"))?;

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
    opt: OptLevel,
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

    let (triple, machine) = pic_target_machine(triple_str, opt)?;
    module.set_triple(&triple);
    optimize_module(module, &machine, opt)?;

    let obj_path = format!("{output_path}.o");
    machine
        .write_to_file(module, FileType::Object, Path::new(&obj_path))
        .map_err(|e| format!("object emit: {e}"))?;

    // A shared library's entry points are called from outside the module, so
    // nothing is pruned and an AI call anywhere in it is reachable.
    let rt = Runtime::for_module(module);
    let release = opt.is_optimized();
    let res = android_link(&obj_path, output_path, release, triple_str, true, rt);
    let _ = std::fs::remove_file(&obj_path);
    res
}

/// Emit just the object file for a freestanding build (no linking step).
/// Used by `axon build --freestanding --emit-obj` to let the caller supply
/// a boot stub object and run the final link manually.
pub(super) fn emit_freestanding_obj(
    module: &inkwell::module::Module<'_>,
    output_path: &str,
    opt: OptLevel,
    target_triple: Option<&str>,
) -> Result<(), String> {
    let triple_str = target_triple.unwrap_or("x86_64-unknown-none");
    let (triple, machine) = freestanding_target_machine(triple_str, opt)?;
    module.set_triple(&triple);
    mark_no_builtins(module);
    optimize_module(module, &machine, opt)?;
    machine
        .write_to_file(module, FileType::Object, Path::new(output_path))
        .map_err(|e| format!("freestanding object emit: {e}"))
}

/// `TargetMachine` for a freestanding (bare-metal) triple.
fn freestanding_target_machine(
    triple_str: &str,
    opt: OptLevel,
) -> Result<(TargetTriple, TargetMachine), String> {
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
        .create_target_machine(&triple, "generic", "", opt.backend(), reloc, code_model)
        .ok_or_else(|| format!("[E0904] could not create target machine for '{triple_str}'"))?;
    Ok((triple, machine))
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
    opt: OptLevel,
    target_triple: Option<&str>,
    entry_fn: Option<&str>,
    linker_script: Option<&str>,
) -> Result<(), String> {
    let triple_str = target_triple.unwrap_or("x86_64-unknown-none");
    let (triple, machine) = freestanding_target_machine(triple_str, opt)?;

    module.set_triple(&triple);
    mark_no_builtins(module);
    optimize_module(module, &machine, opt)?;

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

// ── Native runtime staticlib ─────────────────────────────────────────────────

/// Prefix of every symbol `axon-ai` defines (`__axon_ai_complete`, …).
const AI_SYMBOL_PREFIX: &[u8] = b"__axon_ai_";

/// Directory of prebuilt runtime staticlibs; when set, nothing else is searched.
const RUNTIME_DIR_VAR: &str = "AXON_RUNTIME_DIR";

/// The native runtime a hosted binary links. Exactly ONE is linked: both are
/// Rust staticlibs, each embedding its own copy of `std`, and linking two of
/// them is what used to need `-Wl,--allow-multiple-definition` (BUG_HUNT #43)
/// and doubled every binary's runtime (AX-11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Runtime {
    /// `libaxon_rt.a` (crates/axon-rt): every `__axon_*` builtin but the AI ones.
    Base,
    /// `libaxon_rt_ai.a` (crates/axon-rt-ai): axon-rt + axon-ai (reqwest,
    /// rustls) over one std, for a program that calls an AI builtin.
    WithAi,
}

impl Runtime {
    /// The runtime `module` needs: `WithAi` iff some `__axon_ai_*` extern is
    /// still called. Run [`prune_unreachable_ai_callers`] first, or the AI
    /// wrappers `declare_builtins` emits into every module count as calls.
    pub(super) fn for_module(module: &inkwell::module::Module<'_>) -> Self {
        let calls_ai = module
            .get_functions()
            .any(|f| is_ai_extern(f) && f.as_global_value().get_first_use().is_some());
        if calls_ai {
            Runtime::WithAi
        } else {
            Runtime::Base
        }
    }

    fn package(self) -> &'static str {
        match self {
            Runtime::Base => "axon-rt",
            Runtime::WithAi => "axon-rt-ai",
        }
    }

    fn libname(self) -> &'static str {
        match self {
            Runtime::Base => "libaxon_rt.a",
            Runtime::WithAi => "libaxon_rt_ai.a",
        }
    }
}

fn is_ai_extern(f: inkwell::values::FunctionValue<'_>) -> bool {
    f.count_basic_blocks() == 0 && f.get_name().to_bytes().starts_with(AI_SYMBOL_PREFIX)
}

/// Delete the definitions that reach an `__axon_ai_*` extern but are never
/// called, to a fixpoint, so [`Runtime::for_module`] sees only real AI use.
///
/// `declare_builtins` emits the `ai_complete`/`ai_extract_*` wrappers into
/// EVERY module, so without this even `hello` references `__axon_ai_complete`
/// and must link the AI runtime. The criterion is the one
/// `prune_dead_functions` uses on wasm (a body, zero uses, not `main`; an
/// address-taken function has a use and is kept), narrowed to the functions
/// that reach the AI externs so the rest of the native module is untouched.
/// For executables only: a shared library's entry points have no in-module
/// callers and must not be pruned.
pub(super) fn prune_unreachable_ai_callers(module: &inkwell::module::Module<'_>) {
    loop {
        let dead: Vec<_> = functions_reaching_ai(module)
            .into_iter()
            .filter(|f| {
                f.count_basic_blocks() > 0
                    && f.get_name().to_bytes() != b"main"
                    && f.as_global_value().get_first_use().is_none()
            })
            .collect();
        if dead.is_empty() {
            return;
        }
        for f in dead {
            // SAFETY: `f` has no uses, so no instruction refers to it.
            unsafe { f.delete() };
        }
    }
}

/// The `__axon_ai_*` externs plus every function that calls one, directly or
/// through other functions. A use that is not an instruction (a constant
/// expression) is not followed, which leaves its extern in use: conservative.
fn functions_reaching_ai<'ctx>(
    module: &inkwell::module::Module<'ctx>,
) -> Vec<inkwell::values::FunctionValue<'ctx>> {
    let mut reach: Vec<_> = module
        .get_functions()
        .filter(|f| is_ai_extern(*f))
        .collect();
    let mut i = 0;
    while i < reach.len() {
        let callee = reach[i].as_global_value();
        let mut next_use = callee.get_first_use();
        while let Some(u) = next_use {
            // inkwell classifies a user by its TYPE, so a call returning i32 is
            // an `IntValue`, not an `InstructionValue`; recover the instruction.
            let inst = match u.get_user() {
                AnyValueEnum::InstructionValue(inst) => Some(inst),
                other => BasicValueEnum::try_from(other)
                    .ok()
                    .and_then(|v| v.as_instruction_value()),
            };
            // inkwell ties a use's lifetime to the borrowed callee, not to the
            // context, so re-fetch the caller from the module by name.
            let caller = inst
                .and_then(|inst| inst.get_parent())
                .and_then(|bb| bb.get_parent())
                .and_then(|f| {
                    f.get_name()
                        .to_str()
                        .ok()
                        .and_then(|n| module.get_function(n))
                });
            if let Some(caller) = caller {
                if !reach.contains(&caller) {
                    reach.push(caller);
                }
            }
            next_use = u.get_next_use();
        }
        i += 1;
    }
    reach
}

/// Path of the runtime staticlib for `rt`, built if needed. Independent of the
/// current directory and of `PATH` (AX-09, AUDIT T11):
///
/// 1. `$AXON_RUNTIME_DIR/<lib>` (`<dir>/<triple>/<lib>` for a cross `target`).
///    An explicit choice, so nothing else is tried.
/// 2. The Axon source workspace this compiler was built from: `cargo build -p
///    <pkg>` run FROM the workspace root, so its `rust-toolchain.toml` selects
///    the toolchain (run from elsewhere, rustup picks another toolchain or
///    none). Cargo is a no-op when the lib is fresh and rebuilds it when the
///    runtime sources changed, so a dev compiler never links a stale runtime.
///    Cargo is `$CARGO`, then `PATH`, then `$CARGO_HOME/bin`/`~/.cargo/bin`.
/// 3. A prebuilt lib where a compiler is built or installed (see
///    [`prebuilt_candidates`]) — when there is no workspace, no cargo, or the
///    build failed (then with a warning, since that lib may be stale).
///
/// If none yields the lib, the error names every place searched; linking on
/// without the runtime would only bury that under `undefined reference`s.
fn runtime_staticlib(rt: Runtime, release: bool, target: Option<&str>) -> Result<String, String> {
    let lib = rt.libname();
    let profile = if release { "release" } else { "debug" };
    let how_to_build = format!(
        "build it in the Axon workspace with `cargo build -p {}{}{}`",
        rt.package(),
        if release { " --release" } else { "" },
        target.map(|t| format!(" --target {t}")).unwrap_or_default()
    );

    if let Some(dir) = std::env::var_os(RUNTIME_DIR_VAR).filter(|d| !d.is_empty()) {
        let dir = std::path::PathBuf::from(dir);
        let path = match target {
            Some(t) => dir.join(t).join(lib),
            None => dir.join(lib),
        };
        return if path.is_file() {
            Ok(path.display().to_string())
        } else {
            Err(format!(
                "native runtime `{lib}` not found at {} ({RUNTIME_DIR_VAR}={}); \
                 {how_to_build}, or unset {RUNTIME_DIR_VAR}",
                path.display(),
                dir.display()
            ))
        };
    }

    let root = workspace_root();
    let build_failure = match (&root, find_cargo()) {
        (Some(root), Some(cargo)) => match cargo_build_runtime(&cargo, root, rt, release, target) {
            Ok(path) => return Ok(path),
            Err(e) => Some(e),
        },
        _ => None,
    };

    let candidates = prebuilt_candidates(rt, profile, target, root.as_deref());
    if let Some(found) = candidates.iter().find(|p| p.is_file()) {
        if let Some(e) = &build_failure {
            eprintln!(
                "warning: could not rebuild the native runtime ({}); linking the prebuilt {}, \
                 which may predate the runtime sources",
                e.lines().next().unwrap_or_default(),
                found.display()
            );
        }
        return Ok(found.display().to_string());
    }

    let build_note = match (&root, &build_failure) {
        (_, Some(e)) => format!("building it failed: {e}"),
        (Some(r), None) => format!(
            "the Axon workspace is at {}, but no `cargo` was found ($CARGO, PATH, \
             $CARGO_HOME/bin, ~/.cargo/bin) to build it",
            r.display()
        ),
        (None, None) => format!(
            "no Axon source workspace to build it from (looked at {} and the \
             directories above the axon executable)",
            baked_workspace_dir().display()
        ),
    };
    let searched: String = candidates
        .iter()
        .map(|p| format!("\n    {}", p.display()))
        .collect();
    Err(format!(
        "native runtime `{lib}` (crate {}) not found, so the program cannot be linked.\n  \
         {build_note}\n  searched:{searched}\n  \
         Fix: {how_to_build}, or set {RUNTIME_DIR_VAR} to a directory containing {lib}.",
        rt.package()
    ))
}

/// The workspace directory baked in at compile time (`crates/axon-core/../..`).
fn baked_workspace_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Root of the Axon source workspace: the one this compiler was compiled from
/// if it still exists, else the first directory above the executable that is
/// one (an installed or copied binary). Never the current directory.
fn workspace_root() -> Option<std::path::PathBuf> {
    let is_axon_workspace = |d: &Path| {
        d.join("Cargo.toml").is_file()
            && d.join("crates")
                .join("axon-rt")
                .join("Cargo.toml")
                .is_file()
    };
    let baked = baked_workspace_dir();
    if is_axon_workspace(&baked) {
        return Some(baked.canonicalize().unwrap_or(baked));
    }
    let exe = std::env::current_exe().ok()?;
    exe.ancestors()
        .skip(1)
        .find(|d| is_axon_workspace(d))
        .map(Path::to_path_buf)
}

/// `$CARGO` (set when run under cargo), then `cargo` on `PATH`, then the rustup
/// install location — so a `PATH` without `~/.cargo/bin` still finds it.
fn find_cargo() -> Option<std::path::PathBuf> {
    if let Some(c) = std::env::var_os("CARGO").map(std::path::PathBuf::from) {
        if c.is_file() {
            return Some(c);
        }
    }
    if let Ok(c) = which::which("cargo") {
        return Some(c);
    }
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".cargo")))?;
    Some(cargo_home.join("bin").join("cargo")).filter(|c| c.is_file())
}

/// Places a prebuilt `rt` staticlib can sit, most specific first: the cargo
/// target dirs (`$CARGO_TARGET_DIR`, `<workspace>/target`, and the one the
/// running executable was built into, `<target>/<profile>/axon`) for the wanted
/// profile; for a host build the executable's own directory and
/// `<exe>/../lib/axon` (an install layout); then the other profile, whose
/// runtime is functionally identical, only differently optimised.
fn prebuilt_candidates(
    rt: Runtime,
    profile: &str,
    target: Option<&str>,
    root: Option<&Path>,
) -> Vec<std::path::PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    let mut target_dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Some(d) = std::env::var_os("CARGO_TARGET_DIR") {
        target_dirs.push(d.into());
    }
    if let Some(r) = root {
        target_dirs.push(r.join("target"));
    }
    if let Some(d) = exe_dir.as_deref().and_then(Path::parent) {
        target_dirs.push(d.to_path_buf());
    }
    let in_profile = |p: &str| -> Vec<std::path::PathBuf> {
        target_dirs
            .iter()
            .map(|t| match target {
                Some(triple) => t.join(triple).join(p),
                None => t.join(p),
            })
            .collect()
    };

    let mut dirs = in_profile(profile);
    if target.is_none() {
        if let Some(e) = &exe_dir {
            dirs.push(e.clone());
            dirs.push(e.join("..").join("lib").join("axon"));
        }
    }
    dirs.extend(in_profile(if profile == "release" {
        "debug"
    } else {
        "release"
    }));

    let mut out: Vec<std::path::PathBuf> = Vec::new();
    for d in dirs {
        let p = d.join(rt.libname());
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// `cargo build -p <pkg>` in the workspace at `root`, returning the staticlib
/// path cargo reports (so `CARGO_TARGET_DIR` and `.cargo/config` target dirs
/// are honoured without guessing). When `target` is `Some(triple)` the crate is
/// cross-built for it (R14: the Android cross-build).
fn cargo_build_runtime(
    cargo: &Path,
    root: &Path,
    rt: Runtime,
    release: bool,
    target: Option<&str>,
) -> Result<String, String> {
    let mut cmd = Command::new(cargo);
    cmd.current_dir(root)
        .args([
            "build",
            "-p",
            rt.package(),
            "--message-format=json-render-diagnostics",
        ])
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"));
    if release {
        cmd.arg("--release");
    }
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
    let out = cmd
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("could not run {}: {e}", cargo.display()))?;
    let command = format!(
        "`{} build -p {}` in {}",
        cargo.display(),
        rt.package(),
        root.display()
    );
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let lines: Vec<&str> = stderr.lines().collect();
        let tail = lines[lines.len().saturating_sub(15)..].join("\n      ");
        return Err(format!(
            "{command} exited with {}:\n      {tail}",
            out.status
        ));
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    for line in stdout.lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if msg["reason"] != "compiler-artifact" {
            continue;
        }
        let files = msg["filenames"].as_array().into_iter().flatten();
        for f in files.filter_map(|f| f.as_str()) {
            if Path::new(f).file_name() == Some(std::ffi::OsStr::new(rt.libname())) {
                return Ok(f.to_string());
            }
        }
    }
    Err(format!(
        "{command} succeeded but reported no {}",
        rt.libname()
    ))
}

/// R14 slice 1/2: link an Android object into a runnable bionic ELF executable.
///
/// Uses the NDK clang as the linker (cross.toml override or NDK auto-detect),
/// links the Android-triple cross-build of the native runtime `rt`, and
/// produces a PIE ELF (the Android ABI requirement). Produces an E1710 if the
/// NDK is absent and E1712 if the runtime cannot be found or the link fails.
fn android_link(
    obj_path: &str,
    output_path: &str,
    release: bool,
    triple: &str,
    shared: bool,
    rt: Runtime,
) -> Result<(), String> {
    // Default NDK API level. The minSdk for Axon mobile artifacts; overridable
    // via $AXON_ANDROID_API.
    let api: u32 = std::env::var("AXON_ANDROID_API")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(34);

    let linker = android_linker(triple, api)?;

    // The Android-triple cross-build of the runtime. Channel/spawn/gfx-mock
    // builtins resolve to axon-rt; ai_complete/ai_extract_* to the axon-ai half
    // of `Runtime::WithAi`.
    let rt_lib = runtime_staticlib(rt, release, Some(triple))
        .map_err(|e| format!("[E1712] mobile link failed for '{triple}': {e}"))?;

    // The NDK clang already knows the bionic sysroot, PIE, and libm. We do NOT
    // pass -no-pie (Android requires PIE) and let clang supply libc. `-lm` goes
    // after the runtime, which is what references it.
    let mut args: Vec<String> = vec![
        obj_path.to_string(),
        "-o".to_string(),
        output_path.to_string(),
    ];
    if shared {
        // R14: a loadable JNI shared object. Keep the Axon entry symbols
        // (`main`/on_start/…) globally visible so the Kotlin wrapper can bind
        // them; the staticlib is linked whole so __axon_* resolve.
        args.push("-shared".to_string());
        args.push("-Wl,--export-dynamic".to_string());
    }
    args.push(rt_lib);
    args.push("-lm".to_string());

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
