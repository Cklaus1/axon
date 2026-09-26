#!/usr/bin/env python3
"""Honesty invariant for a vendored Cortex build package — shared by
scripts/cortex_package_gate.sh (v0.15) and scripts/cortex_package_gate_v022.sh.

A gate in the package's gate_manifest.json may carry a product_result other than
NOT_RUN, and a work package in task_manifest.json a status other than
"Not started", ONLY when governance/cortex_gate_execution_registry.json has a row
for it naming a file in THIS repository that exists and is grepped out of an
invoker that exists. EVERY registry row is re-validated on every run, including
rows for gates the package still reports as NOT_RUN, so a row whose script was
deleted or unwired breaks the day it rots, not the day someone flips a manifest.

This is the v0.15 logic moved here unchanged so both packages are held to one
implementation (Stage 6, lesson 7). Stdlib only; never imports package code.
Run from the repository root (registry paths are repo-relative).

Exit 0 = invariant holds; exit 1 = violated (reasons on stdout).
"""
from __future__ import annotations

import argparse
import json
import os
import sys

DEFAULT_RESULT = "NOT_RUN"
DEFAULT_STATUS = "Not started"
DONE_STATUSES = {"Done", "Complete", "Completed", "Landed"}


def load(path):
    with open(path, encoding="utf-8") as fh:
        return json.load(fh)


