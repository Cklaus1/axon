#!/usr/bin/env bash
# Drive the v0.22 closed loop (`axon-loop`) end to end on a scratch store.
#
#   bash examples/closed-loop/run.sh <path-to-axon-loop-binary> [work-dir]
#
# Every input is a hand-written JSON file under inputs/. Fields marked
# "SET-BY-RUN.SH" are references (digests) or times that only exist once an
# earlier step has run; this script fills them with jq and keeps every
# document it actually sent under <work-dir>/docs/.
#
# The trial evidence (episodes, context receipts, policy acks, check
# request/receipt, verifier attestation) stands in for what MiCode and
# Compute Fabric produce in a real deployment. The demo verifier key is
# generated here with openssl; nothing is read from or written to
# /etc/axon/trust.
#
# Every step asserts its exit code and the field that matters. The script
# exits 0 only if every assertion held. Requires: bash, jq, openssl (3.x,
# Ed25519), sha256sum, od.
#
# If AXON_ATTEST_KEY is set (hex, >= 16 bytes) the main store is keyed by it.
# The last section always runs a separate keyed-ledger demo on its own store.
set -euo pipefail

if [ $# -lt 1 ]; then
    echo "usage: $0 <axon-loop-binary> [work-dir]" >&2
    exit 2
fi
BIN=$(realpath "$1")
[ -x "$BIN" ] || { echo "not executable: $BIN" >&2; exit 2; }
HERE=$(cd "$(dirname "$0")" && pwd)
IN=$HERE/inputs
for tool in jq openssl sha256sum od; do
    command -v "$tool" >/dev/null || { echo "missing tool: $tool" >&2; exit 2; }
done

if [ $# -ge 2 ]; then
    WORK=$2
    mkdir -p "$WORK"
    [ -z "$(ls -A "$WORK")" ] || { echo "work dir $WORK is not empty" >&2; exit 2; }
else
    WORK=$(mktemp -d "${TMPDIR:-/tmp}/closed-loop.XXXXXX")
fi
WORK=$(cd "$WORK" && pwd)
STORE=$WORK/store
DOCS=$WORK/docs
OUT=$WORK/out
mkdir -p "$STORE" "$DOCS" "$OUT" "$WORK/keys"

TENANT=demo-tenant
FAMILY=demo-coding
SC=(--tenant "$TENANT" --family "$FAMILY")
NULL_REF=cl22:0000000000000000000000000000000000000000000000000000000000000000

# ---------------------------------------------------------------- helpers

# cl22: of a JSON document on stdin: sha256 over the canonical bytes
# (keys sorted, no whitespace), as crates/axon-loop-contracts/src/canonical.rs.
cl22() {
    local h
    h=$(jq -S -c . | tr -d '\n' | sha256sum)
    printf 'cl22:%s' "${h%% *}"
}

# Not `date +%s%3N`: uutils `date` (the default on recent Ubuntu) ignores the
# 3 width and printed timestamps up to ~1 s in the future, which EVL refuses.
now_ms() { local ns; ns=$(date +%s%N); echo $((ns / 1000000)); }

show_cmd() {
    local s="axon-loop $*"
    s=${s//$WORK/\$WORK}
    echo "\$ ${s//$HERE/examples/closed-loop}"
}

# call NAME WANT_EXIT ARGS... : run axon-loop against $STORE, keep stdout in
# $OUT/NAME.json and stderr in $OUT/NAME.err, assert the exit code.
call() {
    local name=$1 want=$2
    shift 2
    show_cmd --store "$STORE" "$@"
    local got=0
    "$BIN" --store "$STORE" "$@" >"$OUT/$name.json" 2>"$OUT/$name.err" || got=$?
    if [ "$got" != "$want" ]; then
        echo "FAIL [$name]: expected exit $want, got $got" >&2
        cat "$OUT/$name.err" >&2
        exit 1
    fi
    if [ "$want" != 0 ]; then
        echo "  -> exit $got (expected): $(jq -c '{error, exit_code, message}' "$OUT/$name.err")"
    fi
}

# expect NAME JQ_FILTER WANT : assert a field of step NAME's output.
expect() {
    local got
    got=$(jq -rc "$2" "$OUT/$1.json")
    if [ "$got" != "$3" ]; then
        echo "FAIL [$1]: $2 is '$got', expected '$3'" >&2
        exit 1
    fi
    echo "  ok: $2 = $got"
}

field() { jq -r "$2" "$OUT/$1.json"; }

section() { printf '\n== %s\n' "$*"; }

# ---------------------------------------------------------------- Ed25519 keys
# verifier.pem stands in for Compute Fabric's verifier signer. Its public key
# is registered in the store's config (verifier_keys). Development-class
# evidence is checked against that key; PROTECTED evidence would also need
# the key in the root-owned /etc/axon/trust/verifier/. worker.pem is a key
# nobody registered, used once to show a forged attestation being refused.
openssl genpkey -algorithm ed25519 -out "$WORK/keys/verifier.pem" 2>/dev/null
openssl genpkey -algorithm ed25519 -out "$WORK/keys/worker.pem" 2>/dev/null
pub_hex() { # KEYFILE -> 64-hex raw public key
    openssl pkey -in "$1" -pubout -outform DER | tail -c 32 | od -An -v -tx1 | tr -d ' \n'
}
key_id() { # KEYFILE -> ed25519:<first 16 hex of sha256(raw public key)>
    local h
    h=$(openssl pkey -in "$1" -pubout -outform DER | tail -c 32 | sha256sum)
    printf 'ed25519:%s' "${h:0:16}"
}
VERIFIER_PUB=$(pub_hex "$WORK/keys/verifier.pem")

# attest KEYFILE BINDING_FILE: an acf-receipt-attestation/2. The signature is
# over the canonical bytes of the binding with key_id set to KEYFILE's id
# (crates/axon-loop-contracts/src/attestation.rs).
attest() {
    local b sig
    b=$(jq -c --arg kid "$(key_id "$1")" '.key_id = $kid' "$2")
    printf '%s' "$b" | jq -S -c . | tr -d '\n' >"$WORK/keys/msg.bin"
    sig=$(openssl pkeyutl -sign -inkey "$1" -rawin -in "$WORK/keys/msg.bin" |
        od -An -v -tx1 | tr -d ' \n')
    jq -n --argjson b "$b" --arg pk "$(pub_hex "$1")" --arg s "$sig" \
        '$b + {alg:"ed25519", public_key:$pk, signature:$s}'
}

# The pinned acceptance-check executable: acf1 identity of
# {registered_executable_ref, sha256} (crates/axon-workspace-recipe).
CHECK_EXE=demo-acceptance-check
CHECK_EXE_SHA=$(printf 'demo acceptance check bytes' | sha256sum)
CHECK_EXE_SHA=${CHECK_EXE_SHA%% *}
H=$(printf '{"registered_executable_ref":"%s","sha256":"%s"}' "$CHECK_EXE" "$CHECK_EXE_SHA" | sha256sum)
CHECK_EXE_DIGEST="acf1:${H%% *}"

# ---------------------------------------------------------------- 1. operator config
section "1. operator config (written by hand: no axon-loop verb writes config.json)"
jq --arg k "$VERIFIER_PUB" --arg d "$CHECK_EXE_DIGEST" \
    '.verifier_keys["demo:verifier"] = $k
     | .verifier_pins["demo:verifier"].executable_digest = $d' \
    "$IN/config.json" >"$STORE/config.json"
cp "$STORE/config.json" "$DOCS/config.json"
echo "wrote \$WORK/store/config.json: admitters=$(jq -c .trusted_admitters "$STORE/config.json")" \
    "verifiers=$(jq -c .trusted_verifiers "$STORE/config.json")" \
    "observers=$(jq -c .trusted_observers "$STORE/config.json")"

# ---------------------------------------------------------------- 2. registries
section "2. register the candidate list and the task manifest"
call candidates-put 0 candidates put --in "$IN/candidate-set.json"
CAND_SET_REF=$(field candidates-put .candidate_set_ref)
WANT=$(jq -c '.candidates | sort' "$IN/candidate-set.json" | cl22)
expect candidates-put .candidate_set_ref "$WANT"

call tasks-put 0 tasks put --in "$IN/task-manifest.json"
TASKS_REF=$(field tasks-put .task_manifest_ref)
echo "  task_manifest_ref = $TASKS_REF"

section "3. store the incumbent policy"
jq --arg c "$CAND_SET_REF" '.candidate_set_ref = $c' "$IN/policy-incumbent.json" \
    >"$DOCS/policy-incumbent.json"
call policy-put 0 policy put --in "$DOCS/policy-incumbent.json"
INC_REF=$(field policy-put .policy_ref)
expect policy-put .policy_ref "$(cl22 <"$DOCS/policy-incumbent.json")"

# ---------------------------------------------------------------- 4. pointer from paused
section "4. the scope starts paused: resolve refuses"
call resolve-paused 6 pointer resolve "${SC[@]}"

section "5. designate the incumbent-of-record and activate it (epoch 0 -> 1)"
jq --arg p "$INC_REF" '.policy_ref = $p' "$IN/baseline.json" >"$DOCS/baseline.json"
call baseline 0 pointer baseline --in "$DOCS/baseline.json"
BASELINE_REF=$(field baseline .baseline_ref)
echo "  baseline_ref = $BASELINE_REF"

# transition DOC_NAME ID KIND EXPECTED TARGET EXPECTED_EPOCH ADMISSION
make_transition() {
    jq --arg id "$2" --arg kind "$3" --arg exp "$4" --arg tgt "$5" \
        --argjson e "$6" --arg adm "$7" \
        '.transition_id = $id | .kind = $kind | .expected_policy_ref = $exp
         | .target_policy_ref = (if $tgt == "" then null else $tgt end)
         | .expected_epoch = $e | .next_epoch = ($e + 1)
         | .admission_ref = (if $adm == "" then null else $adm end)' \
        "$IN/transition.json" >"$DOCS/$1.json"
}
make_transition transition-boot boot activate "$NULL_REF" "$INC_REF" 0 "$BASELINE_REF"
call activate-incumbent 0 pointer transition --in "$DOCS/transition-boot.json"
expect activate-incumbent .pointer.epoch 1
expect activate-incumbent .pointer.active_policy_ref "$INC_REF"

call resolve-1 0 pointer resolve "${SC[@]}"
expect resolve-1 .pin.epoch 1
expect resolve-1 .pin.version.digest "$INC_REF"

# ---------------------------------------------------------------- trial evidence
# make_trial DIR POLICY_DOC ARM TASK TRIAL RESULT COST ROLE EPOCH
# Builds what MiCode (episode sidecar, context receipt) and Fabric (the
# acceptance check's request/receipt and the verifier's attestation) would
# deliver for one trial, with every cross-reference digest computed.
make_trial() {
    local dir=$1 pol=$2 arm=$3 task=$4 trial=$5 result=$6 cost=$7 role=$8 epoch=$9
    mkdir -p "$dir"
    local now ident pref
    now=$(now_ms)
    pref=$(cl22 <"$pol")
    ident=$(jq -nc --arg t "$task" --arg a "$arm" --arg tr "$trial" \
        '{task_id:$t, arm_id:$a, trial_id:$tr, attempt_id:($tr+"-a1"),
          operation_id:($tr+"-a1-op"), execution_id:($tr+"-a1-ex")}')
    jq --argjson id "$ident" --argjson now "$now" --argjson e "$epoch" \
        '.identity = $id | .created_ms = $now | .expires_ms = ($now + 3600000)
         | .authority_epoch = $e' "$IN/trial/context.json" >"$dir/context.json"
    jq --argjson id "$ident" \
        '.operation_id = $id.operation_id | .task_id = $id.task_id
         | .trial_id = $id.trial_id | .attempt_id = $id.attempt_id' \
        "$IN/trial/acf-request.json" >"$dir/acf-request.json"
    jq --argjson id "$ident" \
        '.operation_id = $id.operation_id | .task_id = $id.task_id
         | .trial_id = $id.trial_id | .attempt_id = $id.attempt_id
         | .execution_id = $id.execution_id' \
        "$IN/trial/acf-receipt.json" >"$dir/acf-receipt.json"
    jq --argjson id "$ident" --arg d "$CHECK_EXE_DIGEST" \
        '.operation_id = $id.operation_id | .task_id = $id.task_id
         | .trial_id = $id.trial_id | .attempt_id = $id.attempt_id
         | .executable_digest = $d' \
        "$IN/trial/check-request.json" >"$dir/check-request.json"
    jq --argjson id "$ident" --arg v "$result" \
        '.operation_id = $id.operation_id | .task_id = $id.task_id
         | .trial_id = $id.trial_id | .attempt_id = $id.attempt_id
         | .execution_id = $id.execution_id | .verification = $v' \
        "$IN/trial/check-receipt.json" >"$dir/check-receipt.json"
    local creq_ref crc_ref
    creq_ref=$(cl22 <"$dir/check-request.json")
    crc_ref=$(cl22 <"$dir/check-receipt.json")
    jq --argjson id "$ident" --arg p "$pref" --arg cs "$(jq -r .candidate_set_ref "$pol")" \
        --arg ctx "$(cl22 <"$dir/context.json")" \
        --arg rq "$(cl22 <"$dir/acf-request.json")" --arg rc "$(cl22 <"$dir/acf-receipt.json")" \
        --argjson e "$epoch" --arg role "$role" --arg res "$result" \
        --arg vref "$crc_ref" --arg eref "$creq_ref" --argjson cost "$cost" \
        --arg aref "$(jq -n --arg t "$trial" '$t' | cl22)" \
        '.identity = $id | .policy_ref = $p | .candidate_set_ref = $cs | .context_ref = $ctx
         | .acf_request_ref = $rq | .acf_receipt_ref = $rc | .authority_epoch = $e
         | .corpus_role = $role
         | .verification.result = $res | .verification.verifier_ref = $vref
         | .verification.evidence_refs = [$eref]
         | .usage.cost_micro = $cost
         | .usage.state = (if $cost == null then "unknown" else "final" end)
         | .usage.attempt_refs = [$aref]' \
        "$IN/trial/episode.json" >"$dir/episode.json"
    jq --arg p "$pref" '.sidecar_policy_ref = $p' "$IN/trial/projection.json" >"$dir/projection.json"
    # What the verifier signs: which request/receipt pair, for which trial,
    # and when (issued_ms must be after the plan froze).
    jq -n --argjson ms "$(now_ms)" \
        --arg rq "$creq_ref" --arg rc "$crc_ref" --argjson id "$ident" \
        '{schema:"acf-receipt-attestation/2", issuer_ref:"demo:verifier", key_id:null,
          issued_ms:$ms, request_ref:$rq, receipt_ref:$rc,
          operation_id:$id.operation_id, task_id:$id.task_id, trial_id:$id.trial_id,
          attempt_id:$id.attempt_id, execution_id:$id.execution_id}' >"$dir/binding.json"
    attest "$WORK/keys/verifier.pem" "$dir/binding.json" >"$dir/attestation.json"
}

# ---------------------------------------------------------------- 6. EVO
section "6. EVO: propose one candidate from a discovery episode"
make_trial "$DOCS/discovery/d-1" "$DOCS/policy-incumbent.json" incumbent disc-task d-1 \
    passed 100 discovery 1
jq --slurpfile inc "$DOCS/policy-incumbent.json" \
    --slurpfile ep "$DOCS/discovery/d-1/episode.json" \
    '.incumbent = $inc[0] | .episodes = [$ep[0]]' "$IN/evo-request.json" >"$DOCS/evo-request.json"
call evo 0 evo propose --in "$DOCS/evo-request.json"
CAND_REF=$(field evo .candidate_policy_ref)
jq .candidate "$OUT/evo.json" >"$DOCS/policy-candidate.json"
expect evo .candidate_policy_ref "$(cl22 <"$DOCS/policy-candidate.json")"
echo "  candidate shortlist: $(jq -c .shortlist "$DOCS/policy-candidate.json")" \
    "(incumbent: $(jq -c .shortlist "$DOCS/policy-incumbent.json"))"

# ---------------------------------------------------------------- 7. plan
section "7. plan: an unconfigured plan registers but cannot be frozen"
call plan-register-unset 0 plan register --in "$IN/plan-unset.json"
expect plan-register-unset '.unset_fields | length' 24
call plan-freeze-unset 7 plan freeze --experiment operator-must-configure

section "8. plan: register the configured plan and freeze it"
jq --arg t "$TASKS_REF" --arg i "$INC_REF" --arg c "$CAND_REF" \
    '.task_manifest_ref = $t | .incumbent_policy_ref = $i | .candidate_policy_ref = $c' \
    "$IN/plan.json" >"$DOCS/plan.json"
call plan-register 0 plan register --in "$DOCS/plan.json"
expect plan-register '.unset_fields | length' 0
call plan-freeze 0 plan freeze --experiment demo-exp
PLAN_REF=$(field plan-freeze .plan_ref)
call plan-show 0 plan show --experiment demo-exp
expect plan-show .frozen_ref "$PLAN_REF"
expect plan-show '.start_blockers | length' 0
jq '.repetitions = 9' "$DOCS/plan.json" >"$DOCS/plan-changed.json"
call plan-register-frozen 4 plan register --in "$DOCS/plan-changed.json"

# ---------------------------------------------------------------- 9. assignment
section "9. the admitter issues the trial population (after the freeze, before any trial)"
jq --arg i "$INC_REF" --arg c "$CAND_REF" \
    '.trials |= map(.policy_ref = (if .arm_id == "incumbent" then $i else $c end))' \
    "$IN/assignment.json" >"$DOCS/assignment.json"
call plan-assign 0 plan assign --in "$DOCS/assignment.json"
expect plan-assign .trials 4

# ---------------------------------------------------------------- 10. trials + intake
section "10. trials: build the MiCode/Fabric evidence and intake each episode"
mkdir -p "$DOCS/acks"
for pol in policy-incumbent policy-candidate; do
    jq --slurpfile p "$DOCS/$pol.json" --arg r "$(cl22 <"$DOCS/$pol.json")" \
        '.pin = {state: "pinned", policy_id: $p[0].policy_id, policy_ref: $r,
                 controls_ref: $p[0].controls_ref, candidate_set_ref: $p[0].candidate_set_ref,
                 shortlist: $p[0].shortlist}
         | .candidate_set_ref = $p[0].candidate_set_ref' \
        "$IN/policy-ack.json" >"$DOCS/acks/$pol.json"
done
# arm task trial policy result cost
TRIALS=(
    "incumbent task-0 i0 policy-incumbent passed 100"
    "incumbent task-1 i1 policy-incumbent passed 100"
    "challenger-1 task-0 c0 policy-candidate passed 50"
    "challenger-1 task-1 c1 policy-candidate passed 50"
)
for spec in "${TRIALS[@]}"; do
    read -r arm task trial pol result cost <<<"$spec"
    make_trial "$DOCS/trials/$trial" "$DOCS/$pol.json" "$arm" "$task" "$trial" \
        "$result" "$cost" confirmation 1
    T=$DOCS/trials/$trial
    if [ "$trial" = i0 ]; then
        # Same bytes, but the attestation is signed by a key the operator never
        # registered for demo:verifier: naming the verifier proves nothing.
        attest "$WORK/keys/worker.pem" "$T/binding.json" >"$T/attestation-forged.json"
        call intake-forged 4 intake episode --in "$T/episode.json" --context "$T/context.json" \
            --ack "$DOCS/acks" --verification-request "$T/check-request.json" \
            --verification-receipt "$T/check-receipt.json" \
            --verification-attestation "$T/attestation-forged.json"
    fi
    call "intake-$trial" 0 intake episode --in "$T/episode.json" --context "$T/context.json" \
        --ack "$DOCS/acks" --verification-request "$T/check-request.json" \
        --verification-receipt "$T/check-receipt.json" \
        --verification-attestation "$T/attestation.json"
    expect "intake-$trial" .recorded_now true
done

# ---------------------------------------------------------------- 11. EVL
section "11. EVL: evaluate the frozen experiment"
TRIAL_ARRAY=$(for spec in "${TRIALS[@]}"; do
    read -r _ _ trial _ _ _ <<<"$spec"
    T=$DOCS/trials/$trial
    jq -n --slurpfile e "$T/episode.json" --slurpfile c "$T/context.json" \
        --slurpfile rq "$T/acf-request.json" --slurpfile rc "$T/acf-receipt.json" \
        --slurpfile vq "$T/check-request.json" --slurpfile vr "$T/check-receipt.json" \
        --slurpfile va "$T/attestation.json" --slurpfile pj "$T/projection.json" \
        '{episode: $e[0], context: $c[0], acf_request: $rq[0], acf_receipt: $rc[0],
          projection: $pj[0], verification_request: $vq[0], verification_receipt: $vr[0],
          verification_attestation: $va[0]}'
done | jq -s .)
jq --slurpfile i "$DOCS/policy-incumbent.json" --slurpfile c "$DOCS/policy-candidate.json" \
    --slurpfile a "$DOCS/assignment.json" --argjson t "$TRIAL_ARRAY" \
    '.policies = [$i[0], $c[0]] | .assigned = ($a[0].trials | map(del(.attempt_id)))
     | .trials = $t' "$IN/evl-request.json" >"$DOCS/evl-request.json"
