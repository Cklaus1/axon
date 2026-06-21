//! Output / driver methods on `Codegen<'ctx>` plus the `TestResult` type.
//!
//! Phase 2.6 of the §7.5 module split: extracts the user-facing entry
//! points that finalise compilation (write IR, compile to binary, emit
//! bitcode for the cache) and the JIT test runner.
//!
//! All Codegen methods declared `pub` because they are part of the
//! external API consumed by `lib.rs` and `main.rs`.  TestResult is also
//! `pub` and re-exported by `super::mod` for backwards compatibility
//! with `axon_core::codegen::TestResult` paths.

use std::path::Path;

use inkwell::values::BasicValue;
use inkwell::OptimizationLevel;

use super::link::emit_object_and_link;

// Public test-result type used by `run_tests` callers.
#[derive(Debug)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
}

impl<'ctx> super::Codegen<'ctx> {
    // ── Output ────────────────────────────────────────────────────────────────

    /// Write the LLVM IR text representation to `path` (usually `*.ll`).
    pub fn write_ir(&self, path: &str) -> Result<(), String> {
        self.ir
            .module
            .print_to_file(Path::new(path))
            .map_err(|e| e.to_string())
    }

    /// Compile the module to a native binary at `output_path`.
    ///
    /// Convenience wrapper around `compile_to_binary_target` with
    /// `target_triple = None`.
    pub fn compile_to_binary(&self, output_path: &str, release: bool) -> Result<(), String> {
        self.compile_to_binary_target(output_path, release, None)
    }

    /// Dead-function elimination (R7 + general hygiene): `declare_builtins`
    /// emits ~119 builtin helper functions (to_str, parse_int, temporal_new,
    /// the str ops, …) with bodies, UNCONDITIONALLY — so even a trivial program
    /// drags every one, and every `__axon_*` runtime extern they call. On wasm32
    /// the unused str/array helpers carry the i64-pointer ABI that clashes with
    /// wasm's i32 libc (a `function signature mismatch` at link). This deletes
    /// every function with a body and ZERO uses, to a fixpoint — `main` and any
    /// address-taken function (a vtable thunk, a lambda, an `@[adaptive]` fn in
    /// the registry — all of which have a `use`) are KEPT. So a pure-integer
    /// program emerges with NO `__axon_*` imports at all. Precise + safe: only a
    /// genuinely-unreferenced definition is removed; an extern declaration with
    /// no callers is dropped too once its callers are gone.
    ///
    /// Returns the number of functions deleted (for logging/tests).
    pub fn prune_dead_functions(&self) -> usize {
        let mut total = 0;
        loop {
            // Collect deletable functions this round: a DEFINITION (has a body)
            // that is not `main` and has no uses. We snapshot first because
            // deleting mutates the module's function list.
            let mut to_delete = Vec::new();
            let mut f = self.ir.module.get_first_function();
            while let Some(func) = f {
                let next = func.get_next_function();
                let name = func.get_name().to_string_lossy().to_string();
                let has_body = func.count_basic_blocks() > 0;
                let used = func.as_global_value().get_first_use().is_some();
                if has_body && !used && name != "main" {
                    to_delete.push(func);
                }
                f = next;
            }
            if to_delete.is_empty() {
                break;
            }
            for func in to_delete {
                // Deleting a definition drops its instructions (and thus the uses
                // of any extern it called), enabling the next round to prune those.
                unsafe {
                    func.delete();
                }
                total += 1;
            }
        }
        total
    }

    /// Compile the module to a binary for an optional target triple.
    ///
    /// Steps:
    /// 1. Verify the LLVM IR.
    /// 2. Initialize the appropriate LLVM backend (native or all targets for cross).
    /// 3. Create the `TargetMachine`.
    /// 4. Emit an object file to a temp path.
    /// 5. Link with the system linker (or cross-linker from `~/.config/axon/cross.toml`).
    pub fn compile_to_binary_target(
        &self,
        output_path: &str,
        release: bool,
        target_triple: Option<&str>,
    ) -> Result<(), String> {
        // NOTE: dead-function pruning is applied on the WASM object path
        // (`compile_to_wasm_object`), where dropping the unused i64-ABI `__axon_*`
        // helpers is the prerequisite for linking. It is intentionally NOT run
        // here: the native link tolerates the unused helpers, and pruning them
        // exposed a latent libm (`pow`) link-order fragility on the native path
        // (axon-rt's f64::powf → undefined `pow`). Keeping native unchanged.
        self.ir
            .module
            .verify()
            .map_err(|e| format!("IR verification failed: {}", e.to_string()))?;
        emit_object_and_link(&self.ir.module, output_path, release, target_triple)
    }

