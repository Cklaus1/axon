#!/usr/bin/env bash
# fc_linux_profile.sh — launch ONE job in the protected Linux microVM profile
# (B263) through the Firecracker JAILER, return its output drive, and destroy
# every host resource the launch acquired.
#
# Interface (the contract axon-vm / Fabric is expected to drive; see
# profiles/linux-microvm/README.md for the full description):
#
#   fc_linux_profile.sh --program PROG.ax --out DIR [options]
#
#   --program FILE        Axon program run as /work/job/program.ax (required)
#   --policy FILE         capability policy, schema axon-vm-mmds/1 (required).
#                         Validated here (strict JSON, known keys only, grantable
#                         effect names, constrains something) and embedded as
#                         exactly one `axon.policy=<standard padded base64 of
#                         the file's exact bytes>` kernel-cmdline word, which
#                         axon-guest-init enforces in the guest. A request for an
#                         axis this profile cannot project (path or host scope)
#                         is REFUSED, never silently dropped.
#   --out DIR             result directory, must not exist or be empty (required)
#   --put SRC:DEST        copy SRC into the workspace drive at DEST (repeatable)
#   --vcpus N             guest vCPUs                        (default 1)
#   --mem-mib N           guest RAM                          (default 256)
#   --cg-mem-max BYTES    host cgroup memory.max for the VMM (default mem+128MiB)
#   --cg-pids-max N       host cgroup pids.max for the VMM   (default 16)
#   --cg-cpu-max "Q P"    host cgroup cpu.max                (default "100000 100000")
#   --workspace-mib N     workspace drive size = output cap  (default 64)
#   --timeout-s N         wall-clock limit, then cgroup.kill (default 60)
#   --id ID               jail id (default b263-<random>)
#   --manifest FILE       artifact + engine pins (default dist/guest-linux/manifest.json)
#   --fc-bin FILE         firecracker binary (default /usr/local/bin/firecracker;
#   --jailer-bin FILE     jailer binary       default /usr/local/bin/jailer).
#                         Either must still match the manifest's `engine` pin —
#                         these exist so qualification can prove a swapped
#                         engine is refused, not to bypass the pin.
#
# Test hooks (qualification only; every hooked run lists them in result.json
# `test_hooks`, and neither can make a run admissible that would not be):
#   FC_PROFILE_TEST_POLICY_WORD=W   (set, possibly empty) put W on the cmdline
#                                   INSTEAD of the validated policy word, so the
#                                   GUEST's refusal of an absent/empty/malformed
#                                   policy is observable. No policy sha is
#                                   recorded, so the run is never admissible.
#   FC_PROFILE_TEST_EMBED_POLICY=F  validate and record --policy as usual but
#                                   embed F's bytes: the guest then reports a
#                                   digest the host did not send.
#
# Exit status:
#   0   workload ran, exit 0, output bound to the serial digest
#   10  workload ran, nonzero exit (see result.json workload_exit)
#   20  wall-clock timeout: VMM killed
#   21  VMM died without a completed run (crash, host OOM kill, boot failure)
#   22  refused before launch (artifact or engine digest != manifest, policy
#       absent/invalid/over-long, bad input, not root) — nothing acquired
#       when refused in the pin block
#   23  output drive and serial digest disagree (result NOT admissible)
#   24  cleanup incomplete (result.json lists what was left)
#   25  policy unbound: the guest reported (serial `B263-POLICY sha=<hex>`) a
#       policy other than the one the host embedded, or none (NOT admissible)
#   26  engine unbound: the VMM that ran is not the pinned firecracker (the
#       jailer's chroot copy or the running image differ; NOT admissible)
#
# DIR on return: result.json, serial.log, jailer.log, workspace.img (the
# returned drive), out/ (files extracted from the drive WITHOUT mounting it on
# the host — debugfs reads the image, so a hostile guest filesystem never
# reaches the host kernel's ext4 driver), launch.json (written at launch, for
# a supervisor that must observe or kill the VMM while it runs).
set -uo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE_UID_NAME="axonb263"
CHROOT_BASE="/srv/axon-b263"
CG_PARENT="axon-b263"
FC_BIN="/usr/local/bin/firecracker"
JAILER_BIN="/usr/local/bin/jailer"
# x86 COMMAND_LINE_SIZE is 2048; the kernel keeps 2047 bytes and silently drops
# the rest. axon-guest-init refuses a cmdline over 2046 bytes as possibly
# truncated, so the launcher refuses to build one instead of booting a guest
# that will refuse.
CMDLINE_MAX=2046

BOOT_ARGS="console=ttyS0 reboot=k panic=1 pci=off acpi=off root=/dev/vda rootfstype=squashfs ro init=/init loglevel=4"

