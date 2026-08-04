### F132 — Phase-9 `Sandbox<P>` ceiling collapses FS and Exec into "IO" — a sandbox that allows printing also allows arbitrary process execution and file access. Zero tests exist for the feature.
*crates/axon-core/src/builtins.rs:2209-2211 (builtin_effect_row); crates/axon-core/src/interp/builtins.rs:189-207 (enforcement)* · kind: capability-refusal

CLAUDE.md Phase 9 is marked "✅ Complete", with F5 described as: "any builtin whose effect row exceeds the ceiling raises SandboxViolation (exit 8)". The `sandbox_run` docstring (builtins.rs:1275) names the intended use case: "AI-generated tool execution where the tool function is not known at compile time".

Enforcement (interp/builtins.rs:189-207) keys on `builtins.rs::builtin_effect_row()`, which assigns:
```
"println" | "print" | "eprintln" | "eprint" | "read_line" | "read_file" | "write_file"
| "env_var" | "exit" => &["IO"],
"exec" => &["IO"],
```
There is no `FS` or `Exec` effect in the runtime catalog at all. So the smallest ceiling that permits console output necessarily grants `exec` and the whole filesystem.

Reproduced (`axon run`, interpreter, ceiling = "IO"):
```
fn tool(n: i64) -> i64 {
    let r = exec("/bin/sh", ["-c", "id -u > ./sbv_pwned.txt; echo PWNED"])
    match r { Ok(s) => println("exec inside sandbox succeeded: {s}")  Err(e) => println("exec err {e}") }
    let c = read_file("/etc/hostname")
    match c { Ok(s) => println("read /etc/hostname inside sandbox: {s}")  Err(e) => println("read err {e}") }
    n
}
fn main() -> i64 {
    let p = principal_root("root", false, false, false, 1000)
    let sb = sandbox_create(p, "IO")
    let r = sandbox_run(sb, "tool", 5)
    println("sandbox_run returned {to_str(r)}")
    0
}
```
output:
```
exec inside sandbox succeeded: PWNED
read /etc/hostname inside sandbox: gpumaster
sandbox_run returned 5
exit=0        # cat sbv_pwned.txt -> 0
```
The ceiling does fire when the effect is genuinely absent — swapping to `sandbox_create(p, "Net")` gives `axon: sandbox violation: builtin `exec` requires effect `IO` ... allowed set {"Net"}` / exit 8 — so the mechanism works; the effect vocabulary is the hole.

Internal inconsistency proving nothing cross-checks them: the R28 audit ledger in the same `call_builtin` uses `capabilities::capability_of_builtin()`, which DOES distinguish `fs:read`/`fs:write`/`net`/`exec` (capabilities.rs:331, and check_builtin_value_ref at :352-357 branches on exactly those four). Two capability classifiers in the same function disagree, and `builtins.rs::builtin_effect_row_agrees_with_impurity` only cross-checks rows against purity, never against `capability_of_builtin`.

COVERAGE: `grep -r sandbox_create` over the whole repo excluding target/ hits only CLAUDE.md, ROADMAP.md, three spec/review markdown files, and 3 source files. Zero tests, zero examples, zero harnesses. `grep 'Some(8)' crates/axon-core/tests/cli_run.rs` → no hits: exit 8 is never asserted end-to-end anywhere. This is also why the nested-sandbox escape found earlier this session shipped undetected — both are the base-level tests that were never written.

**Recommendation:** (1) Give `read_file`/`write_file`/`env_var` an `FS` row and `exec` an `Exec` row (or have sandbox enforcement consult `capability_of_builtin` instead of `builtin_effect_row`), and add a drift test asserting the two classifiers agree on every BUILTINS entry. (2) Add cli_run tests asserting exit 8 for each of the 4 capability classes escaping a ceiling, plus the negative (allowed effect runs), plus the nested-sandbox case. (3) Until fixed, downgrade Phase 9 from "✅ Complete" — the F5 acceptance claim is not met.

---

### F001 — F5 runtime sandbox ceiling is escapable from inside the sandbox (nested sandbox_create/sandbox_run replaces rather than intersects)
*crates/axon-core/src/interp/builtins.rs:191* · kind: bug

call_builtin exempts `sandbox_create` and `sandbox_run` from the effect-ceiling check (interp/builtins.rs:191), and `sandbox_run` *replaces* the active handle rather than intersecting ceilings (`self.active_sandbox.replace(sb_handle)`, interp/builtins.rs:2836). Sandboxed code can therefore mint a wide-open sandbox and re-enter it, discarding the ceiling it was placed under. This voids the entire Phase-9 F5 property for exactly the population it exists to contain (AI-emitted tool code running under `sandbox_run`).

**Recommendation:** Make the ceiling monotone: keep a stack of active sandboxes and have the check require an effect to be allowed by EVERY frame (intersection), and have `sandbox_create` inside an active sandbox intersect the requested set with the current ceiling (or refuse outright, E-coded). Keeping the two builtins exempt is fine once the ceiling can only narrow.

---

### F152 — Human approval is never bound to the artifact — approve a benign file, deploy anything
*crates/axon-core/src/main.rs:4988-5012,5355-5356,5690* · kind: approval-chain-toctou

