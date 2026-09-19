# Cortex skills

Bug-finding STRATEGIES, not facts about Axon. Each is a way of looking that
found a real defect here and should transfer to another compiler or runtime.

They are data (`*.json`), not prose, because the intent is for Cortex to select
among them by trigger. Nothing consumes them yet — `skills_are_well_formed_and_cite_real_evidence`
validates them, and that is deliberately ALL that is claimed. A skill set with
no consumer is honest; a skill set that claims a consumer it does not have is
the defect these skills are about.

Schema (enforced by the test, which fails on any missing or empty field):

| field | meaning |
|---|---|
| `id` | unique kebab-case identifier |
| `trigger` | the observable condition that should bring this to mind |
| `probe` | ordered, MECHANICAL steps — a human or agent should be able to run them without judgement |
| `priority` | when this outranks other things to look at |
| `evidence` | ≥1 entry, each with a `file` that MUST EXIST and a `finding` |
| `transfers` | why it is not Axon-specific |
| `limits` | what it does NOT find — required, so a skill cannot oversell itself |
