"""Inert Neural Program format reference. No tensors, network, execution or admission.

Requires jsonschema from requirements-review.txt. This is contract tooling, not a
production archive/tensor loader or a substitute for the Axon authority boundary.
"""
from __future__ import annotations
import argparse, hashlib, io, json, re, stat, struct, zipfile
from pathlib import Path
from typing import Any
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[1]
MAX_MANIFEST = 1 << 20
MAX_SOURCE = 256 << 10
MAX_ARCHIVE = (2 << 30) + (4 << 20)
MAX_PAYLOAD = 2 << 30
MAX_MEMBER = 1 << 30
MAX_MEMBERS = 256
DOMAIN = b'AXON-NP\x00v1\x00'

class ContractError(ValueError):
    """Expected closed failure of a source/container contract."""

def _fail(reason: str) -> None:
    raise ContractError(reason)

def _walk(value: Any, depth: int = 0, count: list[int] | None = None) -> None:
    if count is None: count = [0]
    count[0] += 1
    if depth > 64 or count[0] > 50000: _fail('JSON structural budget')
    if isinstance(value, str):
        try: value.encode('utf-8', errors='strict')
        except UnicodeError: _fail('Invalid Unicode scalar')
    elif value is None or isinstance(value, bool): pass
    elif isinstance(value, int):
        if abs(value) > 9007199254740991: _fail('Integer out of interoperable range')
    elif isinstance(value, float): _fail('Floating JSON values not allowed in identity profile')
    elif isinstance(value, list):
        for x in value: _walk(x, depth + 1, count)
    elif isinstance(value, dict):
        for k, v in value.items():
            if not isinstance(k, str): _fail('Non-string JSON key')
            _walk(k, depth + 1, count); _walk(v, depth + 1, count)
    else: _fail('Unsupported JSON value')

def canonical(value: Any) -> bytes:
    _walk(value)
    return json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(',', ':'), allow_nan=False).encode('utf-8')

def parse(raw: bytes, limit: int = MAX_MANIFEST) -> Any:
    if len(raw) > limit: _fail('JSON byte budget')
    if raw.startswith(b'\xef\xbb\xbf'): _fail('UTF-8 BOM unsupported')
    def pairs(items):
        out = {}
        for k, v in items:
            if k in out: _fail('Duplicate JSON key: ' + k)
            out[k] = v
        return out
    def nonint(_): _fail('Non-integer numeric encoding')
    try:
        value = json.loads(raw.decode('utf-8'), object_pairs_hook=pairs, parse_float=nonint, parse_constant=nonint)
        _walk(value)
        return value
    except ContractError: raise
    except (UnicodeError, ValueError, RecursionError, OverflowError) as exc:
        raise ContractError('Invalid bounded JSON') from exc

def _schema(name: str, value: Any) -> None:
    schema = json.loads((ROOT / 'schemas/json' / name).read_text())
    errors = list(Draft202012Validator(schema).iter_errors(value))
    if errors:
        e = errors[0]
        _fail('Schema: ' + '/'.join(map(str, e.absolute_path)) + ': ' + e.message)

def validate_source(raw: bytes) -> dict:
    source = parse(raw, MAX_SOURCE)
    _schema('neural-program-source.schema.json', source)
    ids = [r['id'] for r in source['requirements']]
    if len(ids) != len(set(ids)): _fail('Duplicate requirement ID')
    return source

def _path(name: str) -> None:
    if not re.fullmatch(r'[a-z0-9][a-z0-9._/-]{0,239}', name): _fail('Non-canonical member path')
    if any(p in ('', '.', '..') for p in name.split('/')): _fail('Unsafe member path')

