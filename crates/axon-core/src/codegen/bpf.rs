//! R23 — eBPF backend: lower a restricted, capability-typed Axon subset to a
//! BPF object file (`elf64-bpf`) the Linux verifier accepts.
//!
//! This is a **focused, self-contained** backend: it builds its own inkwell
//! context + module (NOT the hosted `Codegen` pipeline, which would attach
//! provenance hooks, a `main` wrapper, and the host runtime — none of which
//! exist on BPF), lowers only the BPF-lowerable expression subset, and emits
//! the object via the `bpf` target machine. The hosted codegen path is
//! completely untouched.
//!
//! ## The proven IR shape
//! A `@[bpf]` program lowers to:
//!   - a `maps` ELF section global: `{i32 type, i32 key_size, i32 value_size,
//!     i32 max_entries, i32 flags}` (Slice 1: one `BPF_MAP_TYPE_ARRAY`,
//!     key=4, value=8, max=1, named `axon_map`).
//!   - a function in the program's ELF section (`socket`/`xdp`/…), whose body
//!     references the map via an `lddw` that LLVM emits with an `R_BPF_64_64`
//!     relocation on `axon_map` — the loader patches it to `BPF_PSEUDO_MAP_FD`.
//!   - `bpf_map_lookup_elem` → a `call 1`; `bpf_map_value_add` → an
//!     `atomicrmw add`; the other helpers → `call <id>`.
//!
//! This shape was validated against the real in-kernel verifier
//! (`scripts/bpfload.c` → `bpf(BPF_PROG_LOAD)` ACCEPTS).
//!
//! ## What is refused (sound-by-refusal)
//! Anything outside the lowerable subset (closures, structs, match, strings,
//! arrays, while, …) is E2301 — a clean refusal, never bytecode that only
//! sometimes verifies. `@[total]`/`@[no_alloc]` are already enforced by the
//! checker (E1208/E1704) before we get here, so unbounded loops / heap are
//! already gone.

use std::collections::HashMap;
use std::path::Path;

use inkwell::basic_block::BasicBlock;
use inkwell::context::Context;
use inkwell::module::{Linkage, Module};
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetTriple,
};
use inkwell::values::{FunctionValue, IntValue, PointerValue};
use inkwell::{AddressSpace, IntPredicate, OptimizationLevel};

use crate::ast::{Expr, Item, Literal, Program, Stmt};

/// The fixed Slice-1 map: a single `BPF_MAP_TYPE_ARRAY` with one i64 slot.
const MAP_NAME: &str = "axon_map";
const BPF_MAP_TYPE_ARRAY: u64 = 2;
const MAP_KEY_SIZE: u64 = 4;
const MAP_VALUE_SIZE: u64 = 8;
const MAP_MAX_ENTRIES: u64 = 1;

/// Emit a `.bpf.o` for the first `@[bpf]`-annotated fn in `program`.
///
/// Returns the ELF section name the program was placed in (for the loader /
/// verify script), or an `Err(msg)` (E2301 for an unsupported construct, or a
/// target/emit failure). The caller has already type-checked + run the
/// E2300/E2302/E1208/E1704 gates, so this only fails on a genuinely
/// unlowerable body or a toolchain problem.
pub fn emit_bpf_object(program: &Program, output_path: &str) -> Result<String, String> {
    // Find the BPF program fn.
    let bpf_fn = program.items.iter().find_map(|it| match it {
        Item::FnDef(f) if f.attrs.iter().any(|a| a.name == "bpf") => Some(f),
        _ => None,
    });
    let f = bpf_fn.ok_or_else(|| {
        "no `@[bpf]`-annotated function found; mark one with `@[bpf(kind: socket_filter)]`"
            .to_string()
    })?;

    // Resolve the ELF section from the kind.
    let kind = f
        .attrs
        .iter()
        .find(|a| a.name == "bpf")
        .and_then(|a| {
            a.args.iter().find_map(|arg| {
                arg.strip_prefix("kind:")
                    .map(|s| s.trim().to_string())
                    .or_else(|| {
                        if !arg.contains(':') {
                            Some(arg.trim().to_string())
                        } else {
                            None
                        }
                    })
            })
        })
        .unwrap_or_else(|| "socket_filter".to_string());
    let section = crate::builtins::bpf_kind_section(&kind)
        .ok_or_else(|| format!("unknown @[bpf] kind `{kind}`"))?;

    let ctx = Context::create();
    let module = ctx.create_module("axon_bpf");

    let mut lowerer = BpfLowerer {
        ctx: &ctx,
        module: &module,
        builder: ctx.create_builder(),
        locals: HashMap::new(),
        map_global: None,
    };

    lowerer.declare_map();
    lowerer.declare_license();
    lowerer.lower_program_fn(f, section)?;

    write_object(&module, output_path).map(|_| section.to_string())
}

