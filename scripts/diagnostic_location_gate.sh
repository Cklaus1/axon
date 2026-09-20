#!/usr/bin/env bash
# A reported diagnostic location must EXIST in the file the diagnostic names.
#
# This needs no expected values and no oracle. It asks one question of every
# diagnostic any verb emits: is `line` within the file `file` points at? A
# wrong-file diagnostic is usually out of range, because the files differ in
# length — which makes this cheap test catch a class that per-case expected
# values would only catch one instance at a time.
#
# It is RED on HEAD, deliberately. `Span` is (start, end) with no file
# identity, `load_use_decls` prepends imported modules' items carrying offsets
# into THEIR files, and every diagnostic is then rendered through the entry
# file's SourceMap. `SourceMap::line_col` has no bounds check, so an offset
# past EOF clamps to the last line instead of refusing.
#
# Measured: an error truly at lib.ax:15 reports as main.ax:6, in a 5-line file.
#
# It does NOT catch a wrong file that happens to be long enough — that needs
# real file identity. This is the floor: a location that cannot exist.
set -uo pipefail
cd "$(dirname "$0")/.."
AXON="${AXON:-$PWD/target/debug/axon}"
[ -x "$AXON" ] || { echo "SKIP: no binary at $AXON (build axon-core first)"; exit 0; }

WS=$(mktemp -d); trap 'rm -rf "$WS"' EXIT
fails=0; checked=0; diags=0

# The import-bearing corpus: these are the files that can desync at all.
mapfile -t FILES < <(grep -rl '^use ' examples --include='*.ax' 2>/dev/null)

# Plus a purpose-built case, so the gate has a known-bad input of its own and
# does not depend on the corpus continuing to contain one.
mkdir -p "$WS/m"
# lib.ax must be LONGER than main.ax, or an out-of-range line cannot occur and
# the fixture would prove nothing.
{ echo 'fn ok_one() -> i64 { 1 }'
  for i in $(seq 12); do echo "// pad $i"; done
  echo 'fn deliberately_broken(a: i64) -> i64 { a + "not a number" }'
} > "$WS/m/lib.ax"
printf 'use lib\nfn main() -> i64 {\n    println(to_str(ok_one()))\n    0\n}\n' > "$WS/m/main.ax"
FILES+=("$WS/m/main.ax")

for f in "${FILES[@]}"; do
  [ -f "$f" ] || continue
  checked=$((checked+1))
  # DIAGNOSTICS GO TO STDERR. The first version of this gate captured stdout
  # and found nothing, then reported "no located diagnostics" — a probe that
  # discarded the very thing it was looking for, and which would have said
  # exactly the same on a fully fixed compiler.
  out=$( { cd "$(dirname "$f")" && AXON_PATH="$(dirname "$f")" "$AXON" check "$(basename "$f")"; } 2>&1 )
  while IFS= read -r line; do
    [ -n "$line" ] || continue
    read -r df dl < <(printf '%s' "$line" | python3 -c '
import json,sys
try: d=json.loads(sys.stdin.read())
except Exception: raise SystemExit
if d.get("line"): print(d.get("file",""), d["line"])
') || continue
    [ -n "${dl:-}" ] || continue
    diags=$((diags+1))
    target="$(dirname "$f")/$df"
    if [ ! -f "$target" ]; then
      echo "FAIL $f: diagnostic names \`$df\`, which does not exist"; fails=$((fails+1)); continue
    fi
    n=$(wc -l < "$target")
    if [ "$dl" -gt "$n" ]; then
      echo "FAIL $f: diagnostic reports $df:$dl, but $df has only $n lines"
      echo "     the location cannot exist — the span was rendered against the wrong file"
      fails=$((fails+1))
    fi
  done <<< "$out"
done

# A gate that examined no diagnostics would pass on anything.
if [ "$diags" -lt 1 ]; then
  echo "CHECKER ERROR: examined $checked file(s) and found NO located diagnostics;"
  echo "  this gate proves nothing in that state"
  exit 2
fi
echo "diagnostic locations: $diags located diagnostic(s) over $checked file(s), $fails impossible"
[ "$fails" -eq 0 ]
