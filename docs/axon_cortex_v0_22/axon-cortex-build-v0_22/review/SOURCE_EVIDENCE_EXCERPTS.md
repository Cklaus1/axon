# Source evidence excerpts — v0.21

These excerpts are generated directly from the supplied source snapshot. Line numbers and SHA256 bind the observation to that file version; they do not prove runtime execution. The archive does not establish the current live Git revision.

## SRC01 — `Cargo.toml`

Lines 25–37; SHA256 `bdbac15c9d8087ad1de2c236660804cc16e1a69f83c3b5a61431db2e02def1d6`.

```text
0025 ]
0026 resolver = "2"
0027 
0028 [workspace.package]
0029 version = "0.1.0"
0030 edition = "2021"
0031 authors = ["cklaus"]
0032 license = "MIT"
0033 
0034 [workspace.dependencies]
0035 logos = "0.14"
0036 serde = { version = "1", features = ["derive"] }
0037 serde_json = "1"
```

## SRC02 — `crates/axon-reflex/src/lib.rs`

Lines 42–50; SHA256 `92cd5835658e6940a29f1d868b94ab53e47692be64b5aa83666c2603d0f2d9e7`.

```text
0042 
0043 /// Wire/protocol version. A client and a backend that disagree must refuse
0044 /// rather than guess at a shape.
0045 pub const PROTOCOL: &str = "axon-reflex/1";
0046 
0047 /// Maximum bytes in one request or response frame.
0048 ///
0049 /// Matches `cortex-policy-adapter`'s `MAX_REQUEST`. Nothing bounded a payload
0050 /// before: the sidecar's line reader and the HTTP body reader both grew without
```

## SRC03 — `crates/axon-reflex/src/lib.rs`

Lines 166–191; SHA256 `92cd5835658e6940a29f1d868b94ab53e47692be64b5aa83666c2603d0f2d9e7`.

```text
0166 
0167 /// The backend-neutral, mode-neutral surface.
0168 pub trait ReflexBackend {
0169     fn mode(&self) -> Mode;
0170     fn encode_state(&mut self, input: &str, scope: &PrincipalScope)
0171         -> Result<StateHandle, Refusal>;
0172     fn decide(
0173         &mut self,
0174         handle: &StateHandle,
0175         question: &str,
0176         scope: &PrincipalScope,
0177     ) -> Result<Decision, Refusal>;
0178     fn release_state(
0179         &mut self,
0180         handle: &StateHandle,
0181         scope: &PrincipalScope,
0182     ) -> Result<(), Refusal>;
0183 }
0184 
0185 // ── the shared decision core ────────────────────────────────────────────────
0186 //
0187 // ONE implementation of the invariant, used by all three modes: embedded calls
0188 // it directly, the sidecar binary calls it, and the remote test server calls
0189 // it. Three copies of an authority check is three places for it to drift, and
0190 // the engine-parity work that motivated this crate is a long record of exactly
0191 // that drift.
```

## SRC04 — `crates/axon-reflex/src/lib.rs`

Lines 201–217; SHA256 `92cd5835658e6940a29f1d868b94ab53e47692be64b5aa83666c2603d0f2d9e7`.

```text
0201     pub fn new() -> Self {
0202         Self::default()
0203     }
0204 
0205     pub fn encode_state(&mut self, _input: &str, scope: &PrincipalScope) -> StateHandle {
0206         self.next += 1;
0207         let id = format!("st-{}", self.next);
0208         self.states.insert(id.clone(), scope.principal.clone());
0209         StateHandle {
0210             id,
0211             principal: scope.principal.clone(),
0212         }
0213     }
0214 
0215     /// The invariant, in one place.
0216     ///
0217     /// Existence is checked before ownership. An earlier comment here claimed
```

## SRC05 — `crates/axon-reflex/src/lib.rs`

Lines 234–249; SHA256 `92cd5835658e6940a29f1d868b94ab53e47692be64b5aa83666c2603d0f2d9e7`.

