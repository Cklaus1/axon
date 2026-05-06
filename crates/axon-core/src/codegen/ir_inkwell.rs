//! Inkwell-backed implementation of the [`super::ir::IR`] trait.
//!
//! Phase IR.2 of the inkwell-shim refactor (see `ir.rs` and ROADMAP §7.5).
//!
//! # Goal
//!
//! Bound *all* inkwell trait machinery to this single impl block.  Codegen
//! modules call `self.ir.iadd(a, b)` (monomorphic over the IR trait); only
//! THIS file lowers those calls into `builder.build_int_add(...)` (which
//! drags in the heavy inkwell generics).
//!
//! # Arena layout
//!
//! Five `Vec`s, indexed by the handle types from `ir.rs`.  Handles are
//! plain `u32` so they round-trip through HashMap / Vec / iteration without
//! lifetime entanglement.  This is the same pattern cranelift uses
//! internally — proven, no unsafe required.
//!
//! ```text
//! IRValue(idx)    → values:    Vec<BasicValueEnum<'ctx>>
//! IRType(idx)     → types:     Vec<BasicTypeEnum<'ctx>>     (or void = sentinel)
//! IRBlock(idx)    → blocks:    Vec<BasicBlock<'ctx>>
//! IRFunction(idx) → functions: Vec<FunctionValue<'ctx>>
//! IRGlobal(idx)   → globals:   Vec<GlobalValue<'ctx>>
//! ```
//!
//! Lookups are O(1).  Per-call cost is one Vec index (cache-friendly).
//!
//! # Lifetime scoping
//!
//! `InkwellBackend<'ctx>` holds `&'ctx Context` plus owned `Module<'ctx>`
//! and `Builder<'ctx>`.  All values stored in the arenas are `<'ctx>`-bound.
//! When the backend is dropped, all arenas are torn down with the context.

#![allow(dead_code)] // Used once the codegen modules migrate to self.ir.*

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, FunctionType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FunctionValue, GlobalValue, PointerValue,
};
use inkwell::AddressSpace;
use inkwell::{FloatPredicate, IntPredicate};

use super::ir::{
    IRAddrSpace, IRBlock, IRFloatPred, IRFunction, IRGlobal, IRIntPred, IROptLevel, IRType,
    IRValue, IR,
};

// ── Backend struct ──────────────────────────────────────────────────────────

/// LLVM/inkwell-backed implementation of [`IR`].
///
/// Holds a borrowed `Context` and owned `Module` + `Builder`, plus arenas
/// of LLVM types/values/blocks/functions/globals indexed by IR* handles.
pub struct InkwellBackend<'ctx> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,

    /// Arena: `IRValue(idx)` → `values[idx]`.  Slot 0 is reserved as a
    /// sentinel (so `IRValue(0)` can mean "no value" if needed).
    values: Vec<BasicValueEnum<'ctx>>,
    /// Arena: `IRType(idx)` → `types[idx]`.  Slot 0 reserved as void.
    types: Vec<Option<BasicTypeEnum<'ctx>>>, // None = void
    blocks: Vec<BasicBlock<'ctx>>,
    functions: Vec<FunctionValue<'ctx>>,
    globals: Vec<GlobalValue<'ctx>>,
    /// Function types kept separately because inkwell's `FunctionType<'ctx>`
    /// doesn't sit in the basic-type enum.  Indexed when `t_fn` is called.
    fn_types: Vec<FunctionType<'ctx>>,

    /// Optional name counter for synthesized SSA names.  inkwell requires
    /// unique strings; we generate `v_0`, `v_1`, ... for value names.
    name_counter: u32,
}