# ── --reap ID: destroy whatever a launch with this id left behind ────────────
# For a supervisor that died (or was SIGKILLed) between acquisition and its own
# cleanup. Idempotent; exit 0 only when re-observation finds nothing left.
if [[ "${1:-}" == "--reap" ]]; then
    RID="${2:-}"
    [[ "$RID" =~ ^[a-zA-Z0-9-]{1,60}$ ]] || { echo "fc_linux_profile: --reap needs a valid id" >&2; exit 22; }
    RCG="/sys/fs/cgroup/$CG_PARENT/$RID"; RJAIL="$CHROOT_BASE/firecracker/$RID"
    [[ -f "$RCG/cgroup.kill" ]] && echo 1 > "$RCG/cgroup.kill"
    for pid in $(pgrep -f -- "--id $RID( |$)"); do kill -9 "$pid" 2>/dev/null; done
    for _ in $(seq 50); do [[ -d "$RCG" ]] || break; rmdir "$RCG" 2>/dev/null && break; sleep 0.1; done
    for m in $(awk -v p="$RJAIL" 'index($2,p)==1 {print $2}' /proc/mounts | sort -r); do umount -l "$m"; done
    rm -rf "$RJAIL"; ip netns del "$RID" 2>/dev/null
    left=()
    pgrep -f -- "--id $RID( |$)" >/dev/null && left+=("process")
    [[ -e "$RCG" ]] && left+=("cgroup")
    [[ -e "$RJAIL" ]] && left+=("chroot")
    grep -q -- "$RJAIL" /proc/mounts && left+=("mount")
    ip netns list | grep -qw -- "$RID" && left+=("netns")
    echo "{\"reaped\":\"$RID\",\"left_behind\":\"${left[*]:-}\"}"
    [[ ${#left[@]} -eq 0 ]]; exit $?
fi

# ── --verify-result DIR: re-derive the output binding from the returned drive ─
# Independent of the launch: re-extracts /out/stdout from DIR/workspace.img and
# checks it against the digest the guest printed on the serial console and the
# digest recorded in result.json. Exit 0 bound, 23 not bound.
if [[ "${1:-}" == "--verify-result" ]]; then
    VD="${2:-}"; [[ -f "$VD/workspace.img" && -f "$VD/serial.log" && -f "$VD/result.json" ]] || exit 22
    T="$(mktemp -d)"
    debugfs -R "dump /out/stdout $T/stdout" "$VD/workspace.img" >/dev/null 2>&1
    D="$( [[ -f "$T/stdout" ]] && sha256sum "$T/stdout" | cut -d' ' -f1 )"
    rm -rf "$T"
    S="$(sed -n 's/.*B263-OUT stdout=\([0-9a-f]*\) exit=.*/\1/p' "$VD/serial.log" | tr -d '\r' | tail -1)"
    R="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["outputs"]["stdout"]["sha256"])' "$VD/result.json" 2>/dev/null)"
    # the policy the guest reported (FIRST line: printed before the workload
    # starts, so a workload cannot pre-empt it) must be the one recorded
    SP="$(sed -n 's/.*B263-POLICY sha=\([0-9a-f]*\).*/\1/p' "$VD/serial.log" | tr -d '\r' | head -1)"
    RP="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("policy_sha256") or "")' "$VD/result.json" 2>/dev/null)"
    echo "{\"drive\":\"$D\",\"serial\":\"$S\",\"result\":\"$R\",\"policy_serial\":\"$SP\",\"policy_result\":\"$RP\"}"
    [[ -n "$D" && "$D" == "$S" && "$D" == "$R" && -n "$RP" && "$SP" == "$RP" ]] && exit 0
    exit 23
fi

inject() {  # fault injection for cleanup qualification (FC_PROFILE_INJECT_FAIL)
    if [[ "${FC_PROFILE_INJECT_FAIL:-}" == "$1" ]]; then
        echo "fc_linux_profile: injected failure at $1" >&2
        STATUS="injected-failure:$1"; RC=21; exit 21
    fi
}

PROGRAM="" OUT="" VCPUS=1 ADIR="" MEM_MIB=256 CG_MEM_MAX="" CG_PIDS_MAX=16
CG_CPU_MAX="100000 100000" WS_MIB=64 TIMEOUT_S=60 ID="" MANIFEST="$REPO/dist/guest-linux/manifest.json"
PUTS=()
POLICY=""

die_usage() { echo "fc_linux_profile: $*" >&2; exit 22; }

while [[ $# -gt 0 ]]; do
    case "$1" in
        --program) PROGRAM="$2"; shift 2 ;;
        --policy) POLICY="$2"; shift 2 ;;
        --fc-bin) FC_BIN="$2"; shift 2 ;;
        --jailer-bin) JAILER_BIN="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        --put) PUTS+=("$2"); shift 2 ;;
        --vcpus) VCPUS="$2"; shift 2 ;;
        --mem-mib) MEM_MIB="$2"; shift 2 ;;
        --cg-mem-max) CG_MEM_MAX="$2"; shift 2 ;;
        --cg-pids-max) CG_PIDS_MAX="$2"; shift 2 ;;
        --cg-cpu-max) CG_CPU_MAX="$2"; shift 2 ;;
        --workspace-mib) WS_MIB="$2"; shift 2 ;;
        --timeout-s) TIMEOUT_S="$2"; shift 2 ;;
        --id) ID="$2"; shift 2 ;;
        --manifest) MANIFEST="$2"; shift 2 ;;
        --artifacts-dir) ADIR="$2"; shift 2 ;;
        *) die_usage "unknown argument $1" ;;
    esac
done
[[ -n "$PROGRAM" && -f "$PROGRAM" ]] || die_usage "--program FILE required"
[[ -n "$OUT" ]] || die_usage "--out DIR required"
# the jailer names the chroot after the exec file's basename; JAIL_DIR below
# assumes `firecracker`
[[ "$(basename "$FC_BIN")" == firecracker ]] || die_usage "--fc-bin must be a file named 'firecracker'"
[[ -f "$FC_BIN" && -f "$JAILER_BIN" ]] || die_usage "firecracker/jailer binary missing ($FC_BIN, $JAILER_BIN)"
[[ "$(id -u)" == 0 ]] || die_usage "must run as root (jailer needs it to drop to the profile uid)"
[[ -z "$CG_MEM_MAX" ]] && CG_MEM_MAX=$(( (MEM_MIB + 128) * 1024 * 1024 ))
[[ -z "$ID" ]] && ID="b263-$(od -An -N6 -tx1 /dev/urandom | tr -d ' \n')"
[[ "$ID" =~ ^[a-zA-Z0-9-]{1,60}$ ]] || die_usage "bad --id"
mkdir -p "$OUT" && [[ -z "$(ls -A "$OUT")" ]] || die_usage "--out must be a new/empty directory"
OUT="$(cd "$OUT" && pwd)"

