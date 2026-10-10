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

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::time_passes;

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
/// pipelines with the `Default` backend level and mark every definition
/// `optsize` (`Os`) or `optsize minsize` (`Oz`), as clang does: LLVM's size
/// heuristics (inline cost, loop unrolling, block placement, instruction
/// selection) key on those attributes, not on the pipeline name (AX-38). `O0`
/// runs only `globaldce`, which deletes the builtin helpers nothing calls and
/// transforms no surviving function (AX-37).
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

    /// The new-pass-manager pipeline for this level.
    fn pipeline(self) -> &'static str {
        match self {
            // Dead internal definitions only: the unoptimised code that remains
            // is exactly what codegen emitted, so a debug build stays
            // debuggable without carrying ~70 unused builtin wrappers and the
            // runtime code they reference (AX-37).
            Self::O0 => "globaldce",
            Self::O1 => "default<O1>",
            Self::O2 => "default<O2>",
            Self::O3 => "default<O3>",
            Self::Os => "default<Os>",
            Self::Oz => "default<Oz>",
        }
    }

    /// The enum function attributes clang puts on every definition at this
    /// level (`-Os` → `optsize`, `-Oz` → `optsize minsize`).
    fn size_attributes(self) -> &'static [&'static str] {
        match self {
            Self::Os => &["optsize"],
            Self::Oz => &["optsize", "minsize"],
            Self::O0 | Self::O1 | Self::O2 | Self::O3 => &[],
        }
    }
}

/// AX-21: run the IR pass pipeline for `opt` on `module`, verifying the module
/// before and after. The pipeline is target-aware, so the caller sets the
/// module triple first; the data layout is taken from `machine` here (codegen
/// emits none, and the passes would otherwise lay out types with LLVM's
/// target-neutral default, e.g. 4-byte-aligned i64). At `O0` the pipeline is
/// `globaldce` alone (AX-37), unverified: the module was verified when codegen
/// finished it (cached bitcode is such a module), a pass that only deletes
/// unreferenced functions cannot make it invalid, and two more verifier walks
/// would only slow every debug build.
///
/// Every definition is marked `"disable-tail-calls"="true"` first, so each
/// Axon call keeps its stack frame. Unbounded recursion must stay a graceful
/// "stack overflow" exit 101 (the runtime's guard-page handler), matching the
/// interpreter's recursion-limit panic; tail-recursion elimination would turn
/// `fn rec(n: i64) -> i64 { rec(n + 1) }` into a loop spinning ~2^63 times,
/// and backend sibling calls would do the same to mutual recursion. The
/// attribute switches off both. The size levels also get their
/// `optsize`/`minsize` attributes here (AX-38).
fn optimize_module(
    module: &Module<'_>,
    machine: &TargetMachine,
    opt: OptLevel,
) -> Result<(), String> {
    let pipeline = opt.pipeline();
    time_passes::time("ir_opt", || {
        let context = module.get_context();
        mark_definitions(
            module,
            context.create_string_attribute("disable-tail-calls", "true"),
        );
        for name in opt.size_attributes() {
            let kind = Attribute::get_named_enum_kind_id(name);
            debug_assert_ne!(kind, 0, "LLVM has no `{name}` attribute");
            mark_definitions(module, context.create_enum_attribute(kind, 0));
        }
        module.set_data_layout(&machine.get_target_data().get_data_layout());
        let verify = opt.is_optimized();
        if verify {
            module.verify().map_err(|e| {
                format!(
                    "IR verification failed before `{pipeline}`: {}",
                    e.to_string()
                )
            })?;
        }
        module
            .run_passes(pipeline, machine, PassBuilderOptions::create())
            .map_err(|e| format!("LLVM pass pipeline `{pipeline}` failed: {}", e.to_string()))?;
        if verify {
            module.verify().map_err(|e| {
                format!(
                    "IR verification failed after `{pipeline}`: {}",
                    e.to_string()
                )
            })?;
        }
        Ok(())
    })
}

/// AX-36: writing the object file (instruction selection, register
/// allocation, encoding) is the `backend` phase of `--time-passes`.
fn write_object(
    machine: &TargetMachine,
    module: &Module<'_>,
    path: &Path,
) -> Result<(), inkwell::support::LLVMString> {
    time_passes::time("backend", || {
        machine.write_to_file(module, FileType::Object, path)
    })
}

/// AX-36: a linker invocation is the `link` phase of `--time-passes`.
fn run_linker(cmd: &mut Command) -> std::io::Result<ExitStatus> {
    time_passes::time("link", || cmd.status())
}

/// Freestanding output links no libc, so the optimiser must not synthesise
/// libc calls (loop-idiom recognition turning a fill/copy loop into
/// `memset`/`memcpy`, printf→puts, …). `"no-builtins"` on every definition is
/// what clang's `-ffreestanding` emits for the same reason.
fn mark_no_builtins(module: &Module<'_>) {
    mark_definitions(
        module,
        module
            .get_context()
            .create_string_attribute("no-builtins", ""),
    );
}

