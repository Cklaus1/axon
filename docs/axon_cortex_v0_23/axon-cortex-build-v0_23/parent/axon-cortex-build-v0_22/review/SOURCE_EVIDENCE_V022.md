# Source evidence — 0.22 integration

Exact excerpts from the supplied files. Line numbers refer to those input files. SHA-256 pins byte identity, not authorship or execution. Reported Git labels were not independently Git-verified. Recommendations and new requirements are stated separately in the integration contract.

## A01 — axon/crates/axon-os/src/supervisor.rs

Existing supervisor owns approval/admission and observed run records; extend that convergence point.

Lines 18–104; SHA-256 `7199b59f3d7101bc6c6cc638834f6812df7102cc180f6d50630a309dffb97eb2`.

```text
18: pub fn run(
19:     manifest: &JobManifest,
20:     job_path: &std::path::Path,
21:     supervisor_grant: &crate::grant::Grant,
22:     run_id: &str,
23:     rt: &impl Runtime,
24: ) -> RunRecord {
25:     // 0. AUTHORIZATION — before anything else, at the point every execution
26:     //    path converges on.
27:     //
28:     //    This check lived in `cmd_run`, so `axon-os replay` reached execution
29:     //    by a different route and never performed it. REPRODUCED: a stored job
30:     //    marked `require_approval = true` with no token anywhere was refused by
31:     //    `run` with exit 8 and no side effect, then re-executed by `replay`
32:     //    with exit 0 and the side-effect file's mtime advancing. The public
33:     //    `axon_os::supervise` re-export was a third route with no gate at all.
34:     //
35:     //    Step 3 below already demonstrates the pattern: `admit` sits here and
36:     //    every caller gets it whether or not they remember. A check in a CALLER
37:     //    is opt-in per call site.
38:     let approval = match crate::approval::authorize(job_path, manifest) {
39:         Ok(a) => a,
40:         Err(reason) => {
41:             let denial = RawEvent::new("denied", "approval", EffectSet::default(), "");
42:             let mut rec = build(
43:                 run_id,
44:                 manifest,
45:                 manifest.seed,
46:                 std::slice::from_ref(&denial),
47:                 Verdict::Denied {
48:                     reason,
49:                     axis: "approval".to_string(),
50:                 },
51:             );
52:             rec.approval = crate::approval::ApprovalStatus::Denied.as_str().to_string();
53:             return rec;
54:         }
55:     };
56:     // 1. What does the program declare it may do? (deny-by-default via the runtime)
57:     let declared = rt.declared_effects(&manifest.program);
58: 
59:     // 2. The effective grant: you cannot delegate authority you lack.
60:     let eff = manifest.grant.intersect(supervisor_grant);
61: 
62:     // 3. Static admission — fail closed BEFORE any execution.
63:     // The authorization decision travels with the record, so a reader can tell
64:     // afterward whether the run was authorized. Stamped on EVERY exit path —
65:     // the first version computed it and used it on none, and the compiler said
66:     // so in a warning I walked past. A record field that is always `unknown` is
67:     // exactly the absent-vs-verified collapse this field exists to close.
68:     let approval_str = approval.as_str().to_string();
69: 
70:     if let Admission::Deny { reason, axis } = admit(&declared, &eff) {
71:         let denial = RawEvent::new("denied", &axis, EffectSet::default(), "");
72:         let mut rec = build(
73:             run_id,
74:             manifest,
75:             manifest.seed,
76:             std::slice::from_ref(&denial),
77:             Verdict::Denied { reason, axis },
78:         );
79:         rec.approval = approval_str;
80:         return rec;
81:     }
82: 
83:     // 4. Mint a Principal holding exactly the effective grant.
84:     let principal = rt.mint_principal(&eff);
85: 
86:     // 5. Run sandboxed to the effective ceiling + budget + seed.
87:     let outcome = rt.run_sandboxed(
88:         &manifest.program,
89:         &principal,
90:         &eff,
91:         &eff.budget,
92:         manifest.seed,
93:     );
94: 
95:     // 6. Seal a tamper-evident record from the observed events + verdict.
96:     let mut rec = build(
97:         run_id,
98:         manifest,
99:         manifest.seed,
100:         &outcome.events,
101:         outcome.verdict,
102:     );
103:     rec.approval = approval_str;
104:     rec
```

## A02 — axon/crates/axon-os/src/runtime.rs

Program-oriented Runtime is not the generic arbitrary-native sandbox API.

Lines 17–66; SHA-256 `7b033ba159eee680c20795c77b6007e0dfd7558227db65008897858a43b8158f`.

```text
17: pub struct PrincipalHandle(pub usize);
18: 
19: /// The result of running a program inside the sandbox: the observed
20: /// capability-bearing actions and the sealing verdict.
21: #[derive(Debug, Clone, PartialEq, Eq)]
22: pub struct RunOutcome {
23:     pub events: Vec<RawEvent>,
24:     pub verdict: Verdict,
25: }
26: 
27: /// The seam. Every method that touches the model/interpreter/OS lives here.
28: pub trait Runtime {
29:     /// The effect row a program declares it may perform. An error / absent
30:     /// declaration MUST map to `DeclaredEffects::unknown()` (deny-by-default).
31:     fn declared_effects(&self, program: &Path) -> DeclaredEffects;
32: 
33:     /// Mint a Principal holding exactly `grant`. Attenuation (no authority the
34:     /// supervisor lacks) is guaranteed by the caller passing the effective
35:     /// grant `J ∩ S`; the runtime mints to that, no more.
36:     fn mint_principal(&self, grant: &Grant) -> PrincipalHandle;
37: 
38:     /// Run `program` as `principal` inside a sandbox enforcing `ceiling` +
39:     /// `budget` with a fixed `seed`. Returns the observed events and the verdict
40:     /// (mapping any runtime over-reach to Denied/BudgetExhausted/RefineViolation).
41:     /// AUDIT T3: takes the full `Grant`, not just its induced `EffectSet`.
42:     /// `effect_set()` reduces the grant to four booleans, discarding the path
43:     /// prefixes and host allowlists — so the ceiling could only ever express
44:     /// "may write SOMEWHERE", never "may write ./out/". The scoped runtime
45:     /// needs the allowlists themselves.
46:     fn run_sandboxed(
47:         &self,
48:         program: &Path,
49:         principal: &PrincipalHandle,
50:         grant: &Grant,
51:         budget: &Budget,
52:         seed: u64,
53:     ) -> RunOutcome;
54: }
55: 
56: // ── S6: the real runtime — hermetic, isolated subprocess execution (A4) ──────
57: 
58: use crate::gate::DeclaredEffects as Decl;
59: use crate::grant::Label;
60: use crate::record::RawEvent as RE;
61: use std::path::PathBuf;
62: use std::process::{Command, Stdio};
63: use std::time::Duration;
64: 
65: /// The real `Runtime`: runs programs by invoking the canonical `axon`
66: /// interpreter in a fresh, time-bounded subprocess (R21 §4.4). The only impure
```

## A03 — axon/crates/axon-cortex/src/runner.rs

Authorized capability is distinct from a bare action.

Lines 110–146; SHA-256 `3f970015a52590d79805654ceb0f1e0b25ee0425c9ca55661b2dbd169e232e68`.

```text
110: ///
111: /// The field is private and there is no public constructor, so the only way to
112: /// hold one is to have been given it by [`Runner::authorize_action`]. That is
113: /// what makes `execute` unable to run an unauthorized action: not a check
114: /// inside execute that could be forgotten or reordered, but a value the caller
115: /// cannot produce without passing the gate.
116: ///
117: /// It borrows the action rather than copying it, so the thing executed is
118: /// necessarily the thing authorized — a copy could drift between the two calls.
119: #[derive(Debug)]
120: pub struct Authorized<'a> {
121:     action: &'a CortexAction,
122: }
123: 
124: impl<'a> Authorized<'a> {
125:     pub fn action(&self) -> &'a CortexAction {
126:         self.action
127:     }
128: }
129: 
130: /// What executing an action actually did.
131: ///
132: /// Every variant is a distinct observed outcome. There is no `Ok`/`Err` pair,
133: /// because "the symbol was not found" and "the check failed" are different
134: /// facts that a caller and an auditor both need to tell apart, and collapsing
135: /// them into one error string is how a missing target comes to read as a
136: /// failing test.
137: #[derive(Debug, Clone, PartialEq, Eq)]
138: pub enum ExecOutcome {
139:     /// Read a symbol's body. No side effect.
140:     Inspected { symbol: String, body: String },
141:     /// The named symbol is not in the file. NOT an error and NOT a failure:
142:     /// the action could not be carried out, which is a third thing.
143:     SymbolNotFound { symbol: String, path: String },
144:     /// A named check ran. `matched` is how many tests the name selected — 0
145:     /// means the check does not exist, which is never a pass.
146:     CheckRan {
```

## A04 — axon/crates/axon-cortex/src/runner.rs

Registered check execution seam; the new CheckExecutor is proposed, not claimed present.

Lines 1248–1339; SHA-256 `3f970015a52590d79805654ceb0f1e0b25ee0425c9ca55661b2dbd169e232e68`.

