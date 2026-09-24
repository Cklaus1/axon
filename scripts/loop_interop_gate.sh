#!/usr/bin/env bash
# loop_interop_gate.sh — Axon v0.22 closed loop, PAIRED interop gate.
#
# Real bytes, both directions, no fixture parser on either side:
#   Axon DEC (axon-reflex decide_builtin → to_policy_envelope)
#     → axon-loop policy put / pointer baseline / pointer transition / pointer resolve
#     → the REAL `micode exec` binary consumes the resolved policy (fixture LLM
#       provider on 127.0.0.1 that records every request body; no credentials)
#     → MiCode writes context receipt + policy-ack + episode sidecar
#     → `axon-loop intake episode` parses and joins those files and records them in the ledger.
# Negative cases assert ABSENCE of effect: zero tool additions, zero provider
# requests, store bytes unchanged, nothing recorded.
#
# Env: MICODE_DIR (default ../micode-v022-wt), KEEP=1 keeps the temp dir.
# Exit 0 iff every assertion passed and at least one ran.
set -uo pipefail

AXON_DIR="$(cd "$(dirname "$0")/.." && pwd)"
MICODE_DIR="${MICODE_DIR:-$(cd "$AXON_DIR/../micode-v022-wt" 2>/dev/null && pwd)}"
[ -n "$MICODE_DIR" ] && [ -d "$MICODE_DIR" ] || { echo "FATAL: MICODE_DIR not found" >&2; exit 2; }

PASS=0; FAIL=0
ok()   { PASS=$((PASS+1)); echo "  PASS  $*"; }
bad()  { FAIL=$((FAIL+1)); echo "  FAIL  $*"; }
check() { local d="$1"; shift; if "$@"; then ok "$d"; else bad "$d"; fi; }
eq()   { [ "$1" == "$2" ] || { echo "        expected [$2] got [$1]" >&2; return 1; }; }
section() { echo; echo "== $*"; }

WORK="$(mktemp -d "${TMPDIR:-/tmp}/loop-interop.XXXXXX")"
PROVIDER_PID=""
cleanup() {
  [ -n "$PROVIDER_PID" ] && kill "$PROVIDER_PID" 2>/dev/null
  if [ "${KEEP:-0}" = 1 ]; then echo "kept $WORK"; else rm -rf "$WORK"; fi
}
trap cleanup EXIT

# ── build both sides ───────────────────────────────────────────────────────
section "build"
( cd "$AXON_DIR" && env -u CARGO_TARGET_DIR cargo build -q -p axon-loop --bins ) \
  || { echo "FATAL: axon-loop build failed" >&2; exit 2; }
( cd "$MICODE_DIR" && env -u CARGO_TARGET_DIR cargo build -q -p micode --bin micode ) \
  || { echo "FATAL: micode build failed" >&2; exit 2; }
AXL="$AXON_DIR/target/debug/axon-loop"
MICODE="$MICODE_DIR/target/debug/micode"
echo "axon   $(git -C "$AXON_DIR" rev-parse --short HEAD) $(git -C "$AXON_DIR" status --porcelain | grep -q . && echo '(dirty)')"
echo "micode $(git -C "$MICODE_DIR" rev-parse --short HEAD) $(git -C "$MICODE_DIR" status --porcelain | grep -q . && echo '(dirty)')"

# ── fixture provider: counts + captures requests (same SSE as micode's
#    closed_loop_v022_real_binary.rs); 10 input + 3 output tokens ─────────────
cat > "$WORK/provider.py" <<'PY'
import http.server, os, sys, threading
OUT, PORTFILE = sys.argv[1], sys.argv[2]
os.makedirs(OUT, exist_ok=True)
SSE = "".join([
 'event: message_start\ndata: {"type":"message_start","message":{"id":"m","type":"message","role":"assistant","model":"claude-sonnet-5","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":10,"cache_read_input_tokens":0,"cache_creation_input_tokens":0}}}\n\n',
 'event: content_block_start\ndata: {"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}\n\n',
 'event: content_block_delta\ndata: {"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"pong"}}\n\n',
 'event: content_block_stop\ndata: {"type":"content_block_stop","index":0}\n\n',
 'event: message_delta\ndata: {"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":3}}\n\n',
 'event: message_stop\ndata: {"type":"message_stop"}\n\n']).encode()
