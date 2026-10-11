"""Check package contracts and examples offline. This is not a production gate."""
from __future__ import annotations

import argparse
import csv
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from harness.contracts import Schemas, bundle, lint_bundle, load  # noqa: E402
from harness.simulate import demonstrate  # noqa: E402


def check() -> dict:
    schemas = Schemas(ROOT)
    valid = []
    for name in ('document', 'coding', 'research'):
        findings = lint_bundle(*bundle(name, ROOT), schemas=schemas)
        if findings:
            raise AssertionError(f'{name}: {findings}')
        valid.append(name)
    negative = []
    _, registry, contract, config, intent = bundle('document', ROOT)
    for case in load(ROOT / 'examples/invalid_cases.json'):
        findings = lint_bundle(load(ROOT / case['path']), registry, contract, config, intent, schemas)
        codes = sorted({f.code for f in findings})
        if case['expected_code'] not in codes:
            raise AssertionError(f"{case['path']}: expected {case['expected_code']}, got {codes}")
        negative.append({'path': case['path'], 'expected_code': case['expected_code'], 'observed_codes': codes})
    typed_examples = [
        ('evaluation_result', 'evaluation-result'), ('experience', 'experience'),
        ('event', 'event'), ('repair_request', 'repair'),
        ('owner_profile', 'owner-profile'), ('strategy_artifact', 'strategy-artifact'),
        ('experiment_profile', 'experiment-profile'), ('evidence_projection', 'evidence-projection'),
        ('resource_projection', 'resource-projection'), ('forge_binding', 'forge-binding'),
        ('compilation_record', 'compilation-record'),
    ]
    for name, kind in typed_examples:
        schemas.require(kind, load(ROOT / f'examples/{name}.json'))
    # All checked-in JSON must parse strictly, including intentionally schema-invalid plans.
    json_files = [p for p in ROOT.rglob('*.json') if not any(part in {'.venv', '__pycache__', '.git', 'local-runs'} for part in p.relative_to(ROOT).parts)]
    for path in json_files:
        load(path)
    # No undeclared remote schema resources; $schema is a dialect identifier, not a fetch instruction.
    known_ids = {doc['$id'] for doc in schemas.documents.values()}
    for doc in schemas.documents.values():
        stack = [doc]
        while stack:
            value = stack.pop()
            if isinstance(value, dict):
                if '$ref' in value:
                    target = value['$ref'].split('#', 1)[0]
                    if target and target not in known_ids:
                        raise AssertionError(f'Unregistered schema reference: {target}')
                stack.extend(value.values())
            elif isinstance(value, list):
                stack.extend(value)
    required = ['README.md', 'SPEC.md', 'AGENTS.md', 'PACKAGE_STATUS.md', 'SOURCES.md',
                'build/BUILD_PLAN.md', 'build/ACCEPTANCE_GATES.md', 'build/TEST_MATRIX.md',
                'build/FIRST_TASK.md', 'build/BACKLOG.json', 'build/TRACEABILITY.csv',
                'docs/CORTEX_ALIGNMENT.md', 'docs/FORGE_SOFTWARE_SEMANTICS.md',
                'docs/STRATEGY_EVIDENCE_EXPERIMENTS.md', 'integration/CORTEX_V023_OWNER_PROFILE.json',
                'integration/CORTEX_V023_INPUT_LOCK.json', 'integration/FORGE_V03_INPUT_LOCK.json',
                'integration/CORTEX_V023_REQUIREMENT_CROSSWALK.json']
    for path in required:
        if not (ROOT / path).is_file():
            raise AssertionError(f'Missing required handoff file: {path}')
    spec_ids = set(re.findall(r'\*\*([A-Z]+-\d{3})\.', (ROOT / 'SPEC.md').read_text()))
    with (ROOT / 'build/TRACEABILITY.csv').open(newline='') as stream:
        rows = list(csv.DictReader(stream))
    if {row['requirement_id'] for row in rows} != spec_ids:
        raise AssertionError('Traceability requirements do not match the specification')
    matrix = (ROOT / 'build/TEST_MATRIX.md').read_text()
    for row in rows:
        if not (ROOT / row['detail_document']).is_file() or row['test_family'] not in matrix:
            raise AssertionError(f'Invalid traceability target: {row}')
    backlog = load(ROOT / 'build/BACKLOG.json')['tasks']
    ids = {task['id'] for task in backlog}
    if len(ids) != len(backlog):
        raise AssertionError('Duplicate backlog identity')
    for task in backlog:
        if not set(task['dependencies']) <= ids or task['primary_requirement'] not in spec_ids:
            raise AssertionError(f'Unresolved backlog dependency/requirement: {task}')
    demo = demonstrate()
    if demo['original_accepted'] or not demo['repaired_accepted'] or demo['reused_nodes'] != ['normalize']:
        raise AssertionError('Synthetic repair demonstration failed')
    return {'status': 'pass', 'scope': 'offline_contract_and_fixture_checks_only',
            'schema_count': len(schemas.documents), 'valid_bundles': valid,
            'invalid_fixture_count': len(negative), 'invalid_fixture_results': negative,
            'strict_json_file_count': len(json_files), 'requirement_count': len(spec_ids),
            'backlog_task_count': len(backlog), 'synthetic_demo': {
                'original_accepted': demo['original_accepted'], 'repaired_accepted': demo['repaired_accepted'],
                'reused_nodes': demo['reused_nodes'], 'live_provider_calls': 0, 'external_effects': 0},
            'not_tested': ['real compiler/provider adapters', 'durable/parallel runtime', 'policy enforcement',
                           'live external effects/reconciliation', 'model calibration', 'live Cortex IntentIR/AIR/UEG/experiment/resource adapters',
                           'live Forge SIR/build execution', 'live MiCode workflow integration',
                           'quality or cost improvements on real tasks']}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--json', action='store_true', help='Print machine-readable check report')
    args = parser.parse_args()
    try:
        report = check()
        if args.json:
            print(json.dumps(report, indent=2))
        else:
            print(f"PASS: {report['schema_count']} schemas, {len(report['valid_bundles'])} valid bundles, "
                  f"{report['invalid_fixture_count']} adversarial fixtures, {report['requirement_count']} mapped requirements.")
            print('Synthetic repair: incomplete output rejected; repaired output accepted; normalize reused.')
            print('Scope: offline conformance only; no production/runtime/model-performance claim.')
        return 0
    except Exception as exc:
        print(f'FAIL: {type(exc).__name__}: {exc}', file=sys.stderr)
        return 1


if __name__ == '__main__':
    raise SystemExit(main())
