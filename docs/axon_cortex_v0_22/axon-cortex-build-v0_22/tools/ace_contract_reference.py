"""Inert Axon ACE record-conformance reference. No APIs, models, effects or grants.

Validates a bounded projection and selected semantic bindings. A valid record is
not authenticated, admitted, accurate or safe to execute. Real host/runtime owners
must validate current identities, permissions, effect and storage boundaries.
"""
from __future__ import annotations
import hashlib, json, math
from pathlib import Path
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[1]
PROFILE = 'axon.ace-projection/1'
class ContractError(ValueError): pass

def require(ok, message):
    if not ok: raise ContractError(message)

def parse_json(raw: str | bytes, *, max_bytes=2*1024*1024, max_depth=32):
    require(type(raw) in (str,bytes),'expected JSON bytes/text')
    if isinstance(raw,bytes):
        require(len(raw)<=max_bytes,'JSON too large')
        try: raw=raw.decode('utf-8')
        except UnicodeError as e: raise ContractError('invalid UTF-8') from e
    require(len(raw.encode('utf-8'))<=max_bytes,'JSON too large')
    def pairs(xs):
        out={}
        for k,v in xs:
            require(k not in out,'duplicate JSON key');out[k]=v
        return out
    def constant(x): raise ContractError('nonfinite JSON constant')
    try: out=json.loads(raw,object_pairs_hook=pairs,parse_constant=constant)
    except (ValueError,RecursionError) as e: raise ContractError(str(e)) from e
    stack=[(out,0)]
    while stack:
        value,depth=stack.pop();require(depth<=max_depth,'JSON too deep')
        if isinstance(value,float):require(math.isfinite(value),'nonfinite JSON value')
        if isinstance(value,dict):stack.extend((v,depth+1) for v in value.values())
        if isinstance(value,list):stack.extend((v,depth+1) for v in value)
    return out

def canonical_digest(value):
    try:return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False,allow_nan=False).encode()).hexdigest()
    except (ValueError,TypeError) as e:raise ContractError('invalid digest input') from e

def validate_record(value, *, schema_path: Path | None=None):
    # Reparse to check finite values and parser constraints even for in-memory input.
    try: value=parse_json(json.dumps(value,allow_nan=False))
    except (ValueError,TypeError) as e:raise ContractError('non-JSON record') from e
    schema=parse_json((schema_path or ROOT/'schemas/json/ace-execution.schema.json').read_bytes())
    errors=list(Draft202012Validator(schema).iter_errors(value))
    if errors:raise ContractError('record schema mismatch: '+errors[0].message[:250])
    return value

def validate_profile(p, *, mappings_path: Path | None=None):
    validate_record(p);require(p['record_type']=='PhysicalExecutionProfile','wrong record type')
    m=parse_json((mappings_path or ROOT/'integration/ACE_VOCABULARY_MAP.json').read_bytes())
    row=next((x for x in m['rows'] if x['id']==p['mapping_id']),None)
    require(row is not None,'unregistered host mapping')
    for k in ('air_node_kind','cognitive_class','operation_kind','capability_class'):
        require(p[k]==row[k],'mapping axis mismatch: '+k)
    require(p['physical_mechanism'] in row['mechanisms'],'mechanism incompatible with mapping')
    if p['capability_class']=='NEURAL_PROGRAM':require(p['artifact_ref'] is not None,'Neural Program missing artifact')
    return 'ProposedProfile'

def require_features(p, required, *, backend_ref, deployment, allowed_emulations=()):
    validate_record(p);require(p['record_type']=='BackendFeatureProjection','wrong record type')
    require(p['backend_ref']==backend_ref and p['deployment_mode']==deployment,'feature deployment mismatch')
    for name,f in p['features'].items():
        if f['status'] in {'Supported','Emulated'}:require(f['evidence_ref'] is not None,'support lacks scoped evidence reference')
        if f['status']=='Emulated':require(f['emulation_ref'] is not None,'hidden emulation')
        else:require(f['emulation_ref'] is None,'unexpected emulation metadata')
    for name in required:
        f=p['features'].get(name);require(f is not None,'unknown required feature')
        require(f['status']=='Supported' or (f['status']=='Emulated' and f['emulation_ref'] in allowed_emulations),'feature unavailable: '+name)
    return 'ProposedFeatureMatch' # evidence references still require authenticated resolution

