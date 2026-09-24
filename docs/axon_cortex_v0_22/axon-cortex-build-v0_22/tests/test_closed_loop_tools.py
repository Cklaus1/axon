"""Read-only tools and evidence-completeness tests; no trusted runtime claims."""
from pathlib import Path
import hashlib
import json
import sys
import tempfile
import unittest
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
from plan_upgrade_v022 import inspect as inspect_source,safe_path
from check_v022_runtime_evidence import inspect as inspect_evidence
from closed_loop_reference import Refusal


class UpgradeTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.p=Path(self.tmp.name);(self.p/'main.rs').write_text('unchanged source')
        self.rows=[{'path':'main.rs','sha256':hashlib.sha256(b'unchanged source').hexdigest()}]
    def tearDown(self):self.tmp.cleanup()
    def test_read_only_matching(self):
        before=(self.p/'main.rs').read_bytes();r=inspect_source(self.p,self.rows)
        self.assertEqual(r['matched_files'],1);self.assertFalse(r['writes_performed']);self.assertEqual((self.p/'main.rs').read_bytes(),before)
    def test_changed_requires_review(self):
        (self.p/'main.rs').write_text('new useful work');r=inspect_source(self.p,self.rows);self.assertEqual(r['changed_paths'],['main.rs']);self.assertTrue(r['intake_review_required'])
    def test_missing_requires_review(self):
        (self.p/'main.rs').unlink();self.assertEqual(inspect_source(self.p,self.rows)['missing_paths'],['main.rs'])
    def test_extra_work_preserved(self):
        (self.p/'new.rs').write_text('new');r=inspect_source(self.p,self.rows);self.assertEqual(r['extra_paths'],['new.rs']);self.assertTrue((self.p/'new.rs').exists())
    def test_git_not_claimed_clean(self):
        (self.p/'.git').mkdir();r=inspect_source(self.p,self.rows);self.assertIn('.git',r['excluded_metadata_or_build_directories']);self.assertIn('NOT_ESTABLISHED',r['git_cleanliness'])
    def test_symlink_file_not_followed(self):
        (self.p/'main.rs').unlink();(self.p/'main.rs').symlink_to('/etc/passwd');self.assertTrue(inspect_source(self.p,self.rows)['unsafe_paths'])
    def test_symlink_root_refused(self):
        target=self.p/'sub';target.mkdir();link=self.p/'link';link.symlink_to(target)
        with self.assertRaises(ValueError):inspect_source(link,[])
    def test_inventory_traversal(self):
        for path in ['../outside','/outside','a/../b','C:/outside','a\\b','a//b']:
            with self.subTest(path=path),self.assertRaises(ValueError):safe_path(self.p,path)
    def test_source_script_is_only_hashed(self):
        (self.p/'evil.sh').write_text('exit 99\n');r=inspect_source(self.p,self.rows);self.assertFalse(r['source_scripts_executed'])


class EvidenceTests(unittest.TestCase):
    def empty(self):return {'schema':'axon.closed-loop.runtime-evidence/1','package_version':'0.22','records':[]}
    def full(self):
        q=json.loads((ROOT/'integration/V022_RUNTIME_QUALIFICATION.json').read_text());ids=q['required_master_gates']+q['required_fabric_gates']+q['required_legacy_gates']
        d=self.empty();d['records']=[{'gate_id':g,'result':'PASS','source_refs':['cl22:'+'a'*64], 'test_command':'untrusted claimed command','test_cases':['claimed-case'],'assertions':1,'profile_ref':'cl22:'+'b'*64,'issuer_ref':'untrusted-claimed-issuer','evidence_refs':['cl22:'+'c'*64],'synthetic':False} for g in ids];return d
    def test_empty_never_qualifies(self):
        r=inspect_evidence(self.empty());self.assertEqual(r['result'],'NOT_QUALIFIED');self.assertFalse(r['engineering_qualified']);self.assertGreater(r['required_gate_count'],100)
    def test_forged_complete_table_still_cannot_qualify(self):
        r=inspect_evidence(self.full());self.assertEqual(r['result'],'REQUIRES_EXTERNAL_VERIFICATION');self.assertFalse(r['engineering_qualified']);self.assertFalse(r['signatures_or_sources_authenticated'])
    def test_synthetic_cannot_close_live_gates(self):
        d=self.full();d['records'][0]['synthetic']=True;self.assertEqual(inspect_evidence(d)['result'],'NOT_QUALIFIED')
    def test_duplicate_receipt_rejected(self):
        d=self.full();d['records'].append(d['records'][0]);self.assertTrue(inspect_evidence(d)['errors'])
    def test_out_of_scope_gate(self):
        d=self.full();d['records'][0]['gate_id']='UNKNOWN';self.assertTrue(inspect_evidence(d)['errors'])
    def test_nonpass_leaves_gate_open(self):
        d=self.full();d['records'][0]['result']='BLOCKED';self.assertEqual(inspect_evidence(d)['result'],'NOT_QUALIFIED')
    def test_zero_assertions_rejected(self):
        d=self.full();d['records'][0]['assertions']=0
        with self.assertRaises(Refusal):inspect_evidence(d)
    def test_claim_fields_never_inferred(self):
        r=inspect_evidence(self.full());self.assertFalse(r['policy_activated']);self.assertFalse(r['measured_improvement_supported']);self.assertEqual(r['product_gates_executed'],0)

if __name__=='__main__':unittest.main()