PUID="$(id -u "$PROFILE_UID_NAME" 2>/dev/null)" || die_usage "profile user $PROFILE_UID_NAME missing (useradd --system --user-group $PROFILE_UID_NAME)"
PGID="$(id -g "$PROFILE_UID_NAME")"

JAIL_DIR="$CHROOT_BASE/firecracker/$ID"
CHROOT="$JAIL_DIR/root"
CG_DIR="/sys/fs/cgroup/$CG_PARENT/$ID"
NETNS="$ID"
FC_PID=""
STATUS="launch-refused"
RC=22

json_str() { python3 -c 'import json,sys; print(json.dumps(sys.argv[1]))' "$1"; }

# ── 1. pins: every artifact must match the manifest BEFORE anything is acquired
read_pin() { python3 -c 'import json,sys; m=json.load(open(sys.argv[1])); print(m["artifacts"][sys.argv[2]]["sha256"])' "$MANIFEST" "$1"; }
[[ -z "$ADIR" ]] && ADIR="$REPO/dist/guest-linux"
KERNEL="$ADIR/vmlinux"
ROOTFS="$ADIR/rootfs.sqfs"
[[ -f "$MANIFEST" ]] || die_usage "manifest $MANIFEST missing (run AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh)"
PIN_KERNEL="$(read_pin vmlinux)"; PIN_ROOTFS="$(read_pin rootfs.sqfs)"; PIN_AXON="$(read_pin axon)"
GOT_KERNEL="$(sha256sum "$KERNEL" | cut -d' ' -f1)"
GOT_ROOTFS="$(sha256sum "$ROOTFS" | cut -d' ' -f1)"
PROG_SHA="$(sha256sum "$PROGRAM" | cut -d' ' -f1)"
# refuse_prelaunch REASON [EXTRA_JSON_FIELDS] — nothing has been acquired yet
refuse_prelaunch() {
    python3 - "$OUT/result.json" "$ID" "$1" "${2:-{\}}" <<'PY'
import json, sys
p, jid, reason, extra = sys.argv[1:5]
r = {"schema": "axon-linux-microvm-result/1", "id": jid, "status": "launch-refused",
     "exit_code": 22, "reason": reason, "admissible": False, "acquired": []}
r.update(json.loads(extra))
json.dump(r, open(p, "w"), indent=2)
PY
    echo "fc_linux_profile: REFUSED: $1 (nothing acquired)" >&2
    exit 22
}
# ── 1a. engine: firecracker + jailer must match the manifest's `engine` pins.
# They were exec'd from fixed paths with no digest check, so a swapped VMM ran
# under the qualification of the one that was measured.
read_engine_pin() { python3 -c 'import json,sys; m=json.load(open(sys.argv[1])); v=(m.get("engine") or {}).get(sys.argv[2]); print(v if isinstance(v,str) else "")' "$MANIFEST" "$1"; }
PIN_FC="$(read_engine_pin firecracker_sha256)"; PIN_JAILER="$(read_engine_pin jailer_sha256)"
GOT_FC="$(sha256sum "$FC_BIN" | cut -d' ' -f1)"
GOT_JAILER="$(sha256sum "$JAILER_BIN" | cut -d' ' -f1)"
ENGINE_JSON="{\"engine\":{\"firecracker\":{\"path\":$(json_str "$FC_BIN"),\"pinned\":\"$PIN_FC\",\"actual\":\"$GOT_FC\"},\"jailer\":{\"path\":$(json_str "$JAILER_BIN"),\"pinned\":\"$PIN_JAILER\",\"actual\":\"$GOT_JAILER\"}}}"
[[ "$PIN_FC" =~ ^[0-9a-f]{64}$ && "$PIN_JAILER" =~ ^[0-9a-f]{64}$ ]] \
    || refuse_prelaunch "manifest carries no engine pins (engine.firecracker_sha256 / engine.jailer_sha256); an unpinned VMM is not launched" "$ENGINE_JSON"
[[ "$GOT_FC" == "$PIN_FC" && "$GOT_JAILER" == "$PIN_JAILER" ]] \
    || refuse_prelaunch "engine digest does not match manifest" "$ENGINE_JSON"
if [[ "$GOT_KERNEL" != "$PIN_KERNEL" || "$GOT_ROOTFS" != "$PIN_ROOTFS" ]]; then
    cat > "$OUT/result.json" <<EOF
{"schema":"axon-linux-microvm-result/1","id":"$ID","status":"launch-refused",
 "reason":"artifact digest does not match manifest",
 "kernel":{"pinned":"$PIN_KERNEL","actual":"$GOT_KERNEL"},
 "rootfs":{"pinned":"$PIN_ROOTFS","actual":"$GOT_ROOTFS"},"acquired":[]}
EOF
    echo "fc_linux_profile: REFUSED: artifact digest mismatch (nothing acquired)" >&2
    exit 22
fi

