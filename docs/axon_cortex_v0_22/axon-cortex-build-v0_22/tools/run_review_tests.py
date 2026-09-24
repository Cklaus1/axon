#!/usr/bin/env python3
"""Execute local documentation and inert format tests; write exact scope-aware result evidence."""
from __future__ import annotations
import hashlib, io, json, sys, unittest
from datetime import datetime, timezone
from pathlib import Path
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[1]
class TrackingResult(unittest.TextTestResult):
    def __init__(self,*args,**kwargs):super().__init__(*args,**kwargs);self.cases=[]
    def addSuccess(self,test):super().addSuccess(test);self.cases.append({'test':test.id(),'result':'PASS'})
    def addFailure(self,test,err):super().addFailure(test,err);self.cases.append({'test':test.id(),'result':'FAIL'})
    def addError(self,test,err):super().addError(test,err);self.cases.append({'test':test.id(),'result':'ERROR'})
    def addSkip(self,test,reason):super().addSkip(test,reason);self.cases.append({'test':test.id(),'result':'SKIPPED','reason':reason})
stream=io.StringIO();suite=unittest.defaultTestLoader.discover(str(ROOT/'tests'))
r=unittest.TextTestRunner(stream=stream,verbosity=2,resultclass=TrackingResult).run(suite)
log=stream.getvalue();(ROOT/'review/TEST_LOG.txt').write_text(log)
report={'schema':'axon-review-tests/1','run_at':datetime.now(timezone.utc).isoformat(),'scope':'DOCUMENTATION_AND_INERT_FORMAT_ONLY','result':'PASS' if r.wasSuccessful() else 'FAIL','tests_run':r.testsRun,'failures':len(r.failures),'errors':len(r.errors),'skipped':len(r.skipped),'product_gates_executed':0,'model_inference_executed':False,'live_source_audit_performed':False,'cases':r.cases,'test_code_sha256':{p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((ROOT/'tests').glob('test_*.py'))},'log_sha256':hashlib.sha256(log.encode()).hexdigest()}
(ROOT/'review/TEST_RESULTS.json').write_text(json.dumps(report,indent=2)+'\n')
print(log);print(json.dumps({k:report[k] for k in ['scope','result','tests_run','failures','errors','skipped','product_gates_executed']},indent=2))
raise SystemExit(0 if r.wasSuccessful() else 1)
