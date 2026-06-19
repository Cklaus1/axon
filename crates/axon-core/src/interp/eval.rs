//! The core tree-walking evaluator for the interpreter (R0 slice 5 —
//! extracted verbatim from `interp.rs`, zero behavior change). `eval`
//! (the 33-arm expression dispatch), `eval_block`, `eval_call`, `eval_binop`,
//! and `match_pattern`. These are `impl Interp` methods moved into a second
//! inherent-impl block; they reach the parent's `call_builtin`/`call_fn`/
//! `call_closure`/`flatten_place` across split impl blocks, and `use super::*`
//! pulls in Expr/Stmt/Pattern/BinOp/Value/Flow/Env/R + the free helpers.

use super::*;

// ── R19 Slice B — coercion helpers ────────────────────────────────────────────

/// Map an `AxonType` annotation to the semantic `Type` it represents, but only
/// for non-i64 fixed-width integer types. Returns `None` for i64 (no coercion
/// needed — `Int(i64)` is already the correct representation) and for all
/// non-integer types.
fn axon_type_to_width(ty: &crate::ast::AxonType) -> Option<crate::types::Type> {
    use crate::ast::AxonType::Named;
    use crate::types::Type;
    match ty {
        Named(n) => match n.as_str() {
            "u8" => Some(Type::U8),
            "u16" => Some(Type::U16),
            "u32" => Some(Type::U32),
            "u64" => Some(Type::U64),
            "i8" => Some(Type::I8),
            "i16" => Some(Type::I16),
            "i32" => Some(Type::I32),
            _ => None, // i64 and non-integer names: no coercion
        },
        _ => None,
    }
}

/// Coerce a runtime value to a `SizedInt` when the target type is a non-i64
/// integer. `Int(n)` → `SizedInt{n, ty}`. Any other value is returned as-is
/// (the type-checker has already validated the types match; this is a
/// representation upgrade only). `SizedInt` with a different width is
/// re-tagged to the new width (preserves the stored bit-pattern; the checker
/// ensures same-width ops only).
fn coerce_to_sized(v: Value, width: crate::types::Type) -> Value {
    match v {
        Value::Int(n) => Value::SizedInt { val: n, ty: width },
        Value::SizedInt { val, .. } => Value::SizedInt { val, ty: width },
        other => other,
    }
}

impl<'p> Interp<'p> {
    // ── Core evaluator ───────────────────────────────────────────────────────

