#!/usr/bin/env python3
"""Validate this proposal and synthetic fixtures, never Axon product gates."""
from __future__ import annotations
import argparse
import hashlib
import json
import math
from pathlib import Path

class StrictJsonError(ValueError):
    pass

def strict_json(text: str):
    def pairs(items):
        out = {}
        for key, value in items:
            if key in out:
                raise StrictJsonError(f'duplicate key: {key}')
            out[key] = value
        return out
    def constant(value):
        raise StrictJsonError(f'non-finite constant: {value}')
    def walk(v):
        if isinstance(v, float):
            raise StrictJsonError('floating point is not admitted in ACF v1 fixtures')
        if isinstance(v, str):
            v.encode('utf-8', errors='strict')
        elif isinstance(v, list):
            for x in v: walk(x)
        elif isinstance(v, dict):
            for k,x in v.items(): walk(k); walk(x)
    obj = json.loads(text, object_pairs_hook=pairs, parse_constant=constant)
    walk(obj)
    return obj

def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--report', type=Path)
    args = ap.parse_args()
    root = Path(__file__).resolve().parents[1]
    try:
        from jsonschema import Draft202012Validator
    except ImportError:
        print('BLOCKED: jsonschema is required for proposal fixture validation; no product gates were executed.')
        return 2
    errors=[]; results=[]
    def load(path): return strict_json((root/path).read_text(encoding='utf-8'))
    tasks=load('manifests/tasks.json')['tasks']
    gates=load('manifests/gates.json')['gates']
    ti={t['id']:t for t in tasks};gi={g['id']:g for g in gates}
    if len(ti)!=len(tasks) or len(gi)!=len(gates):errors.append('Duplicate task/gate IDs')
    if len(tasks)!=14 or len(gates)!=55:errors.append('Unexpected task/gate count; review inventory change')
    if any(t['status']!='Not started' for t in tasks):errors.append('Proposal task cannot claim implementation')
    if any(g['product_result']!='NOT_RUN' or g['execution_evidence'] or g['implementation_test_path'] is not None for g in gates):errors.append('Proposal gate cannot invent product evidence')
    states={}
    def visit(ident):
        if states.get(ident)==1: errors.append(f'Task cycle at {ident}');return
        if states.get(ident)==2:return
        states[ident]=1
        for d in ti[ident]['depends_on']:
            if d not in ti:errors.append(f'Unknown dependency {d}')
            else:visit(d)
        states[ident]=2
    for t in tasks:
        visit(t['id'])
        if not t['gates']:errors.append(f'No gates for {t["id"]}')
        for g in t['gates']:
            if g not in gi or gi[g]['owner']!=t['id']:errors.append(f'Gate ownership mismatch {g}')
    for g in gates:
        if g['owner'] not in ti or g['id'] not in ti[g['owner']]['gates']:errors.append(f'Orphan gate {g["id"]}')
    schemas={}
    for p in sorted((root/'contracts').glob('*.schema.json')):
        schema=load(str(p.relative_to(root)));Draft202012Validator.check_schema(schema)
        schemas[str(p.relative_to(root))]=Draft202012Validator(schema)
    for case in load('manifests/examples.json')['examples']:
        why=[]
        try:
            data=load(case['path'])
            why=[str(e.message) for e in schemas[case['schema']].iter_errors(data)]
            valid=not why
        except (ValueError, UnicodeError) as e:
            valid=False;why=[str(e)]
        passed=valid==case['expected_valid']
        results.append({'case':case['path'],'expected_valid':case['expected_valid'],'observed_valid':valid,'result':'PASS' if passed else 'FAIL','diagnostics':why})
        if not passed:errors.append(f'Unexpected fixture result {case["path"]}')
    raw_cases=[
        ('escaped_duplicate',r'{"principal":1,"princip\u0061l":2}',False),
        ('nested_duplicate','{"a":{"x":1,"x":2}}',False),
        ('non_finite','{"a":NaN}',False),
        ('infinite_numeric','{"a":1e400}',False),
        ('unpaired_surrogate',r'{"a":"\ud800"}',False),
        ('valid_siblings','{"a":{"x":1},"b":{"x":2}}',True),
        ('words_in_strings','{"a":"NaN and Infinity are text"}',True),
    ]
    for name,text,expected in raw_cases:
        try:strict_json(text);valid=True
        except (ValueError,UnicodeError):valid=False
        passed=valid==expected
        results.append({'case':name,'expected_valid':expected,'observed_valid':valid,'result':'PASS' if passed else 'FAIL'})
        if not passed:errors.append('Unexpected strict JSON result '+name)
    # Canonical identity is intentionally separate from any production Axon hash.
    a=strict_json('{"b":2,"a":1}')
    b=strict_json('{"a":1,"b":2}')
    canonical=lambda x:json.dumps(x,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode('utf-8')
    if canonical(a)!=canonical(b):errors.append('Canonical key ordering failure')
    source=load('manifests/source_manifest.json')
    evidence=load('manifests/evidence_index.json')['evidence']
    for e in evidence:
        if source['files'].get(e['path'],{}).get('sha256')!=e['sha256']:errors.append('Evidence/source mismatch '+e['id'])
    required=['README.md','specs/ACF-01-AXON-COMPUTE-FABRIC.md','review/SOURCE_REVIEW.md','review/ADVERSARIAL_REVIEW.md','review/SOURCE_EVIDENCE.md','build/IMPLEMENTATION_PLAN.md','build/ACCEPTANCE_GATES.md','build/BOOTSTRAP_PROMPT.md','reports/source_checks.json']
    for rel in required:
        if not (root/rel).is_file():errors.append('Missing '+rel)
    spec=(root/'specs/ACF-01-AXON-COMPUTE-FABRIC.md').read_text()
    if 'implementation_evidence: []' not in spec or 'status: Draft' not in spec:errors.append('Spec proposal status missing')
    report={'schema':'acf-package-validation/1','result':'PASS' if not errors else 'FAIL','scope':'Proposal consistency and synthetic contract fixtures ONLY','counts':{'tasks':len(tasks),'proposed_product_gates':len(gates),'source_evidence_ranges':len(evidence),'json_schemas':len(schemas),'fixture_and_parser_cases':len(results),'canonical_identity_checks':1},'product_gates_executed':0,'rust_tests_executed':0,'backend_tests_executed':0,'errors':errors,'cases':results}
    if args.report:
        args.report.parent.mkdir(parents=True,exist_ok=True)
        args.report.write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k!='cases'},indent=2))
    return 1 if errors else 0

if __name__=='__main__':
    raise SystemExit(main())
