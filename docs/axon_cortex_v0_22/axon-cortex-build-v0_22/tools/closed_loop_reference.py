#!/usr/bin/env python3
"""Offline semantic reference for the proposed Axon 0.22 integration.

No database, subprocess, model, network, signature verifier or policy deployment.
Caller-provided "trusted" identities below are test premises, NOT authentication.
Passing these functions never qualifies a runtime or proves physical confinement.
"""
from __future__ import annotations

from copy import deepcopy
from dataclasses import dataclass, field
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
from typing import Any, Mapping, Iterable

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[1]
MAX_INTEGER = 9_007_199_254_740_991
MAX_BYTES = 1_048_576
MAX_DEPTH = 32
REF = re.compile(r'^(?:cl22|acf1|sha256):[0-9a-f]{64}$')


class Refusal(ValueError):
    """The reference contract refused an invalid or unsupported input."""


def integer(value: Any, *, positive: bool = False) -> int:
    if type(value) is not int or not (1 if positive else 0) <= value <= MAX_INTEGER:
        raise Refusal('expected bounded non-boolean integer')
    return value


def _bounded_depth(text: str) -> None:
    depth, quoted, escaped = 0, False, False
    for char in text:
        if quoted:
            if escaped:
                escaped = False
            elif char == '\\':
                escaped = True
            elif char == '"':
                quoted = False
        elif char == '"':
            quoted = True
        elif char in '[{':
            depth += 1
            if depth > MAX_DEPTH:
                raise Refusal('JSON nesting limit')
        elif char in ']}':
            depth -= 1
    # json.loads validates balancing/quotes; this scan only bounds its recursion.


def _json_tree(value: Any, depth: int = 0) -> None:
    if depth > MAX_DEPTH:
        raise Refusal('tree nesting limit')
    if value is None or type(value) is bool:
        return
    if type(value) is int:
        if abs(value) > MAX_INTEGER:
            raise Refusal('unsafe JSON integer')
    elif type(value) is str:
        try:
            value.encode('utf-8', errors='strict')
        except UnicodeError as exc:
            raise Refusal('invalid Unicode scalar') from exc
    elif type(value) is list:
        for item in value:
            _json_tree(item, depth + 1)
    elif type(value) is dict:
        for key, item in value.items():
            if type(key) is not str:
                raise Refusal('non-string object key')
            _json_tree(key, depth + 1)
            _json_tree(item, depth + 1)
    else:
        raise Refusal('floats and non-JSON objects are unsupported')


def strict_json(data: str | bytes) -> Any:
    if isinstance(data, bytes):
        if len(data) > MAX_BYTES:
            raise Refusal('JSON byte limit')
        try:
            text = data.decode('utf-8', errors='strict')
        except UnicodeError as exc:
            raise Refusal('invalid UTF-8') from exc
    elif isinstance(data, str):
        try:
            if len(data.encode('utf-8', errors='strict')) > MAX_BYTES:
                raise Refusal('JSON byte limit')
        except UnicodeError as exc:
            raise Refusal('invalid Unicode scalar') from exc
        text = data
    else:
        raise Refusal('JSON input must be str or bytes')
    _bounded_depth(text)

    def pairs(items):
        obj = {}
        for key, value in items:
            if key in obj:
                raise Refusal('duplicate decoded JSON key: ' + key)
            obj[key] = value
        return obj

    def no_float(_):
        raise Refusal('float/non-finite JSON number')

    def bounded_int(text):
        if len(text.lstrip('-')) > 16:
            raise Refusal('unsafe JSON integer')
        value = int(text)
        if abs(value) > MAX_INTEGER:
            raise Refusal('unsafe JSON integer')
        return value

    try:
        result = json.loads(text, object_pairs_hook=pairs, parse_float=no_float,
                            parse_constant=no_float, parse_int=bounded_int)
        _json_tree(result)
        return result
    except (json.JSONDecodeError, RecursionError, UnicodeError) as exc:
        raise Refusal('malformed JSON') from exc


def canonical(value: Any) -> bytes:
    _json_tree(value)
    try:
        raw = json.dumps(value, sort_keys=True, separators=(',', ':'),
                         ensure_ascii=False, allow_nan=False).encode('utf-8')
    except (TypeError, ValueError, UnicodeError) as exc:
        raise Refusal('not canonicalizable') from exc
    if len(raw) > MAX_BYTES:
        raise Refusal('canonical byte limit')
    return raw


