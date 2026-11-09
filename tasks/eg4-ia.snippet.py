# enable-gaps EG-4 lane I-A (ids M4910-M4925, block M4910-M4969)
#
# Refusal coverage of crates/axon-intent/src/authorize.rs (49 sites at e217a759): 20 sites by 16 rows
# (the predicate heads export_verify_key, export_run_record_key and init_private_dir ride on the rows in
# their bodies; the form and value sites of the preview's `.stdin` ride on M4918), 26 EXEMPT entries,
# 1 VALUE_EXEMPT entry, 2 REMAINDER (const_tag: PREVIEW_SCHEMA, ARGV_SHAPE). No row's `find` covers
# authorize.rs:38 or :110-113 (lane N's EG-1 lines); the :38 exemption anchors on the text lane N keeps.
# Tests: crates/axon-intent/tests/authorize_repair.rs (four new, three ATTACK messages added to
# authorize_repair_mints_a_verifiable_issued_envelope_from_the_preview).
#
# Three sections, one per registry script; the integrator splices each where EG-4 Fix says.

# ──── into scripts/v022_g01_mutations.py (after the last MUTATIONS block) ────────────────────────────
_EG4IA_AR = 'crates/axon-intent/src/authorize.rs'
_EG4IA_T = 'axon-intent'
MUTATIONS += [
    ('M4910', 'EG-4 I-A: gate_admits admits a gate with a row that is not `admit`', _EG4IA_AR,
     '    if refused.is_empty() {',
     '    if true {',
     _EG4IA_T, '--lib', 'authorize::tests::the_gate_admits_only_all_admit_rows'),
    ('M4911', 'EG-4 I-A: every authorize-repair refusal exits 0', _EG4IA_AR,
     '    ExitCode::from(code)\n',
     '    ExitCode::from(code & 0)\n',
     _EG4IA_T, '--test authorize_repair', 'a_refused_gate_row_mints_nothing'),
    ('M4912', 'EG-4 I-A: --export-verify-key ignores a failed (existing-file) write and reports success', _EG4IA_AR,
     '    if let Err(e) = write_new(path, format!("{hex}\\n").as_bytes()) {',
     '    if let Err(e) = write_new(path, format!("{hex}\\n").as_bytes()).or(Ok::<(), std::io::Error>(())) {',
     _EG4IA_T, '--test authorize_repair', 'export_verify_key_writes_the_public_half_private_and_once'),
    ('M4913', 'EG-4 I-A: --export-run-record-key ignores a failed (existing-file) write and reports success', _EG4IA_AR,
     '    if let Err(e) = write_new(path, format!("{}\\n", key.to_hex()).as_bytes()) {',
     '    if let Err(e) = write_new(path, format!("{}\\n", key.to_hex()).as_bytes()).or(Ok::<(), std::io::Error>(())) {',
     _EG4IA_T, '--test authorize_repair', 'export_run_record_key_writes_the_subkey_private_and_once'),
    ('M4914', "EG-4 I-A: an init the store/ledger refused (already initialised) exits 0", _EG4IA_AR,
     '        Err(e) => fail(2, &e),\n    }\n}',
     '        Err(e) => fail(0, &e),\n    }\n}',
     _EG4IA_T, '--test authorize_repair', 'init_spent_store_creates_a_private_store_once'),
    ('M4915', 'EG-4 I-A: a missing required mint flag is not refused before the cortex preview runs', _EG4IA_AR,
     '        if missing {',
     '        if false && missing {',
     _EG4IA_T, '--test authorize_repair', 'a_mint_missing_a_required_flag_runs_no_preview'),
    ('M4916', 'EG-4 I-A: the preview is asked of another cortex verb than `grant`', _EG4IA_AR,
     '        .args(["grant", "--envelope-preview"])',
     '        .args(["repair", "--envelope-preview"])',
     _EG4IA_T, '--test authorize_repair', 'authorize_repair_mints_a_verifiable_issued_envelope_from_the_preview'),
    ('M4917', 'EG-4 I-A: the preview is asked without `--envelope-preview`', _EG4IA_AR,
     '        .args(["grant", "--envelope-preview"])',
     '        .args(["grant", "--preview"])',
     _EG4IA_T, '--test authorize_repair', 'authorize_repair_mints_a_verifiable_issued_envelope_from_the_preview'),
    ('M4918', "EG-4 I-A: the cortex preview child inherits the minter's stdin", _EG4IA_AR,
     '        .stdin(std::process::Stdio::null())',
     '        .stdin(std::process::Stdio::inherit())',
     _EG4IA_T, '--test authorize_repair', 'the_preview_child_gets_no_stdin'),
    ('M4919', 'EG-4 I-A: a cortex binary that changed (or became unreadable) while its preview ran is minted from', _EG4IA_AR,
     '        _ => {\n            return fail(\n                2,\n                &format!("{} changed while its preview ran", cortex.display()),\n            )\n        }',
     '        _ => {}',
     _EG4IA_T, '--test authorize_repair', 'a_cortex_binary_changed_during_its_preview_mints_nothing'),
    ('M4920', 'EG-4 I-A: the signed envelope binds another id than the run ledger it records into', _EG4IA_AR,
     '        store_id.clone(),\n        ledger_id,\n        now,',
     '        store_id.clone(),\n        store_id.clone(),\n        now,',
     _EG4IA_T, '--test authorize_repair', 'authorize_repair_mints_a_verifiable_issued_envelope_from_the_preview'),
    ('M4921', 'EG-4 I-A: a gate refusal exits 0', _EG4IA_AR,
     '        Err(Refused::Gate(e)) => return fail(8, &e),',
     '        Err(Refused::Gate(e)) => return fail(0, &e),',
     _EG4IA_T, '--test authorize_repair', 'a_refused_gate_row_mints_nothing'),
    ('M4922', 'EG-4 I-A: a refused (malformed) mint request exits 0', _EG4IA_AR,
     '        Err(Refused::Bad(e)) => return fail(2, &e),',
     '        Err(Refused::Bad(e)) => return fail(0, &e),',
     _EG4IA_T, '--test authorize_repair', 'a_ttl_above_the_max_mints_nothing'),
    ('M4923', 'EG-4 I-A: a mint record that cannot be written does not stop the issue', _EG4IA_AR,
     '    if let Err(e) = write_new(&mint_path, format!("{record}\\n").as_bytes()) {',
     '    if let Err(e) = write_new(&mint_path, format!("{record}\\n").as_bytes()).or(Ok::<(), std::io::Error>(())) {',
     _EG4IA_T, '--test authorize_repair', 'an_out_that_cannot_be_written_issues_nothing'),
    ('M4924', 'EG-4 I-A: the mint record names another schema than axon-intent-mint/1', _EG4IA_AR,
     'pub const MINT_RECORD_SCHEMA: &str = "axon-intent-mint/1";',
     'pub const MINT_RECORD_SCHEMA: &str = "axon-intent-mint/2";',
     _EG4IA_T, '--test authorize_repair', 'authorize_repair_mints_a_verifiable_issued_envelope_from_the_preview'),
    ('M4925', "EG-4 I-A: BUDGET_FLAGS binds --max-proposals' value to deadline_ms and the reverse", _EG4IA_AR,
     '    "--max-proposals",\n    "--deadline-ms",',
     '    "--deadline-ms",\n    "--max-proposals",',
     _EG4IA_T, '--test authorize_repair', 'authorize_repair_mints_a_verifiable_issued_envelope_from_the_preview'),
]

