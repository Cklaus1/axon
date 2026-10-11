"""Strict local contract validation and a deliberately limited semantic linter.

No live I/O adapters, authorization grants, shell execution or remote schema fetches.
See PACKAGE_STATUS.md and docs/IR_SEMANTICS.md for the boundary of these checks.
"""
from __future__ import annotations

from dataclasses import dataclass
import hashlib
import json
import math
from pathlib import Path
from typing import Any

from jsonschema import Draft202012Validator
from referencing import Registry, Resource
from referencing.exceptions import NoSuchResource

ROOT = Path(__file__).resolve().parents[1]
# Bootstrap parser limits, independent of an untrusted configuration document.
BOOTSTRAP_MAX_BYTES = 4 * 1024 * 1024
BOOTSTRAP_MAX_DEPTH = 64
MAX_SAFE_INTEGER = 2**53 - 1
CALL_OPS = frozenset({"transform", "model.call", "tool.call", "code.execute", "evaluate", "human.request"})
WRITE_EFFECTS = frozenset({"write", "communication", "financial"})


class ContractError(ValueError):
    """Invalid wire document or unsupported operation in this offline harness."""


@dataclass(frozen=True)
class Finding:
    code: str
    path: str
    message: str


def _object_no_duplicates(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ContractError(f"Duplicate JSON object key: {key!r}")
        result[key] = value
    return result


def _reject_constant(value: str) -> None:
    raise ContractError(f"Non-JSON numeric constant: {value}")


def _check_tree(value: Any, *, identity: bool = False, max_depth: int = BOOTSTRAP_MAX_DEPTH) -> None:
    stack = [(value, 0)]
    while stack:
        current, depth = stack.pop()
        if depth > max_depth:
            raise ContractError("Document exceeds the configured depth limit")
        if current is None or isinstance(current, bool):
            continue
        if isinstance(current, int):
            if identity and abs(current) > MAX_SAFE_INTEGER:
                raise ContractError("Identity document contains an unsafe-range integer")
        elif isinstance(current, float):
            if not math.isfinite(current):
                raise ContractError("NaN and infinity are not valid wire numbers")
            if identity:
                raise ContractError("asi-json-v1 identity documents cannot contain floats")
        elif isinstance(current, str):
            try:
                current.encode("utf-8", errors="strict")
            except UnicodeError as exc:
                raise ContractError("Unpaired Unicode surrogate") from exc
        elif isinstance(current, list):
            stack.extend((x, depth + 1) for x in current)
        elif isinstance(current, dict):
            if any(not isinstance(k, str) for k in current):
                raise ContractError("JSON object keys must be strings")
            stack.extend((x, depth + 1) for x in current.keys())
            stack.extend((x, depth + 1) for x in current.values())
        else:
            raise ContractError(f"Unsupported JSON value: {type(current).__name__}")


def loads(text: str, *, max_bytes: int = BOOTSTRAP_MAX_BYTES, max_depth: int = BOOTSTRAP_MAX_DEPTH) -> Any:
    try:
        if len(text.encode("utf-8", errors="strict")) > max_bytes:
            raise ContractError("Document exceeds the byte limit")
        value = json.loads(text, object_pairs_hook=_object_no_duplicates, parse_constant=_reject_constant)
        _check_tree(value, max_depth=max_depth)
        return value
    except (json.JSONDecodeError, RecursionError, UnicodeError) as exc:
        raise ContractError(f"Invalid JSON: {exc}") from exc


def load(path: str | Path, **kwargs: Any) -> Any:
    path = Path(path)
    maximum = kwargs.get("max_bytes", BOOTSTRAP_MAX_BYTES)
    # Check before reading an unexpectedly large file into memory.
    if path.stat().st_size > maximum:
        raise ContractError("Document exceeds the byte limit")
    try:
        return loads(path.read_text(encoding="utf-8"), **kwargs)
    except UnicodeError as exc:
        raise ContractError("Invalid UTF-8 document") from exc


def canonical_bytes(value: Any) -> bytes:
    """asi-json-v1 (integer-only identity profile), NOT an RFC 8785 implementation."""
    _check_tree(value, identity=True)
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False, allow_nan=False).encode("utf-8")


def digest(value: Any) -> str:
    return "sha256:" + hashlib.sha256(canonical_bytes(value)).hexdigest()


