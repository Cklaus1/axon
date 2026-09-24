# Source evidence excerpts

These excerpts are copied from the uploaded archive. Line numbers refer to the original files, not this report. Statements about execution are not inferred from comments or from test presence. File hashes are in `manifests/source_manifest.json`.

## E01 — Bundle provenance; reported commit, exclusions.

`AI_CONTEXT_README.md:1-14`

```text

    1  # Axon — AI context bundle
    2  
    3  Snapshot of github.com/Cklaus1/axon at commit 4cceb89 (2026-09-24).
    4  
    5  ## Suggested reading order
    6  1. README.md, CLAUDE.md, AGENTS.md — overview + working conventions
    7  2. ARCHITECTURE.md, AXON_REFERENCE.md, AXON_VM.md, AXON_KERNEL.md — architecture
    8  3. spec/ (grammar.ebnf, language-tour.md, stdlib.md, runtime.md) — language spec
    9  4. ROADMAP.md, STATUS.md, IMPLEMENTATION_PLAN.md, tasks/*.md — current state + planned work
   10  5. crates/ — Rust source (compiler/interpreter in crates/axon-core/src: parser.rs, checker.rs, interp*, codegen/)
   11  6. examples/ — Axon (.ax) programs
   12  7. governance/specs/ — per-feature design specs (R-series)
   13  
   14  ## Excluded (noise for feature design)

```

## E02 — Interpreter-first reference and feature constraint.

`AGENTS.md:18-39`

```text

   18  
   19  ## Running code
   20  
   21  Execution is **interpreter-first** — do not reach for `axon build` (native LLVM) during
   22  normal development.
   23  
   24  ```bash
   25  cargo build -p axon-core --no-default-features --bin axon   # build the interpreter CLI (sub-second)
   26  axon run file.ax      # type-check + interpret
   27  axon check file.ax    # type-check only
   28  axon test file.ax     # run @[test] functions
   29  ```
   30  
   31  The tree-walking interpreter (`crates/axon-core/src/interp.rs`) is the **reference
   32  execution semantics**. Any new language feature must be implemented there; native codegen
   33  follows the interpreter, never the reverse.
   34  
   35  ## Working on the compiler
   36  
   37  See [`CLAUDE.md`](CLAUDE.md) for the compiler-project conventions (pipeline, crate layout,
   38  how to add a builtin, phase status) and [`ROADMAP.md`](ROADMAP.md) for forward planning.
   39  

```

## E03 — Existing Runtime seam; scoped Grant not just EffectSet.

`crates/axon-os/src/runtime.rs:15-54`

```text

   15  /// An opaque handle to a minted Principal in the runtime's registry.
   16  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   17  pub struct PrincipalHandle(pub usize);
   18  
   19  /// The result of running a program inside the sandbox: the observed
   20  /// capability-bearing actions and the sealing verdict.
   21  #[derive(Debug, Clone, PartialEq, Eq)]
   22  pub struct RunOutcome {
   23      pub events: Vec<RawEvent>,
   24      pub verdict: Verdict,
   25  }
   26  
   27  /// The seam. Every method that touches the model/interpreter/OS lives here.
   28  pub trait Runtime {
   29      /// The effect row a program declares it may perform. An error / absent
   30      /// declaration MUST map to `DeclaredEffects::unknown()` (deny-by-default).
   31      fn declared_effects(&self, program: &Path) -> DeclaredEffects;
   32  
   33      /// Mint a Principal holding exactly `grant`. Attenuation (no authority the
   34      /// supervisor lacks) is guaranteed by the caller passing the effective
   35      /// grant `J ∩ S`; the runtime mints to that, no more.
   36      fn mint_principal(&self, grant: &Grant) -> PrincipalHandle;
   37  
   38      /// Run `program` as `principal` inside a sandbox enforcing `ceiling` +
   39      /// `budget` with a fixed `seed`. Returns the observed events and the verdict
   40      /// (mapping any runtime over-reach to Denied/BudgetExhausted/RefineViolation).
   41      /// AUDIT T3: takes the full `Grant`, not just its induced `EffectSet`.
   42      /// `effect_set()` reduces the grant to four booleans, discarding the path
   43      /// prefixes and host allowlists — so the ceiling could only ever express
   44      /// "may write SOMEWHERE", never "may write ./out/". The scoped runtime
   45      /// needs the allowlists themselves.
   46      fn run_sandboxed(
   47          &self,
   48          program: &Path,
   49          principal: &PrincipalHandle,
   50          grant: &Grant,
   51          budget: &Budget,
   52          seed: u64,
   53      ) -> RunOutcome;
   54  }

```

## E04 — Approval, grant intersection, admission, mint, run, record.

`crates/axon-os/src/supervisor.rs:18-105`

```text

   18  pub fn run(
   19      manifest: &JobManifest,
   20      job_path: &std::path::Path,
   21      supervisor_grant: &crate::grant::Grant,
   22      run_id: &str,
   23      rt: &impl Runtime,
   24  ) -> RunRecord {
   25      // 0. AUTHORIZATION — before anything else, at the point every execution
   26      //    path converges on.
   27      //
   28      //    This check lived in `cmd_run`, so `axon-os replay` reached execution
   29      //    by a different route and never performed it. REPRODUCED: a stored job
   30      //    marked `require_approval = true` with no token anywhere was refused by
   31      //    `run` with exit 8 and no side effect, then re-executed by `replay`
   32      //    with exit 0 and the side-effect file's mtime advancing. The public
   33      //    `axon_os::supervise` re-export was a third route with no gate at all.
   34      //
   35      //    Step 3 below already demonstrates the pattern: `admit` sits here and
   36      //    every caller gets it whether or not they remember. A check in a CALLER
   37      //    is opt-in per call site.
   38      let approval = match crate::approval::authorize(job_path, manifest) {
   39          Ok(a) => a,
   40          Err(reason) => {
   41              let denial = RawEvent::new("denied", "approval", EffectSet::default(), "");
   42              let mut rec = build(
   43                  run_id,
   44                  manifest,
   45                  manifest.seed,
   46                  std::slice::from_ref(&denial),
   47                  Verdict::Denied {
   48                      reason,
   49                      axis: "approval".to_string(),
   50                  },
   51              );
   52              rec.approval = crate::approval::ApprovalStatus::Denied.as_str().to_string();
   53              return rec;
   54          }
   55      };
   56      // 1. What does the program declare it may do? (deny-by-default via the runtime)
   57      let declared = rt.declared_effects(&manifest.program);
   58  
   59      // 2. The effective grant: you cannot delegate authority you lack.
   60      let eff = manifest.grant.intersect(supervisor_grant);
   61  
   62      // 3. Static admission — fail closed BEFORE any execution.
   63      // The authorization decision travels with the record, so a reader can tell
   64      // afterward whether the run was authorized. Stamped on EVERY exit path —
   65      // the first version computed it and used it on none, and the compiler said
   66      // so in a warning I walked past. A record field that is always `unknown` is
   67      // exactly the absent-vs-verified collapse this field exists to close.
   68      let approval_str = approval.as_str().to_string();
   69  
   70      if let Admission::Deny { reason, axis } = admit(&declared, &eff) {
   71          let denial = RawEvent::new("denied", &axis, EffectSet::default(), "");
   72          let mut rec = build(
   73              run_id,
   74              manifest,
   75              manifest.seed,
   76              std::slice::from_ref(&denial),
   77              Verdict::Denied { reason, axis },
   78          );
   79          rec.approval = approval_str;
   80          return rec;
   81      }
   82  
   83      // 4. Mint a Principal holding exactly the effective grant.
   84      let principal = rt.mint_principal(&eff);
   85  
   86      // 5. Run sandboxed to the effective ceiling + budget + seed.
   87      let outcome = rt.run_sandboxed(
   88          &manifest.program,
   89          &principal,
   90          &eff,
   91          &eff.budget,
   92          manifest.seed,
   93      );
   94  
   95      // 6. Seal a tamper-evident record from the observed events + verdict.
   96      let mut rec = build(
   97          run_id,
   98          manifest,
   99          manifest.seed,
  100          &outcome.events,
  101          outcome.verdict,
  102      );
  103      rec.approval = approval_str;
  104      rec
  105  }

```

## E05 — Local subprocess wrapper and per-process temp naming.

`crates/axon-os/src/runtime.rs:495-563`

