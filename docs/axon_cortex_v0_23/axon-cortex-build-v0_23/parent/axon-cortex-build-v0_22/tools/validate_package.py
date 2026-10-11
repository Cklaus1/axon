#!/usr/bin/env python3
"""Validate proposed documents/manifests/inert format fixtures. NEVER executes product gates."""
from __future__ import annotations
import argparse, hashlib, json, re, sys
from datetime import datetime, timezone
from pathlib import Path
from urllib.parse import urlsplit, unquote
sys.dont_write_bytecode=True
from package_views import render_all
from research_package_checks import checks as research_checks
from closed_loop_package_checks import checks as closed_loop_checks
from schema_reflex_package_checks import checks as schema_reflex_checks
from ace_integration_checks import checks as ace_checks
from neural_contract_reference import validate_package as validate_np, validate_source, ContractError
from jsonschema import Draft202012Validator

HASH_FILE='SHA256SUMS_v0_22.json'
# Non-recursive output receipts; all other files, including test results, are hashed.
HASH_EXCLUDED={HASH_FILE,'package_validation.json'}
GATE_RE=re.compile(r'^\s*(?:-\s*)?\*\*(G\d{2}-[a-z0-9-]+):\*\*\s*(.+)$',re.M)

def file_digest(p: Path) -> str:
    h=hashlib.sha256()
    with p.open('rb') as f:
        while b:=f.read(1<<20):h.update(b)
    return h.hexdigest()

def hash_inventory(root: Path) -> dict:
    return {p.relative_to(root).as_posix():file_digest(p) for p in sorted(root.rglob('*')) if p.is_file() and p.relative_to(root).as_posix() not in HASH_EXCLUDED}

