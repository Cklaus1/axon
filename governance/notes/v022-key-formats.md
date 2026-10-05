# Key and signature formats on the v0.22 protected host

Derived from the code at `e0801593` (branch `c9r4b/opkit`; the kit as of the commit that
adds this note). Line numbers in `scripts/operator_deploy_protected_host.sh` are approximate,
because the kit changes more often than the Rust loaders. If this note and the code disagree,
the code wins.

Paths are relative to the repository root. No real key material appears here.
`<64 lowercase hex>` stands for a public key, and `<128 lowercase hex>` for a signature.

## 1. Summary

| Role | Algorithm | Public half lives in | Private half: custody | Signature it makes |
|---|---|---|---|---|
| Qualification (B263 records, waivers, certification records) | Ed25519 | `/etc/axon/trust/qualification/*.pub` | operator, offline | `axon-evidence-signature/2`, domain `qualification` |
| Verifier / host signer (Fabric) | Ed25519 | `/etc/axon/trust/verifier/*.pub`, plus `signer.public_key` in `/etc/axon/protected-host.json`, `observer.host_signer_public_key` in `/etc/axon/protected-launcher.json`, and the loop store's `verifier_keys` | Fabric service uid, on the host | `acf-receipt-attestation/2`, and `axon-document-signature/1` with domain `axon.fabric-execution/1` |
| Observer | Ed25519 | `/etc/axon/trust/observer/*.pub`, plus the loop store's `observer_keys` | the operator's observer program (not in this repo) | `axon-evidence-signature/2` with domain `observer` (the observation), and `axon-document-signature/1` with domain `axon.closed-loop.context/1` (the context) |
| Admission | Ed25519 (by type only) | `/etc/axon/trust/admission/*.pub` | operator | no verifier consumes one today (section 6) |
| Monitor | Ed25519 | `/etc/axon/trust/monitor/*.pub`, plus the loop store's `monitor_keys` | the monitor | `axon-document-signature/1`, domain `axon.loop.trial-safety/1` |
| Custodian | **no key** | n/a | n/a | n/a. It is authenticated by uid and program pin (section 8) |
| `AXON_ATTEST_KEY` | HMAC-SHA256 (symmetric) | n/a. It is a shared secret | the operator, through the environment of `axon-vm` and `axon-loop` | the axon-vm attestation report MAC and the loop ledger MACs |
| Completion secret (per attempt) | HMAC-SHA256 | n/a | Fabric, ephemeral | guest completion and outcome tokens |

The five trust roots come from `TrustAuthority`
(`crates/axon-loop-contracts/src/operator_trust.rs:21-59`). They are fixed at
`/etc/axon/trust/<qualification|observer|verifier|admission|monitor>/`
(`OPERATOR_TRUST_ROOT`, `operator_trust.rs:16`, `operator_dir` at `:56`). No flag, env var,
store or repository file can name another root. A test root exists only in
`test-trust-root` builds (`operator_trust.rs:155-178`).

## 2. Common formats

### 2.1 Private keys (every Ed25519 role)

- Format: PKCS#8 **v2** DER, exactly as `ring` 0.17 `Ed25519KeyPair::generate_pkcs8`
  writes it. Every loader uses `Ed25519KeyPair::from_pkcs8`, and that function accepts only
  v2, which embeds the public key. A PKCS#8 v1 key (for example a default OpenSSL Ed25519
  key) is refused. Loaders: `attestation.rs:94-99` (`public_key_of`), `:104-123`
  (`sign`), `:245-262` (`sign_document`), and `crates/axon-fabric/src/bin/axon-fabric.rs:379`
  (`sign-evidence`).
- Producer: `axon-fabric keygen --out PATH` (`axon-fabric.rs:736-771`). It opens the file
  with `create_new` (it never overwrites) and mode `0o400`, then writes the raw DER. It
  prints `{"schema":"axon-fabric-issuer-key/1","private_key":…,"public_key":"<64 lowercase hex>","fingerprint":"ed25519:<16 hex>"}`.
  It writes **no** `.pub` file. The operator copies `public_key` into one.