call evl 0 evl evaluate --in "$DOCS/evl-request.json"
EVAL_REF=$(field evl .evaluation_ref)
echo "  evaluation_ref = $EVAL_REF"
jq -c '.evaluation.arms[]? | {arm_id, verified_pass, fail, unknown, missing}' "$OUT/evl.json" || true

# ---------------------------------------------------------------- 12. admission
section "12. admission: apply the frozen plan's rule to the evaluation"
jq --arg e "$EVAL_REF" '.evaluation_ref = $e | .admitter_ref = "op:stranger"' \
    "$IN/admit-request.json" >"$DOCS/admit-request-stranger.json"
call admit-stranger 4 admit --in "$DOCS/admit-request-stranger.json"
jq --arg e "$EVAL_REF" '.evaluation_ref = $e' "$IN/admit-request.json" >"$DOCS/admit-request.json"
call admit 0 admit --in "$DOCS/admit-request.json"
expect admit .admission.decision ACCEPT
ADM_REF=$(field admit .admission_ref)
echo "  admission_ref = $ADM_REF"
jq -c '.admission.reasons' "$OUT/admit.json"

# ---------------------------------------------------------------- 13. promote
section "13. move the pointer to the candidate (CAS on incumbent @ epoch 1 -> 2)"
make_transition transition-promote promote-1 activate "$INC_REF" "$CAND_REF" 1 "$ADM_REF"
call promote 0 pointer transition --in "$DOCS/transition-promote.json"
expect promote .pointer.epoch 2
expect promote .pointer.active_policy_ref "$CAND_REF"
# The same intent under a new id, still claiming epoch 1: a writer that read
# the pointer before the promotion. Compare-and-swap refuses it.
make_transition transition-stale promote-stale activate "$INC_REF" "$CAND_REF" 1 "$ADM_REF"
call promote-stale 5 pointer transition --in "$DOCS/transition-stale.json"
call resolve-2 0 pointer resolve "${SC[@]}"
expect resolve-2 .pin.epoch 2
expect resolve-2 .pin.version.digest "$CAND_REF"
expect resolve-2 .admission_ref "$ADM_REF"

