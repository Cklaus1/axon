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
# The preflight takes its path list from `axon-fabric protected-host-paths`
# (the list ProtectedHost::load walks). The binary is the one the caller names
# in AXON_FABRIC_BIN, or the one built HERE from this tree -- never one that
# merely sits in target/ (C9 round 4; scripts/lib/axon_bin.sh).
. "$HERE/lib/axon_bin.sh"
if [ -z "${AXON_FABRIC_BIN:-}" ]; then
  (cd "$HERE/.." && cargo build -q -p axon-fabric --bins) || fail "cargo build -p axon-fabric --bins"
  AXON_FABRIC_BIN="$(cd "$HERE/.." && built_bin axon-fabric)" || fail "cannot tell where cargo built axon-fabric"
fi
FABRIC_BIN=$AXON_FABRIC_BIN
[ -x "$FABRIC_BIN" ] || fail "needs the axon-fabric binary at $FABRIC_BIN (cargo build -p axon-fabric, or set AXON_FABRIC_BIN)"
# A (amendment 45): the privileged launcher helper, built beside axon-fabric.
HELPER_BIN=${AXON_PROTECTED_LAUNCHER_BIN:-$(dirname "$FABRIC_BIN")/axon-protected-launcher}
[ -x "$HELPER_BIN" ] || fail "needs the axon-protected-launcher binary at $HELPER_BIN (cargo build -p axon-fabric)"
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
  O1D=$BASE/o1; rm -rf "$O1D"; mkdir -p "$O1D/keys" "$O1D/dist" "$O1D/grants" "$O1D/svc"
  chmod 0755 "$O1D" "$O1D/keys" "$O1D/dist" "$O1D/grants" "$O1D/svc"
  for f in launcher.sh manifest.json registry.json record.json observer.sh dist/vmlinux grants/g.axgrant; do
    echo x >"$O1D/$f"; chmod 0644 "$O1D/$f"
  done
  echo k >"$O1D/keys/attest.pk8"; chown $F:$F "$O1D/keys/attest.pk8"; chmod 0400 "$O1D/keys/attest.pk8"
  # The Fabric service's own private directory, under an operator directory.
  mkdir -p "$O1D/svc/runs"; chown $F:$F "$O1D/svc/runs"; chmod 0700 "$O1D/svc/runs"
  # Amendment 50: the custodian's own 0700 store, and the operator directory
  # its socket is bound in.
  mkdir -p "$O1D/cust/nonces" "$O1D/run"; chmod 0755 "$O1D/cust" "$O1D/run"
  chown ${STORE_OWNER:-$C}:${STORE_OWNER:-$C} "$O1D/cust/nonces"; chmod 0700 "$O1D/cust/nonces"
  # A: the setuid-root helper (group = the Fabric's), its root-private staging
  # root, and the engine its own config pins.
  cp "$HELPER_BIN" "$O1D/protected-launcher"; chown 0:$F "$O1D/protected-launcher"
  chmod 4750 "$O1D/protected-launcher"
  mkdir -p "$O1D/staging" "$O1D/engine"; chmod 0700 "$O1D/staging"; chmod 0755 "$O1D/engine"
  for f in engine/firecracker engine/jailer dist/rootfs.sqfs; do echo x >"$O1D/$f"; chmod 0644 "$O1D/$f"; done
  python3 - "$O1D" "${HELPER_FABRIC:-$F}" "${CUSTODIAN_AS:-$C}" "$F" <<'PY'
import json, sys
d = sys.argv[1]
json.dump({"schema": "axon-custodian/1", "custodian_uid": int(sys.argv[3]),
           "fabric_uid": int(sys.argv[4]), "launcher_uid": 0,
           "socket": f"{d}/run/custodian.sock", "store": f"{d}/cust/nonces", "max_age_s": 300},
          open(f"{d}/custodian.json", "w"))
