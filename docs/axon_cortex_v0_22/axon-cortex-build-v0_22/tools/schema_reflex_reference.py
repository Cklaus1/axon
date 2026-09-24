"""Model-free v0.20 conformance reference, NOT a production compiler or executor.

Only the explicitly documented subset is modeled. No network, tool execution,
model loading, training, signatures or admission. Digests are fixture-local JSON.
"""
from __future__ import annotations
import hashlib
import json
import math
import re
from typing import Any, Iterable

PROFILE = 'schema-reflex-reference/1'
GRID = (0, .5, .6, .7, .8, .85, .9, .925, .95, .975, .99, .995, 1)
ROLES = {'train', 'selection', 'calibration', 'acceptance', 'test'}
DANGEROUS = {'__proto__', 'prototype', 'constructor'}
BINDING_KEYS = frozenset({'task_digest', 'label_map_digest', 'input_projection_digest',
    'preprocessing_digest', 'encoder_digest', 'tokenizer_digest', 'head_digest',
    'precision', 'runtime', 'device_class', 'batch_profile', 'calibration_ref',
    'event', 'domain', 'generation'})
JOB_KEYS = frozenset({'task_digest', 'teacher_revision', 'endpoint', 'tenant', 'project',
    'principal', 'grant_ref', 'data_policy_digest', 'retention_policy_digest',
    'corpus_digest', 'partition', 'evaluator_epoch', 'budget_ref'})

class Refusal(ValueError):
    """Explicit invalid/unsupported fixture; never coerced to a result."""

def canonical(value: Any) -> str:
    try:
        return json.dumps(value, sort_keys=True, ensure_ascii=False,
                          allow_nan=False, separators=(',', ':'))
    except (ValueError, TypeError, RecursionError) as e:
        raise Refusal('invalid fixture JSON') from e

def digest(value: Any) -> str:
    return hashlib.sha256(canonical(value).encode()).hexdigest()

def _finite(value: Any) -> bool:
    try:
        return type(value) in (int, float) and math.isfinite(value)
    except (OverflowError, TypeError):
        return False

def _text(value: Any) -> bool:
    return isinstance(value, str) and bool(value)

def parse_json(text: str) -> Any:
    if not isinstance(text, str) or len(text.encode()) > 65536:
        raise Refusal('oversized/nontext JSON')
    def pairs(items):
        out = {}
        for key, value in items:
            if key in out or key in DANGEROUS:
                raise Refusal('duplicate/unsafe JSON key')
            out[key] = value
        return out
    try:
        result = json.loads(text, object_pairs_hook=pairs,
                            parse_constant=lambda _: (_ for _ in ()).throw(Refusal('nonfinite JSON')))
    except (ValueError, TypeError, RecursionError) as e:
        raise Refusal('invalid bounded JSON') from e
    _bounded(result)
    return result

def _bounded(value: Any) -> None:
    stack = [(value, 0)]; count = 0
    while stack:
        obj, depth = stack.pop(); count += 1
        if depth > 12 or count > 8192:
            raise Refusal('structure budget exceeded')
        if isinstance(obj, dict):
            if any(not isinstance(k, str) or k in DANGEROUS for k in obj):
                raise Refusal('unsafe property')
            stack.extend((v, depth+1) for v in obj.values())
        elif isinstance(obj, list):
            stack.extend((v, depth+1) for v in obj)
        elif isinstance(obj, str):
            if len(obj) > 4096:
                raise Refusal('descriptor string budget exceeded')
        elif obj is not None and type(obj) not in (bool, int, float):
            raise Refusal('non-JSON value')
        elif type(obj) in (int, float) and not _finite(obj):
            raise Refusal('nonfinite value')
    if len(canonical(value).encode()) > 65536:
        raise Refusal('manifest byte budget exceeded')

def _identifier(value: Any) -> bool:
    return isinstance(value, str) and bool(re.fullmatch(r'[A-Za-z_][A-Za-z0-9_-]{0,63}', value)) and value not in DANGEROUS

def _matches(value: Any, kind: str) -> bool:
    return {'string': lambda: isinstance(value, str),
            'boolean': lambda: type(value) is bool,
            'integer': lambda: type(value) is int,
            'number': lambda: _finite(value)}.get(kind, lambda: False)()

