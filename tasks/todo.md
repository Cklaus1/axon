# Todo — derived view of commit history

Not a plan. This is what the commits on `governance-audit-2026-07-18` actually
show, so it can be rebuilt from `git log` if this file is lost.

## Done

| task | commit | what |
|---|---|---|
| Step 0 | `47e79c3` | baseline pinned: 1 failing test, not 2 |
| Step 1 | `47e79c3` | adversarial triage of the 20 criticals |
| Step 1b | `9d3af95` | full 185-finding triage folded into the DAG |
| **T1** | `d74d04c` | nested sandbox may only narrow the ceiling (F041, F013) |
| **T8** | `71e135b` | an IO grant no longer implies process spawn (P6-COV-01) |
| **T4** | `ac1a590` | `scan_effects` not evaded by whitespace / `mod` (OSK-P4-H1) |
| **T2** | `870469a` | `@[contained]` not launderable via string dispatch (F153) |
| T5+T6 | `1a89758` | MIT LICENSE added; README test count corrected |
| **T3** | `4dd69e2` | fs prefixes + net hosts actually constrain (OSK-P4-C2, F014, F040) |
| T7 | `aba53a2` | CI covers 19 crates + codegen/parity, not 1 crate and no codegen |
| **T9** | `3e579ff` | `axon fmt` no longer deletes `mod` declarations (P5-ECO-01, P5-31) |
| **T10** | `c0c4d55` | deploy approval binds the program text, not the filename (P7-SEC-01) |
| **T11** | `5a3282f` | link no longer cwd-dependent; un-skipped a dead parity gate (P5-15, DOC-01) |
| T12a | `88cd2e4` | guest image builds again (json-target-spec + EXIT trap scope) |
| O006 | `570dd95` | meta-test: harness success markers must be strings their scripts emit |
| **T13** | `de03d7f` | attest refuses an unverifiable hardware root-of-trust claim (OSK-P7-C1, partial) |
| O006b | `c9c5491` | harness skips countable + fatal under `AXON_HARNESS_STRICT` |
| **T15** | `c79034a` | `--features serde-json` compiles again (`axon lsp`/`parse --json`) + gate stage |
| **T14** | `6426c75` | wasm-parity corpus fixed — **suite now 426 passed / 0 failed** |
| O009 | `e459558` | drift test: the pure-compute corpus can no longer silently widen |
| O010 | `9cb7859` | persistent-bandit fixed /tmp path removed (mechanism undiagnosed — see correction) |
| O011 | `110`-era | persistent-learner same hardening (explicitly NOT a bug-fix claim) |
| **T16** | `—` | host_await worker gets a deep stack: abort/exit-134 → graceful limit (INTERP-H03) |
| **T17** | `1289b74` | deploy gates accept 0/1-arg (P5-01, Acid Test 2 works); legacy approvals ≠ tampering |
| O012 | logged | F062 per-call AI tier ignored natively — attempted, reverted, needs a `walk_expr` refactor first |
| **O004** | `f9f3869` | all wasm harnesses take the shared build lock — **suite 430/0, no known flakes** |
| **T19** | `—` | checked arithmetic in the guest-kernel bump allocator (OSK-L02) |
| **T20** | `—` | unknown principal handle refused, not silently treated as root (F006) |

### axon-os supervisor sweep — found by EXECUTING code-read findings

| task | commit | what |
|---|---|---|
| **T23** | `—` | `AXON_*` operator controls forwarded through `env_clear` — the R28 ledger was never written for ANY supervised run; mock/replay were inert (OSK-P4-H4) |
| **T24** | `—` | a job that never ran no longer seals as `Completed` — parse failure sealed `Completed{2}`, AI-policy refusal `Completed{5}` (O016/O018) |
| **T25** | `—` | child pipes drained concurrently — >64 KiB output deadlocked, blamed as `Denied{time}`; 30s → 0.50s (OSK-P4-H3) |
| **T26** | `—` | `axon-os kill` trips every live latch — `--killable --monitor` polled nothing while printing "🛑 kill tripped" (OSK-P4-H6) |
| **T27** | `—` | compliance monitor drains the ledger before exiting — violations in the last ~100 ms were reported as a clean run (OSK-P4-H5) |

### Formatter — found the same way (running the tool, not reading it)

