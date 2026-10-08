//! Native rendering of an interpolated value (`"{v}"`), by its static type.
//!
//! The interpreter's `display` (`interp/value.rs`) is the oracle, and this
//! reproduces its text byte for byte:
//!
//! * a struct is `Name { a: 1, b: 2 }` and an enum variant `Enum::V { a: 1 }`,
//!   or `Enum::V` when it has no fields. Fields are NOT in declaration order:
//!   the interpreter sorts the rendered `name: value` strings, so the order is
//!   that of `name:` compared bytewise (`a1` sorts before `a`, because `1` is
//!   below `:`). Field names are distinct, so that order is fixed per type and
//!   is computed here, at compile time. A struct with no fields renders as
//!   `Name {  }` (two spaces), which is also what the interpreter prints;
//! * `Some(x)`/`None`, `Ok(x)`/`Err(x)`, `[a, b]`, `(a, b)`;
//! * a `str` is its raw contents, with no quotes or escaping, at any depth;
//!   integers in decimal at their own signedness, `f64` as `%.6g`, `bool` as
//!   `true`/`false` — the same runtime formatters top-level interpolation uses.
//!
//! A scalar renders inline. A compound type renders through one helper
//! function per type, `__axon_display_N(T) -> str`, built on first use and
//! memoised in `display_fns`; a recursive enum's helper calls itself for its
//! boxed fields, so generation terminates. Anything else (`Decimal`, a dict,
//! a channel, a closure, `Uncertain`, a payload of type `()`) has no native
//! rendering yet, and `display_blocker` names it so the interpolation is
//! refused with E0910 instead of printing something the interpreter would not.

use std::collections::HashSet;

use inkwell::module::Linkage;
use inkwell::types::BasicType;
use inkwell::values::{BasicValueEnum, FunctionValue, StructValue};
use inkwell::IntPredicate;

use crate::ast;
use crate::types::Type;

use super::build_wrappers;

/// A `str` being built from literal text and rendered values. Adjacent
/// literal pieces are merged, so `Name { ` + `a: ` costs one concat, not two.
struct StrAcc<'ctx> {
    acc: Option<BasicValueEnum<'ctx>>,
    pending: String,
}

impl StrAcc<'_> {
    fn new(prefix: &str) -> Self {
        StrAcc {
            acc: None,
            pending: prefix.to_string(),
        }
    }

    fn lit(&mut self, s: &str) {
        self.pending.push_str(s);
    }
}

/// The interpreter's field order: by the rendered `name: value` text, which
/// is decided by `name:` alone because field names are distinct.
fn display_order(names: &[String]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..names.len()).collect();
    order.sort_by_cached_key(|&i| format!("{}:", names[i]));
    order
}

impl<'ctx> super::Codegen<'ctx> {
    /// The first component of `ty` that native codegen cannot render the way
    /// the interpreter's `display` does, or `None` when all of `ty` renders.
    /// Decided from the semantic types alone, before anything is emitted, so
    /// a helper is never left half built. `visiting` holds the nominal types
    /// being decided: reaching one again (a recursive enum) adds nothing new.
    pub(super) fn display_blocker(&self, ty: &Type, visiting: &mut HashSet<Type>) -> Option<Type> {
        let blocked = || Some(ty.clone());
        match ty {
            Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Str => None,
            Type::Struct(name) => {
                if !visiting.insert(ty.clone()) {
                    return None;
                }
                let (Some(names), Some(tys)) = (
                    self.struct_fields.get(name),
                    self.struct_field_sem_types.get(name),
                ) else {
                    return blocked();
                };
                if names.len() != tys.len() || self.llvm_type(ty).is_none() {
                    return blocked();
                }
                tys.iter().find_map(|t| self.display_blocker(t, visiting))
            }
            Type::Enum(name) => {
                if !visiting.insert(ty.clone()) {
                    return None;
                }
                let Some(variants) = self.enum_variants.get(name) else {
                    return blocked();
                };
                if self.llvm_type(ty).is_none() {
                    return blocked();
                }
                variants
                    .iter()
                    .flat_map(|(_, _, fs)| fs)
                    .find_map(|f| self.payload_blocker(&f.ty, visiting))
            }
            Type::Tuple(tys) => tys.iter().find_map(|t| self.payload_blocker(t, visiting)),
            Type::Slice(elem) | Type::Option(elem) => self.payload_blocker(elem, visiting),
            Type::Result(ok, err) => self
                .payload_blocker(ok, visiting)
                .or_else(|| self.payload_blocker(err, visiting)),
            _ => blocked(),
        }
    }

