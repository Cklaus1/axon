# Causal and Active Experimentation build guide

Implement CX-33 first on low-risk replayable cognitive policies. Every protected experiment freezes treatment, controls, metrics, stopping rule, causal assumptions and corpus roles before protected outcomes are inspected.

Preferred sequence:

```text
replay matched episodes
→ compare one declared delta
→ estimate information gain / cost / risk
→ live no-effect shadow
→ protected analysis
→ ImprovementIntent / reject
```

Use resettable direct interventions where practical. Treat simulations and off-policy estimates as counterfactual evidence until reproduced. Integrate reversibility classes into authority requirements.

## v0.16 causal class correction

A counterfactual estimate never becomes a realized outcome merely because an estimator was validated. Actual observation remains required. Match upstream state, task and authority; when the treatment changes representation, prompt, working-set or candidate construction, record both effective inputs as part of the intervention. Do not pretend they were identical. Pre-register the estimand, family-level analysis and stopping rule before protected evaluation.
