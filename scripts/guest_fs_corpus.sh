#!/usr/bin/env bash
# guest_fs_corpus.sh — guest-fs GF-3 corpus differential (host only, no KVM).
#
# Promoted from the CE-9 spike's corpus_diff2.sh (tasks/guest-fs-spike/). For
# every tracked .ax program, copied ALONE into a scratch directory (as the guest
# sees /work/job/program.ax), it records rc + stdout digest for:
#   (a)  `axon run program.ax`                        (today's guest run)
#   (a2) the same again                               (nondeterminism control)
#   (f)  `axon run --source-fd 3 program.ax`, fd 3 opened by the harness (GF-1)
#   (b)  the guest child's fs:none path, run on the host by `axon-guest-init
#        --host-fs-mirror` (feature `host-fs-mirror`): load before gate, empty
#        path scopes, the compiled-in filter, exec (GF-2 + GF-3)
# and whether `fc_linux_profile.sh` would ADMIT the program under fs:none
# (section 1c there: `axon check` passes with module search confined to an
# empty directory, `axon caps --json` is fully classified and names nothing
# outside {env, net}).
#
# FAILS (exit 1) on any program with (a) = (a2) and
#   * (f) != (a): --source-fd changed a program's behaviour, or
#   * admitted and (b) != (a): fs:none changed an admitted program's behaviour.
# Programs with (a) != (a2) are listed as NONDETERMINISTIC (no evidence either
# way); admitted programs that fail identically under (a) and (b) are listed as
# BOTH-FAIL (no parity evidence: both refuse before effects matter).
#
# usage: scripts/guest_fs_corpus.sh [--jobs N] [--out FILE.tsv]
# Exit: 0 no difference, 1 difference, 2 harness error.
set -uo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
JOBS="$(nproc)" OUT=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --jobs) JOBS="$2"; shift 2 ;;
        --out) OUT="$2"; shift 2 ;;
        *) echo "guest_fs_corpus: unknown arg $1" >&2; exit 2 ;;
    esac
done

. scripts/lib/axon_bin.sh
# Static musl, as in the guest image: under the filter a dynamic loader could
# not open its libraries.
T=x86_64-unknown-linux-musl
cargo build --locked -q -p axon-core --no-default-features --bin axon --release --target "$T" \
    || { echo "guest_fs_corpus: FAIL — cargo build axon" >&2; exit 2; }
cargo build --locked -q -p axon-guest-init --features host-fs-mirror --release --target "$T" \
    || { echo "guest_fs_corpus: FAIL — cargo build axon-guest-init" >&2; exit 2; }
use_built AXON axon "$T" release
use_built AXON_GUEST_INIT axon-guest-init "$T" release

W="$(mktemp -d "${TMPDIR:-/tmp}/guest-fs-corpus.XXXXXX")"
trap 'rm -rf "$W"' EXIT
[[ -n "$OUT" ]] || OUT="$W/results.tsv"
git ls-files -z '*.ax' > "$W/list"

one() {  # one REL -> REL \t admit \t a \t a2 \t f \t b
    local rel="$1" d
    d="$W/p/$(printf '%s' "$rel" | sha256sum | cut -c1-16)"
    mkdir -p "$d/job" "$d/no-modules"
    cp "$rel" "$d/job/program.ax"
    ax() {  # ax CACHE-TAG [cmd...] : the guest's environment, fixed
        local tag="$1"; shift
        (cd "$d/job" && env -i PATH=/bin:/usr/bin HOME="$d/h-$tag" XDG_CACHE_HOME="$d/c-$tag" \
            AXON_AI_MOCK=1 AXON_PATH="$d/no-modules" AXON_PATH_EXCLUSIVE=1 timeout 10 "$@" </dev/null)
    }
    local admit=no
    if ax chk "$AXON" check program.ax >/dev/null 2>&1 \
        && ax chk "$AXON" caps --json program.ax 2>/dev/null | python3 -I -c '
import json, sys
c = json.load(sys.stdin)
sys.exit(0 if c.get("unknown") is False
         and all(x in ("env", "net") for x in c.get("capabilities") or []) else 1)' 2>/dev/null; then
        admit=yes
    fi
    run() {  # run TAG cmd... -> "rc digest"
        local tag="$1"; shift
        ax "$tag" "$@" > "$d/out-$tag" 2>/dev/null
        echo "$? $(sha256sum < "$d/out-$tag" | cut -c1-16)"
    }
    local a a2 f b
    a="$(run a "$AXON" run program.ax)"
    a2="$(run a2 "$AXON" run program.ax)"
    f="$(exec 3<"$d/job/program.ax" && run f "$AXON" run --source-fd 3 program.ax)"
    b="$(run b "$AXON_GUEST_INIT" --host-fs-mirror --source-fd 3=program.ax -- "$AXON" run --source-fd 3 program.ax)"
    # A difference is only evidence when the program is deterministic: before
    # reporting one, run (a) three more times; any change makes it
    # NONDETERMINISTIC (the differing run is recorded as (a2)).
    if [[ "$a" == "$a2" && ( "$f" != "$a" || "$b" != "$a" ) ]]; then
        local k ak
        for k in 3 4 5; do
            ak="$(run "a$k" "$AXON" run program.ax)"
            [[ "$ak" != "$a" ]] && { a2="$ak"; break; }
        done
    fi
    printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$rel" "$admit" "$a" "$a2" "$f" "$b"
    rm -rf "$d"
}
export -f one
export W AXON AXON_GUEST_INIT
xargs -0 -P "$JOBS" -I{} bash -c 'one "$1"' _ {} < "$W/list" | sort > "$OUT"

python3 -I - "$OUT" <<'PY'
import sys
rows = [l.rstrip("\n").split("\t") for l in open(sys.argv[1]) if l.strip()]
bad, nondet, bothfail, admitted, same = [], [], [], 0, 0
for rel, admit, a, a2, f, b in rows:
    if a != a2:
        nondet.append(rel); continue
    if f != a:
        bad.append(f"{rel}: --source-fd changed it: (a) {a} -> (f) {f}")
    if admit == "yes":
        admitted += 1
        if b != a:
            bad.append(f"{rel}: fs:none changed an admitted program: (a) {a} -> (b) {b}")
        elif a.split()[0] != "0":
            bothfail.append(f"{rel} (rc {a.split()[0]})")
        else:
            same += 1
print(f"guest_fs_corpus: {len(rows)} programs; {admitted} admitted under fs:none, "
      f"{same} of them exit 0 with identical stdout under (a) and (b)")
for n in nondet: print(f"NONDETERMINISTIC {n}")
for n in bothfail: print(f"BOTH-FAIL {n}")
for n in bad: print(f"DIFF {n}")
if bad:
    print(f"guest_fs_corpus: FAIL — {len(bad)} difference(s)"); sys.exit(1)
print(f"guest_fs_corpus: PASS — 0 differences ({len(nondet)} nondeterministic, {len(bothfail)} both-fail listed)")
PY