| task | commit | what |
|---|---|---|
| **T28** | `—` | `axon fmt` no longer emits source it cannot re-parse. `1e400` overflows to infinity at lex time; the renderer wrote the bare word `inf`, then appended `.0`, producing `inf.0` — so formatting a VALID file produced one that fails `E0001 cannot find name 'inf'`, in place, exit 0. Also `1e100` → a 103-char literal. (O021) |

The four `examples/*.ax` files dirty at session start were not an edit — `axon fmt`
on a clean checkout reproduced them byte-for-byte. Restored to committed state.

**The test gap matters more than the bug.** `fmt.rs` already had five round-trip
tests; every one asserts `out1 == out2` — idempotence only, never comparing
against the input. A stable-but-wrong rendering passes all five. Added
`assert_float_fidelity` (output must parse; every float must survive with an
identical bit pattern). Third data-loss bug in this one tool — `emit_program`
still carries the `AUDIT T9` note about `mod` declarations having been silently
DELETED — which argues the AST-based architecture is the problem, not any arm.

### Purity / safety-flag sweep — executed-repro first, per the re-triage result

| task | commit | what |
|---|---|---|
| **T29** | `—` | `@[pure]` could launder any impure operation through a lambda — the purity walker listed `Expr::Lambda` in its terminal LEAF arm, so it never looked inside a closure body. `arr_fold(xs, 0, \|a, x\| { println("boom") a + x })` in a `@[pure]` fn checked exit 0, while the same `println` written directly is E1207 (P4-FE-01) |
| **T29** | `—` | the four R15 `host_await*` builtins were in neither `is_impure_builtin` nor `builtin_effect_row`, so `@[pure] fn g() -> str { host_await("hi") }` checked clean — they suspend to the host and resume with its reply, which is I/O plus unbounded nondeterminism (P4-INT-02) |
| **T30** | `—` | `axon deploy --risk criticl` (one typo) reported `risk:"low"`, `stages_run:[]`, `status:"deployed"`, exit 0. `.unwrap_or(0)` collapsed an unparseable level to the WEAKEST one and the `== -1` guard beneath it was dead code — `parse_risk_level` returns Option and never yields -1. Fail-open on a safety flag (P4-PROD-05) |

T29 is the transitive-laundering class again, third instance after `@[contained]`
and `@[sensitive]`: a guard that inspects only the immediate body is escapable by
moving the work one hop. Worth a systematic sweep of every walker's leaf arm
rather than waiting for the next report — `Expr::InlineAsm` is still a leaf there.

### Attestation-chain integrity

| task | commit | what |
|---|---|---|
| **T31** | `—` | chain **truncation** was undetectable in both verify paths. `chain.rs`, the CLI help and the R34 spec all claimed removal was detectable; only INTERIOR deletion was, and only interior deletion was tested. Chopping the tail off a 3-entry chain reported `CHAIN OK: 1 entries` exit 0; erasing it reported `CHAIN OK: 0 entries`; truncate-then-re-export reported `EXPORT OK`. Added `--expect-head`/`--expect-count` pins (OSK-P7-H3 / P7-KRN-06 / P6-COV-02) |

Linkage cannot catch truncation unaided — every prefix of a valid chain is a
valid chain, and `--genesis` pins the ROOT while truncation moves the TIP. The
pin has to come from outside, so **nothing yet stores one**: where the tip should
live (R33 quorum state / R28 ledger / external attestation) is on the
needs-human list, not something a local fix can close.

### Boot attestation — the R26 gate was not actually mandatory

| task | commit | what |
|---|---|---|
| **T32** | `—` | `axon-vm run`'s "mandatory kernel attestation" had two bypasses, both reproduced against the built binary against a tampered kernel. **(a) Trust-on-first-use.** A missing `~/.axon/kernel_baseline.sha256` made the CURRENT kernel the baseline — `attestation: baseline established`, exit 0, tampered image booted. The file is user-writable at a predictable path, so anyone who can swap the kernel can `rm` the pin. **(b) An env var.** `AXON_CI_NO_KVM=1` — inherited, not a flag, not typed at the boot site — short-circuited the gate to `Ok` on any host, including with a correct baseline present (P7-KRN-05 / P4-OS-16) |

Fixed by making a missing pin a REFUSAL (exit 10, `no pinned baseline`), removing
the env bypass entirely so `--no-attest` (which warns) is the only one, adding
`attest --pin-baseline` as the explicit operator action that blesses a kernel
(refused for a mock/absent kernel; overwriting needs `--repin`), and adding
`run --expect-digest <sha256>` so the pin can come from off the box entirely and
outrank a planted baseline file. `REQUIREMENTS.md:72` claimed "mandatory kernel
attestation before every boot" throughout; that row now records when it became
true.

