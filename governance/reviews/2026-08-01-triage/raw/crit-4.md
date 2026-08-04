### F153 — @[contained] sandbox escape via string-named indirect dispatch (scheduler_spawn, sandbox_run, goal_run family)
*crates/axon-core/src/capabilities.rs:620-712 (walker); crates/axon-core/src/interp/builtins.rs:116,2832; crates/axon-core/src/interp/goal.rs:33,109,255,349,446,521,630,1277* · kind: capability-laundering

`capabilities::check_expr` builds the capability graph purely from `Expr::Call`/`Expr::MethodCall` *name* edges (crates/axon-core/src/capabilities.rs:620-712). But the interpreter dispatches user functions by STRING at eleven sites: `self.fns.get(fn_name)` at interp/builtins.rs:116 (`scheduler_spawn`) and :2832 (`sandbox_run`), plus eight in interp/goal.rs. To the walker those are just `Expr::Literal(Str(..))` arguments — no call edge exists, so the callee's body is never checked against the caller's spec. This defeats the whole transitive-follow design the file's comments describe as "a sandbox must not be escapable by moving the forbidden I/O one function call away".

Vector 1 — `scheduler_spawn`:
```
@[contained(fs: [read("./data/")], net: [], exec: none)]
fn sandboxed() -> i64 { let f = scheduler_spawn("evil", 1)  scheduler_run() }
```
```
$ axon check l2.ax
warning: [W0006] unused variable `f` ...
exit=0                      <-- NO E1001
$ axon run l2.ax
EXFIL len=892               <-- /etc/shadow read from inside the sandbox
NET attempted: http_get requires the asi-runtime feature or a network-capable host
result 1
```

Vector 2 — `sandbox_run` (the builtin advertised as the *runtime* enforcement counterpart):
```
@[contained(fs: [], net: [], exec: none)]
fn sealed() -> i64 {
  let p = principal_root("attacker", true, true, true, 1000)
  let sb = sandbox_create(p, "FS,Net,Exec,AI,IO")
  sandbox_run(sb, "evil", 7)
}
```
```
$ axon check l6.ax
exit=0                      <-- NO E1001
$ axon run l6.ax
EXFIL via sandbox_run: gpumaster
7
$ axon deploy l6.ax --json
{"schema":"axon-deploy/1",...,"status":"deployed","exit_code":0,"risk":"low","stages_run":[]}
```
There is no runtime backstop for `@[contained]` (it is a compile-time-only gate by design), so a static miss is a total escape. I verified this is a genuine gap and not a general weakness: the documented routes — impl methods, `handler NAME = handler {...}` named handlers, closures constructed outside and invoked inside, `use mod.{f}` imports, and dynamically-built paths — all correctly produce E1001.

**Recommendation:** Treat a string-literal argument in the fn-name position of `scheduler_spawn`/`sandbox_run`/`goal_run*`/`kernel_goal_*` as a call edge in `check_expr` and `collect_caps_expr`, resolving it through `fn_map` (a non-literal argument must fail closed, exactly as `read_file(<dynamic path>)` already does). Single-source the list of name-dispatching builtins next to `classify_call` so it cannot drift. Longer term this class recurs for every new builtin that takes a fn name, so a `BUILTINS` field marking the fn-name parameter index would be more durable than a hardcoded list.

---

### F041 — Nested sandbox_create/sandbox_run escapes the enclosing effect ceiling entirely
*crates/axon-core/src/interp/builtins.rs:191* · kind: bug

crates/axon-core/src/interp/builtins.rs:191 exempts `sandbox_create` and `sandbox_run` from the ceiling check, and the `sandbox_run` arm (same file, ~line 2819) does `self.active_sandbox.replace(sb_handle)` — it REPLACES the active ceiling instead of intersecting with it. Any program running inside a sandbox can therefore mint a wider one for itself. Verified by running: a program whose outer wrapper is `principal_root(...); sandbox_create(__p, ""); sandbox_run(__sb, "__job_entry", 0)` (the exact shape axon-os generates) where __job_entry does `sandbox_create(p2, "Random,IO,Net,AI"); sandbox_run(sb2, "evil", 0)` printed "ESCAPED: got random 1" — `random_i64` executed under an empty ceiling. This is the fence axon-os's own comment calls "what actually contains" (runtime.rs:299-305), so the R21/R22 containment story has no floor today.

**Recommendation:** Make nesting monotone: in the `sandbox_run` arm, set the new active ceiling to the INTERSECTION of the requested sandbox's allowed set with the currently-active one (and keep the exemption only for that, not for sandbox_create). Add a regression test asserting an inner sandbox cannot widen an outer one.

---

### F161 — The guest syscall gate's ALLOW path triple-faults the VM; kernel_enforce_test's "permitted" case is vacuous and wired into nothing
*crates/axon-guest-kernel/src/enforce.rs* · kind: kernel-security

`crates/axon-guest-kernel/src/enforce.rs:295-323` (`run_program`) issues a real `syscall` ONLY on the deny branch. On the grant branch it prints "K5: policy GRANTS FS — open permitted" and halts without issuing anything. `scripts/kernel_enforce_test.sh` case 2 greps for exactly that print line and reports "✓ the gate PERMITTED the openat". It proves nothing.

I tested the real allow path: I copied the crate to scratch (no repo edits), changed only the grant branch to actually issue the same `syscall`, rebuilt, and booted under Firecracker with `{"allowed_effects":["IO","FS"]}`:

```
[axon-kernel] K5: policy GRANTS FS — AUDIT PROBE: actually issuing the syscall
--- (no further output) ---
[probe] firecracker EXITED (guest shut down / triple fault)
[fc_vcpu 0] Received KVM_EXIT_SHUTDOWN signal
Firecracker exiting successfully. exit_code=0
```

