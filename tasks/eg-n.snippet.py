# enable-gaps lane N: ids M4890-M4909 all used (EG-1 M4890-M4897; EG-2 M4898-M4909).
# Integrator: splice section 1 into scripts/v022_g01_mutations.py (after the last MUTATIONS block,
# before `if __name__`), section 2 into scripts/v022_attack_markers.py (after the last ATTACK_MARKERS
# block), section 3 replaces the eleven amended rows IN PLACE in scripts/v022_g01_mutations.py (their
# markers are unchanged), section 4 goes before `def load_rows` in scripts/v022_refusal_coverage.py.
# No NOT_YET_SCANNED edits (lanes I-A/I-D own authorize.rs/audit_runs.rs keys).

# ── 1. scripts/v022_g01_mutations.py: new rows ───────────────────────────────
# enable-gaps EG-1 (ids M4890-M4897, plus EG-2's M4898-M4899; block M4890-M4899): the envelope pins the AI endpoint's
# host:port (O-NHR-08); schema axon-approval/5, /4 refused at phase A, still read by audit-runs.
MUTATIONS += [
    ('M4890', 'EG-1: endpoint_authority returns the bare host', 'crates/axon-ai/src/lib.rs',
     '    Ok(format!("{}:{port}", host_of(u)))',
     '    let _ = port;\n    Ok(host_of(u))',
     'axon-ai', '--lib', 'host_of_url_tests::the_endpoint_authority_carries_the_effective_port'),
    ('M4891', "EG-1: check_generator compares the endpoint's host part only", 'crates/axon-os/src/repair_envelope.rs',
     '            ("endpoint", g.endpoint_host.as_str(), endpoint),',
     '            (\n                "endpoint",\n                g.endpoint_host.rsplit_once(\':\').map_or(g.endpoint_host.as_str(), |h| h.0),\n'
     '                endpoint.rsplit_once(\':\').map_or(endpoint, |h| h.0),\n            ),',
     'axon-cortex', _NHT, 's6_an_envelope_pinned_to_one_port_refuses_another'),
    ('M4892', "EG-1: the runner's verify accepts the audit schema list", 'crates/axon-os/src/repair_envelope.rs',
     '    verify_schemas(json, key, now_unix, &[ENVELOPE_SCHEMA])',
     '    verify_schemas(json, key, now_unix, AUDIT_ENVELOPE_SCHEMAS)',
     'axon-os', '--lib', 'repair_envelope::tests::an_axon_approval_4_envelope_is_refused_by_name'),
    ('M4893', 'EG-1: validate admits an endpoint with no port', 'crates/axon-os/src/repair_envelope.rs',
     '    if e.schema == ENVELOPE_SCHEMA {',
     '    if false && e.schema == ENVELOPE_SCHEMA {',
     'axon-os', '--lib', 'repair_envelope::tests::an_endpoint_without_its_port_is_refused'),
    ('M4894', 'EG-1: load_envelopes refuses a retired /4 envelope again', 'crates/axon-intent/src/audit_runs.rs',
     '        let ve = repair_envelope::verify_for_audit(text, vk, at)',
     '        let ve = repair_envelope::verify(text, vk, at)',
     'axon-intent', '--test audit_runs', 'an_audit_still_reads_a_retired_axon_approval_4_envelope'),
    ('M4895', 'EG-1: verify drops the by-name refusal of a retired /4 envelope', 'crates/axon-os/src/repair_envelope.rs',
     '        if RETIRED_ENVELOPE_SCHEMAS.contains(&schema) {',
     '        if false && RETIRED_ENVELOPE_SCHEMAS.contains(&schema) {',
     'axon-os', '--lib', 'repair_envelope::tests::an_axon_approval_4_envelope_is_refused_by_name'),
    ('M4896', 'EG-1: the program-approval path no longer refuses a retired /4 envelope by name', 'crates/axon-os/src/approval.rs',
     '            || crate::repair_envelope::RETIRED_ENVELOPE_SCHEMAS.contains(&schema.as_str())',
     '            || false && crate::repair_envelope::RETIRED_ENVELOPE_SCHEMAS.contains(&schema.as_str())',
     'axon-os', '--lib', 'approval::tests::a_repair_envelope_is_not_a_program_approval'),
    ('M4897', "EG-1: check_endpoint_authority's refusal admits", 'crates/axon-os/src/repair_envelope.rs',
     '        Err(format!(\n            "generator.endpoint_host {s:?} is not host:port ({why}): an {ENVELOPE_SCHEMA} envelope \\\n'
     '             pins the endpoint\'s port (O-NHR-08)"\n        ))\n    };',
     '        Ok::<(), String>(()).or(Err(format!(\n            "generator.endpoint_host {s:?} is not host:port ({why}): an {ENVELOPE_SCHEMA} envelope \\\n'
     '             pins the endpoint\'s port (O-NHR-08)"\n        )))\n    };',
     'axon-os', '--lib', 'repair_envelope::tests::an_endpoint_without_its_port_is_refused'),
    # EG-2 rows in EG-1's spare ids (lane N's block is M4890-M4909; EG-2's ten ids are all used).
    ('M4898', 'EG-2: tree_cap calls every tree complete', 'crates/axon-os/src/run_ledger.rs',
     '    if stop.is_none() && over == 0 {',
     '    if true || stop.is_none() && over == 0 {',
     'axon-os', '--lib', 'run_ledger::tests::c1_tree_over_cap'),
    ('M4899', 'EG-2: an over-cap length with a leading zero is admitted', 'crates/axon-os/src/run_ledger.rs',
     "        && (s == \"0\" || !s.starts_with('0'))",
     '        && (s == "0" || !s.is_empty())',
     'axon-os', '--lib', 'run_ledger::tests::a_tree_value_is_a_sha256_or_an_over_cap_marker'),
]

