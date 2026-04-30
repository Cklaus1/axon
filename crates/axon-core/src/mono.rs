//! Phase 3 — Monomorphization pass.
//!
//! Collects all instantiation sites of generic functions and types, then
//! produces concrete (non-generic) copies of their definitions with type
//! parameters substituted.  The resulting `MonoProgram` is handed to codegen
//! which emits LLVM IR only for the concrete instances actually used.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::ast::{
    AxonType, EnumDef, Expr, FnDef, Item, LambdaParam,
    MatchArm, Param, Program, SelectArm, Stmt, TypeDef,
};

// ── Type substitution ─────────────────────────────────────────────────────────

/// A mapping from type parameter names to their concrete argument types.
pub type TypeSubst = HashMap<String, AxonType>;

fn subst_type(ty: &AxonType, subst: &TypeSubst) -> AxonType {
    match ty {
        AxonType::Named(n) => {
            if let Some(concrete) = subst.get(n) {
                concrete.clone()
            } else {
                AxonType::Named(n.clone())
            }
        }
        AxonType::TypeParam(n) => {
            subst.get(n).cloned().unwrap_or_else(|| AxonType::TypeParam(n.clone()))
        }
        AxonType::Generic { base, args } => {
            let new_args: Vec<_> = args.iter().map(|a| subst_type(a, subst)).collect();
            AxonType::Generic { base: base.clone(), args: new_args }
        }
        AxonType::Result { ok, err } =>
            AxonType::Result { ok: Box::new(subst_type(ok, subst)), err: Box::new(subst_type(err, subst)) },
        AxonType::Option(inner) => AxonType::Option(Box::new(subst_type(inner, subst))),
        AxonType::Chan(inner)   => AxonType::Chan(Box::new(subst_type(inner, subst))),
        AxonType::Slice(inner)  => AxonType::Slice(Box::new(subst_type(inner, subst))),
        AxonType::Fn { params, ret } => AxonType::Fn {
            params: params.iter().map(|p| subst_type(p, subst)).collect(),
            ret: Box::new(subst_type(ret, subst)),
        },
        AxonType::Ref(inner)      => AxonType::Ref(Box::new(subst_type(inner, subst))),
        AxonType::DynTrait(name)  => AxonType::DynTrait(name.clone()),
    }
}