# ---------------------------------------------------------------- 14. TEL
section "14. TEL: whole-task economics over the trials' Usage (no store needed)"
# tel_run NAME REQUEST: tel summarize reads no store.
tel_run() {
    show_cmd tel summarize --in "$2"
    "$BIN" tel summarize --in "$2" >"$OUT/$1.json" 2>"$OUT/$1.err" || {
        echo "FAIL [$1]" >&2
        cat "$OUT/$1.err" >&2
        exit 1
    }
}
EPISODES=()
for spec in "${TRIALS[@]}"; do
    read -r _ _ trial _ _ _ <<<"$spec"
    EPISODES+=("$DOCS/trials/$trial/episode.json")
done
jq -s --slurpfile t "$IN/tel-request.json" '$t[0] + {episodes: .}' "${EPISODES[@]}" \
    >"$DOCS/tel-request.json"
tel_run tel "$DOCS/tel-request.json"
expect tel .summary.records 4
expect tel '.summary.by_currency[0].total' '{"cost_micro":300,"state":"known"}'
# One more attempt whose cost was never reported: the total stops being a
# number. It is "unresolved", never a smaller known sum.
make_trial "$DOCS/tel-extra/c9" "$DOCS/policy-candidate.json" challenger-1 task-1 c9 \
    failed null confirmation 2
