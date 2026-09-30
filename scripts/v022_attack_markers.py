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
    # C9 r2 (rows): the test now attacks both routes; these guards are the direct route's only ones.
    'M228': "ATTACK: direct: the caller's environment reached the verify step",
    'M229': 'ATTACK: direct: the secret outlived the launch into the verify step',
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
    'M260': r"ATTACK: a sealed module's use pulled the suite's unimported reference module into the program",
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
    # C9 round 2 (LOOP): ACTIVE again; its own '@'-id attack at check_bundle.
    'M384': r'ATTACK: a protected bundle for suite other joined a request that ran\s+check:acceptance: ACCEPTED',
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
    # C9 r2 (rows): the second read is now served by an in-place write under a held open.
    'M411': r'ATTACK: one result\.json was hashed as evidence \(\w+\) and another, written in\s+place, decided the outcome Ok',
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
    # ── C9 round 2, KEYS workstream (M460-M469; A67) ──
    'M460': r'ATTACK: an observer key shared with an UNREADABLE verifier root passed key-role separation',
    'M461': r"ATTACK: a [a-z]+ root holding the host signer's public key loaded",
    'M462': r"ATTACK: a B263 record minted with the host signer's key qualified the protected",
    'M463': r"ATTACK: a B263 record minted with the host signer's key qualified the protected",
    'M464': r'ATTACK: a qualification key also held by the verifier root qualified the protected',
    'M465': r"ATTACK: the host signer's key, planted in the qualification root, signed the certification and readiness still said PASS",
    'M466': r'ATTACK: an agent-owned monitor root was read as holding no shared key and readiness still said PASS',
    # ── Retired EQUIVALENT rows: the JOINT cell's attack (paired-disable
    # matches it; the mutation run never scores a retired row). Carried from
    # C8 without markers, which made their joint cell unmatchable (C9 r1b).
    'M58': r"ATTACK: `[^`]*` passed: a break/continue escaped a function body",
    'M245': r'ATTACK: monitor: activated on a revoked key',
    # ── C9 round 2, workstream LOOP (M470-M479) ──
    'M470': r'ATTACK: an observation signed by a rooted key the store trusts for no observer was\s+ACCEPTED',
    'M471': r'ATTACK: an observation signed by the key of an observer the store does not trust\s+was ACCEPTED',
    'M472': r'ATTACK: a forged attribution was admitted: Accept',
    'M473': r'ATTACK: a protected manifest whose verifier_sha256 unknown is not a sha256 was ACCEPTED',
    'M474': r'ATTACK: prepare built a launch manifest whose launcher_sha256 is "unknown"',
    'M475': r'ATTACK: a pinned suite reference with a second reading was WRITTEN to the config',
    'M476': r'ATTACK: a config pinning a suite reference with a second reading was READ',
    'M477': r'ATTACK: check suite id "acceptance@x" holding a reference separator was REGISTERED',
    'M478': r'ATTACK: an observation whose key two trusted observers share was ACCEPTED',
    # ── C9 round 2, workstream PROVENANCE (M450-M459) ──
    'M450': r"ATTACK: build provenance ran the repository's filter driver as the builder",
    'M451': r"ATTACK: an index entry forged to match the modified file's stat data hid the edit, and the build provenance still says source_dirty: false",
    'M452': r"ATTACK: git run as the verifier lazily fetched a missing object and ran the\s+repository's core\.sshCommand",
    'M453': r"ATTACK: a gitfile naming a repository elsewhere was accepted as the build's tree",
    'M454': r"ATTACK: an untracked build\.rs appeared after the snapshot, before the manifest, and the guest manifest says axon_tree_dirty_at_build: false",
    'M455': r'ATTACK: the provenance helper could not be built \("cannot tell"\), and the guest manifest says axon_tree_dirty_at_build: false',
    'M456': r"ATTACK: no snapshot of the tree the artifacts were built from, and the guest manifest says axon_tree_dirty_at_build: false",
    'M457': r"ATTACK: the tree moved to another commit between the snapshot and the manifest, and the guest manifest says axon_tree_dirty_at_build: false",
    'M458': r"ATTACK: the tree was dirty when the build started and clean again by manifest time, and the guest manifest says axon_tree_dirty_at_build: false",
    'M459': r"ATTACK: grafted ancestry passed the guest build's PCI lineage check",
    # ── C9 round 2, workstream BCE (M500-M519): decisions C and E ──
    'M500': r"ATTACK: a \.gitignored build\.rs changes the build, and the build provenance still says source_dirty: false",
    'M501': r"ATTACK: a \.gitignored \.cargo/config\.toml changes the build, and the build provenance still says source_dirty: false",
    'M502': r"ATTACK: an other-writable allowlist excused an untracked target/, and the build provenance still says source_dirty: false",
    'M503': r"ATTACK: an allowlist entry \(src/\) covering a tracked source directory excused an\s+untracked src/build\.rs",
    'M504': r"ATTACK: readiness certified a linked worktree \(a gitfile names the repository\)",
    'M505': r"ATTACK: a HEAD that does not descend from the PCI-certified revision, and the guest manifest says axon_tree_dirty_at_build: false",
    'M506': r"ATTACK: a HEAD that does not descend from the PCI-certified revision, and the guest manifest says axon_tree_dirty_at_build: false",
    # ── C9 round 2, workstream LAUNCHER (M520-M549) ──
    'M520': r'ATTACK: a symlinked launcher was verified for exec',
    'M521': r'ATTACK: a launcher of mode 775 \(writable by another uid\) was verified for exec',
    'M522': r"ATTACK: a launcher owned by a uid other than the operator's was verified for exec",
    'M523': r'ATTACK: a launcher another process holds open for writing was verified for exec',
    'M524': r'ATTACK: a writer opened the launcher between its hash and its exec, and the\s+exec went ahead',
    'M525': r'ATTACK: a launcher swapped in by a rename race between its hash and its exec was\s+executed',
    'M526': r'ATTACK: a binary swapped in by a rename race between its hash and its exec was\s+executed',
    'M527': r'ATTACK: a writer opened the launcher between its hash and its exec, and the\s+exec went ahead',
    'M528': r'ATTACK: bytes other than the pin were verified for exec',
    'M529': r'ATTACK: a script was executed through the interpreter its #! line names, unpinned',
    'M530': r'ATTACK: a FIFO was verified for exec',
    'M531': r'ATTACK: uid 4243, not the configured Fabric uid, made the helper launch as root',
    'M532': r'ATTACK: a request naming a path outside the operator roots \(out elsewhere\) was\s+accepted',
    'M534': r'ATTACK: a request naming a path outside the operator roots \(job from another inputs dir\) was\s+accepted',
    'M535': r'ATTACK: a request with an unknown field `launcher` was accepted',
    'M536': r'ATTACK: a helper config admitting root \(uid 0\) as the Fabric uid was accepted',
    'M537': r'ATTACK: an out root group-accessible \(0770\) was launched into',
    'M538': r"ATTACK: a root-owned file among the Fabric's inputs was read by the root helper",
    'M539': r"ATTACK: inputs over the operator's max_input_bytes were launched",
    'M540': r'ATTACK: a FIFO the launch left in the out dir was handed to Fabric',
    'M541': r'ATTACK: a launch by a launcher other than the one the manifest pins was accepted',
    'M542': r"ATTACK: the per-attempt secret was still in Fabric's job dir while the root launcher ran",
    'M543': r"ATTACK: the snapshot's copy of the secret was still on disk when --verify-result ran",
    'M544': r'ATTACK: a launch Fabric ran itself \(the development route, no privileged launcher\)\s+was attested protected',
    'M545': r'ATTACK: a test-trust helper \(which takes --test-config, a caller-chosen config\)\s+attested a protected verdict in a production Fabric',
    'M546': r'ATTACK: Fabric running as root was accepted on a protected host',
    'M547': r'ATTACK: a helper config running another launcher than the host pins was accepted',
    'M548': r"ATTACK: a helper config admitting another uid than Fabric's was accepted",
    'M549': r"ATTACK: the launcher ran with the caller's real uid",
}