def _manifest(value: dict) -> None:
    _schema('neural-program-manifest.schema.json', value)
    seen = set()
    for a in value['assets']:
        _path(a['path'])
        if a['path'] == 'manifest.json' or a['path'] in seen: _fail('Self/duplicate asset entry')
        seen.add(a['path'])
        if a['bytes'] > MAX_MEMBER: _fail('Declared member budget')
    if sum(a['bytes'] for a in value['assets']) > MAX_PAYLOAD: _fail('Declared payload budget')
    if value['source']['path'] not in seen: _fail('Source not inventoried')
    source_asset = next(a for a in value['assets'] if a['path'] == value['source']['path'])
    if source_asset['role'] != 'source' or source_asset['bytes'] > MAX_SOURCE: _fail('Source role/byte budget')
    if len(value['assets']) + 1 > MAX_MEMBERS: _fail('Member count budget')
    deps = [(d['kind'], d['id']) for d in value['dependencies']]
    if len(deps) != len(set(deps)): _fail('Duplicate dependency')
    kind = value['target']['execution_kind']
    roles = {a['role'] for a in value['assets']}
    if kind == 'fixture':
        if value['artifact_kind'] != 'conformance_fixture' or value['target']['payload_format'] != 'inert_fixture' or value['target']['supported_operations'] != ['inspect']: _fail('Fixture misrepresented as model')
    elif value['artifact_kind'] != 'neural_program': _fail('Fixture cannot use model execution target')
    elif kind == 'adapter':
        if not {'base_model','tokenizer'}.issubset({d['kind'] for d in value['dependencies']}) or 'adapter' not in roles: _fail('Adapter dependency closure incomplete')
        if value['target']['payload_format'] not in ('safetensors','gguf_lora'): _fail('Incompatible adapter format')
    elif kind == 'standalone':
        if not (roles & {'weights','graph'}): _fail('Standalone payload missing')
        if value['target']['payload_format'] not in ('safetensors','onnx'): _fail('Incompatible standalone format')

def artifact_digest(manifest: dict) -> str:
    return hashlib.sha256(DOMAIN + canonical(manifest)).hexdigest()

