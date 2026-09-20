#!/usr/bin/env python3
"""STAGED driver patch — apply ONLY when no real-model run is in flight.

The driver is invoked once per model call, so editing it mid-run changes the
treatment and the digest in the provenance line stops describing the
experiment that produced the data.

Two changes, both for joining evidence by identity instead of by order:

1. `extraction_status` — how the body was recovered from the response, so a
   strange model answer is distinguishable from a driver that mangled a
   conventional one. That distinction is what voided an earlier run.

2. `candidate` / `proposal_index_*` — which candidate the proposal was FOR,
   read from the prompt's own `Function:` line. Proposal ownership was
   previously inferred from position in the stream, which is wrong whenever a
   candidate is selected and never reaches the generator; measured, that
   reported "ranking waste is 100% of inference spend" with half the
   proposals on the true target.

   Read from the prompt rather than from a new Cortex field because Cortex is
   frozen for this experiment. It is still IDENTITY — the symbol is named
   explicitly — but it is coupled to the prompt's wording, so the extraction
   asserts it matched rather than defaulting to None. A silent None here would
   reintroduce exactly the positional fallback this removes.
"""
import hashlib, os, re, sys

D = "crates/axon-cortex/benchmarks/drivers/claude_cli.sh"
s = open(D).read()
before = hashlib.sha256(s.encode()).hexdigest()[:16]
if before != "14f975119008ca2e":
    sys.exit(f"driver digest is {before}, expected 14f975119008ca2e — "
             "it changed since this patch was written; re-derive it")

# --- 1 + 2: classify extraction, and name the candidate -------------------
old = """fences = re.findall(r"```[a-zA-Z]*\\n(.*?)```", body, re.S)
if fences:
"""
new = """# The model's own text, kept before `body` is reassigned. Classifying against
# `raw` would be classifying the JSON envelope the CLI wraps it in, which
# always starts with `{` — the status would be a constant, and a constant
# dressed as a measurement is the failure this field exists to detect.
text = body
fences = re.findall(r"```[a-zA-Z]*\\n(.*?)```", body, re.S)
if fences:
"""
assert s.count(old) == 1, "fence-find anchor drift"
s = s.replace(old, new, 1)

old = """    body = fences[-1]
"""
new = """    body = fences[-1]
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
"""
assert s.count(old) == 1, "body-assign anchor drift"
s = s.replace(old, new, 1)

old = '        "body": body,\n'
new = """        "body": body,
        "extraction_status": status,
        # WHICH CANDIDATE this proposal was for, by name. The prompt states it
        # outright; nothing here infers it from position.
        "candidate": candidate,
        "proposal_index_for_candidate": 1 + prompt.count("was REJECTED because"),
"""
assert s.count(old) == 1, "log-record anchor drift"
s = s.replace(old, new, 1)

old = 'prompt = os.environ.get("CORTEX_PROMPT_TEXT", "")\n'
new = """prompt = os.environ.get("CORTEX_PROMPT_TEXT", "")
m = re.search(r"^Function: (.+)$", prompt, re.M)
if not m:
    # Fail loudly. A None candidate would silently return the analysis to
    # positional inference, which is the defect this exists to remove.
    sys.stderr.write("driver: prompt has no `Function:` line; cannot "
                     "attribute this proposal to a candidate\\n")
    sys.exit(3)
candidate = m.group(1).strip()
"""
assert s.count(old) == 1, "prompt anchor drift"
s = s.replace(old, new, 1)

# The driver imports `json, os, re, sys` on one line, so a naive
# `"import re" in s` is FALSE on a file that does import it — the first dry
# run failed here, which is the whole reason this patch is dry-run before it
# is applied to a driver under measurement.
assert re.search(r"^import .*\bre\b", s, re.M), "driver must already import re"

open(D, "w").write(s)
after = hashlib.sha256(s.encode()).hexdigest()[:16]
print(f"driver patched: {before} -> {after}")
print("NEXT: record the new digest in the provenance line before trial 1,")
print("      and add `candidate` to real_model.py's per_proposal record.")