def validate_request(q):
    validate_record(q);require(q['record_type']=='DecisionRequest','wrong request type')
    ids=[c['id'] for c in q['candidates']]
    require(len(ids)==len(set(ids)),'duplicate candidates')
    require(q['candidate_set_digest']==canonical_digest(sorted(q['candidates'],key=lambda c:c['id'])),'candidate set digest mismatch')
    require(q['candidate_order_digest']==canonical_digest(ids),'candidate order digest mismatch')
    f=q['family']
    if f=='BinaryProbability':
        require(not ids and q['event_ref'] is not None and not q['levels'],'invalid binary event/candidates')
        require(q['selection_policy']=='distribution_only' and q['tie_rule']=='not_applicable','binary selection semantics')
    else:
        require(len(ids)>=2 and q['event_ref'] is None,'invalid candidate family')
        if f=='OrdinalDistribution':
            require([v['id'] for v in q['levels']]==ids,'ordinal IDs/order differ')
            values=[v['value'] for v in q['levels']]
            require(all(a<b for a,b in zip(values,values[1:])),'ordinal values not increasing')
            require(q['requires_complete_scores'] and q['selection_policy']=='distribution_only','ordinal distribution required')
        else:require(not q['levels'],'choice has ordinal levels')
    if q['selection_policy']=='argmax':require(q['tie_rule']=='first_declared' and q['requires_complete_scores'],'argmax requires complete/tie convention')
    else:require(q['tie_rule']=='not_applicable','irrelevant tie convention')
    return ids

def validate_uncertainty(c):
    if c['status']=='Estimated':
        require(c['value'] is not None and all(c[k] is not None for k in ('event_ref','domain_ref','estimator_ref','evidence_ref')),'unnamed/ungrounded correctness estimate')
    else:require(c['value'] is None,'unavailable/inapplicable correctness has usable number')

def validate_result(q,r,p=None):
    ids=validate_request(q);validate_record(r);require(r['record_type']=='DecisionResult','wrong result type')
    for k in ('request_id','operation_id','question_id','family','observation_ref','effective_input_ref','candidate_set_digest','candidate_order_digest'):
        require(q[k]==r[k],'request/result binding mismatch: '+k)
    validate_uncertainty(r['correctness'])
    if r['outcome']!='Value':
        require(r['absence_reason'] is not None,'non-value outcome needs reason')
        require(all(r[k] is None for k in ('selected_id','distribution','probability','expectation','score_provenance')),'fabricated value for non-value outcome')
        require(r['score_completeness']=='unavailable','non-value score availability')
        require(r['correctness']['status']!='Estimated','correctness attached to absent selection')
        return 'NoValue'
    require(r['absence_reason'] is None,'value has absence reason')
    if q['family']=='BinaryProbability':
        require(r['event_ref']==q['event_ref'] and r['probability'] is not None,'missing/wrong binary event')
        require(r['selected_id'] is None and r['distribution'] is None and r['expectation'] is None and not r['levels'],'binary coerced to another family')
        require(r['score_completeness']=='complete','binary probability unavailable')
    else:
        require(r['event_ref'] is None and r['probability'] is None,'cross-family probability')
        if q['family']=='Choice':
            require(not r['levels'] and r['expectation'] is None,'choice coerced to ordinal')
            if q['selection_policy']=='distribution_only':require(r['selected_id'] is None,'distribution-only choice has selection')
            else:require(r['selected_id'] in ids,'selected candidate missing')
        else:require(r['levels']==q['levels'] and r['selected_id'] is None,'ordinal levels/selected value mismatch')
        dist=r['distribution']
        if dist is None:
            require(q['family']=='Choice' and not q['requires_complete_scores'] and q['selection_policy']=='provider_choice','required distribution missing')
            require(r['score_completeness']=='unavailable' and r['score_provenance'] is None,'invented score metadata')
            return 'ProposedLabel'
        ps={x['id']:x['p'] for x in dist};require(len(ps)==len(dist) and set(ps)<=set(ids),'duplicate/unknown probability IDs')
        complete=r['score_completeness']=='complete'
        require(r['score_completeness']!='unavailable','distribution with unavailable scores')
        if complete:require(set(ps)==set(ids) and abs(sum(ps.values())-1)<=1e-6,'incomplete/non-normalized full distribution')
        else:require(sum(ps.values())<=1+1e-6,'partial distribution exceeds mass')
        if q['requires_complete_scores']:require(complete,'required complete scores missing')
        if q['selection_policy']=='argmax':
            require(complete,'argmax of partial scores')
            require(r['selected_id']==max(ids,key=lambda i:ps[i]),'argmax or tie mismatch')
        if q['family']=='OrdinalDistribution' and r['expectation'] is not None:
            require(complete and abs(r['expectation']-sum(ps[v['id']]*v['value'] for v in q['levels']))<=1e-6,'incorrect ordinal expectation')
    origin=r['score_provenance'];require(origin is not None,'probability lacks origin')
    require(origin['evidence_ref'] is not None,'scores lack source evidence reference')
    if q['selection_policy']=='argmax':require(origin['origin']!='GeneratedEstimate','generated estimate cannot satisfy genuine-score argmax')
    if p is not None:
        validate_profile(p)
        origins={'NativeOptionLogit':{'candidate_token_scoring'},'SequenceLikelihood':{'sequence_scoring'},'LearnedDecisionHead':{'learned_head'},'GeneratedEstimate':{'structured_generation','validated_text'}}
        if origin['origin'] in origins:require(p['physical_mechanism'] in origins[origin['origin']],'origin/mechanism laundering')
    return 'ProposedDecision' # never VerifiedComplete