- `attestation::generate` (`attestation.rs:81-91`) does the same thing for tests.

### 2.2 Key id / fingerprint: a label, never a key

`ed25519:` + the first 16 lowercase hex characters of SHA-256 over the raw 32-byte public key.
Two identical implementations exist: `attestation.rs:66-70` (`key_fingerprint`) and
`operator_trust.rs:259-266`. `key_id_of_hex` (`attestation.rs:73-77`) maps a registered hex
key to its id.

It appears as `issuer_key_id` (B263 record, compared with the signer at
`crates/axon-fabric/src/backend.rs:853-860`), `observer_key_id` (observation,
`crates/axon-fabric/src/observer.rs:231`, `protected_evidence.rs` check_bundle,
`readiness.rs:742`), `verifier_key_id` (certification, `readiness.rs:712-715, 969-987`), and
`key_id` inside attestations and document signatures.

A fingerprint is **never accepted as a key**:

- every trust-root file must be 64 hex characters (`operator_trust.rs:143`), and
  `ed25519:…` fails that;
- every registered key goes through `unhex` (`attestation.rs:329-340`), which refuses `:`;
- acceptance always compares the raw 32 key bytes (`operator_trust.rs:317`,
  `attestation.rs:161-172, 296-305`) and then verifies the signature under those bytes.

The id only names which root key to look up (`readiness.rs:972-974`) or is compared with the
signer's own id after verification.

### 2.3 Public-key text: what each loader accepts

The format is bare hex of the 32-byte key. **The loaders do not all agree.**

| Loader | Case | Whitespace | Length | Cite |
|---|---|---|---|---|
| Trust-root `*.pub` file (`keys_in`, used by Fabric, readiness and the loop) | upper or lower; lowercased on read | `str::trim()`: leading and trailing whitespace, including a trailing `\n` or `\r\n`, removed | exactly 64 hex characters after the trim | `operator_trust.rs:119-153` (`:142` trim+lowercase, `:143` length and hex check) |
| `signer.public_key` in `/etc/axon/protected-host.json` | **lowercase only in effect** | **none** | 64 | read as any string at `protected_host.rs:326-329`. Then `signer_from` requires it to equal, byte for byte, the lowercase hex derived from the key file (`axon-fabric.rs:276-283`), otherwise every protected submit is refused with exit 4 |
| `observer.host_signer_public_key` in `/etc/axon/protected-launcher.json` | lowercase only (`0-9a-f`) | none | 64 | `privileged_launcher.rs:217-219, 404-410`. It must also equal `signer.public_key` exactly (`protected_host.rs:745`) |
| Loop store `verifier_keys` / `monitor_keys` / `observer_keys` (`<store>/config.json`) | **lowercase only** | **none** | 64 | the root lookup `rooted` trims and lowercases (`operator_trust.rs:228-241`), but verification uses the stored string through strict `unhex` (`attestation.rs:135-139, 274-279, 329-340`), and so does `key_id_of_hex`. An uppercase store value passes the root check and then fails verification |
| `public_key` / `signature` inside an `axon-evidence-signature/2` file | upper or lower | trimmed | 32 / 64 bytes | `hex_bytes`, `operator_trust.rs:268-277` |
| `public_key` / `signature` inside `acf-receipt-attestation/2` and `axon-document-signature/1` | lowercase only | none | 32 / 64 bytes | `unhex`, `attestation.rs:329-340` |

Operator rule: write every key as 64 **lowercase** hex characters and nothing else. A single
trailing newline in a `.pub` file is harmless. Anywhere else it breaks the key.

### 2.4 Trust-root directory rules (`*.pub` files)

Parser: `keys_in` (`operator_trust.rs:119-153`). Ownership: `check_owned_chain`
(`operator_trust.rs:65-112`).

- **File-name rule.** Only entries whose extension is exactly `pub` are parsed
  (`Path::extension() == Some("pub")`, `:138`). The match is case-sensitive, so `x.PUB` is
  ignored. A file named just `.pub` has no extension and is ignored. Other files are ignored
  by the parser, but they are still ownership-checked (next bullet).
