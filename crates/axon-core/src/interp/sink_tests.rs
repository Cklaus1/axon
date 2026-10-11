//! R51 S3: the seal SINKS inside evaluation (v0.22 interp.rs:3697-3927,
//! taint.rs:1351-1497), wired into main's tree evaluator: the global-read
//! edge (`seal_global` behind `global_ref`, the only read of `globals`), the
//! name rule at a name-resolving builtin (`seal_name_args`, `t_check_names`),
//! the width rule (`seal_width`, `seal_width_unary`, `t_check_width`), the
//! refinement edge (`seal_refine`), the method-dispatch edges
//! (`seal_method`, `seal_dispatch`), the uniform no-such-fn text, the
//! channel stamps (`chan_created`, `chan_send_check`) and the declared-type
//! casts at `let x: T` and at record construction.
//!
//! Every sealed run is refused at build until S6 (`SEAL_RUNTIME_COMPLETE`), so
//! these drive an `Interp` made by [`Interp::build_with`] (seal state and pin
//! analysis computed, refuse-all pin skipped, as `taint_tests` and
//! `kernel_tests` do). Each probe is a suite fn whose body runs through
//! `eval_t::<true>` (the sealed instantiation) in an operator frame or, via
//! [`Interp::with_frame`], in a sealed one, with `pin_fn` set to it as the
//! call path sets it. The drift tests at the end pin the one global lookup
//! and the classification of main's in-place sinks (spec §4 Fork 3).

use super::taint::{ALL, VAL};
use super::*;

