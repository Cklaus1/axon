### F160 — axon-attest verify_report never verifies the signature — forged reports (and fake hw_root="sev-snp") pass
*crates/axon-attest/src/lib.rs* · kind: attestation

crates/axon-attest/src/lib.rs:191-227 (`verify_report`) checks only that `signature` is NON-EMPTY, then compares `measurement.digest` and `measurement.axtcb1` to caller-supplied expectations. The HMAC is never recomputed and `hw_root` is never checked against anything. I built a probe against the real crate:

```
1) genuine report        -> Ok(())
2) FORGED sig b"A"       -> Ok(())
3) signed w/ WRONG key   -> Ok(())
4) claims hw_root=sev-snp, junk sig -> Ok(())
5) try_admit_job(forged) -> Ok("axrec1:91e31b4c…")
```

So a one-byte signature verifies, a report signed with an attacker's key verifies, and a software-stand-in report that FALSELY CLAIMS `hw_root = "sev-snp"` (real hardware confidential computing) verifies and admits a job. The `SOFTWARE_TPM_HW_ROOT` honesty caveat is printed to stderr by the CLI but is not enforceable by any relying party, because nothing in `verify_report` looks at `hw_root`.

Compounding it: the signing key in `crates/axon-vm/src/main.rs:670-678` is `SHA256(b"axon-r26-software-tpm-ephemeral-key" || process::id().to_le_bytes())` — ~22 bits of entropy from a public value. Even if the HMAC were checked, it is trivially brute-forced.

The module header claims a chain "sign_report … → verify_report(…) → checks digest + axtcb1 + non-empty signature (fail-closed)". It is fail-closed only against accidental corruption, never against an adversary.