```text

  495  impl Runtime for AxonCoreRuntime {
  496      fn declared_effects(&self, program: &Path) -> DeclaredEffects {
  497          match std::fs::read_to_string(program) {
  498              Ok(src) => scan_effects(&src),
  499              Err(_) => DeclaredEffects::unknown(), // deny-by-default
  500          }
  501      }
  502  
  503      fn mint_principal(&self, _grant: &Grant) -> PrincipalHandle {
  504          // Attenuation is enforced by the supervisor passing the EFFECTIVE grant
  505          // (J ∩ S, R20-proven ⊆) to the gate before we run; the handle is opaque.
  506          PrincipalHandle(0)
  507      }
  508  
  509      fn run_sandboxed(
  510          &self,
  511          program: &Path,
  512          _principal: &PrincipalHandle,
  513          grant: &Grant,
  514          budget: &Budget,
  515          seed: u64,
  516      ) -> RunOutcome {
  517          let ceiling = grant.effect_set();
  518          // RUNTIME ENFORCEMENT (the sound fence). Rather than running the program
  519          // raw, wrap it: mint a principal + `sandbox_run` it inside an effect
  520          // ceiling. The interpreter then refuses ANY builtin whose effect row
  521          // (AI/Net/IO) exceeds the ceiling — SandboxViolation, exit 8 — at the
  522          // builtin-dispatch level, so an effect cannot be hidden by renaming or
  523          // indirection the way the static-gate source scan could be fooled. The
  524          // static gate is a best-effort PRE-check; THIS is what actually contains.
  525          let src = match std::fs::read_to_string(program) {
  526              Ok(s) => s,
  527              Err(e) => {
  528                  return RunOutcome {
  529                      events: vec![],
  530                      verdict: Verdict::Denied {
  531                          reason: format!("cannot read program: {e}"),
  532                          axis: "io".into(),
  533                      },
  534                  }
  535              }
  536          };
  537          let nonce = done_nonce();
  538          let wrapper_src = wrap_in_sandbox(&src, grant, budget, &nonce);
  539          let wrapper_path = std::env::temp_dir().join(format!(
  540              "axon-os-wrap-{}-{}.ax",
  541              std::process::id(),
  542              program
  543                  .file_stem()
  544                  .and_then(|s| s.to_str())
  545                  .unwrap_or("job")
  546          ));
  547          if std::fs::write(&wrapper_path, &wrapper_src).is_err() {
  548              return RunOutcome {
  549                  events: vec![],
  550                  verdict: Verdict::Denied {
  551                      reason: "cannot stage sandbox wrapper".into(),
  552                      axis: "io".into(),
  553                  },
  554              };
  555          }
  556  
  557          let mut cmd = Command::new(&self.axon_bin);
  558          cmd.arg("run").arg(&wrapper_path);
  559          cmd.env_clear();
  560          cmd.env("AXON_SEED", seed.to_string());
  561          if let Some(p) = std::env::var_os("PATH") {
  562              cmd.env("PATH", p); // cc/linker discovery for the interpreter
  563          }

```

## E06 — Environment projection, virtual clock, cancellation path.

`crates/axon-os/src/runtime.rs:588-625`

```text

  588          if !grant.reproducible {
  589              for key in [
  590                  "AXON_AUDIT_LEDGER",
  591                  "AXON_AI_MOCK",
  592                  "AXON_AI_REPLAY",
  593                  "AXON_PATH",
  594                  "AXON_MAX_DEPTH",
  595              ] {
  596                  if let Some(v) = std::env::var_os(key) {
  597                      cmd.env(key, v);
  598                  }
  599              }
  600          } else {
  601              // Deterministic virtual clock: `now_ms()` becomes a function of the
  602              // run rather than of when it happened, and `sleep_ms(n)` advances it
  603              // without really sleeping. Without this, a hermetic run reading the
  604              // clock produces different bytes every time — which is precisely
  605              // what the profile promises it will not do.
  606              cmd.env(
  607                  "AXON_CLOCK",
  608                  crate::profile::Profile::Hermetic.virtual_clock().unwrap(),
  609              );
  610          }
  611          // Relative paths in the program resolve against the job's directory, so
  612          // an example runs the same wherever it is invoked from (hermetic).
  613          if let Some(dir) = program.parent() {
  614              if !dir.as_os_str().is_empty() {
  615                  cmd.current_dir(dir);
  616              }
  617          }
  618  
  619          // R27/R29: if AXON_KILL_FILE is set, poll it during run_bounded.
  620          // R27 uses this for operator kill (`axon-os kill`); R29 uses it for the
  621          // compliance monitor. Both write `{"latch":"tripped"}` to stop the job.
  622          let kill_file_env = std::env::var_os("AXON_KILL_FILE").map(std::path::PathBuf::from);
  623          let kill_file = kill_file_env.as_deref();
  624          let proc_res = run_bounded(&mut cmd, self.timeout, kill_file);
  625          let _ = std::fs::remove_file(&wrapper_path); // best-effort cleanup

```

## E07 — Current Budget axes and scoped Grant.

`crates/axon-os/src/grant.rs:62-99`

```text

   62  /// A resource budget over a set of axes (mirrors the userland ResBudget). Any
   63  /// axis overrun exhausts the whole budget (conjunctive contract).
   64  #[derive(Debug, Clone, Copy, PartialEq, Eq)]
   65  pub struct Budget {
   66      pub calls: i64,
   67      pub tokens: i64,
   68      pub cost_micro: i64,
   69  }
   70  
   71  /// The induced effect-set view of a grant: which capability axes are *present*
   72  /// (an axis is present iff its allowlist is non-empty / exec ≠ none).
   73  #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
   74  pub struct EffectSet {
   75      pub fs_read: bool,
   76      pub fs_write: bool,
   77      pub net: bool,
   78      pub exec: bool,
   79  }
   80  
   81  /// A capability grant: what a program is permitted to touch, spend, and handle.
   82  #[derive(Debug, Clone, PartialEq, Eq)]
   83  pub struct Grant {
   84      pub fs_read: Vec<PathPrefix>,
   85      pub fs_write: Vec<PathPrefix>,
   86      pub net: Vec<Host>,
   87      pub exec: ExecPolicy,
   88      pub max_label: Label,
   89      pub budget: Budget,
   90      /// Must this run be REPRODUCIBLE (virtual clock, no ambient env)?
   91      ///
   92      /// Carried on the grant rather than passed alongside it because the grant
   93      /// is what reaches the spawn point; a policy that does not travel with the
   94      /// thing it governs is a policy with no enforcement site.
   95      ///
   96      /// Set from `profile = "hermetic"`. Every other profile leaves it false, and
   97      /// the canonical form below excludes it — it constrains HOW a run executes,
   98      /// not WHAT it may touch, so it must not change a grant digest that existing
   99      /// approval tokens were signed over.

```

## E08 — Existing flat manifest and JobManifest.

`crates/axon-os/src/manifest.rs:1-25`

```text

    1  //! R21 §3.1 — the `.axjob` job manifest: parse + validate. Pure (no I/O).
    2  //!
    3  //! Hand-rolled parser for exactly the flat `.axjob` schema (top-level keys +
    4  //! `[grant]` + `[grant.budget]`), per the spec's "no new heavy deps" rule. A
    5  //! malformed manifest fails closed → `Verdict::Malformed` (exit 2). The program
    6  //! path is resolved relative to the manifest dir but NOT stat'd here (existence
    7  //! is an I/O check the supervisor/runtime performs in a later slice).
    8  
    9  use crate::grant::{Budget, ExecPolicy, Grant, Label};
   10  use crate::verdict::Verdict;
   11  use std::path::{Path, PathBuf};
   12  
   13  /// A parsed, validated job manifest (R21 §3.1).
   14  #[derive(Debug, Clone, PartialEq, Eq)]
   15  pub struct JobManifest {
   16      pub program: PathBuf,
   17      pub intent: String,
   18      pub seed: u64,
   19      pub grant: Grant,
   20      /// JOB POLICY: does this job require a valid approval token to run?
   21      ///
   22      /// Deliberately separate from whether a token is PRESENT, which is runtime
   23      /// EVIDENCE. Neither is inferred from the other (triage OSK-P4-H8):
   24      ///
   25      ///   not required + no token            → run

```

## E09 — Existing raw/audit events and RunRecord wire schema.

`crates/axon-os/src/record.rs:19-71`

```text

   19  /// A raw observed action from the runtime, before it is sequenced + chained.
   20  #[derive(Debug, Clone, PartialEq, Eq)]
   21  pub struct RawEvent {
   22      pub action: String,
   23      pub target: String,
   24      pub caps_used: Vec<String>, // axis names; canonicalized (sorted) on build
   25      pub label: String,
   26  }
   27  
   28  impl RawEvent {
   29      pub fn new(action: &str, target: &str, caps: EffectSet, label: &str) -> RawEvent {
   30          RawEvent {
   31              action: action.to_string(),
   32              target: target.to_string(),
   33              caps_used: axes(caps),
   34              label: label.to_string(),
   35          }
   36      }
   37  }
   38  
   39  /// A sequenced, hash-chained audit event (R21 §3.4).
   40  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
   41  pub struct AuditEvent {
   42      pub seq: u64,
   43      pub action: String,
   44      pub target: String,
   45      pub caps_used: Vec<String>,
   46      pub label: String,
   47      pub prev_hash: String,
   48      pub hash: String,
   49  }
   50  
   51  /// The tamper-evident run record (R21 §3.4); serialized as `axon-os-record/1`.
   52  #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
   53  pub struct RunRecord {
   54      pub schema: String,
   55      pub run_id: String,
   56      pub manifest_digest: String,
   57      pub seed: u64,
   58      pub events: Vec<AuditEvent>,
   59      pub verdict: Verdict,
   60      /// What the authorization boundary decided for this run.
   61      ///
   62      /// The record previously carried NO approval field, so an unapproved
   63      /// execution and an approved one archived identically — a reviewer reading
   64      /// the record could not tell "signed off" from "nobody checked". Defaults
   65      /// on deserialization so records written before this field keep loading,
   66      /// and an older record reads as `unknown` rather than as approved: an
   67      /// ABSENT statement is not a positive one.
   68      #[serde(default = "unknown_approval")]
   69      pub approval: String,
   70      pub record_digest: String,
   71  }

```

## E10 — Existing replay re-executes under the supervisor.

`crates/axon-os/src/replay.rs:1-40`