jq -s --slurpfile t "$IN/tel-request.json" '$t[0] + {episodes: .}' "${EPISODES[@]}" \
    "$DOCS/tel-extra/c9/episode.json" >"$DOCS/tel-request-unknown.json"
tel_run tel-unknown "$DOCS/tel-request-unknown.json"
expect tel-unknown .summary.records 5
expect tel-unknown '.summary.by_currency[0].total.state' unresolved
expect tel-unknown '.summary.by_currency[0].total.known_sum_micro' 300
expect tel-unknown '.summary.by_currency[0].total.unknown_count' 1
# The same episode twice is refused, not double-counted.
jq -s --slurpfile t "$IN/tel-request.json" '$t[0] + {episodes: .}' "${EPISODES[0]}" \
    "${EPISODES[0]}" >"$DOCS/tel-request-dup.json"
show_cmd tel summarize --in "$DOCS/tel-request-dup.json"
got=0
"$BIN" tel summarize --in "$DOCS/tel-request-dup.json" >/dev/null 2>"$OUT/tel-dup.err" || got=$?
[ "$got" = 4 ] || { echo "FAIL [tel-dup]: expected exit 4, got $got" >&2; exit 1; }
echo "  -> exit $got (expected): $(jq -c '{error, exit_code, message}' "$OUT/tel-dup.err")"

