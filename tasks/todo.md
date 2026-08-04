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

### T43 — the `@[contained]` string-dispatch guard had two holes, and one was in the guard itself

P7-SEC-02 (critical, executed). T2 (`870469a`) closed the class where a builtin
dispatches to a user fn **named by a string** — the walker built follow-edges
only from `Call` callee names, so `scheduler_spawn("evil", 0)` walked the target
as an inert string argument. It built an explicit allowlist,
`indirect_dispatch_args`. Re-probing every entry found the allowlist was both
**incomplete** and **bypassable**.

**(a) `kernel_goal_create` was missing.** Its dispatch is one builtin removed
from the name — `kernel_goal_create(p, "evil", …)` stores it, `kernel_goal_run(g,
n)` invokes it later — which is exactly why reading call sites did not surface
it. Executed:

```
@[contained(fs: [], net: [], exec: none)]
fn sandboxed() -> f64 {
    let p = principal_root("p", true, true, true, 100)
    let g = kernel_goal_create(p, "leak", 100.0)   // leak() reads /etc/passwd
    kernel_goal_run(g, 3)
}
```
`axon check` exit 0. `axon run` exit 0, file read.

**(b) A non-literal callee name silently skipped the whole check.** The follow
loop matched `Expr::Literal(Str)` and `continue`d on anything else, so *every*
builtin the allowlist covered — including the three T2 fixed — was bypassed by
building the name at runtime:

```
let n = "le{str_trim(\"ak \")}"
scheduler_spawn(n, 0)
```
`axon check` exit 0. This is the worse of the two: it defeats the fix rather than
missing a case. A dynamic callee is exactly as unverifiable as a dynamic *path*,
which already fails closed with a clear E1001 — it now fails closed the same way,
with a message in the same shape.

**The allowlist is now drift-guarded.** A unit test walks `BUILTINS` and requires
every builtin with a fn-name-shaped `str` parameter to appear in
`indirect_dispatch_args` **or** in a new `NON_DISPATCHING_NAME_PARAMS` list that
records *why* it does not dispatch (`env_var` reads an env var; the
`goal_best_*`/`agent_*` family reads recorded provenance; `fn_addr` takes an
address and never invokes). Landing in neither is a build failure, because
landing in neither is what silence looks like — and here silence fails open.
**Verified: deleting the `kernel_goal_create` row makes the test name that exact
builtin.** The list-rot direction is checked too: an entry naming a builtin that
no longer exists, or appearing in both tables, fails.

Two regression cases added to T2's own test plus a **negative control** — a
literal callee whose body stays inside the sandbox must still check clean,
otherwise the fix would merely have banned the feature and every case above would
pass for the wrong reason.

### T44 — axon-os inferred the verdict from stderr prose, and the prose had drifted

OSK-P4-H2 / P4-OS-21 (high). The sealing verdict was a chain of
`err.contains(...)` substring tests over the child's stderr with a terminal
`else => Completed { value: exit_code }` — so any fault whose wording the chain
did not match sealed as a **success**. The finding called that a future risk. It
was already happening.

T24 added a `parse error`/`type error` arm for exactly this. But `axon run`
reports **type** errors as JSON diagnostics (`{"schema":"axon-diag/1",…}`) and
only **syntax** errors as prose, so the arm caught one and missed its sibling:

```
syntax error in a job -> "⚠ DENIED: program failed to compile"   axon-os exit 8
type   error in a job -> "✓ completed (value=2)"                 axon-os exit 0
```

Same class of job, opposite records, and the wrong one is the silent one. The
record is hash-chained, so it attests success for a job that executed zero
statements — durably. That is an attestation-integrity failure, not a cosmetic
one: the record lies and the chain makes the lie tamper-evident.