```text

    1  //! R21 §4.6 — deterministic replay + record verification. Pure core; the
    2  //! `Runtime` and the record store are injected.
    3  //!
    4  //! `replay` (1) verifies the stored record's integrity, then (2) re-executes
    5  //! the job with the RECORDED seed and asserts the new record is byte-identical
    6  //! to the stored one. Divergence or tamper → `VerifyMismatch` (exit 9). This is
    7  //! the deterministic-audit guarantee: a recorded run can be reproduced and
    8  //! proven to be exactly what happened.
    9  
   10  use crate::manifest::JobManifest;
   11  use crate::record::{to_json, verify, RunRecord, VerifyMismatch};
   12  use crate::runtime::Runtime;
   13  use crate::supervisor;
   14  
   15  /// Re-run a stored record's job and assert the result is byte-identical.
   16  ///
   17  /// `stored` is the record loaded from disk; `manifest` is the job it ran (the
   18  /// caller loads both — replay does no I/O itself). `supervisor_grant` is the
   19  /// supervisor's current authority. Returns the freshly-produced record on a
   20  /// match, or `VerifyMismatch` on tamper / divergence.
   21  pub fn replay(
   22      stored: &RunRecord,
   23      manifest: &JobManifest,
   24      job_path: &std::path::Path,
   25      supervisor_grant: &crate::grant::Grant,
   26      rt: &impl Runtime,
   27  ) -> Result<RunRecord, VerifyMismatch> {
   28      // 1. The stored record must be intact before we trust it.
   29      verify(stored)?;
   30  
   31      // 2. Re-execute with the recorded seed (the manifest carries it) under the
   32      //    stored run-id, then compare byte-for-byte.
   33      // The job path is threaded through so the authorization boundary inside
   34      // `supervisor::run` can find this job's `.approval` sibling. Replay
   35      // previously reached execution without any approval check at all.
   36      let fresh = supervisor::run(manifest, job_path, supervisor_grant, &stored.run_id, rt);
   37      if to_json(&fresh) == to_json(stored) {
   38          Ok(fresh)
   39      } else {
   40          Err(VerifyMismatch {

```

## E11 — Binary-only launcher packaging.

`crates/axon-vm/Cargo.toml:1-27`

```text

    1  [package]
    2  name = "axon-vm"
    3  version.workspace = true
    4  edition.workspace = true
    5  authors.workspace = true
    6  license.workspace = true
    7  description = "Axon microVM launcher: Firecracker API client, BPF policy generation, Principal registry"
    8  
    9  [[bin]]
   10  name = "axon-vm"
   11  path = "src/main.rs"
   12  
   13  [dependencies]
   14  serde = { workspace = true }
   15  serde_json = { workspace = true }
   16  clap = { workspace = true }
   17  base64 = "0.22"
   18  toml = "0.8"
   19  sha2 = "0.10"
   20  hex = "0.4"
   21  axon-attest = { path = "../axon-attest" }
   22  
   23  [target.'cfg(target_os = "linux")'.dependencies]
   24  libc = "0.2"
   25  
   26  [dev-dependencies]
   27  tempfile = "3"

```

## E12 — Firecracker launcher role.

`crates/axon-vm/src/main.rs:1-14`

```text

    1  //! axon-vm — Axon microVM launcher.
    2  //!
    3  //! Wraps Firecracker to run an Axon program in a hardware-isolated microVM with:
    4  //!   • Capability-derived seccomp-BPF policy (from `.axmeta` manifest)
    5  //!   • MMDS-delivered boot policy (principal, budget, allowed effects, BPF)
    6  //!   • vsock host_await substrate for interactive programs
    7  //!   • Principal registry (~/.config/axon/principals.toml)
    8  //!   • R26: software-TPM attestation gate (axon-vm attest)
    9  //!
   10  //! Commands:
   11  //!   axon-vm run <program.ax>  [options]     Launch program in a microVM
   12  //!   axon-vm attest --kernel K [options]     Measure kernel, produce attestation report
   13  //!   axon-vm principal add <name> [options]  Register a principal
   14  //!   axon-vm principal list                  List principals

```

## E13 — VM policy is not the full OS Grant.

`crates/axon-vm/src/main.rs:593-636`

```text

  593  /// Schema: axon-vm-mmds/1 — written to MMDS before VM boot.
  594  #[derive(Serialize, Deserialize, Debug, Clone)]
  595  struct MmdsPayload {
  596      schema: String,
  597      run_id: String,
  598      principal: Option<String>,
  599      allowed_effects: Option<Vec<String>>,
  600      budget_tokens: Option<u64>,
  601      source_hash: Option<String>,
  602      seccomp_bpf_b64: Option<String>,
  603  }
  604  
  605  /// Schema: axon-manifest/1 — sidecar emitted by `axon build --emit-manifest`. `schema`/`source`/
  606  /// `binary`/`per_fn` mirror the full sidecar schema for documentation/forward-compat even though
  607  /// only `effect_union`/`syscall_hint`/`risk` are read today — pre-existing, found (not introduced)
  608  /// while adding axon-vm to gate.sh's clippy coverage 2026-07-19.
  609  #[derive(Deserialize, Debug, Default)]
  610  #[allow(dead_code)]
  611  struct AxonManifest {
  612      schema: Option<String>,
  613      source: Option<String>,
  614      binary: Option<String>,
  615      effect_union: Option<Vec<String>>,
  616      syscall_hint: Option<Vec<String>>,
  617      /// AUDIT T48. This was `Option<String>`, but `axon build --emit-manifest`
  618      /// emits `"risk": 0` — a NUMBER — alongside `"risk_label": "low"`. So every
  619      /// manifest failed to deserialise, and `load_manifest`'s `unwrap_or_default()`
  620      /// silently substituted an empty one. The producer and the consumer of the
  621      /// policy source of record had never once agreed, and nothing said so.
  622      risk: Option<u8>,
  623      risk_label: Option<String>,
  624      #[serde(default)]
  625      per_fn: Vec<serde_json::Value>,
  626  }
  627  
  628  /// Gap 7: Principal registry entry (~/.config/axon/principals.toml).
  629  #[derive(Serialize, Deserialize, Debug, Clone)]
  630  struct Principal {
  631      name: String,
  632      budget_tokens: u64,
  633      allowed_effects: Vec<String>,
  634      mem_mib: u64,
  635      cpu_pct: u32,
  636  }

```

## E14 — Direct spawn and fallible work after spawning.

`crates/axon-vm/src/main.rs:2237-2297`

```text

 2237  fn run_in_firecracker(
 2238      program: &Path,
 2239      kernel: &Path,
 2240      initrd: &Path,
 2241      mem_mib: u64,
 2242      vcpus: u64,
 2243      vsock_port: u32,
 2244      socket_path: &Path,
 2245      mmds: &MmdsPayload,
 2246      principal: Option<&Principal>,
 2247  ) -> Result<RunResult, Box<dyn std::error::Error>> {
 2248      // Check Firecracker is installed.
 2249      let fc_bin = which_firecracker()?;
 2250  
 2251      // Spawn Firecracker.
 2252      let mut fc = Command::new(&fc_bin)
 2253          .arg("--api-sock")
 2254          .arg(socket_path)
 2255          .stdin(Stdio::null())
 2256          .stdout(Stdio::piped())
 2257          .stderr(Stdio::piped())
 2258          .spawn()?;
 2259  
 2260      // Drain Firecracker's stdout/stderr (the guest serial console + FC logs) to our
 2261      // stderr on background threads. Without this the piped buffers fill and the guest
 2262      // BLOCKS — a deadlock, since `fc.wait()` can't return until FC exits and FC can't
 2263      // make progress while its stdout pipe is full. Draining also surfaces the guest
 2264      // boot log (set AXON_VM_QUIET=1 to suppress).
 2265      //
 2266      // The stdout drain also WATCHES for the guest's exit sentinels. The guest kernel
 2267      // writes `-VIOLATION8` to COM1 and then attempts an ACPI S5 power-off — which
 2268      // Firecracker does not implement, so the guest spins in `hlt` and the run would
 2269      // otherwise be reported as a 124 timeout with `ok:true` (P7-KRN-04). Reading the
 2270      // sentinel means the guest's own verdict decides the outcome, independent of
 2271      // whether it manages to power the machine off.
 2272      let quiet = env::var("AXON_VM_QUIET").map(|v| v == "1").unwrap_or(false);
 2273      let signal: Arc<Mutex<Option<GuestOutcome>>> = Arc::new(Mutex::new(None));
 2274      let mut drains = Vec::new();
 2275      if let Some(out) = fc.stdout.take() {
 2276          let sig = Arc::clone(&signal);
 2277          drains.push(std::thread::spawn(move || {
 2278              drain_to_stderr(out, "guest", quiet, Some(sig))
 2279          }));
 2280      }
 2281      if let Some(err) = fc.stderr.take() {
 2282          drains.push(std::thread::spawn(move || {
 2283              drain_to_stderr(err, "fc", quiet, None)
 2284          }));
 2285      }
 2286  
 2287      // Wait for Firecracker to create its API socket (typically < 50ms; a fixed 5s margin
 2288      // was found flaky under heavy host CPU contention — R30's own acceptance gate observed
 2289      // acc_a1/acc_a4 failing at this exact R26_ATTESTATION stage under concurrent load, isolated
 2290      // reruns always passing clean, "root cause not chased further" per REQUIREMENTS.md — a
 2291      // starved Firecracker process spawn can plausibly take longer than 5s to even get scheduled.
 2292      // Tunable via AXON_VM_SOCKET_TIMEOUT_SECS (default 5), mirroring AXON_VM_TIMEOUT_SECS below.
 2293      let socket_timeout_secs: u64 = env::var("AXON_VM_SOCKET_TIMEOUT_SECS")
 2294          .ok()
 2295          .and_then(|v| v.parse().ok())
 2296          .unwrap_or(5);
 2297      let api = wait_for_socket(socket_path, Duration::from_secs(socket_timeout_secs))?;

```

