"""Additional v0.20 documentation/reference coverage checks, not product gates."""
from __future__ import annotations
import json
from pathlib import Path

def checks(root:Path)->list[str]:
    errors=[]
    def err(s):errors.append('SCHEMA-REFLEX: '+s)
    def load(p):return json.loads((root/p).read_text())
    try:
        cw=load('integration/SCHEMA_REFLEX_REQUIREMENTS.json')
        if (cw.get('axon_document_version'),cw.get('micode_document_version'))!=('0.20','0.14'):err('cross-project version mismatch')
        rows=cw['requirements']
        if len(rows)!=20 or {r['id'] for r in rows}!={f'SR{i:02}' for i in range(1,21)}:err('requirement coverage mismatch')
        td={t['id']:t for t in load('task_manifest.json')['tasks']};gd={g['id']:g for g in load('gate_manifest.json')['gates']};sd={s['id']:s for s in load('spec_manifest.json')['specs']}
        targeted=set()
        for row in rows:
            if row.get('product_result')!='NOT_RUN' or row.get('status')!='PROPOSED':err('false requirement completion')
            part=row['axon']
            if not all(part.get(k) for k in ['specs','tasks','gates']):err('unowned requirement '+row['id'])
            for sid in part['specs']:
                if sid not in sd:err('unknown spec '+sid)
            for gid in part['gates']:
                if gid not in gd:err('unknown gate '+gid);continue
                targeted.add(gid)
                if gd[gid]['spec'] not in part['specs']:err('gate owner mismatch '+gid)
                if not any(gid in td.get(t,{}).get('gate_targets',[]) for t in part['tasks']):err('missing task backlink '+gid)
            for tid in part['tasks']:
                if tid not in td:err('unknown task '+tid)
        new_gates={g for t in td.values() if 210<=int(t['id'][1:])<=222 for g in t['gate_targets']}
        if targeted!=new_gates:err('new gate coverage differs')
        profiles={p['id']:p for p in load('release_profiles.json')['profiles']}
        for pid in ['schema_decision_contract','lightweight_reflex_shadow','lightweight_reflex_runtime','schema_reflex_micode','v020_package_conformance']:
            seen=set()
            def walk(t):
                if t in seen:return
                seen.add(t)
                for dep in td[t]['depends_on']:walk(dep)
            for t in profiles[pid]['exit_tasks']:walk(t)
            if any(td[t].get('optional') for t in seen):err('optional experiment on new critical path '+pid)
        for tid in ['B221','B222']:
            if td[tid].get('optional') is not True:err('extension not optional '+tid)
        for rel in ['schemas/SCHEMA_DECISION_PROFILE.md','schemas/LIGHTWEIGHT_REFLEX_PROFILE.md','build/SCHEMA_DECISION_FRONTEND.md','build/LIGHTWEIGHT_REFLEX_DISTILLATION.md','tests/test_schema_reflex_reference.py','review/JIMOTHY_WEBMCP_SOURCES.json']:
            if not (root/rel).is_file():err('missing profile/reference '+rel)
        fixture=load('fixtures/schema_reflex/records.json')
        if fixture.get('fixture_only') is not True or fixture.get('product_status')!='NOT_RUN' or fixture['expected'].get('executed') is not False or fixture['expected'].get('verified_complete') is not False:err('fixture misrepresented as live evidence')
        template=load('fixtures/schema_reflex/experiment-template.json')
        if template.get('deployment_enabled') is not False or template.get('live_evidence') is not False or template.get('product_result')!='NOT_RUN':err('draft experiment falsely activated')
        export=load('integration/ACE_OWNER_EXPORT_LOCK.json')
        mandatory={'schemas/SCHEMA_DECISION_PROFILE.md','schemas/LIGHTWEIGHT_REFLEX_PROFILE.md','tools/schema_reflex_reference.py','fixtures/schema_reflex/records.json','integration/SCHEMA_REFLEX_REQUIREMENTS.json','task_manifest.json','gate_manifest.json'}
        if export.get('document_version')!=load('spec_manifest.json').get('package_version') or not mandatory <= {r['path'] for r in export['files']}:err('owner export missing current profile')
    except (OSError,KeyError,ValueError,TypeError,RecursionError) as e:err('invalid coverage metadata: '+str(e))
    return errors
