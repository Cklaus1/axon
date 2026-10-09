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
/// representation upgrade only). A `SizedInt` of another width is left as it
/// is, and the declared-type cast that follows refuses it (the checker
/// refuses it too, E0307): re-tagging it would turn one width into another
/// without converting its value (C9 round 4b, amendment 60).
fn coerce_to_sized(v: Value, width: crate::types::Type) -> Value {
    match v {
        Value::Int(n) => Value::SizedInt { val: n, ty: width },
        other => other,
    }
}

/// R28 ledger class for a native module's declared effect row (AUDIT T45).
/// Native modules carry an effect row rather than a builtin name, so the
/// name-keyed `audit_effect_kind` does not apply to them.
fn native_ledger_kind(effects: &[&str]) -> Option<axon_audit::EffectKind> {
    effects.iter().find_map(|&tag| match tag {
        "FS" => Some(axon_audit::EffectKind::FS),
        "Net" => Some(axon_audit::EffectKind::Net),
        "AI" => Some(axon_audit::EffectKind::AI),
        "Exec" => Some(axon_audit::EffectKind::Exec),
        "Random" => Some(axon_audit::EffectKind::Random),
        "IO" => Some(axon_audit::EffectKind::IO),
        _ => None,
    })
}

/// Whether `e` reads, assigns or rebinds the variable `name` anywhere.
fn mentions_var(e: &Expr, name: &str) -> bool {
    let mut hit = false;
    crate::ast::walk_expr(e, &mut |x| {
        hit |= match x {
            Expr::Ident(n)
            | Expr::Assign { name: n, .. }
            | Expr::Let { name: n, .. }
            | Expr::Own { name: n, .. }
            | Expr::RefBind { name: n, .. } => n == name,
            _ => false,
        }
    });
    hit
}

/// The appending updates `assign_in_place` performs on a variable's buffer.
#[derive(Clone, Copy)]
enum AppendOp {
    /// `x = arr_push(x, v)`
    Push,
    /// `x = arr_concat(x, ys)`
    Concat,
    /// `x = x + y` (str or array)
    Add,
}

/// AX-25: the closure value of the top-level fn `name` (arity `arity`): the
/// capture-free forwarding lambda `|#0, #1, ..| name(#0, #1, ..)`. The `#n`
/// parameter names cannot be written in source, so they never shadow a name the
/// callee's body or arguments could refer to.
fn fn_value(name: &str, arity: usize, sealed: bool) -> Value {
    let params: Vec<String> = (0..arity).map(|i| format!("#{i}")).collect();
    let args = params.iter().map(|p| Expr::Ident(p.clone())).collect();
    let mut cell = HashMap::new();
    // A fn value a SEALED frame took (the call edge already refused every
    // operator fn there) carries its own mark: without it the candidate's
    // call through its own `let g = inc; g(n)` read as sealed code calling an
    // OPERATOR closure and was refused (C9 round 9). The operator's fn values
    // stay unmarked. It is NOT `SEALED_CLOSURE_MARK`: the body still runs in
    // the operator frame and reaches the candidate fn through `call_fn`'s own
    // seal crossing (strict return cast), exactly as before.
    if sealed {
        cell.insert(SEALED_FNVAL_MARK.to_string(), Value::Bool(true));
    }
    Value::Closure {
        params,
        body: Box::new(Expr::Call {
            callee: Box::new(Expr::Ident(name.to_string())),
            args,
            tier: None,
        }),
        captured: std::rc::Rc::new(std::cell::RefCell::new(cell)),
        contract: None,
    }
}

impl<'p> Interp<'p> {
    // ── Core evaluator ───────────────────────────────────────────────────────

    /// Evaluate `expr`: the plain evaluator, or in a SEALED run the one wrapped by
    /// the taint accumulator (`interp/taint.rs`). This is the entry for code that
    /// does not know which run it is in; the evaluator's own recursion (and the
    /// fn and closure calls under it) is generic over the run (`eval_t::<T>`) and
    /// never branches here. NOT a function pointer: the wasm Asyncify guard
    /// (R15) refuses a module in which a fn that can suspend is address-taken.
    #[inline]
    pub(super) fn eval(&self, expr: &Expr, env: &mut Env) -> R {
        if self.seal.active {
            self.eval_tainted(expr, env)
        } else {
            self.eval_arm::<false>(expr, env)
        }
    }

    /// `eval`, resolved at compile time: the evaluator's own recursion knows
    /// whether it is the sealed run's (`T`) and calls the right one DIRECTLY.
    #[inline(always)]
    pub(super) fn eval_t<const T: bool>(&self, expr: &Expr, env: &mut Env) -> R {
        if T {
            self.eval_tainted(expr, env)
        } else {
            self.eval_arm::<false>(expr, env)
        }
    }