fn subst_expr(expr: &Expr, subst: &TypeSubst) -> Expr {
    match expr {
        Expr::Block(stmts) =>
            Expr::Block(stmts.iter().map(|s| Stmt { expr: subst_expr(&s.expr, subst), span: s.span }).collect()),

        Expr::Let { name, value } =>
            Expr::Let { name: name.clone(), value: Box::new(subst_expr(value, subst)) },
        Expr::Own { name, value } =>
            Expr::Own { name: name.clone(), value: Box::new(subst_expr(value, subst)) },
        Expr::RefBind { name, value } =>
            Expr::RefBind { name: name.clone(), value: Box::new(subst_expr(value, subst)) },
        Expr::Assign { name, value } =>
            Expr::Assign { name: name.clone(), value: Box::new(subst_expr(value, subst)) },

        Expr::Call { callee, args } => Expr::Call {
            callee: Box::new(subst_expr(callee, subst)),
            args: args.iter().map(|a| subst_expr(a, subst)).collect(),
        },
        Expr::MethodCall { receiver, method, args } => Expr::MethodCall {
            receiver: Box::new(subst_expr(receiver, subst)),
            method: method.clone(),
            args: args.iter().map(|a| subst_expr(a, subst)).collect(),
        },

        Expr::BinOp { op, left, right } => Expr::BinOp {
            op: op.clone(),
            left: Box::new(subst_expr(left, subst)),
            right: Box::new(subst_expr(right, subst)),
        },
        Expr::UnaryOp { op, operand } => Expr::UnaryOp {
            op: op.clone(),
            operand: Box::new(subst_expr(operand, subst)),
        },
        Expr::Question(inner) => Expr::Question(Box::new(subst_expr(inner, subst))),

        Expr::If { cond, then, else_ } => Expr::If {
            cond: Box::new(subst_expr(cond, subst)),
            then: Box::new(subst_expr(then, subst)),
            else_: else_.as_ref().map(|e| Box::new(subst_expr(e, subst))),
        },

        Expr::Match { subject, arms } => Expr::Match {
            subject: Box::new(subst_expr(subject, subst)),
            arms: arms.iter().map(|arm| MatchArm {
                pattern: arm.pattern.clone(),
                guard: arm.guard.as_ref().map(|g| subst_expr(g, subst)),
                body: subst_expr(&arm.body, subst),
            }).collect(),
        },

        Expr::Return(v) => Expr::Return(v.as_ref().map(|e| Box::new(subst_expr(e, subst)))),

        Expr::FieldAccess { receiver, field } => Expr::FieldAccess {
            receiver: Box::new(subst_expr(receiver, subst)),
            field: field.clone(),
        },
        Expr::Index { receiver, index } => Expr::Index {
            receiver: Box::new(subst_expr(receiver, subst)),
            index: Box::new(subst_expr(index, subst)),
        },

        Expr::Ok(v)   => Expr::Ok(Box::new(subst_expr(v, subst))),
        Expr::Err(v)  => Expr::Err(Box::new(subst_expr(v, subst))),
        Expr::Some(v) => Expr::Some(Box::new(subst_expr(v, subst))),
        Expr::None    => Expr::None,
        Expr::Break   => Expr::Break,
        Expr::Continue => Expr::Continue,
        Expr::For { var, start, end, body } => Expr::For {
            var: var.clone(),
            start: Box::new(subst_expr(start, subst)),
            end: Box::new(subst_expr(end, subst)),
            body: body.iter().map(|s| Stmt { expr: subst_expr(&s.expr, subst), span: s.span }).collect(),
        },
        Expr::Literal(_) | Expr::Ident(_) => expr.clone(),

        Expr::FmtStr { parts } => {
            use crate::ast::FmtPart;
            Expr::FmtStr {
                parts: parts.iter().map(|p| match p {
                    FmtPart::Lit(s)  => FmtPart::Lit(s.clone()),
                    FmtPart::Expr(e) => FmtPart::Expr(Box::new(subst_expr(e, subst))),
                }).collect(),
            }
        }

        Expr::StructLit { name, fields } => Expr::StructLit {
            name: name.clone(),
            fields: fields.iter().map(|(f, v)| (f.clone(), subst_expr(v, subst))).collect(),
        },

        Expr::Array(elems) => Expr::Array(elems.iter().map(|e| subst_expr(e, subst)).collect()),

        Expr::While { cond, body } => Expr::While {
            cond: Box::new(subst_expr(cond, subst)),
            body: body.iter().map(|s| Stmt { expr: subst_expr(&s.expr, subst), span: s.span }).collect(),
        },

        Expr::Lambda { params, body, captures } => Expr::Lambda {
            params: params.iter().map(|p| LambdaParam {
                name: p.name.clone(),
                ty: p.ty.as_ref().map(|t| subst_type(t, subst)),
            }).collect(),
            body: Box::new(subst_expr(body, subst)),
            captures: captures.clone(),
        },

        Expr::Spawn(body) => Expr::Spawn(Box::new(subst_expr(body, subst))),
        Expr::Select(arms) => Expr::Select(arms.iter().map(|a| SelectArm {
            recv: subst_expr(&a.recv, subst),
            body: subst_expr(&a.body, subst),
        }).collect()),
        Expr::Comptime(inner) => Expr::Comptime(Box::new(subst_expr(inner, subst))),
    }
}