def _domain(schema: dict) -> list | None:
    if not isinstance(schema, dict):
        raise Refusal('invalid property schema')
    annotations = {'title', 'description'}
    kind = schema.get('type')
    if kind not in {'string', 'boolean', 'integer', 'number'}:
        raise Refusal('unsupported property type')
    if any(not isinstance(schema[k], str) for k in annotations if k in schema):
        raise Refusal('invalid annotations')
    if 'enum' in schema or 'const' in schema:
        if not set(schema) <= annotations | {'type', 'enum', 'const'} or ('enum' in schema and 'const' in schema):
            raise Refusal('unsupported enum constraints')
        values = schema.get('enum', [schema.get('const')])
        if not isinstance(values, list) or not 1 <= len(values) <= 256:
            raise Refusal('enum cardinality')
        if not all(_matches(v, kind) for v in values):
            raise Refusal('typed enum mismatch')
        if any(v == prior for i, v in enumerate(values) for prior in values[:i]):
            raise Refusal('duplicate enum value')
        return values
    if kind == 'integer':
        if not set(schema) <= annotations | {'type', 'minimum', 'maximum'}:
            raise Refusal('unsupported integer constraint')
        low, high = schema.get('minimum'), schema.get('maximum')
        if type(low) is not int or type(high) is not int or not 1 <= high-low+1 <= 32:
            raise Refusal('unsupported integer bounds')
        return list(range(low, high+1))
    if not set(schema) <= annotations | {'type'}:
        raise Refusal('unsupported scalar constraint')
    if kind == 'boolean': return [False, True]
    if kind == 'string': return None
    raise Refusal('unbounded number unsupported')

def _question(qid: str, kind: str, values: list, dependency: str,
              projections: list | None = None, include_none: bool = False) -> dict:
    candidates = [{'candidate_id': f'c{i:04d}', 'value': value, 'kind': 'value',
                   'projection': projections[i] if projections else None}
                  for i, value in enumerate(values)]
    if include_none:
        candidates.append({'candidate_id': 'NO_SUITABLE_CANDIDATE', 'kind': 'none',
                           'value': None, 'projection': None})
    return {'id': qid, 'type': kind, 'dependency': dependency,
            'candidates': candidates}

def compile_plan(tools: list[dict], snapshot: str, source: str,
                 span_catalogs: dict | None = None) -> dict:
    """Fixture projection of ArtifactCatalog/questions/composition, without I/O."""
    _bounded(tools)
    if not isinstance(tools, list) or not 1 <= len(tools) <= 32 or not _text(snapshot):
        raise Refusal('tool/snapshot bounds')
    if not isinstance(source, str) or len(source.encode()) > 65536:
        raise Refusal('source budget')
    spans = {} if span_catalogs is None else span_catalogs
    if not isinstance(spans, dict): raise Refusal('span catalog type')
    _bounded(spans)
    if not set(spans) <= {t.get('tool_id') for t in tools if isinstance(t, dict)}:
        raise Refusal('unknown span tool')
    records = {}; questions = {}; source_digest = digest(source)
    for tool in tools:
        if not isinstance(tool, dict) or not set(tool) <= {'tool_id', 'schema', 'description', 'annotations'}:
            raise Refusal('unsupported tool descriptor')
        tid = tool.get('tool_id')
        if not _identifier(tid) or tid in records:
            raise Refusal('duplicate/invalid tool identity')
        schema = tool.get('schema')
        if not isinstance(schema, dict) or not set(schema) <= {'type', 'properties', 'required', 'additionalProperties', 'title', 'description'}:
            raise Refusal('unsupported object keywords')
        if schema.get('type') != 'object' or schema.get('additionalProperties') is not False:
            raise Refusal('closed object schema required')
        props, required = schema.get('properties'), schema.get('required', [])
        if not isinstance(props, dict) or len(props) > 32 or not all(_identifier(k) for k in props):
            raise Refusal('property bounds/identity')
        if not isinstance(required, list) or not all(isinstance(k, str) for k in required) or len(required) != len(set(required)) or not set(required) <= set(props):
            raise Refusal('required field mismatch')
        if any(not isinstance(schema[k], str) for k in ('title', 'description') if k in schema):
            raise Refusal('invalid object annotation')
        tool_spans = spans.get(tid, {})
        if not isinstance(tool_spans, dict) or not set(tool_spans) <= set(props):
            raise Refusal('unknown/invalid span field')
        fields = []
        for name, sub in props.items():
            values = _domain(sub); qid = tid+'.'+name
            presence = None
            if name not in required:
                presence = qid+'.present'
                questions[presence] = _question(presence, 'BinaryDecision', [False, True], 'ConditionallyRelevant')
            projections = None; include_none = values is None
            if include_none:
                entries = tool_spans.get(name, [])
                if not isinstance(entries, list) or len(entries) > 32: raise Refusal('span cardinality')
                values = []; projections = []; seen = set()
                for item in entries:
                    if not isinstance(item, dict) or set(item) != {'start', 'end', 'offset_unit', 'source_digest', 'source_version'}:
                        raise Refusal('span shape')
                    if item['offset_unit'] != 'unicode_codepoint' or item['source_digest'] != source_digest or item['source_version'] != snapshot:
                        raise Refusal('span source/unit identity')
                    start, end = item['start'], item['end']
                    if type(start) is not int or type(end) is not int or not 0 <= start < end <= len(source):
                        raise Refusal('span offsets')
                    if (start, end) in seen: raise Refusal('duplicate span')
                    seen.add((start, end)); values.append(source[start:end])
                    projections.append({'source_digest': source_digest, 'source_version': snapshot,
                        'offset_unit': 'unicode_codepoint', 'start': start, 'end': end,
                        'projection_rule_id': 'exact-codepoint-copy/1'})
            kind = 'BinaryDecision' if sub['type'] == 'boolean' and len(values) == 2 else 'Choice'
            questions[qid] = _question(qid, kind, values, 'ConditionallyRelevant', projections, include_none)
            fields.append({'name': name, 'qid': qid, 'presence_qid': presence, 'type': sub['type']})
        records[tid] = {'schema_digest': digest(schema), 'descriptor_digest': digest(tool), 'fields': fields}
    questions['route'] = _question('route', 'Choice', list(records), 'Independent', include_none=True)
    plan = {'fixture_only': True, 'profile': PROFILE, 'snapshot_digest': snapshot,
            'source_digest': source_digest, 'tools': records, 'questions': questions,
            'catalog_digest': digest(questions)}
    plan['plan_digest'] = digest(plan)
    return plan

