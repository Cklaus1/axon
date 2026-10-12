#!/usr/bin/env python3
"""Read-only paired source inventory check; never runs or writes source code.
Exit 2 requires intake/reconciliation, not an instruction to overwrite local work.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import stat
import sys
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[1]


def safe_path(root: Path, name: str) -> Path:
    if '\\' in name or '\x00' in name:
        raise ValueError('unsafe path encoding')
    parts=name.split('/')
    if not name or name.startswith('/') or any(x in ('','..','.') for x in parts) or ':' in parts[0]:
        raise ValueError('unsafe inventory path')
    node=root
    for part in parts:
        node=node/part
        if node.is_symlink():raise ValueError('symlink needs review: '+name)
    return node


def inspect(repo: Path, rows: list[dict]) -> dict:
    if repo.is_symlink():raise ValueError('source root is a symlink')
    root=repo.resolve(strict=True)
    if not root.is_dir():raise ValueError('source root is not a directory')
    matched=[];changed=[];missing=[];unsafe=[];seen=set()
    for row in rows:
        name=row['path'];seen.add(name)
        try:
            path=safe_path(root,name)
            if not path.exists():missing.append(name);continue
            if not stat.S_ISREG(path.stat().st_mode):unsafe.append(name+': not a regular file');continue
            h=hashlib.sha256()
            with path.open('rb') as f:
                while block:=f.read(1024*1024):h.update(block)
            (matched if h.hexdigest()==row['sha256'] else changed).append(name)
        except (OSError,ValueError) as exc:unsafe.append(name+': '+str(exc))
    extra=[];excluded=[]
    # Never follow symlink directories. .git and known generated dependency/build dirs
    # are enumerated as exclusions, not treated as evidence of a clean checkout.
    omit={'.git','target','node_modules','.venv','venv'}
    for current,dirs,files in os.walk(root,followlinks=False):
        for name in list(dirs):
            p=Path(current)/name
            if p.is_symlink():unsafe.append(p.relative_to(root).as_posix()+': symlink directory');dirs.remove(name)
            elif name in omit:excluded.append(p.relative_to(root).as_posix());dirs.remove(name)
        for name in files:
            p=Path(current)/name;rel=p.relative_to(root).as_posix()
            if p.is_symlink():unsafe.append(rel+': symlink file')
            if rel not in seen:extra.append(rel)
    needs_review=bool(changed or missing or unsafe or extra)
    return {'reviewed_files':len(rows),'matched_files':len(matched),'changed_paths':changed,'missing_paths':missing,
            'unsafe_paths':sorted(set(unsafe)),'extra_paths':sorted(extra),'excluded_metadata_or_build_directories':sorted(excluded),
            'source_matches_reviewed_inventory':not(changed or missing or unsafe),'intake_review_required':needs_review,
            'git_cleanliness':'NOT_ESTABLISHED; inspect current git status/diff/staged/untracked work separately',
            'writes_performed':False,'source_scripts_executed':False}


def main() -> int:
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--axon',required=True,type=Path);p.add_argument('--micode',required=True,type=Path)
    a=p.parse_args()
    try:
        reports={name:inspect(path,json.loads((ROOT/f'integration/source-inventory-{name}_source.json').read_text()))
                 for name,path in [('axon',a.axon),('micode',a.micode)]}
        result={'schema':'axon.paired-upgrade-plan/1','package_version':'0.22','mode':'READ_ONLY','sources':reports,
                'source_mutated':False,'product_gates_executed':0,'runtime_qualification':'NOT_RUN',
                'next_step':'Reconcile B255; preserve live evidence and local work; use build/UPGRADE_V022.md',
                'proposed_documentation_destination':'docs/axon_cortex_v0_22/axon-cortex-build-v0_22'}
        print(json.dumps(result,indent=2))
        return 2 if any(r['intake_review_required'] for r in reports.values()) else 0
    except (OSError,ValueError,KeyError,TypeError) as exc:
        print(json.dumps({'mode':'READ_ONLY','error':str(exc),'source_mutated':False}),file=sys.stderr);return 2

if __name__=='__main__':raise SystemExit(main())
