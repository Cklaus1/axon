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
        self.ir.module
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
        self.ir.module
            .verify()
            .map_err(|e| format!("IR verification failed: {}", e.to_string()))?;
        emit_object_and_link(&self.ir.module, output_path, release, target_triple)
    }

    /// Serialize the compiled LLVM IR as bitcode bytes (for the incremental cache).
    pub fn emit_bitcode(&self) -> Vec<u8> {
        self.ir.module.write_bitcode_to_memory().as_slice().to_vec()
    }

    // ── Test runner ───────────────────────────────────────────────────────

    /// Run functions tagged `@[test]` via the LLVM JIT and report results.
    ///
    /// One JIT execution engine is created for the entire module and reused
    /// across all tests. Test functions have Axon signature `fn()` (void);
    /// they pass if they return normally. If `assert(false)` fires it calls
    /// `exit(1)`, terminating the process — Phase 1 limitation.
    pub fn run_tests(&self, fns: &[String]) -> Vec<TestResult> {
        // Verify the module before running tests.
        if let Err(e) = self.ir.module.verify() {
            return fns.iter().map(|name| TestResult {
                name: name.clone(),
                passed: false,
                duration_ms: 0,
                error: Some(format!("IR verification failed: {}", e.to_string())),
            }).collect();
        }

        // Create a single JIT engine for the whole module.
        let ee = match self.ir.module.create_jit_execution_engine(OptimizationLevel::None) {
            Ok(e) => e,
            Err(e) => {
                return fns.iter().map(|name| TestResult {
                    name: name.clone(),
                    passed: false,
                    duration_ms: 0,
                    error: Some(format!("JIT init: {e}")),
                }).collect();
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
                    Ok(()) => TestResult { name: name.clone(), passed: true, duration_ms, error: None },
                    Err(e) => TestResult { name: name.clone(), passed: false, duration_ms, error: Some(e) },
                }
            })
            .collect()
    }
}