```text
1248:     ///   so a PASSING test named `test_FAILED_path` was recorded as failed.
1249:     /// * test bodies run in-process and their stdout is not captured, so a
1250:     ///   `print` without a trailing newline glues the next result line to it
1251:     ///   and that test vanishes from the run.
1252:     ///
1253:     /// Each of those is a different bug with one cause: the transcript is for
1254:     /// people, and its shape may change for reasons that have nothing to do
1255:     /// with this caller. `--json` is the contract meant to be parsed. A test
1256:     /// body could still print a line that happens to be a matching JSON
1257:     /// object; nothing here can prevent that, and it is a far narrower target
1258:     /// than a line beginning with "test ".
1259:     ///
1260:     /// Returns `(failed, passed, total)`. `total` comes from the run's own
1261:     /// summary, so "the filter matched nothing" is distinguishable from "every
1262:     /// test passed" — those are byte-identical in the human transcript, and
1263:     /// both exit 0.
1264:     fn run_tests_json(
1265:         &self,
1266:         rel_path: &str,
1267:         filter: Option<&str>,
1268:     ) -> std::io::Result<(Vec<String>, Vec<String>, usize)> {
1269:         let mut cmd = std::process::Command::new(&self.axon_bin);
1270:         cmd.arg("test")
1271:             .arg(self.workspace.join(rel_path))
1272:             .arg("--json");
1273:         if let Some(f) = filter {
1274:             cmd.arg("--filter").arg(f);
1275:         }
1276:         let out = cmd.output()?;
1277:         let text = format!(
1278:             "{}{}",
1279:             String::from_utf8_lossy(&out.stdout),
1280:             String::from_utf8_lossy(&out.stderr)
1281:         );
1282:         let (mut failed, mut passed, mut total) = (Vec::new(), Vec::new(), None);
1283:         for line in text.lines() {
1284:             let line = line.trim();
1285:             if !line.starts_with('{') {
1286:                 continue;
1287:             }
1288:             let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
1289:                 continue;
1290:             };
1291:             if v.get("type").and_then(|t| t.as_str()) == Some("summary") {
1292:                 total = v.get("total").and_then(|t| t.as_u64()).map(|t| t as usize);
1293:                 continue;
1294:             }
1295:             let (Some(name), Some(status)) = (
1296:                 v.get("name").and_then(|n| n.as_str()),
1297:                 v.get("status").and_then(|s| s.as_str()),
1298:             ) else {
1299:                 continue;
1300:             };
1301:             match status {
1302:                 "ok" => passed.push(name.to_string()),
1303:                 "failed" => failed.push(name.to_string()),
1304:                 // An unknown status is neither. Guessing which it resembles is
1305:                 // how a new status becomes a silent wrong answer.
1306:                 _ => {}
1307:             }
1308:         }
1309:         // No summary means the run did not finish — a compile error, a crash,
1310:         // a missing binary. That is an ERROR, never "no tests failed".
1311:         let Some(total) = total else {
1312:             return Err(std::io::Error::other(format!(
1313:                 "`axon test --json` produced no summary for {rel_path}; the run \
1314:                  did not complete, which is not the same as nothing failing"
1315:             )));
1316:         };
1317:         Ok((failed, passed, total))
1318:     }
1319: 
1320:     /// Run ONE named check and report how many tests the name matched.    /// Run ONE named check and report how many tests the name matched.
1321:     ///
1322:     /// `run_check` passes the name to the episode record but not to the
1323:     /// command, so it runs the whole file and reports a named result — the same
1324:     /// defect `verify()` had. This selects, and returns the match count so a
1325:     /// zero-match run can be told from a pass.
1326:     fn run_named_check(&mut self, name: &str, rel_path: &str) -> std::io::Result<(bool, usize)> {
1327:         let (failed, passed, _) = self.run_tests_json(rel_path, Some(name))?;
1328:         // `--filter` is a SUBSTRING match, so it can select more than the
1329:         // check asked for. The count reported is what actually ran under that
1330:         // name, and a filter matching nothing exits 0 with an `ok` summary —
1331:         // zero matched tests is not a pass.
1332:         let matched = failed.len() + passed.len();
1333:         // A DIFFERENT question from the hidden check's, and it took a
1334:         // regression to make that clear. The adjudicator is an exact test name
1335:         // the operator supplied, so its verdict must come from that exact name
1336:         // (`verdict_for`). This check is named after the SYMBOL under repair,
1337:         // which is not a test name at all — it is a filter selecting the tests
1338:         // that touch that symbol, and the question is whether any of THEM
1339:         // failed.
```

## A05 — axon/crates/axon-vm/src/main.rs

Actual Firecracker binary is the extraction target; a separate full-VM runtime is not assumed.

Lines 1–83; SHA-256 `d2da243325792507c9f3ea88f87179eb3271525b160125ce30015dc728308232`.

```text
1: //! axon-vm — Axon microVM launcher.
2: //!
3: //! Wraps Firecracker to run an Axon program in a hardware-isolated microVM with:
4: //!   • Capability-derived seccomp-BPF policy (from `.axmeta` manifest)
5: //!   • MMDS-delivered boot policy (principal, budget, allowed effects, BPF)
6: //!   • vsock host_await substrate for interactive programs
7: //!   • Principal registry (~/.config/axon/principals.toml)
8: //!   • R26: software-TPM attestation gate (axon-vm attest)
9: //!
10: //! Commands:
11: //!   axon-vm run <program.ax>  [options]     Launch program in a microVM
12: //!   axon-vm attest --kernel K [options]     Measure kernel, produce attestation report
13: //!   axon-vm principal add <name> [options]  Register a principal
14: //!   axon-vm principal list                  List principals
15: 
16: use std::io::{Read, Write};
17: use std::os::unix::net::UnixStream;
18: use std::path::{Path, PathBuf};
19: use std::process::{Command, Stdio};
20: use std::sync::{Arc, Mutex};
21: use std::time::{Duration, Instant};
22: use std::{env, fs, process};
23: 
24: use base64::Engine as _;
25: use clap::{Parser, Subcommand};
26: use serde::{Deserialize, Serialize};
27: 
28: use axon_attest::{
29:     measure_host_stack, measure_kernel, measure_kernel_bytes, report_to_json,
30:     report_to_json_extended, sign_report, try_admit_job, verify_extended, verify_report,
31:     SOFTWARE_TPM_HW_ROOT,
32: };
33: 
34: /// R33: cross-VM safety quorum (VoteRequest/Response + strict-majority check).
35: /// NOTE (T49): votes are NOT authenticated — see `quorum` module header.
36: mod quorum;
37: 
38: /// R34: incremental attestation rolling hash chain (ChainStore, compute_entry_hash).
39: mod chain;
40: 
41: /// R34: chain verification/tamper/stale-root failure (exit 15). Distinct from
42: /// 10 (attestation mismatch), 11 (TCB chain break, unused today), 12 (extended
43: /// measure failure), 13/14 (R33 quorum-blocked / vote-attestation-rejected).
44: /// Confirmed free of 1/2/10/12/13/14 before being claimed
45: /// (`governance/specs/R34-incremental-attestation.md` spec-meta `reserves`).
46: const CHAIN_VERIFY_FAIL_EXIT_CODE: i32 = 15;
47: 
48: /// R31: extended measurement failed — required component missing/unreadable (exit 12).
49: const EXTENDED_TCB_MEASURE_FAIL: i32 = 12;
50: 
51: /// R31/T52: the measured extended TCB did not match the pinned expectation, or
52: /// no expectation was pinned at all. Shares the attestation exit code (10) with
53: /// the kernel-baseline gate — both mean "the software about to run is not the
54: /// software that was blessed".
55: const EXTENDED_TCB_MISMATCH: i32 = 10;
56: 
57: /// R33: cross-VM safety quorum not met — insufficient approvals (or empty/timeout
58: /// in the fuller protocol). Reserved per `governance/specs/R33-cross-vm-safety-quorum.md`
59: /// spec-meta; confirmed free of the existing axon-vm exit codes (1, 2, 10, 12) and of
60: /// R34's separately-reserved 15 before being claimed.
61: const QUORUM_BLOCKED_EXIT_CODE: i32 = 13;
62: 
63: /// R33: cross-VM safety quorum blocked specifically by an attestation mismatch
64: /// (voters disagree on `voter_tcb`) — distinct from ordinary insufficient-approvals
65: /// (`QUORUM_BLOCKED_EXIT_CODE`); never collapsed into it (spec §8 invariant I-6).
66: const QUORUM_ATTEST_FAIL_EXIT_CODE: i32 = 14;
67: 
68: // ── CLI surface ───────────────────────────────────────────────────────────────
69: 
70: #[derive(Parser)]
71: #[command(name = "axon-vm", about = "Run Axon programs in Firecracker microVMs")]
72: struct Cli {
73:     #[command(subcommand)]
74:     cmd: Cmd,
75: }
76: 
77: #[derive(Subcommand)]
78: enum Cmd {
79:     /// Run an Axon program in a microVM
80:     Run {
81:         /// Path to the .ax program to run
82:         program: PathBuf,
83: 
```

## A06 — axon/crates/axon-reflex/src/lib.rs

Reflex decision-core behavior in this snapshot is not evidence of a CLM implementation.

Lines 227–264; SHA-256 `92cd5835658e6940a29f1d868b94ab53e47692be64b5aa83666c2603d0f2d9e7`.

```text
227:                 owner: owner.clone(),
228:                 caller: caller.to_string(),
229:             }),
230:         }
231:     }
232: 
233:     pub fn decide(
234:         &mut self,
235:         id: &str,
236:         question: &str,
237:         scope: &PrincipalScope,
238:     ) -> Result<Decision, Refusal> {
239:         self.authorize(id, &scope.principal)?;
240:         // Deterministic by construction: a fixed function of the question.
241:         // Phase 1 is about the SEAM, not about inference quality, and a
242:         // stochastic stub would make the cross-mode comparison meaningless.
243:         Ok(Decision {
244:             choice: format!("decided({question})"),
245:             principal: scope.principal.clone(),
246:         })
247:     }
248: 
249:     pub fn release_state(&mut self, id: &str, scope: &PrincipalScope) -> Result<(), Refusal> {
250:         self.authorize(id, &scope.principal)?;
251:         self.states.remove(id);
252:         Ok(())
253:     }
254: }
255: 
256: // ── Embedded ────────────────────────────────────────────────────────────────
257: 
258: /// In-process. The mode with the most ways to leak state across principals,
259: /// since there is no address-space boundary doing any of the work.
260: pub struct EmbeddedBackend {
261:     core: ReflexCore,
262: }
263: 
264: impl Default for EmbeddedBackend {
```

## A07 — axon/scripts/cortex_package_gate.sh

Source gate has legacy vendor/manifest/CLI expectations; use reviewed migration rather than only changing a path.

Lines 1–110; SHA-256 `61b23c11afbf934452b71a4fed4f26bb1702c6ed13ce85a79cb51daa549f94b5`.