def _offline_only(uri: str) -> Resource:
    raise NoSuchResource(ref=uri)


class Schemas:
    def __init__(self, root: Path = ROOT):
        self.root = root
        self.documents = {p.stem.replace(".schema", ""): load(p) for p in sorted((root / "schemas").glob("*.schema.json"))}
        resources = []
        for document in self.documents.values():
            Draft202012Validator.check_schema(document)
            resources.append((document["$id"], Resource.from_contents(document)))
        self.registry = Registry(retrieve=_offline_only).with_resources(resources)

    def errors(self, name: str, value: Any) -> list[Finding]:
        _check_tree(value)
        validator = Draft202012Validator(self.documents[name], registry=self.registry)
        result = []
        for error in sorted(validator.iter_errors(value), key=lambda e: str(list(e.absolute_path))):
            path = "/" + "/".join(str(x) for x in error.absolute_path)
            result.append(Finding("SCHEMA", path, error.message))
        return result

    def require(self, name: str, value: Any) -> None:
        findings = self.errors(name, value)
        if findings:
            raise ContractError("; ".join(f"{f.code} {f.path}: {f.message}" for f in findings))


def infer_type(value: Any) -> str:
    if isinstance(value, bool):
        return "boolean"
    if isinstance(value, int):
        return "integer"
    if isinstance(value, float) and math.isfinite(value):
        return "number"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        if set(value) == {"digest", "tenant", "media_type", "size_bytes"}:
            validator = Draft202012Validator(Schemas().documents["common"]["$defs"]["artifact"])
            if validator.is_valid(value):
                return "artifact"
        return "object"
    return "unsupported"


def compatible(actual: str, expected: str) -> bool:
    return actual == expected or (actual == "integer" and expected == "number")


def topological(plan: dict[str, Any]) -> list[str]:
    nodes = {n["id"]: n for n in plan["nodes"]}
    if len(nodes) != len(plan["nodes"]):
        raise ContractError("Duplicate node identity")
    if any(d not in nodes for n in nodes.values() for d in n["depends_on"]):
        raise ContractError("Unknown dependency")
    remaining = set(nodes)
    done: set[str] = set()
    order: list[str] = []
    while remaining:
        ready = sorted(n for n in remaining if set(nodes[n]["depends_on"]) <= done)
        if not ready:
            raise ContractError("Cyclic graph")
        order.extend(ready)
        done.update(ready)
        remaining.difference_update(ready)
    return order


def ancestors(plan: dict[str, Any]) -> dict[str, set[str]]:
    nodes = {n["id"]: n for n in plan["nodes"]}
    closure: dict[str, set[str]] = {}
    for nid in topological(plan):
        found = set(nodes[nid]["depends_on"])
        for dep in nodes[nid]["depends_on"]:
            found.update(closure[dep])
        closure[nid] = found
    return closure


def affected_nodes(plan: dict[str, Any], changed: set[str]) -> set[str]:
    closure = ancestors(plan)
    if not changed <= set(closure):
        raise ContractError("Repair names an unknown node")
    return set(changed) | {nid for nid, parents in closure.items() if parents & changed}