/// Add the function attribute `attr` to every function definition in `module`
/// (declarations are left alone).
fn mark_definitions(module: &Module<'_>, attr: Attribute) {
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
    time_passes::time("ir_opt", || {
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
    })
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
    write_object(&machine, module, Path::new(output_path))
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
    write_object(&machine, module, Path::new(output_path))
        .map_err(|e| format!("mobile object emit: {e}"))
}

/// `TargetMachine` for an explicit (cross/device) triple: all backends
/// initialised, PIC relocations, default code model.
fn pic_target_machine(
    triple_str: &str,
    opt: OptLevel,
) -> Result<(TargetTriple, TargetMachine), String> {
    time_passes::time("target_init", || {
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
    })
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
    time_passes::time("target_init", || {
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
    })
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
    write_object(&machine, module, Path::new(obj_path)).map_err(|e| format!("object emit: {e}"))
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
    let buf = time_passes::time("backend", || {
        machine.write_to_memory_buffer(module, FileType::Object)
    })
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
/// emitted (codegen's raw IR, every builtin helper included), which the golden
/// IR tests read; the `O0` object additionally drops the dead helpers.
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
    let rt_lib = time_passes::time("runtime", || runtime_staticlib(rt, release, None))?;

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

    let status = run_linker(Command::new(&linker).args(&link_args))
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
    write_object(&machine, module, Path::new(&obj_path))
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
    write_object(&machine, module, Path::new(output_path))
        .map_err(|e| format!("freestanding object emit: {e}"))
}

/// `TargetMachine` for a freestanding (bare-metal) triple.
fn freestanding_target_machine(
    triple_str: &str,
    opt: OptLevel,
) -> Result<(TargetTriple, TargetMachine), String> {
    time_passes::time("target_init", || {
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
    })
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
    write_object(&machine, module, Path::new(&obj_path))
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
        run_linker(Command::new(&ld).args(&args))
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
        run_linker(Command::new(&cc).args(&args))
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

/// Path of the runtime staticlib for `rt`. Independent of the current
/// directory, of `PATH` (AX-09) and of the caller's Rust toolchain (AX-35):
/// when a usable runtime is already available, no cargo runs. The order is
/// [`RuntimeLookup::resolve`]'s.
fn runtime_staticlib(rt: Runtime, release: bool, target: Option<&str>) -> Result<String, String> {
    RuntimeLookup::from_env(rt, release, target).resolve(|root, target_dir| {
        let cargo = find_cargo().ok_or_else(|| {
            "no `cargo` was found ($CARGO_HOME/bin, ~/.cargo/bin, PATH, $CARGO) to rebuild it"
                .to_string()
        })?;
        cargo_build_runtime(&cargo, root, target_dir, rt, release, target)
    })
}

/// What the runtime lookup depends on, read from the process once so that the
/// order itself is testable.
struct RuntimeLookup<'a> {
    rt: Runtime,
    release: bool,
    /// The cross `--target` triple; `None` for the host.
    target: Option<&'a str>,
    /// `$AXON_RUNTIME_DIR`, when set and non-empty.
    runtime_dir: Option<PathBuf>,
    /// The running compiler executable.
    exe: Option<PathBuf>,
    /// The Axon source workspace ([`workspace_root`]).
    root: Option<PathBuf>,
}

impl<'a> RuntimeLookup<'a> {
    fn from_env(rt: Runtime, release: bool, target: Option<&'a str>) -> Self {
        RuntimeLookup {
            rt,
            release,
            target,
            runtime_dir: std::env::var_os(RUNTIME_DIR_VAR)
                .filter(|d| !d.is_empty())
                .map(PathBuf::from),
            exe: std::env::current_exe().ok(),
            root: workspace_root(),
        }
    }

    /// The install layout beside the compiler (`scripts/install.sh`):
    /// `<bin>/../lib/axon/runtime/<profile>[/<triple>]`, `<bin>` being the
    /// executable's directory. Each profile directory has the shape
    /// `AXON_RUNTIME_DIR` expects.
    fn installed_dir(&self, profile: &str) -> Option<PathBuf> {
        let bin = self.exe.as_deref()?.parent()?;
        let dir = bin
            .join("..")
            .join("lib")
            .join("axon")
            .join("runtime")
            .join(profile);
        Some(match self.target {
            Some(t) => dir.join(t),
            None => dir,
        })
    }

    /// Where cargo puts the runtime in the workspace target dir `target_dir`:
    /// `<target_dir>[/<triple>]/<profile>`.
    fn built_dir(&self, target_dir: &Path, profile: &str) -> PathBuf {
        match self.target {
            Some(t) => target_dir.join(t).join(profile),
            None => target_dir.join(profile),
        }
    }