## E15 — vsock path, no NIC, and balloon rather than jailer controls.

`crates/axon-vm/src/main.rs:2327-2376`

```text

 2327      // Configure vsock device so the guest can use host_await.
 2328      // uds_path is the host-side Unix socket; the guest connects via CID 2.
 2329      let vsock_host_uds = format!("/tmp/axon-vm-vsock-{}.sock", process::id());
 2330      fc_put(
 2331          &api,
 2332          "/vsock",
 2333          &serde_json::json!({
 2334              "guest_cid": 3,
 2335              "uds_path": vsock_host_uds,
 2336          }),
 2337      )?;
 2338  
 2339      // MMDS is a SECONDARY policy channel and requires a network interface to bind to.
 2340      // This launcher delivers the policy via the kernel cmdline (`axon.policy=<base64>`,
 2341      // the K2 cmdline-reader path) and configures no NIC, so MMDS V2 config with an empty
 2342      // `network_interfaces` is rejected (400) — correctly. Make it best-effort: try it for
 2343      // hosts that do add a NIC, but never fail the run, since the cmdline already carries
 2344      // the policy. (Was a hard `?` that aborted every run at /mmds/config.)
 2345      if let Err(e) = fc_put(
 2346          &api,
 2347          "/mmds/config",
 2348          &serde_json::json!({
 2349              "version": "V2",
 2350              "network_interfaces": [],
 2351          }),
 2352      ) {
 2353          eprintln!("axon-vm: MMDS config skipped ({e}); policy is delivered via the kernel cmdline");
 2354      } else {
 2355          // Only write the payload if MMDS config succeeded.
 2356          let mmds_content = serde_json::json!({ "latest": { "axon": mmds } });
 2357          if let Err(e) = fc_put(&api, "/mmds", &mmds_content) {
 2358              eprintln!("axon-vm: MMDS payload write skipped ({e})");
 2359          }
 2360      }
 2361  
 2362      // Apply cgroup limits via jailer-style resource controls (if principal has limits).
 2363      // In production use, Firecracker would be launched via jailer with uid/gid isolation.
 2364      // Here we set balloon memory limits instead (available without jailer).
 2365      if let Some(p) = principal {
 2366          if p.mem_mib < mem_mib {
 2367              fc_put(
 2368                  &api,
 2369                  "/balloon",
 2370                  &serde_json::json!({
 2371                      "amount_mib": mem_mib - p.mem_mib,
 2372                      "deflate_on_oom": true,
 2373                  }),
 2374              )?;
 2375          }
 2376      }

```

## E16 — Echo relay, read-only program device, start action.

`crates/axon-vm/src/main.rs:2378-2407`

```text

 2378      // Start a vsock relay thread to bridge vsock ↔ host_await callbacks.
 2379      // Uses EchoHandler by default; plug in a custom HostAwaitHandler to forward
 2380      // requests to a real host process (e.g. a stdin/stdout bridge).
 2381      let vsock_uds = vsock_host_uds.clone();
 2382      let handler: Arc<dyn HostAwaitHandler> = Arc::new(EchoHandler);
 2383      let _vsock_thread = std::thread::spawn(move || {
 2384          vsock_relay(&vsock_uds, vsock_port, handler);
 2385      });
 2386  
 2387      // Copy the .ax program into a tmpfs-backed guest path.
 2388      // For real deployments this would be a read-only virtio-blk device.
 2389      // We pass it via a read-only drive.
 2390      let prog_abs = program.canonicalize()?;
 2391      fc_put(
 2392          &api,
 2393          "/drives/program",
 2394          &serde_json::json!({
 2395              "drive_id": "program",
 2396              "path_on_host": prog_abs.to_str().unwrap(),
 2397              "is_root_device": false,
 2398              "is_read_only": true,
 2399          }),
 2400      )?;
 2401  
 2402      // Start the VM.
 2403      fc_put(
 2404          &api,
 2405          "/actions",
 2406          &serde_json::json!({"action_type": "InstanceStart"}),
 2407      )?;

```

## E17 — Guest outcome handling and normal-path cleanup.

`crates/axon-vm/src/main.rs:2425-2482`

```text

 2425      let (exit_code, outcome) = loop {
 2426          if let Some(status) = fc.try_wait()? {
 2427              // Firecracker exited. A sentinel, if any, is the more specific answer.
 2428              let sig = *signal.lock().unwrap();
 2429              break match sig {
 2430                  Some(GuestOutcome::Violation) => (8, GuestOutcome::Violation),
 2431                  Some(GuestOutcome::Panic(c)) => (c, GuestOutcome::Panic(c)),
 2432                  Some(GuestOutcome::Exited(c)) => (c, GuestOutcome::Exited(c)),
 2433                  _ => {
 2434                      let c = status.code().unwrap_or(1);
 2435                      (c, GuestOutcome::Exited(c))
 2436                  }
 2437              };
 2438          }
 2439  
 2440          let sig = *signal.lock().unwrap();
 2441          if let Some(o) = sig {
 2442              let since = *sentinel_seen_at.get_or_insert_with(Instant::now);
 2443              if since.elapsed() >= sentinel_grace {
 2444                  let _ = fc.kill();
 2445                  let _ = fc.wait();
 2446                  eprintln!(
 2447                      "axon-vm: guest signalled {} but did not power off — reaped. \
 2448                       (The guest's ACPI S5 write is a no-op under Firecracker; the \
 2449                       guest verdict is authoritative.)",
 2450                      o.as_str()
 2451                  );
 2452                  break match o {
 2453                      GuestOutcome::Violation => (8, o),
 2454                      GuestOutcome::Panic(c) => (c, o),
 2455                      GuestOutcome::Exited(c) => (c, o),
 2456                      other => (0, other),
 2457                  };
 2458              }
 2459          }
 2460  
 2461          if Instant::now() >= deadline {
 2462              let _ = fc.kill();
 2463              let _ = fc.wait();
 2464              eprintln!(
 2465                  "axon-vm: guest did not power off within {timeout_secs}s — killed. \
 2466                   See the guest log above; the guest image's init must run the program \
 2467                   and then poweroff/reboot for the VM to exit."
 2468              );
 2469              break (124, GuestOutcome::Timeout);
 2470          }
 2471          std::thread::sleep(Duration::from_millis(100));
 2472      };
 2473  
 2474      for d in drains {
 2475          let _ = d.join();
 2476      }
 2477  
 2478      // Clean up socket files.
 2479      let _ = fs::remove_file(socket_path);
 2480      let _ = fs::remove_file(&vsock_host_uds);
 2481  
 2482      Ok(RunResult { exit_code, outcome })

```

## E18 — Versioned result, guest verdict and CLI exit preservation.

`crates/axon-vm/src/main.rs:1514-1546`

```text

 1514      let elapsed_ms = start.elapsed().as_millis();
 1515  
 1516      // `ok` reports what the GUEST did. It used to be `result.is_ok()` — whether the
 1517      // launcher succeeded in driving the Firecracker API — so a guest that hit its
 1518      // deadline, or that refused an operation and halted, was reported `ok:true`
 1519      // (P7-KRN-04). A run is OK only if the guest reached a definite end and exited 0.
 1520      let exit_code = result.as_ref().map(|r| r.exit_code).unwrap_or(-1);
 1521      let outcome = result
 1522          .as_ref()
 1523          .map(|r| r.outcome.as_str())
 1524          .unwrap_or("launch-failed");
 1525      let ok = matches!(result.as_ref(), Ok(r) if matches!(r.outcome, GuestOutcome::Exited(_)))
 1526          && exit_code == 0;
 1527  
 1528      if json_out {
 1529          let out = serde_json::json!({
 1530              "schema": "axon-vm-run/2",
 1531              "ok": ok,
 1532              "run_id": run_id,
 1533              "exit_code": exit_code,
 1534              "guest_outcome": outcome,
 1535              "elapsed_ms": elapsed_ms,
 1536              "error": result.as_ref().err().map(|e| e.to_string()),
 1537              "principal": principal_name,
 1538              "risk": manifest.risk,
 1539          });
 1540          println!("{}", serde_json::to_string_pretty(&out).unwrap());
 1541          // `--json` used to print and fall off the end of this function, so
 1542          // `axon-vm run --json` exited 0 no matter what the guest did — every caller
 1543          // testing `$?` was reading a constant. Propagate, as the non-JSON path does.
 1544          match result {
 1545              Ok(_) => process::exit(exit_code),
 1546              Err(_) => process::exit(1),

```

## E19 — Linux guest init enforcement and source-hash limitation.

`crates/axon-guest-init/src/main.rs:1-31`

