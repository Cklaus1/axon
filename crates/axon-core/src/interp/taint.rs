//! The runtime taint (C9 round 11, PSV-1, amendment 102).
//!
//! PSV-1 is "candidate bytes can never define, add or select the rubric". Five
//! review rounds in a row found a new member of one class: a value, name, key or
//! owner chosen by SEALED code reaches operator state with no seal edge, and the
//! static pin analysis (`interp/pin.rs`) was one instance short of the next.
//! This is the rule at the source: values sealed code PRODUCED carry a taint
//! that every operation propagates, and the primitives that SELECT operator
//! code, an operator impl or an integer width refuse a tainted selector.
//!
//! WHAT IS SELECTED (refused in an operator frame): a NAME a sealed value chose,
//! given to a name-resolving builtin; an operator CLOSURE a sealed value picked
//! out of a container, then called; the IMPL a method call dispatches to when
//! the receiver's TYPE was sealed code's; the WIDTH of fixed-width arithmetic on
//! an operand whose width was sealed code's. WHAT IS NOT: plain data compared
//! with an expected value, and control flow on data (`if cand_ok() { a() } else
//! { b() }`: both arms are the operator's own code). The property is "candidate
//! bytes cannot choose WHICH operator code runs", not "candidate output cannot
//! influence the verdict": a suite exists to check the candidate's answer.
//!
//! TWO BITS. [`VAL`]: the VALUE is the candidate's choice. [`TYP`]: the runtime
//! TYPE or width is. Everything a sealed frame computes carries both. A pin the
//! operator wrote clears TYP and only TYP (`let x: T = ..` after the cast machinery
//! verified a closed `T`, a typed parameter, a declared return of an operator fn, a
//! field, `x as T`, a comparison, an interpolation, a builtin with a closed scalar
//! return). An annotation never clears VAL: it pins a type, not a name or an index.
//!
//! HOW IT TRAVELS. Not in `Value` (17 variants; `Int(i64)` has no spare bit;
//! `Rc<Vec>`/`Rc<String>` are shared copy-on-write; ~100 construction sites in the
//! builtins). The EVALUATOR carries it: an accumulator (`acc`) holding the OR of
//! every taint touched while the expression being evaluated was computed, saved
//! and merged by ONE wrapper around `eval` ([`Interp::eval_tainted`]), so any
//! construct, a builtin included, propagates by default and no per-builtin code
//! can drop it; a taint per binding in `Env` (a closure's capture cell carries it
//! in companion keys); a taint per SHARED object (dict, channel) by Rc address; a
//! taint per module-level `let`. A value built under a tainted branch is tainted
//! too (`pc`, the program-counter taint), including by an early exit out of one.
//! Heap values are coarse (a read inherits the key's AND the container's taint).
//!
//! Gated on `seal.active`: an `axon run` takes one predictable branch per `eval`.

use super::*;
use std::sync::OnceLock;

/// The VALUE is a candidate's choice (a name, a key, an index, a closure pick).
pub(crate) const VAL: u8 = 1;
/// The runtime TYPE or WIDTH is a candidate's choice.
pub(crate) const TYP: u8 = 2;
/// Everything a sealed frame produces.
pub(crate) const ALL: u8 = VAL | TYP;
/// Capture-cell key prefix of a binding's taint companion. Starts with NUL, so
/// no source identifier can name or shadow it.
pub(crate) const CAP_T: &str = "\u{0}t\u{0}";

/// The evaluator's taint state (one per `Interp`).
#[derive(Default)]
pub(crate) struct Taint {
    /// The OR of the taints touched computing the expression being evaluated.
    pub(crate) acc: Cell<u8>,
    /// The taint of the expression `eval` last finished.
    pub(crate) last: Cell<u8>,
    /// The program-counter taint: the taint of the conditions the code now
    /// running is control-dependent on. Raised by a tainted `if`/`match`/loop for
    /// its branches. It taints what is STORED there, not every value computed
    /// there (`if c { run("a") } else { run("b") }` stays free).
    pub(crate) pc: Cell<u8>,
    /// Raised by an early exit out of a tainted branch, and kept to the end of
    /// the fn: what runs after it is control-dependent on the condition, whether
    /// or not the exit was taken. Two LOCATIONS can then hold different values
    /// by the candidate's bit: the fn's result (`if c { return "x" }; "ref"`) and
    /// whatever is stored after the exit (`x = "a"; if c { break }; x = "b"`). So
    /// the fn's result and every store carry it; a value that is merely computed
    /// there does not (a name literal given straight to a sink is the operator's
    /// statement-level choice to run it).
    pub(crate) sticky: Cell<u8>,
    /// The taint of the value the last `return` carried (a `return` unwinds past
    /// the statements around it, whose accumulation must not become the fn's
    /// result, so the fn's frame reads it from here).
    pub(crate) ret: Cell<u8>,
    /// Operator-kernel state a builtin reads back later (see [`Class::Kernel`]).
    kernel: Cell<u8>,
    /// State outside the interpreter that sealed code can write and the
    /// operator read back (see [`Class::World`]).
    world: Cell<u8>,
    /// Shared objects (dict, channel) by address, with a clone that keeps the
    /// address from being reused.
    objs: RefCell<HashMap<usize, (u8, Value)>>,
    /// Operator closures a candidate chose, by capture-cell address.
    picked: RefCell<HashMap<usize, Value>>,
    /// Module-level lets by name.
    lets: RefCell<HashMap<String, u8>>,
}

