# enable-gaps EG-4 lane I-E snippet (generator, policy, grant_infer, emit, synth, admit, approval, pipeline,
# refusal; main/lib have no uncovered site). Spliced by the integrator, then deleted.
# Four parts, each marked with the script it goes into:
#   [g01]  scripts/v022_g01_mutations.py     after the last `MUTATIONS += [...]` block, before `if __name__`
#   [mk]   scripts/v022_attack_markers.py    after the last `ATTACK_MARKERS.update({...})`
#   [cov]  scripts/v022_refusal_coverage.py  before `def load_rows():`
#   [nys]  scripts/v022_refusal_coverage.py  NOT_YET_SCANNED: delete these keys
#
# Judged: 42 sites. generator.rs 16 (15 by 10 rows, 1 exempt), policy.rs 12 (VALUE_EXEMPT NOTROUTE), grant_infer.rs 4
# (4 rows), emit.rs 3 (exempt: 2 OS ERROR, 1 value DOMINATED), synth.rs 3 (3 rows), admit/approval/pipeline/refusal 1
# each (1 row each). REMAINDER: none. generator.rs:212-213 (`cfg(feature = "ai")`) are rows: their target
# `--features ai --test refusal_e` builds and runs offline (a loopback listener; the mock off, no key, then a fake key).

