# Forge v0.23 work packages

Generated from `integration/V023_WORK_PACKAGES.json`.

## B287 — Freeze Forge v0.3 input and ownership boundary

Depends on: B255, B284  
Owners: CX-00  
Gates: G00-r23-forge-owner-map, G00-r23-parent-preservation

Pin parent/Forge hashes, owner map and no-duplicate-authority contract.

## B288 — Add canonical StrategyArtifact integration

Depends on: B287  
Owners: CX-29, CX-33, CX-34  
Gates: G29-r23-strategy-artifact

Implement/version the generic strategy identity used by Forge/Cortex experiments.

## B289 — Bind Forge SIR to compiler and IntentIR seams

Depends on: B287  
Owners: CX-15, CX-19  
Gates: G15-r23-sir-boundary, G19-r23-intent-sir-separation

Implement SIR/frontend traceability and explicit IntentIR/SIR boundary.

## B290 — Bind Forge meta execution to existing authority/isolation

Depends on: B289  
Owners: CX-03, CX-13  
Gates: G03-r23-meta-no-mint, G03-r23-forge-recheck, G13-r23-meta-isolation, G13-r23-forge-receipt

Route meta/build/compiler-plugin capability requests through CX-03 and effects through CX-13.

## B291 — Normalize compiler diagnostics and MiCode repair loop

Depends on: B289, B290  
Owners: CX-16, CX-21  
Gates: G16-r23-structured-repair, G21-r23-forge-benchmark

Expose structured diagnostics/SIR references and measure compile-diagnose-repair iterations.

## B292 — Implement Forge coding truth/bootstrap profile

Depends on: B289  
Owners: CX-17, CX-24  
Gates: G17-r23-bootstrap-source, G24-r23-truth-order, G24-r23-executable-retrieval

Bind versioned bootstrap packs and current compiler/source/conformance truth resolution.

## B293 — Implement Transform ABI and semantic diff

Depends on: B289  
Owners: CX-15, CX-30  
Gates: G15-r23-transform-abi, G30-r23-semantic-transform

Register/apply transforms through exact Forge ABI and emit semantic diffs/verification requirements.

## B294 — Implement semantic migration contract

Depends on: B293  
Owners: CX-18, CX-30  
Gates: G30-r23-migration

Add dry-run/idempotent migration artifacts for broad semantic refactors.

## B295 — Implement BuildProgram and backend adapters

Depends on: B290, B293  
Owners: CX-15, CX-30  
Gates: G15-r23-build-backend, G30-r23-build-program

Produce deterministic BuildGraph data and bind backend/toolchain identity.

## B296 — Project Compute Fabric resources into Forge

Depends on: B287, B295  
Owners: CX-34  
Gates: G34-r23-resource-projection, G34-r23-no-second-registry

Expose epoch-bound non-authoritative resource projection without a second registry.

## B297 — Project Forge observations into UEG

Depends on: B287  
Owners: CX-32  
Gates: G32-r23-evidence-projection, G32-r23-provenance-projection

Attach Forge evidence/provenance payloads to canonical UEG identities.

## B298 — Bind Forge profiles to CX-33 causal experiments

Depends on: B288, B297  
Owners: CX-33  
Gates: G33-r23-experiment-profile, G33-r23-pareto-hard-gates

Use canonical experiment identity/lineage/hypothesis and multi-objective hard-gated comparison.

## B299 — Bind Forge candidates to admission/native promotion

Depends on: B293, B294, B298  
Owners: CX-11, CX-18  
Gates: G11-r23-forge-admission, G18-r23-forge-promotion, G18-r23-compiler-promotion

Require independent admission and preserve Forge lineage/rollback on compiler/runtime promotion.

## B300 — Wire end-to-end MiCode Forge episode

Depends on: B291, B292, B293, B295, B297  
Owners: CX-16  
Gates: G16-r23-forge-loop

Run source/SIR/compile/repair/test/evidence path with exact identities and no authority bypass.

## B301 — Validate the v0.23 Forge integration package

Depends on: B287, B288, B289, B290, B291, B292, B293, B294, B295, B296, B297, B298, B299, B300  
Owners: CX-00, CX-21, CX-29  
Gates: G21-r23-frozen-evaluator, G29-r23-frozen-self-eval

Run offline package, schema, owner-map, crosswalk, dependency and negative conformance checks.

## B302 — Qualify one bounded Forge semantic optimization pilot

Depends on: B271, B276, B301  
Owners: CX-21, CX-30, CX-33  
Gates: G21-r23-forge-benchmark, G30-r23-semantic-transform, G33-r23-experiment-profile

On a fixed task/evaluator/toolchain, compare incumbent versus one Forge semantic candidate and retain truthful PASS/FAIL/INCONCLUSIVE evidence; no improvement claim is forced.