# ──── into scripts/v022_attack_markers.py (after the last ATTACK_MARKERS.update) ─────────────────────
# enable-gaps EG-4 lane I-A (ids M4910-M4925, block M4910-M4969)
ATTACK_MARKERS.update({
    'M4910': r'ATTACK: a gate row that is not exactly admit was admitted',
    'M4911': r'ATTACK: an envelope was minted over a refused gate row',
    'M4912': r'ATTACK: an existing verify-key file was overwritten',
    'M4913': r'ATTACK: an existing run-record key file was overwritten',
    'M4914': r'ATTACK: an initialised store was initialised again',
    'M4915': r'ATTACK: the cortex preview ran for a mint command line without --by',
    'M4916': r'ATTACK: the preview was not asked of `cortex grant --envelope-preview`',
    'M4917': r'ATTACK: the preview was not asked of `cortex grant --envelope-preview`',
    'M4918': r"ATTACK: the cortex preview child read the minter's stdin",
    'M4919': r'ATTACK: an envelope was minted by a cortex binary that changed while it ran',
    'M4920': r'ATTACK: the envelope does not bind the run ledger it records into',
    'M4921': r'ATTACK: an envelope was minted over a refused gate row',
    'M4922': r'ATTACK: an envelope living past MAX_TTL_S was minted',
    'M4923': r'ATTACK: an envelope was issued for an --out that cannot be written',
    'M4924': r'ATTACK: the mint record does not name its schema axon-intent-mint/1',
    'M4925': r"ATTACK: a budget flag's value was bound to another budget field",
})

