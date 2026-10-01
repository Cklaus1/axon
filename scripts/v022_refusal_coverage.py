#!/usr/bin/env python3
"""Refusal-site coverage drift check for the protected helper files
(C9 round 3, harness workstream; amendment 48).

The mutation registry (scripts/v022_g01_mutations.py) is a LIST of guards, and
a guard nobody listed is untested evidence: the round-3 EQUIVALENCE review
found ten guards in the decision-A/D files with no row, and the whole
axon-fabric suite green with each of them removed. This check derives the
guards from the CODE instead of trusting the list.

A refusal site is a non-test line in a PROTECTED file matching SITE
(`return Err(`, `Err(format!`, `refuse(`, `Err(bad(`, or a read of
`TEST_TRUST_BUILD`; a `use` declaration is not a read). Its guard block runs from the nearest opening
condition above it (`if` / `else if` / `match` / a match arm), at most
MAX_UP lines up, to the site itself. The site is COVERED when some row of
the registry (ACTIVE or retired: a retired row is still reviewed, with its
four-cell record) mutates text that overlaps that block. Otherwise it must be
on EXEMPT, with a reason, by an anchor unique in its file that lies in the
block. The check fails on:

  * a site neither covered nor exempt (a new or unlisted guard);
  * an exemption whose anchor is missing, ambiguous, matches no site, or
    names a site a row already covers (the list must not outlive its reason);
  * a row for a protected file whose `old` text is not present exactly once
    (it covers nothing).

    python3 scripts/v022_refusal_coverage.py [--without=M1,M2]

--without drops rows before checking: the check must then name their sites
(a gate that cannot speak proves nothing).
"""
import importlib.util
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PROTECTED = [
    "crates/axon-fabric/src/privileged_launcher.rs",
    "crates/axon-fabric/src/sealed_exec.rs",
    "crates/axon-fabric/src/bin/axon-protected-launcher.rs",
    # C9 round 4 fix wave, rows2 (amendment 58): the custodian, its binary,
    # readiness, the protected host config and the observer's nonce store.
    "crates/axon-fabric/src/custodian.rs",
    "crates/axon-fabric/src/bin/axon-custodian.rs",
    "crates/axon-fabric/src/readiness.rs",
    "crates/axon-fabric/src/protected_host.rs",
    "crates/axon-fabric/src/observer.rs",
    "crates/axon-psv/src/lib.rs",
    "crates/axon-fabric/src/psv.rs",
    "crates/axon-psv/src/runner.rs",
    "crates/axon-loop-contracts/src/protected_evidence.rs",
    "crates/axon-loop/src/admission.rs",
    "crates/axon-loop/src/intake.rs",
]
# Protected decision files NOT yet scanned, with the refusal sites that had
# neither a row nor an exemption when they were last measured (amendment 58).
# Printed on every run so the gap is visible; moving a file into PROTECTED is
# how it closes.
NOT_YET_SCANNED = {}
SITE = re.compile(r"return Err\(|\bErr\(format!|\brefuse\(|\bErr\(bad\(|TEST_TRUST_BUILD")
OPENER = re.compile(r"^\s*(\}\s*else\s+if\b|if\b|match\b|let\s+\w+\s*=\s*if\b)|=>")
MAX_UP = 10

# (file, anchor, reason). The anchor is a substring that occurs ONCE in the
# file and lies in the site's guard block. Reasons say why no row is needed:
# an OS error propagated (the operation fails closed and no input chooses
# success), a check dominated on every path by a named row, or an
# operator-authored field (the config file is operator-owned, M585/M586).
PL = "crates/axon-fabric/src/privileged_launcher.rs"
SE = "crates/axon-fabric/src/sealed_exec.rs"
BIN = "crates/axon-fabric/src/bin/axon-protected-launcher.rs"
EXEMPT = [
    (PL, "    if unsafe { libc::fstat(fd, &mut st) } != 0 {",
     "OS error from fstat on an open descriptor: fails closed, no input chooses success"),
    (PL, "    if fd < 0 {\n        return Err(std::io::Error::last_os_error());",
     "OS error from openat: fails closed"),
    (PL, '        return Err(format!("{} is not a directory", p.display()));',
     "unreachable: every descriptor operator_dir sees was opened O_DIRECTORY (walk_open, "
     "load_config's parent, new_staging's leaf), so it is a directory"),
    (PL, '            return Err(format!("{} is not a plain path", path.display()));',
     "operator-authored or compiled in: walk_open's callers pass the helper config's out_root "
     "and staging_root (fields of the operator-owned config, M585-M589) or the parent of the "
     "compiled-in CONFIG_PATH; a config path of the caller's choosing (--test-config) exists "
     "only in a test-trust build (M601)"),
    (PL, '        return Err(bad("not a regular file".into()));',
     "the config's directory chain and file are operator-owned (M585-M589): only the operator "
     "can put a non-regular file at the config path"),
    (PL, '        return Err(bad(format!("schema is not {CONFIG_SCHEMA}")));',
     "operator-authored field of an operator-owned file (M585/M586): a version tag"),
    (PL, "        if !is_hex64(&p.sha256) {",
     "operator-authored: a pin in the operator-owned helper config (M585/M586)"),
    (PL, "            || p.components()",
     "operator-authored field of an operator-owned file (M585/M586)"),
    (PL, '        return Err(bad("firecracker must be a file named \'firecracker\'".into()));',
     "operator-authored field (the jailer requires the name); operator-owned file"),
    (PL, "    if c.observer.max_age_s == 0 || !is_hex64(&c.observer.host_signer_public_key) {",
     "operator-authored fields of an operator-owned file (M585/M586), amendment 50: a zero "
     "max age refuses every observation, and a key that is not 64 hex matches no root key, so "
     "each only fails closed"),
    (PL, "    if c.max_timeout_s == 0 || c.max_input_bytes == 0 {",
     "operator-authored limits; zero only makes every request fail (fails closed)"),
    (PL, '            _ => {\n                return Err(format!(\n                    "{} is not a regular file or directory",',
     "fail-closed on an entry the snapshot cannot represent; skipping it (the only "
     "alternative) cannot add bytes to the guest image; symlinks are refused by O_NOFOLLOW"),
    (PL, "        if unsafe { libc::fstatat(dir, cn.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) } != 0 {",
     "OS error during the hand-over: reported as EXIT_UNKNOWN, fails closed"),
    (PL, '        if !ok {\n            return Err(format!(\n                "handing {name:?} to the Fabric uid: {}",',
     "OS error during the hand-over: reported as EXIT_UNKNOWN, fails closed"),
    (PL, '        return Err("the manifest changed while it was read".into());',
     "the manifest is operator-owned (open_verified owner check) and leased under "
     "Lease::Required (M591): only the operator could change it between the two reads"),
    (SE, "    if unsafe { libc::fstat(fd, &mut st) } != 0 {",
     "OS error from fstat: fails closed"),
    (SE, "            1 => Err(format!(",
     "the verdict function's arms; each caller's refusal on its verdict has a row mutating "
     "that call: the post-hash check (M592) and the child's re-check before execveat (M527)"),
    (SE, "            _ => Err(format!(",
     "as above"),
    (SE, "            if unsafe { libc::fcntl(*fd, libc::F_SETFD, 0) } != 0 {",
     "OS error clearing FD_CLOEXEC in the child: the exec does not happen (fails closed)"),
    (BIN, '        refuse(format!("request: {e}"));',
     "OS error reading stdin: nothing is launched (fails closed)"),
]

