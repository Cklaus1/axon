# ACE integration review and adversarial cases — v0.18

## Design findings folded in

1. **Owner inversion:** MiCode v0.10 anticipated upstream physical records. v0.18 now owns the scoped profile; v0.12 consumes and pins it. Native schemas still need live mapping.
2. **Older artifact encoding:** ACE referenced original CX-36. Preserve reviewed r0.2 bytes, domain-separated identity and detached release; never import the reference ZIP profile.
3. **Packing order:** freeze executable bytes before evaluation; attach evaluation/admission afterward without changing tested bytes.
4. **Interface privilege merger:** ACE's combined conceptual backend list must not join compiler, inference, publication and activation privileges.
5. **Axis ambiguity:** node/accounting/operation/capability/mechanism/deployment are independent. Freeze a small complete mapping; unsupported combinations refuse rather than invent opcodes.
6. **Result/provenance loss:** explicit families, complete scores and raw tags are preserved. Distribution, named-event correctness and OOD remain different. Known aliases require matching mechanisms.
7. **Physical claims:** feature flags, batch size and residency do not prove one-pass, actual KV reuse or zero billing. Real conformance is a separate profile.
8. **Build scope:** core with one real incumbent does not require a trained head, Neural Program, three-arm experiment or native-state service. Existing dependencies were not silently removed.

## Adversarial review folded into gates and tests

| Attack | Required defense / product gate |
|---|---|
| Reuse legacy ACE .np metadata as current admission | unchanged CX-36 plus G23-ace-lifecycle |
| Inject a fake Rule/native/NeuralProgram combination | tuple mapping, G04-ace-lowering |
| Claim unsupported or emulated complete scores | independent features, G05-ace-features |
| Drop top-k candidates then renormalize | exact full candidate set, G05-ace-results |
| Switch raw generated probability to token/head provenance | original tag + mapping + mechanism, G06-ace-provenance |
| Select wrong argmax or substitute expected ordinal number | complete scores/tie/level validation, G05-ace-results |
| Reuse a state across tenant/adapter/epoch/lease | G28-ace-reuse; synthetic checks do not prove engine isolation |
| Return stale or duplicate branch results | G04-ace-joins |
| Commit a canceled reply or retry unknown remote compile | G13-ace-attempts and CX-36 job reconciliation |
| Hide repair/fallback work or unknown cost | G32-ace-attempt-lineage / G34-ace-fallback |
| Revoke between selection and dispatch | G34-ace-dispatch |
| Hide network work behind localhost/offline | G13-ace-locality |
| Make new optional research block first useful slice | dependency-closure checks, G00-ace-coverage |
| Treat fixture PASS as live peer/model evidence | G16-ace-core / G16-ace-peer remain NOT_RUN |

No attack was run against a live product, backend, issuer or kernel. Tests exercise only inert record/format/package rules. Source obligations and amended owner locations are machine-indexed in the crosswalk. Live D01/D07/D08 remain unresolved; D02/D04 are resolved only for the bounded document subset.
