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

impl<'p> Interp<'p> {
    // ── Builtins ───────────────────────────────────────────────────────────────

    /// Dispatch a builtin call. Returns `Ok(Some(v))` if `name` is a builtin,
    /// `Ok(None)` if it is not (caller should try user functions).
    pub(super) fn call_builtin(&self, name: &str, args: &[Value]) -> Result<Option<Value>, Flow> {
        // Helpers --------------------------------------------------------------
        let want = |n: usize| -> Result<(), Flow> {
            if args.len() == n {
                Ok(())
            } else {
                Err(Flow::Panic(format!("{name}: expected {n} args, got {}", args.len())))
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
                append_agent_action_jsonl(&agent_fn, name, cap);
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
                    other => return panic(format!("exec: args must be [str], got {}", other.type_name())),
                };
                match crate::host::with_host(|h| h.exec(&cmd, &arg_list)) {
                    Ok(s) => ok!(Value::Ok(Box::new(Value::Str(s)))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e)))),
                }
            }

            // ── Conversion / formatting ─────────────────────────────────────────
            "to_str" => {
                want(1)?;
                // Polymorphic over scalars (BUG_HUNT #29): dispatch on the
                // runtime value so to_str(i64|f64|bool) all work. Int/Float/Bool
                // render identically to to_str / to_str_f64 / to_str_bool
                // respectively (display() shares fmt_g + "true"/"false").
                ok!(Value::Str(match &args[0] {
                    Value::Int(_) | Value::Float(_) | Value::Bool(_) => display(&args[0]),
                    other => return panic(format!(
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
                ok!(Value::Str(if as_bool(&args[0])? { "true" } else { "false" }.to_string()));
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
                        other => return panic(format!(
                            "parse_bool_or: default must be bool, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!("len: expected str/array, got {}", other.type_name())),
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
                    return panic(
                        "abs_i32 overflow (i32::MIN has no positive)",
                    );
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
                    other => return panic(format!(
                        "arr_push: expected array, got {}",
                        other.type_name()
                    )),
                };
                xs.push(args[1].clone());
                ok!(Value::Array(xs));
            }
            // Sum of an i64 array. Empty → 0. Saturates on overflow.
            "arr_sum_i64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_sum_i64: expected array, got {}",
                        other.type_name()
                    )),
                };
                let mut s: i64 = 0;
                for v in xs {
                    let n = match v {
                        Value::Int(n) => *n,
                        other => return panic(format!(
                            "arr_sum_i64: element must be i64, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "arr_map: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_fold: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_sort_by: expected array, got {}",
                        other.type_name()
                    )),
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
                            other => return panic(format!(
                                "arr_sort_by: comparator must return i64, got {}",
                                other.type_name()
                            )),
                        };
                        if r < 0 { break; }
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
                    other => return panic(format!(
                        "arr_concat: expected array, got {}",
                        other.type_name()
                    )),
                };
                let ys = match &args[1] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_concat: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_flatten: expected array of arrays, got {}",
                        other.type_name()
                    )),
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
                        other => return panic(format!(
                            "arr_flatten: inner element must be array, got {}",
                            other.type_name()
                        )),
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
                    Value::Bool(b) => if *b { 1.0 } else { 0.0 },
                    other => return panic(format!(
                        "as_f64: expected i64/f64/bool, got {}",
                        other.type_name()
                    )),
                };
                ok!(Value::Float(f));
            }
            "as_i64" => {
                want(1)?;
                let n = match &args[0] {
                    Value::Int(n) => *n,
                    Value::Float(v) => *v as i64,
                    Value::Bool(b) => if *b { 1 } else { 0 },
                    other => return panic(format!(
                        "as_i64: expected i64/f64/bool, got {}",
                        other.type_name()
                    )),
                };
                ok!(Value::Int(n));
            }

            // ── Polymorphic slicing / reordering ──────────────────────────
            "arr_reverse" => {
                want(1)?;
                let mut xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_reverse: expected array, got {}",
                        other.type_name()
                    )),
                };
                xs.reverse();
                ok!(Value::Array(xs));
            }
            "arr_take" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_take: expected array, got {}",
                        other.type_name()
                    )),
                };
                let n = as_int(&args[1])?.max(0) as usize;
                let take = n.min(xs.len());
                ok!(Value::Array(xs[..take].to_vec()));
            }
            "arr_drop" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_drop: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_sum_f64: expected array, got {}",
                        other.type_name()
                    )),
                };
                let mut s = 0.0_f64;
                for v in xs {
                    let f = match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        other => return panic(format!(
                            "arr_sum_f64: element must be numeric, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "arr_mean_i64: expected array, got {}",
                        other.type_name()
                    )),
                };
                if xs.is_empty() { ok!(Value::Float(0.0)); }
                let mut s: i64 = 0;
                for v in xs {
                    let n = match v {
                        Value::Int(n) => *n,
                        other => return panic(format!(
                            "arr_mean_i64: element must be i64, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "arr_mean_f64: expected array, got {}",
                        other.type_name()
                    )),
                };
                if xs.is_empty() { ok!(Value::Float(0.0)); }
                let mut s = 0.0_f64;
                for v in xs {
                    let f = match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        other => return panic(format!(
                            "arr_mean_f64: element must be numeric, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "arr_std_f64: expected array, got {}",
                        other.type_name()
                    )),
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
                        other => return panic(format!(
                            "arr_std_f64: element must be numeric, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "{name}: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "{name}: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "str_join: expected array, got {}",
                        other.type_name()
                    )),
                };
                let sep = as_str(&args[1])?.to_string();
                let mut parts = Vec::with_capacity(xs.len());
                for v in xs {
                    match v {
                        Value::Str(s) => parts.push(s.clone()),
                        other => return panic(format!(
                            "str_join: element must be str, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "arr_zip: expected array, got {}",
                        other.type_name()
                    )),
                };
                let ys = match &args[1] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_zip: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_chunk: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_unique: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_index_of: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_any: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut hit = false;
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x])?;
                    match r {
                        Value::Bool(true) => { hit = true; break; }
                        Value::Bool(false) => {}
                        other => return panic(format!(
                            "arr_any: predicate must return bool, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "arr_all: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut all = true;
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x])?;
                    match r {
                        Value::Bool(true) => {}
                        Value::Bool(false) => { all = false; break; }
                        other => return panic(format!(
                            "arr_all: predicate must return bool, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "arr_count_if: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut n: i64 = 0;
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x])?;
                    match r {
                        Value::Bool(true) => { n += 1; }
                        Value::Bool(false) => {}
                        other => return panic(format!(
                            "arr_count_if: predicate must return bool, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "arr_zip_with: expected array, got {}",
                        other.type_name()
                    )),
                };
                let ys = match &args[1] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_zip_with: expected array, got {}",
                        other.type_name()
                    )),
                };
                let f = args[2].clone();
                let n = xs.len().min(ys.len());
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let z = self.call_closure(
                        f.clone(),
                        vec![xs[i].clone(), ys[i].clone()],
                    )?;
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
                    other => return panic(format!(
                        "arr_find: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut hit: Option<Value> = None;
                for x in xs {
                    let keep = self.call_closure(pred.clone(), vec![x.clone()])?;
                    match keep {
                        Value::Bool(true) => { hit = Some(x); break; }
                        Value::Bool(false) => {}
                        other => return panic(format!(
                            "arr_find: predicate must return bool, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "arr_contains: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_filter: expected array, got {}",
                        other.type_name()
                    )),
                };
                let f = args[1].clone();
                let mut out = Vec::with_capacity(xs.len());
                for x in xs {
                    let keep = self.call_closure(f.clone(), vec![x.clone()])?;
                    match keep {
                        Value::Bool(true) => out.push(x),
                        Value::Bool(false) => {}
                        other => return panic(format!(
                            "arr_filter: predicate must return bool, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "{name}: expected array, got {}",
                        other.type_name()
                    )),
                };
                if xs.is_empty() {
                    return panic(format!("{name}: array is empty"));
                }
                let pick_max = name == "arr_max_i64";
                let mut best: i64 = match &xs[0] {
                    Value::Int(n) => *n,
                    other => return panic(format!(
                        "{name}: element must be i64, got {}",
                        other.type_name()
                    )),
                };
                for v in &xs[1..] {
                    let n = match v {
                        Value::Int(n) => *n,
                        other => return panic(format!(
                            "{name}: element must be i64, got {}",
                            other.type_name()
                        )),
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
                let (n, lo, hi) = (as_float(&args[0])?, as_float(&args[1])?, as_float(&args[2])?);
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
                ok!(Value::Float((next_rand_u64() >> 11) as f64 / 9_007_199_254_740_992.0));
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
                    return panic(format!("i64_to_str_radix: radix must be 2..=36, got {base}"));
                }
                ok!(Value::Str(i64_to_radix(n, base as u32)));
            }
            "uncertain_new_f64" => {
                want(2)?;
                ok!(make_uncertain(Value::Float(as_float(&args[0])?), as_float(&args[1])?));
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
                ok!(Value::Int(as_int(&args[0])?.wrapping_shl(as_int(&args[1])? as u32)));
            }
            "shr" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])?.wrapping_shr(as_int(&args[1])? as u32)));
            }

            // ── String ops ──────────────────────────────────────────────────────
            "str_len" => {
                want(1)?;
                ok!(Value::Int(as_str(&args[0])?.len() as i64));
            }
            "str_concat" | "axon_concat" => {
                want(2)?;
                ok!(Value::Str(format!("{}{}", as_str(&args[0])?, as_str(&args[1])?)));
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
                ok!(Value::Bool(as_str(&args[0])?.starts_with(as_str(&args[1])?)));
            }
            "str_ends_with" => {
                want(2)?;
                ok!(Value::Bool(as_str(&args[0])?.ends_with(as_str(&args[1])?)));
            }
            "str_to_upper" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.to_uppercase()));
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
                ok!(Value::Str(as_str(&args[0])?.replace(as_str(&args[1])?, as_str(&args[2])?)));
            }
            "str_index_of" => {
                want(2)?;
                let hay = as_str(&args[0])?;
                let needle = as_str(&args[1])?;
                ok!(Value::Int(hay.find(needle).map(|i| i as i64).unwrap_or(-1)));
            }
            "str_count" => {
                want(2)?;
                ok!(Value::Int(as_str(&args[0])?.matches(as_str(&args[1])?).count() as i64));
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
                ok!(Value::Int(s.as_bytes().get(i).map(|b| *b as i64).unwrap_or(-1)));
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
                ok!(make_uncertain(Value::Int(as_int(&args[0])?), as_float(&args[1])?));
            }
            "uncertain_dyn_f64" => {
                want(2)?;
                ok!(make_uncertain(Value::Float(as_float(&args[0])?), as_float(&args[1])?));
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
                        let confidence = fields.get("confidence").and_then(as_float_opt).unwrap_or(1.0);
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
                        ok!(make_temporal(value, new_conf, horizon, decay, created + offset));
                    }
                    _ => panic("temporal_at: expected a Temporal value"),
                }
            }
            // Read the present confidence of a Temporal value (PRD `rev.confidence`).
            "temporal_confidence" => {
                want(1)?;
                match &args[0] {
                    Value::Struct { fields, .. } => {
                        ok!(Value::Float(fields.get("confidence").and_then(as_float_opt).unwrap_or(1.0)));
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
                ok!(Value::Float(self.run_goal_random(&name, target, n_samples, lo, hi)?));
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
                    &name, target, n_starts, evals_per_start, lo, hi
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
                let n = self.provenance.borrow().get(&name).map(|v| v.len()).unwrap_or(0);
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
                ok!(Value::Dict(Rc::new(RefCell::new(std::collections::BTreeMap::new()))));
            }
            "dict_get" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_get: expected dict, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "dict_set: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let k = as_str(&args[1])?.to_string();
                d.borrow_mut().insert(k, args[2].clone());
                ok!(Value::Unit);
            }
            "dict_has" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_has: expected dict, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "dict_remove: expected dict, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "dict_len: expected dict, got {}",
                        other.type_name()
                    )),
                };
                ok!(Value::Int(d.borrow().len() as i64));
            }
            // `dict_keys(d) -> [str]` — sorted by BTreeMap ordering, so
            // iteration is deterministic.
            "dict_keys" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_keys: expected dict, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "dict_map_values: expected dict, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_enumerate: expected array, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "arr_partition: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut yes = Vec::new();
                let mut no = Vec::new();
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x.clone()])?;
                    match r {
                        Value::Bool(true) => yes.push(x),
                        Value::Bool(false) => no.push(x),
                        other => return panic(format!(
                            "arr_partition: predicate must return bool, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "dict_get_or: expected dict, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "dict_inc: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let k = as_str(&args[1])?.to_string();
                let mut m = d.borrow_mut();
                let cur = m.get(&k).cloned().unwrap_or(Value::Int(0));
                let n = match cur {
                    Value::Int(n) => n + 1,
                    other => return panic(format!(
                        "dict_inc: existing value at '{k}' is {}, not i64",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "dict_filter: expected dict, got {}",
                        other.type_name()
                    )),
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
                    let keep = self.call_closure(
                        pred.clone(),
                        vec![Value::Str(k.clone()), v.clone()],
                    )?;
                    match keep {
                        Value::Bool(true) => { out.insert(k, v); }
                        Value::Bool(false) => {}
                        other => return panic(format!(
                            "dict_filter: predicate must return bool, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "dict_to_pairs: expected dict, got {}",
                        other.type_name()
                    )),
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
                    other => return panic(format!(
                        "dict_from_pairs: expected array of (str, V) tuples, got {}",
                        other.type_name()
                    )),
                };
                let mut out: std::collections::BTreeMap<String, Value> =
                    std::collections::BTreeMap::new();
                for v in xs {
                    let pair = match v {
                        Value::Tuple(t) if t.len() == 2 => t,
                        other => return panic(format!(
                            "dict_from_pairs: each element must be a 2-tuple (str, V), got {}",
                            other.type_name()
                        )),
                    };
                    let k = match &pair[0] {
                        Value::Str(s) => s.clone(),
                        other => return panic(format!(
                            "dict_from_pairs: tuple's first element must be str, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "dict_to_str: expected dict, got {}",
                        other.type_name()
                    )),
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
                        Some((k, v)) => { out.insert(k.to_string(), Value::Str(v.to_string())); }
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
                    other => return panic(format!(
                        "dict_merge: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let d2 = match &args[1] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_merge: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let mut out: std::collections::BTreeMap<String, Value> =
                    d1.borrow().clone();
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
                    other => return panic(format!(
                        "{name}: expected array, got {}",
                        other.type_name()
                    )),
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
                let mut best_key = to_f(
                    self.call_closure(key_fn.clone(), vec![xs[0].clone()])?,
                )?;
                for (i, x) in xs.iter().enumerate().skip(1) {
                    let k = to_f(
                        self.call_closure(key_fn.clone(), vec![x.clone()])?,
                    )?;
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
                    other => return panic(format!(
                        "arr_take_while: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut out = Vec::new();
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x.clone()])?;
                    match r {
                        Value::Bool(true) => out.push(x),
                        Value::Bool(false) => break,
                        other => return panic(format!(
                            "arr_take_while: predicate must return bool, got {}",
                            other.type_name()
                        )),
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
                    other => return panic(format!(
                        "arr_drop_while: expected array, got {}",
                        other.type_name()
                    )),
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
                            other => return panic(format!(
                                "arr_drop_while: predicate must return bool, got {}",
                                other.type_name()
                            )),
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
                    other => return panic(format!(
                        "dict_each: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let f = args[1].clone();
                let pairs: Vec<(String, Value)> = d
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (k, v) in pairs {
                    let _ = self.call_closure(
                        f.clone(),
                        vec![Value::Str(k), v],
                    )?;
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
                    other => return panic(format!(
                        "arr_group_by: expected array, got {}",
                        other.type_name()
                    )),
                };
                let key_fn = args[1].clone();
                let mut out: std::collections::BTreeMap<String, Vec<Value>> =
                    std::collections::BTreeMap::new();
                for x in xs {
                    let k = self.call_closure(key_fn.clone(), vec![x.clone()])?;
                    let key = match k {
                        Value::Str(s) => s,
                        other => return panic(format!(
                            "arr_group_by: key fn must return str, got {}",
                            other.type_name()
                        )),
                    };
                    out.entry(key).or_default().push(x);
                }
                let map = out
                    .into_iter()
                    .map(|(k, v)| (k, Value::Array(v)))
                    .collect();
                ok!(Value::Dict(Rc::new(RefCell::new(map))));
            }
            // `dict_values(d) -> [V]` — values in key-sorted order.
            "dict_values" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_values: expected dict, got {}",
                        other.type_name()
                    )),
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
                        let who = if caller.is_empty() { "<main>".to_string() } else { caller.clone() };
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
                    let who = if caller.is_empty() { "<main>".to_string() } else { caller.clone() };
                    eprintln!(
                        "warning: [{}] AI call in `{who}` has no @[ai(policy)] — cost is unmetered and the call is harder to audit",
                        crate::error::W1310
                    );
                }
                if ai_mock_enabled() {
                    // Deterministic stub — but a fully-stamped provenance record
                    // (mode:"mock") with the REAL per-token cost charged to the
                    // meter, so the audit trail and the cost budget are honest
                    // about what a call costs even under mock. The tier/model are
                    // the RESOLVED routing (the routing is real; only the
                    // response is stubbed), so the cost is the routing's cost.
                    self.ai_cost_micro.set(self.ai_cost_micro.get() + cost_micro);
                    append_ai_call_jsonl(
                        &caller, &prompt, tier_name, model_id, model_ver, params,
                        "mock", "", cost_usd,
                    );
                    ok!(Value::Ok(Box::new(Value::Str(
                        "Mock summary: the single most important fact, stated concisely.".to_string()
                    ))));
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
                    ok!(match axon_ai::complete_with_model_usage(&prompt, &tier.api_model()) {
                        Ok((s, real_tokens)) => {
                            let real_micro = tier.cost_micro(real_tokens);
                            let real_usd = real_micro as f64 / 1_000_000.0;
                            self.ai_cost_micro.set(self.ai_cost_micro.get() + real_micro);
                            append_ai_call_jsonl(
                                &caller, &prompt, tier_name, model_id, model_ver,
                                params, "live", "", real_usd,
                            );
                            Value::Ok(Box::new(Value::Str(s)))
                        }
                        Err(e) => Value::Err(Box::new(Value::Str(e))),
                    });
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
                            &caller, &prompt, tier_name, "none", "offline", params,
                            "fallback", "offline: no model reachable", 0.0,
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
                    ok!(match axon_ai::complete_typed_uncertain_i64(as_str(&args[0])?) {
                        Ok((v, c)) => Value::Ok(Box::new(make_uncertain(Value::Int(v), c))),
                        Err(e) => Value::Err(Box::new(Value::Str(e))),
                    });
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
                    ok!(match axon_ai::complete_typed_uncertain_f64(as_str(&args[0])?) {
                        Ok((v, c)) => Value::Ok(Box::new(make_uncertain(Value::Float(v), c))),
                        Err(e) => Value::Err(Box::new(Value::Str(e))),
                    });
                }
                #[cfg(not(feature = "asi-runtime"))]
                return panic(
                    "ai_extract_uncertain_f64 requires --features asi-runtime (or set AXON_AI_MOCK=1)",
                );
            }

            _ => Ok(None),
        }
    }
}
