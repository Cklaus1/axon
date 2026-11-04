# neural-programs - spec (NP-0..NP-5): the first learned-function slice

**Status: DRAFT (after Step 1 review). Not authorized for build.**

**Stop in force at NP-0: STOP-1v (value) fires.** `[REVISED: NPD-2]` The repo names no keyless consumer of `cortex repair`'s ranker
(§2 consumer search). Until the human names one with a frequency (H6), NP-1..NP-5 are **conditional** and written for that case.
The NPD-3 oracle ceiling does **not** fire: a perfect tie-break gains +7/22 on the pool, and the floor is 3.

### Step 1 review

Sixteen findings from AuditNeural (NPA-1..8) and DevloopNeural (NPD-1..8). **16 accepted, 0 rejected.** Each was re-verified with
scripts in `/var/tmp/r51-tmp/NeuralPrograms/` (`oracle.py`, plus inline checks on the `results-regress` rows).

| id | sev | finding | disposition | where |
|---|---|---|---|---|
| NPA-1 | blocker | the reproduction check compared against `ranked_top` (the spectrum top), so it would abort on every run | accepted; re-verified: `order` ≠ `ranked_top` as sets on 20/68 (9b5) and 22/69 (1d8, f26) | §3 NP-1.3, §1 |
| NPA-2 | blocker | the reordered sets are 68/69/69 tasks (intersection 67, union 70) | accepted; H_r = the 68 9b5 ids, sha256 `17214f55…4ead`; per-run denominators 68/67/67 | §2, §3, §4 |
| NPA-3 | major | N0 looked at H (LocRank ⊂ receipt, 60/60), and that shaped the candidates | accepted; disclosed, and the features and C1 variants are frozen in this text | §2, §4 |
| NPA-4 | major | the 22 pool tasks come from 8 files; the `other` stratum has 0 dev tasks | accepted; re-verified sizes [1,1,2,2,2,4,5,5]; `other` on H is report-only | §2, §4 |
| NPA-5 | major | the C2-vs-C1 margin of ≥ 2/22 selects on noise; leftover train/dev wording | accepted; nested leave-one-cluster-out (LOCO) + exact sign test p ≤ 0.10 AND net ≥ 2 (p ≤ 0.10 already implies net ≥ 4 for d ≤ 22, computed) | §3, §4 |
| NPA-6 | major | STOP-1 could be read two ways | accepted; ordered integer rule, net ≥ 3 | §2 |
| NPA-7 | major | no readiness state or g01 row for "offline" | accepted; `verify` → `ready|partial`, M4867/M4868; NP-5 adds a mismatch row each for base, tokenizer and template | §3, §5 |
| NPA-8 | minor | stale anchors | accepted; `MAX_PROMPT_CHARS` is `rank.rs:46`; the hidden-name refusal is at `:628-631`/`:710` | §3, §4 |
| NPD-1 | major | costs were guessed; the capped run is a subset | accepted; measured rows, per-iteration column | §6 |
| NPD-2 | blocker | no stop rule tied to value | accepted; STOP-1v; repo search finds no consumer, so **it fires now** | status, §2 |
| NPD-3 | major | the cheapest kill was not first | accepted; order is value → oracle → C1 → C2 → PREREG; oracle +7 ≥ 3, does not fire | §2, §7 |
| NPD-4 | major | the NP-5 GPU text left out binding cere rules | accepted; H3 rewritten (owner approval, probe ticket, ≤ 3 attempts) | §9 |
| NPD-5 | major | lane 1's ownership contradicted the S23 wiring | accepted; selftest is run by hand; `gate.sh` untouched unless NP-2 starts | §6, §7 |
| NPD-6 | major | no strict gate is needed if the work stops at NP-0/NP-1 | accepted; `GLOBAL_INPUTS` = Cargo.toml/Cargo.lock/rust-toolchain.toml (`gate_lane.py:846`, observed) | §6 |
| NPD-7 | minor | no need for an `axon-reflex` dependency | accepted; `StateHandle` is cited as a pattern only | §3, §7 |
| NPD-8 | minor | Lane B's NE-2 edits four `axon-cortex/src` files (`nhr-evidence-spec.md:316`) | accepted; NP-4 runs after Lane B merges and pins the cortex sha256 | §3, §7 |

Subject: the first vertical slice of Axon neural programs (CX-23/CX-36 "typed learned-function artifacts"). It adds a NEW crate,
`crates/axon-neural`, reached only through a host/library adapter. It adds no language syntax and does not edit `crates/axon-cortex`
or `crates/axon-core/src/interp*`. Wiring it into `cortex repair` is a later integration spec. Base: `b4/specs` = main 9300d9fd.
Every anchor below was read at that commit. Probe binaries: `/home/cklaus/projects/axon-lanes/.doc-build/b4-draft/debug/{axon,cortex}`
(`axon 0.1.0 (9300d9fd)`). Probe scripts and outputs are in `/var/tmp/r51-tmp/NeuralPrograms/`. No model was called, no GPU
command was run, and nothing was spent.