# C9 round 4 fix wave, rows2 (amendment 58). Categories as above, plus: a site
# whose condition a named row already mutates at another line (the removal is
# the same), an arm that has no value to admit with, and a check that only
# re-reports what the next statement refuses on the same input.
CU = "crates/axon-fabric/src/custodian.rs"
RD = "crates/axon-fabric/src/readiness.rs"
PH = "crates/axon-fabric/src/protected_host.rs"
OB = "crates/axon-fabric/src/observer.rs"
PS = "crates/axon-psv/src/lib.rs"
EXEMPT += [
    (CU, "            if !plain_absolute(p) {",
     "operator-authored fields of the operator-owned /etc/axon/custodian.json (read through "
     "read_operator_file, the helper config's walk, M585-M589); and on the protected route the "
     "socket must EQUAL the path systemd bound (M703), which is absolute"),
    (CU, '        return Err(format!("SO_PEERCRED: {}", std::io::Error::last_os_error()));',
     "OS error from getsockopt: the connection is answered with a refusal (server) or refused "
     "(client); no peer is assumed"),
    (CU, "        if r.schema != REPLY_SCHEMA || Mode::parse(&r.mode).is_none() {",
     "authored by the operator-installed custodian, authenticated by its uid before a byte "
     "is sent (M626): `schema` is its version tag, and a mode that does not parse is read as "
     "Dev, which launches nothing protected (custodian_mode_launches accepts Protected, or Test "
     "in a test authority)"),
    (CU, '            other => Err(format!("unknown op {other:?}")),',
     "an unknown op changes no state and returns no nonce: admitted, it could only answer ok with "
     "nothing, which neither client reads as a nonce (issue requires one) or sends (the helper "
     "sends `spend` only)"),
    (CU, '        return Err("fd 3 is not the activated socket".into());',
     "fails closed: UnixListener::local_addr on a descriptor that is not a socket fails "
     "(ENOTSOCK, 'activated socket:'), and a socket at another path is refused by M703"),
    (RD, "        Err(e) => return Err(format!(\"{}: {e}\", path.display())),\n        Ok(_) => {}",
     "OS error from lstat other than NotFound: the path is then walked by check_owned_from_pub and "
     "read by read_regular, which fail on it; never read as the default"),
    (RD, "                        if writable_by_me(&q) {",
     "rowed where the same removal is made: M490 (the enclosing require_unwritable condition) and "
     "M491 (writable_by_me's predicate), both killed on the production decision"),
    (RD, "    if grafts.is_empty() {",
     "unreachable, and fails closed: `git rev-parse --git-path info/grafts` prints a path "
     "whenever it succeeds (a failure is refused by the git() primitive, M809); an empty "
     "answer would name `repo` itself as the grafts file, which exists, so the graft refusal "
     "on the next lines fires"),
    (RD, "    if !rec.exists() {",
     "nothing to admit with: with no record file there are no bytes to verify or bind; the "
     "one read of the path (read_once) is the next statement"),
    (RD, '            return Err(format!("{component}: {k} is not a sha256"));',
     "a format check on fields of the OPERATOR-SIGNED record (signature verified over these bytes, "
     "M335); each is also compared for equality with a digest computed here (M695 spec, M349 "
     "observation, M342 B263, M691 bundle, M149 verifier, M151 preflight, M344 guest), which a "
     "non-hex value never equals"),
    (RD, '            return Err(format!("{component}: {k} is not a full commit id"));',
     "format check on fields of the operator-signed record (M335): axon_sha is then resolved by "
     "descends (M696), fabric_revision joined to the observation (M341), micode_sha is "
     "operator-attested by design (amendment 57)"),
    (RD, '    if ["id", "version", "entry", "test", "digest"]',
     "format check on the operator-signed record's suite (M335); each field is joined to the "
     "launch manifest (M747)"),
    (RD, "        if !trust.key_ids(dir)?.iter().any(|k| k == s(field)) {",
     "rowed where the same removal is made, per field of this loop: the observer_key_id entry "
     "is M418's and the verifier_key_id entry M338's, both retired EQUIVALENT_DID with executed "
     "four-cell records (M418 against M339/M340, M338 against M812, launched()'s lookup of "
     "verifier_key_id in the verifier root)"),
    (RD, "        [] => Err(format!(",
     "no document to admit with: an arm that let an empty match through would have to invent the "
     "run's bytes"),
    (RD, "        many => Err(format!(",
     "rowed: M749's mutation (`[(_, _, b), ..] => Ok(b)`) is the removal of this arm"),
    (RD, "    if b.schema != protected_evidence::PSV_EVIDENCE_SCHEMA {",
     "a version tag inside the certified run document, which the operator-signed evidence bundle "
     "digest binds (M691); its payload (manifest, verdict) is joined by digest to the attested "
     "receipt (M742)"),
    (PS, "    if doc.schema != GUEST_POLICY_SCHEMA {",
     "a version tag of a fixed-shape document (deny_unknown_fields) already pinned by digest to "
     "the launch manifest's policy_sha256 (the check before it), which the observation joins; "
     "the tag selects nothing, only allowed_effects is read"),
    (PS, "            Some(first) => Err(format!(",
     "rowed where the same removal is made: M350 and M351 (each call of no_xattr) and M352 (the "
     "predicate that selects `first`)"),
    (PS, '    Err(format!(\n        "{shown}, whose extended attributes this platform cannot list"',
     "compiled only for a unix that is not Linux (cfg): the guest and every protected host are "
     "Linux; it fails closed where it exists"),
    (PS, "        if self.schema != PREFLIGHT_OBSERVATION_SCHEMA {",
     "a version tag of a fixed-shape document (deny_unknown_fields) whose observer-root signature "
     "is verified over these bytes before joins is called (verify_observation; readiness's "
     "attribution, M340); every fact it carries is then joined (M773, M774)"),
    (OB, "            Err(_) if used.exists() => return Err(",
     "no record to continue with: a mutation admitting here would have to invent the `.issued` "
     "record, and the atomic rename of the missing `.issued` below fails ('already used')"),
    (OB, '            Err(_) => return Err(format!("nonce {nonce} was never issued by this custodian")),',
     "as above: no `.issued` record, and the rename below fails on it"),
]


