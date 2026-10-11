//! The core tree-walking evaluator for the interpreter (R0 slice 5 —
//! extracted verbatim from `interp.rs`, zero behavior change). `eval`
//! (the 33-arm expression dispatch), `eval_block`, `eval_call`, `eval_binop`,
//! and `match_pattern`. These are `impl Interp` methods moved into a second
//! inherent-impl block; they reach the parent's `call_builtin`/`call_fn`/
//! `call_closure`/`flatten_place` across split impl blocks, and `use super::*`
//! pulls in Expr/Stmt/Pattern/BinOp/Value/Flow/Env/R + the free helpers.

use super::*;

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

/// An evaluated operand of a binary operation (AX-55): an int or a float as
/// the scalar itself, so the int/float fast path never builds or drops a
/// `Value`; any other value as is.
pub(super) enum Operand {
    Int(i64),
    Float(f64),
    Val(Value),
}

impl Operand {
    #[inline(always)]
    pub(super) fn of(v: Value) -> Operand {
        match v {
            Value::Int(n) => Operand::Int(n),
            Value::Float(f) => Operand::Float(f),
            v => Operand::Val(v),
        }
    }

    /// The operand a local's current value `v` gives, copied out of the slot
    /// for an int or a float.
    #[inline(always)]
    pub(super) fn of_ref(v: &Value) -> Operand {
        match v {
            Value::Int(n) => Operand::Int(*n),
            Value::Float(f) => Operand::Float(*f),
            v => Operand::Val(v.clone()),
        }
    }

    #[inline(always)]
    fn into_value(self) -> Value {
        match self {
            Operand::Int(n) => Value::Int(n),
            Operand::Float(f) => Value::Float(f),
            Operand::Val(v) => v,
        }
    }
}

/// `l op r` for every operator but `&&`/`||`, on evaluated operands. AX-46: two
/// plain ints or floats (the hot case) skip the general dispatch; same
/// helpers, so the same semantics, as `eval_binop_vals`. Shared by
/// `eval_binop` and the bytecode engine (R50).
#[inline(always)]
pub(super) fn binop_operands(op: &BinOp, l: Operand, r: Operand) -> R {
    let fast = match (&l, &r) {
        (Operand::Int(a), Operand::Int(b)) => int_binop(op, *a, *b),
        (Operand::Float(a), Operand::Float(b)) => float_binop(op, *a, *b),
        _ => None,
    };
    match fast {
        Some(res) => res,
        None => eval_binop_vals(op, l.into_value(), r.into_value()),
    }
}

/// Whether `&&`/`||` (`op`) is decided by its left operand `lv` alone:
/// `false && _` and `true || _`. An `Uncertain<bool>` operand never
/// short-circuits (the confidences must combine). Shared by `eval_binop` and
/// the bytecode engine (R50).
#[inline(always)]
pub(super) fn short_circuits(op: &BinOp, lv: &Value) -> bool {
    matches!(
        (op, lv),
        (BinOp::And, Value::Bool(false)) | (BinOp::Or, Value::Bool(true))
    )
}

/// `lv && rv` / `lv || rv` once [`short_circuits`] said the right operand
/// runs. A plain-bool left operand needs a bool (or an `Uncertain`) right
/// operand; any other left operand (an `Uncertain<bool>`) takes the
/// value-level path, which propagates uncertainty or errs.
pub(super) fn logic_rhs(op: &BinOp, lv: Value, rv: Value) -> R {
    if let Value::Bool(_) = lv {
        return match rv {
            Value::Bool(b) => Ok(Value::Bool(b)),
            other if uncertain_parts(&other).is_some() => eval_binop_vals(op, lv, other),
            other => {
                let sym = if matches!(op, BinOp::And) { "&&" } else { "||" };
                panic(format!(
                    "`{sym}` rhs must be bool, got {}",
                    other.type_name()
                ))
            }
        };
    }
    eval_binop_vals(op, lv, rv)
}

/// An `if` or `while` condition (`what` is `"if"` or `"while"`): an
/// `Uncertain<bool>` or `Temporal<bool>` branches on its inner bool
/// (confidence is irrelevant to control flow); any other non-bool panics.
/// Not used for match guards, which are true only for a plain `true`. Shared
/// by the `If`/`While` arms and the bytecode engine (R50).
#[inline(always)]
pub(super) fn cond_bool(v: Value, what: &str) -> Result<bool, Flow> {
    match v {
        Value::Bool(b) => Ok(b),
        v => cond_bool_slow(v, what),
    }
}

/// [`cond_bool`] for anything but a plain bool (`soft_inner` returns `None`
/// for a bool, so testing it first changes nothing).
#[cold]
#[inline(never)]
fn cond_bool_slow(v: Value, what: &str) -> Result<bool, Flow> {
    let v = match soft_inner(&v) {
        Some(inner) => inner,
        None => v,
    };
    match v {
        Value::Bool(b) => Ok(b),
        other => panic(format!(
            "{what} condition must be bool, got {}",
            other.type_name()
        )),
    }
}

/// A `for` bound or an index read: an `Int` only; anything else, a sized int
/// included, panics. Shared by the `For`/`Index` arms and the bytecode engine
/// (R50).
#[inline(always)]
pub(super) fn strict_int(v: Value) -> Result<i64, Flow> {
    match v {
        Value::Int(n) => Ok(n),
        other => strict_int_panic(other),
    }
}

#[cold]
#[inline(never)]
fn strict_int_panic(other: Value) -> Result<i64, Flow> {
    panic(format!("expected i64, got {}", other.type_name()))
}

