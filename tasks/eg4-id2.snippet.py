# enable-gaps EG-4 lane I-D2 snippet (audit_runs.rs). Spliced by the integrator, then deleted.
# Four parts, each marked with the script it goes into:
#   [g01]  scripts/v022_g01_mutations.py   after the last `MUTATIONS += [...]` block, before `if __name__`
#   [mk]   scripts/v022_attack_markers.py  after the last `ATTACK_MARKERS.update({...})`
#   [cov]  scripts/v022_refusal_coverage.py  before `def load_rows():`
#   [nys]  scripts/v022_refusal_coverage.py  NOT_YET_SCANNED: delete this key
#
# Judged on eg/int 1f087fc4 (lane N's EG-1 merged): audit_runs.rs 16 sites, 8 by 8 rows, 8 exempt.
# :112 (schema neither /5, /4 nor a mint record) is re-judged after EG-1: DOMINATED by verify_for_audit's
# accept list, which is the same two schemas (repair_envelope.rs AUDIT_ENVELOPE_SCHEMAS).
# Lane N's amended M4390 (verify_for_audit) and new M4894 cover no audit_runs.rs site; M4391 covers :250.

# ---- [g01] -------------------------------------------------------------------------------------------
# enable-gaps EG-4 lane I-D2 (ids M5130-M5137, block M5130-M5149): axon-intent audit-runs. parse_args'
# repeated and required flags, load_envelopes' symlink and one-id refusals, an unreadable --envelopes
# skipped, the usage exit code, and the mint-record skip widened to every envelope.
_AUR = 'crates/axon-intent/src/audit_runs.rs'
MUTATIONS += [
    ('M5130', 'EG-4 I-D2: a repeated audit-runs flag is accepted (the last value wins)', _AUR,
     '        if slot.replace(PathBuf::from(val)).is_some() {',
     '        if slot.replace(PathBuf::from(val)).is_some() && false {',
     'axon-intent', '--lib', 'audit_runs::tests::audit_runs_needs_both_directories'),
    ('M5131', 'EG-4 I-D2: audit-runs parses without --spent-store', _AUR,
     '    if a.spent.is_none() {', '    if false && a.spent.is_none() {',
     'axon-intent', '--lib', 'audit_runs::tests::audit_runs_needs_both_directories'),
    ('M5132', 'EG-4 I-D2: audit-runs parses without --run-ledger', _AUR,
     '    if a.ledger.is_none() {', '    if false && a.ledger.is_none() {',
     'axon-intent', '--lib', 'audit_runs::tests::audit_runs_needs_both_directories'),
    ('M5133', 'EG-4 I-D2: a symlink in --envelopes is followed', _AUR,
     '        if !m.is_file() {', '        if false && !m.is_file() {',
     'axon-intent', '--test audit_runs', 'a_symlink_in_envelopes_is_refused'),
    ('M5134', 'EG-4 I-D2: two different envelopes with one id are read last-wins', _AUR,
     '            if old != ve.digest {', '            if false && old != ve.digest {',
     'axon-intent', '--test audit_runs', 'two_different_envelopes_with_one_id_are_refused'),
    ('M5135', 'EG-4 I-D2: an --envelopes directory that does not load is audited as no envelopes', _AUR,
     '                Err(e) => return fail(&e),\n            }\n        }\n    };',
     '                Err(_) => BTreeMap::new(),\n            }\n        }\n    };',
     'axon-intent', '--test audit_runs', 'usage_and_unreadable_inputs_exit_2'),
    ('M5136', 'EG-4 I-D2: an audit-runs refusal exits 0', _AUR,
     '    eprintln!("axon-intent audit-runs: {msg}");\n    ExitCode::from(2)',
     '    eprintln!("axon-intent audit-runs: {msg}");\n    ExitCode::from(0)',
     'axon-intent', '--test audit_runs', 'usage_and_unreadable_inputs_exit_2'),
    ('M5137', 'EG-4 I-D2: the mint-record skip names the envelope schema, so every envelope is skipped unverified', _AUR,
     'const MINT_RECORD_SCHEMA: &str = crate::authorize::MINT_RECORD_SCHEMA;',
     'const MINT_RECORD_SCHEMA: &str = repair_envelope::ENVELOPE_SCHEMA;',
     'axon-intent', '--test audit_runs', 'a_supplied_envelope_the_records_do_not_name_is_a_finding'),
]

