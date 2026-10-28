# hx2 pre-registration

Status: **§0 only (HX-0 pilot gate), committed before any pilot row.** §1/§2
(digest, seeds, binaries, per-cell-program) follow at N3, only for classes
that pass §0; §6 takes the pilot rows and the verdict. Design:
`tasks/hx2-spec.md` HX-0, HX-1, HX-3, HX-4 (including every [REVISED]
marker) and the human decisions of 2026-10-27 (NH-1 b, NH-2 a, NH-3 b,
NH-4 a, NH-5 a). `PREREG.md` keeps the FU-1 record; its §6 points here.

## 0. HX-0 pilot gate (node N0b)

### 0.1 What is piloted

| field | value |
|---|---|
| prototype | branch `hx2/proto` (never merged), commit `e83e89d50a2ed8ae051e25bb9eb3baa9630e634a`: HX-1 caller hidden direct pin (`caller_pin`, `--check` recomputes it), HX-3a `erase`, HX-3b `sparse` in `build.py`/`mutators.py`; `selftest.py` green (4 new tests, red before) |
| scratch pool | `/var/tmp/r51-tmp/hx2-scratch/hx-pool.json` (not committed), sha256 `d95e28243f74f8d18f42d831bbb192573e656c4a13592576392a192e5b6791e7`; 868 tasks, 57 programs (6 dropped), `split_ok` true, build wall 429.1 s; `meta.examples_commit` = the prototype commit; `build.py --check` on it: 868 tasks, 0 failures |
| scratch build command | in `hx2/proto` `B/headroom`: `nice -n 10 python3 -B build.py --axon /var/tmp/r51-tmp/hx-bin/axon --cortex /var/tmp/r51-tmp/hx-bin/cortex --out /var/tmp/r51-tmp/hx2-scratch/hx-pool.json --seed 1 --split-seed 1 --stratum all --prior-split /var/tmp/r51-tmp/hx-build-1/hx-pool.json --allow-short-split --jobs 16 --report /var/tmp/r51-tmp/hx2-scratch/report.json` |
| builder binaries | the `PREREG.md` §1 pins: axon `819e04609b49c2b329edb2f79bd5a7653d5ee59ed8ddf5809a29c3247924ddda`, cortex `e052ea815762ad13a6417e94efa5d3f5c08854c483c320da1fddeab6087a433a` (`/var/tmp/r51-tmp/hx-bin/`) |
| builder-input digest (prototype) | `84c8312ebf5f3c9aae2d79da506485cf7a466f3f7fb50ff1ea8a93df9b22a978` |
| split | 56 programs keep their recorded `hx-build-1` side (none changed); 1 program new to the split, dealt once with split seed 1: `crates/axon-cortex/benchmarks/hardset/src/should_fail_test.ax` → **test** (stratum D; it had < 3 valid tasks in `hx-build-1`) |
| incumbent (arm B) | f26e9506 cortex, sha256 `957e5343529712b7745370a088043f9f3f9a550986c7f0472f640cce297a8a4f` = `meta.cortex_sha256_16` of every committed calibration row. Found (not rebuilt) at `/home/cklaus/projects/axon-lanes/.doc-build/rep-int/debug/cortex`; byte copy used: `/var/tmp/r51-tmp/hx2-inc-bin/cortex` |
| arms' axon (`AXON_BIN`) | the same rep-int f26e9506 build, sha256 `05b60c7ed89201461da314bf3d609edfbdd5d01246b73a064221d5374c864e72` = `meta.axon_sha256_16` of every committed calibration row; byte copy `/var/tmp/r51-tmp/hx2-inc-bin/axon` |
| grading axon | `/var/tmp/r51-tmp/hx-bin/axon` (`819e0460…`, = the calibration rows' `grade_axon_sha256_16`) |
| model, ranker, k | `claude-haiku-5-5` over micode (driver `drivers/micode.sh` sha256 `0a56dfa6da6428b0…`, as in the calibration rows), ranker `openrouter:anthropic/claude-haiku-5.5`, k = 1, `--parallel 1` |

Dev tasks of the scratch pool on the **18 programs recorded `dev` in
`/var/tmp/r51-tmp/hx-build-1/hx-pool.json` `meta.headroom.split`**: 261
(caller 42, erase 45, sparse 31, multi 71, subtle 53, caps 12, refine 7).

### 0.2 Eligible-cell rule (checked before the pilots)

A class with no knob holding ≥ 12 dev tasks from ≥ 3 dev programs does not
advance (`calibrate.py:51`).

| class | dev tasks / programs | knob: tasks / programs | eligible knobs |
|---|---|---|---|
| caller | 42 / 14 | 1: 28 / 14; 2: 14 / 7 | 1, 2 |
| erase | 45 / 13 (stratum C only) | <8: 22 / 13; 8-15: 19 / 11; >=16: 4 / 3 | <8, 8-15 |
| sparse | 31 / 16 | 1: 31 / 16 | 1 |

Every class has an eligible knob; each has ≥ 20 dev tasks, so each pilot is
20 ids.

### 0.3 Decision per class

n = pilot size; c = rows with `verified_clean` true; a false success is a row
with `false_success` true.

| Class | Advances iff |
|---|---|
| erase, sparse | 0 false successes **and** a three-way screen on the first 20: 4 ≤ c ≤ 16 (ceil(0.2n)..floor(0.8n)) → advance; c ≤ 3 or c ≥ 19 → reject; c ∈ {17, 18} → escalate: run the escalation ids (0.4), advance iff ceil(0.2·N) ≤ c_N ≤ floor(0.8·N) over all N rows: **erase N = 40 → 8..32; sparse N = 31 (only 11 dev sparse ids remain) → 7..24** (the proportional band) |
| caller (HX-1) | 0 false successes. Informational only: caller stays in the pool either way, and alone it never continues the DAG. No escalation. |

- A false success in any pilot: the recipe admits a workaround. The class is
  fixed and re-piloted under a new pilot seed recorded here first; it never
  advances on that pilot. At most one re-pilot per class, inside that class's
  $0.25 grant; a second false success parks the class in the HX-3 excluded
  table.
- A pilot stopped by the cap or by `RATE_LIMIT_STOP` (`run_ab.py:875`) before
  all its ids ran is **not decided**; after review its remaining ids run once
  under the same seed. Spend beyond a class's $0.25 needs NH-1 approval.
- **If neither erase nor sparse advances: stop.** The pilot rows and the
  verdict go to §6; HX-1 may still land as a recipe fix; no HX-5 run; R4 is
  decided under NH-3 (b): "retired without measurement".
- Triage of every not-clean row, by hand: `attempted`, `true_symbol_changed`,
  `regressed`, and whether the accepted patch edits a callee of g.
- Reported per pilot, not decided on (E8): clean count per knob.
- Overrun rule: a pilot above 2× its expected cost (≈ $0.10) or wall (≤ 14 min
  per 20) stops for review before the next paid run.

### 0.4 Ids (pilot seed 2)

Draw, per class, over the sorted ids of the class's scratch-pool tasks on the
18 recorded dev programs: `rng = random.Random(2)`; `ids =
rng.sample(sorted(ids), 20)`, redrawn from the same rng until the sample
covers ≥ 5 programs; the escalation ids are `rng.sample(sorted(rest), min(20,
len(rest)))` from the same rng right after, `rest` = the class's ids not in
the first sample. Number of draws: **caller 1, erase 1, sparse 1.**

| class | pilot ids (20) | programs | knobs |
|---|---|---|---|
| caller | `hx-caller-08c263bf73,hx-caller-0b1eccaeb3,hx-caller-fa50b985b9,hx-caller-8aee354e9a,hx-caller-2e2e508bd2,hx-caller-685ba329aa,hx-caller-63f77648aa,hx-caller-505966a09b,hx-caller-074f5e3f36,hx-caller-dc6f9898ab,hx-caller-94d040b59d,hx-caller-6c64f353a9,hx-caller-39187ac9ed,hx-caller-91286cfbe0,hx-caller-df3ebae89e,hx-caller-b2ce15951d,hx-caller-2e4a179c36,hx-caller-6657639c05,hx-caller-50d6356040,hx-caller-93ef592681` | 12 | 1: 11, 2: 9 |
| erase | `hx-erase-13e50ebe06,hx-erase-35f5e88844,hx-erase-f81e07ed22,hx-erase-825ccc4c03,hx-erase-4d2857c1e0,hx-erase-77cdb227e0,hx-erase-5d85759117,hx-erase-5a236ede94,hx-erase-103d808ab4,hx-erase-e9c561af4f,hx-erase-8dccf5e6a2,hx-erase-89e52037ba,hx-erase-a34b1373a2,hx-erase-f317e19dac,hx-erase-5e7be329fa,hx-erase-925bf442a2,hx-erase-5bf340cb1b,hx-erase-d5c35796e6,hx-erase-4272d49992,hx-erase-09ce8df7d2` | 13 | <8: 12, 8-15: 7, >=16: 1 |
| sparse | `hx-sparse-fbb4078717,hx-sparse-d6ea9c87f0,hx-sparse-e3c271bb97,hx-sparse-0ef370b947,hx-sparse-16edfe9fcf,hx-sparse-cc035ca5b6,hx-sparse-76db7c58d5,hx-sparse-3a29c663fc,hx-sparse-c4c95c8d13,hx-sparse-70cb59a1fc,hx-sparse-520698262e,hx-sparse-bff9d8d22b,hx-sparse-43f9cbc0c1,hx-sparse-dfb3f5eb1d,hx-sparse-c5a9b65f49,hx-sparse-7ae949197d,hx-sparse-720d596dd4,hx-sparse-bab3242baa,hx-sparse-7ac945e835,hx-sparse-c6ec54bdce` | 13 | 1: 20 |

| class | escalation ids (only on c ∈ {17, 18}) |
|---|---|
| erase (20) | `hx-erase-05492dce90,hx-erase-82456d7b8e,hx-erase-8e13fb7c80,hx-erase-7df3ad2569,hx-erase-83c7e679e3,hx-erase-8c1de3f7b6,hx-erase-9923bc3a38,hx-erase-5162b0c411,hx-erase-a5df6e808e,hx-erase-5cc2e96d6a,hx-erase-38fd583816,hx-erase-fe9b22c033,hx-erase-38cdc76064,hx-erase-a701894e85,hx-erase-cd8ea1d999,hx-erase-cec131cd75,hx-erase-68c8535eb5,hx-erase-f6d4bf9ff0,hx-erase-441a3713e6,hx-erase-97040aa3cc` |
| sparse (11) | `hx-sparse-c2abb2264b,hx-sparse-8289a3dbb0,hx-sparse-cb488666c5,hx-sparse-b31cbae965,hx-sparse-7ddf5fb723,hx-sparse-349499b37f,hx-sparse-024c0b1d4b,hx-sparse-9c9d040135,hx-sparse-3379dcc61b,hx-sparse-c59bc8757f,hx-sparse-48ed9fe09e` |

### 0.5 Pilot commands

One empty spend root per class, cap $0.25, reserve $0.03, run from the
`hx2/rec` worktree root (`/home/cklaus/projects/axon-lanes/hx2-rec`). `<IDS>`
is the class's pilot id list from 0.4; an escalation run is the same command
with `--only` = the escalation ids (same root, same cap: the cap sums every
row under the root).

```
cd /home/cklaus/projects/axon-lanes/hx2-rec && \
AXON_BIN=/var/tmp/r51-tmp/hx2-inc-bin/axon CORTEX_BIN=/var/tmp/r51-tmp/hx2-inc-bin/cortex \
nice -n 10 python3 crates/axon-cortex/benchmarks/ab/run_ab.py \
  /var/tmp/r51-tmp/hx2-scratch/hx-pool.json --arm B --generator real --transport micode \
  --model claude-haiku-5-5 --trials 1 --parallel 1 --only <IDS> \
  --grade-axon /var/tmp/r51-tmp/hx-bin/axon --ranker openrouter:anthropic/claude-haiku-5.5 \
  --spend-root crates/axon-cortex/benchmarks/headroom/hx2-pilot-2026-10-28/<class>/ \
  --out crates/axon-cortex/benchmarks/headroom/hx2-pilot-2026-10-28/<class>/pilot.jsonl \
  --reserve-usd 0.03 --cap-usd 0.25
```

with (`<class>`, `<IDS>`) = (`caller`, caller ids), (`erase`, erase ids),
(`sparse`, sparse ids). Before each run: `sha256sum` of the pool, both
`hx2-inc-bin` binaries and the grading axon equal the values in 0.1, and the
class's spend root holds no `*.jsonl`.
