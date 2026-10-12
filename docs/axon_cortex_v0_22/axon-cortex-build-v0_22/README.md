# Axon Cortex Build Files — v0.22

**Closed-Loop Integration: Axon 0.21 profiles + Compute Fabric M0–M2 + the supplied MiCode source.**

**Status: proposed specifications and implementation plan, with executable offline conformance/reference tools. This archive is not an implemented or activated runtime.**

This is the complete additive successor to v0.21, not just an assessment. It retains the **37 CX specifications**, all **255 prior tasks** and **424 prior gates**, and appends **B255–B286: 32 work packages and 80 CX gates**. The main manifests now contain **287 tasks and 504 proposed gates**. The bundled, byte-preserved Compute Fabric v0.1 reference retains its own **14 tasks and 55 gates**; **9 tasks / 38 gates (M0–M2)** are mapped to the bounded integration. These namespaces must not be double-counted as newly implemented features.

## Begin here

Read [the release contract](build/CLOSED_LOOP_INTEGRATION_V022.md), [source review](review/SOURCE_REVIEW_V022.md), [work packages](build/WORK_PACKAGES_V022.md), [migration procedure](build/UPGRADE_V022.md), then [the implementation-agent prompt](build/BOOTSTRAP_PROMPT_V022.md). The paired [MiCode consumer delta](integration/MICODE_V022_CONSUMER_DELTA.md) and [Fabric integration addendum](integration/FABRIC_V022_INTEGRATION.md) are delivered in this archive, not left as a follow-up.

| Release profile | Meaning |
|---|---|
| `v022_package_conformance` | Documents, source pins, schemas and model-free executable reference tests. |
| `v022_protected_execution` | One real registered check through a physically qualified Linux microVM profile. |
| `v022_peer_roundtrip` | Actual Axon/MiCode policy and episode paths with context/authority/cost evidence. |
| `v022_operational_closed_loop` | Real bounded experiment, independent admission/refusal, future-task policy use and rollback. |
| `v022_improvement_study` | Optional independent study; a benefit claim is not guaranteed or required. |

Only the first profile is exercised by this document build. Runtime qualification fields remain false and all product gates are `NOT_RUN`. Use [the validation report](review/VALIDATION_V022.md) for actual commands/results and limitations.

## Build validation

The delivered offline suite passes **567 tests** (367 retained plus 200 added), with no failures or skips. Both source inventories match unchanged: **1,213 Axon files and 859 MiCode files**. The original Fabric validator also passes its synthetic checks. These results validate the build pack and reference behavior, not the live three-system loop.

## Stable ownership

EVO/DEC/EVL/RTR/CVM/SPX/TEL remain aliases over existing CX owners, not seven new services. Axon proposes and independently admits policies. MiCode retains local permissions and its coding workflow. Fabric manages authorized compute effects and receipts. Verifiers evaluate exact artifacts outside subject control. Neither a policy nor a receipt is a bearer grant.

The first pilot changes only tool/skill shortlisting within an already-allowed set. Model, task family, verifier, authority, compute profile, budget policy and context settings are fixed. Unqualified CLM/Jev/router/cascade alternatives remain disabled or shadow-only; the historical B250 all-provider pilot is not the new bounded release's dependency closure.

## What is preserved

CX-36 r0.2, neural formats, ACE v1 schemas, all prior task/gate rows, prior research provenance, and the original Fabric reference bytes are preserved. Existing source CX-35 is not renumbered. The Axon and MiCode Cargo workspace versions are **not** changed to 0.22. Historical MiCode v0.15 proposal files are retained as history; the supplied peer snapshot embeds a v0.3 support roadmap, so this release provides a **0.22 consumer delta**, not a fabricated global MiCode support release.

## Validate and plan the upgrade

Authenticate the ZIP independently before running any bundled code. Then, from the extracted package root in an isolated Python environment with `jsonschema` installed:

```bash
python -B tools/validate_package.py
python -B -m unittest discover -s tests -v
python -B tools/plan_upgrade_v022.py --axon /path/to/axon --micode /path/to/micode
python -B tools/check_v022_runtime_evidence.py --evidence fixtures/closed_loop_v022/empty-runtime-evidence.json
```

The last command is intentionally **NOT QUALIFIED / exit 2** for the delivered empty evidence. It must never turn documentation tests into live product PASS. The source planner is read-only. Neither command extracts over existing repositories or runs their scripts.

## Package map

| Path | Contents |
|---|---|
| `specs/` | Existing CX owners, with 0.22 amendments and 80 new gates. |
| `task_manifest.json`, `gate_manifest.json` | Authoritative append-only work and gate rows. |
| `build/WORK_PACKAGES_V022.md` | Implementation steps, source seams, test recipes and stop conditions. |
| `schemas/CLOSED_LOOP_PROFILE_V022.md` | Versioned sidecar proposal and trust semantics. |
| `schemas/json/closed-loop-*.schema.json` | Inert proposed exchange/plan projections; not stable production ABI. |
| `integration/compute-fabric-v0_1-reference/` | All original Fabric files, byte-preserved. |
| `integration/V022_*` | Paired source locks, ownership/crosswalks, requirement coverage and unqualified runtime ledger. |
| `review/` | Source evidence, reviews, actual package-test logs, history and limitations. |
| `tools/`, `tests/`, `fixtures/closed_loop_v022/` | Offline semantics, negative tests, read-only upgrade/evidence tools. |

The master document is generated from the package's Markdown sources. Historical documents remain visibly historical; the current README, 0.22 addenda and declared release profiles determine this upgrade's scope. A stricter new pilot rule does not silently change historical schemas or recorded hashes.
