#!/usr/bin/env python3
"""Promote a Cortex config only through a frozen evaluation in `axon-loop`.

    python3 promote.py run --plan PLAN.json --incumbent-run RUN_ID --candidate-run RUN_ID \\
        --work DIR [--history HISTORY.jsonl] [--axon-loop BIN] [--record]
    python3 promote.py rollback --work DIR --reason TEXT [--record]
    python3 promote.py check
    python3 promote.py standin --rows RUN_AB_DRYRUN.jsonl --corpus CORPUS.json --out ROWS.jsonl

`run` drives `axon-loop` end to end on a fresh store under DIR for ONE
candidate against the incumbent `policy/cortex-policy.json`:

  config -> candidates put -> tasks put -> policy put -> baseline -> activate
  -> evo propose -> plan register/freeze -> plan assign -> intake (one per
  trial) -> evl evaluate -> admit -> [pointer transition: promote] -> tel

Every step asserts its exit code and the field that matters, and prints it.
The evidence is the scorecard's: two run records from
`../scorecard/history.jsonl` (re-verified by `scorecard.verify`, judged by
`scorecard.judge`; no metric is recomputed here), one trial per (task, arm)
built from the run's own rows, signed by the grader key of this store.

It refuses before any `axon-loop` call when the plan was not committed
strictly BEFORE the evaluation rows (git ancestry), when the plan was changed
after it was added, when a run's config does not decode to the policy it
stands for, or when the candidate is not ONE `evo propose` edit away from the
incumbent.

The decision is PROMOTE only if `axon-loop` admission says ACCEPT AND the
plan's Cortex rule passes (0 false successes; verified not significantly
lower by exact McNemar; proposals or cost per verified fix lower). `--record`
appends the decision to `decisions.jsonl` and, on PROMOTE, writes the
candidate to `policy/cortex-policy.json`; `crates/axon-cortex/tests/
promoted_policy.rs` then fails until Cortex's compiled defaults are changed
to match in the same commit.

`rollback` revokes the promoted candidate in DIR's store and rolls the
pointer back to the incumbent-of-record; `--record` restores the policy file.

`check` verifies `policy/cortex-policy.json` is the policy the last
`decisions.jsonl` record put in place.

`standin` turns zero-cost run_ab.py dry-run rows (oracle / none generator)
into scorecard rows under the model `stand-in:<variant>`, for dry_run plans.

Standard library only; needs `git` and `openssl` (Ed25519) on PATH.
"""
import argparse
import datetime
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))
BENCH = os.path.dirname(HERE)
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(BENCH)))
sys.path.insert(0, os.path.join(BENCH, "scorecard"))
import scorecard as sc  # noqa: E402  (history records, verify, judge)
import corpus as corpus_mod  # noqa: E402  (../ab/corpus.py, on sc's path)

POLICY = os.path.join(HERE, "policy", "cortex-policy.json")
OPTIONS = os.path.join(HERE, "policy", "options.json")
DECISIONS = os.path.join(HERE, "decisions.jsonl")
RUN_AB = os.path.join(BENCH, "ab", "run_ab.py")

POLICY_SCHEMA = "cortex-policy/1"
PLAN_SCHEMA = "cortex-promotion-plan/1"
DECISION_SCHEMA = "cortex-promotion-decision/1"
POLICY_KEYS = ("ranker", "audit", "walk", "model", "transport")
# Records that put a policy in place; `check` and the Rust invariant follow
# the last of these.
PLACING = ("GRANDFATHERED", "PROMOTE", "ROLLBACK")

SCOPE = {"tenant_id": "axon", "task_family": "cortex-repair"}
ADMITTER = "op:cortex-admitter"
PROPOSER = "agent:cortex-promotion"
VERIFIER = "grader:run-ab"
OBSERVER = "observer:run-ab"
EVALUATOR = "evl:cortex-promotion"
SUBJECT = "agent:cortex"
BACKEND = "run-ab:local"
GRADER_EXE = "run-ab-grade"
SUITE_ID = "hidden"
INC_ARM, CAND_ARM = "incumbent", "challenger-1"
# run_ab.py reserves B_TASK_RESERVE (default $3) per arm-B task before it
# starts; that reservation is each trial's execution liability here.
RESERVE_MICRO = 3_000_000
ALPHA = sc.ALPHA


class Refused(Exception):
    """promote.py refuses; nothing is recorded."""


# ---------------------------------------------------------------- digests

def canonical(v):
    """The `cl22:` canonical bytes (crates/axon-loop-contracts/src/canonical.rs)."""
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()


def sha(b):
    return hashlib.sha256(b).hexdigest()


def cl22(v):
    return "cl22:" + sha(canonical(v))


def load_json(path):
    with open(path) as fh:
        return json.load(fh)


def rel(path):
    return os.path.relpath(os.path.abspath(path), ROOT).replace(os.sep, "/")


# ---------------------------------------------------------------- git

def git(*args):
    r = subprocess.run(["git", "-C", ROOT, *args], capture_output=True, text=True)
    return r.returncode, r.stdout.strip()


def committed_once(path, what):
    """The commit that added `path`; refuse if untracked, dirty, or changed since."""
    p = rel(path)
    if git("ls-files", "--error-unmatch", p)[0] != 0:
        raise Refused(f"{what} {p} is not committed")
    if git("status", "--porcelain", "--", p)[1]:
        raise Refused(f"{what} {p} has uncommitted changes")
    added = git("log", "--diff-filter=A", "--format=%H", "--", p)[1].split()
    touched = git("log", "--format=%H", "--", p)[1].split()
    if not added:
        raise Refused(f"{what} {p}: no commit adds it")
    if touched[0] != added[0]:
        raise Refused(f"{what} {p} was changed after it was committed (in {touched[0][:12]}); "
                      "a frozen document is never edited - write a new one")
    return added[0]


def require_plan_first(plan_path, rows_paths):
    p = committed_once(plan_path, "plan")
    for rows in rows_paths:
        r = committed_once(rows, "evaluation rows")
        if r == p or git("merge-base", "--is-ancestor", p, r)[0] != 0:
            raise Refused(f"the plan {rel(plan_path)} (commit {p[:12]}) was not committed "
                          f"before the evaluation rows {rel(rows)} (commit {r[:12]}): the decision "
                          "rule must be frozen before any evaluation row exists")
    return p


# ---------------------------------------------------------------- policies

def load_options():
    o = load_json(OPTIONS)
    if o.get("schema") != "cortex-promotion-options/1" or tuple(o["slots"]) != POLICY_KEYS:
        raise Refused(f"{OPTIONS}: unexpected schema or slots")
    for oid in o["options"]:
        if oid.split(".", 1)[0] not in POLICY_KEYS:
            raise Refused(f"option {oid}: unknown slot")
    return o


def check_policy(p, where):
    if not isinstance(p, dict) or p.get("schema") != POLICY_SCHEMA or \
            set(p) != {"schema", *POLICY_KEYS}:
        raise Refused(f"{where}: a policy is {{schema: {POLICY_SCHEMA}, "
                      f"{', '.join(POLICY_KEYS)}}}")
    return p


def option_ids(opts, slot):
    return sorted(k for k in opts["options"] if k.split(".", 1)[0] == slot)


