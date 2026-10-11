from pathlib import Path
import json, sys
ROOT=Path(__file__).resolve().parents[1]
required=[
 'README.md','SPEC.md','OWNERSHIP.md','MANIFEST.json','CHANGELOG_v0_3.md',
 'architecture/semantic-ir.md','architecture/meta-api.md','architecture/transformation-contract.md','architecture/experiment-protocol.md','architecture/provenance.md','architecture/resource-model.md',
 'schemas/semantic-ir.schema.json','schemas/transform.schema.json','schemas/pipeline.schema.json','schemas/meta-execution.schema.json','schemas/migration.schema.json','schemas/diagnostic.schema.json','schemas/bootstrap-pack.schema.json','schemas/backend.schema.json','schemas/capability-request.schema.json','schemas/semantic-diff.schema.json','schemas/build-program.schema.json','schemas/forge-experiment-profile.schema.json','schemas/forge-evidence-payload.schema.json','schemas/provenance-projection.schema.json','schemas/resource-projection.schema.json','schemas/forge-strategy-profile.schema.json','schemas/evaluation-profile.schema.json',
 'interfaces/axon-forge-api.md','interfaces/transform-api.md','interfaces/meta-api.md','interfaces/build-program-api.md','interfaces/truth-resolver-api.md',
 'conformance/ACCEPTANCE_CHECKLIST.md','conformance/vectors/core.json','integration/axon.md','integration/cortex.md','integration/micode.md','integration/compute-fabric.md','reference/minimal-reference-implementation/reference.py']
missing=[p for p in required if not (ROOT/p).is_file()]
if missing:
 print('MISSING',*missing,sep='\n'); raise SystemExit(1)
for p in (ROOT/'schemas').glob('*.schema.json'):
 d=json.loads(p.read_text());
 if '/v0.3/' not in d.get('$id',''): raise SystemExit(f'wrong schema id: {p.name}')
m=json.loads((ROOT/'MANIFEST.json').read_text())
if m.get('version')!='0.3.0': raise SystemExit('wrong manifest version')
for forbidden in ['experiment.schema.json','evidence.schema.json','provenance.schema.json','resource-graph.schema.json','strategy.schema.json']:
 if (ROOT/'schemas'/forbidden).exists(): raise SystemExit('superseded generic authority schema remains normative: '+forbidden)
v=json.loads((ROOT/'conformance/vectors/core.json').read_text())
if len(v)<15 or len({x['id'] for x in v})!=len(v): raise SystemExit('conformance vector coverage')
print(json.dumps({'result':'PASS','version':'0.3.0','required_paths':len(required),'schemas':len(list((ROOT/'schemas').glob('*.schema.json'))),'vectors':len(v)}))
