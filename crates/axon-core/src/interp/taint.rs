//! The runtime taint (C9 round 11, PSV-1, amendment 102).
//!
//! PSV-1 started as "candidate bytes can never define, add or select the rubric". Five
//! review rounds in a row found a new member of one class: a value, name, key or owner
//! chosen by SEALED code reaches operator state with no seal edge, and the static pin
//! analysis (`interp/pin.rs`) was one instance short of the next. This is the rule at the
//! source: values sealed code PRODUCED carry a taint that every operation propagates, and
//! the primitives that SELECT operator code, an operator impl or an integer width refuse a
//! tainted selector.
//!
//! WHAT IS SELECTED (refused in an operator frame): a NAME a sealed value chose, given to a
//! name-resolving builtin; an operator CLOSURE a sealed value picked out of a container,
//! then called; the IMPL a method call dispatches to when the receiver's TYPE was sealed
//! code's, or when the operator-typed receiver was itself PICKED by sealed code (the
//! `PICK` mark, amendment 121; R51 S4); the WIDTH of fixed-width arithmetic on an operand whose width was sealed code's
//! (a width inside an `Uncertain`/`Temporal` included). WHAT IS NOT: plain data compared
//! with an expected value, and control flow on data (`if cand_ok() { a() } else { b() }`:
//! both arms are the operator's own code). The property is "candidate bytes cannot choose
//! WHICH operator code runs through the constructs of `CONTROL_TABLE` and
//! `CALLBACK_BUILTINS` and the value routes of the taint classes", not "candidate output
//! cannot influence the verdict": a suite exists to check the candidate's answer.
//!
//! IT IS NOT A PROOF that candidate bytes cannot influence the rubric in all ways
//! (amendment 117, after a six-pass find-until-dry loop that did not go dry): omission, a
//! table of precomputed verdicts, a branch into a weaker check, integer handles, paths,
//! URLs, prompts, native codegen, and the existence-oracle text on untested paths are
//! standing non-claims. (An operator-typed value the candidate chose was one until
//! amendment 121, which closes the shapes it names with `PICK` (R51 S4); a pick through an edge
//! it does not name is still not claimed.) An omission has a consequence in a skipped
//! ASSERTION as much as in a skipped store. The
//! list and its drift check live in `governance/specs/v022-protected-suite-verdict.md`
//! (PSV-1) and `governance/notes/v022-psv1-loop-triage.md`.
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
//! can drop it; a taint per binding in `Env` (the parallel taint vector of
//! `interp/env.rs`'s `Bindings`, carried by its `Snapshot`); a taint per SHARED object (dict, channel) by Rc address; a
//! taint per module-level `let`. A value built under a tainted branch is tainted
//! too (`pc`, the program-counter taint), including by an early exit out of one.
//! Heap values are coarse (a read inherits the key's AND the container's taint).
//!
//! Gated on `seal.active`: an `axon run` runs the evaluator's unsealed
//! instantiation, which carries none of it.
//!
//! R51 (spec §4 Fork 2, §11 S2): the port of v0.22's taint channel onto main's model.
//! A binding's taint lives in `Bindings`' parallel vector (present exactly in a
//! sealed run), module-level lets are keyed by `Sym`, and the evaluator is
//! monomorphised (`eval_t::<T>`), so the unsealed instantiation carries no taint
//! code. What reads a taint to REFUSE (the name rule, the PICK mark, the dispatch
//! and width rules) and the seal edges that make a frame sealed are S3/S4; until
//! S6 every sealed run is refused at build (`SEAL_RUNTIME_COMPLETE`).

// S2.3 step 1 of 4: the tables and hooks land before their readers (the
// `eval_t::<true>` hook sites, step 2), so nothing calls them yet.
#![allow(dead_code)]

