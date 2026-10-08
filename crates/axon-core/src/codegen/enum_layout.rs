//! Native layout of user enums, and the by-value type graph it needs.
//!
//! An enum value is the named LLVM struct `{Name}_enum` = `{ i32 tag, [N x i8]
//! payload }`. Each variant's fields are packed into the payload in
//! DECLARATION order at byte offsets fixed here, once, and recorded in
//! `enum_variants`. Construction (`emit_struct_lit`) and match bindings
//! (`match_pat.rs`) both look a field up there by NAME, so neither depends on
//! the order a literal or a pattern happens to write its fields in.
//!
//! Layout is computed before any struct body is set, in dependency order: an
//! enum's payload size depends on the sizes of the structs and enums it holds
//! by value, and a struct's LLVM body may hold an enum (or a `Result` sized by
//! one). Computing either on the fly in source order sized a not-yet-declared
//! enum as a struct with no layout.

use std::collections::{HashMap, HashSet};

use crate::ast;
use crate::types::Type;

/// One payload field of an enum variant, as native codegen lays it out.
#[derive(Debug, Clone)]
pub(super) struct EnumField {
    /// Declared field name.
    pub(super) name: String,
    /// Semantic type of the field's value.
    pub(super) ty: Type,
    /// Byte offset of the field's slot inside the payload array.
    pub(super) offset: u64,
}

/// `(variant name, tag, fields in declaration order)`.
pub(super) type EnumVariantLayout = (String, usize, Vec<EnumField>);

/// An enum's variants with their payload fields' semantic types, in
/// declaration order: `(variant, [(field, type)])`.
type SemVariants = Vec<(String, Vec<(String, Type)>)>;

/// Push the nominal types (`Struct`/`Enum`) a value of `ty` holds BY VALUE at
/// its top level: through `Option`/`Result`/tuple/`Uncertain`/`Temporal`,
/// which embed their payload inline, but not through an array, `str`, channel,
/// closure, dict or raw pointer, which hold it behind a pointer.
fn by_value_nominals(ty: &Type, out: &mut Vec<Type>) {
    match ty {
        Type::Struct(_) | Type::Enum(_) => out.push(ty.clone()),
        Type::Option(inner) | Type::Uncertain(inner) | Type::Temporal(inner) => {
            by_value_nominals(inner, out)
        }
        Type::Result(ok, err) => {
            by_value_nominals(ok, out);
            by_value_nominals(err, out);
        }
        Type::Tuple(elems) => {
            for e in elems {
                by_value_nominals(e, out);
            }
        }
        _ => {}
    }
}

/// The "holds by value" relation over every struct and enum of the program.
struct ByValueGraph<'a> {
    structs: &'a HashMap<String, Vec<Type>>,
    /// enum name → every payload field type of every variant.
    enums: &'a HashMap<String, Vec<Type>>,
}

impl ByValueGraph<'_> {
    fn successors(&self, node: &Type, out: &mut Vec<Type>) {
        let fields = match node {
            Type::Struct(n) => self.structs.get(n),
            Type::Enum(n) => self.enums.get(n),
            _ => None,
        };
        for f in fields.into_iter().flatten() {
            by_value_nominals(f, out);
        }
    }

    /// Does a value of type `from` hold a `target` by value, at any depth?
    fn reaches(&self, from: &Type, target: &Type) -> bool {
        let mut stack = Vec::new();
        by_value_nominals(from, &mut stack);
        let mut seen: HashSet<Type> = HashSet::new();
        while let Some(node) = stack.pop() {
            if node == *target {
                return true;
            }
            if seen.insert(node.clone()) {
                self.successors(&node, &mut stack);
            }
        }
        false
    }
}

impl<'ctx> super::Codegen<'ctx> {
    /// Register every enum name, with its `{Name}_enum` LLVM type created
    /// OPAQUE, before any struct or enum body is laid out.
    ///
    /// `axon_type_to_semantic` decides enum-vs-struct by this set. It used to
    /// ask `enum_variants`, which was filled only AFTER every struct body was
    /// set, so a struct field of enum type (`type R = { a: A, k: i64 }`)
    /// resolved to a struct named `A` with no layout and the whole struct was
    /// refused (AX-43); an enum field naming an enum declared later in the
    /// file was mis-typed the same way.
    pub(super) fn declare_enum_names(&mut self, program: &ast::Program) {
        for item in &program.items {
            if let ast::Item::EnumDef(ed) = item {
                self.enum_names.insert(ed.name.clone());
                let mangled = format!("{}_enum", ed.name);
                if self.ir.module.get_struct_type(&mangled).is_none() {
                    self.ir.context.opaque_struct_type(&mangled);
                }
            }
        }
    }

