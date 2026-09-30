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
NOT_YET_SCANNED = {
    "crates/axon-fabric/src/psv.rs": 2,
    "crates/axon-psv/src/runner.rs": 4,
    "crates/axon-loop-contracts/src/protected_evidence.rs": 9,
    "crates/axon-loop/src/admission.rs": 18,
    "crates/axon-loop/src/intake.rs": 27,
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
     "dominated: every walked path is the constant CONFIG_PATH's parent or a config path "
     "load_config already required to be absolute and plain (RootDir/Normal components only); "
     "--test-config exists in test-trust builds alone (M601)"),
    (PL, '        return Err(bad("not a regular file".into()));',
     "the config's directory chain and file are operator-owned (M585-M589): only the operator "
     "can put a non-regular file at the config path"),
    (PL, '        return Err(bad(format!("schema is not {CONFIG_SCHEMA}")));',
     "operator-authored field of an operator-owned file (M585/M586): a version tag"),
    (PL, "        if !is_hex64(&p.sha256) {",
     "operator-authored; and dominated: a pin that is not lowercase hex can never equal "
     "sha256_hex of the bytes, so open_verified's comparison (M528) refuses it"),
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
    (PL, "    if r.schema != REQUEST_SCHEMA {",
     "a version tag of a fixed-shape request whose every field is validated on its own "
     "(M532/M534/M535/M600) and unknown fields refused (M535)"),
    (PL, "    if !is_hex64(&r.psv_manifest_sha256) {",
     "dominated: the launcher (fc_linux_profile.sh) refuses unless sha256sum of the job's "
     "launch manifest EQUALS this word, which a non-hex value never does"),
    (PL, "    if r.timeout_s == 0 || r.timeout_s > c.max_timeout_s {",
     "UNROWED, candidate: bounds the wall time of a root launch; the launcher applies its "
     "own ceiling (--timeout-s), not verified here. Left for a row with a launcher-side check"),
    (PL, "    if bytes.len() > MAX_POLICY {",
     "bound on the policy bytes copied to the snapshot: the read before it is already bounded "
     "to MAX_POLICY + 1 bytes (`take`), so this refuses one byte more than a bound that exists; "
     "and the bytes are then held to the manifest's policy_sha256 (PSV-6)"),
    (PL, "        if p.file_name() != Some(OsStr::new(leaf)) {",
     "UNROWED, candidate: the leaf name of each psv input; the snapshot opens the FIXED "
     "names candidate/check/job under the inputs dir whatever the request spells, so a "
     "different leaf changes nothing that is read (validate_request test covers it)"),
    (PL, "        if parent.join(leaf).as_os_str() != p.as_os_str() {",
     "as above: the snapshot opens only the fixed leaves of the one inputs dir"),
    (PL, "    if inputs_name == out_name {",
     "dominated: make_out's mkdirat requires the out dir to be NEW, and the inputs dir "
     "exists, so the same name refuses there"),
    (PL, '        if st.st_uid != c.fabric_uid {\n            return Err(format!(\n                "psv input {leaf} is owned',
     "dominated by the kernel: a root-owned leaf inside the Fabric's inputs dir can only be "
     "moved there by renaming a root-owned directory to a new parent, which needs write "
     "permission on that directory; the helper creates none there (M599 guards the "
     "inputs dir itself, M538 every file)"),
    (PL, '            _ => {\n                return Err(format!(\n                    "{} is not a regular file or directory",',
     "fail-closed on an entry the snapshot cannot represent; skipping it (the only "
     "alternative) cannot add bytes to the guest image; symlinks are refused by O_NOFOLLOW"),
    (PL, "    if unsafe { libc::mkdirat(root, cn.as_ptr(), 0o700) } != 0 {",
     "UNROWED pair with the next: a pre-existing out dir is refused by BOTH this and the "
     "owner re-check; the owner re-check alone guards a swap race between mkdirat and "
     "openat, which no deterministic test can interpose"),
    (PL, '        return Err("the out dir was replaced between its creation and its open".into());',
     "see the mkdirat exemption"),
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
     "the verdict function's arms; every caller is rowed where it is the only check: the "
     "post-hash check (M592), the child's re-check before execveat (M527); the parent's "
     "re-check in command() is dominated by the child's"),
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
    (CU, "    if m.file_type().is_symlink() || !m.is_dir() {",
     "a symlink: lstat reports mode 0777 for every symlink on Linux, so the next check (no "
     "group/other access, M325) refuses it; a non-directory owned by the custodian with mode 0600 "
     "fails every issue and spend (creating or reading <store>/<nonce>.issued is ENOTDIR): fails "
     "closed"),
    (CU, '        return Err(format!("SO_PEERCRED: {}", std::io::Error::last_os_error()));',
     "OS error from getsockopt: the connection is answered with a refusal (server) or refused "
     "(client); no peer is assumed"),
    (CU, "        if r.schema != REPLY_SCHEMA || Mode::parse(&r.mode).is_none() {",
     "the reply comes from the authenticated custodian (peer uid, M626); `schema` is a version "
     "tag, and a mode that does not parse is read as Dev, which launches nothing protected "
     "(custodian_mode_launches accepts Protected, or Test in a test authority)"),
    (CU, "        if r.schema != REQUEST_SCHEMA {",
     "a version tag of a fixed-shape request (deny_unknown_fields) whose every field is judged on "
     "its own: the op by name, the peer per op (M627, M628), the nonce by the store (M196), the "
     "manifest as 64 hex"),
    (CU, '            other => Err(format!("unknown op {other:?}")),',
     "an unknown op changes no state and returns no nonce: admitted, it could only answer ok with "
     "nothing, which neither client reads as a nonce (issue requires one) or sends (the helper "
     "sends `spend` only)"),
    (CU, '        return Err("fd 3 is not the activated socket".into());',
     "fails closed: UnixListener::local_addr on a descriptor that is not a socket fails "
     "(ENOTSOCK, 'activated socket:'), and a socket at another path is refused by M703"),
    (PH, '        return Err(bad(format!("{ptr} is not absolute")));',
     "pinned_paths authorizes nothing: it prints the preflight's probe list; load() refuses the "
     "same relative path (M145) before anything runs"),
    (PH, '    if v["schema"] != PROTECTED_HOST_SCHEMA {\n        return Err(bad(format!("schema is not {PROTECTED_HOST_SCHEMA}")));\n    }\n    let path_at',
     "pinned_paths authorizes nothing (the probe list); load() refuses the same config (M764)"),
    (RD, "        Err(e) => return Err(format!(\"{}: {e}\", path.display())),\n        Ok(_) => {}",
     "OS error from lstat other than NotFound: the path is then walked by check_owned_from_pub and "
     "read by read_regular, which fail on it; never read as the default"),
    (RD, "                        if writable_by_me(&q) {",
     "rowed where the same removal is made: M490 (the enclosing require_unwritable condition) and "
     "M491 (writable_by_me's predicate), both killed on the production decision"),
    (RD, "    if found != top {",
     "dominated: `found` differs from `top` only when `top` holds no .git (discover returns the "
     "first directory upward that does), and the next statement, refuse_config(&top), refuses "
     "a top with no .git directory ('is not a git directory')"),
    (RD, '        return Err(format!("{component}: cannot list refs/replace/"));',
     "serves the refs/replace/ refusal M285, retired EQUIVALENT: git_cmd turns replacement "
     "objects off (--no-replace-objects and GIT_NO_REPLACE_OBJECTS, the M385 pair, itself retired "
     "with a four-cell record), so a replace ref never changes an answer"),
    (RD, "    if !ok || grafts.is_empty() {",
     "fails closed: without it an empty answer names `repo` itself as the grafts file, which "
     "exists, and the graft refusal fires (the graft refusal M286 is itself retired EQUIVALENT)"),
    (RD, '        return Err(format!("{component}: cannot read the index"));',
     "serves the skip-worktree / assume-unchanged refusals M287/M288, retired EQUIVALENT: the "
     "working tree is compared by its bytes (tree_differs, M290), never through the index"),
    (RD, "    if !rec.exists() {",
     "fails closed: read_once, the next read of the same path, fails on a missing record"),
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
    (RD, "    if !ok {\n        return Err(format!(\n            \"{component}: cannot compare this tree",
     "git's answers only ADD to the change set: whenever they add nothing (a failed git included), "
     "the comparison from hash-checked objects runs (the certified and HEAD trees' entries, and "
     "tree_differs over the filesystem, M290) and M697 refuses any difference"),
    (RD, "    if !ok_staged || !ok_untracked {",
     "as the `git diff` failure above: an empty answer leaves the hash-checked comparison (M290, "
     "M697) to decide"),
    (RD, '            return Err(format!("{component}: this tree has no HEAD commit"));',
     "fails closed: an empty HEAD name makes Objects::entries fail"),
    (RD, "        if !trust.key_ids(dir)?.iter().any(|k| k == s(field)) {",
     "per field: the observer_key_id entry is M418's (retired EQUIVALENT, four-cell record: the "
     "observation's signer must verify under the observer root, M340, and be that key, M339); "
     "the verifier_key_id entry is M338's, which since amendment 57 is REFUSED_ELSEWHERE "
     "(launched() looks verifier_key_id up in the verifier root and verifies the receipt "
     "attestation under it, M740): M338 needs a four-cell retirement (open item, amendment 58)"),
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
    (OB, '        return Err(format!("observer exited {:?}", status.code()));',
     "the exit status adds no authority: whatever the observer left is read once and its "
     "signature verified under the observer root (verify_observation), then joined to the "
     "manifest; an observer that failed leaves nothing that verifies"),
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