impl<'ctx> InkwellBackend<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        let mut backend = Self {
            context,
            module,
            builder,
            values: Vec::new(),
            types: vec![None], // index 0 = void
            blocks: Vec::new(),
            functions: Vec::new(),
            globals: Vec::new(),
            fn_types: Vec::new(),
            name_counter: 0,
        };
        // Pre-register sentinel value to keep arena indices ≥ 1 if we want.
        // For now we allow IRValue(0) to be a real value; users rely on
        // method semantics to determine "no value".
        backend
    }

    fn next_name(&mut self, prefix: &str) -> String {
        let n = self.name_counter;
        self.name_counter += 1;
        format!("{}_{}", prefix, n)
    }

    fn store_val(&mut self, v: BasicValueEnum<'ctx>) -> IRValue {
        let idx = self.values.len() as u32;
        self.values.push(v);
        IRValue(idx)
    }
    fn store_ty(&mut self, t: BasicTypeEnum<'ctx>) -> IRType {
        let idx = self.types.len() as u32;
        self.types.push(Some(t));
        IRType(idx)
    }
    fn store_block(&mut self, b: BasicBlock<'ctx>) -> IRBlock {
        let idx = self.blocks.len() as u32;
        self.blocks.push(b);
        IRBlock(idx)
    }
    fn store_fn(&mut self, f: FunctionValue<'ctx>) -> IRFunction {
        let idx = self.functions.len() as u32;
        self.functions.push(f);
        IRFunction(idx)
    }
    fn store_global(&mut self, g: GlobalValue<'ctx>) -> IRGlobal {
        let idx = self.globals.len() as u32;
        self.globals.push(g);
        IRGlobal(idx)
    }
    fn store_fn_ty(&mut self, ft: FunctionType<'ctx>) -> IRType {
        // Function types share the IRType handle space but live in fn_types.
        // We tag them by encoding the index with a high bit: index | 0x80000000.
        let idx = self.fn_types.len() as u32;
        self.fn_types.push(ft);
        IRType(idx | 0x8000_0000)
    }

    fn val(&self, h: IRValue) -> BasicValueEnum<'ctx> {
        self.values[h.0 as usize]
    }
    fn ty(&self, h: IRType) -> BasicTypeEnum<'ctx> {
        self.types[h.0 as usize]
            .expect("attempted to use void as a basic type — wrong API call")
    }
    fn ty_opt(&self, h: IRType) -> Option<BasicTypeEnum<'ctx>> {
        self.types[h.0 as usize]
    }
    fn block(&self, h: IRBlock) -> BasicBlock<'ctx> {
        self.blocks[h.0 as usize]
    }
    fn func(&self, h: IRFunction) -> FunctionValue<'ctx> {
        self.functions[h.0 as usize]
    }
    fn glob(&self, h: IRGlobal) -> GlobalValue<'ctx> {
        self.globals[h.0 as usize]
    }
    fn fn_ty(&self, h: IRType) -> FunctionType<'ctx> {
        let idx = (h.0 & 0x7fff_ffff) as usize;
        self.fn_types[idx]
    }

    fn translate_int_pred(p: IRIntPred) -> IntPredicate {
        match p {
            IRIntPred::Eq => IntPredicate::EQ,
            IRIntPred::Ne => IntPredicate::NE,
            IRIntPred::Slt => IntPredicate::SLT,
            IRIntPred::Sle => IntPredicate::SLE,
            IRIntPred::Sgt => IntPredicate::SGT,
            IRIntPred::Sge => IntPredicate::SGE,
            IRIntPred::Ult => IntPredicate::ULT,
            IRIntPred::Ule => IntPredicate::ULE,
            IRIntPred::Ugt => IntPredicate::UGT,
            IRIntPred::Uge => IntPredicate::UGE,
        }
    }

    fn translate_float_pred(p: IRFloatPred) -> FloatPredicate {
        match p {
            IRFloatPred::Oeq => FloatPredicate::OEQ,
            IRFloatPred::Ogt => FloatPredicate::OGT,
            IRFloatPred::Oge => FloatPredicate::OGE,
            IRFloatPred::Olt => FloatPredicate::OLT,
            IRFloatPred::Ole => FloatPredicate::OLE,
            IRFloatPred::One => FloatPredicate::ONE,
            IRFloatPred::Ord => FloatPredicate::ORD,
            IRFloatPred::Uno => FloatPredicate::UNO,
            IRFloatPred::Ueq => FloatPredicate::UEQ,
            IRFloatPred::Ugt => FloatPredicate::UGT,
            IRFloatPred::Uge => FloatPredicate::UGE,
            IRFloatPred::Ult => FloatPredicate::ULT,
            IRFloatPred::Ule => FloatPredicate::ULE,
            IRFloatPred::Une => FloatPredicate::UNE,
        }
    }
}

// ── IR trait impl ───────────────────────────────────────────────────────────
//
// Method ordering matches `ir.rs` for ease of cross-referencing.