The "AUDIT PROBE: syscall RETURNED" line never prints. Cause is in `enforce::init` (lines 427-429): `STAR[63:48] = 0x20`, so `sysretq` loads CS=0x30 / SS=0x28 at CPL 3, but the boot GDT (`src/boot.s:132-138`) has only 5 entries (limit 0x27) and every PD entry is `0x83` (P|RW|PS — no U/S bit), so the first ring-3 instruction fetch #PFs with no IDT installed → triple fault.

Also note the handler itself (`syscall_entry`, lines 344-399) never *performs* any syscall — the "allowed" path just SYSRETs with rax=0. So even if SYSRET worked, `write` would return "0 bytes written" and `mmap` would return NULL. There is no syscall implementation behind the gate at all.

And `scripts/kernel_enforce_test.sh` — the script carrying the entire live-enforcement claim — is referenced by NOTHING: not gate.sh, not parity_all.sh (which globs only `scripts/*_parity.sh`), not integration_fixtures.rs, not CI.

**Recommendation:** Either build the ring-3 story properly (add user CS/SS GDT entries at the SYSRET-implied selectors, set U/S on the user pages, install an IDT, and actually dispatch the syscall) or delete the allow branch and rename the harness to `kernel_enforce_deny_test.sh` so the PASS line stops claiming "denies/permits by policy, live, end-to-end". Wire the script into `parity_all.sh` or a cargo test either way.

---

### F138 — axon-os reports "✓ completed" and exits 0 for guest exits 3 (@[verify]), 4 (kill-switch), and 5 (AI policy)
*crates/axon-os/src/runtime.rs* · kind: unreachable-code-path

`AxonCoreRuntime::run_sandboxed` (crates/axon-os/src/runtime.rs:377-414) classifies the guest run by grepping the child's stderr. There are branches for sandbox/capability, budget, refinement, timeout, and "axon: panic" — and nothing for "axon: verify failed", "axon: halted", or "axon: ai policy". Everything unmatched falls through to `Verdict::Completed { value: proc.code }`, and `Verdict::Completed` maps to exit 0 (verdict.rs:47).

Measured, three separate jobs, each guest verified to exit correctly on its own first:

  # guest alone
  $ axon run verifyfail.ax  -> axon: verify failed: ... ; exit=3
  $ axon run halt.ax        -> axon: halted: `act` refused: corrigibility kill-switch is latched ; exit=4
  $ axon run aipol.ax       -> axon: ai policy: [E1302] unknown AI tier `nonexistent` ; exit=5

  # same programs under the supervisor
  $ axon-os run verifyfail.axjob
  ✓ completed (value=3)  (run-id: run, record: .../run.json)
  exit=0
  $ axon-os run halt_io.axjob
  ✓ completed (value=4)  (run-id: run, record: .../run.json)
  exit=0
  $ axon-os run aipol.axjob
  ✓ completed (value=5)  (run-id: run, record: .../run.json)
  exit=0

The supervisor prints a checkmark for a program whose corrigibility kill-switch fired. The persisted `run.json` audit record also stores `verdict: Completed`, so the false success is what gets archived and re-verified.

**Recommendation:** Stop inferring the verdict from stderr prose. The guest is `axon run`, whose exit codes are a defined contract — have the runtime wrapper emit a structured, non-guest-writable status (e.g. a fd-3 or side-file JSON line written by the interpreter itself, or reserve the child's exit code by making the sandbox wrapper's `main` never return the job's raw value). At minimum, add explicit branches mapping guest exit 3 → a policy verdict, 4 → Halted, 5 → a policy verdict, before the Completed fallthrough.

---

### F013 — Contained job escapes the axon-os runtime sandbox by minting a wider sandbox (sandbox_create/sandbox_run are exempt from the ceiling check)
*crates/axon-os/src/runtime.rs:299* · kind: bug

crates/axon-os/src/runtime.rs:299-305 calls the sandbox wrapper "the sound fence… what actually contains". But crates/axon-core/src/interp/builtins.rs:191 exempts `sandbox_create` and `sandbox_run` from the active-sandbox effect check, `sandbox_create` has an empty effect row, and `sandbox_run` (builtins.rs:2846) *replaces* rather than intersects the active ceiling. So code already inside a ceiling can mint an arbitrarily wide one and run under it.

VERIFIED end-to-end on this checkout. Job manifest: fs_read=[], fs_write=[], net=[], exec="none", budget.calls=1. Program:

    fn pwn(_x: i64) -> i64 { let args = ["/tmp/PWNED2.txt"]  let r = exec ("touch", &args)  match r { Ok(_s) => 0  Err(_e) => 1 } }
    fn main() -> i64 { let p = principal_root("inner", true, true, true, 999999)
                       let sb = sandbox_create(p, "IO,Net,AI")
                       sandbox_run(sb, "pwn", 0) }

`axon-os run evil2.axjob` printed `✓ completed (value=0)`, exited 0, and /tmp/PWNED2.txt was created — an arbitrary process spawned under exec="none". Without the escape the same program is correctly refused (`sandbox violation: builtin exec requires effect IO … allowed set {}`, exit 8), so the escape is the whole difference.

**Recommendation:** Make sandbox ceilings monotonically narrowing: in the `sandbox_run` arm, intersect the requested sandbox's allowed set with the currently-active one (and refuse, not silently narrow, if the requested set is not a subset). `sandbox_create` should likewise be denied — or forced to intersect — while a sandbox is active. Add a regression test that a nested sandbox_create/sandbox_run cannot re-acquire an effect the outer ceiling withheld, and an axon-os acceptance test that runs a self-widening job under a zero-cap grant and asserts exit 8.

---