**Result.** N0 (§2) finds one eligible function, **reordering `cortex repair`'s localization candidates** (`rank.rs`, `--ranker`).
Its labels are mechanical: the injected defect's symbol. The paid default ranker (Haiku 5.5) puts the truth first on 68/68 of H_r.
The **keyless fallback, the spectrum order, gets 29/68**, so headroom exists only over the keyless path. That path has no named
consumer `[REVISED: NPD-2]`, and the clean training pool has only 22 multi-candidate tasks from 8 files `[REVISED: NPA-4]`. The
expected outcome is therefore a stop: STOP-1v now, or "C1 rule sufficient" at STOP-1 `[INFERENCE]`. The candidates come simplest
first (COGNITIVE_SPECIALIZATION.md:16): C1 a deterministic tie-break, C2 a ≤ 4-weight linear scorer, NP-5 a human-gated LM adapter.

## 0. Source entries

| item | source | sev |
|---|---|---|
| NP-0 | N0 profiling (task brief); CX-22 §4 eligibility (`docs/axon_cortex_v0_15/axon-cortex-build-v0_15/specs/CX-22-cognitive-specialization-compiler.md:69-85`); COGNITIVE_SPECIALIZATION.md loop steps 1-4, authority exclusion `:23` | P0 |
| NP-1 | PREREG and frozen splits: CX-36 §10 (freeze before protected evaluation), CX-22 §6 | P0 |
| NP-2 | CX-36 §4-§8 (`/home/cklaus/projects/axon/docs/axon_cortex_v0_23/axon-cortex-build-v0_23/parent/axon-cortex-build-v0_22/specs/CX-36-neural-program-artifact-contract.md:39-166`); D42 (`DECISIONS.md:81`); G23-artifact (`build/ACCEPTANCE_GATES.md:203`) | P1 |
| NP-3 | CX-36 §12 (`:223-247`), §17 (`:281-285`); D43/D44 (`DECISIONS.md:82-83`); G23-typed-output/-no-fallback/-authority/-offline (`ACCEPTANCE_GATES.md:204-207`); G22-applicability/-fallback (`:199`, `:202`); NEURAL_PROGRAMS.md:32-34 | P1 |
| NP-4 | NEURAL_PROGRAMS.md first slice, steps 3, 7, 9-11 (`build/NEURAL_PROGRAMS.md:13-27`); G22-comparison/-roi (`:198`, `:200`); CX-23 §11 | P1 |
| NP-5 | CX-23 §5 adapter mode; CX-36 §5 LoRA binding (`:115-117`) | P2, human-gated |

### Out of scope (logged, not dropped)

| what | why not here | where it goes |
|---|---|---|
| Language syntax (`neural fn`, `np://`, `axon neural`) | CX-36 §2 (`:31`): these are sketches; initial use is a host/library adapter | H4 |
| Wiring `cortex repair --ranker np:<digest>` | needs `axon-cortex/src` edits plus a receipt regress (≈ $0.14) | integration spec after NP-4; integrator O-entry |
| `.np` ZIP container (G36-container) | the directory store keeps the same identity (§8 decision 2) | O-entry "np ZIP container reader" |
| Shared-base cache, G23-cache | no base model for C2; reported `not-applicable` | NP-5 or later |
| Sidecar/remote modes, ProgramAsWeights import, CX-11 activation, drift loop | step 11 asks for evidence, not activation | later specs |
| `route55` advice (n=60, majority 0.600, best 0.933; `axon-reflex/benchmarks/decisions/results/usecases.md:119-123`) | labels come from one noisy run; no deterministic incumbent measured | O-entry for the CX-22 registry |
| Unused C2 features (body lines, operator/literal counts, called-by-another-eligible) | STOP-0(e) allows ≤ 4 weights | a later spec with more data |

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P0 | NP-0 (done), NP-1 (**conditional on H6**) | free, CPU only; both stop before any crate code. **NP-0 has stopped the slice at STOP-1v** `[REVISED: NPD-2]` |
| P1 | NP-2, NP-3, NP-4 (conditional) | built only if STOP-1 selects C2 AND STOP-1v passes. No shipped binary changes |
| P2 | NP-5 (conditional) | only under STOP-3 |

**Risk to existing programs:** none by construction. No shipped crate is edited, no valid program can be falsely refused, and no
`axon` sha256 pin moves. The silent-wrong-answer risk lives in the harness. The NP-1.3 identity check guards it: fresh eligible
lists against the recorded `ranker.order` as a set `[REVISED: NPA-1]`, not the earlier "= `ranked_top`", which is the spectrum
top list and would have aborted every run.

