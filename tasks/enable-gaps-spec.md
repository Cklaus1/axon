# enable-gaps - the gaps before the unattended enabling decision (EG-1..EG-4)

**Status: DRAFT (after Step 1 review). Build authorized by the human 2026-11-08 ("fix both; do your recommendations"). Enabling
--unattended is NOT authorized by this spec (§10 Q3 of unattended-auth-spec.md).**

### Step 1 review (2026-11-08: AuditEG EGA-1..11, DevloopEG EGD-1..11)

Every finding was re-checked against d07c7429 before it was applied (cited lines re-read; checks in §7 rows 12-16). 22 accepted,
0 rejected. Integrator dispositions (Main, 2026-11-08) are canon where noted.

| id | sev | disposition | where |
|---|---|---|---|
| EGA-1 | major | accepted: the `/4`, preview-schema and doc literals added to the fixture list | EG-1 Fix 4 |
| EGA-2 | major | accepted: the S6 runs on other ports pin their own port; `:916` label; loopback test in Acceptance | EG-1 Fix 4, Acceptance |
| EGA-3 | major | accepted, merged with EGD-1: lane N delivers `tasks/eg-n.snippet.py` | EG-1 g01, §3 |
| EGA-4 | major | accepted: the join skips keys a truncated tree lacks; row M4907 | EG-2 Decision, tests, g01 |
| EGA-5 | major | accepted (integrator): the time cap applies only to terminal walks | EG-2 Decision, tests |
| EGA-6 | major | accepted (integrator): deadline checked before each 64 KiB chunk; row M4908 | EG-2 Decision, tests, g01 |
| EGA-7 | minor | accepted: any `over-cap:` value in the started walk is `Err` | EG-2 Decision |
| EGA-8 | minor | accepted: M4291's `find` moves to `/5` as well | EG-1 g01 |
| EGA-9 | minor | accepted: three envelopes, one per run | EG-1 Reproduction |
| EGA-10 | minor | accepted: `Plain` at `cas.rs:360-366`, `read_plain` at `:376` | EG-2 Problem |
| EGA-11 | minor | accepted: a load-induced `over-cap:time` run is re-collected, not waived | §5, §7 |
| EGD-1 | blocker | accepted (integrator text): `eg/int`, throwaway `<lane>-g01fb` branches, coverage judged on the spliced head | EG-4 Fix, §3 |
| EGD-2 | major | accepted (integrator): a regress is ≈ $0.27 (main is measured first) | §4 |
| EGD-3 | major | accepted (integrator): **$0 waiver** under a cfg rule, else regress; reverses NE-2 D6's precedent | §4 |
| EGD-4 | major | accepted: 20 s/row, per-lane minutes, the uncosted full freeze run named | EG-4 Cost, §4 |
| EGD-5 | major | accepted: one target dir per checkout, disk budget | §3 |
| EGD-6 | major | accepted: EG-3 budget of 3 dispatch runs, stop at 6 | EG-3 Cost |
| EGD-7 | major | accepted: §5 re-collection completed (tee'd S09U log, accepted runs, amended rows, probes, freeze, review owner) | §5 |
| EGD-8 | minor | accepted (integrator): I-A on d07c7429 avoiding `:38`/`:110-113`; I-D split into I-D1/I-D2 | §3, Appendix A |
| EGD-9 | minor | accepted: per-lane effort and the 2× rule | Appendix A |
| EGD-10 | minor | accepted (integrator): production CERE `:18307` stays, citing the human's 2026-11-05 decision; no new question | §5 |
| EGD-11 | minor | accepted: the coverage commands get a §4 row | §4 |

Source: O-NHR-08, O-NHR-07, O-CI-02, O-NHR-04 (`tasks/opportunities.md:3563-3566, 3557-3561, 3573-3586, 3345-3350`). The human, on
2026-11-08: "fix both [O-NHR-08, O-NHR-07] before enabling unattended repair; do your recommendations [O-CI-02, O-NHR-04]".

Base: `b5/specs` d07c7429 (= main). Every anchor is file:line at d07c7429, read in the worktree. Reference binaries:
`/var/tmp/r51-tmp/b5-bin/{axon,cortex,axon-intent}` (d07c7429 code; cortex built `--features unattended`). Probes and scratch:
`/var/tmp/r51-tmp/b5-draft/` (`probe/*.sh` + `.transcript`, `eg3-ci.md`, `eg4-coverage.md`, `eg4-inventory.md`, `NOTES.md`).

**Out of scope.** Enabling `cortex repair --unattended` (a human decision plus a per-host opt-in file, `unattended-auth-spec.md:483-486`).
Ruleset 26386102 and NH-1's 10-green-runs rule (`ci-hygiene-spec.md:347-372`) are unchanged. The T35 program net allowlist
(`crates/axon-ai/src/lib.rs:117-210`) still pins hosts only; that is the program grant, not the envelope. The 35-minute CI
`refusal_coverage_gate` suite (`eg3-ci.md` §6). O-C-01 and the other open O-entries.

## 1. Items and tiers

| id | opportunity | tier | what | size | lane |
|---|---|---|---|---|---|
| EG-1 | O-NHR-08 | P0 (blocks §9) | the envelope pins the endpoint's `host:port`; schema `axon-approval/5` | M | N |
| EG-2 | O-NHR-07 | P0 (blocks §9) | a bounded, streaming pinned-tree walk; an over-cap tree is recorded and audited | M | N |
| EG-3 | O-CI-02 | P1 | CI's workspace job runs its tests as root, as the gate does | S | C |
| EG-4 | O-NHR-04 | P1 (blocks the next PSV `--freeze`) | the axon-intent refusal-coverage judging batch: 244 sites | L | I-A..I-E |

## 2. Items

### EG-1 (P0) Pin the AI endpoint's port (O-NHR-08)

**Problem.** `axon_ai::endpoint_host` returns `host_of(&u)` (`crates/axon-ai/src/lib.rs:309-311`), and `host_of` is
`u.host_str()` lowercased, with no port (`:183-185`; its own test asserts `http://127.0.0.1:8931` → `127.0.0.1`, `:1546-1549`). The
only producer of the pin is `auth::resolved_generator` (`crates/axon-cortex/src/auth.rs:199-206`, the call at `:205`). It feeds both
the mint preview (`crates/axon-cortex/src/envelope_preview.rs:117`, emitted as `generator.endpoint_host` at `:139-144`) and
phase A's check (`auth.rs:476-477` → `RepairEnvelope::check_generator`, `crates/axon-os/src/repair_envelope.rs:522-542`, the host
row at `:534`). axon-intent mints whatever the preview says (`crates/axon-intent/src/authorize.rs:79, 225`). The field is
`Generator.endpoint_host: String` (`repair_envelope.rs:158-164`), validated only as non-empty (`:700-714`), under schema
`ENVELOPE_SCHEMA = "axon-approval/4"` (`:47`). So any port on the pinned host is admitted. With the CERE loopback exception
(19c277eb, `lib.rs:322-344`), "any port" includes plain `http://` to any local listener on that loopback address.

