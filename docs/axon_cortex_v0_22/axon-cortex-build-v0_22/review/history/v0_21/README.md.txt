# Axon Cortex build files — v0.21

**Status: proposed contracts and build plan, with executable model-free conformance fixtures. Not a completed runtime release.**

This is an additive upgrade of the uploaded v0.20 pack, reviewed against `axon-ai-context-20260924.zip`. It preserves all 37 CX specifications and B00–B222, adds B223–B254, and contains **255 work packages and 424 proposed product gates**. Source review does not reset repository implementation evidence. Every gate in this portable package remains `NOT_RUN` until a real source run supplies separately qualified evidence.

## Start here

Read the [source and baseline review](review/SOURCE_REVIEW_V021.md), [upgrade procedure](build/UPGRADE_V021.md), [research integration contract](build/RESEARCH_INTEGRATION_V021.md), then the [bootstrap prompt](build/BOOTSTRAP_PROMPT_V021.md). The [requirements matrix](integration/RESEARCH_REQUIREMENTS.json) maps all eight requested research families to existing owners, work packages and gates. The [source registry](integration/RESEARCH_SOURCES.json) distinguishes verified reference material, pinned commits, missing execution pins and unreplicated claims.

| Navigation alias | Existing primary owner | New profile, not a new authority |
|---|---|---|
| EVO | CX-29 | RRSI-inspired regularized harness evolution |
| DEC | CX-05 | CLM and Jev/RLCD typed scoring, with pijev robustness |
| EVL | CX-01 | CheckEval-inspired evidence-linked checklists |
| RTR | CX-06 | Model/role routing with TinyRouter, TRINITY and Semantic Router candidates |
| CVM | CX-28 | Cliff-inspired canonical-history working-set projection |
| SPX | CX-26 | Bounded RLM-Cascade-inspired speculative proposals |
| TEL | CX-10 | Whole-task usage and economic evidence through existing owners |

Names of research projects remain in provenance, provider adapters and qualification recipes. These aliases do not create seven services, a new action registry, a new event store, or a competing promotion authority. See the [ownership map](integration/SOURCE_OWNER_MAP.json).

## What is preserved

CX-36 r0.2, `axon.nps/1`, `axon.np/1`, `axon.cjson/1`, and existing ACE v1 JSON schemas are byte-preserved. Existing `axon-reflex/1` is not silently version-bumped; new capability semantics require explicit negotiation. Cargo workspace version `0.1.0` is not the build-pack version and must not be changed merely to match `0.21`.

The source has a hardened Reflex serving seam and an existing deterministic repair/authorization loop. Its Reflex decision core is still a protocol fixture, not a CLM implementation. Live source calls this work CX-35 while the package reserves CX-35: the upgrade records an alias and preserves source naming rather than renumbering code.

## Bounded delivery

`v021_package_conformance` needs no model, GPU, Rust compiler or peer. `v021_guardrails` requires real deterministic source integration. `v021_provider_qualification` and `v021_live_pilot` are separately authorized runtime work. B251–B254 are optional research and are excluded from bounded nonresearch closures. Learned-model success is not a prerequisite for accepting the document pack.

MiCode support **0.15 is a proposed consumer target only**. No MiCode source or support pack was uploaded for this review. The included [consumer delta](integration/MICODE_V015_PROPOSED_DELTA.md) is not an updated or tested MiCode release.

## Validate this pack

Run in a trusted, isolated Python environment with `jsonschema` available:

```bash
python -B tools/validate_package.py
python -B -m unittest discover -s tests -v
python -B tools/plan_upgrade.py --repo /path/to/axon
```

The last command is read-only and prints JSON. It does not extract archives, alter source, run project scripts, install providers or enable a proxy. Checksums detect changes relative to this package; authenticate the received ZIP separately before executing its Python tools. Do not refresh hashes merely to silence an unexplained mismatch.

After intentional author edits, rebuild views, update owner-export hashes through the documented release procedure, rerun tests, then explicitly reseal. The package validator never executes product gates. Test receipts and limitations are in [validation evidence](review/VALIDATION_V021.md).

## Contents

The full [master compilation](AXON_CORTEX_MASTER.md) is generated from active source documents. Detailed [tasks](build/TASKS.md), [gates](build/ACCEPTANCE_GATES.md), seven profile guides, [provider qualification plans](build/PROVIDER_QUALIFICATION_V021.md), strict reference schemas, fixtures, a non-mutating upgrade planner and review evidence are included. Original archive bytes and third-party model weights are not redistributed.