struct BpfLowerer<'ctx, 'a> {
    ctx: &'ctx Context,
    module: &'a Module<'ctx>,
    builder: inkwell::builder::Builder<'ctx>,
    /// name → i64 stack slot holding the local's value.
    locals: HashMap<String, PointerValue<'ctx>>,
    map_global: Option<PointerValue<'ctx>>,
}

impl<'ctx, 'a> BpfLowerer<'ctx, 'a> {
    /// Emit the `maps`-section global describing the single array map.
    fn declare_map(&mut self) {
        let i32_ty = self.ctx.i32_type();
        let def_ty = self.ctx.struct_type(
            &[
                i32_ty.into(),
                i32_ty.into(),
                i32_ty.into(),
                i32_ty.into(),
                i32_ty.into(),
            ],
            false,
        );
        let g = self.module.add_global(def_ty, None, MAP_NAME);
        g.set_initializer(&def_ty.const_named_struct(&[
            i32_ty.const_int(BPF_MAP_TYPE_ARRAY, false).into(),
            i32_ty.const_int(MAP_KEY_SIZE, false).into(),
            i32_ty.const_int(MAP_VALUE_SIZE, false).into(),
            i32_ty.const_int(MAP_MAX_ENTRIES, false).into(),
            i32_ty.const_int(0, false).into(),
        ]));
        g.set_section(Some("maps"));
        g.set_linkage(Linkage::External);
        self.map_global = Some(g.as_pointer_value());
    }

    /// Emit the GPL license string the verifier requires for helper use.
    fn declare_license(&mut self) {
        let bytes = b"GPL\0";
        let arr_ty = self.ctx.i8_type().array_type(bytes.len() as u32);
        let g = self.module.add_global(arr_ty, None, "_license");
        g.set_initializer(&self.ctx.const_string(b"GPL", true));
        g.set_section(Some("license"));
        g.set_linkage(Linkage::External);
    }

    /// Lower the `@[bpf]` fn into a BPF function in `section`.
    fn lower_program_fn(&mut self, f: &crate::ast::FnDef, section: &str) -> Result<(), String> {
        let i64_ty = self.ctx.i64_type();
        let ptr_ty = self.ctx.i8_type().ptr_type(AddressSpace::default());
        // BPF program signature: i64 prog(ptr ctx). The single param (ctx) is a
        // pointer; we keep the body's view of it as an i64 for uniformity.
        let fn_ty = i64_ty.fn_type(&[ptr_ty.into()], false);
        let func = self.module.add_function(&f.name, fn_ty, None);
        func.set_section(Some(section));

        let entry = self.ctx.append_basic_block(func, "entry");
        self.builder.position_at_end(entry);

        // Bind the ctx param as an i64 local (ptrtoint) so the body can name it.
        if let Some(p) = f.params.first() {
            let ctx_ptr = func.get_nth_param(0).unwrap().into_pointer_value();
            let ctx_int = self
                .builder
                .build_ptr_to_int(ctx_ptr, i64_ty, "ctx_int")
                .unwrap();
            let slot = self.builder.build_alloca(i64_ty, &p.name).unwrap();
            self.builder.build_store(slot, ctx_int).unwrap();
            self.locals.insert(p.name.clone(), slot);
        }

        let ret = self.lower_expr(&f.body, func)?;
        // Ensure the block is terminated with a return.
        if self
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_none()
        {
            let v = ret.unwrap_or_else(|| i64_ty.const_zero());
            self.builder.build_return(Some(&v)).unwrap();
        }
        Ok(())
    }