```text
0234         &mut self,
0235         id: &str,
0236         question: &str,
0237         scope: &PrincipalScope,
0238     ) -> Result<Decision, Refusal> {
0239         self.authorize(id, &scope.principal)?;
0240         // Deterministic by construction: a fixed function of the question.
0241         // Phase 1 is about the SEAM, not about inference quality, and a
0242         // stochastic stub would make the cross-mode comparison meaningless.
0243         Ok(Decision {
0244             choice: format!("decided({question})"),
0245             principal: scope.principal.clone(),
0246         })
0247     }
0248 
0249     pub fn release_state(&mut self, id: &str, scope: &PrincipalScope) -> Result<(), Refusal> {
```

## SRC06 — `crates/axon-reflex/src/lib.rs`

Lines 119–164; SHA256 `92cd5835658e6940a29f1d868b94ab53e47692be64b5aa83666c2603d0f2d9e7`.

```text
0119 /// Why a request was not served. Closed, so the set of things a backend may
0120 /// refuse is enumerable rather than open-ended.
0121 #[derive(Debug, Clone, PartialEq, Eq)]
0122 pub enum Refusal {
0123     /// The caller is not the principal the handle belongs to.
0124     ///
0125     /// Distinct from `UnknownState` ON PURPOSE. This says "it exists and is
0126     /// not yours"; that says "there is nothing here". Collapsing them would
0127     /// turn an authority violation into a retryable miss.
0128     CrossPrincipal { owner: String, caller: String },
0129     /// No such handle — a genuine miss, safe to re-encode.
0130     UnknownState { id: String },
0131     /// The backend cannot honour a declared control in this mode. Stated at
0132     /// the boundary rather than degraded quietly.
0133     Unsupported { control: String, mode: &'static str },
0134     /// The transport failed. An infrastructure failure is NOT a decision and
0135     /// NOT an abstention.
0136     Transport(String),
0137     /// Protocol mismatch between client and backend.
0138     Protocol(String),
0139 }
0140 
0141 impl std::fmt::Display for Refusal {
0142     fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
0143         match self {
0144             Refusal::CrossPrincipal { owner, caller } => write!(
0145                 f,
0146                 "cross-principal reuse refused: state belongs to `{owner}`, caller is `{caller}` \
0147                  — this is a refusal, not a cache miss"
0148             ),
0149             Refusal::UnknownState { id } => write!(f, "no such state handle: {id}"),
0150             Refusal::Unsupported { control, mode } => {
0151                 write!(f, "control `{control}` is not supported in {mode} mode")
0152             }
0153             Refusal::Transport(e) => write!(f, "transport failure: {e}"),
0154             Refusal::Protocol(e) => write!(f, "protocol: {e}"),
0155         }
0156     }
0157 }
0158 
0159 /// A decision. Carries the principal it was decided under, so an audit reading
0160 /// the response alone can attribute it.
0161 #[derive(Debug, Clone, PartialEq, Eq)]
0162 pub struct Decision {
0163     pub choice: String,
0164     pub principal: String,
```

## SRC07 — `crates/axon-cortex/src/select.rs`

Lines 108–144; SHA256 `1068dd7e11cb9861388774396f9bae154e4514d40e54ca8c216fa0c76c24b51b`.

```text
0108 }
0109 
0110 /// Selection with what the loop has learned so far.
0111 pub fn select_action_with(
0112     obs: &Observation,
0113     target: &SymbolRef,
0114     ctx: &SelectionContext,
0115 ) -> Selection {
0116     let compiles = match fact(obs, "compiles") {
0117         Some(Observed::Known { value }) => value == "true",
0118         Some(Observed::Unknown { reason }) => {
0119             return Selection::Blocked(format!(
0120                 "cannot choose an action: `compiles` is unknown ({reason})"
0121             ))
0122         }
0123         None => {
0124             return Selection::Blocked(
0125                 "cannot choose an action: the observation carries no `compiles` fact".to_string(),
0126             )
0127         }
0128     };
0129 
0130     if !compiles {
0131         return Selection::Act(CortexAction::Inspect {
0132             target: target.clone(),
0133         });
0134     }
0135 
0136     // It compiles. Whether it compiles CLEANLY decides between confirming
0137     // behaviour and claiming completion — which is exactly why the observer
0138     // carries warnings separately instead of reporting "no errors" as "nothing
0139     // to report".
0140     // It compiles, and a claim of completion has already been refused: the
0141     // program is syntactically fine and semantically wrong. This is the one
0142     // state where a repair is warranted, and the body is left EMPTY on purpose
0143     // — the selector does not invent it. The loop reports NeedsInput and a
0144     // generator fills it, keeping the creative step outside the control loop.
```