fn subst_fn(fndef: &FnDef, subst: &TypeSubst) -> FnDef {
    FnDef {
        public: fndef.public,
        name: fndef.name.clone(),
        generic_params: vec![], // concrete instance has no type params
        generic_bounds: vec![], // bounds are resolved during monomorphisation
        params: fndef.params.iter().map(|p| Param {
            name: p.name.clone(),
            ty: subst_type(&p.ty, subst),
            span: p.span,
        }).collect(),
        return_type: fndef.return_type.as_ref().map(|t| subst_type(t, subst)),
        body: subst_expr(&fndef.body, subst),
        attrs: fndef.attrs.clone(),
        span: fndef.span,
    }
}

// ── Name mangling ─────────────────────────────────────────────────────────────

/// Mangle a type argument for use in a symbol name.
pub fn mangle_type(ty: &AxonType) -> String {
    match ty {
        AxonType::Named(n)      => n.clone(),
        AxonType::TypeParam(n)  => n.clone(),
        AxonType::Generic { base, args } => {
            let mangled: Vec<_> = args.iter().map(mangle_type).collect();
            format!("{}__{}", base, mangled.join("__"))
        }
        AxonType::Result { ok, err } =>
            format!("Result__{}__{}", mangle_type(ok), mangle_type(err)),
        AxonType::Option(t)     => format!("Option__{}", mangle_type(t)),
        AxonType::Chan(t)       => format!("Chan__{}", mangle_type(t)),
        AxonType::Slice(t)      => format!("Slice__{}", mangle_type(t)),
        AxonType::Fn { params, ret } => {
            let ps: Vec<_> = params.iter().map(mangle_type).collect();
            format!("Fn__{}__{}", ps.join("__"), mangle_type(ret))
        }
        AxonType::Ref(t)        => format!("Ref__{}", mangle_type(t)),
        AxonType::DynTrait(n)   => format!("dyn__{}", n),
    }
}

pub fn mangle_fn(base: &str, args: &[AxonType]) -> String {
    if args.is_empty() {
        return base.to_string();
    }
    let mangled: Vec<_> = args.iter().map(mangle_type).collect();
    format!("{}__{}", base, mangled.join("__"))
}

// ── Mono pass ────────────────────────────────────────────────────────────────

/// The output of monomorphization — a flat list of concrete function
/// definitions (no type parameters) that can be handed directly to codegen.
#[derive(Debug, Default)]
pub struct MonoProgram {
    /// Concrete function instances.  Includes both non-generic functions
    /// (emitted as-is) and monomorphised copies of generic ones.
    pub fns: Vec<FnDef>,
    /// Non-generic type / enum / trait / impl items passed through unchanged.
    pub other_items: Vec<Item>,
}

#[allow(dead_code)]
pub struct MonoContext {
    /// All generic function definitions (name → def).
    generic_fns: HashMap<String, FnDef>,
    /// All generic struct definitions.
    generic_structs: HashMap<String, TypeDef>,
    /// All generic enum definitions.
    generic_enums: HashMap<String, EnumDef>,
    /// Work queue: (base_name, type_args, substituted_def).
    queue: VecDeque<(String, Vec<AxonType>, FnDef)>,
    /// Already-emitted instances (base + mangled suffix).
    seen: HashSet<String>,
    output: MonoProgram,
}

impl MonoContext {
    pub fn new(program: &Program) -> Self {
        let mut generic_fns = HashMap::new();
        let mut generic_structs = HashMap::new();
        let mut generic_enums = HashMap::new();
        let mut output = MonoProgram::default();

        for item in &program.items {
            match item {
                Item::FnDef(f) if !f.generic_params.is_empty() => {
                    generic_fns.insert(f.name.clone(), f.clone());
                }
                Item::FnDef(f) => {
                    output.fns.push(f.clone());
                }
                Item::TypeDef(t) if !t.generic_params.is_empty() => {
                    generic_structs.insert(t.name.clone(), t.clone());
                }
                Item::EnumDef(e) if !e.generic_params.is_empty() => {
                    generic_enums.insert(e.name.clone(), e.clone());
                }
                other => {
                    output.other_items.push(other.clone());
                }
            }
        }

        MonoContext {
            generic_fns,
            generic_structs,
            generic_enums,
            queue: VecDeque::new(),
            seen: HashSet::new(),
            output,
        }
    }