- **One malformed `.pub` refuses the whole root.** Malformed means unreadable, not UTF-8,
  or not 64 hex after the trim (`:141-148`). A directory named `*.pub` also fails the read
  and refuses the root.
- **Absent root** (NotFound) holds nothing. **Empty root** holds nothing. Any other listing
  error (EACCES, ENOTDIR, ELOOP) refuses (`:121-130`). Fabric then refuses a root with no
  keys: "no trusted evidence issuer is configured" (`backend.rs:649-668`). The loop refuses
  "not in the operator's … root" (`operator_trust.rs:233-239`).
- Duplicate keys inside one root are not refused.
- **Ownership** (when the root is the operator's). Every path component from `/` down to the
  root, **and every entry in it**, must be not a symlink, owned by uid 0, and have
  `mode & 0o022 == 0` (no group or other write) (`operator_trust.rs:73-110`). Read
  permission is not checked by the loaders. The readiness verifier must be able to read the
  files: `scripts/trust_root_preflight.sh:248-255` proves it. Readiness also refuses a root
  or entry that the running process can write (`readiness.rs:196-225`).
- Where the walk runs: the loop on every `rooted`/`rooted_keys` read
  (`operator_trust.rs:182-193`); Fabric's `qualification()` (`backend.rs:1027-1029`); every
  observation check (`observer.rs:215-217`); readiness (`readiness.rs:196-225`); and every
  present peer root during a separation check when a walk base is given
  (`backend.rs:628-630`). `ProtectedHost::load` does **not** walk the roots. It does check
  separation at load (`protected_host.rs:360-385`).
- Install (runbook step 5, deploy kit): directory `root:root 0755`, file `root:root 0644`.
  The kit checks this at `scripts/operator_deploy_protected_host.sh:969-1019`. Its `.pub`
  rule (`open(p).read().strip().lower()`, 64 hex) mirrors `keys_in`.

### 2.5 Signature formats

**`axon-evidence-signature/2`** (qualification and observer, and any `--authority`).

- File: `<record>.sig`, a JSON object:
  `{"schema":"axon-evidence-signature/2","alg":"ed25519","domain":"<authority dir name>","public_key":"<64 hex>","signature":"<128 hex>"}`.
- Signed bytes: `"axon-evidence-signature/2\n" + <authority dir name> + "\n" + <the exact record file bytes>`
  (`evidence_signing_message`, `operator_trust.rs:252-256`). The record is never
  reformatted or canonicalized.
- Producer: `axon-fabric sign-evidence --record FILE --key PKCS8 --authority A`
  (`axon-fabric.rs:374-397`). It writes `<record>.sig` with `fs::write`, so the umask sets
  its mode.
- Verifier: `verify_evidence_signature` (`operator_trust.rs:282-335`). It checks schema and
  alg (`:292`), then `domain` equals the caller's authority (`:298`), then `public_key` is
  among the trusted root keys, compared as bytes (`:317`), then the Ed25519 signature
  (`:324`). It returns the signer's fingerprint. Unknown extra fields are **not** refused.
- The `.sig` file is read once by `read_signature` / `read_regular` (`backend.rs:696-709`,
  `:447-499`). It must be a regular file, not a symlink, at most 256 MiB, and UTF-8. An
  absent `.sig` is its own refusal (RULE:unsigned).
- `--authority` accepts all five names (`TrustAuthority::parse`, `operator_trust.rs:43`).
  The usage message at `axon-fabric.rs:364` lists only four and omits `monitor`.

**`acf-receipt-attestation/2`** (verifier).

- Signed bytes: `canonical_bytes` (`crates/axon-loop-contracts/src/canonical.rs:193-201`,
  the `cl22` rule: keys sorted, no whitespace, `ensure_ascii=False`, integers only;
  module doc `canonical.rs:1-24`) of the binding
  `{schema, issuer_ref, key_id, issued_ms, request_ref, receipt_ref, operation_id, task_id, trial_id, attempt_id, execution_id}`
  (`attestation.rs:43-63`). `request_ref` and `receipt_ref` are `cl22:` digests.
- Document: the binding plus `alg:"ed25519"`, `public_key`, `signature` (lowercase hex)
  (`attestation.rs:118-121`).
- Verifier `attestation::verify` (`attestation.rs:127-196`). Unknown fields are refused. The
  presented `public_key` must equal the registered key. Every bound field must match.

**`axon-document-signature/1`** (verifier execution attestation, observer context, monitor clearance).

- Signed bytes: `canonical_bytes` of `{schema:"axon-document-signature/1", domain, issuer_ref, key_id, doc_ref}`,
  where `doc_ref` is the `cl22:` digest (`digest_value`, `canonical.rs:215`) of the document
  (`attestation.rs:208-227`).
- Document: the binding plus `alg`, `public_key`, `signature` (`attestation.rs:245-262`).
- Verifier `verify_document` (`attestation.rs:266-322`): the same discipline as `verify`.
- Domains: `axon.fabric-execution/1` (`attestation.rs:229`), `axon.closed-loop.context/1`
  (`crates/axon-loop/src/evl.rs:221`), and `axon.loop.trial-safety/1`
  (`crates/axon-loop/src/safety.rs:35`).

## 3. Qualification (B263 issuer and certification issuer)

- **Public half:** `/etc/axon/trust/qualification/*.pub` (runbook: `operator.pub`). Loaded by
  `QualificationTrust::operator` (`backend.rs:313-321`) and `ReadinessTrust::operator`
  (`readiness.rs:133-146`).
- **Algorithm:** Ed25519.
- **Private half:** PKCS#8 v2 from `keygen`. Custody: the operator, **offline** (ADR-001
  D5/D6, `axon-fabric.rs:370-373`). The code places no mode rule on this file:
  `sign-evidence` reads it with `fs::read` (`axon-fabric.rs:378`).