    /// 1. `$AXON_RUNTIME_DIR/<lib>` (`<dir>/<triple>/<lib>` for a cross
    ///    `target`). An explicit choice, so nothing else is tried.
    /// 2. The install layout beside the compiler ([`Self::installed_dir`]):
    ///    the runtime installed together with THIS compiler, linked as is.
    /// 3. The Axon source workspace this compiler was built from: the lib in
    ///    its target dir ([`workspace_target_dir`]) when [`runtime_freshness`]
    ///    finds it current, with no cargo run. Otherwise `rebuild(root,
    ///    target_dir)` — [`cargo_build_runtime`], pinned to the workspace's
    ///    toolchain — and the lib it returns is stamped current.
    /// 4. When 3 could not produce the lib: the install layout in the other
    ///    profile (the same runtime, only differently optimised), then the
    ///    workspace lib of either profile, with a warning saying why when it
    ///    is not current — a stale runtime is never linked silently.
    ///
    /// If none yields the lib, ONE error names every place searched; linking
    /// on without the runtime would only bury that under `undefined
    /// reference`s.
    fn resolve(
        &self,
        rebuild: impl FnOnce(&Path, &Path) -> Result<String, String>,
    ) -> Result<String, String> {
        let lib = self.rt.libname();
        let (profile, other) = if self.release {
            ("release", "debug")
        } else {
            ("debug", "release")
        };
        let how_to_build = format!(
            "build it in the Axon workspace with `cargo build -p {}{}{}`",
            self.rt.package(),
            if self.release { " --release" } else { "" },
            self.target
                .map(|t| format!(" --target {t}"))
                .unwrap_or_default()
        );

        if let Some(dir) = &self.runtime_dir {
            let path = match self.target {
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

        let installed = |p: &str| self.installed_dir(p).map(|d| d.join(lib));
        if let Some(path) = installed(profile).filter(|p| p.is_file()) {
            return Ok(path.display().to_string());
        }

        let workspace = self
            .root
            .as_deref()
            .map(|root| (root, workspace_target_dir(root, self.exe.as_deref())));
        let mut rebuild_failure = None;
        if let Some((root, target_dir)) = &workspace {
            let built = self.built_dir(target_dir, profile).join(lib);
            // Taken before cargo runs: every input it can see is older.
            let checked_at = std::time::SystemTime::now();
            match runtime_freshness(&built, root) {
                Freshness::Current => return Ok(built.display().to_string()),
                Freshness::Stale(why) => match rebuild(root, target_dir) {
                    Ok(path) => {
                        stamp_as_current(Path::new(&path), checked_at);
                        return Ok(path);
                    }
                    Err(e) => {
                        rebuild_failure = Some(format!(
                            "the workspace runtime {} {why}, and rebuilding it failed: {e}",
                            built.display()
                        ))
                    }
                },
            }
        }

        let mut candidates: Vec<PathBuf> = Vec::new();
        candidates.extend(installed(profile));
        candidates.extend(installed(other));
        if let Some((_, target_dir)) = &workspace {
            candidates.push(self.built_dir(target_dir, profile).join(lib));
            candidates.push(self.built_dir(target_dir, other).join(lib));
        }
        let note = rebuild_failure.unwrap_or_else(|| {
            format!(
                "no Axon source workspace to build it from (looked at {} and the \
                 directories above the axon executable)",
                baked_workspace_dir().display()
            )
        });
        if let Some(found) = candidates.iter().find(|p| p.is_file()) {
            if let Some((root, target_dir)) = &workspace {
                if found.starts_with(target_dir) {
                    if let Freshness::Stale(why) = runtime_freshness(found, root) {
                        eprintln!(
                            "warning: linking the native runtime {}, which {why} ({})",
                            found.display(),
                            note.lines().next().unwrap_or_default()
                        );
                    }
                }
            }
            return Ok(found.display().to_string());
        }

        let searched: String = candidates
            .iter()
            .map(|p| format!("\n    {}", p.display()))
            .collect();
        Err(format!(
            "native runtime `{lib}` (crate {}) not found, so the program cannot be linked.\n  \
             {note}\n  searched:{searched}\n  \
             Fix: {how_to_build}, or set {RUNTIME_DIR_VAR} to a directory containing {lib}.",
            self.rt.package()
        ))
    }
}

/// Whether a runtime staticlib in the workspace target dir can be linked
/// without asking cargo.
#[derive(Debug, PartialEq, Eq)]
enum Freshness {
    Current,
    /// Why not, as a phrase completing "the runtime …".
    Stale(String),
}

/// Decide whether the workspace runtime `lib` can be linked as is, with a few
/// `stat`s and no cargo or rustc, so the check is cheap and depends on no
/// toolchain. It is current when BOTH hold:
///
/// - It carries the stamp [`stamp_as_current`] wrote after a pinned cargo
///   build ([`cargo_build_runtime`]) produced or confirmed it, for this very
///   file (device, inode, size, mtime). A lib that anything else rebuilt —
///   say `cargo build` under another `RUSTUP_TOOLCHAIN` — no longer matches,
///   so it is rebuilt with the pinned toolchain rather than linked.
/// - Nothing it was built from is newer than the lib or than the start of the
///   cargo run that stamped it (the stamp's mtime): the sources rustc read for
///   it (the dep-info `<lib>.d` cargo writes beside it lists every workspace
///   source of the crate and its path dependencies), those crates' manifests,
///   and the workspace's `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` and
///   `.cargo/config.toml`. An input cargo judged and left the lib alone for
///   (a comment in `.cargo/config.toml`) is older than the stamp.
///
/// That is cargo's own mtime rule over the same inputs, so editing a runtime
/// source in the workspace always rebuilds before the next link. What it
/// shares with cargo's rule: a source restored with an OLDER mtime than the
/// lib goes unnoticed.
fn runtime_freshness(lib: &Path, root: &Path) -> Freshness {
    let stale = |why: String| Freshness::Stale(why);
    let Ok(meta) = std::fs::metadata(lib) else {
        return stale("has not been built".into());
    };
    if std::fs::read_to_string(stamp_path(lib)).ok() != Some(lib_identity(&meta)) {
        return stale("was not built by `axon build` with the workspace's pinned toolchain".into());
    }
    let Ok(lib_time) = meta.modified() else {
        return stale("has no readable modification time".into());
    };
    // A cargo run that finds the lib fresh leaves its mtime alone, so an input
    // edited before that run (say a comment in `.cargo/config.toml`) would be
    // newer than the lib forever. The stamp's mtime is when that run started:
    // every input older than it, cargo has already judged.
    let checked = std::fs::metadata(stamp_path(lib))
        .and_then(|m| m.modified())
        .ok();
    let built = checked.map_or(lib_time, |t| t.max(lib_time));
    let dep_info = lib.with_extension("d");
    let sources = std::fs::read_to_string(&dep_info)
        .map(|d| dep_info_sources(&d))
        .unwrap_or_default();
    if sources.is_empty() {
        return stale(format!(
            "has no dep-info listing its sources at {}",
            dep_info.display()
        ));
    }

    let mut inputs: Vec<PathBuf> = ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml"]
        .iter()
        .map(|f| root.join(f))
        .chain(std::iter::once(root.join(".cargo").join("config.toml")))
        .filter(|p| p.exists())
        .collect();
    for src in &sources {
        let manifest = src
            .ancestors()
            .skip(1)
            .take_while(|d| d.starts_with(root))
            .map(|d| d.join("Cargo.toml"))
            .find(|m| m.is_file());
        if let Some(m) = manifest {
            if !inputs.contains(&m) {
                inputs.push(m);
            }
        }
    }
    inputs.extend(sources);
    for input in &inputs {
        match std::fs::metadata(input).and_then(|m| m.modified()) {
            Ok(t) if t <= built => {}
            Ok(_) => return stale(format!("is older than {}", input.display())),
            Err(_) => {
                return stale(format!(
                    "was built from {}, which no longer exists",
                    input.display()
                ))
            }
        }
    }
    Freshness::Current
}

/// The prerequisites of the first rule in a Makefile-style dep-info file
/// (`target: dep dep …`, spaces in paths escaped as `\ `).
fn dep_info_sources(dep_info: &str) -> Vec<PathBuf> {
    let line = dep_info.lines().next().unwrap_or_default();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&' ') => {
                word.push(' ');
                chars.next();
            }
            ' ' => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    match words.iter().position(|w| w.ends_with(':')) {
        Some(target) => words[target + 1..].iter().map(PathBuf::from).collect(),
        None => Vec::new(),
    }
}

/// `<lib>.axon-stamp`: see [`runtime_freshness`].
fn stamp_path(lib: &Path) -> PathBuf {
    let mut p = lib.as_os_str().to_owned();
    p.push(".axon-stamp");
    PathBuf::from(p)
}

/// The identity of the file `meta` describes. Rebuilding or replacing the lib
/// changes it: cargo writes a new file and hard-links it into place, while a
/// cargo run that finds the lib fresh leaves the same file there.
fn lib_identity(meta: &std::fs::Metadata) -> String {
    let mtime_ns = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::MetadataExt;
        format!("dev={} ino={} ", meta.dev(), meta.ino())
    };
    #[cfg(not(unix))]
    let file = String::new();
    format!(
        "axon-runtime-stamp/1 {file}size={} mtime_ns={mtime_ns}\n",
        meta.len()
    )
}