/// Restores the program-counter taint on drop.
pub(super) struct PcGuard<'a> {
    pub(super) cell: &'a Cell<u8>,
    pub(super) prev: u8,
}
impl Drop for PcGuard<'_> {
    fn drop(&mut self) {
        self.cell.set(self.prev);
    }
}

/// How a builtin relates to state it did not receive as an argument.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Class {
    /// The result is a function of the arguments (and the shared objects among
    /// them): the accumulator's default propagation is all it needs.
    Pure,
    /// Reads or writes the operator's kernel tables (principals, sandboxes, the
    /// scheduler, supervisors, stores, gateways, goals): one sticky taint is
    /// ORed in from every call's arguments and out into every call's result.
    Kernel,
    /// Reads or writes state outside the interpreter (files, env, exec, http,
    /// sql, the clock, the RNG, hardware): the same, one sticky taint, and a
    /// call made by SEALED code writes it too.
    World,
    /// In no table: treated as both, so a builtin nobody classified can only
    /// over-taint. `every_builtin_has_a_taint_class` makes this unreachable.
    Unclassified,
}

/// Operator-kernel builtins (prefix families).
pub(crate) const KERNEL_PREFIXES: &[&str] = &[
    "goal_",
    "agent_",
    "principal_",
    "sandbox_",
    "scheduler_",
    "supervisor_",
    "dstore_",
    "llm_",
    "kernel_goal_",
    "corrigible_",
];

/// Builtins that read or write state outside the interpreter.
pub(crate) const WORLD_BUILTINS: &[&str] = &[
    "read_line",
    "read_file",
    "write_file",
    "append_file",
    "file_size",
    "file_exists",
    "dir_create",
    "dir_list",
    "file_copy",
    "file_rename",
    "exec",
    "sql_query",
    "http_get",
    "http_post",
    "http_sse",
    "http_sse_post",
    "sleep_ms",
    "now_ms",
    "env_var",
    "host_await",
    "host_await_opt",
    "host_await_val",
    "host_await_val_opt",
    "random_i64",
    "random_f64",
    "srand",
    "temporal_now",
    "ai_complete",
    "ai_extract_uncertain_i64",
    "ai_extract_uncertain_f64",
    "ai_cost_spent",
    "gaussian_sample",
    "beta_sample",
    "categorical_sample",
    "volatile_load_u8",
    "volatile_load_u16",
    "volatile_load_u32",
    "volatile_load_u64",
    "volatile_store_u8",
    "volatile_store_u16",
    "volatile_store_u32",
    "volatile_store_u64",
    "hlt",
    "cli",
    "sti",
    "port_out_u8",
    "port_in_u8",
    "lidt",
    "zephyr_console_putc",
    "atomic_load_i64",
    "atomic_store_i64",
    "atomic_fetch_add_i64",
    "atomic_cas_i64",
    "fn_addr",
    "ptr_from_addr",
    "bpf_map_lookup_elem",
    "bpf_map_value_add",
    "bpf_ktime_get_ns",
    "bpf_get_smp_processor_id",
    "tee_seal",
    "tee_unseal",
    "tee_in_enclave",
    "tee_attest_measurement",
];

