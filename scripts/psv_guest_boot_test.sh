#!/usr/bin/env bash
# psv_guest_boot_test.sh — the PSV guest path in a REAL Firecracker guest
# (development host; proves the mechanism, certifies nothing).
#
# It boots the pinned Linux guest built by build-guest-image.sh (whose axon
# descends from PCI 31413ca7) through fc_linux_profile.sh's PSV mode, with the
# candidate, the suite and the job on three separate read-only drives. Cases:
#   pass         the named test passes; the verdict is on the returned drive,
#                bound to /init's serial digest; the completion token verifies
#                under the host-derived key
#   custody      the child sees EOF on stdin and gets Permission denied on the
#                secret file (it runs as nobody in the guest)
#   seal         a candidate that reads the suite's answer is refused (E0004)
#   candidate    the candidate changes after the job is made: the guest refuses
#                and nothing runs
#   tampered     the returned drive's verdict is replaced: --verify-result 27
#   helper       the pass case through the setuid privileged helper, as a
#                non-root Fabric uid (amendment 45)
#   mounts       guest-init.sh's mounts are IN EFFECT in the guest, as the test
#                child sees them: /in/{candidate,suite,job} ro,nodev,nosuid,
#                noexec; /work nodev,nosuid
#                (C9 round 2: rows M493-M496 pin only the script's text)
#   trust-probe  amendment 65: scripts/trust_root_guest_probe.sh (the trust
#                preflight's guest check, until now run only through an
#                operator-supplied --guest-cmd) runs INSIDE the real guest (the
#                launcher's plain mode under an Exec grant; the PSV runner strips
#                Exec from test children): it reports the operator trust root
#                unaddressable there, and (control) addressable for a path the
#                guest does have (/work)
#   policy-*     PSV-6 (C9 round 4, A87): the guest runs only the policy the
#                launch manifest names. The launcher refuses a --policy the
#                manifest does not name (nothing acquired); a guest booted
#                under another policy word refuses in its runner; the helper
#                refuses one before spending the nonce
#
# Exit 0 all PASS; 1 any FAIL; 77 SKIP (not root / no KVM / no image). A SKIP is
# a non-result and is reported as such, never as a pass.
set -uo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
LAUNCH="$REPO/scripts/fc_linux_profile.sh"
POLICY="$REPO/profiles/linux-microvm/fixtures/policy-io.json"
skip() { echo "SKIP: $*"; exit 77; }
[[ "$(id -u)" == 0 ]] || skip "needs root"
[[ -c /dev/kvm ]] || skip "no /dev/kvm"
[[ -f "$REPO/dist/guest-linux/manifest.json" && -f "$REPO/dist/guest-linux/rootfs.sqfs" ]] \
    || skip "no built guest image (AXON_KERNEL_BACKEND=linux scripts/build-guest-image.sh)"
python3 - "$REPO/dist/guest-linux/manifest.json" <<'PY' || skip "the built image does not pin axon-psv-runner (rebuild it)"
import json, sys
sys.exit(0 if "axon-psv-runner" in json.load(open(sys.argv[1]))["artifacts"] else 1)
PY
# The host-side judge is BUILT here from this tree, never taken as found: a
# psv_dev left over from an earlier tree judged every verdict against an old
# schema and failed 6/11 cases while the guest was right (C9 round 1b). An
# explicit PSV_DEV is still honoured, and named in the output.
if [[ -z "${PSV_DEV:-}" ]]; then
    (cd "$REPO" && cargo build -q -p axon-psv --example psv_dev) \
        || { echo "FAIL: cargo build -p axon-psv --example psv_dev"; exit 1; }
fi
# At the path cargo built it to, not a guessed ${CARGO_TARGET_DIR:-target}
# (scripts/lib/axon_bin.sh).
. "$REPO/scripts/lib/axon_bin.sh"
DEV="${PSV_DEV:-$(cd "$REPO" && built_bin examples/psv_dev)}"
[[ -x "$DEV" ]] || { echo "FAIL: $DEV missing (cargo build -p axon-psv --example psv_dev)"; exit 1; }
echo "judge: $DEV"

