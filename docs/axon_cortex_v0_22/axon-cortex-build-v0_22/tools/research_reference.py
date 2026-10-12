"""Model-free v0.21 executable contract examples, NOT a runtime or authority.

These fixtures reject selected invalid records and demonstrate deterministic
reference semantics. They do not prove provider quality, authenticate principals,
execute tools, implement statistical inference, or satisfy product gates.
All serialized numbers use integer micro/ppm units under existing axon.cjson/1.
"""
from __future__ import annotations
import hashlib
import math
from dataclasses import dataclass, field
from fractions import Fraction
from pathlib import Path
from typing import Any
import json
from jsonschema import Draft202012Validator
from neural_contract_reference import canonical, parse, ContractError

ROOT = Path(__file__).resolve().parents[1]
PPM = 1_000_000


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ContractError(message)


def uint(value: Any, name: str) -> int:
    require(type(value) is int and value >= 0, f'{name}: nonnegative integer required')
    return value


def digest(value: Any) -> str:
    return hashlib.sha256(canonical(value)).hexdigest()


def ids(rows: list[dict]) -> list[str]:
    names = [r['id'] for r in rows]
    require(bool(names) and all(isinstance(x, str) and x for x in names), 'nonempty IDs')
    require(len(names) == len(set(names)), 'duplicate ID')
    return names


def candidate_set_digest(candidates: list[dict]) -> str:
    ids(candidates)
    return digest(sorted(candidates, key=lambda c: c['id']))


def pipeline_binding(record: dict) -> str:
    """Conservative fixed-candidate-domain qualification, not a learned calibrator."""
    return digest({'pipeline': record['pipeline'],
                   'candidate_set_sha256': record['candidate_set_sha256'],
                   'domain_id': record['domain_id']})


def cache_key(record: dict, text_sha256: str, side: str) -> str:
    require(side in ('state', 'action'), 'unknown embedding side')
    require(len(text_sha256) == 64 and all(c in '0123456789abcdef' for c in text_sha256), 'text digest')
    return digest({'principal': record['principal'], 'retention_scope': record['retention_scope'],
                   'pipeline': record['pipeline'], 'domain': record['domain_id'],
                   'candidates': record['candidate_set_sha256'], 'side': side, 'text': text_sha256})


def distribution(values: dict[str, int], names: list[str]) -> None:
    require(set(values) == set(names), 'distribution labels differ')
    for value in values.values():
        require(uint(value, 'probability') <= PPM, 'probability above one')
    require(sum(values.values()) == PPM, 'distribution must sum to one million ppm')


def validate_decision(record: dict, *, expected_principal: str | None = None,
                      expected_epoch: int | None = None) -> dict:
    schema = json.loads((ROOT / 'schemas/json/research-decision.schema.json').read_text())
    errors = sorted(Draft202012Validator(schema).iter_errors(record), key=lambda e: str(e.path))
    require(not errors, 'decision schema: ' + (errors[0].message if errors else ''))
    require(record['fixture_only'] is True, 'reference record must stay a fixture')
    require(record['product_result'] == 'NOT_RUN', 'fixture cannot claim a product result')
    if expected_principal is not None:
        require(record['principal'] == expected_principal, 'principal mismatch')
    if expected_epoch is not None:
        require(record['pipeline']['epoch'] == expected_epoch, 'head epoch mismatch')
    names = ids(record['candidates'])
    require(record['candidate_set_sha256'] == candidate_set_digest(record['candidates']), 'candidate digest mismatch')
    require(record['presentation_sha256'] == digest(names), 'presentation digest mismatch')
    usage = record['usage']
    if usage['status'] == 'UNKNOWN':
        require(usage['observed_micro'] is None, 'unknown usage cannot be zero or observed')
        require(usage['reserved_micro'] > 0, 'unknown usage needs a positive ceiling')
    else:
        require(type(usage['observed_micro']) is int, 'complete usage needs observed amount')
        require(usage['observed_micro'] <= usage['reserved_micro'], 'usage exceeds reservation')
    if record['status'] != 'DECIDED':
        require(record['chosen_id'] is None and not record['probabilities_ppm']
                and not record['raw_scores_micros'] and record['calibration'] is None,
                'nondecision cannot fabricate scores, choice or confidence')
        return record
    distribution(record['probabilities_ppm'], names)
    require(set(record['raw_scores_micros']) == set(names), 'raw score labels differ')
    best = sorted(names, key=lambda n: (-record['probabilities_ppm'][n], n))[0]
    require(record['chosen_id'] == best, 'choice must respect ranking and stable-ID tie rule')
    cal = record['calibration']
    if cal is not None:
        require(cal['pipeline_sha256'] == pipeline_binding(record), 'stale calibration binding')
        require(cal['domain_id'] == record['domain_id'], 'calibration domain mismatch')
    return record