def lint_bundle(plan: dict[str, Any], registry: dict[str, Any], contract: dict[str, Any],
                config: dict[str, Any], intent: dict[str, Any], schemas: Schemas | None = None) -> list[Finding]:
    """Check shape and selected semantics. This never grants execution authority."""
    schemas = schemas or Schemas()
    findings: list[Finding] = []
    for kind, value in [("plan", plan), ("capability-registry", registry), ("evaluation-contract", contract),
                        ("config", config), ("intent", intent)]:
        findings.extend(schemas.errors(kind, value))
    if findings:
        return findings

    def add(code: str, path: str, message: str) -> None:
        findings.append(Finding(code, path, message))

    for key, value in [("intent_ref", intent), ("registry_ref", registry), ("evaluation_ref", contract), ("config_ref", config)]:
        try:
            expected = digest(value)
        except ContractError as exc:
            add("IDENTITY_PROFILE", key, str(exc))
            continue
        if plan[key] != expected:
            add("DIGEST_MISMATCH", key, "Pinned document does not match supplied snapshot")
    try:
        if len(canonical_bytes(plan)) > config["max_plan_bytes"]:
            add("LIMIT", "plan", "Plan exceeds configured byte limit")
    except ContractError as exc:
        add("IDENTITY_PROFILE", "plan", str(exc))
    if plan["task_class"] != intent["task_class"] or plan["task_class"] != contract["task_class"]:
        add("TASK_MISMATCH", "task_class", "Task class differs across bundle")
    if intent["acceptance_contract_name"] != contract["name"]:
        add("CONTRACT_MISMATCH", "evaluation", "Named acceptance contract differs")
    if plan["input_types"] != intent["input_types"]:
        add("TYPE_MISMATCH", "input_types", "Plan input declarations differ from approved intent")
    if len(plan["nodes"]) > config["max_nodes"]:
        add("LIMIT", "nodes", "Too many nodes")
    for key, cfg in [("max_cost_microusd", "max_run_cost_microusd"), ("wall_time_ms", "max_wall_time_ms"), ("max_parallelism", "max_parallelism")]:
        if plan["limits"][key] > min(config[cfg], intent["limits"][key]):
            add("LIMIT", "limits/" + key, "Requested run limit exceeds intent/config")
    if contract["repair"]["max_rounds"] > config["max_repair_rounds"]:
        add("LIMIT", "evaluation/repair", "Repair cap exceeds configuration")
    if config["mode"] == "captured" and (config["allow_external_effects"] or config["automatic_promotion"]):
        add("UNSAFE_CONFIG", "config", "Captured mode cannot allow external effects or auto-promotion")

    caps = {c["id"]: c for c in registry["capabilities"]}
    if len(caps) != len(registry["capabilities"]):
        add("DUPLICATE_CAPABILITY", "registry", "Capability identity must be unique")
    for cid, cap in caps.items():
        effects = set(cap["effects"])
        if cap["replay_class"] == "pure" and effects:
            add("INVALID_EFFECT_CLASS", cid, "Pure capability cannot declare external effects")
        if cap["replay_class"] == "idempotent_write" and not cap["supports_idempotency"]:
            add("INVALID_EFFECT_CLASS", cid, "Idempotent write lacks idempotency contract")
        if effects & WRITE_EFFECTS and cap["replay_class"] not in {"idempotent_write", "non_idempotent_write"}:
            add("INVALID_EFFECT_CLASS", cid, "Write effects need an explicit write replay class")

    nodes = {n["id"]: n for n in plan["nodes"]}
    if len(nodes) != len(plan["nodes"]):
        add("DUPLICATE_NODE", "nodes", "Node IDs must be unique")
        return findings
    for nid, node in nodes.items():
        for dep in node["depends_on"]:
            if dep not in nodes:
                add("UNKNOWN_DEPENDENCY", nid, f"Unknown dependency {dep}")
    if any(f.code == "UNKNOWN_DEPENDENCY" for f in findings):
        return findings
    try:
        closure = ancestors(plan)
    except ContractError as exc:
        add("CYCLE", "nodes", str(exc))
        return findings

    def binding_type(binding: dict[str, Any], nid: str) -> str:
        if "literal" in binding:
            return infer_type(binding["literal"])
        if "input" in binding:
            if binding["input"] not in plan["input_types"]:
                add("UNKNOWN_INPUT", nid, f"Input {binding['input']} is not declared")
                return "unsupported"
            return plan["input_types"][binding["input"]]
        other = binding["node"]
        if other not in nodes:
            add("UNKNOWN_REFERENCE", nid, f"Referenced node {other} does not exist")
            return "unsupported"
        if other not in closure[nid]:
            add("UNORDERED_REFERENCE", nid, f"Referenced node {other} is not a dependency")
        if binding["port"] not in nodes[other]["outputs"]:
            add("UNKNOWN_PORT", nid, f"Unknown port {other}.{binding['port']}")
            return "unsupported"
        return nodes[other]["outputs"][binding["port"]]

    returns = [n for n in nodes.values() if n["mechanism"] == "return"]
    if len(returns) != 1:
        add("RETURN_COUNT", "nodes", "Exactly one return node is required")
    else:
        final = returns[0]["id"]
        if any(final in n["depends_on"] for n in nodes.values()):
            add("NONTERMINAL_RETURN", final, "Return must be terminal")
        for unused in set(nodes) - closure[final] - {final}:
            add("DISCONNECTED_NODE", unused, "Every node must contribute to terminal return")

    for nid, node in nodes.items():
        if node["retry"]["max_attempts"] > config["max_attempts_per_node"]:
            add("LIMIT", nid, "Retry cap exceeds configuration")
        if node["limits"]["max_cost_microusd"] > min(config["max_node_cost_microusd"], plan["limits"]["max_cost_microusd"]):
            add("LIMIT", nid, "Node cost cap exceeds run/config")
        if node["limits"]["timeout_ms"] > min(config["max_node_time_ms"], plan["limits"]["wall_time_ms"]):
            add("LIMIT", nid, "Node timeout exceeds run/config")
        actual_args = {key: binding_type(binding, nid) for key, binding in node["args"].items()}
        if node["mechanism"] in CALL_OPS:
            cap = caps.get(node["capability"])
            if cap is None:
                add("UNKNOWN_CAPABILITY", nid, "Capability is absent from pinned registry")
                continue
            if cap["mechanism"] != node["mechanism"]:
                add("MECHANISM_MISMATCH", nid, "Capability mechanism differs from node mechanism")
            if node["semantic_op"] not in cap["semantic_ops"]:
                add("SEMANTIC_OP_MISMATCH", nid, "Capability does not support the declared semantic operation")
            expected_args = cap["input_types"]
            if set(actual_args) != set(expected_args):
                add("ARGUMENTS", nid, "Argument names differ from capability contract")
            for key in set(actual_args) & set(expected_args):
                if not compatible(actual_args[key], expected_args[key]):
                    add("TYPE_MISMATCH", nid + "/" + key, f"{actual_args[key]} cannot bind {expected_args[key]}")
            if node["outputs"] != cap["output_types"]:
                add("OUTPUT_MISMATCH", nid, "Declared outputs differ from registered capability")
            write = bool(set(cap["effects"]) & WRITE_EFFECTS)
            if node["retry"]["max_attempts"] > 1 and (cap["replay_class"] == "non_idempotent_write" or (write and not cap["supports_idempotency"])):
                add("UNSAFE_RETRY", nid, "Non-idempotent write cannot retry automatically")
        elif node["mechanism"] == "assert":
            if node.get("semantic_op") != "check":
                add("SEMANTIC_OP_MISMATCH", nid, "assert is a check semantic operation")
            if actual_args != {"condition": "boolean"} or node["outputs"] != {"passed": "boolean"}:
                add("TYPE_MISMATCH", nid, "assert requires condition:boolean and emits passed:boolean")
            if node["retry"]["max_attempts"] != 1:
                add("UNSAFE_RETRY", nid, "A deterministic assertion cannot be retried into a pass")
        elif node["mechanism"] == "return":
            if set(actual_args) != {"value"} or set(node["outputs"]) != {"value"}:
                add("RETURN_CONTRACT", nid, "Return requires one value argument and output")
            elif not compatible(actual_args["value"], node["outputs"]["value"]):
                add("TYPE_MISMATCH", nid, "Return type differs from its bound value")
            elif node["outputs"]["value"] != intent["output_type"]:
                add("TYPE_MISMATCH", nid, "Return type differs from approved intent")

    ids: set[str] = set()
    if not any(c["required"] for c in contract["criteria"]):
        add("NO_REQUIRED_CRITERIA", "evaluation", "At least one protected criterion is required")
    for criterion in contract["criteria"]:
        cid = criterion["id"]
        if cid in ids:
            add("DUPLICATE_CRITERION", cid, "Criterion IDs must be unique")
        ids.add(cid)
        target = criterion["target"]
        if target["node"] not in nodes or target["port"] not in nodes[target["node"]]["outputs"]:
            add("EVALUATION_TARGET", cid, "Criterion points to an unavailable output")
            continue
        cap = caps.get(criterion["evaluator"])
        if cap is None or cap["mechanism"] != "evaluate" or "check" not in cap["semantic_ops"]:
            add("UNKNOWN_EVALUATOR", cid, "Criterion must name a registered evaluator")
            continue
        args = criterion["arguments"]
        unknown = set(args) - set(cap["input_types"])
        unbound = set(cap["input_types"]) - set(args)
        if unknown or len(unbound) != 1:
            add("EVALUATOR_ARGUMENTS", cid, "Exactly one evaluator argument must be filled by the target")
        else:
            target_arg = next(iter(unbound))
            target_type = nodes[target["node"]]["outputs"][target["port"]]
            if not compatible(target_type, cap["input_types"][target_arg]):
                add("TYPE_MISMATCH", cid, "Evaluation target type differs from evaluator contract")
        for key in set(args) & set(cap["input_types"]):
            if not compatible(infer_type(args[key]), cap["input_types"][key]):
                add("TYPE_MISMATCH", cid + "/" + key, "Criterion argument has wrong type")
    if not set(contract["repair"]["allowed_nodes"]) <= set(nodes):
        add("REPAIR_SCOPE", "evaluation/repair", "Repair scope names unavailable nodes")
    return findings


