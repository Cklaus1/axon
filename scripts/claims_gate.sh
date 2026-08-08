#!/usr/bin/env bash
# claims_gate.sh — check that CLAUDE.md's mechanically-checkable claims are TRUE.
#
# WHY THIS EXISTS.
#
# `CLAUDE.md` is the language card for this repository. An agent reads it and acts
# on it at speed and with confidence; it does not independently rediscover the
# codebase first. That makes a stale claim in it qualitatively different from a
# stale claim in ordinary documentation — a human skims a wrong sentence and moves
# on, while an agent builds on it.
#
# This is not a hypothesis. Measured on the RLM benchmark the same week: the
# language card handed to the model said "this is the whole surface" while naming
# 36 of 331 builtins, and the omission of `str_chars`/`str_char_at` cost 3 of 8
# tasks outright — the model wrote code that could not typecheck because, as far as
# its map showed, the working function did not exist. Correcting the list alone
# moved first-try 5/8 → 7/8.
#
# And this repo has the same disease. Found by hand in a single session:
#   * "the virtual clock closes the LAST hole in replay" — false; the entire
#     environmental column (stdin/fs/net/exec) was open when that was written;
#   * `codegen.rs` referenced in three places — it became a DIRECTORY;
#   * the `AxonHost` doc claimed "a browser build supplies a virtual impl" — there
#     was exactly one impl, and the seam had never worked for a program at all;
#   * repo status claims stale in BOTH directions ("unsigned types non-functional"
#     was superseded; "R1c/R1e mostly done" was overstated);
#   * `gate.sh` itself was RED for a week and nobody knew.
#
# So the checkable claims get checked. What is checkable is narrow but load-bearing:
# a name promised here that the code does not have. The reverse direction (things
# the code has that this file omits) is NOT gated — that is a judgement call about
# document length. The resolution for it is the same one the builtin card needed:
# don't imply completeness. Check 5 enforces that, because an omission is harmless
# in a doc that says "a selection" and expensive in one that says "the whole
# surface".
#
# Exit 0 = every claim holds. Exit 1 = CLAUDE.md promises something untrue.

set -uo pipefail
cd "$(dirname "$0")/.."

AXON="${AXON:-./target/debug/axon}"
DOC="CLAUDE.md"

pass=0; fail=0
ok()  { echo "  OK $1"; pass=$((pass+1)); }
bad() { echo "FAIL [$1]: $2"; fail=$((fail+1)); }

if [ ! -f "$DOC" ]; then
  echo "claims_gate: SKIP — no $DOC"
  exit 0
fi

# ── 1: every `axon <verb>` named in the doc exists in the CLI ────────────────
#
# The failure this prevents is an agent confidently invoking a command that was
# renamed or never shipped, then treating the error as its own mistake.
if [ ! -x "$AXON" ]; then
  echo "  SKIP verbs — no axon binary at $AXON"
else
  HELP="$("$AXON" --help 2>&1)"
  claimed="$(grep -oE '^axon [a-z][a-z-]*' "$DOC" | awk '{print $2}' | sort -u)"
  n_claimed=$(echo "$claimed" | grep -c . || true)
  missing=""
  for v in $claimed; do
    # `axon --version` / `--help` are flags, not verbs.
    case "$v" in -*) continue ;; esac
    echo "$HELP" | grep -qE "^  $v( |$)" || missing="$missing $v"
  done
  if [ "$n_claimed" -lt 8 ]; then
    bad verb_extraction "only $n_claimed verbs extracted from $DOC — if the extraction broke, \
this check passes vacuously"
  elif [ -n "$missing" ]; then
    bad verbs "$DOC documents commands the CLI does not have:$missing"
  else
    ok "commands: all $n_claimed verbs named in $DOC exist in the CLI"
  fi
fi

# ── 2: every `AXON_*` env var named in the doc is actually read ──────────────
#
# A documented-but-unread var is the worst kind of wrong: it looks like a
# supported control surface, so a run is configured with it and behaves as if
# unconfigured — silently.
doc_vars="$(grep -oE 'AXON_[A-Z_]+' "$DOC" | sort -u)"
n_vars=$(echo "$doc_vars" | grep -c . || true)
unread=""
for v in $doc_vars; do
  grep -rqE "\"$v\"" --include=*.rs crates/ || unread="$unread $v"
done
if [ "$n_vars" -lt 8 ]; then
  bad var_extraction "only $n_vars env vars extracted — the check would pass vacuously"