W="$(mktemp -d /var/tmp/psv-boot.XXXXXX)"; chmod 0755 "$W"
# Kept on failure (or with PSV_KEEP=1) so a FAIL can be diagnosed.
trap '[[ ${FAILS:-0} == 0 && -z "${PSV_KEEP:-}" ]] && rm -rf "$W" || echo "work dir kept: $W"' EXIT
FAILS=0
ok() { echo "PASS $1"; }
bad() { echo "FAIL $1: $2"; FAILS=$((FAILS + 1)); }
jq_r() { python3 -c 'import json,sys; v=json.load(open(sys.argv[1]))
for k in sys.argv[2].split("."): v = v.get(k) if isinstance(v, dict) else None
print("" if v is None else (json.dumps(v) if isinstance(v,(dict,list,bool)) else v))' "$1" "$2" 2>/dev/null; }

mkdir -p "$W/cand" "$W/suite"
printf 'fn double(x: i64) -> i64 { x * 2 }\n' > "$W/cand/f.ax"
printf 'fn double2(x: i64) -> i64 { expected() }\n' > "$W/cand/g.ax"
cat > "$W/suite/accept.ax" <<'AX'
mod f
use f.{double}

@[test]
fn t_ok() { assert_eq(double(21), 42) }

@[test]
fn t_bad() { assert_eq(double(1), 3) }

@[test]
fn t_mounts() {
    match read_file("/proc/mounts") {
        Ok(m) => println("MOUNTS-BEGIN\n{m}MOUNTS-END")
        Err(e) => println("MOUNTS-ERR:{e}")
    }
    assert_eq(double(2), 4)
}

@[test]
fn t_custody() {
    let line = read_line()
    println("STDIN-SAW:[{line}]")
    match read_file("/in/job/completion-secret") {
        Ok(s) => println("SECRET-READ:{s}")
        Err(e) => println("SECRET-REFUSED:{e}")
    }
    assert_eq(double(2), 4)
}
AX
printf 'mod g\nuse g.{double2}\n\nfn expected() -> i64 { 42 }\n\n@[test]\nfn t_seal() { assert_eq(double2(21), expected()) }\n' > "$W/suite/seal.ax"

# run CASE ENTRY TEST [MUTATE-CMD]  → leaves $W/CASE/{job,out}
run() {
    local c="$1" entry="$2" test="$3"
    mkdir -p "$W/$c"
    cp -r "${CAND_SRC:-$W/cand}" "$W/$c/cand"; cp -r "${SUITE_SRC:-$W/suite}" "$W/$c/suite"
    # The manifest names the policy's digest (PSV-6, A87); POLICY_LAUNCH hands
    # the launcher another one.
    MSHA="$("$DEV" make-job --candidate "$W/$c/cand" --suite "$W/$c/suite" --entry "$entry" \
        --test "$test" --job "$W/$c/job" --policy "${POLICY_FOR:-$POLICY}" \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["manifest_sha256"])')"
    [[ -n "${4:-}" ]] && eval "$4"
    "$LAUNCH" --psv-candidate "$W/$c/cand" --psv-suite "$W/$c/suite" --psv-job "$W/$c/job" \
        --psv-manifest-sha "$MSHA" --policy "${POLICY_LAUNCH:-${POLICY_FOR:-$POLICY}}" \
        --out "$W/$c/out" --timeout-s 90 > "$W/$c/launch.log" 2>&1
    RC=$?
}
check() { "$DEV" check-verdict --job "$W/$1/job" --verdict "$W/$1/out/out/verdict.json" 2>/dev/null; }

# pass
run pass accept.ax t_ok
V="$(check pass)"
if [[ $RC == 0 && "$(jq_r "$W/pass/out/result.json" psv.bound)" == true ]] \
   && printf '%s' "$V" | python3 -c 'import json,sys; v=json.load(sys.stdin); sys.exit(0 if v["status"]=="passed" and v["manifest_joins"] and v["policy_joins"] and v["token_verifies"] else 1)'; then
    ok "pass: the named test passed in the guest under the manifest's policy; verdict bound to serial; token verifies under the host-derived key"
else
    bad pass "rc=$RC psv=$(jq_r "$W/pass/out/result.json" psv) verdict=$V"
fi
"$LAUNCH" --verify-result "$W/pass/out" >/dev/null 2>&1 && ok "pass: --verify-result re-derives the binding" \
    || bad pass-verify "--verify-result rc=$?"

# tampered verdict on the returned drive
cp -r "$W/pass/out" "$W/tamper"
printf '{"forged":true}' > "$W/forged.json"
debugfs -w -R "rm /out/verdict.json" "$W/tamper/workspace.img" >/dev/null 2>&1
debugfs -w -R "write $W/forged.json /out/verdict.json" "$W/tamper/workspace.img" >/dev/null 2>&1
"$LAUNCH" --verify-result "$W/tamper" >/dev/null 2>&1; T=$?
[[ $T == 27 ]] && ok "tampered: a replaced verdict on the returned drive is verdict-unbound (27)" \
    || bad tampered "--verify-result rc=$T (want 27)"

# custody
run custody accept.ax t_custody
SO="$(cat "$W/custody/out/out/test-stdout" 2>/dev/null)"
if [[ $RC == 0 ]] && grep -q 'STDIN-SAW:\[\]' <<<"$SO" && grep -q 'SECRET-REFUSED:.*Permission denied' <<<"$SO" && ! grep -q 'SECRET-READ:' <<<"$SO"; then
    ok "custody: stdin at EOF; the secret file is Permission denied to the test uid"
else
    bad custody "rc=$RC stdout=$(head -c 400 <<<"$SO")"
fi

# mounts: what the kernel actually applied, read by the test child itself.
run mounts accept.ax t_mounts
SO="$(cat "$W/mounts/out/out/test-stdout" 2>/dev/null)"
MW="$(python3 - "$W/mounts/out/out/test-stdout" <<'PY'
import sys
t = open(sys.argv[1], errors="replace").read()
if "MOUNTS-BEGIN" not in t:
    print("no mount table in the test output"); sys.exit(0)
t = t.split("MOUNTS-BEGIN", 1)[1].split("MOUNTS-END", 1)[0]
opts = {}
for l in t.splitlines():
    f = l.split()
    if len(f) >= 4:
        opts[f[1]] = set(f[3].split(","))
want = {"/in/candidate": {"ro", "nodev", "nosuid", "noexec"},
        "/in/suite": {"ro", "nodev", "nosuid", "noexec"},
        "/in/job": {"ro", "nodev", "nosuid", "noexec"},
        "/work": {"nodev", "nosuid"}}
bad = [f"{m}: {sorted(w - opts.get(m, set()))} missing (have {sorted(opts.get(m, set()))})"
       for m, w in want.items() if m not in opts or not w <= opts[m]]
print("; ".join(bad))
PY
)"
if [[ $RC == 0 && -z "$MW" ]]; then
    ok "mounts: the inputs are ro,nodev,nosuid,noexec and /work nodev,nosuid in the guest"
