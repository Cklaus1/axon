# hx3 pre-registration

Status: **§0 was committed before any pilot row (730b82c4); §6 records the
HX3-0 pilot rows and the verdict: masked rejects, excerpt is held on NH-1.**
[REVISED: HX3-0 re-pilot 2026-10-29: NH-1 granted; §0.6 (568d4c22) preceded
the excerpt seed-4 re-pilot, which rejects (c = 19, §6.3). Neither A nor B
advances.]
[REVISED: HX3-0 class C 2026-10-29: §0.7 (0824f16a) and §0.8 (3c8ba568)
preceded the class C pilot, which **advances** (c = 15, 0 false successes,
no overrun, §6.4). HX3-1 needs a human go-ahead (§0.3).]
§1/§2 are never written unless a class advances.
Design: `tasks/hx3-spec.md` on `hx3/spec` (10ed553f) HX3-0, HX3-A, HX3-B,
HX3-2, including every [REVISED] marker. Authorization (2026-10-29): HX3-0
only, pilots ≤ $0.25 per class, $0.50 in all; NH-1 not granted, so no resume,
re-pilot, calibration, class C or HX3-1 without a further human decision.
`PREREG-hx2.md` keeps the hx2 record.

## 0. HX3-0 pilot gate (node N0b)

### 0.1 What is piloted

| field | value |
|---|---|
| prototype | branch `hx3/proto` (never merged) commit `0621536a`, cut from main 43e895d0 + e83e89d5 (cherry-picked as 8b125e58): HX3-A `masked`, `bundle.py`, HX3-B `excerpt` on bundle programs (stratum `X`), `build.py --bundles`; `selftest.py` green (4 new tests — `test_masked_split`, `test_masked_rebuild`, `test_excerpt_rules`, `test_bundle` — red before: 3 AttributeError, 1 ModuleNotFoundError); `ab/selftest_ab.py` 12/12 |
| bundles | `/var/tmp/r51-tmp/hx3-bundles/manifest.json` (not committed) sha256 `a77bff409aa42a013012d8c895f0ce97224e4f9f8fa2cde79c1af8aac847617f`; 18 dev / 39 test bundles, all reach min-chars 26000 (dev 26158-31131 chars, 5-8 fillers; test 26301-34912, 3-8 fillers); bundle seed 1; `bundle.py --check`: 0 differences |
| scratch pool | `/var/tmp/r51-tmp/hx3-scratch/hx-pool.json` (not committed, 4.7 MB) sha256 `53fb3542b4347b23626c78fb1600883c1fb81b20f8eb12dd5e103aca7ef31f59`; 1187 tasks (masked 167, excerpt 152, multi 224, subtle 193, erase 148, caller 139, sparse 102, caps 48, refine 14), 114 programs (57 + their 57 bundles; 6 dropped, 1 rejected: `validate.ax`), `split_ok` true; build wall 455.5 s at load avg 12-20; `build.py --check`: 1187 tasks, 0 failures, rc 0, wall 391.4 s at load avg 13-19 |
| scratch build commands | in `hx3/proto` `B/headroom`, verbatim: `nice -n 10 python3 -B bundle.py --pool /var/tmp/r51-tmp/hx2-scratch/hx-pool.json --seed 1 --min-chars 26000 --out /var/tmp/r51-tmp/hx3-bundles/`; `nice -n 10 python3 -B build.py --axon /var/tmp/r51-tmp/hx-bin/axon --cortex /var/tmp/r51-tmp/hx-bin/cortex --out /var/tmp/r51-tmp/hx3-scratch/hx-pool.json --seed 1 --split-seed 1 --stratum all --bundles /var/tmp/r51-tmp/hx3-bundles/manifest.json --prior-split /var/tmp/r51-tmp/hx2-scratch/hx-pool.json --allow-short-split --jobs 16 --report /var/tmp/r51-tmp/hx3-scratch/report.json`; `nice -n 10 python3 -B build.py --check /var/tmp/r51-tmp/hx3-scratch/hx-pool.json --axon /var/tmp/r51-tmp/hx-bin/axon --cortex /var/tmp/r51-tmp/hx-bin/cortex --jobs 16` |
| builder binaries | the `PREREG.md` §1 pins: axon `819e04609b49c2b329edb2f79bd5a7653d5ee59ed8ddf5809a29c3247924ddda`, cortex `e052ea815762ad13a6417e94efa5d3f5c08854c483c320da1fddeab6087a433a` (`/var/tmp/r51-tmp/hx-bin/`) |
| builder-input digest (prototype) | `5b5c8fbd1cfcbc68fa48a48631997db66178e9cdcd35928661aeed8f7b3d56ff` (`meta.headroom.inputs_digest`) |
| split | chained from `/var/tmp/r51-tmp/hx2-scratch/hx-pool.json` (sha256 `d95e2824…91e7`): every real program keeps its recorded side; programs new to the split: none but the 57 bundles, each forced to its target's side |
| bundle split | every bundle task's `split` = its target program's recorded split: checked, 0 mismatches (HX3A-2) |
| incumbent (arm B) | main 43e895d0 (receipt-PASS head for repair digest cb7f132e), built in the detached worktree `/var/tmp/r51-tmp/hx3-inc-src` with `CARGO_TARGET_DIR=/home/cklaus/projects/axon-lanes/.doc-build/hx3-inc` (no other checkout uses it): `cargo build --locked -p axon-core --bin axon` and `cargo build --locked -p axon-cortex --features ai --bin cortex`, debug profile, cold (188 crates compiled), cargo-reported 35.79 s + 8.78 s, wall ≈ 45 s at load avg ≈ 57; byte copies in `/var/tmp/r51-tmp/hx3-inc-bin/`: cortex sha256 `1b2f8edb101d3fbae1cb3b58f21e6fb6ad426fc5cda63add7b884b23cfd924c9` |
| arms' axon (`AXON_BIN`) | the same build, `/var/tmp/r51-tmp/hx3-inc-bin/axon`, sha256 `f15fad70807e5f51d94104237df208fcd6944071228478700f46189f136957f4` |
| grading axon | `/var/tmp/r51-tmp/hx-bin/axon` (`819e0460…ddda`) |
| model, ranker, k | `claude-haiku-5-5` over micode (driver `drivers/micode.sh` sha256 `0a56dfa6da6428b0b24d5a9b893d0aef5aea6d042fd3e345db193bf871a04144`; `/usr/local/bin/micode` sha256 `9a5e3f5effa1112377913e9eb0ecf581570215e519787b102a301958aa071bc3`), ranker `openrouter:anthropic/claude-haiku-5.5`, k = 1, `--parallel 1` |
| rec worktree | [REVISED: N0 — the spec put §0 on a new `hx3/rec` branch; the authorization of 2026-10-29 puts it on `hx3/proto`] `/home/cklaus/projects/axon-lanes/hx3-proto`, branch `hx3/proto`; the pilots run at the commit that adds this file |

