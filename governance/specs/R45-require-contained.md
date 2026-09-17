# R45 — `--require-contained`: containment as the default, not the opt-in

**Spec ID:** `R45-require-contained`
**Status:** Landed — implemented 2026-09-17; the forks below were resolved against measured behaviour before any code was written
**Risk class:** Behaviour change to a safety boundary, gated behind an explicit flag (default off).
**Author / date:** 2026-09-17, from `AXON_FOR_RLM.md` §4 — the last of that document's five
recommendations with no successor.

```spec-meta
id: R45-require-contained
status-claim: Landed
depends-on: R6-capability-security
blocks: none
blocked-by: none
supersedes: none
related: R44-accumulating-session, R11-capability-minting, R13-native-ffi
reserves: E1005 (confirmed free at spec time — grepped across crates/ and every governance/specs
  `reserves:` line; E1001-E1004 are R6's, E1005 is the next free code in that band)
evidence: cargo test -p axon-core --test cli_run require_contained (7 tests) + session_honours_require_contained_per_cell + rlm_host_end_to_end_typed_contained_replayable_session (ALL PASS 2026-09-17; each RED against the pre-flag binary, and the off-by-default guard mutation-proved against an always-on mutant)
```

---

## 1. Why

`@[contained]` is **opt-in**, so the compiler is a boundary only over code that asked to be bounded.
Verified, not assumed:

```
$ cat leak.ax
fn leak() -> str { match read_file("/etc/passwd") { Ok(s) => s  Err(e) => "" } }
fn main() { println(leak()) }

$ axon check leak.ax ; echo $?
0                        # silent pass. no diagnostic of any kind.
```

For an RLM host that is the wrong default. The host would have to **inject** the annotation into
model-written source — the fragile rewrite-the-model's-output pattern the design was trying to avoid.
A flag makes the annotation a *grant* rather than an opt-in, and the host passes a flag instead of
editing code it did not write.

---

## 2. Fork 1 — which functions get the default?

Three candidates, and the wrong choice makes the flag unusable rather than merely imperfect.

### (a) Every function in the program — REJECTED, measured

`load_use_decls` merges imported module items into one `Program`, so "every function" includes every
function of every imported module. Measured: importing a module that merely *contains* a `read_file`
fails the build **even when the program never calls it**.

```
mod lib                      # lib.ax has an IO helper the program does not use
use lib.{pure_helper}
fn main() { println(to_str(pure_helper(21))) }
→ E1001 read_file not permitted        ← a false positive on unused code
```

Any real library would fail. This is the option that looks obvious and does not survive contact.

### (b) Functions defined in the entry file — REJECTED, not expressible

`FnDef` carries no origin marker and `Span` is byte offsets with no file, so after the merge an
entry-file function is indistinguishable from an imported one. Adding an origin field touches the
parser and every construction site — a real change, and it buys worse semantics than (c).

### (c) `main`, and everything it transitively reaches — **CHOSEN**

Apply the deny-all default to the entry point and let the **existing** transitive walk do the rest.
It already follows calls into user helpers and imported functions, and already refuses to let a
helper launder I/O one call away.

Measured, all three cases:

| Case | Result |
|---|---|
| import an IO module, never call it | **passes** — no false positive |
| call the imported IO helper | **E1001** — no laundering through a module |
| hide the I/O in a local helper | **E1001** — no laundering through a helper |

Semantically it is also the strongest statement: *what can this program reach?* A function nothing
calls cannot do anything, so requiring it to be annotated protects nobody.

**Blast radius, measured over the example corpus:** 115 programs check clean today; **20 are refused**
under the flag, and all 20 genuinely reach the filesystem, the network, or a model. 83% are unaffected.

### 2.1 The limit this leaves, stated

Functions invoked **by name at runtime** are not reachable from `main` statically — `sandbox_run("f", …)`,
and the deploy gates (`redteam_check`, `assert_deployable`) that `axon deploy` calls directly. Under
this flag they are unchecked unless they carry their own `@[contained]`.

That is a real hole and it is **named rather than papered over**. v1 does not close it; a follow-on
may extend the default to the known gate entry points. Do not describe this flag as total containment.

---

## 3. Fork 2 — what does the diagnostic say?

The existing message is actionable but, under this flag, **blames an annotation the user never wrote**:

```
E1001  `read_file(...)` is not permitted by @[contained]
       help: add an `fs: [read("...")]` clause
```

There is no `@[contained]` on that function. Saying "not permitted by @[contained]" describes a
restriction the author cannot see in their own source.

**Resolved: a distinct code, E1005.** Not merely a reworded E1001, because the two are genuinely
different conditions and a host should be able to tell them apart:

* **E1001** — you granted capabilities and reached outside them. *Widen the grant, or stop.*
* **E1005** — you granted nothing and `--require-contained` is in effect. *Declare what you need.*