else
    bad mounts "rc=$RC ${MW:-} stdout=$(head -c 300 <<<"$SO")"
fi

# PSV-6 (C9 round 4, A87): the manifest names policy-io; the launcher is
# handed policy-io-exec. It refuses before acquiring anything.
IOEXEC="$REPO/profiles/linux-microvm/fixtures/policy-io-exec.json"
POLICY_LAUNCH="$IOEXEC" run policy-launcher accept.ax t_ok
if [[ $RC == 22 && ! -e "$W/policy-launcher/out/out/verdict.json" ]] \
   && grep -q "not the policy_sha256" "$W/policy-launcher/launch.log"; then
    ok "policy-launcher: a --policy the launch manifest does not name is refused before anything is acquired (22)"
else
    bad policy-launcher "rc=$RC log=$(tail -c 300 "$W/policy-launcher/launch.log")"
fi
# …and a guest booted under ANOTHER policy word than the one the launcher
# validated (test hook: the launcher embeds policy-io-exec on the cmdline).
# axon-guest-init enforces that word; the runner holds it to the manifest's
# policy_sha256 and refuses before anything runs; the verdict names it.
FC_PROFILE_TEST_EMBED_POLICY="$IOEXEC" run policy-guest accept.ax t_ok
V="$(check policy-guest)"
if printf '%s' "$V" | python3 -c 'import json,sys; v=json.load(sys.stdin); sys.exit(0 if v["status"]=="refused" and "not the policy_sha256" in (v["refusal"] or "") and not v["policy_joins"] else 1)' \
   && [[ ! -e "$W/policy-guest/out/out/test-stdout" ]]; then
    ok "policy-guest: a guest booted under a policy the manifest does not name refuses in its runner (nothing ran; the verdict names the policy it was given)"
else
    bad policy-guest "rc=$RC verdict=$V"
fi