lock = threading.Lock(); n = [0]
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get('content-length', 0)))
        with lock:
            n[0] += 1; i = n[0]
        open(os.path.join(OUT, f"req-{i:04d}.json"), "wb").write(body)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(SSE)))
        self.send_header("Connection", "close")
        self.end_headers(); self.wfile.write(SSE)
    def log_message(self, *a): pass
s = http.server.ThreadingHTTPServer(("127.0.0.1", 0), H)
open(PORTFILE + ".tmp", "w").write(str(s.server_address[1])); os.rename(PORTFILE + ".tmp", PORTFILE)
s.serve_forever()
PY
REQ="$WORK/requests"
python3 "$WORK/provider.py" "$REQ" "$WORK/port" & PROVIDER_PID=$!
for _ in $(seq 50); do [ -f "$WORK/port" ] && break; sleep 0.1; done
PORT="$(cat "$WORK/port")" || { echo "FATAL: provider did not start" >&2; exit 2; }
nreq() { find "$REQ" -name 'req-*.json' 2>/dev/null | wc -l; }
last_req() { find "$REQ" -name 'req-*.json' | sort | tail -1; }
req_tools() { jq -c '[.tools[].name] | sort' "$1"; }

# ── a small real git repo with a task in it ────────────────────────────────
REPO="$WORK/repo"; mkdir -p "$REPO/src" "$WORK/home"
g() { git -C "$REPO" -c user.name=gate -c user.email=gate@example "$@"; }
g init -q -b main
printf 'fn main() {\n    println!("hello");\n}\n' > "$REPO/src/main.rs"
g add -A && g commit -q -m "base"
BASE_PARENT="$(g rev-parse HEAD)"
printf '# task\nsay pong\n' > "$REPO/TASK.md"
g add -A && g commit -q -m "task"
HEAD_SHA="$(g rev-parse HEAD)"
REPO_REAL="$(cd "$REPO" && pwd -P)"
CL="$REPO/.micode/axon/closed-loop"

TENANT=tenant-a; FAMILY=coding; MODEL=anthropic/claude-sonnet-5

# expected_context <out> <trial> <arm> <base> <epoch>
expected_context() {
  jq -n --arg trial "$2" --arg arm "$3" --arg base "$4" --argjson epoch "$5" \
        --arg wd "$REPO_REAL" --arg t "$TENANT" --arg f "$FAMILY" --arg m "$MODEL" '{
    schema:"micode.expected-context/1", profile:"paired_trial", context_id:("ctx-"+$trial),
    identity:{task_id:"task-pong", arm_id:$arm, trial_id:$trial, attempt_id:($trial+"-a1"),
              operation_id:($trial+"-op"), execution_id:($trial+"-ex")},
    scope:{tenant_id:$t, task_family:$f}, repo_id:"gate-repo", base_commit:$base, branch:"main",
    worktree_id:null, working_directory:$wd, build_namespace:"shared:default-target", model:$m,
    role:"implementation", read_paths:[], write_paths:["src/**"], is_primary_worktree:true,
    expected_issuer_ref:"loop-interop-gate", authority_epoch:$epoch, ttl_ms:600000,
    corpus_role:"mechanism_test", data_use_ref:("cl22:"+("d"*64)) }' > "$1"
}

