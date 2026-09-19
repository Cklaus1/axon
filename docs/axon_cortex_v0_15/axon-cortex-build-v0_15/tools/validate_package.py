#!/usr/bin/env python3
"""Validate the documentation package only; never executes Axon/product gates."""
from __future__ import annotations
import argparse
import json
import re
import sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import unquote


def validate(root: Path) -> dict:
    errors: list[str] = []
    counts: dict[str, int] = {}

    def read_manifest(filename: str, key: str) -> list[dict]:
        try:
            value = json.loads((root / filename).read_text(encoding="utf-8"))[key]
            if not isinstance(value, list):
                raise ValueError(f"{key} must be a list")
            return value
        except (OSError, ValueError, KeyError) as exc:
            errors.append(f"{filename}: {exc}")
            return []

    specs = read_manifest("spec_manifest.json", "specs")
    tasks = read_manifest("task_manifest.json", "tasks")
    gates = read_manifest("gate_manifest.json", "gates")
    sources = read_manifest("source_manifest.json", "sources")

    def indexed(items: list[dict], kind: str) -> dict[str, dict]:
        result: dict[str, dict] = {}
        for item in items:
            ident = item.get("id")
            if not isinstance(ident, str) or not ident:
                errors.append(f"{kind}: missing/invalid ID")
                continue
            if ident in result:
                errors.append(f"{kind}: duplicate ID {ident}")
            result[ident] = item
        return result

    si, ti, gi = indexed(specs, "spec"), indexed(tasks, "task"), indexed(gates, "gate")

    def dag(items: dict[str, dict], kind: str) -> list[str]:
        state: dict[str, int] = {}
        order: list[str] = []

        def visit(ident: str, stack: list[str]) -> None:
            if state.get(ident) == 2:
                return
            if state.get(ident) == 1:
                errors.append(f"{kind} dependency cycle: {' -> '.join(stack + [ident])}")
                return
            state[ident] = 1
            for dep in items[ident].get("depends_on", []):
                if dep not in items:
                    errors.append(f"{kind} {ident}: unknown dependency {dep}")
                else:
                    visit(dep, stack + [ident])
            state[ident] = 2
            order.append(ident)

        for ident in items:
            visit(ident, [])
        return order

    spec_order, task_order = dag(si, "spec"), dag(ti, "task")
    declared_gate_ids: set[str] = set()
    for ident, item in si.items():
        path = root / item.get("path", "")
        if not path.is_file():
            errors.append(f"spec {ident}: missing file {path}")
            continue
        text = path.read_text(encoding="utf-8")
        if item.get("status") != "Draft" or "status: Draft\n" not in text:
            errors.append(f"spec {ident}: status must remain Draft in this proposal package")
        if "implementation_evidence: []" not in text:
            errors.append(f"spec {ident}: proposal must not invent implementation evidence")
        match = re.search(r"^depends_on: (\[.*\])$", text, re.M)
        try:
            deps = json.loads(match.group(1)) if match else None
        except ValueError:
            deps = None
        if deps != item.get("depends_on"):
            errors.append(f"spec {ident}: metadata dependencies differ from manifest")
        if not re.search(rf"^id: {re.escape(ident)}$", text, re.M):
            errors.append(f"spec {ident}: metadata ID mismatch")
        definitions = re.findall(r"\*\*(G\d{2}-[a-z-]+):\*\*", text)
        for gate in definitions:
            if gate in declared_gate_ids:
                errors.append(f"gate {gate}: duplicate definition")
            declared_gate_ids.add(gate)
            if gate not in gi:
                errors.append(f"spec {ident}: gate {gate} absent from registry")
            elif gi[gate].get("spec") != ident:
                errors.append(f"gate {gate}: incorrect owning spec")

    for ident, item in gi.items():
        if ident not in declared_gate_ids:
            errors.append(f"gate {ident}: no definition in spec")
        if item.get("spec") not in si:
            errors.append(f"gate {ident}: unknown spec")
        if item.get("product_result") != "NOT_RUN":
            errors.append(f"gate {ident}: package must not claim a product run")

    for ident, item in ti.items():
        if item.get("status") != "Not started":
            errors.append(f"task {ident}: proposal must remain Not started")
        for ref in item.get("specs", []):
            if ref not in si:
                errors.append(f"task {ident}: unknown spec {ref}")
        for ref in item.get("gate_targets", []):
            if ref not in gi:
                errors.append(f"task {ident}: unknown gate {ref}")
        if not item.get("optional"):
            for dep in item.get("depends_on", []):
                if ti.get(dep, {}).get("optional"):
                    errors.append(f"task {ident}: required slice depends on optional {dep}")

    md_files = list(root.rglob("*.md"))
    local_link_count = 0
    word_count = 0
    for path in md_files:
        text = path.read_text(encoding="utf-8")
        if path.name != "AXON_CORTEX_MASTER.md":
            word_count += len(text.split())
        if "\x00" in text:
            errors.append(f"{path.relative_to(root)}: null byte")
        fences = re.findall(r"^\s*```", text, re.M)
        if len(fences) % 2:
            errors.append(f"{path.relative_to(root)}: unbalanced fenced code blocks")
        for target in re.findall(r"\[[^\]\n]*\]\(([^\)\n]+)\)", text):
            target = target.strip()
            if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", target):
                continue
            file_part = unquote(target.split("#", 1)[0])
            if not file_part:
                continue
            destination = (path.parent / file_part).resolve()
            local_link_count += 1
            if not destination.is_relative_to(root):
                errors.append(f"{path.relative_to(root)}: link escapes package: {target}")
            elif not destination.is_file():
                errors.append(f"{path.relative_to(root)}: broken local link: {target}")

    counts.update(specifications=len(si), work_packages=len(ti), proposed_product_gates=len(gi),
                  source_attachments=len(sources), markdown_files=len(md_files),
                  local_file_links_checked=local_link_count, words_excluding_master=word_count)
    return {
        "schema": "cortex-package-validation/1",
        "validated_at": datetime.now(timezone.utc).isoformat(),
        "scope": "Documentation consistency only. No Axon build, runtime, safety gate or model benchmark executed.",
        "result": "PASS" if not errors else "FAIL",
        "counts": counts,
        "spec_topological_order": spec_order,
        "task_topological_order": task_order,
        "product_gates_executed": 0,
        "errors": errors,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--report", type=Path, help="Defaults to <root>/package_validation.json")
    args = parser.parse_args()
    root = args.root.resolve()
    if not root.is_dir():
        parser.error(f"Not a directory: {root}")
    report = validate(root)
    output = args.report or root / "package_validation.json"
    output.write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(report, indent=2))
    return 0 if report["result"] == "PASS" else 1


if __name__ == "__main__":
    sys.exit(main())