# ---- [g01] -------------------------------------------------------------------------------------------
# enable-gaps EG-4 lane I-E (ids M5150-M5170, block M5150-M5209): axon-intent's generator seam (spec parsing, the
# `cmd:` answer's exit/cap arms, the `ai:` arms, the tokens line), the least-privilege grant inference, the
# parse-failure effects, the mock type check, admissibility, `--risk` parsing, the confidence short-circuit and the
# refusal exit code.
_IGEN = 'crates/axon-intent/src/generator.rs'
_IGRI = 'crates/axon-intent/src/grant_infer.rs'
_ISYN = 'crates/axon-intent/src/synth.rs'
MUTATIONS += [
    ('M5150', 'EG-4 I-E: `--generator cmd:` with no path is accepted (an empty program path)', _IGEN,
     '        if path.is_empty() {', '        if false && path.is_empty() {',
     'axon-intent', '--lib', 'generator::tests::an_empty_or_unknown_generator_spec_is_refused'),
    ('M5151', 'EG-4 I-E: `--generator ai:` with no model name is accepted', _IGEN,
     '        if model.is_empty() {', '        if false && model.is_empty() {',
     'axon-intent', '--lib', 'generator::tests::an_empty_or_unknown_generator_spec_is_refused'),
    ('M5152', 'EG-4 I-E: an unknown generator spec runs as a command', _IGEN,
     '    Err(format!(\n        "unknown generator',
     '    if true { return Ok(Box::new(CmdGenerator::new(PathBuf::from(spec), timeout))); } Err(format!(\n        "unknown generator',
     'axon-intent', '--lib', 'generator::tests::an_empty_or_unknown_generator_spec_is_refused'),
    ('M5153', 'EG-4 I-E: a build without the ai feature runs `ai:<model>` as a command', _IGEN,
     '    Err("`--generator ai:` needs axon-intent built with `--features ai`".into())',
     '    Ok(Box::new(CmdGenerator::new(PathBuf::from(_model), _timeout)))',
     'axon-intent', '--lib', 'generator::tests::an_ai_generator_in_a_build_without_the_ai_feature_is_refused'),
    ('M5154', 'EG-4 I-E: a `cmd:` answer over the candidate cap is a (truncated) candidate', _IGEN,
     '            Some(0) if run.stdout_omitted > 0 => Err(GenFailure::Failed {',
     '            Some(0) if false && run.stdout_omitted > 0 => Err(GenFailure::Failed {',
     'axon-intent', '--lib', 'generator::tests::an_answer_over_the_candidate_cap_is_a_failure_not_a_truncated_candidate'),
    ('M5155', 'EG-4 I-E: the candidate cap widens from 256 KiB to 1 MiB', _IGEN,
     'pub const MAX_CANDIDATE_BYTES: usize = 256 * 1024;', 'pub const MAX_CANDIDATE_BYTES: usize = 1024 * 1024;',
     'axon-intent', '--lib', 'generator::tests::an_answer_over_the_candidate_cap_is_a_failure_not_a_truncated_candidate'),
    ('M5156', "EG-4 I-E: a `cmd:` generator's non-zero exit yields its stdout as a candidate", _IGEN,
     '            Some(code) => Err(GenFailure::Failed {',
     '            Some(_) if true => Ok(Generation { text: run.stdout.clone(), tokens: None }),\n'
     '            Some(code) => Err(GenFailure::Failed {',
     'axon-intent', '--lib', 'generator::tests::a_failing_cmd_generator_is_a_failure_not_a_candidate'),
    ('M5157', 'EG-4 I-E: a failed `ai:` call (axon-ai error) yields an empty candidate', _IGEN,
     '            Ok(Err(e)) => Err(GenFailure::Failed { reason: e.message }),',
     '            Ok(Err(_e)) => Ok(Generation { text: String::new(), tokens: None }),',
     'axon-intent', '--features ai --test refusal_e', 'an_ai_generator_s_failed_or_late_call_is_a_failure_not_a_candidate'),
    ('M5158', 'EG-4 I-E: an `ai:` call past its deadline yields an empty candidate', _IGEN,
     '            Err(_) => Err(GenFailure::TimedOut {\n                after_ms: self.timeout.as_millis() as u64,\n            }),',
     '            Err(_) => Ok(Generation { text: String::new(), tokens: None }),',
     'axon-intent', '--features ai --test refusal_e', 'an_ai_generator_s_failed_or_late_call_is_a_failure_not_a_candidate'),
    ('M5159', "EG-4 I-E: a `cmd:` generator's tokens line is dropped (the token budget never sees it)", _IGEN,
     '        .and_then(|n| n.trim().parse::<u64>().ok())',
     '        .and_then(|n| n.trim().parse::<u64>().ok())\n        .filter(|_| false)',
     'axon-intent', '--lib', 'generator::tests::the_last_tokens_line_a_generator_prints_is_its_count'),
    ('M5160', 'EG-4 I-E: exec is granted under a ceiling with exec none', _IGRI,
     '        if matches!(ceiling.exec, ExecPolicy::Any) {', '        if true || matches!(ceiling.exec, ExecPolicy::Any) {',
     'axon-intent', '--lib', 'grant_infer::tests::exec_needs_ceiling_any'),
    ('M5161', "EG-4 I-E: a program above the ceiling's label is granted (the label clamps down)", _IGRI,
     '    if declared.max_label > ceiling.max_label {', '    if false && declared.max_label > ceiling.max_label {',
     'axon-intent', '--lib', 'grant_infer::tests::label_above_ceiling_refused'),
    ('M5162', 'EG-4 I-E: a declared axis the ceiling grants no scope for is granted (empty)', _IGRI,
     '    if ceiling_scope.is_empty() {', '    if false && ceiling_scope.is_empty() {',
     'axon-intent', '--lib', 'grant_infer::tests::grant_exceeds_ceiling_refused'),
    ('M5163', 'EG-4 I-E: an axis the program does not use is granted the ceiling scope (not least privilege)', _IGRI,
     'fn needs(used: bool) -> bool {\n    used\n}', 'fn needs(used: bool) -> bool {\n    used || !used\n}',
     'axon-intent', '--lib', 'grant_infer::tests::grant_is_least_privilege'),
    ('M5164', 'EG-4 I-E: a source that does not parse declares no effect (instead of every effect)', _ISYN,
     '            None => DeclaredEffects::unknown(),',
     '            None => DeclaredEffects { row: axon_os::grant::EffectSet { fs_read: false, fs_write: false, '
     'net: false, exec: false }, max_label: axon_os::grant::Label::Public },',
     'axon-intent', '--lib', 'synth::tests::a_source_that_does_not_parse_declares_every_effect'),
    ('M5165', 'EG-4 I-E: any fn counts as the entry point (not only `fn main`)', _ISYN,
     '            .any(|i| matches!(i, Item::FnDef(f) if f.name == "main"))',
     '            .any(|i| matches!(i, Item::FnDef(_)))',
     'axon-intent', '--lib', 'synth::tests::a_program_without_fn_main_has_no_entry'),
    ('M5166', "EG-4 I-E: the mock's type check accepts an unclosed brace", _ISYN,
     '    depth == 0\n}', '    depth >= 0\n}',
     'axon-intent', '--lib', 'synth::tests::the_mock_type_check_refuses_unbalanced_braces'),
    ('M5167', 'EG-4 I-E: a gate denial is admitted (declared effects outside the grant ship)', 'crates/axon-intent/src/admit.rs',
     '        Admission::Deny { reason, axis } => Err(Refusal::NotAdmissible { axis, reason }),',
     '        Admission::Deny { .. } => Ok(()),',
     'axon-intent', '--lib', 'admit::tests::non_admissible_refused'),
    ('M5168', 'EG-4 I-E: an unknown --risk word reads as Low', 'crates/axon-intent/src/approval.rs',
     '            "critical" => Some(Risk::Critical),\n            _ => None,',
     '            "critical" => Some(Risk::Critical),\n            _ => Some(Risk::Low),',
     'axon-intent', '--lib', 'approval::tests::an_unknown_risk_word_is_refused_not_read_as_a_tier'),
    ('M5169', 'EG-4 I-E: a draft the confidence gate refuses goes on to grant inference and is resolved', 'crates/axon-intent/src/pipeline.rs',
     '    if let Confidence::Refuse { reason } = &confidence {',
     '    if let (true, Confidence::Refuse { reason }) = (false, &confidence) {',
     'axon-intent', '--lib', 'pipeline::tests::low_confidence_short_circuits_no_resolve'),
    ('M5170', 'EG-4 I-E: an admissibility or ceiling refusal exits 0', 'crates/axon-intent/src/refusal.rs',
     '            Refusal::NotAdmissible { .. } | Refusal::GrantExceedsCeiling { .. } => 8,',
     '            Refusal::NotAdmissible { .. } | Refusal::GrantExceedsCeiling { .. } => 0,',
     'axon-intent', '--lib', 'refusal::tests::exit_codes_are_the_carved_scheme'),
]