# run_micode <label> [ENV=VAL ...]; sets RC, NEWREQ (requests this run), writes $WORK/<label>.{out,err}
run_micode() {
  local label="$1"; shift
  local before; before="$(nreq)"
  ( cd "$REPO" && env -u CARGO_TARGET_DIR -u MICODE_CONFIG_DIR \
      -u MICODE_AXON_EXPECTED_CONTEXT -u MICODE_AXON_ACTIVE_POLICY \
      HOME="$WORK/home" MICODE_PROVIDER=anthropic MICODE_PROVIDER_MODEL=claude-sonnet-5 \
      MICODE_PROVIDER_API_KEY=loop-interop-gate-not-a-real-key \
      MICODE_PROVIDER_BASE_URL="http://127.0.0.1:$PORT" MICODE_EPISODE_LOG=1 \
      "$@" "$MICODE" exec "say pong" ) >"$WORK/$label.out" 2>"$WORK/$label.err"
  RC=$?
  NEWREQ=$(( $(nreq) - before ))
}
# the file a directory gained during the last run
list_dir() { find "$1" -maxdepth 1 -name '*.json' 2>/dev/null | sort; }
new_file() { comm -13 <(printf '%s\n' "$2") <(list_dir "$1") | head -1; }
snap_cl() { SN_CTX="$(list_dir "$CL/context")"; SN_EP="$(list_dir "$CL/episodes")"; SN_ACK="$(list_dir "$CL/policy-ack")"; }

# ── Axon store ─────────────────────────────────────────────────────────────
STORE="$WORK/store"; mkdir -p "$STORE"
cat > "$STORE/config.json" <<'J'
{"schema":"axon.loop.config/1","trusted_admitters":["op:gate-admitter"],"trusted_verifiers":["gate:independent-verifier"]}
J
store_hash() { (cd "$STORE" && find . -type f ! -path './locks/*' -print0 | sort -z | xargs -0 sha256sum) | sha256sum | cut -d' ' -f1; }
ledger_n() { [ -f "$STORE/ledger.jsonl" ] && wc -l < "$STORE/ledger.jsonl" || echo 0; }
axl() { "$AXL" --store "$STORE" "$@"; }

# ════════════════════════════════════════════════════════════════════════════
section "0. old peer: no expected context, a policy file present"
# A stale/foreign policy is set, but the harness declared no context: MiCode
# must behave exactly as before — every tool, no closed-loop artefact.
echo '{"schema":"axon.closed-loop.policy/1"}' > "$WORK/stale-policy.json"
run_micode old MICODE_AXON_ACTIVE_POLICY="$WORK/stale-policy.json"
check "old peer: micode exec succeeds" eq "$RC" 0
check "old peer: exactly one provider request" eq "$NEWREQ" 1
OLD_TOOLS="$(req_tools "$(last_req)")"
check "old peer: no .micode/axon/closed-loop directory" test ! -e "$CL"
check "old peer: Axon ledger has nothing to record" eq "$(ledger_n)" 0

# ════════════════════════════════════════════════════════════════════════════
section "1. incumbent run: learn the candidate view from MiCode's own ack"
expected_context "$WORK/exp-inc.json" trial-inc incumbent "$HEAD_SHA" 1
snap_cl
run_micode inc MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-inc.json"
check "incumbent: micode exec succeeds" eq "$RC" 0
check "incumbent: exactly one provider request" eq "$NEWREQ" 1
INC_TOOLS="$(req_tools "$(last_req)")"
INC_ACK="$(new_file "$CL/policy-ack" "$SN_ACK")"; INC_EP="$(new_file "$CL/episodes" "$SN_EP")"
check "incumbent: ack records not_configured" eq "$(jq -r .pin.state "$INC_ACK")" not_configured
check "incumbent: ack candidates == tools the provider was sent" eq "$(jq -c '.candidates|sort' "$INC_ACK")" "$INC_TOOLS"
check "incumbent: tool set identical to the old-peer run" eq "$INC_TOOLS" "$OLD_TOOLS"
CANDIDATES="$(jq -r '.candidates|join(",")' "$INC_ACK")"
MICODE_CSR="$(jq -r .candidate_set_ref "$INC_ACK")"
H0="$(store_hash)"
axl intake episode --in "$INC_EP" --context "$CL/context" --ack "$CL/policy-ack" >/dev/null 2>"$WORK/inc-intake.err"
check "incumbent: Axon intake refuses a task that ran under no Axon policy (exit 4)" eq "$?" 4
check "incumbent: refusal names the not-produced marker" grep -q "not-produced" "$WORK/inc-intake.err"
check "incumbent: store bytes unchanged" eq "$(store_hash)" "$H0"

