# Post-C9: key formats, signature format and key endorsement

Status: **DRAFT, not normative, not in C9 scope.** Recorded 2026-10-04 at the operator's request
(decision I and its two companions J and K). Nothing here changes what C9 accepts. C9 keeps
lowercase 64-hex Ed25519 public keys and raw detached Ed25519 signatures. The authoritative
description of the C9 formats is `governance/notes/v022-key-formats.md`, which is derived
from the loaders.

These items touch trust-root parsing, which is in the protected TCB. Each one needs its own
amendment, rows and a review round before it lands. No item may weaken an accepted C9 property.

## I. Role-typed, checksummed public keys

**Problem.** A public key is 64 hex characters with no role. Nothing in its text says whether it
belongs to the qualification signer, the verifier, an evidence issuer or the observer. A key
pasted into the wrong trust root fails late, or not at all, and a truncated paste is caught
only by the length check. The format is parsed at six or more sites across three crates
(`operator_trust.rs`, `attestation.rs`, `store.rs`, `privileged_launcher.rs`, `backend.rs`).

**Proposal.**
- **Text form:** `ax<role>_pk_<43-char base64url of the 32 key bytes, no padding>`, optionally
  with a checksum (a bech32-style `ax<role>1…` with a BCH checksum, or a 4-byte truncated
  SHA-256 suffix). The role letter is one of `q` (qualification), `v` (verifier), `e` (evidence
  issuer), `o` (observer). The list is closed, and adding a role is an amendment.
- **One shared parser** `parse_public_key(text, expected_role)`, which every site calls. A drift
  test fails when a loader decodes key text any other way. Fix it at the primitive, never by
  listing call sites (see the "fix at the source" rule).
- **The parser refuses:**
  - a wrong role for the trust root being loaded;
  - a bad checksum;
  - wrong length;
  - non-canonical text (padding, mixed case where case matters, surrounding whitespace beyond
    one trailing newline);
  - a fingerprint (`ed25519:<16 hex>`) offered as a key. A fingerprint is a display label, never
    an identity (precedent: A91, the 32-bit lineage abbreviation).
- `keygen --role <r>` emits the typed form. The fingerprint stays display-only.

**Open decisions (operator):**
- the checksum scheme;
- whether 64-hex stays accepted during a migration window, and for how long (it should be a
  dated, logged acceptance that is refused after expiry, never silent);
- whether one key may legitimately serve two roles (recommended: no).

**Evidence needed:** a row per refusal (wrong role, bad checksum, length, non-canonical text,
fingerprint-as-key, a bypassing call site), each killed by its own attack, and a negative-matrix
row for "a key of role X is accepted in the trust root of role Y".

## J. An interoperable signature container

**Problem.** Records (the B263 record, waivers, certification records, attestations) are signed
with raw Ed25519 and a bespoke `.sig`. Only our own binary can verify them. An independent
auditor has to trust the tool that is being audited.

**Proposal.** Keep Ed25519 and adopt a standard container that stock tools verify:
- **minisign** (Ed25519, with trusted and untrusted comments, prehashed mode), or
- **SSH signatures** (`ssh-keygen -Y sign/verify`, which has a namespace field that maps
  naturally onto the role).

Our verifier keeps doing the canonical-form check (which exact bytes are signed). The container
only standardises the envelope. Signing a canonical serialisation stays mandatory, so two
renderings of one record cannot carry different meanings under one signature.

**Open decisions (operator):**
- minisign or SSH signatures (SSH namespaces align with role typing in I);
- whether to dual-sign during migration.

**Evidence needed:**
- a test where a stock tool (`minisign -V`, `ssh-keygen -Y verify`) verifies a record our
  binary produced;
- a test where our verifier refuses a record the stock tool accepts but whose canonical form
  differs (an envelope-valid signature over non-canonical bytes);
- rows for each.

## K. A root key that endorses role keys

**Problem.** Every role key is installed directly as a trust root. Rotating the observer or
verifier key means editing `/etc/axon/trust/*` on every protected host. Nothing ties the role
keys to the operator except where the files happen to sit.

**Proposal.**
- **Endorsements:** an offline operator root key signs short-lived endorsements:
  `{role, public key, not_before, not_after, serial}`, in the canonical form, in J's container.
- **Install and check:** hosts install only the root's public key, plus endorsements, which do
  not need to be secret. Loaders accept a role key only with a valid, unexpired endorsement for
  that role.
- **Revocation:** a signed revocation list, or short endorsement lifetimes. This is decided with
  the operator.
- **Qualification key as root:** the qualification key generated on 2026-10-04 could become this
  root, or remain a role key under a new root. That is the operator's decision.

**Open decisions (operator):**
- whether the root is the existing qualification key or a new one;
- endorsement lifetime;
- the revocation mechanism;
- the root's custody (offline, hardware token).

**Evidence needed:** rows for an expired endorsement, a wrong-role endorsement, an endorsement by
a non-root key, a revoked key, and a role key with no endorsement, plus a negative-matrix row
for "an unendorsed key is accepted".

## Ordering

I, then J, then K. K's endorsements are typed records (I) in a standard container (J). Each item
is its own branch, amendment and review round after the C9 certification. None of it may start
in the C9 freeze window.