/// Every other builtin: a function of its arguments. Listed, not defaulted, so a
/// builtin added to `BUILTINS` is in no class until someone decides, and
/// `every_builtin_has_a_taint_class` fails until they do.
pub(crate) const PURE_BUILTINS: &[&str] = &[
    "print",
    "println",
    "eprint",
    "eprintln",
    "assert",
    "assert_eq",
    "assert_err",
    "len",
    "to_str_f64",
    "format",
    "axon_concat",
    "to_str",
    "parse_int",
    "parse_int_radix",
    "abs_i32",
    "abs_f64",
    "min_i32",
    "max_i32",
    "decimal_from_str",
    "decimal_to_str",
    "decimal_round",
    "decimal_div",
    "decimal_abs",
    "decimal_neg",
    "Chan::new",
    "sqrt",
    "pow",
    "exp",
    "ln",
    "log10",
    "floor",
    "ceil",
    "assert_eq_str",
    "assert_eq_f64",
    "re_is_match",
    "re_find",
    "re_find_all",
    "re_captures",
    "re_replace_all",
    "re_split",
    "base64_encode",
    "base64_decode",
    "hex_encode",
    "hex_decode",
    "str_eq",
    "str_cmp",
    "str_contains",
    "str_starts_with",
    "str_ends_with",
    "str_slice",
    "str_index_of",
    "parse_float",
    "char_at",
    "to_str_bool",
    "wrapping_add",
    "wrapping_sub",
    "wrapping_mul",
    "wrapping_div",
    "wrapping_rem",
    "abs_i64",
    "min_i64",
    "max_i64",
    "arr_range",
    "arr_push",
    "arr_sum_i64",
    "arr_max_i64",
    "arr_min_i64",
    "arr_map",
    "arr_filter",
    "arr_fold",
    "arr_sort_by",
    "arr_zip",
    "arr_contains",
    "arr_chunk",
    "arr_unique",
    "arr_index_of",
    "arr_find",
    "arr_any",
    "arr_all",
    "arr_count_if",
    "arr_sum_by",
    "arr_sum_by_f64",
    "arr_zip_with",
    "arr_reverse",
    "arr_repeat",
    "arr_concat",
    "arr_flatten",
    "as_f64",
    "as_i64",
    "as_u8",
    "as_u16",
    "as_u32",
    "as_u64",
    "as_i8",
    "as_i16",
    "as_i32",
    "arr_take",
    "arr_drop",
    "arr_sum_f64",
    "arr_max_f64",
    "arr_min_f64",
    "arr_mean_i64",
    "arr_mean_f64",
    "arr_std_f64",
    "arr_argmax_i64",
    "arr_argmin_i64",
    "arr_argmax_f64",
    "arr_argmin_f64",
    "str_split",
    "str_join",
    "str_to_upper",
    "str_to_lower",
    "str_trim",
    "str_trim_start",
    "str_trim_end",
    "str_replace",
    "str_repeat",
    "str_chars",
    "str_len_chars",
    "str_char_at",
    "str_char_slice",
    "char_code",
    "char_is_digit",
    "char_is_alpha",
    "char_is_space",
    "chr",
    "json_parse",
    "json_from_pairs",
    "dict_to_json",
    "json_arr_from_i64",
    "json_arr_from_f64",
    "json_arr_from_str",
    "json_len",
    "json_at",
    "json_keys",
    "json_get_json",
    "json_path_json",
    "json_path_i64",
    "json_path_f64",
    "json_arr_i64",
    "json_arr_f64",
    "json_arr_str",
    "json_stringify",
    "json_get_str",
    "json_get_i64",
    "json_path_str",
    "exit",
    "str_len",
    "str_pad_start",
    "str_pad_end",
    "min_f64",
    "max_f64",
    "clamp_i64",
    "clamp_f64",
    "parse_bool",
    "parse_int_or",
    "parse_float_or",
    "parse_bool_or",
    "str_digits_only",
    "i64_to_f64",
    "f64_to_i64",
    "sign_i64",
    "pow_i64",
    "sqrt_f64",
    "floor_f64",
    "ceil_f64",
    "round_f64",
    "str_count",
    "str_reverse",
    "i64_to_str_radix",
    "uncertain_confidence",
    "uncertain_new",
    "uncertain_deterministic",
    "uncertain_new_f64",
    "uncertain_dyn_i64",
    "uncertain_dyn_f64",
    "temporal_new",
    "temporal_at",
    "temporal_is_valid",
    "temporal_confidence",
    "bit_and",
    "bit_or",
    "bit_xor",
    "bit_not",
    "shl",
    "shr",
    "dict_new",
    "dict_get",
    "dict_set",
    "dict_has",
    "dict_remove",
    "dict_len",
    "dict_keys",
    "dict_values",
    "dict_map_values",
    "arr_group_by",
    "arr_max_by",
    "arr_min_by",
    "arr_take_while",
    "arr_drop_while",
    "dict_each",
    "arr_enumerate",
    "arr_partition",
    "dict_merge",
    "dict_filter",
    "dict_get_or",
    "dict_inc",
    "dict_to_pairs",
    "dict_from_pairs",
    "dict_to_str",
    "dict_from_str",
    "dict_try_from_str",
    "gaussian_pdf",
    "gaussian_cdf",
    "beta_mean",
    "beta_variance",
    "beta_cdf",
    "categorical_mean",
    "categorical_variance",
    "categorical_cdf",
    "E",
    "Var",
    "P",
];

