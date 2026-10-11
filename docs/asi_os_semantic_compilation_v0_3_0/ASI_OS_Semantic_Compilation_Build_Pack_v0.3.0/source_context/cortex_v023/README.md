# Axon Cortex Build Files — v0.23

## Forge semantic software evolution integration

v0.23 is a **preservation-first integration overlay** over the validated Axon Cortex v0.22 package. It does not rewrite the largely completed 0.22 specification and it does not absorb Forge into Cortex.

This archive embeds two immutable inputs:

- `parent/Axon_Cortex_Build_Files_v0_22.zip` — the exact supplied 0.22 parent;
- `dependencies/Axon_Forge_Build_Files_v0_3.zip` — the standalone Forge 0.3 normative package.

The v0.23 delta adds owner amendments, task/gate deltas, strategy-artifact and binding schemas, Forge/Cortex crosswalks, runtime seams and conformance tests.

### Ownership

- **Forge:** program semantics, SIR, meta/compiler API, transforms, migrations, semantic diff, build/backend contracts, diagnostics, bootstrap/truth resolution.
- **Cortex:** cognition, higher-order reasoning, self-application policy and strategy proposal.
- **Axon/CX-33:** canonical experiment identity/causal comparison.
- **CX-32:** canonical evidence/provenance graph.
- **CX-03/CX-13:** authority and isolated effects.
- **CX-11/CX-18:** admission/native promotion.
- **CX-34/Compute Fabric:** physical resource truth.
- **MiCode/CX-16:** coding workflow.

This means Forge becomes the semantic engine under repository/compiler optimization while Cortex/Axon remain the learning and governance system above it.

## Validate

```bash
python -B tools/validate_package.py
python -B -m unittest discover -s tests -v
```

These commands validate the document/integration package only. They do not claim Forge or Cortex runtime implementation, activation or measured improvement.