json.dump({"schema": "axon-fabric-grant-registry/1",
           "grants": [{"grant_ref": "grant:g", "principal_ref": "principal:p", "path": "g.axgrant",
                       "sha256": "0" * 64}]}, open(f"{d}/grants/grants.json", "w"))
json.dump({"schema": "axon-protected-host/1",
           "launcher": {"path": f"{d}/launcher.sh"}, "profile_manifest": {"path": f"{d}/manifest.json"},
           "artifacts_dir": f"{d}/dist",
           "suite_registry": {"path": f"{d}/registry.json"},
           "qualification": {"record": f"{d}/record.json", "signature": None, "waivers": None},
           "signer": {"key_path": f"{d}/keys/attest.pk8"},
           "out_root": f"{d}/svc/runs",
           "privileged_launcher": {"path": f"{d}/protected-launcher"},
           "observer": {"command": {"path": f"{d}/observer.sh"},
                        "custodian": {"socket": f"{d}/run/custodian.sock", "uid": int(sys.argv[3])}},
           "grant_registry": {"path": f"{d}/grants/grants.json"}},
          open(f"{d}/protected-host.json", "w"))
z = "0" * 64
json.dump({"schema": "axon-protected-launcher/2", "fabric_uid": int(sys.argv[2]),
           "interpreter": {"path": "/bin/bash", "sha256": z},
           "launcher": {"path": f"{d}/launcher.sh", "sha256": z},
           "profile_manifest": {"path": f"{d}/manifest.json", "sha256": z},
           "artifacts_dir": f"{d}/dist", "firecracker": f"{d}/engine/firecracker",
           "jailer": f"{d}/engine/jailer", "out_root": f"{d}/svc/runs",
           "staging_root": f"{d}/staging", "max_timeout_s": 60, "max_input_bytes": 1,
           "observer": {"root": "/etc/axon/trust/observer", "max_age_s": 300,
                        "host_signer_public_key": "0" * 64},
           "custodian": {"socket": f"{d}/run/custodian.sock", "uid": int(sys.argv[3])}},
          open(f"{d}/protected-launcher.json", "w"))
