//! Tree-walking interpreter over the typed AST.
//!
//! This is the codegen-free execution path: it runs a parsed [`Program`]
//! directly, with no dependency on `inkwell`/LLVM. It exists so `axon run`
//! works end-to-end (and the Phase-10 `goal.md → goal.ax → result` story is
//! runnable) while the native codegen build is slow/blocked.
//!
//! For an LLM-orchestration workload the hot path is network latency, not
//! arithmetic, so a tree-walker is the appropriate execution model — native
//! codegen is a later performance concern, not a prerequisite.
//!
//! ## Scope (M1)
//! Implements the full deterministic core: scalars, arithmetic, `if`/`match`,
//! `let`/assignment, `while`/`while let`/`for`, user functions + recursion,
//! closures (lambdas with capture), structs, enum ADTs, `Option`/`Result`,
//! arrays, `?` propagation, string interpolation, impl-method dispatch, and a
//! broad builtin set (I/O, conversion, math, `str_*`, `assert*`).
//!
//! The ASI builtins (`ai_complete`, `ai_extract_*`, `goal_run`, the
//! `uncertain_*`/`temporal_*` family) are stubbed with a clear runtime error —
//! wiring them to `axon-ai`/`axon-rt` is M2.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::rc::Rc;

use crate::ast::{
    BinOp, EnumDef, Expr, FnDef, ImplBlock, Item, Literal, Pattern, Program, Stmt, TypeDef, UnaryOp,
};

// ── Runtime values ────────────────────────────────────────────────────────────

/// A runtime value produced by evaluating an expression.
#[derive(Debug, Clone)]
pub enum Value {
    /// Default i64 integer (I-7: integers default to `i64`).
    Int(i64),
    /// R19 Slice B — a width-aware integer value for non-i64 fixed-width types.
    /// Stores the value as i64 internally but carries its declared type so ops
    /// can apply width-correct masks (u8=0xFF, u16=0xFFFF, …). Only non-i64
    /// integer widths use this variant; `Int(i64)` remains the default, keeping
    /// the ~102 builtin `Int` sites untouched (blast-radius isolation, spec §11).
    SizedInt {
        val: i64,
        /// One of: I8/I16/I32/U8/U16/U32/U64 — never I64 (that stays as Int).
        ty: crate::types::Type,
    },
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    Array(Vec<Value>),
    /// Structural record: `Point { x, y }`.
    Struct {
        name: String,
        fields: HashMap<String, Value>,
    },
    /// Enum variant: `Shape::Circle { radius }`.
    Enum {
        enum_name: String,
        variant: String,
        fields: HashMap<String, Value>,
    },
    Some(Box<Value>),
    None,
    Ok(Box<Value>),
    Err(Box<Value>),
    /// A lambda plus the environment it captured at creation time.
    Closure {
        params: Vec<String>,
        body: Box<Expr>,
        captured: HashMap<String, Value>,
    },
    /// A channel — a shared FIFO queue. Cloning shares the same channel (Rc), so
    /// a `spawn`ed body and the main flow see the same queue. The interpreter is
    /// cooperative/single-threaded: `spawn` runs eagerly, so a `send` happens
    /// before the matching `recv`.
    Chan(Rc<RefCell<VecDeque<Value>>>),
    /// Tuple value `(a, b, …)`. Accessed via `t.0`, `t.1` (numeric field).
    Tuple(Vec<Value>),
    /// String-keyed dictionary — the ASI workhorse for caches, frequency
    /// tables, named state. Mutating builtins (`dict_set`, `dict_remove`)
    /// share the inner `RefCell` so a stored handle stays in sync with
    /// the live state, matching the channel model. Keys are `str` only
    /// (not arbitrary `Value`) — covers 95% of ASI use cases without
    /// requiring `Hash + Eq` on the full Value enum.
    Dict(Rc<RefCell<std::collections::BTreeMap<String, Value>>>),
    /// R13 native FFI: an opaque, affine native `Handle` = `{tag, payload}`
    /// where `payload` indexes a per-module handle table — NEVER a raw pointer,
    /// so Axon cannot forge a native pointer (I-4/I-11). `module`/`name` give
    /// nominal identity (`gfx::Window` ≠ `gfx::Surface`); `resource` marks an
    /// affine handle (consumed by a consuming native fn). The handle is opaque
    /// at the surface — no field access, no arithmetic (E1803).
    Handle {
        module: String,
        name: String,
        payload: i64,
        resource: bool,
    },
}

impl Value {
    fn type_name(&self) -> String {
        match self {
            Value::Int(_) => "i64".into(),
            Value::SizedInt { ty, .. } => ty.display(),
            Value::Float(_) => "f64".into(),
            Value::Bool(_) => "bool".into(),
            Value::Str(_) => "str".into(),
            Value::Unit => "()".into(),
            Value::Array(_) => "[]".into(),
            Value::Struct { name, .. } => name.clone(),
            Value::Enum { enum_name, .. } => enum_name.clone(),
            Value::Some(_) | Value::None => "Option".into(),
            Value::Ok(_) | Value::Err(_) => "Result".into(),
            Value::Closure { .. } => "fn".into(),
            Value::Chan(_) => "chan".into(),
            Value::Tuple(_) => "tuple".into(),
            Value::Dict(_) => "dict".into(),
            Value::Handle { module, name, .. } => format!("{module}::{name}"),
        }
    }
}

// ── Non-local control flow ──────────────────────────────────────────────────

/// A non-`Ok` outcome of evaluation. Normal values flow as `Ok(Value)`; these
/// are the ways evaluation can stop short of producing a value in-place.
#[derive(Debug)]
pub enum Flow {
    /// `return <expr>` — unwind to the enclosing function boundary.
    Return(Value),
    /// `break` — exit the nearest loop.
    Break,
    /// `continue` — skip to the next loop iteration.
    Continue,
    /// A runtime panic (failed assert, type error, OOB index, …).
    Panic(String),
    /// An `@[verify]` / deploy-gate rejection — a *policy* failure (the artifact
    /// didn't meet its declared bound), distinct from a bug-crash. Mapped to a
    /// dedicated exit code (3) so CI can branch on "verification failed" vs "the
    /// program crashed" (BUG_HUNT #26).
    VerifyFailed(String),
    /// An `@[corrigible]` fn was called while the corrigibility latch was
    /// tripped (`corrigible_halt()`). The call is *refused* — the body never
    /// runs — and the latch never clears. A distinct flow (and exit code 4) so
    /// CI / a supervisor can tell "the kill-switch caught this" apart from a
    /// crash (101), a policy reject (3), or a static error (2). (R9)
    Halted(String),
    /// An AI-policy condition that stops the program but is NOT a crash: an
    /// `ai_*` call can't run because no model is reachable and no
    /// `@[ai(policy(fallback: …))]` is declared (E1300), the per-fn AI call
    /// budget is exhausted (E1301), or an unknown tier name is configured
    /// (E1302). These are user-actionable *policy/environment* mismatches with
    /// a clear fix in the message — not bugs like overflow/div0/OOB. A distinct
    /// flow (and exit code 5) so a supervisor can branch on "AI policy needs
    /// attention" specifically, exactly as @[verify]→3 and @[corrigible]→4 are
    /// carved out of the generic panic (101).
    AiPolicyUnreachable(String),
    /// `exit(code)` — terminate the process with `code`.
    Exit(i32),
    /// Phase 6: `resume(v)` inside an effect-handler arm — carries the value the
    /// handled operation should yield so the handled computation continues with
    /// it (tail-resumptive, single-shot). Raised by evaluating a `resume(..)`
    /// call and caught at the builtin-interception site that invoked the arm. If
    /// it escapes to a function/loop/top-level boundary, that is a `resume`
    /// outside a handler arm — treated as a panic (it should have been caught).
    Resume(Value),
    /// Phase 6 (multi-shot): a non-tail / multi-resume handler arm finished with
    /// `value` as the result of the whole `with` block. Unlike single-shot tail
    /// resume (which continues the suspended body via `Ok(Some(v))`), the replay
    /// path reifies the continuation by re-running the body, so the original
    /// suspended body is abandoned and its block value is `value`. Caught only by
    /// `eval_with_handler`; if it escapes, that is an interpreter bug.
    HandlerDone(Value),
    /// Phase 6 (multi-shot): a handler arm tried to resume more than once (or
    /// resume non-tail) over a body that performs effects beyond the single
    /// intercepted operation — the replay-based continuation cannot soundly
    /// re-fire those effects. Surfaced as E1314 and mapped to a panic-class exit
    /// (the program is asking for true delimited continuations, which are
    /// deferred). Carries an explanatory message.
    MultiShotUnsound(String),
    /// Phase 5: a refinement-type PRECONDITION was violated at runtime — a value
    /// passed to a parameter `p: T where P` failed `P` when `_` was bound to it.
    /// The checker discharges this statically for constant args (E1209); for a
    /// non-constant arg the predicate becomes a runtime check (the spec's
    /// Z3-free `--proof-timeout 0` fallback). A distinct flow (exit code 6) so a
    /// supervisor can tell a caller's precondition breach apart from a @[verify]
    /// postcondition (3), a kill-switch (4), an ai-policy stop (5), and a generic
    /// bug-panic (101).
    RefineViolation(String),
    /// R12b: a kernel `Goal` exhausted its principal's budget mid-run. Not a
    /// crash — the goal hit the spend ceiling its principal was granted — so a
    /// distinct flow (exit code 7) lets a supervisor branch on "goal ran out of
    /// budget" apart from a @[verify] (3), kill-switch (4), ai-policy (5),
    /// refinement (6), and a generic panic (101). The partial best is preserved
    /// (queryable via `kernel_goal_best_score`). See R12b-kernel-goal.md (E1604).
    GoalBudgetExhausted(String),
    /// F5 (Phase 9): a builtin tried to perform an effect outside the ceiling
    /// declared by the active `Sandbox<P>` — e.g. an AI-emitted tool attempted
    /// `ai_complete` (Net effect) when the sandbox only permits `FS`. A distinct
    /// flow (exit code 8) so a supervisor can tell "the sandbox caught this" apart
    /// from a genuine crash (101), a policy rejection (3), or the kill-switch (4).
    SandboxViolation(String),
}

/// Process exit code for an `@[verify]` / deploy-gate rejection. Distinct from
/// 101 (genuine panic) and 2 (static check error) so pipelines can branch on a
/// policy rejection specifically (BUG_HUNT #26).
pub const VERIFY_FAILED_EXIT_CODE: i32 = 3;

/// Process exit code when an `@[corrigible]` call is refused by the tripped
/// corrigibility latch. Distinct from 101 (panic), 3 (verify), and 2 (static)
/// so a supervisor can branch on "the kill-switch fired" specifically. (R9)
pub const HALTED_EXIT_CODE: i32 = 4;

/// Process exit code for an AI-policy condition (E1300/E1301/E1302) — offline
/// with no fallback, AI budget exhausted, or an unknown tier. A user-actionable
/// policy/environment mismatch, not a crash; distinct from 101 (panic), 3
/// (verify), 4 (corrigible halt), and 2 (static) so a supervisor can branch on
/// "AI policy needs attention" specifically.
pub const AI_POLICY_EXIT_CODE: i32 = 5;

/// Process exit code for a runtime refinement-precondition violation — a
/// non-constant argument failed a parameter's `where` predicate. Distinct from
/// 101 (panic), 3 (verify postcondition), 4 (corrigible), 5 (ai-policy), and 2
/// (static) so a supervisor can branch on "a caller passed an out-of-contract
/// value" specifically. The spec's Z3-free runtime-check fallback (Phase-5 §4).
pub const REFINE_VIOLATION_EXIT_CODE: i32 = 6;

/// Process exit code when a kernel `Goal` exhausts its principal's budget mid-run
/// (R12b / E1604). Distinct from 101 (panic), 6 (refinement), 5 (ai-policy), 4
/// (corrigible), 3 (verify), 2 (static) so a supervisor can branch on "goal out
/// of budget" specifically. VERIFIED free in the exit-code table.
pub const GOAL_BUDGET_EXIT_CODE: i32 = 7;

/// Process exit code when `sandbox_run` catches a builtin attempting an effect
/// outside the sandbox's declared ceiling (F5 / Phase 9). Distinct from 101
/// (crash), 7 (goal-budget), 6 (refinement), 5 (ai-policy), 4 (kill-switch),
/// 3 (verify), 2 (static) so a supervisor can branch on "the sandbox caught this"
/// specifically. The tool call is refused; the sandbox and the enclosing program
/// continue with an `Err` result.
pub const SANDBOX_VIOLATION_EXIT_CODE: i32 = 8;

type R = Result<Value, Flow>;

fn panic<T>(msg: impl Into<String>) -> Result<T, Flow> {
    Err(Flow::Panic(msg.into()))
}

/// Like `panic`, but for AI-policy conditions (E1300/E1301/E1302) that should
/// stop the program with the distinct [`AI_POLICY_EXIT_CODE`] (5) rather than
/// the generic crash code (101) — see [`Flow::AiPolicyUnreachable`].
fn ai_policy_err<T>(msg: impl Into<String>) -> Result<T, Flow> {
    Err(Flow::AiPolicyUnreachable(msg.into()))
}

// ── Lexical environment ──────────────────────────────────────────────────────

/// A stack of lexical scopes. Innermost scope is last.
struct Env {
    scopes: Vec<HashMap<String, Value>>,
}

impl Env {
    fn new() -> Self {
        Env {
            scopes: vec![HashMap::new()],
        }
    }
    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop(&mut self) {
        self.scopes.pop();
    }
    fn define(&mut self, name: String, val: Value) {
        self.scopes.last_mut().unwrap().insert(name, val);
    }
    fn get(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }
    /// Update the nearest existing binding; returns false if none exists.
    fn assign(&mut self, name: &str, val: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), val);
                return true;
            }
        }
        false
    }
    /// Mutable reference to the nearest existing binding (for place assignment).
    fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name))
    }
    /// Flatten all visible bindings into one map (inner shadows outer).
    /// Used to snapshot the environment a closure captures.
    fn snapshot(&self) -> HashMap<String, Value> {
        let mut out = HashMap::new();
        for scope in &self.scopes {
            for (k, v) in scope {
                out.insert(k.clone(), v.clone());
            }
        }
        out
    }
    /// Build an env whose single base scope is a captured snapshot — used to
    /// run a closure/handler-arm body in its defining environment.
    fn from_snapshot(captured: HashMap<String, Value>) -> Self {
        Env {
            scopes: vec![captured],
        }
    }
}

// ── Sandbox state ────────────────────────────────────────────────────────────

/// F5 (Phase 9): one live sandbox entry — a named principal + the concrete set
/// of effects it allows at runtime. Created by `sandbox_create`; enforced by
/// `call_builtin` whenever `active_sandbox` points at this entry.
struct SandboxEntry {
    /// The principal handle this sandbox is scoped to (for audit attribution).
    principal: i64,
    /// The concrete effects this sandbox permits (e.g. {"AI", "Net"}).
    allowed: std::collections::HashSet<String>,
}

// ── Interpreter ──────────────────────────────────────────────────────────────