**Why it was written that way, and how that is now fixed.** The comment above
the chain is honest about the constraint: `axon run` propagates `main`'s return
as the exit code, so a job returning 7 is not budget exhaustion, and the exit
code alone could not be trusted. That collision is real — and it is the same
**exit-code semantics** question sitting at the top of the needs-human list. It
did not have to be answered globally to fix this, because axon-os *generates its
own wrapper*: the wrapper now prints a nonce-bearing completion marker after the
job returns. Its presence answers "did the job finish?" separately, so the exit
code can then be read as the return value. Same shape as the T33 guest-kernel
`-EXIT0` sentinel.

The classifier now branches on the exit code — 2 malformed, 3 verify, 4 halted,
5 ai-policy, 6 refine, 7 budget, 8 sandbox, **anything else non-zero → Denied,
never Completed**. stderr is still read, but only to phrase the human-readable
`reason`. Exit 0 with no marker is also a denial: it means the wrapper's tail
never ran, so the outcome is unknown, and "unknown" must not seal as success.

Two details worth recording:

- The first attempt had the marker carry `to_str(__v)`. That panics for the
  common `fn main()` job, whose renamed entry returns unit — caught by the
  existing R27 acceptance tests, which is what they are for. The marker now
  carries only the nonce.
- The nonce is **not** derived from the job's seed. A job can read `AXON_SEED`
  from its own environment, so a seed-derived marker would be forgeable by the
  very code whose completion it attests. Tested: a job that prints two forged
  markers and then panics is still `Denied`.

Regression test drives the real CLI over four cases — type error, syntax error,
**a clean job returning 8** (the collision itself: it must record
`Completed{value: 8}`, not a sandbox denial), and the forgery attempt. **Verified
to FAIL against the old terminal-else**, reproducing `✓ completed (value=2)`
exactly.

### T45 — every `native::M::*` call bypassed the runtime sandbox, the agent log and the audit ledger

INTERP-H02 (high, executed). Three controls sat inline at the head of
`call_builtin`: the R4 `@[agent]` action log, the F5 runtime sandbox ceiling,
and the R28 audit ledger. That is sound only if `call_builtin` is the sole way
to reach an effect. It was not — `eval_native_call` handles `native::M::*`
directly off `Expr::Call` and returns before `eval_call`, so native modules
never touched any of the three.

Reproduced with `gfx`, which declares `effects: &["IO"]`, under
`sandbox_create(p, "")` — an **empty** ceiling:

```
window_open / surface / clear / present / present / frame_count
→ process exit 2
```

Exit 2 *is* `frame_count`, so both `present()` calls executed inside a sandbox
that permitted nothing. No violation raised. With `AXON_AUDIT_LEDGER` set, the
hash-chained ledger recorded **zero** rows for the run; it now records four.

Worth being precise about which layer failed: the static
`@[contained(gfx: any)]` grant was satisfied, and E1004 correctly refuses the
program without it. The **runtime** sandbox is a separate and stricter layer —
`sandbox_run` under an explicit ceiling — and that is the one that saw nothing.

Fixed by extracting the three blocks into one `pre_effect_gate(op_name,
effects, cap, ledger_kind, scope_args)` and calling it from both entry points.
Its parameters are supplied by the caller rather than derived from the op name,
because a native module's effects come from `native::Module`, not from the
builtin tables — deriving them would have forced a second, divergent
classification path, which is how this class recurs in the first place. The call
sits above the `modbus`/`fhir`/`fix` branch, so the domain modules
(`effects: &["Net"]`) are covered by the same gate rather than a second one.
`scope_args` is `None` for native calls: their arguments are handles and
scalars, with no path or host for the T3 scope check to constrain.

Regression test has the deny case plus **two** negative controls — an
IO-granting ceiling must still run the module to completion, and no sandbox at
all must be unaffected. Without them "native calls always fail" would satisfy
the deny case. **Verified to FAIL with the gate removed**, reproducing exit 2.

Also confirmed already-fixed and needing no work: **INTERP-H01** (the four
`host_await*` builtins absent from `is_impure_builtin` / `builtin_effect_row`)
is the same finding as P4-INT-02, closed by T29 — both tables now list them.

### T46 — the per-call AI tier was dropped natively, and the walker beneath the check had a hole