```text
1: #!/usr/bin/env bash
2: # cortex_package_gate.sh — continuous check for the vendored Cortex v0.15 build package.
3: #
4: # WHY THIS EXISTS.
5: #
6: # `docs/axon_cortex_v0_15/axon-cortex-build-v0_15/` is a 114-file documentation
7: # package: 35 specs, 152 work packages and 247 PROPOSED product gates. Every one
8: # of those 247 carries `"product_result": "NOT_RUN"` and
9: # `"implementation_status": "Not implemented in this package"`, and every work
10: # package is `"status": "Not started"`. The package is scrupulous about saying so.
11: #
12: # That honesty is the property most easily destroyed on intake. This repository
13: # already has the defect at smaller scale: 7 of the 37 `scripts/*.sh` cited as
14: # evidence in `governance/REQUIREMENTS.md` are invoked by nothing, so a
15: # requirement reads as verified while its cited gate has never run. 247 more
16: # gate IDs, each one edit away from reading as executed, would multiply it.
17: #
18: # So this gate asserts two things on every run:
19: #
20: #   1. INTEGRITY. `SHA256SUMS_v0_15.json` (schema `cortex-package-sha256/1`) is
21: #      checked in BOTH directions -- every listed file hashes to its recorded
22: #      digest, AND no unlisted file has appeared under the package root. One
23: #      direction alone would miss a file being ADDED. The manifest itself is the
24: #      single legitimate unlisted file (it cannot contain its own hash).
25: #      The package's own `tools/validate_package.py` is also run -- it is a real
26: #      check that, before this script, ran nowhere: the same orphaned-verification
27: #      class described above.
28: #
29: #   2. HONESTY. A gate may claim a `product_result` other than NOT_RUN, and a work
30: #      package a `status` other than "Not started", ONLY when
31: #      `governance/cortex_gate_execution_registry.json` carries a row naming a
32: #      file in THIS repository that exists and is invoked by something that runs.
33: #      Today the registry is empty, so all 247 must remain NOT_RUN and this is a
34: #      tripwire. It is deliberately NOT a re-assertion of today's constants: when
35: #      CX-02's gates are implemented for real, the documented path is to add a
36: #      registry row (see `how_to_add_a_row` in that file) rather than to weaken
37: #      this script. The row is not self-certifying -- the invoker is grepped for
38: #      the executed script, so a row pointing at an orphan is rejected, and every
39: #      row is re-validated on every run so one cannot rot into a rubber stamp.
40: #
41: #      Note the vendored manifest is NOT where an implemented gate gets recorded:
42: #      the package's own validator hard-fails on any product_result other than
43: #      NOT_RUN, and the sha256 manifest pins the file. NOT_RUN is a true statement
44: #      about the PACKAGE and stays true however much this repo implements. The
45: #      registry is this repository's execution record; the manifest clause exists
46: #      for the day an upstream release re-vendors results in.
47: #
48: # NON-VACUITY. Verifying zero files or parsing zero gates is a FAILURE, not a
49: # pass. This repository has shipped a harness that exited 0 having compared
50: # nothing; the floors below exist so this cannot be another one.
51: #
52: # COST. Pure text and SHA256 over 1.9MB plus one python3 run. No cargo, no
53: # toolchain, no network. Cheap enough for every gate run.
54: #
55: # Exit 0 = package intact and still honest. Exit 1 = one of the two broke.
56: 
57: set -uo pipefail
58: cd "$(dirname "$0")/.."
59: 
60: PKG="docs/axon_cortex_v0_15/axon-cortex-build-v0_15"
61: SUMS="$PKG/SHA256SUMS_v0_15.json"
62: REGISTRY="governance/cortex_gate_execution_registry.json"
63: REPORT="target/cortex-package-validation.json"
64: 
65: # Non-vacuity floors. Minimums, not exact values: a legitimately re-vendored
66: # package may grow. A truncated or empty manifest trips these.
67: MIN_FILES=100
68: MIN_GATES=200
69: MIN_TASKS=100
70: 
71: # Prerequisite problems abort; CHECK failures accumulate. Reporting every broken
72: # section in one run matters here because a single bad edit trips more than one:
73: # flipping a gate to PASS breaks BOTH the sha256 manifest and the honesty
74: # invariant, and seeing only the first would hide which property actually moved.
75: FAILURES=()
76: abort()     { echo "❌ cortex package gate ABORTED: $1"; exit 1; }
77: note_fail() { echo "   ↳ FAILED: $1"; FAILURES+=("$1"); }
78: 
79: command -v python3 >/dev/null 2>&1 || abort "python3 not found (required to parse the package manifests)"
80: [ -d "$PKG" ] || abort "package root missing: $PKG"
81: [ -f "$SUMS" ] || abort "sha256 manifest missing: $SUMS"
82: [ -f "$REGISTRY" ] || abort "execution registry missing: $REGISTRY"
83: mkdir -p target
84: 
85: echo "── cortex: package integrity (SHA256SUMS_v0_15, both directions) ──"
86: python3 - "$PKG" "$SUMS" "$MIN_FILES" <<'PY'
87: import hashlib, json, os, sys
88: pkg, sums, min_files = sys.argv[1], sys.argv[2], int(sys.argv[3])
89: man = json.load(open(sums, encoding="utf-8"))
90: if man.get("schema") != "cortex-package-sha256/1":
91:     print(f"  manifest schema is {man.get('schema')!r}, expected cortex-package-sha256/1"); sys.exit(1)
92: listed = man["files"]
93: if not isinstance(listed, dict) or len(listed) < min_files:
94:     print(f"  NON-VACUITY: manifest lists {len(listed) if hasattr(listed,'__len__') else '?'} files, floor is {min_files}"); sys.exit(1)
95: 
96: present = set()
97: for root, _dirs, files in os.walk(pkg):
98:     for f in files:
99:         present.add(os.path.relpath(os.path.join(root, f), pkg))
100: 
101: # KNOWN DEVIATION, pinned. Exactly one file in this repository's copy differs
102: # from the upstream manifest, and the cause is the footgun documented below:
103: # `tools/validate_package.py` writes its report to <root>/package_validation.json
104: # BY DEFAULT, and that file is itself hash-listed. The intake session ran the
105: # validator to verify the package (reporting "113/113 files match" -- true when
106: # measured, false immediately afterwards) and the rewritten file, carrying a
107: # fresh `validated_at` timestamp, is what got committed in 32937d3.
108: #
109: # The file is NOT un-checked here: it is pinned to the digest actually committed,
110: # so any FURTHER change to it still fails. The deviation lives in this script
```

## M01 — micode/docs/axon-support/README.md

Embedded support roadmap is v0.3 and separates Current/Intended/Partial/Deferred/UNVERIFIED.

Lines 1–40; SHA-256 `b154dd731e18eb6f9d39a861cd1c033717f79831e86ba7a53db3c5edc94f972f`.

```text
1: # MiCode → Axon Support Program v0.3
2: 
3: **Document type:** implementation roadmap · spec family · task DAG · build protocol  
4: **Primary objective:** evolve MiCode from a coding-agent product into the controlled software-world laboratory, evidence generator, transfer benchmark plane, and knowledge interface that accelerates Axon Cortex and the Axon self-optimizing OS.
5: 
6: ## One-sentence thesis
7: 
8: MiCode should not merely become faster at orchestration. It should become the **instrumented coding world in which Axon learns how software behaves, how coding intelligence succeeds and fails, which bounded decisions transfer across unseen repositories, which abstractions survive protected evaluation, and which repeated behaviors deserve promotion into Axon Reflex, libraries, compiler passes, or runtime primitives.**
9: 
10: ## Scope
11: 
12: This package specifies MiCode-side work. It does **not** assume Axon Cortex is already implemented. MiCode remains independently useful and continues to enforce its own authority model.
13: 
14: The program adds eight platform roles:
15: 
16: 1. **Experience engine** — every eligible coding episode becomes structured, replayable evidence.
17: 2. **Intent/build bridge** — solution descriptions become typed intent, build specs, `/build-loop` execution, and evidence-bound completion.
18: 3. **Software observer** — repository state is represented as typed semantic objects, not only transcript text.
19: 4. **Reflex corpus producer** — shadow decisions become exact state/question/candidate/outcome records for Axon CX-20.
20: 5. **Experimental harness** — planners, retrievers, Reflex backends, generators, and verifiers can be compared under controlled tasks.
21: 6. **Coding Frontier environment** — incumbent/challenger systems run under matched authority, budgets, hidden acceptance checks, and T0–T5 transfer tiers for Axon CX-21.
22: 7. **Knowledge miner** — external repositories and histories yield evidence-backed pattern candidates while protected benchmark worlds remain uncontaminated.
23: 8. **Axon bridge** — admitted traces, datasets, benchmarks, abstractions, tasks, and results flow into Axon Cortex without transferring MiCode authority.
24: 
25: ## Status vocabulary
26: 
27: Use MiCode's own vocabulary literally: **Current**, **Intended**, **Partial**, **Deferred**, **UNVERIFIED**. No `Current` claim in these files means a new behavior exists unless a live use-site anchor is named in the repository.
28: 
29: ## Critical current limitations
30: 
31: The supplied MiCode snapshot states that there is no execution sandbox for approved shell work; `DelegationScope::path_globs` and `tool_allowlist` are not enforcement mechanisms; upward child→parent taint is partial; and several long-lived-work/cancellation semantics remain partial or deferred. Therefore experimental Reflex/world-model components begin as measurement/recommendation layers and may not widen authority.
32: 
33: ## Read order
34: 
35: 1. `ARCHITECTURE.md`
36: 2. `build/BUILD_PLAN.md`
37: 3. `build/LOOPS.md`
38: 4. `build/TASKS.md`
39: 5. `specs/INDEX.md`
40: 6. `build/ACCEPTANCE_GATES.md`
```

## M02 — micode/docs/axon-support/specs/MX-12-axon-bridge.md

Versioned CX-16/MX-12 boundary already owns policy/experience transfer without authority transfer.

Lines 1–50; SHA-256 `cff24178d3f29472321fa54ae817eccf7754a504d4823ac77fd1638c68a68a5a`.

```text
1: # MX-12 — Axon Bridge
2: **Status:** Intended
3: 
4: ## Objective
5: Create a versioned boundary between MiCode's experience plane and Axon Cortex without coupling MiCode internals directly to Axon implementation details.
6: 
7: ## Exportable artifact families
8: - canonical episodes;
9: - software observations and semantic object catalogs;
10: - bounded decision datasets;
11: - failure-attribution records;
12: - knowledge/abstraction candidates;
13: - benchmark/evaluation bundles;
14: - Skill/Tool/Compiler candidate descriptions.
15: 
16: ## Importable artifact families
17: - AIR/Cortex decision policies;
18: - Reflex backends/adapters;
19: - Axon verifier profiles;
20: - capability schemas;
21: - approved ontology/version maps.
22: 
23: ## Rule
24: Importing an Axon artifact does not grant it MiCode authority. It remains subject to local MiCode policy.
25: 
26: ## Exit gate
27: A recorded MiCode episode can be exported, validated against a versioned schema, consumed by a dummy Axon-side reader, and round-tripped without semantic field loss.
28: 
29: ## Intent/build-loop artifact family (v0.2)
30: Additional exportable artifacts:
31: - Intent IR and revision lineage;
32: - Build Spec semantic/document digests;
33: - task-DAG snapshots/revisions;
34: - `/build-loop` run records;
35: - IntentConflict / PlanChangeProposal records;
36: - intent-verification outcomes.
37: 
38: These artifacts allow Axon to study design→decomposition→execution→evidence trajectories. They do not grant Axon or an imported policy the right to alter MiCode's local intent, approvals, or authority.
39: 
40: ## Reflex/Coding-Frontier artifact families (v0.3)
41: Additional exportable artifacts:
42: - `CanonicalDecisionRecord` and `QuestionDecompositionRecord`;
43: - Reflex-shadow result bundles and backend manifests;
44: - `StateHandle`/candidate-set replay identities;
45: - corpus-role and contamination manifests;
46: - `CodingFrontierEpisode` benchmark bundles;
47: - T0–T5 transfer-tier results;
48: - CX-20 Reflex Lab and CX-21 Coding Frontier conformance metadata.
49: 
50: The bridge preserves exact candidate order, canonical encoding version, AIR vocabulary version, acceptable-action sets, unknown counterfactuals, and evidence references. Training/evaluation exports may not silently re-render the decision into a different semantic format.
```

## M03 — micode/docs/axon-support/specs/MX-08-challenger-lab.md

Intended challenger harness is the integration owner, not proof that the live peer loop exists.

Lines 1–21; SHA-256 `86d518fad4f84aaa2ba7ddce66e6eda67f30b93cd5f720b4d879d525b7a2d24b`.

```text
1: # MX-08 — Challenger / Experimental Harness
2: **Status:** Intended
3: 
4: ## Objective
5: Run controlled incumbent-vs-challenger experiments for MiCode and Axon-support components.
6: 
7: ## Replaceable components
8: Observer, retriever, candidate generator, Reflex backend, planner, generator, verification strategy, context policy, failure attributor.
9: 
10: ## Controls
11: - same task corpus and frozen acceptance contracts;
12: - same authority/tool access unless the experiment explicitly studies them;
13: - grouped train/dev/test splits where learning is involved;
14: - failure cases remain in denominators;
15: - cost, latency, tool calls, model calls, build/test calls, human interventions tracked.
16: 
17: ## Exit gate
18: The harness can compare two decision policies on the same resettable task set and produce a reproducible evidence bundle.
19: 
20: ## Coding Frontier alignment
21: The challenger lab is the execution substrate for MX-19 matched incumbent/challenger runs. It must expose frozen authority, budget, task contract, visible-context policy, and acceptance-bundle identities so CX-21 comparisons cannot win by receiving an easier task.
```

## M04 — micode/crates/micode-delegate/src/spawner.rs

Documented lack of path_globs/tool_allowlist parent-subset enforcement must not become a security assumption.

Lines 77–110; SHA-256 `7454a73638668549704d331c15356315d3887ac132859f8fbec41d90670915be`.

```text
77: //! `SpawnError::CapExceeded` — reused rather than adding a new cross-crate `SpawnError` variant
78: //! for one specific cap kind; see this crate's doc comment discipline elsewhere) any requested
79: //! `DelegationScope::tier_ceiling` exceeding the IMMEDIATE parent's own tracked scope, when the
80: //! parent is itself a tracked child of a previous `spawn()` call.
81: //!
82: //! **The "untracked parent" gap is now CLOSED (ROADMAP_EXECUTION_SPEC §15.2 (c)).** Previously an
83: //! "untracked parent" was trusted as "the true top-level root" and had its ceiling check skipped —
84: //! and because `SessionHandle.task_id` was a plain public field, a freshly-fabricated handle this
85: //! spawner never returned was indistinguishable from the real root, so a `Critical`-ceiling child
86: //! spawned under it with NO ceiling check (reproduced empirically by the reviewer). Two changes
87: //! seal it: (1) `SessionHandle.task_id` is now crate-private to `micode-core`, so no downstream crate
88: //! can fabricate a handle by struct literal at all (the compile-time half — `SubagentSpawner`'s
89: //! `root_handle` doc carries the `compile_fail` repro); (2) `spawn` now fails **closed**
90: //! (`CapExceeded`) on any parent whose id it did not itself register, and the genuine top-level root
91: //! is registered via [`SubagentSpawner::root_handle`] rather than being an untracked handle (the
92: //! runtime half). Even the documented `SessionHandle::from_registered` escape cannot smuggle an
93: //! *unregistered* id past `spawn`. Residual, disclosed: a caller who already holds a *registered*
94: //! id can still build a handle for it and spawn under it (consuming that parent's own child slots —
95: //! a minor same-tree DoS, not a ceiling bypass). The formerly-residual *cross-subtree resume* half
96: //! of this is now closed — see the parent-scoping note above.
97: //!
98: //! **`path_globs`/`tool_allowlist` are NOT validated as a subset of the parent's** — only
99: //! `tier_ceiling` and (`SUBAGENT_SYNTHESIS_SPEC.md` §7.10 gap #10) `spawn_allowlist` are checked.
100: //! A real subset check for the globs needs glob-matching semantics this milestone doesn't specify;
101: //! disclosed, not silently skipped (see `BUILD_STATE.md`'s knowledge graph). The spawn allowlist
102: //! has no such excuse — it is plain set membership — which is why it did not join the disclosure.
103: 
104: use std::collections::HashMap;
105: use std::sync::{Arc, Mutex};
106: use std::time::Duration;
107: 
108: use micode_core::{
109:     AgentPath, AgentType, Collected, DelegationScope, MiCodeEvent, ProgressClock, RiskTier,
110:     SessionHandle, SpawnError, SubAgentCompletion, SubAgentHandle, SubAgentHost, SubAgentSpec,
```

## M05 — micode/crates/micode-git/src/worktree.rs

Existing worktree manager should be reused; worktree isolation is not OS confinement.

Lines 150–196; SHA-256 `68db1756a956c0376c20c94a2eb7325e488178741077e95df5a2b0048cefc904`.

```text
150:         };
151:         totals.lines_added += count(fields.next());
152:         totals.lines_removed += count(fields.next());
153:     }
154:     totals
155: }
156: 
157: /// Creates, lists, removes and reaps worktrees for one repository.
158: #[derive(Debug, Clone)]
159: pub struct WorktreeManager {
160:     repo: PathBuf,
161:     base: PathBuf,
162: }
163: 
164: impl WorktreeManager {
165:     /// Binds to `repo`, storing worktrees under `base`.
166:     ///
167:     /// `base` is injected rather than resolved internally so tests never touch a real `~/.micode`.
168:     /// Production passes [`default_base`].
169:     pub fn new(repo: impl Into<PathBuf>, base: impl Into<PathBuf>) -> Self {
170:         WorktreeManager {
171:             repo: repo.into(),
172:             base: base.into(),
173:         }
174:     }
175: 
176:     fn git(&self) -> Git {
177:         Git::at(&self.repo)
178:     }
179: 
180:     /// Where a given child's worktree lives. Pure path arithmetic; touches no disk.
181:     pub fn path_for(&self, session: &str, child: &str) -> PathBuf {
182:         self.base.join(session).join(child)
183:     }
184: 
185:     /// Can this repository host a worktree right now?
186:     ///
187:     /// `SUBAGENT_SYNTHESIS_SPEC.md` S3c: [`WorktreeManager::create`] answers this as a side effect
188:     /// of creating, which is too late for a caller that must **refuse a spawn before it starts**.
189:     /// This asks the same two questions with no side effects, so a spawner can fail cleanly.
190:     ///
191:     /// Deliberately shares `create`'s preconditions rather than restating them: the address check
192:     /// is the only one omitted, since it is per-child and this is per-repo. A caller passing this
193:     /// and then failing `create` on `AlreadyExists` is a real (rare) outcome, not a bug here.
194:     pub async fn repo_is_usable(&self) -> Result<(), String> {
195:         let git = self.git();
196:         match git.is_repo().await {
```

## M06 — micode/EXECUTION_CONTEXT_RECEIPT_SPEC.md

Observed context gates first model turn; parent/task/worker/integrator share a receipt.

Lines 35–85; SHA-256 `991de8603a0bf0b947f1f1155dae223d7be64529748ab73b606d5856b0206563`.

```text
35: ## The invariant
36: 
37: ```
38: TaskSpec
39:   ↓
40: ExpectedExecutionContext          (declared by the parent, part of task identity)
41:   ↓
42: MiCode provisions worktree
43:   ↓
44: child INDEPENDENTLY observes its own context
45:   ↓
46: expected == observed ?
47:       yes → ExecutionContextReceipt issued → task may transition to InProgress
48:       no  → TASK_NOT_STARTED, reason = EXECUTION_CONTEXT_MISMATCH
49: ```
50: 
51: Two properties carry the weight:
52: 
53: - **The child observes, it is not told.** A parent that passes down its own belief and asks the
54:   child to echo it verifies nothing. The check has value only because the observation is
55:   independent of the expectation.
56: - **It gates the first model turn.** Not the first commit, not the result — refusal must happen
57:   before any tokens are spent, because the whole cost of this bug is work done before anyone looks.
58: 
59: ## `ExecutionContextReceipt`
60: 
61: ```
62: ExecutionContextReceipt {
63:     task_id,
64:     parent_run_id,
65:     repo_id,
66:     expected_base_commit,
67:     observed_head,
68:     expected_branch,
69:     observed_branch,
70:     worktree_id,
71:     working_directory,
72:     write_scope,
73:     read_scope,
74:     build_namespace,        // dedicated cargo target dir
75:     model_identity,         // provider route + model actually resolved
76:     subagent_role,
77:     created_at,
78: }
79: ```
80: 
81: Three parties, one artifact:
82: 
83: - the **task engine** stores it,
84: - the **worker** cannot start without it,
85: - the **integrator** cannot admit a result without it.
```

## M07 — micode/EXECUTION_CONTEXT_RECEIPT_SPEC.md

Role-specific preflight, declared write scope and stale-on-return treatment already specified; exact base for paired experiments is a new stricter profile.

Lines 87–179; SHA-256 `991de8603a0bf0b947f1f1155dae223d7be64529748ab73b606d5856b0206563`.

```text
87: ## Hard conditions (implementation worker)
88: 
89: Fail closed on any:
90: 
91: - observed HEAD **descends from** expected base commit
92: - repo identity matches
93: - expected crate/module namespace **exists** (`crates/micode/`)
94: - known-stale namespace **does not exist** (`crates/atlas-*/`) — today's bug caught by a single
95:   negative existence check
96: - worktree is **not** the primary integration checkout
97: - dedicated build target dir configured
98: - declared write-set is valid and non-empty
99: 
100: Rendered as a preflight the human can read:
101: 
102: ```
103: SUBAGENT PREFLIGHT
104: ✓ HEAD descends from expected base b984027e
105: ✓ crates/micode exists
106: ✓ crates/atlas does NOT exist
107: ✓ worktree is not primary integration checkout
108: ✓ dedicated target dir configured
109: → task may start
110: ```
111: 
112: That block would have killed the two bad lanes in seconds.
113: 
114: ## Declared write-set
115: 
116: Worktree identity is one dimension; authority is another. A launch declares:
117: 
118: ```
119: task: model-picker-empty-state
120: execution:
121:   repo: micode
122:   base_commit: b984027e
123:   isolated_worktree: true
124:   cargo_target_dir: dedicated
125: writes:
126:   - crates/micode/src/model_browser.rs
127:   - crates/micode/tests/model_browser/**
128: forbidden:
129:   - provider/*
130:   - benchmark treatment files
131:   - integration branch
132: ```
133: 
134: A worker discovering it needs a file outside its scope reports `BLOCKED_WRITE_SCOPE` with the
135: requested path, rather than silently widening. That hands the parent a re-planning decision instead
136: of a surprise in the diff.
137: 
138: ## Staleness on return
139: 
140: Launch-time validity **does not survive parent integration**. An agent starting at commit A while
141: the parent lands three changes is stale at hour three even though its launch was legitimate. So the
142: result carries its own receipt:
143: 
144: ```
145: ResultReceipt { base_commit = A, current_integration_head = D }
146: ```
147: 
148: classified as one of: `directly_integratable`, `needs_rebase`, `conflicting`, `obsolete`.
149: 
150: A patch is **not** admitted merely because its tests passed in the worker's worktree. Same rule as
151: everywhere else in this project: evidence is about the state against which it was produced.
152: 
153: ## Role-specific preflights
154: 
155: Roles are different execution contracts, not one contract with different prompts:
156: 
157: | Role | Invariants |
158: |---|---|
159: | Implementation worker | repo + base + write-set + build isolation |
160: | Read-only critic | repo + base; **no** writes; no builds while an experiment is active |
161: | Experiment runner | frozen binary + treatment + task corpus |
162: | Documentation worker | docs write-set only |
163: | Verifier | verifier environment **outside** subject authority |
164: 
165: ## Build-loop integration
166: 
167: This is a new gate in the existing lifecycle (`BUILD_LOOP.md`):
168: 
169: ```
170: Planned → ContextProvisioned → ContextVerified → InProgress
171:         → Implemented → Verified → Admitted
172: ```
173: 
174: A worker never reaches `InProgress` without a context receipt, which lets the loop derive
175: runnability directly:
176: 
177: ```
178: task runnable  =  dependencies admitted  AND  execution context verified
179: ```
```

## M08 — micode/crates/micode-verify/src/graduation.rs

Specific trust-judge enforcement verdict remains NoGo; unrelated Fabric verification cannot graduate it.

Lines 61–91; SHA-256 `29ca1c93336b20a7f16d38b18f4760e8b0d5a397337e4aafc93ca5f430d44c2b`.

```text
61: //! [`TIER_B_MAX_FAIL_OPEN_RATE`]-bounded fail-open rate), closing TECH_SPEC.md §11 item 6's Tier B
62: //! latency-budget half mechanically. The statistical criteria (sample size, FPR, recall) are
63: //! computed over `CaseOrigin::Captured` cases ONLY — synthetic cases stay mechanism regression
64: //! checks, never statistical evidence — which is exactly why the published verdict remains **NO-GO**
65: //! (this sandbox has zero captured cases). The published artifact is regenerated from a REAL run by
66: //! the `#[ignore]`d `generate_graduation_decision` test at the composition root, and the committed
67: //! [`PUBLISHED_VERDICT`] is drift-pinned to the doc.
68: 
69: use std::time::Duration;
70: 
71: use micode_core::{AssistantTurn, ContentFailure, TurnContext};
72: 
73: use crate::judge::{GateMode, JudgeCall, JudgeGate, JudgeModel, JudgeOutcome, TieredTrustGate};
74: use crate::replay::{self, CaseOrigin, Expected, ReplayCase, ShadowModeReport};
75: use crate::HeuristicGate;
76: 
77: /// REMAINING_WORK_SPEC §1.2c/§1.2d: the committed, drift-pinned graduation verdict. This is the ONE
78: /// constant the composition root (`micode::SessionAssembly`) reads to decide whether
79: /// `GateMode::Enforcement` is even constructable: requesting enforcement while this is anything but
80: /// `Go` is a hard, fail-closed startup error. It is regenerated ONLY in the same commit that
81: /// regenerates `ENFORCEMENT_GRADUATION_DECISION.md` from a real `evaluate_with` run, and a drift-pin
82: /// test (`published_verdict_matches_the_committed_decision_doc`) asserts the two never disagree —
83: /// the same discipline `BENCHMARK_RESULTS.md`'s regression pins use. Enforcement is therefore
84: /// reachable only after a regenerated GO decision lands here, never by env var alone.
85: pub const PUBLISHED_VERDICT: GraduationVerdict = GraduationVerdict::NoGo;
86: 
87: /// PRD/04 NFR (Tier B latency budget), the denominator half of the
88: /// `tier_b_p95_latency_within_budget` criterion: "Tier B p95 added latency < 15% of mean turn
89: /// time." 0.15 is that 15%.
90: pub const TIER_B_P95_LATENCY_BUDGET_FRACTION: f64 = 0.15;
91: 
```

## M09 — micode/crates/micode-persist/src/strategy_memory.rs

Existing strategy memory is not to be overwritten by a duplicate generic history store; observed outcome and experiment evidence remain separate.

Lines 1–113; SHA-256 `378c7499227b3f1f25f68423653c9659bda694398bdb3bd917e18d34eabda1a4`.

```text
1: //! `MEMORY_SPEC.md` R2 — what approach worked, for what shape of task.
2: //!
3: //! # What this replaces, and why it is a new type rather than a migration
4: //!
5: //! `micode-heal`'s `Strategy { task_summary, pipeline_config, outcome_score }` recorded *which
6: //! self-heal pipeline configuration* ran. That loop was removed, so the middle field named a
7: //! concept that no longer exists. Wiring it as-is would have shipped a field whose name lies;
8: //! migrating the schema would have meant writing and maintaining a migration for data that, by
9: //! inspection, exists nowhere — the index never ran in a default session.
10: //!
11: //! Resolved 2026-08-03 (user decision): archive the old type (`archive/micode-heal-v1/`), keep the
12: //! *question*, and give the answer a shape that matches it.
13: //!
14: //! **A new table name (`task_strategies`, not `strategies`) is load-bearing.** A stale
15: //! `strategy.redb` from the old crate must be unable to deserialize as the new shape. Reusing the
16: //! name would let a two-field-mismatched record either fail confusingly or, worse, partially
17: //! decode.
18: //!
19: //! # What it is for
20: //!
21: //! "I have done something like this before — what did I do, and did it work?" That is a different
22: //! question from `RepoMemory`'s "what is true about this repository", and it is answered
23: //! differently: by shape-matching a new task against past ones, not by relevance-ranking facts.
24: //!
25: //! Retrieval is the same Jaccard word-overlap the old index used and `RepoMemory` still uses. It
26: //! is cheap, has no model call, and — importantly for something on a session path — cannot fail.
27: 
28: use std::path::Path;
29: 
30: use redb::ReadableTable;
31: use serde::{Deserialize, Serialize};
32: 
33: use crate::{Scored, SimilarityStore};
34: 
35: /// How a past attempt turned out.
36: ///
37: /// An enum, not the old `outcome_score: f64`. A score implies a calibrated measurement nobody
38: /// produces: there is no scale on which "0.7" is meaningful for "did this refactor work", and a
39: /// float invites averaging numbers that were guesses. Three states are honestly reportable, and
40: /// `Mixed` exists because most real attempts are neither.
41: #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
42: #[serde(rename_all = "snake_case")]
43: pub enum Outcome {
44:     Worked,
45:     Mixed,
46:     Failed,
47: }
48: 
49: impl Outcome {
50:     /// Ranking weight. `Failed` is deliberately **not** zero: "this approach was tried and did not
51:     /// work" is among the most useful things to recall, and weighting it out would recall only
52:     /// successes and quietly invite repeating known failures.
53:     fn weight(self) -> f64 {
54:         match self {
55:             Outcome::Worked => 1.0,
56:             Outcome::Mixed => 0.7,
57:             Outcome::Failed => 0.6,
58:         }
59:     }
60: }
61: 
62: /// One recorded attempt.
63: #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
64: pub struct Strategy {
65:     /// What kind of task this was, in the author's words. Matched against a new task's own
66:     /// description at recall time — the "match on the text itself" shape, not a precomputed hash,
67:     /// so a summary written today is comparable to one written months ago.
68:     pub task_shape: String,
69:     /// What was actually done. Free text on purpose: an approach is prose ("ported the trait into
70:     /// a port and wired it at the composition root"), and any enum here would be a taxonomy
71:     /// invented before there was evidence for one.
72:     pub approach: String,
73:     pub outcome: Outcome,
74:     /// Why it turned out that way, when that is worth more than the outcome. Optional because
75:     /// forcing a note produces empty ceremony.
76:     #[serde(default)]
77:     pub note: Option<String>,
78: }
79: 
80: /// New table name — see the module docs. A stale `strategies` table from `micode-heal` is invisible
81: /// to this store rather than half-readable by it.
82: const TASK_STRATEGIES: redb::TableDefinition<u64, Vec<u8>> =
83:     redb::TableDefinition::new("task_strategies");
84: 
85: pub struct StrategyMemory {
86:     db: redb::Database,
87: }
88: 
89: impl StrategyMemory {
90:     /// Open, degrading a corrupt file to an `Err` rather than a process abort.
91:     ///
92:     /// `catch_unwind` carried over from the archived version, which learned it the hard way:
93:     /// `redb::Database::create` panics on a corrupted file, and a memory store must never be able
94:     /// to take a session with it.
95:     pub fn open(path: &Path) -> anyhow::Result<Self> {
96:         let owned = path.to_path_buf();
97:         let db = std::panic::catch_unwind(move || redb::Database::create(&owned)).map_err(
98:             |panic| {
99:                 let message = panic
100:                     .downcast_ref::<&str>()
101:                     .map(|s| (*s).to_string())
102:                     .or_else(|| panic.downcast_ref::<String>().cloned())
103:                     .unwrap_or_else(|| "non-string panic payload".to_string());
104:                 anyhow::anyhow!(
105:                     "opening {} panicked (likely corrupted): {message}",
106:                     path.display()
107:                 )
108:             },
109:         )??;
110:         Ok(StrategyMemory { db })
111:     }
112: 
113:     /// Record an attempt.
```

## M10 — micode/Cargo.toml

MiCode Cargo workspace version is independent of the Axon build-pack version.

Lines 1–34; SHA-256 `32e3c1c5c01af4c5350651e2dc4d2dc5bf791f84c5c74c8619292532a575732a`.

```text
1: [workspace]
2: resolver = "2"
3: members = [
4:     "crates/micode-git",
5:     "crates/micode-mcp",
6:     "crates/micode-reliability",
7:     "crates/micode-providers",
8:     "crates/micode-core",
9:     "crates/micode-verify",
10:     "crates/micode-delegate",
11:     "crates/micode-extend",
12:     "crates/micode-persist",
13:     "crates/micode",
14:     "crates/micode-rpc-client",
15: ]
16: # REMAINING_WORK_SPEC §6.2: the cargo-fuzz crate is its own `[workspace]` root (it needs a nightly
17: # toolchain + sanitizer runtime `libfuzzer-sys` pulls in). Excluded so `cargo build/test/clippy
18: # --workspace` never tries to build it on stable.
19: exclude = ["crates/micode/fuzz"]
20: 
21: [workspace.package]
22: version = "0.2.0"
23: edition = "2021"
24: rust-version = "1.95.0"
25: repository = "https://github.com/Cklaus1/micode"
26: publish = false
27: # MIT (2026-09-09). Supersedes `LicenseRef-Proprietary`, which was chosen on 2026-08-03 explicitly
28: # because it was the REVERSIBLE option — "adding an open-source licence later is a one-line change,
29: # while withdrawing one already published is not". This is that change, and it spends that
30: # reversibility: every version released under MIT stays MIT for anyone who received it, regardless
31: # of what a later version says.
32: #
33: # The `LICENSE` file at the repo root is the matching MIT text. `publish = false` is unrelated and
34: # unchanged — it governs crates.io, not rights.
```

## F01 — fabric/specs/ACF-01-AXON-COMPUTE-FABRIC.md

Existing native execution and authority owners plus dependency direction.

Lines 36–72; SHA-256 `472993ba33fc64cf78552f9f51a01e7e4376f663775f2f9da11b67aee519792e`.

```text
36: ## 2. Goals, release boundary, and non-goals
37: 
38: The first protected vertical slice MUST execute one registered Cortex check in an isolated Linux workspace, under the existing authority boundary, with bounded resources, an independently attributable result, and no write to the operator's source workspace. A second request MUST NOT cross-contaminate its inputs, IDs, credentials, handles or budget.
39: 
40: The initial release contains contracts, a truthful backend registry, a native Linux execution route, immutable workspace materialization, reservations, cancellation/reconciliation, and Cortex evidence linkage. A legacy subprocess route remains available for its existing scoped/interpreter use and trusted fixtures; it is never silently selected to satisfy a hardware-isolation or arbitrary-native-code requirement.
41: 
42: Logical workspace branches follow that vertical slice. Full RAM checkpoints, memory forks, a generic bounded WASM host, one remote provider, desktop takeover, GPU placement and learned routing are separately gated extensions. They are not prerequisites for the first useful release.
43: 
44: Non-goals are a new language VM, a rewrite of `axon-core`, a replacement for Cortex's cognitive scheduler, a second identity/approval system, universal live-memory migration, automatic merge of machine state, automatic production deployment, eleven vendor integrations, and completing the custom guest kernel as a dependency of hosted execution.
45: 
46: ## 3. Ownership and integration
47: 
48: ### 3.1 Reuse existing owners
49: 
50: | Concern | Existing owner / source | Proposed integration |
51: |---|---|---|
52: | Language execution semantics | `axon-core` interpreter; `AGENTS.md` | Preserve interpreter-first behavior and parity gates. |
53: | Per-builtin host effects | `axon-core/src/host.rs::AxonHost` | Reuse for Axon host I/O, recording and virtual hosts; it is not the machine lifecycle API. |
54: | Grant algebra, approval and containment admission | `axon-os::{grant,approval,gate,supervisor}` | Keep as authority owner; introduce a shared compute-admission path without bypassing the existing one. |
55: | Legacy program execution | `axon-os/src/runtime.rs::Runtime` | Add an opt-in `FabricRuntime` bridge for compatible `.axjob` program runs. |
56: | Firecracker execution | `axon-vm/src/main.rs` | Extract a library with CLI compatibility, then add an explicitly profiled backend. |
57: | Workspace observations and semantic action authorization | `axon-cortex::{WorkspaceSnapshot,runner::Authorized}` | Preserve these types and digests; link durable workspace artifacts instead of renaming observations into disk snapshots. |
58: | Registered test execution | `Runner::run_tests_json` | Inject `CheckExecutor`, with a fabric-backed implementation and a fixture implementation. |
59: | Episode evidence | `axon-cortex::episode`, `axon-os::RunRecord` | Add versioned references to execution receipts; preserve existing encodings. |
60: | Cognitive selection | Cortex/CX-34 | Supply feasible execution profiles and measurements; do not select goals or grant authority. |
61: 
62: These ownership choices follow the source seams and the existing CX-03/CX-13 contracts. [E03–E10, E24–E36]
63: 
64: ### 3.2 Dependency direction
65: 
66: A proposed `axon-compute` library MAY hold pure protocol types, capability/profile matching, workspace manifests, durable operation state and the backend contract. It MUST NOT depend on `axon-cortex`, `axon-core` with native codegen, `axon-os`, or a provider SDK. A module-first implementation is acceptable if it preserves this dependency direction; the source's CX-04 guidance explicitly warns against premature crate splits. [E35]
67: 
68: `axon-os` owns the admitted service and optional `FabricRuntime`; it consumes these contracts. An extracted `axon-vm` library implements the native microVM backend and remains dependent on `axon-attest` as appropriate. A small integration module in a trusted host binary wires authority to drivers. Driver SDKs MUST be feature-gated and absent from core interpreter/browser builds. Avoid a cycle in which `axon-os` depends on a fabric that depends back on `axon-os`.
69: 
70: `axon-cortex` gets a narrow `CheckExecutor` seam, not direct vendor credentials or API dependencies. `cortex-policy-adapter` remains a typed boundary and cannot turn possession of a JSON compute request into permission to execute it.
71: 
72: The existing `axon-vm` CLI may remain directly usable by an authorized host operator. Inside a worker it MUST be inaccessible as a bypass to the fabric. Feature disabling changes routes, not the protection an admitted workload requires.
```

## F02 — fabric/specs/ACF-01-AXON-COMPUTE-FABRIC.md

Identity separation, new sidecars, grant intersection and strict request validation.

Lines 118–164; SHA-256 `472993ba33fc64cf78552f9f51a01e7e4376f663775f2f9da11b67aee519792e`.

```text
118: ## 5. Canonical resource model
119: 
120: ### 5.1 Identity is not content identity
121: 
122: Use separate identities for `Task`, `ExperimentArm`, `Trial`, `Attempt`, `Operation`, `Execution`, `Branch`, `WorkspaceVersion`, `Checkpoint` and `Artifact`. A retry of a transport operation reuses its `OperationId`; an authorized new execution attempt gets a new `AttemptId`. Repeated trials of the same task and arm have distinct `TrialId`s.
123: 
124: A task-definition digest and deterministic seed describe reproducibility; they do not identify a unique execution. This is important because the present Cortex episode name is derived from target path, symbol and hidden check and repeats for repeated runs of that same request. Keep that useful semantic identifier and add execution IDs around it. [E30]
125: 
126: A runtime handle binds tenant, principal scope, execution ID, backend profile, provider resource, lease ID and fencing epoch. It is an opaque service reference. Knowing a handle string does not authorize attach, read, resume or destroy. A stale handle cannot operate on a recycled resource.
127: 
128: ### 5.2 Resource definitions
129: 
130: | Resource | Required meaning |
131: |---|---|
132: | `EnvironmentSpec` | Content-bound executable/interpreter, dependency closure, base image, guest kind, OS/architecture, approved configuration and startup recipe. |
133: | `WorkspaceVersion` | Immutable retrievable contents and metadata, parent references, tenant/label, exact scope and completeness declaration. |
134: | `WorkspaceLease` | A bounded mutable worktree derived from a version, with one writer and fenced promotion rights. |
135: | `SemanticStateRef` | Reference to the owning agent/Cortex store; the fabric does not invent or overwrite agent memory. |
136: | `ExecutionCheckpoint` | Typed reference to captured local execution state, capture scope, consistency level, compatibility envelope, provenance and retention. |
137: | `ComputeJob` | A validated request binding all authority, input, executable, resource and output requirements for one execution attempt. |
138: | `RegisteredExecutable` | Trusted mapping from check/tool ID to executable digest, argv schema, environment projection, effect ceiling, output contract and required enclosure. |
139: | `ExecutionReceipt` | Observed lifecycle outcome, exit data, verifier status, usage and evidence origins, without conflating them. |
140: | `ComputeEvent` | Ordered, versioned evidence with task/trial/attempt/operation and causal references. |
141: 
142: `WorkspaceSnapshot` in Cortex remains a scoped observation of hashes. The new `WorkspaceVersion` is not a retroactive reinterpretation of it. An explicit projection links the observation digest to the durable object and lists omissions. [E27–E28]
143: 
144: ## 6. Admission and API contract
145: 
146: ### 6.1 Preserve existing paths
147: 
148: The current supervisor performs approval, declared-effect admission, grant intersection, mint, execute and record. Keep that convergence point; a new command, replay path, GUI attach or remote driver cannot skip equivalent checks. [E04]
149: 
150: The existing `Runtime` signature is program-oriented and synchronous. `FabricRuntime` MUST implement only the `.axjob` operations that retain those semantics. It MUST NOT interpret a fake `.ax` filename as an arbitrary shell request or convert unknown native effects into an empty effect set. The current flat manifest parser is not extended silently. [E03, E08]
151: 
152: Introduce an explicit, versioned `ComputeJob` request for registered checks and later native tools. The trusted registry supplies the executable, approved argv shape and declared maximum effects. The caller supplies only validated arguments and references. Arbitrary generated native content remains untrusted even under a registered build command. Authorization covers the dependency closure and the actual executable artifact, not a display name or entry file alone.
153: 
154: Extract shared admission logic where required rather than maintain two independently evolving policy implementations. Existing `Grant::intersect` remains the basis for filesystem, network and executable ceilings; new rights such as checkpoint, export and attach live in a supervisor-owned registry extension. They MUST be included in the new authority digest and revocation logic.
155: 
156: ### 6.2 Request validation
157: 
158: A request binds principal/session, task/trial/attempt IDs, input version, executable and dependency digests, registry entry version, working directory, permitted mounts, environment projection, network policy, result schema, reservation, deadline, cancellation scope and idempotency semantics. A reference is resolved from trusted state before use.
159: 
160: The effective authorization is the intersection of job request, supervisor authority, inherited scoped authority, tenant policy and current resource/export restrictions. Backend capabilities only narrow eligible implementation; they never expand authority.
161: 
162: Admission MUST fail closed on unknown fields in a closed schema, duplicate JSON keys including escaped aliases, malformed digests, absent required facts, expired approval, revoked epoch, unsupported policy translation, missing executable binding, and absent resource reservations. Closed enums never default to the closest known value. [E26, E33–E34]
163: 
164: ### 6.3 Host-side API shape
```

## F03 — fabric/specs/ACF-01-AXON-COMPUTE-FABRIC.md

Durable journal, failure certainty and separate cleanup/billing state.

Lines 189–229; SHA-256 `472993ba33fc64cf78552f9f51a01e7e4376f663775f2f9da11b67aee519792e`.

```text
189: ## 7. Durable lifecycle, cleanup, cancellation and retry
190: 
191: ### 7.1 Two distinct state machines
192: 
193: Keep action state separate from machine state. The CX-03 action sequence remains:
194: 
195: `Proposed → Prepared → Validated → Running → Observed → Verified`.
196: 
197: Its failure alternatives remain Refused, Failed, Canceled and OutcomeUnknown. `Verified` is a verifier conclusion, not a process exit. [E34]
198: 
199: A machine resource uses:
200: 
201: `Requested → Admitted → Provisioning → Ready → Running → Stopping → Stopped → Destroying → Destroyed`.
202: 
203: Negotiated branches include `Running → Quiescing → Checkpointing → Running|Suspended`, `Suspended → Restoring → Ready`, and `Unknown → Reconciling → observed state`. A failed checkpoint must report whether the original is still usable. `Failed` does not imply resource deletion.
204: 
205: ### 7.2 Journal before effects
206: 
207: Before provisioning, executing, forking or promotion, atomically persist the operation identity, authorized input/configuration digest, resource reservation, expected prior version and intended transition. Persist observations and provider receipts afterward. Crash recovery reconciles outstanding operations; it does not invoke `axon-os replay` as an effect-recovery mechanism. That existing command intentionally re-executes a job. [E10, E33–E34]
208: 
209: For the first single-host implementation, a durable transactional journal with uniqueness constraints, compare-and-swap state updates and an outbox is sufficient. The chosen store and its crash semantics must be documented and fault-tested. An in-memory map or a stream of stdout lines is not the journal. Existing audit records provide integrity evidence; they do not by themselves prove atomic reservation, locking, persistence or issuer authentication.
210: 
211: A duplicate request with the same operation ID and same immutable inputs returns its recorded operation. The same ID with different inputs is rejected. A new trial is not deduplicated just because its program and seed match an older trial.
212: 
213: ### 7.3 Failure certainty
214: 
215: Each effect adapter declares whether it supports server-side deduplication, safely repeatable reads, or reconciliation-required execution. A timeout after a possible effect produces `OutcomeUnknown`. The service MUST NOT promise exactly-once external side effects merely because it stores an idempotency key.
216: 
217: Automatic provider fallback is allowed before a consequential effect begins. After a possible effect, reconcile or require an approved new action. Billing uncertainty remains reserved liability until settled or explicitly written off by trusted policy.
218: 
219: ### 7.4 Resource ownership
220: 
221: A launch guard owns every child process, cgroup, socket, mount, temporary file, network namespace, relay task and provider handle from first creation. All failure paths clean up, and cleanup failure remains a visible reconciliation obligation. Never rely solely on the success-path code at the end of a function. The existing launcher has fallible operations after spawn and cleanup at its normal end; this must be hardened before pooling. [E14–E17]
222: 
223: Replace per-process temporary names with per-attempt private directories, restrictive permissions and safe atomic file creation. Separate operation identity from process ID. Concurrent executions in one supervisor must not share the wrapper path or vsock/API socket namespace. [E05, E15]
224: 
225: Cancellation first latches against new dispatch and revokes relevant credentials/leases, then terminates execution and descendants, then verifies stop and reconciles usage. A process-group signal alone cannot be the claimed confinement guarantee for arbitrary descendants. The supported Linux profile must demonstrate termination of deliberately detached descendants and resource release. Remote stop uncertainty is reported, not hidden.
226: 
227: A suspended or restored worker cannot clear the supervisor's cancellation latch. Out-of-band authority and fencing epochs remain current even when guest memory is old.
228: 
229: ## 8. Workspace and state architecture
```

## F04 — fabric/specs/ACF-01-AXON-COMPUTE-FABRIC.md

Durable workspace and fenced CAS publication.

Lines 231–257; SHA-256 `472993ba33fc64cf78552f9f51a01e7e4376f663775f2f9da11b67aee519792e`.

```text
231: ### 8.1 Four layers, with bounded portability
232: 
233: | Layer | Owner | Contract |
234: |---|---|---|
235: | S0 Environment | Axon artifact/registry services | Reconstructible definition for explicitly supported OS, architecture, dependency and license constraints. |
236: | S1 Workspace | Axon workspace store | Durable contents, history, labels and approved exports. |
237: | S2 Execution state | Backend-specific capture | Optional accelerator with an exact compatibility and durability envelope. |
238: | S3 Semantic state | Cortex/agent memory owner | Versioned reference in execution lineage, not a copy of all memory into the VM. |
239: 
240: Logical migration is supported **only when** the destination can satisfy the environment, authorization and export requirements. It is not guaranteed across incompatible operating systems, architectures, missing dependencies, unsupported secret policies or unexportable vendor storage. A RAM capture is never the only copy of an accepted artifact.
241: 
242: ### 8.2 Durable workspace content
243: 
244: Store actual immutable bytes, not only file hashes. The manifest includes normalized relative path, entry type, content digest, size, required mode/executable bit, permitted link metadata, parent relation, label and capture scope. Empty directories, missing paths, file deletion and unavailable observation are distinct states.
245: 
246: Capture includes explicitly selected dirty and untracked inputs; it does not assume Git tracked files equal the environment. The first release may deny symlinks, hard links, devices and unusual metadata rather than implement them unsafely. Denial is explicit. Later support must defend resolution/replacement races at open time, not just normalize a string once. Protected verifier files, credentials and host files are never part of a writable worker projection.
247: 
248: Materialization must verify each object, enforce size/count/path-depth bounds, create a private worktree and make required protected inputs read-only. Archive import is untrusted: reject absolute paths, traversal, link escapes, device entries and decompression/resource amplification. No provider-local path is accepted as a portable artifact reference without import and verification.
249: 
250: The existing `Runner::stage_copy` copies only immediate regular files. Keep it a fixture helper until a separate recursively safe materializer is tested; do not advertise it as workspace backup. [E32]
251: 
252: ### 8.3 Promotion
253: 
254: Workers produce immutable candidate versions. An independent verifier runs against the precise candidate plus a protected verifier/environment digest. Promotion checks that the base reference and authority epoch still match, then atomically advances the approved workspace reference with a compare-and-swap. A stale base produces a conflict and explicit rebase/reverification, not an overwrite.
255: 
256: First release: one writer per branch and promotion authority separate from execution authority. Parallel branches have independent writable trees. Shared mutable volumes are not permitted as a shortcut. No automatic RAM merge exists. Branch selection does not replay writes to external services or deploy a candidate.
257: 
```

## F05 — fabric/specs/ACF-01-AXON-COMPUTE-FABRIC.md

First protected Linux path and separately qualified egress/secret/WASM work.

Lines 290–325; SHA-256 `472993ba33fc64cf78552f9f51a01e7e4376f663775f2f9da11b67aee519792e`.

```text
290: ## 10. Native backend requirements
291: 
292: ### 10.1 Legacy interpreter adapter
293: 
294: Preserve reference interpreter execution and existing fail-closed verdicts, scoped wrappers, virtual-clock behavior and approval tests. Fix concurrency-sensitive temporary resources and bounded output as part of reuse. The adapter advertises only its observed scoped `.ax` behavior. It cannot satisfy arbitrary native confinement, hardware isolation, memory capture, desktop or remote guarantees. [E03–E06]
295: 
296: `PrincipalHandle(0)` in the existing implementation is an implementation-local placeholder, not a global principal identity. A service adapter must retain principal-to-grant binding in supervisor state rather than expose that value as authority across requests. [E05]
297: 
298: ### 10.2 Firecracker/Linux profile
299: 
300: Extract `axon-vm` into reusable launch/configuration/result modules without changing CLI meanings, exit codes, attestation/quorum controls, existing schemas or source-era tests. New daemon/service code must return values rather than call `process::exit` inside library paths. [E11, E18]
301: 
302: The first protected microVM profile MUST explicitly identify a supported **Linux guest image and init protocol**. Existing launcher policy delivery favors kernel command-line data without a NIC; Linux guest init reads MMDS. Reconcile that protocol mismatch deliberately and test the actual image pair. Do not weaken fail-closed guest policy handling to make it boot. [E15, E19–E21]
303: 
304: Required gates include real registered workload execution, loaded executable/content binding, secure VMM launch ownership, restricted API sockets, host cgroup/UID boundaries, disk/workspace projections, guest policy enforcement, independent deadline/descendant control, output limits, default-deny egress, result provenance and failure cleanup.
305: 
306: The current payload contains effect names and token caps rather than the complete OS path/host grant. Translation must either enforce every requested scope through a tested mechanism or refuse the profile. Do not downgrade path-specific authority into `FS=true` or host-specific authority into `Net=true`. [E07, E13]
307: 
308: The current source explicitly substitutes balloon settings for production jailer-style controls. That is not evidence of full host resource/UID isolation. Attestation of a kernel does not prove the right job ran. Guest success must be coupled to a workload receipt and independent check result; the kernel demonstration's clean halt cannot pass a registered workload gate. [E15, E18, E21]
309: 
310: ### 10.3 WASM host extension
311: 
312: Keep the browser `axon-wasm` ABI intact. An optional hosted WASM profile needs an explicit pinned host engine, bounded instance memory/fuel or interruption, host-call allowlist, deterministic-enough clocks/randomness where required, output limits and a fresh instance boundary per protected run. It must exercise the real `.ax` interpreter/WASM artifact, not only an empty test module.
313: 
314: WASM code confinement does not constrain a permissive host import. Untrusted guest code must never share the supervisor's authority or arbitrary filesystem/network host implementation. Cancellation and host-call deadlines must work even when a module loops or blocks in an import. General host snapshots require their own evidence and cannot be inferred from Axon's cooperative fiber suspend/resume.
315: 
316: ## 11. Egress, secrets, imports and trust
317: 
318: Initial protected work uses offline fixtures and no egress. Later egress requires a trusted broker outside the guest plus enforcement that prevents bypass. Destination, method/path, scope, redirect and credential policy are checked at the actual boundary; raw IPs, DNS changes, proxies, alternate protocols, metadata endpoints and existing connections are included in tests.
319: 
320: Credential injection is permitted only for explicitly integrated protocols/services where destination and request semantics can be authenticated. A generic TLS byte tunnel cannot magically inject HTTP credentials. Other cases use short-lived scoped credentials with a documented exposure profile, or refuse. Never claim that brokered credentials eliminate all sandbox secrets or external effects.
321: 
322: The broker binds each request to principal, attempt, current epoch, grant and remaining reservation. It keeps credentials and audit authority outside worker-accessible memory and disks. Provider API credentials remain in the trusted driver service, not worker images. Artifact upload, package download, preview URL creation and desktop streaming are all separately scoped export/access operations.
323: 
324: Protected channels, caches, snapshots and storage are tenant-isolated. Backend logs and guest output are untrusted and may contain secrets or forged result-looking data. Redact without destroying authoritative accounting; record intentional omissions. Telemetry must not become an unbounded data-exfiltration sink.
325: 
```

## F06 — fabric/specs/ACF-01-AXON-COMPUTE-FABRIC.md

Whole-task costs and use of existing Cortex evidence/authority.

Lines 326–355; SHA-256 `472993ba33fc64cf78552f9f51a01e7e4376f663775f2f9da11b67aee519792e`.

```text
326: ## 12. Budgets and measurement
327: 
328: Reuse existing calls/tokens/cost semantics; add a separate versioned compute resource contract for CPU quota, memory, storage, output, wall-time, GPU resources, concurrent jobs and provider costs. The current `Budget` has only calls, tokens and `cost_micro`; it cannot represent all these constraints as-is. [E07]
329: 
330: A trusted reservation ledger enforces, on each relevant axis:
331: 
332: `settled_consumption + active_reservations + unresolved_liability <= authorized_limit`.
333: 
334: Execution changes a reservation into measured consumption plus remaining reserved capacity; it never double-counts the same charge. Child allocations carve the parent budget atomically; ten forks do not each inherit the full parent's remaining funds. Snapshot/restore cannot rewind the ledger.
335: 
336: Use explicit units, checked integer arithmetic and currency/price-version bindings. Admission prices may be estimates; final usage distinguishes `estimated`, `metered`, `provider_reported`, `settled` and `unknown`. Storage, idle resources, failed boots, retries, losing branches, egress and cleanup are counted. Unknown invoice exposure cannot be marked free.
337: 
338: Hard local resource controls and dispatch ceilings bound known execution; uncertain external billing cannot be represented as an exact hard monetary guarantee without a supporting provider contract. Requests missing a required bound are denied.
339: 
340: Optimize completed-task economics, not a single boot time. Measure cold create, warm attach, resume, checkpoint, fork, ready-to-use, end-to-end completion, verified quality, outcome uncertainty, cancellation lag and total cost. A cheap provider with more retries may cost more per accepted task. Learned ranking is admitted only after hard filtering and a fixed baseline evaluation.
341: 
342: ## 13. Cortex, Axon evidence and ASI OS integration
343: 
344: Introduce `CheckExecutor` into the runner so a registered check becomes a typed compute job rather than a direct `Command::output`. Preserve the `Authorized` action boundary and grader-not-patchable rules. The worker is not the verifier and stdout is not the supervisor's authoritative result channel. [E29, E31]
345: 
346: A trusted, framed result channel identifies the attempt and executable, records exit/timeout/transport outcome, and validates the expected test inventory/schema. An exit code of zero, missing test summary, zero matching checks, duplicate verdict, late forged summary or worker-provided “passed” cannot produce independent verification.
347: 
348: Append execution evidence keyed by `TaskId / ArmId / TrialId / AttemptId / OperationId`, with links to the existing episode, workspace observation, effective authority digest, environment, backend profile, immutable output and verifier receipt. This is one provenance system with typed nodes and relations, not a collection of disconnected graphs. Graph lineage is evidence, not permission.
349: 
350: Do not rewrite historical `axc1:` hashes, `axon-os-record/1`, `axon-vm-run/2` or episode JSON in place. Use an `acf-execution/1` sidecar envelope referencing existing digests. Any stronger wire format has a new schema/version and explicit reader migration. Old missing fields remain unknown, never retroactively verified. [E09, E18, E26–E27]
351: 
352: The ASI OS `Computer` presentation maps to an execution/workspace/lease. A stateless pure WASM job need not pretend to have a terminal or desktop. Later human attach grants are read-only or read/write separately, short-lived and exclusive where needed. Taking control fences agent input; releasing control rechecks policy and records provenance.
353: 
354: The fabric may publish measured runtime capabilities to CX-34. The cognitive scheduler keeps ownership of goals, actions, model choices and experiments. The compute broker performs constrained placement and lifecycle management. Neither learns away a safety requirement. [E36]
355: 
```

## P01 — v021/build/RESEARCH_INTEGRATION_V021.md

Plane aliases use existing owners; paper reproduction and mandatory live learned providers are separate qualifications.

Lines 1–54; SHA-256 `137f024fd041409c70869710913897c8108343f46b002b1302a6523578faeda9`.

```text
1: # v0.21 research integration contract
2: 
3: **Normative scope:** additive profiles for existing CX owners. This document supersedes earlier conversational proposals for seven independent planes. It does not replace source authority, declare a new runtime ABI or assert implementation.
4: 
5: ## Ownership and version rules
6: 
7: EVO/DEC/EVL/RTR/CVM/SPX/TEL are stable navigation aliases. The authoritative mapping is [SOURCE_OWNER_MAP](../integration/SOURCE_OWNER_MAP.json). CX-11 retains independent admission, CX-03 and source authorization retain effect permission, CX-10 retains canonical evidence, CX-28 retains working-set projections, CX-34 retains capability registry/scheduling, and CX-37 retains ACE provider/runtime ownership. CX-36 bytes and neural wire formats are unchanged.
8: 
9: The live source's CX-35 Reflex work maps to CX-05 and related existing owners without renaming the source ID or consuming the package's reserved ID. New work uses B223–B254 and explicitly prefixed `Gxx-r21-*` gates. Old requirements, source mappings and MiCode v0.14 origin references remain historical contracts, not proof of current peer delivery.
10: 
11: No new `.reflex`, `.clm`, `.dec` or alternate neural container is introduced. Model/head revisions belong in the existing artifact lifecycle. A provisional research record schema is a package reference/negotiation payload, not an automatically accepted `axon-reflex/1` request or new top-level ACE wire tag.
12: 
13: ## Shared flow
14: 
15: ```text
16: canonical observations + current grants + registry epoch
17:   -> authorized candidate view (CX-03/CX-34)
18:   -> bounded state projection (CX-28)
19:   -> eligible deterministic / CLM / Jev decision provider (CX-05)
20:   -> qualified calibration and routing policy (CX-06)
21:   -> optional isolated draft/verify proposal (CX-26)
22:   -> existing permission and transactional effect barrier (CX-03)
23:   -> actual final checks and evaluation evidence (CX-01/CX-21)
24:   -> existing replay, audit, usage and learning records (CX-10)
25:   -> independent strategy/head admission, never self-activation (CX-11)
26: ```
27: 
28: Eligibility is checked before sending private context or candidates to a provider. Ranking an action is not authorization to execute it. Evaluation may consume DEC judgments, but deterministic completion and independent admission are not delegated to DEC. EVO can propose policies for these components but cannot change the frozen evaluator, safety floor or approval authority during an experiment.
29: 
30: ## Seven profile guides
31: 
32: The [EVO guide](EVO_REGULARIZED_EVOLUTION.md), [DEC guide](DEC_RESEARCH_PROVIDERS.md), [EVL guide](EVL_CHECKLIST_EVIDENCE.md), [RTR guide](RTR_MODEL_ROLE_ROUTING.md), [CVM guide](CVM_CANONICAL_PROJECTIONS.md), [SPX guide](SPX_BOUNDED_CASCADES.md), and [TEL guide](TEL_WHOLE_TASK_ECONOMICS.md) define implementation invariants. Every guide is an Axon adaptation, not a claim that its research source already implements these protections.
33: 
34: ## Common record and lifecycle
35: 
36: Use collision-free `task_id`, `episode_id`, `attempt_id`, `candidate_id` and `call_id`, plus principal/tenant/workspace scope. Repeated trials on the same task and arm must not share identity. Bind records to the source snapshot, candidate manifest, effective input, model/provider runtime, tokenizer, pooling, head, wrapper/threshold policy and context projection. Information omitted for privacy must be a permission-qualified reference with explicit availability status, not fabricated content.
37: 
38: A decision result distinguishes raw scores, candidate-conditional probabilities, separately calibrated correctness and a reasoned disposition. `ABSTAIN`, `REFUSED`, `TRANSPORT_ERROR`, `CANCELLED`, `UNSUPPORTED` and a legitimate negative judgment are not interchangeable. Missing evidence/usage is unknown, never automatically zero or false. Existing refusal semantics remain intact.
39: 
40: Record exact nondeterministic results for replay. Replaying must not query live models, recompute a new context policy or reuse a different cache epoch. An episode digest binds content but is not, alone, externally anchored append-only integrity. Integrate the existing audit/recording mechanisms rather than advertising a second hash-based proof layer.
41: 
42: ## Research versus release requirements
43: 
44: A P0 research priority means a required contract, migration seam and falsification plan. It does not mean all providers must be installed, all papers reproduced, or all models perform better than the incumbent. The initial release accepts a deterministic provider and explicit unsupported outcomes where learned adapters are not qualified.
45: 
46: The package conformance closure is deliberately small. Runtime build tasks remain explicit and model-free fixture success cannot satisfy them. Real pilots need a reviewed source revision, authenticated endpoints, frozen evaluation policy, budget, independent final outcome and rollback. A rejected/inconclusive research arm is a valid result; forcing a named method into production is not an acceptance criterion.
47: 
48: Optional scopes B251–B254 are outer-loop meta-policy learning, ANN-scale candidate selection, specialization and best-of-N. Native latent cache handoff, new inference engines, unrelated sandboxes, new ABI work and multimodal CLM support are outside this delta.
49: 
50: ## Compatibility posture
51: 
52: Keep the existing deterministic Cortex policy adapter and grant boundary. Extend a negotiated Reflex capability rather than stuffing new fields into its strict existing wire. A legacy `/1` request retains existing meaning; unknown versions/profiles refuse. Provider output is bounded and schema-validated before entering the host; score fields cannot modify the grant snapshot. Permission denial must not trigger a retry as another principal.
53: 
54: Use existing feature flags/admission records. Start new research profiles disabled, then offline, then shadow, then one explicitly authorized low-risk scope. The rollback target is the prior source/registry/model/config revision, not merely a different model name. Sidecar/model processes are opt-in and confined; no installer edits shell startup files, starts background daemons or downloads weights during package validation.
```

## P02 — v021/integration/MICODE_V015_PROPOSED_DELTA.md

The historical v0.15 proposal was not delivered/tested MiCode peer work.

Lines 1–19; SHA-256 `8a27f578f361807acefa8b1fffd493b713ac32148274908c3d1143c9a7ab6d29`.

```text
1: # MiCode support 0.15 — proposed consumer delta, not delivered peer files
2: 
3: **Evidence:** only the Axon source snapshot and Axon v0.20 build pack were supplied. The previously paired v0.14 requirements are retained as historical owner contracts. This addendum does not modify, package, pin or test MiCode source or its support archive.
4: 
5: ## Required consumer mapping
6: 
7: MiCode should consume existing Axon owner exports with a reviewed v0.21 lock after its actual files are available. Map candidate/proposal IDs, principal scope, negotiated decision profile, model/head/wrapper/calibration identity, effective context projection, usage and refusal/cancellation through the existing experience bridge. Do not add a second MiCode authority for Axon admission or allow an Axon ranking to bypass MiCode PermissionGate.
8: 
9: Use CVM as the context execution policy while Axon owns learned policy/evidence. Preserve original transcripts/artifacts under approved retention and rehydrate only authorized data. SPX branches remain isolated proposals until MiCode's effect and independent-completion checks allow the exact selected candidate. A generated patch is not a committed patch; a model choice is not permission to transmit private source.
10: 
11: ## Compatibility acceptance
12: 
13: Test existing v0.14-compatible paths, new profile negotiation, unsupported profile refusal, incomplete/transport results, candidate-set mismatch, expired calibration, cancellation, next-turn recovery, unknown effect reconciliation, total budget and principal/tenant isolation. Model and wrapper revisions survive round trips without conversion of conditional score to correctness confidence.
14: 
15: Pin actual Axon/provider/MiCode revisions and exported bytes before claiming B247 peer completion. A schema fixture, simulated consumer or copied export file cannot satisfy the real paired gate. Never invent task IDs in the unavailable consumer manifest; attach real IDs after inspecting that pack.
16: 
17: ## Operator handoff
18: 
19: When the consumer source is reviewed, create a separate consumer upgrade plan against its actual evidence. Preserve existing local implementation and task state just as for Axon. Until then, its status is `PROPOSED_NOT_SUPPLIED`, not “MiCode 0.15 complete.”
```