def validate_decision(decision: dict[str, Any], tolerance: float = 1e-6) -> None:
    """Validate typed semantics, without inferring calibrated correctness."""
    if tolerance < 0 or not math.isfinite(tolerance):
        raise ContractError("Invalid probability tolerance")
    _check_tree(decision)
    schemas = Schemas()
    validator = Draft202012Validator({"$ref": "urn:asi:common:0.3.0#/$defs/decision"}, registry=schemas.registry)
    errors = list(validator.iter_errors(decision))
    if errors:
        raise ContractError(errors[0].message)
    if "probabilities" in decision:
        probabilities = decision["probabilities"]
        if abs(math.fsum(probabilities.values()) - 1.0) > tolerance:
            raise ContractError("Decision probabilities must sum to one")
    if decision["kind"] == "choice":
        probabilities = decision["probabilities"]
        selected = decision["selected"]
        if selected not in probabilities:
            raise ContractError("Selected candidate is not in candidate set")
        if probabilities[selected] + tolerance < max(probabilities.values()):
            raise ContractError("Selected candidate is not a maximum-probability option")


def accept_results(contract: dict[str, Any], results: list[dict[str, Any]], expected_targets: dict[str, str]) -> tuple[bool, list[str]]:
    """Offline acceptance envelope check; no policy signatures/calibration are verified.

    Learned acceptance is always blocked here: this harness has no trusted
    calibration/decision-policy verifier. Production must implement that separate gate.
    """
    schemas = Schemas()
    schemas.require("evaluation-contract", contract)
    by_id: dict[str, dict[str, Any]] = {}
    problems: list[str] = []
    known = {c["id"] for c in contract["criteria"]}
    if len(known) != len(contract["criteria"]):
        problems.append("Duplicate criterion identity in contract")
    if not any(c["required"] for c in contract["criteria"]):
        problems.append("At least one protected criterion is required")
    for result in results:
        errors = schemas.errors("evaluation-result", result)
        if errors:
            problems.append("Malformed evaluation result")
            continue
        cid = result["criterion_id"]
        if cid not in known or cid in by_id:
            problems.append("Unknown or duplicate evaluation criterion")
            continue
        by_id[cid] = result
    for criterion in contract["criteria"]:
        if not criterion["required"]:
            continue
        cid = criterion["id"]
        result = by_id.get(cid)
        if result is None:
            problems.append(f"{cid}: missing result")
            continue
        if result["evaluator"] != criterion["evaluator"]:
            problems.append(f"{cid}: wrong evaluator")
        if expected_targets.get(cid) != result["target_digest"]:
            problems.append(f"{cid}: stale or unknown target")
        if not result["evidence_refs"]:
            problems.append(f"{cid}: missing evidence")
        if result["status"] != "pass":
            problems.append(f"{cid}: {result['status']}")
        decision = result["decision"]
        if decision is None or decision["kind"] != criterion["decision_kind"]:
            problems.append(f"{cid}: decision type mismatch")
            continue
        try:
            validate_decision(decision)
        except ContractError as exc:
            problems.append(f"{cid}: {exc}")
        if decision["kind"] == "deterministic":
            if decision["passed"] is not True:
                problems.append(f"{cid}: deterministic check failed")
        else:
            problems.append(f"{cid}: offline harness cannot verify learned acceptance/calibration")
    return not problems, problems


def bundle(name: str = "document", root: Path = ROOT) -> tuple[dict[str, Any], ...]:
    return (load(root / f"examples/plans/{name}.json"), load(root / "examples/registry.json"),
            load(root / f"examples/evaluations/{name}.json"), load(root / "config/defaults.json"),
            load(root / f"examples/intents/{name}.json"))