# C9 round 4 fix wave, rows2 wave 2 (amendment 58): the sites the extended
# pattern (tail `Err(…)`, refusal constructors) names in the files scanned
# since, outside the loop crate. Kinds: NOTHING TO ADMIT (the arm holds an
# error and no value the code after it could run on), OS ERROR (fails closed:
# the operation does not happen), NAMED ROW (a registry row mutates this
# refusal's condition at another line; the removal is the same), NON-LINUX
# (compiled out), UNREACHABLE (no input reaches it; the fact is named).
PSVF = "crates/axon-fabric/src/psv.rs"
RUN = "crates/axon-psv/src/runner.rs"
PEV = "crates/axon-loop-contracts/src/protected_evidence.rs"
EXEMPT += [
    (PL, "        Err(why) => return (LaunchReport::refused(why), EXIT_REFUSED),\n    };\n    if caller_uid",
     "NOTHING TO ADMIT: load_config failed, so there is no config to launch under"),
    (PL, "    if let Err(why) = become_root() {",
     "OS ERROR: setresgid/setgroups/setresuid failing leaves the process not root in every id; "
     "the helper then launches nothing (the refusal). It can fail only without CAP_SETUID/SETGID, "
     "and the binary refuses before this unless its euid is 0 (M602)"),
    (PL, "        Err(why) => (LaunchReport::refused(why), EXIT_REFUSED),\n        Ok(p) => run(&c, p),",
     "NOTHING TO ADMIT: prepare failed, so there is no Prepared (verified programs, staged "
     "snapshot, out dir) to run"),
    (PL, "        Err(why) => {\n            let _ = std::fs::remove_dir_all(&staging);\n            Err(why)",
     "NOTHING TO ADMIT: snapshot/observation/out-dir failed, so no out dir exists to build a "
     "Prepared from; the staging dir is removed"),
    (SE, "        Err(std::io::Error::last_os_error())\n    }\n}\n\n/// Open `p` for execution",
     "OS ERROR: F_SETLEASE refused; the caller's Lease::Required refusal on it is M591 (killed on "
     "the production helper, amendment 55)"),
    (SE, "        Err(std::io::Error::last_os_error())\n    }\n}\n\n// The closure owns",
     "OS ERROR: execveat returned, so the verified program did not replace the child; nothing "
     "ran"),
    (CU, "            Err(e) => self.reply(Err(e)),",
     "NOTHING TO ADMIT: SO_PEERCRED failed, so there is no peer uid for decide() to judge"),
    (RD, '        Err("operator trust cannot be checked on this platform".into())',
     "NON-UNIX: compiled only under cfg(not(unix)); Linux builds the unix branch"),
    (PSVF, "    if cand_tree != req.workspace_version_ref.as_str() {",
     "UNREACHABLE: the candidate dir was created NEW (DirBuilder::create, not recursive) and "
     "0700 by materialize_inputs, and written by WorkspaceStore::materialize from store.tree(r), "
     "which re-derives r from hash-checked blobs (\"does not re-derive\"); between that write and "
     "this read only the custodian issue call runs, which is another uid's process. The guest "
     "holds the same digest again (M159)"),
    (PSVF, "    if suite_tree != i.suite_version {",
     "UNREACHABLE: as the candidate check above (the suite is materialized from the same ref)"),
    (PSVF, '        Err(e) => return unknown(format!("no guest verdict: {e}"), evidence, None),',
     "NOTHING TO ADMIT: no verdict bytes were read"),
    (PSVF, '        Err(e) => return unknown(format!("guest verdict is malformed: {e}"), evidence, None),',
     "NOTHING TO ADMIT: the bytes do not parse as a GuestVerdict"),
    (PSVF, '        Err(e) => return unknown(format!("no guest test output: {e}"), evidence, None),',
     "NOTHING TO ADMIT: no test output bytes were read"),
    (PSVF, '        Err(e) => {\n            return unknown(\n                format!("no verdict from the test output: {e}"),',
     "NOTHING TO ADMIT: the test output parses to no report"),
    (RUN, '        Ok(b) => {\n            return refused(\n                cfg,\n                "",\n                none,\n                "",\n                format!("completion secret is {} bytes, not 32", b.len()),',
     "NOTHING TO ADMIT: no 32-byte secret, so no key K; the length rule itself is M170"),
    (RUN, '        Err(e) => return refused(cfg, "", none, "", format!("completion secret: {e}")),',
     "NOTHING TO ADMIT: the secret was not read"),
    (RUN, '        Err(e) => return refused(cfg, "", none, "", format!("launch manifest: {e}")),',
     "NOTHING TO ADMIT: the manifest was not read"),
    (RUN, "        Err(e) => return refused(cfg, &sha256_hex(&m_bytes), none, \"\", e),",
     "NOTHING TO ADMIT: no verified manifest; the digest it is verified under is M167"),
    (RUN, "        Err(e) => return refused(cfg, &m_sha, none, &test, e),",
     "NAMED ROW: the Err is protected_policy_ceiling's, whose call is M673 and whose no-ceiling "
     "refusal is M674; with no policy there is no ceiling to run under"),
    (RUN, '        Err(e) => return refused(cfg, &m_sha, inputs, &test, format!("axon test: {e}")),',
     "NOTHING TO ADMIT: the test did not run to an exit status and output (spawn failed, killed "
     "at the wall-clock limit, or over the output limit, M781)"),
    (RUN, "                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0",
     "OS ERROR: pre_exec failing makes spawn fail; the test does not run"),
    (RUN, "                    if libc::setgroups(0, std::ptr::null()) != 0",
     "OS ERROR: pre_exec failing makes spawn fail; the test does not run (the uid drop is M172)"),
    (RUN, "    let Some(status) = status else {",
     "NOTHING TO ADMIT: the child was killed at the wall-clock limit, so there is no exit status; "
     "a verdict needs Some(0) for a pass (M293) and Some(nonzero) for a failure (M242)"),
    (PEV, "        [one] => {\n            return Err(format!(\n                \"the receipt's evidence class is {one}",
     "NAMED ROW: M212 (`[\"protected\"]` -> `[_]`) is the removal of this arm"),
    (PEV, '            [d] => return Err(format!("{prefix}{d} is not a sha256")),',
     "NAMED ROW: M217 (retired EQUIVALENT, four-cell record) drops the hex condition of the arm "
     "above, which makes this arm unreachable: the same removal"),
    (PEV, '            _ => {\n                return Err(format!(\n                    "a protected receipt names {prefix}… more than once"',
     "NAMED ROW: M216 (retired EQUIVALENT, four-cell record) widens the arm above to `[d, ..]`, "
     "which makes this arm unreachable: the same removal"),
    (PEV, "        [] => {\n            return Err(format!(\n                \"the observation is signed by {signer}, which is no observer",
     "NAMED ROW: M470 (the key filter that makes `by` empty) is the removal of this refusal"),
    (PEV, "        _ => {\n            return Err(format!(\n                \"the observation is signed by {signer}, which {} trusted observers share",
     "NAMED ROW: M478 (`[one]` -> `[one, ..]`) is the removal of this arm"),
]

# C9 round 4 fix wave, rows2 wave 2, LOOP (admission.rs, intake.rs). Kinds:
# NOT A SITE (the body of a refusal constructor, each use a site of its own),
# NOTHING TO ADMIT (an error or a None with no value the code could continue
# with: a mutation letting it through would have to invent the value), a
# CONDITION A NAMED ROW MUTATES at another line (that row ACTIVE and killed).
LA = "crates/axon-loop/src/admission.rs"
LI = "crates/axon-loop/src/intake.rs"
EXEMPT += [
    (LA, "        refused(format!(\n            \"trial {}'s protected verdict does not re-verify",
     "NOT A SITE: the body of reverify_protected's `fail` constructor; each use is its own site"),
    (LA, "    .map_err(|e| fail(e.to_string()))?;\n    // The record's attribution IS the signer",
     "NOTHING TO ADMIT: verify_check_evidence failed, so there is no verified receipt, key or "
     "observation signer to destructure (the tuple exists only on Ok)"),
    (LA, "        (o, r) => {\n            return Err(fail(format!(",
     "A CONDITION A NAMED ROW MUTATES: the arm is the complement of the two matching arms M268 "
     "replaces with `(_, _) if true => {}` (ACTIVE, killed)"),
    (LA, '        serde_json::from_str(&ctx_text).map_err(|e| fail(format!("context: {e}")))?;',
     "NOTHING TO ADMIT: the stored context does not parse, so there is no context to verify"),
    (LA, '        .ok_or_else(|| fail("it cites no context signature".into()))?;',
     "NOTHING TO ADMIT: no context signature ref, so there is no signature to verify"),
    (LA, '        .map_err(|e| fail(format!("context signature: {e}")))?;\n    let who',
     "NOTHING TO ADMIT: the stored context signature does not parse"),
    (LA, '        .map_err(|e| fail(format!("context observer: {e}")))?;',
     "NOTHING TO ADMIT: the context names no valid observer identity"),
    (LA, "        fail(format!(\n            \"its protected context is not authenticated: observer {who}",
     "NOTHING TO ADMIT: rooted_key found no operator-rooted key for the observer, so there is no key "
     "to verify the context signature under"),
    (LA, '            .map_err(|e| fail(format!("execution request: {e}")))?;',
     "NOTHING TO ADMIT: the stored execution request does not parse"),
    (LA, '            .map_err(|e| fail(format!("execution receipt: {e}")))?;',
     "NOTHING TO ADMIT: the stored execution receipt does not parse"),
    (LA, '        .map_err(|e| fail(format!("execution attestation: {e}")))?;',
     "NOTHING TO ADMIT: the stored execution attestation does not parse"),
    (LA, "                    refused(format!(\n                        \"trial {}'s verdict is no longer pinned",
     "A CONDITION A NAMED ROW MUTATES: the check_pins call whose error this maps is M103's "
     "(ACTIVE, killed)"),
    (LA, "                    return Err(refused(format!(\n                        \"trial {}'s protected context is not authenticated",
     "A CONDITION A NAMED ROW MUTATES: M104 disables this refusal's condition at its first line "
     "(ACTIVE, killed)"),
    (LA, "                    return Err(refused(format!(\n                        \"trial {}'s clearance is not from a monitor",
     "A CONDITION A NAMED ROW MUTATES: M105 (the clearance condition) and M264 (its signature "
     "re-verification) (ACTIVE, killed)"),
    (LI, "        Refusal::Semantic(s) => refused(format!(\"{what}: {s}\")),",
     "NOT A SITE: the body of the `semantic` converter; every use is `parse(..).map_err(semantic(..))?`, "
     "its own site with no parsed value on Err"),
    (LI, "    if ep.policy_ref.scheme() != RefScheme::Cl22 {",
     "UNREACHABLE ALONE (flagged for the integrator: the strict reading may require a four-cell "
     "record, which cannot be built): no stored record is named other than by the cl22 digest of "
     "its content: Store::cas_path refuses any other scheme and check_name requires the name to "
     "equal digest(content), always cl22; so with this check removed the next statement's policy "
     "lookup refuses the same reference, and with that lookup's scheme check also removed "
     "check_name, then bind_episode's policy digest, still refuse it. Measured: the row (former "
     "M833) is REFUSED_ELSEWHERE ('not a policy this store knows')"),
    (LI, "        Err(LoopError::Refused(_)) => {\n            return Err(refused(format!(",
     "NOTHING TO ADMIT: the store holds no such policy, so there is no PolicyEnvelope to bind"),
    (LI, "        Err(e) => return Err(e),\n    };\n    if tx.is_revoked",
     "NOTHING TO ADMIT: the policy could not be read (an I/O or corrupt-record error)"),
    (LI, '                .map_err(|e| refused(format!("psv evidence bundle: {e}")))?;',
     "NOTHING TO ADMIT: the bundle text does not parse, so there is no value to store; the same "
     "text was already parsed and joined by check_bundle (M218/M230)"),
    (LI, '    let obj = v.as_object().ok_or_else(|| shape("ack: not an object"))?;',
     "NOTHING TO ADMIT: the ack is not an object, so it has no fields to check"),
    (LI, '                .ok_or_else(|| shape(format!("ack: candidate {c} is not a candidate id")))',
     "NOTHING TO ADMIT: the entry is not a CandidateId, so there is no candidate to add"),
    (LI, "        [] => Err(refused(format!(\n            \"no ack:",
     "NOTHING TO ADMIT: no ack joins the episode"),
    (LI, "        refused(format!(\n            \"projection_ref {want} names a PolicyProjection",
     "NOTHING TO ADMIT: the episode names a projection and none was presented"),
    (LI, "        return Err(refused(format!(\n            \"verifier_ref {vref} names a Fabric check receipt",
     "NOTHING TO ADMIT: the verification request and receipt were not both presented"),
    (LI, '        .ok_or_else(|| refused("the episode cites no verifier_ref"))?;',
     "NOTHING TO ADMIT: no verifier_ref to join the receipt to"),
    (LI, "            refused(format!(\n                \"protected evidence from verifier {issuer} carries no",
     "NOTHING TO ADMIT: a protected claim with no psv bundle has nothing to join"),
    (LI, "            refused(format!(\n                \"protected evidence from verifier {issuer} does not join",
     "NOTHING TO ADMIT: check_bundle failed, so there is no verified observation (its observer and "
     "key) to attribute; the call itself is M218/M230's (ACTIVE, killed)"),
    (LI, "        refused(format!(\n            \"the verification is not authenticated: no acf-receipt",
     "NOTHING TO ADMIT: no attestation text to verify"),
    (LI, "        refused(format!(\n            \"verifier {issuer} has no operator pin",
     "NOTHING TO ADMIT: no operator pin for the verifier to compare the check with"),
    (LI, "        return Err(refused(format!(\n            \"rubric: the check ran {entry:?}, a file",
     "NOTHING TO ADMIT: the check names no registered suite id to join to the pin"),
    (LI, "        refused(format!(\n            \"acceptance: task {} has no operator-registered",
     "NOTHING TO ADMIT: the task has no registered acceptance check to compare with"),
    (LI, "            Some(x) => x.as_u64().map(|n| Some(Some(n))).ok_or_else(|| {\n                shape(format!(",
     "NOTHING TO ADMIT: the spend is not a number, so there is no spend to read"),
    (LI, "                let hint = if mc == 0 && c > 0 {",
     "A CONDITION A NAMED ROW MUTATES: this refusal's condition is `if c != want` two lines up, "
     "M846's (ACTIVE, killed); the `if` here only chooses the message"),
    (LI, "        None => Err(shape(\n            \"source episode: cost states neither",
     "NOTHING TO ADMIT: the canonical episode states no cost field at all"),
]


