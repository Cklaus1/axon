# R41 — Axon Polyglot Runtime: sandboxed multi-language execution + mountable virtual filesystem

**Spec ID:** `R41-polyglot-runtime` (new requirement row; platform-vision bucket, alongside R16/R17/R18/R36-R38)
**Status:** Draft — strategic proposal; Slice 0 is a kill-gated spike, nothing past it opens without §12 Q1
**Risk class:** Structural (a new TCB surface — foreign-language process isolation — not a language feature)
**Author / date:** strategic-research agent, 2026-07-31 (ASI-trajectory review corrections folded in
2026-07-31 — entry-content pinning + write/exec overlap refusal, untrusted subprocess output, evidentiary
ledger + exec replay, aggregate Principal-debited ceiling, `exec: any` deprecation gradient, hardened jail
probe set + default-deny seccomp allowlist, R22/R24 approval binding; §12 Q7–Q9 opened)

```spec-meta
id: R41-polyglot-runtime
status-claim: Draft
depends-on: R6-capability-security, R13-native-ffi, R7b-axonhost
blocks: none
blocked-by: R41 §12 Q1 (feasibility spike must clear before Slice 1 opens)
supersedes: none
related: R38-embedded-agent-runtime, R26-confidential-microvm-substrate, R21-axon-os-supervisor, R22-intent-approve-gateway, R24-defended-approval-boundary, R25-information-flow-monitor, R11-capability-minting, R12b-kernel-goal, R28-capability-audit-ledger
conflicts-with: none
reserves: E20xx block (E2000-E2003), confirmed free (grepped against error.rs + all governance/specs `reserves:` lines at spec time — E1900 is R19's single code, E2300-E2302 are eBPF's, next contiguous free band starts at E2000)
evidence: none (Draft; Slice-0 gate is r41_ceiling_refuses_undeclared_language_or_mount per §9)
```