## SRC08 — `crates/axon-cortex/src/generate.rs`

Lines 123–146; SHA256 `623854e6968bc8cb3b95515f2794d1b134ec3fce6359039bba2e15a1892165e7`.

```text
0123 }
0124 
0125 /// Supplies the one thing the control loop cannot.
0126 pub trait PatchGenerator {
0127     /// Stable identity of this generator — model name and version, or the name
0128     /// of a deterministic strategy. Recorded in the episode, so two runs of the
0129     /// same loop with different generators are distinguishable afterwards.
0130     fn id(&self) -> String;
0131 
0132     /// Propose a body for `target`, given what was observed.
0133     ///
0134     /// Receives the observation rather than the file, so a generator sees what
0135     /// Cortex saw — including the omissions and the Unknown facts. Handing it
0136     /// the raw workspace would make it a second observer with a different view,
0137     /// and then a disagreement between them would have no arbiter.
0138     fn propose(
0139         &self,
0140         observation: &Observation,
0141         target: &SymbolRef,
0142         constraints: &PatchConstraints,
0143     ) -> Result<ProposedPatch, GenerationFailure>;
0144 }
0145 
0146 /// Check a proposal against its constraints.
```

## SRC09 — `crates/axon-cortex/src/runner.rs`

Lines 100–111; SHA256 `3f970015a52590d79805654ceb0f1e0b25ee0425c9ca55661b2dbd169e232e68`.

```text
0100 /// function already reasons in path prefixes, so this reuses the idiom instead
0101 /// of adding a second matching rule. Cortex repairs `.ax` symbols; these trees
0102 /// are policy and evidence surfaces, not repair targets.
0103 const POLICY_PREFIXES: &[&str] = &["scripts/", "governance/", ".github/"];
0104 
0105 /// Exact paths that are never writable — evidence artifacts that live at the
0106 /// repository root and so have no protecting prefix.
0107 const POLICY_PATHS: &[&str] = &["AXON-COMPLETENESS.json", "AXON-COMPLETENESS.md"];
0108 
0109 /// Proof that a specific action passed authorization.
0110 ///
0111 /// The field is private and there is no public constructor, so the only way to
```

## SRC10 — `crates/axon-cortex/src/runner.rs`

Lines 450–477; SHA256 `3f970015a52590d79805654ceb0f1e0b25ee0425c9ca55661b2dbd169e232e68`.

```text
0450     /// nonsense combinations the string API could express are not decided here
0451     /// — they cannot be built. `ClaimDone` carries no path, so no path check is
0452     /// skipped for it; there is nothing to skip.
0453     pub fn authorize_action<'a>(
0454         &mut self,
0455         action: &'a CortexAction,
0456         grant: Option<&EditGrant>,
0457         principal: &str,
0458         current_snapshot: &WorkspaceSnapshot,
0459     ) -> Result<Authorized<'a>, Refusal> {
0460         let r = self.check_typed_authority(action, grant, principal, current_snapshot);
0461         match r {
0462             // BOTH halves recorded. `ActionDenied`'s own doc claims it is
0463             // "recorded with the same weight as an allowed one", and the
0464             // weight was zero on one side: `ActionAllowed` was defined,
0465             // documented, and never constructed, so an episode could show what
0466             // it was stopped from doing and never what it was permitted to do.
0467             // `grant_id` was written at three production sites and read
0468             // nowhere — this is the field it existed for.
0469             Ok(()) => {
0470                 self.episode.push(EpisodeEvent::ActionAllowed {
0471                     action: action.name().to_string(),
0472                     // An action needing no grant records the ABSENCE rather
0473                     // than borrowing a name: "no grant was required" and "a
0474                     // grant authorised this" are different facts.
0475                     grant_id: grant
0476                         .map(|g| g.grant_id.clone())
0477                         .unwrap_or_else(|| "<none required>".to_string()),
```

## SRC11 — `crates/axon-cortex/src/runner.rs`

