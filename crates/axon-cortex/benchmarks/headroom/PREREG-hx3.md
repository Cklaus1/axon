# hx3 pre-registration

Status: **§0 is committed before any pilot row.** §1/§2 are written at N3
only if a class advances; §6 records the HX3-0 pilot rows and the verdict.
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
