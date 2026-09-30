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
`TEST_TRUST_BUILD`). Its guard block runs from the nearest opening
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
]
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
    (PL, "    if r.policy_json.len() > MAX_POLICY {",
     "bound on bytes copied to the staging dir; the whole request is already bounded to "
     "MAX_REQUEST (256 KiB) at the read, so this only tightens a bound that exists"),
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


def load_rows():
    spec = importlib.util.spec_from_file_location("mut", os.path.join(ROOT, "scripts/v022_g01_mutations.py"))
    mut = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mut)
    return mut.MUTATIONS


def code_lines(text):
    """The file's lines up to its unit-test module (tests are not guards)."""
    lines = text.split("\n")
    for i, l in enumerate(lines):
        if l.startswith("#[cfg(test)]") and i + 1 < len(lines) and lines[i + 1].startswith("mod tests"):
            return lines[:i]
    return lines


def line_of(text, offset):
    return text.count("\n", 0, offset)


def sites(text):
    lines = code_lines(text)
    out = []
    for i, l in enumerate(lines):
        s = l.strip()
        if s.startswith("//") or not SITE.search(l):
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
    for b in bad:
        print(f"BAD {b}")
    if bad:
        sys.exit(1)
    print("refusal coverage: every refusal site in the protected files has a row or a reasoned exemption")


if __name__ == "__main__":
    main()