# ---- [mk] --------------------------------------------------------------------------------------------
# enable-gaps EG-4 lane I-E (ids M5150-M5170, block M5150-M5209).
ATTACK_MARKERS.update({
    'M5150': r'ATTACK: `--generator cmd:` with no path was accepted',
    'M5151': r'ATTACK: `--generator ai:` with no model was accepted',
    'M5152': r'ATTACK: an unknown generator spec was accepted',
    'M5153': r'ATTACK: an `ai:` generator was accepted by a build without the ai feature',
    'M5154': r'ATTACK: an answer over the candidate cap became a candidate',
    'M5155': r'ATTACK: an answer over the candidate cap became a candidate',
    'M5156': r"ATTACK: a failing generator's output became a candidate",
    'M5157': r'ATTACK: an ai: generator whose call failed yielded a candidate',
    'M5158': r'ATTACK: an ai: generator past its deadline yielded a candidate',
    'M5159': r"ATTACK: a generator's reported tokens were not counted as reported",
    'M5160': r'ATTACK: a program needing exec was granted it under a ceiling with exec none',
    'M5161': r"ATTACK: a program above the ceiling's label was granted",
    'M5162': r'ATTACK: a program needing net was granted it under a ceiling with no net hosts',
    'M5163': r'ATTACK: an axis the program does not use was granted',
    'M5164': r'ATTACK: a source that does not parse declared fewer than every effect',
    'M5165': r'ATTACK: a program with no `fn main` was read as having an entry',
    'M5166': r'ATTACK: a program with an unclosed brace type-checked under the mock',
    'M5167': r'ATTACK: a program declaring more than its grant was admitted',
    'M5168': r'ATTACK: an unknown --risk word "[a-z]*" was read as a risk tier',
    'M5169': r'ATTACK: a stub program the confidence gate refuses was resolved',
    'M5170': r'ATTACK: a NotAdmissible refusal does not exit 8',
})

