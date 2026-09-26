#!/usr/bin/env bash
# loop_interop_gate.sh — Axon v0.22 closed loop, PAIRED interop gate.
#
# Real bytes, both directions, no fixture parser on either side:
#   Axon DEC (axon-reflex decide_builtin → to_policy_envelope)
#     → axon-loop candidates put / policy put / pointer baseline / pointer transition / pointer resolve
#     → the REAL `micode exec` binary consumes the resolved policy (fixture LLM
#       provider on 127.0.0.1 that records every request body; no credentials)
#     → MiCode writes context receipt + policy-ack + episode sidecar
#     → `axon-loop intake episode` parses and joins those files and records them in the ledger.
# Negative cases assert ABSENCE of effect: zero tool additions, zero provider
# requests, store bytes unchanged, nothing recorded.
#
# Env: MICODE_DIR (default ../micode-v022-wt), KEEP=1 keeps the temp dir,
#      CARGO_TARGET_DIR (Axon side; default <axon>/target),
#      MICODE_TARGET_DIR (MiCode side; default $CARGO_TARGET_DIR/micode when
#      CARGO_TARGET_DIR is set, else <micode>/target).
#
# Exit / output contract (the harness convention, scripts/lib/harness_skip.sh):
#   0 + final line "loop_interop_gate: PASS — N assertions"  every assertion held
#   0 + final line "loop_interop_gate: SKIP — …"             the DEFAULT MiCode
#       worktree is absent: a NON-RESULT, not a pass. Under
#       AXON_HARNESS_STRICT=1 this is exit 3 instead.
#   1 + final line "loop_interop_gate: FAIL — …"             an assertion failed
#       or none ran
#   2  prerequisite broken in a way that is NOT absence: an explicitly named
#      MICODE_DIR that does not exist, a build failure, the provider not starting.
# All builds pass --locked: this is a release-significant harness.
set -uo pipefail

AXON_DIR="$(cd "$(dirname "$0")/.." && pwd)"
# shellcheck source=lib/harness_skip.sh
. "$AXON_DIR/scripts/lib/harness_skip.sh"
if [ -n "${MICODE_DIR:-}" ]; then
  # Named explicitly: absence is a misconfiguration, never a skip.
  [ -d "$MICODE_DIR" ] || { echo "FATAL: MICODE_DIR=$MICODE_DIR does not exist" >&2; exit 2; }
  MICODE_DIR="$(cd "$MICODE_DIR" && pwd)"
else
  MICODE_DIR="$(cd "$AXON_DIR/../micode-v022-wt" 2>/dev/null && pwd)"
  if [ -z "$MICODE_DIR" ]; then
    if [ "${AXON_HARNESS_STRICT:-}" = 1 ]; then
      echo "loop_interop_gate: FAIL — SKIP under AXON_HARNESS_STRICT=1: no MiCode worktree at $AXON_DIR/../micode-v022-wt (set MICODE_DIR)"
      exit 3
    fi
    harness_skip loop_interop_gate \
      "no MiCode worktree at $AXON_DIR/../micode-v022-wt and MICODE_DIR unset" \
      "this is a NON-RESULT: zero interop assertions ran" \
      "MiCode peer absent (set MICODE_DIR to a MiCode v0.22 checkout)"
  fi
fi
AXON_TGT="${CARGO_TARGET_DIR:-$AXON_DIR/target}"
if [ -n "${MICODE_TARGET_DIR:-}" ]; then MICODE_TGT="$MICODE_TARGET_DIR"
elif [ -n "${CARGO_TARGET_DIR:-}" ]; then MICODE_TGT="$CARGO_TARGET_DIR/micode"
else MICODE_TGT="$MICODE_DIR/target"; fi

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
( cd "$AXON_DIR" && CARGO_TARGET_DIR="$AXON_TGT" cargo build --locked -q -p axon-loop --bins ) \
  || { echo "FATAL: axon-loop build failed" >&2; exit 2; }
( cd "$MICODE_DIR" && CARGO_TARGET_DIR="$MICODE_TGT" cargo build --locked -q -p micode --bin micode ) \
  || { echo "FATAL: micode build failed (--locked: is MiCode's Cargo.lock tracked and current?)" >&2; exit 2; }
# Section 8 (G3) drives the REAL Fabric submit path, which runs the check in
# the real interpreter: both are built here, never mocked.
( cd "$AXON_DIR" && CARGO_TARGET_DIR="$AXON_TGT" cargo build --locked -q -p axon-fabric --bin axon-fabric \
    && CARGO_TARGET_DIR="$AXON_TGT" cargo build --locked -q -p axon-core --no-default-features --bin axon ) \
  || { echo "FATAL: axon-fabric / axon interpreter build failed" >&2; exit 2; }
( cd "$AXON_DIR" && CARGO_TARGET_DIR="$AXON_TGT" cargo build --locked -q -p cortex-policy-adapter ) \
  || { echo "FATAL: cortex-policy-adapter build failed" >&2; exit 2; }
AXF="$AXON_TGT/debug/axon-fabric"; AXI="$AXON_TGT/debug/axon"; CPA="$AXON_TGT/debug/cortex-policy-adapter"
[ -x "$CPA" ] || { echo "FATAL: cortex-policy-adapter not built" >&2; exit 2; }
[ -x "$AXF" ] && [ -x "$AXI" ] || { echo "FATAL: axon-fabric or axon not built" >&2; exit 2; }
AXL="$AXON_TGT/debug/axon-loop"
MICODE="$MICODE_TGT/debug/micode"
[ -x "$AXL" ] || { echo "FATAL: built axon-loop not at $AXL" >&2; exit 2; }
[ -x "$MICODE" ] || { echo "FATAL: built micode not at $MICODE" >&2; exit 2; }
echo "micode dir $MICODE_DIR"
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
        # A one-shot scripted turn (section 10: a tool call): served once, then pong again.
        sse, nxt = SSE, os.path.join(os.path.dirname(OUT), "next.sse")
        with lock:
            if os.path.exists(nxt):
                sse = open(nxt, "rb").read(); os.remove(nxt)
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(sse)))
        self.send_header("Connection", "close")
        self.end_headers(); self.wfile.write(sse)
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
# The CANDIDATE's code: what the task produced.
cat > "$REPO/calc.ax" <<'AX'
fn double(n: i64) -> i64 { n * 2 }
AX
# A check file IN the candidate's own tree. The candidate controls these bytes,
# so they can never define the acceptance rubric (G01-r22-verifier-separation):
# section 8 shows a genuinely signed verdict from this file is refused. The
# real acceptance suite is the operator's (below, outside the repository).
mkdir -p "$REPO/checks"
cat > "$REPO/checks/accept.ax" <<'AX'
fn double(n: i64) -> i64 { n * 2 }

@[test]
fn t_ok_double() { assert_eq(double(2), 4) }

@[test]
fn t_ok_zero() { assert_eq(double(0), 0) }

@[test]
fn t_bad() { assert_eq(double(2), 5) }
AX
g add -A && g commit -q -m "task"
HEAD_SHA="$(g rev-parse HEAD)"
# The repository identity MiCode OBSERVES (G16-r22-preflight-start): its root commit.
REPO_ID="git-root:$(g rev-list --max-parents=0 HEAD | sort | head -1)"
REPO_REAL="$(cd "$REPO" && pwd -P)"
CL="$REPO/.micode/axon/closed-loop"

