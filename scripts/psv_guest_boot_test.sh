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
DEV="${PSV_DEV:-${CARGO_TARGET_DIR:-$REPO/target}/debug/examples/psv_dev}"
[[ -x "$DEV" ]] || { echo "FAIL: $DEV missing (cargo build -p axon-psv --example psv_dev)"; exit 1; }

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
    cp -r "${CAND_SRC:-$W/cand}" "$W/$c/cand"; cp -r "$W/suite" "$W/$c/suite"
    MSHA="$("$DEV" make-job --candidate "$W/$c/cand" --suite "$W/$c/suite" --entry "$entry" \
        --test "$test" --job "$W/$c/job" | python3 -c 'import json,sys; print(json.load(sys.stdin)["manifest_sha256"])')"
    [[ -n "${4:-}" ]] && eval "$4"
    "$LAUNCH" --psv-candidate "$W/$c/cand" --psv-suite "$W/$c/suite" --psv-job "$W/$c/job" \
        --psv-manifest-sha "$MSHA" --policy "${POLICY_FOR:-$POLICY}" --out "$W/$c/out" --timeout-s 90 \
        > "$W/$c/launch.log" 2>&1
    RC=$?
}
check() { "$DEV" check-verdict --job "$W/$1/job" --verdict "$W/$1/out/out/verdict.json" 2>/dev/null; }

# pass
run pass accept.ax t_ok
V="$(check pass)"
if [[ $RC == 0 && "$(jq_r "$W/pass/out/result.json" psv.bound)" == true ]] \
   && printf '%s' "$V" | python3 -c 'import json,sys; v=json.load(sys.stdin); sys.exit(0 if v["status"]=="passed" and v["manifest_joins"] and v["token_verifies"] else 1)'; then
    ok "pass: the named test passed in the guest; verdict bound to serial; token verifies under the host-derived key"
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

echo "psv guest boot test: $FAILS failure(s)"
[[ $FAILS == 0 ]]