def encode(opts, policy):
    """Policy -> shortlist: per slot, the selected option, then the slot's others."""
    out = []
    for slot in POLICY_KEYS:
        ids = option_ids(opts, slot)
        sel = [k for k in ids if opts["options"][k] == policy[slot]]
        if not sel:
            raise Refused(f"{slot} {policy[slot]!r} is not an option in {rel(OPTIONS)}")
        out += sel + [k for k in ids if k != sel[0]]
    return out


def decode(opts, shortlist):
    """Shortlist -> policy: per slot, the FIRST listed option is the one in force."""
    p = {"schema": POLICY_SCHEMA}
    for slot in POLICY_KEYS:
        first = next((k for k in shortlist if k.split(".", 1)[0] == slot), None)
        if first is None:
            raise Refused(f"shortlist {shortlist} selects no {slot}")
        p[slot] = opts["options"][first]
    return p


def envelope(opts, policy, cset_ref, controls_ref, policy_id):
    return {"schema": "axon.closed-loop.policy/1", "policy_id": policy_id,
            "parent_policy_ref": "cl22:" + "0" * 64, "scope": SCOPE, "mode": "shortlist_only",
            "candidate_set_ref": cset_ref, "controls_ref": controls_ref,
            "shortlist": encode(opts, policy), "discovery_evidence_refs": [],
            "authority_expansion": False}


# EVO (crates/axon-loop/src/evo.rs) - predicted here only to choose the seed
# whose first untried one-edit mutation is the requested change; axon-loop's
# output is then asserted to be exactly that shortlist.
M64 = (1 << 64) - 1


def splitmix64(state):
    state = (state + 0x9E3779B97F4A7C15) & M64
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & M64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & M64
    return state, z ^ (z >> 31)


def mutation_space(lst):
    m = [("remove", c) for c in lst] if len(lst) > 1 else []
    m += [("swap", lst[i], lst[j]) for i in range(len(lst)) for j in range(i + 1, len(lst))]
    return m


def apply(lst, m):
    if m[0] == "remove":
        return [c for c in lst if c != m[1]]
    a, b = m[1], m[2]
    return [b if c == a else a if c == b else c for c in lst]


def evo_pick(lst, seed, tried):
    space = mutation_space(lst)
    st = seed
    for i in range(len(space) - 1, 0, -1):
        st, r = splitmix64(st)
        j = r % (i + 1)
        space[i], space[j] = space[j], space[i]
    for m in space:
        s = apply(lst, m)
        if s not in tried:
            return m, s
    return None, None


def one_edit(opts, inc_policy, cand_policy):
    """The single swap that turns the incumbent's shortlist into the candidate."""
    lst = encode(opts, inc_policy)
    diff = [k for k in POLICY_KEYS if inc_policy[k] != cand_policy[k]]
    if len(diff) != 1:
        raise Refused(f"the candidate changes {diff or 'nothing'}: `evo propose` makes ONE edit, "
                      "so a promotion changes exactly one of " + ", ".join(POLICY_KEYS))
    slot = diff[0]
    target = [k for k in option_ids(opts, slot) if opts["options"][k] == cand_policy[slot]]
    if not target:
        raise Refused(f"{slot} {cand_policy[slot]!r} is not an option in {rel(OPTIONS)}")
    want = apply(lst, ("swap", lst[lst.index(next(k for k in lst if k.startswith(slot + ".")))],
                       target[0]))
    assert decode(opts, want) == cand_policy
    for seed in range(1 << 16):
        m, s = evo_pick(lst, seed, [lst])
        if s == want:
            return seed, m, want
    raise Refused("no seed below 65536 makes `evo propose` choose this edit")


# ---------------------------------------------------------------- Ed25519 (openssl)

class Signer:
    def __init__(self, keydir):
        self.key = os.path.join(keydir, "grader.pem")
        self.msg = os.path.join(keydir, "msg.bin")
        subprocess.run(["openssl", "genpkey", "-algorithm", "ed25519", "-out", self.key],
                       check=True, capture_output=True)
        der = subprocess.run(["openssl", "pkey", "-in", self.key, "-pubout", "-outform", "DER"],
                             check=True, capture_output=True).stdout
        self.pub = der[-32:].hex()
        self.key_id = "ed25519:" + sha(der[-32:])[:16]

    def attest(self, binding):
        b = dict(binding, key_id=self.key_id)
        with open(self.msg, "wb") as fh:
            fh.write(canonical(b))
        sig = subprocess.run(["openssl", "pkeyutl", "-sign", "-inkey", self.key, "-rawin",
                              "-in", self.msg], check=True, capture_output=True).stdout
        return dict(b, alg="ed25519", public_key=self.pub, signature=sig.hex())


# ---------------------------------------------------------------- axon-loop

def default_bin():
    if os.environ.get("AXON_LOOP_BIN"):
        return os.environ["AXON_LOOP_BIN"]
    target = os.environ.get("CARGO_TARGET_DIR") or os.path.join(ROOT, "target")
    return os.path.join(target, "debug", "axon-loop")


class Loop:
    def __init__(self, binary, work):
        self.bin = binary
        self.work = work
        self.store = os.path.join(work, "store")
        self.docs = os.path.join(work, "docs")
        self.out = os.path.join(work, "out")
        for d in (self.store, self.docs, self.out):
            os.makedirs(d, exist_ok=True)

    def doc(self, name, value):
        path = os.path.join(self.docs, name + ".json")
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as fh:
            fh.write(canonical(value))
        return path

    def call(self, name, want, *args, store=True, quiet=False):
        argv = [self.bin] + (["--store", self.store] if store else []) + list(args)
        shown = " ".join(argv[1:]).replace(self.work, "$WORK")
        if not quiet:
            print(f"$ axon-loop {shown}")
        r = subprocess.run(argv, capture_output=True, text=True)
        with open(os.path.join(self.out, name + ".json"), "w") as fh:
            fh.write(r.stdout)
        with open(os.path.join(self.out, name + ".err"), "w") as fh:
            fh.write(r.stderr)
        if r.returncode != want:
            raise SystemExit(f"FAIL [{name}]: expected exit {want}, got {r.returncode}\n"
                             f"{r.stderr.strip()}")
        if want != 0:
            e = json.loads(r.stderr)
            print(f"  -> exit {r.returncode} (expected): "
                  + json.dumps({k: e.get(k) for k in ("error", "exit_code", "message")}))
            return e
        return json.loads(r.stdout) if r.stdout.strip() else {}


def expect(name, what, got, want):
    if got != want:
        raise SystemExit(f"FAIL [{name}]: {what} is {got!r}, expected {want!r}")
    shown = json.dumps(got) if not isinstance(got, str) else got
    print(f"  ok: {what} = {shown}")


def section(title):
    print(f"\n== {title}", flush=True)


def now_ms():
    return time.time_ns() // 1_000_000


# ---------------------------------------------------------------- evidence

def load_tasks(corpus_path):
    """{task id: task} with the derived texts (broken file), as scorecard loads it."""
    d = load_json(corpus_path)
    if "sources" in d.get("meta", {}):
        d = corpus_mod.load(corpus_path)
    return {t["id"]: t for t in d["tasks"]}