The pattern from T29/T31 again, from a third angle: **the bypass was in the
same function as the check.** Both `test_attest_ci_mock_mode` and the TOFU branch
were tested — as features, asserting they returned `Ok`. A test can pin a hole
open just as easily as it can catch one.

`scripts/axon_kernel_gate.sh` Layer 3 now pins the kernel it just built rather
than relying on TOFU. That gate is red on this host for an unrelated,
pre-existing reason (O025: a halted guest hangs `axon-vm run` forever — same
under `--no-attest`).

### Guest outcome vs launcher plumbing — the microVM reported its own success

| task | commit | what |
|---|---|---|
| **T33** | `—` | `axon-vm run`'s `"ok"` was `result.is_ok()` — whether the LAUNCHER drove the Firecracker API, not what the guest did. The headline containment case, an agent whose `openat` the in-guest syscall gate DENIES, reported `{"ok": true, "exit_code": 124}` and **process exit 0**, after burning the full 45s deadline. Three causes: the guest's `-VIOLATION8` serial sentinel had no parser in `axon-vm` at all; the guest's ACPI-S5 power-off (port 0x604) is a no-op under Firecracker, so it span in `hlt` until killed; and `--json` printed and fell off the end of `cmd_run`, so `axon-vm run --json` exited **0 unconditionally** whatever the guest did (P7-KRN-04) |

Before → after on the same command, same image:

```
BEFORE  {"ok": true,  "exit_code": 124}                    process exit 0   20.2s
AFTER   {"ok": false, "exit_code": 8, "guest_outcome":     process exit 8    0.16s
         "violation"}
```

Fixed on both sides. Host: the serial drain watches for `-VIOLATION8` /
`-PANIC<n>` / `-EXIT<n>`; the guest's own verdict is authoritative and is acted on
even if it then fails to power off (a substrate that ignores every shutdown
mechanism must not be able to turn a refusal into a success); `ok` means the guest
reached a definite end and exited 0; `--json` propagates the exit code; schema →
`axon-vm-run/2` with a new `guest_outcome` field. Guest: shut down via **i8042
reset (0x64 ← 0xFE), which is what Firecracker actually implements** — hence
`reboot=k` in the boot args — keeping S5 and the ISA debug-exit device as QEMU
fallbacks. A clean run went from a 45s timeout to 157 ms.

**`axon_kernel_gate.sh` Layer 3 had never once observed the violation it tests.**
`run` derives the guest policy from the program's `.axmeta`, and the evil agent
cannot have one — the compiler refuses it (3× E1001), so `build --emit-manifest`
produces nothing, and with no manifest the policy defaults to OPEN. The check was
passing FS to an agent whose whole purpose is to be denied FS; it failed on a
timeout, not on a denial. The gate now states the grant explicitly. That default
is logged as **O026** — unfixed, because closing it changes every unmanifested
run and is an operator's call, not a bug fix to slip into an unrelated commit.

### Deploy risk + gates — the incentive was inverted and the gates were open

| task | commit | what |
|---|---|---|
| **T34** | `—` | `derive_risk_from_ast` read only DECLARED effect rows and `@[contained]` args on top-level fns. Executed: a program that really `exec`s a shell and `write_file`s, declaring nothing, derived `risk:"low"` and deployed (both side-effect files appeared on disk); a program declaring `\| {Exec}` that does nothing derived `critical`. **Declaring your effects honestly made you look dangerous; hiding them made you look safe** (P7-SEC-06) |
| **T34** | `—` | at Risk ≥ High a MISSING pipeline gate counted as a passed one, and the gates were read from the very file under review. A Critical-risk deploy ran with `stages_run:[]`, exit 0, and executed the program. `main.rs`'s own comment conceded the open gate. A program cannot be its own red team (P7-SEC-07) |

Risk is now the **max** of what the program does — via
`capabilities::program_capabilities`, which already had the right answer and was
never consulted — and what it declares. That walker follows impl-method bodies,
string-dispatch call sites and comptime initializers, i.e. exactly the laundering
routes the old top-level item-walk missed by construction. A declaration can
still only RAISE. The dead `@[contained]` attr-arg scan (parsed into `f.contained`,
never left in `attrs`) is gone.