const SUITE: &str = "\
let TABLE = [1, 2, 3]
let CFG = Cfg { k: 7 }
let G = |x: i64| x + 1
type Cfg = { k: i64 }
type Nat = i64 where _ >= 0
trait Judge {
    fn ok(self) -> bool
}
impl Judge for i64 {
    fn ok(self: i64) -> bool { self == 9 }
}
impl Judge for bool {
    fn ok(self: bool) -> bool { self }
}
fn w(n: i64) -> i64 { n }
fn read_index() -> i64 { TABLE[0] }
fn read_whole() -> [i64] { TABLE }
fn read_field() -> i64 { CFG.k }
fn call_const() -> i64 { G(1) }
fn read_mine() -> i64 { MINE }
fn take_w() -> i64 {
    let f = w
    f(1)
}
fn spawn_lit() -> i64 { scheduler_spawn(\"w\", 1) }
fn spawn_param(name: str) -> i64 { scheduler_spawn(name, 1) }
fn spawn_branch(c: i64) -> i64 {
    let n = if c > 0 { \"w\" } else { \"w\" }
    scheduler_spawn(n, 1)
}
fn spawn_missing() -> i64 { scheduler_spawn(\"nope\", 1) }
fn shift(d: Dict) -> i64 {
    match dict_get(d, \"k\") {
        Some(v) => v << 1
        None => 0
    }
}
fn shift_pinned(d: Dict) -> i64 {
    match dict_get(d, \"k\") {
        Some(v) => {
            let x: u8 = v
            x << 1
        }
        None => 0
    }
}
fn compare(d: Dict) -> bool {
    match dict_get(d, \"k\") {
        Some(v) => v == 4
        None => false
    }
}
fn neg(d: Dict) -> i64 {
    match dict_get(d, \"k\") {
        Some(v) => -v
        None => 0
    }
}
fn shift_t(c: u8) -> u8 { c << 1 }
fn neg_t(c: u8) -> u8 { -c }
fn theirs(c: i64) -> i64 {
    let x: Pos = c
    x
}
fn mine(c: i64) -> i64 {
    let x: Nat = c
    x
}
fn judge(d: Dict) -> bool {
    match dict_get(d, \"k\") {
        Some(v) => v.ok()
        None => false
    }
}
fn judge_pinned(d: Dict) -> bool {
    match dict_get(d, \"k\") {
        Some(v) => {
            let y: i64 = v
            y.ok()
        }
        None => false
    }
}
fn judge_fake(x: Fake) -> bool { x.ok() }
fn no_method(x: i64) -> bool { x.nothing() }
fn typed_let(c: bool) -> i64 {
    let x: i64 = c
    0
}
fn build_cfg(c: bool) -> Cfg { Cfg { k: c } }
fn mk_chan() -> Chan<i64> { chan<i64>() }
fn send_on(q: Chan<i64>, x: bool) -> i64 {
    q.send(x)
    1
}
";

/// The sealed candidate: its own global, refinement, type and method.
const CAND: &str = "\
let MINE = 5
type Pos = i64 where _ > 0
type Fake = { v: i64 }
trait Mine {
    fn ok(self) -> bool
}
impl Mine for Fake {
    fn ok(self: Fake) -> bool { true }
}
fn fake() -> Fake { Fake { v: 1 } }
fn solve() -> i64 { 41 }
";

/// Whether `r` is the value `want` (`Value` has no `PartialEq`).
pub(super) fn yields(r: &Result<Value, Flow>, want: &Value) -> bool {
    r.as_ref()
        .is_ok_and(|v| super::value::values_equal(v, want))
}

/// The suite and a sealed candidate, as one program, and the sealed directory.
fn prog_of(tag: &str, suite: &str, cand: &str) -> (Program, std::path::PathBuf) {
    use crate::span::intern_source;
    let sdir = format!("/{tag}-sealed");
    let s = crate::parse_source_in(suite, intern_source(&format!("/{tag}-suite/h.ax"), suite))
        .expect("suite parses");
    let c = crate::parse_source_in(cand, intern_source(&format!("{sdir}/f.ax"), cand))
        .expect("candidate parses");
    let prog = Program {
        items: s.items.into_iter().chain(c.items).collect(),
    };
    (prog, std::path::PathBuf::from(sdir))
}

/// An `Interp` over `suite` and the sealed `cand`, built but with its
/// module-level `let`s not yet initialised; `sealed` gives it a live seal (a
/// unit-test `Interp`, not a runnable sealed run - those stay refused until S6).
fn with_prog<R>(
    tag: &str,
    suite: &str,
    cand: &str,
    sealed: bool,
    f: impl FnOnce(&mut Interp<'_>) -> R,
) -> R {
    let (prog, sdir) = prog_of(tag, suite, cand);
    let dirs = if sealed { vec![sdir] } else { Vec::new() };
    let mut ix = Interp::build_with(&prog, &dirs, Engine::Tree);
    assert_eq!(ix.seal.active, sealed);
    f(&mut ix)
}

/// [`with_prog`] over `suite` and `cand`, with the module-level `let`s
/// initialised, as a run evaluates them before `main`: without them every
/// global read would miss, and a refusal would prove nothing.
pub(super) fn with_src<R>(
    tag: &str,
    suite: &str,
    cand: &str,
    sealed: bool,
    f: impl FnOnce(&Interp<'_>) -> R,
) -> R {
    with_prog(tag, suite, cand, sealed, |ix| {
        ix.init_globals().expect("globals initialise");
        f(ix)
    })
}

/// [`with_src`] over [`SUITE`] and [`CAND`].
fn with_ix<R>(tag: &str, sealed: bool, f: impl FnOnce(&Interp<'_>) -> R) -> R {
    with_src(tag, SUITE, CAND, sealed, f)
}

/// Run fn `name`'s body with `args` (value, taint) bound to its parameters,
/// in a sealed frame when `sealed_frame`, with `pin_fn` set to it. The
/// sealed instantiation runs exactly when the run is sealed.
pub(super) fn go_t(
    ix: &Interp<'_>,
    name: &str,
    sealed_frame: bool,
    args: Vec<(Value, u8)>,
) -> Result<Value, Flow> {
    let f = *ix.fns.get(name).expect("a fn of the program");
    assert_eq!(f.params.len(), args.len(), "{name}: arity");
    let mut env = ix.new_env();
    let taints: Vec<u8> = args.iter().map(|(_, t)| *t).collect();
    env.bind_fresh(
        f.params
            .iter()
            .zip(args)
            .map(|(p, (v, _))| (intern(&p.name), v)),
        |i| taints[i],
    );
    let prev = ix.pin_fn.replace(f as *const FnDef as usize);
    let r = ix.with_frame(sealed_frame, || {
        if ix.seal.active {
            ix.eval_t::<true>(&f.body, &mut env)
        } else {
            ix.eval_t::<false>(&f.body, &mut env)
        }
    });
    ix.pin_fn.set(prev);
    match r {
        Ok(v) | Err(Flow::Return(v)) => Ok(v),
        Err(e) => Err(e),
    }
}

/// [`go_t`] with every argument clean.
pub(super) fn go(
    ix: &Interp<'_>,
    name: &str,
    sealed_frame: bool,
    args: Vec<Value>,
) -> Result<Value, Flow> {
    go_t(
        ix,
        name,
        sealed_frame,
        args.into_iter().map(|v| (v, 0)).collect(),
    )
}

pub(super) fn refused<T: std::fmt::Debug>(r: &Result<T, Flow>, needle: &str) -> bool {
    matches!(r, Err(Flow::Panic(m)) if m.contains(needle))
}

/// The runtime taint rules apply on this test's thread from here on
/// (`TAINT_FORCE_ON`, off by default in a unit test; R51 S4). A static-layer
/// assertion before the call is judged by the static rule alone.
pub(super) fn taint_rules_on() {
    TAINT_FORCE_ON.with(|c| c.set(true));
}

pub(super) fn strv(s: &str) -> Value {
    Value::Str(Rc::new(s.to_string()))
}

pub(super) fn u8v(n: i64) -> Value {
    Value::SizedInt {
        val: n,
        ty: IntWidth::U8,
    }
}

/// A dict `{ "k": v }`.
pub(super) fn dict_k(v: Value) -> Value {
    let mut m = std::collections::BTreeMap::new();
    m.insert("k".to_string(), v);
    Value::Dict(Rc::new(RefCell::new(m)))
}

/// The text sealed code is told for a name it cannot use.
pub(super) fn no_fn(name: &str) -> String {
    Interp::sealed_no_fn_msg(name)
}

const WIDTH: &str = "operator code did arithmetic on a fixed-width integer whose width nothing \
                     on the operator side determined";
const NAME: &str = "a function name nothing on the operator side determined";

/// v0.22 C9 round 9 (`a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path`,
/// veto interp.rs:10080): a sealed frame reads no operator global by any
/// path - the `Ident` arm (`ident_unbound`), the index and field fast paths
/// (`index_in_place`, `field_in_place`) and a closure-constant call
/// (`dispatch_named`) - and is told the common no-such-name text. Its own
/// global it reads; the operator frame reads everything.
#[test]
fn a_sealed_frame_cannot_read_an_operator_global_by_any_path() {
    with_ix("r51-s3-glob", true, |ix| {
        for (probe, global, want) in [
            ("read_index", "TABLE", Value::Int(1)),
            ("read_field", "CFG", Value::Int(7)),
            ("call_const", "G", Value::Int(2)),
        ] {
            let r = go(ix, probe, true, vec![]);
            assert!(
                refused(&r, &no_fn(global)),
                "ATTACK: {probe}: a sealed frame read the operator global `{global}`: {r:?}"
            );
            let c = go(ix, probe, false, vec![]);
            assert!(yields(&c, &want), "control: {probe}: {c:?}");
        }
        let r = go(ix, "read_whole", true, vec![]);
        assert!(refused(&r, &no_fn("TABLE")), "ATTACK: the Ident arm: {r:?}");
        assert!(go(ix, "read_whole", false, vec![]).is_ok());
        assert!(
            yields(&go(ix, "read_mine", true, vec![]), &Value::Int(5)),
            "a sealed frame reads its own global"
        );
        // The lookup itself, for every global of the program.
        let r = ix.with_frame(true, || ix.global_ref(intern("TABLE")).map(|v| v.cloned()));
        assert!(refused(&r, &no_fn("TABLE")), "global_ref: {r:?}");
        assert!(ix.global_ref(intern("TABLE")).is_ok_and(|v| v.is_some()));
        assert!(ix.global_ref(intern("NOPE")).is_ok_and(|v| v.is_none()));
    });
    // Unsealed: the same "sealed" frame reads everything (no edge runs).
    with_ix("r51-s3-glob-u", false, |ix| {
        assert!(yields(&go(ix, "read_index", true, vec![]), &Value::Int(1)));
    });
}

/// v0.22 `a_sealed_frame_cannot_take_an_operator_fn_as_a_value` (veto
/// interp.rs:10348) and `a_sealed_caller_cannot_tell_an_operator_fn_from_a_missing_one`
/// (11143): a fn named in value position is a reference to it, so the call
/// edge applies there; and a sealed caller is told the same words for an
/// operator name and a missing one, by `ident_unbound`, a name-resolving
/// builtin and a method miss.
#[test]
fn a_sealed_caller_cannot_use_or_tell_apart_an_operator_name() {
    with_ix("r51-s3-name", true, |ix| {
        let r = go(ix, "take_w", true, vec![]);
        assert!(
            refused(&r, &no_fn("w")),
            "ATTACK: a sealed frame took `w` as a value: {r:?}"
        );
        assert!(yields(&go(ix, "take_w", false, vec![]), &Value::Int(1)));
        let op = go(ix, "spawn_lit", true, vec![]);
        let missing = go(ix, "spawn_missing", true, vec![]);
        assert!(refused(&op, &no_fn("w")), "an operator fn: {op:?}");
        assert!(
            refused(&missing, &no_fn("nope")),
            "a missing fn: {missing:?}"
        );
        // The operator is told what it always was.
        let r = go(ix, "spawn_missing", false, vec![]);
        assert!(
            refused(&r, "[E1602] scheduler_spawn: no function `nope`"),
            "{r:?}"
        );
        // A method miss reads like a refused operator method.
        let r = go(ix, "no_method", true, vec![Value::Int(1)]);
        assert!(
            refused(&r, &no_fn("nothing")),
            "a sealed method miss: {r:?}"
        );
        let r = go(ix, "no_method", false, vec![Value::Int(1)]);
        assert!(refused(&r, "no method `nothing` on type `i64`"), "{r:?}");
        // The goal-constraint and kernel-goal lookups.
        for (b, args) in [
            (
                "kernel_goal_create",
                vec![Value::Int(0), strv("w"), Value::Float(1.0)],
            ),
            (
                "goal_run_constrained",
                vec![strv("solve"), strv("w"), Value::Float(0.0), Value::Int(1)],
            ),
        ] {
            let r = ix.with_frame(true, || ix.call_builtin(b, &args));
            assert!(refused(&r, &no_fn("w")), "ATTACK: {b} resolved `w`: {r:?}");
        }
    });
}

/// v0.22 `scheduler_spawn_takes_no_function_name_the_candidate_chose` and
/// `the_taint_name_rule_refuses_what_the_static_name_analysis_lets_through`
/// (amendment 100): operator code hands a name-resolving builtin only a name
/// the operator determined. The static rule (`seal_name_args`, pin.rs
/// `Ctx::npure`) refuses a parameter; the runtime rule (`t_check_names`)
/// refuses a name the static rule calls pure (a branch between two operator
/// literals) when the branch was chosen by the candidate's value.
#[test]
fn operator_code_hands_a_name_resolving_builtin_only_a_determined_name() {
    with_ix("r51-s3-nrule", true, |ix| {
        assert!(
            go(ix, "spawn_lit", false, vec![]).is_ok(),
            "an operator literal"
        );
        let r = go(ix, "spawn_param", false, vec![strv("w")]);
        assert!(
            refused(&r, NAME) && refused(&r, "scheduler_spawn"),
            "ATTACK: a parameter named the fn: {r:?}"
        );
        assert!(
            !refused(&r, "runtime taint"),
            "the static rule refused it: {r:?}"
        );
        taint_rules_on();
        // Pure statically; the runtime taint of the branch decides.
        assert!(go_t(ix, "spawn_branch", false, vec![(Value::Int(1), 0)]).is_ok());
        let r = go_t(ix, "spawn_branch", false, vec![(Value::Int(1), ALL)]);
        assert!(
            refused(&r, NAME) && refused(&r, "runtime taint"),
            "ATTACK: the candidate's value chose the name: {r:?}"
        );
        // The sealed frame's own calls are judged by the call edge instead.
        assert!(ix
            .with_frame(true, || ix.t_check_names("scheduler_spawn", &[VAL]))
            .is_ok());
        assert!(ix.t_check_names("scheduler_spawn", &[0]).is_ok());
        assert!(ix.t_check_names("len", &[VAL]).is_ok(), "not a name sink");
        assert!(refused(&ix.t_check_names("scheduler_spawn", &[VAL]), NAME));
    });
    with_ix("r51-s3-nrule-u", false, |ix| {
        assert!(
            go(ix, "spawn_param", false, vec![strv("w")]).is_ok(),
            "unsealed: no rule"
        );
    });
}

/// v0.22 `operator_arithmetic_never_runs_at_a_width_the_candidate_chose`
/// (veto interp.rs:10752) and its unary form (10178): operator arithmetic on
/// a fixed-width integer whose width nothing on the operator side determined
/// is refused - statically (`seal_width`, `seal_width_unary`: a value read
/// untyped from a dict) and by the runtime taint (`t_check_width`: a typed
/// parameter whose value carries the candidate's type taint). A comparison is
/// not arithmetic; a `let x: u8` pin determines the width; a sealed frame's
/// own arithmetic is its own.
#[test]
fn operator_arithmetic_never_runs_at_a_width_the_candidate_chose() {
    with_ix("r51-s3-width", true, |ix| {
        let d = || dict_k(u8v(255));
        let r = go(ix, "shift", false, vec![d()]);
        assert!(
            refused(&r, WIDTH),
            "ATTACK: `v << 1` at the candidate's width: {r:?}"
        );
        let r = go(ix, "neg", false, vec![d()]);
        assert!(
            refused(&r, WIDTH),
            "ATTACK: `-v` at the candidate's width: {r:?}"
        );
        assert!(go(ix, "compare", false, vec![d()]).is_ok(), "a comparison");
        assert!(
            go(ix, "shift", false, vec![dict_k(Value::Int(4))]).is_ok(),
            "an i64"
        );
        assert!(
            go(ix, "shift_pinned", false, vec![d()]).is_ok(),
            "pinned by `let x: u8`"
        );
        assert!(
            go(ix, "shift", true, vec![d()]).is_ok(),
            "the sealed frame's own"
        );
        assert!(
            go(ix, "neg", true, vec![d()]).is_ok(),
            "the sealed frame's own"
        );
        taint_rules_on();
        // The runtime rule: determined statically, the value's TYPE taint
        // says the candidate chose it.
        assert!(go_t(ix, "shift_t", false, vec![(u8v(255), 0)]).is_ok());
        let r = go_t(ix, "shift_t", false, vec![(u8v(255), ALL)]);
        assert!(
            refused(&r, WIDTH) && refused(&r, "runtime taint"),
            "ATTACK: a tainted width: {r:?}"
        );
        let r = go_t(ix, "neg_t", false, vec![(u8v(4), ALL)]);
        assert!(
            refused(&r, "runtime taint"),
            "ATTACK: a tainted unary width: {r:?}"
        );
        assert!(
            go_t(ix, "neg_t", false, vec![(u8v(4), VAL)]).is_ok(),
            "VAL is not TYP"
        );
    });
    with_ix("r51-s3-width-u", false, |ix| {
        assert!(
            go(ix, "shift", false, vec![dict_k(u8v(255))]).is_ok(),
            "unsealed"
        );
    });
}

/// v0.22 PCI candidate-3 review (veto interp.rs:3904, 4165, 4415, eval.rs:
/// 227, 868): a refinement attaches BY NAME, so the candidate's refinement
/// never runs in operator code; the operator's own does.
#[test]
fn a_candidates_refinement_never_runs_in_operator_code() {
    with_ix("r51-s3-refine", true, |ix| {
        let r = go(ix, "theirs", false, vec![Value::Int(3)]);
        assert!(
            refused(
                &r,
                "the refinement `Pos` is the candidate's and cannot run in the operator's code"
            ),
            "ATTACK: the candidate's `where` ran in operator code: {r:?}"
        );
        assert!(
            go(ix, "theirs", true, vec![Value::Int(3)]).is_ok(),
            "its own frame"
        );
        assert!(
            go(ix, "mine", false, vec![Value::Int(3)]).is_ok(),
            "the operator's own"
        );
        assert!(ix.seal_type("Fake") && !ix.seal_type("Cfg"), "seal_type");
    });
}

/// v0.22 `a_candidates_method_never_runs_under_the_operators_method_name`
/// (veto interp.rs:6052) and `operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict`
/// (9292): a method name the operator defines never selects the candidate's
/// method (`seal_method`), and operator code never selects between its own
/// impls by a receiver type nothing on its side determined (`seal_dispatch`).
#[test]
fn operator_method_dispatch_is_never_the_candidates_choice() {
    with_ix("r51-s3-method", true, |ix| {
        let fake = ix
            .with_frame(true, || go(ix, "fake", true, vec![]))
            .expect("a Fake");
        let r = go(ix, "judge_fake", false, vec![fake.clone()]);
        assert!(
            refused(&r, "a method the operator defines")
                && refused(&r, "the candidate would choose the code"),
            "ATTACK: the candidate's `ok` ran under the operator's name: {r:?}"
        );
        assert!(
            go(ix, "judge_fake", true, vec![fake]).is_ok(),
            "its own frame"
        );
        let r = go(ix, "judge", false, vec![dict_k(Value::Bool(true))]);
        assert!(
            refused(&r, "whose type nothing on the operator side determined"),
            "ATTACK: dispatched on a dict value's runtime type: {r:?}"
        );
        assert!(
            yields(
                &go(ix, "judge_pinned", false, vec![dict_k(Value::Int(9))]),
                &Value::Bool(true)
            ),
            "pinned by `let y: i64`"
        );
    });
    with_ix("r51-s3-method-u", false, |ix| {
        assert!(go(ix, "judge", false, vec![dict_k(Value::Bool(true))]).is_ok());
    });
}

/// v0.22 amendment 53 (veto eval.rs:241-255, 842-850) and 72 (eval.rs:621,
/// 664): in a sealed run a `let x: T` annotation and a record field are
/// declared types the value is cast to, a `chan<T>()` is stamped with its
/// element type, and a value sent on it is cast to that type.
#[test]
fn declared_types_inside_a_frame_refuse_a_value_of_another_type() {
    with_ix("r51-s3-cast", true, |ix| {
        let r = go(ix, "typed_let", false, vec![Value::Bool(true)]);
        assert!(
            refused(&r, "`x` is declared `i64` but was bound to"),
            "ATTACK: a bool bound to `let x: i64`: {r:?}"
        );
        let r = go(ix, "build_cfg", false, vec![Value::Bool(true)]);
        assert!(
            refused(&r, "field `k` of `Cfg` was given true"),
            "ATTACK: a bool in an i64 field: {r:?}"
        );
        let q = go(ix, "mk_chan", false, vec![]).expect("a channel");
        let r = go(ix, "send_on", true, vec![q.clone(), Value::Bool(true)]);
        assert!(
            refused(&r, "a channel declared `Chan<i64>` was sent true"),
            "ATTACK: a sealed frame sent a bool on a `Chan<i64>`: {r:?}"
        );
        let Value::Chan(c) = &q else { unreachable!() };
        assert!(c.borrow().is_empty(), "nothing was sent");
    });
    with_ix("r51-s3-cast-u", false, |ix| {
        assert!(
            go(ix, "typed_let", false, vec![Value::Bool(true)]).is_ok(),
            "unsealed"
        );
        let q = go(ix, "mk_chan", false, vec![]).expect("a channel");
        assert!(go(ix, "send_on", true, vec![q, Value::Bool(true)]).is_ok());
    });
}

// ── Carriers: what sealed code touched reaches an operator name sink ──────

/// Operator probes for the carriers a value can travel by (v0.22
/// `taint_tests::carrier_cases`, veto taint_tests.rs:432, and
/// `channel_cases`, 889). Each ends in `named`/`named_i`, a name-resolving
/// builtin fed a branch between two operator literals: statically pure, so
/// only the runtime taint of the branch condition decides (`t_check_names`).
const CARRY: &str = "\
let TABLE = [1, 2, 3]
let PAIR = (9, 4)
fn w(n: i64) -> i64 { n }
fn secret(n: i64) -> i64 { n * 7 }
fn plain(n: i64) -> i64 { n }
fn named(s: str) -> i64 {
    let n = if s == \"r\" { \"w\" } else { \"w\" }
    scheduler_spawn(n, 1)
}
fn named_i(k: i64) -> i64 {
    let n = if k == 1 { \"w\" } else { \"w\" }
    scheduler_spawn(n, 1)
}
fn via_dict() -> i64 {
    let d = dict_new()
    put(d)
    named(match dict_get(d, \"n\") { Some(s) => s  None => \"\" })
}
fn via_dict_read() -> i64 {
    let d = dict_new()
    dict_set(d, \"n\", \"r\")
    let k = peek(d)
    named(match dict_get(d, \"n\") { Some(s) => s  None => \"\" })
}
fn via_chan() -> i64 {
    let c = chan<str>()
    push(c)
    named(c.recv())
}
fn via_chan_read() -> i64 {
    let c = chan<str>()
    c.send(\"r\")
    let k = peek_c(c)
    named(c.recv())
}
fn via_mut() -> i64 {
    let xs = [\"w\"]
    fill(&mut xs)
    named(xs[0])
}
fn via_capture() -> i64 {
    let n = 0
    let cb = |v| { if v > 0 { n = v }
        n }
    let r = run_cb(cb)
    named_i(cb(0))
}
fn via_capture_own() -> i64 {
    let n = 0
    let cb = |v| { if v > 0 { n = v }
        n }
    let r = cb(1)
    named_i(cb(0))
}
fn via_returned() -> i64 { named(mk()()) }
fn via_sched() -> i64 {
    let id = scheduler_spawn(\"idx1\", 0)
    let n = scheduler_run()
    named_i(scheduler_result(id))
}
fn via_sandbox() -> i64 {
    let p = principal_root(\"r\", true, true, true, 100)
    let sb = sandbox_create(p, \"IO\")
    named_i(sandbox_run(sb, \"idx1\", 0))
}
fn chan_own() -> i64 {
    let c = chan<i64>()
    c.send(7)
    named_i(c.len())
}
fn chan_untouched() -> i64 {
    let c = chan<i64>()
    c.send(7)
    let d = chan<i64>()
    push_i(d)
    named_i(c.len())
}
fn chan_relay() -> i64 {
    let c = chan<i64>()
    c.send(21)
    relay(c)
}
fn chan_sent() -> i64 {
    let c = chan<i64>()
    push_i(c)
    named_i(c.len())
}
fn chan_drained() -> i64 {
    let c = chan<i64>()
    c.send(7)
    drain(c)
    named_i(c.len())
}
fn chan_try_drained() -> i64 {
    let c = chan<i64>()
    c.send(7)
    drain_try(c)
    named_i(c.len())
}
fn chan_try_recv_after() -> i64 {
    let c = chan<i64>()
    c.send(7)
    drain(c)
    named_i(match c.try_recv() { Some(v) => 1  None => 0 })
}
fn chan_select() -> i64 {
    let a = chan<i64>()
    let b = chan<i64>()
    a.send(7)
    b.send(7)
    drain(a)
    named_i(select { a.recv() => 0  b.recv() => 1 })
}
fn chan_loop() -> i64 {
    let c = chan<i64>()
    for i in 0..idx() { c.send(i) }
    named_i(c.len())
}
fn chan_loop_own() -> i64 {
    let c = chan<i64>()
    for i in 0..1 { c.send(i) }
    named_i(c.len())
}
fn own_fn_value() -> i64 { solve_fv(3) }
fn held_fn_value() -> i64 {
    let d = dict_new()
    dict_set(d, \"f\", plain)
    call_held(d)
}
fn held_closure() -> i64 {
    let d = dict_new()
    dict_set(d, \"f\", || 9)
    swap_in(d)
    0
}
fn build_range() -> i64 {
    let r = Range { lo: 1, hi: 2 }
    1
}
fn name_by_mine() -> i64 { named_i(MINE) }
";

/// The sealed candidate of [`CARRY`].
const CARRY_CAND: &str = "\
let MINE = 1
type Range = { lo: i64, hi: i64 } where stash(_.lo)
fn stash(x: i64) -> bool {
    let id = scheduler_spawn(\"secret\", 21)
    true
}
fn put(d: Dict) { dict_set(d, \"n\", \"r\") }
fn peek(d: Dict) -> i64 { dict_len(d) }
fn push(c: Chan<str>) { c.send(\"r\") }
fn peek_c(c: Chan<str>) -> i64 { c.len() }
fn push_i(c: Chan<i64>) { c.send(7) }
fn drain(c: Chan<i64>) { let x = c.recv() }
fn drain_try(c: Chan<i64>) { let x = c.try_recv() }
fn relay(c: Chan<i64>) -> i64 { c.recv() * 2 }
fn fill(xs: &mut [str]) { xs[0] = \"r\" }
fn run_cb(f: fn(i64) -> i64) -> i64 { f(1) }
fn mk() -> fn() -> str { || \"r\" }
fn idx() -> i64 { 1 }
fn idx1(x: i64) -> i64 { 1 }
fn inc(n: i64) -> i64 { n + 1 }
fn solve_fv(n: i64) -> i64 {
    let g = inc
    g(n)
}
fn call_held(d: Dict) -> i64 {
    match dict_get(d, \"f\") { Some(f) => f(3)  None => 0 }
}
fn mine9() -> i64 { 9 }
fn swap_in(d: Dict) { dict_set(d, \"f\", mine9) }
fn read_pair() -> i64 { PAIR.0 }
";

const TAINT_NAME: &str = "(runtime taint)";

/// v0.22 `taint_survives_every_carrier_a_value_can_travel_by` (veto
/// taint_tests.rs:457; sealed_frames.rs:1606): a value sealed code wrote
/// reaches operator code tainted by every carrier - a dict it wrote
/// (`t_builtin_out` marks, `t_builtin_in` reads), a channel it sent on
/// (`t_chan_access`), a `&mut` it wrote back, a closure capture it set, a
/// closure it returned, a fiber it ran and `sandbox_run`'s result - and the
/// name rule (`t_check_names`) refuses the operator name it chose. A sealed
/// READ of an operator dict or channel, or a closure the operator alone
/// called, taints nothing.
#[test]
fn taint_reaches_an_operator_name_by_every_carrier() {
    let fails = judge_probes(
        "r51-s3e-carry",
        &[
            "via_dict",
            "via_chan",
            "via_mut",
            "via_capture",
            "via_returned",
            "via_sched",
            "via_sandbox",
        ],
        &["via_dict_read", "via_chan_read", "via_capture_own"],
    );
    assert!(fails.is_empty(), "{}", fails.join("\n"));
    // Unsealed: no rule, every probe runs.
    with_src("r51-s3e-carry-u", CARRY, CARRY_CAND, false, |ix| {
        for probe in [
            "via_dict",
            "via_chan",
            "via_mut",
            "via_capture",
            "via_sched",
        ] {
            let r = go(ix, probe, false, vec![]);
            assert!(r.is_ok(), "unsealed {probe}: {r:?}");
        }
    });
}

/// Run each [`CARRY`] probe in an operator frame of its OWN sealed `Interp`
/// (one test, one run: a refusal ends a run, so no probe inherits another's
/// taint state). Every `attacks` probe must be refused by the runtime name
/// rule, every `honest` one must run; the misses, listed.
fn judge_probes(tag: &str, attacks: &[&str], honest: &[&str]) -> Vec<String> {
    taint_rules_on();
    let run = |probe: &str| {
        with_src(&format!("{tag}-{probe}"), CARRY, CARRY_CAND, true, |ix| {
            go(ix, probe, false, vec![])
        })
    };
    let mut fails = Vec::new();
    for probe in attacks {
        let r = run(probe);
        if !(refused(&r, NAME) && refused(&r, TAINT_NAME)) {
            fails.push(format!("ATTACK: {probe}: {r:?}"));
        }
    }
    for probe in honest {
        let r = run(probe);
        if r.is_err() {
            fails.push(format!("control {probe}: {r:?}"));
        }
    }
    fails
}

/// v0.22 `a_channel_the_candidate_touched_never_selects_an_operator_closure_or_name`
/// (veto taint_tests.rs:921, amendment 106; sealed_frames.rs:1649), the NAME
/// half: a channel is one shared object with one taint, so a count, an
/// emptiness or a `select` arm the candidate decided by a send or a drain
/// (`t_chan_access` marks it; the method and `select` read it), or a send in
/// a loop it sized (`t_chan_choice` over the loop's control taint), never
/// names an operator fn. The operator's own channel, a channel the candidate
/// never touched, and data the candidate relays pass. The CLOSURE half
/// (`ops[c.len()]`, then a call) is the PICK rule, R51 S4.
#[test]
fn a_channel_the_candidate_touched_never_selects_an_operator_name() {
    let mut fails = judge_probes(
        "r51-s3e-chan",
        &[
            "chan_sent",
            "chan_drained",
            "chan_try_drained",
            "chan_try_recv_after",
            "chan_select",
            "chan_loop",
        ],
        &["chan_own", "chan_untouched", "chan_loop_own"],
    );
    let r = with_src("r51-s3e-chan-relay", CARRY, CARRY_CAND, true, |ix| {
        go(ix, "chan_relay", false, vec![])
    });
    if !yields(&r, &Value::Int(42)) {
        fails.push(format!("control chan_relay: {r:?}"));
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// v0.22 `a_candidates_own_fn_value_takes_arguments_and_an_operators_still_does_not`
/// (veto interp.rs:10128) and the tuple path of
/// `a_candidate_calls_its_own_fn_value_and_never_reads_an_operator_global`
/// (sealed_frames.rs:1250): a candidate calls its OWN fn value with an
/// argument (the `sealed_fnval` mark: the call is not `entering` operator
/// code); an operator fn value a dict held, called by sealed code, has its
/// argument cast strictly and is refused; a candidate fn value never replaces
/// an operator closure the operator's dict held (`replaced_ok`); and a sealed
/// frame reads no operator global through a tuple field either.
#[test]
fn a_candidate_calls_its_own_fn_value_and_no_operator_one() {
    with_src("r51-s3e-fnval", CARRY, CARRY_CAND, true, |ix| {
        let r = go(ix, "own_fn_value", false, vec![]);
        assert!(
            yields(&r, &Value::Int(4)),
            "the candidate's own fn value: {r:?}"
        );
        let r = go(ix, "solve_fv", true, vec![Value::Int(3)]);
        assert!(yields(&r, &Value::Int(4)), "inside its own frame: {r:?}");
        let r = go(ix, "held_fn_value", false, vec![]);
        assert!(
            refused(&r, "nothing on the operator side determined"),
            "ATTACK: sealed code called an operator fn value with an undetermined argument: {r:?}"
        );
        let r = go(ix, "held_closure", false, vec![]);
        assert!(
            refused(&r, "now holds the candidate's"),
            "ATTACK: a candidate fn value replaced an operator closure: {r:?}"
        );
        let r = go(ix, "read_pair", true, vec![]);
        assert!(
            refused(&r, &no_fn("PAIR")),
            "ATTACK: a sealed frame read an operator global through a tuple field: {r:?}"
        );
        assert!(yields(&go(ix, "read_pair", false, vec![]), &Value::Int(9)));
    });
}

/// v0.22 PCI candidate-4 review (veto eval.rs:908, interp.rs:3292;
/// check_effects.rs `a_sealed_candidate_cannot_reach_operator_state_by_handle_or_definition`):
/// definition-owned code runs under its DEFINITION's provenance, whoever
/// evaluates it. A candidate struct's whole-struct `where`, run when the
/// operator builds one, is a sealed frame (so it may not name an operator
/// fn); a candidate's module-level `let` initialiser is a sealed frame too,
/// so its value carries the candidate's taint into an operator name sink.
#[test]
fn definition_owned_code_runs_under_its_definitions_provenance() {
    let r = with_src("r51-s3e-defn-where", CARRY, CARRY_CAND, true, |ix| {
        go(ix, "build_range", false, vec![])
    });
    assert!(
        refused(&r, &no_fn("secret")),
        "ATTACK: the candidate's `where` ran in an operator frame: {r:?}"
    );
    taint_rules_on();
    let r = with_src("r51-s3e-defn-let", CARRY, CARRY_CAND, true, |ix| {
        go(ix, "name_by_mine", false, vec![])
    });
    assert!(
        refused(&r, NAME) && refused(&r, TAINT_NAME),
        "ATTACK: the candidate's `let` initialiser was the operator's: {r:?}"
    );
    with_src("r51-s3e-defn-u", CARRY, CARRY_CAND, false, |ix| {
        assert!(go(ix, "build_range", false, vec![]).is_ok());
        assert!(go(ix, "name_by_mine", false, vec![]).is_ok());
    });
}

/// veto interp.rs:3292 (`init_globals`): each module-level initialiser runs
/// under its own definition's provenance in a fresh environment, so the call
/// edge and the global-read edge see a candidate's initialiser (an earlier
/// global is reached through `global_ref`, never as a local of a shared
/// env), and its kernel effects land in the candidate's kernel.
#[test]
fn a_candidates_module_level_let_is_a_sealed_frame() {
    let refused_init = |tag: &str, cand: &str, needle: &str| {
        with_prog(tag, CARRY, cand, true, |ix| {
            let r = ix.init_globals();
            assert!(
                refused(&r, needle),
                "ATTACK: a candidate initialiser {cand:?}: {r:?}"
            );
        });
        with_prog(&format!("{tag}-u"), CARRY, cand, false, |ix| {
            assert!(ix.init_globals().is_ok(), "unsealed {cand:?}");
        });
    };
    refused_init(
        "r51-s3e-init-call",
        "let STOLEN = secret(1)\n",
        &no_fn("secret"),
    );
    refused_init(
        "r51-s3e-init-read",
        "let PEEK = TABLE[0]\n",
        &no_fn("TABLE"),
    );
    refused_init("r51-s3e-init-whole", "let PEEK = PAIR\n", &no_fn("PAIR"));
    with_prog(
        "r51-s3e-init-k",
        CARRY,
        "fn idx1(x: i64) -> i64 { 1 }\nlet ID = scheduler_spawn(\"idx1\", 1)\nlet OWN = ID + 1\n",
        true,
        |ix| {
            ix.init_globals()
                .expect("a candidate initialiser reads its own earlier global");
            assert_eq!(ix.kernels[0].scheduler.borrow().len(), 0, "operator fibers");
            assert_eq!(ix.kernels[1].scheduler.borrow().len(), 1, "sealed fibers");
            assert!(yields(
                &Ok(ix.globals[&intern("OWN")].clone()),
                &Value::Int(1)
            ));
        },
    );
}

/// check_effects.rs `a_candidate_cannot_redefine_a_suite_helpers_impl_or_constant`
/// (PCI 2b), the runtime half: the resolver refuses a second definition of a
/// suite helper's impl, dispatch key or constant (E0002,
/// `resolver::tests::duplicate_let_refinement_or_impl_produces_e0002`); were
/// a candidate's `impl Other for Expect { fn check .. }` to take the
/// operator's dispatch key anyway (this `Interp` is built without the
/// resolver), the operator's `e.check(..)` is refused by the method edge
/// (`seal_method`), never run as the candidate's no-op.
#[test]
fn a_candidates_impl_never_answers_an_operator_types_method() {
    const SUITE_E: &str = "\
type Expect = { want: i64 }
trait Judge { fn check(self: Expect, got: i64) -> bool }
impl Judge for Expect { fn check(self: Expect, got: i64) -> bool { got == self.want } }
fn judge_expect() -> bool {
    let e = Expect { want: 42 }
    e.check(0)
}
";
    const CAND_E: &str = "\
trait Other { fn check(self: Expect, got: i64) -> bool }
impl Other for Expect { fn check(self: Expect, got: i64) -> bool { true } }
";
    let r = with_src("r51-s3e-impl", SUITE_E, CAND_E, true, |ix| {
        go(ix, "judge_expect", false, vec![])
    });
    assert!(
        refused(&r, "a method the operator defines"),
        "ATTACK: the candidate's `check` answered the operator's `e.check(..)`: {r:?}"
    );
    let honest = with_src("r51-s3e-impl-h", SUITE_E, "", true, |ix| {
        go(ix, "judge_expect", false, vec![])
    });
    assert!(
        yields(&honest, &Value::Bool(false)),
        "the operator's own: {honest:?}"
    );
}

// ── Drift ───────────────────────────────────────────────────────────────────

/// `(file under interp/, fn, code)` for every non-comment line of the
/// interpreter (`interp.rs` and `interp/**`, `interp/vm/` included; test
/// modules excluded) that contains `needle` as a whole word after a `.`.
fn field_sites(needle: &str) -> Vec<(String, String, String)> {
    let sources = vm::tests::r51_interp_sources();
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut out = Vec::new();
    for (file, src) in &sources {
        if file.ends_with("_tests.rs") {
            continue;
        }
        let mut func = String::new();
        for line in src.lines() {
            let code = line.find("//").map_or(line, |i| &line[..i]);
            let b = code.as_bytes();
            let mut from = 0;
            while let Some(i) = code[from..].find("fn ") {
                let at = from + i;
                from = at + 3;
                if at > 0 && ident(b[at - 1]) {
                    continue;
                }
                let name: String = code[at + 3..]
                    .bytes()
                    .take_while(|&c| ident(c))
                    .map(char::from)
                    .collect();
                if !name.is_empty() {
                    func = name;
                }
            }
            let pat = format!(".{needle}");
            let mut from = 0;
            while let Some(i) = code[from..].find(&pat) {
                let end = from + i + pat.len();
                from = end;
                if end < b.len() && ident(b[end]) && ident(*pat.as_bytes().last().unwrap()) {
                    continue;
                }
                out.push((file.clone(), func.clone(), code.trim().to_string()));
            }
        }
    }
    assert!(
        sources.iter().any(|(f, _)| f == "vm/mod.rs"),
        "the sweep reads interp/vm/"
    );
    out
}

/// v0.22 C9 round 9 (veto interp.rs:9632, widened per TaintPort-9 to main's
/// identifiers and `interp/vm/`): the global-read edge lives in ONE lookup.
/// Every line of the interpreter that touches a `globals` map is listed with
/// the reason it is not a read by running code; everything else reads through
/// [`Interp::global_ref`] (or asks existence only, [`Interp::is_global`]).
#[test]
fn every_global_read_goes_through_global_ref() {
    const ALLOWED: &[(&str, &str, &str, &str)] = &[
        ("", "of", "seal.globals.insert(intern(name));", "the seal's own set of sealed global names, built once"),
        ("", "merged_bindings", ".globals", "the end-of-run binding dump, not running code"),
        ("", "seal_global", "if self.seal.active && self.frame_sealed.get() && !self.seal.globals.contains(&s) {", "the edge itself (the sealed names)"),
        ("", "is_global", "self.globals.contains_key(&s)", "existence only, no value: the index arm's and the VM's in-place test, followed by `global_ref`"),
        ("", "init_globals_sealed", "let sealed = self.seal.globals.contains(&s);", "the seal's set of sealed global names: the initialiser's provenance"),
        ("", "init_globals_sealed", "self.globals.insert(s, v);", "the write at build, one initialiser at a time (a later one reads it through `global_ref`)"),
        ("", "global_ref", "match self.globals.get(&s) {", "THE lookup, which applies `seal_global`"),
        ("", "init_globals", "self.globals = env.snapshot().into_parts().0.into_iter().collect();", "the write at build"),
        ("pin.rs", "det", "self.globals.contains(n)", "the static pin analysis' set of global NAMES"),
        ("pin.rs", "build", "ctx.globals = lets.iter().map(|(n, _)| n.to_string()).collect();", "the static pin analysis' set of global NAMES"),
        ("pin.rs", "build", ".filter(|(n, e)| ctx.globals.contains(*n) && ctx.det(e, &none, &none))", "the static pin analysis' set of global NAMES"),
        ("pin.rs", "build", "if next == ctx.globals {", "the static pin analysis' set of global NAMES"),
        ("pin.rs", "build", "ctx.globals = next;", "the static pin analysis' set of global NAMES"),
    ];
    let mut found: Vec<(String, String, String)> = field_sites("globals");
    let mut allowed: Vec<(String, String, String)> = ALLOWED
        .iter()
        .map(|(f, n, l, _)| (f.to_string(), n.to_string(), l.to_string()))
        .collect();
    found.sort();
    allowed.sort();
    assert_eq!(
        found, allowed,
        "DRIFT: a mention of the `globals` map bypasses `global_ref` (and so `seal_global`); \
         route it through `Interp::global_ref`, or list it with its reason"
    );
    // The lookup has its readers: the `Ident` arm's rest, both in-place fast
    // paths and the closure-constant call (no vacuous pass).
    let mut readers: Vec<String> = field_sites("global_ref(")
        .into_iter()
        .map(|(_, f, _)| f)
        .collect();
    readers.sort();
    readers.dedup();
    for want in [
        "dispatch_named",
        "field_in_place",
        "ident_unbound",
        "index_in_place",
    ] {
        assert!(
            readers.iter().any(|r| r == want),
            "DRIFT: `{want}` no longer reads globals through `global_ref`: {readers:?}"
        );
    }
}

/// The body of the first `fn name` in the interpreter sources (test modules
/// excluded): its lines up to the closing brace at the `fn`'s indent.
pub(super) fn fn_body(name: &str) -> String {
    for (file, src) in vm::tests::r51_interp_sources() {
        if file.ends_with("_tests.rs") {
            continue;
        }
        let lines: Vec<&str> = src.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            let t = l.trim_start();
            let t = t
                .strip_prefix("pub(super) ")
                .or_else(|| t.strip_prefix("pub(crate) "))
                .unwrap_or(t);
            let Some(rest) = t.strip_prefix("fn ") else {
                continue;
            };
            if !rest.starts_with(name) || !rest[name.len()..].starts_with(['(', '<']) {
                continue;
            }
            let indent = l.len() - l.trim_start().len();
            let close = format!("{}}}", " ".repeat(indent));
            let end = lines[i + 1..]
                .iter()
                .position(|x| *x == close)
                .map_or(lines.len(), |p| i + 1 + p);
            return lines[i..=end.min(lines.len() - 1)]
                .iter()
                .map(|x| x.find("//").map_or(*x, |c| &x[..c]))
                .collect::<Vec<_>>()
                .join("\n");
        }
    }
    panic!("DRIFT: no `fn {name}` in the interpreter sources")
}

/// Spec §4 Fork 3 (TaintPort-8, VmPin-4b/c/e): every main-only in-place or
/// split-path function that has no v0.22 counterpart is classified as a taint
/// touch point and/or a seal edge, and the classification is pinned by the
/// code that implements it (`checked_in` contains each `needle`). A sink
/// whose edge lives on the call path (the crossing lane) is classified with
/// the fn and text where that edge is applied.
#[test]
fn every_main_only_sink_is_classified() {
    // (sink, classification, fn whose body implements it, needles)
    const SINKS: &[(&str, &str, &str, &[&str])] = &[
        ("operand", "neither: never reached under seal (eval_binop's sealed arm evaluates both operands through eval_t and returns first)", "eval_binop", &["if T {", "self.seal_width(", "return binop_operands(op, Operand::of(l), Operand::of(r))"]),
        ("binop_operands", "seal edge (width) and taint touch point (t_check_width) applied to its operands before it", "eval_binop", &["self.seal_width(", "self.t_check_width(&l, lt", "self.t_check_width(&r, rt"]),
        ("index_in_place", "seal edge: global read", "index_in_place", &["self.global_ref(s)?"]),
        ("field_in_place", "seal edge: global read", "field_in_place", &["self.global_ref(s)?"]),
        ("assign_in_place", "taint touch point (the appended operand's taint joins the binding); no width edge: the binding is a str or an array", "assign_in_place", &["get_var_mut_t::<T>(s, slot, yt)", "Some(Value::Str(_) | Value::Array(_))"]),
        ("assign_in_place_shaped", "neither: a syntactic shape test, reads no value", "assign_in_place_shaped", &["match value {"]),
        ("write_place", "taint touch point: the AssignTo arm writes the binding's taint before it", "eval_arm", &["env.get_var_mut_t::<T>(base, base_slot, wt);", "write_place(base, base_slot, &steps, v, env)?"]),
        ("call_mut_with", "seal edge on the call path (the `&mut` hand-back cast, crossing lane); the callee frame is call_fn_in's", "call_mut_with", &["self.call_fn_in(entry, argv, &mut frame)"]),
        ("chan_method", "seal edge (chan_send_check, dict_edge_in) and taint touch point (t_mark_obj)", "chan_method", &["self.chan_send_check(q, &mut v)?", "self.dict_edge_in(&v)?", "self.t_mark_obj("]),
        ("dispatch_call", "seal edge before it: the name rule at eval_call's sealed arm", "eval_name_sink_call", &["self.seal_name_args(name, args)?", "self.t_check_names(name, &ats)?"]),
        ("dispatch_named", "seal edge (global read, no-such-fn text) and taint touch point (builtin in/out)", "dispatch_named", &["self.global_ref(s)?", "self.no_such_fn(", "t_builtin_in(", "t_builtin_out("]),
        ("impl_method", "seal edge via its sealed twin (the tree MethodCall arm under T; VM Op::MethodCall never runs sealed)", "impl_method_sealed", &["self.seal_method(f, &tn)?", "self.seal_dispatch(receiver, f, &argv[0], &tn)?", "self.no_such_fn("]),
        ("call_closure_arg", "taint touch point (the builtin's loop pc); the closure argument cast is a crossing edge", "call_closure_arg", &["t_loop_pc"]),
        ("cond_bool", "neither: value to bool; the branch's control taint is taken by its callers (t_branch)", "eval_arm", &["self.t_branch("]),
        ("logic_rhs", "neither: combines evaluated values; taint at short_circuit_tainted", "short_circuit_tainted", &["logic_rhs("]),
        ("short_circuits", "neither: a value test; taint at short_circuit_tainted", "short_circuit_tainted", &["short_circuits("]),
        ("finish_record", "seal edge (field casts, refinement edge, definition-owned provenance)", "finish_record", &["self.cast_fields(name, &mut fields)?", "self.seal_refine(rn)?", "self.seal_type(name)", "self.with_frame(sealed"]),
        ("bind_let", "seal edge (declared-type cast, refinement edge) and taint touch point (TYP strip by pin)", "bind_let", &["self.cast(&mut v, t,", "self.t_pins(t)", "self.seal_refine(rn)?", "define_var_t::<T>(s, slot, v, vt)"]),
        ("bind_params", "taint touch point: every parameter binds with the frame's taint", "bind_params", &["|_| t,"]),
        ("call_fn_common", "neither: only a plain entry takes it, and no entry of a sealed run is plain", "call_fn_in", &["if !entry.plain"]),
        ("call_fn_in_general", "seal edge and taint touch point: a sealed run's call goes through call_fn_sealed (the crossing, then the frame's sealed instantiation)", "call_fn_in_general", &["self.call_fn_sealed(entry, args, env)"]),
        ("scalar_return", "neither: a value-shape check; the declared-return cast is the crossing edge", "scalar_return", &["match (entry.ret, &result) {"]),
    ];
    const FORK3: &[&str] = &[
        "operand",
        "binop_operands",
        "index_in_place",
        "field_in_place",
        "assign_in_place",
        "assign_in_place_shaped",
        "write_place",
        "call_mut_with",
        "chan_method",
        "dispatch_call",
        "dispatch_named",
        "impl_method",
        "call_closure_arg",
        "cond_bool",
        "logic_rhs",
        "short_circuits",
        "finish_record",
        "bind_let",
        "bind_params",
        "call_fn_common",
        "call_fn_in_general",
        "scalar_return",
    ];
    let listed: Vec<&str> = SINKS.iter().map(|s| s.0).collect();
    assert_eq!(
        listed, FORK3,
        "DRIFT: the spec's main-only sink list changed"
    );
    let mut bad = Vec::new();
    for (sink, class, at, needles) in SINKS {
        // The sink exists (a renamed one must be re-classified).
        fn_body(sink);
        assert!(!class.is_empty());
        let body = fn_body(at);
        for n in *needles {
            if !body.contains(n) {
                bad.push(format!("{sink} ({class}): `{at}` no longer contains `{n}`"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "DRIFT: a sink's classification no longer holds:\n{}",
        bad.join("\n")
    );
    // `scalar_return` and `call_fn_common` stay off every sealed path.
    let sym = vm::tests::r51_interp_sources()
        .into_iter()
        .find(|(f, _)| f == "sym.rs")
        .expect("interp/sym.rs")
        .1;
    assert!(
        sym.contains("entry.plain = !(sealed"),
        "DRIFT: a sealed run's entries are not plain"
    );
}

/// S2 leftover, v0.22 taint_tests.rs:2427 (loop findings 0, 1, 11, 13:
/// `temporal_new` read the process clock from an arm classed Pure). Every arm
/// of a builtin classed Pure is scanned: it may reach no ambient state (the
/// clock, the host seam, the RNG, either kernel), and no interpreter state
/// except the two it is allowed: `call_closure_arg` (main's `call_cb`, which
/// carries the callbacks' taint) and `dict_mutated` (the dict edge's dirty
/// flag).
#[test]
fn no_arm_of_a_pure_builtin_reads_or_advances_ambient_state() {
    use super::taint::{info, Class};
    const FORBIDDEN: &[&str] = &[
        "program_now_ms",
        "crate::clock",
        "with_host",
        "rng_next",
        "rng_reseed",
        "self.k()",
        "self.kernels",
        "std::env",
        "std::fs",
        "std::process",
        "std::time",
    ];
    // `fold_leaf` (main only): `arr_fold`'s register fast path runs the same
    // callback `call_closure_arg` would, on a compiled body - none exists in a
    // sealed run (VmPin-3), so there it always declines.
    const ALLOWED_SELF: &[&str] = &[
        "self.call_closure_arg",
        "self.dict_mutated",
        "self.fold_leaf",
    ];
    let sources = vm::tests::r51_interp_sources();
    let src = &sources
        .iter()
        .find(|(f, _)| f == "builtins.rs")
        .expect("interp/builtins.rs")
        .1;
    let mut scanned = 0;
    let mut bad: Vec<String> = Vec::new();
    for (names, body) in super::tests::builtin_arms(src) {
        if !names
            .iter()
            .any(|n| info(n).is_some_and(|i| i.class == Class::Pure))
        {
            continue;
        }
        scanned += 1;
        if names
            .iter()
            .any(|n| info(n).is_some_and(|i| i.class != Class::Pure))
        {
            bad.push(format!(
                "{names:?}: one arm serves a Pure and a non-Pure builtin"
            ));
        }
        for line in body.lines() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            for f in FORBIDDEN {
                if t.contains(f) {
                    bad.push(format!("{names:?}: `{f}` in `{t}`"));
                }
            }
            let mut rest = t;
            while let Some(i) = rest.find("self.") {
                let tail = &rest[i..];
                let word: String = tail
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
                    .collect();
                if !ALLOWED_SELF.iter().any(|a| word.starts_with(a)) {
                    bad.push(format!(
                        "{names:?}: `{word}` reads interpreter state in a Pure arm"
                    ));
                }
                rest = &tail[4..];
            }
        }
    }
    assert!(scanned >= 20, "DRIFT: only {scanned} Pure arms found");
    assert!(
        bad.is_empty(),
        "ATTACK: DRIFT: a Pure-classed builtin touches ambient state (classify it World):\n{}",
        bad.join("\n")
    );
}
