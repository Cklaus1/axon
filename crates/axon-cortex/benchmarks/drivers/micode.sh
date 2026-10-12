#!/bin/sh
# A real model behind the `cmd:` seam, reached through the `micode` CLI.
#
# Same contract as claude_cli.sh: Cortex's prompt on stdin, ONE function body
# on stdout, one JSON line per call appended to $CORTEX_MODEL_LOG with the
# same keys, so ab/run_ab.py accounts both transports with one reader.
#
# Differences from claude_cli.sh, each forced by micode:
#   * micode writes a `.micode/` state directory (sessions, trust capture,
#     builtin skills) into its working directory. It runs from a scratch
#     directory created here and removed on exit, never from Cortex's
#     workspace, or the workspace would gain files the loop did not write.
#   * The generator has NO tools: the scratch directory holds a project mode
#     (`.micode/agents/generate.md`, `mode: primary`, no `tools:` key), and a
#     micode mode with no tool list admits none. `--no-skills` keeps the skill
#     catalog out of the prompt.
#   * ANTHROPIC_BASE_URL is unset for the call: micode refuses to send its
#     OAuth grant to a non-Anthropic base URL, and the shell may point that
#     variable at a local gateway.
#   * micode on an OAuth grant reports tokens, not dollars, so `cost_usd` is
#     logged as null; run_ab.py prices the tokens.
set -e
prompt=$(cat)
model=${CORTEX_MODEL:-claude-haiku-5-5}
log=${CORTEX_MODEL_LOG:-/dev/null}

scratch=$(mktemp -d "${TMPDIR:-/tmp}/cortex-micode-gen.XXXXXX")
trap 'rm -rf "$scratch"' EXIT
mkdir -p "$scratch/.micode/agents"
printf -- '---\nmode: primary\nname: generate\ndescription: text generation only, no tools\n---\nYou have no tools. Answer with text only.\n' \
  > "$scratch/.micode/agents/generate.md"

began=$(date +%s%N)
rc=0
out=$(cd "$scratch" && env -u ANTHROPIC_BASE_URL micode exec --format jsonl --no-skills \
        --mode generate --config "provider.model=$model" \
        "$(printf '%s\n\nReturn ONLY the replacement body. No fences, no commentary.' "$prompt")" \
      2>/dev/null) || rc=$?
ended=$(date +%s%N)

CORTEX_PROMPT_TEXT="$prompt"
CORTEX_MICODE_RC=$rc
CORTEX_CALL_MS=$(( (ended - began) / 1000000 ))
CORTEX_REQUESTED_MODEL=$model
export CORTEX_PROMPT_TEXT CORTEX_MICODE_RC CORTEX_CALL_MS CORTEX_REQUESTED_MODEL
# The Python below sits inside ONE sh single-quoted string: a single quote
# anywhere in it, comments included, ends the string and hands the rest to sh
# (the defect tests/claude_driver.rs pins for claude_cli.sh; tests/
# micode_driver.rs runs this file against a stub `micode`).
printf '%s' "$out" | python3 -c '
import json, os, re, sys

events = []
for line in sys.stdin.read().splitlines():
    try:
        events.append(json.loads(line))
    except ValueError:
        pass
rc = int(os.environ.get("CORTEX_MICODE_RC", "0"))
errors = [e for e in events if e.get("type") == "session_error"]
retries = [e for e in events if e.get("type") == "retry"]
start = next((e for e in events if e.get("type") == "session_start"), {})
ends = [e for e in events if e.get("type") == "turn_end"]

# Text of the LAST turn: the generate mode has no tools, so there is one turn,
# but a reply is the final turn in any case, never a concatenation of turns.
text = ""
for e in events:
    if e.get("type") == "turn_start":
        text = ""
    elif e.get("type") == "assistant_delta":
        text += e.get("text") or ""

usage = lambda k: sum((e.get("usage") or {}).get(k) or 0 for e in ends)
prompt = os.environ.get("CORTEX_PROMPT_TEXT", "")
log = os.environ.get("CORTEX_MODEL_LOG", "/dev/null")
row = {
    "transport": "micode",
    "model": [start["model"]] if start.get("model") else [],
    "requested_model": os.environ.get("CORTEX_REQUESTED_MODEL"),
    "micode_version": start.get("micode_version"),
    "cost_usd": None,
    "duration_api_ms": int(os.environ.get("CORTEX_CALL_MS", "0")),
    "input_tokens": usage("input_tokens"),
    "output_tokens": usage("output_tokens"),
    "cache_read": usage("cache_read_input_tokens"),
    "cache_create": usage("cache_creation_input_tokens"),
    "turns": len(ends),
    "exit": rc,
    "session_errors": [json.dumps(e)[:400] for e in errors],
    "retries": [json.dumps(e)[:400] for e in retries],
}

def fail(why, code=1):
    # A failed call still spent tokens; it is logged before the driver exits
    # so the run accounts it.
    row.update(error=why, body=None, extraction_status="call_failed")
    with open(log, "a") as fh:
        fh.write(json.dumps(row) + "\n")
    sys.stderr.write("driver: " + why + "\n")
    sys.exit(code)

if rc != 0:
    fail("micode exited %d" % rc)
if errors:
    fail("micode reported a session error")
if not ends:
    fail("micode produced no turn_end event")

# Extraction, identical to claude_cli.sh: the LAST fenced block if there is one.
body = text
fences = re.findall(r"```[a-zA-Z]*\n(.*?)```", text, re.S)
if fences:
    body = fences[-1]
    status = ("ambiguous_multiple_fences" if len(fences) > 1
              else "clean_fence" if text.strip().startswith("```")
              else "prose_plus_fence")
else:
    status = "raw_code" if text.strip() else "no_code_found"

m = re.search(r"^Function: (.+)$", prompt, re.M)
if not m:
    sys.stderr.write("driver: prompt has no `Function:` line; cannot "
                     "attribute this proposal to a candidate\n")
    sys.exit(3)
candidate = m.group(1).strip()
# Rejected attempts, counted by their header LINE: the prompt now also carries
# the file and test output, where the bare words could appear.
prior = len(re.findall(r"^Attempt \d+ was REJECTED because", prompt, re.M))
row.update({
    "proposal_number": 1 + prior if prompt else None,
    "prior_rejections": prior if prompt else None,
    "body": body,
    "extraction_status": status,
    "candidate": candidate,
    "proposal_index_for_candidate": 1 + prior,
})
with open(log, "a") as fh:
    fh.write(json.dumps(row) + "\n")
sys.stdout.write(body)
'