## 2. NP-0: N0 first cut (done; the build commits the scripts)

Commands (`cwd = crates/axon-cortex/benchmarks`; scripts in `/var/tmp/r51-tmp/NeuralPrograms/`):
- `n0_rank.py`: truth-first per receipt task.
- `n0_ties.py`: tie structure on LocRank's 60 tasks.
- `n0_pool.py` + dedup.
- `AXON_BIN=<draft> PER_CLASS=100000 PER_FILE_CLASS=60 python3 -B ab/corpus.py pool_uncapped.json $(cat pool_clean.txt)`: 4 min 4 s.
  The earlier capped run (10 min 6 s) yielded the same 45 tasks and is dropped as a measured subset `[REVISED: NPD-1]`.
- `locrank.run_locate` on the pool (4.2 s).
- `oracle.py`, `power.py`.

| candidate function | recurrence | labelled examples | incumbent, measured | headroom | N0 verdict |
|---|---|---|---|---|---|
| **loc-rank**: order `cortex repair`'s eligible candidates (`rank.rs:1-35`, `select` `:106-114`; no key → spectrum) | model-reordered tasks per receipt run: 68/69/69 in the 3 latest of 20 `ab/results-regress/` runs `[REVISED: NPA-2]` | receipt corpus 115 (`scorecard/receipt-corpus.json`, sha256 `7ad2a9f1…2a7c`). Clean pool: 45 tasks, 22 multi-candidate from **8 files** | on **H_r** (the 68 9b5b10355224 reordered ids, `H_r.txt` sha256 `17214f55998fba61d85960fc76e5c1c5757f2a866385536b738495a0c1ac4ead`): spectrum 29/68. Haiku 68/68 (9b5), 67/67 (1d8 on H_r), 66/67 (f26 on H_r). $0.000571/call, p50 1815 ms | +39/68 over keyless; ≤ 1/67 over paid | **ELIGIBLE** on quality; **value: STOP-1v fires** (below) |
| BuildLog → DiagnosticSummary | every `axon check` | n/a | exact (`axon check --json`: two `axon-diag/1` lines, rc 2) | 0 | **STOP** |
| `ai_extract`/`ai_complete` | 38 + 25 call texts | 0 recorded outcomes (`provenance.rs:192-195` hashes only) | none | unknown | **INELIGIBLE** |
| genviab synthesis | 204 metadata records | ≤ 28 tasks | arm grades | n/a | **OUT** (output is code) |
| DecisionBench trust/audit/risk/tamper/gate/repair | per site | 36-48 | tables | n/a | **EXCLUDED** (authority) |
| DecisionBench axbool/improve | per site | 48/40 | labels are deterministic outputs | 0 | **STOP** |

**Statistics already computed on H (disclosed; PREREG repeats them)** `[REVISED: NPA-3]`. Spectrum 74/115 overall and 29/68 on
H_r. Haiku as above. On the LocRank subset (60/60 receipt ids): spectrum 25, truth in the top score tie 47, uniform-random
tie-break 25.3, truth strictly below the top 13. The corpus field `visible_calls_symbol_directly` was seen (as a field name, not
tabulated). These looks motivated C1's tie-break, which is why the features and variants are frozen in §4 now, before NP-1. Apart
from the NP-1.3 identity check, NP-1 computes nothing on H, and that check is logged in `heldout-spent.jsonl`.

**Protected-partition hygiene (observed).** 13 pool files are `hard-once` sources (`protected.py:39-42`) and 1 is a `hard-dev`
source; they are dropped with their basename twins. `headroom/programs*` (134 files, feeding hx-dev/hx-test, `protected.py:44-48`)
is excluded. 11 copies of receipt sources are dropped (by basename or > 50 % shared lines).

**Pool profile (observed, `oracle.py`)** `[REVISED: NPA-4]`. 22 multi-candidate tasks from 8 files, per-file sizes [1,1,2,2,2,4,5,5].
Classes: operator 11, constant 5, comparison 4, boolean 1, drop-stmt 1. The `other` stratum has **0** pool tasks; on H_r it has 16 of
68 (mutation 52).

**Oracle ceiling (NPD-3; observed, < 1 s)** `[REVISED: NPD-3]`. On the 22 pool tasks the spectrum gets 9. A perfect tie-break (truth
chosen whenever it is in the top score tie) gets 16, a **gain of +7 ≥ 3: does not fire**. The probe also scored one tie-break key
that was later frozen as C1a: 7/22, net −2 against the spectrum. That was a pool (dev) look and is disclosed. No other variant was
computed.