    /// R17: Compile to a freestanding (bare-metal) ELF binary.
    ///
    /// Uses a static/kernel code model, omits axon-rt/libc, and sets the ELF
    /// entry symbol to the `@[entry]`-annotated function.
    pub fn compile_to_freestanding_binary(
        &self,
        output_path: &str,
        release: bool,
        target_triple: Option<&str>,
        entry_fn: Option<&str>,
    ) -> Result<(), String> {
        // Prune unused builtins (println, assert, str ops, ...) before linking.
        // For freestanding we preserve the @[entry] fn and any @[panic_handler]
        // fn in addition to `main` (which won't exist in a kernel image).
        self.prune_dead_functions_keep(
            &["main", "on_panic", "__axon_kernel_panic"]
                .iter()
                .copied()
                .chain(entry_fn)
                .collect::<Vec<_>>(),
        );
        self.ir
            .module
            .verify()
            .map_err(|e| format!("IR verification failed: {}", e.to_string()))?;
        super::link::emit_freestanding_binary(
            &self.ir.module,
            output_path,
            release,
            target_triple,
            entry_fn,
        )
    }

    /// Like `prune_dead_functions` but preserves a set of named roots.
    fn prune_dead_functions_keep(&self, keep: &[&str]) -> usize {
        let mut total = 0;
        loop {
            let mut to_delete = Vec::new();
            let mut f = self.ir.module.get_first_function();
            while let Some(func) = f {
                let next = func.get_next_function();
                let name = func.get_name().to_string_lossy().to_string();
                let has_body = func.count_basic_blocks() > 0;
                let used = func.as_global_value().get_first_use().is_some();
                if has_body && !used && !keep.contains(&name.as_str()) {
                    to_delete.push(func);
                }
                f = next;
            }
            if to_delete.is_empty() {
                break;
            }
            for func in to_delete {
                unsafe { func.delete(); }
                total += 1;
            }
        }
        total
    }

    /// Serialize the compiled LLVM IR as bitcode bytes (for the incremental cache).
    pub fn emit_bitcode(&self) -> Vec<u8> {
        self.ir.module.write_bitcode_to_memory().as_slice().to_vec()
    }

    /// R7 Slice B (AOT wasm, object half): verify the IR and emit a WebAssembly
    /// **object** at `output_path` via the inkwell `wasm32` backend (no link).
    /// The runnable-`.wasm` link needs a wasm sysroot + `wasm-ld` (the deferred
    /// remaining gap); this is the verifiable IR→wasm codegen step.
    pub fn compile_to_wasm_object(
        &self,
        output_path: &str,
        release: bool,
        target_triple: &str,
    ) -> Result<(), String> {
        // Prune unused builtin helpers FIRST (R7): drops the unused str/array
        // helpers and their i64-ABI `__axon_*` imports, so a pure-integer
        // program emits a wasm object with no clashing externs.
        let _pruned = self.prune_dead_functions();
        self.ir
            .module
            .verify()
            .map_err(|e| format!("IR verification failed: {}", e.to_string()))?;
        super::link::emit_wasm_object(&self.ir.module, output_path, release, target_triple)
    }

    // ── Test runner ───────────────────────────────────────────────────────

    /// Run functions tagged `@[test]` via the LLVM JIT and report results.
    ///
    /// One JIT execution engine is created for the entire module and reused
    /// across all tests. Test functions have Axon signature `fn()` (void);
    /// they pass if they return normally. If `assert(false)` fires it calls
    /// `exit(101)` (the runtime-panic code), terminating the process — Phase 1
    /// limitation.
    pub fn run_tests(&self, fns: &[String]) -> Vec<TestResult> {
        // Verify the module before running tests.
        if let Err(e) = self.ir.module.verify() {
            return fns
                .iter()
                .map(|name| TestResult {
                    name: name.clone(),
                    passed: false,
                    duration_ms: 0,
                    error: Some(format!("IR verification failed: {}", e.to_string())),
                })
                .collect();
        }

        // Create a single JIT engine for the whole module.
        let ee = match self
            .ir
            .module
            .create_jit_execution_engine(OptimizationLevel::None)
        {
            Ok(e) => e,
            Err(e) => {
                return fns
                    .iter()
                    .map(|name| TestResult {
                        name: name.clone(),
                        passed: false,
                        duration_ms: 0,
                        error: Some(format!("JIT init: {e}")),
                    })
                    .collect();
            }
        };

        fns.iter()
            .map(|name| {
                let start = std::time::Instant::now();

                type VoidFn = unsafe extern "C" fn();
                let result: Result<(), String> = unsafe {
                    ee.get_function::<VoidFn>(name)
                        .map_err(|e| format!("JIT lookup '{name}': {e}"))
                        .map(|f| f.call())
                };

                let duration_ms = start.elapsed().as_millis() as u64;
                match result {
                    Ok(()) => TestResult {
                        name: name.clone(),
                        passed: true,
                        duration_ms,
                        error: None,
                    },
                    Err(e) => TestResult {
                        name: name.clone(),
                        passed: false,
                        duration_ms,
                        error: Some(e),
                    },
                }
            })
            .collect()
    }
}