impl<'ctx> IR for InkwellBackend<'ctx> {
    // ── Type construction ─────────────────────────────────────────────────
    fn t_void(&mut self) -> IRType {
        IRType(0) // sentinel = void
    }
    fn t_bool(&mut self) -> IRType {
        let t = self.context.bool_type().into();
        self.store_ty(t)
    }
    fn t_i8(&mut self) -> IRType {
        let t = self.context.i8_type().into();
        self.store_ty(t)
    }
    fn t_i16(&mut self) -> IRType {
        let t = self.context.i16_type().into();
        self.store_ty(t)
    }
    fn t_i32(&mut self) -> IRType {
        let t = self.context.i32_type().into();
        self.store_ty(t)
    }
    fn t_i64(&mut self) -> IRType {
        let t = self.context.i64_type().into();
        self.store_ty(t)
    }
    fn t_u8(&mut self) -> IRType {
        // LLVM has no signed/unsigned distinction at the type level.
        self.t_i8()
    }
    fn t_u16(&mut self) -> IRType {
        self.t_i16()
    }
    fn t_u32(&mut self) -> IRType {
        self.t_i32()
    }
    fn t_u64(&mut self) -> IRType {
        self.t_i64()
    }
    fn t_f32(&mut self) -> IRType {
        let t = self.context.f32_type().into();
        self.store_ty(t)
    }
    fn t_f64(&mut self) -> IRType {
        let t = self.context.f64_type().into();
        self.store_ty(t)
    }
    fn t_ptr(&mut self) -> IRType {
        let t = self.context.i8_type().ptr_type(AddressSpace::default()).into();
        self.store_ty(t)
    }
    fn t_array(&mut self, elem: IRType, n: u32) -> IRType {
        let elem_ty = self.ty(elem);
        let arr = elem_ty.array_type(n).into();
        self.store_ty(arr)
    }
    fn t_struct(&mut self, fields: &[IRType], packed: bool) -> IRType {
        let field_tys: Vec<BasicTypeEnum<'ctx>> = fields.iter().map(|h| self.ty(*h)).collect();
        let s = self.context.struct_type(&field_tys, packed).into();
        self.store_ty(s)
    }
    fn t_named_struct(&mut self, name: &str, fields: &[IRType], packed: bool) -> IRType {
        let field_tys: Vec<BasicTypeEnum<'ctx>> = fields.iter().map(|h| self.ty(*h)).collect();
        let s = self.context.opaque_struct_type(name);
        s.set_body(&field_tys, packed);
        self.store_ty(s.into())
    }
    fn t_fn(&mut self, params: &[IRType], ret: Option<IRType>, variadic: bool) -> IRType {
        let param_tys: Vec<BasicMetadataTypeEnum<'ctx>> =
            params.iter().map(|h| self.ty(*h).into()).collect();
        let fn_ty = match ret.and_then(|h| self.ty_opt(h)) {
            Some(r) => r.fn_type(&param_tys, variadic),
            None => self.context.void_type().fn_type(&param_tys, variadic),
        };
        self.store_fn_ty(fn_ty)
    }

    // ── Type queries ──────────────────────────────────────────────────────
    fn ty_of(&self, val: IRValue) -> IRType {
        let v = self.val(val);
        // Small one-time alloc; would be nicer with intern table but adequate.
        // We don't store back into self because &self only.
        // For now, pre-cache common types by index — but that's deferred.
        // This method is rarely called; the cost is acceptable.
        let _ = v;
        // TODO(IR.4): proper type round-trip.  For now return t_i64 as a
        // placeholder — callers that need exact types use ty_of sparingly.
        IRType(0)
    }
    fn t_get_struct_field(&self, ty: IRType, idx: u32) -> Option<IRType> {
        if let BasicTypeEnum::StructType(s) = self.ty(ty) {
            s.get_field_type_at_index(idx).map(|f| {
                // Can't store_ty from &self; field types fall through.
                // Most callers index immediately; full support deferred.
                let _ = f;
                IRType(0)
            })
        } else {
            None
        }
    }
    fn t_get_named_struct(&self, name: &str) -> Option<IRType> {
        self.module.get_struct_type(name).map(|s| {
            let _ = s;
            IRType(0) // placeholder — full lookup deferred to IR.4
        })
    }

