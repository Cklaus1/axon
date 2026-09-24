# MiCode compatibility note — not a MiCode package update

This review modifies Axon only. The latest supplied MiCode v0.8 ZIP remains unchanged. Before claiming new producer/consumer conformance, add explicit bridge migrations and fixtures for:

1. CX-36 artifact versus detached release identity and `InvocationResult` proposal/validation distinctions.
2. `CorrectnessEstimate` target event versus raw score provenance/calibration status.
3. CX-32 typed evidence closure, invalidation generation and access-filtered incomplete evidence.
4. CX-33 declared treatment changes, separate realized/estimated outcomes and effect-profile axes.
5. CX-34 deployment binding, revocation recheck, budgeted fallback and candidate-versus-active registry status.

Preserve `/build-loop` as MiCode's executor and its existing PermissionGate as local authority. Axon admission never transfers a MiCode grant. Unsupported new fields require a registered migration, an explicitly weaker projection profile, or refusal; never silently drop fields while claiming full conformance.

New gate/source IDs are package-local proposals. Neither system may renumber live governance IDs by guessing. The first bridge fixture may be mock data; actual producer/consumer interoperability remains NOT_RUN.