def answer_for(plan: dict, qid: str, value: Any, *, none: bool = False) -> dict:
    """Construct a synthetic deterministic response, not model confidence."""
    candidates = plan['questions'][qid]['candidates']
    found = [c for c in candidates if (c['kind'] == 'none' if none else c['kind'] == 'value' and type(c['value']) is type(value) and c['value'] == value)]
    if len(found) != 1: raise Refusal('answer not uniquely in fixture catalog')
    selected = found[0]['candidate_id']
    return {'selected_id': selected, 'probabilities': {c['candidate_id']: float(c['candidate_id'] == selected) for c in candidates}}

def envelope(plan: dict, answers: dict) -> dict:
    return {'plan_digest': plan['plan_digest'], 'snapshot_digest': plan['snapshot_digest'],
            'catalog_digest': plan['catalog_digest'], 'answers': answers}

def _selected(question: dict, answers: dict) -> dict:
    raw = answers.get(question['id'])
    if not isinstance(raw, dict) or set(raw) != {'selected_id', 'probabilities'}:
        raise Refusal('missing/malformed active answer')
    candidates = {c['candidate_id']: c for c in question['candidates']}
    probabilities = raw['probabilities']
    if raw['selected_id'] not in candidates or not isinstance(probabilities, dict) or set(probabilities) != set(candidates):
        raise Refusal('answer candidate identity mismatch')
    if any(not _finite(p) or not 0 <= p <= 1 for p in probabilities.values()) or abs(sum(probabilities.values())-1) > 1e-8:
        raise Refusal('invalid active distribution')
    return candidates[raw['selected_id']]

