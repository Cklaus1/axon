"""Mutation tests for mappings, retained contracts and non-destructive planning."""
from __future__ import annotations
import hashlib,json,shutil,sys,tempfile,unittest
from pathlib import Path
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
from research_package_checks import checks
from plan_upgrade import inspect,safe_member

class ResearchPackageTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup)
        self.root=Path(self.tmp.name)/'pack';shutil.copytree(ROOT,self.root)
    def edit(self,path,fn):
        p=self.root/path;d=json.loads(p.read_text());fn(d);p.write_text(json.dumps(d))
    def test_consistent(self):self.assertEqual(checks(self.root),[])
    def test_missing_family(self):
        self.edit('integration/RESEARCH_REQUIREMENTS.json',lambda x:x['requirements'].pop());self.assertTrue(checks(self.root))
    def test_bad_gate_backlink(self):
        self.edit('integration/RESEARCH_REQUIREMENTS.json',lambda x:x['requirements'][0]['gates'].pop());self.assertTrue(checks(self.root))
    def test_duplicate_authority(self):
        self.edit('integration/SOURCE_OWNER_MAP.json',lambda x:x['aliases'][0].update(new_authority=True));self.assertTrue(checks(self.root))
    def test_owner_change(self):
        self.edit('integration/SOURCE_OWNER_MAP.json',lambda x:x['aliases'][0].update(primary_owner='CX-05'));self.assertTrue(checks(self.root))
    def test_false_peer_delivery(self):
        self.edit('integration/SOURCE_OWNER_MAP.json',lambda x:x['versions'].update(micode_consumer_target='0.15 DONE'));self.assertTrue(checks(self.root))
    def test_research_not_optional(self):
        self.edit('task_manifest.json',lambda x:next(t for t in x['tasks'] if t['id']=='B251').update(optional=False));self.assertTrue(checks(self.root))
    def test_optional_critical_dependency(self):
        self.edit('task_manifest.json',lambda x:next(t for t in x['tasks'] if t['id']=='B249')['depends_on'].append('B251'));self.assertTrue(checks(self.root))
    def test_original_task_not_reset_or_changed(self):
        self.edit('task_manifest.json',lambda x:x['tasks'][0].update(output='replaced'));self.assertTrue(checks(self.root))
    def test_original_gate_not_changed(self):
        self.edit('gate_manifest.json',lambda x:x['gates'][0].update(description='replaced'));self.assertTrue(checks(self.root))
    def test_protected_schema(self):
        p=self.root/'schemas/json/ace-runtime.schema.json';p.write_text(p.read_text()+'\n');self.assertTrue(checks(self.root))
    def test_false_replication(self):
        self.edit('integration/RESEARCH_SOURCES.json',lambda x:x.update(replication_status='ALL_PASS'));self.assertTrue(checks(self.root))
    def test_fixture_not_live(self):
        self.edit('fixtures/research_v021/decision.json',lambda x:x.update(product_result='PASS'));self.assertTrue(checks(self.root))
    def test_disabled_experiment(self):
        self.edit('fixtures/research_v021/experiment-plan.json',lambda x:x.update(deployment_enabled=True));self.assertTrue(checks(self.root))
    def test_missing_profile(self):
        (self.root/'build/CVM_CANONICAL_PROJECTIONS.md').unlink();self.assertTrue(checks(self.root))

class UpgradeTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.root=Path(self.tmp.name)
        (self.root/'x.txt').write_text('original')
        self.inv=[{'path':'x.txt','sha256':hashlib.sha256(b'original').hexdigest()}]
    def test_dry_run_no_mutation(self):
        before=(self.root/'x.txt').read_bytes();r=inspect(self.root,self.inv)
        self.assertTrue(r['source_equivalent_on_reviewed_inventory']);self.assertFalse(r['source_mutated']);self.assertEqual((self.root/'x.txt').read_bytes(),before)
    def test_changed_source_not_silently_current(self):
        (self.root/'x.txt').write_text('new work');r=inspect(self.root,self.inv)
        self.assertEqual(r['changed_paths'],['x.txt']);self.assertEqual((self.root/'x.txt').read_text(),'new work')
    def test_missing_source(self):
        (self.root/'x.txt').unlink();self.assertEqual(inspect(self.root,self.inv)['missing_paths'],['x.txt'])
    def test_path_escape(self):
        with self.assertRaises(ValueError):safe_member(self.root,'../outside')
    def test_absolute_path(self):
        with self.assertRaises(ValueError):safe_member(self.root,'/etc/passwd')
    def test_symlink_not_followed(self):
        (self.root/'link').symlink_to('/etc/passwd')
        with self.assertRaises(ValueError):safe_member(self.root,'link')
    def test_legacy_gate_signatures(self):
        (self.root/'scripts').mkdir();(self.root/'scripts/cortex_package_gate.sh').write_text('axon_cortex_v0_15 cortex-package-sha256/1 --report min(c.values(), default=0) package_validation.json')
        self.assertEqual(len(inspect(self.root,self.inv)['package_gate_findings']),5)

if __name__=='__main__':unittest.main()