Lines 597–630; SHA256 `3f970015a52590d79805654ceb0f1e0b25ee0425c9ca55661b2dbd169e232e68`.

```text
0597     ///
0598     /// Conflating them is how a loop that is stuck gets read as a loop that was
0599     /// merely rushed.
0600     pub fn run_episode(
0601         &mut self,
0602         target: &crate::action::SymbolRef,
0603         grant: Option<&EditGrant>,
0604         principal: &str,
0605         hidden_check: &str,
0606         budget: usize,
0607         generator: Option<&dyn crate::generate::PatchGenerator>,
0608     ) -> EpisodeOutcome {
0609         // Every (workspace state, action) pair already tried. A SET, not just
0610         // the previous step: the loop alternates patch → claim → patch, so a
0611         // last-step comparison cannot see a two-step cycle and the episode
0612         // would spin until the budget ran out — reporting "out of budget" for
0613         // what is really "going in circles".
0614         let mut seen: Vec<(String, String, usize)> = Vec::new();
0615         // The episode NAMES this run. Every Runner was built with the same
0616         // constant, so the id could not tell two repairs apart — which is what
0617         // an id is for. Derived from the target and the adjudicator, so it is
0618         // deterministic (a replay of the same request produces the same id)
0619         // and distinguishing (a different target or a different grader does
0620         // not).
0621         self.episode.episode_id = format!(
0622             "cortex-repair:{}:{}:{}",
0623             target.path, target.symbol, hidden_check
0624         );
0625         let mut ctx = crate::select::SelectionContext::default();
0626         // Attempts that did not stick, and the body most recently applied.
0627         // Carried across steps so the generator is not asked the same question
0628         // with no record of what its last answer was.
0629         // The grant, with its state-pin advanced across transitions THIS
0630         // episode performed. Everything that confers authority — the principal,
```

## SRC12 — `crates/axon-cortex/src/episode.rs`

Lines 42–67; SHA256 `2e0061d1c4f9a062622bbd1f287df382dff07601c63a50960a14d274cf3f64b6`.