# seal
run seal seal.ax t_seal
ST="$(check seal | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])' 2>/dev/null)"
if [[ "$ST" != passed ]] && grep -q E0004 "$W/seal/out/out/test-stderr" 2>/dev/null; then
    ok "seal: a candidate reading the suite's answer is refused (E0004), status $ST"
else
    bad seal "status=$ST stderr=$(head -c 300 "$W/seal/out/out/test-stderr" 2>/dev/null)"
fi

# candidate changed after the job was made
run candidate accept.ax t_ok 'printf "fn double(x: i64) -> i64 { 42 }\n" > "$W/$c/cand/f.ax"'
V="$(check candidate)"
if printf '%s' "$V" | python3 -c 'import json,sys; v=json.load(sys.stdin); sys.exit(0 if v["status"]=="refused" and (v["refusal"] or "").startswith("candidate tree is") else 1)'; then
    ok "candidate: the guest refused a changed candidate before running anything"
else
    bad candidate "rc=$RC verdict=$V"
fi

# A22: the candidate defines a test, in the very module the suite imports,
# under the name the job registers. It is never collected, so never run and
# never a pass (review wf_d725935a-7ed, B1).
mkdir -p "$W/cand-probe"; cp -r "$W/cand/." "$W/cand-probe/"
printf '\n@[test]\nfn t_cand_probe() { println("CAND-RAN") }\n' >> "$W/cand-probe/f.ax"
CAND_SRC="$W/cand-probe" run candtest accept.ax t_cand_probe
V="$(check candtest)"
ST="$(printf '%s' "$V" | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])' 2>/dev/null)"
if [[ -n "$ST" && "$ST" != passed ]] && ! grep -q CAND-RAN "$W/candtest/out/out/test-stdout" 2>/dev/null; then
    ok "candtest: a candidate's own @[test] under the registered name never runs (status $ST)"
else
    bad candtest "status=$ST rc=$RC stdout=$(head -c 300 "$W/candtest/out/out/test-stdout" 2>/dev/null)"
fi

# A25: a failure is a verdict only with the interpreter's KEYED failure token
# (review wf_1bc28496-38e, PSV-4). Control: a genuine failure is "failed".
run fail accept.ax t_bad
ST="$(check fail | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])' 2>/dev/null)"
[[ "$ST" == failed ]] && ok "fail: a genuine failure is a keyed \"failed\" verdict" \
    || bad fail "status=$ST rc=$RC"
# The candidate prints a failure line for t_ok over its own genuine pass.
mkdir -p "$W/cand-splice"; cp -r "$W/cand/." "$W/cand-splice/"
cat > "$W/cand-splice/f.ax" <<'SPL'
fn double(x: i64) -> i64 {
    let o = chr(123)
    let c = chr(125)
    println(o + "\"name\":\"t_ok\",\"status\":\"failed\",\"duration_ms\":0,\"message\":\"forged\"" + c)
    x * 2
}
SPL
CAND_SRC="$W/cand-splice" run splice accept.ax t_ok
ST="$(check splice | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])' 2>/dev/null)"
if [[ -n "$ST" && "$ST" != failed && "$ST" != passed ]] && grep -q forged "$W/splice/out/out/test-stdout" 2>/dev/null; then
    ok "splice: a candidate's unkeyed failure line over its genuine pass is no verdict (status $ST)"
else
    bad splice "status=$ST rc=$RC"
fi

# A27: K is unreachable from candidate code even under an Exec GRANT (review
# wf_ecfcd666-6c9, PSV-3). On the candidate-4 image this exact probe recovered
# K from the axon process's memory (the recorded reproduction); now the
# key-holding process spawns nothing and is non-dumpable.
mkdir -p "$W/cand-reach"; cp -r "$W/cand/." "$W/cand-reach/"
cat > "$W/cand-reach/f.ax" <<'RCH'
fn double(x: i64) -> i64 {
    match exec("/bin/sh", ["-c", "P=$PPID; grep rw-p /proc/$P/maps | cut -d' ' -f1 | tr '-' ' ' | while read S E; do N=$(((0x$E-0x$S)/4096)); if [ $N -le 16384 ]; then dd if=/proc/$P/mem bs=4096 skip=$((0x$S/4096)) count=$N 2>/dev/null; fi; done | tr -c '0-9a-f' '\\n' | awk 'length($0)==64' | sort -u | head -20"]) {
        Ok(o) => println("REACH-OK:{o}")
        Err(e) => println("REACH-ERR:{e}")
    }
    x * 2
}
RCH
POLICY_FOR="$REPO/profiles/linux-microvm/fixtures/policy-io-exec.json" CAND_SRC="$W/cand-reach" run reach accept.ax t_ok
SO="$(cat "$W/reach/out/out/test-stdout" 2>/dev/null)"
if grep -q "requires effect .Exec." <<<"$SO" && ! grep -q "REACH-OK" <<<"$SO"; then
    ok "reach: under an Exec grant, candidate code cannot spawn a reader of K (refused: Exec)"