Dev tasks of the scratch pool on the **18 programs recorded `dev` in
`/var/tmp/r51-tmp/hx-build-1/hx-pool.json` `meta.headroom.split`** (excerpt:
by `target_program`): 359 (masked 54, excerpt 44, multi 71, subtle 53, erase
45, caller 42, sparse 31, caps 12, refine 7).

Offline pipeline check (no model, $0): `run_ab.py --arm B --generator oracle
--ranker spectrum` with `CORTEX_AUDIT=off` and the §0.1 incumbent/grading
binaries on `hx-masked-5b808adcf8` and `hx-excerpt-78e084c8a2`, out to
`/var/tmp/r51-tmp/hx3-dry/` (outside every spend root): both
`verified_clean` true, 0 false successes, `generator_context` used `view`.
This is not a pilot row and does not count toward any spend root.

### 0.2 Eligible-cell rule (checked before the pilots)

A class with no knob holding ≥ 12 dev tasks from ≥ 3 dev programs does not
advance (`calibrate.py:51`).

| class | dev tasks / programs | knob: tasks / programs | eligible knobs |
|---|---|---|---|
| masked | 54 / 16 | s: 24 / 13; o: 30 / 16 | s, o |
| excerpt | 44 / 13 (target programs) | xfn: 18 / 9; erase: 26 / 13 | xfn, erase |

Both classes have ≥ 20 eligible dev tasks, so each pilot runs 20 rows.

### 0.3 Decision per class

n = the rows run (20), c = the rows with `verified_clean` true.

- **Advance** iff 0 false successes **and** 4 ≤ c ≤ 16 of 20, and the class
  has an eligible knob (§0.2: both do). No escalation: c ∈ {17, 18} rejects.
  P(advance) = P(4 ≤ Bin(20, p) ≤ 16): 0.133 / 0.352 / 0.589 / 0.893 / 0.997
  at p = 0.1 / 0.15 / 0.2 / 0.3 / 0.5, symmetric above 0.5.
- **Decided early** (a pilot stopped by the cap or `RATE_LIMIT_STOP`): any
  false success → a false success; ≥ 4 clean and ≥ 4 not clean with 0 false
  successes → advance; > 16 clean or > 16 not clean → reject. Otherwise
  **not decided**: after review its remaining ids run once under the same
  seed (spend-root rule ii), which needs NH-1 (not granted).
- **A false success**: the recipe admits a workaround; the class is fixed and
  re-piloted once under pilot seed 4 (spend-root rule iii, needs NH-1). A
  second false success parks the class. Masked's pre-registered fix: if the
  final of a masked false success fails only cut tests, the re-pilot restricts
  masked to `cut_tests` = [] tasks under one knob `c0`.
- **Neither advances**: stop; rows and verdict in §6; go to NH-3 (class C).
  **Either advances**: HX3-1 (needs a human go-ahead).
- **Spend-root rules**: (i) the first run of a class uses `<class>/pilot.jsonl`
  in a spend root holding no `*.jsonl`; (ii) a resume reuses that `--out` and
  root with `--cap-usd 0.25 + <NH-1 grant>`; (iii) a re-pilot uses
  `<class>/pilot-s4.jsonl` in the same root with the same cap rule.
- **Stop rule**: `--reserve-usd 0.05 --cap-usd 0.25`, so no row starts once
  the class's spend root holds > $0.20; at most $0.05 overshoots $0.25.
- **Triage** every not-clean row by hand: `attempted`, `true_symbol_changed`,
  `regressed`, whether the accepted patch edits a callee of g; masked: did an
  attempt pass every visible check and fail the hidden one; excerpt: did the
  excerpt view make attempts fail to compile (unknown name, field, arity);
  every class: is the missing fact inferable from what the generator saw
  (recorded, not decided on). Per-knob clean counts are reported.
- **Overrun** per class iff spend > 2 · (c · $0.0010 + (20 − c) · $0.0093) or
  wall > 2 · (c · 6.6 s + (20 − c) · 76 s).

### 0.4 Ids (pilot seed 3)