TENANT=tenant-a; FAMILY=coding; MODEL=anthropic/claude-sonnet-5

# expected_context <out> <trial> <arm> <base> <epoch>
expected_context() {
  jq -n --arg trial "$2" --arg arm "$3" --arg base "$4" --argjson epoch "$5" --arg repo "$REPO_ID" \
        --arg wd "$REPO_REAL" --arg t "$TENANT" --arg f "$FAMILY" --arg m "$MODEL" '{
    schema:"micode.expected-context/1", profile:"paired_trial", context_id:("ctx-"+$trial),
    identity:{task_id:"task-pong", arm_id:$arm, trial_id:$trial, attempt_id:($trial+"-a1"),
              operation_id:($trial+"-op"), execution_id:($trial+"-ex")},
    scope:{tenant_id:$t, task_family:$f}, repo_id:$repo, base_commit:$base, branch:"main",
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
      -u MICODE_AXON_EXPECTED_CONTEXT -u MICODE_AXON_ACTIVE_POLICY -u MICODE_AXON_POLICY_AUTHORITY \
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
# G01-r22-independent-issuer: the verifier's Ed25519 key, provisioned by the real
# `axon-fabric keygen`; the operator registers its PUBLIC key for the verifier id,
# so the verifier's evidence is authenticated, not merely named.
ISSUER_KEY="$WORK/issuer.pk8"
ISSUER_PK="$("$AXF" keygen --out "$ISSUER_KEY" | jq -r .public_key)"
jq -c --arg pk "$ISSUER_PK" '.verifier_keys = {"gate:independent-verifier": $pk}' "$STORE/config.json" > "$STORE/config.json.tmp" \
  && mv "$STORE/config.json.tmp" "$STORE/config.json"
store_hash() { (cd "$STORE" && find . -type f ! -path './locks/*' -print0 | sort -z | xargs -0 sha256sum) | sha256sum | cut -d' ' -f1; }
ledger_n() { [ -f "$STORE/ledger.jsonl" ] && wc -l < "$STORE/ledger.jsonl" || echo 0; }
axl() { "$AXL" --store "$STORE" "$@"; }

# ════════════════════════════════════════════════════════════════════════════
section "0. old peer: no expected context, no policy"
# A peer that declares nothing gets MiCode exactly as before: every tool, no
# closed-loop artefact. This is the baseline tool set section 1 compares to.
run_micode old
check "old peer: micode exec succeeds" eq "$RC" 0
check "old peer: exactly one provider request" eq "$NEWREQ" 1
OLD_TOOLS="$(req_tools "$(last_req)")"
check "old peer: no .micode/axon/closed-loop directory" test ! -e "$CL"
check "old peer: Axon ledger has nothing to record" eq "$(ledger_n)" 0

section "0b. a policy set WITHOUT an expected context is REFUSED, not ignored"
# Before v022 Stage 4 (MiCode B268, 03f6dea4) MiCode silently IGNORED an active
# policy that came with no expected context, and this section asserted that
# silent ignore as correct. A configured key that does nothing is the failure
# AGENTS.md §1 forbids, and ACTIVE_POLICY_ENV's own doc says it requires the
# context. Now: TASK_NOT_STARTED, zero provider calls, nothing written.
echo '{"schema":"axon.closed-loop.policy/1"}' > "$WORK/stale-policy.json"
run_micode stale MICODE_AXON_ACTIVE_POLICY="$WORK/stale-policy.json"
check "policy without context: micode exec is refused (nonzero)" test "$RC" -ne 0
check "policy without context: zero provider requests" eq "$NEWREQ" 0
check "policy without context: names POLICY_WITHOUT_EXPECTED_CONTEXT" \
  grep -q POLICY_WITHOUT_EXPECTED_CONTEXT "$WORK/stale.out" "$WORK/stale.err"
check "policy without context: no .micode/axon/closed-loop directory" test ! -e "$CL"
check "policy without context: Axon ledger has nothing to record" eq "$(ledger_n)" 0

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
( cd "$AXON_DIR" && CARGO_TARGET_DIR="$AXON_TGT" \
    INTAKE_DEC_OUT="$WORK/dec-policy.json" INTAKE_DEC_CANDIDATES="$CANDIDATES" \
    INTAKE_DEC_TENANT="$TENANT" INTAKE_DEC_FAMILY="$FAMILY" INTAKE_DEC_POLICY_ID=pol-dec-freq-1 \
    INTAKE_DEC_PROVIDER=deterministic-frequency INTAKE_DEC_LIMIT=2 INTAKE_DEC_USAGE="read:5,grep:3" \
    INTAKE_DEC_MODEL="$MODEL" \
    cargo test --locked -q -p axon-loop --test intake_dec_driver -- --ignored --exact produce_policy ) \
    >"$WORK/dec.log" 2>&1
check "DEC driver produced a policy" test -s "$WORK/dec-policy.json"
check "DEC shortlist is [read, grep] (frequency ranking, limit 2)" eq "$(jq -c .shortlist "$WORK/dec-policy.json")" '["read","grep"]'
check "Axon candidate_set_ref == MiCode's (two independent cl22 implementations agree)" \
  eq "$(jq -r .candidate_set_ref "$WORK/dec-policy.json")" "$MICODE_CSR"
# G2: register MiCode's authorized candidate LIST (from its own ack) so Axon can
# check shortlist ⊆ candidates itself. Its cl22 must be the policy's view.
jq -n --argjson c "$(jq -c '.candidates|sort' "$INC_ACK")" --arg t "$TENANT" --arg f "$FAMILY" \
  '{schema:"axon.loop.candidate-set/1",scope:{tenant_id:$t,task_family:$f},candidates:$c,issuer_ref:"op:gate-admitter"}' \
  > "$WORK/candidates.json"
H_PRE_PUT="$(store_hash)"
axl policy put --in "$WORK/dec-policy.json" >/dev/null 2>"$WORK/put-unreg.err"
check "G2: policy put REFUSES a view with no registered candidate list (exit 4)" eq "$?" 4
check "G2: refusal names the unregistered candidate_set_ref" grep -q "not a registered candidate list" "$WORK/put-unreg.err"
check "G2: store unchanged by the refused put" eq "$(store_hash)" "$H_PRE_PUT"
CSR="$(axl candidates put --in "$WORK/candidates.json" | jq -r .candidate_set_ref)"
check "candidates put: registered list digests to MiCode's candidate_set_ref" eq "$CSR" "$MICODE_CSR"
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
check "G6: sidecar projection_ref is null (the ack is not a PolicyProjection)" eq "$(jq -r .projection_ref "$PIN_EP")" null
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
# The canonical MiCode episode the sidecar references. G1 is fixed in MiCode:
# the episode states the task's metered spend (cost.spend_micro_cents, beside
# the legacy micro_cents default), so the digest AND the cost join under the
# 1e-8 → 1e-6 round-up rule.
SRC_EP="$(find "$REPO/.micode/axon/episodes" -name '*.json' | while read -r f; do
  [ "cl22:$(python3 -c 'import json,sys,hashlib;v=json.load(open(sys.argv[1]));print(hashlib.sha256(json.dumps(v,sort_keys=True,separators=(",",":"),ensure_ascii=False).encode()).hexdigest())' "$f")" = "$(jq -r .source_episode_ref "$PIN_EP")" ] && echo "$f"; done | head -1)"