def decode(plan: dict, response: dict, *, snapshot: str, source: str,
           registry: dict) -> dict:
    """Return a proposed composition only; registry is trusted fixture input."""
    if plan.get('profile') != PROFILE or plan.get('fixture_only') is not True:
        raise Refusal('unsupported plan')
    if digest({k:v for k,v in plan.items() if k != 'plan_digest'}) != plan.get('plan_digest'):
        raise Refusal('plan modified')
    if plan['snapshot_digest'] != snapshot or plan['source_digest'] != digest(source):
        raise Refusal('stale snapshot/source')
    if not isinstance(response, dict) or any(response.get(k) != plan[k] for k in ['plan_digest', 'snapshot_digest', 'catalog_digest']):
        raise Refusal('stale result identity')
    answers = response.get('answers')
    if not isinstance(answers, dict) or not set(answers) <= set(plan['questions']):
        raise Refusal('unknown answer identity')
    route = _selected(plan['questions']['route'], answers)
    base = {'fixture_only': True, 'model_executed': False, 'execution_authorized': False,
            'executed': False, 'verified_complete': False, 'joint_correctness': None,
            'plan_digest': plan['plan_digest'], 'snapshot_digest': snapshot,
            'source_digest': digest(source)}
    if route['kind'] == 'none': return {**base, 'status': 'NoSuitableCandidate'}
    tid = route['value']; registered = registry.get(tid)
    if not isinstance(registered, dict) or registered.get('schema_digest') != plan['tools'][tid]['schema_digest'] or registered.get('descriptor_digest') != plan['tools'][tid]['descriptor_digest']:
        raise Refusal('stale/unregistered tool')
    args = {}; provenance = {}; active = {'route'}
    for field in plan['tools'][tid]['fields']:
        presence = field['presence_qid']
        if presence:
            active.add(presence)
            stated = _selected(plan['questions'][presence], answers)
            if type(stated['value']) is not bool: raise Refusal('invalid presence')
            if not stated['value']: continue
        qid = field['qid']; active.add(qid)
        selected = _selected(plan['questions'][qid], answers)
        if selected['kind'] == 'none':
            return {**base, 'status': 'NoSuitableCandidate', 'missing_field': field['name']}
        value, projection = selected['value'], selected['projection']
        if projection:
            if projection['source_digest'] != digest(source) or projection['source_version'] != snapshot:
                raise Refusal('stale span')
            value = source[projection['start']:projection['end']]
            if value != selected['value']: raise Refusal('changed span projection')
        if not _matches(value, field['type']): raise Refusal('final argument type mismatch')
        args[field['name']] = value
        provenance[field['name']] = projection or {'kind': 'declared_typed_value', 'candidate_id': selected['candidate_id'], 'catalog_digest': plan['catalog_digest']}
    for constraint in registered.get('constraints', []):
        if not isinstance(constraint, dict) or set(constraint) != {'kind', 'when', 'requires'} or constraint['kind'] != 'require_if':
            raise Refusal('unsupported trusted constraint')
        when, required = constraint['when'], constraint['requires']
        if not isinstance(when, dict) or not isinstance(required, list): raise Refusal('constraint shape')
        if all(k in args and type(args[k]) is type(v) and args[k] == v for k,v in when.items()) and any(k not in args for k in required):
            raise Refusal('complete invocation constraint failed')
    return {**base, 'status': 'CompleteComposition', 'tool_id': tid, 'args': args,
            'schema_digest': registered['schema_digest'], 'descriptor_digest': registered['descriptor_digest'],
            'provenance': provenance, 'active_question_ids': sorted(active),
            'unused_question_ids': sorted(set(answers)-active)}

def validate_binding(expected: dict, actual: dict) -> None:
    if not isinstance(expected, dict) or not isinstance(actual, dict) or set(expected) != BINDING_KEYS or set(actual) != BINDING_KEYS:
        raise Refusal('incomplete qualification identity')
    if not all(_text(v) for v in expected.values()) or expected != actual:
        raise Refusal('unqualified task/label/runtime/calibration identity')

def route_recommendation(probability: float, recommendation: dict, expected: dict,
                         actual: dict, *, ood: str, revoked: bool) -> str:
    validate_binding(expected, actual)
    if type(revoked) is not bool or not _finite(probability) or not 0 <= probability <= 1:
        raise Refusal('invalid routing input')
    if revoked or ood != 'qualified_in_domain': return 'FALLBACK'
    threshold = recommendation.get('threshold')
    if recommendation.get('status') != 'ready' or threshold is None: return 'FALLBACK'
    if not _finite(threshold) or not 0 <= threshold <= 1: raise Refusal('invalid threshold')
    # Even this result is only a proposal, not permission to execute.
    return 'ELIGIBLE_PROPOSAL' if probability >= threshold else 'FALLBACK'

def validate_partitions(rows: list[dict]) -> dict:
    ids = set(); inputs = {}; groups = {}; counts = {r:0 for r in ROLES}
    for row in rows:
        if not isinstance(row, dict) or any(not _text(row.get(k)) for k in ['id','input_digest','group','partition','label_digest','label_kind']):
            raise Refusal('incomplete dataset record')
        rid, inp, group, role = (row[k] for k in ['id','input_digest','group','partition'])
        if rid in ids or role not in ROLES or row['label_kind'] not in {'teacher','reviewed_human','verified_outcome'}:
            raise Refusal('invalid dataset identity/provenance')
        if row.get('protected') is True and role != 'test': raise Refusal('protected label leakage')
        if group in groups and groups[group] != role: raise Refusal('cross-partition group leakage')
        if inp in inputs and inputs[inp][0] != role: raise Refusal('cross-partition duplicate leakage')
        if inp in inputs and inputs[inp][1] != row['label_digest']: raise Refusal('conflicting labels require adjudication')
        ids.add(rid); groups[group] = role
        if inp not in inputs: counts[role] += 1
        inputs[inp] = (role, row['label_digest'])
    return {'fixture_only': True, 'unique_rows_by_role': counts, 'independence_proved': False}