**Consumer search (STOP-1v(i); observed)** `[REVISED: NPD-2]`. Question: does any keyless run of `cortex repair` use the ranker
order? Findings:
- `.github/workflows/*`: no `cortex` invocation.
- `--unattended` requires `--symbol` (`auth.rs:423`; envelope `repair_envelope.rs:613-614`), and `--ranker` is "Ignored with
  `--symbol`" (`bin/cortex.rs:124-133`). Not a consumer.
- `examples/cortex/run.sh:48` pins `CORTEX_RANKER=spectrum` for determinism, and gate CLI tests are deterministic fixtures. A
  learned order would not replace spectrum in either.
- A non-`ai` build always walks in spectrum order (`docs/CORTEX.md:43-49`, the benchmark-driver build). The receipt runs used an
  `ai` build with a key (`ranker.used = openrouter` on 68/115).
- Of the 20 regress runs, none was keyless. The exception is `2026-10-12-6b36f907572d`, a single row in spectrum order. Keyed
  runs still fall back to spectrum on 0-2 rows per run when Haiku's answer is invalid (for example "`restart_set` is not an
  eligible candidate"), which is ≤ 2/115 per run.

**No named consumer, no measured keyless frequency → STOP-1v fires now.** The documented-but-unmeasured case is an operator with a
non-`ai` build or no `OPENROUTER_API_KEY` running `--generator cmd:`/`ai:` without `--symbol` (H6).

### STOP rules (preregistered; evaluated in this order) `[REVISED: NPD-3, NPA-6]`

- **STOP-0 (eligibility; passed for loc-rank).** All must hold:
  (a) low-risk, with output checked by a deterministic validator;
  (b) labels are mechanical or outcomes, never one model's answers;
  (c) an incumbent measured on a frozen held-out set;
  (d) power ≥ 0.80 at α = 0.05 for an effect inside the incumbent's error.
  Against the paid incumbent (≤ 1/67 error), (d) fails.
- **STOP-0(e) (data floor).** Learned parameters ≤ n_multi/5: 22 tasks → ≤ 4 weights. NP-5 needs ≥ 200 multi-candidate training
  tasks.
- **STOP-1v (value; FIRES NOW)** `[REVISED: NPD-2]`. Work past NP-0 needs (i) a named keyless consumer (CI without a key, an offline
  user, or an unattended mode) with a stated repair frequency f per week. NP-2 also needs (ii) f × (CV Δ top-1 of C2 over C0) ×
  68/115 ≥ 5 extra truth-first answers per week. A saving on the paid default (≤ $0.04 per run) never counts. When this fires,
  the report is "no consumer", and if NP-1 ran, `results/dev.json` is committed.
- **STOP-1o (oracle ceiling; does not fire, +7).** A perfect tie-break gaining < 3/22 → "rejected" before any harness exists.
- **STOP-1 (nested-LOCO CV, before any `crates/axon-neural/Cargo.toml` exists)** `[REVISED: NPA-6]`:
  (1) compute net-vs-spectrum (wins − losses) for C1 and C2; a candidate qualifies at net ≥ 3;
  (2) neither qualifies → "rejected";
  (3) apply the §4 selection rule among the qualifiers;
  (4) C1 selected → stop the neural slice ("deterministic rule sufficient"), hand C1 to a cortex spec, build no crate;
  (5) only C2 selected (and STOP-1v(ii)) starts NP-2.
- **STOP-2 (NP-4).** P1 fails → rejected; recorded in the evidence bundle (CX-36 §10).
- **STOP-3 (NP-5).** Requires P1 passed, S1 failed, STOP-0(e)'s 200-task floor met, and H1-H3 answered.

## 3. Items

### NP-1 (P0, conditional on H6) Pool, nested LOCO, C0/C1/C2, PREREG

Location: `crates/axon-neural/benchmarks/locrank-np/` (stdlib Python; no Cargo files).
1. `pool.py`: the N0 pool rules and the uncapped `ab/corpus.py` run, writing `pool.json` with the generator/axon/cortex sha256s.
   New mutator classes may be added in this directory (importing `ab/common.py`, never editing `ab/`) only if preregistered
   before they are run.
2. `split.py`: clusters files (basename twins, > 50 % shared lines); 8 clusters at N0. **Held-out H_r** = the 68 ids in `H_r.txt`
   (sha256 above) `[REVISED: NPA-2]`. Each recorded Haiku run is scored on H_r ∩ its own reordered ids, with the denominator
   reported (68/67/67). Strata: `mutation` (52) vs `other` (16, transfer, report-only).
3. Identity check (fails closed) `[REVISED: NPA-1]`. Fresh `cortex locate` eligible lists (`locrank.run_locate`, `locrank.py:82`)
   must, for rows with `ranker.used` = openrouter, equal `ranker.order` as a set; for every other row they must have length ≤ 1.
   Any mismatch aborts. Logged in `heldout-spent.jsonl`.