def state_reusable(a,b,*,authorization_current=False,expired=True):
    validate_record(a);validate_record(b)
    require(a['record_type']==b['record_type']=='StateHandleReceipt','wrong state type')
    return (authorization_current is True and expired is False and a['kind']==b['kind']=='native_state' and a['reuse_mode']==b['reuse_mode']=='actual' and a['native_handle_ref'] is not None and b['native_handle_ref'] is not None and a['epoch']==b['epoch'] and a['bindings']==b['bindings'] and a['expiry_ref']==b['expiry_ref'])

def join(plan,results,*,active_branch,allow_unused_failure=False):
    """Synthetic join only. Plan/result refs are not authenticated grants."""
    ids=[q['id'] for q in plan];require(len(ids)==len(set(ids)),'duplicate plan ID')
    byid={q['id']:q for q in plan};rids=[r['id'] for r in results]
    require(len(rids)==len(set(rids)) and set(rids)<=set(ids),'unknown/duplicate result')
    require(active_branch in {q['branch'] for q in plan},'unknown active branch')
    visited=set();visiting=set()
    def visit(i):
        require(i in byid,'unknown dependency');require(i not in visiting,'dependency cycle')
        if i in visited:return
        visiting.add(i);q=byid[i]
        require(q['dependency'] in {'Independent','ConditionallyRelevant','AnswerDependent'},'unknown dependency kind')
        if q['dependency']!='AnswerDependent':require(not q['depends_on'],'nondependent question consumes sibling answer')
        for d in q['depends_on']:visit(d)
        visiting.remove(i);visited.add(i)
    for i in ids:visit(i)
    rs={r['id']:r for r in results};active={q['id'] for q in plan if q['branch'] in {'all',active_branch}}
    terminal={'Value','Abstained','Refused','Failed','Canceled','DeadlineExceeded','OutcomeUnknown','Unsupported'}
    for r in results:require(r['outcome'] in terminal,'unknown outcome')
    for q in plan:
        r=rs.get(q['id'])
        if q['id'] in active:
            require(r is not None and r['outcome']=='Value' and r['effective_input_ref']==q['effective_input_ref'],'missing/failed/stale active field')
            for d in q['depends_on']:
                require(d in active and rs.get(d,{}).get('outcome')=='Value','inactive/uncommitted parent')
                require(r['committed_dependencies'].get(d)==rs[d]['result_ref'],'wrong committed dependency result')
            require(set(r['committed_dependencies'])==set(q['depends_on']),'undeclared parent inputs')
        elif r is not None and r['outcome']!='Value':require(allow_unused_failure,'unused failure policy absent')
    return 'ProposedComposition'

def validate_attempt_stream(events,*,root_id,budget):
    require(type(budget) in (int,float) and math.isfinite(budget) and budget>=0,'invalid budget')
    starts={};terminal=set();reserved=0
    for e in events:
        validate_record(e);require(e['record_type']=='ExecutionAttemptEvent' and e['root_id']==root_id,'attempt/root mismatch')
        aid=e['attempt_id'];kind=e['kind']
        for k in ['reservation_upper_bound','actual_cost']:
            require(e[k] is None or e[k]>=0,'negative resource measure')
        if kind=='Start':
            require(aid not in starts,'duplicate attempt')
            require(e['parent_attempt_id'] is None or e['parent_attempt_id'] in starts,'unknown parent attempt')
            require(e['requested_producer_ref'] is not None and e['selected_producer_ref'] is not None,'missing selection identity')
            require(e['outcome'] is None and not e['committed'] and e['accepted_producer_ref'] is None and e['observed_producer_ref'] is None and e['result_ref'] is None,'fabricated start outcome')
            require(e['reservation_upper_bound'] is not None,'unknown hard budget bound')
            reserved+=e['reservation_upper_bound'];require(reserved<=budget,'root reservation exceeded');starts[aid]=e
        elif kind=='Terminal':
            require(aid in starts and aid not in terminal,'duplicate/unstarted terminal')
            for k in ['effective_input_ref','parent_attempt_id','selected_producer_ref','requested_producer_ref']:
                require(e[k]==starts[aid][k],'attempt identity changed')
            require(e['outcome'] is not None,'missing terminal outcome')
            if e['actual_cost'] is not None:require(e['actual_cost']<=starts[aid]['reservation_upper_bound'],'actual cost exceeds reserved upper bound')
            if e['outcome']=='Value':
                require(e['committed'] and e['result_ref'] is not None and e['observed_producer_ref'] is not None and e['accepted_producer_ref']==e['observed_producer_ref'],'invalid accepted producer')
                require(e['remote_status']=='Finished','accepted output while remote work unresolved')
            else:require(not e['committed'] and e['accepted_producer_ref'] is None,'non-value committed as success')
            terminal.add(aid)
        else:
            require(aid in terminal and not e['committed'] and e['accepted_producer_ref'] is None,'late result committed')
    return 'ValidInertAttemptTrace'
