#!/usr/bin/env python3
"""Explicit trusted-package CI wrapper; no runtime gate claims or repository writes.
Checksum integrity is checked BEFORE optional package tests. Authenticate the
received package independently before executing any tool included in it.
"""
from __future__ import annotations
import argparse
import json
from pathlib import Path
import subprocess
import sys
sys.dont_write_bytecode = True
ROOT = Path(__file__).resolve().parents[1]
from validate_package import validate


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--run-package-tests', action='store_true',
                        help='Explicitly execute this trusted pack unit suite; never source/runtime gates')
    args = parser.parse_args()
    report = validate(ROOT)
    result = {'schema': 'axon.repo-package-gate/1', 'scope': 'DOCUMENTATION_AND_MODEL_FREE_REFERENCE_ONLY',
              'package_version': '0.22', 'validation': report,
              'unit_tests': 'NOT_RUN', 'product_gates_executed': 0,
              'source_runtime_result': 'NOT_RUN'}
    if report['result'] != 'PASS':
        print(json.dumps(result, indent=2)); return 1
    if args.run_package_tests:
        proc = subprocess.run([sys.executable, '-B', '-m', 'unittest', 'discover', '-s', 'tests', '-v'],
                              cwd=ROOT, text=True, capture_output=True, check=False)
        result['unit_tests'] = {'exit_code': proc.returncode, 'scope': 'MODEL_FREE_REFERENCE_ONLY',
                                'stdout': proc.stdout, 'stderr': proc.stderr}
        print(json.dumps(result, indent=2)); return int(proc.returncode != 0)
    print(json.dumps(result, indent=2)); return 0

if __name__ == '__main__':
    raise SystemExit(main())