/// Record that `lib` is what a pinned cargo build started at `checked_at`
/// just produced or confirmed; the stamp's mtime is set to `checked_at`.
/// Best effort: a target dir that cannot take the stamp only costs the next
/// build another (no-op) cargo run.
fn stamp_as_current(lib: &Path, checked_at: std::time::SystemTime) {
    if let Ok(meta) = std::fs::metadata(lib) {
        let stamp = stamp_path(lib);
        if std::fs::write(&stamp, lib_identity(&meta)).is_ok() {
            if let Ok(f) = std::fs::File::options().write(true).open(&stamp) {
                let _ = f.set_modified(checked_at);
            }
        }
    }
}

/// The workspace directory baked in at compile time (`crates/axon-core/../..`).
fn baked_workspace_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

/// Root of the Axon source workspace: the one this compiler was compiled from
/// if it still exists, else the first directory above the executable that is
/// one (an installed or copied binary). Never the current directory.
fn workspace_root() -> Option<PathBuf> {
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

/// The cargo target dir whose runtime the workspace step links and, when it is
/// not current, rebuilds (`--target-dir`): the one the running compiler was
/// built into (`<dir>/<profile>/axon` or `<dir>/<triple>/<profile>/axon`,
/// recognised by the `CACHEDIR.TAG` cargo writes at a target dir's root), else
/// `<root>/target`. Not `$CARGO_TARGET_DIR`: which runtime a compiler links
/// must not depend on its caller's environment.
fn workspace_target_dir(root: &Path, exe: Option<&Path>) -> PathBuf {
    exe.and_then(Path::parent)
        .and_then(|profile_dir| {
            profile_dir
                .ancestors()
                .skip(1)
                .take(2)
                .find(|d| d.join("CACHEDIR.TAG").is_file())
        })
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("target"))
}

