from pathlib import Path
import hashlib, json, zipfile, sys
ROOT=Path(__file__).resolve().parents[1]
def sha(p):
 h=hashlib.sha256();
 with p.open('rb') as f:
  for b in iter(lambda:f.read(1<<20),b''):h.update(b)
 return h.hexdigest()
def load(p): return json.loads((ROOT/p).read_text())
errors=[]
def err(x): errors.append(x)
try:
 m=load('MANIFEST.json'); pl=load('integration/V023_PARENT_LOCK.json'); fl=load('integration/V023_FORGE_INPUT_LOCK.json')
 if m.get('version')!='0.23' or m.get('runtime_qualified') is not False or m.get('product_gates_executed')!=0: err('manifest/version/runtime claim')
 if sha(ROOT/pl['path'])!=pl['sha256'] or sha(ROOT/fl['path'])!=fl['sha256']: err('pinned input hash mismatch')
 for path in [pl['path'],fl['path']]:
  with zipfile.ZipFile(ROOT/path) as z:
   if any(n.startswith('/') or '..' in Path(n).parts for n in z.namelist()): err('unsafe embedded zip member')
 # validate parent manifest prefix without extracting code
 with zipfile.ZipFile(ROOT/pl['path']) as z:
  base='axon-cortex-build-v0_22/'
  tasks=json.loads(z.read(base+'task_manifest.json'))['tasks']; gates=json.loads(z.read(base+'gate_manifest.json'))['gates']
  canon=lambda x:json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
  if len(tasks)!=pl['task_count'] or hashlib.sha256(canon(tasks)).hexdigest()!=pl['task_prefix_sha256']: err('parent task prefix drift')
  if len(gates)!=pl['gate_count'] or hashlib.sha256(canon(gates)).hexdigest()!=pl['gate_prefix_sha256']: err('parent gate prefix drift')
 # Forge package validator data-level checks
 with zipfile.ZipFile(ROOT/fl['path']) as z:
  names=set(z.namelist()); prefix='axon-forge-v0_3/'
  for req in ['MANIFEST.json','SPEC.md','OWNERSHIP.md','schemas/semantic-ir.schema.json','schemas/forge-experiment-profile.schema.json','schemas/resource-projection.schema.json','conformance/vectors/core.json']:
   if prefix+req not in names: err('forge missing '+req)
  fm=json.loads(z.read(prefix+'MANIFEST.json'))
  if fm.get('version')!='0.3.0' or 'experiment_identity_lineage_comparison' not in fm.get('delegated_authorities',{}): err('forge ownership normalization missing')
 # deltas
 wp=load('integration/V023_WORK_PACKAGES.json')['tasks']; gd=load('integration/V023_GATE_DELTA.json')['gates']; req=load('integration/V023_REQUIREMENTS.json')['requirements']
 if [x['id'] for x in wp] != [f'B{i}' for i in range(287,303)]: err('task delta IDs')
 if len(gd)!=31 or len({g['id'] for g in gd})!=31 or any('-r23-' not in g['id'] for g in gd): err('gate delta coverage')
 if len(req)!=16 or {r['task'] for r in req}!={x['id'] for x in wp}: err('requirement coverage')
 ptask={x['id'] for x in tasks}; dtask={x['id'] for x in wp}; alltask=ptask|dtask
 gateset={g['id'] for g in gd}
 used=set()
 for t in wp:
  if any(d not in alltask for d in t['depends_on']): err('unknown delta dependency '+t['id'])
  if any(g not in gateset for g in t['gate_targets']): err('unknown delta gate '+t['id'])
  used.update(t['gate_targets'])
 if used!=gateset: err('orphan delta gate')
 owner=load('integration/V023_OWNER_MAP.json')['rows']
 if any(r.get('new_authority') is not False for r in owner): err('duplicate authority')
 if len(owner)<13: err('owner coverage')
 cross=load('integration/V023_FORGE_CROSSWALK.json')['rows']
 if len(cross)<12: err('forge crosswalk coverage')
 q=load('integration/V023_RUNTIME_QUALIFICATION.json')
 for k in ['engineering_qualified','policy_activated','measured_improvement_supported','forge_runtime_executed','micode_roundtrip_executed','physical_backend_tests_executed']:
  if q.get(k) is not False: err('false runtime claim '+k)
 if set(q['required_delta_gates'])!=gateset: err('runtime gate closure')
 # json schemas parse
 for p in (ROOT/'schemas/json').glob('*.schema.json'): json.loads(p.read_text())
 # package hygiene
 for p in ROOT.rglob('*'):
  if p.is_symlink(): err('symlink in package '+str(p.relative_to(ROOT)))
  if '__pycache__' in p.parts or p.suffix=='.pyc': err('generated python cache in package '+str(p.relative_to(ROOT)))
 # required amendment coverage
 for cx in ['CX-00','CX-03','CX-11','CX-13','CX-15','CX-16','CX-17','CX-18','CX-19','CX-21','CX-24','CX-29','CX-30','CX-32','CX-33','CX-34']:
  if not any(p.name.startswith(cx+'-') for p in (ROOT/'amendments').glob('*.md')): err('missing amendment '+cx)
except Exception as e: err('exception: '+repr(e))
result={'schema':'axon-cortex-v023-validation/1','package_version':'0.23','scope':'DOCUMENTATION_AND_INTEGRATION_ONLY','result':'PASS' if not errors else 'FAIL','parent_tasks':287,'parent_gates':504,'delta_tasks':16,'delta_gates':31,'composed_tasks':303,'composed_gates':535,'product_gates_executed':0,'errors':errors}
print(json.dumps(result,indent=2))
raise SystemExit(0 if not errors else 1)
