# Adversarial triage — crit-1.md (5 findings claimed CRITICAL)

Method: each cited file opened at the cited lines; reachability traced to a caller or
reproduced against the built binary. Default was "refuted"; three findings survive at a
reduced severity, two survive at HIGH.

---

## F160 — `verify_report` never verifies the signature / `hw_root`

**VERDICT: CONFIRMED (as fact) — severity CRITICAL -> MEDIUM**

The cited code says what the finding claims. `crates/axon-attest/src/lib.rs:197` is the only
signature check — `if report.signature.is_empty() { return Err(...) }` — and nothing in the
function body reads `report.hw_root`; steps 2 and 3 (`:206`, `:216`) compare only
`measurement.digest` and `measurement.axtcb1`. `try_admit_job` (`:242`) gates on exactly this,
so a forged report does admit a job, and `crates/axon-vm/src/main.rs:670-678` really does derive
the key as `SHA256(b"axon-r26-software-tpm-ephemeral-key" || process::id().to_le_bytes())`.

Severity is overstated, though. The function's own doc block is explicit rather than deceptive:
`lib.rs:187-190` — *"the HMAC key is not re-checked here (the operator owns the stand-in's EK;
§8 is honest about this). Only real hardware (hw-attest) provides cryptographic binding"* — and
`crates/axon-attest/Cargo.toml` has **no `[features]` section at all**, so no `hw-attest` path
exists and `sign_report` (`:173`) hardcodes `hw_root = SOFTWARE_TPM_HW_ROOT`. There is no real
confidential-computing claim to subvert and no cross-machine trust chain (the key is
per-process ephemeral and never published), so this is misleading naming around a content-digest
check, not a broken production attestation gate. The `hw_root`-unchecked gap is the genuinely
sharp edge, since it is what would fail open the day a real backend lands.

*Fix:* recompute the HMAC against a verifier-held key and make `hw_root` an explicit caller
expectation that must match; until then rename to `content_digest_matches`.

---

## F093 — `axon build` fails with ~100 undefined-reference errors unless cwd is the repo root

**VERDICT: CONFIRMED — severity CRITICAL -> HIGH**

Code and repro both hold. `crates/axon-core/src/codegen/link.rs:663-666`:
`let manifest = std::env::var("CARGO_MANIFEST_DIR").map(|d| format!("{d}/../../../Cargo.toml")).unwrap_or_else(|_| "Cargo.toml".into());`
— `CARGO_MANIFEST_DIR` is cargo-compile-time only, so the runtime `env::var` always misses and
the relative `"Cargo.toml"` is used; the same fallback repeats for the target dir at `:712`.
The failure is double-swallowed at `:702-707` (`Stdio::null()` on both streams, then
`if !status.success() { return None }`), and `link.rs:332` (`let rt_lib = build_axon_rt(release);`)
proceeds to link without the runtime. Reproduced with the codegen-enabled binary: from the repo
root `axon build examples/hello.ax` succeeds in 728ms; from `$HOME` the identical binary and file
yield `undefined reference to '__axon_goal_run'`, `'__axon_set_provenance_source'`, … and
`error: linker (/usr/bin/cc) exited with exit status: 1`.

Not CRITICAL: no security or silent-corruption dimension — it is a loud, deterministic failure of
a documented command, and the interpreter path (`axon run`) is unaffected. It is a real
release blocker for the shipped-binary story (the compiler shells out to `cargo build -p axon-rt`
at link time).

*Fix:* resolve the workspace root from `current_exe()` (or bake in `env!("CARGO_MANIFEST_DIR")`),
and abort with the captured cargo stderr instead of linking a runtime-less object.

---

## F110 — f64 -> string is `%.6g`, lossy, with no lossless alternative

**VERDICT: CONFIRMED — severity CRITICAL -> MEDIUM**

Reproduced: `println(to_str(1234567.891))` prints `1.23457e+06` and `to_str(1.0/3.0)` prints
`0.333333`. The implementation is `crates/axon-core/src/interp/value.rs:921-946` (`fmt_g`), with
`let p: i32 = 6;` at `:931` and the comment at `:934-935` confirming this is deliberately
converged onto C's `%.6g` for interp/native parity. `builtins.rs:89-92` documents `to_str_f64` as
`"%.6g" format`, and a scan of the builtin table found no precision-taking or exact float
formatter, so the "no escape hatch" claim holds.