# ──── into scripts/v022_refusal_coverage.py (before `def load_rows`) ────────────────────────────────
# enable-gaps EG-4 lane I-A: authorize.rs's sites that carry no row (rows M4910-M4925).
_EG4IA_AR = 'crates/axon-intent/src/authorize.rs'
_EG4IA_NTA = _NTA + "; authorize-repair then prints the refusal and exits 2: nothing is exported, initialised, signed, issued or written"
EXEMPT += [
    # run
    (_EG4IA_AR, 'pub fn run(rest: &[&str]) -> ExitCode {',
     "NOT A SITE: authorize-repair's argv dispatch (`--help`, the four one-path forms, else parse_args then mint); "
     "each target is judged at its own sites (rows M4912-M4925, the exemptions below)"),
    (_EG4IA_AR, '            Err(e) => fail(2, &e),\n        },',
     _EG4IA_NTA + " (parse_args refused the command line: there are no Args, so mint cannot run)"),
    # export_verify_key / export_run_record_key
    (_EG4IA_AR, '        Err(e) => return fail(2, &e),\n    };\n    let hex = key.verify_key().to_hex();',
     _EG4IA_NTA + " (MintKey::from_env refused AXON_APPROVAL_KEY: there is no key to export; "
     "export_verify_key_without_the_operator_key_writes_nothing)"),
    (_EG4IA_AR, '        Err(e) => return fail(2, &e),\n    };\n    if let Err(e) = write_new(path, format!("{}\\n", key.to_hex())',
     _EG4IA_NTA + " (SealKey::from_env refused AXON_APPROVAL_KEY: there is no run-record key to export; "
     "export_run_record_key_writes_the_subkey_private_and_once's no-key half)"),
    # init_private_dir and its two wrappers
    (_EG4IA_AR, '    if let Err(e) = std::fs::DirBuilder::new().mode(0o700).create(dir) {',
     "ENSURE-EXISTS (checkable): makes the spent store or run ledger directory when absent and tolerates "
     "AlreadyExists; the decision is `init(dir, euid())` on the lines below (repair_envelope::init_store, "
     "run_ledger::init_ledger), whose private_path::check_private refuses a path that is a symlink, not a "
     "directory, not owned by the euid or open to group/other (`m.mode() & 0o077 != 0`), and which refuses "
     "a directory that already has an id (M4914)"),
    (_EG4IA_AR, '        if e.kind() != std::io::ErrorKind::AlreadyExists {',
     "DOMINATED (checkable): `init(dir, euid())` runs next in every case, and check_private's "
     "`symlink_metadata(path)` refuses a directory that does not exist (\"(it must exist)\"); a create failure "
     "that left a directory behind is judged by check_private like any existing one, so ignoring this error "
     "changes the message, never the outcome"),
    (_EG4IA_AR, 'fn init_spent_store(dir: &Path) -> ExitCode {',
     "NOT A SITE: a one-line wrapper, init_private_dir(dir, repair_envelope::init_store, ..); judged at "
     "init_private_dir's sites (M4914 and the two exemptions above)"),
    (_EG4IA_AR, 'fn init_run_ledger(dir: &Path) -> ExitCode {',
     "NOT A SITE: a one-line wrapper, init_private_dir(dir, run_ledger::init_ledger, ..); judged at "
     "init_private_dir's sites (M4914 and the two exemptions above)"),
    # parse_args
    (_EG4IA_AR, '            return Err(format!("`{flag}` needs a value (see --help)"));', _USE),
    (_EG4IA_AR, '                None => return Err(format!("unknown argument `{f}` (see --help)")),', _USE),
    (_EG4IA_AR, '            return Err(format!("{b} is required (see --help)"));',
     "DOMINATED (checkable), a pair with authorize.rs mint's `a budget flag is missing` let-else: that "
     "let-else (`let [Some(max_proposals), .., Some(max_steps)] = a.budget else { return fail(2, ..) }`) "
     "refuses the same missing budget flag before anything is signed, issued or written, and a zero budget "
     "is refused again by repair_envelope::validate (\"every limit is at least 1\"); the pair is shown "
     "refusing by authorize_repair `a_missing_budget_flag_mints_nothing`"),
    # mint
    (_EG4IA_AR, '        Err(e) => return fail(2, &e),\n    };\n    let (Some(cortex), Some(store)',
     _EG4IA_NTA + " (MintKey::from_env refused AXON_APPROVAL_KEY: there is no key to sign with; "
     "minting_without_the_operator_key_mints_nothing)"),
    (_EG4IA_AR, '        return fail(\n            2,\n            "missing --cortex, --spent-store',
     "DOMINATED (checkable): mint is called only from run's `Ok(a) => mint(&a)`, and parse_args's "
     "required-flag loop (`if missing {`, rowed: M4915) refuses each of these six flags first, so the "
     "let-else is reached only if that loop is removed"),
    (_EG4IA_AR, '        Err(e) => return fail(2, &e),\n    };\n    let ledger_id = match run_ledger::ledger_id(ledger, me) {',
     _EG4IA_NTA + " (repair_envelope::store_id refused --spent-store: there is no store id to bind or issue into)"),
    (_EG4IA_AR, '        Err(e) => return fail(2, &e),\n    };\n    // Security F6: what the check pins may never reach, canonical.',
     _EG4IA_NTA + " (run_ledger::ledger_id refused --run-ledger: there is no ledger id to bind; "
     "a_mint_into_an_uninitialised_run_ledger_mints_nothing)"),
    (_EG4IA_AR, '            Err(e) => return fail(2, &format!("{flag} {}: {e}", p.display())),',
     _EG4IA_NTA + " (the guarded path does not resolve, so there is no canonical path for the F6 overlap check)"),
    (_EG4IA_AR, '        Err(e) => return fail(2, &format!("--cortex {}: {e}", cortex.display())),',
     _EG4IA_NTA + " (--cortex does not resolve: there is no binary to run and record)"),
    (_EG4IA_AR, '        Err(e) => return fail(2, &format!("cannot read {}: {e}", cortex.display())),',
     _EG4IA_NTA + " (the cortex binary cannot be read: there is no sha256 to record)"),
    (_EG4IA_AR, '        Err(e) => return fail(2, &format!("cannot run {}: {e}", cortex.display())),',
     _EG4IA_NTA + " (the preview could not be spawned: there is no output to judge)"),
    (_EG4IA_AR, '        Err(_) => return fail(2, "cortex preview: output is not UTF-8"),',
     _EG4IA_NTA + " (the preview's stdout is not text: there is no document to parse)"),
    (_EG4IA_AR, '        Err(e) => return fail(2, &e),\n    };\n    let envelope_id = match repair_envelope::new_id() {',
     _EG4IA_NTA + " (parse_preview refused the document: there is no Preview to judge; a_malformed_preview_mints_nothing)"),
    (_EG4IA_AR, '        Err(e) => return fail(2, &e),\n    };\n    let [Some(max_proposals)',
     _IO + " (repair_envelope::new_id: the OS random source failed; there is no envelope id)"),
    (_EG4IA_AR, '        return fail(2, "a budget flag is missing");',
     "DOMINATED (checkable), a pair with authorize.rs parse_args's `{b} is required` loop over BUDGET_FLAGS: "
     "that loop refuses every missing budget flag, and mint is called only from run's `Ok(a) => mint(&a)`, "
     "so this let-else is reached only if the loop is removed; the pair is shown refusing by "
     "authorize_repair `a_missing_budget_flag_mints_nothing`"),
    (_EG4IA_AR, '        Err(e) => return fail(2, &e),\n    };\n    let verified = match repair_envelope::verify(',
     _EG4IA_NTA + " (MintKey::sign failed: there is no signed envelope)"),
    (_EG4IA_AR, '        Err(e) => return fail(2, &format!("the minted envelope does not verify: {e}")),',
     _EG4IA_NTA + " (the just-signed envelope does not verify: there is no verified digest to record)"),
    (_EG4IA_AR, '    if let Err(e) = write_new(out, format!("{json}\\n").as_bytes()) {',
     _IO + " (reached only when `<out>.mint`, in the same directory, was just written (M4923) and `out`, found "
     "absent above (M4313), cannot be created: a race or an OS error; the issued envelope has no file and "
     "cannot be used, and the refusal names its id)"),
    # constants
    (_EG4IA_AR, 'pub const PREVIEW_SCHEMA: &str = ', _A95['const_tag']),
    (_EG4IA_AR, 'pub const ARGV_SHAPE: &str = "axon-test-local/1";', _A95['const_tag']),
]
VALUE_EXEMPT += [
    (_EG4IA_AR, 'init_private_dir', 1, '0o700', 'DOMINATED',
     "DOMINATED (checkable): the directory this mode creates is judged by `init(dir, euid())` next "
     "(repair_envelope::init_store, run_ledger::init_ledger -> private_path::check_private), which refuses "
     "any group/other bit (`m.mode() & 0o077 != 0`): a wider mode makes init refuse, never admit"),
]
NOT_YET_SCANNED.pop("crates/axon-intent/src/authorize.rs")