# ── C9 round 2, HARNESS workstream (amendment 43) ───────────────────────────
# New rows M480-M492, and the round-2 audit of every marker that matched a
# bare non-pass verdict, a generic status or a generic assertion shape: each
# is tightened so it cannot also match another check's refusal or a setup
# failure. Overrides are applied here, not in place, so the edit is additive.
ATTACK_MARKERS.update({
    # new rows
    'M480': r'ATTACK: certified PASS despite the attack: \{[^\n]*"status":"PASS"',
    'M481': r'ATTACK: readiness hung on a FIFO signature with no writer',
    'M482': r'ATTACK: a FIFO served the checked record and the signed record as two reads',
    'M483': r'ATTACK: a registered check was attested as a protected execution\s*\n\s*left: Ok\(\(\)\)',
    'M484': r'ATTACK: an execution on a non-protected backend was attested as protected\s*\n\s*left: Ok\(\(\)\)',
    'M485': r'ATTACK: an execution whose journal names no backend was attested as protected\s*\n\s*left: Ok\(\(\)\)',
    'M486': r"ATTACK: a regular file was accepted as the service's own directory: Ok\(\(\)\)",
    'M487': r"ATTACK: a symlinked service leaf was accepted as the service's own directory: Ok\(\(\)\)",
    'M488': r'ATTACK: a signer key reached through a symlink out of the operator-owned key\s+directory was accepted and a protected run launched',
    'M489': r'ATTACK: an entry whose extended attributes could not be listed was read as\s+carrying none: Ok\(\(\)\)',
    'M490': r'ATTACK: a trust root writable by the process running readiness authorized: Ok\(\(\)\)',
    'M491': r'ATTACK: a trust root writable by the process running readiness authorized: Ok\(\(\)\)',
    'M492': r'ATTACK: the production readiness trust accepts a trust root the verifier can write',
    # M04/M177: the ORDER, on the property only it guards (an honest
    # candidate's colliding module is never resolved first). Was: any non-pass
    # (M177), which M436's E0901 refusal matched (round-2 blocker).
    'M04': r"ATTACK: the candidate's helper\.ax was resolved before the suite's own helper \(its\s+presence alone changed the verdict\)[^\n]*\n\s*left: (Unknown|Failed)",
    'M177': r"ATTACK: the candidate's helper\.ax was resolved before the suite's own helper: (Unknown|Failed) ",
    # M220: the sibling's own result line, not any non-pass verdict.
    'M220': r'ATTACK: a suite sibling ran beside the registered test t_pair: .*\{"name":"t_pair_breaks","status":"failed"',
    # M74: the attack's own value (the candidate's handler swallowed the
    # operator's completion: 107), not any Err of an honest program.
    'M74': r'ATTACK: a handler completion was caught by a `with` that did not install it\s*\n\s*left: Err\("assertion failed: 107 != 7"\)',
    # M44: after a control run with no AXON_STRICT passes.
    'M44': r"ATTACK: AXON_STRICT in the launcher's environment steered the signed verdict",
    # M140: the run went on to read its arguments, i.e. past the signer check
    # (the test pins the grant registry so nothing else refuses first).
    'M140': r'ATTACK: a (group-readable host signer key|host signer key that does not derive its pin) was not refused: \{"kind":"usage","reason":"--tenant is required"',
    # A launched observation shows a verdict of a run; NotRun is the refusal,
    # and NotRequested is no launch either.
    'M192': r'failed: op-obs--qualification-observer\s*\n\s*left: (Passed|Failed|Unknown)\s*\n\s*right: NotRun',
    'M193': r'failed: op-obs-claims-other-key-observer-observer\s*\n\s*left: (Passed|Failed|Unknown)\s*\n\s*right: NotRun',
    'M194': r'failed: op-obs-epoch-observer-observer\s*\n\s*left: (Passed|Failed|Unknown)\s*\n\s*right: NotRun',
    'M195': r'failed: op-obs-stale-observer-observer\s*\n\s*left: (Passed|Failed|Unknown)\s*\n\s*right: NotRun',
    'M200': r'failed: op-obs-other-manifest-observer-observer\s*\n\s*left: (Passed|Failed|Unknown)\s*\n\s*right: NotRun',
    'M201': r'failed: op-obs-nonce-issued-elsewhere-observer-observer\s*\n\s*left: (Passed|Failed|Unknown)\s*\n\s*right: NotRun',
    'M202': r'failed: op-obs-kernel-observer-observer\s*\n\s*left: (Passed|Failed|Unknown)\s*\n\s*right: NotRun',
    # M204 swallows observe's Err for EVERY defective observation, so its
    # attack is whichever case runs first; it must still be a LAUNCH.
    'M204': r'failed: op-obs-[\w-]+\s*\n\s*left: (Passed|Failed|Unknown)\s*\n\s*right: NotRun',
    'M227': r'failed: op-obs-verifier-[\w-]+\n\s*left: (Passed|Failed|Unknown)\n\s*right: NotRun',
    # A forgery ACCEPTED is a verdict (Passed/Failed); any other non-Unknown
    # would be another refusal.
    'M239': r'failed: forge-fail\n\s*left: (Passed|Failed)\n\s*right: Unknown',
    'M241': r'failed: swap-after\n\s*left: (Passed|Failed)\n\s*right: Unknown',
    # It RAN (a run's status), not Denied/Canceled by another check.
    'M265': r'failed: None\n\s*left: (Completed|Failed|TimedOut|OutcomeUnknown)\n\s*right: Unsupported',
    'M310': r'failed: Some\(\"ok\"\)\n\s*left: (Completed|Failed|TimedOut|OutcomeUnknown)\n\s*right: Unsupported',
    # These ATTACK messages sit on `is_err_and(contains(reason))`, which also
    # fails on a refusal for another reason: only the printed Ok(()) is a load.
    'M324': r"ATTACK: (runs|nonces) as an agent-owned leaf was accepted as the service's own private directory: Ok\(\(\)\)",
    'M325': r"ATTACK: (runs|nonces) as an agent-owned leaf was accepted as the service's own private directory: Ok\(\(\)\)",
    'M326': r"ATTACK: (runs|nonces) as an agent-owned leaf was accepted as the service's own private directory: Ok\(\(\)\)",
    'M327': r"ATTACK: (runs|nonces) as a group-writable leaf was accepted as the service's own private directory: Ok\(\(\)\)",
    'M328': r"ATTACK: an observer root holding the host signer's public key loaded: Ok\(\(\)\)",
    'M330': r"ATTACK: an observer root holding the host signer's public key loaded: Ok\(\(\)\)",
    'M331': r'ATTACK: an observer key that is also a [a-z]+ key loaded: Ok\(\(\)\)',
})
ATTACK_MARKERS.update({
    # guest-init.sh (the script text; the boot test proves it in effect)
    'M493': r'ATTACK: the candidate drive is in effect mounted without ro \(',  # C9 r3 (harness): behavioural
    'M494': r'ATTACK: the suite drive is in effect mounted without ro \(',  # C9 r3 (harness): behavioural
    'M495': r'ATTACK: the job drive is in effect mounted without ro \(',  # C9 r3 (harness): behavioural
    'M496': r'ATTACK: the candidate drive is in effect mounted without noexec \(',  # C9 r3 (harness): behavioural
    'M497': r'ATTACK: a cmdline naming two launch manifests was accepted: TOOK=',
    'M498': r"ATTACK: the PSV runner started with a variable from PID 1's environment",  # C9 r3 (harness): behavioural
    'M499': r"ATTACK: the workload started with a variable from PID 1's environment",  # C9 r3 (harness): behavioural
})
# ── C9 round 3, HARNESS workstream (EQUIVALENCE; M585-M609) ──
ATTACK_MARKERS.update({
    'M585': r'ATTACK: a helper config of mode 646 \(writable by another uid\) was obeyed and\s+launched',
    'M586': r'ATTACK: a helper config owned by uid 4243 \(not the operator\) was obeyed and\s+launched',
    'M587': r'ATTACK: the helper launched into an out root below a directory another uid can\s+write \(mode 777\)',
    'M588': r'ATTACK: the helper launched into an out root below a directory uid 4243 owns',
    'M589': r'ATTACK: the helper launched into an out root below a directory another uid can\s+write \(mode 777\)',
    'M590': r'ATTACK: the helper launched with a firecracker binary owned by uid 4243, who can\s+rewrite it after it is verified',
    'M591': r'ATTACK: an authority program that could not be leased \(leased=false\) was\s+verified under Lease::Required',
    'M592': r'ATTACK: a file that changed between its read and the verdict was vouched for\s+as the pinned bytes',
    'M593': r'ATTACK: an authority program larger than MAX_BYTES \(268435456\) was read and\s+hashed whole',
    'M594': r'ATTACK: an interpreter that is itself a script ran through its own #! line',
    'M595': r'ATTACK: an interpreter that is itself a script ran through its own #! line',
    'M596': r"ATTACK: uid 4243 read the per-attempt secret out of the helper's snapshot",
    'M597': r"ATTACK: uid 4243 read the per-attempt secret out of the helper's snapshot",
    'M598': r"ATTACK: the Fabric swapped the root helper's snapshot under a staging root it owns",
    'M599': r'ATTACK: the root helper took a launch.s inputs from, and deleted the job dir out of,\s+a directory the Fabric uid does not own',
    'M600': r'ATTACK: a jail id holding a path staged the launch.s inputs at "[^"]*\.\./escaped',
    'M601': r'ATTACK: a production helper took its config from a path its caller named\s+\(--test-config\) and launched as root',
    'M602': r'ATTACK: a production helper that is not root in every id launched \(the launcher ran\s+with ruid Some\("4242',
    'M603': r'ATTACK: a test-trust helper \(which obeys --test-config\) reported itself as a\s+production build',
    'M604': r'ATTACK: a helper reporting build "test-trust" put the launch on a route that\s+attests protected in a production Fabric',
    'M605': r'ATTACK: a production Fabric lets a launch by a test-trust helper \(which obeys a\s+caller-chosen --test-config\) attest a protected verdict',
    'M606': r'ATTACK: a launch Fabric ran itself \(the development route, no privileged launcher\)\s+was attested protected',
    'M607': r'ATTACK: the candidate drive is in effect mounted without ro \(',
    'M608': r'ATTACK: the suite drive is in effect mounted without noexec \(',
    'M609': r'ATTACK: the job drive is in effect mounted without nodev \(',
})
# ── C9 round 3, workstream READINESS (M570-M584) ──
ATTACK_MARKERS.update({
    'M570': r'ATTACK: a B263 record with a FAIL assertion certified the protected backend and readiness still said PASS',
    'M571': r'ATTACK: a B263 record whose result is FAIL certified the protected backend and readiness still said PASS',
    'M572': r'ATTACK: a B263 record 89 days old certified the protected backend and readiness still said PASS',
    'M573': r'ATTACK: a B263 record produced from a dirty source tree certified the protected backend and readiness still said PASS',
    'M574': r'ATTACK: a B263 record naming no host certified the protected backend and readiness still said PASS',
    'M575': r"ATTACK: a B263 record qualifying another host's firecracker certified the observed launch and readiness still said PASS",
    'M576': r"ATTACK: the host's B263 qualification lapsed \(31 days\) and readiness still said PASS",
    'M577': r'ATTACK: a certification dated before the run it certifies was observed and readiness still said PASS',
    'M578': r'ATTACK: a certification dated in the future and readiness still said PASS',
    'M579': r'ATTACK: an agent-signed waiver excused a BLOCKED B263 assertion and readiness still said PASS',
    'M580': r'ATTACK: readiness certified a linked worktree whose admin dir was placed as \.git',
    'M581': r'ATTACK: a forged ancestor object made HEAD descend from the PCI-certified revision, and the guest manifest says axon_tree_dirty_at_build: false',
    'M582': r'ATTACK: the freeze bound evidence from a linked worktree whose admin dir was placed as \.git: Ok\(',
    'M583': r"ATTACK: the freeze bound evidence from a tree whose \.git is a symlink to another clone's: Ok\(",
    'M584': r'ATTACK: the freeze bound evidence from a guest image built from a dirty tree: Ok\(',
})
# C9 round 3, workstream CORE (M560-M569; amendment 46).
ATTACK_MARKERS.update({
    'M560': r'ATTACK: sealed handler answered operator code: the candidate.s arm aborted the operator.s listener at its println \(t_abort\)',
    'M561': r'ATTACK: sealed handler answered operator code: the candidate.s multi-shot replay fed the operator.s read_file \(t_replay\)',
    'M562': r'ATTACK: sealed handler answered operator code: the candidate.s arm aborted the operator.s listener through a candidate call \(t_through\)',
    'M563': r'ATTACK: the candidate steered the operator.s challenge by calling its closure five times: Ok\(Completed\)',
    'M564': r'ATTACK: a test that ended early at `\?` was counted complete \(t_find\): Ok\(Completed\)',
    'M565': r'ATTACK: a test that ended early at `\?` was counted complete \(t_find\): Ok\(Completed\)',
    'M566': r'ATTACK: a fn declared -> Result returned a None across its boundary: Ok\(EndedEarly',
})
# C9 round 3, integration.
ATTACK_MARKERS['M567'] = r'ATTACK: the host config allows one hour, the B263 record is 12\.5 h old, and readiness still said PASS'
# ── C9 round 3, LOOP workstream (M610-M618; amendment 49) ──
ATTACK_MARKERS.update({
    'M610': r'ATTACK: a manifest naming suite version "acf1:5{64}#x" entry accept\.ax joined the pin',
    'M611': r'ATTACK: suite version "acf1:5{64}#x" entry "accept\.ax" was written as check-suite:acceptance@acf1:5{64}#x#accept\.ax',
    'M612': r'ATTACK: prepare built a launch manifest for suite id "acc@x"',
    'M613': r'ATTACK: suite version "acf1:1{64}#x" holding a reference separator was REGISTERED',
    'M614': r'ATTACK: a protected verdict launched under authority epoch 5 was recorded for a\s+trial at epoch 0: ACCEPTED',
    'M615': r'ATTACK: a protected verdict launched for tenant other-tenant was recorded in\s+tenant tenant-a: ACCEPTED',
    'M616': r'ATTACK: a protected verdict launched for task family other-family was recorded\s+in family coding: ACCEPTED',
    'M617': r'ATTACK: a protected launch took its authority epoch from a caller-named --store\s+\S*caller-store: ACCEPTED',
    'M618': r'ATTACK: a protected host that pins no authority store launched with the caller\'s\s+--store: ACCEPTED',
})
ATTACK_MARKERS.update({
    # ── C9 round 3, CUSTODIAN workstream (M620-M638; amendment 50) ─────────
    # M325 re-pointed to the custodian's store (the Fabric nonce_store leaf it
    # covered no longer exists); M196 to the custodian's spend (same test and
    # marker); M292 to the custodian socket's directory (same marker).
    'M325': r'ATTACK: a nonce store its group can write \(the Fabric.s group\) was accepted',
    'M620': r'ATTACK: the root helper launched with no observation',
    'M621': r'ATTACK: one observation launched the root launcher twice',
    'M622': r'ATTACK: one observation launched the root launcher twice',
    'M623': r'ATTACK: the root helper launched a snapshot manifest other than the one the request',
    'M624': r'ATTACK: a launch whose nonce a DEV custodian spent ran as a protected launch',
    'M625': r"ATTACK: a production helper accepted a test custodian's spend",
    'M626': r'ATTACK: the root helper spent the nonce through a custodian the Fabric uid serves',
    'M627': r'ATTACK: a uid that is not the Fabric was issued a nonce',
    'M628': r'ATTACK: the Fabric spent a nonce itself',
    'M629': r'ATTACK: a custodian running as the Fabric uid was accepted',
    'M630': r'ATTACK: a nonce store another uid owns was accepted',
    'M631': r'ATTACK: the custodian served from a nonce store its group can write',
    'M632': r'ATTACK: a custodian ran as a uid other than its configured custodian uid',
    'M633': r'ATTACK: a helper config spending through a custodian of uid \d+',
    'M634': r"ATTACK: a protected host config naming the Fabric's own uid as its custodian was",
    'M635': r'ATTACK: a host config giving Fabric its own nonce store loaded: Ok\(\(\)\)',
    'M636': r"ATTACK: a helper config spending through another custodian than the host's was\s+accepted: Ok\(\(\)\)",
    'M637': r"ATTACK: a helper config naming another host signer than the host's was accepted: Ok\(\(\)\)",
    'M638': r"ATTACK: the root helper launched on an observation signed with the host signer's key",
    'M639': r'ATTACK: an observation for epoch 7 launched a manifest naming authority epoch 0',
})
ATTACK_MARKERS.update({
    # ── C9 round 3, ROWS workstream (M640-M649): the 15 rows the 1084ed1c
    # run left unkilled, each re-attacked where its guard is still the ONLY
    # guard, or retired under the four-cell rule.
    # M72: through an operator HELPER the escaped `return` ends the helper
    # with the candidate's value and the test body completes (the completion
    # rule refuses the direct shape first).
    'M72': r"ATTACK: a `return` escaped the candidate's [a-z ]+ into the operator's helper and the test completed",
    # M194/M201/M204: on the DIRECT route no privileged helper re-verifies the
    # observation; the attack is the defective observation's launch.
    'M194': r'ATTACK: op-obs-epoch-observer-observer-direct: a defective observation launched on the direct route',
    'M201': r'ATTACK: op-obs-nonce-(forged|issued-elsewhere)-observer-observer-direct: a defective observation launched on the direct route',
    'M204': r'ATTACK: op-obs-[\w-]*-direct: a defective observation launched on the direct route',
    # M310: a protected host composed with a launcher Fabric runs itself.
    'M310': r'ATTACK: a protected host with no observer launched a protected-profile check\s+\(direct route\)',
    # M284: the development lineage answer, under the caller's GIT_DIR (decision
    # E's repository-identity rule refuses it on every protected path).
    'M284': r"ATTACK: the caller's GIT_DIR named another repository and it answered the development\s+lineage check",
    'M640': r'ATTACK: a protected custodian config letting uid 4242 spend nonces was accepted',
    'M641': r'ATTACK: a protected custodian served from a store whose parent the Fabric uid\s+owns',
    'M642': r"ATTACK: a branch named like the certified revision's abbreviation made an orphan HEAD\s+descend from it",
    'M650': r'ATTACK: a freeze was made through a compiler wrapper',
})