pub struct Interp<'p> {
    fns: HashMap<String, &'p FnDef>,
    #[allow(dead_code)]
    structs: HashMap<String, &'p TypeDef>,
    #[allow(dead_code)]
    enums: HashMap<String, &'p EnumDef>,
    /// `(type_name, method_name) → method def` from impl blocks.
    methods: HashMap<(String, String), &'p FnDef>,
    /// Module-level `let NAME = …` constant definitions, in source order.
    global_defs: Vec<(String, &'p Expr)>,
    /// Evaluated module-level constants (populated by [`Interp::init_globals`]).
    globals: HashMap<String, Value>,
    /// In-memory provenance store: `@[adaptive]` fn name → recorded return
    /// scores, in call order. Read by `goal_run` (mirrors `axon-rt`'s store).
    provenance: RefCell<HashMap<String, Vec<f64>>>,
    /// Per-call i64-prefix input tuple, in lock-step with `provenance`. The
    /// vec collects every leading i64 arg the fn took (length = how many
    /// of the fn's first args were i64). Empty when none were. Read by
    /// `goal_best_input` (returns the first dim) and `goal_best_inputs`
    /// (returns the full tuple), and used by the multi-arg coordinate-
    /// descent hill-climb to seed the next sweep.
    provenance_inputs: RefCell<HashMap<String, Vec<Vec<i64>>>>,
    /// Per-call f64-prefix input tuple, mirror of `provenance_inputs` for
    /// `@[adaptive] fn(f64, …) -> f64`. Read by `goal_best_input_f64` /
    /// `goal_best_inputs_f64`. Lets the optimizer cover continuous-domain
    /// problems (linear-regression weights, control parameters, etc.)
    /// without forcing the user to discretize via integer indices.
    provenance_inputs_f64: RefCell<HashMap<String, Vec<Vec<f64>>>>,
    /// Current call-stack depth, bounded by `max_depth` so runaway recursion
    /// fails with a catchable panic rather than overflowing the (large but
    /// finite) interpreter thread stack and aborting the process.
    call_depth: Cell<usize>,
    /// Effective recursion ceiling for this run — `RECURSION_LIMIT` by default,
    /// or `AXON_MAX_DEPTH` (clamped) when set. Resolved once at build time so
    /// every `call_fn` sees a consistent value.
    max_depth: usize,
    /// R9 corrigibility latch. `corrigible_halt()` sets this to `true`; once
    /// set it never clears (there is intentionally no resume builtin). While
    /// set, every call to an `@[corrigible]` fn is refused — its body never
    /// runs — so the system cannot resist or reverse its own shutdown. A
    /// one-way latch is the whole safety property: a kill-switch you can turn
    /// back off is not a kill-switch.
    corrigible_halted: Cell<bool>,
    /// Name of the Axon function currently executing, for attributing builtin
    /// side effects (e.g. R3's `ai_call` provenance records) to their caller.
    /// Set on entry to `call_fn`, restored on exit. Empty at top level.
    current_fn: RefCell<String>,
    /// R4/I-13 — the nearest ENCLOSING `@[agent]` fn on the call stack (not just
    /// the immediate fn). Set when entering an `@[agent]` fn and INHERITED through
    /// non-agent helpers, so a capability builtin called inside a helper of an
    /// agent is still logged to the agent's action trail (the un-opt-out-able
    /// audit can't be escaped by wrapping the I/O one call away). `None` outside
    /// any agent.
    enclosing_agent: RefCell<Option<String>>,
    /// F3: the goal/metric name currently being optimized by a `run_goal*` call,
    /// set for the duration of the optimization so an `ai_complete` invoked inside
    /// a metric evaluation can stamp the CAUSAL goal that triggered it into the
    /// audit trail (`axon trace --ai` cost-attribution per goal). `None` outside
    /// any goal optimization.
    current_goal: RefCell<Option<String>>,
    /// F3 (Phase 9): the name of the principal currently in scope for audit
    /// attribution. Set via `principal_activate(handle)` to associate a kernel
    /// principal with the execution context, so capability audit records carry the
    /// principal name rather than the opaque "root" default. Defaults to "root".
    current_principal: RefCell<String>,
    /// Active `subject_to` constraint fn name during `goal_run_constrained`
    /// (pillar-3 constrained search). When set, the optimizer scores an
    /// INFEASIBLE candidate as maximally distant so it is rejected; the real
    /// score is still recorded in provenance. `None` outside a constrained goal
    /// (so plain `goal_run` is byte-identical). See `apply_goal_constraint`.
    goal_constraint: RefCell<Option<String>>,
    /// Per-call AI tier from a `tier:` named arg (R3b), set by `eval_call` for
    /// the duration of a single builtin dispatch. `ai_complete`'s tier
    /// resolution reads this first (step 1: per-call > policy > default).
    current_call_tier: RefCell<Option<String>>,
    /// R3c: count of `ai_complete` calls made by the current fn activation, used
    /// to enforce `@[ai(policy(budget: N))]`. Reset on entry to `call_fn`,
    /// restored on exit (so the budget is per-activation, not global).
    ai_calls_this_fn: Cell<u64>,
    /// Phase-7 `cost_meter` / F4: cumulative AI spend across the whole run, in
    /// integer micro-dollars (µ$). Every `ai_complete` adds `tier.cost_micro(est
    /// tokens)` — the real per-token cost, stamped into the `ai_call` provenance
    /// (replacing the hardcoded 0). Read by the `ai_cost_spent()` builtin. This
    /// is per-TOKEN cost, distinct from R3c's per-CALL-count budget.
    ai_cost_micro: Cell<i64>,
    /// Phase 6: the stack of active effect-handler frames installed by enclosing
    /// `with handler { … } { body }` expressions. When a builtin carrying effect
    /// `E` is dispatched, the nearest frame with an `on E` arm intercepts it
    /// (tail-resumptive, single-shot). Pushed/popped around the handled body in
    /// `eval` of `Expr::WithHandler`. Interior-mutable like the other per-run
    /// state above.
    handlers: RefCell<Vec<HandlerFrame>>,
    /// Phase 6 (multi-shot resume): the active replay continuation, if the
    /// interpreter is currently re-running a handler's body to service a
    /// `resume(v)` call. `None` in the common case (no replay in flight). When
    /// `Some`, the builtin-interception site consumes one feed value for the
    /// handled effect instead of re-entering the arm (so the body runs straight
    /// through to its value), and flags any OTHER effect as E1314-unsound (a
    /// replay cannot re-fire side effects). See [`ResumeReplay`].
    resume_replay: RefCell<Option<ResumeReplay>>,
    /// Phase 6 (multi-shot resume): the stack of body+env contexts a handler arm
    /// is currently handling, so a `resume(v)` evaluated inside the arm knows
    /// WHICH suspended computation to replay. Pushed in `run_handler_arm` before
    /// the arm body runs, popped after. The top entry is the innermost handled
    /// operation. `resume(v)` replays `body` (the handled `with`-block body) with
    /// `v` fed at the intercepted op and returns the continuation's value.
    resume_ctx: RefCell<Vec<ResumeCtx>>,
    /// Phase 7 (R12 Slice 1): the live principal-authority registry. The
    /// `principal_*` builtins mint/spend/authorize against it, so attenuation is
    /// enforced by the KERNEL (the registry), not just as userland values. A
    /// handle is a plain `i64` index. Empty until a program mints a root.
    principals: RefCell<crate::kernel::PrincipalRegistry>,
    /// Phase 7 (R12 Slice 2): the cooperative fiber scheduler. `scheduler_spawn`
    /// queues a (named fn, arg) fiber; `scheduler_run` runs the ready fibers in a
    /// seed-deterministic round-robin, catching a panicking fiber (recorded as
    /// failed, not a process abort). The interpreter owns the run loop (it has
    /// `call_fn`); the queue + ordering live in `kernel::Scheduler`.
    scheduler: RefCell<crate::kernel::Scheduler>,
    /// Phase 7 (R12 Slice 3): live supervisors, indexed by handle. Each oversees
    /// an ordered set of scheduler fibers and, when one fails, restarts the set
    /// its OTP strategy dictates — latching a halt (exit 4) on a crash loop.
    supervisors: RefCell<Vec<crate::kernel::Supervisor>>,
    /// Phase 7 (R12 Slice 4): durable stores, indexed by handle. Each is an
    /// in-memory `kernel::Store` (rebuilt by replaying its NDJSON log on open)
    /// plus the log path it appends applied ops to, so its value survives a fresh
    /// process and a retried op_id dedups cross-process under linearizable.
    stores: RefCell<Vec<(crate::kernel::Store, std::path::PathBuf)>>,
    /// Phase 7 (R12 Slice 5): principal-scoped LLM gateways, indexed by handle.
    /// Each mediates AI calls with per-token cost metering debited from its
    /// principal's budget (Slice 1), degrading to a fallback + latch on overrun.
    llm_gateways: RefCell<Vec<crate::kernel::LlmGateway>>,
    /// Phase 7 (R12b): principal-scoped `KernelGoal`s, indexed by handle. Each
    /// runs the existing optimizer (`run_goal`) scoped to a Slice-1 principal's
    /// budget, refusing to exceed it (E1604, exit 7). See R12b-kernel-goal.md.
    goals: RefCell<Vec<crate::kernel::KernelGoal>>,
    /// F5 (Phase 9): registered sandboxes, indexed by handle (0-based). Created
    /// by `sandbox_create`; the handle is the index into this vec.
    sandboxes: RefCell<Vec<SandboxEntry>>,
    /// F5 (Phase 9): the handle of the currently active sandbox (-1 = none).
    /// `sandbox_run` sets this before calling the user fn and restores it after.
    /// `call_builtin` reads it to gate effectful builtins.
    active_sandbox: Cell<i64>,
    /// Phase 5: named refinement → its predicate Expr (binder `_`). Collected
    /// from `RefineDef` items (inline `where` on a param desugars to a synthetic
    /// named refinement during parsing). Drives the runtime precondition check in
    /// `call_fn`: when a parameter's type is one of these, the predicate is
    /// evaluated with `_` bound to the argument and a violation raises
    /// [`Flow::RefineViolation`]. Empty when the program has no refinements.
    refine_preds: HashMap<String, &'p Expr>,
    /// Phase 5 §4: obligations an SMT prover discharged for ALL inputs, so the
    /// matching runtime check is provably dead and may be elided. Empty by
    /// default (and always, unless `Interp::with_discharged` is used by a
    /// pipeline built with the `smt` feature), keeping the default run path
    /// byte-identical to pre-discharge behaviour.
    discharged: crate::verify::Discharged,
    /// R13 native FFI: the GPU-FREE `native::gfx` mock's in-process state — a
    /// per-module handle table + a frame counter. The shim is in-memory (no
    /// wgpu/winit/GPU); this is the interp-side realization of the dual-engine
    /// boundary (the codegen side links the `axon-rt` mock symbols, byte-
    /// identical observable behaviour for the value-returning calls).
    gfx_mock: RefCell<crate::native::GfxMock>,
}

/// One active effect-handler frame: the inline-handler arms in scope for the
/// body it wraps. Each arm intercepts one effect name. Captured at the `with`
/// site so the arm body closes over its defining environment.
struct HandlerFrame {
    /// `on E(binding) => body` arms, keyed by the effect name `E`.
    arms: Vec<HandlerArmRt>,
    /// Phase 6 (multi-shot resume): the `with`-block body this frame wraps, and
    /// the environment snapshot it ran in — so a non-tail / multi-shot arm can
    /// REPLAY the continuation (re-run the body, feeding the resume value at the
    /// intercepted op). Unused by the bare-tail-resume fast path.
    body: crate::ast::Expr,
    env_snapshot: HashMap<String, Value>,
}

/// A runtime handler arm: the payload binding, the arm body, and a snapshot of
/// the environment where the handler was written (so the arm closes over it).
struct HandlerArmRt {
    effect: String,
    binding: crate::ast::Pattern,
    body: crate::ast::Expr,
    captured: HashMap<String, Value>,
}

/// Phase 6 (multi-shot resume): state for one in-flight continuation replay. A
/// `resume(v)` in a handler arm reifies "the rest of the body after the
/// intercepted op" by RE-RUNNING the body from the top, with `v` fed at the
/// effect site instead of re-entering the handler. This makes the continuation a
/// first-class, repeatedly-callable thing in a tree-walking interpreter without
/// a CPS rewrite — the arm may `resume` as many times as it likes (multi-shot).
///
/// Soundness boundary: a replay that re-encounters ANY effect (the handled one a
/// second time, or a different effect) cannot soundly re-execute it — the side
/// effect already happened on the original pass. The first feed is the resume
/// value; a second effect hit during the same replay is E1314
/// ([`Flow::MultiShotUnsound`]). So the supported multi-shot subset is "a body
/// that performs exactly one effect, then is pure" — retries, backtracking
/// search, and `a + b` over two resumes all fit; an effect-after-resume does not.
struct ResumeReplay {
    /// The handled effect name this replay feeds (only this effect is fed).
    effect: String,
    /// The value to yield at the (single) intercepted op during this replay.
    feed: Value,
    /// Whether the feed has been consumed yet (the first hit consumes it; a
    /// second effect hit in the same replay is the unsound case → E1314).
    consumed: bool,
}

/// Phase 6 (multi-shot resume): the suspended computation a handler arm is
/// servicing — the `with`-block body and the environment it ran in. A
/// `resume(v)` evaluated in the arm replays this body (feeding `v` at the
/// intercepted op) and returns the continuation's value to the arm, so the arm
/// can resume again (multi-shot). Cloned cheaply (the body is an `Expr` already
/// owned by the AST clone in `HandlerArmRt`).
#[derive(Clone)]
struct ResumeCtx {
    /// The handled effect name (the op the body performs once, fed by resume).
    effect: String,
    /// The `with`-block body to replay on each resume.
    body: crate::ast::Expr,
    /// The environment snapshot the body originally evaluated in.
    env_snapshot: HashMap<String, Value>,
}

/// Default max interpreter call depth before a graceful "recursion limit"
/// panic. The debug-build `eval` frame is large (~128 KB/call), so a 1 GiB
/// thread stack overflows around ~8000 frames; this limit fires first, turning
/// runaway recursion into a catchable panic instead of a process-aborting
/// overflow. Overridable via `AXON_MAX_DEPTH` — see [`resolve_max_depth`].
#[cfg(not(target_arch = "wasm32"))]
const RECURSION_LIMIT: usize = 6_000;

/// On wasm32 the interpreter runs on the single linear-memory stack (no OS
/// thread to size up — see [`on_deep_stack`]), so the recursion guard must
/// trip before that stack overflows or a deep recursion *traps* the module
/// instead of producing the graceful "recursion limit" panic native gives.
/// With the 64 MiB wasm stack the build sets (`.cargo/config.toml`
/// `-zstack-size`) the empirical overflow boundary is ~700 interpreter frames;
/// 450 leaves a comfortable margin so the failure is the same observable,
/// catchable panic as native (R7 §4.3 / BUG_HUNT #28). This is the bounded,
/// documented host divergence: deep recursion fails the same *way*, at a lower
/// *depth*, on wasm.
#[cfg(target_arch = "wasm32")]
const RECURSION_LIMIT: usize = 450;

/// Hard ceiling on the configurable recursion limit. `AXON_MAX_DEPTH` is
/// clamped to this so a user can't set a value so high the native stack
/// overflows *before* the guard fires (which would reintroduce the very
/// process-abort the guard exists to prevent). Paired with the stack-size
/// scaling in [`stack_size_for_depth`]: at this ceiling the thread stack is
/// ~8 GiB, leaving ample headroom over the ~256 KB/frame worst case.
const MAX_DEPTH_CEILING: usize = 1_000_000;

/// Native stack budget per interpreter call frame, used to size the thread
/// stack so the [`resolve_max_depth`] guard always trips before a real
/// overflow. Generous (2×) over the observed ~128 KB debug frame.
const STACK_BYTES_PER_FRAME: usize = 256 * 1024;

/// Minimum interpreter thread stack — the historical 1 GiB floor, so shallow
/// runs keep their previous generous headroom regardless of the depth setting.
const MIN_STACK_BYTES: usize = 1024 * 1024 * 1024;

/// Resolve the effective recursion limit: `AXON_MAX_DEPTH` if set to a positive
/// integer (clamped to [`MAX_DEPTH_CEILING`]), else [`RECURSION_LIMIT`]. A
/// malformed or zero value falls back to the default rather than failing the
/// run — the env var is a convenience lever, not load-bearing.
fn resolve_max_depth() -> usize {
    max_depth_from_env(std::env::var("AXON_MAX_DEPTH").ok().as_deref())
}

/// Pure core of [`resolve_max_depth`]: maps an optional raw env value to the
/// effective ceiling. Split out so the clamping/fallback logic is unit-testable
/// without mutating process-global environment state.
fn max_depth_from_env(raw: Option<&str>) -> usize {
    match raw {
        Some(s) => match s.trim().parse::<usize>() {
            Ok(n) if n > 0 => n.min(MAX_DEPTH_CEILING),
            _ => RECURSION_LIMIT,
        },
        None => RECURSION_LIMIT,
    }
}

/// Thread stack size that keeps the recursion guard ahead of a real overflow
/// for the given depth: `depth × per-frame budget`, floored at the historical
/// 1 GiB so we never shrink below the previous default. Unused on wasm32 (no
/// OS threads — `on_deep_stack` runs on the single main stack there).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn stack_size_for_depth(depth: usize) -> usize {
    depth
        .saturating_mul(STACK_BYTES_PER_FRAME)
        .max(MIN_STACK_BYTES)
}

/// Decrements the call-depth counter when a `call_fn` frame unwinds (any path).
struct DepthGuard<'a>(&'a Cell<usize>);
impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

/// Saves the caller's `current_fn` and restores it on drop, so builtin side
/// effects (R3 `ai_call` provenance) are attributed to the nearest enclosing
/// Axon function even across nested calls.
struct FnNameGuard<'a> {
    cell: &'a RefCell<String>,
    prev: String,
}
impl Drop for FnNameGuard<'_> {
    fn drop(&mut self) {
        *self.cell.borrow_mut() = std::mem::take(&mut self.prev);
    }
}

/// Like `FnNameGuard` but for an `Option<String>` cell — used for the
/// `enclosing_agent` save/restore (R4/I-13 transitive agent attribution).
struct FnNameOptGuard<'a> {
    cell: &'a RefCell<Option<String>>,
    prev: Option<String>,
}
impl Drop for FnNameOptGuard<'_> {
    fn drop(&mut self) {
        *self.cell.borrow_mut() = self.prev.take();
    }
}

/// R3c: saves the caller's `ai_calls_this_fn` count and restores it on drop, so
/// each fn activation meters its own `ai_complete` calls against its own
/// `@[ai(policy(budget))]` (the budget is per-activation, not global).
struct AiBudgetGuard<'a> {
    cell: &'a Cell<u64>,
    prev: u64,
}
impl Drop for AiBudgetGuard<'_> {
    fn drop(&mut self) {
        self.cell.set(self.prev);
    }
}

/// Run `f` on a thread with a large stack. The tree-walking interpreter uses a
/// lot of native stack per call, so an 8 MB main stack overflows at only a few
/// hundred frames; this lets reasonably deep recursion run, while the
/// `RECURSION_LIMIT` guard backstops truly runaway recursion with a clean panic.
///
/// On `wasm32` there are no OS threads (`std::thread::spawn` traps with
/// "invalid stack size"), so we run `f` directly on the single wasm stack. The
/// `RECURSION_LIMIT` / `AXON_MAX_DEPTH` guard still fires its graceful panic
/// before a wasm stack overflow, so deep recursion fails the same observable
/// way as native — only the (impossible-on-wasm) extra thread is dropped.
/// This is R7 §4.3: the one host touchpoint that must change for the
/// interpreter to run identically in the browser / under wasmtime.
#[cfg(not(target_arch = "wasm32"))]
fn on_deep_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    // Size the stack to the (possibly user-raised) recursion limit so the
    // RECURSION_LIMIT guard always trips before a real overflow (BUG_HUNT #28).
    let stack = stack_size_for_depth(resolve_max_depth());
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(stack)
            .spawn_scoped(s, f)
            .expect("spawn interpreter thread")
            .join()
            .expect("interpreter thread panicked")
    })
}

/// wasm32 has no OS threads — run on the single main stack. The depth guard
/// (`RECURSION_LIMIT` / `AXON_MAX_DEPTH`) still backstops runaway recursion.
#[cfg(target_arch = "wasm32")]
fn on_deep_stack<T>(f: impl FnOnce() -> T) -> T {
    f()
}

