//! The builtin dispatch for the interpreter (R0 slice 6 — extracted verbatim
//! from `interp.rs`, zero behavior change). `call_builtin` is the ~2400-line
//! `match name { … }` that IS the I-2 reference semantics for every builtin
//! (println, arr_*, dict_*, str_*, ai_*, goal_*, …). Moved as ONE method into
//! a second inherent-impl block; its function-local `want` closure and `ok!`
//! macro travel with it unchanged (no promotion needed — the finer category
//! split the R0 spec floated is deliberately NOT done, keeping this a pure
//! move). `self.<method>` (incl. the already-split goal.rs methods) and the
//! parent's private `Interp` fields resolve across the module boundary;
//! `use super::*` pulls in Value/Flow + the free helpers (display, as_*,
//! emit_stdout, fmt_g, next_rand_u64, append_*_jsonl, values_equal, …).

use super::*;

/// F3 (Phase 9): map a raw capability kind (from `capability_of_builtin`) to its
/// effect-row tag for audit records. Unmapped kinds default to the raw cap name.
fn cap_to_effect_row(cap: &str) -> &'static str {
    match cap {
        "net" | "ai" => "Net",
        "fs" => "FS",
        "exec" => "Exec",
        _ => "Other",
    }
}

/// Phase 7 (R12 Slice 4): the durable-store NDJSON log path for store `key`:
/// `$XDG_CACHE_HOME/axon/stores/<key>.ndjson` (or under `$HOME/.cache`). Sibling
/// of the provenance log dir, so it reuses the same cache-root discovery. The key
/// is sanitized to a safe filename (non-alnum → `_`) so a store name can't escape
/// the stores dir. Returns None if no cache root is discoverable.
fn store_log_path(key: &str) -> Option<std::path::PathBuf> {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("HOME")
                .ok()
                .map(|h| std::path::PathBuf::from(h).join(".cache"))
        })?;
    let safe: String = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = if safe.is_empty() {
        "_".to_string()
    } else {
        safe
    };
    Some(
        base.join("axon")
            .join("stores")
            .join(format!("{safe}.ndjson")),
    )
}

impl<'p> Interp<'p> {
    // ── Builtins ───────────────────────────────────────────────────────────────

    /// Phase 7 (R12 Slice 2/3): run every currently-READY scheduler fiber to
    /// completion in the seed-deterministic order, catching per-fiber panics
    /// (recorded as failed, not fatal). Returns the count that completed
    /// successfully. Shared by the `scheduler_run` builtin and the live
    /// `supervisor_run` loop. A whole-program `exit`/`Halt` from a fiber still
    /// propagates (only per-fiber Panic/RefineViolation/VerifyFailed are caught).
    pub(super) fn builtin_scheduler_run_once(&self) -> Result<i64, Flow> {
        let order = self.scheduler.borrow().ready_order();
        let mut completed: i64 = 0;
        for id in order {
            let Some((fn_name, arg)) = self.scheduler.borrow().fiber_call(id) else {
                continue;
            };
            let outcome = match self.fns.get(fn_name.as_str()).copied() {
                Some(f) => {
                    let fiber_args = if f.params.is_empty() {
                        vec![]
                    } else {
                        vec![Value::Int(arg)]
                    };
                    self.call_fn(f, fiber_args)
                }
                None => Err(Flow::Panic(format!("fiber fn `{fn_name}` vanished"))),
            };
            match outcome {
                Ok(v) => {
                    let r = numeric_score(&v).map(|s| s as i64).unwrap_or(0);
                    self.scheduler.borrow_mut().complete(id, r);
                    completed += 1;
                }
                Err(Flow::Panic(m))
                | Err(Flow::RefineViolation(m))
                | Err(Flow::VerifyFailed(m)) => {
                    self.scheduler.borrow_mut().fail(id, m);
                }
                Err(other) => return Err(other),
            }
        }
        self.scheduler.borrow_mut().passes += 1;
        Ok(completed)
    }