- **Signatures:** `axon-evidence-signature/2`, domain `qualification`, over:
  - the B263 record (`axon-b263-evidence/1`), sidecar `<record>.sig` unless the host config
    sets `qualification.signature`. The record's `issuer_key_id` must equal the signer's
    fingerprint (RULE:issuer-claimed, `backend.rs:853-860`);
  - the waiver file (`axon-b263-waiver/1`), `<waivers>.sig`;
  - the protected-host certification record (`axon-v022-protected-certification/2`), `<record>.sig`.
- **Accepted by:**
  - `LinuxProfileConfig::qualification` (`backend.rs:1017-1046`), waivers at `:1060-1068`;
  - `verify_operator_evidence*` (`backend.rs:716-769`), used by `axon-fabric verify-evidence`
    (`axon-fabric.rs:287-331`). That command reports `authoritative:true` only for a
    production build reading the operator root (`backend.rs:275-289`);
  - readiness `certification` (`readiness.rs:623-629`), `attribution` (B263 record,
    `:779-785`), and `certified_waivers` (`:1107-1125`).
  - A repository file `governance/status/trust-expectations.json` may narrow the accepted
    issuers by fingerprint, but never add one (`readiness.rs:630-651`).
- **Install:** `/etc/axon/trust/qualification/` `root:root 0755`, with `*.pub` files
  `root:root 0644`. The signed records go to `/etc/axon/qualification/b263.json` and
  `.sig`, `root:root 0644` (`operator_deploy_protected_host.sh:564-570`).

## 4. Verifier / host attestation signer (Fabric)

- **Public half**, in four places that must agree:
  1. `/etc/axon/trust/verifier/*.pub` (runbook: `host-signer.pub`). The loop requires it
     there for protected evidence (`rooted_key(…, Verifier)`, `intake.rs:726-736`,
     `evl.rs:122-126`). Readiness requires it there (`readiness.rs:969-978`). Fabric itself
     does not require it there. The deploy kit blocks without it
     (`operator_deploy_protected_host.sh:1006-1008`).
  2. `signer.public_key` in `/etc/axon/protected-host.json` (`protected_host.rs:320-331`).
  3. `observer.host_signer_public_key` in `/etc/axon/protected-launcher.json`
     (`privileged_launcher.rs:111-119`), equal to item 2 (`protected_host.rs:745`).
  4. The loop store `verifier_keys[<issuer_ref>]` (`crates/axon-loop/src/store.rs:58-67`),
     lowercase (section 2.3).