// ── In-process stdout capture (R10 G1 observable tuple) ──────────────────────
//
// The interpreter normally writes `print`/`println` straight to the process
// stdout. The R10 verification harness needs to compare a program's *observable
// output* before and after a candidate compiler pass — in-process, over a whole
// corpus, deterministically. A thread-local sink lets `run_program_capturing`
// redirect that output into a buffer without spawning a subprocess per program.
// When the sink is `None` (the normal case) output goes to real stdout, so this
// is zero-overhead and invisible to every existing run.
thread_local! {
    static OUTPUT_SINK: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Route an interpreter `print`/`println` write: into the capture buffer if one
/// is active on this thread, else to real stdout (the normal path).
fn emit_stdout(s: &str, newline: bool) {
    OUTPUT_SINK.with(|sink| {
        let mut b = sink.borrow_mut();
        if let Some(buf) = b.as_mut() {
            buf.push_str(s);
            if newline {
                buf.push('\n');
            }
        } else {
            use std::io::Write;
            let mut out = std::io::stdout();
            let _ = out.write_all(s.as_bytes());
            if newline {
                let _ = out.write_all(b"\n");
            }
            let _ = out.flush();
        }
    });
}

/// Run `program`, capturing its stdout into a buffer instead of the process
/// stdout, and return the **observable tuple** `(exit_code, stdout)`. This is
/// the R10 G1 oracle's comparison input: a candidate pass is correct iff this
/// tuple is identical for the original and transformed program on every corpus
/// member. Runs on the deep stack like `run_program`. Not thread-safe with
/// concurrent captures on the same thread (the sink is per-thread, restored on
/// return).
pub fn run_program_capturing(program: &Program) -> (i32, String) {
    on_deep_stack(|| {
        // Install a fresh capture buffer, restoring any prior one on exit.
        let prev = OUTPUT_SINK.with(|s| s.replace(Some(String::new())));
        let code = run_program_inner(program, crate::verify::Discharged::default());
        let captured = OUTPUT_SINK.with(|s| s.replace(prev)).unwrap_or_default();
        (code, captured)
    })
}

/// Parse-and-run convenience: returns the process exit code.
pub fn run_program(program: &Program) -> i32 {
    on_deep_stack(|| run_program_inner(program, crate::verify::Discharged::default()))
}

/// Phase 5 §4: run with a set of SMT-discharged obligations installed, so the
/// interpreter elides the runtime checks Z3 proved ∀-inputs. Identical to
/// [`run_program`] with an empty set.
pub fn run_program_with_discharged(
    program: &Program,
    discharged: crate::verify::Discharged,
) -> i32 {
    on_deep_stack(|| run_program_inner(program, discharged))
}

/// Phase 11: call a specific named function (no args) and return an exit code.
///
/// - Returns `None` if the function does not exist in the program (gate is skipped).
/// - Returns `Some(0)` if the function returns `true`, `0`, or `()` without panicking.
/// - Returns `Some(1)` if the function returns `false` or a non-zero `i64`.
/// - Returns `Some(exit_code)` for any runtime error (panic, verify, etc.).
///
/// Globals are initialized before calling so the function can read module-level lets.
pub fn run_named_fn_as_bool(program: &Program, fn_name: &str) -> Option<i32> {
    on_deep_stack(|| {
        let mut interp =
            Interp::build(program).with_discharged(crate::verify::Discharged::default());
        if !interp.fns.contains_key(fn_name) {
            return None;
        }
        // Initialize globals so the gate function can read module-level lets.
        if let Err(e) = interp.init_globals() {
            let code = match e {
                Flow::Exit(c) => c,
                Flow::Panic(_) => 101,
                _ => 101,
            };
            return Some(code);
        }
        let f: &FnDef = *interp.fns.get(fn_name)?;
        let result = interp.call_fn(f, vec![]);
        Some(match result {
            Ok(Value::Bool(true)) => 0,
            Ok(Value::Bool(false)) => 1,
            Ok(Value::Int(0)) => 0,
            Ok(Value::Int(_)) => 1,
            Ok(_) => 0,
            Err(Flow::Exit(c)) => c,
            Err(Flow::VerifyFailed(msg)) => {
                let _ = std::io::stdout().flush();
                eprintln!("axon: verify failed in {fn_name}: {msg}");
                VERIFY_FAILED_EXIT_CODE
            }
            Err(Flow::Halted(msg)) => {
                let _ = std::io::stdout().flush();
                eprintln!("axon: halted in {fn_name}: {msg}");
                HALTED_EXIT_CODE
            }
            Err(Flow::Panic(msg)) => {
                let _ = std::io::stdout().flush();
                eprintln!("axon: panic in {fn_name}: {msg}");
                101
            }
            Err(_) => 101,
        })
    })
}

// ── R15 resume runtime ──────────────────────────────────────────────────────────
//
// `host_await(req)` suspends the program, yields `req` to the host, and resumes
// with the host's reply. The interpreter (`Interp`/`Value`) is `!Send` (`Rc`), so
// it CANNOT cross threads — but the program can run on a worker thread that owns
// its OWN interp (created there, never moved). The worker BLOCKING on the reply
// channel is the suspension; the host (this caller's thread) regains control,
// services the request, and unblocks the worker. No `Flow` plumbing, no `unsafe`,
// no dependency — additive.
//
// SLICE 2 (this revision): the channel now carries an OWNED `Send` deep-clone of
// the payload (`SendValue`), not just `String`, so **arbitrary `Value` payloads —
// dict, struct, enum, nested arrays/options/results, tuples, closures — survive a
// suspend** on the native worker-thread substrate. This is the spec's
// safe-alternative path (governance/specs/R15-resume-runtime.md §11 Slice (2)):
// deep-clone Values to a `Send` owned form across the thread, with NO `unsafe` and
// NO new dependency — the worker-thread substrate stays. `Chan` (and any value
// transitively containing one) is identity-shared mutable state; deep-cloning it
// would silently break the sharing across the suspend, so it is REFUSED with a
// clear runtime error rather than mis-copied (the spec's "identity-shared payloads
// may be out of scope — refuse them" boundary). `Closure` IS deep-clonable (its
// `Expr` body + captures are `!Send`-free), so a fn-valued payload crosses fine.
//
// wasm has no threads, so the wasm `host_await_yield` variants read stdin / call a
// JS import on the single stack (synchronous, str-only) — those bindings serialize
// the request to its `str` form (a non-str payload there is refused).

/// An owned, `Send` deep-clone of a [`Value`], used to cross the worker-thread
/// channel that drives `host_await`. Mirrors `Value` minus the identity-shared
/// `Chan` variant: a `Chan` cannot cross (it would lose its sharing) and is refused
/// at the boundary; a `Dict` crosses as an OWNED snapshot (`Vec<(key, SendValue)>`)
/// — sound because the program retains its own `Rc` to the original dict, so what
/// the host receives is a copy to inspect, and a reply dict is freshly constructed
/// with no prior sharing to preserve.
#[derive(Debug, Clone)]
pub enum SendValue {
    Int(i64),
    SizedInt { val: i64, ty: crate::types::Type },
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    Array(Vec<SendValue>),
    Struct { name: String, fields: Vec<(String, SendValue)> },
    Enum { enum_name: String, variant: String, fields: Vec<(String, SendValue)> },
    Some(Box<SendValue>),
    None,
    Ok(Box<SendValue>),
    Err(Box<SendValue>),
    Closure { params: Vec<String>, body: Box<Expr>, captured: Vec<(String, SendValue)> },
    Tuple(Vec<SendValue>),
    /// An owned snapshot of a `Dict`'s entries (sorted key order — the source is a
    /// `BTreeMap`). Reconstructs into a fresh `Rc<RefCell<…>>` dict.
    Dict(Vec<(String, SendValue)>),
}

/// Why a `Value` could not be deep-cloned to a `SendValue` for crossing the
/// suspend boundary. Today the only case is a `Chan` (identity-shared mutable
/// state) appearing in the payload.
#[derive(Debug)]
pub struct UnsendablePayload {
    /// A human-readable path to the offending value (e.g. `.q` / `[2]`).
    pub path: String,
}

impl SendValue {
    /// Deep-clone a `Value` into an owned `Send` form. Returns `Err` if the value
    /// (transitively) contains a `Chan` — identity-shared mutable state that cannot
    /// cross a suspend without silently losing its sharing.
    pub fn from_value(v: &Value) -> Result<SendValue, UnsendablePayload> {
        Self::from_value_at(v, String::new())
    }

    fn from_value_at(v: &Value, path: String) -> Result<SendValue, UnsendablePayload> {
        fn arr(xs: &[Value], path: &str) -> Result<Vec<SendValue>, UnsendablePayload> {
            xs.iter()
                .enumerate()
                .map(|(i, x)| SendValue::from_value_at(x, format!("{path}[{i}]")))
                .collect()
        }
        fn fields(
            m: &HashMap<String, Value>,
            path: &str,
        ) -> Result<Vec<(String, SendValue)>, UnsendablePayload> {
            // Sort keys for a deterministic owned snapshot (HashMap iteration order
            // is nondeterministic; the parity round-trip must be stable).
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            keys.into_iter()
                .map(|k| Ok((k.clone(), SendValue::from_value_at(&m[k], format!("{path}.{k}"))?)))
                .collect()
        }
        Ok(match v {
            Value::Int(n) => SendValue::Int(*n),
            Value::SizedInt { val, ty } => SendValue::SizedInt { val: *val, ty: ty.clone() },
            Value::Float(f) => SendValue::Float(*f),
            Value::Bool(b) => SendValue::Bool(*b),
            Value::Str(s) => SendValue::Str(s.clone()),
            Value::Unit => SendValue::Unit,
            Value::Array(xs) => SendValue::Array(arr(xs, &path)?),
            Value::Struct { name, fields: f } => SendValue::Struct {
                name: name.clone(),
                fields: fields(f, &path)?,
            },
            Value::Enum { enum_name, variant, fields: f } => SendValue::Enum {
                enum_name: enum_name.clone(),
                variant: variant.clone(),
                fields: fields(f, &path)?,
            },
            Value::Some(b) => {
                SendValue::Some(Box::new(Self::from_value_at(b, format!("{path}.Some"))?))
            }
            Value::None => SendValue::None,
            Value::Ok(b) => SendValue::Ok(Box::new(Self::from_value_at(b, format!("{path}.Ok"))?)),
            Value::Err(b) => {
                SendValue::Err(Box::new(Self::from_value_at(b, format!("{path}.Err"))?))
            }
            Value::Closure { params, body, captured } => {
                let mut keys: Vec<&String> = captured.keys().collect();
                keys.sort();
                let cap: Result<Vec<_>, _> = keys
                    .into_iter()
                    .map(|k| {
                        Ok((
                            k.clone(),
                            Self::from_value_at(&captured[k], format!("{path}.capture[{k}]"))?,
                        ))
                    })
                    .collect();
                SendValue::Closure {
                    params: params.clone(),
                    body: body.clone(),
                    captured: cap?,
                }
            }
            Value::Tuple(xs) => SendValue::Tuple(arr(xs, &path)?),
            Value::Dict(d) => {
                let borrowed = d.borrow();
                let mut out = Vec::with_capacity(borrowed.len());
                for (k, val) in borrowed.iter() {
                    out.push((k.clone(), Self::from_value_at(val, format!("{path}.{k}"))?));
                }
                SendValue::Dict(out)
            }
            Value::Chan(_) => {
                return Err(UnsendablePayload {
                    path: if path.is_empty() { "<root>".to_string() } else { path },
                });
            }
            // R13: a native handle is identity-bound to the in-process handle
            // table (like a Chan) — it cannot cross a suspend boundary. Refuse
            // rather than corrupt (the table index would be meaningless on the
            // other side).
            Value::Handle { .. } => {
                return Err(UnsendablePayload {
                    path: if path.is_empty() { "<root>".to_string() } else { path },
                });
            }
        })
    }

    /// Reconstruct a `Value` from this owned `Send` form, allocating fresh
    /// `Rc<RefCell<…>>` cells for any `Dict` (there is no prior sharing to preserve
    /// — a reply value is freshly built by the host).
    pub fn into_value(self) -> Value {
        match self {
            SendValue::Int(n) => Value::Int(n),
            SendValue::SizedInt { val, ty } => Value::SizedInt { val, ty },
            SendValue::Float(f) => Value::Float(f),
            SendValue::Bool(b) => Value::Bool(b),
            SendValue::Str(s) => Value::Str(s),
            SendValue::Unit => Value::Unit,
            SendValue::Array(xs) => Value::Array(xs.into_iter().map(Self::into_value).collect()),
            SendValue::Struct { name, fields } => Value::Struct {
                name,
                fields: fields.into_iter().map(|(k, v)| (k, v.into_value())).collect(),
            },
            SendValue::Enum { enum_name, variant, fields } => Value::Enum {
                enum_name,
                variant,
                fields: fields.into_iter().map(|(k, v)| (k, v.into_value())).collect(),
            },
            SendValue::Some(b) => Value::Some(Box::new(b.into_value())),
            SendValue::None => Value::None,
            SendValue::Ok(b) => Value::Ok(Box::new(b.into_value())),
            SendValue::Err(b) => Value::Err(Box::new(b.into_value())),
            SendValue::Closure { params, body, captured } => Value::Closure {
                params,
                body,
                captured: captured.into_iter().map(|(k, v)| (k, v.into_value())).collect(),
            },
            SendValue::Tuple(xs) => Value::Tuple(xs.into_iter().map(Self::into_value).collect()),
            SendValue::Dict(entries) => {
                let map: std::collections::BTreeMap<String, Value> =
                    entries.into_iter().map(|(k, v)| (k, v.into_value())).collect();
                Value::Dict(Rc::new(RefCell::new(map)))
            }
        }
    }

    /// The `str` projection of a payload, for the str-only wasm bindings (stdin /
    /// JS import): a `Str` is itself; anything else is `None` (those substrates only
    /// model a text reply channel, so a non-str request there is refused).
    #[cfg(target_arch = "wasm32")]
    fn as_str_payload(&self) -> Option<&str> {
        match self {
            SendValue::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// The str-form `host_await` reply contract: EOF (`None`) → `""`; a `Str` reply →
/// itself; any other reply Value (a Value-host answering the str form) → its display
/// rendering. Used by the str-typed `host_await`/`host_await_opt` builtins.
pub(crate) fn send_reply_to_string(reply: Option<SendValue>) -> String {
    match reply {
        None => String::new(),
        Some(SendValue::Str(s)) => s,
        Some(other) => send_value_display(&other),
    }
}

/// A best-effort text rendering of a `SendValue` (shared by the str reply contract
/// and the str-host wrapper). Scalars render naturally; composites render a compact
/// form.
pub(crate) fn send_value_display(v: &SendValue) -> String {
    match v {
        SendValue::Int(n) => n.to_string(),
        SendValue::SizedInt { val, .. } => val.to_string(),
        SendValue::Float(f) => f.to_string(),
        SendValue::Bool(b) => b.to_string(),
        SendValue::Str(s) => s.clone(),
        SendValue::Unit => "()".to_string(),
        SendValue::Array(xs) | SendValue::Tuple(xs) => {
            let inner: Vec<String> = xs.iter().map(send_value_display).collect();
            format!("[{}]", inner.join(", "))
        }
        SendValue::Struct { name, fields } => {
            let inner: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", send_value_display(v)))
                .collect();
            format!("{name} {{ {} }}", inner.join(", "))
        }
        SendValue::Enum { enum_name, variant, .. } => format!("{enum_name}::{variant}"),
        SendValue::Some(b) => format!("Some({})", send_value_display(b)),
        SendValue::None => "None".to_string(),
        SendValue::Ok(b) => format!("Ok({})", send_value_display(b)),
        SendValue::Err(b) => format!("Err({})", send_value_display(b)),
        SendValue::Closure { .. } => "<fn>".to_string(),
        SendValue::Dict(entries) => {
            let inner: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{k}: {}", send_value_display(v)))
                .collect();
            format!("{{ {} }}", inner.join(", "))
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct HostChannels {
    req_tx: std::sync::mpsc::Sender<SendValue>,
    // `None` reply = end-of-input (host has no more to give) — distinct from an
    // empty-string reply (a blank line). `host_await` collapses both to ""; the
    // EOF-aware `host_await_opt` surfaces the distinction as `None`.
    rep_rx: std::sync::mpsc::Receiver<Option<SendValue>>,
}
#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static HOST_AWAIT: RefCell<Option<HostChannels>> = const { RefCell::new(None) };
}

/// Reach the active host channels from inside `host_await` (interp/builtins.rs).
/// Returns `Ok(Some(reply))`, `Ok(None)` at end-of-input, or `Err(())` if there is
/// no host at all (a bare `axon run`). NATIVE: the `SendValue` request (an owned
/// deep-clone of the program's payload — see the builtin dispatch) crosses the
/// worker→host channel; the worker BLOCKS on the reply channel (that IS the
/// suspension); the host's `SendValue` reply crosses back and is reconstructed into
/// a `Value` by the caller.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn host_await_yield(req: SendValue) -> Result<Option<SendValue>, ()> {
    HOST_AWAIT.with(|h| {
        let guard = h.borrow();
        match &*guard {
            Some(ch) => {
                ch.req_tx.send(req).map_err(|_| ())?;
                // Holding the borrow across this blocking recv is fine: nothing
                // else on THIS thread touches HOST_AWAIT while the worker is parked.
                ch.rep_rx.recv().map_err(|_| ())
            }
            None => Err(()),
        }
    })
}

/// WASI wasm (wasmtime): no threads, so there's no worker/channel substrate. Read
/// the reply from stdin DIRECTLY on the single stack — a synchronous host_await
/// that works under `wasmtime` (wasip1) with piped stdin, the same observable
/// behavior as native's stdio host. Writes the request (a prompt) to stdout,
/// reads one line as the reply (trailing newline stripped; EOF → `None`). This
/// makes interactive Axon programs run on headless wasm.
#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
pub(crate) fn host_await_yield(req: SendValue) -> Result<Option<SendValue>, ()> {
    use std::io::{BufRead, Write};
    // str-only substrate: a non-str payload is refused (no value channel here).
    let req = req.as_str_payload().ok_or(())?;
    print!("{req}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => Ok(None), // EOF / no stdin → end-of-input
        Ok(_) => Ok(Some(SendValue::Str(
            line.trim_end_matches(['\n', '\r']).to_string(),
        ))),
    }
}

/// BROWSER wasm (`wasm32-unknown-unknown`): no stdin, no threads. Yield the request
/// to JavaScript via an imported `axon_host_await(req_ptr, req_len, out_ptr,
/// out_cap) -> i64` host function: JS reads the request from linear memory, writes
/// up to `out_cap` reply bytes into `out_ptr`, and returns the reply byte length
/// (or a negative value for end-of-input → `None`). v1 is SYNCHRONOUS (the page
/// answers immediately — a tool-call, a pre-filled value); the ASYNC cases (input
/// box, fetch, frame loop) are the R15 §13 B3 follow-on, where `wasm-opt
/// --asyncify` lets THIS SAME import suspend the module and resume after a JS
/// Promise. The axon-wasm cdylib hosts this binding; JS must supply the import.
#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
pub(crate) fn host_await_yield(req: SendValue) -> Result<Option<SendValue>, ()> {
    extern "C" {
        fn axon_host_await(
            req_ptr: *const u8,
            req_len: usize,
            out_ptr: *mut u8,
            out_cap: usize,
        ) -> i64;
    }
    // str-only substrate: a non-str payload is refused (no value channel here).
    let req = req.as_str_payload().ok_or(())?;
    // A generous fixed reply buffer (the page truncates to fit). 64 KiB covers a
    // prompt reply / tool result; larger payloads should stream (future).
    let mut buf = vec![0u8; 64 * 1024];
    let n = unsafe { axon_host_await(req.as_ptr(), req.len(), buf.as_mut_ptr(), buf.len()) };
    if n < 0 {
        return Ok(None); // end-of-input
    }
    let n = (n as usize).min(buf.len());
    buf.truncate(n);
    Ok(Some(SendValue::Str(
        String::from_utf8_lossy(&buf).into_owned(),
    )))
}

/// Run `program` with a `Value`-aware HOST driving its `host_await` suspensions.
/// `host(req)` is called once per `host_await`, on THIS thread, with the owned
/// `SendValue` deep-clone of the program's request payload; its return —
/// `Some(reply)` or `None` at end-of-input — is fed back (reconstructed into a
/// `Value`) as the resume value. Returns the program's exit code. (R15 Slice 2 —
/// arbitrary `Value` payloads: dict/struct/enum/tuple/closure all cross.)
///
/// NATIVE-ONLY: the substrate is a worker thread (`std::thread::scope`), which is
/// unavailable on `wasm32` (`thread::spawn` traps). The browser binding (R7c)
/// drives `host_await` via Asyncify + a JS import instead (R15 §13), NOT this
/// thread-based path. The owned `SendValue` deep-clone is what makes a `!Send`
/// `Value` payload cross the worker channel (§11 Slice (2) safe alternative).
#[cfg(not(target_arch = "wasm32"))]
pub fn run_suspendable_values(
    program: &Program,
    mut host: impl FnMut(SendValue) -> Option<SendValue>,
) -> i32 {
    use std::sync::mpsc::channel;
    let (req_tx, req_rx) = channel::<SendValue>(); // worker → host (await requests)
    let (rep_tx, rep_rx) = channel::<Option<SendValue>>(); // host → worker (replies; None = EOF)
    std::thread::scope(|scope| {
        let worker = scope.spawn(move || {
            HOST_AWAIT.with(|h| *h.borrow_mut() = Some(HostChannels { req_tx, rep_rx }));
            let code = run_program_inner(program, crate::verify::Discharged::default());
            // Drop the channels → req_tx closes → the host loop below ends.
            HOST_AWAIT.with(|h| *h.borrow_mut() = None);
            code
        });
        // Host loop: service each suspension until the worker finishes (req_tx drops).
        while let Ok(req) = req_rx.recv() {
            let _ = rep_tx.send(host(req));
        }
        worker.join().unwrap_or(101)
    })
}

/// Run `program` with a str-only HOST driving its `host_await` suspensions — the
/// interactive-text path (a prompt loop / REPL / the stdio driver). `host(req)` is
/// called once per `host_await` with the request's `str` projection; its
/// `Option<String>` reply is fed back. A non-str request payload is rendered to its
/// `Value` display string for the prompt (text hosts can't carry structured
/// payloads); use [`run_suspendable_values`] for full `Value` fidelity. (R15.)
#[cfg(not(target_arch = "wasm32"))]
pub fn run_suspendable(program: &Program, mut host: impl FnMut(&str) -> Option<String>) -> i32 {
    run_suspendable_values(program, |req| {
        let s = match &req {
            SendValue::Str(s) => s.clone(),
            other => send_value_display(other),
        };
        host(&s).map(SendValue::Str)
    })
}

/// wasm32 has no OS threads, so the worker-thread host-driver substrate can't run
/// here. Run the program directly with NO host driver: a `host_await` call then
/// hits the clean "called outside a suspendable run (no host driver)" panic
/// (exit 101), rather than trapping on `thread::spawn`. The browser binding (R7c)
/// will drive `host_await` via Asyncify + a JS import (R15 §13) — a different
/// substrate that replaces this one on wasm, with the same surface + semantics.
#[cfg(target_arch = "wasm32")]
pub fn run_suspendable(program: &Program, _host: impl FnMut(&str) -> Option<String>) -> i32 {
    run_program_inner(program, crate::verify::Discharged::default())
}

/// wasm `Value`-aware variant — same no-thread story as `run_suspendable`: there is
/// no worker-thread substrate here, so the program runs with no host driver and a
/// `host_await` call hits the clean "no host driver" panic. The browser binding
/// (R7c) drives `host_await` via Asyncify + a JS import (str-only, R15 §13).
#[cfg(target_arch = "wasm32")]
pub fn run_suspendable_values(
    program: &Program,
    _host: impl FnMut(SendValue) -> Option<SendValue>,
) -> i32 {
    run_program_inner(program, crate::verify::Discharged::default())
}

/// The default CLI host for `host_await`: write the request (a prompt) to stdout,
/// then read a line from stdin as the reply (trailing newline stripped). EOF →
/// `None` (end-of-input), which `host_await_opt` surfaces so a read loop can stop;
/// plain `host_await` collapses it to "". This makes an interactive Axon program —
/// a prompt loop, a REPL, a quiz — work under a plain `axon run`. (R15 v0; the
/// program's own `println`s and the prompt share stdout, ordered by the protocol.)
pub fn run_suspendable_stdio(program: &Program) -> i32 {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    run_suspendable(program, |prompt| {
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => None, // EOF or error → end-of-input
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        }
    })
}

fn run_program_inner(program: &Program, discharged: crate::verify::Discharged) -> i32 {
    let mut interp = Interp::build(program).with_discharged(discharged);
    // BUG_HUNT #23: a missing entry point is a COMPILE-time error (the program
    // is malformed), not a runtime panic. Report it cleanly with exit 2 (the
    // compile-error code) instead of `panic: no main` + exit 101 — and never
    // exit 0, which masqueraded as success.
    if !interp.fns.contains_key("main") {
        let _ = std::io::stdout().flush();
        eprintln!("error: no `main` function defined — a runnable program needs `fn main() -> i64` (or `fn main()`)");
        return 2;
    }
    let outcome = interp.init_globals().and_then(|()| interp.run_main());
    match outcome {
        Ok(Value::Int(n)) => n as i32,
        Ok(_) => 0,
        Err(Flow::Exit(code)) => code,
        Err(Flow::VerifyFailed(msg)) => {
            // Policy rejection, not a crash — distinct exit code so CI can tell
            // "verification failed" apart from "the program panicked" (#26).
            let _ = std::io::stdout().flush();
            eprintln!("axon: verify failed: {msg}");
            VERIFY_FAILED_EXIT_CODE
        }
        Err(Flow::Halted(msg)) => {
            // The corrigibility kill-switch caught a call — refused, not crashed.
            // Distinct exit code (4) so a supervisor branches on "the switch
            // fired" specifically. (R9)
            let _ = std::io::stdout().flush();
            eprintln!("axon: halted: {msg}");
            HALTED_EXIT_CODE
        }
        Err(Flow::AiPolicyUnreachable(msg)) => {
            // AI-policy condition (E1300/E1301/E1302): offline-no-fallback,
            // budget exhausted, or unknown tier. User-actionable, not a crash —
            // distinct exit code (5) so a supervisor branches on "AI policy needs
            // attention" instead of treating it like an overflow/div0 bug.
            let _ = std::io::stdout().flush();
            eprintln!("axon: ai policy: {msg}");
            AI_POLICY_EXIT_CODE
        }
        Err(Flow::Panic(msg)) => {
            let _ = std::io::stdout().flush();
            eprintln!("axon: panic: {msg}");
            101
        }
        Err(Flow::Resume(_)) => {
            // `resume(..)` reached the top level — it was used outside a handler
            // arm (the resolver normally rejects this at check time; this is the
            // runtime backstop). A crash, not a silent exit.
            let _ = std::io::stdout().flush();
            eprintln!("axon: panic: `resume` called outside an effect-handler arm");
            101
        }
        Err(Flow::HandlerDone(_)) => {
            // A multi-shot handler's `HandlerDone` escaped its `with` block — an
            // interpreter bug (it is always caught by `eval_with_handler`). Treat
            // as a panic rather than a silent exit.
            let _ = std::io::stdout().flush();
            eprintln!("axon: panic: handler continuation escaped its `with` block");
            101
        }
        Err(Flow::MultiShotUnsound(msg)) => {
            // E1314: a multi-shot `resume` over a body that re-fires effects. The
            // replay-based continuation can't soundly re-execute a side effect, so
            // we refuse rather than silently double-fire or drop work. A panic-class
            // stop (the program wants true delimited continuations — deferred).
            let _ = std::io::stdout().flush();
            eprintln!("axon: multi-shot resume not supported here: {msg}");
            101
        }
        Err(Flow::RefineViolation(msg)) => {
            // A refinement-type precondition was violated by a non-constant arg.
            // Not a crash — the caller passed an out-of-contract value — so a
            // distinct exit code (6), like @[verify]→3 / @[corrigible]→4.
            let _ = std::io::stdout().flush();
            eprintln!("axon: refinement violated: {msg}");
            REFINE_VIOLATION_EXIT_CODE
        }
        Err(Flow::GoalBudgetExhausted(msg)) => {
            // R12b: a kernel Goal hit its principal's budget ceiling. Not a crash;
            // distinct exit code (7) so a supervisor can branch on it. (E1604)
            let _ = std::io::stdout().flush();
            eprintln!("axon: goal budget exhausted: {msg}");
            GOAL_BUDGET_EXIT_CODE
        }
        Err(Flow::SandboxViolation(msg)) => {
            // F5: a sandboxed call attempted an effect outside its declared ceiling.
            // Not a crash — the sandbox is doing its job — distinct exit code (8)
            // so a supervisor can branch on "the sandbox caught this" specifically.
            let _ = std::io::stdout().flush();
            eprintln!("axon: sandbox violation: {msg}");
            SANDBOX_VIOLATION_EXIT_CODE
        }
        // A stray return/break/continue escaping `main` — treat as clean exit.
        Err(_) => 0,
    }
}

/// Run a single zero-argument function (e.g. an `@[test]`) by name.
///
/// Returns `Ok(())` if it completed without panicking, or `Err(message)` on a
/// runtime panic / non-zero `exit`. Used by `axon test` to run tests in-process.
pub fn run_test_fn(program: &Program, name: &str) -> Result<(), String> {
    on_deep_stack(|| run_test_fn_inner(program, name))
}

fn run_test_fn_inner(program: &Program, name: &str) -> Result<(), String> {
    let mut interp = Interp::build(program);
    if let Err(f) = interp.init_globals() {
        return Err(flow_to_msg(f));
    }
    let Some(f) = interp.fns.get(name).copied() else {
        return Err(format!("no function `{name}`"));
    };
    match interp.call_fn(f, vec![]) {
        Ok(_) => Ok(()),
        Err(Flow::Panic(m)) => Err(m),
        // A verify failure inside a test is still a failure (drives
        // `@[test(should_fail)]`); surface its message like a panic.
        Err(Flow::VerifyFailed(m)) => Err(m),
        // A corrigibility halt inside a test is a failure too (lets
        // `@[test(should_fail)]` assert the kill-switch latched).
        Err(Flow::Halted(m)) => Err(m),
        // An AI-policy stop inside a test is a failure (surfaces like a panic;
        // also lets `@[test(should_fail)]` assert the policy gate fired).
        Err(Flow::AiPolicyUnreachable(m)) => Err(m),
        // A refinement-precondition violation inside a test is a failure too
        // (lets `@[test(should_fail)]` assert a bad arg is caught).
        Err(Flow::RefineViolation(m)) => Err(m),
        // A kernel-goal budget exhaustion inside a test is a failure too (lets
        // `@[test(should_fail)]` assert the budget ceiling fired).
        Err(Flow::GoalBudgetExhausted(m)) => Err(m),
        // A sandbox violation inside a test is a failure (lets
        // `@[test(should_fail)]` assert the sandbox ceiling fired).
        Err(Flow::SandboxViolation(m)) => Err(m),
        Err(Flow::Resume(_)) => Err("`resume` called outside an effect-handler arm".to_string()),
        // E1314 multi-shot-unsound inside a test is a failure (lets
        // `@[test(should_fail)]` assert the unsound-replay case is refused).
        Err(Flow::MultiShotUnsound(m)) => Err(m),
        Err(Flow::Exit(0)) => Ok(()),
        Err(Flow::Exit(n)) => Err(format!("exited with code {n}")),
        // A stray return/break/continue escaping the fn — treat as clean.
        Err(_) => Ok(()),
    }
}

fn flow_to_msg(f: Flow) -> String {
    match f {
        Flow::Panic(m)
        | Flow::VerifyFailed(m)
        | Flow::Halted(m)
        | Flow::AiPolicyUnreachable(m)
        | Flow::RefineViolation(m)
        | Flow::GoalBudgetExhausted(m)
        | Flow::SandboxViolation(m) => m,
        Flow::Exit(n) => format!("exited with code {n}"),
        _ => "non-local control flow escaped the program".into(),
    }
}

/// Founder-facing label for a `@[verify]`-armed function in failure messages.
/// `assert_deployable` is the *generated* deploy-gate symbol the surface
/// compiler emits (see axon-surface `compile.rs`); leaking that name to a
/// non-technical user is an impl-detail leak (BUG_HUNT #25). Map it to plain
/// language; any author-named verify fn keeps its own name (the author chose
/// it, so it's meaningful to them).
fn verify_fn_label(fn_name: &str) -> String {
    if fn_name == "assert_deployable" {
        "the deploy gate".to_string()
    } else {
        format!("`{fn_name}`")
    }
}

impl<'p> Interp<'p> {
    pub fn build(program: &'p Program) -> Self {
        let mut fns = HashMap::new();
        let mut structs = HashMap::new();
        let mut enums = HashMap::new();
        let mut methods = HashMap::new();
        let mut global_defs = Vec::new();
        let mut refine_preds = HashMap::new();

        for item in &program.items {
            match item {
                Item::FnDef(f) => {
                    fns.insert(f.name.clone(), f);
                }
                Item::RefineDef(r) => {
                    // Phase 5: index the predicate so `call_fn` can evaluate it as
                    // a runtime precondition when a param's type is this refinement.
                    refine_preds.insert(r.name.clone(), r.predicate.as_ref());
                }
                Item::TypeDef(t) => {
                    structs.insert(t.name.clone(), t);
                }
                Item::EnumDef(e) => {
                    enums.insert(e.name.clone(), e);
                }
                Item::ImplBlock(ImplBlock {
                    for_type,
                    methods: ms,
                    ..
                }) => {
                    let tn = type_name_of(for_type);
                    for m in ms {
                        methods.insert((tn.clone(), m.name.clone()), m);
                    }
                }
                Item::LetDef { name, value, .. } => {
                    global_defs.push((name.clone(), value.as_ref()));
                }
                _ => {}
            }
        }

        Interp {
            fns,
            structs,
            enums,
            methods,
            global_defs,
            globals: HashMap::new(),
            provenance: RefCell::new(HashMap::new()),
            provenance_inputs: RefCell::new(HashMap::new()),
            provenance_inputs_f64: RefCell::new(HashMap::new()),
            call_depth: Cell::new(0),
            max_depth: resolve_max_depth(),
            corrigible_halted: Cell::new(false),
            enclosing_agent: RefCell::new(None),
            current_goal: RefCell::new(None),
            current_principal: RefCell::new("root".to_string()),
            goal_constraint: RefCell::new(None),
            current_fn: RefCell::new(String::new()),
            current_call_tier: RefCell::new(None),
            ai_calls_this_fn: Cell::new(0),
            ai_cost_micro: Cell::new(0),
            handlers: RefCell::new(Vec::new()),
            resume_replay: RefCell::new(None),
            resume_ctx: RefCell::new(Vec::new()),
            principals: RefCell::new(crate::kernel::PrincipalRegistry::new()),
            // Scheduler order is a function of spawn order + AXON_SEED (R12 §5
            // determinism): derive the round-robin start offset from the seed.
            scheduler: RefCell::new(crate::kernel::Scheduler::new(rng_seed() as usize)),
            supervisors: RefCell::new(Vec::new()),
            stores: RefCell::new(Vec::new()),
            llm_gateways: RefCell::new(Vec::new()),
            goals: RefCell::new(Vec::new()),
            sandboxes: RefCell::new(Vec::new()),
            active_sandbox: Cell::new(-1),
            refine_preds,
            discharged: crate::verify::Discharged::default(),
            gfx_mock: RefCell::new(crate::native::GfxMock::new()),
        }
    }

    /// Install the set of statically-discharged obligations (Phase 5 §4). A
    /// pipeline that ran the SMT prover passes its `Discharged` here so the
    /// interpreter elides the runtime checks Z3 already proved ∀-inputs. A no-op
    /// for any obligation not in the set, so this only ever *removes* a check
    /// that could not have fired.
    pub fn with_discharged(mut self, discharged: crate::verify::Discharged) -> Self {
        self.discharged = discharged;
        self
    }

    /// Evaluate module-level constants in source order, so each may reference
    /// those defined before it. Populates [`Interp::globals`].
    fn init_globals(&mut self) -> Result<(), Flow> {
        if self.global_defs.is_empty() {
            return Ok(());
        }
        let defs = std::mem::take(&mut self.global_defs);
        let mut env = Env::new();
        for (name, expr) in &defs {
            let v = self.eval(expr, &mut env)?;
            env.define(name.clone(), v);
        }
        self.globals = env.snapshot();
        Ok(())
    }

    /// Run `main` with no arguments.
    fn run_main(&self) -> R {
        match self.fns.get("main") {
            Some(f) => self.call_fn(f, vec![]),
            None => panic("no `main` function"),
        }
    }

    // ── Function / closure calls ─────────────────────────────────────────────

    /// R3 §3.3: the `@[ai(policy(fallback: "…"))]` value declared on the
    /// currently-executing fn, if any. Accepts both the grouped form and the
    /// flat `@[ai(fallback: "…")]` (the parser flattens the group), so the arg
    /// reads as `"fallback: <value>"`. Returns the fallback string when present.
    /// This is what lets an offline `ai_complete` stay total instead of panicking.
    /// Only read on the `#[cfg(not(asi-runtime))]` offline branch — when the live
    /// model is compiled in, there is no offline fallback path, so it is dead there.
    #[cfg_attr(feature = "asi-runtime", allow(dead_code))]
    fn current_ai_fallback(&self) -> Option<String> {
        let name = self.current_fn.borrow().clone();
        let f = self.fns.get(name.as_str())?;
        let ai = f.attrs.iter().find(|a| a.name == "ai")?;
        for arg in &ai.args {
            if let Some(rest) = arg.strip_prefix("fallback:") {
                return Some(rest.trim().to_string());
            }
        }
        None
    }

    /// R3c: the `@[ai(policy(budget: N))]` ceiling declared on the
    /// currently-executing fn, if any. Returns `Some(N)` for a well-formed
    /// non-negative integer; `None` when no `budget:` field is present **or** the
    /// value is malformed (in which case a `W1311` is emitted once and the fn
    /// runs unmetered — a bad budget must never silently enforce a wrong number).
    fn current_ai_budget(&self) -> Option<u64> {
        let name = self.current_fn.borrow().clone();
        let f = self.fns.get(name.as_str())?;
        let ai = f.attrs.iter().find(|a| a.name == "ai")?;
        for arg in &ai.args {
            if let Some(rest) = arg.strip_prefix("budget:") {
                let raw = rest.trim();
                return match raw.parse::<u64>() {
                    Ok(n) => Some(n),
                    Err(_) => {
                        eprintln!(
                            "warning: [{}] @[ai(policy(budget: {raw}))] on `{name}` is not a \
                             non-negative integer — ignored (fn runs unmetered)",
                            crate::error::W1311
                        );
                        None
                    }
                };
            }
        }
        None
    }

    /// R4: the name of the currently-executing fn if it is in the `@[agent]`
    /// zone, else `None`. Used to inject the mandatory agent action log: every
    /// capability-bearing action an agent takes is audited (I-13).
    fn current_agent_fn(&self) -> Option<String> {
        // R4/I-13: the nearest ENCLOSING `@[agent]` on the call stack — so a
        // capability builtin called from a helper of an agent is still logged to
        // that agent's action trail (the audit can't be escaped by indirection).
        self.enclosing_agent.borrow().clone()
    }

    /// F3: enter goal-optimization scope. Records `name` as the current goal until
    /// the returned guard drops, so an `ai_complete` invoked inside a metric
    /// evaluation is attributed to the goal that triggered it. Nesting-safe (the
    /// previous goal is restored on drop). Call at the top of each `run_goal*`.
    fn enter_goal(&self, name: &str) -> FnNameOptGuard<'_> {
        FnNameOptGuard {
            cell: &self.current_goal,
            prev: self.current_goal.replace(Some(name.to_string())),
        }
    }

    /// The goal/metric name currently being optimized, or `None` outside any
    /// `run_goal*`. Stamped into the `ai_call` provenance for causal attribution.
    fn current_goal_name(&self) -> Option<String> {
        self.current_goal.borrow().clone()
    }

    /// F3 (Phase 9): the name of the principal currently in scope for audit
    /// attribution. Defaults to "root"; overridden by `principal_activate`.
    fn current_principal_name(&self) -> String {
        self.current_principal.borrow().clone()
    }

    /// Whether the currently-executing fn carries an `@[ai(policy)]` attribute.
    /// Used for W1310: a live/mock AI call from a fn with no policy is allowed
    /// but un-metered and un-pinned, so it warns (R3 §6).
    fn current_fn_has_ai_policy(&self) -> bool {
        let name = self.current_fn.borrow().clone();
        self.fns
            .get(name.as_str())
            .map(|f| f.attrs.iter().any(|a| a.name == "ai"))
            .unwrap_or(false)
    }

    /// R3 §4.2 — resolve the AI tier for the current call from the enclosing
    /// `@[ai(policy(tier: …))]`, defaulting to [`crate::ai_routing::DEFAULT_TIER`]
    /// when the fn has no policy or its policy names no tier. (Per-call `tier:`
    /// args — step 1 — are deferred until named-arg call syntax lands; this
    /// covers steps 2-3.) An *unknown* tier name in the policy is **E1302**.
    fn current_ai_tier(&self) -> Result<crate::ai_routing::Tier, Flow> {
        use crate::ai_routing::{Tier, DEFAULT_TIER};
        // R3b — step 1: a per-call `tier:` arg overrides the policy/default.
        // `take` it so it applies to exactly this one call and never leaks to a
        // nested or subsequent call.
        if let Some(raw) = self.current_call_tier.borrow_mut().take() {
            return match Tier::parse(&raw) {
                Some(t) => Ok(t),
                None => Err(Flow::AiPolicyUnreachable(format!(
                    "[{}] unknown AI tier `{raw}` — configured tiers: {}",
                    crate::error::E1302,
                    Tier::configured()
                ))),
            };
        }
        // Steps 2-3: the enclosing @[ai(policy(tier:))], else the default —
        // resolved via the shared `ai_routing::tier_from_attrs` so the interp and
        // the native codegen refusal agree on a fn's tier exactly.
        let name = self.current_fn.borrow().clone();
        let Some(f) = self.fns.get(name.as_str()) else {
            return Ok(DEFAULT_TIER);
        };
        crate::ai_routing::tier_from_attrs(&f.attrs).map_err(|raw| {
            Flow::AiPolicyUnreachable(format!(
                "[{}] unknown AI tier `{raw}` — configured tiers: {}",
                crate::error::E1302,
                Tier::configured()
            ))
        })
    }

    fn call_fn(&self, f: &FnDef, args: Vec<Value>) -> R {
        // Bound recursion: a graceful panic instead of a process-aborting stack
        // overflow on runaway/infinite recursion. `_guard` restores the depth on
        // any return path (including `?`).
        let depth = self.call_depth.get() + 1;
        if depth > self.max_depth {
            return panic(format!(
                "recursion limit exceeded ({}) in `{}` — infinite or excessively deep recursion? \
                 (raise with AXON_MAX_DEPTH if this recursion is legitimate)",
                self.max_depth, f.name
            ));
        }
        self.call_depth.set(depth);
        let _guard = DepthGuard(&self.call_depth);
        // Track the executing fn so builtins (R3 ai_call provenance) can
        // attribute their records to the caller; restored on return.
        let _fn_guard = FnNameGuard {
            cell: &self.current_fn,
            prev: self.current_fn.replace(f.name.clone()),
        };
        // R4/I-13: if THIS fn is an `@[agent]`, it becomes the enclosing agent for
        // everything it transitively calls; otherwise the caller's enclosing agent
        // is inherited unchanged. Restored on return so sibling calls aren't
        // wrongly attributed. The agent action log reads this (not just the
        // immediate fn) so an agent can't escape the audit by calling a helper.
        let _agent_guard = if f.attrs.iter().any(|a| a.name == "agent") {
            Some(FnNameOptGuard {
                cell: &self.enclosing_agent,
                prev: self.enclosing_agent.replace(Some(f.name.clone())),
            })
        } else {
            None
        };
        // R3c: each fn activation meters its own ai_complete calls — reset to 0
        // on entry, restore the caller's count on exit.
        let _ai_budget_guard = AiBudgetGuard {
            cell: &self.ai_calls_this_fn,
            prev: self.ai_calls_this_fn.replace(0),
        };

        if f.params.len() != args.len() {
            return panic(format!(
                "{}: expected {} args, got {}",
                f.name,
                f.params.len(),
                args.len()
            ));
        }

        // R9 corrigibility: if the kill-switch latch is tripped, REFUSE every
        // `@[corrigible]` call before its body can run. The body's side effects
        // never happen, and the latch never clears — the function cannot resist
        // or reverse its own shutdown. Keyed on the annotation, enforced by the
        // engine, so a user cannot write a corrigible fn that ignores the halt.
        if self.corrigible_halted.get() && f.attrs.iter().any(|a| a.name == "corrigible") {
            return Err(Flow::Halted(format!(
                "`{}` refused: corrigibility kill-switch is latched \
                 (corrigible_halt() was called; there is no resume)",
                f.name
            )));
        }
        // The leading i64 / f64 args (if any) form the goal-search input
        // tuple — recorded so goal_run can resume from the best prior probe
        // and multi-arg coordinate descent can seed each dim independently.
        // Two parallel collectors so an i64-prefix fn and an f64-prefix fn
        // both populate the right store; we choose the right one based on
        // the fn's signature in `run_goal`.
        let input_args: Vec<i64> = args
            .iter()
            .take_while(|v| matches!(v, Value::Int(_)))
            .map(|v| if let Value::Int(n) = v { *n } else { 0 })
            .collect();
        let input_args_f64: Vec<f64> = args
            .iter()
            .take_while(|v| matches!(v, Value::Float(_)))
            .map(|v| if let Value::Float(f) = v { *f } else { 0.0 })
            .collect();
        // First dim, for back-compat with the verify-panic enrichment that
        // reports a single "input N" suffix.
        let input_arg: Option<i64> = input_args.first().copied();
        let mut env = Env::new();
        for (p, a) in f.params.iter().zip(args) {
            // Soft typing: `Uncertain<T>` is compatible with a plain-`T` parameter
            // (the checker allows it). If the declared param type is NOT itself a
            // soft wrapper but the argument IS one, unwrap to the inner value so
            // the body sees a plain `T` (else `x * 2` on the struct silently
            // produced 0). Confidence/horizon dropped at this T-typed boundary.
            let param_is_soft = matches!(
                &p.ty,
                crate::ast::AxonType::Generic { base, .. } if base == "Uncertain" || base == "Temporal"
            );
            let a = if !param_is_soft {
                value::soft_inner(&a).unwrap_or(a)
            } else {
                a
            };
            // R19 Slice B: coerce Int→SizedInt when the declared param type is a
            // non-i64 integer width — ensures arithmetic inside the callee's body
            // uses width-correct ops (completeness, I-9).
            let a = if let Some(width) = interp_eval_axon_type_to_width(&p.ty) {
                interp_eval_coerce_to_sized(a, width)
            } else {
                a
            };
            env.define(p.name.clone(), a);
        }
        // Phase 5: refinement-type PRECONDITIONS. A parameter `p: T where P`
        // desugars to a synthetic named refinement; the checker discharges P
        // statically only for compile-time-CONSTANT args (E1209). For a
        // non-constant arg the predicate becomes a runtime check (the spec's
        // Z3-free `--proof-timeout 0` fallback): evaluate P with `_` bound to the
        // actual value and refuse with a distinct exit code (6) on violation.
        // Skipped entirely unless the program declares refinements AND this fn has
        // a refined param, so unrefined hot recursion pays nothing. The predicate
        // references only `_` plus pure helpers (impure builtins in a `where` are
        // E1209-rejected), so it cannot re-enter this fn's body; any pure-helper
        // recursion is bounded by the same `max_depth` guard above.
        if !self.refine_preds.is_empty() {
            for p in &f.params {
                if let crate::ast::AxonType::Named(rname) = &p.ty {
                    if let Some(pred) = self.refine_preds.get(rname.as_str()).copied() {
                        let val = env.get(&p.name).cloned().unwrap_or(Value::Unit);
                        let mut pred_env = Env::new();
                        pred_env.define("_".into(), val.clone());
                        // Also bind the parameter name (for inline refinements
                        // `p: T where E[p] > k` that use the param name directly).
                        pred_env.define(p.name.clone(), val.clone());
                        if let Value::Bool(false) = self.eval(pred, &mut pred_env)? {
                            return Err(Flow::RefineViolation(format!(
                                "parameter `{}` of `{}` (= {}) violates the refinement `{}` — \
                                 the value does not satisfy the type's predicate",
                                p.name,
                                f.name,
                                value::display(&val),
                                rname
                            )));
                        }
                    }
                }
            }
        }
        // R5 `#[goal(...)]` sugar: train the metric, evaluate on the held-out
        // set, gate. With a `test_set: [a, b, c]` (or repeated `holdout:`), the
        // goal is met only if the metric clears `target` on EVERY held-out point
        // — i.e. on the WORST (minimum) score — so a fn cannot pass by
        // overfitting one point. With no held-out set, fall back to the best
        // observed training score.
        let mut goal_met: i64 = 0;
        if let Some(spec) = self.goal_spec_of(f) {
            // Dispatch on the selected strategy (PRD L889-899). All run the
            // metric and accumulate provenance the same way; they differ only in
            // HOW they explore. The held-out gate below is strategy-agnostic.
            let me = spec.max_evals;
            match spec.strategy {
                GoalStrategy::HillClimb => {
                    let _ = self.run_goal(&spec.metric, spec.target, me)?;
                }
                GoalStrategy::Random => {
                    let _ = self.run_goal_random(
                        &spec.metric,
                        spec.target,
                        me.max(1),
                        spec.lo,
                        spec.hi,
                    )?;
                }
                GoalStrategy::Multistart => {
                    let (starts, per) = split_budget(me);
                    let _ = self.run_goal_multistart(
                        &spec.metric,
                        spec.target,
                        starts,
                        per,
                        spec.lo,
                        spec.hi,
                    )?;
                }
                GoalStrategy::Tournament => {
                    let _ = self.run_goal_tournament(
                        &spec.metric,
                        spec.target,
                        me.max(1),
                        spec.lo,
                        spec.hi,
                        false,
                    )?;
                }
                GoalStrategy::Bayesian => {
                    // Exploit-biased tournament (single elite + heavy refine).
                    let _ = self.run_goal_tournament(
                        &spec.metric,
                        spec.target,
                        me.max(1),
                        spec.lo,
                        spec.hi,
                        true,
                    )?;
                }
            }
            let s = if spec.holdout_set.is_empty() {
                self.best_observed(&spec.metric, spec.target, 0)
            } else {
                let mut worst = f64::INFINITY;
                for h in &spec.holdout_set {
                    let score = self.goal_eval_holdout(&spec.metric, *h)?;
                    if score < worst {
                        worst = score;
                    }
                }
                worst
            };
            goal_met = if s >= spec.target { 1i64 } else { 0i64 };
        }
        env.define("goal_met".into(), Value::Int(goal_met));
        let mut result = match self.eval(&f.body, &mut env) {
            Ok(v) => v,
            Err(Flow::Return(v)) => v,
            Err(other) => return Err(other),
        };
        // Soft typing at the RETURN boundary: a fn declared `-> T` (a plain
        // scalar) whose body produces an `Uncertain<T>`/`Temporal<T>` unwraps to
        // the inner value — the same rule as a plain-T parameter. Without this,
        // `fn f() -> i64 { uncertain }` leaked the struct and `f() + 1` silently
        // produced 0. A fn declared `-> Uncertain<T>`/`-> Temporal<T>` keeps it.
        {
            // Only unwrap when the declared return is a plain SCALAR (i64/i32/
            // f64/bool); a str/struct/tuple/soft-wrapper return is left untouched.
            let ret_is_scalar = matches!(
                &f.return_type,
                Some(crate::ast::AxonType::Named(n))
                    if matches!(n.as_str(), "i64" | "i32" | "f64" | "bool")
            );
            if ret_is_scalar {
                if let Some(inner) = value::soft_inner(&result) {
                    result = inner;
                }
            }
        }

        // Phase 5: refinement-type POSTCONDITION — the dual of the entry-time
        // precondition check above. A fn declared `-> T where P` must produce a
        // value satisfying `P`. The checker discharges a CONSTANT return (E1209)
        // and the SMT backend proves some non-constant cases (`axon verify`); for
        // a non-constant return in the default build the predicate becomes a
        // runtime check (the spec's Z3-free fallback). Evaluate P with `_` bound
        // to the finalized return value; a violation is the same runtime
        // refinement-contract breach as a bad argument → exit 6. Skipped unless
        // the program declares refinements AND this fn returns one.
        // Phase 5 §4: if the SMT prover discharged this fn's refinement-return
        // postcondition for ALL inputs, the check below is provably dead — skip
        // it. `refine_return_proven` is false for every fn unless a `Discharged`
        // set was installed, so the default build still runs the check.
        if !self.refine_preds.is_empty() && !self.discharged.refine_return_proven(&f.name) {
            if let Some(crate::ast::AxonType::Named(rname)) = &f.return_type {
                if let Some(pred) = self.refine_preds.get(rname.as_str()).copied() {
                    // R20 Slice 2: evaluate the predicate with `_` bound to the
                    // return value AND the fn's params still in scope, so a
                    // RELATIONAL return refinement (e.g.
                    // `-> (i64 where _ <= parent_rem)`) can reference parameters.
                    // `env` already holds the param bindings from the body; add
                    // `_` and evaluate against it instead of a bare env.
                    env.define("_".into(), result.clone());
                    if let Value::Bool(false) = self.eval(pred, &mut env)? {
                        return Err(Flow::RefineViolation(format!(
                            "the return value of `{}` (= {}) violates the refinement return \
                             type `{}` — the value does not satisfy the type's predicate",
                            f.name,
                            value::display(&result),
                            rname
                        )));
                    }
                }
            }
        }

        // R4 zone provenance injection. Keyed on the fn's annotation, performed
        // by the engine — there is no opt-out (I-13). Two adaptive-family zones
        // record a numeric return:
        //   - `@[adaptive]`    → logged AND pushed to the in-memory best store
        //                        that `goal_run`/`goal_count` read (an
        //                        optimization target).
        //   - `@[experiment(l)]` → logged tagged `zone:"experiment"` + label,
        //                        but EXCLUDED from the best store (a comparison
        //                        baseline, never auto-promoted). This is the
        //                        behavioral distinction that makes the PRD's
        //                        third zone real instead of a synonym.
        // Both still log to the JSONL, so a zoned fn that executes always
        // leaves a provenance record.
        let is_adaptive_zone = f.attrs.iter().any(|a| a.name == "adaptive");
        let experiment_label = f
            .attrs
            .iter()
            .find(|a| a.name == "experiment")
            .map(|a| a.args.first().cloned().unwrap_or_default());
        if is_adaptive_zone || experiment_label.is_some() {
            if let Some(score) = numeric_score(&result) {
                // The in-memory best store feeds `goal_run` — adaptive only.
                // Experiment records are deliberately withheld so the optimizer
                // never treats a baseline as a candidate to beat.
                if is_adaptive_zone {
                    self.provenance
                        .borrow_mut()
                        .entry(f.name.clone())
                        .or_default()
                        .push(score);
                    self.provenance_inputs
                        .borrow_mut()
                        .entry(f.name.clone())
                        .or_default()
                        .push(input_args.clone());
                    self.provenance_inputs_f64
                        .borrow_mut()
                        .entry(f.name.clone())
                        .or_default()
                        .push(input_args_f64.clone());
                }
                // Durable JSONL log (axon-rt's format) — every zoned execution,
                // tagged with its zone (and label for experiments) so
                // `axon trace`/observability can separate the streams.
                let payload = match &result {
                    Value::Int(n) => format!("ret_i64={n}"),
                    _ => format!("ret_f64={score}"),
                };
                let (zone, label) = match &experiment_label {
                    Some(l) => ("experiment", Some(l.as_str())),
                    None => ("adaptive", None),
                };
                append_provenance_jsonl(&f.name, &payload, score, input_arg, zone, label);
            }
        }

        // `@[verify(predicate)]`: runtime gate. Two paths:
        //  - For `confidence OP K` / `value OP K` (the codegen-decodable
        //    shapes), do the comparison directly and emit a rich panic
        //    naming both fields and the search input. This matches what
        //    native codegen will emit when the codegen build completes.
        //  - For anything more complex (`&&`, `||`, multi-field, function
        //    calls in the predicate), bind `confidence` / `value` /
        //    `source_tag` into a fresh env and evaluate the predicate as
        //    a normal Expr. Lets the user write
        //    `@[verify(value > 0 && confidence >= 0.8)]` and have it
        //    enforced at runtime — closing ROADMAP §9.5 F6.
        if let Some(spec) = &f.verify {
            if let Value::Struct { name, fields } = &result {
                // `@[verify]` enforces a postcondition on the returned value's
                // `value`/`confidence` fields. Both `Uncertain` and `Temporal`
                // carry those fields, so the same gate applies to both — a
                // `@[verify(value <= 500)]` on a Temporal-returning fn was
                // silently UNENFORCED before (only Uncertain hit this branch).
                if name == "Uncertain" || name == "Temporal" {
                    let decoded =
                        crate::verify::decode_verify_predicate_with_ident(&spec.predicate);
                    let val_str = fields
                        .get("value")
                        .map(display)
                        .unwrap_or_else(|| "?".into());
                    let conf_str = fields
                        .get("confidence")
                        .map(display)
                        .unwrap_or_else(|| "?".into());
                    let input_str = input_arg
                        .map(|n| format!(", input {n}"))
                        .unwrap_or_default();

                    if let Some((ident, op, bound)) = decoded {
                        // Simple shape: do the targeted, well-typed compare.
                        let observed: Option<f64> =
                            match (ident.as_str(), fields.get(ident.as_str())) {
                                ("confidence", Some(Value::Float(c))) => Some(*c),
                                ("value", Some(Value::Int(n))) => Some(*n as f64),
                                ("value", Some(Value::Float(v))) => Some(*v),
                                _ => None,
                            };
                        if let Some(c) = observed {
                            if !cmp_f64(&op, c, bound) {
                                return Err(Flow::VerifyFailed(format!(
                                    "verify failed in {}: {} {} {} {} is false \
                                     (value {}, confidence {}{})",
                                    verify_fn_label(&f.name),
                                    ident,
                                    c,
                                    crate::verify::binop_to_verify_str(&op),
                                    bound,
                                    val_str,
                                    conf_str,
                                    input_str,
                                )));
                            }
                        }
                    } else {
                        // Composite predicate: evaluate as a normal Expr with
                        // `value`, `confidence`, `source_tag` in scope. Any
                        // boolean expression Axon understands is accepted —
                        // `&&`, `||`, comparisons, function calls, you name it.
                        let mut pred_env = Env::new();
                        if let Some(v) = fields.get("value") {
                            pred_env.define("value".into(), v.clone());
                        }
                        if let Some(c) = fields.get("confidence") {
                            pred_env.define("confidence".into(), c.clone());
                        }
                        if let Some(s) = fields.get("source_tag") {
                            pred_env.define("source_tag".into(), s.clone());
                        }
                        let outcome = self.eval(&spec.predicate, &mut pred_env)?;
                        if let Value::Bool(false) = outcome {
                            return Err(Flow::VerifyFailed(format!(
                                "verify failed in {}: composite predicate did not hold \
                                 (value {}, confidence {}{})",
                                verify_fn_label(&f.name),
                                val_str,
                                conf_str,
                                input_str,
                            )));
                        }
                    }
                }
            } else if let Some(observed) =
                // Phase 5 §4: skip the scalar `@[verify]` gate when the SMT prover
                // discharged this fn's `value OP K` bound for ALL inputs — the
                // check is provably dead. `verify_proven` is false unless a
                // `Discharged` set was installed, so the default build is unchanged.
                scalar_as_f64(&result)
                    .filter(|_| !self.discharged.verify_proven(&f.name))
            {
                // SCALAR return (`i64`/`f64`/`bool`): `value` binds to the
                // returned scalar itself. A `@[verify(value OP K)]` safety bound on
                // a plain-typed fn used to be SILENTLY UNENFORCED (the gate only
                // fired for an Uncertain result) — a real hole for a hard bound
                // like `@[verify(value <= 500)]` on an i64 spend recommender.
                let val_str = display(&result);
                let input_str = input_arg
                    .map(|n| format!(", input {n}"))
                    .unwrap_or_default();
                let decoded = crate::verify::decode_verify_predicate_with_ident(&spec.predicate);
                if let Some((ident, op, bound)) = decoded {
                    // Only the `value` ident maps to a scalar return (a scalar has
                    // no `confidence`/`source_tag`); other idents fall through to
                    // the composite path which leaves them unbound (predicate
                    // can't reference a field a scalar doesn't have).
                    if ident == "value" && !cmp_f64(&op, observed, bound) {
                        return Err(Flow::VerifyFailed(format!(
                            "verify failed in {}: value {} {} {} is false (value {}{})",
                            verify_fn_label(&f.name),
                            observed,
                            crate::verify::binop_to_verify_str(&op),
                            bound,
                            val_str,
                            input_str,
                        )));
                    }
                } else {
                    // Composite predicate: bind `value` to the scalar and evaluate.
                    let mut pred_env = Env::new();
                    pred_env.define("value".into(), result.clone());
                    let outcome = self.eval(&spec.predicate, &mut pred_env)?;
                    if let Value::Bool(false) = outcome {
                        return Err(Flow::VerifyFailed(format!(
                            "verify failed in {}: composite predicate did not hold (value {}{})",
                            verify_fn_label(&f.name),
                            val_str,
                            input_str,
                        )));
                    }
                }
            }
        }

        Ok(result)
    }

    fn call_closure(&self, c: Value, args: Vec<Value>) -> R {
        let Value::Closure {
            params,
            body,
            captured,
        } = c
        else {
            return panic(format!("value of type {} is not callable", c.type_name()));
        };
        if params.len() != args.len() {
            return panic(format!(
                "lambda: expected {} args, got {}",
                params.len(),
                args.len()
            ));
        }
        let mut env = Env::new();
        // Base scope = captured bindings; a fresh scope holds the parameters.
        *env.scopes.last_mut().unwrap() = captured;
        env.push();
        for (p, a) in params.iter().zip(args) {
            env.define(p.clone(), a);
        }
        match self.eval(&body, &mut env) {
            Ok(v) => Ok(v),
            Err(Flow::Return(v)) => Ok(v),
            Err(other) => Err(other),
        }
    }

    // ── goal_run: hill-climb / retrospective best-observed ───────────────────

    /// `goal_run(name, target, max_evals)`. Live hill-climb when `name` is an
    /// `@[adaptive] fn(i64) -> i64` in the program; otherwise a retrospective
    /// best-observed lookup over the provenance store. Mirrors `axon-rt`'s
    /// `goal.rs` semantics.
    /// Warm-start counterpart of `run_goal`. Reads the best prior probe from
    /// in-memory provenance and seeds the multi-arg hill climb there;
    /// single-arg paths use the existing on-disk continuation hook unchanged.
    /// Falls through to fresh start (origin seed) when no prior best exists,
    /// so calling cold is a no-op.
    /// A goal name is "known" if it names a defined function OR already has
    /// provenance recorded (legitimate retrospective best-observed lookup).
    /// A name matching neither is a typo — returning `target` silently
    /// (BUG_HUNT #19 / I-9) makes a misspelled metric look like an achieved
    /// goal. Callers error out instead.
    fn goal_name_is_known(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        self.fns.contains_key(name) || self.provenance.borrow().contains_key(name)
    }

    fn unknown_goal_name(name: &str) -> Flow {
        Flow::Panic(format!(
            "goal function `{name}` is not defined and has no recorded provenance — \
             check the name matches an @[adaptive] fn (typo?)"
        ))
    }

    /// Flatten a place expression (`base.f[i].g …`) into the root variable name
    /// and a base-to-leaf list of steps, evaluating any index expressions now
    /// (so the later mutable walk holds no other borrow of `env`).
    fn flatten_place(&self, place: &Expr, env: &mut Env) -> Result<(String, Vec<PlaceStep>), Flow> {
        let mut steps = Vec::new();
        let mut cur = place;
        let base = loop {
            match cur {
                Expr::Ident(name) => break name.clone(),
                Expr::FieldAccess { receiver, field } => {
                    steps.push(PlaceStep::Field(field.clone()));
                    cur = receiver.as_ref();
                }
                Expr::Index { receiver, index } => {
                    let idx = as_int(&self.eval(index, env)?)?;
                    if idx < 0 {
                        return Err(Flow::Panic(format!("negative index {idx}")));
                    }
                    steps.push(PlaceStep::Index(idx as usize));
                    cur = receiver.as_ref();
                }
                _ => return Err(Flow::Panic("invalid assignment target".into())),
            }
        };
        steps.reverse();
        Ok((base, steps))
    }
}

enum LoopStep {
    Break,
    Continue,
}

// ── Free helpers ──────────────────────────────────────────────────────────────

fn lit_to_val(lit: &Literal) -> Value {
    match lit {
        Literal::Int(n) => Value::Int(*n),
        Literal::Float(f) => Value::Float(*f),
        Literal::Bool(b) => Value::Bool(*b),
        Literal::Str(s) => Value::Str(s.clone()),
    }
}

fn type_name_of(ty: &crate::ast::AxonType) -> String {
    use crate::ast::AxonType::*;
    match ty {
        Named(n) => n.clone(),
        Generic { base, .. } => base.clone(),
        Ref(inner) | RawPtr(inner) => type_name_of(inner),
        DynTrait(n) => n.clone(),
        TypeParam(n) => n.clone(),
        Slice(_) => "[]".into(),
        Option(_) => "Option".into(),
        Result { .. } => "Result".into(),
        Chan(_) => "Chan".into(),
        Fn { .. } => "fn".into(),
        Tuple(_) => "tuple".into(),
        Union(_) => "union".into(),
    }
}

/// One step of a flattened place expression (for nested place assignment).
enum PlaceStep {
    Field(String),
    Index(usize),
}

fn as_int(v: &Value) -> Result<i64, Flow> {
    match v {
        Value::Int(n) => Ok(*n),
        // R19 Slice B: SizedInt values are valid integer values; return the raw stored i64.
        // Builtin operations that receive a SizedInt (e.g. abs_i64, to_str, etc.) get the
        // value as i64 and apply their semantics. This keeps the ~102 builtin Int sites
        // working without modification.
        Value::SizedInt { val, .. } => Ok(*val),
        other => panic(format!("expected i64, got {}", other.type_name())),
    }
}
fn as_float(v: &Value) -> Result<f64, Flow> {
    match v {
        Value::Float(f) => Ok(*f),
        other => panic(format!("expected f64, got {}", other.type_name())),
    }
}
fn as_bool(v: &Value) -> Result<bool, Flow> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => panic(format!("expected bool, got {}", other.type_name())),
    }
}
fn as_str(v: &Value) -> Result<&str, Flow> {
    match v {
        Value::Str(s) => Ok(s),
        other => panic(format!("expected str, got {}", other.type_name())),
    }
}
fn as_int_opt(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) => Some(*n),
        Value::SizedInt { val, .. } => Some(*val),
        _ => None,
    }
}
fn as_float_opt(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// Whether deterministic mock-LLM responses are enabled (`AXON_AI_MOCK` set and
/// not "0"/empty). Lets the ASI demos run end-to-end with no API key, no
/// network, and no `asi-runtime` feature — for showcases, CI, and tests.
pub fn ai_mock_enabled() -> bool {
    std::env::var("AXON_AI_MOCK")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// Milliseconds since the Unix epoch.
#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// wasm32 (esp. unknown-unknown / the browser) has no `SystemTime` clock —
/// `SystemTime::now()` PANICS there ("time not implemented"), which would trap
/// `Interp::build` (rng seeding) for EVERY program. Return a fixed 0: the RNG
/// then seeds deterministically (fine for a browser playground; a real clock for
/// `now_ms()`/`temporal_*` will arrive via a JS-import host in the R7c binding).
#[cfg(target_arch = "wasm32")]
fn now_ms() -> i64 {
    0
}

// Goal-directed optimization (run_goal*/hill_climb*/introspection) extracted to
// interp/goal.rs (R0 slice 3). Its methods live in a second `impl Interp` block
// there; inherent methods resolve across split impl blocks, so call sites in
// this file are unchanged. (A `mod` must be at module scope, not inside `impl`.)
mod goal;
// Core tree-walking evaluator (eval/eval_block/eval_call/eval_binop/
// match_pattern) extracted to interp/eval.rs (R0 slice 5). Its methods live in a
// second `impl Interp` block there; inherent methods resolve across split impl
// blocks, so this file's call sites (and eval's calls to call_builtin/call_fn)
// are unchanged.
mod eval;
// The builtin dispatch (`call_builtin`, the ~2400-line `match name`) extracted to
// interp/builtins.rs (R0 slice 6). Moved as ONE method into a second `impl Interp`
// block; its function-local `want`/`ok!` travel with it. eval.rs's call to
// call_builtin and call_builtin's calls to goal.rs/eval.rs methods + the parent's
// private Interp fields all resolve across the split impl blocks.
mod builtins;
// `@[forall]` property-testing harness extracted to interp/proptest.rs (R0
// slice 4). Self-contained — its only entry point is the public `run_property_test`,
// re-exported here at the original `interp::` path for main.rs (no unqualified
// internal call sites in this file, so no `use proptest::*` glob needed).
mod proptest;
pub use proptest::{run_property_test, PropertyOutcome};
// Provenance / audit logging extracted to interp/provenance.rs (R0 slice 1).
mod provenance;
// Bring the moved free fns/types back into scope so existing unqualified
// call sites (json_quote, append_*_jsonl, read_best_input, …) are unchanged,
// and re-export the public API at the original `interp::` path for main.rs.
use provenance::*;
pub use provenance::{
    append_run_start_jsonl, best_recorded_score, find_run_start, provenance_log_path,
    read_ai_calls, read_provenance, set_provenance_source, AiCallRecord, ProvRecord,
    RunStartRecord,
};

/// A pseudo-random `u64` from a process-global xorshift state (seeded from the
/// clock on first use). Single-threaded interpreter, so no CAS needed.
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

/// Global xorshift64 RNG state. `0` means "uninitialized" — the first draw
/// seeds it (see [`rng_seed`]). Explicitly settable via [`set_rand_seed`]
/// (the `srand` builtin) for reproducible runs.
static RNG_STATE: AtomicU64 = AtomicU64::new(0);

/// Initial seed for the RNG. Reproducibility (BUG_HUNT #11 / I-10):
/// uses `AXON_SEED` (parsed as u64) when set for a deterministic run,
/// otherwise time-based entropy. A fixed seed makes every `random_*`,
/// `goal_run_random`, and `goal_run_multistart` result replayable.
fn rng_seed() -> u64 {
    if let Ok(s) = std::env::var("AXON_SEED") {
        if let Ok(n) = s.trim().parse::<u64>() {
            return n | 1; // avoid the 0 "uninitialized" sentinel
        }
    }
    (now_ms() as u64) | 1
}

/// Explicitly set the RNG seed (the `srand(n)` builtin). `n == 0` is mapped
/// to a non-zero sentinel so it doesn't read as "uninitialized".
fn set_rand_seed(n: i64) {
    let s = (n as u64) | 1;
    RNG_STATE.store(s, AtomicOrdering::Relaxed);
}

fn next_rand_u64() -> u64 {
    let mut x = RNG_STATE.load(AtomicOrdering::Relaxed);
    if x == 0 {
        x = rng_seed();
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG_STATE.store(x, AtomicOrdering::Relaxed);
    x
}

/// Render `n` in `base` (2–36), '-'-prefixed when negative.
fn i64_to_radix(n: i64, base: u32) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let neg = n < 0;
    let mut v = (n as i128).unsigned_abs(); // u128 — handles i64::MIN
    let b = base as u128;
    let mut buf = Vec::new();
    while v > 0 {
        buf.push(DIGITS[(v % b) as usize]);
        v /= b;
    }
    if neg {
        buf.push(b'-');
    }
    buf.reverse();
    String::from_utf8(buf).unwrap()
}

/// A numeric value coerced to `f64` for scoring, if it is numeric.
fn numeric_score(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::SizedInt { val, .. } => Some(*val as f64),
        Value::Float(f) => Some(*f),
        // An `@[adaptive]` fn that returns `Uncertain<T>`/`Temporal<T>` (e.g. an
        // AI scorer whose score carries a confidence) scores on its INNER value —
        // the same soft-typing rule as everywhere else. Without this, the
        // optimizer recorded no score for such a fn and `goal_run` silently fell
        // back to the target (no optimization happened at all).
        _ => value::soft_inner(v).and_then(|inner| numeric_score(&inner)),
    }
}

/// Split a total eval budget into (n_starts, evals_per_start) for multistart —
/// roughly sqrt(N) starts so each gets a meaningful local refinement budget.
fn split_budget(total: i64) -> (i64, i64) {
    let t = total.max(1);
    let starts = ((t as f64).sqrt().floor() as i64).max(2).min(t);
    let per = (t / starts).max(1);
    (starts, per)
}

/// The optimization strategy a `#[goal(strategy: …)]` selects (PRD L889-899).
/// A closed set — an unknown name is E1505 (validated by the checker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoalStrategy {
    /// Gradient-style local search (the default). Maps to `run_goal`.
    HillClimb,
    /// Uniform random sampling of the `[lo, hi)` box. Maps to `run_goal_random`.
    Random,
    /// Random restarts + local refinement. Maps to `run_goal_multistart`.
    Multistart,
    /// Generational: sample a population, keep the top-K, mutate around them,
    /// repeat. Good for multi-modal objectives. Maps to `run_goal_tournament`.
    Tournament,
    /// Exploit-biased search: multistart that spends most of its budget
    /// refining the best basin found. (A true Gaussian-process surrogate is
    /// out of v1 scope; this is the honest exploit-heavy approximation —
    /// documented as such, never claimed to be a GP.) Maps to a tournament with
    /// a single elite + heavy local refinement.
    Bayesian,
}