# ── C9 round 4b, workstream ROWS4B (amendment 62): the Fabric decision files
# the round-4b EQUIVALENCE review found unscanned. Every refusal site in them is
# rowed (M1020-M1083) or exempt below. Kinds, as above, plus: DEVELOPMENT
# ROUTE (reached only when the selected profile is the local one, whose
# receipts are Development class, M189; the protected profile launches only an
# operator suite `check:<id>` with a named test, M400/M187, and a protected
# host runs nothing else, M265); NOT ON THE PROTECTED ROUTE (no caller on the
# submit/PSV/readiness paths, callers named); NOT A VERDICT PROPERTY (decides
# billing, idempotence or a state transition: a fresh run's `ran_under` is
# built in memory (submit.rs `RanUnder`), the journal's recorded intent and
# outcome are read back only for a replay or `status`, and a replay is never
# signed (M01, M402)); USAGE (a missing or malformed argument: the command
# runs, signs and writes nothing). FLAGGED marks a reason the integrator must
# accept or replace.
PROTECTED += [
    "crates/axon-fabric/src/backend.rs",
    "crates/axon-fabric/src/submit.rs",
    "crates/axon-fabric/src/git_data.rs",
    "crates/axon-fabric/src/provenance.rs",
    "crates/axon-fabric/src/bin/axon-fabric.rs",
    "crates/axon-fabric/src/bin/axon-provenance.rs",
    "crates/axon-fabric/src/signing.rs",
    "crates/axon-fabric/src/workspace.rs",
    "crates/axon-fabric/src/journal.rs",
    "crates/axon-fabric/src/branches.rs",
    "crates/axon-fabric/src/grants.rs",
]
FB = "crates/axon-fabric/src/backend.rs"
FS = "crates/axon-fabric/src/submit.rs"
FG = "crates/axon-fabric/src/git_data.rs"
FP = "crates/axon-fabric/src/provenance.rs"
FC = "crates/axon-fabric/src/bin/axon-fabric.rs"
FN = "crates/axon-fabric/src/signing.rs"
FW = "crates/axon-fabric/src/workspace.rs"
FJ = "crates/axon-fabric/src/journal.rs"
FR = "crates/axon-fabric/src/branches.rs"
FA = "crates/axon-fabric/src/grants.rs"
_DEV = ("DEVELOPMENT ROUTE: reached only for a local-profile run or an argv[0] that is not "
        "`check:<id>`; the protected profile launches only an operator suite with a named test "
        "(submit's suite-only refusal after check_target, M400/M187) and a local receipt is "
        "Development class (M189)")
_JNV = ("NOT A VERDICT PROPERTY: a journal state transition, budget or settlement rule; no "
        "verdict, receipt class or attestation is derived from the journal (a fresh run's ran_under "
        "is built in memory; the recorded intent/outcome is read back only by a replay, never "
        "signed (M01, M402), or by `status`)")
_BRN = ("NOT ON THE PROTECTED ROUTE: Branches::open_experiment / publish / cancel have no caller "
        "outside tests (tests/branches.rs, tests/restart_matrix.rs); submit calls only "
        "branch_of_run and is_cancelled (the cancelled-branch refusal is M1066)")