    /// Lay out every enum and set its `{ i32 tag, [N x i8] payload }` body.
    ///
    /// Needs `struct_field_sem_types` for every struct (sizes) and refuses,
    /// with E0910, a nominal type that holds itself by value: it has no finite
    /// layout. Returns the names of the structs refused that way so the
    /// caller does not give them a body.
    pub(super) fn declare_enum_types(&mut self, program: &ast::Program) -> HashSet<String> {
        // Semantic field types of every variant, in declaration order.
        let mut defs: Vec<(String, SemVariants)> = Vec::new();
        for item in &program.items {
            if let ast::Item::EnumDef(ed) = item {
                let variants = ed
                    .variants
                    .iter()
                    .map(|v| {
                        let fields = v
                            .fields
                            .iter()
                            .map(|f| (f.name.clone(), self.axon_type_to_semantic(&f.ty)))
                            .collect();
                        (v.name.clone(), fields)
                    })
                    .collect();
                defs.push((ed.name.clone(), variants));
            }
        }
        let enum_field_tys: HashMap<String, Vec<Type>> = defs
            .iter()
            .map(|(name, variants)| {
                let tys = variants
                    .iter()
                    .flat_map(|(_, fs)| fs.iter().map(|(_, t)| t.clone()))
                    .collect();
                (name.clone(), tys)
            })
            .collect();

        let mut refusals: Vec<String> = Vec::new();
        let mut cyclic_structs: HashSet<String> = HashSet::new();
        {
            let graph = ByValueGraph {
                structs: &self.struct_field_sem_types,
                enums: &enum_field_tys,
            };
            // A field holding its own enum by value makes the enum infinitely
            // large.
            for (ename, variants) in &defs {
                let me = Type::Enum(ename.clone());
                for (vname, fields) in variants {
                    if let Some((fname, _)) = fields.iter().find(|(_, t)| graph.reaches(t, &me)) {
                        refusals.push(format!(
                            "codegen error [E0910]: enum `{ename}` holds itself by value \
                             (variant `{vname}`, field `{fname}`), which native codegen has no \
                             layout for. The interpreter supports it; run under `axon run`."
                        ));
                    }
                }
            }
            // The same for a struct (`type Node = { next: Option<Node> }`). This
            // used to recurse in `llvm_sizeof` until the compiler's own stack
            // overflowed.
            for item in &program.items {
                if let ast::Item::TypeDef(td) = item {
                    let me = Type::Struct(td.name.clone());
                    let Some(ftys) = self.struct_field_sem_types.get(&td.name) else {
                        continue;
                    };
                    if let Some((f, _)) = td
                        .fields
                        .iter()
                        .zip(ftys)
                        .find(|(_, t)| graph.reaches(t, &me))
                    {
                        refusals.push(format!(
                            "codegen error [E0910]: struct `{}` holds itself by value (field \
                             `{}` of type {}), so it has no finite native layout. The \
                             interpreter supports it; run under `axon run`.",
                            td.name,
                            f.name,
                            crate::doc::render_type(&f.ty)
                        ));
                        cyclic_structs.insert(td.name.clone());
                    }
                }
            }
        }
        for msg in refusals {
            if !self.codegen_errors.iter().any(|e| e == &msg) {
                eprintln!("{msg}");
                self.codegen_errors.push(msg);
            }
        }
        // A refused struct is sized as unknown from here on, so no size
        // computation can follow its cycle.
        for s in &cyclic_structs {
            self.struct_field_sem_types.remove(s);
        }

        let defs: HashMap<String, SemVariants> = defs.into_iter().collect();
        let mut in_progress: HashSet<String> = HashSet::new();
        let mut names: Vec<&String> = defs.keys().collect();
        names.sort();
        for name in names {
            self.lay_out_enum(name, &defs, &mut in_progress);
        }
        cyclic_structs
    }