```text

    1  //! axon-guest-init — static PID-1 supervisor for Axon microVM guests.
    2  //!
    3  //! Boot sequence:
    4  //!   1. Re-seed entropy from virtio-rng (/dev/urandom)
    5  //!   2. Read capability policy from MMDS at 169.254.169.254 (schema axon-vm-mmds/1).
    6  //!      If no policy can be read, REFUSE to start the guest — an absent policy is
    7  //!      not a permissive one, and this binary exists to install the sandbox.
    8  //!      `AXON_GUEST_ALLOW_NO_POLICY=1` opts into the old unpoliced behaviour
    9  //!      (development only) and says so loudly on every boot.
   10  //!   3. Fork: parent becomes PID-1 supervisor; child applies seccomp then execs Axon
   11  //!   4. Supervisor loop: reap zombies, forward SIGTERM/SIGINT, exit with child's code
   12  //!
   13  //! Invocation:
   14  //!   axon-guest-init <binary> [args...]
   15  //!   axon-guest-init /usr/bin/axon run /axon/program.ax
   16  //!
   17  //! Environment variables exported to child (from MMDS payload):
   18  //!   AXON_PRINCIPAL, AXON_BUDGET_TOKENS, AXON_RUN_ID, AXON_ALLOWED_EFFECTS,
   19  //!   AXON_SOURCE_HASH
   20  //!
   21  //! Two of those are the guest's ENFORCED policy, not just labels:
   22  //! AXON_ALLOWED_EFFECTS is the run's effect ceiling (SandboxViolation, exit 8)
   23  //! and AXON_BUDGET_TOKENS its AI token cap (E1303, exit 5). Both were exported
   24  //! here and read by nothing for as long as they have existed, so a policy that
   25  //! capped tokens or restricted effects produced a guest that did neither and
   26  //! said nothing about it. The other three are LABELS, deliberately: AXON_PRINCIPAL
   27  //! is audit attribution, AXON_RUN_ID correlates records, and AXON_SOURCE_HASH
   28  //! carries the approved digest. None of the three grants or withholds anything,
   29  //! and nothing in the workspace reads them -- in particular the guest does NOT
   30  //! today check the loaded image against AXON_SOURCE_HASH (R36 S2 owns that; see
   31  //! its (i) clause). Do not read this list as a policy that is enforced.

```

## E20 — Linux init currently reads MMDS.

`crates/axon-guest-init/src/main.rs:264-285`

```text

  264  fn read_mmds() -> Result<Option<MmdsPayload>, String> {
  265      // Step 1: obtain a V2 session token.
  266      let token = mmds_get_token()?;
  267  
  268      // Step 2: GET /latest/axon with the session token.
  269      let body = mmds_get("/latest/axon", &token)?;
  270  
  271      let body = body.trim();
  272      if body.is_empty() || body == "null" {
  273          return Ok(None);
  274      }
  275  
  276      // The launcher writes the full payload under /latest/axon in the MMDS
  277      // store, so the response body IS the MmdsPayload JSON.
  278      // MALFORMED, not unavailable. Both fail closed, but the call site logged
  279      // every Err as "MMDS unavailable", so a launcher writing bad JSON and a
  280      // launcher that cannot be reached produced the same message and sent the
  281      // operator looking at the network.
  282      let payload: MmdsPayload = serde_json::from_str(body)
  283          .map_err(|e| format!("MMDS policy is MALFORMED (not unreachable): {e}"))?;
  284  
  285      Ok(Some(payload))

```

## E21 — Kernel demo explicitly does not load full interpreter/VFS.

`crates/axon-guest-kernel/src/enforce.rs:316-364`

```text

  316  /// K5: launch the Axon program under the active syscall gate.
  317  ///
  318  /// The full path — loading the ~7 MB interpreter ELF from the initramfs and giving it a
  319  /// VFS so it can open the program file — is the remaining K5 work. What we demonstrate
  320  /// here is the part that carries the security claim: the program's first effectful
  321  /// operation is opening its input file, a real `openat` syscall, and the gate
  322  /// **intercepts and denies it** when the policy withholds the FS effect.
  323  ///
  324  /// We issue the genuine `syscall` instruction. The CPU traps to the LSTAR handler
  325  /// (`syscall_entry`), which calls `syscall_dispatch(257)`. Under an FS-denying policy it
  326  /// returns the VIOLATION sentinel, the handler jumps to `violation_exit`, and the VM
  327  /// halts with exit code 8 (`-VIOLATION8` on the serial stream). When FS *is* granted the
  328  /// open would be permitted, so we don't issue it here — the allowed return path
  329  /// (`sysretq`) needs ring-3 user segments the boot GDT doesn't yet define, which is part
  330  /// of the full-execution work — and instead halt cleanly.
  331  pub fn run_program(policy: &Policy) -> ! {
  332      const SYS_OPENAT: u64 = 257;
  333      kprintln!(
  334          "[axon-kernel] K5: launch — program's first op is openat(\"/axon/hello.ax\") → needs FS"
  335      );
  336  
  337      if policy.allowed_effects.contains(EffectSet::FS) {
  338          kprintln!("[axon-kernel] K5: policy GRANTS FS — open permitted; program would proceed");
  339          kprintln!(
  340              "[axon-kernel] K5: (full interpreter ELF load + VFS is the remaining work) — halting"
  341          );
  342          clean_halt();
  343      }
  344  
  345      kprintln!(
  346          "[axon-kernel] K5: policy WITHHOLDS FS — issuing the real openat to exercise the gate"
  347      );
  348      unsafe {
  349          core::arch::asm!(
  350              "syscall",
  351              inout("rax") SYS_OPENAT => _, // dispatch result; VIOLATION path never returns here
  352              in("rdi") 0u64,               // dirfd  (denied before the args are ever used)
  353              in("rsi") 0u64,               // path
  354              in("rdx") 0u64,               // flags
  355              out("rcx") _,                 // clobbered by SYSCALL (saved RIP)
  356              out("r11") _,                 // clobbered by SYSCALL (saved RFLAGS)
  357              options(nostack),
  358          );
  359      }
  360  
  361      // A withheld-effect syscall diverts into `violation_exit` (power-off) and never
  362      // returns. Reaching here means the gate FAILED to block it — make that loud.
  363      kprintln!("[axon-kernel] K5: UNEXPECTED — the gate did NOT block a withheld-FS syscall!");
  364      clean_halt();

```

## E22 — Browser-oriented WASM interpreter ABI.

`crates/axon-wasm/src/lib.rs:1-24`

```text

    1  //! `axon-wasm` — the Axon interpreter as a `wasm32-unknown-unknown` cdylib, driven
    2  //! from JavaScript by raw C-ABI exports (no wasm-bindgen, so the module has zero
    3  //! JS-glue imports and loads with a bare `WebAssembly.instantiate`).
    4  //!
    5  //! It is the in-browser PLAYGROUND/REPL: run arbitrary `.ax` source dynamically
    6  //! via the tree-walking interpreter — distinct from the codegen browser path,
    7  //! which AOT-compiles each program separately and can't `eval` source. It is also
    8  //! the entry-point foundation for the R15 browser `host_await` binding (R7c): a
    9  //! follow-on slice adds an imported `axon_host_await` + Asyncify so a suspending
   10  //! program can be driven from the page; this slice runs the non-suspending case.
   11  //!
   12  //! ABI (all lengths are byte counts; pointers index the module's linear memory):
   13  //!   axon_alloc(len) -> ptr        — reserve `len` bytes; JS writes the source there
   14  //!   axon_eval(ptr, len) -> i32    — parse+run the source; returns the exit code;
   15  //!                                   captures the program's stdout for read-back
   16  //!   axon_output_ptr() -> ptr      — start of the captured output (valid until the
   17  //!   axon_output_len() -> len        next axon_eval)
   18  //!
   19  //! Typical JS:
   20  //!   const p = inst.exports.axon_alloc(bytes.length);
   21  //!   new Uint8Array(mem.buffer, p, bytes.length).set(bytes);
   22  //!   const code = inst.exports.axon_eval(p, bytes.length);
   23  //!   const out  = new TextDecoder().decode(new Uint8Array(mem.buffer,
   24  //!                  inst.exports.axon_output_ptr(), inst.exports.axon_output_len()));

```

## E23 — WASM dependency and build shape.

`crates/axon-wasm/Cargo.toml:8-28`

```text

    8  # A wasm32-unknown-unknown cdylib: the codegen-free Axon INTERPRETER exposed to
    9  # JavaScript as a dynamic `eval` of `.ax` source — the in-browser playground /
   10  # REPL (run arbitrary source with no per-program AOT compile, which the codegen
   11  # browser path can't do), and the entry-point foundation for the R15 browser
   12  # `host_await` binding (R7c). Depends on axon-core WITHOUT the codegen feature
   13  # (no LLVM/inkwell on wasm) — the pure tree-walking interpreter only.
   14  [lib]
   15  crate-type = ["cdylib", "rlib"]
   16  
   17  [dependencies]
   18  axon-core = { path = "../axon-core", default-features = false }
   19  # The prose→AST surface compiler (clap-free lib), so the playground compiles a
   20  # PROSE goal file in-browser (`axon goal`), not just `.ax`. default-features off
   21  # drops clap (CLI-only) from the wasm build.
   22  axon-surface = { path = "../axon-surface", default-features = false }
   23  
   24  # R7c Slice 2: the browser pure-compute PARITY test runs the wasm interpreter's
   25  # `axon_eval` ABI in REAL headless Chrome via wasm-bindgen-test and asserts the
   26  # exit code + captured stdout match the native/interp oracle (recorded into the
   27  # .ax source as a `// EXPECT:` comment so the wasm test is oracle-free). Pinned to
   28  # match the installed `wasm-bindgen` CLI 0.2.122 (the crate + CLI versions MUST