# ════════════════════════════════════════════════════════════════════════════
section "2. Axon produces the policy: DEC → put → baseline → activate → resolve"
( cd "$AXON_DIR" && env -u CARGO_TARGET_DIR \
    INTAKE_DEC_OUT="$WORK/dec-policy.json" INTAKE_DEC_CANDIDATES="$CANDIDATES" \
    INTAKE_DEC_TENANT="$TENANT" INTAKE_DEC_FAMILY="$FAMILY" INTAKE_DEC_POLICY_ID=pol-dec-freq-1 \
    INTAKE_DEC_PROVIDER=deterministic-frequency INTAKE_DEC_LIMIT=2 INTAKE_DEC_USAGE="read:5,grep:3" \
    INTAKE_DEC_MODEL="$MODEL" \
    cargo test -q -p axon-loop --test intake_dec_driver -- --ignored --exact produce_policy ) \
    >"$WORK/dec.log" 2>&1
check "DEC driver produced a policy" test -s "$WORK/dec-policy.json"
check "DEC shortlist is [read, grep] (frequency ranking, limit 2)" eq "$(jq -c .shortlist "$WORK/dec-policy.json")" '["read","grep"]'
check "Axon candidate_set_ref == MiCode's (two independent cl22 implementations agree)" \
  eq "$(jq -r .candidate_set_ref "$WORK/dec-policy.json")" "$MICODE_CSR"
PUT="$(axl policy put --in "$WORK/dec-policy.json")"; check "policy put exit 0" eq "$?" 0
POL_REF="$(jq -r .policy_ref <<<"$PUT")"
jq -n --arg p "$POL_REF" --arg t "$TENANT" --arg f "$FAMILY" \
  '{schema:"axon.loop.baseline/1",scope:{tenant_id:$t,task_family:$f},policy_ref:$p,issuer_ref:"op:gate-admitter",reason_ref:("cl22:"+("b"*64))}' > "$WORK/baseline.json"
BASE_REF="$(axl pointer baseline --in "$WORK/baseline.json" | jq -r .baseline_ref)"
jq -n --arg p "$POL_REF" --arg b "$BASE_REF" --arg t "$TENANT" --arg f "$FAMILY" '{
  schema:"axon.closed-loop.transition/1", transition_id:"gate-activate-1", kind:"activate",
  scope:{tenant_id:$t,task_family:$f}, expected_policy_ref:("cl22:"+("0"*64)), target_policy_ref:$p,
  expected_epoch:0, next_epoch:1, admission_ref:$b, reason_ref:("cl22:"+("e"*64)),
  issuer_ref:"op:gate-admitter", mechanism_test:false }' > "$WORK/activate.json"
axl pointer transition --in "$WORK/activate.json" >/dev/null; check "activate transition exit 0" eq "$?" 0
RES="$(axl pointer resolve --tenant "$TENANT" --family "$FAMILY")"; check "pointer resolve exit 0" eq "$?" 0
RES_ID="$(jq -r .pin.version.policy_id <<<"$RES")"; RES_DIGEST="$(jq -r .pin.version.digest <<<"$RES")"
RES_EPOCH="$(jq -r .pin.epoch <<<"$RES")"
check "resolve pins the DEC policy (digest == put ref)" eq "$RES_DIGEST" "$POL_REF"
jq -c .policy <<<"$RES" > "$WORK/active-policy.json"   # the bytes MiCode will read