The E1005 message must name the entry point, the operation, and the clause that would permit it.

---

## 4. Surface

```bash
axon check --require-contained prog.ax    # refuse un-granted I/O reachable from main
axon run   --require-contained prog.ax    # same gate before execution
```

Default **off**. It is a host's decision, not a language change: a flag that altered the meaning of
existing source by default would break every program in §2's 17%.

```bash
axon session --require-contained           # per cell (R44 §4 S8)
```

> **Correction, 2026-09-17.** This section claimed the session composition while the flag existed
> only on `check` and `run` — a false claim in a spec marked Landed, which is precisely the drift
> `AXON_REFERENCE.md`'s gate exists to prevent, introduced in the same session that built the gate.
> The flag is now on `session` too, verified to apply **per cell** and to refuse a helper declared in
> one cell and called from another. `session_honours_require_contained_per_cell` pins it so the claim
> cannot go stale again.

---

## 5. Semantics

| # | Rule |
|---|---|
| S1 | With the flag, a `main` with no `@[contained]` is checked against a deny-all spec (empty fs/net allowlists, `exec: none`). |
| S2 | An explicit `@[contained]` on `main` **wins** — the flag sets a default, it does not impose a ceiling. Widening is the author's stated intent, and R11 attenuation still governs what a principal may actually hold. |
| S3 | Enforcement is the existing transitive walk. No new analysis, therefore no second mechanism to drift (I-2). |
| S4 | A violation is **E1005**, distinct from E1001 (§3). |
| S5 | Without the flag, behaviour is byte-identical to today. This is checked, not assumed. |
| S6 | A program with no `main` (a library) is unaffected — there is no entry point to contain. |

---

## 6. Test plan

* the §1 `/etc/passwd` program: exit 0 today, **E1005** with the flag;
* an unused IO import: **passes** (the false positive that killed option (a));
* a called IO import: **refused** (no laundering through a module);
* a local helper: **refused** (no laundering through a helper);
* an explicit `@[contained]` on `main` granting the read: **passes** (S2);
* without the flag, the whole example corpus behaves exactly as before (S5);
* a library with no `main`: unaffected (S6).

Each must be RED against the pre-flag binary, and the S5 case must be shown to *fail* if the default
is applied unconditionally — a flag that is always on would satisfy every other test here.

---

## 7. Stop condition

```
DONE = the §1 program is refused WITH the flag and passes WITHOUT it
   AND an unused IO import does not false-positive
   AND neither a module nor a local helper can launder the I/O
   AND E1005 names the entry point and the clause that would permit the call
   AND §2.1's limit is documented in the flag's own --help text, not only here
   AND the full suite is green in every configuration the gate runs, each reported with its
       configuration (R42 §12.1)
```

§2.1 in the help text is not optional. A containment flag that quietly does not cover runtime-dispatched
entry points, while being described as "require contained", is the absent-vs-passed defect this cycle
spent seven surfaces fixing.

---

## 7.5 Outcome

Landed as specced. `--require-contained` on both `check` and `run`; **E1005** distinct from E1001;
the deny-all default applies to `main` and the existing transitive walk covers everything it reaches.

```
$ axon check leak.ax                       # unchanged: exit 0
$ axon check --require-contained leak.ax
E1005  `read_file("/etc/passwd")` is reached from `main`, which declares no
       capabilities — --require-contained needs an explicit grant
       help: Add `fs: [read("/etc/")]` to the @[contained(...)] attribute
```

7 tests, each RED against the pre-flag binary. Two mutation proofs, because two of the properties
here are the kind that pass on broken code:

* **always-on mutant** — the off-by-default guard caught it via its own precondition ("without the
  flag this must still pass, or the test below proves nothing about the flag"). Without that
  precondition, a flag wired permanently on would have satisfied every other test in the file.
* **relabel-dropped mutant** — E1001 leaked through instead of E1005, caught.

One implementation note worth carrying forward: the E1001→E1005 relabel is done **centrally**, on the
errors the walk produced, rather than at each of the eight emission sites. A rule applied per-site is
a rule that misses one.

---

## 8. Open questions

| # | Question | Default |
|---|---|---|
| Q1 | Extend the default to known runtime entry points (`redteam_check`, `assert_deployable`, `sandbox_run` targets)? | Not in v1. Name the limit (§2.1); revisit with a real ask. |
| Q2 | Should `axon deploy` turn it on automatically, as it already does for `AXON_STRICT`? | Tempting and deferred. Deploy is the consequential path, but flipping it silently would refuse 17% of existing programs at the worst moment. Decide with a migration note. |
| Q3 | An env-var form (`AXON_REQUIRE_CONTAINED`) for hosts that cannot pass flags? | Yes if asked; it would need a registry row and both-direction gating like every other var. |