F062 / O012 (high, executed) — the item this run explicitly **deferred** rather
than rush, because the fix needed a real expression walker first. That judgement
held, and the walker turned out to be hiding a second defect.

**The finding.** `codegen/mod.rs` refuses (E0910) when a fn's ATTRIBUTES request
a non-`balanced` AI tier, because the native runtime routes every call to the
default model. R3b also allows the per-call form, and the interpreter gives it
**top** priority. Codegen dropped it under a comment asserting *"native AI calls
aren't in the codegen path"* — false; `ai_complete` is fully lowered in
`codegen/builtins.rs`. Reproduced:

```
ai_complete("say hi", tier: "cheap")
  axon build  → exit 0, no diagnostic
  axon trace --ai → [cheap anthropic:claude-haiku]  $0.000000
```

The attribute path was closed and the per-call path, with the identical hazard,
was wide open. The refusal now fires for **any** `Some(tier)`, not just
non-`balanced`: an unknown tier name is E1302/exit 5 in the interpreter, which
native cannot replicate either, so `"balanced"` is the only value that would
need a carve-out and it buys nothing.

**The walker.** `expr_calls` was a bespoke ~70-line recursion ending in
`_ => false`. The deferral note predicted that copy-pasting it would produce
"the next walker-missed-an-arm bug". It had already produced one: that
catch-all silently dropped `Select` and `WithHandler`, so **no** scan in
`codegen/mod.rs` — every one of them a sound-by-refusal E0910 gate — ever looked
inside an effect handler or a select arm. A per-call tier inside a `with
handler` body built clean.

`walk_expr` now visits every sub-expression, and its match is **exhaustive on
purpose — there is no `_` arm**. Adding a variant to `ast::Expr` is a compile
error here, which is the only mechanism that reliably keeps a walker complete;
leaf variants are listed explicitly so "has no children" is a decision on the
record rather than an omission. `expr_calls` is now four lines over it.

One implementation note worth keeping: the callback must be `&mut dyn
FnMut(&ast::Expr)`, not a generic. A generic re-wraps its own closure type at
every recursion level, and rustc does not diagnose that — it **segfaults**
(`SIGSEGV`, "cycle encountered after 3 frames with period 4").

Tests: unit tests that the walk reaches a handler body, a handler *arm*, and the
root expression (**verified to FAIL** when the two arms are stubbed back to
`{}`); and an end-to-end refusal test whose second case is the `with handler`
body — the case that proves the walk rather than just the check — plus a
negative control that an `ai_complete` with no `tier:` still builds.

Also confirmed already-fixed, no work needed: **P4-INT-04** (`run_suspendable_values`
using a default-stack `scope.spawn`) is the same code as INTERP-H03, closed by
T16 — it now uses `stack_size_for_depth(resolve_max_depth())`.

### T47 — the tamper-evident record did not cover its own verdict

P6-EXIT-03 (critical). `record::build`/`verify` hash-chain
`manifest_digest → events → record_digest` and stop. `run_id`, `seed` and —
most importantly — `verdict` sit **outside** the chain. Executed against a real
sealed record:

```
forge verdict Completed{value:3} → Denied{axis:"sandbox"}   ✓ intact, exit 0
forge seed    42 → 999                                       ✓ intact, exit 0
forge run_id  "run" → "someone-elses-run"                    ✓ intact, exit 0
   ...all three with a BYTE-IDENTICAL digest
tamper any chained event field                               ✗ TAMPERED, exit 11
```

So the tamper-evident record was tamper-evident for the fields nobody needs to
forge, and silent on the one it exists to attest. This lands directly on top of
T44: the verdict that fix just made *correct* was not covered by the hash that
is supposed to make it *durable*.

A terminal seal now folds `(run_id, seed, canonical_verdict)` into the chain
before the head, keeping the chain shape (each link still hashes its
predecessor) and needing no new schema field. The verdict's **payloads** are
sealed too, not just the discriminant — a denial whose `reason`/`axis` can be
rewritten is only half-attested.