# enable-gaps EG-2 (ids M4900-M4908, block M4900-M4909): the pinned-tree walk is bounded
# (O-NHR-07): streamed hash under a byte cap, a manifest cap, a terminal-only time cap checked
# per 64 KiB chunk; a capped terminal tree is recorded (C1 tree-over-cap), a capped baseline refused.
MUTATIONS += [
    ('M4900', 'EG-2: hash_plain ignores the byte budget', 'crates/axon-cortex/src/cas.rs',
     '    if len > budget.bytes_left {',
     '    if false && len > budget.bytes_left {',
     'axon-cortex', _NHT, 'r6_a_huge_file_a_check_writes_is_valued_by_its_size'),
    ('M4901', 'EG-2: the manifest cap is not checked', 'crates/axon-cortex/src/auth.rs',
     '        if grown + MANIFEST.len() > TREE_MANIFEST_BYTES {',
     '        if false && grown + MANIFEST.len() > TREE_MANIFEST_BYTES {',
     'axon-cortex', _NHT, 'r6_a_check_that_writes_many_files_still_gets_a_terminal_record'),
    ('M4902', "EG-2: the walk's time budget is not checked", 'crates/axon-cortex/src/auth.rs',
     '        if budget.expired() {\n            out.insert(rl::TREE_STOP_KEY.into(), TIME.into());',
     '        if false && budget.expired() {\n            out.insert(rl::TREE_STOP_KEY.into(), TIME.into());',
     'axon-cortex', '--features unattended --lib', 'auth::tests::a_spent_walk_budget_caps_the_terminal_tree'),
    ('M4903', 'EG-2: a capped terminal tree drops its "" marker', 'crates/axon-cortex/src/auth.rs',
     '            out.insert(rl::TREE_STOP_KEY.into(), MANIFEST.into());',
     '            let _ = MANIFEST;',
     'axon-cortex', _NHT, 'r6_a_check_that_writes_many_files_still_gets_a_terminal_record'),
    ('M4904', 'EG-2: a started tree over a cap is recorded and the run proceeds', 'crates/axon-cortex/src/auth.rs',
     '        if grown + MANIFEST.len() > TREE_MANIFEST_BYTES {\n            if walk == Walk::Started {',
     '        if grown + MANIFEST.len() > TREE_MANIFEST_BYTES {\n            if false && walk == Walk::Started {',
     'axon-cortex', _NHT, 'r6_a_write_root_over_the_tree_cap_refuses_before_any_check'),
    ('M4905', 'EG-2: C1 does not report tree-over-cap', 'crates/axon-os/src/run_ledger.rs',
     '            if let Some(why) = tree_cap(tree) {',
     '            if let Some(why) = tree_cap(tree).filter(|_| false) {',
     'axon-os', '--lib', 'run_ledger::tests::c1_tree_over_cap'),
    ('M4906', 'EG-2: check_tree admits any value', 'crates/axon-os/src/run_ledger.rs',
     '        if !ok {\n            return Err(format!(\n                "{name}: entry {p:?}',
     '        if false && !ok {\n            return Err(format!(\n                "{name}: entry {p:?}',
     'axon-os', '--lib', 'run_ledger::tests::a_tree_value_is_a_sha256_or_an_over_cap_marker'),
    ('M4907', 'EG-2 (EGA-4): the join no longer skips keys a truncated after lacks', 'crates/axon-os/src/run_ledger.rs',
     '                if stopped && a.is_none() {',
     '                if false && stopped && a.is_none() {',
     'axon-cortex', _NHT, 'r6_a_check_that_writes_many_files_still_gets_a_terminal_record'),
    ('M4908', 'EG-2 (EGA-6): hash_plain does not check the deadline inside the chunk loop', 'crates/axon-cortex/src/cas.rs',
     '        if budget.expired() {\n            return Ok(Hashed::Expired(len));',
     '        if false && budget.expired() {\n            return Ok(Hashed::Expired(len));',
     'axon-cortex', '--features unattended --lib', 'cas::hash_plain_tests::an_expired_budget_stops_hash_plain_inside_a_file'),
    ('M4909', "NE-2/EG-2: read_plain's O_PATH open becomes O_RDONLY (a FIFO open blocks)", 'crates/axon-cortex/src/cas.rs',
     '        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW)\n        .open(path)?;\n    let t = at.metadata()?.file_type();',
     '        .custom_flags(libc::O_RDONLY | libc::O_NOFOLLOW)\n        .open(path)?;\n    let t = at.metadata()?.file_type();',
     'axon-cortex', '--features unattended --lib', 'cas::tests::a_special_file_read_is_valued_by_its_kind'),
]

