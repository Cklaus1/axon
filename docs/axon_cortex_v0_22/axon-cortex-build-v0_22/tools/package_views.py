#!/usr/bin/env python3
"""Generate reviewable Markdown views from authoritative source/manifests; no product execution."""
from __future__ import annotations
import argparse, hashlib, json, posixpath, re
from pathlib import Path
from urllib.parse import urlsplit, urlunsplit

VIEW_PATHS=('specs/INDEX.md','build/TASKS.md','build/ACCEPTANCE_GATES.md','AXON_CORTEX_MASTER.md')

def load(root: Path, name: str, key: str):
    return json.loads((root/name).read_text(encoding='utf-8'))[key]

def cell(s):return str(s).replace('|',r'\|').replace('\n',' ')

def spec_index(root: Path) -> str:
    specs=load(root,'spec_manifest.json','specs')
    out=['# Axon Cortex spec index — v0.22','','Generated from `spec_manifest.json`. All specifications are Draft proposals. CX-35 remains reserved in this package namespace (the source uses that ID for Reflex serving; see integration/SOURCE_OWNER_MAP.json); CX-36 does not create a second CX-23 runtime; CX-37 owns the ACE provider/runtime contract.','','| ID | Specification | Contract dependencies | First contract stage |','|---|---|---|---|']
    for s in specs:
        out.append(f"| {s['id']} | [{cell(s['title'])}]({Path(s['path']).name}) | {', '.join(s['depends_on']) or 'none'} | {s['stage']} |")
    out += ['','A spec dependency requires its relevant contract, not completion of every experiment. The [task DAG](../build/TASKS.md) and [release slices](../build/IMPLEMENTATION_RELEASE_SLICES.md) define implementation ordering.','']
    return '\n'.join(out)

def tasks_view(root: Path) -> str:
    tasks=load(root,'task_manifest.json','tasks')
    out=['# Axon Cortex implementation task DAG — v0.22','','Generated from `task_manifest.json`. Task statuses are **Not started** in this proposed package; they do not reset or establish the existing repository implementation status. Documentation/reference tests do not mark product work complete. Historical IDs are preserved; amendments are logged in `review/BASELINE_GAPS.json`.','','See [release slices](IMPLEMENTATION_RELEASE_SLICES.md) for bounded entry points. A late stage denotes maturity; depend only on explicit task edges, not the numeric task identifier. Optional research is not required to succeed.','','| Task | Stage | Work package | Depends on | Spec owners | Gate targets | Lane | Optional |','|---|---|---|---|---|---|---|---|']
    for t in tasks:
        out.append(f"| {t['id']} | {t['stage']} | {cell(t['title'])} | {', '.join(t['depends_on']) or 'none'} | {', '.join(t['specs'])} | {', '.join(t['gate_targets'])} | {t['lane']} | {'yes' if t['optional'] else 'no'} |")
    out += ['','## Work-package outputs','']
    for t in tasks:
        out += [f"### {t['id']} — {t['title']}",'',t['output'],'']
    return '\n'.join(out)

def gates_view(root: Path) -> str:
    gates=load(root,'gate_manifest.json','gates');tasks=load(root,'task_manifest.json','tasks')
    owners={g['id']:[t['id'] for t in tasks if g['id'] in t['gate_targets']] for g in gates}
    out=['# Axon Cortex acceptance gates — v0.22','','Generated from source spec declarations and `gate_manifest.json`. Every product result is **NOT_RUN**. A schema/container fixture PASS is not semantic correctness, authorization, model quality or runtime confinement. Every gate has at least one owning work package.','']
    for g in gates:
        out += [f"## {g['id']}",'',f"Owner: [{g['spec']}](../{g['spec_path']}). Work packages: {', '.join(owners[g['id']])}. Product result: `NOT_RUN`.",'',g['description'],'']
    return '\n'.join(out)

def rebase_links(text: str, path: str) -> str:
    base=posixpath.dirname(path)
    def sub(m):
        raw=m.group(2);u=urlsplit(raw)
        if u.scheme or u.netloc or raw.startswith('/') or raw.startswith('mailto:'):return m.group(0)
        p=posixpath.normpath(posixpath.join(base,u.path)) if u.path else path
        return m.group(1)+urlunsplit(('', '', p,u.query,u.fragment))+m.group(3)
    # Code examples stay byte-for-byte: they are examples, not navigational links.
    lines=[];fence=None
    for line in text.splitlines(keepends=True):
        fm=re.match(r'^\s*(`{3,}|~{3,})',line)
        if fm:
            mark=fm.group(1)
            if fence is None:fence=mark
            elif mark[0]==fence[0] and len(mark)>=len(fence):fence=None
            lines.append(line);continue
        lines.append(line if fence else re.sub(r'(!?\[[^\]\n]*\]\()([^\s)]+)(\))',sub,line))
    return ''.join(lines)

def render_all(root: Path) -> dict[str,str]:
    views={'specs/INDEX.md':spec_index(root),'build/TASKS.md':tasks_view(root),'build/ACCEPTANCE_GATES.md':gates_view(root)}
    paths={p.relative_to(root).as_posix() for p in root.rglob('*.md') if p.name!='AXON_CORTEX_MASTER.md'} | set(views)
    lead=['README.md','build/OWNERSHIP_AND_COMPATIBILITY.md','build/REVIEW_INVARIANTS.md','review/REVIEW_REPORT.md','build/IMPLEMENTATION_RELEASE_SLICES.md','build/BUILD_PLAN.md','specs/INDEX.md']
    def order(path):
        if path in lead:return (0,lead.index(path),'')
        if path.startswith('specs/'):return (1,0,path)
        if path.startswith('schemas/'):return (2,0,path)
        if path.startswith('build/'):return (3,0,path)
        if path.startswith('CHANGELOG'):return (9,0,path)
        return (4,0,path)
    paths=sorted(paths,key=order)
    text=['# Axon Cortex master — v0.22','','**Generated compilation, not a second specification owner.** All source documents below are included in full. Current source files and their per-section digests are authoritative; historical changelogs retain old counts as history. Product gates remain NOT_RUN.','','Rebuild with `python -B tools/package_views.py --write`; the package validator rejects a stale master.','']
    for path in paths:
        original=views.get(path,(root/path).read_text(encoding='utf-8') if (root/path).exists() else '')
        h=hashlib.sha256(original.encode()).hexdigest()
        text += ['---','',f'# Source: `{path}`','',f'<!-- source-path: {path}; sha256: {h} -->','',rebase_links(original,path).rstrip(),'']
    views['AXON_CORTEX_MASTER.md']='\n'.join(text)
    return views

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--write',action='store_true');a=p.parse_args()
    views=render_all(a.root)
    if a.write:
        for name,text in views.items(): (a.root/name).write_text(text,encoding='utf-8')
        print(json.dumps({'generated':list(views),'scope':'DOCUMENTATION_ONLY'}))
    else:
        stale=[name for name,text in views.items() if not (a.root/name).exists() or (a.root/name).read_text(encoding='utf-8')!=text]
        print(json.dumps({'stale':stale,'scope':'DOCUMENTATION_ONLY'}));return int(bool(stale))
    return 0
if __name__=='__main__':raise SystemExit(main())