`verify` **refuses** a pre-seal `axrec1:` record rather than accepting it. An
attacker chooses which format to present, so accepting the old one is a free
downgrade back to an unauthenticated verdict; the refusal says exactly that.

Note on my own probing: my first "control" mutated a field name that does not
exist in the schema, so it changed nothing and appeared to show that event
tampering *also* went undetected. Re-run against the real field names
(`action`/`target`/`label`/`caps_used`), every one correctly gave exit 11. The
finding was right as written and my first reading of it was wrong.

Why the existing `acc_a6_record_tamper_detected` stayed green throughout: it
only ever mutated an **event** field. Same shape as the recurring class — the
test next to the hole checks the neighbouring property. The new
`acc_a6b_…_p6_exit_03` drives the real CLI over the verdict, the verdict's
value, the seed, the run_id, the `axrec1` downgrade, and — the case that
matters — a **real denial laundered into a completion** (`overreach.axjob`,
which the static gate refuses). **Verified to FAIL against the unsealed chain**,
reproducing `✓ intact` for a forged verdict.

`governance/specs/R21-axon-os-supervisor.md` §4.3 updated: it documented the
`axrec1` construction, so leaving it would have made the spec the authority for
the defect.

### T48 — the guest kernel granted every effect by default, and the policy path had never connected

OSK-P7-C3 (critical), plus three defects found by fixing it. `R36 §2` already
documents this as four fail-open policy-provenance sites and calls closing them
S0 work; §9 clause (f) gates each negatively. It had not been done.

**(1) The guest kernel failed open.** Six paths returned `EffectSet(0xFF)` —
IO+FS+Net+AI+Exec+Random: absent key, non-array value, no `boot_params`, no
cmdline pointer, absent `axon.policy=`, and `POLICY_READY == false`. The static
default was `0xFF` too. Every one now denies, and a base64 value that decodes to
nothing is refused explicitly rather than relying on the static happening to be
right.

**(2) It had never been tested — it could not be.** `axon-guest-kernel` is
`test = false` (bare-metal `no_std`, cannot link the std harness), so the parser
deciding what a confined guest may do had **zero** tests. The pure helpers are
now in `mmds_parse.rs`, `include!`d by both the kernel and a host-side test in
`axon-vm` — the same text, not a copy that can drift. Four tests, **verified to
FAIL against the original `0xFF` returns**.

**(3) The `.axmeta` policy path had never connected.** `axon build
--emit-manifest` wrote the sidecar beside the **binary** (default: the current
directory); `axon-vm` reads it beside the **source**. They never met. Invisible
because a missing manifest meant `allowed_effects: null` meant `0xFF` — the
disconnected path failed *open*, so nothing complained. `axon_kernel_gate.sh`
even has a `[[ -f "$META" ]]` check against the source-adjacent path that has
been silently false — printing neither ok nor fail — the whole time.

**(4) And when it did connect, it did not parse.** `AxonManifest.risk` was
declared `Option<String>`; the producer emits `"risk": 0`, a number. So *every*
manifest failed to deserialise and `unwrap_or_default()` silently substituted an
empty one. A sidecar that exists but cannot be parsed is now an error — a corrupt
policy file must not read as "no restrictions".

**(5) `effect_union` was derived from declarations.** Only from declared
`| {IO, Net}` rows, so `agent_task.ax` — whose `main` prints twice — emitted
`"effect_union": []`. Same shape as the T34 risk derivation. It is now seeded
from `capabilities::program_effect_rows` (what the program actually calls, via
`builtin_effect_row` — the same table the runtime gate enforces), with
declarations able to widen but never shrink it.

Producer side: `axon-vm` now **refuses to launch** with no grant at all rather
than sending `null`, naming the three ways to supply one.

The T46 walker moved from `codegen/mod.rs` to `ast.rs` so `capabilities` could
reuse it. I had started writing a third private traversal for
`program_effect_rows` before catching myself — codegen is behind a feature flag,
and a walker every safety check depends on must not be.