    /// Dispatch a builtin call. Returns `Ok(Some(v))` if `name` is a builtin,
    /// `Ok(None)` if it is not (caller should try user functions).
    pub(super) fn call_builtin(&self, name: &str, args: &[Value]) -> Result<Option<Value>, Flow> {
        // Helpers --------------------------------------------------------------
        let want = |n: usize| -> Result<(), Flow> {
            if args.len() == n {
                Ok(())
            } else {
                Err(Flow::Panic(format!(
                    "{name}: expected {n} args, got {}",
                    args.len()
                )))
            }
        };
        macro_rules! ok {
            ($v:expr) => {
                return Ok(Some($v))
            };
        }

        // R4 §4.3 — mandatory `@[agent]` action log (I-13). When a capability-
        // bearing builtin is called from inside an `@[agent]` fn, inject one
        // `agent_action` audit record naming the tool and the capability it
        // exercises. Compiler-injected at the call site, so an agent cannot act
        // on the world (fs/net/exec) without the action being logged — the
        // highest-trust zone's un-opt-out-able audit trail. Pure builtins
        // (no capability) are not logged; non-agent callers are unaffected.
        if let Some(cap) = crate::capabilities::capability_of_builtin(name) {
            if let Some(agent_fn) = self.current_agent_fn() {
                // F3 (Phase 9): map the raw cap kind to its effect-row tag and
                // include the current principal name for audit attribution.
                let effect_row = cap_to_effect_row(cap);
                let principal = self.current_principal_name();
                append_agent_action_jsonl(&agent_fn, name, cap, effect_row, &principal);
            }
        }

        // F5 (Phase 9): runtime sandbox enforcement. If there is an active
        // sandbox AND this builtin has a non-empty effect row, check that every
        // effect it requires is in the sandbox's allowed set. Any effect outside
        // the ceiling is refused (SandboxViolation, exit 8) before the real call.
        // Pure builtins (empty row) are always allowed — this check costs nothing
        // for the common case. sandbox_create/sandbox_run themselves are exempt
        // (they manage sandbox state; exempting them avoids infinite regress).
        {
            let sb_handle = self.active_sandbox.get();
            if sb_handle >= 0 && name != "sandbox_create" && name != "sandbox_run" {
                let row = crate::builtins::builtin_effect_row(name);
                if !row.is_empty() {
                    let sbs = self.sandboxes.borrow();
                    if let Some(sb) = sbs.get(sb_handle as usize) {
                        for &eff in row {
                            if !sb.allowed.contains(eff) {
                                return Err(crate::interp::Flow::SandboxViolation(format!(
                                    "builtin `{name}` requires effect `{eff}` which is not \
                                     in the active sandbox's allowed set {:?} \
                                     (principal handle {})",
                                    sb.allowed, sb.principal
                                )));
                            }
                        }
                    }
                }
            }
        }

        // Phase 6 (multi-shot resume): if we are REPLAYING a continuation to
        // service a `resume(v)`, the handled effect's op is FED the resume value
        // instead of really running — this is what makes the continuation
        // resumable without a CPS rewrite. The handler frame was split off during
        // the arm (shallow semantics), so this check must run BEFORE the
        // frame-based interception below and independently of any active frame.
        // The first hit of the replay's effect consumes the feed; a second effect
        // during the same replay is unsound to re-fire → E1314. Only effect-
        // bearing builtins can be fed (a pure builtin during replay runs
        // normally — re-running pure code is exact).
        {
            let mut replay = self.resume_replay.borrow_mut();
            if let Some(r) = replay.as_mut() {
                let row = crate::builtins::builtin_effect_row(name);
                if row.iter().any(|e| r.effect == **e) {
                    // The handled effect's op. The first hit consumes the feed
                    // (the resume value); a second hit can't be soundly re-fired.
                    if !r.consumed {
                        r.consumed = true;
                        return Ok(Some(r.feed.clone()));
                    }
                    return Err(crate::interp::Flow::MultiShotUnsound(format!(
                        "effect `{}` (via `{name}`) is performed a second time during a \
                         handler-continuation replay; multi-shot `resume` is supported only \
                         when the handled body performs exactly one effect and is otherwise \
                         pure (a side effect cannot be re-executed on replay) [E1314]",
                        r.effect
                    )));
                } else if !row.is_empty() {
                    // A DIFFERENT effect during the replay also can't be re-fired.
                    return Err(crate::interp::Flow::MultiShotUnsound(format!(
                        "effect `{}` (via `{name}`) is performed during a handler-continuation \
                         replay for a different effect; multi-shot `resume` requires the handled \
                         body to be pure after its single intercepted op [E1314]",
                        row[0]
                    )));
                }
                // Pure builtin (empty row) during replay: fall through and run it
                // normally — re-running pure code is exact.
            }
        }

        // Phase 6: effect-handler interception (tail-resumptive, single-shot).
        // If this builtin carries an effect that an enclosing `with handler`
        // frame handles, the arm intercepts the operation: a `resume(v)` in the
        // arm makes `v` the builtin's result (the real operation is SKIPPED — its
        // effect is discharged); an arm that returns without resuming replaces
        // the whole `with` block. Only builtins with a non-empty effect row can
        // be intercepted, and only when a matching arm is active — so a program
        // with no handlers (or no matching effect) runs the builtin normally,
        // exactly as before. The payload bound in the arm is the builtin's args:
        // the single arg as-is, or a tuple for 0/2+ args.
        if !self.handlers.borrow().is_empty() {
            for eff in crate::builtins::builtin_effect_row(name) {
                let handled = self
                    .handlers
                    .borrow()
                    .iter()
                    .any(|f| f.arms.iter().any(|a| a.effect == *eff));
                if !handled {
                    continue;
                }
                let payload = match args.len() {
                    1 => args[0].clone(),
                    _ => Value::Tuple(args.to_vec()),
                };
                if let Some(v) = self.run_handler_arm(eff, payload)? {
                    return Ok(Some(v));
                }
            }
        }

        match name {
            // ── I/O ───────────────────────────────────────────────────────────
            "print" => {
                want(1)?;
                emit_stdout(&display(&args[0]), false);
                ok!(Value::Unit);
            }
            "println" => {
                want(1)?;
                emit_stdout(&display(&args[0]), true);
                ok!(Value::Unit);
            }
            "eprint" => {
                want(1)?;
                eprint!("{}", display(&args[0]));
                ok!(Value::Unit);
            }
            "eprintln" => {
                want(1)?;
                eprintln!("{}", display(&args[0]));
                ok!(Value::Unit);
            }
            "read_line" => {
                want(0)?;
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(_) => {
                        while line.ends_with('\n') || line.ends_with('\r') {
                            line.pop();
                        }
                        ok!(Value::Str(line));
                    }
                    Err(e) => ok!(Value::Str(format!("<read error: {e}>"))),
                }
            }
            "read_file" => {
                want(1)?;
                let path = as_str(&args[0])?.to_string();
                match crate::host::with_host(|h| h.read_file(&path)) {
                    Ok(s) => ok!(Value::Ok(Box::new(Value::Str(s)))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e)))),
                }
            }
            "write_file" => {
                want(2)?;
                let path = as_str(&args[0])?.to_string();
                let data = as_str(&args[1])?.to_string();
                match crate::host::with_host(|h| h.write_file(&path, &data)) {
                    Ok(()) => ok!(Value::Ok(Box::new(Value::Unit))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e)))),
                }
            }
            "exec" => {
                want(2)?;
                let cmd = as_str(&args[0])?.to_string();
                // args is `[str]`; collect into Vec<String> (a non-str element is
                // a type error the checker should have caught).
                let arg_list: Vec<String> = match &args[1] {
                    Value::Array(xs) => {
                        let mut out = Vec::with_capacity(xs.len());
                        for x in xs {
                            out.push(as_str(x)?.to_string());
                        }
                        out
                    }
                    other => {
                        return panic(format!(
                            "exec: args must be [str], got {}",
                            other.type_name()
                        ))
                    }
                };
                match crate::host::with_host(|h| h.exec(&cmd, &arg_list)) {
                    Ok(s) => ok!(Value::Ok(Box::new(Value::Str(s)))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e)))),
                }
            }
            "http_get" => {
                want(2)?;
                let url = as_str(&args[0])?.to_string();
                let headers = as_str(&args[1])?.to_string();
                match crate::host::with_host(|h| h.http_get(&url, &headers)) {
                    Ok(s) => ok!(Value::Ok(Box::new(Value::Str(s)))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e)))),
                }
            }
            "http_post" => {
                want(3)?;
                let url = as_str(&args[0])?.to_string();
                let headers = as_str(&args[1])?.to_string();
                let body = as_str(&args[2])?.to_string();
                match crate::host::with_host(|h| h.http_post(&url, &headers, &body)) {
                    Ok(s) => ok!(Value::Ok(Box::new(Value::Str(s)))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e)))),
                }
            }
            "http_sse" => {
                want(3)?;
                let url = as_str(&args[0])?.to_string();
                let headers = as_str(&args[1])?.to_string();
                let callback = args[2].clone();
                let events = match crate::host::with_host(|h| h.http_sse(&url, &headers)) {
                    Ok(v) => v,
                    Err(e) => {
                        ok!(Value::Err(Box::new(Value::Str(e))))
                    }
                };
                let count = events.len() as i64;
                for event in events {
                    self.call_closure(callback.clone(), vec![Value::Str(event)])?;
                }
                ok!(Value::Ok(Box::new(Value::Int(count))))
            }
            "http_sse_post" => {
                want(4)?;
                let url = as_str(&args[0])?.to_string();
                let headers = as_str(&args[1])?.to_string();
                let body = as_str(&args[2])?.to_string();
                let callback = args[3].clone();
                let events =
                    match crate::host::with_host(|h| h.http_sse_post(&url, &headers, &body)) {
                        Ok(v) => v,
                        Err(e) => {
                            ok!(Value::Err(Box::new(Value::Str(e))))
                        }
                    };
                let count = events.len() as i64;
                for event in events {
                    self.call_closure(callback.clone(), vec![Value::Str(event)])?;
                }
                ok!(Value::Ok(Box::new(Value::Int(count))))
            }

            // ── Conversion / formatting ─────────────────────────────────────────
            "to_str" => {
                want(1)?;
                // Polymorphic over scalars (BUG_HUNT #29): dispatch on the
                // runtime value so to_str(i64|f64|bool) all work. Int/Float/Bool
                // render identically to to_str / to_str_f64 / to_str_bool
                // respectively (display() shares fmt_g + "true"/"false").
                // R19 Slice B: SizedInt also renders via display().
                ok!(Value::Str(match &args[0] {
                    Value::Int(_) | Value::Float(_) | Value::Bool(_) | Value::SizedInt { .. } => {
                        display(&args[0])
                    }
                    other =>
                        return panic(format!(
                            "to_str: expected a scalar (i64/f64/bool), got {}",
                            other.type_name()
                        )),
                }));
            }
            "to_str_f64" => {
                want(1)?;
                ok!(Value::Str(fmt_g(as_float(&args[0])?)));
            }
            "to_str_bool" => {
                want(1)?;
                ok!(Value::Str(
                    if as_bool(&args[0])? { "true" } else { "false" }.to_string()
                ));
            }
            "i64_to_str" => {
                want(1)?;
                ok!(Value::Str(as_int(&args[0])?.to_string()));
            }
            "format" => {
                want(1)?;
                // Interpolation is lowered at parse time; `format` is the identity
                // on an already-interpolated string.
                ok!(Value::Str(as_str(&args[0])?.to_string()));
            }
            "parse_int" => {
                want(1)?;
                let s = as_str(&args[0])?;
                let t = s.trim();
                ok!(match t.parse::<i64>() {
                    Ok(n) => Value::Ok(Box::new(Value::Int(n))),
                    // Specific, actionable message (BUG_HUNT #22): echo the
                    // input and say what was expected. Radix prefixes are a
                    // common mistake — `parse_int` is base-10 only, so hint it.
                    Err(_) => {
                        let lower = t.to_ascii_lowercase();
                        let hint = if lower.starts_with("0x")
                            || lower.starts_with("0o")
                            || lower.starts_with("0b")
                        {
                            " (parse_int is base-10 only; strip the radix prefix)"
                        } else {
                            ""
                        };
                        Value::Err(Box::new(Value::Str(format!(
                            "could not parse `{s}` as a base-10 integer{hint}"
                        ))))
                    }
                });
            }
            // `parse_int_radix(s, base) -> Result<i64, str>` (BUG_HUNT #22):
            // the radix-aware counterpart to `parse_int` and the input-side
            // inverse of `i64_to_str_radix`. An out-of-range base or a bad
            // digit is a recoverable `Err`, never a panic.
            "parse_int_radix" => {
                want(2)?;
                let s = as_str(&args[0])?;
                let base = as_int(&args[1])?;
                if !(2..=36).contains(&base) {
                    ok!(Value::Err(Box::new(Value::Str(format!(
                        "parse_int_radix: base must be 2..=36, got {base}"
                    )))));
                }
                let t = s.trim();
                // Accept and strip a radix prefix that matches `base`
                // (0x→16, 0o→8, 0b→2), preserving a leading sign.
                let (sign, rest) = match t.strip_prefix('-') {
                    Some(r) => ("-", r),
                    None => ("", t),
                };
                let lower = rest.to_ascii_lowercase();
                let digits = match base {
                    16 if lower.starts_with("0x") => &rest[2..],
                    8 if lower.starts_with("0o") => &rest[2..],
                    2 if lower.starts_with("0b") => &rest[2..],
                    _ => rest,
                };
                let normalized = format!("{sign}{digits}");
                ok!(match i64::from_str_radix(&normalized, base as u32) {
                    Ok(n) => Value::Ok(Box::new(Value::Int(n))),
                    Err(_) => Value::Err(Box::new(Value::Str(format!(
                        "could not parse `{s}` as a base-{base} integer"
                    )))),
                });
            }
            "parse_float" => {
                want(1)?;
                let s = as_str(&args[0])?;
                ok!(match s.trim().parse::<f64>() {
                    Ok(f) => Value::Ok(Box::new(Value::Float(f))),
                    Err(_) => Value::Err(Box::new(Value::Str(format!(
                        "could not parse `{s}` as a float"
                    )))),
                });
            }
            "parse_bool" => {
                want(1)?;
                let s = as_str(&args[0])?;
                ok!(match s.trim() {
                    "true" => Value::Ok(Box::new(Value::Bool(true))),
                    "false" => Value::Ok(Box::new(Value::Bool(false))),
                    _ => Value::Err(Box::new(Value::Str(format!(
                        "could not parse `{s}` as a bool (expected `true` or `false`)"
                    )))),
                });
            }
            // Parse-with-default variants that fold the Result-match
            // ceremony away. Useful in load-from-disk paths where
            // a missing or malformed value should fall back silently
            // rather than propagate an Err the caller has to unwrap.
            "parse_int_or" => {
                want(2)?;
                let n = as_str(&args[0])?
                    .trim()
                    .parse::<i64>()
                    .unwrap_or(as_int(&args[1])?);
                ok!(Value::Int(n));
            }
            "parse_float_or" => {
                want(2)?;
                let f = as_str(&args[0])?
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(as_float(&args[1])?);
                ok!(Value::Float(f));
            }
            "parse_bool_or" => {
                want(2)?;
                let b = match as_str(&args[0])?.trim() {
                    "true" => true,
                    "false" => false,
                    _ => match &args[1] {
                        Value::Bool(b) => *b,
                        other => {
                            return panic(format!(
                                "parse_bool_or: default must be bool, got {}",
                                other.type_name()
                            ))
                        }
                    },
                };
                ok!(Value::Bool(b));
            }
            // Keep only ASCII digits; everything else is dropped. Closes
            // ROADMAP §9.5 F7 — gives string-shape demos (phone numbers,
            // codes, IDs) a one-liner alternative to pushing parsing onto
            // an LLM. Composes with parse_int: parse_int(str_digits_only(s)).
            "str_digits_only" => {
                want(1)?;
                let s = as_str(&args[0])?;
                let out: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
                ok!(Value::Str(out));
            }
            "len" => {
                want(1)?;
                ok!(match &args[0] {
                    Value::Str(s) => Value::Int(s.len() as i64),
                    Value::Array(a) => Value::Int(a.len() as i64),
                    other =>
                        return panic(format!(
                            "len: expected str/array, got {}",
                            other.type_name()
                        )),
                });
            }

            // ── Math ────────────────────────────────────────────────────────────
            "abs_i64" => {
                want(1)?;
                // checked — abs(i64::MIN) overflows. Match the native runtime's
                // graceful panic (`__axon_abs_i64`, same message, exit 101)
                // instead of a raw Rust `.abs()` "attempt to negate with
                // overflow" thread panic. The interpreter is the reference
                // semantics; a clean Flow::Panic is what native mirrors.
                let n = as_int(&args[0])?;
                match n.checked_abs() {
                    Some(v) => ok!(Value::Int(v)),
                    None => panic("abs_i64 overflow (i64::MIN has no positive)"),
                }
            }
            "abs_i32" => {
                want(1)?;
                // The value is held as i64 but the i32 builtin's domain is i32 —
                // mirror `__axon_abs_i32`: overflow exactly at i32::MIN.
                let n = as_int(&args[0])?;
                if n == i32::MIN as i64 {
                    return panic("abs_i32 overflow (i32::MIN has no positive)");
                }
                ok!(Value::Int((n as i32).unsigned_abs() as i64));
            }

            // ── Array helpers ─────────────────────────────────────────────────
            // Concrete-typed because the interpreter's array path holds
            // `Vec<Value>` and the inference layer wants concrete return shapes;
            // generic [T] forms wait on Phase-8 search-strategy work that also
            // teaches the optimizer about user-defined domains.

            // Half-open range `[start, end)`. Returns an empty slice when
            // `end <= start`. Saturates element count silently if asked for an
            // implausibly large range — caller should size with awareness.
            "arr_range" => {
                want(2)?;
                let start = as_int(&args[0])?;
                let end = as_int(&args[1])?;
                if end <= start {
                    ok!(Value::Array(Vec::new()));
                }
                let len = (end - start) as usize;
                let mut out = Vec::with_capacity(len.min(1 << 20));
                let mut i = start;
                while i < end && out.len() < (1 << 20) {
                    out.push(Value::Int(i));
                    i += 1;
                }
                ok!(Value::Array(out));
            }
            // Append: returns a fresh array with `x` at the end. Copy
            // semantics — the input array is unaffected.
            "arr_push" => {
                want(2)?;
                let mut xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_push: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                xs.push(args[1].clone());
                ok!(Value::Array(xs));
            }
            // Sum of an i64 array. Empty → 0. Saturates on overflow.
            "arr_sum_i64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_sum_i64: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let mut s: i64 = 0;
                for v in xs {
                    let n = match v {
                        Value::Int(n) => *n,
                        other => {
                            return panic(format!(
                                "arr_sum_i64: element must be i64, got {}",
                                other.type_name()
                            ))
                        }
                    };
                    s = s.saturating_add(n);
                }
                ok!(Value::Int(s));
            }
            // Map a closure / lambda across an array, producing a fresh
            // array of the results. The closure runs through `call_closure`
            // so it sees its captured environment, and any `Flow::Panic`
            // it raises is bubbled up — failures aren't swallowed.
            "arr_map" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_map: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let f = args[1].clone();
                let mut out = Vec::with_capacity(xs.len());
                for x in xs {
                    let mapped = self.call_closure(f.clone(), vec![x])?;
                    out.push(mapped);
                }
                ok!(Value::Array(out));
            }
            // Reduce an array to a single value via `f(acc, x) -> acc`.
            // The most general functional combinator — arr_sum_i64, arr_max,
            // arr_min, count, product, etc. are all special cases.
            "arr_fold" => {
                want(3)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_fold: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let mut acc = args[1].clone();
                let f = args[2].clone();
                for x in xs {
                    acc = self.call_closure(f.clone(), vec![acc, x])?;
                }
                ok!(acc);
            }
            // Sort an array via a comparator closure `(a, b) -> i64` with
            // standard cmp semantics (neg = a<b, 0 = eq, pos = a>b).
            // Stable sort (insertion-sort under the hood for simplicity);
            // not big-O optimal but plenty for ASI-scale arrays. Returns a
            // fresh sorted array — input untouched.
            "arr_sort_by" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_sort_by: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let cmp = args[1].clone();
                // Insertion sort. Each comparison hits call_closure;
                // O(n²) on length but the closure dispatch dominates so
                // a fancier algorithm wouldn't move the needle here.
                let mut out: Vec<Value> = Vec::with_capacity(xs.len());
                for x in xs {
                    let mut lo = 0;
                    let hi = out.len();
                    // Linear probe (binary search would re-run the cmp for
                    // already-sorted items; for typical ASI use n is small).
                    while lo < hi {
                        let r = self.call_closure(cmp.clone(), vec![x.clone(), out[lo].clone()])?;
                        let r = match r {
                            Value::Int(n) => n,
                            other => {
                                return panic(format!(
                                    "arr_sort_by: comparator must return i64, got {}",
                                    other.type_name()
                                ))
                            }
                        };
                        if r < 0 {
                            break;
                        }
                        lo += 1;
                    }
                    out.insert(lo, x);
                }
                ok!(Value::Array(out));
            }
            // Build an array by repeating `v` `n` times. Common need:
            // initialize a fresh array with a default value before mutating
            // it in place. Polymorphic via `T`.
            "arr_repeat" => {
                want(2)?;
                let v = args[0].clone();
                let n = as_int(&args[1])?.max(0) as usize;
                ok!(Value::Array(vec![v; n.min(1 << 20)]));
            }
            // Concatenate two arrays into a fresh one. Element types must
            // agree at the runtime — we don't do conversions here. The
            // result preserves order: `xs ++ ys`.
            "arr_concat" => {
                want(2)?;
                let mut out = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_concat: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let ys = match &args[1] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_concat: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                out.reserve(ys.len());
                for v in ys {
                    out.push(v.clone());
                }
                ok!(Value::Array(out));
            }
            // Flatten `[[T]] -> [T]`. Each inner element must itself be an
            // array; mixed shapes panic. Useful after `arr_map` produces
            // nested results.
            "arr_flatten" => {
                want(1)?;
                let xss = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_flatten: expected array of arrays, got {}",
                            other.type_name()
                        ))
                    }
                };
                let total: usize = xss
                    .iter()
                    .map(|v| match v {
                        Value::Array(inner) => inner.len(),
                        _ => 0,
                    })
                    .sum();
                let mut out = Vec::with_capacity(total);
                for v in xss {
                    match v {
                        Value::Array(inner) => {
                            for x in inner {
                                out.push(x.clone());
                            }
                        }
                        other => {
                            return panic(format!(
                                "arr_flatten: inner element must be array, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                ok!(Value::Array(out));
            }

            // Numeric `as`-style casts. Concrete builtins that work on
            // either i64 or f64 input — let the value's runtime type drive.
            // Pairs with the existing `i64_to_f64` / `f64_to_i64` but is
            // polymorphic on the source so ASI demos don't need to know
            // the source type at the call site.
            "as_f64" => {
                want(1)?;
                let f = match &args[0] {
                    Value::Int(n) => *n as f64,
                    Value::Float(v) => *v,
                    Value::Bool(b) => {
                        if *b {
                            1.0
                        } else {
                            0.0
                        }
                    }
                    other => {
                        return panic(format!(
                            "as_f64: expected i64/f64/bool, got {}",
                            other.type_name()
                        ))
                    }
                };
                ok!(Value::Float(f));
            }
            "as_i64" => {
                want(1)?;
                let n = match &args[0] {
                    Value::Int(n) => *n,
                    Value::Float(v) => *v as i64,
                    Value::Bool(b) => {
                        if *b {
                            1
                        } else {
                            0
                        }
                    }
                    other => {
                        return panic(format!(
                            "as_i64: expected i64/f64/bool, got {}",
                            other.type_name()
                        ))
                    }
                };
                ok!(Value::Int(n));
            }

            // ── Polymorphic slicing / reordering ──────────────────────────
            "arr_reverse" => {
                want(1)?;
                let mut xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_reverse: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                xs.reverse();
                ok!(Value::Array(xs));
            }
            "arr_take" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_take: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let n = as_int(&args[1])?.max(0) as usize;
                let take = n.min(xs.len());
                ok!(Value::Array(xs[..take].to_vec()));
            }
            "arr_drop" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_drop: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let n = as_int(&args[1])?.max(0) as usize;
                let skip = n.min(xs.len());
                ok!(Value::Array(xs[skip..].to_vec()));
            }

            // ── f64 array reductions (mirrors arr_*_i64) ──────────────────
            "arr_sum_f64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_sum_f64: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let mut s = 0.0_f64;
                for v in xs {
                    let f = match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        other => {
                            return panic(format!(
                                "arr_sum_f64: element must be numeric, got {}",
                                other.type_name()
                            ))
                        }
                    };
                    s += f;
                }
                ok!(Value::Float(s));
            }
            // Mean of an i64 array → f64 (almost never an integer). Empty
            // → 0.0 rather than NaN/panic; caller should guard if zero is
            // meaningful for them.
            "arr_mean_i64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_mean_i64: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                if xs.is_empty() {
                    ok!(Value::Float(0.0));
                }
                let mut s: i64 = 0;
                for v in xs {
                    let n = match v {
                        Value::Int(n) => *n,
                        other => {
                            return panic(format!(
                                "arr_mean_i64: element must be i64, got {}",
                                other.type_name()
                            ))
                        }
                    };
                    s = s.saturating_add(n);
                }
                ok!(Value::Float(s as f64 / xs.len() as f64));
            }
            // Mean of an f64 (or i64-coerced) array.
            "arr_mean_f64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_mean_f64: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                if xs.is_empty() {
                    ok!(Value::Float(0.0));
                }
                let mut s = 0.0_f64;
                for v in xs {
                    let f = match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        other => {
                            return panic(format!(
                                "arr_mean_f64: element must be numeric, got {}",
                                other.type_name()
                            ))
                        }
                    };
                    s += f;
                }
                ok!(Value::Float(s / xs.len() as f64));
            }
            // Sample standard deviation: sqrt of (sum of squared deviations
            // / (n - 1)). `n < 2` panics — std on a single sample is
            // undefined; caller should guard.
            "arr_std_f64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_std_f64: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                // BUG_HUNT #21: an array of fewer than 2 elements has no spread,
                // so its standard deviation is 0 — return it rather than PANIC.
                // A single sample (or empty) is a legitimate input (a stats loop
                // can collapse to one point); aborting the program is wrong.
                if xs.len() < 2 {
                    ok!(Value::Float(0.0));
                }
                let mut sum = 0.0_f64;
                let mut fs: Vec<f64> = Vec::with_capacity(xs.len());
                for v in xs {
                    let f = match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        other => {
                            return panic(format!(
                                "arr_std_f64: element must be numeric, got {}",
                                other.type_name()
                            ))
                        }
                    };
                    sum += f;
                    fs.push(f);
                }
                let mean = sum / fs.len() as f64;
                let mut var_acc = 0.0_f64;
                for f in &fs {
                    let d = *f - mean;
                    var_acc += d * d;
                }
                let variance = var_acc / (fs.len() - 1) as f64;
                ok!(Value::Float(variance.sqrt()));
            }
            // Index of the largest / smallest element. Empty array panics
            // (no sensible default). Ties broken by lowest index.
            "arr_argmax_i64" | "arr_argmin_i64" | "arr_argmax_f64" | "arr_argmin_f64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!("{name}: expected array, got {}", other.type_name()))
                    }
                };
                if xs.is_empty() {
                    return panic(format!("{name}: array is empty"));
                }
                let pick_max = name == "arr_argmax_i64" || name == "arr_argmax_f64";
                let as_f = |v: &Value| -> Result<f64, Flow> {
                    match v {
                        Value::Int(n) => Ok(*n as f64),
                        Value::Float(f) => Ok(*f),
                        other => Err(Flow::Panic(format!(
                            "{name}: element must be numeric, got {}",
                            other.type_name()
                        ))),
                    }
                };
                let mut best_idx = 0;
                let mut best_val = as_f(&xs[0])?;
                for (i, v) in xs.iter().enumerate().skip(1) {
                    let f = as_f(v)?;
                    if (pick_max && f > best_val) || (!pick_max && f < best_val) {
                        best_val = f;
                        best_idx = i;
                    }
                }
                ok!(Value::Int(best_idx as i64));
            }
            "arr_max_f64" | "arr_min_f64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!("{name}: expected array, got {}", other.type_name()))
                    }
                };
                if xs.is_empty() {
                    return panic(format!("{name}: array is empty"));
                }
                let pick_max = name == "arr_max_f64";
                let as_f = |v: &Value| -> Result<f64, Flow> {
                    match v {
                        Value::Float(f) => Ok(*f),
                        Value::Int(n) => Ok(*n as f64),
                        other => Err(Flow::Panic(format!(
                            "{name}: element must be numeric, got {}",
                            other.type_name()
                        ))),
                    }
                };
                let mut best = as_f(&xs[0])?;
                for v in &xs[1..] {
                    let f = as_f(v)?;
                    if (pick_max && f > best) || (!pick_max && f < best) {
                        best = f;
                    }
                }
                ok!(Value::Float(best));
            }

            // ── String split / join ───────────────────────────────────────
            // str_split("a,b,c", ",") → ["a", "b", "c"]. Empty separator
            // returns the input as a single-element slice (matches Rust's
            // is-not-allowed semantics by sidestepping the panic).
            "str_split" => {
                want(2)?;
                let s = as_str(&args[0])?;
                let sep = as_str(&args[1])?;
                let parts: Vec<Value> = if sep.is_empty() {
                    vec![Value::Str(s.to_string())]
                } else {
                    s.split(sep).map(|p| Value::Str(p.to_string())).collect()
                };
                ok!(Value::Array(parts));
            }
            // str_join(["a","b","c"], "-") → "a-b-c". Non-string elements
            // panic — caller should arr_map(to_str(x)) first if needed.
            "str_join" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "str_join: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let sep = as_str(&args[1])?.to_string();
                let mut parts = Vec::with_capacity(xs.len());
                for v in xs {
                    match v {
                        Value::Str(s) => parts.push(s.clone()),
                        other => {
                            return panic(format!(
                                "str_join: element must be str, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                ok!(Value::Str(parts.join(&sep)));
            }

            // Pair two arrays element-wise into a `[(a, b)]` slice; truncates
            // to the shorter input. Composes with arr_map / arr_filter for
            // dataset zipping (features + labels, etc.).
            "arr_zip" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_zip: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let ys = match &args[1] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_zip: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let n = xs.len().min(ys.len());
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    out.push(Value::Tuple(vec![xs[i].clone(), ys[i].clone()]));
                }
                ok!(Value::Array(out));
            }
            // Split an array into consecutive chunks of size `n`. The last
            // chunk may be shorter if `len(xs)` isn't a multiple of `n`.
            // `n <= 0` panics — caller should guard with their own min.
            "arr_chunk" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_chunk: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let n = as_int(&args[1])?;
                if n <= 0 {
                    return panic(format!("arr_chunk: chunk size must be positive, got {n}"));
                }
                let n = n as usize;
                let mut out: Vec<Value> = Vec::with_capacity(xs.len().div_ceil(n));
                let mut start = 0;
                while start < xs.len() {
                    let end = (start + n).min(xs.len());
                    out.push(Value::Array(xs[start..end].to_vec()));
                    start = end;
                }
                ok!(Value::Array(out));
            }
            // Dedupe an array, preserving first occurrence and order. Uses
            // structural equality so deeply-nested values dedupe correctly.
            // O(n²) — fine for ASI-scale arrays; a hash-based version waits
            // on a HashMap/HashSet primitive.
            "arr_unique" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_unique: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let mut out: Vec<Value> = Vec::new();
                for v in xs {
                    if !out.iter().any(|seen| values_equal(seen, v)) {
                        out.push(v.clone());
                    }
                }
                ok!(Value::Array(out));
            }
            // First index where the element structurally equals `v`. Returns
            // `Some(i)` if found, `None` otherwise. Pairs with arr_contains
            // for "where's the match" follow-ups.
            "arr_index_of" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_index_of: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let needle = &args[1];
                let mut found: Option<i64> = None;
                for (i, v) in xs.iter().enumerate() {
                    if values_equal(v, needle) {
                        found = Some(i as i64);
                        break;
                    }
                }
                ok!(match found {
                    Some(i) => Value::Some(Box::new(Value::Int(i))),
                    None => Value::None,
                });
            }
            // `arr_any(xs, pred)` — does at least one element satisfy the
            // predicate? Short-circuits on the first true. The bool dual
            // of arr_find: arr_find returns the element, arr_any returns
            // whether one exists.
            "arr_any" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_any: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let pred = args[1].clone();
                let mut hit = false;
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x])?;
                    match r {
                        Value::Bool(true) => {
                            hit = true;
                            break;
                        }
                        Value::Bool(false) => {}
                        other => {
                            return panic(format!(
                                "arr_any: predicate must return bool, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                ok!(Value::Bool(hit));
            }
            // `arr_all(xs, pred)` — do ALL elements satisfy the predicate?
            // Short-circuits on the first false. Empty array → true
            // (vacuous truth, matches mathematical convention).
            "arr_all" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_all: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let pred = args[1].clone();
                let mut all = true;
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x])?;
                    match r {
                        Value::Bool(true) => {}
                        Value::Bool(false) => {
                            all = false;
                            break;
                        }
                        other => {
                            return panic(format!(
                                "arr_all: predicate must return bool, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                ok!(Value::Bool(all));
            }
            // `arr_count_if(xs, pred)` — count elements where the
            // predicate returns true. Equivalent to `len(arr_filter(xs,
            // pred))` but doesn't materialize the filtered array.
            "arr_count_if" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_count_if: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let pred = args[1].clone();
                let mut n: i64 = 0;
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x])?;
                    match r {
                        Value::Bool(true) => {
                            n += 1;
                        }
                        Value::Bool(false) => {}
                        other => {
                            return panic(format!(
                                "arr_count_if: predicate must return bool, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                ok!(Value::Int(n));
            }
            // `arr_zip_with(xs, ys, f)` — pair element-wise then map via
            // a 2-arg closure: `f(x, y) -> z`. Truncates to the shorter
            // input. More efficient than `arr_zip` + `arr_map` (no
            // intermediate tuple slice) and lets the closure see both
            // values without destructuring.
            "arr_zip_with" => {
                want(3)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_zip_with: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let ys = match &args[1] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_zip_with: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let f = args[2].clone();
                let n = xs.len().min(ys.len());
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let z = self.call_closure(f.clone(), vec![xs[i].clone(), ys[i].clone()])?;
                    out.push(z);
                }
                ok!(Value::Array(out));
            }
            // First element matching the predicate closure. Returns
            // `Some(v)` when one is found, `None` otherwise — the
            // closure shape mirrors arr_filter, but the result is a
            // single element so callers can early-exit.
            "arr_find" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_find: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let pred = args[1].clone();
                let mut hit: Option<Value> = None;
                for x in xs {
                    let keep = self.call_closure(pred.clone(), vec![x.clone()])?;
                    match keep {
                        Value::Bool(true) => {
                            hit = Some(x);
                            break;
                        }
                        Value::Bool(false) => {}
                        other => {
                            return panic(format!(
                                "arr_find: predicate must return bool, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                ok!(match hit {
                    Some(v) => Value::Some(Box::new(v)),
                    None => Value::None,
                });
            }
            // Linear scan: does `xs` contain a value equal to `v`?
            // Structural equality via the existing `values_equal` helper —
            // works for primitives, strings, tuples, nested arrays.
            "arr_contains" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_contains: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let needle = &args[1];
                let mut found = false;
                for v in xs {
                    if values_equal(v, needle) {
                        found = true;
                        break;
                    }
                }
                ok!(Value::Bool(found));
            }
            // Filter an array by a predicate closure (`i64 -> bool`). Keeps
            // elements where the closure returns `true`. Closures that
            // don't return bool panic, surfacing a typing mistake.
            "arr_filter" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_filter: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let f = args[1].clone();
                let mut out = Vec::with_capacity(xs.len());
                for x in xs {
                    let keep = self.call_closure(f.clone(), vec![x.clone()])?;
                    match keep {
                        Value::Bool(true) => out.push(x),
                        Value::Bool(false) => {}
                        other => {
                            return panic(format!(
                                "arr_filter: predicate must return bool, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                ok!(Value::Array(out));
            }
            // Max / min of an i64 array. Empty → panic (no sensible default
            // for an unbounded domain; caller should `if len(xs) > 0` first).
            "arr_max_i64" | "arr_min_i64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!("{name}: expected array, got {}", other.type_name()))
                    }
                };
                if xs.is_empty() {
                    return panic(format!("{name}: array is empty"));
                }
                let pick_max = name == "arr_max_i64";
                let mut best: i64 = match &xs[0] {
                    Value::Int(n) => *n,
                    other => {
                        return panic(format!(
                            "{name}: element must be i64, got {}",
                            other.type_name()
                        ))
                    }
                };
                for v in &xs[1..] {
                    let n = match v {
                        Value::Int(n) => *n,
                        other => {
                            return panic(format!(
                                "{name}: element must be i64, got {}",
                                other.type_name()
                            ))
                        }
                    };
                    if (pick_max && n > best) || (!pick_max && n < best) {
                        best = n;
                    }
                }
                ok!(Value::Int(best));
            }
            "abs_f64" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.abs()));
            }
            "min_i32" | "min_i64" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])?.min(as_int(&args[1])?)));
            }
            "max_i32" | "max_i64" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])?.max(as_int(&args[1])?)));
            }
            "sqrt" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.sqrt()));
            }
            "pow" => {
                want(2)?;
                ok!(Value::Float(as_float(&args[0])?.powf(as_float(&args[1])?)));
            }
            "exp" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.exp()));
            }
            // Natural log. `x <= 0` returns NaN (matches Rust's f64::ln);
            // callers should guard if zero is plausible — UCB-style
            // algorithms compute ln(t) where t starts at 1, so this is
            // fine in practice.
            "ln" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.ln()));
            }
            // Base-10 log, for human-readable scales (orders of magnitude).
            "log10" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.log10()));
            }
            "floor" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.floor()));
            }
            "ceil" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.ceil()));
            }
            "min_f64" => {
                want(2)?;
                ok!(Value::Float(as_float(&args[0])?.min(as_float(&args[1])?)));
            }
            "max_f64" => {
                want(2)?;
                ok!(Value::Float(as_float(&args[0])?.max(as_float(&args[1])?)));
            }
            "clamp_i64" => {
                want(3)?;
                let (n, lo, hi) = (as_int(&args[0])?, as_int(&args[1])?, as_int(&args[2])?);
                ok!(Value::Int(n.max(lo).min(hi)));
            }
            "clamp_f64" => {
                want(3)?;
                let (n, lo, hi) = (
                    as_float(&args[0])?,
                    as_float(&args[1])?,
                    as_float(&args[2])?,
                );
                ok!(Value::Float(n.max(lo).min(hi)));
            }
            "sign_i64" => {
                want(1)?;
                ok!(Value::Int(as_int(&args[0])?.signum()));
            }
            "pow_i64" => {
                want(2)?;
                let (base, exp) = (as_int(&args[0])?, as_int(&args[1])?);
                if exp < 0 {
                    return panic("pow_i64: negative exponent");
                }
                ok!(Value::Int(base.wrapping_pow(exp as u32)));
            }
            "sqrt_f64" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.sqrt()));
            }
            "floor_f64" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.floor()));
            }
            "ceil_f64" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.ceil()));
            }
            "round_f64" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.round()));
            }
            "srand" => {
                // Seed the RNG for reproducible runs (BUG_HUNT #11 / I-10).
                // Same seed → identical random_*/goal_run_random sequence.
                // (The AXON_SEED env var does the same without code changes.)
                want(1)?;
                set_rand_seed(as_int(&args[0])?);
                ok!(Value::Unit);
            }
            "random_f64" => {
                want(0)?;
                // 53-bit mantissa → uniform [0.0, 1.0)
                ok!(Value::Float(
                    (next_rand_u64() >> 11) as f64 / 9_007_199_254_740_992.0
                ));
            }
            "random_i64" => {
                want(2)?;
                let (lo, hi) = (as_int(&args[0])?, as_int(&args[1])?);
                // Inverted bounds are a caller error: fail loudly instead of
                // silently returning `lo`, which masquerades as success
                // (BUG_HUNT #27 / I-9 — no silent success on degenerate input).
                if hi < lo {
                    return panic(format!(
                        "random_i64: inverted bounds — lo ({lo}) must be <= hi ({hi}); \
                         the range is [lo, hi). Did you swap the arguments?"
                    ));
                }
                // hi == lo is the empty half-open range [lo, lo); `lo` is the
                // only sensible return and is not an error (a collapsed loop
                // bound can legitimately produce it).
                if hi == lo {
                    ok!(Value::Int(lo));
                }
                let range = (hi as i128 - lo as i128) as u128;
                ok!(Value::Int(lo + (next_rand_u64() as u128 % range) as i64));
            }
            "str_pad_start" => {
                want(3)?;
                let s = as_str(&args[0])?;
                let width = as_int(&args[1])?.max(0) as usize;
                let fill = as_str(&args[2])?.chars().next().unwrap_or(' ');
                ok!(Value::Str(if s.len() >= width {
                    s.to_string()
                } else {
                    format!("{}{}", fill.to_string().repeat(width - s.len()), s)
                }));
            }
            "str_pad_end" => {
                want(3)?;
                let s = as_str(&args[0])?;
                let width = as_int(&args[1])?.max(0) as usize;
                let fill = as_str(&args[2])?.chars().next().unwrap_or(' ');
                ok!(Value::Str(if s.len() >= width {
                    s.to_string()
                } else {
                    format!("{}{}", s, fill.to_string().repeat(width - s.len()))
                }));
            }
            "i64_to_str_radix" => {
                want(2)?;
                let n = as_int(&args[0])?;
                let base = as_int(&args[1])?;
                if !(2..=36).contains(&base) {
                    return panic(format!(
                        "i64_to_str_radix: radix must be 2..=36, got {base}"
                    ));
                }
                ok!(Value::Str(i64_to_radix(n, base as u32)));
            }
            "uncertain_new_f64" => {
                want(2)?;
                ok!(make_uncertain(
                    Value::Float(as_float(&args[0])?),
                    as_float(&args[1])?
                ));
            }

            // ── Bit ops ───────────────────────────────────────────────────────
            "bit_and" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])? & as_int(&args[1])?));
            }
            "bit_or" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])? | as_int(&args[1])?));
            }
            "bit_xor" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])? ^ as_int(&args[1])?));
            }
            "bit_not" => {
                want(1)?;
                ok!(Value::Int(!as_int(&args[0])?));
            }
            "shl" => {
                want(2)?;
                ok!(Value::Int(
                    as_int(&args[0])?.wrapping_shl(as_int(&args[1])? as u32)
                ));
            }
            "shr" => {
                want(2)?;
                ok!(Value::Int(
                    as_int(&args[0])?.wrapping_shr(as_int(&args[1])? as u32)
                ));
            }

            // ── String ops ──────────────────────────────────────────────────────
            "str_len" => {
                want(1)?;
                ok!(Value::Int(as_str(&args[0])?.len() as i64));
            }
            "str_concat" | "axon_concat" => {
                want(2)?;
                ok!(Value::Str(format!(
                    "{}{}",
                    as_str(&args[0])?,
                    as_str(&args[1])?
                )));
            }
            "str_eq" => {
                want(2)?;
                ok!(Value::Bool(as_str(&args[0])? == as_str(&args[1])?));
            }
            "str_contains" => {
                want(2)?;
                ok!(Value::Bool(as_str(&args[0])?.contains(as_str(&args[1])?)));
            }
            "str_starts_with" => {
                want(2)?;
                ok!(Value::Bool(
                    as_str(&args[0])?.starts_with(as_str(&args[1])?)
                ));
            }
            "str_ends_with" => {
                want(2)?;
                ok!(Value::Bool(as_str(&args[0])?.ends_with(as_str(&args[1])?)));
            }
            "str_to_upper" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.to_uppercase()));
            }
            // R15 resume runtime (v0): suspend, yield `req` to the host, resume
            // with the reply. The worker thread blocks on the reply channel
            // (that IS the suspension); a bare `axon run` (no host) errors.
            "host_await" => {
                want(1)?;
                let req = as_str(&args[0])?.to_string();
                match crate::interp::host_await_yield(req) {
                    // EOF (`Ok(None)`) collapses to "" for the simple str form.
                    Ok(reply) => ok!(Value::Str(reply.unwrap_or_default())),
                    Err(()) => Err(Flow::Panic(
                        "host_await: called outside a suspendable run (no host driver)".into(),
                    )),
                }
            }
            // R15: EOF-aware form — `None` at end-of-input lets a read loop stop
            // instead of spinning on an endless empty reply.
            "host_await_opt" => {
                want(1)?;
                let req = as_str(&args[0])?.to_string();
                match crate::interp::host_await_yield(req) {
                    Ok(Some(reply)) => ok!(Value::Some(Box::new(Value::Str(reply)))),
                    Ok(None) => ok!(Value::None),
                    Err(()) => Err(Flow::Panic(
                        "host_await_opt: called outside a suspendable run (no host driver)".into(),
                    )),
                }
            }
            "str_to_lower" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.to_lowercase()));
            }
            "str_trim" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.trim().to_string()));
            }
            "str_trim_start" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.trim_start().to_string()));
            }
            "str_trim_end" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.trim_end().to_string()));
            }
            "str_reverse" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.chars().rev().collect()));
            }
            "str_repeat" => {
                want(2)?;
                let n = as_int(&args[1])?.max(0) as usize;
                ok!(Value::Str(as_str(&args[0])?.repeat(n)));
            }
            "str_replace" => {
                want(3)?;
                ok!(Value::Str(
                    as_str(&args[0])?.replace(as_str(&args[1])?, as_str(&args[2])?)
                ));
            }
            "str_index_of" => {
                want(2)?;
                let hay = as_str(&args[0])?;
                let needle = as_str(&args[1])?;
                ok!(Value::Int(hay.find(needle).map(|i| i as i64).unwrap_or(-1)));
            }
            "str_count" => {
                want(2)?;
                ok!(Value::Int(
                    as_str(&args[0])?.matches(as_str(&args[1])?).count() as i64
                ));
            }
            "str_slice" => {
                want(3)?;
                let s = as_str(&args[0])?;
                let start = as_int(&args[1])?.max(0) as usize;
                let end = (as_int(&args[2])?.max(0) as usize).min(s.len());
                let start = start.min(end);
                ok!(Value::Str(s.get(start..end).unwrap_or("").to_string()));
            }
            "char_at" => {
                want(2)?;
                let s = as_str(&args[0])?;
                let i = as_int(&args[1])?.max(0) as usize;
                ok!(Value::Int(
                    s.as_bytes().get(i).map(|b| *b as i64).unwrap_or(-1)
                ));
            }

            "chr" => {
                want(1)?;
                let n = as_int(&args[0])?;
                match u32::try_from(n).ok().and_then(char::from_u32) {
                    Some(c) => ok!(Value::Str(c.to_string())),
                    None => panic(format!("chr: {n} is not a valid Unicode code point")),
                }
            }

            // ── JSON builtins ────────────────────────────────────────────────────
            "json_parse" => {
                want(1)?;
                let s = as_str(&args[0])?.to_string();
                match serde_json::from_str::<serde_json::Value>(&s) {
                    Ok(_) => ok!(Value::Ok(Box::new(Value::Str(s)))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e.to_string())))),
                }
            }
            "json_stringify" => {
                want(1)?;
                let s = as_str(&args[0])?;
                let mut out = String::with_capacity(s.len() + 2);
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        c if (c as u32) < 0x20 => {
                            out.push_str(&format!("\\u{:04x}", c as u32));
                        }
                        c => out.push(c),
                    }
                }
                out.push('"');
                ok!(Value::Str(out))
            }
            "json_get_str" => {
                want(2)?;
                let json = as_str(&args[0])?;
                let key = as_str(&args[1])?;
                match serde_json::from_str::<serde_json::Value>(json) {
                    Ok(serde_json::Value::Object(map)) => match map.get(key) {
                        Some(serde_json::Value::String(v)) => {
                            ok!(Value::Ok(Box::new(Value::Str(v.clone()))))
                        }
                        Some(other) => ok!(Value::Err(Box::new(Value::Str(format!(
                            "key {key:?} is not a string (found {})",
                            if other.is_null() {
                                "null"
                            } else {
                                "other type"
                            }
                        ))))),
                        None => ok!(Value::Err(Box::new(Value::Str(format!(
                            "key {key:?} not found"
                        ))))),
                    },
                    Ok(_) => ok!(Value::Err(Box::new(Value::Str(
                        "json_get_str: input is not a JSON object".into()
                    )))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e.to_string())))),
                }
            }
            "json_get_i64" => {
                want(2)?;
                let json = as_str(&args[0])?;
                let key = as_str(&args[1])?;
                match serde_json::from_str::<serde_json::Value>(json) {
                    Ok(serde_json::Value::Object(map)) => match map.get(key) {
                        Some(serde_json::Value::Number(n)) => match n.as_i64() {
                            Some(v) => ok!(Value::Ok(Box::new(Value::Int(v)))),
                            None => ok!(Value::Err(Box::new(Value::Str(format!(
                                "key {key:?} is a number but not an integer"
                            ))))),
                        },
                        Some(_) => ok!(Value::Err(Box::new(Value::Str(format!(
                            "key {key:?} is not a number"
                        ))))),
                        None => ok!(Value::Err(Box::new(Value::Str(format!(
                            "key {key:?} not found"
                        ))))),
                    },
                    Ok(_) => ok!(Value::Err(Box::new(Value::Str(
                        "json_get_i64: input is not a JSON object".into()
                    )))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e.to_string())))),
                }
            }
            "json_path_str" => {
                want(2)?;
                let json_str = as_str(&args[0])?.to_string();
                let path = as_str(&args[1])?.to_string();
                let root: serde_json::Value = match serde_json::from_str(&json_str) {
                    Ok(v) => v,
                    Err(e) => {
                        ok!(Value::Err(Box::new(Value::Str(e.to_string()))))
                    }
                };
                let mut cur = &root;
                for key in path.split('.') {
                    match cur {
                        serde_json::Value::Object(map) => match map.get(key) {
                            Some(next) => cur = next,
                            None => {
                                ok!(Value::Err(Box::new(Value::Str(format!(
                                    "json_path_str: key {key:?} not found"
                                )))))
                            }
                        },
                        serde_json::Value::Array(arr) => match key.parse::<usize>() {
                            Ok(idx) => match arr.get(idx) {
                                Some(next) => cur = next,
                                None => {
                                    ok!(Value::Err(Box::new(Value::Str(format!(
                                        "json_path_str: array index {idx} out of bounds (len {})",
                                        arr.len()
                                    )))))
                                }
                            },
                            Err(_) => {
                                ok!(Value::Err(Box::new(Value::Str(format!(
                                    "json_path_str: array requires numeric index, got {key:?}"
                                )))))
                            }
                        },
                        _ => {
                            ok!(Value::Err(Box::new(Value::Str(format!(
                                "json_path_str: cannot index into scalar at key {key:?}"
                            )))))
                        }
                    }
                }
                match cur {
                    serde_json::Value::String(s) => {
                        ok!(Value::Ok(Box::new(Value::Str(s.clone()))))
                    }
                    other => ok!(Value::Err(Box::new(Value::Str(format!(
                        "json_path_str: leaf is not a string (found {})",
                        if other.is_null() {
                            "null"
                        } else {
                            "other type"
                        }
                    ))))),
                }
            }

            // ── Assertions ──────────────────────────────────────────────────────
            "assert" => {
                want(1)?;
                if !as_bool(&args[0])? {
                    return Err(Flow::Panic("assertion failed".into()));
                }
                ok!(Value::Unit);
            }
            "assert_eq" => {
                want(2)?;
                let (a, b) = (as_int(&args[0])?, as_int(&args[1])?);
                if a != b {
                    return Err(Flow::Panic(format!("assertion failed: {a} != {b}")));
                }
                ok!(Value::Unit);
            }
            "assert_eq_str" => {
                want(2)?;
                let (a, b) = (as_str(&args[0])?, as_str(&args[1])?);
                if a != b {
                    return Err(Flow::Panic(format!("assertion failed: {a:?} != {b:?}")));
                }
                ok!(Value::Unit);
            }
            "assert_eq_f64" => {
                want(2)?;
                let (a, b) = (as_float(&args[0])?, as_float(&args[1])?);
                if (a - b).abs() > 1e-9 {
                    return Err(Flow::Panic(format!("assertion failed: {a} != {b}")));
                }
                ok!(Value::Unit);
            }
            "assert_err" => {
                want(1)?;
                if as_bool(&args[0])? {
                    return Err(Flow::Panic("assert_err: expected Err, got Ok".into()));
                }
                ok!(Value::Unit);
            }

            // ── Environment / time / process ──────────────────────────────────
            "env_var" => {
                want(1)?;
                let key = as_str(&args[0])?.to_string();
                ok!(match crate::host::with_host(|h| h.env_var(&key)) {
                    Some(v) => Value::Ok(Box::new(Value::Str(v))),
                    None => Value::Err(Box::new(Value::Str("not set".into()))),
                });
            }
            "now_ms" => {
                want(0)?;
                ok!(Value::Int(crate::host::with_host(|h| h.now_ms())));
            }
            "sleep_ms" => {
                want(1)?;
                let ms = as_int(&args[0])?.max(0) as u64;
                crate::host::with_host(|h| h.sleep_ms(ms));
                ok!(Value::Unit);
            }
            "exit" => {
                want(1)?;
                Err(Flow::Exit(as_int(&args[0])? as i32))
            }

            // ── ASI: numeric conversions ────────────────────────────────────
            "f64_to_i64" => {
                want(1)?;
                ok!(Value::Int(as_float(&args[0])? as i64));
            }
            "i64_to_f64" => {
                want(1)?;
                ok!(Value::Float(as_int(&args[0])? as f64));
            }

            // ── ASI: Uncertain<T> construction ──────────────────────────────
            // Represented as a struct `Uncertain { value, confidence }`, so
            // `.value` / `.confidence` field access works directly.
            "uncertain_new" | "uncertain_dyn_i64" => {
                want(2)?;
                ok!(make_uncertain(
                    Value::Int(as_int(&args[0])?),
                    as_float(&args[1])?
                ));
            }
            "uncertain_dyn_f64" => {
                want(2)?;
                ok!(make_uncertain(
                    Value::Float(as_float(&args[0])?),
                    as_float(&args[1])?
                ));
            }
            "uncertain_deterministic" => {
                want(1)?;
                ok!(make_uncertain(Value::Int(as_int(&args[0])?), 1.0));
            }
            "uncertain_confidence" => {
                // Compile-time confidence hint; no runtime effect.
                ok!(Value::Unit);
            }

            // ── ASI: Temporal<T> ────────────────────────────────────────────
            // Represented as `Temporal { value, horizon_ms, decay, created_ms }`.
            "temporal_now" => {
                want(0)?;
                ok!(Value::Int(now_ms()));
            }
            "temporal_new" => {
                want(3)?;
                ok!(make_temporal(
                    Value::Int(as_int(&args[0])?),
                    1.0, // confidence starts full at creation; decays via temporal_at
                    as_int(&args[1])?,
                    as_float(&args[2])?,
                    now_ms(),
                ));
            }
            "temporal_at" => {
                want(2)?;
                match &args[0] {
                    Value::Struct { fields, .. } => {
                        let value = fields.get("value").cloned().unwrap_or(Value::Int(0));
                        let confidence = fields
                            .get("confidence")
                            .and_then(as_float_opt)
                            .unwrap_or(1.0);
                        let horizon = fields.get("horizon_ms").and_then(as_int_opt).unwrap_or(0);
                        let decay = fields.get("decay").and_then(as_float_opt).unwrap_or(0.0);
                        let created = fields.get("created_ms").and_then(as_int_opt).unwrap_or(0);
                        let offset = as_int(&args[1])?;
                        // PRD §"Temporal": project forward by `offset` ms, DECAYING
                        // confidence as `c * (1 - decay)^(offset_ms / 86_400_000)`
                        // (decay is per-day; a negative/zero offset leaves it
                        // unchanged). This is the time-awareness the type exists
                        // to make explicit — knowledge degrades as time passes.
                        let days = offset as f64 / 86_400_000.0;
                        let new_conf = if days > 0.0 {
                            confidence * (1.0 - decay).max(0.0).powf(days)
                        } else {
                            confidence
                        };
                        ok!(make_temporal(
                            value,
                            new_conf,
                            horizon,
                            decay,
                            created + offset
                        ));
                    }
                    _ => panic("temporal_at: expected a Temporal value"),
                }
            }
            // Read the present confidence of a Temporal value (PRD `rev.confidence`).
            "temporal_confidence" => {
                want(1)?;
                match &args[0] {
                    Value::Struct { fields, .. } => {
                        ok!(Value::Float(
                            fields
                                .get("confidence")
                                .and_then(as_float_opt)
                                .unwrap_or(1.0)
                        ));
                    }
                    _ => panic("temporal_confidence: expected a Temporal value"),
                }
            }
            "temporal_is_valid" => {
                want(1)?;
                match &args[0] {
                    Value::Struct { fields, .. } => {
                        let horizon = fields.get("horizon_ms").and_then(as_int_opt).unwrap_or(0);
                        let created = fields.get("created_ms").and_then(as_int_opt).unwrap_or(0);
                        ok!(Value::Bool(now_ms() <= created + horizon));
                    }
                    _ => panic("temporal_is_valid: expected a Temporal value"),
                }
            }

            // ── ASI: goal-directed optimization ─────────────────────────────
            // Live hill-climb over an `@[adaptive] fn(i64) -> i64`, else a
            // retrospective best-observed lookup (mirrors axon-rt's goal.rs).
            "goal_run" => {
                want(3)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                let max_evals = as_int(&args[2])?;
                ok!(Value::Float(self.run_goal(&name, target, max_evals)?));
            }

            // Constrained search (pillar 3, `subject_to`): hill-climb the
            // @[adaptive] `metric` toward `target`, but only over candidates the
            // boolean `constraint` fn ACCEPTS. The constraint shares the metric's
            // parameter list; an infeasible candidate is scored maximally-distant
            // so the optimizer rejects it (a hard gate, not a soft penalty baked
            // into the metric). The constraint runs as a plain call, so it never
            // pollutes the score trajectory. If every candidate is infeasible the
            // result is the INFEASIBLE_SCORE sentinel.
            "goal_run_constrained" => {
                want(4)?;
                let name = as_str(&args[0])?.to_string();
                let constraint = as_str(&args[1])?.to_string();
                let target = as_float(&args[2])?;
                let max_evals = as_int(&args[3])?;
                // Fail loudly if the constraint names no defined fn — silently
                // ignoring it would defeat the whole point (the caller believes
                // the search is constrained when it is not).
                if !self.fns.contains_key(constraint.as_str()) {
                    return panic(format!(
                        "goal_run_constrained: constraint fn `{constraint}` is not defined"
                    ));
                }
                *self.goal_constraint.borrow_mut() = Some(constraint);
                let result = self.run_goal(&name, target, max_evals);
                // Clear BEFORE propagating so a metric error can't leave a stale
                // constraint armed for the next goal_run.
                *self.goal_constraint.borrow_mut() = None;
                ok!(Value::Float(result?));
            }

            // Categorical search: the @[adaptive] metric's single i64 arg is a
            // CHOICE INDEX in [0, n_choices). No ordinal assumption (unlike the
            // hill-climbers) — exhaustive when the budget covers the set, else a
            // random sample. For unordered options (prompt templates, models).
            "goal_run_categorical" => {
                want(4)?;
                let name = as_str(&args[0])?.to_string();
                let n_choices = as_int(&args[1])?;
                let target = as_float(&args[2])?;
                let max_evals = as_int(&args[3])?;
                ok!(Value::Float(self.run_goal_categorical(
                    &name, n_choices, target, max_evals
                )?));
            }

            // Random-search strategy. Baseline against the hill-climb
            // path; useful for multi-modal objectives where the
            // gradient gets stuck in a local optimum.
            "goal_run_random" => {
                want(5)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                let n_samples = as_int(&args[2])?;
                let lo = as_int(&args[3])?;
                let hi = as_int(&args[4])?;
                ok!(Value::Float(
                    self.run_goal_random(&name, target, n_samples, lo, hi)?
                ));
            }

            // Multi-start hill climb. Random restarts + local refinement —
            // the standard recipe for escaping local optima while keeping
            // gradient-style convergence once a basin is found.
            "goal_run_multistart" => {
                want(6)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                let n_starts = as_int(&args[2])?;
                let evals_per_start = as_int(&args[3])?;
                let lo = as_int(&args[4])?;
                let hi = as_int(&args[5])?;
                ok!(Value::Float(self.run_goal_multistart(
                    &name,
                    target,
                    n_starts,
                    evals_per_start,
                    lo,
                    hi
                )?));
            }

            // Warm-start variant: seeds the optimizer at the best prior
            // probe from in-memory provenance, rather than starting at the
            // origin. Pair with a previous goal_run / goal_clear pattern
            // to do iterative refinement: each call resumes where the last
            // left off. Falls through to a fresh run when the fn has no
            // prior provenance entry, so it's safe to call cold.
            "goal_continue" => {
                want(3)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                let max_evals = as_int(&args[2])?;
                ok!(Value::Float(self.run_goal_warm(&name, target, max_evals)?));
            }

            // Read back the best-scoring leading-i64 input observed for an
            // `@[adaptive]` fn. Pairs with `goal_run` — the optimizer logs
            // (input, score) on every call, so a follow-up call can
            // introspect "what probe got us closest to target?". For
            // multi-arg fns this returns the FIRST dim; use
            // `goal_best_inputs` to read the full tuple. Returns 0 when
            // the fn was never called or has no i64 leading param.
            "goal_best_input" => {
                want(2)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                ok!(Value::Int(self.best_input(&name, target)));
            }

            // All i64 input dims that produced the best score, as a slice.
            // The multi-arg companion to `goal_best_input`: for an
            // `@[adaptive] fn(x: i64, y: i64) -> i64` it returns `[x*, y*]`.
            // Empty slice when the fn was never called.
            "goal_best_inputs" => {
                want(2)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                ok!(self.best_inputs(&name, target));
            }

            // f64-flavored counterpart: returns the f64-prefix input tuple
            // of the best-scoring entry. Pairs with the f64 hill climb for
            // continuous-domain `@[adaptive] fn(f64, …) -> f64` searches.
            "goal_best_inputs_f64" => {
                want(2)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                ok!(self.best_inputs_f64(&name, target));
            }

            // Read the best observed score WITHOUT running another
            // optimization. Equivalent to `goal_run(name, target, 0)`
            // but doesn't mutate provenance — purely a query against
            // what's already been recorded. The right primitive for
            // "how am I doing?" checkpoints between rounds.
            "goal_best_score" => {
                want(2)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                ok!(Value::Float(self.best_observed(&name, target, 0)));
            }

            // Number of provenance entries recorded for an @[adaptive] fn.
            // Equivalent to `len(goal_history(name))` but O(1) — avoids
            // materializing the trace just to count it. Useful for budget
            // gates: `if goal_count("score") > 1000 { stop }`.
            "goal_count" => {
                want(1)?;
                let name = as_str(&args[0])?.to_string();
                let n = self
                    .provenance
                    .borrow()
                    .get(&name)
                    .map(|v| v.len())
                    .unwrap_or(0);
                ok!(Value::Int(n as i64));
            }

            // `ai_cost_spent() -> i64` — Phase-7 cost_meter / F4: the cumulative
            // AI spend so far this run, in integer micro-dollars (µ$). Every
            // dispatched `ai_complete` (mock or live) adds its per-token cost
            // (`tier.cost_micro(est_tokens)`); a fallback (no model reached)
            // adds nothing. This is the kernel cost meter that the userland
            // `llm_gateway.ax` modeled — a program can read its real spend and
            // gate on a cost budget, distinct from R3c's per-call-count budget.
            "ai_cost_spent" => {
                want(0)?;
                ok!(Value::Int(self.ai_cost_micro.get()));
            }

            // ── Phase 7 (R12 Slice 1): principal_authority kernel registry ──────
            // The KERNEL enforces R11 attenuation: a child can never hold a cap
            // the parent lacks, and budget is carved from the parent, not
            // conjured. Handles are plain i64 indices into the live registry.
            // Observable semantics are byte-identical to the userland oracle
            // (`examples/stdlib/principal_mint.ax`) — I-2.

            // `principal_root(name, net, fs_write, exec, budget) -> i64` —
            // register a ROOT authority; returns its handle.
            "principal_root" => {
                want(5)?;
                let name = as_str(&args[0])?.to_string();
                let net = as_bool(&args[1])?;
                let fs_write = as_bool(&args[2])?;
                let exec = as_bool(&args[3])?;
                let budget = as_int(&args[4])?;
                let h = self
                    .principals
                    .borrow_mut()
                    .root(name, net, fs_write, exec, budget);
                ok!(Value::Int(h as i64));
            }

            // `principal_mint(parent, name, net, fs_write, exec, grant) -> i64` —
            // mint an attenuated child; returns its handle, or panics E1601 if the
            // parent handle is unknown (a defense-in-depth guard — the registry
            // makes escalation structurally impossible, so a valid parent always
            // yields a clamped child).
            "principal_mint" => {
                want(6)?;
                let parent = as_int(&args[0])?;
                let name = as_str(&args[1])?.to_string();
                let net = as_bool(&args[2])?;
                let fs_write = as_bool(&args[3])?;
                let exec = as_bool(&args[4])?;
                let grant = as_int(&args[5])?;
                if parent < 0 {
                    return panic(format!(
                        "[E1601] principal_mint: invalid parent handle {parent} \
                         (a principal handle comes from principal_root/principal_mint)"
                    ));
                }
                match self.principals.borrow_mut().mint(
                    parent as usize,
                    name,
                    net,
                    fs_write,
                    exec,
                    grant,
                ) {
                    Some(h) => ok!(Value::Int(h as i64)),
                    None => panic(format!(
                        "[E1601] principal_mint: unknown parent handle {parent} \
                         (no such principal in the kernel registry)"
                    )),
                }
            }

            // `principal_holds(handle, cap) -> bool` — does the principal hold the
            // named capability ("net"/"fs_write"/"exec")? Unknown name/handle → false.
            "principal_holds" => {
                want(2)?;
                let h = as_int(&args[0])?;
                let cap = as_str(&args[1])?.to_string();
                let held = h >= 0
                    && self
                        .principals
                        .borrow()
                        .get(h as usize)
                        .map(|p| p.holds(&cap))
                        .unwrap_or(false);
                ok!(Value::Bool(held));
            }

            // `principal_budget_remaining(handle) -> i64` — the principal's
            // remaining budget (0 if unknown / exhausted).
            "principal_budget_remaining" => {
                want(1)?;
                let h = as_int(&args[0])?;
                let rem = if h >= 0 {
                    self.principals.borrow().budget_remaining(h as usize)
                } else {
                    0
                };
                ok!(Value::Int(rem));
            }

            // `principal_spend(handle, amount) -> i64` — debit the principal's own
            // budget; returns the new remaining. Caps untouched.
            "principal_spend" => {
                want(2)?;
                let h = as_int(&args[0])?;
                let amount = as_int(&args[1])?;
                let rem = if h >= 0 {
                    self.principals.borrow_mut().spend(h as usize, amount)
                } else {
                    0
                };
                ok!(Value::Int(rem));
            }

            // `principal_authorize(handle, needs_net, needs_fs_write, needs_exec)
            // -> bool` — action gate: holds every needed cap AND not exhausted.
            "principal_authorize" => {
                want(4)?;
                let h = as_int(&args[0])?;
                let n = as_bool(&args[1])?;
                let f = as_bool(&args[2])?;
                let e = as_bool(&args[3])?;
                let ok = h >= 0 && self.principals.borrow().authorize(h as usize, n, f, e);
                ok!(Value::Bool(ok));
            }

            // `principal_can_mint(handle, want_net, want_fs_write, want_exec,
            // grant) -> bool` — would a mint grant anything (holds the caps + has
            // budget)? The explicit gate; mint is total + safe without it.
            "principal_can_mint" => {
                want(5)?;
                let h = as_int(&args[0])?;
                let n = as_bool(&args[1])?;
                let f = as_bool(&args[2])?;
                let e = as_bool(&args[3])?;
                let g = as_int(&args[4])?;
                let ok = h >= 0 && self.principals.borrow().can_mint(h as usize, n, f, e, g);
                ok!(Value::Bool(ok));
            }

            // F5 (Phase 9): `sandbox_create(principal, allowed_effects) -> i64`
            // Register a runtime sandbox that allows only the comma-separated
            // effects in `allowed_effects` (e.g. "AI,Net", "IO", or "" = pure).
            // The sandbox is bound to `principal` for audit attribution. Returns
            // the sandbox handle (an index into `self.sandboxes`). Interp-only.
            "sandbox_create" => {
                want(2)?;
                let principal = as_int(&args[0])?;
                let raw = as_str(&args[1])?.to_string();
                let allowed: std::collections::HashSet<String> = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                let mut sbs = self.sandboxes.borrow_mut();
                let handle = sbs.len() as i64;
                sbs.push(SandboxEntry { principal, allowed });
                ok!(Value::Int(handle));
            }

            // F5 (Phase 9): `sandbox_run(sandbox, fn_name, arg) -> i64`
            // Call the named user function with `arg` inside the sandbox. Any
            // builtin that attempts an effect outside the sandbox's ceiling raises
            // SandboxViolation (exit 8) and the call is refused. The active sandbox
            // is restored to its previous value after the call (nesting-safe). The
            // function's return value (coerced to i64; () → 0) is returned. Interp-only.
            "sandbox_run" => {
                want(3)?;
                let sb_handle = as_int(&args[0])?;
                let fn_name = as_str(&args[1])?.to_string();
                let arg = as_int(&args[2])?;
                // Validate sandbox handle.
                {
                    let sbs = self.sandboxes.borrow();
                    if sb_handle < 0 || sb_handle as usize >= sbs.len() {
                        return panic(format!("sandbox_run: unknown sandbox handle {sb_handle}"));
                    }
                }
                // Validate the function exists.
                let Some(f) = self.fns.get(&fn_name).copied() else {
                    return panic(format!("sandbox_run: no function `{fn_name}`"));
                };
                // Set the active sandbox, save the previous value for restore.
                let prev_sandbox = self.active_sandbox.replace(sb_handle);
                let result = self.call_fn(f, vec![Value::Int(arg)]);
                self.active_sandbox.set(prev_sandbox);
                match result {
                    Ok(Value::Int(n)) => ok!(Value::Int(n)),
                    Ok(Value::Tuple(ref v)) if v.is_empty() => ok!(Value::Int(0)),
                    Ok(v) => ok!(v),
                    Err(e) => Err(e),
                }
            }

            // F3 (Phase 9): `principal_activate(handle) -> ()` — set the named
            // principal as the current audit context so capability audit records
            // (ai_call, agent_action) carry its name rather than the opaque "root"
            // default. A negative or unknown handle resets to "root". Interp-only.
            "principal_activate" => {
                want(1)?;
                let h = as_int(&args[0])?;
                let name = if h >= 0 {
                    self.principals
                        .borrow()
                        .get(h as usize)
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| "root".to_string())
                } else {
                    "root".to_string()
                };
                *self.current_principal.borrow_mut() = name;
                ok!(Value::Tuple(vec![]));
            }

            // F3 (Phase 9): `principal_current_name() -> str` — return the name of
            // the principal currently active in the audit context. "root" when none
            // has been set via principal_activate. Useful for audit queries.
            "principal_current_name" => {
                want(0)?;
                ok!(Value::Str(self.current_principal_name()));
            }

            // ── Phase 7 (R12 Slice 2): cooperative scheduler ────────────────────
            // A fiber run-queue over the eager-spawn model. Fibers are (named fn,
            // i64 arg); `scheduler_run` runs the ready ones in a seed-deterministic
            // round-robin, catching a panicking fiber (recorded failed, not a
            // process abort). Interp-only (codegen E0910-refuses), like goal_run.

            // `scheduler_spawn(fn_name, arg) -> i64` — queue a fiber; returns its id.
            "scheduler_spawn" => {
                want(2)?;
                let fn_name = as_str(&args[0])?.to_string();
                let arg = as_int(&args[1])?;
                if !self.fns.contains_key(&fn_name) {
                    return panic(format!(
                        "[E1602] scheduler_spawn: no function `{fn_name}` to run as a fiber"
                    ));
                }
                let id = self.scheduler.borrow_mut().spawn(fn_name, arg);
                ok!(Value::Int(id as i64));
            }

            // `scheduler_run() -> i64` — run all READY fibers to completion in the
            // seed-deterministic order; returns the count that completed
            // successfully. A fiber that panics/halts is caught and marked failed
            // (observable via `scheduler_failed`), NOT a process abort — this is
            // what lets the Slice-3 supervisor restart it. Re-runnable: a fiber
            // restarted by the supervisor becomes Ready and runs on the next call.
            "scheduler_run" => {
                want(0)?;
                let completed = self.builtin_scheduler_run_once()?;
                ok!(Value::Int(completed));
            }

            // `scheduler_result(id) -> i64` — a completed fiber's result (0 if not
            // done / unknown id).
            "scheduler_result" => {
                want(1)?;
                let id = as_int(&args[0])?;
                let r = if id >= 0 {
                    self.scheduler.borrow().result(id as usize)
                } else {
                    0
                };
                ok!(Value::Int(r));
            }

            // `scheduler_failed(id) -> bool` — did the fiber fail (panic/halt)?
            "scheduler_failed" => {
                want(1)?;
                let id = as_int(&args[0])?;
                let f = id >= 0 && self.scheduler.borrow().failed(id as usize);
                ok!(Value::Bool(f));
            }

            // `scheduler_restart(id) -> i64` — re-queue a fiber as Ready (the
            // Slice-3 supervisor hook); returns the id. No-op on unknown id.
            "scheduler_restart" => {
                want(1)?;
                let id = as_int(&args[0])?;
                if id >= 0 {
                    self.scheduler.borrow_mut().restart(id as usize);
                }
                ok!(Value::Int(id));
            }

            // `scheduler_done_count() -> i64` / `scheduler_failed_count() -> i64`
            // — the run tally (completed, failed) for a fan-out/collect summary.
            "scheduler_done_count" => {
                want(0)?;
                ok!(Value::Int(self.scheduler.borrow().tally().0 as i64));
            }
            "scheduler_failed_count" => {
                want(0)?;
                ok!(Value::Int(self.scheduler.borrow().tally().1 as i64));
            }

            // ── Phase 7 (R12 Slice 3): live supervisor_root ─────────────────────
            // The pure OTP restart logic of supervisor_tree.ax, made LIVE over the
            // Slice-2 scheduler: a supervised fiber that fails is ACTUALLY
            // restarted per its strategy, with the max-restart-intensity latch
            // tripping a real halt (Flow::Halted, exit 4) on a crash loop.

            // `supervisor_new(strategy, max_restarts) -> i64` — create a live
            // supervisor (strategy 0=one_for_one, 1=one_for_all, 2=rest_for_one);
            // returns its handle.
            "supervisor_new" => {
                want(2)?;
                let strategy = as_int(&args[0])?;
                let max_restarts = as_int(&args[1])?;
                let mut sups = self.supervisors.borrow_mut();
                sups.push(crate::kernel::Supervisor::new(strategy, max_restarts));
                ok!(Value::Int((sups.len() - 1) as i64));
            }

            // `supervisor_supervise(sup, fiber_id) -> i64` — register a scheduler
            // fiber as a supervised child (in start order); returns its child index.
            "supervisor_supervise" => {
                want(2)?;
                let sup = as_int(&args[0])?;
                let fiber = as_int(&args[1])?;
                if sup < 0 || fiber < 0 {
                    return panic("[E1602] supervisor_supervise: negative handle".to_string());
                }
                let mut sups = self.supervisors.borrow_mut();
                let Some(s) = sups.get_mut(sup as usize) else {
                    return panic(format!(
                        "[E1602] supervisor_supervise: unknown supervisor {sup}"
                    ));
                };
                let idx = s.supervise(fiber as usize);
                ok!(Value::Int(idx as i64));
            }

            // `supervisor_run(sup) -> i64` — run the supervised fibers via the
            // scheduler; on each failure apply the OTP restart set (re-queue those
            // fibers) and re-run, until all children succeed OR the
            // max-restart-intensity latch trips → HALT this subtree (Flow::Halted,
            // exit 4). Returns the number of restart rounds performed. A bounded
            // loop (the latch guarantees termination) so a crash loop can't spin
            // forever.
            "supervisor_run" => {
                want(1)?;
                let sup = as_int(&args[0])?;
                if sup < 0 {
                    return panic("[E1602] supervisor_run: negative supervisor handle".to_string());
                }
                // Hard bound on rounds: max_restarts + 2 (the latch trips at
                // max_restarts+1; +1 slack). Defends against any logic slip.
                let max_rounds = {
                    let sups = self.supervisors.borrow();
                    match sups.get(sup as usize) {
                        Some(s) => s.max_restarts.max(0) + 2,
                        None => {
                            return panic(format!(
                                "[E1602] supervisor_run: unknown supervisor {sup}"
                            ))
                        }
                    }
                };
                let mut rounds: i64 = 0;
                loop {
                    // Run all currently-ready fibers (Slice-2 catches per-fiber
                    // panics). Propagate a whole-program exit/Halt.
                    self.builtin_scheduler_run_once()?;
                    // Find the first supervised child that FAILED, in child order.
                    let failed_child: Option<i64> = {
                        let sups = self.supervisors.borrow();
                        let sched = self.scheduler.borrow();
                        sups.get(sup as usize).and_then(|s| {
                            s.children.iter().enumerate().find_map(|(ci, &fid)| {
                                if sched.failed(fid) {
                                    Some(ci as i64)
                                } else {
                                    None
                                }
                            })
                        })
                    };
                    let Some(child_idx) = failed_child else {
                        // No supervised child is in a failed state → done.
                        break;
                    };
                    // Apply the OTP restart set; latch-halt on crash loop.
                    let to_restart = {
                        let mut sups = self.supervisors.borrow_mut();
                        sups[sup as usize].on_failure(child_idx)
                    };
                    let halted = {
                        let sups = self.supervisors.borrow();
                        sups[sup as usize].halted
                    };
                    if halted {
                        let restarts = self.supervisors.borrow()[sup as usize].restarts;
                        return Err(Flow::Halted(format!(
                            "[E1602] supervisor halted its subtree after {restarts} restarts \
                             (max-restart intensity exceeded — crash loop abandoned)"
                        )));
                    }
                    // Re-queue the strategy's restart set for the next round.
                    {
                        let mut sched = self.scheduler.borrow_mut();
                        for fid in to_restart {
                            sched.restart(fid);
                        }
                    }
                    rounds += 1;
                    if rounds > max_rounds {
                        // Defense-in-depth: the latch should have tripped already.
                        return Err(Flow::Halted(format!(
                            "[E1602] supervisor exceeded {max_rounds} restart rounds without \
                             latching — abandoned (defensive)"
                        )));
                    }
                }
                ok!(Value::Int(rounds));
            }

            // `supervisor_alive(sup) -> bool` — is the supervisor still running
            // (not halted by a crash loop)?
            "supervisor_alive" => {
                want(1)?;
                let sup = as_int(&args[0])?;
                let alive = sup >= 0
                    && self
                        .supervisors
                        .borrow()
                        .get(sup as usize)
                        .map(|s| s.alive())
                        .unwrap_or(false);
                ok!(Value::Bool(alive));
            }

            // `supervisor_restarts(sup) -> i64` — cumulative restart events observed.
            "supervisor_restarts" => {
                want(1)?;
                let sup = as_int(&args[0])?;
                let n = if sup >= 0 {
                    self.supervisors
                        .borrow()
                        .get(sup as usize)
                        .map(|s| s.restarts)
                        .unwrap_or(0)
                } else {
                    0
                };
                ok!(Value::Int(n));
            }

            // ── Phase 7 (R12 Slice 4): durable Store<T,C> ───────────────────────
            // A persistent store keyed by name: its applied ops are appended to an
            // NDJSON log, replayed on open, so the value survives a fresh process
            // and a retried op_id dedups cross-process under linearizable.

            // `dstore_open(key, consistency) -> i64` — open (and replay) the durable
            // store `key` (0=at_least_once, 1=linearizable); returns its handle.
            "dstore_open" => {
                want(2)?;
                let key = as_str(&args[0])?.to_string();
                let consistency = as_int(&args[1])?;
                let path = store_log_path(&key);
                let mut store = crate::kernel::Store::new(consistency);
                // Replay the log to rebuild state (cross-process durability). Each
                // line is `op_id delta`; apply through the same consistency logic,
                // so a linearizable store reconstructs its `seen` set and dedups.
                if let Some(p) = &path {
                    if let Ok(contents) = std::fs::read_to_string(p) {
                        for line in contents.lines() {
                            let mut it = line.split_whitespace();
                            if let (Some(o), Some(d)) = (it.next(), it.next()) {
                                if let (Ok(op_id), Ok(delta)) = (o.parse::<i64>(), d.parse::<i64>())
                                {
                                    store.apply(op_id, delta);
                                }
                            }
                        }
                    }
                }
                let path = match path {
                    Some(p) => p,
                    None => {
                        return panic(
                            "[E1603] dstore_open: no cache dir for the durable store log"
                                .to_string(),
                        )
                    }
                };
                let mut stores = self.stores.borrow_mut();
                stores.push((store, path));
                ok!(Value::Int((stores.len() - 1) as i64));
            }

            // `dstore_apply(handle, op_id, delta) -> i64` — apply an op; returns the
            // new value. Under linearizable a retried op_id is deduped (no-op) and
            // NOT re-logged; under at_least_once it re-applies. An actually-applied
            // op is appended to the durable log so it survives a process restart.
            "dstore_apply" => {
                want(3)?;
                let h = as_int(&args[0])?;
                let op_id = as_int(&args[1])?;
                let delta = as_int(&args[2])?;
                if h < 0 {
                    return panic("[E1603] dstore_apply: negative store handle".to_string());
                }
                let (applied, new_value, path) = {
                    let mut stores = self.stores.borrow_mut();
                    let Some((store, path)) = stores.get_mut(h as usize) else {
                        return panic(format!("[E1603] dstore_apply: unknown store handle {h}"));
                    };
                    let applied = store.apply(op_id, delta);
                    (applied, store.value, path.clone())
                };
                // Persist only an op that actually took effect (a deduped retry is
                // not re-logged — replay would dedup it again, but not re-logging
                // keeps the log = the applied total order).
                if applied {
                    if let Some(dir) = path.parent() {
                        let _ = std::fs::create_dir_all(dir);
                    }
                    use std::io::Write as _;
                    if let Ok(mut f) = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(&path)
                    {
                        let _ = writeln!(f, "{op_id} {delta}");
                    }
                }
                ok!(Value::Int(new_value));
            }

            // `dstore_value(handle) -> i64` — the store's current accumulated value.
            "dstore_value" => {
                want(1)?;
                let h = as_int(&args[0])?;
                let v = if h >= 0 {
                    self.stores
                        .borrow()
                        .get(h as usize)
                        .map(|(s, _)| s.value)
                        .unwrap_or(0)
                } else {
                    0
                };
                ok!(Value::Int(v));
            }

            // `dstore_version(handle) -> i64` — the monotonic total-order stamp
            // (linearizable only; at_least_once stays 0).
            "dstore_version" => {
                want(1)?;
                let h = as_int(&args[0])?;
                let v = if h >= 0 {
                    self.stores
                        .borrow()
                        .get(h as usize)
                        .map(|(s, _)| s.version)
                        .unwrap_or(0)
                } else {
                    0
                };
                ok!(Value::Int(v));
            }

            // `dstore_clear(key) -> i64` — delete a durable store's log (reset
            // persistence). Returns 1 if a log existed and was removed, else 0.
            // Lets a test start from a clean slate; not part of the consistency
            // model itself.
            "dstore_clear" => {
                want(1)?;
                let key = as_str(&args[0])?.to_string();
                let removed = store_log_path(&key)
                    .map(|p| std::fs::remove_file(p).is_ok())
                    .unwrap_or(false);
                ok!(Value::Int(if removed { 1 } else { 0 }));
            }

            // ── Phase 7 (R12 Slice 5): kernel LLM<Caps> + Goal<M> ───────────────
            // A principal-scoped LLM gateway: per-token cost metering debited from
            // a Slice-1 principal's budget — authority and spend are ONE model.
            // Overrun → graceful fallback + latch (degrade, not crash). Mirrors
            // llm_gateway.ax (I-2), but the budget IS the principal's.

            // `llm_open(model, rate_micro, principal, fallback) -> i64` — open a
            // gateway bound to a principal; rate_micro is µ$ per 1000 tokens.
            "llm_open" => {
                want(4)?;
                let model = as_str(&args[0])?.to_string();
                let rate = as_int(&args[1])?;
                let principal = as_int(&args[2])?;
                let fallback = as_str(&args[3])?.to_string();
                if principal < 0 || self.principals.borrow().get(principal as usize).is_none() {
                    return panic(format!(
                        "[E1604] llm_open: unknown principal handle {principal} \
                         (an LLM gateway must be scoped to a minted principal)"
                    ));
                }
                let mut gws = self.llm_gateways.borrow_mut();
                gws.push(crate::kernel::LlmGateway::new(
                    model,
                    rate,
                    principal as usize,
                    fallback,
                ));
                ok!(Value::Int((gws.len() - 1) as i64));
            }

            // `llm_complete(gw, prompt, tokens) -> i64` — mediate one AI call.
            // Charges the REAL token cost against the principal's budget when it
            // fits (returns the µ$ cost charged); on overrun spends nothing,
            // returns -1, and LATCHES the gateway (every later call also falls
            // back). The "mediates every call" contract: there is no un-metered
            // path. (`prompt` shapes the mock response in a live binding; here the
            // metering is the observable contract.)
            "llm_complete" => {
                want(3)?;
                let gw = as_int(&args[0])?;
                let _prompt = as_str(&args[1])?;
                let tokens = as_int(&args[2])?;
                if gw < 0 {
                    return panic("[E1604] llm_complete: negative gateway handle".to_string());
                }
                // Read gateway state (cost, principal, halted).
                let (cost, principal, halted) = {
                    let gws = self.llm_gateways.borrow();
                    let Some(g) = gws.get(gw as usize) else {
                        return panic(format!("[E1604] llm_complete: unknown gateway {gw}"));
                    };
                    (g.call_cost(tokens), g.principal, g.halted)
                };
                let remaining = self.principals.borrow().budget_remaining(principal);
                if halted || cost > remaining {
                    // Overrun / already-latched: latch, no spend, signal fallback.
                    if let Some(g) = self.llm_gateways.borrow_mut().get_mut(gw as usize) {
                        g.halted = true;
                    }
                    ok!(Value::Int(-1));
                } else {
                    // Affordable: debit the PRINCIPAL's budget by the real cost.
                    self.principals.borrow_mut().spend(principal, cost);
                    if let Some(g) = self.llm_gateways.borrow_mut().get_mut(gw as usize) {
                        g.spent_micro += cost;
                    }
                    ok!(Value::Int(cost));
                }
            }

            // `llm_alive(gw) -> bool` — has the gateway NOT yet latched on an overrun?
            "llm_alive" => {
                want(1)?;
                let gw = as_int(&args[0])?;
                let alive = gw >= 0
                    && self
                        .llm_gateways
                        .borrow()
                        .get(gw as usize)
                        .map(|g| !g.halted)
                        .unwrap_or(false);
                ok!(Value::Bool(alive));
            }

            // `llm_spent(gw) -> i64` — µ$ spent through this gateway so far.
            "llm_spent" => {
                want(1)?;
                let gw = as_int(&args[0])?;
                let spent = if gw >= 0 {
                    self.llm_gateways
                        .borrow()
                        .get(gw as usize)
                        .map(|g| g.spent_micro)
                        .unwrap_or(0)
                } else {
                    0
                };
                ok!(Value::Int(spent));
            }

            // ── R12b: kernel Goal — principal-scoped, budgeted objective runner ──
            // `kernel_goal_create(principal, name, target) -> i64` — register a
            // goal optimizing the @[adaptive] `name` toward `target`, scoped to
            // `principal` (its Slice-1 budget bounds total spend). Typo-guards
            // `name` like goal_run. Returns an opaque goal handle.
            "kernel_goal_create" => {
                want(3)?;
                let principal = as_int(&args[0])?;
                let name = as_str(&args[1])?.to_string();
                let target = as_float(&args[2])?;
                // Typo guard: `name` must be a defined fn or already-recorded goal.
                if !self.fns.contains_key(&name) && !self.provenance.borrow().contains_key(&name) {
                    return panic(format!(
                        "kernel_goal_create: `{name}` is neither a defined function nor a recorded goal"
                    ));
                }
                let mut goals = self.goals.borrow_mut();
                let handle = goals.len();
                goals.push(crate::kernel::KernelGoal::new(
                    principal.max(0) as usize,
                    name,
                    target,
                ));
                ok!(Value::Int(handle as i64));
            }
            // `kernel_goal_run(goal, max_evals) -> f64` — run ≤ max_evals optimizer
            // evaluations, but no more than the principal's remaining budget; debit
            // that budget by the evals used; update best. If the budget bounds the
            // run below max_evals, STOP and raise GoalBudgetExhausted (exit 7) — the
            // partial best stays queryable.
            "kernel_goal_run" => {
                want(2)?;
                let g = as_int(&args[0])?;
                let max_evals = as_int(&args[1])?;
                let (principal, name, target) = {
                    let goals = self.goals.borrow();
                    match goals.get(g.max(0) as usize) {
                        Some(k) => (k.principal, k.name.clone(), k.target),
                        None => return panic(format!("kernel_goal_run: invalid goal handle {g}")),
                    }
                };
                let avail = self.principals.borrow().budget_remaining(principal).max(0);
                let evals = max_evals.max(0).min(avail);
                // Run the existing optimizer for `evals` steps (warm-starts from
                // accumulated provenance, like goal_continue).
                let best = self.run_goal(&name, target, evals)?;
                if evals > 0 {
                    self.principals.borrow_mut().spend(principal, evals);
                }
                {
                    let mut goals = self.goals.borrow_mut();
                    if let Some(k) = goals.get_mut(g.max(0) as usize) {
                        k.evals_spent += evals;
                        k.best_score = best;
                    }
                }
                if evals < max_evals.max(0) {
                    // Budget bounded the run short of the request → exhausted.
                    return Err(Flow::GoalBudgetExhausted(format!(
                        "goal `{name}` (principal {principal}) ran {evals} of {} requested evaluations before its budget was exhausted",
                        max_evals.max(0)
                    )));
                }
                ok!(Value::Float(best));
            }
            // `kernel_goal_best_score(goal) -> f64` — best observed score (no spend).
            "kernel_goal_best_score" => {
                want(1)?;
                let g = as_int(&args[0])?;
                let best = self
                    .goals
                    .borrow()
                    .get(g.max(0) as usize)
                    .map(|k| k.best_score)
                    .unwrap_or(0.0);
                ok!(Value::Float(best));
            }
            // `kernel_goal_spent(goal) -> i64` — evaluations charged to the principal.
            "kernel_goal_spent" => {
                want(1)?;
                let g = as_int(&args[0])?;
                let spent = self
                    .goals
                    .borrow()
                    .get(g.max(0) as usize)
                    .map(|k| k.evals_spent)
                    .unwrap_or(0);
                ok!(Value::Int(spent));
            }
            // `kernel_goal_budget_left(goal) -> i64` — the principal's remaining budget.
            "kernel_goal_budget_left" => {
                want(1)?;
                let g = as_int(&args[0])?;
                let p = self
                    .goals
                    .borrow()
                    .get(g.max(0) as usize)
                    .map(|k| k.principal);
                let left = match p {
                    Some(principal) => self.principals.borrow().budget_remaining(principal).max(0),
                    None => 0,
                };
                ok!(Value::Int(left));
            }

            // `goal_eval(name, input) -> f64` — HELD-OUT evaluation (R5).
            "goal_eval" => {
                want(2)?;
                let name = as_str(&args[0])?.to_string();
                let input = as_int(&args[1])?;
                let score = self.goal_eval_holdout(&name, input)?;
                ok!(Value::Float(score));
            }

            // Full optimization trace as a slice of `(input, score)` tuples,
            // in call order. The companion to goal_run / goal_best_input.
            "goal_history" => {
                want(1)?;
                let name = as_str(&args[0])?.to_string();
                ok!(self.history(&name));
            }

            // ── Agent metacognition (PRD "Agent Metacognition") ──────────────
            // Read-only inspection of the agent's own score trace so it can
            // catch its own failures (a stalled loop, low confidence). All three
            // read the same in-memory provenance `goal_history` exposes; pure
            // functions of the recorded trace, deterministic.
            "agent_detect_loop" => {
                want(1)?;
                let name = as_str(&args[0])?.to_string();
                let store = self.provenance.borrow();
                let stalled = match store.get(&name) {
                    // Need at least 3 points to call it a loop. The last 3 are a
                    // loop when their spread is within epsilon of the score scale.
                    Some(scores) if scores.len() >= 3 => {
                        let tail = &scores[scores.len() - 3..];
                        let lo = tail.iter().cloned().fold(f64::INFINITY, f64::min);
                        let hi = tail.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
                        let scale = hi.abs().max(lo.abs()).max(1.0);
                        (hi - lo) <= scale * 1e-9
                    }
                    _ => false,
                };
                ok!(Value::Bool(stalled));
            }
            "agent_uncertainty" => {
                want(1)?;
                let name = as_str(&args[0])?.to_string();
                let store = self.provenance.borrow();
                let u = match store.get(&name) {
                    Some(scores) if scores.len() >= 2 => {
                        let n = scores.len() as f64;
                        let mean = scores.iter().sum::<f64>() / n;
                        let var = scores.iter().map(|s| (s - mean).powi(2)).sum::<f64>() / n;
                        let stdev = var.sqrt();
                        // Normalize by the score scale → a unitless [0,1]-ish
                        // dispersion; clamp to [0,1] (high spread ⇒ uncertain).
                        let scale = scores
                            .iter()
                            .cloned()
                            .fold(0.0_f64, |a, s| a.max(s.abs()))
                            .max(1.0);
                        (stdev / scale).clamp(0.0, 1.0)
                    }
                    // Not enough trace to judge ⇒ maximally uncertain.
                    _ => 1.0,
                };
                ok!(Value::Float(u));
            }
            "agent_trace_len" => {
                want(1)?;
                let name = as_str(&args[0])?.to_string();
                let store = self.provenance.borrow();
                let len = store.get(&name).map(|s| s.len()).unwrap_or(0);
                ok!(Value::Int(len as i64));
            }

            // Reset the @[adaptive] provenance for `name` so the next
            // goal_run starts fresh. Returns the count of evicted records
            // (so a caller can sanity-check there was anything to clear).
            "goal_clear" => {
                want(1)?;
                let name = as_str(&args[0])?.to_string();
                ok!(Value::Int(self.clear(&name)));
            }

            // R9 corrigibility kill-switch. `corrigible_halt()` trips a one-way
            // latch; from then on every `@[corrigible]` fn call is refused (see
            // `call_fn`). `corrigible_halted()` reports the latch state so a
            // program / supervisor can branch on it. There is deliberately no
            // un-halt builtin: a reversible kill-switch is not a kill-switch.
            "corrigible_halt" => {
                want(0)?;
                self.corrigible_halted.set(true);
                ok!(Value::Unit);
            }
            "corrigible_halted" => {
                want(0)?;
                ok!(Value::Bool(self.corrigible_halted.get()));
            }

            // ── Dict (string-keyed map) ──────────────────────────────────────
            // Reference-shared like Chan: mutating builtins update the same
            // underlying state every handle sees. The workhorse for caches,
            // frequency tables, named state.
            "dict_new" => {
                want(0)?;
                ok!(Value::Dict(Rc::new(RefCell::new(
                    std::collections::BTreeMap::new()
                ))));
            }
            "dict_get" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_get: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let k = as_str(&args[1])?.to_string();
                ok!(match d.borrow().get(&k) {
                    Some(v) => Value::Some(Box::new(v.clone())),
                    None => Value::None,
                });
            }
            "dict_set" => {
                want(3)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_set: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let k = as_str(&args[1])?.to_string();
                d.borrow_mut().insert(k, args[2].clone());
                ok!(Value::Unit);
            }
            "dict_has" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_has: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let k = as_str(&args[1])?.to_string();
                ok!(Value::Bool(d.borrow().contains_key(&k)));
            }
            // `dict_remove(d, k)` — remove entry, return the prior value as
            // `Option<T>`. Mirrors the Map API of every reasonable language.
            "dict_remove" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_remove: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let k = as_str(&args[1])?.to_string();
                ok!(match d.borrow_mut().remove(&k) {
                    Some(v) => Value::Some(Box::new(v)),
                    None => Value::None,
                });
            }
            "dict_len" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_len: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                ok!(Value::Int(d.borrow().len() as i64));
            }
            // `dict_keys(d) -> [str]` — sorted by BTreeMap ordering, so
            // iteration is deterministic.
            "dict_keys" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_keys: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let keys: Vec<Value> = d.borrow().keys().map(|k| Value::Str(k.clone())).collect();
                ok!(Value::Array(keys));
            }
            // `dict_map_values(d, f) -> Dict` — transform every value via
            // a closure, keys preserved. Returns a FRESH dict; the input
            // is not mutated. The dict analogue of arr_map.
            "dict_map_values" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_map_values: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let f = args[1].clone();
                let mut out = std::collections::BTreeMap::new();
                let pairs: Vec<(String, Value)> = d
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (k, v) in pairs {
                    let nv = self.call_closure(f.clone(), vec![v])?;
                    out.insert(k, nv);
                }
                ok!(Value::Dict(Rc::new(RefCell::new(out))));
            }
            // `arr_enumerate(xs)` — turn `[a, b, c]` into `[(0, a), (1, b),
            // (2, c)]`. The "iterate with index" pattern's primitive
            // form. Pairs with `arr_map` so closures can see both index
            // and value without a `let i = 0` + manual increment.
            "arr_enumerate" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "arr_enumerate: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let mut out = Vec::with_capacity(xs.len());
                for (i, v) in xs.iter().enumerate() {
                    out.push(Value::Tuple(vec![Value::Int(i as i64), v.clone()]));
                }
                ok!(Value::Array(out));
            }
            // `arr_partition(xs, pred)` — split into `(yes, no)` tuple
            // where `yes` is the elements satisfying `pred` and `no` is
            // the rest. One pass over the input; the natural complement
            // to `arr_filter` (which only keeps the yes side).
            "arr_partition" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_partition: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let pred = args[1].clone();
                let mut yes = Vec::new();
                let mut no = Vec::new();
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x.clone()])?;
                    match r {
                        Value::Bool(true) => yes.push(x),
                        Value::Bool(false) => no.push(x),
                        other => {
                            return panic(format!(
                                "arr_partition: predicate must return bool, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                ok!(Value::Tuple(vec![Value::Array(yes), Value::Array(no)]));
            }
            // `dict_get_or(d, k, default)` — get the value at `k`, or
            // return `default` if absent. Compresses the ubiquitous
            // `match dict_get(d, k) { Some(v) => v  None => default }`
            // pattern into one call. The `default` can be any Value.
            "dict_get_or" => {
                want(3)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_get_or: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let k = as_str(&args[1])?.to_string();
                let default = args[2].clone();
                let v = d.borrow().get(&k).cloned().unwrap_or(default);
                ok!(v);
            }
            // `dict_inc(d, k)` — atomically bump an i64 counter at `k`.
            // Initializes to 1 if absent. Returns the new value. The
            // standard "increment-a-counter" idiom — replaces the
            // four-line get-default-add-set dance with one call.
            "dict_inc" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_inc: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let k = as_str(&args[1])?.to_string();
                let mut m = d.borrow_mut();
                let cur = m.get(&k).cloned().unwrap_or(Value::Int(0));
                let n = match cur {
                    Value::Int(n) => n + 1,
                    other => {
                        return panic(format!(
                            "dict_inc: existing value at '{k}' is {}, not i64",
                            other.type_name()
                        ))
                    }
                };
                m.insert(k, Value::Int(n));
                ok!(Value::Int(n));
            }
            // `dict_filter(d, pred)` — keep entries where the predicate
            // `fn(str, V) -> bool` holds. Returns a fresh dict. The dict
            // analogue of arr_filter, with the closure seeing (key, value).
            "dict_filter" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_filter: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let pred = args[1].clone();
                let pairs: Vec<(String, Value)> = d
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut out: std::collections::BTreeMap<String, Value> =
                    std::collections::BTreeMap::new();
                for (k, v) in pairs {
                    let keep =
                        self.call_closure(pred.clone(), vec![Value::Str(k.clone()), v.clone()])?;
                    match keep {
                        Value::Bool(true) => {
                            out.insert(k, v);
                        }
                        Value::Bool(false) => {}
                        other => {
                            return panic(format!(
                                "dict_filter: predicate must return bool, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                ok!(Value::Dict(Rc::new(RefCell::new(out))));
            }
            // `dict_to_pairs(d) -> [(str, V)]` — entries as a slice of
            // tuples in BTreeMap key order. The natural primitive for
            // "sort a dict by value": dict_to_pairs → arr_sort_by →
            // arr_take. Empty dict → empty slice.
            "dict_to_pairs" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_to_pairs: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let pairs: Vec<Value> = d
                    .borrow()
                    .iter()
                    .map(|(k, v)| Value::Tuple(vec![Value::Str(k.clone()), v.clone()]))
                    .collect();
                ok!(Value::Array(pairs));
            }
            // `dict_from_pairs(xs) -> Dict` — inverse: build a Dict from
            // a slice of `(str, V)` tuples. Duplicate keys: the LAST
            // pair wins (matches the conventional construction order).
            "dict_from_pairs" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => {
                        return panic(format!(
                            "dict_from_pairs: expected array of (str, V) tuples, got {}",
                            other.type_name()
                        ))
                    }
                };
                let mut out: std::collections::BTreeMap<String, Value> =
                    std::collections::BTreeMap::new();
                for v in xs {
                    let pair = match v {
                        Value::Tuple(t) if t.len() == 2 => t,
                        other => {
                            return panic(format!(
                                "dict_from_pairs: each element must be a 2-tuple (str, V), got {}",
                                other.type_name()
                            ))
                        }
                    };
                    let k = match &pair[0] {
                        Value::Str(s) => s.clone(),
                        other => {
                            return panic(format!(
                                "dict_from_pairs: tuple's first element must be str, got {}",
                                other.type_name()
                            ))
                        }
                    };
                    out.insert(k, pair[1].clone());
                }
                ok!(Value::Dict(Rc::new(RefCell::new(out))));
            }

            // `dict_to_str(d) -> str` — serialize a `Dict<str, str>` to a
            // stable line-oriented text format: `key1=value1\nkey2=value2…`.
            // Each entry on its own line; BTreeMap key order so the output
            // is deterministic (round-trippable + diff-friendly). Non-string
            // values are converted via `display`; keys containing `=` or
            // `\n` are rejected to keep the format unambiguous.
            "dict_to_str" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_to_str: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let mut out = String::new();
                for (k, v) in d.borrow().iter() {
                    // Un-representable key/value is a recoverable condition, not
                    // a host crash: return `Err(msg)` so the caller can react
                    // (BUG_HUNT #20). `no exceptions — Result everywhere`.
                    if k.contains('=') || k.contains('\n') {
                        ok!(Value::Err(Box::new(Value::Str(format!(
                            "dict_to_str: key '{k}' contains an unrepresentable char (= or newline)"
                        )))));
                    }
                    let vs = display(v);
                    if vs.contains('\n') {
                        ok!(Value::Err(Box::new(Value::Str(format!(
                            "dict_to_str: value for key '{k}' contains a newline (unsupported)"
                        )))));
                    }
                    out.push_str(k);
                    out.push('=');
                    out.push_str(&vs);
                    out.push('\n');
                }
                ok!(Value::Ok(Box::new(Value::Str(out))));
            }
            // `dict_from_str(s) -> Dict` — inverse of `dict_to_str`.
            // Splits `s` into lines, each line at the FIRST `=` into
            // (key, value). All values are stored as `str` — caller
            // converts via `parse_int` / `parse_float` if needed.
            // Trailing newline is allowed; empty AND malformed lines are
            // skipped (lenient, #31). Use `dict_try_from_str` for strict
            // Result-returning parsing.
            "dict_from_str" => {
                // BUG_HUNT #31: a malformed line no longer PANICS (aborting the
                // whole program on untrusted input). `dict_from_str` is lenient —
                // it SKIPS malformed lines (like it already skips empty lines) —
                // and `dict_try_from_str` is the strict, Result-returning sibling
                // for callers that want to reject malformed input recoverably.
                want(1)?;
                let s = as_str(&args[0])?;
                let mut out = std::collections::BTreeMap::new();
                for line in s.lines() {
                    if line.is_empty() {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once('=') {
                        out.insert(k.to_string(), Value::Str(v.to_string()));
                    }
                    // malformed (no '=') → skipped (lenient).
                }
                ok!(Value::Dict(Rc::new(RefCell::new(out))));
            }
            // BUG_HUNT #31: strict variant — a malformed line is a recoverable
            // `Err(message)`, not a panic. The language's "Result, not
            // exceptions" parse-on-untrusted-input form.
            "dict_try_from_str" => {
                want(1)?;
                let s = as_str(&args[0])?;
                let mut out = std::collections::BTreeMap::new();
                for line in s.lines() {
                    if line.is_empty() {
                        continue;
                    }
                    match line.split_once('=') {
                        Some((k, v)) => {
                            out.insert(k.to_string(), Value::Str(v.to_string()));
                        }
                        None => {
                            ok!(Value::Err(Box::new(Value::Str(format!(
                                "dict_try_from_str: malformed line '{line}' (expected key=value)"
                            )))));
                        }
                    }
                }
                ok!(Value::Ok(Box::new(Value::Dict(Rc::new(RefCell::new(out))))));
            }

            // `dict_merge(d1, d2) -> Dict` — union of two dicts. Right-
            // biased on collision (values in `d2` win when both have the
            // same key). Returns a fresh dict; the inputs are unchanged.
            "dict_merge" => {
                want(2)?;
                let d1 = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_merge: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let d2 = match &args[1] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_merge: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let mut out: std::collections::BTreeMap<String, Value> = d1.borrow().clone();
                for (k, v) in d2.borrow().iter() {
                    out.insert(k.clone(), v.clone());
                }
                ok!(Value::Dict(Rc::new(RefCell::new(out))));
            }

            // `arr_max_by(xs, key_fn)` / `arr_min_by(xs, key_fn)` —
            // pick the element that maximizes / minimizes the closure's
            // numeric output. Folds three calls (arr_map + arr_argmax +
            // index) into one. Panics on an empty array (no sensible
            // default for an unbounded ordering).
            "arr_max_by" | "arr_min_by" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!("{name}: expected array, got {}", other.type_name()))
                    }
                };
                if xs.is_empty() {
                    return panic(format!("{name}: array is empty"));
                }
                let key_fn = args[1].clone();
                let pick_max = name == "arr_max_by";
                let to_f = |v: Value| -> Result<f64, Flow> {
                    match v {
                        Value::Int(n) => Ok(n as f64),
                        Value::Float(f) => Ok(f),
                        other => Err(Flow::Panic(format!(
                            "{name}: key fn must return numeric, got {}",
                            other.type_name()
                        ))),
                    }
                };
                let mut best_idx = 0;
                let mut best_key = to_f(self.call_closure(key_fn.clone(), vec![xs[0].clone()])?)?;
                for (i, x) in xs.iter().enumerate().skip(1) {
                    let k = to_f(self.call_closure(key_fn.clone(), vec![x.clone()])?)?;
                    if (pick_max && k > best_key) || (!pick_max && k < best_key) {
                        best_key = k;
                        best_idx = i;
                    }
                }
                ok!(xs[best_idx].clone());
            }
            // `arr_take_while(xs, pred)` — prefix that satisfies pred,
            // up to (not including) the first failing element. The
            // streaming-prefix dual of arr_filter.
            "arr_take_while" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_take_while: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let pred = args[1].clone();
                let mut out = Vec::new();
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x.clone()])?;
                    match r {
                        Value::Bool(true) => out.push(x),
                        Value::Bool(false) => break,
                        other => {
                            return panic(format!(
                                "arr_take_while: predicate must return bool, got {}",
                                other.type_name()
                            ))
                        }
                    }
                }
                ok!(Value::Array(out));
            }
            // `arr_drop_while(xs, pred)` — skip leading elements that
            // satisfy pred, keep the rest. Complement of arr_take_while.
            "arr_drop_while" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_drop_while: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let pred = args[1].clone();
                let mut still_dropping = true;
                let mut out = Vec::new();
                for x in xs {
                    if still_dropping {
                        let r = self.call_closure(pred.clone(), vec![x.clone()])?;
                        match r {
                            Value::Bool(true) => continue,
                            Value::Bool(false) => still_dropping = false,
                            other => {
                                return panic(format!(
                                    "arr_drop_while: predicate must return bool, got {}",
                                    other.type_name()
                                ))
                            }
                        }
                    }
                    out.push(x);
                }
                ok!(Value::Array(out));
            }
            // `dict_each(d, f)` — iterate (k, v) pairs via a closure
            // for side effects. Closure takes (str, V); return is
            // ignored. Useful for "print every entry" or "write each
            // to disk" patterns where you don't want to materialize
            // a new dict via dict_map_values.
            "dict_each" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_each: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let f = args[1].clone();
                let pairs: Vec<(String, Value)> = d
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (k, v) in pairs {
                    let _ = self.call_closure(f.clone(), vec![Value::Str(k), v])?;
                }
                ok!(Value::Unit);
            }

            // `arr_group_by(xs, key_fn) -> Dict[str, [T]]` — bucket the
            // array by a closure that maps each element to a string key.
            // Stable: elements appear in their input order within each
            // bucket. The natural "frequency table" / "by-category"
            // reduction.
            "arr_group_by" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "arr_group_by: expected array, got {}",
                            other.type_name()
                        ))
                    }
                };
                let key_fn = args[1].clone();
                let mut out: std::collections::BTreeMap<String, Vec<Value>> =
                    std::collections::BTreeMap::new();
                for x in xs {
                    let k = self.call_closure(key_fn.clone(), vec![x.clone()])?;
                    let key = match k {
                        Value::Str(s) => s,
                        other => {
                            return panic(format!(
                                "arr_group_by: key fn must return str, got {}",
                                other.type_name()
                            ))
                        }
                    };
                    out.entry(key).or_default().push(x);
                }
                let map = out.into_iter().map(|(k, v)| (k, Value::Array(v))).collect();
                ok!(Value::Dict(Rc::new(RefCell::new(map))));
            }
            // `dict_values(d) -> [V]` — values in key-sorted order.
            "dict_values" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => {
                        return panic(format!(
                            "dict_values: expected dict, got {}",
                            other.type_name()
                        ))
                    }
                };
                let vals: Vec<Value> = d.borrow().values().cloned().collect();
                ok!(Value::Array(vals));
            }

            // ── ASI: live LLM calls (require `--features asi-runtime`) ───────
            "ai_complete" => {
                want(1)?;
                let prompt = as_str(&args[0])?.to_string();
                let caller = self.current_fn.borrow().clone();
                let params = "max_tokens=default;temperature=default";
                // R3 §4.2: resolve the tier from the enclosing @[ai(policy(tier))]
                // (default balanced); an unknown tier name is E1302. The resolved
                // tier picks the concrete (model, version) from the host table, so
                // the provenance record names the REAL routed model, not a
                // hardcoded placeholder. (Per-call `tier:` args are deferred until
                // named-arg call syntax — this covers policy + default.)
                let tier = self.current_ai_tier()?;
                let tier_name = tier.as_str();
                let (model_id, model_ver) = tier.model();
                // Phase-7 cost_meter / F4: estimate the call's tokens
                // deterministically from the prompt length (~4 chars/token, a
                // standard rule of thumb, +1 so an empty prompt still costs a
                // token) and compute the real per-token cost at this tier's
                // rate. Deterministic ⇒ mock/offline runs meter the same as
                // live. The meter is only CHARGED after the R3c budget gate
                // below (an over-budget call that panics dispatches nothing, so
                // it must not charge cost).
                let est_tokens = (prompt.len() as i64) / 4 + 1;
                let cost_micro = tier.cost_micro(est_tokens);
                let cost_usd = cost_micro as f64 / 1_000_000.0;
                // R3c: meter this call against the fn's @[ai(policy(budget: N))].
                // The (N+1)th call halts with E1301 BEFORE any model dispatch
                // (mock/live/fallback) and before the W1310 warning — an
                // over-budget call is a hard failure, not a stray response. A fn
                // with no budget is unmetered (current_ai_budget → None).
                if let Some(budget) = self.current_ai_budget() {
                    let used = self.ai_calls_this_fn.get();
                    if used >= budget {
                        let who = if caller.is_empty() {
                            "<main>".to_string()
                        } else {
                            caller.clone()
                        };
                        return ai_policy_err(format!(
                            "[{}] `{who}` exceeded its AI budget of {budget} call(s) — \
                             raise the budget or reduce ai_complete calls",
                            crate::error::E1301,
                        ));
                    }
                    self.ai_calls_this_fn.set(used + 1);
                }
                // W1310: a fn making an AI call with no @[ai(policy)] is allowed,
                // but its cost is unmetered and the call un-pinned — warn once so
                // the audit gap is visible (only meaningful for live/mock calls).
                if !self.current_fn_has_ai_policy() {
                    let who = if caller.is_empty() {
                        "<main>".to_string()
                    } else {
                        caller.clone()
                    };
                    eprintln!(
                        "warning: [{}] AI call in `{who}` has no @[ai(policy)] — cost is unmetered and the call is harder to audit",
                        crate::error::W1310
                    );
                }
                // F2 (ROADMAP §9.5): deterministic REPLAY. With AXON_AI_REPLAY set,
                // a previously-recorded (prompt, model) response is replayed
                // verbatim — no mock, no live call, no API key — so an AI run is
                // exactly reproducible (the auditability backbone). Checked BEFORE
                // mock/live; the recorded token-count drives the (deterministic)
                // cost so the metered cost reproduces too. A miss falls through and
                // the mock/live branches RECORD their response below.
                let replay_model = tier.api_model();
                // F3: the goal being optimized when this call fired (causal link).
                let goal = self.current_goal_name().unwrap_or_default();
                // F3 (Phase 9): principal name for audit attribution.
                let principal = self.current_principal_name();
                if let Some((cached, cached_tokens)) = ai_replay_lookup(&prompt, &replay_model) {
                    let micro = tier.cost_micro(cached_tokens);
                    self.ai_cost_micro.set(self.ai_cost_micro.get() + micro);
                    append_ai_call_jsonl(
                        &caller,
                        &prompt,
                        tier_name,
                        model_id,
                        model_ver,
                        params,
                        "replay",
                        "",
                        micro as f64 / 1_000_000.0,
                        &goal,
                        "AI",
                        &principal,
                    );
                    ok!(Value::Ok(Box::new(Value::Str(cached))));
                }
                if ai_mock_enabled() {
                    // Deterministic stub — but a fully-stamped provenance record
                    // (mode:"mock") with the REAL per-token cost charged to the
                    // meter, so the audit trail and the cost budget are honest
                    // about what a call costs even under mock. The tier/model are
                    // the RESOLVED routing (the routing is real; only the
                    // response is stubbed), so the cost is the routing's cost.
                    let stub = "Mock summary: the single most important fact, stated concisely."
                        .to_string();
                    self.ai_cost_micro
                        .set(self.ai_cost_micro.get() + cost_micro);
                    append_ai_call_jsonl(
                        &caller, &prompt, tier_name, model_id, model_ver, params, "mock", "",
                        cost_usd, &goal, "AI", &principal,
                    );
                    // Record so a re-run replays this exact response (under mock the
                    // recorded tokens are the deterministic estimate).
                    ai_replay_store(&prompt, &replay_model, &stub, est_tokens);
                    ok!(Value::Ok(Box::new(Value::Str(stub))));
                }
                #[cfg(feature = "asi-runtime")]
                {
                    // Live call: route to the RESOLVED tier's model (R3 — the
                    // live request now actually honors the tier; `strong` reaches
                    // the strong model, not the hardcoded sonnet). The model is
                    // env-overridable per tier for a proxy/gateway deployment.
                    // Charge the per-token cost to the meter and stamp it into the
                    // provenance (Phase-7 cost_meter / F4 — was 0). R3: use the
                    // model's REAL token count (input+output from `usage`) for the
                    // charge, not the pre-dispatch prompt-length estimate — so the
                    // metered cost matches what the provider actually billed. (The
                    // BUDGET gate above still uses the estimate, correctly: it must
                    // decide before the call whether to dispatch at all.)
                    ok!(
                        match axon_ai::complete_with_model_usage(&prompt, &replay_model) {
                            Ok((s, real_tokens)) => {
                                let real_micro = tier.cost_micro(real_tokens);
                                let real_usd = real_micro as f64 / 1_000_000.0;
                                self.ai_cost_micro
                                    .set(self.ai_cost_micro.get() + real_micro);
                                append_ai_call_jsonl(
                                    &caller, &prompt, tier_name, model_id, model_ver, params,
                                    "live", "", real_usd, &goal, "AI", &principal,
                                );
                                // Record the live (response, real token-count) so a
                                // re-run with the same AXON_AI_REPLAY file reproduces
                                // this exact response AND cost — the F2 replay engine.
                                ai_replay_store(&prompt, &replay_model, &s, real_tokens);
                                Value::Ok(Box::new(Value::Str(s)))
                            }
                            Err(e) => Value::Err(Box::new(Value::Str(e))),
                        }
                    );
                }
                #[cfg(not(feature = "asi-runtime"))]
                {
                    // Offline (no live model compiled in, mock not set). R3 §3.3:
                    // a declared `@[ai(policy(fallback: …))]` keeps the program
                    // total — return Ok(fallback) stamped mode:"fallback" so the
                    // audit trail never mistakes it for a model answer. With no
                    // fallback in scope this is E1300, not a generic panic: a
                    // program that wants to run offline MUST declare a fallback.
                    if let Some(fallback) = self.current_ai_fallback() {
                        // Fallback: NO model was reached, so NO cost is charged
                        // (cost_usd stays the unused estimate; the record is 0).
                        // The cost meter only reflects calls that actually
                        // dispatched to a (mock or live) model.
                        append_ai_call_jsonl(
                            &caller,
                            &prompt,
                            tier_name,
                            "none",
                            "offline",
                            params,
                            "fallback",
                            "offline: no model reachable",
                            0.0,
                            &goal,
                            "AI",
                            &principal,
                        );
                        ok!(Value::Ok(Box::new(Value::Str(fallback))));
                    }
                    ai_policy_err(format!(
                        "[{}] `ai_complete` cannot run: no model reachable and no \
                         @[ai(policy(fallback: …))] in scope — declare a fallback to run \
                         offline (or set AXON_AI_MOCK=1 / build --features asi-runtime)",
                        crate::error::E1300,
                    ))
                }
            }
            "ai_extract_uncertain_i64" => {
                want(1)?;
                if ai_mock_enabled() {
                    ok!(Value::Ok(Box::new(make_uncertain(Value::Int(1), 0.9))));
                }
                #[cfg(feature = "asi-runtime")]
                {
                    ok!(
                        match axon_ai::complete_typed_uncertain_i64(as_str(&args[0])?) {
                            Ok((v, c)) => Value::Ok(Box::new(make_uncertain(Value::Int(v), c))),
                            Err(e) => Value::Err(Box::new(Value::Str(e))),
                        }
                    );
                }
                #[cfg(not(feature = "asi-runtime"))]
                return panic(
                    "ai_extract_uncertain_i64 requires --features asi-runtime (or set AXON_AI_MOCK=1)",
                );
            }
            "ai_extract_uncertain_f64" => {
                want(1)?;
                if ai_mock_enabled() {
                    ok!(Value::Ok(Box::new(make_uncertain(Value::Float(1.0), 0.9))));
                }
                #[cfg(feature = "asi-runtime")]
                {
                    ok!(
                        match axon_ai::complete_typed_uncertain_f64(as_str(&args[0])?) {
                            Ok((v, c)) => Value::Ok(Box::new(make_uncertain(Value::Float(v), c))),
                            Err(e) => Value::Err(Box::new(Value::Str(e))),
                        }
                    );
                }
                #[cfg(not(feature = "asi-runtime"))]
                return panic(
                    "ai_extract_uncertain_f64 requires --features asi-runtime (or set AXON_AI_MOCK=1)",
                );
            }

            // ── Phase 13: Probabilistic distribution builtins ───────────────────
            // Pure helpers (CDF/PDF/moments) — no RNG, no side effects.
            // Impure sampling variants share the same section but carry the
            // "Random" effect row (enforced in builtins.rs is_impure_builtin).
            "gaussian_pdf" => {
                want(3)?;
                let mu = as_float(&args[0])?;
                let sigma = as_float(&args[1])?;
                let x = as_float(&args[2])?;
                if sigma <= 0.0 {
                    return panic(format!("gaussian_pdf: sigma must be > 0 (got {sigma})"));
                }
                let z = (x - mu) / sigma;
                let pdf = (-0.5 * z * z).exp() / (sigma * (2.0 * std::f64::consts::PI).sqrt());
                ok!(Value::Float(pdf));
            }

            "gaussian_cdf" => {
                want(3)?;
                let mu = as_float(&args[0])?;
                let sigma = as_float(&args[1])?;
                let x = as_float(&args[2])?;
                if sigma <= 0.0 {
                    return panic(format!("gaussian_cdf: sigma must be > 0 (got {sigma})"));
                }
                let z = (x - mu) / (sigma * std::f64::consts::SQRT_2);
                ok!(Value::Float(0.5 * (1.0 + erf_approx(z))));
            }

            "gaussian_sample" => {
                want(2)?;
                let mu = as_float(&args[0])?;
                let sigma = as_float(&args[1])?;
                if sigma <= 0.0 {
                    return panic(format!("gaussian_sample: sigma must be > 0 (got {sigma})"));
                }
                ok!(Value::Float(mu + sigma * std_normal_sample()));
            }

            "beta_mean" => {
                want(2)?;
                let alpha = as_float(&args[0])?;
                let beta_b = as_float(&args[1])?;
                if alpha <= 0.0 || beta_b <= 0.0 {
                    return panic(format!(
                        "beta_mean: alpha and beta_b must be > 0 (got {alpha}, {beta_b})"
                    ));
                }
                ok!(Value::Float(alpha / (alpha + beta_b)));
            }

            "beta_variance" => {
                want(2)?;
                let alpha = as_float(&args[0])?;
                let beta_b = as_float(&args[1])?;
                if alpha <= 0.0 || beta_b <= 0.0 {
                    return panic(format!(
                        "beta_variance: alpha and beta_b must be > 0 (got {alpha}, {beta_b})"
                    ));
                }
                let s = alpha + beta_b;
                ok!(Value::Float((alpha * beta_b) / (s * s * (s + 1.0))));
            }

            "beta_cdf" => {
                want(3)?;
                let alpha = as_float(&args[0])?;
                let beta_b = as_float(&args[1])?;
                let x = as_float(&args[2])?;
                if alpha <= 0.0 || beta_b <= 0.0 {
                    return panic(format!(
                        "beta_cdf: alpha and beta_b must be > 0 (got {alpha}, {beta_b})"
                    ));
                }
                ok!(Value::Float(reg_inc_beta(alpha, beta_b, x)));
            }

            "beta_sample" => {
                want(2)?;
                let alpha = as_float(&args[0])?;
                let beta_b = as_float(&args[1])?;
                if alpha <= 0.0 || beta_b <= 0.0 {
                    return panic(format!(
                        "beta_sample: alpha and beta_b must be > 0 (got {alpha}, {beta_b})"
                    ));
                }
                // Beta(alpha, beta_b) = Gamma(alpha) / (Gamma(alpha) + Gamma(beta_b))
                let ga = gamma_sample(alpha);
                let gb = gamma_sample(beta_b);
                let s = ga + gb;
                ok!(Value::Float(if s > 0.0 {
                    ga / s
                } else {
                    alpha / (alpha + beta_b)
                }));
            }

            "categorical_mean" => {
                want(1)?;
                let probs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "categorical_mean: expected [f64], got {}",
                            other.type_name()
                        ))
                    }
                };
                if probs.is_empty() {
                    return panic("categorical_mean: probs must be non-empty".to_string());
                }
                let mean: f64 = probs
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        i as f64
                            * match v {
                                Value::Float(f) => *f,
                                Value::Int(n) => *n as f64,
                                _ => 0.0,
                            }
                    })
                    .sum();
                ok!(Value::Float(mean));
            }

            "categorical_variance" => {
                want(1)?;
                let probs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "categorical_variance: expected [f64], got {}",
                            other.type_name()
                        ))
                    }
                };
                if probs.is_empty() {
                    return panic("categorical_variance: probs must be non-empty".to_string());
                }
                let mean: f64 = probs
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        i as f64
                            * match v {
                                Value::Float(f) => *f,
                                Value::Int(n) => *n as f64,
                                _ => 0.0,
                            }
                    })
                    .sum();
                let e_x2: f64 = probs
                    .iter()
                    .enumerate()
                    .map(|(i, v)| {
                        (i as f64)
                            * (i as f64)
                            * match v {
                                Value::Float(f) => *f,
                                Value::Int(n) => *n as f64,
                                _ => 0.0,
                            }
                    })
                    .sum();
                ok!(Value::Float(e_x2 - mean * mean));
            }

            "categorical_cdf" => {
                want(2)?;
                let probs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "categorical_cdf: expected [f64], got {}",
                            other.type_name()
                        ))
                    }
                };
                let k = as_int(&args[1])?;
                if probs.is_empty() {
                    return panic("categorical_cdf: probs must be non-empty".to_string());
                }
                if k < 0 {
                    ok!(Value::Float(0.0));
                }
                let k_usize = k as usize;
                let cdf: f64 = probs
                    .iter()
                    .take(k_usize + 1)
                    .map(|v| match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        _ => 0.0,
                    })
                    .sum::<f64>()
                    .min(1.0);
                ok!(Value::Float(cdf));
            }

            "categorical_sample" => {
                want(1)?;
                let probs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => {
                        return panic(format!(
                            "categorical_sample: expected [f64], got {}",
                            other.type_name()
                        ))
                    }
                };
                if probs.is_empty() {
                    return panic("categorical_sample: probs must be non-empty".to_string());
                }
                let u = (next_rand_u64() >> 11) as f64 / 9_007_199_254_740_992.0;
                let mut cum = 0.0;
                let mut result = probs.len() as i64 - 1;
                for (i, v) in probs.iter().enumerate() {
                    let p = match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        _ => 0.0,
                    };
                    cum += p;
                    if u < cum {
                        result = i as i64;
                        break;
                    }
                }
                ok!(Value::Int(result));
            }

            // ── R17 HAL builtins — interp stubs (codegen-only) ──────────────────
            // Raw hardware access cannot be emulated in the tree-walking interpreter;
            // these builtins only run in a compiled freestanding binary.  An honest
            // E0910 abort is safer than a silent wrong result.
            "ptr_from_addr" | "volatile_load_u8" | "volatile_load_u16" | "volatile_load_u32"
            | "volatile_load_u64" | "volatile_store_u8" | "volatile_store_u16"
            | "volatile_store_u32" | "volatile_store_u64" | "hlt" | "cli" | "sti" => {
                Err(crate::interp::Flow::Panic(format!(
                    "[E0910] `{name}` is a HAL builtin — it requires native codegen \
                     (`axon build --freestanding`) and cannot run in the interpreter. \
                     Use `axon check` to type-check the kernel source without running it."
                )))
            }

            _ => Ok(None),
        }
    }
}

