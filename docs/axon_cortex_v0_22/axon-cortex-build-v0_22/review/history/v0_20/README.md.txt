# Axon Cortex build package — v0.20

**Proposed design/build plan with executable model-free reference fixtures. All product gates remain NOT_RUN.** This coordinated amendment adds schema-driven decisions and lightweight fixed-task Reflex distillation to existing owners. It preserves the v0.19 RLCD/Jev work, ACE v1 wire schemas and byte-identical CX-36 r0.2 source/artifact contracts.

Paired consumer: **MiCode Axon Support v0.14**. The current package has 37 specifications, 223 proposed work packages and 360 product gates. New work is B210–B222; the final two are optional research. A learned Reflex can be a `.np`; a schema-generated decision plan is not automatically a trained/admitted artifact. No `.reflex` format or competing runtime is added.

## Start here

[Apply both packs](build/APPLY_V020_WITH_MICODE_V014.md) · [Design review](review/JIMOTHY_WEBMCP_DESIGN_REVIEW.md) · [Adversarial review](review/JIMOTHY_WEBMCP_ADVERSARIAL_REVIEW.md) · [Requirement map](integration/SCHEMA_REFLEX_REQUIREMENTS.json)

[Schema profile](schemas/SCHEMA_DECISION_PROFILE.md) · [Lightweight training/qualification profile](schemas/LIGHTWEIGHT_REFLEX_PROFILE.md) · [Schema build loop](build/SCHEMA_DECISION_FRONTEND.md) · [Distillation build loop](build/LIGHTWEIGHT_REFLEX_DISTILLATION.md)

[Spec index](specs/INDEX.md) · [Task DAG](build/TASKS.md) · [Acceptance gates](build/ACCEPTANCE_GATES.md) · [Release slices](build/IMPLEMENTATION_RELEASE_SLICES.md) · [Master](AXON_CORTEX_MASTER.md)

## Bounded first paths

`schema_decision_contract` implements the closed scalar/optional/source-span subset with active-output, current identity and local authority checks. `lightweight_reflex_shadow` compares one fixed-domain function against the incumbent and simpler baselines with independent data/calibration/threshold/test roles. `v020_package_conformance` performs model-free document/fixture intake. Qualified deployment and real paired MiCode use have separate profiles; richer encoders/context/routing/schema research does not block these entry points.

## Validate without changing the package

```sh
python -B tools/package_views.py
python -B tools/validate_package.py
python -B -m unittest discover -s tests -v
```

After intentional reviewed edits, regenerate views, owner export/member hashes and downstream pins before resealing. To record test results use `python -B tools/run_review_tests.py`, then reseal with `python -B tools/validate_package.py --refresh-hashes --output package_validation.json`. Do not reseal an unexplained integrity failure as though it were validation.

These checks execute no model, teacher call, tool action, browser bridge, sandbox or real consumer. They establish pack/reference consistency, not training quality, calibration representativeness, runtime isolation, live interoperability or product acceptance. Preserve live evidence when importing into existing repositories.