class Evidence:
    """Turns scorecard rows into the trial documents MiCode/Fabric would deliver.

    Real digests where a Cortex run has the thing: the task's broken file
    (input workspace), the run's own row (output and source episode), the
    grader script (`run_ab.py`, the verifier's pinned executable), the
    corpus (check suite version), the run config (agent executable), the
    hidden check's name. Fabric-only fields with no Cortex counterpart
    (hardware, limits other than the reservation, principal/grant) keep the
    constants of examples/closed-loop/inputs/trial/.
    """

    def __init__(self, signer, corpus_sha, cset_ref, controls_ref, data_use_ref):
        self.signer = signer
        self.suite = f"check-suite:{SUITE_ID}@acf1:{corpus_sha}#corpus"
        self.cset_ref, self.controls_ref, self.data_use_ref = cset_ref, controls_ref, data_use_ref
        with open(RUN_AB, "rb") as fh:
            grader_sha = sha(fh.read())
        self.grader_digest = "acf1:" + sha(canonical(
            {"registered_executable_ref": GRADER_EXE, "sha256": grader_sha}))

    def trial(self, task, rec, raw, norm, pol_doc, arm, trial, role, epoch):
        ident = {"task_id": norm["id"], "arm_id": arm, "trial_id": trial,
                 "attempt_id": trial + "-a1", "operation_id": trial + "-a1-op",
                 "execution_id": trial + "-a1-ex"}
        ids4 = {k: ident[k] for k in ("operation_id", "task_id", "trial_id", "attempt_id")}
        ids5 = dict(ids4, execution_id=ident["execution_id"])
        pol_ref = cl22(pol_doc)
        pdig = "acf1:" + pol_ref[5:]
        ws_in = "acf1:" + sha(task["broken"].encode())
        ws_out = "acf1:" + sha(canonical(raw))
        cfg = rec["config"]
        now = now_ms()
        facts = {"repo_id": "axon", "base_commit": cfg.get("cortex_commit") or "unrecorded",
                 "workspace_ref": ws_in, "branch": "run-ab", "worktree_id": "run-ab-" + norm["id"],
                 "working_directory": "/isolated/" + norm["id"],
                 "build_namespace": "/build/" + trial,
                 "model_ref": cl22({"model": cfg["model"], "transport": cfg["transport"]}),
                 "role": "implementation", "namespace_ref": cl22({"run_id": rec["run_id"]}),
                 "read_paths": [task["file"]], "write_paths": [task["file"]],
                 "is_primary_worktree": False}
        context = {"schema": "axon.closed-loop.context/1", "context_id": "run-ab-" + trial,
                   "identity": ident, "scope": SCOPE, "expected": facts, "observed": facts,
                   "expected_issuer_ref": "run-ab:parent", "observed_issuer_ref": OBSERVER,
                   "observed_evidence_ref": "sha256:" + rec["rows_sha256"],
                   "created_ms": now, "expires_ms": now + 3_600_000, "authority_epoch": epoch}
        limits = {"cpu_millicores": 1000, "memory_bytes": 268435456, "disk_bytes": 1073741824,
                  "wall_time_ms": 300000, "output_bytes": 1048576,
                  "max_cost_micro": RESERVE_MICRO, "currency_code": "USD",
                  "price_schedule_ref": "unpriced:run-ab"}
        acf_req = dict(ids4, schema="acf-compute-request/1", principal_ref="cortex:run-ab",
                       grant_ref="grant:run-ab", approval_ref=None, job_kind="registered_check",
                       registered_executable_ref="cortex-repair",
                       executable_digest="acf1:" + sha(canonical(cfg)),
                       workspace_version_ref=ws_in, semantic_state_ref=None, policy_digest=pdig,
                       required={"engine": "native_process", "hardware_isolation": True,
                                 "os": "linux", "architecture": "x86_64",
                                 "network_mode": "deny", "checkpoint_kind": "logical_workspace"},
                       limits=limits, argv=[], result_schema_ref="cortex-repair-outcome/1")
        acf_rc = dict(ids5, schema="acf-execution-receipt/1", backend_profile_ref="run-ab:agent",
                      input_workspace_ref=ws_in, output_workspace_ref=ws_out, policy_digest=pdig,
                      status="completed", process_exit_code=0, verification="not_run",
                      matched_checks=0, evidence_source="supervisor_observed", evidence_refs=[],
                      usage_state="unknown", cost_micro=None,
                      unresolved_liability_micro=RESERVE_MICRO)
        result = "passed" if norm["verified"] else "failed"
        chk_req = dict(ids4, schema="acf-compute-request/1", principal_ref="principal:grader",
                       grant_ref="grant:check", approval_ref=None, job_kind="registered_check",
                       registered_executable_ref=GRADER_EXE, executable_digest=self.grader_digest,
                       workspace_version_ref=ws_out, semantic_state_ref=None, policy_digest=pdig,
                       required={"engine": "axon_interpreter", "hardware_isolation": False,
                                 "os": "none", "architecture": "x86_64", "network_mode": "deny",
                                 "checkpoint_kind": "none"},
                       limits=dict(limits, disk_bytes=268435456, wall_time_ms=60000,
                                   max_cost_micro=100),
                       argv=[f"check:{SUITE_ID}", task["hidden"]],
                       result_schema_ref="cortex-check-report/1")
        chk_rc = dict(ids5, schema="acf-execution-receipt/1", backend_profile_ref=BACKEND,
                      input_workspace_ref=ws_out, output_workspace_ref=None, policy_digest=pdig,
                      status="completed", process_exit_code=0, verification=result,
                      matched_checks=1, evidence_source="supervisor_observed",
                      evidence_refs=["sha256:" + rec["rows_sha256"], self.suite],
                      usage_state="unknown", cost_micro=None, unresolved_liability_micro=100)
        cost = norm["cost_usd"]
        if cost is None:
            usage_state, cost_micro = "unknown", None
        else:
            usage_state = "estimated" if norm["cost_estimated"] else "final"
            cost_micro = int(round(cost * 1_000_000))
        episode = {"schema": "axon.closed-loop.episode/1", "identity": ident, "scope": SCOPE,
                   "policy_ref": pol_ref, "controls_ref": self.controls_ref,
                   "context_ref": cl22(context), "candidate_set_ref": self.cset_ref,
                   "input_workspace_ref": ws_in, "output_workspace_ref": ws_out,
                   "acf_request_ref": cl22(acf_req), "acf_receipt_ref": cl22(acf_rc),
                   "source_episode_ref": "sha256:" + sha(canonical(raw)),
                   "authority_epoch": epoch, "status": "completed",
                   "verification": {"result": result, "matched_checks": 1, "issuer_ref": VERIFIER,
                                    "verifier_ref": cl22(chk_rc), "output_workspace_ref": ws_out,
                                    "evidence_refs": [cl22(chk_req)]},
                   "usage": {"state": usage_state, "cost_micro": cost_micro,
                             "unresolved_liability_micro": 0, "currency": "USD",
                             "price_schedule_ref": cl22({"cost_estimated": norm["cost_estimated"],
                                                         "run_id": rec["run_id"]}),
                             "attempt_refs": [cl22(trial)]},
                   "corpus_role": role, "data_use_ref": self.data_use_ref, "projection_ref": None}
        projection = {"sidecar_policy_ref": pol_ref, "acf_policy_digest": pdig,
                      "projection_ref": cl22({"sidecar_policy_ref": pol_ref,
                                              "acf_policy_digest": pdig})}
        binding = {"schema": "acf-receipt-attestation/2", "issuer_ref": VERIFIER, "key_id": None,
                   "issued_ms": now_ms(), "request_ref": cl22(chk_req),
                   "receipt_ref": cl22(chk_rc), **{k: ident[k] for k in (
                       "operation_id", "task_id", "trial_id", "attempt_id", "execution_id")}}
        return {"episode": episode, "context": context, "acf_request": acf_req,
                "acf_receipt": acf_rc, "projection": projection,
                "verification_request": chk_req, "verification_receipt": chk_rc,
                "verification_attestation": self.signer.attest(binding)}