else
    bad reach "rc=$RC stdout=$(head -c 400 <<<"$SO")"
fi

# Amendment 65: the trust preflight's guest probe, run in the REAL guest. Not
# through the PSV runner: it strips Exec from every test child (A27), so no
# suite or candidate code may spawn there. This is the same image, kernel and
# launcher in plain mode (as B263's x1e), with an Exec grant: the probe is put
# on the workspace drive and run by busybox sh (it is POSIX sh for the guest).
mkdir -p "$W/trust-probe"
cat > "$W/trust-probe/probe.ax" <<'AX'
fn main() {
    match exec("/bin/sh", ["/work/job/trust_probe.sh", "/etc/axon/trust"]) {
        Ok(o) => print("TRUST-PROBE:{o}")
        Err(e) => println("TRUST-PROBE-ERR:{e}")
    }
    match exec("/bin/sh", ["/work/job/trust_probe.sh", "/work"]) {
        Ok(o) => print("TRUST-CONTROL:{o}")
        Err(e) => println("TRUST-CONTROL-ERR:{e}")
    }
}
AX
"$LAUNCH" --program "$W/trust-probe/probe.ax" \
    --policy "$REPO/profiles/linux-microvm/fixtures/policy-io-exec.json" \
    --put "$REPO/scripts/trust_root_guest_probe.sh:job/trust_probe.sh" \
    --out "$W/trust-probe/out" --timeout-s 90 > "$W/trust-probe/launch.log" 2>&1
RC=$?
SO="$(cat "$W/trust-probe/out/out/stdout" 2>/dev/null)"
TP="$(python3 - "$W/trust-probe/out/out/stdout" <<'PY'
import json, sys
try: t = open(sys.argv[1]).read()
except OSError: print("no stdout"); sys.exit()
def probe(tag):
    for l in t.splitlines():
        if l.startswith(tag):
            try: return json.loads(l[len(tag):])
            except ValueError: return None
    return None
p, c = probe("TRUST-PROBE:"), probe("TRUST-CONTROL:")
bad = []
if not p or p.get("schema") != "axon-trust-guest-probe/1" or p.get("root") != "/etc/axon/trust" or p.get("addressable") is not False:
    bad.append(f"probe {p}")
if not c or c.get("addressable") is not True or "mount:/work" not in (c.get("found") or ""):
    bad.append(f"control {c}")
print("; ".join(bad))
PY
)"
if [[ $RC == 0 && -z "$TP" ]]; then
    ok "trust-probe: trust_root_guest_probe.sh ran in the real guest: the operator trust root is unaddressable there; control: /work is addressable (a mount)"
else
    bad trust-probe "rc=$RC $TP stdout=$(head -c 400 <<<"$SO") log=$(tail -c 300 "$W/trust-probe/launch.log")"
fi

# A28: a suite module that exists but cannot be read (one Latin-1 byte) never
# falls through to the candidate's same-named module (review wf_293dfdb6-9d8,
# PSV-1, executed there to a keyed PASS). The operator's `want` (7) can never
# pass; the candidate's plant (42) would.
mkdir -p "$W/suite-fall" "$W/cand-fall"; cp -r "$W/cand/." "$W/cand-fall/"
printf 'mod helper\nuse helper.{want}\n\n@[test]\nfn t_helper() { assert_eq(want(), 42) }\n' > "$W/suite-fall/accept.ax"
printf 'fn want() -> i64 { 7 }\n// caf\351\n' > "$W/suite-fall/helper.ax"
printf 'fn want() -> i64 { 42 }\n' > "$W/cand-fall/helper.ax"
SUITE_SRC="$W/suite-fall" CAND_SRC="$W/cand-fall" run fall accept.ax t_helper
ST="$(check fall | python3 -c 'import json,sys; print(json.load(sys.stdin)["status"])' 2>/dev/null)"
if [[ -n "$ST" && "$ST" != passed ]] && grep -q "does not fall through" "$W/fall/out/out/test-stderr" 2>/dev/null; then
    ok "fall: an unreadable suite module is E0901, never the candidate's same-named module (status $ST)"
