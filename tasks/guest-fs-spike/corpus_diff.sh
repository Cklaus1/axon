#!/bin/bash
# CE-9 spike corpus differential: for one tracked .ax program, record
#   caps line, then rc+stdout digest for (a) --source-fd, no filter and
#   (b) --source-fd under the spike deny-path filter (EACCES).
# usage: run.sh REL_PATH   (relative to the b2-ce9 worktree) -> one TSV line
set -u
W=/home/cklaus/projects/axon-lanes/b2-ce9
C=/var/tmp/r51-tmp/CE9Spike/corpus
L=/var/tmp/r51-tmp/CE9Spike/linux
B=/home/cklaus/projects/axon-lanes/.doc-build/CE9Spike-musl/x86_64-unknown-linux-musl/release/axon
REF=/var/tmp/r51-tmp/b2-bin/axon
rel=$1
id=$(printf '%s' "$rel" | sha256sum | cut -c1-12)
d=$C/w/$id
rm -rf "$d"; mkdir -p "$d/h"
caps=$(cd "$W" && HOME=$d/h timeout 20 "$REF" caps "$rel" 2>/dev/null | grep -E '^ +capabilities:' | sed 's/^ *capabilities: *//')
[ -z "$caps" ] && caps="?"
run() { # mode -> "rc digest"
    local out rc
    if [ "$1" = nofilter ]; then
        out=$(cd "$d" && exec 7<"$W/$rel" && env -i PATH=/bin:/usr/bin HOME=$d/h XDG_CACHE_HOME=$d/c AXON_AI_MOCK=1 \
            timeout 10 "$B" run --source-fd 7 "$W/$rel" 2>"$d/err-$1" </dev/null)
        rc=$?
    else
        out=$(cd "$d" && env -i PATH=/bin:/usr/bin HOME=$d/h XDG_CACHE_HOME=$d/c AXON_AI_MOCK=1 \
            timeout 10 python3 "$L/hostgate.py" "$FILTER" --source "$W/$rel" -- "$B" run 2>"$d/err-$1" </dev/null)
        rc=$?
    fi
    printf '%s' "$out" > "$d/out-$1"
    echo "$rc $(printf '%s' "$out" | sha256sum | cut -c1-12)"
}
export FILTER
FILTER=$(python3 "$L/mkfilter.py" errno)
a=$(run nofilter)
b=$(run filter)
printf '%s\t%s\t%s\t%s\t%s\n' "$rel" "$caps" "$a" "$b" "$id"