# ---------------------------------------------------------------- 15. revoke + rollback
section "15. revoke the candidate, then roll back to the incumbent"
call revoke 0 pointer revoke "${SC[@]}" --policy "$CAND_REF" \
    --reason cl22:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa --issuer op:admitter
call resolve-revoked 6 pointer resolve "${SC[@]}"
make_transition transition-rollback rollback-1 rollback "$CAND_REF" "$INC_REF" 2 "$BASELINE_REF"
call rollback 0 pointer transition --in "$DOCS/transition-rollback.json"
expect rollback .pointer.epoch 3
expect rollback .pointer.active_policy_ref "$INC_REF"
call resolve-3 0 pointer resolve "${SC[@]}"
expect resolve-3 .pin.epoch 3
expect resolve-3 .pin.version.digest "$INC_REF"
call show 0 pointer show "${SC[@]}"
expect show .transitions 3
expect show '.revocations.revoked | length' 1

# ---------------------------------------------------------------- 16. ledger
section "16. every accepted write is one entry in the store's hash-chained ledger"
echo "  ledger.jsonl: $(wc -l <"$STORE/ledger.jsonl") entries;" \
    "head: $(jq -c '{seq, entry_ref}' "$STORE/ledger.head")"
jq -r '"  \(.seq)\t\(.event.event)"' "$STORE/ledger.jsonl"