def content_ref(value: Any) -> str:
    return 'cl22:' + hashlib.sha256(canonical(value)).hexdigest()


def validate(name: str, value: Any) -> None:
    if name not in {'policy', 'context', 'episode', 'transition', 'pilot', 'evidence'}:
        raise Refusal('unknown local schema')
    _json_tree(value)
    schema = json.loads((ROOT / f'schemas/json/closed-loop-{name}.schema.json').read_text())
    errors = sorted(Draft202012Validator(schema).iter_errors(value), key=lambda e: str(list(e.path)))
    if errors:
        e = errors[0]
        raise Refusal(name + ': ' + '/'.join(map(str, e.path)) + ': ' + e.message)


def validate_acf(name: str, value: Any) -> None:
    if name not in {'compute_request', 'execution_receipt', 'backend_profile'}:
        raise Refusal('unknown Fabric schema')
    _json_tree(value)
    p = ROOT / 'integration/compute-fabric-v0_1-reference/contracts' / (name + '.schema.json')
    errors = list(Draft202012Validator(json.loads(p.read_text())).iter_errors(value))
    if errors:
        raise Refusal('Fabric schema: ' + errors[0].message)


def concrete_path(path: str) -> str:
    if not isinstance(path, str) or not path or '\\' in path or '\x00' in path:
        raise Refusal('invalid concrete path')
    if any(char in path for char in '*?[]:'):
        raise Refusal('unproved pattern/drive path is not a concrete permission')
    raw = path.split('/')
    if path.startswith('/') or any(part in ('', '.', '..') for part in raw):
        raise Refusal('unsafe or noncanonical relative path')
    if PurePosixPath(path).as_posix() != path:
        raise Refusal('noncanonical path')
    return path


def narrowed_effects(requested_tools: Iterable[str], requested_paths: Iterable[str],
                     allowed_tools: Iterable[str], allowed_paths: Iterable[str]) -> None:
    requested_tools, requested_paths = list(requested_tools), list(requested_paths)
    allowed_tools, allowed_paths = set(allowed_tools), {concrete_path(p) for p in allowed_paths}
    if len(set(requested_tools)) != len(requested_tools) or len(set(requested_paths)) != len(requested_paths):
        raise Refusal('duplicate requested effect')
    if not set(requested_tools) <= allowed_tools:
        raise Refusal('tool authority expansion')
    if not {concrete_path(p) for p in requested_paths} <= allowed_paths:
        raise Refusal('file authority expansion')


def shortlist(policy: dict, *, eligible: set[str], candidate_set_ref: str,
              controls_ref: str, scope: dict) -> tuple[str, ...]:
    validate('policy', policy)
    if policy['scope'] != scope or policy['candidate_set_ref'] != candidate_set_ref:
        raise Refusal('wrong scope or eligible-candidate view')
    if policy['controls_ref'] != controls_ref:
        raise Refusal('pilot control drift')
    if not set(policy['shortlist']) <= eligible:
        raise Refusal('shortlist expands eligible candidates')
    return tuple(policy['shortlist'])


def preflight(context: dict, *, now_ms: int, current_epoch: int,
              trusted_observers: set[str]) -> None:
    validate('context', context)
    integer(now_ms); integer(current_epoch)
    if not context['created_ms'] <= now_ms < context['expires_ms']:
        raise Refusal('context future/expired')
    if context['authority_epoch'] != current_epoch:
        raise Refusal('stale authority epoch')
    if context['observed_issuer_ref'] == context['expected_issuer_ref']:
        raise Refusal('parent echo is not independent observation')
    if context['observed_issuer_ref'] not in trusted_observers:
        raise Refusal('unrecognized observer premise')
    if context['expected'] != context['observed']:
        raise Refusal('TASK_NOT_STARTED: EXECUTION_CONTEXT_MISMATCH')
    observed = context['observed']
    if observed['is_primary_worktree']:
        raise Refusal('primary integration checkout is not a trial')
    for path in observed['read_paths'] + observed['write_paths']:
        concrete_path(path)
    if observed['role'] in {'critic', 'verifier'} and observed['write_paths']:
        raise Refusal('read-only/verifier role cannot write subject workspace')
    if observed['role'] in {'implementation', 'documentation'} and not observed['write_paths']:
        raise Refusal('implementation write set must be explicit and nonempty')


