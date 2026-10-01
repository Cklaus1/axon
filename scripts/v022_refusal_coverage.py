#!/usr/bin/env python3
"""Refusal-site coverage drift check for the protected helper files
(C9 round 3, harness workstream; amendment 48).

The mutation registry (scripts/v022_g01_mutations.py) is a LIST of guards, and
a guard nobody listed is untested evidence: the round-3 EQUIVALENCE review
found ten guards in the decision-A/D files with no row, and the whole
axon-fabric suite green with each of them removed. This check derives the
guards from the CODE instead of trusting the list.

A refusal site is a non-test line in an in-scope file (see SCOPE_DIRS) matching SITE
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

    python3 scripts/v022_refusal_coverage.py [--without=M1,M2] [--freeze]

--without drops rows before checking: the check must then name their sites
(a gate that cannot speak proves nothing). --freeze also fails while any
in-scope file is NOT_YET_SCANNED (amendment 61); v022_freeze_manifest.py runs
the check that way and refuses to bind a freeze otherwise.

The FILE SET is a rule (amendment 61), not a list: see SCOPE_DIRS below.
"""
import importlib.util
import os
import re
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# Amendment 61 (C9 round 4b, rows4a): the file set is a RULE, not a list.
# Round 4b (EQUIVALENCE) found whole protected decision files unscanned while
# a hand-kept PROTECTED list said otherwise and NOT_YET_SCANNED was {}: the
# list stood in for the set (class a). The scope is now derived from the tree:
#
#   IN SCOPE = every non-test .rs file under SCOPE_DIRS (recursively: the
#   protected crates' whole sources, bins included), every file in
#   SCOPE_FILES, and, in each SCOPE_FN_REGIONS file, the non-test functions
#   whose name matches its pattern (the interpreter's seal edges);
#
# and each in-scope file is exactly one of: SCANNED (every refusal site has a
# row or a reasoned exemption, BAD otherwise), OUT_OF_SCOPE (named with a
# reason a reviewer can check), or NOT_YET_SCANNED (named with the number of
# its sites that have neither a row nor an exemption, which the gate
# re-measures and refuses if it differs). An in-scope file in none of these is
# SCANNED, so a new file is checked the day it appears. NOT_YET_SCANNED is
# shown on every run; under --freeze (and in v022_freeze_manifest.py, which
# calls check(freeze=True)) a non-empty NOT_YET_SCANNED fails: no freeze binds
# evidence over a decision file nobody scanned.
SCOPE_DIRS = (
    "crates/axon-fabric/src",
    "crates/axon-loop/src",
    "crates/axon-loop-contracts/src",
    "crates/axon-psv/src",
)
SCOPE_FILES = (
    "crates/axon-core/src/interp/conform.rs",
)
# The interpreter's seal edges: the functions that decide what a sealed
# (candidate) frame may reach. A site outside such a function in these files
# is not in scope (the rest of the interpreter is the language, not a
# protected decision).
SEAL_FN = r"seal|conform|cast"
SCOPE_FN_REGIONS = {
    "crates/axon-core/src/interp.rs": SEAL_FN,
    "crates/axon-core/src/interp/eval.rs": SEAL_FN,
}
# (file -> reason). A file here is in scope by the rule and judged not to be a
# decision path; the reason names what makes that checkable.
OUT_OF_SCOPE = {
}
# (file -> sites with neither a row nor an exemption, as last measured). The
# gate re-measures each count and refuses a stale one, in both directions.
NOT_YET_SCANNED = {
}
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