    // ── Constants ─────────────────────────────────────────────────────────
    fn const_i64(&mut self, v: i64) -> IRValue {
        let val = self.context.i64_type().const_int(v as u64, true).into();
        self.store_val(val)
    }
    fn const_i32(&mut self, v: i32) -> IRValue {
        let val = self.context.i32_type().const_int(v as u64, true).into();
        self.store_val(val)
    }
    fn const_i8(&mut self, v: u8) -> IRValue {
        let val = self.context.i8_type().const_int(v as u64, false).into();
        self.store_val(val)
    }
    fn const_bool(&mut self, v: bool) -> IRValue {
        let val = self.context.bool_type().const_int(v as u64, false).into();
        self.store_val(val)
    }
    fn const_f64(&mut self, v: f64) -> IRValue {
        let val = self.context.f64_type().const_float(v).into();
        self.store_val(val)
    }
    fn const_zero(&mut self, ty: IRType) -> IRValue {
        let t = self.ty(ty);
        let val = match t {
            BasicTypeEnum::IntType(it) => it.const_zero().into(),
            BasicTypeEnum::FloatType(ft) => ft.const_zero().into(),
            BasicTypeEnum::PointerType(pt) => pt.const_null().into(),
            BasicTypeEnum::StructType(st) => st.const_zero().into(),
            BasicTypeEnum::ArrayType(at) => at.const_zero().into(),
            BasicTypeEnum::VectorType(vt) => vt.const_zero().into(),
        };
        self.store_val(val)
    }
    fn const_undef(&mut self, ty: IRType) -> IRValue {
        let t = self.ty(ty);
        let val: BasicValueEnum<'ctx> = match t {
            BasicTypeEnum::IntType(it) => it.get_undef().into(),
            BasicTypeEnum::FloatType(ft) => ft.get_undef().into(),
            BasicTypeEnum::PointerType(pt) => pt.get_undef().into(),
            BasicTypeEnum::StructType(st) => st.get_undef().into(),
            BasicTypeEnum::ArrayType(at) => at.get_undef().into(),
            BasicTypeEnum::VectorType(vt) => vt.get_undef().into(),
        };
        self.store_val(val)
    }
    fn const_struct(&mut self, fields: &[IRValue], packed: bool) -> IRValue {
        let vals: Vec<BasicValueEnum<'ctx>> = fields.iter().map(|h| self.val(*h)).collect();
        let val = self.context.const_struct(&vals, packed).into();
        self.store_val(val)
    }
    fn const_array(&mut self, elem_ty: IRType, elems: &[IRValue]) -> IRValue {
        let _ = elem_ty;
        // Inkwell needs typed arrays; defer to caller using elem_ty for shape.
        // For homogenous int arrays we use the first element's type.
        if let Some(first) = elems.first() {
            let first_v = self.val(*first);
            match first_v {
                BasicValueEnum::IntValue(_) => {
                    // Build array via int_type.const_array — needs Vec<IntValue>.
                    // For simplicity, fall through to an undef array; real impl
                    // collects into IntValue<'ctx> array.  TODO(IR.2): finish.
                    let len = elems.len() as u32;
                    let arr = first_v.get_type().array_type(len).get_undef().into();
                    self.store_val(arr)
                }
                _ => {
                    let len = elems.len() as u32;
                    let arr = first_v.get_type().array_type(len).get_undef().into();
                    self.store_val(arr)
                }
            }
        } else {
            // Empty — return undef of the element type as a 0-length array.
            let t = self.ty(elem_ty);
            let arr = t.array_type(0).get_undef().into();
            self.store_val(arr)
        }
    }
    fn const_string(&mut self, s: &str, null_terminate: bool) -> IRValue {
        let val = self.context.const_string(s.as_bytes(), null_terminate).into();
        self.store_val(val)
    }
    fn global_string(&mut self, s: &str, name: &str) -> IRGlobal {
        let g = self.builder.build_global_string_ptr(s, name).unwrap();
        // build_global_string_ptr returns a GlobalValue<'ctx> wrapper.
        // Convert via as_pointer_value().get_first_use() — simpler: just
        // store the pointer-typed value as a pseudo-global.  Real impl
        // uses module.add_global; this is a bridge.
        let pv: PointerValue<'ctx> = g.as_pointer_value();
        let _ = pv;
        // For now, register into globals via the module's lookup.
        // The proper API: build_global_string_ptr already creates a global
        // via the module; we just need to find it.  TODO(IR.2): polish.
        let placeholder = self
            .module
            .add_global(self.context.i8_type().array_type(s.len() as u32 + 1), None, name);
        self.store_global(placeholder)
    }

    // ── Function / module ─────────────────────────────────────────────────
    fn add_function(&mut self, name: &str, ty: IRType) -> IRFunction {
        let ft = self.fn_ty(ty);
        let f = self.module.add_function(name, ft, None);
        self.store_fn(f)
    }
    fn get_function(&self, name: &str) -> Option<IRFunction> {
        self.module.get_function(name).map(|f| {
            // Can't push into self (we're &self).  Linear-scan existing
            // arena; if not found, this method's caller must accept a
            // freshly-alloc'd handle via IR.4 polish.
            let _ = f;
            IRFunction(0) // placeholder — full lookup deferred
        })
    }
    fn add_global(&mut self, name: &str, ty: IRType, init: Option<IRValue>) -> IRGlobal {
        let t = self.ty(ty);
        let g = self.module.add_global(t, None, name);
        if let Some(iv) = init {
            // Initial-value setter requires the constant to be of the matching type.
            // inkwell's set_initializer takes `&dyn BasicValue`.  We do a small dance:
            let v = self.val(iv);
            match v {
                BasicValueEnum::IntValue(x) => g.set_initializer(&x),
                BasicValueEnum::FloatValue(x) => g.set_initializer(&x),
                BasicValueEnum::PointerValue(x) => g.set_initializer(&x),
                BasicValueEnum::StructValue(x) => g.set_initializer(&x),
                BasicValueEnum::ArrayValue(x) => g.set_initializer(&x),
                BasicValueEnum::VectorValue(x) => g.set_initializer(&x),
            }
        }
        self.store_global(g)
    }
    fn fn_param(&self, f: IRFunction, idx: u32) -> IRValue {
        let func = self.func(f);
        let _ = func.get_nth_param(idx);
        // Same as get_function: can't mutate self in &self method.
        // TODO(IR.4): refactor to take &mut self.
        IRValue(0)
    }
    fn fn_as_value(&self, f: IRFunction) -> IRValue {
        let func = self.func(f);
        let _ = func.as_global_value().as_pointer_value();
        IRValue(0)
    }
    fn global_as_value(&self, g: IRGlobal) -> IRValue {
        let glob = self.glob(g);
        let _ = glob.as_pointer_value();
        IRValue(0)
    }

    // ── Basic blocks ──────────────────────────────────────────────────────
    fn append_block(&mut self, f: IRFunction, name: &str) -> IRBlock {
        let func = self.func(f);
        let b = self.context.append_basic_block(func, name);
        self.store_block(b)
    }
    fn position_at_end(&mut self, b: IRBlock) {
        let block = self.block(b);
        self.builder.position_at_end(block);
    }
    fn current_block(&self) -> Option<IRBlock> {
        let b = self.builder.get_insert_block()?;
        // Linear scan to find handle; small N typically.
        self.blocks.iter().position(|x| *x == b).map(|i| IRBlock(i as u32))
    }
    fn block_terminated(&self, b: IRBlock) -> bool {
        self.block(b).get_terminator().is_some()
    }
    fn block_parent_fn(&self, b: IRBlock) -> IRFunction {
        let func = self.block(b).get_parent().unwrap();
        self.functions
            .iter()
            .position(|f| *f == func)
            .map(|i| IRFunction(i as u32))
            .unwrap_or(IRFunction(0))
    }

    // ── Memory ────────────────────────────────────────────────────────────
    fn alloca(&mut self, ty: IRType) -> IRValue {
        let t = self.ty(ty);
        let name = self.next_name("alloca");
        let p = self.builder.build_alloca(t, &name).unwrap();
        self.store_val(p.into())
    }
    fn load(&mut self, ty: IRType, ptr: IRValue) -> IRValue {
        let t = self.ty(ty);
        let p = self.val(ptr).into_pointer_value();
        let name = self.next_name("load");
        let v = self.builder.build_load(t, p, &name).unwrap();
        self.store_val(v)
    }
    fn store(&mut self, ptr: IRValue, val: IRValue) {
        let p = self.val(ptr).into_pointer_value();
        let v = self.val(val);
        let _ = self.builder.build_store(p, v);
    }
    fn struct_gep(&mut self, ty: IRType, ptr: IRValue, idx: u32) -> IRValue {
        let t = self.ty(ty);
        let p = self.val(ptr).into_pointer_value();
        let name = self.next_name("gep");
        let g = self.builder.build_struct_gep(t, p, idx, &name).unwrap();
        self.store_val(g.into())
    }
    fn array_gep(&mut self, ty: IRType, ptr: IRValue, idx: IRValue) -> IRValue {
        let t = self.ty(ty);
        let p = self.val(ptr).into_pointer_value();
        let i = self.val(idx).into_int_value();
        let name = self.next_name("agep");
        let g = unsafe { self.builder.build_gep(t, p, &[i], &name) }.unwrap();
        self.store_val(g.into())
    }
    fn ptr_cast(&mut self, ptr: IRValue, target_ty: IRType) -> IRValue {
        let p = self.val(ptr).into_pointer_value();
        let t = match self.ty(target_ty) {
            BasicTypeEnum::PointerType(pt) => pt,
            _ => panic!("ptr_cast target_ty is not a pointer type"),
        };
        let name = self.next_name("cast");
        let c = self.builder.build_pointer_cast(p, t, &name).unwrap();
        self.store_val(c.into())
    }
    fn ptr_to_int(&mut self, ptr: IRValue, int_ty: IRType) -> IRValue {
        let p = self.val(ptr).into_pointer_value();
        let t = match self.ty(int_ty) {
            BasicTypeEnum::IntType(it) => it,
            _ => panic!("ptr_to_int int_ty is not an integer type"),
        };
        let name = self.next_name("p2i");
        let v = self.builder.build_ptr_to_int(p, t, &name).unwrap();
        self.store_val(v.into())
    }

    // ── Aggregate ─────────────────────────────────────────────────────────
    fn extract_value(&mut self, agg: IRValue, idx: u32) -> IRValue {
        let a = self.val(agg);
        let name = self.next_name("ext");
        let v = match a {
            BasicValueEnum::StructValue(sv) => self.builder.build_extract_value(sv, idx, &name),
            BasicValueEnum::ArrayValue(av) => self.builder.build_extract_value(av, idx, &name),
            _ => panic!("extract_value on non-aggregate"),
        }
        .unwrap();
        self.store_val(v)
    }
    fn insert_value(&mut self, agg: IRValue, val: IRValue, idx: u32) -> IRValue {
        let a = self.val(agg);
        let v = self.val(val);
        let name = self.next_name("ins");
        let r = match a {
            BasicValueEnum::StructValue(sv) => {
                self.builder.build_insert_value(sv, v, idx, &name).unwrap()
            }
            BasicValueEnum::ArrayValue(av) => {
                self.builder.build_insert_value(av, v, idx, &name).unwrap()
            }
            _ => panic!("insert_value on non-aggregate"),
        };
        // build_insert_value returns AggregateValueEnum; we convert.
        let v: BasicValueEnum<'ctx> = match r {
            inkwell::values::AggregateValueEnum::ArrayValue(av) => av.into(),
            inkwell::values::AggregateValueEnum::StructValue(sv) => sv.into(),
        };
        self.store_val(v)
    }

    // ── Integer arithmetic ────────────────────────────────────────────────
    fn iadd(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("add");
        let r = self.builder.build_int_add(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn isub(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("sub");
        let r = self.builder.build_int_sub(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn imul(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("mul");
        let r = self.builder.build_int_mul(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn idiv_signed(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("sdiv");
        let r = self.builder.build_int_signed_div(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn idiv_unsigned(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("udiv");
        let r = self.builder.build_int_unsigned_div(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn irem_signed(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("srem");
        let r = self.builder.build_int_signed_rem(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn irem_unsigned(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("urem");
        let r = self.builder.build_int_unsigned_rem(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn ineg(&mut self, a: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let name = self.next_name("neg");
        let r = self.builder.build_int_neg(av, &name).unwrap();
        self.store_val(r.into())
    }
    fn icmp(&mut self, pred: IRIntPred, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("cmp");
        let p = Self::translate_int_pred(pred);
        let r = self.builder.build_int_compare(p, av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn shl(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("shl");
        let r = self.builder.build_left_shift(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn shr(&mut self, a: IRValue, b: IRValue, signed: bool) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("shr");
        let r = self.builder.build_right_shift(av, bv, signed, &name).unwrap();
        self.store_val(r.into())
    }
    fn iand(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("and");
        let r = self.builder.build_and(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn ior(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("or");
        let r = self.builder.build_or(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn ixor(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let bv = self.val(b).into_int_value();
        let name = self.next_name("xor");
        let r = self.builder.build_xor(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn inot(&mut self, a: IRValue) -> IRValue {
        let av = self.val(a).into_int_value();
        let name = self.next_name("not");
        let r = self.builder.build_not(av, &name).unwrap();
        self.store_val(r.into())
    }

    // ── Integer width conversions ─────────────────────────────────────────
    fn int_truncate(&mut self, a: IRValue, target_ty: IRType) -> IRValue {
        let av = self.val(a).into_int_value();
        let t = match self.ty(target_ty) {
            BasicTypeEnum::IntType(it) => it,
            _ => panic!("int_truncate target is not int"),
        };
        let name = self.next_name("trunc");
        let r = self.builder.build_int_truncate(av, t, &name).unwrap();
        self.store_val(r.into())
    }
    fn int_zext(&mut self, a: IRValue, target_ty: IRType) -> IRValue {
        let av = self.val(a).into_int_value();
        let t = match self.ty(target_ty) {
            BasicTypeEnum::IntType(it) => it,
            _ => panic!("int_zext target is not int"),
        };
        let name = self.next_name("zext");
        let r = self.builder.build_int_z_extend(av, t, &name).unwrap();
        self.store_val(r.into())
    }
    fn int_sext(&mut self, a: IRValue, target_ty: IRType) -> IRValue {
        let av = self.val(a).into_int_value();
        let t = match self.ty(target_ty) {
            BasicTypeEnum::IntType(it) => it,
            _ => panic!("int_sext target is not int"),
        };
        let name = self.next_name("sext");
        let r = self.builder.build_int_s_extend(av, t, &name).unwrap();
        self.store_val(r.into())
    }

    // ── Float arithmetic ──────────────────────────────────────────────────
    fn fadd(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_float_value();
        let bv = self.val(b).into_float_value();
        let name = self.next_name("fadd");
        let r = self.builder.build_float_add(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn fsub(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_float_value();
        let bv = self.val(b).into_float_value();
        let name = self.next_name("fsub");
        let r = self.builder.build_float_sub(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn fmul(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_float_value();
        let bv = self.val(b).into_float_value();
        let name = self.next_name("fmul");
        let r = self.builder.build_float_mul(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn fdiv(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_float_value();
        let bv = self.val(b).into_float_value();
        let name = self.next_name("fdiv");
        let r = self.builder.build_float_div(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn frem(&mut self, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_float_value();
        let bv = self.val(b).into_float_value();
        let name = self.next_name("frem");
        let r = self.builder.build_float_rem(av, bv, &name).unwrap();
        self.store_val(r.into())
    }
    fn fneg(&mut self, a: IRValue) -> IRValue {
        let av = self.val(a).into_float_value();
        let name = self.next_name("fneg");
        let r = self.builder.build_float_neg(av, &name).unwrap();
        self.store_val(r.into())
    }
    fn fcmp(&mut self, pred: IRFloatPred, a: IRValue, b: IRValue) -> IRValue {
        let av = self.val(a).into_float_value();
        let bv = self.val(b).into_float_value();
        let p = Self::translate_float_pred(pred);
        let name = self.next_name("fcmp");
        let r = self.builder.build_float_compare(p, av, bv, &name).unwrap();
        self.store_val(r.into())
    }

    // ── Float ↔ int ───────────────────────────────────────────────────────
    fn sitof(&mut self, a: IRValue, target_ty: IRType) -> IRValue {
        let av = self.val(a).into_int_value();
        let t = match self.ty(target_ty) {
            BasicTypeEnum::FloatType(ft) => ft,
            _ => panic!("sitof target not float"),
        };
        let name = self.next_name("itof");
        let r = self.builder.build_signed_int_to_float(av, t, &name).unwrap();
        self.store_val(r.into())
    }
    fn ftosi(&mut self, a: IRValue, target_ty: IRType) -> IRValue {
        let av = self.val(a).into_float_value();
        let t = match self.ty(target_ty) {
            BasicTypeEnum::IntType(it) => it,
            _ => panic!("ftosi target not int"),
        };
        let name = self.next_name("ftoi");
        let r = self.builder.build_float_to_signed_int(av, t, &name).unwrap();
        self.store_val(r.into())
    }

    // ── Control flow ──────────────────────────────────────────────────────
    fn br(&mut self, target: IRBlock) {
        let b = self.block(target);
        let _ = self.builder.build_unconditional_branch(b);
    }
    fn cond_br(&mut self, cond: IRValue, then_b: IRBlock, else_b: IRBlock) {
        let c = self.val(cond).into_int_value();
        let t = self.block(then_b);
        let e = self.block(else_b);
        let _ = self.builder.build_conditional_branch(c, t, e);
    }
    fn switch(&mut self, scrutinee: IRValue, default: IRBlock, cases: &[(IRValue, IRBlock)]) {
        let s = self.val(scrutinee).into_int_value();
        let d = self.block(default);
        let case_pairs: Vec<(inkwell::values::IntValue<'ctx>, BasicBlock<'ctx>)> = cases
            .iter()
            .map(|(v, b)| (self.val(*v).into_int_value(), self.block(*b)))
            .collect();
        let _ = self.builder.build_switch(s, d, &case_pairs);
    }
    fn ret_void(&mut self) {
        let _ = self.builder.build_return(None);
    }
    fn ret(&mut self, val: IRValue) {
        let v = self.val(val);
        let _ = self.builder.build_return(Some(&v));
    }
    fn unreachable(&mut self) {
        let _ = self.builder.build_unreachable();
    }
    fn select(&mut self, cond: IRValue, then_v: IRValue, else_v: IRValue) -> IRValue {
        let c = self.val(cond).into_int_value();
        let t = self.val(then_v);
        let e = self.val(else_v);
        let name = self.next_name("sel");
        let r = self.builder.build_select(c, t, e, &name).unwrap();
        self.store_val(r)
    }
    fn phi(&mut self, ty: IRType) -> IRValue {
        let t = self.ty(ty);
        let name = self.next_name("phi");
        let p = self.builder.build_phi(t, &name).unwrap();
        // Phi values are stored as their basic-value cast.  We round-trip
        // through as_basic_value to land in BasicValueEnum.
        self.store_val(p.as_basic_value())
    }
    fn phi_add_incoming(&mut self, _phi: IRValue, _val: IRValue, _b: IRBlock) {
        // Phi nodes need to retain their PhiValue handle to call add_incoming.
        // Our arena only stores BasicValueEnum, which loses the phi-specific
        // API.  TODO(IR.4): add a separate phi arena, or use a side table.
        // For now this is a stub — full phi support deferred.
    }

    // ── Calls ─────────────────────────────────────────────────────────────
    fn call(&mut self, callee: IRFunction, args: &[IRValue]) -> Option<IRValue> {
        let f = self.func(callee);
        let arg_vals: Vec<BasicMetadataValueEnum<'ctx>> =
            args.iter().map(|h| self.val(*h).into()).collect();
        let name = self.next_name("call");
        let cs = self.builder.build_call(f, &arg_vals, &name).ok()?;
        cs.try_as_basic_value().left().map(|v| self.store_val(v))
    }
    fn call_indirect(&mut self, sig: IRType, fn_ptr: IRValue, args: &[IRValue]) -> Option<IRValue> {
        let ft = self.fn_ty(sig);
        let p = self.val(fn_ptr).into_pointer_value();
        let arg_vals: Vec<BasicMetadataValueEnum<'ctx>> =
            args.iter().map(|h| self.val(*h).into()).collect();
        let name = self.next_name("icall");
        let cs = self.builder.build_indirect_call(ft, p, &arg_vals, &name).ok()?;
        cs.try_as_basic_value().left().map(|v| self.store_val(v))
    }

    // ── Output ────────────────────────────────────────────────────────────
    fn verify(&self) -> Result<(), String> {
        self.module.verify().map_err(|e| e.to_string())
    }
    fn write_ir_text(&self, path: &str) -> Result<(), String> {
        self.module
            .print_to_file(std::path::Path::new(path))
            .map_err(|e| e.to_string())
    }
    fn emit_bitcode(&self) -> Vec<u8> {
        self.module.write_bitcode_to_memory().as_slice().to_vec()
    }
}

// Suppress unused warnings on the helper enums until callers migrate.
#[allow(dead_code)]
fn _unused_addr_space(_: IRAddrSpace) {}
#[allow(dead_code)]
fn _unused_opt_level(_: IROptLevel) {}
