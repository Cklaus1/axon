# CX-29 v0.23 Forge integration amendment — strategy artifacts and self-application

Compiler pipeline, build, repair, retrieval, meta-execution and resource-placement strategies are versioned `StrategyArtifact` candidates. Cortex may propose and compare them but a candidate strategy cannot change the frozen evaluator/admission authority judging itself.

## v0.23 acceptance gates

**G29-r23-strategy-artifact:** Every experimentable Forge/Cortex generation, repair, routing, compiler/build/evaluation/search strategy has stable versioned identity and lineage rather than being an untracked prompt/config mutation.

**G29-r23-frozen-self-eval:** Self-application experiments keep the evaluator/admission rule outside candidate control and preserve a previous-good strategy for rollback.

