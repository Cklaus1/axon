# Universal Evidence Graph build guide

Implement CX-32 as a typed append/index layer over existing durable artifacts, not as a second source of truth. Start with one resettable repair episode and map intent, observation, working-set receipt, decision, action, test evidence and verifier claim into immutable nodes/typed edges.

Priorities:
1. immutable node/edge schemas and content digests;
2. claim-strength typing;
3. effective-input closure;
4. contradiction and invalidation propagation;
5. authorization-aware evidence queries;
6. admission-subgraph export for CX-11.

Do not rewrite raw source evidence, infer proof from confidence, or let graph queries bypass project/tenant disclosure policy.