- **Algorithm:** Ed25519.
- **Private half:** PKCS#8 v2 at `signer.key_path` (default in the kit:
  `/etc/axon/keys/fabric-attest.pk8`). In development it can be at a check-registry `signer`
  block's `key_path` instead, relative to the registry's directory (`axon-fabric.rs:182-190`).
  `signer_from` (`axon-fabric.rs:203-285`) enforces:
  - the block has exactly `issuer_ref`, `key_path`, `public_key` (`:213-218`);
  - the file is opened once with `O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC`, so a symlink as the
    final component is refused (`:233-252`);
  - it is a regular file (`:256-258`);
  - it is **owned by the Fabric process euid**, and `mode & 0o277 == 0`: the owner may read,
    the owner may not write, and group and other get nothing. So `0400` is accepted, and
    `0600` and `0640` are refused (`:259-272`);
  - it derives exactly the pinned `public_key` (`:276-283`).

  Every directory above the key must be operator-owned (`protected_host.rs:332-336`).
  Custody: the Fabric service uid, on the host. `trust_root_preflight.sh:332-347` proves
  that no other actor can read it. Fabric itself is refused as root
  (`protected_host.rs:652-664`).
- **Signatures:**
  - `acf-receipt-attestation/2` in the submit output's `receipt_attestation`
    (`axon-fabric.rs:921-948`). It is signed only when `signing::attestation_decision`
    allows it (`crates/axon-fabric/src/signing.rs:40`);
  - `axon-document-signature/1` with domain `axon.fabric-execution/1` over
    `execution_document` (`attestation.rs:233-243`), in `execution_attestation`
    (`axon-fabric.rs:899-920, 968`).
- **Accepted by:** loop intake `verify_check_evidence` (`intake.rs:665`, verify at `:781`);
  `evl::verify_execution` (`evl.rs:104-136`); admission re-derivation, which compares the
  recorded `key_id` with the root key's id (`admission.rs:600-620`); and readiness
  `launched` (`readiness.rs:969-987`).

## 5. Observer

- **Public half:** `/etc/axon/trust/observer/*.pub`, and the loop store `observer_keys`. An
  observation is attributed only if exactly one trusted observer's registered key has the
  signer's id (`protected_evidence.rs:274-290`).
- **Algorithm:** Ed25519.
- **Private half:** held by the operator-installed observer program (`observer.command`,
  pinned by sha256). No production observer ships in this repository
  (`operator_deploy_protected_host.sh:426`). The code sets no format or mode rule for it.
- **Signatures:**
  1. The preflight observation. The program writes `observation.json` and
     `observation.json.sig` (`observer.rs:304-311`): `axon-evidence-signature/2`, domain
     `observer`, over the exact `observation.json` bytes. `observer_key_id` must equal the
     signer's fingerprint.
     Accepted by `verify_observation` (`observer.rs:208-245`), in Fabric (early) and in the
     privileged helper at the root boundary (`privileged_launcher.rs:1008-1030`). Also
     accepted by the loop's `protected_evidence::check_bundle` (`protected_evidence.rs:256-262`,
     through `rooted_keys(Observer)`) and by readiness `attribution` (`readiness.rs:734-747`).
  2. The closed-loop context signature. `axon-document-signature/1`, domain
     `axon.closed-loop.context/1`, over the context. Accepted by `evl::authenticated_context`
     (`evl.rs:412-436`) and `admission::reverify_protected` (`admission.rs:419-435`).

## 6. Admission

- A trust root exists: `/etc/axon/trust/admission/` (`operator_trust.rs:28-29`).
  `sign-evidence` and `verify-evidence` accept `--authority admission`.
- **No code verifies a signature under it today.** A search for `TrustAuthority::Admission`
  outside `operator_trust.rs` and the tests finds no consumer. The loop trusts admitters by
  name (`trusted_admitters`); there is no `admitter_keys`.
- Its keys still count in every separation check (section 10). The kit treats the root as
  optional (`operator_deploy_protected_host.sh:977-978`).