# ---------------------------------------------------------------- the Cortex rule

def cortex_rule(judgement, ma, mb):
    """The plan's rule, read off `scorecard.judge` (A = incumbent, B = candidate)."""
    steps = judgement["steps"]
    fs_ok = mb["false_success"] == 0
    mcn = steps[1]
    verified_ok = mcn["better"] != "A"

    def per_fix(m, k):
        # A run with no verified fix has an infinite cost per verified fix.
        return float("inf") if m["verified"] == 0 else m[k]

    eff = {}
    for k in ("proposals_per_verified", "cost_per_verified"):
        a, b = per_fix(ma, k), per_fix(mb, k)
        eff[k] = {"incumbent": None if a == float("inf") else a,
                  "candidate": None if b == float("inf") else b,
                  "lower": a is not None and b is not None and b < a}
    eff_ok = any(v["lower"] for v in eff.values())
    criteria = [
        {"criterion": "candidate false successes = 0", "pass": fs_ok,
         "candidate": mb["false_success"], "incumbent": ma["false_success"]},
        {"criterion": f"verified not significantly lower (exact McNemar, alpha {ALPHA})",
         "pass": verified_ok, "candidate": mb["verified"], "incumbent": ma["verified"],
         "mcnemar_p": judgement["paired"]["mcnemar_p"], "judge": mcn["criterion"]},
        {"criterion": "proposals or cost per verified fix lower", "pass": eff_ok, **eff},
    ]
    return all(c["pass"] for c in criteria), criteria


# ---------------------------------------------------------------- run

def check_run(rec, policy, opts, plan, role, dry_run):
    if rec["agent"] != "cortex":
        raise Refused(f"run {rec['run_id']} is agent {rec['agent']}: only Cortex runs evaluate a "
                      "Cortex policy")
    cfg = rec["config"]
    standin = dry_run and str(cfg["model"]).startswith("stand-in:")
    keys = ("ranker", "audit", "walk") if standin else POLICY_KEYS
    bad = {k: (cfg[k], policy[k]) for k in keys if cfg[k] != policy[k]}
    if bad:
        raise Refused(f"run {rec['run_id']} ({role}) ran {', '.join(f'{k}={v[0]!r}' for k, v in bad.items())}"
                      f" but the {role} policy says {', '.join(f'{k}={v[1]!r}' for k, v in bad.items())}")
    if rec["set"] != plan["set"]:
        raise Refused(f"run {rec['run_id']} is on set {rec['set']}, the plan evaluates on {plan['set']}")
    if rec["corpus"]["sha256"] != plan["corpus"]["sha256"]:
        raise Refused(f"run {rec['run_id']} was scored on corpus {rec['corpus']['sha256'][:12]}, "
                      f"the plan names {plan['corpus']['sha256'][:12]}")
    return standin


def load_plan(path, dry_run_ok):
    plan = load_json(path)
    if plan.get("schema") != PLAN_SCHEMA:
        raise Refused(f"{path}: schema must be {PLAN_SCHEMA}")
    for k in ("experiment_id", "dry_run", "set", "corpus", "incumbent", "candidate",
              "discovery_run", "rule", "axon_loop"):
        if k not in plan:
            raise Refused(f"{path}: missing {k}")
    if plan["set"] != "heldout" and not plan["dry_run"]:
        raise Refused("a promotion evaluates on the heldout set; only a dry_run plan may use "
                      f"{plan['set']!r}")
    if plan["dry_run"] and not dry_run_ok:
        raise Refused("a dry_run plan cannot be --record'ed")
    return plan


def placing_records():
    if not os.path.exists(DECISIONS):
        raise Refused(f"{DECISIONS} is missing")
    recs = [json.loads(line) for line in open(DECISIONS) if line.strip()]
    for r in recs:
        if r.get("schema") != DECISION_SCHEMA:
            raise Refused(f"{DECISIONS}: a record has schema {r.get('schema')!r}")
    placing = [r for r in recs if r["decision"] in PLACING]
    if not placing:
        raise Refused(f"{DECISIONS}: no record places a policy")
    return placing


def cmd_check():
    last = placing_records()[-1]
    with open(POLICY, "rb") as fh:
        got = sha(fh.read())
    if got != last["policy_after"]["sha256"]:
        raise Refused(f"{rel(POLICY)} (sha256 {got[:12]}) is not the policy the last decision "
                      f"({last['decision']} {last.get('experiment_id')}) put in place "
                      f"({last['policy_after']['sha256'][:12]})")
    check_policy(load_json(POLICY), rel(POLICY))
    print(f"ok: {rel(POLICY)} = policy_after of {last['decision']} "
          f"{last.get('experiment_id') or ''} ({last['date']})")


