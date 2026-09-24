#!/usr/bin/env bash
# b263_qualify.sh — physically qualify the protected Linux microVM profile.
#
# Every assertion drives the REAL profile (scripts/fc_linux_profile.sh → jailer
# → Firecracker → pinned Linux guest) and observes the result from the HOST,
# independently of what the guest says about itself. Refusals assert ABSENCE
# OF EFFECT (no connection arrived, no token surfaced, no process/cgroup/mount
# survived), each paired with a positive control proving the probe was able to
# see the effect had it happened. Anything this host cannot test honestly is
# recorded BLOCKED with a reason — never downgraded to a weaker check.
#
# Usage:  sudo scripts/b263_qualify.sh [--evidence-dir DIR] [--keep]
# Exit:   0 all assertions PASS or BLOCKED-with-reason; 1 any FAIL.
#
# Evidence: <evidence-dir>/<UTC timestamp>.json, schema axon-b263-evidence/1.
set -uo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"
LAUNCH="$REPO/scripts/fc_linux_profile.sh"
FIX="$REPO/profiles/linux-microvm/fixtures"
EVIDENCE_DIR="/home/cklaus/projects/aicoding/axon/.axon-v022/evidence/b263"
KEEP=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --evidence-dir) EVIDENCE_DIR="$2"; shift 2 ;;
        --keep) KEEP=1; shift ;;
        *) echo "unknown arg $1" >&2; exit 2 ;;
    esac
done
[[ "$(id -u)" == 0 ]] || { echo "b263_qualify: must run as root" >&2; exit 2; }

TS="$(date -u +%Y%m%dT%H%M%SZ)"
START_ISO="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
WORK="$(mktemp -d /tmp/b263-qualify.XXXXXX)"
RESULTS="$WORK/assertions.jsonl"
: > "$RESULTS"
CG_PARENT_DIR="/sys/fs/cgroup/axon-b263"
CHROOT_BASE="/srv/axon-b263/firecracker"

say() { printf '[b263] %s\n' "$*" >&2; }

