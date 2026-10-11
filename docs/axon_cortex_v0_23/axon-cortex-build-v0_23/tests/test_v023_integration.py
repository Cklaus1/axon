from pathlib import Path
import json, subprocess, sys, tempfile, shutil, unittest, zipfile
ROOT=Path(__file__).resolve().parents[1]
class V023Tests(unittest.TestCase):
 def test_validator_passes(self):
  p=subprocess.run([sys.executable,str(ROOT/'tools/validate_package.py')],capture_output=True,text=True)
  self.assertEqual(p.returncode,0,p.stdout+p.stderr)
  self.assertEqual(json.loads(p.stdout)['result'],'PASS')
 def test_no_duplicate_authority(self):
  rows=json.loads((ROOT/'integration/V023_OWNER_MAP.json').read_text())['rows']
  self.assertTrue(all(r['new_authority'] is False for r in rows))
 def test_forge_is_standalone(self):
  m=json.loads((ROOT/'integration/V023_FORGE_INPUT_LOCK.json').read_text())
  self.assertEqual(m['authority'],'external_standalone_normative_dependency')
  self.assertFalse(m['copied_into_cortex_specs'])
 def test_parent_is_immutable_zip(self):
  m=json.loads((ROOT/'integration/V023_PARENT_LOCK.json').read_text())
  self.assertTrue((ROOT/m['path']).is_file())
 def test_runtime_default_off(self):
  q=json.loads((ROOT/'integration/V023_RUNTIME_QUALIFICATION.json').read_text())
  self.assertFalse(q['engineering_qualified']);self.assertEqual(q['product_gates_executed'],0)
 def test_strategy_schema(self):
  d=json.loads((ROOT/'schemas/json/strategy-artifact.schema.json').read_text());self.assertEqual(d['title'],'StrategyArtifact')
 def test_all_delta_gates_owned(self):
  t=json.loads((ROOT/'integration/V023_WORK_PACKAGES.json').read_text())['tasks'];g=json.loads((ROOT/'integration/V023_GATE_DELTA.json').read_text())['gates']
  self.assertEqual({x['id'] for x in g},{y for x in t for y in x['gate_targets']})
if __name__=='__main__':unittest.main()