check "source_episode_ref resolves to a canonical MiCode episode on disk" test -n "$SRC_EP"
check "G1 fixed: canonical episode cost.spend_micro_cents is the metered spend (7500), not 0" eq "$(jq -r .cost.spend_micro_cents "$SRC_EP")" 7500
axl intake episode --in "$PIN_EP" --context "$CL/context" --ack "$CL/policy-ack" --source-episode "$SRC_EP" >/dev/null 2>"$WORK/src.err"
check "G1 fixed: --source-episode join succeeds (digest + round-up cost conversion)" eq "$?" 0
# A canonical episode whose cost disagrees is still refused, never repaired:
# a stated zero, and (G16-r22-negotiation) a v014-shaped episode — its default
# micro_cents 0 with no spend field is UNKNOWN through the pinned migration
# table, never a free task.
#
# These run on a FRESH task's sidecar, changed ONLY in source_episode_ref, so
# intake reaches the cost join. (They used to re-point the already-recorded
# PIN_EP under a new trial_id — which the identity bind refused first, so the
# "refused" assertion passed without the cost join ever running. The reason
# checks below are what caught that.)
cl22_of() { python3 -c 'import json,sys,hashlib;v=json.load(open(sys.argv[1]));print("cl22:"+hashlib.sha256(json.dumps(v,sort_keys=True,separators=(",",":"),ensure_ascii=False).encode()).hexdigest())' "$1"; }
expected_context "$WORK/exp-src.json" trial-src challenger-1 "$HEAD_SHA" "$RES_EPOCH"
snap_cl
run_micode src MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-src.json" MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json"
check "source-join task: micode exec succeeds" eq "$RC" 0
SRC_SIDE="$(new_file "$CL/episodes" "$SN_EP")"
SRC_CANON="$(find "$REPO/.micode/axon/episodes" -name '*.json' | while read -r f; do
  [ "$(cl22_of "$f")" = "$(jq -r .source_episode_ref "$SRC_SIDE")" ] && echo "$f"; done | head -1)"
check "source-join task: its canonical episode is on disk" test -n "$SRC_CANON"
H2="$(store_hash)"
for shape in $'zero\t.cost.spend_micro_cents = 0\tZERO spend' \
             $'v014\tdel(.cost.spend_micro_cents) | .cost.micro_cents = 0\tNO known spend'; do
  IFS=$'\t' read -r tag filt want <<<"$shape"
  jq -c "$filt" "$SRC_CANON" > "$WORK/src-$tag.json"
  jq -c --arg r "$(cl22_of "$WORK/src-$tag.json")" '.source_episode_ref = $r' "$SRC_SIDE" > "$WORK/ep-src-$tag.json"
  axl intake episode --in "$WORK/ep-src-$tag.json" --context "$CL/context" --ack "$CL/policy-ack" --source-episode "$WORK/src-$tag.json" >/dev/null 2>"$WORK/src-$tag.err"
  check "$tag canonical cost vs a metered sidecar is refused (exit 4)" eq "$?" 4
  check "$tag: refused BY THE COST JOIN ($want), not by anything earlier" grep -q "$want" "$WORK/src-$tag.err"
done
check "store unchanged by the source-episode cross-checks" eq "$(store_hash)" "$H2"

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
# G16-r22-closed-wire: an ESCAPED alias of a key (`controls\u005fref` decodes to
# `controls_ref`) is the same key twice once decoded, and a malformed digest is
# not a digest. Both real peers must refuse them, not read the first or the
# last. A different key from dup-key's, so the refusal (and MiCode's
# content-addressed ack) is distinct, and naming `controls_ref` proves the peer
# DECODED the escape — the raw text never repeats that key literally.
CTRL="$(jq -r .controls_ref "$WORK/active-policy.json")"
sed "s/\"controls_ref\":/\"controls\\\\u005fref\":\"$CTRL\",\"controls_ref\":/" \
  "$WORK/active-policy.json" > "$WORK/bad-escaped-alias.json"
check "escaped-alias fixture carries the escaped key once and the plain key once" \
  bash -c "[ \$(grep -oF 'controls\\u005fref' '$WORK/bad-escaped-alias.json' | wc -l) -eq 1 ] && [ \$(grep -oF '\"controls_ref\"' '$WORK/bad-escaped-alias.json' | wc -l) -eq 1 ]"
make_bad malformed-digest '.controls_ref = "cl22:not-a-digest"'
i=0
for name in added-tool authority-expansion unknown-field dup-key escaped-alias malformed-digest; do
  i=$((i+1))
  H="$(store_hash)"
  axl policy put --in "$WORK/bad-$name.json" >/dev/null 2>"$WORK/put-bad-$name.err"
  PRC=$?
  if [ "$name" = added-tool ]; then
    # G2 CLOSED: Axon checks shortlist ⊆ the registered candidate list itself;
    # MiCode is no longer the sole enforcement point.
    check "$name: G2: Axon policy put REFUSES a tool-adding shortlist (exit 4)" eq "$PRC" 4
    check "$name: G2: refusal names the added tool" grep -q teleport "$WORK/put-bad-$name.err"
  else
    check "$name: Axon policy put refuses it (exit 3/4)" bash -c "[ $PRC -eq 3 ] || [ $PRC -eq 4 ]"
  fi
  check "$name: store bytes unchanged" eq "$(store_hash)" "$H"
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
  if [ "$name" = escaped-alias ]; then
    check "escaped-alias: MiCode decoded the escape (duplicate controls_ref)" \
      bash -c "jq -r .pin.reason '$A' | grep -q 'duplicate.*controls_ref'"
    check "escaped-alias: Axon names the decoded duplicate too" grep -q 'controls_ref' "$WORK/put-bad-$name.err"
  fi
done

# G16-r22-closed-wire, unsafe numbers, where each peer READS numbers: MiCode's
# expected context (authority_epoch) and Axon's pointer transition
# (expected_epoch). 2^53+1 is not exactly representable in a double, so a reader
# that coerces would act on a different epoch than the one written.
expected_context "$WORK/exp-unsafe.json" trial-unsafe challenger-1 "$HEAD_SHA" "$RES_EPOCH"
sed 's/"authority_epoch":[0-9]*/"authority_epoch":9007199254740993/' "$WORK/exp-unsafe.json" > "$WORK/exp-unsafe2.json"
check "unsafe-number fixture carries 2^53+1" grep -q '9007199254740993' "$WORK/exp-unsafe2.json"
run_micode unsafe MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-unsafe2.json"
check "unsafe number: MiCode refuses the context before any provider request" \
  bash -c "[ $RC -ne 0 ] && [ $NEWREQ -eq 0 ]"
H="$(store_hash)"
jq -c '.transition_id="gate-unsafe" | .expected_epoch=1 | .next_epoch=2' "$WORK/activate.json" \
  | sed 's/"expected_epoch":1/"expected_epoch":9007199254740993/' > "$WORK/unsafe-transition.json"