4. Candidates (frozen in §4): C0 spectrum (name order within a tie, `locate.rs:392-397`); C1a-C1d; C2.
5. `PREREG-np.md`: §4 verbatim, the H statistics disclosed in §2, the STOP order, and digests. Its sha256 goes in the integrator's
   CHANGELOG line.

**Fail-first (`selftest.py`, run by hand)** `[REVISED: NPA-5, NPD-5]`:
- a pool task sharing > 50 % lines with a receipt source is refused;
- an H row whose fresh eligible set ≠ `ranker.order` set aborts;
- no cluster is in both the fit and the scored fold, at either nesting level.
**g01:** none (Python; precedent b3 B3-1).
**Acceptance:** `results/dev.json` holds the nested-LOCO table (C0/C1/C2 top-1/top-3/MRR), the sign-test p, and the STOP-1 and
STOP-1v verdicts. H is touched only by step 3.

### NP-2 (P1, conditional) `axon-neural`: source, manifest, identity, strict loader

1. **Source** `axon.nps/1`: parsed by `axon_canonical::parse_value` (`axon-canonical/src/lib.rs:125`: duplicate keys refused, no
   floats, depth ≤ 32, ≤ 1 MiB). Unknown keys are refused, and `schema` must be `axon.nps/1`.
2. **Manifest** `axon.np/1`, cl22 canonical bytes (`canonical_bytes`, `:249`), containing:
   - `target.execution_kind = "linear-listwise/1"`;
   - a `feature_extractor` dependency (id, revision, table digest);
   - `runtime_manifest_version`, the schema refs and `assets[] {path, role, sha256, bytes}`;
   - `offline_required: true` (CX-36 §17 `:283`) `[REVISED: NPA-7]`.
3. **Identity** = `sha256("AXON-NP" 0x00 "v1" 0x00 || M)` (CX-36 §6 `:127-133`).
4. **Store** `<store>/<digest>/`. The loader:
   - opens with `O_NOFOLLOW` and accepts regular files only;
   - restricts names to `[a-z0-9._/-]`, with no `..`, empty or absolute segments;
   - refuses undeclared members;
   - checks every size and sha256 before any parse;
   - accepts only finite decimal-string weights;
   - resolves by digest only, never by alias.
5. Refusals are `LoadError { component, expected, found }`.

**Fail-first** (`tests/artifact.rs`): M4850-M4856, M4861, M4862, M4865, M4866. **Acceptance:** G23-artifact.

### NP-3 (P1, conditional) Runtime: typed invocation, readiness, applicability, no fallback, offline

1. `load(digest, &PrincipalScope) -> Handle`, with the principal bound to the handle. The `axon_reflex::StateHandle` pattern
   (`axon-reflex/src/lib.rs:109-119`) is copied as a pattern, not a dependency `[REVISED: NPD-7]`. Another principal gets
   `Refused`.
2. `invoke` returns `ProposedValue{order, artifact_digest, input_digest, format_validation: Passed, semantic_validation:
   NotRun(..)} | Abstained | Refused | Failed`. `Canceled`/`OutcomeUnknown` are `not-applicable` (embedded, synchronous). The
   input is `rank::RankView`'s fields (`rank.rs:209-218`, `build_view` `:627`) plus spectrum scores as decimal strings.
3. The output is validated by `axon_cortex::rank::validate_ranking` (`rank.rs:121-143`). A non-permutation is `Failed`; ids are
   opaque (G23-authority).
4. Applicability: 2 ≤ eligible ≤ 6, `source` ≤ `rank::MAX_PROMPT_CHARS` (`rank.rs:46`) `[REVISED: NPA-8]`, and every candidate
   defined. Otherwise `Abstained`.
5. No fallback inside the crate: a missing or corrupt asset is `Refused`, never the spectrum order (the caller's fallback is at
   `rank.rs:180-187`).
6. **Readiness** `[REVISED: NPA-7]`: `axon-np verify` → `{"ready"}` or `{"partial": [component…]}`. `invoke` refuses unless ready.
7. Offline: there is no network code. `axon-cortex` is used with default features (`default = []`; HTTP only under
   `cfg(feature = "ai")`, `openrouter.rs:36-54`).
8. Bin `axon-np {inspect, verify, features, invoke}`, JSON in and out.

**Fail-first** (`tests/runtime.rs`):
- M4857-M4860, M4863, M4864;
- M4867 `partial_readiness_is_explicit`;
- M4868 `invoke_under_unshare_net_succeeds_and_missing_asset_refuses` (`bwrap --unshare-net`) `[REVISED: NPA-7]`.
**Acceptance:** G23-typed-output/-no-fallback/-authority/-offline, G22-applicability; G23-cache `not-applicable`.

