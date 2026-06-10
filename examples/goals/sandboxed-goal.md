# Goal: A capability-sandboxed agent whose effect surface the compiler enforces

This goal's **Effect surface** section is not a comment — it carries a structured
`@[contained(...)]` declaration that the compiler stamps onto the generated search
loop. So the prose-declared capabilities are *enforced* (E1001/E1004 at
`axon check`): `try_variant` and everything it calls — `build_prompt`,
`score_output`, `ai_complete` — must stay within the allowed surface, transitively.
The prose boundary becomes a hard boundary. This is the value wedge ("the prose
says no subprocess, the compiler refuses it") applied to a prose goal.

## Intent

Search prompt variants for one that makes the model produce a good answer, while
the agent is confined to a single capability: an LLM call to the model host. No
files, no subprocesses, no other network.

## Inputs

- `text: str` — the input to answer about.

## Outputs

- `answer: str` — the model's answer.

## Score (higher is better)

A non-empty, reasonably-sized answer scores well. The exact formula is in the
Implementation section; it is a pure function of the reply (no I/O of its own).

## Constraints (must hold)

- The agent may ONLY call `ai_complete` (to the model host). Any file, subprocess,
  or other-host access is rejected at compile time by the declared effect surface.

## Budget

- Up to 8 evaluations per run.

## Verify (post-conditions, gated at runtime)

```axon
@[verify(score >= 70)]
```

## Redteam (adversarial test cases)

- None for this demo; the point is the enforced capability boundary.

## Effect surface

@[contained(net: ["api.anthropic.com"], exec: none)]

## Provenance

- Every evaluation is logged to `~/.cache/axon/provenance.jsonl`.

## Implementation (author-supplied Axon bodies)

The author supplies only the scorer, so the default LLM-driven loop is generated —
and the compiler stamps the prose `@[contained]` onto it. `score_output` is pure,
so the only capability the loop needs is the declared LLM network call.

```axon
fn score_output(text: str, answer: str) -> i64 {
    // Reward a non-trivial answer; pure, so it needs no capabilities.
    let n = len(answer)
    if n == 0 { 0 } else { 75 + n % 25 }
}
```