# ════════════════════════════════════════════════════════════════════════════
section "3. MiCode consumes Axon's policy (real binary)"
expected_context "$WORK/exp-pin.json" trial-pin challenger-1 "$HEAD_SHA" "$RES_EPOCH"
snap_cl
run_micode pin MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-pin.json" MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json"
check "pinned: micode exec succeeds" eq "$RC" 0
check "pinned: exactly one provider request" eq "$NEWREQ" 1
check "pinned: provider was sent EXACTLY the shortlisted tools" eq "$(req_tools "$(last_req)")" '["grep","read"]'
PIN_ACK="$(new_file "$CL/policy-ack" "$SN_ACK")"; PIN_EP="$(new_file "$CL/episodes" "$SN_EP")"
PIN_CTX="$(new_file "$CL/context" "$SN_CTX")"
check "pinned: ack state pinned" eq "$(jq -r .pin.state "$PIN_ACK")" pinned
check "pinned: ack policy_id == Axon's" eq "$(jq -r .pin.policy_id "$PIN_ACK")" "$RES_ID"
check "pinned: ack policy_ref == Axon's cl22 digest" eq "$(jq -r .pin.policy_ref "$PIN_ACK")" "$RES_DIGEST"
check "pinned: sidecar policy_ref == Axon's digest" eq "$(jq -r .policy_ref "$PIN_EP")" "$RES_DIGEST"
check "pinned: sidecar epoch == Axon pointer epoch" eq "$(jq -r .authority_epoch "$PIN_EP")" "$RES_EPOCH"
# Unit conversion on real bytes: 10 in + 3 out tokens at $3/$15 per MTok
# = 10*300 + 3*1500 = 7500 MicroCents (1e-8) → 75 cost_micro (1e-6), rounded up.
check "pinned: sidecar cost_micro is 7500 MicroCents converted (75)" eq "$(jq -r .usage.cost_micro "$PIN_EP")" 75
check "pinned: usage state estimated (not final)" eq "$(jq -r .usage.state "$PIN_EP")" estimated

# ════════════════════════════════════════════════════════════════════════════
section "4. Axon intakes MiCode's real episode + context receipt"
N0="$(ledger_n)"
INTAKE="$(axl intake episode --in "$PIN_EP" --context "$CL/context" --ack "$CL/policy-ack" 2>"$WORK/intake.err")"
check "intake exit 0" eq "$?" 0
check "ledger grew by exactly one entry" eq "$(ledger_n)" "$((N0+1))"
check "ledger event is episode_intake" eq "$(tail -1 "$STORE/ledger.jsonl" | jq -r .event.event)" episode_intake
check "recorded corpus_role mechanism_test" eq "$(jq -r .record.corpus_role <<<"$INTAKE")" mechanism_test
check "recorded policy_ref == Axon's" eq "$(jq -r .record.policy_ref <<<"$INTAKE")" "$RES_DIGEST"
check "recorded cost_micro 75" eq "$(jq -r .record.cost_micro <<<"$INTAKE")" 75
check "recorded context_ref == receipt file name" eq "$(jq -r .record.context_ref <<<"$INTAKE")" "cl22:$(basename "$PIN_CTX" .json)"
check "recorded trial identity" eq "$(jq -c '.record.identity|[.arm_id,.trial_id]' <<<"$INTAKE")" '["challenger-1","trial-pin"]'
check "episode bytes stored content-addressed" test -f "$STORE/episodes/$(jq -r .episode_ref <<<"$INTAKE" | cut -d: -f2).json"
check "paired-trial structural refusal recorded (primary checkout), not hidden" \
  grep -q primary <<<"$(jq -r .record.trial_profile_refusal <<<"$INTAKE")"
H1="$(store_hash)"
axl intake episode --in "$PIN_EP" --context "$CL/context" --ack "$CL/policy-ack" > "$WORK/intake2.json"
check "re-intake is idempotent (recorded_now false)" eq "$(jq -r .recorded_now "$WORK/intake2.json")" false
check "re-intake leaves store bytes unchanged" eq "$(store_hash)" "$H1"
# The canonical MiCode episode the sidecar references: digest joins, but its
# cost is MiCode's default-0 ResourceCost, not the metered spend (see gaps).
SRC_EP="$(find "$REPO/.micode/axon/episodes" -name '*.json' | while read -r f; do
  [ "cl22:$(python3 -c 'import json,sys,hashlib;v=json.load(open(sys.argv[1]));print(hashlib.sha256(json.dumps(v,sort_keys=True,separators=(",",":"),ensure_ascii=False).encode()).hexdigest())' "$f")" = "$(jq -r .source_episode_ref "$PIN_EP")" ] && echo "$f"; done | head -1)"
