#!/bin/bash
# CE-9 spike corpus differential, rev2 (GFA-2): the GF-1 design build (fd mode keeps
# entry_dir + lock check). Per tracked .ax program records:
#   caps line, imports-a-module flag, then rc+stdout digest for
#   (a)  path mode, no filter, no scopes            (today's guest run)
#   (a2) the same again                             (nondeterminism control)
#   (b)  --source-fd under the spike deny-path filter (EACCES) with
#        AXON_ALLOWED_FS_READ= and AXON_ALLOWED_FS_WRITE= (the layered GF-3 run)
# usage: run2.sh REL_PATH   (relative to the b2-ce9 worktree) -> one TSV line
set -u
W=/home/cklaus/projects/axon-lanes/b2-ce9
C=/var/tmp/r51-tmp/CE9Spike/corpus
L=/var/tmp/r51-tmp/CE9Spike/linux
B=/home/cklaus/projects/axon-lanes/.doc-build/CE9Spike-musl/x86_64-unknown-linux-musl/release/axon
REF=/var/tmp/r51-tmp/b2-bin/axon
rel=$1
id=$(printf '%s' "$rel" | sha256sum | cut -c1-12)
d=$C/w2/$id
rm -rf "$d"; mkdir -p "$d/h"
caps=$(cd "$W" && HOME=$d/h timeout 20 "$REF" caps "$rel" 2>/dev/null | grep -E '^ +capabilities:' | sed 's/^ *capabilities: *//')
[ -z "$caps" ] && caps="?"
if grep -q -E '^[[:space:]]*(pub[[:space:]]+)?mod[[:space:]]+[A-Za-z_]' "$W/$rel"; then mods=mod; else mods=-; fi
run() { # mode -> "rc digest"
    local out rc
    if [ "$1" != filter ]; then
        out=$(cd "$d" && env -i PATH=/bin:/usr/bin HOME=$d/h XDG_CACHE_HOME=$d/c-$1 AXON_AI_MOCK=1 \
            timeout 10 "$B" run "$W/$rel" 2>"$d/err-$1" </dev/null)
        rc=$?
    else
        out=$(cd "$d" && env -i PATH=/bin:/usr/bin HOME=$d/h XDG_CACHE_HOME=$d/c-$1 AXON_AI_MOCK=1 \
            AXON_ALLOWED_FS_READ= AXON_ALLOWED_FS_WRITE= \
            timeout 10 python3 "$L/hostgate.py" "$FILTER" --source "$W/$rel" -- "$B" run 2>"$d/err-$1" </dev/null)
        rc=$?
    fi
    printf '%s' "$out" > "$d/out-$1"
    echo "$rc $(printf '%s' "$out" | sha256sum | cut -c1-12)"
}
export FILTER
FILTER=$(python3 "$L/mkfilter.py" errno)
a=$(run path)
a2=$(run path2)
b=$(run filter)
printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$rel" "$caps" "$mods" "$a" "$a2" "$b" "$id"
