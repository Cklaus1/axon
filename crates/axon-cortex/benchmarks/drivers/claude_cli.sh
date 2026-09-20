#!/bin/sh
# A real model behind the `cmd:` seam.
#
# Reads Cortex's prompt on stdin, returns ONE function body on stdout. The
# credentials live in the `claude` CLI's own environment and never enter this
# repository, this process, or any Axon config — which is the whole reason the
# seam takes a program rather than an API key.
#
# Every call appends one JSON line to $CORTEX_MODEL_LOG: model, tokens, cost,
# latency, the prompt's proposal number, and the body returned. The harness
# reads that to attribute an outcome to a specific proposal, which the loop's
# own episode record cannot do — it sees text arriving, not what it cost or
# which attempt produced it.
set -e
prompt=$(cat)
model=${CORTEX_MODEL:-claude-haiku-4-5-20251001}
log=${CORTEX_MODEL_LOG:-/dev/null}

# The generator is a GENERATOR. No tools: it is asked for text and must not be
# able to read the workspace, run the checks, or see the adjudicator — the
# separation the loop is built on would be meaningless if the program behind
# the seam could reach past it.
out=$(printf '%s\n\nReturn ONLY the replacement body. No fences, no commentary.' "$prompt" \
  | claude -p --output-format json --model "$model" \
      --disallowed-tools "Bash" "Read" "Write" "Edit" "Glob" "Grep" "Task" "WebFetch" "WebSearch" \
  2>/dev/null) || { echo "claude CLI failed" >&2; exit 1; }

# The prompt reaches the reader below, so the proposal number can be read
# from the rejected-attempts section the loop wrote into it.
CORTEX_PROMPT_TEXT="$prompt"
export CORTEX_PROMPT_TEXT
printf '%s' "$out" | python3 -c '
import json, os, re, sys

raw = sys.stdin.read()
try:
    v = json.loads(raw)
except ValueError:
    sys.stderr.write("driver: model output was not JSON\n")
    sys.exit(1)
if v.get("is_error"):
    sys.stderr.write("driver: model reported an error\n")
    sys.exit(1)

body = v.get("result") or ""
# EXTRACT THE CODE, wherever the model put it.
#
# The first version only stripped a fence when the response was ENTIRELY a
# fence. Measured against a real run, half the proposals on a hard trial came
# back as prose wrapped around a fenced block — "Looking at the pattern of
# failed attempts... let me try:\n```\n2\n```" — and the whole paragraph was
# passed through as the function body. It cannot compile, the loop rejects it,
# and a proposal is burned on a parse failure that never reached the question.
#
# That inflates generator effort, understates first-shot success, and spends
# money on unparsed prose. It is a transport defect being measured as model
# quality.
# The model's own text, kept before `body` is reassigned. Classifying against
# `raw` would be classifying the JSON envelope the CLI wraps it in, which
# always starts with `{` — the status would be a constant, and a constant
# dressed as a measurement is the failure this field exists to detect.
text = body
fences = re.findall(r"```[a-zA-Z]*\n(.*?)```", body, re.S)
if fences:
    # The LAST block: a model that reasons before answering puts the answer
    # last, and one that shows a rejected alternative first would otherwise
    # have its own discarded attempt submitted.
    body = fences[-1]
    status = ("ambiguous_multiple_fences" if len(fences) > 1
              else "clean_fence" if text.strip().startswith("```")
              else "prose_plus_fence")
else:
    # No fence at all: either the model returned bare code as asked, or it
    # returned nothing usable. `{`/`}` and a newline are weak signals, so the
    # distinction is left to whether there is any non-empty content — the
    # verifier decides whether it COMPILES, which is a different question and
    # a different cost bucket.
    status = "raw_code" if text.strip() else "no_code_found"

prompt = os.environ.get("CORTEX_PROMPT_TEXT", "")
m = re.search(r"^Function: (.+)$", prompt, re.M)
if not m:
    # Fail loudly. A None candidate would silently return the analysis to
    # positional inference, which is the defect this exists to remove.
    sys.stderr.write("driver: prompt has no `Function:` line; cannot "
                     "attribute this proposal to a candidate\n")
    sys.exit(3)
candidate = m.group(1).strip()
log = os.environ.get("CORTEX_MODEL_LOG", "/dev/null")
u = v.get("usage", {}) or {}
with open(log, "a") as fh:
    fh.write(json.dumps({
        "model": list((v.get("modelUsage") or {}).keys()),
        "cost_usd": v.get("total_cost_usd"),
        "duration_api_ms": v.get("duration_api_ms"),
        "input_tokens": u.get("input_tokens"),
        "output_tokens": u.get("output_tokens"),
        "cache_read": u.get("cache_read_input_tokens"),
        "cache_create": u.get("cache_creation_input_tokens"),
        # Which attempt this was, read from the prompt the loop built: the
        # rejected-attempts section is how the loop tells a generator what it
        # has already tried, so its size IS the proposal number.
        "proposal_number": 1 + prompt.count("Attempt ") if prompt else None,
        "prior_rejections": prompt.count("was REJECTED because") if prompt else None,
        "body": body,
        "extraction_status": status,
        # WHICH CANDIDATE this proposal was for, by name. The prompt states it
        # outright; nothing here infers it from position.
        "candidate": candidate,
        "proposal_index_for_candidate": 1 + prompt.count("was REJECTED because"),
    }) + "\n")
sys.stdout.write(body)
'
