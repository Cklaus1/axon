# Domain-Interop Native Modules — FHIR / FIX / Modbus

**Spec ID:** `R22-domain-modules` (builds ON `R13-native-ffi`; no invariant change)
**Status:** Landed interp-side (2026-06-26, branch `feat-domain-modules`). Codegen E0910-refused (interp-only), matching the R13 `host_await`/native precedent.
**Risk class:** Additive (new leaf shim crate; pure-Axon programs unaffected)
**Author / date:** cklaus, 2026-06-26

---

### 1. Motivation

R13 proved the curated native-module machinery with a single GPU-free `gfx`
mock. This spec puts that machinery to work on three **real, locally-verifiable
domain protocols** — one per high-value vertical:

| Module | Vertical | Backend | Verification |
|---|---|---|---|
| `native::modbus` | industrial | `tokio-modbus` (TCP client + in-test server) | in-test Modbus TCP server: write reg → read back → assert |
| `native::fhir` | healthtech | `reqwest` + `serde_json` (FHIR R4 REST) | in-test `tiny_http` server returns a canned Patient → read + parse a field |
| `native::fix` | fintech | pure codec (no deps) | build → parse round-trip + checksum validate (and a corrupted-byte negative) |

These are the kind of integration a real ASI agent needs: read a sensor bank,
fetch a patient record, place an order. Each ships as a **verifiable beachhead**,
not a fake stub — see §4 for the honest scope boundaries.

### 2. Requirement link

Extends R13's "an Axon program can drive a native Rust library through a
declared, capability-gated interface" to three third-party protocol libraries.
Reuses R13's machinery wholesale — no new error codes, no new front-end surface.

### 3. Surface

Identical to R13: `use native::M`, a `@[contained(M: …)]` grant, opaque affine
handles. The new modules just add registry rows.

```axon
use native::modbus
@[contained(modbus: any, net: ["127.0.0.1"])]   // module grant + host-pinned net
fn main() -> i64 {
  let h = modbus::modbus_connect("127.0.0.1", 502)   // -> Conn (affine resource)
  modbus::modbus_write_register(h, 3, 4660)          // ref h (borrow)
  let regs = modbus::modbus_read_holding(h, 3, 1)    // -> [i64]
  modbus::modbus_close(h)                             // consumes h (affine drop)
  regs[0]
}
```

```axon
use native::fhir
@[contained(fhir: any, net: ["fhir.example.org"])]
fn main() -> i64 {
  let h = fhir::fhir_connect("https://fhir.example.org")
  let patient = fhir::fhir_read(h, "Patient", "example")  // -> str (PHI: Secret-tagged in the lattice)
  let family  = fhir::fhir_json_get(patient, "name.0.family")
  fhir::fhir_close(h)
  len(family)
}
```

```axon
use native::fix
@[contained(fix: any)]                              // pure codec — no net grant
fn main() -> i64 {
  let msg = fix::fix_new_order_single("SND","TGT","ORD1","AAPL", 1, 100, 150)  // valid FIX 4.4
  let h   = fix::fix_parse(msg)
  let sym = fix::fix_get(h, 55)                     // "AAPL"
  fix::fix_close(h)
  fix::fix_valid(msg)                               // 1 (BodyLength + CheckSum correct)
}
```

### 4. Semantics — what's REAL vs beachhead-scoped

| Module | Real | Beachhead boundary (documented, not faked) |
|---|---|---|
| `modbus` | A true Modbus TCP client (`tokio-modbus`): read/write holding registers + coils over a real socket, verified against a real in-test Modbus server. | No Modbus-RTU (serial), no function-code coverage beyond reg/coil read/write. |
| `fhir` | A real HTTP client (`reqwest` blocking + `serde_json`): FHIR R4 `read` + `search` interactions, verified against a real in-test HTTP server, JSON field extraction. | Not a full FHIR server, no auth/OAuth/SMART-on-FHIR, no resource validation — `read`/`search` are the verifiable client beachhead. |
| `fix` | A correct FIX 4.4 **message codec**: build a NewOrderSingle with correct `BodyLength`(9) + `CheckSum`(10), parse any SOH string, read fields, independent checksum validation. | **No FIX session** (logon/heartbeat/sequence-recovery over a TCP acceptor) — that is heavier and stateful. The message codec is the locally-verifiable core; said so explicitly. |

**Boundary value layer.** R13's `gfx` only needed Int/Float/Str args and
Unit/Int/Handle returns. These modules add **str returns** (FHIR JSON, FIX
fields) and **`[i64]` returns** (Modbus reads), so `axon-domain` defines its own
`DomainArg`/`DomainValue` enums spanning the full representable set; the
interpreter marshals `Value`↔`Domain*` at the boundary exactly as for gfx.

**Handles.** Each connection/session is a generation-tracked slab slot
(`axon_domain::Slab`), reusing R13's unforgeable-index invariant: a
forged/stale/out-of-range/`i64::MIN` index → a graceful `Err` (exit 101),
**never** a wild deref or host abort (I-4). Proven by per-module
`bad_handle_is_graceful_err` tests.

### 5. Capability gating (no new code)

- **E1004 (module grant):** an ungranted `use native::fix` call is E1004 —
  inherited automatically from R13's `check_native_grants` (the registry row is
  the only addition). Verified: `fix` without `@[contained(fix: any)]` → E1004.
- **Net-host pinning:** `modbus`/`fhir` declare the `Net` effect; their
  `*_connect` host literal is pinned against the `net: […]` allowlist via the
  **same** mechanism as builtin net calls. The net-check core was factored into
  `check_net_host`; `native_net_host` extracts the connect host (stripping
  `scheme://` and `:port/path`). Verified: connecting to `127.0.0.1` under
  `net: ["10.0.0.5"]` → E1001 against the ACTUAL connect host (the
  `ai-complete host check` lesson — pin the real host, not a prompt). A dynamic
  (non-literal) host fails closed.
