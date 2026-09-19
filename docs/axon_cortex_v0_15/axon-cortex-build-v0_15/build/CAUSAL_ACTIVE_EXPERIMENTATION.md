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