    /// Request a monomorphisation of `fn_name` with `type_args`.
    /// Returns the mangled name that will be used.
    pub fn request_fn(&mut self, fn_name: &str, type_args: Vec<AxonType>) -> String {
        let mangled = mangle_fn(fn_name, &type_args);
        if !self.seen.contains(&mangled) {
            if let Some(template) = self.generic_fns.get(fn_name).cloned() {
                let subst: TypeSubst = template.generic_params.iter()
                    .cloned()
                    .zip(type_args.iter().cloned())
                    .collect();
                let mut concrete = subst_fn(&template, &subst);
                concrete.name = mangled.clone();
                self.seen.insert(mangled.clone());
                self.queue.push_back((fn_name.to_string(), type_args, concrete));
            }
        }
        mangled
    }

    /// Drain the work queue, collecting all concrete instances.
    pub fn run(mut self) -> MonoProgram {
        while let Some((_, _, concrete)) = self.queue.pop_front() {
            // Walk the body to find any nested generic calls.
            self.collect_from_expr(&concrete.body.clone());
            self.output.fns.push(concrete);
        }
        self.output
    }

    fn collect_from_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Block(stmts) => {
                for s in stmts { self.collect_from_expr(&s.expr); }
            }
            Expr::Call { callee, args } => {
                self.collect_from_expr(callee);
                for a in args { self.collect_from_expr(a); }
            }
            Expr::Let { value, .. } | Expr::Own { value, .. } | Expr::RefBind { value, .. } => {
                self.collect_from_expr(value);
            }
            Expr::BinOp { left, right, .. } => {
                self.collect_from_expr(left);
                self.collect_from_expr(right);
            }
            Expr::UnaryOp { operand, .. } | Expr::Question(operand) | Expr::Comptime(operand) => {
                self.collect_from_expr(operand);
            }
            Expr::If { cond, then, else_ } => {
                self.collect_from_expr(cond);
                self.collect_from_expr(then);
                if let Some(e) = else_ { self.collect_from_expr(e); }
            }
            Expr::Return(Some(v)) => self.collect_from_expr(v),
            Expr::FieldAccess { receiver, .. } => self.collect_from_expr(receiver),
            Expr::Index { receiver, index } => {
                self.collect_from_expr(receiver);
                self.collect_from_expr(index);
            }
            Expr::Ok(v) | Expr::Err(v) | Expr::Some(v) | Expr::Spawn(v) => {
                self.collect_from_expr(v);
            }
            Expr::Array(elems) => { for e in elems { self.collect_from_expr(e); } }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields { self.collect_from_expr(v); }
            }
            Expr::Lambda { body, .. } => self.collect_from_expr(body),
            Expr::Match { subject, arms } => {
                self.collect_from_expr(subject);
                for arm in arms { self.collect_from_expr(&arm.body); }
            }
            Expr::While { cond, body } => {
                self.collect_from_expr(cond);
                for s in body { self.collect_from_expr(&s.expr); }
            }
            Expr::Assign { value, .. } => self.collect_from_expr(value),
            Expr::MethodCall { receiver, args, .. } => {
                self.collect_from_expr(receiver);
                for a in args { self.collect_from_expr(a); }
            }
            Expr::FmtStr { parts } => {
                use crate::ast::FmtPart;
                for p in parts {
                    if let FmtPart::Expr(e) = p { self.collect_from_expr(e); }
                }
            }
            Expr::Select(arms) => {
                for arm in arms {
                    self.collect_from_expr(&arm.recv);
                    self.collect_from_expr(&arm.body);
                }
            }
            Expr::For { start, end, body, .. } => {
                self.collect_from_expr(start);
                self.collect_from_expr(end);
                for s in body { self.collect_from_expr(&s.expr); }
            }
            Expr::Literal(_) | Expr::Ident(_) | Expr::None | Expr::Return(None)
            | Expr::Break | Expr::Continue => {}
        }
    }
}