## 7. Monitor

- **Public half:** `/etc/axon/trust/monitor/*.pub`, and the loop store `monitor_keys`.
- **Algorithm:** Ed25519. The private half is the monitor's own. The code places no rule on it.
- **Signature:** `axon-document-signature/1`, domain `axon.loop.trial-safety/1`, over the
  safety report (`safety.rs:177-187`).
- **Accepted by:**
  - `safety::report` (`safety.rs:100`, check at `:152-189`). The root check applies only to
    a protected scope (`:161-174`). Otherwise the store key alone is used;
  - `admission::clearance_verifies` (`admission.rs:220-257`), always against the root.

## 8. Custodian: no key

The custodian (`axon-custodian`) holds no signing key, and no trust root names it.

- Callers are authenticated by the kernel (`SO_PEERCRED`, `custodian.rs:196-216`).
- The custodian is authenticated by the uid that bound its socket: the custodian uid, or
  root for systemd activation (`custodian.rs:265-275`).
- When the helper's config pins it, the custodian program's sha256 is also checked over a
  pidfd (`custodian.rs:251-258, 280-285`).
- Uid rules: `custodian.rs:118-132` and `protected_host.rs:667-675`.
- The nonce is 128 random bits, kept in the custodian's own `0700` store. It is not a key.

## 9. Symmetric keys

### 9.1 `AXON_ATTEST_KEY` (HMAC-SHA256)

It is supplied only through the environment. No file is read, and the code checks no mode.
The Fabric protected route does not use it (runbook step 5).

| | `axon-vm` | `axon-loop` ledger |
|---|---|---|
| Read at | `crates/axon-vm/src/main.rs:939-962` | `store.rs:296-310` (`LedgerKey::from_env`), from `store.rs:362` |
| Parse | trimmed; `hex::decode` (upper or lower case) | trimmed; `decode_hex` (`store.rs:312-320`), upper or lower case. It uses `from_str_radix` without a character filter, so a pair such as `+f` also decodes |
| Minimum | 16 bytes, else exit 10 | 16 bytes, else exit 2 (`store.rs:276-286`) |
| Unset or blank | an ephemeral per-process key (signer == verifier) | an unkeyed store. An unkeyed opener refuses a keyed ledger |
| MAC input | the raw key over `digest ‖ axtcb1` (`crates/axon-attest/src/lib.rs:188-199`, verified at `:226-300`) | `k = HMAC(key, "axon-loop ledger key v1")` (`store.rs:262-286`). Entry: `HMAC(k, "axon-loop entry v1\0" ‖ <cl22 digest string of the entry without mac>)`. Head: `HMAC(k, "axon-loop head v1\0" ‖ seq as u64 little-endian ‖ entry_ref string)` (`crates/axon-loop/src/ledger.rs:245-262`) |

The `\0` separator and the little-endian `seq` are in the code. The module doc at
`ledger.rs:37-45` writes `‖` and does not mention either.

`axon-audit` has a keyed chain mode (`crates/axon-audit/src/lib.rs:237`), but no production
caller passes it a key. Only its tests do.

### 9.2 Completion secret (per attempt, ephemeral)

- `S`: 32 random bytes per attempt, written to `job/completion-secret` with `create_new`
  and mode `0400` (`crates/axon-fabric/src/psv.rs:307-313, 332-342`). The development route
  uses `fresh_completion_key` (`submit.rs:735-745`).
- `K = HMAC-SHA256(S, "axon-guest-completion/1\n" ‖ hex(sha256(canonical binding)))`
  (`crates/axon-psv/src/lib.rs:533-559`).
- Outcome tokens: `HMAC(K, "axon-test-completion/1\0"|"axon-test-failed/1\0" ‖ name)`
  (`axon-psv/src/lib.rs:567-578`; `axon-core/src/main.rs:6058-6107`).
- The operator never handles it.

## 10. One key, one role (ADR-002): what is enforced

**Cross-root duplicates are refused today, in every loader, for all five roots.** The pair
list is derived from `TrustAuthority::ALL`, never written by hand. Keys are compared after
`keys_in`'s trim and lowercase, so a case difference does not hide a duplicate.

