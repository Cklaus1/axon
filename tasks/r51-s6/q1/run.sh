#!/usr/bin/env bash
# R51 §10/Q1: wall time of one sealed suite verdict vs the same suite unsealed (VM default, tree).
# usage: run.sh <axon-binary> [runs]   -> one line per mode: median wall seconds over <runs>
set -u
B=$(readlink -f "$1"); N=${2:-5}; D=$(cd "$(dirname "$0")" && pwd)
for mode in sealed unsealed-vm unsealed-tree; do
  case $mode in sealed) a=(--seal "$D/cand"); e=();; unsealed-vm) a=(); e=();; unsealed-tree) a=(); e=(AXON_ENGINE=tree);; esac
  ts=()
  for _ in $(seq "$N"); do
    s=$(date +%s.%N)
    out=$(cd "$D/suite" && env -i PATH=/usr/bin:/bin AXON_PATH="$D/suite:$D/cand" AXON_PATH_EXCLUSIVE=1 "${e[@]}" \
          "$B" test "$D/suite/accept.ax" --json "${a[@]}" 2>&1); rc=$?
    ts+=("$(echo "$(date +%s.%N) - $s" | bc)")
    grep -q '"passed":4,"failed":0' <<<"$out" || { echo "$mode: suite did not pass (rc=$rc): $out" >&2; exit 1; }
  done
  med=$(printf '%s\n' "${ts[@]}" | sort -n | sed -n "$(( (N + 1) / 2 ))p")
  echo "$mode median_wall_s=$med runs=$N"
done
