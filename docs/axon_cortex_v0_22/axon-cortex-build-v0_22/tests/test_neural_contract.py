"""Adversarial format fixtures. No model, learned output, grants or sandbox are executed."""
from __future__ import annotations
import copy, hashlib, json, stat, struct, sys, tempfile, unittest, warnings, zipfile
from pathlib import Path
from unittest.mock import patch
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
import neural_contract_reference as nc

class SourceTests(unittest.TestCase):
    def setUp(self):
        self.raw=(ROOT/'fixtures/neural_programs/minimal-source.nps').read_bytes()
        self.s=json.loads(self.raw)
    def reject(self,obj):
        with self.assertRaises(nc.ContractError):nc.validate_source(json.dumps(obj).encode())
    def test_valid_source(self):self.assertEqual(nc.validate_source(self.raw)['schema'],'axon.nps/1')
    def test_pretty_source_preserves_semantic_digest(self):
        self.assertEqual(nc.canonical(nc.validate_source(self.raw)),nc.canonical(nc.validate_source(nc.canonical(self.s))))
    def test_canonical_known_unicode_vector(self):self.assertEqual(nc.canonical({'é':'a\nb','a':2}),b'{"a":2,"\xc3\xa9":"a\\nb"}')
    def test_duplicate_key(self):
        with self.assertRaises(nc.ContractError):nc.parse(b'{"x":1,"x":2}')
    def test_escaped_duplicate_key(self):
        with self.assertRaises(nc.ContractError):nc.parse(b'{"x":1,"\\u0078":2}')
    def test_bom(self):
        with self.assertRaises(nc.ContractError):nc.validate_source(b'\xef\xbb\xbf'+self.raw)
    def test_invalid_utf8(self):
        with self.assertRaises(nc.ContractError):nc.parse(b'{"x":"\xff"}')
    def test_lone_surrogate(self):
        with self.assertRaises(nc.ContractError):nc.parse(b'{"x":"\\ud800"}')
    def test_float(self):
        with self.assertRaises(nc.ContractError):nc.parse(b'{"x":1.0}')
    def test_exponent(self):
        with self.assertRaises(nc.ContractError):nc.parse(b'{"x":1e3}')
    def test_nonfinite(self):
        with self.assertRaises(nc.ContractError):nc.parse(b'{"x":NaN}')
    def test_huge_integer(self):
        with self.assertRaises(nc.ContractError):nc.parse(b'{"x":9007199254740992}')
    def test_depth_budget(self):
        with self.assertRaises(nc.ContractError):nc.parse(b'['*70+b'0'+b']'*70)
    def test_node_budget(self):
        with self.assertRaises(nc.ContractError):nc.canonical([None]*50001)
    def test_byte_budget(self):
        with self.assertRaises(nc.ContractError):nc.parse(self.raw,10)
    def test_unknown_field(self):self.s['grant_root']=True;self.reject(self.s)
    def test_unknown_version(self):self.s['schema']='axon.nps/999';self.reject(self.s)
    def test_duplicate_clause_id(self):self.s['requirements']*=2;self.reject(self.s)
    def test_must_without_validator(self):del self.s['requirements'][0]['validation_ref'];self.reject(self.s)
    def test_mutable_schema_ref(self):del self.s['input_schema_ref']['digest'];self.reject(self.s)
    def test_effect_ceiling_enum(self):self.s['effect_ceiling']['network']='anything';self.reject(self.s)
    def test_absent_input_schema(self):del self.s['input_schema_ref'];self.reject(self.s)
    def test_not_valid_json(self):
        with self.assertRaises(nc.ContractError):nc.validate_source(b'behavior: pretend yaml\n')

class PackageTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.path=Path(self.tmp.name)/'test.np'
        self.m=json.loads((ROOT/'fixtures/neural_programs/minimal-manifest.json').read_bytes())
        with zipfile.ZipFile(ROOT/'fixtures/neural_programs/minimal.np') as z:self.assets={a['path']:z.read(a['path']) for a in self.m['assets']}
    def emit(self):nc.write_fixture(self.path,self.m,self.assets)
    def reject(self):
        with self.assertRaises(nc.ContractError):nc.validate_package(self.path)
    def rewrite(self,mutator):
        with zipfile.ZipFile(self.path) as z:items=[(i,z.read(i.filename)) for i in z.infolist()]
        with zipfile.ZipFile(self.path,'w') as z:
            for i,b in items:
                i,b=mutator(i,b);z.writestr(i,b)
    def test_valid_but_not_admitted(self):
        self.emit();r=nc.validate_package(self.path);self.assertFalse(r['admitted']);self.assertFalse(r['model_executed']);self.assertEqual(r['semantic_correctness'],'NOT_EVALUATED')
    def test_domain_separated_identity(self):
        self.assertEqual(nc.artifact_digest(self.m),hashlib.sha256(b'AXON-NP\0v1\0'+nc.canonical(self.m)).hexdigest())
    def test_archive_timestamp_not_semantic_identity(self):
        self.emit();a=nc.validate_package(self.path)
        def change(i,b):i.date_time=(2026,2,2,0,0,0);return i,b
        self.rewrite(change);b=nc.validate_package(self.path)
        self.assertEqual(a['artifact_digest'],b['artifact_digest']);self.assertNotEqual(a['archive_digest'],b['archive_digest'])
    def test_target_revision_changes_identity(self):
        old=nc.artifact_digest(self.m);self.m['target']['backend_revision']='different';self.assertNotEqual(old,nc.artifact_digest(self.m))
    def test_self_id_rejected(self):self.m['artifact_id']='self';self.emit();self.reject()
    def test_embedded_admission_rejected(self):self.m['admitted']=True;self.emit();self.reject()
    def test_noncanonical_manifest(self):
        self.emit()
        def change(i,b):return i,json.dumps(self.m,indent=2).encode() if i.filename=='manifest.json' else b
        self.rewrite(change);self.reject()
    def test_extra_member(self):self.assets['extra.txt']=b'not in inventory';self.emit();self.reject()
    def test_missing_member(self):self.assets.pop('fixtures/payload.txt');self.emit();self.reject()
    def test_duplicate_member(self):
        self.emit()
        with warnings.catch_warnings():
            warnings.simplefilter('ignore',UserWarning)
            with zipfile.ZipFile(self.path,'a') as z:z.writestr('manifest.json',nc.canonical(self.m))
        self.reject()
    def test_traversal(self):self.assets['../escape.txt']=b'x';self.emit();self.reject()
    def test_absolute(self):self.assets['/escape.txt']=b'x';self.emit();self.reject()
    def test_backslash(self):self.assets['sub\\escape.txt']=b'x';self.emit();self.reject()
    def test_uppercase_path(self):self.assets['FIXTURES/payload.txt']=b'x';self.emit();self.reject()
    def test_unicode_path(self):self.assets['fixtures/é.txt']=b'x';self.emit();self.reject()
    def test_empty_path_segment(self):self.assets['fixtures//bad.txt']=b'x';self.emit();self.reject()
    def test_directory_member(self):self.assets['directory/']=b'';self.emit();self.reject()
    def test_symlink(self):
        self.emit()
        def change(i,b):
            if i.filename=='fixtures/payload.txt':i.external_attr=(stat.S_IFLNK|0o777)<<16
            return i,b
        self.rewrite(change);self.reject()
    def test_executable(self):
        self.emit()
        def change(i,b):
            if i.filename=='fixtures/payload.txt':i.external_attr=(stat.S_IFREG|0o755)<<16
            return i,b
        self.rewrite(change);self.reject()
    def test_compressed(self):nc.write_fixture(self.path,self.m,self.assets,compression=zipfile.ZIP_DEFLATED);self.reject()
    def test_checksum_substitution(self):self.assets['fixtures/payload.txt']=b'ATTACK';self.emit();self.reject()
    def test_declared_size_mismatch(self):self.m['assets'][0]['bytes']+=1;self.emit();self.reject()
    def test_source_manifest_contract_mismatch(self):self.m['effect_ceiling']['network']='approved_only';self.emit();self.reject()
    def test_source_semantic_digest_mismatch(self):self.m['source']['semantic_digest']='0'*64;self.emit();self.reject()
    def test_source_role_mismatch(self):
        for a in self.m['assets']:
            if a['path'].endswith('.nps'):a['role']='fixture'
        self.emit();self.reject()
    def test_duplicate_asset_inventory(self):self.m['assets'].append(self.m['assets'][0]);self.emit();self.reject()
    def test_manifest_in_own_inventory(self):self.m['assets'][0]['path']='manifest.json';self.emit();self.reject()
    def test_fixture_as_active_kind(self):self.m['artifact_kind']='neural_program';self.emit();self.reject()
    def test_fixture_cannot_invoke(self):self.m['target']['supported_operations'].append('invoke');self.emit();self.reject()
    def test_adapter_missing_dependencies(self):
        self.m['artifact_kind']='neural_program';self.m['target']['execution_kind']='adapter';self.m['target']['payload_format']='gguf_lora';self.emit();self.reject()
    def test_payload_budget(self):
        self.emit()
        with patch.object(nc,'MAX_PAYLOAD',10):self.reject()
    def test_source_budget_before_read(self):
        self.emit()
        with patch.object(nc,'MAX_SOURCE',10):self.reject()
    def test_member_count_budget(self):
        self.emit()
        with patch.object(nc,'MAX_MEMBERS',2):self.reject()
    def test_archive_budget(self):
        self.emit()
        with patch.object(nc,'MAX_ARCHIVE',5):self.reject()
    def test_local_header_mismatch(self):
        self.emit();b=bytearray(self.path.read_bytes());b[14]^=1;self.path.write_bytes(b);self.reject()
    def test_encrypted_flag(self):
        self.emit();b=bytearray(self.path.read_bytes());b[6]|=1;self.path.write_bytes(b);self.reject()
    def test_prefix_polyglot(self):self.emit();self.path.write_bytes(b'JUNK'+self.path.read_bytes());self.reject()
    def test_trailing_bytes(self):self.emit();self.path.write_bytes(self.path.read_bytes()+b'JUNK');self.reject()
    def test_truncated_archive(self):self.emit();self.path.write_bytes(self.path.read_bytes()[:-8]);self.reject()
    def test_zip_comment(self):
        self.emit()
        with zipfile.ZipFile(self.path,'a') as z:z.comment=b'untracked'
        self.reject()
    def test_extension_does_not_determine_validity(self):
        self.path.write_text('pretend these are raw weights renamed to .np');self.reject()

if __name__=='__main__':unittest.main()
