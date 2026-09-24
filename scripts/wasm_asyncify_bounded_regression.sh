#!/usr/bin/env bash
# wasm_asyncify_bounded_regression.sh — proves the R15 §13 B3 HOST-SAFETY
# invariant: no .ax program, and no runtime bug, can consume unbounded host
# memory or run forever during a test.
#
#   unbounded program → bounded execution → deterministic RESOURCE_EXHAUSTED /
#   TIMEOUT → child and descendants dead → host survives
#
# HISTORY: axon_eval took ownership of (and freed) the source buffer, while the
# Asyncify rewind loop re-entered it with the SAME pointer once per suspend. The
# resulting double-free corrupted the wasm allocator and grew linear memory at
# ~0.5 GiB/s until the WSL VM died. The fix is axon_eval_borrowed + axon_free; this
# harness is the guard that keeps the safeguard honest if such a bug returns.
#
# THIS SCRIPT MUST NOT BE ABLE TO KILL THE HOST — including when the inner
# safeguard it is testing is broken. So every run here is wrapped in its OWN
# independent emergency bound (a second, smaller ceiling and a shorter deadline
# applied by this script, not by the code under test). If the inner safeguard is
# mutated away, the outer one still stops the run.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. "$ROOT/scripts/lib_bounded_run.sh"

command -v node >/dev/null 2>&1 || { echo "wasm_asyncify_bounded_regression: node not found — skipping"; exit 0; }

# The emergency cap. Deliberately tighter than anything the asyncify harness uses,
# so it binds first if the harness's own limit is broken or removed.
EMERG_MEM="768M"
EMERG_SECS="25"

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
fails=0

# A memory bomb that does NOT depend on the axon runtime, so this regression keeps
# working (and keeps being safe) regardless of the state of the wasm build. It
# grows Wasm linear memory AND touches the pages, which is the allocation class
# that --max-old-space-size provably fails to bound.
cat > "$WORK/bomb.js" <<'JS'
const m = new WebAssembly.Memory({ initial: 1 });
let pages = 1;
for (;;) {
  m.grow(160); pages += 160;
  const v = new Uint8Array(m.buffer);
  for (let o = (pages - 160) * 65536; o < pages * 65536; o += 4096) v[o] = 1;
}
JS

# An infinite loop that allocates nothing — the TIMEOUT path.
cat > "$WORK/spin.js" <<'JS'
for (;;) {}
JS

# A wrapper that backgrounds its child and `wait`s, which returns 0 when the child
# is killed. Proves the safeguard does not trust a launderable exit status.
cat > "$WORK/launder.sh" <<'SH'
#!/bin/bash
node "$1" >/dev/null 2>&1 &
wait
exit 0
SH
chmod +x "$WORK/launder.sh"

check() {
  local name="$1" want_class="$2" rc="$3" class="$4" peak_mb="$5" cap_mb="$6"
  if [ "$class" != "$want_class" ]; then
    echo "  FAIL  $name: class=$class want=$want_class rc=$rc"; fails=$((fails+1)); return
  fi
  if [ "$rc" = "0" ]; then
    echo "  FAIL  $name: safeguard fired but status was 0 (false green)"; fails=$((fails+1)); return
  fi
  # Peak must respect the declared bound. Allow one page of slack.
  if [ -n "$peak_mb" ] && [ "$peak_mb" -gt "$cap_mb" ]; then
    echo "  FAIL  $name: peak ${peak_mb}MB exceeded declared ${cap_mb}MB"; fails=$((fails+1)); return
  fi
  echo "  OK    $name: $class rc=$rc peak=${peak_mb:-n/a}MB (bound ${cap_mb}MB)"
}

echo "wasm_asyncify_bounded_regression: proving the host-safety invariant…"

# (0) THE DEADLINE LAYER, FIRST AND SELF-BOUNDED. Every later check would hang if
# bounded_run lost its wall-clock deadline, and a hang is not a report — it makes
# the discriminator whatever outer bound the caller happened to supply. So this
# check runs before anything that can block, and wraps the call in its OWN
# `timeout` (independent of the deadline under test) so a missing inner deadline
# comes back as an explicit FAIL here instead of stalling the suite.
t0=$SECONDS
timeout --signal=KILL 15s bash -c '
  . "'"$ROOT"'/scripts/lib_bounded_run.sh"
  bounded_run '"$EMERG_MEM"' 2 node "'"$WORK"'/spin.js" >/dev/null 2>&1
  rc=$?
  echo "$rc $BOUNDED_RUN_CLASS"
' > "$WORK/deadline.res" 2>/dev/null
outer=$?
elapsed=$(( SECONDS - t0 ))
read -r d_rc d_cls < "$WORK/deadline.res" 2>/dev/null || { d_rc=""; d_cls=""; }
if [ "$outer" != "0" ]; then
  echo "  FAIL  deadline layer ABSENT: inner run never terminated; outer bound fired after ${elapsed}s"
  fails=$((fails+1))
