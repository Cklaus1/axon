//! R50: the bytecode engine for fn bodies (`governance/specs/R50-register-vm.md`).
//!
//! A fn body in `Interp::fn_table` is compiled on its first run under
//! `AXON_ENGINE=vm` into a [`Body`]: a flat op sequence that runs against the
//! same `Env` the tree-walker would use (spec §4 Fork 1), so params, `goal_met`,
//! `&mut` read-back, postconditions and closure capture see the bindings where
//! they always were. A node the compiler does not lower is one [`Op::Tree`],
//! which calls [`Interp::eval`] on it (§4 Fork 2). Through slice S0 nothing is
//! lowered: every body is exactly one `Tree` op over the whole body.
//!
//! The tree-walker stays the reference engine (I-2). `AXON_ENGINE=tree` (the
//! default) never compiles anything.

use super::*;

mod compile;
#[cfg(test)]
mod tests;

pub(super) use compile::compile;

/// Which engine runs fn bodies. Chosen once per `Interp` (spec §4 Activation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// The tree-walker (`Interp::eval`), the reference engine.
    Tree,
    /// Compiled ops ([`Body`]) for fn-table bodies, the tree for the rest.
    Vm,
}

impl Engine {
    /// The engine when nothing selects one.
    pub const DEFAULT: Engine = Engine::Tree;

    /// Parse an `AXON_ENGINE` value; the error is the message the CLI prints
    /// before exiting 2 (spec §3).
    pub fn parse(raw: &str) -> Result<Engine, String> {
        match raw {
            "tree" => Ok(Engine::Tree),
            "vm" => Ok(Engine::Vm),
            other => Err(format!(
                "AXON_ENGINE must be \"vm\" or \"tree\" (got \"{other}\")"
            )),
        }
    }
}

/// [`set_engine`]'s choice: 0 = none (use `AXON_ENGINE`), 1 = tree, 2 = vm.
/// A process-wide atomic rather than a thread-local because native runs build
/// the `Interp` on the deep-stack thread (`on_deep_stack`).
static ENGINE_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Select the engine for every `Interp` built afterwards, overriding
/// `AXON_ENGINE`. For hosts without an environment: `axon-wasm`'s
/// `axon_set_engine` export on `wasm32-unknown-unknown` (spec §4 Activation).
pub fn set_engine(engine: Engine) {
    let code = match engine {
        Engine::Tree => 1,
        Engine::Vm => 2,
    };
    ENGINE_OVERRIDE.store(code, std::sync::atomic::Ordering::Relaxed);
}

/// The engine an `Interp` being built runs with: [`set_engine`]'s choice, else
/// `AXON_ENGINE`, else [`Engine::DEFAULT`]. An invalid `AXON_ENGINE` prints the
/// spec §3 message and exits 2. `Interp::build` runs before any of the
/// program does, under every entry (`axon run`, `axon-run`, `axon test`,
/// `axon goal`), so this is the one place that check needs.
pub(super) fn engine_at_build() -> Engine {
    match ENGINE_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => return Engine::Tree,
        2 => return Engine::Vm,
        _ => {}
    }
    match std::env::var_os("AXON_ENGINE") {
        None => Engine::DEFAULT,
        Some(raw) => Engine::parse(&raw.to_string_lossy()).unwrap_or_else(|msg| {
            eprintln!("{msg}");
            std::process::exit(2)
        }),
    }
}

/// `AXON_VM_TRACE=1` (spec §3), read once when the `Interp` is built.
pub(super) fn trace_at_build() -> bool {
    std::env::var_os("AXON_VM_TRACE").is_some_and(|v| v == "1")
}

/// A compiled fn body. It borrows the nodes of the program it was compiled
/// from (`Tree` ops), so it lives in that fn's `FnEntry::compiled` and never
/// outlives the program.
pub(super) struct Body<'p> {
    pub(super) ops: Box<[Op<'p>]>,
}

impl Body<'_> {
    /// The number of [`Op::Tree`] ops, the trace's `<k> tree nodes`.
    pub(super) fn tree_nodes(&self) -> usize {
        self.ops
            .iter()
            .filter(|op| matches!(op, Op::Tree(_)))
            .count()
    }
}

/// One instruction. Every op runs against the activation's `Env`; a
/// value-producing op yields the `Result<Value, Flow>` the tree-walker would
/// have yielded there.
pub(super) enum Op<'p> {
    /// Evaluate the node on the tree-walker; its value becomes the body's
    /// current value. `eval` pops every scope it pushes on every outcome, so
    /// the scope count is the same after the op as before it.
    Tree(&'p Expr),
    /// `env.push()`, where `eval` pushes a scope (block, loop iteration, match
    /// arm, while-let, `for`). Emitted by the lowering slices (S1 on); S0
    /// compiles every body to a single `Tree` op.
    #[cfg_attr(not(test), allow(dead_code))]
    ScopePush,
    /// `env.pop()` of the innermost scope this activation pushed.
    #[cfg_attr(not(test), allow(dead_code))]
    ScopePop,
}