// ── Phase 13 math helpers ─────────────────────────────────────────────────────

/// erf(x) approximation — Abramowitz & Stegun 7.1.26, max error ≤ 1.5e-7.
fn erf_approx(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = x.abs();
    let t = 1.0 / (1.0 + 0.3275911 * ax);
    let poly = ((((1.061405429 * t - 1.453152027) * t + 1.421413741) * t - 0.284496736) * t
        + 0.254829592)
        * t;
    sign * (1.0 - poly * (-ax * ax).exp())
}

/// Standard normal sample via Box-Muller transform.
fn std_normal_sample() -> f64 {
    let u1 = ((next_rand_u64() >> 11) as f64 / 9_007_199_254_740_992.0).max(1e-300);
    let u2 = (next_rand_u64() >> 11) as f64 / 9_007_199_254_740_992.0;
    (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos()
}

/// Gamma(k) sample via Marsaglia-Tsang "squeeze" method.
/// Works for any k > 0 (uses k < 1 reduction: Gamma(k) = Gamma(k+1) * U^(1/k)).
fn gamma_sample(k: f64) -> f64 {
    if k < 1.0 {
        let u = ((next_rand_u64() >> 11) as f64 / 9_007_199_254_740_992.0).max(1e-300);
        return gamma_sample(k + 1.0) * u.powf(1.0 / k);
    }
    let d = k - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x = std_normal_sample();
        let v_inner = 1.0 + c * x;
        if v_inner <= 0.0 {
            continue;
        }
        let v = v_inner * v_inner * v_inner;
        let u = ((next_rand_u64() >> 11) as f64 / 9_007_199_254_740_992.0).max(1e-300);
        let x2 = x * x;
        if u < 1.0 - 0.0331 * x2 * x2 {
            return d * v;
        }
        if u.ln() < 0.5 * x2 + d * (1.0 - v + v.ln()) {
            return d * v;
        }
    }
}

