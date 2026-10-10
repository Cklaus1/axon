#!/usr/bin/env python3
"""Read-only comparison of a repository with the reviewed source archive.
No extraction, source execution, git writes, downloads or persistent output.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]


def safe_member(root: Path, relative: str) -> Path:
    parts = Path(relative).parts
    if not parts or Path(relative).is_absolute() or any(p in ('.', '..') for p in parts):
        raise ValueError('unsafe inventory path: ' + relative)
    node = root
    for part in parts:
        node = node / part
        if node.is_symlink():
            raise ValueError('symlink requires review: ' + relative)
    return node


def inspect(repo: Path, inventory: list[dict]) -> dict:
    if repo.is_symlink():
        raise ValueError('repository root must not be a symlink')
    repo = repo.resolve(strict=True)
    if not repo.is_dir():
        raise ValueError('repository root is not a directory')
    matched, changed, missing, unsafe = [], [], [], []
    for row in inventory:
        name = row['path']
        try:
            path = safe_member(repo, name)
        except ValueError as exc:
            unsafe.append(str(exc)); continue
        if not path.is_file():
            missing.append(name); continue
        try:
            h = hashlib.sha256()
            with path.open('rb') as f:
                for block in iter(lambda: f.read(1024 * 1024), b''):
                    h.update(block)
            (matched if h.hexdigest() == row['sha256'] else changed).append(name)
        except OSError as exc:
            unsafe.append(name + ': ' + str(exc))
    findings = []
    script = safe_member(repo, 'scripts/cortex_package_gate.sh')
    if script.is_file():
        text = script.read_text(errors='replace')
        signatures = {
            'V015_VENDOR_PATH': 'axon_cortex_v0_15',
            'LEGACY_CHECKSUM_SCHEMA': 'cortex-package-sha256/1',
            'LEGACY_REPORT_CLI': '--report',
            'ZERO_COUNT_REJECTION': 'min(c.values(), default=0)',
            'VALIDATION_REPORT_HASH_ASSUMPTION': "package_validation.json",
        }
        findings = [name for name, marker in signatures.items() if marker in text]
    return {
        'schema': 'axon.upgrade-plan/1', 'package_version': '0.21', 'mode': 'READ_ONLY',
        'source_mutated': False, 'source_scripts_executed': False,
        'reviewed_files': len(inventory), 'matched_files': len(matched),
        'changed_paths': changed, 'missing_paths': missing, 'unsafe_paths': unsafe,
        'untracked_files': 'NOT_ENUMERATED; review git status separately, do not delete them',
        'package_gate_findings': findings,
        'source_equivalent_on_reviewed_inventory': not (changed or missing or unsafe),
        'proposed_documentation_destination': 'docs/axon_cortex_v0_21/axon-cortex-build-v0_21',
        'do_not_overwrite': ['crates', 'governance', 'docs/axon_cortex_v0_15', 'Cargo.toml', 'Cargo.lock'],
        'next_action': 'Review drift, preserve live evidence and implement B223/B224 before runtime changes',
        'product_gates_executed': 0,
    }


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', required=True, type=Path)
    args = parser.parse_args()
    try:
        inventory = json.loads((ROOT / 'review/source_inventory.json').read_text())
        report = inspect(args.repo, inventory)
        print(json.dumps(report, indent=2))
        return 0 if report['source_equivalent_on_reviewed_inventory'] else 2
    except (OSError, ValueError, KeyError) as exc:
        print(json.dumps({'mode': 'READ_ONLY', 'error': str(exc), 'source_mutated': False}), file=sys.stderr)
        return 2

if __name__ == '__main__':
    raise SystemExit(main())