else
    bad fall "status=$ST rc=$RC stderr=$(head -c 300 "$W/fall/out/out/test-stderr" 2>/dev/null)"
fi

# A (amendment 45): the pass case THROUGH THE PRIVILEGED HELPER, as a non-root
# Fabric uid: the real launcher, image and engine, run by a setuid-root
# (test-trust) axon-protected-launcher from its verified descriptors, with the
# out dir handed to the launcher as /dev/fd/N and the manifest as /dev/fd/M.
# Amendment 50: the helper launches only with the launch's ONE observation,
# verified under its observer root, and spends the manifest's nonce through the
# custodian (a test-trust axon-custodian here); the same request a second time
# launches nothing.
# PSV_SKIP_HELPER=1 skips it (then it is reported, never counted as a pass).
if [[ -n "${PSV_SKIP_HELPER:-}" ]]; then
    echo "SKIP helper: PSV_SKIP_HELPER set (UNPROVEN)"
else
    FU="${PSV_HELPER_UID:-4242}"
    (cd "$REPO" && cargo build -q -p axon-fabric --features test-trust-root \
        --bin axon-protected-launcher --bin axon-custodian --bin axon-fabric) \
        || bad helper "cargo build -p axon-fabric --features test-trust-root --bin axon-protected-launcher --bin axon-custodian --bin axon-fabric"
    # The binaries cargo JUST built, at the path cargo built them to (not a
    # guessed ${CARGO_TARGET_DIR:-...}/debug: a config file naming the target
    # directory made that run whatever sat under $REPO/target; C9 round 4c).
    # Cleared first: a variable in the caller's environment never names a
    # binary this leg runs.
    pushd "$REPO" >/dev/null || bad helper "cannot enter $REPO"
    HB=""; use_built HB axon-protected-launcher
    CUB=""; use_built CUB axon-custodian
    FAB=""; use_built FAB axon-fabric
    popd >/dev/null
    H="$W/helper"; mkdir -p "$H/runs" "$H/staging" "$H/cust/nonces" "$H/observer"; chmod 0755 "$H"
    chmod 0700 "$H/cust/nonces"
    cp -r "$REPO/dist/guest-linux" "$H/dist"; chmod -R go-w "$H/dist"
    cp "$LAUNCH" "$H/fc_linux_profile.sh"; chmod 0755 "$H/fc_linux_profile.sh"
    chmod 0700 "$H/staging"; chown "$FU:$FU" "$H/runs"; chmod 0700 "$H/runs"
    # The custodian (test-trust; every role this root process: it issues the
    # nonce to this script and spends it for the helper, which is root then).
    python3 - "$H" <<'PY'
import json, sys
h = sys.argv[1]
json.dump({"schema": "axon-custodian/1", "custodian_uid": 0, "fabric_uid": 0, "launcher_uid": 0,
           "socket": f"{h}/cust/custodian.sock", "store": f"{h}/cust/nonces", "max_age_s": 300},
          open(f"{h}/cust/custodian.json", "w"))
