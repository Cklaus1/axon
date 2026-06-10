# Tech Spec — Resume / suspend-across-host-event runtime

**Spec ID:** `R15-resume-runtime` (Phase-6 continuation runtime; gates R7c interactive, R13/R14 UI)
**Status:** Implementing — **slice-1 v0 LANDED** (`623d1a2`): `host_await(str)->str` via a
worker-thread substrate (str payloads), B1/B2/B4/B5 green. Remaining slices below.
**Risk class:** Structural
**Author / date:** loop (autonomous), 2026-06-10

---

### 1. Motivation

Every *interactive* Axon target — a browser frame loop / `fetch` (R7c), a native or
mobile UI run loop (R13/R14) — requires a running program to **suspend** when it asks the
host for something that isn't ready (the next frame, a network reply, a user event) and
**resume** later, *with its in-flight state and native resources still alive*. Today Axon
has no such mechanism, so every interactive target is stuck at "compute-only" — which is
why R7c, R13, and R14 all list this runtime as their one shared blocker.

This is distinct from what already shipped. **Phase-6 replay-based multi-shot `resume`**
(`7a79772`, E1314) reifies a continuation *inside a `with`-handler block* by RE-RUNNING the
block body from a snapshot and feeding the resume value at the intercepted effect
(`interp/eval.rs::replay_continuation`). That is sufficient for handler semantics but
**cannot** model suspend-to-host: see §4.

### 2. Requirement link

`REQUIREMENTS.md` R7 (cross-platform, interactive tier) + the R7c/R13/R14 specs, all of
which say "interactive/async is gated on the Phase-6 `resume` runtime." Also completes the
Phase-6 row in `CLAUDE.md` ("true suspend-across-host-event continuation … remaining").

### 3. Surface (what the user writes)

No new *language* surface in v1 — the primitive is a builtin the stdlib/host wraps:

```axon
// Yield `request` to the host and block until the host resumes with a reply.
let reply = host_await(request)        // request: T_req → reply: T_rep
```

Higher-level forms (`on_frame { … }`, `fetch(url)`, an event loop) desugar to `host_await`
over a typed effect, exactly as Phase-8 `for!`/`goal{}` desugar to `goal_run`. They are
**out of scope here**; this spec is the primitive + the host driver API.

### 4. Semantics (what it does) — and the decisive fork

A program reaches `host_await(req)`. Control must return to the host (the event loop /
`cmd_run`) carrying `req`; the host, some time later (a different stack, possibly a
different event-loop turn), calls `resume(token, reply)`; the program continues from the
`host_await` with `reply` as its value, **with all intervening state intact**.

**The decisive fork is HOW the suspended computation is preserved.** Three options; the
spec must pick one before any code (Gate 1):

