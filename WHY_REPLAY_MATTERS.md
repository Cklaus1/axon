# Why Axon optimizes for auditability, not stdlib parity

Status: position paper. Argues what Axon should be world-best at, and why the
obvious goal — "get the stdlib to 90% of Python's" — is the wrong one.

---

## 1. The question this answers

Axon's stdlib is roughly 70% of the surface everyday task-solving code touches and
roughly 55% of a full mainstream stdlib. **Both figures are my estimates, not
measurements** — they come from counting the 336 builtins and 36 userland modules by
area against Python/Go/Rust and judging each area's coverage. They are stated to
frame the question, and §5 argues they should not be the target precisely because
nobody can gate on a number arrived at that way. The natural next goal is "push
everything to 90%". This document argues against that, and names what to do instead.

The argument is not that parity is worthless. It is that parity is **not
differentiating**, and Axon has four properties available to it where the entire
competition sits at approximately zero.

## 2. Why uniform 90% is the wrong target

**Nobody chooses a language for its date library.** Moving Time from 30% to 90% is
weeks of work that changes no adoption decision. The gap is real; it is just not
decisive.

**Some areas must be capped below 100% on purpose.** Axon's regex engine refuses
backreferences and lookaround (E2203). That is not an unfinished feature — it is
what makes matching linear-time, which is a *containment* property: it removes a CPU
AMPLIFICATION primitive, where a short pattern triggers super-linear work on
attacker-shaped input. It does not make Axon CPU-safe — `while true {}` burns
unbounded CPU with no capability at all, and real containment needs fuel/step
metering, which does not exist yet. The narrow claim is still enough to matter:
"get regex to 90% feature parity" would delete a guarantee. The same logic will
apply to networking and process control.

**And the percentage misleads.** `tasks_hard` scores moved on *language card*
changes alone, with no compiler change whatsoever — and moved in BOTH directions:
some card edits cut the score (9 → 6 → 4 of 16 while adding builtin names to the
card), while a signature-listing arm reached 11/16. Those runs live in the atlas
repo (`spikes/rlm-engine/measurements/`, referenced by R42 §1.1), not here, and the
samples are small — 16 binary outcomes per arm. Two conclusions, and the second
matters more: a stdlib percentage cannot see any of this, and neither can a
3-run sample support a per-commit gate. See §5.

## 3. The six properties where the competition is at zero

### 3.1 Total replayability

No mainstream language can replay a program that called an LLM, read a clock, and
used randomness. Axon can nearly do it today:

| Source of nondeterminism | Mechanism | Status |
|---|---|---|
| `random_*`, distribution samplers | `AXON_SEED` (seeded xorshift) | covered |
| dict iteration | `BTreeMap`, so ordered by construction | covered |
| scheduler order | function of spawn order + `AXON_SEED` | covered |
| `ai_complete` | `AXON_AI_REPLAY` (memoized by prompt+model) | covered |
| `ai_extract_uncertain_i64` / `_f64` | consulted the cache — **fixed 2026-08-08** | covered |
| `now_ms` / `sleep_ms` | `AXON_CLOCK` + `trace --replay` anchoring | **covered 2026-08-08** |
| `temporal_now` / `temporal_new` / `temporal_is_valid` | routed to the same clock — **fixed 2026-08-08** | covered |
| `dir_list` **ordering** | results sorted (`read_dir` order is filesystem-dependent) | covered |
| `dir_list` **contents**, `read_file`, `file_size`, `file_exists` | `AXON_RECORD`/`AXON_REPLAY` host journal | **covered 2026-08-08** |
| `read_line`, `env_var`, `http_*`, `exec` | same host journal (one seam, not per-builtin) | **covered 2026-08-08** |
| `host_await*` (host replies) | — | **open** |

The last two rows closed together, and the reason is the point of §3.1: they were
never separate problems. Every environmental effect passes through one trait, so
one wrapper covers the column and a future builtin cannot forget to opt in —
there is nowhere else for it to reach the world. A recorded run now reproduces
byte-for-byte with its files deleted and stdin closed, while the same program
without the journal produces different output in that same stripped environment.

Two findings from building it are worth recording, because both are the shape this
paper keeps running into — a claim that was *nearly* true:

* **The seam had never worked for its actual purpose.** The host was stored in a
  `thread_local!`, and the interpreter runs every program on a freshly-spawned
  thread, so a host installed *for* a program was invisible *to* it. Every test
  passed because they all called into the seam on the installing thread; "install
  a host, then run a program" had no test at all.
