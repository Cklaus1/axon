#!/usr/bin/env python3
"""
analyze.py — cross-run analysis of the Axon provenance log.

Reads ~/.cache/axon/provenance.jsonl and prints:
  - per-fn summary (call/return counts, score distribution, time gaps)
  - convergence analysis (how many evals to reach 90% of best)
  - latency breakdown (where wall-clock time is spent)
  - optimization recommendations:
      * variants to cull (never improve over baseline)
      * suggested max_evals (based on observed plateau point)
      * cost-per-score-point estimate (when LLM-call timing is available)

Zero external deps — stdlib only.

Usage:
  python3 analyze.py                       # analyze whole log
  python3 analyze.py --fn try_variant      # filter to one fn
  python3 analyze.py --tail 100            # last N records only
  python3 analyze.py --json                # machine-readable output
"""
from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
from collections import defaultdict
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


def prov_path() -> Path:
    base = os.environ.get("XDG_CACHE_HOME") or str(Path.home() / ".cache")
    return Path(base) / "axon" / "provenance.jsonl"


@dataclass
class FnStats:
    name: str
    calls: int = 0
    returns: int = 0
    scores: list[float] = field(default_factory=list)
    timestamps: list[int] = field(default_factory=list)

    def summary(self) -> dict[str, Any]:
        s = self.scores
        ts = sorted(self.timestamps)
        gaps = [(ts[i] - ts[i - 1]) / 1000.0 for i in range(1, len(ts))]

        out: dict[str, Any] = {
            "fn": self.name,
            "calls": self.calls,
            "returns": self.returns,
        }
        if s:
            out["score_min"] = round(min(s), 3)
            out["score_max"] = round(max(s), 3)
            out["score_mean"] = round(statistics.mean(s), 3)
            out["score_first"] = round(s[0], 3)
            out["score_last"] = round(s[-1], 3)
            if len(s) > 1:
                out["score_stdev"] = round(statistics.stdev(s), 3)
            # Convergence: index of first record at >= 90% of max.
            threshold = 0.9 * max(s)
            for i, v in enumerate(s):
                if v >= threshold:
                    out["evals_to_90pct"] = i + 1
                    break
            # Plateau detection: largest stretch where score doesn't improve.
            best = s[0]
            stale = 0
            max_stale = 0
            for v in s[1:]:
                if v > best:
                    best = v
                    stale = 0
                else:
                    stale += 1
                    max_stale = max(max_stale, stale)
            out["max_plateau_run"] = max_stale
        if gaps:
            out["gap_min_s"] = round(min(gaps), 3)
            out["gap_max_s"] = round(max(gaps), 3)
            out["gap_mean_s"] = round(statistics.mean(gaps), 3)
            out["gap_total_s"] = round(sum(gaps), 3)
        return out

    def recommendations(self) -> list[str]:
        recs: list[str] = []
        s = self.scores
        if len(s) < 4:
            return recs
        # Did the search ever improve over its first observation?
        if max(s) <= s[0] + 1e-6:
            recs.append(
                f"variant search produced no improvement over the seed eval "
                f"({s[0]:.2f}); consider widening the prompt-variant catalog or "
                f"changing the score function"
            )
        # Suggested max_evals: index of first eval at 95% of max + 4 buffer.
        threshold = 0.95 * max(s)
        for i, v in enumerate(s):
            if v >= threshold:
                suggested = max(8, i + 4)
                if suggested + 4 < len(s):
                    recs.append(
                        f"suggested max_evals={suggested} (95% of best score reached at eval {i + 1}; "
                        f"current run used {len(s)} evals)"
                    )
                break
        # Plateau too long → narrow search or stop earlier.
        best = s[0]
        stale = 0
        max_stale = 0
        for v in s[1:]:
            if v > best:
                best = v
                stale = 0
            else:
                stale += 1
                max_stale = max(max_stale, stale)
        if max_stale >= 8:
            recs.append(
                f"observed plateau of {max_stale} evals with no improvement; "
                f"consider an early-stop policy (Phase 8 work)"
            )
        # Convergence variability.
        if len(s) > 4:
            second_half = s[len(s) // 2 :]
            if statistics.stdev(second_half) > 5.0:
                recs.append(
                    f"high score variance in second half (stdev={statistics.stdev(second_half):.1f}); "
                    f"signal is noisy — consider averaging N samples per variant"
                )
        return recs


def load_records(path: Path, tail: int | None) -> list[dict]:
    if not path.exists():
        return []
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    if tail is not None:
        lines = lines[-tail:]
    out = []
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return out


def aggregate(records: list[dict], fn_filter: str | None) -> dict[str, FnStats]:
    by_fn: dict[str, FnStats] = defaultdict(lambda: FnStats(name=""))
    for r in records:
        fn = r.get("fn", "?")
        if fn_filter and fn != fn_filter:
            continue
        s = by_fn[fn]
        s.name = fn
        ev = r.get("event")
        if ev == "call":
            s.calls += 1
        if ev == "return":
            s.returns += 1
        if "score" in r:
            try:
                s.scores.append(float(r["score"]))
            except (TypeError, ValueError):
                pass
        if "ts_ms" in r:
            try:
                s.timestamps.append(int(r["ts_ms"]))
            except (TypeError, ValueError):
                pass
    return by_fn


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.strip().splitlines()[0])
    ap.add_argument("--fn", default=None, help="filter to a single fn name")
    ap.add_argument("--tail", type=int, default=None, help="only last N records")
    ap.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    ap.add_argument("--path", default=str(prov_path()), help="path to provenance.jsonl")
    args = ap.parse_args()

    path = Path(args.path)
    records = load_records(path, args.tail)
    if not records:
        print(f"no records at {path}", file=sys.stderr)
        return 1

    by_fn = aggregate(records, args.fn)

    if args.json:
        out = {
            "path": str(path),
            "total_records": len(records),
            "fns": {fn: s.summary() for fn, s in sorted(by_fn.items())},
            "recommendations": {fn: s.recommendations() for fn, s in sorted(by_fn.items())},
        }
        print(json.dumps(out, indent=2))
        return 0

    print(f"# Axon provenance analysis: {path}")
    print(f"# total records: {len(records)}; fns: {len(by_fn)}")
    print()
    for fn, s in sorted(by_fn.items()):
        sm = s.summary()
        print(f"=== fn: {fn}")
        for k, v in sm.items():
            if k == "fn":
                continue
            print(f"  {k:24s}: {v}")
        recs = s.recommendations()
        if recs:
            print(f"  recommendations:")
            for r in recs:
                print(f"    - {r}")
        print()
    return 0


if __name__ == "__main__":
    sys.exit(main())