# ---------------------------------------------------------------- 17. keyed ledger
section "17. keyed ledger (AXON_ATTEST_KEY): a keyed store refuses to open without its key"
KSTORE=$WORK/keyed-store
mkdir -p "$KSTORE"
cp "$STORE/config.json" "$KSTORE/config.json"
DEMO_KEY=000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f
WRONG_KEY=ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100
# kcall NAME WANT_EXIT KEY ARGS... (KEY "-" = unset)
kcall() {
    local name=$1 want=$2 key=$3
    shift 3
    local got=0
    if [ "$key" = - ]; then
        echo "\$ env -u AXON_ATTEST_KEY axon-loop --store \$WORK/keyed-store $*"
        env -u AXON_ATTEST_KEY "$BIN" --store "$KSTORE" "$@" >"$OUT/$name.json" 2>"$OUT/$name.err" || got=$?
    else
        echo "\$ AXON_ATTEST_KEY=${key:0:8}... axon-loop --store \$WORK/keyed-store ${*//$HERE/examples/closed-loop}"
        AXON_ATTEST_KEY=$key "$BIN" --store "$KSTORE" "$@" >"$OUT/$name.json" 2>"$OUT/$name.err" || got=$?
    fi
    if [ "$got" != "$want" ]; then
        echo "FAIL [$name]: expected exit $want, got $got" >&2
        cat "$OUT/$name.err" >&2
        exit 1
    fi
    if [ "$want" != 0 ]; then
        echo "  -> exit $got (expected): $(jq -c '{error, exit_code, message}' "$OUT/$name.err")"
    fi
}
kcall keyed-put 0 "$DEMO_KEY" candidates put --in "$IN/candidate-set.json"
echo "  ledger entry carries a MAC: $(jq -c 'has("mac")' "$KSTORE/ledger.jsonl")" \
    "head: $(jq -c 'keys' "$KSTORE/ledger.head")"
kcall keyed-nokey 2 - pointer show "${SC[@]}"
kcall keyed-wrongkey 2 "$WRONG_KEY" pointer show "${SC[@]}"
kcall keyed-show 0 "$DEMO_KEY" pointer show "${SC[@]}"
expect keyed-show .transitions 0

section "done: every step above ran and every assertion held"
echo "work dir: $WORK"