- **Loop:** `exclusive` (`operator_trust.rs:201-223`) runs on every `rooted` lookup
  (`:228-241`) and on every key of `rooted_keys` (`:340-346`). An absent peer root holds
  nothing. A present peer root is ownership-walked and must be readable.
- **Fabric:** `exclusive_root_keys` (`backend.rs:590-643`), against `sibling_roots`
  (`:567-574`). It is used by:
  - `QualificationTrust::trusted_keys` (`backend.rs:342-349`);
  - `verify_operator_evidence*` (`:716-769`);
  - `ObserverTrust::check_separation` (`observer.rs:153-161`), at host-config load and at
    every observation, in Fabric and in the helper;
  - readiness `exclusive_keys` (`readiness.rs:253-262`).
- **Host-config load** (`protected_host.rs:360-385`): the qualification, admission and
  monitor roots, plus the observer root (through `check_separation` at `:412-414` when an
  observer is configured), are each checked against all the others. That covers every pair,
  including pairs with `verifier`. The peers are not ownership-walked at this point
  (`owned_from = None`).
- **Host signer outside `verifier/`:** refused where the host signer is known. That is at
  load (`protected_host.rs:370, 383`), in `qualification()` through
  `host_signer_public_key` (`backend.rs:604-616`), and in observer checks in Fabric and the
  helper (`privileged_launcher.rs:1023`). It is **not** passed by readiness
  (`readiness.rs:261`, `None`) or by `verify-evidence` (`backend.rs:728`, `None`). Both rely
  on the host signer's key being in `verifier/`, so that the cross-root rule catches it. The
  loop has no host-signer rule of its own and relies on the same thing. If the operator
  leaves the host signer out of `verifier/`, only Fabric's load-time and qualification-time
  checks still refuse it in another root. The deploy kit blocks that state
  (`operator_deploy_protected_host.sh:1006-1011`).
- **Store level:** `Config::check_separation` (`store.rs:202-243`) refuses one key
  registered under two of `verifier_keys`, `monitor_keys` and `observer_keys`. It compares
  lowercased but untrimmed strings. It also refuses an observer identity that is also a
  verifier, admitter or monitor.
- **Domain separation** stops a key validating another role's statement even inside one
  root. The evidence signature binds the authority name (`operator_trust.rs:252-256,
  298-305`), and document signatures bind `domain` (`attestation.rs:210-227`).
- `verify-evidence --issuers DIR` with a caller's directory checks separation against
  `DIR`'s own siblings, not the operator roots. Its answer is marked non-authoritative
  (`backend.rs:275-289`).

## 11. Places where the code and the kit or docs differ

- The `keygen` doc comment says the key is written `0600` (`axon-fabric.rs:100-102`). The
  code writes `0400` (`:752`).
- Fixed in the kit with this note: the kit used to accept a signing key with
  `mode & 077 == 0`, so `0600` passed. Fabric requires `mode & 0o277 == 0`
  (`axon-fabric.rs:264`) and refuses `0600`. The kit now applies Fabric's rule and BLOCKS a
  `0600` key; `scripts/test_operator_deploy.sh` covers that case. Use `0400`.
- Public-key text is lenient in `.pub` files and evidence `.sig` files (trimmed, either
  case). It is strict in the host config, the helper config, the loop store, and the
  attestation and document signatures (lowercase, no whitespace). See section 2.3.
- `verify_evidence_signature` ignores unknown fields in a `.sig`. The two attestation
  formats refuse them.
- The `--authority` usage text omits `monitor` (`axon-fabric.rs:364`).

## 12. Planned (post-C9, operator decision I)

**Not implemented.** The plan is role-typed, checksummed public keys: a role prefix per
authority (for example `axq_pk_` for qualification) followed by 43 base64url characters, with
a checksum, and every loader parsing through one shared parser. A key would then carry its
role, so a key in the wrong root, or one mistyped, would be refused by its format and not
only by the separation checks above.

Today's format is the bare 64-hex key described in section 2.3, and nothing in the code
reads the planned format.