# record NAME STATUS DETAIL [GATES]
record() {
    python3 - "$RESULTS" "$1" "$2" "$3" "${4:-}" <<'PY'
import json, sys
p, name, status, detail, gates = sys.argv[1:6]
with open(p, "a") as f:
    f.write(json.dumps({"name": name, "status": status, "detail": detail,
                        "gates": [g for g in gates.split(",") if g]}) + "\n")
PY
    say "$(printf '%-44s %s' "$1" "$2")  $3"
}
jq_r() { python3 -c 'import json,sys
d=json.load(open(sys.argv[1]))
for k in sys.argv[2].split("."):
    d = d.get(k) if isinstance(d, dict) else None
print("" if d is None else (json.dumps(d) if isinstance(d,(dict,list,bool)) else d))' "$1" "$2"; }
kv() { sed -n "s/^$2=//p" "$1" | head -1 | tr -d '\r'; }

# Host-independent observation of what a launch left behind.
residue() {  # residue ID -> prints space-separated leftovers (empty = clean)
    local id="$1" left=()
    pgrep -f -- "--id $id( |$)" >/dev/null && left+=("process")
    pgrep -u axonb263 >/dev/null && left+=("uid-axonb263-process")
    [[ -e "$CG_PARENT_DIR/$id" ]] && left+=("cgroup")
    [[ -e "$CHROOT_BASE/$id" ]] && left+=("chroot")
    grep -q -- "$CHROOT_BASE/$id" /proc/mounts && left+=("mount")
    ip netns list | grep -qw -- "$id" && left+=("netns")
    echo "${left[*]:-}"
}

# ── pre-flight: host facts ────────────────────────────────────────────────────
MANIFEST="$REPO/dist/guest-linux/manifest.json"
[[ -f "$MANIFEST" ]] || { echo "build first: AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh" >&2; exit 2; }
pgrep -u axonb263 >/dev/null && { echo "stale axonb263 processes exist; reap them first" >&2; exit 2; }

HOSTIP="$(ip -4 route get 1.1.1.1 2>/dev/null | sed -n 's/.* src \([0-9.]*\).*/\1/p')"
[[ -z "$HOSTIP" ]] && HOSTIP="$(hostname -I | cut -d' ' -f1)"
rnd() { od -An -N8 -tx1 /dev/urandom | tr -d ' \n'; }
TA="B263HOST$(rnd)"; TB="$(rnd)"; HOST_TOKEN="$TA$TB"
EA="B263ENV$(rnd)"; EB="$(rnd)"; ENV_TOKEN="$EA$EB"
WA="B263WS$(rnd)"; WB="$(rnd)"; WS_TOKEN="$WA$WB"

# Host canaries: the token in places a guest would read if the host FS leaked.
CANARIES=(/root/.b263-canary "$REPO/.b263-canary" /etc/b263-canary /tmp/b263-canary "$HOME/.aws/b263-canary")
for c in "${CANARIES[@]}"; do mkdir -p "$(dirname "$c")"; echo "$HOST_TOKEN" > "$c"; done
cleanup_canaries() { for c in "${CANARIES[@]}"; do rm -f "$c"; done; }
trap 'cleanup_canaries; pkill -f "b263-listener-$TS" 2>/dev/null; [[ $KEEP == 1 ]] || rm -rf "$WORK"' EXIT

# ── (a) boot + trivial program ────────────────────────────────────────────────
say "(a) boot and run hello.ax"
A="$WORK/a"
AXON_AI_API_KEY="$ENV_TOKEN" ANTHROPIC_API_KEY="$ENV_TOKEN" \
    "$LAUNCH" --program "$FIX/hello.ax" --out "$A" --timeout-s 60 >/dev/null 2>&1; RC_A=$?
if [[ $RC_A == 0 ]] && cmp -s "$A/out/stdout" "$FIX/hello.expected"; then
    record a1_boot_runs_ax_expected_stdout PASS "exit 0; stdout == fixtures/hello.expected ($(sha256sum < "$A/out/stdout" | cut -c1-16)); wall $(jq_r "$A/result.json" wall_ms) ms" "ACF-G23,G03-r22-physical-isolation"
else
    record a1_boot_runs_ax_expected_stdout FAIL "launcher rc=$RC_A; stdout=$(head -c 200 "$A/out/stdout" 2>/dev/null | tr '\n' '|')" "ACF-G23"
fi
KREL="$(sed -n 's/.*B263-BOOT \([^ \r]*\).*/\1/p' "$A/serial.log" | head -1)"
PIN_VER="$(jq_r "$MANIFEST" kernel.version)"
if [[ "$KREL" == "$PIN_VER" ]] && grep -q "Linux version $PIN_VER" "$A/serial.log"; then
    record a2_guest_is_pinned_linux_kernel PASS "serial reports 'Linux version $KREL' (pinned $PIN_VER); not the axon-guest-kernel" "G13-r22-guest-truth,ACF-G23"
else
    record a2_guest_is_pinned_linux_kernel FAIL "guest reported '$KREL', pinned '$PIN_VER'" "ACF-G23"
fi
LOADED_AXON="$(sed -n 's/.*B263-LOADED axon=\([0-9a-f]*\) .*/\1/p' "$A/serial.log" | head -1)"
if [[ "$LOADED_AXON" == "$(jq_r "$MANIFEST" artifacts.axon.sha256)" && -n "$LOADED_AXON" ]]; then
    record a3_loaded_interpreter_matches_pin PASS "guest-measured /usr/bin/axon = manifest axon sha ${LOADED_AXON:0:16}" "ACF-G24"
else
    record a3_loaded_interpreter_matches_pin FAIL "guest measured '$LOADED_AXON'" "ACF-G24"
fi
if [[ -f "$A/out/artifact.txt" ]] && grep -q "b263-artifact fib20=6765" "$A/out/artifact.txt"; then
    record a4_workload_file_artifact_returned PASS "/work/out/artifact.txt returned via drive, content verified" "ACF-G23"
else
    record a4_workload_file_artifact_returned FAIL "artifact.txt missing or wrong" "ACF-G23"
fi

# ── host enclosure, observed from the host while (a) ran ─────────────────────
HO="$A/result.json"
UIDS="$(jq_r "$HO" host_observed.uid)"; GIDS="$(jq_r "$HO" host_observed.gid)"
PUID="$(id -u axonb263)"; PGID="$(id -g axonb263)"
if [[ "$UIDS" == "[\"$PUID\", \"$PUID\", \"$PUID\", \"$PUID\"]" && "$GIDS" == "[\"$PGID\", \"$PGID\", \"$PGID\", \"$PGID\"]" && "$PUID" != 0 ]]; then
    record h1_vmm_runs_as_dedicated_uid_gid PASS "real/eff/saved/fs uid=$PUID gid=$PGID (axonb263), CapEff=$(jq_r "$HO" host_observed.cap_eff)" "ACF-G27,G03-r22-physical-isolation"
else
    record h1_vmm_runs_as_dedicated_uid_gid FAIL "uid=$UIDS gid=$GIDS" "ACF-G27"
fi
SECC="$(python3 -c 'import json,sys; t=json.load(open(sys.argv[1]))["host_observed"]["threads"]; print(",".join(sorted({v["seccomp"] for v in t.values()})), len(t))' "$HO")"
if [[ "$(jq_r "$HO" host_observed.seccomp_main)" == 2 && "${SECC%% *}" == 2 && "$(jq_r "$HO" host_observed.no_new_privs)" == 1 ]]; then
    record h2_seccomp_filter_all_threads PASS "Seccomp=2 (filter) on every VMM thread ($(echo "$SECC" | cut -d' ' -f2) threads), NoNewPrivs=1" "ACF-G27"
else
    record h2_seccomp_filter_all_threads FAIL "seccomp states: $SECC nnp=$(jq_r "$HO" host_observed.no_new_privs)" "ACF-G27"
fi
RL="$(jq_r "$HO" host_observed.root_listing)"
if [[ "$(jq_r "$HO" host_observed.own_mntns)" == true && "$RL" != *'"home"'* && "$RL" != *'"etc"'* && "$RL" == *'"vm.json"'* ]]; then
    record h3_vmm_chrooted_own_mount_ns PASS "VMM / = jail root $RL; own mount ns" "ACF-G27"
else
    record h3_vmm_chrooted_own_mount_ns FAIL "root listing $RL" "ACF-G27"
fi
CGP="$(jq_r "$HO" host_observed.cgroup)"
CGL="$(python3 -c 'import json,sys; c=json.load(open(sys.argv[1]))["cgroup_final"]; print(" ".join(f"{k}={c[k]}" for k in ("memory.max","memory.swap.max","pids.max","cpu.max")))' "$HO" 2>/dev/null)"
if [[ "$CGP" == "0::/axon-b263/"* && "$CGL" == *"memory.swap.max=0"* && "$CGL" == *"pids.max=16"* ]]; then
    record h4_vmm_in_dedicated_cgroup_v2 PASS "$CGP; applied: $CGL" "ACF-G27"
else
    record h4_vmm_in_dedicated_cgroup_v2 FAIL "cgroup=$CGP" "ACF-G27"
fi
ENVV="$(jq_r "$HO" host_observed.environ_vars)"
if [[ "$ENVV" == "[]" ]] && ! grep -rqF -- "$ENV_TOKEN" "$A/out" "$A/serial.log"; then
    record h5_no_secrets_reach_vmm_or_guest PASS "VMM environ empty; provider keys set in the launching shell absent from VMM env, guest output and serial; MMDS not configured" "G03-r22-physical-isolation"
else
    record h5_no_secrets_reach_vmm_or_guest FAIL "VMM env vars: $ENVV" "G03-r22-physical-isolation"
fi

# ── (b)+(c) network egress + host-file visibility, from inside the guest ─────
say "(b)(c) adversarial probe"
PORT=$(( 20000 + RANDOM % 20000 ))
CONN_LOG="$WORK/listener.log"; : > "$CONN_LOG"
python3 - "$PORT" "$CONN_LOG" "b263-listener-$TS" <<'PY' &
import socket, sys, threading, time
port, log = int(sys.argv[1]), sys.argv[2]
s = socket.socket(); s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
s.bind(("0.0.0.0", port)); s.listen(16); s.settimeout(1.0)
end = time.time() + 240
while time.time() < end:
    try:
        c, a = s.accept()
        with open(log, "a") as f: f.write(f"{a[0]}\n")
        c.close()
    except socket.timeout:
        pass
PY
LISTENER=$!
sleep 0.5
# positive control for the listener: the host itself can reach it
python3 -c 'import socket,sys; socket.create_connection((sys.argv[1], int(sys.argv[2])), 2).close()' "$HOSTIP" "$PORT" 2>/dev/null
sleep 0.3
CTL_CONN=$(wc -l < "$CONN_LOG")
: > "$CONN_LOG"
sed -e "s/@TA@/$TA/; s/@TB@/$TB/; s/@EA@/$EA/; s/@EB@/$EB/; s/@WA@/$WA/; s/@WB@/$WB/; s/@HOSTIP@/$HOSTIP/; s/@PORT@/$PORT/" \
    "$FIX/probe.sh.in" > "$WORK/probe.sh"
echo "$WS_TOKEN" > "$WORK/ws_token.txt"
B="$WORK/b"
AXON_AI_API_KEY="$ENV_TOKEN" ANTHROPIC_API_KEY="$ENV_TOKEN" OPENAI_API_KEY="$ENV_TOKEN" \
    "$LAUNCH" --program "$FIX/probe.ax" --out "$B" --timeout-s 120 \
    --put "$WORK/probe.sh:job/probe.sh" --put "$WORK/ws_token.txt:job/ws_token.txt" >/dev/null 2>&1; RC_B=$?
sleep 1
GUEST_CONN=$(wc -l < "$CONN_LOG")
kill "$LISTENER" 2>/dev/null; wait "$LISTENER" 2>/dev/null
P="$B/out/probe.txt"
if ! grep -q PROBE-END "$P" 2>/dev/null; then
    record b0_probe_completed FAIL "launcher rc=$RC_B; probe did not finish: $(tail -c 300 "$P" 2>/dev/null | tr '\n' '|')" ""
else
    record b0_probe_completed PASS "probe ran to PROBE-END as a native descendant of axon (exec); launcher rc=$RC_B" ""
fi
ND="$(kv "$P" netdevs)"
if [[ "$ND" == "lo," && "$(jq_r "$B/result.json" host_observed.vmm_netns_devices)" == '["lo"]' ]]; then
    record b1_no_guest_nic_no_vmm_route PASS "guest /sys/class/net = {lo}; VMM netns devices = {lo}; network-interfaces=[]" "ACF-G28,G03-r22-physical-isolation"
else
    record b1_no_guest_nic_no_vmm_route FAIL "guest netdevs=$ND vmm=$(jq_r "$B/result.json" host_observed.vmm_netns_devices)" "ACF-G28"
fi
CTLW="$(kv "$P" ctl_wget_loopback_rc)"; CTLN="$(kv "$P" ctl_nc_loopback_body)"
if [[ "$CTLW" == "0 body=lo-http-ok" && "$CTLN" == "lo-http-ok" && "$CTL_CONN" -ge 1 ]]; then
    record b2_positive_controls_tools_and_listener_work PASS "guest wget and raw nc both fetch the loopback httpd body (same tools, same code paths used for the egress probes); host listener on $HOSTIP:$PORT accepted the host's own connection" "ACF-G28"
else
    record b2_positive_controls_tools_and_listener_work FAIL "wget_lo='$CTLW' nc_lo='$CTLN' host_ctl_conns=$CTL_CONN" "ACF-G28"
fi
EW="$(kv "$P" egress_wget_host_rc)"; EN="$(kv "$P" egress_nc_host_rc)"
if [[ -n "$EW" && "$EW" != 0 && -n "$EN" && "$EN" != 0 && "$GUEST_CONN" == 0 ]]; then
    record b3_egress_to_host_ip_absent PASS "guest wget rc=$EW, nc rc=$EN to $HOSTIP:$PORT; host listener observed 0 connections during the guest run" "ACF-G28,G03-r22-physical-isolation"
else
    record b3_egress_to_host_ip_absent FAIL "wget rc=$EW nc rc=$EN; host saw $GUEST_CONN connection(s)" "ACF-G28"
fi
EP="$(kv "$P" egress_wget_public_rc)"; EM="$(kv "$P" egress_metadata_rc)"; ED="$(kv "$P" egress_dns_rc)"; EI="$(kv "$P" egress_ping_rc)"; NB="$(kv "$P" nic_bringup_rc)"
if [[ -n "$EP$EM$ED$EI$NB" && "$EP" != 0 && "$EM" != 0 && "$ED" != 0 && "$EI" != 0 && "$NB" != 0 ]]; then
    record b4_egress_public_metadata_dns_icmp_refused PASS "1.1.1.1 rc=$EP, 169.254.169.254 rc=$EM, DNS rc=$ED, ICMP 8.8.8.8 rc=$EI, bring-up eth0 rc=$NB" "ACF-G28"
else
    record b4_egress_public_metadata_dns_icmp_refused FAIL "public=$EP metadata=$EM dns=$ED icmp=$EI eth0=$NB" "ACF-G28"
fi
HF="$(kv "$P" host_token_file_hits)"; HB="$(kv "$P" host_token_blockdev_hits)"; WF="$(kv "$P" ws_token_file_hits)"
if [[ "$WF" -ge 1 && "$HF" == 0 && "$HB" == 0 ]]; then
    record c1_host_files_invisible_to_guest PASS "host canary token in ${#CANARIES[@]} host paths (incl. /root, /etc, repo, ~/.aws): 0 file hits, 0 raw-block hits in guest; control token on workspace drive found ($WF hit)" "G03-r22-physical-isolation,ACF-G27"
else
    record c1_host_files_invisible_to_guest FAIL "host_file_hits=$HF blockdev_hits=$HB ws_control_hits=$WF" "G03-r22-physical-isolation"
fi
BD="$(kv "$P" blockdevs)"; M9="$(kv "$P" mount_9p_rc)"; MV="$(kv "$P" mount_virtiofs_rc)"
if [[ "$BD" == "vda,vdb," && "$M9" != 0 && "$MV" != 0 ]]; then
    record c2_only_declared_drives_no_host_share PASS "guest block devices = {vda (ro rootfs), vdb (workspace)}; 9p rc=$M9, virtiofs rc=$MV" "G03-r22-physical-isolation"
else
    record c2_only_declared_drives_no_host_share FAIL "blockdevs=$BD 9p=$M9 virtiofs=$MV" "G03-r22-physical-isolation"
fi
EH="$(kv "$P" env_token_hits)"
if [[ "$EH" == 0 ]] && ! grep -qF -- "$ENV_TOKEN" "$B/serial.log"; then
    record c3_provider_secret_absent_in_guest PASS "token exported as AXON_AI_API_KEY/ANTHROPIC_API_KEY/OPENAI_API_KEY in launcher env: 0 hits in guest env, /proc/cmdline, PID1 environ, serial; workload env = $(kv "$P" workload_env_names)" "G03-r22-physical-isolation"
else
    record c3_provider_secret_absent_in_guest FAIL "env_token_hits=$EH" "G03-r22-physical-isolation"
fi
RW="$(kv "$P" rootfs_write_rc)"; RR="$(kv "$P" rootfs_remount_rw_rc)"; RD="$(kv "$P" rootfs_rawdev_write_rc)"
PIN_RFS="$(jq_r "$MANIFEST" artifacts.rootfs\.sqfs.sha256)"
AFTER_RFS="$(jq_r "$B/result.json" rootfs_after_run_sha256)"
[[ -z "$PIN_RFS" ]] && PIN_RFS="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["artifacts"]["rootfs.sqfs"]["sha256"])' "$MANIFEST")"
# NOTE (measured in the first real run): `mount -o remount,rw /` SUCCEEDS in
# the guest (rc 0) — the guest is root in its own kernel and may flip a mount
# flag. That is not a write. What must hold is the EFFECT: writes still fail
# after the remount (squashfs has no write path), the virtio device is
# read-only (is_read_only -> /sys/block/vda/ro = 1, raw writes EPERM), and the
# host image is byte-identical afterwards.
RO="$(kv "$P" rootfs_blockdev_ro_flag)"; RW2="$(kv "$P" rootfs_write_after_remount_rc)"
RW3="$(kv "$P" rootfs_write_bin_after_remount_rc)"
if [[ -n "$RW$RW2$RW3$RD" && "$RW" != 0 && "$RW2" != 0 && "$RW3" != 0 && "$RD" != 0 && "$RO" == 1 && -n "$PIN_RFS" && "$AFTER_RFS" == "$PIN_RFS" ]]; then
    record c4_rootfs_read_only_unchanged PASS "vda ro flag=$RO; guest writes fail before (rc=$RW) and after an in-guest remount,rw (remount rc=$RR, mode then '$(kv "$P" rootfs_mode_after_remount)'; writes rc=$RW2/$RW3); raw /dev/vda write rc=$RD; host rootfs image sha after run == pin ${PIN_RFS:0:16}" "ACF-G24"
else
    record c4_rootfs_read_only_unchanged FAIL "ro=$RO write=$RW remount=$RR write_after=$RW2/$RW3 raw=$RD after=$AFTER_RFS pin=$PIN_RFS" "ACF-G24"
fi
GPE="$(kv "$P" guest_pids_events_max)"; GPC="$(kv "$P" guest_pids_current_at_peak)"
if [[ -n "$GPE" && "$GPE" -gt 0 && -n "$GPC" && "$GPC" -le 32 ]]; then
    record d3_guest_pids_bound_enforced PASS "100 background children attempted; guest pids.max=32 refused $GPE forks, pids.current=$GPC" "G03-r22-physical-isolation"
else
    record d3_guest_pids_bound_enforced FAIL "pids.events max=$GPE current=$GPC" ""
fi

# ── (d) host memory limit on the VMM ─────────────────────────────────────────
# The guest is given 512 MiB of RAM but the VMM's host cgroup only 160 MiB, so
# touching 300 MiB inside the guest can only be satisfied by the host charging
# the VMM beyond memory.max -> the host OOM-kills the VMM.
say "(d) memory: host cgroup limit below guest RAM"
echo 300 > "$WORK/mb300"
D1="$WORK/d1"
"$LAUNCH" --program "$FIX/mem.ax" --out "$D1" --mem-mib 512 --cg-mem-max $((160*1024*1024)) \
    --put "$WORK/mb300:job/mb.txt" --timeout-s 90 >/dev/null 2>&1; RC_D1=$?
OOMK="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["cgroup_final"]["memory.events"]["oom_kill"])' "$D1/result.json" 2>/dev/null)"
PEAK="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["cgroup_final"]["memory.peak"])' "$D1/result.json" 2>/dev/null)"
TOUCHED_D1="$(grep -c MEM-TOUCHED "$D1/out/stdout" 2>/dev/null)"
# positive control: same program, 16 MiB, same limits -> completes
echo 16 > "$WORK/mb16"
D0="$WORK/d0"
"$LAUNCH" --program "$FIX/mem.ax" --out "$D0" --mem-mib 512 --cg-mem-max $((160*1024*1024)) \
    --put "$WORK/mb16:job/mb.txt" --timeout-s 90 >/dev/null 2>&1; RC_D0=$?
if [[ $RC_D0 == 0 ]] && grep -q "MEM-TOUCHED 16 MiB" "$D0/out/stdout" \
   && [[ $RC_D1 == 21 && -n "$OOMK" && "$OOMK" -ge 1 && "${TOUCHED_D1:-0}" == 0 && -n "$PEAK" && "$PEAK" -le $((160*1024*1024)) ]]; then
    record d1_host_memory_limit_kills_vmm PASS "16 MiB control ok; 300 MiB run: VMM OOM-killed by host cgroup (oom_kill=$OOMK, memory.peak=$PEAK <= memory.max=167772160), no result produced, status=$(jq_r "$D1/result.json" status)" "ACF-G27,G03-r22-physical-isolation"
else
    record d1_host_memory_limit_kills_vmm FAIL "control rc=$RC_D0; over-limit rc=$RC_D1 oom_kill=$OOMK peak=$PEAK touched=$TOUCHED_D1" "ACF-G27"
fi
R_D1="$(residue "$(jq_r "$D1/result.json" id)")"
[[ -z "$R_D1" ]] && record d1b_oom_killed_run_fully_cleaned PASS "no process/cgroup/chroot/mount/netns left after host OOM kill" "G13-r22-launch-cleanup" \
                  || record d1b_oom_killed_run_fully_cleaned FAIL "left: $R_D1" "G13-r22-launch-cleanup"

# guest RAM bound: guest has 128 MiB, asks for 300 MiB -> guest-side OOM, workload fails
D2="$WORK/d2"
"$LAUNCH" --program "$FIX/mem.ax" --out "$D2" --mem-mib 128 \
    --put "$WORK/mb300:job/mb.txt" --timeout-s 90 >/dev/null 2>&1; RC_D2=$?
WX="$(jq_r "$D2/result.json" workload_exit)"
GOOM="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("oom_kills",""))' "$D2/out/guest.json" 2>/dev/null)"
if [[ $RC_D2 == 10 && -n "$WX" && "$WX" != 0 && -n "$GOOM" && "$GOOM" -ge 1 ]] && ! grep -q MEM-TOUCHED "$D2/out/stdout"; then
    record d2_guest_ram_bound_enforced PASS "128 MiB guest, 300 MiB request: workload OOM-killed by the guest cgroup (exit $WX, memory.oom_kill=$GOOM), no MEM-TOUCHED; PID 1 survived and reported; VMM exited cleanly" "G03-r22-physical-isolation"
else
    record d2_guest_ram_bound_enforced FAIL "rc=$RC_D2 workload_exit=$WX oom=$GOOM" ""
fi

# host pids bound on the VMM: pids.max below what Firecracker needs -> VMM cannot start its vcpu thread
say "(d) pids: host cgroup pids.max"
D4="$WORK/d4"
"$LAUNCH" --program "$FIX/hello.ax" --out "$D4" --cg-pids-max 1 --timeout-s 30 >/dev/null 2>&1; RC_D4=$?
PEV="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["cgroup_final"]["pids.events"]["max"])' "$D4/result.json" 2>/dev/null)"
if [[ $RC_D4 != 0 && -n "$PEV" && "$PEV" -ge 1 ]] && ! grep -q "B263-START" "$D4/serial.log"; then
    record d4_host_pids_limit_enforced_on_vmm PASS "pids.max=1: the jailed VMM's thread creation refused by the host cgroup (pids.events max=$PEV), guest never started (rc=$RC_D4); pids.max=16 control = run (a) (pids.peak=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["cgroup_final"]["pids.peak"])' "$A/result.json"))" "ACF-G27"
else
    record d4_host_pids_limit_enforced_on_vmm FAIL "rc=$RC_D4 pids.events.max=$PEV" "ACF-G27"
fi

# cpu: cpu.max applied and throttling observed on a busy guest (from (e) below)

# ── (e) wall-time kill + full cleanup ────────────────────────────────────────
say "(e) wall-clock kill of a looping guest"
E="$WORK/e"
T0=$(date +%s%N)
"$LAUNCH" --program "$FIX/loop.ax" --out "$E" --timeout-s 8 --cg-cpu-max "50000 100000" >/dev/null 2>&1; RC_E=$?
T1=$(date +%s%N)
EL=$(( (T1 - T0) / 1000000 ))
EID="$(jq_r "$E/result.json" id)"
R_E="$(residue "$EID")"
EVMM="$(jq_r "$E/launch.json" vmm_pid)"
if [[ $RC_E == 20 && "$(jq_r "$E/result.json" status)" == timeout ]] && grep -q "B263-START" "$E/serial.log" && ! grep -q "B263-DONE" "$E/serial.log" \
   && [[ "$(jq_r "$E/result.json" wall_ms)" -ge 8000 && $EL -lt 20000 ]]; then
    record e1_looping_guest_killed_at_timeout PASS "guest started, never finished; killed at wall $(jq_r "$E/result.json" wall_ms) ms (limit 8000), launcher returned in $EL ms, status=timeout" "G13-r22-launch-cleanup"
else
    record e1_looping_guest_killed_at_timeout FAIL "rc=$RC_E status=$(jq_r "$E/result.json" status) wall=$(jq_r "$E/result.json" wall_ms) elapsed=$EL" "G13-r22-launch-cleanup"
fi
if [[ -z "$R_E" ]] && ! kill -0 "$EVMM" 2>/dev/null; then
    record e2_timeout_cleanup_complete PASS "VMM pid $EVMM gone; no process, cgroup, chroot, mount or netns for $EID (host re-observed)" "G13-r22-launch-cleanup"
else
    record e2_timeout_cleanup_complete FAIL "left: $R_E" "G13-r22-launch-cleanup"
fi
THR="$(python3 -c 'import json,sys; c=json.load(open(sys.argv[1]))["cgroup_final"]["cpu.stat"]; print(c.get("nr_throttled","0"), c.get("usage_usec","0"))' "$E/result.json" 2>/dev/null)"
if [[ -n "$THR" && "${THR%% *}" -gt 0 ]]; then
    USE="${THR##* }"; WALLU=$(( $(jq_r "$E/result.json" wall_ms) * 1000 ))
    record d5_host_cpu_limit_enforced PASS "cpu.max=50000/100000: nr_throttled=${THR%% *}, usage ${USE}us over ${WALLU}us wall (~$(( USE * 100 / WALLU ))% of one CPU)" "ACF-G27"
else
    record d5_host_cpu_limit_enforced FAIL "cpu.stat=$THR" "ACF-G27"
fi

# ── (f) crash: SIGKILL the VMM mid-run ───────────────────────────────────────
say "(f) SIGKILL firecracker mid-run"
F="$WORK/f"
"$LAUNCH" --program "$FIX/loop.ax" --out "$F" --timeout-s 60 >/dev/null 2>&1 &
LPID=$!
for _ in $(seq 100); do [[ -f "$F/launch.json" ]] && grep -q B263-START "$F/serial.log" 2>/dev/null && break; sleep 0.2; done
FPID="$(jq_r "$F/launch.json" vmm_pid)"
FID="$(jq_r "$F/launch.json" id)"
FEXE="$(readlink /proc/"$FPID"/exe 2>/dev/null)"
kill -9 "$FPID" 2>/dev/null
wait "$LPID"; RC_F=$?
R_F="$(residue "$FID")"
if [[ "$FEXE" == */firecracker* && $RC_F == 21 && "$(jq_r "$F/result.json" status)" == vmm-died && -z "$R_F" ]]; then
    record f1_vmm_sigkill_detected_and_cleaned PASS "SIGKILL to running firecracker pid $FPID: launcher reported vmm-died (rc 21, no result admitted); no residue for $FID" "G13-r22-launch-cleanup"
else
    record f1_vmm_sigkill_detected_and_cleaned FAIL "exe=$FEXE rc=$RC_F status=$(jq_r "$F/result.json" status) left: $R_F" "G13-r22-launch-cleanup"
fi

# supervisor itself killed: SIGKILL the launcher mid-run -> resources orphaned -> --reap
say "(f) SIGKILL the launcher (supervisor death) then reap"
F2="$WORK/f2"
"$LAUNCH" --program "$FIX/loop.ax" --out "$F2" --timeout-s 60 >/dev/null 2>&1 &
LPID2=$!
for _ in $(seq 100); do [[ -f "$F2/launch.json" ]] && grep -q B263-START "$F2/serial.log" 2>/dev/null && break; sleep 0.2; done
F2ID="$(jq_r "$F2/launch.json" id)"
kill -9 "$LPID2"; wait "$LPID2" 2>/dev/null
sleep 0.5
BEFORE="$(residue "$F2ID")"
"$LAUNCH" --reap "$F2ID" >/dev/null 2>&1; RC_R=$?
AFTER="$(residue "$F2ID")"
if [[ -n "$BEFORE" && $RC_R == 0 && -z "$AFTER" ]]; then
    record f2_supervisor_death_orphans_reaped PASS "launcher SIGKILLed: orphaned [$BEFORE] observed (VMM kept running -> cancel is not cleanup); --reap $F2ID removed all, re-observed clean" "G13-r22-launch-cleanup"
else
    record f2_supervisor_death_orphans_reaped FAIL "before=[$BEFORE] reap rc=$RC_R after=[$AFTER]" "G13-r22-launch-cleanup"
fi

# injected failures after each acquisition point
INJ_FAIL=""
for pt in after-chroot after-netns after-launch; do
    FI="$WORK/inj-$pt"
    FC_PROFILE_INJECT_FAIL=$pt "$LAUNCH" --program "$FIX/loop.ax" --out "$FI" --timeout-s 30 >/dev/null 2>&1; rc=$?
    iid="$(jq_r "$FI/result.json" id)"
    acq="$(python3 -c 'import json,sys; print(len(json.load(open(sys.argv[1]))["cleanup"]["acquired"]))' "$FI/result.json" 2>/dev/null)"
    left="$(residue "$iid")"
    if [[ $rc == 21 && -n "$iid" && -z "$left" && "${acq:-0}" -ge 1 ]]; then
        INJ_FAIL="$INJ_FAIL $pt:ok(acq=$acq)"
    else
        INJ_FAIL="$INJ_FAIL $pt:BAD(rc=$rc,left=$left)"
    fi
done
if [[ "$INJ_FAIL" != *BAD* ]]; then
    record f3_injected_failure_after_each_acquisition PASS "failure injected after chroot / netns / VMM launch: every acquired resource released, re-observed clean:$INJ_FAIL" "G13-r22-launch-cleanup"
else
    record f3_injected_failure_after_each_acquisition FAIL "$INJ_FAIL" "G13-r22-launch-cleanup"
fi

# ── (g) output drive returned and sha256-bound ───────────────────────────────
say "(g) output binding"
if "$LAUNCH" --verify-result "$A" >/dev/null 2>&1; then
    record g1_output_drive_sha256_bound PASS "run (a): stdout re-extracted from returned workspace.img == guest serial digest == result.json ($(jq_r "$A/result.json" outputs.stdout.sha256 | cut -c1-16)); workspace_out_sha256=$(jq_r "$A/result.json" workspace_out_sha256 | cut -c1-16)" "G13-r22-profile-qualification,ACF-G23"
else
    record g1_output_drive_sha256_bound FAIL "verify-result failed on run (a)" "ACF-G23"
fi
# tamper: flip the returned drive's stdout and re-verify -> must refuse
G="$WORK/g"; cp -r "$A" "$G"
debugfs -w -R "rm /out/stdout" "$G/workspace.img" >/dev/null 2>&1
printf 'forged output\n' > "$WORK/forged"
debugfs -w -R "write $WORK/forged /out/stdout" "$G/workspace.img" >/dev/null 2>&1
"$LAUNCH" --verify-result "$G" >/dev/null 2>&1; RC_G=$?
FORGED_IN="$(debugfs -R 'cat /out/stdout' "$G/workspace.img" 2>/dev/null)"
if [[ $RC_G == 23 && "$FORGED_IN" == "forged output" ]]; then
    record g2_tampered_output_refused PASS "returned drive's /out/stdout replaced after the run: binding check refused (rc 23)" "G13-r22-profile-qualification"
else
    record g2_tampered_output_refused FAIL "rc=$RC_G forged_present='$FORGED_IN'" "G13-r22-profile-qualification"
fi
# artifact swap: rootfs differing from the pin must refuse before acquiring anything
SW="$WORK/swap"; mkdir -p "$SW"
cp "$REPO/dist/guest-linux/vmlinux" "$SW/vmlinux"; cp "$REPO/dist/guest-linux/rootfs.sqfs" "$SW/rootfs.sqfs"
printf 'X' | dd of="$SW/rootfs.sqfs" bs=1 seek=4096 conv=notrunc 2>/dev/null
G3="$WORK/g3"
"$LAUNCH" --program "$FIX/hello.ax" --out "$G3" --artifacts-dir "$SW" --timeout-s 30 >/dev/null 2>&1; RC_G3=$?
if [[ $RC_G3 == 22 && "$(jq_r "$G3/result.json" status)" == launch-refused && ! -f "$G3/serial.log" && "$(jq_r "$G3/result.json" acquired)" == "[]" ]]; then
    record g3_swapped_rootfs_refused_before_launch PASS "1-byte-modified rootfs: refused (rc 22), no VMM started, nothing acquired" "ACF-G24"
else
    record g3_swapped_rootfs_refused_before_launch FAIL "rc=$RC_G3 status=$(jq_r "$G3/result.json" status)" "ACF-G24"
fi

# ── BLOCKED: required by the gates, not testable honestly here ───────────────
record x1_guest_policy_channel BLOCKED "ACF-G25 needs a boot-policy protocol (absent/empty/malformed/mismatched policy fails closed). This profile deliberately has none (offline, no MMDS; axon-guest-init not included); defining one belongs to the axon-vm/Fabric owner. Not substituted." "ACF-G25"
record x2_scope_preservation BLOCKED "ACF-G26 needs a path/host-specific grant projected into VM policy; depends on the same missing policy channel. The profile is all-or-nothing (workspace rw, no net). Not substituted." "ACF-G26"
record x3_l0_hypervisor_boundary BLOCKED "Host is WSL2 with nested KVM under Hyper-V (operator decision D2). The L0 hypervisor and the WSL2 utility VM are outside the qualified boundary; no assertion here covers a guest escape through L0/L1." "G03-r22-physical-isolation"
record x4_trusted_evidence_issuer BLOCKED "G13-r22-profile-qualification requires binding to a TRUSTED evidence issuer. This evidence is produced and signed by nobody but the invoking root shell; no issuer key exists in this repo. The record is unsigned." "G13-r22-profile-qualification"

# ── evidence ──────────────────────────────────────────────────────────────────
END_ISO="$(date -u +%Y-%m-%dT%H:%M:%SZ)"
mkdir -p "$EVIDENCE_DIR"
EVF="$EVIDENCE_DIR/$TS.json"
python3 - "$RESULTS" "$EVF" "$MANIFEST" "$START_ISO" "$END_ISO" "$REPO" "$0 $*" "$A/result.json" <<'PY'
import json, os, subprocess, sys, hashlib, platform
res_p, evf, man_p, start, end, repo, cmd, a_res = sys.argv[1:9]
rows = [json.loads(l) for l in open(res_p)]
def run(c):
    try: return subprocess.run(c, capture_output=True, text=True, cwd=repo).stdout.strip()
    except OSError: return None
def sha(p): return hashlib.sha256(open(p, "rb").read()).hexdigest()
man = json.load(open(man_p))
counts = {s: sum(1 for r in rows if r["status"] == s) for s in ("PASS", "FAIL", "BLOCKED")}
cgc = open("/sys/fs/cgroup/cgroup.controllers").read().split()
ev = {
  "schema": "axon-b263-evidence/1",
  "work_package": "B263", "fabric_task": "ACF-T06",
  "host": "WSL2-nested",
  "caveat": "Nested virtualization under Hyper-V; the L0 hypervisor is outside the qualified boundary. (operator decision D2, .axon-v022/coordination/operator_decisions.json)",
  "host_facts": {
    "uname": run(["uname", "-a"]), "kernel": platform.release(),
    "kvm": {"dev_kvm": os.path.exists("/dev/kvm"),
            "nested": run(["sh", "-c", "cat /sys/module/kvm_intel/parameters/nested /sys/module/kvm_amd/parameters/nested 2>/dev/null"]) or "unreadable"},
    "cpu": run(["sh", "-c", "grep -m1 'model name' /proc/cpuinfo | cut -d: -f2"]),
    "cgroup": {"version": 2, "root_controllers": cgc},
    "os": run(["sh", "-c", ". /etc/os-release; echo $PRETTY_NAME"]),
  },
  "invoker": {"uid": os.getuid(), "user": run(["id", "-un"]), "authenticated": "local root shell only (no authenticated invoker identity)"},
  "command": cmd.strip(),
  "source": {"axon_git_rev": run(["git", "rev-parse", "HEAD"]),
             "tree_dirty": bool(run(["git", "status", "--porcelain", "--untracked-files=no"])),
             "harness_sha256": sha(os.path.join(repo, "scripts/b263_qualify.sh")),
             "launcher_sha256": sha(os.path.join(repo, "scripts/fc_linux_profile.sh")),
             "guest_init_sha256": sha(os.path.join(repo, "profiles/linux-microvm/guest-init.sh"))},
  "engine": {"firecracker": run(["/usr/local/bin/firecracker", "--version"]).splitlines()[0],
             "firecracker_sha256": sha("/usr/local/bin/firecracker"),
             "jailer": run(["/usr/local/bin/jailer", "--version"]).splitlines()[0],
             "jailer_sha256": sha("/usr/local/bin/jailer")},
  "profile": {"name": man["profile"], "manifest_sha256": sha(man_p),
              "kernel": man["kernel"], "busybox": man["busybox"],
              "artifacts": man["artifacts"],
              "boot_args": json.load(open(a_res)).get("boot_args"),
              "jail": json.load(open(a_res)).get("jail")},
  "fixture_labeling": "all runs are REAL launches of the profile; no mock or fixture result is counted",
  "assertions": rows,
  "counts": {"total": len(rows), **counts},
  "result": "FAIL" if counts["FAIL"] else ("PASS_WITH_BLOCKED" if counts["BLOCKED"] else "PASS"),
  "start": start, "end": end,
}
json.dump(ev, open(evf, "w"), indent=2)
print(evf)
print(json.dumps(ev["counts"]))
PY
say "evidence: $EVF"
[[ $KEEP == 1 ]] && say "work dir kept: $WORK"
grep -q '"status": "FAIL"' "$RESULTS" && exit 1
exit 0