def bind_episode(episode: dict, policy: dict, context: dict, *, current_epoch: int,
                 trusted_verifiers: set[str], subject_issuers: set[str]) -> None:
    validate('episode', episode); validate('policy', policy); validate('context', context)
    if episode['identity'] != context['identity']:
        raise Refusal('task/arm/trial/attempt/operation/execution mismatch')
    if episode['scope'] != context['scope'] or episode['scope'] != policy['scope']:
        raise Refusal('cross-scope episode')
    if episode['policy_ref'] != content_ref(policy) or episode['context_ref'] != content_ref(context):
        raise Refusal('policy/context byte mismatch')
    if episode['controls_ref'] != policy['controls_ref'] or episode['candidate_set_ref'] != policy['candidate_set_ref']:
        raise Refusal('effective control/candidate view mismatch')
    if episode['input_workspace_ref'] != context['observed']['workspace_ref']:
        raise Refusal('wrong input workspace')
    if episode['authority_epoch'] != current_epoch or context['authority_epoch'] != current_epoch:
        raise Refusal('stale result authority')
    verification = episode['verification']
    if verification['result'] == 'passed':
        if verification['issuer_ref'] not in trusted_verifiers or verification['issuer_ref'] in subject_issuers:
            raise Refusal('subject or unknown verifier cannot establish outcome')
        if verification['output_workspace_ref'] != episode['output_workspace_ref']:
            raise Refusal('checked output is not candidate output')


def bind_acf(episode: dict, request: dict, receipt: dict, projection: dict) -> None:
    """Structural mapping only; production must authenticate/resolve projection first."""
    validate('episode', episode); validate_acf('compute_request', request); validate_acf('execution_receipt', receipt)
    if set(projection) != {'sidecar_policy_ref', 'acf_policy_digest', 'projection_ref'}:
        raise Refusal('closed policy projection')
    if not REF.fullmatch(projection['projection_ref']):
        raise Refusal('bad projection reference')
    if projection['sidecar_policy_ref'] != episode['policy_ref']:
        raise Refusal('wrong policy projection subject')
    if request['policy_digest'] != projection['acf_policy_digest'] or receipt['policy_digest'] != projection['acf_policy_digest']:
        raise Refusal('wrong supervisor policy projection')
    if episode['acf_request_ref'] != content_ref(request) or episode['acf_receipt_ref'] != content_ref(receipt):
        raise Refusal('Fabric reference mismatch')
    for key in ['task_id', 'trial_id', 'attempt_id', 'operation_id']:
        if request[key] != episode['identity'][key] or receipt[key] != episode['identity'][key]:
            raise Refusal('Fabric identity mismatch: ' + key)
    if receipt['execution_id'] != episode['identity']['execution_id']:
        raise Refusal('Fabric execution mismatch')
    if request['workspace_version_ref'] != episode['input_workspace_ref'] or receipt['input_workspace_ref'] != episode['input_workspace_ref']:
        raise Refusal('Fabric input mismatch')
    if receipt['output_workspace_ref'] != episode['output_workspace_ref']:
        raise Refusal('Fabric output mismatch')
    # Explicit spelling/status projection; timed_out does not become success.
    status_map = {'completed':'completed','failed':'failed','canceled':'cancelled',
                  'denied':'refused','unsupported':'unsupported','outcome_unknown':'outcome_unknown','timed_out':'outcome_unknown'}
    if status_map[receipt['status']] != episode['status']:
        raise Refusal('Fabric outcome semantics lost')


def eligible_profile(profile: dict, required: dict, *, expected_config: str,
                     independently_qualified_refs: set[str]) -> bool:
    """Eligibility model; test premises never establish actual host qualification."""
    validate_acf('backend_profile', profile)
    if profile['support_status'] != 'verified' or not set(profile['evidence_refs']) & independently_qualified_refs:
        return False
    if profile['configuration_digest'] != expected_config:
        return False
    if any(profile[k] != required[k] for k in ['engine', 'os', 'architecture']):
        return False
    if profile['placement'] != 'local' or profile['guest_kind'] != 'linux_init':
        return False
    enforcement = set(profile['enforcement'])
    mandatory = {'host_filesystem_boundary','host_network_deny','descendant_resource_limits','current_epoch_rebind','output_limit'}
    if not mandatory <= enforcement or required['network_mode'] != 'deny':
        return False
    if required['hardware_isolation'] and (profile['enclosure'] != 'kvm_microvm' or 'hardware_isolation' not in enforcement):
        return False
    if required['checkpoint_kind'] not in {'none','logical_workspace'}:
        return False
    return 'registered_check' in profile['functionality']