# ---- [cov] -------------------------------------------------------------------------------------------
# enable-gaps EG-4 lane I-E: the sites of its files no row covers.
_IGEN = "crates/axon-intent/src/generator.rs"
_IEMIT = "crates/axon-intent/src/emit.rs"
_IPOL = "crates/axon-intent/src/policy.rs"
EXEMPT += [
    (_IGEN, "            None => Err(GenFailure::Failed {",
     "DOMINATED (checkable): run_bounded_job's `code` is None only for End::TimedOut, refused by the "
     "`run.timed_out` return above (M4036), or End::Signal, a termination signal to THIS process "
     "(axon-os runtime.rs:682-686). axon-intent's main installs OnSignal::Die (main.rs:10): the handler kills the "
     "job's group and the process dies by the same signal, so no caller reads this arm's result. A job that dies by "
     "its own signal is an exit, `status.code().unwrap_or(-1)` (runtime.rs:683), refused by the Some(code) arm "
     "(M5156; generator::tests::a_generator_killed_by_a_signal_is_a_failure_not_a_candidate)"),
    (_IEMIT, "    std::fs::create_dir_all(out_dir)?;\n    let program = out_dir.join(",
     "OS ERROR (checkable): the `?` returns the io error and write_compiled writes nothing; both callers "
     "(cli.rs:338 `compile`, cli.rs:610 `emit`) print `cannot write triple` and exit 2 (cli.rs:354-355, 613-614)"),
    (_IEMIT, "    std::fs::create_dir_all(out_dir)?;\n    let path = out_dir.join(",
     "OS ERROR (checkable): the `?` returns the io error and no approval token is written; the one caller "
     "(cli.rs:649 `emit`) prints `cannot write approval` and exits 2 (cli.rs:663-664)"),
]
_IPOL_NR = ("NOT ON THE PROTECTED ROUTE (checkable): `grep -rn 'policy_for' crates/ --include=*.rs` finds no caller "
            "outside policy.rs (only its own tests); the §3.3 friction ladder is derived, not enforced: "
            "`it does not yet enforce anything (S3)` (policy.rs:8)")
VALUE_EXEMPT += [
    (_IPOL, 'policy_for', 1, '1', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 2, '0', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 3, '0', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 4, '1', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 5, '10_000', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 6, '100', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 7, '2', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 8, '1_000', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 9, '10', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 10, '3', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 11, '100', 'NOTROUTE', _IPOL_NR),
    (_IPOL, 'policy_for', 12, '1', 'NOTROUTE', _IPOL_NR),
    (_IEMIT, 'build_manifest', 1, 'false', 'DOMINATED',
     "DOMINATED (checkable): false is the manifest's own default (axon-os manifest.rs:251 "
     "`require_approval.unwrap_or(false)`), and the one other value, true, adds a refusal (axon-os approval.rs:843) "
     "and admits nothing. A job policy is the operator's to state (emit.rs:28-29); axon-intent's own verify refuses "
     "a triple with no approval token whatever this value (cli.rs:698-702)"),
]

# ---- [nys] -------------------------------------------------------------------------------------------
# Delete from NOT_YET_SCANNED:
#   "crates/axon-intent/src/admit.rs": 1,
#   "crates/axon-intent/src/approval.rs": 1,
#   "crates/axon-intent/src/emit.rs": 3,
#   "crates/axon-intent/src/generator.rs": 16,
#   "crates/axon-intent/src/grant_infer.rs": 4,
#   "crates/axon-intent/src/lib.rs": 0,
#   "crates/axon-intent/src/main.rs": 0,
#   "crates/axon-intent/src/pipeline.rs": 1,
#   "crates/axon-intent/src/policy.rs": 12,
#   "crates/axon-intent/src/refusal.rs": 1,
#   "crates/axon-intent/src/synth.rs": 3,