### NP-4 (P1, conditional) Held-out evaluation and evidence bundle

1. Rust `features` matches the Python byte for byte on the pool, and the CV table re-run through `axon-np` equals `dev.json`.
2. Runs **after Lane B merges** (NE-2 edits `axon-cortex/src`), with the `cortex` sha256 pinned in the ledger row `[REVISED: NPD-8]`.
   H_r runs ONCE, ledgered before the run, with 3 byte-identical repeats.
3. `results/evidence.json`: digest, PREREG sha256, per-claim verdicts, the CX-22 §6 matrix, and `decision: candidate-for-CX-11 |
   rejected | inconclusive`. No activation.

### NP-5 (P2, conditional, human-gated) Small-LM listwise adapter

Only under STOP-3. The base, tokenizer, LoRA adapter and template are pinned as CX-36 §5 dependencies. CPU inference is the
default; training is H3. A bare base is `Refused`. Its spec amendment must add a g01 mismatch row each for base, tokenizer and
template `[REVISED: NPA-7]`, plus a DEPENDENCY_ADOPTION review of the inference runtime.

## 4. Evaluation design and power (frozen now) `[REVISED: NPA-3, NPA-5]`

- **C2 features (4, frozen):**
  - f1 = in the top score tie (0/1);
  - f2 = spectrum position (0-5);
  - f3 = directly called in the body of a visible failing check (0/1);
  - f4 = Jaccard of `_`-split name tokens with the visible failing check names.
  The hidden check is never an input (stripped at `rank.rs:628-631`; a request still naming it is refused at `:710`)
  `[REVISED: NPA-8]`. Pairwise logistic loss, L2 with λ ∈ {0.01, 0.1, 1, 10}; ties broken by spectrum position.
- **C1 variants (frozen).** Each reorders within the top score tie only, then by spectrum position:
  - C1a: f3 = 1 first (pool look: 7/22, disclosed);
  - C1b: f3 = 0 first (callee first);
  - C1c: fewest body lines first;
  - C1d: highest f4 first.
- **Selection (nested LOCO over the 8 clusters).** The inner LOCO picks λ and the C1 variant; the outer LOCO scores them. C2
  is selected only if an exact one-sided sign test on the C2-vs-C1 discordant outer-fold tasks gives p ≤ 0.10 AND net ≥ 2.
  Otherwise C1 is selected. With d ≤ 22, p ≤ 0.10 already needs net ≥ 4 (computed: d = 4 → 4/4, d = 9 → 7/9), which meets NPD-2's
  4/22 bar. `other` has no CV tasks and is not a selection condition. Only the selected candidate touches H, so it faces one test.
- **P1 (primary):** on H_r (n = 68), a one-sided exact McNemar on truth-first gives p ≤ 0.05 AND Δ top-1 ≥ +0.10 (≥ 7 net tasks).
  Harm floor: net loss ≤ 2 in each stratum. The `other` gain is report-only `[REVISED: NPA-4]`.
- **S1 (secondary, vs paid):** mutation stratum only (52) `[REVISED: NPA-4]`, Clopper-Pearson lower bound ≥ 0.90, i.e. ≥ 51/52
  (0.912; 50/52 gives 0.884, computed). Each Haiku run is reported with its own denominator.
- **Noise:** deterministic arms, so the variance is task sampling (paired Bernoulli).
- **Power** (`power.py`, n = 68, spectrum 0.426): Δ +0.12 → 0.681, +0.17 → 0.904, +0.22 → 0.980; false-pass 0.017. AuditNeural's
  joint simulation with all P1 conditions: 0.668/0.899/0.016. MDE ≈ +0.15.

## 5. g01 rows (M4850-M4869; M4850-M4868 used, M4869 spare)

Format as `v022_g01_mutations.py`. The find strings are post-build `[INFERENCE]`. Each row has an `ATTACK:` marker. Crate
`axon-neural`.

