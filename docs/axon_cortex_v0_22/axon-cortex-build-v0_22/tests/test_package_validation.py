"""Mutation tests for documentation validation, not product-level adversarial execution."""
from __future__ import annotations
import json, shutil, sys, tempfile, unittest
from pathlib import Path
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
from validate_package import validate, hash_inventory, HASH_FILE, HASH_EXCLUDED

class DocumentationTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.root=Path(self.tmp.name)/'package'
        shutil.copytree(ROOT,self.root)
    def edit_json(self,name,fn):
        p=self.root/name;d=json.loads(p.read_text());fn(d);p.write_text(json.dumps(d))
    def fails(self,needle,integrity=False):
        r=validate(self.root,integrity=integrity);self.assertEqual(r['result'],'FAIL');self.assertTrue(any(needle in e for e in r['errors']),(needle,r['errors']))
    def test_unchanged_sources_and_full_orders(self):
        r=validate(self.root,integrity=False);self.assertEqual(r['errors'],[]);self.assertEqual(len(r['topological_orders']['tasks']),r['counts']['tasks']);self.assertEqual(len(r['topological_orders']['specs']),r['counts']['specs']);self.assertEqual(r['product_gates_executed'],0)
    def test_unknown_task_dependency(self):self.edit_json('task_manifest.json',lambda d:d['tasks'][0]['depends_on'].append('B9999'));self.fails('unknown dependency')
    def test_task_cycle(self):self.edit_json('task_manifest.json',lambda d:d['tasks'][0]['depends_on'].append('B01'));self.fails('dependency cycle')
    def test_orphan_gate(self):
        def change(d):
            for t in d['tasks']:t['gate_targets']=[g for g in t['gate_targets'] if g!='G36-source']
        self.edit_json('task_manifest.json',change);self.fails('Unowned gates')
    def test_unknown_gate(self):self.edit_json('task_manifest.json',lambda d:d['tasks'][0]['gate_targets'].append('G99-fiction'));self.fails('unknown gate')
    def test_false_product_pass(self):self.edit_json('gate_manifest.json',lambda d:d['gates'][0].update(product_result='PASS'));self.fails('product result/status')
    def test_false_task_completion(self):self.edit_json('task_manifest.json',lambda d:d['tasks'][0].update(status='Completed'));self.fails('live implementation status')
    def test_gate_description_drift(self):self.edit_json('gate_manifest.json',lambda d:d['gates'][0].update(description='Weaker obligation'));self.fails('gate text mismatch')
    def test_missing_source_gate(self):self.edit_json('gate_manifest.json',lambda d:d['gates'].pop());self.fails('Source gate IDs differ')
    def test_spec_frontmatter_drift(self):
        p=self.root/'specs/CX-00-system-contract.md';p.write_text(p.read_text().replace('depends_on: []','depends_on: ["CX-01"]'));self.fails('metadata dependencies differ')
    def test_stale_master(self):
        p=self.root/'AXON_CORTEX_MASTER.md';p.write_text(p.read_text()+'\nSTALE\n');self.fails('Stale generated view: AXON_CORTEX_MASTER.md')
    def test_stale_task_view(self):
        p=self.root/'build/TASKS.md';p.write_text('outdated');self.fails('Stale generated view: build/TASKS.md')
    def test_missing_protocol_in_master(self):
        p=self.root/'schemas/PROTOCOLS.md';p.write_text(p.read_text()+'\nNew contract added after compilation.\n');self.fails('Stale generated view: AXON_CORTEX_MASTER.md')
    def test_missing_link(self):
        p=self.root/'README.md';p.write_text(p.read_text()+'\n[broken](missing-file.md)\n');self.fails('missing link')
    def test_fence_unclosed(self):
        p=self.root/'README.md';p.write_text(p.read_text()+'\n```text\nunclosed\n');self.fails('unclosed fenced block')
    def test_profile_unknown_task(self):self.edit_json('release_profiles.json',lambda d:d['profiles'][0]['exit_tasks'].append('B999'));self.fails('unknown task')
    def test_profile_activates_default(self):self.edit_json('release_profiles.json',lambda d:d.update(default_active_runtime=True));self.fails('may not activate runtime')
    def test_optional_hosted_import_on_critical_path(self):self.edit_json('release_profiles.json',lambda d:d['profiles'][0]['exit_tasks'].append('B157'));self.fails('Optional hosted importer')
    def test_reserved_id_duplicate(self):self.edit_json('spec_manifest.json',lambda d:d['reserved_ids'].append({'id':'CX-00'}));self.fails('Reserved spec ID')
    def test_bad_format_fixture(self):
        p=self.root/'fixtures/neural_programs/minimal.np';p.write_bytes(b'bad zip');self.fails('Neural format fixture')
    def test_complete_hash_inventory(self):
        (self.root/HASH_FILE).write_text(json.dumps({'schema':'axon-package-sha256/1','excluded':sorted(HASH_EXCLUDED),'files':hash_inventory(self.root)}))
        r=validate(self.root,integrity=True);self.assertEqual(r['errors'],[])
    def test_hash_tamper(self):
        (self.root/HASH_FILE).write_text(json.dumps({'schema':'axon-package-sha256/1','excluded':sorted(HASH_EXCLUDED),'files':hash_inventory(self.root)}))
        p=self.root/'review/INPUTS.json';p.write_text(p.read_text()+' ');self.fails('Checksum mismatch:',True)
    def test_untracked_file(self):
        (self.root/HASH_FILE).write_text(json.dumps({'schema':'axon-package-sha256/1','excluded':sorted(HASH_EXCLUDED),'files':hash_inventory(self.root)}))
        (self.root/'rogue.txt').write_text('not in inventory');self.fails('Checksum file inventory',True)

if __name__=='__main__':unittest.main()
