# CX-21 v0.23 Forge integration amendment — Forge-assisted coding/compiler evaluation

Coding Frontier evaluation extends to Forge-assisted strategies: semantic transforms, structured repair, migrations, compiler pipelines, BuildPrograms and backend/resource-placement strategies. Evaluator revision and hard constraints remain frozen outside subject authority.

## v0.23 acceptance gates

**G21-r23-forge-benchmark:** Forge-assisted strategies are compared on completed-task correctness plus whole-task latency/cost/iterations and relevant compile/runtime metrics; faster invalid candidates cannot win.

**G21-r23-frozen-evaluator:** The candidate strategy cannot mutate the evaluator, hidden tests, hard requirements or admission rule used to judge that same candidate.

