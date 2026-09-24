#!/usr/bin/env python3
"""Check structural completeness, NEVER authenticate/qualify runtime evidence.
Exit 2: NOT_QUALIFIED. Exit 3: complete but REQUIRES_EXTERNAL_VERIFICATION.
There is deliberately no success code that grants production qualification.
"""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import sys
sys.dont_write_bytecode=True
from closed_loop_reference import strict_json,validate,Refusal
ROOT=Path(__file__).resolve().parents[1]


def inspect(evidence: dict, root: Path=ROOT) -> dict:
    validate('evidence',evidence)
    qualification=json.loads((root/'integration/V022_RUNTIME_QUALIFICATION.json').read_text())
    required=set(qualification['required_master_gates']+qualification['required_fabric_gates']+qualification['required_legacy_gates'])
    records={};errors=[]
    for row in evidence['records']:
        gid=row['gate_id']
        if gid in records:errors.append('duplicate gate: '+gid)
        if gid not in required:errors.append('unknown/out-of-scope gate: '+gid)
        records[gid]=row
    passing={gid for gid,r in records.items() if r['result']=='PASS' and not r['synthetic']}
    missing=sorted(required-passing)
    status='NOT_QUALIFIED' if errors or missing else 'REQUIRES_EXTERNAL_VERIFICATION'
    return {'schema':'axon.runtime-evidence-completeness/1','package_version':'0.22','result':status,
            'scope':'STRUCTURAL_COMPLETENESS_ONLY; does not authenticate issuers or prove tests ran',
            'required_gate_count':len(required),'submitted_gate_count':len(records),
            'missing_or_nonlive_passing_gates':missing,'errors':errors,
            'engineering_qualified':False,'policy_activated':False,'measured_improvement_supported':False,
            'signatures_or_sources_authenticated':False,'product_gates_executed':0}


def main() -> int:
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--evidence',required=True,type=Path);a=p.parse_args()
    try:
        if a.evidence.is_symlink():raise Refusal('evidence path is a symlink; review explicitly')
        if a.evidence.stat().st_size>1_048_576:raise Refusal('evidence size limit')
        result=inspect(strict_json(a.evidence.read_bytes()))
        print(json.dumps(result,indent=2));return 2 if result['result']=='NOT_QUALIFIED' else 3
    except (OSError,ValueError,KeyError,TypeError) as exc:
        print(json.dumps({'result':'NOT_QUALIFIED','error':str(exc),'engineering_qualified':False}));return 2

if __name__=='__main__':raise SystemExit(main())
