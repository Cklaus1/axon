"""0.22 preservation, coverage and default-off checks. No source/product execution."""
from __future__ import annotations
import hashlib
import json
from pathlib import Path


def checks(root: Path) -> list[str]:
    errors = []
    def err(text): errors.append('R22: ' + text)
    def read(path): return json.loads((root/path).read_text())
    def digest(path): return hashlib.sha256((root/path).read_bytes()).hexdigest()
    def canonical(x): return json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()
    try:
        export=read('integration/V022_OWNER_EXPORT_LOCK.json')
        if export.get('package_version')!='0.22' or export.get('status')!='PROPOSED_NOT_RUNTIME_QUALIFIED':
            err('invalid current owner export status/version')
        export_paths=set()
        for row in export['files']:
            path=row['path']
            if path in export_paths:err('duplicate owner export '+path)
            export_paths.add(path)
            if digest(path)!=row['sha256']:err('owner export drift: '+path)
        required_exports={'task_manifest.json','gate_manifest.json','spec_manifest.json','release_profiles.json',
                          'schemas/CLOSED_LOOP_PROFILE_V022.md','integration/MICODE_V022_CONSUMER_DELTA.md',
                          'integration/FABRIC_V022_INTEGRATION.md'}
        required_exports|={p.relative_to(root).as_posix() for p in (root/'schemas/json').glob('closed-loop-*.schema.json')}
        if not required_exports<=export_paths:err('incomplete owner export')
        specs=read('spec_manifest.json'); tasks=read('task_manifest.json'); gates=read('gate_manifest.json')
        for item in [specs,tasks,gates,read('release_profiles.json')]:
            if item.get('package_version')!='0.22':err('wrong active package version')
        td={r['id']:r for r in tasks['tasks']}; gd={r['id']:r for r in gates['gates']}
        lock=read('integration/V022_PARENT_LOCK.json')
        for kind,records,current in [('task',lock['tasks'],td),('gate',lock['gates'],gd)]:
            for row in records:
                if row['id'] not in current or hashlib.sha256(canonical(current[row['id']])).hexdigest()!=row['sha256']:
                    err('parent '+kind+' changed: '+row['id'])
        if {s['id'] for s in specs['specs']}!=set(lock['spec_ids']):err('parent spec owner IDs changed')
        for f in lock['protected_files']:
            if digest(f['path'])!=f['sha256']:err('protected bytes changed: '+f['path'])
        reference=root/'integration/compute-fabric-v0_1-reference'
        actual={p.relative_to(reference).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in reference.rglob('*') if p.is_file()}
        if actual!=lock['fabric_files']:err('Fabric reference bytes/inventory changed')
        expected_tasks={f'B{i}' for i in range(255,287)}
        new={r['id']:r for r in read('integration/V022_WORK_PACKAGES.json')['tasks']}
        reqs=read('integration/V022_REQUIREMENTS.json')['requirements']
        if set(new)!=expected_tasks or len(reqs)!=32 or {r['task'] for r in reqs}!=expected_tasks:err('new task requirement coverage')
        if len({r['id'] for r in reqs})!=32:err('duplicate requirement ID')
        newg=set()
        for tid,row in new.items():
            if tid not in td:err('missing task '+tid);continue
            for key in ['title','depends_on','specs','output','gate_targets','optional']:
                if row[key]!=td[tid][key]:err('work package differs: '+tid+'/'+key)
            newg.update(row['gate_targets'])
        for row in reqs:
            if row['runtime_status']!='NOT_RUN':err('unexecuted integration marked complete')
            if row['task'] in td and (row['gates']!=td[row['task']]['gate_targets'] or row['owners']!=td[row['task']]['specs']):err('requirement backlinks')
        if len(newg)!=80 or newg!={g for g in gd if '-r22-' in g}:err('new gate coverage')
        if any(g not in gd for g in newg):err('unknown integration gate')
        cross=read('integration/V022_FABRIC_CROSSWALK.json')
        aft={t['id']:t for t in json.loads((reference/'manifests/tasks.json').read_text())['tasks']}
        afg={g['id']:g for g in json.loads((reference/'manifests/gates.json').read_text())['gates']}
        if len(cross['rows'])!=14 or {x['fabric_task'] for x in cross['rows']}!=set(aft):err('Fabric task coverage')
        required=[];deferred=[]
        for row in cross['rows']:
            a=aft[row['fabric_task']]
            if row['fabric_gates']!=a['gates'] or row['milestone']!=a['milestone']:err('Fabric task semantics altered')
            if a['milestone'] in ['M0','M1','M2']:
                if row['master_task'] not in expected_tasks or row['disposition']!='REQUIRED_M0_M2':err('required Fabric work deferred')
                required+=a['gates']
            else:
                if row['master_task'] is not None or row['disposition']!='DEFERRED_EXTENSION':err('extension accidentally mandatory')
                deferred+=a['gates']
        if len(required)!=38 or set(required)!=set(cross['required_gates']):err('Fabric required gate scope')
        if len(deferred)!=17 or set(deferred)!=set(cross['deferred_gates']):err('Fabric extension scope')
        if any(g['product_result']!='NOT_RUN' for g in afg.values()):err('Fabric runtime claims in proposal')
        def closure(tid,active=None,seen=None):
            active=set() if active is None else active; seen=set() if seen is None else seen
            if tid in active:raise ValueError('cyclic integration tasks')
            if tid not in td:raise ValueError('unknown task '+tid)
            if tid in seen:return seen
            active.add(tid)
            for dep in td[tid]['depends_on']:closure(dep,active,seen)
            active.remove(tid);seen.add(tid);return seen
        mapped={r['fabric_task']:r['master_task'] for r in cross['rows'] if r['master_task']}
        for fid,tid in mapped.items():
            for dep in aft[fid]['depends_on']:
                if mapped[dep] not in closure(tid):err('Fabric dependency lost '+fid+'/'+dep)
        profiles={p['id']:p for p in read('release_profiles.json')['profiles']}
        expected_profiles={'v022_package_conformance','v022_protected_execution','v022_peer_roundtrip','v022_operational_closed_loop','v022_improvement_study'}
        if not expected_profiles<=profiles.keys():err('missing release slices')
        for pid in expected_profiles:
            seen=set()
            for t in profiles[pid]['exit_tasks']:seen|=closure(t)
            if any(int(t[1:])<255 for t in seen):err('all-provider/historical backlog reintroduced into '+pid)
            if pid!='v022_improvement_study' and any(td[t]['optional'] for t in seen):err('optional research on bounded critical path')
        if not td['B286']['optional']:err('measured study forced into bounded release')
        legacy=read('integration/V022_LEGACY_GATE_CLOSURE.json')['rows']
        legacyg=[]
        for row in legacy:
            if row['task'] not in new:err('legacy obligation missing integration owner')
            for g in row['gates']:
                if g not in gd or '-r22-' in g:err('invalid legacy gate mapping')
                legacyg.append(g)
        if len(legacyg)!=40 or len(set(legacyg))!=40:err('legacy behavior closure altered')
        q=read('integration/V022_RUNTIME_QUALIFICATION.json')
        for field in ['engineering_qualified','policy_activated','measured_improvement_supported','source_tests_executed','physical_backend_tests_executed','peer_roundtrip_executed','default_active_runtime']:
            if q.get(field) is not False:err('unexecuted runtime claim '+field)
        if q['live_evidence'] or q['qualified_profiles']:err('fabricated runtime evidence/profile')
        if set(q['required_fabric_gates'])!=set(required) or set(q['required_legacy_gates'])!=set(legacyg):err('qualification gate closure mismatch')
        mandatory_new={g for t in new.values() if not t['optional'] for g in t['gate_targets']}
        if set(q['required_master_gates'])!=mandatory_new:err('new gate qualification closure mismatch')
        il=read('integration/V022_INPUT_LOCK.json')
        if il['micode_source_supplied'] is not True or il['source_files_modified'] is not False or il['runtime_tests_executed'] is not False:err('input/status claim mismatch')
        inventory={origin:{x['path']:x['sha256'] for x in read('integration/source-inventory-'+origin+'_source.json')} for origin in ['axon','micode']}
        for e in read('review/SOURCE_EVIDENCE_V022.json')['evidence']:
            if e['start_line']<1 or e['end_line']<e['start_line']:err('invalid source excerpt range')
            if e['origin'] in inventory and inventory[e['origin']].get(e['path'])!=e['sha256']:err('source excerpt hash not pinned')
        b=read('fixtures/closed_loop_v022/bundle.json')
        if b.get('fixture_only') is not True or b.get('product_gates_executed')!=0 or b.get('source_runtime_executed') is not False:err('fixture misrepresented')
        plan=read('fixtures/closed_loop_v022/pilot-template.json')
        if plan['runtime_ready'] or plan['deployment_enabled'] or plan['operator_approved'] or plan['live_evidence']:err('template activated')
        files=['build/CLOSED_LOOP_INTEGRATION_V022.md','build/WORK_PACKAGES_V022.md','build/PILOT_PLAN_V022.md',
               'build/UPGRADE_V022.md','build/BOOTSTRAP_PROMPT_V022.md','build/RUNTIME_EVIDENCE_V022.md',
               'integration/MICODE_V022_CONSUMER_DELTA.md','integration/FABRIC_V022_INTEGRATION.md',
               'schemas/CLOSED_LOOP_PROFILE_V022.md','tools/closed_loop_reference.py','tools/plan_upgrade_v022.py',
               'tools/check_v022_runtime_evidence.py','tools/repo_gate_v022.py','review/REVIEW_V022.md']
        for path in files:
            if not (root/path).is_file():err('missing integration delivery '+path)
    except (OSError,KeyError,TypeError,ValueError,RecursionError) as exc:
        err('invalid integration metadata: '+str(exc))
    return errors