# ── 2. scripts/v022_attack_markers.py ────────────────────────────────────────
# enable-gaps lane N (M4890-M4909). The amended rows' markers are unchanged.
ATTACK_MARKERS.update({
    'M4890': r'ATTACK: O-NHR-08 the endpoint authority of https://API\.anthropic\.com dropped or changed its port',
    'M4891': r'ATTACK: O-NHR-08 an envelope pinned to 127\.0\.0\.1:9 ran at https://127\.0\.0\.1:8443',
    'M4892': r'ATTACK: O-NHR-08 an axon-approval/4 envelope verified for a run',
    'M4893': r'ATTACK: O-NHR-08 the endpoint "api\.anthropic\.com" without a valid port verified',
    'M4894': r'ATTACK: EG-1 the audit no longer reads a retired axon-approval/4 envelope',
    'M4895': r'ATTACK: O-NHR-08 an axon-approval/4 envelope was not refused by name',
    'M4896': r'ATTACK: a repair envelope was read as a program approval token',
    'M4897': r'ATTACK: O-NHR-08 the endpoint "api\.anthropic\.com" without a valid port verified',
    'M4898': r'ATTACK: O-NHR-07 a capped pinned tree went unreported',
    'M4899': r'ATTACK: O-NHR-07 a malformed pinned-tree entry was admitted',
    'M4900': r"ATTACK: O-NHR-07 a file over the walk's hash bytes was read until the walk stopped",
    'M4901': r'ATTACK: O-NHR-07 (a check that wrote 20000 files left the run: no terminal record|a capped terminal tree carries no stop marker)',
    'M4902': r'ATTACK: O-NHR-07 a terminal walk past its deadline went on',
    'M4903': r'ATTACK: O-NHR-07 a capped terminal tree carries no stop marker',
    'M4904': r'ATTACK: O-NHR-07 a run over the tree cap went on',
    'M4905': r'ATTACK: O-NHR-07 a capped pinned tree went unreported',
    'M4906': r'ATTACK: O-NHR-07 a malformed pinned-tree entry was admitted',
    'M4907': r'ATTACK: EGA-4 a path cut from a stopped tree read as deleted',
    'M4908': r'ATTACK: O-NHR-07 an expired walk budget hashed the rest of the file',
    'M4909': r'ATTACK: a special file in a pinned tree failed the walk',
})