```text
0042     ///
0043     /// A PROCESS RAN. Nothing else belongs in this variant. Two events used to
0044     /// be pushed here that were not checks at all — a candidate's own claim of
0045     /// completion, and the identity of whichever generator proposed a patch —
0046     /// each with `exit_code: 0, passed: true` invented, because the variant had
0047     /// no other shape to carry them. `Claimed` and `Proposed` exist so that is
0048     /// no longer necessary.
0049     CheckRun {
0050         name: String,
0051         exit_code: i32,
0052         passed: bool,
0053     },
0054     /// The candidate asserted it is finished. NOT evidence that it is.
0055     ///
0056     /// Carries no exit code and no `passed`, deliberately: nothing ran, so
0057     /// there is nothing to report an outcome for. `verify()` adjudicates the
0058     /// claim against a check the candidate cannot modify, and that adjudication
0059     /// is a separate `Verified` event.
0060     ///
0061     /// This used to be pushed as `CheckRun { name: "claim_done", exit_code: 0,
0062     /// passed: claim.done }` — a self-report rendered as a check that ran and
0063     /// returned success. The crate's own first rule says a missing fact cannot
0064     /// be silently rendered as a present one; this is that rule applied to the
0065     /// crate's own evidence trail.
0066     Claimed { rationale: String },
0067     /// WHO proposed a patch, recorded before it is applied.
```

## SRC13 — `crates/axon-cortex/src/episode.rs`

Lines 165–185; SHA256 `2e0061d1c4f9a062622bbd1f287df382dff07601c63a50960a14d274cf3f64b6`.

```text
0165     /// The contract this restores is stated by the crate's own test
0166     /// `cli_ships_evidence_a_reader_can_verify`: "its own verdict agrees with
0167     /// the exit code — two independent readers of the same run must not be able
0168     /// to disagree."
0169     pub fn verified_ok(&self) -> bool {
0170         self.events
0171             .iter()
0172             .rev()
0173             .find_map(|e| match e {
0174                 EpisodeEvent::Verified { passed, .. } => Some(*passed),
0175                 _ => None,
0176             })
0177             .unwrap_or(false)
0178     }
0179 
0180     /// Every action the catalog refused. Reviewers read this first.
0181     pub fn denials(&self) -> Vec<&str> {
0182         self.events
0183             .iter()
0184             .filter_map(|e| match e {
0185                 EpisodeEvent::ActionDenied { action, .. } => Some(action.as_str()),
```

## SRC14 — `crates/axon-cortex/src/episode.rs`

Lines 118–139; SHA256 `2e0061d1c4f9a062622bbd1f287df382dff07601c63a50960a14d274cf3f64b6`.

```text
0118     /// Order is part of the identity: the same events in a different sequence
0119     /// are a different episode, because "checked then patched" and "patched
0120     /// then checked" are different claims about what was verified.
0121     pub fn digest(&self) -> Result<String, ContractError> {
0122         // The ID is covered. It is documented as "identity of the whole
0123         // episode" and hashed only the events, so the one field that could
0124         // distinguish two runs was outside the thing that attests them — and
0125         // every Runner was constructed with the same constant id besides, so
0126         // two repairs of different files with coincidentally identical event
0127         // sequences were indistinguishable by both.
0128         let body = crate::to_canonical_json(&(&self.episode_id, &self.events))?;
0129         Ok(content_digest(body.as_bytes()))
0130     }
0131 
0132     /// Did an independent verifier pass this episode?
0133     ///
0134     /// THE LAST `Verified` event is the verdict. Absent verification is NOT
0135     /// success — an episode with no `Verified` event returns `false`, the same
0136     /// absent-vs-passed rule the contracts enforce for observations.
0137     ///
0138     /// This used to be `.any(|e| matches!(e, Verified { passed: true, .. }))`,
0139     /// which reported an episode verified when an EARLIER, SUPERSEDED check had
```

## SRC15 — `crates/cortex-policy-adapter/src/main.rs`

Lines 257–286; SHA256 `d0f315d416e14632c14a2a8ff7fe28c9257fc0faf831de2deeda651812859587`.

```text
0257     // `typed` is bound OUTSIDE the match so the action outlives the decision.
0258     // That is the Authorized witness doing its job rather than an inconvenience:
0259     // it borrows the action, so the thing authorized cannot be dropped while a
0260     // proof of its authorization is still held, and cannot be swapped for a
0261     // copy that drifted.
0262     let decision = match &typed {
0263         Ok(a) => runner
0264             .authorize_action(a, Some(&grant), req_principal, &current)
0265             .map(|_authorized| ()),
0266         Err(refusal) => Err(refusal.clone()),
0267     };
0268 
0269     let out = match decision {
0270         // Mapped to () above: this adapter DECIDES, it does not execute. That
0271         // boundary is the point — Cortex answers "may I?", the caller acts.
0272         // WHAT WAS CHECKED, not merely the verdict.
0273         //
0274         // Cortex answers Ok immediately for any action that needs no write
0275         // authority, BEFORE the principal, staleness, traversal and
0276         // policy-file checks. So `inspect` with a principal the grant does not
0277         // belong to, a snapshot that never existed, and a `..` path returned
0278         // the same four bytes as a granted write — measured. "The grant
0279         // authorised this" and "no authority question was asked" were the same
0280         // token, which is this crate's own absent-vs-passed collapse sitting
0281         // in its security boundary.
0282         Ok(()) => serde_json::json!({
0283             "protocol_version": PROTOCOL_VERSION,
0284             "decision": "allow",
0285             "basis": if typed.as_ref().map(|a| a.requires_write_authority()).unwrap_or(false) {
0286                 "granted: the principal, the grant's state and the path were all checked"
```

## SRC16 — `crates/axon-ai/src/lib.rs`

Lines 567–588; SHA256 `92c2dcdd9d9bd35ac549ede1673d6ebda582283601c569605800b9beae0bdb96`.

```text
0567 /// interpreter uses this on a LIVE call so the cost reflects what the model
0568 /// actually charged, not a prompt-length estimate. Under AXON_AI_MOCK the count
0569 /// is the same deterministic estimate the pre-dispatch meter uses (reproducible).
0570 pub fn complete_with_model_usage(prompt: &str, model: &str) -> Result<(String, i64), String> {
0571     ai_complete_inner_model_usage(prompt, model)
0572 }
0573 
0574 // -- Layer-3 ASI: typed structured-output ("Uncertain<T>") ---------------------
0575 //
0576 // These helpers force Anthropic to return its answer through a tool-use call
0577 // whose JSON schema demands `{value, confidence}`.  This makes the LLM's
0578 // self-reported confidence cross into Axon's type system as `Uncertain<T>`,
0579 // so user code can branch on `.confidence > threshold`.
0580 
0581 /// Build the Anthropic Messages API request body for typed structured output.
0582 ///
0583 /// `value_type` should be `"integer"` for i64 or `"number"` for f64; the rest
0584 /// of the schema is identical.  Exposed as `pub` so unit tests can pin the
0585 /// JSON shape without going over the wire.
0586 pub fn build_typed_uncertain_body(prompt: &str, value_type: &str) -> serde_json::Value {
0587     serde_json::json!({
0588         "model": "claude-sonnet-4-6",
```

## SRC17 — `crates/axon-core/src/kernel.rs`

Lines 686–731; SHA256 `3bb71267bf53535441bc51de7b215e957947d639f0cddfed50e77480e0f25ae6`.

```text
0686 /// budget, with a graceful fallback + latch on overrun. `rate_micro` is µ$ per
0687 /// 1000 tokens (integer µ$ keeps the arithmetic exact + tests deterministic).
0688 #[derive(Debug, Clone)]
0689 pub struct LlmGateway {
0690     pub model: String,
0691     pub rate_micro: i64,
0692     /// The principal whose budget bounds this gateway's spend (Slice 1 handle).
0693     /// The OWNER's handle token (T42: a token, not an index — see
0694     /// [`PrincipalRegistry`]). Stored so budget debits hit the right principal.
0695     pub principal: i64,
0696     pub fallback: String,
0697     pub halted: bool,
0698     /// µ$ spent through THIS gateway (for observability; the authoritative cap is
0699     /// the principal's budget).
0700     pub spent_micro: i64,
0701 }
0702 
0703 impl LlmGateway {
0704     pub fn new(model: String, rate_micro: i64, principal: i64, fallback: String) -> Self {
0705         LlmGateway {
0706             model,
0707             rate_micro,
0708             principal,
0709             fallback,
0710             halted: false,
0711             spent_micro: 0,
0712         }
0713     }
0714 
0715     /// µ$ cost of a call of `tokens` tokens (rate is per 1000 tokens). Negative
0716     /// token counts clamp to 0.
0717     pub fn call_cost(&self, tokens: i64) -> i64 {
0718         let t = tokens.max(0);
0719         self.rate_micro * t / 1000
0720     }
0721 }
0722 
0723 /// R12b: a principal-scoped, budgeted objective runner. A `KernelGoal` records
0724 /// the `@[adaptive]` metric `name` and `target` to optimize, the owning Slice-1
0725 /// `principal` (whose `Budget` bounds total spend), how many evaluations have
0726 /// been charged so far (`evals_spent`), and the best score observed. The interp
0727 /// runs the EXISTING optimizer (`run_goal`) for `min(requested, budget_remaining)`
0728 /// evaluations, debits the principal's budget, and refuses to exceed it (E1604,
0729 /// exit 7). Authority (Slice 1) and spend are ONE model — a goal can never spend
0730 /// beyond its principal's grant. See governance/specs/R12b-kernel-goal.md.
0731 #[derive(Debug, Clone)]
```

## SRC18 — `crates/axon-core/src/replay.rs`

Lines 483–504; SHA256 `0e37658160f5bbef237b519e65e2ab351c2a89bc996b698359605a285dade230`.

```text
0483 // ── ReplayHost ───────────────────────────────────────────────────────────────
0484 
0485 /// Serves host calls from a recorded journal, in order, performing nothing.
0486 pub struct ReplayHost {
0487     events: Vec<HostEvent>,
0488     cursor: AtomicUsize,
0489 }
0490 
0491 impl ReplayHost {
0492     pub fn new(events: Vec<HostEvent>) -> Self {
0493         Self {
0494             events,
0495             cursor: AtomicUsize::new(0),
0496         }
0497     }
0498 
0499     pub fn from_path(path: &std::path::Path) -> Result<Self, String> {
0500         Ok(Self::new(read_journal(path)?))
0501     }
0502 
0503     /// How many recorded events were not consumed. A replay that stops early is
0504     /// as much a divergence as one that asks for too much — it just cannot be
```

## SRC19 — `scripts/cortex_package_gate.sh`

Lines 2–13; SHA256 `61b23c11afbf934452b71a4fed4f26bb1702c6ed13ce85a79cb51daa549f94b5`.

```text
0002 # cortex_package_gate.sh — continuous check for the vendored Cortex v0.15 build package.
0003 #
0004 # WHY THIS EXISTS.
0005 #
0006 # `docs/axon_cortex_v0_15/axon-cortex-build-v0_15/` is a 114-file documentation
0007 # package: 35 specs, 152 work packages and 247 PROPOSED product gates. Every one
0008 # of those 247 carries `"product_result": "NOT_RUN"` and
0009 # `"implementation_status": "Not implemented in this package"`, and every work
0010 # package is `"status": "Not started"`. The package is scrupulous about saying so.
0011 #
0012 # That honesty is the property most easily destroyed on intake. This repository
0013 # already has the defect at smaller scale: 7 of the 37 `scripts/*.sh` cited as
```

## SRC20 — `scripts/cortex_package_gate.sh`

Lines 16–28; SHA256 `61b23c11afbf934452b71a4fed4f26bb1702c6ed13ce85a79cb51daa549f94b5`.

```text
0016 # gate IDs, each one edit away from reading as executed, would multiply it.
0017 #
0018 # So this gate asserts two things on every run:
0019 #
0020 #   1. INTEGRITY. `SHA256SUMS_v0_15.json` (schema `cortex-package-sha256/1`) is
0021 #      checked in BOTH directions -- every listed file hashes to its recorded
0022 #      digest, AND no unlisted file has appeared under the package root. One
0023 #      direction alone would miss a file being ADDED. The manifest itself is the
0024 #      single legitimate unlisted file (it cannot contain its own hash).
0025 #      The package's own `tools/validate_package.py` is also run -- it is a real
0026 #      check that, before this script, ran nowhere: the same orphaned-verification
0027 #      class described above.
0028 #
```

## SRC21 — `scripts/cortex_package_gate.sh`

Lines 111–122; SHA256 `61b23c11afbf934452b71a4fed4f26bb1702c6ed13ce85a79cb51daa549f94b5`.

```text
0111 # rather than a data file so that adding one requires a reviewable code edit.
0112 # Removing it means re-vendoring the file with its upstream bytes.
0113 # Deliberately EMPTY. A deviation was briefly needed here: the intake run of
0114 # `tools/validate_package.py` defaults `--report` to <root>/package_validation.json,
0115 # which is itself hash-listed, so verifying the package MUTATED it and the
0116 # mutated copy was committed. The cause is fixed in two places instead — the
0117 # pristine bytes are restored, and this gate passes `--report` to target/ so the
0118 # footgun cannot re-fire — which leaves this map with nothing legitimate to
0119 # hold. It stays as a named, reviewable mechanism rather than an implicit one:
0120 # a future deviation must be added here with a reason, not tolerated silently.
0121 DEVIATIONS: dict[str, tuple[str, str]] = {}
0122 
```

## SRC22 — `scripts/cortex_package_gate.sh`

Lines 174–184; SHA256 `61b23c11afbf934452b71a4fed4f26bb1702c6ed13ce85a79cb51daa549f94b5`.

```text
0174 r = json.load(open(sys.argv[1], encoding="utf-8"))
0175 if r.get("result") != "PASS":
0176     print("  validator result:", r.get("result"), r.get("errors")); sys.exit(1)
0177 c = r.get("counts", {})
0178 if not c or min(c.values(), default=0) <= 0:
0179     print(f"  NON-VACUITY: validator counted nothing: {c}"); sys.exit(1)
0180 print(f"  validator PASS; counts={c}; product_gates_executed={r.get('product_gates_executed')}")
0181 PY
0182 [ $? -eq 0 ] || note_fail "package validator self-report"
0183 
0184 # The validator must not have written into the package. Re-hash the one file it
```

## SRC23 — `scripts/cortex_package_gate.sh`

Lines 187–201; SHA256 `61b23c11afbf934452b71a4fed4f26bb1702c6ed13ce85a79cb51daa549f94b5`.

```text
0187 import hashlib, json, os, sys
0188 pkg, sums = sys.argv[1], sys.argv[2]
0189 rel = "package_validation.json"
0190 try:
0191     want = json.load(open(sums, encoding="utf-8"))["files"][rel]
0192 except (OSError, ValueError, KeyError) as e:
0193     print(f"  cannot read the recorded digest for {rel}: {e}"); sys.exit(1)
0194 # Same pinned deviation as the integrity pass: what matters here is that the file
0195 # did not change ACROSS the validator run, so compare against whichever digest the
0196 # integrity pass already accepted.
0197 PINNED = "cb2deaf7a7fb64da94d3fca150690715aa6e3b5325b649857ffb03297c01e421"
0198 got = hashlib.sha256(open(os.path.join(pkg, rel), "rb").read()).hexdigest()
0199 if got not in (want, PINNED):
0200     print(f"  {rel} changed while the validator ran (it wrote into the vendored package)"); sys.exit(1)
0201 PY
```

## SRC24 — `crates/axon-cortex/benchmarks/real_model.py`

Lines 169–182; SHA256 `e5e2fa9e68386005442c0f49fa6557ccc52dd2c85792c02157ad46883138addb`.

```text
0169                 "n": c["proposal_number"],
0170                 "priors": c["prior_rejections"],
0171                 "input": c["input_tokens"],
0172                 "output": c["output_tokens"],
0173                 "cache_create": c["cache_create"],
0174                 "cache_read": c["cache_read"],
0175                 "cost_usd": c["cost_usd"],
0176                 "api_ms": c["duration_api_ms"],
0177             }
0178             for c in calls
0179         ],
0180         "cost_usd": round(sum(c["cost_usd"] or 0 for c in calls), 4),
0181         "api_ms": sum(c["duration_api_ms"] or 0 for c in calls),
0182         "wall_s": round(wall, 1),
```

## SRC25 — `governance/cortex-v015/CX-35-reflex-serving.md`

Lines 11–25; SHA256 `33dba9877983b2ce989f07db3da38fdae7f590b89b0744f18378e9ca1f276b5b`.

```text
0011 # CX-35 — Reflex Serving and Model-State Runtime
0012 
0013 ## 0. What this document is, and where it lives
0014 
0015 This is a **new CX spec written in the live repository**, at
0016 `governance/cortex-v015/CX-35-reflex-serving.md`. It is deliberately NOT placed
0017 inside the vendored package, because `scripts/cortex_package_gate.sh` verifies
0018 `SHA256SUMS_v0_15.json` **in both directions** — "every listed file hashes to
0019 its recorded digest, AND no unlisted file has appeared under the package root"
0020 (`scripts/cortex_package_gate.sh:22-27`). A CX-35 written under
0021 `docs/axon_cortex_v0_15/axon-cortex-build-v0_15/specs/` would break the intake
0022 gate on its first run. It follows that:
0023 
0024 * `specs/INDEX.md` and `spec_manifest.json` inside the package cannot be
0025   updated to list CX-35. The package's spec set stops at CX-34
```

## SRC26 — `governance/cortex_gate_execution_registry.json`

Lines 9–16; SHA256 `f56d80d7278c296543619d7d5b6f8300cda25fadca9b4aefcd7a6245dbcd00f3`.

```text
0009     "5. If a future upstream release re-vendors the package WITH results filled in, the honesty check will accept a non-NOT_RUN product_result only for gates that have a valid row here, and only when the result is in allowed_product_result.",
0010     "Removing a gate's implementation means removing its row."
0011   ],
0012   "gates": [],
0013   "tasks": [],
0014   "tasks_note": "A work package status other than 'Not started' needs a row in `tasks` with task_id and allowed_status. A status in {Done, Complete, Completed, Landed} additionally requires every one of that task's gate_targets to have a valid row in `gates` -- so 'Done' cannot launder an unexecuted gate.",
0015   "checked_by": "scripts/cortex_package_gate.sh"
0016 }
```