check "source_episode_ref resolves to a canonical MiCode episode on disk" test -n "$SRC_EP"
axl intake episode --in "$PIN_EP" --context "$CL/context" --ack "$CL/policy-ack" --source-episode "$SRC_EP" >/dev/null 2>"$WORK/src.err"
SRC_RC=$?
if [ "$(jq -r .cost.micro_cents "$SRC_EP")" = 0 ]; then
  check "KNOWN GAP G1: canonical episode cost.micro_cents=0 vs sidecar 75 → intake refuses the join (exit 4)" eq "$SRC_RC" 4
  check "KNOWN GAP G1: refusal is the unit-conversion check, not a digest mismatch" grep -q "unit conversion" "$WORK/src.err"
else
  check "canonical episode cost joins the sidecar under the round-up rule" eq "$SRC_RC" 0
fi
check "store unchanged by the source-episode cross-check" eq "$(store_hash)" "$H1"

# ════════════════════════════════════════════════════════════════════════════
section "5. negative: bad policies → MiCode abstains to the incumbent, zero tool additions"
make_bad() {  # <name> <jq filter over the good policy>  (or raw bytes via sed for dup-key)
  jq -c "$2" "$WORK/active-policy.json" > "$WORK/bad-$1.json"
}
make_bad added-tool '.shortlist += ["teleport"]'
make_bad authority-expansion '.authority_expansion = true'
make_bad unknown-field '. + {"extra":"x"}'
sed 's/"policy_id":"pol-dec-freq-1"/"policy_id":"pol-dec-freq-1","policy_id":"pol-evil"/' \
  "$WORK/active-policy.json" > "$WORK/bad-dup-key.json"
check "dup-key fixture really has a duplicate key" eq "$(grep -o '"policy_id"' "$WORK/bad-dup-key.json" | wc -l)" 2
i=0
for name in added-tool authority-expansion unknown-field dup-key; do
  i=$((i+1))
  H="$(store_hash)"
  axl policy put --in "$WORK/bad-$name.json" >/dev/null 2>&1
  PRC=$?
  if [ "$name" = added-tool ]; then
    # KNOWN GAP G2: the store holds only candidate_set_ref (a digest); it has no
    # candidate LIST to check shortlist ⊆ candidates against, so `policy put`
    # stores this. MiCode is the enforcement point (below). Asserted as-is so
    # the gate turns red the day put starts checking and this label must change.
    check "$name: KNOWN GAP G2: Axon policy put ACCEPTS a tool-adding shortlist (exit 0)" eq "$PRC" 0
  else
    check "$name: Axon policy put refuses it (exit 3/4)" bash -c "[ $PRC -eq 3 ] || [ $PRC -eq 4 ]"
    check "$name: store bytes unchanged" eq "$(store_hash)" "$H"
  fi
  expected_context "$WORK/exp-bad-$name.json" "trial-bad-$i" challenger-1 "$HEAD_SHA" "$RES_EPOCH"
  snap_cl
  run_micode "bad-$name" MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-bad-$name.json" MICODE_AXON_ACTIVE_POLICY="$WORK/bad-$name.json"
  check "$name: micode task still runs (abstention never fails a task)" eq "$RC" 0
  check "$name: exactly one provider request" eq "$NEWREQ" 1
  check "$name: provider sent the incumbent tool set, zero additions" eq "$(req_tools "$(last_req)")" "$INC_TOOLS"
  check "$name: 'teleport' never reached the provider" bash -c "! jq -e '.tools[]|select(.name==\"teleport\")' '$(last_req)' >/dev/null"
  A="$(new_file "$CL/policy-ack" "$SN_ACK")"; E="$(new_file "$CL/episodes" "$SN_EP")"
  check "$name: ack records abstained" eq "$(jq -r .pin.state "$A")" abstained
  check "$name: abstention reason recorded" test -n "$(jq -r '.pin.reason // empty' "$A")"
  N="$(ledger_n)"; H="$(store_hash)"
  axl intake episode --in "$E" --context "$CL/context" --ack "$CL/policy-ack" >/dev/null 2>&1
  check "$name: Axon intake refuses the fallback episode (exit 4)" eq "$?" 4
  check "$name: nothing recorded, store unchanged" bash -c "[ $(ledger_n) -eq $N ] && [ '$(store_hash)' = '$H' ]"
  echo "        reason: $(jq -r .pin.reason "$A" | cut -c1-110)"