elif [ "$d_cls" = "TIMEOUT" ] && [ "$d_rc" != "0" ] && [ "$elapsed" -le 12 ]; then
  echo "  OK    deadline layer binds independently: TIMEOUT after ${elapsed}s (limit 2s)"
else
  echo "  FAIL  deadline layer: class=$d_cls rc=$d_rc elapsed=${elapsed}s (want TIMEOUT, nonzero, <=12s)"
  fails=$((fails+1))
fi

# (1) Unbounded MEMORY growth is terminated at the ceiling, not by the host dying.
bounded_run "$EMERG_MEM" "$EMERG_SECS" node --max-old-space-size=512 "$WORK/bomb.js" >/dev/null 2>&1
rc=$?; cls="$BOUNDED_RUN_CLASS"; peak=$(( ${BOUNDED_RUN_PEAK:-0} / 1048576 ))
check "memory bomb terminated at ceiling" RESOURCE_EXHAUSTED "$rc" "$cls" "$peak" 768

# (2) A non-allocating infinite loop is terminated by the deadline.
timeout --signal=KILL 20s bash -c '
  . "'"$ROOT"'/scripts/lib_bounded_run.sh"
  bounded_run '"$EMERG_MEM"' 3 node "'"$WORK"'/spin.js" >/dev/null 2>&1
  echo "$? $BOUNDED_RUN_CLASS ${BOUNDED_RUN_PEAK:-0}"' > "$WORK/spin.res" 2>/dev/null
if [ $? != 0 ]; then
  echo "  FAIL  infinite loop terminated by deadline: inner run never terminated (outer bound fired)"
  fails=$((fails+1))
else
  read -r rc cls pk < "$WORK/spin.res"; peak=$(( ${pk:-0} / 1048576 ))
  check "infinite loop terminated by deadline" TIMEOUT "$rc" "$cls" "$peak" 768
fi

# (3) A DESCENDANT's exhaustion is caught, and a laundering wrapper cannot hide it.
bounded_run "$EMERG_MEM" "$EMERG_SECS" "$WORK/launder.sh" "$WORK/bomb.js" >/dev/null 2>&1
rc=$?; cls="$BOUNDED_RUN_CLASS"; peak=$(( ${BOUNDED_RUN_PEAK:-0} / 1048576 ))
check "descendant exhaustion survives a laundering wrapper" RESOURCE_EXHAUSTED "$rc" "$cls" "$peak" 768

# (4) No orphans: nothing from the bounded runs outlives the boundary.
sleep 0.5
orphans="$(pgrep -f "$WORK/(bomb|spin)\.js" 2>/dev/null | wc -l)"
if [ "$orphans" != "0" ]; then
  echo "  FAIL  orphan check: $orphans process(es) survived the boundary"; fails=$((fails+1))
else
  echo "  OK    orphan check: no process survived the boundary"
fi

# (5) No leaked cgroups — scoped to THIS script's own runs. bounded_run names each
# cgroup axon_bounded_<sourcing-shell-pid>_<random>, so globbing the bare prefix
# also matches cgroups belonging to other concurrent bounded_run callers (another
# agent session, a parallel harness). That made this check a false RED: it caught
# a peer's in-flight compile. Scope by $$ so the check sees only what we created.
leaked="$(ls -d /sys/fs/cgroup/axon_bounded_${$}_* 2>/dev/null | wc -l)"
if [ "$leaked" != "0" ]; then
  echo "  FAIL  cgroup check: $leaked bounded cgroup(s) leaked"; fails=$((fails+1))
else
  echo "  OK    cgroup check: no cgroup leaked"
fi

# (6) A normal, well-behaved run still passes — the safeguard must not break the
# happy path, or every gate downstream becomes a false red.
bounded_run "$EMERG_MEM" "$EMERG_SECS" node -e 'console.log("fine")' >/dev/null 2>&1
rc=$?; cls="$BOUNDED_RUN_CLASS"
if [ "$rc" = "0" ] && [ "$cls" = "OK" ]; then
  echo "  OK    well-behaved run unaffected: rc=0 class=OK"
else
  echo "  FAIL  well-behaved run: rc=$rc class=$cls (want rc=0 class=OK)"; fails=$((fails+1))
fi

# (8) The safeguard must REPORT a degraded state rather than pretend to protect.
if [ "$(id -u)" = "0" ]; then
  if grep -q 'BOUNDED_RUN_CLASS_DEGRADED' "$ROOT/scripts/lib_bounded_run.sh"; then
    echo "  OK    degraded-mode signal present (no silent loss of the memory bound)"
  else
    echo "  FAIL  no degraded-mode signal in lib_bounded_run.sh"; fails=$((fails+1))
  fi
fi

if [ "$fails" != "0" ]; then
  echo "wasm_asyncify_bounded_regression: FAIL ($fails check(s))"; exit 1
fi
echo "wasm_asyncify_bounded_regression: OK — unbounded programs are bounded; host is safe"