/// Regularized incomplete beta I_x(a, b) via Lentz continued fraction.
/// Returns P(X ≤ x) for X ~ Beta(a, b). Clamped to [0, 1].
fn reg_inc_beta(a: f64, b: f64, x: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    // Use symmetry for numerical stability when x > (a+1)/(a+b+2)
    let sym = (a + 1.0) / (a + b + 2.0);
    if x > sym {
        return 1.0 - reg_inc_beta(b, a, 1.0 - x);
    }
    // log of Beta(a, b) via Stirling-accurate log-gamma approximation
    let lbeta_val = log_gamma(a) + log_gamma(b) - log_gamma(a + b);
    let front = (a * x.ln() + b * (1.0 - x).ln() - lbeta_val).exp() / a;
    front * beta_cf(a, b, x)
}

/// Log-gamma via Lanczos approximation (g=7, n=9 coefficients, ~15 digits).
fn log_gamma(z: f64) -> f64 {
    const G: f64 = 7.0;
    const C: [f64; 9] = [
        0.999_999_999_999_809_9,
        676.520_368_121_885_1,
        -1_259.139_216_722_402_8,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_312_2e-7,
    ];
    if z < 0.5 {
        // Reflection: Γ(z)Γ(1-z) = π/sin(πz)
        std::f64::consts::PI.ln() - (std::f64::consts::PI * z).sin().ln() - log_gamma(1.0 - z)
    } else {
        let z = z - 1.0;
        let x = C[0]
            + C[1..]
                .iter()
                .enumerate()
                .map(|(i, &c)| c / (z + i as f64 + 1.0))
                .sum::<f64>();
        let t = z + G + 0.5;
        (2.0 * std::f64::consts::PI).sqrt().ln() + x.ln() + (z + 0.5) * t.ln() - t
    }
}

/// Lentz's continued fraction for the incomplete beta function.
/// Evaluates the CF expansion convergent to beta_cf(a, b, x).
fn beta_cf(a: f64, b: f64, x: f64) -> f64 {
    const MAX_ITER: usize = 200;
    const EPS: f64 = 3.0e-7;
    const TINY: f64 = 1.0e-30;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0_f64;
    let mut d = (1.0 - qab * x / qap).abs().max(TINY);
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=MAX_ITER {
        let m = m as f64;
        let m2 = 2.0 * m;
        // Even step
        let aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = (1.0 + aa * d).abs().max(TINY);
        c = (1.0 + aa / c).abs().max(TINY);
        d = 1.0 / d;
        h *= d * c;
        // Odd step
        let aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = (1.0 + aa * d).abs().max(TINY);
        c = (1.0 + aa / c).abs().max(TINY);
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < EPS {
            break;
        }
    }
    h
}
