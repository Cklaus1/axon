# Changelog — v0.19

- Reviewed Di Zhang's 2026-09-21 RLCD/Jev analysis against Axon v0.18 and official TypeSafe/Jev public documentation.
- Kept ACE v1 and CX-36 r0.2 wire/artifact formats stable.
- Added Outcome-Calibrated Decision Training reference objectives, calibration-registry semantics, proper-score gates and drift control.
- Added FANOUT/JOIN compiler/runtime lowering, packed branch-isolation research gates and high-cardinality staged-decision lineage.
- Added adversarial falsification rules: IIA diagnostic rather than universal invariant; no softmax laundering; no confidence→authority/completion; no online self-promotion.
- Added tasks B200–B209 and the disabled `ace_calibrated_decision` profile.
- Added reference implementation/tests for calibration keys, proper scores, consistency diagnostics and high-cardinality score lineage.
- All product gates remain NOT_RUN.