impl GoalStrategy {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "hill_climb" | "hillclimb" => Some(GoalStrategy::HillClimb),
            "random" => Some(GoalStrategy::Random),
            "multistart" => Some(GoalStrategy::Multistart),
            "tournament" => Some(GoalStrategy::Tournament),
            "bayesian" => Some(GoalStrategy::Bayesian),
            _ => None,
        }
    }
}

/// The parsed `#[goal(...)]` attribute (R5 sugar).
struct GoalSpec {
    metric: String,
    target: f64,
    max_evals: i64,
    holdout_set: Vec<i64>,
    strategy: GoalStrategy,
    lo: i64,
    hi: i64,
}

/// Build an `Uncertain { value, confidence }` struct value.
fn make_uncertain(value: Value, confidence: f64) -> Value {
    // `source_tag` (0=user-constructed, 1=AI-sourced, 2=runtime) is a field of the
    // Uncertain struct — the checker lists it as a valid field and codegen builds
    // the 3-field `{value, confidence, source_tag}` layout. Without it here,
    // `u.source_tag` type-checked but panicked at runtime ("no field source_tag")
    // and the interp's 2-field struct diverged from codegen's 3-field one.
    // Default 0 (user-constructed), matching codegen's `source_tag = 0`.
    let mut fields = HashMap::new();
    fields.insert("value".to_string(), value);
    fields.insert("confidence".to_string(), Value::Float(confidence));
    fields.insert("source_tag".to_string(), Value::Int(0));
    Value::Struct {
        name: "Uncertain".to_string(),
        fields,
    }
}