def validate(root: Path, *, integrity: bool=True) -> dict:
    root=root.resolve();errors=[];counts={};orders={};profiles={}
    def err(s):errors.append(s)
    def read(name,key=None):
        try:
            d=json.loads((root/name).read_text(encoding='utf-8'))
            return d if key is None else d[key]
        except (OSError,ValueError,KeyError) as e:err(f'{name}: {e}');return {} if key is None else []
    def index(rows,kind):
        out={}
        if not isinstance(rows,list):err(kind+': expected list');return out
        for row in rows:
            if not isinstance(row,dict) or not isinstance(row.get('id'),str):err(kind+': missing ID');continue
            if row['id'] in out:err(kind+': duplicate '+row['id'])
            out[row['id']]=row
        return out
    si=index(read('spec_manifest.json','specs'),'spec');ti=index(read('task_manifest.json','tasks'),'task');gi=index(read('gate_manifest.json','gates'),'gate')
    def dag(items,kind):
        state={};order=[]
        def visit(x,stack):
            if state.get(x)==2:return
            if state.get(x)==1:err(kind+' dependency cycle: '+' -> '.join(stack+[x]));return
            state[x]=1
            for dep in items[x].get('depends_on',[]):
                if dep not in items:err(kind+' unknown dependency '+dep+' in '+x)
                else:visit(dep,stack+[x])
            state[x]=2;order.append(x)
        for x in items:visit(x,[])
        return order
    orders['specs']=dag(si,'spec');orders['tasks']=dag(ti,'task')
    actual_specs={p.relative_to(root).as_posix() for p in (root/'specs').glob('CX-*.md')}
    if actual_specs!={s.get('path') for s in si.values()}:err('Spec file inventory differs from manifest')
    reserved=read('spec_manifest.json').get('reserved_ids',[])
    if any(x.get('id') in si for x in reserved):err('Reserved spec ID also defined')
    declared={}
    for sid,s in si.items():
        p=root/s.get('path','')
        if not p.is_file():err(sid+': missing source');continue
        try:p.resolve().relative_to(root)
        except ValueError:err(sid+': path escapes root');continue
        text=p.read_text(encoding='utf-8')
        if s.get('status')!='Draft' or not re.search(r'^status: Draft$',text,re.M) or 'implementation_evidence: []' not in text:err(sid+': unsupported implementation status/evidence')
        if not re.search(r'^id: '+re.escape(sid)+r'$',text,re.M):err(sid+': metadata ID mismatch')
        m=re.search(r'^depends_on: (\[.*\])$',text,re.M)
        try:deps=json.loads(m.group(1)) if m else None
        except ValueError:deps=None
        if deps!=s.get('depends_on'):err(sid+': metadata dependencies differ')
        stage=re.search(r'^first_stage: (\S+)',text,re.M)
        if stage and stage.group(1)!=s.get('stage'):err(sid+': first_stage differs')
        for m in GATE_RE.finditer(text):
            gid,desc=m.groups()
            if gid in declared:err('Duplicate source gate '+gid)
            declared[gid]=(sid,s['path'],desc.strip())
    if set(declared)!=set(gi):err('Source gate IDs differ from manifest')
    for gid,g in gi.items():
        if g.get('product_result')!='NOT_RUN' or g.get('implementation_status')!='Not implemented in this package':err(gid+': product result/status is not proposed NOT_RUN')
        if gid in declared and declared[gid]!=(g.get('spec'),g.get('spec_path'),g.get('description')):err(gid+': source/manifest gate text mismatch')
    targeted=set()
    for tid,t in ti.items():
        if t.get('status')!='Not started':err(tid+': live implementation status asserted')
        if not t.get('gate_targets'):err(tid+': no acceptance gate')
        for sid in t.get('specs',[]):
            if sid not in si:err(tid+': unknown spec '+sid)
        for gid in t.get('gate_targets',[]):
            if gid not in gi:err(tid+': unknown gate '+gid)
            else:
                targeted.add(gid)
                if gi[gid]['spec'] not in t.get('specs',[]):err(tid+': gate owner missing from task specs '+gid)
    orphan=sorted(set(gi)-targeted)
    if orphan:err('Unowned gates: '+', '.join(orphan))
    # Release profiles make a bounded task closure explicit; optional hosted import must not sneak in.
    rp=read('release_profiles.json')
    if rp.get('default_active_runtime') is not False:err('Release profiles may not activate runtime by default')
    for pr in rp.get('profiles',[]):
        seen=set()
        def gather(x):
            if x in seen:return
            if x not in ti:err('Profile '+pr['id']+' unknown task '+x);return
            seen.add(x)
            for d in ti[x].get('depends_on',[]):gather(d)
        for x in pr.get('exit_tasks',[]):gather(x)
        profiles[pr['id']]=[x for x in orders['tasks'] if x in seen]
        if 'B157' in seen:err('Optional hosted importer is on release critical path '+pr['id'])
    # Every local document link resolves; fragments are displayed but not asserted validated.
    local_links=0;fences_checked=0;mds=sorted(root.rglob('*.md'))
    for p in mds:
        fence=None
        for n,line in enumerate(p.read_text(encoding='utf-8').splitlines(),1):
            fm=re.match(r'^\s*(`{3,}|~{3,})',line)
            if fm:
                mark=fm.group(1)
                if fence is None:fence=mark;fences_checked+=1
                elif mark[0]==fence[0] and len(mark)>=len(fence):fence=None
                continue
            if fence:continue
            for m in re.finditer(r'!?\[[^\]\n]*\]\(([^\s)]+)\)',line):
                link=m.group(1);u=urlsplit(link)
                if u.scheme or u.netloc or not u.path:continue
                path=(p.parent/unquote(u.path)).resolve()
                try:path.relative_to(root)
                except ValueError:err(f'{p.relative_to(root)}:{n}: link escapes root {link}');continue
                local_links+=1
                if not path.exists():err(f'{p.relative_to(root)}:{n}: missing link {link}')
        if fence:err(f'{p.relative_to(root)}: unclosed fenced block')
    try:
        for name,text in render_all(root).items():
            if not (root/name).exists() or (root/name).read_text(encoding='utf-8')!=text:err('Stale generated view: '+name)
    except (OSError,ValueError,KeyError,TypeError) as e:err('Cannot regenerate views: '+str(e))
    for p in sorted((root/'schemas/json').glob('*.schema.json')):
        try:Draft202012Validator.check_schema(json.loads(p.read_text()))
        except Exception as e:err('Invalid JSON schema '+p.name+': '+str(e))
    fixture={}
    try:
        validate_source((root/'fixtures/neural_programs/minimal-source.nps').read_bytes())
        fixture=validate_np(root/'fixtures/neural_programs/minimal.np')
        if fixture.get('model_executed') is not False or fixture.get('admitted') is not False:err('Fixture claims model/admission execution')
    except (OSError,ContractError) as e:err('Neural format fixture: '+str(e))
    for p in root.rglob('*'):
        if p.is_symlink():err('Symlink in package: '+str(p.relative_to(root)))
        if '__pycache__' in p.parts or p.suffix=='.pyc':err('Generated Python cache in release: '+str(p.relative_to(root)))
    # Complete inventory; outputs excluded by explicit policy to prevent self-hash cycles.
    hash_count=0
    if integrity:
        h=read(HASH_FILE)
        if set(h.get('excluded',[]))!=HASH_EXCLUDED:err('Unexpected checksum exclusions')
        recorded=h.get('files',{});actual=hash_inventory(root);hash_count=len(actual)
        if set(recorded)!=set(actual):err('Checksum file inventory mismatch')
        for name,d in actual.items():
            if recorded.get(name)!=d:err('Checksum mismatch: '+name)
    errors.extend(ace_checks(root))
    errors.extend(schema_reflex_checks(root))
    errors.extend(research_checks(root))
    errors.extend(closed_loop_checks(root))
    counts.update({'specs':len(si),'tasks':len(ti),'proposed_gates':len(gi),'markdown_files':len(mds),'local_links_checked':local_links,'fenced_blocks_checked':fences_checked,'orphan_gates':len(orphan),'hashed_files_checked':hash_count})
    return {'schema':'axon-document-validation/2','package_version':'0.22','generated_at':datetime.now(timezone.utc).isoformat(),'scope':'DOCUMENTATION_AND_INERT_FORMAT_ONLY','result':'PASS' if not errors else 'FAIL','counts':counts,'topological_orders':orders,'release_dependency_closures':profiles,'fixture_result':fixture,'integrity_checked':integrity,'local_link_fragments_checked':False,'product_gates_executed':0,'errors':errors}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--root',type=Path,default=Path(__file__).resolve().parents[1]);p.add_argument('--output',type=Path);p.add_argument('--refresh-hashes',action='store_true',help='Explicitly rebuild integrity inventory after reviewed changes; does not turn product gates green.');a=p.parse_args()
    if a.refresh_hashes:
        data={'schema':'axon-package-sha256/1','excluded':sorted(HASH_EXCLUDED),'files':hash_inventory(a.root)}
        (a.root/HASH_FILE).write_text(json.dumps(data,indent=2)+'\n')
    r=validate(a.root)
    if a.output:a.output.write_text(json.dumps(r,indent=2)+'\n')
    print(json.dumps(r,indent=2));return int(r['result']!='PASS')
if __name__=='__main__':raise SystemExit(main())