_IO = "OS ERROR: the operation failed; nothing is run, signed or written (fails closed)"
_USE = "USAGE: a missing or malformed argument; the command runs, signs and writes nothing"
_NTA = "NOTHING TO ADMIT: the arm holds an error and no value the code after it could run on"
EXEMPT += [
    # backend.rs
    (FB, 'pub const TEST_TRUST_BUILD: bool = cfg!(any(test, feature = "test-trust-root"));',
     "NOT A SITE: the definition of the build constant; every read of it decides on its own line "
     "(verify_evidence_authority M415, attests_protected M545/M605, the binary's flag refusal)"),
    (FB, '    Err(format!(\n        "{}: operator ownership cannot be checked on this platform",',
     "NON-UNIX: compiled only under cfg(not(unix)); fails closed where it exists"),
    (FB, '        return Err(format!("{} is larger than {MAX} bytes", p.display()));',
     "RESOURCE BOUND: with it removed read_regular still returns the ONE buffer every caller "
     "verifies or hashes (at most MAX+1 bytes, a prefix the signature or pinned digest must then "
     "cover byte for byte); it bounds memory and decides no fact"),
    (FB, '    if std::fs::symlink_metadata(sig_path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)',
     "RE-REPORT: the next statement, read_regular(sig_path), fails on the same missing path "
     "(ENOENT); this only names it RULE:unsigned"),
    (FB, '        if !p.job_kinds.contains(&req.job_kind) {\n            return Err(Unsupported(format!(\n                "{}: job_kind {:?} unsupported — the protected profile',
     "NAMED ROW: M321 adds InterpreterRun to the job_kinds this condition reads; JobKind has two "
     "variants, so the only kind this refusal can refuse is admitted by either removal"),
    (FB, '        Err(e) => return refused(format!("privileged launcher: {e}")),',
     "NOTHING TO ADMIT: open_verified failed, so there is no verified helper descriptor to "
     "execute; the pin, owner and lease rules themselves are sealed_exec.rs's (scanned)"),
    (FB, '        Err(e) => return refused(format!("privileged launcher did not start: {e}")),',
     "OS ERROR: the helper did not start; nothing was launched"),
    (FB, '        Err(e) => {\n            return unknown(\n                format!("privileged launcher (exit {status:?}) gave no report: {e}"),',
     "NOTHING TO ADMIT: the helper's output is no LaunchReport, so there is no report to judge"),
    (FB, '            _ => unknown(\n                format!("privileged launcher exited {status:?}: {why}"),',
     "NOTHING TO ADMIT: the report says nothing was launched, so there is no result to "
     "interpret; the arm only chooses Unknown over Refused (billing), never a verdict"),
    (FB, '        Err(e) => return refused(format!("launcher: {e}")),',
     "DEVELOPMENT ROUTE (run_direct, never protected: M544) and NOTHING TO ADMIT: no verified "
     "launcher descriptor to execute"),
    (FB, '        Err(e) => return refused(format!("launcher interpreter: {e}")),',
     "DEVELOPMENT ROUTE (run_direct, never protected: M544) and NOTHING TO ADMIT: no verified "
     "interpreter descriptor"),
    (FB, '        return refused(format!("out root {}: {e}", lx.out_root.display()));',
     "DEVELOPMENT ROUTE (run_direct, M544) and OS ERROR: the out root could not be created"),
    # submit.rs
    (FS, '    if req.executable_digest != want {', _DEV + " (resolve_executable is the `else` of the "
     "protected-profile executable binding, M1056/M1057)"),
    (FS, '        (JobKind::InterpreterRun, _) => {',
     "UNREACHABLE: check_target runs only after backend::select succeeded, and select refuses "
     "every interpreter_run (local: M1054; protected: M321; the Axon-kernel VM: M1052)"),
    (FS, '            .any(|c| !matches!(c, std::path::Component::Normal(_)))', _DEV),
    (FS, '            if r != *want {', _DEV),
    (FS, '        } else if *want == workspace_digest(&file, &bytes) {', _DEV),
    (FS, '        .any(|e| e.path == file && e.mode != workspace::MODE_LINK)', _DEV),
    (FS, '    if req.job_kind != JobKind::RegisteredCheck {',
     "UNREACHABLE: as the InterpreterRun arm, only an interpreter_run could reach "
     "check_suite_target with another job kind, and select refuses every one"),
    (FS, '    if !store.contains(cand) {',
     "RE-REPORT: the next statement, store.load(cand), fails on a ref the store does not hold "
     "(StoreError::NotPublished)"),
    (FS, '        .any(|e| e.path == c.entry && matches!(e.kind, workspace::EntryKind::File { .. }))',
     "OPERATOR-AUTHORED: entry and tree are fields of the operator's registry entry, the tree "
     "equal to the registered ref (M1061); a missing entry gives the interpreter nothing to run, so "
     "no PASS (fails closed)"),
    (FS, '            Err(_) => axon_os::DeclaredEffects::unknown(),',
     "NOT A SITE: the deny-by-default effect row (an unreadable program declares every effect), "
     "not the `unknown(` refusal constructor"),
    (FS, '            if let Err(e) = journal.reserve_within(&req.operation_id, exp.regime, move |i| {',
     _JNV + " (a branch's budget regime; an op left Intended is refused by mark_launched)"),
    (FS, '                    &format!("profile no longer qualified: {e}"),',
     "NOTHING TO ADMIT: the launch-time re-qualification failed, so there is no "
     "LinuxQualification to launch or receipt under (its rules are rowed, M1025-M1043)"),
    (FS, '                journal.cancel(&req.operation_id, &format!("not launched: {e}"), None)?;',
     "NOTHING TO ADMIT: no host executor was built"),
    (FS, '    if let Some(Err(e)) = local.as_ref().map(|l| l.verify()) {',
     _DEV + "; also cortex run_checks re-verifies the pin (verified_path) before it spawns"),
    (FS, '                        now => Err(format!(',
     "NAMED ROW: M253 replaces the arm above with `_ => Ok((launch, Some(v)))`, which makes this "
     "arm unreachable: the same removal"),
    (FS, '                    let why = if why.starts_with("preflight observation refused") {',
     "NOT A SITE: Journal::fail records the terminal state the arm already decided"),
    (FS, '            let why = format!("backend {other} has no dispatcher");',
     "NOT A SITE (Journal::fail) and UNREACHABLE: select returns only the local or the protected "
     "profile"),
    (FS, '        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {',
     "NOT A SITE: Journal::fail records the timeout the arm decided (Unknown, liability kept)"),
    (FS, '            // or no summary): failed, verification unknown, liability kept.',
     "NOT A SITE: Journal::fail records the arm's decision"),
    (FS, '        ReceiptStatus::OutcomeUnknown => {\n            journal.fail(&req.operation_id, &res.reason, Billing::Unknown)?',
     "NOT A SITE: Journal::fail records the receipt status already decided"),
    (FS, '        _ => journal.fail(&req.operation_id, &res.reason, Billing::Unknown)?,',
     "NOT A SITE: Journal::fail records the receipt status already decided"),
    (FS, '    journal.fail(\n        &req.operation_id,\n        why,\n        Billing::Known(ResourceVector::default()),',
     "NOT A SITE: Journal::fail records an unsupported receipt as failed (nothing launched)"),
    # git_data.rs
    (FG, '        return Err(format!("git {} failed", args.join(" ")));',
     "FLAGGED, RE-REPORT on every caller: each caller refuses the empty answer a failed git leaves "
     "(rev-parse HEAD '' -> Objects::read error; disambiguate '' -> the (None, _) arm; cat-file -t "
     "'' -> M1073; git-common-dir '' -> canonicalize error; grafts path '' names the repo, whose "
     "graft refusal fires); in provenance an empty revision fails head_bytes_differ"),
    (FG, '        _ => return Err(format!("{} is not a git directory", dotgit.display())),',
     "NOTHING TO ADMIT: no git directory to read the config from (every caller has passed "
     "discover / discover_linked first)"),
    (FG, '        Err(e) => return Err(format!("{}: {e}", gitdir.join("commondir").display())),', _IO),
    (FG, '    if std::fs::symlink_metadata(&wt).is_ok() {',
     "FLAGGED, DOMINATED: git reads config.worktree only under extensions.worktreeConfig, and "
     "refuse_config refuses every extensions.* key (allowed_key, M450)"),
    (FG, '        return Err(format!("cannot read {path}: refused, never interpreted"));',
     "FLAGGED, RE-REPORT: every later git call parses the same config and fails on it through "
     "run()'s status check; a config git cannot read is never interpreted as empty"),
    (FG, '                        return Err(format!("commit {c} names a malformed parent"));',
     "RE-REPORT: the parent is then read by Objects::read, which refuses a name that is not an "
     "object id hashing to its bytes (M289)"),
    (FG, '        if lines.next() != Some(ALLOWLIST_SCHEMA) {',
     "OPERATOR-AUTHORED: the allowlist is read only after its chain is root-owned and unwritable "
     "(M502, M1070, M1071); an entry covering a source is still refused (M503)"),
    (FG, '            if bad {', "OPERATOR-AUTHORED: as the allowlist schema above"),
    (FG, '        return Err(format!("{} is not an absolute path", path.display()));',
     "RE-REPORT: the next statement, strip_prefix(base), refuses a relative path; in production "
     "the path is the absolute ALLOWLIST_PATH constant"),
    (FG, '            return Err(format!("{} is a symlink", p.display()));',
     "RE-REPORT: a symlink's own lstat mode is 0777, so the mode rule on the same metadata "
     "(M1071) refuses it, and the open below is O_NOFOLLOW"),
    (FG, '        Err(e) => return Err(format!("{}: {e}", src.path.display())),', _IO),
    (FG, '        if !md.is_file() || md.dev() != lst.dev() || md.ino() != lst.ino() {',
     "OPERATOR-AUTHORED: the chain was just checked root-owned and unwritable by others, so only "
     "root can swap the file between lstat and open; a directory fails read_to_end"),
    (FG, '        Err("operator ownership cannot be checked on this platform".into())',
     "NON-UNIX: compiled only under cfg(not(unix))"),
    (FG, '        return Err(format!("{rev:?} is not a revision"));\n    }\n    let head',
     "OPERATOR-AUTHORED: the certified revision is a constant of the build script "
     "(PCI_CERTIFIED) or readiness's full axon_sha; object_named refuses anything not hex "
     "(the next rule) and git is never asked to parse an option"),
    (FG, '    {\n        return Err(format!(\n            "{rev:?} is not an object id or an abbreviation',
     "OPERATOR-AUTHORED: as above; a name that is not hex never reaches git as a revision"),
    (FG, '        (None, _) => Err(format!("no object is named {rev}")),',
     "NOTHING TO ADMIT: no object is named, so there is nothing to peel"),
    (FG, '    } else {\n        Err(format!(\n            "{} is a symlink: refused",',
     "DEVELOPMENT ROUTE: discover_linked's only caller is axon-provenance --descends, the guest "
     "build's early check; the manifest's lineage (--lineage) goes through discover (M1072)"),
    # provenance.rs
    (FP, '        Err(e) => return unknown(e),\n    };\n    // The repository',
     "NOTHING TO ADMIT: no working tree top, so provenance is unknown with a dirty reason "
     "(fails closed)"),
    (FP, '        Err(e) => return unknown(e),\n    };\n    let watch',
     "NOTHING TO ADMIT: no HEAD revision, so provenance is unknown with a dirty reason"),
    (FP, '    if rev.is_empty() || rev.starts_with(\'-\') {\n        return Err(format!("{rev:?} is not a revision"));\n    }\n    git_data::refuse_config(&top)?;',
     "OPERATOR-AUTHORED: as git_data::descends's identical check, which runs on the same rev next"),
    (FP, '        return Err("git check-ignore did not account for every ignored path".into());',
     "FLAGGED, DOMINATED: M414's property (retired, subsumed by M501): the tree walk reports every "
     "ignored untracked object whatever check-ignore answers"),
    # signing.rs
    (FN, '            _ => Err(WRONG_CLASS),\n        }\n    } else {\n        Err(KEY_REACHABLE)',
     "NAMED ROW (68): M42's `} else if true {` sends every local run to the development arm, the "
     "removal of this branch; (65) UNREACHABLE: a local receipt's first class ref is Development "
     "(inserted by submit) and a replay is refused before (M01/M402)"),
    # bin/axon-fabric.rs
    (FC, 'fn refuse(kind: &str, reason: &str, code: i32) -> ! {', "NOT A SITE: the definition of refuse"),
    (FC, '            .unwrap_or_else(|| refuse("usage", &format!("{flag} is required"), 2))', _USE),
    (FC, '                .unwrap_or_else(|_| refuse("usage", &format!("{flag} must be a number"), 2)),', _USE),
    (FC, '        _ => refuse(\n            "usage",', _USE),
    (FC, '    let bad = |why: String| -> ! { refuse("unregistered", &format!("registry signer: {why}"), 4) };',
     "NOT A SITE: signer()'s refusal closure (its only call is the no-protected-host, development "
     "arm)"),
    (FC, '    let bad = |why: String| -> ! { refuse("unregistered", &format!("{what}: {why}"), 4) };',
     "NOT A SITE: signer_from's refusal closure; its uses are rows M140, M348, M488, M1078, M1079, "
     "M1080 (FLAGGED: the scan does not see `bad(` uses; they are rowed here regardless)"),
    (FC, '        axon_fabric::backend::TEST_TRUST_BUILD,',
     "NAMED ROW: the argument is the build constant; verify_evidence_authority's use of it is "
     "M415, and it only withholds authority"),
    (FC, '                "build": if axon_fabric::backend::TEST_TRUST_BUILD { "test-trust" } else { "production" },',
     "NOT A SITE: a status field printed, deciding nothing"),
    (FC, '        Err(e) => refuse("unregistered", &e, 4),\n    }\n}',
     "NOTHING TO ADMIT: the record did not verify, so there is no issuer to print"),
    (FC, '        if !axon_fabric::backend::TEST_TRUST_BUILD {',
     "cfg: in a production build the block that would honour the test flags is compiled out "
     "(#[cfg(feature = \"test-trust-root\")]), so with the refusal removed the flags are ignored and "
     "the operator config is read; in a test-trust build it never fires"),
    (FC, '                    .unwrap_or_else(|e| refuse("unregistered", &format!("protected host: {e}"), 4)),',
     "cfg: compiled only in a test-trust build (#[cfg(feature = \"test-trust-root\")])"),
    (FC, '        .unwrap_or_else(|e| refuse("unregistered", &format!("protected host: {e}"), 4))\n}',
     "FLAGGED, UNTESTED: an operator config that exists but does not load must refuse rather than "
     "read as no protected host; operator() reads only /etc/axon, which no test may write, so no "
     "test reaches it (the loader's own rules are protected_host.rs's, scanned)"),
    (FC, '        refuse(\n            "usage",\n            "--authority must be', _USE),
    (FC, '    let key = std::fs::read(a.req("--key")).unwrap_or_else(|e| refuse("io", &e.to_string(), 2));',
     _IO + " (sign-evidence, an operator tool)"),
    (FC, '        .unwrap_or_else(|_| refuse("usage", "--key is not an Ed25519 PKCS#8 key", 2));',
     "NOTHING TO ADMIT: no key pair to sign with"),
    (FC, '    let bytes = std::fs::read(&record).unwrap_or_else(|e| refuse("io", &e.to_string(), 2));', _IO),
    (FC, '    std::fs::write(&out, sig.to_string()).unwrap_or_else(|e| refuse("io", &e.to_string(), 2));', _IO),
    (FC, '        .unwrap_or_else(|e| refuse("io", &e.to_string(), 2));\n    let roots', _IO),
    (FC, '        .unwrap_or_else(|e| refuse("unregistered", &e, 4));\n    // A: the privileged',
     "NOTHING TO ADMIT: the trust preflight's path list could not be built; the same configs are "
     "parsed and refused by the protected-host, launcher and custodian loaders (scanned)"),
    (FC, '        .unwrap_or_else(|e| refuse("unregistered", &e, 4));\n    for hp in helper_paths',
     "NOTHING TO ADMIT: as above (the helper config's paths)"),
    (FC, '        .unwrap_or_else(|e| refuse("unregistered", &e, 4));\n    for cp in cust_paths',
     "NOTHING TO ADMIT: as above (the custodian config's paths)"),
    (FC, "        if s.contains(['\\t', '\\n']) || s != p.as_os_str().to_str().unwrap_or_default() {",
     "OPERATOR-AUTHORED: the paths come from the operator configs the preflight names; a path the "
     "preflight cannot print unambiguously is refused rather than listed (fails closed)"),
    (FC, '        .unwrap_or_else(|_| refuse("io", "key generation failed", 2));', _IO + " (keygen)"),
    (FC, '        .unwrap_or_else(|_| refuse("io", "generated key does not load", 2))', _IO + " (keygen)"),
    (FC, '            .unwrap_or_else(|e| refuse("io", &format!("{}: {e}", out.display()), 2));',
     "NOTHING TO ADMIT: create_new refused, so an existing key is never overwritten"),
    (FC, '            .unwrap_or_else(|e| refuse("io", &e.to_string(), 2));\n    }', _IO),
    (FC, '            .unwrap_or_else(|e| refuse("unauthorized", &e, 7)),',
     "NOTHING TO ADMIT: the development registry does not load (no protected host), so there are "
     "no grants to resolve"),
    (FC, 'fn refuse_caller_grant_registry() -> ! {',
     "NOT A SITE: the body of refuse_caller_grant_registry; its uses are M274 (and M273, retired)"),
    (FC, '            .read_to_string(&mut s)', _IO + " (the request)"),
    (FC, '        std::fs::read_to_string(&req_src).unwrap_or_else(|e| refuse("io", &e.to_string(), 2))', _IO),
    (FC, '    let registry = axon_cortex::runner::CheckRegistry::load(&registry_path)',
     "NOTHING TO ADMIT: no check registry; on a protected host it is the operator's, loaded at "
     "its pin by protected_host.rs"),
    (FC, '        scope(&a.req("--tenant"), &a.req("--family")).unwrap_or_else(|e| refuse("usage", &e, 2));', _USE),
    (FC, '        .unwrap_or_else(|e| refuse("usage", &format!("--expected-epoch: {e}"), 2));', _USE),
    (FC, '            .unwrap_or_else(|e| refuse("unregistered", &e, 4)),',
     "NOTHING TO ADMIT: the protected host's authority store is unusable; the store rule is "
     "protected_host.rs's"),
    (FC, '                    .unwrap_or_else(|e| refuse("io", &e.to_string(), 2));', _IO + " (output)"),
    (FC, '                    .unwrap_or_else(|e| refuse("io", &e, 2)),', _IO + " (output)"),
    (FC, '                            .unwrap_or_else(|e| refuse("malformed", &e.to_string(), 3));',
     "UNREACHABLE: submit() already parsed the same request text"),
    (FC, '                            .unwrap_or_else(|e| refuse("io", &e, 2));', _IO + " (output)"),
    (FC, '        Err(e) => refuse(e.kind(), &e.to_string(), e.exit_code()),',
     "NOTHING TO ADMIT: submit refused, so there is no submission to print"),
    (FC, '        .unwrap_or_else(|e| refuse("usage", &format!("--tenant: {e}"), 2));', _USE),
    (FC, '            .unwrap_or_else(|e| refuse("workspace", &e.to_string(), 2));', "NOTHING TO ADMIT: no store"),
    (FC, '    .unwrap_or_else(|e| refuse(e.class(), &e.to_string(), 3));', "NOTHING TO ADMIT: no tree imported"),
    (FC, '        .unwrap_or_else(|e| refuse("workspace", &e.to_string(), 2));\n    println!(',
     "NOTHING TO ADMIT: nothing published"),
    (FC, '        .unwrap_or_else(|e| refuse("unauthorized", &e, 7));\n    let op',
     "FLAGGED, DOMINATED on the existing attacks by M1077 (a wrong principal|grant is refused by "
     "the op's recorded binding); its unique case is a grant revoked after submission"),
    (FC, '    let op = OperationId::new(a.req("--op")).unwrap_or_else(|e| refuse("usage", &e.to_string(), 2));', _USE),
    (FC, '        .unwrap_or_else(|e| refuse("journal", &e.to_string(), 2))\n        .unwrap_or_else(|| refuse("unknown_op"',
     "NOTHING TO ADMIT: no journal"),
    (FC, '        .unwrap_or_else(|| refuse("unknown_op", &format!("no operation {op}"), 5));',
     "NOTHING TO ADMIT: the journal holds no op"),
    (FC, '        refuse("unknown_op", &format!("no operation {op}"), 5)\n    };\n    if v.intent',
     "NOTHING TO ADMIT: no such op"),
    (FC, '        .unwrap_or_else(|e| refuse("journal", &e.to_string(), 2));\n    (j, op)', _NTA),
    (FC, '        None => refuse("unknown_op", &format!("no operation {op}"), 5),',
     "UNREACHABLE: authorized() already found the op's view"),
    (FC, '        refuse("unknown_op", &format!("no operation {op}"), 5)\n    };\n    let billing',
     "UNREACHABLE: authorized() already found the op's view"),
    (FC, '        Err(e) => refuse("journal", &e.to_string(), 5),', _NTA + " (the cancel transition failed)"),
]
EXEMPT += [
    # workspace.rs
    (FW, '            return Err(ImportRefusal::QuotaEntries {',
     "RESOURCE BOUND: without it a larger tree is admitted, but its reference still covers exactly "
     "its bytes, and the guest's walk applies its own quota"),
    (FW, '                return Err(ImportRefusal::QuotaBytes { limit: quota.bytes });', "RESOURCE BOUND: as above"),
    (FW, '            return Err(ImportRefusal::Duplicate(w[0].path.clone()));',
     "FLAGGED, UNREACHABLE ALONE: a directory lists each name once (import), and a stored "
     "manifest with a duplicate hashes to no reference publish wrote (load refuses it, M1083)"),
    (FW, '                        return Err(ImportRefusal::Collision {',
     "NOT A VERDICT PROPERTY: the case-folding rule is for case-insensitive filesystems; on the "
     "Linux guest and host each spelling is its own file and the guest's digest still equals the "
     "reference (M159); a file/directory collision fails create_dir_all (fails closed)"),
    (FW, '            return Err(ImportRefusal::SpecialFile(rel.into()));',
     "DEVELOPMENT ROUTE: import_file's only caller is check_target's plain argv file (submit)"),
    (FW, '        if meta.len() > quota.bytes {', "DEVELOPMENT ROUTE and RESOURCE BOUND: as above"),
    (FW, '        if have != bytes {',
     "FLAGGED, DOMINATED: a blob or manifest file holding other bytes than its name is refused "
     "when read (tree() re-verifies every blob, M1081/M1082; load() the manifest, M1083)"),
    (FW, '            if std::fs::read(dest)? != bytes {', "FLAGGED, DOMINATED: as above (a concurrent publisher)"),
    (FW, '        Err(e) => Err(e.into()),\n    }\n}', _IO),
    (FW, '    }\n    Err(e)', _IO + " (renameat2)"),
    (FW, '            Err(e) => return Err(e.into()),\n        }\n        if files.is_empty() {', _IO),
    (FW, '            return Err(StoreError::Corrupt(format!("{r} has no omission record")));',
     "NOT A VERDICT PROPERTY: omissions are not part of the reference; they are read only into "
     "WorkspaceTree.omissions, which only `axon-fabric workspace import` prints"),
    (FW, '            if f.parent() == Some(self.omissions_dir(r).as_path())', "NOT A VERDICT PROPERTY: as above"),
    (FW, '                return Err(StoreError::NotPublished(r.to_string()))', "NOTHING TO ADMIT: no manifest"),
    (FW, '            Err(e) => return Err(e.into()),\n        };\n        if workspace_version_ref', _IO),
    (FW, '            return Err(StoreError::DestinationExists(dest.to_path_buf()));',
     "UNREACHABLE on the protected route: psv.rs creates the inputs directory NEW (not recursive) "
     "and materializes into new subdirectories (FLAGGED: RunDir::new on the development route "
     "uses create_dir_all)"),
    (FW, '                    return Err(StoreError::Io(format!("no symlinks here: {target}")));',
     "NON-UNIX: compiled only under cfg(not(unix))"),
    # journal.rs
    (FJ, '                return Err(name);', _JNV),
    (FJ, '                    return Err(JournalError::ScopeConflict {', _JNV),
    (FJ, '                    return Err(JournalError::UnknownScope(Box::new(intent.scope.clone())));', _JNV),
    (FJ, '                        return Err(JournalError::Conflict {\n', _JNV + " (one op, one intent; submit's input-digest rule is M1063)"),
    (FJ, '                    return Err(bad(&v, "reserved"));', _JNV),
    (FJ, '                    return Err(JournalError::BudgetExceeded {', _JNV),
    (FJ, '                    return Err(bad(&v, "launched"));', _JNV),
    (FJ, '                    return Err(bad(&v, "completed"));', _JNV),
    (FJ, '                    return Err(bad(&v, "failed"));', _JNV),
    (FJ, '                    _ => return Err(bad(&v, "cancelled")),', _JNV),
    (FJ, '                    return Err(bad(&v, "outcome_unknown"));', _JNV),
    (FJ, '                        return Err(JournalError::SettlementConflict {', _JNV),
    (FJ, '                    _ => return Err(bad(&get(op)?, "settle_conflict")),', _JNV),
    (FJ, '                    return Err(bad(&v, "outcome"));', _JNV),
    (FJ, '            return Err(JournalError::InvalidSettlement(', _JNV),
    (FJ, '            return Err(JournalError::InvalidTransition {', _JNV),
    (FJ, '            return Err(JournalError::Locked(path.to_path_buf()));', "OS ERROR: the lock is still held at the deadline; nothing runs"),
    (FJ, '            Err(e) => return Err(JournalError::Io(e)),', "OS ERROR: stat failed other than NotFound (NotFound is M333)"),
    (FJ, '            if line.seq != seq + 1 {',
     _JNV + "; a corrupt journal can at most hide an op (run fresh, a genuine run) or invent one "
     "(a replay, never signed)"),
    (FJ, '                        return Err(JournalError::Corrupt {', _JNV + "; as above"),
    (FJ, '            if matches!(change, Change::Duplicate) {', _JNV + "; as above"),
    (FJ, '                }\n                return Err(JournalError::Conflict {', _JNV),
    (FJ, '            Err(e) => Err(e),\n        }\n    }\n\n    /// Carve', _IO + " (append's own refusals are their own sites)"),
    (FJ, '        ) {\n            return Err(JournalError::BudgetExceeded {', _JNV),
    (FJ, '                Err(JournalError::SettlementConflict { op, reason })', _JNV),
    (FJ, '            Err(e) => Err(e),\n        }\n    }\n\n    /// Attach', _IO + " (passes on the error)"),
    # branches.rs
    (FR, '        Err(e) => Err(e.into()),', _IO + " (create_once's hard_link)"),
    (FR, '        if !store.contains(base) {', _BRN),
    (FR, '        if arms.len() < 2 {', _BRN),
    (FR, '                return Err(BranchError::Invalid(format!("arm {a} declared twice")));', _BRN),
    (FR, '            if approvers.contains(w) {', _BRN),
    (FR, '        if approvers.is_empty() {', _BRN),
    (FR, '            }\n            return Err(BranchError::Exists(format!(', _BRN),
    (FR, '                return Err(BranchError::Exists(format!(', _BRN),
    (FR, '            return Err(BranchError::Unknown(format!(',
     "NOT A VERDICT PROPERTY: experiment(), reached on submit through branch_of_run; the lookup can "
     "only add a refusal, and the experiment/arm it names goes only into the intent (read back "
     "only by replays, never signed)"),
    (FR, '            return Err(BranchError::Cancelled(format!(', _BRN),
    (FR, '        if req.writer != br.writer {', _BRN),
    (FR, '        if req.approver == req.writer || !exp.approvers.contains(&req.approver) {', _BRN),
    (FR, '            return Err(BranchError::StaleEpoch(format!(', _BRN),
    (FR, '        if !store.contains(&req.new_version) {', _BRN),
    (FR, '        if v.intent.scope != self.scope || v.intent.trial_id != br.run_id {', _BRN),
    (FR, '            || rc.verification != ReceiptVerification::Passed', _BRN),
    (FR, '            || rc.output_workspace_ref.as_ref() != Some(&req.new_version)', _BRN),
    (FR, '        if head.seq != req.expected_seq || head.version != req.expected_version {', _BRN),
    (FR, '                return Err(BranchError::Conflict(format!(', _BRN),
    (FR, '            Err(e) => return Err(e.into()),', _BRN),
    (FR, '            journal.fail(', _BRN + " (538 is Journal::fail, not a refusal)"),
    # grants.rs
    (FA, '        if v.get("schema").and_then(|s| s.as_str()) != Some(GRANT_REGISTRY_SCHEMA) {',
     "OPERATOR-AUTHORED: on a protected host the registry is parsed only at its pin (M277, M275, "
     "M276); a development registry is the caller's own"),
    (FA, '                return Err(format!("grant registry names `{grant_ref}` twice"));', "OPERATOR-AUTHORED: as above"),
    (FA, '                return Err(format!("grant `{grant_ref}`: sha256 must be 64 hex"));',
     "OPERATOR-AUTHORED: as above; a non-hex value also never equals the digest computed below"),
    (FA, '        if e.principal_ref != principal_ref {',
     "FLAGGED, NOT AUTHENTICATION: principal_ref is a request field nothing authenticates, so a "
     "caller passes it by naming the bound principal; the limits are the registry pin (M275-M277) "
     "and grant-file ownership (protected_host.rs)"),
    (FA, '        if found != e.sha256 {',
     "OPERATOR-AUTHORED on the protected route: grant files are checked root-owned when the "
     "config loads (protected_host.rs) and the registry is pinned (M277)"),
    (FA, '        if manifest.program != base.join(PLACEHOLDER_PROGRAM) {',
     "UNREACHABLE EFFECT: the parsed program is never read; ResolvedGrant::manifest_for replaces "
     "it with the request's program"),
]

