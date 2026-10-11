#!/usr/bin/env python3
"""Inert mathematical/contract helpers for v0.19 decision-model review. No model or product execution."""
from __future__ import annotations
import hashlib, json, math

def softmax(logits):
    if not logits: raise ValueError("empty logits")
    m=max(logits); ex=[math.exp(x-m) for x in logits]; z=sum(ex)
    return [x/z for x in ex]

def bradley_terry(u_i,u_j):
    d=u_i-u_j
    if d>=0: return 1.0/(1.0+math.exp(-d))
    e=math.exp(d); return e/(1.0+e)

def luce_probabilities(utilities):
    return softmax(list(utilities))

def brier_binary(probabilities, labels):
    if len(probabilities)!=len(labels) or not probabilities: raise ValueError("length/support")
    return sum((float(p)-int(y))**2 for p,y in zip(probabilities,labels))/len(labels)

def nll_binary(probabilities, labels, eps=1e-12):
    if len(probabilities)!=len(labels) or not probabilities: raise ValueError("length/support")
    out=0.0
    for p,y in zip(probabilities,labels):
        p=min(1-eps,max(eps,float(p))); y=int(y)
        out-=y*math.log(p)+(1-y)*math.log(1-p)
    return out/len(labels)

def calibration_key(payload):
    required=("producer","model","score_origin","question","candidate_policy","order_policy","effective_input_policy","domain","calibrator")
    miss=[k for k in required if not payload.get(k)]
    if miss: raise ValueError("missing calibration key fields: "+','.join(miss))
    raw=json.dumps({k:payload[k] for k in sorted(payload)},sort_keys=True,separators=(',',':')).encode()
    return hashlib.sha256(raw).hexdigest()

def pairwise_odds(probabilities,i,j):
    pi,pj=probabilities[i],probabilities[j]
    if pi<=0 or pj<=0: raise ValueError("positive probabilities required")
    return pi/pj

def staged_choice_record(full_digest,shortlist_digest,policy_ref,stage1_semantics,final_event):
    if stage1_semantics in {"joint_probability","calibrated_full_choice_probability"}:
        raise ValueError("stage-1 score cannot be laundered into final joint probability")
    if not all((full_digest,shortlist_digest,policy_ref,final_event)): raise ValueError("missing lineage")
    return {"full_candidate_set_digest":full_digest,"shortlist_candidate_set_digest":shortlist_digest,
            "shortlist_policy_ref":policy_ref,"stage1_semantics":stage1_semantics,"final_event":final_event}