**Reproduction** (`probe/eg1.sh`, transcript `probe/eg1.transcript`; private mount namespace + tmpfs `/etc/axon` opt-in, throwaway key).
Three envelopes were minted at `AXON_AI_BASE_URL=https://127.0.0.1:9`, each with `generator.endpoint_host` = `"127.0.0.1"`, and
each was run at one base `[REVISED: EGA-9]`. At `https://127.0.0.1:8443`: cortex exit 20 `budget_exhausted`, the envelope spent
(admitted). At `http://127.0.0.1:18308`: the same, admitted. At `https://127.0.0.2:9`: exit 29 `generator endpoint host
"127.0.0.2" is not the envelope's "127.0.0.1"`. Store: 1 `.issued`, 2 `.used`. `audit-runs`: exit 0, no finding.

**Decision (canon).** Widen the value, keep the name, and bump the schema:
- `Generator.endpoint_host` holds the endpoint's **authority with its effective port**, `host:port`: `api.anthropic.com:443`,
  `127.0.0.1:18307`, `[::1]:443`. The port is `Url::port_or_known_default()`, so `https://h` and `https://h:443` pin the same thing.
- Why not a new `endpoint_port` field: `RunRecord` embeds the same `Generator` struct (`crates/axon-os/src/run_ledger.rs:40, 281`)
  under `deny_unknown_fields` (`:253-255`), and "every field is always present" (`:251-252`). A new field would make every existing
  `cortex-run-record/1` ledger, CERE's Q1 ledger included, unparseable by the new `audit-runs`, or force a record-schema bump.
  A widened string keeps the record grammar. Old records keep their bare host, which `audit-runs` never interprets.
- `ENVELOPE_SCHEMA` = `"axon-approval/5"`. `/4` is retired: phase A's `verify` refuses a `/4` envelope by name ("an axon-approval/4
  envelope pins the endpoint host without its port (O-NHR-08); mint an axon-approval/5 envelope"). That check goes before the
  generic `axon-approval/*` branch (`repair_envelope.rs:385-394`), which today would call it an "approval token". A `/4` envelope
  is refused, never re-read as `/5`.
- `ENVELOPE_SIG_DOMAIN` and the seed domain do not change, so operator keys and exported verify keys stay valid. The signed
  canonical bytes include `schema`, so a `/4` signature can never verify as `/5`.
- `PREVIEW_SCHEMA` → `cortex-envelope-preview/2` in both copies (`envelope_preview.rs:25`, `authorize.rs:38`). A cortex/axon-intent
  version mix then fails at `authorize.rs:110-113` with a schema message.
- Audit compatibility: `audit-runs --envelopes` still verifies `/4` files. `load_envelopes` refuses any other schema
  (`crates/axon-intent/src/audit_runs.rs:111-117`); it moves to `repair_envelope::verify_for_audit`, which accepts
  `RETIRED_ENVELOPE_SCHEMAS = ["axon-approval/4"]` plus `/5`, and validates `/4` under its old grammar. The job path's by-name
  refusal (`crates/axon-os/src/approval.rs:291-298`) refuses both `/4` and `/5`.

**Fix.**
1. axon-ai: replace `endpoint_host` with `pub fn endpoint_authority(p) -> Result<String, String>` (`host_of(&u)` + `:` + effective
   port; `Err` when the scheme has no known default, i.e. not http/https). `host_of` stays (T35 and `wire_is_private` use it).
2. auth.rs `resolved_generator` returns the authority. The `check_generator` row label becomes `"endpoint"`.
3. repair_envelope.rs: the schema bump, the retired-schema refusal, `verify_for_audit`. `validate` checks `/5`'s `endpoint_host`
   as `host ":" port`: host non-empty, lowercase, no whitespace; port decimal 1-65535 with no leading zero; IPv6 hosts bracketed.