def load_rows():
    spec = importlib.util.spec_from_file_location("mut", os.path.join(ROOT, "scripts/v022_g01_mutations.py"))
    mut = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mut)
    # A retired STALE row names text that is gone by definition (its
    # replacement, an ACTIVE row, mutates the guard's current form).
    stale = set(getattr(mut, "STALE_REFACTORED", {}))
    return [r for r in mut.MUTATIONS if r[0] not in stale]


def code_lines(text):
    """The file's lines up to its unit-test module (tests are not guards)."""
    lines = text.split("\n")
    for i, l in enumerate(lines):
        if l.startswith("#[cfg(test)]") and i + 1 < len(lines) and lines[i + 1].startswith("mod tests"):
            return lines[:i]
    return lines


def line_of(text, offset):
    return text.count("\n", 0, offset)


# Amendment 58 (extended): a refusal is also a TAIL expression (`Err(…)` as
# an arm's or a block's value, no `return`) and a call of a refusal
# constructor (`refused(`, `fail(`, `shape(` in the loop, the runner's
# `refused(`, the verdict's `unknown(`). A definition of one is not a site.
ERR = re.compile(r"\bErr\(")
CTOR = re.compile(r"\b(refused|fail|shape|unknown)\(")
CTOR_DEF = re.compile(r"\bfn\s+(refused|fail|shape|unknown)\b|\blet\s+(refused|fail|shape|unknown)\s*=")