| id | mutation | killing test |
|---|---|---|
| M4850 | the digest drops the `AXON-NP\0v1\0` prefix | `artifact_digest_is_domain_separated` |
| M4851 | asset sha256 skipped | `a_mutated_weight_byte_is_refused_naming_the_asset` |
| M4852 | undeclared members ignored | `an_undeclared_member_is_refused` |
| M4853 | `O_NOFOLLOW` dropped | `a_symlinked_member_is_refused` |
| M4854 | feature-extractor revision not compared | `a_feature_extractor_mismatch_is_refused` |
| M4855 | runtime manifest version not compared | `a_runtime_manifest_mismatch_is_refused` |
| M4856 | resolve falls back to `name` | `an_alias_never_resolves_an_artifact` |
| M4857 | `validate_ranking` skipped | `a_non_permutation_output_is_a_validation_failure` |
| M4858 | a missing asset returns the input order | `a_missing_asset_is_refused_not_spectrum` |
| M4859 | applicability guard removed | `an_input_outside_the_envelope_abstains` |
| M4860 | principal check removed | `a_handle_is_refused_to_another_principal` |
| M4861 | source parsed with `serde_json::from_str` | `a_source_with_a_duplicate_key_is_refused` |
| M4862 | `schema` check removed | `an_unknown_source_schema_is_refused` |
| M4863 | receipt omits `input_digest` | `every_proposed_value_binds_artifact_and_input_digests` |
| M4864 | equal-score spectrum tie-break removed | `equal_scores_keep_spectrum_order` |
| M4865 | `..` accepted in asset paths | `an_asset_path_escaping_the_store_is_refused` |
| M4866 | non-finite weights accepted | `a_non_finite_weight_is_refused` |
| M4867 | `verify` reports ready with an asset missing `[REVISED: NPA-7]` | `partial_readiness_is_explicit` |
| M4868 | a network call path is made reachable (std `TcpStream` connect in `invoke`) `[REVISED: NPA-7]` | `invoke_under_unshare_net_succeeds_and_missing_asset_refuses` |

## 6. Gates and cost `[REVISED: NPD-1, NPD-6]`

| phase | what | CPU h | per iteration | paid $ | GPU h |
|---|---|---|---|---|---|
| NP-0 (done) | uncapped generator 4 m 4 s + locate 4 s + scripts, measured | 0.07 | — | 0 | 0 |
| NP-1 | pool rebuild 4 m (measured); locate on pool + H ≈ 30 s `[INFERENCE: 0.2 s/task]`; nested LOCO < 1 min `[INFERENCE]` | ≈ 0.1 | < 1 min (reusing `pool.json`) | 0 | 0 |
| NP-2/3 | lane build + `cargo test -p axon-neural` | ≈ 2-3 `[INFERENCE]` | minutes, deps cached `[INFERENCE]` | 0 | 0 |
| NP-4 | parity + one H run + 3 repeats | < 0.5 `[INFERENCE]` | — | 0 | 0 |
| strict gate | **only with NP-2/3**; none if the slice stops at NP-0/NP-1 | 0.73 (2663 s) | — | 0 | 0 |
| g01 | M4850-M4868 (19 rows) | ≈ 1-2 `[INFERENCE]` | 3-7 min/row | 0 | 0 |
| receipt regress | none: NP reads `axon_cortex::rank` as a library and edits nothing in `REPAIR_SOURCES` (`scorecard.py:89-91`) | 0 | — | 0 | 0 |
| NP-5 | probe ticket ≤ 10 min, then training; inference on CPU | `[INFERENCE]` | — | 0 | probe ≤ 0.17, then measured |

**Stop at NP-0/NP-1:** no strict gate. Verification is `python3 -B selftest.py` plus the identity check, and the commit rides the
next batch gate. The lane adds only stdlib Python outside every Cargo member, and `GLOBAL_INPUTS` = `Cargo.toml`, `Cargo.lock`,
`rust-toolchain.toml` (`gate_lane.py:846`) `[INFERENCE: lane selection runs no Rust stage for these paths]`. **With NP-2/3:** S14
clippy list (`gate.sh:1031-1036`), S20 manifest command (`:1165-1177`), and S15 coverage (`:1047`); the S23 selftest line is
added by the integrator only then `[REVISED: NPD-5]`.

## 7. Sequencing and lanes `[REVISED: NPD-3, NPD-5, NPD-8]`

| order | lane | items | files owned |
|---|---|---|---|
| 0 | human | H6 answer (consumer + f) | — |
| 1 | b4/np-data | oracle (done), then C1, C2, PREREG (NP-1) | `crates/axon-neural/benchmarks/locrank-np/**` only; no `gate.sh`/Cargo edits |
| 2 (STOP-1 → C2 and STOP-1v(ii)) | b4/np-rs | NP-2, NP-3 | `crates/axon-neural/**`; `v022_{g01_mutations,attack_markers}.py` tails |
| 3 | integrator | workspace member + `Cargo.lock` (no new external crates; deps `serde_json`, `sha2`, `axon-canonical`, `axon-cortex`; **no `axon-reflex`** `[REVISED: NPD-7]`); S14/S20/S23 lines after CiHygiene and NE-1 land in `gate.sh`; strict gate; g01; CHANGELOG; O-entries | integrator-owned |
| 4 | b4/np-rs | NP-4, after Lane B merges | as above |
| 5 | — | NP-5 only via H3 | amendment |

## 8. Engineering decisions (canon after Step 2)

