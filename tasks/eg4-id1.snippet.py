# enable-gaps EG-4 lane I-D1 snippet (confidence.rs, cli.rs). Spliced by the integrator, then deleted.
# Four parts, each marked with the script it goes into:
#   [g01]  scripts/v022_g01_mutations.py   after the last `MUTATIONS += [...]` block, before `if __name__`
#   [mk]   scripts/v022_attack_markers.py  after the last `ATTACK_MARKERS.update({...})`
#   [cov]  scripts/v022_refusal_coverage.py  before `def load_rows():`
#   [nys]  scripts/v022_refusal_coverage.py  NOT_YET_SCANNED: delete these keys
#
# Judged: confidence.rs 21 sites (15 rows, 2 exempt), cli.rs 16 sites (10 by 8 rows, 6 exempt).
# cli.rs:217 (`out.resolved.map_err(Produce::Refused)?`) is covered by M5107: its guard block opens at the
# transcript write (cli.rs:210) the row mutates.

# ---- [g01] -------------------------------------------------------------------------------------------
# enable-gaps EG-4 lane I-D1 (ids M5090-M5112, block M5090-M5129): axon-intent's confidence rubric and
# CLI surface. confidence.rs: the four hard refusals, the two output predicates, the floor, the five
# weights and the three deduction names. cli.rs: no fallback when there is no program source, the two
# transcript writes, verify_triple's no-token refusal, the unrecognized invocation, a refusal's exit
# code, and `approve --reject`.
_CONF = 'crates/axon-intent/src/confidence.rs'
_ICLI = 'crates/axon-intent/src/cli.rs'
MUTATIONS += [
    ('M5090', 'EG-4 I-D1: a draft that does not typecheck is scored instead of refused', _CONF,
     '    if !m.typechecks {', '    if false && !m.typechecks {',
     'axon-intent', '--lib', 'confidence::tests::low_confidence_synthesis_refused'),
    ('M5091', 'EG-4 I-D1: a draft with no entry point is scored instead of refused', _CONF,
     '    if !m.has_entry {', '    if false && !m.has_entry {',
     'axon-intent', '--lib', 'confidence::tests::low_confidence_synthesis_refused'),
    ('M5092', 'EG-4 I-D1: an empty or stub body is scored instead of refused', _CONF,
     '    if is_stub_body(&draft.source) {', '    if false && is_stub_body(&draft.source) {',
     'axon-intent', '--lib', 'confidence::tests::low_confidence_synthesis_refused'),
    ('M5093', 'EG-4 I-D1: a draft writing an unasked output is scored instead of refused (scope expansion)', _CONF,
     '    if !outputs_subset(&m.resolved_outputs, &intent.outputs) {',
     '    if false && !outputs_subset(&m.resolved_outputs, &intent.outputs) {',
     'axon-intent', '--lib', 'confidence::tests::low_confidence_synthesis_refused'),
    ('M5094', 'EG-4 I-D1: outputs_subset asks for ANY written output to be wanted, so an asked one licenses an unasked one', _CONF,
     '        .all(|o| wanted.iter().any(|w| same_path(o, w)))',
     '        .any(|o| wanted.iter().any(|w| same_path(o, w)))',
     'axon-intent', '--lib', 'confidence::tests::an_unasked_output_beside_an_asked_one_is_refused'),
    ('M5095', 'EG-4 I-D1: outputs_match asks for ANY wanted output to be written, so part of them earns the match', _CONF,
     '        .all(|w| resolved.iter().any(|o| same_path(o, w)))',
     '        .any(|w| resolved.iter().any(|o| same_path(o, w)))',
     'axon-intent', '--lib', 'confidence::tests::a_program_writing_part_of_the_asked_outputs_is_not_a_match'),
    ('M5096', 'EG-4 I-D1: the floor rises above typecheck + entry, so a deduction alone refuses a draft (R22 §4.3)', _CONF,
     'pub const CONFIDENCE_FLOOR: u8 = 60;', 'pub const CONFIDENCE_FLOOR: u8 = 61;',
     'axon-intent', '--lib', 'confidence::tests::no_examples_is_a_named_deduction_that_never_refuses'),
    ('M5097', 'EG-4 I-D1: the typecheck weight drops, so typecheck + entry no longer reach the floor', _CONF,
     'const W_TYPECHECKS: u8 = 40;', 'const W_TYPECHECKS: u8 = 39;',
     'axon-intent', '--lib', 'confidence::tests::no_examples_is_a_named_deduction_that_never_refuses'),
    ('M5098', 'EG-4 I-D1: the entry weight drops, so typecheck + entry no longer reach the floor', _CONF,
     'const W_ENTRY: u8 = 20;', 'const W_ENTRY: u8 = 19;',
     'axon-intent', '--lib', 'confidence::tests::no_examples_is_a_named_deduction_that_never_refuses'),
    ('M5099', 'EG-4 I-D1: the outputs-match weight is not the documented 15', _CONF,
     'const W_OUTPUTS_MATCH: u8 = 15;', 'const W_OUTPUTS_MATCH: u8 = 14;',
     'axon-intent', '--lib', 'confidence::tests::each_rubric_weight_is_the_documented_one'),
    ('M5100', 'EG-4 I-D1: the no-uncertainty weight is not the documented 15', _CONF,
     'const W_NO_UNCERTAINTY: u8 = 15;', 'const W_NO_UNCERTAINTY: u8 = 14;',
     'axon-intent', '--lib', 'confidence::tests::each_rubric_weight_is_the_documented_one'),
    ('M5101', 'EG-4 I-D1: the examples weight is not the documented 10', _CONF,
     'const W_EXAMPLES: u8 = 10;', 'const W_EXAMPLES: u8 = 9;',
     'axon-intent', '--lib', 'confidence::tests::each_rubric_weight_is_the_documented_one'),
    ('M5102', 'EG-4 I-D1: the unmatched-outputs deduction is shown under another name', _CONF,
     'pub const DEDUCT_OUTPUTS: &str = "outputs-not-matched";', 'pub const DEDUCT_OUTPUTS: &str = "outputs-unmatched";',
     'axon-intent', '--lib', 'confidence::tests::the_deductions_carry_their_documented_names'),
    ('M5103', 'EG-4 I-D1: the uncertainty deduction is shown under another name', _CONF,
     'pub const DEDUCT_UNCERTAINTY: &str = "uncertainty-notes";', 'pub const DEDUCT_UNCERTAINTY: &str = "uncertain";',
     'axon-intent', '--lib', 'confidence::tests::the_deductions_carry_their_documented_names'),
    ('M5104', 'EG-4 I-D1: the no-examples deduction is shown under another name', _CONF,
     'pub const DEDUCT_NO_EXAMPLES: &str = "no-examples";', 'pub const DEDUCT_NO_EXAMPLES: &str = "examples-missing";',
     'axon-intent', '--lib', 'confidence::tests::the_deductions_carry_their_documented_names'),
    ('M5105', 'EG-4 I-D1: compile with no program source and no --generator falls back to the mock synthesizer', _ICLI,
     '        let Some(spec) = flags.generator.as_deref() else {\n            return Err(Produce::Usage(',
     '        let Some(spec) = flags.generator.as_deref() else {\n'
     '            return pipeline::resolve(intent, &MockSynth::new(), GenMode::Auto, risk_floor).map_err(Produce::Refused);\n'
     '            return Err(Produce::Usage(',
     'axon-intent', '--test cli_generate', 'no_program_and_no_generator_exits_2_naming_the_flag'),
    ('M5106', 'EG-4 I-D1: a lift report that cannot be written is ignored and compile goes on', _ICLI,
     '            if let Err(e) = std::fs::write(path, text) {',
     '            if let Err(e) = std::fs::write(path, text).or_else(|_| Ok::<(), std::io::Error>(())) {',
     'axon-intent', '--test cli_generate', 'an_unwritable_lift_transcript_exits_2_with_no_triple'),
    ('M5107', 'EG-4 I-D1: a generation transcript that cannot be written is ignored and compile goes on', _ICLI,
     '            if let Err(e) = std::fs::write(path, out.transcript.to_json()) {',
     '            if let Err(e) = std::fs::write(path, out.transcript.to_json()).or_else(|_| Ok::<(), std::io::Error>(())) {',
     'axon-intent', '--test cli_generate', 'an_unwritable_generation_transcript_exits_2_with_no_triple'),
    ('M5108', 'EG-4 I-D1: verify_triple admits a triple with no approval token (require_approval = false)', _ICLI,
     '    if auth.token.is_none() {', '    if false && auth.token.is_none() {',
     'axon-intent', '--lib', 'cli::tests::a_triple_with_no_approval_token_is_refused'),
    ('M5109', 'EG-4 I-D1: an unrecognized invocation exits 0', _ICLI,
     '            eprintln!("axon-intent: unrecognized invocation\\n\\n{USAGE}");\n            ExitCode::from(2)',
     '            eprintln!("axon-intent: unrecognized invocation\\n\\n{USAGE}");\n            ExitCode::from(0)',
     'axon-intent', '--test cli_generate', 'an_unrecognized_invocation_exits_2'),
    ('M5110', 'EG-4 I-D1: produce_failed reports a refusal with exit 0', _ICLI,
     '            ExitCode::from(r.exit_code() as u8)', '            ExitCode::from(0)',
     'axon-intent', '--test cli_generate', 'a_lifted_program_failing_its_example_is_refused'),
    ('M5111', 'EG-4 I-D1: compile exits 0 when no program was produced', _ICLI,
     '        Err(p) => return produce_failed("compile", p),', '        Err(_) => return ExitCode::from(0),',
     'axon-intent', '--test cli_generate', 'a_lifted_program_failing_its_example_is_refused'),
    ('M5112', 'EG-4 I-D1: approve --reject records an approval', _ICLI,
     '                decision = Decision::Rejected;', '                decision = Decision::Approved;',
     'axon-intent', '--test cli_generate', 'approve_reject_writes_no_approval_and_exits_3'),
]