    pub(super) fn eval(&self, expr: &Expr, env: &mut Env) -> R {
        match expr {
            Expr::Literal(lit) => Ok(lit_to_val(lit)),

            Expr::Ident(name) => {
                if let Some(v) = env.get(name) {
                    Ok(v.clone())
                } else if let Some(v) = self.globals.get(name) {
                    Ok(v.clone())
                } else {
                    panic(format!("undefined identifier `{name}`"))
                }
            }

            Expr::Block(stmts) => self.eval_block(stmts, env),

            // Phase 6: `with handler { on E(p) => arm } { body }` installs an
            // effect-handler frame for the duration of `body`. A builtin in
            // `body` carrying effect `E` is intercepted (tail-resumptively) by
            // the `on E` arm — see `call_builtin`. An inline `return(v) => e`
            // arm rewrites the body's final value. NAMED handlers that did not
            // resolve to an inline definition (HandlerExpr::Named — an undefined
            // name) stay inert (run as the body), as before. NOTE (I-2): this
            // runtime interception is interpreter-only; native codegen still
            // erases handlers, a documented bounded divergence confined to
            // resume-using programs (none ship under examples/).
            Expr::WithHandler { handler, body } => self.eval_with_handler(handler, body, env),

            Expr::Let { name, value, ty }
            | Expr::Own { name, value, ty }
            | Expr::RefBind { name, value, ty } => {
                let v = self.eval(value, env)?;
                // Phase 5: a `let/own/ref p: T where P = …` annotation is a
                // refinement obligation — check the bound value against the
                // predicate (the non-constant case the checker defers; constant is
                // E1209). All three binding forms enforce it identically so native
                // codegen (which shares one Let/Own/RefBind arm) stays in lock-step
                // (I-2).
                if !self.refine_preds.is_empty() {
                    if let Some(crate::ast::AxonType::Named(rn)) = ty {
                        if let Some(pred) = self.refine_preds.get(rn.as_str()).copied() {
                            let mut pe = Env::new();
                            pe.define("_".into(), v.clone());
                            // Also bind the bound name for inline `let x: T where E[x] > k`.
                            pe.define(name.clone(), v.clone());
                            if let Value::Bool(false) = self.eval(pred, &mut pe)? {
                                return Err(Flow::RefineViolation(format!(
                                    "the value bound to `{}` (= {}) violates the refinement `{}` \
                                     — the value does not satisfy the type's predicate",
                                    name,
                                    display(&v),
                                    rn
                                )));
                            }
                        }
                    }
                }
                // R19 Slice B: if the annotation names a non-i64 integer type, coerce
                // the stored value to SizedInt so downstream arithmetic is width-correct.
                // Completeness requirement: EVERY static-type-introduction site must
                // coerce so no SizedInt value is left as a bare Int at any missed site,
                // which would silently compute in i64 (I-9).
                let v = if let Some(width) = ty.as_ref().and_then(axon_type_to_width) {
                    coerce_to_sized(v, width)
                } else {
                    v
                };
                env.define(name.clone(), v);
                Ok(Value::Unit)
            }

            Expr::Assign { name, value } => {
                let v = self.eval(value, env)?;
                if env.assign(name, v) {
                    Ok(Value::Unit)
                } else {
                    panic(format!("assignment to undefined variable `{name}`"))
                }
            }

            // Place assignment: `<place> = v`, where `place` is a (possibly
            // nested) chain of index / field accesses rooted at a variable —
            // e.g. `xs[i] = v`, `s.field = v`, `grid[i][j] = v`, `cfg.row[i] = v`.
            // Phase 1 flattens the place to (base ident, steps), evaluating index
            // expressions; phase 2 walks the binding mutably and sets the leaf.
            Expr::AssignTo { place, value } => {
                let v = self.eval(value, env)?;
                let (base, steps) = self.flatten_place(place, env)?;
                let mut slot = env.get_mut(&base).ok_or_else(|| {
                    Flow::Panic(format!("assignment to undefined variable `{base}`"))
                })?;
                let (last, prefix) = steps
                    .split_last()
                    .ok_or_else(|| Flow::Panic("invalid assignment target".into()))?;
                for step in prefix {
                    slot = match (step, slot) {
                        (PlaceStep::Field(f), Value::Struct { fields, .. }) => fields
                            .get_mut(f)
                            .ok_or_else(|| Flow::Panic(format!("no field `{f}`")))?,
                        (PlaceStep::Index(i), Value::Array(items)) => {
                            let n = items.len();
                            items.get_mut(*i).ok_or_else(|| {
                                Flow::Panic(format!("index {i} out of bounds (len {n})"))
                            })?
                        }
                        (_, other) => {
                            return panic(format!(
                                "cannot index/field-assign into {}",
                                other.type_name()
                            ));
                        }
                    };
                }
                match (last, slot) {
                    (PlaceStep::Field(f), Value::Struct { fields, .. }) => {
                        fields.insert(f.clone(), v);
                    }
                    (PlaceStep::Index(i), Value::Array(items)) => {
                        if *i >= items.len() {
                            return panic(format!("index {i} out of bounds (len {})", items.len()));
                        }
                        items[*i] = v;
                    }
                    (_, other) => {
                        return panic(format!(
                            "cannot index/field-assign into {}",
                            other.type_name()
                        ));
                    }
                }
                Ok(Value::Unit)
            }

            Expr::BinOp { op, left, right } => self.eval_binop(op, left, right, env),

            Expr::UnaryOp { op, operand } => {
                let v = self.eval(operand, env)?;
                eval_unary(op, v)
            }

            Expr::If { cond, then, else_ } => {
                let cv = self.eval(cond, env)?;
                // An `Uncertain<bool>` condition (e.g. `if a > 5` where `a` is
                // Uncertain — the comparison stays Uncertain) branches on its
                // inner bool; confidence is irrelevant to control flow. Unwrap it
                // to the inner value before the bool match.
                let cv = match soft_inner(&cv) {
                    Some(inner) => inner,
                    None => cv,
                };
                match cv {
                    Value::Bool(true) => self.eval(then, env),
                    Value::Bool(false) => match else_ {
                        Some(e) => self.eval(e, env),
                        None => Ok(Value::Unit),
                    },
                    other => panic(format!(
                        "if condition must be bool, got {}",
                        other.type_name()
                    )),
                }
            }

            Expr::Match { subject, arms } => {
                let v = self.eval(subject, env)?;
                for arm in arms {
                    env.push();
                    if self.match_pattern(&arm.pattern, &v, env)? {
                        if let Some(guard) = &arm.guard {
                            let ok = matches!(self.eval(guard, env)?, Value::Bool(true));
                            if !ok {
                                env.pop();
                                continue;
                            }
                        }
                        let r = self.eval(&arm.body, env);
                        env.pop();
                        return r;
                    }
                    env.pop();
                }
                panic("no match arm matched")
            }

            Expr::While { cond, body } => {
                loop {
                    let cv = self.eval(cond, env)?;
                    // An `Uncertain<bool>` condition branches on its inner bool
                    // (confidence is irrelevant to control flow) — same as `if`.
                    let cv = match soft_inner(&cv) {
                        Some(inner) => inner,
                        None => cv,
                    };
                    match cv {
                        Value::Bool(true) => {}
                        Value::Bool(false) => break,
                        other => {
                            return panic(format!(
                                "while condition must be bool, got {}",
                                other.type_name()
                            ))
                        }
                    }
                    match self.run_loop_body(body, env)? {
                        LoopStep::Break => break,
                        LoopStep::Continue => {}
                    }
                }
                Ok(Value::Unit)
            }

            Expr::WhileLet {
                pattern,
                expr,
                body,
            } => {
                loop {
                    let v = self.eval(expr, env)?;
                    env.push();
                    let matched = self.match_pattern(pattern, &v, env)?;
                    if !matched {
                        env.pop();
                        break;
                    }
                    let step = self.run_loop_body(body, env);
                    env.pop();
                    match step? {
                        LoopStep::Break => break,
                        LoopStep::Continue => {}
                    }
                }
                Ok(Value::Unit)
            }

            Expr::For {
                var,
                start,
                end,
                inclusive,
                body,
            } => {
                let s = self.eval_int(start, env)?;
                let e = self.eval_int(end, env)?;
                let mut i = s;
                loop {
                    let cont = if *inclusive { i <= e } else { i < e };
                    if !cont {
                        break;
                    }
                    env.push();
                    env.define(var.clone(), Value::Int(i));
                    let step = self.run_loop_body(body, env);
                    env.pop();
                    match step? {
                        LoopStep::Break => break,
                        LoopStep::Continue => {}
                    }
                    i += 1;
                }
                Ok(Value::Unit)
            }

            Expr::Return(opt) => {
                let v = match opt {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Unit,
                };
                Err(Flow::Return(v))
            }
            Expr::Break => Err(Flow::Break),
            Expr::Continue => Err(Flow::Continue),

            Expr::Question(inner) => match self.eval(inner, env)? {
                Value::Ok(x) => Ok(*x),
                Value::Some(x) => Ok(*x),
                Value::Err(e) => Err(Flow::Return(Value::Err(e))),
                Value::None => Err(Flow::Return(Value::None)),
                other => panic(format!(
                    "`?` applied to non-Result/Option ({})",
                    other.type_name()
                )),
            },

            Expr::Call { callee, args, tier } => {
                // `chan<T>()` lowers to a call whose callee is `chan::<T>`.
                if let Expr::StructLit { name, .. } = callee.as_ref() {
                    if name.starts_with("chan::<") {
                        return Ok(Value::Chan(Rc::new(RefCell::new(VecDeque::new()))));
                    }
                }
                self.eval_call(callee, args, tier.as_deref(), env)
            }

            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                let recv = self.eval(receiver, env)?;
                // Channel methods (cooperative, single-threaded): send pushes to
                // the shared queue, recv pops from it, clone shares the handle.
                if let Value::Chan(q) = &recv {
                    return match method.as_str() {
                        "send" => {
                            let v = self.eval(&args[0], env)?;
                            q.borrow_mut().push_back(v);
                            Ok(Value::Unit)
                        }
                        "recv" => q.borrow_mut().pop_front().ok_or_else(|| {
                            Flow::Panic(
                                "recv on an empty channel — the interpreter runs `spawn` bodies \
                                 eagerly, so a value must be sent before it is received"
                                    .into(),
                            )
                        }),
                        // Non-blocking pop. Returns `Some(v)` when a value is
                        // available, `None` otherwise. Lets ASI loops poll a
                        // channel without panicking on the empty case — useful
                        // for fan-out workers where the consumer races the
                        // producers and needs to know when results have stopped
                        // coming, not just block on the first miss.
                        "try_recv" => Ok(match q.borrow_mut().pop_front() {
                            Some(v) => Value::Some(Box::new(v)),
                            None => Value::None,
                        }),
                        // How many values are queued and unread. Useful with
                        // try_recv for "drain everything available" loops, or
                        // as a "did the workers do any work?" probe.
                        "len" => Ok(Value::Int(q.borrow().len() as i64)),
                        "clone" => Ok(Value::Chan(q.clone())),
                        other => panic(format!("no method `{other}` on a channel")),
                    };
                }
                let mut argv = Vec::with_capacity(args.len() + 1);
                argv.push(recv);
                for a in args {
                    argv.push(self.eval(a, env)?);
                }
                let tn = argv[0].type_name();
                if let Some(f) = self.methods.get(&(tn.clone(), method.clone())) {
                    self.call_fn(f, argv)
                } else {
                    panic(format!("no method `{method}` on type `{tn}`"))
                }
            }

            Expr::FieldAccess { receiver, field } => {
                let v = self.eval(receiver, env)?;
                match v {
                    Value::Struct { fields, .. } | Value::Enum { fields, .. } => fields
                        .get(field)
                        .cloned()
                        .ok_or_else(|| Flow::Panic(format!("no field `{field}`"))),
                    Value::Tuple(items) => {
                        // `t.0`, `t.1`, … : the parser stores the digit as the
                        // field name, and the interpreter reads it as the index.
                        let i: usize = field.parse().map_err(|_| {
                            Flow::Panic(format!(
                                "tuple access expects a numeric index, got `.{field}`"
                            ))
                        })?;
                        items.get(i).cloned().ok_or_else(|| {
                            Flow::Panic(format!(
                                "tuple index {i} out of bounds (len {})",
                                items.len()
                            ))
                        })
                    }
                    other => panic(format!(
                        "field access on non-struct ({})",
                        other.type_name()
                    )),
                }
            }

            Expr::Tuple(elems) => {
                let mut vs = Vec::with_capacity(elems.len());
                for e in elems {
                    vs.push(self.eval(e, env)?);
                }
                Ok(Value::Tuple(vs))
            }

            Expr::Index { receiver, index } => {
                // Phase 13 Slice 2: E[dist] and Var[dist] moment predicates.
                // Intercept before normal array indexing to avoid "undefined E".
                if let Expr::Ident(tag) = receiver.as_ref() {
                    if matches!(tag.as_str(), "E" | "Var") {
                        let dist_val = self.eval(index, env)?;
                        if let Some(moment) = eval_dist_moment(tag, &dist_val) {
                            return Ok(Value::Float(moment));
                        }
                    }
                }
                let arr = self.eval(receiver, env)?;
                let idx = self.eval_int(index, env)?;
                match arr {
                    Value::Array(items) => items.get(idx as usize).cloned().ok_or_else(|| {
                        Flow::Panic(format!("index {idx} out of bounds (len {})", items.len()))
                    }),
                    other => panic(format!("indexing non-array ({})", other.type_name())),
                }
            }

            Expr::Array(elems) => {
                let mut out = Vec::with_capacity(elems.len());
                for e in elems {
                    out.push(self.eval(e, env)?);
                }
                Ok(Value::Array(out))
            }

            Expr::StructLit { name, fields } => {
                let mut fmap = HashMap::with_capacity(fields.len());
                for (fname, fexpr) in fields {
                    let fval = self.eval(fexpr, env)?;
                    // R19 Slice B: coerce field values to SizedInt when the struct's
                    // declared field type is a non-i64 integer width.
                    let fval = if let Some(td) = self.structs.get(name.as_str()) {
                        if let Some(tf) = td.fields.iter().find(|f| &f.name == fname) {
                            if let Some(width) = axon_type_to_width(&tf.ty) {
                                coerce_to_sized(fval, width)
                            } else {
                                fval
                            }
                        } else {
                            fval
                        }
                    } else {
                        fval
                    };
                    fmap.insert(fname.clone(), fval);
                }
                if let Some((enum_name, variant)) = name.split_once("::") {
                    Ok(Value::Enum {
                        enum_name: enum_name.to_string(),
                        variant: variant.to_string(),
                        fields: fmap,
                    })
                } else {
                    // Phase 5: refinement obligations at struct CONSTRUCTION (the
                    // dual of the param/return checks), for non-constant values the
                    // checker (E1209) only discharges for constants. Two checks,
                    // both reusing the refinement-predicate evaluator with `_`
                    // bound to the relevant value. A whole-struct `where` lives on
                    // the TypeDef (not a RefineDef), so gate on the TypeDef having
                    // a refinement OR the program having named refinements (for
                    // refined fields), not on `refine_preds` alone.
                    if let Some(td) = self.structs.get(name.as_str()).copied() {
                        if td.refinement.is_some() || !self.refine_preds.is_empty() {
                            // (1) per-FIELD refinement: each field whose declared
                            // type is a refinement must satisfy that predicate.
                            for tf in &td.fields {
                                if let crate::ast::AxonType::Named(rn) = &tf.ty {
                                    if let Some(pred) = self.refine_preds.get(rn.as_str()).copied()
                                    {
                                        if let Some(fv) = fmap.get(&tf.name) {
                                            let mut pe = Env::new();
                                            pe.define("_".into(), fv.clone());
                                            if let Value::Bool(false) = self.eval(pred, &mut pe)? {
                                                return Err(Flow::RefineViolation(format!(
                                                    "field `{}` of `{}` (= {}) violates the refinement \
                                                     `{}` — the value does not satisfy the type's predicate",
                                                    tf.name,
                                                    name,
                                                    display(fv),
                                                    rn
                                                )));
                                            }
                                        }
                                    }
                                }
                            }
                            // (2) WHOLE-STRUCT refinement: `_` binds to the whole
                            // instance and `_.field` projects, so build it first.
                            if let Some(pred) = &td.refinement {
                                let sv = Value::Struct {
                                    name: name.clone(),
                                    fields: fmap,
                                };
                                let mut pe = Env::new();
                                pe.define("_".into(), sv.clone());
                                if let Value::Bool(false) = self.eval(pred, &mut pe)? {
                                    return Err(Flow::RefineViolation(format!(
                                        "the constructed `{name}` violates its struct refinement \
                                         — the value does not satisfy the type's predicate"
                                    )));
                                }
                                return Ok(sv);
                            }
                            return Ok(Value::Struct {
                                name: name.clone(),
                                fields: fmap,
                            });
                        }
                    }
                    Ok(Value::Struct {
                        name: name.clone(),
                        fields: fmap,
                    })
                }
            }

            Expr::Ok(e) => Ok(Value::Ok(Box::new(self.eval(e, env)?))),
            Expr::Err(e) => Ok(Value::Err(Box::new(self.eval(e, env)?))),
            Expr::Some(e) => Ok(Value::Some(Box::new(self.eval(e, env)?))),
            Expr::None => Ok(Value::None),

            Expr::FmtStr { parts } => {
                use crate::ast::FmtPart;
                let mut s = String::new();
                for part in parts {
                    match part {
                        FmtPart::Lit(t) => s.push_str(t),
                        FmtPart::Expr(e) => {
                            let v = self.eval(e, env)?;
                            s.push_str(&display(&v));
                        }
                    }
                }
                Ok(Value::Str(s))
            }

            Expr::Lambda { params, body, .. } => Ok(Value::Closure {
                params: params.iter().map(|p| p.name.clone()).collect(),
                body: body.clone(),
                captured: env.snapshot(),
            }),

            Expr::Comptime(inner) => self.eval(inner, env),

            // Cooperative concurrency: run the spawned body eagerly (single-
            // threaded), so its sends are queued before the main flow continues.
            Expr::Spawn(body) => {
                self.eval(body, env)?;
                Ok(Value::Unit)
            }
            // Cooperative select: fire the first arm whose channel has a ready
            // value (its queue is non-empty), consuming that value and running the
            // arm body. Arms are `c.recv() => body`. With eager `spawn`, channels
            // are pre-filled, so this is deterministic (first ready arm wins).
            Expr::Select(arms) => {
                for arm in arms {
                    let Expr::MethodCall {
                        receiver, method, ..
                    } = &arm.recv
                    else {
                        return panic("select arms must be channel `recv()` operations");
                    };
                    if method != "recv" {
                        return panic("select arms must be channel `recv()` operations");
                    }
                    let Value::Chan(q) = self.eval(receiver, env)? else {
                        return panic("select arm `recv` on a non-channel");
                    };
                    let ready = q.borrow_mut().pop_front();
                    if ready.is_some() {
                        return self.eval(&arm.body, env);
                    }
                }
                panic("select: no channel was ready (cooperative interpreter — send before select)")
            }
        }
    }

    pub(super) fn eval_block(&self, stmts: &[Stmt], env: &mut Env) -> R {
        env.push();
        let mut last = Value::Unit;
        for stmt in stmts {
            match self.eval(&stmt.expr, env) {
                Ok(v) => last = v,
                Err(e) => {
                    env.pop();
                    return Err(e);
                }
            }
        }
        env.pop();
        Ok(last)
    }

    /// Run a loop body (a `Vec<Stmt>`) in a fresh scope, translating `break`
    /// and `continue` into a [`LoopStep`] for the caller's loop construct.
    pub(super) fn run_loop_body(&self, body: &[Stmt], env: &mut Env) -> Result<LoopStep, Flow> {
        env.push();
        for stmt in body {
            match self.eval(&stmt.expr, env) {
                Ok(_) => {}
                Err(Flow::Break) => {
                    env.pop();
                    return Ok(LoopStep::Break);
                }
                Err(Flow::Continue) => {
                    env.pop();
                    return Ok(LoopStep::Continue);
                }
                Err(e) => {
                    env.pop();
                    return Err(e);
                }
            }
        }
        env.pop();
        Ok(LoopStep::Continue)
    }

    pub(super) fn eval_call(
        &self,
        callee: &Expr,
        args: &[Expr],
        tier: Option<&str>,
        env: &mut Env,
    ) -> R {
        // Phase 13 Slice 2: P(dist op k) probability predicate.
        // The argument is a comparison expression, not a plain value — intercept
        // before normal arg evaluation to avoid evaluating "dist <= k" literally.
        if let Expr::Ident(name) = callee {
            if name == "P" && args.len() == 1 {
                if let Some(prob) = self.eval_prob_pred(&args[0], env) {
                    return Ok(Value::Float(prob));
                }
                // If we can't recognize the pattern, fall through to "undefined P"
                // which will surface as a meaningful error rather than a type error.
            }
        }

        // Evaluate arguments left-to-right.
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval(a, env)?);
        }

        // Phase 6: `resume(v)` inside a handler arm carries `v` back to the
        // intercepted operation as a `Flow::Resume`. It is caught at the
        // builtin-interception site (`run_handler_arm`). `resume` with no arg
        // resumes with Unit. (The resolver only binds `resume` inside an arm, so
        // a `resume` here is genuinely a handler resume, not a user fn named
        // `resume` — and the builtin/user-fn lookups never define one.)
        if let Expr::Ident(name) = callee {
            if name == "resume" {
                let v = argv.into_iter().next().unwrap_or(Value::Unit);
                // Phase 6 multi-shot: if a handler arm is currently servicing a
                // suspended computation (`resume_ctx` non-empty) AND we are not
                // already inside a replay (no nested-replay re-entrancy), reify
                // the continuation by REPLAYING the body with `v` fed at the
                // intercepted op, and return its value to the arm. This lets the
                // arm use `resume`'s result (`let a = resume(2)`) and resume again
                // (multi-shot) — without a CPS rewrite. When there is no ctx (or
                // we're mid-replay), fall back to the single-shot `Flow::Resume`
                // unwind, which the interception site catches tail-resumptively
                // (byte-identical to the pre-multishot fast path).
                let in_replay = self.resume_replay.borrow().is_some();
                let has_ctx = !self.resume_ctx.borrow().is_empty();
                if has_ctx && !in_replay {
                    return self.replay_continuation(v);
                }
                return Err(Flow::Resume(v));
            }
        }

        // R3b: make the per-call `tier:` (if any) visible to the builtin dispatch
        // for the duration of this call (read by `current_ai_tier`).
        *self.current_call_tier.borrow_mut() = tier.map(|t| t.to_string());

        if let Expr::Ident(name) = callee {
            // 1. A local/captured variable holding a closure.
            if let Some(Value::Closure { .. }) = env.get(name) {
                let c = env.get(name).unwrap().clone();
                return self.call_closure(c, argv);
            }
            // 2. A builtin.
            if let Some(v) = self.call_builtin(name, &argv)? {
                return Ok(v);
            }
            // 3. A user-defined function.
            if let Some(f) = self.fns.get(name) {
                return self.call_fn(f, argv);
            }
            // 4. A module-level closure constant.
            if let Some(Value::Closure { .. }) = self.globals.get(name) {
                let c = self.globals.get(name).unwrap().clone();
                return self.call_closure(c, argv);
            }
            return panic(format!("call to unknown function `{name}`"));
        }

        // Callee is an expression that should evaluate to a closure
        // (e.g. `make_adder(1)(2)` or an array element).
        let c = self.eval(callee, env)?;
        self.call_closure(c, argv)
    }

    /// Phase 6: evaluate `with handler { … } { body }`. Installs the inline
    /// handler's arms as an active frame for the duration of `body`, then (if an
    /// inline `return(v) => e` arm is present) rewrites the body's value. A
    /// `HandlerExpr::Named` that did not desugar to an inline definition is an
    /// unresolved name → the handler is inert (run the body unwrapped).
    fn eval_with_handler(
        &self,
        handler: &crate::ast::HandlerExpr,
        body: &Expr,
        env: &mut Env,
    ) -> R {
        let (arms, return_arm) = match handler {
            crate::ast::HandlerExpr::Inline { arms, return_arm } => (arms, return_arm),
            // Unresolved named handler: inert (matches the pre-Phase-6 behavior).
            crate::ast::HandlerExpr::Named(_) => return self.eval(body, env),
        };

        // Capture the defining environment once; every arm closes over it.
        let captured = env.snapshot();
        let frame = HandlerFrame {
            arms: arms
                .iter()
                .map(|a| HandlerArmRt {
                    effect: a.effect.clone(),
                    binding: a.binding.clone(),
                    body: a.body.clone(),
                    captured: captured.clone(),
                })
                .collect(),
            // Phase 6 multi-shot: remember the body + its env so a non-tail arm
            // can replay the continuation.
            body: body.clone(),
            env_snapshot: env.snapshot(),
        };
        self.handlers.borrow_mut().push(frame);
        let result = self.eval(body, env);
        self.handlers.borrow_mut().pop();
        // Phase 6 multi-shot: a non-tail / multi-resume arm reifies the
        // continuation by replay and finishes the WHOLE block with
        // `Flow::HandlerDone(v)` — the original suspended body is abandoned, so
        // catch it here and use `v` as the block value (the single-shot tail path
        // never raises this, so its behavior is unchanged).
        let mut value = match result {
            Ok(v) => v,
            Err(Flow::HandlerDone(v)) => v,
            Err(e) => return Err(e),
        };

        // An inline `return(v) => e` arm rewrites the body's final value.
        if let Some(ra) = return_arm {
            let mut ra_env = Env::from_snapshot(captured);
            ra_env.push();
            self.match_pattern(&ra.binding, &value, &mut ra_env)?;
            value = self.eval(&ra.body, &mut ra_env)?;
        }
        Ok(value)
    }

    /// Phase 6: run the nearest active handler arm for effect `eff`, with the
    /// intercepted operation's `payload` bound (the builtin's args: the single
    /// value, or a tuple for 0/2+ args). Returns `Ok(Some(v))` when the arm calls
    /// `resume(v)` — `v` is the operation's result and the handled computation
    /// continues (tail-resumptive); `Ok(None)` when no active arm handles `eff`
    /// (the caller performs the real operation); or `Err(Flow::Return(v))` when
    /// the arm completes WITHOUT resuming — its value `v` replaces the whole
    /// `with` block (handle-and-abort), unwinding to the enclosing `with` frame.
    /// Phase 6 (multi-shot resume): compute the continuation value for a
    /// `resume(v)` by REPLAYING the handled body with `v` fed at the intercepted
    /// op. Re-runs `body` (from the active `resume_ctx`) in its captured env with
    /// `resume_replay` armed so the FIRST hit of the handled effect yields `v`
    /// directly (no re-interception) and the body runs straight through to its
    /// value. Any SECOND effect during the replay is unsound to re-fire (the side
    /// effect already happened on the original pass) → E1314. The arm may call
    /// this many times with different `v` (multi-shot): each call is an
    /// independent replay over the same suspended body, so backtracking search,
    /// retry loops, and `a + b` over two resumes all work.
    fn replay_continuation(&self, v: Value) -> R {
        let ctx = self
            .resume_ctx
            .borrow()
            .last()
            .cloned()
            .expect("replay_continuation called with no active resume_ctx");
        // Arm the feed: the first handled-effect hit during the replay returns
        // `v`; a second effect hit trips E1314.
        *self.resume_replay.borrow_mut() = Some(crate::interp::ResumeReplay {
            effect: ctx.effect.clone(),
            feed: v,
            consumed: false,
        });
        let mut body_env = Env::from_snapshot(ctx.env_snapshot.clone());
        let result = self.eval(&ctx.body, &mut body_env);
        // Disarm regardless of outcome so a later resume (or the arm's own code)
        // is not mistaken for a replay.
        *self.resume_replay.borrow_mut() = None;
        result
    }

    pub(super) fn run_handler_arm(&self, eff: &str, payload: Value) -> Result<Option<Value>, Flow> {
        // (The replay-feed interception lives in the builtin dispatch — it must
        // fire even though the handler frame is split off during the arm, so it
        // cannot be gated on an active frame here.)

        // Find the INDEX of the nearest frame with a matching arm (search from
        // the top/innermost). Clone the arm data so we don't hold the RefCell
        // borrow across arm evaluation. Also grab that frame's body + env snapshot
        // so a non-tail arm can replay the continuation (multi-shot).
        let hit = {
            let stack = self.handlers.borrow();
            stack.iter().enumerate().rev().find_map(|(i, frame)| {
                frame.arms.iter().find(|a| a.effect == eff).map(|a| {
                    (
                        i,
                        a.binding.clone(),
                        a.body.clone(),
                        a.captured.clone(),
                        frame.body.clone(),
                        frame.env_snapshot.clone(),
                    )
                })
            })
        };
        let Some((idx, binding, arm_body, captured, with_body, with_env)) = hit else {
            return Ok(None);
        };

        // SHALLOW-handler semantics: the arm body runs OUTSIDE the handler it
        // belongs to. Temporarily remove the handling frame AND every frame
        // inner to it (they were installed inside the now-suspended body) so an
        // effect performed BY THE ARM is not re-intercepted by the same handler —
        // that self-interception is an infinite loop (a handler whose `on IO`
        // arm itself does IO). The removed frames are restored after the arm
        // runs, whether it resumes, aborts, or errors.
        let suspended: Vec<HandlerFrame> = self.handlers.borrow_mut().split_off(idx);

        // BARE TAIL-RESUME FAST PATH (single-shot): when the arm body is exactly
        // `resume(<expr>)`, the operation yields that value and the suspended body
        // continues in place — no continuation reification needed. This is the
        // ONLY shape native codegen lowers, and the path the parity harness pins,
        // so it must stay byte-identical: evaluate the arm and propagate the
        // `Flow::Resume(v)` it raises directly (no resume_ctx, no replay).
        if crate::effects::arm_is_bare_tail_resume(&arm_body) {
            let mut arm_env = Env::from_snapshot(captured);
            arm_env.push();
            let bound = self.match_pattern(&binding, &payload, &mut arm_env);
            let outcome = bound.and_then(|_| self.eval(&arm_body, &mut arm_env));
            self.handlers.borrow_mut().extend(suspended);
            return match outcome {
                Err(Flow::Resume(v)) => Ok(Some(v)),
                Ok(v) => Err(Flow::Return(v)),
                Err(other) => Err(other),
            };
        }

        // GENERAL PATH (non-tail / multi-shot / abort): push a resume context so a
        // `resume(v)` inside the arm reifies the continuation by replaying
        // `with_body` (feeding `v` at the intercepted op) and RETURNS its value to
        // the arm — letting the arm use the result and resume again (multi-shot).
        self.resume_ctx.borrow_mut().push(crate::interp::ResumeCtx {
            effect: eff.to_string(),
            body: with_body,
            env_snapshot: with_env,
        });
        let mut arm_env = Env::from_snapshot(captured);
        arm_env.push();
        let bound = self.match_pattern(&binding, &payload, &mut arm_env);
        let outcome = bound.and_then(|_| self.eval(&arm_body, &mut arm_env));
        self.resume_ctx.borrow_mut().pop();
        self.handlers.borrow_mut().extend(suspended);

        match outcome {
            // The arm ran to completion. Whether it resumed zero times (abort) or
            // one-or-more times (each `resume` was a replay returning a value),
            // the arm's final value `v` is the value of the WHOLE `with` block.
            // The original suspended body is abandoned (the continuation was
            // reified via replay), so finish the block with HandlerDone — caught
            // by `eval_with_handler`.
            Ok(v) => Err(Flow::HandlerDone(v)),
            // A stray tail `resume` that escaped without a ctx (shouldn't happen
            // on this path, but be safe): treat as a single-shot resume.
            Err(Flow::Resume(v)) => Ok(Some(v)),
            // E1314 and any other control flow (panic, exit, …) propagate.
            Err(other) => Err(other),
        }
    }

    pub(super) fn eval_binop(&self, op: &BinOp, left: &Expr, right: &Expr, env: &mut Env) -> R {
        // Short-circuit boolean operators. An `Uncertain<bool>` operand can't
        // short-circuit (we must combine confidences), so it falls through to
        // the value-level path which propagates uncertainty.
        match op {
            BinOp::And => {
                let lv = self.eval(left, env)?;
                if let Value::Bool(false) = lv {
                    return Ok(Value::Bool(false));
                }
                if let Value::Bool(true) = lv {
                    return match self.eval(right, env)? {
                        Value::Bool(b) => Ok(Value::Bool(b)),
                        other if uncertain_parts(&other).is_some() => {
                            eval_binop_vals(op, lv, other)
                        }
                        other => panic(format!("`&&` rhs must be bool, got {}", other.type_name())),
                    };
                }
                // lv is Uncertain (or other) — value-level path handles/errors.
                let rv = self.eval(right, env)?;
                return eval_binop_vals(op, lv, rv);
            }
            BinOp::Or => {
                let lv = self.eval(left, env)?;
                if let Value::Bool(true) = lv {
                    return Ok(Value::Bool(true));
                }
                if let Value::Bool(false) = lv {
                    return match self.eval(right, env)? {
                        Value::Bool(b) => Ok(Value::Bool(b)),
                        other if uncertain_parts(&other).is_some() => {
                            eval_binop_vals(op, lv, other)
                        }
                        other => panic(format!("`||` rhs must be bool, got {}", other.type_name())),
                    };
                }
                let rv = self.eval(right, env)?;
                return eval_binop_vals(op, lv, rv);
            }
            _ => {}
        }

        let l = self.eval(left, env)?;
        let r = self.eval(right, env)?;
        eval_binop_vals(op, l, r)
    }

    pub(super) fn eval_int(&self, expr: &Expr, env: &mut Env) -> Result<i64, Flow> {
        match self.eval(expr, env)? {
            Value::Int(n) => Ok(n),
            other => panic(format!("expected i64, got {}", other.type_name())),
        }
    }

    // ── Pattern matching ─────────────────────────────────────────────────────

    /// Try to match `val` against `pat`, binding identifiers into the current
    /// scope of `env`. Returns whether it matched.
    pub(super) fn match_pattern(
        &self,
        pat: &Pattern,
        val: &Value,
        env: &mut Env,
    ) -> Result<bool, Flow> {
        match pat {
            Pattern::Wildcard => Ok(true),
            Pattern::Ident(name) => {
                env.define(name.clone(), val.clone());
                Ok(true)
            }
            Pattern::Literal(lit) => Ok(values_equal(&lit_to_val(lit), val)),
            Pattern::Some(inner) => match val {
                Value::Some(v) => self.match_pattern(inner, v, env),
                _ => Ok(false),
            },
            Pattern::None => Ok(matches!(val, Value::None)),
            Pattern::Ok(inner) => match val {
                Value::Ok(v) => self.match_pattern(inner, v, env),
                _ => Ok(false),
            },
            Pattern::Err(inner) => match val {
                Value::Err(v) => self.match_pattern(inner, v, env),
                _ => Ok(false),
            },
            Pattern::Struct { name, fields } => {
                // Enum-variant pattern when the name is qualified (`Enum::Variant`).
                let field_map = if let Some((enum_name, variant)) = name.split_once("::") {
                    match val {
                        Value::Enum {
                            enum_name: en,
                            variant: v,
                            fields,
                        } if en == enum_name && v == variant => fields,
                        _ => return Ok(false),
                    }
                } else {
                    match val {
                        Value::Struct { name: sn, fields } if sn == name => fields,
                        _ => return Ok(false),
                    }
                };
                for (fname, fpat) in fields {
                    let Some(fval) = field_map.get(fname) else {
                        return Ok(false);
                    };
                    let fval = fval.clone();
                    if !self.match_pattern(fpat, &fval, env)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Pattern::Tuple(pats) => {
                let Value::Tuple(items) = val else {
                    return Ok(false);
                };
                if items.len() != pats.len() {
                    return Ok(false);
                }
                for (p, v) in pats.iter().zip(items.iter()) {
                    let v = v.clone();
                    if !self.match_pattern(p, &v, env)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    // ── Phase 13 Slice 2: probabilistic predicate helpers ────────────────────

    /// Evaluate `P(dist op k)` — extracts the distribution and threshold from
    /// the comparison argument and returns the tail probability as `Some(f64)`.
    /// Returns `None` when the pattern isn't recognized (falls through to error).
    fn eval_prob_pred(&self, arg: &Expr, env: &mut Env) -> Option<f64> {
        use crate::ast::BinOp;
        let Expr::BinOp { op, left, right } = arg else {
            return None;
        };
        let dist_val = self.eval(left, env).ok()?;
        let k = self.eval_f64_val(right, env)?;
        let cdf = eval_dist_cdf(&dist_val, k)?;
        Some(match op {
            BinOp::LtEq => cdf,
            BinOp::Lt => cdf, // continuous: P(X < k) == P(X <= k)
            BinOp::Gt => 1.0 - cdf,
            BinOp::GtEq => 1.0 - cdf,
            BinOp::Eq => {
                // For continuous distributions P(X = k) = 0; Categorical: exact mass
                if let Value::Struct { name, fields } = &dist_val {
                    if name == "Categorical" {
                        if let Some(Value::Array(probs)) = fields.get("probs") {
                            let ki = k as i64;
                            if ki >= 0 && (ki as usize) < probs.len() {
                                if let Value::Float(p) = probs[ki as usize] {
                                    return Some(p);
                                }
                            }
                        }
                    }
                }
                0.0
            }
            _ => return None,
        })
    }

    /// Evaluate an expression as f64 (literal float or literal int cast).
    fn eval_f64_val(&self, expr: &Expr, env: &mut Env) -> Option<f64> {
        match self.eval(expr, env).ok()? {
            Value::Float(f) => Some(f),
            Value::Int(n) => Some(n as f64),
            _ => None,
        }
    }
}

// ── Phase 13: distribution moment + CDF evaluation (free fns) ────────────────

/// Compute E[dist] or Var[dist] from a runtime distribution struct value.
/// Returns None for unrecognized distributions.
pub(super) fn eval_dist_moment(tag: &str, dist: &Value) -> Option<f64> {
    let Value::Struct { name, fields } = dist else {
        return None;
    };
    match name.as_str() {
        "Gaussian" => {
            let mu = get_f64_field(fields, "mu")?;
            let sigma = get_f64_field(fields, "sigma")?;
            match tag {
                "E" => Some(mu),
                "Var" => Some(sigma * sigma),
                _ => None,
            }
        }
        "Beta" => {
            let alpha = get_f64_field(fields, "alpha")?;
            let beta_b = get_f64_field(fields, "beta_b")?;
            let s = alpha + beta_b;
            match tag {
                "E" => Some(alpha / s),
                "Var" => Some((alpha * beta_b) / (s * s * (s + 1.0))),
                _ => None,
            }
        }
        "Categorical" => {
            let probs = get_f64_array_field(fields, "probs")?;
            if probs.is_empty() {
                return None;
            }
            let mean: f64 = probs.iter().enumerate().map(|(i, p)| i as f64 * p).sum();
            match tag {
                "E" => Some(mean),
                "Var" => {
                    let e2: f64 = probs
                        .iter()
                        .enumerate()
                        .map(|(i, p)| i as f64 * i as f64 * p)
                        .sum();
                    Some(e2 - mean * mean)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Compute CDF P(X <= k) for a distribution struct value.
fn eval_dist_cdf(dist: &Value, k: f64) -> Option<f64> {
    let Value::Struct { name, fields } = dist else {
        return None;
    };
    match name.as_str() {
        "Gaussian" => {
            let mu = get_f64_field(fields, "mu")?;
            let sigma = get_f64_field(fields, "sigma")?;
            if sigma <= 0.0 {
                return None;
            }
            Some(gaussian_cdf(mu, sigma, k))
        }
        "Beta" => {
            let alpha = get_f64_field(fields, "alpha")?;
            let beta_b = get_f64_field(fields, "beta_b")?;
            if alpha <= 0.0 || beta_b <= 0.0 {
                return None;
            }
            Some(beta_cdf(alpha, beta_b, k))
        }
        "Categorical" => {
            let probs = get_f64_array_field(fields, "probs")?;
            let ki = k as i64;
            if ki < 0 {
                return Some(0.0);
            }
            let limit = (ki as usize + 1).min(probs.len());
            Some(probs[..limit].iter().sum())
        }
        _ => None,
    }
}

fn get_f64_field(fields: &HashMap<String, Value>, key: &str) -> Option<f64> {
    match fields.get(key)? {
        Value::Float(f) => Some(*f),
        Value::Int(n) => Some(*n as f64),
        _ => None,
    }
}

fn get_f64_array_field(fields: &HashMap<String, Value>, key: &str) -> Option<Vec<f64>> {
    match fields.get(key)? {
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for v in items {
                match v {
                    Value::Float(f) => out.push(*f),
                    Value::Int(n) => out.push(*n as f64),
                    _ => return None,
                }
            }
            Some(out)
        }
        _ => None,
    }
}

// Gaussian CDF via Abramowitz & Stegun erf approximation (max error < 1.5e-7)
fn gaussian_cdf(mu: f64, sigma: f64, x: f64) -> f64 {
    let z = (x - mu) / (sigma * std::f64::consts::SQRT_2);
    0.5 * (1.0 + erf(z))
}

fn erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t
        * (0.254829592
            + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    sign * (1.0 - poly * (-x * x).exp())
}

// Beta CDF via Lentz continued-fraction algorithm for regularized incomplete beta
fn beta_cdf(alpha: f64, beta_b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    // Use symmetry relation for numerical stability
    if x > (alpha + 1.0) / (alpha + beta_b + 2.0) {
        return 1.0 - beta_cdf(beta_b, alpha, 1.0 - x);
    }
    let ln_beta = ln_gamma(alpha) + ln_gamma(beta_b) - ln_gamma(alpha + beta_b);
    let front = (alpha * x.ln() + beta_b * (1.0 - x).ln() - ln_beta).exp() / alpha;
    front * beta_cf(alpha, beta_b, x)
}

fn beta_cf(alpha: f64, beta_b: f64, x: f64) -> f64 {
    let max_iter = 200;
    let eps = 1e-10;
    let mut c = 1.0;
    let mut d = 1.0 - (alpha + beta_b) * x / (alpha + 1.0);
    d = 1.0
        / if d.abs() < f64::MIN_POSITIVE {
            f64::MIN_POSITIVE
        } else {
            d
        };
    let mut f = d;
    for m in 1..=max_iter {
        let m = m as f64;
        // Even step
        let num = m * (beta_b - m) * x / ((alpha + 2.0 * m - 1.0) * (alpha + 2.0 * m));
        d = 1.0 + num * d;
        c = 1.0 + num / c;
        d = 1.0
            / if d.abs() < f64::MIN_POSITIVE {
                f64::MIN_POSITIVE
            } else {
                d
            };
        c = if c.abs() < f64::MIN_POSITIVE {
            f64::MIN_POSITIVE
        } else {
            c
        };
        f *= d * c;
        // Odd step
        let num =
            -(alpha + m) * (alpha + beta_b + m) * x / ((alpha + 2.0 * m) * (alpha + 2.0 * m + 1.0));
        d = 1.0 + num * d;
        c = 1.0 + num / c;
        d = 1.0
            / if d.abs() < f64::MIN_POSITIVE {
                f64::MIN_POSITIVE
            } else {
                d
            };
        c = if c.abs() < f64::MIN_POSITIVE {
            f64::MIN_POSITIVE
        } else {
            c
        };
        let delta = d * c;
        f *= delta;
        if (delta - 1.0).abs() < eps {
            break;
        }
    }
    f
}

fn ln_gamma(x: f64) -> f64 {
    // Lanczos approximation
    let p = [
        676.5203681218851_f64,
        -1259.1392167224028,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507343278686905,
        -0.13857109526572012,
        9.984_369_578_019_572e-6,
        1.5056327351493116e-7,
    ];
    let x = x - 1.0;
    let mut a = 0.999_999_999_999_809_9_f64;
    for (i, &p_i) in p.iter().enumerate() {
        a += p_i / (x + i as f64 + 1.0);
    }
    let t = x + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
}