check "unsafe-number transition fixture carries 2^53+1" grep -q '9007199254740993' "$WORK/unsafe-transition.json"
axl pointer transition --in "$WORK/unsafe-transition.json" >/dev/null 2>"$WORK/unsafe-transition.err"
check "unsafe number: Axon pointer transition refuses it as malformed (exit 3)" eq "$?" 3
check "unsafe number: store unchanged" eq "$(store_hash)" "$H"

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
# G16-r22-preflight-start, "a mismatched repository": everything else right (the
# exact base included), but the declaration names another repository — an opaque
# label, then another history's root. MiCode observes its own repo_id, so both
# refuse before the first model turn and name the field.
for bad in gate-repo "git-root:$(printf '1%.0s' $(seq 40))"; do
  expected_context "$WORK/exp-repo.json" trial-repo challenger-1 "$HEAD_SHA" "$RES_EPOCH"
  jq --arg r "$bad" '.repo_id=$r' "$WORK/exp-repo.json" > "$WORK/exp-repo2.json"
  run_micode repo-mm MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-repo2.json" MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json"
  check "repo mismatch ($bad): TASK_NOT_STARTED naming repo_id" \
    bash -c "[ $RC -ne 0 ] && grep -q 'TASK_NOT_STARTED: EXECUTION_CONTEXT_MISMATCH' '$WORK/repo-mm.err' && grep -q 'repo_id' '$WORK/repo-mm.err'"
  check "repo mismatch ($bad): ZERO provider requests" eq "$NEWREQ" 0
done

# ════════════════════════════════════════════════════════════════════════════
section "7. negative: tampered MiCode episodes → Axon intake refuses, store unchanged"
tamper() { jq -c "$2" "$PIN_EP" > "$WORK/tamper-$1.json"; }
tamper policy-ref '.policy_ref = ("cl22:"+("e"*64))'
tamper unknown-field '. + {"surprise":1}'
tamper context-ref '.context_ref = ("cl22:"+("f"*64))'
tamper cost-unconverted '.usage.cost_micro = 7500'   # MicroCents passed through as µUSD
tamper identity '.identity.trial_id = "trial-other"'
sed 's/"corpus_role":"mechanism_test"/"corpus_role":"discovery","corpus_role":"mechanism_test"/' "$PIN_EP" > "$WORK/tamper-dup-key.json"
tamper projection-ref '.projection_ref = ("cl22:"+("9"*64))'   # G6: must be a real PolicyProjection
for spec in "policy-ref:4" "unknown-field:3" "context-ref:4" "dup-key:3" "identity:4" "projection-ref:4"; do
  name="${spec%%:*}"; want="${spec##*:}"
  N="$(ledger_n)"; H="$(store_hash)"
  axl intake episode --in "$WORK/tamper-$name.json" --context "$PIN_CTX" --ack "$PIN_ACK" >/dev/null 2>"$WORK/tamper-$name.err"
  check "tampered $name: refused with exit $want" eq "$?" "$want"
  check "tampered $name: nothing recorded, store unchanged" bash -c "[ $(ledger_n) -eq $N ] && [ '$(store_hash)' = '$H' ]"
done
# cost-unconverted changes the sidecar bytes but keeps the trial identity, so it
# is refused as an identity conflict with the already-recorded episode.
N="$(ledger_n)"
axl intake episode --in "$WORK/tamper-cost-unconverted.json" --context "$PIN_CTX" --ack "$PIN_ACK" >/dev/null 2>"$WORK/tamper-cost.err"
TC_RC=$?
check "tampered cost (same trial, different bytes): refused as identity conflict (exit 5)" eq "$TC_RC" 5
check "tampered cost: nothing recorded" eq "$(ledger_n)" "$N"

# ════════════════════════════════════════════════════════════════════════════
section "8. G3: MiCode's acceptance check runs through REAL Fabric; Axon intake joins it"
G3_PASS0=$PASS; G3_FAIL0=$FAIL
# D12: only the acceptance check goes through Fabric; the agent's own tool calls
# stay under local MiCode authority, so the sidecar's execution refs stay
# not-produced markers. What must join, on real bytes from three real binaries:
#   MiCode → axon-fabric workspace-import + submit → supervisor-observed receipt
#   → sidecar verifier_ref / evidence_refs / output tree → axon-loop intake.
FAB="$WORK/fabric"; mkdir -p "$FAB/grants"
AXI_SHA="$(sha256sum "$AXI" | cut -d' ' -f1)"
# The operator's registry also names Fabric's SIGNER: the verifier identity it
# issues receipts as, its key, and that key's public half, pinned. No caller
# (MiCode included) names an issuer or a key.
# The ACCEPTANCE CHECK (G3) is the operator's registered, hidden suite: outside
# the candidate's repository, pinned by its WorkspaceVersion, reaching the
# candidate only through `mod calc`. Fabric judges the EXACTLY NAMED check
# (argv[1]); a name that matches no test is not_run, never passed.
SUITE="$FAB/suites/acceptance"; mkdir -p "$SUITE"
cat > "$SUITE/accept.ax" <<'AX'
mod calc
use calc.{double}

@[test]
fn t_ok_double() { assert_eq(double(2), 4) }

@[test]
fn t_ok_zero() { assert_eq(double(0), 0) }

@[test]
fn t_bad() { assert_eq(double(2), 5) }
AX
SUITE_REF="$("$AXF" workspace-import --state "$WORK/suite-ref" --tenant "$TENANT" --root "$SUITE" | jq -r .workspace_version_ref)"
jq -n --arg p "$AXI" --arg s "$AXI_SHA" --arg k "$ISSUER_KEY" --arg pk "$ISSUER_PK" --arg sr "$SUITE" --arg sv "$SUITE_REF" \
  '{schema:"cortex-check-registry/1",executors:[{id:"axon-test-local",path:$p,sha256:$s}],
    checks:[{id:"acceptance",visibility:"hidden",root:$sr,entry:"accept.ax",workspace_version_ref:$sv}],
    signer:{issuer_ref:"gate:independent-verifier",key_path:$k,public_key:$pk}}' > "$FAB/checks.json"
# No effect at all: the acceptance check is pure tests, and a check workload
# that could read files could read the signing key — Fabric would then
# withhold its attestation.
cat > "$FAB/grants/grant_check.axgrant" <<'G'
profile = "restricted"
[grant]
max_label = "internal"
[grant.budget]
cost_micro = 1000
G
CHECK_PRINCIPAL="principal:gate-check"   # NOT micode-host-observer: a task cannot verify itself
jq -n --arg s "$(sha256sum "$FAB/grants/grant_check.axgrant" | cut -d' ' -f1)" --arg pr "$CHECK_PRINCIPAL" \
  '{schema:"axon-fabric-grant-registry/1",grants:[{grant_ref:"grant:check",principal_ref:$pr,path:"grant_check.axgrant",sha256:$s}]}' \
  > "$FAB/grants/grants.json"
