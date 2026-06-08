//! The core tree-walking evaluator for the interpreter (R0 slice 5 —
//! extracted verbatim from `interp.rs`, zero behavior change). `eval`
//! (the 33-arm expression dispatch), `eval_block`, `eval_call`, `eval_binop`,
//! and `match_pattern`. These are `impl Interp` methods moved into a second
//! inherent-impl block; they reach the parent's `call_builtin`/`call_fn`/
//! `call_closure`/`flatten_place` across split impl blocks, and `use super::*`
//! pulls in Expr/Stmt/Pattern/BinOp/Value/Flow/Env/R + the free helpers.

use super::*;

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
                let mut slot = env
                    .get_mut(&base)
                    .ok_or_else(|| Flow::Panic(format!("assignment to undefined variable `{base}`")))?;
                let (last, prefix) = steps.split_last().ok_or_else(|| Flow::Panic("invalid assignment target".into()))?;
                for step in prefix {
                    slot = match (step, slot) {
                        (PlaceStep::Field(f), Value::Struct { fields, .. }) => fields
                            .get_mut(f)
                            .ok_or_else(|| Flow::Panic(format!("no field `{f}`")))?,
                        (PlaceStep::Index(i), Value::Array(items)) => {
                            let n = items.len();
                            items
                                .get_mut(*i)
                                .ok_or_else(|| Flow::Panic(format!("index {i} out of bounds (len {n})")))?
                        }
                        (_, other) => {
                            return panic(format!("cannot index/field-assign into {}", other.type_name()));
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
                        return panic(format!("cannot index/field-assign into {}", other.type_name()));
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
                    other => panic(format!("if condition must be bool, got {}", other.type_name())),
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

            Expr::WhileLet { pattern, expr, body } => {
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

            Expr::For { var, start, end, inclusive, body } => {
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
                other => panic(format!("`?` applied to non-Result/Option ({})", other.type_name())),
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

            Expr::MethodCall { receiver, method, args } => {
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
                            Flow::Panic(format!("tuple access expects a numeric index, got `.{field}`"))
                        })?;
                        items.get(i).cloned().ok_or_else(|| {
                            Flow::Panic(format!("tuple index {i} out of bounds (len {})", items.len()))
                        })
                    }
                    other => panic(format!("field access on non-struct ({})", other.type_name())),
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
                let arr = self.eval(receiver, env)?;
                let idx = self.eval_int(index, env)?;
                match arr {
                    Value::Array(items) => items
                        .get(idx as usize)
                        .cloned()
                        .ok_or_else(|| Flow::Panic(format!("index {idx} out of bounds (len {})", items.len()))),
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
                    fmap.insert(fname.clone(), self.eval(fexpr, env)?);
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
                                    if let Some(pred) = self.refine_preds.get(rn.as_str()).copied() {
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
                                let sv = Value::Struct { name: name.clone(), fields: fmap };
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
                            return Ok(Value::Struct { name: name.clone(), fields: fmap });
                        }
                    }
                    Ok(Value::Struct { name: name.clone(), fields: fmap })
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
                    let Expr::MethodCall { receiver, method, .. } = &arm.recv else {
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

    pub(super) fn eval_call(&self, callee: &Expr, args: &[Expr], tier: Option<&str>, env: &mut Env) -> R {
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
        };
        self.handlers.borrow_mut().push(frame);
        let result = self.eval(body, env);
        self.handlers.borrow_mut().pop();
        let mut value = result?;

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
    pub(super) fn run_handler_arm(&self, eff: &str, payload: Value) -> Result<Option<Value>, Flow> {
        // Find the INDEX of the nearest frame with a matching arm (search from
        // the top/innermost). Clone the arm data so we don't hold the RefCell
        // borrow across arm evaluation.
        let hit = {
            let stack = self.handlers.borrow();
            stack.iter().enumerate().rev().find_map(|(i, frame)| {
                frame
                    .arms
                    .iter()
                    .find(|a| a.effect == eff)
                    .map(|a| (i, a.binding.clone(), a.body.clone(), a.captured.clone()))
            })
        };
        let Some((idx, binding, body, captured)) = hit else {
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

        let mut arm_env = Env::from_snapshot(captured);
        arm_env.push();
        let bound = self.match_pattern(&binding, &payload, &mut arm_env);
        let outcome = bound.and_then(|_| self.eval(&body, &mut arm_env));

        // Restore the suspended frames (the body continues under them on resume).
        self.handlers.borrow_mut().extend(suspended);

        match outcome {
            // The arm called `resume(v)` — v is the operation's result.
            Err(Flow::Resume(v)) => Ok(Some(v)),
            // The arm completed WITHOUT resuming — its value replaces the entire
            // `with` block (handle-and-abort). Unwind to the `with` frame.
            Ok(v) => Err(Flow::Return(v)),
            // Any other control flow (panic, exit, …) propagates.
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
                        other if uncertain_parts(&other).is_some() => eval_binop_vals(op, lv, other),
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
                        other if uncertain_parts(&other).is_some() => eval_binop_vals(op, lv, other),
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
    pub(super) fn match_pattern(&self, pat: &Pattern, val: &Value, env: &mut Env) -> Result<bool, Flow> {
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
                        Value::Enum { enum_name: en, variant: v, fields }
                            if en == enum_name && v == variant =>
                        {
                            fields
                        }
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
                let Value::Tuple(items) = val else { return Ok(false) };
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

}
