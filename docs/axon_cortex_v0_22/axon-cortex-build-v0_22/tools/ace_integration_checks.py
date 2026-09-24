"""ACE integration document checks, not runtime/model authorization."""
from pathlib import Path
import hashlib, json

def checks(root: Path) -> list[str]:
    errors=[]
    def err(s):errors.append('ACE: '+s)
    def load(rel):return json.loads((root/rel).read_text(encoding='utf-8'))
    def hash_path(rel):
        p=(root/rel).resolve()
        if not p.is_relative_to(root.resolve()) or not p.is_file():raise ValueError('outside/missing path '+rel)
        return hashlib.sha256(p.read_bytes()).hexdigest()
    try:
        lock=load('integration/ACE_SOURCE_LOCK.json')
        if lock.get('execution_profile')!='axon.ace-execution/1' or lock.get('product_gates_executed')!=0:err('profile or product status mismatch')
        if lock.get('retained_formats')!=['axon.nps/1','axon.np/1','axon.cjson/1']:err('reviewed formats changed')
        for r in lock['reference_files']:
            if hash_path(r['path'])!=r['sha256']:err('source reference changed: '+r['path'])
            if r['use']!='historical_reference_not_active_format':err('legacy source promoted to active format')
        for r in lock['unchanged_reviewed_contracts']:
            if hash_path(r['path'])!=r['sha256']:err('reviewed CX-36 contract changed: '+r['path'])
        original=load('integration/anea-v0_2-reference/REQUIREMENTS.json')['requirements'];oi={r['id']:r for r in original}
        cw=load('integration/ACE_REQUIREMENT_CROSSWALK.json');rs=cw['requirements'];ids=[r['source_requirement_id'] for r in rs]
        if cw['source_requirement_count']!=86 or len(ids)!=86 or set(ids)!=set(oi):err('86-requirement coverage mismatch')
        td={t['id']:t for t in load('task_manifest.json')['tasks']};sd={s['id']:s for s in load('spec_manifest.json')['specs']};gd={g['id']:g for g in load('gate_manifest.json')['gates']}
        text=(root/'schemas/ACE_EXECUTION_PROFILE.md').read_text();source=(root/'integration/anea-v0_2-reference/ANEA_SPEC.txt').read_text()
        allowed={'OWNER_AMENDMENT','REVIEWED_CONTRACT_REAFFIRMED','REVIEWED_CONTRACT_SUPERSEDES_REFERENCE','DEFERRED_RESEARCH_UNSUPPORTED_BY_DEFAULT','CONSUMER_OBLIGATION_WITH_AXON_BRIDGE'}
        for r in rs:
            o=oi.get(r['source_requirement_id'],{})
            digest=hashlib.sha256(json.dumps(o,sort_keys=True,ensure_ascii=False,separators=(',',':')).encode()).hexdigest()
            if r['source_title']!=o.get('title') or r['source_requirement_sha256']!=digest:err('original requirement changed '+r['source_requirement_id'])
            if ('**'+r['source_requirement_id']+' —') not in source:err('source anchor missing '+r['source_requirement_id'])
            if '## '+r['profile_section']+' —' not in text:err('owner section missing')
            if r['disposition'] not in allowed or r['product_result']!='NOT_RUN' or r['document_status']!='INTEGRATED_PROPOSED':err('false disposition/product claim')
            if not all(r.get(k) for k in ['resolution','tasks','gates','owner_specs','amended_files']):err('unowned requirement')
            for t in r['tasks']:
                if t not in td:err('unknown requirement task '+t)
            for g in r['gates']:
                if g not in gd:err('unknown requirement gate '+g)
                elif not any(g in td.get(t,{}).get('gate_targets',[]) for t in r['tasks']):err('requirement gate not linked by task '+g)
            for s in r['owner_specs']:
                if s not in sd:err('unknown owner '+s)
            for f in r['amended_files']:hash_path(f)
        original_d={r['id']:r for r in load('integration/anea-v0_2-reference/OPEN_DECISIONS.json')['decisions']}
        ds=load('integration/ACE_DECISIONS.json')['decisions']
        if {r['id'] for r in ds}!=set(original_d):err('decision coverage differs')
        for r in ds:
            if r['original']!=original_d[r['id']] or r['product_status']!='NOT_RUN':err('source decision rewritten or product claim asserted')
        aliases=load('integration/ACE_LEGACY_ALIAS_MAP.json')
        expected_gate_aliases={g:g.replace('-anea-','-ace-') for g in ['G00-anea-coverage','G00-anea-owner-map','G04-anea-joins','G04-anea-lowering','G05-anea-features','G05-anea-results','G05-anea-scoring','G06-anea-provenance','G13-anea-attempts','G13-anea-locality','G15-anea-parity','G16-anea-core','G16-anea-peer','G20-anea-three-family','G23-anea-lifecycle','G28-anea-reuse','G32-anea-attempt-lineage','G34-anea-dispatch','G34-anea-fallback']}
        if aliases.get('schema_aliases')!={'axon.anea-execution/1':'axon.ace-execution/1','axon.anea-projection/1':'axon.ace-projection/1'}:err('legacy schema alias drift')
        if aliases.get('gate_aliases')!=expected_gate_aliases:err('legacy gate alias drift')
        if aliases.get('producer_vocabulary_aliases',{}).get('micode.anea-fixture/1')!='micode.ace-fixture/1':err('legacy producer vocabulary alias missing')
        mapping=load('integration/ACE_VOCABULARY_MAP.json')
        if mapping['status']!='PROPOSED_DOCUMENT_CONTRACT' or mapping['product_status']!='NOT_RUN':err('mapping false live claim')
        mids=[r['id'] for r in mapping['rows']]
        if len(mids)!=len(set(mids)):err('duplicate host mapping')
        for r in mapping['rows']:
            if r['cognitive_class'] in {'VERIFY','NEURAL_PROGRAM'}:err('invented accounting opcode')
        raw={(r['producer_vocabulary'],r['tag'],r['required_mechanism'],r['canonical']) for r in mapping.get('raw_origin_aliases',[])}
        for vocab in ('micode.anea-fixture/1','micode.ace-fixture/1'):
            for tag,mech,canonical in [('TokenLikelihood','candidate_token_scoring','NativeOptionLogit'),('SequenceLikelihood','sequence_scoring','SequenceLikelihood'),('DecisionHeadDistribution','learned_head','LearnedDecisionHead')]:
                if (vocab,tag,mech,canonical) not in raw:err('raw score vocabulary alias missing: '+vocab+'/'+tag)
        pr={p['id']:p for p in load('release_profiles.json')['profiles']}
        seen=set()
        def visit(t):
            if t in seen:return
            seen.add(t)
            for d in td.get(t,{}).get('depends_on',[]):visit(d)
        for t in pr['ace_core']['exit_tasks']:visit(t)
        if seen & {'B157','B159','B164','B181','B184','B185','B186'}:err('optional research on core critical path')
        ownerlock=load('integration/ACE_OWNER_EXPORT_LOCK.json')
        if ownerlock['profile']!='axon.ace-execution/1' or ownerlock['product_status']!='NOT_RUN':err('export lock false profile/status')
        for r in ownerlock['files']:
            if hash_path(r['path'])!=r['sha256']:err('owner export changed: '+r['path'])
    except (OSError,ValueError,KeyError,TypeError,RecursionError) as e:err('malformed integration metadata: '+str(e))
    return errors