(Edge notes: `depends-on` = the landed machinery this spec extends rather than reinvents — `@[contained]`'s
allowlist/deny-by-default model and its transitive-laundering closure (R6), the `use native::M` capability-gated
import pattern and the opaque affine `Handle` type (R13), and the `AxonHost` trait + thread-local injection
seam that already routes every host touchpoint through a swappable trait (R7b). `related` holds R38 — same
"embeddable substrate" shape, opposite direction: R38 lets a host run *Axon* code with Axon's own capability
checker; R41 lets an *Axon program* run **other languages'** code inside a sandbox Axon's checker still
gates. R26 is related as the escalation tier for Slice 3 (see §9). *(Added 2026-07-31:* R22-intent-approve-gateway,
R24-defended-approval-boundary and R25-information-flow-monitor were absent from the original `related:`
line and are now load-bearing — R22 because the approval token binds `sha256(program source bytes)` of the
`.ax` file *only*, which an R41 entry script sits outside of (§5's entry-pin rule); R24 because R41 replaces a
one-token capability with a nested grant grammar that a human must still read and accept (§4's approval-surface
subsection, §1 hard-truth 5); R25 because a subprocess's stdout is the widest un-labelled channel back across
the sandbox boundary (§5's trust-label rule). R11/R12b are related because subprocess metering debits the same
Principal budget path rather than a private counter (§5's aggregate-ceiling rule).)

> **One-line thesis:** `@[contained]` already proves what an Axon program *may touch* — but only for
> builtins the compiler knows about. The moment an agent needs to `git clone`, run a legacy Python script, or
> invoke a native toolchain, that whole subprocess is invisible to the capability checker: an opaque `exec`
> grant, all-or-nothing. R41 extends the same allowlist/deny-by-default discipline one level down, into the
> subprocess itself — a typed, capability-gated **sandbox host** for running foreign-language code (Slice 1)
> and a typed, capability-gated **mount surface** for POSIX-style access to cloud storage (Slice 2) — so
> "shell out to Python" stops being a hole in the containment story and becomes another declared, checked,
> audited effect.

---

### 1. Motivation

`@[contained(exec: any)]` (or `exec: none`) is the entire *attribute* vocabulary R41 inherits — and that
vocabulary already has a live spawn primitive attached to it: the landed `exec(cmd, [args])` builtin, which
spawns an arbitrary unjailed, environment-inheriting process (see §5's legacy-exec rule for how R41's
structured grants interact with it). An Axon program either may spawn processes or may not. There is no way to say *"may run `transform.py` under Python with this exact
argv, no network, no filesystem beyond one mounted directory, capped at 2 CPU-seconds"* — the granularity
`fs`/`net` already have. In practice this pushes real automations toward one of two bad shapes: (a) refuse
`exec` entirely and reimplement everything the agent needs as Axon builtins (the stdlib-breadth tax, already
named as a gap in R38 §3), or (b) grant `exec: any` and lose the compile-time guarantee for the most
consequential capability axis — arbitrary code execution — right when it matters most.

Separately, `fs:` capability entries are path-prefix strings resolved against the local disk
(the `..`-component denial in `crates/axon-core/src/capabilities.rs` — `has_dotdot_component`, applied to
every fs allowlist check — closed the one traversal hole in that model). Real agent workloads
increasingly want the *same* typed, capability-checked file access pattern against object storage (S3) and
sync-drive folders (Google Drive), not just the local filesystem — without hand-rolling a bespoke SDK call
per provider inside every automation.

Both gaps share one shape: extend an *already-landed* allowlist/deny-by-default capability axis
(`exec`, `fs`) from a coarse boolean/path-prefix into a typed, provider-backed surface, using the seam R7b
already built (a trait behind a thread-local, swappable for tests/hosts) rather than threading a new
parameter through hundreds of call sites.

### 2. Requirement link

Opens a new `REQUIREMENTS.md` row **R41** (platform-vision bucket). No change to the type system or
existing `@[contained]` semantics for the axes it doesn't touch (`net`, `env` denial, `never`). Advances
ROADMAP §0 **Containment** (closes the `exec`/`fs` granularity gap the pillar has carried since R6) and
composes with R38 (an embedded host wanting to let generated Axon shell out to a legacy Python step needs
exactly this). Acceptance (full vision): *an Axon program declares a language + argv + resource ceiling for
a subprocess and a provider-backed mount for a storage path, the compiler refuses anything outside those
declarations before the program runs once, and violations are caught at the sandbox boundary — not by the
foreign process's own good behavior.* *(Sharpened 2026-07-31: "outside those declarations" must include the
**contents** of the entry script, not merely its filename — otherwise the claim holds only against an author
who did not write the script too. See §1 hard-truth 5 and §5's entry-pin rule.)* v1 acceptance is Slice 0 +
Slice 1 (§9).

> **Hard truths up front (same discipline as R18/R38):**
> 1. **The sandbox is the new TCB member.** Everything upstream (checker, effect rows, audit ledger) is
>    only as trustworthy as the process-isolation primitive underneath `exec_run`. Slice 0 must establish
>    which primitive (OS rlimits/seccomp/namespaces vs. a WASM-compiled language runtime vs. a real VM) is
>    actually being relied on, honestly, before any capability syntax is designed around it.
> 2. **Language coverage is not free.** "Run Python" in practice means either shelling out to a real
>    CPython (full OS process, real syscall surface, needs OS-level jailing) or embedding a WASM-compiled
>    interpreter (narrower syscall surface, but slower/incomplete stdlib, per language). These are different
>    engineering bets with different TCB shapes — R41 must pick one per language, not hand-wave "sandboxed."
>    Default position (subject to Slice 0 findings): **OS-level jailing (rlimits + seccomp + a scoped mount
>    namespace) for v1**, matching what a subprocess already needs regardless of language; WASM-per-language
>    is a Slice-3+ optimization for cost/density, not a v1 requirement.
> 3. **This is not a container orchestrator.** R41 is scoped to *one subprocess per `exec_run` call*,
>    capability-gated and resource-capped, not a general container runtime, not multi-tenant density
>    engineering. If a workload needs a persistent, stateful, long-running foreign process, that is out of
>    scope for v1 (§9 non-goals).
> 4. **Mount providers are credentialed integrations, not free.** S3/Drive mounts need the host to supply
>    real credentials somewhere; R41 defines the capability-checked *surface* (which prefixes may be
>    mounted, which providers, read vs write) but the credential-supply mechanism is the embedding host's
>    responsibility (mirrors `AxonHost::env_var` staying outside `@[contained]`'s reach by design).
> 5. **The v1 grant grammar assumes the program's author is scarce and the declaration is read.**
>    *(Stated limit, added 2026-07-31.)* Every other capability axis bounds **reach**: an `fs:` prefix bounds
>    which bytes a program may touch, a `net:` host bounds which endpoint it may reach. An `exec:` entry-path
>    prefix bounds only a **filename** — it says nothing about that file's contents. That is adequate when a
>    human writes both the attribute and the script and a human reads the grant before approving it; it is
>    *not* adequate against a generator that writes both (ROADMAP §2.1's authorship model), for which
>    `fs: write("./scripts/")` + `exec: [python("./scripts/")]` composes into arbitrary dynamic code behind a
>    literal argv. R41 therefore does **not** rely on that assumption: §5 pins entry-script content by hash,
>    statically refuses write/exec prefix overlap, and folds the entry digest into R22's approval binding.
>    Where an assumption of human review remains (the *legibility* of a nested grant — §4's approval-surface
>    subsection), it is named here as a limit and delegated to R24, not left implicit.

### 3. What is genuinely landed vs. derived vs. greenfield

| Polyglot-runtime need | Shipped? | Reality |
|---|---|---|
| Deny-by-default capability allowlist model (`fs`/`net`/`exec`/`never`) | ✅ shipped | `@[contained]` E1001/E1004, transitive (no helper laundering), path-traversal-safe |
| A swappable host-trait injection seam (thread-local, `set_host`/scoped override) | ✅ shipped | R7b `AxonHost` — exactly the mechanism a `SandboxHost`/`FsProvider` trait needs; no new injection design required |
| A capability-gated foreign-boundary import pattern (`use native::M`) + opaque unforgeable `Handle` | ✅ shipped | R13 — `exec_run` can reuse `Handle` for a live sandbox process (consume-on-wait, E0601 use-after-consume) exactly as R13's module handles already work |
| Effect-row tagging + anti-laundering (E1310) | ✅ shipped | Phase 6 — `exec_run`/mount calls get an effect row (`Exec`, `FS`) that composes with the existing subsumption checker, no new laundering hole |
| Capability audit ledger | ✅ shipped | R28 — `EffectKind::Exec`/`FS` already exist (`crates/axon-audit/src/lib.rs`); sandbox events append to the same hash-chained ledger, genuinely no new plumbing |
| Subprocess resource metering + mid-run kill-switch | ❌ **greenfield** *(row split 2026-07-31 — originally overclaimed as shipped)* | R3c's budget meter is specifically the per-fn `ai_complete` **call-count** budget (`governance/specs/R3c-ai-budget-meter.md` §1–2; even per-token cost was deferred) — nothing meters subprocess cpu/mem/wall into any budget. And R27's kill-switch is a latch checked at Axon call boundaries (refusal in `call_fn`): an interpreter blocked in `exec_wait` on a running foreign process cannot observe the latch until the process exits. Slice 1 must add the **latch-to-SIGKILL hook** (latch flip → SIGKILL to the jailed pid) so the corrigibility story stays true for long-running foreign processes — small, but real new plumbing |
| OS-level process jailing (rlimits/seccomp/namespace scoping) | ❌ **greenfield** | nothing in the tree does this today; R13's `native::` modules are in-process library calls, not spawned processes — this is the actual Slice-0/1 build |
| A typed sandbox-boundary schema (argv/env/resource ceiling as data the checker validates) | ❌ **greenfield** | derived from R38's `axon-embed` tool-registry idea (schema'd calls checked before run), but for subprocess argv/ceilings, not host tool functions |
| Mount-provider abstraction (S3/Drive as a `fs:` source, not just local paths) | ❌ **greenfield** | `fs:` today is exclusively local path prefixes; needs a `MountSource` enum + provider trait behind `AxonHost`'s pattern |
| WASM-per-language execution (cheap footprint, narrower syscall surface) | ❌ **greenfield, deferred** | Slice 3+; v1 uses OS jailing per §1 hard-truth 2 |

**Net:** the capability-checking, audit, and injection machinery is fully reused — this spec adds one new
primitive underneath it (a real process sandbox) and one new provider abstraction (mounts), not a new safety
model.

### 4. Surface

**`.ax` surface** — `exec` and `fs` gain structured forms; both remain backward compatible with the existing
`exec: none | any` and bare path-prefix `fs:` entries (a program using only the old forms is unaffected):

```axon
@[contained(
  exec: [
    python("./scripts/transform.py", sha256: "9f2c…", max_spawns: 4, total_cpu_ms: 8000),
    bash("./scripts/fetch.sh", sha256: "1ab7…", max_spawns: 1, total_cpu_ms: 2000),
  ],
  fs: [
    read(mount("./data/", s3("my-bucket/reports/"))),
    write("./out/"),
  ],
  net: none,
)]
fn run_etl() -> Result<i64, str> {
    let handle = exec_run("python", "./scripts/transform.py", ["./data/report.csv"], ExecCeiling { cpu_ms: 2000, mem_mb: 256, wall_ms: 10000 })?
    let outcome = exec_wait(handle)?
    write_file("./out/summary.txt", outcome.stdout)
    Ok(outcome.exit_code)
}
```

- `exec: [python(entry, sha256: …, max_spawns: N, total_cpu_ms: M), ...]` — declares which language runtimes
  may be spawned, which entry script is permitted, **which exact bytes that entry must contain**, and the
  **aggregate** spawn/cpu ceiling for the whole run (E2000 if the requested language or entry isn't declared;
  E2000 on a content-hash mismatch at spawn; exit 7 on aggregate exhaustion — see §5).
  **The entry declaration is NOT a reach bound** *(clarified 2026-07-31 — the original draft sold it as
  "mirroring `fs:`'s path-prefix allowlist", which is the one analogy that does not hold; see §1 hard-truth 5)*:
  an `fs:` prefix bounds which bytes a program may reach, whereas an entry declaration bounds only *which file
  is spawned*, and — with the `sha256:` pin — *what that file contains*. The spawned process's **reach** is
  bounded solely by the mount namespace, the net-deny jail, and `ExecCeiling`/aggregate ceilings. A bare
  directory-prefix form (`python("./scripts/")`, no `sha256:`) is retained only for the write/exec-disjoint
  case and is refused whenever it overlaps an `fs: write` prefix (§5).
- `mount(prefix, source)` inside an `fs:` entry — `source` is `local` (default, today's behavior), `s3(bucket_prefix)`,
  or `gdrive(folder_id)`; reads/writes against `prefix` route through the mounted provider instead of
  `std::fs` (E2001 if the mount source isn't registered by the host).
- `exec_run`/`exec_wait` — new builtins, registered via **R13's interp-only builtin pattern** (a `BUILTINS`
  entry + interpreter dispatch + codegen E0910 refusal), *not* CLAUDE.md's "fast path" — that path is
  specifically the native-extern recipe (an `axon-rt` `__axon_*` impl + a `BUILTIN_EXTERNS` `ExternSig` row
  declaring LLVM lowering), which would be wrong here twice over: these builtins are interp-only at Slice 1,
  and `exec_wait`'s struct return `{ exit_code, stdout, stderr }` disqualifies it from the plain-scalar shape
  anyway (an `ExternSig` row would also trip `drift_tests` into demanding codegen support). `exec_run` spawns
  and returns an opaque `Handle` (R13-style, consumed by `exec_wait`); `exec_wait` blocks until exit and
  returns `{ exit_code, stdout, stderr }`. Interp-only at Slice 1 (codegen E0910-refused, same posture as
  R13's native module calls). *(Corrected 2026-07-31: originally cited CLAUDE.md's fast path.)*

**Rust host-embedding surface** (mirrors `AxonHost`, plugs into the same thread-local seam — no new
injection mechanism):

```rust
pub trait SandboxHost {
    fn spawn(&self, lang: Language, entry: &str, argv: &[String], ceiling: ExecCeiling) -> Result<SandboxHandle, String>;
    fn wait(&self, h: SandboxHandle) -> Result<ExecOutcome, String>;
}

pub trait FsProvider {
    fn mount(&self, prefix: &str, source: MountSource) -> Result<(), String>;
    fn read(&self, path: &str) -> Result<Vec<u8>, String>;
    fn write(&self, path: &str, data: &[u8]) -> Result<(), String>;
}

pub enum MountSource { Local, S3 { bucket_prefix: String }, GDrive { folder_id: String } }
```

**Relation to the effect-row successor surface (E1316)** *(added 2026-07-31 — this section existed nowhere
in the original draft, which froze the entire v1 surface onto `@[contained]` without mentioning that the
tree already ships a deprecation notice against it):* `axon check --effects-strict` emits **E1316**
("`@[contained](...)` deprecation notice; prefer `| {…}` effect-row syntax",
`crates/axon-core/src/error.rs`), and R38 §3 calls Phase-6 effect rows the successor surface. R41's position
is: **effect rows tag *which* effects a function performs; `@[contained]` scopes *what those effects may
touch*** — rows have no parameter grammar (no path prefixes, no host allowlists, no `ExecCeiling`), so the
structured `exec:`/`mount` grants are capability *parameters* and land on `@[contained]`, the only surface
that carries parameters today. When the effect-row successor grows a parameterized form (e.g.
`| {Exec(python: "./scripts/"), FS(mount(...))}`), the structured grants defined here are exactly the
payload that carries over — the grammar is designed to be surface-portable (the grant *data model* in
`SandboxHost`/`FsProvider`/`MountSource` is independent of which attribute spells it). Until then, a program
using R41 syntax under `--effects-strict` will see E1316 like any other `@[contained]` user; that is
accepted, not a blocker. §12 Q6 tracks the migration story.

**Relation to the defended approval boundary (R24)** *(added 2026-07-31 — R41 replaces a one-token
capability with a nested, parameterized grammar while `related:` referenced no approval spec at all; R24
exists in this directory built on exactly the premise this raises — "against a capability-proven box, a
superhuman AI's highest-probability escape is persuading the human approver"):*

- **Legibility, not syntax.** An R41 grant renders in R24's pre-approval simulation (`axon-intent simulate`)
  in *reach* terms — which languages, which pinned entry digests, which mount sources and directions, which
  aggregate ceiling — never as raw attribute text. What a human accepts is "may run *this* Python program,
  at most 4 times, 8 cpu-seconds total, seeing only `./data/` (S3 `my-bucket/reports/`), no network."
- **Friction is risk-proportional and already Critical.** `exec` maps to Critical in the landed
  `axon deploy --risk` derivation table, so an `exec:` grant inherits R24's High/Critical friction path
  (multi-party / multi-sig): a structured exec grant **cannot be single-approved**.
- **Expiry and revocation apply.** Mount grants and exec grants participate in R24's default-deny +
  expiring/revocable regime — a stale S3 mount grant must not be permanent.
- **The entry digest is part of what is approved.** Per §5's entry-pin rule, entry-script `sha256:` values
  are folded into R22's binding (`program_digest`/an adjacent `artifact_digest`) and into R24's
  `grant_digest`, so approving a grant approves a *specific program*, and revocation/expiry have a concrete
  referent. §12 Q7 tracks which of the two digests carries it.

`DefaultSandboxHost` (native) implements `spawn`/`wait` via `std::process::Command` plus OS jailing
(rlimits + seccomp + a scoped bind-mount namespace on Linux; the jailing mechanism is the Slice-0 spike's
actual subject). `DefaultFsProvider` reproduces today's local-path behavior for `MountSource::Local` and
requires the embedding host to register credentials for `S3`/`GDrive` out of band (§1 hard-truth 4).

### 5. Semantics

- **Deny-by-default, as always — and fail-closed on dynamic targets:** an undeclared language, an undeclared
  mount source, or an argv/entry-path outside the declared prefix is refused at `axon check` (E2000/E2001,
  same class as E1001/E1004). A **dynamically-built** language/entry-path/mount-source argument (not a string
  literal at the call site) is *also* refused at `axon check` — matching `@[contained]`'s actual landed
  posture, which statically refuses non-literal fs paths and net hosts rather than deferring them to runtime
  (`crates/axon-core/src/capabilities.rs`: "a dynamically-built host cannot be statically verified against
  the sandbox; use a literal host", and the identical non-literal-path denial against a non-empty fs
  allowlist). Only the *refinement* machinery uses a static/runtime split, and R41 deliberately does **not**
  adopt it for these axes: relaxing exec — arbitrary code execution — to runtime enforcement by the
  not-yet-proven Slice-0 jail would be a posture downgrade dressed as continuity. Runtime enforcement (exit
  8, `SANDBOX_VIOLATION_EXIT_CODE`, already-shipped R21/Phase-9 code — no new exit code) remains as
  **defense-in-depth at the sandbox boundary itself** — the jail catching a spawned process's own escape
  attempts (§8 adversarial cases) — not as a substitute for the static literal-target requirement.
  *(Corrected 2026-07-31: originally claimed a static/dynamic split "the same as `@[contained]`" — the cited
  antecedent is static fail-closed, and this spec now matches it.)*
- **The entry is pinned by content, and write/exec prefixes may not overlap** *(added 2026-07-31 — closes the
  write-then-exec hole: §5's fail-closed rule refuses a dynamically-built argv, but a program whose argv is a
  **literal** and whose **code** is dynamic (`write_file("./scripts/t.py", ai_complete(prompt))` then
  `exec_run("python", "./scripts/t.py", …)`) passed every check in the original draft, making E2000 a proof that
  a declaration matches a string while the program's behavior was unconstrained — and inverting §2's acceptance
  claim):*
  1. **Static overlap refusal.** `axon check` refuses any `@[contained]` whose `exec:` entry declaration
     (or entry prefix) intersects any `fs: write` prefix in the same attribute — E2000. Both prefixes live in
     the same attribute, so this is a string-prefix comparison with no analysis: it is implementable at Slice 0
     as a crate-level refusal alongside E2000/E2002, and it is the cheap fix *now* that becomes a breaking
     change once the syntax freezes.
  2. **Content pinning.** A grant carries the entry script's content hash (`sha256:` in §4); `exec_run`
     re-hashes the entry at spawn time and refuses on mismatch (E2000). This reuses R6's already-landed
     content-hash-on-`axon add` discipline at the import edge (I-12) rather than inventing a mechanism, and it
     makes what a human approves a **specific program**, not a directory a generator can refill.
  3. **Approval binding.** Entry digests are folded into R22's approval binding
     (`crates/axon-intent/src/approval.rs`: `program_digest = "axsha256:"+sha256(program source bytes)` binds
     the `.ax` file *only*, and `verify_token` reports "program edited after approval (digest mismatch)"), so
     an entry script rewritten **after** approval and **before** the run is still caught. Without this, R41
     would move a program's semantics outside the artifact ROADMAP §2.4 declares to be *the* legal/audit
     contract ("the typed AST — not the English — is the legal/audit artifact"). §12 Q7 fixes whether the
     digest extends `program_digest` or lands as an adjacent `artifact_digest`.
- **Subprocess stdout/stderr cross back as untrusted data** *(added 2026-07-31 — §6's E2003 governs what may
  *cross* the boundary, i.e. representability; nothing governed the **trust** of what comes back, and §4's own
  worked example pipes `outcome.stdout` straight into `write_file`):* the foreign process is not only something
  to confine — its output is attacker-controlled, and stdout is the widest channel out of the sandbox (a
  prompt-injection payload into a downstream `ai_complete`, a skewed `goal_run` score turning the optimizer into
  the delivery vehicle, an interpolated path or argv elsewhere). `ExecOutcome`'s `stdout`/`stderr` therefore
  originate at the **lowest trust rung** of the landed lattice — `examples/stdlib/tainted.ax`'s source-trust
  level for AI/Net-sourced values, equivalently R25-information-flow-monitor's untrusted `Labeled` — and an
  explicit `trust_validate`/declassification step is required before those values may flow into an
  `ai_complete` prompt, an fs write, or another `exec_run` argv. R25's monitor is **not** a Slice-1 dependency,
  but the *shape* is committed now (a label-carrying `ExecOutcome` field, or a distinct return type) so it is
  not a breaking change to the one surface a human approves. §12 Q8 holds whether declassifying sandbox output
  is an audited privileged operation (R25's rule) or a plain call.
- **The ceiling is enforced by the sandbox, not honor system:** `ExecCeiling` (cpu_ms, mem_mb, wall_ms) is
  passed to the OS jail at spawn time (cgroups/rlimits on Linux), not merely checked after the fact —
  matching R27's "the bound is real, not advisory" posture. **Granularity honesty** *(added 2026-07-31)*:
  the default rlimit primitive cannot deliver sub-second cpu kills — `RLIMIT_CPU` is whole-second, and
  cgroups-v2 `cpu.max` throttles bandwidth rather than killing at a cumulative-cpu total. So under the
  rlimit primitive, `cpu_ms` is **rounded up to the next whole second**; where a sub-second ceiling
  matters, the host-side watchdog kill (poll + SIGKILL) blessed in §9's Slice-0 gate is the enforcement
  mechanism. Either way the bound is demonstrated by measurement against a non-cooperating process, never
  asserted from documentation.
- **The ceiling is also aggregate, and it debits a Principal** *(added 2026-07-31 — `ExecCeiling` was
  per-spawn with nothing anywhere bounding the NUMBER of spawns, which is not a generic resource-exhaustion
  nit but specific to how this project executes code):* `goal_run` / `for! maximize` calls an `@[adaptive]` fn
  repeatedly while hill-climbing, so an `@[adaptive]` fn that calls `exec_run` spawns N jailed processes —
  each individually within `ExecCeiling`, with **N chosen by the optimizer, not by the grant**. The same shape
  appears in `Schedule`-paced improve loops. Therefore: (a) the grant carries an **aggregate** field
  (`max_spawns: N, total_cpu_ms: M` in §4) so the ceiling a human approves bounds the whole run, not one call;
  (b) `exec_run` **debits the currently-active Principal's budget**, cost derived from the requested
  `ExecCeiling` (a larger ceiling costs more), reusing the landed R11 `mint`/attenuation and R12b `KernelGoal`
  path so exhaustion is the already-defined **exit 7 / E1604** rather than a new mechanism behind a private
  counter. This is what §3's greenfield metering row is scoped to, and it is what makes the meter compose with
  the rest of the kernel. Note the corrigibility asymmetry it also closes: the latch-to-SIGKILL hook below
  covers *one long-running* process, but a tight loop of *short* processes reaches the latch only between
  spawns — the aggregate ceiling is what bounds host CPU burned in the meantime.
- **Kill-switch reach into a running subprocess** *(added 2026-07-31, see §3's split row)*: R27's latch is
  observed at Axon call boundaries, which a blocking `exec_wait` never crosses while the foreign process
  runs; Slice 1 therefore wires the latch flip to SIGKILL of the jailed pid, so a corrigibility halt
  terminates live foreign processes too.
- **`Handle` affinity:** a `SandboxHandle` from `exec_run` is R13-style opaque and affine — `exec_wait`
  consumes it (E0601 on reuse), it cannot be forged or crossed with another handle type (E1802-class), and it
  cannot be indexed/arithmetic'd (E1803-class). No new handle machinery — R13's rules apply verbatim.
- **The jail policy is a default-deny syscall ALLOWLIST, and it is a versioned artifact** *(added
  2026-07-31 — §1 hard-truth 2 and §9 both said "rlimits + seccomp" without ever specifying allowlist vs
  denylist, which is the single most load-bearing detail of a seccomp policy and the difference between a jail
  that holds against **unknown** syscalls and one that holds only against enumerated ones; §7 rightly names the
  jail as a new TCB member, and a TCB member whose policy shape is unspecified is not yet a decision):* the
  seccomp filter is default-deny with an explicit allowlist, checked in as a versioned artifact that the
  `r41_sandbox_spike` gate diffs, so **widening the policy is a reviewable change rather than an invisible
  one**.
- **Mount containment resolves real paths, not lexical ones** *(added 2026-07-31)*: E2002 mirrors
  `path_has_dotdot`/`path_has_prefix` (`crates/axon-core/src/capabilities.rs`), which is a purely **lexical**
  `..`-component check. That was sound in its original context — no hostile process could plant anything inside
  the checked tree. Under R41 a spawned process *can write into a mount it holds*, so mount-relative path
  containment additionally resolves the real path (symlink-resolved, TOCTOU-aware at the point of use) before
  admitting an access; the lexical check is retained as a cheap pre-filter, not as the containment argument.
  This interacts with the entry-pin rule above: a hostile process that can write into a mount must not be able
  to reach the entry script or the Axon program's own inputs.
- **Mount reads/writes are still capability-checked FS effects:** a mounted S3 prefix is exactly as subject to
  `fs: [read(...)]`/`fs: [write(...)]` and the effect-row/audit-ledger machinery as a local path — the
  provider is a *source*, not a bypass.
- **The legacy `exec` builtin is refused under a structured `exec:` grant** *(added 2026-07-31 — the
  original draft, and the first fold-in pass, never mentioned it):* the tree already ships a landed
  `exec(cmd, [args]) -> Result<str, str>` builtin (`crates/axon-core/src/builtins.rs`, `IoKind::Exec` in
  `capabilities.rs`) that spawns an arbitrary **unjailed** process via `AxonHost::exec`
  (`DefaultHost` = `std::process::Command`, **parent environment inherited**), gated today only by the
  boolean `exec: none | any` axis. If a structured grant like `exec: [python("./scripts/")]` counted as
  "exec granted" for that builtin, the entire granularity story of §1 would collapse — the program could
  still `exec("curl", ["evil.com"])` unjailed and env-inheriting, contradicting this section's own
  no-ambient-environment rule. So: under a structured `exec:` grant, a call to the legacy `exec` builtin
  is **refused at `axon check` (E2000**: `unjailed exec() is not permitted by a structured exec grant —
  use exec_run, or grant exec: any`**)**. `exec: any` remains the *only* spelling that permits the legacy
  builtin, exactly as today (E1001 semantics for `exec: none`/`never: [exec]` are untouched). This is a
  deliberate, spec'd interaction with landed behavior — see §8's test case and §11's rollout note — not
  an accidental one.
- **`exec: any` gets a deprecation gradient, shipped with Slice 1** *(added 2026-07-31 — read as an incentive
  problem rather than a compatibility one, the rule above is one-directional: adopting
  `exec: [python("./scripts/transform.py", sha256: …)]` costs a declaration, an entry pin, a ceiling, an argv
  the checker refuses if non-literal, and the loss of a working builtin, while `exec: any` costs one token and
  keeps everything. A human who has read this spec picks the safe path because they were persuaded; a generator
  optimizing for the task passing picks the cheap path by construction, and R41 lands without §1's containment
  claim ever materializing):*
  (a) **`axon check --exec-strict`** — mirroring the landed `--effects-strict`/E1316 soft-deprecation
      precedent — refuses bare `exec: any` with a message pointing at the structured form;
  (b) **risk derivation rates `exec: any` strictly above** a structured, pinned, ceilinged, mount-scoped
      grant. The `axon deploy --risk` table already maps Exec→Critical, so this is a refinement of a landed
      table, and it makes the **Phase-11 gate chain** — not documentation — the thing pushing toward the
      structured form;
  (c) **stated trajectory:** notice now (`--exec-strict`), refused-by-default under a future flag. Recorded in
      §11 so the direction is on record before Slice 1 freezes the grammar. §13(c)'s own critique applies here
      — shipping a structured path nobody is pushed toward leaves the sharper problem untouched.
- **No change to `env`:** reading environment variables remains denied inside `@[contained]` (the ambient
  ungrantable-secret-channel rule — the `env_var` E1001 denial in `crates/axon-core/src/capabilities.rs`,
  documented in CLAUDE.md's `@[contained]` section) — a spawned subprocess inherits **no** environment
  by default; if a host needs to pass one credential through, that is an explicit, typed argv/stdin value at
  the call site, never ambient inheritance.

### 6. Error codes

New block **E20xx** (confirmed free — see spec-meta `reserves`):

| Code | Trigger | Message shape |
|---|---|---|
| E2000 | `exec_run` for a language/entry-path not declared in `exec:` | `language 'python' with entry './x.py' not permitted — declared: [...]` |
| E2000 | an `exec:` entry declaration overlapping an `fs: write` prefix (§5's write-then-exec rule, added 2026-07-31) | `exec entry './scripts/' overlaps writable fs prefix './scripts/' — a writable entry is not a bound; pin it with sha256:` |
| E2000 | entry-script content hash mismatch at spawn (§5's entry-pin rule, added 2026-07-31) | `entry './scripts/transform.py' does not match its declared sha256: — refusing spawn` |
| E2000 | a call to the legacy unjailed `exec` builtin under a structured `exec:` grant (§5) | `unjailed exec() is not permitted by a structured exec grant — use exec_run, or grant exec: any` |
| E2001 | `mount(...)` source not registered by the host / not declared in `fs:` | `mount source 's3(...)' not permitted at prefix './data/' — declared: [...]` |
| E2002 | mount-relative path escapes its prefix — lexical `..`-component pre-filter (mirrors `path_has_dotdot` in `crates/axon-core/src/capabilities.rs`) **plus real-path/symlink resolution** at the point of use, since under R41 a spawned process can write into its own mount (§5, added 2026-07-31) | `path './data/../secrets' escapes its mounted prefix` / `path './data/link' resolves outside its mounted prefix` |
| E2003 | `exec_run`/`exec_wait` arg or return value not FFI-representable across the sandbox boundary (mirrors R13's E1801) | `type 'T' is not representable across the sandbox boundary (allowed: scalars, str, [str])` |

E2003 governs values crossing **into or out of the foreign process** (argv, stdin, the raw stdout/stderr
byte streams); `exec_wait`'s `ExecOutcome` struct `{ exit_code, stdout, stderr }` does not itself cross the
boundary — it is assembled **host-side** from permitted boundary components (an i64 and two strs), so it is
not an E2003 violation of the spec's own return type. *(Clarified 2026-07-31.)*

Dynamic (non-literal) targets are refused *statically* (E2000/E2001 — see §5's fail-closed rule; there is no
runtime-check fallback for undeclared targets). Exit **8** (`SANDBOX_VIOLATION_EXIT_CODE`, already defined —
no new exit code needed) is reserved for the jail catching a spawned process's own boundary violations at
runtime (defense-in-depth, §5). Exhausting a grant's **aggregate** spawn/cpu ceiling is the already-defined
Principal-budget exhaustion — **exit 7 / E1604**, not a new code (§5's aggregate-ceiling rule).
*(Corrected 2026-07-31.)*

E2003 governs *representability* across the boundary; it says nothing about **trust**. The trust rule for
values coming *back* (stdout/stderr originate untrusted, explicit validation required before an AI prompt, an
fs write, or another argv) is §5's trust-label bullet, and a missing validation step is a
`Tainted`/R25-`Labeled` flow refusal, not an E20xx code. *(Added 2026-07-31.)*

### 7. Invariants touched

- **I-11 (capability boundary)** — *extended*, not weakened: `exec`/`fs` go from coarse to structured, but
  deny-by-default and transitive anti-laundering hold identically; a program cannot gain reach it couldn't
  already declare.
- **I-9 (no silent success on degenerate input)** — an unregistered language/mount source is a hard refusal
  (E2000/E2001), never a silent fallback to a broader grant — the same sound-by-refusal posture as codegen's
  E0910. *(Corrected 2026-07-31: originally cited "I-1 (refuse, don't downgrade)" — global I-1 is "Pipeline
  order is fixed"; no refuse-don't-downgrade invariant exists in the registry.)*
- **I-13 (provenance not opt-out-able)** — every `exec_run`/mount access is an audited capability event
  reusing R28's ledger **with the existing `EffectKind` tags** — `Exec` for `exec_run`/`exec_wait`, `FS` for
  mount reads/writes (`crates/axon-audit/src/lib.rs` already defines both). No new enum variants: this is
  consistent with §5's "the provider is a source, not a bypass" framing, and it sidesteps the hash-chained
  ledger-schema compatibility question that adding variants would raise (`axon-os audit verify` over old
  chains stays untouched). *(Corrected 2026-07-31: originally claimed "two new effect kinds", contradicting
  §5.)*
  **Strengthened 2026-07-31 — the no-new-plumbing claim was true but the invariant claim was too weak.**
  `LedgerEntry` (`crates/axon-audit/src/lib.rs`) carries `seq, ts_ms, principal, effect, operation: String,
  prev_hash, entry_hash` — a **free-text** operation and **no payload commitment**. So the hash chain proved
  only that *an Exec event occurred at a sequence position*: not which script, which argv, which ceiling, or
  what came back. R41 therefore requires the `operation` string for an exec event to be a **canonical
  commitment** over `(language, entry content hash, argv, ceiling, exit_code, sha256(stdout), sha256(stderr))`.
  No schema change and no new `EffectKind`, so the ledger-compatibility argument above survives intact while
  the entry becomes evidentiary — and the entry hash is then both the check-time pin (§5) and the audit record.
- **I-13, replay half — an R41 program must stay replayable** *(added 2026-07-31)*: the project's replay story
  (`axon trace --replay <run-id>` re-executing with the recorded seed, plus `AXON_AI_REPLAY` memoizing
  `ai_complete` by `(prompt, model)`) is the auditability backbone, and an un-memoized subprocess silently ends
  it — leaving the **most consequential** capability axis with a strictly weaker audit artifact than AI calls
  have. This matters more, not less, as generated-program volume rises: at volume nobody reads the program,
  they read the chain and replay the run. R41 therefore specs **`AXON_EXEC_REPLAY`** mirroring `AXON_AI_REPLAY`:
  memoize `(entry hash, argv, stdin)` → `(exit_code, stdout, stderr)`, record on first run, replay verbatim
  after — a direct port of already-landed code, scoped as a Slice-1 acceptance criterion (§9), not aspiration.
- **New TCB surface acknowledged, not hidden:** unlike every other capability axis today, `exec`'s safety now
  depends on an OS-level jailing primitive outside the compiler's own proof machinery (§1 hard-truth 1) — this
  is named explicitly rather than implied to be as strong as the compile-time-checked axes.
- **I-2 (interp/codegen parity)** — not violated: `exec_run`/mount builtins are interp-only at Slice 1
  (E0910-refused in codegen), same posture as R13.

### 8. Test plan

- [ ] Unit: `exec: [python("./scripts/")]` permits `./scripts/x.py`, refuses `./other/x.py` (E2000) and an
      undeclared language (E2000).
- [ ] Unit: a fn with a structured `exec:` grant that calls the **legacy `exec` builtin** is refused at
      check (E2000, §5's legacy-exec rule); the same call under `exec: any` still passes exactly as today.
- [ ] Unit: `mount("./data/", s3(...))` permits reads under `./data/`, refuses an unregistered provider
      (E2001) and a `..`-escaping path (E2002).
- [ ] Integration: `ExecCeiling` is enforced by the OS jail, not just checked — a script that busy-loops past
      `cpu_ms` is killed by the sandbox, not merely reported after the fact.
- [ ] Adversarial: a spawned Python process attempts to read outside its mount, open a socket with `net: none`
      declared, or fork past its process ceiling — all three caught at the sandbox boundary (not by the
      foreign process behaving), red-teamed the same way the flagship corpus red-teamed `@[contained]`.
- [ ] Integration: kill-switch reach (§5) — flipping the R27 latch while `exec_wait` blocks on a
      long-running foreign process SIGKILLs the jailed pid; the interpreter observes the halt on return
      (not only after the process's natural exit).
- [ ] Property: `Handle` affinity — `exec_wait` twice on the same handle is E0601; a `SandboxHandle` cannot be
      passed where an R13 native `Handle` is expected (E1802-class).
- [ ] Unit *(added 2026-07-31)*: **write-then-exec is refused at check** — `fs: [write("./scripts/")]` +
      `exec: [python("./scripts/")]` in one attribute is E2000 (prefix overlap), even though every argv at the
      call site is a literal.
- [ ] Integration *(added 2026-07-31)*: **entry pin holds** — an entry script edited between `axon check` and
      `exec_run` is refused at spawn (E2000, hash mismatch); an entry edited **after approval** fails R22's
      `verify_token` the same way an edited `.ax` file does today.
- [ ] Adversarial *(added 2026-07-31)*: a spawned process emits a hostile stdout payload; it cannot reach an
      `ai_complete` prompt, an fs write, or another `exec_run` argv **without** an explicit
      `trust_validate`/declassification step (§5's trust-label rule).
- [ ] Integration *(added 2026-07-31)*: **aggregate ceiling** — an `@[adaptive]` fn calling `exec_run` under
      `goal_run` halts on aggregate exhaustion (exit 7 / E1604) after `max_spawns`, not after unbounded spawns.
- [ ] Integration *(added 2026-07-31)*: **audit + replay** — the ledger `operation` for an exec event commits
      to `(language, entry hash, argv, ceiling, exit_code, sha256(stdout), sha256(stderr))` and the chain
      verifies; the same program under `AXON_EXEC_REPLAY` reproduces byte-identical output **without spawning**.
- [ ] Adversarial *(added 2026-07-31)*: a hostile process **writes a symlink into its own mount** pointing
      outside it; the mount check refuses via real-path resolution (the lexical `..` check alone does not catch
      this — §5's mount-containment rule).
- [ ] Journey *(added 2026-07-31)*: the ETL journey program below passes under `axon check --exec-strict`,
      and a bare `exec: any` variant is refused by it.
- [ ] Journey: an ETL-shaped `.ax` program (mount S3 input, spawn Python transform, write local output) runs
      end-to-end, audit-chain verifies, and a version with an undeclared mount source is refused before first
      run.

### 9. Acceptance criteria (per slice, kill-gated)

**Slice 0 — spike (feasibility, both halves must clear):** *(gate hardened 2026-07-31 — the original
gate's only measured criterion was a cooperative CPU busy-loop; the E2000/E2002 refusals are pre-spawn
path-string checks (implemented at Slice 0 as `axon-sandbox` crate-level refusals — the checker-integrated
E-codes of §6 arrive with Slice 1), and every hostile-confinement probe lived in Slice-1 acceptance, i.e.
AFTER §12 Q1 froze the syntax around an unproven jail. The hostile probes are now Slice-0 measured
criteria.)*
- [ ] `r41_ceiling_refuses_undeclared_language_or_mount` — a minimal `axon-sandbox` crate spawns one
      language (Python) under OS jailing (rlimits + seccomp on Linux) with a mounted local directory,
      refuses an undeclared entry path/language and an escaping mount path (crate-level refusals,
      E2000/E2002-shaped), and the ceiling is enforced (measured, not asserted) — a busy-loop script is
      actually killed at its cpu ceiling. Under the default rlimit primitive, `cpu_ms` is enforced at
      whole-second granularity (`RLIMIT_CPU` rounds up); a host-side watchdog kill (poll
      `/proc/<pid>/stat`, deliver SIGKILL) is **explicitly blessed** as an acceptable enforcement
      mechanism for sub-second ceilings, provided the kill is demonstrated by measurement against a
      hostile (non-cooperating, signal-ignoring) busy-loop — see §5's ceiling bullet.
      **Added 2026-07-31:** the same crate-level refusal set includes the **write/exec prefix overlap**
      refusal (§5's entry-pin rule 1) — a string-prefix comparison, near-free, and the cheap version of the
      fix that becomes a breaking change once §12 Q1 freezes the syntax.
- [ ] `r41_hostile_confinement_probes` — hostile probes per axis, measured on Linux CI **without**
      Docker/gVisor: (a) the spawned process attempts to read a file outside its mounted directory —
      blocked by the jail; (b) opens a socket under a net-deny jail — blocked; (c) fork-bombs past its
      process ceiling — bounded. These are the §8 adversarial cases pulled forward: the syntax must not
      freeze (§12 Q1) around a jail these probes haven't passed.
      **Probe set extended 2026-07-31** — the original three assume a careless rather than an optimizing
      adversary and omit the classes that actually defeat this configuration; the set additionally covers:
      (d) **io_uring** (submits operations that bypass syscall-entry seccomp filtering); (e) **`ptrace`**;
      (f) **`/proc` and procfs-mediated reach**; (g) **symlink / TOCTOU against the bind-mounted directory**
      (§5's real-path mount rule); (h) **write-back poisoning** — writing into the mount to corrupt the Axon
      program's own inputs or its entry script (§5's entry-pin rule).
- [ ] `r41_seccomp_policy_artifact` *(added 2026-07-31)* — the seccomp policy is **default-deny allowlist**
      (§5) and is checked in as a **versioned artifact** that this gate diffs, so a policy widening shows up
      as a reviewable diff rather than an invisible change.
- [ ] `r41_jailing_primitive_decision` — Slice 0 must answer, in writing, which jailing primitive v1 commits
      to (rlimits+seccomp vs. a container runtime dependency vs. something else) and what its actual escape
      surface is, before any capability syntax freezes around it. The decision doc must **cite the measured
      results of the tests above** — not vendor/kernel documentation — as its evidence.
      **Kill line:** if the measured probes (fs-read escape, net-deny socket, fork bomb, cpu-ceiling kill,
      plus the 2026-07-31 additions: io_uring, ptrace, /proc reach, symlink/TOCTOU, mount write-back) are
      **not all contained** on Linux CI without a heavyweight external dependency (Docker/gVisor), the
      wedge is re-scoped (e.g. to a container-runtime dependency, stated plainly per §13(a)) or killed
      outright — record the finding either way.
      **Framing (added 2026-07-31): passing the probes is a necessary floor, not an escape-surface
      characterization.** The decision doc must additionally state, in writing, the escape surface it did
      **not** probe — an enumerated probe list can only ever falsify confinement, never establish it. And
      because a one-time measurement is invalidated by kernel/toolchain bumps, the doc carries a
      **re-measure cadence**: the probe set re-runs in `r41_sandbox_spike` on every CI image/kernel bump, and
      the decision is re-affirmed (or reopened) at each such bump.
- [ ] **CI wiring:** the Slice-0 tests are registered as a named `scripts/gate.sh` stage
      (`r41_sandbox_spike`), so an environment-skip is visible in gate output as SKIP, never
      indistinguishable from a pass (the vacuous-pass guard: the stage asserts probes-run > 0).
- [ ] A green Slice 0 authorizes Slice 1; it does **not** authorize Slice 2/3.

**Slice 1 — multi-language sandboxed exec:** `exec_run`/`exec_wait` builtins, `exec:` structured `@[contained]`
syntax, E2000/E2003, OS jailing per the Slice-0 decision, for Python + Bash first (C/Rust added once the
jailing primitive is proven — they mainly need a compile step before the same spawn path, not new isolation
work), plus the latch-to-SIGKILL kill-switch hook and subprocess resource metering named in §3's split
row. Acceptance: `r41_exec_sandbox_journey` e2e + the adversarial red-team pass in §8 (including the
kill-switch-reach and legacy-`exec`-refusal cases).
**Slice-1 scope additions (2026-07-31), each with a §8 case:**
- [ ] **Entry-content pinning + write/exec overlap refusal** wired into the checker as real E2000s (the
      Slice-0 versions are crate-level), and entry digests folded into R22's approval binding (§5, §12 Q7).
- [ ] **Untrusted-by-construction `ExecOutcome`** — `stdout`/`stderr` carry the lowest trust label and
      require explicit validation before an AI prompt / fs write / argv (§5). R25's monitor is *not* a
      dependency; only the label-carrying shape is committed, so it is not a later breaking change.
- [ ] **Aggregate ceiling + Principal debit** — `max_spawns`/`total_cpu_ms` in the grant, `exec_run` debits
      the active Principal via the R11/R12b path, exhaustion is exit 7 / E1604 (§5). This is what §3's
      greenfield "subprocess resource metering" row is scoped to.
- [ ] **Evidentiary ledger + `AXON_EXEC_REPLAY`** — the exec `operation` string is a canonical commitment
      over `(language, entry hash, argv, ceiling, exit_code, sha256(stdout), sha256(stderr))`; a replayed
      R41 program reproduces byte-identical output **without spawning** and the chain verifies against the
      recorded digests (§7).
- [ ] **`axon check --exec-strict`** ships with Slice 1 (not later), the risk table rates `exec: any` above
      a structured grant, and the §8 journey program passes under `--exec-strict` (§5's gradient rule, §11).

**Slice 2 — mountable virtual filesystem:** `mount(prefix, source)` surface, `FsProvider` trait, S3 + local
providers first (Drive requires an OAuth-flow credential story that is host-embedding work, not core R41 —
may slip to Slice 3). Acceptance: `r41_mount_provider_parity` — S3-backed reads/writes behave identically to
local-path `fs:` entries from the checker's perspective (same E-codes, same audit shape).

**Slice 3 — tiered footprint (deferred, not v1):** WASM-per-language execution as a cheap default tier, with
escalation to R26's confidential microVM substrate for workloads whose derived risk (the already-landed
Phase-11 `risk_derive`/`pipeline_run` gate chain) crosses High/Critical. This is the "cheap default, real
isolation when it matters" property named in the original design discussion — sequenced last because it's an
optimization over a working Slice 1, not a prerequisite for one.

### 10. Performance budget

Slice 1: `exec_run` spawn overhead (jail setup + process fork) < 50 ms p95 on the reference host (Linux CI) —
this is the "not a 4.8 ms cold-start WASM sandbox" honest ceiling for OS-process jailing; if a workload needs
sub-10ms spawn, that is exactly what Slice 3's WASM tier is for, not a Slice-1 goal. `exec_wait` overhead
beyond the foreign process's own runtime: negligible (blocking wait, no polling). Mount reads/writes: within
2x of a direct SDK call to the same provider (proxying through the capability check, not re-implementing the
provider's protocol).

### 11. Rollout & rollback

Ships as a new crate (`crates/axon-sandbox`) plus the `exec_run`/`exec_wait`/mount builtins and the
`@[contained]` structured-syntax extension. Existing `exec: none | any` and bare-path `fs:` programs are
unaffected (additive syntax, not a breaking change to the attribute grammar) — with **one deliberate
behavior interaction with landed code** *(added 2026-07-31)*: a fn that adopts the *new* structured
`exec:` form thereby loses access to the legacy unjailed `exec` builtin (E2000, §5) — additive for
existing programs, a spec'd trade for programs opting into the structured grant.
**Deprecation trajectory for `exec: any`** *(added 2026-07-31 — recorded before Slice 1 freezes the grammar,
because the structured path being strictly costlier than `exec: any` means a generator optimizing for task
completion never adopts it; see §5's gradient rule):* **now** — `axon check --exec-strict` refuses bare
`exec: any` with a pointer at the structured form, and `axon deploy --risk` rates `exec: any` strictly above a
structured/pinned/ceilinged grant, so the Phase-11 gate chain is what applies the pressure; **later** —
`exec: any` refused by default under a future flag, with `--exec-strict` inverted to an opt-out. This mirrors
the landed E1316 + `--effects-strict` soft-deprecation precedent for a superseded surface; no existing program
breaks at either step without an explicit flag.
Slice 0 is a spike branch;
killing it deletes one crate and leaves the language untouched. Slice 1/2 are interp-only and additive to
`builtins.rs`/`checker.rs` — rollback is deleting the new match arms and error codes, no migration needed
since nothing existing depends on them.

### 12. Open questions

1. **(THE gate) Which OS jailing primitive does v1 commit to, and does it actually hold?** rlimits+seccomp is
   the default assumption (§1 hard-truth 2) but Slice 0 must measure real escape surface, not assume it from
   documentation. If the honest answer is "we need gVisor/Docker as a dependency," that changes R41's
   deployment story materially (no longer "one npm/cargo install," now "requires a container runtime") —
   decide before Slice 1 freezes the capability syntax around an assumption.
2. **Language list for v1** — Python + Bash first is proposed (§9); is C/Rust (which need a compile step,
   not just an interpreter spawn) in Slice 1 scope or deferred to a Slice 1b? Compiling untrusted code adds a
   toolchain-invocation surface (another subprocess) before the sandboxed run even starts.
3. **Credential supply for S3/Drive mounts** — R41 defines the capability-checked surface; who holds the
   actual bucket/OAuth credentials, and how do they reach `FsProvider` without becoming another
   ambient-secret hole of the shape `@[contained]`'s `env_var` denial closed (E1001,
   `crates/axon-core/src/capabilities.rs`)? Likely answer: explicit host-side registration at
   `FsProvider::mount` time (out of the Axon program's reach entirely), mirroring how R38's tool registry
   keeps host credentials out of generated code's hands — needs to be spec'd precisely before Slice 2.
4. **Does this compose with R38's embed API, or are they the same crate?** A host embedding Axon (R38) that
   also wants its generated Axon to shell out (R41) needs both `axon-embed` and `axon-sandbox` — worth
   deciding now whether `axon-sandbox` is a dependency `axon-embed` re-exports or a fully separate opt-in,
   since R38 Slice 2's frozen embedding profile (R38 §12 Q3) will need to say whether `exec_run`/mounts are
   in or out of that frozen surface.
5. **Tiered footprint (Slice 3) risk-gate wiring** — does escalating to R26 microVM isolation hook into the
   existing `risk_derive`/`pipeline_run` chain directly (treating "spawn a foreign-language sandbox" as an
   effect-row input to risk derivation the same way `Exec`/`Net`/`FS` already are), or does R41 need its own
   risk axis? Default: reuse `risk_derive` unmodified — `exec` already maps to Critical risk in the existing
   derivation table (`axon deploy --risk`), so a naive reading is Slice 3 needs no new risk-typing work at
   all, only the escalation *action* (mount a heavier sandbox) wired to an already-computed risk level.
   Confirm this holds once Slice 1 exists to test against.
6. **(added 2026-07-31) Effect-row successor migration** — §4's E1316 note commits to `@[contained]` as the
   parameter-carrying surface for now and sketches a parameterized-row future
   (`| {Exec(python: "./scripts/"), FS(mount(...))}`). Before Slice 2 freezes the mount grammar, decide
   whether the parameterized-row form is spec'd alongside it (so both surfaces freeze together) or the row
   form is explicitly deferred with `@[contained]` blessed as long-lived for capability parameters — and
   whether E1316's message should be softened for parameterized grants that have no row equivalent yet.
7. **(added 2026-07-31) Where does the entry-script digest live in the approval binding?** §5's entry-pin rule
   requires an entry's content hash to survive into what a human approved, but R22's
   `program_digest = "axsha256:"+sha256(program source bytes)` (`crates/axon-intent/src/approval.rs`) binds the
   `.ax` file *only*, and R24 has its own `grant_digest` over the canonical grant. Decide before Slice 1
   whether entry digests (a) extend `program_digest`'s preimage, (b) land as an adjacent `artifact_digest`
   alongside it, or (c) ride entirely inside `grant_digest` since they are written in the grant. (b) is the
   current lean — it keeps `program_digest` meaning exactly "the `.ax` source" while giving `verify_token` a
   second thing to check — but this touches a landed, chain-verified structure and must be decided, not
   defaulted.
8. **(added 2026-07-31) Is declassifying sandbox output a privileged, audited operation?** §5 makes
   `ExecOutcome.stdout`/`stderr` untrusted-by-construction and requires an explicit validation step before an
   AI prompt / fs write / argv. R25-information-flow-monitor's model makes declassification a **privileged**
   operation (audited, principal-scoped); `examples/stdlib/tainted.ax`'s `trust_validate` is a plain call.
   Decide which applies here — a plain call is cheaper and composes with the landed stdlib, but it makes the
   one step that undoes the sandbox's output boundary invisible in the ledger.
9. **(added 2026-07-31) The motivating workload is runtime-composed argv, which §5's literal rule cannot
   serve — where does that pressure land?** §1 motivates R41 with "an agent needs to `git clone`, run a legacy
   Python script, or invoke a native toolchain", but §4's example and §5's semantics serve only the
   *fixed-pipeline* case: language, entry and mount source must be literals, and a dynamically-built one is
   refused at `axon check`. That is the right v1 call and §5 defends it well — **do not soften it**. It also
   means the workload most likely to drive demand (an agent deciding at runtime *which* repo to clone, *which*
   argument to pass) is exactly what R41 statically refuses, so pressure will accumulate on §5 to add a
   runtime-checked path, which §5 itself calls "a posture downgrade dressed as continuity". No answer is
   invented here, but a candidate is sketched so one exists before the pressure arrives: a **parameterized
   argv template with typed holes** — e.g.
   `exec: [python("./scripts/fetch.py", sha256: "…", argv: [repo_url where is_https_host(_, "github.com")])]`
   — where the *template* is a literal (so the shape of what may run is fixed at check time and legible to an
   approver, preserving §5's posture) and each runtime hole carries a **refinement predicate** the checker
   discharges. This invents no machinery: refinement types have all four obligation sites closed, SMT discharge
   is wired into the default pipeline for what it can prove, and the exit-6 runtime fallback covers what it
   cannot. Deciding this before Slice 1 costs a paragraph; retrofitting it after `exec_run`'s signature ships
   is a breaking change to the one surface a human approves.

### 13. Alternatives considered and rejected

**(a) A generic container-runtime dependency (Docker/gVisor) as the v1 isolation primitive.** Strongest
isolation with the least new code, but breaks the "no new heavyweight dependency" posture every other Axon
target has held (native codegen is a `cc` linker away; wasm targets need nothing external) and makes
`cargo build -p axon-core` no longer sufficient to exercise this capability. Rejected as the *default*; kept
as the honest fallback if Slice 0 finds rlimits+seccomp insufficient (§12 Q1) — in which case R41 should say
so plainly rather than ship a weaker guarantee under the same capability-checked syntax.

**(b) WASM-per-language as the v1 execution model (skip OS jailing entirely).** Matches the original
per-agent-footprint design point (cheap, narrow syscall surface) but requires a WASM-compiled interpreter per
target language — mature for some (a WASI Python exists but is slow/incomplete stdlib-wise), unproven for
others (Bash, C toolchains). Committing to this for v1 risks shipping "sandboxed Python that can't do half of
what real Python scripts do," which is worse than an honest OS-process jail. Rejected as v1; retained as
Slice 3's cost/density optimization once Slice 1 has proven the capability-syntax and audit shape against a
real (if heavier) isolation primitive.

**(c) Treat `exec: any` as sufficient and only build the mount surface (Slice 2 without Slice 1).** Real
value on its own (S3/Drive as typed `fs:` sources is useful even for programs that never spawn foreign
processes), but leaves the sharper problem — arbitrary code execution as an opaque, ungranular capability —
untouched. Rejected as the sole scope; kept as a valid *sequencing* option if Slice 0's jailing spike takes
longer than the mount-provider work (§12 Q1 vs. mount work are independently gateable).