# ── 3. scripts/v022_g01_mutations.py: amended rows (replace each row in place) ──
# Their markers are unchanged. g01_anchors.py flagged each find as gone (0) or ambiguous (2) after lane N:
# M4214: the call now names `&endpoint` (EG-1). M4291: the schema is /5, the mutation /6 (EGA-8).
# M4073: the by-name branch of the retired/approval-token refusal moved into verify_schemas.
# M4074/M4818: the job path's by-name refusal now covers /5 and the retired /4.
# M4087: the minter's schema check is `validate_as`'s accept list. M4339/M4340: check_tree's grammar
# (EG-2: `""` is the stop key, values sha256 or over-cap). M4390: audit-runs reads `verify_for_audit`.
# M4550: hash_plain repeats `Err(e) => return Err(e),`; the find widens. M4812: the pinned tree now opens
# through hash_plain, so the row (its marker names the tree) moves there; read_plain's open is M4909.
AMENDED = [
    ('M4214', 'NH-R1 §5.2: the resolved generator is not compared with the envelope', _NHA,
     '    e.check_generator("ai", model, &provider, &endpoint)?;\n',
     '    let _ = (model, &provider, &endpoint);\n',
     'axon-cortex', _NHT, 's6_an_envelope_for_anything_else_is_refused'),
    ('M4291', 'NH-R1 A1 S7: the envelope schema tag changes', 'crates/axon-os/src/repair_envelope.rs',
     'pub const ENVELOPE_SCHEMA: &str = "axon-approval/5";',
     'pub const ENVELOPE_SCHEMA: &str = "axon-approval/6";',
     'axon-os', '--lib', 'repair_envelope::tests::the_envelope_constants_are_the_documented_ones'),
    ('M4073', 'NH-R1 A1 S7: a program token is read as an envelope', 'crates/axon-os/src/repair_envelope.rs',
     '        if schema.starts_with("axon-approval/") {',
     '        if false && schema.starts_with("axon-approval/") {',
     'axon-os', '--lib', 'repair_envelope::tests::s7_a_program_token_is_not_an_envelope_and_the_reverse'),
    ('M4074', 'NH-R1 A1 S7: a repair envelope is read as a program approval', 'crates/axon-os/src/approval.rs',
     '        if schema == crate::repair_envelope::ENVELOPE_SCHEMA\n',
     '        if false && schema == crate::repair_envelope::ENVELOPE_SCHEMA\n',
     'axon-os', '--lib', 'approval::tests::a_repair_envelope_is_not_a_program_approval'),
    ('M4818', 'NE-5 (S12 axon-os half): a repair envelope is read as a job approval', 'crates/axon-os/src/approval.rs',
     '        if schema == crate::repair_envelope::ENVELOPE_SCHEMA\n',
     '        if schema.is_empty()\n',
     'axon-os', '--test acceptance', 's12_a_repair_envelope_is_not_a_job_approval'),
    ('M4087', 'NH-R1 A1 S2: the minter signs another schema', 'crates/axon-os/src/repair_envelope.rs',
     '    if !accept.contains(&e.schema.as_str()) {',
     '    if false && !accept.contains(&e.schema.as_str()) {',
     'axon-os', '--lib', 'repair_envelope::tests::s2_a_malformed_sig_or_schema_is_refused'),
    ('M4339', 'R6 L1 (§3): a pinned-tree entry with an empty path validates', 'crates/axon-os/src/run_ledger.rs',
     '        let ok = if p == TREE_STOP_KEY {',
     '        let ok = if false && p == TREE_STOP_KEY {',
     'axon-os', '--lib', 'run_ledger::tests::each_malformed_record_is_refused'),
    ('M4340', 'R6 L1 (§3): a pinned-tree entry with a malformed digest validates', 'crates/axon-os/src/run_ledger.rs',
     '            is_lower_hex(d, 64) || d.strip_prefix(OVER_CAP).is_some_and(is_decimal_len)',
     '            !d.is_empty() || d.strip_prefix(OVER_CAP).is_some_and(is_decimal_len)',
     'axon-os', '--lib', 'run_ledger::tests::each_malformed_record_is_refused'),
    ('M4390', 'R6 L3 (§5): an unverifiable file in --envelopes is skipped', 'crates/axon-intent/src/audit_runs.rs',
     '        let ve = repair_envelope::verify_for_audit(text, vk, at)\n            .map_err(|e| format!("{}: not an envelope this operator signed: {e}", p.display()))?;',
     '        let Ok(ve) = repair_envelope::verify_for_audit(text, vk, at) else {\n            continue;\n        };',
     'axon-intent', '--test audit_runs', 'usage_and_unreadable_inputs_exit_2'),
    ('M4550', 'NH-R1 security re-review 2 N8: an existing target makes the placement rename over it',
     'crates/axon-cortex/src/cas.rs',
     '            Err(e) => return Err(e),\n        }\n        std::fs::remove_file(from)?;',
     '            Err(_) => std::fs::rename(from, to)?,\n        }\n        std::fs::remove_file(from)?;',
     'axon-cortex', '--lib', 'cas::tests::a_file_created_at_the_q4_placement_is_not_clobbered'),
    ('M4812', "NE-2: the pinned tree's O_PATH open (hash_plain) becomes O_RDONLY (a FIFO open blocks)", 'crates/axon-cortex/src/cas.rs',
     '        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW)\n        .open(path)?;\n    let m = at.metadata()?;',
     '        .custom_flags(libc::O_RDONLY | libc::O_NOFOLLOW)\n        .open(path)?;\n    let m = at.metadata()?;',
     'axon-cortex', '--features unattended --lib', 'auth::tests::the_pinned_tree_never_blocks_on_a_special_file'),
]

