"""Mutation checks for ACE source/owner integration; no product execution."""
import json, shutil, sys, tempfile, unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
from ace_integration_checks import checks
class ACEPackageTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.root=Path(self.tmp.name)/'p';shutil.copytree(ROOT,self.root)
    def edit(self,p,fn):
        q=self.root/p;d=json.loads(q.read_text());fn(d);q.write_text(json.dumps(d))
    def fails(self,s):self.assertTrue(any(s in e for e in checks(self.root)),checks(self.root))
    def test_clean_crosswalk(self):self.assertEqual(checks(self.root),[])
    def test_missing_requirement(self):self.edit('integration/ACE_REQUIREMENT_CROSSWALK.json',lambda d:d['requirements'].pop());self.fails('coverage')
    def test_rewritten_source_requirement(self):self.edit('integration/ACE_REQUIREMENT_CROSSWALK.json',lambda d:d['requirements'][0].update(source_title='Fake'));self.fails('original requirement')
    def test_false_product_pass(self):self.edit('integration/ACE_REQUIREMENT_CROSSWALK.json',lambda d:d['requirements'][0].update(product_result='PASS'));self.fails('false disposition')
    def test_unknown_requirement_owner(self):self.edit('integration/ACE_REQUIREMENT_CROSSWALK.json',lambda d:d['requirements'][0].update(tasks=['B999']));self.fails('unknown requirement task')
    def test_changed_reviewed_source_schema(self):
        p=self.root/'schemas/json/neural-program-source.schema.json';p.write_text(p.read_text()+'\n');self.fails('reviewed CX-36 contract changed')
    def test_changed_legacy_source(self):
        p=self.root/'integration/anea-v0_2-reference/ANEA_SPEC.txt';p.write_text(p.read_text()+'\n');self.fails('source reference changed')
    def test_changed_owner_schema(self):
        p=self.root/'schemas/json/ace-execution.schema.json';p.write_text(p.read_text()+'\n');self.fails('owner export changed')
    def test_invented_neural_accounting_class(self):self.edit('integration/ACE_VOCABULARY_MAP.json',lambda d:d['rows'][0].update(cognitive_class='NEURAL_PROGRAM'));self.fails('invented accounting opcode')
    def test_immutable_original_decision(self):self.edit('integration/ACE_DECISIONS.json',lambda d:d['decisions'][0]['original'].update(title='changed'));self.fails('source decision rewritten')
    def test_legacy_gate_alias_cannot_be_rewritten_as_current_key(self):
        self.edit('integration/ACE_LEGACY_ALIAS_MAP.json',lambda d:d['gate_aliases'].__setitem__('G16-anea-peer','G16-anea-peer'))
        self.fails('legacy gate alias drift')
    def test_historical_raw_score_vocabulary_is_preserved(self):
        self.edit('integration/ACE_VOCABULARY_MAP.json',lambda d:d.update(raw_origin_aliases=[r for r in d['raw_origin_aliases'] if r['producer_vocabulary']!='micode.anea-fixture/1']))
        self.fails('raw score vocabulary alias missing')
    def test_core_cannot_depend_on_three_family(self):self.edit('task_manifest.json',lambda d:next(t for t in d['tasks'] if t['id']=='B182')['depends_on'].append('B184'));self.fails('optional research')