| Option | Mechanism | Verdict |
|---|---|---|
| **(a) Replay at program scope** | Re-run `main` from the start on each resume, feeding prior replies at earlier `host_await`s (the §1 handler-replay model, lifted to program scope). | **REJECTED.** Any side effect *before* a suspend (`println`, a mutation, an FFI call) RE-FIRES on every resume — a frame loop would reprint its whole history each frame. Sound only for a pure prefix; useless for interactive I/O. (Confirmed by reading `replay_continuation`: it works precisely because a `with`-body's pre-resume effects are guarded by E1314, which would fire on the *first* real-world effect here.) |
| **(b) CPS transform** | Transform the program to continuation-passing style so the continuation is a reified, callable value. | Viable but a **large front-end change** (every eval form rewritten), and it fights the existing direct recursive-descent `eval`. Deferred — too invasive for the first sound mechanism. |
| **(c) Stackful coroutine** | Run `eval` on a **separate stack**; `host_await` PARKS that stack (yielding `req`); `resume` UNPARKS it with `reply`. The interpreter's recursive `eval` is **unchanged** — suspension is a runtime concern, not a code-shape concern. | **RECOMMENDED** for the interpreter. Preserves all existing semantics by construction (I-2), no CPS rewrite, and maps onto the shipped cooperative-scheduler fiber model (`kernel.rs`) — which today has only `Ready/Done/Failed` and needs a `Suspended(token)` state + a park/unpark channel. |

**Resolution: (c) stackful coroutine, interpreter-only — and it MUST be SAME-THREAD.**

> **Substrate correction (verified against code, 2026-06-10).** The first draft proposed an
> OS-thread substrate ("park the worker thread on a channel"). **That is INFEASIBLE.**
> `Interp<'p>` and `Value` are pervasively `Rc<RefCell<…>>` (`Value::Chan`/`Dict`,
> `interp.rs:35,252` — every interp field is `RefCell`, values hold `Rc`), so both are
> **`!Send`**: the interp can't be owned by another thread, and a `req`/`reply` `Value`
> can't cross a channel between threads. A thread substrate would require either making the
> entire interpreter `Send` (`Rc→Arc`, `RefCell→Mutex` everywhere — a huge, perf-regressing
> refactor) or restricting `host_await` to `Send`-only payloads (a crippled API). Both are
> rejected.

The substrate is therefore a **same-thread stackful coroutine**: a crate
(`corosensei` — `no_std`, the cleanest safe-ish API; or `generator`) switches the *stack
pointer* on the current thread, so the `Rc`/`RefCell` graph stays valid and nothing needs to
be `Send`. The coroutine runs `eval(main)`; `host_await(req)` calls the coroutine's
`suspend(req)` (saving the worker stack, returning to the host's stack); the host driver
calls `coroutine.resume(reply)` to continue. The host owns the token→coroutine map.

**The hard part of slice 1 (call it out): yielder plumbing.** `host_await` is reached DEEP
inside the recursive `eval`, but the coroutine's *yielder/suspend handle* is created at the
coroutine boundary. So the handle must be reachable from `eval` — stored on the `Interp`
behind a `RefCell<Option<…>>`, like `resume_replay`. Its lifetime is tied to the coroutine
(< the `Interp`'s), so storing it needs a scoped install/uninstall around each
`coroutine.resume`, or a contained `unsafe` lifetime-erasure (the handle is only valid while
the coroutine is running, which is exactly when `eval` runs). This is the slice-1 design
risk; the test that proves it correct is B2.

**Codegen:** native/wasm codegen does NOT get suspend-across-call in v1 (that needs Asyncify
or a CPS lowering). A program that reaches `host_await` under codegen is **E0910-refused** —
exactly the sound posture handlers already use ("interp runs it, codegen refuses it"), so
I-2 holds by refusal. The browser interactive case (R7c) reuses the *interp→wasm* path (the
shipped Slice-A engine), where the coroutine is a JS-driven re-entry, NOT a thread — that
browser binding is a **follow-on slice** (Asyncify or interp-loop integration), called out
but not built here.

Behavior table (the test plan in §8 maps 1:1):

| # | Program shape | Expected |
|---|---|---|
| B1 | `host_await(x)` once, host doubles it | program sees `2x`, runs to completion |
| B2 | side effect, then `host_await`, then side effect | **each side effect fires EXACTLY once** (the bar option (a) fails) |
| B3 | `host_await` in a loop (N iterations) | host sees N requests in order, program sees N replies, final value correct |
| B4 | program never calls `host_await` | runs to completion with no suspension (zero overhead path unchanged) |
| B5 | `host_await` under `axon build` (codegen) | **E0910** at compile time, naming the unsupported primitive |
| B6 | host drops a suspended computation (no resume) | resources freed; no deadlock/leak |

### 6. Error codes

- **E1315** — `host_await` reached under native/wasm codegen (out of the lowerable subset).
  Reuses the E131x effects band (next free after E1314).
- A `resume(token)` for an unknown/already-finished token → host-side error (not a program
  error); v1 returns a `Result::Err` to the host driver, never panics the interpreter.

### 7. Invariants touched

- **I-2 (interpreter is the oracle):** preserved — codegen refuses (E0910/E1315) what it
  can't run; the interp coroutine runs the *same* `eval`, so observable results are
  identical by construction.
- **I-5 (affine resources):** a native resource held across a suspend stays owned by the
  parked computation; this spec must ensure park/unpark does not duplicate or drop it
  (the coroutine owns its stack, so ownership is natural — no clone, unlike replay).
- No invariant is *weakened* (unlike R13's I-11/I-12 note); this is additive.

### 8. Test plan (interp-only, no browser needed)

A Rust-driven test harness is the "host": it runs a suspendable `.ax` via the new entry
point, collects each `host_await` request, computes a reply, resumes, and asserts the final
value + that each side effect (a counter / appended log) fired exactly once.

- `host_await_single_roundtrip` (B1)
- `host_await_effects_fire_once_not_per_resume` (B2) — the load-bearing test; the replay bar.
- `host_await_loop_n_times` (B3)
- `no_await_runs_unchanged` (B4) + `parity_all.sh` stays green (no regression to the
  non-suspending path).
- `host_await_codegen_refused_e1315` (B5) — `axon build` aborts with E1315.
- `dropped_suspension_frees_resources` (B6).

### 9. Acceptance criteria (the done gate)

A `.ax` program that suspends via `host_await`, driven by a Rust host harness, resumes
correctly with **each side effect firing exactly once** (B2), across an N-iteration loop
(B3); `axon build` of the same program emits E1315; `parity_all.sh` + the full suite stay
green. (The browser binding and the `on_frame`/`fetch` surface are explicitly NOT in this
acceptance set — they are follow-on slices.)

### 10. Performance budget

The non-suspending path (B4 — the overwhelming common case) must be **zero-overhead**:
no coroutine stack is allocated unless a program actually reaches `host_await` (the entry
point runs `eval` directly and only wraps it in a coroutine when the program is declared
suspendable / the first `host_await` is hit). A suspendable run pays one coroutine-stack
allocation + a register-save stack-switch per suspend — far cheaper than a thread; no thread
is ever spawned (the same-thread substrate is the only feasible one — §4).

### 11. Rollout & rollback

Interp-only + additive → the `host_await` builtin is inert unless a `run_suspendable`
driver is active. Rollback = revert the slice; nothing else depends on it yet.

Slices:
- **(1) v0 — LANDED (`623d1a2`).** `host_await(str)->str` via a **worker-thread** substrate
  (the `!Send` interp stays on the worker; str payloads cross as `String`, result as `i32`).
  B1/B2/B4/**B5** green + a clean no-host error. No `Flow` variant or `unsafe` was needed —
  the worker thread blocking on the reply channel IS the suspension. This validates the API
  + the suspension property (B2: host called exactly once per await) NOW.
- **(1b) v0+ — LANDED (`f71d75e`, `652376e`).** A stdin/stdout host (`run_suspendable_stdio`)
  wired to `axon run`, so interactive programs work via the CLI (B1 end-to-end through the
  binary). **B3 (host_await in a loop) also passes in v0** — the loop shape is
  payload-agnostic, so the thread substrate handles it; only arbitrary-`Value` payloads
  actually need the coroutine. Demo: `examples/interactive/`. So v0 already covers
  **B1/B2/B3/B4/B5**.
- **(2) v1 — the coroutine swap.** Replace the thread substrate with a same-thread stackful
  coroutine (§4) so **arbitrary-`Value` payloads** (dict/struct — they're `!Send`, so can't
  cross a thread) work, and to drop the per-suspend thread cost. This is the intricate part
  (the yielder-lifetime plumbing + vendored `unsafe`). *This is the ONLY thing the coroutine
  is needed for* — str/scalar payloads, loops, and the suspension property all work on the
  thread substrate today. A safe alternative to evaluate first: **deep-clone Values to a
  `Send` owned form** across the thread (no `unsafe`, no coroutine) — viable for native
  hosts, though the browser (single-threaded) still needs the coroutine/Asyncify path.
- **(3)** drop/resource (B6) + the kernel-scheduler `Suspended(token)` state.
- **(4)** the browser binding (single-threaded → Asyncify / JS step-loop; R7c follow-on) and
  the `on_frame`/`fetch` surface (desugar to `host_await`, like Phase-8 `for!`).

### 12. Open questions

- **Q1 — substrate: RESOLVED to a same-thread stackful-coroutine crate** (OS thread is OUT —
  `Interp`/`Value` are `!Send`, verified). Open sub-question: `corosensei` (maintained,
  `no_std`, used by async runtimes) vs `generator` vs a hand-rolled `ucontext`/`makecontext`
  binding. *Lean: `corosensei`.* The remaining risk is the `unsafe` stack switch (vendored by
  the crate) + the yielder-lifetime plumbing (§4) — both contained to the runtime module.
- **Q2 — token identity:** an opaque `u64` handle in a host-owned map (like the kernel
  `*mut` handles) — unforgeable, I-4-compatible. Confirmed direction.
- **Q3 — multi-shot:** does a host ever resume the SAME token twice (multi-shot
  continuation)? A coroutine is single-shot by nature. v1 = single-shot (a second resume of
  a token → host error). Multi-shot stays the replay-handler's job (different mechanism,
  different soundness envelope).
- **Q4 — browser:** the interp→wasm path is single-threaded; a thread substrate can't run
  there. The browser slice needs Asyncify or a JS-driven interp step-loop — deferred to an
  R7c follow-on, but the `host_await` *surface* and *interp semantics* defined here are the
  target both bindings implement.
