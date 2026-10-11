#!/usr/bin/env python3
"""Report source drift without modifying a checkout. Exit 2 means rebase needed."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source', type=Path, required=True)
    parser.add_argument('--all', action='store_true', help='Check all archive files, not just reviewed evidence files.')
    args = parser.parse_args()
    package = Path(__file__).resolve().parents[1]
    source = args.source.resolve()
    manifest = json.loads((package/'manifests/source_manifest.json').read_text())
    evidence = json.loads((package/'manifests/evidence_index.json').read_text())
    paths = sorted(manifest['files'] if args.all else {r['path'] for r in evidence['evidence']})
    drift = []
    for rel in paths:
        p = (source/rel).resolve()
        if not p.is_relative_to(source):
            drift.append({'path': rel, 'state': 'outside_source_root'}); continue
        if not p.is_file():
            drift.append({'path': rel, 'state': 'missing'}); continue
        digest = hashlib.sha256(p.read_bytes()).hexdigest()
        if digest != manifest['files'][rel]['sha256']:
            drift.append({'path': rel, 'state': 'changed', 'actual_sha256': digest})
    print(json.dumps({'schema':'acf-source-drift/1','checked':len(paths),'result':'MATCH' if not drift else 'REBASE_REQUIRED','drift':drift,'note':'New checkout files and excluded archive content require separate intake. Never overwrite to make this check pass.'}, indent=2))
    return 2 if drift else 0

if __name__ == '__main__':
    raise SystemExit(main())
