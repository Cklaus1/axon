#!/bin/sh
# genviab's program generator behind axon-intent's `cmd:` seam, reached
# through the `micode` CLI. The prompt (card + task, or a repair prompt) on
# stdin, the model's reply VERBATIM on stdout: axon-intent strips fences, the
# Python arm strips its own. One JSON line per call is appended to
# $GENVIAB_MODEL_LOG with the keys of the cortex driver
# (crates/axon-cortex/benchmarks/drivers/micode.sh), so the same token pricing
# (ab/run_ab.py `estimate_usd`) accounts both.
#
# Differences from cortex's micode.sh, each forced by this use:
#   * No `Function:` line is required and nothing is appended to the prompt:
#     the loop's prompt already states the answer rules.
#   * micode takes the prompt as ONE argv string; Linux caps a single argument
#     at 128 KiB (MAX_ARG_STRLEN), so a longer prompt is refused here (exit 4,
#     logged) instead of failing inside exec.
# Kept from micode.sh: a scratch working directory with a tool-less project
# mode (`.micode/agents/generate.md`), `--no-skills`, ANTHROPIC_BASE_URL unset.
#
# Env: GENVIAB_MODEL (default claude-haiku-5-5), GENVIAB_MODEL_LOG,
# GENVIAB_TASK and GENVIAB_ARM (copied into the log row), GENVIAB_EFFORT
# (optional micode --effort; unset sends none).
set -e
# `$(cat)` would strip trailing newlines; the sentinel keeps the prompt exact.
prompt=$(cat; printf x)
prompt=${prompt%x}
model=${GENVIAB_MODEL:-claude-haiku-5-5}
log=${GENVIAB_MODEL_LOG:-/dev/null}
here=$(cd "$(dirname "$0")" && pwd)
bytes=$(printf '%s' "$prompt" | wc -c)

GENVIAB_PROMPT_BYTES=$bytes
GENVIAB_REQUESTED_MODEL=$model
export GENVIAB_PROMPT_BYTES GENVIAB_REQUESTED_MODEL

if [ "$bytes" -ge 131072 ]; then
  GENVIAB_MICODE_RC=-1 GENVIAB_CALL_MS=0 GENVIAB_REFUSED="prompt is $bytes bytes; micode takes it as one argv string, capped at 131072" \
    python3 "$here/micode_program_log.py" </dev/null
  exit 4
fi

scratch=$(mktemp -d "${TMPDIR:-/tmp}/genviab-micode-gen.XXXXXX")
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/.micode/agents"
printf -- '---\nmode: primary\nname: generate\ndescription: text generation only, no tools\n---\nYou have no tools. Answer with text only.\n' \
  > "$scratch/.micode/agents/generate.md"

set -- --format jsonl --no-skills --mode generate --config "provider.model=$model"
if [ -n "${GENVIAB_EFFORT:-}" ]; then
  set -- "$@" --effort "$GENVIAB_EFFORT"
fi

began=$(date +%s%N)
rc=0
out=$(cd "$scratch" && env -u ANTHROPIC_BASE_URL micode exec "$@" "$prompt" 2>/dev/null) || rc=$?
ended=$(date +%s%N)

GENVIAB_PROMPT_SHA256=$(printf '%s' "$prompt" | sha256sum | cut -d' ' -f1)
GENVIAB_MICODE_RC=$rc
GENVIAB_CALL_MS=$(( (ended - began) / 1000000 ))
export GENVIAB_PROMPT_SHA256 GENVIAB_MICODE_RC GENVIAB_CALL_MS
printf '%s' "$out" | python3 "$here/micode_program_log.py"