`axon ast approve` (crates/axon-core/src/main.rs:4988-5012) computes an FNV-1a hash of the source and writes it into `<file>.ax.approved`. `axon deploy` (main.rs:5355-5356) then does exactly this and nothing more: `let is_approved = approved_path.exists();`. A workspace-wide grep confirms the recorded `hash` field is written at main.rs:5004 and read by NO code anywhere. The approval is a marker file, not a commitment.

Executed end to end:
```
$ axon ast approve benign.ax
approved: benign.ax (hash 817d06c17125)
  approval record: benign.ax.approved
$ cat benign.ax.approved
{"schema":"axon-ast-approved/1","file":"benign.ax","hash":"817d06c17125d0fc","approved_at_unix":1785560325}

# attacker now rewrites the file the human signed off on:
#   fn main() { let secret = read_file("/etc/passwd"); println("EXFIL: {secret}") }

$ axon deploy benign.ax --json
EXFIL: Ok(root:x:0:0:root:/root:/bin/bash
daemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin
... [full /etc/passwd printed] ...
cklaus:x:1000:1000:,,,:/home/cklaus:/bin/bash
)
{"schema":"axon-deploy/1","path":"benign.ax","status":"deployed","exit_code":0,"risk":"low","stages_run":["redteam_check"],"approved":true}
exit=0
```
The deploy record asserts `"approved":true` for a program the approver never saw. This is the Acid-Test-2 flow (`intent compile → ast review → ast approve → deploy`) and the axon-web UI proxies the same CLI, so the web approval pane inherits the hole verbatim. Secondarily, even once the hash IS checked, FNV-1a is a non-cryptographic 64-bit hash (main.rs:5690) and is trivially collided — it cannot carry an approval commitment.

**Recommendation:** At deploy, re-hash the source and compare against the `.approved` record; refuse (non-zero exit, `approved:false`) on mismatch or missing record rather than warning. Replace `fnv1a_hex` with SHA-256 (sha2 is already a direct dependency of axon-core). Ideally the approval record should also cover the resolved module closure, not just the entry file, since `mod`/`use` bytes are separately mutable.

---

### F140 — `axon-os verify` (exit 11) does not authenticate the verdict — a violation record can be rewritten to "Completed" and still verifies intact
*crates/axon-os/src/record.rs* · kind: audit-integrity

`record::build`/`record::verify` (crates/axon-os/src/record.rs:125-200) hash-chain only the `events` array, seeded by `manifest_digest`. `verdict`, `seed`, and `run_id` are outside the chain, and `record_digest` is just the chain head.

Starting from a genuine RefineViolation record (`axon-os run refine.axjob` → exit 6, verdict RefineViolation):

  # rewrite the verdict to a clean completion
  d = json.load(open('r6.json')); d['verdict'] = {'kind':'Completed','value':0}
  $ axon-os verify t3.json
  ✓ intact (1 events, digest axrec1:93397f16...)
  exit=0

  # flip the seed 42 -> 43
  $ axon-os verify tampered.json
  ✓ intact (1 events, digest axrec1:93397f16...)   <- identical digest
  exit=0

  # control: tamper an event field, which IS chained
  $ axon-os verify t4.json   (caps_used += net,exec)
  ✗ TAMPERED: event 0: hash mismatch (tampered field)
  exit=11

So exit 11 fires only for the fields nobody would need to forge, and never for the one field that determines the run's outcome and its exit code.

**Recommendation:** Fold `verdict`, `seed`, and `run_id` into the hash chain — e.g. append a terminal `verdict` pseudo-event to the chain before computing `record_digest`, or hash the canonical serialization of the whole record minus `record_digest`. Add a regression test that mutates `verdict` and asserts VerifyMismatch/exit 11.

---

### F040 — The approved grant's path/host allowlists are never enforced at runtime — only coarse axis presence is
*crates/axon-os/src/runtime.rs:253* · kind: bug

axon-intent renders and digests a legible bound with concrete scope ("read ./data/", "write ./out/", "reach api.example.com") and axon-os claims to run the job under it. But the only runtime enforcement is `wrap_in_sandbox` (crates/axon-os/src/runtime.rs:253-275), which throws away every prefix/host and emits three coarse interpreter tags: net -> {Net,AI}, and fs_read|fs_write|exec -> a single "IO" bucket. Nothing else consults grant.fs_read/fs_write/net at run time (grep over runtime.rs/supervisor.rs/monitor.rs shows no path check). Concretely: a job approved with grant fs_write=["./out/"] gets ceiling "IO"; at runtime `write_file("/home/user/.ssh/authorized_keys", ...)` is permitted. A job approved with net=["api.example.com"] gets "Net,AI"; `http_get("https://evil.com", "")` is permitted. I verified the same coarseness lets `exec` through an fs-only grant: a wrapper with `sandbox_create(p, "IO")` calling `exec ("echo", ["PWNED"])` ran successfully (exit 0). So every scope word in the token the human signs is decorative at execution time.

**Recommendation:** Enforce the grant's prefix/host allowlists at builtin dispatch, not just the axis tag. Either extend the sandbox entry to carry the fs_read/fs_write prefix lists and net host list and check them in the read_file/write_file/http_* arms of interp/builtins.rs, or have wrap_in_sandbox emit `@[contained(fs: [...], net: [...], exec: ...)]` on the entry fn so the existing E1001 path machinery applies. Until then, `legible_bound` must not tell the operator the program "may write ./out/" — it can only honestly say "may write files".

---