PY
    "$CUB" --test-config "$H/cust/custodian.json" 2>"$H/cust/log" &
    CUST_PID=$!
    for _ in $(seq 100); do [[ -S "$H/cust/custodian.sock" ]] && break; sleep 0.05; done
    ask() { python3 - "$H/cust/custodian.sock" "$1" <<'PY'
import socket, sys
s = socket.socket(socket.AF_UNIX); s.connect(sys.argv[1])
s.sendall(sys.argv[2].encode() + b"\n"); s.shutdown(socket.SHUT_WR)
b = b""
while True:
    c = s.recv(4096)
    if not c: break
    b += c
print(b.decode())
PY
    }
    NONCE="$(ask '{"schema":"axon-custodian-request/1","op":"issue","epoch":0}' \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["nonce"])')"
    I="$H/runs/fab-boot.psv-inputs"; mkdir -p "$I"
    cp -r "$W/cand" "$I/candidate"; cp -r "$W/suite" "$I/check"
    MSHA="$("$DEV" make-job --candidate "$I/candidate" --suite "$I/check" --entry accept.ax \
        --test t_ok --job "$I/job" --nonce "$NONCE" --policy "$POLICY" \
        | python3 -c 'import json,sys; print(json.load(sys.stdin)["manifest_sha256"])')"
    # The guest policy travels beside the job, never in the request (A87).
    cp "$POLICY" "$I/policy.json"
    cp -r "$I/job" "$W/helper-job"   # the helper consumes Fabric's job files
    cp -r "$I" "$W/helper-inputs-again"   # …and Fabric can always rebuild them
    chown -R "$FU:$FU" "$I"
    # The observer: a key in the helper's observer root, and the observation of
    # this manifest signed with it (observer domain).
    KEYJ="$("$FAB" keygen --out "$H/obs.pk8")"
    printf '%s' "$KEYJ" | python3 -c 'import json,sys; print(json.load(sys.stdin)["public_key"])' >"$H/observer/obs.pub"
    python3 - "$I/job/launch-manifest.json" "$H/observation.json" \
        "$(printf '%s' "$KEYJ" | python3 -c 'import json,sys; print(json.load(sys.stdin)["fingerprint"])')" <<'PY'
import datetime, hashlib, json, sys
mp, out, kid = sys.argv[1:]
raw = open(mp, "rb").read(); m = json.loads(raw)
now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
json.dump({"schema": "axon-preflight-observation/1", "observer_key_id": kid,
 "nonce": m["observation_nonce"], "epoch": 0, "observed_at": now,
 "host_profile": m["backend_profile"], "fabric_revision": m["fabric_revision"],
 "firecracker_sha256": m["firecracker_sha256"], "launcher_sha256": m["launcher_sha256"],
 "host_config_sha256": m["host_config_sha256"], "guest": m["guest"],
 "verifier_sha256": m["verifier_sha256"],
 "suite_registry_sha256": m["suite"]["registry_sha256"], "policy_sha256": m["policy_sha256"],
 "intended_launch_manifest_sha256": hashlib.sha256(raw).hexdigest()}, open(out, "w"))
PY
    "$FAB" sign-evidence --record "$H/observation.json" --key "$H/obs.pk8" \
        --authority observer >/dev/null || bad helper "sign-evidence (observer)"
    python3 - "$H" "$FU" "$REPO/dist/guest-linux/manifest.json" "${POLICY}" "$MSHA" "$CUB" <<'PY'
import hashlib, json, sys
h, fu, man, pol, msha, cust = sys.argv[1:]
sha = lambda p: hashlib.sha256(open(p, "rb").read()).hexdigest()
import os
sh_real = os.path.realpath("/bin/sh")
json.dump({"schema": "axon-protected-launcher/2", "fabric_uid": int(fu),
           # Amendment 79: the helper serves only the pinned Fabric program. Here
           # the Fabric is the shell that runs the helper as the Fabric uid (a
           # child of it: see helper() below).
           "fabric": {"path": sh_real, "sha256": sha(sh_real), "revision": "0" * 40},
           "interpreter": {"path": "/bin/bash", "sha256": sha("/bin/bash")},
           "launcher": {"path": f"{h}/fc_linux_profile.sh", "sha256": sha(f"{h}/fc_linux_profile.sh")},
           "profile_manifest": {"path": f"{h}/dist/manifest.json", "sha256": sha(f"{h}/dist/manifest.json")},
           "artifacts_dir": f"{h}/dist", "firecracker": "/usr/local/bin/firecracker",
           "jailer": "/usr/local/bin/jailer", "out_root": f"{h}/runs", "staging_root": f"{h}/staging",
           "max_timeout_s": 300, "max_input_bytes": 1 << 30,
           "observer": {"root": f"{h}/observer", "max_age_s": 300, "host_signer_public_key": "0" * 64},
           # Amendment 65: the custodian PROGRAM is pinned too (checked
           # against the process that answers each spend).
           "custodian": {"socket": f"{h}/cust/custodian.sock", "uid": 0,
                         "sha256": sha(cust)}},
          open(f"{h}/protected-launcher.json", "w"))
