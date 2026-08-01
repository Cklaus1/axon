# Adversarial triage — crit-3.md (5 findings)

Method: each cited file/line opened and read in context; a finding survives only where
the code itself proves the defect and no caller/gate/cfg blocks it.

---

## F132 — Sandbox ceiling collapses FS/Exec/console into a single "IO" effect

**VERDICT: CONFIRMED (with one sub-claim refuted)**
**Severity: CRITICAL -> HIGH**

The cited code says exactly what the finding claims. `crates/axon-core/src/builtins.rs:2209-2211`:

```rust
"println" | "print" | "eprintln" | "eprint" | "read_line" | "read_file" | "write_file"
| "env_var" | "exit" => &["IO"],
"exec" => &["IO"],
```

There is no `FS` or `Exec` tag anywhere in `builtin_effect_row`, and the enforcement at
`crates/axon-core/src/interp/builtins.rs:192-205` keys on precisely that function
(`let row = crate::builtins::builtin_effect_row(name); … if !sb.allowed.contains(eff)`).
So a ceiling of `"IO"` — the minimum that permits `println` — necessarily also admits
`exec` and unrestricted `read_file`/`write_file`. The claimed inconsistency is also real:
`crates/axon-core/src/capabilities.rs:331` (`capability_of_builtin`) does distinguish
`fs:read`/`fs:write`/`net`/`exec` and is called in the *same* `call_builtin` at
interp/builtins.rs:172 for the R28/F3 audit ledger, so two classifiers with different
granularity coexist in one function with no drift test between them.

Sub-claim REFUTED: "Zero tests exist for the feature." Three F5 unit tests exist at
`crates/axon-core/src/interp.rs:3772`, `:3796`, `:3816`
(`sandbox_run_enforces_effect_ceiling_at_runtime_f5`, the allowed-effect positive, and the
pure-fn case) and they do assert the violation exit code. They only exercise the `Random`
axis, which is exactly why the IO conflation went unnoticed — but "zero tests" is wrong.

Downgraded to HIGH, not CRITICAL: the feature is interp-only (codegen E0910-refuses it),
has one in-tree consumer (`axon-os`), and the coarseness is documented as deliberate at
`crates/axon-os/src/runtime.rs:249-252`. It is a genuine capability-confusion defect, not
a silent regression in a broadly-deployed path.

**Fix:** Give `read_file`/`write_file`/`env_var` an `FS` row and `exec` an `Exec` row (or
enforce on `capability_of_builtin`), plus a drift test asserting the two classifiers agree
over every `BUILTINS` entry.

---

## F001 — Nested `sandbox_create`/`sandbox_run` replaces rather than intersects the ceiling

**VERDICT: CONFIRMED**
**Severity: CRITICAL -> HIGH**

Both halves are literally in the code. The exemption, `crates/axon-core/src/interp/builtins.rs:191`:

```rust
if sb_handle >= 0 && name != "sandbox_create" && name != "sandbox_run" {
```

and the replacement, `crates/axon-core/src/interp/builtins.rs:2836`:

```rust
let prev_sandbox = self.active_sandbox.replace(sb_handle);
let result = self.call_fn(f, vec![Value::Int(arg)]);
self.active_sandbox.set(prev_sandbox);
```

`active_sandbox` is a single `Cell<i64>` (`interp.rs:477`), not a stack of ceilings, and
`sandbox_create` performs no intersection with the currently-active set. Sandboxed code can
therefore call `sandbox_create(p, "AI,Net,IO,Random,Time")` and `sandbox_run` itself into it,
discarding the ceiling it was placed under — and the two builtins needed to do so are the
exact two the check exempts. Reachable in the real `axon-os` path, where the sandboxed
program is untrusted job source (`crates/axon-os/src/runtime.rs:268-274` wraps it verbatim
and the only other layer is a substring scan at `runtime.rs:218-242`).

HIGH rather than CRITICAL for the same scoping reason as F132: interp-only, one consumer,
young opt-in feature — but it does fully void the advertised F5 property.

**Fix:** Make the ceiling monotone — keep a stack of active sandboxes, require every frame
to allow an effect, and have `sandbox_create` inside an active sandbox intersect (or refuse).

---

## F152 — Approval is not bound to the artifact