```

## E24 — Existing AxonHost host seam.

`crates/axon-core/src/host.rs:20-29`

```text

   20  /// The host capabilities the interpreter needs from its environment. A
   21  /// `DefaultHost` (native) wraps std; a browser/wasm build supplies a virtual
   22  /// impl (fetch-backed FS, a provided env map, performance.now). Every method
   23  /// returns the SAME shape the builtin already returns, so behavior is identical.
   24  pub trait AxonHost {
   25      fn read_file(&self, path: &str) -> Result<String, String>;
   26      fn write_file(&self, path: &str, data: &str) -> Result<(), String>;
   27      fn env_var(&self, key: &str) -> Option<String>;
   28      fn now_ms(&self) -> i64;
   29      fn sleep_ms(&self, ms: u64);

```

## E25 — Default-deny exec/network in virtual hosts.

`crates/axon-core/src/host.rs:84-108`

```text

   84      /// Spawn a process (the `exec` builtin / `@[contained] exec` capability).
   85      /// `cmd` is the program; `args` the argument list. Returns the captured
   86      /// stdout on success, or a `str` error. Defaults to **denied** — a host that
   87      /// does not explicitly grant process spawning refuses it, so a virtual
   88      /// (browser/wasm/sandbox) host is exec-free unless it opts in. `DefaultHost`
   89      /// (native) overrides this to actually spawn.
   90      fn exec(&self, _cmd: &str, _args: &[String]) -> Result<String, String> {
   91          Err("exec is not permitted by the active host".to_string())
   92      }
   93  
   94      /// HTTP GET `url` with `headers` (a JSON object string, `"{}"` for none).
   95      /// Returns Ok(body) on 2xx, Err("HTTP <status>: <body>") on non-2xx, or
   96      /// Err(message) on transport failure. Defaults to **denied** — a virtual
   97      /// (wasm/sandbox) host is network-free unless it opts in. `DefaultHost`
   98      /// (native, `asi-runtime` feature) overrides this to use a blocking reqwest
   99      /// client.
  100      fn http_get(&self, _url: &str, _headers: &str) -> Result<String, String> {
  101          Err("http_get requires the asi-runtime feature or a network-capable host".to_string())
  102      }
  103  
  104      /// HTTP POST `url` with `headers` (JSON object string) and `body`.
  105      /// Returns Ok(response_body) on 2xx, Err("HTTP <status>: <body>") on non-2xx,
  106      /// or Err(message) on transport failure. Defaults to **denied**.
  107      fn http_post(&self, _url: &str, _headers: &str, _body: &str) -> Result<String, String> {
  108          Err("http_post requires the asi-runtime feature or a network-capable host".to_string())

```

## E26 — Identity is not issuer authentication; Observed unknown.

`crates/axon-cortex/src/lib.rs:36-66`

```text

   36  /// `axc1:`-tagged SHA-256 over canonical bytes.
   37  ///
   38  /// IDENTITY, not authenticity: it says two artifacts are the same artifact, and
   39  /// says nothing about who produced either. An issuer check is a separate
   40  /// signature/registry lookup that this crate deliberately does not pretend to
   41  /// provide.
   42  pub fn content_digest(bytes: &[u8]) -> String {
   43      use sha2::{Digest, Sha256};
   44      let mut h = Sha256::new();
   45      h.update(bytes);
   46      format!("axc1:{:x}", h.finalize())
   47  }
   48  
   49  /// A fact the observer either knows or explicitly does not.
   50  ///
   51  /// There is no `Default` and no `unwrap_or`-shaped convenience on purpose. A
   52  /// partial observer "supplies unknown type/parse facts rather than fabricated
   53  /// defaults" (PROTOCOLS.md), and the only way to make that hold under later
   54  /// edits is to leave no cheap path from `Unknown` to a plausible value.
   55  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
   56  #[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
   57  pub enum Observed<T> {
   58      Known {
   59          value: T,
   60      },
   61      /// Not merely absent — absent WITH A REASON, which is what makes an
   62      /// omission auditable instead of indistinguishable from a null.
   63      Unknown {
   64          reason: String,
   65      },
   66  }

```

## E27 — WorkspaceSnapshot records digests, scope, and parent.

`crates/axon-cortex/src/lib.rs:91-140`

```text

   91  /// A content-addressed snapshot of the workspace region an episode may touch.
   92  #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
   93  #[serde(deny_unknown_fields)]
   94  pub struct WorkspaceSnapshot {
   95      pub snapshot_id: String,
   96      pub parent_snapshot_id: Option<String>,
   97      /// Path → content digest. Sorted on canonicalisation so the snapshot's own
   98      /// digest is stable across map iteration order.
   99      pub files: Vec<(String, String)>,
  100      /// What the observer was allowed to look at. An episode that reasons beyond
  101      /// this scope is reasoning about things it never observed.
  102      pub observation_scope: Vec<String>,
  103  }
  104  
  105  impl WorkspaceSnapshot {
  106      /// The snapshot's identity, over canonical bytes with `files` sorted.
  107      pub fn digest(&self) -> String {
  108          let mut files = self.files.clone();
  109          files.sort();
  110          let canon = files
  111              .iter()
  112              .map(|(p, d)| format!("{p}\u{0}{d}"))
  113              .collect::<Vec<_>>()
  114              .join("\u{1}");
  115          // The SCOPE and the PARENT are covered too.
  116          //
  117          // Hashing `files` alone left this unable to witness most of what it is
  118          // presented as identifying. `observation_scope` decides what the
  119          // episode is entitled to reason about — its own doc says an episode
  120          // reasoning beyond it "is reasoning about things it never observed" —
  121          // and two snapshots over different scopes hashed identically whenever
  122          // the extra paths happened to be absent. `parent_snapshot_id` is what
  123          // makes a chain of states auditable rather than a set of orphans, and
  124          // it could be rewritten to any value with every recorded digest still
  125          // validating.
  126          //
  127          // The scope is sorted so the digest names the SET, not the order the
  128          // caller happened to pass — the same property the file list already
  129          // had.
  130          let mut scope = self.observation_scope.clone();
  131          scope.sort();
  132          content_digest(
  133              format!(
  134                  "{canon}\u{2}{}\u{2}{}",
  135                  scope.join("\u{1}"),
  136                  self.parent_snapshot_id.as_deref().unwrap_or("<root>")
  137              )
  138              .as_bytes(),
  139          )
  140      }

```

## E28 — Snapshot collects hashes, not durable content objects.

`crates/axon-cortex/src/runner.rs:268-321`

```text

  268      /// Snapshot the workspace region, content-addressed.
  269      pub fn snapshot(&mut self, scope: &[&str]) -> std::io::Result<WorkspaceSnapshot> {
  270          let mut files = Vec::new();
  271          for rel in scope {
  272              let p = self.workspace.join(rel);
  273              if p.is_file() {
  274                  let bytes = std::fs::read(&p)?;
  275                  files.push(((*rel).to_string(), content_digest(&bytes)));
  276              } else {
  277                  // AN ABSENT FILE IS RECORDED AS ABSENT, not omitted.
  278                  //
  279                  // Skipping it silently made two different workspaces share an
  280                  // id: a scope of ["a.ax"] with `a.ax` deleted produced exactly
  281                  // the same empty file list — and therefore the same
  282                  // snapshot_id — as a scope of ["b.ax"] with `b.ax` deleted, or
  283                  // as an empty scope. A grant pinned to the first state was
  284                  // accepted as current in the second, so `StaleSnapshot` could
  285                  // not fire between two genuinely different states.
  286                  //
  287                  // It also meant deleting the file under repair moved nothing
  288                  // in the recorded evidence. That is this crate's first rule —
  289                  // an absence must not render as a present fact — broken in the
  290                  // function that computes the evidence.
  291                  files.push(((*rel).to_string(), "<absent>".to_string()));
  292              }
  293          }
  294          files.sort();
  295          // Derived from content, never a constant. A grant is pinned to the
  296          // state it was granted over, so if the id does not move when the
  297          // workspace moves, `StaleSnapshot` can never fire on a real pair of
  298          // snapshots and the check is decoration.
  299          let snapshot_id = content_digest(
  300              files
  301                  .iter()
  302                  .map(|(p, d)| format!("{p}\u{0}{d}\n"))
  303                  .collect::<String>()
  304                  .as_bytes(),
  305          );
  306          let snap = WorkspaceSnapshot {
  307              snapshot_id,
  308              parent_snapshot_id: self.last_snapshot_id.take(),
  309              files,
  310              observation_scope: scope.iter().map(|s| s.to_string()).collect(),
  311          };
  312          self.last_snapshot_id = Some(snap.snapshot_id.clone());
  313          self.episode.push(EpisodeEvent::Snapshot {
  314              snapshot_id: snap.snapshot_id.clone(),
  315              snapshot_digest: snap.digest(),
  316          });
  317          Ok(snap)
  318      }
  319  
  320      /// Partial observation: run the real checker and record what it says.
  321      ///

```

## E29 — Authorized action token pattern.

`crates/axon-cortex/src/runner.rs:109-127`

```text

  109  /// Proof that a specific action passed authorization.
  110  ///
  111  /// The field is private and there is no public constructor, so the only way to
  112  /// hold one is to have been given it by [`Runner::authorize_action`]. That is
  113  /// what makes `execute` unable to run an unauthorized action: not a check
  114  /// inside execute that could be forgotten or reordered, but a value the caller
  115  /// cannot produce without passing the gate.
  116  ///
  117  /// It borrows the action rather than copying it, so the thing executed is
  118  /// necessarily the thing authorized — a copy could drift between the two calls.
  119  #[derive(Debug)]
  120  pub struct Authorized<'a> {
  121      action: &'a CortexAction,
  122  }
  123  
  124  impl<'a> Authorized<'a> {
  125      pub fn action(&self) -> &'a CortexAction {
  126          self.action
  127      }

```

## E30 — Deterministic episode name is not a unique trial ID.

`crates/axon-cortex/src/runner.rs:615-624`

```text

  615          // The episode NAMES this run. Every Runner was built with the same
  616          // constant, so the id could not tell two repairs apart — which is what
  617          // an id is for. Derived from the target and the adjudicator, so it is
  618          // deterministic (a replay of the same request produces the same id)
  619          // and distinguishing (a different target or a different grader does
  620          // not).
  621          self.episode.episode_id = format!(
  622              "cortex-repair:{}:{}:{}",
  623              target.path, target.symbol, hidden_check
  624          );

```

## E31 — Direct test subprocess and mixed output result parsing.

`crates/axon-cortex/src/runner.rs:1237-1317`

```text

 1237      /// Run `axon test` and read its MACHINE-READABLE output.
 1238      ///
 1239      /// This used to parse the human transcript, and every shape that transcript
 1240      /// can take which the parser did not expect was a silent wrong answer
 1241      /// rather than an error:
 1242      ///
 1243      /// * `test NAME [should_fail] ... FAILED` — the annotation is printed ONLY
 1244      ///   on the failure branch, so the name came out as `NAME [should_fail]`,
 1245      ///   the `== hidden` comparison never matched, and the ADJUDICATING check
 1246      ///   was fed to localization as ordinary evidence.
 1247      /// * the classifier looked for `FAILED` before `ok` anywhere in the line,
 1248      ///   so a PASSING test named `test_FAILED_path` was recorded as failed.
 1249      /// * test bodies run in-process and their stdout is not captured, so a
 1250      ///   `print` without a trailing newline glues the next result line to it
 1251      ///   and that test vanishes from the run.
 1252      ///
 1253      /// Each of those is a different bug with one cause: the transcript is for
 1254      /// people, and its shape may change for reasons that have nothing to do
 1255      /// with this caller. `--json` is the contract meant to be parsed. A test
 1256      /// body could still print a line that happens to be a matching JSON
 1257      /// object; nothing here can prevent that, and it is a far narrower target
 1258      /// than a line beginning with "test ".
 1259      ///
 1260      /// Returns `(failed, passed, total)`. `total` comes from the run's own
 1261      /// summary, so "the filter matched nothing" is distinguishable from "every
 1262      /// test passed" — those are byte-identical in the human transcript, and
 1263      /// both exit 0.
 1264      fn run_tests_json(
 1265          &self,
 1266          rel_path: &str,
 1267          filter: Option<&str>,
 1268      ) -> std::io::Result<(Vec<String>, Vec<String>, usize)> {
 1269          let mut cmd = std::process::Command::new(&self.axon_bin);
 1270          cmd.arg("test")
 1271              .arg(self.workspace.join(rel_path))
 1272              .arg("--json");
 1273          if let Some(f) = filter {
 1274              cmd.arg("--filter").arg(f);
 1275          }
 1276          let out = cmd.output()?;
 1277          let text = format!(
 1278              "{}{}",
 1279              String::from_utf8_lossy(&out.stdout),
 1280              String::from_utf8_lossy(&out.stderr)
 1281          );
 1282          let (mut failed, mut passed, mut total) = (Vec::new(), Vec::new(), None);
 1283          for line in text.lines() {
 1284              let line = line.trim();
 1285              if !line.starts_with('{') {
 1286                  continue;
 1287              }
 1288              let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
 1289                  continue;
 1290              };
 1291              if v.get("type").and_then(|t| t.as_str()) == Some("summary") {
 1292                  total = v.get("total").and_then(|t| t.as_u64()).map(|t| t as usize);
 1293                  continue;
 1294              }
 1295              let (Some(name), Some(status)) = (
 1296                  v.get("name").and_then(|n| n.as_str()),
 1297                  v.get("status").and_then(|s| s.as_str()),
 1298              ) else {
 1299                  continue;
 1300              };
 1301              match status {
 1302                  "ok" => passed.push(name.to_string()),
 1303                  "failed" => failed.push(name.to_string()),
 1304                  // An unknown status is neither. Guessing which it resembles is
 1305                  // how a new status becomes a silent wrong answer.
 1306                  _ => {}
 1307              }
 1308          }
 1309          // No summary means the run did not finish — a compile error, a crash,
 1310          // a missing binary. That is an ERROR, never "no tests failed".
 1311          let Some(total) = total else {
 1312              return Err(std::io::Error::other(format!(
 1313                  "`axon test --json` produced no summary for {rel_path}; the run \
 1314                   did not complete, which is not the same as nothing failing"
 1315              )));
 1316          };
 1317          Ok((failed, passed, total))

```

## E32 — Fixture copy is top-level regular files only.

`crates/axon-cortex/src/runner.rs:1517-1528`

```text

 1517      /// Prepare a resettable COPY of a fixture directory. The original is never
 1518      /// the thing edited.
 1519      pub fn stage_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
 1520          std::fs::create_dir_all(dst)?;
 1521          for e in std::fs::read_dir(src)? {
 1522              let e = e?;
 1523              if e.file_type()?.is_file() {
 1524                  std::fs::copy(e.path(), dst.join(e.file_name()))?;
 1525              }
 1526          }
 1527          Ok(())
 1528      }

```

## E33 — Existing H0/H1/H2/K confinement and recovery requirements.

`docs/axon_cortex_v0_15/axon-cortex-build-v0_15/specs/CX-13-os-runtime.md:13-69`

```text

   13  ## Intent and source basis
   14  
   15  Provide the operating-system responsibilities needed for safe long-running cognition: process authority, isolation, scheduling, persistence, cancellation and recovery. S1 pp.16, 62–63 and 84–88 documents hosted services and unresolved kernel/VM scope. See [SOURCES](../SOURCES.md).
   16  
   17  ## Decisive fork
   18  
   19  Adopt staged host tiers. Cortex v0 depends on a tested hosted executor, not an Axon-authored kernel. Native test/build artifacts still run in external confinement; interpreter capability checks do not automatically restrict arbitrary native subprocesses.
   20  
   21  ## Tiered roadmap
   22  
   23  Tier H0: one supported host, single-writer isolated workspaces, no production effects, supervisor-managed model/tool jobs. Tier H1: durable multi-job service, quotas, revocation, observability, recovery and separately approved remote adapters. Tier H2: parity/equivalent-contract host portability, optional microVM and hardware-attestation integration. Tier K: separately approved bare-metal branch with syscall/confinement evidence. Tier K is not an implied deliverable of H0–H2.
   24  
   25  Support status is per host/profile and version. A missing sandbox feature refuses real tool execution; a documented mock can run non-consequential fixtures but never pass a production-confinement gate.
   26  
   27  ## Supervisor and job contract
   28  
   29  The trusted parent owns grants, credentials, policy, immutable artifact store, required-gate execution and job state. Untrusted model/generated-code workers receive only a projected input, scoped handles and an ephemeral filesystem view. The parent must not load arbitrary worker code into its own address space.
   30  
   31  `JobManifest` binds executable/artifact, principal, input snapshot, working directory, resource reservations, permitted mounts, secret/environment projection, egress policy, deadline, cancellation channel and expected output schema. Restrict descendants as well as the initial process. A sandbox that limits the command string but lets descendants access the host is insufficient.
   32  
   33  Reserve aggregate CPU/memory/GPU/time/token/cost/storage budgets where supported. Report estimated versus actual model usage and reconcile with available provider receipts; an estimate alone cannot guarantee exact monetary spending. Hard local deadlines and conservative call/output limits remain enforcement tools. Backpressure bounds queued work and abandoned speculative inference.
   34  
   35  ## Confinement and secrets
   36  
   37  No worker sees the parent's verifier/admission credentials. The environment is an allowlist projection, not inherited ambient state. Mount only permitted content, make protected artifacts read-only, and prevent escape through symlinks, replacement executables, inherited descriptors and shared caches.
   38  
   39  Network-denied workloads must be denied at the actual host boundary. For later scoped egress, validate the chosen enforcement mechanism against names, resolved destinations, redirects, proxy behavior and credential forwarding. Do not infer effective network controls from an annotation or an untested container flag.
   40  
   41  Source/build dependencies are code. Dependency fetching and builds need separate policies, pinning and isolated artifacts. The initial release uses approved offline fixtures; live package installation is not silently authorized by RunBuild.
   42  
   43  ## Cancellation and recovery
   44  
   45  Cancel propagates from trusted supervisor to all child processes/model requests where possible, prevents new dispatch, records possibly ongoing remote work, and never clears a latched stop without the approved lifecycle. Worker acknowledgement alone is not evidence that it stopped. Test actual termination and resource release under the configured bound.
   46  
   47  Restart reconstructs durable jobs/actions and reconciles OutcomeUnknown; it does not replay effects as a recovery strategy. A worker cannot modify the kill latch, supervisor binary or root policy. S1 distinguishes supervisor kill behavior from interpreter flags; preserve that distinction.
   48  
   49  ## Attestation and portability
   50  
   51  Claims identify whether evidence is simulated, local hash/HMAC, or hardware-rooted, and exactly which loaded components/artifacts are covered. Do not call a digest proof that the runtime remained uncompromised or that a decision was correct. Reuse R26–R34 only after their target-specific gates are reproduced.
   52  
   53  For each added engine/host, run a contract matrix: isolation, effects, cancellation, replay, resource limits and evidence. Equivalent user-visible guarantees matter more than identical host implementation. Unsupported cases refuse explicitly.
   54  
   55  ## Acceptance gates
   56  
   57  **G13-escape:** adversarial build/test fixtures try host reads/writes, unapproved subprocesses, network access, environment secrets and protected evaluator paths. Allowed operations succeed; denied operations produce no unauthorized realized effect.
   58  
   59  **G13-kill:** stuck process trees and slow inference are canceled under the declared bound; no further dispatch or resumable self-clear occurs.
   60  
   61  **G13-quota:** concurrent child jobs cannot oversubscribe carved quotas or evade them through restart.
   62  
   63  **G13-recovery:** parent/worker crashes at action boundaries produce safe reconciliation and no blind external retry.
   64  
   65  **G13-tier:** unsupported/native profiles cannot inherit interpreter-only guarantees; simulated attestation cannot satisfy a hardware-required profile.
   66  
   67  ## Build slices and exclusions
   68  
   69  Implement H0 before untrusted test execution in M1, then operational H1. Desktop UI, drivers, general kernel replacement, multi-VM consensus and hosted commercial service packaging are separate decisions.

```

## E34 — Server-side authority, immutable transactions, OutcomeUnknown.

`docs/axon_cortex_v0_15/axon-cortex-build-v0_15/specs/CX-03-capabilities-executor.md:13-63`

```text

   13  ## Intent and source basis
   14  
   15  Turn selected semantic actions into bounded effects without allowing model output to create authority. S2 pp.18–33 motivates opaque observed targets and freshness checks; S1 pp.12, 16 and 48 limits the guarantees available from the existing interpreter/FFI stack. See [SOURCES](../SOURCES.md).
   16  
   17  ## Decisive fork
   18  
   19  Choose a trusted server-side grant registry and immutable workspace transactions. Do not treat enum membership, a signed-looking string, or a path prefix as authorization.
   20  
   21  ## Capability contract
   22  
   23  A grant stores grant ID, issuer, principal/session, action kind, snapshot/epoch, allowed target set or namespace, read/write scopes, payload constraints, preconditions, expiry, invocation and resource bounds, and revocation state. The model sees a reference and readable description. The reference is meaningful only after trusted registry lookup. Child grants can attenuate parent authority and carved budgets, never enlarge them.
   24  
   25  The selected action is a tagged union: Inspect(target), Search(scope), ProposePatch(targets, artifact), RunCheck(check_id), Revert(local_checkpoint), RequestContext(scope), Escalate(reason), DoneClaim(evidence_refs), or Blocked(reason). Additional action types require a manifest and recovery semantics. DONE has no execution authority.
   26  
   27  A registered check maps to trusted executor configuration: program/entrypoint, argument schema, working directory, sandbox profile, effect permissions, environment/secret projection, and budgets. Arguments are arrays/typed values, never a shell command string. A generated test/build script is untrusted executable content even when invoked through a registered check.
   28  
   29  ## Payload and freshness validation
   30  
   31  Generated patches are data. Parse/apply them only to the approved workspace; reject out-of-scope paths, traversal, symlink escapes, secret destinations and forbidden manifest changes. Validate new files through an explicitly permitted namespace. An inspected target can be valid yet semantically wrong: task correctness remains the verifier's job.
   32  
   33  Prepare on an immutable snapshot. Before commit, validate expected content hashes and authority under a workspace lock or equivalent atomic compare-and-swap. Include dirty/untracked inputs and dependency/environment fingerprints. Avoid a check-then-write gap. Concurrent actions need compatible read/write sets and an atomic commit order; v0 is single-writer.
   34  
   35  ## Durable action state machine
   36  
   37  `Proposed → Prepared → Validated → Running → Observed → Verified` is the normal path. Terminal alternatives are Refused, Failed, Canceled, or OutcomeUnknown. “Observed” means effect outcome recorded; “Verified” means the action/task contract was checked, not that every goal is complete.
   38  
   39  Persist action ID and preparation evidence before effects. The execution attempt has an idempotency key and the host adapter declares at-most-once, deduplicated retry, or reconciliation-required semantics. A crash between an external effect and receipt is OutcomeUnknown. Reconcile against the external system or require operator review; never blindly rerun a non-idempotent effect.
   40  
   41  Local patch rollback restores an immutable checkpoint and invalidates descendant artifacts/observations. It does not undo external calls or previously disclosed secrets. A test may generate local artifacts; include them in observed deltas and delete only inside its owned ephemeral workspace.
   42  
   43  ## Threat model and host dependency
   44  
   45  Cover cross-session handle replay, principal impersonation, grant expiry, budget reuse, TOCTOU, symlinks/hardlinks, executable/tool replacement, compiler plugin/build-script execution and a worker attempting to mutate verifier assets. Where network is allowed later, destinations and redirects are executor policy, not model-provided permission. Strong host confinement is CX-13; unsafe host profiles remain disabled for real execution.
   46  
   47  ## Acceptance gates
   48  
   49  **G03-forgery:** unknown, expired, wrong-principal and previous-snapshot grants all refuse without a side effect.
   50  
   51  **G03-payload:** a valid Edit grant paired with a traversal, symlink or policy-file patch refuses; a permitted ordinary patch succeeds.
   52  
   53  **G03-race:** mutate input between prepare and commit; exactly one compatible local transaction can commit and the stale one must reobserve.
   54  
   55  **G03-crash:** inject crashes at every durable boundary. Recovery never blindly duplicates an external effect and identifies an unknown outcome.
   56  
   57  **G03-budget:** nested child actions and speculative requests cannot exceed the parent's reserved aggregate budget; concurrent debits are atomic.
   58  
   59  **G03-done:** a DONE claim with failing hidden checks cannot close the task, regardless of confidence or agent explanation.
   60  
   61  ## Build slices and exclusions
   62  
   63  Build denial cases first, local snapshot patching second, isolated checks third. No generic shell tool, automatic production deployment, irreversible external effect, multi-agent concurrent writer, or capability self-minting in v0.

```

## E35 — Narrow host seam; avoid arbitrary exec and premature crate splits.

`docs/axon_cortex_v0_15/axon-cortex-build-v0_15/specs/CX-04-air-runtime.md:37-55`

```text

   37  ## Graph contract
   38  
   39  An AIR artifact contains schema version, content digest, node IDs/kinds, typed inputs/outputs, data dependencies, control predicates, capability requirements, resource limits, model/provider selectors, timeout/retry policies, and observation/artifact lineage. Acyclic per-iteration graphs are the initial representation. Repetition belongs to a bounded task controller with explicit stop conditions, not hidden graph recursion.
   40  
   41  Static validation checks unknown references, cycles, type mismatches, missing branch fields, authorization requirements, effect escalation, unbounded iteration, and invalid speculative regions. Runtime checks bind dynamic targets and payloads to actual grants/snapshots. A field needed after another field's resolution has a real dependency edge; it is not independent merely because a model can answer both.
   42  
   43  Rule functions claiming determinism cannot call an effectful provider. Returned uncertainty and unavailable evidence must be represented in the output type. No implicit empty artifact, default success, guessed distribution or hidden provider fallback is permitted.
   44  
   45  ## Scheduling and execution
   46  
   47  Start with a deterministic serial scheduler. Parallelization is an optimization over nodes whose effects and dependencies permit it. Record order of observed completions and cancellation events. Speculative Decide outputs may be computed; Act and untrusted tool effects are never speculative in v0.
   48  
   49  Budget reservations happen before dispatch and reconcile actual usage when known. Queue deadlines, timeouts, bounded retries and backpressure propagate to children. Refusal, failure, abstention and cancellation are distinct outcomes. Provider fallback is a new explicit dispatch recorded with the effective model/schema/calibration versions.
   50  
   51  ## Axon integration
   52  
   53  Reuse Axon typed programs for deterministic transformations, metrics and scoped checks. Add a narrow host seam rather than routing arbitrary AIR operations through `exec`. Logical modules may initially live together behind interfaces; physical crate splits should follow measured build/test boundaries.
   54  
   55  R44 sessions cannot carry arbitrary live handles across cells. Store stable artifact references in the Cortex controller; reacquire validated handles per call. Do not rebuild a parallel type inference engine. Native lowering is deferred; unsupported cognitive operations refuse explicitly rather than silently run without interpreter safety.

```

## E36 — Capability registry does not bypass evidence admission.

`docs/axon_cortex_v0_15/axon-cortex-build-v0_15/specs/CX-34-cognitive-scheduler-capability-registry.md:120-133`

```text

  120  ## 8. Capability lifecycle
  121  
  122  The registry accepts artifacts only through existing admission paths. CX-22 can propose specialization, CX-31 can propose shared capabilities, and CX-18 can propose native promotion, but registry publication never bypasses CX-11 evidence/admission. Superseded or drifted capabilities can be quarantined, scope-narrowed or rolled back.
  123  
  124  ## 9. Integration
  125  
  126  - CX-06 provides calibration/risk-aware routing signals.
  127  - CX-13 exposes runtime resource/budget state.
  128  - CX-22/CX-23 supply specialized and neural-program capabilities.
  129  - CX-24/CX-26 supply semantic perception/retrieval and composition capabilities.
  130  - CX-28 provides working-set/cache context.
  131  - CX-31 contributes transfer-scoped shared capabilities.
  132  - CX-32 links registry artifacts and scheduler decisions to evidence.
  133  

```