def cmd_run(a):
    plan_path = os.path.abspath(a.plan)
    plan = load_plan(plan_path, dry_run_ok=not a.record)
    dry = plan["dry_run"]
    opts = load_options()
    with open(POLICY, "rb") as fh:
        inc_bytes = fh.read()
    inc_policy = check_policy(json.loads(inc_bytes), rel(POLICY))
    cand_path = os.path.join(ROOT, plan["candidate"]["path"])
    with open(cand_path, "rb") as fh:
        cand_bytes = fh.read()
    cand_policy = check_policy(json.loads(cand_bytes), plan["candidate"]["path"])
    if plan["incumbent"] != {"path": rel(POLICY), "sha256": sha(inc_bytes)}:
        raise Refused(f"the plan's incumbent {plan['incumbent']} is not the policy in force "
                      f"({rel(POLICY)} sha256 {sha(inc_bytes)}): the plan is stale")
    if plan["candidate"]["sha256"] != sha(cand_bytes):
        raise Refused(f"{plan['candidate']['path']} changed since the plan named it")
    cmd_check()
    seed, mutation, want_shortlist = one_edit(opts, inc_policy, cand_policy)

    history = os.path.abspath(a.history)
    recs = sc.load_history(history)
    by_id = {r["run_id"]: r for r in recs}
    for i in (a.incumbent_run, a.candidate_run):
        if i not in by_id:
            raise Refused(f"run {i} is not in {history}")
    inc_rec, cand_rec = by_id[a.incumbent_run], by_id[a.candidate_run]
    # Discovery evidence is always a real dev run from the canonical history.
    canon = {r["run_id"]: r for r in sc.load_history(sc.HISTORY)}
    if plan["discovery_run"] not in canon:
        raise Refused(f"discovery run {plan['discovery_run']} is not in {rel(sc.HISTORY)}")
    disc_rec = canon[plan["discovery_run"]]
    if len({inc_rec["rows_sha256"], cand_rec["rows_sha256"], disc_rec["rows_sha256"]}) != 3:
        raise Refused("discovery, incumbent and candidate runs must be three different row files")
    if disc_rec["set"] != "dev":
        raise Refused(f"discovery run {disc_rec['run_id']} is on set {disc_rec['set']}: only dev "
                      "evidence may reach the proposer")
    plan_commit = require_plan_first(plan_path, [os.path.join(ROOT, r["rows"])
                                                 for r in (inc_rec, cand_rec)])
    standin = {r["run_id"]: check_run(r, p, opts, plan, role, dry) for r, p, role in (
        (inc_rec, inc_policy, "incumbent"), (cand_rec, cand_policy, "candidate"))}
    disc_policy = {"schema": POLICY_SCHEMA, **{k: disc_rec["config"][k] for k in POLICY_KEYS}}
    encode(opts, disc_policy)
    if os.path.exists(a.work) and os.listdir(a.work):
        raise Refused(f"work dir {a.work} is not empty")

    section("0. the scorecard's evidence (scorecard.verify + scorecard.judge)")
    norms = sc.verify(recs, sc.ROOT)
    judgement = sc.judge(recs, norms, inc_rec["run_id"], cand_rec["run_id"])
    sc.print_compare(judgement)
    rule_ok, criteria = cortex_rule(judgement, inc_rec["metrics"], cand_rec["metrics"])
    for c in criteria:
        print(f"  rule: {c['criterion']}: {'PASS' if c['pass'] else 'FAIL'}")
    print(f"  plan {rel(plan_path)} committed in {plan_commit[:12]}, an ancestor of both "
          "evaluation rows' commits")

    work = os.path.abspath(a.work)
    loop = Loop(os.path.abspath(a.axon_loop), work)
    os.makedirs(os.path.join(work, "keys"), exist_ok=True)
    signer = Signer(os.path.join(work, "keys"))
    tasks = load_tasks(os.path.join(ROOT, plan["corpus"]["path"]))
    disc_tasks = load_tasks(os.path.join(ROOT, disc_rec["corpus"]["path"]))
    task_ids = sorted(r["id"] for r in norms[inc_rec["run_id"]])
    if task_ids != sorted(r["id"] for r in norms[cand_rec["run_id"]]):
        raise Refused("the two runs do not cover the same tasks")

    section("1. operator config (written by promote.py: no axon-loop verb writes config.json)")
    controls_ref = cl22(load_json(OPTIONS))
    cset = sorted(opts["options"])
    cset_ref = cl22(cset)
    data_use_ref = cl22({"data_use": "evaluation rows decide this experiment only; they never "
                                     "reach a proposer", "plan": cl22(plan)})
    ev = Evidence(signer, plan["corpus"]["sha256"], cset_ref, controls_ref, data_use_ref)
    config = {"schema": "axon.loop.config/1", "trusted_admitters": [ADMITTER],
              "trusted_verifiers": [VERIFIER], "trusted_observers": [OBSERVER],
              "verifier_keys": {VERIFIER: signer.pub},
              "verifier_pins": {VERIFIER: {"registered_executable_ref": GRADER_EXE,
                                           "executable_digest": ev.grader_digest,
                                           "backend_profiles": [BACKEND],
                                           "check_suites": [ev.suite]}},
              "task_acceptance": {t: {"check_suite": ev.suite, "check": tasks[t]["hidden"]}
                                  for t in task_ids},
              "protected_scopes": [], "trusted_monitors": [], "monitor_keys": {},
              "observer_keys": {}}
    with open(os.path.join(loop.store, "config.json"), "wb") as fh:
        fh.write(canonical(config))
    loop.doc("config", config)
    print(f"  verifier {VERIFIER}: key {signer.key_id}, pinned grader {ev.grader_digest[:20]}... "
          f"(sha256 of run_ab.py), suite {ev.suite[:40]}...")

    section("2. register the candidate list (the option table) and the task manifest")
    out = loop.call("candidates-put", 0, "candidates", "put", "--in", loop.doc(
        "candidate-set", {"schema": "axon.loop.candidate-set/1", "scope": SCOPE,
                          "candidates": cset, "issuer_ref": ADMITTER}))
    expect("candidates-put", ".candidate_set_ref", out["candidate_set_ref"], cset_ref)
    manifest = {"schema": "axon.loop.task-manifest/1", "scope": SCOPE, "tasks": task_ids,
                "issuer_ref": ADMITTER}
    out = loop.call("tasks-put", 0, "tasks", "put", "--in", loop.doc("task-manifest", manifest))
    tasks_ref = out["task_manifest_ref"]
    print(f"  task_manifest_ref = {tasks_ref} ({len(task_ids)} tasks)")

    section("3. store the incumbent policy (policy/cortex-policy.json, encoded as a shortlist)")
    inc_env = envelope(opts, inc_policy, cset_ref, controls_ref, "cortex-incumbent")
    out = loop.call("policy-put", 0, "policy", "put", "--in", loop.doc("policy-incumbent", inc_env))
    inc_ref = out["policy_ref"]
    expect("policy-put", ".policy_ref", inc_ref, cl22(inc_env))
    print(f"  shortlist {inc_env['shortlist']}")

    section("4. designate the incumbent-of-record and activate it (epoch 0 -> 1)")
    loop.call("resolve-paused", 6, "pointer", "resolve", "--tenant", SCOPE["tenant_id"],
              "--family", SCOPE["task_family"])
    placed = placing_records()[-1]
    baseline = {"schema": "axon.loop.baseline/1", "scope": SCOPE, "policy_ref": inc_ref,
                "issuer_ref": ADMITTER, "reason_ref": cl22(placed)}
    out = loop.call("baseline", 0, "pointer", "baseline", "--in", loop.doc("baseline", baseline))
    baseline_ref = out["baseline_ref"]
    print(f"  baseline_ref = {baseline_ref} (reason: the {placed['decision']} record)")
    null_ref = "cl22:" + "0" * 64

    def transition(name, tid, kind, expected, target, epoch, adm, reason):
        return loop.doc(name, {"schema": "axon.closed-loop.transition/1", "transition_id": tid,
                               "kind": kind, "scope": SCOPE, "expected_policy_ref": expected,
                               "target_policy_ref": target, "expected_epoch": epoch,
                               "next_epoch": epoch + 1, "admission_ref": adm,
                               "reason_ref": reason, "issuer_ref": ADMITTER,
                               "mechanism_test": False})

    out = loop.call("activate-incumbent", 0, "pointer", "transition", "--in", transition(
        "transition-boot", "boot", "activate", null_ref, inc_ref, 0, baseline_ref, cl22(placed)))
    expect("activate-incumbent", ".pointer.epoch", out["pointer"]["epoch"], 1)
    expect("activate-incumbent", ".pointer.active_policy_ref", out["pointer"]["active_policy_ref"],
           inc_ref)

    section(f"5. EVO: one edit from the dev-set discovery run {disc_rec['run_id']}")
    disc_env = envelope(opts, disc_policy, cset_ref, controls_ref, "cortex-discovery")
    disc_raw, disc_norm = sc.run_inputs(disc_rec, sc.ROOT)
    disc_eps = []
    for raw, norm in zip(disc_raw, disc_norm):
        if norm["verified"] and len(disc_eps) < 256:
            t = ev.trial(disc_tasks[norm["id"]], disc_rec, raw, norm, disc_env, INC_ARM,
                         "d-" + norm["id"], "discovery", 1)
            if t["episode"]["usage"]["state"] == "final":
                disc_eps.append(t["episode"])
    print(f"  {len(disc_eps)} verified discovery episodes with final usage "
          f"(of {disc_rec['metrics']['verified']} verified)")
    if not disc_eps:
        raise Refused(f"discovery run {disc_rec['run_id']} has no verified row with a reported "
                      "(not estimated) cost: axon-loop's learning gate admits only final usage")
    evo_req = {"schema": "axon.loop.evo-request/1", "incumbent": inc_env, "eligible": cset,
               "episodes": disc_eps, "seed": seed, "new_policy_id": "cortex-candidate",
               "proposer_ref": PROPOSER,
               "rationale": f"{plan['experiment_id']}: {mutation[0]} {' <-> '.join(mutation[1:])} "
                            f"-> {', '.join(f'{k}={cand_policy[k]}' for k in POLICY_KEYS if cand_policy[k] != inc_policy[k])}"}
    out = loop.call("evo", 0, "evo", "propose", "--in", loop.doc("evo-request", evo_req))
    cand_env = out["candidate"]
    cand_ref = out["candidate_policy_ref"]
    expect("evo", ".candidate_policy_ref", cand_ref, cl22(cand_env))
    expect("evo", ".candidate.shortlist", cand_env["shortlist"], want_shortlist)
    expect("evo", "decode(.candidate.shortlist)", decode(opts, cand_env["shortlist"]), cand_policy)
    loop.doc("policy-candidate", cand_env)

    section(f"6. plan: register the frozen plan {plan['experiment_id']} and freeze it")
    ax = plan["axon_loop"]
    n_trials = 2 * len(task_ids)
    pilot = {"schema": "axon.closed-loop.pilot/1", "experiment_id": plan["experiment_id"],
             "scope": SCOPE, "mutable_surface": "eligible_tool_skill_shortlist",
             "runtime_ready": True, "deployment_enabled": True, "operator_approved": True,
             "live_evidence": [], "task_manifest_ref": tasks_ref,
             "repository_split_ref": cl22({"discovery_corpus": disc_rec["corpus"]["sha256"],
                                           "evaluation_corpus": plan["corpus"]["sha256"]}),
             "discovery_manifest_ref": cl22({"role": "discovery", "run_id": disc_rec["run_id"],
                                             "rows_sha256": disc_rec["rows_sha256"]}),
             "confirmation_manifest_ref": cl22({"role": "confirmation",
                                                "corpus_sha256": plan["corpus"]["sha256"],
                                                "set": plan["set"]}),
             "reporting_manifest_ref": cl22({"role": "reporting",
                                             "history": rel(sc.HISTORY)}),
             "controls_ref": controls_ref, "incumbent_policy_ref": inc_ref,
             "candidate_policy_ref": cand_ref, "analysis_method_ref": cl22(plan),
             "data_use_ref": data_use_ref,
             "rollback_policy_ref": cl22({"rollback": "promote.py rollback: revoke the "
                                                      "candidate, roll back to the baseline"}),
             "approval_ref": cl22({"plan_commit": plan_commit}),
             "independent_units": len(task_ids), "repetitions": 1, "candidate_budget": 1,
             "independent_unit": "task", "quality_margin": ax["quality_margin"],
             "economic_threshold": ax["economic_threshold"],
             "uncertainty_rule": "exact_bounds", "missing_data_rule": "unknown_bounds",
             "multiplicity_rule": "single_candidate", "order_rule": "paired_tasks",
             "cache_rule": "not_enforced_here",
             "budget_rule": f"max_unresolved_liability_micro={n_trials * (RESERVE_MICRO + 100)}"}
    out = loop.call("plan-register", 0, "plan", "register", "--in", loop.doc("plan", pilot))
    expect("plan-register", ".unset_fields | length", len(out["unset_fields"]), 0)
    out = loop.call("plan-freeze", 0, "plan", "freeze", "--experiment", plan["experiment_id"])
    plan_ref = out["plan_ref"]
    out = loop.call("plan-show", 0, "plan", "show", "--experiment", plan["experiment_id"])
    expect("plan-show", ".frozen_ref", out["frozen_ref"], plan_ref)
    expect("plan-show", ".start_blockers | length", len(out["start_blockers"]), 0)
    print(f"  analysis_method_ref = cl22 of {rel(plan_path)}; approval_ref = cl22 of its commit")

    section("7. the admitter issues the trial population (after the freeze, before any trial)")
    arms = ((INC_ARM, "i-", inc_ref, inc_rec), (CAND_ARM, "c-", cand_ref, cand_rec))
    assignment = {"schema": "axon.loop.assignment/1", "experiment_id": plan["experiment_id"],
                  "scope": SCOPE, "issuer_ref": ADMITTER,
                  "trials": [{"task_id": t, "arm_id": arm, "trial_id": pfx + t,
                              "attempt_id": pfx + t + "-a1", "policy_ref": ref}
                             for arm, pfx, ref, _ in arms for t in task_ids]}
    out = loop.call("plan-assign", 0, "plan", "assign", "--in", loop.doc("assignment", assignment))
    expect("plan-assign", ".trials", out["trials"], n_trials)

    section(f"8. trials: one per (task, arm) from the runs' rows; intake each ({n_trials})")
    acks = os.path.join(loop.docs, "acks")
    os.makedirs(acks, exist_ok=True)
    for name, env in (("incumbent", inc_env), ("candidate", cand_env)):
        ack = {"schema": "micode.closed-loop.policy-ack/1",
               "pin": {"state": "pinned", "policy_id": env["policy_id"], "policy_ref": cl22(env),
                       "controls_ref": env["controls_ref"],
                       "candidate_set_ref": env["candidate_set_ref"],
                       "shortlist": env["shortlist"]},
               "candidates": cset, "candidate_set_ref": env["candidate_set_ref"]}
        with open(os.path.join(acks, name + ".json"), "wb") as fh:
            fh.write(canonical(ack))
    trials, episodes, recorded = [], [], 0
    for arm, pfx, ref, rec in arms:
        env = inc_env if arm == INC_ARM else cand_env
        raws, ns = sc.run_inputs(rec, sc.ROOT)
        for raw, norm in sorted(zip(raws, ns), key=lambda x: x[1]["id"]):
            t = ev.trial(tasks[norm["id"]], rec, raw, norm, env, arm, pfx + norm["id"],
                         "confirmation", 1)
            d = f"trials/{pfx}{norm['id']}"
            paths = {k: loop.doc(f"{d}/{k}", v) for k, v in t.items()}
            out = loop.call("intake-" + pfx + norm["id"], 0, "intake", "episode",
                            "--in", paths["episode"], "--context", paths["context"],
                            "--ack", acks, "--verification-request", paths["verification_request"],
                            "--verification-receipt", paths["verification_receipt"],
                            "--verification-attestation", paths["verification_attestation"],
                            quiet=True)
            if out.get("recorded_now") is not True:
                raise SystemExit(f"FAIL [intake-{pfx}{norm['id']}]: recorded_now is "
                                 f"{out.get('recorded_now')!r}")
            recorded += 1
            trials.append(t)
            episodes.append(t["episode"])
    print(f"$ axon-loop --store $WORK/store intake episode --in $WORK/docs/trials/<trial>/episode.json ... "
          f"(x{n_trials})")
    expect("intake", "trials recorded_now = true", recorded, n_trials)

    section("9. EVL: evaluate the frozen experiment")
    evl_req = {"schema": "axon.loop.evl-request/1", "experiment_id": plan["experiment_id"],
               "scope": SCOPE, "evaluator_ref": EVALUATOR, "subject_issuers": [SUBJECT],
               "policies": [inc_env, cand_env],
               "assigned": [{k: v for k, v in x.items() if k != "attempt_id"}
                            for x in assignment["trials"]],
               "trials": trials}
    path = loop.doc("evl-request", evl_req)
    print(f"  request: {os.path.getsize(path)} bytes")
    out = loop.call("evl", 0, "evl", "evaluate", "--in", path)
    eval_ref = out["evaluation_ref"]
    arms_out = {x["arm_id"]: x for x in out["evaluation"]["arms"]}
    for arm, _, _, rec in arms:
        x = arms_out[arm]
        expect("evl", f"{arm}.verified_pass (= scorecard verified of {rec['run_id']})",
               x["verified_pass"], rec["metrics"]["verified"])
        expect("evl", f"{arm}.unknown + missing", x["unknown"] + x["missing"], 0)
        print(f"  {arm} economics: {json.dumps(x['economics'])}")

    section("10. admission: apply the frozen plan's rule to the evaluation")
    out = loop.call("admit", 0, "admit", "--in", loop.doc(
        "admit-request", {"schema": "axon.loop.admit-request/1",
                          "experiment_id": plan["experiment_id"], "evaluation_ref": eval_ref,
                          "admitter_ref": ADMITTER, "mechanism_test": False}))
    adm_ref = out["admission_ref"]
    adm = out["admission"]
    print(f"  admission_ref = {adm_ref}\n  axon-loop decision: {adm['decision']}; "
          f"reasons: {json.dumps(adm['reasons'])}")
    promote = adm["decision"] == "ACCEPT" and rule_ok
    decision = "PROMOTE" if promote else ("INCONCLUSIVE" if adm["decision"] == "INCONCLUSIVE"
                                          else "REJECT")
    print(f"  Cortex rule: {'PASS' if rule_ok else 'FAIL'} -> decision {decision}")

    transition_ref = None
    if promote:
        section("11. move the pointer to the candidate (CAS on incumbent @ epoch 1 -> 2)")
        out = loop.call("promote", 0, "pointer", "transition", "--in", transition(
            "transition-promote", "promote-1", "activate", inc_ref, cand_ref, 1, adm_ref,
            cl22(judgement)))
        expect("promote", ".pointer.epoch", out["pointer"]["epoch"], 2)
        expect("promote", ".pointer.active_policy_ref", out["pointer"]["active_policy_ref"],
               cand_ref)
        transition_ref = out.get("transition_ref")
        out = loop.call("resolve-2", 0, "pointer", "resolve", "--tenant", SCOPE["tenant_id"],
                        "--family", SCOPE["task_family"])
        expect("resolve-2", ".pin.epoch", out["pin"]["epoch"], 2)
        expect("resolve-2", ".pin.version.digest", out["pin"]["version"]["digest"], cand_ref)
        expect("resolve-2", ".admission_ref", out["admission_ref"], adm_ref)
    else:
        section("11. no transition: the pointer stays on the incumbent")
        out = loop.call("resolve-1", 0, "pointer", "resolve", "--tenant", SCOPE["tenant_id"],
                        "--family", SCOPE["task_family"])
        expect("resolve-1", ".pin.version.digest", out["pin"]["version"]["digest"], inc_ref)

    section("12. TEL: whole-task economics over the trials' usage")
    out = loop.call("tel", 0, "tel", "summarize", "--in", loop.doc(
        "tel-request", {"schema": "axon.loop.tel-request/1", "episodes": episodes}), store=False)
    expect("tel", ".summary.records", out["summary"]["records"], n_trials)
    print(f"  by currency: {json.dumps(out['summary']['by_currency'])}")
    with open(os.path.join(loop.store, "ledger.head")) as fh:
        head = json.load(fh)

    record = {"schema": DECISION_SCHEMA, "decision": decision,
              "date": datetime.date.today().isoformat(), "experiment_id": plan["experiment_id"],
              "dry_run": dry,
              "plan": {"path": rel(plan_path), "sha256": sha(canonical(plan)),
                       "commit": plan_commit},
              "incumbent": {"policy": plan["incumbent"], "run_id": inc_rec["run_id"],
                            "rows_sha256": inc_rec["rows_sha256"]},
              "candidate": {"policy": plan["candidate"], "run_id": cand_rec["run_id"],
                            "rows_sha256": cand_rec["rows_sha256"]},
              "discovery_run": disc_rec["run_id"],
              "stand_in_generators": sorted(k for k, v in standin.items() if v),
              "cortex_rule": {"pass": rule_ok, "criteria": criteria},
              "judge": judgement,
              "axon_loop": {"plan_ref": plan_ref, "incumbent_policy_ref": inc_ref,
                            "candidate_policy_ref": cand_ref, "evaluation_ref": eval_ref,
                            "admission_ref": adm_ref, "admission": adm["decision"],
                            "reasons": adm["reasons"], "baseline_ref": baseline_ref,
                            "transition_ref": transition_ref, "ledger_head": head},
              "policy_after": {"path": rel(POLICY), "sha256": sha(cand_bytes if promote
                                                                  else inc_bytes)}}
    state = {"record": record, "incumbent_policy": inc_bytes.decode(),
             "candidate_policy": cand_bytes.decode(), "inc_ref": inc_ref, "cand_ref": cand_ref,
             "baseline_ref": baseline_ref, "admission_ref": adm_ref, "axon_loop": loop.bin}
    with open(os.path.join(work, "state.json"), "w") as fh:
        json.dump(state, fh, indent=1)
    with open(os.path.join(work, "decision.json"), "w") as fh:
        json.dump(record, fh, indent=1)

    section(f"decision: {decision}")
    print(f"  written to $WORK/decision.json; ledger head seq {head.get('seq')}")
    if a.record:
        with open(DECISIONS, "a") as fh:
            fh.write(json.dumps(record, sort_keys=True) + "\n")
        print(f"  appended to {rel(DECISIONS)}")
        if promote:
            with open(POLICY, "wb") as fh:
                fh.write(cand_bytes)
            print(f"  {rel(POLICY)} := {plan['candidate']['path']}; now change Cortex's compiled "
                  "defaults to match (tests/promoted_policy.rs fails until they do)")
    return 0


