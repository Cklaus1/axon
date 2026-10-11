"""Synthetic captured-output DAG demonstration. NO live models, tools or effects."""
from __future__ import annotations

import argparse
import copy
import json
from typing import Any

from .contracts import (ROOT, ContractError, Schemas, accept_results, affected_nodes, bundle,
                        compatible, digest, infer_type, lint_bundle, load, topological)


def resolve(binding: dict[str, Any], inputs: dict[str, Any], values: dict[str, dict[str, Any]]) -> Any:
    if "literal" in binding:
        return copy.deepcopy(binding["literal"])
    if "input" in binding:
        return inputs[binding["input"]]
    return values[binding["node"]][binding["port"]]


def pure_call(capability: str, args: dict[str, Any]) -> dict[str, Any]:
    if capability == "text.normalize@1":
        return {"text": " ".join(args["text"].split())}
    if capability == "eval.nonempty@1":
        return {"passed": bool(args["text"].strip())}
    if capability == "eval.contains@1":
        return {"passed": all(term.lower() in args["text"].lower() for term in args["required_terms"])}
    if capability == "eval.is_true@1":
        return {"passed": args["value"] is True}
    raise ContractError(f"No pure fixture implementation for {capability}")


def execute(plan: dict[str, Any], registry: dict[str, Any], inputs: dict[str, Any],
            captures: dict[str, Any], reuse: dict[str, Any] | None = None) -> dict[str, Any]:
    """Sequential fixture execution, with checked reuse of pure upstream results only."""
    Schemas().require("plan", plan)
    if set(inputs) != set(plan["input_types"]):
        raise ContractError("Fixture inputs differ from plan input declarations")
    for key, expected in plan["input_types"].items():
        if not compatible(infer_type(inputs[key]), expected):
            raise ContractError(f"Wrong fixture input type for {key}")
    caps = {c["id"]: c for c in registry["capabilities"]}
    nodes = {n["id"]: n for n in plan["nodes"]}
    values: dict[str, dict[str, Any]] = {}
    stored: dict[str, Any] = {}
    trace: list[dict[str, Any]] = []
    final: Any = None
    for nid in topological(plan):
        node = nodes[nid]
        args = {key: resolve(binding, inputs, values) for key, binding in node["args"].items()}
        cap = caps.get(node.get("capability", ""))
        identity = {"node_digest": digest(node), "input_digest": digest(args),
                    "registry_ref": plan["registry_ref"], "config_ref": plan["config_ref"]}
        reused = False
        if reuse and nid in reuse:
            previous = reuse[nid]
            if cap is None or cap["replay_class"] != "pure":
                raise ContractError("Only pure fixture results may be reused in this demo")
            if any(previous[key] != value for key, value in identity.items()):
                raise ContractError("Stale/incompatible fixture result reuse")
            output = copy.deepcopy(previous["outputs"])
            reused = True
        elif node["mechanism"] == "assert":
            if args["condition"] is not True:
                raise ContractError(f"Assertion failed: {nid}")
            output = {"passed": True}
        elif node["mechanism"] == "return":
            output = {"value": args["value"]}
        elif cap is not None and cap["replay_class"] == "pure":
            output = pure_call(cap["id"], args)
        else:
            # Captures are data. No provider dispatch path exists in this package.
            if cap is None or set(cap["effects"]) & {"write", "communication", "financial"}:
                raise ContractError("Effectful/unknown capability refused by fixture simulator")
            if nid not in captures:
                raise ContractError(f"Missing synthetic capture for {nid}")
            output = copy.deepcopy(captures[nid])
        if set(output) != set(node["outputs"]):
            raise ContractError(f"Output ports do not match at {nid}")
        for key, expected in node["outputs"].items():
            if not compatible(infer_type(output[key]), expected):
                raise ContractError(f"Wrong output type at {nid}.{key}")
        values[nid] = output
        stored[nid] = {**identity, "outputs": output}
        trace.append({"node": nid, "mechanism": node["mechanism"], "semantic_op": node.get("semantic_op"), "reused": reused, "synthetic": True})
        if node["mechanism"] == "return":
            final = output["value"]
    return {"values": values, "stored": stored, "trace": trace, "output": final,
            "output_digest": digest(final), "work_completed": True, "synthetic": True}