def registry_row_is_real(row, what):
    """A row may only vouch for a gate if it names a file that EXISTS and is
    INVOKED. Grepping the invoker for the executed path is what stops the
    registry from being a place to declare things green."""
    bad = []
    ex, inv = row.get("executed_by"), row.get("invoked_by")
    if not ex or not os.path.isfile(ex):
        return [f"{what}: executed_by {ex!r} does not exist in this repo"]
    if not os.access(ex, os.X_OK) and not ex.endswith((".rs", ".py")):
        bad.append(f"{what}: executed_by {ex!r} is not executable")
    if not inv or not os.path.isfile(inv):
        return bad + [f"{what}: invoked_by {inv!r} does not exist in this repo"]
    if os.path.realpath(inv) == os.path.realpath(ex):
        return bad + [f"{what}: invoked_by is the executed script itself — a script cannot invoke itself into running"]
    try:
        text = open(inv, encoding="utf-8", errors="replace").read()
    except OSError as e:
        return bad + [f"{what}: cannot read invoker {inv!r}: {e}"]
    if ex not in text and os.path.basename(ex) not in text:
        bad.append(f"{what}: {inv} does not invoke {ex} — a registry row may not "
                   f"vouch for a script nothing runs (orphaned-gate class)")
    return bad


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, allow_abbrev=False)
    ap.add_argument("--pkg", required=True)
    ap.add_argument("--execution-registry", dest="registry", required=True)  # not --re…: the v0.22 gate self-greps for --re* (argparse prefixes of --refresh-hashes)
    ap.add_argument("--min-gates", type=int, required=True)
    ap.add_argument("--min-tasks", type=int, required=True)
    ap.add_argument("--also-known", action="append", default=[],
                    help="another vendored package whose gate/task IDs are not 'unknown' for notices")
    a = ap.parse_args()

    gates = load(os.path.join(a.pkg, "gate_manifest.json"))["gates"]
    tasks = load(os.path.join(a.pkg, "task_manifest.json"))["tasks"]
    reg = load(a.registry)
    errors, notices = [], []

    if reg.get("schema") != "cortex-gate-execution/1":
        errors.append(f"{a.registry}: schema is {reg.get('schema')!r}, expected cortex-gate-execution/1")
    reg_gates = {r.get("gate_id"): r for r in reg.get("gates", [])}
    reg_tasks = {r.get("task_id"): r for r in reg.get("tasks", [])}
    if len(reg_gates) != len(reg.get("gates", [])):
        errors.append(f"{a.registry}: duplicate gate_id rows")
    if len(reg_tasks) != len(reg.get("tasks", [])):
        errors.append(f"{a.registry}: duplicate task_id rows")

    if len(gates) < a.min_gates:
        errors.append(f"NON-VACUITY: parsed {len(gates)} gates, floor is {a.min_gates}")
    if len(tasks) < a.min_tasks:
        errors.append(f"NON-VACUITY: parsed {len(tasks)} work packages, floor is {a.min_tasks}")

    n_off_g = 0
    for g in gates:
        gid, res = g.get("id"), g.get("product_result")
        if res is None:
            errors.append(f"gate {gid}: no product_result field"); continue
        if res == DEFAULT_RESULT:
            continue
        n_off_g += 1
        row = reg_gates.get(gid)
        if row is None:
            errors.append(f"gate {gid}: product_result={res!r} but no row in {a.registry}. "
                          f"The package may only claim a gate ran if something in this repo runs it.")
            continue
        allowed = row.get("allowed_product_result") or []
        if res not in allowed:
            errors.append(f"gate {gid}: product_result={res!r} not in registry allowed_product_result {allowed}")

    n_off_t = 0
    for t in tasks:
        tid, st = t.get("id"), t.get("status")
        if st is None:
            errors.append(f"task {tid}: no status field"); continue
        if st == DEFAULT_STATUS:
            continue
        n_off_t += 1
        row = reg_tasks.get(tid)
        if row is None:
            errors.append(f"task {tid}: status={st!r} but no row in {a.registry}."); continue
        allowed = row.get("allowed_status") or []
        if st not in allowed:
            errors.append(f"task {tid}: status={st!r} not in registry allowed_status {allowed}")
        # "Done" implies its gate targets ran; each must be registry-backed too.
        if st in DONE_STATUSES:
            for gt in t.get("gate_targets", []):
                if gt not in reg_gates:
                    errors.append(f"task {tid}: status={st!r} but gate target {gt} has no registry row")

    gate_ids = {g.get("id") for g in gates}
    task_ids = {t.get("id") for t in tasks}
    other_g, other_t = set(), set()
    for other in a.also_known:
        try:
            other_g |= {g.get("id") for g in load(os.path.join(other, "gate_manifest.json"))["gates"]}
            other_t |= {t.get("id") for t in load(os.path.join(other, "task_manifest.json"))["tasks"]}
        except (OSError, ValueError, KeyError) as e:
            notices.append(f"--also-known {other}: unreadable ({e})")
    for gid in sorted(set(reg_gates) - gate_ids - other_g):
        notices.append(f"registry row for unknown gate {gid} (in no vendored gate_manifest.json)")
    for tid in sorted(set(reg_tasks) - task_ids - other_t):
        notices.append(f"registry row for unknown task {tid} (in no vendored task_manifest.json)")

    by_id = {g.get("id"): g for g in gates}
    n_backed = 0
    for gid, row in sorted(reg_gates.items()):
        errors.extend(registry_row_is_real(row, f"registry row {gid}"))
        if gid in by_id:
            n_backed += 1
            if by_id[gid].get("product_result") == DEFAULT_RESULT:
                notices.append(f"{gid} is executed by {row.get('executed_by')} (invoked by "
                               f"{row.get('invoked_by')}) while the vendored manifest says NOT_RUN — "
                               f"expected: NOT_RUN is a true statement about the PACKAGE")
    for tid, row in sorted(reg_tasks.items()):
        if not row.get("allowed_status"):
            errors.append(f"registry task row {tid}: no allowed_status")

    for n in notices:
        print(f"  notice: {n}")
    if errors:
        print("  honesty invariant VIOLATED:")
        for e in errors[:40]:
            print("   ", e)
        return 1
    print(f"  {len(gates)} gates, {len(tasks)} work packages parsed; {n_off_g} gates off {DEFAULT_RESULT} and "
          f"{n_off_t} tasks off '{DEFAULT_STATUS}', all registry-backed; registry rows valid: "
          f"{len(reg_gates)} gate(s) ({n_backed} for this package), {len(reg_tasks)} task(s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