**What closing the default surfaced.** With `0xFF` gone, `good_agent.ax` died in
the microVM: `VIOLATION: syscall 257 blocked (FS not in policy)`. The guest runs
the *interpreter*, which must `openat` the program before any Axon executes — so
the grant a guest needs is the program's effects ∪ what the substrate requires,
and those have different owners. That is a design question (**O034**), not
something to answer silently, so the gate now states `IO,FS` explicitly with the
reason. Worth being blunt: Layer 3 of that gate was passing by granting full
authority, which is exactly what it claims to test.

### T49 — the cross-VM quorum counted votes cast for other proposals

P4-OS-12 / P7-SEC-04 / P7-KRN-07 (high; canonical entry for the
unauthenticated-quorum class). Two distinct holes, executed:

**(a) Votes were not bound to the proposal.** `check_quorum(votes, required_n)`
did not take the request at all, so `.vote` files naming *different* runs
aggregated into one decision:

```
three .vote files naming "benign-dry-run", "some-other-job", "deploy-prod"
  → QUORUM MET: 3/3 approvals      exit 0
```

This needs no forgery and no key material to exploit — only a fleet that votes
on more than one thing. Honest approvals gathered for a dry run authorize a
production deploy. `check_quorum` now takes the run id and counts only votes
naming it; "no votes for this run" is reported distinctly from "voted against",
because those mean different things to an operator. `axon deploy --quorum-dir`
gained a required `--quorum-run-id` for the same reason — it was counting every
`.vote` in the directory.

**(b) Identity was self-declared.** The consistency check only required votes to
*agree* on `voter_tcb`, so three forged votes agreeing on a made-up digest passed
it. `--expect-tcb` lets the operator pin the expected identity — the same shape
as the T31/T32 `--expect-digest` / `--pin-baseline` gates.

**What is NOT fixed, and is now stated instead of implied.** A `.vote` file
carries no signature; three hand-written JSON files still produce
`QUORUM MET: 3/3`, exit 0. The module header called these "attested
VoteRequest/VoteResponse" — nothing verified either. That claim is corrected in
both the header and the CLI help, which now say plainly that votes are
unauthenticated and the responses directory is part of the TCB. Signing needs a
key-distribution decision (**O035**, three options sketched) that this module
cannot make for the operator; asserting a property the code does not have is
worse than the gap itself.

One regression test deliberately asserts the *residual* hole — mutually-agreeing
forged votes still pass without a pin — so the limit is pinned in the suite
rather than described in prose that can drift from the code.

### T50 — every approval clicked in the web UI was silently discarded

P4-PROD-10 + P4-PROD-09 (both high), same subsystem and the same class: the
approval flow's gating was untethered from the artifact it claimed to gate.

**(a) Approve and deploy staged to different files.** `write_temp` named the
staged program from `subsec_nanos()`, so every request minted a NEW path.
Reproduced against the running server:

```
POST /api/ast/approve → "approved_path": "/tmp/axon_web_400484501.ax.approved"
POST /api/deploy      → "path": "/tmp/axon_web_416046457.ax", "approved": false
```

`ast approve` wrote `<tmp-A>.approved`; deploy ran `<tmp-B>` and looked for
`<tmp-B>.approved`, which never existed. Acid Test 2's sign-off step was
decorative — and the deploy still reported `"status":"deployed"`.

Staging is now **content-addressed** (`sha256(content)[..32]`), which fixes it
with no session plumbing and gives exactly the right semantics: the same program
text resolves to the same path so its approval is found, while text edited after
approval resolves elsewhere so it is **not** approved. Verified both directions.
That is the same property `axon ast approve` enforces internally (T10 made the
approval bind the program text, not the filename), so the two layers now agree
rather than merely coexisting.

**(b) Three UI gates keyed on a field no schema emits.** All three checked
`j.error`:

| pane | schema actually emits | old behaviour |
|---|---|---|
| review | `errors` (an **array**) | a program that fails to type-check set `done.reviewed` and unlocked Approve |
| redteam | `caught` | `done.redteamed = true` sat **outside** the branches, so a redteam that CAUGHT still unlocked Deploy |
| deploy | `status` | any response without `error` rendered as "deployed" |