* **`read_line` was not behind the trait**, calling `std::io::stdin()` straight
  from the builtin — so "every effect funnels through one seam" was false by one
  member, and that member is what an interactive agent run depends on.

Both were found by running a fifteen-line probe, not by reading the trait's method
list, which looked complete. That is now gated two ways: a test that runs real
`.ax` source and fails if the seam stops reaching the program, and a coverage test
asserting every `AxonHost` method is present in both the recording and replaying
hosts — the mechanical version of the question nobody remembered to ask about
`ai_extract_uncertain_*`.

The clock was the sharpest hole, because `axon trace --replay <run-id>` advertised
a deterministic `(Trace, Seed)` pair "for every run" and that claim was simply
false for any program calling `now_ms()`. It is now anchored to the `ts_ms` the
original run already recorded — no log-format change was needed, the anchor was
already being written.

Two of those rows were found by review of this document's own first draft, which is
worth recording because both were cases of a claim being *nearly* true:

* `temporal_now`, `temporal_new` and `temporal_is_valid` read the real clock
  directly, so `Temporal<T>` was unreplayable — and worse, a program mixing
  `now_ms()` with `temporal_*` saw **two disagreeing timelines**, one virtual and
  one real, making any `created_ms` vs `now_ms()` comparison arbitrary.
* `ai_extract_uncertain_i64`/`_f64` never consulted `AXON_AI_REPLAY` at all. The
  headline claim is "replay a program that called an LLM", and a typed extract made
  a live, unrecorded call straight through the cache.

Both are fixed, and both are now guarded — the first by a `clock_parity.sh` check
that two different clock readers agree, which is the property a future third reader
would break.

**Why this is the top priority:** an agent you cannot replay is an agent you cannot
review. Every safety story Axon tells — the audit ledger, the capability sandbox,
the provenance trace, `@[corrigible]` — assumes you can re-run the thing and see
the same behaviour. Replay is not a debugging convenience here; it is the
foundation the rest of the guarantees stand on.

### 3.1.1 Partial replay is worse than none, unless divergence is loud

This is the part to get right before advertising anything. Replay now covers
entropy, the clock, and model calls — but not the filesystem, network, stdin, or
`exec`. A replayed run that touches any of those **silently diverges** from the
original, and an auditor who trusts a "bit-for-bit" claim would then be reviewing a
run that never happened. That is a worse failure than having no replay at all,
because it manufactures false confidence.

So the claim must be scoped honestly: today, *any run whose only nondeterminism is
entropy, time, or model calls reproduces exactly.* Not "any agent run".

The fix is architectural and cheaper than it looks: **every environmental effect
already funnels through one choke point, the `Host` trait.** A `RecordingHost` /
`ReplayHost` pair — memoizing every host call the way `AXON_AI_REPLAY` memoizes
`ai_complete`, and **refusing loudly on a cache miss** rather than falling through
to the live environment — closes the entire open column of the table above in one
design, `read_line` and `http_*` and `exec` and file contents together. That is the
single highest-value build on this axis, and it is what turns replay from a
debugging aid into an audit instrument: once every effect is recorded, **replay-diff
— "the first event at which run B departed from run A" — becomes possible**, which
is the feature an auditor actually wants.

A smaller caveat, recorded rather than hidden: replay is currently *deterministic*
(two replays agree byte-for-byte) but not byte-*faithful* to the original run,
because `ts_ms` is stamped a few milliseconds before the program's first clock read.
Full fidelity needs a recorded clock trace, in the same shape as the above.

### 3.2 A capability-typed stdlib

Every builtin carries an effect row, enforced, with **per-argument** path
classification — `classify_call_paths` returns `Option<Vec<(IoKind, usize)>>`
(`capabilities.rs:77`; the older `classify_call` at `:92` still returns a single
`Option<IoKind>`) because enforcement reads the argument at a recorded index, and
`file_copy(src, dst)` needs FsRead on argument 0 and FsWrite on argument 1. A kind
*list* without indices would have been unsound.

Python cannot tell you what a function touches. Axon can refuse to compile code
that touches what it did not declare.

The target is **zero unclassified I/O builtins**, and the framing matters: an
unclassified path argument is not a missing feature, it is a sandbox escape,
because the path is never checked against the `@[contained]` allowlist. This is
the value wedge — "AI code sandboxed by the compiler" — and it is worth more than
any amount of stdlib breadth.