/// Top-level entry point: monomorphise `program` and return a `MonoProgram`.
///
/// `instantiations` is a list of `(fn_name, type_args)` produced by the
/// inference pass; each entry represents one required concrete instance.
pub fn monomorphise(program: &Program, instantiations: Vec<(String, Vec<AxonType>)>) -> MonoProgram {
    let mut ctx = MonoContext::new(program);

    // Build a rename map: generic function name → list of mangled concrete names.
    let mut rename: HashMap<String, Vec<String>> = HashMap::new();
    for (name, type_args) in &instantiations {
        let mangled = ctx.request_fn(name, type_args.clone());
        rename.entry(name.clone()).or_default().push(mangled);
    }

    // Build a single-instantiation rename map: name → unique mangled name.
    // (If a generic function has multiple distinct instantiations, call-site
    // renaming for it isn't yet supported — leave as-is and let codegen warn.)
    let single_rename: HashMap<String, String> = rename.into_iter()
        .filter_map(|(k, mut v)| {
            v.sort();
            v.dedup();
            if v.len() == 1 { Some((k, v.into_iter().next().unwrap())) }
            else { None }
        })
        .collect();

    let mut prog = ctx.run();

    // Rename call sites in non-generic functions.
    for fndef in &mut prog.fns {
        fndef.body = rename_calls_expr(&fndef.body, &single_rename);
    }
    for item in &mut prog.other_items {
        if let Item::FnDef(f) = item {
            f.body = rename_calls_expr(&f.body, &single_rename);
        }
    }

    prog
}