Over each class's eligible dev tasks: `rng = random.Random(3)`; `ids =
rng.sample(sorted(ids), min(20, n))`, redrawn from the same rng until the draw
covers ≥ min(5, #programs) programs (target programs for excerpt), at most 100
draws, else the first draw with the most programs. Script:
`/var/tmp/r51-tmp/specs-Hx3/draw_ids.py` (not committed; run on the pool
above). Draws: masked 1, excerpt 1.

| class | pilot ids (20) | programs | knobs |
|---|---|---|---|
| masked | hx-masked-5b808adcf8, hx-masked-ac10cf42e5, hx-masked-9da51ca0e1, hx-masked-388b4d171b, hx-masked-75d1d30224, hx-masked-b0e55d971a, hx-masked-95a9d6b0bc, hx-masked-c329412f8e, hx-masked-f9e51fa027, hx-masked-2447069fc0, hx-masked-e6c12d9365, hx-masked-0d66b91ccf, hx-masked-e25dcacb71, hx-masked-5dd94884fc, hx-masked-a115f9ba7e, hx-masked-58c4fc015c, hx-masked-4e2e83cf32, hx-masked-c54757bade, hx-masked-f7e5b9d6b9, hx-masked-a7f4a2d0ea | 11: sum_types, bowling, dates, gridpath, interest, intervals, invoice, rational, recurrence, shipping, wordwrap | o 11, s 9 |
| excerpt | hx-excerpt-78e084c8a2, hx-excerpt-d1359f76b0, hx-excerpt-b3c44c4c97, hx-excerpt-249dfe171f, hx-excerpt-8faa6965bb, hx-excerpt-ded05fff3b, hx-excerpt-a8bccc2087, hx-excerpt-1376dde808, hx-excerpt-0895e2e2a9, hx-excerpt-f76fb9a006, hx-excerpt-79165c0750, hx-excerpt-696e7875ae, hx-excerpt-5c0924be9b, hx-excerpt-8b2863c2d2, hx-excerpt-fa279973a1, hx-excerpt-7940a4afdb, hx-excerpt-9c3f93393f, hx-excerpt-a1cfeffe5d, hx-excerpt-a5e992a567, hx-excerpt-af94b4f851 | 11 targets: bowling, bsearch, dates, graph, interest, intervals, invoice, rational, shipping, units, wordwrap | erase 11, xfn 9 |

Masked pilot ids by `len(cut_tests)`: 0: 4, 1: 4, 2: 4, 3: 3, 5: 1, 6: 1,
7: 2, 8: 1. Every excerpt pilot task's model file (broken minus the hidden
block) is ≥ 25,605 characters (> the 24,000 view cap).

### 0.5 Pilot commands

Before each run: `sha256sum` of the pool, both `/var/tmp/r51-tmp/hx3-inc-bin/`
binaries and `/var/tmp/r51-tmp/hx-bin/axon` must equal §0.1, and the class's
spend root must hold no `*.jsonl` (rule i).

```
cd /home/cklaus/projects/axon-lanes/hx3-proto && \
AXON_BIN=/var/tmp/r51-tmp/hx3-inc-bin/axon CORTEX_BIN=/var/tmp/r51-tmp/hx3-inc-bin/cortex \
nice -n 10 python3 crates/axon-cortex/benchmarks/ab/run_ab.py \
  /var/tmp/r51-tmp/hx3-scratch/hx-pool.json --arm B --generator real --transport micode \
  --model claude-haiku-5-5 --trials 1 --parallel 1 --only hx-masked-5b808adcf8,hx-masked-ac10cf42e5,hx-masked-9da51ca0e1,hx-masked-388b4d171b,hx-masked-75d1d30224,hx-masked-b0e55d971a,hx-masked-95a9d6b0bc,hx-masked-c329412f8e,hx-masked-f9e51fa027,hx-masked-2447069fc0,hx-masked-e6c12d9365,hx-masked-0d66b91ccf,hx-masked-e25dcacb71,hx-masked-5dd94884fc,hx-masked-a115f9ba7e,hx-masked-58c4fc015c,hx-masked-4e2e83cf32,hx-masked-c54757bade,hx-masked-f7e5b9d6b9,hx-masked-a7f4a2d0ea \
  --grade-axon /var/tmp/r51-tmp/hx-bin/axon --ranker openrouter:anthropic/claude-haiku-5.5 \
  --spend-root crates/axon-cortex/benchmarks/headroom/hx3-pilot-2026-10-29/masked/ \
  --out crates/axon-cortex/benchmarks/headroom/hx3-pilot-2026-10-29/masked/pilot.jsonl \
  --reserve-usd 0.05 --cap-usd 0.25
```

```
cd /home/cklaus/projects/axon-lanes/hx3-proto && \
AXON_BIN=/var/tmp/r51-tmp/hx3-inc-bin/axon CORTEX_BIN=/var/tmp/r51-tmp/hx3-inc-bin/cortex \
nice -n 10 python3 crates/axon-cortex/benchmarks/ab/run_ab.py \
  /var/tmp/r51-tmp/hx3-scratch/hx-pool.json --arm B --generator real --transport micode \
  --model claude-haiku-5-5 --trials 1 --parallel 1 --only hx-excerpt-78e084c8a2,hx-excerpt-d1359f76b0,hx-excerpt-b3c44c4c97,hx-excerpt-249dfe171f,hx-excerpt-8faa6965bb,hx-excerpt-ded05fff3b,hx-excerpt-a8bccc2087,hx-excerpt-1376dde808,hx-excerpt-0895e2e2a9,hx-excerpt-f76fb9a006,hx-excerpt-79165c0750,hx-excerpt-696e7875ae,hx-excerpt-5c0924be9b,hx-excerpt-8b2863c2d2,hx-excerpt-fa279973a1,hx-excerpt-7940a4afdb,hx-excerpt-9c3f93393f,hx-excerpt-a1cfeffe5d,hx-excerpt-a5e992a567,hx-excerpt-af94b4f851 \
  --grade-axon /var/tmp/r51-tmp/hx-bin/axon --ranker openrouter:anthropic/claude-haiku-5.5 \
  --spend-root crates/axon-cortex/benchmarks/headroom/hx3-pilot-2026-10-29/excerpt/ \
  --out crates/axon-cortex/benchmarks/headroom/hx3-pilot-2026-10-29/excerpt/pilot.jsonl \
  --reserve-usd 0.05 --cap-usd 0.25