Severity reduced because this is a documented, intentional, parity-preserving *display* default
matching C's `printf("%g")` — the same choice C, and historically many languages, make — not a
silent computation error. Values are exact in memory; only rendering truncates. It becomes a data
bug only when `to_str` is used as a serialization primitive, which is a real but narrower hazard
and has an exact alternative for the money case (`Decimal`).

*Fix:* add a lossless `to_str_f64_exact` (shortest-round-trip) and a
`parse_float(to_str_exact(x)) == x` property test; consider making it the default and
re-converging codegen.

---

## F162 — Guest policy parser fails OPEN to all 8 effects on every error path

**VERDICT: CONFIRMED — severity CRITICAL -> HIGH**

Every cited default is verbatim in the code. `crates/axon-guest-kernel/src/mmds.rs:256-265`:
`/// Returns 0xFF (open) if field absent.` then `None => return EffectSet(0xFF)` and
`if rest.is_empty() || rest[0] != b'[' { return EffectSet(0xFF); }` — so `"allowed_effects":null`
(not a `[`) grants all eight bits. The same open default appears at `:41`
(`static mut ALLOWED_EFFECTS: EffectSet = EffectSet(0xFF); // open by default`), `:161`
(the `!POLICY_READY` path), and `:186` (`set_open_policy`), reached from `init` at `:51`,
`:69`, and `:105`. The `null`-in-practice claim also holds: `crates/axon-vm/src/main.rs:886-899`
sets `allowed_effects` to `manifest.effect_union.or_else(principal…)` — `None` with neither — and
`MmdsPayload.allowed_effects: Option<Vec<String>>` (`:506`) carries no
`skip_serializing_if`, so it serializes as `null`. The non-structural parse is real too:
`find_subslice` (`:184`) is a raw first-match byte scan, so duplicate keys diverge from any
real JSON parser at the enforcement point.

Held below CRITICAL only because reachability requires booting the R26 Firecracker/KVM guest
substrate (an experimental lane, not the default `axon run`/`axon build` path), and the
enforcement it fails open is a defense-in-depth in-guest syscall gate
(`enforce.rs:414`, `:441`) rather than the primary language-level capability checker. Within
that substrate the finding is exactly right: the default is fail-open.

*Fix:* invert every default to `EffectSet(0)` + refuse to boot, and make `axon-vm run` refuse to
launch rather than serialize `allowed_effects: null`.

---

## F139 — Guest program controls the supervisor's verdict via stderr

**VERDICT: CONFIRMED — severity CRITICAL -> MEDIUM**

The classifier is bare substring matching on a guest-writable stream, as claimed:
`crates/axon-os/src/runtime.rs:388-391` — `} else if err.contains("sandbox violation") ||
err.contains("SandboxViolation") || err.contains("not permitted by @[contained]") ||
err.contains("capability") {` — over `let err = &proc.stderr;` (`:377`), where `proc` is the
captured output of the child spawned at `:336-337` (`Command::new(&self.axon_bin); cmd.arg("run")
.arg(&wrapper_path);`). A guest `eprintln` lands in that same buffer, so the forged-verdict
repros are credible.

The severity claim ("guest-controlled in both directions… hide a real one") does not survive.
Substring matching is monotonic: extra guest output can only *add* matches, never remove the
interpreter's real diagnostics, and the sandbox arm is tested first, so a guest committing a real
violation cannot reach `Verdict::Completed`. The exploit is therefore one-directional — a guest
can mislabel or self-deny its own run, corrupting audit records and exit codes, but cannot
launder a genuine capability violation into a clean verdict. Real containment is the
`sandbox_run` wrapper (`:299-318`), which is unaffected. Audit-integrity defect, not a
containment bypass.

*Fix:* classify on a structured channel (exit-code sidecar or a fd the guest cannot write), and
in the interim drop the bare `capability` substring and anchor matches to a line-leading
`axon: ` prefix.
