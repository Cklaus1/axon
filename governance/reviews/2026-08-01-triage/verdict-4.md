# Adversarial triage — crit-4.md

Method: each finding was read against the cited source and, where possible,
reproduced with a built `axon` / `axon-os` binary on this checkout
(`cargo build -p axon-core --no-default-features --bin axon`, `cargo build -p axon-os`).
Default posture was refutation; three survived with live repros.

---

## F153 — @[contained] escape via string-named indirect dispatch

**VERDICT: CONFIRMED** · severity CRITICAL -> **CRITICAL**

The cited code says what the finding claims. `capabilities::check_expr` builds its
follow-edges only from `Expr::Call`/`Expr::MethodCall` callee *names*
(`crates/axon-core/src/capabilities.rs:630-651`: `let callee_name = match callee.as_ref() {
Expr::Ident(name) => …}` then `if let Some(helper) = ctx.fn_map.get(name)`), and the
`Expr::MethodCall` arm at `:705-712`. A string in argument position is only walked as an
arg (`:661-663 for arg in args { check_expr(arg, ctx); }`), never resolved through
`fn_map`. The interpreter meanwhile dispatches by string at
`crates/axon-core/src/interp/builtins.rs:116` (`self.fns.get(fn_name.as_str()).copied()`,
scheduler fibers) and `:2832` (`let Some(f) = self.fns.get(&fn_name).copied() else`,
`sandbox_run`).

Reproduced both vectors on this checkout. `scheduler_spawn("evil", 1)` inside
`@[contained(fs: [read("./data/")], net: [], exec: none)]`: `axon check` exit 0 (no
E1001), `axon run` printed `EXFIL len=10` after `read_file("/etc/hostname")`. Same for
`sandbox_run(sb, "evil", 7)` inside `@[contained(fs: [], …)]`: check exit 0, run printed
`EXFIL via sandbox_run len=10`. Control: rewriting the same body to call `evil(1)`
directly is correctly refused —
`[E1001] read_file("/etc/hostname") is not permitted by @[contained]`, exit 2. Since
`@[contained]` is compile-time-only, a static miss is a total escape of the repo's
flagship capability-sandbox claim.

**Fix:** in `check_expr`/`collect_caps_expr`, treat a string-literal arg in the fn-name
slot of `scheduler_spawn`/`sandbox_run`/`goal_run*`/`kernel_goal_*` as a call edge
resolved via `fn_map`, and fail closed on a non-literal argument.

---

## F041 — Nested sandbox_create/sandbox_run escapes the enclosing effect ceiling

**VERDICT: CONFIRMED** · severity CRITICAL -> **CRITICAL**

Both cited lines are real. `crates/axon-core/src/interp/builtins.rs:191`:
`if sb_handle >= 0 && name != "sandbox_create" && name != "sandbox_run" {` — the two
sandbox-management builtins skip the ceiling check entirely. And the `sandbox_run` arm at
`:2836` is `let prev_sandbox = self.active_sandbox.replace(sb_handle);` — a *replace*,
with no intersection against the currently-active set; `sandbox_create` (`:2798-2810`)
happily registers any comma-separated set with no reference to the active ceiling.

Reproduced: outer `sandbox_create(p, "")` (empty ceiling) → `sandbox_run(sb, "job", 0)`,
where `job` does `sandbox_create(p2, "Random,IO")` → `sandbox_run(sb2, "evil", 0)`,
printed `ESCAPED: got random 4`, exit 0. The comment at `:186-188` claims the exemption
merely "avoids infinite regress"; in fact it removes the floor.

**Fix:** intersect the requested allowed-set with the active one in the `sandbox_run` arm
(or refuse a non-subset), and drop the `sandbox_create` exemption.

---

## F161 — Guest syscall gate's ALLOW path triple-faults; kernel_enforce_test case 2 vacuous

**VERDICT: REFUTED** (as stated) · severity CRITICAL -> **MEDIUM**

The headline defect is not in shipped code. `crates/axon-guest-kernel/src/enforce.rs:299-303`
takes the grant branch and immediately `clean_halt()`s — it never issues `syscall`, so
nothing triple-faults. The doc comment at `:290-294` states the reason verbatim: "the
allowed return path (`sysretq`) needs ring-3 user segments the boot GDT doesn't yet
define, which is part of the full-execution work". The reviewer's triple fault came from a
*modified* scratch copy they wrote themselves, so it is not a reachable defect here. The
underlying facts check out (`enforce.rs:426-427` `USER_CS: u64 = 0x20`; `boot.s:132-137`
has only 5 GDT entries ending at 0x20), but they describe unwritten future work, tracked
as todo in `governance/EXECUTION_MODEL.md:41` ("R36.S1 … deny + permit | todo") and
`governance/specs/R36-full-asi-os.md:343` (Ring 3 = S1).