```

### 0.6 Addendum: excerpt re-pilot under seed 4 (committed before its first row)

[DECIDED by human 2026-10-29: NH-1 granted for excerpt, up to $0.25 more, for
one seed-4 re-pilot with the corrected grading pin (§6.2). NH-3 (b): class C
is authored after this re-pilot and piloted at $0.25 with the same pin.]

- **Fix (the only change from §0):** the grading axon is the arms' axon,
  `/var/tmp/r51-tmp/hx3-inc-bin/axon` sha256
  `f15fad70807e5f51d94104237df208fcd6944071228478700f46189f136957f4`
  (43e895d0, post SF-5). Justification: it is `run_ab.py`'s own default
  (`--grade-axon` defaults to `AXON_BIN`, `run_ab.py:912`); the offline regrade
  of §6.1 reproduced every seed-3 row with it except the 7 skewed ones; and
  `probe_grade_axon` (`:821`, run before any task) still requires an
  authentic completion token from it. No other post-SF-5 axon is pinned
  anywhere. The fix was decided (§6.2, 1adbda44) before this draw; the draw
  is a function of seed 4 and the pool's sha256 alone.
- **Recipe unchanged:** same pool (`53fb3542…1f59`), bundles, view cap,
  knobs, arms' axon and cortex, model, ranker, k = 1, `--parallel 1`,
  `--reserve-usd 0.05`.
- **Preamble probe:** `/var/tmp/r51-tmp/hx3-s4/probe_mut.ax` (sha256
  `f5665552…0698`; a 10-line file whose only diagnostic is the I0002 `let
  mut` note) must get the same `axon check` rc from the grading axon and the
  arms' axon, else the run aborts before any model call.
- **Ids (seed 4):** the §0.4 procedure with `random.Random(4)`, excerpt only,
  over the same 44 eligible dev tasks (`/var/tmp/r51-tmp/hx3-s4/draw_ids_s4.py`,
  not committed; output sha256 `dcaa31b0…06fd`). Draws: 1. 20 ids over 10
  target programs (bowling, dates, graph, gridpath, interest, intervals,
  invoice, rational, recurrence, shipping), knobs erase 13 / xfn 7. 8 ids also
  have a seed-3 row; none is skipped (rule iii): hx-excerpt-78e084c8a2,
  hx-excerpt-7de5642522, hx-excerpt-2186d43edd, hx-excerpt-971021a7ce,
  hx-excerpt-a8bccc2087, hx-excerpt-2b852cf3fc, hx-excerpt-159f7014da,
  hx-excerpt-1376dde808, hx-excerpt-09e59ab8b6, hx-excerpt-e76a65d945,
  hx-excerpt-7a1cd94e7d, hx-excerpt-1118d4d30b, hx-excerpt-696e7875ae,
  hx-excerpt-79165c0750, hx-excerpt-7940a4afdb, hx-excerpt-4eace48161,
  hx-excerpt-249dfe171f, hx-excerpt-91543376b4, hx-excerpt-d1359f76b0,
  hx-excerpt-b0501e04eb.
- **Spend (rule iii):** `--out excerpt/pilot-s4.jsonl` in the same root,
  `--cap-usd 0.50` (= 0.25 + the 0.25 grant). `spent()` (`:808`) sums every
  `*.jsonl` under the root, so the seed-3 $0.0365 counts: no row starts once
  the root holds > $0.45, i.e. after > $0.4135 of new spend; class total
  ≤ $0.55.
- **Decision, unchanged (§0.3):** advance iff 0 false successes and
  4 ≤ c ≤ 16 of 20; c ∈ {17, 18} rejects; decided early as in §0.3; a second
  false success parks excerpt in §HX3-X. Overrun as in §0.3 at the observed c.
- **Command** (script `/var/tmp/r51-tmp/hx3-pilot-excerpt-s4.sh` checks the
  pins, the root and the probe, then runs):

```
cd /home/cklaus/projects/axon-lanes/hx3-proto && \
AXON_BIN=/var/tmp/r51-tmp/hx3-inc-bin/axon CORTEX_BIN=/var/tmp/r51-tmp/hx3-inc-bin/cortex \
nice -n 10 python3 crates/axon-cortex/benchmarks/ab/run_ab.py \
  /var/tmp/r51-tmp/hx3-scratch/hx-pool.json --arm B --generator real --transport micode \
  --model claude-haiku-5-5 --trials 1 --parallel 1 --only hx-excerpt-78e084c8a2,hx-excerpt-7de5642522,hx-excerpt-2186d43edd,hx-excerpt-971021a7ce,hx-excerpt-a8bccc2087,hx-excerpt-2b852cf3fc,hx-excerpt-159f7014da,hx-excerpt-1376dde808,hx-excerpt-09e59ab8b6,hx-excerpt-e76a65d945,hx-excerpt-7a1cd94e7d,hx-excerpt-1118d4d30b,hx-excerpt-696e7875ae,hx-excerpt-79165c0750,hx-excerpt-7940a4afdb,hx-excerpt-4eace48161,hx-excerpt-249dfe171f,hx-excerpt-91543376b4,hx-excerpt-d1359f76b0,hx-excerpt-b0501e04eb \
  --grade-axon /var/tmp/r51-tmp/hx3-inc-bin/axon --ranker openrouter:anthropic/claude-haiku-5.5 \
  --spend-root crates/axon-cortex/benchmarks/headroom/hx3-pilot-2026-10-29/excerpt/ \
  --out crates/axon-cortex/benchmarks/headroom/hx3-pilot-2026-10-29/excerpt/pilot-s4.jsonl \
  --reserve-usd 0.05 --cap-usd 0.50