EXE_DIGEST="acf1:$(python3 -c 'import json,sys,hashlib;print(hashlib.sha256(json.dumps({"registered_executable_ref":"axon-test-local","sha256":sys.argv[1]},sort_keys=True,separators=(",",":")).encode()).hexdigest())' "$AXI_SHA")"
# The operator's PIN for the verifier: the revision, compute profile and suite
# version its verdicts must come from (G01-r22-independent-issuer).
jq -c --arg ex "$EXE_DIGEST" --arg sv "$SUITE_REF" '.verifier_pins = {"gate:independent-verifier": {
    registered_executable_ref: "axon-test-local", executable_digest: $ex,
    backend_profiles: ["process_scoped/local-interpreter"],
    check_suites: [("check-suite:acceptance@" + $sv)] }}' "$STORE/config.json" > "$STORE/config.json.tmp" \
  && mv "$STORE/config.json.tmp" "$STORE/config.json"
# fabric_check_config <out> <filter> [argv0, default the operator's check:acceptance]
fabric_check_config() {
  jq -n --arg fab "$AXF" --arg j "$FAB/ops.journal" --arg c "$FAB/checks.json" --arg g "$FAB/grants/grants.json" \
        --arg st "$STORE" --arg state "$FAB/state" --arg pr "$CHECK_PRINCIPAL" --arg ex "$EXE_DIGEST" --arg f "$2" \
        --arg entry "${3:-check:acceptance}" '{
    schema:"micode.fabric-check/1", fabric:$fab, journal:$j, check_registry:$c, grant_registry:$g,
    loop_store:$st, state:$state, issuer_ref:"gate:independent-verifier", timeout_s:300,
    request_template:{
      principal_ref:$pr, grant_ref:"grant:check", approval_ref:null,
      registered_executable_ref:"axon-test-local", executable_digest:$ex,
      semantic_state_ref:null, policy_digest:("acf1:"+("c"*64)),
      required:{engine:"axon_interpreter",hardware_isolation:false,os:"none",architecture:"x86_64",
                network_mode:"deny",checkpoint_kind:"none"},
      limits:{cpu_millicores:1000,memory_bytes:268435456,disk_bytes:268435456,wall_time_ms:60000,
              output_bytes:1048576,max_cost_micro:100,currency_code:"USD",price_schedule_ref:"unpriced:gate"},
      argv:[$entry,$f], result_schema_ref:"cortex-check-report/1" } }' > "$1"
}
# expected context whose execution id is the one Fabric mints for the op
g3_context() {  # <out> <trial>
  expected_context "$1" "$2" challenger-1 "$HEAD_SHA" "$RES_EPOCH"
  jq --arg t "$2" '.identity.execution_id = ("exec-"+$t+"-op")' "$1" > "$1.tmp" && mv "$1.tmp" "$1"
}
# sidecar_verification_docs <sidecar> → sets VREQ VRC (files under fabric/)
g3_docs() {
  VRC="$CL/fabric/$(jq -r .verification.verifier_ref "$1" | cut -d: -f2).json"
  VREQ="$CL/fabric/$(jq -r '.verification.evidence_refs[0]' "$1" | cut -d: -f2).json"
  # Fabric's attestation of that receipt, kept by MiCode beside it.
  VATT="$(grep -l '"acf-receipt-attestation/1"' "$CL"/fabric/*.json 2>/dev/null | while read -r f; do
    [ "$(jq -r .receipt_ref "$f")" = "$(jq -r .verification.verifier_ref "$1")" ] && echo "$f"; done | head -1)"
}

fabric_check_config "$WORK/fabric-pass.json" t_ok_double
g3_context "$WORK/exp-g3.json" trial-g3
snap_cl
run_micode g3 MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-g3.json" MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json" \
  MICODE_AXON_FABRIC_CHECK="$WORK/fabric-pass.json"
check "G3: micode exec succeeds" eq "$RC" 0
G3_EP="$(new_file "$CL/episodes" "$SN_EP")"
check "G3: a sidecar was written" test -n "$G3_EP"
check "G3: verification cites a verifier (MiCode ran the check through Fabric)" \
  test "$(jq -r .verification.verifier_ref "$G3_EP")" != null
g3_docs "$G3_EP"
check "G3: the cited receipt is on disk under closed-loop/fabric/" test -s "$VRC"
check "G3: the cited request is on disk under closed-loop/fabric/" test -s "$VREQ"
check "G3: verification passed with matched_checks 1 (the named check, t_ok_double)" \
  eq "$(jq -c '[.verification.result,.verification.matched_checks]' "$G3_EP")" '["passed",1]'
check "G3: receipt is supervisor-observed" eq "$(jq -r .evidence_source "$VRC")" supervisor_observed
check "G3: receipt's operation is this attempt's" eq "$(jq -r .operation_id "$VRC")" "trial-g3-op"
check "G3: receipt's execution id is the sidecar's" eq "$(jq -r .execution_id "$VRC")" "$(jq -r .identity.execution_id "$G3_EP")"
check "G3: the check ran as the check principal, not the subject" eq "$(jq -r .principal_ref "$VREQ")" "$CHECK_PRINCIPAL"
# The tree: MiCode's WorkspaceVersion == Fabric's import == the receipt's input.
check "G3: output_workspace_ref == receipt input tree" eq "$(jq -r .output_workspace_ref "$G3_EP")" "$(jq -r .input_workspace_ref "$VRC")"
FAB_IMPORT="$("$AXF" workspace-import --state "$WORK/fabric-reimport" --tenant "$TENANT" --root "$REPO" | jq -r .workspace_version_ref)"
check "G3: an independent Fabric import of the repo yields the same tree ref" eq "$FAB_IMPORT" "$(jq -r .output_workspace_ref "$G3_EP")"
# D12 limitation, stated on the wire: execution refs are still not-produced markers.
NP_REQ="cl22:$(python3 -c 'import json,hashlib;print(hashlib.sha256(json.dumps({"not_produced_by":"micode","field":"acf_request_ref"},sort_keys=True,separators=(",",":")).encode()).hexdigest())')"
check "G3/D12: acf_request_ref stays MiCode's not-produced marker" eq "$(jq -r .acf_request_ref "$G3_EP")" "$NP_REQ"
# Fabric's own journal witnesses the operation (not MiCode's word for it).
# `status` acts only for the submitting authority (G03-r22-authority-intersection).
fab_status() {  # <op>
  "$AXF" status --journal "$FAB/ops.journal" --op "$1" \
    --grant-registry "$FAB/grants/grants.json" --principal "$CHECK_PRINCIPAL" --grant-ref grant:check
}
FSTAT="$(fab_status trial-g3-op)"
check "G3: Fabric journal holds the op, launched" eq "$(jq -r .launched <<<"$FSTAT")" true
check "G3: Fabric journal op state is completed" eq "$(jq -r .state <<<"$FSTAT")" completed

H_G3="$(store_hash)"
axl intake episode --in "$G3_EP" --context "$CL/context" --ack "$CL/policy-ack" >/dev/null 2>"$WORK/g3-nodocs.err"
check "G3: intake WITHOUT the Fabric documents is refused (exit 4)" eq "$?" 4
check "G3: store unchanged by that refusal" eq "$(store_hash)" "$H_G3"
jq -c '.verification = "failed"' "$VRC" > "$WORK/g3-tampered-rc.json"
axl intake episode --in "$G3_EP" --context "$CL/context" --ack "$CL/policy-ack" \
  --verification-request "$VREQ" --verification-receipt "$WORK/g3-tampered-rc.json" >/dev/null 2>"$WORK/g3-tamper.err"