def comparable(left: dict, right: dict, *, controls: Iterable[str]) -> None:
    for key in controls:
        if key not in left or key not in right or left[key] != right[key]:
            raise Refusal('unmatched control: ' + key)
    if left.get('trial_id') == right.get('trial_id') or left.get('attempt_id') == right.get('attempt_id'):
        raise Refusal('paired arms reuse execution identity')
    if left.get('arm_id') == right.get('arm_id'):
        raise Refusal('not distinct experimental arms')


def pilot_ready(plan: dict) -> None:
    validate('pilot', plan)
    if not plan['runtime_ready'] or not plan['operator_approved']:
        raise Refusal('pilot not configured/approved')
    required = [k for k in plan if k not in {'schema','experiment_id','scope','mutable_surface','runtime_ready','deployment_enabled','operator_approved','live_evidence'}]
    if any(plan[k] is None for k in required):
        raise Refusal('unresolved pilot field')
    if len({plan[k] for k in ['discovery_manifest_ref','confirmation_manifest_ref','reporting_manifest_ref']}) != 3:
        raise Refusal('corpus role aliasing')
    if plan['incumbent_policy_ref'] == plan['candidate_policy_ref']:
        raise Refusal('candidate equals incumbent')
    # Authentication/power/statistical adequacy are external owner requirements.


def learning_eligible(episode: dict) -> bool:
    validate('episode', episode)
    return (episode['corpus_role'] in {'discovery','tuning'}
            and episode['status'] == 'completed'
            and episode['verification']['result'] == 'passed'
            and episode['usage']['state'] == 'final')


@dataclass
class ReservationBook:
    """In-memory economic state machine, NOT durable/concurrent production accounting."""
    limit_micro: int
    rows: dict[str, dict] = field(default_factory=dict)

    def __post_init__(self):
        integer(self.limit_micro, positive=True)

    def exposure(self) -> int:
        return sum(r['cost'] + r['held'] for r in self.rows.values())

    def reserve(self, operation: str, amount: int) -> None:
        integer(amount, positive=True)
        if operation in self.rows:
            if self.rows[operation]['reserved'] != amount:
                raise Refusal('changed duplicate reservation')
            return
        if self.exposure() + amount > self.limit_micro:
            raise Refusal('aggregate budget exhausted')
        self.rows[operation] = {'reserved':amount,'cost':0,'held':amount,'sequence':0,'receipts':{},'final':False}

    def observe(self, operation: str, *, sequence: int, cumulative_cost: int,
                liability: int, final: bool, cleanup_complete: bool) -> None:
        integer(sequence, positive=True); integer(cumulative_cost); integer(liability)
        if type(final) is not bool or type(cleanup_complete) is not bool:
            raise Refusal('invalid state flags')
        if operation not in self.rows:
            raise Refusal('unreserved operation')
        row = self.rows[operation]
        payload = content_ref([cumulative_cost,liability,final,cleanup_complete])
        if sequence in row['receipts']:
            if row['receipts'][sequence] != payload:
                raise Refusal('conflicting duplicate usage receipt')
            return
        if row['final'] or sequence != row['sequence'] + 1:
            raise Refusal('finalized or noncontiguous usage sequence')
        if cumulative_cost < row['cost']:
            raise Refusal('unregistered downward cost correction')
        if final and (liability or not cleanup_complete):
            raise Refusal('cannot release unresolved financial/resource liability')
        # Observed overspend is RECORDED rather than discarded for violating a budget.
        row['cost'] = cumulative_cost
        row['held'] = 0 if final else max(liability, row['reserved'] - cumulative_cost, 0)
        row['sequence'] = sequence; row['receipts'][sequence] = payload; row['final'] = final