/// The cargo to rebuild the runtime with, rustup's proxy first because the
/// proxy is what honours the workspace's `rust-toolchain.toml`:
/// `$CARGO_HOME/bin/cargo` (`~/.cargo/bin/cargo`), then `cargo` on `PATH`, then
/// `$CARGO`. `$CARGO` comes last because rustup sets it to the CALLER's
/// toolchain binary (`~/.rustup/toolchains/<toolchain>/bin/cargo`), which
/// ignores the pin.
fn find_cargo() -> Option<PathBuf> {
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cargo")));
    if let Some(c) = cargo_home
        .map(|h| h.join("bin").join("cargo"))
        .filter(|c| c.is_file())
    {
        return Some(c);
    }
    if let Ok(c) = which::which("cargo") {
        return Some(c);
    }
    std::env::var_os("CARGO")
        .map(PathBuf::from)
        .filter(|c| c.is_file())
}

/// Whether the environment variable `var` would make the runtime cargo build
/// differ from what the workspace pins, so [`cargo_build_runtime`] removes it
/// and the runtime a program links does not depend on who runs `axon build`
/// (AX-35):
///
/// - `RUSTUP_TOOLCHAIN` outranks `rust-toolchain.toml`, and rustup exports it
///   (with `$CARGO`) into everything run under cargo, so an `axon build` from
///   another toolchain's `cargo run`/`cargo test`/build script inherits it.
/// - `RUSTC`/`CARGO_BUILD_RUSTC` name a rustc outside rustup; the wrappers
///   replace it (`cargo clippy` exports `RUSTC_WORKSPACE_WRAPPER`).
/// - The rustflags variables and `CARGO_PROFILE_*` change the code built, and
///   when they change, cargo rebuilds over the pinned artifacts.
/// - `CARGO_BUILD_TARGET` would cross-build a host runtime (a cross build
///   passes `--target`); `CARGO_BUILD_DEP_INFO_BASEDIR` would make the dep-info
///   [`runtime_freshness`] reads relative.
///
/// Kept: `RUSTUP_HOME`/`CARGO_HOME` (where rustup and the registry live, not
/// which toolchain), the Android NDK linker/`CC_*`/`AR_*` variables a cross
/// build needs, and the target-dir variables, which `--target-dir` overrides.
fn redirects_runtime_toolchain(var: &str) -> bool {
    matches!(
        var,
        "RUSTUP_TOOLCHAIN"
            | "RUSTC"
            | "CARGO_BUILD_RUSTC"
            | "RUSTC_WRAPPER"
            | "CARGO_BUILD_RUSTC_WRAPPER"
            | "RUSTC_WORKSPACE_WRAPPER"
            | "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"
            | "RUSTFLAGS"
            | "CARGO_ENCODED_RUSTFLAGS"
            | "CARGO_BUILD_RUSTFLAGS"
            | "CARGO_BUILD_TARGET"
            | "CARGO_BUILD_DEP_INFO_BASEDIR"
    ) || var.starts_with("CARGO_PROFILE_")
        || (var.starts_with("CARGO_TARGET_") && var.ends_with("_RUSTFLAGS"))
}

