"""Verify archive inventory and hashes, using only Python's standard library."""
from __future__ import annotations

import hashlib
import json
from pathlib import Path, PurePosixPath
import sys

ROOT = Path(__file__).resolve().parents[1]
IGNORED_DIRS = {'.venv', '__pycache__', '.git', '.pytest_cache', 'local-runs'}


def main() -> int:
    try:
        manifest = json.loads((ROOT / 'MANIFEST.json').read_text())
        expected = set()
        for record in manifest['files']:
            rel = record['path']
            pure = PurePosixPath(rel)
            if pure.is_absolute() or '..' in pure.parts or rel in expected:
                raise ValueError(f'Unsafe or duplicate inventory path: {rel}')
            expected.add(rel)
            path = ROOT / rel
            if not path.is_file() or path.is_symlink():
                raise ValueError(f'Missing or unsafe file: {rel}')
            data = path.read_bytes()
            if len(data) != record['bytes'] or hashlib.sha256(data).hexdigest() != record['sha256']:
                raise ValueError(f'Checksum/size mismatch: {rel}')
        actual = {p.relative_to(ROOT).as_posix() for p in ROOT.rglob('*') if p.is_file()
                  and not any(part in IGNORED_DIRS for part in p.relative_to(ROOT).parts)
                  and p.name not in {'MANIFEST.json', 'SHA256SUMS'} and not p.name.endswith('.pyc')}
        if actual != expected:
            raise ValueError(f'Inventory mismatch; extra={sorted(actual-expected)}, missing={sorted(expected-actual)}')
        listed_sums = set()
        for line in (ROOT / 'SHA256SUMS').read_text().splitlines():
            expected_hash, rel = line.split('  ', 1)
            pure = PurePosixPath(rel)
            if pure.is_absolute() or '..' in pure.parts or rel in listed_sums:
                raise ValueError('Unsafe/duplicate SHA256SUMS path')
            listed_sums.add(rel)
            if hashlib.sha256((ROOT / rel).read_bytes()).hexdigest() != expected_hash:
                raise ValueError(f'SHA256SUMS mismatch: {rel}')
        if listed_sums != expected | {'MANIFEST.json'}:
            raise ValueError('SHA256SUMS coverage mismatch')
        print(f'PASS: {len(expected)} payload files and manifest verified; SHA256SUMS excludes itself.')
        return 0
    except (OSError, ValueError, KeyError, TypeError) as exc:
        print(f'FAIL: {exc}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
