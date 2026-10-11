from __future__ import annotations
from dataclasses import dataclass, asdict
from typing import Any, Dict, List, Optional
import copy, hashlib, json

def canonical_json(obj: Any) -> str:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)

def artifact_hash(obj: Any) -> str:
    return hashlib.sha256(canonical_json(obj).encode()).hexdigest()

@dataclass(frozen=True)
class Diagnostic:
    severity: str
    code: str
    message: str
    sir_node_id: Optional[str] = None

@dataclass
class TransformResult:
    status: str
    output_ir: Optional[Dict[str, Any]]
    semantic_diff: Dict[str, Any]
    provenance: Dict[str, Any]
    diagnostics: List[Dict[str, Any]]
    verification_requirements: List[Dict[str, Any]]
    observed_inputs: List[Dict[str, Any]]
    metrics: Dict[str, Any]

class RenameFunction:
    id = "reference.rename-function"
    version = "1"
    def apply(self, input_ir: Dict[str, Any], context: Dict[str, Any]) -> TransformResult:
        before = copy.deepcopy(input_ir); after = copy.deepcopy(input_ir)
        target = context["parameters"]["function_id"]; new_name = context["parameters"]["new_name"]
        changed = False
        def visit(node):
            nonlocal changed
            if node.get("id") == target and node.get("kind") == "function":
                node["name"] = new_name; changed = True
            for c in node.get("children", []): visit(c)
        for m in after.get("modules", []): visit(m)
        if not changed:
            return TransformResult("not_applicable",None,{"changes":[]},{"transform_id":self.id,"version":self.version},[asdict(Diagnostic("warning","TARGET_NOT_FOUND","Function not found",target))],[],[],{})
        return TransformResult("applied",after,{"changes":[{"node_id":target,"kind":"rename","after":new_name}]},{"transform_id":self.id,"version":self.version,"input_hash":artifact_hash(before),"output_hash":artifact_hash(after)},[],[{"kind":"contract_check","required":True}],[],{})

def demo():
    sir={"schema_version":"0.2","program_id":"demo","modules":[{"id":"m1","kind":"module","name":"main","type_ref":None,"effects":[],"capabilities":[],"resources":[],"children":[{"id":"f1","kind":"function","name":"add","type_ref":"fn(i64,i64)->i64","effects":[],"capabilities":[],"resources":[],"children":[],"attributes":{},"provenance":{"artifact_hash":"src"}}],"attributes":{},"provenance":{"artifact_hash":"src"}}],"contracts":[],"provenance":{"artifact_hash":"src"}}
    r=RenameFunction().apply(sir,{"parameters":{"function_id":"f1","new_name":"sum"}})
    assert r.status=="applied" and r.output_ir["modules"][0]["children"][0]["name"]=="sum"
    assert artifact_hash(sir)==artifact_hash(json.loads(canonical_json(sir)))
    print(json.dumps({"input_hash":artifact_hash(sir),"result":asdict(r)},indent=2))
if __name__=='__main__': demo()