/// Builtins that WRITE into a dict they are given (argument 0). Every other
/// builtin with a `Dict` parameter only reads it or builds a new one.
/// Builtins whose RESULT is the text of their argument: a channel prints its
/// length and a dict its contents, wherever they sit in the value, so the
/// result carries the taint of every shared object inside it.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) const STRINGIFIERS: &[&str] = &["to_str", "dict_to_str"];

/// Builtins that render a value to a stream and return nothing the program
/// can read back (named for the drift test that classifies every renderer).
#[cfg(test)]
pub(crate) const EMITTERS: &[&str] = &["print", "println", "eprint", "eprintln"];

pub(crate) const DICT_WRITERS: &[&str] = &["dict_set", "dict_remove", "dict_inc"];

/// What the dispatcher needs to know about a builtin.
#[derive(Clone, Copy)]
pub(crate) struct Info {
    pub(crate) class: Class,
    /// Its declared return is a closed scalar: the BUILTIN chose the result's
    /// type, so the result carries no TYP.
    pub(crate) untype: bool,
    pub(crate) writes_dict: bool,
    pub(crate) as_cast: bool,
}

fn closed_scalar_ret(ret: &str) -> bool {
    !pin::builtin_ret_open(ret)
        && matches!(
            ret,
            "i64"
                | "f64"
                | "bool"
                | "str"
                | "()"
                | "Decimal"
                | "i32"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "i8"
                | "i16"
        )
}

pub(crate) fn info(name: &str) -> Option<Info> {
    static MAP: OnceLock<HashMap<&'static str, Info>> = OnceLock::new();
    MAP.get_or_init(|| {
        crate::builtins::BUILTINS
            .iter()
            .map(|b| {
                let class = if KERNEL_PREFIXES.iter().any(|p| b.name.starts_with(p)) {
                    Class::Kernel
                } else if WORLD_BUILTINS.contains(&b.name) {
                    Class::World
                } else if PURE_BUILTINS.contains(&b.name) {
                    Class::Pure
                } else {
                    Class::Unclassified
                };
                (
                    b.name,
                    Info {
                        class,
                        untype: closed_scalar_ret(b.ret),
                        writes_dict: DICT_WRITERS.contains(&b.name),
                        as_cast: b.name.starts_with("as_"),
                    },
                )
            })
            .collect()
    })
    .get(name)
    .copied()
}

fn addr_of(v: &Value) -> Option<usize> {
    match v {
        Value::Dict(rc) => Some(Rc::as_ptr(rc) as *const u8 as usize),
        Value::Chan(rc) => Some(Rc::as_ptr(rc) as *const u8 as usize),
        _ => None,
    }
}

impl<'p> Interp<'p> {
    /// Whether the taint rules refuse. A unit test of ANOTHER seal layer runs
    /// with the selection rules off (as a paired-disable cell does), so each
    /// layer is judged by its own attack; the rules' own tests run with them on.
    pub(super) fn t_rules(&self) -> bool {
        // In a unit test the taint rules apply only when the test asks for them
        // (`TAINT_FORCE_ON`): every older test was written to judge one STATIC
        // layer by its own attack, and the taint refusing the same attack first
        // would hide a removed static guard (a REFUSED_ELSEWHERE row).
        #[cfg(test)]
        let on = TAINT_FORCE_ON.load(std::sync::atomic::Ordering::SeqCst);
        #[cfg(not(test))]
        let on = true;
        on
    }

    /// OR `t` into the expression being evaluated.
    #[inline(always)]
    pub(super) fn t_touch(&self, t: u8) {
        let a = &self.taint.acc;
        a.set(a.get() | t);
    }

    /// The taint a binding or container write stores: the value's, and the
    /// VALUE taint of the control it ran under. Control never gives a value a
    /// candidate-chosen TYPE: a branch picks among types the operator wrote, as
    /// `if c { x.ok() } else { y.ok() }` does, but a name or an index built out
    /// of the branch is the candidate's bit made into data.
    #[inline(always)]
    pub(super) fn t_stored(&self, value_taint: u8) -> u8 {
        value_taint | ((self.taint.pc.get() | self.taint.sticky.get()) & VAL)
    }

    /// The control contribution of a branch condition or match subject to the
    /// value of the whole expression: VALUE only. Which branch ran says which
    /// of the operator's own alternatives was taken, never a type.
    #[inline(always)]
    pub(super) fn t_control_val_only(&self) {
        self.taint.acc.set(self.taint.acc.get() & VAL);
    }

    /// [`Interp::t_last`] in the evaluator's own recursion: a constant 0 outside a
    /// sealed run, so the ordinary run's code carries none of it.
    #[inline(always)]
    pub(super) fn tl<const T: bool>(&self) -> u8 {
        if T {
            self.taint.last.get()
        } else {
            0
        }
    }