def round_distribution(values: dict[str, Fraction]) -> dict[str, int]:
    """Largest-remainder rounding with semantic-ID ties; deterministic exact sum."""
    raw = {k: v * PPM for k, v in values.items()}
    base = {k: v.numerator // v.denominator for k, v in raw.items()}
    remaining = PPM - sum(base.values())
    for name in sorted(raw, key=lambda k: (-(raw[k] - base[k]), k))[:remaining]:
        base[name] += 1
    return base


def permutation_average(names: list[str], schedule: list[list[str]],
                        responses: list[dict[str, int]], *, ordinal: bool = False) -> dict[str, int]:
    require(not ordinal, 'ordinal meanings cannot be permuted as categorical choices')
    require(bool(names) and len(set(names)) == len(names), 'candidate IDs must be unique and nonempty')
    require(0 < len(schedule) <= 64, 'bounded permutation schedule required')
    require(len(schedule) == len(responses), 'incomplete permutation responses')
    totals = {n: 0 for n in names}
    for permutation, response in zip(schedule, responses):
        require(len(permutation) == len(names) and set(permutation) == set(names), 'not the same candidate permutation')
        distribution(response, names)
        for name in names:
            totals[name] += response[name]
    return round_distribution({n: Fraction(v, len(schedule) * PPM) for n, v in totals.items()})


def stable_rank(scores: dict[str, int]) -> list[str]:
    require(bool(scores), 'no scores')
    require(all(isinstance(k, str) and k and type(v) is int for k, v in scores.items()), 'integer scores with IDs required')
    return sorted(scores, key=lambda k: (-scores[k], k))


def checklist(rubric: list[dict], results: list[dict]) -> str:
    """The applicability booleans are trusted frozen fixture policy, not judge output."""
    names = ids(rubric)
    result_ids = ids(results)
    require(set(names) == set(result_ids), 'missing or extra checklist criterion')
    byid = {r['id']: r for r in results}
    outcomes = []
    for criterion in rubric:
        require(type(criterion['applicable']) is bool and type(criterion['required']) is bool,
                'rubric flags must be Boolean')
        r = byid[criterion['id']]
        require(r['status'] in ('PASS', 'FAIL', 'UNKNOWN', 'NOT_APPLICABLE'), 'invalid checklist result')
        if not criterion['applicable']:
            require(r['status'] == 'NOT_APPLICABLE', 'inapplicable criterion misrepresented')
        else:
            require(r['status'] != 'NOT_APPLICABLE', 'candidate-controlled N/A')
            if r['status'] in ('PASS', 'FAIL'):
                require(bool(r.get('evidence_ids')), 'claim lacks evidence reference')
            if criterion['required']:
                outcomes.append(r['status'])
    require(bool(outcomes), 'no applicable required criteria; cannot certify success')
    return 'FAIL' if 'FAIL' in outcomes else ('UNKNOWN' if 'UNKNOWN' in outcomes else 'PASS')


def context_projection(events: list[dict], token_budget: int) -> dict:
    """Greedy original-event fixture with dependency closure, not a tokenizer/store."""
    uint(token_budget, 'token budget')
    names = ids(events)
    require(len(events) <= 256, 'reference event limit')
    byid = {e['id']: e for e in events}
    for e in events:
        uint(e['tokens'], 'event token count')
        require(e['kind'] == 'original', 'a prior summary is not canonical input')
        require(type(e['pinned']) is bool, 'pinned must be Boolean')
        require(all(d in byid for d in e['depends_on']), 'missing original dependency')
        require(len(set(e['depends_on'])) == len(e['depends_on']), 'duplicate dependency')
    visiting, memo = set(), {}
    def closure(name: str) -> set[str]:
        require(name not in visiting, 'event dependency cycle')
        if name in memo: return memo[name]
        visiting.add(name)
        found = {name}
        for dependency in byid[name]['depends_on']:
            found |= closure(dependency)
        visiting.remove(name)
        memo[name] = found
        return found
    # Check every event, not only events selected by this particular budget.
    for n in names:
        closure(n)
    selected: set[str] = set()
    for e in events:
        if e['pinned']:
            selected |= closure(e['id'])
    cost = lambda group: sum(byid[n]['tokens'] for n in group)
    require(cost(selected) <= token_budget, 'mandatory closure exceeds context budget')
    for n in reversed(names):
        proposed = selected | closure(n)
        if cost(proposed) <= token_budget:
            selected = proposed
    return {'event_ids': [n for n in names if n in selected], 'tokens': cost(selected),
            'source_sha256': digest(events), 'complete': len(selected) == len(events)}


def eligible_route(candidates: list[dict], scores: dict[str, int], *, principal: str,
                   remaining_micro: int, revoked: set[str] | None = None) -> str:
    names = ids(candidates)
    uint(remaining_micro, 'remaining budget')
    revoked = revoked or set()
    allowed = []
    for c in candidates:
        uint(c['ceiling_micro'], 'route ceiling')
        require(type(c['healthy']) is bool and type(c['compatible']) is bool, 'route flags')
        if (c['id'] not in revoked and principal in c['principals'] and c['healthy']
                and c['compatible'] and c['ceiling_micro'] <= remaining_micro):
            allowed.append(c['id'])
    require(bool(allowed), 'no eligible route')
    # In production the remote scorer receives only this eligible set, not all candidates.
    require(set(scores) == set(allowed), 'scorer scope must equal prefiltered eligible set')
    return stable_rank(scores)[0]


def evolution_screen(proposal: dict, policy: dict) -> str:
    """Fixture gate over trusted policy inputs; not diff analysis or a statistics engine."""
    require(proposal['policy_id'] == policy['id'], 'unregistered acceptance policy')
    require(proposal['incumbent_sha256'] == policy['incumbent_sha256'], 'incumbent drift')
    require(proposal['attempt_id'] not in policy['seen_attempts'], 'reused attempt ID')
    require(proposal['intervention'] == 'harness', 'intervention family requires separate qualification')
    components = proposal['components']
    require(bool(components) and len(components) == len(set(components)), 'empty/duplicate mutation')
    require(set(components) <= set(policy['allowed_components']), 'protected or unknown mutation component')
    require(len(components) <= policy['edit_budget'], 'annealed mutation budget exceeded')
    require(proposal['final_test_exposed'] is False, 'held-out leakage')
    if proposal['critic'] != 'PASS':
        return 'REJECTED'
    uint(proposal['total_cost_micro'], 'cost')
    require(proposal['total_cost_micro'] <= policy['cost_ceiling_micro'], 'experiment exceeds budget')
    if proposal['disposition'] != 'QUALIFIED_COMPARISON':
        return 'INCONCLUSIVE'
    require(proposal['statistical_policy_id'] == policy['statistical_policy_id'], 'selection method drift')
    # An independent upstream estimator supplies this qualified lower bound; no CI is computed here.
    lower = proposal['quality_delta_lower_ppm']
    require(type(lower) is int, 'qualified lower bound required')
    return 'CANDIDATE_FOR_INDEPENDENT_ADMISSION' if lower >= policy['min_delta_ppm'] else 'REJECTED'


@dataclass
class Cascade:
    """Single-branch reference, not production concurrency or authorization."""
    ceiling_micro: int
    phase: str = 'CREATED'
    spent_micro: int = 0
    candidate_sha256: str | None = None
    verified_sha256: str | None = None
    calls: set[str] = field(default_factory=set)

    def __post_init__(self):
        require(uint(self.ceiling_micro, 'ceiling') > 0, 'positive ceiling required')

    def reserve(self):
        require(self.phase == 'CREATED', 'already reserved or terminal')
        self.phase = 'RESERVED'

    def charge(self, call_id: str, amount: int):
        require(self.phase != 'CREATED', 'spend before reservation')
        require(bool(call_id) and call_id not in self.calls, 'duplicate or empty call ID')
        uint(amount, 'charge')
        require(self.spent_micro + amount <= self.ceiling_micro, 'budget exceeded')
        self.calls.add(call_id)
        self.spent_micro += amount

    def draft(self, candidate: bytes):
        require(self.phase in ('RESERVED', 'DRAFT_READY', 'VERIFIED'), 'cannot draft in terminal/unreserved state')
        require(bool(candidate), 'empty proposal')
        self.candidate_sha256 = hashlib.sha256(candidate).hexdigest()
        self.verified_sha256 = None  # Any changed/repaired bytes require verification again.
        self.phase = 'DRAFT_READY'

    def verify(self, candidate_sha256: str, passed: bool):
        require(self.phase == 'DRAFT_READY', 'verification requires a draft')
        require(candidate_sha256 == self.candidate_sha256, 'verifier digest mismatch')
        require(type(passed) is bool, 'typed verifier result required')
        if passed:
            self.verified_sha256 = candidate_sha256
            self.phase = 'VERIFIED'
        else:
            self.phase = 'REJECTED'

    def cancel(self):
        require(self.phase not in ('COMMITTED', 'CANCELLED', 'REJECTED'), 'already terminal')
        self.phase = 'CANCELLED'

    def commit(self, *, authorized: bool, final_check: bool) -> str:
        # These fixture booleans stand for the EXISTING external authority, never model output.
        require(self.phase == 'VERIFIED', 'unverified or terminal branch')
        require(authorized is True and final_check is True, 'effect authority and actual checks required')
        require(self.candidate_sha256 == self.verified_sha256, 'changed bytes after verification')
        self.phase = 'COMMITTED'
        return self.candidate_sha256


def priced_usage(entry: dict) -> int:
    categories = ('uncached_input', 'cache_read', 'cache_write', 'output')
    for c in categories:
        uint(entry['tokens'][c], c)
        uint(entry['price_micro_per_million'][c], c + ' price')
    require(bool(entry['price_revision']), 'price revision missing')
    total = uint(entry['total_input'], 'total input')
    require(total == sum(entry['tokens'][c] for c in categories if c != 'output'), 'overlapping/inconsistent input categories')
    numerator = sum(entry['tokens'][c] * entry['price_micro_per_million'][c] for c in categories)
    return (numerator + PPM - 1) // PPM


def task_ledger(entries: list[dict], *, principal: str, assigned_tasks: list[str]) -> dict:
    require(bool(assigned_tasks) and len(set(assigned_tasks)) == len(assigned_tasks), 'task assignment IDs')
    calls = set()
    known, reserved_unknown = 0, 0
    for e in entries:
        require(e['principal'] == principal, 'cross-principal cost join')
        require(e['task_id'] in assigned_tasks, 'unassigned task cost')
        require(bool(e['attempt_id']) and bool(e['call_id']), 'attempt/call identity missing')
        require(e['call_id'] not in calls, 'duplicate cost event')
        calls.add(e['call_id'])
        if e['usage_status'] == 'UNKNOWN':
            require(e['observed_micro'] is None, 'unknown usage represented as zero')
            require(uint(e['reserved_micro'], 'unknown reservation') > 0, 'unknown needs ceiling')
            reserved_unknown += e['reserved_micro']
        else:
            require(e['usage_status'] == 'COMPLETE', 'usage status')
            amount = priced_usage(e)
            require(e['observed_micro'] == amount, 'computed tariff differs from observed fixture')
            require(amount <= uint(e['reserved_micro'], 'reservation'), 'observed cost exceeds reservation')
            known += amount
    return {'assigned_tasks': len(assigned_tasks), 'call_count': len(calls),
            'known_micro': known, 'unknown_reserved_micro': reserved_unknown,
            'complete': reserved_unknown == 0}