def cmd_rollback(a):
    work = os.path.abspath(a.work)
    with open(os.path.join(work, "state.json")) as fh:
        st = json.load(fh)
    rec = st["record"]
    if rec["decision"] != "PROMOTE":
        raise Refused(f"{work}: the run there decided {rec['decision']}; nothing to roll back")
    if a.record and rec["dry_run"]:
        raise Refused("a dry_run promotion cannot be --record'ed")
    if a.record:
        last = placing_records()[-1]
        if last.get("experiment_id") != rec["experiment_id"] or last["decision"] != "PROMOTE":
            raise Refused(f"the last placing decision is {last['decision']} "
                          f"{last.get('experiment_id')}, not this promotion")
    loop = Loop(st["axon_loop"], work)
    reason = {"rollback_of": rec["experiment_id"], "reason": a.reason}
    sc_args = ("--tenant", SCOPE["tenant_id"], "--family", SCOPE["task_family"])

    section("13. revoke the promoted candidate, then roll back to the incumbent-of-record")
    loop.call("revoke", 0, "pointer", "revoke", *sc_args, "--policy", st["cand_ref"],
              "--reason", cl22(reason), "--issuer", ADMITTER)
    loop.call("resolve-revoked", 6, "pointer", "resolve", *sc_args)
    path = loop.doc("transition-rollback", {
        "schema": "axon.closed-loop.transition/1", "transition_id": "rollback-1",
        "kind": "rollback", "scope": SCOPE, "expected_policy_ref": st["cand_ref"],
        "target_policy_ref": st["inc_ref"], "expected_epoch": 2, "next_epoch": 3,
        "admission_ref": st["baseline_ref"], "reason_ref": cl22(reason), "issuer_ref": ADMITTER,
        "mechanism_test": False})
    out = loop.call("rollback", 0, "pointer", "transition", "--in", path)
    expect("rollback", ".pointer.epoch", out["pointer"]["epoch"], 3)
    expect("rollback", ".pointer.active_policy_ref", out["pointer"]["active_policy_ref"],
           st["inc_ref"])
    out = loop.call("resolve-3", 0, "pointer", "resolve", *sc_args)
    expect("resolve-3", ".pin.epoch", out["pin"]["epoch"], 3)
    expect("resolve-3", ".pin.version.digest", out["pin"]["version"]["digest"], st["inc_ref"])
    out = loop.call("show", 0, "pointer", "show", *sc_args)
    expect("show", ".transitions", out["transitions"], 3)
    expect("show", ".revocations.revoked | length", len(out["revocations"]["revoked"]), 1)
    with open(os.path.join(loop.store, "ledger.head")) as fh:
        head = json.load(fh)
    inc_sha = sha(st["incumbent_policy"].encode())
    record = {"schema": DECISION_SCHEMA, "decision": "ROLLBACK",
              "date": datetime.date.today().isoformat(), "experiment_id": rec["experiment_id"],
              "dry_run": rec["dry_run"], "reason": a.reason,
              "axon_loop": {"revoked_policy_ref": st["cand_ref"],
                            "restored_policy_ref": st["inc_ref"],
                            "baseline_ref": st["baseline_ref"], "ledger_head": head},
              "policy_after": {"path": rel(POLICY), "sha256": inc_sha}}
    with open(os.path.join(work, "rollback.json"), "w") as fh:
        json.dump(record, fh, indent=1)
    section("decision: ROLLBACK")
    print(f"  written to $WORK/rollback.json; ledger head seq {head.get('seq')}")
    if a.record:
        with open(DECISIONS, "a") as fh:
            fh.write(json.dumps(record, sort_keys=True) + "\n")
        with open(POLICY, "w") as fh:
            fh.write(st["incumbent_policy"])
        print(f"  appended to {rel(DECISIONS)}; {rel(POLICY)} restored to the incumbent")
    return 0