def err_is_expression(l, at):
    """Whether the `Err(` at `at` in line `l` builds a value (a refusal)
    rather than matching one (`Err(e) =>`, `if let Err(e) = …`,
    `matches!(x, Err(_))`, `Ok(_) | Err(_) =>`). A group that does not close
    on its line is an expression (a pattern is never multi-line here)."""
    before = l[:at]
    if re.search(r"\b(let|matches!\()\s*$", before) or re.search(r"\bmatches!\(.*,\s*$", before):
        return False
    depth, j = 0, at + 3
    while j < len(l):
        if l[j] == "(":
            depth += 1
        elif l[j] == ")":
            depth -= 1
            if depth == 0:
                break
        j += 1
    else:
        return True
    rest = l[j + 1:].lstrip()
    if re.match(r"(=>|if\b|\||=[^=])", rest):
        return False
    if before.rstrip().endswith("|"):
        return False
    return True


def is_site(l):
    if SITE.search(l):
        return True
    if any(err_is_expression(l, m.start()) for m in ERR.finditer(l)):
        return True
    return bool(CTOR.search(l)) and not CTOR_DEF.search(l)


def sites(text):
    lines = code_lines(text)
    out = []
    for i, l in enumerate(lines):
        s = l.strip()
        # A `use` declaration names TEST_TRUST_BUILD; it reads nothing.
        if s.startswith("//") or s.startswith("use ") or not is_site(l):
            continue
        g = i
        for j in range(i, max(-1, i - MAX_UP - 1), -1):
            if OPENER.search(lines[j]):
                g = j
                break
        out.append((g, i))
    return out