/// `cargo build -p <pkg> --target-dir <target_dir>` in the workspace at
/// `root`, with the workspace's pinned toolchain: run FROM `root`, so rustup
/// reads its `rust-toolchain.toml`, and without the variables
/// [`redirects_runtime_toolchain`] lists. Returns the staticlib path cargo
/// reports. When `target` is `Some(triple)` the crate is cross-built for it
/// (R14: the Android cross-build).
fn cargo_build_runtime(
    cargo: &Path,
    root: &Path,
    target_dir: &Path,
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
        .arg(root.join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target_dir);
    for (var, _) in std::env::vars_os() {
        if var.to_str().is_some_and(redirects_runtime_toolchain) {
            cmd.env_remove(var);
        }
    }
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
    let rt_lib = time_passes::time("runtime", || runtime_staticlib(rt, release, Some(triple)))
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

    let status = run_linker(Command::new(&linker).args(&args))
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

#[cfg(test)]
mod runtime_lookup_tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    const T0: u64 = 1_700_000_000;

    fn write(path: &Path, contents: &str, mtime: u64) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
        set_mtime(path, mtime);
    }

    fn set_mtime(path: &Path, secs: u64) {
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(secs))
            .unwrap();
    }

    fn no_cargo(_: &Path, _: &Path) -> Result<String, String> {
        panic!("cargo must not run when a usable runtime is available")
    }

    fn s(p: &Path) -> String {
        p.display().to_string()
    }

    /// A scratch workspace shaped like Axon's (sources, manifests, the
    /// toolchain pin, a cargo target dir holding the compiler), plus an
    /// install prefix.
    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("axon_rt_lookup_{name}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            let fx = Fixture { dir };
            let root = fx.root();
            write(&root.join("Cargo.toml"), "[workspace]\n", T0);
            write(&root.join("Cargo.lock"), "", T0);
            write(&root.join("rust-toolchain.toml"), "", T0);
            write(&root.join("crates/axon-rt/Cargo.toml"), "[package]\n", T0);
            write(&fx.source(), "", T0);
            write(&fx.target_dir().join("CACHEDIR.TAG"), "", T0);
            write(&fx.dev_compiler(), "", T0);
            fx
        }

        fn root(&self) -> PathBuf {
            self.dir.join("ws")
        }

        fn source(&self) -> PathBuf {
            self.root().join("crates/axon-rt/src/lib.rs")
        }

        fn target_dir(&self) -> PathBuf {
            self.root().join("target")
        }

        /// `cargo build -p axon-core --release`'s compiler.
        fn dev_compiler(&self) -> PathBuf {
            self.target_dir().join("release").join("axon")
        }

        /// A compiler installed under `<dir>/prefix`.
        fn installed_compiler(&self) -> PathBuf {
            let exe = self.dir.join("prefix/bin/axon");
            write(&exe, "", T0);
            exe
        }

        fn installed_lib(&self, profile: &str) -> PathBuf {
            self.dir
                .join("prefix/bin")
                .join("..")
                .join("lib/axon/runtime")
                .join(profile)
                .join("libaxon_rt.a")
        }

        fn workspace_lib(&self, profile: &str) -> PathBuf {
            self.target_dir().join(profile).join("libaxon_rt.a")
        }

        fn lookup(&self, exe: PathBuf) -> RuntimeLookup<'static> {
            RuntimeLookup {
                rt: Runtime::Base,
                release: true,
                target: None,
                runtime_dir: None,
                exe: Some(exe),
                root: Some(self.root()),
            }
        }

        /// The workspace runtime as a pinned cargo build started at `at`
        /// leaves it: built at `at` from the fixture's source, with its
        /// dep-info, stamped.
        fn build_workspace_lib(&self, profile: &str, at: u64) -> PathBuf {
            let lib = self.workspace_lib(profile);
            write(&lib, "!<arch>\n", at);
            let dep_info = format!("{}: {}\n", lib.display(), self.source().display());
            write(&lib.with_extension("d"), &dep_info, at);
            stamp_as_current(&lib, SystemTime::UNIX_EPOCH + Duration::from_secs(at));
            lib
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn axon_runtime_dir_is_the_only_place_searched() {
        let fx = Fixture::new("envdir");
        fx.build_workspace_lib("release", T0 + 10);
        let mut lookup = fx.lookup(fx.dev_compiler());
        let dir = fx.dir.join("rt");
        lookup.runtime_dir = Some(dir.clone());
        let err = lookup.resolve(no_cargo).unwrap_err();
        assert!(err.contains(&s(&dir.join("libaxon_rt.a"))), "{err}");
        assert!(err.contains(RUNTIME_DIR_VAR), "{err}");

        write(&dir.join("libaxon_rt.a"), "", T0);
        assert_eq!(lookup.resolve(no_cargo), Ok(s(&dir.join("libaxon_rt.a"))));
        lookup.target = Some("aarch64-linux-android");
        let cross = dir.join("aarch64-linux-android").join("libaxon_rt.a");
        write(&cross, "", T0);
        assert_eq!(lookup.resolve(no_cargo), Ok(s(&cross)));
    }

    #[test]
    fn the_runtime_installed_beside_the_compiler_comes_before_the_workspace() {
        let fx = Fixture::new("installed");
        fx.build_workspace_lib("release", T0 + 10);
        let exe = fx.installed_compiler();
        write(&fx.installed_lib("release"), "", T0);
        assert_eq!(
            fx.lookup(exe).resolve(no_cargo),
            Ok(s(&fx.installed_lib("release")))
        );
    }

    #[test]
    fn a_current_workspace_runtime_is_linked_without_cargo() {
        let fx = Fixture::new("current");
        let lib = fx.build_workspace_lib("release", T0 + 10);
        assert_eq!(runtime_freshness(&lib, &fx.root()), Freshness::Current);
        assert_eq!(fx.lookup(fx.dev_compiler()).resolve(no_cargo), Ok(s(&lib)));
    }

    #[test]
    fn an_edited_runtime_source_rebuilds_before_the_link() {
        let fx = Fixture::new("edited");
        let lib = fx.build_workspace_lib("release", T0 + 10);
        set_mtime(&fx.source(), T0 + 20);
        assert_eq!(
            runtime_freshness(&lib, &fx.root()),
            Freshness::Stale(format!("is older than {}", fx.source().display()))
        );

        let mut rebuilt_in = None;
        let got = fx.lookup(fx.dev_compiler()).resolve(|root, target_dir| {
            rebuilt_in = Some((root.to_path_buf(), target_dir.to_path_buf()));
            set_mtime(&lib, T0 + 30);
            Ok(s(&lib))
        });
        assert_eq!(got, Ok(s(&lib)));
        assert_eq!(rebuilt_in, Some((fx.root(), fx.target_dir())));
        // The rebuilt lib is stamped, so the next link runs no cargo.
        assert_eq!(runtime_freshness(&lib, &fx.root()), Freshness::Current);
        assert_eq!(fx.lookup(fx.dev_compiler()).resolve(no_cargo), Ok(s(&lib)));
    }

    #[test]
    fn an_input_cargo_confirmed_without_rebuilding_stays_current() {
        // An edit cargo judges irrelevant (a comment in `.cargo/config.toml`)
        // makes the lib look stale; cargo then leaves it untouched. The link
        // after that confirming run must not run cargo again.
        let fx = Fixture::new("confirmed");
        let lib = fx.build_workspace_lib("release", T0 + 10);
        let config = fx.root().join(".cargo").join("config.toml");
        write(&config, "# comment\n", T0 + 20);
        assert!(matches!(runtime_freshness(&lib, &fx.root()),
            Freshness::Stale(w) if w.contains("config.toml")));

        let mut runs = 0;
        let got = fx.lookup(fx.dev_compiler()).resolve(|_, _| {
            runs += 1;
            Ok(s(&lib))
        });
        assert_eq!((got, runs), (Ok(s(&lib)), 1));
        assert_eq!(runtime_freshness(&lib, &fx.root()), Freshness::Current);
        assert_eq!(fx.lookup(fx.dev_compiler()).resolve(no_cargo), Ok(s(&lib)));

        // An input edited after that run is newer than the stamp: stale again.
        let far = SystemTime::now() + Duration::from_secs(3600);
        let f = std::fs::File::options()
            .write(true)
            .open(fx.source())
            .unwrap();
        f.set_modified(far).unwrap();
        assert!(matches!(
            runtime_freshness(&lib, &fx.root()),
            Freshness::Stale(_)
        ));
    }

    #[test]
    fn manifests_the_lockfile_and_the_toolchain_pin_are_inputs() {
        for input in [
            "Cargo.toml",
            "Cargo.lock",
            "rust-toolchain.toml",
            "crates/axon-rt/Cargo.toml",
        ] {
            let fx = Fixture::new("inputs");
            let lib = fx.build_workspace_lib("release", T0 + 10);
            set_mtime(&fx.root().join(input), T0 + 20);
            assert!(
                matches!(runtime_freshness(&lib, &fx.root()), Freshness::Stale(w) if w.contains(input)),
                "{input} newer than the lib must make it stale"
            );
        }
    }

    #[test]
    fn a_lib_some_other_build_produced_is_not_current() {
        let fx = Fixture::new("foreign");
        // e.g. `RUSTUP_TOOLCHAIN=<other> cargo build -p axon-rt`: a new file in
        // the lib's place, newer than every source, that no pinned build stamped.
        let lib = fx.build_workspace_lib("release", T0 + 10);
        std::fs::remove_file(&lib).unwrap();
        write(&lib, "!<arch>\nother toolchain", T0 + 20);
        let stale = |fx: &Fixture| {
            matches!(runtime_freshness(&fx.workspace_lib("release"), &fx.root()),
                Freshness::Stale(w) if w.contains("pinned toolchain"))
        };
        assert!(stale(&fx));

        let fx = Fixture::new("unstamped");
        let lib = fx.build_workspace_lib("release", T0 + 10);
        std::fs::remove_file(stamp_path(&lib)).unwrap();
        assert!(stale(&fx));
        let mut rebuilt = false;
        let got = fx.lookup(fx.dev_compiler()).resolve(|_, _| {
            rebuilt = true;
            Ok(s(&lib))
        });
        assert!(rebuilt);
        assert_eq!(got, Ok(s(&lib)));
    }

    #[test]
    fn a_missing_source_or_dep_info_is_not_current() {
        let fx = Fixture::new("gone");
        let lib = fx.build_workspace_lib("release", T0 + 10);
        std::fs::remove_file(fx.source()).unwrap();
        assert!(matches!(runtime_freshness(&lib, &fx.root()),
            Freshness::Stale(w) if w.contains("no longer exists")));

        let fx = Fixture::new("nodepinfo");
        let lib = fx.build_workspace_lib("release", T0 + 10);
        std::fs::remove_file(lib.with_extension("d")).unwrap();
        assert!(matches!(runtime_freshness(&lib, &fx.root()),
            Freshness::Stale(w) if w.contains("dep-info")));
    }

    #[test]
    fn without_a_rebuild_the_fallbacks_are_installed_then_workspace_libs() {
        // A stale workspace lib that cannot be rebuilt is still linked (with
        // a warning), rather than failing the build.
        let fx = Fixture::new("fallback_stale");
        let lib = fx.build_workspace_lib("release", T0 + 10);
        set_mtime(&fx.source(), T0 + 20);
        let no_cargo_found = |_: &Path, _: &Path| Err("no `cargo` was found".to_string());
        assert_eq!(
            fx.lookup(fx.dev_compiler()).resolve(no_cargo_found),
            Ok(s(&lib))
        );

        // The other profile's workspace lib.
        let fx = Fixture::new("fallback_profile");
        let debug = fx.build_workspace_lib("debug", T0 + 10);
        assert_eq!(
            fx.lookup(fx.dev_compiler()).resolve(no_cargo_found),
            Ok(s(&debug))
        );

        // The other profile's installed lib, before any workspace lib.
        let fx = Fixture::new("fallback_installed");
        fx.build_workspace_lib("debug", T0 + 10);
        let exe = fx.installed_compiler();
        write(&fx.installed_lib("debug"), "", T0);
        assert_eq!(
            fx.lookup(exe).resolve(no_cargo_found),
            Ok(s(&fx.installed_lib("debug")))
        );
    }

    #[test]
    fn no_runtime_anywhere_is_one_error_listing_every_place_searched() {
        let fx = Fixture::new("none");
        let exe = fx.installed_compiler();
        let err = fx
            .lookup(exe.clone())
            .resolve(|_, _| Err("no `cargo` was found".to_string()))
            .unwrap_err();
        assert!(
            err.starts_with("native runtime `libaxon_rt.a` (crate axon-rt) not found"),
            "{err}"
        );
        assert!(err.contains("no `cargo` was found"), "{err}");
        assert!(err.contains(RUNTIME_DIR_VAR), "{err}");
        let searched: Vec<PathBuf> = vec![
            fx.installed_lib("release"),
            fx.installed_lib("debug"),
            fx.workspace_lib("release"),
            fx.workspace_lib("debug"),
        ];
        let listed = err.split("searched:").nth(1).unwrap_or_default();
        let positions: Vec<usize> = searched
            .iter()
            .map(|p| {
                listed
                    .find(&s(p))
                    .unwrap_or_else(|| panic!("{} not listed:\n{err}", p.display()))
            })
            .collect();
        assert!(
            positions.windows(2).all(|w| w[0] < w[1]),
            "search order:\n{err}"
        );

        // No workspace at all: the error says so instead of blaming cargo.
        let mut lookup = fx.lookup(exe);
        lookup.root = None;
        let err = lookup.resolve(no_cargo).unwrap_err();
        assert!(err.contains("no Axon source workspace"), "{err}");
        assert!(err.contains(&s(&fx.installed_lib("release"))), "{err}");
    }

    #[test]
    fn the_workspace_runtime_lives_in_the_compilers_own_target_dir() {
        let fx = Fixture::new("targetdir");
        let root = fx.root();
        assert_eq!(
            workspace_target_dir(&root, Some(&fx.dev_compiler())),
            fx.target_dir()
        );
        let cross = fx
            .target_dir()
            .join("x86_64-unknown-linux-gnu/release/axon");
        assert_eq!(workspace_target_dir(&root, Some(&cross)), fx.target_dir());
        let elsewhere = fx.dir.join("shared/target");
        write(&elsewhere.join("CACHEDIR.TAG"), "", T0);
        assert_eq!(
            workspace_target_dir(&root, Some(&elsewhere.join("debug/axon"))),
            elsewhere
        );
        // A compiler outside any target dir uses the workspace's own.
        assert_eq!(
            workspace_target_dir(&root, Some(&fx.installed_compiler())),
            root.join("target")
        );
        assert_eq!(workspace_target_dir(&root, None), root.join("target"));
    }

    #[test]
    fn cross_runtimes_sit_under_the_triple() {
        let fx = Fixture::new("cross");
        let mut lookup = fx.lookup(fx.installed_compiler());
        lookup.target = Some("aarch64-linux-android");
        assert_eq!(
            lookup.installed_dir("release"),
            Some(
                fx.installed_lib("release")
                    .parent()
                    .unwrap()
                    .join("aarch64-linux-android")
            )
        );
        assert_eq!(
            lookup.built_dir(&fx.target_dir(), "release"),
            fx.target_dir().join("aarch64-linux-android/release")
        );
    }

    #[test]
    fn dep_info_prerequisites_are_parsed_with_escaped_spaces() {
        let d = "/t/release/libaxon_rt.a: /w/a\\ b/lib.rs /w/c.rs\n\n/w/a\\ b/lib.rs:\n";
        assert_eq!(
            dep_info_sources(d),
            vec![PathBuf::from("/w/a b/lib.rs"), PathBuf::from("/w/c.rs")]
        );
        assert!(dep_info_sources("").is_empty());
        assert!(dep_info_sources("no rule here").is_empty());
    }

    #[test]
    fn toolchain_redirecting_env_is_removed_from_the_runtime_build() {
        for var in [
            "RUSTUP_TOOLCHAIN",
            "RUSTC",
            "CARGO_BUILD_RUSTC",
            "RUSTC_WRAPPER",
            "CARGO_BUILD_RUSTC_WRAPPER",
            "RUSTC_WORKSPACE_WRAPPER",
            "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
            "RUSTFLAGS",
            "CARGO_ENCODED_RUSTFLAGS",
            "CARGO_BUILD_RUSTFLAGS",
            "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
            "CARGO_PROFILE_RELEASE_LTO",
            "CARGO_BUILD_TARGET",
            "CARGO_BUILD_DEP_INFO_BASEDIR",
        ] {
            assert!(redirects_runtime_toolchain(var), "{var} must be removed");
        }
        for var in [
            "RUSTUP_HOME",
            "CARGO_HOME",
            "CARGO_TARGET_DIR",
            "CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER",
            "CC_aarch64-linux-android",
            "AR_aarch64-linux-android",
            "PATH",
        ] {
            assert!(!redirects_runtime_toolchain(var), "{var} must be kept");
        }
    }
}