# ── 1b. policy: validated and encoded BEFORE anything is acquired ─────────────
TEST_HOOKS=()
POLICY_SHA="" POLICY_BYTES=0 POLICY_WORD=""
if [[ -n "${FC_PROFILE_TEST_POLICY_WORD+x}" ]]; then
    TEST_HOOKS+=("FC_PROFILE_TEST_POLICY_WORD")
    POLICY_WORD="$FC_PROFILE_TEST_POLICY_WORD"
else
    [[ -n "$POLICY" ]] || refuse_prelaunch "no --policy: the guest refuses to run a workload without a capability policy, so the launcher does not boot one"
    [[ -f "$POLICY" ]] || refuse_prelaunch "--policy $POLICY is not a file"
    PV="$(python3 - "$POLICY" <<'PY' 2>&1
import json, sys
# Strict: duplicate keys (compared as decoded), NaN/Infinity, non-UTF-8, a
# non-object top level, unknown keys and wrong types are all refusals. The
# guest parser is equally strict about duplicates and schema; this adds what it
# cannot see from inside: an axis the profile cannot enforce.
raw = open(sys.argv[1], "rb").read()
def fail(m): print(m); sys.exit(1)
def no_dups(pairs):
    seen = set()
    for k, _ in pairs:
        if k in seen: fail(f"duplicate key {k!r}")
        seen.add(k)
    return dict(pairs)
def no_const(c): fail(f"non-finite number {c}")
try: text = raw.decode("utf-8")
except UnicodeDecodeError as e: fail(f"not UTF-8: {e}")
try: p = json.loads(text, object_pairs_hook=no_dups, parse_constant=no_const)
except json.JSONDecodeError as e: fail(f"malformed JSON: {e}")
if not isinstance(p, dict): fail("top-level value is not a JSON object")
if p.get("schema") != "axon-vm-mmds/1": fail(f"schema must be 'axon-vm-mmds/1', got {p.get('schema')!r}")
# ACF-G26 / operator default D8: path and host scope are NOT projected into this
# profile's guest. A request for them is refused by name — the guest's parser
# ignores unknown keys, so passing one through would read as enforced.
SCOPE = {"fs", "fs_read", "fs_write", "fs_scope", "paths", "path", "net", "net_hosts",
         "hosts", "host", "scope", "exec_scope"}
KNOWN = {"schema", "principal", "allowed_effects", "budget_tokens", "source_hash",
         "seccomp_bpf_b64", "run_id"}
scope = sorted(k for k in p if k in SCOPE)
if scope: fail(f"unsupported axis: {scope} (path/host scope is not projected into the linux-microvm guest; refused, not ignored)")
unknown = sorted(k for k in p if k not in KNOWN)
if unknown: fail(f"unknown key(s) {unknown}: an axis the guest does not read would be silently unenforced")
GRANTABLE = {"AI", "Bpf", "Chan", "Exec", "Hal", "IO", "Net", "Pure", "Random", "Tee", "Time"}
ae = p.get("allowed_effects")
if ae is not None:
    if not isinstance(ae, list) or not all(isinstance(e, str) for e in ae): fail("allowed_effects must be a list of strings")
    bad = sorted(e for e in ae if e not in GRANTABLE)
    if bad: fail(f"allowed_effects names {bad}, which are not effects (valid: {sorted(GRANTABLE)})")
bt = p.get("budget_tokens")
if bt is not None and (isinstance(bt, bool) or not isinstance(bt, int) or bt < 0): fail("budget_tokens must be a non-negative integer")
for k in ("principal", "source_hash", "seccomp_bpf_b64", "run_id"):
    if p.get(k) is not None and not isinstance(p[k], str): fail(f"{k} must be a string")
if ae is None and bt is None and p.get("seccomp_bpf_b64") is None:
    fail("policy constrains nothing (no allowed_effects, budget_tokens or seccomp_bpf_b64)")
PY
)" || refuse_prelaunch "invalid --policy: $PV"
    POLICY_SHA="$(sha256sum "$POLICY" | cut -d' ' -f1)"
    POLICY_BYTES="$(stat -c %s "$POLICY")"
    EMBED="$POLICY"
    if [[ -n "${FC_PROFILE_TEST_EMBED_POLICY:-}" ]]; then
        TEST_HOOKS+=("FC_PROFILE_TEST_EMBED_POLICY")
        EMBED="$FC_PROFILE_TEST_EMBED_POLICY"
    fi
    POLICY_WORD="axon.policy=$(base64 -w0 < "$EMBED")"
fi
BOOT_ARGS_FULL="$BOOT_ARGS${POLICY_WORD:+ $POLICY_WORD}"
CMDLINE_BYTES="$(printf '%s' "$BOOT_ARGS_FULL" | wc -c)"
(( CMDLINE_BYTES <= CMDLINE_MAX )) \
    || refuse_prelaunch "kernel cmdline would be $CMDLINE_BYTES bytes (> $CMDLINE_MAX): the guest kernel may truncate the policy word" "{\"cmdline_bytes\":$CMDLINE_BYTES}"
TEST_HOOKS_JSON="$(printf '%s\n' "${TEST_HOOKS[@]:-}" | python3 -c 'import sys,json; print(json.dumps([l for l in sys.stdin.read().splitlines() if l]))')"