At Risk ≥ High a missing gate now BLOCKS (exit 1, `blocked_gate`,
`missing:<names>`), and the blocked deploy does not run the program — verified by
its absent side-effect file. `--gates <path>` lets the DEPLOYING operator supply
gates from outside the artifact; `--allow-missing-gates` restores the old
behaviour with a warning on every run, the same single-explicit-bypass shape as
`--no-attest` in T32. Below High the open-gate convention is unchanged — the
finding is about the tier where the pipeline is claimed to be mandatory.

**Two existing tests encoded the defect as expected behaviour** and had to be
corrected, not worked around: my own T30 test asserted `--risk high` deploys
exit 0, and `r33_acceptance_gate.sh` deploys `hello.ax` at `--risk high`
expecting success. Both now pass `--allow-missing-gates`, because both are
measuring something else (risk-level parsing; the quorum gate) and would
otherwise be measuring this instead.

### AI runtime — a capability bypass and a mock gap, both REPRODUCED first

Both were code-read-only findings ("Not reproduced live"; "confidence rests on
code reading"). Per the re-triage result, they were reproduced before any edit.
Both held — but only after being made concrete.

| task | commit | what |
|---|---|---|
| **T35** | `—` | **The host the checker approves and the host the runtime dials were independent values.** `axon check` validates `ai_complete` against the implicit constant `api.anthropic.com`; `base_url()` resolved `AXON_AI_BASE_URL`/`ANTHROPIC_BASE_URL` at call time, and `load_dotenv_once` walked from the working directory **up to the filesystem root** looking for `.env`, setting whatever it found into the process env. Reproduced end-to-end against a local sink: an agent whose `@[contained(net: ["api.anthropic.com"])]` passes `axon check` **exit 0** sent its prompt and the real `x-api-key` to `127.0.0.1:8931`, from a `.env` one directory ABOVE the program (RT-02) |
| **T35** | `—` | `complete_structured_inner` went straight to `api_key(p)?` and a live billed POST with no mock check in it or any caller — only the plain `ai_complete` path honoured `AXON_AI_MOCK`. All five `ai_extract_*` builtins route through it. Reproduced: `AXON_AI_MOCK=1` with no key → interpreter prints `ok`, the runtime returns `ANTHROPIC_API_KEY … is not set`; with a key it would have made a real billed call. I-2 divergence (RT-01) |

The exfil capture, verbatim from the sink, before the fix:

```
EXFIL-PATH: /v1/messages
EXFIL-HEADER x-api-key: sk-ant-VICTIM-REAL-KEY-000
EXFIL-BODY: {"messages":[{"content":"summarize the confidential quarterly numbers",…
```

Fixed in two independent layers, because either alone leaves a hole. **Pin:**
the interpreter passes the program's declared net allowlist to the AI runtime,
which refuses any resolved endpoint host outside it — verified separately, with
the `.env` moved INTO the program's own directory so it still loaded, and the
call was refused by host. **Blast radius:** `load_dotenv_once` no longer walks
upward by default (invocation directory only; `AXON_DOTENV_WALK=1` restores it).
All five mock stubs now match the interpreter byte-for-byte (i64→1, f64→1.0,
bool→true, confidence 0.9), verified against every typed entry point.

**Limitation, stated rather than implied (O027):** the pin binds only programs
that declared a net grant. An unannotated program is still redirectable — I
confirmed it still reaches the sink after the fix. That is deliberate (it made
no claim, and pinning it would break self-hosted gateway workflows), but it
means the guarantee is "a program that says where it will connect cannot be
redirected", not "AI traffic cannot be redirected". Same shape as O026; the two
want deciding together.

### The parity gate — the I-2 soundness mechanism could pass while asserting nothing

| task | commit | what |
|---|---|---|
| **T36** | `—` | `parity_all.sh` had **no floor on `$pass` anywhere in the file** — the only decision was `if [ "$fail" -ne 0 ]`. An all-SKIP run printed `0 passed, 49 skipped, 0 failed` and then `PASS — no interp↔codegen / AOT-wasm divergence ✓`, exit 0. Proved with a before/after against an identical stub tree: the HEAD script says PASS, the fixed one says FAILED (GATE-01) |
| **T36** | `—` | `wasm_parity.sh` counted a row as OK when BOTH legs failed with identical empty output — zero parity evidence dressed as coverage. Verified by planting a program that fails on both: pre-fix logic gives `OK (exit 101)`, now `NO-COVERAGE … both legs failed silently`, exit 1 (GATE-02, remaining half) |

`EXPECT_MIN_PASS` defaults to 40, below the observed healthy count (**44 passed /
5 skipped of 49**, 2026-08-04; the 5 need an Android NDK or headless Chrome), so
a box missing a toolchain or two still passes while "nothing ran" and "half the
suite vanished" cannot. The no-coverage test is *empty stdout AND non-zero*, not
merely non-zero — `examples/sum_types.ax` legitimately exits 47 (its computed
total), and the finding's suggested "reject any non-zero exit" would have
silently deleted real rows.

**Two thirds of GATE-02 were already fixed and the finding was stale on them** —
`HOST_BUILTINS` now includes `http_*`/`env_var` (T14) and the suite is green with
`ANTHROPIC_API_KEY` set, where the finding recorded it as live-red. Only the
both-legs-failed half survived. Worth noting as evidence the audit's own findings
decay.

**GATE-03 is only half closed, deliberately.** The default gate's test stage is
`--no-default-features`, so every codegen parity wrapper reports `ok` while
asserting nothing, and `parity_all.sh` runs under `--strict` only — a non-strict
run proves nothing about I-2 and still prints `✅ gate PASSED`. The fix sketch
says to promote `parity_all.sh` into the default path; **measured, that costs
7m49s**, not the ~2 min the sketch assumed. Changing gate latency by 4× is a
decision, not a bug fix, so instead the non-strict run now REFUSES TO BE SILENT:
it lists every skipped harness from `target/harness-skips.log`, states plainly
that I-2 was not verified, and names the three commands that would verify it.
Making it fatal is the existing `AXON_HARNESS_STRICT=1` needs-human item.

### Closure ABI miscompilation — and the build cache that hid the fix

| task | commit | what |
|---|---|---|
| **T37** | `—` | Closures whose declared param/return type isn't plain i64 were **miscompiled silently**, exit 0, no diagnostic, from a compiler that documents "refuse, never miscompile" (I-2). `\|x: f64\| x * 2.0` applied to 3.0 → interp `6`, native `4618441417868443648` (the i64 reinterpretation of 6.0's bits). `\|x: i32\| min_i32(x, 0-5)` applied to -3 → interp `-5`, native `4294967291` (F061) |
| **T38** | `—` | `axon build`'s incremental cache served the PREVIOUS compiler's object for unchanged source, so a real codegen fix silently did not take effect. `axon --version` reported `0.1.0 (0718794)` with two modified files in the tree |

