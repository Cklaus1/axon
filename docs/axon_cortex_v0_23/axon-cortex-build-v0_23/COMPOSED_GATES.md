# Cortex v0.23 composed gate delta

Parent: 504 gates. v0.23 appends 31 gates. Composed total: 535 proposed gates.

# Forge v0.23 acceptance gates

Generated from `integration/V023_GATE_DELTA.json`. Product results are NOT_RUN.

## G00-r23-forge-owner-map

Owner: CX-00  
Every Forge concern maps to one canonical existing authority or an explicitly Forge-owned program-semantic contract; no duplicate global experiment, evidence, capability, admission or resource authority is introduced.

## G00-r23-parent-preservation

Owner: CX-00  
The exact v0.22 parent archive and Forge v0.3 dependency are hash-pinned; the integration delta cannot silently rewrite either input.

## G03-r23-meta-no-mint

Owner: CX-03  
A Forge meta/build/transform request cannot create or widen a capability grant; unknown, stale, revoked or over-budget authority refuses before side effects.

## G03-r23-forge-recheck

Owner: CX-03  
Cached SIR, compiler results or resource projections do not bypass current capability recheck at dispatch or commit.

## G13-r23-meta-isolation

Owner: CX-13  
Meta/build/compiler-plugin execution is capability-scoped and isolated; undeclared filesystem/network/process/secret effects are denied and recorded.

## G13-r23-forge-receipt

Owner: CX-13  
Every protected Forge execution used as evidence binds to an external execution receipt/environment identity rather than a self-asserted Forge success flag.

## G15-r23-sir-boundary

Owner: CX-15  
Source/frontend lowering into Forge SIR has deterministic identity, explicit contracts/effects/resources and traceability; SIR is not confused with IntentIR/AIR.

## G15-r23-transform-abi

Owner: CX-15  
Compiler/program transformations use the Forge Transform/TransformResult ABI with declared preconditions, preserved invariants, semantic diff and verification requirements.

## G15-r23-build-backend

Owner: CX-15  
BuildProgram/backend adapters declare inputs, toolchain/target identity and outputs; backend success cannot bypass semantic verification or external authority.

## G16-r23-structured-repair

Owner: CX-16  
When a supported backend provides structured diagnostics, the MiCode repair loop consumes stable diagnostic/SIR references rather than relying only on raw terminal text.

## G16-r23-forge-loop

Owner: CX-16  
A MiCode Forge episode preserves task/context/authority/evaluator/strategy identities through source -> SIR -> candidate -> compile/test -> Axon evidence submission.

## G17-r23-bootstrap-source

Owner: CX-17  
A bootstrap pack records compiler/language revision and authoritative local source roots; stale or mismatched packs are not silently reused as current language truth.

## G18-r23-forge-promotion

Owner: CX-18  
Forge transforms/migrations/pipelines become native promotion candidates only with independent evidence and existing CX-11/CX-18 admission; no Forge-local winner flag activates code.

## G18-r23-compiler-promotion

Owner: CX-18  
A promoted compiler transform retains source Forge artifact lineage, rollback target and semantic/conformance requirements.

## G19-r23-intent-sir-separation

Owner: CX-19  
IntentIR/AIR and Forge SIR retain distinct schemas, identities and ownership; any lowering/constraint bridge is explicit and evidence-bearing.

## G21-r23-forge-benchmark

Owner: CX-21  
Forge-assisted strategies are compared on completed-task correctness plus whole-task latency/cost/iterations and relevant compile/runtime metrics; faster invalid candidates cannot win.

## G21-r23-frozen-evaluator

Owner: CX-21  
The candidate strategy cannot mutate the evaluator, hidden tests, hard requirements or admission rule used to judge that same candidate.

## G24-r23-truth-order

Owner: CX-24  
When local current compiler/source/conformance evidence conflicts with model prior or stale documentation, the resolver surfaces the conflict and ranks the current local evidence higher for that toolchain revision.

## G24-r23-executable-retrieval

Owner: CX-24  
Retrieved coding examples identify source/revision and, when claimed executable, carry compiler/conformance evidence or remain explicitly unverified.

## G29-r23-strategy-artifact

Owner: CX-29  
Every experimentable Forge/Cortex generation, repair, routing, compiler/build/evaluation/search strategy has stable versioned identity and lineage rather than being an untracked prompt/config mutation.

## G29-r23-frozen-self-eval

Owner: CX-29  
Self-application experiments keep the evaluator/admission rule outside candidate control and preserve a previous-good strategy for rollback.

## G30-r23-semantic-transform

Owner: CX-30  
A repository optimization based on Forge Transform records applicability, semantic diff, exact output artifact and verification requirements before comparison.

## G30-r23-migration

Owner: CX-30  
Large repetitive refactors use a Migration dry-run with scope, rewrite, idempotency expectation and rollback/validation metadata before application.

## G30-r23-build-program

Owner: CX-30  
Programmable build logic produces an inspectable deterministic BuildGraph and does not hide undeclared tool/effect dependencies.

## G32-r23-evidence-projection

Owner: CX-32  
Every Forge observation admitted to learning attaches to a canonical UEG evidence identity; Forge-local records cannot substitute for required evidence lineage.

## G32-r23-provenance-projection

Owner: CX-32  
Forge provenance records stable artifact/input/tool/environment references and remains a projection of CX-32 rather than a parallel lineage database.

## G33-r23-experiment-profile

Owner: CX-33  
A ForgeExperimentProfile references a canonical CX-33 experiment and contains only Forge-domain subject/mutation/compiler/backend/resource references.

## G33-r23-pareto-hard-gates

Owner: CX-33  
Forge experiments preserve hard correctness/security constraints before multi-objective Pareto comparison; no single performance score can rescue an invalid candidate.

## G34-r23-resource-projection

Owner: CX-34  
Every Forge resource node/edge used for placement identifies authoritative CX-34/Fabric source and observation epoch; stale projections are revalidated before protected execution.

## G34-r23-no-second-registry

Owner: CX-34  
Forge cannot register, reserve or authorize physical resources independently of the existing capability/Fabric owners.

## G11-r23-forge-admission

Owner: CX-11  
A Forge candidate with failed semantic/security/acceptance invariants is refused by admission regardless of runtime, cost or judge preference improvements.