use super::*;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The VALUE is a candidate's choice (a name, a key, an index, a closure pick).
pub(crate) const VAL: u8 = 1;
/// The runtime TYPE or WIDTH is a candidate's choice.
pub(crate) const TYP: u8 = 2;
/// Everything a sealed frame produces.
pub(crate) const ALL: u8 = VAL | TYP;
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
    /// How many `with handler` bodies that can be aborted are being evaluated
    /// (see `eval_with_handler`): inside one, every branch on tainted data is a
    /// possible exit.
    pub(crate) abortable: Cell<u32>,
    /// How many scheduler fibers are running: a panic in one is caught and
    /// recorded, so a call into sealed code from operator code in a fiber is a
    /// possible exit (sticky once it returns).
    pub(crate) catchable: Cell<u32>,
    /// The taint of the value the last `return` carried (a `return` unwinds past
    /// the statements around it, whose accumulation must not become the fn's
    /// result, so the fn's frame reads it from here).
    pub(crate) ret: Cell<u8>,
    /// Operator-kernel state a builtin reads back later (see [`Class::Kernel`]).
    kernel: Cell<u8>,
    /// How many times a SEALED frame has been entered (a scheduler pass
    /// compares it before and after a fiber to know whether sealed code ran).
    pub(crate) entries: Cell<u64>,
    /// State outside the interpreter that sealed code can write and the
    /// operator read back (see [`Class::World`]).
    world: Cell<u8>,
    /// Shared objects (dict, channel) by address, with a clone that keeps the
    /// address from being reused.
    objs: RefCell<HashMap<usize, (u8, Value)>>,
    /// Module-level lets by interned name.
    lets: RefCell<HashMap<Sym, u8>>,
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
    "now_ns",
    "env_var",
    "host_await",
    "host_await_opt",
    "host_await_val",
    "host_await_val_opt",
    "random_i64",
    "random_f64",
    "srand",
    // The durable store is a log file in the user cache dir that every kernel
    // opens: state outside the interpreter (amendment 117, PSV-1 loop finding 12).
    "dstore_open",
    "dstore_apply",
    "dstore_value",
    "dstore_version",
    "dstore_clear",
    "temporal_now",
    "temporal_new",
    "temporal_is_valid",
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