def evaluate(contract: dict[str, Any], registry: dict[str, Any], values: dict[str, Any]) -> dict[str, Any]:
    caps = {c["id"]: c for c in registry["capabilities"]}
    results = []
    targets = {}
    for criterion in contract["criteria"]:
        target = criterion["target"]
        value = values[target["node"]][target["port"]]
        target_digest = digest(value)
        targets[criterion["id"]] = target_digest
        cap = caps[criterion["evaluator"]]
        args = copy.deepcopy(criterion["arguments"])
        unresolved = set(cap["input_types"]) - set(args)
        if len(unresolved) != 1:
            raise ContractError("Evaluation target binding is ambiguous")
        args[next(iter(unresolved))] = value
        status, decision = "error", None
        if criterion["decision_kind"] == "deterministic" and cap["replay_class"] == "pure":
            try:
                output = pure_call(criterion["evaluator"], args)
                passed = output["passed"] is True
                status, decision = ("pass" if passed else "fail"), {"kind": "deterministic", "passed": passed}
            except ContractError:
                pass
        results.append({"result_version": "asi.evaluation-result/v0.3", "criterion_id": criterion["id"],
                        "evaluator": criterion["evaluator"], "target_digest": target_digest, "status": status,
                        "decision": decision, "calibration_ref": None,
                        "evidence_refs": [digest({"synthetic_checker": criterion["evaluator"], "args": args, "status": status})],
                        "usage": {"cost_microusd": 0, "input_tokens": None, "cached_input_tokens": None, "output_tokens": None, "synthetic": True},
                        "synthetic": True})
    accepted, problems = accept_results(contract, results, targets)
    return {"accepted": accepted, "problems": problems, "results": results, "synthetic": True}


def demonstrate() -> dict[str, Any]:
    plan, registry, contract, config, intent = bundle("document")
    findings = lint_bundle(plan, registry, contract, config, intent)
    if findings:
        raise ContractError(str(findings))
    inputs = load(ROOT / "examples/document_inputs.json")
    captures = load(ROOT / "examples/document_captures.json")
    if captures.get("synthetic") is not True:
        raise ContractError("This demonstration requires explicit synthetic captures")
    original = execute(plan, registry, inputs, captures["cases"]["flawed"])
    first_eval = evaluate(contract, registry, original["values"])
    if first_eval["accepted"]:
        raise ContractError("Deliberately flawed fixture unexpectedly passed")
    if contract["repair"]["max_rounds"] < 1 or "draft" not in contract["repair"]["allowed_nodes"]:
        raise ContractError("Repair is not within the fixture contract")
    repaired_plan = copy.deepcopy(plan)
    repaired_plan["revision"] += 1
    changed = {"draft"}
    invalidated = affected_nodes(plan, changed)
    reuse = {nid: record for nid, record in original["stored"].items() if nid not in invalidated}
    if lint_bundle(repaired_plan, registry, contract, config, intent):
        raise ContractError("Repair candidate failed revalidation")
    repaired = execute(repaired_plan, registry, inputs, captures["cases"]["repaired"], reuse)
    second_eval = evaluate(contract, registry, repaired["values"])
    return {"demonstration": "synthetic_only_not_a_model_benchmark", "synthetic": True,
            "original_plan_ref": digest(plan), "repaired_plan_ref": digest(repaired_plan),
            "original_accepted": first_eval["accepted"], "original_problems": first_eval["problems"],
            "repaired_accepted": second_eval["accepted"], "invalidated_nodes": sorted(invalidated),
            "reused_nodes": [e["node"] for e in repaired["trace"] if e["reused"]],
            "final_output": repaired["output"], "final_output_digest": repaired["output_digest"],
            "repair_trace": repaired["trace"], "final_evaluations": second_eval["results"],
            "live_provider_calls": 0, "external_effects": 0,
            "limitations": "No compiler/model/repair provider, durable scheduler, real cost/quality evidence or live authorization is implemented."}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--case", choices=["document"], default="document")
    parser.parse_args()
    try:
        print(json.dumps(demonstrate(), indent=2, ensure_ascii=False, allow_nan=False))
        return 0
    except (ContractError, OSError, KeyError) as exc:
        parser.exit(1, f"Simulation failed: {exc}\n")


if __name__ == "__main__":
    raise SystemExit(main())