    /// Lower a BPF-subset expression to an i64 value. Returns `Ok(None)` for a
    /// statement-like expression with no value (e.g. a bare helper call with
    /// `()` result). Refuses anything outside the subset with E2301.
    fn lower_expr(
        &mut self,
        expr: &Expr,
        func: FunctionValue<'ctx>,
    ) -> Result<Option<IntValue<'ctx>>, String> {
        let i64_ty = self.ctx.i64_type();
        match expr {
            Expr::Block(stmts) => {
                let mut last: Option<IntValue<'ctx>> = None;
                for s in stmts {
                    last = self.lower_stmt(s, func)?;
                    // Stop emitting after a terminator (e.g. an early return).
                    if self
                        .builder
                        .get_insert_block()
                        .and_then(|b| b.get_terminator())
                        .is_some()
                    {
                        break;
                    }
                }
                Ok(last)
            }
            Expr::Literal(Literal::Int(n)) => Ok(Some(i64_ty.const_int(*n as u64, true))),
            Expr::Literal(Literal::Bool(b)) => Ok(Some(i64_ty.const_int(*b as u64, false))),
            Expr::Return(inner) => {
                let v = match inner {
                    Some(e) => self
                        .lower_expr(e, func)?
                        .unwrap_or_else(|| i64_ty.const_zero()),
                    None => i64_ty.const_zero(),
                };
                self.builder.build_return(Some(&v)).unwrap();
                Ok(None)
            }
            Expr::Ident(name) => {
                let slot = self
                    .locals
                    .get(name)
                    .copied()
                    .ok_or_else(|| e2301(func, &format!("unknown identifier `{name}`")))?;
                let v = self.builder.build_load(i64_ty, slot, name).unwrap();
                Ok(Some(v.into_int_value()))
            }
            Expr::Let { name, value, .. } => {
                let v = self
                    .lower_expr(value, func)?
                    .ok_or_else(|| e2301(func, "let binding to a value-less expression"))?;
                let slot = self.builder.build_alloca(i64_ty, name).unwrap();
                self.builder.build_store(slot, v).unwrap();
                self.locals.insert(name.clone(), slot);
                Ok(None)
            }
            Expr::BinOp { op, left, right } => {
                let l = self
                    .lower_expr(left, func)?
                    .ok_or_else(|| e2301(func, "binop on a value-less lhs"))?;
                let r = self
                    .lower_expr(right, func)?
                    .ok_or_else(|| e2301(func, "binop on a value-less rhs"))?;
                self.lower_binop(op, l, r, func).map(Some)
            }
            Expr::If { cond, then, else_ } => self.lower_if(cond, then, else_.as_deref(), func),
            Expr::Call { callee, args, .. } => self.lower_call(callee, args, func),
            other => Err(e2301(func, expr_kind(other))),
        }
    }

    fn lower_stmt(
        &mut self,
        stmt: &Stmt,
        func: FunctionValue<'ctx>,
    ) -> Result<Option<IntValue<'ctx>>, String> {
        self.lower_expr(&stmt.expr, func)
    }

    fn lower_binop(
        &mut self,
        op: &crate::ast::BinOp,
        l: IntValue<'ctx>,
        r: IntValue<'ctx>,
        func: FunctionValue<'ctx>,
    ) -> Result<IntValue<'ctx>, String> {
        use crate::ast::BinOp::*;
        let b = &self.builder;
        Ok(match op {
            Add => b.build_int_add(l, r, "add").unwrap(),
            Sub => b.build_int_sub(l, r, "sub").unwrap(),
            Mul => b.build_int_mul(l, r, "mul").unwrap(),
            // Division/modulo are valid BPF but need a zero guard the verifier
            // wants; keep the subset minimal and refuse them for Slice 1.
            Eq => self.icmp_to_i64(IntPredicate::EQ, l, r),
            NotEq => self.icmp_to_i64(IntPredicate::NE, l, r),
            Lt => self.icmp_to_i64(IntPredicate::SLT, l, r),
            LtEq => self.icmp_to_i64(IntPredicate::SLE, l, r),
            Gt => self.icmp_to_i64(IntPredicate::SGT, l, r),
            GtEq => self.icmp_to_i64(IntPredicate::SGE, l, r),
            other => return Err(e2301(func, &format!("binary operator {other:?}"))),
        })
    }

    fn icmp_to_i64(
        &self,
        pred: IntPredicate,
        l: IntValue<'ctx>,
        r: IntValue<'ctx>,
    ) -> IntValue<'ctx> {
        let cmp = self.builder.build_int_compare(pred, l, r, "cmp").unwrap();
        self.builder
            .build_int_z_extend(cmp, self.ctx.i64_type(), "cmp64")
            .unwrap()
    }

    fn lower_if(
        &mut self,
        cond: &Expr,
        then_branch: &Expr,
        else_branch: Option<&Expr>,
        func: FunctionValue<'ctx>,
    ) -> Result<Option<IntValue<'ctx>>, String> {
        let i64_ty = self.ctx.i64_type();
        let cond_v = self
            .lower_expr(cond, func)?
            .ok_or_else(|| e2301(func, "if-condition with no value"))?;
        let cond_bool = self
            .builder
            .build_int_compare(IntPredicate::NE, cond_v, i64_ty.const_zero(), "ifcond")
            .unwrap();

        let then_bb = self.ctx.append_basic_block(func, "then");
        let else_bb = self.ctx.append_basic_block(func, "else");
        let merge_bb = self.ctx.append_basic_block(func, "ifcont");
        self.builder
            .build_conditional_branch(cond_bool, then_bb, else_bb)
            .unwrap();

        // then
        self.builder.position_at_end(then_bb);
        let _ = self.lower_expr(then_branch, func)?;
        self.br_if_open(merge_bb);

        // else
        self.builder.position_at_end(else_bb);
        if let Some(e) = else_branch {
            let _ = self.lower_expr(e, func)?;
        }
        self.br_if_open(merge_bb);

        self.builder.position_at_end(merge_bb);
        // Slice-1 `if` is used for control flow (e.g. guard a map increment), so
        // it yields no value; the program's value comes from the trailing expr.
        Ok(None)
    }

    /// Branch to `dest` if the current block is not already terminated.
    fn br_if_open(&self, dest: BasicBlock<'ctx>) {
        if self
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_none()
        {
            self.builder.build_unconditional_branch(dest).unwrap();
        }
    }

    fn lower_call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        func: FunctionValue<'ctx>,
    ) -> Result<Option<IntValue<'ctx>>, String> {
        let name = match callee {
            Expr::Ident(n) => n.as_str(),
            _ => return Err(e2301(func, "indirect / method call")),
        };
        let i64_ty = self.ctx.i64_type();
        let ptr_ty = self.ctx.i8_type().ptr_type(AddressSpace::default());

        match name {
            // bpf_map_lookup_elem(map_handle, key) -> value ptr (as i64).
            // Slice 1: map_handle is ignored (the single axon_map is used); key
            // is passed by its address on the stack.
            "bpf_map_lookup_elem" => {
                if args.len() != 2 {
                    return Err(e2301(func, "bpf_map_lookup_elem expects (map, key)"));
                }
                let key_val = self
                    .lower_expr(&args[1], func)?
                    .ok_or_else(|| e2301(func, "bpf_map_lookup_elem key has no value"))?;
                // Spill the key (truncated to i32) to a stack slot → ptr.
                let i32_ty = self.ctx.i32_type();
                let key32 = self
                    .builder
                    .build_int_truncate(key_val, i32_ty, "key32")
                    .unwrap();
                let key_slot = self.builder.build_alloca(i32_ty, "key").unwrap();
                self.builder.build_store(key_slot, key32).unwrap();
                let map_ptr = self.map_global.unwrap();
                // helper #1: ptr bpf_map_lookup_elem(ptr map, ptr key)
                let helper_ty = ptr_ty.fn_type(&[ptr_ty.into(), ptr_ty.into()], false);
                let helper = self
                    .builder
                    .build_int_to_ptr(i64_ty.const_int(1, false), ptr_ty, "h_lookup")
                    .unwrap();
                let key_ptr = self
                    .builder
                    .build_pointer_cast(key_slot, ptr_ty, "key_ptr")
                    .unwrap();
                let map_ptr_c = self
                    .builder
                    .build_pointer_cast(map_ptr, ptr_ty, "map_ptr")
                    .unwrap();
                let call = self
                    .builder
                    .build_indirect_call(
                        helper_ty,
                        helper,
                        &[map_ptr_c.into(), key_ptr.into()],
                        "lookup",
                    )
                    .unwrap();
                let vptr = call
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_pointer_value();
                let as_i64 = self
                    .builder
                    .build_ptr_to_int(vptr, i64_ty, "vptr_int")
                    .unwrap();
                Ok(Some(as_i64))
            }
            // bpf_map_value_add(value_ptr, delta) -> () : atomicrmw add.
            "bpf_map_value_add" => {
                if args.len() != 2 {
                    return Err(e2301(func, "bpf_map_value_add expects (ptr, delta)"));
                }
                let ptr_int = self
                    .lower_expr(&args[0], func)?
                    .ok_or_else(|| e2301(func, "bpf_map_value_add ptr has no value"))?;
                let delta = self
                    .lower_expr(&args[1], func)?
                    .ok_or_else(|| e2301(func, "bpf_map_value_add delta has no value"))?;
                let vptr = self
                    .builder
                    .build_int_to_ptr(ptr_int, i64_ty.ptr_type(AddressSpace::default()), "vptr")
                    .unwrap();
                self.builder
                    .build_atomicrmw(
                        inkwell::AtomicRMWBinOp::Add,
                        vptr,
                        delta,
                        inkwell::AtomicOrdering::SequentiallyConsistent,
                    )
                    .map_err(|e| e2301(func, &format!("atomic add: {e}")))?;
                Ok(None)
            }
            // Zero-arg helpers: bpf_ktime_get_ns (#5), bpf_get_smp_processor_id (#8).
            "bpf_ktime_get_ns" | "bpf_get_smp_processor_id" => {
                let id = crate::builtins::bpf_helper_id(name).unwrap();
                let helper_ty = i64_ty.fn_type(&[], false);
                let helper = self
                    .builder
                    .build_int_to_ptr(i64_ty.const_int(id, false), ptr_ty, "helper")
                    .unwrap();
                let call = self
                    .builder
                    .build_indirect_call(helper_ty, helper, &[], name)
                    .unwrap();
                Ok(Some(
                    call.try_as_basic_value().left().unwrap().into_int_value(),
                ))
            }
            other => Err(e2301(func, &format!("call to `{other}`"))),
        }
    }
}