check "G3: intake with a receipt that is not the cited one is refused (exit 4)" eq "$?" 4
check "G3: store unchanged by that refusal" eq "$(store_hash)" "$H_G3"
# G01-r22-independent-issuer on real bytes: the verdict is AUTHENTICATED.
check "G01: MiCode kept Fabric's attestation of the cited receipt" test -n "$VATT"
check "G01: it is signed by the provisioned verifier key" eq "$(jq -r .public_key "$VATT")" "$ISSUER_PK"
check "G01: it binds this attempt's operation" eq "$(jq -r .operation_id "$VATT")" "trial-g3-op"
axl intake episode --in "$G3_EP" --context "$CL/context" --ack "$CL/policy-ack" \
  --verification-request "$VREQ" --verification-receipt "$VRC" >/dev/null 2>"$WORK/g01-noatt.err"
check "G01: the right documents WITHOUT the attestation are refused (exit 4)" eq "$?" 4
check "G01: ...as unauthenticated" grep -q "not authenticated" "$WORK/g01-noatt.err"
# An impostor: a REAL Fabric process run against a registry of its own that
# pins ITS key under the verifier's name — the one thing a local caller can do.
# It replays the same operation and signs a genuine-looking attestation of the
# very same receipt; only the operator's key registration tells them apart.
IMP_PK="$("$AXF" keygen --out "$WORK/impostor.pk8" | jq -r .public_key)"
jq --arg k "$WORK/impostor.pk8" --arg pk "$IMP_PK" '.signer.key_path = $k | .signer.public_key = $pk' \
  "$FAB/checks.json" > "$WORK/impostor-checks.json"
"$AXF" submit --request "$VREQ" --journal "$FAB/ops.journal" --check-registry "$WORK/impostor-checks.json" \
  --grant-registry "$FAB/grants/grants.json" --store "$STORE" --tenant "$TENANT" --family "$FAMILY" \
  --expected-epoch "$RES_EPOCH" --state "$FAB/state" > "$WORK/g01-impostor.json"
check "G01: the impostor Fabric replayed the same receipt" \
  eq "$(jq -c '[.replayed, (.receipt|tojson)]' "$WORK/g01-impostor.json")" "$(jq -c --slurpfile r "$VRC" '[true, ($r[0]|tojson)]' -n)"
jq -c .receipt_attestation "$WORK/g01-impostor.json" > "$WORK/g01-impostor-att.json"
axl intake episode --in "$G3_EP" --context "$CL/context" --ack "$CL/policy-ack" \
  --verification-request "$VREQ" --verification-receipt "$VRC" \
  --verification-attestation "$WORK/g01-impostor-att.json" >/dev/null 2>"$WORK/g01-impostor.err"
check "G01: an attestation under an unregistered key is refused (exit 4)" eq "$?" 4
check "G01: ...naming the key it is not signed by" grep -q "not by" "$WORK/g01-impostor.err"
check "G01: store unchanged by the refusals" eq "$(store_hash)" "$H_G3"
G3_IN="$(axl intake episode --in "$G3_EP" --context "$CL/context" --ack "$CL/policy-ack" \
  --verification-request "$VREQ" --verification-receipt "$VRC" --verification-attestation "$VATT" 2>"$WORK/g3-intake.err")"
check "G3: intake joins the real Fabric check (exit 0)" eq "$?" 0
check "G3: record cites the receipt" eq "$(jq -r .record.verification_receipt_ref <<<"$G3_IN")" "$(jq -r .verification.verifier_ref "$G3_EP")"
check "G3: Fabric receipt stored content-addressed" \
  test -f "$STORE/fabric-receipts/$(jq -r .verification.verifier_ref "$G3_EP" | cut -d: -f2).json"

# The rubric is the operator's, never the candidate's. The same task with its
# check pointed at a file IN its own tree runs through real Fabric, but the
# verifier does not vouch for candidate bytes: Fabric withholds its attestation,
# so MiCode cites no verdict at all. (That a genuinely SIGNED candidate-tree
# verdict is still refused at intake is axon-loop tests/intake.rs
# a_verdict_counts_only_for_what_the_operator_pinned.)
fabric_check_config "$WORK/fabric-own.json" t_ok_double checks/accept.ax
g3_context "$WORK/exp-g3o.json" trial-g3o
snap_cl
run_micode g3o MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-g3o.json" MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json" \
  MICODE_AXON_FABRIC_CHECK="$WORK/fabric-own.json"
G3O_EP="$(new_file "$CL/episodes" "$SN_EP")"
check "rubric: the task still completes (a withheld attestation never fails a task)" eq "$RC" 0
check "rubric: MiCode cites NO verdict from the candidate's own check file" \
  eq "$(jq -c '[.verification.verifier_ref, .verification.result]' "$G3O_EP")" '[null,"not_run"]'

# A FAILING acceptance check is recorded as failed — never dropped, never passed.
fabric_check_config "$WORK/fabric-fail.json" t_bad
g3_context "$WORK/exp-g3f.json" trial-g3f
snap_cl
run_micode g3f MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-g3f.json" MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json" \
  MICODE_AXON_FABRIC_CHECK="$WORK/fabric-fail.json"
G3F_EP="$(new_file "$CL/episodes" "$SN_EP")"
check "G3: a failing check → verification failed" eq "$(jq -r .verification.result "$G3F_EP")" failed
g3_docs "$G3F_EP"
axl intake episode --in "$G3F_EP" --context "$CL/context" --ack "$CL/policy-ack" \
  --verification-request "$VREQ" --verification-receipt "$VRC" --verification-attestation "$VATT" >/dev/null 2>"$WORK/g3f.err"
check "G3: a failed Fabric check is intaken as failed (exit 0)" eq "$?" 0

# A check name that matches no test is NOT a pass: Fabric says not_run, MiCode records unknown.
fabric_check_config "$WORK/fabric-none.json" t_ok
g3_context "$WORK/exp-g3n.json" trial-g3n
snap_cl
run_micode g3n MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-g3n.json" MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json" \
  MICODE_AXON_FABRIC_CHECK="$WORK/fabric-none.json"
G3N_EP="$(new_file "$CL/episodes" "$SN_EP")"
check "G3: a check name matching no test → unknown with 0 matched (never passed)" \
  eq "$(jq -c '[.verification.result,.verification.matched_checks]' "$G3N_EP")" '["unknown",0]'

# A harness cannot make a check vouch for another trial: identity in the template is refused.
jq '.request_template.trial_id = "someone-else"' "$WORK/fabric-pass.json" > "$WORK/fabric-hijack.json"
g3_context "$WORK/exp-g3h.json" trial-g3h
snap_cl
run_micode g3h MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-g3h.json" MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json" \
  MICODE_AXON_FABRIC_CHECK="$WORK/fabric-hijack.json"
G3H_EP="$(new_file "$CL/episodes" "$SN_EP")"
check "G3: a template naming a task-owned field → no verifier cited (not_run)" \
  eq "$(jq -c '[.verification.result,.verification.verifier_ref]' "$G3H_EP")" '["not_run",null]'
