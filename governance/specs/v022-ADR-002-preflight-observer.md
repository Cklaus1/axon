# ADR-002: the protected preflight observer

Status: **ACCEPTED** by the operator, 2026-09-27. This complements ADR-001 (D1 profile separation,
D2 authority hybrid, D5 key custody, D6 custodian deployment) and does not amend it.

## Decision

The preflight observer is its **own protected control-plane identity**. It is not MiCode, not
Claude, not the candidate, and not Fabric.

- **Service.** A dedicated systemd service, `axon-preflight-observer`, runs as its own uid
  `axon-observer` on socket `/run/axon/preflight-observer.sock`, on the PROTECTED host.
- **Key.** The service holds its own online Ed25519 key, readable only by `axon-observer` and
  scoped to preflight-observation attestations.
- **Authorization.** The operator's offline root key authorizes the observer's public key (D5):
  the root private key never reaches a service or an agent.
- **Distinct keys.** The observer key is not the B263 qualification key, the verifier key, the
  admission key or the custodian key. Distinct keys give distinct compromise boundaries.
- **Read-mostly.** The observer cannot edit candidate workspaces, change policies or admit
  anything.

## Why not Fabric

Fabric launches the protected environment. If Fabric also asserted that the environment was
protected, that would be self-attestation. The flow is:

```
custodian requests an observation (fresh nonce, epoch)
        → observer inspects and SIGNS the observed state
        → custodian verifies the observation
        → Fabric launches exactly the bound configuration
        → Fabric's receipt references the observation digest and the launch manifest digest
        → the guest verifier's evidence joins back to the same launch/preflight digest
        → intake verifies the joins
```

Fabric consumes an observation and cannot mint one.

## What an observation binds (non-replayable, one launch)

```
PreflightObservation {
    schema, observer_key_id, nonce, epoch, timestamp,
    host_profile, fabric_revision, firecracker_revision,
    guest_image_digest, guest_kernel_digest,
    verifier_digest, suite_digest, policy_digest,
    intended_launch_manifest_digest
}   — signed by the observer
```

The receipt carries `preflight_observation_digest` and `launch_manifest_digest`. An observation from
another launch, another guest image, or yesterday authorizes nothing.

## Separation from B263

The two answer different questions:

| | Question | When |
|---|---|---|
| **B263** (operator-signed qualification) | Has this protected execution PROFILE been qualified? | once per profile |
| **Preflight observation** | Is THIS launch actually using the qualified configuration? | per protected execution |

## Consequences for the code

1. **Now (DEV-buildable):**
   - no public key is registered for two roles;
   - an observer identity holds no verifier, admitter or monitor role;
   - a PROTECTED-class context counts only if signed by a registered observer. This is already
     enforced, and is G32.
2. **With the protected backend (microVM suite-verdict workstream):**
   - the `PreflightObservation` schema;
   - the custodian's nonce;
   - the receipt fields;
   - the intake/EVL joins;
   - root-signed authorization of `observer_keys` entries.

   These are certified in the PROTECTED environment, not on this DEV host (ADR-001 D1).