# ---- [mk] --------------------------------------------------------------------------------------------
# enable-gaps EG-4 lane I-D1 (ids M5090-M5112, block M5090-M5129)
ATTACK_MARKERS.update({
    'M5090': r'ATTACK: a draft that does not typecheck was not refused as does-not-typecheck',
    'M5091': r'ATTACK: a draft with no entry point was not refused as no-entry-point',
    'M5092': r'ATTACK: an empty body was not refused as empty-or-stub-body',
    'M5093': r'ATTACK: a draft writing an unasked output was not refused as outputs-mismatch',
    'M5094': r'ATTACK: a program writing an unasked output beside the asked one shipped',
    'M5095': r'ATTACK: a program writing only part of the asked outputs earned the outputs match',
    'M5096': r'ATTACK: a deduction refused a draft on its own',
    'M5097': r'ATTACK: a deduction refused a draft on its own',
    'M5098': r'ATTACK: a deduction refused a draft on its own',
    'M5099': r'ATTACK: the rubric weight W_OUTPUTS_MATCH is not 15',
    'M5100': r'ATTACK: the rubric weight W_NO_UNCERTAINTY is not 15',
    'M5101': r'ATTACK: the rubric weight W_EXAMPLES is not 10',
    'M5102': r'ATTACK: the deduction for unmatched outputs is not named outputs-not-matched',
    'M5103': r'ATTACK: the deduction for uncertainty notes is not named uncertainty-notes',
    'M5104': r'ATTACK: the deduction for missing examples is not named no-examples',
    'M5105': r'ATTACK: compile without a program source fell back to another source',
    'M5106': r'ATTACK: the lift report could not be written and compile went on',
    'M5107': r'ATTACK: the generation transcript could not be written and compile went on',
    'M5108': r'ATTACK: a triple with no approval token verified',
    'M5109': r'ATTACK: an unrecognized invocation did not exit 2',
    'M5110': r'ATTACK: an author program failing its example was compiled',
    'M5111': r'ATTACK: an author program failing its example was compiled',
    'M5112': r'ATTACK: approve --reject wrote an approval',
})