/// Build a `Temporal { value, confidence, horizon_ms, decay, created_ms,
/// valid_until_ms }` struct value. `confidence` is the present trust in the value
/// (1.0 at creation), which `temporal_at` decays as time advances (PRD
/// §"Temporal"). `created_ms` is internal (read by temporal_at/is_valid);
/// `valid_until_ms` = created_ms + horizon_ms is the user-facing expiry timestamp
/// the checker exposes as a field — without it, `t.valid_until_ms` type-checked
/// then panicked "no field valid_until_ms" (a checker-only phantom field).
fn make_temporal(
    value: Value,
    confidence: f64,
    horizon_ms: i64,
    decay: f64,
    created_ms: i64,
) -> Value {
    let mut fields = HashMap::new();
    fields.insert("value".to_string(), value);
    fields.insert(
        "confidence".to_string(),
        Value::Float(confidence.clamp(0.0, 1.0)),
    );
    fields.insert("horizon_ms".to_string(), Value::Int(horizon_ms));
    fields.insert("decay".to_string(), Value::Float(decay));
    fields.insert("created_ms".to_string(), Value::Int(created_ms));
    fields.insert(
        "valid_until_ms".to_string(),
        Value::Int(created_ms.saturating_add(horizon_ms)),
    );
    Value::Struct {
        name: "Temporal".to_string(),
        fields,
    }
}