The redteam one directly falsified the documented Acid-Test-4 gating claim.

**The fix sketch was wrong about the deploy pane, and testing caught it.** Gating
on `j.status` would have *regressed* it: `/api/deploy` used `run_json`, which
parses stdout as one JSON document and falls back to an `{ok, stdout, …}`
wrapper otherwise — and a deployed program prints its own output first, so the
fallback was the normal case and `status` never appeared at the top level. The
real fix is server-side: `/api/deploy` now uses `run_json_merged` (already used
by `/api/redteam`), which lifts the report object and keeps prose as
`run_output`. Confirmed against a gate-blocked deploy: `"status":"blocked_gate"`,
`"gate":"assert_deployable"` — both now at the top level, and the pane names the
gate that refused.

Two regression tests, **verified to FAIL against the pre-fix code**. One of them
needed a second pass: my own explanatory comment quotes
`done.redteamed = true`, and a naive `find` read the *comment* rather than the
code — so the test now strips `//` lines first. A test that can pass or fail on
prose is not testing anything.

### T51 — a failing harness could report itself as skipped, and 44 tests believed it

P6-GATE-04 / GATE-04 (high). Every one of the 44 harness wrappers in
`cli_run.rs` inlined the identical check

```rust
if stdout.contains("skipping") || stderr.contains("skipping") { … return; }
assert!(out.status.success(), …);      // ← never reached
```

so a harness that **exited non-zero** while the word "skipping" appeared
anywhere in its output was reported GREEN.

That was not theoretical. `scripts/random_i64_parity.sh`'s `build_run` called
`exit 0` from inside a command substitution (`code="$(build_run …)"`), so the
exit terminated only the subshell, its message became the *value* of `$code`,
and the script emitted a FAIL line and a skip line together — and the test
passed. Both halves are fixed: the harness now returns a sentinel the caller
tests at top level (a subshell cannot end a script, so the decision has to come
back as data), and the wrappers go through one `harness_skipped` helper with two
rules in order — **a non-zero exit is a failure, never a skip**, and a skip is
recognised from the harness's **final** line rather than by scanning everything,
mirroring `parity_all.sh` which already got this right.

**The last-line rule turned out to matter on its own.** Re-running the suite
afterwards, `codegen_fuzz_parity_finds_no_divergence` went from returning in
milliseconds to running **75 seconds** and asserting `fuzz_parity: PASS`.

That led somewhere more interesting than the finding: the parity harnesses
resolve `target/debug/axon` and skip if it cannot emit native code. Whether they
run therefore depends on **what built that path last** — and I had observed both
states in this session without noticing, because a `--no-default-features`
rebuild during T45/T46 left a codegen-less binary there. A green suite carries no
information about whether the primary I-2 differential guard executed. Recorded
as **O036** with two options; I corrected my own first draft of it, which claimed
the harnesses "never run" — they run whenever that artifact happens to be
codegen-capable, which is worse.

A direct rule test covers both cases, including the mid-run
`"skipping 2 of 40 cases"` note that must *not* read as a skipped harness. Suite
now 443 passing with only the two Android harnesses legitimately skipped, down
from four.

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

**Forty-one fixes landed; all four confirmed sandbox-escape CRITICALs are closed** (F013, F041, F153,
plus OSK-P4-C2 which triage rated critical). Each has a regression test verified
to FAIL before the fix — no fix landed against a test that would have passed
anyway, which is the defect class this audit exists to document.

## Not done, deliberately

- ~~**O012 / F062**~~ — **now done (T46)**. It was deferred for needing a generic
  `walk_expr` first, on the grounds that copy-pasting `expr_calls`' 70-line
  recursion is how the "walker missed an arm" class recurs. Extracting it
  revealed that recursion's `_ => false` had already dropped `Select` and
  `WithHandler` from every refusal scan in `codegen/mod.rs`.
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