4. envelope_preview.rs, authorize.rs, audit_runs.rs, approval.rs as above. Every fixture that spells a generator moves to
   `host:port`: `crates/axon-cortex/tests/unattended.rs:197-202` (`127.0.0.1:9`, matching `run_unshared`'s
   `https://127.0.0.1:9`, `:362`), `:1179, 1296`; `tests/envelope_preview.rs:141`;
   `crates/axon-intent/tests/authorize_repair.rs:99, 421`; `tests/audit_runs.rs:140`; `crates/axon-os/src/repair_envelope/tests.rs:57, 259`;
   `crates/axon-os/tests/acceptance.rs:331`; the known answer `crates/axon-fabric/tests/signing_domains.rs:203` (`:265`; it is
   regenerated, and the commit says so). `run_ledger/tests.rs:54` (a record) is left alone on purpose: the old value stays valid.
   `[REVISED: EGA-2]` `check_generator` (`auth.rs:476-477`) runs before the https check (`:481`), so a run on another port now
   fails first on the pin: `s6_plain_http_to_a_loopback_literal_reaches_the_pinned_host` pins
   `format!("127.0.0.1:{}", rec.addr.port())` before minting (its assert at `:1268` follows; the run base is `:1270`);
   `:1221` pins `format!("localhost:{}", rec.addr.port())` so it still reaches the `"https"` refusal (M4881's killing test);
   `:916` sets `api.example.com:443` and expects `"generator endpoint"`.
   `[REVISED: EGA-1]` The `/4` and preview-schema literals: `acceptance.rs:369` becomes `axon-approval/5 repair envelope is not a
   program approval` (M4818's killing test); `approval.rs:585-595` keeps `/4` only where the envelope's schema is `/4`;
   `repair_envelope/tests.rs:449, 466` (M4291's killing test), `:576`; `signing_domains.rs:138`; `tests/envelope_preview.rs:70`;
   `authorize_repair.rs:6, 83` (the fake preview's schema, so every mint test); the HELP/doc strings `authorize.rs:235`,
   `audit_runs.rs:23`, `envelope_preview.rs:2`, `bin/cortex.rs:2215`.
5. Docs: `unattended-auth-spec.md` §4 generator row (`:105`) gets `[REVISED: EG-1 ...]`; CHANGELOG at build.

**Fail-first tests.**
- `s6_an_envelope_pinned_to_one_port_refuses_another` (`tests/unattended.rs`, root). The envelope pins `127.0.0.1:9`; runs at
  `https://127.0.0.1:8443` and `http://127.0.0.1:18308` must each be refused (`refused()`: exit 29, envelope unspent, no record).
  ATTACK `O-NHR-08 an envelope pinned to 127.0.0.1:9 ran at {base}`. Fails today: both are admitted (probe above).
- `repair_envelope::tests::an_axon_approval_4_envelope_is_refused_by_name` (axon-os `--lib`): a validly signed `/4` envelope →
  `verify` is `Err` naming `axon-approval/4` and the port. Fails today: it verifies.
- `repair_envelope::tests::an_endpoint_without_its_port_is_refused` (axon-os `--lib`): cases `api.anthropic.com`, `h:0`, `h:080`,
  `h:65536`, `h:`, `:443`. Each is `Err`. Fails today: each validates.
- `host_of_url_tests::the_endpoint_authority_carries_the_effective_port` (axon-ai `--lib`): `https://API.anthropic.com` →
  `api.anthropic.com:443`, `http://127.0.0.1:18307` → `127.0.0.1:18307`, `https://h:443` → `h:443`, `https://[::1]` → `[::1]:443`.
  Fails today: the function does not exist (compile failure).
- `an_audit_still_reads_a_retired_axon_approval_4_envelope` (`crates/axon-intent/tests/audit_runs.rs`): a `/4` envelope plus its
  ledger audits clean. Passes today; it fails if the bump drops audit compatibility (a regression guard, not fail-first).

**g01** (block M4890-M4899; the find strings are post-fix code `[INFERENCE: exact text fixed at build]`).

| id | file | mutation | crate / target | killing test |
|---|---|---|---|---|
| M4890 | `axon-ai/src/lib.rs` | `endpoint_authority` returns the bare host | axon-ai `--lib` | `the_endpoint_authority_carries_the_effective_port` |
| M4891 | `axon-os/src/repair_envelope.rs` | `check_generator` compares the endpoint's host part only | axon-cortex `_NHT` | `s6_an_envelope_pinned_to_one_port_refuses_another` |
| M4892 | `axon-os/src/repair_envelope.rs` | the runner's `verify` accepts the audit schema list | axon-os `--lib` | `an_axon_approval_4_envelope_is_refused_by_name` |
| M4893 | `axon-os/src/repair_envelope.rs` | `validate` admits an endpoint with no port | axon-os `--lib` | `an_endpoint_without_its_port_is_refused` |
| M4894 | `axon-intent/src/audit_runs.rs` | `load_envelopes` refuses `/4` again | axon-intent `--test audit_runs` | `an_audit_still_reads_a_retired_axon_approval_4_envelope` |

Amended rows: M4291 (`find` `pub const ENVELOPE_SCHEMA: &str = "axon-approval/5";` → `replace` `"axon-approval/6"`
`[REVISED: EGA-8]`: today's find at `v022_g01_mutations.py:13436` spells `/4`), M4214 (its `find` spells the `check_generator`
call), M4818 (its killing test's text, EGA-1), plus any row whose `find` the lane edits; the lane lists them.
`[REVISED: EGA-3, EGD-1]` Lane N delivers `tasks/eg-n.snippet.py`: rows M4890-M4909, their markers, the amended rows, its
exemptions. It carries no `NOT_YET_SCANNED` edits for `authorize.rs`/`audit_runs.rs` (lanes I-A/I-D delete those keys). Its
refusal-coverage and G01-anchors stages are judged on the integrator's spliced head (§3), not on lane N's branch.

**Acceptance.** The tests above pass; the existing S6 endpoint tests (`unattended.rs:1165, 1216, 1291`),
`s6_plain_http_to_a_loopback_literal_reaches_the_pinned_host` (`:1264`, `[REVISED: EGA-2]`) and
`s12_a_repair_envelope_is_not_a_job_approval` (`acceptance.rs:258`) pass. M4890-M4894 and the amended rows are killed (`--only`).
Re-running `probe/eg1.sh` against the new binaries gives exit 29 for the ports 8443 and 18308.
**Gate stages.** S02/S03 (cortex, fabric `signing_domains`), S09U, S20 (axon-os, axon-ai, axon-intent per-crate tests), the
parallel "refusal-site coverage" (`scripts/gate.sh:581-582`) and "G01 mutation anchors" (`:683-684`) stages, both judged on the
spliced head.
**Cost.** $0 for the lane. See §4 for the receipt.

### EG-2 (P0) Bound the pinned-tree walk (O-NHR-07)

**Problem.** `tree()` (`crates/axon-cortex/src/auth.rs:868-900`; the O-entry's `:833-857` is stale) reads each regular file whole
(`crate::cas::read_plain`, `:887`, then `sha256_hex(&b)` `:891`; `read_plain` returns `Plain::Bytes(Vec<u8>)`,
`Plain` at `crates/axon-cortex/src/cas.rs:360-366`, `read_plain` at `:376` `[REVISED: EGA-10]`), with no limit on bytes, entries
or time. It runs twice:
- `record_started` (`auth.rs:945`), before the watchdog is armed (`bin/cortex.rs:1007`);
- `record_terminal` (`auth.rs:1006`), on the main path after `w.disarm()` (`bin/cortex.rs:1633-1634`, the call at `:1896-1913`),
  and on the deadline path inside the watchdog's `fire` (`bin/cortex.rs:1025`), where the lock-free stall timer `_exit`s at
  `STALL_AT` = ¾ `HARD_GRACE` (`watchdog.rs:85, 128-135`) whether or not the record was written.

A manifest bigger than the canonical limit (`MAX_BYTES` = 1 MiB, `crates/axon-canonical/src/lib.rs:38`) fails the append. For the
terminal record that is one stderr line (`auth.rs:1022-1027`), and the run then has no terminal record. That breaks the R6
invariant "a terminal record on every exit", and C1 then calls the run killed or crashed.

**Reproduction** (`probe/eg2.sh`, `probe/eg2b.sh`; transcripts alongside; a wrapper `axon` that acts on the witness `axon test`).

| case | observed at d07c7429 |
|---|---|
| 2 GiB sparse file already under write prefix `out` | exit 20, 3.15 s, **max RSS 2.1 GB**; both records written |
| 12 GiB sparse file written by the check (deadline 4 s + grace 2 s) | exit 20 after **8.81 s**, **max RSS 12.6 GB**; terminal written; C1 `unattributed-change` |
| 20000 empty files written by the check | exit 20; stderr `the terminal record could not be written (run record: JSON byte limit: 1590052 bytes)`; C1: `started-no-terminal ... the run was killed or crashed` (wrong cause) |
| 20000 empty files already under `out` | started append fails: exit 29, envelope `.unrecorded` (fails closed today) |

So time and memory grow with file size, past the deadline, and the entry count can delete the terminal record. That last
failure is not in O-NHR-07's text; this spec covers it.

**Decision (canon).**
- Streaming hash: a new `cas::hash_plain(path, &mut WalkBudget) -> io::Result<Hashed>`, `cfg(feature = "unattended")`, beside
  `read_plain`. It uses the same O_PATH + `/proc/self/fd` reopen and kind check. It hashes the file in 64 KiB chunks into one
  `Sha256`, so memory is constant per file. `read_plain` stays for phase A's task and envelope reads, which need the bytes.
  `[REVISED: EGA-6]` When the budget has a deadline, `hash_plain` checks it before each 64 KiB chunk; on expiry the file is
  valued `over-cap:<len>` and the walk ends with `"" → over-cap:time`. A file of up to 256 MiB therefore never runs the walk
  past its deadline by more than one chunk read.
- Three caps, constants in `auth.rs`: `TREE_HASH_BYTES` = 256 MiB of file bytes hashed per walk; `TREE_MANIFEST_BYTES` = 256 KiB
  of manifest (Σ key + value bytes), which keeps a record well under `MAX_BYTES` next to `writes` and `enforcement`;
  `TREE_WALK_TIME` = 500 ms per walk, which fits inside `STALL_AT` (1.5 s) on the deadline path with the restore and the append.
  `[REVISED: EGA-5]` The started walk enforces only the two deterministic caps (bytes, manifest). `TREE_WALK_TIME` applies only
  to terminal walks: the started walk runs after consume (`bin/cortex.rs:983-985`), so a timing-dependent refusal there would
  spend the operator's envelope at random under load or a cold page cache.
- Tree grammar, which `check_tree` (`run_ledger.rs:330-339`) widens to:
  - a key is a path → its value is 64-hex (as today) or `over-cap:<len>`. `over-cap:<len>` marks a regular file the walk did not
    hash because its length (from `fstat`) exceeds the hash bytes left, or because it changed length while being read. Such a
    file is never read at all. `[REVISED: EGA-7]` In the started walk any `over-cap:` value (a length change included) is `Err`:
    a baseline holds only 64-hex values. `fs_write` pins are absolute and may lie outside the workspace (`auth.rs:928-936`).
  - the reserved key `""` (never a path) → `over-cap:manifest` or `over-cap:time`: the walk stopped there, and later entries are
    absent.

  `RECORD_SCHEMA` stays `cortex-run-record/1`. The grammar only grows, existing records keep the meaning they had, and an older
  reader refuses a new over-cap record at `check_tree` (fails closed, never misread).
- Started walk over the byte or manifest cap, or holding any `over-cap:` value: `tree()` returns `Err` and the existing
  failed-started path refuses the run (`auth.rs:966-974`: `.unrecorded`, exit 29, no check run). The operator narrows the write
  prefixes. A baseline that is not complete is never recorded.
- Terminal walk over a cap: the record is written with the capped tree. C1 adds the finding `tree-over-cap`: `FINDINGS` goes from
  13 to 14 (`run_ledger.rs:111-125`), one finding per capped tree, naming the reason and the count of `over-cap:` paths.
  `[REVISED: EGA-4]` The `unattributed-change` join changes: today it iterates `before.keys().chain(after.keys())`
  (`run_ledger.rs:1012-1022`), so a key missing from a truncated `after` reads as a deletion. When `after` holds `""`, a key in
  `before` but absent from `after` is skipped, and the `""` key itself is never joined. A path valued `over-cap:<len>` in
  `after` is still joined (it differs from its `before` sha256 unless a write explains it). `audit-runs` therefore exits 1 for
  an over-cap run through `tree-over-cap`: the operator has to look.
- `r6-ledger-spec.md` §3's tree row (`:49`) gets `[REVISED: EG-2 ...]`; CHANGELOG at build.

**Fail-first tests** (root S-tests in `tests/unattended.rs`, using the `s10_*` wrapper idiom at `:1948-1967`, plus units).
- `r6_a_huge_file_a_check_writes_is_valued_by_its_size`: the witness `axon test` makes an 8 GiB sparse `out/big` (`set_len`).
  Asserts the terminal record exists, `pinned_tree_after["out/big"] == "over-cap:8589934592"`, run wall < 4 s, C1 = `[tree-over-cap,
  unattributed-change]`. Fails today: the value is a sha256, and the read takes ≈ 6 s and 8 GB RSS `[INFERENCE: scaled from the
  12 GiB probe]`.
- `r6_a_check_that_writes_many_files_still_gets_a_terminal_record`: `out/zz-keep` exists before the run; the witness creates
  20000 empty files `out/<n>`. Asserts a terminal record exists, its tree has `"" → "over-cap:manifest"` and ≤
  `TREE_MANIFEST_BYTES` of entries, C1 has `tree-over-cap`, no `started-no-terminal`, and no `unattributed-change` for
  `out/zz-keep` (which sorts after the cut, `[REVISED: EGA-4]`). Fails today: no terminal record (probe case 3).
- `r6_a_write_root_over_the_tree_cap_refuses_before_any_check`: 5000 files under `out` before the run, under 1 MiB of canonical
  record but over `TREE_MANIFEST_BYTES`. Asserts exit 29, `.unrecorded`, a reason naming the cap, the check never ran. Fails today:
  the run proceeds.
- `auth::tests::a_spent_walk_budget_caps_the_terminal_tree` (`--features unattended --lib`): a terminal-mode budget whose
  deadline has passed → `""` → `over-cap:time`; the same tree in started mode is not time-capped `[REVISED: EGA-5]`. Fails
  today: no budget parameter (compile failure).
- `run_ledger::tests::c1_tree_over_cap` and `run_ledger::tests::a_tree_value_is_a_sha256_or_an_over_cap_marker` (axon-os `--lib`):
  the finding; `over-cap:` with a non-decimal length, `""` with another value, and a bare `over-cap` are refused. Fail today: no
  such finding, and `check_tree` refuses every `over-cap` value.
- `cas::tests::hash_plain_equals_the_whole_file_digest` (`--lib`): regression for small files, FIFOs, and a file at exactly the
  budget.
- `cas::tests::an_expired_budget_stops_hash_plain_inside_a_file` (`--features unattended --lib`) `[REVISED: EGA-6]`: a 3-chunk
  file and a budget whose deadline passes during the first chunk → `over-cap:<len>`, not a digest. Fails today: no `hash_plain`.

**g01** (block M4900-M4909; post-fix find text `[INFERENCE]`).

| id | file | mutation | target | killing test |
|---|---|---|---|---|
| M4900 | `axon-cortex/src/cas.rs` | `hash_plain` ignores the byte budget | `_NHT` | `r6_a_huge_file_a_check_writes_is_valued_by_its_size` |
| M4901 | `axon-cortex/src/auth.rs` | the manifest cap is not checked | `_NHT` | `r6_a_check_that_writes_many_files_still_gets_a_terminal_record` |
| M4902 | `axon-cortex/src/auth.rs` | the time budget is not checked | `--features unattended --lib` | `a_spent_walk_budget_caps_the_tree` |
| M4903 | `axon-cortex/src/auth.rs` | a capped terminal tree drops its `""` marker | `_NHT` | `r6_a_check_that_writes_many_files_still_gets_a_terminal_record` |
| M4904 | `axon-cortex/src/auth.rs` | a started tree over a cap is recorded and the run proceeds | `_NHT` | `r6_a_write_root_over_the_tree_cap_refuses_before_any_check` |
| M4905 | `axon-os/src/run_ledger.rs` | C1 does not report `tree-over-cap` | axon-os `--lib` | `c1_tree_over_cap` |
| M4906 | `axon-os/src/run_ledger.rs` | `check_tree` admits any value | axon-os `--lib` | `a_tree_value_is_a_sha256_or_an_over_cap_marker` |
| M4907 | `axon-os/src/run_ledger.rs` | the join no longer skips keys a truncated `after` lacks `[REVISED: EGA-4]` | `_NHT` | `r6_a_check_that_writes_many_files_still_gets_a_terminal_record` |
| M4908 | `axon-cortex/src/cas.rs` | `hash_plain` does not check the deadline inside the chunk loop `[REVISED: EGA-6]` | `--features unattended --lib` | `an_expired_budget_stops_hash_plain_inside_a_file` |

Constant memory is asserted through the byte cap: a file above it is never read. The chunk loop's deadline check is M4908
`[REVISED: EGA-6]`.

**Acceptance.** The tests pass; s8 (`unattended.rs:478`), the s10 deadline tests (`:1948, 2024`) and every `c1_is` assertion are
unchanged. M4900-M4908 are killed. `probe/eg2b.sh` on the new binaries: the 12 GiB case ends in < 4 s with RSS < 100 MB, and the
20000-file case writes a terminal record.
**Gate stages.** S09U, S20 (axon-os), refusal coverage and G01 anchors (judged on the spliced head). **Cost.** $0 for the lane,
plus a transient 8 GB sparse file per S-test run (sparse: no disk blocks).

### EG-3 (P1) CI's workspace job runs as root (O-CI-02)

**Problem.** `workspace-test` (`.github/workflows/ci.yml:146-184`, `runs-on: ubuntu-latest` `:151`, test step `:184`) runs
`cargo test --workspace --no-default-features --no-fail-fast` as the runner user. Run 41992018913 (b4/merge 43dda51d) failed it
with **74 tests in 11 suites**; Docs, Check & Test and Codegen passed. The research (`/var/tmp/r51-tmp/b5-draft/eg3-ci.md` §2)
gives each suite's precondition with anchors: CAP_MKNOD (cortex lib), the ntfs3 loop mount (cortex cli N3), root + `/etc/axon` +
chown (freeze_manifest 26), `/var/lib` + a root-owned rustup under root's home + musl + mksquashfs (guest_build_env 38, guards 1),
`chown -R root:root` failing silently at `scripts/fc_linux_profile.sh:561` (launcher_isolation; its "exec bit" message is
misleading), `fanotify_init` (one_read 2), chown (protected_host), a non-dumpable `/proc/self/environ` (axon-os sandbox_capture),
DAC override (axon-psv protocol). **All 74 print `ok` in the local root gate logs** (`b5-gate.log`, `b4-gate-final.log`).

**check_effects is a privilege effect, not a bug.** The suite dir is materialized read-only (`crates/axon-fabric/src/submit.rs:881-882`
→ 0555/0444, `workspace.rs:571-584`). The test's write (`tests/check_effects.rs:234-250`) is `let _ =`-ignored, so
unprivileged nothing changes and Passed is the correct verdict. Reproduced: the prebuilt test binary passes as root and gives
the CI panic byte-for-byte as uid cklaus (`eg3-ci.md` §3). NH-3 B applies to every suite: nothing is skipped or `#[ignore]`d.

**Fix** (`.github/workflows/ci.yml`, the `workspace-test` job only; full YAML in `eg3-ci.md` §4):
1. `runs-on: ubuntu-24.04` (pinned: the job depends on kernel modules and apt names, and the `ubuntu-latest` → 26 migration is
   announced in the run's own annotation). Name: "Test the whole workspace as root (no LLVM)".
2. A prerequisites step, its apt block retried under the CIA-5 pattern: `squashfs-tools ntfs-3g e2fsprogs`;
   `modprobe ntfs3` (falling back to `linux-modules-extra-$(uname -r)`); an empty root-owned `/etc/axon` (the tests mount a private
   tmpfs over it, as `scripts/gate.sh:948-965` requires); `git config --system --add safe.directory "$GITHUB_WORKSPACE"`.
3. Root's own copy of the pinned toolchain (`/root/.cargo`, `/root/.rustup`, root-owned, `go-w`) plus
   `rustup target add x86_64-unknown-linux-musl`, retried (`scripts/guest_build_env.py:777-783, 1094-1122` refuse anything else).
4. The test step: `sudo env HOME=/root PATH=/root/.cargo/bin:$PATH CARGO_HOME=$HOME/.cargo CARGO_TERM_COLOR=always
   CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS= cargo test --workspace --no-default-features --no-fail-fast`. The command is
   unchanged. `sudo -E` is wrong here: it keeps `HOME=/home/runner`, and freeze and guest_build key off the euid's home.
5. `if: always()`: `sudo chown -R` the checkout and `~/.cargo` back to the runner user, so the cache can save (it never has: the job
   has never been green, `eg3-ci.md` §1).

**Fail-first.** Today's run 41992018913 is the failing witness (74 failures). There is no new test; the 74 are the tests.
**Acceptance.** Observable only on GitHub: `gh workflow run ci.yml --ref <eg3 branch>` (CID-2: a draft PR or dispatch, never a
push to main) shows the workspace job green with `0 failed` in every suite, and the other three jobs unchanged. Not observable
locally. If ntfs3 or fanotify is unavailable on the Azure kernel, that test stays red under NH-3 B. The lane then files the
O-entry with the log line and does not claim EG-3 green (`[INFERENCE: risk; eg3-ci.md §5 items 5-6]`). Three crates the gate
never runs as root (axon-gfx, axon-rt-ai, axon-workspace-recipe) get their first root run here.
**Gate stages.** None: `scripts/gate.sh` already runs as root. **Cost.** GitHub minutes only, about 126 → about 130 per full run
(public repo, standard runners `[INFERENCE: free]`), wall still about 54-60 min; $0 model. `[REVISED: EGD-6]` **Budget:** 3
`workflow_dispatch` runs ≈ 390 GH min, ≈ 3 h wall (every run builds cold: no cache has ever been saved). At 2× (6 runs, ≈ 780
min) the lane stops, files the O-entry with the failing log lines, and does not claim green. Main pushes after integration are
NH-1's counting runs and are not charged to EG-3.

### EG-4 (P1) The axon-intent refusal-coverage judging batch (O-NHR-04)

**Problem.** `NOT_YET_SCANNED` (`scripts/v022_refusal_coverage.py:342-361`) lists 17 axon-intent files with 244 uncovered sites
(authorize 49/66, intent 58/60, genloop 42/45, confidence 21/23, audit_runs 16/17, cli 16/20, generator 16/18, policy 12/12,
grant_infer 4/4, emit 3/3, synth 3/4, admit/approval/pipeline/refusal 1/1 each, main 0/1, lib 0/0). `--freeze` fails on exactly
those 17 (decided, containment-b4 NH-1 = A), so the next PSV freeze is blocked until a judging batch lands.

**Mechanics** (`eg4-coverage.md` §1). A site is covered by a g01 row whose `find` occurs exactly once in the file and whose
changed lines fall inside the site's guard block (form sites: the site line itself). The row is scored KILLED only when its
test fails with the `ATTACK_MARKERS[id]` text, and the runner refuses to start on drift between markers and rows. An `EXEMPT`
entry `(file, anchor, reason)` has to lie in the site's block, and its reason is `KIND (checkable): fact`. The kinds in use:
NOTHING TO ADMIT, OS ERROR, NAMED ROW, USAGE, DOMINATED, NOT A SITE, ENSURE-EXISTS, `_EXITMAP`, NOTROUTE for `VALUE_EXEMPT`. A
cited `M<n>` must exist; an exemption on a covered site is BAD. A `REMAINDER` reason is counted rather than covered, under an `_A95`
category or `other`; this batch allows REMAINDER only for a `cfg(feature = "ai")` arm or a const with no guard-reachable
mutation, ≤ 25 in total, each listed by `--remainder`. Tuple: `(id, title, file, find, replace, crate, target, test)`.
Precedents: c423aa8d (C-3, cov only); 45a3ca35 + ab2a650a (NE-2: a test and an exemption in the lane, the rows and markers
spliced by the integrator).

**Inventory** (appendix A summary; per-site lists for authorize.rs and audit_runs.rs in
`/var/tmp/r51-tmp/b5-draft/eg4-inventory.md`, the rest by pattern `[INFERENCE: sampled]`). About 150 sites by row (~120 rows),
~75 exempt, ~22 REMAINDER, ~11 new tests, ~10 `ATTACK:` messages added to existing asserts.

**Traps (canon).**
- authorize `:445` and `:559` guard the same missing budget flag, so neither alone is killable. Exempt both as a DOMINATED pair,
  each citing the other, and add one test showing the pair refuses.
- In-file tests must not repeat a row's `find` text (count ≠ 1 is BAD, and `g01_anchors.py` returns rc 1).
- `policy_for` has no caller outside `policy.rs`: its 12 values are `VALUE_EXEMPT` NOTROUTE, citing the grep.
- `generator.rs:212-213` compile only under `--features ai`: REMAINDER (`other`, "cfg(feature = ai) arm"), unless the lane shows
  an offline `--features ai --lib` row target builds.
- `main.rs`/`lib.rs` (0 uncovered): delete their entries only.

**Fix.** Each lane delivers, in its branch:
1. its tests and `ATTACK:` messages (crates/axon-intent only, in its own files);
2. a snippet file `tasks/eg4-<lane>.snippet.py` holding a `MUTATIONS += [...]` block with the header `# enable-gaps EG-4 lane X
   (ids ..., block ...)`, the `ATTACK_MARKERS.update({...})`, the `EXEMPT`/`VALUE_EXEMPT` entries, and the `NOT_YET_SCANNED` keys
   it deletes.

The integrator splices them: rows after the last `MUTATIONS` block (`v022_g01_mutations.py:15319`, before `if __name__` at
`:15321`), markers after `v022_attack_markers.py:4244`, exemptions before `def load_rows` (`v022_refusal_coverage.py:4689`), and it
empties `NOT_YET_SCANNED` with its comment kept. The snippet files are deleted after splicing. `[REVISED: EGD-1]` A lane's own
g01 feedback runs on a throwaway branch (§3 "Lane feedback"); the runner reads rows only from the registry and refuses any
uncommitted change (`v022_g01_mutations.py:49, 6917-6920`).

**Fail-first.** `python3 -B scripts/v022_refusal_coverage.py --freeze` → rc 1 with 17 `NOT YET SCANNED at a freeze` lines today
(reproduced). Each new test is fail-first against its own row: under the mutation it fails with its marker.
**Acceptance** (committed integrated head):
1. `--freeze` rc 0, no axon-intent `NOT YET SCANNED` line, no `SIGNING INPUT ... authorize.rs:576 ... UNCOVERED`.
2. The default run rc 0, and `--remainder` lists ≤ 25 axon-intent REMAINDER sites.
3. `scripts/g01_anchors.py` rc 0.
4. `v022_g01_mutations.py --only=<all EG-4 ids> OUT.json` shows every row KILLED by its marker.
5. `cargo test -p axon-intent` is green.
6. One `--without=<row>` per lane makes cov name the site again.

**Gate stages.** S20 (axon-intent), refusal coverage, G01 anchors; the full g01 run at the freeze head.
**Cost** `[REVISED: EGD-4]`. ≈ 128 rows (appendix A) × ≈ 20 s ≈ 43 min of per-lane `--only` feedback (A 5, B 14, C 8, D1+D2 10,
E 6 min), then one integrated `--only` pass ≈ 43 min; $0 model. A lane past 1.5× its estimated rows re-states its cost before
running. Between passes `--changed-since=<last record>` is feedback only, never evidence. Not in this batch: the merged
`--scope=all --shard=K/N` run the next freeze needs (`v022_freeze_manifest.py:162-177`; ≈ 3100-3300 rows × 20 s ≈ 18 h serial
÷ shards, root rows serialized in shard 0) `[INFERENCE]`.

## 3. Lanes and sequencing

| order | lane | base | items | files (exclusive) |
|---|---|---|---|---|
| 1 | N | d07c7429 | EG-1, EG-2 | `crates/axon-ai/src/lib.rs`; `crates/axon-cortex/src/{auth.rs,cas.rs,envelope_preview.rs}`, `bin/cortex.rs:2215` (doc line); `crates/axon-cortex/tests/{unattended.rs,envelope_preview.rs}`; `crates/axon-os/src/{repair_envelope.rs,repair_envelope/tests.rs,run_ledger.rs,run_ledger/tests.rs,approval.rs}`; `crates/axon-os/tests/acceptance.rs`; `crates/axon-fabric/tests/signing_domains.rs`; `crates/axon-intent/src/{authorize.rs:38,110-113,235,audit_runs.rs}` + `tests/{authorize_repair.rs,audit_runs.rs}` (EG-1's lines only); `tasks/eg-n.snippet.py` |
| 1 | C | d07c7429 | EG-3 | `.github/workflows/ci.yml` |
| 1 | I-B, I-C, I-E, I-D1 | d07c7429 | EG-4 | appendix A; none of lane N's files |
| 1 | I-A | d07c7429, rebased onto `eg/int` after N lands | EG-4 | `authorize.rs` + its tests; writes no row whose `find` covers `:38` or `:110-113` |
| 2 | I-D2 | `eg/int` after N | EG-4 | `audit_runs.rs`, `tests/audit_runs.rs` (EG-1 changes `load_envelopes`; `:112` is re-judged) |
| 3 | integrator | `eg/int` | splice | `scripts/v022_g01_mutations.py`, `scripts/v022_attack_markers.py`, `scripts/v022_refusal_coverage.py`, CHANGELOG, `tasks/opportunities.md` stamps, spec docs |
| 4 | integrator | spliced `eg/int` | evidence | receipt waiver or regress, g01 `--only` (all new + amended ids), coverage, one strict gate, then §5 |

`[REVISED: EGD-1, EGD-8]` Critical path: N → I-D2 → splice → evidence. Lane N is one lane because EG-1 and EG-2 share `auth.rs`
and `tests/unattended.rs`. Lanes land on the integration branch `eg/int` (from main d07c7429); main gets only the integrated,
gated head. Until the order-3 splice, refusal coverage on `eg/int` may be BAD (N changes `authorize.rs`/`audit_runs.rs` site
text while their `NOT_YET_SCANNED` counts stand); it is judged, for every lane, on the integrator's spliced head.
**Lane feedback.** A lane runs g01 in its own checkout on a throwaway branch: `git switch -c <lane>-g01fb`, splice its snippet into
the three registry scripts, commit, run `--only=<its ids>`, `git switch <lane>`. The fb branch is never merged.
**Target dirs** `[REVISED: EGD-5]`. Each checkout builds into its own `<checkout>/target`; two checkouts never share one, and
`CARGO_TARGET_DIR` never points into another checkout. Lane C builds nothing. A lane that runs root tests (N, the integrator's
strict gate) does all of its cargo work as root, or keeps a second dir `<checkout>/target-root` (root-owned files in a user
target are `[INFERENCE]` a hazard). Budget ≤ 7 live targets × ≈ 30 GB ≈ 210 GB (`/` has 1.7 T free, 77% used); each lane deletes
its target after merge; stop if `/` passes 85%.
g01 blocks: N = M4890-M4909; I-A M4910-M4969; I-B M4970-M5029; I-C M5030-M5089; I-D1 M5090-M5129; I-D2 M5130-M5149;
I-E M5150-M5209.

## 4. Gates and cost

| what | command | cost |
|---|---|---|
| lane N tests | `cargo test --locked -p axon-cortex --features unattended` (root), `-p axon-os`, `-p axon-ai --lib`, `-p axon-intent`, `-p axon-fabric --test signing_domains` | about 50 s warm for the cortex suite (NED-2's measure) plus the 8 GiB walk case; minutes `[INFERENCE]` |
| g01 lane N | `--only=M4890..M4894,M4900..M4908,M4214,M4291,M4818,+amended` on `n-g01fb` | ≈ 17 rows × 20 s ≈ 6 min `[REVISED: EGD-4]` |
| g01 EG-4 | `--only=<lane block>` per lane on `<lane>-g01fb`, then one integrated pass | ≈ 43 + 43 min (EG-4 Cost) |
| coverage at head `[REVISED: EGD-11]` | `v022_refusal_coverage.py --freeze`, `--remainder`, 5 × `--without=<row>` (not gate stages: the gate runs default mode, `gate.sh:582`) | seconds; $0 |
| cortex receipt `[REVISED: EGD-2, EGD-3]` | **$0 waiver** if `git diff d07c7429..<head> -- crates/axon-cortex/src` touches only items inside `cfg(feature = "unattended")` (lane N attaches the hunk list with each hunk's cfg): `scorecard.py waive --reason "EG-1/EG-2: unattended-only (auth, envelope_preview, cas::hash_plain); the receipt build is --features ai (scorecard.py:481-484) and compiles none of it"`. Otherwise `scorecard.py regress --cap-usd 0.50`. Then `receipt --check`; before the strict gate | waiver $0; regress ≈ $0.27 (main's digest d8c6ecce has only a waiver, so main is measured first, `scorecard.py:343-346, 594-603`: 2 × ≈ $0.13) |
| strict gate | `./scripts/gate.sh --strict 2>&1 \| tee <evidence>/strict.log`, as root, once at the spliced head (S09U included) | about 50-54 min (3015 s at d07c7429) |
| CI | `gh workflow run ci.yml --ref <branch>` (EG-3), then main pushes | GH minutes ≈ 130 per run, budget 3 runs (EG-3); $0 |
| §9 re-collection | §5 | $0 (CERE tokens are not billed, `nhr-evidence-spec.md:498`) |
| **total paid** | | **$0 with the waiver; ≈ $0.27 if a regress is needed** |

The waiver rule reverses the NE-2 D6 precedent (`nhr-evidence-spec.md:374-375`, a regress "since §9 evidence should not rest on
a waiver"). That precedent was a Main ruling, not a scorecard requirement, and main's current digest d8c6ecce already rests on a
cere-loopback waiver; the integrator decided the scorecard rule ("a change that cannot alter behaviour") governs here.

## 5. §9 evidence to re-collect after EG-1/EG-2

The batch-4 artifacts were taken with `/4` envelopes and the unbounded walk. These are stale and must be retaken on the
integrated head `[REVISED: EGD-7]`:
1. S-tests green: the S09U section of the tee'd `strict.log` (§4; gate.sh deletes its own S09U log, `gate.sh:950-965`, after
   printing it to stdout).
2. C1 clean over the S-test runs: the `c1_is` asserts in that log.
3. The Q1 dry-run series on CERE: `http://127.0.0.1:18307`, `/5` envelopes pinning `127.0.0.1:18307`; 3 accepted `ai:` runs
   plus 10 $0 runs, as NE-7.2; `audit-runs --json` gives `findings: []` with ≥ 1 `accepted` record; plus one negative run at
   another port (exit 29), proving the pin on CERE. `[REVISED: EGD-10]` Scope as decided by the human 2026-11-05 ("Q1 runs on
   CERE instead of OpenRouter", `nhr-evidence-spec.md:389`): production `:18307`, serial, ≤ 13 model requests, the trainloop
   token, as NE-7. `[REVISED: EGA-11]` A run whose audit reports a load-induced `tree-over-cap` (`over-cap:time`) is
   re-collected, not waived.
4. A read-only adversarial review of the EG-1/EG-2 diff (envelope grammar, `verify_for_audit`, the tree grammar, the caps on the
   watchdog path), every finding dispositioned. Owner: the integrator spawns it; ≈ 1 agent-hour, $0 metered `[INFERENCE]`.
5. g01 at head: M4520-M4552, M4810-M4819, M4870-M4881, M4890-M4909 (those used), M4214, M4291 and lane N's amended list, all
   killed.
6. `probe/eg1.sh` on the integrated binaries: exit 29 for :8443 and :18308. `probe/eg2.sh` + `eg2b.sh`: EG-2's Acceptance
   figures (12 GiB case < 4 s, RSS < 100 MB) and a terminal record in all four cases.
7. `v022_refusal_coverage.py --freeze` rc 0 (EG-4).

Cost: $0 metered; wall ≈ 55 min strict gate + ≈ 10 min Q1 + ≈ 5 min probes + the review `[INFERENCE]`. The enabling decision
itself stays with the human (§10 Q3), after these exist.

## 6. Needs human

Engineering choices are canon above: the `host:port` value, the `/5` bump, audit-only `/4`, the caps and their values, the
started-walk refusal, root for the whole CI job, `ubuntu-24.04`. No product question remains.

`[REVISED: Step 2, integrator 2026-11-08]` EGQ-a (root on every trigger, `pull_request` included) is reclassified as an
engineering decision, decided **A**. Reason: root gives PR code nothing it lacks today. Every GitHub-hosted job already runs as
a user with passwordless `sudo`, and the existing workflow's PR-triggered jobs already use it (`ci.yml:38` `sudo apt-get`,
`:230-235` `sudo tee` / `sudo apt-get install`). The repo is public (`gh repo view`: `PUBLIC`), and fork PRs get a read-only
token and no secrets `[INFERENCE: GitHub policy, not probed]`. B would leave PRs red on the 74, which NH-3 B forbids masking;
C adds a host to maintain.

Not asked here: the §10 Q3 enabling decision, which follows §5. Decided by the integrator, not asked: the receipt waiver rule
(§4, EGD-3) and the CERE scope of §5 item 3, which cites the human's 2026-11-05 decision (EGD-10).

## 7. Verification log (2026-11-08, worktree d07c7429; `PATH=$HOME/.cargo/bin:$PATH TMPDIR=/var/tmp/r51-tmp`)

| # | command | observed |
|---|---|---|
| 1 | `git log --oneline -3` | d07c7429, 85bbd368, 19c277eb |
| 2 | `bash probe/eg1.sh` | mint rc 0 `endpoint_host "127.0.0.1"`; :8443 rc 20 admitted; http :18308 rc 20 admitted; 127.0.0.2 rc 29; audit rc 0 |
| 3 | `bash probe/eg2.sh` | 2 GiB pre-existing: rc 20, 3.15 s, RSS 2107744 KiB; 20000 pre-existing files: rc 29 `JSON byte limit: 1590622 bytes`, `.unrecorded` |
| 4 | `bash probe/eg2b.sh` | 12 GiB check-written: rc 20, 8.81 s, RSS 12592660 KiB, C1 `unattributed-change`; 20000 check-written: rc 20, no terminal record, C1 `started-no-terminal` |
| 5 | `python3 -B scripts/v022_refusal_coverage.py --freeze` | rc 1; 17 `NOT YET SCANNED at a freeze`, 17 BAD in all |
| 6 | `gh run view 41992018913` | Docs ✓ 15 s, Check & Test ✓ 25m4s, workspace ✗ 54m6s |
| 7 | CiPrivResearch (`eg3-ci.md` "Commands run" 1-13) | 74/11 from `--log-failed`; 74/74 `ok` in the root gate logs; check_effects passes as root and fails as uid cklaus |
| 8 | CoverageResearch (`eg4-coverage.md` "Commands run" 1-13) | default rc 0, `--remainder` rc 0 (208 REMAINDER, 0 axon-intent), the per-file counts above; top g01 id M4881; M489x/M49xx unused |
| 9 | grep `endpoint_host` over `crates` | producers and consumers as listed in EG-1 Fix 4; no docs/README hit |
| 10 | `python3 -B scripts/spec_ledger.py` (before this file) | `PASS (0 problem(s), 32 specs)` |
| 11 | `python3 -B scripts/spec_ledger.py` (with this file) | `PASS (0 problem(s), 33 specs)`; `--list` shows it DRAFT |
| 12 | re-read the EGA-1/EGA-2 lines (`acceptance.rs:366-370`, `repair_envelope/tests.rs:449, 466, 576`, `signing_domains.rs:138`, `tests/envelope_preview.rs:70`, `authorize_repair.rs:6, 83`, `authorize.rs:235`, `audit_runs.rs:23`, `envelope_preview.rs:2`, `bin/cortex.rs:2215`, `approval.rs:585-595`, `unattended.rs:914-918, 1216-1235, 1262-1275`, `auth.rs:474-484`) | each holds the cited `/4`, `/1` or bare-host literal; `check_generator` precedes `endpoint_is_private` |
| 13 | re-read `run_ledger.rs:1008-1023`, `bin/cortex.rs:982-986`, `cas.rs:359-376`, g01 `:13436`, `probe/eg1.sh:6-11` | EGA-4/5/8/9/10 confirmed as stated |
| 14 | re-read g01 `:45-49, 6915-6921`, cov `:345-349, 7489-7497` | runner refuses uncommitted trees; a count drift is BAD (EGD-1) |
| 15 | `scorecard.py` `:343-346, 594-603, 481-484`; `repair_digest('.', 'HEAD')`; `tail -3 history.jsonl`; `lib.rs:31-36` | digest `d8c6ecce…`; last entry at it is the 2026-11-05 waiver; receipt build `--features ai`; `auth`/`envelope_preview` cfg-gated |
| 16 | `v022_freeze_manifest.py:162-177`; `gate.sh:950-965`; `df -h /`; `nhr-evidence-spec.md:383-392` | merged run required; S09U log `rm -f`d after `cat`; 1.7T free (77%); CERE decision present |

Each probe ran in a private mount namespace with its own tmpfs `/etc/axon`; afterwards `ls -A /etc/axon | wc -l` gave 0. The
sparse and many-file trees were deleted.

**Not verified.** Any EG-3 run as root on a GitHub runner (ntfs3 and fanotify there in particular). The per-site classes outside
authorize.rs and audit_runs.rs (sampled). The 500 ms / 256 MiB cap timing under gate load: a load-induced `over-cap:time` on a
terminal walk turns a §5 item 3 run's audit to exit 1 (EGA-11; such a run is re-collected). The EG-1/EG-2 find strings
(post-fix code). The 20 s/row g01 figure is DevloopEG's; I found no recorded source (cs3 says 15 s) `[INFERENCE]`.

## Appendix A. EG-4 lane table (from `eg4-coverage.md` §4-5)

| lane | files | uncovered | est. R / E / M | new tests | test files it owns | ids | effort `[REVISED: EGD-9]` |
|---|---|---|---|---|---|---|---|
| I-A | authorize.rs | 49 | 19 sites (15 rows) / 26 / 4 | 3 (+2 messages) | `tests/authorize_repair.rs`, authorize.rs `mod tests` | M4910-M4969 | ≈ 5 h |
| I-B | intent.rs | 58 | ~55 (~42 rows) / ~2 / ~1 | ~1 table-driven (+2 messages) | intent.rs `mod tests` | M4970-M5029 | ≈ 8 h |
| I-C | genloop.rs | 42 | ~25 / ~10 / ~7 | ~3 (a fake `axon`) | `tests/genloop.rs` | M5030-M5089 | ≈ 6 h |
| I-D1 | confidence.rs, cli.rs | 37 | ~17+~5 / 1+~11 / ~3 | ~1 (+messages) | `tests/cli_generate.rs`, their `mod tests` | M5090-M5129 | ≈ 4.5 h |
| I-D2 | audit_runs.rs | 16 | 7 / ~8 / ~1 | 2 | `tests/audit_runs.rs`, its `mod tests` | M5130-M5149 | ≈ 2.5 h |
| I-E | generator, policy, grant_infer, emit, synth, admit, approval, pipeline, refusal, main, lib | 42 | ~17 / ~19 / ~6 | ~1 | each file's `mod tests`; new `tests/refusal_e.rs` | M5150-M5209 | ≈ 4.5 h |

Effort ≈ rows × 10 min authoring + tests × 30 min + exemptions × 3 min `[INFERENCE]`. At twice its estimate a lane stops and
the integrator re-slices it (I-B by function range). The I-D1/I-D2 split of I-D's 53 sites and ≈ 7 h is by file
(`[REVISED: EGD-8]`); the E/M split inside I-D2 is `[INFERENCE]` from `eg4-inventory.md`.

Nobody edits `tests/acceptance.rs` or `tests/signals.rs` (they span modules). Lane I-E does not touch `tests/genloop.rs`.
authorize.rs in summary: 26 E are mostly NOTHING TO ADMIT key/arg arms and USAGE (`:385, :406`); the rows go on the gate
(`:159`), the write-once exports (`:306, :318`), init (`:341`) and required flags (`:440`). audit_runs.rs: 7 rows, 2 of them with
new tests (a symlink in `--envelopes`; two envelopes with one id). `:112` is DOMINATED by `repair_envelope::verify`'s schema
refusal; after EG-1 that is re-judged against `verify_for_audit`.