```

### 0.7 Addendum: class C `erase-long` pilot (node N0c; committed before its first row)

[DECIDED by human 2026-10-29, NH-3 (b): C is authored after the excerpt
re-pilot and piloted at $0.25 with the corrected grading pin.] The trigger
holds (§6.2, §6.3: A and B rejected). **Not run yet:** the excerpt re-pilot
overran on wall (§6.3), so this pilot waits for that review (spec NH-3).

- **Programs (N0c):** 12 dev programs in `programs-c/` (463b7aaf, committed
  before any defect was derived): checksums, chessmoves, globmatch, lifegrid,
  mdlite, payroll, poker, rooms, stackvm, sudoku, taxcalc, vending. Three
  authoring agents each wrote 4 from short neutral problem statements, reading
  only `AXON_REFERENCE.md`, `docs/` and `examples/`; none was given or allowed
  to read benchmark sources, cortex prompts, pools or results (FU-1b). Each
  file has ≥ 2 functions of 24-60 lines with ≥ 3 direct tests each, 13-21
  tests, and zero `axon check` diagnostics; `check` and `test` exit 0 under
  both `hx-bin/axon` and `hx3-inc-bin/axon` (re-run by the integrator).
  sha256 over the sorted per-file sha256 lines: `a01bd485…e3e9`.
- **Builder:** `build.py` dd7d4b4f (class `erase-long`: an erase single on a
  span ≥ 24 lines, hidden = `caller_pin`, visible = the first other failing
  test, nothing cut, precedence over `erase`; `--deal-dev`), fail-first
  `test_erase_long` / `test_deal_dev` (3 red before, green after). Builder
  pins as §0.1: axon `819e0460…`, cortex `e052ea81…`.
- **Pool:** `/var/tmp/r51-tmp/hx3c-scratch/hx-pool.json` sha256
  `e5b49c68871900c5dc78f36aef2631f127c1f69219799138a0337350cf0ce830`, built
  with `--stratum C --programs-dir …/programs-c --prior-split
  /var/tmp/r51-tmp/hx3-scratch/hx-pool.json --deal-dev --allow-short-split
  --per-cell-program 2 --seed 1 --split-seed 1` (452.7 s at load ≈ 11-25);
  `meta.headroom.deal` = `dev`, inputs digest `57c7c2b8…0804`, 241 tasks;
  `build.py --check`: 0 failures. **Eligible cell (§0.2):** (erase-long,
  `>=24`) has 23 dev tasks over 12 programs (≥ 12 from ≥ 3): eligible, and
  ≥ 20, so the pilot has 20 rows.
- **Ids (seed 3, the §0.4 procedure):** `/var/tmp/r51-tmp/hx3c-scratch/draw_ids_c.py`
  (not committed; output sha256 `5cf60a95…747b`). Draws: 1. 20 ids over 12
  programs: hx-erase-long-79d95585d4, hx-erase-long-d2710c6e28,
  hx-erase-long-c305637ec8, hx-erase-long-4e404dfb2d, hx-erase-long-84c6ebc2cf,
  hx-erase-long-abe013b07e, hx-erase-long-3b5432b8d5, hx-erase-long-09d091fed3,
  hx-erase-long-9e7a22b0b4, hx-erase-long-9989154306, hx-erase-long-faa5cc3d57,
  hx-erase-long-e66a8b55f6, hx-erase-long-7ff6b007fb, hx-erase-long-495a95b844,
  hx-erase-long-80455d7868, hx-erase-long-88bfbdd468, hx-erase-long-f63ebca8ae,
  hx-erase-long-6e103917dc, hx-erase-long-8186423ec7, hx-erase-long-5a490dc013.
- **Zero-cost controls on these 20 ids** (no model call, rows outside the
  spend root in `/var/tmp/r51-tmp/hx3c-scratch/`): arm A oracle 20/20
  verified_clean; arm B `--generator none` 0/20, `needs_input` 20, localization
  rank 1 on 19 and 5 on 1.
- **Grading pin:** as §0.6, the arms' axon `f15fad70…57f4`. The `let mut`
  probe now runs inside `run_ab.py`'s `probe_grade_axon` (5bb4e0f4, O-HX3-01
  fix; fail-first `case_grade_axon_mut_skew`): a grading axon whose `check`
  rc on a one-`let mut` file differs from the arms' axon aborts the run before
  any model call (`hx-bin/axon` aborts, `hx3-inc-bin/axon` passes, checked).
- **Spend:** own empty root `hx3-pilot-2026-10-29/erase-long/`, `--cap-usd
  0.25`, `--reserve-usd 0.05`. Arms, model, ranker, k = 1, `--parallel 1` as
  §0.5.
- **Decision (§0.3, unchanged):** advance iff 0 false successes and 4 ≤ c ≤ 16
  of 20; c ≥ 17 rejects; decided early as in §0.3. Overrun iff spend >
  2·(c·$0.0010 + (20 − c)·$0.0093) or wall > 2·(c·6.6 s + (20 − c)·76 s) at
  the observed c.
- **Command:**

```
cd /home/cklaus/projects/axon-lanes/hx3-proto && \
AXON_BIN=/var/tmp/r51-tmp/hx3-inc-bin/axon CORTEX_BIN=/var/tmp/r51-tmp/hx3-inc-bin/cortex \
nice -n 10 python3 crates/axon-cortex/benchmarks/ab/run_ab.py \
  /var/tmp/r51-tmp/hx3c-scratch/hx-pool.json --arm B --generator real --transport micode \
  --model claude-haiku-5-5 --trials 1 --parallel 1 --only hx-erase-long-79d95585d4,hx-erase-long-d2710c6e28,hx-erase-long-c305637ec8,hx-erase-long-4e404dfb2d,hx-erase-long-84c6ebc2cf,hx-erase-long-abe013b07e,hx-erase-long-3b5432b8d5,hx-erase-long-09d091fed3,hx-erase-long-9e7a22b0b4,hx-erase-long-9989154306,hx-erase-long-faa5cc3d57,hx-erase-long-e66a8b55f6,hx-erase-long-7ff6b007fb,hx-erase-long-495a95b844,hx-erase-long-80455d7868,hx-erase-long-88bfbdd468,hx-erase-long-f63ebca8ae,hx-erase-long-6e103917dc,hx-erase-long-8186423ec7,hx-erase-long-5a490dc013 \
  --grade-axon /var/tmp/r51-tmp/hx3-inc-bin/axon --ranker openrouter:anthropic/claude-haiku-5.5 \
  --spend-root crates/axon-cortex/benchmarks/headroom/hx3-pilot-2026-10-29/erase-long/ \
  --out crates/axon-cortex/benchmarks/headroom/hx3-pilot-2026-10-29/erase-long/pilot.jsonl \
  --reserve-usd 0.05 --cap-usd 0.25