# ── 4. scripts/v022_refusal_coverage.py: exemptions ─────────────────────────
# enable-gaps lane N (EG-1, EG-2): the new refusal sites no row of M4890-M4909 changes.
EXEMPT += [
    ('crates/axon-cortex/src/cas.rs', '    pub fn expired(&self) -> bool {',
     "NAMED ROW (checkable): WalkBudget::expired has two callers and each is rowed: tree()'s check between "
     "paths (M4902, killed by auth::tests::a_spent_walk_budget_caps_the_terminal_tree) and hash_plain's check "
     "before every chunk (M4908, killed by hash_plain_tests::an_expired_budget_stops_hash_plain_inside_a_file); "
     "a predicate that answers false is exactly those rows' mutation"),
    ('crates/axon-cortex/src/cas.rs', 'pub const HASH_CHUNK: usize = 64 * 1024;',
     "REMAINDER (const_bound): hash_plain's read size. The byte cap is the fstat length before any read (M4900) "
     "and the deadline is checked before every chunk (M4908) whatever the size; only how far one read can run "
     "past the deadline scales with it, which no test measures"),
    ('crates/axon-cortex/src/cas.rs', 'Err(e) => return Err(e),\n        };\n        read += n as u64;',
     "OS ERROR (checkable): a read error on hash_plain's reopened fd is returned as is; tree() maps every Err "
     "but NotFound to `pinned tree: <key>: <e>` (auth.rs `Err(e) => return Err(err(e))`), so a started walk "
     "refuses the run and a terminal walk's error is the existing terminal-record failure path"),
    ('crates/axon-os/src/repair_envelope.rs', 'pub const RETIRED_ENVELOPE_SCHEMAS: &[&str] = &["axon-approval/4"];',
     "NAMED ROW (checkable): every reader is rowed: verify_schemas' by-name refusal (M4895), the "
     "program-approval path (M4896), and audit-runs' verify_for_audit (M4894); an empty list makes "
     "repair_envelope::tests::an_axon_approval_4_envelope_is_refused_by_name fail (the /4 refusal loses its port text)"),
    ('crates/axon-os/src/run_ledger.rs', 'pub const OVER_CAP: &str = "over-cap:";',
     "NAMED ROW (checkable): check_tree's grammar row M4906 and M4899; the cortex walk writes rl::OVER_CAP and "
     "the tests spell `over-cap:<len>` literally (run_ledger::tests::a_tree_value_is_a_sha256_or_an_over_cap_marker, "
     "unattended r6_a_huge_file_a_check_writes_is_valued_by_its_size), so another spelling fails both"),
    ('crates/axon-os/src/run_ledger.rs', 'pub const TREE_STOP_KEY: &str = "";',
     "NAMED ROW (checkable): check_tree's grammar row M4906, tree_cap M4898 and the join skip M4907 read it; "
     "the tests spell the stop key as `\"\"` literally (a_tree_value_is_a_sha256_or_an_over_cap_marker, "
     "r6_a_check_that_writes_many_files_still_gets_a_terminal_record), so another key fails both"),
]
