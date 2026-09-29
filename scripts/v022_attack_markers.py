"""Per-row ATTACK MARKERS for scripts/v022_g01_mutations.py (amendment 39).

A mutation row counts as KILLED only when the panic that FAILS its test (the
last panic on the test's thread, whole message) matches the row's marker: a
Python regex, searched with re.S. The marker states the row's OWN attack
getting through (an attack accepted, counted, signed, launched, passed), never
a refusal reason, which would also appear when a different check still
refuses the attack. A failure that does not match is REFUSED_ELSEWHERE:
reported separately and never counted killed.

Kept apart from the MUTATIONS table so the tuple shape is unchanged. The
mutation run refuses to start unless EVERY active row has a marker and every
marker names a row (drift check, both directions). No marker uses a line
number: other workstreams move test lines.

Derived C9 round 1 from each row's killing test's own assertion and audited
against the recorded kill evidence of the a3db33bd two-shard run
(bind-c9dev/mut-merged.json). Rows whose recorded kill was NOT their attack
are marked `# WEAK@a3db33bd` with the finding; see the round's report.
"""

# A weak row whose test has not (yet) been given an attack assertion: the
# test's "ATTACK:" message is the only thing that can count as its kill.
ATTACK = r"ATTACK:"

ATTACK_MARKERS = {
    # C9 round 1b (fabric): only-guard: the decision's own contract (lib test)
    'M01': r'ATTACK: a replayed receipt was signed',
    'M02': 'must be refused',
    'M03': 'altered, still verified',
    'M04': r"ATTACK: the candidate's helper\.ax judged its own broken double",
    'M05': 'unwrap_err\\(\\)` on an `Ok` value',
    'M06': 'unwrap_err\\(\\)` on an `Ok` value',
    'M07': 'unwrap_err\\(\\)` on an `Ok` value',
    'M08': 'corrupted signature: IntakeOutcome',
    'M09': "another trial's evidence: IntakeOutcome",
    'M10': 'unwrap_err\\(\\)` on an `Ok` value: IntakeOutcome',
    'M11': 'ArmResult .*left: [1-9]\\d*\\s+right: 0',
    'M12': 'unwrap_err\\(\\)` on an `Ok` value: PointerRecord',
    'M13': 'ArmResult .*left: [1-9]\\d*\\s+right: 0',
    'M14': 'a counted verdict cites',
    'M15': 'ArmResult .*left: [1-9]\\d*\\s+right: 0',
    'M16': 'TrialResult .*left: [1-9]\\d*\\s+right: 0',
    'M17': 'unwrap_err\\(\\)` on an `Ok` value',
    'M18': 'unwrap_err\\(\\)` on an `Ok` value',
    # C9 round 1b (LOOP): EQUIVALENT with M428 (verify_execution's backend
    # join); this is the four-cell attack's own marker.
    'M19': r'ATTACK: an attested execution on a development backend was counted as protected',
    # C9 round 1b (LOOP): only-guard route = a CITED unknown from a development
    # verification backend (M213's protected-evidence check never runs for it).
    'M20': r"ATTACK: a development-backend verification decided a protected trial's unknown\s+kind",
    'M21': 'a failed verdict on another tree: IntakeOutcome',
    'M22': 'worker_reported: IntakeOutcome',
    'M23': 'two suite versions: IntakeOutcome',
    # WEAK@a3db33bd: refused by 'matched_checks differs from the check receipt's' where the test wants 'verifier_ref': reason mismatch
    'M24': r"ATTACK: intake verified a receipt other than the one the sidecar's verifier_ref cites",
    'M25': 'extra evidence ref: IntakeOutcome',
    'M26': r'ATTACK: another compute profile: the verdict was ACCEPTED',
    # WEAK@a3db33bd: reason mismatch: refused by 'rubric: the receipt does not record exactly one check suite version' (M23's guard); reviewer: subsumed by intake.rs:916 argv[0]==check:{suite}
    'M27': r'ATTACK: a bare candidate file defined the acceptance rubric',
    # WEAK@a3db33bd: reason mismatch: refused by the acceptance argv join ('not task task-1's registered acceptance check'), not the pin
    'M28': r'ATTACK: a verdict on a suite not pinned for its verifier was ACCEPTED',
    # WEAK@a3db33bd: reason mismatch: refused by the acceptance join; implied by recorded == acc.check_suite
    'M29': r'ATTACK: a verdict recorded for a suite the request did not name',
    'M30': 'requester-chosen test: IntakeOutcome',
    'M31': 'not a registered check: IntakeOutcome',
    'M32': 'receipt for another operation: IntakeOutcome',
    'M33': 'receipt input differs from request: IntakeOutcome',
    'M34': 'sidecar names another verified tree: IntakeOutcome',
    'M35': 'sidecar upgrades a failed check to passed: IntakeOutcome',
    'M36': 'matched_checks inflated: IntakeOutcome',
    'M37': 'NS4p: proposer self-observed',
    'M38': 'matches!\\(evaluate\\(&w\\.s, &v\\), Err\\(LoopError::Refused\\(_\\)\\)\\)',
    'M39': 'NS4p: proposer self-observed',
    'M40': 'left: Ok\\(\\(\\)\\)\\s+right: Err\\(\\"not a registered_check',
    'M41': 'left: Ok\\(\\(\\)\\)\\s+right: Err\\(\\"the check is a file of the candidate\'s tree',
    'M42': 'left: Ok\\(\\(\\)\\)\\s+right: Err\\(\\"the admitted grant gives the check workload',
    'M43': "the receipt names the suite's ENTRY file too",
    'M44': 'left: String\\(\\"(unknown|failed)\\"\\)',
    'M45': "execution refs are the check's own documents: IntakeOutcome",
    'M46': 'check ran as the subject: IntakeOutcome',
    'M47': r'ATTACK: another verifier revision: the verdict was ACCEPTED',
    'M48': 'left: None\\s+right: Some\\(',
    'M49': 'left: Passed\\s+right: Unknown',
    'M50': 'unwrap_err\\(\\)` on an `Ok` value: PointerRecord',
    'M51': 'unwrap_err\\(\\)` on an `Ok` value: Some\\(Ref',
    # C9 round 1b (fabric): only-guard: the candidate is published under the colon state dir
    'M52': r"ATTACK: a check ran under a state dir containing ':'",
    'M53': 'a module from the trial cache judged the candidate.*left: Passed',
    'M54': 'unwrap_err\\(\\)` on an `Ok` value',
    'M55': 'the check workload copied the signing key',
    'M57': 'unwrap_err\\(\\)` on an `Ok` value: Submission',
    'M59': 'these escapes passed the test',
    # WEAK@a3db33bd: e.len()==1 but the message is M69's 'method ... defined more than once': the duplicate impl was still refused by the dispatch check
    # core r1b: EQUIVALENT_DID vs M69; its own attack (the joint cell) is the no-op second impl running.
    'M60': r"ATTACK: a second `impl J for E` replaced the operator's `check`: Ok\(Completed\)",
    'M61': 'failed: let: \\[\\]',
    'M62': 'failed: refinement: \\[\\]',
    'M63': 'failed: exit\\(0\\): .*left: true\\s+right: false',
    'M64': 'failed: forged ok line: .*left: true\\s+right: false',
    'M65': 't_err: Ok\\(Completed\\)',
    'M67': 't_exit: Ok\\(Completed\\)',
    'M68': 'was tagged as completed',
    'M69': 'failed: same method from a second trait: \\[\\]',
    'M70': 'failed: trait: \\[\\]',
    'M71': "the check wrote into the launcher's cwd",
    'M72': '\\): Ok\\(Completed\\)',
    'M73': 'resume[^\\n]*: Ok\\(Completed\\)',
    # UNCLEAR@a3db33bd: bare assert_eq end(t_handler)==Ok(Completed); with the mutation v becomes 107 (swallowed completion), which IS the attack, but t_inf/t_honest share the same assertion shape so the message cannot tell
    'M74': r'ATTACK: a handler completion was caught by a `with` that did not install it',
    'M75': 'failed: refinement named after a builtin type: \\[\\]',
    'M76': 'NaN passed an f64 assertion',
    'M77': 'an exit\\(0\\) property case passed',
    'M78': 'expected exactly one E0002\\s+left: 0',
    'M79': 'mutate a suite constant: \\[\\]',
    'M80': 'reads the answer key: .*left: true\\s+right: false',
    'M81': 'reads the answer key: .*left: true\\s+right: false',
    'M82': 'failed: refinement named after a generic parameter: \\[\\]',
    # WEAK@a3db33bd: an honest two-file merge got a spurious E0002 '__refine_0 defined more than once': the collision is refused by the duplicate-refinement check (M62), a false refusal not a shadowing attack
    # core r1b: killed on the collision itself, observed at the parser before any check runs.
    'M83': r"ATTACK: the candidate's inline refinement took the operator's synthetic name `__refine_\d+`",
    'M84': 'unwrap_err\\(\\)` on an `Ok` value: CheckReport',
    'M85': 'the ambient module resolved',
    'M86': 'direct call: Ok\\(Completed\\)',
    'M87': 'global read: Ok\\(Completed\\)',
    'M88': 'refinement attaching to an operator annotation: Ok\\(Completed\\)',
    # WEAK@a3db33bd: out is Err('assertion failed: 0 != 42'): the by-name call did not deliver the operator's answer (another mechanism, likely the per-provenance kernel M96 or a run-time seal, stopped it); the candidate's own assert failed
    # core r1b: EQUIVALENT_DID vs {M86, M96}; the joint cell's attack is an operator fn run through a fiber.
    'M89': r"ATTACK: (a sealed frame ran the operator's `expected` as a fiber and returned its answer|the operator's scheduler ran an operator function a sealed frame queued): Ok\(Completed\)",
    'M90': 'a candidate closure called by the operator: Ok\\(Completed\\)',
    'M91': 'a candidate closure called by the operator: Ok\\(Completed\\)',
    'M92': 'a candidate handler arm: Ok\\(Completed\\)',
    'M93': 'match guard: \\[\\]',
    'M94': 'failed: refinement named after a deferred-prefix type: \\[\\]',
    'M95': 'module-level initializer: Ok\\(Completed\\)',
    'M96': "reads the operator's fiber result by id: Ok\\(Completed\\)",
    'M97': "a candidate struct's where, built by the operator: Ok\\(Completed\\)",
    'M98': 'a pre-freeze verdict counted',
    'M99': 'failed: unsigned: \\[TrialResult',
    'M100': 'an episode with another input workspace was recorded',
    'M101': 'issued the \\w+: Ok\\(',
    'M102': 'designated the incumbent-of-record: Ok\\(',
    'M103': 're-derived on a withdrawn pin: Ok\\(',
    'M104': 'a forged attribution was admitted',
    'M105': 'activated on withdrawn authority: Ok\\(',
    'M106': 'stated without its execution: Some\\(Known',
    'M107': 'independently verified[^\\n]*\\n\\s*left: \\(VerifiedPass',
    # C9 round 1b (LOOP): index panic made a structured refusal; only-guard route = the post-hoc trial requested but undelivered.
    'M108': r'ATTACK: a population chosen after outcomes was evaluated \(issued c0 dropped\): \(EvaluationRecord',
    'M109': 'preflighted before its issue was evaluated: Ok\\(',
    'M110': 'issued a population over a recorded outcome: Ok\\(',
    'M111': 'issued the population: Ok\\(',
    'M112': 'must refuse: Ref\\(',
    'M113': 'admitted: Ok\\(',
    # C9 round 1b (LOOP): only-guard route = a store-written record that does not list the proposer as a subject.
    'M114': r'ATTACK: the proposer admitted its own candidate: Accept',
    'M115': 'a shared-key config was read back: Ok\\(',
    'M116': 'left: 0\\s*\\n\\s*right: 900000',
    # C9 round 1b (LOOP): only-guard case = two FINAL currencies, no liability, report-only economics.
    'M117': r'ATTACK: a two-currency arm was decided as known: Accept',
    'M118': 'panicked at [^\\n]*\\nindependently verified \\(\\d+ checks\\)(\\n|$)',
    'M119': 'rolled back to a baseline whose issuer is now Fabric: Ok\\(',
    'M120': 'activated a baseline whose issuer is now Fabric: Ok\\(',
    'M121': 'Fabric revoked: Ok\\(',
    'M122': 'activated on a withdrawn observer: Ok\\(',
    'M123': 'c0: independently verified[^\\n]*\\n\\s*left: \\(VerifiedPass',
    'M124': 'c1: verifier reported failure[^\\n]*\\n\\s*left: \\(Fail',
    'M125': 'left: [^\\n]*\\n\\s*right: Some\\(TimedOut\\)',
    'M126': 'c7: [^\\n]*\\n\\s*left: [^\\n]*\\n\\s*right: \\(Unknown, Some\\(Cancelled\\)\\)',
    'M127': 'left: [^\\n]*\\n\\s*right: Some\\(Unmatched\\)',
    'M128': 'c3: [^\\n]*\\n\\s*left: [^\\n]*\\n\\s*right: \\(Unknown, Some\\(Cancelled\\)\\)',
    'M129': 'carried execution documents: Ok\\(',
    'M130': 'D12 local execution[^\\n]*\\n\\s*left: \\(Unknown, Some\\(Unbound\\)\\)\\s*\\n\\s*right: \\(Unknown, Some\\(Unverifiable\\)\\)',
    'M131': 'verifier reported failure[^\\n]*\\n\\s*left: \\(Fail',
    'M132': 'panicked at [^\\n]*\\n\\[\\](\\n|$)',
    'M133': 'c0: [^\\n]*\\n\\s*left: [^\\n]*\\n\\s*right: \\(Unknown, Some\\(TimedOut\\)\\)',
    'M134': 'c2: [^\\n]*\\n\\s*left: [^\\n]*\\n\\s*right: \\(Unknown, Some\\(MissingEvidence\\)\\)',
    'M135': 'c3: [^\\n]*\\n\\s*left: [^\\n]*\\n\\s*right: \\(Unknown, Some\\(Unverifiable\\)\\)',
    'M136': 'c5: [^\\n]*\\n\\s*left: [^\\n]*\\n\\s*right: \\(Unknown, Some\\(Cancelled\\)\\)',
    'M137': 'unwrap_err\\(\\)` on an `Ok` value: LinuxQualification',
    # C9 round 1b (fabric): only-guard: a full valid dev submit; nothing else reads these flags
    'M138': r'ATTACK: --[a-z-]+ was accepted: a submit ran with a caller protected-profile flag',
    # C9 round 1b (fabric): EQUIVALENT (four-cell with M141)
    'M139': r"ATTACK: the caller's --check-registry was loaded as the suite registry",
    # WEAK@a3db33bd: refused by the D1 grant check 'pins no grant_registry' (protected_host.rs), signer never reached
    'M140': r'ATTACK: a (group-readable host signer key|host signer key that does not derive its pin) was not refused',
    # C9 round 1b (fabric): EQUIVALENT (four-cell with M139); re-anchored mutation
    'M141': r"ATTACK: the caller's --check-registry was loaded as the suite registry",
    'M142': 'unwrap_err\\(\\)` on an `Ok` value: ProtectedHost',
    'M143': 'unwrap_err\\(\\)` on an `Ok` value: ProtectedHost',
    'M144': 'unwrap_err\\(\\)` on an `Ok` value: ProtectedHost',
    'M145': 'unwrap_err\\(\\)` on an `Ok` value: ProtectedHost',
    'M146': 'unwrap_err\\(\\)` on an `Ok` value: ProtectedHost',
    'M147': 'unwrap_err\\(\\)` on an `Ok` value: ProtectedHost',
    'M148': 'unwrap_err\\(\\)` on an `Ok` value: ProtectedHost',
    'M149': r'assertion `left != right` failed: (ATTACK: certified PASS despite the attack: )?\{[^\n]*"status":"PASS"',
    'M150': r'ATTACK: certified PASS despite the attack: a dev-mode trust preflight',
    'M151': r'ATTACK: certified PASS despite the attack: a trust preflight that is no certified evidence file',
    # C9 r1b psv: EQUIVALENT_DID (four-cell vs M153); the attack is an UNRELABELLED cross-domain signature
    'M152': 'ATTACK: a \\w+ signature, its domain field unchanged, was accepted as \\w+',
    'M153': 'relabelled as [^\\n]*"verified":true',
    'M154': 'unwrap_err\\(\\)` on an `Ok` value: LaunchManifest',
    'M155': 'unwrap_err\\(\\)` on an `Ok` value: LaunchManifest',
    'M156': 'unwrap_err\\(\\)` on an `Ok` value: LaunchManifest \\{[^\\n]*backend_profile: "local-interpreter"',
    'M157': 'unwrap_err\\(\\)` on an `Ok` value: LaunchManifest \\{[^\\n]*scheme: "none"',
    'M158': 'unwrap_err\\(\\)` on an `Ok` value: LaunchManifest \\{ schema: "axon-launch-manifest/0"',
    'M159': 'unwrap_err\\(\\)` on an `Ok` value: InputCheck',
    'M160': 'unwrap_err\\(\\)` on an `Ok` value: InputCheck',
    'M161': 'left: Object \\{(?:(?!"attempt_id":)[^\\n])*\\n\\s*right: Object \\{[^\\n]*"attempt_id":',
    'M162': 'left: Object \\{(?:(?!"test":)[^\\n])*\\n\\s*right: Object \\{[^\\n]*"test":',
    'M163': 'did not move the key',
    'M164': 'left: Object \\{(?:(?!"candidate_tree_digest":)[^\\n])*\\n\\s*right: Object \\{[^\\n]*"candidate_tree_digest":',
    'M165': 'did not move the key',
    'M166': 'right: "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"',
    'M167': 'left: (Passed|Failed|Unknown)\\s*\\n\\s*right: Refused',
    'M168': 'left: (Passed|Failed|Unknown)\\s*\\n\\s*right: Refused',
    'M169': 'failed: "(t_ok --json|-x|t_\\*|)"\\s*\\n\\s*left: (Passed|Failed|Unknown)',
    # C9 r1b psv: the post-guard copy is total (no expect), so the length guard is the only check
    'M170': 'ATTACK: a \\d+-byte completion secret was accepted and the test ran',
    'M171': 'stdin was not at EOF: STDIN-SAW:\\[[^\\]]',
    # C9 r1b psv: the fixture secret is printable, so the uid drop is the only guard
    'M172': 'ATTACK: candidate code read the completion secret',
    'M173': 'failed: substring\\s*\\n\\s*left: Passed',
    'M174': 'left: Passed\\s*\\n\\s*right: Unknown',
    'M175': 'left: Passed\\s*\\n\\s*right: Unknown',
    'M177': 'left: (Failed|Unknown|Refused|NotRun)\\s*\\n\\s*right: Passed',
    'M178': 'left: Passed\\s*\\n\\s*right: Passed',
    'M179': 'failed: other-manifest\\s*\\n\\s*left: (Passed|Failed)',
    # C9 r1b psv: a Refused verdict over a genuine pass; nothing after the check reads the status
    'M180': 'ATTACK: the guest refused, and the Fabric counted its run as Passed',
    'M181': 'failed: inputs\\s*\\n\\s*left: (Passed|Failed)',
    'M182': 'failed: stdout\\s*\\n\\s*left: (Passed|Failed)',
    # C9 round 1b (fabric): EQUIVALENT (four-cell with M312)
    'M183': r"ATTACK: a previous attempt's genuine pass was counted",
    'M184': 'left: (Passed|Failed)\\s*\\n\\s*right: Unknown',
    # C9 round 1b (fabric): only-guard: the verdict moved off Failed
    'M185': r"ATTACK: the guest's claim of a pass steered a failing test's verdict.*\n\s*left: (Unknown|Passed|NotRun)",
    'M186': 'left: "protected"\\s*\\n\\s*right: "guest-unobserved"',
    # C9 round 1b (fabric): EQUIVALENT (four-cell with M400); the expect panic is now a structured refusal
    'M187': r'ATTACK: a candidate file was launched as a check on the protected profile',
    'M188': 'left: Passed\\s*\\n\\s*right: Passed',
    'M189': 'left: ""\\s*\\n\\s*right: "development"',
    'M190': 'linux-microvm-protected "" development\\s*\\n\\s*left: Ok',
    # C9 round 1b (fabric): only-guard: the changed program still signs a valid observation
    'M191': r'ATTACK: an unpinned observer program ran and its observation authorized a launch',
    'M192': 'failed: op-obs--qualification-observer\\s*\\n\\s*left: (?!NotRun)',
    'M193': 'failed: op-obs-claims-other-key-observer-observer\\s*\\n\\s*left: (?!NotRun)',
    'M194': 'failed: op-obs-epoch-observer-observer\\s*\\n\\s*left: (?!NotRun)',
    'M195': 'failed: op-obs-stale-observer-observer\\s*\\n\\s*left: (?!NotRun)',
    'M196': r"ATTACK: a verified observation's nonce was not spent",
    'M197': 'unwrap_err\\(\\)` on an `Ok` value: \\(\\)',
    'M198': 'unwrap_err\\(\\)` on an `Ok` value: \\(\\)',
    # C9 round 1b (fabric): only-guard: a record planted at the traversal target
    'M199': r'ATTACK: a path-shaped nonce was consumed from outside the custodian',
    'M200': 'failed: op-obs-other-manifest-observer-observer\\s*\\n\\s*left: (?!NotRun)',
    # WEAK@a3db33bd: refused by the custodian 'nonce ... was never issued', not the join
    'M201': r'failed: op-obs-nonce-issued-elsewhere-observer-observer\s*\n\s*left: (?!NotRun)',
    'M202': 'failed: op-obs-kernel-observer-observer\\s*\\n\\s*left: (?!NotRun)',
    'M203': 'left: "guest-unobserved"\\s*\\n\\s*right: "protected"',
    # WEAK@a3db33bd: refused 'bundle presented for a receipt that does not claim protected evidence' (another check)
    # C9 round 1: M204 re-anchored ACTIVE on the current observe seam. A defective
    # observation launched and yielded a Passed verification (reviewer M204re.log).
    'M204': r'left: Passed\s+right: NotRun',
    'M205': r'ATTACK: a protected claim was authenticated by a store-planted verifier key',
    'M206': 'unwrap_err\\(\\)` on an `Ok` value: IntakeOutcome',
    'M207': 'outcome: VerifiedPass.*left: [1-9]\\d*\\s*\\n\\s*right: 0',
    'M208': 'outcome: VerifiedPass.*left: [1-9]\\d*\\s*\\n\\s*right: 0',
    # C9 round 1b (LOOP): EQUIVALENT, four-cell vs M360 / M361 (joint cell = this marker).
    'M209': r'ATTACK: a forged attribution was admitted: Accept',
    'M210': r'ATTACK: a forged attribution was admitted: Accept',
    'M211': 'TrialResult.*\\n\\s*left: [1-9]\\d*\\n\\s*right: 0',
    'M212': 'TrialResult.*\\n\\s*left: [1-9]\\d*\\n\\s*right: 0',
    'M213': r'ATTACK: development backend: ACCEPTED',
    # WEAK@a3db33bd: Failing panic is the reason assert ('no observation: refused: ... does not join: <other join>'); the claim was still refused.
    'M214': r'ATTACK: no observation: ACCEPTED',
    # WEAK@a3db33bd: Failing panic is the reason assert ('another interpreter: refused: ... does not join: <other join>'); the claim was still refused.
    'M215': r'a guest that ran an unpinned interpreter, every document consistent: ACCEPTED',
    # WEAK@a3db33bd: Failing panic is the reason assert ('duplicated verdict: refused: ... does not join: <other join>'); the claim was still refused.
    'M216': r'ATTACK: duplicated verdict: ACCEPTED',
    # WEAK@a3db33bd: Failing panic is the reason assert ('malformed digest: refused: ... does not join: <other join>'); the claim was still refused.
    'M217': r'a malformed kernel digest every document agrees on: ACCEPTED',
    'M218': r'ATTACK: [^\n]*: ACCEPTED',
    'M219': '\\n\\[[^\\n]*\\"t_cand_probe\\"[^\\n]*\\]',
    'M220': 'failed: GuestVerdict \\{.*\\n\\s*left: (?!Passed)\\w+\\n\\s*right: Passed',
    # C9 round 1b (fabric): only-guard: the verdict moved off Failed
    'M221': r"ATTACK: a run dir swapped under the caller's state changed the verdict.*\n\s*left: (Unknown|Passed|NotRun)",
    'M222': 'unwrap_err\\(\\)` on an `Ok` value: LaunchManifest',
    'M223': 'unwrap_err\\(\\)` on an `Ok` value: LaunchManifest',
    'M224': 'unwrap_err\\(\\)` on an `Ok` value: InputCheck',
    # C9 r1b psv: the manifest digest names the tree WITH the link, so the digest join agrees
    'M225': 'ATTACK: a candidate holding a symlink was accepted under a digest naming it',
    'M226': 'the guest-path receipt parses as a contract: .*matched_checks',
    'M227': 'failed: op-obs-verifier-[\\w-]+\\n\\s*left: (?!NotRun)\\w+\\n\\s*right: NotRun',
    'M228': 'assertion `left == right` failed\\n\\s*left: \\"yes\\"\\n\\s*right: \\"no\\"',
    'M229': 'the secret outlived the launch into the verify step',
    'M230': 'manifest\\ for\\ another\\ trial: ACCEPTED',
    'M231': 'unwrap_err\\(\\)` on an `Ok` value: IntakeOutcome',
    'M232': 'receipt\\ names\\ another\\ launch\\ manifest: ACCEPTED',
    'M233': "observation\\ ref\\ is\\ the\\ manifest\\ ref\\ \\(the\\ reviewer's\\ repro\\): ACCEPTED",
    'M234': 'observation\\ claims\\ another\\ observer: ACCEPTED',
    'M235': 'observation\\ of\\ another\\ manifest: ACCEPTED',
    'M236': 'manifest\\ for\\ another\\ trial: ACCEPTED',
    'M237': 'manifest\\ for\\ another\\ suite\\ version: ACCEPTED',
    'M238': 'unwrap_err\\(\\)` on an `Ok` value: IntakeOutcome',
    'M239': 'failed: forge-fail\\n\\s*left: (?!Unknown)\\w+\\n\\s*right: Unknown',
    'M240': '\\n\\s*left: Failed\\n\\s*right: Unknown',
    'M241': 'failed: swap-after\\n\\s*left: (?!Unknown)\\w+\\n\\s*right: Unknown',
    'M242': 'keyed=false: GuestVerdict.*\\n\\s*left: Failed\\n\\s*right: Unknown',
    'M243': '\\"status\\":\\"failed\\"\\}\\n\\s*left: Some\\(\\"[0-9a-f]{64}\\"\\)\\n\\s*right: Some\\(\\"[0-9a-f]{64}\\"\\)',
    'M244': 'unwrap_err\\(\\)` on an `Ok` value: \\(SafetyReport',
    'M246': 'assertion `left == right` failed\\n\\s*left: \\"OPENED\\"\\n\\s*right: \\"DENIED\\"',
    'M247': 'Some\\(\\"IO,Exec\\"\\): .*SPAWNED',
    'M248': 'None: .*SPAWNED',
    'M249': 'CEIL:\\[[^\\]]*Exec',
    'M250': '(?m)^[\\w-]+: \\{[^\\n]*\\"status\\":\\"ok\\"',
    # WEAK@a3db33bd: Failing panic is err.contains("may not supply it") with EMPTY stderr; line 471 (no ok status) held, so the candidate copy never became the rubric.
    # core r1b: only-guard route = the operator's library (~/.axon/lib), searched after the candidate.
    'M251': r'ATTACK: a sealed module\'s copy of the operator library\'s `rubric` defined the rubric: [^\n]*"status":"ok"',
    'M252': 'observation\\ of\\ another\\ authority\\ epoch: ACCEPTED',
    'M253': '\\n\\s*left: Passed\\n\\s*right: Passed',
    'M254': '\\n\\s*left: Accept\\n\\s*right: Accept',
    'M255': 'a genuinely signed \\S+ verdict was counted in a PROTECTED decision',
    'M256': 'TrialResult.*\\n\\s*left: [1-9]\\d*\\n\\s*right: 0',
    'M257': 'TrialResult.*\\n\\s*left: [1-9]\\d*\\n\\s*right: 0',
    'M258': '\\n\\s*left: Accept\\n\\s*right: Accept',
    'M259': '\\n\\s*left: Accept\\n\\s*right: Accept',
    # WEAK@a3db33bd: Same shape as M251: empty stderr reason mismatch, stdout never ok.
    # core r1b: same only-guard route as M251 (the candidate module's NESTED use).
    'M260': r'ATTACK: a sealed module\'s copy of the operator library\'s `rubric` defined the rubric: [^\n]*"status":"ok"',
    'M261': 'counts are not its trials: admitted',
    # WEAK@a3db33bd: Refused by reverify_protected ('does not re-verify from its stored documents'), reason mismatch (reviewer log M262.log).
    'M262': r'ATTACK: a protected plan admitted an evaluation relabelled development',
    # WEAK@a3db33bd: Refused by reverify_protected (context signature doc_ref mismatch), reason mismatch (reviewer log M263.log).
    'M263': r"ATTACK: a trial counted another trial's whole verified evidence",
    'M264': '\\n\\s*left: Accept\\n\\s*right: Accept',
    'M265': 'failed: None\\n\\s*left: (?!Unsupported)\\w+\\n\\s*right: Unsupported',
    'M266': "the launcher imported a module from the caller's working directory",
    'M267': 'ran policy: admitted',
    'M268': 'is not the signed verdict: admitted',
    'M269': 'context signature: admitted',
    'M270': "the candidate's RNG activity moved the operator's stream",
    'M271': 'the reseed must not pass',
    'M272': 'its stream mirrors the operator seed',
    # C9 round 1b (fabric): EQUIVALENT (four-cell with M401)
    'M273': r'ATTACK: (status|cancel): a caller grant registry was honoured',
    'M274': 'submit \\(unread request\\): \\{\\"kind\\":\\"io\\"',
    'M275': 'unwrap_err\\(\\)` on an `Ok` value: Submission',
    'M276': 'unwrap_err\\(\\)` on an `Ok` value: Submission',
    'M277': 'unwrap_err\\(\\)` on an `Ok` value: GrantRegistry',
    'M278': 'unwrap_err\\(\\)` on an `Ok` value: ProtectedHost',
    'M279': 'forged status: .*\\n\\s*left: 0\\n\\s*right: 7',
    'M280': 'failed: op-a\\n\\s*left: Null',
    'M281': 'an unauthorized call wrote',
    'M282': "another scope's op was reconciled by an authorized status",
    'M283': 'unwrap_err\\(\\)` on an `Ok` value: \\(\\)',
    # UNCLEAR@a3db33bd: Combined assert prints both verdicts; 2-line evidence does not show whether either was PASS or a different refusal.
    'M284': r"ATTACK: the caller's environment steered the verdict to PASS",
    # WEAK@a3db33bd: PARTIAL via worktree_differs (M290); reason mismatch only.
    'M285': r'assertion `left != right` failed: (ATTACK: certified PASS despite the attack: )?\{[^\n]*"status":"PASS"',
    'M286': r'assertion `left != right` failed: (ATTACK: certified PASS despite the attack: )?\{[^\n]*"status":"PASS"',
    # WEAK@a3db33bd: PARTIAL via worktree_differs ('1 file(s) ... changed'); reason mismatch only.
    'M287': r'assertion `left != right` failed: (ATTACK: certified PASS despite the attack: )?\{[^\n]*"status":"PASS"',
    # WEAK@a3db33bd: PARTIAL via worktree_differs; reason mismatch only.
    'M288': r'assertion `left != right` failed: (ATTACK: certified PASS despite the attack: )?\{[^\n]*"status":"PASS"',
    'M289': r'assertion `left != right` failed: (ATTACK: certified PASS despite the attack: )?\{[^\n]*"status":"PASS"',
    'M290': r'assertion `left != right` failed: (ATTACK: certified PASS despite the attack: )?\{[^\n]*"status":"PASS"',
    'M291': 'unwrap_err\\(\\)` on an `Ok` value: ProtectedHost',
    'M292': 'unwrap_err\\(\\)` on an `Ok` value: ProtectedHost',
    'M293': 'a genuine keyed pass from a run that exited non-zero was reported Passed',
    'M294': 'manifest\\ for\\ another\\ operation: ACCEPTED',
    'M295': 'manifest\\ for\\ another\\ task: ACCEPTED',
    'M296': "receipt\\ rootfs\\ ref\\ not\\ the\\ manifest's: ACCEPTED",
    'M297': "manifest\\ for\\ another\\ interpreter\\ than\\ the\\ receipt's\\ pinned\\ one: ACCEPTED",
    'M298': "receipt\\ init\\ ref\\ not\\ the\\ manifest's: ACCEPTED",
    'M299': "bundle\\ verdict\\ is\\ not\\ the\\ receipt's: ACCEPTED",
    'M300': 'verdict\\ with\\ another\\ schema: ACCEPTED',
    'M301': 'verdict\\ for\\ another\\ launch\\ manifest: ACCEPTED',
    'M302': 'verdict\\ for\\ another\\ test: ACCEPTED',
    'M303': 'verdict\\ whose\\ inputs\\ did\\ not\\ match: ACCEPTED',
    'M304': 'verdict\\ claims\\ another\\ outcome\\ than\\ the\\ receipt\\ counts: ACCEPTED',
    'M305': 'the loop joins what Fabric launched and observed: .*missing field `guest_verdict`',
    'M310': 'failed: Some\\(\\"ok\\"\\)\\n\\s*left: (?!Unsupported)\\w+\\n\\s*right: Unsupported',
    'M311': 'a bundle travels with a verdict that is not protected',
    'M312': '\\n\\s*left: Passed\\n\\s*right: Unknown',
    'M313': 'the prctl failed and the runner proceeded to read S',
    'M314': 'accepted, but must refuse with',
    'M315': 'accepted, but must refuse with \\"candidate input holds an empty directory \\(g\\.ax\\)',
    'M316': 'accepted, but must refuse with \\"candidate input holds an empty directory \\(lib/lost\\+found\\)',
    'M317': 'accepted, but must refuse with \\"candidate input holds lib with mode 0700',
    'M318': 'accepted, but must refuse with \\"candidate input holds f\\.ax with mode 0000',
    # ── C9 round 1, HARNESS workstream rows (M375-M385) ──
    'M375': r'manifest for another trial: ACCEPTED',
    'M376': r'manifest for another attempt: ACCEPTED',
    # M377/M378 alone are EQUIVALENT (four-cell); M379 removes both joins.
    'M377': r'manifest for another candidate: ACCEPTED',
    'M378': r'manifest for another candidate: ACCEPTED',
    'M379': r'manifest for another candidate: ACCEPTED',
    'M380': r'manifest for another test: ACCEPTED',
    'M381': r"receipt kernel ref not the manifest's \(the reviewer's repro\): ACCEPTED",
    'M382': r"receipt qualification ref not the manifest's: ACCEPTED",
    'M383': r'a bundle of another schema version: ACCEPTED',
    'M384': r'ATTACK: a protected verdict for a request that named another suite: ACCEPTED',
    'M385': r'ATTACK: certified PASS despite the attack: \{[^\n]*"status":"PASS"',
    # ── C9 round 1: FABRIC/READINESS/INPUTS/LOOP rows (M320-M369). Placeholder
    # marker until tightened from the merged tree's own kill evidence.
    # ── C9 round 1b, workstream LOOP: M360-M368 tightened from the
    # b3f32ee2 `--only` run's failing panics.
    'M360': r'ATTACK: a forged attribution was admitted: Accept',
    'M361': r'ATTACK: a forged attribution was admitted: Accept',
    'M362': r'ATTACK: a forged attribution was admitted: Accept',
    'M363': r"ATTACK: an observation signed by a key the operator's verifier root also holds was ACCEPTED",
    'M364': r"ATTACK: a verdict signed by a key the operator's monitor root also holds was ACCEPTED",
    'M365': r"ATTACK: an observation signed by a key the operator's verifier root also holds was ACCEPTED",
    'M366': r'ATTACK: no host config: a manifest naming no operator host was ACCEPTED',
    'M367': r'ATTACK: no host config: a manifest naming no operator host was ACCEPTED',
    'M368': r'ATTACK: no suite registry: a manifest naming no operator host was ACCEPTED',
    # ── C9 round 1b, workstream LOOP (M425-M434): the consumer-side join on
    # the protected execution leg (verify_execution, class b).
    'M425': r'ATTACK: (no evidence class|guest-unobserved class): an unobserved execution leg was counted as protected',
    'M426': r'ATTACK: no launch manifest: an unobserved execution leg was counted as protected',
    'M427': r'ATTACK: no preflight observation: an unobserved execution leg was counted as protected',
    'M428': r'ATTACK: a development-backend execution leg was admitted as protected: Accept',
    'M335': 'ATTACK: the genuine record was renamed in after the fields were checked on the\\s+forged one',
    'M336': 'ATTACK: the certified \\(failing\\) preflight was hashed, a passing one was renamed in',
    'M337': 'ATTACK: the qualified manifest was hashed, another was renamed in, and the guest',
    'M338': 'ATTACK: the record names a verifier key the operator never trusted and readiness still said PASS',
    'M339': 'ATTACK: the record attributes the observation to a key that did not make it and readiness still said PASS',
    'M340': "ATTACK: an agent-signed observation stood in for the operator observer's and readiness still said PASS",
    'M341': 'ATTACK: the observer saw another (guest kernel|fabric revision) than the one certified and readiness still said PASS',
    'M342': "ATTACK: the record's b263_qualification_sha256 names no certified evidence and readiness still said PASS",
    'M343': "ATTACK: an agent-signed B263 record stood in for the operator's qualification and readiness still said PASS",
    'M344': 'ATTACK: the B263 qualification qualified another \\S+ than the certified \\S+ and readiness still said PASS',
    'M345': 'ATTACK: a B263 qualification of another profile stood in for the protected one and readiness still said PASS',
    'M346': 'ATTACK: a skip-worktree entry hid a modified source file, and the build provenance still says source_dirty: false',
    'M347': 'ATTACK: an untracked \\.cargo/config\\.toml changes the build, and the build provenance still says source_dirty: false',
    'M348': 'ATTACK: an owner-writable \\(0600\\) signing key signed a protected run',
    'M349': "ATTACK: the record's observation_sha256 names no certified evidence and readiness still said PASS",
    'M350': 'ATTACK: an input carrying \\S+ on "\\." was ACCEPTED',
    'M351': 'ATTACK: an input carrying \\S+ on "[^."][^"]*" was ACCEPTED',
    'M352': 'ATTACK: an input carrying system\\.posix_acl_\\w+ on "[^"]*" was ACCEPTED',
    'M353': 'ATTACK: an input carrying a POSIX ACL was not refused and the job ran',
    # C9 r1b psv: LAYER row (decision in its MUTATIONS comment): ACTIVE, killed by its own layer's output
    'M354': "ATTACK: psv_image's copy carried extended attributes of the input into the\\s+staging tree",
    # C9 r1b psv: LAYER row (decision in its MUTATIONS comment): ACTIVE, killed by its own layer's output
    'M355': 'ATTACK: an extended attribute on the staging tree reached the guest input image',
    'M369': 'ATTACK: a narrowing list that cannot be read was read as absent',
    # ── C9 round 1b, workstream PSV (M410-M418) ──
    'M410': r'ATTACK: prepare pinned guest kernel \w+ from a profile manifest the qualification never hashed',
    'M411': r'ATTACK: one result\.json was hashed as evidence \(\w+\) and another, renamed in,\s+decided the outcome Ok',
    'M412': r'ATTACK: a replace ref rewrote what HEAD names, and the build provenance still says source_dirty: false',
    'M413': r'ATTACK: a grafts file rewrote ancestry, and the build provenance still says source_dirty: false',
    'M414': r'ATTACK: info/exclude hid an untracked \.cargo/config\.toml, and the build provenance still says source_dirty: false',
    'M415': r'ATTACK: a test-trust build reported its answer as authoritative',
    'M416': r'ATTACK: a caller-chosen --issuers root was reported as authoritative in a\s+production build',
    'M417': r'ATTACK: an operator root failing the ownership walk was reported as\s+authoritative',
    # EQUIVALENT_DID (four-cell vs M339+M340): the joint cell's attack.
    'M418': r'ATTACK: the record names an observer key the operator never trusted, that key made\s+the observation, and readiness still said PASS',
    # ── C9 round 1b, FABRIC workstream rows (M400-M409) ──
    # M400 / M401: EQUIVALENT (four-cell with M187 / M273).
    'M400': r'ATTACK: a candidate file was launched as a check on the protected profile',
    'M401': r'ATTACK: (status|cancel): a caller grant registry was honoured',
    # ── C9 round 1: FABRIC/READINESS/INPUTS/LOOP rows (M320-M369). M320-M334
    # (fabric) tightened in round 1b from a fresh --only run's kill evidence;
    # the rest are placeholders until their workstreams tighten them.
    'M320': r'ATTACK: an unobserved protected-profile execution \(.*\) was attested as a protected execution',
    'M321': r'ATTACK: the protected profile was selected for an interpreter_run',
    'M322': r'ATTACK: [a-z-]+: a bundle travels beside a receipt downgraded to guest-unobserved',
    'M323': r'ATTACK: [a-z-]+: an inadmissible observed launch was classed protected',
    'M324': r"ATTACK: (runs|nonces) as an agent-owned leaf was accepted as the service's own private directory",
    'M325': r"ATTACK: (runs|nonces) as an agent-owned leaf was accepted as the service's own private directory",
    'M326': r"ATTACK: (runs|nonces) as an agent-owned leaf was accepted as the service's own private directory",
    'M327': r"ATTACK: (runs|nonces) as a group-writable leaf was accepted as the service's own private directory",
    'M328': r"ATTACK: an observer root holding the host signer's public key loaded",
    'M329': r'ATTACK: an observation signed by a key that is also the [a-z-]+ key launched',
    'M330': r"ATTACK: an observer root holding the host signer's public key loaded",
    'M331': r'ATTACK: an observer key that is also a [a-z]+ key loaded',
    'M332': r"ATTACK: a host config that cannot be stat'ed \(ENOTDIR\) was read as 'not a protected host'",
    'M333': r"ATTACK: a journal that cannot be stat'ed \(ENOTDIR\) was read as no journal",
    'M334': r'ATTACK: the trust preflight never probes .*, which load pins',
    # ── C9 round 1b, integration ──
    'M436': r"ATTACK: the operator's own `use rubric` loaded the candidate's copy, which defined the rubric: [^\n]*\"status\":\"ok\"",
    'M402': r'ATTACK: a replayed execution claiming an observed protected launch was attested',
    'M403': r'ATTACK: a job file \((secret|manifest|job dir)\) carrying \S+ was not refused and the job ran',
}
