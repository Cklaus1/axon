"""v0.21 crosswalk/immutability checks. No product or model execution."""
from __future__ import annotations
import hashlib
import json
from pathlib import Path
from neural_contract_reference import canonical


def checks(root: Path) -> list[str]:
    errors = []
    def err(message): errors.append('R21: ' + message)
    def load(name): return json.loads((root / name).read_text())
    try:
        sources = load('integration/RESEARCH_SOURCES.json')
        sd = {s['id']: s for s in sources['sources']}
        matrix = load('integration/RESEARCH_REQUIREMENTS.json')
        reqs = matrix['requirements']
        td = {t['id']: t for t in load('task_manifest.json')['tasks']}
        gd = {g['id']: g for g in load('gate_manifest.json')['gates']}
        specd = {s['id']: s for s in load('spec_manifest.json')['specs']}
        if len(reqs) != 8 or {r['id'] for r in reqs} != {f'R21-{i:02d}' for i in range(1, 9)}:
            err('eight-family requirement coverage mismatch')
        covered_tasks = set(matrix['cross_cutting_tasks'])
        for r in reqs:
            if r['runtime_implementation'] != 'NOT_ESTABLISHED_BY_PACKAGE':
                err('false research runtime completion')
            if not r['tasks'] or not r['gates'] or not r['sources']:
                err('empty research mapping ' + r['id'])
            if any(s not in sd for s in r['sources']):
                err('unknown research source')
            actual = set()
            for task in r['tasks']:
                if task not in td: err('unknown research task ' + task); continue
                covered_tasks.add(task)
                actual.update(td[task]['gate_targets'])
            if actual != set(r['gates']): err('research gate backlinks differ ' + r['id'])
        if covered_tasks != {f'B{i}' for i in range(223, 255)}:
            err('new task crosswalk coverage mismatch')
        mapping = load('integration/SOURCE_OWNER_MAP.json')
        expected = {'EVO':'CX-29','DEC':'CX-05','EVL':'CX-01','RTR':'CX-06','CVM':'CX-28','SPX':'CX-26','TEL':'CX-10'}
        if len(mapping['aliases']) != 7 or {r['alias']:r['primary_owner'] for r in mapping['aliases']} != expected:
            err('existing-owner alias map mismatch')
        for row in mapping['aliases']:
            if row['new_authority'] is not False: err('duplicate authority')
            if any(s not in specd for s in [row['primary_owner']] + row['supporting_owners']):
                err('alias unknown owner')
        if 'CX-35' in specd or mapping['cx35']['migration'] != 'PRESERVE_SOURCE_ID_AND_RECORD_ALIAS; no automatic renumbering':
            err('live CX-35 reconciliation drift')
        if mapping['versions']['micode_consumer_target'] != '0.15 PROPOSED_NOT_SUPPLIED':
            err('unreviewed peer claimed delivered')
        original = load('review/baseline_contract_ids.json')
        for kind, rows, current in [('task', original['tasks'], td), ('gate', original['gates'], gd)]:
            for row in rows:
                value = current.get(row['id'])
                if value is None or hashlib.sha256(canonical(value)).hexdigest() != row['sha256']:
                    err('changed original ' + kind + ' ' + row['id'])
        if set(specd) != set(original['spec_ids']): err('original spec IDs not preserved')
        lock = load('integration/V021_INPUT_LOCK.json')
        if lock['repo_changes_performed'] is not False or lock['micode_source_supplied'] is not False:
            err('unsupported source/peer claim')
        for row in lock['protected_files']:
            if hashlib.sha256((root / row['path']).read_bytes()).hexdigest() != row['sha256']:
                err('protected contract changed ' + row['path'])
        if sources['replication_status'] != 'NO_MODEL_OR_PAPER_BENCHMARKS_EXECUTED':
            err('unsupported replication claim')
        new_optional = {f'B{i}' for i in range(251, 255)}
        if set(matrix['optional_tasks']) != new_optional: err('optional task list drift')
        if any(not td[t]['optional'] for t in new_optional): err('research task no longer optional')
        profiles = load('release_profiles.json')['profiles']
        for profile in profiles:
            if not profile['id'].startswith('v021_'): continue
            seen, active = set(), set()
            def walk(task):
                if task in active: raise ValueError('task cycle')
                if task in seen: return
                active.add(task)
                for dependency in td[task]['depends_on']: walk(dependency)
                active.remove(task); seen.add(task)
            for t in profile['exit_tasks']: walk(t)
            if any(td[t]['optional'] for t in seen): err('optional task on bounded closure ' + profile['id'])
        fixture = load('fixtures/research_v021/decision.json')
        if fixture['fixture_only'] is not True or fixture['product_result'] != 'NOT_RUN':
            err('fixture falsely claims runtime')
        plan = load('fixtures/research_v021/experiment-plan.json')
        if plan['deployment_enabled'] is not False or plan['live_evidence'] != [] or plan['product_result'] != 'NOT_RUN':
            err('unexecuted experiment activated')
        files = ['build/RESEARCH_INTEGRATION_V021.md','build/EVO_REGULARIZED_EVOLUTION.md',
                 'build/DEC_RESEARCH_PROVIDERS.md','build/EVL_CHECKLIST_EVIDENCE.md',
                 'build/RTR_MODEL_ROLE_ROUTING.md','build/CVM_CANONICAL_PROJECTIONS.md',
                 'build/SPX_BOUNDED_CASCADES.md','build/TEL_WHOLE_TASK_ECONOMICS.md',
                 'review/SOURCE_REVIEW_V021.md','build/UPGRADE_V021.md',
                 'tools/research_reference.py','tools/plan_upgrade.py','tools/repo_gate_v021.py',
                 'integration/MICODE_V015_PROPOSED_DELTA.md']
        for f in files:
            if not (root / f).is_file(): err('missing research profile ' + f)
    except (OSError, ValueError, KeyError, TypeError, RecursionError) as exc:
        err('invalid research metadata: ' + str(exc))
    return errors