# ── cleanup: runs on every exit path; records what it did and what remained ──
ACQUIRED=()
cleanup() {
    local rc_in=$?
    set +e
    local killed_by_cleanup=false
    if [[ -n "$FC_PID" ]] && kill -0 "$FC_PID" 2>/dev/null; then
        [[ -f "$CG_DIR/cgroup.kill" ]] && echo 1 > "$CG_DIR/cgroup.kill"
        kill -9 "$FC_PID" 2>/dev/null
        killed_by_cleanup=true
        for _ in $(seq 50); do kill -0 "$FC_PID" 2>/dev/null || break; sleep 0.1; done
    fi
    # cgroup stats must be read BEFORE the directory is removed
    local cg_stats="{}"
    if [[ -d "$CG_DIR" ]]; then
        cg_stats=$(python3 - "$CG_DIR" <<'PY'
import sys, os, json
d = sys.argv[1]
def rd(n):
    try: return open(os.path.join(d, n)).read().strip()
    except OSError: return None
def kv(n):
    v = rd(n)
    return dict(l.split() for l in v.splitlines()) if v else None
print(json.dumps({"memory.max": rd("memory.max"), "memory.swap.max": rd("memory.swap.max"),
  "memory.peak": rd("memory.peak"), "memory.events": kv("memory.events"),
  "pids.max": rd("pids.max"), "pids.peak": rd("pids.peak"), "pids.events": kv("pids.events"),
  "cpu.max": rd("cpu.max"), "cpu.stat": kv("cpu.stat")}))
PY
)
        for _ in $(seq 50); do rmdir "$CG_DIR" 2>/dev/null && break; sleep 0.1; done
    fi
    # the jailer pivots into its own mount namespace, so nothing should be
    # mounted on the host side; unmount defensively if something is.
    local m
    for m in $(awk -v p="$JAIL_DIR" 'index($2,p)==1 {print $2}' /proc/mounts | sort -r); do
        umount -l "$m" 2>/dev/null
    done
    rm -rf "$JAIL_DIR" 2>/dev/null
    [[ -n "${ENGINE_DIR:-}" ]] && rm -rf "$ENGINE_DIR" 2>/dev/null
    ip netns del "$NETNS" 2>/dev/null

    # verification — independent re-observation, not trust in the calls above
    local left=()
    [[ -n "$FC_PID" ]] && kill -0 "$FC_PID" 2>/dev/null && left+=("vmm-pid:$FC_PID")
    pgrep -f -- "--id $ID( |$)" >/dev/null 2>&1 && left+=("process-matching-id")
    [[ -e "$CG_DIR" ]] && left+=("cgroup:$CG_DIR")
    [[ -e "$JAIL_DIR" ]] && left+=("chroot:$JAIL_DIR")
    [[ -n "${ENGINE_DIR:-}" && -e "$ENGINE_DIR" ]] && left+=("engine-copy:$ENGINE_DIR")
    grep -q -- "$JAIL_DIR" /proc/mounts && left+=("mount-under:$JAIL_DIR")
    ip netns list 2>/dev/null | grep -qw -- "$NETNS" && left+=("netns:$NETNS")
    local left_json
    left_json=$(printf '%s\n' "${left[@]:-}" | python3 -c 'import sys,json; print(json.dumps([l for l in sys.stdin.read().splitlines() if l]))')
    local acq_json
    acq_json=$(printf '%s\n' "${ACQUIRED[@]:-}" | python3 -c 'import sys,json; print(json.dumps([l for l in sys.stdin.read().splitlines() if l]))')
    if [[ ${#left[@]} -gt 0 ]]; then STATUS="cleanup-incomplete"; RC=24; fi

    python3 - "$OUT" "$STATUS" "$RC" "$ID" "$cg_stats" "$left_json" "$acq_json" \
        "$killed_by_cleanup" <<'PY'
import json, os, sys
out, status, rc, jid, cg, left, acq, killed = sys.argv[1:9]
p = os.path.join(out, "result.json")
r = json.load(open(p)) if os.path.exists(p) else {"schema": "axon-linux-microvm-result/1", "id": jid}
r["status"] = status
r["exit_code"] = int(rc)
r["cgroup_final"] = json.loads(cg)
r["cleanup"] = {"acquired": json.loads(acq), "left_behind": json.loads(left),
                "complete": json.loads(left) == [], "vmm_killed_by_cleanup": killed == "true"}
json.dump(r, open(p, "w"), indent=2)
PY
    exit "$RC"
}
trap cleanup EXIT
trap 'STATUS="interrupted"; RC=21; exit 21' INT TERM

# ── 2. workspace drive (job in, result out) ───────────────────────────────────
STAGE="$(mktemp -d)"
mkdir -p "$STAGE/job" "$STAGE/out"
cp "$PROGRAM" "$STAGE/job/program.ax"
for p in "${PUTS[@]:-}"; do
    [[ -z "$p" ]] && continue
    src="${p%%:*}"; dest="${p#*:}"
    mkdir -p "$STAGE/$(dirname "$dest")"
    cp "$src" "$STAGE/$dest"
done
truncate -s "${WS_MIB}M" "$OUT/workspace.img"
mkfs.ext4 -q -F -O ^has_journal -E root_owner=0:0 -d "$STAGE" "$OUT/workspace.img" \
    || { rm -rf "$STAGE"; STATUS="launch-refused"; RC=22; exit 22; }
rm -rf "$STAGE"
WS_IN_SHA="$(sha256sum "$OUT/workspace.img" | cut -d' ' -f1)"

# ── 3. chroot contents (acquired) ─────────────────────────────────────────────
mkdir -p "$CHROOT"; ACQUIRED+=("chroot:$JAIL_DIR")
inject after-chroot
cp "$KERNEL" "$CHROOT/vmlinux"
cp "$ROOTFS" "$CHROOT/rootfs.sqfs"
# re-check the COPIES the VMM will actually open (TOCTOU between pin check and copy)
if [[ "$(sha256sum "$CHROOT/vmlinux" | cut -d' ' -f1)" != "$PIN_KERNEL" ||
      "$(sha256sum "$CHROOT/rootfs.sqfs" | cut -d' ' -f1)" != "$PIN_ROOTFS" ]]; then
    echo "fc_linux_profile: REFUSED: chroot copy digest mismatch" >&2
    STATUS="launch-refused"; RC=22; exit 22
fi
# The engine is exec'd from a PRIVATE root-only copy whose digest is re-checked,
# not from the path checked in the pin block (TOCTOU between check and exec).
ENGINE_DIR="$(mktemp -d /run/axon-b263-engine.XXXXXX)"; ACQUIRED+=("engine-copy:$ENGINE_DIR")
chmod 0700 "$ENGINE_DIR"
cp "$FC_BIN" "$ENGINE_DIR/firecracker"; cp "$JAILER_BIN" "$ENGINE_DIR/jailer"
chmod 0755 "$ENGINE_DIR/firecracker" "$ENGINE_DIR/jailer"
if [[ "$(sha256sum "$ENGINE_DIR/firecracker" | cut -d' ' -f1)" != "$PIN_FC" ||
      "$(sha256sum "$ENGINE_DIR/jailer" | cut -d' ' -f1)" != "$PIN_JAILER" ]]; then
    echo "fc_linux_profile: REFUSED: engine copy digest mismatch" >&2
    STATUS="launch-refused"; RC=22; exit 22
fi
# read-only to the VMM uid at the FILE level too, not only is_read_only
chown root:root "$CHROOT/vmlinux" "$CHROOT/rootfs.sqfs"; chmod 0444 "$CHROOT/vmlinux" "$CHROOT/rootfs.sqfs"
# workspace goes in via a hard link-free copy so the VMM owns only that file
mv "$OUT/workspace.img" "$CHROOT/workspace.img"
chown "$PUID:$PGID" "$CHROOT/workspace.img"; chmod 0600 "$CHROOT/workspace.img"
cat > "$CHROOT/vm.json" <<EOF
{
  "boot-source": {
    "kernel_image_path": "vmlinux",
    "boot_args": "$BOOT_ARGS_FULL"
  },
  "drives": [
    {"drive_id": "rootfs", "path_on_host": "rootfs.sqfs", "is_root_device": true, "is_read_only": true},
    {"drive_id": "workspace", "path_on_host": "workspace.img", "is_root_device": false, "is_read_only": false}
  ],
  "machine-config": {"vcpu_count": $VCPUS, "mem_size_mib": $MEM_MIB, "smt": false},
  "network-interfaces": []
}
EOF
chmod 0444 "$CHROOT/vm.json"

# empty network namespace (defense in depth: even the VMM process has no route)
ip netns add "$NETNS" && ACQUIRED+=("netns:$NETNS")
inject after-netns

# cgroup parent must delegate the controllers the jailer writes
mkdir -p "/sys/fs/cgroup/$CG_PARENT"
echo "+cpu +memory +pids" > /sys/fs/cgroup/cgroup.subtree_control 2>/dev/null
echo "+cpu +memory +pids" > "/sys/fs/cgroup/$CG_PARENT/cgroup.subtree_control" 2>/dev/null

# ── 4. launch through the jailer ──────────────────────────────────────────────
START_NS=$(date +%s%N)
ACQUIRED+=("cgroup:$CG_DIR")
# env -i: nothing from the invoking shell (API keys, AXON_* policy) reaches the VMM
env -i PATH=/usr/sbin:/usr/bin:/sbin:/bin \
    "$ENGINE_DIR/jailer" \
    --id "$ID" --uid "$PUID" --gid "$PGID" \
    --exec-file "$ENGINE_DIR/firecracker" \
    --chroot-base-dir "$CHROOT_BASE" \
    --cgroup-version 2 --parent-cgroup "$CG_PARENT" \
    --cgroup "memory.max=$CG_MEM_MAX" --cgroup "memory.swap.max=0" \
    --cgroup "pids.max=$CG_PIDS_MAX" --cgroup "cpu.max=$CG_CPU_MAX" \
    --resource-limit "no-file=128" --resource-limit "fsize=$(( WS_MIB * 1024 * 1024 + 1 ))" \
    --netns "/var/run/netns/$NETNS" \
    -- --no-api --config-file vm.json --level Warning \
    > "$OUT/serial.log" 2> "$OUT/jailer.log" < /dev/null &
FC_PID=$!
ACQUIRED+=("vmm-pid:$FC_PID")
cat > "$OUT/launch.json" <<EOF
{"id":"$ID","vmm_pid":$FC_PID,"chroot":"$CHROOT","cgroup":"$CG_DIR","netns":"$NETNS",
 "uid":$PUID,"gid":$PGID,"started_ns":$START_NS,"policy_sha256":"$POLICY_SHA","test_hooks":$TEST_HOOKS_JSON}
EOF
if [[ "${FC_PROFILE_INJECT_FAIL:-}" == after-launch ]]; then
    # let the jailer finish creating the cgroup + chroot mounts, then fail
    for _ in $(seq 50); do [[ -d "$CG_DIR" ]] && break; sleep 0.05; done
    sleep 0.3
    inject after-launch
fi

# ── 5. host-side observation of the running VMM (independent of the guest) ───
observe() {
    python3 - "$FC_PID" "$CHROOT" <<'PY'
import os, sys, json
pid, chroot = sys.argv[1], sys.argv[2]
def rd(p):
    try: return open(p).read()
    except OSError: return None
st = rd(f"/proc/{pid}/status") or ""
f = dict(l.split(":", 1) for l in st.splitlines() if ":" in l)
g = lambda k: f.get(k, "").strip()
threads = {}
for t in os.listdir(f"/proc/{pid}/task") if os.path.isdir(f"/proc/{pid}/task") else []:
    ts = rd(f"/proc/{pid}/task/{t}/status") or ""
    tf = dict(l.split(":", 1) for l in ts.splitlines() if ":" in l)
    threads[t] = {"name": tf.get("Name", "").strip(), "seccomp": tf.get("Seccomp", "").strip()}
env = rd(f"/proc/{pid}/environ") or ""
try: root = os.readlink(f"/proc/{pid}/root")
except OSError: root = None
try: rootls = sorted(os.listdir(f"/proc/{pid}/root/"))
except OSError: rootls = None
try:
    ns_net = os.readlink(f"/proc/{pid}/ns/net"); host_net = os.readlink("/proc/1/ns/net")
    ns_mnt = os.readlink(f"/proc/{pid}/ns/mnt"); host_mnt = os.readlink("/proc/1/ns/mnt")
except OSError: ns_net = host_net = ns_mnt = host_mnt = None
netdevs = None
try:
    netdevs = [l.split(":")[0].strip() for l in open(f"/proc/{pid}/net/dev").read().splitlines()[2:]]
except OSError: pass
print(json.dumps({
  "exe_name": g("Name"), "uid": g("Uid").split(), "gid": g("Gid").split(),
  "seccomp_main": g("Seccomp"), "no_new_privs": g("NoNewPrivs"),
  "cap_eff": g("CapEff"), "threads": threads,
  "environ_vars": [e.split("=", 1)[0] for e in env.split("\0") if e],
  "root": root, "root_listing": rootls,
  "cgroup": (rd(f"/proc/{pid}/cgroup") or "").strip(),
  "own_netns": ns_net != host_net, "own_mntns": ns_mnt != host_mnt,
  "vmm_netns_devices": netdevs,
}))
PY
}

HOST_OBS="{}"
DEADLINE=$(( START_NS + TIMEOUT_S * 1000000000 ))
# Observe once the jailer has exec'd into firecracker (comm changes) and the
# vcpu thread exists — i.e. the VMM is in its final, jailed, filtered state.
# Polled tightly: a trivial job finishes in well under a second.
for _ in $(seq 400); do
    kill -0 "$FC_PID" 2>/dev/null || break
    if [[ "$(cat /proc/$FC_PID/comm 2>/dev/null)" == firecracker ]] &&
       grep -qs "fc_vcpu" /proc/$FC_PID/task/*/comm; then
        HOST_OBS="$(observe)"; echo "$HOST_OBS" > "$OUT/host_observed.json"
        ENGINE_RUN_SHA="$(sha256sum /proc/$FC_PID/exe 2>/dev/null | cut -d' ' -f1)"
        break
    fi
    sleep 0.01
done
while kill -0 "$FC_PID" 2>/dev/null; do
    if (( $(date +%s%N) >= DEADLINE )); then
        [[ "$HOST_OBS" == "{}" ]] && { HOST_OBS="$(observe)"; echo "$HOST_OBS" > "$OUT/host_observed.json"; }
        echo 1 > "$CG_DIR/cgroup.kill" 2>/dev/null
        wait "$FC_PID" 2>/dev/null
        STATUS="timeout"; RC=20
        break
    fi
    sleep 0.2
done
wait "$FC_PID" 2>/dev/null; VMM_EXIT=$?
[[ "$STATUS" == "timeout" ]] || VMM_EXIT_REAL=$VMM_EXIT
END_NS=$(date +%s%N)

# ── 6. return the output drive and bind it to the serial digest ───────────────
cp "$CHROOT/workspace.img" "$OUT/workspace.img" 2>/dev/null
# the jailer's own copy of the engine inside the chroot, i.e. what it exec'd
ENGINE_CHROOT_SHA="$(sha256sum "$CHROOT/firecracker" 2>/dev/null | cut -d' ' -f1)"
ROOTFS_AFTER_SHA="$(sha256sum "$CHROOT/rootfs.sqfs" 2>/dev/null | cut -d' ' -f1)"
if [[ -f "$OUT/workspace.img" ]]; then
    RD="$(mktemp -d)"
    debugfs -R "rdump /out $RD" "$OUT/workspace.img" >/dev/null 2>&1
    [[ -d "$RD/out" ]] && mv "$RD/out" "$OUT/out"
    rm -rf "$RD"
fi
mkdir -p "$OUT/out"
SERIAL_OUT_SHA="$(sed -n 's/.*B263-OUT stdout=\([0-9a-f]*\) exit=.*/\1/p' "$OUT/serial.log" | tr -d '\r' | tail -1)"
SERIAL_EXIT="$(sed -n 's/.*B263-OUT stdout=[0-9a-f]* exit=\([0-9]*\).*/\1/p' "$OUT/serial.log" | tr -d '\r' | tail -1)"
DONE=false; grep -q "B263-DONE" "$OUT/serial.log" && DONE=true
# FIRST B263-POLICY line: guest-init.sh prints it before the workload starts, so
# a workload writing to the console later cannot pre-empt it.
SERIAL_POLICY="$(grep -a -m1 'B263-POLICY ' "$OUT/serial.log" | sed 's/.*B263-POLICY //' | tr -d '\r')"
SERIAL_POLICY_SHA="$(printf '%s' "$SERIAL_POLICY" | sed -n 's/^sha=\([0-9a-f]\{64\}\)$/\1/p')"
POLICY_BOUND=false
[[ -n "$POLICY_SHA" && "$SERIAL_POLICY_SHA" == "$POLICY_SHA" ]] && POLICY_BOUND=true
ENGINE_BOUND=true
[[ "$ENGINE_CHROOT_SHA" == "$PIN_FC" ]] || ENGINE_BOUND=false
[[ -z "${ENGINE_RUN_SHA:-}" || "$ENGINE_RUN_SHA" == "$PIN_FC" ]] || ENGINE_BOUND=false
DRIVE_OUT_SHA=""; [[ -f "$OUT/out/stdout" ]] && DRIVE_OUT_SHA="$(sha256sum "$OUT/out/stdout" | cut -d' ' -f1)"
WORKLOAD_EXIT=""; [[ -f "$OUT/out/exit" ]] && WORKLOAD_EXIT="$(tr -d '\n' < "$OUT/out/exit")"

if [[ "$STATUS" != "timeout" ]]; then
    if [[ "$ENGINE_BOUND" != true ]]; then
        STATUS="engine-unbound"; RC=26
    elif [[ "$DONE" != true ]]; then
        STATUS="vmm-died"; RC=21
    elif [[ "$POLICY_BOUND" != true ]]; then
        STATUS="policy-unbound"; RC=25
    elif [[ -z "$DRIVE_OUT_SHA" || "$DRIVE_OUT_SHA" != "$SERIAL_OUT_SHA" || "$WORKLOAD_EXIT" != "$SERIAL_EXIT" ]]; then
        STATUS="output-unbound"; RC=23
    elif [[ "$WORKLOAD_EXIT" == 0 ]]; then
        STATUS="ok"; RC=0
    else
        STATUS="workload-failed"; RC=10
    fi
fi

python3 - "$OUT" <<PY
import json, os, hashlib, sys
out = sys.argv[1]
def sha(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest()
outputs = {}
od = os.path.join(out, "out")
for root, _, files in os.walk(od):
    for n in files:
        p = os.path.join(root, n)
        outputs[os.path.relpath(p, od)] = {"sha256": sha(p), "bytes": os.path.getsize(p)}
r = {
  "schema": "axon-linux-microvm-result/1",
  "id": "$ID", "status": "$STATUS",
  "program_sha256": "$PROG_SHA",
  "admissible": "$STATUS" in ("ok", "workload-failed"),
  "test_hooks": json.loads('$TEST_HOOKS_JSON'),
  "boot_args": "$BOOT_ARGS",
  "cmdline_bytes": $CMDLINE_BYTES,
  "policy_sha256": "$POLICY_SHA" or None,
  "policy": {"channel": "kernel cmdline, one word axon.policy=<base64>",
             "sha256": "$POLICY_SHA" or None, "bytes": $POLICY_BYTES,
             "serial_report": "$SERIAL_POLICY" or None,
             "serial_sha256": "$SERIAL_POLICY_SHA" or None,
             "bound": "$POLICY_BOUND" == "true"},
  "engine": {"firecracker_pinned": "$PIN_FC", "jailer_pinned": "$PIN_JAILER",
             "firecracker_chroot_copy_sha256": "$ENGINE_CHROOT_SHA" or None,
             "firecracker_running_exe_sha256": "${ENGINE_RUN_SHA:-}" or None,
             "bound": "$ENGINE_BOUND" == "true"},
  "jail": {"uid": $PUID, "gid": $PGID, "chroot_base": "$CHROOT_BASE", "cgroup_parent": "$CG_PARENT",
           "netns": "empty (lo only)", "network_interfaces": 0, "mmds": "not configured (policy rides the kernel cmdline)",
           "api_socket": "none (--no-api)", "seccomp": "firecracker default filter"},
  "artifacts": {"vmlinux": "$GOT_KERNEL", "rootfs.sqfs": "$GOT_ROOTFS", "axon_pinned": "$PIN_AXON"},
  "limits": {"vcpus": $VCPUS, "mem_mib": $MEM_MIB, "cg_memory_max": "$CG_MEM_MAX",
             "cg_pids_max": "$CG_PIDS_MAX", "cg_cpu_max": "$CG_CPU_MAX",
             "workspace_mib": $WS_MIB, "timeout_s": $TIMEOUT_S},
  "workspace_in_sha256": "$WS_IN_SHA",
  "rootfs_after_run_sha256": "$ROOTFS_AFTER_SHA",
  "workspace_out_sha256": sha(os.path.join(out, "workspace.img")) if os.path.exists(os.path.join(out, "workspace.img")) else None,
  "serial": {"done": "$DONE" == "true", "stdout_sha256": "$SERIAL_OUT_SHA" or None,
             "exit": "$SERIAL_EXIT" or None},
  "workload_exit": int("$WORKLOAD_EXIT") if "$WORKLOAD_EXIT".isdigit() else None,
  "vmm_exit": ${VMM_EXIT_REAL:-None},
  "outputs": outputs,
  "output_bound": "$DRIVE_OUT_SHA" != "" and "$DRIVE_OUT_SHA" == "$SERIAL_OUT_SHA",
  "wall_ms": ($END_NS - $START_NS) // 1000000,
  "host_observed": json.loads(open(os.path.join(out, "host_observed.json")).read()) if os.path.exists(os.path.join(out, "host_observed.json")) else None,
}
json.dump(r, open(os.path.join(out, "result.json"), "w"), indent=2)
PY
exit "$RC"