    /// `display_blocker` for a value held inside another, which must also
    /// have an LLVM type to be loaded at all (a `()` payload has none).
    fn payload_blocker(&self, ty: &Type, visiting: &mut HashSet<Type>) -> Option<Type> {
        if self.llvm_type(ty).is_none() {
            return Some(ty.clone());
        }
        self.display_blocker(ty, visiting)
    }

    /// Render `v`, a value of semantic type `ty`, to a `str` value exactly as
    /// the interpreter's `display` would. The caller has checked
    /// `display_blocker(ty)` is `None`.
    pub(super) fn emit_value_display(
        &mut self,
        v: BasicValueEnum<'ctx>,
        ty: &Type,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let call1 = |cg: &Self, name: &str, arg: BasicValueEnum<'ctx>| {
            let f = cg.functions.get(name).copied()?;
            build_wrappers::w_call(&cg.ir.builder, f, &[arg.into()], "disp")
                .try_as_basic_value()
                .left()
        };
        match ty {
            Type::Str => Some(v),
            Type::I64 => call1(self, "to_str", v),
            Type::I8 | Type::I16 | Type::I32 => {
                let wide = build_wrappers::w_int_s_extend(
                    &self.ir.builder,
                    v.into_int_value(),
                    i64_ty,
                    "disp_sext",
                );
                call1(self, "to_str", wide.into())
            }
            Type::U8 | Type::U16 | Type::U32 => {
                let wide = build_wrappers::w_int_z_extend(
                    &self.ir.builder,
                    v.into_int_value(),
                    i64_ty,
                    "disp_zext",
                );
                call1(self, "to_str", wide.into())
            }
            Type::U64 => self.emit_u64_to_str(v.into_int_value()),
            Type::Bool => call1(self, "to_str_bool", v),
            Type::F64 => call1(self, "to_str_f64", v),
            Type::F32 => {
                let wide = build_wrappers::w_float_ext(
                    &self.ir.builder,
                    v.into_float_value(),
                    self.ir.context.f64_type(),
                    "disp_fext",
                );
                call1(self, "to_str_f64", wide.into())
            }
            _ => {
                let f = self.display_fn(ty)?;
                build_wrappers::w_call(&self.ir.builder, f, &[v.into()], "disp")
                    .try_as_basic_value()
                    .left()
            }
        }
    }

    /// The `__axon_display_N(T) -> str` helper for compound type `ty`, built
    /// once per module. Registered BEFORE its body is emitted, so a recursive
    /// type's body finds it and calls it rather than building another.
    fn display_fn(&mut self, ty: &Type) -> Option<FunctionValue<'ctx>> {
        if let Some(f) = self.display_fns.get(ty) {
            return Some(*f);
        }
        let param_ty = self.llvm_type(ty)?;
        let str_ty = self.llvm_type(&Type::Str)?;
        let name = format!("__axon_display_{}", self.display_fns.len());
        let f = self.ir.module.add_function(
            &name,
            str_ty.fn_type(&[param_ty.into()], false),
            Some(Linkage::Internal),
        );
        self.display_fns.insert(ty.clone(), f);
        let saved_bb = self.ir.builder.get_insert_block();
        let entry = self.ir.context.append_basic_block(f, "entry");
        self.ir.builder.position_at_end(entry);
        let body = self.emit_display_body(f, ty);
        if let Some(bb) = saved_bb {
            self.ir.builder.position_at_end(bb);
        }
        // `display_blocker` said every part renders, so a body cannot fail
        // short of an internal inconsistency. Say so rather than leave a
        // helper without a terminator for the verifier to report obliquely.
        if body.is_none() {
            self.record_error(format!(
                "codegen error [E0910]: internal: native codegen could not build the \
                 display helper for type `{}`. The interpreter renders it; run under \
                 `axon run`.",
                ty.display()
            ));
        }
        Some(f)
    }

    /// Emit the body of `f`, the display helper of `ty`; the builder stands at
    /// its empty entry block.
    fn emit_display_body(&mut self, f: FunctionValue<'ctx>, ty: &Type) -> Option<()> {
        let v = f.get_nth_param(0)?;
        match ty {
            Type::Struct(name) => {
                let names = self.struct_fields.get(name)?.clone();
                let tys = self.struct_field_sem_types.get(name)?.clone();
                let sv = v.into_struct_value();
                let mut acc = StrAcc::new(&format!("{name} {{ "));
                for (k, i) in display_order(&names).into_iter().enumerate() {
                    if k > 0 {
                        acc.lit(", ");
                    }
                    acc.lit(&format!("{}: ", names[i]));
                    let field =
                        build_wrappers::w_extract_value(&self.ir.builder, sv, i as u32, "fld");
                    let s = self.emit_value_display(field, &tys[i])?;
                    self.acc_push(&mut acc, s)?;
                }
                acc.lit(" }");
                self.ret_acc(acc)
            }
            Type::Tuple(tys) => {
                let sv = v.into_struct_value();
                let mut acc = StrAcc::new("(");
                for (i, t) in tys.iter().enumerate() {
                    if i > 0 {
                        acc.lit(", ");
                    }
                    let elem =
                        build_wrappers::w_extract_value(&self.ir.builder, sv, i as u32, "elt");
                    let s = self.emit_value_display(elem, t)?;
                    self.acc_push(&mut acc, s)?;
                }
                acc.lit(")");
                self.ret_acc(acc)
            }
            Type::Enum(name) => self.emit_enum_display_body(f, v.into_struct_value(), name),
            Type::Option(inner) => {
                let sv = v.into_struct_value();
                let (some_bb, none_bb) = self.branch_on_tag(f, sv, "some", "none");
                self.ir.builder.position_at_end(none_bb);
                self.ret_acc(StrAcc::new("None"))?;
                self.ir.builder.position_at_end(some_bb);
                let payload = build_wrappers::w_extract_value(&self.ir.builder, sv, 1, "somev");
                self.ret_wrapped("Some(", payload, inner)
            }
            Type::Result(ok, err) => {
                let sv = v.into_struct_value();
                // The payload bytes are read here, before the branch ends
                // this block; each arm reinterprets them at its own type.
                let bytes = build_wrappers::w_extract_value(&self.ir.builder, sv, 1, "pay");
                let (ok_bb, err_bb) = self.branch_on_tag(f, sv, "ok", "err");
                for (bb, label, t) in [(ok_bb, "Ok(", ok), (err_bb, "Err(", err)] {
                    self.ir.builder.position_at_end(bb);
                    let payload = self.extract_result_payload(bytes, t)?;
                    self.ret_wrapped(label, payload, t)?;
                }
                Some(())
            }
            Type::Slice(elem) => self.emit_slice_display_body(f, v.into_struct_value(), elem),
            _ => None,
        }
    }

    /// `Enum::Variant` or `Enum::Variant { a: 1 }`, by a switch on the tag.
    fn emit_enum_display_body(
        &mut self,
        f: FunctionValue<'ctx>,
        sv: StructValue<'ctx>,
        name: &str,
    ) -> Option<()> {
        let variants = self.enum_variants.get(name)?.clone();
        let ctx = self.ir.context;
        let tag = build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "tag").into_int_value();
        let dispatch = self.ir.builder.get_insert_block()?;
        // Every tag a value can hold names a variant; a value built by this
        // program never reaches the default.
        let bad = ctx.append_basic_block(f, "bad_tag");
        self.ir.builder.position_at_end(bad);
        build_wrappers::w_unreachable(&self.ir.builder);
        let mut cases = Vec::with_capacity(variants.len());
        for (vname, vtag, fields) in &variants {
            let bb = ctx.append_basic_block(f, &format!("v_{vname}"));
            cases.push((tag.get_type().const_int(*vtag as u64, false), bb));
            self.ir.builder.position_at_end(bb);
            let mut acc = StrAcc::new(&format!("{name}::{vname}"));
            if !fields.is_empty() {
                let payload = self.enum_payload_ptr(sv, name)?;
                acc.lit(" { ");
                let names: Vec<String> = fields.iter().map(|fl| fl.name.clone()).collect();
                for (k, i) in display_order(&names).into_iter().enumerate() {
                    if k > 0 {
                        acc.lit(", ");
                    }
                    acc.lit(&format!("{}: ", names[i]));
                    let value = self.load_enum_field(payload, &fields[i])?;
                    let s = self.emit_value_display(value, &fields[i].ty)?;
                    self.acc_push(&mut acc, s)?;
                }
                acc.lit(" }");
            }
            self.ret_acc(acc)?;
        }
        self.ir.builder.position_at_end(dispatch);
        build_wrappers::w_switch(&self.ir.builder, tag, bad, &cases);
        Some(())
    }

    /// `[a, b, c]`: a loop over the elements, with `, ` before every one but
    /// the first.
    fn emit_slice_display_body(
        &mut self,
        f: FunctionValue<'ctx>,
        sv: StructValue<'ctx>,
        elem: &Type,
    ) -> Option<()> {
        let ctx = self.ir.context;
        let i64_ty = ctx.i64_type();
        let str_ty = self.llvm_type(&Type::Str)?;
        let elem_ty = self.llvm_type(elem)?;
        let len = build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "len").into_int_value();
        let data =
            build_wrappers::w_extract_value(&self.ir.builder, sv, 1, "data").into_pointer_value();
        let acc_slot = build_wrappers::w_alloca(&self.ir.builder, str_ty, "acc");
        let i_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "i");
        let open = self.str_lit("[");
        build_wrappers::w_store(&self.ir.builder, acc_slot, open);
        build_wrappers::w_store(&self.ir.builder, i_slot, i64_ty.const_zero().into());

        let cond_bb = ctx.append_basic_block(f, "cond");
        let sep_bb = ctx.append_basic_block(f, "sep");
        let elem_bb = ctx.append_basic_block(f, "elem");
        let done_bb = ctx.append_basic_block(f, "done");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i =
            build_wrappers::w_load(&self.ir.builder, i64_ty.into(), i_slot, "iv").into_int_value();
        let more =
            build_wrappers::w_int_compare(&self.ir.builder, IntPredicate::SLT, i, len, "more");
        let first = build_wrappers::w_int_compare(
            &self.ir.builder,
            IntPredicate::EQ,
            i,
            i64_ty.const_zero(),
            "first",
        );
        let body_bb = ctx.append_basic_block(f, "body");
        build_wrappers::w_cond_br(&self.ir.builder, more, body_bb, done_bb);
        self.ir.builder.position_at_end(body_bb);
        build_wrappers::w_cond_br(&self.ir.builder, first, elem_bb, sep_bb);

        self.ir.builder.position_at_end(sep_bb);
        let sep = self.str_lit(", ");
        self.append_to_slot(acc_slot, sep)?;
        build_wrappers::w_br(&self.ir.builder, elem_bb);

        self.ir.builder.position_at_end(elem_bb);
        // SAFETY: `i < len`, and `data` holds `len` elements of `elem_ty`.
        let elem_ptr =
            unsafe { build_wrappers::w_gep(&self.ir.builder, elem_ty, data, &[i], "ep") };
        let value = build_wrappers::w_load(&self.ir.builder, elem_ty, elem_ptr, "ev");
        let s = self.emit_value_display(value, elem)?;
        self.append_to_slot(acc_slot, s)?;
        let next =
            build_wrappers::w_int_add(&self.ir.builder, i, i64_ty.const_int(1, false), "inext");
        build_wrappers::w_store(&self.ir.builder, i_slot, next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(done_bb);
        let close = self.str_lit("]");
        self.append_to_slot(acc_slot, close)?;
        let out = build_wrappers::w_load(&self.ir.builder, str_ty, acc_slot, "out");
        build_wrappers::w_ret(&self.ir.builder, out);
        Some(())
    }

    /// Split on the `i1` tag in field 0 of `sv` (1 = `Some`/`Ok`): returns the
    /// (set, clear) blocks, with the builder left in neither.
    fn branch_on_tag(
        &mut self,
        f: FunctionValue<'ctx>,
        sv: StructValue<'ctx>,
        set: &str,
        clear: &str,
    ) -> (
        inkwell::basic_block::BasicBlock<'ctx>,
        inkwell::basic_block::BasicBlock<'ctx>,
    ) {
        let tag = build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "tag").into_int_value();
        let set_bb = self.ir.context.append_basic_block(f, set);
        let clear_bb = self.ir.context.append_basic_block(f, clear);
        build_wrappers::w_cond_br(&self.ir.builder, tag, set_bb, clear_bb);
        (set_bb, clear_bb)
    }

    /// Return `label` + display(`payload`) + `)`.
    fn ret_wrapped(&mut self, label: &str, payload: BasicValueEnum<'ctx>, ty: &Type) -> Option<()> {
        let mut acc = StrAcc::new(label);
        let s = self.emit_value_display(payload, ty)?;
        self.acc_push(&mut acc, s)?;
        acc.lit(")");
        self.ret_acc(acc)
    }

    /// A `str` constant.
    fn str_lit(&self, s: &str) -> BasicValueEnum<'ctx> {
        self.emit_literal(&ast::Literal::Str(s.to_string()))
    }

    /// `axon_concat(a, b)`.
    fn concat(
        &self,
        a: BasicValueEnum<'ctx>,
        b: BasicValueEnum<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let f = self.functions.get("axon_concat").copied()?;
        build_wrappers::w_call(&self.ir.builder, f, &[a.into(), b.into()], "dcat")
            .try_as_basic_value()
            .left()
    }

    /// `*slot = *slot ++ s`.
    fn append_to_slot(
        &self,
        slot: inkwell::values::PointerValue<'ctx>,
        s: BasicValueEnum<'ctx>,
    ) -> Option<()> {
        let str_ty = s.get_type();
        let cur = build_wrappers::w_load(&self.ir.builder, str_ty, slot, "cur");
        let joined = self.concat(cur, s)?;
        build_wrappers::w_store(&self.ir.builder, slot, joined);
        Some(())
    }

    /// Flush `acc`'s pending literal text into its value.
    fn acc_flush(&self, acc: &mut StrAcc<'ctx>) -> Option<()> {
        if acc.pending.is_empty() && acc.acc.is_some() {
            return Some(());
        }
        let lit = self.str_lit(&std::mem::take(&mut acc.pending));
        acc.acc = Some(match acc.acc {
            Some(a) => self.concat(a, lit)?,
            None => lit,
        });
        Some(())
    }

    /// Append the rendered value `s` to `acc`.
    fn acc_push(&self, acc: &mut StrAcc<'ctx>, s: BasicValueEnum<'ctx>) -> Option<()> {
        self.acc_flush(acc)?;
        acc.acc = Some(self.concat(acc.acc?, s)?);
        Some(())
    }

    /// Return `acc`'s text from the current block.
    fn ret_acc(&self, mut acc: StrAcc<'ctx>) -> Option<()> {
        self.acc_flush(&mut acc)?;
        let out = acc.acc?;
        build_wrappers::w_ret(&self.ir.builder, out);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::display_order;

    #[test]
    fn field_order_is_the_interpreters_sort_of_name_colon() {
        // The interpreter sorts rendered `name: value` strings. `1` (0x31) is
        // below `:` (0x3a), which is below every letter, so `a1` precedes `a`
        // and `a` precedes `ab`.
        let names: Vec<String> = ["y", "ab", "a", "a1", "B"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let got: Vec<&str> = display_order(&names)
            .into_iter()
            .map(|i| names[i].as_str())
            .collect();
        assert_eq!(got, ["B", "a1", "a", "ab", "y"]);
    }
}