PY
  chmod 0644 "$O1D/protected-host.json" "$O1D/grants/grants.json" "$O1D/protected-launcher.json" \
    "$O1D/custodian.json"
}
# A guest's view: the root's parent hidden behind an empty mount (dev stand-in
# for a Firecracker guest, whose image never contains the host path).
GUEST_OK="unshare --mount --propagation private sh -c 'mount -t tmpfs none $BASE && sh $HERE/trust_root_guest_probe.sh $ROOT'"
GUEST_HOST="sh $HERE/trust_root_guest_probe.sh $ROOT"
run() { # guest-cmd → sets OUT, RC
  OUT=$("$PF" --root "$ROOT" --host-config "$BASE/o1/protected-host.json" \
    --launcher-config "$BASE/o1/protected-launcher.json" \
    --custodian-config "$BASE/o1/custodian.json" --fabric-bin "$FABRIC_BIN" --verifier $V \
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

# C9 dev review round 1: every path ProtectedHost::load pins is probed, so each
# one made agent-writable FAILS (the preflight's own list used to omit these).
for t in grants/grants.json grants/g.axgrant observer.sh dist/vmlinux; do
  fixture; chmod 0666 "$BASE/o1/$t"; run "$GUEST_OK"
  failed_on "agent-writable $t" "c['action']=='open-write' and c['actor'].startswith('agent') and c['target'].endswith('/$t')"
done
for t in dist grants svc; do
  fixture; chown $A1 "$BASE/o1/$t"; run "$GUEST_OK"
  failed_on "agent-owned directory $t" "c['action']=='create' and c['actor']=='agent:$A1' and c['target'].endswith('/o1/$t')"
done
for t in runs; do
  fixture; chown $A1 "$BASE/o1/svc/$t"; run "$GUEST_OK"
  failed_on "agent-owned service directory $t" "c['action']=='create' and c['actor']=='agent:$A1' and c['target'].endswith('/svc/$t')"
  fixture; chmod 0777 "$BASE/o1/svc/$t"; run "$GUEST_OK"
  failed_on "world-writable service directory $t" "c['action']=='create' and c['actor'].startswith('agent') and c['target'].endswith('/svc/$t')"
done

# Amendment 50: the custodian. Each defect FAILS on its own check.
STORE_OWNER=$F fixture; run "$GUEST_OK"
failed_on "custodian store owned by the Fabric" "c['action']=='create' and c['actor']=='fabric' and c['target'].endswith('/cust/nonces')"
fixture; chgrp $F "$BASE/o1/cust/nonces"; chmod 0770 "$BASE/o1/cust/nonces"; run "$GUEST_OK"
failed_on "custodian store the Fabric group can write" "c['action']=='create' and c['actor']=='fabric' and c['target'].endswith('/cust/nonces')"
CUSTODIAN_AS=$F fixture; run "$GUEST_OK"
failed_on "custodian running as the Fabric uid" "c['action']=='custodian-separate'"
fixture; chown $A1 "$BASE/o1/run"; run "$GUEST_OK"
failed_on "agent-owned custodian socket directory" "c['action']=='create' and c['actor']=='agent:$A1' and c['target'].endswith('/o1/run')"
fixture; chmod 0666 "$BASE/o1/custodian.json"; run "$GUEST_OK"
failed_on "agent-writable custodian config" "c['action']=='open-write' and c['actor'].startswith('agent') and c['target'].endswith('/custodian.json')"

# A (amendment 45): the privileged helper. Each defect FAILS on its own check.
fixture; chmod 0750 "$BASE/o1/protected-launcher"; run "$GUEST_OK"
failed_on "helper not setuid" "c['action']=='helper-mode' or (c['action']=='exec-helper' and c['actor']=='fabric')"
fixture; chmod 4755 "$BASE/o1/protected-launcher"; run "$GUEST_OK"
failed_on "helper executable by other actors" "c['action']=='exec-helper' and c['actor'].startswith('agent')"
fixture; chmod 4770 "$BASE/o1/protected-launcher"; run "$GUEST_OK"
failed_on "helper writable by the Fabric group" "c['action']=='open-write' and c['actor']=='fabric' and c['target'].endswith('protected-launcher')"
fixture; chown $A1 "$BASE/o1/protected-launcher"; chmod 4750 "$BASE/o1/protected-launcher"; run "$GUEST_OK"
failed_on "helper not root-owned" "c['action']=='helper-mode'"
HELPER_FABRIC=$A2 fixture; run "$GUEST_OK"
failed_on "helper admits another uid than the Fabric" "c['action']=='helper-admits'"
fixture; chmod 0666 "$BASE/o1/engine/jailer"; run "$GUEST_OK"
failed_on "agent-writable engine the helper pins" "c['action']=='open-write' and c['actor'].startswith('agent') and c['target'].endswith('/engine/jailer')"

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
out=$(setpriv --reuid=65534 --regid=65534 --clear-groups bash "$BASE/pf.sh" --root "$ROOT" --fabric-bin "$FABRIC_BIN" --verifier $V --custodian $C --fabric $F --agent $A1 --guest-cmd true); rc=$?
[ $rc = 2 ] && printf '%s' "$out" | grep -q NOT_RUN || fail "non-root run must be NOT_RUN (2): $rc $out"
echo "ok: not root → NOT_RUN, never a pass"
fixture; out=$("$PF" --root "$ROOT" --host-config "$BASE/o1/protected-host.json" --fabric-bin "$FABRIC_BIN" --verifier 0 --custodian $C --fabric $F --agent $A1 --guest-cmd true); rc=$?
[ $rc = 2 ] || fail "root as an actor must be refused: $rc $out"
echo "ok: root is never accepted as a service actor"
echo "trust-root preflight mechanism: PASS (dev mode; certifies nothing)"