**VERDICT: CONFIRMED (weaker than framed)**
**Severity: CRITICAL -> MEDIUM**

`cmd_ast_approve` does compute and record the hash — `crates/axon-core/src/main.rs:4996`
`let hash = fnv1a_hex(src.as_bytes());` written into the record at :5003-5006 — and deploy
does read only file existence, `main.rs:5355-5356`:

```rust
let approved_path = PathBuf::from(format!("{}.approved", file.display()));
let is_approved = approved_path.exists();
```

A grep confirms the `hash` field is written once (5004) and read nowhere; `fnv1a_hex`
(main.rs:5690) is a 64-bit non-cryptographic hash, unusable as a commitment either way.

However, the finding's framing overstates it: the approval was never a gate. The line
immediately above is commented `// Check if approve file exists (informational, not
blocking).` and the only consequence of a *missing* record is a stderr warning at
main.rs:5504-5508 — deploy runs unapproved files identically. So nothing is being
*bypassed*; the real defect is that `"approved":true` in the `axon-deploy/1` JSON (main.rs:5457,
:5499) and the axon-web pane (`crates/axon-web/src/html.rs:296`) assert a human commitment
that the record cannot carry. That is a misleading-provenance bug, not a broken enforcement
boundary — MEDIUM, and it becomes HIGH the moment anything starts gating on `approved`.

**Fix:** Re-hash the source at deploy with SHA-256 and set `approved:false` (and refuse
under a `--require-approval` flag) on mismatch or missing record.

---

## F140 — `axon-os verify` does not authenticate `verdict`/`seed`/`run_id`

**VERDICT: CONFIRMED**
**Severity: CRITICAL -> HIGH**

`record::build` (`crates/axon-os/src/record.rs:126-163`) chains only events: `prev` is
seeded from `manifest_digest`, each link is `event_hash(prev, seq, e)` where the canonical
input is `"{seq}{UNIT}{action}{UNIT}{target}{UNIT}{caps_used}{UNIT}{label}"` (record.rs:113-121),
and `record_digest = format!("axrec1:{prev}")` (record.rs:152). `verdict`, `seed` and
`run_id` are assigned into the struct at :155-161 without ever entering the hash.
`record::verify` (:169-200) walks exactly the same event chain and compares `record_digest`
to the chain head — it never recomputes `manifest_digest` (the manifest is not in the
record) and never touches `rec.verdict` or `rec.seed`. Since `rec.verdict.exit_code()` is
what the CLI returns (`crates/axon-os/src/cli.rs:385`), the single field that determines
the run's outcome is the one field forgeable without tripping exit 11.

**Fix:** Append a terminal `verdict`/`seed`/`run_id` pseudo-event to the chain before
computing `record_digest`, plus a regression test mutating `verdict` and asserting exit 11.

---

## F040 — Grant path/host allowlists are never enforced at runtime

**VERDICT: CONFIRMED**
**Severity: CRITICAL -> HIGH**

`wrap_in_sandbox` (`crates/axon-os/src/runtime.rs:253-275`) discards every prefix and host,
keeping only three booleans:

```rust
if ceiling.net { tags.push("Net"); tags.push("AI"); }
if ceiling.fs_read || ceiling.fs_write || ceiling.exec { tags.push("IO"); }
```

`EffectSet` is boolean-only by construction (`crates/axon-os/src/grant.rs:75-76`), and
`Grant::effects()` (grant.rs:97-98) collapses `Vec<PathPrefix>` to `!is_empty()`. The
prefix lists survive only in the *static* attenuation algebra (`prefixes_within`,
`intersect_prefixes`, grant.rs:116-133) and in the operator-facing text
(`crates/axon-os/src/cli.rs:69,74` render "read ./data/", "write ./out/"). The only other
runtime layer, `scan_effects` (runtime.rs:218-242), is a substring presence test with no
path or host awareness. So a job approved with `fs_write=["./out/"]` runs under ceiling
`"IO"` and may write anywhere — and, compounded by F132, may also `exec`. Every scope word
in the signed token is decorative at execution time.

**Fix:** Carry the fs prefix lists and net host list into the sandbox entry and check them
in the `read_file`/`write_file`/`http_*` arms of `interp/builtins.rs` (or emit
`@[contained(fs: [...], net: [...])]` on the entry fn so the E1001 path machinery applies).