```

### 0.8 Addendum: wall limit measured under real load (committed before any class C row)

[DECIDED by human 2026-10-29: "approved, do it" - the wall limit is set from
wall times measured under real load plus a stated margin, as the NH-R1
`HARD_GRACE` rule (`tasks/unattended-auth-spec.md:285-287`).] It replaces
only the wall half of the §0.3 overrun rule; the spend half is unchanged.

- **Formula:** wall limit = n × p90 × 1.5, where n = the pilot's rows and p90
  = the nearest-rank 90th percentile (the ⌈0.9·N⌉-th smallest) of per-row
  `wall_s` over every recorded HX3-0 row, clean or not: masked (§0.5),
  excerpt seed 3 (§0.5) and excerpt seed 4 (§0.6), N = 60. Margin: 1.5.
- **Loads of those runs** (1-min load average at start / end, from the run
  logs): masked 13.33 / 9.77; excerpt seed 3 8.05 / 12.45; excerpt seed 4
  4.73 / 11.21. Row walls: masked sum 520.9 s, seed 3 394.9 s, seed 4 407.0 s.
- **Value:** p90 = 41.9 s (neighbours 35.4 s and 55.6 s); for C's 20-row
  pilot the wall limit is 20 × 41.9 × 1.5 = **1257.0 s**. Spend limit
  unchanged: spend > 2·(c·$0.0010 + (20 − c)·$0.0093) overruns.
- **Seed-4 overrun (§6.3): reviewed 2026-10-29: within the load-measured
  rule** (407.0 s ≤ 1257.0 s). Masked (520.9 s) and excerpt seed 3 (394.9 s)
  are within it too. The rule does not change any verdict: both classes were
  rejected on c.
- **Start condition:** the C pilot starts once the 1-min load average is
  ≤ 15 (above every load measured above), checked once a minute for up to
  60 min; if it never is, it starts anyway and the load is recorded. The
  start and end loads are recorded in §6.
- **If C's wall exceeds 1257.0 s:** recorded as an overrun and C stops; no
  re-pilot. The §0.3 decision on c and false successes is unchanged.

## 6. HX3-0 pilot rows and verdict (2026-10-29)

Rows: `hx3-pilot-2026-10-29/<class>/pilot.jsonl`, one class at a time, with
the §0.5 commands verbatim. The pins of 0.1 and the empty spend roots were
checked before each run (`/var/tmp/r51-tmp/hx3-pilot-<class>.sh`). Every row
carries commit `730b82c4`, cortex `1b2f8edb101d3fba`, axon `f15fad70807e5f51`,
grading axon `819e04609b49c2b3`, driver `0a56dfa6da6428b0`. No row was
rate-limited, no pilot hit the cap or the reserve, and none overran.

| class | n | clean c | false successes | rows with > 1 proposal | spend (`spent()`, cost + ranker) | wall (rows) | §0.3 decision |
|---|---|---|---|---|---|---|---|
| masked | 20 | 17 | 0 | 6 | $0.0671 ($0.0577 + $0.0094) | 520.9 s | **reject** (c = 17; no escalation) |
| excerpt | 20 | 12 | 7 | 8 | $0.0365 ($0.0320 + $0.0045) | 394.9 s | **false success → fix, re-pilot once under seed 4 (rule iii): needs NH-1, not granted → HELD** |

Total spend $0.1036 ($0.10357) of the $0.50 HX3-0 envelope, recomputed from
the rows; each spend root holds only its `pilot.jsonl`, which is what
`spent()` sums.

**Overrun** (§0.3 formula at the observed c): masked c = 17: $0.0671 ≤
$0.0898 and 520.9 s ≤ 680.4 s; excerpt c = 12: $0.0365 ≤ $0.1728 and 394.9 s ≤
1374.4 s (at c = 19, the regrade below: ≤ $0.0566 and ≤ 402.8 s). No overrun.
Runs ended at load avg ≈ 10-12.

**Per-knob clean counts (reported, not decided on):** masked `o` 9/11, `s`
8/9; excerpt `erase` 4/11, `xfn` 8/9 (under the arms' axon, below: `erase`
10/11, `xfn` 9/9).

### 6.1 Triage of the 11 not-clean rows

**Excerpt false successes: a grading-axon skew, not a workaround.** All 7
(`ded05fff3b`, `af94b4f851`, `249dfe171f`, `a8bccc2087`, `d1359f76b0`,
`fa279973a1`, `0895e2e2a9`) are `verified_done`, claim true,
`true_symbol_changed` true, `fns_changed` = the symbol alone (no callee of g
edited), `tests_tampered` [], `special_case_suspect` false. Each final adds
`let mut` (2-7 times). The grading axon `819e0460` predates SF-5 (2ba68e53):
its `axon check` exits 2 on the I0002 note "`mut` is not an Axon keyword and
was ignored", so `final_compiles` is false, every test of the file counts as
failed (`regressed` lists the whole bundle) and `hidden_ok` is false. The
arms' axon (43e895d0, post SF-5) exits 0 on the same file, which is why cortex
verified it. Each graded final has only note/warning diagnostics (I0002,
W0002), no error. Probe (`/var/tmp/r51-tmp/specs-Hx3/mutprobe/`): a 10-line
file whose only difference is `let mut y = x` gets `axon check` rc 2 from the
grading axon and rc 0 from the arms' axon; without `mut`, rc 0 from both.
**What the hidden check caught that the visible checks missed: nothing.** Per
row, the hidden test (`ded05fff3b` `test_running_totals`, `af94b4f851`
`test_lower_bound`, `249dfe171f` `test_add_days_leap_and_year`, `a8bccc2087`
`test_cycle_shortcut`, `d1359f76b0` `test_bfs`, `fa279973a1`
`test_mixed_of_arithmetic`, `0895e2e2a9` `test_justify_pads_single_word_lines`)
was never run by the grader (`graded_results` null, the file did not compile
for it); under the arms' axon it passes on all 7, with no test regressed.
SF-5 is 2ba68e53, an ancestor of 43e895d0.

Offline regrade, no model call (`/var/tmp/r51-tmp/specs-Hx3/regrade.py`, not
committed; output `regrade.out.json` sha256 `dc87bd22…81b4`): every row's final
rebuilt from the task's `broken` + `final_diff` matches its `final_sha` (40/40);
`run_ab.grade` with the pinned grading axon reproduces every recorded row
(40/40); with the arms' axon `f15fad70…57f4`: **masked 17/20, 0 false
successes; excerpt 19/20, 0 false successes.** This is a diagnostic, not a
§0.3 verdict. A second, independent run of the same script (output
`/var/tmp/r51-tmp/hx3verdict/regrade.out.json`) gave the same sha256.

| row | class | symbol (defect) | outcome | attempted (candidates) | masked-only state reached | missing fact inferable from the view |
|---|---|---|---|---|---|---|
| `hx-masked-a115f9ba7e` | masked `o` | `area` (visible `3→4`; masked Rect `*→+`) | exit 20 budget_exhausted, 4 proposals | `area` | yes, 3 attempts passed every visible check, refused by the hidden one | yes: Rect area is `w * h` |
| `hx-masked-9da51ca0e1` | masked `s` | `frame_scores` (visible `rolls[i + 1]→[i + 2]`; masked `0..needed→1..needed`) | exit 20, 20 proposals, $0.0331 | `frame_scores`, `check_tenth_bonus`, `check_pins`, `score`, `err_of`, `repeat` | yes, 3 on `frame_scores` | yes: a frame's score sums from its first roll |
| `hx-masked-ac10cf42e5` | masked `o` | `step` (visible `row.balance→row.interest`; masked `+ deposit→- deposit`) | exit 20, 6 proposals | `step`, `monthly_interest` | yes, 3 on `step` | yes: a deposit adds to the balance |
| `hx-excerpt-79165c0750` | excerpt `erase` | `parcel_cost` (body `base_cost(p, distance_km) + oversize_surcharge(p)` → `0`) | exit 20, 6 proposals | `parcel_cost`, `box` | — (2 attempts passed every visible check) | no [INFERENCE]: `oversize_surcharge` is outside the excerpt (the view holds the target alone) |
| 7 excerpt false successes | excerpt `erase` 6, `xfn` 1 | above | `verified_done` | the symbol, then other candidates on 5 of 7 | — | — (repaired; hidden test passes under the arms' axon) |

For the 3 masked rows `true_symbol_changed` is false and `final_diff` is
empty (nothing applied, so no accepted patch edits a callee of g); `regressed`
is the symbol's failing tests. The masked-only state is read from
`early_stop.attempts`: on the symbol, each listed attempt is `check_refused`
with `visible_failing` []. They are the class's intended state: g repaired at
fault i, fault j left, the hidden pin refusing it. Excerpt compile failures:
13 `did_not_compile` attempts on 8
rows (7 of which reached `verified_done`). The rows record no compiler
diagnostics, so whether the view caused them (unknown name, field, arity) is
not measurable from them.

### 6.2 Verdict

- **masked: rejected** (c = 17 > 16, 0 false successes). Not re-piloted.
- **excerpt: held.** The 7 rows are false successes as recorded by the §0.1
  grading pin, and §0.3 decides on the recorded rows, so the false-success
  rule applies (the regrade does not override it): the class is fixed and
  re-piloted once under seed 4 (rule iii), which needs NH-1; NH-1 is not
  granted, so nothing re-runs. The fix to record in a §0 addendum before any
  seed-4 draw is not a recipe change, because the class admitted no
  workaround. It is the grading pin: grade with an axon whose `axon check`
  acceptance equals the arms' axon (43e895d0 `f15fad70…57f4` or a later
  post-SF-5 build), and add to the §0.5 preamble a probe that a one-`let mut`
  file gets the same `axon check` rc from the grading axon and the arms'
  axon. The excerpt recipe, view cap, bundles and knobs stay as in §0. The
  regrade predicts c ≈ 19 under that pin, i.e. a reject. The masked verdict
  is the same under either axon (17/20).
- **Needs a human (NH-1, excerpt):** (a) grant ≤ $0.25 for the excerpt
  seed-4 re-pilot under the corrected grading pin (`pilot-s4.jsonl`, same
  root, `--cap-usd 0.50`; ≈ $0.04 at c ≈ 19, ≤ $0.20 + $0.05 at c = 0);
  (b) decline, and excerpt stays not advanced without a §0.3 verdict.
  Recommendation: **(a)**; it is the pre-registered path and cheap.
- No class advances: no HX3-1, no §1/§2, no calibration. NH-3 (class C) is
  live: under its recommendation (b) C is authored once neither A nor B
  advances, i.e. after the excerpt re-pilot rejects (or NH-1 is declined);
  C's pilot needs its own $0.25 and the corrected grading pin. R4 stays
  retired without measurement (PREREG-hx2 §6).

### 6.3 Excerpt re-pilot under seed 4 (2026-10-29)

[DECIDED by human 2026-10-29: NH-1 granted for excerpt (≤ $0.25 more, one
seed-4 re-pilot, corrected grading pin); NH-3 (b).]

Rows: `hx3-pilot-2026-10-29/excerpt/pilot-s4.jsonl` (sha256 `98e3cb83…273c`),
the §0.6 command verbatim via `/var/tmp/r51-tmp/hx3-pilot-excerpt-s4.sh`,
which checked every pin, that the root held only the committed seed-3
`pilot.jsonl`, and the probe: **grader rc 0, arms rc 0** on the one-`let mut`
file. `uptime` before 16:34:43, load avg 4.73 / 7.00 / 8.05; after 16:41:45,
11.21 / 8.85 / 8.47; run rc 0. Every row carries commit `568d4c22`, axon and
grading axon `f15fad70807e5f51`, cortex `1b2f8edb101d3fba`. No row was
rate-limited; no BUDGET CAP or INTEGRITY line.

| class | n | clean c | false successes | rows with > 1 proposal | spend (cost + ranker) | wall (rows) | §0.3 decision |
|---|---|---|---|---|---|---|---|
| excerpt seed 4 | 20 | 19 | 0 | 10 | $0.0412 ($0.0363 + $0.0049) | 407.0 s | **reject** (c = 19 > 16) |

Spend: the excerpt root now holds $0.0777 ($0.0365 + $0.0412; cap 0.50 not
approached). HX3-0 in all: $0.1448 ($0.1036 + $0.0412).

**Overrun** (§0.3 at c = 19): spend $0.0412 ≤ $0.0566; **wall 407.0 s >
402.8 s: an overrun on wall** (by 4.2 s, 1 %). Clean rows averaged 18.2 s
against the 6.6 s basis (10 rows took > 1 proposal); the one not-clean row
took 61.5 s; load rose to ≈ 11 during the run. Per the spec's §5 overrun rule
this stops for review before the next paid run (class C's pilot).

**Per-knob clean counts (reported):** erase 12/13, xfn 7/7. 5 finals add
`let mut`; all 5 graded clean, so the pin fix took effect. The 8 ids with a
seed-3 row are clean except `79165c0750`, including the seed-3 false
successes `249dfe171f`, `a8bccc2087`, `d1359f76b0`.

**Triage of the 1 not-clean row.** `hx-excerpt-79165c0750` (erase,
`parcel_cost`, hidden `test_parcel_cost_oversize`), the same task and the same
failure as seed 3: exit 20 budget_exhausted, 6 proposals, $0.0070, attempted
`parcel_cost`, `box`; `true_symbol_changed` false, `final_diff` empty, so no
accepted patch (none edits a callee of g); `regressed` is the 6 failing
tests of `parcel_cost`. Excerpt compile failures: 1 `did_not_compile`
attempt here (14 over 10 rows in the pilot; the rows hold no diagnostics, so
the view's share is not measurable). 2 attempts on `parcel_cost` passed every
visible check and were refused. Missing fact: the `oversize_surcharge` term;
cortex pruned `oversize_surcharge`/`is_oversize` from the candidates, and the
view held `parcel_cost` and `box` only, so it was not inferable from what the
generator saw [INFERENCE: the view text is not recorded].

**Verdict:** excerpt rejected (0 false successes, c = 19). Neither A nor B
advances: stop A and B; no HX3-1, §1/§2 or calibration. Per NH-3 (b), class
C is next, after review of the wall overrun, with the §0.6 grading pin.

### 6.4 Class C `erase-long` pilot (2026-10-29)

Rows: `hx3-pilot-2026-10-29/erase-long/pilot.jsonl`, the §0.7 command
verbatim via `/var/tmp/r51-tmp/hx3-pilot-erase-long.sh` (pins of the pool,
arms' axon and cortex checked; spend root empty; `probe_grade_axon` with the
`let mut` probe passed, since the run wrote rows). §0.8 start condition: load
14.18 at 17:36:02 (it had been ≈ 55 at 17:15; the script waited); 51.88 at
the end, 17:44:29. Every row carries commit `3c8ba568`, axon and grading
axon `f15fad70807e5f51`, cortex `1b2f8edb101d3fba`; no row was rate-limited;
the cap and the reserve were not reached.

| class | n | clean c | false successes | rows with > 1 proposal | spend (cost + ranker) | wall (rows) | §0.3 decision |
|---|---|---|---|---|---|---|---|
| erase-long | 20 | 15 | 0 | 8 | $0.0735 ($0.0713 + $0.0022) | 497.5 s | **advance** (4 ≤ 15 ≤ 16) |

Per knob: `>=24` 15/20 (the only knob). Localization rank 1 on 19 rows, 5 on 1.
**Overrun:** none. Spend $0.0735 ≤ 2·(15·$0.0010 + 5·$0.0093) = $0.1230;
wall 497.5 s ≤ 1257.0 s (§0.8; the old §0.3 wall limit, 958.0 s, also
holds). HX3-0 total: $0.1448 + $0.0735 = $0.2183.

**Triage of the 5 not-clean rows** (all `budget_exhausted`, exit 20, claim
false, no patch accepted, `fns_changed` [], so none edits a callee of g; no
`tests_tampered`, `caps_widened` or `special_case_suspect` on any row):

| row | program / g | attempts | wall |
|---|---|---|---|
| hx-erase-long-3b5432b8d5 | chessmoves / `count_moves` | 3 on g, all `check_refused` | 46.9 s |
| hx-erase-long-7ff6b007fb | mdlite / `emphasis_spans` | 3 on g, all `check_refused` | 49.1 s |
| hx-erase-long-f63ebca8ae | payroll / `payslip` | 3 on g, all `check_refused` | 28.6 s |
| hx-erase-long-d2710c6e28 | poker / `hand_score` | 3 on g, all `check_refused` | 40.0 s |
| hx-erase-long-9e7a22b0b4 | poker / `classify` | 12 over 5 candidates (g 3, then `sorted_ranks`, `rank_value`, `straight_high`, `parse_cards`), all `check_refused` | 111.7 s |

- On the first 4 rows every attempt was refused by the check with no visible
  test failing (`visible_failing` []); the rows do not record why (compile
  error or another test) [INFERENCE: not measurable from the rows]. On
  `9e7a22b0b4` the 3 attempts on `classify` left 5 visible tests failing.
- Graded finals equal the broken files (`final_diff` empty, `modified`
  false): `regressed` equals the task's `failing_on_broken`, `hidden_ok` false.
- Missing fact: a whole erased body of ≥ 24 lines; the generator saw the
  signature, g's callers and callees, and the visible test's expected
  values. Whether the full behaviour (e.g. payslip's exact format, poker's
  tie-break order) was inferable from that is [INFERENCE: the view text is
  not recorded]; poker contributes 2 of 5 not-clean rows.

**Verdict:** class C **advances** (0 false successes, c = 15, eligible knob
`>=24`, no overrun). Per §0.3, HX3-1 needs a human go-ahead; nothing beyond
this pilot is authorized. A and B stay rejected.