i = f"{h}/runs/fab-boot.psv-inputs"
for name, out in (("op-boot", "request.json"), ("op-boot-again", "request-again.json"),
                  ("op-boot-policy", "request-policy.json")):
    json.dump({"schema": "axon-protected-launch-request/3", "id": "fab-boot", "out": f"{h}/runs/{name}",
               "psv_candidate": f"{i}/candidate", "psv_suite": f"{i}/check", "psv_job": f"{i}/job",
               "psv_manifest_sha256": msha, "timeout_s": 90,
               "observation": open(f"{h}/observation.json").read(),
               "observation_signature": open(f"{h}/observation.json.sig").read()},
              open(f"{h}/{out}", "w"))
PY
    chmod 0644 "$H/protected-launcher.json"
    cp "$HB" "$H/axon-protected-launcher"; chown "0:$FU" "$H/axon-protected-launcher"
    chmod 4750 "$H/axon-protected-launcher"
    # A shell running AS the Fabric uid makes the exec (setpriv's own exec
    # still holds root's DAC override) and STAYS the helper's parent (no
    # `exec`): it is the Fabric program the helper's config pins (amendment 79).
    helper() { setpriv --reuid="$FU" --regid="$FU" --clear-groups -- sh -c '"$0" --test-config "$1"; exit $?' \
        "$H/axon-protected-launcher" "$H/protected-launcher.json" <"$1" >"$2" 2>>"$H/helper.log"; }
    # A87 through the helper: the SAME genuine observation, with Fabric's
    # policy.json swapped for policy-io-exec. Refused before the spend, so the
    # pass case below still launches on this very nonce.
    cp "$IOEXEC" "$I/policy.json"; chown "$FU:$FU" "$I/policy.json"
    helper "$H/request-policy.json" "$H/report-policy.json"
    HRCP=$?
    RP="$(cat "$H/report-policy.json")"
    if [[ $HRCP == 30 && ! -e "$H/runs/op-boot-policy" ]] && grep -q "not the policy_sha256" <<<"$RP"; then
        ok "helper-policy: a genuine observation of the manifest's policy never launches another (refused before the nonce is spent)"
    else
        bad helper-policy "rc=$HRCP report=$RP"
    fi
    rm -rf "$I"; cp -r "$W/helper-inputs-again" "$I"; chown -R "$FU:$FU" "$I"
    helper "$H/request.json" "$H/report.json"
    HRC=$?
    R="$(cat "$H/report.json")"
    V="$("$DEV" check-verdict --job "$W/helper-job" --verdict "$H/runs/op-boot/out/verdict.json" 2>/dev/null)"
    OWN="$(stat -c %u "$H/runs/op-boot" 2>/dev/null)"
    if [[ $HRC == 0 ]] && printf '%s' "$R" | python3 -c 'import json,sys; r=json.load(sys.stdin); sys.exit(0 if r["launched"] and r["launcher_exit"]==0 and r["verify_exit"]==0 and r["unchanged"] and r["error"] is None else 1)' \
       && [[ "$(jq_r "$H/runs/op-boot/result.json" psv.bound)" == true && "$OWN" == "$FU" ]] \
       && printf '%s' "$V" | python3 -c 'import json,sys; v=json.load(sys.stdin); sys.exit(0 if v["status"]=="passed" and v["manifest_joins"] and v["token_verifies"] else 1)'; then
        ok "helper: a non-root Fabric uid launched the pass case through the setuid helper (launcher and bash same-byte; out dir as /dev/fd/N; observation verified and nonce spent at the root boundary); verdict bound, token verifies, out dir handed back"
    else
        bad helper "rc=$HRC report=$R owner=$OWN verdict=$V log=$(tail -c 300 "$H/helper.log")"
    fi
    # Amendment 50 (A84): the SAME observation, inputs rebuilt by the Fabric
    # uid, a second out dir: the custodian spent the nonce, nothing launches.
    rm -rf "$I"; cp -r "$W/helper-inputs-again" "$I"; chown -R "$FU:$FU" "$I"
    helper "$H/request-again.json" "$H/report-again.json"
    HRC2=$?
    R2="$(cat "$H/report-again.json")"
    if [[ $HRC2 == 30 && ! -e "$H/runs/op-boot-again" ]] && grep -q "already used" <<<"$R2"; then
        ok "helper-replay: one observation, one root launch (the second request spends a used nonce: refused, nothing launched)"
    else
        bad helper-replay "rc=$HRC2 report=$R2"
    fi
    kill "$CUST_PID" 2>/dev/null; wait "$CUST_PID" 2>/dev/null
fi

echo "psv guest boot test: $FAILS failure(s)"
[[ $FAILS == 0 ]]