# C9 round 4b fix wave, rows4a (amendment 61): the LOOP side's files the rule
# brings in. Kinds, as before: NOTHING TO ADMIT (the refusal holds an error and
# no value the code after it could run on), SELECTS NOTHING (the refused field
# decides nothing the code reads: named), NAMED ROW (a registry row mutates the
# same removal at another line), OPERATOR-AUTHORED (a field of an operator-owned
# file or of the operator's environment: only the operator chooses it, and the
# refusal only fails closed), OS ERROR, NON-UNIX (compiled out), UNREACHABLE
# (no input reaches it; the fact is named).
AT = "crates/axon-loop-contracts/src/attestation.rs"
OT = "crates/axon-loop-contracts/src/operator_trust.rs"
EXEMPT += [
    (AT, '            shape(format!(\n                "the key registered for verifier {issuer_ref} is not a 64-hex Ed25519 public key"',
     "NOTHING TO ADMIT: the registered key is not a key, so there is nothing to verify under; it "
     "comes from Config::rooted_key (an operator-rooted key, M207/M206)"),
    (AT, '.ok_or_else(|| shape("attestation: not a JSON object"))?;',
     "NOTHING TO ADMIT: a non-object carries no binding or signature to verify"),
    (AT, '.ok_or_else(|| shape("attestation: no issued_ms (the issuer\'s signing time)"))?;',
     "NOTHING TO ADMIT: no signing time to bind (the binding needs one) or to compare with the "
     "plan's freeze; any value substituted is then compared with the document's (M03) and signed "
     "over (M02)"),
    (AT, 'return Err(shape(format!("attestation: unknown field {k:?}")));',
     "SELECTS NOTHING: an unbound field is neither signed nor read; every consumer reads only bound "
     "fields (compared by M03, signed by M02) and issued_ms through issued_ms(), which the binding "
     "signs"),
    (AT, '        return Err(shape(format!(\n            "attestation: not {ATTESTATION_SCHEMA} with alg ed25519"',
     "SELECTS NOTHING: `schema` is a bound field (M03 compares it with ATTESTATION_SCHEMA, M02 "
     "signs it) and `alg` chooses nothing: verification is always Ed25519"),
    (AT, '.ok_or_else(|| shape("attestation: no 32-byte public_key"))?;',
     "SELECTS NOTHING: the presented key is only compared; the signature is always verified under "
     "the REGISTERED key (M02)"),
    (AT, '        return Err(shape(format!(\n            "attestation is signed by {}, not by {key_id}',
     "SELECTS NOTHING: the presented key is never used to verify; the signature is verified under "
     "the registered key (M02), so a document presenting another key is accepted only if the "
     "registered key signed it"),
    (AT, '.ok_or_else(|| shape("attestation: no 64-byte signature"))?;',
     "NOTHING TO ADMIT: no signature to verify"),
    (AT, '            shape(format!(\n                "attestation signature does not verify under {key_id}',
     "NAMED ROW: M02 mutates the verification this refusal reports (the same removal)"),
    (AT, '            shape(format!(\n                "the key registered for {issuer_ref} is not a 64-hex',
     "NOTHING TO ADMIT: the registered key is not a key; every caller passes an operator-rooted "
     "key (Config::rooted_key, M207/M257)"),
    (AT, '.ok_or_else(|| shape("signature: not a JSON object"))?;',
     "NOTHING TO ADMIT: a non-object carries no binding or signature"),
    (AT, 'return Err(shape(format!("signature: unknown field {k:?}")));',
     "SELECTS NOTHING: an unbound field is neither signed nor read; callers take the domain, the "
     "issuer and the document from their own inputs (document_binding), never from the signature"),
    (AT, 'return Err(shape("signature: alg is not ed25519"));',
     "SELECTS NOTHING: verification is always Ed25519 under the registered key"),
    (AT, '.ok_or_else(|| shape("signature: no 32-byte public_key"))?;',
     "SELECTS NOTHING: the presented key is only compared, never used to verify"),
    (AT, '        return Err(shape(format!(\n            "signed by {}, not by {key_id}',
     "SELECTS NOTHING: the signature is verified under the registered key (M951), never the "
     "presented one"),
    (AT, '            return Err(shape(format!(\n                "signature: {field} is {} but',
     "SELECTS NOTHING: the signature is verified over the binding built from the CALLER's domain, "
     "issuer, key id and document (M951), and no caller reads a field of the signature document, "
     "so a displayed field that differs changes nothing that is read"),
    (AT, '.ok_or_else(|| shape("signature: no 64-byte signature"))?;',
     "NOTHING TO ADMIT: no signature to verify"),
    (OT, "    if !dir.is_absolute() {",
     "re-reported by the next statement: a relative `dir` is not below the absolute `base` every "
     "caller passes (\"/\" or an absolute test root), so strip_prefix refuses it"),
    (OT, '        Err(e) => {\n            return Err(format!(\n                "trust root {} cannot be read',
     "NAMED ROW: M460 mutates the arm above it to read every listing failure as NotFound, the same "
     "removal"),
    (OT, "            if t.len() != 64 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {",
     "OPERATOR-AUTHORED: a key file in a root the ownership walk holds root-owned and unwritable by "
     "others (M948-M950); a malformed entry, kept, equals no presented key (keys are compared as "
     "64-hex strings or 32-byte values), so it only fails closed"),
    (OT, 'return Err("operator ownership cannot be checked on this platform".into());',
     "NON-UNIX: compiled out on the only supported platform (cfg(not(unix)))"),
    (OT, '    if sv["schema"] != EVIDENCE_SIGNATURE_SCHEMA || sv["alg"] != "ed25519" {',
     "SELECTS NOTHING: the signed message always begins with EVIDENCE_SIGNATURE_SCHEMA "
     "(evidence_signing_message) and verification is always Ed25519, so neither field chooses what "
     "is verified"),
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
# Amendment 61: and the interpreter's `panic(` (a Flow refusal: the seal
# edges and conform.rs refuse that way; `panic!(` is not it).
ERR = re.compile(r"\bErr\(")
CTOR = re.compile(r"\b(refused|fail|shape|unknown|panic)\(")
CTOR_DEF = re.compile(r"\bfn\s+(refused|fail|shape|unknown|panic)\b|\blet\s+(refused|fail|shape|unknown|panic)\s*=")


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


def fn_regions(lines, pattern):
    """(first, last) line spans of the functions in `lines` whose name matches
    `pattern`. A function ends at the first later line that is its own
    indentation followed by `}` (rustfmt's layout, which cargo fmt --check
    holds every file to)."""
    out = []
    head = re.compile(r"^(\s*)(pub(\([a-z]+\))?\s+)?fn\s+(\w+)")
    for i, l in enumerate(lines):
        m = head.match(l)
        if not m or not re.search(pattern, m.group(4)):
            continue
        close = m.group(1) + "}"
        for j in range(i + 1, len(lines)):
            if lines[j] == close:
                out.append((i, j))
                break
    return out


def sites(text, pattern=None):
    lines = code_lines(text)
    regions = fn_regions(lines, pattern) if pattern else None
    out = []
    for i, l in enumerate(lines):
        s = l.strip()
        # A `use` declaration names TEST_TRUST_BUILD; it reads nothing.
        if s.startswith("//") or s.startswith("use ") or not is_site(l):
            continue
        if regions is not None and not any(a <= i <= b for a, b in regions):
            continue
        g = i
        for j in range(i, max(-1, i - MAX_UP - 1), -1):
            if OPENER.search(lines[j]):
                g = j
                break
        out.append((g, i))
    return out


def in_scope_files():
    """The rule's file set (amendment 61), sorted: every .rs under SCOPE_DIRS,
    SCOPE_FILES, and the SCOPE_FN_REGIONS files."""
    found = set(SCOPE_FILES) | set(SCOPE_FN_REGIONS)
    for d in SCOPE_DIRS:
        for dirpath, _, names in os.walk(os.path.join(ROOT, d)):
            for n in names:
                if n.endswith(".rs"):
                    found.add(os.path.relpath(os.path.join(dirpath, n), ROOT))
    return sorted(found)


def judge_file(f, rows, bad):
    """Judge one in-scope file: (covered, exempt, uncovered sites). Problems
    with the registry or the exemptions go to `bad` whatever the file's state;
    uncovered sites are returned, and reported by the caller."""
    text = open(os.path.join(ROOT, f)).read()
    lines = text.split("\n")
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
    uncovered = []
    for g, i in sites(text, SCOPE_FN_REGIONS.get(f)):
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
            uncovered.append(f"{f}:{i + 1}: refusal site with no row and no exemption: "
                             f"{lines[g].strip()} ... {lines[i].strip()}")
    for line, anchor, _, hits in ex:
        if hits == 0:
            bad.append(f"{f}:{line + 1}: exemption matches no refusal site: {anchor!r}")
    return covered, exempt, uncovered


def check(without=(), freeze=False, out=print):
    """Run the gate. Returns the list of problems (empty: it holds). With
    `freeze`, a non-empty NOT_YET_SCANNED is itself a problem."""
    rows = [r for r in load_rows() if r[0] not in set(without)]
    bad = []
    scope = in_scope_files()
    for f in sorted(set(OUT_OF_SCOPE) | set(NOT_YET_SCANNED)):
        if f not in scope:
            bad.append(f"{f}: named in OUT_OF_SCOPE/NOT_YET_SCANNED but not in scope by the rule "
                       "(or gone): the table must not outlive its file")
        if f in OUT_OF_SCOPE and f in NOT_YET_SCANNED:
            bad.append(f"{f}: both OUT_OF_SCOPE and NOT_YET_SCANNED")
    for f in scope:
        if f in OUT_OF_SCOPE:
            out(f"{f}: OUT OF SCOPE ({OUT_OF_SCOPE[f]})")
            continue
        covered, exempt, uncovered = judge_file(f, rows, bad)
        if f in NOT_YET_SCANNED:
            listed = NOT_YET_SCANNED[f]
            out(f"{f}: NOT YET SCANNED: {len(uncovered)} of {covered + exempt + len(uncovered)} "
                f"refusal sites have neither a row nor an exemption")
            if len(uncovered) != listed:
                bad.append(f"{f}: NOT_YET_SCANNED says {listed} uncovered sites, measured "
                           f"{len(uncovered)}: update the count (or, at 0, scan the file)")
            if freeze:
                bad.append(f"{f}: NOT YET SCANNED at a freeze ({len(uncovered)} uncovered sites)")
            continue
        out(f"{f}: {covered} covered by a row, {exempt} exempt")
        bad.extend(uncovered)
    for b in bad:
        out(f"BAD {b}")
    return bad


def main():
    without = set()
    freeze = False
    for a in sys.argv[1:]:
        if a.startswith("--without="):
            without = set(a.split("=", 1)[1].split(","))
        elif a == "--freeze":
            freeze = True
        else:
            sys.exit(__doc__)
    if check(without, freeze):
        sys.exit(1)
    if NOT_YET_SCANNED:
        print(f"refusal coverage: every refusal site in the scanned files has a row or a reasoned "
              f"exemption; {len(NOT_YET_SCANNED)} in-scope file(s) NOT YET SCANNED (a freeze "
              f"refuses until none is)")
    else:
        print("refusal coverage: every refusal site in every in-scope file has a row or a reasoned "
              "exemption")


if __name__ == "__main__":
    main()