**Recommendation:** Recompute the HMAC in `verify_report` against a key the verifier holds (not one derived from the prover's PID), and make `hw_root` an explicit caller expectation that must match — a report claiming `sev-snp` must be rejected unless a real SEV-SNP cert chain verifies. Until then, rename these functions so they cannot be read as attestation (`content_digest_matches`), and make `AttestationReport` refuse to serialize a `hw_root` the producing substrate cannot actually back.

---

### F093 — `axon build` fails with ~100 undefined-reference linker errors unless cwd is the Axon repo root
*crates/axon-core/src/codegen/link.rs:663-712 (build_crate_staticlib), :332 (call site)* · kind: documented-command-broken

README Quick Start and CLAUDE.md both document `axon build foo.ax` as the native AOT path ("builds in ~3s"). It only works when the process cwd is the Axon workspace root.

Verified, same file, same binary, three cwds:
```
### from repo root:
Compiling examples/hello.ax...
Binary: .../hA (1025ms)

### from $HOME:
collect2: error: ld returned 1 exit status
error: linker (/usr/bin/cc) exited with exit status: 1

### from $HOME with CARGO_MANIFEST_DIR=/home/cklaus/projects/axon/crates/axon-core/src:
Binary: .../hC (7015ms)
```
The failure mode is ~100 lines of raw ld output with no diagnosis — the *whole* runtime is missing, not one symbol:
```
/usr/bin/ld: arithN.o: in function `str_reverse':
arith:(.text+0xf): undefined reference to `__axon_str_reverse'
... (98 more) ...
/usr/bin/ld: arithN.o: in function `to_str':
arith:(.text+0x2f4): undefined reference to `__axon_i64_to_str_radix'
```
Root cause: `build_crate_staticlib` reads `CARGO_MANIFEST_DIR` **at runtime** (it is a compile-time-only cargo variable, so always absent), and falls back to the *relative* paths `"Cargo.toml"` and `"target"`:
```rust
let manifest = std::env::var("CARGO_MANIFEST_DIR")
    .map(|d| format!("{d}/../../../Cargo.toml"))
    .unwrap_or_else(|_| "Cargo.toml".into());
```
The `cargo build -p axon-rt --manifest-path Cargo.toml` subprocess then fails, and the failure is swallowed twice — stdout/stderr are piped to `Stdio::null()`, and `if !status.success() { return None }` — after which `link()` at :332 just links without `libaxon_rt.a`. The symbols exist in the archive (`nm target/debug/libaxon_rt.a` shows `T __axon_arith_panic`, `T __axon_set_provenance_source`, 101 `T __axon*` total); it is simply never passed to `cc`.

Second-order release blocker in the same code: even when it works, the shipped compiler shells out to `cargo build -p axon-rt` at link time, so a released `axon` binary requires a Rust toolchain **and** the full Axon source tree at runtime.

**Recommendation:** Resolve the workspace root from the running executable (`std::env::current_exe()` → walk up to the dir containing `Cargo.toml`) or bake it in with `env!("CARGO_MANIFEST_DIR")` at compile time instead of `std::env::var`. Separately, stop discarding the subprocess output: if `build_crate_staticlib` returns `None`, abort with a real diagnostic ("could not build/locate libaxon_rt.a — <cargo stderr>") rather than linking a runtime-less object. Longer term, ship a prebuilt `libaxon_rt.a` alongside the binary so `axon build` does not need cargo or the source tree.

---

### F110 — f64 -> string is %.6g, silently corrupting numeric data on output with no lossless alternative
*crates/axon-core/src/interp/value.rs:4* · kind: correctness-data-corruption

`to_str`/`to_str_f64` format f64 with `%.6g` — six significant figures. crates/axon-core/src/interp/value.rs:4 documents this as deliberate: "`fmt_g` is the `%.6g` float formatter converged onto C's printf (R1f slice 2b)", i.e. chosen to preserve interp/native parity. Parity is preserved; both are lossy.

Measured round-trip (`parse_float_or(to_str(x))` vs `x`):
```
1/3        : "0.333333"    round-trips? false  diff=3.33333e-07
0.1+0.2    : "0.3"         round-trips? false  diff=5.55112e-17
1234567.891: "1.23457e+06" round-trips? false  diff=-2.109
pi15dp     : "3.14159"     round-trips? false  diff=2.65359e-06
1e-9       : "1e-09"       round-trips? true   diff=0
```
The 1234567.891 case is the damaging one: an absolute error of 2.109. This reaches the filesystem — `write_file("big.csv", "total={to_str(v)}\n")` with v=1234567.891 produces a file literally containing `total=1.23457e+06`.

There is no escape hatch. I enumerated every formatting-adjacent builtin; `format(template: str) -> str` only interpolates and takes no precision spec (`format("{to_str(1.0/3.0)}")` -> `0.333333`). `Decimal` IS exact (`decimal_from_str("1234567.891")` -> `decimal_to_str` -> `1234567.891`), but it is only constructible from a string, and JSON numbers cannot be extracted as strings (see the JSON finding), so the exact path is unreachable from JSON input.

**Recommendation:** Make the default f64 rendering shortest-round-trip (Rust's `{}` / Ryu, which C's printf `%.17g` also achieves), and re-converge native codegen on that instead of on `%.6g`. If %.6g must stay the default for display, add a lossless `to_str_f64_exact` (or a precision argument on `format`) so data pipelines have any correct option at all. Add a round-trip property test: `parse_float(to_str(x)) == x`.

---

### F162 — Guest policy parser fails OPEN to all 8 effects on every error path — including the default policy axon-vm run actually sends
*crates/axon-guest-kernel/src/mmds.rs* · kind: kernel-security

`crates/axon-guest-kernel/src/mmds.rs:257-282` (`json_array_effects`) returns `EffectSet(0xFF)` — every effect granted — when the field is absent, when it is not an array, or when the JSON did not decode. Same for `boot_params == 0`, a missing `axon.policy=`, and `POLICY_READY == false` (lines 53-55, 106-108, 159-165, 184-187). The static default is `EffectSet(0xFF)` too.

I booted the shipped kernel under Firecracker with a series of policies:

```
### policy: {"allowed_effects":["IO"]}
   enforce: gate active — 1 effect bit(s) allowed (0x1)     ← correct
### policy: {"allowed_effects":null}
   enforce: gate active — 8 effect bit(s) allowed (0xff)    ← ALL EFFECTS
### policy: {"allowed_effects":"Exec"}   (wrong JSON type)
   enforce: gate active — 8 effect bit(s) allowed (0xff)    ← ALL EFFECTS
### base64 with ONE character corrupted
   K2: policy 26 json bytes
   enforce: gate active — 8 effect bit(s) allowed (0xff)    ← ALL EFFECTS
```

The `null` case is not hypothetical — it is exactly what `axon-vm run` emits for any program without an `.axmeta` manifest and no `--principal`. I decoded the cmdline from a live run:

```
{"schema":"axon-vm-mmds/1","run_id":"vm-35106-16330","principal":null,
 "allowed_effects":null,"budget_tokens":null,"source_hash":"c9a4891…","seccomp_bpf_b64":null}
```

So the default `axon-vm run` grants the guest IO+FS+Net+AI+Exec+Random.

The parser is also non-structural (`find_subslice` over the raw bytes, first match wins). With `{"allowed_effects":["Exec","Net"],"allowed_effects":["IO"]}` Python's `json.loads` yields `['IO']` while the kernel reports `gate active — 2 effect bit(s) allowed (0x14)` (Exec|Net). Any producer/verifier pair that disagrees on duplicate-key or escaping semantics diverges from the enforcement point.

**Recommendation:** Invert every default: absent/unparseable/wrong-type policy must yield `EffectSet(0)` and refuse to run, not 0xFF. Make `axon-vm` refuse to launch when `allowed_effects` is None rather than serializing `null`. Replace the substring scan with a bounded structural scan that at minimum requires the key to be at object depth 1 and rejects duplicate keys.

---

### F139 — Guest program controls the supervisor's verdict and exit code by writing to stderr
*crates/axon-os/src/runtime.rs* · kind: verdict-forgery

Because classification is `err.contains(...)` (runtime.rs:388-406), any guest granted IO can name its own verdict. Three clean programs that do nothing but `eprintln` and `return 0`:

  forge.ax:  fn main() -> i64 { eprintln("budget exhausted: totally legit")  0 }
  $ axon-os run forge_io.axjob
  budget exhausted: budget  (run-id: run, ...)
  exit=7

  forge2.ax: fn main() -> i64 { eprintln("capability")  0 }
  $ axon-os run forge2_io.axjob
  ⚠ DENIED: runtime capability/sandbox violation (axis: sandbox)  (run-id: run, ...)
  exit=8

  forge3.ax: fn main() -> i64 { eprintln("axon: refinement violated: nope")  0 }
  $ axon-os run forge3_io.axjob
  refinement violated: refinement violated: nope  (run-id: run, ...)
  exit=6

The bare substring `capability` is especially wide — any diagnostic or log line containing that word flips a clean run to DENIED/8. Paired with the previous finding, the supervisor's exit code is guest-controlled in both directions: forge a fault, or hide a real one.

**Recommendation:** Same root fix. Until the channel is separated, at minimum anchor the matches to the interpreter's own `axon: ` prefix at line start and drop the bare `capability` substring — but note that a guest can print `axon: ` too, so substring matching on a guest-writable stream cannot be made sound.

---