fn is_i64_type(ty: &crate::ast::AxonType) -> bool {
    matches!(ty, crate::ast::AxonType::Named(n) if n == "i64")
}

/// An `@[adaptive]` fn's return type is i64-SCORED when it's `i64` OR a soft
/// wrapper around i64 (`Uncertain<i64>`/`Temporal<i64>`) — the optimizer reads
/// the score from the inner value (numeric_score unwraps it). Without this, an
/// AI scorer `-> Uncertain<i64>` wasn't recognized as i64-returning, so goal_run
/// never entered the hill-climb and silently returned the target (no optimization).
fn is_i64_scored_ret(ty: &crate::ast::AxonType) -> bool {
    use crate::ast::AxonType::*;
    match ty {
        Named(n) => n == "i64",
        Generic { base, args } if (base == "Uncertain" || base == "Temporal") => {
            args.first().map(is_i64_type).unwrap_or(false)
        }
        _ => false,
    }
}

/// Same as `is_i64_scored_ret` for f64.
fn is_f64_scored_ret(ty: &crate::ast::AxonType) -> bool {
    use crate::ast::AxonType::*;
    match ty {
        Named(n) => n == "f64",
        Generic { base, args } if (base == "Uncertain" || base == "Temporal") => {
            args.first().map(is_f64_type).unwrap_or(false)
        }
        _ => false,
    }
}