/// Kernel builtins that draw from the RNG stream a World-class `srand` seeds and
/// `random_*` reads. The kernel and world cells are separate, so a search whose
/// stream the operator seeded from a candidate's number (`srand(cand())`) was
/// joined to that seed by nothing (amendment 117, loop finding 47): these READ
/// the world cell too. (The other direction, a `random_*` read after a search
/// that drew a candidate-decided number of times, is marked by the search's own
/// provenance append, which every search that runs a metric makes and which is
/// World state: tested, but no separate edge is needed.)
/// `taint_tests` fails if a builtin arm that draws is not listed.
pub(crate) const RNG_COUPLED: &[&str] = &[
    "goal_run",
    "goal_run_constrained",
    "goal_run_categorical",
    "goal_run_random",
    "goal_run_multistart",
    "goal_continue",
    "kernel_goal_run",
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
    "temporal_at",
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

/// Builtins whose FIRST argument is only counted, keyed into or appended to: they
/// never look at what the entries hold, so a big dict or array they touch in a
/// loop is not walked end to end on every call (a million `dict_set`s would
/// otherwise be quadratic). The entries are read by their own routes: a value
/// taken out carries its holder's taint, and a shared object inside it its own.
/// Fail-closed: a builtin NOT listed here walks every argument deep.
pub(crate) const SHALLOW_FIRST_ARG: &[&str] = &[
    "dict_set",
    "dict_remove",
    "dict_inc",
    "dict_get",
    "dict_get_or",
    "dict_has",
    "dict_len",
    "dict_keys",
    "len",
    "arr_push",
];

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

    /// [`Interp::t_last`] in the evaluator's own recursion: a constant 0 in the
    /// unsealed instantiation, so the ordinary run's code carries none of it.
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
    pub(super) fn t_branch<X>(
        &self,
        ct: u8,
        exits: bool,
        f: impl FnOnce() -> Result<X, Flow>,
    ) -> Result<X, Flow> {
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
        // Inside an abort-capable `with` body any branch on tainted data is a
        // possible exit (amendment 117, loop findings 31, 32, 34).
        if self.taint.abortable.get() != 0 {
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

    /// The taint of every shared object (dict, channel) reachable from `v`,
    /// through arrays, tuples, records, enum payloads and dict values. For a
    /// value being turned into TEXT or compared: a channel prints its length
    /// and a dict its contents, wherever they sit in the value (amendment
    /// 106). Past the depth bound the answer is "tainted", the sound direction.
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
                Value::Array(xs) | Value::Tuple(xs) => {
                    xs.iter().fold(0, |t, x| t | go(i, x, d - 1, seen))
                }
                Value::Struct(s) => s
                    .fields
                    .iter()
                    .fold(0, |t, (_, x)| t | go(i, x, d - 1, seen)),
                Value::Enum(e) => e
                    .fields
                    .iter()
                    .fold(0, |t, (_, x)| t | go(i, x, d - 1, seen)),
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

    /// The taint of what variable `s` (resolved to `slot`) holds: its
    /// binding's, read through the one index lookup the value is read
    /// through ([`Bindings::get_var_idx`]), or (no binding) the module-level
    /// let's.
    fn t_holder(&self, s: Sym, slot: u32, env: &Env) -> u8 {
        match env.get_var_idx(s, slot) {
            Some(i) => env.taint(i),
            None => self.t_global(s),
        }
    }

    /// The taint of a module-level let (set when it was initialised).
    pub(super) fn t_global(&self, s: Sym) -> u8 {
        self.taint.lets.borrow().get(&s).copied().unwrap_or(0)
    }

    pub(super) fn t_set_global(&self, s: Sym, t: u8) {
        if t != 0 {
            self.taint.lets.borrow_mut().insert(s, t);
        }
    }

    /// Before a builtin runs: the taint of the state it reads goes into the
    /// result. Called with the arguments evaluated and their taint in `acc`.
    pub(super) fn t_builtin_in(&self, name: &str, args: &[Value]) {
        // Every argument is walked DEEP: a builtin that compares, searches,
        // sorts, hashes or prints a container reads the content of every shared
        // object inside it, so there is no table of such builtins. The one table
        // is the opposite, fail-closed: the few that only count, key into or append
        // to their first argument (amendment 108).
        for (i, a) in args.iter().enumerate() {
            if i == 0 && SHALLOW_FIRST_ARG.contains(&name) {
                self.t_touch(self.t_obj(a));
            } else {
                self.t_touch(self.t_obj_deep(a));
            }
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
        // A search that draws the RNG stream reads the world cell `srand` wrote
        // (amendment 117, loop finding 47).
        if RNG_COUPLED.contains(&name) {
            self.t_touch(self.taint.world.get());
        }
    }

    /// The provenance a zoned (`@[adaptive]` / `@[experiment]`) call records
    /// is a write the runtime performs for the CALLER, not a builtin, so it
    /// reached no `t_builtin_out` (amendment 117, PSV-1 loop findings 19, 24,
    /// 43). The in-memory best store is kernel state (`agent_trace_len`,
    /// `goal_count`, `goal_history` read it back); the JSONL append is a file
    /// both kernels share, so it is World state like `write_file`. A sealed
    /// call writes the sealed kernel's store (not the operator's) but the file
    /// is shared: ALL. An operator call stores the taint of what it recorded
    /// and the control it ran under, as any other operator write does.
    pub(super) fn t_provenance_write(&self, arg_t: u8, in_memory: bool, file: bool) {
        if !self.seal.active {
            return;
        }
        let (kernel, world) = (&self.taint.kernel, &self.taint.world);
        if self.frame_sealed.get() {
            if file {
                world.set(world.get() | ALL);
            }
            return;
        }
        let a = self.t_stored(arg_t);
        if in_memory {
            kernel.set(kernel.get() | a);
        }
        if file {
            world.set(world.get() | a);
        }
    }

    /// Inside a builtin that runs operator code a number of times the earlier
    /// results decide (a callback loop, a goal search, a scheduler pass): raise
    /// the control taint of everything the runs returned so far for the rest of
    /// the builtin, so the next run and what it stores are control-dependent on
    /// them (amendment 117). The builtin dispatch restores the pc on return.
    /// Operator frames only: a sealed frame's control is already everything.
    #[inline(always)]
    pub(super) fn t_loop_pc(&self) {
        if self.seal.active && !self.frame_sealed.get() {
            let t = self.taint.acc.get() & VAL;
            if t != 0 {
                self.taint.pc.set(self.taint.pc.get() | t);
            }
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
}

/// Whether `e` can leave the fn or the loop it is in: a `return`, `break` or
/// `continue` anywhere in it (a lambda's too: over-approximate). A `?` is not
/// one: its early path carries the error, never a second operator-written
/// alternative for the same location.
pub(super) fn has_exit(e: &Expr) -> bool {
    let mut hit = false;
    crate::ast::walk_expr(e, &mut |x| {
        hit |= matches!(
            x,
            Expr::Return(_) | Expr::Break | Expr::Continue | Expr::Question(_)
        )
    });
    hit
}

/// [`has_exit`] over a statement list.
pub(super) fn stmts_have_exit(ss: &[crate::ast::Stmt]) -> bool {
    ss.iter().any(|s| has_exit(&s.expr))
}