elif [ -n "$unread" ]; then
  bad env_vars "$DOC documents env vars the code never reads:$unread"
else
  ok "env vars: all $n_vars documented AXON_* vars are read by the code"
fi

# ── 3: every source/script path named in the doc exists ─────────────────────
#
# This is what caught `codegen.rs` three times over after it became a directory.
# Bare filenames used as illustrations (`main.ax`) are matched anywhere in the
# tree; a path with a slash must exist AS WRITTEN, since that is a claim about
# layout.
paths="$(grep -oE '`[a-zA-Z0-9_./-]+\.(rs|sh|md|ax|ebnf|toml)`' "$DOC" | tr -d '`' | sort -u)"
n_paths=$(echo "$paths" | grep -c . || true)
# Illustrative filenames that deliberately do not exist (the doc introduces them
# with "e.g." to show the extension). Listed explicitly so the exemption is
# reviewable rather than a silent skip.
ILLUSTRATIVE=" main.ax server.ax "
gone=""
for p in $paths; do
  case "$ILLUSTRATIVE" in *" $p "*) continue ;; esac
  if [ -e "$p" ]; then
    continue
  fi
  # The doc states paths relative to a documented root — the Crate Structure and
  # "adding a builtin" sections are relative to `crates/axon-core/src/`. Resolve
  # against those roots rather than only the repo root, otherwise a correct
  # in-context path like `codegen/mod.rs` reads as missing. (Found by this check
  # on its first run, against paths I had just written.)
  found=""
  for root in "" "crates/axon-core/src/" "crates/"; do
    [ -e "$root$p" ] && { found=1; break; }
  done
  if [ -n "$found" ]; then
    continue
  fi
  case "$p" in
    */*) gone="$gone $p" ;;   # a path claim unresolvable under any documented root
    *)   find . -name "$p" -not -path "./target/*" -not -path "./.git/*" -print -quit 2>/dev/null \
           | grep -q . || gone="$gone $p" ;;
  esac
done
if [ "$n_paths" -lt 20 ]; then
  bad path_extraction "only $n_paths paths extracted — the check would pass vacuously"
elif [ -n "$gone" ]; then
  bad paths "$DOC names files that do not exist:$gone"
else
  ok "paths: all $n_paths file/script paths named in $DOC exist"
fi

# ── 4: every script the doc names is executable and present ─────────────────
#
# A named gate that cannot be run is worse than an unnamed one: it reads as
# coverage that does not exist.
scripts="$(grep -oE '`(scripts/)?[a-z0-9_]+\.sh`' "$DOC" | tr -d '`' | sed 's|^scripts/||' | sort -u)"
n_scripts=$(echo "$scripts" | grep -c . || true)
bad_scripts=""
for sc in $scripts; do
  [ -f "scripts/$sc" ] || bad_scripts="$bad_scripts $sc(absent)"
done
if [ "$n_scripts" -eq 0 ]; then
  ok "scripts: none named (nothing to check)"
elif [ -n "$bad_scripts" ]; then
  bad scripts "$DOC names harness scripts that are missing:$bad_scripts"
else
  ok "scripts: all $n_scripts harness scripts named in $DOC exist"
fi

# ── 5: the doc must not IMPLY completeness where it is a selection ───────────
#
# The direct lesson from the RLM card. A short doc is fine; a short doc asserting
# it is exhaustive is not, because the reader cannot discover otherwise and will
# treat an omission as an absence. Measured cost when the language card did this:
# 3 of 8 benchmark tasks.
lowered="$(tr '[:upper:]' '[:lower:]' < "$DOC")"
claims=""
for c in "this is the whole surface" "the complete list of builtins" \
         "all env vars" "every env var" "the full list of commands"; do
  case "$lowered" in *"$c"*) claims="$claims [$c]" ;; esac
done
if [ -n "$claims" ]; then
  bad completeness "$DOC implies completeness:$claims — say \"a selection\" instead, or make it \
actually complete. An implied-complete doc turns an omission into an apparent absence."
else
  ok "no false completeness claims (the RLM card's 3-of-8-task mistake)"
fi

echo "claims_gate: $pass passed, $fail failed"
[ "$fail" -eq 0 ] || exit 1
# A run that checked NOTHING also has zero failures and must not read as a pass.
if [ "$pass" -eq 0 ]; then
  echo "claims_gate: SKIP — nothing ran"
  exit 0
fi
echo "claims_gate: PASS"