### 3.3 Zero silent-wrong-answer paths

This is the sharpest property for an RLM engine, and the least glamorous.

A single session found four instances:

* `str_slice` returned `""` for a byte range splitting a UTF-8 character;
* `"a{2,3}"` compiled to the string `a2` — the interpolation slot evaluated `2`
  and discarded `,3` with no diagnostic, so every counted regex repetition
  silently searched for a different pattern;
* `$5` in a replacement expanded to nothing when the pattern had two groups;
and a fourth that belongs in a different bucket: `base64_decode` **refuses** input
it should decode, because there is no bytes type to return. That one is a loud
refusal — a capability gap (specced as R43), not a silent wrong answer. It is listed
here only because it was found in the same sweep, and it actually demonstrates the
discipline rather than violating it.

Many mainstream stdlibs ship these as documented quirks — Rust's regex crate
substitutes empty for a `$5` that has no group. Python, to its credit, does NOT:
`re.sub(r'(a)(b)', r'\5', 'ab')` raises `re.error: invalid group reference 5`. So
Axon's E2205 matches Python here rather than beating it, and the honest claim is
about *breadth* of the discipline, not novelty on this one case.

**Why it matters more here than anywhere else:** a silent wrong answer teaches the
model to trust a broken tool. A loud failure is a repair signal — the model reads
the error and fixes the call. A quiet one becomes a wrong belief that propagates
into every downstream cell and poisons self-improvement loops, which are precisely
what Axon is for. And it is *testable*: the discipline is expected-VALUE gates
rather than agreement gates, because an agreement oracle (compare interp against
native) is structurally blind to any bug both engines share. The UTF-8 bug lived in
the fuzz corpus, with non-ASCII inputs, and the fuzzer could never have found it.

The claim to earn: **every stdlib function either returns the right answer or
refuses loudly.** No one else claims this.

### 3.4 Machine-checked contracts on stdlib signatures

Refinement types (`T where <pred>`) exist and are enforced at all four obligation
sites — parameters, returns, struct construction, and let-bindings. Putting
`where start <= end` on the index-taking builtins would make Axon the only
language whose stdlib preconditions are *checked* rather than documented.

Most languages write the precondition in a doc comment and hope. Axon can make
violating it a compile error when the arguments are constant and a clean exit-6
refusal when they are not.

### 3.5 Per-run cost accounting

No mainstream language can tell you what a run cost. Axon meters AI calls
per-token and debits them from the calling principal's carved budget, so authority
and spend are one model rather than two bookkeeping systems that can disagree
(`crates/axon-core/src/kernel.rs:678-715`). Exhausting a kernel-goal budget stops
the run with its own exit code (7) rather than a generic failure, so a supervisor
can distinguish "out of money" from "broken".

This was already built and went unclaimed through this paper's first draft — which
is its own small lesson about a project whose docs lag its code. For an ASI
substrate, bounded-cost-by-construction belongs next to bounded-CPU: an agent that
can spend without limit is unsafe in a way that has nothing to do with memory
safety. Note the honest boundary — this bounds *spend*, not CPU. `while true {}`
still burns unbounded compute with no capability at all, and real containment there
needs fuel metering, which does not exist.

### 3.6 Deterministic concurrency

Scheduler order is a function of spawn order plus `AXON_SEED`: ready fibers run in
spawn order rotated by the seed (`crates/axon-core/src/kernel.rs:327-400`). So a
concurrent Axon program is reproducible by default, and concurrency does not
silently opt a program out of §3.1.

Every mainstream language gives you OS-scheduler nondeterminism the moment you
spawn anything, which is why concurrency bugs are the canonical
"unreproducible" class. Getting determinism there normally requires a special
tool — a deterministic replay debugger, a model checker — rather than being the
default. It costs nothing to claim here because it is already true.

## 4. Ranked, for ASI specifically

1. **Replayability** — auditability's foundation.
2. **Loud failure** — a model cannot repair what it cannot see.
3. **Capability enforcement** — containment.
3b. **Cost accounting** (§3.5) and **deterministic concurrency** (§3.6) — both
   already built, both unclaimed until this revision. They rank here rather than
   lower because each is a precondition for the ones above being *usable*: an
   agent with no spend ceiling cannot be contained, and a concurrent program that
   cannot be replayed cannot be audited.
