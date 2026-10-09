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
    /// R21 — exact fixed-point decimal: i128 mantissa at `decimal::SCALE` (9 dp).
    /// Money-safe: exact arithmetic, no binary floating error.
    Decimal(i128),
    Bool(bool),
    /// String value. Same VALUE-semantics-over-shared-storage contract as
    /// `Array`: cloning a `Value` (env lookup, argument passing, `len(s)`) bumps
    /// a refcount instead of copying the bytes, and the only in-place write —
    /// `s + t` appending to an operand nobody else holds — goes through
    /// [`Rc::make_mut`]. `Rc<String>` rather than `Rc<str>` so that append can
    /// grow the buffer in place (amortized O(len t)) instead of reallocating
    /// (AX-31: a plain `String` made every string read O(len)).
    Str(Rc<String>),
    Unit,
    /// Array value. VALUE semantics, shared representation: cloning a `Value`
    /// (env lookup, argument passing, a struct field read) bumps a refcount
    /// instead of deep-copying the elements, and every in-place write goes
    /// through [`Rc::make_mut`], which copies only when the vector is shared.
    /// So `let b = a; b[0] = 9` still leaves `a` untouched, while `a[i] = v` on
    /// a uniquely owned binding is O(1) (AX-06: the deep copy made index reads
    /// and writes O(len), turning a sieve quadratic).
    Array(Rc<Vec<Value>>),
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
    ///
    /// AUDIT T40 (findings F094 / P5-16 / DOC-02). `captured` used to be a plain
    /// `HashMap<String, Value>` — a fresh clone per call — so an assignment to a
    /// captured binding inside the lambda was silently DROPPED when the call
    /// returned. Native codegen heap-allocates the capture and the write
    /// persists, so the same source printed different answers on the two
    /// engines with no error from either:
    ///
    ///   let n = 0; let bump = || { n = n + 1  n }
    ///   interp:  call1=1 call2=1 call3=1   outer n=0
    ///   native:  call1=1 call2=2 call3=3   outer n=0
    ///
    /// The Rc/RefCell makes the capture PERSISTENT ACROSS CALLS of this closure
    /// (matching codegen) while still being a by-value snapshot of the defining
    /// scope — note `outer n=0` on both engines: the outer binding is not
    /// aliased. Cloning a closure value shares the same cell, which is what
    /// makes `let b = bump` observe the same counter.
    Closure {
        params: Vec<String>,
        body: Box<Expr>,
        captured: Rc<RefCell<HashMap<String, Value>>>,
        /// The `fn(..) -> ..` types this REFERENCE crossed (outermost =
        /// latest), cast on every call — gradual typing's function proxy,
        /// per reference, so one closure used at two types is not confused
        /// (C9 round 4, PSV-1, amendment 53; see `interp/conform.rs`).
        contract: Option<Rc<conform::Contract>>,
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
            Value::Decimal(_) => "Decimal".into(),
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

/// One channel's entry: the channel, the element contracts it was stamped
/// with, and whether OPERATOR code created it (amendment 72).
type ChanEntry = (
    std::rc::Weak<RefCell<VecDeque<Value>>>,
    Vec<Rc<conform::Contract>>,
    bool,
);
/// Channel address → its entry.
type ChanContracts = HashMap<usize, ChanEntry>;

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
    ///
    /// The `usize` is the handler-stack index of the frame whose arm ran: only
    /// the `with` block that pushed THAT frame may catch it. It used to be caught
    /// by the nearest `with` of any kind, so a `with` in candidate code could
    /// swallow a completion aimed at the operator's handler and keep running.
    HandlerDone(Value, usize),
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

/// The enforcement ledger's block, and the shell's.
///
/// 2 through 15 belong to `governance/EXIT_CODES.md` — 3 verify, 4 kill-switch,
/// 6 refinement, 8 sandbox, 11 replay-divergence, up to its own "next free: 16".
/// 101 is a panic. Everything else in 0..=255 is a program's to use.
///
/// 126 and up are the SHELL's by convention (not-executable, not-found,
/// killed-by-signal-N) and are deliberately NOT reserved here: that is a
/// convention about how a shell reports its own failures, not a claim this
/// project makes on the number, and ordinary answers land there (a test summing
/// to 220 is not making a claim about signals).
const LEDGER_TOP: i64 = 15;

/// A status the program STATED, via `exit(n)`.
///
/// Stating a status is a deliberate act, so the ledger vocabulary is available:
/// a userland deploy gate that has decided to reject may say `exit(3)` and mean
/// the same "policy rejection" the `@[verify]` gate means. That is this repo's
/// own design (BUG_HUNT #26/#34 — every deploy-gate rejection is one exit
/// class), and taking the vocabulary away from userland would break it.
///
/// What is still refused is a value that is not a status at all: a status is one
/// byte, and `exit(3240)` would be observed as 168 — a number the program never
/// mentioned.
pub fn stated_exit_status(n: i64) -> (i32, Option<String>) {
    if (0..=255).contains(&n) {
        return (n as i32, None);
    }
    (
        1,
        Some(format!(
            "axon: exit({n}) is not a status — a status is one byte, so the caller would have \
             seen {}. Exiting 1 instead; pass a value in 0..=255.",
            n.rem_euclid(256)
        )),
    )
}

/// A value that fell out of `main`, which is an ANSWER, not a status.
///
/// `fn main() -> i64 { … result }` is the shape a program takes when it computes
/// something and hands it back. Passing that through to the process status
/// unchanged went wrong two ways, both measured rather than imagined:
///
/// * **Silent truncation.** `3240` was observed by the caller as `168`. The
///   number the program produced was not the number anyone saw, and nothing said
///   so. (A benchmark program computed its answer correctly, printed it,
///   returned it, and scored as a failure.)
/// * **Impersonating a guard.** A program whose answer happens to be 6 is
///   indistinguishable from a refinement violation, and 11 from a replay
///   divergence. A supervisor branches on that number precisely because it is
///   supposed to mean a guard fired; arithmetic could forge it.
///
/// Both become status 1 — which is what a nonzero return meant anyway — with the
/// reason on stderr. Ordinary values pass through, so `main() -> i64 { 49 }`
/// still exits 49; a program that MEANS a ledger code says so with `exit(n)`,
/// where the intent is explicit and is honoured.
///
/// MIRRORED in `axon-rt` (`__axon_main_status`) for the native engine. The two
/// are held together by `scripts/exit_code_parity.sh`, not by shared code — the
/// runtime crate deliberately depends on nothing.
pub fn returned_exit_status(n: i64) -> (i32, Option<String>) {
    let advice = "print it (`println(to_str(v))`) and return 0; if you MEAN a status, state it \
                  with `exit(n)`, which is honoured as written — see governance/EXIT_CODES.md";
    if n == 0 || n == 1 {
        return (n as i32, None);
    }
    if (2..=LEDGER_TOP).contains(&n) || n == RUNTIME_PANIC_EXIT_CODE as i64 {
        return (
            1,
            Some(format!(
                "axon: `main` returned {n}, and {n} is RESERVED — the exit-code ledger assigns \
                 it, so exiting with it would make this run indistinguishable from a guard \
                 firing. Exiting 1 instead; {advice}"
            )),
        );
    }
    if !(0..=255).contains(&n) {
        return (
            1,
            Some(format!(
                "axon: `main` returned {n}, which is not a status — a status is one byte, so the \
                 caller would have seen {}. Exiting 1 instead; {advice}",
                n.rem_euclid(256)
            )),
        );
    }
    (n as i32, None)
}

/// A panic is a crash, i.e. a bug — distinct from every enforcement code, which
/// is a guard doing its job. Named here so [`returned_exit_status`] can refuse to
/// let a computed value impersonate one.
pub const RUNTIME_PANIC_EXIT_CODE: i32 = 101;

type R = Result<Value, Flow>;

/// Loop control never leaves the frame it was written in. A `break`/
/// `continue` that reaches the edge of a function call, a closure call, a
/// refinement or `@[verify]` predicate, or an effect-handler arm has no loop of
/// its own there: it is an error AT that edge, never a jump in whatever loop
/// the surrounding code is running. Without this, candidate code ended an
/// operator test's loop before its assertions ran and the verdict was signed
/// (v0.22 G01 final reviews, FG-063/064/065).
/// A FRAME EDGE is an allowlist, not a list of known escapes (Protected Check
/// Isolation 7/8/10/13). A call — named fn or closure — and a predicate
/// evaluation may end only with a value or with an ABORTIVE flow that
/// terminates or fails the program. Every flow that TRANSFERS control to some
/// enclosing construct (`return`, `break`, `continue`, `resume`) is meaningful
/// only inside the frame that raised it; leaving the frame is an error here.
///
/// Escapes used to be closed one at a time — `break` out of a callee (FG-063/
/// 064), out of a predicate (FG-065), then `return` out of a predicate and a
/// smuggled `resume` closure (PCI candidate-1 review): each let a candidate end
/// the operator's test early as a normal completion. The match has no wildcard,
/// so a new `Flow` variant must be classified here before the crate compiles.
pub(crate) fn contain_frame(r: R, site: &str) -> R {
    match r {
        Ok(v) => Ok(v),
        Err(f) => match f {
            // Transfers: meaningful only inside the frame that raised them.
            Flow::Return(_) => panic(format!("`return` escaped {site}")),
            Flow::Break | Flow::Continue => {
                panic(format!("`break`/`continue` outside a loop in {site}"))
            }
            Flow::Resume(_) => panic(format!("`resume` escaped {site}")),
            // Abortive: they end or fail the program, so they may cross.
            // `HandlerDone` is the one cross-frame completion, and it is
            // addressed: only the `with` that installed its handler catches it.
            Flow::Panic(_)
            | Flow::VerifyFailed(_)
            | Flow::Halted(_)
            | Flow::AiPolicyUnreachable(_)
            | Flow::Exit(_)
            | Flow::HandlerDone(..)
            | Flow::MultiShotUnsound(_)
            | Flow::RefineViolation(_)
            | Flow::GoalBudgetExhausted(_)
            | Flow::SandboxViolation(_) => Err(f),
        },
    }
}

/// Runtime provenance for Protected Check Isolation. The static check
/// (`resolver::check_sealed`) is a SYNTAX walk and kept missing routes — a match
/// guard, a method call, a function named in a string (`scheduler_spawn`,
/// `@[goal(metric: …)]`), a refinement attaching by name (PCI candidate-3
/// review). Every one of them ends in a CALL, a GLOBAL READ, or a REFINEMENT
/// application, so the interpreter enforces sealing at exactly those three
/// edges, whatever syntax led there:
/// * a sealed frame may call only sealed functions (direct, method, or by
///   name through any builtin) — plus builtins and closures handed to it;
/// * a sealed frame may not read an unsealed global;
/// * a sealed refinement never runs in an unsealed (operator) frame.
///
/// Frames carry provenance: a function by its definition's file, a closure by
/// the frame that CREATED it (a marker in its capture cell), a handler arm by
/// the frame that installed the `with`.
#[derive(Default)]
struct Seal {
    active: bool,
    fns: std::collections::HashSet<usize>,
    globals: std::collections::HashSet<String>,
    refines: std::collections::HashSet<String>,
    /// Struct types defined in a sealed module: their whole-struct `where`
    /// runs under THEIR provenance, whoever constructs one.
    types: std::collections::HashSet<String>,
    /// Method names the OPERATOR's code defines (in its impls and traits).
    /// In operator code such a name means the operator's method: a call that
    /// would dispatch it to a sealed method (the receiver's runtime type is
    /// one the candidate chose) is refused (C9 round 4, PSV-1, amendment 53).
    operator_methods: std::collections::HashSet<String>,
}

/// Test-only: the dispatch rule is switched off while a test of another seal
/// layer runs (serialised by `SEALED_DIRS_TEST_LOCK`).
#[cfg(test)]
pub(crate) static DISPATCH_RULE_OFF: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
/// Test-only: the runtime taint rules (amendment 102) apply. Off by default in a
/// unit test, so each older test judges one STATIC layer by its own attack and
/// the taint cannot refuse the same attack first and hide a removed guard; the
/// taint's own tests (`interp/taint_tests.rs`) and the sweep turn it on.
#[cfg(test)]
pub(crate) static TAINT_FORCE_ON: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Capture-cell key marking a closure created in a SEALED frame. Starts with
/// NUL, so no source identifier can name or shadow it.
pub(crate) const SEALED_CLOSURE_MARK: &str = "\u{0}sealed";
/// Capture-cell key marking a fn VALUE (`let g = f`) a sealed frame took of a
/// candidate fn: a call to it from sealed code is not a call to an operator
/// closure, and an operator closure may not be replaced by it.
pub(crate) const SEALED_FNVAL_MARK: &str = "\u{0}sealedfn";
/// Capture-cell key holding the `FnDef` address of the fn that created a
/// closure (the pin analysis owns a lambda body by its creator).
pub(crate) const PIN_FN_MARK: &str = "\u{0}pinfn";

pub(crate) fn contain_loop_control(r: R, site: &str) -> R {
    match r {
        Err(Flow::Break) | Err(Flow::Continue) => {
            panic(format!("`break`/`continue` outside a loop in {site}"))
        }
        other => other,
    }
}

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

/// A stack of lexical scopes, stored flat: every visible binding in one
/// vector (outermost first), with `marks` holding the start index of each
/// pushed scope (the base scope starts at 0 and has no mark).
///
/// Flat rather than one `HashMap` per scope because a call frame holds a
/// handful of names: a reverse linear scan beats hashing the name once per
/// scope on every lookup, and pushing a scope allocates nothing. Semantics are
/// the per-scope-map ones exactly — `define` replaces a same-named binding in
/// the CURRENT scope (so a loop re-binding a name does not grow the stack) and
/// shadows outer ones; lookups see the innermost binding first.
struct Env {
    /// Every visible binding: its name, its value and its TAINT (C9 round 11,
    /// amendment 102, `interp/taint.rs`; always zero outside a sealed run).
    vars: Vec<(String, Value, u8)>,
    marks: Vec<usize>,
}

impl Env {
    fn new() -> Self {
        Env {
            vars: Vec::new(),
            marks: Vec::new(),
        }
    }
    fn push(&mut self) {
        self.marks.push(self.vars.len());
    }
    fn pop(&mut self) {
        let start = self.marks.pop().unwrap_or(0);
        self.vars.truncate(start);
    }
    /// Bind `name` to `val` carrying taint `t`. The taint is a REQUIRED
    /// argument: a binding site that does not say what it carries does not
    /// compile, so no site can drop it by omission.
    fn define(&mut self, name: String, val: Value, t: u8) {
        let start = self.marks.last().copied().unwrap_or(0);
        match self.vars[start..].iter_mut().find(|(k, _, _)| *k == name) {
            Some(slot) => {
                slot.1 = val;
                slot.2 = t;
            }
            None => self.vars.push((name, val, t)),
        }
    }
    fn get(&self, name: &str) -> Option<&Value> {
        self.vars
            .iter()
            .rev()
            .find(|(k, _, _)| k == name)
            .map(|(_, v, _)| v)
    }
    /// The taint of the nearest binding of `name` (0 if unbound).
    fn taint_of(&self, name: &str) -> u8 {
        self.vars
            .iter()
            .rev()
            .find(|(k, _, _)| k == name)
            .map_or(0, |(_, _, t)| *t)
    }
    /// OR `t` into the nearest binding of `name` (a write into a container the
    /// binding holds makes the whole binding carry it).
    fn taint_or(&mut self, name: &str, t: u8) {
        if let Some(slot) = self.vars.iter_mut().rev().find(|(k, _, _)| k == name) {
            slot.2 |= t;
        }
    }
    /// Update the nearest existing binding; returns false if none exists. The
    /// binding takes the taint `t` of the value now in it.
    fn assign(&mut self, name: &str, val: Value, t: u8) -> bool {
        match self.vars.iter_mut().rev().find(|(k, _, _)| k == name) {
            Some(slot) => {
                slot.1 = val;
                slot.2 = t;
                true
            }
            None => false,
        }
    }
    /// Mutable reference to the nearest existing binding (for place assignment).
    fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.vars
            .iter_mut()
            .rev()
            .find(|(k, _, _)| k == name)
            .map(|(_, v, _)| v)
    }
    /// Flatten all visible bindings into one map (inner shadows outer).
    /// Used to snapshot the environment a closure captures. A binding that
    /// carries taint is recorded under a companion key (`taint::CAP_T`).
    fn snapshot(&self) -> HashMap<String, Value> {
        let mut out = HashMap::with_capacity(self.vars.len());
        // Outside a sealed run no binding is tainted: the plain copy.
        let tainted = self.vars.iter().any(|(_, _, t)| *t != 0);
        for (k, v, t) in &self.vars {
            out.insert(k.clone(), v.clone());
            if tainted {
                let ck = format!("{}{k}", taint::CAP_T);
                if *t != 0 {
                    out.insert(ck, Value::Int(i64::from(*t)));
                } else {
                    out.remove(&ck);
                }
            }
        }
        out
    }
    /// Load captured bindings (a capture cell's entries, companions included)
    /// into the base scope of an empty env.
    fn load_captured(&mut self, cells: impl Iterator<Item = (String, Value)>) {
        // Companion entries are rare (a sealed run only): collect them lazily.
        let mut comp: Vec<(String, u8)> = Vec::new();
        for (k, v) in cells {
            if let Some(name) = k.strip_prefix(taint::CAP_T) {
                if let Value::Int(t) = v {
                    comp.push((name.to_string(), t as u8));
                }
            } else {
                self.vars.push((k, v, 0));
            }
        }
        for (name, t) in comp {
            if let Some(slot) = self.vars.iter_mut().find(|(k, _, _)| *k == name) {
                slot.2 = t;
            }
        }
    }
    /// Build an env whose single base scope is a captured snapshot — used to
    /// run a closure/handler-arm body in its defining environment.
    fn from_snapshot(captured: HashMap<String, Value>) -> Self {
        let mut env = Env::new();
        env.load_captured(captured.into_iter());
        env
    }
    /// The bindings of the base (outermost) scope, with their taints.
    fn base_scope(&self) -> &[(String, Value, u8)] {
        let end = self.marks.first().copied().unwrap_or(self.vars.len());
        &self.vars[..end]
    }
    /// Move the base scope's bindings into a capture cell (taint companions
    /// included; see [`Env::base_scope`]).
    fn drain_base_scope_into(&mut self, cell: &mut HashMap<String, Value>) {
        let end = self.marks.first().copied().unwrap_or(self.vars.len());
        for (k, v, t) in self.vars.drain(..end) {
            if t != 0 {
                cell.insert(format!("{}{k}", taint::CAP_T), Value::Int(i64::from(t)));
            }
            cell.insert(k, v);
        }
    }
}

// ── Sandbox state ────────────────────────────────────────────────────────────

/// F5 (Phase 9): one live sandbox entry — a named principal + the concrete set
/// of effects it allows at runtime. Created by `sandbox_create`; enforced by
/// `call_builtin` whenever `active_sandbox` points at this entry.
#[derive(Clone)]
struct SandboxEntry {
    /// The principal handle this sandbox is scoped to (for audit attribution).
    principal: i64,
    /// The concrete effects this sandbox permits (e.g. {"AI", "Net"}).
    allowed: std::collections::HashSet<String>,
    /// AUDIT T3: the SCOPE of those effects. `None` means "unscoped" — the
    /// effect is granted without a path/host restriction, which is the
    /// pre-existing `sandbox_create` behaviour and stays the default so old
    /// callers are unaffected. `Some(list)` restricts the effect to arguments
    /// matching one of the entries.
    ///
    /// Without this, `@[contained(fs: [write("./out/")], net: ["api.x.com"])]`
    /// enforced only "may write SOMEWHERE" / "may reach SOME host": the
    /// allowlists parsed, type-checked and were rendered to the approving human,
    /// then discarded before anything could enforce them.
    scope: SandboxScope,
}

/// Path/host restrictions attached to a sandbox. `None` = unscoped (grant the
/// effect with no argument restriction); `Some(v)` = the call argument must
/// match an entry, or it is a SandboxViolation.
#[derive(Default, Clone)]
struct SandboxScope {
    fs_read: Option<Vec<String>>,
    fs_write: Option<Vec<String>>,
    net: Option<Vec<String>>,
}

// ── Interpreter ──────────────────────────────────────────────────────────────

/// Per-provenance kernel state (PCI runtime sealing). Every table that holds
/// objects addressed by HANDLE — fibers, supervisors, principals, stores, LLM
/// gateways, kernel goals, sandboxes — plus the attribution and constraint
/// state they drive. A sealed (candidate) frame gets its OWN instance, so an
/// operator handle does not exist from a sealed frame, whatever id is guessed,
/// and a candidate's fibers never run in the operator's scheduler (PCI
/// candidate-4 review: forged fiber ids read the operator's reference result,
/// reset its failed fibers, and a predicted principal token spent its budget).
struct Kernel {
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
    /// R9 corrigibility latch. `corrigible_halt()` sets this to `true`; once
    /// set it never clears (there is intentionally no resume builtin). While
    /// set, every call to an `@[corrigible]` fn is refused — its body never
    /// runs — so the system cannot resist or reverse its own shutdown. A
    /// one-way latch is the whole safety property: a kill-switch you can turn
    /// back off is not a kill-switch.
    corrigible_halted: Cell<bool>,
    /// This kernel's xorshift64 RNG state (`0` = not yet seeded). EVERY draw —
    /// `random_*`, the distribution samplers, `srand`, the `goal_*` searches
    /// and `@[forall]` input generation — goes through [`Interp::rng_next`] /
    /// [`Interp::rng_reseed`], i.e. through the kernel of the frame that is
    /// running. There is no process-global stream, so a sealed candidate can
    /// neither advance, reseed nor observe the operator's stream, whichever
    /// builtin it uses (PSV-1). The C8 certifying review wf_ae3a5a74-41e
    /// steered the old shared stream through `goal_run_random`, whose effect
    /// row does not say `Random`, after two narrower fixes, one keyed on
    /// builtin names and one on the effect row, had each missed a route.
    rng: Cell<u64>,
    /// Whether this is the sealed (candidate) kernel. Its stream is seeded by a
    /// one-way derivation, so drawing from it reveals nothing about the
    /// operator's seed — see [`sealed_rng_seed`].
    rng_sealed: bool,
}

impl Kernel {
    /// The next draw from THIS kernel's stream, seeding it on first use.
    fn rng_next(&self) -> u64 {
        let mut x = self.rng.get();
        if x == 0 {
            let s = rng_seed();
            x = if self.rng_sealed {
                sealed_rng_seed(s)
            } else {
                s
            };
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng.set(x);
        x
    }

    /// `srand(n)`: reseed THIS kernel's stream only. `n == 0` maps to a
    /// non-zero sentinel so it does not read as "not yet seeded".
    fn rng_set(&self, n: i64) {
        self.rng.set((n as u64) | 1);
    }
}

/// The sealed kernel's initial RNG state: SHA-256 over a domain tag and the
/// operator seed. Deterministic under `AXON_SEED` (replay still reproduces a
/// candidate's draws), but one-way, so a candidate that reads its own stream
/// (xorshift64's output IS its state) learns nothing about the operator's.
fn sealed_rng_seed(operator_seed: u64) -> u64 {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"axon-sealed-rng/1\0");
    h.update(operator_seed.to_le_bytes());
    let d = h.finalize();
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_le_bytes(b) | 1
}

pub struct Interp<'p> {
    /// The KERNEL tables, one per provenance: `kernels[0]` for the operator,
    /// `kernels[1]` for sealed (candidate) frames — selected by `k()`.
    kernels: [Kernel; 2],
    /// Protected Check Isolation, RUNTIME sealing: which definitions come from
    /// a sealed (candidate) module, and whether the frame now running is one.
    seal: Seal,
    /// The method-call sites whose receiver's type nothing on the operator
    /// side determined (built only for a sealed run; amendment 83).
    pins: pin::Pins,
    /// The fn whose body is running, for the pin analysis (its `FnDef`
    /// address; a closure carries its creator's). 0 outside any fn.
    pin_fn: Cell<usize>,
    frame_sealed: Cell<bool>,
    /// How many frames of each provenance are active (entered through
    /// [`Interp::with_frame`] and not yet left). `with_frame` is the ONLY
    /// place provenance changes, so these are exact. Two decisions read them:
    /// * a sealed handler frame answers an operation only when no OPERATOR
    ///   frame was entered after it was installed ([`Interp::handler_may_answer`]);
    /// * operator code never draws from the operator RNG while a sealed frame
    ///   is below it ([`Interp::rng_next`]).
    sealed_frames: Cell<usize>,
    operator_frames: Cell<usize>,
    /// The call depth of the `@[test]` frame being run (`0` = none), and
    /// whether THAT frame's body evaluated to its end. The one fact test
    /// completion is decided on (C9 round 3, PSV-3): not the returned value's
    /// tag, which a `?` on a type-confused `None` made read as a completion.
    test_frame_depth: Cell<usize>,
    test_body_finished: Cell<bool>,
    fns: HashMap<String, &'p FnDef>,
    #[allow(dead_code)]
    structs: HashMap<String, &'p TypeDef>,
    #[allow(dead_code)]
    enums: HashMap<String, &'p EnumDef>,
    /// `(type_name, method_name) → method def` from impl blocks.
    methods: HashMap<(String, String), &'p FnDef>,
    /// The impl block each method was defined in (by `FnDef` address), for
    /// reading its signature: `Self` and the impl's type parameters.
    impl_of: HashMap<usize, &'p ImplBlock>,
    /// The traits the program defines, and `(type_name, trait)` for each
    /// `impl Trait for Type`: `dyn Trait` and trait bounds are cast by them.
    user_traits: std::collections::HashSet<String>,
    trait_impls: std::collections::HashSet<(String, String)>,
    /// A named refinement's base type (`type Pos = i64 where …` → `i64`).
    refine_bases: HashMap<String, &'p crate::ast::AxonType>,
    /// The element types each channel OBJECT crossed (by address; the weak
    /// handle keeps the address from being reused while the entry exists).
    chan_contracts: RefCell<ChanContracts>,
    /// The dicts the operator handed sealed code (by address): what each held
    /// (amendment 72 part 2, `interp/conform.rs`).
    dict_snaps: RefCell<HashMap<usize, conform::DictSnap>>,
    /// Bumped by every operator-side dict mutation: a snapshot from an older
    /// epoch is retaken at the next hand-over.
    dict_epoch: std::cell::Cell<u64>,
    /// [`Interp::fn_cx`]'s per-fn signature environments.
    fn_cx_cache: RefCell<HashMap<usize, conform::Cx>>,
    /// Module-level `let NAME = …` constant definitions, in source order.
    global_defs: Vec<(String, &'p Expr)>,
    /// Evaluated module-level constants (populated by [`Interp::init_globals`]).
    globals: HashMap<String, Value>,
    /// Current call-stack depth, bounded by `max_depth` so runaway recursion
    /// fails with a catchable panic rather than overflowing the (large but
    /// finite) interpreter thread stack and aborting the process.
    call_depth: Cell<usize>,
    /// Effective recursion ceiling for this run — `RECURSION_LIMIT` by default,
    /// or `AXON_MAX_DEPTH` (clamped) when set. Resolved once at build time so
    /// every `call_fn` sees a consistent value.
    max_depth: usize,
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
    /// Per-call AI tier from a `tier:` named arg (R3b), set by `eval_call` for
    /// the duration of a single builtin dispatch. `ai_complete`'s tier
    /// resolution reads this first (step 1: per-call > policy > default).
    current_call_tier: RefCell<Option<String>>,
    /// Callee names already proven NOT to be builtins, mapped to the user fn
    /// they name (if any). Filled by `eval_call` when `call_builtin` answered
    /// `Ok(None)` for a name whose pre-dispatch steps are provably inert
    /// (`builtin_dispatch_is_inert`), so later calls skip the ~600-arm builtin
    /// dispatch and the `fns` lookup — the dominant per-call costs of a
    /// recursive user fn.
    resolved_callees: RefCell<HashMap<String, Option<&'p FnDef>>>,
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
    /// Fns already warned about by W1310, so the warning is emitted once per fn
    /// rather than once per CALL. The comment at the emit site always claimed
    /// "warn once"; it did not, so a `goal_run` search over an un-policied
    /// `@[adaptive]` fn printed one identical line per evaluation -- 8 lines for
    /// 8 evals, hundreds for a real search -- burying the diagnostics that were
    /// not repetitions. The text names only the fn, so every repeat after the
    /// first carries no information a reader did not already have.
    w1310_warned: RefCell<std::collections::HashSet<String>>,
    /// Tokens consumed so far by `ai_complete`, against [`Interp::token_budget`].
    ///
    /// Distinct from both sibling meters: `ai_cost_micro` is MONEY (per-token
    /// µ$ at the tier's rate) and R3c's `ai_calls_this_fn` counts CALLS within
    /// one fn. This is TOKENS across the whole run, because that is the unit an
    /// ambient operator cap is denominated in.
    tokens_used: Cell<i64>,
    /// Ambient run-level token cap from `AXON_BUDGET_TOKENS`, or `None` for no
    /// cap (the default). `Some(0)` is meaningful — it means "no AI at all" —
    /// so this is deliberately not a sentinel `0`.
    ///
    /// The var was set by `axon-guest-init` from the VM's MMDS policy and read
    /// by nothing, so a VM operator who capped a run's tokens got no cap: an
    /// inert control surface that reads as a safety mechanism. Enforced
    /// pre-dispatch beside the R3c per-fn budget gate.
    token_budget: Option<i64>,
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
    /// Phase 5: named refinement → its predicate Expr (binder `_`). Collected
    /// from `RefineDef` items (inline `where` on a param desugars to a synthetic
    /// named refinement during parsing). Drives the runtime precondition check in
    /// `call_fn`: when a parameter's type is one of these, the predicate is
    /// evaluated with `_` bound to the argument and a violation raises
    /// [`Flow::RefineViolation`]. Empty when the program has no refinements.
    refine_preds: HashMap<String, &'p Expr>,
    /// PROTOTYPE (RLM session option 2): `main`'s final top-level locals,
    /// captured after its body evaluates so AXON_DUMP_BINDINGS can persist a
    /// cell's mutated locals. Only populated when the env var is set.
    main_locals: RefCell<HashMap<String, Value>>,
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
    // Under `gfx-wgpu` the real backend is used instead, so the mock field is
    // unread there — keep it (cheap, always-constructed) and silence dead_code.
    #[cfg_attr(feature = "gfx-wgpu", allow(dead_code))]
    gfx_mock: RefCell<crate::native::GfxMock>,
    /// R22 domain-interop native modules (`native::modbus`/`fhir`/`fix`) —
    /// per-`Interp` backend state (one slab table per module). Interp-only; the
    /// network ones are codegen-refused (the `host_await`/native precedent).
    /// Gated to non-wasm targets (its tokio/reqwest deps don't build for the
    /// in-browser wasm32-unknown-unknown interpreter, R7c); on wasm a domain
    /// call is a clean "unavailable on this target" refusal.
    #[cfg(not(target_arch = "wasm32"))]
    domain: axon_domain::Registry,
    /// R13 slice 5: the REAL wgpu offscreen-render backend for `native::gfx`,
    /// present only under the `gfx-wgpu` feature. When set, `eval_native_call`
    /// routes `gfx::*` here (a real headless GPU render) instead of the mock —
    /// the same `dispatch` interface, a drop-in backend (one interface, two
    /// backends). The value-returning `read_pixel`/`frame_count` are byte-
    /// identical to the mock (same packing), so I-2 parity holds.
    #[cfg(feature = "gfx-wgpu")]
    gfx_real: RefCell<axon_gfx::GfxReal>,
    /// The runtime taint (C9 round 11, amendment 102, `interp/taint.rs`). Last,
    /// so the hot fields above keep the offsets they had.
    taint: taint::Taint,
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
    /// Provenance of the frame that installed this handler: its arms run
    /// under it (PCI runtime sealing).
    sealed: bool,
    /// `Interp::operator_frames` when this handler was installed. A SEALED
    /// frame may answer an operation only while the count is unchanged, i.e.
    /// no operator code lies between the `with` and the operation (PSV-1,
    /// C9 round 3). See [`Interp::handler_may_answer`].
    operator_frames: usize,
    /// `Interp::pin_fn` when this handler was installed: the fn that OWNS the
    /// `with` and so the arms' source text. The arm runs under THIS owner, not
    /// under whichever fn happened to perform the effect (C9 round 10, PSV-1,
    /// amendment 100): pin sites are keyed (owner, site text), so an arm run
    /// under the performer's owner read the performer's verdicts.
    pin_owner: usize,
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
    /// The taint of `feed` (the `resume(..)` argument, with the control it was
    /// evaluated under): read when the replay yields it, so a value derived from
    /// a candidate-chosen resume value stays tainted inside the replayed body
    /// (amendment 117, loop finding 28).
    feed_t: u8,
    /// Provenance of the handler whose arm armed this replay, and
    /// `Interp::operator_frames` when it did. The feed answers an operation
    /// only under the same rule as a live handler frame
    /// ([`Interp::handler_may_answer`]): a sealed arm's `resume(v)` never
    /// becomes the result of an operation the operator's code performs.
    sealed: bool,
    operator_frames: usize,
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
    /// Provenance of the handler frame whose arm is servicing this body.
    sealed: bool,
    /// The pin owner of the fn that installed the handler: the replayed body is
    /// that fn's own text, so it replays under that owner (amendment 100).
    pin_owner: usize,
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
/// overflow. Generous (2×) over the observed debug frame: ~265 KB per
/// interpreted call in the v0.22 build (measured 2026-10-06 as the deepest
/// `c(n) = 1 + c(n-1)` that fits a 1 GiB stack, ~4070), which grew from the
/// ~128 KB this budget was first sized against when the PCI seal edges
/// (`call_fn_sealed`) and the shared-string/lent-capture work (AX-31) landed.
const STACK_BYTES_PER_FRAME: usize = 512 * 1024;

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

/// Restores the caller's `pin_fn` on drop.
struct PinGuard<'a> {
    cell: &'a Cell<usize>,
    prev: usize,
}
impl Drop for PinGuard<'_> {
    fn drop(&mut self) {
        self.cell.set(self.prev);
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
        // Unmapped: the oracle compares what two programs PRODUCE. Mapping first
        // would collapse distinct results into 1 and call a rewrite equivalent
        // when it is not.
        let code = run_program_inner(program, crate::verify::Discharged::default(), false);
        let captured = OUTPUT_SINK.with(|s| s.replace(prev)).unwrap_or_default();
        (code, captured)
    })
}

/// Parse-and-run convenience: returns the **process exit status** — what the
/// caller of `axon run` observes, with `stated_exit_status` /
/// `returned_exit_status` applied.
pub fn run_program(program: &Program) -> i32 {
    on_deep_stack(|| run_program_inner(program, crate::verify::Discharged::default(), true))
}

/// Like [`run_program`], but returns the value the program PRODUCED rather than
/// the status a caller would observe.
///
/// For reading a result out of a program — a test asserting on a computed value,
/// or anything comparing two runs. Guard outcomes (verify, refinement, sandbox …)
/// still come back as their ledger codes; only `main`'s own return and `exit(n)`
/// are left alone.
pub fn run_program_unmapped(program: &Program) -> i32 {
    on_deep_stack(|| run_program_inner(program, crate::verify::Discharged::default(), false))
}

/// Phase 5 §4: run with a set of SMT-discharged obligations installed, so the
/// interpreter elides the runtime checks Z3 proved ∀-inputs. Identical to
/// [`run_program`] with an empty set.
/// Describe a value's SHAPE — its structure, never its contents.
///
/// `AXON_FOR_RLM.md` §5 / atlas `RLM_MODE_SPEC.md` §11 arm A. A session tells
/// the model which names are bound; the measured failure is that names alone
/// leave it guessing what they *hold*, so it indexes a `[Row]` as if it were a
/// str and the reuse produces a wrong answer confidently.
///
/// The describer answers that without executing anything and without leaking a
/// value. Every branch here is written to that rule:
///
///   * scalars give a type name and never a number,
///   * `str` gives a LENGTH and never its characters,
///   * containers give an element description and a length,
///   * a struct gives its field NAMES — schema, which is the point — and its
///     field types, never their values.
///
/// A field name is the one branch that could leak, if a namespace were keyed by
/// data rather than by schema. That is a real risk and the reason the caller is
/// expected to scan the emitted text for dataset values rather than trust this
/// doc comment.
///
/// Depth-limited to two levels: deeper nesting would make the description grow
/// with the DATA, and the whole claim of a shape inventory is that it is a
/// per-turn constant that tracks schema instead.
pub fn value_shape(v: &Value) -> String {
    fn go(v: &Value, d: usize) -> String {
        match v {
            // A length only at the TOP level. Nested, it would be ambiguous
            // (`Row { region: str, len=5 }` reads as a field named `len`) and it
            // would track the DATA — every element of a `[str]` has its own
            // length, so the description would grow with the array instead of
            // staying a schema.
            Value::Str(s) if d == 0 => format!("str, len={}", s.chars().count()),
            Value::Str(_) => "str".to_string(),
            Value::Array(items) => {
                if let (Some(first), true) = (items.first(), d < 2) {
                    format!("[{}], len={}", go(first, d + 1), items.len())
                } else {
                    format!("[], len={}", items.len())
                }
            }
            Value::Tuple(items) => {
                if d < 2 {
                    let inner: Vec<String> = items.iter().map(|i| go(i, d + 1)).collect();
                    format!("({})", inner.join(", "))
                } else {
                    format!("tuple, len={}", items.len())
                }
            }
            Value::Struct { name, fields } => {
                if d < 2 {
                    let mut ks: Vec<&String> = fields.keys().collect();
                    ks.sort();
                    let inner: Vec<String> = ks
                        .iter()
                        .map(|k| format!("{k}: {}", go(&fields[*k], d + 1)))
                        .collect();
                    format!("{name} {{ {} }}", inner.join(", "))
                } else {
                    name.clone()
                }
            }
            Value::Dict(dd) => {
                let b = dd.borrow();
                let n = b.len();
                // Keys are schema ONLY when the dict is a record. A dict built by
                // GROUPING is keyed by the data itself, so printing the key set
                // leaks dataset values into what is supposed to be a value-free
                // shape — `arr_group_by(rows, region)` would emit the regions.
                // The two cases are indistinguishable here, so neither prints its
                // keys: the key TYPE and the count are the schema, and the value
                // shape (which tells the model whether `dict_get` hands back an
                // i64 or a [Row]) is kept because it carries no data.
                if let (Some(k), true) = (b.keys().next(), d < 2) {
                    format!(
                        "dict, len={n}, keys are str, values are {}",
                        go(&b[k], d + 1)
                    )
                } else {
                    format!("dict, len={n}")
                }
            }
            Value::Some(inner) if d < 2 => format!("Option<{}>", go(inner, d + 1)),
            Value::Ok(inner) if d < 2 => format!("Result, Ok<{}>", go(inner, d + 1)),
            Value::Err(inner) if d < 2 => format!("Result, Err<{}>", go(inner, d + 1)),
            Value::None => "Option, None".to_string(),
            // Scalars and everything else: the type name alone, which for an
            // Int/Float/Bool is exactly the no-value rule.
            other => other.type_name(),
        }
    }
    go(v, 0)
}

/// Render a value as an Axon **literal**, or explain why it cannot be.
///
/// `AXON_FOR_RLM.md` §5, the values-persisting session. A session restores a
/// prior cell's bindings by emitting `let name = <literal>` — a literal, never
/// the original expression, because re-evaluating `let rows = expensive()`
/// would re-run `expensive()` on every subsequent cell. That side-effect replay
/// is the failure the declarations-only spike avoided by having no values at
/// all, and it is the one this must not reintroduce.
///
/// `Err(reason)` is not a failure path: an open channel or a closure is a
/// perfectly normal binding that simply cannot be written down. The caller
/// reports it as `skipped`, which is what `Engine::Snapshot` models and what
/// CPython's `dill` does.
pub fn value_as_literal(v: &Value) -> std::result::Result<String, String> {
    match v {
        Value::Int(n) => Ok(n.to_string()),
        Value::SizedInt { val, .. } => Ok(val.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Float(f) => {
            // `1` would re-parse as an i64 and silently change the binding's type.
            if f.fract() == 0.0 && f.is_finite() {
                Ok(format!("{f:.1}"))
            } else if f.is_finite() {
                Ok(format!("{f:?}"))
            } else {
                Err("non-finite float has no literal form".to_string())
            }
        }
        Value::Str(st) => {
            // Braces MUST be doubled. Axon string literals interpolate, so a
            // value containing `{` dumped verbatim produces a literal the lexer
            // rejects (`unclosed \u{7b} in interpolated string`) — and since the
            // dump becomes the next cell's prelude, that bricks the session
            // permanently: even `let n = 1` is then refused. A model building a
            // JSON-ish string hits this immediately. `{{` → `{` is the parser's
            // own convention (`parser.rs:96-101`); `}}` is doubled with it so
            // the escaping is the parser's exact inverse rather than nearly so.
            let esc = st
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t")
                .replace('{', "{{")
                .replace('}', "}}");
            Ok(format!("\"{esc}\""))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items.iter() {
                out.push(value_as_literal(it)?);
            }
            Ok(format!("[{}]", out.join(", ")))
        }
        // A record. Needed because the very first binding in a realistic session
        // is a list of records — `rows` in `stateful.rs`'s chain fixture — and
        // skipping structs means the session cannot carry its own headline case.
        // Field order is sorted so the emitted literal is deterministic; a
        // HashMap's iteration order would make the session file differ run to run.
        Value::Struct { name, fields } => {
            let mut keys: Vec<&String> = fields.keys().collect();
            keys.sort();
            let mut parts = Vec::with_capacity(keys.len());
            for k in keys {
                parts.push(format!("{k}: {}", value_as_literal(&fields[k])?));
            }
            Ok(format!("{name} {{ {} }}", parts.join(", ")))
        }
        // Option / Result / Tuple / Enum all HAVE literal syntax, and a realistic
        // session binds them constantly — `let o = parse_int(s)` is a `Result`.
        // Leaving them in the catch-all meant the most ordinary binding a model
        // writes could not cross a cell boundary.
        Value::Some(inner) => Ok(format!("Some({})", value_as_literal(inner)?)),
        Value::None => Ok("None".to_string()),
        Value::Ok(inner) => Ok(format!("Ok({})", value_as_literal(inner)?)),
        Value::Err(inner) => Ok(format!("Err({})", value_as_literal(inner)?)),
        Value::Tuple(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items {
                out.push(value_as_literal(it)?);
            }
            // A 1-tuple needs the trailing comma or it re-parses as a
            // parenthesised expression and silently changes type.
            if out.len() == 1 {
                Ok(format!("({},)", out[0]))
            } else {
                Ok(format!("({})", out.join(", ")))
            }
        }
        Value::Enum {
            enum_name,
            variant,
            fields,
        } => {
            if fields.is_empty() {
                Ok(format!("{enum_name}::{variant}"))
            } else {
                let mut keys: Vec<&String> = fields.keys().collect();
                keys.sort(); // deterministic, as for structs
                let mut parts = Vec::with_capacity(keys.len());
                for k in keys {
                    parts.push(format!("{k}: {}", value_as_literal(&fields[k])?));
                }
                Ok(format!("{enum_name}::{variant} {{ {} }}", parts.join(", ")))
            }
        }
        // A dict. Not a *literal* — Axon has no dict literal syntax — but
        // `dict_from_pairs` is a pure total call over data already written down,
        // so it re-creates the value without re-running the computation that
        // produced it, which is the property the session actually needs.
        //
        // Two things are refused rather than fudged:
        //   * a dict whose values are not all the same shape, because
        //     `dict_from_pairs` takes `[(str, V)]` and a mixed array does not
        //     type-check — emitting it would produce a prelude that bricks the
        //     next cell;
        //   * aliasing, which is handled by the caller (see `dump_bindings`),
        //     since a single value cannot see that another binding shares it.
        Value::Dict(d) => {
            let map = d.borrow();
            if map.is_empty() {
                // `dict_from_pairs([])` has no element type to infer from.
                return Ok("dict_new()".to_string());
            }
            let tag = |v: &Value| -> &'static str {
                match v {
                    Value::Int(_) | Value::SizedInt { .. } => "int",
                    Value::Float(_) => "float",
                    Value::Bool(_) => "bool",
                    Value::Str(_) => "str",
                    Value::Array(_) => "array",
                    Value::Dict(_) => "dict",
                    Value::Struct { .. } => "struct",
                    Value::Tuple(_) => "tuple",
                    _ => "other",
                }
            };
            let first = tag(map.values().next().expect("non-empty"));
            if map.values().any(|v| tag(v) != first) {
                return Err(
                    "dict with mixed value types has no writable form (dict_from_pairs needs one \
                     element type)"
                        .to_string(),
                );
            }
            let mut parts = Vec::with_capacity(map.len());
            for (k, v) in map.iter() {
                // The key goes through the same escaping as any str.
                let kl = value_as_literal(&Value::Str(Rc::new(k.clone())))?;
                parts.push(format!("({kl}, {})", value_as_literal(v)?));
            }
            Ok(format!("dict_from_pairs([{}])", parts.join(", ")))
        }
        Value::Unit => Err("unit has no binding form".to_string()),
        // Closure/Chan/Handle/… — a session can carry a
        // value only if it can write it down, and these cannot be written as a
        // literal today. Reported by name so the caller can say WHICH binding
        // was dropped, which is the whole contract of a skip list.
        // The SHAPE, not the Rust `Debug`. This message is read by a person (or
        // a model) deciding what to do about a binding that did not survive a
        // cell; `Closure { params: ["x"], body: BinOp { op: Add, left: … } }`
        // tells them nothing actionable and buries the one word that matters.
        other => Err(format!("a {} has no literal form", value_shape(other))),
    }
}

pub fn run_program_with_discharged(
    program: &Program,
    discharged: crate::verify::Discharged,
) -> i32 {
    on_deep_stack(|| run_program_inner(program, discharged, true))
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
    run_named_fn_as_bool_with_score(program, fn_name, None)
}

/// As [`run_named_fn_as_bool`], but supplies `score` to a gate declared with one
/// parameter (AUDIT T17 / P5-01).
pub fn run_named_fn_as_bool_with_score(
    program: &Program,
    fn_name: &str,
    score: Option<i64>,
) -> Option<i32> {
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
        // AUDIT T17 (finding P5-01): this called every gate with `vec![]` and no
        // arity check, so a gate declared `fn assert_deployable(score: i64)`
        // panicked with "expected 1 args, got 0". `axon intent compile` GENERATES
        // exactly that signature, so the tool's own generator emitted a gate ABI
        // the tool's own runner could not call — and CLAUDE.md's Acid Test 2
        // failed 100% of the time on the shipped example.
        //
        // Both shapes are now accepted: a nullary gate is called as before, and a
        // 1-arg gate receives the score it is written to judge (0 when no score
        // is available, which is the conservative input for a `score >= N` gate).
        // Any other arity is a clear error rather than a panic from deep inside
        // the interpreter.
        let args: Vec<Value> = match f.params.len() {
            0 => vec![],
            1 => vec![Value::Int(score.unwrap_or(0))],
            n => {
                eprintln!(
                    "axon: gate `{fn_name}` takes {n} arguments; a deploy gate must take \
                     0 (no input) or 1 (the score to judge)"
                );
                return Some(2);
            }
        };
        let result = interp.call_fn(f, args);
        Some(match result {
            Ok(Value::Bool(true)) => 0,
            Ok(Value::Bool(false)) => 1,
            Ok(Value::Int(0)) => 0,
            Ok(Value::Int(_)) => 1,
            // A GATE THAT PRODUCED NO READABLE VERDICT IS NOT A GATE THAT
            // PASSED.
            //
            // This arm used to be `=> 0`, making Unit, Str, Struct, Option and
            // Result indistinguishable from `Bool(true)`. Measured on a
            // Critical-risk program whose simulate, stress and
            // assert_deployable all returned `Err("... FAILED")`: the deploy
            // reported `status:"deployed"`, exit 0, with all four gates listed
            // in `stages_run` — which makes it look MORE audited than the
            // skipped-gate case that `gates_skipped` exists to expose. The
            // gate was not absent; it was unreadable, and unreadable meant
            // pass.
            //
            // Two shapes reach this and neither is exotic: a gate written with
            // no return type at all, and `-> Result<bool, str>` returning
            // `Err`, which is what "no null, no exceptions — Result<T,E>
            // everywhere" pushes an author toward.
            //
            // 2, matching the arity error above: the gate's ABI is unusable,
            // which is a different fact from the gate having judged and
            // refused (1). Any non-zero code blocks the deploy.
            Ok(other) => {
                let _ = std::io::stdout().flush();
                eprintln!(
                    "axon: gate `{fn_name}` returned {} — a gate must return \
                     bool (true = pass) or i64 (0 = pass). No verdict was \
                     produced, so this is NOT a pass.",
                    other.type_name()
                );
                2
            }
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
    SizedInt {
        val: i64,
        ty: crate::types::Type,
    },
    Float(f64),
    Decimal(i128),
    Bool(bool),
    Str(String),
    Unit,
    Array(Vec<SendValue>),
    Struct {
        name: String,
        fields: Vec<(String, SendValue)>,
    },
    Enum {
        enum_name: String,
        variant: String,
        fields: Vec<(String, SendValue)>,
    },
    Some(Box<SendValue>),
    None,
    Ok(Box<SendValue>),
    Err(Box<SendValue>),
    Closure {
        params: Vec<String>,
        body: Box<Expr>,
        captured: Vec<(String, SendValue)>,
    },
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
                .map(|k| {
                    Ok((
                        k.clone(),
                        SendValue::from_value_at(&m[k], format!("{path}.{k}"))?,
                    ))
                })
                .collect()
        }
        Ok(match v {
            Value::Int(n) => SendValue::Int(*n),
            Value::SizedInt { val, ty } => SendValue::SizedInt {
                val: *val,
                ty: ty.clone(),
            },
            Value::Float(f) => SendValue::Float(*f),
            Value::Decimal(m) => SendValue::Decimal(*m),
            Value::Bool(b) => SendValue::Bool(*b),
            Value::Str(s) => SendValue::Str(String::clone(s)),
            Value::Unit => SendValue::Unit,
            Value::Array(xs) => SendValue::Array(arr(xs, &path)?),
            Value::Struct { name, fields: f } => SendValue::Struct {
                name: name.clone(),
                fields: fields(f, &path)?,
            },
            Value::Enum {
                enum_name,
                variant,
                fields: f,
            } => SendValue::Enum {
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
            Value::Closure {
                params,
                body,
                captured,
                ..
            } => {
                let snapshot = captured.borrow();
                let mut keys: Vec<&String> = snapshot.keys().collect();
                keys.sort();
                let cap: Result<Vec<_>, _> = keys
                    .into_iter()
                    .map(|k| {
                        Ok((
                            k.clone(),
                            Self::from_value_at(&snapshot[k], format!("{path}.capture[{k}]"))?,
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
                    path: if path.is_empty() {
                        "<root>".to_string()
                    } else {
                        path
                    },
                });
            }
            // R13: a native handle is identity-bound to the in-process handle
            // table (like a Chan) — it cannot cross a suspend boundary. Refuse
            // rather than corrupt (the table index would be meaningless on the
            // other side).
            Value::Handle { .. } => {
                return Err(UnsendablePayload {
                    path: if path.is_empty() {
                        "<root>".to_string()
                    } else {
                        path
                    },
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
            SendValue::Decimal(m) => Value::Decimal(m),
            SendValue::Bool(b) => Value::Bool(b),
            SendValue::Str(s) => Value::Str(Rc::new(s)),
            SendValue::Unit => Value::Unit,
            SendValue::Array(xs) => {
                Value::Array(Rc::new(xs.into_iter().map(Self::into_value).collect()))
            }
            SendValue::Struct { name, fields } => Value::Struct {
                name,
                fields: fields
                    .into_iter()
                    .map(|(k, v)| (k, v.into_value()))
                    .collect(),
            },
            SendValue::Enum {
                enum_name,
                variant,
                fields,
            } => Value::Enum {
                enum_name,
                variant,
                fields: fields
                    .into_iter()
                    .map(|(k, v)| (k, v.into_value()))
                    .collect(),
            },
            SendValue::Some(b) => Value::Some(Box::new(b.into_value())),
            SendValue::None => Value::None,
            SendValue::Ok(b) => Value::Ok(Box::new(b.into_value())),
            SendValue::Err(b) => Value::Err(Box::new(b.into_value())),
            SendValue::Closure {
                params,
                body,
                captured,
            } => Value::Closure {
                params,
                body,
                // A closure that crossed the host boundary gets a FRESH capture
                // cell: the SendValue path is a deep clone by construction (a
                // shared cell is exactly what it cannot carry), so the two sides
                // are independent counters, not aliases (T40 + R15 Slice 2).
                captured: Rc::new(RefCell::new(
                    captured
                        .into_iter()
                        .map(|(k, v)| (k, v.into_value()))
                        .collect(),
                )),
                // The host built this value: it crossed no declared type here.
                contract: None,
            },
            SendValue::Tuple(xs) => Value::Tuple(xs.into_iter().map(Self::into_value).collect()),
            SendValue::Dict(entries) => {
                let map: std::collections::BTreeMap<String, Value> = entries
                    .into_iter()
                    .map(|(k, v)| (k, v.into_value()))
                    .collect();
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
        SendValue::Decimal(m) => crate::decimal::format_decimal(*m),
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
        SendValue::Enum {
            enum_name, variant, ..
        } => format!("{enum_name}::{variant}"),
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

// ── vsock host_await substrate (Linux, axon-vm microVM) ───────────────────────
//
// When AXON_VM_VSOCK_PORT is set the interpreter uses AF_VSOCK to communicate
// with the axon-vm launcher (CID 2 = host) instead of the worker-thread channel.
// The interpreter runs single-threaded; host_await_yield opens a connection per
// suspension (cheap — vsock connect is ~10µs in a VM).
//
// Protocol: length-prefixed JSON over a stream socket.
//   send: 4-byte LE u32 length, then `length` UTF-8 bytes (the request payload)
//   recv: 4-byte LE u32 length, then `length` bytes (the reply); length=0 → EOF

#[cfg(target_os = "linux")]
thread_local! {
    // File descriptor of the open vsock connection, or -1 when not in vsock mode.
    static VSOCK_PORT: std::cell::Cell<i32> = const { std::cell::Cell::new(-1) };
}

#[cfg(target_os = "linux")]
fn vsock_send_recv(port: u32, req: &str) -> Result<Option<String>, ()> {
    use std::io::{Read, Write};
    use std::os::unix::io::FromRawFd;

    // AF_VSOCK = 40, VMADDR_CID_HOST = 2
    const AF_VSOCK: libc::c_int = 40;
    const VMADDR_CID_HOST: u32 = 2;

    // sockaddr_vm layout: family(u16), reserved(u16), port(u32), cid(u32), flags(u8), pad(3)
    #[repr(C)]
    struct SockaddrVm {
        svm_family: u16,
        svm_reserved1: u16,
        svm_port: u32,
        svm_cid: u32,
        svm_flags: u8,
        svm_zero: [u8; 3],
    }

    let fd = unsafe { libc::socket(AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(());
    }

    let addr = SockaddrVm {
        svm_family: AF_VSOCK as u16,
        svm_reserved1: 0,
        svm_port: port,
        svm_cid: VMADDR_CID_HOST,
        svm_flags: 0,
        svm_zero: [0; 3],
    };

    let r = unsafe {
        libc::connect(
            fd,
            &addr as *const SockaddrVm as *const libc::sockaddr,
            std::mem::size_of::<SockaddrVm>() as libc::socklen_t,
        )
    };
    if r < 0 {
        unsafe {
            libc::close(fd);
        }
        return Err(());
    }

    // Safety: we just created fd and it's a valid stream socket.
    let mut stream = unsafe { std::net::TcpStream::from_raw_fd(fd) };

    // Send: 4-byte LE length + payload bytes.
    let payload = req.as_bytes();
    let len_bytes = (payload.len() as u32).to_le_bytes();
    stream.write_all(&len_bytes).map_err(|_| ())?;
    stream.write_all(payload).map_err(|_| ())?;
    stream.flush().map_err(|_| ())?;

    // Recv: 4-byte LE length + reply bytes.
    let mut lbuf = [0u8; 4];
    stream.read_exact(&mut lbuf).map_err(|_| ())?;
    let reply_len = u32::from_le_bytes(lbuf) as usize;
    if reply_len == 0 {
        return Ok(None); // EOF / end-of-input sentinel
    }
    let mut rbuf = vec![0u8; reply_len];
    stream.read_exact(&mut rbuf).map_err(|_| ())?;
    Ok(Some(String::from_utf8_lossy(&rbuf).into_owned()))
}

/// Run `program` using vsock for host_await (axon-vm microVM substrate).
/// The host (axon-vm launcher) listens on `vsock_port`; we connect per suspension.
#[cfg(target_os = "linux")]
pub fn run_suspendable_vsock(program: &Program, vsock_port: u32) -> i32 {
    VSOCK_PORT.with(|c| c.set(vsock_port as i32));
    let code =
        on_deep_stack(|| run_program_inner(program, crate::verify::Discharged::default(), true));
    VSOCK_PORT.with(|c| c.set(-1));
    code
}

// ── Unix-socket / hypercall host_await substrate (axon-guest-kernel bridge) ───
//
// When AXON_HOST_SOCKET is set the interpreter uses a Unix domain socket to
// communicate with the axon-guest-kernel's hypercall bridge instead of vsock or
// stdin/stdout. The guest kernel creates the socket before exec-ing the
// interpreter and issues the actual VMCALL to the host on our behalf.
//
// Protocol: same 4-byte-LE-length-prefixed UTF-8 as the vsock substrate.
//   send: 4-byte LE u32 length, then `length` UTF-8 bytes (the request payload)
//   recv: 4-byte LE u32 length, then `length` bytes (the reply); length=0 → EOF
//
// The socket is opened fresh per host_await call (cheap on Unix) so the
// interpreter remains single-threaded with no persistent fd lifecycle.

#[cfg(unix)]
thread_local! {
    // Active Unix socket path set by run_suspendable_hypercall; None when inactive.
    static UNIX_SOCK_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Open a connection to the Unix stream socket at `path`, perform one
/// length-prefixed request/reply round-trip, and close the socket.
/// Returns `Ok(Some(reply))` on success, `Ok(None)` when the peer sends
/// length=0 (end-of-input sentinel), or `Err(())` on any I/O failure.
/// Retries once with a 20 ms pause if the first `connect` fails — the
/// guest-kernel may be fractionally behind when exec-ing the interpreter.
#[cfg(unix)]
fn unix_socket_roundtrip(path: &str, req: &str) -> Result<Option<String>, ()> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(path)
        .or_else(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            UnixStream::connect(path)
        })
        .map_err(|_| ())?;

    // Send: 4-byte LE length + payload bytes.
    let payload = req.as_bytes();
    stream
        .write_all(&(payload.len() as u32).to_le_bytes())
        .map_err(|_| ())?;
    stream.write_all(payload).map_err(|_| ())?;
    stream.flush().map_err(|_| ())?;

    // Recv: 4-byte LE length + reply bytes.
    let mut lbuf = [0u8; 4];
    stream.read_exact(&mut lbuf).map_err(|_| ())?;
    let reply_len = u32::from_le_bytes(lbuf) as usize;
    if reply_len == 0 {
        return Ok(None); // EOF / end-of-input sentinel
    }
    let mut rbuf = vec![0u8; reply_len];
    stream.read_exact(&mut rbuf).map_err(|_| ())?;
    Ok(Some(String::from_utf8_lossy(&rbuf).into_owned()))
}

/// Run `program` using the axon-guest-kernel hypercall substrate for `host_await`.
///
/// The guest kernel exposes a Unix stream socket at the path given by
/// `AXON_HOST_SOCKET` (default: `/tmp/axon-host.sock`) that forwards each
/// `host_await` request to the host via VMCALL and returns the host's reply.
/// The interpreter itself runs entirely in ring-3 userspace — it never issues
/// VMCALL directly.
///
/// On connection failure (socket absent, wrong path) `unix_socket_roundtrip`
/// returns `Err(())`, which `host_await_yield` surfaces as end-of-input
/// (`None`) rather than panicking. A program that never calls `host_await`
/// completes normally even when no socket exists at the default path.
#[cfg(unix)]
pub fn run_suspendable_hypercall(program: &Program) -> i32 {
    let sock_path =
        std::env::var("AXON_HOST_SOCKET").unwrap_or_else(|_| "/tmp/axon-host.sock".to_string());
    UNIX_SOCK_PATH.with(|c| *c.borrow_mut() = Some(sock_path));
    let code =
        on_deep_stack(|| run_program_inner(program, crate::verify::Discharged::default(), true));
    UNIX_SOCK_PATH.with(|c| *c.borrow_mut() = None);
    code
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
    // Unix-socket hypercall path: takes priority over vsock and the worker-thread
    // channel. Active when run_suspendable_hypercall set UNIX_SOCK_PATH.
    #[cfg(unix)]
    {
        let path = UNIX_SOCK_PATH.with(|c| c.borrow().clone());
        if let Some(path) = path {
            let req_str = match &req {
                SendValue::Str(s) => s.clone(),
                other => format!("{other:?}"),
            };
            return unix_socket_roundtrip(&path, &req_str).map(|opt| opt.map(SendValue::Str));
        }
    }

    // vsock path: if running inside axon-vm, bypass the worker-thread channel.
    #[cfg(target_os = "linux")]
    {
        let port = VSOCK_PORT.with(|c| c.get());
        if port >= 0 {
            let req_str = match &req {
                SendValue::Str(s) => s.clone(),
                other => format!("{:?}", other),
            };
            return vsock_send_recv(port as u32, &req_str).map(|opt| opt.map(SendValue::Str));
        }
    }

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
    host: impl FnMut(SendValue) -> Option<SendValue>,
) -> i32 {
    run_suspendable_values_inner(program, host, /*map_status=*/ true)
}

/// [`run_suspendable_values`] returning the value the program PRODUCED rather
/// than the status a caller would observe — see [`run_program_unmapped`] for why
/// the two are separate, and why reading a result out of a program must not go
/// through the process-status rule.
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) fn run_suspendable_values_unmapped(
    program: &Program,
    host: impl FnMut(SendValue) -> Option<SendValue>,
) -> i32 {
    run_suspendable_values_inner(program, host, /*map_status=*/ false)
}

#[cfg(not(target_arch = "wasm32"))]
fn run_suspendable_values_inner(
    program: &Program,
    mut host: impl FnMut(SendValue) -> Option<SendValue>,
    map_status: bool,
) -> i32 {
    use std::sync::mpsc::channel;
    let (req_tx, req_rx) = channel::<SendValue>(); // worker → host (await requests)
    let (rep_tx, rep_rx) = channel::<Option<SendValue>>(); // host → worker (replies; None = EOF)

    // AUDIT T16 (finding INTERP-H03): this used a bare `scope.spawn`, giving the
    // worker the DEFAULT ~2 MiB stack. The tree-walking interpreter burns a lot
    // of native stack per call, so a suspendable program recursing a few hundred
    // frames deep hit a hard stack overflow (SIGSEGV) instead of the graceful
    // RECURSION_LIMIT panic — the guard exists precisely so the limit trips
    // first. The two sibling substrates (vsock, hypercall) already size their
    // threads this way, so this was an isolated omission rather than a design
    // choice. Same sizing as `on_deep_stack`.
    //
    // The worker must remain the thread that installs HOST_AWAIT (thread-local),
    // so the closure is sized in place rather than nested inside another thread.
    let stack = stack_size_for_depth(resolve_max_depth());
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .stack_size(stack)
            .spawn_scoped(scope, move || {
                HOST_AWAIT.with(|h| *h.borrow_mut() = Some(HostChannels { req_tx, rep_rx }));
                let code =
                    run_program_inner(program, crate::verify::Discharged::default(), map_status);
                // Drop the channels → req_tx closes → the host loop below ends.
                HOST_AWAIT.with(|h| *h.borrow_mut() = None);
                code
            })
            .expect("spawn interpreter worker");
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
pub fn run_suspendable(program: &Program, host: impl FnMut(&str) -> Option<String>) -> i32 {
    run_suspendable_str_inner(program, host, /*map_status=*/ true)
}

/// [`run_suspendable`] returning the value the program PRODUCED — the same
/// distinction [`run_program_unmapped`] draws, for the suspendable substrate.
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) fn run_suspendable_unmapped(
    program: &Program,
    host: impl FnMut(&str) -> Option<String>,
) -> i32 {
    run_suspendable_str_inner(program, host, /*map_status=*/ false)
}

#[cfg(not(target_arch = "wasm32"))]
fn run_suspendable_str_inner(
    program: &Program,
    mut host: impl FnMut(&str) -> Option<String>,
    map_status: bool,
) -> i32 {
    run_suspendable_values_inner(
        program,
        |req| {
            let s = match &req {
                SendValue::Str(s) => s.clone(),
                other => send_value_display(other),
            };
            host(&s).map(SendValue::Str)
        },
        map_status,
    )
}

/// wasm32 has no OS threads, so the worker-thread host-driver substrate can't run
/// here. Run the program directly with NO host driver: a `host_await` call then
/// hits the clean "called outside a suspendable run (no host driver)" panic
/// (exit 101), rather than trapping on `thread::spawn`. The browser binding (R7c)
/// will drive `host_await` via Asyncify + a JS import (R15 §13) — a different
/// substrate that replaces this one on wasm, with the same surface + semantics.
#[cfg(target_arch = "wasm32")]
pub fn run_suspendable(program: &Program, _host: impl FnMut(&str) -> Option<String>) -> i32 {
    run_program_inner(program, crate::verify::Discharged::default(), true)
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
    run_program_inner(program, crate::verify::Discharged::default(), true)
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

/// AUDIT T35 (finding RT-02): tell the AI runtime which hosts this program was
/// statically permitted to reach.
///
/// `axon check` validates `ai_complete` against the IMPLICIT constant host
/// `api.anthropic.com`, but the runtime resolved `AXON_AI_BASE_URL` /
/// `ANTHROPIC_BASE_URL` at call time — so the host the checker approved and the
/// host actually dialled were independent values, bridgeable by any program with
/// an fs:write grant via a `.env` file. Reproduced end-to-end: a program whose
/// `@[contained(net: ["api.anthropic.com"])]` passed `axon check` exit 0 sent its
/// prompt and the real `x-api-key` to `127.0.0.1`.
///
/// The union of every declared net allowlist is pinned. A program that declares
/// no `@[contained]` net grant is not pinned — it never made a claim to violate.
/// The union of every `@[contained(net: [...])]` host the program declares,
/// across top-level fns and impl methods. Empty = the program made no net claim.
///
/// Always compiled (not feature-gated) so it stays unit-testable in the default
/// build, where the `asi-runtime` AI runtime it feeds is absent.
#[cfg_attr(not(feature = "asi-runtime"), allow(dead_code))]
fn declared_net_hosts(program: &Program) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    let mut push = |c: &Option<crate::ast::ContainedSpec>| {
        if let Some(c) = c {
            for h in &c.net_allow {
                if !hosts.contains(h) {
                    hosts.push(h.clone());
                }
            }
        }
    };
    for item in &program.items {
        match item {
            Item::FnDef(f) => push(&f.contained),
            Item::ImplBlock(b) => {
                for m in &b.methods {
                    push(&m.contained);
                }
            }
            _ => {}
        }
    }
    if !hosts.is_empty() {
        // The implicit endpoint every AI builtin contacts is always permitted —
        // it is exactly what the static checker validated the grant against, so a
        // program that reaches an AI builtin at all has already been required to
        // declare it.
        let implicit = crate::capabilities::ai_implicit_host();
        if !hosts.iter().any(|h| h == implicit) {
            hosts.push(implicit.to_string());
        }
    }
    hosts
}

#[cfg(feature = "asi-runtime")]
fn pin_ai_net_allowlist(program: &Program) {
    let hosts = declared_net_hosts(program);
    if !hosts.is_empty() {
        axon_ai::pin_net_allowlist(hosts);
    }
}

/// Without the `asi-runtime` feature there is no live AI runtime to pin.
#[cfg(not(feature = "asi-runtime"))]
fn pin_ai_net_allowlist(_program: &Program) {}

/// `map_status` decides whether the value the program produced is turned into a
/// PROCESS EXIT STATUS (`stated_exit_status` / `returned_exit_status`) or handed
/// back raw.
///
/// The rule belongs at the process boundary and nowhere else. Two callers read
/// `main`'s return as a *value* rather than as a status, and must not see it
/// rewritten: the R10 G1 oracle, which compares `(exit_code, stdout)` between an
/// original and a transformed program — collapsing 6 and 7 both to 1 would make
/// a meaning-changing rewrite look equivalent — and the interpreter's own tests,
/// which use the return as the cheapest way to read a computed result out.
/// R44 Slice 1 — session capture, the API twin of `AXON_DUMP_BINDINGS`.
///
/// The prototype this grew from was driven entirely by env vars, which is fine
/// for an external driver but absurd for `axon session`: the session process
/// would have to set an environment variable on itself to talk to its own
/// interpreter, and then read its own answer back off the filesystem. This flag
/// is the same signal delivered in-process. The env vars still work — they are
/// the surface existing tests and external drivers already use.
static SESSION_CAPTURE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `(materialised_bindings, shape_inventory)` from the most recent session run.
///
/// Taken, not read: leaving a stale result behind would let a cell that produced
/// nothing silently inherit the previous cell's bindings.
///
/// A `Mutex`, NOT a `thread_local` — `on_deep_stack` runs the program on a
/// separate thread (the interpreter needs a bigger stack than the default), so a
/// thread-local result is stashed on a thread the caller never sees and every
/// cell reads `None`. That mistake presents exactly as "every cell failed at
/// runtime", which is a misleading enough symptom to be worth the comment.
static SESSION_RESULT: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

/// The reason the last run aborted, for a caller that cannot see stderr.
///
/// The abort arms below print `axon: panic: …` and return an exit code, and that
/// was the whole of the reporting. `axon session --protocol jsonl` builds its
/// frame from the exit code, so a cell that panicked came back as
/// `{"ok":false,"diagnostics":[]}` — a host driver was told the cell failed and
/// given no reason at all. A session is the surface a model iterates on; "it
/// failed" with no cause is the least actionable thing a compiler can say.
///
/// A `Mutex` for the same reason `SESSION_RESULT` is one: `on_deep_stack` runs
/// the program on another thread.
static LAST_ABORT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Record why a run aborted. Called beside each `eprintln!` so the two cannot
/// drift — a reader of either surface sees the same sentence.
fn set_last_abort(msg: String) {
    if let Ok(mut g) = LAST_ABORT.lock() {
        *g = Some(msg);
    }
}

/// Take the last abort reason, clearing it.
pub fn take_last_abort() -> Option<String> {
    LAST_ABORT.lock().ok().and_then(|mut g| g.take())
}

/// Enable in-process session capture (R44). Returns the previous setting.
pub fn set_session_capture(on: bool) -> bool {
    SESSION_CAPTURE.swap(on, std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn session_capture() -> bool {
    SESSION_CAPTURE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Take the `(bindings, shapes)` produced by the last session run, clearing it.
///
/// `None` means the run did not complete successfully — which is exactly the
/// case in which the session must NOT advance (R44 §4 S4).
pub fn take_session_result() -> Option<(String, String)> {
    SESSION_RESULT.lock().ok().and_then(|mut g| g.take())
}

/// Every binding visible at the end of `main`, as `name → shape`.
///
/// Describes EVERY binding, including ones [`materialise_bindings`] had to skip:
/// a closure or an aliased dict cannot be persisted, but its structure can still
/// be described, and a name the model can see but not reuse is the case it most
/// needs told about (R44 §4 S10).
fn describe_bindings(interp: &Interp) -> String {
    let locals = interp.main_locals.borrow();
    let mut merged: HashMap<&String, &Value> = interp.globals.iter().collect();
    for (k, v) in locals.iter() {
        merged.insert(k, v);
    }
    let mut names: Vec<&&String> = merged.keys().collect();
    names.sort();
    let mut out = String::new();
    for name in names {
        out.push_str(&format!("{name}: {}\n", value_shape(merged[*name])));
    }
    out
}

/// The session's bindings as Axon source literals, for the next cell's prelude.
///
/// `main`'s final top-level locals override globals of the same name — a cell
/// that mutated a binding persists the mutated value.
fn materialise_bindings(interp: &Interp) -> String {
    let locals = interp.main_locals.borrow();
    let mut merged: HashMap<&String, &Value> = interp.globals.iter().collect();
    for (k, v) in locals.iter() {
        merged.insert(k, v);
    }
    let mut names: Vec<&&String> = merged.keys().collect();
    names.sort();

    // A `Dict` is `Rc<RefCell<..>>` — SHARED MUTABLE state. Writing two aliasing
    // bindings out as two `dict_from_pairs(..)` calls would reconstruct them as
    // two INDEPENDENT dicts, so a later `dict_set(a, ..)` would stop being
    // visible through `b`. That is a silent semantic change across a cell
    // boundary, which is exactly the class of thing this session refuses rather
    // than fudges (the same call R15 made for `Chan`). So: find every dict
    // reachable from more than one binding and skip it with a reason.
    let mut seen: HashMap<*const (), usize> = HashMap::new();
    fn count_dicts(v: &Value, seen: &mut HashMap<*const (), usize>) {
        match v {
            Value::Dict(d) => {
                let key = Rc::as_ptr(d) as *const ();
                let e = seen.entry(key).or_insert(0);
                *e += 1;
                // Descend only the FIRST time this dict is seen. A dict can
                // contain itself (`dict_set(d, "self", d)`), and an
                // unconditional recursion would not terminate — a hang at
                // session-dump time, which is worse than the aliasing bug this
                // guard exists to prevent.
                if *e == 1 {
                    for inner in d.borrow().values() {
                        count_dicts(inner, seen);
                    }
                }
            }
            Value::Array(items) => {
                for it in items.iter() {
                    count_dicts(it, seen);
                }
            }
            Value::Tuple(items) => {
                for it in items {
                    count_dicts(it, seen);
                }
            }
            Value::Struct { fields, .. } | Value::Enum { fields, .. } => {
                for f in fields.values() {
                    count_dicts(f, seen);
                }
            }
            Value::Some(i) | Value::Ok(i) | Value::Err(i) => count_dicts(i, seen),
            _ => {}
        }
    }
    for name in &names {
        count_dicts(merged[**name], &mut seen);
    }
    let is_aliased = |v: &Value| -> bool {
        matches!(v, Value::Dict(d) if seen.get(&(Rc::as_ptr(d) as *const ())).copied().unwrap_or(0) > 1)
    };

    let mut lines = String::new();
    for name in names {
        // A binding that SHADOWS A BUILTIN must not persist. It is legal Axon
        // inside one cell — `let len = 5` merely warns (W0002) — but once it
        // reaches the prelude, every later cell that calls `len(xs)` dies with
        // E0306 "cannot call non-function value", and the session never
        // recovers. Measured: one task naming a variable `len` poisoned 14 of
        // the following cells in a tasks_hard run, which scored as Axon failing
        // tasks it can actually do.
        //
        // Skipping loses the value, which is why it is REPORTED — the
        // alternative is a session that silently breaks a builtin for every cell
        // after this one.
        if crate::builtins::is_known_builtin(name) {
            lines.push_str(&format!(
                "// SKIPPED {name}: shadows the builtin `{name}`; persisting it would \
                 break every later call to it\n"
            ));
            continue;
        }
        if is_aliased(merged[*name]) {
            lines.push_str(&format!(
                "// SKIPPED {name}: dict is shared with another binding; writing it out \
                 would split it into independent copies\n"
            ));
            continue;
        }
        match value_as_literal(merged[*name]) {
            Ok(lit) => lines.push_str(&format!("let {name} = {lit}\n")),
            Err(why) => lines.push_str(&format!("// SKIPPED {name}: {why}\n")),
        }
    }
    lines
}

fn run_program_inner(
    program: &Program,
    discharged: crate::verify::Discharged,
    map_status: bool,
) -> i32 {
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
    // R44 Slice 1: materialise the session's bindings. Two consumers now — the
    // AXON_DUMP_* env vars (the original prototype surface, kept so existing
    // tests and external drivers keep working) and `axon session`, which calls
    // the API rather than setting an env var on itself to talk to its own
    // interpreter. Only on success: a cell that failed must not mutate the
    // session (R44 §4 S4).
    if outcome.is_ok() {
        if session_capture() {
            let b = materialise_bindings(&interp);
            let s = describe_bindings(&interp);
            if let Ok(mut g) = SESSION_RESULT.lock() {
                *g = Some((b, s));
            }
        }
        if let Ok(spath) = std::env::var("AXON_DUMP_SHAPES") {
            let _ = std::fs::write(spath, describe_bindings(&interp));
        }
        if let Ok(path) = std::env::var("AXON_DUMP_BINDINGS") {
            let _ = std::fs::write(path, materialise_bindings(&interp));
        }
    }
    // Two different things, deliberately judged by two different rules: a status
    // the program STATED with `exit(n)` is honoured as written (the ledger
    // vocabulary is userland's to use), while a value that merely fell out of
    // `main` is an answer and may not impersonate a guard. See
    // `stated_exit_status` / `returned_exit_status`.
    let report = |(code, complaint): (i32, Option<String>)| -> i32 {
        if let Some(msg) = complaint {
            let _ = std::io::stdout().flush();
            eprintln!("{msg}");
        }
        code
    };
    match outcome {
        Ok(Value::Int(n)) if map_status => report(returned_exit_status(n)),
        Ok(Value::Int(n)) => n as i32,
        Ok(_) => 0,
        Err(Flow::Exit(code)) if map_status => report(stated_exit_status(code as i64)),
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
            set_last_abort(format!("ai policy: {msg}"));
            eprintln!("axon: ai policy: {msg}");
            AI_POLICY_EXIT_CODE
        }
        Err(Flow::Panic(msg)) => {
            let _ = std::io::stdout().flush();
            set_last_abort(format!("panic: {msg}"));
            eprintln!("axon: panic: {msg}");
            101
        }
        Err(Flow::Resume(_)) => {
            // `resume(..)` reached the top level — it was used outside a handler
            // arm (the resolver normally rejects this at check time; this is the
            // runtime backstop). A crash, not a silent exit.
            let _ = std::io::stdout().flush();
            set_last_abort("panic: `resume` called outside an effect-handler arm".to_string());
            eprintln!("axon: panic: `resume` called outside an effect-handler arm");
            101
        }
        Err(Flow::HandlerDone(..)) => {
            // A multi-shot handler's `HandlerDone` escaped its `with` block — an
            // interpreter bug (it is always caught by `eval_with_handler`). Treat
            // as a panic rather than a silent exit.
            let _ = std::io::stdout().flush();
            set_last_abort("panic: handler continuation escaped its `with` block".to_string());
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
    run_test_fn_outcome(program, name).map(|_| ())
}

/// How a test that did not fail ENDED — the affirmative evidence Protected
/// Check Isolation rests on. `Completed` means the test body itself returned
/// normally, with a value that is not an `Err`. That is NOT evidence that every
/// assertion ran: an assertion inside a closure handed to code that never calls
/// it does not execute, and the test still completes (PSV-3; a suite must
/// assert after the call). A
/// test ended by `exit(0)` from below it, or one that returned `Err`, is
/// `EndedEarly`: `axon test` still reports it as passing (unchanged
/// semantics), but no completion evidence is issued for it, so a check that
/// requires completion does not count it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestEnd {
    Completed,
    EndedEarly(String),
}

pub fn run_test_fn_outcome(program: &Program, name: &str) -> Result<TestEnd, String> {
    on_deep_stack(|| run_test_fn_inner(program, name))
}

fn run_test_fn_inner(program: &Program, name: &str) -> Result<TestEnd, String> {
    let mut interp = Interp::build(program);
    if let Err(f) = interp.init_globals() {
        return Err(flow_to_msg(f));
    }
    let Some(f) = interp.fns.get(name).copied() else {
        return Err(format!("no function `{name}`"));
    };
    interp.test_frame_depth.set(interp.call_depth.get() + 1);
    interp.test_body_finished.set(false);
    match interp.call_fn(f, vec![]) {
        // COMPLETED is decided on one fact: the test's own body evaluated to
        // its end (`call_fn_frame` records it). `call_fn` turns a `return` or
        // a `?` into an ordinary `Ok` at the test's frame, so the returned
        // VALUE cannot say whether the assertions after it ran: a `?` on a
        // type-confused `None` returned `None`, which read as a completion and
        // was minted a token (C9 round 3, PSV-3).
        Ok(_) if !interp.test_body_finished.get() => Ok(TestEnd::EndedEarly(
            "the test body ended early (a `return` or `?`): it did not complete".to_string(),
        )),
        Ok(Value::Err(_)) => Ok(TestEnd::EndedEarly(
            "the test returned `Err`: it did not complete".to_string(),
        )),
        Ok(_) => Ok(TestEnd::Completed),
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
        Err(Flow::Exit(0)) => Ok(TestEnd::EndedEarly(
            "`exit(0)` ended the test before it completed".to_string(),
        )),
        Err(Flow::Exit(n)) => Err(format!("exited with code {n}")),
        // Unreachable: `call_fn` ends a `return` at the callee. Kept only so
        // the match stays total; it grants nothing.
        Err(Flow::Return(_)) => Err("`return` escaped the test's own frame".to_string()),
        // A `break` / `continue` (or an effect-handler completion) that
        // escapes a function unwinds the test BEFORE its assertions ran: the
        // test did not complete, so it did not pass. It used to count as
        // clean, which let a candidate's function end the operator's
        // acceptance test early and have Fabric sign a pass (v0.22 G01 final
        // re-audit, executed).
        Err(Flow::Break) | Err(Flow::Continue) => Err(
            "a `break`/`continue` escaped a function and unwound the test before it completed"
                .to_string(),
        ),
        Err(Flow::HandlerDone(..)) => {
            Err("an effect handler completed outside its handled computation".to_string())
        }
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
        pin_ai_net_allowlist(program);
        let seal = {
            let dirs = crate::resolver::sealed_module_dirs();
            let mut seal = Seal {
                active: !dirs.is_empty(),
                ..Seal::default()
            };
            if seal.active {
                let sealed = |sp: crate::span::Span| crate::resolver::span_in_sealed(sp, &dirs);
                for item in &program.items {
                    match item {
                        Item::FnDef(f) if sealed(f.span) => {
                            seal.fns.insert(f as *const FnDef as usize);
                        }
                        Item::ImplBlock(b) => {
                            for m in &b.methods {
                                if sealed(m.span) || sealed(b.span) {
                                    seal.fns.insert(m as *const FnDef as usize);
                                }
                            }
                        }
                        Item::LetDef { name, span, .. } if sealed(*span) => {
                            seal.globals.insert(name.clone());
                        }
                        Item::RefineDef(r) if sealed(r.span) => {
                            seal.refines.insert(r.name.clone());
                        }
                        Item::TypeDef(t) if sealed(t.span) => {
                            seal.types.insert(t.name.clone());
                        }
                        _ => {}
                    }
                    match item {
                        Item::ImplBlock(b) if !sealed(b.span) => {
                            for m in &b.methods {
                                if !sealed(m.span) {
                                    seal.operator_methods.insert(m.name.clone());
                                }
                            }
                        }
                        Item::TraitDef(t) if !sealed(t.span) => {
                            for m in &t.methods {
                                seal.operator_methods.insert(m.name.clone());
                            }
                        }
                        _ => {}
                    }
                }
            }
            seal
        };
        let mut fns = HashMap::new();
        let mut structs = HashMap::new();
        let mut enums = HashMap::new();
        let mut methods = HashMap::new();
        let mut impl_of = HashMap::new();
        let mut user_traits = std::collections::HashSet::new();
        let mut trait_impls = std::collections::HashSet::new();
        let mut refine_bases = HashMap::new();
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
                    refine_bases.insert(r.name.clone(), &r.base);
                }
                Item::TraitDef(t) => {
                    user_traits.insert(t.name.clone());
                }
                Item::TypeDef(t) => {
                    structs.insert(t.name.clone(), t);
                }
                Item::EnumDef(e) => {
                    enums.insert(e.name.clone(), e);
                }
                Item::ImplBlock(b) => {
                    let tn = type_name_of(&b.for_type);
                    if !b.trait_name.is_empty() {
                        trait_impls.insert((tn.clone(), b.trait_name.clone()));
                    }
                    for m in &b.methods {
                        methods.insert((tn.clone(), m.name.clone()), m);
                        impl_of.insert(m as *const FnDef as usize, b);
                    }
                }
                Item::LetDef { name, value, .. } => {
                    global_defs.push((name.clone(), value.as_ref()));
                }
                _ => {}
            }
        }

        // Read the ambient effect ceiling once; both the sandbox registry
        // and the active-handle field below are derived from it.
        // Both kernels start identical — the SAME ambient effect ceiling, so a
        // sealed frame may narrow it but never widen it.
        let mk_kernel = |sealed: bool| {
            let ambient = ambient_sandbox();
            Kernel {
                rng: Cell::new(0),
                rng_sealed: sealed,
                provenance: RefCell::new(HashMap::new()),
                provenance_inputs: RefCell::new(HashMap::new()),
                provenance_inputs_f64: RefCell::new(HashMap::new()),
                corrigible_halted: Cell::new(false),
                current_principal: RefCell::new(
                    std::env::var("AXON_PRINCIPAL")
                        .ok()
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "root".to_string()),
                ),
                goal_constraint: RefCell::new(None),
                principals: RefCell::new(if sealed {
                    crate::kernel::PrincipalRegistry::new_sealed()
                } else {
                    crate::kernel::PrincipalRegistry::new()
                }),
                // Scheduler order is a function of spawn order + AXON_SEED (R12 §5
                // determinism): derive the round-robin start offset from the seed.
                scheduler: RefCell::new(crate::kernel::Scheduler::new(rng_seed() as usize)),
                supervisors: RefCell::new(Vec::new()),
                stores: RefCell::new(Vec::new()),
                llm_gateways: RefCell::new(Vec::new()),
                goals: RefCell::new(Vec::new()),
                active_sandbox: Cell::new(if ambient.is_empty() { -1 } else { 0 }),
                sandboxes: RefCell::new(ambient),
            }
        };
        let pins = if seal.active {
            let dirs = crate::resolver::sealed_module_dirs();
            pin::Pins::build(program, &|sp| crate::resolver::span_in_sealed(sp, &dirs))
        } else {
            pin::Pins::default()
        };
        Interp {
            kernels: [mk_kernel(false), mk_kernel(true)],
            seal,
            pins,
            pin_fn: Cell::new(0),
            frame_sealed: Cell::new(false),
            sealed_frames: Cell::new(0),
            operator_frames: Cell::new(0),
            test_frame_depth: Cell::new(0),
            test_body_finished: Cell::new(false),
            fns,
            structs,
            enums,
            methods,
            impl_of,
            user_traits,
            trait_impls,
            refine_bases,
            chan_contracts: RefCell::new(HashMap::new()),
            dict_snaps: RefCell::new(HashMap::new()),
            dict_epoch: std::cell::Cell::new(0),
            fn_cx_cache: RefCell::new(HashMap::new()),
            global_defs,
            globals: HashMap::new(),
            call_depth: Cell::new(0),
            max_depth: resolve_max_depth(),
            enclosing_agent: RefCell::new(None),
            current_goal: RefCell::new(None),
            current_fn: RefCell::new(String::new()),
            current_call_tier: RefCell::new(None),
            resolved_callees: RefCell::new(HashMap::new()),
            ai_calls_this_fn: Cell::new(0),
            ai_cost_micro: Cell::new(0),
            w1310_warned: RefCell::new(std::collections::HashSet::new()),
            tokens_used: Cell::new(0),
            token_budget: parse_token_budget(),
            handlers: RefCell::new(Vec::new()),
            resume_replay: RefCell::new(None),
            resume_ctx: RefCell::new(Vec::new()),
            refine_preds,
            main_locals: RefCell::new(HashMap::new()),
            discharged: crate::verify::Discharged::default(),
            gfx_mock: RefCell::new(crate::native::GfxMock::new()),
            #[cfg(not(target_arch = "wasm32"))]
            domain: axon_domain::Registry::new(),
            #[cfg(feature = "gfx-wgpu")]
            gfx_real: RefCell::new(axon_gfx::GfxReal::new()),
            taint: taint::Taint::default(),
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
        for (name, expr) in &defs {
            // Each initializer runs under its own definition's provenance, in a
            // FRESH environment: earlier globals are reached through
            // `self.globals`, so the global-read edge (`seal_global`) sees every
            // read. A shared env let a later initializer read an earlier global
            // as a LOCAL, past the edge (PCI candidate-4 review).
            let sealed = self.seal.active && self.seal.globals.contains(name);
            let mut env = Env::new();
            let v = self.with_frame(sealed, || self.eval(expr, &mut env))?;
            if self.seal.active {
                self.t_set_global(name, self.taint.last.get());
            }
            self.globals.insert(name.clone(), v);
        }
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
        // Parsed by the SHARED `budget_from_attrs`, so "is this fn metered?" has
        // exactly one answer here and in the native codegen refusal (F141).
        match crate::ai_routing::budget_from_attrs(&f.attrs)? {
            Ok(n) => Some(n),
            Err(raw) => {
                eprintln!(
                    "warning: [{}] @[ai(policy(budget: {raw}))] on `{name}` is not a \
                     non-negative integer — ignored (fn runs unmetered)",
                    crate::error::W1311
                );
                None
            }
        }
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
        self.k().current_principal.borrow().clone()
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

    /// May the runtime persist a provenance row right now?
    ///
    /// Asks the SAME question `pre_effect_gate` asks for a builtin, through the
    /// same predicate, so the ceiling cannot mean one thing for `write_file`
    /// and another for the telemetry writer. `IO` is the row the effect
    /// catalog gives filesystem and console builtins.
    fn provenance_write_permitted(&self) -> bool {
        let handle = self.k().active_sandbox.get();
        if handle < 0 {
            return true; // no ceiling in force
        }
        let sbs = self.k().sandboxes.borrow();
        match sbs.get(handle as usize) {
            Some(sb) => {
                crate::interp::builtins::first_effect_outside_ceiling(sb, &["IO"]).is_none()
            }
            // UNREACHABLE BY CONSTRUCTION, and deliberately fail-closed
            // anyway. `active_sandbox` only ever holds -1 or an index that
            // was valid when it was stored, so there is no way to observe
            // this arm — a mutation flipping it to `true` survives the suite,
            // which is the honest signal that it is untested rather than a
            // test gap to paper over with a contrived fixture.
            //
            // Worth recording that `pre_effect_gate` diverges here: its
            // `if let Some(sb) = sbs.get(..)` has no else, so in this same
            // impossible state it SKIPS the ceiling check entirely and fails
            // OPEN. If the invariant on `active_sandbox` is ever weakened,
            // that is the site that turns into a hole, not this one.
            None => false,
        }
    }

    fn call_fn(&self, f: &FnDef, args: Vec<Value>) -> R {
        // A `&mut` param must be moved back to the caller (`call_fn_mut`); a
        // path that cannot do that (a fn reached by name string, a method)
        // would silently drop the callee's writes — refuse instead.
        if f.params
            .iter()
            .any(|p| matches!(p.ty, crate::ast::AxonType::RefMut(_)))
        {
            return panic(format!(
                "`{}` takes `&mut` parameters and can only be called directly as `{}(&mut a, ...)`",
                f.name, f.name
            ));
        }
        let mut env = Env::new();
        self.call_fn_sealed(f, args, &mut env)
    }

    /// AX-08: call `f` with its `&mut` arguments already MOVED out of the
    /// caller's bindings (no copy). Returns the call's result and, per param
    /// index, the param's final value (`Unit` for non-`&mut` params) for the
    /// caller to move back — on every outcome, including `return` / `?` /
    /// error unwinds, so the caller's binding is never left hollow.
    pub(super) fn call_fn_mut(&self, f: &FnDef, args: Vec<Value>) -> (R, Vec<Value>, Vec<u8>) {
        let mut env = Env::new();
        // A candidate fn writing through `&mut` returns control to operator
        // code with the parameter's FINAL value, which is moved into the
        // operator's binding: a second return channel, so a seal crossing
        // exactly as the return value is (C9 round 8, PSV-1 blocker).
        let crossing = self.seal.active && self.fn_is_sealed(f) && !self.frame_sealed.get();
        // What the operator held, to judge a replacement against (dicts,
        // amendment 78). Only a sealed crossing pays the clone.
        let held: Vec<Option<Value>> = f
            .params
            .iter()
            .zip(&args)
            .map(|(p, a)| match (&p.ty, crossing) {
                (crate::ast::AxonType::RefMut(_), true) => Some(a.clone()),
                _ => None,
            })
            .collect();
        let mut result = self.call_fn_sealed(f, args, &mut env);
        // The body's block scopes are popped by now (on `return`/`?` too), so
        // each name resolves to the parameter binding itself.
        let mut outs: Vec<Value> = f
            .params
            .iter()
            .map(|p| match (&p.ty, env.get_mut(&p.name)) {
                (crate::ast::AxonType::RefMut(_), Some(slot)) => {
                    std::mem::replace(slot, Value::Unit)
                }
                _ => Value::Unit,
            })
            .collect();
        // What each `&mut` parameter holds when the call ends is a second return
        // channel for taint, as it is for the value: the callee's own binding for
        // an operator fn, everything for a candidate's (amendment 102).
        let out_ts: Vec<u8> = f
            .params
            .iter()
            .map(|p| match (&p.ty, self.seal.active) {
                (crate::ast::AxonType::RefMut(_), true) if crossing => taint::ALL,
                (crate::ast::AxonType::RefMut(_), true) => env.taint_of(&p.name),
                _ => 0,
            })
            .collect();
        if crossing {
            let cx = self.fn_cx(f).strict(true);
            // A refused value is never handed back: the binding is left `()`
            // and the call's result is the refusal.
            let refuse = |result: &mut R, out: &mut Value, msg: String| {
                *out = Value::Unit;
                if !matches!(result, Err(Flow::Panic(_))) {
                    *result = panic(msg);
                }
            };
            for (i, p) in f.params.iter().enumerate() {
                if !matches!(p.ty, crate::ast::AxonType::RefMut(_)) {
                    continue;
                }
                if let Err(why) = self.cast(&mut outs[i], &p.ty, &cx) {
                    let msg = format!(
                        "`&mut` argument `{}` of `{}` is declared `{}` but was left holding {} — \
                         a runtime type confusion ({why})",
                        p.name,
                        f.name,
                        crate::doc::render_type(&p.ty),
                        value::display(&outs[i])
                    );
                    refuse(&mut result, &mut outs[i], msg);
                    continue;
                }
                if let Some(old) = &held[i] {
                    if let Err(why) = self.replaced_ok_top(old, &outs[i]) {
                        let msg = format!(
                            "`&mut` argument `{}` of `{}` was left holding a value the operator's \
                             did not allow — a runtime type confusion ({why})",
                            p.name, f.name
                        );
                        refuse(&mut result, &mut outs[i], msg);
                    }
                }
            }
        }
        (result, outs, out_ts)
    }

    fn call_fn_sealed(&self, f: &FnDef, args: Vec<Value>, env: &mut Env) -> R {
        // PCI runtime sealing: the CALL edge (see `Seal`).
        self.seal_call(f)?;
        // The WHOLE call — parameter refinements, body, return refinement,
        // `@[verify]` — is one frame for loop control, and runs under the
        // callee's provenance.
        let callee = self.fn_is_sealed(f);
        // A candidate fn returning to operator code: the seal crossing where
        // a value at an undetermined type parameter is refused (amendment 53).
        let crossing = self.seal.active && callee && !self.frame_sealed.get();
        // What an early exit inside the frame raises on the control taints is the
        // frame's own business: restored below, whatever the outcome (amendment 102).
        // The fn's frame is compiled twice (`call_fn_frame::<true>` with the taint hooks,
        // `::<false>` without them: amendment 102).
        let (ctl_pc, ctl_sticky) = (self.taint.pc.get(), self.taint.sticky.get());
        // The operator hands the candidate its arguments: every dict in them
        // is snapshotted; at the return every dict sealed code mutated is
        // checked against it (amendment 72 part 2).
        if crossing {
            for a in &args {
                self.dict_edge_in(a)?;
            }
        }
        let r = self.with_frame(callee, || {
            contain_frame(
                if self.seal.active {
                    self.call_fn_frame::<true>(f, args, crossing, env)
                } else {
                    self.call_fn_frame::<false>(f, args, crossing, env)
                },
                &format!("`{}`", f.name),
            )
        });
        self.taint.pc.set(ctl_pc);
        self.taint.sticky.set(ctl_sticky);
        // Operator code in a scheduler fiber (or in the body of a `with handler`
        // that can abort it) that called sealed code: a panic in the callee would
        // have ended the fiber here, and an operation it performed would have
        // ended the body, so what the fiber does
        // after this point is control-dependent on the candidate whether or not
        // it panicked (amendment 117, loop finding 15).
        if crossing && (self.taint.catchable.get() | self.taint.abortable.get()) != 0 {
            let st = &self.taint.sticky;
            st.set(ctl_sticky | taint::VAL);
        }
        if crossing && r.is_ok() {
            self.dict_edge_out()?;
        }
        r
    }

    /// The type environment of `f`'s signature for ONE activation: the
    /// signature part is cached per fn; the type-parameter bindings are fresh.
    fn fn_cx(&self, f: &FnDef) -> conform::Cx {
        let key = f as *const FnDef as usize;
        let cached = self.fn_cx_cache.borrow().get(&key).cloned();
        let cx = match cached {
            Some(cx) => cx,
            None => {
                let cx = conform::Cx::of_fn(f, self.impl_of.get(&key).copied());
                self.fn_cx_cache.borrow_mut().insert(key, cx.clone());
                cx
            }
        };
        cx.fresh()
    }

    /// Whether `f` was defined in a sealed (candidate) module.
    fn fn_is_sealed(&self, f: &FnDef) -> bool {
        self.seal.active && self.seal.fns.contains(&(f as *const FnDef as usize))
    }

    /// The call edge: a sealed frame may run only sealed functions.
    fn seal_call(&self, f: &FnDef) -> Result<(), Flow> {
        if self.seal.active && self.frame_sealed.get() && !self.fn_is_sealed(f) {
            return panic(Self::sealed_no_fn_msg(&f.name));
        }
        Ok(())
    }

    /// What sealed code is told when a name does not resolve FOR IT: the
    /// operator's fn and a fn that does not exist read the same, so a sealed
    /// caller cannot use the refusal to learn which names the operator defines
    /// (existence oracle, C9 round 10).
    pub(crate) fn sealed_no_fn_msg(name: &str) -> String {
        format!(
            "sealed code (the candidate under test) cannot use `{name}`: no such function or \
             value is visible to it"
        )
    }

    /// The error for a name that resolves to nothing: the common text above for
    /// a sealed caller, `plain` for anyone else.
    pub(crate) fn no_such_fn<T>(&self, name: &str, plain: String) -> Result<T, Flow> {
        if self.seal.active && self.frame_sealed.get() {
            return panic(Self::sealed_no_fn_msg(name));
        }
        panic(plain)
    }

    /// The NAME rule's edge (C9 round 10, amendment 100): operator code does not
    /// hand a name-resolving builtin (`sandbox_run`, `scheduler_spawn`,
    /// `goal_eval` ...) a name the candidate could have chosen. A `str` the
    /// candidate returned is a perfectly good `str`, and the builtin then ran
    /// whichever operator fn it named. Judged at the CALL SITE, by the static
    /// name-purity analysis (`interp/pin.rs`): an operator literal, constant or
    /// operator fn result passes; a candidate fn's result, a parameter or any
    /// untyped read does not. Fail-closed: an unrecorded site is refused.
    pub(crate) fn seal_name_args(&self, builtin: &str, args: &[Expr]) -> Result<(), Flow> {
        if !self.seal.active || self.frame_sealed.get() {
            return Ok(());
        }
        let Some(idxs) = pin::name_sink(builtin) else {
            return Ok(());
        };
        #[cfg(test)]
        if DISPATCH_RULE_OFF.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        for &i in idxs {
            if let Some(a) = args.get(i) {
                if !self.pins.name_determined(self.pin_fn.get(), a) {
                    return panic(format!(
                        "operator code gave `{builtin}` a function name nothing on the operator \
                         side determined (argument {}) — the candidate would choose which \
                         function runs; name it with a literal or a constant the operator wrote",
                        i + 1
                    ));
                }
            }
        }
        Ok(())
    }

    /// The method-dispatch edge: in operator code, a method name the operator
    /// defines is the operator's. A method call selects its method by the
    /// receiver's RUNTIME type, which the candidate chooses (its declared
    /// return type, or a type confusion), so without this edge the candidate
    /// chose which code ran under the operator's judging method's name — the
    /// C9 round-4 review's keyed pass. A candidate's OWN method names (its
    /// API, which the suite may call) are not affected.
    pub(crate) fn seal_method(&self, f: &FnDef, tn: &str) -> Result<(), Flow> {
        if self.seal.active
            && !self.frame_sealed.get()
            && self.fn_is_sealed(f)
            && self.seal.operator_methods.contains(&f.name)
        {
            return panic(format!(
                "operator code called `.{}()`, a method the operator defines, on a value of type \
                 `{tn}` whose `{}` is the candidate's — the candidate would choose the code that \
                 runs under the operator's method",
                f.name, f.name
            ));
        }
        Ok(())
    }

    /// The DISPATCH rule (C9 round 6, amendment 83): in a sealed run, operator
    /// code does not select an operator impl's method by the runtime type of a
    /// receiver nothing on the operator side determined (`interp/pin.rs`). The
    /// impl is chosen by that type, which for a value read from a dict, a
    /// channel, an unannotated lambda parameter or an unbound generic position
    /// is the candidate's choice. Applies only where there is an impl to choose
    /// between (two or more operator impl types define the method).
    pub(crate) fn seal_dispatch(
        &self,
        receiver: &Expr,
        f: &FnDef,
        recv: &Value,
        tn: &str,
    ) -> Result<(), Flow> {
        if !self.seal.active || self.frame_sealed.get() || self.fn_is_sealed(f) {
            return Ok(());
        }
        // Tests of the OTHER seal layers observe their attacks through a dispatch
        // on an untyped read — exactly what this rule refuses first. They run
        // with the rule off (as a paired-disable cell does) so each layer is
        // judged by its own attack; the rule's own tests run with it on.
        #[cfg(test)]
        if DISPATCH_RULE_OFF.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        if !self.pins.selects_between_impls(&f.name) {
            return Ok(());
        }
        // A receiver that is an operator-defined struct or enum selects an
        // impl the operator chose: sealed code cannot build one (E0004).
        let operator_value = match recv {
            Value::Struct { name, .. } => self.pins.is_operator_type(name),
            Value::Enum { enum_name, .. } => self.pins.is_operator_type(enum_name),
            _ => false,
        };
        if operator_value {
            return Ok(());
        }
        if !self.pins.determined(self.pin_fn.get(), receiver, &f.name) {
            return panic(format!(
                "operator code dispatched `{}` on a value whose type nothing on the operator side \
                 determined (here `{tn}`) — the candidate would choose the impl; pin it with \
                 `let x: T = ...`",
                f.name
            ));
        }
        Ok(())
    }

    /// The dispatch rule's arithmetic arm (amendment 83): operator code does
    /// not do arithmetic on a fixed-width integer whose WIDTH nothing on the
    /// operator side determined — a `u8` the candidate chose wraps where the
    /// operator's `i64` does not (`(v << 1) == 254` with `v = 255 as u8`).
    pub(crate) fn seal_width(
        &self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        l: &Value,
        r: &Value,
    ) -> Result<(), Flow> {
        if !self.seal.active || self.frame_sealed.get() {
            return Ok(());
        }
        let sized = |v: &Value| width_sized(v);
        if !(sized(l) || sized(r)) {
            return Ok(());
        }
        if matches!(
            op,
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq
        ) {
            return Ok(());
        }
        #[cfg(test)]
        if DISPATCH_RULE_OFF.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        if !self
            .pins
            .determined_arith(self.pin_fn.get(), op, left, right)
        {
            return panic(format!(
                "operator code did arithmetic on a fixed-width integer whose width nothing on the \
                 operator side determined ({} {:?} {}) — the candidate would choose the \
                 wrapping; pin it with `let x: T = ...`",
                l.type_name(),
                op,
                r.type_name()
            ));
        }
        Ok(())
    }

    /// The arithmetic arm's UNARY form: `-x` and `~x` on a fixed-width integer
    /// wrap at a width the candidate chose exactly as `x + y` does, and the
    /// binary arm never saw them (C9 round 9 sweep: `(-xs[0]) == 252` completed
    /// for a candidate `4 as u8`).
    pub(crate) fn seal_width_unary(
        &self,
        op: &UnaryOp,
        operand: &Expr,
        v: &Value,
    ) -> Result<(), Flow> {
        if !self.seal.active || self.frame_sealed.get() {
            return Ok(());
        }
        if !width_sized(v) || !matches!(op, UnaryOp::Neg | UnaryOp::BitNot) {
            return Ok(());
        }
        #[cfg(test)]
        if DISPATCH_RULE_OFF.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(());
        }
        if !self.pins.determined_unary(self.pin_fn.get(), op, operand) {
            return panic(format!(
                "operator code did arithmetic on a fixed-width integer whose width nothing on the \
                 operator side determined ({op:?} {}) — the candidate would choose the wrapping; \
                 pin it with `let x: T = ...`",
                v.type_name()
            ));
        }
        Ok(())
    }

    /// The global-read edge: a sealed frame may not read an operator global.
    pub(crate) fn seal_global(&self, name: &str) -> Result<(), Flow> {
        if self.seal.active && self.frame_sealed.get() && !self.seal.globals.contains(name) {
            return panic(Self::sealed_no_fn_msg(name));
        }
        Ok(())
    }

    /// Whether `name` is a module-level `let`: existence only, no value.
    pub(crate) fn is_global(&self, name: &str) -> bool {
        self.globals.contains_key(name)
    }

    /// THE ONLY read of a module-level `let` by name from running code: the
    /// global-read edge ([`Interp::seal_global`]) applied at the one lookup, so
    /// no arm (a receiver fast path, an index fast path, a closure-constant
    /// call) can reach `self.globals` around it (C9 round 9: `TABLE[0]`,
    /// `CFG.k`, `PAIR.0` read an operator global from sealed code because two
    /// fast paths skipped the edge). `Ok(None)`: no such global.
    /// Drift: `every_global_read_goes_through_global_ref`.
    pub(crate) fn global_ref(&self, name: &str) -> Result<Option<&Value>, Flow> {
        match self.globals.get(name) {
            Some(v) => {
                self.seal_global(name)?;
                Ok(Some(v))
            }
            None => Ok(None),
        }
    }

    /// The refinement edge: a candidate's refinement never runs in operator
    /// code (a refinement attaches BY NAME, so a candidate `type DictTable =
    /// … where P` ran P inside the operator's helper — PCI candidate-3 review).
    pub(crate) fn seal_refine(&self, rname: &str) -> Result<(), Flow> {
        if self.seal.active && !self.frame_sealed.get() && self.seal.refines.contains(rname) {
            return panic(format!(
                "the refinement `{rname}` is the candidate's and cannot run in the operator's code"
            ));
        }
        Ok(())
    }

    /// Look up a function by NAME for a builtin that will run it (scheduler,
    /// goal, sandbox …). The call edge applies at RESOLUTION time, so a name a
    /// sealed frame queues cannot run later on the operator's behalf.
    pub(crate) fn fn_by_name(&self, name: &str) -> Result<Option<&'p FnDef>, Flow> {
        match self.fns.get(name).copied() {
            Some(f) => {
                self.seal_call(f)?;
                Ok(Some(f))
            }
            None => Ok(None),
        }
    }

    /// Whether struct type `name` was defined in a sealed module.
    pub(crate) fn seal_type(&self, name: &str) -> bool {
        self.seal.active && self.seal.types.contains(name)
    }

    /// The kernel of the frame now running: the operator's, or the sealed
    /// candidate's own. Handle-addressed state is only ever reached through it.
    fn k(&self) -> &Kernel {
        &self.kernels[usize::from(self.frame_sealed.get())]
    }

    /// The ONLY way to draw a random number: from the running frame's kernel.
    /// A sealed frame draws from the candidate's own stream and so can never
    /// advance or observe the operator's (PSV-1).
    ///
    /// OPERATOR code running ABOVE a sealed frame (a closure the suite handed
    /// the candidate, an operator handler arm answering a candidate operation)
    /// is REFUSED: the candidate decides how many times such code runs, so
    /// every draw it made would advance the operator's stream by a count the
    /// candidate chose, and steer the operator's next challenge (C9 round 3,
    /// PSV-1, amendment 46).
    pub(crate) fn rng_next(&self) -> Result<u64, Flow> {
        self.rng_guard()?;
        Ok(self.k().rng_next())
    }

    /// The ONLY way to reseed (`srand`): the running frame's kernel only,
    /// under the same refusal as [`Interp::rng_next`].
    pub(crate) fn rng_reseed(&self, n: i64) -> Result<(), Flow> {
        self.rng_guard()?;
        self.k().rng_set(n);
        Ok(())
    }

    fn rng_guard(&self) -> Result<(), Flow> {
        if !self.frame_sealed.get() && self.sealed_frames.get() > 0 {
            return panic(
                "the operator's random stream cannot be used by operator code that sealed code \
                 (the candidate under test) is running — the candidate would choose how far it \
                 advances; draw before handing the code to the candidate",
            );
        }
        Ok(())
    }

    /// Whether handler `frame` may answer (or abort) the operation being
    /// performed now. An operator frame may answer anything. A SEALED frame
    /// may answer only while no operator frame has been entered since it was
    /// installed: otherwise the operation belongs to — or unwinds through —
    /// the operator's code, and the candidate would choose its result or skip
    /// the rest of it (C9 round 3, PSV-1). The single predicate for live
    /// frames (`call_builtin`, `run_handler_arm`) and the replay feed.
    pub(crate) fn handler_may_answer(&self, sealed: bool, operator_frames: usize) -> bool {
        !sealed || operator_frames == self.operator_frames.get()
    }

    /// Run `g` with the frame's provenance set to `sealed`, restoring it after.
    pub(crate) fn with_frame<T>(&self, sealed: bool, g: impl FnOnce() -> T) -> T {
        let prev = self.frame_sealed.replace(sealed);
        let count = if sealed {
            &self.sealed_frames
        } else {
            &self.operator_frames
        };
        count.set(count.get() + 1);
        if sealed {
            self.taint.entries.set(self.taint.entries.get() + 1);
        }
        let out = g();
        count.set(count.get() - 1);
        self.frame_sealed.set(prev);
        out
    }

    fn call_fn_frame<const T: bool>(
        &self,
        f: &FnDef,
        args: Vec<Value>,
        crossing: bool,
        env: &mut Env,
    ) -> R {
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
        let _pin_guard = PinGuard {
            cell: &self.pin_fn,
            prev: self.pin_fn.replace(f as *const FnDef as usize),
        };
        // The taint the arguments arrived with (their union, in `acc`), and the
        // frame's own program-counter taint, which an early exit out of a tainted
        // branch may raise and the caller never sees (amendment 102).
        let entry_t = if T { self.taint.acc.get() } else { 0 };
        // (The control taints are saved and restored around the whole frame by
        // `call_fn_sealed`, which is the only caller.)
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
        if self.k().corrigible_halted.get() && f.attrs.iter().any(|a| a.name == "corrigible") {
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
        // the fn's signature in `run_goal`. Only an `@[adaptive]` fn records
        // them, so every other call skips the two allocations.
        let is_adaptive_zone = f.attrs.iter().any(|a| a.name == "adaptive");
        let (input_args, input_args_f64): (Vec<i64>, Vec<f64>) = if is_adaptive_zone {
            (
                args.iter()
                    .take_while(|v| matches!(v, Value::Int(_)))
                    .map(|v| if let Value::Int(n) = v { *n } else { 0 })
                    .collect(),
                args.iter()
                    .take_while(|v| matches!(v, Value::Float(_)))
                    .map(|v| if let Value::Float(f) = v { *f } else { 0.0 })
                    .collect(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        // First dim, for back-compat with the verify-panic enrichment that
        // reports a single "input N" suffix.
        let input_arg: Option<i64> = match args.first() {
            Some(Value::Int(n)) => Some(*n),
            _ => None,
        };
        // The signature's type environment for this activation: every
        // argument and the result are CAST to the declared types in it
        // (amendment 53, `interp/conform.rs`).
        let cx = self.fn_cx(f);
        for (p, a) in f.params.iter().zip(args) {
            // Soft typing: `Uncertain<T>` is compatible with a plain-`T` parameter
            // (the checker allows it). If the declared param type is NOT itself a
            // soft wrapper but the argument IS one, unwrap to the inner value so
            // the body sees a plain `T` (else `x * 2` on the struct silently
            // produced 0). Confidence/horizon dropped at this T-typed boundary.
            let param_is_soft = conform::is_soft_decl(&p.ty);
            let a = if !param_is_soft {
                value::soft_inner(&a).unwrap_or(a)
            } else {
                a
            };
            // R19 Slice B: coerce Int→SizedInt when the declared param type is a
            // non-i64 integer width — ensures arithmetic inside the callee's body
            // uses width-correct ops (completeness, I-9).
            let mut a = if let Some(width) = interp_eval_axon_type_to_width(&p.ty) {
                interp_eval_coerce_to_sized(a, width)
            } else {
                a
            };
            if let Err(why) = self.cast(&mut a, &p.ty, &cx) {
                return panic(format!(
                    "argument `{}` of `{}` is declared `{}` but is {} — a runtime type confusion \
                     ({why})",
                    p.name,
                    f.name,
                    crate::doc::render_type(&p.ty),
                    value::display(&a)
                ));
            }
            // An operator fn's typed parameter is a pin: the cast above verified the
            // argument against a closed type the operator wrote.
            let mut pt = 0;
            if T {
                pt = entry_t | self.taint.pc.get();
                if pt & taint::TYP != 0 && !self.frame_sealed.get() && self.t_pins(&p.ty) {
                    pt &= !taint::TYP;
                }
            }
            env.define(p.name.clone(), a, pt);
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
                        self.seal_refine(rname)?;
                        let val = env.get(&p.name).cloned().unwrap_or(Value::Unit);
                        let mut pred_env = Env::new();
                        pred_env.define("_".into(), val.clone(), entry_t);
                        // Also bind the parameter name (for inline refinements
                        // `p: T where E[p] > k` that use the param name directly).
                        pred_env.define(p.name.clone(), val.clone(), entry_t);
                        if let Value::Bool(false) =
                            contain_frame(self.eval_t::<T>(pred, &mut pred_env), "a predicate")?
                        {
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
        let mut goal_ran = false;
        if let Some(spec) = self.goal_spec_of(f) {
            goal_ran = true;
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
        env.define(
            "goal_met".into(),
            Value::Int(goal_met),
            // The goal search ran the (possibly sealed) metric.
            if goal_ran { taint::ALL } else { 0 },
        );
        // PROTOTYPE (RLM session option 2): when dumping bindings, run main's
        // top-level statements WITHOUT the extra block scope (eval_block pops
        // its scope before returning, discarding the locals), then capture the
        // frame's final locals for the dump.
        let capture = f.name == "main"
            && self.call_depth.get() == 1
            && (session_capture()
                || std::env::var("AXON_DUMP_BINDINGS").is_ok()
                || std::env::var("AXON_DUMP_SHAPES").is_ok());
        let body_result = if capture {
            if let Expr::Block(stmts) = &f.body {
                let mut last = Ok(Value::Unit);
                for stmt in &stmts[..] {
                    match self.eval_t::<T>(&stmt.expr, env) {
                        Ok(v) => last = Ok(v),
                        Err(e) => {
                            last = Err(e);
                            break;
                        }
                    }
                }
                last
            } else {
                self.eval_t::<T>(&f.body, env)
            }
        } else {
            self.eval_t::<T>(&f.body, env)
        };
        // The taint of the value the body produced (a `return` carries its own),
        // before anything else is evaluated.
        let mut body_t = if T {
            if matches!(body_result, Err(Flow::Return(_))) {
                self.taint.ret.get()
            } else {
                self.taint.last.get()
            }
        } else {
            0
        };
        if capture && !matches!(body_result, Err(ref e) if !matches!(e, Flow::Return(_))) {
            let mut snap = env.snapshot();
            snap.remove("goal_met"); // injected by call_fn, not a user binding
            *self.main_locals.borrow_mut() = snap;
        }
        // PSV-3: the test frame's completion is WHETHER ITS BODY RAN TO ITS
        // END. A `return` or a `?` (whatever it carried) did not.
        if depth == self.test_frame_depth.get() {
            self.test_body_finished.set(body_result.is_ok());
        }
        let mut result = match body_result {
            Ok(v) => v,
            Err(Flow::Return(v)) => v,
            // Loop control never crosses a function boundary: a `break` or
            // `continue` with no loop of its own in this body is an error HERE,
            // not a jump in whatever loop the CALLER happens to be running.
            // It used to escape, so candidate code could end an operator
            // test's loop early and skip its assertions (v0.22 G01 final
            // re-audit of candidate 2, executed).
            Err(Flow::Break) | Err(Flow::Continue) => {
                return panic(format!("`break`/`continue` outside a loop in `{}`", f.name))
            }
            Err(other) => return Err(other),
        };
        // The result is CAST to the declared return type (C9 round 3 checked
        // only a `Result`/`Option` constructor against the other; round 4,
        // amendment 53, checks the whole declared type — scalar kind, struct
        // or enum name and fields, `Option`/`Result` payloads, array and tuple
        // elements, trait bounds, type parameters bound by the arguments).
        // An untyped `dict_get` yields a free type variable, so a stored
        // value of ANY type type-checks as the declared one; refused here, at
        // the one boundary every return crosses, it never reaches a caller
        // that would dispatch a method on its runtime type. At a seal
        // crossing (`crossing`), a value at a type parameter no argument
        // determined is refused too.
        {
            let mismatch = match f.return_type.as_ref() {
                Some(rt) => self.cast(&mut result, rt, &cx.strict(crossing)).err(),
                // No declared return type: the checker types the call `()`,
                // so at a seal crossing the operator receives exactly `()`.
                // The body's last value was handed out uncast, and an
                // operator method call on it ran the impl for whatever type
                // the candidate chose (C9 round 4c, amendment 72).
                None if crossing => {
                    result = Value::Unit;
                    None
                }
                None => None,
            };
            let confused = mismatch.is_some();
            if confused {
                return panic(format!(
                    "`{}` is declared to return `{}` but produced {} — a runtime type confusion \
                     ({})",
                    f.name,
                    f.return_type
                        .as_ref()
                        .map(crate::doc::render_type)
                        .unwrap_or_default(),
                    value::display(&result),
                    mismatch.unwrap_or_default()
                ));
            }
            // A declared return of an OPERATOR fn is a pin: the cast above
            // verified the result against a closed type the operator wrote.
            if body_t & taint::TYP != 0
                && !self.frame_sealed.get()
                && f.return_type.as_ref().is_some_and(|rt| self.t_pins(rt))
            {
                body_t &= !taint::TYP;
            }
        }
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
                    self.seal_refine(rname)?;
                    // R20 Slice 2: evaluate the predicate with `_` bound to the
                    // return value AND the fn's params still in scope, so a
                    // RELATIONAL return refinement (e.g.
                    // `-> (i64 where _ <= parent_rem)`) can reference parameters.
                    // `env` already holds the param bindings from the body; add
                    // `_` and evaluate against it instead of a bare env.
                    env.define("_".into(), result.clone(), body_t);
                    if let Value::Bool(false) =
                        contain_frame(self.eval_t::<T>(pred, env), "a predicate")?
                    {
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
                    self.t_provenance_write(entry_t | self.taint.last.get(), true, false);
                    self.k()
                        .provenance
                        .borrow_mut()
                        .entry(f.name.clone())
                        .or_default()
                        .push(score);
                    self.k()
                        .provenance_inputs
                        .borrow_mut()
                        .entry(f.name.clone())
                        .or_default()
                        .push(input_args.clone());
                    self.k()
                        .provenance_inputs_f64
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
                // PROVENANCE IS RUNTIME-OWNED TELEMETRY, NOT A PROGRAM EFFECT.
                //
                // This append is a durable filesystem write that no capability
                // mechanism used to see: it is not a builtin call, so it never
                // reached `pre_effect_gate`, where the ceiling is enforced.
                // MEASURED — every one of these produced a row on disk, exit 0:
                // a direct `@[adaptive]` call under
                // `@[contained(fs: [], net: [], exec: none)]`; the same under
                // `AXON_ALLOWED_EFFECTS=Pure`; and inside
                // `sandbox_run(sandbox_create(p, ""), …)`, whose ceiling is
                // deny-all. `goal_run` amplifies it — `max_evals <= 0` means
                // unlimited, so a contained fn could drive an unbounded number
                // of rows whose `score`, `input` and `payload` it chooses. That
                // is a general durable store reached through the audit
                // machinery, which is exactly what must not exist.
                //
                // The remedy is SUPPRESSION, not refusal. Refusing would abort
                // every contained optimiser, and the optimiser does not need
                // this write: the in-memory best store pushed just above is
                // what `goal_run` reads, and it is untouched here. So a program
                // denied filesystem effects keeps optimising and simply leaves
                // no durable trace — it gains no capability it was refused, and
                // there is no channel to amplify.
                //
                // Cross-process resume (`AXON_GOAL_CONTINUE`, which reads the
                // log) does degrade under a restrictive ceiling. That is the
                // correct direction: durable state is what was not granted.
                if self.provenance_write_permitted() {
                    self.t_provenance_write(entry_t | self.taint.last.get(), false, true);
                    append_provenance_jsonl(&f.name, &payload, score, input_arg, zone, label);
                }
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
                            pred_env.define("value".into(), v.clone(), body_t);
                        }
                        if let Some(c) = fields.get("confidence") {
                            pred_env.define("confidence".into(), c.clone(), body_t);
                        }
                        if let Some(s) = fields.get("source_tag") {
                            pred_env.define("source_tag".into(), s.clone(), body_t);
                        }
                        let outcome = contain_frame(
                            self.eval_t::<T>(&spec.predicate, &mut pred_env),
                            "a predicate",
                        )?;
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
                    pred_env.define("value".into(), result.clone(), body_t);
                    let outcome = contain_frame(
                        self.eval_t::<T>(&spec.predicate, &mut pred_env),
                        "a predicate",
                    )?;
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

        // The call's value carries what the arguments arrived with and what the
        // body produced.
        if T {
            // The result also carries an early exit's condition (`sticky`).
            body_t |= self.taint.sticky.get() & taint::VAL;
            self.taint.acc.set(entry_t | body_t);
        }
        Ok(result)
    }

    /// A callback a BUILTIN runs (arr_any/all/find/take_while/drop_while,
    /// arr_sort_by's comparator, every `arr_*`/`dict_*` higher-order builtin):
    /// the number of calls, and which ones run, is decided by what the earlier
    /// results said, so every call AFTER the first is control-dependent on them
    /// (amendment 117, PSV-1 loop findings 3/10/18/27/40/49). The taint was
    /// reaching the callback only through its PARAMETERS; a store that does not
    /// use the parameter (`dict_inc(op, "c")`) was clean. The control taint of
    /// everything the callbacks returned so far is raised for the rest of the
    /// builtin's life (the builtin dispatch restores it), so the body of the
    /// next call runs under it and so does anything stored after the loop.
    /// Operator frames only: a sealed frame's pc is already everything.
    fn call_cb(&self, c: Value, args: Vec<Value>) -> R {
        let r = self.call_closure(c, args)?;
        self.t_loop_pc();
        Ok(r)
    }

    fn call_closure(&self, c: Value, args: Vec<Value>) -> R {
        if self.seal.active {
            self.call_closure_owned_by::<true>(c, args, 1)
        } else {
            self.call_closure_owned_by::<false>(c, args, 1)
        }
    }

    /// `f(..)` where `f` names a closure in the caller's env, `c` being a clone
    /// of that binding: the binding is the one reference to the capture cell
    /// besides `c` that the body can never reach (see `call_closure_owned_by`).
    fn call_local_closure(&self, c: Value, args: Vec<Value>) -> R {
        if self.seal.active {
            self.call_closure_owned_by::<true>(c, args, 2)
        } else {
            self.call_closure_owned_by::<false>(c, args, 2)
        }
    }

    /// Run closure `c`. `private_refs` counts the references to its capture
    /// cell that nothing the body runs can reach: `c` itself, plus the
    /// caller's binding for a call by local name — an `Env` is only ever
    /// visible to the frame evaluating it, since closures, fns, handler arms
    /// and continuation replays all run on their own (snapshot) envs.
    fn call_closure_owned_by<const T: bool>(
        &self,
        c: Value,
        args: Vec<Value>,
        private_refs: usize,
    ) -> R {
        let Value::Closure {
            params,
            body,
            captured,
            contract,
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
        let mut args = args;
        // Operator code does not run an operator closure the candidate picked
        // (amendment 102): the selection primitive of a table of closures.
        if T {
            self.t_check_call_picked(&captured)?;
        }
        // The taint the arguments arrived with; from a sealed caller, everything.
        let entry_t = self.taint.acc.get();
        let in_t = if !T {
            0
        } else if self.frame_sealed.get() {
            taint::ALL
        } else {
            entry_t | self.taint.pc.get()
        };
        // A closure runs under the provenance of the frame that CREATED it.
        let origin = self.seal.active && captured.borrow().contains_key(SEALED_CLOSURE_MARK);
        // The arguments are cast to every `fn` type this reference crossed. A
        // sealed frame calling an OPERATOR closure is a seal crossing: the
        // arguments are cast strictly (amendment 72).
        let entering = self.seal.active
            && self.frame_sealed.get()
            && !origin
            && !captured.borrow().contains_key(SEALED_FNVAL_MARK);
        // A candidate closure called by operator code: the operator hands it
        // the arguments (snapshot). A sealed frame calling an operator closure
        // returns control to operator code (verify what it mutated).
        if origin && !self.frame_sealed.get() {
            for a in &args {
                self.dict_edge_in(a)?;
            }
        }
        if entering {
            self.dict_edge_out()?;
        }
        self.closure_args_check(&contract, &mut args, entering)?;
        // Operator code a sealed frame runs is control-dependent on the
        // candidate (whether, and how often, it is called).
        // The control taints are restored right after the body ran (below).
        let ctl = if T {
            Some((self.taint.pc.get(), self.taint.sticky.get()))
        } else {
            None
        };
        if entering {
            self.taint.pc.set(self.taint.pc.get() | taint::ALL);
        }
        let _pin_guard = if self.seal.active {
            let owner = match captured.borrow().get(PIN_FN_MARK) {
                Some(Value::Int(n)) => *n as usize,
                _ => 0,
            };
            Some(PinGuard {
                cell: &self.pin_fn,
                prev: self.pin_fn.replace(owner),
            })
        } else {
            None
        };
        // Base scope = captured bindings; a fresh scope holds the parameters.
        // Assignments land in the base scope and are written back below, which
        // is what makes them survive to the next call (T40).
        //
        // AX-31: if the cell has no owner besides the private ones, no call the
        // body makes can re-enter this closure, so the bindings are MOVED out of
        // the cell for the call and moved back after it. A captured array is then
        // uniquely owned and `xs[i] = v` writes in place. Copying them instead
        // (the general case) made every such write copy the whole array — but
        // that copy is required when the closure is reachable (passed as an
        // argument, stored in an array, dict or capture, or currently running):
        // a re-entrant call must see the cell as it stood before this call, and
        // this call's in-place writes would otherwise destroy that state.
        let lend = Rc::strong_count(&captured) == private_refs;
        let mut env = Env::new();
        if lend {
            env.load_captured(captured.borrow_mut().drain());
        } else {
            env.load_captured(
                captured
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone())),
            );
        }
        env.push();
        for (i, (p, a)) in params.iter().zip(args).enumerate() {
            // A parameter whose declared type is closed is a pin: the argument was
            // cast to it (strictly, from a sealed caller), so its runtime type is
            // the operator's. The value is not (amendment 102).
            let mut t = in_t;
            if t & taint::TYP != 0
                && Self::param_types(&contract, i)
                    .iter()
                    .any(|ty| self.t_pins(ty))
            {
                t &= !taint::TYP;
            }
            env.define(p.clone(), a, t);
        }
        // A closure's own `return` ends the closure; every other transfer is
        // refused at its edge, as for a named fn (`contain_frame`).
        // A candidate closure returning to operator code is a seal crossing.
        let crossing = origin && !self.frame_sealed.get();
        let mut ret_t = 0u8;
        let out = self.with_frame(origin, || {
            contain_frame(
                match self.eval_t::<T>(&body, &mut env) {
                    Err(Flow::Return(v)) => {
                        if T {
                            ret_t = self.taint.ret.get();
                        }
                        Ok(v)
                    }
                    other => {
                        if T {
                            ret_t = self.taint.last.get();
                        }
                        other
                    }
                },
                "a closure",
            )
        });
        ret_t |= self.taint.sticky.get() & taint::VAL;
        if let Some((pc, sticky)) = ctl {
            self.taint.pc.set(pc);
            self.taint.sticky.set(sticky);
        }
        // Write back only names the closure actually captured. A `let` introduced
        // inside the body lives in a pushed scope and must not leak into the
        // capture; a parameter shadowing a captured name must not overwrite it
        // either, which is why the params live in their own pushed scope above.
        {
            let mut cell = captured.borrow_mut();
            if lend {
                // The cell is empty and the base scope holds exactly its keys.
                env.drain_base_scope_into(&mut cell);
            } else {
                for (k, v, t) in env.base_scope() {
                    if let Some(slot) = cell.get_mut(k) {
                        *slot = v.clone();
                    } else {
                        continue;
                    }
                    // What the name holds now carries what the body gave it.
                    if T {
                        let ck = format!("{}{k}", taint::CAP_T);
                        if *t != 0 {
                            cell.insert(ck, Value::Int(i64::from(*t)));
                        } else {
                            cell.remove(&ck);
                        }
                    }
                }
            }
        }
        // The result is cast to every `fn` type this reference crossed.
        let mut v = out?;
        self.closure_ret_check(&contract, &mut v, crossing)?;
        // A candidate closure returns to operator code: verify. An operator
        // closure returns into sealed code: its result is handed over.
        if crossing {
            self.dict_edge_out()?;
        } else if entering {
            self.dict_edge_in(&v)?;
        }
        if T {
            self.taint.acc.set(entry_t | ret_t);
        }
        Ok(v)
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
        // A sealed caller names its OWN goals (an operator fn is not visible to it,
        // and answers like a name nothing defines: the one existence message).
        let defined = match self.fns.get(name) {
            Some(f) => !(self.seal.active && self.frame_sealed.get()) || self.fn_is_sealed(f),
            None => false,
        };
        defined || self.k().provenance.borrow().contains_key(name)
    }

    fn unknown_goal_name(&self, name: &str) -> Flow {
        if self.seal.active && self.frame_sealed.get() {
            return Flow::Panic(Self::sealed_no_fn_msg(name));
        }
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
        Literal::Str(s) => Value::Str(Rc::new(s.clone())),
        Literal::Decimal(m) => Value::Decimal(*m),
    }
}

pub(crate) fn type_name_of(ty: &crate::ast::AxonType) -> String {
    use crate::ast::AxonType::*;
    match ty {
        Named(n) => n.clone(),
        Generic { base, .. } => base.clone(),
        Ref(inner) | RefMut(inner) | RawPtr(inner) => type_name_of(inner),
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
fn as_decimal(v: &Value) -> Result<i128, Flow> {
    match v {
        Value::Decimal(m) => Ok(*m),
        other => panic(format!("expected Decimal, got {}", other.type_name())),
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
// Declared-type conformance at every value boundary (C9 round 4, PSV-1,
// amendment 53).
pub mod conform;
// The dispatch rule: an operator impl is never selected by a type nothing on
// the operator side determined (C9 round 6, amendment 83).
mod pin;
mod taint;
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
// Re-exported for CODEGEN, so the caps->effect-row rule has exactly one
// implementation. Native agent_action records carried no effect row until
// the ABI was widened to pass one; re-deriving it on the runtime side would
// have created a second copy of this mapping, free to drift from the
// interpreter's.
// Only CODEGEN consumes this; the interpreter calls the fn directly inside its
// own module. Gated on the same feature, or a --no-default-features build fails
// on an unused import.
#[cfg(feature = "codegen")]
pub(crate) use builtins::cap_to_effect_row;
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
// `sha256_hex` is re-exported crate-internally so `replay.rs` can fingerprint a
// journal payload with the SAME hash the provenance/AI-replay caches use, rather
// than adding a second digest implementation that could disagree with them.
pub(crate) use provenance::sha256_hex;
pub use provenance::{
    append_run_start_jsonl, best_recorded_score, find_run_start, provenance_log_path,
    read_ai_calls, read_provenance, set_provenance_source, set_session_cell, AiCallRecord,
    ProvRecord, RunStartRecord,
};

/// Parse the ambient run-level token cap from `AXON_BUDGET_TOKENS`.
///
/// Unset means no cap. A malformed value FAILS CLOSED to `Some(0)` — no AI at
/// all — rather than falling back to no-cap, because this is a safety control
/// an operator sets from outside the program: a typo (`1O000`) silently
/// disarming the cap is the same inert-surface failure the var had before
/// anything read it, just one level down. The warning names the bad value so
/// the typo is recoverable in one look.
///
/// A negative value clamps to 0, which is meaningful and distinct from unset.
fn parse_token_budget() -> Option<i64> {
    let raw = std::env::var("AXON_BUDGET_TOKENS").ok()?;
    let t = raw.trim();
    match t.parse::<i64>() {
        Ok(n) => Some(n.max(0)),
        Err(_) => {
            eprintln!(
                "warning: AXON_BUDGET_TOKENS={raw:?} is not an integer — failing closed \
                 to a budget of 0 (no AI calls). Set a whole number of tokens, or unset \
                 the variable for no cap."
            );
            Some(0)
        }
    }
}

/// Build the ambient sandbox from `AXON_ALLOWED_EFFECTS`, or an empty registry
/// when the var is unset.
///
/// The var is the ambient counterpart to `sandbox_create`: a comma-separated
/// effect ceiling for the whole run, for a caller who cannot edit the program to
/// wrap it in an explicit sandbox. It was documented as "enforced at runtime"
/// and nothing read it, so `AXON_ALLOWED_EFFECTS=Pure` let an FS write through
/// and exited 0 — a control surface that reads as a safety mechanism while a run
/// configured with it behaves exactly like an unconfigured one.
///
/// The enforcement was never the missing part: `call_builtin`'s F5 hook already
/// refuses any effect outside the active sandbox's set (SandboxViolation, exit
/// 8). It is gated on `active_sandbox >= 0` and nothing ambient ever set that.
/// So this registers a sandbox and makes it active, reusing the same
/// `SandboxEntry` and the same check as `sandbox_create` rather than adding a
/// second enforcement path that could drift from it.
///
/// An EMPTY value is meaningful and is NOT the same as unset:
/// `AXON_ALLOWED_EFFECTS=` means "pure only, deny every effect", while unset
/// means "no ceiling at all". Distinguishing them matters because
/// deny-everything is a case a caller reaches for deliberately.
///
/// The scope is unscoped (`Default`) — this grants effects without a path/host
/// restriction, matching plain `sandbox_create`. Bound to principal handle 0 so
/// audit attribution matches the rest of the run.
fn ambient_sandbox() -> Vec<SandboxEntry> {
    let Ok(raw) = std::env::var("AXON_ALLOWED_EFFECTS") else {
        return Vec::new();
    };
    let allowed: std::collections::HashSet<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    // A misspelled entry is silently a grant of NOTHING. That fails closed,
    // which is the right direction — but it also means an operator who writes
    // `AXON_ALLOWED_EFFECTS=IO,Exce` believes they granted `Exec` and is told
    // otherwise only if the program happens to spawn. A grant that was never
    // valid, recorded as granted: say so at parse time, the way
    // `AXON_BUDGET_TOKENS` already does for a malformed value.
    let mut unknown: Vec<&String> = allowed
        .iter()
        .filter(|e| !crate::builtins::is_grantable_effect(e))
        .collect();
    if !unknown.is_empty() {
        unknown.sort();
        for e in unknown {
            eprintln!(
                "warning: AXON_ALLOWED_EFFECTS names `{e}`, which is not an effect — \
                 it grants nothing. Valid names: {}",
                crate::builtins::GRANTABLE_EFFECTS.join(", ")
            );
        }
    }
    vec![SandboxEntry {
        principal: 0,
        allowed,
        scope: SandboxScope::default(),
    }]
}

/// Initial seed for the RNG. Reproducibility (BUG_HUNT #11 / I-10):
/// uses `AXON_SEED` (parsed as u64) when set for a deterministic run,
/// otherwise time-based entropy. A fixed seed makes every `random_*`,
/// `goal_run_random`, and `goal_run_multistart` result replayable.
/// Whether arithmetic on `v` runs at a fixed WIDTH: a fixed-width integer, or
/// an `Uncertain`/`Temporal` whose inner `value` is one (`eval_binop_vals`
/// unwraps the soft wrapper and runs the inner operation at the inner width, so
/// a width rule that looked only at the top-level value never saw it: amendment
/// 117, PSV-1 loop finding 52).
pub(crate) fn width_sized(v: &Value) -> bool {
    match v {
        Value::SizedInt { .. } => true,
        Value::Struct { name, fields } if name == "Uncertain" || name == "Temporal" => {
            fields.get("value").is_some_and(width_sized)
        }
        _ => false,
    }
}

fn rng_seed() -> u64 {
    if let Ok(s) = std::env::var("AXON_SEED") {
        if let Ok(n) = s.trim().parse::<u64>() {
            return n | 1; // avoid the 0 "uninitialized" sentinel
        }
    }
    (now_ms() as u64) | 1
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

/// Build an `Uncertain { value, confidence }` struct value with
/// `source_tag = 0` (user-constructed).
fn make_uncertain(value: Value, confidence: f64) -> Value {
    make_uncertain_tagged(value, confidence, SRC_TAG_USER)
}

/// An `Uncertain` whose value came from a MODEL (`ai_extract_uncertain_*`, on
/// the mock, replay and live paths alike). Stamps 1, as codegen does. These
/// paths used `make_uncertain` and so stamped 0 — a model's answer read as
/// user-constructed under `axon run`, the fail-open direction for a provenance
/// field (UPGRADE_V0_20.md D-014).
fn make_uncertain_ai(value: Value, confidence: f64) -> Value {
    make_uncertain_tagged(value, confidence, SRC_TAG_AI)
}

/// `source_tag` values, as stamped by codegen. These are OBSERVABLE — the
/// checker lists `source_tag` as a field of `Uncertain<T>` and both engines
/// let a program read `u.source_tag` — so the interpreter must stamp the same
/// number codegen does, or a program that branches on provenance takes a
/// different branch under `axon run` than under `axon build`.
pub(crate) const SRC_TAG_USER: i64 = 0;
pub(crate) const SRC_TAG_AI: i64 = 1;
pub(crate) const SRC_TAG_RUNTIME: i64 = 2;

/// Build an `Uncertain { value, confidence, source_tag }` struct value.
///
/// `source_tag` (0=user-constructed, 1=AI-sourced, 2=runtime) is a field of the
/// Uncertain struct — the checker lists it as a valid field and codegen builds
/// the 3-field `{value, confidence, source_tag}` layout. Without it here,
/// `u.source_tag` type-checked but panicked at runtime ("no field source_tag")
/// and the interp's 2-field struct diverged from codegen's 3-field one.
///
/// It was then HARDCODED to 0 for every construction path, which diverged
/// again in the opposite direction: codegen stamps 2 for the `uncertain_dyn_*`
/// constructors, so `uncertain_dyn_f64(1.0, 0.5).source_tag` printed 0 under
/// the interpreter and 2 natively. The interpreter — the reference engine and
/// the default execution path — was reporting a runtime-sourced value as
/// user-constructed, which is the fail-open direction for a provenance field.
fn make_uncertain_tagged(value: Value, confidence: f64, source_tag: i64) -> Value {
    let mut fields = HashMap::new();
    fields.insert("value".to_string(), value);
    fields.insert("confidence".to_string(), Value::Float(confidence));
    fields.insert("source_tag".to_string(), Value::Int(source_tag));
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

/// R42 Slice 5: the Pike VM regex engine. Its own module because it is a real
/// compiler+VM rather than a builtin body, and because its refusals
/// (backreferences, lookaround, oversized counted repetition) are a security
/// boundary worth reading in one place.
mod regex;
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

/// Coerce Int → SizedInt. Other values (a `SizedInt` of another width
/// included) pass through unchanged and meet the declared-type cast, which
/// refuses another width (amendment 60). Used at the call-arg → param
/// boundary.
fn interp_eval_coerce_to_sized(v: Value, width: crate::types::Type) -> Value {
    match v {
        Value::Int(n) => Value::SizedInt { val: n, ty: width },
        other => other,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {

    /// v0.22 G01 final re-audit: a `break` escaping a function called by a
    /// test used to count as a clean pass, so candidate code could end an
    /// acceptance test before its assertion ran. It is a failure now; a test
    /// that completes still passes.
    /// Candidate 4's final-review blocker (executed): a `break` in a candidate
    /// function's parameter/return refinement, `@[verify]` predicate, or a
    /// candidate type's struct refinement escaped the call and ended the
    /// operator test's loop. Loop control is contained at every frame edge now.
    #[test]
    fn loop_control_does_not_escape_through_a_predicate() {
        let cases = [
            (
                "return refinement",
                "fn solve(n: i64) -> (i64 where if n > 0 { break } else { true }) { 0 }\n",
            ),
            (
                "param refinement",
                "fn solve(n: i64 where if n > 0 { break } else { true }) -> i64 { 0 }\n",
            ),
            (
                "verify",
                "@[verify(if value == 0 { break } else { true })]\nfn solve(n: i64) -> i64 { 0 }\n",
            ),
        ];
        let mut passed = Vec::new();
        for (why, def) in cases {
            let src = format!(
                "{def}@[test]\nfn t() {{\n    let mut i = 1\n    while i < 4 {{\n        assert_eq(solve(i), 42)\n        i = i + 1\n    }}\n}}\n"
            );
            let prog = crate::parse_source(&src).expect("parses");
            if run_test_fn(&prog, "t").is_ok() {
                passed.push(why);
            }
        }
        let prog = crate::parse_source(
            "type Arg = { n: i64 } where if _.n > 0 { break } else { true }\n\
             @[test]\nfn t() {\n    for i in 1..4 {\n        let a = Arg { n: i }\n        assert_eq(a.n, 42)\n    }\n}\n",
        )
        .expect("parses");
        if run_test_fn(&prog, "t").is_ok() {
            passed.push("struct refinement");
        }
        assert!(
            passed.is_empty(),
            "these escapes passed the test: {passed:?}"
        );
    }

    /// Serialises the interpreter tests that set the process-global sealed
    /// directory set (`resolver::set_sealed_module_dirs`).
    pub(super) static SEALED_DIRS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// A second `impl J for E` must never replace the first's methods. The
    /// interpreter keys methods by (type, name) and keeps the LAST, so a
    /// duplicate impl whose `check` is a no-op would silently disarm the
    /// operator's. Two resolver checks refuse it (the impl-uniqueness check
    /// and the per-(type, method) dispatch check), each on its own; this
    /// accepts EITHER refusal (E0002), and fails "ATTACK:" only when the
    /// pipeline accepts the program and the no-op actually ran.
    #[test]
    fn a_second_impl_never_replaces_the_first_impls_method() {
        let head = "type E = { want: i64 }\ntrait J { fn check(self: E, got: i64) }\n\
                    impl J for E { fn check(self: E, got: i64) { assert_eq(got, self.want) } }\n\
                    @[test]\nfn t() {\n    let e = E { want: 42 }\n    e.check(0)\n}\n";
        let errors = |src: &str| -> Vec<String> {
            crate::check_pipeline(src, "t.ax")
                .into_iter()
                .filter(|d| d.severity == "error")
                .map(|d| format!("{} {}", d.code, d.message))
                .collect()
        };
        // Control: the operator's check is live — the program is well formed
        // and its test FAILS on the wrong answer.
        assert!(errors(head).is_empty(), "control: {:?}", errors(head));
        let prog = crate::parse_source(head).expect("parses");
        assert!(
            run_test_fn_outcome(&prog, "t").is_err(),
            "control: check is live"
        );
        // Attack: a second impl whose `check` accepts anything.
        let src = format!("{head}impl J for E {{ fn check(self: E, got: i64) {{ }} }}\n");
        let e = errors(&src);
        if e.is_empty() {
            let prog = crate::parse_source(&src).expect("parses");
            let out = run_test_fn_outcome(&prog, "t");
            assert!(
                out.is_err(),
                "ATTACK: a second `impl J for E` replaced the operator's `check`: {out:?}"
            );
        }
        assert!(
            e.len() == 1 && e[0].starts_with("E0002"),
            "a duplicate impl is refused by exactly one E0002: {e:?}"
        );
    }

    /// A sealed frame that queues an operator function as a fiber never gets
    /// it run. `scheduler_spawn` seal-checks the name when it is queued; were
    /// that check absent, the fiber would still never run an operator
    /// function, on EITHER route to its execution:
    /// * run from a sealed frame: every fiber is called through `call_fn`,
    ///   whose call edge refuses an operator function in a sealed frame;
    /// * run by the operator: it cannot be — a fiber lives in the kernel of
    ///   the frame that queued it, and a sealed frame's kernel is its own.
    ///
    /// So this accepts any refusal, and fails "ATTACK:" only when an operator
    /// function actually ran. (Moved here from
    /// `runtime_sealing_holds_without_the_static_check`, whose two fiber cases
    /// asserted the queue-time refusal itself — a refusal reason on routes the
    /// call edge and the per-provenance kernel each dominate.)
    #[test]
    fn a_sealed_fiber_never_runs_an_operator_function() {
        use crate::span::intern_source;
        let suite = "fn expected(n: i64) -> i64 { n * 2 }\n\
                     @[test]\nfn t() { assert_eq(double(21), expected(21)) }\n\
                     @[test]\nfn t_later() {\n    let d = double(21)\n    scheduler_run()\n    assert(d == 42 && scheduler_done_count() > 0)\n}\n\
                     @[test]\nfn t_later_control() {\n    let _ = scheduler_spawn(\"expected\", 1)\n    scheduler_run()\n    assert(scheduler_done_count() > 0)\n}\n";
        let run = |cand: &str, test: &str| {
            let s = crate::parse_source_in(suite, intern_source("/pci-fb-suite/h.ax", suite))
                .expect("suite parses");
            let c = crate::parse_source_in(cand, intern_source("/pci-fb-sealed/f.ax", cand))
                .expect("candidate parses");
            let prog = Program {
                items: s.items.into_iter().chain(c.items).collect(),
            };
            // The sealed set is process-global: hold the lock across
            // set/run/clear so a concurrent sealed test cannot swap it.
            let _g = SEALED_DIRS_TEST_LOCK
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            crate::resolver::set_sealed_module_dirs(&[std::path::PathBuf::from("/pci-fb-sealed")]);
            let out = run_test_fn_outcome(&prog, test);
            crate::resolver::set_sealed_module_dirs(&[]);
            out
        };
        // Controls: a sealed frame may queue and run its OWN function as a
        // fiber and read the result, and the operator's scheduler runs what
        // the OPERATOR queues — so each attack below fails on the seal, not
        // on the scheduler.
        let honest = "fn double(n: i64) -> i64 { n * 2 }\n";
        let own = "fn twice(n: i64) -> i64 { n * 2 }\n\
                   fn double(n: i64) -> i64 {\n    let id = scheduler_spawn(\"twice\", n)\n    scheduler_run()\n    scheduler_result(id)\n}\n";
        assert_eq!(run(own, "t"), Ok(TestEnd::Completed), "control: own fiber");
        assert_eq!(
            run(honest, "t_later_control"),
            Ok(TestEnd::Completed),
            "control: the operator's scheduler runs the operator's fiber"
        );
        // Run from the sealed frame, returning the operator's answer.
        let now = "fn double(n: i64) -> i64 {\n    let id = scheduler_spawn(\"expected\", n)\n    scheduler_run()\n    scheduler_result(id)\n}\n";
        let out = run(now, "t");
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: a sealed frame ran the operator's `expected` as a fiber and returned its answer: {out:?}"
        );
        // Queued for the operator's scheduler to run later.
        let later = "fn double(n: i64) -> i64 {\n    let _ = scheduler_spawn(\"expected\", n)\n    n * 2\n}\n";
        let out = run(later, "t_later");
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: the operator's scheduler ran an operator function a sealed frame queued: {out:?}"
        );
    }

    /// Run `test` of `suite` with `cand` loaded from a SEALED directory
    /// (`/<tag>-sealed`), holding the sealed-set lock across set/run/clear.
    fn sealed_outcome(tag: &str, suite: &str, cand: &str, test: &str) -> Result<TestEnd, String> {
        sealed_outcome_rule(tag, suite, cand, test, false)
    }

    /// `rule`: whether the dispatch rule (amendment 83) is on. The other
    /// layers' tests run with it off, so each is judged by its own attack.
    fn sealed_outcome_rule(
        tag: &str,
        suite: &str,
        cand: &str,
        test: &str,
        rule: bool,
    ) -> Result<TestEnd, String> {
        use crate::span::intern_source;
        let sdir = format!("/{tag}-sealed");
        let s = crate::parse_source_in(suite, intern_source(&format!("/{tag}-suite/h.ax"), suite))
            .expect("suite parses");
        let c = crate::parse_source_in(cand, intern_source(&format!("{sdir}/f.ax"), cand))
            .expect("candidate parses");
        let prog = Program {
            items: s.items.into_iter().chain(c.items).collect(),
        };
        let _g = SEALED_DIRS_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        crate::resolver::set_sealed_module_dirs(&[std::path::PathBuf::from(&sdir)]);
        // `PSV1T_TAINT_ONLY=1` (a sweep of the whole suite, amendment 102)
        // runs every rule-on test with the STATIC pin analysis off and only the
        // runtime taint rules on: a verdict that changes is a hole in one of the two.
        let taint_only = rule && std::env::var_os("PSV1T_TAINT_ONLY").is_some();
        DISPATCH_RULE_OFF.store(!rule || taint_only, std::sync::atomic::Ordering::SeqCst);
        TAINT_FORCE_ON.store(taint_only, std::sync::atomic::Ordering::SeqCst);
        let out = run_test_fn_outcome(&prog, test);
        DISPATCH_RULE_OFF.store(false, std::sync::atomic::Ordering::SeqCst);
        TAINT_FORCE_ON.store(false, std::sync::atomic::Ordering::SeqCst);
        crate::resolver::set_sealed_module_dirs(&[]);
        out
    }

    /// C9 round 3, PSV-1: a SEALED handler frame never answers or aborts an
    /// operation performed under OPERATOR provenance. The review executed two
    /// variants to a keyed pass: the candidate's `on IO` arm ABORTED the
    /// operator's listener at its first `println` (skipping its assert), and
    /// RESUMED the operator's `read_file` with bytes the candidate chose. A
    /// third route has the same root: the operator's listener calls a
    /// CANDIDATE function whose `println` the candidate's outer handler
    /// aborts, unwinding through the operator's code. A fourth goes through
    /// the multi-shot replay feed instead of a live frame.
    #[test]
    fn a_sealed_handler_never_answers_or_aborts_operator_code() {
        let dir = std::env::temp_dir().join(format!("psv1-handler-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let want = dir.join("expected.txt");
        std::fs::write(&want, "42\n").unwrap();
        let want = want.display().to_string();
        let suite = format!(
            "fn want() -> i64 {{\n    let raw = match read_file(\"{want}\") {{ Ok(s) => s  Err(e) => \"\" }}\n    match parse_int(str_trim(raw)) {{ Ok(n) => n  Err(e) => -1 }}\n}}\n\
             @[test]\nfn t_abort() {{ visit(|v: i64| {{\n    println(\"listener saw {{to_str(v)}}\")\n    assert_eq(v, 42)\n}}) }}\n\
             @[test]\nfn t_resume() {{ visit(|v: i64| {{ assert_eq(v, want()) }}) }}\n\
             @[test]\nfn t_through() {{ visit(|v: i64| {{ assert_eq(noisy(v), 42) }}) }}\n\
             @[test]\nfn t_replay() {{ visit(|v: i64| {{ assert_eq(v, want()) }}) }}\n\
             @[test]\nfn t_own() {{ assert_eq(mine(), 5) }}\n\
             @[test]\nfn t_op() {{\n    let v = with handler {{ on IO(p) => resume(Ok(\"42\")) }} {{\n        match read_file(\"/nonexistent/psv1\") {{ Ok(s) => s  Err(e) => \"no\" }}\n    }}\n    assert(v == \"42\")\n}}\n\
             @[test]\nfn t_op_around() {{\n    with handler {{ on IO(p) => resume(Ok(\"7\")) }} {{\n        visit(|v: i64| {{ assert_eq(v, want()) }})\n    }}\n}}\n"
        );
        let honest = "fn noisy(v: i64) -> i64 { println(\"n\")  v }\n\
                      fn mine() -> i64 {\n    let a = with handler { on IO(p) => resume(Ok(\"5\")) } {\n        match read_file(\"/nonexistent/own\") { Ok(s) => s  Err(e) => \"0\" }\n    }\n    let b = with handler { on IO(p) => 5 } {\n        println(\"x\")\n        1\n    }\n    match parse_int(a) { Ok(n) => if n == b { n } else { 0 }  Err(e) => 0 }\n}\n";
        let good = format!("{honest}fn visit(cb: fn(i64) -> ()) {{ cb(42) }}\n");
        let wrong7 = format!("{honest}fn visit(cb: fn(i64) -> ()) {{ cb(7) }}\n");
        // Controls: the honest candidate passes each shape and a wrong one
        // fails it, so each check below is live; the candidate's OWN handler
        // still handles the candidate's own operations (resume and abort);
        // an operator handler still handles the operator's operation, also
        // when candidate frames lie between it and the operation.
        for t in ["t_abort", "t_resume", "t_through", "t_own", "t_op"] {
            assert_eq!(
                sealed_outcome("psv1h", &suite, &good, t),
                Ok(TestEnd::Completed),
                "control: {t}"
            );
        }
        for t in ["t_abort", "t_resume", "t_through"] {
            assert!(
                sealed_outcome("psv1h", &suite, &wrong7, t).is_err(),
                "control: wrong fails {t}"
            );
        }
        assert_eq!(
            sealed_outcome("psv1h", &suite, &wrong7, "t_op_around").map_err(|_| ()),
            Ok(TestEnd::Completed),
            "control: the operator's own handler answers the operator's read_file"
        );
        let attacks = [
            (
                "t_abort",
                "the candidate's arm aborted the operator's listener at its println",
                "fn visit(cb: fn(i64) -> ()) {\n    let _ = with handler { on IO(p) => 0 } {\n        cb(7)\n        0\n    }\n}\n",
            ),
            (
                "t_resume",
                "the candidate's arm resumed the operator's read_file with its own bytes",
                "fn visit(cb: fn(i64) -> ()) {\n    with handler { on IO(p) => resume(Ok(\"7\")) } {\n        cb(7)\n    }\n}\n",
            ),
            (
                "t_through",
                "the candidate's arm aborted the operator's listener through a candidate call",
                "fn visit(cb: fn(i64) -> ()) {\n    let _ = with handler { on IO(p) => 0 } {\n        cb(7)\n        0\n    }\n}\n",
            ),
            (
                "t_replay",
                "the candidate's multi-shot replay fed the operator's read_file",
                "fn visit(cb: fn(i64) -> ()) {\n    let st = dict_new()\n    let _ = with handler { on IO(p) => {\n        dict_set(st, \"replaying\", 1)\n        resume(Ok(\"7\"))\n    } } {\n        if !dict_has(st, \"replaying\") { println(\"mine\") }\n        cb(7)\n        0\n    }\n}\n",
            ),
        ];
        for (t, what, visit) in attacks {
            let cand = format!("{honest}{visit}");
            let out = sealed_outcome("psv1h", &suite, &cand, t);
            assert!(
                out != Ok(TestEnd::Completed),
                "ATTACK: sealed handler answered operator code: {what} ({t}): {out:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// C9 round 3, PSV-1 (the RNG route of the same root): operator code a
    /// sealed frame runs — here a closure the suite hands the candidate —
    /// cannot draw from the operator's stream, because the CANDIDATE chooses
    /// how many times it runs and so how far the stream advances. The attack:
    /// the candidate is right only at the challenge the operator draws after
    /// FIVE ticks, and ticks five times. Refused, not given another stream:
    /// amendment 46.
    #[test]
    fn operator_code_a_sealed_frame_runs_never_draws_the_operator_rng() {
        // `srand(7)`: the operator's stream is 327, 652, 743, 107, 850, 125 …
        let suite = "@[test]\nfn t_accept() {\n    srand(7)\n    warm(|| { let _ = random_i64(0, 1000) })\n    let c = random_i64(0, 1000)\n    assert(solve(c))\n}\n\
                     @[test]\nfn t_seeded() {\n    srand(7)\n    warm(|| { let _ = random_i64(0, 1000) })\n    assert_eq(random_i64(0, 1000), 327)\n}\n\
                     @[test]\nfn t_reseed() {\n    srand(7)\n    warm(|| { srand(1) })\n    assert(solve(random_i64(0, 1000)))\n}\n";
        let right_at_125 = "fn solve(c: i64) -> bool { c == 125 }\n";
        // Controls: with no tick the operator's own draws work and follow
        // the seed; the candidate draws freely from its OWN stream.
        let idle =
            format!("{right_at_125}fn warm(tick: fn() -> ()) {{ let _ = random_i64(0, 9) }}\n");
        assert_eq!(
            sealed_outcome("psv1r", suite, &idle, "t_seeded"),
            Ok(TestEnd::Completed),
            "control"
        );
        assert!(
            sealed_outcome("psv1r", suite, &idle, "t_accept").is_err(),
            "control: 327 is wrong"
        );
        let steer = format!(
            "{right_at_125}fn warm(tick: fn() -> ()) {{ tick() tick() tick() tick() tick() }}\n"
        );
        let out = sealed_outcome("psv1r", suite, &steer, "t_accept");
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: the candidate steered the operator's challenge by calling its closure five times: {out:?}"
        );
        assert!(
            matches!(&out, Err(m) if m.contains("operator's random stream")),
            "the refusal names the rule: {out:?}"
        );
        let reseed = "fn solve(c: i64) -> bool { c == 0 }\nfn warm(tick: fn() -> ()) { tick() }\n";
        assert!(
            sealed_outcome("psv1r", suite, reseed, "t_reseed").is_err(),
            "operator code a sealed frame runs cannot reseed the operator stream either"
        );
    }

    /// C9 round 3, PSV-3: a test is COMPLETED only when its body evaluated to
    /// its end. The review's route: a `?` on a type-confused `None` returned
    /// `None` from the test, which is not an `Err`, so the returned VALUE read
    /// as a completion and a token was minted though the assert never ran.
    /// The return-boundary check also stops `t_solve` (the test itself is
    /// declared `-> Result` and would return a `None`). `t_find` is the route
    /// where the completion rule is the ONLY guard: an `Option`-returning test
    /// whose `?` meets an honest `None` returns a well-typed `None`.
    #[test]
    fn a_test_ended_by_question_mark_is_never_completed() {
        let suite = "@[test]\nfn t_solve() -> Result<i64, str> {\n    let f = solver()\n    let v = f(21)?\n    assert_eq(v, 42)\n    Ok(v)\n}\n\
                     @[test]\nfn t_find() -> Option<i64> {\n    let v = find(21)?\n    assert_eq(v, 42)\n    Some(v)\n}\n";
        let honest = "fn solver() -> fn(i64) -> Result<i64, str> { |x: i64| Ok(x * 2) }\nfn find(x: i64) -> Option<i64> { Some(x * 2) }\n";
        let err = "fn solver() -> fn(i64) -> Result<i64, str> { |x: i64| Err(\"no\") }\nfn find(x: i64) -> Option<i64> { Some(x) }\n";
        for t in ["t_solve", "t_find"] {
            assert_eq!(
                sealed_outcome("psv3q", suite, honest, t),
                Ok(TestEnd::Completed),
                "control: {t}"
            );
        }
        assert!(
            matches!(
                sealed_outcome("psv3q", suite, err, "t_solve"),
                Ok(TestEnd::EndedEarly(_))
            ),
            "control: an honest Err ends the test early"
        );
        assert!(
            sealed_outcome("psv3q", suite, err, "t_find").is_err(),
            "control: wrong fails"
        );
        let confused = "fn solver() -> fn(i64) -> Result<i64, str> {\n    |x: i64| {\n        let d = dict_new()\n        let n: Option<i64> = None\n        dict_set(d, \"k\", n)\n        match dict_get(d, \"k\") { Some(v) => v  None => Err(\"u\") }\n    }\n}\nfn find(x: i64) -> Option<i64> { None }\n";
        for t in ["t_solve", "t_find"] {
            let out = sealed_outcome("psv3q", suite, confused, t);
            assert!(
                out != Ok(TestEnd::Completed),
                "ATTACK: a test that ended early at `?` was counted complete ({t}): {out:?}"
            );
        }
    }

    /// C9 round 3, PSV-3: a `fn` declared `-> Result` never hands its caller a
    /// `None` (nor an `Option` fn an `Ok`/`Err`). The review's exact
    /// candidate: an untyped `dict_get` gives a free type variable, so a
    /// stored `None` type-checks as the `Result`. Refused at the return
    /// boundary, the test FAILS on the confusion instead of ending early.
    #[test]
    fn a_declared_result_fn_never_returns_an_option() {
        let suite = "@[test]\nfn t_solve() -> Result<i64, str> {\n    let v = solve(21)?\n    assert_eq(v, 42)\n    Ok(v)\n}\n";
        let confused = "fn solve(x: i64) -> Result<i64, str> {\n    let d = dict_new()\n    let n: Option<i64> = None\n    dict_set(d, \"k\", n)\n    match dict_get(d, \"k\") {\n        Some(v) => v\n        None => Err(\"unreachable\")\n    }\n}\n";
        let honest = "fn solve(x: i64) -> Result<i64, str> { Ok(x * 2) }\n";
        assert_eq!(
            sealed_outcome("psv3b", suite, honest, "t_solve"),
            Ok(TestEnd::Completed),
            "control"
        );
        let out = sealed_outcome("psv3b", suite, confused, "t_solve");
        assert!(
            matches!(&out, Err(m) if m.contains("runtime type confusion")),
            "ATTACK: a fn declared -> Result returned a None across its boundary: {out:?}"
        );
        let opt = "@[test]\nfn t_find() -> Option<i64> {\n    let v = find(21)?\n    Some(v)\n}\n";
        let confused_opt = "fn find(x: i64) -> Option<i64> {\n    let d = dict_new()\n    let e: Result<i64, str> = Ok(42)\n    dict_set(d, \"k\", e)\n    match dict_get(d, \"k\") { Some(v) => v  None => None }\n}\n";
        let out = sealed_outcome("psv3b", opt, confused_opt, "t_find");
        assert!(
            matches!(&out, Err(m) if m.contains("runtime type confusion")),
            "an Option fn returned an Ok across its boundary: {out:?}"
        );
    }

    // ── C9 round 4, PSV-1 (amendment 53, matrix A86): the candidate never
    // chooses the code that runs under the operator's judging method. The
    // review's keyed pass: a candidate returned its own `Fake` from a fn
    // declared `-> i64` (a type confusion through `dict_get`, whose value
    // type is free), and the operator's `r.ok()` dispatched on the RUNTIME
    // type to the candidate's `Fake::ok`. Two sources close it: every value
    // is CAST to the declared type at each boundary it crosses, and in
    // operator code an operator-defined method name is the operator's.
    //
    // `ok` on an i64 is the real check (3 * 3 = 9); `ok` on a bool reads the
    // bool — a second, lenient impl the operator wrote for its own reasons.
    // A confused `true` selects it, so each attack below reaches a PASS
    // unless its own guard refuses it (no candidate method is involved, so
    // the dispatch edge cannot stand in for the cast).
    const JUDGE: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for bool {\n    fn ok(self: bool) -> bool { self }\n}\n";
    /// Candidate-side laundering: a `true`, typed as whatever the context
    /// wants.
    const LAUNDER: &str = "fn stash(v: bool) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";
    const CONFUSED: &str = "match dict_get(stash(true), \"k\") { Some(v) => v  None => 0 }";

    fn judged(tag: &str, suite: &str, cand: &str) -> Result<TestEnd, String> {
        sealed_outcome(
            tag,
            &format!("{JUDGE}{suite}"),
            &format!("{LAUNDER}{}", cand.replace("CONFUSED", CONFUSED)),
            "t",
        )
    }

    /// A control pair on one suite: the right candidate completes, the wrong
    /// one fails — so the attack meets a live check.
    fn live(tag: &str, suite: &str, good: &str, wrong: &str) {
        assert_eq!(
            judged(tag, suite, good),
            Ok(TestEnd::Completed),
            "control: the right answer"
        );
        assert!(
            judged(tag, suite, wrong).is_err(),
            "control: the wrong answer fails"
        );
    }

    fn confusion_refused(out: &Result<TestEnd, String>) -> bool {
        matches!(out, Err(m) if m.contains("runtime type confusion"))
    }

    /// The dispatch edge. No type confusion at all: the candidate DECLARES
    /// `-> Fake`, and `Fake` has its own `ok`. The operator's `r.ok()` would
    /// run it. The candidate's own method names (its API) stay callable.
    #[test]
    fn a_candidates_method_never_runs_under_the_operators_method_name() {
        let suite = "@[test]\nfn t() {\n    let r = solve(3)\n    assert(r.ok())\n}\n\
                     @[test]\nfn t_api() {\n    assert_eq(sq(3).twice(), 18)\n}\n";
        let api = "type Sq = { v: i64 }\ntrait Api {\n    fn twice(self) -> i64\n}\nimpl Api for Sq {\n    fn twice(self: Sq) -> i64 { self.v * 2 }\n}\nfn sq(n: i64) -> Sq { Sq { v: n * n } }\n";
        live(
            "psv1d",
            suite,
            &format!("{api}fn solve(n: i64) -> i64 {{ n * n }}\n"),
            &format!("{api}fn solve(n: i64) -> i64 {{ n + 1 }}\n"),
        );
        let fake = format!(
            "{api}type Fake = {{ v: i64 }}\ntrait Mine {{\n    fn ok(self) -> bool\n}}\nimpl Mine for Fake {{\n    fn ok(self: Fake) -> bool {{ true }}\n}}\nfn solve(n: i64) -> Fake {{ Fake {{ v: n }} }}\n"
        );
        let out = judged("psv1d", suite, &fake);
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: the candidate's `ok` ran under the operator's method name: {out:?}"
        );
        assert!(
            matches!(&out, Err(m) if m.contains("a method the operator defines")),
            "the refusal names the rule: {out:?}"
        );
        assert_eq!(
            sealed_outcome(
                "psv1d",
                &format!("{JUDGE}{suite}"),
                &format!("{LAUNDER}{fake}"),
                "t_api"
            ),
            Ok(TestEnd::Completed),
            "control: the suite still calls the candidate's own API"
        );
    }

    /// Scalar kind: a confused `true` from a fn declared `-> i64`.
    #[test]
    fn a_confused_scalar_never_crosses_a_declared_return() {
        let suite = "@[test]\nfn t() {\n    let r = solve(3)\n    assert(r.ok())\n}\n";
        live(
            "psv1s",
            suite,
            "fn solve(n: i64) -> i64 { n * n }\n",
            "fn solve(n: i64) -> i64 { n + 1 }\n",
        );
        let out = judged("psv1s", suite, "fn solve(n: i64) -> i64 { CONFUSED }\n");
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a confused bool crossed a declared `-> i64` return: {out:?}"
        );
    }

    /// Struct name: an `Other` returned from a fn declared `-> Point`.
    #[test]
    fn a_confused_struct_never_crosses_as_another_struct() {
        let suite = "impl Judge for Point {\n    fn ok(self: Point) -> bool { self.x == 9 }\n}\n\
                     impl Judge for Other {\n    fn ok(self: Other) -> bool { true }\n}\n\
                     @[test]\nfn t() {\n    assert(solve(3).ok())\n}\n";
        let types = "type Point = { x: i64 }\ntype Other = { x: i64 }\n";
        live(
            "psv1n",
            suite,
            &format!("{types}fn solve(n: i64) -> Point {{ Point {{ x: n * n }} }}\n"),
            &format!("{types}fn solve(n: i64) -> Point {{ Point {{ x: n + 1 }} }}\n"),
        );
        let attack = format!(
            "{types}fn keep(v: Other) -> Dict {{\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}}\n\
             fn solve(n: i64) -> Point {{\n    match dict_get(keep(Other {{ x: n }}), \"k\") {{ Some(v) => v  None => Point {{ x: 0 }} }}\n}}\n"
        );
        let out = judged("psv1n", suite, &attack);
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: an `Other` crossed a declared `-> Point` return: {out:?}"
        );
    }

    /// Array elements, `Option` payloads, tuple elements.
    #[test]
    fn a_confused_element_never_crosses_inside_an_array() {
        let suite = "@[test]\nfn t() {\n    assert(solve(3)[0].ok())\n}\n";
        live(
            "psv1a",
            suite,
            "fn solve(n: i64) -> [i64] { [n * n] }\n",
            "fn solve(n: i64) -> [i64] { [n + 1] }\n",
        );
        let out = judged("psv1a", suite, "fn solve(n: i64) -> [i64] { [CONFUSED] }\n");
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a confused element crossed a declared `-> [i64]` return: {out:?}"
        );
    }

    #[test]
    fn a_confused_payload_never_crosses_inside_an_option() {
        let suite = "@[test]\nfn t() {\n    match solve(3) {\n        Some(r) => assert(r.ok())\n        None => assert(false)\n    }\n}\n";
        live(
            "psv1o",
            suite,
            "fn solve(n: i64) -> Option<i64> { Some(n * n) }\n",
            "fn solve(n: i64) -> Option<i64> { Some(n + 1) }\n",
        );
        let out = judged(
            "psv1o",
            suite,
            "fn solve(n: i64) -> Option<i64> { Some(CONFUSED) }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a confused payload crossed a declared `-> Option<i64>` return: {out:?}"
        );
    }

    #[test]
    fn a_confused_element_never_crosses_inside_a_tuple() {
        let suite = "@[test]\nfn t() {\n    assert(solve(3).0.ok())\n}\n";
        live(
            "psv1t",
            suite,
            "fn solve(n: i64) -> (i64, i64) { (n * n, 0) }\n",
            "fn solve(n: i64) -> (i64, i64) { (n + 1, 0) }\n",
        );
        let out = judged(
            "psv1t",
            suite,
            "fn solve(n: i64) -> (i64, i64) { (CONFUSED, 0) }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a confused element crossed a declared `-> (i64, i64)` return: {out:?}"
        );
    }

    /// A struct's fields at the return: the field was ASSIGNED after
    /// construction, so only the deep return cast sees it.
    #[test]
    fn a_confused_field_never_crosses_inside_a_struct() {
        let suite = "@[test]\nfn t() {\n    assert(solve(3).v.ok())\n}\n";
        let ty = "type Holder = { v: i64 }\n";
        live(
            "psv1f",
            suite,
            &format!("{ty}fn solve(n: i64) -> Holder {{ Holder {{ v: n * n }} }}\n"),
            &format!("{ty}fn solve(n: i64) -> Holder {{ Holder {{ v: n + 1 }} }}\n"),
        );
        let out = judged(
            "psv1f",
            suite,
            &format!("{ty}fn solve(n: i64) -> Holder {{\n    let h = Holder {{ v: 0 }}\n    h.v = CONFUSED\n    h\n}}\n"),
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a confused field crossed a declared `-> Holder` return: {out:?}"
        );
    }

    /// A field at CONSTRUCTION: a candidate global the suite reads crosses
    /// no fn boundary, so the struct literal is the declared type it meets.
    #[test]
    fn a_confused_field_is_refused_at_construction() {
        let suite = "@[test]\nfn t() {\n    assert(H.v.ok())\n}\n";
        let ty = "type Holder = { v: i64 }\n";
        live(
            "psv1c",
            suite,
            &format!("{ty}let H = Holder {{ v: 9 }}\n"),
            &format!("{ty}let H = Holder {{ v: 4 }}\n"),
        );
        let out = judged(
            "psv1c",
            suite,
            &format!("{ty}let H = Holder {{ v: CONFUSED }}\n"),
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a confused field was constructed into a candidate global: {out:?}"
        );
    }

    /// A declared PARAMETER: an unannotated candidate global handed to the
    /// operator's typed helper.
    #[test]
    fn a_confused_argument_never_enters_a_declared_parameter() {
        let suite =
            "fn judge(x: i64) -> bool { x.ok() }\n@[test]\nfn t() {\n    assert(judge(X))\n}\n";
        live("psv1p", suite, "let X = 9\n", "let X = 4\n");
        let out = judged("psv1p", suite, "let X = CONFUSED\n");
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a confused value entered the operator's `x: i64` parameter: {out:?}"
        );
    }

    /// Parametricity at a seal crossing: `fn solve<T>(n: i64) -> T` has no
    /// argument of type `T`, so no honest body produces a `T`. An honest
    /// generic candidate whose `T` IS determined still crosses.
    #[test]
    fn a_value_at_an_undetermined_type_parameter_never_crosses_the_seal() {
        let suite = "@[test]\nfn t() {\n    let r = solve(3)\n    assert(r.ok())\n}\n\
                     @[test]\nfn t_pick() {\n    assert(pick(9).ok())\n}\n";
        let pick = "fn pick<T>(x: T) -> T { x }\n";
        live(
            "psv1g",
            suite,
            &format!("{pick}fn solve(n: i64) -> i64 {{ n * n }}\n"),
            &format!("{pick}fn solve(n: i64) -> i64 {{ n + 1 }}\n"),
        );
        let attack = format!(
            "{pick}fn solve<T>(n: i64) -> T {{\n    match dict_get(stash(true), \"k\") {{ Some(v) => v  None => solve(n) }}\n}}\n"
        );
        let out = judged("psv1g", suite, &attack);
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a value at an undetermined type parameter crossed into operator code: {out:?}"
        );
        assert_eq!(
            sealed_outcome(
                "psv1g",
                &format!("{JUDGE}{suite}"),
                &format!("{LAUNDER}{attack}"),
                "t_pick"
            ),
            Ok(TestEnd::Completed),
            "control: an honest generic crosses when its argument determines `T`"
        );
    }

    /// A closure's RESULT under the `fn` type it crossed.
    #[test]
    fn a_closures_confused_result_never_crosses_its_declared_type() {
        let suite = "@[test]\nfn t() {\n    let f = mk()\n    assert(f(3).ok())\n}\n";
        live(
            "psv1k",
            suite,
            "fn mk() -> fn(i64) -> i64 { |n| n * n }\n",
            "fn mk() -> fn(i64) -> i64 { |n| n + 1 }\n",
        );
        let out = judged(
            "psv1k",
            suite,
            "fn mk() -> fn(i64) -> i64 { |n| CONFUSED }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a closure declared `fn(i64) -> i64` returned a confused bool: {out:?}"
        );
    }

    /// A closure's ARGUMENTS: the candidate calls the operator's listener
    /// with a confused value under the `fn(i64) -> ()` it declared.
    #[test]
    fn a_closures_confused_argument_never_crosses_its_declared_type() {
        let suite = "@[test]\nfn t() {\n    visit(|r| assert(r.ok()))\n}\n";
        live(
            "psv1l",
            suite,
            "fn visit(cb: fn(i64) -> ()) { cb(9) }\n",
            "fn visit(cb: fn(i64) -> ()) { cb(4) }\n",
        );
        let out = judged(
            "psv1l",
            suite,
            "fn visit(cb: fn(i64) -> ()) { cb(CONFUSED) }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: the operator's listener was called with a confused bool: {out:?}"
        );
    }

    /// The listener's OWN annotation: the candidate declares `fn(T) -> ()`,
    /// so only the operator's `|r: i64|` states the type.
    #[test]
    fn a_lambdas_annotated_parameter_is_cast() {
        let suite = "@[test]\nfn t() {\n    visit(|r: i64| assert(r.ok()))\n}\n";
        live(
            "psv1m",
            suite,
            "fn visit<T>(cb: fn(T) -> ()) { cb(9) }\n",
            "fn visit<T>(cb: fn(T) -> ()) { cb(4) }\n",
        );
        let out = judged(
            "psv1m",
            suite,
            "fn visit<T>(cb: fn(T) -> ()) { cb(CONFUSED) }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: the operator's `|r: i64|` listener was called with a confused bool: {out:?}"
        );
    }

    /// A channel is one invariant object: what the candidate sends on a
    /// `Chan<i64>` is cast.
    #[test]
    fn a_confused_value_is_never_sent_on_a_declared_channel() {
        let suite = "@[test]\nfn t() {\n    let c = chan<i64>()\n    fill(c)\n    assert(c.recv().ok())\n}\n";
        live(
            "psv1q",
            suite,
            "fn fill(c: Chan<i64>) { c.send(9) }\n",
            "fn fill(c: Chan<i64>) { c.send(4) }\n",
        );
        let out = judged(
            "psv1q",
            suite,
            "fn fill(c: Chan<i64>) { c.send(CONFUSED) }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a confused bool was sent on a `Chan<i64>` the operator reads: {out:?}"
        );
    }

    /// A `let` annotation: the suite pins the type of a candidate global.
    #[test]
    fn a_let_annotation_is_cast() {
        let suite = "@[test]\nfn t() {\n    let r: i64 = X\n    assert(r.ok())\n}\n";
        live("psv1b", suite, "let X = 9\n", "let X = 4\n");
        let out = judged("psv1b", suite, "let X = CONFUSED\n");
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a confused bool was bound to the operator's `let r: i64`: {out:?}"
        );
    }

    /// A trait BOUND: `judge<T: Judge>` over a value whose type implements
    /// only another trait with a same-named, lenient method.
    #[test]
    fn a_type_parameters_trait_bound_is_cast() {
        let suite = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\n\
                     trait Lax {\n    fn ok(self) -> bool\n}\nimpl Lax for bool {\n    fn ok(self: bool) -> bool { self }\n}\n\
                     fn judge<T: Judge>(x: T) -> bool { x.ok() }\n@[test]\nfn t() {\n    assert(judge(X))\n}\n";
        let run = |cand: &str| {
            sealed_outcome(
                "psv1r2",
                suite,
                &format!("{LAUNDER}{}", cand.replace("CONFUSED", CONFUSED)),
                "t",
            )
        };
        assert_eq!(
            run("let X = 9\n"),
            Ok(TestEnd::Completed),
            "control: the right answer"
        );
        assert!(
            run("let X = 4\n").is_err(),
            "control: the wrong answer fails"
        );
        let out = run("let X = CONFUSED\n");
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a bool passed the operator's `T: Judge` bound through `Lax`: {out:?}"
        );
    }

    // ── C9 round 4b (amendment 60): the cast's notion of "the same type" is
    // the key a method call dispatches on (`Value::type_name`). ──────────────

    /// The operator's judge: strict at `i64` and `u16`, LENIENT at `u8` (a
    /// second impl the operator wrote for its own reasons).
    const WJUDGE: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u16 {\n    fn ok(self: u16) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";
    /// Candidate-side laundering of a `u8`, typed as whatever the context wants.
    const WLAUNDER: &str = "fn narrow(n: i64) -> u8 { n as u8 }\nfn wstash(v: u8) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";
    const WIDE: &str = "match dict_get(wstash(narrow(4)), \"k\") { Some(v) => v  None => DEFAULT }";

    fn widened(tag: &str, suite: &str, cand: &str, default: &str) -> Result<TestEnd, String> {
        sealed_outcome(
            tag,
            &format!("{WJUDGE}{suite}"),
            &format!(
                "{WLAUNDER}{}",
                cand.replace("WIDE", &WIDE.replace("DEFAULT", default))
            ),
            "t",
        )
    }

    /// The round-4b review's blocker: the cast took every integer width for
    /// one kind, the dispatch keys on the width, so a `u8` at a declared `i64`
    /// ran the operator's lenient `u8` impl — even under the suite's own
    /// `let r: i64` pin. Every boundary the cast applies at.
    #[test]
    fn a_value_of_another_integer_width_never_crosses_a_declared_integer() {
        let holder = "type Holder = { v: i64 }\n";
        let cases: [(&str, &str, String, String, String, &str); 7] = [
            (
                "w4ret",
                "@[test]\nfn t() {\n    let r: i64 = solve(3)\n    assert(r.ok())\n}\n",
                "fn solve(n: i64) -> i64 { n * n }\n".into(),
                "fn solve(n: i64) -> i64 { n + 1 }\n".into(),
                "fn solve(n: i64) -> i64 { WIDE }\n".into(),
                "0",
            ),
            (
                "w4opt",
                "@[test]\nfn t() {\n    match solve(3) {\n        Some(r) => assert(r.ok())\n        None => assert(false)\n    }\n}\n",
                "fn solve(n: i64) -> Option<i64> { Some(n * n) }\n".into(),
                "fn solve(n: i64) -> Option<i64> { Some(n + 1) }\n".into(),
                "fn solve(n: i64) -> Option<i64> { Some(WIDE) }\n".into(),
                "0",
            ),
            (
                "w4clo",
                "@[test]\nfn t() {\n    let f = mk()\n    assert(f(3).ok())\n}\n",
                "fn mk() -> fn(i64) -> i64 { |n: i64| n * n }\n".into(),
                "fn mk() -> fn(i64) -> i64 { |n: i64| n + 1 }\n".into(),
                "fn mk() -> fn(i64) -> i64 { |n: i64| WIDE }\n".into(),
                "n",
            ),
            (
                "w4chan",
                "@[test]\nfn t() {\n    let c = chan<i64>()\n    fill(c)\n    assert(c.recv().ok())\n}\n",
                "fn fill(c: Chan<i64>) { c.send(9) }\n".into(),
                "fn fill(c: Chan<i64>) { c.send(4) }\n".into(),
                "fn fill(c: Chan<i64>) { c.send(WIDE) }\n".into(),
                "0",
            ),
            (
                "w4fld",
                "@[test]\nfn t() {\n    assert(solve(3).v.ok())\n}\n",
                format!("{holder}fn solve(n: i64) -> Holder {{ Holder {{ v: n * n }} }}\n"),
                format!("{holder}fn solve(n: i64) -> Holder {{ Holder {{ v: n + 1 }} }}\n"),
                format!("{holder}fn solve(n: i64) -> Holder {{ Holder {{ v: WIDE }} }}\n"),
                "0",
            ),
            (
                "w4tp",
                "@[test]\nfn t() {\n    assert(pick(3, 9).ok())\n}\n",
                "fn pick<T>(a: T, b: T) -> T { b }\n".into(),
                "fn pick<T>(a: T, b: T) -> T { a }\n".into(),
                "fn pick<T>(a: T, b: T) -> T { WIDE }\n".into(),
                "b",
            ),
            (
                "w4let",
                "@[test]\nfn t() {\n    let r: i64 = X\n    assert(r.ok())\n}\n",
                "let X = 9\n".into(),
                "let X = 4\n".into(),
                "let X = WIDE\n".into(),
                "0",
            ),
        ];
        for (tag, suite, good, wrong, attack, default) in cases {
            assert_eq!(
                widened(tag, suite, &good, default),
                Ok(TestEnd::Completed),
                "control ({tag}): the right answer"
            );
            assert!(
                widened(tag, suite, &wrong, default).is_err(),
                "control ({tag}): the wrong answer fails"
            );
            let out = widened(tag, suite, &attack, default);
            assert!(
                out != Ok(TestEnd::Completed) && confusion_refused(&out),
                "ATTACK: a value of another integer width crossed a declared integer type ({tag}): {out:?}"
            );
        }
    }

    /// Two FIXED widths: a `u8` at a declared `u16` (the strict `u16` impl
    /// against the lenient `u8` one). The `let r: u16` used to re-tag it.
    #[test]
    fn a_value_of_another_fixed_width_never_crosses_a_declared_fixed_width() {
        let suite = "@[test]\nfn t() {\n    let r: u16 = solve(3)\n    assert(r.ok())\n}\n";
        assert_eq!(
            widened(
                "w4u16",
                suite,
                "fn solve(n: i64) -> u16 { (n * n) as u16 }\n",
                "0 as u16"
            ),
            Ok(TestEnd::Completed),
            "control: the right answer"
        );
        assert!(
            widened(
                "w4u16",
                suite,
                "fn solve(n: i64) -> u16 { (n + 1) as u16 }\n",
                "0 as u16"
            )
            .is_err(),
            "control: the wrong answer fails"
        );
        let out = widened(
            "w4u16",
            suite,
            "fn solve(n: i64) -> u16 { WIDE }\n",
            "0 as u16",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a u8 crossed a declared u16: {out:?}"
        );
    }

    /// The other direction: an `i64` at a declared `u8` is CONVERTED to the
    /// width (as a `u8` parameter, `let` or field always converted it), so it
    /// dispatches as the `u8` it is declared — never on the `i64` impl.
    #[test]
    fn an_i64_at_a_declared_fixed_width_takes_the_width() {
        let judge = "trait Judge8 {\n    fn ok8(self) -> bool\n}\nimpl Judge8 for u8 {\n    fn ok8(self: u8) -> bool { self == 9 }\n}\nimpl Judge8 for i64 {\n    fn ok8(self: i64) -> bool { true }\n}\n";
        let suite = format!(
            "{judge}@[test]\nfn t() {{\n    match solve(3) {{\n        Some(r) => assert(r.ok8())\n        None => assert(false)\n    }}\n}}\n"
        );
        let stash = "fn istash(v: i64) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";
        let run = |body: &str| {
            sealed_outcome(
                "w4conv",
                &suite,
                &format!("{stash}fn solve(n: i64) -> Option<u8> {{ {body} }}\n"),
                "t",
            )
        };
        assert_eq!(
            run("Some((n * n) as u8)"),
            Ok(TestEnd::Completed),
            "control: the right answer"
        );
        assert!(
            run("Some((n + 1) as u8)").is_err(),
            "control: the wrong answer fails"
        );
        assert_eq!(
            run("match dict_get(istash(n * n), \"k\") { Some(v) => Some(v)  None => None }"),
            Ok(TestEnd::Completed),
            "control: an i64 at a declared u8 is converted, and judged as the u8 it is"
        );
        let out = run("match dict_get(istash(n + 1), \"k\") { Some(v) => Some(v)  None => None }");
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: an i64 crossed a declared u8 without taking its width: {out:?}"
        );
    }

    /// Width-correct and generic code is unaffected: a `u8` keeps its width
    /// through a type parameter, an `i64` literal at a `u8` field converts.
    #[test]
    fn width_correct_values_cross_unchanged() {
        let judge = "trait W {\n    fn w(self) -> str\n}\nimpl W for u8 {\n    fn w(self: u8) -> str { \"u8\" }\n}\nimpl W for i64 {\n    fn w(self: i64) -> str { \"i64\" }\n}\n";
        let suite = format!(
            "{judge}type P = {{ b: u8 }}\nfn id<T>(x: T) -> T {{ x }}\nfn small() -> u8 {{ 4 as u8 }}\n\
             @[test]\nfn t() {{\n    assert(id(small()).w() == \"u8\")\n    assert(id(5).w() == \"i64\")\n    let p = P {{ b: 7 as u8 }}\n    assert(p.b.w() == \"u8\")\n    let a: Option<u8> = Some(4 as u8)\n    match a {{\n        Some(v) => assert(v.w() == \"u8\")\n        None => assert(false)\n    }}\n}}\n"
        );
        assert_eq!(
            sealed_outcome("w4ok", &suite, "fn unused() -> i64 { 0 }\n", "t"),
            Ok(TestEnd::Completed)
        );
    }

    /// The round-4b review's second blocker, at the cast: a named fn is never
    /// a value (E0306), so a non-closure at a declared `fn(..) -> ..` is a
    /// confusion. Waved through, a confused `9` selected the operator's `i64`
    /// impl for a method called on the fn-typed value.
    #[test]
    fn a_non_closure_never_crosses_a_declared_fn_type() {
        let suite = "@[test]\nfn t() {\n    let sq = make()\n    assert(sq.ok())\n}\n\
                     @[test]\nfn t_call() {\n    let f = make()\n    assert(f(3) == 9)\n}\n";
        let stash = "fn istash(v: i64) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";
        let run = |cand: &str, test: &str| {
            sealed_outcome(
                "f4cast",
                &format!("{JUDGE}{suite}"),
                &format!("{stash}{cand}"),
                test,
            )
        };
        assert_eq!(
            run("fn make() -> fn(i64) -> i64 { |n: i64| n * n }\n", "t_call"),
            Ok(TestEnd::Completed),
            "control: a real closure crosses and is called"
        );
        assert!(
            run("fn make() -> fn(i64) -> i64 { |n: i64| n + 1 }\n", "t_call").is_err(),
            "control: the wrong closure fails"
        );
        let out = run(
            "fn make() -> fn(i64) -> i64 {\n    match dict_get(istash(9), \"k\") { Some(v) => v  None => |n: i64| n }\n}\n",
            "t",
        );
        assert!(
            out != Ok(TestEnd::Completed)
                && confusion_refused(&out)
                && matches!(&out, Err(m) if m.contains("declared `fn(i64) -> i64`")),
            "ATTACK: a non-closure crossed a declared fn type: {out:?}"
        );
    }

    /// The second blocker, at the call: a call through a name bound in the
    /// local environment goes through that value, and a non-closure is not
    /// callable. It used to fall through to a builtin or fn of the same NAME,
    /// so a confused value in the operator's local `square` ran the
    /// operator's own reference `fn square`. This route crosses no declared
    /// fn type (a `Dict`'s values are untyped), so the cast cannot stand in.
    #[test]
    fn a_call_through_a_local_never_resolves_the_name_elsewhere() {
        let suite = "fn square(n: i64) -> i64 { n * n }\n\
                     @[test]\nfn t() {\n    match dict_get(table(), \"sq\") {\n        Some(square) => assert(square(3) == 9 && square(5) == 25)\n        None => assert(false)\n    }\n}\n\
                     @[test]\nfn t_shadow() {\n    let square = |n: i64| n + 100\n    assert(square(1) == 101)\n}\n\
                     @[test]\nfn t_plain() {\n    let sq = 4\n    assert(square(sq) == 16)\n}\n";
        let table = |v: &str| {
            format!("fn table() -> Dict {{\n    let d = dict_new()\n    dict_set(d, \"sq\", {v})\n    d\n}}\n")
        };
        let run = |v: &str, test: &str| sealed_outcome("f4call", suite, &table(v), test);
        assert_eq!(
            run("|n: i64| n * n", "t"),
            Ok(TestEnd::Completed),
            "control: the right closure"
        );
        assert!(
            run("|n: i64| n + 1", "t").is_err(),
            "control: the wrong closure fails"
        );
        assert_eq!(
            run("0", "t_shadow"),
            Ok(TestEnd::Completed),
            "control: a local closure shadowing a fn is what the name calls"
        );
        assert_eq!(
            run("0", "t_plain"),
            Ok(TestEnd::Completed),
            "control: with no local of that name, the fn is called"
        );
        let out = run("0", "t");
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("is not callable")),
            "ATTACK: a call through a local ran the operator's fn of the same name: {out:?}"
        );
    }

    /// Soft wrappers. At a plain declared type the wrapper is replaced by its
    /// inner value, so it dispatches as the declared type and never on an
    /// operator impl for `Uncertain`.
    #[test]
    fn a_soft_wrapper_takes_the_declared_plain_type() {
        let suite = "impl Judge for Uncertain {\n    fn ok(self: Uncertain) -> bool { true }\n}\n\
                     @[test]\nfn t() {\n    assert(solve(3)[0].ok())\n}\n";
        live(
            "s4unwrap",
            suite,
            "fn solve(n: i64) -> [i64] { [n * n] }\n",
            "fn solve(n: i64) -> [i64] { [n + 1] }\n",
        );
        let out = judged(
            "s4unwrap",
            suite,
            "fn solve(n: i64) -> [i64] { [uncertain_new(n + 1, 0.5)] }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: an Uncertain wrapper crossed a declared plain type: {out:?}"
        );
        assert_eq!(
            judged(
                "s4unwrap",
                suite,
                "fn solve(n: i64) -> [i64] { [uncertain_new(n * n, 0.5)] }\n",
            ),
            Ok(TestEnd::Completed),
            "control: the right answer in a wrapper is the right answer"
        );
    }

    /// At a declared soft type: the wrapper of THAT name, its inner value
    /// cast to `T`; or a plain value cast to `T` (soft typing); the bare name
    /// takes only its wrapper.
    #[test]
    fn a_declared_soft_type_casts_its_wrapper_and_its_inner_value() {
        let soft = "impl Judge for Uncertain {\n    fn ok(self: Uncertain) -> bool { self.value == 9 }\n}\n\
                    impl Judge for Temporal {\n    fn ok(self: Temporal) -> bool { true }\n}\n\
                    impl Judge for f64 {\n    fn ok(self: f64) -> bool { true }\n}\n";
        let s_wrap = format!(
            "{soft}@[test]\nfn t() {{\n    let r: Uncertain<i64> = solve(3)\n    assert(r.ok())\n}}\n\
             @[test]\nfn t_inner() {{\n    let r: Uncertain<i64> = solve(3)\n    assert(r.value.ok())\n}}\n"
        );
        let good = "fn solve(n: i64) -> Uncertain<i64> { uncertain_new(n * n, 0.9) }\n";
        let wrong = "fn solve(n: i64) -> Uncertain<i64> { uncertain_new(n + 1, 0.9) }\n";
        let laundered = |v: &str| {
            format!(
                "fn solve(n: i64) -> Uncertain<i64> {{\n    let d = dict_new()\n    dict_set(d, \"k\", {v})\n    match dict_get(d, \"k\") {{ Some(v) => v  None => uncertain_new(0, 0.9) }}\n}}\n"
            )
        };
        for test in ["t", "t_inner"] {
            assert_eq!(
                sealed_outcome(
                    "s4soft",
                    &format!("{JUDGE}{s_wrap}"),
                    &format!("{LAUNDER}{good}"),
                    test
                ),
                Ok(TestEnd::Completed),
                "control ({test}): the right answer"
            );
            assert!(
                sealed_outcome(
                    "s4soft",
                    &format!("{JUDGE}{s_wrap}"),
                    &format!("{LAUNDER}{wrong}"),
                    test
                )
                .is_err(),
                "control ({test}): the wrong answer fails"
            );
        }
        let attempt = |v: &str, test: &str| {
            sealed_outcome(
                "s4soft",
                &format!("{JUDGE}{s_wrap}"),
                &format!("{LAUNDER}{}", laundered(v)),
                test,
            )
        };
        let out = attempt("temporal_new(4, 100, 0.1)", "t");
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a Temporal crossed a declared Uncertain: {out:?}"
        );
        let out = attempt("uncertain_new_f64(4.0, 0.5)", "t_inner");
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: an Uncertain of another inner type crossed a declared Uncertain<i64>: {out:?}"
        );
        let out = attempt("true", "t");
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a plain value of another type crossed a declared Uncertain<i64>: {out:?}"
        );
        // The bare name.
        let s_bare = format!(
            "{soft}@[test]\nfn t() {{\n    let r: Uncertain = solve(3)\n    assert(r.ok())\n}}\n"
        );
        let bare = |body: &str| {
            sealed_outcome(
                "s4bare",
                &format!("{JUDGE}{s_bare}"),
                &format!("{LAUNDER}fn solve(n: i64) -> Uncertain {{ {body} }}\n"),
                "t",
            )
        };
        assert_eq!(
            bare("uncertain_new(n * n, 0.9)"),
            Ok(TestEnd::Completed),
            "control: bare"
        );
        assert!(
            bare("uncertain_new(n + 1, 0.9)").is_err(),
            "control: bare, wrong"
        );
        let out = bare(
            "match dict_get(stash(true), \"k\") { Some(v) => v  None => uncertain_new(0, 0.9) }",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a plain value crossed a declared bare Uncertain: {out:?}"
        );
    }

    /// The round-4b EQUIVALENCE blocker: the ENUM-name check had no row. An
    /// `Other::A` laundered into a declared `-> Grade` ran the operator's
    /// lenient `impl Judge for Other`.
    #[test]
    fn a_confused_enum_never_crosses_as_another_enum() {
        let suite = "impl Judge for Grade {\n    fn ok(self: Grade) -> bool { match self { Grade::A { x } => x == 9  Grade::B { x } => false } }\n}\n\
                     impl Judge for Other {\n    fn ok(self: Other) -> bool { true }\n}\n\
                     @[test]\nfn t() {\n    assert(solve(3).ok())\n}\n";
        let types =
            "type Grade = A { x: i64 } | B { x: i64 }\ntype Other = A { x: i64 } | B { x: i64 }\n";
        live(
            "e4name",
            suite,
            &format!("{types}fn solve(n: i64) -> Grade {{ Grade::A {{ x: n * n }} }}\n"),
            &format!("{types}fn solve(n: i64) -> Grade {{ Grade::A {{ x: n + 1 }} }}\n"),
        );
        let attack = format!(
            "{types}fn keep(v: Other) -> Dict {{\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}}\n\
             fn solve(n: i64) -> Grade {{\n    match dict_get(keep(Other::A {{ x: n }}), \"k\") {{ Some(v) => v  None => Grade::A {{ x: 0 }} }}\n}}\n"
        );
        let out = judged("e4name", suite, &attack);
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: an `Other::A` crossed a declared `-> Grade` return: {out:?}"
        );
    }

    /// Every refusal arm of the cast, each reached ALONE (C9 round 4b
    /// EQUIVALENCE audit, amendment 60). An `i64` laundered to a fn declared
    /// another type crosses into the operator's generic judge, which would
    /// run its lenient `i64` impl on it: each declared type's own arm is the
    /// only check between that value and a keyed pass. Control: the type's
    /// own value crosses (the test `t_cross` completes).
    #[test]
    fn every_declared_type_refuses_a_value_of_another_type() {
        let suite = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { true }\n}\n\
                     fn judge<T: Judge>(x: T) -> bool { x.ok() }\n\
                     @[test]\nfn t() {\n    assert(judge(solve(3)))\n}\n\
                     @[test]\nfn t_cross() {\n    let r = solve(3)\n    assert(true)\n}\n";
        let prelude = "fn istash(v: i64) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n\
                       type Pt = { x: i64 }\ntype Gr = A { x: i64 } | B { y: i64 }\n";
        let cases: [(&str, &str); 14] = [
            ("[i64]", "[0]"),
            ("(i64, i64)", "(0, 0)"),
            ("Chan<i64>", "chan<i64>()"),
            ("Option<i64>", "None"),
            ("Result<i64, str>", "Ok(0)"),
            ("f64", "0.5"),
            ("bool", "false"),
            ("str", "\"x\""),
            ("()", "println(\"\")"),
            ("Decimal", "1.5d"),
            ("Dict", "dict_new()"),
            ("Pt", "Pt { x: 0 }"),
            ("Gr", "Gr::B { y: 0 }"),
            // After `str` and `bool`: under a mutated scalar arm the union
            // admits the value through that member, which is the scalar
            // arm's own attack reached first.
            ("str|bool", "\"x\""),
        ];
        for (decl, own) in cases {
            let run = |v: &str, test: &str| {
                sealed_outcome(
                    "a4every",
                    suite,
                    &format!(
                        "{prelude}fn solve(n: i64) -> {decl} {{\n    match dict_get(istash({v}), \"k\") {{ Some(v) => v  None => {own} }}\n}}\n"
                    ),
                    test,
                )
            };
            // The control: `dict_get` of a missing key is `None`, so the
            // declared type's own value is what crosses.
            let honest = sealed_outcome(
                "a4every",
                suite,
                &format!("{prelude}fn solve(n: i64) -> {decl} {{\n    let d = dict_new()\n    match dict_get(d, \"k\") {{ Some(v) => v  None => {own} }}\n}}\n"),
                "t_cross",
            );
            assert_eq!(
                honest,
                Ok(TestEnd::Completed),
                "control: a `{decl}` crosses `-> {decl}`"
            );
            let out = run("n", "t");
            assert!(
                out != Ok(TestEnd::Completed) && confusion_refused(&out),
                "ATTACK: an i64 crossed a declared `{decl}`: {out:?}"
            );
        }
    }

    /// The arms the generic route does not reach alone: a type parameter's
    /// BINDING, `dyn Trait`, a generic enum's variant fields, a channel's
    /// already-queued values, and a refinement's base type.
    #[test]
    fn the_remaining_cast_arms_refuse_a_value_of_another_type() {
        // A type parameter bound by the arguments: every later value at it
        // must agree (T = i64 here; the candidate returns a `bool`).
        let suite = "@[test]\nfn t() {\n    assert(pick(3, 9).ok())\n}\n";
        live(
            "a4bind",
            suite,
            "fn pick<T>(a: T, b: T) -> T { b }\n",
            "fn pick<T>(a: T, b: T) -> T { a }\n",
        );
        let out = judged(
            "a4bind",
            suite,
            "fn pick<T>(a: T, b: T) -> T { match dict_get(stash(true), \"k\") { Some(v) => v  None => b } }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a bool crossed a type parameter the arguments bound to i64: {out:?}"
        );

        // `dyn Strict`: a bool, which implements only `Lax` (whose method has
        // the same name), never crosses as a `Strict`.
        let suite = "trait Strict {\n    fn st(self) -> bool\n}\nimpl Strict for i64 {\n    fn st(self: i64) -> bool { self == 9 }\n}\n\
                     trait Lax {\n    fn st(self) -> bool\n}\nimpl Lax for bool {\n    fn st(self: bool) -> bool { self }\n}\n\
                     @[test]\nfn t() {\n    assert(solve(3).st())\n}\n";
        let dyn_run = |body: &str| {
            sealed_outcome(
                "a4dyn",
                suite,
                &format!("{LAUNDER}fn solve(n: i64) -> dyn Strict {{ {body} }}\n"),
                "t",
            )
        };
        assert_eq!(
            dyn_run("n * n"),
            Ok(TestEnd::Completed),
            "control: dyn, the right answer"
        );
        assert!(
            dyn_run("n + 1").is_err(),
            "control: dyn, the wrong answer fails"
        );
        let out = dyn_run("match dict_get(stash(true), \"k\") { Some(v) => v  None => n }");
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a bool crossed a declared `dyn Strict`: {out:?}"
        );

        // A generic enum's variant field: at construction `T` is free, so the
        // field is checked only against the declared `Wr<i64>`.
        let suite = "@[test]\nfn t() {\n    match solve(3) {\n        Wr::W { v } => assert(v.ok())\n        Wr::N { v } => assert(false)\n    }\n}\n";
        let wr = "type Wr<T> = W { v: T } | N { v: T }\n";
        live(
            "a4enumf",
            suite,
            &format!("{wr}fn solve(n: i64) -> Wr<i64> {{ Wr::W {{ v: n * n }} }}\n"),
            &format!("{wr}fn solve(n: i64) -> Wr<i64> {{ Wr::W {{ v: n + 1 }} }}\n"),
        );
        let out = judged(
            "a4enumf",
            suite,
            &format!("{wr}fn solve(n: i64) -> Wr<i64> {{ Wr::W {{ v: CONFUSED }} }}\n"),
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a confused variant field crossed a declared `-> Wr<i64>`: {out:?}"
        );

        // A channel's QUEUED values: filled before it crossed `Chan<i64>`. The
        // channel is `Chan::new` (unstamped): a `chan<i64>()` refuses the send
        // itself (amendment 72) and never reaches the queued-value cast.
        let suite = "@[test]\nfn t() {\n    let c = solve(3)\n    assert(c.recv().ok())\n}\n";
        let chan_run = |v: &str| {
            sealed_outcome(
                "a4queue",
                &format!("{JUDGE}{suite}"),
                &format!("{LAUNDER}fn solve(n: i64) -> Chan<i64> {{\n    let c = Chan::new(1)\n    c.send({v})\n    c\n}}\n"),
                "t",
            )
        };
        assert_eq!(
            chan_run("n * n"),
            Ok(TestEnd::Completed),
            "control: queue, the right answer"
        );
        assert!(
            chan_run("n + 1").is_err(),
            "control: queue, the wrong answer fails"
        );
        let out = chan_run(CONFUSED);
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a value queued before the channel crossed `Chan<i64>` was never cast: {out:?}"
        );

        // A non-integer at a declared fixed width (the width arm's last arm).
        let suite = "impl Judge for u8 {\n    fn ok(self: u8) -> bool { self == 9 }\n}\n\
                     @[test]\nfn t() {\n    assert(solve(3).ok())\n}\n";
        live(
            "a4u8",
            suite,
            "fn solve(n: i64) -> u8 { (n * n) as u8 }\n",
            "fn solve(n: i64) -> u8 { (n + 1) as u8 }\n",
        );
        let out = judged(
            "a4u8",
            suite,
            "fn solve(n: i64) -> u8 { match dict_get(stash(true), \"k\") { Some(v) => v  None => 0 as u8 } }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a bool crossed a declared `u8`: {out:?}"
        );

        // A refinement's base type: `Pos = i64 where _ > 0`, and a `u8` that
        // satisfies the predicate would run the operator's lenient `u8` impl.
        let suite = "@[test]\nfn t() {\n    assert(solve(3).ok())\n}\n";
        let pos = "type Pos = i64 where _ > 0\n";
        assert_eq!(
            widened(
                "a4refine",
                suite,
                &format!("{pos}fn solve(n: i64) -> Pos {{ n * n }}\n"),
                "1"
            ),
            Ok(TestEnd::Completed),
            "control: refinement, the right answer"
        );
        assert!(
            widened(
                "a4refine",
                suite,
                &format!("{pos}fn solve(n: i64) -> Pos {{ n + 1 }}\n"),
                "1"
            )
            .is_err(),
            "control: refinement, the wrong answer fails"
        );
        let out = widened(
            "a4refine",
            suite,
            &format!("{pos}fn solve(n: i64) -> Pos {{ WIDE }}\n"),
            "1",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a u8 crossed a declared refinement of i64: {out:?}"
        );
    }

    #[test]
    fn runtime_sealing_holds_without_the_static_check() {
        // PCI candidate-3 review: the STATIC sealing walk missed match guards,
        // method calls, functions named in strings, refinements attaching by
        // name. The runtime edges (call, global read, refinement) must hold on
        // their own, so this builds the interpreter WITHOUT the resolver.
        use crate::span::intern_source;
        let suite = "fn expected(n: i64) -> i64 { n * 2 }\n\
                     trait Answers { fn answer(self) -> i64 }\n\
                     impl Answers for i64 { fn answer(self: i64) -> i64 { self * 2 } }\n\
                     fn build() -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", 42)\n    d\n}\n\
                     let TABLE = build()\n\
                     fn lookup(t: Loot, k: str) -> i64 { dict_get_or(t, k, 0) }\n\
                     fn apply(g: fn(i64) -> i64) -> i64 { g(21) }\n\
                     @[test]\nfn t() { assert_eq(double(21), expected(21)) }\n\
                     @[test]\nfn t_key() { assert_eq(double(21), lookup(TABLE, \"k\")) }\n\
                     @[test]\nfn t_closure() { assert_eq(twice()(21), expected(21)) }\n\
                     @[test]\nfn t_callback() { assert_eq(via(|n: i64| expected(n)), 42) }\n\
                     fn reference(n: i64) -> i64 { n * 7 + 5 }\n\
                     @[test]\nfn t_fiber() {\n    let id = scheduler_spawn(\"reference\", 21)\n    scheduler_run()\n    assert_eq(solve(21), scheduler_result(id))\n}\n\
                     fn check_one(x: i64) { assert_eq(double(x), x * 2) }\n\
                     @[test]\nfn t_fanout() {\n    let a = scheduler_spawn(\"check_one\", 1)\n    let b = scheduler_spawn(\"check_one\", 2)\n    scheduler_run()\n    assert(!scheduler_failed(a) && !scheduler_failed(b))\n}\n\
                     @[test]\nfn t_range() {\n    let r = Range { lo: 1, hi: 2 }\n    assert_eq(double(21), expected(21))\n}\n\
                     @[test]\nfn t_latch() {\n    let d = double(21)\n    if !corrigible_halted() { assert_eq(d, 42) }\n}\n\
                     @[adaptive]\nfn score(n: i64) -> i64 { n * 3 }\n\
                     @[test]\nfn t_adaptive() {\n    let s = score(14)\n    assert_eq(double(21), s)\n}\n";
        let run = |cand: &str, test: &str| {
            let s = crate::parse_source_in(suite, intern_source("/pci-rt-suite/h.ax", suite))
                .expect("suite parses");
            let c = crate::parse_source_in(cand, intern_source("/pci-rt-sealed/f.ax", cand))
                .expect("candidate parses");
            let prog = Program {
                items: s.items.into_iter().chain(c.items).collect(),
            };
            // The sealed set is process-global: hold the lock across
            // set/run/clear so a concurrent sealed test cannot swap it.
            let _g = SEALED_DIRS_TEST_LOCK
                .lock()
                .unwrap_or_else(|p| p.into_inner());
            crate::resolver::set_sealed_module_dirs(&[std::path::PathBuf::from("/pci-rt-sealed")]);
            let out = run_test_fn_outcome(&prog, test);
            crate::resolver::set_sealed_module_dirs(&[]);
            out
        };
        // Honest definitions of everything the suite imports; an attack
        // replaces the ones it names (the interpreter keeps the LAST definition,
        // so a base part is included only when the candidate does not define it).
        let base_parts = [
            (
                "fn twice",
                "fn twice() -> fn(i64) -> i64 { |n: i64| n * 2 }\n",
            ),
            ("fn via", "fn via(g: fn(i64) -> i64) -> i64 { g(21) }\n"),
            ("fn solve", "fn solve(n: i64) -> i64 { n * 7 + 5 }\n"),
            ("type Range", "type Range = { lo: i64, hi: i64 }\n"),
        ];
        let with_base = |cand: &str| -> String {
            let mut out = cand.to_string();
            for (key, def) in base_parts {
                if !cand.contains(key) {
                    out.push_str(def);
                }
            }
            out
        };
        for (why, cand, test) in [
            ("direct call", "fn double(n: i64) -> i64 { expected(n) }\n", "t"),
            ("method on a builtin type", "fn double(n: i64) -> i64 { n.answer() }\n", "t"),
            // A function named in a string (`scheduler_spawn`): see
            // `a_sealed_fiber_never_runs_an_operator_function`.
            ("global read", "fn double(n: i64) -> i64 { dict_get_or(TABLE, \"k\", 0) }\n", "t_key"),
            (
                "match guard",
                "fn double(n: i64) -> i64 {\n    match n {\n        x if dict_get_or(TABLE, \"k\", 0) > 0 => 42\n        _ => 0\n    }\n}\n",
                "t_key",
            ),
            (
                "refinement attaching to an operator annotation",
                "fn poke(d: Dict) -> bool {\n    dict_set(d, \"k\", 0)\n    true\n}\ntype Loot = Dict where poke(_)\nfn double(n: i64) -> i64 { 0 }\n",
                "t_key",
            ),
            (
                "a candidate closure called by the operator",
                "fn double(n: i64) -> i64 { n * 2 }\nfn twice() -> fn(i64) -> i64 { |n: i64| expected(n) }\n",
                "t_closure",
            ),
            (
                "module-level initializer",
                "let STEAL = expected(21)\nfn double(n: i64) -> i64 { n * 2 }\n",
                "t",
            ),
            // PCI candidate-4 review: definition provenance and handle-addressed
            // kernel state.
            (
                "a candidate struct's where, built by the operator",
                "type Range = { lo: i64, hi: i64 } where expected(_.lo) > 0\nfn double(n: i64) -> i64 { n * 2 }\n",
                "t_range",
            ),
            (
                "an initializer reading an operator global",
                "let STEAL = dict_get_or(TABLE, \"k\", 0)\nfn double(n: i64) -> i64 { n * 2 }\n",
                "t",
            ),
            (
                "a candidate handler arm",
                "fn double(n: i64) -> i64 {\n    with handler { on IO(p) => resume(expected(21)) } {\n        println(\"x\")\n    }\n    n * 0 + 42\n}\n",
                "t",
            ),
        ] {
            let cand = with_base(cand);
            // Refused BY THE SEAL — not failing for some unrelated reason.
            let out = run(&cand, test);
            assert!(
                matches!(&out, Err(m) if m.contains("sealed code") || m.contains("is the candidate's")),
                "ATTACK: sealed code reached the operator ({why}): {out:?}"
            );
        }
        // Honest, and the interface: the operator calls the candidate, and a
        // closure the OPERATOR hands the candidate may call operator helpers.
        let honest = with_base("fn double(n: i64) -> i64 { n * 2 }\n");
        for test in [
            "t",
            "t_closure",
            "t_callback",
            "t_fiber",
            "t_fanout",
            "t_range",
            "t_latch",
            "t_adaptive",
        ] {
            assert_eq!(run(&honest, test), Ok(TestEnd::Completed), "{test}");
        }
        // Forged HANDLES: the operator's fiber ids do not exist from a sealed
        // frame (its own kernel), so reading the operator's result or erasing
        // its failure does not work — the test FAILS rather than passing.
        for (why, cand, test) in [
            (
                "reads the operator's fiber result by id",
                "fn double(n: i64) -> i64 { n * 2 }\nfn solve(n: i64) -> i64 { scheduler_result(0) }\n",
                "t_fiber",
            ),
            (
                "restarts the operator's failed fibers",
                "let CALLS = dict_new()\n\
                 fn double(n: i64) -> i64 {\n    let c = dict_get_or(CALLS, \"c\", 0)\n    dict_set(CALLS, \"c\", c + 1)\n    if c == 0 { 999 } else {\n        scheduler_restart(0)\n        scheduler_restart(1)\n        n * 2\n    }\n}\n",
                "t_fanout",
            ),
            // Effect-free state a builtin reads is per provenance too: the
            // kill-switch latch, and the adaptive-score store `agent_*` read.
            (
                "trips the operator's kill-switch latch to skip its assertion",
                "fn double(n: i64) -> i64 {\n    corrigible_halt()\n    0\n}\n",
                "t_latch",
            ),
            (
                "reads the operator's adaptive trace by name",
                "fn double(n: i64) -> i64 { agent_trace_len(\"score\") * 42 }\n",
                "t_adaptive",
            ),
        ] {
            let out = run(&with_base(cand), test);
            assert!(
                out.is_err(),
                "ATTACK: a sealed frame reached the operator's kernel state ({why}): {out:?}"
            );
        }
    }

    #[test]
    fn no_control_transfer_escapes_a_frame() {
        // PCI candidate-1 review: `return` out of a predicate, and a `resume`
        // smuggled out of a handler arm in a closure, both unwound the
        // operator's test as a NORMAL completion (a completion token for a test
        // whose assertion never ran). The frame edge is now an allowlist
        // (`contain_frame`): whatever the transfer, it cannot leave the frame.
        let escapes = [
            ("param refinement", "fn solve(n: i64 where if n > 0 { return 0 } else { true }) -> i64 { n * 0 }\n"),
            ("return refinement", "fn solve(n: i64) -> (i64 where if _ == 0 { return 0 } else { true }) { n * 0 }\n"),
            ("verify", "@[verify(if value == 0 { return 0 } else { true })]\nfn solve(n: i64) -> i64 { n * 0 }\n"),
            (
                "resume closure",
                "fn grab() -> fn() -> str {\n    with handler { on IO(p) => || resume(\"go\") } {\n        println(\"x\")\n        || \"never\"\n    }\n}\n\
                 fn solve(n: i64) -> i64 {\n    let k = grab()\n    let _ = k()\n    n * 0\n}\n",
            ),
        ];
        // Each escape is tried from two operator test shapes, separately — one
        // shape's own failure must not mask the other's escape: a direct call,
        // and a call from inside the operator's own handler arm (where a
        // smuggled `resume` lands).
        let bodies = [
            ("direct", "@[test]\nfn t() { assert_eq(solve(21), 42) }\n"),
            (
                "in the operator's arm",
                "fn check(p: str) -> str {\n    assert_eq(solve(21), 42)\n    p\n}\n\
                 @[test]\nfn t() {\n    with handler { on IO(p) => resume(check(p)) } {\n        println(\"go\")\n    }\n}\n",
            ),
        ];
        // Through an OPERATOR HELPER (C9 round 3, rows): a `return` escaping
        // the candidate's frame lands in the helper, which then returns the
        // CANDIDATE's value as its own. The test body itself runs to its end,
        // so the completion rule (`EndedEarly`) cannot see it: the frame edge
        // is the only guard on this route. Checked first, so a shape the
        // completion rule also refuses cannot mask it.
        for (why, def) in [
            ("param refinement", "fn solve(n: i64 where if n > 0 { return 42 } else { true }) -> i64 { n * 0 }\n"),
            ("return refinement", "fn solve(n: i64) -> (i64 where if _ == 0 { return 42 } else { true }) { n * 0 }\n"),
            ("verify", "@[verify(if value == 0 { return 42 } else { true })]\nfn solve(n: i64) -> i64 { n * 0 }\n"),
        ] {
            let prog = crate::parse_source(&format!(
                "{def}fn check(n: i64) -> i64 {{\n    solve(n)\n}}\n\
                 @[test]\nfn t() {{ assert_eq(check(21), 42) }}\n"
            ))
            .expect("parses");
            let end = run_test_fn_outcome(&prog, "t");
            assert!(
                end.is_err(),
                "ATTACK: a `return` escaped the candidate's {why} into the operator's helper \
                 and the test completed ({why} (helper): {end:?})"
            );
        }
        for (why, def) in escapes {
            for (shape, body) in bodies {
                let prog = crate::parse_source(&format!("{def}{body}")).expect("parses");
                let end = run_test_fn_outcome(&prog, "t");
                assert!(end.is_err(), "{why} ({shape}): {end:?}");
            }
        }
        let struct_src = "type Arg = { n: i64 } where if _.n > 0 { return 0 } else { true }\n\
                          fn make(n: i64) -> i64 {\n    let a = Arg { n: n }\n    a.n * 0\n}\n\
                          @[test]\nfn t() { assert_eq(make(21), 42) }\n";
        let prog = crate::parse_source(struct_src).expect("parses");
        assert!(
            run_test_fn_outcome(&prog, "t").is_err(),
            "struct refinement"
        );

        let prog = crate::parse_source(
            // A handler completion is ADDRESSED: the IO arm (7, no resume)
            // finishes the operator's `with`, not the nearest one — the
            // candidate's Random handler used to swallow it (v was 107).
            "fn c() -> i64 {\n    with handler { on Random(p) => resume(1) } {\n        println(\"x\")\n        5\n    }\n}\n\
             @[test]\nfn t_handler() {\n    let v = with handler { on IO(p) => 7 } {\n        let r = c()\n        r + 100\n    }\n    assert_eq(v, 7)\n}\n\
             @[test]\nfn t_nan() { assert_eq_f64(0.0 / 0.0, 2.0) }\n\
             @[test]\nfn t_inf() { assert_eq_f64(1.0 / 0.0, 1.0 / 0.0) }\n\
             fn honest(n: i64 where n >= 0) -> (i64 where _ >= 0) { n * 2 }\n\
             @[test]\nfn t_honest() { assert_eq(honest(21), 42) }\n",
        )
        .expect("parses");
        let end = |t: &str| run_test_fn_outcome(&prog, t);
        assert_eq!(
            end("t_handler"),
            Ok(TestEnd::Completed),
            "ATTACK: a handler completion was caught by a `with` that did not install it"
        );
        assert!(end("t_nan").is_err(), "NaN passed an f64 assertion");
        assert_eq!(end("t_inf"), Ok(TestEnd::Completed));
        assert_eq!(end("t_honest"), Ok(TestEnd::Completed));

        // A property case that `exit(0)`s did not complete either.
        let prog = crate::parse_source(
            "fn bail(n: i64) -> i64 {\n    exit(0)\n    n\n}\n\
             @[test]\n@[forall]\nfn p(n: i64) { assert_eq(bail(n), 99) }\n",
        )
        .expect("parses");
        assert!(
            matches!(
                proptest::run_property_test(&prog, "p", 5),
                proptest::PropertyOutcome::Failed { .. }
            ),
            "an exit(0) property case passed"
        );
    }

    #[test]
    fn a_test_completes_only_when_its_body_returns_normally() {
        // PCI 11/12/13: the affirmative completion point. `run_test_fn` still
        // reports the early ends as "not failed" (existing `axon test`
        // behaviour); what changes is that they are not `Completed`, so no
        // completion evidence is issued for them.
        let prog = crate::parse_source(
            "fn bail(n: i64) -> i64 {\n    if n > 0 { exit(0) }\n    n\n}\n\
             fn early(n: i64) -> i64 {\n    if n > 0 { return 5 }\n    n\n}\n\
             @[test]\nfn t_done() { assert_eq(early(0), 0) }\n\
             @[test]\nfn t_exit() { assert_eq(bail(1), 99) }\n\
             @[test]\nfn t_err() -> Result<i64, str> { Err(\"no\") }\n\
             @[test]\nfn t_q() -> Result<i64, str> {\n    let n = parse_int(\"x\")?\n    assert_eq(n, 99)\n    Ok(n)\n}\n\
             @[test]\nfn t_ok() -> Result<i64, str> { Ok(1) }\n\
             @[test]\nfn t_return() { assert_eq(early(1), 99) }\n\
             @[test]\nfn t_closure_return() {\n    let g = |n: i64| { if n > 0 { return 5 }  n }\n    assert_eq(g(1), 99)\n}\n",
        )
        .expect("parses");
        let end = |t: &str| run_test_fn_outcome(&prog, t);
        assert_eq!(end("t_done"), Ok(TestEnd::Completed));
        assert_eq!(end("t_ok"), Ok(TestEnd::Completed));
        for t in ["t_exit", "t_err", "t_q"] {
            assert!(
                matches!(end(t), Ok(TestEnd::EndedEarly(_))),
                "{t}: {:?}",
                end(t)
            );
        }
        // PCI 8: a callee's `return` ends the callee, never the test — the
        // assertion after it still runs and fails.
        for t in ["t_return", "t_closure_return"] {
            assert!(end(t).is_err(), "{t}: {:?}", end(t));
        }
    }

    #[test]
    fn an_escaped_break_or_continue_does_not_pass_a_test() {
        let prog = crate::parse_source(
            "fn stop(n: i64) -> i64 {\n    if n > 0 { break }\n    n\n}\n\
             fn skip(n: i64) -> i64 {\n    if n > 0 { continue }\n    n\n}\n\
             @[test]\nfn t_break() { assert_eq(stop(1), 99) }\n\
             @[test]\nfn t_continue() { assert_eq(skip(1), 99) }\n\
             @[test]\nfn t_ok() { assert_eq(stop(0), 0) }\n",
        )
        .expect("parses");
        assert!(run_test_fn(&prog, "t_break").is_err());
        assert!(run_test_fn(&prog, "t_continue").is_err());
        assert!(run_test_fn(&prog, "t_ok").is_ok());
        // Candidate 2's blocker: the escape lands in a loop the TEST owns. It
        // must not end that loop and skip the assertions (while, for, and
        // through a closure).
        let prog = crate::parse_source(
            "fn stop(n: i64) -> i64 {\n    if n > 0 { break }\n    n\n}\n\
             fn skip(n: i64) -> i64 {\n    if n > 0 { continue }\n    n\n}\n\
             @[test]\nfn t_while() {\n    let mut i = 1\n    while i < 4 {\n        assert_eq(stop(i), 99)\n        i = i + 1\n    }\n}\n\
             @[test]\nfn t_for() {\n    for i in 1..4 {\n        assert_eq(skip(i), 99)\n    }\n}\n\
             @[test]\nfn t_closure() {\n    let g = |n: i64| { if n > 0 { break }  n }\n    for i in 1..4 {\n        assert_eq(g(i), 99)\n    }\n}\n\
             @[test]\nfn t_own_loop_ok() {\n    let mut i = 0\n    while true {\n        i = i + 1\n        if i > 2 { break }\n    }\n    assert_eq(i, 3)\n}\n",
        )
        .expect("parses");
        for t in ["t_while", "t_for", "t_closure"] {
            assert!(run_test_fn(&prog, t).is_err(), "{t} passed");
        }
        assert!(
            run_test_fn(&prog, "t_own_loop_ok").is_ok(),
            "a test's own break still works"
        );
    }

    /// M58's OWN property, and only it: a `break`/`continue` raised in a named
    /// FUNCTION BODY does not escape into the caller's loop. The test above also
    /// attacks a closure, which never goes through `call_fn_frame`, so it cannot
    /// separate M58 from `contain_frame` (M59): with M59 removed the closure case
    /// reopens whatever M58 does (C9 four-cell run). Every attack here enters
    /// `call_fn_frame`, whose ONLY caller wraps it in `contain_frame`.
    #[test]
    fn an_escaped_break_or_continue_from_a_function_body_does_not_pass_a_test() {
        let prog = crate::parse_source(
            "fn stop(n: i64) -> i64 {\n    if n > 0 { break }\n    n\n}\n\
             fn skip(n: i64) -> i64 {\n    if n > 0 { continue }\n    n\n}\n\
             @[test]\nfn t_break() { assert_eq(stop(1), 99) }\n\
             @[test]\nfn t_continue() { assert_eq(skip(1), 99) }\n\
             @[test]\nfn t_while() {\n    let mut i = 1\n    while i < 4 {\n        assert_eq(stop(i), 99)\n        i = i + 1\n    }\n}\n\
             @[test]\nfn t_for() {\n    for i in 1..4 {\n        assert_eq(skip(i), 99)\n    }\n}\n\
             @[test]\nfn t_ok() { assert_eq(stop(0), 0) }\n",
        )
        .expect("parses");
        for t in ["t_break", "t_continue", "t_while", "t_for"] {
            assert!(
                run_test_fn(&prog, t).is_err(),
                "ATTACK: `{t}` passed: a break/continue escaped a function body"
            );
        }
        assert!(run_test_fn(&prog, "t_ok").is_ok(), "control: t_ok");
    }
    use super::*;

    fn run(src: &str) -> i32 {
        let program = crate::parse_source(src).expect("parse failed");
        // Unmapped: these tests use `main`'s return as a value channel — the
        // cheapest way to read a computed result out of a program — and the
        // status rule is about what a PROCESS reports, not what a program
        // computed. The rule has its own tests, plus `exit_code_parity.sh`.
        run_program_unmapped(&program)
    }

    /// AUDIT T35 (finding RT-02). The static checker validates `ai_complete`
    /// against the implicit host `api.anthropic.com`, but the runtime resolved
    /// AXON_AI_BASE_URL / ANTHROPIC_BASE_URL at call time — two independent
    /// values. Reproduced end-to-end: a program whose
    /// `@[contained(net: ["api.anthropic.com"])]` passed `axon check` exit 0 sent
    /// its prompt and the real x-api-key to a listener on 127.0.0.1, via a `.env`
    /// one directory ABOVE it. This is the collection half of the pin.
    #[test]
    fn declared_net_hosts_collects_every_contained_grant() {
        let p = crate::parse_source(
            r#"
@[contained(net: ["api.anthropic.com"])]
fn ask() -> i64 { 0 }
fn main() { }
"#,
        )
        .expect("parse failed");
        let hosts = declared_net_hosts(&p);
        assert_eq!(hosts, vec!["api.anthropic.com".to_string()]);
    }

    /// A program that declares no net grant is NOT pinned — it never made a
    /// claim to violate, and pinning it would break legitimate self-hosted
    /// gateway workflows that rely on ANTHROPIC_BASE_URL.
    #[test]
    fn a_program_with_no_net_grant_is_not_pinned() {
        let p = crate::parse_source(r#"fn main() { println("hi") }"#).expect("parse failed");
        assert!(
            declared_net_hosts(&p).is_empty(),
            "an unannotated program must not be constrained by the pin"
        );
    }

    /// The implicit AI host is added so a program that passed the checker can
    /// still reach the endpoint the checker approved it for.
    #[test]
    fn the_implicit_ai_host_is_always_included_once_a_grant_exists() {
        let p = crate::parse_source(
            r#"
@[contained(net: ["gw.trusted.io"])]
fn fetch() -> i64 { 0 }
fn main() { }
"#,
        )
        .expect("parse failed");
        let hosts = declared_net_hosts(&p);
        assert!(hosts.contains(&"gw.trusted.io".to_string()));
        assert!(
            hosts.contains(&crate::capabilities::ai_implicit_host().to_string()),
            "the host the checker validates against must be reachable: {hosts:?}"
        );
    }

    #[test]
    fn hello_runs_and_exits_zero() {
        assert_eq!(run(r#"fn main() { println("hi") }"#), 0);
    }

    #[test]
    fn main_returns_exit_code() {
        // 49, not 7: 7 is GOAL_BUDGET_EXIT_CODE, and a value falling out of
        // `main` may no longer claim a ledger code. That this test had to change
        // is itself the finding — the assertion was using a reserved number as
        // an ordinary return value.
        assert_eq!(run("fn main() -> i64 { 49 }"), 49);
    }

    #[test]
    fn a_returned_answer_may_not_impersonate_a_guard() {
        // The whole point of the ledger is that a supervisor can branch on it.
        // A program whose ANSWER happens to be 6 must not be readable as a
        // refinement violation, or 11 as a replay divergence — otherwise any
        // program's arithmetic can forge the one signal the supervisor trusts.
        for reserved in [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 101] {
            let (code, complaint) = returned_exit_status(reserved);
            assert_eq!(code, 1, "a returned {reserved} must not exit {reserved}");
            assert!(
                complaint.is_some_and(|m| m.contains("RESERVED")),
                "and it must say why it did not"
            );
        }
        // The ordinary ground either side of the ledger is untouched.
        for ok in [0, 1, 16, 49, 125, 200, 255] {
            assert_eq!(returned_exit_status(ok), (ok as i32, None));
        }
    }

    #[test]
    fn a_returned_answer_may_not_truncate_silently() {
        // A status is one byte. `main` returning 3240 was OBSERVED as 168: the
        // number the program produced was not the number anyone saw, and nothing
        // said so. Refusing to exit at all is not an option (the process must
        // exit with something), so it exits 1 and explains itself.
        let (code, complaint) = returned_exit_status(3240);
        assert_eq!(code, 1);
        let msg = complaint.expect("silent truncation is the bug — it must speak");
        assert!(
            msg.contains("168"),
            "the message must name what the caller WOULD have seen, or it does \
             not explain the surprise: {msg}"
        );
        // Negative values are the same hazard wearing another hat.
        assert_eq!(returned_exit_status(-1).0, 1);
        assert_eq!(returned_exit_status(256).0, 1);
        // 126..=255 are the SHELL's convention, not this project's ledger, and
        // ordinary answers land there — they pass through.
        assert_eq!(returned_exit_status(200), (200, None));
        assert_eq!(returned_exit_status(126), (126, None));
    }

    #[test]
    fn a_stated_status_is_honoured_including_ledger_codes() {
        // `exit(n)` is a deliberate claim about the process status, so userland
        // keeps the ledger vocabulary — this is how a deploy gate written in
        // Axon says "policy rejection" with the same code the @[verify] gate
        // uses (BUG_HUNT #26/#34). Taking that away would have broken the
        // one-class-per-rejection design while claiming to protect it.
        for n in [0, 1, 3, 6, 11, 49, 101, 200, 255] {
            assert_eq!(
                stated_exit_status(n),
                (n as i32, None),
                "exit({n}) is a statement of intent and must be honoured as written"
            );
        }
        // What is still refused is a value that is not a status at all.
        let (code, complaint) = stated_exit_status(3240);
        assert_eq!(code, 1);
        assert!(complaint.is_some_and(|m| m.contains("168")));
    }

    #[test]
    fn the_two_rules_differ_only_where_intent_differs() {
        // The load-bearing distinction, stated as an assertion rather than left
        // to prose: on the ledger block the two rules must DISAGREE (stated is
        // honoured, returned is not), and everywhere else they must agree.
        for n in 2..=15 {
            assert_ne!(
                stated_exit_status(n).0,
                returned_exit_status(n).0,
                "the ledger block is exactly where stating a status differs from \
                 producing a value; if these agree, one of the two rules is wrong"
            );
        }
        for n in [0, 1, 16, 49, 125, 3240, -1] {
            assert!(
                stated_exit_status(n).0 == returned_exit_status(n).0,
                "outside the reserved ranges the rules must not diverge (n={n})"
            );
        }
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
        let code = super::run_suspendable_unmapped(&prog, |req| Some(format!("{req}{req}")));
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
        let code = super::run_suspendable_unmapped(&prog, |_req| {
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
        let code = super::run_suspendable_unmapped(&prog, |_req| {
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
        let code = super::run_suspendable_unmapped(&prog, |_| {
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
        let code = super::run_suspendable_unmapped(&prog, |_| Some("ok".to_string()));
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
        let code = super::run_suspendable_unmapped(&prog, |_| {
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
        let code = super::run_suspendable_unmapped(&prog, |_| None); // immediate EOF
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
        let code = super::run_suspendable_values_unmapped(&prog, |req| {
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
        let code = super::run_suspendable_values_unmapped(&prog, |req| {
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
        let code = super::run_suspendable_values_unmapped(&prog, |req| {
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
        let prog = parse("fn main() -> i64 { let c = chan<i64>()  let r = host_await_val(c)  0 }");
        // The host is never reached (the refusal happens at the boundary, worker-side).
        let mut host_calls = 0;
        let code = super::run_suspendable_values_unmapped(&prog, |_req| {
            host_calls += 1;
            Some(SendValue::Int(0))
        });
        assert_eq!(
            code, 101,
            "Chan payload → clean refusal (exit 101), not corruption"
        );
        assert_eq!(host_calls, 0, "the refused payload never reached the host");
    }

    #[test]
    fn r15_slice2_str_form_still_works_through_value_substrate() {
        // Regression: the str-typed host_await still works now that the substrate
        // carries SendValue (str crosses as SendValue::Str). The B1 case, unchanged.
        let prog = parse(r#"fn main() -> i64 { let r = host_await("ab")  str_len(r) }"#);
        let code = super::run_suspendable_unmapped(&prog, |req| Some(format!("{req}{req}")));
        assert_eq!(
            code, 4,
            "str host_await round-trips through the SendValue channel"
        );
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
        let s = Value::Struct {
            name: "S".to_string(),
            fields: sf,
        };
        let v = Value::Array(Rc::new(vec![s, Value::Str(Rc::new("hi".to_string()))]));
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

    // ── K4: run_suspendable_hypercall (Unix-socket / hypercall substrate) ───────

    /// Without a live socket, a program that never calls `host_await` should
    /// complete normally — the socket is only opened on the first suspension, so
    /// a simple `main` that returns 0 is indistinguishable from a bare `run_program`.
    #[test]
    #[cfg(unix)]
    fn run_suspendable_hypercall_no_host_await_exits_zero() {
        let src = r#"fn main() -> i64 { println("ok") 0 }"#;
        let prog = crate::parse_source(src).expect("parse failed");
        // AXON_HOST_SOCKET may or may not be set; the default socket
        // (/tmp/axon-host.sock) very likely doesn't exist in the test
        // environment. Neither matters: the program never calls host_await,
        // so unix_socket_roundtrip is never invoked.
        let code = super::run_suspendable_hypercall(&prog);
        assert_eq!(code, 0);
    }
    // ── C9 round 4c, PSV-1 (amendment 72): an undetermined type position ──
    //
    // At a seal crossing, every position of the type a value is cast to is
    // determined from the operator side, or the crossing is refused. The
    // review's class: a free type parameter, an erased type argument or an
    // absent declaration left a position open, and the candidate chose the
    // runtime type there — and with it the operator's impl. The operator's
    // `u8` impl is the lenient one, so a laundered `u8` is a keyed pass.

    pub(super) const JUDGE8: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";
    pub(super) const LAUNDER8: &str = "fn narrow(n: i64) -> u8 { n as u8 }\nfn stash(v: u8) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";

    /// `match dict_get(stash(narrow(4)), "k") { Some(v) => {some}  None => {none} }`
    fn u8_or(some: &str, none: &str) -> String {
        format!("match dict_get(stash(narrow(4)), \"k\") {{ Some(v) => {some}  None => {none} }}")
    }

    fn judged8(tag: &str, suite: &str, cand: &str) -> Result<TestEnd, String> {
        sealed_outcome(
            tag,
            &format!("{JUDGE8}{suite}"),
            &format!("{LAUNDER8}{cand}"),
            "t",
        )
    }

    fn live8(tag: &str, suite: &str, good: &str, wrong: &str) {
        assert_eq!(
            judged8(tag, suite, good),
            Ok(TestEnd::Completed),
            "control: the right answer"
        );
        assert!(
            judged8(tag, suite, wrong).is_err(),
            "control: the wrong answer fails"
        );
    }

    fn honest8(tag: &str, suite: &str, cand: &str) {
        assert_eq!(
            judged8(tag, suite, cand),
            Ok(TestEnd::Completed),
            "control: an honest program crosses"
        );
    }

    /// B1. The operator's `chan<i64>()` STATES its element type: the channel
    /// is stamped with it at creation, so a candidate's generic `Chan<T>`
    /// binds `T` from it, and a value of another type is never sent. The
    /// second attack reaches the stamp alone: through a dict hop the
    /// candidate re-declares the channel `Chan<u8>` (a determined type of its
    /// OWN choosing), which the send-side rule accepts.
    #[test]
    fn a_channel_carries_the_element_type_its_creation_states() {
        let suite = "@[test]\nfn t() {\n    let c = chan<i64>()\n    fill(c)\n    assert(c.recv().ok())\n}\n";
        live8(
            "r4c-chan",
            suite,
            "fn fill(c: Chan<i64>) { c.send(9) }\n",
            "fn fill(c: Chan<i64>) { c.send(4) }\n",
        );
        honest8(
            "r4c-chan",
            suite,
            "fn fill<T>(c: Chan<T>) {\n    let d: Chan<T> = c\n    put(d)\n}\nfn put(c: Chan<i64>) { c.send(9) }\n",
        );
        let out = judged8(
            "r4c-chan",
            suite,
            &format!(
                "fn fill<T>(c: Chan<T>) {{\n    {}\n}}\n",
                u8_or("c.send(v)", "{}")
            ),
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a u8 was sent on the operator's chan<i64>() through a generic Chan<T>: {out:?}"
        );
        let hop = "fn fill<T>(c: Chan<T>) {\n    let d = dict_new()\n    dict_set(d, \"c\", c)\n    match dict_get(d, \"c\") { Some(x) => put(x)  None => {} }\n}\nfn put(c: Chan<u8>) { c.send(narrow(4)) }\n";
        let out = judged8("r4c-chan", suite, hop);
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: the candidate re-declared the operator's chan<i64>() as Chan<u8> and sent a u8: {out:?}"
        );
    }

    /// B1, the channel the operator created WITHOUT a closed element type
    /// (`Chan::new`, or `chan<T>()` in its own generic code): a sealed send
    /// on it needs an element type something on the operator side
    /// determined. A free `T` of the candidate's never counts.
    #[test]
    fn a_sealed_send_on_an_operator_channel_needs_a_determined_element_type() {
        let suite = "@[test]\nfn t() {\n    assert(run(9))\n}\nfn run<T: Judge>(x: T) -> bool {\n    let c = chan<T>()\n    c.send(x)\n    fill(c)\n    c.recv().ok()\n}\n";
        // Control: an honest generic relay — `U` is bound from the value the
        // operator queued, so the send meets a determined type.
        honest8(
            "r4c-opchan",
            suite,
            "fn fill<U>(c: Chan<U>) {\n    let v = c.recv()\n    c.send(v)\n}\n",
        );
        // Control: the candidate's OWN unstamped channel is its business.
        honest8(
            "r4c-opchan",
            "@[test]\nfn t() {\n    assert(mine())\n}\n",
            "fn mine() -> bool {\n    let c = Chan::new(2)\n    c.send(9)\n    c.recv() == 9\n}\n",
        );
        // The attack: nothing queued, so nothing binds the candidate's `U`.
        let empty = "@[test]\nfn t() {\n    assert(run(9))\n}\nfn run<T: Judge>(x: T) -> bool {\n    let c = chan<T>()\n    fill(c)\n    c.recv().ok()\n}\n";
        let cand = format!(
            "fn fill<U>(c: Chan<U>) {{\n    {}\n}}\n",
            u8_or("c.send(v)", "{}")
        );
        let out = judged8("r4c-opchan", empty, &cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("nothing on the operator side determined")),
            "ATTACK: a u8 was sent on the operator's unstamped chan<T>() at the candidate's free U: {out:?}"
        );
        let suite_new = "@[test]\nfn t() {\n    let c = Chan::new(4)\n    fill(c)\n    assert(c.recv().ok())\n}\n";
        let out = judged8(
            "r4c-opchan",
            suite_new,
            &format!(
                "fn fill<U>(c: Chan<U>) {{\n    {}\n}}\n",
                u8_or("c.send(v)", "{}")
            ),
        );
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("nothing on the operator side determined")),
            "ATTACK: a u8 was sent on the operator's Chan::new(4) at the candidate's free U: {out:?}"
        );
    }

    /// B1, the return direction: a candidate returning `Chan<T>` at a `T`
    /// nothing determined is refused — even with values queued, which the
    /// candidate chose.
    #[test]
    fn a_channel_returned_at_an_undetermined_element_type_is_refused() {
        let suite = "@[test]\nfn t() {\n    let c = mk(9)\n    assert(c.recv().ok())\n}\n";
        honest8(
            "r4c-chanret",
            suite,
            "fn mk<T>(x: T) -> Chan<T> {\n    let c = Chan::new(1)\n    c.send(x)\n    c\n}\n",
        );
        let out = judged8(
            "r4c-chanret",
            suite,
            "fn mk<T>(n: i64) -> Chan<T> {\n    let c = chan<u8>()\n    c.send(narrow(4))\n    c\n}\n",
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: the candidate returned its own Chan<u8> at a Chan<T> nothing determined: {out:?}"
        );
    }

    /// B3. A generic struct's or enum's type argument is never erased: the
    /// caller's `T` stays `T`, so the operator's argument binds it and the
    /// candidate's field of another type is refused at the return.
    #[test]
    fn a_generic_struct_or_enum_argument_binds_its_type_parameter() {
        let wrap = "type Wrap<T> = { v: T }\n";
        let suite =
            "@[test]\nfn t() {\n    let w = solve(Wrap { v: 3 })\n    assert(w.v.ok())\n}\n";
        live8(
            "r4c-wrap",
            suite,
            &format!("{wrap}fn solve(w: Wrap<i64>) -> Wrap<i64> {{ Wrap {{ v: w.v * w.v }} }}\n"),
            &format!("{wrap}fn solve(w: Wrap<i64>) -> Wrap<i64> {{ Wrap {{ v: w.v + 1 }} }}\n"),
        );
        honest8(
            "r4c-wrap",
            "@[test]\nfn t() {\n    let w = solve(Wrap { v: Wrap { v: 9 } })\n    assert(w.v.v.ok())\n}\n",
            &format!("{wrap}fn solve<T>(w: Wrap<Wrap<T>>) -> Wrap<Wrap<T>> {{ Wrap {{ v: Wrap {{ v: w.v.v }} }} }}\n"),
        );
        let cases = [
            (
                "a Wrap<T> field (the review's candidate)",
                suite.to_string(),
                format!("{wrap}fn solve<T>(w: Wrap<T>) -> Wrap<T> {{\n    {}\n}}\n", u8_or("Wrap { v: v }", "w")),
            ),
            (
                "a nested Wrap<Wrap<T>> field",
                "@[test]\nfn t() {\n    let w = solve(Wrap { v: Wrap { v: 3 } })\n    assert(w.v.v.ok())\n}\n".to_string(),
                format!(
                    "{wrap}fn solve<T>(w: Wrap<Wrap<T>>) -> Wrap<Wrap<T>> {{\n    {}\n}}\n",
                    u8_or("Wrap { v: Wrap { v: v } }", "w")
                ),
            ),
            (
                "a generic enum variant's field",
                "@[test]\nfn t() {\n    match solve(Opt::Has { v: 3 }) {\n        Opt::Has { v } => assert(v.ok())\n        Opt::Nada => assert(false)\n    }\n}\n".to_string(),
                format!(
                    "type Opt<T> = Has {{ v: T }} | Nada\nfn solve<T>(o: Opt<T>) -> Opt<T> {{\n    {}\n}}\n",
                    u8_or("Opt::Has { v: v }", "o")
                ),
            ),
        ];
        for (why, suite, cand) in cases {
            let out = judged8("r4c-wrap", &suite, &cand);
            assert!(
                out != Ok(TestEnd::Completed) && confusion_refused(&out),
                "ATTACK: a u8 crossed a generic type argument the operator fixed to i64 ({why}): {out:?}"
            );
        }
    }

    /// A position NOTHING determined — an empty array's element type bound
    /// through `T`, or a generic struct handed through a plain `T` — is
    /// refused at the crossing; a later operator-side value fills it (the
    /// pair binding), and an honest pass-through crosses.
    #[test]
    fn a_value_at_an_undetermined_position_never_crosses_a_seal() {
        let wrap = "type Wrap<T> = { v: T }\n";
        honest8(
            "r4c-undet",
            "@[test]\nfn t() {\n    let e: [i64] = []\n    let xs = solve(e)\n    assert(len(xs) == 0)\n}\n",
            "fn solve<T>(x: T) -> T { x }\n",
        );
        honest8(
            "r4c-undet",
            "@[test]\nfn t() {\n    let e: Option<i64> = None\n    match pick(e, Some(9)) {\n        Some(v) => assert(v.ok())\n        None => assert(false)\n    }\n}\n",
            "fn pick<T>(a: T, b: T) -> T { b }\n",
        );
        honest8(
            "r4c-undet",
            "@[test]\nfn t() {\n    let w = solve(Wrap { v: 9 })\n    assert(w.v.ok())\n}\n",
            &format!("{wrap}fn solve<T>(x: T) -> T {{ x }}\n"),
        );
        let out = judged8(
            "r4c-undet",
            "@[test]\nfn t() {\n    let e: [i64] = []\n    let xs = solve(e)\n    assert(xs[0].ok())\n}\n",
            &format!("fn solve<T>(x: T) -> T {{\n    {}\n}}\n", u8_or("[v]", "x")),
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a u8 filled the element type of the operator's empty [i64] through T: {out:?}"
        );
        let out = judged8(
            "r4c-undet",
            "@[test]\nfn t() {\n    let w = solve(Wrap { v: 3 })\n    assert(w.v.ok())\n}\n",
            &format!(
                "{wrap}fn solve<T>(x: T) -> T {{\n    {}\n}}\n",
                u8_or("Wrap { v: v }", "x")
            ),
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a Wrap of u8 crossed a T the operator bound to Wrap<i64>: {out:?}"
        );
    }

    /// B2. A fn with NO declared return type: the checker types its call
    /// `()`, so at the seal crossing the operator receives `()` — never the
    /// value the body ended on, which the operator's method call would
    /// dispatch on. (The checker now also refuses `.ok()` on `()` with no
    /// impl; this test reaches the runtime rule without the checker.)
    #[test]
    fn a_fn_with_no_declared_return_type_hands_the_operator_unit() {
        let suite = "@[test]\nfn t() {\n    assert(solve(3).ok())\n}\n";
        live8(
            "r4c-noret",
            suite,
            "fn solve(n: i64) -> i64 { n * n }\n",
            "fn solve(n: i64) -> i64 { n + 1 }\n",
        );
        // Control: an honest unit fn whose body ends on a value still runs.
        honest8(
            "r4c-noret",
            "@[test]\nfn t() {\n    note(3)\n    assert(true)\n}\n",
            "fn note(n: i64) {\n    let d = dict_new()\n    dict_set(d, \"n\", n)\n    d\n}\n",
        );
        for (why, suite, cand) in [
            ("a fn", suite.to_string(), format!("fn solve(n: i64) {{\n    {}\n}}\n", u8_or("v", "0"))),
            (
                "an impl method",
                "@[test]\nfn t() {\n    assert(mk(3).val().ok())\n}\n".to_string(),
                format!(
                    "type Sq = {{ n: i64 }}\ntrait Api {{\n    fn val(self)\n}}\nimpl Api for Sq {{\n    fn val(self: Sq) {{\n        {}\n    }}\n}}\nfn mk(n: i64) -> Sq {{ Sq {{ n: n }} }}\n",
                    u8_or("v", "0")
                ),
            ),
        ] {
            let out = judged8("r4c-noret", &suite, &cand);
            assert!(
                out != Ok(TestEnd::Completed)
                    && matches!(&out, Err(m) if m.contains("no method `ok` on type `()`")),
                "ATTACK: the u8 a unit {why} ended on reached the operator's method call: {out:?}"
            );
        }
    }

    /// MAJOR-ADJACENT. A sealed frame calling an OPERATOR closure: each
    /// argument must meet a position some declared type determined, and is
    /// cast strictly — the return direction's parametricity rule, applied to
    /// arguments. The operator's unannotated `|x| x.ok()` reached through a
    /// free `fn(T)`, a struct field or a dict.
    #[test]
    fn an_operator_closure_called_from_sealed_code_takes_only_determined_arguments() {
        let suite = "@[test]\nfn t() {\n    assert(apply(|x| x.ok()))\n}\n";
        live8(
            "r4c-clos",
            suite,
            "fn apply(f: fn(i64) -> bool) -> bool { f(9) }\n",
            "fn apply(f: fn(i64) -> bool) -> bool { f(4) }\n",
        );
        honest8(
            "r4c-clos",
            "@[test]\nfn t() {\n    assert(apply(Fx { f: |x| x.ok() }, 9))\n}\n",
            "type Fx<T> = { f: fn(T) -> bool }\nfn apply<T>(b: Fx<T>, x: T) -> bool {\n    let f = b.f\n    f(x)\n}\n",
        );
        honest8(
            "r4c-clos",
            "@[test]\nfn t() {\n    visit(|r: i64| assert(r.ok()))\n}\n",
            "fn visit<T>(cb: fn(T) -> ()) { cb(9) }\n",
        );
        for (why, suite, cand) in [
            (
                "a free fn(T) (the review's candidate)",
                suite.to_string(),
                format!("fn apply<T>(f: fn(T) -> bool) -> bool {{\n    {}\n}}\n", u8_or("f(v)", "false")),
            ),
            (
                "a generic struct's fn field",
                "@[test]\nfn t() {\n    assert(apply(Fx { f: |x| x.ok() }))\n}\n".to_string(),
                format!(
                    "type Fx<T> = {{ f: fn(T) -> bool }}\nfn apply<T>(b: Fx<T>) -> bool {{\n    let f = b.f\n    {}\n}}\n",
                    u8_or("f(v)", "false")
                ),
            ),
            (
                "a dict entry",
                "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"f\", |x| x.ok())\n    assert(apply(d))\n}\n".to_string(),
                format!(
                    "fn apply(d: Dict) -> bool {{\n    match dict_get(d, \"f\") {{\n        Some(f) => {}\n        None => false\n    }}\n}}\n",
                    u8_or("f(v)", "false")
                ),
            ),
        ] {
            let out = judged8("r4c-clos", &suite, &cand);
            assert!(
                out != Ok(TestEnd::Completed)
                    && matches!(&out, Err(m) if m.contains("a position nothing on the operator side determined")),
                "ATTACK: the candidate called the operator's unannotated closure with a u8 ({why}): {out:?}"
            );
        }
    }

    /// A native handle bound to a type parameter: only a handle of that kind
    /// crosses back at it.
    #[test]
    fn a_handle_binding_admits_only_that_handle() {
        let suite = "@[test]\nfn t() {\n    let w = gfx::window_open(8, 8, \"x\")\n    let r = pass(w)\n    assert(r.ok())\n}\n";
        let ok = judged8("r4c-handle", "@[test]\nfn t() {\n    let w = gfx::window_open(8, 8, \"x\")\n    let r = pass(w)\n    assert(true)\n}\n", "fn pass<T>(x: T) -> T { x }\n");
        assert_eq!(
            ok,
            Ok(TestEnd::Completed),
            "control: the handle crosses back"
        );
        let out = judged8(
            "r4c-handle",
            suite,
            &format!("fn pass<T>(x: T) -> T {{\n    {}\n}}\n", u8_or("v", "x")),
        );
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a u8 crossed a T the operator bound to a native handle: {out:?}"
        );
    }

    /// The shapes a type parameter can sit inside — tuple, array, `Option`,
    /// `Result`, generic struct in each, two parameters, a returned closure —
    /// each with the operator's i64 binding `T` and the candidate answering
    /// with a laundered `u8`. All were refused before amendment 72 except the
    /// generic-struct ones; pinned together so none regresses.
    #[test]
    fn a_type_parameter_inside_any_shape_is_never_filled_by_the_candidate() {
        let wrap = "type Wrap<T> = { v: T }\n";
        let cases: Vec<(&str, &str, String)> = vec![
            ("tuple", "let r = solve((3, 4))\n    assert(r.0.ok())",
             format!("fn solve<T>(p: (T, T)) -> (T, T) {{\n    {}\n}}\n", u8_or("(v, v)", "p"))),
            ("array", "let r = solve([3])\n    assert(r[0].ok())",
             format!("fn solve<T>(p: [T]) -> [T] {{\n    {}\n}}\n", u8_or("[v]", "p"))),
            ("Option", "match solve(Some(3)) {\n        Some(v) => assert(v.ok())\n        None => assert(false)\n    }",
             format!("fn solve<T>(p: Option<T>) -> Option<T> {{\n    {}\n}}\n", u8_or("Some(v)", "p"))),
            ("Result", "match solve(Ok(3)) {\n        Ok(v) => assert(v.ok())\n        Err(e) => assert(false)\n    }",
             format!("fn solve<T>(p: Result<T, str>) -> Result<T, str> {{\n    {}\n}}\n", u8_or("Ok(v)", "p"))),
            ("Option<Wrap<T>>", "match solve(Some(Wrap { v: 3 })) {\n        Some(w) => assert(w.v.ok())\n        None => assert(false)\n    }",
             format!("{wrap}fn solve<T>(o: Option<Wrap<T>>) -> Option<Wrap<T>> {{\n    {}\n}}\n", u8_or("Some(Wrap { v: v })", "o"))),
            ("[Wrap<T>]", "let r = solve([Wrap { v: 3 }])\n    assert(r[0].v.ok())",
             format!("{wrap}fn solve<T>(o: [Wrap<T>]) -> [Wrap<T>] {{\n    {}\n}}\n", u8_or("[Wrap { v: v }]", "o"))),
            ("(Wrap<T>, i64)", "let r = solve((Wrap { v: 3 }, 1))\n    assert(r.0.v.ok())",
             format!("{wrap}fn solve<T>(p: (Wrap<T>, i64)) -> (Wrap<T>, i64) {{\n    {}\n}}\n", u8_or("(Wrap { v: v }, 1)", "p"))),
            ("Wrap<T> -> T", "let r = solve(Wrap { v: 3 })\n    assert(r.ok())",
             format!("{wrap}fn solve<T>(w: Wrap<T>) -> T {{\n    {}\n}}\n", u8_or("v", "w.v"))),
            ("two parameters", "let w = solve(P { a: 3, b: 3 })\n    assert(w.a.ok())",
             format!("type P<A, B> = {{ a: A, b: B }}\nfn solve<A, B>(w: P<A, B>) -> P<A, B> {{\n    {}\n}}\n", u8_or("P { a: v, b: w.b }", "w"))),
            ("a returned closure", "let f = solve(|x| x)\n    assert(f(3).ok())",
             format!("fn solve<T>(f: fn(T) -> T) -> fn(T) -> T {{\n    {}\n}}\n", u8_or("|y| v", "f"))),
            ("a generic impl's method", "let b = mk(3)\n    assert(b.get().ok())",
             format!("{wrap}fn mk(n: i64) -> Wrap<i64> {{ Wrap {{ v: n }} }}\ntrait Get {{\n    fn get(self) -> i64\n}}\nimpl Get for Wrap<i64> {{\n    fn get(self: Wrap<i64>) -> i64 {{\n        {}\n    }}\n}}\n", u8_or("v", "self.v"))),
        ];
        for (why, body, cand) in cases {
            let suite = format!("@[test]\nfn t() {{\n    {body}\n}}\n");
            let out = judged8("r4c-shapes", &suite, &cand);
            assert!(
                out != Ok(TestEnd::Completed) && confusion_refused(&out),
                "ATTACK: a u8 filled the operator's i64 inside {why}: {out:?}"
            );
        }
    }

    /// `host_await_val` is unavailable to a run with no host driver (every
    /// sealed `axon test`), so no payload crosses a suspend inside a seal.
    #[test]
    fn a_host_await_crossing_is_unavailable_inside_a_sealed_test_run() {
        let out = judged8(
            "r4c-await",
            "@[test]\nfn t() {\n    assert(solve(3).ok())\n}\n",
            "fn solve<T>(x: T) -> T {\n    host_await_val(x)\n}\n",
        );
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("outside a suspendable run")),
            "ATTACK: a host_await_val payload came back inside a sealed test run: {out:?}"
        );
    }

    /// The suite for the dict tests: the operator's dict holds an i64 at
    /// "a"; `body` hands it to the candidate; the operator then reads "a"
    /// UNTYPED and calls the judge on it (amendment 72 part 2).
    fn dict_suite(body: &str) -> String {
        format!("@[test]\nfn t() {{\n    let d = dict_new()\n    dict_set(d, \"a\", 3)\n    {body}\n    match dict_get(d, \"a\") {{\n        Some(x) => assert(x.ok())\n        None => assert(false)\n    }}\n}}\n")
    }

    fn dict_refused(why: &str, suite: &str, cand: &str) {
        let out = judged8("r4c-dict", suite, cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("retyped a dict entry")),
            "ATTACK: sealed code retyped the operator's dict entry to a u8 ({why}): {out:?}"
        );
    }

    /// `match <laundered u8> { Some(v) => { act } None => {} }`
    fn put_u8(act: &str) -> String {
        u8_or(&format!("{{ {act} }}"), "{}")
    }

    /// The reviewer's remaining attack: the candidate overwrites a key the
    /// operator held with a laundered `u8`, and the operator's untyped
    /// `dict_get(..).ok()` ran the lenient `u8` impl. The operator's dict is
    /// SNAPSHOTTED when it is handed over and verified at every edge back.
    #[test]
    fn sealed_code_cannot_retype_a_dict_entry_the_operator_held() {
        let suite = dict_suite("solve(d)");
        live8(
            "r4c-dict",
            &suite,
            "fn solve(d: Dict) { dict_set(d, \"a\", 9) }\n",
            "fn solve(d: Dict) { dict_set(d, \"a\", 4) }\n",
        );
        dict_refused(
            "the review's overwrite",
            &suite,
            &format!(
                "fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8("dict_set(d, \"a\", v)")
            ),
        );
        dict_refused(
            "remove, then add the key back",
            &suite,
            &format!(
                "fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8("dict_remove(d, \"a\")\n        dict_set(d, \"a\", v)")
            ),
        );
        dict_refused(
            "through an alias in an array",
            &suite,
            &format!(
                "fn solve(d: Dict) {{\n    let held = [d]\n    {}\n}}\n",
                put_u8("dict_set(held[0], \"a\", v)")
            ),
        );
        // A dict nested in the one handed over.
        dict_refused(
            "a dict nested in the handed dict",
            "@[test]\nfn t() {\n    let d = dict_new()\n    let inner = dict_new()\n    dict_set(inner, \"a\", 3)\n    dict_set(d, \"in\", inner)\n    solve(d)\n    match dict_get(inner, \"a\") {\n        Some(x) => assert(x.ok())\n        None => assert(false)\n    }\n}\n",
            &format!(
                "fn solve(d: Dict) {{\n    match dict_get(d, \"in\") {{\n        Some(i) => {}\n        None => {{}}\n    }}\n}}\n",
                put_u8("dict_set(i, \"a\", v)")
            ),
        );
        // Dict reached through generic positions.
        let wrap = "type Wrap<T> = { v: T }\n";
        dict_refused(
            "Wrap<Dict>",
            &dict_suite("solve(Wrap { v: d })"),
            &format!(
                "{wrap}fn solve(w: Wrap<Dict>) {{\n    {}\n}}\n",
                put_u8("dict_set(w.v, \"a\", v)")
            ),
        );
        dict_refused(
            "Option<Dict>",
            &dict_suite("solve(Some(d))"),
            &format!(
                "fn solve(o: Option<Dict>) {{\n    match o {{\n        Some(x) => {}\n        None => {{}}\n    }}\n}}\n",
                put_u8("dict_set(x, \"a\", v)")
            ),
        );
        dict_refused(
            "[Dict]",
            &dict_suite("solve([d])"),
            &format!(
                "fn solve(xs: [Dict]) {{\n    {}\n}}\n",
                put_u8("dict_set(xs[0], \"a\", v)")
            ),
        );
        dict_refused(
            "a generic fn's T bound to the dict",
            &dict_suite("solve(d)"),
            &format!(
                "fn solve<T>(x: T) {{\n    {}\n}}\nfn put(d: Dict, v: u8) {{ dict_set(d, \"a\", v) }}\n",
                u8_or("{ put(x, v) }", "{}")
            ),
        );
    }

    /// The other edges: a dict handed to an operator CLOSURE the candidate
    /// calls (the operator's `|x| …` reads it untyped), over a channel, and
    /// through the candidate's own closure the operator calls.
    #[test]
    fn a_dict_the_candidate_mutated_is_verified_at_every_edge_back() {
        let closure_suite = "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"a\", 3)\n    assert(run(d, |x| {\n        match dict_get(x, \"a\") {\n            Some(y) => y.ok()\n            None => false\n        }\n    }))\n}\n";
        honest8(
            "r4c-dict",
            closure_suite,
            "fn run(d: Dict, f: fn(Dict) -> bool) -> bool {\n    dict_set(d, \"a\", 9)\n    f(d)\n}\n",
        );
        let out = judged8(
            "r4c-dict",
            closure_suite,
            &format!(
                "fn run(d: Dict, f: fn(Dict) -> bool) -> bool {{\n    {}\n    f(d)\n}}\n",
                put_u8("dict_set(d, \"a\", v)")
            ),
        );
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("retyped a dict entry")),
            "ATTACK: the operator's closure read a dict entry the candidate retyped: {out:?}"
        );
        // Over a channel the operator filled.
        let chan_suite = dict_suite("let c = chan<Dict>()\n    c.send(d)\n    solve(c)");
        honest8(
            "r4c-dict",
            &chan_suite,
            "fn solve(c: Chan<Dict>) {\n    let d = c.recv()\n    dict_set(d, \"a\", 9)\n}\n",
        );
        dict_refused(
            "received from the operator's channel",
            &chan_suite,
            &format!(
                "fn solve(c: Chan<Dict>) {{\n    let d = c.recv()\n    {}\n}}\n",
                put_u8("dict_set(d, \"a\", v)")
            ),
        );
        // A closure replaced by a non-closure: refused where it is stored, not
        // only when the operator later calls it.
        let fnonc = "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"f\", |n: i64| n * n)\n    solve(d)\n    match dict_get(d, \"f\") {\n        Some(f) => assert(true)\n        None => assert(false)\n    }\n}\n";
        honest8(
            "r4c-dict",
            fnonc,
            "fn solve(d: Dict) { dict_set(d, \"g\", 1) }\n",
        );
        let out = judged8(
            "r4c-dict",
            fnonc,
            "fn solve(d: Dict) { dict_set(d, \"f\", 0) }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("retyped a dict entry")),
            "ATTACK: a closure the operator held was replaced by a non-closure: {out:?}"
        );
        // The candidate's closure, stored where the operator held its own.
        let fsuite = "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"f\", |n: i64| n * n)\n    solve(d)\n    match dict_get(d, \"f\") {\n        Some(f) => assert(f(3).ok())\n        None => assert(false)\n    }\n}\n";
        honest8(
            "r4c-dict",
            fsuite,
            "fn solve(d: Dict) { dict_set(d, \"g\", 1) }\n",
        );
        let out = judged8(
            "r4c-dict",
            fsuite,
            "fn solve(d: Dict) { dict_set(d, \"f\", |n: i64| narrow(n)) }\n",
        );
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("retyped a dict entry")),
            "ATTACK: the candidate's closure replaced the operator's in its dict: {out:?}"
        );
    }

    /// What the rule does NOT touch: keys the candidate ADDS, a dict the
    /// candidate builds, the same type written again, and a dict the operator
    /// itself retypes between two calls (the snapshot is retaken).
    #[test]
    fn a_dict_the_candidate_adds_to_or_builds_still_crosses() {
        let suite = dict_suite("solve(d)");
        honest8(
            "r4c-dict",
            &suite,
            "fn solve(d: Dict) {\n    dict_set(d, \"a\", 9)\n    dict_set(d, \"b\", narrow(4))\n}\n",
        );
        honest8(
            "r4c-dict",
            "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"a\", 3)\n    solve(d)\n    dict_set(d, \"a\", 7.5)\n    touch(d)\n    assert(true)\n}\n",
            "fn solve(d: Dict) { dict_set(d, \"a\", 9) }\nfn touch(d: Dict) { dict_set(d, \"a\", 8.5) }\n",
        );
        honest8(
            "r4c-dict",
            "@[test]\nfn t() {\n    match dict_get(make(), \"a\") {\n        Some(x) => assert(x.ok())\n        None => assert(false)\n    }\n}\n",
            "fn make() -> Dict {\n    let d = dict_new()\n    dict_set(d, \"a\", 9)\n    d\n}\n",
        );
    }

    /// Round 5 BLOCKER (amendment 78): the candidate REPLACES a position the
    /// operator held with a candidate-built value carrying a retyped element.
    /// A held dict's recorded type is just `Dict`, so the replacement used to
    /// pass and its nested values were never cast. A replacement is judged by
    /// what the operator held at that position, deeply — through nested
    /// dicts, arrays, structs, `Option`s — and a key only the replacement has
    /// is free (exactly as part 2's new keys).
    #[test]
    fn a_position_the_operator_held_is_judged_by_what_it_held_when_replaced() {
        // The operator's `d` holds `inner = {x: 3}` at "inner"; it reads
        // d.inner.x untyped afterwards.
        let nested = |held: &str, read: &str, call: &str| {
            format!("@[test]\nfn t() {{\n    let inner = dict_new()\n    dict_set(inner, \"x\", 3)\n    let d = dict_new()\n    dict_set(d, \"inner\", {held})\n    {call}\n    match dict_get(d, \"inner\") {{\n        Some(h) => {read}\n        None => assert(false)\n    }}\n}}\n")
        };
        let wrap = "type Wrap<T> = { v: T }\n";
        let build = |x: &str| {
            format!(
                "let n = dict_new()\n    dict_set(n, \"x\", {x})\n    dict_set(d, \"inner\", n)"
            )
        };
        let read_dict = "match dict_get(h, \"x\") {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }";
        let suite = nested("inner", read_dict, "solve(d)");
        live8(
            "r4c-repl",
            &suite,
            &format!("fn solve(d: Dict) {{\n    {}\n}}\n", build("9")),
            &format!("fn solve(d: Dict) {{\n    {}\n}}\n", build("4")),
        );
        dict_refused(
            "the review's candidate-built dict at the held key",
            &suite,
            &format!(
                "fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", n)")
            ),
        );
        // A key only the replacement has is free; the key both have is checked.
        honest8(
            "r4c-repl",
            &suite,
            "fn solve(d: Dict) {\n    let n = dict_new()\n    dict_set(n, \"x\", 9)\n    dict_set(n, \"y\", narrow(4))\n    dict_set(d, \"inner\", n)\n}\n",
        );
        // Two levels down.
        dict_refused(
            "a dict nested in the replacing dict",
            "@[test]\nfn t() {\n    let leaf = dict_new()\n    dict_set(leaf, \"x\", 3)\n    let mid = dict_new()\n    dict_set(mid, \"leaf\", leaf)\n    let d = dict_new()\n    dict_set(d, \"mid\", mid)\n    solve(d)\n    match dict_get(d, \"mid\") {\n        Some(m) => match dict_get(m, \"leaf\") {\n            Some(l) => match dict_get(l, \"x\") {\n                Some(v) => assert(v.ok())\n                None => assert(false)\n            }\n            None => assert(false)\n        }\n        None => assert(false)\n    }\n}\n",
            &format!(
                "fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8("let leaf = dict_new()\n        dict_set(leaf, \"x\", v)\n        let mid = dict_new()\n        dict_set(mid, \"leaf\", leaf)\n        dict_set(d, \"mid\", mid)")
            ),
        );
        // The family: an array, a struct, an Option or a tuple carrying a dict,
        // and an array or struct carrying scalars, replaced by the candidate's.
        let repl = |held: &str, read: &str, cand: String| {
            dict_refused(held, &nested(held, read, "solve(d)"), &cand);
        };
        repl(
            "[inner]",
            "match dict_get(h[0], \"x\") {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }",
            format!(
                "fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", [n])")
            ),
        );
        repl(
            "Some(inner)",
            "match h {\n            Some(i) => match dict_get(i, \"x\") {\n                Some(v) => assert(v.ok())\n                None => assert(false)\n            }\n            None => assert(false)\n        }",
            format!(
                "fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", Some(n))")
            ),
        );
        repl(
            "(inner, 1)",
            "match dict_get(h.0, \"x\") {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }",
            format!(
                "fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", (n, 1))")
            ),
        );
        let wsuite = format!(
            "{wrap}{}",
            nested(
                "Wrap { v: inner }",
                "match dict_get(h.v, \"x\") {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }",
                "solve(d)"
            )
        );
        dict_refused(
            "Wrap { v: inner }",
            &wsuite,
            &format!(
                "fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", Wrap { v: n })")
            ),
        );
        honest8(
            "r4c-repl",
            &wsuite,
            "fn solve(d: Dict) {\n    let n = dict_new()\n    dict_set(n, \"x\", 9)\n    dict_set(d, \"inner\", Wrap { v: n })\n}\n",
        );
        // Scalars in containers: the leaf rule.
        for (held, read, newv) in [
            ("[3]", "assert(h[0].ok())", "[v]"),
            ("Wrap { v: 3 }", "assert(h.v.ok())", "Wrap { v: v }"),
            ("(3, 1)", "assert(h.0.ok())", "(v, 1)"),
            ("Some(3)", "match h {\n            Some(x) => assert(x.ok())\n            None => assert(false)\n        }", "Some(v)"),
        ] {
            let suite = format!("{wrap}{}", nested(held, read, "solve(d)"));
            dict_refused(
                held,
                &suite,
                &format!(
                    "{wrap}fn solve(d: Dict) {{\n    {}\n}}\n",
                    put_u8(&format!("dict_set(d, \"inner\", {newv})"))
                ),
            );
        }
        // Over a channel: the dict the candidate received.
        dict_refused(
            "a held dict received on the operator's channel",
            &format!(
                "@[test]\nfn t() {{\n    let inner = dict_new()\n    dict_set(inner, \"x\", 3)\n    let d = dict_new()\n    dict_set(d, \"inner\", inner)\n    let c = chan<Dict>()\n    c.send(d)\n    solve(c)\n    match dict_get(d, \"inner\") {{\n        Some(h) => {read_dict}\n        None => assert(false)\n    }}\n}}\n"
            ),
            &format!(
                "fn solve(c: Chan<Dict>) {{\n    let d = c.recv()\n    {}\n}}\n",
                put_u8("let n = dict_new()\n        dict_set(n, \"x\", v)\n        dict_set(d, \"inner\", n)")
            ),
        );
    }

    /// Round 5 MAJOR-ADJACENT (amendment 78): a position the operator held at
    /// an UNDETERMINED type (`None`, an empty array) is refused to the
    /// candidate — a strict store, as at a strict crossing — instead of left
    /// free. An operator that wants the candidate to fill a slot holds a
    /// typed placeholder (`Some(0)`, `[0]`).
    #[test]
    fn a_placeholder_the_operator_held_is_not_filled_by_the_candidate() {
        let suite = |held: &str, read: &str| {
            format!("@[test]\nfn t() {{\n    let d = dict_new()\n    dict_set(d, \"best\", {held})\n    solve(d)\n    match dict_get(d, \"best\") {{\n        Some(o) => {read}\n        None => assert(false)\n    }}\n}}\n")
        };
        let some = "match o {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }";
        let some_suite = suite("None", some);
        let fill = |x: &str| format!("fn solve(d: Dict) {{ dict_set(d, \"best\", Some({x})) }}\n");
        // A typed placeholder is filled by the candidate, and judged.
        live8("r4c-hold", &suite("Some(0)", some), &fill("9"), &fill("4"));
        dict_refused(
            "None filled with a u8",
            &some_suite,
            &format!(
                "fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8("dict_set(d, \"best\", Some(v))")
            ),
        );
        dict_refused(
            "None filled with an i64 (no position was determined)",
            &some_suite,
            &fill("9"),
        );
        dict_refused(
            "an empty array filled",
            &suite("[]", "assert(o[0].ok())"),
            &format!(
                "fn solve(d: Dict) {{\n    {}\n}}\n",
                put_u8("dict_set(d, \"best\", [v])")
            ),
        );
        // Leaving the placeholder alone, or emptying a typed slot, is fine.
        honest8(
            "r4c-hold",
            "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"best\", None)\n    dict_set(d, \"n\", 1)\n    solve(d)\n    assert(true)\n}\n",
            "fn solve(d: Dict) { dict_set(d, \"n\", 2) }\n",
        );
        honest8(
            "r4c-hold",
            "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"best\", Some(3))\n    solve(d)\n    assert(true)\n}\n",
            "fn solve(d: Dict) { dict_set(d, \"best\", None) }\n",
        );
    }

    // ── C9 round 6, PSV-1 (amendment 83): the DISPATCH rule ──────────────────
    //
    // In a sealed run, operator code does not dispatch an operator impl's
    // method on a receiver whose type nothing on the operator side determined.
    // These tests run with the rule ON (the other layers' tests run with it
    // off, so each is judged by its own attack).

    fn judged_on(tag: &str, suite: &str, cand: &str) -> Result<TestEnd, String> {
        sealed_outcome_rule(
            tag,
            &format!("{JUDGE8}{suite}"),
            &format!("{LAUNDER8}{cand}"),
            "t",
            true,
        )
    }

    fn dispatch_refused(why: &str, suite: &str, cand: &str) {
        let out = judged_on("r6-disp", suite, cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("whose type nothing on the operator side determined")),
            "ATTACK: operator code dispatched on a value of the candidate's chosen type ({why}): {out:?}"
        );
    }

    /// A suite reading the candidate-written `result` of a dict, either
    /// UNPINNED (`x.ok()` on the read) or PINNED (`let y: i64 = x`).
    fn out_suite(setup: &str, unpinned: bool) -> String {
        let read = if unpinned {
            "assert(x.ok())"
        } else {
            "{ let y: i64 = x\n            assert(y.ok()) }"
        };
        format!("@[test]\nfn t() {{\n    {setup}\n    match dict_get(out, \"result\") {{\n        Some(x) => {read}\n        None => assert(false)\n    }}\n}}\n")
    }

    /// d2/d3 and the empty accumulator: the candidate writes (or returns) a
    /// dict the operator then reads. Unpinned: refused at the dispatch. Pinned
    /// by `let y: i64`: GOOD passes, WRONG fails, the u8 is refused by the cast.
    #[test]
    fn operator_code_never_dispatches_on_a_value_read_untyped_from_a_dict() {
        for (why, setup, fill) in [
            (
                "an output dict the candidate fills",
                "let out = dict_new()\n    solve(out)",
                "fn solve(out: Dict) { dict_set(out, \"result\", VAL) }\n",
            ),
            (
                "a returned dict",
                "let out = solve()",
                "fn solve() -> Dict {\n    let d = dict_new()\n    dict_set(d, \"result\", VAL)\n    d\n}\n",
            ),
            (
                "an empty accumulator",
                "let out = dict_new()\n    let n = 0\n    fill(out, n)",
                "fn fill(out: Dict, n: i64) { dict_set(out, \"result\", VAL) }\n",
            ),
        ] {
            let cand = |v: &str| fill.replace("VAL", v);
            let atk = cand("narrow(4)");
            dispatch_refused(why, &out_suite(setup, true), &atk);
            let pinned = out_suite(setup, false);
            assert_eq!(judged_on("r6-disp", &pinned, &cand("9")), Ok(TestEnd::Completed), "control ({why}): GOOD, pinned");
            assert!(judged_on("r6-disp", &pinned, &cand("4")).is_err(), "control ({why}): WRONG fails");
            let out = judged_on("r6-disp", &pinned, &atk);
            assert!(
                out != Ok(TestEnd::Completed) && confusion_refused(&out),
                "ATTACK: a u8 passed the suite's `let y: i64` pin ({why}): {out:?}"
            );
        }
    }

    /// `match <laundered u8> { Some(v) => { a[0] = v } None => {} }` as the
    /// body of a candidate `fill(a: &mut [i64])`.
    fn mut_fill(decl: &str, act: &str) -> String {
        format!(
            "fn fill(a: {decl}) {{\n    {}\n}}\n",
            u8_or(&format!("{{ {act} }}"), "{}")
        )
    }

    fn mut_refused(why: &str, out: &Result<TestEnd, String>, needle: &str) {
        assert!(
            out != &Ok(TestEnd::Completed) && matches!(out, Err(m) if m.contains(needle)),
            "ATTACK: a `&mut` write-through carried a candidate-chosen value to the operator ({why}): {out:?}"
        );
    }

    /// C9 round 8, PSV-1 blocker: a candidate fn that writes through `&mut`
    /// returns control to operator code with the parameter's FINAL value, which
    /// moved into the operator's binding with no seal edge. Cast at the edge
    /// back against the DECLARED parameter type (dispatch arm and width arm).
    #[test]
    fn a_mut_write_through_value_is_cast_at_the_seal_edge_back() {
        let dispatch = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    assert(a[0].ok())\n}\n";
        let width = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    assert((a[0] << 1) == 254)\n}\n";
        live8(
            "r8-mutcast",
            dispatch,
            "fn fill(a: &mut [i64]) { a[0] = 9 }\n",
            "fn fill(a: &mut [i64]) { a[0] = 4 }\n",
        );
        live8(
            "r8-mutcast",
            width,
            "fn fill(a: &mut [i64]) { a[0] = 127 }\n",
            "fn fill(a: &mut [i64]) { a[0] = 200 }\n",
        );
        // First: into an EMPTY output array, which shows no type at the position
        // (so what the operator held cannot judge it): the honest fill passes, and
        // the declared type alone refuses an appended `u8` — or a value at an
        // undetermined type parameter.
        let empty = "@[test]\nfn t() {\n    let mut a: [i64] = []\n    fill(&mut a)\n    assert(a[0].ok())\n}\n";
        live8(
            "r8-mutcast",
            empty,
            "fn fill(a: &mut [i64]) { a = arr_concat(a, [9]) }\n",
            "fn fill(a: &mut [i64]) { a = arr_concat(a, [4]) }\n",
        );
        mut_refused(
            "an appended u8 into an empty output array",
            &judged8(
                "r8-mutcast",
                empty,
                &mut_fill("&mut [i64]", "a = arr_concat(a, [v])"),
            ),
            "left holding",
        );
        mut_refused(
            "a generic element into an empty output array",
            &judged8(
                "r8-mutcast",
                empty,
                &mut_fill("&mut [T]", "a = arr_concat(a, [v])").replace("fn fill(", "fn fill<T>("),
            ),
            "left holding",
        );
        let atk = mut_fill("&mut [i64]", "a[0] = v");
        mut_refused(
            "the u8 impl answered",
            &judged8("r8-mutcast", dispatch, &atk),
            "left holding",
        );
        let atk255 = atk.replace("narrow(4)", "narrow(255)");
        mut_refused(
            "255 as u8 wrapped in the operator's shift",
            &judged8("r8-mutcast", width, &atk255),
            "left holding",
        );
    }

    /// The same edge judges a replacement by what the operator HELD there (the
    /// declared type alone admits it: `[i64 | u8]` admits a `u8`; a `[Dict]`
    /// admits a fresh dict whose key was retyped).
    #[test]
    fn a_mut_write_through_value_is_judged_by_what_the_operator_held() {
        let dispatch = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    assert(a[0].ok())\n}\n";
        honest8(
            "r8-mutheld",
            dispatch,
            "fn fill(a: &mut [i64 | u8]) { a[0] = 9 }\n",
        );
        mut_refused(
            "a union element admitted the u8",
            &judged8(
                "r8-mutheld",
                dispatch,
                &mut_fill("&mut [i64 | u8]", "a[0] = v"),
            ),
            "did not allow",
        );
        let dicts = "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"k\", 3)\n    let mut a = [d]\n    fill(&mut a)\n    match dict_get(a[0], \"k\") {\n        Some(x) => assert(x.ok())\n        None => assert(false)\n    }\n}\n";
        live8(
            "r8-mutheld",
            dicts,
            "fn fill(a: &mut [Dict]) {\n    let n = dict_new()\n    dict_set(n, \"k\", 9)\n    a[0] = n\n}\n",
            "fn fill(a: &mut [Dict]) {\n    let n = dict_new()\n    dict_set(n, \"k\", 4)\n    a[0] = n\n}\n",
        );
        mut_refused(
            "a fresh dict replaced the held one with a retyped key",
            &judged8(
                "r8-mutheld",
                dicts,
                &mut_fill(
                    "&mut [Dict]",
                    "let n = dict_new()\n        dict_set(n, \"k\", v)\n        a[0] = n",
                ),
            ),
            "did not allow",
        );
        // The same replacement with the dispatch rule ON, and an in-place retype of
        // the dict an array held (the snapshot edge), rule off and on.
        let repl = mut_fill(
            "&mut [Dict]",
            "let n = dict_new()\n        dict_set(n, \"k\", v)\n        a[0] = n",
        );
        let out = judged_on("r8-mutheld", dicts, &repl);
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: a replaced `&mut [Dict]` element passed with the rule on: {out:?}"
        );
        let inplace = mut_fill("&mut [Dict]", "dict_set(a[0], \"k\", v)");
        mut_refused(
            "an in-place retype of the held dict",
            &judged8("r8-mutheld", dicts, &inplace),
            "retyped a dict entry",
        );
        let out = judged_on("r8-mutheld", dicts, &inplace);
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: an in-place retype passed with the rule on: {out:?}"
        );
    }

    /// The cast runs on EVERY outcome: an operator handler that aborts the call
    /// (`HandlerDone`) after the candidate wrote a `u8` must not leave it in the
    /// operator's binding.
    #[test]
    fn a_mut_value_is_cast_when_an_operator_handler_aborts_the_call() {
        let suite = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    let _r = with handler { on IO(p) => 0 } {\n        fill(&mut a)\n        1\n    }\n    assert(a[0].ok())\n}\n";
        // Control: an honest write then an aborted call keeps the honest value.
        let honest = "fn fill(a: &mut [i64]) {\n    a[0] = 9\n    println(\"x\")\n}\n";
        assert_eq!(
            judged8("r8-mutabort", suite, honest),
            Ok(TestEnd::Completed),
            "control"
        );
        let out = judged8(
            "r8-mutabort",
            suite,
            &mut_fill("&mut [i64]", "a[0] = v\n        println(\"x\")"),
        );
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: a `&mut` u8 survived an aborted call into the operator's binding: {out:?}"
        );
    }

    /// An ANNOTATED operator array is determined by its annotation, so the
    /// dispatch rule cannot help: only the cast at the edge back refuses.
    #[test]
    fn an_annotated_operator_array_lent_as_mut_is_still_cast_at_the_edge_back() {
        let suite = "@[test]\nfn t() {\n    let mut a: [i64] = [3]\n    fill(&mut a)\n    let y: i64 = a[0]\n    assert(y.ok())\n}\n";
        live8(
            "r8-mutann",
            suite,
            "fn fill(a: &mut [i64]) { a[0] = 9 }\n",
            "fn fill(a: &mut [i64]) { a[0] = 4 }\n",
        );
        mut_refused(
            "the annotated array was left holding a u8",
            &judged8("r8-mutann", suite, &mut_fill("&mut [i64]", "a[0] = v")),
            "left holding",
        );
    }

    /// The dispatch rule sees a `&mut` operand as OPEN: what the callee left in
    /// it was not chosen by the operator. The honest suite re-pins.
    #[test]
    fn a_mut_operand_is_open_in_the_dispatch_analysis() {
        let unpinned = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    assert(a[0].ok())\n}\n";
        let repinned = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    let b: [i64] = a\n    assert(b[0].ok())\n}\n";
        let good = "fn fill(a: &mut [i64]) { a[0] = 9 }\n";
        let wrong = "fn fill(a: &mut [i64]) { a[0] = 4 }\n";
        // The cost, stated: even an honest write is refused until re-pinned.
        dispatch_refused("an un-re-pinned read after `&mut`", unpinned, good);
        assert_eq!(
            judged_on("r8-mutopen", repinned, good),
            Ok(TestEnd::Completed),
            "control: GOOD, re-pinned"
        );
        assert!(
            judged_on("r8-mutopen", repinned, wrong).is_err(),
            "control: WRONG fails"
        );
        let out = judged_on("r8-mutopen", repinned, &mut_fill("&mut [i64]", "a[0] = v"));
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: re-pinned suite accepted the u8: {out:?}"
        );
        // The arithmetic arm, unpinned.
        let width = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    assert((a[0] << 1) == 254)\n}\n";
        let out = judged_on(
            "r8-mutopen",
            width,
            &mut_fill("&mut [i64]", "a[0] = v").replace("narrow(4)", "narrow(255)"),
        );
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: width arm accepted a `&mut` value: {out:?}"
        );
    }

    /// The analysis itself, with the cast out of the picture: a variable passed
    /// as `&mut x` is NOT determined; the same program without the call is.
    #[test]
    fn a_mut_operand_is_never_determined_by_the_pin_analysis() {
        let verdict = |src: &str| {
            let prog = crate::parse_source(src).expect("parses");
            let pins = pin::Pins::build(&prog, &|_| false);
            let mut out = None;
            for item in &prog.items {
                if let Item::FnDef(f) = item {
                    if f.name != "t" {
                        continue;
                    }
                    crate::ast::walk_expr(&f.body, &mut |e: &Expr| {
                        if let Expr::MethodCall {
                            receiver, method, ..
                        } = e
                        {
                            if method == "ok" {
                                out = Some(pins.determined(
                                    f as *const FnDef as usize,
                                    receiver,
                                    method,
                                ));
                            }
                        }
                    });
                }
            }
            out.expect("a call site")
        };
        let base = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nfn fill(a: &mut [i64]) { a[0] = 9 }\n";
        let with = |call: &str| {
            format!("{base}fn t() {{\n    let mut a = [1, 2, 3]\n    {call}\n    let r = a[0].ok()\n}}\n")
        };
        assert!(
            verdict(&with("let z = 0")),
            "control: a literal array is determined"
        );
        assert!(
            !verdict(&with("fill(&mut a)")),
            "ATTACK: a variable lent as `&mut` was still determined"
        );
        // A union annotation does not pin the one runtime type.
        let un = format!("{base}fn t() {{\n    let a: i64 | u8 = 9\n    let r = a.ok()\n}}\n");
        assert!(
            !verdict(&un),
            "ATTACK: a union annotation pinned the receiver"
        );
    }

    /// The interpreter's source files, with each file's own test module cut off.
    pub(super) fn interp_sources() -> Vec<(&'static str, String)> {
        let cut = |s: &str| {
            let end = s.find("\n#[cfg(test)]\nmod tests").unwrap_or(s.len());
            s[..end].to_string()
        };
        vec![
            ("interp.rs", cut(include_str!("interp.rs"))),
            (
                "interp/builtins.rs",
                cut(include_str!("interp/builtins.rs")),
            ),
            ("interp/conform.rs", cut(include_str!("interp/conform.rs"))),
            ("interp/eval.rs", cut(include_str!("interp/eval.rs"))),
            ("interp/goal.rs", cut(include_str!("interp/goal.rs"))),
            ("interp/pin.rs", cut(include_str!("interp/pin.rs"))),
            ("interp/taint.rs", cut(include_str!("interp/taint.rs"))),
            (
                "interp/proptest.rs",
                cut(include_str!("interp/proptest.rs")),
            ),
            (
                "interp/provenance.rs",
                cut(include_str!("interp/provenance.rs")),
            ),
            ("interp/value.rs", cut(include_str!("interp/value.rs"))),
            ("interp/regex.rs", cut(include_str!("interp/regex.rs"))),
        ]
    }

    /// C9 round 9 drift: the global-read edge lives in ONE lookup. Every
    /// non-comment line of the interpreter that touches the `globals` map is
    /// listed here with the reason it is allowed; a new read (a fast path, a
    /// cache, a debug dump) fails this test until it is routed through
    /// `Interp::global_ref` or listed with its reason.
    #[test]
    fn every_global_read_goes_through_global_ref() {
        // C9 round 10: ANY mention of the word `globals` (any receiver, a
        // binding, a struct pattern, a field initialiser), not only
        // `self.`/`interp.`. `interp/pin.rs` is excluded as a whole: it holds a
        // compile-time `HashSet<String>` of NAMES and names neither `Interp`
        // nor `Value` (asserted below).
        const ALLOWED: &[(&str, &str, &str)] = &[
            ("interp.rs", "globals: std::collections::HashSet<String>,", "the seal's set of global NAMES"),
            ("interp.rs", "globals: HashMap<String, Value>,", "the map's definition"),
            ("interp.rs", "let mut merged: HashMap<&String, &Value> = interp.globals.iter().collect();", "session post-run report (operator side, after the run)"),
            ("interp.rs", "let mut merged: HashMap<&String, &Value> = interp.globals.iter().collect();", "session post-run report (operator side, after the run)"),
            ("interp.rs", "seal.globals.insert(name.clone());", "the seal's set of NAMES"),
            ("interp.rs", "globals: HashMap::new(),", "the map's initialiser"),
            ("interp.rs", "let sealed = self.seal.active && self.seal.globals.contains(name);", "the seal's set of NAMES"),
            ("interp.rs", "self.globals.insert(name.clone(), v);", "init_globals: the definition itself"),
            ("interp.rs", "if self.seal.active && self.frame_sealed.get() && !self.seal.globals.contains(name) {", "seal_global: the edge itself (names)"),
            ("interp.rs", "self.globals.contains_key(name)", "is_global: existence only, no value"),
            ("interp.rs", "match self.globals.get(name) {", "global_ref: THE lookup, which applies seal_global"),
        ];
        let is_word = |c: char| c.is_alphanumeric() || c == '_';
        let mut found: Vec<(String, String)> = Vec::new();
        for (file, src) in interp_sources() {
            if file == "interp/pin.rs" {
                assert!(
                    !src.contains("Interp") && !src.contains("Value"),
                    "pin.rs now touches runtime values: its `globals` are no longer only names"
                );
                continue;
            }
            for line in src.lines() {
                let t = line.trim();
                if t.starts_with("//") {
                    continue;
                }
                let mut rest = t;
                let mut before_c: Option<char> = None;
                while let Some(i) = rest.find("globals") {
                    let before = rest[..i].chars().next_back().or(before_c);
                    let after = &rest[i + "globals".len()..];
                    if !before.is_some_and(is_word) && !after.starts_with(is_word) {
                        found.push((file.to_string(), t.to_string()));
                        break;
                    }
                    before_c = Some('s');
                    rest = after;
                }
            }
        }
        let mut allowed: Vec<(String, String)> = ALLOWED
            .iter()
            .map(|(f, l, _)| (f.to_string(), l.to_string()))
            .collect();
        found.sort();
        allowed.sort();
        assert_eq!(
            found, allowed,
            "DRIFT: a mention of the `globals` map bypasses `global_ref` (and so `seal_global`); route it through `Interp::global_ref`, or list it with its reason"
        );
    }

    /// The identifiers called (`name(` / `.name(`) on one line.
    fn called_idents(line: &str) -> Vec<String> {
        let b: Vec<char> = line.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i].is_alphabetic() || b[i] == '_' {
                let st = i;
                while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                    i += 1;
                }
                // A call on some OTHER receiver (`ch.send(..)`, `x.get(..)`) is not
                // a call of an interpreter fn of that name.
                let pre: String = b[..st].iter().collect();
                let other_receiver =
                    pre.ends_with('.') && !pre.ends_with("self.") && !pre.ends_with("interp.");
                if i < b.len() && b[i] == '(' && !other_receiver {
                    out.push(b[st..i].iter().collect());
                }
            } else {
                i += 1;
            }
        }
        out
    }

    /// `(file, fn name, body)` of every fn item in the interpreter sources.
    fn fn_items() -> Vec<(&'static str, String, String)> {
        let mut out = Vec::new();
        for (file, src) in interp_sources() {
            let lines: Vec<&str> = src.lines().collect();
            let mut i = 0;
            while i < lines.len() {
                let l = lines[i];
                let t = l.trim_start();
                let indent = l.len() - t.len();
                let t2 = t
                    .strip_prefix("pub(super) ")
                    .or_else(|| t.strip_prefix("pub(crate) "))
                    .or_else(|| t.strip_prefix("pub "))
                    .unwrap_or(t);
                if let Some(rest) = t2.strip_prefix("fn ") {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let mut body = String::new();
                    let mut j = i + 1;
                    // The signature may span lines; the body ends at the line
                    // that closes at the fn's own indent.
                    while j < lines.len() {
                        let lj = lines[j];
                        if lj.len() - lj.trim_start().len() == indent
                            && lj.trim_start().starts_with('}')
                        {
                            break;
                        }
                        body.push_str(lj);
                        body.push('\n');
                        j += 1;
                    }
                    out.push((file, name, body));
                    i += 1;
                    continue;
                }
                i += 1;
            }
        }
        out
    }

    /// Every interpreter fn that runs a `Value::Closure` or an `FnDef`, found
    /// by closure over the call graph from the primitives that do (`call_fn`,
    /// `call_closure`, ...). The evaluator itself and the builtin dispatcher
    /// are the graph's ROOTS, not its nodes: everything reaches them.
    fn user_code_runners() -> std::collections::HashSet<String> {
        const PRIMITIVES: &[&str] = &[
            "call_fn",
            "call_fn_mut",
            "call_fn_frame",
            "call_closure",
            "call_local_closure",
            "call_closure_owned_by",
        ];
        const ROOTS: &[&str] = &[
            "eval",
            "eval_call",
            "eval_call_mut",
            "eval_block",
            "eval_binop",
            "eval_with_handler",
            "eval_method_call",
            "call_builtin",
            "run_handler_arm",
            "replay_continuation",
            // Host entry points that START a run: no builtin arm reaches them
            // (and `axon_host_await` is also the name of an extern declaration).
            "run_program_inner",
            "run_main",
            "run_test_fn_inner",
            "axon_host_await",
            "run_named_fn_as_bool_with_score",
            "run_suspendable_values_inner",
            "run_property_test_inner",
        ];
        let items = fn_items();
        let mut runners: std::collections::HashSet<String> =
            PRIMITIVES.iter().map(|s| s.to_string()).collect();
        loop {
            let before = runners.len();
            for (file, name, body) in &items {
                if *file == "interp/eval.rs" || ROOTS.contains(&name.as_str()) {
                    continue;
                }
                if body.lines().any(|l| {
                    !l.trim_start().starts_with("//")
                        && called_idents(l)
                            .iter()
                            .any(|c| runners.contains(c) && c != name)
                }) {
                    runners.insert(name.clone());
                }
            }
            if runners.len() == before {
                break;
            }
        }
        runners
    }

    /// What the sweep established about a builtin that runs user code.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Disp {
        /// The result's type names a type variable (`[U]`, `T`): the pin
        /// analysis treats it as undetermined.
        Open,
        /// A `Dict`: its values are an untyped position (amendment 83).
        Dict,
        /// A scalar the builtin builds itself (a count, a score, `()`).
        Scalar,
        /// The candidate fn's value is cast to the declared type at the seal
        /// crossing in the arm itself (`fn_is_sealed(f)` appears in the arm).
        Crossing,
    }

    /// C9 round 9 drift: every builtin that RUNS user code (a callback, a fn
    /// named by string, a goal metric, a scheduler fiber) is classified by how
    /// its result reaches operator code. A new one fails this test until it is
    /// classified, and its declared return must be consistent with the class.
    #[test]
    fn every_builtin_that_runs_user_code_is_classified() {
        use Disp::*;
        const SCALAR_RETS: &[&str] = &["i64", "f64", "bool", "str", "()", "Result<i64, str>"];
        const TABLE: &[(&str, Disp)] = &[
            ("arr_all", Scalar),
            ("arr_any", Scalar),
            ("arr_count_if", Scalar),
            ("arr_drop_while", Open),
            ("arr_filter", Open),
            ("arr_find", Open),
            ("arr_fold", Open),
            ("arr_group_by", Dict),
            ("arr_map", Open),
            ("arr_max_by", Open),
            ("arr_min_by", Open),
            ("arr_partition", Open),
            ("arr_sort_by", Open),
            ("arr_sum_by", Scalar),
            ("arr_sum_by_f64", Scalar),
            ("arr_take_while", Open),
            ("arr_zip_with", Open),
            ("dict_each", Scalar),
            ("dict_filter", Dict),
            ("dict_map_values", Dict),
            ("goal_continue", Scalar),
            ("goal_eval", Scalar),
            ("goal_run", Scalar),
            ("goal_run_categorical", Scalar),
            ("goal_run_constrained", Scalar),
            ("goal_run_multistart", Scalar),
            ("goal_run_random", Scalar),
            ("http_sse", Scalar),
            ("http_sse_post", Scalar),
            ("kernel_goal_run", Scalar),
            ("sandbox_run", Crossing),
            ("scheduler_run", Scalar),
            ("supervisor_run", Scalar),
        ];
        let src = interp_sources()
            .into_iter()
            .find(|(f, _)| *f == "interp/builtins.rs")
            .unwrap()
            .1;
        // Split `call_builtin`'s arms at their 12-space `"name" … =>` headers.
        let mut arms: Vec<(Vec<String>, String)> = Vec::new();
        for line in src.lines() {
            let indent = line.len() - line.trim_start().len();
            let t = line.trim_start();
            if indent == 12 && t.starts_with('"') && t.contains("=>") {
                let head = &t[..t.find("=>").unwrap()];
                let names = head
                    .split('|')
                    .map(|n| n.trim().trim_matches('"').to_string())
                    .collect();
                arms.push((names, String::new()));
            } else if let Some(last) = arms.last_mut() {
                last.1.push_str(line);
                last.1.push('\n');
            }
        }
        // C9 round 10: not four literal call patterns. A builtin runs user code
        // if its arm calls ANY fn that (transitively, through the interpreter's
        // own fns) reaches a runner of a `Value::Closure` or an `FnDef`.
        let runners = user_code_runners();
        let runs_user_code = |body: &str| {
            body.lines().any(|l| {
                let l = l.trim_start();
                !l.starts_with("//") && called_idents(l).iter().any(|c| runners.contains(c))
            })
        };
        let declared: std::collections::HashMap<&str, &str> = crate::builtins::BUILTINS
            .iter()
            .map(|b| (b.name, b.ret))
            .collect();
        let mut found: Vec<String> = Vec::new();
        for (names, body) in &arms {
            if !runs_user_code(body) {
                continue;
            }
            for n in names {
                if declared.contains_key(n.as_str()) {
                    found.push(n.clone());
                }
            }
        }
        found.sort();
        found.dedup();
        let mut want: Vec<String> = TABLE.iter().map(|(n, _)| n.to_string()).collect();
        want.sort();
        assert_eq!(
            found, want,
            "DRIFT: a builtin arm that runs user code is unclassified (or a classified one no longer does); decide how its result reaches operator code and list it"
        );
        for (name, disp) in TABLE {
            let ret = declared[name];
            let ok = match disp {
                Open => pin::builtin_ret_open(ret),
                Dict => ret == "Dict",
                Scalar => SCALAR_RETS.contains(&ret),
                Crossing => arms.iter().any(|(ns, b)| {
                    ns.iter().any(|n| n == name) && b.contains("self.fn_is_sealed(f)")
                }),
            };
            assert!(
                ok,
                "DRIFT: `{name}` is classified {disp:?} but declares `-> {ret}`"
            );
        }
        // The goal metric and the property runner score or discard the result.
        for (file, src) in interp_sources() {
            if file != "interp/goal.rs" {
                continue;
            }
            let lines: Vec<&str> = src.lines().collect();
            for (i, l) in lines.iter().enumerate() {
                if l.trim_start().starts_with("//") || !l.contains("self.call_fn(") {
                    continue;
                }
                let window = lines[i..(i + 40).min(lines.len())].join("\n");
                assert!(
                    window.contains("numeric_score(") || window.contains("Value::Bool("),
                    "DRIFT: goal.rs line {} runs user code and does not reduce the result to a score or a bool",
                    i + 1
                );
            }
        }
    }

    /// Amendment 96, honest-program cost (fail-closed over-refusal, stated): the
    /// VALUE of a `with handler` expression is undetermined to the dispatch rule
    /// (a handler arm may answer with any value), so an operator dispatch on it
    /// is refused until it is pinned. Dispatch on determined values INSIDE a
    /// handler body or arm is unaffected.
    #[test]
    fn the_value_of_a_with_handler_expression_is_undetermined_until_pinned() {
        let h = "with handler { on IO(p) => resume(Ok(\"x\")) } {\n        9\n    }";
        let run = |suite: String| judged_on("r9-wh", &suite, "fn mk() -> i64 { 9 }\n");
        let pinned = format!("@[test]\nfn t() {{\n    let v: i64 = {h}\n    assert(v.ok())\n}}\n");
        assert_eq!(
            run(pinned),
            Ok(TestEnd::Completed),
            "pinned: the cost is only the annotation"
        );
        let bare = format!("@[test]\nfn t() {{\n    let v = {h}\n    assert(v.ok())\n}}\n");
        let out = run(bare);
        assert!(
            matches!(&out, Err(m) if m.contains("nothing on the operator side determined")),
            "the value of a handler expression is undetermined: {out:?}"
        );
        let inside = "@[test]\nfn t() {\n    with handler { on IO(p) => resume(Ok(\"x\")) } {\n        let x: i64 = 9\n        assert(x.ok())\n    }\n}\n".to_string();
        assert_eq!(
            run(inside),
            Ok(TestEnd::Completed),
            "dispatch inside a handler body on a pinned value"
        );
    }

    // ── C9 round 9 (PSV1G, amendment 96) ─────────────────────────────────────

    /// The operator's sandbox suite: `body` runs in a sandbox scope.
    fn sandbox_suite(body: &str) -> String {
        format!("@[test]\nfn t() {{\n    let p = principal_root(\"r\", true, true, true, 100)\n    let sb = sandbox_create(p, \"IO\")\n{body}\n}}\n")
    }

    /// C9 round 9, PSV-1: `sandbox_run` declares `-> i64`, but the candidate fn
    /// it runs only had its OWN declared return applied (`-> u8` is satisfied by
    /// `4 as u8`), and the pin analysis treats the builtin's closed result as
    /// determined: `let r = sandbox_run(..)` then `r.ok()` dispatched the lenient
    /// `u8` impl, and `(r << 1) == 254` wrapped at `u8`. The result is now cast
    /// to the declared type at the seal crossing.
    #[test]
    fn sandbox_run_results_are_cast_at_the_seal_crossing() {
        let run = |suite: &str, cand: &str| judged_on("r9-sbx", suite, cand);
        let disp = sandbox_suite("    let r = sandbox_run(sb, \"work\", 0)\n    assert(r.ok())");
        // Controls: the honest candidate passes, a wrong i64 fails (the check is live).
        assert_eq!(
            run(&disp, "fn work(x: i64) -> i64 { 9 }\n"),
            Ok(TestEnd::Completed),
            "control: honest"
        );
        assert!(
            run(&disp, "fn work(x: i64) -> i64 { 4 }\n").is_err(),
            "control: wrong fails"
        );
        let unit = sandbox_suite("    let r = sandbox_run(sb, \"work\", 0)\n    assert_eq(r, 0)");
        assert_eq!(
            run(&unit, "fn work(x: i64) { }\n"),
            Ok(TestEnd::Completed),
            "control: a unit-returning callee is 0"
        );
        // The operator's OWN fn is not the candidate's value: it is not cast.
        let own = format!(
            "fn opwork(x: i64) -> i64 {{ 9 }}\n{}",
            sandbox_suite("    let r = sandbox_run(sb, \"opwork\", 0)\n    assert(r.ok())")
        );
        assert_eq!(
            run(&own, "fn solve() -> i64 { 1 }\n"),
            Ok(TestEnd::Completed),
            "control: operator callee"
        );
        // The attacks.
        let width =
            sandbox_suite("    let r = sandbox_run(sb, \"work\", 0)\n    assert((r << 1) == 254)");
        let pinned =
            sandbox_suite("    let r: i64 = sandbox_run(sb, \"work\", 0)\n    assert(r.ok())");
        for (what, suite, cand) in [
            (
                "dispatch on a u8 result",
                &disp,
                "fn work(x: i64) -> u8 { narrow(4) }\n",
            ),
            (
                "a u8 result wrapped by the operator's shift",
                &width,
                "fn work(x: i64) -> u8 { 255 as u8 }\n",
            ),
            (
                "a u8 result under a pinned binding",
                &pinned,
                "fn work(x: i64) -> u8 { narrow(4) }\n",
            ),
            ("a str result", &disp, "fn work(x: i64) -> str { \"a\" }\n"),
        ] {
            let out = run(suite, cand);
            assert!(
                out != Ok(TestEnd::Completed)
                    && matches!(&out, Err(m) if m.contains("sandbox_run") || m.contains("declared")),
                "ATTACK: sandbox_run handed the operator a value the candidate chose the type of ({what}): {out:?}"
            );
        }
    }

    /// C9 round 9, SENTINEL: the `Expr::FieldAccess` and `Expr::Index` arms read
    /// `self.globals` directly for an identifier receiver and never applied the
    /// global-read edge: `TABLE[0]`, `CFG.k`, `PAIR.0`, `NN[0][0]` and
    /// `|| TABLE[1]` all completed for sealed code reading an operator global.
    /// Every lookup now goes through `Interp::global_ref`.
    #[test]
    fn a_sealed_frame_cannot_read_an_operator_global_through_a_fast_path() {
        let suite = "type Cfg = { k: i64 }\nlet TABLE = [9, 8]\nlet CFG = Cfg { k: 9 }\nlet PAIR = (9, 4)\nlet NN = [[9]]\nlet FN = || 9\n\
                     @[test]\nfn t() { assert_eq(solve(), 9) }\n";
        let run = |cand: &str| sealed_outcome_rule("r9-glob", suite, cand, "t", true);
        // Controls: the candidate's OWN globals read through the same arms; a
        // wrong one fails; the operator's whole-value read stays refused.
        let own = "type Mc = { k: i64 }\nlet MT = [9, 8]\nlet MC = Mc { k: 9 }\nlet MP = (9, 4)\nlet MN = [[9]]\nlet MF = || 9\n";
        for (what, body) in [
            ("index", "MT[0]"),
            ("field", "MC.k"),
            ("tuple", "MP.0"),
            ("nested", "MN[0][0]"),
            ("closure const", "MF()"),
            ("lambda", "(|| MT[0])()"),
        ] {
            assert_eq!(
                run(&format!("{own}fn solve() -> i64 {{ {body} }}\n")),
                Ok(TestEnd::Completed),
                "control: the candidate's own global ({what})"
            );
        }
        assert!(
            run(&format!("{}fn solve() -> i64 {{ MT[1] }}\n", own)).is_err(),
            "control: wrong fails"
        );
        for (what, body) in [
            ("a whole read", "let t = TABLE\n    t[0]"),
            ("an index", "TABLE[0]"),
            ("a field", "CFG.k"),
            ("a tuple field", "PAIR.0"),
            ("a nested index", "NN[0][0]"),
            ("an index inside a lambda", "(|| TABLE[0])()"),
            ("a closure constant call", "FN()"),
        ] {
            let out = run(&format!("fn solve() -> i64 {{\n    {body}\n}}\n"));
            assert!(
                out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains("cannot use")),
                "ATTACK: a sealed frame read an operator global ({what}): {out:?}"
            );
        }
    }

    /// C9 round 9, SENTINEL minor: a candidate calling its OWN fn value with
    /// arguments (`let g = inc; g(n)`) was refused as "sealed code called an
    /// operator closure", because a fn value carried no sealed mark. The
    /// operator's fn values stay unmarked, so the refusal stands for them, and a
    /// candidate fn value cannot take the place of an operator closure a dict held.
    #[test]
    fn a_candidates_own_fn_value_takes_arguments_and_an_operators_still_does_not() {
        let run = |suite: &str, cand: &str| sealed_outcome_rule("r9-fnval", suite, cand, "t", true);
        let s = "@[test]\nfn t() { assert_eq(solve(3), 4) }\n";
        let own = "fn inc(n: i64) -> i64 { n + 1 }\nfn solve(n: i64) -> i64 {\n    let g = inc\n    g(n)\n}\n";
        assert_eq!(
            run(s, own),
            Ok(TestEnd::Completed),
            "the candidate's own fn value with an argument"
        );
        assert!(
            run(s, &own.replace("n + 1", "n + 2")).is_err(),
            "control: wrong fails"
        );
        // A fn value a candidate fn mutates a dict through, and one stored and
        // called back, still pass the dict edge.
        let dict = "fn bump(d: Dict) -> i64 {\n    dict_set(d, \"k\", 2)\n    2\n}\nfn solve(n: i64) -> i64 {\n    let d = dict_new()\n    dict_set(d, \"k\", 1)\n    let g = bump\n    g(d) + n - 1\n}\n";
        assert_eq!(
            run(s, dict),
            Ok(TestEnd::Completed),
            "a dict through the candidate's own fn value"
        );
        // An operator fn value held in a dict, called by sealed code with an
        // argument no contract determined: refused as before.
        let held = "fn plain(n: i64) -> i64 { n }\n@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"f\", plain)\n    assert_eq(solve(d), 3)\n}\n";
        let out = run(
            held,
            "fn solve(d: Dict) -> i64 {\n    match dict_get(d, \"f\") { Some(f) => f(3)  None => 0 }\n}\n",
        );
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("nothing on the operator side determined")),
            "ATTACK: sealed code called an operator fn value with an undetermined argument: {out:?}"
        );
        // A candidate fn value does not replace an operator closure a dict held.
        let slot = "@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"f\", || 9)\n    solve(d)\n    match dict_get(d, \"f\") { Some(f) => assert_eq(f(), 9)  None => assert(false) }\n}\n";
        let out = run(
            slot,
            "fn mine() -> i64 { 9 }\nfn solve(d: Dict) {\n    dict_set(d, \"f\", mine)\n}\n",
        );
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("operator closure")),
            "ATTACK: a candidate fn value replaced an operator closure in the operator's dict: {out:?}"
        );
    }

    /// C9 round 9 sweep: the dispatch rule's arithmetic arm covered `x + y` but
    /// not `-x` / `~x`, which wrap at a width the candidate chose just the same:
    /// `(-xs[0]) == 252` completed for a candidate returning `4 as u8`.
    #[test]
    fn operator_unary_arithmetic_never_runs_at_a_width_the_candidate_chose() {
        let run = |suite: &str, cand: &str| judged_on("r9-unary", suite, cand);
        let t = |body: &str| {
            format!("@[test]\nfn t() {{\n    let xs = arr_map([1], work)\n{body}\n}}\n")
        };
        let neg = t("    assert((-xs[0]) == 252)");
        let not = t("    assert((~xs[0]) == 251)");
        // Controls: an i64 result is plain arithmetic (-4 != 252: the check is live);
        // a width the OPERATOR chose is determined and wraps as written.
        assert!(
            run(&neg, "fn work(x: i64) -> i64 { 4 }\n").is_err(),
            "control: i64 -4 != 252"
        );
        let own = "@[test]\nfn t() {\n    let w = as_u8(5)\n    assert((-w) == 251)\n    assert((~w) == 250)\n}\n";
        assert_eq!(
            run(own, "fn work(x: i64) -> i64 { 4 }\n"),
            Ok(TestEnd::Completed),
            "control: the operator's own u8"
        );
        let ok_i64 = t("    assert((-xs[0]) == -4)");
        assert_eq!(
            run(&ok_i64, "fn work(x: i64) -> i64 { 4 }\n"),
            Ok(TestEnd::Completed),
            "control: honest i64"
        );
        for (what, suite) in [("negation", &neg), ("bitwise not", &not)] {
            let out = run(suite, "fn work(x: i64) -> u8 { narrow(4) }\n");
            assert!(
                out != Ok(TestEnd::Completed)
                    && matches!(&out, Err(m) if m.contains("did arithmetic on a fixed-width integer")),
                "ATTACK: the operator's unary {what} wrapped at a width the candidate chose: {out:?}"
            );
        }
    }

    /// C9 round 9 sweep, CLOSED routes: each carries a value the candidate
    /// produced into operator code, and none lets the candidate choose the type
    /// the operator's dispatch or arithmetic runs at. A `Refused` route fails
    /// with the guard that closes it; a `Fresh` one hands over a value the
    /// builtin builds itself (so `.ok()` lands on the `i64` impl).
    #[test]
    fn the_sweep_routes_stay_closed() {
        let t = |body: &str| format!("@[test]\nfn t() {{\n{body}\n}}\n");
        let refused = |what: &str, suite: String, cand: &str, needle: &str| {
            let out = judged_on("r9-sweep", &suite, cand);
            assert!(
                out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains(needle)),
                "ATTACK: {what} reached the operator as a type the candidate chose: {out:?}"
            );
        };
        let nd = "nothing on the operator side determined";
        let tc = "type confusion";
        let u8w = "fn work(x: i64) -> u8 { narrow(4) }\n";
        refused(
            "an arr_map result",
            t("    let xs = arr_map([1, 2], work)\n    assert(xs[0].ok())"),
            u8w,
            nd,
        );
        refused(
            "an arr_map result (pinned)",
            t("    let xs = arr_map([1, 2], work)\n    let y: i64 = xs[0]\n    assert(y.ok())"),
            u8w,
            tc,
        );
        refused(
            "an arr_fold result",
            t("    let r = arr_fold([1], 0, work)\n    assert(r.ok())"),
            "fn work(a: i64, x: i64) -> u8 { narrow(4) }\n",
            nd,
        );
        refused(
            "an arr_zip_with result",
            t("    let r = arr_zip_with([1], [2], work)\n    assert(r[0].ok())"),
            "fn work(a: i64, x: i64) -> u8 { narrow(4) }\n",
            nd,
        );
        refused(
            "an arr_max_by element",
            t("    let r = arr_max_by([1, 2], work)\n    assert(r.ok())"),
            "fn work(a: i64) -> f64 { 1.0 }\n",
            nd,
        );
        refused("a dict_map_values entry", t("    let d = dict_new()\n    dict_set(d, \"a\", 1)\n    let m = dict_map_values(d, work)\n    match dict_get(m, \"a\") { Some(v) => assert(v.ok())  None => assert(false) }"), u8w, nd);
        refused(
            "an arr_sort_by comparator",
            t("    let r = arr_sort_by([2, 1], work)\n    assert(r[0].ok())"),
            "fn work(a: i64, b: i64) -> u8 { narrow(4) }\n",
            "comparator must return i64",
        );
        refused(
            "an arr_sum_by key",
            t("    let r = arr_sum_by([2, 1], work)\n    assert(r.ok())"),
            "fn work(a: i64) -> u8 { narrow(4) }\n",
            "must return a number",
        );
        refused("a candidate lambda handed back as a fn type", "fn run(f: fn(i64) -> i64) -> i64 { f(1) }\n@[test]\nfn t() {\n    let r = run(mk())\n    assert(r.ok())\n}\n".to_string(), "fn mk() -> fn(i64) -> i64 { |x: i64| narrow(4) }\n", tc);
        refused("a candidate channel", t("    let c = mk()\n    match c.recv() { Some(v) => assert(v.ok())  None => assert(false) }"), "fn mk() -> Chan<i64> {\n    let c = Chan::new(2)\n    c.send(narrow(4))\n    c\n}\n", tc);
        refused("a forged Uncertain", t("    let u = mk()\n    assert(u.value.ok())"), "fn mk() -> Uncertain<i64> { Uncertain { value: narrow(4), confidence: 0.5, source_tag: 0 } }\n", tc);
        refused(
            "a forged Uncertain returned as i64",
            t("    let u = mk()\n    assert(u.ok())"),
            "fn mk() -> i64 { Uncertain { value: narrow(4), confidence: 0.5, source_tag: 0 } }\n",
            tc,
        );
        refused(
            "an Option element",
            t("    match mk() { Some(v) => assert(v.ok())  None => assert(false) }"),
            "fn mk() -> Option<i64> { Some(narrow(4)) }\n",
            tc,
        );
        refused(
            "an array element",
            t("    let a = mk()\n    assert(a[0].ok())"),
            "fn mk() -> [i64] { [narrow(4)] }\n",
            tc,
        );
        refused(
            "a candidate global array",
            t("    assert(TBL[0].ok())"),
            "let TBL = [narrow(4)]\nfn mk() -> i64 { 1 }\n",
            nd,
        );
        refused(
            "a candidate global tuple",
            t("    assert(TP.0.ok())"),
            "let TP = (narrow(4), 1)\nfn mk() -> i64 { 1 }\n",
            nd,
        );
        refused(
            "a candidate global's struct field",
            t("    assert(CF.k.ok())"),
            "type Cf = { k: i64 }\nlet CF = Cf { k: narrow(4) }\nfn mk() -> i64 { 1 }\n",
            tc,
        );
        refused("a candidate impl of an operator trait method", "trait Val {\n    fn val(self) -> i64\n}\n@[test]\nfn t() {\n    let m = mk()\n    let r = m.val()\n    assert(r.ok())\n}\n".to_string(), "type Mine = { a: i64 }\nimpl Val for Mine {\n    fn val(self: Mine) -> u8 { narrow(4) }\n}\nfn mk() -> Mine { Mine { a: 1 } }\n", "a method the operator defines");
        for (what, body) in [
            ("a let copy", "    let a = xs[0]\n    assert(a.ok())"),
            ("a tuple pattern", "    let (a, b) = (xs[0], 1)\n    assert(a.ok())"),
            ("a match binding", "    match xs[0] { v => assert(v.ok()) }"),
            ("a Some payload", "    let o = Some(xs[0])\n    match o { Some(v) => assert(v.ok())  None => assert(false) }"),
            ("a lambda parameter", "    let f = |v| v.ok()\n    assert(f(xs[0]))"),
            ("an if branch", "    let a = if true { xs[0] } else { 1 }\n    assert(a.ok())"),
            ("a block tail", "    let a = { let b = xs[0]\n b }\n    assert(a.ok())"),
            ("a reassignment", "    let mut a = 1\n    a = xs[0]\n    assert(a.ok())"),
            ("an array literal", "    let ys = [xs[0], xs[0]]\n    assert(ys[1].ok())"),
            ("a for variable", "    for x in xs { assert(x.ok()) }"),
        ] {
            refused(what, t(&format!("    let xs = arr_map([1], work)\n{body}")), u8w, nd);
        }
        // FRESH: the builtin builds the value, so the i64 impl is the one reached.
        let fresh = |what: &str, suite: String, cand: &str| {
            let out = judged_on("r9-sweep", &suite, cand);
            assert_eq!(
                out,
                Ok(TestEnd::Completed),
                "{what} must hand the operator a fresh i64: {out:?}"
            );
        };
        fresh("scheduler_result", t("    let id = scheduler_spawn(\"work\", 0)\n    let n = scheduler_run()\n    let r = scheduler_result(id)\n    assert(!r.ok())"), "fn work(x: i64) -> u8 { narrow(4) }\n");
        fresh("goal_best_input", t("    let r0 = goal_run(\"work\", 100.0, 3)\n    let r = goal_best_input(\"work\", 100.0)\n    assert(!r.ok())"), "@[adaptive]\nfn work(x: i64) -> i64 { 4 }\n");
    }

    /// C9 round 8 (sentinel blocker): a top-level fn named in VALUE position is
    /// a first-class closure. Made by a SEALED frame from an OPERATOR fn it was
    /// a forwarding body that ran in an operator frame, where the call edge no
    /// longer applied (`let g = secret; g()` ran the operator's `secret`; a
    /// global `SECRET` leaked through `let g = peek`). The edge now applies at
    /// creation.
    #[test]
    fn a_sealed_frame_cannot_take_an_operator_fn_as_a_value() {
        let suite = "fn secret() -> i64 { 9 }\nlet SECRET = 41\nfn peek() -> i64 { SECRET }\n\
                     fn id_of(g: fn() -> i64) -> i64 { g() }\n\
                     @[test]\nfn t() { assert_eq(solve(), 9) }\n\
                     @[test]\nfn t_leak() { assert_eq(solve(), 41) }\n\
                     @[test]\nfn t_pass() { assert_eq(take(secret), 9) }\n\
                     @[test]\nfn t_back() { let g = give()\n    assert_eq(g(), 9) }\n\
                     @[test]\nfn t_arg() { assert_eq(solve(), 9) }\n";
        let run = |cand: &str, t: &str| sealed_outcome_rule("r8-fnval", suite, cand, t, true);
        // Controls: the candidate names ITS OWN fn as a value and calls it; the
        // operator names ITS OWN fn as a value and hands it to the candidate;
        // a candidate fn value returned to the operator runs.
        let own = "fn mine() -> i64 { 9 }\nfn solve() -> i64 {\n    let g = mine\n    g()\n}\n\
                   fn take(f: fn() -> i64) -> i64 { f() }\nfn give() -> fn() -> i64 { mine }\n";
        for t in ["t", "t_pass", "t_back"] {
            assert_eq!(run(own, t), Ok(TestEnd::Completed), "control: {t}");
        }
        let wrong = own.replace("fn mine() -> i64 { 9 }", "fn mine() -> i64 { 4 }");
        assert!(run(&wrong, "t").is_err(), "control: wrong fails");
        // The attacks.
        let rest = "fn take(f: fn() -> i64) -> i64 { f() }\nfn give() -> fn() -> i64 { secret }\n";
        for (t, what, solve) in [
            ("t", "ran the operator's `secret` through a fn value", "fn solve() -> i64 {\n    let g = secret\n    g()\n}\n"),
            ("t_leak", "read the operator's global through a fn value", "fn solve() -> i64 {\n    let g = peek\n    g()\n}\n"),
            ("t", "stored the operator's fn in an array", "fn solve() -> i64 {\n    let gs = [secret]\n    gs[0]()\n}\n"),
            ("t", "stored the operator's fn in a dict", "fn solve() -> i64 {\n    let d = dict_new()\n    dict_set(d, \"f\", secret)\n    match dict_get(d, \"f\") { Some(f) => f()  None => 0 }\n}\n"),
            ("t", "handed the operator's fn to its own helper", "fn run(f: fn() -> i64) -> i64 { f() }\nfn solve() -> i64 { run(secret) }\n"),
        ] {
            let out = run(&format!("{rest}{solve}"), t);
            assert!(
                out != Ok(TestEnd::Completed)
                    && matches!(&out, Err(m) if m.contains("cannot use")),
                "ATTACK: a sealed frame took an operator fn as a value ({what}): {out:?}"
            );
        }
        // Returned to the operator, which calls it: the creation is refused.
        let out = run("fn solve() -> i64 { 0 }\nfn take(f: fn() -> i64) -> i64 { f() }\nfn give() -> fn() -> i64 { secret }\n", "t_back");
        assert!(
            out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains("cannot use")),
            "ATTACK: the candidate returned the operator's fn as a value: {out:?}"
        );
    }

    /// A candidate fn's value, called by the OPERATOR, still reaches only the
    /// candidate's fn, which cannot reach an operator fn (a regression control:
    /// no guard of its own, the call edge in `call_fn_sealed` refuses).
    #[test]
    fn a_candidate_fn_value_the_operator_calls_still_cannot_reach_operator_fns() {
        let suite =
            "fn secret() -> i64 { 9 }\n@[test]\nfn t() { let g = give()\n    assert_eq(g(), 9) }\n";
        // The candidate's own fn calls the operator's `secret`: refused whether the
        // call is direct or through the value the operator invokes.
        let out = sealed_outcome_rule(
            "r8-fnval2",
            suite,
            "fn mine() -> i64 { secret() }\nfn give() -> fn() -> i64 { mine }\n",
            "t",
            true,
        );
        assert!(
            out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains("cannot use")),
            "ATTACK: the operator's call of a candidate fn value ran operator code in the candidate's name: {out:?}"
        );
    }

    /// d1 (round 6 blocker): a candidate closure that captured the operator's
    /// dict retypes an entry AFTER the operator changed it (the snapshot was
    /// from the hand-over). Refused by the dict layer (rule off) AND by the
    /// dispatch rule (rule on); the pinned suite refuses it by the cast.
    #[test]
    fn a_closure_that_captured_the_operators_dict_cannot_retype_after_an_operator_write() {
        let suite = "@[test]\nfn t() {\n    let d = dict_new()\n    let f = make(d)\n    dict_set(d, \"answer\", 3)\n    f()\n    match dict_get(d, \"answer\") {\n        Some(v) => assert(v.ok())\n        None => assert(false)\n    }\n}\n";
        let cand = |v: &str| {
            format!("fn make(d: Dict) -> fn() -> i64 {{\n    || {{\n        dict_set(d, \"answer\", {v})\n        0\n    }}\n}}\n")
        };
        let out = judged8(
            "r6-stale",
            suite,
            &format!("{LAUNDER8}{}", cand("narrow(4)")),
        );
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("retyped a dict entry")),
            "ATTACK: a captured-dict closure retyped an entry against a stale snapshot: {out:?}"
        );
        // With the rule on too, it is refused (by whichever layer meets it first).
        let on = judged_on("r6-stale", suite, &cand("narrow(4)"));
        assert!(
            matches!(&on, Err(m) if m.contains("retyped a dict entry") || m.contains("whose type nothing on the operator side determined")),
            "ATTACK: the captured-dict closure's retype passed with the dispatch rule on: {on:?}"
        );
        honest8("r6-stale", suite, &cand("9"));
    }

    /// The rest of the container family the rule must cover: a payload, a
    /// struct field, a tuple element, a generic enum, a channel receive, a
    /// closure result, a `match` on the untyped value before the dispatch.
    #[test]
    fn operator_code_never_dispatches_on_a_value_from_any_untyped_position() {
        let wrap = "type Wrap<T> = { v: T }\ntype Opt<T> = Has { v: T } | Nada\n";
        let stash_in = |what: &str| format!("fn solve(d: Dict) {{ dict_set(d, \"k\", {what}) }}\n");
        let cases: Vec<(&str, String, String)> = vec![
            (
                "an Option payload read from a dict",
                "let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(o) => match o {\n            Some(v) => assert(v.ok())\n            None => assert(false)\n        }\n        None => assert(false)\n    }".into(),
                stash_in("Some(narrow(4))"),
            ),
            (
                "a struct field of a dict value",
                "let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(w) => assert(w.v.ok())\n        None => assert(false)\n    }".into(),
                format!("{wrap}{}", stash_in("Wrap { v: narrow(4) }")),
            ),
            (
                "a tuple element of a dict value",
                "let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(t) => assert(t.0.ok())\n        None => assert(false)\n    }".into(),
                stash_in("(narrow(4), 1)"),
            ),
            (
                "a generic enum payload",
                "let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(e) => match e {\n            Opt::Has { v } => assert(v.ok())\n            Opt::Nada => assert(false)\n        }\n        None => assert(false)\n    }".into(),
                format!("{wrap}{}", stash_in("Opt::Has { v: narrow(4) }")),
            ),
            (
                "a channel receive",
                "let c = chan<i64>()\n    c.send(3)\n    fill(c)\n    assert(c.recv().ok())".into(),
                "fn fill(c: Chan<i64>) { c.send(9) }\n".into(),
            ),
            (
                "an unannotated lambda's result",
                "let f = |x| x\n    assert(f(3).ok())".into(),
                "fn unused() {}\n".into(),
            ),
            (
                "an unannotated lambda parameter",
                "let f = |x| assert(x.ok())\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => f(v)\n        None => assert(false)\n    }".into(),
                stash_in("narrow(4)"),
            ),
            (
                "a match on the untyped value before the dispatch",
                "let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => match v {\n            w => assert(w.ok())\n        }\n        None => assert(false)\n    }".into(),
                stash_in("narrow(4)"),
            ),
        ];
        for (why, body, cand) in cases {
            let suite = format!("@[test]\nfn t() {{\n    {body}\n}}\n");
            // The wrap/enum types are the candidate's here only for the dict
            // cases; the unannotated-lambda and channel cases need no attack.
            dispatch_refused(why, &suite, &cand);
        }
    }

    /// What stays unaffected: every PINNED receiver dispatches, a method with
    /// no impl to choose between dispatches on anything, and outside a sealed
    /// run nothing changes.
    #[test]
    fn a_determined_receiver_dispatches_and_so_does_an_unambiguous_method() {
        let pinned = [
            "let r: i64 = solve(3)\n    assert(r.ok())",
            "assert(9.ok())",
            "let w = Wrap { v: 9 }\n    assert(w.v.ok())",
            "let xs = [9]\n    assert(xs[0].ok())",
            "let r: i64 = solve(3)\n    assert((r + 0).ok())",
            "assert((solve(3) as i64).ok())",
        ];
        for body in pinned {
            let suite = format!("@[test]\nfn t() {{\n    {body}\n}}\n");
            let out = judged_on(
                "r6-pin",
                &format!("type Wrap<T> = {{ v: T }}\n{suite}"),
                "fn solve(n: i64) -> i64 { n * n }\n",
            );
            assert_eq!(
                out,
                Ok(TestEnd::Completed),
                "control (pinned): {body}: {out:?}"
            );
        }
        // One impl type only: nothing to choose between.
        let one = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\n@[test]\nfn t() {\n    let d = dict_new()\n    dict_set(d, \"k\", solve(3))\n    match dict_get(d, \"k\") {\n        Some(v) => assert(v.ok())\n        None => assert(false)\n    }\n}\n";
        let out = sealed_outcome_rule(
            "r6-one",
            one,
            "fn solve(n: i64) -> i64 { n * n }\n",
            "t",
            true,
        );
        assert_eq!(
            out,
            Ok(TestEnd::Completed),
            "control (a single impl): {out:?}"
        );
        // Outside a sealed run the same program is unchanged.
        let src = format!("{JUDGE8}@[test]\nfn t() {{\n    let d = dict_new()\n    dict_set(d, \"k\", 9)\n    match dict_get(d, \"k\") {{\n        Some(v) => assert(v.ok())\n        None => assert(false)\n    }}\n}}\n");
        let prog = crate::parse_source(&src).expect("parses");
        let _g = SEALED_DIRS_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        crate::resolver::set_sealed_module_dirs(&[]);
        assert_eq!(run_test_fn_outcome(&prog, "t"), Ok(TestEnd::Completed));
    }

    // ── C9 round 7 (amendment 88): the analysis must only trust what the OPERATOR chose ──

    /// A suite whose receiver is `read`, with the operator's `Judge` and a
    /// candidate that supplies `cand`; GOOD/WRONG are checked for pinned
    /// shapes by the callers.
    fn r7_refused(why: &str, suite: &str, cand: &str) {
        let out = judged_on("r7", suite, cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("whose type nothing on the operator side determined")
                    || m.contains("whose width nothing on the operator side determined")
                    || m.contains("runtime type confusion")),
            "ATTACK: the operator's analysis trusted a type the candidate chose ({why}): {out:?}"
        );
    }

    /// B1: a type the CANDIDATE declared pins nothing — its fn's declared
    /// return, a type it defines, a candidate method's name.
    #[test]
    fn a_type_the_candidate_declared_does_not_determine_the_receiver() {
        let t = |body: &str| format!("@[test]\nfn t() {{\n    {body}\n}}\n");
        r7_refused(
            "a candidate fn declared -> u8",
            &t("assert(solve(3).ok())"),
            "fn solve(n: i64) -> u8 { 255 as u8 }\n",
        );
        r7_refused(
            "the same through an unannotated let",
            &t("let r = solve(3)\n    assert(r.ok())"),
            "fn solve(n: i64) -> u8 { 255 as u8 }\n",
        );
        r7_refused(
            "a candidate-defined struct's u8 field",
            &t("let p: P = solve()\n    assert(p.x.ok())"),
            "type P = { x: u8 }\nfn solve() -> P { P { x: 4 as u8 } }\n",
        );
        r7_refused(
            "arithmetic on a candidate fn declared -> u8",
            &t("assert((solve(3) << 1) == 254)"),
            "fn solve(n: i64) -> u8 { 255 as u8 }\n",
        );
        // The operator's own pin on the SAME call still determines: the cast
        // to the operator's type is the operator's choice.
        let pinned = t("let r: i64 = solve(3)\n    assert(r.ok())");
        assert_eq!(
            judged_on("r7", &pinned, "fn solve(n: i64) -> i64 { n * n }\n"),
            Ok(TestEnd::Completed)
        );
        assert!(judged_on("r7", &pinned, "fn solve(n: i64) -> i64 { n + 1 }\n").is_err());
        r7_refused(
            "a u8 at the i64 pin",
            &pinned,
            "fn solve(n: i64) -> u8 { 255 as u8 }\n",
        );
    }

    /// B2: a LOCAL binding named like an operator fn is not that fn.
    #[test]
    fn a_local_binding_named_like_an_operator_fn_is_not_judged_by_it() {
        let suite = "fn f() -> i64 { 5 }\n@[test]\nfn t() {\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"f\") {\n        Some(f) => assert(f().ok())\n        None => assert(false)\n    }\n}\n";
        r7_refused(
            "a closure stored under a new key, called through a local named f",
            suite,
            "fn solve(d: Dict) { dict_set(d, \"f\", || 4 as u8) }\n",
        );
    }

    /// B3: a trait name, `dyn`, `Option<Trait>`, `[Trait]` and a fn type
    /// returning one do not constrain the runtime type.
    #[test]
    fn a_trait_annotation_does_not_pin_the_runtime_type() {
        let cand = "fn solve(d: Dict) { dict_set(d, \"k\", 4 as u8) }\n";
        let read = |ann: &str, use_: &str| {
            format!("@[test]\nfn t() {{\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {{\n        Some(v) => {{\n            let y: {ann} = v\n            {use_}\n        }}\n        None => assert(false)\n    }}\n}}\n")
        };
        r7_refused("a trait name", &read("Judge", "assert(y.ok())"), cand);
        r7_refused("dyn Judge", &read("dyn Judge", "assert(y.ok())"), cand);
        let idj = "fn idj(x: Judge) -> Judge { x }\n@[test]\nfn t() {\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => {\n            let y = idj(v)\n            assert(y.ok())\n        }\n        None => assert(false)\n    }\n}\n";
        r7_refused("a trait-typed fn and return", idj, cand);
        r7_refused(
            "Option<Judge>",
            &read(
                "Option<Judge>",
                "match y { Some(z) => assert(z.ok())  None => assert(false) }",
            ),
            "fn solve(d: Dict) { dict_set(d, \"k\", Some(4 as u8)) }\n",
        );
    }

    /// Adjacent (a)/(b): a verdict belongs to its own fn, and a candidate's
    /// methods never steer an operator site.
    #[test]
    fn a_verdict_belongs_to_its_own_fn_and_the_candidate_steers_none() {
        // An unrelated, never-called fn with `|p| p.ok()` does not refuse an
        // honest fully annotated `check(p: i64)`.
        let suite = "fn check(p: i64) -> bool { p.ok() }\nfn unrelated() { let f = |p| p.ok()\n    f(1) }\n@[test]\nfn t() {\n    let r: i64 = solve(3)\n    assert(check(r))\n}\n";
        let out = judged_on("r7", suite, "fn solve(n: i64) -> i64 { n * n }\n");
        assert_eq!(out, Ok(TestEnd::Completed), "control: {out:?}");
        // A candidate method named like an operator method with an open return
        // does not flip the operator's determined `let r = b.pick()`.
        let suite = "type B = { v: i64 }\ntrait Pick {\n    fn pick(self) -> i64\n}\nimpl Pick for B {\n    fn pick(self: B) -> i64 { self.v }\n}\n@[test]\nfn t() {\n    let b = B { v: 9 }\n    let r = b.pick()\n    assert(r.ok())\n}\n";
        let out = judged_on(
            "r7",
            suite,
            "type CB = { w: i64 }\nfn pick<T>(self: CB) -> T { self.w }\n",
        );
        assert_eq!(
            out,
            Ok(TestEnd::Completed),
            "control: a candidate method steers nothing: {out:?}"
        );
    }

    /// Adjacent (d): operator-only polymorphic helpers pass (the receiver is
    /// an operator struct), a candidate-influenced one is refused.
    #[test]
    fn operator_only_polymorphism_dispatches_and_a_candidate_influenced_one_does_not() {
        let shapes = "trait Shape {\n    fn area(self) -> i64\n}\ntype Sq = { s: i64 }\ntype Rect = { w: i64, h: i64 }\nimpl Shape for Sq {\n    fn area(self: Sq) -> i64 { self.s * self.s }\n}\nimpl Shape for Rect {\n    fn area(self: Rect) -> i64 { self.w * self.h }\n}\nimpl Shape for u8 {\n    fn area(self: u8) -> i64 { 0 }\n}\n";
        for helper in [
            "fn total(a: dyn Shape, b: dyn Shape) -> i64 { a.area() + b.area() }",
            "fn total<T: Shape, U: Shape>(a: T, b: U) -> i64 { a.area() + b.area() }",
        ] {
            let suite = format!("{shapes}{helper}\n@[test]\nfn t() {{\n    let r: i64 = solve(1)\n    assert_eq(total(Sq {{ s: 2 }}, Rect {{ w: 2, h: 3 }}), r)\n}}\n");
            let good = judged_on("r7", &suite, "fn solve(n: i64) -> i64 { 10 }\n");
            assert_eq!(
                good,
                Ok(TestEnd::Completed),
                "control (honest): {helper}: {good:?}"
            );
            assert!(judged_on("r7", &suite, "fn solve(n: i64) -> i64 { 11 }\n").is_err());
        }
        // The candidate supplies a u8 to the generic helper.
        let suite = format!("{shapes}fn one<T: Shape>(a: T) -> i64 {{ a.area() }}\n@[test]\nfn t() {{\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {{\n        Some(v) => assert_eq(one(v), 0)\n        None => assert(false)\n    }}\n}}\n");
        r7_refused(
            "a candidate u8 handed to a generic helper",
            &suite,
            "fn solve(d: Dict) { dict_set(d, \"k\", 4 as u8) }\n",
        );
    }

    /// Adjacent (e): an operator method named `recv` does not make a channel
    /// read look determined.
    #[test]
    fn an_operator_method_named_like_a_channel_method_does_not_determine_a_recv() {
        // The candidate's own unstamped channel, read through the operator's dict.
        let dsuite = "trait R {\n    fn recv(self) -> i64\n}\nimpl R for bool {\n    fn recv(self: bool) -> i64 { 1 }\n}\nimpl R for i64 {\n    fn recv(self: i64) -> i64 { 2 }\n}\n".to_string()
            + &format!("{JUDGE8}@[test]\nfn t() {{\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"c\") {{\n        Some(c) => assert(c.recv().ok())\n        None => assert(false)\n    }}\n}}\n");
        let out = sealed_outcome_rule(
            "r7",
            &dsuite,
            &format!("{LAUNDER8}fn solve(d: Dict) {{\n    let c = Chan::new(2)\n    c.send(narrow(4))\n    dict_set(d, \"c\", c)\n}}\n"),
            "t",
            true,
        );
        assert!(
            matches!(&out, Err(m) if m.contains("nothing on the operator side determined")),
            "ATTACK: an operator method named recv made a dict-read channel's value look determined: {out:?}"
        );
    }

    /// A candidate's module-level `let` is the candidate's value; the verdict
    /// of one fn's site never decides another's (the key carries the owner).
    #[test]
    fn a_candidates_global_and_a_sibling_fns_site_determine_nothing() {
        r7_refused(
            "a candidate global `let X = 4 as u8`",
            "@[test]\nfn t() {\n    assert(X.ok())\n}\n",
            "let X = 4 as u8\n",
        );
        let suite = "fn check(p: i64) -> bool { p.ok() }\n@[test]\nfn t() {\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => {\n            let f = |p| p.ok()\n            assert(f(v))\n        }\n        None => assert(false)\n    }\n}\n";
        r7_refused(
            "the same receiver text as a sibling fn's pinned one",
            suite,
            "fn solve(d: Dict) { dict_set(d, \"k\", 4 as u8) }\n",
        );
    }

    /// What is NOT a dispatch, so the rule leaves it alone: interpolation and
    /// `to_str` of an untyped read, and a comparison — none selects an
    /// operator impl (the language has no operator overloading and no trait
    /// default methods), so the candidate's type choice picks no operator code.
    #[test]
    fn interpolation_and_comparison_of_an_untyped_read_select_no_operator_impl() {
        let suite = "@[test]\nfn t() {\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => {\n            assert(\"{v}\" == \"9\")\n            assert(to_str(v) == \"9\")\n            assert(v == 9)\n        }\n        None => assert(false)\n    }\n}\n";
        let out = judged_on(
            "r6-nd",
            suite,
            "fn solve(d: Dict) { dict_set(d, \"k\", 9) }\n",
        );
        assert_eq!(out, Ok(TestEnd::Completed), "control: {out:?}");
        assert!(judged_on(
            "r6-nd",
            suite,
            "fn solve(d: Dict) { dict_set(d, \"k\", 4) }\n"
        )
        .is_err());
    }

    /// The arithmetic arm: the candidate stores `255 as u8` and the operator's
    /// untyped `v << 1 == 254` truncates to a pass (an `i64` 255 << 1 is 510;
    /// plain `+`/`*` PANIC on overflow, so the shift is the width-dependent
    /// result that completes).
    #[test]
    fn operator_arithmetic_never_runs_at_a_width_the_candidate_chose() {
        let suite = |read: &str| {
            format!("@[test]\nfn t() {{\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {{\n        Some(v) => {read}\n        None => assert(false)\n    }}\n}}\n")
        };
        let cand = |v: &str| format!("fn solve(d: Dict) {{ dict_set(d, \"k\", {v}) }}\n");
        let out = judged_on(
            "r6-arith",
            &suite("assert((v << 1) == 254)"),
            &cand("255 as u8"),
        );
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("whose width nothing on the operator side determined")),
            "ATTACK: a u8 the candidate chose truncated the operator's arithmetic into a pass: {out:?}"
        );
        let pinned = suite("{ let y: i64 = v\n            assert((y << 1) == 254) }");
        assert_eq!(
            judged_on("r6-arith", &pinned, &cand("127")),
            Ok(TestEnd::Completed),
            "control: GOOD, pinned"
        );
        assert!(
            judged_on("r6-arith", &pinned, &cand("5")).is_err(),
            "control: WRONG fails"
        );
        let out = judged_on("r6-arith", &pinned, &cand("255 as u8"));
        assert!(
            out != Ok(TestEnd::Completed) && confusion_refused(&out),
            "ATTACK: a u8 passed the suite's `let y: i64` pin: {out:?}"
        );
    }

    /// A dict too big to snapshot is REFUSED at the crossing, never skipped.
    #[test]
    fn a_dict_over_the_snapshot_bound_is_refused_not_skipped() {
        let suite = "@[test]\nfn t() {\n    let d = dict_new()\n    let i = 0\n    while i <= 1000000 {\n        dict_set(d, to_str(i), i)\n        i = i + 1\n    }\n    solve(d)\n    assert(true)\n}\n";
        let out = judged8("r4c-dict", suite, "fn solve(d: Dict) { }\n");
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("cannot cross a seal")),
            "ATTACK: a dict past the snapshot bound crossed unrecorded: {out:?}"
        );
    }

    // ---- C9 round 10, PSV1H (amendment 100) ----

    const ARM_PRE: &str = "fn v_one() -> i64 { 1 }\n";
    const ARM_OVERFLOW: &str =
        "arr_sum_i64([9223372036854775807, 1]) + 0 + (9223372036854775807 + v_one())";

    fn arm_suites() -> (String, String, String, String, String) {
        let colliding = "fn helper() -> i64 {\n    let v: i64 = 9\n    println(\"p\")\n    if v.ok() { 1 } else { 0 }\n}\n";
        let plain = "fn helper() -> i64 {\n    println(\"p\")\n    1\n}\n";
        let dispatch = |helper: &str, bind: &str| {
            format!(
            "{ARM_PRE}{helper}@[test]\nfn t() {{\n  {bind}\n  let r = with handler {{ on IO(p) => resume(if v.ok() {{ 0 }} else {{ {ARM_OVERFLOW} }}) }} {{ helper() }}\n  assert(r == 1)\n}}\n"
        )
        };
        let general = format!(
            "{ARM_PRE}{colliding}@[test]\nfn t() {{\n  let v = work(0)\n  let r = with handler {{ on IO(p) => {{\n      let k = resume(0)\n      if v.ok() {{ k }} else {{ {ARM_OVERFLOW} }}\n  }} }} {{ helper() }}\n  assert(r == 1)\n}}\n"
        );
        let width = format!(
            "{ARM_PRE}fn helper() -> i64 {{\n    let v: i64 = 255\n    println(\"p\")\n    let w = v << 1\n    w\n}}\n@[test]\nfn t() {{\n  let v = work(0)\n  let r = with handler {{ on IO(p) => resume(if (v << 1) == 254 {{ 0 }} else {{ {ARM_OVERFLOW} }}) }} {{ helper() }}\n  assert(r == 510)\n}}\n"
        );
        (
            dispatch(colliding, "let v: i64 = work(0)"),
            dispatch(colliding, "let v = work(0)"),
            dispatch(plain, "let v = work(0)"),
            general,
            width,
        )
    }

    /// BLOCKER A. A handler arm runs under the pin owner of the fn that INSTALLED
    /// it. Before, it ran under the owner of the fn that PERFORMED the effect, so
    /// a performer holding an identical, determined site text (`let v: i64 = 9
    /// ... v.ok()`) lent its verdict to the arm's undetermined `v.ok()`.
    fn arm_attack(what: &str, suite: &str, cand: &str, msg: &str) {
        let out = judged_on("r10-arm", suite, cand);
        assert!(
            out != Ok(TestEnd::Completed) && matches!(&out, Err(m) if m.contains(msg)),
            "ATTACK: a handler arm borrowed the performer's pin verdict ({what}): {out:?}"
        );
    }

    const U8_CAND: &str = "fn work(x: i64) -> u8 { narrow(4) }\n";
    const DISP_MSG: &str = "whose type nothing on the operator side determined";

    #[test]
    fn a_handler_arm_dispatch_runs_under_the_installers_pin_owner() {
        let (pinned, colliding, plain, _, _) = arm_suites();
        let run = |suite: &str, cand: &str| judged_on("r10-arm", suite, cand);
        // Controls: honest pinned i64 9 passes, the wrong i64 fails, and without a
        // colliding site the u8 is refused by its OWN site.
        assert_eq!(
            run(&pinned, "fn work(x: i64) -> i64 { 9 }\n"),
            Ok(TestEnd::Completed),
            "control: honest pinned"
        );
        assert!(
            run(&pinned, "fn work(x: i64) -> i64 { 4 }\n").is_err(),
            "control: wrong i64 fails"
        );
        let out = run(&plain, U8_CAND);
        assert!(
            matches!(&out, Err(m) if m.contains(DISP_MSG)),
            "control: no colliding site: {out:?}"
        );
        arm_attack(
            "colliding site, dispatch arm",
            &colliding,
            U8_CAND,
            DISP_MSG,
        );
    }

    #[test]
    fn a_handler_arm_replay_runs_under_the_installers_pin_owner() {
        let (_, _, _, general, _) = arm_suites();
        arm_attack(
            "colliding site, general replay arm",
            &general,
            U8_CAND,
            DISP_MSG,
        );
    }

    /// A closure made INSIDE an arm remembers the owner the arm ran under
    /// (`PIN_FN_MARK`), and so is judged by the installing fn too.
    #[test]
    fn a_closure_made_in_a_handler_arm_is_judged_by_the_installing_fn() {
        let colliding = "fn helper() -> i64 {\n    let v: i64 = 9\n    println(\"p\")\n    if v.ok() { 1 } else { 0 }\n}\n";
        let suite = |bind: &str| {
            format!(
            "{ARM_PRE}{colliding}@[test]\nfn t() {{\n  {bind}\n  let r = with handler {{ on IO(p) => {{\n      let f = |a: i64| if v.ok() {{ 0 }} else {{ {ARM_OVERFLOW} }}\n      let k = resume(f(1))\n      k\n  }} }} {{ helper() }}\n  assert(r == 1)\n}}\n"
        )
        };
        assert_eq!(
            judged_on(
                "r10-arm",
                &suite("let v: i64 = work(0)"),
                "fn work(x: i64) -> i64 { 9 }\n"
            ),
            Ok(TestEnd::Completed),
            "control: honest pinned"
        );
        arm_attack(
            "colliding site, closure made in an arm",
            &suite("let v = work(0)"),
            U8_CAND,
            DISP_MSG,
        );
    }

    #[test]
    fn a_handler_arm_arithmetic_runs_under_the_installers_pin_owner() {
        let (_, _, _, _, width) = arm_suites();
        // Control: the suite discriminates. An i64 255 does not wrap, so the arm
        // takes its overflow branch and the run FAILS (a keyed failure, not a pass).
        let wide = judged_on("r10-arm", &width, "fn work(x: i64) -> i64 { 255 }\n");
        assert!(
            wide.is_err() && !matches!(&wide, Err(m) if m.contains("determined")),
            "control: honest i64 255 fails on the overflow, not on the rule: {wide:?}"
        );
        arm_attack(
            "colliding site, arithmetic arm",
            &width,
            "fn work(x: i64) -> u8 { narrow(255) }\n",
            "whose width nothing on the operator side determined",
        );
    }

    /// The structural half of A: pin verdicts are keyed (owner, site text), so
    /// two fns holding IDENTICAL text must never share one. The analysis keeps
    /// them apart by owner; the interpreter must hand the right owner to every
    /// frame that runs operator code (`every_frame_that_runs_stored_operator_code_sets_its_owner`).
    #[test]
    fn identical_site_text_in_two_fns_gets_two_verdicts() {
        use crate::ast::{walk_expr, Item};
        let src = "trait J { fn ok(self) -> bool }\nimpl J for i64 { fn ok(self: i64) -> bool { self == 9 } }\nimpl J for u8 { fn ok(self: u8) -> bool { true } }\nfn a() -> bool { let v: i64 = 9\n v.ok() }\nfn b() -> bool { let v = dict_get(dict_new(), \"k\")\n v.ok() }\n";
        let prog = crate::parse_source(src).expect("parses");
        let pins = pin::Pins::build(&prog, &|_| false);
        let mut verdicts = Vec::new();
        for it in &prog.items {
            if let Item::FnDef(f) = it {
                let mut site = None;
                walk_expr(&f.body, &mut |e: &Expr| {
                    if let Expr::MethodCall {
                        receiver, method, ..
                    } = e
                    {
                        site = Some((receiver.as_ref().clone(), method.clone()));
                    }
                });
                let (r, m) = site.expect("a method call");
                verdicts.push((
                    f.name.clone(),
                    pins.determined(f as *const FnDef as usize, &r, &m),
                ));
            }
        }
        assert_eq!(
            verdicts,
            vec![("a".to_string(), true), ("b".to_string(), false)]
        );
    }

    fn name_refused(what: &str, suite: &str, cand: &str) {
        let out = judged_on("r10-name", suite, cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("a function name nothing on the operator side determined")),
            "ATTACK: the candidate's string chose the operator fn ({what}): {out:?}"
        );
    }

    /// BLOCKER B. A `str` the candidate returned selects which OPERATOR fn a
    /// name-resolving builtin runs. The name argument is a SINK: operator code
    /// must give it a name-pure expression.
    #[test]
    fn a_function_name_the_candidate_chose_never_selects_an_operator_fn() {
        let sbx = |name_expr: &str, pre: &str| {
            format!(
            "fn reference(x: i64) -> i64 {{ x * 2 }}\n{pre}{}",
            sandbox_suite(&format!("    let got = sandbox_run(sb, {name_expr}, 21)\n    assert(got == reference(21))"))
        )
        };
        let with_local =
            |bind: &str| sbx("nm", "").replace("    let got", &format!("    {bind}\n    let got"));
        let run = |suite: &str, cand: &str| judged_on("r10-name", suite, cand);
        let attack = "fn entry() -> str { \"reference\" }\nfn double(x: i64) -> i64 { 0 }\n";
        let honest = "fn entry() -> str { \"double\" }\nfn double(x: i64) -> i64 { x * 2 }\n";
        // Controls: a literal, an operator-built name, an operator constant and a
        // local bound to one all run the candidate's fn, and a wrong one fails keyed.
        for (what, suite) in [
            ("literal", sbx("\"double\"", "")),
            ("operator fn result", sbx("which()", "fn which() -> str { \"dou\" + \"ble\" }\n")),
            ("operator global", sbx("NAME", "let NAME = \"double\"\n")),
            ("local of a literal", with_local("let nm = \"double\"")),
            ("a branch between two literals", with_local("let nm = if reference(1) == 2 { \"double\" } else { \"reference\" }")),
            ("a loop over a literal range", with_local("let names = [\"nope\", \"double\"]\n    let nm = \"x\"\n    for k in 1..2 { nm = names[k] }")),
            ("an interpolation of literals", with_local("let part = \"ble\"\n    let nm = \"dou{part}\"")),
        ] {
            assert_eq!(run(&suite, honest), Ok(TestEnd::Completed), "control: honest via {what}");
            assert!(
                run(&suite, "fn entry() -> str { \"double\" }\nfn double(x: i64) -> i64 { 0 }\n").is_err(),
                "control: wrong fails via {what}"
            );
        }
        name_refused(
            "sandbox_run, a candidate fn's result",
            &sbx("entry()", ""),
            attack,
        );
        name_refused(
            "a local bound to it",
            &with_local("let nm = entry()"),
            attack,
        );
        name_refused(
            "a local annotated str",
            &with_local("let nm: str = entry()"),
            attack,
        );
        name_refused(
            "a parameter of an operator fn",
            &sbx("pick(entry())", "fn pick(s: str) -> str { s }\n"),
            attack,
        );
        name_refused(
            "an operator fn that returns the candidate's",
            &sbx("pick()", "fn pick() -> str { entry() }\n"),
            attack,
        );
        name_refused(
            "an operator fn that returns it by a return statement",
            &sbx("pick()", "fn pick() -> str {\n    if reference(1) == 2 { return entry() }\n    \"double\"\n}\n"),
            attack,
        );
        name_refused(
            "built from the candidate's",
            &sbx("\"re\" + entry()", ""),
            "fn entry() -> str { \"ference\" }\n",
        );
        name_refused(
            "built from a candidate int through to_str",
            &sbx("\"dou\" + to_str(idx())", ""),
            "fn idx() -> i64 { 1 }\nfn double(x: i64) -> i64 { 0 }\n",
        );
        name_refused(
            "a dict read",
            &with_local("let d = dict_new()\n    dict_set(d, \"k\", entry())\n    let nm = match dict_get(d, \"k\") { Some(s) => s  None => \"\" }"),
            attack,
        );
        name_refused(
            "an index chosen by the candidate",
            &sbx("[\"double\", \"reference\"][idx()]", ""),
            "fn idx() -> i64 { 1 }\nfn double(x: i64) -> i64 { 0 }\n",
        );
        name_refused(
            "a loop variable",
            &sbx("names[i]", "").replace("    let got", "    let names = [\"double\", \"reference\"]\n    let i = 0\n    for k in 0..idx() { i = k }\n    let got"),
            "fn idx() -> i64 { 1 }\nfn double(x: i64) -> i64 { 0 }\n",
        );
    }

    #[test]
    fn scheduler_spawn_takes_no_function_name_the_candidate_chose() {
        let suite = "fn grade(x: i64) -> i64 { 9 }\n@[test]\nfn t() {\n    let id = scheduler_spawn(NAME, 0)\n    let n = scheduler_run()\n    assert(scheduler_result(id) == 9)\n}\n";
        assert_eq!(
            judged_on(
                "r10-name",
                &suite.replace("NAME", "\"grade\""),
                "fn solve() -> i64 { 1 }\n"
            ),
            Ok(TestEnd::Completed),
            "control: a literal names the operator's own fn"
        );
        name_refused(
            "scheduler_spawn",
            &suite.replace("NAME", "name()"),
            "fn name() -> str { \"grade\" }\n",
        );
    }

    #[test]
    fn goal_eval_takes_no_function_name_the_candidate_chose() {
        let suite = "fn easy(x: i64) -> i64 { 100 }\nfn hard(x: i64) -> i64 { 0 }\n@[test]\nfn t() {\n    let s = goal_eval(NAME, 5)\n    assert(s > 50.0)\n}\n";
        assert_eq!(
            judged_on(
                "r10-name",
                &suite.replace("NAME", "\"easy\""),
                "fn solve() -> i64 { 1 }\n"
            ),
            Ok(TestEnd::Completed),
            "control: a literal"
        );
        assert!(
            judged_on(
                "r10-name",
                &suite.replace("NAME", "\"hard\""),
                "fn solve() -> i64 { 1 }\n"
            )
            .is_err(),
            "control: the hard fn fails"
        );
        name_refused(
            "goal_eval",
            &suite.replace("NAME", "name()"),
            "fn name() -> str { \"easy\" }\n",
        );
    }

    #[test]
    fn a_goal_constraint_and_a_kernel_goal_take_no_function_name_the_candidate_chose() {
        let pre = "@[adaptive]\nfn metric(x: i64) -> i64 { x }\nfn ok_c(x: i64) -> bool { true }\n";
        let constrained = format!("{pre}@[test]\nfn t() {{\n    let r = goal_run_constrained(\"metric\", CNAME, 10.0, 5)\n    assert(r >= 0.0)\n}}\n");
        assert_eq!(
            judged_on(
                "r10-name",
                &constrained.replace("CNAME", "\"ok_c\""),
                "fn solve() -> i64 { 1 }\n"
            ),
            Ok(TestEnd::Completed),
            "control: a literal constraint"
        );
        name_refused(
            "goal_run_constrained's constraint",
            &constrained.replace("CNAME", "cname()"),
            "fn cname() -> str { \"ok_c\" }\n",
        );
        let kernel = format!("{pre}@[test]\nfn t() {{\n    let p = principal_root(\"r\", true, true, true, 100)\n    let g = kernel_goal_create(p, KNAME, 10.0)\n    let r = kernel_goal_run(g, 3)\n    assert(r >= 0.0)\n}}\n");
        assert_eq!(
            judged_on(
                "r10-name",
                &kernel.replace("KNAME", "\"metric\""),
                "fn solve() -> i64 { 1 }\n"
            ),
            Ok(TestEnd::Completed),
            "control: a literal goal"
        );
        name_refused(
            "kernel_goal_create",
            &kernel.replace("KNAME", "kname()"),
            "fn kname() -> str { \"metric\" }\n",
        );
    }

    /// Existence oracle (C9 round 10): a sealed caller is told the same thing
    /// whether the name it asked for is the operator's fn or does not exist.
    #[test]
    fn a_sealed_caller_cannot_tell_an_operator_fn_from_a_missing_one() {
        let suite =
            "fn secret(x: i64) -> i64 { 9 }\n@[test]\nfn t() {\n    assert_eq(solve(NAME), 9)\n}\n";
        let cand = |n: &str| {
            format!("fn solve(x: i64) -> i64 {{\n    let p = principal_root(\"r\", true, true, true, 100)\n    let sb = sandbox_create(p, \"IO\")\n    sandbox_run(sb, \"{n}\", 0)\n}}\n")
        };
        let said = |n: &str| match judged_on("r10-oracle", &suite.replace("NAME", "0"), &cand(n)) {
            Err(m) => m,
            Ok(e) => panic!("{n}: {e:?}"),
        };
        let (a, b) = (said("secret"), said("nonexistent_fn"));
        let norm = |m: &str, n: &str| m.replace(n, "NAME");
        assert!(
            norm(&a, "secret") == norm(&b, "nonexistent_fn"),
            "ATTACK: an operator fn and a missing one read differently to sealed code: {a} / {b}"
        );
    }

    /// Drift (amendment 100, BLOCKER A): pin verdicts are keyed (owner, site
    /// text), so every place that runs STORED operator code (a fn body, a
    /// closure body, a handler arm, a continuation replay, a predicate) must
    /// run it under the pin owner of the code's own definition. The fns that set
    /// the owner are exactly the listed ones, and every fn that evaluates a
    /// stored AST body outside the core evaluator is either one of them or
    /// listed with the reason its owner is already right.
    #[test]
    fn every_frame_that_runs_stored_operator_code_sets_its_owner() {
        // (fn, why it needs no guard of its own)
        const EVALUATES_STORED_CODE: &[(&str, &str)] = &[
            ("call_fn_frame", "sets the owner: the fn's own address"),
            (
                "call_closure_owned_by",
                "sets the owner: the creator's, from PIN_FN_MARK",
            ),
            (
                "init_globals",
                "runs under owner 0, the owner the lets are analysed under",
            ),
            (
                "run_test_fn",
                "the host-named test fn, entered through call_fn",
            ),
        ];
        const SETS_OWNER: &[&str] = &[
            "call_fn_frame",
            "call_closure_owned_by",
            "run_handler_arm",
            "replay_continuation",
        ];
        let items = fn_items();
        let mut setters: Vec<&str> = items
            .iter()
            .filter(|(_, _, b)| b.contains("PinGuard {"))
            .map(|(_, n, _)| n.as_str())
            .collect();
        setters.sort();
        let mut want: Vec<&str> = SETS_OWNER.to_vec();
        want.sort();
        assert_eq!(
            setters, want,
            "DRIFT: the set of fns that set `pin_fn` changed"
        );
        // The handler paths set it from the frame's recorded installer.
        for n in ["run_handler_arm", "replay_continuation"] {
            let body = &items.iter().find(|(_, m, _)| m == n).unwrap().2;
            assert!(
                body.contains("pin_owner"),
                "`{n}` must run under the pin owner recorded when the handler was installed"
            );
        }
        let mut unlisted = Vec::new();
        for (file, name, body) in &items {
            if *file == "interp/eval.rs" || SETS_OWNER.contains(&name.as_str()) {
                continue;
            }
            let evals_stored = body.lines().any(|l| {
                let l = l.trim_start();
                !l.starts_with("//")
                    && (l.contains("self.eval(&f.body")
                        || l.contains("self.eval(&body")
                        || l.contains("self.eval(&spec.predicate")
                        || l.contains("self.eval(pred")
                        || l.contains("self.eval(expr"))
            });
            if evals_stored && !EVALUATES_STORED_CODE.iter().any(|(n, _)| n == name) {
                unlisted.push(format!("{file}::{name}"));
            }
        }
        assert!(
            unlisted.is_empty(),
            "DRIFT: these fns evaluate a stored AST body without setting the pin owner and are not listed: {unlisted:?}"
        );
    }

    /// Drift (amendment 100): a lookup of an fn by NAME is either a listed
    /// name-resolving SINK (the name argument is judged at the call site) or
    /// listed here with the reason its name is not a value sealed code chose.
    #[test]
    fn every_name_resolving_lookup_is_a_listed_sink() {
        const ALLOWED: &[(&str, &str, &str)] = &[
            ("interp/eval.rs", "} else if let Some(f) = self.fns.get(name.as_str()) {", "an identifier in the program text, not a value"),
            ("interp/eval.rs", "let f = self.fns.get(name.as_str()).copied();", "the callee identifier in the program text"),
            ("interp/eval.rs", "Expr::Ident(name) => match self.fns.get(name) {", "the callee identifier of a `&mut` call, in the program text"),
            ("interp/proptest.rs", "let Some(f) = interp.fns.get(name).copied() else {", "the host's property-test name"),
            ("interp.rs", "if !interp.fns.contains_key(fn_name) {", "run_named_fn_*: the host-named deploy gate"),
            ("interp.rs", "let f: &FnDef = *interp.fns.get(fn_name)?;", "run_named_fn_*: the host-named deploy gate"),
            ("interp.rs", "if !interp.fns.contains_key(\"main\") {", "the literal `main`"),
            ("interp.rs", "let Some(f) = interp.fns.get(name).copied() else {", "run_test: the host-named test"),
            ("interp.rs", "match self.fns.get(\"main\") {", "the literal `main`"),
            ("interp.rs", "let f = self.fns.get(name.as_str())?;", "current_fn: the running fn's own name (2 sites)"),
            ("interp.rs", "let f = self.fns.get(name.as_str())?;", "current_fn: the running fn's own name (2 sites)"),
            ("interp.rs", "self.fns", "current_fn_has_ai_policy: the running fn's own name"),
            ("interp.rs", "let Some(f) = self.fns.get(name.as_str()) else {", "current_ai_tier: the running fn's own name"),
            ("interp.rs", "match self.fns.get(name).copied() {", "fn_by_name: THE resolver, which applies seal_call"),
            ("interp.rs", "let defined = match self.fns.get(name) {", "goal_name_is_known: reached only from the listed goal_* sinks; a sealed caller sees only its own fns"),
            ("interp/goal.rs", "if let Some(f) = self.fns.get(name) {", "run_goal_*: reached only from the listed goal_* sinks; call_fn applies seal_call"),
            ("interp/goal.rs", "let Some(cf) = self.fns.get(cname.as_str()).copied() else {", "the constraint name, a sink (goal_run_constrained arg 2)"),
            ("interp/goal.rs", "let f = match self.fns.get(name) {", "run_goal_*: reached only from the listed goal_* sinks (4 sites)"),
            ("interp/goal.rs", "let f = match self.fns.get(name) {", "run_goal_*: reached only from the listed goal_* sinks (4 sites)"),
            ("interp/goal.rs", "let f = match self.fns.get(name) {", "run_goal_*: reached only from the listed goal_* sinks (4 sites)"),
            ("interp/goal.rs", "let f = match self.fns.get(name) {", "run_goal_*: reached only from the listed goal_* sinks (4 sites)"),
            ("interp/goal.rs", "if let Some(f) = self.fns.get(name) {", "run_goal: reached only from the listed goal_* sinks"),
            ("interp/goal.rs", "let Some(f) = self.fns.get(name) else {", "goal_eval_holdout: the goal_eval sink"),
            ("interp/builtins.rs", "let outcome = match self.fns.get(fn_name.as_str()).copied() {", "a queued fiber: its name passed the scheduler_spawn sink"),
            ("interp/builtins.rs", "let visible = match self.fns.get(constraint.as_str()) {", "goal_run_constrained: a sink; a sealed caller sees only its own fns"),
            ("interp/builtins.rs", "let Some(f) = self.fns.get(&fn_name).copied() else {", "sandbox_run: a sink"),
            ("interp/builtins.rs", "if self.fn_by_name(&fn_name)?.is_none() {", "scheduler_spawn: a sink"),
            ("interp/builtins.rs", "if self.fn_by_name(&name)?.is_none() && !self.k().provenance.borrow().contains_key(&name) {", "kernel_goal_create: a sink"),
        ];
        let mut found: Vec<(String, String)> = Vec::new();
        for (file, src) in interp_sources() {
            let lines: Vec<&str> = src.lines().collect();
            for (i, line) in lines.iter().enumerate() {
                let t = line.trim();
                if t.starts_with("//") || t.starts_with("///") {
                    continue;
                }
                // `self.fns` split across lines (`self.fns\n .get(..)`).
                let hit = t.contains("fns.get(")
                    || t.contains("fns.contains_key(")
                    || t.contains("fn_by_name(")
                    || (t == "self.fns"
                        && lines
                            .get(i + 1)
                            .is_some_and(|n| n.trim().starts_with(".get(")));
                if hit && !t.starts_with("pub(crate) fn fn_by_name") {
                    found.push((file.to_string(), t.to_string()));
                }
            }
        }
        let mut allowed: Vec<(String, String)> = ALLOWED
            .iter()
            .map(|(f, l, _)| (f.to_string(), l.to_string()))
            .collect();
        found.sort();
        allowed.sort();
        assert_eq!(
            found, allowed,
            "DRIFT: an fn is looked up BY NAME somewhere unlisted — if the name can be a value sealed code chose, add its builtin to `pin::NAME_SINKS`; otherwise list the site with its reason"
        );
        // Every sink is a real builtin whose listed positions are `str` NAME params.
        for (b, idxs) in pin::NAME_SINKS {
            let def = crate::builtins::BUILTINS
                .iter()
                .find(|d| d.name == *b)
                .unwrap_or_else(|| panic!("sink `{b}` is not a builtin"));
            for &i in *idxs {
                let (pname, pty) = def.params[i];
                assert!(
                    pty == "str" && (pname.contains("name") || pname == "constraint"),
                    "sink `{b}` arg {i} is `{pname}: {pty}`, not a name"
                );
            }
        }
        // Every builtin arm that resolves a name is a sink (or reaches a name
        // chosen at a sink: a kernel goal's, a queued fiber's).
        const REACHES_A_SINKS_NAME: &[&str] =
            &["kernel_goal_run", "scheduler_run", "supervisor_run"];
        let src = interp_sources()
            .into_iter()
            .find(|(f, _)| *f == "interp/builtins.rs")
            .unwrap()
            .1;
        let mut arms: Vec<(Vec<String>, String)> = Vec::new();
        for line in src.lines() {
            let indent = line.len() - line.trim_start().len();
            let t = line.trim_start();
            if indent == 12 && t.starts_with('"') && t.contains("=>") {
                let head = &t[..t.find("=>").unwrap()];
                arms.push((
                    head.split('|')
                        .map(|n| n.trim().trim_matches('"').to_string())
                        .collect(),
                    String::new(),
                ));
            } else if let Some(last) = arms.last_mut() {
                last.1.push_str(line);
                last.1.push('\n');
            }
        }
        for (names, body) in &arms {
            let resolves = [
                "fn_by_name(",
                "fns.get(",
                "fns.contains_key(",
                "self.run_goal",
                "goal_eval_holdout(",
                "self.best_observed(",
                "self.best_input(",
                "self.best_inputs",
                "self.history(",
                "self.clear(",
                ".get(&name)",
                "contains_key(&name)",
            ]
            .iter()
            .any(|p| {
                body.lines()
                    .any(|l| !l.trim_start().starts_with("//") && l.contains(p))
            });
            if !resolves {
                continue;
            }
            for n in names {
                if !crate::builtins::BUILTINS.iter().any(|d| d.name == n) {
                    continue;
                }
                assert!(
                    pin::name_sink(n).is_some() || REACHES_A_SINKS_NAME.contains(&n.as_str()),
                    "DRIFT: builtin `{n}` resolves an fn or goal by name and is not in `pin::NAME_SINKS`"
                );
            }
        }
    }
}

// The runtime taint's own tests (C9 round 11, amendment 102).
#[cfg(test)]
mod taint_tests;

#[cfg(test)]
mod literal_escape_tests {
    use super::*;

    /// M1. A value containing `{` used to be dumped un-escaped, and since Axon
    /// strings interpolate, the dumped `let j = "{"` was itself unlexable. Every
    /// subsequent cell then died on `unclosed \`{\` in interpolated string` — the
    /// session was permanently bricked with no recovery path, and a model that
    /// builds a JSON-ish string does this immediately.
    #[test]
    fn braces_in_a_dumped_string_are_escaped_for_re_parsing() {
        let v = Value::Str(Rc::new("{\"a\": 1}".to_string()));
        let lit = value_as_literal(&v).expect("a string always has a literal form");
        assert!(
            lit.contains("{{") && lit.contains("}}"),
            "both braces must be doubled for the interpolating lexer: {lit}"
        );
    }

    /// M2. Option/Result/Tuple/Enum all HAVE literal syntax, and a session binds
    /// them constantly — `let o = parse_int(s)` is a `Result`. They used to fall
    /// into the catch-all and be reported as unserialisable, so the most
    /// ordinary binding a model writes could not cross a cell boundary.
    #[test]
    fn option_result_tuple_and_enum_have_literal_forms() {
        let cases: Vec<(Value, &str)> = vec![
            (Value::Some(Box::new(Value::Int(2))), "Some(2)"),
            (Value::None, "None"),
            (Value::Ok(Box::new(Value::Int(5))), "Ok(5)"),
            (
                Value::Err(Box::new(Value::Str(Rc::new("bad".to_string())))),
                "Err(\"bad\")",
            ),
            (Value::Tuple(vec![Value::Int(1), Value::Int(2)]), "(1, 2)"),
            // A 1-tuple needs the trailing comma, or it re-parses as a
            // parenthesised expression and silently changes type.
            (Value::Tuple(vec![Value::Int(7)]), "(7,)"),
        ];
        for (v, want) in cases {
            assert_eq!(
                value_as_literal(&v).expect("must have a literal form"),
                want
            );
        }
        // A fieldless enum variant renders bare; a fielded one renders with
        // sorted fields, as structs do.
        let mut f = HashMap::new();
        f.insert("r".to_string(), Value::Float(2.0));
        let e = Value::Enum {
            enum_name: "Shape".into(),
            variant: "Circle".into(),
            fields: f,
        };
        assert_eq!(value_as_literal(&e).unwrap(), "Shape::Circle { r: 2.0 }");
        let bare = Value::Enum {
            enum_name: "Shape".into(),
            variant: "Point".into(),
            fields: HashMap::new(),
        };
        assert_eq!(value_as_literal(&bare).unwrap(), "Shape::Point");
    }

    /// The round trip is the real requirement: what comes back must equal what
    /// went in. Escaping that is not the parser's inverse would corrupt values
    /// silently, which is worse than the wedge it replaces.
    #[test]
    fn a_braced_string_round_trips_through_the_parser() {
        for original in [
            "{",
            "}",
            "{\"k\": [1, 2]}",
            "a{b}c",
            "{{already doubled}}",
            "quote\" and \\ and \ttab",
        ] {
            let lit = value_as_literal(&Value::Str(Rc::new(original.to_string()))).unwrap();
            let src = format!("let x = {lit}\nfn main() -> i64 {{ 0 }}\n");
            let prog = crate::parse_source(&src)
                .unwrap_or_else(|e| panic!("dumped literal must re-parse ({original:?}): {e}"));
            let mut interp = Interp::build(&prog);
            interp.init_globals().expect("globals initialise");
            let got = interp.globals.get("x").expect("let x must be bound");
            // Value has no PartialEq; compare the rendered form, which is what
            // the session actually round-trips anyway.
            let got_s = match got {
                Value::Str(t) => t.as_str(),
                other => panic!("expected a Str, got {other:?}"),
            };
            assert_eq!(
                got_s, original,
                "round trip changed the value: {original:?} -> {lit}"
            );
        }
    }
}

#[cfg(test)]
mod value_shape_leak_tests {
    //! `value_shape` is the source of the RLM shape inventory, whose whole
    //! contract is that it carries STRUCTURE and no dataset values. A dict is
    //! the one container whose keys can be data rather than schema.
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    fn dict(pairs: &[(&str, Value)]) -> Value {
        let mut m = BTreeMap::new();
        for (k, v) in pairs {
            m.insert((*k).to_string(), v.clone());
        }
        Value::Dict(Rc::new(RefCell::new(m)))
    }

    /// A dict built by GROUPING is keyed by the data. Printing that key set put
    /// `north`/`south`/`east` straight into the inventory and voided two arm-A
    /// measurement runs (the harness leak guard caught it and withheld the arm).
    #[test]
    fn grouping_dict_keys_are_not_emitted() {
        let s = value_shape(&dict(&[
            ("north", Value::Int(315)),
            ("south", Value::Int(95)),
        ]));
        for leaked in ["north", "south", "315", "95"] {
            assert!(
                !s.contains(leaked),
                "value_shape leaked the dataset value `{leaked}`: {s}"
            );
        }
    }

    /// The shape must still be USEFUL: length, key type, and the value shape are
    /// what tell a model whether `dict_get` hands back an i64 or a record.
    #[test]
    fn dict_shape_still_carries_length_and_value_type() {
        let s = value_shape(&dict(&[("north", Value::Int(315))]));
        assert!(s.contains("len=1"), "{s}");
        assert!(s.contains("keys are str"), "{s}");
        assert!(s.contains("i64"), "{s}");
    }
}