fn is_f64_type(ty: &crate::ast::AxonType) -> bool {
    matches!(ty, crate::ast::AxonType::Named(n) if n == "f64")
}

/// Apply a comparison `BinOp` to two floats (used by the `@[verify]` gate).
fn cmp_f64(op: &BinOp, a: f64, b: f64) -> bool {
    match op {
        BinOp::Gt => a > b,
        BinOp::GtEq => a >= b,
        BinOp::Lt => a < b,
        BinOp::LtEq => a <= b,
        BinOp::Eq => a == b,
        BinOp::NotEq => a != b,
        _ => false,
    }
}

/// A scalar `Value` as an `f64` for a `@[verify(value OP K)]` comparison, or
/// `None` for a non-scalar (struct/enum/array/…), which `value` can't bind to.
/// `bool` maps to 0.0/1.0 so `@[verify(value == 1)]` works on a flag.
fn scalar_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::SizedInt { val, .. } => Some(*val as f64),
        Value::Float(f) => Some(*f),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn eval_unary(op: &UnaryOp, v: Value) -> R {
    match (op, v) {
        (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
        (UnaryOp::Neg, Value::SizedInt { val, ty }) => Ok(Value::SizedInt {
            val: val.wrapping_neg(),
            ty,
        }),
        (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
        (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnaryOp::BitNot, Value::Int(n)) => Ok(Value::Int(!n)),
        (UnaryOp::BitNot, Value::SizedInt { val, ty }) => Ok(Value::SizedInt { val: !val, ty }),
        // `&expr` is a no-op at runtime for a value interpreter.
        (UnaryOp::Ref, v) => Ok(v),
        (op, v) => panic(format!("cannot apply {op:?} to {}", v.type_name())),
    }
}

/// Pull `(inner_value, confidence)` out of a value that may or may not be an
/// `Uncertain { value, confidence }` struct. A non-Uncertain value carries an
/// implicit confidence of 1.0 — so `uncertain(5, 0.8) + 3` treats `3` as
/// certain. Returns `None` for a value that isn't Uncertain (the caller then
/// uses it as-is with confidence 1.0). Mirrors codegen's `extract` closure
/// (`asi.rs::emit_binop_uncertain`) so the interpreter and native agree.
// Value formatting + value-level ops extracted to interp/value.rs (R0 slice 2).
mod value;
use value::*;

// ── R19 Slice B — interp.rs-level coercion helpers ───────────────────────────
// These mirror the ones in eval.rs but are needed by call_fn (in this file).

/// Map AxonType → semantic Type for non-i64 integer widths only. Returns None
/// for i64 (already the default Value::Int representation) and non-integers.
fn interp_eval_axon_type_to_width(ty: &crate::ast::AxonType) -> Option<crate::types::Type> {
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
            _ => None,
        },
        _ => None,
    }
}