/// Recursively rename calls to generic functions to their mangled concrete names.
fn rename_calls_expr(expr: &Expr, rename: &HashMap<String, String>) -> Expr {
    match expr {
        Expr::Call { callee, args } => {
            let new_callee = if let Expr::Ident(name) = callee.as_ref() {
                if let Some(mangled) = rename.get(name) {
                    Box::new(Expr::Ident(mangled.clone()))
                } else {
                    Box::new(rename_calls_expr(callee, rename))
                }
            } else {
                Box::new(rename_calls_expr(callee, rename))
            };
            Expr::Call {
                callee: new_callee,
                args: args.iter().map(|a| rename_calls_expr(a, rename)).collect(),
            }
        }
        Expr::Block(stmts) => Expr::Block(
            stmts.iter().map(|s| Stmt { expr: rename_calls_expr(&s.expr, rename), span: s.span }).collect()
        ),
        Expr::Let { name, value } => Expr::Let { name: name.clone(), value: Box::new(rename_calls_expr(value, rename)) },
        Expr::Own { name, value } => Expr::Own { name: name.clone(), value: Box::new(rename_calls_expr(value, rename)) },
        Expr::RefBind { name, value } => Expr::RefBind { name: name.clone(), value: Box::new(rename_calls_expr(value, rename)) },
        Expr::Assign { name, value } => Expr::Assign { name: name.clone(), value: Box::new(rename_calls_expr(value, rename)) },
        Expr::BinOp { op, left, right } => Expr::BinOp {
            op: op.clone(),
            left: Box::new(rename_calls_expr(left, rename)),
            right: Box::new(rename_calls_expr(right, rename)),
        },
        Expr::UnaryOp { op, operand } => Expr::UnaryOp { op: op.clone(), operand: Box::new(rename_calls_expr(operand, rename)) },
        Expr::Question(inner) => Expr::Question(Box::new(rename_calls_expr(inner, rename))),
        Expr::Comptime(inner) => Expr::Comptime(Box::new(rename_calls_expr(inner, rename))),
        Expr::If { cond, then, else_ } => Expr::If {
            cond: Box::new(rename_calls_expr(cond, rename)),
            then: Box::new(rename_calls_expr(then, rename)),
            else_: else_.as_ref().map(|e| Box::new(rename_calls_expr(e, rename))),
        },
        Expr::Return(Some(v)) => Expr::Return(Some(Box::new(rename_calls_expr(v, rename)))),
        Expr::MethodCall { receiver, method, args } => Expr::MethodCall {
            receiver: Box::new(rename_calls_expr(receiver, rename)),
            method: method.clone(),
            args: args.iter().map(|a| rename_calls_expr(a, rename)).collect(),
        },
        Expr::FieldAccess { receiver, field } => Expr::FieldAccess { receiver: Box::new(rename_calls_expr(receiver, rename)), field: field.clone() },
        Expr::Index { receiver, index } => Expr::Index {
            receiver: Box::new(rename_calls_expr(receiver, rename)),
            index: Box::new(rename_calls_expr(index, rename)),
        },
        Expr::Ok(v) => Expr::Ok(Box::new(rename_calls_expr(v, rename))),
        Expr::Err(v) => Expr::Err(Box::new(rename_calls_expr(v, rename))),
        Expr::Some(v) => Expr::Some(Box::new(rename_calls_expr(v, rename))),
        Expr::Spawn(v) => Expr::Spawn(Box::new(rename_calls_expr(v, rename))),
        Expr::Array(elems) => Expr::Array(elems.iter().map(|e| rename_calls_expr(e, rename)).collect()),
        Expr::StructLit { name, fields } => Expr::StructLit {
            name: name.clone(),
            fields: fields.iter().map(|(f, v)| (f.clone(), rename_calls_expr(v, rename))).collect(),
        },
        Expr::While { cond, body } => Expr::While {
            cond: Box::new(rename_calls_expr(cond, rename)),
            body: body.iter().map(|s| Stmt { expr: rename_calls_expr(&s.expr, rename), span: s.span }).collect(),
        },
        Expr::Lambda { params, body, captures } => Expr::Lambda {
            params: params.clone(),
            body: Box::new(rename_calls_expr(body, rename)),
            captures: captures.clone(),
        },
        Expr::Match { subject, arms } => Expr::Match {
            subject: Box::new(rename_calls_expr(subject, rename)),
            arms: arms.iter().map(|arm| crate::ast::MatchArm {
                pattern: arm.pattern.clone(),
                guard: arm.guard.as_ref().map(|g| rename_calls_expr(g, rename)),
                body: rename_calls_expr(&arm.body, rename),
            }).collect(),
        },
        Expr::Select(arms) => Expr::Select(arms.iter().map(|arm| crate::ast::SelectArm {
            recv: rename_calls_expr(&arm.recv, rename),
            body: rename_calls_expr(&arm.body, rename),
        }).collect()),
        Expr::FmtStr { parts } => Expr::FmtStr {
            parts: parts.iter().map(|p| match p {
                crate::ast::FmtPart::Lit(s) => crate::ast::FmtPart::Lit(s.clone()),
                crate::ast::FmtPart::Expr(e) => crate::ast::FmtPart::Expr(Box::new(rename_calls_expr(e, rename))),
            }).collect(),
        },
        Expr::For { var, start, end, body } => Expr::For {
            var: var.clone(),
            start: Box::new(rename_calls_expr(start, rename)),
            end: Box::new(rename_calls_expr(end, rename)),
            body: body.iter().map(|s| Stmt {
                expr: rename_calls_expr(&s.expr, rename),
                span: s.span,
            }).collect(),
        },
        // Leaves — clone unchanged.
        Expr::Ident(_) | Expr::Literal(_) | Expr::None | Expr::Return(None)
        | Expr::Break | Expr::Continue => expr.clone(),
    }
}