/// Build the E2301 message for an unsupported construct in a `@[bpf]` body.
fn e2301(func: FunctionValue<'_>, what: &str) -> String {
    let name = func.get_name().to_string_lossy().to_string();
    format!(
        "error[E2301]: unsupported construct in @[bpf] program `{name}`: {what} — \
         eBPF cannot lower it. The BPF subset is: i64 arithmetic/comparison, `let`, \
         `if`, and the allowlisted BPF helpers."
    )
}

/// A short kind name for an unsupported Expr (for the E2301 message).
fn expr_kind(e: &Expr) -> &'static str {
    match e {
        Expr::Match { .. } => "match expression",
        Expr::While { .. } => "while loop (unbounded — not BPF-lowerable)",
        Expr::For { .. } => "for loop",
        Expr::FmtStr { .. } => "string interpolation (no heap on BPF)",
        Expr::StructLit { .. } => "struct literal",
        Expr::Lambda { .. } => "lambda / closure",
        Expr::FieldAccess { .. } => "field access",
        Expr::Index { .. } => "index",
        Expr::MethodCall { .. } => "method call",
        Expr::Assign { .. } | Expr::AssignTo { .. } => "assignment",
        _ => "this expression",
    }
}

/// Initialize the BPF target, set the triple, and write the object file.
fn write_object(module: &Module<'_>, output_path: &str) -> Result<(), String> {
    Target::initialize_all(&InitializationConfig::default());
    // `bpfel` = little-endian BPF (the host byte order on x86_64).
    let triple = TargetTriple::create("bpfel");
    let target = Target::from_triple(&triple)
        .map_err(|e| format!("[E0904] BPF target not supported by this LLVM build: {e}"))?;
    let machine = target
        .create_target_machine(
            &triple,
            "generic",
            "",
            OptimizationLevel::Default,
            RelocMode::Static,
            CodeModel::Default,
        )
        .ok_or_else(|| "could not create BPF target machine".to_string())?;
    module.set_triple(&triple);
    // Emit via the target machine for correct BPF instruction encoding.
    machine
        .write_to_file(module, FileType::Object, Path::new(output_path))
        .map_err(|e| format!("BPF object emit: {e}"))?;
    Ok(())
}
