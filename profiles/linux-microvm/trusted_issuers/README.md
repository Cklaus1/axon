# Trusted evidence issuers for `linux-microvm-protected`

`axon-fabric` treats a B263 qualification record as evidence only when a
detached `axon-evidence-signature/1` over its exact bytes verifies under an
Ed25519 **public** key in this directory (one `<name>.pub` file per issuer,
64 hex characters). Waivers for BLOCKED assertions (`axon-b263-waiver/1`) are
verified the same way.

This directory holds **no key** today. Until the operator installs one and
signs a re-qualification (S3-6), Fabric refuses the protected profile — that is
the intended outcome. The signing (private) key is held by the operator
(decision D6) and must never be committed here.