# C9 round 4, CORE workstream (M651-M667, amendment 53, matrix A86): each
# attack's own getting-through is the test COMPLETING (`Ok(Completed)`), never
# a refusal reason, so a different check refusing it is REFUSED_ELSEWHERE.
ATTACK_MARKERS.update({
    'M651': r"ATTACK: the candidate's `ok` ran under the operator's method name: Ok\(Completed\)",
    'M652': r"ATTACK: a confused bool crossed a declared `-> i64` return: Ok\(Completed\)",
    'M653': r"ATTACK: an `Other` crossed a declared `-> Point` return: Ok\(Completed\)",
    'M654': r"ATTACK: a confused element crossed a declared `-> \[i64\]` return: Ok\(Completed\)",
    'M655': r"ATTACK: a confused payload crossed a declared `-> Option<i64>` return: Ok\(Completed\)",
    'M656': r"ATTACK: a confused element crossed a declared `-> \(i64, i64\)` return: Ok\(Completed\)",
    'M657': r"ATTACK: a confused field crossed a declared `-> Holder` return: Ok\(Completed\)",
    'M658': r"ATTACK: a confused field was constructed into a candidate global: Ok\(Completed\)",
    'M659': r"ATTACK: a confused value entered the operator's `x: i64` parameter: Ok\(Completed\)",
    'M660': r"ATTACK: a value at an undetermined type parameter crossed into operator code: Ok\(Completed\)",
    'M661': r"ATTACK: a closure declared `fn\(i64\) -> i64` returned a confused bool: Ok\(Completed\)",
    'M662': r"ATTACK: the operator's listener was called with a confused bool: Ok\(Completed\)",
    'M663': r"ATTACK: a confused bool was sent on a `Chan<i64>` the operator reads: Ok\(Completed\)",
    'M664': r"ATTACK: a confused bool was bound to the operator's `let r: i64`: Ok\(Completed\)",
    'M665': r"ATTACK: a sealed module named the operator's `\w+` in a type position \([^)]*\): \[\]",
    'M666': r"ATTACK: a bool passed the operator's `T: Judge` bound through `Lax`: Ok\(Completed\)",
    'M667': r"ATTACK: the operator's `\|r: i64\|` listener was called with a confused bool: Ok\(Completed\)",
})