/// `v?`: the payload of an `Ok`/`Some`; an `Err`/`None` returns from the fn;
/// anything else panics. Shared by the `Question` arm and the bytecode engine
/// (R50).
pub(super) fn question(v: Value) -> R {
    match v {
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

/// AX-25: the closure value of the top-level fn `name` (arity `arity`): the
/// capture-free forwarding lambda `|#0, #1, ..| name(#0, #1, ..)`. The `#n`
/// parameter names cannot be written in source, so they never shadow a name the
/// callee's body or arguments could refer to.
///
/// `sealed`: a SEALED frame took the value (veto eval.rs:95-118). It carries
/// the `sealed_fnval` mark: a call of it from sealed code is the candidate's
/// own, not a call of an operator closure. It is NOT `sealed_origin`: the
/// body still runs in the operator frame and reaches the candidate fn through
/// the call edge. The operator's fn values stay unmarked.
fn fn_value(name: &str, arity: usize, sealed: bool) -> Value {
    let params: Vec<String> = (0..arity).map(|i| format!("#{i}")).collect();
    let args = params.iter().map(|p| Expr::Ident(p.clone())).collect();
    let mut cv = ClosureVal::plain(
        Rc::new(ClosureCode::unresolved(
            params.iter().map(|p| intern(p)).collect(),
            Expr::Call {
                callee: Box::new(Expr::Ident(name.to_string())),
                args,
                tier: None,
            },
        )),
        Captures::default(),
    );
    cv.seal.sealed_fnval = sealed;
    Value::Closure(Rc::new(cv))
}

impl<'p> Interp<'p> {
    // ── Core evaluator ───────────────────────────────────────────────────────

    /// Evaluate `expr`: the plain evaluator, or in a SEALED run the one wrapped
    /// by the taint accumulator (`interp/taint.rs`, R51 S2). This is the entry
    /// for code that does not know which run it is in; the evaluator's own
    /// recursion is generic over the run (`eval_t::<T>`) and never branches
    /// here. NOT a function pointer: the wasm Asyncify guard (R15) refuses a
    /// module in which a fn that can suspend is address-taken.
    #[inline]
    pub(super) fn eval(&self, expr: &Expr, env: &mut Env) -> R {
        if self.seal.active {
            self.eval_tainted(expr, env)
        } else {
            self.eval_arm::<false>(expr, env)
        }
    }

    /// [`Interp::eval`] resolved at compile time: the evaluator's own
    /// recursion knows whether it is the sealed run's (`T`) and calls the
    /// right one directly, so the unsealed instantiation carries no taint code.
    #[inline(always)]
    pub(super) fn eval_t<const T: bool>(&self, expr: &Expr, env: &mut Env) -> R {
        if T {
            self.eval_tainted(expr, env)
        } else {
            self.eval_arm::<false>(expr, env)
        }
    }

    /// The 33-arm expression dispatch, instantiated once per run kind: `T` is
    /// the sealed run's (entered only through [`Interp::eval_tainted`]).
    pub(super) fn eval_arm<const T: bool>(&self, expr: &Expr, env: &mut Env) -> R {
        nest_guard!(self, EVAL);
        match expr {
            // AX-55: int, float and bool literals are built in place (no call,
            // no table probe). A string or decimal literal's value owns an
            // `Rc`, made once at resolution (AX-47); an evaluation clones the
            // `Rc`. A node outside the table (an AST clone made at run time)
            // builds it.
            Expr::Literal(Literal::Int(n)) => Ok(Value::Int(*n)),
            Expr::Literal(Literal::Float(f)) => Ok(Value::Float(*f)),
            Expr::Literal(Literal::Bool(b)) => Ok(Value::Bool(*b)),
            Expr::Literal(lit) => Ok(match self.res.lit(expr) {
                Some(v) => v.clone(),
                None => lit_to_val(lit),
            }),

            Expr::Ident(name) => {
                let (s, slot) = self.res.var(expr, name);
                match env.get_var(s, slot) {
                    Some(v) => Ok(v.clone()),
                    None => self.ident_unbound(name, s),
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
                // A fresh binding carries the VALUE's taint (the evaluator
                // already added the taint of any early exit before it).
                // Control taints what is WRITTEN to a binding that outlives
                // the branch (an assignment).
                let vt = self.tl::<T>();
                let (s, slot) = self.res.var(expr, name);
                self.bind_let::<T>(name, s, slot, ty.as_ref(), v, vt, env)?;
                Ok(Value::Unit)
            }

            Expr::Assign { name, value } => {
                let (s, slot) = self.res.var(expr, name);
                if self.assign_in_place::<T>(s, slot, name, value, env)? {
                    return Ok(Value::Unit);
                }
                let v = self.eval_t::<T>(value, env)?;
                // An operator-typed value stored under sealed code's control is
                // a pick (veto eval.rs:269-272).
                let vt = if T {
                    self.t_stored_v(self.tl::<T>(), &v)
                } else {
                    0
                };
                if env.assign_var_t::<T>(s, slot, v, vt) {
                    Ok(Value::Unit)
                } else {
                    panic(format!("assignment to undefined variable `{name}`"))
                }
            }

            // Place assignment: `<place> = v`, where `place` is a (possibly
            // nested) chain of index / field accesses rooted at a variable —
            // e.g. `xs[i] = v`, `s.field = v`, `grid[i][j] = v`, `cfg.row[i] = v`.
            // Phase 1 flattens the place to (base ident, steps), evaluating index
            // expressions; phase 2 (`write_place`) walks the binding mutably and
            // sets the leaf.
            Expr::AssignTo { place, value } => {
                let v = self.eval_t::<T>(value, env)?;
                let ((base, base_slot), steps) = self.flatten_place(place, env)?;
                // The write puts the value (and the indices it was written at)
                // into the container the binding holds: the binding now
                // carries it. Read through the lookup `write_place` uses.
                if T {
                    let wt = self.t_stored_v(self.ta::<T>(), &v);
                    env.get_var_mut_t::<T>(base, base_slot, wt);
                }
                write_place(base, base_slot, &steps, v, env)?;
                Ok(Value::Unit)
            }

            Expr::BinOp { op, left, right } => {
                if !T {
                    return self.eval_binop::<T>(op, left, right, env);
                }
                let v = self.eval_binop::<T>(op, left, right, env)?;
                // A comparison or a logical operator yields a `bool` whoever
                // its operands were: the OPERATOR's operator chose that type.
                if matches!(
                    op,
                    BinOp::Eq
                        | BinOp::NotEq
                        | BinOp::Lt
                        | BinOp::Gt
                        | BinOp::LtEq
                        | BinOp::GtEq
                        | BinOp::And
                        | BinOp::Or
                ) {
                    self.t_untype();
                }
                Ok(v)
            }

            Expr::UnaryOp { op, operand } => {
                let v = self.eval_t::<T>(operand, env)?;
                if T {
                    let vt = self.tl::<T>();
                    self.seal_width_unary(op, operand, &v)?;
                    if vt & taint::TYP != 0 && matches!(op, UnaryOp::Neg | UnaryOp::BitNot) {
                        self.t_check_width(&v, vt, &format!("{op:?}"))?;
                    }
                }
                if T && matches!(op, UnaryOp::Not) {
                    self.t_untype();
                }
                eval_unary(op, v)
            }

            Expr::If { cond, then, else_ } => {
                // An `Uncertain<bool>` condition (e.g. `if a > 5` where `a` is
                // Uncertain — the comparison stays Uncertain) branches on its
                // inner bool; confidence is irrelevant to control flow.
                let cv = self.eval_t::<T>(cond, env)?;
                // Which branch runs says which of the operator's own
                // alternatives was taken: what the branch STORES carries the
                // condition's taint, and so does whatever runs after it if the
                // branch can leave early (`t_branch`).
                let ct = self.tl::<T>();
                if T && ct != 0 {
                    self.t_control_val_only();
                }
                let exits = T
                    && ct != 0
                    && (taint::has_exit(then) || else_.as_ref().is_some_and(|e| taint::has_exit(e)));
                // An operator-typed value the branch yields was picked by the
                // condition (veto eval.rs:400-415).
                let r = if cond_bool(cv, "if")? {
                    self.t_branch(ct, exits, || self.eval_t::<T>(then, env))
                } else {
                    match else_ {
                        Some(e) => self.t_branch(ct, exits, || self.eval_t::<T>(e, env)),
                        None => self.t_branch(ct, exits, || Ok(Value::Unit)),
                    }
                };
                if T {
                    self.t_pick_branch(ct, &r);
                }
                r
            }

            Expr::Match { subject, arms } => {
                let v = self.eval_t::<T>(subject, env)?;
                let st = self.tl::<T>();
                if T && st != 0 {
                    self.t_control_val_only();
                }
                // The taint of every guard that REFUSED an earlier arm: which
                // arm runs after a failed guard is that guard's value too, and
                // so is whether a later guard is evaluated at all. It starts at
                // the SUBJECT's taint: a guard is evaluated only because the
                // pattern matched the subject (amendment 117, loop finding 2).
                let mut lost = st;
                for arm in arms {
                    env.push();
                    // R50 S0 (AX-57): an `Err` out of the pattern or the guard
                    // pops the arm's scope before propagating, so the
                    // enclosing scope's single pop removes its own mark.
                    let matched =
                        match self.match_pattern::<T>(&arm.pattern, &v, env, self.ts::<T>(st)) {
                            Ok(m) => m,
                            Err(e) => {
                                env.pop();
                                return Err(e);
                            }
                        };
                    if matched {
                        let mut gt = 0;
                        if let Some(guard) = &arm.guard {
                            let ok = match self.t_branch(lost, false, || self.eval_t::<T>(guard, env)) {
                                Ok(g) => matches!(g, Value::Bool(true)),
                                Err(e) => {
                                    env.pop();
                                    return Err(e);
                                }
                            };
                            gt = self.tl::<T>();
                            if !ok {
                                if T {
                                    lost |= gt;
                                }
                                env.pop();
                                continue;
                            }
                        }
                        let ct = st | gt | lost;
                        let exits = T
                            && ct != 0
                            && arms.iter().any(|a| {
                                taint::has_exit(&a.body) || a.guard.as_ref().is_some_and(taint::has_exit)
                            });
                        let r = self.t_branch(ct, exits, || self.eval_t::<T>(&arm.body, env));
                        env.pop();
                        if T {
                            self.t_pick_branch(ct, &r);
                        }
                        return r;
                    }
                    env.pop();
                }
                panic("no match arm matched")
            }

            Expr::While { cond, body } => {
                // An `Uncertain<bool>` condition branches on its inner bool
                // (confidence is irrelevant to control flow) — same as `if`.
                // Every evaluation of the condition after the first runs only
                // because an earlier (possibly tainted) one was true, so a
                // store in the condition is control-dependent on them
                // (amendment 117, loop finding 26).
                let mut seen = 0u8;
                loop {
                    let cv = self.t_branch(seen, false, || self.eval_t::<T>(cond, env))?;
                    let ct = self.tl::<T>();
                    if T {
                        seen |= ct & taint::VAL;
                    }
                    if !cond_bool(cv, "while")? {
                        break;
                    }
                    let exits = T && ct != 0 && taint::stmts_have_exit(body);
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
                let mut seen = 0u8;
                loop {
                    let v = self.t_branch(seen, false, || self.eval_t::<T>(expr, env))?;
                    let vt = self.tl::<T>();
                    if T {
                        seen |= vt & taint::VAL;
                    }
                    env.push();
                    // R50 S0: pop the iteration's scope on a pattern error too.
                    let matched = match self.match_pattern::<T>(pattern, &v, env, self.ts::<T>(vt)) {
                        Ok(m) => m,
                        Err(e) => {
                            env.pop();
                            return Err(e);
                        }
                    };
                    if !matched {
                        env.pop();
                        break;
                    }
                    let exits = T && vt != 0 && taint::stmts_have_exit(body);
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
                let s = strict_int(self.eval_t::<T>(start, env)?)?;
                let e = strict_int(self.eval_t::<T>(end, env)?)?;
                // The loop variable counts out a range; if sealed code sized
                // the range it chose the variable, and the body runs as often
                // as it said.
                let bt = self.ta::<T>();
                let exits = T && bt != 0 && taint::stmts_have_exit(body);
                let mut i = s;
                let (var, slot) = self.res.var(expr, var);
                loop {
                    let cont = if *inclusive { i <= e } else { i < e };
                    if !cont {
                        break;
                    }
                    env.push();
                    env.define_var_t::<T>(var, slot, Value::Int(i), self.ts::<T>(bt) & taint::VAL);
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
                    let t = self.tl::<T>();
                    self.taint.ret.set(self.ts::<T>(t));
                    // `?` is an early exit chosen by the operand's value: what
                    // runs after it is control-dependent on it, taken or not.
                    let ct = t & taint::VAL;
                    if ct != 0 {
                        let st = &self.taint.sticky;
                        st.set(st.get() | ct);
                    }
                }
                question(r)
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
                        if T {
                            // Stamped with its stated element type, and its
                            // creating side recorded (amendment 72; veto
                            // eval.rs:621-625).
                            let elem =
                                name.strip_prefix("chan::<").and_then(|s| s.strip_suffix('>'));
                            self.chan_created(&q, elem);
                        }
                        return Ok(Value::Chan(q));
                    }
                    // R13 native FFI: a native `M::fn(...)` call dispatches to the
                    // in-process mock shim (one impl, two engines — I-2).
                    if crate::native::is_native_call(name) {
                        let r = self.eval_native_call(name, args, env);
                        // A native handle names state in a registry every
                        // frame shares: the call is `World` (amendment 108).
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
                // The receiver's own taint, before its arguments overwrite
                // `taint.last` (a constant 0 unsealed; veto eval.rs:649).
                let rt = self.tl::<T>();
                if let Value::Chan(q) = &recv {
                    if T {
                        // One access helper before any channel method runs
                        // (amendments 106, 117; veto eval.rs:650-658).
                        self.t_chan_access(&recv, method);
                        self.t_chan_choice(&recv, rt);
                    }
                    return self.chan_method::<T>(q, method, args, env);
                }
                let mut argv = Vec::with_capacity(args.len() + 1);
                argv.push(recv);
                for a in args {
                    argv.push(self.eval_t::<T>(a, env)?);
                }
                if T {
                    return self.impl_method_sealed(receiver, method, argv, rt);
                }
                self.impl_method(method, argv)
            }

            Expr::FieldAccess { receiver, field } => {
                // A variable receiver (`p.x`, the common case) is read in
                // place: only the field is cloned, not the whole record. The
                // lookup is the `Expr::Ident` arm's, verbatim.
                let f = self.res.sym(expr, field);
                if let Expr::Ident(name) = receiver.as_ref() {
                    let (s, slot) = self.res.var(receiver, name);
                    return self.field_in_place(s, slot, name, f, field, env);
                }
                let v = self.eval_t::<T>(receiver, env)?;
                field_of(&v, f, field)
            }

            Expr::Tuple(elems) => {
                let mut vs = Vec::with_capacity(elems.len());
                for e in elems {
                    vs.push(self.eval_t::<T>(e, env)?);
                }
                Ok(Value::tuple(vs))
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
                    let (s, slot) = self.res.var(receiver, name);
                    if env.get_var(s, slot).is_some() || self.is_global(s) {
                        let idx = strict_int(self.eval_t::<T>(index, env)?)?;
                        if T {
                            // An operator-typed element read at an index sealed
                            // code chose is a pick (veto eval.rs:772-789).
                            let it = self.tl::<T>();
                            let r = self.index_in_place(s, slot, name, idx, env);
                            if let Ok(el) = &r {
                                self.t_pick_index(it, el);
                            }
                            return r;
                        }
                        return self.index_in_place(s, slot, name, idx, env);
                    }
                }
                let arr = self.eval_t::<T>(receiver, env)?;
                let idx = strict_int(self.eval_t::<T>(index, env)?)?;
                if T {
                    let it = self.tl::<T>();
                    let r = index_value(arr, idx);
                    if let Ok(el) = &r {
                        self.t_pick_index(it, el);
                    }
                    return r;
                }
                index_value(arr, idx)
            }

            Expr::Array(elems) => {
                let mut out = Vec::with_capacity(elems.len());
                for e in elems {
                    out.push(self.eval_t::<T>(e, env)?);
                }
                Ok(Value::Array(Rc::new(out)))
            }

            Expr::StructLit { name, fields } => {
                // AX-47: the names, storage order and per-field coercion width
                // were resolved once (see `RecordLit`); a node outside the
                // table (an AST clone made at run time) resolves here.
                let unresolved;
                let lit = match self.res.record_lit(expr) {
                    Some(lit) => lit,
                    None => {
                        unresolved = RecordLit::of(name, fields, &self.structs, &self.enums);
                        &unresolved
                    }
                };
                if let Some(v) = &lit.empty {
                    return Ok(v.clone());
                }
                // Fields are evaluated in source order; `finish_record` builds
                // the value.
                let mut vals = self.take_args(fields.len());
                for (_, fexpr) in fields {
                    vals.push(self.eval_t::<T>(fexpr, env)?);
                }
                let v = self.finish_record(lit, name, &mut vals);
                vals.clear();
                self.recycle_args(vals);
                v
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
                // The result is a `str` whoever the parts were.
                if T {
                    self.t_untype();
                }
                Ok(Value::Str(Rc::new(s)))
            }

            Expr::Lambda { .. } => Ok(self.make_closure(expr, env)),

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
                // Which arm fires is the readiness of every queue looked at up
                // to it: the arm's body runs under the control taint of all of
                // them, and so does what runs after it if any arm can leave
                // the fn (a skipped arm is a branch not taken).
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
                    if T {
                        // The channel looked at may itself be the candidate's
                        // pick; whether it is ready is a READ of its queue,
                        // ready or not (amendments 106, 117; veto
                        // eval.rs:1019-1034).
                        let rt = self.tl::<T>();
                        let ch = Value::Chan(q.clone());
                        self.t_chan_access(&ch, "recv");
                        self.t_chan_choice(&ch, rt);
                        lost |= self.t_obj(&ch) & taint::VAL;
                    }
                    let ready = q.borrow_mut().pop_front();
                    if ready.is_some() {
                        let exits = T && lost != 0 && arms.iter().any(|a| taint::has_exit(&a.body));
                        return self.t_branch(lost, exits, || self.eval_t::<T>(&arm.body, env));
                    }
                }
                panic("select: no channel was ready (cooperative interpreter — send before select)")
            }
        }
    }

    /// The closure value of lambda node `expr` evaluated in `env`. Shared by
    /// the `eval` arm and the bytecode engine (R50).
    pub(super) fn make_closure(&self, expr: &Expr, env: &Env) -> Value {
        // A lambda inside an AST clone made at run time (a `SendValue`
        // closure's body) is not in the resolution table.
        let unresolved;
        let info = match self.res.lambda(expr) {
            Some(info) => info,
            None => {
                let Expr::Lambda { params, body, .. } = expr else {
                    unreachable!("make_closure on a non-lambda node")
                };
                unresolved = LambdaInfo::of(params, body);
                &unresolved
            }
        };
        // AX-40: capture only the free variables bound here — not the
        // whole environment. A free name that is not bound here (a
        // global, a fn, a builtin) is reached the same way at call time.
        // In a sealed run each capture keeps its binding's taint (R51 S2.3).
        let captured = env.capture(info.captures.iter().copied());
        // T40: a SHARED, persistent capture cell — see Value::Closure.
        let mut cv = ClosureVal::plain(Rc::clone(&info.code), captured);
        if self.seal.active {
            // Its own parameter annotations are its first contract, and a
            // closure a SEALED frame created runs sealed wherever it is later
            // called (veto eval.rs:961-977; R51 Fork 3, typed marks).
            if let Expr::Lambda { params, .. } = expr {
                cv.contract = Self::lambda_contract(params);
            }
            cv.seal.sealed_origin = self.frame_sealed.get();
            // Its body is judged by the pin analysis as the code of the fn
            // that created it (veto `PIN_FN_MARK`, eval.rs:967-972; R51 S4).
            cv.seal.pin_owner = self.pin_fn.get();
        }
        Value::Closure(Rc::new(cv))
    }

    /// Bind the evaluated value `v` of a `let`/`own`/`ref` (all three behave
    /// identically) to `name` (`s` at frame slot `slot`) with the optional
    /// annotation `ty`: the refinement check, the sized-int coercion, then the
    /// define, carrying the value's taint `vt`. Shared by the `eval` arm and
    /// the bytecode engine (R50), which is never sealed (`T = false`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn bind_let<const T: bool>(
        &self,
        name: &str,
        s: Sym,
        slot: u32,
        ty: Option<&crate::ast::AxonType>,
        v: Value,
        vt: u8,
        env: &mut Env,
    ) -> Result<(), Flow> {
        nest_guard!(self, BIND_LET);
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
                    // The predicate's env is fresh: it keeps the bound value's
                    // taint (T8.3).
                    let mut pe = self.new_env();
                    pe.define_t::<T>(SYM_UNDERSCORE, v.clone(), vt);
                    // Also bind the bound name for inline `let x: T where E[x] > k`.
                    pe.define_t::<T>(s, v.clone(), vt);
                    if let Value::Bool(false) =
                        contain_frame(self.eval_t::<T>(pred, &mut pe), FrameEdge::Predicate)?
                    {
                        return Err(Flow::RefineViolation(
                            format!(
                                "the value bound to `{}` (= {}) violates the refinement `{}` \
                                 — the value does not satisfy the type's predicate",
                                name,
                                display(&v),
                                rn
                            )
                            .into(),
                        ));
                    }
                }
            }
        }
        // R19 Slice B: if the annotation names a non-i64 integer type, coerce
        // the stored value to SizedInt so downstream arithmetic is width-correct.
        // Completeness requirement: EVERY static-type-introduction site must
        // coerce so no SizedInt value is left as a bare Int at any missed site,
        // which would silently compute in i64 (I-9).
        let mut v = if let Some(width) = ty.and_then(axon_type_to_width) {
            coerce_to_sized::<T>(v, width)
        } else {
            v
        };
        let mut vt = vt;
        // The annotation is a declared type: in a sealed run (`T`) the value
        // is cast to it (amendment 53; veto eval.rs:241-255). A closed
        // declared type the cast verified is a PIN: the runtime type is the
        // operator's, whatever produced the value. It pins a type, never a
        // value (amendment 102).
        if T {
            if let Some(t) = ty {
                if let Err(why) = self.cast(&mut v, t, &Default::default()) {
                    return panic(format!(
                        "`{name}` is declared `{}` but was bound to {} — a runtime type \
                         confusion ({why})",
                        crate::doc::render_type(t),
                        display(&v)
                    ));
                }
                if vt & taint::TYP != 0 && self.t_pins(t) {
                    vt &= !taint::TYP;
                }
            }
        }
        env.define_var_t::<T>(s, slot, v, vt);
        Ok(())
    }

    pub(super) fn eval_block<const T: bool>(&self, stmts: &[Stmt], env: &mut Env) -> R {
        nest_guard!(self, EVAL_BLOCK);
        env.push();
        let mut last = Value::Unit;
        let n = stmts.len();
        for (i, stmt) in stmts.iter().enumerate() {
            // What a statement touched does not become the block's value: its
            // effects travel through the bindings and containers it wrote.
            // Only the tail expression is the block's value (amendment 102).
            let before = self.ta::<T>();
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
        nest_guard!(self, RUN_LOOP_BODY);
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

    /// A method call on a channel receiver (R50 S3: extracted from the
    /// `MethodCall` arm, which the vm's `MethodRecv` op shares). The argument
    /// nodes are evaluated here, exactly as the arm always did: `send`
    /// evaluates only `args[0]`, every other method none of them.
    /// Cooperative and single-threaded: `send` pushes to the shared queue,
    /// `recv` pops from it, `clone` shares the handle.
    pub(super) fn chan_method<const T: bool>(
        &self,
        q: &Rc<RefCell<VecDeque<Value>>>,
        method: &str,
        args: &[Expr],
        env: &mut Env,
    ) -> R {
        nest_guard!(self, CHAN_METHOD);
        match method {
            "send" => {
                let mut v = self.eval_t::<T>(&args[0], env)?;
                if T && !self.frame_sealed.get() {
                    // The queue now holds what was sent, and whoever reads it
                    // back inherits that (amendment 102).
                    self.t_mark_obj(&Value::Chan(q.clone()), self.ts::<T>(self.tl::<T>()));
                }
                if T {
                    // Cast to every element type the channel crossed; the
                    // operator sends sealed code a value: dicts in it are
                    // snapshotted (amendment 72 part 2; veto eval.rs:664-675).
                    self.chan_send_check(q, &mut v)?;
                    if !self.frame_sealed.get() {
                        self.dict_edge_in(&v)?;
                    }
                }
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
        }
    }

    /// A method call on any receiver but a channel, after the receiver
    /// (`argv`'s first value) and then the arguments were evaluated left to
    /// right (R50 S3: extracted from the `MethodCall` arm, shared with the
    /// vm): the impl method registered for the receiver's type name, else
    /// the no-method panic.
    pub(super) fn impl_method(&self, method: &str, mut argv: impl CallArgs) -> R {
        nest_guard!(self, IMPL_METHOD);
        let tn = argv.values()[0].type_name();
        if let Some(f) = self.methods.get(&(tn.clone(), method.to_string())) {
            self.call_fn(f, argv)
        } else {
            panic(format!("no method `{method}` on type `{tn}`"))
        }
    }

    /// [`Interp::impl_method`] in a sealed run (the tree's `MethodCall` arm
    /// under `T`; the VM's `Op::MethodCall` never runs sealed, R51 Fork 1):
    /// the method-dispatch edges (`seal_method`, `seal_dispatch`) and the
    /// runtime dispatch rule (`t_check_dispatch` on the receiver's taint `rt`,
    /// R51 S4) before the call, and the sealed caller's common miss text
    /// (veto eval.rs:713-724).
    #[inline(never)]
    fn impl_method_sealed(&self, receiver: &Expr, method: &str, argv: Vec<Value>, rt: u8) -> R {
        nest_guard!(self, IMPL_METHOD);
        let tn = argv[0].type_name();
        if let Some(f) = self.methods.get(&(tn.clone(), method.to_string())) {
            self.seal_method(f, &tn)?;
            self.seal_dispatch(receiver, f, &argv[0], &tn)?;
            self.t_check_dispatch(f, &argv[0], rt, &tn)?;
            self.call_fn(f, argv)
        } else {
            // A sealed caller is told what it is told for an operator method
            // it may not use (`seal_method`): the miss and the refusal read
            // the same (existence oracle, amendment 108).
            self.no_such_fn(method, format!("no method `{method}` on type `{tn}`"))
        }
    }

    /// R13 native FFI: dispatch a `M::fn(...)` call to the in-process native
    /// shim. Evaluates args, marshals them across the boundary (extracting
    /// handle payloads — never raw pointers), dispatches to the registered Rust
    /// impl, and wraps the result. A bad/forged/consumed handle returns a
    /// graceful panic (exit 101, I-4), NEVER a host abort. v1 has one module:
    /// the GPU-free `gfx` mock.
    fn eval_native_call(&self, qualified: &str, args: &[Expr], env: &mut Env) -> R {
        nest_guard!(self, EVAL_NATIVE_CALL);
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
                Value::Handle(h) => {
                    let (hm, hn, payload) = (&h.module, &h.name, &h.payload);
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
            Ok(crate::native::GfxValue::Handle { name, payload }) => {
                Ok(Value::Handle(Rc::new(HandleVal {
                    module: module.name.to_string(),
                    name: name.to_string(),
                    payload,
                    resource: matches!(
                        nf.ret,
                        crate::native::FfiType::Handle { resource: true, .. }
                    ),
                })))
            }
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
                Value::Handle(h) => {
                    let (hm, hn, payload) = (&h.module, &h.name, &h.payload);
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
            Ok(DomainValue::Handle { name, payload }) => Ok(Value::Handle(Rc::new(HandleVal {
                module: module.name.to_string(),
                name: name.to_string(),
                payload,
                resource: matches!(
                    nf.ret,
                    crate::native::FfiType::Handle { resource: true, .. }
                ),
            }))),
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
        nest_guard!(self, EVAL_CALL);
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
            return self.call_mut(callee, args, tier, env);
        }

        // Evaluate arguments left-to-right, into a reused buffer (AX-54).
        let mut argv = self.take_args(args.len());
        // The NAME rule (R51 S3): a sealed run's call of a name-resolving
        // builtin. `T` is set only in a sealed run. Any other named callee
        // keeps its arguments' taints for the builtin pick (R51 S4).
        if T {
            if let Expr::Ident(name) = callee {
                if super::pin::name_sink(name).is_some() {
                    return self.eval_name_sink_call(name, callee, args, argv, tier, env);
                }
                return self.eval_ident_call_sealed(callee, args, argv, tier, env);
            }
        }
        for a in args {
            argv.push(self.eval_t::<T>(a, env)?);
        }
        self.dispatch_call::<T>(callee, argv, tier, env)
    }

    /// [`Interp::eval_call`] of a NAME-RESOLVING builtin (`pin::NAME_SINKS`)
    /// in a sealed run: the arguments are evaluated with their taints, then,
    /// unless a local closure of that name is what runs, the name rule is
    /// applied by the static analysis (`seal_name_args`) and by the runtime
    /// taint (`t_check_names`) before the builtin is dispatched (veto
    /// eval.rs:1481-1484).
    #[inline(never)]
    fn eval_name_sink_call(
        &self,
        name: &str,
        callee: &Expr,
        args: &[Expr],
        mut argv: Vec<Value>,
        tier: Option<&str>,
        env: &mut Env,
    ) -> R {
        let mut ats = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval_t::<true>(a, env)?);
            ats.push(self.tl::<true>());
        }
        let (s, slot) = self.res.var(callee, name);
        if !matches!(env.get_var(s, slot), Some(Value::Closure { .. })) {
            self.seal_name_args(name, args)?;
            self.t_check_names(name, &ats)?;
        }
        self.taint.call_ats.replace(ats);
        self.dispatch_call::<true>(callee, argv, tier, env)
    }

    /// [`Interp::eval_call`] of any other identifier callee in a sealed run:
    /// the arguments are evaluated with their taints, which wait in
    /// `taint.call_ats` for [`Interp::dispatch_named`]'s builtin pick
    /// (`t_builtin_pick`, veto eval.rs:1501-1506). A cell, not a parameter,
    /// so `dispatch_named` keeps the signature the bytecode engine calls.
    #[inline(never)]
    fn eval_ident_call_sealed(
        &self,
        callee: &Expr,
        args: &[Expr],
        mut argv: Vec<Value>,
        tier: Option<&str>,
        env: &mut Env,
    ) -> R {
        let mut ats = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval_t::<true>(a, env)?);
            ats.push(self.tl::<true>());
        }
        self.taint.call_ats.replace(ats);
        self.dispatch_call::<true>(callee, argv, tier, env)
    }

    /// Dispatch a call whose arguments `argv` are already evaluated: `resume`,
    /// the per-call `tier:`, then a local closure, a builtin, a user fn, a
    /// module-level closure, the unknown-function panic; a callee that is not
    /// an identifier is evaluated (after the arguments) and called as a
    /// closure. Only `eval_call` calls it (the bytecode engine joins at
    /// [`Interp::dispatch_named`]).
    pub(super) fn dispatch_call<const T: bool>(
        &self,
        callee: &Expr,
        argv: Vec<Value>,
        tier: Option<&str>,
        env: &mut Env,
    ) -> R {
        nest_guard!(self, DISPATCH_CALL);
        if let Expr::Ident(name) = callee {
            if name == "resume" {
                return self.dispatch_resume(argv);
            }
            let (s, slot) = self.res.var(callee, name);
            return self.dispatch_named::<T>(name, s, slot, argv, tier, env);
        }
        self.set_call_tier(tier);
        // Callee is an expression that should evaluate to a closure
        // (e.g. `make_adder(1)(2)` or an array element).
        let c = self.eval_t::<T>(callee, env)?;
        self.call_closure(c, argv)
    }

    /// Phase 6: `resume(v)` inside a handler arm carries `v` back to the
    /// intercepted operation as a `Flow::Resume`. It is caught at the
    /// builtin-interception site (`run_handler_arm`). `resume` with no arg
    /// resumes with Unit. (The resolver only binds `resume` inside an arm, so
    /// a `resume` here is genuinely a handler resume, not a user fn named
    /// `resume` — and the builtin/user-fn lookups never define one.) Shared
    /// by [`Interp::dispatch_call`] and the bytecode engine (R50).
    #[cold]
    pub(super) fn dispatch_resume(&self, argv: Vec<Value>) -> R {
        nest_guard!(self, DISPATCH_RESUME);
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
        Err(Flow::Resume(v))
    }

    /// R3b: make the per-call `tier:` (if any) visible to the builtin dispatch
    /// for the duration of this call (read by `current_ai_tier`). A call with
    /// no `tier:` leaves an already-clear slot alone (AX-54).
    #[inline(always)]
    pub(super) fn set_call_tier(&self, tier: Option<&str>) {
        if tier.is_some() || self.current_call_tier.borrow().is_some() {
            *self.current_call_tier.borrow_mut() = tier.map(|t| t.to_string());
        }
    }

    /// [`Interp::dispatch_call`] for an identifier callee `name` other than
    /// `resume`, already resolved to `(s, slot)` (the bytecode engine resolves
    /// it at compile time, R50): the per-call `tier:`, then a local closure, a
    /// builtin, a user fn, a module-level closure, the unknown-function panic.
    pub(super) fn dispatch_named<const T: bool>(
        &self,
        name: &str,
        s: Sym,
        slot: u32,
        mut argv: impl CallArgs,
        tier: Option<&str>,
        env: &mut Env,
    ) -> R {
        nest_guard!(self, DISPATCH_NAMED);
        self.set_call_tier(tier);
        // The taints of the arguments, when a sealed `eval_call` left them.
        let ats = if T {
            self.taint.call_ats.take()
        } else {
            Vec::new()
        };
        // 1. A local/captured variable holding a closure.
        if let Some(c @ Value::Closure { .. }) = env.get_var(s, slot) {
            let c = c.clone();
            if T {
                // The callee is read out of the binding without an `eval`: a
                // closure the binding holds under a taint is picked (veto
                // eval.rs:1460-1469).
                if let Some(i) = env.get_var_idx(s, slot) {
                    self.t_note(&c, env.taint(i));
                }
            }
            return self.call_local_closure(c, argv.into_vec(self));
        }
        // A sealed run: any other value a local holds is what the name means
        // here, and it is not callable. It never falls through to a builtin or
        // fn of the same NAME: a confused value in a local `square` would run
        // the operator's own `fn square` (veto eval.rs:1453-1473, amendment 60;
        // R51 S4). Unsealed runs keep main's fall-through.
        if self.seal.active {
            if let Some(c) = env.get_var(s, slot) {
                return super::not_callable(c);
            }
        }
        // 2. A builtin — skipped for a name already proven not to be one
        //    (see `callees`), which also caches step 3's lookup.
        let known = self.callees.borrow().get(s.index()).copied();
        let resolved = match known {
            Some(r) if r != CALLEE_UNKNOWN => r,
            _ => {
                // A sealed run: what the builtin reads (its arguments, deep,
                // and the kernel or world state its class says it reads) goes
                // into the result, and the builtin runs under the control
                // taint of its arguments: a callback it runs once per element
                // of a tainted array runs a number of times the candidate
                // chose, and what that callback stores carries it.
                if T {
                    self.t_builtin_in(name, argv.values());
                }
                let pc = if T {
                    Some(self.t_pc(self.taint.acc.get() & taint::VAL))
                } else {
                    None
                };
                // Whether sealed code ran inside the builtin (a callback).
                let entries = if T { self.taint.entries.get() } else { 0 };
                if let Some(v) = self.call_builtin(name, argv.values())? {
                    if T {
                        self.t_builtin_out(name, argv.values(), &v);
                        self.t_builtin_pick(
                            argv.values(),
                            &ats,
                            self.taint.entries.get() != entries,
                            &v,
                        );
                    }
                    drop(pc);
                    argv.done(self);
                    return Ok(v);
                }
                drop(pc);
                let r = match self.fn_of_sym.get(&s) {
                    Some(&i) => CALLEE_FN + i,
                    None => CALLEE_NOT_FN,
                };
                if super::builtins::builtin_dispatch_is_inert(name) {
                    let mut callees = self.callees.borrow_mut();
                    if callees.len() <= s.index() {
                        callees.resize(s.index() + 1, CALLEE_UNKNOWN);
                    }
                    callees[s.index()] = r;
                }
                r
            }
        };
        // 3. A user-defined function.
        if resolved >= CALLEE_FN {
            let e = &self.fn_table[(resolved - CALLEE_FN) as usize];
            // `call_fn_entry` refuses a `&mut` fn reached without `&mut`
            // operands ahead of the call edge, in words that name it: a sealed
            // caller is told what a missing name gets first (amendment 106).
            if e.has_ref_mut {
                self.seal_call(e.def)?;
            }
            return self.call_fn_entry(e, argv);
        }
        // 4. A module-level closure constant.
        if let Some(c @ Value::Closure { .. }) = self.global_ref(s)? {
            return self.call_closure(c.clone(), argv.into_vec(self));
        }
        self.no_such_fn(name, format!("call to unknown function `{name}`"))
    }

    /// AX-08: `f(.., &mut a, ..)`. The checker (E0605/E0606) guarantees the
    /// callee is a free fn whose matching params are `&mut [T]`, every `&mut`
    /// operand is a whole local, and no other argument mentions it. Each
    /// borrowed value is MOVED out of the caller's binding into the callee (no
    /// copy) and the param's final value is moved back on every outcome,
    /// including `return` / `?` / error unwinds, so the caller's binding is
    /// never left hollow. The bytecode engine (R50 S5) resolves the callee
    /// and evaluates the plain arguments itself, then joins at
    /// [`Interp::call_mut_with`].
    #[inline(never)]
    pub(super) fn call_mut(
        &self,
        callee: &Expr,
        args: &[Expr],
        tier: Option<&str>,
        env: &mut Env,
    ) -> R {
        nest_guard!(self, CALL_MUT);
        let entry = &self.fn_table[self.call_mut_entry(callee)? as usize];
        // Plain arguments first, left to right: they cannot mention a
        // borrowed variable (E0606), so this order is unobservable — and if
        // one unwinds (`?`, panic) nothing has been moved out yet. A pooled
        // buffer (AX-54), which `call_fn_in` recycles.
        let mut argv = self.take_args(args.len());
        for a in args {
            argv.push(match a {
                Expr::UnaryOp {
                    op: UnaryOp::RefMut,
                    ..
                } => Value::Unit,
                _ => self.eval(a, env)?,
            });
        }
        self.call_mut_with(entry, args, argv, tier, env)
    }

    /// The `fn_table` index of a `&mut` call's callee, or the panic of a
    /// callee that is not a fn (spec §4 S5: the VM resolves it at compile
    /// time, the same way).
    pub(super) fn call_mut_entry(&self, callee: &Expr) -> Result<u32, Flow> {
        match callee {
            Expr::Ident(name) => match self.fn_of_sym.get(&self.res.sym(callee, name)) {
                Some(&i) => Ok(i),
                None => panic(format!("`&mut` argument passed to `{name}`, which is not a function taking `&mut` parameters")),
            },
            _ => panic("`&mut` argument passed to a computed callee".to_string()),
        }
    }

    /// [`Interp::call_mut`] after the plain arguments are evaluated into
    /// `argv` (a `Unit` in each `&mut` position): move the borrowed values
    /// in, call `entry` in a pooled frame, move each `&mut` param's final
    /// value back. Allocation-free (R50 S5): the frame and `argv` come from
    /// their pools, and the borrowed bindings are found again from `args`
    /// instead of being listed.
    #[inline(always)]
    pub(super) fn call_mut_with(
        &self,
        entry: &FnEntry<'p>,
        args: &[Expr],
        mut argv: impl CallArgs,
        tier: Option<&str>,
        env: &mut Env,
    ) -> R {
        if self.seal.active {
            return self.call_mut_sealed(entry, args, argv, tier, env);
        }
        for (i, a) in args.iter().enumerate() {
            if let Expr::UnaryOp {
                op: UnaryOp::RefMut,
                operand,
            } = a
            {
                let Expr::Ident(name) = operand.as_ref() else {
                    return panic("`&mut` of something other than a local variable".to_string());
                };
                let (s, slot) = self.res.var(operand, name);
                let Some(b) = env.get_var_mut(s, slot, 0) else {
                    return panic(format!("`&mut {name}`: `{name}` is not a local variable"));
                };
                let v = std::mem::replace(b, Value::Unit);
                forget_scalar(std::mem::replace(&mut argv.values()[i], v));
            }
        }
        self.set_call_tier(tier);
        let mut frame = self.take_frame();
        let result = self.call_fn_in(entry, argv, &mut frame);
        // The body's block scopes are popped by now (on `return`/`?` too), so
        // each name resolves to the parameter binding itself. Each borrowed
        // binding gets its param's final value (`Unit` for a param that is
        // not `&mut`, or none).
        for (i, a) in args.iter().enumerate() {
            if let Expr::UnaryOp {
                op: UnaryOp::RefMut,
                operand,
            } = a
            {
                let Expr::Ident(name) = operand.as_ref() else {
                    continue;
                };
                let out = match (entry.def.params.get(i), entry.params.get(i)) {
                    (Some(p), Some(s)) if matches!(p.ty, crate::ast::AxonType::RefMut(_)) => {
                        match frame.get_mut(*s, 0) {
                            Some(v) => std::mem::replace(v, Value::Unit),
                            None => Value::Unit,
                        }
                    }
                    _ => Value::Unit,
                };
                let (s, slot) = self.res.var(operand, name);
                if let Some(b) = env.get_var_mut(s, slot, 0) {
                    forget_scalar(std::mem::replace(b, out));
                }
            }
        }
        self.give_frame(frame);
        result
    }

    /// [`Interp::call_mut_with`] in a sealed run (veto `call_fn_mut`,
    /// interp.rs:3495-3577; eval.rs:1575-1593). A borrowed binding's taint
    /// goes into the callee with its value, and what the binding holds when
    /// the call ends is a second return channel, for the value and its taint
    /// (the callee's binding for an operator fn, everything for a
    /// candidate's: amendment 102). A candidate fn writing through `&mut`
    /// returns control to operator code with the parameter's FINAL value, so
    /// that value is a seal crossing exactly as the return value is (C9 round
    /// 8, PSV-1 blocker): cast strictly to the declared type, and a
    /// replacement judged against what the operator held (amendment 78). A
    /// refused value is never handed back: the binding is left `()` and the
    /// call's result is the refusal.
    #[inline(never)]
    fn call_mut_sealed(
        &self,
        entry: &FnEntry<'p>,
        args: &[Expr],
        mut argv: impl CallArgs,
        tier: Option<&str>,
        env: &mut Env,
    ) -> R {
        let f = entry.def;
        let crossing = self.fn_is_sealed(f) && !self.frame_sealed.get();
        let is_ref_mut = |i: usize| {
            f.params
                .get(i)
                .is_some_and(|p| matches!(p.ty, crate::ast::AxonType::RefMut(_)))
        };
        let mut held: Vec<Option<Value>> = Vec::new();
        if crossing {
            held.resize(args.len(), None);
        }
        for (i, a) in args.iter().enumerate() {
            if let Expr::UnaryOp {
                op: UnaryOp::RefMut,
                operand,
            } = a
            {
                let Expr::Ident(name) = operand.as_ref() else {
                    return panic("`&mut` of something other than a local variable".to_string());
                };
                let (s, slot) = self.res.var(operand, name);
                let Some(idx) = env.get_var_idx(s, slot) else {
                    return panic(format!("`&mut {name}`: `{name}` is not a local variable"));
                };
                self.t_touch(env.taint(idx));
                let v = std::mem::replace(env.value_mut(idx, 0), Value::Unit);
                if crossing && is_ref_mut(i) {
                    held[i] = Some(v.clone());
                }
                forget_scalar(std::mem::replace(&mut argv.values()[i], v));
            }
        }
        self.set_call_tier(tier);
        let mut frame = self.take_frame();
        let mut result = self.call_fn_in(entry, argv, &mut frame);
        let cx = if crossing {
            Some(self.fn_cx(f).strict(true))
        } else {
            None
        };
        for (i, a) in args.iter().enumerate() {
            if let Expr::UnaryOp {
                op: UnaryOp::RefMut,
                operand,
            } = a
            {
                let Expr::Ident(name) = operand.as_ref() else {
                    continue;
                };
                let (mut out, out_t) = match (is_ref_mut(i), entry.params.get(i)) {
                    (true, Some(s)) => match frame.find(*s) {
                        Some(j) => {
                            let t = if crossing { taint::ALL } else { frame.taint(j) };
                            (std::mem::replace(frame.value_mut(j, 0), Value::Unit), t)
                        }
                        None => (Value::Unit, 0),
                    },
                    _ => (Value::Unit, 0),
                };
                if let (Some(cx), Some(p)) = (&cx, f.params.get(i).filter(|_| is_ref_mut(i))) {
                    let why = match self.cast(&mut out, &p.ty, cx) {
                        Err(why) => Some(format!(
                            "`&mut` argument `{}` of `{}` is declared `{}` but was left holding {} — \
                             a runtime type confusion ({why})",
                            p.name,
                            f.name,
                            crate::doc::render_type(&p.ty),
                            value::display(&out)
                        )),
                        Ok(()) => held[i]
                            .as_ref()
                            .and_then(|old| self.replaced_ok_top(old, &out).err())
                            .map(|why| {
                                format!(
                                    "`&mut` argument `{}` of `{}` was left holding a value the \
                                     operator's did not allow — a runtime type confusion ({why})",
                                    p.name, f.name
                                )
                            }),
                    };
                    if let Some(msg) = why {
                        out = Value::Unit;
                        if !matches!(result, Err(Flow::Panic(_))) {
                            result = panic(msg);
                        }
                    }
                }
                let (s, slot) = self.res.var(operand, name);
                if let Some(b) = env.get_var_mut(s, slot, out_t) {
                    forget_scalar(std::mem::replace(b, out));
                }
            }
        }
        self.give_frame(frame);
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
        nest_guard!(self, EVAL_WITH_HANDLER);
        let (arms, return_arm) = match handler {
            crate::ast::HandlerExpr::Inline { arms, return_arm } => (arms, return_arm),
            // Unresolved named handler: inert (matches the pre-Phase-6 behavior).
            crate::ast::HandlerExpr::Named(_) => return self.eval(body, env),
        };

        // Capture the defining environment once; every arm closes over it.
        // The frame points into `handler` and `body` (no AST clone, AX-56);
        // it is popped below, before this borrow ends (`SrcRef`'s invariant).
        let captured = env.snapshot();
        let frame = HandlerFrame {
            arms: arms
                .iter()
                .map(|a| HandlerArmRt {
                    arm: SrcRef::new(a),
                    captured: captured.clone(),
                })
                .collect(),
            // Phase 6 multi-shot: remember the body + its env so a non-tail arm
            // can replay the continuation.
            body: SrcRef::new(body),
            env_snapshot: env.snapshot(),
            sealed: self.frame_sealed.get(),
            operator_frames: self.operator_frames.get(),
            pin_owner: self.pin_fn.get(),
        };
        let depth = self.handlers.borrow().len();
        self.handlers.borrow_mut().push(frame);
        // An arm that is not a bare tail `resume(..)` can ABORT the body at the
        // operation it answers (or replay it): what runs after that point is
        // skipped by a decision of whoever performed the operation. While such
        // a handler is installed, a branch on candidate data is treated as a
        // possible exit (sticky), taken or not, and sealed code that ran inside
        // the body makes what follows control-dependent on it (amendment 117,
        // loop findings 31, 32, 34).
        let abortable = self.seal.active
            && arms
                .iter()
                .any(|a| !crate::effects::arm_is_bare_tail_resume(&a.body));
        let entries = self.taint.entries.get();
        if abortable {
            let n = &self.taint.abortable;
            n.set(n.get() + 1);
        }
        let result = self.eval(body, env);
        if abortable {
            let n = &self.taint.abortable;
            n.set(n.get() - 1);
            if !self.frame_sealed.get() && self.taint.entries.get() != entries {
                let st = &self.taint.sticky;
                st.set(st.get() | taint::VAL);
            }
        }
        // The block's value: the body's, or (when one of THIS block's arms
        // finished it) the arm's, whose accumulation unwound into this
        // expression's.
        let vt = match (self.seal.active, &result) {
            (false, _) => 0,
            (true, Err(Flow::HandlerDone(done))) if done.1 == depth => self.taint.acc.get(),
            (true, _) => self.t_last(),
        };
        self.handlers.borrow_mut().pop();
        // Phase 6 multi-shot: a non-tail / multi-resume arm reifies the
        // continuation by replay and finishes the WHOLE block with
        // `Flow::HandlerDone((v, depth))` — the original suspended body is
        // abandoned, so catch it here and use `v` as the block value (the
        // single-shot tail path never raises this, so its behavior is
        // unchanged). Only the completion addressed to this block's frame is
        // caught: one raised by an outer handler's arm passes through to the
        // `with` that installed it.
        let mut value = match result {
            Ok(v) => v,
            Err(Flow::HandlerDone(done)) if done.1 == depth => done.0,
            Err(e) => return Err(e),
        };

        // An inline `return(v) => e` arm rewrites the body's final value.
        if let Some(ra) = return_arm {
            let mut ra_env = self.env_from_snapshot(captured);
            ra_env.push();
            self.match_pattern_rt(&ra.binding, &value, &mut ra_env, vt)?;
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
            feed_t: if self.seal.active {
                self.t_stored(self.taint.acc.get())
            } else {
                0
            },
            sealed: ctx.sealed,
            operator_frames: self.operator_frames.get(),
        });
        let mut body_env = self.env_from_snapshot(ctx.env_snapshot);
        // SAFETY: `ctx` came from the live resume context, whose `with` is
        // still evaluating (`SrcRef`'s invariant).
        let body = unsafe { ctx.body.get() };
        // The replayed body is the installing fn's own text (amendment 100).
        let _pin_guard = crate::interp::PinGuard {
            cell: &self.pin_fn,
            prev: self.pin_fn.replace(ctx.pin_owner),
        };
        let result = self.eval(body, &mut body_env);
        // Disarm regardless of outcome so a later resume (or the arm's own code)
        // is not mistaken for a replay.
        *self.resume_replay.borrow_mut() = None;
        result
    }

    pub(super) fn run_handler_arm(&self, eff: &str, payload: Value) -> Result<Option<Value>, Flow> {
        nest_guard!(self, RUN_HANDLER_ARM);
        // (The replay-feed interception lives in the builtin dispatch — it must
        // fire even though the handler frame is split off during the arm, so it
        // cannot be gated on an active frame here.)

        // Find the INDEX of the nearest frame with a matching arm (search from
        // the top/innermost). Copy the arm data out so we don't hold the
        // RefCell borrow across arm evaluation. Also grab that frame's body +
        // env snapshot so a non-tail arm can replay the continuation
        // (multi-shot).
        let hit = {
            let stack = self.handlers.borrow();
            // A sealed frame is skipped (the search goes on outward) when the
            // operation is the operator's: see `handler_may_answer` (PSV-1).
            stack
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, f)| self.handler_may_answer(f.sealed, f.operator_frames))
                .find_map(|(i, frame)| {
                    frame.arms.iter().find(|a| a.effect() == eff).map(|a| {
                        (
                            i,
                            a.arm,
                            a.captured.clone(),
                            frame.body,
                            frame.env_snapshot.clone(),
                            frame.sealed,
                            frame.pin_owner,
                        )
                    })
                })
        };
        let Some((idx, arm, captured, with_body, with_env, arm_sealed, pin_owner)) = hit else {
            return Ok(None);
        };
        // SAFETY: the frame holding `arm` is live (it is split off below and
        // restored before return), so its `with` is still evaluating.
        let arm = unsafe { arm.get() };
        let (binding, arm_body) = (&arm.binding, &arm.body);

        // SHALLOW-handler semantics: the arm body runs OUTSIDE the handler it
        // belongs to. Temporarily remove the handling frame AND every frame
        // inner to it (they were installed inside the now-suspended body) so an
        // effect performed BY THE ARM is not re-intercepted by the same handler —
        // that self-interception is an infinite loop (a handler whose `on IO`
        // arm itself does IO). The removed frames are restored after the arm
        // runs, whether it resumes, aborts, or errors.
        let suspended: Vec<HandlerFrame> = self.handlers.borrow_mut().split_off(idx);
        // The operation's arguments are the arm's payload, with their taint.
        let pt = if self.seal.active {
            self.t_stored(self.taint.acc.get())
        } else {
            0
        };

        // BARE TAIL-RESUME FAST PATH (single-shot): when the arm body is exactly
        // `resume(<expr>)`, the operation yields that value and the suspended body
        // continues in place — no continuation reification needed. This is the
        // ONLY shape native codegen lowers, and the path the parity harness pins,
        // so it must stay byte-identical: evaluate the arm and propagate the
        // `Flow::Resume(v)` it raises directly (no resume_ctx, no replay).
        if crate::effects::arm_is_bare_tail_resume(arm_body) {
            let mut arm_env = self.env_from_snapshot(captured);
            arm_env.push();
            let bound = self.match_pattern_rt(binding, &payload, &mut arm_env, pt);
            // The arm runs under the provenance of the `with` that installed
            // it, and under the pin owner of the fn that installed it
            // (amendment 100).
            let outcome = self.handler_edge_into(arm_sealed, &payload).and_then(|()| {
                let _pin_guard = crate::interp::PinGuard {
                    cell: &self.pin_fn,
                    prev: self.pin_fn.replace(pin_owner),
                };
                self.with_frame(arm_sealed, || {
                    contain_loop_control(
                        bound.and_then(|_| self.eval(arm_body, &mut arm_env)),
                        FrameEdge::HandlerArm,
                    )
                })
            });
            self.handlers.borrow_mut().extend(suspended);
            return match outcome {
                Err(Flow::Resume(v)) => {
                    self.handler_edge_back(arm_sealed, &v)?;
                    Ok(Some(v))
                }
                Ok(v) => {
                    // The arm's value leaves the fn that performed the operation.
                    if self.seal.active {
                        self.taint.ret.set(self.t_last());
                    }
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
        let mut arm_env = self.env_from_snapshot(captured);
        arm_env.push();
        let bound = self.match_pattern_rt(binding, &payload, &mut arm_env, pt);
        let outcome = self.handler_edge_into(arm_sealed, &payload).and_then(|()| {
            let _pin_guard = crate::interp::PinGuard {
                cell: &self.pin_fn,
                prev: self.pin_fn.replace(pin_owner),
            };
            self.with_frame(arm_sealed, || {
                contain_loop_control(
                    bound.and_then(|_| self.eval(arm_body, &mut arm_env)),
                    FrameEdge::HandlerArm,
                )
            })
        });
        self.resume_ctx.borrow_mut().pop();
        self.handlers.borrow_mut().extend(suspended);

        match outcome {
            // The arm ran to completion. Whether it resumed zero times (abort) or
            // one-or-more times (each `resume` was a replay returning a value),
            // the arm's final value `v` is the value of the WHOLE `with` block.
            // The original suspended body is abandoned (the continuation was
            // reified via replay), so finish the block with HandlerDone —
            // addressed to the frame at `idx`, caught by the `eval_with_handler`
            // that pushed it and by no `with` in between.
            Ok(v) => Err(Flow::HandlerDone(Box::new((v, idx)))),
            // A stray tail `resume` that escaped without a ctx (shouldn't happen
            // on this path, but be safe): treat as a single-shot resume.
            Err(Flow::Resume(v)) => Ok(Some(v)),
            // E1314 and any other control flow (panic, exit, …) propagate.
            Err(other) => Err(other),
        }
    }

    /// `&&`/`||` in a sealed run: the left operand decides whether the right
    /// one runs, a branch like `if`. What the right operand STORES carries the
    /// left's taint, and a right operand that can leave the fn or loop keeps
    /// everything after it control-dependent too, taken or not (amendment 114).
    /// Kept out of `eval_binop` so the ordinary run's `&&` is the code it
    /// always was.
    #[cold]
    #[inline(never)]
    fn short_circuit_tainted(&self, op: &BinOp, lv: Value, right: &Expr, env: &mut Env) -> R {
        let ct = self.tl::<true>();
        if ct != 0 {
            self.t_control_val_only();
        }
        let exits = ct != 0 && taint::has_exit(right);
        if short_circuits(op, &lv) {
            // A branch not taken: when it could exit, whether it ran is still
            // the left's bit.
            if exits {
                self.t_branch(ct, exits, || Ok(()))?;
            }
            return Ok(lv);
        }
        let rv = self.t_branch(ct, exits, || self.eval_t::<true>(right, env))?;
        logic_rhs(op, lv, rv)
    }

    pub(super) fn eval_binop<const T: bool>(
        &self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        env: &mut Env,
    ) -> R {
        nest_guard!(self, EVAL_BINOP);
        // Short-circuit boolean operators. An `Uncertain<bool>` operand can't
        // short-circuit (we must combine confidences), so it falls through to
        // the value-level path which propagates uncertainty.
        if matches!(op, BinOp::And | BinOp::Or) {
            let lv = self.eval_t::<T>(left, env)?;
            if T {
                return self.short_circuit_tainted(op, lv, right, env);
            }
            if short_circuits(op, &lv) {
                return Ok(lv);
            }
            let rv = self.eval_t::<T>(right, env)?;
            return logic_rhs(op, lv, rv);
        }
        if T {
            // No in-place operand read: each operand goes through the
            // evaluator so its taint is accumulated.
            let l = self.eval_t::<T>(left, env)?;
            let lt = self.tl::<T>();
            let r = self.eval_t::<T>(right, env)?;
            let rt = self.tl::<T>();
            // The width edges (R51 S3): `T` is set only in a sealed run, so
            // the unsealed instantiation carries none of this.
            self.seal_width(op, left, right, &l, &r)?;
            // A comparison READS the content of every shared object (dict,
            // channel) inside either operand, however deep (`d == e`,
            // `[d] == [e]`, a struct or tuple holding one): the answer is a
            // function of what sealed code wrote there (amendment 108).
            if matches!(
                op,
                BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq
            ) {
                self.t_touch(self.t_obj_deep(&l) | self.t_obj_deep(&r));
            }
            if (lt | rt) & taint::TYP != 0
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
            return binop_operands(op, Operand::of(l), Operand::of(r));
        }
        let l = self.operand::<T>(left, env)?;
        let r = self.operand::<T>(right, env)?;
        binop_operands(op, l, r)
    }

    /// Evaluate binary-operation operand `e` (AX-55). A local or a numeric
    /// literal is read in place: an int or float is copied out of its slot or
    /// literal without building, cloning or dropping a `Value`. Anything else
    /// is evaluated as usual. Reading a local is what `eval` does for it.
    #[inline(always)]
    fn operand<const T: bool>(&self, e: &Expr, env: &mut Env) -> Result<Operand, Flow> {
        match e {
            Expr::Ident(name) => {
                let (s, slot) = self.res.var(e, name);
                if let Some(v) = env.get_var(s, slot) {
                    return Ok(Operand::of_ref(v));
                }
            }
            Expr::Literal(Literal::Int(n)) => return Ok(Operand::Int(*n)),
            Expr::Literal(Literal::Float(f)) => return Ok(Operand::Float(*f)),
            _ => {}
        }
        self.eval_t::<T>(e, env).map(Operand::of)
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
    pub(super) fn assign_in_place<const T: bool>(
        &self,
        s: Sym,
        slot: u32,
        name: &str,
        value: &Expr,
        env: &mut Env,
    ) -> Result<bool, Flow> {
        nest_guard!(self, ASSIGN_IN_PLACE);
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
                    [Expr::Ident(x), operand] if x == name => {
                        (op, operand, Some((f, tier, self.res.var(callee, f))))
                    }
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        if !matches!(env.get_var(s, slot), Some(Value::Str(_) | Value::Array(_)))
            || mentions_var(operand, name)
        {
            return Ok(false);
        }
        if let Some((f, _, (fs, fslot))) = call {
            // `eval_call` would run a local closure of that name instead, or take
            // the `&mut` path; the operand must not be able to change which.
            if matches!(env.get_var(fs, fslot), Some(Value::Closure { .. }))
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
        // The append puts `y` into the container `x` holds: the binding now
        // carries `y`'s taint and the control it was written under.
        let yt = self.ts::<T>(self.tl::<T>());
        if let Some((_, tier, _)) = call {
            *self.current_call_tier.borrow_mut() = tier.clone();
        }
        let Some(slot) = env.get_var_mut_t::<T>(s, slot, yt) else {
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
                    (AppendOp::Push | AppendOp::Concat, Some((f, _, _))) => self
                        .call_builtin(f, &[x, y])?
                        .expect("arr_push/arr_concat are builtins"),
                    _ => eval_binop_vals(&BinOp::Add, x, y)?,
                };
            }
        }
        Ok(true)
    }

    /// Whether `x = value` (`name` is `x`) has one of the shapes
    /// [`Interp::assign_in_place`] matches by syntax; for any other shape it
    /// returns `Ok(false)` without reading or evaluating anything, so the
    /// bytecode engine (R50) does not call it.
    pub(super) fn assign_in_place_shaped(name: &str, value: &Expr) -> bool {
        match value {
            Expr::BinOp {
                op: BinOp::Add,
                left,
                ..
            } => matches!(left.as_ref(), Expr::Ident(x) if x == name),
            Expr::Call { callee, args, .. } => {
                matches!(callee.as_ref(), Expr::Ident(f) if f == "arr_push" || f == "arr_concat")
                    && matches!(args.as_slice(), [Expr::Ident(x), _] if x == name)
            }
            _ => false,
        }
    }

    /// `name[idx]` on a variable bound locally or in `globals`, read in place
    /// (R50 S2, the `Index` arm's in-place path): evaluating the variable
    /// first would copy the whole array per element read, which makes
    /// in-place algorithms over `&mut [T]` (AX-08) quadratic. The caller made
    /// the existence test, then evaluated and converted the index
    /// (`strict_int`).
    pub(super) fn index_in_place(&self, s: Sym, slot: u32, name: &str, idx: i64, env: &Env) -> R {
        let v = match env.get_var(s, slot) {
            Some(v) => Some(v),
            None => self.global_ref(s)?,
        };
        match v {
            Some(Value::Array(items)) => items.get(idx as usize).cloned().ok_or_else(|| {
                Flow::Panic(format!("index {idx} out of bounds (len {})", items.len()).into())
            }),
            Some(other) => panic(format!("indexing non-array ({})", other.type_name())),
            None => self.no_such_fn(name, format!("undefined identifier `{name}`")),
        }
    }

    /// `name.field` (`f` is `field` interned) on a variable receiver, read in
    /// place (R50 S2, the `FieldAccess` arm's `Ident` path): only the field is
    /// cloned, not the whole record. `get_var`, then `globals`, then the
    /// undefined-identifier panic (no fn-as-value step), then `field_of`.
    pub(super) fn field_in_place(
        &self,
        s: Sym,
        slot: u32,
        name: &str,
        f: Sym,
        field: &str,
        env: &Env,
    ) -> R {
        let v = match env.get_var(s, slot) {
            Some(v) => v,
            None => match self.global_ref(s)? {
                Some(v) => v,
                None => return self.no_such_fn(name, format!("undefined identifier `{name}`")),
            },
        };
        field_of(v, f, field)
    }

    /// The rest of a non-empty struct or enum literal `name { .. }` once its
    /// field values are evaluated (R50 S2, the `StructLit` arm after field
    /// evaluation): `vals` holds them in source order and is left holding
    /// units. Each value lands in its slot of the definition's order, a field
    /// declared as a non-i64 integer width is coerced to `SizedInt` (R19
    /// Slice B), then an enum variant is built, or a struct after its
    /// per-field and whole-struct refinement checks.
    pub(super) fn finish_record(&self, lit: &RecordLit, name: &str, vals: &mut [Value]) -> R {
        nest_guard!(self, FINISH_RECORD);
        // In a sealed run a value of another width is left for `cast_fields`.
        let sealed = self.seal.active;
        let take = |v: &mut Value, width: Option<IntWidth>| {
            let v = std::mem::replace(v, Value::Unit);
            match width {
                Some(w) if sealed => coerce_to_sized::<true>(v, w),
                Some(w) => coerce_to_sized::<false>(v, w),
                None => v,
            }
        };
        let mut out: Vec<(Sym, Value)> = Vec::with_capacity(lit.names.len());
        if lit.in_order {
            for (v, &(_, width)) in vals.iter_mut().zip(lit.slots.iter()) {
                out.push((lit.names[out.len()], take(v, width)));
            }
        } else {
            let mut slots: Vec<Option<Value>> = vec![None; lit.names.len()];
            for (v, &(idx, width)) in vals.iter_mut().zip(lit.slots.iter()) {
                slots[idx as usize] = Some(take(v, width));
            }
            out.extend(
                lit.names
                    .iter()
                    .zip(slots)
                    .map(|(&n, v)| (n, v.expect("every named field is given"))),
            );
        }
        let mut fields = Fields(out);
        // A field is a declared type: in a sealed run each value is cast to it
        // (amendment 53; veto eval.rs:842-850).
        if self.seal.active {
            self.cast_fields(name, &mut fields)?;
        }
        let (sname, fields) = match lit.kind {
            RecordKind::Enum(enum_name, variant) => {
                return Ok(Value::Enum(Rc::new(EnumVal {
                    enum_name,
                    variant,
                    fields,
                })));
            }
            RecordKind::Struct(sname) => (sname, fields),
        };
        // Phase 5: refinement obligations at struct CONSTRUCTION (the
        // dual of the param/return checks), for non-constant values the
        // checker (E1209) only discharges for constants. Two checks,
        // both reusing the refinement-predicate evaluator with `_`
        // bound to the relevant value. A whole-struct `where` lives on
        // the TypeDef (not a RefineDef), so gate on the TypeDef having
        // a refinement OR the program having named refinements (for
        // refined fields), not on `refine_preds` alone. In a sealed run each
        // predicate's `_` carries what the field values were computed with
        // (their union, in `acc`; veto eval.rs:879, 902).
        if let Some(td) = self.structs.get(name).copied() {
            if td.refinement.is_some() || !self.refine_preds.is_empty() {
                // (1) per-FIELD refinement: each field whose declared
                // type is a refinement must satisfy that predicate.
                for tf in &td.fields {
                    if let crate::ast::AxonType::Named(rn) = &tf.ty {
                        if let Some(pred) = self.refine_preds.get(rn.as_str()).copied() {
                            self.seal_refine(rn)?;
                            if let Some(fv) = fields.named(&tf.name) {
                                let mut pe = self.new_env();
                                pe.define(SYM_UNDERSCORE, fv.clone(), self.t_acc());
                                if let Value::Bool(false) =
                                    contain_frame(self.eval(pred, &mut pe), FrameEdge::Predicate)?
                                {
                                    return Err(Flow::RefineViolation(
                                                format!(
                                                    "field `{}` of `{}` (= {}) violates the refinement \
                                                     `{}` — the value does not satisfy the type's predicate",
                                                    tf.name,
                                                    name,
                                                    display(fv),
                                                    rn
                                                )
                                                .into(),
                                            ));
                                }
                            }
                        }
                    }
                }
                // (2) WHOLE-STRUCT refinement: `_` binds to the whole
                // instance and `_.field` projects, so build it first.
                if let Some(pred) = &td.refinement {
                    let sv = Value::record(sname, fields);
                    let mut pe = self.new_env();
                    pe.define(SYM_UNDERSCORE, sv.clone(), self.t_acc());
                    // A DEFINITION-owned predicate runs under its definition's
                    // provenance: a candidate struct's `where` ran unsealed
                    // when the OPERATOR built one, and called operator code
                    // from there (PCI candidate-4 review; veto eval.rs:908).
                    let held = if self.seal.active {
                        let sealed = self.frame_sealed.get() || self.seal_type(name);
                        self.with_frame(sealed, || {
                            contain_frame(self.eval(pred, &mut pe), FrameEdge::Predicate)
                        })?
                    } else {
                        contain_frame(self.eval(pred, &mut pe), FrameEdge::Predicate)?
                    };
                    if let Value::Bool(false) = held {
                        return Err(Flow::RefineViolation(
                            format!(
                                "the constructed `{name}` violates its struct refinement \
                                         — the value does not satisfy the type's predicate"
                            )
                            .into(),
                        ));
                    }
                    return Ok(sv);
                }
            }
        }
        Ok(Value::record(sname, fields))
    }

    /// Cast each field value of a record literal `name { .. }` to its declared
    /// field type (sealed runs only, from [`Interp::finish_record`]): a field
    /// is a declared type (amendment 53, veto eval.rs:842-850).
    #[inline(never)]
    fn cast_fields(&self, name: &str, fields: &mut Fields) -> Result<(), Flow> {
        for (n, v) in fields.0.iter_mut() {
            let fname = sym_name(*n);
            if let Err(why) = self.cast_field(name, &fname, v) {
                return panic(format!(
                    "field `{fname}` of `{name}` was given {} — a runtime type confusion \
                     ({why})",
                    display(v)
                ));
            }
        }
        Ok(())
    }

    /// The rest of an identifier read once `get_var` found no local: a
    /// module-level `let`, then a top-level fn as a value, then the
    /// undefined-identifier panic. Shared by the `Ident` arm and the bytecode
    /// engine's local reads (R50).
    pub(super) fn ident_unbound(&self, name: &str, s: Sym) -> R {
        if let Some(v) = self.global_ref(s)? {
            Ok(v.clone())
        } else if let Some(&i) = self.fn_of_sym.get(&s) {
            let f = self.fn_table[i as usize].def;
            // AX-25: a top-level fn named in VALUE position (`let g = f`,
            // `[f, h]`, `apply(f, x)`) is a first-class closure. It is the
            // forwarding lambda `|a0, ..| f(a0, ..)` with no captures, so a
            // call through the value re-enters `eval_call` by NAME and takes
            // exactly the path a direct `f(..)` call takes — contracts,
            // `@[verify]` gates, effect/capability gates and provenance
            // included. The resolver refuses builtins and generic fns here.
            // In a sealed run, a fn value a SEALED frame took is marked
            // (veto eval.rs:175-179). PCI: naming a fn in value position is a
            // REFERENCE to it, so the call edge applies HERE: a sealed frame
            // may not take an operator fn as a value (the forwarding body would
            // run it in an operator frame, where the edge no longer applies).
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

    // ── Pattern matching ─────────────────────────────────────────────────────

    /// Try to match `val` against `pat`, binding identifiers into the current
    /// scope of `env`, each carrying the subject's taint `t` (a destructured
    /// part is the subject's). Returns whether it matched.
    pub(super) fn match_pattern<const T: bool>(
        &self,
        pat: &Pattern,
        val: &Value,
        env: &mut Env,
        t: u8,
    ) -> Result<bool, Flow> {
        match pat {
            Pattern::Wildcard => Ok(true),
            Pattern::Ident(name) => {
                let (s, slot) = self.res.pat_var(pat, name);
                env.define_var_t::<T>(s, slot, val.clone(), t);
                Ok(true)
            }
            Pattern::Literal(lit) => Ok(values_equal(&lit_to_val(lit), val)),
            Pattern::Some(inner) => match val {
                Value::Some(v) => self.match_pattern::<T>(inner, v, env, t),
                _ => Ok(false),
            },
            Pattern::None => Ok(matches!(val, Value::None)),
            Pattern::Ok(inner) => match val {
                Value::Ok(v) => self.match_pattern::<T>(inner, v, env, t),
                _ => Ok(false),
            },
            Pattern::Err(inner) => match val {
                Value::Err(v) => self.match_pattern::<T>(inner, v, env, t),
                _ => Ok(false),
            },
            Pattern::Struct { name, fields } => {
                // Enum-variant pattern when the name is qualified (`Enum::Variant`).
                // AX-47: names compare as syms resolved once; a pattern
                // outside the table (an AST clone) resolves here.
                let unresolved;
                let rp = match self.res.record_pat(pat) {
                    Some(rp) => rp,
                    None => {
                        unresolved = RecordPat::of(name, fields);
                        &unresolved
                    }
                };
                let Some(field_map) = rp.kind.fields_of(val) else {
                    return Ok(false);
                };
                for ((_, fpat), &fname) in fields.iter().zip(rp.fields.iter()) {
                    let Some(fval) = field_map.get(fname) else {
                        return Ok(false);
                    };
                    let fval = fval.clone();
                    if !self.match_pattern::<T>(fpat, &fval, env, t)? {
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
                    if !self.match_pattern::<T>(p, &v, env, t)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    /// [`Interp::match_pattern`] for a caller that does not know which run it
    /// is in (the effect-handler arms): the run picks the instantiation.
    pub(super) fn match_pattern_rt(
        &self,
        pat: &Pattern,
        val: &Value,
        env: &mut Env,
        t: u8,
    ) -> Result<bool, Flow> {
        if self.seal.active {
            self.match_pattern::<true>(pat, val, env, t)
        } else {
            self.match_pattern::<false>(pat, val, env, 0)
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
                if let Value::Struct(s) = &dist_val {
                    if sym_is(s.name, "Categorical") {
                        if let Some(Value::Array(probs)) = s.fields.named("probs") {
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
    let Value::Struct(s) = dist else {
        return None;
    };
    let fields = &s.fields;
    match &*sym_name(s.name) {
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
    let Value::Struct(s) = dist else {
        return None;
    };
    let fields = &s.fields;
    match &*sym_name(s.name) {
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

/// `arr[idx]` on an evaluated receiver (R50 S2, the `Index` arm's general
/// path, after the index went through `strict_int`).
pub(super) fn index_value(arr: Value, idx: i64) -> R {
    match arr {
        Value::Array(items) => items.get(idx as usize).cloned().ok_or_else(|| {
            Flow::Panic(format!("index {idx} out of bounds (len {})", items.len()).into())
        }),
        other => panic(format!("indexing non-array ({})", other.type_name())),
    }
}

/// `v.field` (`f` is `field` interned) for a struct/enum field or a tuple's
/// `.N`: clones only the selected component.
pub(super) fn field_of(v: &Value, f: Sym, field: &str) -> R {
    let fields = match v {
        Value::Struct(s) => &s.fields,
        Value::Enum(e) => &e.fields,
        _ => return field_of_tuple(v, field),
    };
    fields
        .get(f)
        .cloned()
        .ok_or_else(|| Flow::Panic(format!("no field `{field}`").into()))
}

/// [`field_of`] on a non-record value.
fn field_of_tuple(v: &Value, field: &str) -> R {
    match v {
        Value::Tuple(items) => {
            // `t.0`, `t.1`, … : the parser stores the digit as the
            // field name, and the interpreter reads it as the index.
            let i: usize = field.parse().map_err(|_| {
                Flow::Panic(format!("tuple access expects a numeric index, got `.{field}`").into())
            })?;
            items.get(i).cloned().ok_or_else(|| {
                Flow::Panic(format!("tuple index {i} out of bounds (len {})", items.len()).into())
            })
        }
        other => panic(format!(
            "field access on non-struct ({})",
            other.type_name()
        )),
    }
}

fn get_f64_field(fields: &Fields, key: &str) -> Option<f64> {
    match fields.named(key)? {
        Value::Float(f) => Some(*f),
        Value::Int(n) => Some(*n as f64),
        _ => None,
    }
}

fn get_f64_array_field(fields: &Fields, key: &str) -> Option<Vec<f64>> {
    match fields.named(key)? {
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
