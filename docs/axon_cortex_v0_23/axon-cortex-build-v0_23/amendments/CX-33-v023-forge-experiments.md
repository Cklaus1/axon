# CX-33 v0.23 Forge integration amendment — Forge causal experiments

Forge supplies domain-specific experiment profiles while CX-33 owns canonical experiment identity, parent lineage, hypothesis/falsifier, controlled mutation, evaluator, metrics, confidence and causal comparison. Forge may request experiments but cannot self-admit results.

## v0.23 acceptance gates

**G33-r23-experiment-profile:** A ForgeExperimentProfile references a canonical CX-33 experiment and contains only Forge-domain subject/mutation/compiler/backend/resource references.

**G33-r23-pareto-hard-gates:** Forge experiments preserve hard correctness/security constraints before multi-objective Pareto comparison; no single performance score can rescue an invalid candidate.