def main():
    without = set()
    for a in sys.argv[1:]:
        if a.startswith("--without="):
            without = set(a.split("=", 1)[1].split(","))
        else:
            sys.exit(__doc__)
    rows = [r for r in load_rows() if r[0] not in without]
    bad = []
    for f in PROTECTED:
        text = open(os.path.join(ROOT, f)).read()
        spans = []
        for r in rows:
            if r[2] != f:
                continue
            n = text.count(r[3])
            if n != 1:
                bad.append(f"{r[0]}: its old text occurs {n} times in {f} (covers nothing)")
                continue
            a = line_of(text, text.index(r[3]))
            spans.append((r[0], a, a + r[3].count("\n")))
        ex = []
        for ef, anchor, reason in EXEMPT:
            if ef != f:
                continue
            n = text.count(anchor)
            if n != 1:
                bad.append(f"exemption anchor occurs {n} times in {f}: {anchor!r}")
                continue
            ex.append([line_of(text, text.index(anchor)), anchor, reason, 0])
        covered = exempt = 0
        for g, i in sites(text):
            by = [rid for rid, a, b in spans if a <= i and b >= g]
            ex_hit = [e for e in ex if g <= e[0] <= i]
            for e in ex_hit:
                e[3] += 1
            if by:
                covered += 1
                for e in ex_hit:
                    bad.append(f"{f}:{i + 1}: exempt ({e[1]!r}) yet covered by {by}: drop the exemption")
            elif ex_hit:
                exempt += 1
            else:
                lines = text.split("\n")
                bad.append(f"{f}:{i + 1}: refusal site with no row and no exemption: "
                           f"{lines[g].strip()} ... {lines[i].strip()}")
        for line, anchor, _, hits in ex:
            if hits == 0:
                bad.append(f"{f}:{line + 1}: exemption matches no refusal site: {anchor!r}")
        print(f"{f}: {covered} covered by a row, {exempt} exempt")
    for f, n in NOT_YET_SCANNED.items():
        print(f"{f}: NOT YET SCANNED ({n} refusal sites had neither a row nor an exemption "
              f"when measured; amendment 58)")
    for b in bad:
        print(f"BAD {b}")
    if bad:
        sys.exit(1)
    print("refusal coverage: every refusal site in the protected files has a row or a reasoned exemption")


if __name__ == "__main__":
    main()