- **Affine handles (E0601) + cross-module (E1802):** inherited from R13. A
  resource handle used after its consuming `*_close` is E0601 at compile time;
  a `modbus::Conn` passed where a `fix::FixMsg` is expected is E1802. Both
  verified. (R22 refined the R13 borrow rule: a `Move`-mode **non-handle** arg —
  a `str`/`[i64]` passed by value — no longer wrongly consumes its Axon binding;
  only resource handles are affine.)

### 6. Codegen — interp-only by design

`modbus`/`fhir` do live network I/O; `fix` is kept interp-only for a uniform
domain story. Codegen **E0910-refuses** all three (`is_codegen_refused`) — the
`host_await`/native precedent, sound-by-refusal. Verified: `axon build
fix_demo.ax` emits clean E0910 diagnostics and aborts; `axon run` works.
`axon-domain` is therefore **not** linked into `axon-rt` and contributes no
`#[no_mangle] extern "C"` symbols.

### 7. Invariants

- **I-2 (parity):** N/A in the diff sense — codegen refuses these modules, so
  there is no second engine to diverge from (the refusal IS the parity story,
  per R13 §11's interp-only slices).
- **I-4 (never abort the host):** preserved — slab-index handles, graceful `Err`
  on any bad index (tested per module).
- **I-5 (ownership):** preserved — resource handles are affine via the existing
  borrow checker (E0601).
- **I-9 (no silent success):** preserved — a missing backend (wasm target) or a
  network failure is a clean refusal/`panic`, never a no-op.
- **I-11 (capability boundary):** the R13 edge-enforced-trusted-within posture
  is unchanged. `modbus`/`fhir` carry the host-allowlist attenuation (§5), so
  `net` through a domain module is host-pinned, not a blanket grant.

**TCB delta:** `tokio-modbus`, `tokio`, `reqwest` (+ rustls), `serde_json` enter
the TCB **only for non-wasm interpreter builds** that actually compile
`axon-domain`. They are a `cfg(not(target_arch = "wasm32"))` dependency of
`axon-core` and are NOT in the default codegen link, NOT in `axon-rt`, and NOT in
the in-browser wasm interpreter (R7c) — keeping the browser TCB lean.

### 8. wasm-target discipline

The heavy deps (`socket2` via tokio) do not build for
`wasm32-unknown-unknown` (the in-browser interpreter target). `axon-domain` is
gated to non-wasm targets in `axon-core`'s manifest, and the `interp.rs` field +
`eval.rs` dispatch are `#[cfg(not(target_arch = "wasm32"))]`. On wasm a domain
call is a clean "unavailable on the wasm32 (browser) target" refusal. The
`wasm_unknown_interp_builds.sh` gate (the R7c precondition) stays green.

### 9. Test plan / acceptance

- [x] `cargo test -p axon-domain` — 9 tests: FIX build/parse/checksum round-trip +
      corrupted-byte negative + use-after-close; Modbus real TCP write→read→assert
      (in-test server) + coil round-trip; FHIR real HTTP read+search round-trip
      (in-test `tiny_http`) + JSON field parse; per-module forged-handle graceful-Err.
- [x] `scripts/fix_codec.sh` — codec tests + the `.ax` demo end-to-end.
- [x] `scripts/modbus_roundtrip.sh` — in-test server round-trip + the `.ax` demo
      against a fixed-port server (`examples/modbus_test_server.rs`).
- [x] `scripts/fhir_roundtrip.sh` — in-test mock-server round-trip + the `.ax`
      demo against a fixed-port mock (`examples/fhir_test_server.rs`).
- [x] Capability gating: ungranted import → E1004; wrong net host → E1001 against
      the real connect host; affine use-after-consume → E0601; cross-module
      handle → E1802.
- [x] Codegen E0910-refuses each module.
- [x] `wasm_unknown_interp_builds.sh` green (browser interp build unaffected).
- [x] `gate.sh` green (axon-domain in the runtime-crate clippy line).

Each gate SKIP-guards if a build leg is unavailable and asserts it actually ran
(no vacuous pass).

### 10. Files

- `crates/axon-domain/` — the shim crate (`lib.rs` boundary layer + `Slab`;
  `modbus.rs` / `fhir.rs` / `fix.rs` backends; `examples/*_test_server.rs` for the
  fixed-port demo legs).
- `crates/axon-core/src/native.rs` — registry rows (`MODBUS`/`FHIR`/`FIX`) +
  `is_codegen_refused`.
- `crates/axon-core/src/interp.rs` + `interp/eval.rs` — the interp dispatch
  (`eval_domain_native_call`, wasm-gated).
- `crates/axon-core/src/codegen/expr.rs` — the E0910 refusal.
- `crates/axon-core/src/capabilities.rs` — `check_net_host` + `native_net_host`.
- `crates/axon-core/src/borrow.rs` — the non-handle `Move`-arg refinement.
- `examples/domain/{modbus,fhir,fix}_demo.ax` — the demos.
- `scripts/{modbus_roundtrip,fhir_roundtrip,fix_codec}.sh` — the gates.

### 11. Open / deferred

- FIX session management (acceptor/initiator, seq numbers, heartbeats) — deferred;
  the codec is the beachhead.
- Full FHIR server / auth — deferred; `read`/`search` client is the beachhead.
- Modbus-RTU (serial) + broader function codes — deferred.
- A real user-authored domain module remains gated behind R13 Q4 (registration
  is a privileged, content-addressed act); these three are compiler-built-in.