impl<'p> Interp<'p> {
    /// Run a fn body: compiled under [`Engine::Vm`] for a fn-table entry, on
    /// the tree otherwise. `call_fn_in` is the only caller, at the point where
    /// it would evaluate `entry.def.body`; everything before and after the
    /// body stays there (spec §4 Activation).
    #[inline]
    pub(super) fn run_body(&self, entry: &FnEntry<'_>, env: &mut Env) -> R {
        match self.engine {
            Engine::Tree => self.eval(&entry.def.body, env),
            Engine::Vm => self.run_body_vm(entry, env),
        }
    }

    #[inline(never)]
    fn run_body_vm(&self, entry: &FnEntry<'_>, env: &mut Env) -> R {
        let Some(cell) = &entry.compiled else {
            self.vm_trace_tree(entry, "not in fn table");
            return self.eval(&entry.def.body, env);
        };
        let body = cell.get_or_init(|| {
            let body = compile(&entry.def.body);
            if self.vm_trace {
                self.vm_trace_compiled(entry, &body);
            }
            body
        });
        self.exec(body, env)
    }

    /// Execute `body` against `env`. On every exit, normal or `Err`, the
    /// scopes this activation pushed are popped one at a time, innermost
    /// first (never truncated to a recorded depth: `call_mut` reads `&mut`
    /// params back by name afterwards, spec §4 Execution). `Flow::Return(v)`
    /// ends the body with `Ok(v)`, which `call_fn_in` treats exactly as the
    /// `Err(Flow::Return(v))` the tree-walker's body yields; every other
    /// `Flow` propagates unchanged.
    pub(super) fn exec(&self, body: &Body<'_>, env: &mut Env) -> R {
        // The value of the last value-producing op: the body's value when the
        // ops run out. S1 adds the pooled operand stack (spec §4 Execution);
        // through S0 one register suffices and a call allocates nothing.
        let mut acc = Value::Unit;
        let mut scopes = 0usize;
        let mut pc = 0usize;
        let outcome: Result<(), Flow> = loop {
            let Some(op) = body.ops.get(pc) else {
                break Ok(());
            };
            pc += 1;
            match op {
                Op::Tree(e) => match self.eval(e, env) {
                    Ok(v) => acc = v,
                    Err(flow) => break Err(flow),
                },
                Op::ScopePush => {
                    env.push();
                    scopes += 1;
                }
                Op::ScopePop => {
                    let Some(n) = scopes.checked_sub(1) else {
                        unreachable!("vm: ScopePop with no scope pushed (malformed op stream)")
                    };
                    env.pop();
                    scopes = n;
                }
            }
        };
        for _ in 0..scopes {
            env.pop();
        }
        match outcome {
            Ok(()) => Ok(acc),
            Err(Flow::Return(v)) => Ok(v),
            Err(flow) => Err(flow),
        }
    }

    /// `vm: tree <name>: <reason>` under `AXON_ENGINE=vm AXON_VM_TRACE=1`: a
    /// whole fn body runs on the tree-walker.
    pub(super) fn vm_trace_tree(&self, entry: &FnEntry<'_>, reason: &str) {
        if self.engine == Engine::Vm && self.vm_trace {
            eprintln!("vm: tree {}: {reason}", self.vm_body_name(entry));
        }
    }

    /// The per-body trace line and one `tree-op` line per `Tree` op, printed
    /// when a body is compiled (its first run).
    fn vm_trace_compiled(&self, entry: &FnEntry<'_>, body: &Body<'_>) {
        let name = self.vm_body_name(entry);
        eprintln!(
            "vm: {name} {} ops, {} tree nodes",
            body.ops.len(),
            body.tree_nodes()
        );
        for op in body.ops.iter() {
            if let Op::Tree(e) = op {
                let variant = compile::variant_name(e);
                match compile::tree_shape(e) {
                    Some(shape) => eprintln!("vm: tree-op {name} {variant}({shape})"),
                    None => eprintln!("vm: tree-op {name} {variant}"),
                }
            }
        }
    }

    /// A body's trace name: the fn's name, `Type::method` for an impl method.
    fn vm_body_name(&self, entry: &FnEntry<'_>) -> String {
        self.methods
            .iter()
            .find(|(_, d)| std::ptr::eq(**d, entry.def))
            .map(|((ty, m), _)| format!("{ty}::{m}"))
            .unwrap_or_else(|| entry.def.name.clone())
    }
}