**The finding's diagnosis of the i32 half was wrong, and the truth is worse.** It
blamed a zero-extend where a sign-extend belonged. That was real and is fixed —
but the actual defect is that the direct-call site built the indirect-call
signature from the **argument's** LLVM type, emitting
`call i64 %cfp(ptr, i64 -3)` against a function declared `(ptr, i32)`. That is
UB, not a wrong extension, and it showed: the same lambda printed `-5` or
`4294967291` depending purely on whether an unrelated f64 lambda had been
emitted earlier in the same program. A closure value is a bare
`{fn_ptr, env_ptr}` pair with no type tag, so the fix records each
`let f = |…| …` binding's declared signature and uses it to coerce arguments and
convert the i64-ABI result back.

**The new harness found a third divergence the finding never mentioned:** a
bool-returning closure printed `1 0` natively where the interpreter prints
`true false`, because the i64-ABI result reached `to_str` as an integer. Fixed
by narrowing back to i1 at the call site. Writing the test found more than the
report did.

**T38 is the one that matters most.** I fixed T37, rebuilt, re-ran the repro, and
got the old wrong answer — twice. The fix was in the `--emit-llvm` output and
absent from the linked binary. `build.rs` watched only `.git/HEAD` and
`.git/index`, and editing a tracked file dirties the tree without touching the
index, so the embedded sha never refreshed and the cache key was identical
across a semantically different compiler. For the window this existed, **any
codegen work verified without committing first could have been checked against a
stale artifact — including the parity harnesses**, which build from the working
tree. Logged as O029 with three follow-ups: a gate check that `--version`
reports `-dirty` iff the tree is dirty, whether parity harnesses should force
`--no-cache`, and whether attestation/`.axmeta`/R34 trust `VERSION` the same way
(a stale identity in an attestation record is worse than a stale object).

