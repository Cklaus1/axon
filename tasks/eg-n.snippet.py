# enable-gaps lane N (EG-1 ids M4890-M4894, block M4890-M4899; EG-2 ids M4900-M4908, block M4900-M4909).
# Integrator: splice section 1 into scripts/v022_g01_mutations.py (after the last MUTATIONS block,
# before `if __name__`), section 2 into scripts/v022_attack_markers.py (after the last ATTACK_MARKERS
# block), section 3 replaces the five amended rows IN PLACE in scripts/v022_g01_mutations.py (their
# markers are unchanged), section 4 goes before `def load_rows` in scripts/v022_refusal_coverage.py.
# No NOT_YET_SCANNED edits (lanes I-A/I-D own authorize.rs/audit_runs.rs keys).

# ── 1. scripts/v022_g01_mutations.py: new rows ───────────────────────────────
# enable-gaps EG-1 (ids M4890-M4894, block M4890-M4899): the envelope pins the AI endpoint's
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
     'axon-cortex', '--features unattended --lib', 'hash_plain_tests::an_expired_budget_stops_hash_plain_inside_a_file'),
]

# ── 2. scripts/v022_attack_markers.py ────────────────────────────────────────
# enable-gaps lane N (EG-1 M4890-M4894, EG-2 M4900-M4908). The amended rows' markers are unchanged.
ATTACK_MARKERS.update({
    'M4890': r'ATTACK: O-NHR-08 the endpoint authority of https://API\.anthropic\.com dropped or changed its port',
    'M4891': r'ATTACK: O-NHR-08 an envelope pinned to 127\.0\.0\.1:9 ran at https://127\.0\.0\.1:8443',
    'M4892': r'ATTACK: O-NHR-08 an axon-approval/4 envelope verified for a run',
    'M4893': r'ATTACK: O-NHR-08 the endpoint "api\.anthropic\.com" without a valid port verified',
    'M4894': r'ATTACK: EG-1 the audit no longer reads a retired axon-approval/4 envelope',
    'M4900': r"ATTACK: O-NHR-07 a file over the walk's hash bytes was read until the walk stopped",
    'M4901': r'ATTACK: O-NHR-07 (a check that wrote 20000 files left the run: no terminal record|a capped terminal tree carries no stop marker)',
    'M4902': r'ATTACK: O-NHR-07 a terminal walk past its deadline went on',
    'M4903': r'ATTACK: O-NHR-07 a capped terminal tree carries no stop marker',
    'M4904': r'ATTACK: O-NHR-07 a run over the tree cap went on',
    'M4905': r'ATTACK: O-NHR-07 a capped pinned tree went unreported',
    'M4906': r'ATTACK: O-NHR-07 a malformed pinned-tree entry was admitted',
    'M4907': r'ATTACK: EGA-4 a path cut from a stopped tree read as deleted',
    'M4908': r'ATTACK: O-NHR-07 an expired walk budget hashed the rest of the file',
})

# ── 3. scripts/v022_g01_mutations.py: amended rows (replace each row in place) ──
# M4214: the call now names `&endpoint` (EG-1). M4291: the schema is /5, the mutation /6 (EGA-8).
# M4073: the by-name branch of the retired/approval-token refusal moved into verify_schemas.
# M4074/M4818: the job path's by-name refusal now covers /5 and the retired /4.
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
]

# ── 4. scripts/v022_refusal_coverage.py: exemptions ─────────────────────────