What survives is documentation/test honesty, not security: `scripts/kernel_enforce_test.sh:44`
greps `"K5: policy GRANTS FS"` plus absence of a violation, which is vacuous; `:53` then
prints "denies/permits by policy, live, end-to-end", and `AXON_KERNEL.md:18` says "LIVE
ENFORCEMENT IS DEMONSTRATED". The script is referenced only by prose docs — a repo-wide
grep found no hit in `gate.sh`, `parity_all.sh`, CI, or any `.rs` test. Deny-path
enforcement, which is the part carrying the claim, is genuinely exercised.

**Fix (residual):** rename to `kernel_enforce_deny_test.sh`, drop the "permits" wording
from the PASS line and `AXON_KERNEL.md`, and wire it into `parity_all.sh`.

---

## F138 — axon-os reports "✓ completed" / exit 0 for guest exits 3, 4, 5

**VERDICT: CONFIRMED** · severity CRITICAL -> **HIGH**

The verdict chain in `crates/axon-os/src/runtime.rs:377-414` is exactly as described:
branches for `killed_by_latch`, `timed_out`, `"sandbox violation"`/`"capability"`,
`"budget"`+`"exhaust"`, `"refinement violated"`/`"REFINE"`, `"axon: panic"`, then
`else { Verdict::Completed { value: proc.code.unwrap_or(0) as i64 } }`. The guest emits
`axon: verify failed:` (`crates/axon-core/src/interp.rs:1584`), `axon: halted:` (`:1592`)
and `axon: ai policy:` (`:1601`) — none of which match any branch.
`Verdict::Completed => 0` at `crates/axon-os/src/verdict.rs:47`.

Reproduced: a `@[verify(value > 100)]` program returns `axon: verify failed …` / exit 3
standalone, but `axon-os run vf.axjob` printed `✓ completed (value=3)` and exited 0, with
`Completed` persisted to `run.json`. This directly contradicts the contract comment at
`verdict.rs:42-43` ("never collapse a fail-closed outcome into a generic code"). Not
CRITICAL because no capability is granted that was withheld — the guest still stopped —
but the supervisor's audit record and exit code both report success for a tripped
corrigibility kill-switch.

**Fix:** add explicit branches for `"axon: verify failed"` → a policy `Denied`,
`"axon: halted"` → `Verdict::Halted`, `"axon: ai policy"` → `Denied`, ahead of the
`Completed` fallthrough (ideally replaced by a structured non-guest-writable status).

---

## F013 — Contained axon-os job escapes the runtime sandbox by minting a wider one

**VERDICT: CONFIRMED** · severity CRITICAL -> **CRITICAL**

Same root cause as F041 but proven through the axon-os supervisor, which is the layer that
claims soundness: `crates/axon-os/src/runtime.rs:299-305` — "The static gate is a
best-effort PRE-check; THIS is what actually contains." `wrap_in_sandbox`
(`runtime.rs:253-273`) emits `sandbox_create(__p, "{csv}")` + `sandbox_run(__sb,
"__job_entry", 0)`, and the job body then re-widens inside it.

Verified end-to-end with a zero-cap manifest (`fs_read=[] fs_write=[] net=[] exec="none"`).
Control program calling `random_i64` directly: `⚠ DENIED: sandbox violation: builtin
random_i64 requires effect Random which is not in the active sandbox's allowed set {}`,
exit 8. Identical program wrapped in its own `sandbox_create(p, "IO,Net,AI,Random,FS")` +
`sandbox_run(sb, "pwn", 0)`: `✓ completed (value=0)`, exit 0, effect executed. (The
reviewer's `exec ("touch", …)` variant additionally slips the static pre-gate because
`scan_effects` at `runtime.rs:232` matches the literal substring `"exec("` and a space
defeats it — a second, independent weakness in the same path.)

**Fix:** make ceilings monotonically narrowing in the `sandbox_run` arm (intersect, or
refuse a non-subset), plus an axon-os acceptance test asserting a self-widening job under
a zero-cap grant exits 8.