# Exactly unknown_op (exit 5) under the right authority: a bare "nonzero" would
# also be satisfied by an authorization refusal, which proves nothing about the op.
check "G3: Fabric never saw the hijacked op (unknown_op, exit 5)" eq "$(fab_status trial-g3h-op >/dev/null 2>&1; echo $?)" 5
check "G3: status without the submitting authority is refused (exit 7)" \
  eq "$("$AXF" status --journal "$FAB/ops.journal" --op trial-g3-op --grant-registry "$FAB/grants/grants.json" \
        --principal principal:intruder --grant-ref grant:check >/dev/null 2>&1; echo $?)" 7

# Machine-readable: the Stage-5 profile (governance/v022_stage5_verification.json)
# requires this section to have EXECUTED, not merely the gate to have passed.
echo "loop_interop_gate: G3 section executed $(( PASS - G3_PASS0 + FAIL - G3_FAIL0 )) assertions, $(( FAIL - G3_FAIL0 )) failed"

# ════════════════════════════════════════════════════════════════════════════
section "9. B256: real MiCode negotiates closed-loop-profile/1 with the REAL cortex-policy-adapter"
# Before the first model call, from the composition root; the outcome is the
# durable record MiCode writes under closed-loop/profile/.
B256_PASS0=$PASS; B256_FAIL0=$FAIL
PROF="$CL/profile"
b256_run() {  # <label> <adapter path>; sets PREC (the record this run wrote), FIRSTREQ
  local before; before="$(list_dir "$PROF")"
  local nbefore; nbefore="$(nreq)"
  run_micode "$1" MICODE_SEMANTIC_TOOLS=1 MICODE_POLICY_ADAPTER="$2" MICODE_POLICY_NEGOTIATE_PROFILE=1 \
    MICODE_POLICY_GRANT_SNAPSHOT=snap-gate MICODE_POLICY_WRITE_PREFIX=src/ MICODE_POLICY_SNAPSHOT=snap-gate
  PREC="$(new_file "$PROF" "$before")"
  FIRSTREQ="$REQ/req-$(printf '%04d' $((nbefore + 1))).json"
}
# The profile outcome was durable BEFORE the run's first model call (B256:
# negotiation resolves before any model call; nanosecond mtimes).
before_model_call() {
  [ -n "$PREC" ] && [ -f "$PREC" ] && [ -f "$FIRSTREQ" ] && python3 -c '
import os,sys
sys.exit(0 if os.stat(sys.argv[1]).st_mtime_ns <= os.stat(sys.argv[2]).st_mtime_ns else 1)' "$PREC" "$FIRSTREQ"
}
b256_run b256-real "$CPA"
check "B256: micode exec succeeds with negotiation on" eq "$RC" 0
check "B256: a profile record was written" test -n "$PREC"
check "B256: outcome agreed with the real adapter" eq "$(jq -r .outcome "$PREC")" agreed
check "B256: the agreed profile was recorded before the first model call" before_model_call
check "B256: the accept is Axon's (peer axon, role accept)" eq "$(jq -c '[.accept.peer,.accept.role]' "$PREC")" '["axon","accept"]'
check "B256: cortex-policy-adapter/1 agreed; axon-bridge/v0 NOT (Axon never claims MiCode's wire)" \
  eq "$(jq -c '.accept.adapters' "$PREC")" '["cortex-policy-adapter/1"]'
# Two independent cl22 implementations: MiCode confirmed the accept against the
# offer it sent, and the adapter, given that same offer, answers byte-identically.
jq -c .offer "$PREC" > "$WORK/b256-offer.json"
"$CPA" --negotiate < "$WORK/b256-offer.json" > "$WORK/b256-accept.json"; B256_RC=$?
check "B256: the real adapter accepts MiCode's recorded offer (exit 0)" eq "$B256_RC" 0
check "B256: ...with exactly the accept MiCode recorded" eq "$(jq -cS . "$WORK/b256-accept.json")" "$(jq -cS .accept "$PREC")"
# An OLD adapter: the real one, minus --negotiate (as every pre-B256 build answers).
cat > "$WORK/old-adapter.sh" <<SH
#!/bin/sh
if [ "\$1" = --negotiate ]; then echo "unknown argument --negotiate" >&2; exit 2; fi
exec "$CPA" "\$@"
SH
chmod +x "$WORK/old-adapter.sh"
b256_run b256-old "$WORK/old-adapter.sh"
check "B256: an old adapter → micode still runs (incumbent protocol 1)" eq "$RC" 0
check "B256: an old adapter is recorded as old_peer, never as agreed" eq "$(jq -r .outcome "$PREC")" old_peer
# NO COMMON PROFILE, answered by the REAL adapter: it is handed MiCode's offer
# with every schema swapped for one Axon does not speak, so the refusal is the
# real adapter's own `no_common_schema`, and MiCode must record exactly that.
cat > "$WORK/nocommon-adapter.sh" <<SH
#!/bin/sh
if [ "\$1" = --negotiate ]; then
  jq -c '.schemas = ["x.unknown.schema/1"] | .required = []' | "$CPA" --negotiate
  exit \$?
fi
exec "$CPA" "\$@"
SH
chmod +x "$WORK/nocommon-adapter.sh"
b256_run b256-nocommon "$WORK/nocommon-adapter.sh"
check "B256: no common profile → micode still runs (incumbent protocol 1, authority retained)" eq "$RC" 0
check "B256: no common profile is recorded as unsupported, never agreed" eq "$(jq -r .outcome "$PREC")" unsupported
check "B256: ...with the real adapter's own code no_common_schema" eq "$(jq -r .unsupported.code "$PREC")" no_common_schema
check "B256: no common profile resolved (and was recorded) before the first model call" before_model_call
check "B256: no common profile is distinct from an old peer (not old_peer, no accept)" \
  eq "$(jq -c '[.outcome,.accept]' "$PREC")" '["unsupported",null]'
# A LYING peer: the real accept with a schema MiCode never offered.
cat > "$WORK/lying-adapter.sh" <<SH
#!/bin/sh
if [ "\$1" = --negotiate ]; then
  "$CPA" --negotiate | jq -c '.schemas += ["axon.closed-loop.transition/1"] | .schemas |= unique'
  exit 0
fi
exec "$CPA" "\$@"
SH
chmod +x "$WORK/lying-adapter.sh"
b256_run b256-lie "$WORK/lying-adapter.sh"
check "B256: a fabricated accept is recorded as failed, never agreed" eq "$(jq -r .outcome "$PREC")" failed
check "B256: ...and names why" grep -q "does not hold" <<<"$(jq -r .failure "$PREC")"
echo "loop_interop_gate: B256 section executed $(( PASS - B256_PASS0 + FAIL - B256_FAIL0 )) assertions, $(( FAIL - B256_FAIL0 )) failed"