def validate_package(path: Path) -> dict:
    """Stream/check a bounded ZIP inventory; never extract or execute its files."""
    if path.stat().st_size > MAX_ARCHIVE: _fail('Archive byte budget')
    try:
        with path.open('rb') as fh, zipfile.ZipFile(fh) as z:
            fh.seek(0)
            if fh.read(4) != b'PK\x03\x04': _fail('Archive has a prefix or invalid signature')
            if z.comment: _fail('ZIP comments unsupported')
            fh.seek(-22, 2)
            eocd = fh.read(22)
            if len(eocd) != 22 or eocd[:4] != b'PK\x05\x06' or eocd[-2:] != b'\x00\x00': _fail('Trailing bytes or non-v1 ZIP ending')
            infos = z.infolist()
            if not infos or len(infos) > MAX_MEMBERS: _fail('Member count budget')
            seen, ranges = set(), []
            for i in infos:
                _path(i.filename)
                if i.filename in seen: _fail('Duplicate member')
                seen.add(i.filename)
                mode = i.external_attr >> 16
                if i.is_dir() or stat.S_IFMT(mode) not in (0,stat.S_IFREG) or mode & 0o111: _fail('Non-regular/executable member')
                if i.compress_type != zipfile.ZIP_STORED or i.flag_bits & (1|8) or i.extra or i.comment: _fail('Unsupported ZIP profile')
                if i.file_size > MAX_MEMBER or i.compress_size != i.file_size: _fail('Member size budget/profile')
                fh.seek(i.header_offset)
                header = fh.read(30)
                if len(header) != 30: _fail('Truncated local header')
                sig, ver, flags, method, tm, dt, crc, cs, us, nl, el = struct.unpack('<4s5H3I2H',header)
                if sig != b'PK\x03\x04' or flags != i.flag_bits or method != i.compress_type or crc != i.CRC or cs != i.compress_size or us != i.file_size: _fail('Local/central header mismatch')
                if el or fh.read(nl) != i.filename.encode('ascii'): _fail('Local member path/extra mismatch')
                end = i.header_offset+30+nl+el+i.file_size
                if end > z.start_dir: _fail('Member overlaps central directory')
                ranges.append((i.header_offset,end))
            ranges.sort()
            if ranges[0][0] != 0 or any(a[1] != b[0] for a,b in zip(ranges,ranges[1:])) or ranges[-1][1] != z.start_dir: _fail('Unaccounted or overlapping local data')
            if 'manifest.json' not in seen: _fail('Manifest missing')
            if z.getinfo('manifest.json').file_size > MAX_MANIFEST: _fail('Manifest byte budget')
            raw = z.read('manifest.json')
            manifest = parse(raw)
            _manifest(manifest)
            if canonical(manifest) != raw: _fail('Manifest is not canonical')
            expected = {'manifest.json'} | {a['path'] for a in manifest['assets']}
            if seen != expected: _fail('Inventory differs from archive')
            total = 0
            for a in manifest['assets']:
                if z.getinfo(a['path']).file_size != a['bytes']: _fail('Inventoried size mismatch')
                h, n = hashlib.sha256(), 0
                with z.open(a['path']) as f:
                    while chunk := f.read(1 << 20):
                        n += len(chunk); total += len(chunk)
                        if n > MAX_MEMBER or total > MAX_PAYLOAD: _fail('Read budget')
                        h.update(chunk)
                if n != a['bytes'] or h.hexdigest() != a['sha256']: _fail('Asset hash mismatch')
            src_raw = z.read(manifest['source']['path'])
            source = validate_source(src_raw)
            if hashlib.sha256(src_raw).hexdigest() != manifest['source']['raw_digest'] or hashlib.sha256(canonical(source)).hexdigest() != manifest['source']['semantic_digest']: _fail('Source binding mismatch')
            for key in ['input_schema_ref','output_schema_ref','effect_ceiling','data_policy_ref','fallback_policy_ref']:
                if source[key] != manifest[key]: _fail('Source/manifest contract mismatch: '+key)
            fh.seek(0); transport_hash = hashlib.sha256()
            while chunk := fh.read(1 << 20): transport_hash.update(chunk)
            return {'scope':'INERT_FORMAT_ONLY', 'format_valid':True, 'artifact_digest':artifact_digest(manifest), 'archive_digest':transport_hash.hexdigest(), 'artifact_kind':manifest['artifact_kind'], 'admitted':False,'semantic_correctness':'NOT_EVALUATED','model_executed':False}
    except ContractError: raise
    except (zipfile.BadZipFile, OSError, RuntimeError, EOFError, struct.error, UnicodeError, KeyError) as exc:
        raise ContractError('Invalid archive: '+str(exc)) from exc

def write_fixture(path: Path, manifest: dict, assets: dict[str,bytes], *, compression: int = zipfile.ZIP_STORED) -> None:
    """Create test archives; deliberately not an adoption or production compiler."""
    with zipfile.ZipFile(path,'w') as z:
        for name, content in [('manifest.json',canonical(manifest)),*sorted(assets.items())]:
            info=zipfile.ZipInfo(name,date_time=(2026,1,1,0,0,0))
            info.create_system=3; info.external_attr=(stat.S_IFREG | 0o644)<<16
            info.compress_type=compression; z.writestr(info,content)

def main() -> int:
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('path',type=Path);a=p.parse_args()
    try:
        if a.path.suffix == '.nps':
            s=validate_source(a.path.read_bytes());r={'scope':'SOURCE_FORMAT_ONLY','source_digest':hashlib.sha256(canonical(s)).hexdigest(),'semantic_correctness':'NOT_EVALUATED'}
        else:r=validate_package(a.path)
        print(json.dumps(r,indent=2));return 0
    except (OSError,ContractError) as exc: print(json.dumps({'result':'FAIL','reason':str(exc)}));return 1
if __name__ == '__main__': raise SystemExit(main())