/// Coerce Int → SizedInt (or re-tag an existing SizedInt). Other values pass
/// through unchanged. Used at the call-arg → param boundary.
fn interp_eval_coerce_to_sized(v: Value, width: crate::types::Type) -> Value {
    match v {
        Value::Int(n) => Value::SizedInt { val: n, ty: width },
        Value::SizedInt { val, .. } => Value::SizedInt { val, ty: width },
        other => other,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> i32 {
        let program = crate::parse_source(src).expect("parse failed");
        run_program(&program)
    }

    #[test]
    fn hello_runs_and_exits_zero() {
        assert_eq!(run(r#"fn main() { println("hi") }"#), 0);
    }

    #[test]
    fn main_returns_exit_code() {
        assert_eq!(run("fn main() -> i64 { 7 }"), 7);
    }

    // ── R15 resume runtime (v0) — suspend/resume across a host driver ──────────
    fn parse(src: &str) -> crate::ast::Program {
        crate::parse_source(src).expect("parse failed")
    }

    #[test]
    fn r15_host_await_single_roundtrip() {
        // B1: the request reaches the host, and the host's reply flows back into
        // the program. host("ab") → "abab"; str_len("abab") = 4.
        let prog = parse(r#"fn main() -> i64 { let r = host_await("ab")  str_len(r) }"#);
        let code = super::run_suspendable(&prog, |req| Some(format!("{req}{req}")));
        assert_eq!(code, 4);
    }

    #[test]
    fn r15_host_await_effects_fire_once_not_per_resume() {
        // B2 (the load-bearing test): the host is called EXACTLY once per
        // host_await — three awaits ⇒ three host calls. A replay-based suspend
        // would re-run the prefix and call the host MORE than three times; the
        // coroutine/thread substrate suspends in place, so it's exactly three.
        let prog = parse(
            "fn main() -> i64 { let a = host_await(\"1\")  let b = host_await(\"2\")  let c = host_await(\"3\")  str_len(a) + str_len(b) + str_len(c) }",
        );
        let mut calls = 0;
        let code = super::run_suspendable(&prog, |_req| {
            calls += 1;
            Some("ok".to_string()) // len 2
        });
        assert_eq!(
            calls, 3,
            "host must be called exactly once per host_await (no replay)"
        );
        assert_eq!(code, 6); // 2 + 2 + 2
    }

    #[test]
    fn r15_host_await_loop_n_times() {
        // B3: a while-loop of host_await — the common interactive shape (an event /
        // prompt loop). The host feeds a different reply each iteration; the
        // program accumulates and the host is called exactly N times (loop count).
        let prog = parse(
            "fn main() -> i64 { let total = 0  let i = 0  while i < 3 { let s = host_await(\"w\")  total = total + str_len(s)  i = i + 1 }  total }",
        );
        let replies = ["ab", "cde", "f"]; // lengths 2, 3, 1
        let mut n = 0;
        let code = super::run_suspendable(&prog, |_req| {
            let r = replies[n].to_string();
            n += 1;
            Some(r)
        });
        assert_eq!(n, 3, "host called once per loop iteration");
        assert_eq!(code, 6, "2 + 3 + 1");
    }

    #[test]
    fn r15_no_await_runs_unchanged() {
        // B4: a program that never suspends runs to completion under the driver,
        // identically to a bare run, with zero host calls.
        let prog = parse("fn main() -> i64 { 2 + 3 }");
        let mut calls = 0;
        let code = super::run_suspendable(&prog, |_| {
            calls += 1;
            Some(String::new())
        });
        assert_eq!(code, 5);
        assert_eq!(calls, 0, "no host_await ⇒ no suspension");
    }

    #[test]
    fn r15_panic_mid_suspend_does_not_hang() {
        // Robustness: a program that ERRORS after a host_await must return cleanly
        // (the host loop ends when the worker drops its channels), never hang. Here
        // `10 / str_len("")` is a runtime div-by-zero (exit 101) after one await.
        let prog =
            parse("fn main() -> i64 { let g = host_await(\"x\")  let z = str_len(\"\")  10 / z }");
        let code = super::run_suspendable(&prog, |_| Some("ok".to_string()));
        assert_eq!(code, 101, "interp panic mid-suspend → exit 101, no hang");
    }

    #[test]
    fn r15_host_await_opt_none_at_eof_terminates_loop() {
        // EOF semantics: `host_await_opt` returns None once the host signals
        // end-of-input (the closure returns None), so a read loop terminates
        // instead of spinning. The host feeds 2 lines then EOF; the program counts
        // the Some replies and stops on None. 2 inputs ⇒ exit 2.
        let prog = parse(
            "fn main() -> i64 { let n = 0  let go = 1  while go == 1 { match host_await_opt(\"?\") { None => { go = 0 } Some(s) => { n = n + 1 } } }  n }",
        );
        let mut fed = 0;
        let code = super::run_suspendable(&prog, |_| {
            fed += 1;
            if fed <= 2 {
                Some("x".to_string())
            } else {
                None
            } // 2 lines, then EOF
        });
        assert_eq!(code, 2, "two Some replies then None ⇒ loop stops at 2");
    }

    #[test]
    fn r15_host_await_collapses_eof_to_empty_string() {
        // The simple str form maps EOF (host None) to "" — back-compat for
        // fixed-exchange programs that don't distinguish end-of-input.
        let prog = parse(r#"fn main() -> i64 { let r = host_await("x")  str_len(r) }"#);
        let code = super::run_suspendable(&prog, |_| None); // immediate EOF
        assert_eq!(code, 0, "EOF ⇒ host_await returns \"\" ⇒ len 0");
    }

    #[test]
    fn r15_host_await_without_host_errors_cleanly() {
        // A bare `run` (no driver) must error gracefully (exit 101), not hang.
        let prog = parse(r#"fn main() -> i64 { let r = host_await("x")  str_len(r) }"#);
        assert_eq!(super::run_program(&prog), 101);
    }

    // ── R15 Slice 2 — arbitrary-`Value` payloads survive a suspend ─────────────

    #[test]
    fn r15_slice2_dict_payload_round_trips() {
        // The case that did NOT work before Slice 2: a DICT crosses the suspend.
        // The program builds a dict, yields it via host_await_val, the host inspects
        // the request (a dict), and replies with a (different) dict; the program
        // reads a key out of the reply. Proves the !Send `Value::Dict` deep-clones
        // across the worker thread and reconstructs on both directions.
        let prog = parse(
            "fn main() -> i64 { let d = dict_new()  dict_set(d, \"a\", 7)  let r = host_await_val(d)  dict_get_or(r, \"b\", 0) }",
        );
        let mut saw_request_a = 0;
        let code = super::run_suspendable_values(&prog, |req| {
            // The request must be a Dict carrying a=7 (the deep-clone preserved it).
            if let SendValue::Dict(entries) = &req {
                for (k, v) in entries {
                    if k == "a" {
                        if let SendValue::Int(n) = v {
                            saw_request_a = *n;
                        }
                    }
                }
            }
            // Reply with a fresh dict {b: 42}.
            Some(SendValue::Dict(vec![("b".to_string(), SendValue::Int(42))]))
        });
        assert_eq!(saw_request_a, 7, "host saw the request dict's a=7");
        assert_eq!(code, 42, "program read b=42 from the reply dict");
    }

    #[test]
    fn r15_slice2_struct_payload_round_trips() {
        // A STRUCT crosses the suspend: the program builds a Point, yields it, the
        // host reads its fields and replies with a Point whose x is the sum; the
        // program reads the reply's x field.
        let prog = parse(
            "type Point = { x: i64, y: i64 }\nfn main() -> i64 { let p = Point { x: 3, y: 4 }  let r = host_await_val(p)  r.x }",
        );
        let mut sum = 0;
        let code = super::run_suspendable_values(&prog, |req| {
            if let SendValue::Struct { name, fields } = &req {
                assert_eq!(name, "Point");
                let mut x = 0;
                let mut y = 0;
                for (k, v) in fields {
                    if let SendValue::Int(n) = v {
                        if k == "x" {
                            x = *n;
                        } else if k == "y" {
                            y = *n;
                        }
                    }
                }
                sum = x + y;
            }
            Some(SendValue::Struct {
                name: "Point".to_string(),
                fields: vec![
                    ("x".to_string(), SendValue::Int(sum)),
                    ("y".to_string(), SendValue::Int(0)),
                ],
            })
        });
        assert_eq!(sum, 7, "host summed the struct fields");
        assert_eq!(code, 7, "program read x=7 back from the reply struct");
    }

    #[test]
    fn r15_slice2_enum_and_tuple_payload_round_trip() {
        // Enum + tuple payloads cross too (nested composite). The program yields an
        // Option (an enum-like sum) and the host replies with Some(99).
        let prog = parse(
            "fn main() -> i64 { let r = host_await_val_opt(Some(5))  match r { Some(n) => n  None => -1 } }",
        );
        let code = super::run_suspendable_values(&prog, |req| {
            // Request is Some(5).
            assert!(matches!(&req, SendValue::Some(b) if matches!(**b, SendValue::Int(5))));
            Some(SendValue::Int(99))
        });
        assert_eq!(code, 99, "program unwrapped Some(99) from the reply");
    }

    #[test]
    fn r15_slice2_chan_payload_is_refused_not_corrupted() {
        // SOUNDNESS BOUNDARY: a Chan (identity-shared mutable state) cannot cross a
        // suspend without silently losing its sharing, so it is REFUSED (a clean
        // runtime panic, exit 101) rather than deep-cloned into a disconnected copy.
        let prog = parse(
            "fn main() -> i64 { let c = chan<i64>()  let r = host_await_val(c)  0 }",
        );
        // The host is never reached (the refusal happens at the boundary, worker-side).
        let mut host_calls = 0;
        let code = super::run_suspendable_values(&prog, |_req| {
            host_calls += 1;
            Some(SendValue::Int(0))
        });
        assert_eq!(code, 101, "Chan payload → clean refusal (exit 101), not corruption");
        assert_eq!(host_calls, 0, "the refused payload never reached the host");
    }

    #[test]
    fn r15_slice2_str_form_still_works_through_value_substrate() {
        // Regression: the str-typed host_await still works now that the substrate
        // carries SendValue (str crosses as SendValue::Str). The B1 case, unchanged.
        let prog = parse(r#"fn main() -> i64 { let r = host_await("ab")  str_len(r) }"#);
        let code = super::run_suspendable(&prog, |req| Some(format!("{req}{req}")));
        assert_eq!(code, 4, "str host_await round-trips through the SendValue channel");
    }

    #[test]
    fn r15_slice2_send_value_round_trip_is_lossless() {
        // Unit-level: from_value ∘ into_value preserves a deeply nested composite
        // (dict in struct in array). Chan-free, so it must succeed and reconstruct
        // an equal-shaped Value.
        let mut inner = std::collections::BTreeMap::new();
        inner.insert("k".to_string(), Value::Int(9));
        let dict = Value::Dict(Rc::new(RefCell::new(inner)));
        let mut sf = HashMap::new();
        sf.insert("d".to_string(), dict);
        sf.insert("n".to_string(), Value::Int(1));
        let s = Value::Struct { name: "S".to_string(), fields: sf };
        let v = Value::Array(vec![s, Value::Str("hi".to_string())]);
        let sv = SendValue::from_value(&v).expect("Chan-free ⇒ sendable");
        let back = sv.into_value();
        // Spot-check the reconstructed shape.
        if let Value::Array(xs) = &back {
            assert_eq!(xs.len(), 2);
            if let Value::Struct { name, fields } = &xs[0] {
                assert_eq!(name, "S");
                assert!(matches!(fields.get("n"), Some(Value::Int(1))));
                if let Some(Value::Dict(d)) = fields.get("d") {
                    assert!(matches!(d.borrow().get("k"), Some(Value::Int(9))));
                } else {
                    panic!("dict field lost");
                }
            } else {
                panic!("struct lost");
            }
        } else {
            panic!("array lost");
        }
    }

    #[test]
    fn fmt_g_matches_c_printf_six_g() {
        // R1f slice 2b: the interpreter's fmt_g must match C's `%.6g` (the format
        // the native codegen path emits via snprintf) so I-2 holds — the
        // differential fuzzer (fuzz_parity.sh) caught the original divergence.
        // Pins both the mantissa trailing-zero trim AND the signed two-digit
        // exponent in the scientific-notation branch, plus -0.0 normalization.
        assert_eq!(fmt_g(1_000_000.0), "1e+06");
        assert_eq!(fmt_g(1_234_567.0), "1.23457e+06");
        assert_eq!(fmt_g(9_999_999.0), "1e+07");
        assert_eq!(fmt_g(0.000_000_1), "1e-07");
        assert_eq!(fmt_g(-1_234_567.0), "-1.23457e+06");
        assert_eq!(fmt_g(1.5e15), "1.5e+15");
        assert_eq!(fmt_g(-2.5e-12), "-2.5e-12");
        // Non-scientific range is unchanged.
        assert_eq!(fmt_g(123_456.0), "123456");
        assert_eq!(fmt_g(0.0001), "0.0001");
        assert_eq!(fmt_g(2.71875), "2.71875");
        // Zero (and -0.0) normalize to "0".
        assert_eq!(fmt_g(0.0), "0");
        assert_eq!(fmt_g(-0.0), "0");
    }

    #[test]
    fn arithmetic_and_while() {
        let src = r#"
            fn main() -> i64 {
                let acc = 0
                let i = 1
                while i <= 10 { acc = acc + i  i = i + 1 }
                acc
            }
        "#;
        assert_eq!(run(src), 55);
    }

    #[test]
    fn recursion() {
        let src = r#"
            fn fib(n: i64) -> i64 {
                if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
            }
            fn main() -> i64 { fib(10) }
        "#;
        assert_eq!(run(src), 55);
    }

    // BUG_HUNT #22: parse_int error messages are specific, not generic.
    #[test]
    fn parse_int_radix_prefix_errs_with_hint() {
        // Returns 1 (Err arm) for hex; the message is checked via the CLI test.
        let src = r#"
            fn main() -> i64 {
                match parse_int("0xFF") { Ok(_) => 0  Err(_) => 1 }
            }
        "#;
        assert_eq!(run(src), 1, "hex literal must Err (base-10 only)");
    }

    #[test]
    fn parse_int_still_trims_and_parses_decimal() {
        let src = r#"
            fn main() -> i64 {
                match parse_int("  -123  ") { Ok(n) => n  Err(_) => 0 }
            }
        "#;
        assert_eq!(run(src), -123, "leading/trailing space still trims");
    }

    // BUG_HUNT #25: the generated deploy-gate symbol must not leak to users.
    #[test]
    fn verify_label_hides_generated_deploy_gate_name() {
        assert_eq!(verify_fn_label("assert_deployable"), "the deploy gate");
        assert!(
            !verify_fn_label("assert_deployable").contains("assert_deployable"),
            "the internal symbol must not appear in the founder-facing label"
        );
    }

    #[test]
    fn verify_label_keeps_author_named_functions() {
        // An author's own @[verify] fn keeps its name — it's meaningful to them.
        assert_eq!(verify_fn_label("safety_gate"), "`safety_gate`");
        assert_eq!(verify_fn_label("gate"), "`gate`");
    }

    // BUG_HUNT #27: random_i64 rejects inverted bounds and stays in range.
    #[test]
    fn random_i64_valid_bounds_stay_in_range() {
        // Deterministic via srand; sample several draws, all in [lo, hi).
        let src = r#"
            fn main() -> i64 {
                srand(42)
                let bad = 0
                let i = 0
                while i < 50 {
                    let r = random_i64(10, 20)
                    if r < 10 { bad = bad + 1 }
                    if r >= 20 { bad = bad + 1 }
                    i = i + 1
                }
                bad
            }
        "#;
        assert_eq!(run(src), 0, "all draws must fall in [10, 20)");
    }

    #[test]
    fn random_i64_inverted_bounds_is_panic() {
        let src = "fn main() -> i64 { random_i64(20, 10) }";
        // Graceful panic → exit 101 (not a silent return of `lo`).
        assert_eq!(run(src), 101);
    }

    // BUG_HUNT #28: AXON_MAX_DEPTH resolution and stack-coupling are pure and
    // unit-testable without mutating process-global env.
    #[test]
    fn max_depth_defaults_when_unset_or_malformed() {
        assert_eq!(max_depth_from_env(None), RECURSION_LIMIT);
        assert_eq!(max_depth_from_env(Some("")), RECURSION_LIMIT);
        assert_eq!(max_depth_from_env(Some("banana")), RECURSION_LIMIT);
        assert_eq!(
            max_depth_from_env(Some("0")),
            RECURSION_LIMIT,
            "zero is not a useful limit"
        );
        assert_eq!(
            max_depth_from_env(Some("-5")),
            RECURSION_LIMIT,
            "negatives don't parse as usize"
        );
    }

    #[test]
    fn max_depth_honors_valid_value_and_trims() {
        assert_eq!(max_depth_from_env(Some("9000")), 9000);
        assert_eq!(max_depth_from_env(Some("  12345  ")), 12345);
    }

    #[test]
    fn max_depth_clamps_to_ceiling() {
        assert_eq!(max_depth_from_env(Some("999999999999")), MAX_DEPTH_CEILING);
    }

    #[test]
    fn stack_grows_with_depth_but_never_below_floor() {
        // Shallow limits keep the historical 1 GiB floor. The crossover is
        // MIN_STACK_BYTES / STACK_BYTES_PER_FRAME frames; below it, floored.
        let crossover = MIN_STACK_BYTES / STACK_BYTES_PER_FRAME; // 4096
        assert_eq!(stack_size_for_depth(1), MIN_STACK_BYTES);
        assert_eq!(stack_size_for_depth(crossover), MIN_STACK_BYTES);
        // Past the crossover, the stack scales with depth so the guard stays
        // ahead of a real overflow.
        let deep = 100_000;
        assert!(deep > crossover);
        assert_eq!(stack_size_for_depth(deep), deep * STACK_BYTES_PER_FRAME);
        assert!(stack_size_for_depth(deep) > MIN_STACK_BYTES);
        // The default limit (6000 > 4096) already scales above the floor.
        assert!(stack_size_for_depth(RECURSION_LIMIT) > MIN_STACK_BYTES);
        // The ceiling can't overflow the multiply (saturating) — it produces a
        // finite budget at or above the floor.
        assert!(stack_size_for_depth(MAX_DEPTH_CEILING) >= MIN_STACK_BYTES);
    }

    // BUG_HUNT #20: dict_to_str must return Result<str,str>, not panic the
    // host, when a key/value can't be represented in the line format. The
    // caller can then recover.
    #[test]
    fn dict_to_str_bad_key_returns_err_not_panic() {
        let src = r#"
            fn main() -> i64 {
                let d = dict_new()
                dict_set(d, "a=b", "v")
                match dict_to_str(d) { Ok(_) => 0  Err(_) => 7 }
            }
        "#;
        // 7 = the Err arm ran. A host panic would exit 101 instead.
        assert_eq!(run(src), 7);
    }

    #[test]
    fn dict_to_str_newline_value_returns_err_not_panic() {
        let src = r#"
            fn main() -> i64 {
                let d = dict_new()
                dict_set(d, "k", "line1\nline2")
                match dict_to_str(d) { Ok(_) => 0  Err(_) => 7 }
            }
        "#;
        assert_eq!(run(src), 7);
    }

    #[test]
    fn dict_to_str_clean_dict_round_trips() {
        let src = r#"
            fn main() -> i64 {
                let d = dict_new()
                dict_set(d, "k", "v")
                match dict_to_str(d) {
                    Ok(s) => {
                        let back = dict_from_str(s)
                        match dict_get(back, "k") { Some(_) => 1  None => -1 }
                    }
                    Err(_) => -2
                }
            }
        "#;
        assert_eq!(run(src), 1);
    }

    #[test]
    fn result_match_and_question() {
        let src = r#"
            fn half(n: i64) -> Result<i64, str> {
                if n % 2 == 0 { Ok(n / 2) } else { Err("odd") }
            }
            fn doit(n: i64) -> Result<i64, str> {
                let h = half(n)?
                Ok(h + 1)
            }
            fn main() -> i64 {
                match doit(10) { Ok(v) => v  Err(_) => -1 }
            }
        "#;
        assert_eq!(run(src), 6);
    }

    #[test]
    fn enum_match() {
        let src = r#"
            enum Shape { Circle { r: i64 }, Square { s: i64 } }
            fn area(x: Shape) -> i64 {
                match x { Shape::Circle { r } => r * r  Shape::Square { s } => s * s }
            }
            fn main() -> i64 { area(Shape::Square { s: 4 }) }
        "#;
        assert_eq!(run(src), 16);
    }

    #[test]
    fn closures_capture() {
        let src = r#"
            fn make_adder(n: i64) -> (i64) -> i64 { (x: i64) => x + n }
            fn main() -> i64 {
                let add10 = make_adder(10)
                add10(5)
            }
        "#;
        assert_eq!(run(src), 15);
    }

    #[test]
    fn for_loop_break_continue() {
        let src = r#"
            fn main() -> i64 {
                let total = 0
                for i in 0..10 {
                    if i == 5 { break }
                    total = total + i
                }
                total
            }
        "#;
        assert_eq!(run(src), 10); // 0+1+2+3+4
    }

    #[test]
    fn struct_field_access() {
        let src = r#"
            type Point = { x: i64, y: i64 }
            fn main() -> i64 {
                let p = Point { x: 3, y: 4 }
                p.x + p.y
            }
        "#;
        assert_eq!(run(src), 7);
    }

    // ── ASI (M2) ──────────────────────────────────────────────────────────

    #[test]
    fn uncertain_construction_and_field_access() {
        let src = r#"
            fn main() -> i64 {
                let u = uncertain_new(42, 0.75)
                u.value
            }
        "#;
        assert_eq!(run(src), 42);
    }

    #[test]
    fn numeric_conversions() {
        let src = r#"
            fn main() -> i64 { f64_to_i64(i64_to_f64(9) + 0.9) }
        "#;
        assert_eq!(run(src), 9); // 9.9 truncates toward zero
    }

    #[test]
    fn goal_run_hill_climbs_adaptive_fn() {
        // score(x) = 100 - (x-7)^2, peak 100 at x=7. goal_run should climb to it.
        let src = r#"
            @[adaptive(metric: s, target: 100)]
            fn score(x: i64) -> i64 {
                let d = x - 7
                100 - d * d
            }
            fn main() -> i64 {
                let best = goal_run("score", 100.0, 64)
                f64_to_i64(best)
            }
        "#;
        assert_eq!(run(src), 100);
    }

    #[test]
    fn goal_run_retrospective_picks_closest() {
        // measure returns f64 → not hill-climb-eligible → retrospective path
        // over logged records [20, 40, 60]; closest to 50 is 40 (earliest of tie).
        let src = r#"
            @[adaptive(metric: m, target: 50)]
            fn measure(x: i64) -> f64 { i64_to_f64(x) }
            fn main() -> i64 {
                let _ = measure(20)
                let _ = measure(40)
                let _ = measure(60)
                f64_to_i64(goal_run("measure", 50.0, 100))
            }
        "#;
        assert_eq!(run(src), 40);
    }

    #[test]
    fn goal_run_errors_on_unknown_name() {
        // BUG_HUNT #19 / I-9: a name that is neither a defined fn nor in the
        // provenance store is a typo. It must error (exit 101), NOT silently
        // return the target as if the goal were achieved. (This test
        // previously asserted the bug — return-target — as correct.)
        let src = r#"
            fn main() -> i64 { f64_to_i64(goal_run("never_called", 70.0, 20)) }
        "#;
        assert_eq!(run(src), 101);
    }

    #[test]
    fn goal_run_retrospective_lookup_still_works() {
        // The legitimate fallthrough: an adaptive fn that HAS run can be
        // re-queried by name (max_evals=0) and returns its best observed.
        let src = r#"
            @[adaptive]
            fn s(x: i64) -> i64 { x }
            fn main() -> i64 {
                let _ = goal_run("s", 100.0, 20)
                f64_to_i64(goal_run("s", 100.0, 0))
            }
        "#;
        assert_eq!(run(src), 100);
    }

    #[test]
    fn verify_gate_passes_when_confident() {
        let src = r#"
            @[verify(confidence >= 0.8)]
            fn gate(c: f64) -> Uncertain<i64> { uncertain_dyn_i64(1, c) }
            fn main() -> i64 {
                let u = gate(0.9)
                u.value
            }
        "#;
        assert_eq!(run(src), 1);
    }

    #[test]
    fn verify_gate_panics_when_underconfident() {
        let src = r#"
            @[verify(confidence >= 0.8)]
            fn gate(c: f64) -> Uncertain<i64> { uncertain_dyn_i64(1, c) }
            fn main() -> i64 {
                let u = gate(0.5)
                u.value
            }
        "#;
        // verify gate fires → distinct policy exit code 3, NOT a crash 101
        // (BUG_HUNT #26).
        assert_eq!(run(src), VERIFY_FAILED_EXIT_CODE);
    }

    #[test]
    fn extended_math_builtins() {
        // clamp_i64 / sign_i64 / pow_i64 / min_f64 / max_f64 / clamp_f64
        let src = r#"
            fn main() -> i64 {
                let i = clamp_i64(150, 0, 100) + clamp_i64(-5, 0, 100) + sign_i64(-7) + pow_i64(2, 10)
                // 100 + 0 + (-1) + 1024 = 1123
                let f = f64_to_i64(min_f64(3.5, 2.5) + max_f64(1.0, 4.0) + clamp_f64(9.9, 0.0, 5.0))
                // 2.5 + 4.0 + 5.0 = 11.5 -> 11
                i + f
            }
        "#;
        assert_eq!(run(src), 1134);
    }

    #[test]
    fn more_builtins_coverage() {
        // sqrt_f64 / round_f64 / str_pad_start / str_pad_end / i64_to_str_radix / uncertain_new_f64
        let src = r#"
            fn main() -> i64 {
                let a = f64_to_i64(sqrt_f64(144.0))                  // 12
                let b = f64_to_i64(round_f64(2.6))                   // 3
                let p = str_len(str_pad_start("7", 4, "0"))          // "0007" -> 4
                let q = str_len(str_pad_end("7", 2, "x"))            // "7x" -> 2
                let r = if str_eq(i64_to_str_radix(255, 16), "ff") { 1 } else { 0 } // 1
                let u = f64_to_i64(uncertain_new_f64(5.0, 0.9).value) // 5
                a + b + p + q + r + u
            }
        "#;
        assert_eq!(run(src), 27);
    }

    #[test]
    fn ai_mock_mode_returns_ok() {
        // With AXON_AI_MOCK set, ai_complete returns Ok(non-empty) with no key /
        // network / asi-runtime feature. (No other test reads this env var.)
        std::env::set_var("AXON_AI_MOCK", "1");
        let n = run(r#"
            fn main() -> i64 {
                match ai_complete("anything") {
                    Ok(s) => str_len(s)
                    Err(_) => -1
                }
            }
        "#);
        std::env::remove_var("AXON_AI_MOCK");
        assert!(
            n > 0,
            "mock ai_complete should return Ok(non-empty), got {n}"
        );
    }

    #[test]
    fn goal_demos_pure_outcomes() {
        // Regression lock for the key-free goal demos: prose → .ax → run,
        // pinning each gate path. (No LLM / no env — the mock-requiring goals
        // hello/flagship are exercised separately.)
        let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/goals/");
        for (file, expected) in [
            ("optimize-goal.md", 0), // deploys
            ("compose-goal.md", 0),  // deploys (prelude-composed score)
            // enforced confidence gate blocks → distinct verify-failed code 3,
            // not a crash 101 (BUG_HUNT #26).
            ("verified-goal.md", VERIFY_FAILED_EXIT_CODE),
            // redteam gate blocks → the SAME policy-rejection code as the verify
            // gate (3), not 1 — every deploy-gate rejection is one exit class
            // (BUG_HUNT #34 surface follow-on to #26).
            ("redteam-goal.md", VERIFY_FAILED_EXIT_CODE),
        ] {
            let md = std::fs::read_to_string(format!("{base}{file}"))
                .unwrap_or_else(|e| panic!("read {file}: {e}"));
            let goal = axon_surface::parser::GoalFile::parse(&md)
                .unwrap_or_else(|e| panic!("parse {file}: {e}"));
            let ax =
                axon_surface::compile::emit(&goal).unwrap_or_else(|e| panic!("emit {file}: {e}"));
            let program =
                crate::parse_source(&ax).unwrap_or_else(|e| panic!("parse .ax for {file}: {e}"));
            let code = run_program(&program);
            assert_eq!(
                code, expected,
                "{file}: expected exit {expected}, got {code}"
            );
        }
    }

    #[test]
    fn sandbox_run_enforces_effect_ceiling_at_runtime_f5() {
        // F5 gate: sandbox_run must deny a builtin whose effect row is outside
        // the sandbox's declared ceiling (exit 8, SANDBOX_VIOLATION_EXIT_CODE),
        // and must allow builtins whose effects are within the ceiling.

        // Case 1: sandbox allows only "IO" — random_i64 ("Random") is denied.
        let src_denied = r#"
            fn noisy(_x: i64) -> i64 { random_i64(1, 100) }
            fn main() -> i64 {
                let p = principal_root("test", false, false, false, 100)
                let sb = sandbox_create(p, "IO")
                sandbox_run(sb, "noisy", 0)
            }
        "#;
        assert_eq!(
            run(src_denied),
            SANDBOX_VIOLATION_EXIT_CODE,
            "sandbox_run should exit {} when the fn calls random_i64 but Random is not allowed",
            SANDBOX_VIOLATION_EXIT_CODE
        );

        // Case 2: sandbox allows "Random" — random_i64 is permitted; result is non-error.
        let src_allowed = r#"
            fn make_rand(_x: i64) -> i64 {
                let r = random_i64(1, 10)
                r
            }
            fn main() -> i64 {
                let p = principal_root("test2", false, false, false, 100)
                let sb = sandbox_create(p, "Random")
                let v = sandbox_run(sb, "make_rand", 0)
                if v >= 1 && v <= 10 { 0 } else { 1 }
            }
        "#;
        std::env::set_var("AXON_SEED", "42");
        let result = run(src_allowed);
        std::env::remove_var("AXON_SEED");
        assert_eq!(
            result, 0,
            "sandbox_run should succeed and return a value in [1,10] when Random is allowed"
        );
    }

    #[test]
    fn sandbox_create_and_run_pure_fn_f5() {
        // A pure fn (no effects) always runs inside any sandbox, including an empty ceiling.
        // sandbox_run always passes 1 i64 arg, so the fn must accept it.
        let src = r#"
            fn double(x: i64) -> i64 { x * 2 }
            fn main() -> i64 {
                let p = principal_root("pure", false, false, false, 100)
                let sb = sandbox_create(p, "")
                sandbox_run(sb, "double", 21)
            }
        "#;
        assert_eq!(
            run(src),
            42,
            "pure fn should run inside an empty-ceiling sandbox"
        );
    }

    #[test]
    fn all_example_ax_files_parse() {
        // Broad regression guard: every .ax under examples/ must parse (covers
        // the basic examples, the asi "public face", stdlib, and modular libs).
        fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        collect(&p, out);
                    } else if p.extension().map(|x| x == "ax").unwrap_or(false) {
                        out.push(p);
                    }
                }
            }
        }
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples"));
        let mut files = Vec::new();
        collect(root, &mut files);
        assert!(
            files.len() >= 20,
            "expected many example .ax files, found {}",
            files.len()
        );
        for f in &files {
            let src =
                std::fs::read_to_string(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
            if let Err(e) = crate::parse_source(&src) {
                panic!("{} failed to parse: {e}", f.display());
            }
        }
    }
}