1. The first function is loc-rank (DiagnosticSummary has zero headroom).
2. The store is a directory; identity is the manifest digest; the ZIP reader is logged.
3. Canonical form is `axon-canonical` cl22 plus the CX-36 prefix.
4. Validation reuses `rank::validate_ranking`.
5. Labels are mechanical only; model orders are comparators.
6. No fallback inside the crate.
7. H_r is fixed to the 68 9b5 ids, with per-run denominators `[REVISED: NPA-2]`.
8. The identity check is a set equality against `ranker.order` `[REVISED: NPA-1]`.
9. Features and C1 variants are frozen in this text; N0's looks at H are disclosed `[REVISED: NPA-3]`.
10. Selection is nested LOCO + sign test p ≤ 0.10 AND net ≥ 2 `[REVISED: NPA-5]`.
11. STOP-1 is ordered and integer (net ≥ 3) `[REVISED: NPA-6]`.
12. Stops are evaluated value → oracle → C1 → C2 → PREREG `[REVISED: NPD-3]`.
13. Readiness is explicit, with M4867/M4868 `[REVISED: NPA-7]`.
14. No strict gate for a stop at NP-0/NP-1 `[REVISED: NPD-6]`.
15. `axon-reflex` is not a dependency `[REVISED: NPD-7]`.
16. NP-4 runs after Lane B and pins cortex `[REVISED: NPD-8]`.
17. `other` is report-only and S1 uses the mutation stratum `[REVISED: NPA-4]`.

## 9. Needs-human (Step 2: product questions only)

- **H6 (gating now) keyless consumer.** `[REVISED: NPD-2]` Name a consumer of keyless `cortex repair` ranking (an offline user, a
  CI job without a key, a non-`ai` deployment) and its repairs per week, or accept the stop. **Recommend: accept the stop.** The
  repo shows none (§2), the paid default is ≤ $0.04 per run, and the artifact contract can be revisited when a second neural
  candidate with data appears.
- **H1 base model + license (NP-5 only).** Recommend an Apache-2.0 code model ≤ 1.5 B parameters, CPU-servable `[INFERENCE: license
  verified at selection]`. No research-only licenses.
- **H2 training data.** (a) Paid-model outputs as distillation targets: recommend **no**. (b) `headroom/programs*` and the
  hard-once/hard-dev sources: recommend **no** (protected).
- **H3 GPU (NP-5)** `[REVISED: NPD-4]`. Needs the ai-cere owner's approval to run axon NP-5 through ai-cere
  `scripts/live/gpu_queue.py`, class default, on the GPU0 `gpu0-dev.lock` only while `wt/gpu1.production` exists (observed).
  - First, a ≤ 10-min probe ticket measures step time and peak memory, with GPU0 free ≥ 10,240 MiB at peak.
  - Then one training ticket, timeout = 1.25-1.5× the projected time, inside the command.
  - At most 3 live attempts (A6); a refusal (exit 2/3) is not an attempt.
  - One row per ticket.
  - No CPU-heavy work while holding the ticket: data prep and evaluation happen outside it, off the gate CPUs `6-15,22-23`.
  - Never GPU1, `prod-cere-t4`, `sglang-t4` or their ports.
  Recommend: ask only if STOP-3 is met.
- **H4 syntax later.** Recommend not before ≥ 2 admitted artifacts.
- **H5 budget.** Recommend $0 paid for NP-0..NP-4, and a $1 cap for any fresh Haiku comparison.

## 9a. Fail-closed summary

| claim | enforced by |
|---|---|
| pinned identity | M4850-M4856 |
| no silent fallback | M4858 |
| output is data | M4857 |
| offline / ready | M4867, M4868, `cargo tree` (no HTTP client) |

## 10. Verification log

Observed 2026-11-04 (draft binaries `axon 0.1.0 (9300d9fd)`):
- `n0_rank.py`, `n0_ties.py`, `power.py`, `axon check --json`, the pools, and pool `locate`;
- Step 1 recomputations:
  - `order`-vs-`ranked_top` mismatch counts 20/22/22 and non-ranker rows 11/12/12;
  - set sizes 68/69/69, intersection 67, union 70 (argswap-02/06);
  - Haiku on H_r 68/68, 67/67, 66/67, and spectrum 29 on each;
  - H_r 52/16 strata;
  - `H_r.txt` and `receipt-corpus.json` sha256;
  - `oracle.py` (9 → 16, C1a 7, 8 files);
  - the sign-test minimum net (4) and the Clopper-Pearson bounds (51/52 → 0.912, 50/52 → 0.884);
- the consumer grep (workflows, `auth.rs:423`, `run.sh:48`, `CORTEX.md:43-49`) and `gate_lane.py:846`.

Not observed: `[INFERENCE]` items, all post-build behaviour, the NP-1 CV numbers, and model licenses.