4. **Bytes + hashing** — more ASI-relevant than it looks: content-addressed
   hashing is what makes agent memory, caching and dedup possible, and it is
   currently at 0%. Specced as R43.
5. **Time and dates** — scheduling and logs.
6. **CSV** — tabular data is the form real data tasks arrive in.

Items 1–3 are cheap relative to their value. Item 4 is a real build. Items 5–6 are
ordinary stdlib work and should be driven by measured task failures, not by
checklists.

## 5. The goal, restated as numbers you can gate on

Replace "stdlib % complete". But be honest about which replacements are *gates* and
which are only dashboards — an earlier draft of this section called all three
gateable, and two of them are not.

**One real gate:**

| Metric | Target | Now |
|---|---|---|
| Builtins with a host/entropy/time/model effect and no replay story | 0 | **1** (`host_await*`), plus one named exemption (`dstore_*`) |

This one works because the set is *enumerable from the builtin table plus its effect
rows*. It is now an actual CI check, not a proposal
(`replay::tests::every_host_method_is_both_recorded_and_replayed` and
`no_interp_builtin_reaches_the_world_directly`): the first asserts every
`AxonHost` method is present in both the recording and replaying hosts, the second
that no interpreter builtin reaches the world around the seam. Both were verified
non-vacuous by deletion — remove a method and the test names it.

It earned its keep immediately: on its first run it found `dstore_*` (the durable
`Store`) reading and writing its log directly. That is exempted rather than fixed,
*in writing and with the reason*, because closing it needs a `file_remove` on the
trait and that method is deliberately absent pending a human risk decision (R42 §9
Q3). Turning the test green by adding it would have been making a reserved TCB
decision to satisfy a lint. The exemption itself asserts it still matches
something, so a stale allowlist cannot silently cover a future bypass.

**A second real gate, added 2026-08-08 — repair rate:**

| Metric | Target | Now |
|---|---|---|
| RLM benchmark: tasks recovered by ONE repair round | rising, per-task | **+1 of 3 failures** (5/8 → 6/8), 3 runs, zero spread |

This was the §7.1 proposal, and it turned out to be measurable and decision-moving
rather than merely appealing. It is a gate and not a dashboard for one specific
reason: unlike `tasks_hard`, it is reported **per task with its failure text**, so
a regression names which task stopped repairing and why. The count alone would have
the same variance problem.

It also produced the first non-zero repair gain this project has measured. For six
runs the number was 5/8 → 5/8; after fixing two diagnostics it is 5/8 → 6/8, with
first-try unchanged — so the gain is attributable to the repair round, not to better
generation. The task that moved is precisely the one whose diagnostic was fixed.

The mechanism is worth stating because it generalises: the old help for an
`i64`-where-`str`-expected argument said *"cast with `as str` if compatible"*, and
there is no such cast — the model was being pointed at a dead end by a diagnostic
that was **confidently wrong rather than silent**. That is the §3.3 failure mode
wearing a different hat: a wrong suggestion costs a whole repair round, which for
an agent is worse than no suggestion.

**Two dashboards, not gates:**

*`tasks_hard` pass rate.* 16 binary outcomes per arm, and this project has watched
it swing 9 → 6 → 4 on card text alone. "Rising" is not a threshold, and the variance
is far too high to fail a commit on. Track it per-task (which task, which failure)
rather than as a total — R42's stop condition already insisted on per-task
attribution for exactly this reason.

*Count of known silent-wrong-answer paths.* This one is a trap, and worth naming as
such: **"0 known" is satisfied by not looking.** One session found four. The base
rate says more exist, so the number measures search effort, not the property.

What *is* gateable in its place is the **process** that finds them:

| Process metric | Why it is honest |
|---|---|
| Fraction of builtins with an expected-VALUE test, not parity-only | An agreement oracle is structurally blind to a bug both engines share (§3.3). This counts the tests that can actually catch one. |
| Found → closed latency | Measures whether discoveries get fixed, not whether we looked. |
| Red-team budget spent per release | Makes "we looked" a resourced activity rather than a claim. |

So the honest summary is: **two** gates, one dashboard, one discipline — the 
repair-rate metric graduated from proposal to gate on 2026-08-08, and the 
replay-coverage metric from proposal to CI check.

## 6. What this implies for the stdlib work that remains