Gated by the new `scripts/closure_ret_parity.sh` (9 cases, compares STDOUT not
just exit codes, and includes BOTH lambda emission orders since the original bug
depended on order). Parity suite: 45 passed / 5 skipped / 0 failed of 50.

### SQL escaping — the guarantee was broader in the docs than in the code

| task | commit | what |
|---|---|---|
| **T39** | `—` | `sql_query`'s escaping was `replace('\'', "''")` and nothing else, under a code comment reading "Data is never SQL structure" and a builtin doc claiming SQL injection was "**unrepresentable by construction**". On MySQL/MariaDB — `NO_BACKSLASH_ESCAPES` off by default, and the engine of this demo's own exemplar CVE-2024-5314 (Dolibarr) — a backslash escapes the following quote, so a param of `\` consumed its own closing quote and handed the tail of the query to the attacker (P5-25) |

Reproduced byte-for-byte before the fix:

```
sql_query("SELECT * FROM t WHERE a = ? AND b = ?", ["\\", " OR 1=1 -- "])
→ SELECT * FROM t WHERE a = '\' AND b = ' OR 1=1 -- '
                              ^^ quote escaped; the tail is now SQL structure
```

Backslash is doubled now, before quote-doubling (order matters — doubling quotes
first would then re-escape the backslashes that rule introduced). The regression
test uses the exact payload and was verified to FAIL against the old escaping.

**The doc claim was the bigger defect.** E1210 makes the query's *structure*
attacker-independent — a template built by concatenation doesn't compile — and
that is a real, strong property. But it says nothing about escaping the bound
*data*, and "unrepresentable by construction" was read as covering both.
`COVERAGE.md` rated the whole CWE-89 row **PREVENTED** on that basis. Both now
say which half is which.

**Not fixed, because it can't be (O030):** the new escaping is dialect-specific.
Doubling `\` is required on MySQL and CORRUPTS the value on PostgreSQL/SQLite,
where a backslash is an ordinary literal. So this trades a MySQL injection for a
Postgres data-corruption — the right trade, and now documented, but a trade.
Rendering values into query text is itself the unsafe pattern; the correct
destination is returning a placeholder query plus a params array and letting a
driver bind them. Nothing forces that choice until Axon has a database sink,
which is precisely why it should be decided deliberately.

### Mutable closure capture — the two engines gave different answers

| task | commit | what |
|---|---|---|
| **T40** | `—` | A write to a captured binding inside a lambda was **silently dropped by the interpreter** and persisted natively. Same source, two backends, two answers, no error or warning from either (F094 / P5-16 / DOC-02) |

```
let n = 0;  let bump = || { n = n + 1  n }
interp:  call1=1 call2=1 call3=1  outer n=0     ← writes dropped
native:  call1=1 call2=2 call3=3  outer n=0     ← heap capture
```

README.md:65 lists "**Closures** — first-class, heap-captured mutable closures"
as a shipped feature and demonstrates it with this exact code, and
`fixtures/closures.ax` calls the pattern "the spec §3 make_counter pattern". So
native was right and **the interpreter — the reference oracle for I-2 — was the
wrong side**. `Value::Closure.captured` is now an `Rc<RefCell<HashMap<..>>>`:
persistent across calls of that closure, still a by-value snapshot of the
defining scope (`outer n=0` on both engines — the outer binding is not aliased).
A closure crossing the R15 host boundary gets a fresh cell, since the SendValue
path is a deep clone by construction.

**Nothing anywhere executed this.** `closures_fixture_type_checks_cleanly`
asserted only that the fixture type-checks; the phase-15 higher-order test
asserted only that it parses; and `grep -rn closure scripts/*.sh` found no
parity harness at all. Running `axon run fixtures/closures.ax` panicked with
`assertion failed: 0 != 1` — the fixture had been self-asserting the right
answer the whole time, and no test ever called it.

Closed all three gaps: both fixture assertions now EXECUTE (the phase-15 one
asserts exit code **8**, its pass-count — a bare "didn't panic" check would have
accepted 7 with the counter test failing), plus three new executed contract
tests and a new `scripts/closure_capture_parity.sh` (6 cases, compares STDOUT,
covers independent cells, param shadowing, inner-`let` non-leak, and a counter
driven inside a fn). Parity suite: 46 passed / 5 skipped / 0 failed of 51.

### T41 — a declared AI call budget was enforced only under the interpreter

`@[ai(policy(budget: 1))]` on a fn making three `ai_complete` calls (F141 /
P6-EXIT-04, both marked REPRODUCED in triage — reproduced again here before
touching anything):

```
AXON_AI_MOCK=1 axon run   → [E1301] `ask` exceeded its AI budget of 1 call(s), exit 5
AXON_AI_MOCK=1 axon build → exit 0, and the binary ran ALL THREE calls, exit 0
```

`grep -rn 'E1301|ai_budget' crates/axon-core/src/codegen/ crates/axon-rt/src/`
returns nothing: the native runtime has no call meter at all, so exit 5 does not
exist natively. The AOT binary kept spending past a policy stop the interpreter
treats as fatal — I-2 in the unsafe direction, and the exact thing declaring a
budget is meant to prevent.

Native now **refuses** (E0910), the shape already used for the non-`balanced`
tier next to it. The refusal condition mirrors the interpreter's *enforcement*
condition exactly — a direct `ai_complete` in the fn's own body, since the meter
keys on the current fn (R3c §3) — so it neither under- nor over-refuses. A
malformed budget is W1311 + unmetered in the interpreter, so it still builds; so
does a budget on a fn that makes no AI call.

The budget is now parsed by a shared `ai_routing::budget_from_attrs`, matching
how `tier_from_attrs` already single-sources the tier. Codegen and the
interpreter cannot drift on "is this fn metered?", which is the drift that made
the hole possible.

Three findings about the *test surface* fell out, all the same shape as the
recurring class:

- `scripts/exit_code_parity.sh` closed with `"native==interp on all exit codes"`.
  Every case in it was 0, 101, 6, or a plain `main` return — codes 3, 4, 5, 7 and
  8 had **zero** coverage. Exit 5 was "covered" by a line of prose. The summary
  now enumerates what is and is not covered.
- Its `check` helper `exit 0`-ed the **entire harness** when one case's native
  build failed. One un-buildable case silently passed everything. Now a failure.
- The adjacent tier-refusal test's "must still build" case carried
  `budget: 2`, so after this change it would fail the build for an unrelated
  reason while its assertion (tier message absent) still passed. Budget removed
  and `status.success()` asserted, so the case now witnesses what it claims.

New `check_refused` rows cover the budget and tier refusals; new
`build_refuses_ai_call_budget_e0910_f141` covers refuse / no-call / malformed.

Two things this turned up that are **not** fixed, both recorded with executed
evidence: **O031** — native links `axon-ai` unconditionally, so a build without
`--features asi-runtime` (where `axon run` refuses AI calls with E1300) still
produces a binary that dials the live model; and **O032** — a budget is escaped
by moving the call into an un-budgeted helper, which is spec-blessed today but
contradicts the spec's own stated guarantee.

### T42 — a principal handle was its array index, so `child - 1` was root

P7-SEC-03 (critical, executed). `PrincipalRegistry` stored principals in a
`Vec` and handed out `len() - 1` as the handle; every `principal_*` builtin took
it as a bare `i64`. Reproduced against the pre-fix build — a child holding **no
capabilities** and a budget of **5**:

```
root handle = 0 / child handle = 1 / child budget = 5
forged handle = 0                      // child - 1
forged budget = 999995                 // root's
forged can exec? true
escalated can exec? true               // minted itself full capabilities
audit now attributes to: root
exit 0, no diagnostic
```

`kernel.rs`'s own doc comment claimed *"escalation unrepresentable"*. It was one
subtraction away, and `@[contained]` has no runtime backstop behind a miss here.

A handle is now an unguessable token resolved through a `by_token` map;
arithmetic on a held handle lands on no principal at all. Lineage links stay
internal indices, which never move, so the registry is still append-only. Token
generation uses a **private** xorshift state — deliberately not the interpreter's
`RNG_STATE`, which `srand(n)` re-seeds from Axon source, so drawing from it would
have let a program set the seed and enumerate every handle the kernel was about
to issue.

Reproducibility vs. unguessability is a genuine tension and the code says so
rather than papering over it: with `AXON_SEED` set (a deliberate deterministic
run, which `axon trace --replay` depends on) the stream derives from that seed,
domain-separated from `random_*`; without it, time-seeded and unguessable
outright. A program that can *read* `AXON_SEED` could recompute tokens — inside
`@[contained]`, `env_var` is denied (E1001), and outside a sandbox the program
already holds ambient authority.

Two things fell out while wiring it:

- The `h >= 0` guards on every principal builtin had to go: a token is drawn
  from the full `i64` range, so a **negative handle is now perfectly valid**.
  Validity is decided by one thing only — did the registry issue this exact
  token — which is also what makes a forged handle inert.
- `principal_activate` on an **unknown** handle fell back to the name `"root"`.
  So a bogus handle did not fail; it produced a believable audit record naming
  the most privileged principal in the registry. That is the same fail-open
  direction already refused one builtin over in `kernel_goal_create` (T20 /
  F006). Now E1601.

Regression tests: a kernel unit test that walks every arithmetic neighbour of a
held handle and asserts none of `get`/`budget_remaining`/`authorize`/`can_mint`/
`mint`/`spend` yields anything (**verified to FAIL** when `issue()` is reverted
to returning the index), plus a four-case end-to-end test covering the read, the
mint, the audit re-attribution, and a **negative control** that legitimate
handles — including negative-valued ones — still attenuate and carve exactly as
before.

### R16 Axon UI spec (design work, no code)

| section | what |
|---|---|
| §1b | prior-art survey; **decision: we do not build the renderer** |
| §1c | **the whole chosen stack is pre-1.0** — Vello alpha, Masonry/Xilem experimental, Blitz pre-alpha; only Taffy mature, only Slint stable (and not permissively licensed) |
| §1d | **decision: the renderer seam MUST admit a CPU path** — decisive reason is that a GPU-only renderer cannot be gate-tested |
| §3a | rich text marked **a11y-BLOCKED** (externally gated on AccessKit, not on our slice order) |
| §3b | Parley + AccessKit as **type obligations**, not backend details |
| §3c | read Blitz's wiring — yielded the stable-identity requirement |
| §3d | **ASI view semantics** — uncertainty / pending / streaming / agency as one problem |
| §3d(a) | first ASI slice, to implementation depth, **with a stated falsifier** |
| §5 | stable `View` node identity + `.key()` |
| §6a | E2109/E2113–E2116 allocated |
| §7.2 | **empirical status of the controls §7.1 depends on** — the audit findings above, in the spec |
| §9.0 | cross-slice acceptance obligations; every criterion must execute and assert an artifact |
| §11a | §3d(b)(c)(d) need three types the language **does not have** |

**Thirty-two fixes landed; all four confirmed sandbox-escape CRITICALs are closed** (F013, F041, F153,
plus OSK-P4-C2 which triage rated critical). Each has a regression test verified
to FAIL before the fix — no fix landed against a test that would have passed
anyway, which is the defect class this audit exists to document.

## Not done, deliberately

- **O012 / F062** — verified real, fix reverted rather than rushed: it needs a
  generic `walk_expr` extracted first, because copy-pasting `expr_calls`'
  70-line recursion is how the "walker missed an arm" class recurs (already
  fixed 3x here: R6 taint, `@[contained]` helpers, T2 string dispatch).
- **The execution re-triage result (the most useful thing to carry forward).** 8 code-read-only
  findings were re-checked by RUNNING them: 8 answers changed — 2 false (OSK-L03 `--latest`,
  INTERP-H04 budget bypass), 2 true-but-understated, 1 true-and-mis-attributed, 3 true as written.
  Reading never once predicted the outcome. Of the findings worked all session, every code-read
  one cost a revert and every executed-repro one landed cleanly. **Work the remaining findings in
  that order** — the triage JSON records which is which ("REPRODUCED" vs "verified by direct read").
- **~150 findings remain untriaged-for-implementation.** They are triaged for
  *severity* (`governance/reviews/2026-08-01-triage/full-185.json`) but no code
  was written for them. Effort profile: 59 trivial / 125 small / 112 medium /
  42 large.
- **8 needs-human decision groups** — see `build-loop.md`. Group A
  (exit-code semantics) blocks several confirmed criticals: `main()` returning
  3/6/8 collides with reserved exit codes and 256 reports success. Nothing
  downstream can be closed properly until that is decided.
- **O005 — CI's `cargo fmt` job is red on `main`** (41 diffs, from the parallel
  R21/R23 track). Not fixed: a unilateral reformat of another track's live files
  would collide with concurrent work.

## Verification standard used

Through T13: green = **no failing test other than
`wasm_interp_matches_native_on_pure_compute`**, the single clean-baseline
failure. From **T14 onward: zero failures** — that test was fixed rather than
tolerated, so the bar tightened mid-run. Every task was checked against the
standard in force when it landed, never against an aspirational "100% green".
