"""Mutation tests for new profile coverage, not product execution."""
from __future__ import annotations
import json,shutil,sys,tempfile,unittest
from pathlib import Path
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
from schema_reflex_package_checks import checks
class SchemaCoverageTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name)/'pack';shutil.copytree(ROOT,self.root)
    def edit(self,path,fn):
        p=self.root/path;value=json.loads(p.read_text());fn(value);p.write_text(json.dumps(value))
    def test_coverage_is_consistent(self):self.assertEqual(checks(self.root),[])
    def test_requirement_cannot_disappear(self):
        self.edit('integration/SCHEMA_REFLEX_REQUIREMENTS.json',lambda x:x['requirements'].pop())
        self.assertTrue(any('coverage' in e or 'map drift' in e for e in checks(self.root)))
    def test_reference_cannot_claim_product_success(self):
        self.edit('integration/SCHEMA_REFLEX_REQUIREMENTS.json',lambda x:x['requirements'][0].update(product_result='PASS'))
        self.assertTrue(any('completion' in e for e in checks(self.root)))
    def test_optional_research_not_a_new_profile_dependency(self):
        self.edit('task_manifest.json',lambda x:next(t for t in x['tasks'] if t['id']=='B210')['depends_on'].append('B222'))
        self.assertTrue(any('optional' in e for e in checks(self.root)))
    def test_profile_document_required(self):
        (self.root/'schemas/SCHEMA_DECISION_PROFILE.md').unlink()
        self.assertTrue(any('missing profile' in e for e in checks(self.root)))
    def test_current_owner_export_required(self):
        self.edit('integration/ACE_OWNER_EXPORT_LOCK.json',lambda x:x.update(document_version='0.19'))
        self.assertTrue(any('owner export' in e for e in checks(self.root)))
    def test_draft_cannot_activate(self):
        self.edit('fixtures/schema_reflex/experiment-template.json',lambda x:x.update(deployment_enabled=True))
        self.assertTrue(any('activated' in e for e in checks(self.root)))