Tier the surface rather than levelling it.

* **Tier A — target ~95%, driven by measured failures.** Strings, arrays, dicts,
  JSON, files, basic numerics, time formatting. A gap enters the queue when a
  `tasks_hard` failure traces to it. Currently queued: `arr_reduce`/`dedup`/
  `group_by`, `floor`/`ceil`/`round`, printf-style number formatting, ISO-8601.
* **Tier B — "exists and does not block", ~60% is fine.** Bytes, hashing,
  encoding, process/env, CSV.
* **Tier C — do not build; delegate.** YAML/TOML/XML, compression, sockets and
  servers, DB drivers, GUI. These go through `native::` FFI or userland. Each is a
  large surface with near-zero ASI-specific value.

The admission test stays as R42 defined it: an addition is admissible only if a
measured failure traces to its absence AND it either cannot be written in userland
Axon or is a soundness fix. That rule is what kept R42 from becoming a
feature-checklist exercise, and it is what should keep the Tier-A queue honest.

---

## 7. Properties this paper's first draft left out

A review of the draft found seven omissions. Several are things Axon **already has**
and simply does not claim, which is the cheapest kind of gap to close.

### 7.1 Error-message quality as the repair gradient (highest-value omission)

§3.3 argues loud failure beats silence. For a model, the *content* of the failure is
the signal: does the message name the fix? This session's own work is the
illustration — the interpolation error does not merely refuse `"a{2,3}"`, it prints
the corrected spelling `"a{{2,3}}"`, and E2205 says "use 9 or fewer capture groups"
rather than "invalid group".

That is measurable in a way §5's other candidates are not: **one-shot repair rate
after seeing the diagnostic.** Take the tasks a model fails, feed it the diagnostic,
and count how often the next attempt compiles. It is arguably a higher-leverage
metric for task pass-rate than replay, and it belongs in §5 as a second dashboard.

### 7.2 Cost and budget accounting — already built, never claimed

Per-token AI cost metering, per-principal budgets, and kernel-goal budgets with
exit-7 exhaustion all exist (Phase 7 / R12b). "No mainstream language can tell you
what a run cost" is exactly this paper's kind of claim, and it was missing. For an
ASI substrate, bounded-cost-by-construction sits naturally beside bounded-CPU.

### 7.3 Deterministic concurrency — already true, never claimed

Scheduler order is a function of spawn order plus `AXON_SEED`. No mainstream
language gives deterministic concurrent scheduling by default. It costs nothing to
claim and belongs in §3's list.

### 7.4 Checkpoint and resume

Replaying a ten-hour agent run is useless if review requires ten hours. The
suspend-to-host runtime (`host_await`, `FiberState::Suspended`) already exists;
replay-to-prefix plus resume, and durable checkpoints, are the natural extension of
§3.1 for agents that run long enough to matter.

### 7.5 A stability contract for generated code

The users are models emitting code against a language card. This project has
*measured* card drift changing outcomes — so what is the compatibility guarantee for
builtin signatures, diagnostic text, and the JSON schemas (`axon-deploy/1`,
`axon-ai-audit/2`, …)? A language whose surface churns silently invalidates the
competence its users already have. Nothing in the draft addressed this, and it is a
real risk for an "AI-first" language.

### 7.6 Self-modification safety

The Layer-3 firewall — four gates (interp-oracle correctness, capability
monotonicity, test preservation, performance) screening AI-authored compiler passes
represented as data — is the most ASI-specific thing in the repo, and the draft did
not mention it once.

### 7.7 The formal-verification ceiling

§3.4 stops at runtime exit-6. SMT discharge is already wired into the default
pipeline, so contracts provable for all inputs are *statically elided*. That is the
stronger version of §3.4's own claim: not "checked at runtime" but "proved where
provable, checked where not".

---

## 8. Revision note

This document was reviewed against the code, and the review found real errors in the
first draft: a misnamed function, a claim about Python that was backwards (`re.sub`
raises on a bad group reference — it does *not* fail silently, so Axon's E2205
matches Python rather than beating it), an unsourced headline statistic that in-repo
data partly contradicted, two live nondeterminism sources missing from §3.1's table,
and two of three "gateable" metrics that were not gateable.

Both missing nondeterminism sources were fixed rather than merely documented. The
rest is corrected above. That is worth stating plainly, because a paper arguing that
loud failure beats quiet wrongness has no business being quietly wrong.