    /// Compute `name`'s payload offsets after those of every enum it holds by
    /// value, record them in `enum_variants`, and set the LLVM body.
    fn lay_out_enum(
        &mut self,
        name: &str,
        defs: &HashMap<String, SemVariants>,
        in_progress: &mut HashSet<String>,
    ) {
        if self.enum_variants.contains_key(name) || !in_progress.insert(name.to_string()) {
            // Done, or a by-value cycle already refused above: the inner
            // reference sizes as unknown (`UNKNOWN_TYPE_PAYLOAD_SIZE`).
            return;
        }
        let Some(variants) = defs.get(name) else {
            return;
        };
        // Enums held by value, directly or through structs.
        let mut deps: Vec<String> = Vec::new();
        let mut stack: Vec<Type> = Vec::new();
        for (_, fields) in variants {
            for (_, t) in fields {
                by_value_nominals(t, &mut stack);
            }
        }
        let mut seen: HashSet<Type> = HashSet::new();
        while let Some(node) = stack.pop() {
            if !seen.insert(node.clone()) {
                continue;
            }
            match &node {
                Type::Enum(e) => deps.push(e.clone()),
                Type::Struct(s) => {
                    for f in self.struct_field_sem_types.get(s).into_iter().flatten() {
                        by_value_nominals(f, &mut stack);
                    }
                }
                _ => {}
            }
        }
        for dep in deps {
            if dep != name {
                self.lay_out_enum(&dep, defs, in_progress);
            }
        }

        let mut layout: Vec<EnumVariantLayout> = Vec::with_capacity(variants.len());
        let mut max_size: u64 = 0;
        for (tag, (vname, fields)) in variants.iter().enumerate() {
            let mut offset: u64 = 0;
            let mut slots = Vec::with_capacity(fields.len());
            for (fname, ty) in fields {
                slots.push(EnumField {
                    name: fname.clone(),
                    ty: ty.clone(),
                    offset,
                });
                offset += self.llvm_sizeof(ty).unwrap_or(8);
            }
            max_size = max_size.max(offset);
            layout.push((vname.clone(), tag, slots));
        }
        // At least 1 byte of payload so LLVM doesn't complain.
        let payload_size = max_size.max(1) as u32;
        let i32_ty = self.ir.context.i32_type();
        let i8_ty = self.ir.context.i8_type();
        let mangled = format!("{name}_enum");
        let named = match self.ir.module.get_struct_type(&mangled) {
            Some(t) => t,
            None => self.ir.context.opaque_struct_type(&mangled),
        };
        named.set_body(
            &[i32_ty.into(), i8_ty.array_type(payload_size).into()],
            false,
        );
        self.enum_variants.insert(name.to_string(), layout);
    }

    /// Load payload field `field` of an enum value whose payload starts at
    /// `payload` (an `i8*` to field 1 of `{Name}_enum`).
    pub(super) fn load_enum_field(
        &self,
        payload: inkwell::values::PointerValue<'ctx>,
        field: &EnumField,
    ) -> Option<inkwell::values::BasicValueEnum<'ctx>> {
        let llvm_ty = self.llvm_type(&field.ty)?;
        let slot = self.enum_field_slot(payload, field);
        Some(super::build_wrappers::w_load(
            &self.ir.builder,
            llvm_ty,
            slot,
            &field.name,
        ))
    }

    /// Address of `field`'s slot inside the payload at `payload`.
    pub(super) fn enum_field_slot(
        &self,
        payload: inkwell::values::PointerValue<'ctx>,
        field: &EnumField,
    ) -> inkwell::values::PointerValue<'ctx> {
        let i8_ty = self.ir.context.i8_type();
        let off = self.ir.context.i32_type().const_int(field.offset, false);
        // SAFETY: `offset` lies inside the payload array, which
        // `lay_out_enum` sized to hold every variant's fields.
        unsafe {
            self.ir
                .builder
                .build_gep(i8_ty, payload, &[off], "enum_fslot")
                .unwrap()
        }
    }

    /// Spill enum value `sv` to a stack slot; returns the address of its
    /// payload (field 1), or `None` when `enum_name` has no LLVM type.
    pub(super) fn enum_payload_ptr(
        &self,
        sv: inkwell::values::StructValue<'ctx>,
        enum_name: &str,
    ) -> Option<inkwell::values::PointerValue<'ctx>> {
        let ty = self
            .ir
            .module
            .get_struct_type(&format!("{enum_name}_enum"))?;
        let slot = super::build_wrappers::w_alloca(&self.ir.builder, ty.into(), "enumtmp");
        super::build_wrappers::w_store(&self.ir.builder, slot, sv.into());
        Some(super::build_wrappers::w_struct_gep(
            &self.ir.builder,
            ty.into(),
            slot,
            1,
            "pay",
        ))
    }
}