# ════════════════════════════════════════════════════════════════════════════
section "10. G03: revocation is rechecked at TOOL EXECUTION (real pointer show → real MiCode)"
# Last, because it revokes the section-2 policy, and a revoked policy's episodes
# are refused at intake — the sections above rely on it being in force.
# MiCode's fence reads the document `axon-loop pointer show` prints, so this is
# where the two sides' formats must agree: a mismatch fails CLOSED (every call
# refused), which the "in force" half below would catch.
G03_PASS0=$PASS; G03_FAIL0=$FAIL
# scripted_call TOOL ARGS_JSON: the provider's next turn is one call of TOOL.
scripted_call() {
  python3 - "$WORK/next.sse" "$1" "$2" <<'PY'
import json, sys
tool, args = sys.argv[2], json.dumps(json.loads(sys.argv[3]))
ev = lambda name, d: f"event: {name}\ndata: {json.dumps(d)}\n\n"
sse = "".join([
  ev("message_start", {"type":"message_start","message":{"id":"m","type":"message","role":"assistant","model":"claude-sonnet-5","content":[],"stop_reason":None,"usage":{"input_tokens":5,"output_tokens":0}}}),
  ev("content_block_start", {"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"tu1","name":tool,"input":{}}}),
  ev("content_block_delta", {"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":args}}),
  ev("content_block_stop", {"type":"content_block_stop","index":0}),
  ev("message_delta", {"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":5}}),
  ev("message_stop", {"type":"message_stop"})])
open(sys.argv[1] + ".tmp", "w").write(sse)
import os; os.rename(sys.argv[1] + ".tmp", sys.argv[1])
PY
}
# the request that carried the read's tool_result back to the model
result_req() { find "$REQ" -name 'req-*.json' | sort | tail -1; }
expected_context "$WORK/exp-g03.json" trial-g03 challenger-1 "$HEAD_SHA" "$RES_EPOCH"

axl pointer show --tenant "$TENANT" --family "$FAMILY" > "$WORK/view-live.json"
check "G03: pointer show exit 0" eq "$?" 0
check "G03: the live view does not list the pinned policy as revoked" \
  eq "$(jq --arg p "$POL_REF" '[.revocations.revoked[]|select(.policy_ref==$p)]|length' "$WORK/view-live.json")" 0
scripted_call read '{"path":"src/main.rs"}'
run_micode g03-live MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-g03.json" \
  MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json" MICODE_AXON_POLICY_AUTHORITY="$WORK/view-live.json"
check "G03 in force: micode exec succeeds" eq "$RC" 0
check "G03 in force: two provider requests (the tool call, then its result)" eq "$NEWREQ" 2
check "G03 in force: the read ran (its content reached the model)" grep -q 'println' "$(result_req)"
check "G03 in force: no revocation refusal" bash -c "! grep -q 'was revoked' '$(result_req)'"

axl pointer revoke --tenant "$TENANT" --family "$FAMILY" --policy "$POL_REF" \
  --reason "cl22:$(printf 'c%.0s' $(seq 64))" --issuer op:gate-admitter > /dev/null
check "G03: pointer revoke exit 0" eq "$?" 0
axl pointer show --tenant "$TENANT" --family "$FAMILY" > "$WORK/view-revoked.json"
check "G03: the view now lists the pinned policy as revoked" \
  eq "$(jq --arg p "$POL_REF" '[.revocations.revoked[]|select(.policy_ref==$p)]|length' "$WORK/view-revoked.json")" 1
scripted_call read '{"path":"src/main.rs"}'
run_micode g03-revoked MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-g03.json" \
  MICODE_AXON_ACTIVE_POLICY="$WORK/active-policy.json" MICODE_AXON_POLICY_AUTHORITY="$WORK/view-revoked.json"
check "G03 revoked: the task starts (the pin agreed at task start)" eq "$RC" 0
check "G03 revoked: two provider requests (the tool call, then its refusal)" eq "$NEWREQ" 2
check "G03 revoked: the read was refused at execution, naming the revocation" grep -q 'was revoked' "$(result_req)"
check "G03 revoked: the file's content never reached the model" bash -c "! grep -q 'println' '$(result_req)'"
echo "loop_interop_gate: G03 section executed $(( PASS - G03_PASS0 + FAIL - G03_FAIL0 )) assertions, $(( FAIL - G03_FAIL0 )) failed"

# ════════════════════════════════════════════════════════════════════════════
section "11. G16 role-scope: a read-only role writes nothing (real binary)"
# A critic declares no writes (its contract), and an empty write set must not
# read as "unrestricted" at the tool boundary.
expected_context "$WORK/exp-critic.json" trial-critic challenger-1 "$HEAD_SHA" "$RES_EPOCH"
jq '.role="critic" | .write_paths=[]' "$WORK/exp-critic.json" > "$WORK/exp-critic2.json"
scripted_call write '{"path":"src/critic.rs","content":"x"}'
run_micode critic MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-critic2.json"
check "critic: micode exec succeeds (the task runs)" eq "$RC" 0
check "critic: two provider requests (the write, then its refusal)" eq "$NEWREQ" 2
check "critic: the write was refused naming the read-only role" grep -q 'read-only role' "$(result_req)"
check "critic: the file never appeared" test ! -e "$REPO/src/critic.rs"
jq '.role="critic" | .write_paths=["src/**"]' "$WORK/exp-critic.json" > "$WORK/exp-critic3.json"
run_micode critic-w MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-critic3.json"
check "critic declaring writes: TASK_NOT_STARTED, zero provider requests" \
  bash -c "[ $RC -ne 0 ] && [ $NEWREQ -eq 0 ] && grep -q TASK_NOT_STARTED '$WORK/critic-w.err'"

# ════════════════════════════════════════════════════════════════════════════
section "12. G01: the subject cannot read the verifier's signing key (real binary)"
# The attestation authenticates the issuer only while its key is out of the
# subject's reach. A closed-loop implementation task declares a write set, so
# its agent has no unbounded tool (bash et al. are denied) and its file tools
# cannot leave the workspace root. The agent asks to read the key: refused, and
# no byte of it reaches the model.
expected_context "$WORK/exp-key.json" trial-key challenger-1 "$HEAD_SHA" "$RES_EPOCH"
KEY_B64="$(base64 -w0 "$ISSUER_KEY")"
for tool_args in "read|{\"path\":\"$ISSUER_KEY\"}" "bash|{\"command\":\"cat $ISSUER_KEY\"}"; do
  IFS='|' read -r tool args <<<"$tool_args"
  scripted_call "$tool" "$args"
  run_micode "key-$tool" MICODE_AXON_EXPECTED_CONTEXT="$WORK/exp-key.json"
  check "key/$tool: the task runs, the call is answered" bash -c "[ $RC -eq 0 ] && [ $NEWREQ -eq 2 ]"
  check "key/$tool: the call was refused" grep -qiE 'denied|outside|not allowed|refus' "$(result_req)"
  check "key/$tool: no byte of the key reached the model" bash -c "! grep -qF '$KEY_B64' '$(result_req)' && ! grep -qF '$(head -c 24 "$ISSUER_KEY" | base64 -w0)' '$(result_req)'"
done

# ════════════════════════════════════════════════════════════════════════════
section "summary"
TOTAL=$((PASS+FAIL))
echo "assertions: $TOTAL  pass: $PASS  fail: $FAIL"
if [ "$TOTAL" -gt 0 ] && [ "$FAIL" -eq 0 ]; then
  echo "loop_interop_gate: PASS — $TOTAL assertions (axon $(git -C "$AXON_DIR" rev-parse --short HEAD), micode $(git -C "$MICODE_DIR" rev-parse --short HEAD))"
  exit 0
fi
echo "loop_interop_gate: FAIL — $FAIL of $TOTAL assertions failed"
exit 1