# ---- [mk] --------------------------------------------------------------------------------------------
# enable-gaps EG-4 lane I-D2 (ids M5130-M5137, block M5130-M5149)
ATTACK_MARKERS.update({
    'M5130': r'ATTACK: a repeated \-\-spent-store was accepted',
    'M5131': r'ATTACK: an audit-runs without \-\-spent-store was parsed',
    'M5132': r'ATTACK: an audit-runs without \-\-run-ledger was parsed',
    'M5133': r'ATTACK: a symlink in \-\-envelopes was followed',
    'M5134': r'ATTACK: two different envelopes with one id were audited',
    'M5135': r'ATTACK: an unverifiable file in \-\-envelopes was skipped',
    'M5136': r'ATTACK: an audit ran without the operator key',
    'M5137': r'ATTACK: records naming a different envelope digest audited clean',
})

# ---- [cov] -------------------------------------------------------------------------------------------
# enable-gaps EG-4 lane I-D2 (O-NHR-04): the audit_runs.rs sites no row covers. Rows M5130-M5137.
_AUR = "crates/axon-intent/src/audit_runs.rs"
EXEMPT += [
    (_AUR, '            return Err(format!("`{flag}` needs a value (see --help)"));', _USE),
    (_AUR, '            f => return Err(format!("unknown argument `{f}` (see --help)")),', _USE),
    (_AUR, "        if schema != repair_envelope::ENVELOPE_SCHEMA",
     "DOMINATED (checkable): the next statement verifies the file with repair_envelope::verify_for_audit, "
     "whose verify_schemas refuses any schema outside AUDIT_ENVELOPE_SCHEMAS = [ENVELOPE_SCHEMA, "
     "\"axon-approval/4\"], the same two schemas this guard admits (ENVELOPE_SCHEMA and "
     "RETIRED_ENVELOPE_SCHEMAS); that Err is mapped to `not an envelope this operator signed` and returned, "
     "so audit-runs exits 2 either way. A verify_for_audit that read past it is rowed: M4894, M4390"),
    (_AUR, "        Err(e) => return fail(&e),\n    };\n    let (Some(spent), Some(ledger))",
     "NOTHING TO ADMIT (checkable): parse_args returned an Err and no Args; without the directories there "
     "is no ledger or store to audit (exit 2 by fail, M5136)"),
    (_AUR, '        return fail("--spent-store and --run-ledger are required");',
     "DOMINATED (checkable): parse_args returned Ok, and it refuses an Args with no --spent-store (M5131) or "
     "no --run-ledger (M5132), so both are Some here and this let-else never takes its else"),
    (_AUR, "        Err(e) => return fail(&e),\n    };\n    let envelopes = match",
     "NOTHING TO ADMIT (checkable): SealKey::from_env failed (AXON_APPROVAL_KEY absent or malformed), so "
     "there is no run-record key to check a seal with; tests/audit_runs.rs usage_and_unreadable_inputs_exit_2 "
     "shows the keyless audit exits 2 with no output (M5136)"),
    (_AUR, "                Err(e) => return fail(&e),\n            };\n            match load_envelopes",
     "NOTHING TO ADMIT (checkable): MintKey::from_env failed, so there is no verify key and no supplied "
     "envelope can be verified; reachable only after SealKey::from_env read the same AXON_APPROVAL_KEY"),
    (_AUR, "        Err(e) => return fail(&e),\n    };\n    print(",
     "NOTHING TO ADMIT (checkable): run_ledger::audit returned an Err and no Audit (axon-os run_ledger.rs "
     "`pub struct Audit` derives no Default); tests/audit_runs.rs usage_and_unreadable_inputs_exit_2 shows a "
     "missing directory and a ledger others can reach exit 2"),
]

# ---- [nys] -------------------------------------------------------------------------------------------
# Delete from NOT_YET_SCANNED:
#   "crates/axon-intent/src/audit_runs.rs": 16,
