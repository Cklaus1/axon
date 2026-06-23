//! The inkwell-backed IR holder used by codegen.
//!
//! `InkwellBackend<'ctx>` owns the one `Module` + `Builder` per codegen run
//! (created by `Codegen::new`, handed over via `adopt`). Every codegen module
//! reaches LLVM through `self.ir.{context, module, builder}` — these three
//! public fields ARE the IR-emission surface (paired with the
//! `build_wrappers::w_*` helpers that keep inkwell's heavy generics out of the
//! call sites; see `build_wrappers.rs`).
//!
//! History (R1e): an earlier refactor (IR.2/IR.3, `IR_REARCH.md`) tried a
//! handle/arena `IR` *trait* abstraction (`ir.rs` + a big `impl IR` here) so
//! codegen would call `self.ir.iadd(a, b)` over an arena of `u32` handles.
//! That migration was **abandoned** — it had zero callers, its handle methods
//! were never wired in, and `build_wrappers` (the `#[inline(never)]` wrappers)
//! superseded it for the build-speed goal it was meant to serve. R1e deletes
//! the dead trait + arenas; what remains is this thin, real holder.

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;

/// LLVM/inkwell IR holder: a borrowed `Context` plus the owned `Module` +
/// `Builder` for one codegen run. Codegen accesses LLVM through these fields.
pub struct InkwellBackend<'ctx> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
}

impl<'ctx> InkwellBackend<'ctx> {
    /// Adopt an externally-owned `Module<'ctx>` + `Builder<'ctx>`, taking
    /// ownership. This is the path used by `Codegen::new`: Codegen creates the
    /// module + builder, then hands them here so there is exactly ONE Module
    /// per codegen run (the single-symbol-table invariant from `IR_REARCH.md`
    /// option (c)).
    pub fn adopt(context: &'ctx Context, module: Module<'ctx>, builder: Builder<'ctx>) -> Self {
        Self {
            context,
            module,
            builder,
        }
    }

    /// Convenience constructor for tests / standalone benches that don't have a
    /// pre-built module: creates a fresh module + builder and delegates to
    /// `adopt`.
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        Self::adopt(context, module, builder)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The backend exposes the real inkwell surface (context/module/builder)
    /// and can build a trivial function through it — the inherent API codegen
    /// actually uses.
    #[test]
    fn build_const_return_via_inherent_api() {
        let ctx = Context::create();
        let be = InkwellBackend::new(&ctx, "test_ir_holder");
        let i64_t = be.context.i64_type();
        let fn_ty = i64_t.fn_type(&[], false);
        let f = be.module.add_function("answer", fn_ty, None);
        let entry = be.context.append_basic_block(f, "entry");
        be.builder.position_at_end(entry);
        be.builder
            .build_return(Some(&i64_t.const_int(42, false)))
            .unwrap();
        // The module verifies and the function is present.
        assert!(be.module.verify().is_ok());
        assert!(be.module.get_function("answer").is_some());
    }
}