done

# ════════════════════════════════════════════════════════════════════════════
section "6. negative: context mismatch → TASK_NOT_STARTED, zero provider requests"
expected_context "$WORK/exp-mismatch.json" trial-mm challenger-1 "$BASE_PARENT" "$RES_EPOCH"
snap_cl
run_micode mismatch MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-mismatch.json" MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json"
check "mismatch: micode exec fails" bash -c "[ $RC -ne 0 ]"
check "mismatch: stderr says TASK_NOT_STARTED: EXECUTION_CONTEXT_MISMATCH" grep -q "TASK_NOT_STARTED: EXECUTION_CONTEXT_MISMATCH" "$WORK/mismatch.err"
check "mismatch: ZERO provider requests" eq "$NEWREQ" 0
MM_EP="$(new_file "$CL/episodes" "$SN_EP")"
check "mismatch: refused sidecar written (denominator)" eq "$(jq -r .status "$MM_EP")" refused
N="$(ledger_n)"; H="$(store_hash)"
axl intake episode --in "$MM_EP" --context "$CL/context" --ack "$CL/policy-ack" >/dev/null 2>"$WORK/mm-intake.err"
check "mismatch: Axon intake refuses (exit 4)" eq "$?" 4
check "mismatch: refusal says TASK_NOT_STARTED" grep -q TASK_NOT_STARTED "$WORK/mm-intake.err"
check "mismatch: nothing recorded, store unchanged" bash -c "[ $(ledger_n) -eq $N ] && [ '$(store_hash)' = '$H' ]"

# ════════════════════════════════════════════════════════════════════════════
section "7. negative: tampered MiCode episodes → Axon intake refuses, store unchanged"
tamper() { jq -c "$2" "$PIN_EP" > "$WORK/tamper-$1.json"; }
tamper policy-ref '.policy_ref = ("cl22:"+("e"*64))'
tamper unknown-field '. + {"surprise":1}'
tamper context-ref '.context_ref = ("cl22:"+("f"*64))'
tamper cost-unconverted '.usage.cost_micro = 7500'   # MicroCents passed through as µUSD
tamper identity '.identity.trial_id = "trial-other"'
sed 's/"corpus_role":"mechanism_test"/"corpus_role":"discovery","corpus_role":"mechanism_test"/' "$PIN_EP" > "$WORK/tamper-dup-key.json"
for spec in "policy-ref:4" "unknown-field:3" "context-ref:4" "dup-key:3" "identity:4"; do
  name="${spec%%:*}"; want="${spec##*:}"
  N="$(ledger_n)"; H="$(store_hash)"
  axl intake episode --in "$WORK/tamper-$name.json" --context "$PIN_CTX" --ack "$PIN_ACK" >/dev/null 2>"$WORK/tamper-$name.err"
  check "tampered $name: refused with exit $want" eq "$?" "$want"
  check "tampered $name: nothing recorded, store unchanged" bash -c "[ $(ledger_n) -eq $N ] && [ '$(store_hash)' = '$H' ]"
done
# cost-unconverted changes the sidecar bytes, so projection/ack still bind; it is
# refused only against the canonical episode — which is 0 today (gap G1). Record it.
N="$(ledger_n)"
axl intake episode --in "$WORK/tamper-cost-unconverted.json" --context "$PIN_CTX" --ack "$PIN_ACK" >/dev/null 2>"$WORK/tamper-cost.err"
TC_RC=$?
check "tampered cost (same trial, different bytes): refused as identity conflict (exit 5)" eq "$TC_RC" 5
check "tampered cost: nothing recorded" eq "$(ledger_n)" "$N"

# ════════════════════════════════════════════════════════════════════════════
section "summary"
TOTAL=$((PASS+FAIL))
echo "assertions: $TOTAL  pass: $PASS  fail: $FAIL"
[ "$TOTAL" -gt 0 ] && [ "$FAIL" -eq 0 ]