def teacher_key(job: dict, input_digest: str) -> str:
    if not isinstance(job, dict) or set(job) != JOB_KEYS or not all(_text(v) for v in job.values()) or job['partition'] not in ROLES or not _text(input_digest):
        raise Refusal('incomplete teacher job scope')
    return digest({'job': job, 'input_digest': input_digest})

def committed_labels(log: str, *, job_key: str) -> dict:
    """Model newline commit semantics only; does not implement real fs locking."""
    if not isinstance(log, str) or not _text(job_key): raise Refusal('invalid log identity')
    committed = log[:log.rfind('\n')+1]; records = {}
    for line in committed.splitlines():
        record = parse_json(line)
        if not isinstance(record, dict) or set(record) != {'job_key','input_digest','label_digest'} or record.get('job_key') != job_key or not all(_text(v) for v in record.values()):
            raise Refusal('invalid committed label')
        key = record['input_digest']
        if key in records: raise Refusal('duplicate committed identity')
        records[key] = record['label_digest']
    return records

def lower_bound(correct: int, n: int, alpha: float) -> float:
    """One-sided exact binomial lower confidence bound via monotonic bisection."""
    if type(n) is not int or type(correct) is not int or not 0 <= correct <= n or not _finite(alpha) or not 0 < alpha < 1:
        raise Refusal('invalid binomial inputs')
    if correct == 0: return 0.0
    if correct == n: return math.exp(math.log(alpha)/n)
    coefficients = [(i, math.lgamma(n+1)-math.lgamma(i+1)-math.lgamma(n-i+1)) for i in range(correct,n+1)]
    def tail(p):
        if p <= 0: return 0.0
        if p >= 1: return 1.0
        terms = [c+i*math.log(p)+(n-i)*math.log1p(-p) for i,c in coefficients]
        m = max(terms)
        return min(1.0, math.exp(m)*sum(math.exp(t-m) for t in terms))
    low, high = 0.0, 1.0
    for _ in range(75):
        mid = (low+high)/2
        if tail(mid) < alpha: low = mid
        else: high = mid
    return (low+high)/2

def recommend(rows: list[dict], *, target: float = .95, alpha: float = .05,
              comparisons: int = 13, min_accepted: int = 30) -> dict:
    if not _finite(target) or not 0 < target <= 1 or not _finite(alpha) or not 0 < alpha < 1 or type(comparisons) is not int or comparisons < len(GRID) or type(min_accepted) is not int or min_accepted < 30:
        raise Refusal('invalid/underbudgeted acceptance policy')
    ids = set(); representatives = {}
    for row in rows:
        if not isinstance(row, dict) or not _text(row.get('id')) or not _text(row.get('group')) or type(row.get('correct')) is not bool or not _finite(row.get('probability')) or not 0 <= row['probability'] <= 1:
            raise Refusal('invalid acceptance observation')
        if row['id'] in ids: raise Refusal('duplicate acceptance id')
        ids.add(row['id'])
        group = row['group']
        # ID-only selection; neither correctness nor confidence influences the representative.
        if group not in representatives or row['id'] < representatives[group]['id']:
            representatives[group] = row
    selected = list(representatives.values()); candidates = []
    for threshold in GRID:
        accepted = [r for r in selected if r['probability'] >= threshold]
        n = len(accepted); k = sum(r['correct'] for r in accepted)
        lb = lower_bound(k,n,alpha/comparisons)
        candidates.append({'threshold': threshold, 'accepted': n, 'correct': k,
            'coverage': n/len(selected) if selected else 0.0, 'lower_bound': lb,
            'qualifies': n >= min_accepted and lb >= target})
    valid = [r for r in candidates if r['qualifies']]
    best = max(valid, key=lambda r:(r['coverage'], -r['threshold'])) if valid else None
    return {'fixture_only': True, 'threshold': best['threshold'] if best else None,
        'status': 'ready' if best else ('insufficient_data' if len(selected)<min_accepted else 'target_not_met'),
        'targetAccuracy': target, 'event': 'synthetic_reference_label_agreement',
        'representatives': len(selected), 'all_rows': len(rows), 'comparisons': comparisons,
        'independence_proved': False, 'candidates': candidates}