STANDIN_VARIANTS = ("oracle", "none")


def cmd_standin(a):
    """Zero-cost dry-run rows (run_ab.py --arm B --generator oracle|none) as
    scorecard rows: `scorecard.normalize` of each row, plus the provenance
    run_ab.py stamped. The scorecard refuses run_ab.py dry-run rows as they
    are ("rows carry no model"), so a dry run scores them under the model
    `stand-in:<variant>`; `run` accepts that model only in a dry_run plan."""
    truth = sc.load_corpus(os.path.abspath(a.corpus))
    raw = sc.read_jsonl(a.rows)
    if not raw or not all(sc.is_run_ab_row(r) for r in raw):
        raise Refused(f"{a.rows}: not run_ab.py rows")
    meta = sc.row_meta(raw)
    if meta.get("arm") != "B" or meta.get("model") or meta.get("variant") not in STANDIN_VARIANTS:
        raise Refused(f"{a.rows}: standin converts zero-cost arm-B dry-run rows "
                      f"(--generator {'|'.join(STANDIN_VARIANTS)}) only; meta is {meta}")
    asked = {}
    for k in ("ranker", "audit"):
        s = {(r.get(k) or {}).get("requested") for r in raw}
        if len(s) != 1 or None in s:
            raise Refused(f"{a.rows}: rows do not agree on the requested {k}: {sorted(map(str, s))}")
        asked[k] = s.pop()
    used = sorted({(r.get("ranker") or {}).get("used") or "?" for r in raw})
    out = []
    for r in raw:
        n = sc.normalize(r, truth)
        n["standin"] = {"variant": meta["variant"], "commit": meta.get("commit"),
                        "cortex_sha256_16": meta.get("cortex_sha256_16"),
                        "ranker_used": (r.get("ranker") or {}).get("used"),
                        "audit_verdict": (r.get("audit") or {}).get("verdict"),
                        "outcome": r.get("outcome")}
        out.append(n)
    os.makedirs(os.path.dirname(os.path.abspath(a.out)), exist_ok=True)
    with open(a.out, "w") as fh:
        for n in sorted(out, key=lambda x: x["id"]):
            fh.write(json.dumps(n, sort_keys=True) + "\n")
    cfg = {"model": f"stand-in:{meta['variant']}", "transport": "stand-in", **asked,
           "walk": "breadth", "cortex_commit": meta.get("commit"),
           "cortex_sha256": meta.get("cortex_sha256_16")}
    print(json.dumps(cfg, sort_keys=True))
    print(f"standin: {len(out)} rows, verified {sum(n['verified'] for n in out)}, ranker requested "
          f"{asked['ranker']} used {used}, audit requested {asked['audit']}", file=sys.stderr)
    return 0


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    r = sub.add_parser("run")
    r.add_argument("--plan", required=True)
    r.add_argument("--incumbent-run", required=True)
    r.add_argument("--candidate-run", required=True)
    r.add_argument("--work", required=True)
    r.add_argument("--history", default=sc.HISTORY)
    r.add_argument("--axon-loop", default=default_bin())
    r.add_argument("--record", action="store_true")
    b = sub.add_parser("rollback")
    b.add_argument("--work", required=True)
    b.add_argument("--reason", required=True)
    b.add_argument("--record", action="store_true")
    sub.add_parser("check")
    s = sub.add_parser("standin")
    s.add_argument("--rows", required=True)
    s.add_argument("--corpus", required=True)
    s.add_argument("--out", required=True)
    a = ap.parse_args(argv)
    try:
        if a.cmd == "run":
            return cmd_run(a)
        if a.cmd == "rollback":
            return cmd_rollback(a)
        if a.cmd == "standin":
            return cmd_standin(a)
        cmd_check()
        return 0
    except (Refused, sc.Refused) as e:
        print(f"promote.py: refused: {e}", file=sys.stderr)
        return 3


if __name__ == "__main__":
    sys.exit(main())