# ---- [cov] -------------------------------------------------------------------------------------------
# enable-gaps EG-4 lane I-D1 (O-NHR-04): the confidence.rs and cli.rs sites no row covers. Rows M5090-M5112.
_CONF = "crates/axon-intent/src/confidence.rs"
_ICLI = "crates/axon-intent/src/cli.rs"
EXEMPT += [
    (_CONF, "    if s < CONFIDENCE_FLOOR {",
     "UNREACHABLE (checkable): after the four hard refusals the rubric adds W_TYPECHECKS + W_ENTRY "
     "(40 + 20 = 60 = CONFIDENCE_FLOOR) unconditionally and the other weights only add, so `s < "
     "CONFIDENCE_FLOOR` holds on no path. The weakest shippable draft scores exactly the floor "
     "(confidence::tests::no_examples_is_a_named_deduction_that_never_refuses); a lower weight or a "
     "higher floor makes it refuse there (M5096, M5097, M5098)"),
    (_CONF, "fn refuse(reason: &str) -> Confidence {",
     "NOT A SITE (checkable): the body of the refusal constructor `refuse`; each of its four calls in "
     "`score` is a site of its own, rowed (M5090, M5091, M5092, M5093)"),
    (_ICLI, "                _ => Err(format!(",
     "USAGE (checkable): `--max-rounds` above genloop::MAX_REPAIR_ROUNDS (3) or not a u32; compile/emit "
     "exit 2 before an intent is read, so nothing is generated, run or written. Admitted, it could not "
     "widen the loop either: genloop::run takes `cfg.max_repair_rounds.min(MAX_REPAIR_ROUNDS)` "
     "(genloop.rs:249)"),
    (_ICLI, "    if mock && flags.generator.is_some() {",
     "USAGE (checkable): `--generator` together with AXON_AI_MOCK is a contradictory invocation, refused "
     "before any source is produced (exit 2; nothing generated, run or written). Without it the "
     "operator's own AXON_AI_MOCK opt-in selects the deterministic offline MockSynth, whose draft "
     "passes the same pipeline::resolve gates (check, grant, admissible, confidence); no generator is "
     "spawned"),
    (_ICLI, "            .map_err(Produce::Refused);",
     "NOTHING TO ADMIT (checkable): pipeline::resolve (the AXON_AI_MOCK path) returned a Refusal and no "
     "Resolved; the map_err only re-wraps it for produce_failed, which prints it and exits with "
     "Refusal::exit_code (refusal.rs, never 0; M5110). Admitting here would have to invent the program"),
    (_ICLI, "            pipeline::resolve_draft(intent, resolved.draft, risk_floor).map_err(Produce::Refused)",
     "NOTHING TO ADMIT (checkable): pipeline::resolve_draft (re-gating the produced draft under the "
     "--risk floor) returned a Refusal and no Resolved; the map_err only re-wraps it for "
     "produce_failed (exit Refusal::exit_code, never 0; M5110). Admitting here would have to invent "
     "the program"),
    (_ICLI, "fn help(line: &str) -> ExitCode {",
     "NOT A SITE (checkable): prints one usage line and returns ExitCode 0; its only callers are the "
     "`<cmd> --help` arms of `run`, which run, approve and write nothing"),
    (_ICLI, "fn cmd_review(rest: &[&str]) -> ExitCode {",
     "NOTHING TO ADMIT (checkable): `review` prints the legible approval request and returns; its body "
     "writes, spawns and signs nothing (load_job reads the .axjob and the program). Each non-zero exit "
     "is a missing or malformed argument or an unreadable/invalid .axjob (USAGE). The decision is "
     "`approve`'s (M5112) and the token check verify_triple's (M5108, M3625)"),
]

# ---- [nys] -------------------------------------------------------------------------------------------
# Delete from NOT_YET_SCANNED:
#   "crates/axon-intent/src/cli.rs": 16,
#   "crates/axon-intent/src/confidence.rs": 21,