    /// The accumulator, likewise.
    #[inline(always)]
    pub(super) fn ta<const T: bool>(&self) -> u8 {
        if T {
            self.taint.acc.get()
        } else {
            0
        }
    }

    /// [`Interp::t_stored`], likewise.
    #[inline(always)]
    pub(super) fn ts<const T: bool>(&self, vt: u8) -> u8 {
        if T {
            self.t_stored(vt)
        } else {
            0
        }
    }

    /// The taint of the expression `eval` last finished.
    #[inline(always)]
    pub(super) fn t_last(&self) -> u8 {
        self.taint.last.get()
    }

    /// Raise the program-counter taint by `t` for the guard's life.
    pub(super) fn t_pc(&self, t: u8) -> PcGuard<'_> {
        PcGuard {
            cell: &self.taint.pc,
            prev: self.taint.pc.replace(self.taint.pc.get() | t),
        }
    }

    /// Run a branch (or a loop body) whose selection depended on a value of
    /// taint `ct`: whatever it STORES is control-dependent on that value. When
    /// the branch can leave the fn or the loop early (`exits`: a `return`,
    /// `break` or `continue` anywhere in it), whatever runs AFTER it is
    /// control-dependent on the value too, whether or not the exit was taken, so
    /// the program-counter taint stays raised until the fn's frame restores it
    /// (`if c { return "x" }; "reference"` leaks `c` into the name). Running
    /// code on a branch is not a selection (both arms are the operator's own
    /// code); what a branch BUILDS out of the candidate's bit is.
    #[inline(always)]
    pub(super) fn t_branch<T>(
        &self,
        ct: u8,
        exits: bool,
        f: impl FnOnce() -> Result<T, Flow>,
    ) -> Result<T, Flow> {
        if ct == 0 {
            return f();
        }
        let r = {
            let _g = self.t_pc(ct);
            f()
        };
        if exits {
            let st = &self.taint.sticky;
            st.set(st.get() | ct);
        }
        r
    }

    /// The result's type was fixed by the operator's own operator or literal.
    #[inline(always)]
    pub(super) fn t_untype(&self) {
        self.taint.acc.set(self.taint.acc.get() & !TYP);
    }

    /// The taint of a value's shared objects (dict, channel), looking through
    /// `Some`/`Ok`/`Err`/small tuples. Arrays are not scanned: an element is
    /// read out by an expression, and that read carries its holder's taint.
    pub(super) fn t_obj(&self, v: &Value) -> u8 {
        fn go(i: &Interp<'_>, v: &Value, d: u8) -> u8 {
            if let Some(a) = addr_of(v) {
                return i.taint.objs.borrow().get(&a).map_or(0, |(t, _)| *t);
            }
            if d == 0 {
                return 0;
            }
            match v {
                Value::Some(b) | Value::Ok(b) | Value::Err(b) => go(i, b, d - 1),
                Value::Tuple(xs) if xs.len() <= 8 => xs.iter().fold(0, |t, x| t | go(i, x, d - 1)),
                _ => 0,
            }
        }
        go(self, v, 3)
    }

    /// Any access to a channel through a method (or a `select` arm). The shared
    /// queue is one object with one taint: a READ (recv, try_recv, len) takes it
    /// into the result, and a MUTATING access from a sealed frame (a send, a
    /// drain: recv, try_recv, select) marks it, because what the queue holds or
    /// how many values it holds is then something sealed code decided
    /// (amendment 106). A sealed `len`/`clone` changes nothing and marks nothing,
    /// as a sealed `dict_len` does not.
    pub(super) fn t_chan_access(&self, chan: &Value, method: &str) {
        self.t_touch(self.t_obj(chan));
        if self.frame_sealed.get() && matches!(method, "send" | "recv" | "try_recv") {
            self.t_mark_obj(chan, ALL);
        }
    }

    /// The taint of every shared object (dict, channel) reachable from `v`,
    /// through arrays, tuples, records, enum payloads and dict values. For a
    /// value being turned into TEXT: a channel prints its length and a dict its
    /// contents, wherever they sit in the value (amendment 106). Past the depth
    /// bound the answer is "tainted", the sound direction.
    pub(super) fn t_obj_deep(&self, v: &Value) -> u8 {
        fn go(i: &Interp<'_>, v: &Value, d: u8, seen: &mut std::collections::HashSet<usize>) -> u8 {
            if d == 0 {
                return ALL;
            }
            let own = |a: usize| i.taint.objs.borrow().get(&a).map_or(0, |(t, _)| *t);
            match v {
                Value::Chan(_) => addr_of(v).map_or(0, own),
                Value::Dict(m) => {
                    let a = addr_of(v).unwrap_or(0);
                    if !seen.insert(a) {
                        return 0;
                    }
                    m.borrow()
                        .values()
                        .fold(own(a), |t, x| t | go(i, x, d - 1, seen))
                }
                Value::Some(b) | Value::Ok(b) | Value::Err(b) => go(i, b, d - 1, seen),
                Value::Array(xs) => xs.iter().fold(0, |t, x| t | go(i, x, d - 1, seen)),
                Value::Tuple(xs) => xs.iter().fold(0, |t, x| t | go(i, x, d - 1, seen)),
                Value::Struct { fields, .. } | Value::Enum { fields, .. } => {
                    fields.values().fold(0, |t, x| t | go(i, x, d - 1, seen))
                }
                _ => 0,
            }
        }
        go(self, v, 32, &mut std::collections::HashSet::new())
    }

    /// Mark the shared objects in `v` as carrying `t` (a write).
    pub(super) fn t_mark_obj(&self, v: &Value, t: u8) {
        if t == 0 {
            return;
        }
        fn go(i: &Interp<'_>, v: &Value, t: u8, d: u8) {
            if let Some(a) = addr_of(v) {
                let mut o = i.taint.objs.borrow_mut();
                let e = o.entry(a).or_insert_with(|| (0, v.clone()));
                e.0 |= t;
                return;
            }
            if d == 0 {
                return;
            }
            match v {
                Value::Some(b) | Value::Ok(b) | Value::Err(b) => go(i, b, t, d - 1),
                Value::Tuple(xs) if xs.len() <= 8 => xs.iter().for_each(|x| go(i, x, t, d - 1)),
                _ => {}
            }
        }
        go(self, v, t, 3);
    }

    /// A value left an evaluation (or was read out of a binding) carrying `t`:
    /// an OPERATOR closure in it that carries VAL is PICKED (sealed code chose
    /// it: out of a table by a key or index, by a branch, by being handed back),
    /// which makes calling it a selection. One call-time check
    /// ([`Interp::t_check_call_picked`]) then covers every way of reading it.
    pub(super) fn t_note(&self, v: &Value, t: u8) {
        if t & VAL == 0 {
            return;
        }
        fn go(i: &Interp<'_>, v: &Value, d: u8) {
            match v {
                Value::Closure { captured, .. } => {
                    let c = captured.borrow();
                    if c.contains_key(SEALED_CLOSURE_MARK) || c.contains_key(SEALED_FNVAL_MARK) {
                        return;
                    }
                    drop(c);
                    let a = Rc::as_ptr(captured) as *const u8 as usize;
                    i.taint
                        .picked
                        .borrow_mut()
                        .entry(a)
                        .or_insert_with(|| v.clone());
                }
                Value::Some(b) | Value::Ok(b) | Value::Err(b) if d > 0 => go(i, b, d - 1),
                Value::Tuple(xs) if d > 0 && xs.len() <= 8 => {
                    xs.iter().for_each(|x| go(i, x, d - 1))
                }
                _ => {}
            }
        }
        go(self, v, 3);
    }

    /// Whether `captured` is an operator closure sealed code picked.
    pub(super) fn t_is_picked(&self, captured: &Rc<RefCell<HashMap<String, Value>>>) -> bool {
        let a = Rc::as_ptr(captured) as *const u8 as usize;
        self.taint.picked.borrow().contains_key(&a)
    }

    /// The taint of what the name `n` holds: its binding's, or (no binding) the
    /// module-level let's.
    fn t_holder(&self, n: &str, env: &Env) -> u8 {
        if env.get(n).is_some() {
            env.taint_of(n)
        } else {
            self.t_global(n)
        }
    }

    /// The taint of a module-level let (set when it was initialised).
    pub(super) fn t_global(&self, name: &str) -> u8 {
        self.taint.lets.borrow().get(name).copied().unwrap_or(0)
    }

    pub(super) fn t_set_global(&self, name: &str, t: u8) {
        if t != 0 {
            self.taint.lets.borrow_mut().insert(name.to_string(), t);
        }
    }

    /// The evaluator's wrapper in a sealed run. Everything a SEALED frame
    /// computes is the candidate's; in an OPERATOR frame the accumulator is
    /// saved, cleared, and merged back with what this expression touched.
    #[cold]
    #[inline(never)]
    pub(super) fn eval_tainted(&self, expr: &Expr, env: &mut Env) -> R {
        let tn = &self.taint;
        if self.frame_sealed.get() {
            let r = self.eval_arm::<true>(expr, env);
            tn.acc.set(ALL);
            tn.last.set(ALL);
            return r;
        }
        let saved = tn.acc.replace(0);
        let r = self.eval_arm::<true>(expr, env);
        // A read of a binding (or module-level let) takes its taint: the name
        // itself, an element or a field of it. Done here, around the arms, so the
        // fast paths that read the holder in place are covered by the one rule.
        match expr {
            Expr::Ident(n) => self.t_touch(self.t_holder(n, env)),
            Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } => {
                if let Expr::Ident(n) = receiver.as_ref() {
                    self.t_touch(self.t_holder(n, env));
                }
            }
            _ => {}
        }
        let mine = tn.acc.get();
        tn.last.set(mine);
        if let Ok(v) = &r {
            if mine != 0 {
                self.t_note(v, mine);
            }
        }
        tn.acc.set(saved | mine);
        r
    }

    /// Before a builtin runs: the taint of the state it reads goes into the
    /// result. Called with the arguments evaluated and their taint in `acc`.
    pub(super) fn t_builtin_in(&self, name: &str, args: &[Value]) {
        // Every argument is walked DEEP: a builtin that compares, searches,
        // sorts, hashes or prints a container reads the content of every shared
        // object inside it, and no table of such builtins is kept (amendment 108).
        for a in args {
            self.t_touch(self.t_obj_deep(a));
        }
        match info(name).map(|i| i.class) {
            Some(Class::Pure) => {}
            Some(Class::Kernel) => self.t_touch(self.taint.kernel.get()),
            Some(Class::World) => self.t_touch(self.taint.world.get()),
            // In no table: assume it reads both.
            Some(Class::Unclassified) => {
                self.t_touch(self.taint.kernel.get() | self.taint.world.get())
            }
            // Not a builtin of the table (a name `call_builtin` answers that
            // `BUILTINS` does not list): a function of its arguments.
            None => {}
        }
    }

    /// A `native::M::fn(..)` call (gfx surface, modbus/fhir/fix session ...):
    /// the call reads and writes a registry shared by every frame, which no
    /// value taint can follow through an integer-like handle. It is `World`
    /// state like a file: a sealed call marks it ALL, an operator call reads it
    /// back into the result (amendment 108).
    pub(super) fn t_native_call(&self) {
        let w = &self.taint.world;
        if self.frame_sealed.get() {
            w.set(w.get() | ALL);
        } else {
            self.t_touch(w.get());
            w.set(w.get() | self.t_stored(self.taint.acc.get()));
        }
    }

    /// After a builtin ran: what it wrote is recorded, and a result whose type
    /// the builtin chose carries no TYP.
    pub(super) fn t_builtin_out(&self, name: &str, args: &[Value], res: &Value) {
        let sealed = self.frame_sealed.get();
        // What a write stores is the value's taint AND the control it ran under:
        // a spawn in a loop the candidate sized, or a write behind a branch on
        // its answer, is a state the candidate decided (amendment 106).
        let a = if sealed {
            ALL
        } else {
            self.t_stored(self.taint.acc.get())
        };
        let i = info(name);
        match i.map(|i| i.class) {
            Some(Class::Pure) => {}
            Some(Class::Kernel) => {
                if !sealed {
                    self.taint.kernel.set(self.taint.kernel.get() | a);
                }
            }
            Some(Class::World) => self.taint.world.set(self.taint.world.get() | a),
            Some(Class::Unclassified) => {
                self.taint.world.set(self.taint.world.get() | a);
                if !sealed {
                    self.taint.kernel.set(self.taint.kernel.get() | a);
                }
            }
            None => {}
        }
        if i.is_some_and(|i| i.writes_dict) {
            if let Some(d) = args.first() {
                self.t_mark_obj(d, self.t_stored(a));
            }
        }
        if let Some(i) = i {
            let sized = matches!(res, Value::SizedInt { .. });
            if i.untype && (!sized || i.as_cast) {
                self.taint.acc.set(self.taint.acc.get() & !TYP);
            }
        }
    }

    /// The NAME rule at run time: operator code gives a name-resolving builtin
    /// only a name sealed code had no hand in. `ats[i]` is the taint of
    /// argument `i`.
    #[inline(always)]
    pub(crate) fn t_check_names(&self, builtin: &str, ats: &[u8]) -> Result<(), Flow> {
        if !self.seal.active {
            return Ok(());
        }
        self.t_check_names_sealed(builtin, ats)
    }

    #[inline(never)]
    fn t_check_names_sealed(&self, builtin: &str, ats: &[u8]) -> Result<(), Flow> {
        if self.frame_sealed.get() || !self.t_rules() {
            return Ok(());
        }
        let Some(idxs) = pin::name_sink(builtin) else {
            return Ok(());
        };
        for &i in idxs {
            if ats.get(i).copied().unwrap_or(0) & VAL != 0 {
                return panic(format!(
                    "operator code gave `{builtin}` a function name nothing on the operator side \
                     determined (argument {}): the name reached this call from sealed code and \
                     nothing the operator wrote chose it (runtime taint) — the candidate would \
                     choose which function runs; name it with a literal or a constant the \
                     operator wrote",
                    i + 1
                ));
            }
        }
        Ok(())
    }

    /// The CLOSURE rule: operator code does not call an operator closure that
    /// sealed code picked (out of a table by a key or index it chose, or by a
    /// branch on a value it chose).
    #[inline(always)]
    pub(crate) fn t_check_call_picked(
        &self,
        captured: &Rc<RefCell<HashMap<String, Value>>>,
    ) -> Result<(), Flow> {
        if !self.seal.active {
            return Ok(());
        }
        self.t_check_call_picked_sealed(captured)
    }

    #[inline(never)]
    fn t_check_call_picked_sealed(
        &self,
        captured: &Rc<RefCell<HashMap<String, Value>>>,
    ) -> Result<(), Flow> {
        if self.frame_sealed.get() || !self.t_rules() {
            return Ok(());
        }
        if self.t_is_picked(captured) {
            return panic(
                "operator code called an operator closure that the candidate picked (out of a \
                 table by a key or index it chose, or by a branch on a value it chose) — the \
                 candidate would choose which operator code runs"
                    .to_string(),
            );
        }
        Ok(())
    }

    /// The DISPATCH rule at run time: the impl a method call selects is chosen
    /// by the receiver's runtime TYPE; a receiver whose type is sealed code's
    /// has not been pinned by the operator.
    #[inline(always)]
    pub(crate) fn t_check_dispatch(
        &self,
        f: &FnDef,
        recv: &Value,
        recv_taint: u8,
        tn: &str,
    ) -> Result<(), Flow> {
        if recv_taint & TYP == 0 {
            return Ok(());
        }
        self.t_check_dispatch_tainted(f, recv, recv_taint, tn)
    }

    #[inline(never)]
    fn t_check_dispatch_tainted(
        &self,
        f: &FnDef,
        recv: &Value,
        recv_taint: u8,
        tn: &str,
    ) -> Result<(), Flow> {
        if !self.seal.active
            || self.frame_sealed.get()
            || self.fn_is_sealed(f)
            || !self.t_rules()
            || recv_taint & TYP == 0
            || !self.pins.selects_between_impls(&f.name)
        {
            return Ok(());
        }
        let operator_value = match recv {
            Value::Struct { name, .. } => self.pins.is_operator_type(name),
            Value::Enum { enum_name, .. } => self.pins.is_operator_type(enum_name),
            _ => false,
        };
        if operator_value {
            return Ok(());
        }
        panic(format!(
            "operator code dispatched `{}` on a value whose type nothing on the operator side \
             determined (here `{tn}`): its type reached this call from sealed code and no pin \
             the operator wrote cleared it (runtime taint) — the candidate would choose the \
             impl; pin it with `let x: T = ...`",
            f.name
        ))
    }

    /// The WIDTH rule at run time: operator code does not do arithmetic on a
    /// fixed-width integer whose width is sealed code's.
    pub(crate) fn t_check_width(&self, v: &Value, vt: u8, what: &str) -> Result<(), Flow> {
        if !self.seal.active
            || self.frame_sealed.get()
            || !self.t_rules()
            || vt & TYP == 0
            || !matches!(v, Value::SizedInt { .. })
        {
            return Ok(());
        }
        panic(format!(
            "operator code did arithmetic on a fixed-width integer whose width nothing on the \
             operator side determined ({what} on {}): its width reached this operation from \
             sealed code and no pin the operator wrote cleared it (runtime taint) — the \
             candidate would choose the wrapping; pin it with `let x: T = ...`",
            v.type_name()
        ))
    }

    /// Whether the annotation `ty` pins the runtime type: the cast succeeded
    /// and `ty` is a closed type the operator chose.
    pub(super) fn t_pins(&self, ty: &crate::ast::AxonType) -> bool {
        self.pins.closed_ty(ty)
    }
}

/// Whether `e` can leave the fn or the loop it is in: a `return`, `break` or
/// `continue` anywhere in it (a lambda's too: over-approximate). A `?` is not
/// one: its early path carries the error, never a second operator-written
/// alternative for the same location.
pub(super) fn has_exit(e: &Expr) -> bool {
    let mut hit = false;
    crate::ast::walk_expr(e, &mut |x| {
        hit |= matches!(x, Expr::Return(_) | Expr::Break | Expr::Continue)
    });
    hit
}

/// [`has_exit`] over a statement list.
pub(super) fn stmts_have_exit(ss: &[crate::ast::Stmt]) -> bool {
    ss.iter().any(|s| has_exit(&s.expr))
}