    pub(super) fn eval_arm<const T: bool>(&self, expr: &Expr, env: &mut Env) -> R {
        match expr {
            Expr::Literal(lit) => Ok(lit_to_val(lit)),

            Expr::Ident(name) => {
                if let Some(v) = env.get(name) {
                    Ok(v.clone())
                } else if let Some(v) = self.global_ref(name)? {
                    Ok(v.clone())
                } else if let Some(f) = self.fns.get(name.as_str()) {
                    // AX-25: a top-level fn named in VALUE position (`let g = f`,
                    // `[f, h]`, `apply(f, x)`) is a first-class closure. It is the
                    // forwarding lambda `|a0, ..| f(a0, ..)` with no captures, so a
                    // call through the value re-enters `eval_call` by NAME and takes
                    // exactly the path a direct `f(..)` call takes — contracts,
                    // `@[verify]` gates, effect/capability gates and provenance
                    // included. The resolver refuses builtins and generic fns here.
                    //
                    // PCI: naming a fn in value position is a REFERENCE to it, so the
                    // call edge applies HERE: a sealed frame may not take an operator
                    // fn as a value (the forwarding body would run it in an operator
                    // frame, where the edge no longer applies). Provenance needs no
                    // mark: a candidate fn's value, called from anywhere, reaches the
                    // candidate fn through `call_fn`'s own crossing.
                    self.seal_call(f)?;
                    Ok(fn_value(
                        name,
                        f.params.len(),
                        self.seal.active && self.frame_sealed.get(),
                    ))
                } else {
                    self.no_such_fn(name, format!("undefined identifier `{name}`"))
                }
            }

            Expr::Block(stmts) => self.eval_block::<T>(stmts, env),

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
                let v = self.eval_t::<T>(value, env)?;
                let mut vt = self.tl::<T>();
                // Phase 5: a `let/own/ref p: T where P = …` annotation is a
                // refinement obligation — check the bound value against the
                // predicate (the non-constant case the checker defers; constant is
                // E1209). All three binding forms enforce it identically so native
                // codegen (which shares one Let/Own/RefBind arm) stays in lock-step
                // (I-2).
                if !self.refine_preds.is_empty() {
                    if let Some(crate::ast::AxonType::Named(rn)) = ty {
                        if let Some(pred) = self.refine_preds.get(rn.as_str()).copied() {
                            self.seal_refine(rn)?;
                            let mut pe = Env::new();
                            pe.define("_".into(), v.clone(), vt);
                            // Also bind the bound name for inline `let x: T where E[x] > k`.
                            pe.define(name.clone(), v.clone(), vt);
                            if let Value::Bool(false) = crate::interp::contain_frame(self.eval_t::<T>(pred, &mut pe), "a predicate")? {
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
                let mut v = if let Some(width) = ty.as_ref().and_then(axon_type_to_width) {
                    coerce_to_sized(v, width)
                } else {
                    v
                };
                // The annotation is a declared type: cast to it (amendment 53).
                if let Some(t) = ty {
                    if let Err(why) = self.cast(&mut v, t, &Default::default()) {
                        return panic(format!(
                            "`{name}` is declared `{}` but was bound to {} — a runtime type \
                             confusion ({why})",
                            crate::doc::render_type(t),
                            display(&v)
                        ));
                    }
                    // A closed declared type the cast verified is a PIN: the
                    // runtime type is the operator's, whatever produced the value.
                    // It pins a type, never a value (amendment 102).
                    if vt & taint::TYP != 0 && self.t_pins(t) {
                        vt &= !taint::TYP;
                    }
                }
                // A fresh binding lives and dies in the scope it was made in: it
                // carries the VALUE's taint (the evaluator already added the
                // taint of any early exit before it). Control taints what is
                // WRITTEN to a binding that outlives the branch (an assignment).
                env.define(name.clone(), v, vt);
                Ok(Value::Unit)
            }

            Expr::Assign { name, value } => {
                if self.assign_in_place::<T>(name, value, env)? {
                    return Ok(Value::Unit);
                }
                let v = self.eval_t::<T>(value, env)?;
                let vt = if T {
                    self.ts::<T>(self.tl::<T>())
                } else {
                    0
                };
                if env.assign(name, v, vt) {
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
                let v = self.eval_t::<T>(value, env)?;
                let (base, steps) = self.flatten_place(place, env)?;
                // The write puts the value (and the index it was written at) into
                // the container the binding holds: the binding now carries it.
                if T {
                    let wt = self.ts::<T>(self.ta::<T>());
                    env.taint_or(&base, wt);
                }
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
                            // Copy-on-write: copies only if this array is shared
                            // with another binding; a uniquely owned one is
                            // written in place.
                            Rc::make_mut(items).get_mut(*i).ok_or_else(|| {
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
                        Rc::make_mut(items)[*i] = v;
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

            Expr::BinOp { op, left, right } => {
                if !T {
                    return self.eval_binop::<T>(op, left, right, env);
                }
                let v = self.eval_binop::<T>(op, left, right, env)?;
                // A comparison or a logical operator yields a `bool` whoever its
                // operands were: the OPERATOR's operator chose that type.
                if T
                    && matches!(
                        op,
                        BinOp::Eq
                            | BinOp::NotEq
                            | BinOp::Lt
                            | BinOp::Gt
                            | BinOp::LtEq
                            | BinOp::GtEq
                            | BinOp::And
                            | BinOp::Or
                    )
                {
                    self.t_untype();
                }
                Ok(v)
            }

            Expr::UnaryOp { op, operand } => {
                let v = self.eval_t::<T>(operand, env)?;
                let vt = self.tl::<T>();
                self.seal_width_unary(op, operand, &v)?;
                if vt & taint::TYP != 0 && matches!(op, UnaryOp::Neg | UnaryOp::BitNot) {
                    self.t_check_width(&v, vt, &format!("{op:?}"))?;
                }
                if T && matches!(op, UnaryOp::Not) {
                    self.t_untype();
                }
                eval_unary(op, v)
            }

            Expr::If { cond, then, else_ } => {
                let cv = self.eval_t::<T>(cond, env)?;
                let ct = self.tl::<T>();
                if ct != 0 {
                    self.t_control_val_only();
                }
                // An `Uncertain<bool>` condition (e.g. `if a > 5` where `a` is
                // Uncertain — the comparison stays Uncertain) branches on its
                // inner bool; confidence is irrelevant to control flow. Unwrap it
                // to the inner value before the bool match.
                let cv = match soft_inner(&cv) {
                    Some(inner) => inner,
                    None => cv,
                };
                match cv {
                    Value::Bool(true) => {
                        let exits = ct != 0
                            && (taint::has_exit(then) || else_.as_ref().is_some_and(|e| taint::has_exit(e)));
                        self.t_branch(ct, exits, || self.eval_t::<T>(then, env))
                    }
                    Value::Bool(false) => {
                        let exits = ct != 0
                            && (taint::has_exit(then) || else_.as_ref().is_some_and(|e| taint::has_exit(e)));
                        match else_ {
                            Some(e) => self.t_branch(ct, exits, || self.eval_t::<T>(e, env)),
                            None => self.t_branch(ct, exits, || Ok(Value::Unit)),
                        }
                    }
                    other => panic(format!(
                        "if condition must be bool, got {}",
                        other.type_name()
                    )),
                }
            }

            Expr::Match { subject, arms } => {
                let v = self.eval_t::<T>(subject, env)?;
                let st = self.tl::<T>();
                if st != 0 {
                    self.t_control_val_only();
                }
                // The taint of every guard that REFUSED an earlier arm: which arm
                // runs after a failed guard is that guard's value too, and so is
                // whether a later guard is evaluated at all.
                let mut lost = 0u8;
                for arm in arms {
                    env.push();
                    if self.match_pattern(&arm.pattern, &v, env, self.ts::<T>(st))? {
                        let mut gt = 0;
                        if let Some(guard) = &arm.guard {
                            let g = self.t_branch(lost, false, || self.eval_t::<T>(guard, env))?;
                            let ok = matches!(g, Value::Bool(true));
                            gt = self.tl::<T>();
                            if !ok {
                                lost |= gt;
                                env.pop();
                                continue;
                            }
                        }
                        let ct = st | gt | lost;
                        let exits = ct != 0
                            && arms
                                .iter()
                                .any(|a| taint::has_exit(&a.body) || a.guard.as_ref().is_some_and(taint::has_exit));
                        let r = self.t_branch(ct, exits, || self.eval_t::<T>(&arm.body, env));
                        env.pop();
                        return r;
                    }
                    env.pop();
                }
                panic("no match arm matched")
            }

            Expr::While { cond, body } => {
                loop {
                    let cv = self.eval_t::<T>(cond, env)?;
                    // An `Uncertain<bool>` condition branches on its inner bool
                    // (confidence is irrelevant to control flow) — same as `if`.
                    let cv = match soft_inner(&cv) {
                        Some(inner) => inner,
                        None => cv,
                    };
                    let ct = self.tl::<T>();
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
                    let exits = ct != 0 && taint::stmts_have_exit(body);
                    match self.t_branch(ct, exits, || self.run_loop_body::<T>(body, env))? {
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
                    let v = self.eval_t::<T>(expr, env)?;
                    let vt = self.tl::<T>();
                    env.push();
                    let matched = self.match_pattern(pattern, &v, env, self.ts::<T>(vt))?;
                    if !matched {
                        env.pop();
                        break;
                    }
                    let exits = vt != 0 && taint::stmts_have_exit(body);
                    let step = self.t_branch(vt, exits, || self.run_loop_body::<T>(body, env));
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
                let s = self.eval_int::<T>(start, env)?;
                let e = self.eval_int::<T>(end, env)?;
                // The loop variable counts out a range; if sealed code sized the
                // range it chose the variable, and the body runs as often as it said.
                let bt = self.ta::<T>();
                let mut i = s;
                loop {
                    let cont = if *inclusive { i <= e } else { i < e };
                    if !cont {
                        break;
                    }
                    env.push();
                    env.define(var.clone(), Value::Int(i), self.ts::<T>(bt) & taint::VAL);
                    let exits = bt != 0 && taint::stmts_have_exit(body);
                    let step = self.t_branch(bt, exits, || self.run_loop_body::<T>(body, env));
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
                    Some(e) => self.eval_t::<T>(e, env)?,
                    None => Value::Unit,
                };
                // The value leaves under the control it was returned from.
                if T {
                    self.taint.ret.set(self.ts::<T>(self.ta::<T>()));
                }
                Err(Flow::Return(v))
            }
            Expr::Break => Err(Flow::Break),
            Expr::Continue => Err(Flow::Continue),

            Expr::Question(inner) => {
                let r = self.eval_t::<T>(inner, env)?;
                if T {
                    self.taint.ret.set(self.ts::<T>(self.tl::<T>()));
                    // `?` is an early exit chosen by the operand's value: what
                    // runs after it is control-dependent on it, taken or not
                    // (the exit `return` has, via `t_branch`'s `exits`).
                    let ct = self.tl::<T>() & taint::VAL;
                    if ct != 0 {
                        let st = &self.taint.sticky;
                        st.set(st.get() | ct);
                    }
                }
                match r {
                    Value::Ok(x) => Ok(*x),
                    Value::Some(x) => Ok(*x),
                    Value::Err(e) => Err(Flow::Return(Value::Err(e))),
                    Value::None => Err(Flow::Return(Value::None)),
                    other => panic(format!(
                        "`?` applied to non-Result/Option ({})",
                        other.type_name()
                    )),
                }
            }

            Expr::Call { callee, args, tier } => {
                // `chan<T>()` lowers to a call whose callee is `chan::<T>`.
                if let Expr::StructLit { name, .. } = callee.as_ref() {
                    // `chan<T>()` lowers to `chan::<T>`; `Chan::new(n)` is the
                    // other spelling of the same thing and is an entry in
                    // BUILTINS, so `axon reference` advertises it.
                    //
                    // Only the first was handled here, so `Chan::new(4)` passed
                    // the checker (it is a known builtin with a signature) and
                    // then panicked "value of type Chan is not callable" — the
                    // callee evaluated to a Chan and the evaluator tried to call
                    // it. A documented builtin that type-checks and dies at
                    // runtime is worse than one that does not exist.
                    //
                    // The capacity argument is accepted and not used, because
                    // this channel grows (spec §4: "growable ring buffer"), the
                    // same as `chan<T>()`. Its BUILTINS doc said "bounded
                    // channel with the given capacity" and now says what it does.
                    if name.starts_with("chan::<") || name == "Chan::new" {
                        let q = Rc::new(RefCell::new(VecDeque::new()));
                        // Stamped with its stated element type, and its
                        // creating side recorded (amendment 72).
                        let elem = name.strip_prefix("chan::<").and_then(|s| s.strip_suffix('>'));
                        self.chan_created(&q, elem);
                        return Ok(Value::Chan(q));
                    }
                    // R13 native FFI: a native `M::fn(...)` call dispatches to the
                    // in-process mock shim (one impl, two engines — I-2).
                    if crate::native::is_native_call(name) {
                        let r = self.eval_native_call(name, args, env);
                        // A native handle names state in a registry every frame
                        // shares: the call is `World` (amendment 108).
                        if T {
                            self.t_native_call();
                        }
                        return r;
                    }
                }
                self.eval_call::<T>(callee, args, tier.as_deref(), env)
            }

            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                let recv = self.eval_t::<T>(receiver, env)?;
                let rt = self.tl::<T>();
                // Channel methods (cooperative, single-threaded): send pushes to
                // the shared queue, recv pops from it, clone shares the handle.
                if let Value::Chan(q) = &recv {
                    if T {
                        self.t_chan_access(&recv, method);
                    }
                    return match method.as_str() {
                        "send" => {
                            let mut v = self.eval_t::<T>(&args[0], env)?;
                            if T && !self.frame_sealed.get() {
                                // The queue now holds what was sent, and whoever
                                // reads it back inherits that (amendment 102).
                                // (A sealed send is marked ALL by `t_chan_access`.)
                                let st = self.ts::<T>(self.tl::<T>());
                                self.t_mark_obj(&recv, st);
                            }
                            // Cast to every element type the channel crossed.
                            self.chan_send_check(q, &mut v)?;
                            // The operator sends sealed code a value: dicts in
                            // it are snapshotted (amendment 72 part 2).
                            if !self.frame_sealed.get() {
                                self.dict_edge_in(&v)?;
                            }
                            q.borrow_mut().push_back(v);
                            Ok(Value::Unit)
                        }
                        "recv" => {
                            q.borrow_mut().pop_front().ok_or_else(|| {
                                Flow::Panic(
                                    "recv on an empty channel — the interpreter runs `spawn` \
                                     bodies eagerly, so a value must be sent before it is received"
                                        .into(),
                                )
                            })
                        }
                        // Non-blocking pop. Returns `Some(v)` when a value is
                        // available, `None` otherwise. Lets ASI loops poll a
                        // channel without panicking on the empty case — useful
                        // for fan-out workers where the consumer races the
                        // producers and needs to know when results have stopped
                        // coming, not just block on the first miss.
                        "try_recv" => {
                            Ok(match q.borrow_mut().pop_front() {
                                Some(v) => Value::Some(Box::new(v)),
                                None => Value::None,
                            })
                        }
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
                    argv.push(self.eval_t::<T>(a, env)?);
                }
                let tn = argv[0].type_name();
                if let Some(f) = self.methods.get(&(tn.clone(), method.clone())) {
                    self.seal_method(f, &tn)?;
                    self.seal_dispatch(receiver, f, &argv[0], &tn)?;
                    self.t_check_dispatch(f, &argv[0], rt, &tn)?;
                    self.call_fn(f, argv)
                } else {
                    // A sealed caller is told what it is told for an operator
                    // method it may not use (`seal_method`): the miss and the
                    // refusal read the same (existence oracle, amendment 108).
                    self.no_such_fn(method, format!("no method `{method}` on type `{tn}`"))
                }
            }

            Expr::FieldAccess { receiver, field } => {
                // A variable receiver (`p.x`, the common case) is read in
                // place: only the field is cloned, not the whole record. The
                // lookup is the `Expr::Ident` arm's, verbatim.
                if let Expr::Ident(name) = receiver.as_ref() {
                    let v = match env.get(name) {
                        Some(v) => v,
                        None => match self.global_ref(name)? {
                            Some(v) => v,
                            None => {
                                return self
                                    .no_such_fn(name, format!("undefined identifier `{name}`"))
                            }
                        },
                    };
                    return field_of(v, field);
                }
                let v = self.eval_t::<T>(receiver, env)?;
                field_of(&v, field)
            }

            Expr::Tuple(elems) => {
                let mut vs = Vec::with_capacity(elems.len());
                for e in elems {
                    vs.push(self.eval_t::<T>(e, env)?);
                }
                Ok(Value::Tuple(vs))
            }

            Expr::Index { receiver, index } => {
                // Phase 13 Slice 2: E[dist] and Var[dist] moment predicates.
                // Intercept before normal array indexing to avoid "undefined E".
                if let Expr::Ident(tag) = receiver.as_ref() {
                    if matches!(tag.as_str(), "E" | "Var") {
                        let dist_val = self.eval_t::<T>(index, env)?;
                        if let Some(moment) = eval_dist_moment(tag, &dist_val) {
                            return Ok(Value::Float(moment));
                        }
                    }
                }
                // `xs[i]` on a variable reads the element in place: evaluating
                // `xs` first would copy the whole array per element read, which
                // makes in-place algorithms over `&mut [T]` (AX-08) quadratic.
                if let Expr::Ident(name) = receiver.as_ref() {
                    if env.get(name).is_some() || self.is_global(name) {
                        let idx = self.eval_int::<T>(index, env)?;
                        let arr = match env.get(name) {
                            Some(v) => Some(v),
                            None => self.global_ref(name)?,
                        };
                        return match arr {
                            Some(Value::Array(items)) => {
                                items.get(idx as usize).cloned().ok_or_else(|| {
                                    Flow::Panic(format!(
                                        "index {idx} out of bounds (len {})",
                                        items.len()
                                    ))
                                })
                            }
                            Some(other) => {
                                panic(format!("indexing non-array ({})", other.type_name()))
                            }
                            None => self.no_such_fn(name, format!("undefined identifier `{name}`")),
                        };
                    }
                }
                let arr = self.eval_t::<T>(receiver, env)?;
                let idx = self.eval_int::<T>(index, env)?;
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
                    out.push(self.eval_t::<T>(e, env)?);
                }
                Ok(Value::Array(Rc::new(out)))
            }

            Expr::StructLit { name, fields } => {
                let mut fmap = HashMap::with_capacity(fields.len());
                for (fname, fexpr) in fields {
                    let fval = self.eval_t::<T>(fexpr, env)?;
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
                    // A field is a declared type: cast to it (amendment 53).
                    let mut fval = fval;
                    if let Err(why) = self.cast_field(name, fname, &mut fval) {
                        return panic(format!(
                            "field `{fname}` of `{name}` was given {} — a runtime type confusion \
                             ({why})",
                            display(&fval)
                        ));
                    }
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
                                        self.seal_refine(rn)?;
                                        if let Some(fv) = fmap.get(&tf.name) {
                                            let mut pe = Env::new();
                                            pe.define("_".into(), fv.clone(), self.ta::<T>());
                                            if let Value::Bool(false) = crate::interp::contain_frame(self.eval_t::<T>(pred, &mut pe), "a predicate")? {
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
                                pe.define("_".into(), sv.clone(), self.ta::<T>());
                                // A DEFINITION-owned predicate runs under its
                                // definition's provenance: a candidate struct's
                                // `where` ran unsealed when the OPERATOR built
                                // one, and called operator code from there (PCI
                                // candidate-4 review, executed).
                                let sealed = self.frame_sealed.get() || self.seal_type(name);
                                let held = self.with_frame(sealed, || {
                                    crate::interp::contain_frame(self.eval_t::<T>(pred, &mut pe), "a predicate")
                                })?;
                                if let Value::Bool(false) = held {
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

            Expr::Ok(e) => Ok(Value::Ok(Box::new(self.eval_t::<T>(e, env)?))),
            Expr::Err(e) => Ok(Value::Err(Box::new(self.eval_t::<T>(e, env)?))),
            Expr::Some(e) => Ok(Value::Some(Box::new(self.eval_t::<T>(e, env)?))),
            Expr::None => Ok(Value::None),

            Expr::FmtStr { parts } => {
                use crate::ast::FmtPart;
                let mut s = String::new();
                for part in parts {
                    match part {
                        FmtPart::Lit(t) => s.push_str(t),
                        FmtPart::Expr(e) => {
                            let v = self.eval_t::<T>(e, env)?;
                            if T {
                                // The text of a value shows the state in it.
                                self.t_touch(self.t_obj_deep(&v));
                            }
                            s.push_str(&display(&v));
                        }
                    }
                }
                // An interpolation yields a `str` whatever it interpolated.
                if T {
                    self.t_untype();
                }
                Ok(Value::Str(Rc::new(s)))
            }

            Expr::Lambda { params, body, .. } => {
                // Its own parameter annotations are its first contract.
                let contract = Self::lambda_contract(params);
                let mut cell = env.snapshot();
                // PCI: a closure remembers that a SEALED frame created it, so it
                // runs sealed wherever it is later called.
                if self.seal.active {
                    cell.insert(
                        crate::interp::PIN_FN_MARK.to_string(),
                        Value::Int(self.pin_fn.get() as i64),
                    );
                }
                if self.frame_sealed.get() {
                    cell.insert(
                        crate::interp::SEALED_CLOSURE_MARK.to_string(),
                        Value::Bool(true),
                    );
                }
                Ok(Value::Closure {
                    params: params.iter().map(|p| p.name.clone()).collect(),
                    body: body.clone(),
                    // T40: a SHARED, persistent capture cell — see Value::Closure.
                    captured: std::rc::Rc::new(std::cell::RefCell::new(cell)),
                    contract,
                })
            }

            Expr::Comptime(inner) => self.eval_t::<T>(inner, env),

            // R17 Slice 1: inline asm is hardware-only — refuses in the interpreter.
            Expr::InlineAsm { .. } => Err(crate::interp::Flow::Panic(
                "E0910: `asm(...)` requires a freestanding codegen build — use `axon build --freestanding`".into(),
            )),

            // Cooperative concurrency: run the spawned body eagerly (single-
            // threaded), so its sends are queued before the main flow continues.
            Expr::Spawn(body) => {
                self.eval_t::<T>(body, env)?;
                Ok(Value::Unit)
            }
            // Cooperative select: fire the first arm whose channel has a ready
            // value (its queue is non-empty), consuming that value and running the
            // arm body. Arms are `c.recv() => body`. With eager `spawn`, channels
            // are pre-filled, so this is deterministic (first ready arm wins).
            Expr::Select(arms) => {
                let mut lost = 0u8;
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
                    let Value::Chan(q) = self.eval_t::<T>(receiver, env)? else {
                        return panic("select arm `recv` on a non-channel");
                    };
                    // Whether this channel is ready is a READ of its queue, ready
                    // or not: an arm skipped because sealed code drained (or never
                    // fed) it is a choice the candidate made (amendment 106).
                    if T {
                        self.t_chan_access(&Value::Chan(q.clone()), "recv");
                    }
                    // Which arm fires is the readiness of every queue looked at
                    // up to it: the arm's body runs under the control taint of
                    // all of them, and so does what runs after it if any arm
                    // can leave the fn (a skipped arm is a branch not taken).
                    if T {
                        lost |= self.t_obj(&Value::Chan(q.clone())) & taint::VAL;
                    }
                    let ready = q.borrow_mut().pop_front();
                    if ready.is_some() {
                        let exits = lost != 0 && arms.iter().any(|a| taint::has_exit(&a.body));
                        return self.t_branch(lost, exits, || self.eval_t::<T>(&arm.body, env));
                    }
                }
                panic("select: no channel was ready (cooperative interpreter — send before select)")
            }
        }
    }

    pub(super) fn eval_block<const T: bool>(&self, stmts: &[Stmt], env: &mut Env) -> R {
        env.push();
        let mut last = Value::Unit;
        let n = stmts.len();
        for (i, stmt) in stmts.iter().enumerate() {
            // What a statement touched does not become the block's value: its
            // effects travel through the bindings and containers it wrote. Only
            // the tail expression is the block's value (amendment 102).
            let before = if T { self.taint.acc.get() } else { 0 };
            match self.eval_t::<T>(&stmt.expr, env) {
                Ok(v) => {
                    last = v;
                    if T && i + 1 < n {
                        self.taint.acc.set(before);
                    }
                }
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
    pub(super) fn run_loop_body<const T: bool>(
        &self,
        body: &[Stmt],
        env: &mut Env,
    ) -> Result<LoopStep, Flow> {
        env.push();
        for stmt in body {
            match self.eval_t::<T>(&stmt.expr, env) {
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

    /// R13 native FFI: dispatch a `M::fn(...)` call to the in-process native
    /// shim. Evaluates args, marshals them across the boundary (extracting
    /// handle payloads — never raw pointers), dispatches to the registered Rust
    /// impl, and wraps the result. A bad/forged/consumed handle returns a
    /// graceful panic (exit 101, I-4), NEVER a host abort. v1 has one module:
    /// the GPU-free `gfx` mock.
    fn eval_native_call(&self, qualified: &str, args: &[Expr], env: &mut Env) -> R {
        let (module, nf) = match crate::native::resolve_call(qualified) {
            Some(pair) => pair,
            None => return panic(format!("call to unknown native function `{qualified}`")),
        };
        // AUDIT T45 (INTERP-H02). This function is reached DIRECTLY from
        // `Expr::Call` (see the `is_native_call` arm above) and returns without
        // ever entering `eval_call`/`call_builtin` — which was the only place
        // the F5 sandbox ceiling, the R4 @[agent] action log and the R28 audit
        // ledger were applied. So every `native::M::*` call bypassed all three.
        //
        // Reproduced with the `gfx` module, which declares `effects: &["IO"]`,
        // under `sandbox_create(p, "")` — an EMPTY ceiling:
        // window_open/surface/clear/present/frame_count ran to completion and
        // frame_count returned 2, proving both present() calls executed. No
        // violation raised, no ledger row, no agent-log entry.
        //
        // Placed here, above the modbus/fhir/fix branch, so the domain modules
        // (`effects: &["Net"]`) are gated by the same call — a second gate at a
        // second site is how this class recurs.
        //
        // The evaluated arguments ARE passed, because the comment that used to
        // sit here — "native arguments are handles and scalars, with no path or
        // host" — was false. `modbus_connect(host, port)` and
        // `fhir_connect(base_url)` take the target as their first parameter,
        // and passing `None` meant `scope_violation` was never consulted, so a
        // sandbox's `net` allowlist did not apply to native dials at all.
        // Build the scope argument from the host LITERAL. The gate runs before
        // argument evaluation on purpose — evaluating them first would let an
        // argument's side effects happen before the check — so a dynamically
        // computed host cannot be read here. That case passes a marker the
        // scope check cannot match, which FAILS CLOSED, mirroring the static
        // checker's own treatment of a non-literal host.
        let scope_host: Option<Vec<Value>> = crate::capabilities::native_net_host_arg(qualified)
            .map(|idx| {
                let mut v = vec![Value::Unit; idx + 1];
                v[idx] = match args.get(idx) {
                    Some(Expr::Literal(crate::ast::Literal::Str(sl))) => {
                        Value::Str(Rc::new(sl.clone()))
                    }
                    _ => Value::Str(Rc::new(String::from("<dynamic>"))),
                };
                v
            });
        self.pre_effect_gate(
            qualified,
            module.effects,
            Some(module.capability),
            native_ledger_kind(module.effects),
            scope_host.as_deref(),
        )?;
        // R22: the domain-interop modules (`modbus`/`fhir`/`fix`) marshal through
        // a richer DomainArg/DomainValue layer (str + [i64] returns), so they
        // take a separate path. `gfx` keeps the original GfxArg path below.
        if matches!(module.name, "modbus" | "fhir" | "fix") {
            #[cfg(not(target_arch = "wasm32"))]
            {
                return self.eval_domain_native_call(module, nf, args, env);
            }
            // On the in-browser wasm target the domain backends (TCP/HTTP) are
            // not available — a clean refusal, never a silent success (I-9).
            #[cfg(target_arch = "wasm32")]
            {
                let _ = (nf, args, env);
                return panic(format!(
                    "native::{} is unavailable on the wasm32 (browser) target",
                    module.name
                ));
            }
        }
        // Evaluate args left-to-right.
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval(a, env)?);
        }
        // Marshal each Value to a GfxArg. A handle is unwrapped to its payload
        // index, and its module/name are verified against the param's expected
        // handle type (runtime defense in depth beneath the static E1802 check).
        let mut margs = Vec::with_capacity(argv.len());
        for (i, v) in argv.iter().enumerate() {
            let expected = nf.params.get(i).map(|(t, _)| t);
            let marshalled = match v {
                Value::Int(n) => crate::native::GfxArg::Int(*n),
                Value::SizedInt { val, .. } => crate::native::GfxArg::Int(*val),
                Value::Float(f) => crate::native::GfxArg::Float(*f),
                Value::Str(s) => crate::native::GfxArg::Str(String::clone(s)),
                Value::Handle {
                    module: hm,
                    name: hn,
                    payload,
                    ..
                } => {
                    // Verify the handle belongs to the expected module+name.
                    if let Some(crate::native::FfiType::Handle {
                        module: em,
                        name: en,
                        ..
                    }) = expected
                    {
                        if hm != em || hn != en {
                            return panic(format!(
                                "native::{} expected handle `{em}::{en}`, found `{hm}::{hn}`",
                                module.name
                            ));
                        }
                    }
                    // Marshal the handle with BOTH its nominal tag (so the
                    // shared dispatcher's trace/arg-hash matches codegen's,
                    // which embeds the same frozen tag) and its slab index.
                    crate::native::GfxArg::Handle {
                        tag: crate::native::tag_for(hn),
                        payload: *payload,
                    }
                }
                other => {
                    return panic(format!(
                        "native::{} arg {i} is not FFI-representable at runtime ({})",
                        module.name,
                        other.type_name()
                    ));
                }
            };
            margs.push(marshalled);
        }
        // Dispatch to the module's backend. v1: `gfx` and the R14 `platform`
        // headless stub (clean off-device refusals, spec §7). For `gfx`, under the
        // `gfx-wgpu` feature this is the REAL wgpu offscreen renderer (R13 slice
        // 5); without it, the GPU-free mock. SAME `dispatch` interface either way —
        // a drop-in backend swap, no surface change. The value-returning probes
        // (`frame_count`/`read_pixel`) return byte-identical values across the two
        // (same packing), so I-2 parity holds.
        let result = match module.name {
            #[cfg(feature = "gfx-wgpu")]
            "gfx" => self.gfx_real.borrow_mut().dispatch(nf.name, &margs),
            #[cfg(not(feature = "gfx-wgpu"))]
            "gfx" => self.gfx_mock.borrow_mut().dispatch(nf.name, &margs),
            "platform" => crate::native::platform_dispatch_headless(nf.name, &margs),
            _ => Err(format!(
                "native module `{}` has no interp backend",
                module.name
            )),
        };
        match result {
            Ok(crate::native::GfxValue::Unit) => Ok(Value::Unit),
            Ok(crate::native::GfxValue::Int(n)) => Ok(Value::Int(n)),
            Ok(crate::native::GfxValue::Handle { name, payload }) => Ok(Value::Handle {
                module: module.name.to_string(),
                name: name.to_string(),
                payload,
                resource: matches!(
                    nf.ret,
                    crate::native::FfiType::Handle { resource: true, .. }
                ),
            }),
            Err(msg) => panic(msg),
        }
    }

    /// R22: dispatch a `modbus::*` / `fhir::*` / `fix::*` call. Marshals
    /// `Value`→`DomainArg` (handle → its slab-index payload, never a raw
    /// pointer), dispatches to the in-process `axon-domain` backend, and wraps
    /// the `DomainValue` result (str, `[i64]`, Int, Unit, Handle). A bad/forged/
    /// consumed handle → a graceful panic (exit 101, I-4), NEVER a host abort.
    /// Gated to non-wasm targets (the domain backends are native-host-only).
    #[cfg(not(target_arch = "wasm32"))]
    fn eval_domain_native_call(
        &self,
        module: &'static crate::native::NativeModule,
        nf: &'static crate::native::NativeFn,
        args: &[Expr],
        env: &mut Env,
    ) -> R {
        use axon_domain::{DomainArg, DomainValue};
        // Evaluate args left-to-right.
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval(a, env)?);
        }
        // Marshal each Value to a DomainArg, verifying handle module/name
        // against the param's expected handle type (runtime defense beneath the
        // static E1802 nominal check).
        let mut margs = Vec::with_capacity(argv.len());
        for (i, v) in argv.iter().enumerate() {
            let expected = nf.params.get(i).map(|(t, _)| t);
            let marshalled = match v {
                Value::Int(n) => DomainArg::Int(*n),
                Value::SizedInt { val, .. } => DomainArg::Int(*val),
                Value::Float(f) => DomainArg::Float(*f),
                Value::Str(s) => DomainArg::Str(String::clone(s)),
                Value::Array(items) => {
                    // Only `[i64]` is representable at the boundary.
                    let mut ints = Vec::with_capacity(items.len());
                    for it in items.iter() {
                        match it {
                            Value::Int(n) => ints.push(*n),
                            Value::SizedInt { val, .. } => ints.push(*val),
                            other => {
                                return panic(format!(
                                    "native::{} arg {i}: only `[i64]` arrays are FFI-representable, found element {}",
                                    module.name,
                                    other.type_name()
                                ));
                            }
                        }
                    }
                    DomainArg::IntArray(ints)
                }
                Value::Handle {
                    module: hm,
                    name: hn,
                    payload,
                    ..
                } => {
                    if let Some(crate::native::FfiType::Handle {
                        module: em,
                        name: en,
                        ..
                    }) = expected
                    {
                        if hm != em || hn != en {
                            return panic(format!(
                                "native::{} expected handle `{em}::{en}`, found `{hm}::{hn}`",
                                module.name
                            ));
                        }
                    }
                    DomainArg::Handle {
                        tag: axon_domain::tag_for(hn),
                        payload: *payload,
                    }
                }
                other => {
                    return panic(format!(
                        "native::{} arg {i} is not FFI-representable at runtime ({})",
                        module.name,
                        other.type_name()
                    ));
                }
            };
            margs.push(marshalled);
        }
        let result = axon_domain::dispatch(module.name, &self.domain, nf.name, &margs);
        match result {
            Ok(DomainValue::Unit) => Ok(Value::Unit),
            Ok(DomainValue::Int(n)) => Ok(Value::Int(n)),
            Ok(DomainValue::Str(s)) => Ok(Value::Str(Rc::new(s))),
            Ok(DomainValue::IntArray(ns)) => Ok(Value::Array(Rc::new(
                ns.into_iter().map(Value::Int).collect(),
            ))),
            Ok(DomainValue::Handle { name, payload }) => Ok(Value::Handle {
                module: module.name.to_string(),
                name: name.to_string(),
                payload,
                resource: matches!(
                    nf.ret,
                    crate::native::FfiType::Handle { resource: true, .. }
                ),
            }),
            Err(msg) => panic(msg),
        }
    }

    pub(super) fn eval_call<const T: bool>(
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

        // AX-08: a call passing `&mut a` moves each borrowed value out of the
        // caller's binding into the callee and back on return (O(1), no copy).
        if args.iter().any(|a| {
            matches!(
                a,
                Expr::UnaryOp {
                    op: UnaryOp::RefMut,
                    ..
                }
            )
        }) {
            return self.eval_call_mut(callee, args, tier, env);
        }

        // Evaluate arguments left-to-right. In a sealed run `ats` holds each
        // argument's own taint, for the sinks that judge one argument.
        let mut argv = Vec::with_capacity(args.len());
        let mut ats: Vec<u8> = Vec::new();
        for a in args {
            argv.push(self.eval_t::<T>(a, env)?);
            if T {
                ats.push(self.tl::<T>());
            }
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
            // 1. A local/captured variable: it is what the name means here,
            // so the call goes through it — a closure is called, anything
            // else is not callable. It never falls through to a builtin or
            // fn of the same NAME: a confused value in a local `square` would
            // otherwise run the operator's own `fn square` (C9 round 4b,
            // PSV-1, amendment 60).
            if T {
                // The callee is read out of the binding without an `eval`: a
                // closure the binding holds under a taint is picked.
                if let Some(c) = env.get(name) {
                    let bt = env.taint_of(name);
                    if bt & taint::VAL != 0 {
                        self.t_note(c, bt);
                    }
                }
            }
            if let Some(c) = env.get(name) {
                let c = c.clone();
                return self.call_local_closure(c, argv);
            }
            // 2. A builtin — skipped for a name already proven not to be one
            //    (see `resolved_callees`), which also caches step 3's lookup.
            let known = self.resolved_callees.borrow().get(name.as_str()).copied();
            let user_fn = match known {
                Some(f) => f,
                None => {
                    // The NAME rule: a name-resolving builtin's name argument.
                    self.seal_name_args(name, args)?;
                    if T {
                        self.t_check_names(name, &ats)?;
                    }
                    if T {
                        self.t_builtin_in(name, &argv);
                    }
                    // The builtin runs under the control taint of its arguments:
                    // a callback it runs once per element of a tainted array (or
                    // entry of a tainted dict) runs a number of times the
                    // candidate chose, and what that callback stores carries it.
                    let _pc = if T {
                        Some(self.t_pc(self.taint.acc.get() & taint::VAL))
                    } else {
                        None
                    };
                    if let Some(v) = self.call_builtin(name, &argv)? {
                        if T {
                            self.t_builtin_out(name, &argv, &v);
                        }
                        return Ok(v);
                    }
                    let f = self.fns.get(name.as_str()).copied();
                    if super::builtins::builtin_dispatch_is_inert(name) {
                        self.resolved_callees.borrow_mut().insert(name.clone(), f);
                    }
                    f
                }
            };
            // 3. A user-defined function.
            if let Some(f) = user_fn {
                return self.call_fn(f, argv);
            }
            // 4. A module-level closure constant.
            if let Some(c @ Value::Closure { .. }) = self.global_ref(name)? {
                let c = c.clone();
                return self.call_closure(c, argv);
            }
            return self.no_such_fn(name, format!("call to unknown function `{name}`"));
        }

        // Callee is an expression that should evaluate to a closure
        // (e.g. `make_adder(1)(2)` or an array element).
        let c = self.eval_t::<T>(callee, env)?;
        self.call_closure(c, argv)
    }

    /// AX-08: `f(.., &mut a, ..)`. The checker (E0605/E0606) guarantees the
    /// callee is a free fn whose matching params are `&mut [T]`, every `&mut`
    /// operand is a whole local, and no other argument mentions it.
    fn eval_call_mut(&self, callee: &Expr, args: &[Expr], tier: Option<&str>, env: &mut Env) -> R {
        let f = match callee {
            Expr::Ident(name) => match self.fns.get(name) {
                Some(f) => *f,
                None => return panic(format!("`&mut` argument passed to `{name}`, which is not a function taking `&mut` parameters")),
            },
            _ => return panic("`&mut` argument passed to a computed callee".to_string()),
        };
        // Plain arguments first, left to right: they cannot mention a
        // borrowed variable (E0606), so this order is unobservable — and if
        // one unwinds (`?`, panic) nothing has been moved out yet.
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(match a {
                Expr::UnaryOp {
                    op: UnaryOp::RefMut,
                    ..
                } => Value::Unit,
                _ => self.eval(a, env)?,
            });
        }
        let mut borrowed: Vec<(usize, &str)> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            if let Expr::UnaryOp {
                op: UnaryOp::RefMut,
                operand,
            } = a
            {
                let Expr::Ident(name) = operand.as_ref() else {
                    return panic("`&mut` of something other than a local variable".to_string());
                };
                let Some(slot) = env.get_mut(name) else {
                    return panic(format!("`&mut {name}`: `{name}` is not a local variable"));
                };
                argv[i] = std::mem::replace(slot, Value::Unit);
                borrowed.push((i, name.as_str()));
            }
        }
        // A borrowed binding's taint goes into the callee with its value.
        if self.seal.active {
            for (_, name) in &borrowed {
                self.t_touch(env.taint_of(name));
            }
        }
        *self.current_call_tier.borrow_mut() = tier.map(|t| t.to_string());
        let (result, mut outs, out_ts) = self.call_fn_mut(f, argv);
        for (i, name) in borrowed {
            if let Some(slot) = env.get_mut(name) {
                *slot = std::mem::replace(&mut outs[i], Value::Unit);
            }
            // What the binding holds now is what the callee left in it: the
            // value, and its taint (a second return channel).
            if self.seal.active {
                env.taint_or(name, out_ts[i]);
            }
        }
        result
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
            sealed: self.frame_sealed.get(),
            operator_frames: self.operator_frames.get(),
            pin_owner: self.pin_fn.get(),
        };
        let depth = self.handlers.borrow().len();
        self.handlers.borrow_mut().push(frame);
        let result = self.eval(body, env);
        let mut vt = self.t_last();
        if matches!(result, Err(Flow::HandlerDone(_, d)) if d == depth) {
            // The arm's value is the block's value: what the arm touched unwound
            // into this expression's accumulator.
            vt = self.taint.acc.get();
        }
        self.handlers.borrow_mut().pop();
        // Phase 6 multi-shot: a non-tail / multi-resume arm reifies the
        // continuation by replay and finishes the WHOLE block with
        // `Flow::HandlerDone(v)` — the original suspended body is abandoned, so
        // catch it here and use `v` as the block value (the single-shot tail path
        // never raises this, so its behavior is unchanged).
        let mut value = match result {
            Ok(v) => v,
            Err(Flow::HandlerDone(v, d)) if d == depth => v,
            Err(e) => return Err(e),
        };

        // An inline `return(v) => e` arm rewrites the body's final value.
        if let Some(ra) = return_arm {
            let mut ra_env = Env::from_snapshot(captured);
            ra_env.push();
            self.match_pattern(&ra.binding, &value, &mut ra_env, vt)?;
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
            sealed: ctx.sealed,
            operator_frames: self.operator_frames.get(),
        });
        let mut body_env = Env::from_snapshot(ctx.env_snapshot.clone());
        // The replayed body is the installing fn's own text (amendment 100).
        let _pin_guard = crate::interp::PinGuard {
            cell: &self.pin_fn,
            prev: self.pin_fn.replace(ctx.pin_owner),
        };
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
            // A sealed frame is skipped (the search goes on outward) when the
            // operation is the operator's: see `handler_may_answer` (PSV-1).
            let eligible = |f: &&crate::interp::HandlerFrame| {
                self.handler_may_answer(f.sealed, f.operator_frames)
            };
            stack
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, f)| eligible(f))
                .find_map(|(i, frame)| {
                    frame.arms.iter().find(|a| a.effect == eff).map(|a| {
                        (
                            i,
                            a.binding.clone(),
                            a.body.clone(),
                            a.captured.clone(),
                            frame.body.clone(),
                            frame.env_snapshot.clone(),
                            frame.sealed,
                            frame.pin_owner,
                        )
                    })
                })
        };
        let Some((idx, binding, arm_body, captured, with_body, with_env, arm_sealed, pin_owner)) =
            hit
        else {
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
        // The operation's arguments are the arm's payload, with their taint.
        let pt = self.t_stored(self.taint.acc.get());

        // BARE TAIL-RESUME FAST PATH (single-shot): when the arm body is exactly
        // `resume(<expr>)`, the operation yields that value and the suspended body
        // continues in place — no continuation reification needed. This is the
        // ONLY shape native codegen lowers, and the path the parity harness pins,
        // so it must stay byte-identical: evaluate the arm and propagate the
        // `Flow::Resume(v)` it raises directly (no resume_ctx, no replay).
        if crate::effects::arm_is_bare_tail_resume(&arm_body) {
            let mut arm_env = Env::from_snapshot(captured);
            arm_env.push();
            let bound = self.match_pattern(&binding, &payload, &mut arm_env, pt);
            // The arm runs under the provenance of the `with` that installed it,
            // and under the pin owner of the fn that installed it (amendment 100).
            self.handler_edge_into(arm_sealed, &payload)?;
            let _pin_guard = crate::interp::PinGuard {
                cell: &self.pin_fn,
                prev: self.pin_fn.replace(pin_owner),
            };
            let outcome = self.with_frame(arm_sealed, || {
                crate::interp::contain_loop_control(
                    bound.and_then(|_| self.eval(&arm_body, &mut arm_env)),
                    "an effect-handler arm",
                )
            });
            self.handlers.borrow_mut().extend(suspended);
            return match outcome {
                Err(Flow::Resume(v)) => {
                    self.handler_edge_back(arm_sealed, &v)?;
                    Ok(Some(v))
                }
                Ok(v) => {
                    self.taint.ret.set(self.t_last());
                    Err(Flow::Return(v))
                }
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
            sealed: arm_sealed,
            pin_owner,
        });
        let mut arm_env = Env::from_snapshot(captured);
        arm_env.push();
        let bound = self.match_pattern(&binding, &payload, &mut arm_env, pt);
        self.handler_edge_into(arm_sealed, &payload)?;
        let _pin_guard = crate::interp::PinGuard {
            cell: &self.pin_fn,
            prev: self.pin_fn.replace(pin_owner),
        };
        let outcome = self.with_frame(arm_sealed, || {
            crate::interp::contain_loop_control(
                bound.and_then(|_| self.eval(&arm_body, &mut arm_env)),
                "an effect-handler arm",
            )
        });
        self.resume_ctx.borrow_mut().pop();
        self.handlers.borrow_mut().extend(suspended);

        match outcome {
            // The arm ran to completion. Whether it resumed zero times (abort) or
            // one-or-more times (each `resume` was a replay returning a value),
            // the arm's final value `v` is the value of the WHOLE `with` block.
            // The original suspended body is abandoned (the continuation was
            // reified via replay), so finish the block with HandlerDone — caught
            // by `eval_with_handler`.
            Ok(v) => Err(Flow::HandlerDone(v, idx)),
            // A stray tail `resume` that escaped without a ctx (shouldn't happen
            // on this path, but be safe): treat as a single-shot resume.
            Err(Flow::Resume(v)) => Ok(Some(v)),
            // E1314 and any other control flow (panic, exit, …) propagate.
            Err(other) => Err(other),
        }
    }

    /// `&&` / `||` in a sealed run, the left operand `lv` already evaluated. The
    /// right operand runs only for one value of the left: a selection like
    /// `if`'s, so it runs under the left's control taint, and a right operand
    /// that can leave the fn or loop keeps everything after it control-dependent
    /// too, taken or not (the sixth member of the "operator-side control flow"
    /// class, round 13, amendment 114). Kept out of `eval_binop` so the ordinary
    /// run's `&&` is the code it always was.
    #[cold]
    #[inline(never)]
    fn short_circuit_tainted(&self, op: &BinOp, lv: Value, right: &Expr, env: &mut Env) -> R {
        let and = matches!(op, BinOp::And);
        let ct = self.tl::<true>();
        if ct != 0 {
            self.t_control_val_only();
        }
        let exits = ct != 0 && taint::has_exit(right);
        // The value of the left that SKIPS the right operand.
        if let Value::Bool(b) = lv {
            if b != and {
                // A branch not taken: when it could exit, whether it ran is still
                // the left's bit.
                if exits {
                    self.t_branch(ct, exits, || Ok(()))?;
                }
                return Ok(Value::Bool(b));
            }
            return self.t_branch(ct, exits, || match self.eval_t::<true>(right, env)? {
                Value::Bool(r) => Ok(Value::Bool(r)),
                other if uncertain_parts(&other).is_some() => eval_binop_vals(op, lv, other),
                other => panic(format!(
                    "`{}` rhs must be bool, got {}",
                    if and { "&&" } else { "||" },
                    other.type_name()
                )),
            });
        }
        // lv is Uncertain (or other) — value-level path handles/errors.
        let rv = self.t_branch(ct, exits, || self.eval_t::<true>(right, env))?;
        eval_binop_vals(op, lv, rv)
    }

    pub(super) fn eval_binop<const T: bool>(
        &self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        env: &mut Env,
    ) -> R {
        // Short-circuit boolean operators. An `Uncertain<bool>` operand can't
        // short-circuit (we must combine confidences), so it falls through to
        // the value-level path which propagates uncertainty.
        match op {
            BinOp::And => {
                let lv = self.eval_t::<T>(left, env)?;
                if T {
                    return self.short_circuit_tainted(op, lv, right, env);
                }
                if let Value::Bool(false) = lv {
                    return Ok(Value::Bool(false));
                }
                if let Value::Bool(true) = lv {
                    return match self.eval_t::<T>(right, env)? {
                        Value::Bool(b) => Ok(Value::Bool(b)),
                        other if uncertain_parts(&other).is_some() => {
                            eval_binop_vals(op, lv, other)
                        }
                        other => panic(format!("`&&` rhs must be bool, got {}", other.type_name())),
                    };
                }
                // lv is Uncertain (or other) — value-level path handles/errors.
                let rv = self.eval_t::<T>(right, env)?;
                return eval_binop_vals(op, lv, rv);
            }
            BinOp::Or => {
                let lv = self.eval_t::<T>(left, env)?;
                if T {
                    return self.short_circuit_tainted(op, lv, right, env);
                }
                if let Value::Bool(true) = lv {
                    return Ok(Value::Bool(true));
                }
                if let Value::Bool(false) = lv {
                    return match self.eval_t::<T>(right, env)? {
                        Value::Bool(b) => Ok(Value::Bool(b)),
                        other if uncertain_parts(&other).is_some() => {
                            eval_binop_vals(op, lv, other)
                        }
                        other => panic(format!("`||` rhs must be bool, got {}", other.type_name())),
                    };
                }
                let rv = self.eval_t::<T>(right, env)?;
                return eval_binop_vals(op, lv, rv);
            }
            _ => {}
        }

        let l = self.eval_t::<T>(left, env)?;
        let lt = self.tl::<T>();
        let r = self.eval_t::<T>(right, env)?;
        let rt = self.tl::<T>();
        self.seal_width(op, left, right, &l, &r)?;
        // A comparison READS the content of every shared object (dict, channel)
        // inside either operand, however deep (`d == e`, `[d] == [e]`, a struct
        // or tuple holding one): the answer is a function of what sealed code
        // wrote there (amendment 108).
        if T && matches!(
            op,
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq
        ) {
            self.t_touch(self.t_obj_deep(&l) | self.t_obj_deep(&r));
        }
        if T && (lt | rt) & taint::TYP != 0
            && !matches!(
                op,
                BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq
            )
        {
            if lt & taint::TYP != 0 {
                self.t_check_width(&l, lt, &format!("{op:?}"))?;
            }
            if rt & taint::TYP != 0 {
                self.t_check_width(&r, rt, &format!("{op:?}"))?;
            }
        }
        eval_binop_vals(op, l, r)
    }

    /// AX-31: `x = arr_push(x, v)`, `x = arr_concat(x, ys)` and `x = x + y` on a
    /// str or array local append to `x`'s buffer instead of rebuilding it.
    ///
    /// Evaluated the ordinary way, the right-hand side first reads `x` — a
    /// refcount bump — so the builtin or `+` sees a shared buffer and copies
    /// all of it, which makes a builder loop quadratic. Here the other operand
    /// is evaluated first and `x`'s binding is then appended to through
    /// `Rc::make_mut`: in place when the binding is the only owner, copied
    /// first (the old cost) when another binding, element or capture still
    /// holds the old value — which therefore never sees the append.
    ///
    /// Evaluating the operand before reading `x` is unobservable: the operand
    /// cannot mention `x` (checked syntactically), and no code it runs can
    /// reach a local binding — closures, fns, handler arms and continuation
    /// replays run on their own envs. `arr_push`/`arr_concat` are pure
    /// builtins (empty effect row, no capability), so not going through
    /// `call_builtin` skips no gate, audit row or handler. Returns `Ok(false)`,
    /// having evaluated nothing, for any other statement shape or when `x` is
    /// not a str/array local.
    fn assign_in_place<const T: bool>(
        &self,
        name: &str,
        value: &Expr,
        env: &mut Env,
    ) -> Result<bool, Flow> {
        let (op, operand, call) = match value {
            Expr::BinOp {
                op: BinOp::Add,
                left,
                right,
            } => match left.as_ref() {
                Expr::Ident(x) if x == name => (AppendOp::Add, right.as_ref(), None),
                _ => return Ok(false),
            },
            Expr::Call { callee, args, tier } => {
                let Expr::Ident(f) = callee.as_ref() else {
                    return Ok(false);
                };
                let op = match f.as_str() {
                    "arr_push" => AppendOp::Push,
                    "arr_concat" => AppendOp::Concat,
                    _ => return Ok(false),
                };
                match args.as_slice() {
                    [Expr::Ident(x), operand] if x == name => (op, operand, Some((f, tier))),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        if !matches!(env.get(name), Some(Value::Str(_) | Value::Array(_)))
            || mentions_var(operand, name)
        {
            return Ok(false);
        }
        if let Some((f, _)) = call {
            // `eval_call` would run a local closure of that name instead, or take
            // the `&mut` path; the operand must not be able to change which.
            if matches!(env.get(f), Some(Value::Closure { .. }))
                || matches!(
                    operand,
                    Expr::UnaryOp {
                        op: UnaryOp::RefMut,
                        ..
                    }
                )
                || mentions_var(operand, f)
            {
                return Ok(false);
            }
        }
        let y = self.eval_t::<T>(operand, env)?;
        // The append puts `y` into the container `name` holds.
        if T {
            let yt = self.ts::<T>(self.tl::<T>());
            env.taint_or(name, yt);
        }
        if let Some((_, tier)) = call {
            *self.current_call_tier.borrow_mut() = tier.clone();
        }
        let Some(slot) = env.get_mut(name) else {
            unreachable!("the operand does not mention `{name}`, so it is still bound")
        };
        match (op, slot, y) {
            (AppendOp::Push, Value::Array(xs), y) => Rc::make_mut(xs).push(y),
            (AppendOp::Concat | AppendOp::Add, Value::Array(xs), Value::Array(ys)) => {
                Rc::make_mut(xs).extend(ys.iter().cloned())
            }
            (AppendOp::Add, Value::Str(s), Value::Str(t)) => Rc::make_mut(s).push_str(&t),
            // An operand of another type (`Uncertain`, a type error): the
            // ordinary evaluation, on the operands already evaluated.
            (op, slot, y) => {
                let x = slot.clone();
                *slot = match (op, call) {
                    (AppendOp::Push | AppendOp::Concat, Some((f, _))) => self
                        .call_builtin(f, &[x, y])?
                        .expect("arr_push/arr_concat are builtins"),
                    _ => eval_binop_vals(&BinOp::Add, x, y)?,
                };
            }
        }
        Ok(true)
    }

    pub(super) fn eval_int<const T: bool>(&self, expr: &Expr, env: &mut Env) -> Result<i64, Flow> {
        match self.eval_t::<T>(expr, env)? {
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
        t: u8,
    ) -> Result<bool, Flow> {
        match pat {
            Pattern::Wildcard => Ok(true),
            Pattern::Ident(name) => {
                env.define(name.clone(), val.clone(), t);
                Ok(true)
            }
            Pattern::Literal(lit) => Ok(values_equal(&lit_to_val(lit), val)),
            Pattern::Some(inner) => match val {
                Value::Some(v) => self.match_pattern(inner, v, env, t),
                _ => Ok(false),
            },
            Pattern::None => Ok(matches!(val, Value::None)),
            Pattern::Ok(inner) => match val {
                Value::Ok(v) => self.match_pattern(inner, v, env, t),
                _ => Ok(false),
            },
            Pattern::Err(inner) => match val {
                Value::Err(v) => self.match_pattern(inner, v, env, t),
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
                    if !self.match_pattern(fpat, &fval, env, t)? {
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
                    if !self.match_pattern(p, &v, env, t)? {
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

/// `v.field` for a struct/enum field or a tuple's `.N`: clones only the
/// selected component.
fn field_of(v: &Value, field: &str) -> R {
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
            for v in items.iter() {
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
