"""Mutational conformance checks, not source or live-runtime qualification."""
from pathlib import Path
import json
import shutil
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
from closed_loop_package_checks import checks


class PackagePreservationTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.root=Path(self.tmp.name)/'package'
        shutil.copytree(ROOT,self.root)
    def tearDown(self):self.tmp.cleanup()
    def edit(self,path,fn):
        p=self.root/path;d=json.loads(p.read_text());fn(d);p.write_text(json.dumps(d))
    def rejects(self):self.assertTrue(checks(self.root))
    def test_intact_package(self):self.assertEqual(checks(self.root),[])
    def test_export_lock_version(self):
        self.edit('integration/V022_OWNER_EXPORT_LOCK.json',lambda d:d.update(package_version='0.21'));self.rejects()
    def test_export_lock_digest(self):
        self.edit('integration/V022_OWNER_EXPORT_LOCK.json',lambda d:d['files'][0].update(sha256='0'*64));self.rejects()
    def test_export_missing_required_file(self):
        def change(d):d['files']=[x for x in d['files'] if x['path']!='task_manifest.json']
        self.edit('integration/V022_OWNER_EXPORT_LOCK.json',change);self.rejects()

    def test_parent_task_rewrite(self):
        self.edit('task_manifest.json',lambda d:d['tasks'][0].update(title='silently replaced'))
        self.rejects()
    def test_parent_gate_rewrite(self):
        self.edit('gate_manifest.json',lambda d:d['gates'][0].update(description='weakened'))
        self.rejects()
    def test_parent_schema_mutation(self):
        lock=json.loads((self.root/'integration/V022_PARENT_LOCK.json').read_text())
        p=self.root/lock['protected_files'][0]['path'];p.write_bytes(p.read_bytes()+b'\n')
        self.rejects()
    def test_fabric_bytes_mutated(self):
        p=self.root/'integration/compute-fabric-v0_1-reference/README.md';p.write_text(p.read_text()+'\nchanged\n')
        self.rejects()
    def test_fabric_extra_file(self):
        (self.root/'integration/compute-fabric-v0_1-reference/claimed-proof.txt').write_text('pass')
        self.rejects()
    def test_bad_active_version(self):
        self.edit('spec_manifest.json',lambda d:d.update(package_version='0.21'))
        self.rejects()
    def test_work_package_drift(self):
        self.edit('integration/V022_WORK_PACKAGES.json',lambda d:d['tasks'][0].update(title='different'))
        self.rejects()
    def test_requirement_claimed_implemented(self):
        self.edit('integration/V022_REQUIREMENTS.json',lambda d:d['requirements'][0].update(runtime_status='PASS'))
        self.rejects()
    def test_required_fabric_gate_omitted(self):
        self.edit('integration/V022_FABRIC_CROSSWALK.json',lambda d:d['required_gates'].pop())
        self.rejects()
    def test_required_fabric_work_deferred(self):
        self.edit('integration/V022_FABRIC_CROSSWALK.json',lambda d:d['rows'][0].update(master_task=None,disposition='DEFERRED_EXTENSION'))
        self.rejects()
    def test_extension_made_mandatory(self):
        def change(d):
            r=next(x for x in d['rows'] if x['disposition']=='DEFERRED_EXTENSION')
            r.update(master_task='B285',disposition='REQUIRED_M0_M2')
        self.edit('integration/V022_FABRIC_CROSSWALK.json',change);self.rejects()
    def test_unexecuted_runtime_qualified(self):
        self.edit('integration/V022_RUNTIME_QUALIFICATION.json',lambda d:d.update(engineering_qualified=True))
        self.rejects()
    def test_unknown_physical_profile_enabled(self):
        self.edit('integration/V022_RUNTIME_QUALIFICATION.json',lambda d:d.update(qualified_profiles=['fake-profile']))
        self.rejects()
    def test_legacy_gate_obligation_removed(self):
        self.edit('integration/V022_LEGACY_GATE_CLOSURE.json',lambda d:d['rows'][0]['gates'].pop())
        self.rejects()
    def test_qualification_master_gate_removed(self):
        self.edit('integration/V022_RUNTIME_QUALIFICATION.json',lambda d:d['required_master_gates'].pop())
        self.rejects()
    def test_fabric_dependency_removed_consistently(self):
        for path in ['task_manifest.json','integration/V022_WORK_PACKAGES.json']:
            self.edit(path,lambda d:next(x for x in d['tasks'] if x['id']=='B263').update(depends_on=['B255']))
        self.rejects()
    def test_historical_all_provider_backlog_added(self):
        for path in ['task_manifest.json','integration/V022_WORK_PACKAGES.json']:
            self.edit(path,lambda d:next(x for x in d['tasks'] if x['id']=='B272')['depends_on'].append('B250'))
        self.rejects()
    def test_optional_research_on_bounded_path(self):
        for path in ['task_manifest.json','integration/V022_WORK_PACKAGES.json']:
            self.edit(path,lambda d:next(x for x in d['tasks'] if x['id']=='B272').update(optional=True))
        self.rejects()
    def test_source_claim_rewritten(self):
        self.edit('integration/V022_INPUT_LOCK.json',lambda d:d.update(source_files_modified=True))
        self.rejects()
    def test_source_evidence_sha_unpinned(self):
        def change(d):
            r=next(x for x in d['evidence'] if x['origin']=='micode');r['sha256']='0'*64
        self.edit('review/SOURCE_EVIDENCE_V022.json',change);self.rejects()
    def test_synthetic_fixture_labeled_runtime(self):
        self.edit('fixtures/closed_loop_v022/bundle.json',lambda d:d.update(source_runtime_executed=True))
        self.rejects()
    def test_unapproved_template_activated(self):
        self.edit('fixtures/closed_loop_v022/pilot-template.json',lambda d:d.update(deployment_enabled=True))
        self.rejects()
    def test_missing_peer_delta(self):
        (self.root/'integration/MICODE_V022_CONSUMER_DELTA.md').unlink();self.rejects()
    def test_malformed_manifest_fails_closed(self):
        (self.root/'integration/V022_FABRIC_CROSSWALK.json').write_text('{}');self.rejects()
    def test_actual_study_cannot_be_forced(self):
        for path in ['task_manifest.json','integration/V022_WORK_PACKAGES.json']:
            self.edit(path,lambda d:next(x for x in d['tasks'] if x['id']=='B286').update(optional=False))
        self.rejects()

if __name__=='__main__':unittest.main()
