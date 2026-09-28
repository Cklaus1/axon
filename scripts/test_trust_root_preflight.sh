#!/usr/bin/env bash
# test_trust_root_preflight.sh — the preflight's MECHANISM, on a dev fixture
# (mode "dev": certifies nothing). A clean fixture passes; each negative
# control must FAIL on the specific check it breaks, as an ATTEMPTED write the
# kernel allowed — not a mode-bit reading. (The ACL control is caught by mode
# bits too, since the group bits show the ACL mask; what it adds is that the
# grant is exercised, not inferred.) The chmod probe is chmod(2): chmod(1)
# skips the syscall on an unchanged mode, which made a first draft report
# "SUCCEEDED" for an attempt never made. Needs root (to act as UIDs).
set -uo pipefail
HERE=$(cd "$(dirname "$0")" && pwd)
PF="$HERE/trust_root_preflight.sh"
if [ "$(id -u)" != 0 ]; then echo "NOT_RUN: needs root to act as service UIDs (not a pass)"; exit 0; fi
fail() { echo "FAIL: $*"; exit 1; }
V=40001 C=40002 A1=40003 A2=40004 F=40005
BASE=$(mktemp -d /var/tmp/axon-preflight.XXXXXX); chmod 0755 "$BASE"
trap 'rm -rf "$BASE"' EXIT
ROOT=$BASE/trust
fixture() {
  rm -rf "$ROOT"; mkdir -p "$ROOT"/{qualification,observer,verifier,admission}
  chmod 0755 "$ROOT" "$ROOT"/*
  echo "ab" >"$ROOT/qualification/operator.pub"; echo '{}' >"$ROOT/verifier.json"
  chmod 0644 "$ROOT/qualification/operator.pub" "$ROOT/verifier.json"
  # O1: the protected-host config, the files it pins, and the Fabric's key.
  O1D=$BASE/o1; rm -rf "$O1D"; mkdir -p "$O1D/keys"; chmod 0755 "$O1D" "$O1D/keys"
  for f in launcher.sh manifest.json registry.json record.json; do echo x >"$O1D/$f"; chmod 0644 "$O1D/$f"; done
  echo k >"$O1D/keys/attest.pk8"; chown $F:$F "$O1D/keys/attest.pk8"; chmod 0400 "$O1D/keys/attest.pk8"
  python3 - "$O1D" <<'PY'
import json, sys
d = sys.argv[1]
json.dump({"schema": "axon-protected-host/1",
           "launcher": {"path": f"{d}/launcher.sh"}, "profile_manifest": {"path": f"{d}/manifest.json"},
           "suite_registry": {"path": f"{d}/registry.json"},
           "qualification": {"record": f"{d}/record.json", "signature": None, "waivers": None},
           "signer": {"key_path": f"{d}/keys/attest.pk8"}}, open(f"{d}/protected-host.json", "w"))
PY
  chmod 0644 "$O1D/protected-host.json"
}
# A guest's view: the root's parent hidden behind an empty mount (dev stand-in
# for a Firecracker guest, whose image never contains the host path).
GUEST_OK="unshare --mount --propagation private sh -c 'mount -t tmpfs none $BASE && sh $HERE/trust_root_guest_probe.sh $ROOT'"
GUEST_HOST="sh $HERE/trust_root_guest_probe.sh $ROOT"
run() { # guest-cmd → sets OUT, RC
  OUT=$("$PF" --root "$ROOT" --host-config "$BASE/o1/protected-host.json" --verifier $V \
    --custodian $C --fabric $F --agent $A1 --agent $A2 --guest-cmd "$1")
  RC=$?
}
failed_on() { # expected RC 1 and a failing check matching python predicate
  [ "$RC" = 1 ] || fail "$1: expected FAIL (1), got $RC: $OUT"
  printf '%s' "$OUT" | python3 -c "
import json,sys; r=json.load(sys.stdin); bad=[c for c in r['checks'] if not c['ok']]
sys.exit(0 if r['verdict']=='FAIL' and any($2 for c in bad) else 1)" || fail "$1: wrong failure: $OUT"
  echo "ok: $1"
}

fixture; run "$GUEST_OK"
[ "$RC" = 0 ] || fail "clean fixture: $OUT"
printf '%s' "$OUT" | python3 -c "import json,sys; r=json.load(sys.stdin); assert r['mode']=='dev' and r['verdict']=='PASS' and r['schema']=='axon-trust-preflight/1'; assert len(r['checks'])>20" \
  || fail "clean fixture report"
echo "ok: clean fixture passes in dev mode (never protected)"

fixture; chmod 0666 "$ROOT/qualification/operator.pub"; run "$GUEST_OK"
failed_on "world-writable key" "c['action']=='open-write' and c['actor'].startswith('agent')"

fixture; chown $A1 "$ROOT/qualification"; run "$GUEST_OK"
failed_on "agent-owned directory" "c['action']=='create' and c['actor']=='agent:$A1'"

fixture; chgrp $A2 "$ROOT/verifier.json"; chmod 0664 "$ROOT/verifier.json"; run "$GUEST_OK"
failed_on "group-writable manifest" "c['action']=='open-write' and c['actor']=='agent:$A2' and c['target'].endswith('verifier.json')"

fixture; chown $C "$ROOT/qualification/operator.pub"; run "$GUEST_OK"
failed_on "custodian-owned key (chmod)" "c['action']=='chmod' and c['actor']=='custodian'"

fixture; chmod 0700 "$ROOT/qualification"; run "$GUEST_OK"
failed_on "verifier cannot read" "c['action'] in ('read','list') and c['actor']=='verifier'"

fixture; run "$GUEST_HOST"
failed_on "guest can address the root" "c['actor']=='guest'"

fixture; chmod 0444 "$BASE/o1/keys/attest.pk8"; run "$GUEST_OK"
failed_on "signing key readable by others (A20)" "c['action']=='read-key' and c['actor'].startswith('agent')"

fixture; chown 0 "$BASE/o1/keys/attest.pk8"; run "$GUEST_OK"
failed_on "signing key unreadable by the Fabric" "c['action']=='read-key' and c['actor']=='fabric'"

fixture; chmod 0666 "$BASE/o1/launcher.sh"; run "$GUEST_OK"
failed_on "writable pinned launcher (O1)" "c['action']=='open-write' and c['target'].endswith('launcher.sh')"

fixture; chown $F "$BASE/o1"; run "$GUEST_OK"
failed_on "Fabric-owned O1 directory" "c['action']=='create' and c['actor']=='fabric' and c['target'].endswith('/o1')"

# u:$A1:rw as a POSIX access ACL, written as its xattr (no setfacl needed):
# version 2, then (tag u16, perm u16, id u32) entries in tag order.
grant_acl() {
  python3 - "$1" "$2" <<'PY'
import os, struct, sys
path, uid = sys.argv[1], int(sys.argv[2])
X = 0xFFFFFFFF
ents = [(0x01, 6, X), (0x02, 6, uid), (0x04, 4, X), (0x10, 6, X), (0x20, 4, X)]
os.setxattr(path, "system.posix_acl_access",
            struct.pack("<I", 2) + b"".join(struct.pack("<HHI", *e) for e in ents))
PY
}
if fixture && grant_acl "$ROOT/qualification/operator.pub" $A1 2>/dev/null; then
  mode=$(stat -c %a "$ROOT/qualification/operator.pub")
  run "$GUEST_OK"
  failed_on "ACL write grant (mode bits read $mode)" "c['action']=='open-write' and c['actor']=='agent:$A1'"
else
  echo "note: ACLs unavailable on this filesystem; ACL control not exercised"
fi

cp "$PF" "$BASE/pf.sh"; chmod 0755 "$BASE/pf.sh"
out=$(setpriv --reuid=65534 --regid=65534 --clear-groups bash "$BASE/pf.sh" --root "$ROOT" --verifier $V --custodian $C --fabric $F --agent $A1 --guest-cmd true); rc=$?
[ $rc = 2 ] && printf '%s' "$out" | grep -q NOT_RUN || fail "non-root run must be NOT_RUN (2): $rc $out"
echo "ok: not root → NOT_RUN, never a pass"
fixture; out=$("$PF" --root "$ROOT" --host-config "$BASE/o1/protected-host.json" --verifier 0 --custodian $C --fabric $F --agent $A1 --guest-cmd true); rc=$?
[ $rc = 2 ] || fail "root as an actor must be refused: $rc $out"
echo "ok: root is never accepted as a service actor"
echo "trust-root preflight mechanism: PASS (dev mode; certifies nothing)"
