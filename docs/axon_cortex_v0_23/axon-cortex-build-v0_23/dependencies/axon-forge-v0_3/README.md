# Axon Forge Build Files v0.3

**Forge v0.3 is the standalone program-semantic and programmable-compiler substrate for Axon/MiCode.**

v0.3 deliberately removes the ownership overlap present in v0.2. Forge does **not** own a second experiment engine, evidence graph, capability authority, admission system, or physical resource registry. Those remain with Axon/Cortex/Compute Fabric. Forge owns the software-semantic machinery those systems can reason about and optimize.

## Forge owns

- Semantic IR (SIR) and semantic diff;
- transformation ABI and compiler-pass artifacts;
- controlled meta-execution (`#run`-class capability without Jai-specific syntax);
- semantic migrations;
- structured compiler diagnostics and compile/repair feedback;
- programmable `BuildProgram -> BuildGraph` contracts;
- compiler/backend interfaces;
- local executable truth resolution and AI bootstrap packs;
- Forge-specific conformance.

## Forge delegates

- experiments and causal lineage -> **Axon / CX-33**;
- evidence/provenance graph -> **CX-32 UEG**;
- capability grants -> **CX-03**;
- isolation/execution authority -> **CX-13**;
- promotion/admission -> **CX-11/CX-18**;
- physical compute inventory -> **Compute Fabric / CX-34**;
- higher-order self-improvement reasoning -> **Cortex / CX-29**;
- coding workflow -> **MiCode / CX-16**.

The durable rule is:

> **Semantic invariants are contracts. Implementations and optimization strategies are candidates. External Axon owners decide evidence, authority, and promotion.**

Start with `SPEC.md`, `OWNERSHIP.md`, `schemas/`, `interfaces/`, and `conformance/`.