@dataclass
class OperationJournal:
    """Volatile transition specification; cannot prove durable journal-before-effect."""
    rows: dict[str, dict] = field(default_factory=dict)

    def submit(self, operation: str, immutable_input: dict) -> dict:
        ref = content_ref(immutable_input)
        if operation in self.rows:
            if self.rows[operation]['input_ref'] != ref:
                raise Refusal('operation conflict')
            return deepcopy(self.rows[operation])
        row = {'input_ref':ref,'state':'admitted','possible_effect':False,'cleanup_complete':False}
        self.rows[operation] = row
        return deepcopy(row)

    def start(self, operation: str) -> None:
        row = self.rows[operation]
        if row['state'] != 'admitted':
            raise Refusal('cannot blindly reexecute')
        row['state'] = 'running'; row['possible_effect'] = True

    def cancel(self, operation: str) -> None:
        row = self.rows[operation]
        if row['state'] not in {'admitted','running','outcome_unknown','cancel_requested'}:
            raise Refusal('cannot cancel this state')
        row['state'] = 'cancel_requested'

    def timeout(self, operation: str) -> None:
        row = self.rows[operation]
        if row['state'] not in {'running','cancel_requested'}:
            raise Refusal('no outstanding possible effect')
        row['state'] = 'outcome_unknown'

    def reconcile(self, operation: str, *, observed_state: str, cleanup_complete: bool) -> None:
        row = self.rows[operation]
        if observed_state not in {'running','completed','failed','cancelled','outcome_unknown'}:
            raise Refusal('unsupported observation')
        if row['state'] in {'completed','failed','cancelled'}:
            raise Refusal('terminal state cannot be overwritten')
        if type(cleanup_complete) is not bool:
            raise Refusal('bad cleanup flag')
        if observed_state in {'running','outcome_unknown'} and cleanup_complete:
            raise Refusal('unknown/running resource not confirmed cleaned')
        row['state'] = observed_state; row['cleanup_complete'] = cleanup_complete


@dataclass
class PolicyPointer:
    """Volatile CAS specification; no authenticated admission, persistence or deployment."""
    scope: dict
    active_ref: str | None
    epoch: int
    controls_ref: str
    history: list[str] = field(default_factory=list)
    transitions: dict[str, tuple[str, dict]] = field(default_factory=dict)

    def start_task(self) -> tuple[str, int]:
        if self.active_ref is None:
            raise Refusal('paused: no active safe policy')
        return self.active_ref, self.epoch

    def apply(self, transition: dict, *, admissions: Mapping[str, dict],
              trusted_admitters: set[str]) -> dict:
        validate('transition', transition)
        key = transition['transition_id']; request_hash = content_ref(transition)
        if key in self.transitions:
            recorded_hash, reply = self.transitions[key]
            if recorded_hash != request_hash:
                raise Refusal('transition ID conflict')
            return deepcopy(reply)
        if transition['issuer_ref'] not in trusted_admitters or transition['scope'] != self.scope:
            raise Refusal('unrecognized authority or wrong scope')
        if transition['expected_policy_ref'] != self.active_ref or transition['expected_epoch'] != self.epoch:
            raise Refusal('stale active policy or fence')
        if transition['next_epoch'] != self.epoch + 1:
            raise Refusal('non-monotonic/noncontiguous fence')
        target = transition['target_policy_ref']
        if transition['kind'] != 'pause':
            a = admissions.get(transition['admission_ref'])
            if not a or set(a) != {'target_ref','scope','controls_ref','current','revoked','mechanism_test'}:
                raise Refusal('missing/unsupported admission')
            if a['target_ref'] != target or a['scope'] != self.scope or a['controls_ref'] != self.controls_ref:
                raise Refusal('admission subject/scope/controls mismatch')
            if a['current'] is not True or a['revoked'] is not False:
                raise Refusal('revoked/stale predecessor or candidate')
            if a['mechanism_test'] != transition['mechanism_test']:
                raise Refusal('fixture evidence laundering')
            if transition['kind'] == 'rollback' and target not in self.history:
                raise Refusal('not an admitted predecessor')
        if self.active_ref is not None:
            self.history.append(self.active_ref)
        self.active_ref = target; self.epoch = transition['next_epoch']
        result = {'active_ref':target,'epoch':self.epoch,'mode':'paused' if target is None else 'active',
                  'reference_only':True,'runtime_qualified':False,'measured_improvement_supported':False}
        self.transitions[key] = (request_hash,deepcopy(result))
        return result
