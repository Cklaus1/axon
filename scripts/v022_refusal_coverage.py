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

Amendment 76: a refusal that is a RETURNED VERDICT is a site too (a built
negative variant of a verdict enum, a function returning a verdict, a call of a
helper constructor, a closure predicate refused through `ok_or`); the verdict
types are derived from the in-scope enums. See "Amendment 76" below.
"""
import collections
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
# Amendment 71 (C9 round 4c, r4c-fixes part 2): the CRATE set is a rule too.
# Round 4c (SENTINEL) found decision code on the protected path outside the
# four hand-named SCOPE_DIRS: the guest's PID-1 supervisor (axon-guest-init),
# the workspace recipe every snapshot is judged by (axon-workspace-recipe),
# the `axon test` entry that issues the completion evidence (PSV-3) and the
# resolver's sealed-module refusal (E0004). A directory list stood in for "the
# protected path" exactly as the file list had (class a). The rule:
#
#   PROTECTED CRATES = ROOT_CRATES (the host side: Fabric, the PSV crate, the
#   loop and its contracts) + every package GUEST_BUILD builds (`-p NAME`:
#   what runs inside the protected guest) + the transitive closure of their
#   NORMAL workspace dependencies ([dependencies] and
#   [target.*.dependencies] entries with a `path`, read from each crate's
#   Cargo.toml; dev- and build-dependencies are not linked into the shipped
#   binaries). Every non-test .rs under a protected crate's src/ is in scope,
#   EXCEPT LANGUAGE_CRATE (the interpreter: the language, not a decision),
#   whose scope is by function region (below).
#
# A new dependency of a protected crate, or a new package the guest image
# builds, is in scope the day it appears.
ROOT_CRATES = ("axon-fabric", "axon-loop", "axon-loop-contracts", "axon-psv")
GUEST_BUILD = "scripts/build-guest-image.sh"
LANGUAGE_CRATE = "axon-core"
SCOPE_FILES = (
    "crates/axon-core/src/interp/conform.rs",
)
# The interpreter's seal edges: the functions that decide what a sealed
# (candidate) frame may reach. A site outside such a function in these files
# is not in scope (the rest of the interpreter is the language, not a
# protected decision). Amendment 71: selected in EVERY LANGUAGE_CRATE source
# compiled into the guest's `--no-default-features` build (a module lib.rs
# gates behind a feature is not: codegen, smt, lsp), so the resolver's
# sealed-module refusal (check_sealed) is in scope by the same rule as the
# interpreter's. `unseal` is the TEE enclave builtin (checker.rs), not
# Protected Check Isolation.
SEAL_FN = r"(?<!un)seal|conform|cast"
# Amendment 71: the `axon test` verb inside the guest is the PSV-3 entry: it
# reads the completion key, makes the process non-dumpable, drops the Exec
# effect, excludes a sealed module's tests and issues the completion and
# failure tokens. Its function and the main.rs functions it uses for that
# evidence are in scope.
CORE_ENTRY = "crates/axon-core/src/main.rs"
CORE_ENTRY_FN = r"^(cmd_test|read_completion_key|completion_token|failure_token)$"


def _crate_dirs():
    """package name -> crate directory (relative), for every crates/*/Cargo.toml."""
    import tomllib
    out = {}
    base = os.path.join(ROOT, "crates")
    for d in sorted(os.listdir(base)) if os.path.isdir(base) else []:
        t = os.path.join(base, d, "Cargo.toml")
        if os.path.isfile(t):
            with open(t, "rb") as f:
                name = tomllib.load(f).get("package", {}).get("name")
            if name:
                out[name] = f"crates/{d}"
    return out


def _normal_path_deps(crate_dir, dirs, default_features=True):
    """The workspace crates `crate_dir` links: [dependencies] and
    [target.*.dependencies] entries that are a `path` (dev/build excluded).
    An OPTIONAL one counts only when a feature the build enables turns it on:
    the crate's `default` features (expanded through [features]) when
    `default_features`, none otherwise (a `--no-default-features` build)."""
    import tomllib
    with open(os.path.join(ROOT, crate_dir, "Cargo.toml"), "rb") as f:
        t = tomllib.load(f)
    feats = t.get("features", {})
    on, todo = set(), list(feats.get("default", [])) if default_features else []
    while todo:
        x = todo.pop()
        if x in on:
            continue
        on.add(x)
        todo.extend(feats.get(x, []))
    enabled = {x.removeprefix("dep:").split("/")[0].rstrip("?") for x in on}
    tables = [t.get("dependencies", {})]
    tables += [v.get("dependencies", {}) for v in t.get("target", {}).values()]
    out = set()
    for tab in tables:
        for key, spec in tab.items():
            if not (isinstance(spec, dict) and "path" in spec):
                continue
            if spec.get("optional") and key not in enabled:
                continue
            out.add(spec.get("package", key))
    return out


def guest_packages():
    """Every package GUEST_BUILD builds (`-p NAME`)."""
    p = os.path.join(ROOT, GUEST_BUILD)
    if not os.path.isfile(p):
        return set()
    return set(re.findall(r"\s-p\s+([A-Za-z0-9_-]+)", open(p).read()))


def _closure(start, dirs, stop, default_features, bad):
    seen = {}
    todo = list(start)
    while todo:
        n = todo.pop()
        if n in seen or n in stop:
            continue
        if n not in dirs:
            if bad is not None:
                bad.append(f"{n}: a protected crate (by the rule) with no crates/*/Cargo.toml")
            continue
        seen[n] = dirs[n]
        todo.extend(_normal_path_deps(dirs[n], dirs, default_features))
    return seen


def protected_crates(bad=None):
    """The rule's WHOLE-CRATE set (name -> dir): ROOT_CRATES and the guest's
    packages but LANGUAGE_CRATE, closed over their normal dependencies (default
    features), never through LANGUAGE_CRATE. A crate the rule names but the
    tree does not have is BAD (it would scan nothing)."""
    dirs = _crate_dirs()
    start = list(ROOT_CRATES) + sorted(guest_packages() - {LANGUAGE_CRATE})
    return dict(sorted(_closure(start, dirs, {LANGUAGE_CRATE}, True, bad).items()))


def language_crates(bad=None):
    """LANGUAGE_CRATE and what it links in the guest's `--no-default-features`
    build that is not already a whole protected crate: the language and its
    libraries, scoped by function region."""
    dirs = _crate_dirs()
    whole = protected_crates()
    if LANGUAGE_CRATE not in guest_packages():
        return {}
    lang = _closure([LANGUAGE_CRATE], dirs, set(whole), False, bad)
    return dict(sorted(lang.items()))


def _enabled_features(crate_dir, default_features):
    """The features a build of `crate_dir` turns on: its `default` set expanded
    through [features] when `default_features`, none otherwise."""
    import tomllib
    with open(os.path.join(ROOT, crate_dir, "Cargo.toml"), "rb") as f:
        feats = tomllib.load(f).get("features", {})
    on, todo = set(), list(feats.get("default", [])) if default_features else []
    while todo:
        x = todo.pop()
        if x not in on:
            on.add(x)
            todo.extend(feats.get(x, []))
    return on


def _feature_gated_modules(crate_dir, default_features=False):
    """Modules lib.rs declares under `#[cfg(feature = F)]` for an F the build
    does not enable: not compiled, so not scanned."""
    lib = os.path.join(ROOT, crate_dir, "src/lib.rs")
    if not os.path.isfile(lib):
        return set()
    on = _enabled_features(crate_dir, default_features)
    found = re.findall(r'#\[cfg\(feature\s*=\s*"([^"]+)"\)\]\s*\n\s*(?:pub(?:\([a-z]+\))?\s+)?mod\s+(\w+)\s*;',
                       open(lib).read())
    return {m for f, m in found if f not in on}


def _feature_gated_files(crate_dir, default_features):
    out = set()
    for m in _feature_gated_modules(crate_dir, default_features):
        out.add(f"{crate_dir}/src/{m}.rs")
        out.update(_rs_under(f"{crate_dir}/src/{m}"))
    return out


def _rs_under(d):
    out = []
    for dirpath, _, names in os.walk(os.path.join(ROOT, d)):
        for n in names:
            if n.endswith(".rs"):
                out.append(os.path.relpath(os.path.join(dirpath, n), ROOT))
    return out


def scope_dirs():
    """src/ of every whole protected crate."""
    return tuple(f"{d}/src" for d in protected_crates().values())


def _bin_sources(crate_dir):
    """A crate's binary targets: src/main.rs, src/bin/**, and every [[bin]]
    path (relative to ROOT)."""
    import tomllib
    with open(os.path.join(ROOT, crate_dir, "Cargo.toml"), "rb") as f:
        t = tomllib.load(f)
    out = {f"{crate_dir}/src/main.rs"} | set(_rs_under(f"{crate_dir}/src/bin"))
    for b in t.get("bin", []):
        if "path" in b:
            out.add(os.path.normpath(f"{crate_dir}/{b['path']}"))
    lib = t.get("lib", {}).get("path")
    if lib:
        out.discard(os.path.normpath(f"{crate_dir}/{lib}"))
    # A module a binary root declares (`mod X;` in src/main.rs) that the
    # library does not is the binary's own (axon-vm's chain, quorum).
    decl = re.compile(r"^\s*(?:pub(?:\([a-z]+\))?\s+)?mod\s+(\w+)\s*;", re.M)
    librs = os.path.join(ROOT, crate_dir, "src/lib.rs")
    in_lib = set(decl.findall(open(librs).read())) if os.path.isfile(librs) else set()
    mainrs = os.path.join(ROOT, crate_dir, "src/main.rs")
    if os.path.isfile(mainrs):
        for m in set(decl.findall(open(mainrs).read())) - in_lib:
            out.add(f"{crate_dir}/src/{m}.rs")
            out.update(_rs_under(f"{crate_dir}/src/{m}"))
    return out


def linked_only_as_library():
    """Binary sources of the crates in the rule only as a DEPENDENCY (not a
    root, not a package the guest builds): a dependency is linked as its
    library; its own binaries are not part of any protected binary."""
    shipped = set(ROOT_CRATES) | guest_packages()
    out = set()
    for n, d in protected_crates().items():
        if n not in shipped:
            out |= _bin_sources(d)
        # A module behind a feature the (default-feature) build leaves off
        # is not compiled into any protected binary.
        out |= _feature_gated_files(d, True)
    return out


def language_regions():
    """file -> fn-name pattern, over the language crates' guest-build sources
    (LANGUAGE_CRATE's own bins other than main.rs are not in the guest)."""
    out = {}
    for n, d in language_crates().items():
        gated = _feature_gated_modules(d)
        for f in _rs_under(f"{d}/src"):
            rel = os.path.relpath(f, f"{d}/src")
            top = rel.split(os.sep)[0].removesuffix(".rs")
            if top in gated or rel.startswith("bin" + os.sep):
                continue
            out[f] = SEAL_FN
    if CORE_ENTRY in out:
        out[CORE_ENTRY] = f"{SEAL_FN}|{CORE_ENTRY_FN}"
    return out


SCOPE_DIRS = scope_dirs()
SCOPE_FN_REGIONS = language_regions()
# Amendment 60 (core2) + 64 (integration): the seal edges are ALSO selected by
# an anchored span (the first anchor's line to the second's, each once in the
# file): the region core2 rowed, which holds the sealed-provenance helpers
# whose names need not match SEAL_FN. A file's scanned lines are the UNION of
# its SCOPE_FN_REGIONS functions and its REGIONS span, so neither selection can
# drop a site the other names; an anchor that is missing or not unique is BAD.
REGIONS = {"crates/axon-core/src/interp.rs": (
    "    /// Whether `f` was defined in a sealed (candidate) module.",
    "    /// Run `g` with the frame's provenance set to `sealed`, restoring it after.")}
# (file -> reason). A file here is in scope by the rule and judged not to be a
# decision path; the reason names what makes that checkable.
OUT_OF_SCOPE = {
    "crates/axon-loop-contracts/src/profile.rs":
        "bridge-profile negotiation (B256): which wire versions MiCode and Axon speak; it authorizes "
        "nothing: every document is still parsed by its own contract and judged by the scanned "
        "decision code",
}
# (file -> sites with neither a row nor an exemption, as last measured). The
# gate re-measures each count and refuses a stale one, in both directions.
# Amendment 76: EMPTY. Every site amendments 71-76 exposed has a row or an exemption
# stating a checkable fact; a new unscanned file is listed here again, never exempted in bulk.
NOT_YET_SCANNED = {
}
SITE = re.compile(r"return Err\(|\bErr\(format!|\brefuse\(|\bErr\(bad\(|TEST_TRUST_BUILD")
# Amendment 74: a `let .. else {` is the opener of its refusal too.
OPENER = re.compile(r"^\s*(\}\s*else\s+if\b|if\b|match\b|let\s+\w+\s*=\s*if\b|let\b.*\belse\s*\{\s*$)|=>")
MAX_UP = 10

# (file, anchor, reason). The anchor is a substring that occurs ONCE in the
# file and lies in the site's guard block. Reasons say why no row is needed:
# an OS error propagated (the operation fails closed and no input chooses
# success), a check dominated on every path by a named row, or an
# operator-authored field (the config file is operator-owned, M585/M586).
PL = "crates/axon-fabric/src/privileged_launcher.rs"
SE = "crates/axon-fabric/src/sealed_exec.rs"
BIN = "crates/axon-fabric/src/bin/axon-protected-launcher.rs"
TERM_EXEMPT = []  # amendment 95: (file, a substring of the term, a fact)
EXEMPT = [
    (PL, "    if unsafe { libc::fstat(fd, &mut st) } != 0 {",
     "OS error from fstat on an open descriptor: fails closed, no input chooses success"),
    (PL, "    if fd < 0 {\n        return Err(std::io::Error::last_os_error());",
     "OS error from openat: fails closed"),
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
    (PL, "        Err(why) => return (LaunchReport::refused(why), EXIT_REFUSED),\n    };\n    match prepare(",
     "NOTHING TO ADMIT: authenticated() refused (load_config failed, the caller is not the "
     "Fabric uid (M531), or becoming root in every id failed: an OS error that leaves the "
     "process not root, possible only without CAP_SETUID/SETGID, and the binary refuses before "
     "this unless its euid is 0, M602); re-reported, nothing launched"),
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
EV = "crates/axon-loop/src/evl.rs"
EXEMPT += [
    (EV, '        refused(format!(\n            "experiment {} has no assignment journalled before execution',
     "NOTHING TO ADMIT: with no journalled assignment there is no population to judge the request "
     "against (ADR-001 §3.6)"),
    (EV, '                    unknown(\n                        UnknownKind::Unbound,\n                        format!(\n                            "unbound: attempt {} is not this trial\'s issued attempt',
     "NAMED ROW: M107 mutates this arm's condition (`&d.ep.identity.attempt_id != issued_attempt`), "
     "the same removal"),
    (EV, "        if arm.assigned != arm.verified_pass + arm.fail + kinds + arm.missing",
     "UNREACHABLE: every assigned trial increments `assigned` and exactly one counter: a missing one "
     "`missing` and `unknown` (no kind); a delivered one is VerifiedPass or Fail with no kind, or "
     "Unknown through `unknown()`, which always gives a kind; so both sums hold by construction"),
    (EV, "    if intaken.contains_key(&d.ep_ref) {",
     "NAMED ROW: M15 mutates the call `intake_join(&intaken, d)` to Ok, the same removal as this "
     "function's Err arm"),
    (EV, "            _ => Err((\n                UnknownKind::MissingEvidence,",
     "NOTHING TO ADMIT: without the check's request and receipt there is nothing to authenticate"),
    (EV, "        VerificationResult::NotRun => unknown(",
     "NOTHING TO ADMIT: a check that did not run has no verdict; this arm only names the Unknown's "
     "kind (rowed: M133-M136)"),
    (EV, "            Some(rc) => unknown(",
     "NOTHING TO ADMIT: the signed check reached no verdict; this arm only names the Unknown's kind "
     "(rowed: M127, M128)"),
    (EV, "            None => unknown(\n                run_end.or(stated).unwrap_or(UnknownKind::MissingEvidence),",
     "NOTHING TO ADMIT: an uncited unknown has no verdict; this arm only names the Unknown's kind "
     "(rowed: M126, M136)"),
    (EV, '.ok_or_else(|| refused(format!("evaluation has no arm for policy {p}")))?;',
     "NOTHING TO ADMIT: no arm ran the policy, so there is no arm to return"),
]
ST = "crates/axon-loop/src/store.rs"
EXEMPT += [
    (ST, "        if operator_key.len() < 16 {",
     "OPERATOR-AUTHORED: AXON_ATTEST_KEY is the operator's environment for the loop process; the "
     "refusal only fails closed (the store does not open)"),
    (ST, "            Err(std::env::VarError::NotUnicode(_)) => Err(LoopError::Usage(format!(",
     "OPERATOR-AUTHORED: the operator's environment; the refusal only fails closed (the store does "
     "not open, never falls back to unkeyed)"),
    (ST, "                Err(e) => return Err(e.into()),\n            }\n        }\n        Ok(())",
     "OS ERROR: lstat failing for a reason other than NotFound; the path is refused (fails closed)"),
    (ST, "                Err(e) => return Err(e.into()),\n            }\n            let m = fs::symlink_metadata(&cur)?;",
     "OS ERROR: create_dir failing for a reason other than AlreadyExists; nothing is written"),
    (ST, "            Err(e) => Err(map_open(p, e)),",
     "OS ERROR: the open failed (an O_NOFOLLOW refusal of a final-component symlink is the kernel's "
     "ELOOP, which map_open only names); nothing is read"),
    (ST, '            .ok_or_else(|| crate::error::refused(format!("no {kind} record {r}")))',
     "NOTHING TO ADMIT: no such record, so there are no bytes to return"),
]
SA = "crates/axon-loop/src/safety.rs"
EXEMPT += [
    (SA, "    if (r.finding == Finding::Violation) != r.code.is_some() {",
     "SELECTS NOTHING: only `finding` decides (safety::states): a clearance's code is never read, "
     "and a violation with no code is never a veto (states keeps the prior state), so admitting "
     "either changes no safety state"),
    (SA, '            refused(format!(\n                "no intaken trial {:?} in this scope',
     "NOTHING TO ADMIT: no intaken trial to attach the finding to; its subjects come from that "
     "intake record"),
    (SA, '                refused("a clearance is not authenticated: no monitor signature was presented")',
     "NOTHING TO ADMIT: no signature to verify (M984 rows the verification)"),
]
RU = "crates/axon-loop/src/rules.rs"
EP = "crates/axon-loop/src/epoch.rs"
LL = "crates/axon-loop/src/lib.rs"
EXEMPT += [
    (EP, "    if now != claimed {",
     "UNREACHABLE: require_current has no production caller (only crates/axon-loop/tests/pointer.rs); "
     "Fabric's launch-time epoch check reads epoch::current and compares itself (submit.rs)"),
    (LL, '                    Err(serde::de::Error::custom(format!(\n                        "schema must be {:?}, got {:?}",',
     "SELECTS NOTHING: a record's schema tag is a version label; every record type is "
     "deny_unknown_fields with its own field set, and every stored record is read back through its "
     "digest name (M978) or the ledger chain, so the tag chooses no field and no code path"),
]
PLN = "crates/axon-loop/src/plan.rs"
EXEMPT += [
    (PLN, '        _ => return Err(refused(format!("no registered plan {id}"))),',
     "NOTHING TO ADMIT: no registered plan, so there is no plan to load"),
    (PLN, '        return Err(LoopError::NotReady(format!("plan {id} is not frozen")));',
     "NOTHING TO ADMIT: no frozen plan, so nothing to judge the experiment by"),
]

# ── C9 round 4b, CORE2 (amendment 60): the interpreter's declared-type cast
# (conform.rs, every refusal arm, in scope by SCOPE_FILES) and its seal edges
# (interp.rs, scanned in the union of SCOPE_FN_REGIONS' functions and the
# anchored REGIONS span above). The interpreter refuses with `return panic(`
# as well as `Err(`; CTOR's `panic(` is that site.
CF = "crates/axon-core/src/interp/conform.rs"
EXEMPT += [
    (CF, '            return Err("the value is nested too deeply to check against its declared type".into());',
     "NO INPUT REACHES IT: a value nested 1,000,000 deep cannot be built in a run (each "
     "construction copies and casts what it nests; measured 2,000/4,000/8,000 levels of a "
     "recursive struct at 1.7s/7.8s/78s), and a declared type is that deep only through "
     "recursion the value itself must supply"),
    (CF, "                    if ps.len() != params.len() {",
     "NO CONSEQUENCE: a closure of another arity panics at its first call (call_closure checks "
     "the count) and dispatches on `fn` whatever its arity, so it selects no other code"),
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
_JNV = ("NOT A VERDICT PROPERTY (checkable): a journal state transition, budget or settlement "
        "rule. The journal's recorded intent and outcome are read back in exactly three places "
        "(grep `ran_under_of|v.outcome|intent.config` outside journal.rs): submit::replayed, whose "
        "Submission has `replayed: true`, which signing refuses first (signing.rs `if replayed { "
        "return Err(REPLAYED) }`, rows M01/M402); the status CLI, which prints it; and status/cancel's "
        "registry-sha check (M279). A fresh run's receipt, class and ran_under are built in memory "
        "in submit (`RanUnder` from the selected profile and grant), never read from the journal")
_BRN = ("NOT ON THE PROTECTED ROUTE (checkable): `grep -rn 'Branches' crates/*/src` outside "
        "branches.rs finds only submit.rs, which calls Branches::open, branch_of_run and is_cancelled "
        "(the cancelled-branch refusal is M1066); open_experiment, publish and cancel are called only "
        "from tests/branches.rs and tests/restart_matrix.rs, so no production route reaches this site")
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
     "NOTHING TO ADMIT: no file exists at the signature path, so there are no signature bytes to "
     "verify (read_regular of the same path then fails with ENOENT); this only names the refusal "
     "RULE:unsigned"),
    (FB, '        if !p.job_kinds.contains(&req.job_kind) {\n            return Err(Unsupported(format!(\n                "{}: job_kind {:?} unsupported — the protected profile',
     "NAMED ROW, THE SAME REMOVAL (not a second check): JobKind has exactly two variants and this "
     "profile's job_kinds is [RegisteredCheck], so this condition refuses exactly InterpreterRun; "
     "M321 adds InterpreterRun to the job_kinds this very condition reads: the two mutations admit "
     "exactly the same requests at this one site"),
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
    (FS, '            .any(|c| !matches!(c, std::path::Component::Normal(_)))', _DEV),
    (FS, '            if r != *want {', _DEV),
    (FS, '        } else if *want == workspace_digest(&file, &bytes) {', _DEV),
    (FS, '        .any(|e| e.path == file && e.mode != workspace::MODE_LINK)', _DEV),
    (FS, '    if !store.contains(cand) {',
     "NOTHING TO ADMIT: contains() is `manifest_path(cand).is_file()`; with no manifest file the "
     "next statement's load() has no manifest bytes (NotFound -> NotPublished, or a read error), so "
     "there is no version to judge"),
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
     "NAMED ROW, THE SAME REMOVAL: this arm is the complement of the `Ok(now) if now == "
     "expected_epoch` arm; M253 replaces that arm with `_ => Ok(..)`, which is exactly the admission "
     "this arm's removal would make (the match has no other arm for this case)"),
    (FS, '                    let why = if why.starts_with("preflight observation refused") {',
     "NOT A SITE: Journal::fail records the terminal state the arm already decided"),
    (FS, '            let why = format!("backend {other} has no dispatcher");',
     "NOT A SITE: the matched `fail(` is Journal::fail recording the arm's own decision (no backend "
     "dispatcher, so nothing ran); no value is admitted or refused here"),
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
    (FG, '        Err(e) => return Err(format!("{}: {e}", gitdir.join("commondir").display())),', _IO),
    (FG, '                        return Err(format!("commit {c} names a malformed parent"));',
     "NO OUTCOME: the walk can only answer `descends` by reaching the target, and the target is "
     "always a 40-hex id (object_named returns only is_oid names); a parent that is not 40 hex never "
     "equals it and names no object, so following it can never make a non-descending HEAD descend"),
    (FG, '        if lines.next() != Some(ALLOWLIST_SCHEMA) {',
     "OPERATOR-AUTHORED: the allowlist is read only after its chain is root-owned and unwritable "
     "(M502, M1070, M1071); an entry covering a source is still refused (M503)"),
    (FG, '            if bad {', "OPERATOR-AUTHORED: as the allowlist schema above"),
    (FG, '        return Err(format!("{} is not an absolute path", path.display()));',
     "UNREACHABLE (checkable): owned_chain's path is AllowlistSource::path, which is the absolute "
     "constant ALLOWLIST_PATH (`operator()`) or a path a test passes to `test()`, which is "
     "#[cfg(any(test, feature = \"test-trust-root\"))]: no production input names a relative path"),
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
    (FG, '        return Err(format!("cannot read {path}: refused, never interpreted"));',
     "UNREACHABLE (measured, checkable): refuse_config's three callers (provenance_with, "
     "provenance::lineage, readiness) each call git_data::discover on the same repository first, "
     "and discover's own_repository runs `git rev-parse --git-common-dir`, which reads this same "
     "config file; a config `git config --list` cannot read (a bad line, a directory) is one "
     "rev-parse dies on too, so discover refuses before this line ('... --git-common-dir failed'). "
     "With run()'s status check (M1086) also removed, own_repository canonicalizes the empty "
     "answer and fails: the four-cell run of this site (its former row number was never allocated) read set_off=ATTACK_REFUSED. "
     "Test: provenance::tests::a_config_git_cannot_read_is_never_a_clean_tree"),
    # provenance.rs
    (FP, '        Err(e) => return unknown(e),\n    };\n    // The repository',
     "NOTHING TO ADMIT: no working tree top, so provenance is unknown with a dirty reason "
     "(fails closed)"),
    (FP, '        Err(e) => return unknown(e),\n    };\n    let watch',
     "NOTHING TO ADMIT: no HEAD revision, so provenance is unknown with a dirty reason"),
    (FP, '    if rev.is_empty() || rev.starts_with(\'-\') {\n        return Err(format!("{rev:?} is not a revision"));\n    }\n    git_data::refuse_config(&top)?;',
     "OPERATOR-AUTHORED (checkable): the revision comes from the guest build's PCI_CERTIFIED "
     "constant (scripts/linux_profile_manifest.py, `--lineage`/`--descends`), never from the tree "
     "under review; an empty or option-like value names no object and answers no lineage"),
    # signing.rs
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
     "NO OUTCOME TOWARD ADMISSION: the argument is the compile-time build constant; in a production "
     "build it is `false` (so a mutation to false changes nothing) and `true` only makes the answer "
     "non-authoritative; the rule that reads it is M415"),
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
    (FC, '        refuse(\n            "usage",\n            &format!("--authority must be one of', _USE),
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
     "NO OUTCOME: the same parse function on the same bytes submit() parsed successfully a few "
     "lines up (the request text is read once into `text`); a deterministic parse cannot fail here"),
    (FC, '                            .unwrap_or_else(|e| refuse("io", &e, 2));', _IO + " (output)"),
    (FC, '        Err(e) => refuse(e.kind(), &e.to_string(), e.exit_code()),',
     "NOTHING TO ADMIT: submit refused, so there is no submission to print"),
    (FC, '        .unwrap_or_else(|e| refuse("usage", &format!("--tenant: {e}"), 2));', _USE),
    (FC, '            .unwrap_or_else(|e| refuse("workspace", &e.to_string(), 2));', "NOTHING TO ADMIT: no store"),
    (FC, '    .unwrap_or_else(|e| refuse(e.class(), &e.to_string(), 3));', "NOTHING TO ADMIT: no tree imported"),
    (FC, '        .unwrap_or_else(|e| refuse("workspace", &e.to_string(), 2));\n    println!(',
     "NOTHING TO ADMIT: nothing published"),
    (FC, '        .unwrap_or_else(|e| refuse("journal", &e.to_string(), 2))\n        .unwrap_or_else(|| refuse("unknown_op"',
     "NOTHING TO ADMIT: no journal"),
    (FC, '        .unwrap_or_else(|| refuse("unknown_op", &format!("no operation {op}"), 5));',
     "NOTHING TO ADMIT: the journal holds no op"),
    (FC, '        refuse("unknown_op", &format!("no operation {op}"), 5)\n    };\n    if v.intent',
     "NOTHING TO ADMIT: no such op"),
    (FC, '        .unwrap_or_else(|e| refuse("journal", &e.to_string(), 2));\n    (j, op)', _NTA),
    (FC, '        None => refuse("unknown_op", &format!("no operation {op}"), 5),',
     "NO OUTCOME: Journal::view on the in-memory journal authorized() returned, for the op it "
     "found there; the journal only appends, so the op cannot be absent"),
    (FC, '        refuse("unknown_op", &format!("no operation {op}"), 5)\n    };\n    let billing',
     "NO OUTCOME: Journal::view on the in-memory journal authorized() returned, for the op it "
     "found there; the journal only appends, so the op cannot be absent"),
    (FC, '        Err(e) => refuse("journal", &e.to_string(), 5),', _NTA + " (the cancel transition failed)"),
]
EXEMPT += [
    # workspace.rs
    (FW, '            return Err(ImportRefusal::QuotaEntries {',
     "RESOURCE BOUND: without it a larger tree is admitted, but its reference still covers exactly "
     "its bytes, and the guest's walk applies its own quota"),
    (FW, '                        return Err(ImportRefusal::Collision {',
     "NOT A VERDICT PROPERTY: the case-folding rule is for case-insensitive filesystems; on the "
     "Linux guest and host each spelling is its own file and the guest's digest still equals the "
     "reference (M159); a file/directory collision fails create_dir_all (fails closed)"),
    (FW, '            return Err(ImportRefusal::SpecialFile(rel.into()));',
     "DEVELOPMENT ROUTE: import_file's only caller is check_target's plain argv file (submit)"),
    (FW, '        if meta.len() > quota.bytes {', "DEVELOPMENT ROUTE and RESOURCE BOUND: as above"),
    (FW, '        Err(e) => Err(e.into()),\n    }\n}', _IO),
    (FW, '    }\n    Err(e)', _IO + " (renameat2)"),
    (FW, '            Err(e) => return Err(e.into()),\n        }\n        if files.is_empty() {', _IO),
    (FW, '            return Err(StoreError::Corrupt(format!("{r} has no omission record")));',
     "NOT A VERDICT PROPERTY: omissions are not part of the reference; they are read only into "
     "WorkspaceTree.omissions, which only `axon-fabric workspace import` prints"),
    (FW, '            if f.parent() == Some(self.omissions_dir(r).as_path())', "NOT A VERDICT PROPERTY: as above"),
    (FW, '                return Err(StoreError::NotPublished(r.to_string()))', "NOTHING TO ADMIT: no manifest"),
    (FW, '            Err(e) => return Err(e.into()),\n        };\n        if workspace_version_ref', _IO),
    (FW, '                    return Err(StoreError::Io(format!("no symlinks here: {target}")));',
     "NON-UNIX: compiled only under cfg(not(unix))"),
    # journal.rs
    (FJ, '                    return Err(JournalError::ScopeConflict {', _JNV),
    (FJ, '                        return Err(JournalError::Conflict {\n', _JNV),
    (FJ, '                    return Err(bad(&v, "reserved"));', _JNV),
    (FJ, '                    return Err(JournalError::BudgetExceeded {', _JNV),
    (FJ, '                    return Err(bad(&v, "failed"));', _JNV),
    (FJ, '                    _ => return Err(bad(&v, "cancelled")),', _JNV),
    (FJ, '                    return Err(bad(&v, "outcome_unknown"));', _JNV),
    (FJ, '                        return Err(JournalError::SettlementConflict {', _JNV),
    (FJ, '                    _ => return Err(bad(&get(op)?, "settle_conflict")),', _JNV),
    (FJ, '                    return Err(bad(&v, "outcome"));', _JNV),
    (FJ, '            return Err(JournalError::Locked(path.to_path_buf()));', "OS ERROR: the lock is still held at the deadline; nothing runs"),
    (FJ, '            Err(e) => return Err(JournalError::Io(e)),', "OS ERROR: stat failed other than NotFound (NotFound is M333)"),
    (FJ, '                        return Err(JournalError::Corrupt {', _JNV + "; as above"),
    (FJ, '            Err(e) => Err(e),\n        }\n    }\n\n    /// Carve', _IO + " (append's own refusals are their own sites)"),
    (FJ, '        ) {\n            return Err(JournalError::BudgetExceeded {', _JNV),
    (FJ, '                Err(JournalError::SettlementConflict { op, reason })', _JNV),
    (FJ, '            Err(e) => Err(e),\n        }\n    }\n\n    /// Attach', _IO + " (passes on the error)"),
    # branches.rs
    (FR, '        Err(e) => Err(e.into()),', _IO + " (create_once's hard_link)"),
    (FR, '                return Err(BranchError::Invalid(format!("arm {a} declared twice")));', _BRN),
    (FR, '        if approvers.is_empty() {', _BRN),
    (FR, '                return Err(BranchError::Exists(format!(', _BRN),
    (FR, '            return Err(BranchError::Unknown(format!(',
     "NOT A VERDICT PROPERTY: experiment(), reached on submit through branch_of_run; the lookup can "
     "only add a refusal, and the experiment/arm it names goes only into the intent (read back "
     "only by replays, never signed)"),
    (FR, '        if !store.contains(&req.new_version) {', _BRN),
    (FR, '            || rc.verification != ReceiptVerification::Passed', _BRN),
    (FR, '            || rc.output_workspace_ref.as_ref() != Some(&req.new_version)', _BRN),
    (FR, '                return Err(BranchError::Conflict(format!(', _BRN),
    (FR, '            Err(e) => return Err(e.into()),', _BRN),
    (FR, '            journal.fail(',
     "DOMINATED (checkable): this is the lost head-CAS arm (`if !create_once(&file, ..)?`). The only writers of "
     "`head-<n>.json` are `open_experiment` (n = 0, at creation) and this function (`grep -n 'head-' "
     "crates/axon-fabric/src/branches.rs`: the `head()` reader, `head-0.json` and this one "
     "`join(format!(\"head-{}.json\", next.seq))`), `n` is `head.seq + 1` from the `head()` read, and "
     "the operation id `pub.<experiment>.<arm>.<n>` is journalled by `Journal::begin` BEFORE this line, which "
     "refuses a second intent under that id (the `Err(JournalError::Conflict) | Ok(Begin::AlreadyRecorded)` arm "
     "above, a BranchError::Conflict; rowed M2345 for the journal's side), so `create_once` can answer false only for "
     "a head file that exists with no journal operation, and `head()` reads the highest head file, so a planted one "
     "moves `head.seq` and the expected-base refusal (M2355) fires first. NOT ROWED, with the reason: the "
     "exemption survey's `false && (..)` edit of this opener also deleted the `create_once` CALL (the right-hand "
     "side is never evaluated), so that 'kill' was the missing head file and not this guard's own attack; keeping "
     "the call and ignoring its answer is reached by no test or caller, and `two_concurrent_publications_from_one_head_"
     "have_exactly_one_winner` refuses its loser at the journal"),
    # grants.rs
    (FA, '        if v.get("schema").and_then(|s| s.as_str()) != Some(GRANT_REGISTRY_SCHEMA) {',
     "OPERATOR-AUTHORED: on a protected host the registry is parsed only at its pin (M277, M275, "
     "M276); a development registry is the caller's own"),
    (FA, '                return Err(format!("grant registry names `{grant_ref}` twice"));', "OPERATOR-AUTHORED: as above"),
    (FA, '                return Err(format!("grant `{grant_ref}`: sha256 must be 64 hex"));',
     "OPERATOR-AUTHORED: as above; a non-hex value also never equals the digest computed below"),
    (FA, '        if manifest.program != base.join(PLACEHOLDER_PROGRAM) {',
     "UNREACHABLE EFFECT: the parsed program is never read; ResolvedGrant::manifest_for replaces "
     "it with the request's program"),
]



# C9 round 4b, INTEGRATE-A (amendment 64): the contract crate's remaining
# sites. Kinds, as above, plus: COMPILED-IN (an arm that refuses only a
# malformed SCHEMA, never a document: the schemas `validate_against` walks are
# include_str! constants -- crates/axon-loop-contracts/schemas/*.json via
# schema_text!, and crates/axon-loop/schemas/closed-loop-pilot.schema.json --
# and redteam.rs every_checked_in_schema_is_within_the_supported_subset asserts
# over every one of them that each schema position is an object, every keyword,
# `type` and pattern is in the walker's subset and additionalProperties is a
# boolean, so no input reaches these arms).
CT = "crates/axon-loop-contracts/src/canonical.rs"
CI = "crates/axon-loop-contracts/src/ids.rs"
CP = "crates/axon-loop-contracts/src/policy.rs"
CS = "crates/axon-loop-contracts/src/schema.rs"
_COMPILED = ("COMPILED-IN: refuses only a malformed checked-in schema (see the block comment); "
             "redteam.rs every_checked_in_schema_is_within_the_supported_subset holds the fact")
EXEMPT += [
    (CT, "            shape(m)\n",
     "NOTHING TO ADMIT: typed serde refused the value, so there is no typed document; the arm "
     "only classifies the refusal (unknown field/variant vs shape), both arms refuse"),
    (CT, "        return Err(Refusal::TooLarge(bytes.len()));",
     "OPERATOR-SIGNED: parse_bytes's only production caller (grep `parse_bytes(` in crates/*/src) "
     "is readiness.rs's read of the certified compute request, bytes the operator-signed "
     "evidence bundle pins by sha256 (M691/M335); and parse_bytes hands the same text to "
     "parse_value, whose byte limit is M1200"),
    (CT, "    let text = std::str::from_utf8(bytes).map_err(|e| shape(format!(\"invalid UTF-8: {e}\")))?;",
     "NOTHING TO ADMIT: bytes that are not UTF-8 have no text to parse; the only production "
     "caller reads operator-signed bytes (as above)"),
    (CP, "        if self.pinned_at_ms < 0 {",
     "NO PRODUCTION PARSE (checkable): PolicyPin is MiCode's local document; Axon never parses one "
     "(no parse/contract_from_value/from_value of PolicyPin in crates/*/src) and builds one only "
     "in pointer.rs::resolve, with pinned_at_ms = now_ms(), which is >= 0"),
    (CP, '                    return Err(semantic(format!(\n                        "{:?} requires target_policy_ref",',
     "UNREACHABLE TO ADMIT (FLAGGED: no four-cell can be built): the schema's conditional also "
     "requires a string target (M1210), and with both removed pointer.rs's activate/rollback "
     "arm still applies nothing: `t.target_policy_ref.clone().expect(\"validated by parse\")` "
     "panics before any write, and a target is accepted only when an admission record names it "
     "(pointer.rs `adm.target_policy_ref != target`)"),
    (CP, '                    return Err(semantic(format!("{:?} requires admission_ref", self.kind)));',
     "UNREACHABLE TO ADMIT (FLAGGED: no four-cell can be built): as above: the schema's "
     "conditional requires a string admission_ref (M1210), and pointer.rs's `t.admission_ref"
     ".clone().expect(\"validated by parse\")` then the admission lookup by that ref refuse it"),
    (CS, '    shape(format!("{path}: {msg}"))',
     "NOT A SITE: the body of the walker's `fail` constructor; each use is its own site"),
    (CS, 'Value::Bool(false) => return Err(fail(path, "schema `false` admits nothing")),', _COMPILED),
    (CS, '_ => return Err(fail(path, "malformed schema node")),', _COMPILED),
    (CS, "            return Err(fail(path, format!(\"unsupported schema keyword {k:?}\")));", _COMPILED),
    (CS, 'other => return Err(format!("unsupported type {other:?}")),', _COMPILED),
    (CS, '_ => return Err(fail(path, "malformed `type`")),', _COMPILED),
    (CS, "        let matched = type_matches(n, v).map_err(|e| fail(path, e))?;",
     _COMPILED + " (type_matches errs only on an unsupported type name)"),
    (CS, 'let p = p.as_str().ok_or_else(|| fail(path, "malformed pattern"))?;', _COMPILED),
    (CS, "let ok = pattern_matches(p, st).map_err(|e| fail(path, e))?;",
     _COMPILED + " (pattern_matches errs only on an unsupported pattern)"),
    (CS, 'other => return Err(format!("unsupported pattern {other:?}")),', _COMPILED),
    (CS, 'Some(_) => return Err(fail(path, "unsupported additionalProperties form")),', _COMPILED),
    (CS, 'serde_json::from_str(text).map_err(|e| shape(format!("checked-in schema unreadable: {e}")))',
     "COMPILED-IN: schema::load reads only Contract::SCHEMA, the schema_text! include_str! "
     "constants, each parsed by redteam.rs every_checked_in_schema_is_within_the_supported_subset"),
]

# C9 round 4b, INTEGRATE-C (amendment 64)
# Kinds as above. NOTHING TO ADMIT: the arm holds no value the code after it
# could continue with.
PTR = "crates/axon-loop/src/pointer.rs"
EXEMPT += [
    (PTR, '            refused(format!(\n                "rollback target {target} was never an active predecessor in this scope"',
     "NOTHING TO ADMIT: no history entry names the target, so there is no predecessor (its "
     "admission, label and authority) for the rollback rules below to judge; the arm is the "
     "ok_or_else of that lookup"),
]
# ── end INTEGRATE-C ──


# C9 round 4b, INTEGRATE-D (amendment 64): price.rs, tel.rs, the psv runner.
# Kinds as above, plus NO PRODUCTION CALLER (the function's callers are named
# by a grep; none is outside tests).
PRC = "crates/axon-loop/src/price.rs"
TEL = "crates/axon-loop/src/tel.rs"
PRN = "crates/axon-psv/src/bin/axon-psv-runner.rs"
_COHORT = ("NO PRODUCTION CALLER (checkable): `grep -rn 'cohort_cost' crates/*/src` finds only tel.rs's "
           "own definition and doc comment; it is called only from crates/axon-loop/tests/tel_price.rs. "
           "evl/admission build economics through summarize_with_missing/summarize_with_execution, "
           "and the tel CLI through summarize/join, none of which reaches it")
EXEMPT += [
    (PRC, '        refused(format!(\n            "price_schedule_ref {o} is not a content Ref',
     "NOTHING TO ADMIT: the request's opaque string is not a Ref at all, so there is no ref to compare "
     "with the pinned schedule (the comparison is M1351)"),
    (TEL, '        return Err(refused("a cohort with no assigned trials has no cost"));', _COHORT),
    (TEL, '        return Err(refused(format!(\n            "{} missing records exceed {assigned} assigned trials",', _COHORT),
    (TEL, '            return Err(refused("no usage records: the cohort\'s cost is unknown"))', _COHORT),
    (TEL, '        _ => return Err(refused("currencies are never mixed in one cohort cost")),', _COHORT),
    (PRN, '        return Err("axon-psv-runner takes no arguments".into());',
     "SELECTS NOTHING (checkable): main passes start() only std::env::args().len(); no argument is "
     "ever read, every path is a compiled-in guest path and the manifest digest comes from "
     "/proc/cmdline (run_guest), so an extra argument changes nothing the runner does"),
    (PRN, '        Err(e) => Err(format!("could not write the verdict: {e}")),',
     "NOTHING TO ADMIT: run_and_emit failed to write /out/verdict.json, so there is no verdict or "
     "digest to print; the runner exits 3 and the host reads no verdict (psv.rs 'no guest verdict', "
     "an Unknown)"),
]



# C9 round 4b, INTEGRATE-E (amendment 64): the strict rework. The sites below
# were exempted as RE-REPORTED / UNREACHABLE (another check refuses first).
# Each was driven on its production route with the check that refuses first
# ALSO removed, and the input was still refused: the site reads a value the
# input does not have, and every later statement needs that value too
# (logs: the integrate-E report). That is NOTHING TO ADMIT, executed, not a
# dominance argument; the cells are named in each reason.
EXEMPT += [
    (EV, '        return Err("no Fabric execution attestation was delivered".into());',
     "NOTHING TO ADMIT (executed, integrate-E): a null attestation holds no issuer, key or signature. "
     "A protected trial delivered with none, through evaluate: this check removed -> refused \"names no "
     "issuer\"; that also bypassed (issuer substituted) -> refused \"signature: not a JSON object\"; "
     "every later step of verify_document reads a field a null lacks"),
    (EV, "    if d.ctx_sig.is_null() {",
     "NOTHING TO ADMIT (executed, integrate-E): no context signature was presented. Through evaluate on a "
     "protected plan: this check removed -> \"signature: not a JSON object\"; that also bypassed (an empty "
     "map) -> \"alg is not ed25519\"; every later field (public_key, signature) is equally absent"),
    (EV, '.ok_or_else(|| refused("an authenticated attestation states no issued_ms"))?;',
     "NOTHING TO ADMIT (executed, integrate-E): issued_ms is a SIGNED field of the attestation the "
     "statement before verified; an attestation without it has no signed time. Through intake+evaluate: "
     "this arm substituted (u64::MAX) -> intake refuses \"no issued_ms\"; attestation::verify's own "
     "refusal also substituted (0) -> refused by the bound-field comparison (\"issued_ms is null but the "
     "evidence it must vouch for has 0\", M03): no input reaches this arm with the time absent"),
    (EV, '.ok_or_else(|| refused("an authenticated verdict names no issuer"))?,',
     "NOTHING TO ADMIT (executed, integrate-E): a verdict with no issuer has no key to be verified under "
     "(verify_check_evidence looks the key up by the issuer). Through intake+evaluate on failed verdicts "
     "with issuer_ref null: this arm substituted -> intake refuses (\"not a trusted verifier independent "
     "of the subject\"); that refusal also removed -> verify_check_evidence's issuer lookup panics "
     "(intake.rs `expect(\"checked just above\")`): fails closed, never admits"),
]
# C9 round 4b, INTEGRATE-2 (amendment 64). FLAGGED for the integrator (ruling
# R2's form: the measured experiment is stated).
EXEMPT += [
    (OT, "    if !dir.is_absolute() {",
     "NO RELATIVE INPUT (checkable; FLAGGED): every production caller passes an absolute base and an "
     "absolute dir: the bases are the literal `/` (root_keys_hex, check_operator_chain, trusted_issuers' "
     "`operator_owned.then_some(Path::new(\"/\"))`, readiness's production `ownership_base`, the protected "
     "host's `Some(Path::new(\"/\"))`), and the dirs are compiled-in constants (OPERATOR_TRUST_ROOT's "
     "operator_dir, PROTECTED_HOST_CONFIG) or paths of the root-owned protected-host and custodian configs, "
     "whose loaders refuse a relative path (protected_host.rs `is not absolute: a protected path is a fixed "
     "host path`, `{ptr} is not absolute`; custodian.rs `is not an absolute plain path`); the verify CLI's "
     "caller-chosen --issuers is authoritative only when it IS the operator root (verify_evidence_authority). "
     "Measured (integrate-2): with this check removed, check_owned_chain(\"/\", d) for d in \"etc\", "
     "\"etc/axon/trust\", \"./etc\", \"\" still refuses each (`is not below /`, strip_prefix): no four "
     "cells exist because that refusal is not a refusal site of its own"),
]



# C9 round 4b, workstream GAPS (amendment 65): the custodian program pin's
# OS-error and defence-in-depth sites, and the NoNewPrivileges diagnostic.
EXEMPT += [
    (PL, "    Err(if no_new_privs() {",
     "DIAGNOSTIC, dominated (measured, amendment 65): a setuid-installed helper whose euid is not 0 is "
     "refused on the production route by the euid rule (M602) and on a test-trust route by the "
     "operator-file owner rule (read_operator_file: its config is root's, not the caller's); this "
     "site only names the cause (NoNewPrivileges or nosuid) before them"),
    (BIN, "    if let Err(why) = pl::setuid_honoured(euid) {",
     "DIAGNOSTIC, dominated: the caller of setuid_honoured, exempt for the same reason (M602; the "
     "test-trust owner rule)"),
    (BIN, "    if let Err(why) = pl::session_left() {",
     "DIAGNOSTIC: the caller of session_left, whose own guard is M1604 (its attack: a process-group "
     "leader's helper launching anyway)"),
    (CU, "    if r != 0 {\n        return Err(format!(\n            \"SO_PASSPIDFD:",
     "OS error from setsockopt: a kernel that cannot name a reply's sender refuses the call (fails "
     "closed); no sender is assumed"),
    (CU, "            if e.kind() == std::io::ErrorKind::Interrupted {",
     "OS error from recvmsg: the call is refused; EINTR is retried"),
    (CU, "        if msg.msg_flags & libc::MSG_CTRUNC != 0 || other {",
     "defence in depth, dominated by the per-message pin check (M1483): every descriptor received "
     "is owned and closed, and a truncated control buffer has no SCM_PIDFD, which the next line "
     "refuses (the kernel named no sender)"),
    (CU, "        if text.len() as u64 > max {",
     "a memory bound on the reply, not an authority decision: every byte under it was already "
     "attributed to the pinned program"),
    (CU, "    if pidfd_pid(pidfd) != Some(pid) {",
     "a race that fails closed (the sender exited between naming it and opening its executable); "
     "not deterministically reachable, and its absence would still hash an executable the pinned "
     "check must match"),
]

# C9 round 4c, r4c-fixes part 2 (amendment 71): the sites the crate rule and
# the broadened forms bring in, outside the rows M1630-M1651. Kinds as above,
# plus: RESOURCE BOUND (a quota that bounds memory/time; the reference still
# covers exactly the bytes admitted), RELAY (the site exits with a code it was
# handed; it decides nothing), NOT A SITE (the body of a refusal constructor,
# each call of which is a site of its own), NON-PRODUCTION (compiled out of
# the production build by a named cfg).
EVO = "crates/axon-loop/src/evo.rs"
GIN = "crates/axon-guest-init/src/main.rs"
WRC = "crates/axon-workspace-recipe/src/lib.rs"
CMN = "crates/axon-core/src/main.rs"
CRS = "crates/axon-core/src/resolver.rs"
CUB = "crates/axon-fabric/src/bin/axon-custodian.rs"
OBB = "crates/axon-fabric/src/bin/axon-observer.rs"
_OBS_TEST = ("NON-PRODUCTION (checkable): inside axon-observer's `test_config` closure, whose only call "
             "is the `[\"--test-config\", _] if TEST_TRUST_BUILD` arm (M1548); TEST_TRUST_BUILD is "
             "false in the production build, which reads only /etc/axon/observer.json")
FPB = "crates/axon-fabric/src/bin/axon-provenance.rs"
_MMDS = ("NOT ON THE PROTECTED ROUTE (checkable): the protected profile's VM has no network "
         "device (scripts/fc_linux_profile.sh writes `\"network-interfaces\": []`; "
         "profiles/linux-microvm/README.md: the VMM joins an empty network namespace), so "
         "read_mmds cannot connect and only its Err arm is reachable there, which refuses")
EXEMPT += [
    # evo.rs (in scope again: amendment 71)
    (EVO, "    if eligible.len() != req.eligible.len() {",
     "SELECTS NOTHING (checkable): after this line `req.eligible` is never read again "
     "(grep `req.eligible` in evo.rs: lines 190-191 only); the code reads the deduplicated set "
     "`eligible`, so a duplicate id changes nothing the proposal is built from"),
    (EVO, "    if evidence.len() > 256 {",
     "NAMED CHECK: `evidence` is exactly the candidate's discovery_evidence_refs, and "
     "candidate.validate() (policy.rs: check_array(\"discovery_evidence_refs\", .., 0, 256, "
     "true)) refuses more than 256 before anything is written (put_cas follows it)"),
    (EVO, "    if registered.eligible() != eligible {",
     "SELECTS NOTHING (checkable): the caller's `eligible` set only feeds check_shortlist; the "
     "candidate's shortlist is a remove/swap of incumbent.shortlist (mutation_space/apply: never "
     "an added id), and require_shortlist (the next statement) checks the incumbent against the "
     "REGISTERED list, so no caller set can widen what is proposed"),
    (EVO, '            refused("mutation space exhausted: every one-edit shortlist was already tried")',
     "NOTHING TO ADMIT: no untried shortlist exists to build a candidate from"),
    # axon-guest-init (PID 1 in the protected guest)
    (GIN, '                "[axon-guest-init] fork failed: {}",',
     "OS ERROR: fork failed; nothing is exec'd (fails closed)"),
    (GIN, "    if b64.is_empty() {",
     "NOTHING TO ADMIT: an empty value decodes to zero bytes, which parse_policy_json_strict "
     "refuses as BadJson (serde_json: EOF while parsing), and that refusal reaches the "
     "Refuse arm (M1637)"),
    (GIN, "    if !v.is_object() {",
     "SELECTS NOTHING beyond the rows: a non-object that deserializes into MmdsPayload (a "
     "sequence) is still held to constrains_anything (M1640) and the schema check (M1641), so "
     "admitting it admits only a policy that passes both"),
    (GIN, "        Err(e) => Err(e.to_string()),",
     "NOTHING TO ADMIT: the cmdline arm holds an error and no payload"),
    (GIN, "            Ok(Some(_)) => Err(", _MMDS + "; the cmdline arm of the same rule is M1640"),
    (GIN, "            Ok(None) => Err(", "NOTHING TO ADMIT: MMDS returned no payload"),
    (GIN, "            Err(e) => Err(format!(\n", "NOTHING TO ADMIT: MMDS failed; no payload"),
    (GIN, '        return Err("MMDS returned empty token".to_string());', _MMDS),
    (GIN, "            Err(e)\n                if e.kind() == std::io::ErrorKind::WouldBlock", _MMDS),
    (GIN, '            Err(e) => return Err(format!("read response: {e}")),', _MMDS),
    (GIN, '        return Err("BPF bytecode is empty".to_string());',
     "NAMED ROW: an empty program is a zero-length sock_fprog, which the kernel refuses "
     "(seccomp_prepare_filter: len 0 is EINVAL), and that refusal is M1645"),
    (GIN, '        if r != 0 {\n            return Err(format!(\n                "PR_SET_NO_NEW_PRIVS failed: {}",',
     "OS ERROR: prctl(PR_SET_NO_NEW_PRIVS, 1) fails only on invalid arguments (prctl(2)); the "
     "workload is not started (fails closed)"),
    (GIN, '        process::exit(1);\n    });\n    let c_args',
     "UNREACHABLE: the path is an argv string, and execve's argv entries are NUL-terminated C "
     "strings that cannot contain a NUL"),
    (GIN, '                process::exit(1);\n            })\n        })\n        .collect();',
     "UNREACHABLE: as above (argv strings cannot contain a NUL)"),
    (GIN, "    process::exit(127);",
     "OS ERROR: execvp failed; nothing ran (the shell convention 127)"),
    (GIN, "    process::exit(exit_code);",
     "RELAY: the supervisor exits with its first child's own status (WEXITSTATUS, or 128 + the "
     "signal); no branch here chooses the code"),
    # axon-workspace-recipe (the one walker and path rule, host and guest)
    (WRC, "        return Err(ImportRefusal::Absolute(path.into()));",
     "NAMED ROW: an absolute path's first `/`-component is empty, which the next statement's "
     "Traversal arm refuses (M1646)"),
    (WRC, "    if path.split('/').count() > quota.depth {",
     "RESOURCE BOUND: depth bounds the walk; a deeper path admitted is still exactly in the "
     "reference (as workspace.rs's quota exemptions)"),
    (WRC, "            if bytes.saturating_add(meta.len()) > quota.bytes {",
     "RESOURCE BOUND: refuses before reading a file that alone breaks the quota; without it the "
     "file is read and the next check (bytes > quota.bytes) refuses the same tree"),
    (WRC, "        if *bytes > quota.bytes {", "RESOURCE BOUND: as above"),
    (WRC, "        if entries.len() > quota.entries {", "RESOURCE BOUND: as above"),
    # axon-core main.rs (the PSV entry: cmd_test and its key reader)
    (CMN, "        std::process::exit(2)\n    };",
     "NOT A SITE: the body of the key reader's refusal constructor `fail`; each call is a site"),
    (CMN, '            Err(_) => fail("could not read the key from stdin"),',
     "NOTHING TO ADMIT: no key was read"),
    (CMN, '    key.unwrap_or_else(|| fail("the key must be at least 16 bytes of hex"))',
     "UNREACHABLE on the protected route (checkable): the runner writes completion_key(), an "
     "HMAC-SHA256 output (axon-psv lib.rs: [u8; 32]), as 64 hex digits (runner.rs: one line, then "
     "EOF); NOTHING TO ADMIT otherwise (no key)"),
    (CMN, '    if files.is_empty() {\n        eprintln!("error: no source files specified");\n        process::exit(1);\n    }\n    for f in &files {\n        validate_ax_extension(f);\n    }\n\n    // Parse and merge all source files.', _USE),
    (CMN, "        Err(errs) => {\n            for e in &errs {\n                eprintln!(\"error: {e}\");\n            }\n            process::exit(2);\n        }\n    };\n    let (mut program, merge_errors) = axon_core::merge_programs(file_programs);",
     "NOTHING TO ADMIT: the sources did not parse; there is no program"),
    # resolver.rs (the sealed-module refusal, E0004)
    (CRS, "            for n in reached {\n                self.emit_error(",
     "NAMED ROW: this is check_sealed's only emit; M79 removes the check_sealed call, the same "
     "removal"),
    # axon-custodian (the nonce custodian's CLI)
    (CUB, "    std::process::exit(2);\n}\n\nfn euid()",
     "NOT A SITE: the body of the refusal constructor `die`; each call is a site"),
    (CUB, "        die(&format!(\n            \"bind {}: {e} (it must not exist)\",",
     "OS ERROR: the socket could not be bound; the custodian serves nothing"),
    (CUB, "            .unwrap_or_else(|e| die(&e));\n            must_run_as(&c);\n            // WHERE",
     "NOTHING TO ADMIT: load_config refused (its refusals are rowed in custodian.rs); there is "
     "no config"),
    (CUB, '                v.map(|s| s.parse().unwrap_or_else(|_| die("a uid is a number")))',
     "DEVELOPMENT ROUTE: --dev (Mode::Dev); a dev custodian's reply launches nothing protected "
     "(custodian_mode_launches accepts Protected, or Test in a test authority)"),
    (CUB, '                PathBuf::from(opt("--socket").unwrap_or_else(|| die("--dev needs --socket P")));',
     "DEVELOPMENT ROUTE: as above"),
    (CUB, '                PathBuf::from(opt("--store").unwrap_or_else(|| die("--dev needs --store D")));',
     "DEVELOPMENT ROUTE: as above"),
    (CUB, '                    .map(|s| s.parse().unwrap_or_else(|_| die("--max-age-s is a number")))',
     "DEVELOPMENT ROUTE: as above"),
    (CUB, "            c.check(false).unwrap_or_else(|e| die(&e));\n            use std::os::unix::fs::DirBuilderExt;",
     "DEVELOPMENT ROUTE: as above"),
    (CUB, "        Mode::Protected => cu::activated_listener(&cfg.socket).unwrap_or_else(|e| die(&e)),",
     "NOTHING TO ADMIT: no listener (activated_listener's own refusals, the socket systemd bound, "
     "are rowed in custodian.rs: M703)"),
    # axon-fabric.rs, the launcher and provenance CLIs
    (FC, "    std::process::exit(code)\n}",
     "NOT A SITE: the body of the refusal constructor `refuse`; each call is a site"),
    (FC, "        std::process::exit(code);\n    };\n    if tamper == \"vmm-died\" {",
     "NON-PRODUCTION: inside psv_host_guest, `#[cfg(feature = \"test-trust-root\")]` (the guest "
     "emulator a production build does not contain)"),
    (FC, "        std::process::exit(code);\n    }\n    write_result(\"ok\", 0);",
     "NON-PRODUCTION: as above"),
    (BIN, '            eprintln!("usage: axon-protected-launcher [--observe] [--probe] < request.json");', _USE),
    (OBB, "    std::process::exit(2);\n}\n\nfn euid()",
     "NOT A SITE: the body of the refusal constructor `die`; each call is a site"),
    (OBB, '        let p = PathBuf::from(args.get(i).unwrap_or_else(|| die("--test-config FILE")));', _OBS_TEST),
    (OBB, '        let bytes = std::fs::read(&p).unwrap_or_else(|e| die(&format!("{}: {e}", p.display())));', _OBS_TEST),
    (OBB, '            .unwrap_or_else(|e| die(&format!("{}: {e}", p.display())));', _OBS_TEST),
    (OBB, "        c.check(false).unwrap_or_else(|e| die(&e));", _OBS_TEST),
    (OBB, '            .unwrap_or_else(|| die("a test config names its test_paths"));', _OBS_TEST),
    (OBB, "            .unwrap_or_else(|e| die(&e));\n            (\n                c,\n                Sources::operator(),",
     "NOTHING TO ADMIT: load_config refused (its refusals are rowed in observer_service.rs and "
     "privileged_launcher.rs's read_operator_file); there is no config"),
    (OBB, '        _ => die("usage: axon-observer [--test-config FILE]"),', _USE),
    (OBB, "        Mode::Protected => cu::activated_listener(&cfg.socket).unwrap_or_else(|e| die(&e)),",
     "NOTHING TO ADMIT: no listener (activated_listener's own refusals, the socket systemd bound, "
     "are rowed in custodian.rs: M703)"),
    (OBB, '            .unwrap_or_else(|e| die(&format!("bind {}: {e}", cfg.socket.display()))),',
     "NON-PRODUCTION and OS ERROR: Mode::Test and Mode::Dev are set only in the `--test-config` "
     "arm, which TEST_TRUST_BUILD gates (M1548); a failed bind serves nothing"),
    (FPB, '            eprintln!("axon-provenance: {e}");',
     "DEVELOPMENT ROUTE (checkable): `--descends` is called only by linux_profile_manifest.py "
     "--descends, \"the build's early development check [that] makes nothing clean\"; the "
     "protected lineage is the --snapshot record's descends_from_protected answer (M505)"),
]

# C9 round 4c, workstream SITES (amendment 75): the dependency sites the crate
# rule brought in (NOT_YET_SCANNED at 64d9f436), judged one by one by their
# callers on the protected route; the ledger of every disposition is
# governance/notes/v022-dependency-sites.md. The sites that DECIDE there are
# rows M1760-M1769. The rest are below, each with the call-graph fact that
# makes its reason checkable (a grep over the protected crates' sources:
# axon-fabric, axon-loop, axon-loop-contracts, axon-psv, axon-guest-init,
# axon-workspace-recipe, and the axon-core `axon test` entry).
_SA = "crates/axon-attest/src/lib.rs"
_SU = "crates/axon-audit/src/lib.rs"
_SG = "crates/axon-cortex/src/generate.rs"
_SR = "crates/axon-cortex/src/runner.rs"
_SOC = "crates/axon-os/src/coalition.rs"
_SOL = "crates/axon-os/src/ledger.rs"
_SOM = "crates/axon-os/src/manifest.rs"
_SOP = "crates/axon-os/src/profile.rs"
_SOR = "crates/axon-os/src/record.rs"
_SORP = "crates/axon-os/src/replay.rs"
_SOT = "crates/axon-os/src/runtime.rs"
_SPR = "crates/axon-psv/src/bin/axon-psv-runner.rs"
_SVA = "crates/axon-vm/src/admit.rs"
_SVF = "crates/axon-vm/src/firecracker.rs"
_VMUSE = ("the protected crates use axon-vm for exactly axon_vm::BACKEND_PROFILE and "
          "axon_vm::firecracker::{MmdsPayload, embed_policy_in_cmdline} (axon-fabric backend.rs; "
          "embed_policy_in_cmdline is one format! with no site), and the protected guest is "
          "launched by scripts/fc_linux_profile.sh through the root helper, not by axon-vm")
_VM = "NOT ON THE PROTECTED ROUTE (checkable): " + _VMUSE
_ATT = ("NOT ON THE PROTECTED ROUTE (checkable): the protected crates call one axon-attest "
        "function, hmac_sha256 (axon-loop store.rs), which holds no site; the function here is "
        "reached only from axon-attest's own measure/verify functions and from axon-vm (admit.rs, "
        "main.rs), and " + _VMUSE)
_AUD = ("NOT A VERDICT PROPERTY (checkable): no protected crate names axon_audit, but axon-core "
        "(the `axon` binary the guest execs) links it (preflight.rs, main.rs set_ledger_path/"
        "flush_ledger, interp/builtins.rs append_global/append_ai_call), so these functions ARE "
        "reachable there when AXON_AUDIT_LEDGER is set; the psv runner execs `axon test` with "
        "env_clear() (axon-psv runner.rs) and sets no AXON_AUDIT_LEDGER, and every audit failure "
        "in axon-core is only printed (set_ledger_path, flush_ledger) or discarded (`let _ =`), "
        "never an exit code or a verdict")
_CTX = ("NOT ON THE PROTECTED ROUTE (checkable): the Cortex repair loop's own code. The protected "
        "crates use from axon-cortex only parse_strict and ContractError (axon-loop-contracts) and, "
        "from runner, CheckRegistry, LocalInterpreterExecutor (the local route), the CheckExecutor "
        "types, parse_axon_test_json, completion_token, the suite-reference functions and the "
        "workspace/executable digest functions (axon-fabric submit.rs, psv.rs, workspace.rs, "
        "backend.rs, bin/axon-fabric.rs); nothing below is among them")
_EXE = ("NOT ON THE PROTECTED ROUTE (checkable): a registered EXECUTOR. A protected request's "
        "executable is the qualified guest interpreter (submit.rs: LINUX_GUEST_AXON_ID, pinned by "
        "the qualification's guest_axon_sha256); the registry's executors are read only by "
        "submit.rs resolve_executable and host_executor, in the non-protected branch, and a "
        "protected host runs nothing outside the protected profile (M265)")
_REG = ("OPERATOR-AUTHORED on the protected route (checkable): a field of the suite registry "
        "file, which a protected host reads only from its own config (M141: never a caller's "
        "--check-registry), at its pinned sha256 (M142), operator-owned (M143)")
_LOC = ("NOT ON THE PROTECTED ROUTE (checkable): the LOCAL interpreter executor (the development "
        "route, process_scoped/local-interpreter); submit.rs builds it only when the profile is "
        "not the protected one (`let local = if is_linux { None } else { .. host_executor .. }`), "
        "and a protected host runs nothing outside the protected profile (M265); the guest runs "
        "`axon test` from axon-psv's runner (exec_axon_test), not this executor")
_COA = ("NOT ON THE PROTECTED ROUTE (checkable): Coalition::new is pub, but it has no non-test "
        "CALLER in the workspace (grep `Coalition::new|carve_for_member|propose_vote` over all "
        "crates and scripts: coalition.rs's own tests and axon-os/tests/r27_acceptance.rs only); "
        "corrigible.rs uses only the CoalitionBound type")
_LED = ("NOT A VERDICT PROPERTY (checkable): ResourceLedger::carve is called on the protected "
        "route only by journal.rs ResourceVector::carve_within (grep `\\.carve(` outside axon-os's "
        "tests)")
_GRA = ("OPERATOR-AUTHORED on the protected route (checkable): axon_os::parse_manifest's only "
        "protected caller is axon-fabric grants.rs (GrantRegistry resolution), parsing a grant "
        "file the registry names; on a protected host that registry is the operator's "
        "(protected_host grants(); a caller's registry is refused, D1), parsed only at its pin "
        "(M277), each grant file used only at the bytes it pins (M1098) and operator-owned (M278). "
        "The check")
_OSO = ("NOT ON THE PROTECTED ROUTE (checkable): Fabric enters axon-os only through "
        "supervise_requiring (= supervisor::run_requiring: approval::authorize, the AdmissionProbe "
        "runtime Fabric supplies, gate::admit, record::build), parse_manifest, scan_effects and "
        "ledger (submit.rs, grants.rs, journal.rs)")
EXEMPT += [
    # axon-attest/src/lib.rs
    (_SA, '            return Err("DeviceSet missing required device: vsock (job channel)".to_string());',
     _ATT + "; DeviceSet::validate (the VM device set axon-vm admits)"),
    (_SA, '            return Err("DeviceSet missing required device: serial (diagnostics)".to_string());',
     _ATT + "; DeviceSet::validate (the VM device set axon-vm admits)"),
    (_SA, '            return Err("DeviceSet missing required device: timer (scheduler)".to_string());',
     _ATT + "; DeviceSet::validate (the VM device set axon-vm admits)"),
    (_SA, '            other => Err(format!(',
     _ATT + "; DeviceSet::validate_manifest_extra_device"),
    (_SA, '        return Err("no attestation signature present — report is unsigned; \\',
     _ATT + "; verify_report (axon-vm's attestation report check, also behind verify_and_admit)"),
    (_SA, '    if report.measurement.digest != *expected_digest {',
     _ATT + "; verify_report (axon-vm's attestation report check, also behind verify_and_admit)"),
    (_SA, '    if report.measurement.axtcb1 != expected_axtcb1 {',
     _ATT + "; verify_report (axon-vm's attestation report check, also behind verify_and_admit)"),
    (_SA, '    if report.hw_root != SOFTWARE_TPM_HW_ROOT {',
     _ATT + "; verify_report (axon-vm's attestation report check, also behind verify_and_admit)"),
    (_SA, '        return Err(\n            "no verification key supplied — refusing to verify a signature this \\',
     _ATT + "; verify_report (axon-vm's attestation report check, also behind verify_and_admit)"),
    (_SA, '    if !constant_time_eq(&report.signature, &expected_sig) {',
     _ATT + "; verify_report (axon-vm's attestation report check, also behind verify_and_admit)"),
    (_SA, '        return Err(ComponentReadError::Missing);',
     _ATT + "; read_component_file, read only by measure_host_stack/measure_extended (axon-vm admit.rs, main.rs)"),
    (_SA, '        return Err(ComponentReadError::NotRegularFile);',
     _ATT + "; read_component_file, read only by measure_host_stack/measure_extended (axon-vm admit.rs, main.rs)"),
    (_SA, '    if !expected_axtcb1_ext.starts_with(AXTCB_EXT_PREFIX) {',
     _ATT + "; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs)"),
    (_SA, '    if !measured.axtcb1_ext.starts_with(AXTCB_EXT_PREFIX) {',
     _ATT + "; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs)"),
    (_SA, '    if measured.components.len() != 4 {',
     _ATT + "; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs)"),
    (_SA, '            return Err(VerifyError::Malformed(format!(',
     _ATT + "; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs)"),
    (_SA, '        return Err(VerifyError::MonitorSlotMismatch);',
     _ATT + "; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs)"),
    (_SA, '        return Err(VerifyError::DigestMismatch {',
     _ATT + "; verify_extended (axon-vm admit.rs check_extended_tcb, main.rs)"),
    # axon-audit/src/lib.rs
    (_SU, '        return Err(format!("expected 64 hex chars, got {}", s.len()));',
     _AUD + "; the ledger key's hex decoding"),
    (_SU, '        _ => Err(format!("not a hex digit: {b:#x}")),',
     _AUD + "; the ledger key's hex decoding"),
    (_SU, '                    Ok(_) => {',
     _AUD + "; Ledger::open_keyed"),
    (_SU, '                    Err(e) => {',
     _AUD + "; Ledger::open_keyed"),
    (_SU, '            Err(e) => {\n                return Err(format!(',
     _AUD + "; Ledger::verify_against_file"),
    (_SU, '        if entry.seq != i as u64 {',
     _AUD + "; verify_chain_keyed"),
    (_SU, '        if entry.prev_hash != expected_prev {',
     _AUD + "; verify_chain_keyed"),
    # axon-core/src/main.rs
    (CMN, '        process::exit(2);\n    }\n\n    // A test run that passes against bytes nobody locked',
     ("OPERATOR-AUTHORED on the protected route (checkable): `axon test` merges only the files on its command "
     "line, and the runner passes exactly ONE, the suite entry (axon-psv runner.rs exec_axon_test: "
     "`.arg(cfg.suite.join(&m.suite.entry))`; submit's host route likewise one file); the candidate is "
     "reached by `mod` through AXON_PATH and never merged, so a merge error (E0903) is a duplicate "
     "top-level name inside the operator's suite file, pinned at its registered version (M1061)")),
    # axon-cortex/src/generate.rs
    (_SG, '    if patch.generator_id.trim().is_empty() {',
     _CTX + "; generate::validate, the Cortex patch generators' output check (bin/cortex.rs, ai.rs)"),
    (_SG, '        return Err(GenerationFailure::Invalid(format!(\n            "empty body proposed for `{}`",',
     _CTX + "; generate::validate, the Cortex patch generators' output check (bin/cortex.rs, ai.rs)"),
    (_SG, '    if patch.body.len() > constraints.max_bytes {',
     _CTX + "; generate::validate, the Cortex patch generators' output check (bin/cortex.rs, ai.rs)"),
    (_SG, '                Err(e) => {',
     _CTX + "; CommandGenerator::propose (bin/cortex.rs `cmd:` generators)"),
    (_SG, '                        return Err(GenerationFailure::Unavailable(format!(',
     _CTX + "; CommandGenerator::propose (bin/cortex.rs `cmd:` generators)"),
    (_SG, '            return Err(GenerationFailure::Declined(format!(',
     _CTX + "; CommandGenerator::propose (bin/cortex.rs `cmd:` generators)"),
    # axon-cortex/src/runner.rs
    (_SR, '                        Observed::unknown("warnings present but none carried a `code` field")',
     _CTX + "; Runner::observe, the Cortex repair loop's observation (Runner is built by bin/cortex.rs and cortex-policy-adapter, neither a protected crate)"),
    (_SR, '                        Observed::unknown("program does not type-check; types unresolved"),',
     _CTX + "; Runner::observe, the Cortex repair loop's observation (Runner is built by bin/cortex.rs and cortex-policy-adapter, neither a protected crate)"),
    (_SR, '                    Observed::unknown(format!("checker could not run: {e}")),',
     _CTX + "; Runner::observe, the Cortex repair loop's observation (Runner is built by bin/cortex.rs and cortex-policy-adapter, neither a protected crate)"),
    (_SR, '            Err(why) => {\n                self.episode.push(EpisodeEvent::ActionDenied {',
     _CTX + "; Runner::authorize_action (the Cortex loop's typed action authority)"),
    (_SR, '            return Err(Refusal::WrongPrincipal {',
     _CTX + "; Runner::check_typed_authority, under authorize_action"),
    (_SR, '            return Err(Refusal::StaleSnapshot {',
     _CTX + "; Runner::check_typed_authority, under authorize_action"),
    (_SR, '            return Err(Refusal::PathTraversal(target_path.to_string()));',
     _CTX + "; Runner::check_typed_authority, under authorize_action"),
    (_SR, '            return Err(Refusal::PolicyFile(target_path.to_string()));',
     _CTX + "; Runner::check_typed_authority, under authorize_action"),
    (_SR, '            return Err(Refusal::PathOutsideGrant(target_path.to_string()));',
     _CTX + "; Runner::check_typed_authority, under authorize_action"),
    (_SR, '        return Err(unresolvable("not a regular file".into()));',
     _EXE + "; resolve_executable, under register_pinned/register_expected/verified_path"),
    (_SR, '        if found != want {',
     _EXE + "; register_expected, from the registry's `executors` (load) and host_executor (the local route)"),
    (_SR, '        if v.get("schema").and_then(|s| s.as_str()) != Some("cortex-check-registry/1") {',
     _REG + "; the registry's version tag"),
    (_SR, '                    return Err(format!(',
     _REG + "; a check's `visibility` (who may SEE its source; Fabric materializes the suite the same way either way)"),
    (_SR, "            if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {",
     _REG + "; and dominated: the registered workspace_version_ref is compared for equality with the reference Fabric computes by importing the suite (submit.rs check_target, M1061), which a string that is not acf1:<64 hex> never equals"),
    (_SR, '                return Err(format!("check id `{id}` registered twice"));',
     _REG + "; a repeated id in the operator's own registry (register_check keeps the later entry, the operator's choice either way)"),
    (_SR, '            Pin::Failed(r) => Err(r.clone()),',
     _LOC + "; LocalInterpreterExecutor::verified_path"),
    (_SR, '                    return Err(CheckRefusal::DigestChanged {',
     _LOC + "; LocalInterpreterExecutor::verified_path"),
    (_SR, '        if truncated {',
     _LOC + "; LocalInterpreterExecutor::run_checks' output bound"),
    (_SR, '                    return Err(std::io::Error::last_os_error());',
     _LOC + "; run_limited's PR_SET_PDEATHSIG in the local child (also an OS ERROR: the child is not exec'd)"),
    (_SR, '        return Err(std::io::Error::new(',
     _LOC + "; run_limited's wall-clock kill (a RESOURCE BOUND: no status, no report)"),
    (_SR, '                return Err(format!("fabric dispatch needs a non-empty {name}"));',
     _CTX + "; FabricSubmitExecutor::new, Cortex's client of `axon-fabric submit` (bin/cortex.rs); its operator fields are what Cortex sends, and Fabric judges the request itself"),
    (_SR, "        if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {\n            return Err(format!(",
     _CTX + "; FabricSubmitExecutor::new, Cortex's client of `axon-fabric submit` (bin/cortex.rs); its operator fields are what Cortex sends, and Fabric judges the request itself"),
    (_SR, "        if hex.bytes().all(|b| b == b'0') {",
     _CTX + "; FabricSubmitExecutor::new, Cortex's client of `axon-fabric submit` (bin/cortex.rs); its operator fields are what Cortex sends, and Fabric judges the request itself"),
    (_SR, '        if found != e.sha256 {\n            return Err(CheckRefusal::DigestChanged {',
     _CTX + "; FabricSubmitExecutor::submit_path (the pin on the axon-fabric binary Cortex runs)"),
    (_SR, '        if v.get("schema").and_then(|s| s.as_str()) != Some("axon-fabric-submit/1") {',
     _CTX + "; FabricSubmitExecutor::run_checks, Cortex's reading of a receipt Fabric already decided; the loop takes Fabric's receipt through intake, never Cortex's reading of it"),
    (_SR, '        if &rc["input_workspace_ref"] != sent || &rc["output_workspace_ref"] != sent {',
     _CTX + "; FabricSubmitExecutor::run_checks, Cortex's reading of a receipt Fabric already decided; the loop takes Fabric's receipt through intake, never Cortex's reading of it"),
    (_SR, '                return Err(std::io::Error::other(format!(',
     "NAMED ROW: M84 mutates the arm above it to `Some(_) | None => {}`, which is the removal of this arm; " + _CTX),
    # axon-fabric/src/bin/axon-custodian.rs
    (CUB, '        _ => die("usage: axon-custodian [--dev --socket P --store D | --test-config FILE]"),',
     _USE + "; no mode, so no config to serve under (the `_` arm of the mode match; M631's check_store follows the match, after a mode is chosen)"),
    # axon-os/src/coalition.rs
    (_SOC, '        if r.total_compute.saturating_add(c.compute) > self.ceiling.total_compute {',
     _COA),
    (_SOC, '        if r.total_budget.saturating_add(c.budget) > self.ceiling.total_budget {',
     _COA),
    (_SOC, '        let Some(ledger) = self.ledgers.get_mut(slot) else {',
     _COA),
    (_SOC, '        if bound_pid != claimed_pid {',
     _COA),
    (_SOC, '        if self.quorum_power_used.saturating_add(power) > self.ceiling.max_quorum_power {',
     _COA),
    # axon-os/src/ledger.rs
    (_SOL, '        if self.compute_used.saturating_add(c.compute) > self.compute_cap {',
     _LED + "; this is the journal's real spend check (the admission budget guard: a run over budget is not launched) and the compute axis the one carve_within carves; it is still not a verdict property, because its refusal is the journal's BudgetExceeded, exempt above as NOT A VERDICT PROPERTY"),
    # axon-os/src/manifest.rs
    (_SOM, '                    other => {\n                        return Err(bad(format!(\n                            "{}: reproducible must be true or false, got `{other}`",',
     _GRA + ": `reproducible` must be true/false"),
    (_SOM, '                    other => {\n                        return Err(bad(format!(\n                            "{}: require_approval must be true or false, got `{other}`",',
     _GRA + ": `require_approval` must be true/false"),
    (_SOM, '                if profile.is_some() {',
     _GRA + ": one `profile` line"),
    (_SOM, '            (sec, k) => {',
     _GRA + ": an unknown key"),
    (_SOM, '        return Err(bad("`program` must be a .ax file"));',
     _GRA + ": `program` names a .ax file (and Fabric replaces it: grants.rs manifest_for sets the program to the request's target)"),
    (_SOM, '        if pf.is_reproducible() {',
     _GRA + ": a reproducible profile contradicted by `reproducible = false` (and Fabric refuses every reproducible grant as unsupported: grant_authority grants_no_backend_can_enforce_are_unsupported_not_weakened)"),
    (_SOM, '            return Err(bad(format!("{axis}: empty path prefix")));',
     _GRA + ": a grant path prefix (validate_prefixes)"),
    (_SOM, '        if p.split([\'/\', \'\\\\\']).any(|c| c == "..") {',
     _GRA + ": a grant path prefix (validate_prefixes)"),
    (_SOM, '        Err(bad(format!("{what} must be \\u{2265} 0")))',
     _GRA + ": a budget field is non-negative (nonneg)"),
    # axon-os/src/profile.rs
    (_SOP, '            other => Err(format!(',
     _GRA + ": an unknown profile name (Profile::parse, called from manifest::parse on that file)"),
    # axon-os/src/record.rs
    (_SOR, '        if ev.seq != i as u64 {',
     _OSO + "; record::verify (a stored RunRecord's chain), whose callers are replay::replay and the `axon-os` CLI; supervise_requiring builds a record (record::build) and verifies none"),
    (_SOR, '        if ev.prev_hash != prev {',
     _OSO + "; record::verify (a stored RunRecord's chain), whose callers are replay::replay and the `axon-os` CLI; supervise_requiring builds a record (record::build) and verifies none"),
    (_SOR, '        if ev.hash != expect {',
     _OSO + "; record::verify (a stored RunRecord's chain), whose callers are replay::replay and the `axon-os` CLI; supervise_requiring builds a record (record::build) and verifies none"),
    (_SOR, '    if rec.record_digest.starts_with("axrec1:") {',
     _OSO + "; record::verify (a stored RunRecord's chain), whose callers are replay::replay and the `axon-os` CLI; supervise_requiring builds a record (record::build) and verifies none"),
    (_SOR, '    if rec.record_digest != format!("axrec2:{expect_seal}") {',
     _OSO + "; record::verify (a stored RunRecord's chain), whose callers are replay::replay and the `axon-os` CLI; supervise_requiring builds a record (record::build) and verifies none"),
    # axon-os/src/replay.rs
    (_SORP, '        Err(VerifyMismatch {',
     _OSO + "; replay::replay, called only by cli.rs cmd_replay"),
    # axon-os/src/runtime.rs
    (_SOT, '                Err(e) => return Err(e),',
     _OSO + "; StagingDir::create, used by AxonCoreRuntime (the `axon-os` binary's runtime: cli.rs); Fabric admits through its own AdmissionProbe (submit.rs), which stages nothing"),
    (_SOT, '        Err(last.unwrap_or_else(|| std::io::Error::other("staging dir collision")))',
     _OSO + "; StagingDir::create, used by AxonCoreRuntime (the `axon-os` binary's runtime: cli.rs); Fabric admits through its own AdmissionProbe (submit.rs), which stages nothing"),
    (_SOT, '            Err(_) => DeclaredEffects::unknown(), // deny-by-default',
     _OSO + "; AxonCoreRuntime::declared_effects; Fabric's AdmissionProbe implements declared_effects itself (submit.rs, the same deny-by-default unknown())"),
    # axon-psv/src/bin/axon-psv-runner.rs
    (_SPR, '        std::process::exit(3);',
     ("NOTHING TO ADMIT (checkable): start() fails either BEFORE proceed (an argument; the prctl, M313), when "
     "no verdict.json was written, so /init prints `PSV-VERDICT-INIT none` and the launcher binds no verdict "
     "(fc_linux_profile.sh: verdict-unbound, 27; Fabric M241), or when run_and_emit's ONE write of "
     "verdict.json failed, leaving no file or a strict prefix of canonical JSON, which derive refuses as "
     "malformed (psv.rs, exempt above). The exit code is a report; the verdict file is the evidence")),
    # axon-vm/src/admit.rs
    (_SVA, '                return Err(AdmitError::OverrideWidens {',
     _VM + "; resolve_effect_grant, under admit_job"),
    (_SVA, '        return Err(AdmitError::ExtendedTcbUnpinned {',
     _VM + "; check_extended_tcb, under admit_job"),
    (_SVA, '        Err(e) => Err(AdmitError::ExtendedTcbMismatch {',
     _VM + "; check_extended_tcb, under admit_job"),
    (_SVA, '        return Err(format!("kernel not found: {}", kernel_path.display()).into());',
     _VM + "; measure_and_attest_inner (admit_job, and the `axon-vm` binary's run path)"),
    (_SVA, '                return Err(\n                    "attestation failed: no pinned baseline (refusing to trust on first use)"',
     _VM + "; measure_and_attest_inner (admit_job, and the `axon-vm` binary's run path)"),
    (_SVA, '        return Err("attestation failed: kernel tampered".into());',
     _VM + "; measure_and_attest_inner (admit_job, and the `axon-vm` binary's run path)"),
    # axon-vm/src/firecracker.rs
    (_SVF, '    Err(format!(',
     _VM + "; wait_for_socket (axon-vm's own VMM driver)"),
    (_SVF, '    if !(200..300).contains(&status_code) {',
     _VM + "; fc_put (axon-vm's Firecracker API client)"),
    (_SVF, '        Err("firecracker not found in PATH or /usr/local/bin; install from github.com/firecracker-microvm/firecracker".into())',
     _VM + "; FirecrackerBin::resolve/at (axon-vm's VMM lookup)"),
    (_SVF, '        if !path.is_absolute() {',
     _VM + "; FirecrackerBin::resolve/at (axon-vm's VMM lookup)"),
    (_SVF, '            return Err(format!("firecracker not a regular file: {}", path.display()).into());',
     _VM + "; FirecrackerBin::resolve/at (axon-vm's VMM lookup)"),

]

# ── C9 round 4c, workstream GATE (amendment 74): the sites the predicate-
# primitive rule (a function that decides by bool/Option is a site), the
# changed-lines coverage rule, the `let .. else` opener and the function-
# boundary stop expose in the scanned files. Kinds as above, plus: PREDICATE OF
# NAMED ROWS (every production caller of the predicate is a refusal whose
# condition a named row disables WHOLE, a superset of weakening the
# predicate), LOOKUP (returns what a record holds under a key; every consumer's
# refusal on presence or absence is its own site, judged there), RECORDED
# FACT (the answer is a property of the input the recipe records, which the
# digest covers).
# ── amendment 74 (the Fabric side) ──
# Fork A (C9 round 4c, gate workstream): axon-fabric sites (custodian.rs excluded)
# exposed by amendment 74's rules (predicate primitives, changed-lines coverage,
# let-else opener, fn-boundary stop). Kinds as in v022_refusal_coverage.py, plus
# PREDICATE OF NAMED ROWS: every production caller of the predicate is a refusal
# whose condition a named row (or a named exemption) disables WHOLE; disabling
# the call admits everything a weakened predicate could, so that row is a
# superset of this removal.
FB = "crates/axon-fabric/src/backend.rs"
FC = "crates/axon-fabric/src/bin/axon-fabric.rs"
FR = "crates/axon-fabric/src/branches.rs"
FG = "crates/axon-fabric/src/git_data.rs"
FA = "crates/axon-fabric/src/grants.rs"
FJ = "crates/axon-fabric/src/journal.rs"
OB = "crates/axon-fabric/src/observer.rs"
PL = "crates/axon-fabric/src/privileged_launcher.rs"
PH = "crates/axon-fabric/src/protected_host.rs"
FP = "crates/axon-fabric/src/provenance.rs"
PSVF = "crates/axon-fabric/src/psv.rs"
RD = "crates/axon-fabric/src/readiness.rs"
SE = "crates/axon-fabric/src/sealed_exec.rs"
FN = "crates/axon-fabric/src/signing.rs"
FS = "crates/axon-fabric/src/submit.rs"
FW = "crates/axon-fabric/src/workspace.rs"

_USE = "USAGE: a missing or malformed argument; the command runs, signs and writes nothing"
_JNV_SHORT = ("NOT A VERDICT PROPERTY (checkable): a journal state transition, budget, settlement or "
              "recovery rule; the journal's intent and outcome are read back only by submit::replayed "
              "(never signed: M01/M402), the status CLI (prints) and status/cancel's registry-sha check "
              "(M279); a fresh run's receipt, class and ran_under are built in memory in submit")
_WALK = ("PREDICATE OF NAMED ROWS: it selects only the BASE a peer authority root's ownership walk "
         "starts from (`owned_from` of exclusive_root_keys); its `None` skips that walk, and M466 "
         "(`check_owned_chain(base, peer, true)?` -> `let _ = ...`, ACTIVE, killed) removes the walk "
         "for every caller, a superset of this removal. The root itself is walked by the caller "
         "(check_operator_owned when operator_owned)")
EXEMPT += [
    # ── backend.rs ──
    (FB, "            self.operator_owned.then_some(Path::new(\"/\")),", _WALK),
    (FB, "fn operator_walk(a: TrustAuthority, dir: &Path) -> Option<&'static Path> {", _WALK),
    (FB, "    (dir == a.operator_dir()).then_some(Path::new(\"/\"))", _WALK),
    (FB, "pub(crate) fn hex_decode(s: &str) -> Option<Vec<u8>> {",
     "FAILS CLOSED: its `None` drops a trust-root key (`filter_map` in trusted_issuers, readiness's "
     "root read) or only changes a displayed fingerprint (exclusive_root_keys' `fp`); a decode that "
     "admitted a malformed string would add bytes no signature verifies under. keys_in admits only "
     "64-hex entries (operator_trust exemption)"),
    (FB, "fn is_hex64(v: &serde_json::Value) -> bool {",
     "PREDICATE OF NAMED ROWS: its only callers are RULE:engine-digests (M1032, ACTIVE) and "
     "RULE:engine-pin (M1038, retired with its four-cell record), each of which disables the whole "
     "condition this predicate feeds"),
    (FB, "fn non_empty(v: &serde_json::Value) -> Option<String> {",
     "PREDICATE OF NAMED ROWS: its value feeds RULE:waiver-reason (M1029), RULE:host (M574) and "
     "RULE:caveat (M1033), each of which removes the whole emptiness refusal; for a waiver's "
     "assertion name, a blank name excuses no BLOCKED assertion (names are matched exactly)"),
    (FB, "pub fn parse_utc(s: &str) -> Option<i64> {",
     "OPERATOR-AUTHORED (signed): every string it reads is a field of a document whose signature "
     "under an operator-rooted key the caller verified first (the observation's observed_at: "
     "verify_observation; the B263 record's end and its waivers' expires: verify_detached / the "
     "verified waiver file; the certification record's certified_at: M335), and its `None` is "
     "NOTHING TO ADMIT (no time to judge freshness or order with)"),
    (FB, "            .then(|| t.parse().ok())",
     "as parse_utc above (this is its digit check, inside it)"),
    # ── bin/axon-fabric.rs ──
    (FC, '    refuse(\n        "usage",\n        &format!(\n            "--grant-registry is not accepted',
     "NOT A SITE: the body of refuse_caller_grant_registry; its uses are M274 (and M273, retired)"),
    (FC, '    let op = OperationId::new(a.req("--op")).unwrap_or_else(|e| refuse("usage", &e.to_string(), 2));',
     _USE),
    (FC, "    fn opt(&self, flag: &str) -> Option<String> {",
     "USAGE (argv lookup): it returns the caller's own argument; any answer it could give is an "
     "argv the caller could have passed, and every flag's value is judged by the code after it "
     "(a test-trust flag is refused in a production build, the protected-host config is the "
     "operator's)"),
    (FC, "    fn has(&self, flag: &str) -> bool {", "as `opt` above (argv lookup)"),
    (FC, "fn signer(registry: &std::path::Path) -> Option<(axon_loop_contracts::OpaqueRef, Vec<u8>)> {",
     "DEVELOPMENT ROUTE (checkable): its only call is the no-protected-host arm (`None => "
     "signer(&registry_path)`); a protected host's signer is host_signer, from the operator's host "
     "config (O1). Its `None` (the registry names no signer) signs nothing"),
    # ── branches.rs ──
    (FR, "    pub fn is_cancelled(&self, exp: &TaskId, arm: &ArmId) -> bool {",
     "PREDICATE OF NAMED ROWS: its production callers are submit's cancelled-branch refusal (M1066, "
     "ACTIVE, which disables the whole condition) and Branches' own approve, which is not on the "
     "protected route (`grep -rn 'Branches' crates/*/src`: submit calls only open, branch_of_run "
     "and is_cancelled)"),
    # ── git_data.rs ──
    (FG, "fn allowed_key(key: &str) -> bool {",
     "PREDICATE OF NAMED ROWS: its only caller is the repository-config refusal M450 (ACTIVE), "
     "which disables the whole condition"),
    (FG, "pub fn worktree_differs(top: &Path, tree: &Entries) -> Option<String> {",
     "PREDICATE OF NAMED ROWS: its only caller is tree_differs, whose two production results are "
     "dropped whole by M290 (readiness, ACTIVE) and M451 (provenance's head_bytes_differ, which "
     "calls tree_differs), each a superset of this removal"),
    (FG, "    pub fn covers_tracked(&self, tree: &Entries) -> Option<String> {",
     "PREDICATE OF NAMED ROWS: its only caller is tree_differs's allowlist refusal, which M503 "
     "(`.filter(|_| false)` on this call, ACTIVE) removes whole"),
    (FG, "pub fn is_oid(s: &str) -> bool {",
     "PREDICATE OF NAMED ROWS / FAILS CLOSED: on the protected lineage its call is M1084's "
     "(provenance.rs, ACTIVE); in git_data its uses only select an object NAME that is then read by "
     "hash from the object store (a name that is not 40 hex names no object, so the read refuses), "
     "or are exempt sites of their own (the parent walk: NO OUTCOME; the disambiguation filter: "
     "NOTHING TO ADMIT)"),
    # ── grants.rs ──
    (FA, "    pub fn require_approval(&self) -> bool {",
     "NO PRODUCTION CALLER (checkable): `grep -rn 'require_approval()' crates/` finds no call; "
     "axon-os reads the manifest field itself"),
    (FA, "pub fn restricts_effects(g: &Grant) -> bool {",
     "PREDICATE OF NAMED ROWS: its only caller sets AuthorityNeeds::guest_policy_channel "
     "(submit.rs), consumed only by select's x1 refusal M1049 (ACTIVE), which disables the whole "
     "condition"),
    (FA, "pub fn is_path_scoped(g: &Grant) -> bool {",
     "PREDICATE OF NAMED ROWS: its only caller sets AuthorityNeeds::path_scoped_grant (submit.rs), "
     "consumed only by select's x2 refusals M1050 and M1053 (ACTIVE), each removing the whole "
     "condition"),
    # ── journal.rs ──
    (FJ, "        let torn_at = (report.torn_tail_bytes > 0).then_some(good_len);", _JNV_SHORT),
    (FJ, "    pub fn is_terminal(self) -> bool {", _JNV_SHORT),
    (FJ, "    pub fn disputed(&self) -> bool {", _JNV_SHORT + " (a disputed op's held liability)"),
    (FJ, "    pub fn view(&self, op: &OperationId) -> Option<OpView> {",
     "NOTHING TO ADMIT: a lookup; `None` is the operation being absent from the journal (status / "
     "cancel then refuse unknown_op; submit takes a fresh run)"),
    (FJ, "    pub fn is_empty(&self) -> bool {",
     "NO PRODUCTION CALLER (checkable): clippy's len_without_is_empty companion of len(); `grep -rn "
     "'journal.*is_empty()\\|j\\.is_empty()' crates/*/src` finds no call on a Journal"),
    # ── observer.rs ──
    (OB, "            self.operator_owned.then_some(Path::new(\"/\")),", _WALK),
    # ── privileged_launcher.rs ──
    (PL, '            return Err(bad(\n                "custodian.sha256 must pin the axon-custodian program',
     "A CONDITION A NAMED ROW MUTATES: this arm is the complement of the two above; M1485 (`None if "
     "a.test` -> `None`, ACTIVE) is the removal for an unpinned custodian, and a pin that is not 64 "
     "hex is OPERATOR-AUTHORED (a field of the operator-owned helper config, M585/M586)"),
    (PL, "fn is_hex64(s: &str) -> bool {",
     "PREDICATE OF NAMED ROWS / OPERATOR-AUTHORED: its callers judge fields of the operator-owned "
     "helper config (program pins, the observer key: exempt, M585/M586; the custodian pin: M1485), "
     "pins of the operator-owned profile manifest that verify_inputs then hashes against (M590), "
     "and the request's manifest digest (M796, retired with its four-cell record)"),
    (PL, "fn plain_name(s: &OsStr) -> bool {",
     "PREDICATE OF NAMED ROWS: its only caller is the out-path refusal M532 (ACTIVE), which "
     "removes the whole condition"),
    (PL, "fn exit_code(s: std::io::Result<std::process::ExitStatus>) -> Option<i32> {",
     "NOTHING TO ADMIT: `None` is a child that could not be waited for or was killed by a signal, "
     "so there is no exit status; every consumer (interpret_linux_result) reads None as Unknown, and "
     "a mutation could only invent a status the child never returned"),
    (PL, "pub fn no_new_privs() -> bool {",
     "SELECTS NOTHING: it chooses only which message setuid_honoured's refusal gives (it refuses in "
     "both branches) and a diagnostic field of the probe report"),
    # ── protected_host.rs ──
    (PH, '        Err(e) => Err(format!(\n            "{}: {e}: cannot tell whether this is a protected host',
     "NAMED ROW: M766 widens the NotFound arm above to every Err (`Err(_) => Ok(false)`, ACTIVE), "
     "which makes this arm unreachable: the same removal"),
    # ── provenance.rs ──
    (FP, '                .then(|| format!("{g} rewrites ancestry"))',
     "NAMED ROW: M413 (ACTIVE) mutates the condition this `.then` converts (`.is_ok()` on the line "
     "above -> `.is_ok_and(|_| false)`), the same removal"),
    # ── psv.rs ──
    (PSVF, "    pub fn of_receipt(r: &axon_loop_contracts::ExecutionReceipt) -> Option<EvidenceClass> {",
     "PREDICATE OF NAMED ROWS: at signing its only use is observed_launch, whose call is M320 "
     "(ACTIVE); at submit it decides only whether Fabric attaches a psv bundle to its own derived "
     "receipt, and the loop's intake judges the receipt's class itself (protected_evidence: one "
     "class ref, M212's arm)"),
    # ── readiness.rs ──
    (RD, "fn is_hex(v: &Value, n: usize) -> bool {",
     "OPERATOR-AUTHORED (signed): its only callers are the format checks on the operator-signed "
     "record (exempt: each field is also compared for equality with a computed digest or resolved, "
     "M335 signs the bytes)"),
    (RD, "fn one_ref<'a>(rc: &'a axon_loop_contracts::ExecutionReceipt, prefix: &str) -> Option<&'a str> {",
     "PREDICATE OF NAMED ROWS: its only caller is the receipt-to-record join M742 (ACTIVE), which "
     "removes the whole join (the other use only formats the refusal)"),
    # ── sealed_exec.rs ──
    (SE, '    if Path::new("/etc/axon/TEST-no-read-lease").exists() {',
     "NON-PRODUCTION: compiled only under cfg(feature = \"test-trust-root\") (the line above); M709 "
     "(ACTIVE) is the row that compiles it into a production build"),
    (SE, "    pub fn leased(&self) -> bool {",
     "NO PRODUCTION CALLER (checkable): `grep -rn '\\.leased()' crates/*/src` finds only "
     "sealed_exec.rs's unit tests"),
    # ── signing.rs ──
    (FN, "    let Some(r) = ran_under else {\n        return Err(KEY_REACHABLE);",
     "NOTHING TO ADMIT: no RanUnder, so there is no backend, class or effect ceiling to judge the "
     "signature by"),
    (FN, "fn observed_launch(r: &ExecutionReceipt) -> bool {",
     "PREDICATE OF NAMED ROWS: its only caller is execution_attestation_decision's refusal M320 "
     "(ACTIVE), which removes the whole condition"),
    # ── submit.rs ──
    (FS, "            if resume.is_some() {\n                journal.cancel(",
     "NAMED ROW: M1066 (ACTIVE) mutates this refusal's condition (`if branches.is_cancelled(..)`, "
     "the enclosing `if` above the inner one), the same removal"),
    (FS, "                ran_under: v.launched.then(|| ran_under_of(&v.intent)).flatten(),", _JNV_SHORT),
    (FS, "fn ran_under_of(intent: &crate::journal::Intent) -> Option<RanUnder> {", _JNV_SHORT),
    (FS, "    fn scan_source(&self) -> Option<String> {",
     "FLAGGED, DEFENCE IN DEPTH: the source axon-os's admission probe scans for declared effects "
     "(its `None` makes the probe scan the entry file alone); the program's effects are bounded at "
     "run time by the same grant's ceiling (guest policy: protected_policy_ceiling M671-M675; host "
     "AXON_ALLOWED_EFFECTS), and the verdict does not rest on the scan. No test drives a candidate "
     "module's effect through the admission scan"),
    (FS, "    fn candidate_dir(&self) -> Option<PathBuf> {",
     "PREDICATE OF NAMED ROWS: its `None` drops the candidate from sealing (submit's "
     "`with_sealed_dir`, whose removal is M80, ACTIVE) and from the admission scan (scan_source, "
     "FLAGGED above); module_path uses it only for a Legacy bound (no suite)"),
    # ── workspace.rs ──
    (FW, "    pub fn contains(&self, r: &Acf1Ref) -> bool {",
     "SELECTS NOTHING THE LOAD DOES NOT RE-DECIDE: a `true` for an unpublished ref leads to load(r), "
     "which refuses NotPublished, a manifest that does not hash to r (M1083) and a tree that does not "
     "re-derive to r (M1082); a `false` only refuses (submit) or imports bytes whose ref is "
     "re-derived from what was stored"),
    (FW, "fn parse_manifest(m: &[u8]) -> Option<Vec<WorkspaceManifestEntry>> {",
     "NOTHING TO ADMIT / NAMED ROWS: the bytes it parses already hash to the requested reference "
     "(M1083), and tree() re-verifies every blob against its entry and the tree against r (M1082); a "
     "malformed manifest has no entries to materialize"),
    (FW, "    (workspace_manifest_bytes(&out) == m).then_some(out)", "as parse_manifest above"),
]

# ── amendment 74 (the loop and its contracts) ──
# Fork B (C9 round 4c, gate, amendment 74): coverage decisions for the loop and
# loop-contracts sites the new rule exposes. Kinds as in the gate, plus:
# PREDICATE OF NAMED ROWS (every production caller of the predicate is a
# refusal a named row removes, or a site the gate judges on its own), LOOKUP
# (returns the event/field the record holds under a key; None = nothing is
# recorded; every consumer's refusal on presence or absence is its own site in
# a scanned file, judged there).
LA = "crates/axon-loop/src/admission.rs"
LI = "crates/axon-loop/src/intake.rs"
EV = "crates/axon-loop/src/evl.rs"
EVO = "crates/axon-loop/src/evo.rs"
LG = "crates/axon-loop/src/ledger.rs"
PLN = "crates/axon-loop/src/plan.rs"
PTR = "crates/axon-loop/src/pointer.rs"
PRC = "crates/axon-loop/src/price.rs"
RU = "crates/axon-loop/src/rules.rs"
SA = "crates/axon-loop/src/safety.rs"
ST = "crates/axon-loop/src/store.rs"
TEL = "crates/axon-loop/src/tel.rs"
AT = "crates/axon-loop-contracts/src/attestation.rs"
CT = "crates/axon-loop-contracts/src/canonical.rs"
CI = "crates/axon-loop-contracts/src/ids.rs"
OT = "crates/axon-loop-contracts/src/operator_trust.rs"
PEV = "crates/axon-loop-contracts/src/protected_evidence.rs"
CS = "crates/axon-loop-contracts/src/schema.rs"
_LOOKUP = ("LOOKUP: returns what the ledger recorded under this key (None = nothing recorded); it "
           "decides nothing itself: every consumer's refusal on presence or absence is a refusal "
           "site of its own in a scanned file, judged there. Consumers (grep `{name}(` in "
           "crates/*/src): {callers}")
_NPC = ("NO PRODUCTION CALLER (checkable): `grep -rn '{pat}' crates/*/src` finds no caller outside "
        "{where}")
EXEMPT += [
    # ── contracts ──
    (AT, "pub fn key_id_of_hex(public_key_hex: &str) -> Option<String> {",
     "OPERATOR-AUTHORED, FAILS CLOSED: the fingerprint of an operator-registered key (config "
     "verifier/observer/monitor keys, an operator root); None when the text is not a 32-byte hex "
     "key. Every consumer either compares `key_id_of_hex(k) == Some(id)` (admission.rs key_now, "
     "protected_evidence.rs's observer filter M470, readiness.rs's verifier root) or records the id of "
     "a key a signature was just verified under (safety.rs, evl.rs); a None matches no id. A signature "
     "is always verified under the key bytes decoded again by verify/verify_document (M02/M951)"),
    (AT, "pub fn issued_ms(doc: &Value) -> Option<u64> {",
     _LOOKUP.format(name="issued_ms", callers="attestation.rs verify (exempt: NOTHING TO ADMIT, no "
                    "signing time) and evl.rs's `an authenticated attestation states no issued_ms` "
                    "(NOTHING TO ADMIT); the value read is signed over (M02) and compared (M03)")),
    (AT, "fn unhex(s: &str) -> Option<Vec<u8>> {",
     "NOTHING TO ADMIT: decodes an operator-registered key; its callers (key_id_of_hex, verify, "
     "verify_document) refuse a None (exempt there: the registered key is not a key, nothing to verify "
     "under); bytes it decoded wrongly are a key the signature must verify under (M02/M951), so a "
     "mis-decode only fails closed"),
    (CT, "fn negative_zero_to_zero(text: &str) -> Option<String> {",
     "SELECTS NOTHING: None means there is no `-0` to rewrite, and its one caller (parse_value) then "
     "parses the text as given (`normalized.as_deref().unwrap_or(json)`); it refuses nothing, and what "
     "it returns is parsed strictly and checked by json_tree like any input"),
    (CT, "    let value = serde_json::to_value(v).map_err(|e| shape(e.to_string()))?;",
     "NOTHING TO ADMIT: the contract value does not serialize, so there are no canonical bytes to "
     "digest"),
    (CI, '                    shape(format!(concat!(stringify!($name), " {:?}: {}"), s, why))',
     "NAMED ROW: M1228 discards this map_err's Err and its `?` (`let _ = $check(&s)...`, the lines "
     "around it), the same removal: every validated string is then admitted unchecked"),
    (CI, "    fn eq(&self, other: &Acf1Ref) -> bool {",
     "NOT A DECISION OF ITS OWN: the string equality of two validated references (what a derived "
     "PartialEq is); each comparison's refusal is a site of its own where it is made"),
    (OT, "    pub fn parse(s: &str) -> Option<TrustAuthority> {",
     "USAGE: its one production caller is axon-fabric's `--authority` argument "
     "(bin/axon-fabric.rs authority_flag), which refuses a None with exit 2 (a usage refusal, a site "
     "of its own); it names which operator root a keygen/sign command addresses, and that root is "
     "still owner-checked (M948-M950)"),
    (OT, "fn hex_bytes(s: &str) -> Option<Vec<u8>> {",
     "OPERATOR-AUTHORED: decodes a key file of an operator root the ownership walk holds root-owned "
     "and unwritable (M948-M950); a malformed entry is dropped and equals no presented key, so it only "
     "fails closed (as the malformed-entry exemption in root_keys_hex)"),
    (PEV, "pub fn claims_protected(rc: &ExecutionReceipt) -> bool {",
     "PREDICATE OF NAMED ROWS: its production callers are admission.rs reverify_protected "
     "(`if !claims_protected(rc)` refusal, M255, ACTIVE) and intake.rs's operator-rooted key "
     "requirement (`if claims_protected(&rc)`, M205, ACTIVE); each row removes the decision this "
     "predicate feeds"),
    (PEV, "pub fn is_sha256_hex(d: &str) -> bool {",
     "PREDICATE OF NAMED ROWS: its production callers are names_every_digest (`if !is_sha256_hex(&d)`, "
     "M473, ACTIVE) and fabric psv.rs's digest check (a site of its own in a scanned file)"),
    (PEV, "fn one_ref<'a>(rc: &'a ExecutionReceipt, prefix: &str) -> Option<&'a str> {",
     _LOOKUP.format(name="one_ref", callers="check_bundle's `want` (`no single {p} ref`, then compared "
                    "by M232/M233/M299/M296-M298/M381/M382)") .replace("the ledger", "the receipt")),
    (PEV, "        (counted, claimed) => {\n            return Err(format!(\n                \"the guest verdict claims",
     "NAMED ROW: M304 (`(RV::Passed, _)`) and M779 (`(RV::Failed, _)`) widen the accepting arm on the "
     "line above, which makes this arm unreachable for each counted outcome: the same removal"),
    (CS, "fn usize_kw(s: &Map<String, Value>, k: &str) -> Option<usize> {",
     "COMPILED-IN: reads a keyword of a checked-in schema (minLength/maxLength/minItems/maxItems); "
     "redteam.rs every_checked_in_schema_is_within_the_supported_subset asserts each schema's "
     "keywords; a None only means the schema states no such bound"),
    (CS, "fn as_i128(n: &serde_json::Number) -> Option<i128> {",
     "NOTHING TO ADMIT: a number that is no integer has no integer value; its caller number_rules "
     "refuses it (`non-integer number`) and the schema bounds read through it are compiled in"),
    # ── loop ──
    (LA, "fn clearance_verifies(",
     "PREDICATE OF NAMED ROWS: its one caller is the clearance condition at admission (M264 replaces "
     "the call with `signature_ref.is_some() | true`, ACTIVE; M245 the rooted-key leg)"),
    (EV, "fn is_d12(ep: &LoopEpisode) -> bool {",
     "SELECTS ONLY WHICH REFUSAL: its one caller (evaluate) routes a D12 episode (MiCode's not-produced "
     "markers) to the D12 branch, which refuses delivered execution documents (M129) and is never "
     "counted (M123, the d12 arm), and every other episode to the branch that requires its execution "
     "documents; a non-D12 episode cannot carry the markers' digests as its execution refs and also "
     "deliver documents digesting to them, so neither route can be entered wrongly to count"),
    (EV, '            return Err(refused(format!("trials[{i}]: trial delivered twice")));',
     "NAMED ROW: M961 inserts `&& false` into this refusal's condition on the line above (`.is_some()` "
     "... `{`), the same removal"),
    (EV, "                    refused(format!(\n                        \"trial {} was delivered and requested but never issued",
     "NAMED ROW: M1375 replaces the `ok_or_else` this refusal is the body of with a fallback to the "
     "delivered attempt (the line above), the same removal"),
    (EV, "                        Err(e) => unknown(\n                            UnknownKind::Unbound,\n                            format!(\n                            \"context not admissible",
     "NAMED ROW: M1000 makes the matched value `ctx_check.or(Ok(()))` on the line above, so this arm is "
     "never taken: the same removal"),
    (EV, "        let Some((areq, rcpt, _)) = &d.acf else {",
     "NOTHING TO ADMIT: a D12 trial has no execution request or receipt, and every protected leg after "
     "this reads them; the kind it reports is M130's and the protected branch's condition M11's"),
    (EVO, "pub(crate) fn proposer_in(tx: &Tx, scope: &Scope, candidate: &Ref) -> Option<OpaqueRef> {",
     _LOOKUP.format(name="proposer_in", callers="pointer.rs (M1323), intake.rs subjects (M05), "
                    "plan.rs freeze (M1011), evl.rs subjects (M37/M38), admission.rs proposer (M114), "
                    "safety.rs subjects").replace("the ledger", "the EVO hypothesis record")),
    (LI, "pub fn cost_micro_from_micro_cents(micro_cents: Option<u64>) -> Option<u64> {",
     "SELECTS NOTHING: a unit conversion that is None exactly when its input is None (unknown stays "
     "unknown); its one production caller compares the result with the episode's cost (`c != want`, "
     "M846)"),
    (LI, "pub fn micode_not_run_reason(v: &axon_loop_contracts::EpisodeVerification) -> Option<&'static str> {",
     "PREDICATE OF NAMED ROWS: its callers are intake's uncited-verification refusal (M132 replaces "
     "the call with `false`, ACTIVE) and EVL's Unknown kind (M133-M136), which only name the kind of a "
     "non-success"),
    (LG, "fn mac_eq(a: &str, b: &str) -> bool {",
     "PREDICATE OF NAMED ROWS: its callers are the keyed entry and head authentication; M1300 (entry) "
     "and M1302 (head) make the arm a wrong MAC falls to `Ok(())`, the same removal (both ACTIVE)"),
    (LG, "pub fn set_fault_hook(h: fn(&'static str)) -> bool {",
     _NPC.format(pat="set_fault_hook", where="its definition (its one caller is tests/pointer.rs, a "
                 "fault-injection hook; the bool says only whether the hook was installed)")),
    (LG, "    fn last_transition(&self, scope: &Scope) -> Option<(u64, PointerRecord)> {",
     _LOOKUP.format(name="last_transition", callers="the projection consistency check in load "
                    "(M1315/M1316) and the pointer of record")),
    (LG, "    pub fn is_revoked(&self, scope: &Scope, policy: &Ref) -> bool {",
     "PREDICATE OF NAMED ROWS: its refusing callers are rowed: pointer.rs incumbent-of-record (M1324), "
     "resolve (M1325), activation (M1333), intake.rs (M848), all ACTIVE; its remaining caller "
     "(pointer.rs revoke, `if !is_revoked`) is idempotence: a revocation is not appended twice"),
    (LG, "    pub fn freeze_of(&self, experiment_id: &str) -> Option<(u64, &Event)> {",
     _LOOKUP.format(name="freeze_of", callers="plan.rs register (M1006) and plan.rs's frozen-plan read")),
    (LG, "    pub fn latest_registration(&self, experiment_id: &str) -> Option<&Event> {",
     _LOOKUP.format(name="latest_registration", callers="plan.rs register and freeze")),
    (LG, "    pub fn admission_event(&self, admission_ref: &Ref) -> Option<(u64, &Event)> {",
     _LOOKUP.format(name="admission_event", callers="admission.rs (M826) and admission.rs's journalled "
                    "check")),
    (LG, "    pub fn evaluation_event(&self, evaluation_ref: &Ref) -> Option<(u64, &Event)> {",
     _LOOKUP.format(name="evaluation_event", callers="evl.rs (M975) and evl.rs's journalled check")),
    (LG, "    pub fn assignment_of(&self, experiment_id: &str) -> Option<(u64, Ref)> {",
     _LOOKUP.format(name="assignment_of", callers="evl.rs evaluate (exempt: NOTHING TO ADMIT) and "
                    "plan.rs assignment issue (M1018)")),
    (LG, "    pub fn baseline_of(&self, scope: &Scope) -> Option<(Ref, Ref)> {",
     _LOOKUP.format(name="baseline_of", callers="pointer.rs designation (M1321) and activation "
                    "(M1340/M1341)")),
    (LG, "    pub fn candidate_set_event(&self, scope: &Scope, r: &Ref) -> bool {",
     "PREDICATE OF NAMED ROWS: its refusing caller is candidates.rs resolve (M993, ACTIVE); its other "
     "caller (register) only skips a repeat registration whose resolve also succeeds"),
    (LG, "    pub fn task_manifest_event(&self, scope: &Scope, r: &Ref) -> bool {",
     "PREDICATE OF NAMED ROWS: its refusing caller is tasks.rs resolve (M988, ACTIVE); its other "
     "caller (register) only skips a repeat registration whose resolve also succeeds"),
    (PLN, "        if existing == r {\n            return Ok(r);\n        }\n        return Err(refused(format!(\n            \"experiment {} already has its assignment",
     "NAMED ROW: M1018 filters a differing journalled assignment out of the `if let` above "
     "(`.filter(|(_, e)| e == &r)`), so this refusal is never reached: the same removal"),
    (PTR, "                let hint = if t.kind == TransitionKind::Rollback {",
     "NAMED ROW: M1333 disables this refusal's condition (`if false && tx.is_revoked(scope, &target)`) "
     "on the line above; the `if` here only chooses the message"),
    (PRC, "    pub fn covers(&self, c: Coverage) -> bool {",
     _NPC.format(pat=r"\.covers(", where="its definition")),
    (RU, "fn canonical_u64(s: &str) -> Option<u64> {",
     _NPC.format(pat="canonical_u64(", where="rules.rs's own unit tests")),
    (SA, "    pub fn is_unknown(&self) -> bool {",
     _NPC.format(pat="is_unknown()", where="other types' methods of the same name (axon-core Span "
                 "sources, axon-cortex); no SafetyState value calls it in crates/*/src")),
    (ST, "pub(crate) fn decode_hex(s: &str) -> Option<Vec<u8>> {",
     "OPERATOR-AUTHORED: its one caller decodes AXON_LOOP_LEDGER_KEY from the operator's environment "
     "(LedgerKey::from_env), which refuses a None (usage, exit 2), never a fall back to unkeyed"),
    (ST, "    pub fn ledger_key(&self) -> Option<&LedgerKey> {",
     "ACCESSOR of the operator-configured key (from_env); None means unkeyed. Its consumers are the "
     "ledger's authentication (M1300-M1303: a keyed ledger is never read without its key)"),
    (TEL, "    pub fn single_total(&self) -> Option<&Total> {",
     "NAMED ROW: M117 (ACTIVE) makes every arm whose by_currency is not exactly one INCONCLUSIVE at "
     "admission whatever total is stated; its one caller (admission.rs facts) reads None as an "
     "Unresolved total holding every currency's liability"),
]

# ── amendment 74 (the custodian, the interpreter seal edges, the guest, the PSV crate and the recipe) ──
CU = "crates/axon-fabric/src/custodian.rs"
CINT = "crates/axon-core/src/interp.rs"
CRS = "crates/axon-core/src/resolver.rs"
CF = "crates/axon-core/src/interp/conform.rs"
GIN = "crates/axon-guest-init/src/main.rs"
GKM = "crates/axon-guest-kernel/src/mmds.rs"
GKP = "crates/axon-guest-kernel/src/mmds_parse.rs"
PS = "crates/axon-psv/src/lib.rs"
RUN = "crates/axon-psv/src/runner.rs"
WRC = "crates/axon-workspace-recipe/src/lib.rs"
_BAREMETAL = ("NOT ON THE PROTECTED ROUTE (checkable): the bare-metal guest kernel is the `axon` backend of "
              "scripts/build-guest-image.sh (AXON_KERNEL_BACKEND=axon, the default of a DEMO image) and "
              "Fabric's AXON_KERNEL profile (backend.rs: guest_kind axon_kernel_demo, `job_kinds: &[]`, it "
              "runs no program). The protected profile, linux-microvm-protected, boots the Linux backend "
              "(vmlinux + rootfs.sqfs, /init = axon-guest-init), whose policy decisions are scanned in "
              "crates/axon-guest-init and crates/axon-psv")
EXEMPT += [
    # ── custodian.rs ──
    (CU, "    pub fn parse(s: &str) -> Option<Mode> {",
     "SELECTS NOTHING: its callers are the reply check (exempt above: a reply authored by the "
     "operator-installed custodian, authenticated by uid, M626, and program pin, M1483) and issue/spend's "
     "`unwrap_or(Mode::Dev)`: a mode that does not parse is Dev, which launches nothing protected "
     "(custodian_mode_launches accepts Protected, or Test in a test authority)"),
    (CU, "fn plain_absolute(p: &Path) -> bool {",
     "PREDICATE OF AN EXEMPT SITE: its one caller is CustodianConfig::check's path rule, exempt above as "
     "OPERATOR-AUTHORED (the operator-owned custodian.json; on the protected route the socket must EQUAL "
     "the path systemd bound, M703)"),
    (CU, "fn is_hex(s: &str, n: usize) -> bool {",
     "SELECTS NOTHING: a shape check of (a) the nonce the authenticated custodian issued (Fabric only "
     "hands it back to the same custodian, whose store spends only a nonce it recorded as issued: the "
     "atomic rename of its `.issued` record, observer.rs exemptions) and (b) the manifest digest a "
     "spend names, which is written to the `.used` audit record only (it decides nothing)"),
    (CU, "fn pidfd_pid(pidfd: &std::os::fd::OwnedFd) -> Option<i64> {",
     "NOTHING TO ADMIT: its `None` is a sender that has exited (the kernel reports Pid: -1 for a dead "
     "pidfd, and no fdinfo for a closed one): there is no process to identify, and the open of "
     "/proc/<pid>/exe for a pid that is no process fails (OS ERROR) before any hash"),
    (CU, "    (pid > 0).then_some(pid)", "as pidfd_pid above (its last line)"),
    # ── core ──
    (CINT, "    pub(crate) fn seal_type(&self, name: &str) -> bool {",
     "PREDICATE OF A NAMED ROW: its one caller is eval.rs's struct-construction provenance "
     "(`self.frame_sealed.get() || self.seal_type(name)`), which M97 (ACTIVE) mutates to drop this call, "
     "the same removal"),
    (CINT, "    pub(crate) fn is_global(&self, name: &str) -> bool {",
     "PREDICATE OF NAMED ROWS: it decides only whether eval.rs's index fast path is taken (existence, no "
     "value). Its one caller is that fast path, whose value read is `global_ref` (M2472 mutates it to a raw "
     "read); when it answers false the generic path evaluates the receiver through the identifier arm, "
     "which reads through `global_ref` too (M2470). Neither answer reaches a value without the edge"),
    (CF, "            let Some(vd) = ed.variants.iter().find(|x| x.name == *variant) else {",
     "NOTHING TO ADMIT: the declared enum has no such variant, so there are no declared field types to "
     "cast the payload's fields at (the casts below read `vd`)"),
    (CF, "            tparams: (!generics.is_empty()).then(|| Rc::new(generics.clone())),",
     "SELECTS NOTHING: an owner with no type parameters gets no type-parameter table; the field is then "
     "cast at its declared type exactly as with an empty one"),
    # ── guest-init ──
    (GIN, "    fn constrains_anything(&self) -> bool {",
     "PREDICATE OF NAMED ROWS: its refusing caller on the protected (cmdline) route is the "
     "constrains-nothing refusal M1640 (ACTIVE, which disables the whole condition); its other caller is "
     "the MMDS arm (NOT ON THE PROTECTED ROUTE: the protected VM has no network)"),
    (GIN, "    fn has_labels(&self) -> bool {",
     "SELECTS NOTHING: it chooses only which message the constrains-nothing refusal gives; both arms "
     "refuse (M1640)"),
    (GIN, 'fn allow_unpoliced() -> bool {\n    env::var(',
     "NON-PRODUCTION: compiled only with the non-default cargo feature `dev-allow-no-policy`, which "
     "build-guest-image.sh never enables (DEFAULT FEATURES ONLY, and it checks the image's init with "
     "`strings` for the variable's name); the production body is the constant rowed by M1733"),
    # ── guest-kernel (the bare-metal demo backend) ──
    (GKM, "    pub fn contains(self, other: EffectSet) -> bool {", _BAREMETAL),
    (GKP, "fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {", _BAREMETAL),
    (GKP, "fn json_str_field<'a>(json: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {", _BAREMETAL),
    (GKP, "fn json_u64_field(json: &[u8], key: &[u8]) -> Option<u64> {", _BAREMETAL),
    (GKP, "fn top_level_value<'a>(json: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {", _BAREMETAL),
    # ── psv ──
    (PS, "fn mode_is_normalised(dir: bool, exec: bool, mode: u32) -> bool {",
     "PREDICATE OF NAMED ROWS: its two callers are the directory and file mode refusals M317 and M318 "
     "(ACTIVE), each disabling the whole condition"),
    (RUN, "pub fn policy_from_cmdline(cmdline: &str) -> Option<Vec<u8>> {",
     "PREDICATE OF NAMED ROWS: its value is the policy the runner holds, whose digest is joined to the "
     "launch manifest's policy_sha256 before anything runs (M671, ACTIVE) and recorded in the verdict, "
     "where Fabric (M676) and the loop (M677) join it again; a None is the digest of no policy, which no "
     "manifest names"),
    (RUN, "fn is_identifier(s: &str) -> bool {",
     "PREDICATE OF A NAMED ROW: its one caller is the test-name refusal M169 (ACTIVE), which disables "
     "the whole condition"),
    # ── workspace-recipe ──
    (WRC, "    pub fn from_mode(m: &str) -> Option<EntryKind> {",
     "PREDICATE OF NAMED ROWS: its callers are Fabric's workspace parse_manifest (exempt: the bytes it "
     "parses hash to the requested reference, M1083, and tree() re-derives the reference from every "
     "entry, M1082) and an `expect` on a manifest that already parsed"),
    (WRC, "pub fn is_exec(m: &std::fs::Metadata) -> bool {",
     "RECORDED FACT: it reads the file's own exec bit, which the recipe RECORDS (the version digest "
     "covers it); no input chooses the answer but the file's mode, and the guest's mode rule (M318) "
     "compares that mode with the recorded bit"),
    (WRC, "pub fn is_exec(_m: &std::fs::Metadata) -> bool {",
     "NON-UNIX: compiled out on the only supported platform (cfg(not(unix)))"),
]

# Amendment 74: bin/axon-loop.rs is SCANNED, no longer OUT_OF_SCOPE. Its reason
# ("every verb hands its one document to a library function ... refusals are
# usage errors") was false: `tel summarize` decides two things in the CLI
# itself (the request schema, and G10: Fabric attempts need a pinned price
# schedule: M1736, M1737). Its other sites are usage errors and the exit.
ALB = "crates/axon-loop/src/bin/axon-loop.rs"
EXEMPT += [
    (ALB, '                return Err(LoopError::Usage(format!("--{k} given twice")));',
     "USAGE: a flag given twice; the verb runs, signs and writes nothing"),
    (ALB, '            Some(k) => Err(LoopError::Usage(format!("unexpected --{k}"))),',
     "USAGE: a flag the verb does not take; the verb runs, signs and writes nothing"),
    (ALB, '        _ => Err(LoopError::Usage(format!(\n            "unknown verb {:?}',
     "USAGE: an unknown verb; nothing runs"),
    (ALB, '            std::process::exit(e.exit_code());',
     "RELAY: exits with the code of the error the verb already returned; it decides nothing"),
]


# ── C9 round 4c, INTEGRATE (amendment 74 x 75): the sites amendment 74's rules (predicate
# primitives, the whole-file test-module fix) newly expose in files the crate rule brought in
# or the observer added. Each is judged by its callers on the protected route; the five
# that decide on the route and carry no row are NOT_YET_SCANNED, handed back (see
# amendment 74's integration note).
EXEMPT += [
    ('crates/axon-attest/src/lib.rs',
     'fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {',
     _ATT + "; constant_time_eq, used only by axon-attest's own verify functions"),
    ('crates/axon-audit/src/lib.rs',
     '    fn from_str(s: &str) -> Option<EffectKind> {',
     _AUD + "; from_str (a ledger-file reader/helper)"),
    ('crates/axon-audit/src/lib.rs',
     '    pub fn is_empty(&self) -> bool {',
     _AUD + "; is_empty (a ledger-file reader/helper)"),
    ('crates/axon-cortex/src/action.rs',
     '    pub fn write_target(&self) -> Option<&str> {',
     _CTX + "; write_target"),
    ('crates/axon-cortex/src/action.rs',
     '    pub fn requires_write_authority(&self) -> bool {',
     _CTX + "; requires_write_authority"),
    ('crates/axon-cortex/src/episode.rs',
     '    pub fn verified_ok(&self) -> bool {',
     _CTX + "; verified_ok"),
    ('crates/axon-cortex/src/lib.rs',
     '    pub fn value(&self) -> Option<&T> {',
     _CTX + "; value"),
    ('crates/axon-cortex/src/lib.rs',
     '    pub fn is_unknown(&self) -> bool {',
     _CTX + "; is_unknown"),
    ('crates/axon-cortex/src/locate.rs',
     '                .then(|| name.to_string())',
     _CTX + "; defined_fns"),
    ('crates/axon-cortex/src/locate.rs',
     "fn fn_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {",
     _CTX + "; fn_body"),
    ('crates/axon-cortex/src/runner.rs',
     'fn symbol_body(src: &str, symbol: &str) -> Option<(usize, usize)> {',
     _CTX + "; symbol_body"),
    ('crates/axon-cortex/src/runner.rs',
     '    pub fn verify(&mut self, claimed_done: bool, hidden_check: &str, rel_path: &str) -> bool {',
     _CTX + "; verify"),
    ('crates/axon-cortex/src/runner.rs',
     'fn verdict_for(name: &str, failed: &[String], passed: &[String]) -> bool {',
     _CTX + "; verdict_for"),
    ('crates/axon-cortex/src/runner.rs',
     'fn observed_compiles(obs: &Observation) -> Option<bool> {',
     _CTX + "; observed_compiles"),
    ('crates/axon-cortex/src/runner.rs',
     '    pub fn check(&self, id: &str) -> Option<&RegisteredCheck> {',
     "LOOKUP: CheckRegistry::check returns the registered check for an id (None = unregistered); its one protected caller is submit.rs `cfg.registry.check(id).ok_or_else(Unregistered)`, whose refusal on absence is that site's own, judged there"),
    ('crates/axon-cortex/src/select.rs',
     '    pub fn action(&self) -> Option<&CortexAction> {',
     _CTX + "; action"),
    ('crates/axon-cortex/src/select.rs',
     "fn fact<'a>(obs: &'a Observation, key: &str) -> Option<&'a Observed<String>> {",
     _CTX + "; fact"),
    ('crates/axon-fabric/src/observer_service.rs',
     '        operator.then_some(Path::new("/")),',
     "NON-PRODUCTION (checkable): `operator.then_some(Path::new(\"/\"))` picks the base of the peer-root ownership walk; the protected observer passes `operator = true` (`mode == Mode::Protected`), and Mode::Test/Dev are reached only through the TEST_TRUST_BUILD-gated `--test-config` arm (M1548); the walk itself is M466's"),
    ('crates/axon-fabric/src/observer_service.rs',
     'fn plain_absolute(p: &Path) -> bool {',
     "OPERATOR-AUTHORED on the protected route (checkable): plain_absolute judges the socket, store and key paths of /etc/axon/observer.json, an operator-owned file read through the walked operator route (load_config); a path it wrongly accepts is the operator's own"),
    ('crates/axon-fabric/src/observer_service.rs',
     'fn is_hex(s: &str, n: usize) -> bool {',
     "NOT A VERDICT PROPERTY (checkable): is_hex judges the observation nonce's shape before the observer records it; a nonce it wrongly accepts is only recorded, and the nonce is spent by the custodian (M628/M640), never decided here; the observation itself is signed only over what the observer measured (M1531-M1539)"),
    ('crates/axon-loop-contracts/src/attestation.rs',
     'pub fn is_canonical_public_key_hex(s: &str) -> bool {',
     "PREDICATE OF NAMED ROWS: its one production caller is axon-loop store.rs `if !..is_canonical_public_key_hex(k)`, a refusal whose condition M1582 disables whole, a superset of weakening this predicate"),
    ('crates/axon-os/src/cli.rs',
     'fn profile_divergence_note(job: &Path, m: &crate::manifest::JobManifest) -> Option<String> {',
     _OSO + "; profile_divergence_note, called only by cli.rs cmd_run (the `axon-os` binary)"),
    ('crates/axon-os/src/coalition.rs',
     '    pub fn member_pid(&self, slot: usize) -> Option<&str> {',
     _COA + "; Coalition::member_pid has no caller at all"),
    ('crates/axon-os/src/corrigible.rs',
     'pub fn check_kill(state: LatchState, reason: &str) -> Option<Verdict> {',
     _OSO + "; corrigible::check_kill has no non-test caller (grep `check_kill(`: corrigible.rs's own tests only)"),
    ('crates/axon-os/src/corrigible.rs',
     'pub fn r27_tcb_modules_present() -> bool {',
     _OSO + "; corrigible::r27_tcb_modules_present has no non-test caller (grep: corrigible.rs's own tests only)"),
    ('crates/axon-os/src/grant.rs',
     '    pub fn parse(s: &str) -> Option<ExecPolicy> {',
     _OSO + "; parse"),
    ('crates/axon-os/src/grant.rs',
     '    pub fn parse(s: &str) -> Option<Label> {',
     _OSO + "; parse"),
    ('crates/axon-os/src/grant.rs',
     '    pub fn allows(&self, needed: &EffectSet) -> bool {',
     _OSO + "; Grant::allows has no non-test caller (grep `\\.allows(`: grant.rs's own tests only)"),
    ('crates/axon-os/src/grant.rs',
     '    pub fn is_subset_of(&self, other: &Grant) -> bool {',
     _OSO + "; Grant::is_subset_of has no non-test caller (grep `is_subset_of(`: tests in grant.rs and cli.rs only)"),
    ('crates/axon-os/src/grant.rs',
     '    pub fn subset_of(&self, other: &EffectSet) -> bool {',
     _OSO + "; EffectSet::subset_of is called only by Grant::allows (no non-test caller)"),
    ('crates/axon-os/src/grant.rs',
     'fn prefixes_within(a: &[String], b: &[String]) -> bool {',
     _OSO + "; called only by Grant::is_subset_of (no non-test caller)"),
    ('crates/axon-os/src/grant.rs',
     'fn hosts_within(a: &[String], b: &[String]) -> bool {',
     _OSO + "; called only by Grant::is_subset_of (no non-test caller)"),
    ('crates/axon-os/src/killchan.rs',
     '    pub fn is_tripped(&self) -> bool {',
     _OSO + "; is_tripped has no non-test caller (grep `is_tripped(`: latch.rs's own test only)"),
    ('crates/axon-os/src/latch.rs',
     '    pub fn is_tripped(&self) -> bool {',
     _OSO + "; is_tripped has no non-test caller (grep `is_tripped(`: latch.rs's own test only)"),
    ('crates/axon-os/src/ledger.rs',
     '    pub fn would_exceed(&self, c: Carve) -> bool {',
     _OSO + "; ResourceLedger::would_exceed has no caller at all (carve does its own comparison, ledger.rs:74-90)"),
    ('crates/axon-os/src/manifest.rs',
     'fn parse_str(val: &str) -> Option<String> {',
     _GRA + "; a scalar of the grant file's string syntax"),
    ('crates/axon-os/src/manifest.rs',
     'fn parse_int(val: &str) -> Option<i64> {',
     _GRA + "; a scalar of the grant file's integer syntax"),
    ('crates/axon-os/src/manifest.rs',
     'fn parse_arr(val: &str) -> Option<Vec<String>> {',
     _GRA + "; a scalar of the grant file's array syntax"),
    ('crates/axon-os/src/monitor.rs',
     '    fn is_allowed(&self, effect: &str) -> bool {',
     _OSO + "; Monitor::is_allowed, called only inside monitor.rs's own R29 compliance monitor, which Fabric never constructs (the axon_os names Fabric uses are listed above)"),
    ('crates/axon-os/src/profile.rs',
     '        self.is_reproducible().then_some("0:1")',
     _OSO + "; Profile::virtual_clock, called only by AxonCoreRuntime (runtime.rs: the `axon-os` binary's runtime)"),
    ('crates/axon-os/src/profile.rs',
     '    pub fn is_reproducible(self) -> bool {',
     _GRA + "; Profile::is_reproducible, read by parse_manifest for the `profile` line of the operator's grant file"),
    ('crates/axon-os/src/profile.rs',
     "    pub fn virtual_clock(self) -> Option<&'static str> {",
     _OSO + "; Profile::virtual_clock, called only by AxonCoreRuntime (runtime.rs: the `axon-os` binary's runtime)"),
    ('crates/axon-os/src/runtime.rs',
     'fn is_kill_file_tripped(path: &std::path::Path) -> bool {',
     _OSO + "; called only by AxonCoreRuntime (the `axon-os` binary's runtime; Fabric supplies its own AdmissionProbe)"),
    ('crates/axon-os/src/runtime.rs',
     'fn ran_to_completion(stdout: &str, nonce: &str) -> bool {',
     _OSO + "; called only by AxonCoreRuntime (the `axon-os` binary's runtime; Fabric supplies its own AdmissionProbe)"),
    ('crates/axon-vm/src/firecracker.rs',
     '    pub fn satisfies_protected_linux_microvm(&self) -> bool {',
     _VM + "; satisfies_protected_linux_microvm"),
    ('crates/axon-vm/src/firecracker.rs',
     '    pub fn ok(&self) -> bool {',
     _VM + "; ok"),
    ('crates/axon-vm/src/firecracker.rs',
     'pub fn parse_guest_sentinel(line: &str) -> Option<GuestOutcome> {',
     _VM + "; parse_guest_sentinel"),
    ('crates/axon-vm/src/firecracker.rs',
     '    fn handle(&self, request: &str) -> Option<String> {',
     _VM + "; handle"),
    ('crates/axon-vm/src/firecracker.rs',
     'fn bind_vsock_uds(uds_path: &Path) -> Option<std::os::unix::net::UnixListener> {',
     _VM + "; bind_vsock_uds"),
]

# ── Amendment 76 (C9 round 4c, admit): the verdict-form sites in axon-os and the
# files the integrator's round 2 left. Each carries the call-graph fact that
# makes it not a decision on the protected route; the ones that DO decide are
# rows (M1770-M1829, v022_g01_mutations.py "ADMIT").
_KIL = ("NOT ON THE PROTECTED ROUTE (checkable): axon-os's kill channel, latch, corrigibility and "
        "compliance-monitor modules have no caller on the supervise_requiring path or in any "
        "protected crate: `grep -rn 'killchan::\\|latch::\\|corrigible::\\|monitor::\\|FileKillChannel\\|"
        "ComplianceMonitor\\|LatchState' crates/*/src` outside axon-core's interpreter finds only "
        "these modules' own files and axon-os cli.rs (the `axon-os run` command's monitor thread); "
        "supervisor.rs and gate.rs name none of them, and the AdmissionProbe Fabric supplies has no "
        "latch. `axon_os::cli` is named by no other crate")
_GRN = ("DOMINATED BY CONSTRUCTION (checkable): on the protected route Grant::intersect runs once, "
        "supervisor.rs `manifest.grant.intersect(supervisor_grant)`, where `supervisor_grant` is "
        "submit.rs `grant.grant()` and `manifest.grant` is `grant.manifest_for(..).grant`, a clone "
        "of that same grant (tests/admit_route.rs admission_intersects_the_resolved_grant_with_"
        "itself). Every element intersect_prefixes/intersect_hosts emit is a clone of an input "
        "element, so the result is a subset of the job's own grant whatever these predicates "
        "answer: they can only NARROW, and a withheld axis is a denial (fail closed). The result's "
        "only consumers are gate::admit's effect_set() (an axis is present iff its list is "
        "non-empty) and max_label, and a self-intersection keeps every non-empty list non-empty "
        "(is_ancestor(p, p), host_matches(h, h) and `b.contains(h)` hold for every entry), so "
        "nothing widens and nothing narrows. `is_subset_of` and `allows` are called by no "
        "protected crate (`grep -rn 'is_subset_of\\|\\.allows(' crates/*/src` finds only axon-os "
        "and axon-intent)")
_ACR = ("NOT ON THE PROTECTED ROUTE (checkable): AxonCoreRuntime is axon-os's own subprocess "
        "Runtime for `axon-os run`/`replay` (its only constructions are cli.rs "
        "`AxonCoreRuntime::from_env()`: `grep -rn AxonCoreRuntime crates/*/src`), and MockRuntime "
        "exists only under `#[cfg(any(test, feature = \"mock\"))]`, a feature no dependent turns "
        "on (axon-fabric and axon-intent depend on axon-os by bare path). Fabric supplies its own "
        "AdmissionProbe (submit.rs), which performs no effect and returns Completed{0}; the "
        "verdicts here are what a RUN produced, and no protected run is executed by these")
_GRB = (_GRA + " refuses a malformed grant file here (a missing or ill-typed key); the file is "
        "never caller-supplied, and nothing chooses success: the manifest is not produced")
EXEMPT += [
    ('crates/axon-os/src/killchan.rs',
     '            LatchState::Tripped\n        } else {\n            LatchState::Clear\n        }\n    }\n}\n\n// ── Test implementation ────────────────────────────────────────────────────────',
     _KIL),
    ('crates/axon-os/src/killchan.rs',
     '            LatchState::Tripped\n        } else {\n            LatchState::Clear\n        }\n    }\n}\n\n// ── File-backed kill channel (for cross-process kill) ─────────────────────────',
     _KIL),
    ('crates/axon-os/src/killchan.rs',
     '                LatchState::Tripped',
     _KIL),
    ('crates/axon-os/src/killchan.rs',
     '            Err(_) => LatchState::Tripped,',
     _KIL),
    ('crates/axon-os/src/killchan.rs',
     '    fn poll(&self) -> LatchState {\n        if self.flag.load(Ordering::SeqCst) {\n            LatchState::Tripped\n        } else {\n            LatchState::Clear\n        }\n    }\n}\n\n// ── Test implementation ────────────────────────────────────────────────────────',
     _KIL),
    ('crates/axon-os/src/killchan.rs',
     '    fn poll(&self) -> LatchState {\n        if self.flag.load(Ordering::SeqCst) {\n            LatchState::Tripped\n        } else {\n            LatchState::Clear\n        }\n    }\n}\n\n// ── File-backed kill channel (for cross-process kill) ─────────────────────────',
     _KIL),
    ('crates/axon-os/src/killchan.rs',
     '    fn poll(&self) -> LatchState {\n        match std::fs::read_to_string(&self.path) {',
     _KIL),
    ('crates/axon-os/src/latch.rs',
     '            state: LatchState::Tripped,',
     _KIL),
    ('crates/axon-os/src/latch.rs',
     '    pub fn poll(&self) -> LatchState {',
     _KIL),
    ('crates/axon-os/src/corrigible.rs',
     '        LatchState::Tripped => Some(Verdict::Halted {',
     _KIL),
    ('crates/axon-os/src/monitor.rs',
     '                            return MonitorResult::ViolationDetected {',
     _KIL),
    ('crates/axon-os/src/monitor.rs',
     '                        return MonitorResult::ViolationDetected {\n                            effect,',
     _KIL),
    ('crates/axon-os/src/monitor.rs',
     '    pub fn run(self) -> MonitorResult {',
     _KIL),
    ('crates/axon-os/src/cli.rs',
     '                crate::verdict::Verdict::VerifyMismatch { detail: e.detail }.exit_code() as u8,\n            )\n        }\n    }\n}\n\nfn cmd_replay(rest: &[&str]) -> ExitCode {',
     _KIL),
    ('crates/axon-os/src/cli.rs',
     '                crate::verdict::Verdict::VerifyMismatch { detail: e.detail }.exit_code() as u8,\n            )\n        }\n    }\n}\n\n// ── R27: kill / status ───────────────────────────────────────────────────────',
     _KIL),
    ('crates/axon-os/src/grant.rs',
     'fn is_ancestor(prefix: &str, path: &str) -> bool {',
     _GRN),
    ('crates/axon-os/src/grant.rs',
     'fn host_allows(list: &[String], host: &str) -> bool {',
     _GRN),
    ('crates/axon-os/src/grant.rs',
     'fn host_matches(pat: &str, host: &str) -> bool {',
     _GRN),
    ('crates/axon-os/src/runtime.rs',
     '                    verdict: Verdict::Denied {\n                        reason: format!("cannot read program: {e}"),',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    verdict: Verdict::Denied {\n                        reason: format!("cannot create private staging dir: {e}"),',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                        verdict: Verdict::Denied {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    verdict: Verdict::Denied {\n                        reason: format!("could not launch interpreter: {e}"),',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '            Verdict::Halted {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '            Verdict::Denied {\n                reason: format!("timed out after {} ms", self.timeout.as_millis()),',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '            Verdict::Denied {\n                reason: first_axon_line(err, "runtime capability/sandbox violation"),',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '            Verdict::BudgetExhausted {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '            Verdict::RefineViolation {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '            Verdict::Denied {\n                reason: first_axon_line(err, "interpreter panic"),',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '            Verdict::Denied {\n                reason: first_axon_line(err, "AI policy refused the call"),',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '            Verdict::Malformed {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    0 => Verdict::Denied {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    2 => Verdict::Malformed {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    3 => Verdict::Denied {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    4 => Verdict::Halted {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    5 => Verdict::Denied {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    6 => Verdict::RefineViolation {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    7 => Verdict::BudgetExhausted {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    8 => Verdict::Denied {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '                    other => Verdict::Denied {',
     _ACR),
    ('crates/axon-os/src/runtime.rs',
     '    fn run_sandboxed(\n        &self,\n        _program: &Path,',
     _ACR),
    ('crates/axon-fabric/src/journal.rs',
     '                v.state = OpState::Failed;',
     _JNV),
    ('crates/axon-fabric/src/journal.rs',
     '            self.append(Rec::OutcomeUnknown {',
     _JNV),
    ('crates/axon-fabric/src/journal.rs',
     '        self.append(Rec::Failed {',
     _JNV),
    ('crates/axon-fabric/src/journal.rs',
     '        self.append(Rec::Cancelled {',
     _JNV),
    ('crates/axon-vm/src/firecracker.rs',
     '        return Some(GuestOutcome::Violation);',
     _VM),
    ('crates/axon-vm/src/firecracker.rs',
     '                Some(GuestOutcome::Violation) => (8, GuestOutcome::Violation),',
     _VM),
    ('crates/axon-os/src/manifest.rs',
     '            .ok_or_else(|| bad(format!("line {}: expected `key = value`", lineno + 1)))?;',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '            ("", "program") => program = Some(parse_str(val).ok_or_else(|| bad(where_()))?),',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '            ("", "intent") => intent = Some(parse_str(val).ok_or_else(|| bad(where_()))?),',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '                        .ok_or_else(|| bad(where_()))?',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '                            bad(format!("{}: seed must be a non-negative u64", where_()))',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '            ("grant", "fs_read") => fs_read = Some(parse_arr(val).ok_or_else(|| bad(where_()))?),',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '            ("grant", "fs_write") => fs_write = Some(parse_arr(val).ok_or_else(|| bad(where_()))?),',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '            ("grant", "net") => net = Some(parse_arr(val).ok_or_else(|| bad(where_()))?),',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '                let s = parse_str(val).ok_or_else(|| bad(where_()))?;\n                exec = Some(ExecPolicy::parse(&s).ok_or_else(|| {',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '                    bad(format!("{}: exec must be \\"none\\" or \\"any\\"", where_()))',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '                let s = parse_str(val).ok_or_else(|| bad(where_()))?;\n                max_label = Some(Label::parse(&s).ok_or_else(|| {',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '                    bad(format!(\n                        "{}: max_label must be public|internal|secret",',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '            ("grant.budget", "calls") => calls = Some(parse_int(val).ok_or_else(|| bad(where_()))?),',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '                tokens = Some(parse_int(val).ok_or_else(|| bad(where_()))?)',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '                cost_micro = Some(parse_int(val).ok_or_else(|| bad(where_()))?)',
     _GRB),
    ('crates/axon-os/src/manifest.rs',
     '    let max_label = max_label.ok_or_else(|| bad("missing `grant.max_label`"))?;',
     _GRB),
]

_RING = ("DOMINATED BY THE VERIFIER (checkable): the 32-byte key / 64-byte signature filter only "
         "pre-screens what ring's Ed25519 `UnparsedPublicKey::verify` refuses anyway: a key or "
         "signature of any other length is an Err there (tests/ring_length_facts.rs "
         "ring_refuses_a_key_or_signature_of_any_other_length), and every caller reaches that "
         "verify, or the registered-key equality (`presented != registered`) and, in "
         "operator_trust, the trusted-issuer membership, before it can return Ok. Removing the "
         "filter changes the refusal's REASON, never the verdict")
_ALD = ("NO PRODUCTION CALLER (checkable): `grep -rn 'admission::load' crates/*/src` finds no caller; "
        "`load` is `store.get_record(\"admissions\", r)`, a lookup that decides nothing (every "
        "consumer of a stored admission re-derives it: admission::rederive)")
EXEMPT += [
    ('crates/axon-loop-contracts/src/attestation.rs',
     '        .ok_or_else(|| {\n            shape(format!(\n                "the key registered for verifier {issuer_ref} is not a 64-hex Ed25519 public key"',
     _RING),
    ('crates/axon-loop-contracts/src/attestation.rs',
     '        .ok_or_else(|| {\n            shape(format!(\n                "the key registered for {issuer_ref} is not a 64-hex Ed25519 public key"',
     _RING),
    ('crates/axon-loop-contracts/src/operator_trust.rs',
     '        .ok_or(format!("{what} signature has no 32-byte public_key"))?;',
     _RING),
    ('crates/axon-loop-contracts/src/operator_trust.rs',
     '        .ok_or(format!("{what} signature has no 64-byte signature"))?;',
     _RING),
    ('crates/axon-loop/src/admission.rs',
     'pub fn load(store: &Store, r: &Ref) -> Result<AdmissionRecord> {',
     _ALD),
]

_VRD = ("PREDICATE OF NAMED ROWS (checkable): CheckReport::verdict's one protected consumer is "
        "axon-fabric psv.rs `derive` (`match report.verdict(test)`; grep `\\.verdict(` crates/*/src), "
        "which takes a Passed only after the completion token under the launch's key (M183) and "
        "exactly one keyed result line (M312, M1720-M1722), a Failed only with keyed failure "
        "evidence and a non-zero exit (M239, M1721), and refuses NotRun (M775). Whatever "
        "`verdict` answers, a count needs those, so it can only relabel one fail-closed refusal "
        "as another: a failing or absent test read as Passed has no completion token, and a "
        "passing one read as Failed or NotRun is not a pass (cortex tests/ and psv_dispatch "
        "a_failing_test_is_failed_whatever_the_guest_claims, M185)")
EXEMPT += [
    ('crates/axon-cortex/src/runner.rs',
     '                        return ExecOutcome::Failed(format!("cannot read {}: {e}", target.path))',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                    Err(e) => ExecOutcome::Failed(format!("check could not run: {e}")),',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                        return ExecOutcome::Failed(format!("cannot read {}: {e}", symbol.path))',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                    return ExecOutcome::Failed(format!("cannot write {}: {e}", symbol.path));',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                    return EpisodeOutcome::Blocked {\n                        steps: step,\n                        reason: format!("cannot snapshot {}: {e}", target.path),',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                    return EpisodeOutcome::Blocked {\n                        steps: step,\n                        reason,',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                        return EpisodeOutcome::Refused {',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                    return EpisodeOutcome::Refused {\n                        steps: step,\n                        reason: format!(',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                    return EpisodeOutcome::Refused {\n                        steps: step,\n                        reason: why.to_string(),',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                    (true, false) => VisibleCheck::Failed,',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                            return EpisodeOutcome::Blocked {\n                                steps: step,\n                                reason: format!(\n                                    "a rejected patch could not be undone in {}",',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                    return EpisodeOutcome::Blocked {\n                        steps: step,\n                        reason: format!("the action could not be carried out: {why}"),',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                        return EpisodeOutcome::Blocked {\n                            steps: step,\n                            reason: format!(\n                                "a patch broke the build and {} could not be restored: {e}",',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                            return EpisodeOutcome::Blocked {\n                                steps: step,\n                                reason: format!("the file could not be re-checked: {e}"),',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '                        return EpisodeOutcome::Blocked {\n                            steps: step,\n                            reason: format!(\n                                "a rejected patch could not be undone in {}",',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '        EpisodeOutcome::BudgetExhausted { steps: budget }',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '            .ok_or_else(|| {',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     "    pub fn execute(&mut self, auth: Authorized<'_>) -> ExecOutcome {",
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '    pub fn run_episode(',
     _CTX),
    ('crates/axon-cortex/src/runner.rs',
     '            CheckVerdict::Failed',
     _VRD),
    ('crates/axon-cortex/src/runner.rs',
     '    pub fn verdict(&self, name: &str) -> CheckVerdict {',
     _VRD),
    ('crates/axon-cortex/src/runner.rs',
     '                Err(r) => Pin::Failed(r),',
     _LOC),
    ('crates/axon-cortex/src/select.rs',
     '            return Selection::Blocked(format!(',
     _CTX),
    ('crates/axon-cortex/src/select.rs',
     '            return Selection::Blocked(\n                "cannot choose an action: the observation carries no `compiles` fact".to_string(),',
     _CTX),
    ('crates/axon-cortex/src/select.rs',
     '        Some(Observed::Unknown { reason }) => Selection::Blocked(format!(',
     _CTX),
    ('crates/axon-cortex/src/select.rs',
     '        None => Selection::Blocked(',
     _CTX),
    ('crates/axon-cortex/src/select.rs',
     'pub fn select_action(obs: &Observation, target: &SymbolRef) -> Selection {',
     _CTX),
    ('crates/axon-cortex/src/select.rs',
     'pub fn select_action_with(',
     _CTX),
]

_DFL = ("COMPLEMENT (checkable): the default arm of interpret_linux_result's final `match exit`; the "
        "only arm that can return Ok is `Some(0) | Some(10)`, entered through the guards rowed as "
        "M1815-M1820 (schema, cleanup, output_bound, --verify-result, workload_exit, exit/workload "
        "agreement), so every other exit is no success by the match's exhaustiveness: this arm is "
        "what is left when no guarded arm applies")
_KND = ("REFINES THE KIND OF A NON-SUCCESS (checkable): removing this arm sends the exit to the "
        "match's default arm, which is no success either (only exit 0 or 10 reach the Ok-capable "
        "arm, guarded by M1815-M1820); what the arm decides is WHICH non-success (Refused, TimedOut, "
        "WorkloadFailed or OutcomeUnknown), and linux_receipt journals every one of them "
        "`journal.fail(.., Billing::Unknown)` with the launch's liability kept, so no input chooses "
        "success. The mapping of each kind to a receipt status is rowed (M1821-M1823)")
_RPT = ("RE-REPORTED (checkable): the receipt records a refusal another site decided (backend::select "
        "for Unsupported, rows M1044-M1054; supervisor_admits for Denied, rows M1785-M1797; the launch "
        "manifest builder; the executor's own Err); this line only writes it into the receipt of a "
        "run that never produced a verdict, and nothing after it can read a success from it "
        "(the arm returns the receipt it builds)")
_RCP = ("DOMINATED BY NAMED ROWS (checkable): a Failed read as Passed at this line is demoted to "
        "Unknown by the completion check that follows (submit.rs `passed without completion "
        "evidence`, rows M63, M64): a failing test body never returns, so it holds no token under "
        "the run's key, and the key is always Some on this route (`with_completion_key` at the one "
        "call site). Measured at this commit: the edit that reads a failing named check as Passed "
        "(and the unnamed one) was REFUSED_ELSEWHERE by that demotion, verification Unknown, never "
        "Passed; those two rows are therefore not kept (tests/submit.rs "
        "a_failing_check_is_never_receipted_passed pins the receipt)")
_APR = ("NOT A REFUSAL (checkable): AdmissionProbe::run_sandboxed is reached only after gate::admit "
        "admitted (supervisor.rs step 5) and states success by construction, so that "
        "`supervisor_admits` reads Completed; any other value could only refuse. It performs no "
        "effect")
_CNO = ("DOMINATED BY NAMED ROWS (checkable): the nonce is read from the custodian's reply, whose "
        "writer is one process (M1727), and the custodian issues 32-hex nonces; its spend then goes "
        "through the custodian again, which spends only a nonce recorded as issued (M196, M622), so "
        "a malformed nonce could not authorize a launch whatever this filter says")
_GIT1 = ("RESOURCE BOUND (checkable): a 2^30-byte cap on a git object's declared size; removing it "
         "only lets a larger object be read, whose bytes are then re-hashed to the name it was asked "
         "for (the one-read digest check, M289), so it decides no verdict")
_GIT2 = ("DOMINATED (checkable): the value is used as the NEXT object's name (`o.read(&target, ..)?` "
         "on the following loop iteration), and a git read of a name that is not an object id is "
         "refused there; the filter only moves that refusal one step earlier")
_PHP = ("OPERATOR-AUTHORED on the protected route (checkable): a pin field of the operator-owned host "
        "config (protected_host::load, read only from the operator's path, operator-owned: M141-M143); "
        "a malformed pin equals no digest, so every comparison with it refuses")
_RPF = ("PREDICATE OF NAMED ROWS (checkable): report_for's first reading of the guest's output is "
        "re-decided by the next statement in `run` (runner.rs `match (status, keyed_outcome, "
        "exit_code)`), which maps everything but (Passed, Some(true), Some(0)) and (Failed, "
        "Some(false), non-zero) to Unknown (M1813, with keyed_outcome M1720-M1722); report_for is "
        "called by no other production code (`grep -rn report_for crates/*/src`), so its mapping can "
        "only choose which fail-closed value precedes that re-check")
EXEMPT += [
    ('crates/axon-fabric/src/backend.rs',
     '            LinuxOutcome::Unknown,\n            format!("launcher exited {exit:?} with no readable result.json"),',
     _NTA),
    ('crates/axon-fabric/src/backend.rs',
     '                outcome: LinuxOutcome::Unknown,',
     _NTA),
    ('crates/axon-fabric/src/backend.rs',
     '            LinuxOutcome::Unknown,\n            format!("launcher exit {other:?} ({status}): the VMM ended without a bound result"),',
     _DFL),
    ('crates/axon-fabric/src/backend.rs',
     '                LinuxOutcome::Refused,',
     _KND),
    ('crates/axon-fabric/src/backend.rs',
     '                LinuxOutcome::Unknown,\n                format!(',
     _KND),
    ('crates/axon-fabric/src/backend.rs',
     '            LinuxOutcome::TimedOut,',
     _KND),
    ('crates/axon-fabric/src/backend.rs',
     '                    LinuxOutcome::Unknown,\n                    format!("launcher exit {exit:?} disagrees with workload_exit {w}"),',
     _KND),
    ('crates/axon-fabric/src/backend.rs',
     '                outcome: LinuxOutcome::Refused,',
     _KND),
    ('crates/axon-fabric/src/submit.rs',
     '                    status: ReceiptStatus::Unsupported,',
     _RPT),
    ('crates/axon-fabric/src/submit.rs',
     '                    status: ReceiptStatus::Denied,',
     _RPT),
    ('crates/axon-fabric/src/submit.rs',
     '                                status: ReceiptStatus::Failed,',
     _RPT),
    ('crates/axon-fabric/src/submit.rs',
     '                        verification: ReceiptVerification::Unknown,\n                        matched: None,\n                        // The suite that was running is still recorded: an\n                        // honest "unknown" names what it was unknown about.\n                        evidence: suite.clone().map(opaque).into_iter().collect(),\n                        liability_micro: liability,\n                        output,',
     _RPT),
    ('crates/axon-fabric/src/submit.rs',
     '                        ReceiptVerification::Failed\n                    } else {',
     _RCP),
    ('crates/axon-fabric/src/submit.rs',
     '                        ReceiptVerification::Failed\n                    };',
     _RCP),
    ('crates/axon-fabric/src/submit.rs',
     '            ReceiptStatus::OutcomeUnknown,\n            "reconciled after restart: launched with no terminal record",',
     _JNV),
    ('crates/axon-fabric/src/submit.rs',
     '        OpState::Cancelled => (ReceiptStatus::Canceled, "cancelled"),',
     _JNV),
    ('crates/axon-fabric/src/submit.rs',
     '        OpState::Failed => (ReceiptStatus::Failed, "failed"),',
     _JNV),
    ('crates/axon-fabric/src/submit.rs',
     '            ReceiptStatus::OutcomeUnknown,\n            "completed but no receipt recorded",',
     _JNV),
    ('crates/axon-fabric/src/submit.rs',
     '            ReceiptStatus::OutcomeUnknown,\n            "in flight (another submit owns it)",',
     _JNV),
    ('crates/axon-fabric/src/submit.rs',
     '                ReceiptVerification::Unknown',
     _JNV),
    ('crates/axon-fabric/src/submit.rs',
     '    fn run_sandboxed(',
     _APR),
    ('crates/axon-fabric/src/custodian.rs',
     '            .ok_or("custodian issued no well-formed nonce")?;',
     _CNO),
    ('crates/axon-fabric/src/git_data.rs',
     '        .ok_or(format!(\n            "object {oid} is not a {want} in this repository\'s object store (a missing object \\',
     _GIT1),
    ('crates/axon-fabric/src/git_data.rs',
     '                    .ok_or(format!("tag {target} names no object"))?;',
     _GIT2),
    ('crates/axon-fabric/src/protected_host.rs',
     '                .ok_or_else(|| bad(format!("{ptr} is not a sha256")))',
     _PHP),
    ('crates/axon-psv/src/runner.rs',
     '        (1, 0, 1) => GuestStatus::Failed,',
     _RPF),
    ('crates/axon-psv/src/runner.rs',
     '        _ => GuestStatus::Unknown,\n    };\n    (status, report)',
     _RPF),
]


# ── Amendment 81 (C9 round 4c, eqgate): the sites the open-flag, Some(reason),
# Result<bool>/i32/ExitCode forms expose. Every entry states a fact a reader can
# check; the sites with an attack of their own are ROWED (M1907-M1954), not here.
_PLX = PL
EXEMPT += [
    (LA, "    let role = if proposer {\n        Some(\"an EVO proposer (the ranker)\")",
     "DOMINATED (checkable): evl.rs's evaluation adds every stored proposer of an arm policy to "
     "the record's `subject_issuers` (`for pref in policies.keys() { .. subjects.insert(p) }`, "
     "the lines above `the evaluator is a subject issuer`), so on any record the loop wrote the "
     "proposer is ALSO refused by the subject-issuer arm below (that arm has its own row, "
     "M1924); only a store writer who rewrote the record could "
     "separate the two, and the admission route's own proposer check does carry that attack "
     "(M114, `the_proposer_cannot_admit_its_own_candidate_whatever_the_record_lists`)"),
    (OB, '            .create_new(true)\n            .open(self.dir.join(format!("{nonce}.issued")))',
     "UNREACHABLE (checkable): the file name is 16 bytes of ring::rand::SystemRandom (the three "
     "lines above), so an existing `<nonce>.issued` is a 2^-128 event no caller can aim at "
     "(`issue` takes no name); create_new there only makes the impossible collision a refusal "
     "instead of an overwrite"),
    (PSVF, "        .create_new(true)\n        .mode(0o400)",
     "UNREACHABLE (checkable): write_private's one caller writes `completion-secret` into "
     "`job_dir`, created by `std::fs::create_dir(i.job_dir)` a few lines above in the same "
     "function (which fails if it exists) and holding only launch-manifest.json, so nothing "
     "can exist at the name; the dir is the helper's snapshot source, not a path a caller names"),
    (ST, "    o.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);",
     "DOMINATED (checkable): every open through nofollow() (`grep -n 'nofollow()' "
     "crates/axon-loop/src/store.rs`: read_text, write_atomic, append_jsonl, lock_file) is "
     "preceded in its function by self.guard(path), the lstat walk that refuses a symlink "
     "anywhere below the root (rows M979, M980, M998); write_atomic's create_new is O_EXCL, which "
     "refuses a symlink at the temporary name (M1917). The flag closes only the window between "
     "the guard's lstat and the open, which needs a writer of the store directory, the "
     "principal the store already trusts to be its owner"),
    ("crates/axon-os/src/cli.rs", "            .create_new(true)\n            .open(&kf)", _KIL),
    ("crates/axon-os/src/runtime.rs", "            .create_new(true)\n            .mode(0o600)", _ACR),
    ("crates/axon-os/src/cli.rs", "    if widened.is_empty() {",
     "NOT A DECISION (checkable): the `Profile:` line of `axon-os explain`, a human description "
     "of a grant (`profile_line`); its only caller (cmd_explain) prints it and nothing branches "
     "on it, and no crate outside axon-os names axon_os::cli"),
    (FP, "            } else if tag.is_ascii_lowercase() {",
     "DOMINATED (checkable): an assume-unchanged entry that differs from the committed tree is "
     "also refused by the byte comparison against the tree's committed bytes (M451); this arm "
     "supplies the reason's wording, and the first half of "
     "`a_skip_worktree_or_assume_unchanged_change_is_dirty` shows either guard alone refuses "
     "(the comment at its head)"),
]
_OSCLI = ("NOT ON THE PROTECTED ROUTE (checkable): the `axon-os` binary's own command functions: "
          "`grep -rn 'axon_os::cli\\|cli::run' crates/*/src` finds no caller outside axon-os (the "
          "protected crates name only supervise_requiring, parse_manifest, scan_effects, the ledger "
          "and the Grant/Isolation/Verdict::Completed types); a returned ExitCode is the CLI "
          "process's, never a Fabric verdict")
EXEMPT += [
    ("crates/axon-os/src/cli.rs", head, _OSCLI) for head in (
        "pub fn run(args: Vec<String>) -> ExitCode {", "fn help(line: &str) -> ExitCode {",
        "fn cmd_explain(job: &Path) -> ExitCode {", "fn cmd_verify(record: &Path) -> ExitCode {",
        "fn cmd_replay(rest: &[&str]) -> ExitCode {", "fn cmd_kill(rest: &[&str]) -> ExitCode {",
        "fn cmd_status(rest: &[&str]) -> ExitCode {",
        "fn status_all(store: &Path, latest_only: bool, json_out: bool) -> ExitCode {",
        "fn cmd_audit(rest: &[&str]) -> ExitCode {")
]
_EXITMAP = ("REFINES THE KIND OF A NON-SUCCESS (checkable): `{what}` maps an error variant to the "
            "process exit code of the CLI that reports it; it is called only on the Err path ({where}) "
            "and every arm is a literal non-zero code, so no variant can be reported as success; "
            "which non-zero code names the kind of failure, not whether it failed")
EXEMPT += [
    ("crates/axon-loop/src/error.rs", "    pub fn exit_code(&self) -> i32 {",
     _EXITMAP.format(what="LoopError::exit_code", where="axon-loop.rs's `Err(e)` arm, the only caller")),
    (FS, "    pub fn exit_code(&self) -> i32 {",
     _EXITMAP.format(what="SubmitError::exit_code", where="axon-fabric.rs's `Err(e) => refuse(..)` arm, the only caller")),
    ("crates/axon-os/src/verdict.rs", "    pub fn exit_code(&self) -> i32 {",
     "NOT ON THE PROTECTED ROUTE (checkable): Verdict::exit_code is called only by axon-os "
     "(`grep -rn '\\.exit_code()' crates/*/src` outside axon-os names LoopError, SubmitError and "
     "AdmitError, never axon_os::Verdict); Fabric consumes the Verdict by variant, not by code, and "
     "the one variant mapped to 0 is Completed"),
    ("crates/axon-vm/src/admit.rs", "    pub fn exit_code(&self) -> i32 {", _VM),
]


# ── Amendment 87 (C9 round 6, eqgate2): the sites the permission / privilege /
# process forms expose. The sites with an attack of their own are ROWED
# (M2144-M2169), not here. An entry that starts REMAINDER is NOT a claim that
# the guard is dominated: it states that no test observes it yet (a guest-only
# step, an unobservable bit, a fallback, a development-only path), so the list
# of what is left is greppable, not hidden. An exemption pinned on the site's OWN
# line does not conflict with a row on a LATER line of the same arm (the rows of
# the arm's fchmod/fchown), which the block rule would otherwise count as
# covering it.
#   grep -n 'REMAINDER' scripts/v022_refusal_coverage.py
_R87_GUEST = ("REMAINDER (no row yet): runs only as the guest's PID 1 inside the microVM; no host "
              "test executes it, and a guest change needs a real boot (psv_guest_boot_test.sh), "
              "which this workstream did not run")
EXEMPT += [
    ("crates/axon-fabric/src/workspace.rs", "                let _ = set_mode(p, 0o755);",
     "REMAINDER (no row yet): unlocks a read-only tree's directories so it can be removed; only a "
     "NON-root remover is stopped by a mode (root bypasses it, and the suite runs as root where it "
     "matters), and a failure only leaves the tree behind (availability, never a verdict). The row "
     "this would have carried (amendment 87's withdrawn row) survived as root and was withdrawn, not weakened"),
    (PL, "                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,\n                )\n                .map_err(|e| e.to_string())?;\n                unsafe {",
     "RACE-ONLY (checkable): this open is reached only for an entry the fstatat above "
     "(AT_SYMLINK_NOFOLLOW, M1912) reported as a regular file, in a directory only "
     "this process can write until the hand-over gives it away (make_out: mkdirat 0700, owner "
     "re-checked), so no other uid can swap the entry for a symlink between the two calls and no "
     "input drives this; a symlink the launch leaves is classified S_IFLNK by the fstatat and never "
     "reaches this arm. The fchmod and fchown after it are rowed (M2163, M2164)"),
    ("crates/axon-cortex/src/runner.rs", "        cmd.pre_exec(move || {", _LOC),
    ("crates/axon-cortex/src/runner.rs", "                if libc::prctl(", _LOC),
    ("crates/axon-fabric/src/bin/axon-custodian.rs", "                .mode(0o700)\n                .recursive(true)",
     "NOT ON THE PROTECTED ROUTE (checkable): this is the `--dev` branch of axon-custodian "
     "(`DEV custodian ... (never protected)`, the line below): a development custodian never yields "
     "a protected launch (a_dev_custodian_never_yields_a_protected_launch); the protected store is "
     "the operator's, checked by custodian.rs's owner/mode refusal (0o077, rowed)"),
    (PL, "    unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) == 1 }",
     "REMAINDER (no row yet): a QUERY of the caller's NoNewPrivileges (the trust preflight's `--probe` "
     "and the production launch's refusal read it). Weakening the read to 0 is the attack of the test "
     "`a_fabric_under_no_new_privs_is_told_why_the_helper_launches_nothing` (its ATTACK at 'under its "
     "caller's NoNewPrivileges'), which would fail, but no row of its own carries that kill"),
    (PL, "                if libc::getrlimit(r, &mut cur) == 0 {\n                    cur.rlim_cur = if v == 0",
     "REMAINDER (no row yet): the fallback of `lim` for a helper that is not root (a test-trust build run "
     "unprivileged) lowering the soft limit to the hard one; the production helper is root and takes "
     "the setrlimit above (rowed, M2167)"),
    (PL, "                if libc::getrlimit(r, &mut cur) == 0 {\n                    cur.rlim_cur = soft.min",
     "REMAINDER (no row yet): the fallback of `lim2` for a helper that is not root (a test-trust build run unprivileged), which lowers the soft limit to the hard one; the tests run as root, where the setrlimit above succeeds (that call is rowed, M2276, and every lim2(..) call is rowed, M1605-M1613; the stale text that said 'no row yet' for them is withdrawn)"),
    ("crates/axon-fabric/src/sealed_exec.rs", "    if unsafe { libc::fcntl(fd, libc::F_SETLEASE, libc::F_RDLCK) } == 0 {",
     "DOMINATED (checkable): a lease that was not taken is refused by the F_GETLEASE re-read before the "
     "exec (`leased` is judged under Lease::Required, rows M524 and M591); this call only takes it"),
    ("crates/axon-fabric/src/sealed_exec.rs", "        unsafe { libc::fcntl(fd, libc::F_SETOWN, 0) };",
     "REMAINDER (no row yet): clears the lease-break signal's owner after the lease is taken (hygiene, "
     "so a break does not signal this process); no test observes it"),
    ("crates/axon-fabric/src/sealed_exec.rs", "        cmd.pre_exec(move || exec.run());",
     "ROWED ELSEWHERE (checkable): the closure `exec.run()` this pre_exec installs is the child's "
     "re-check and execveat of the verified program (M527, M528); removing the hook is those rows' attack"),
    ("crates/axon-os/src/runtime.rs", "match std::fs::DirBuilder::new().mode(0o700)", _ACR),
    ("crates/axon-os/src/runtime.rs", "            .mode(0o600)", _ACR),
    ("crates/axon-os/src/runtime.rs", "        let rc = unsafe { libc::killpg(pid, libc::SIGKILL) };", _ACR),
    ("crates/axon-os/src/runtime.rs", "        .process_group(0)", _ACR),
]


# ── Amendment 91 (C9 round 7, eqgate3): the sites the environment, stdio, option,
# cap, OS-call and delegating-refusal forms expose and no row attacks. Every entry
# states a call-graph or documentation FACT; an entry that begins REMAINDER is a
# guard no test observes yet, not a claim of domination (grep -n REMAINDER).
EXEMPT += [
    ('crates/axon-fabric/src/git_data.rs', '        .env("LC_ALL", "C")\n        .env("GIT_NO_REPLACE_OBJECTS", "1")',
     "DIAGNOSTICS/LOCALE (checkable): LC_ALL=C fixes the LANGUAGE of git's messages; every answer is parsed from plumbing and porcelain output (`rev-parse`, `status --porcelain`, `cat-file --batch`), never from a message, so no verdict can depend on it"),
    ('crates/axon-fabric/src/git_data.rs', '        .env("GIT_CONFIG_NOSYSTEM", "1")\n        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_OPTIONAL_LOCKS", "0")',
     "REMAINDER (needs a writable /etc/gitconfig): GIT_CONFIG_NOSYSTEM stops git reading the HOST's system config; no test can plant one without writing under /etc, which this workstream never does. The repository-controlled configuration (the attack) is rowed (M2283-M2291)"),
    ('crates/axon-fabric/src/git_data.rs', '        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_OPTIONAL_LOCKS", "0")',
     'DOMINATED BY env_clear (rowed, M2283) (checkable): git reads GLOBAL config only from $XDG_CONFIG_HOME/git/config, $HOME/.config/git/config and $HOME/.gitconfig (git-config(1), FILES); env_clear leaves none of those variables set, so the file named here is the only global config and it is /dev/null either way'),
    ('crates/axon-fabric/src/git_data.rs', '        .env("GIT_TERMINAL_PROMPT", "0")',
     'DOMINATED (checkable): GIT_TERMINAL_PROMPT=0 only stops a credential prompt, which only a transport would raise; `protocol.allow=never` and GIT_NO_LAZY_FETCH (rowed, a_git_call_on_a_promisor_repository_fetches_nothing) leave git no transport'),
    ('crates/axon-fabric/src/git_data.rs', '            "-c",\n            "core.untrackedCache=false",\n            "-c",',
     'REMAINDER (a forged untracked-cache extension that hides a file was not constructible: measured with `git update-index --untracked-cache`, then a file added under a directory whose mtime was restored; git re-listed the directory): core.untrackedCache=false; the index is never written (GIT_OPTIONAL_LOCKS, rowed M2284), so no cache is built by git_cmd either'),
    ('crates/axon-fabric/src/git_data.rs', '            "-c",\n            "advice.graftFileDeprecated=false",\n        ])',
     'DIAGNOSTICS ONLY (checkable): advice.graftFileDeprecated=false silences an advice line on stderr, which git_cmd never reads'),
    ('crates/axon-fabric/src/git_data.rs', '        .stdin(Stdio::null())\n        .stderr(Stdio::null());',
     "DOMINATED (checkable): a git_cmd call's stdin/stderr: every caller either runs `.output()` (stdin is never inherited by output(), std::process::Command docs) or overrides it with a pipe (Objects::open, provenance.rs); stderr is never read (`grep -n 'stderr' git_data.rs`: no read), so an inherited one writes git's own diagnostics into the Fabric's log and nothing else"),
    ('crates/axon-fabric/src/git_data.rs', '        .stderr(Stdio::null());',
     "DIAGNOSTICS ONLY (checkable): a git_cmd call's stdin/stderr: every caller either runs `.output()` (stdin is never inherited by output(), std::process::Command docs) or overrides it with a pipe (Objects::open, provenance.rs); stderr is never read (`grep -n 'stderr' git_data.rs`: no read), so an inherited one writes git's own diagnostics into the Fabric's log and nothing else"),
    ('crates/axon-fabric/src/git_data.rs', '        .env_clear()\n        .env("PATH", "/usr/bin:/bin")\n        .env("LC_ALL", "C")\n        .env("GIT_CONFIG_NOSYSTEM", "1")\n        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_CEILING_DIRECTORIES", "/")',
     'DOMINATED (env_clear) (checkable): the config reader (`git -C / config --file <path> --no-includes --list -z`) reads ONLY the named file (git-config(1): --file reads that file instead of the usual ones, so GIT_CONFIG_*, HOME and the system and global files are not consulted); its answer is a key list judged by allowed_key, so the environment given to it cannot change which keys the file lists'),
    ('crates/axon-fabric/src/git_data.rs', '        .env("PATH", "/usr/bin:/bin")\n        .env("LC_ALL", "C")\n        .env("GIT_CONFIG_NOSYSTEM", "1")\n        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_CEILING_DIRECTORIES", "/")',
     'DOMINATED (PATH) (checkable): the config reader (`git -C / config --file <path> --no-includes --list -z`) reads ONLY the named file (git-config(1): --file reads that file instead of the usual ones, so GIT_CONFIG_*, HOME and the system and global files are not consulted); its answer is a key list judged by allowed_key, so the environment given to it cannot change which keys the file lists'),
    ('crates/axon-fabric/src/git_data.rs', '        .env("LC_ALL", "C")\n        .env("GIT_CONFIG_NOSYSTEM", "1")\n        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_CEILING_DIRECTORIES", "/")',
     'DOMINATED (LC_ALL) (checkable): the config reader (`git -C / config --file <path> --no-includes --list -z`) reads ONLY the named file (git-config(1): --file reads that file instead of the usual ones, so GIT_CONFIG_*, HOME and the system and global files are not consulted); its answer is a key list judged by allowed_key, so the environment given to it cannot change which keys the file lists'),
    ('crates/axon-fabric/src/git_data.rs', '        .env("GIT_CONFIG_NOSYSTEM", "1")\n        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_CEILING_DIRECTORIES", "/")',
     'DOMINATED (GIT_CONFIG_NOSYSTEM) (checkable): the config reader (`git -C / config --file <path> --no-includes --list -z`) reads ONLY the named file (git-config(1): --file reads that file instead of the usual ones, so GIT_CONFIG_*, HOME and the system and global files are not consulted); its answer is a key list judged by allowed_key, so the environment given to it cannot change which keys the file lists'),
    ('crates/axon-fabric/src/git_data.rs', '        .env("GIT_CONFIG_GLOBAL", "/dev/null")\n        .env("GIT_CEILING_DIRECTORIES", "/")',
     'DOMINATED (GIT_CONFIG_GLOBAL) (checkable): the config reader (`git -C / config --file <path> --no-includes --list -z`) reads ONLY the named file (git-config(1): --file reads that file instead of the usual ones, so GIT_CONFIG_*, HOME and the system and global files are not consulted); its answer is a key list judged by allowed_key, so the environment given to it cannot change which keys the file lists'),
    ('crates/axon-fabric/src/git_data.rs', '        .env("GIT_CEILING_DIRECTORIES", "/")',
     'DOMINATED (GIT_CEILING_DIRECTORIES) (checkable): the config reader (`git -C / config --file <path> --no-includes --list -z`) reads ONLY the named file (git-config(1): --file reads that file instead of the usual ones, so GIT_CONFIG_*, HOME and the system and global files are not consulted); its answer is a key list judged by allowed_key, so the environment given to it cannot change which keys the file lists'),
    ('crates/axon-fabric/src/git_data.rs', '            "--no-includes",',
     'DOMINATED (checkable): --no-includes only stops `include.path` being EXPANDED into the listing; the key `include.path` itself is listed either way and `allowed_key` refuses every include.* key (the `else` arm: unknown section), so a config with an include is refused with or without the flag (`repository_config_that_redirects_or_runs_code_is_refused`)'),
    ('crates/axon-fabric/src/git_data.rs', '        .stdin(Stdio::null())\n        .stderr(Stdio::null())\n        .output()',
     "DOMINATED/DIAGNOSTICS (checkable): a git_cmd call's stdin/stderr: every caller either runs `.output()` (stdin is never inherited by output(), std::process::Command docs) or overrides it with a pipe (Objects::open, provenance.rs); stderr is never read (`grep -n 'stderr' git_data.rs`: no read), so an inherited one writes git's own diagnostics into the Fabric's log and nothing else"),
    ('crates/axon-fabric/src/git_data.rs', '        .stderr(Stdio::null())\n        .output()',
     "DIAGNOSTICS ONLY (checkable): a git_cmd call's stdin/stderr: every caller either runs `.output()` (stdin is never inherited by output(), std::process::Command docs) or overrides it with a pipe (Objects::open, provenance.rs); stderr is never read (`grep -n 'stderr' git_data.rs`: no read), so an inherited one writes git's own diagnostics into the Fabric's log and nothing else"),
    ('crates/axon-fabric/src/git_data.rs', '        if out.len() > MAX_REASONS {',
     'DIAGNOSTICS ONLY (checkable): MAX_REASONS caps how many reason strings are LISTED; a non-empty list is the refusal either way (callers test `is_empty()`), so truncating or not changes only the length of the message'),
    ('crates/axon-fabric/src/backend.rs', '            .stderr(std::process::Stdio::null())\n            .spawn()',
     "DIAGNOSTICS ONLY (checkable): the privileged helper's stderr is never read (`grep -n 'child.stderr' backend.rs`: none); its reply is the stdout JSON, parsed and judged. An inherited stderr puts the helper's own diagnostics in the Fabric's log"),
    ('crates/axon-fabric/src/observer.rs', '            .stderr(std::process::Stdio::null())\n            .spawn()',
     "DIAGNOSTICS ONLY (checkable): as backend.rs's helper spawn: the observation helper's stderr is never read; its reply is the stdout JSON"),
    ('crates/axon-fabric/src/observer.rs', '        let _ = o\n            .take(crate::observer_service::MAX_REPLY + 4096)',
     "REMAINDER (no row yet): bounds the privileged helper's reply read to MAX_REPLY+4096; a flood test needs a stand-in helper that streams past the bound, which the existing observer fixtures do not provide. The service side's bounds (M2305-M2307) are rowed"),
    ('crates/axon-fabric/src/privileged_launcher.rs', '            .current_dir("/")\n            .status()',
     'DOMINATED (checkable): `harden()` chdirs the helper to / at start (libc::chdir(c"/"), rowed by a_callers_process_state_never_reaches_the_root_helper_or_its_launcher, ATTACK \'kept its caller\'s working directory\'), so the child inherits / even without this call; the stand-in launcher\'s `pwd` is / (the_root_launcher_runs_with_null_stdio_in_the_root_directory)'),
    ('crates/axon-fabric/src/privileged_launcher.rs', '            if let Ok(n) = cstr(&p.out_name) {',
     'CLEANUP ONLY (checkable): removes the EMPTY out dir after sealed_exec::command failed to build the launch command (nothing ran); a leftover empty directory is availability, not a verdict (the next launch of the same name is refused as an existing out dir, `a_root_owned_out_dir_that_already_exists_is_never_launched_into`)'),
    ('crates/axon-fabric/src/workspace.rs', '        libc::renameat2(\n            libc::AT_FDCWD,',
     "ROWED ELSEWHERE (checkable): the call's flags argument is M1916 (RENAME_NOREPLACE); removing the call itself makes every publish fail, which every workspace test sees"),
    ('crates/axon-fabric/src/custodian.rs', '            None => {\n                let mut text = Vec::new();\n                (&s).take(MAX_MESSAGE)',
     'DEVELOPMENT ROUTE (checkable): the arm runs only for a custodian with no pinned program (`self.sha256` None); a production helper refuses a custodian with no pin (privileged_launcher.rs, a_production_helper_launched_with_no_custodian_program_pin_...), and the pinned arm reads through read_from_pinned with the same bound'),
    ('crates/axon-os/src/runtime.rs', '            cmd.env("PATH", p); // cc/linker discovery for the interpreter',
     "NO EFFECT (checkable): the interpreter child's PATH is for `cc`/linker discovery by `axon run`, and the legacy process adapter is not on the protected route (_ACR); a missing PATH cannot widen what the child may do"),
    ('crates/axon-psv/src/runner.rs', '        .env("PATH", "/usr/bin:/bin")\n        // As on the host',
     'NO EFFECT (checkable): the check child spawns nothing (`Exec` is removed from its ceiling by without_exec, rowed M249, `the_process_holding_k_is_given_no_exec`), so no PATH lookup happens; the value is the fixed guest PATH either way'),
    ('crates/axon-vm/src/firecracker.rs', '        .stdin(Stdio::null())\n        .stdout(Stdio::piped())',
     "NOT ON THE PROTECTED ROUTE (checkable): axon-vm's firecracker spawn; the protected crates use axon_vm only for BACKEND_PROFILE and embed_policy_in_cmdline/MmdsPayload (grep `axon_vm::`), never the spawn"),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '    let text = std::fs::read_to_string(registry).unwrap_or_else(|e| bad(e.to_string()));',
     'FAILS CLOSED (checkable): `unwrap_or_else(.. bad(..))` on a read, parse or metadata error of the signer registry or key; the alternative is `.unwrap()`, a panic (exit 101): either way the process ends before any signer is returned and nothing is signed. The checks that decide WHICH files qualify are rowed: regular file (M2281), the exact fields (M2282), mode (M348), the public-key pin'),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '    let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| bad(e.to_string()));',
     'FAILS CLOSED (checkable): `unwrap_or_else(.. bad(..))` on a read, parse or metadata error of the signer registry or key; the alternative is `.unwrap()`, a panic (exit 101): either way the process ends before any signer is returned and nothing is signed. The checks that decide WHICH files qualify are rowed: regular file (M2281), the exact fields (M2282), mode (M348), the public-key pin'),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '        .unwrap_or_else(|| bad("not an object".into()));',
     'DOMINATED (checkable): a signer that is not an object has no keys, so the next check (`keys != [issuer_ref, key_path, public_key]`, rowed M2282) refuses it with the same exit code; this arm only words the reason (a_signer_the_operator_did_not_provision_properly_signs_nothing asserts the wording)'),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '        .unwrap_or_else(|e| bad(format!("issuer_ref: {e}")));',
     'FAILS CLOSED (checkable): `unwrap_or_else(.. bad(..))` on a read, parse or metadata error of the signer registry or key; the alternative is `.unwrap()`, a panic (exit 101): either way the process ends before any signer is returned and nothing is signed. The checks that decide WHICH files qualify are rowed: regular file (M2281), the exact fields (M2282), mode (M348), the public-key pin'),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '                if e.raw_os_error() == Some(libc::ELOOP) {',
     'DOMINATED (checkable): the ELOOP arm words the refusal of a symlinked key; the open itself (O_NOFOLLOW) fails for it either way and the next line\'s `bad(format!("key {}: {e}"..))` refuses it with the same exit code'),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '                bad(format!("key {}: {e}", path.display()))',
     'FAILS CLOSED (checkable): `unwrap_or_else(.. bad(..))` on a read, parse or metadata error of the signer registry or key; the alternative is `.unwrap()`, a panic (exit 101): either way the process ends before any signer is returned and nothing is signed. The checks that decide WHICH files qualify are rowed: regular file (M2281), the exact fields (M2282), mode (M348), the public-key pin'),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '        .unwrap_or_else(|e| bad(format!("key {}: {e}", path.display())));\n    {',
     'FAILS CLOSED (checkable): `unwrap_or_else(.. bad(..))` on a read, parse or metadata error of the signer registry or key; the alternative is `.unwrap()`, a panic (exit 101): either way the process ends before any signer is returned and nothing is signed. The checks that decide WHICH files qualify are rowed: regular file (M2281), the exact fields (M2282), mode (M348), the public-key pin'),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '        .unwrap_or_else(|e| bad(format!("key {}: {e}", path.display())));\n    let pk',
     'FAILS CLOSED (checkable): `unwrap_or_else(.. bad(..))` on a read, parse or metadata error of the signer registry or key; the alternative is `.unwrap()`, a panic (exit 101): either way the process ends before any signer is returned and nothing is signed. The checks that decide WHICH files qualify are rowed: regular file (M2281), the exact fields (M2282), mode (M348), the public-key pin'),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '        .unwrap_or_else(|e| bad(format!("key {}: {e}", path.display())));\n    if pk != field',
     'FAILS CLOSED (checkable): `unwrap_or_else(.. bad(..))` on a read, parse or metadata error of the signer registry or key; the alternative is `.unwrap()`, a panic (exit 101): either way the process ends before any signer is returned and nothing is signed. The checks that decide WHICH files qualify are rowed: regular file (M2281), the exact fields (M2282), mode (M348), the public-key pin'),
    ('crates/axon-fabric/src/bin/axon-protected-launcher.rs', '        if observe {\n            finish(\n                serde_json::to_string(&pl::ObserveReport::refused(why)).unwrap_or_default(),',
     'ROWED ELSEWHERE (checkable): `finish` prints the reply and exits with the code it is given (its print is M2335); the report and the code are what serve_as/serve_observe return, rowed through the exit-code tests of every launch test (they parse the reply and assert the code)'),
    ('crates/axon-fabric/src/bin/axon-protected-launcher.rs', '        finish(serde_json::to_string(&r).unwrap_or_default(), code)',
     'ROWED ELSEWHERE (checkable): `finish` prints the reply and exits with the code it is given (its print is M2335); the report and the code are what serve_as/serve_observe return, rowed through the exit-code tests of every launch test (they parse the reply and assert the code)'),
    ('crates/axon-fabric/src/bin/axon-protected-launcher.rs', '    finish(report(r), code)',
     'ROWED ELSEWHERE (checkable): `finish` prints the reply and exits with the code it is given (its print is M2335); the report and the code are what serve_as/serve_observe return, rowed through the exit-code tests of every launch test (they parse the reply and assert the code)'),
]

EXEMPT += [
    ("crates/axon-os/src/runtime.rs", "        .stdin(Stdio::null())\n        .stdout(Stdio::piped())",
     "NOT ON THE PROTECTED ROUTE (checkable): the legacy process adapter (_ACR). REMAINDER (no row yet): its interpreter child's stdin; a test would need a hostile parent with a live stdin pipe, and the child runs a program the supervisor already admitted"),
]


# ── C9 round 8, EQGATE4 (amendment 95): what the new forms exposed, and what was done with each ──────
# Every entry below was a refusal SITE the gate could not see before amendment 95: a value or atomic
# directory creation (VALUE_FORM), a single-line `.ok_or(..)?` (OKOR_FORM), a constant a refusing function
# reads (const_sites), one TERM of a compound guard (TERM_EXEMPT), or a form credited by a row on a
# neighbouring line. The entries that begin REMAINDER are guards no test observes alone yet, NOT claims of
# domination: `grep -n REMAINDER scripts/v022_refusal_coverage.py`.
_A95 = {
    'ensure_outroot': "ENSURE-EXISTS (checkable): `create_dir_all` of the service's out root, which exists already on a protected host: ProtectedHost::load refuses an out_root leaf that is absent, agent-owned or group/other accessible (the_out_root_leaf_is_the_services_own_and_private, A56) and the root helper re-judges it at every launch (open_out_root: its owner M2438). What is created NEW is below it: the per-operation private inputs dir (M2418) and job dir (M2409)",
    'ensure_runs': "ENSURE-EXISTS (checkable): `<state>/runs` is the parent of the per-operation run dirs, each created NEW one call below (RunDir::create_new, M2408); the state dir is the caller's own, not an authority",
    'ensure_store': "ENSURE-EXISTS (checkable): a store's own directory, made on first use and idempotent by intent (refusing an existing one would fail every second call). What the store refuses is done under it, on the entry: publish_file's no-clobber rename (M1916) and its same-bytes check, create_once's no-clobber rename (M1939), materialize's existing-destination refusal (M2274)",
    'ensure_obs_store': "ENSURE-EXISTS (checkable): the custodian's nonce store directory, made on first use; the per-nonce record is created with create_new on the lines below (a 2^-128 name, exempt there) and the store's owner and 0700 mode are judged by check_store before axon-custodian accepts a connection (custodian.rs, bin/axon-custodian.rs)",
    'ensure_loop_root': "ENSURE-EXISTS (checkable): the loop store's root, made on open; every later path is judged by Store::guard and ensure_dir, which refuse a symlink or a non-directory in any component (M979, M980, M998)",
    'loop_ensure_dir': 'DOMINATED (checkable): `create_dir` here only makes a missing component and tolerates AlreadyExists; the decision is the `symlink_metadata` check on the SAME component directly below (a symlink, M979/M980, and a non-directory, M998), which judges a component this call made and one it found alike, so replacing `create_dir` with `create_dir_all` changes no outcome',
    'hidden_verb': "NOT ON THE PROTECTED ROUTE (checkable): `__psv-host-guest`, a hidden verb run only as the stand-in guest of the PSV dispatch tests (`grep -rn '__psv-host-guest' crates scripts` finds axon-fabric.rs and tests/psv_dispatch.rs only). The protected guest runs axon-psv-runner, whose drop and digest pin are rowed (M2401, M2402); what the stand-in returns is judged by Fabric (the forgery matrix, M179-M186, M2439-M2441)",
    'staging_random': "UNREACHABLE (checkable): the staging dir's name is `<id>-<16 hex of ring randomness>` (64 bits) under the root-private staging root the lines above just walked and verified (operator_dir, mode 0700), so an existing entry is a 2^-64 event no caller can aim at, and `recursive(true)` would differ from the plain create only on an existing path or a missing parent (the root exists: walk_open opened it)",
    'setuid_msg': 'DOMINATED (checkable): setuid_honoured only chooses WHICH message a non-root helper gets; the caller refuses every helper whose effective uid is not 0 outside a test-trust build (`if euid != 0 && !authority.test` in axon-protected-launcher.rs, M602) whichever way this function answers',
    'pidfd': "OS ERROR (checkable): the raw syscall's only decision is the `raw < 0` check on the next lines (pidfd_open failed: the parent cannot be identified, nothing is relayed: fails closed, exempted there); a success returns a descriptor for the parent that is then identified by its uid and executable",
    'okor_piped': 'UNREACHABLE (checkable): the child was spawned a few lines above with `Stdio::piped()` for this stream and nothing else in the file takes it, so the `Option` is `Some`; the `ok_or` converts an impossible None into an error',
    'okor_plainname': 'DOMINATED (checkable): `plain_name` on the next line refuses the empty name (`!b.is_empty()`), which is what a path with no file name would become, so removing this refusal changes no outcome',
    'okor_parent_leaf': 'UNREACHABLE (checkable): `p.file_name() != Some(leaf)` three lines above refuses every path that has no file name, and `Path::parent()` is None only for `/` or an empty path, which have none; a path that got this far has a parent',
    'okor_field': "REMAINDER (no row yet, amendment 95): a single-line `.ok_or(..)?` refusal of an ABSENT field of a document or an absent lookup or path component. It was invisible to the gate until amendment 95 (only closures that call refused()/bad()/fail() were sites). No claim is made that a test removes it alone: the bypass needs a per-site default value of the field's type, which no generic mutation supplies, so the survey could not try it. `grep -n 'REMAINDER (no row yet, amendment 95)' scripts/v022_refusal_coverage.py` lists them",
    'const_tag': "REMAINDER (no row yet, amendment 95): a compiled-in tag (schema, scheme, domain or class prefix) compared with the value a document or peer carries. Survey (amendment 95): ten tags were sampled, each changed by appending `-eq4x`: six were killed by existing fixtures (custodian, observer, launcher, grants registry, the policy ack, the guest policy) and FOUR SURVIVED (readiness CERT_SCHEMA, the journal's, the attestation's, the launch manifest's), so a literal pin from outside is not universal. The other tags were not surveyed. Not counted killed",
    'const_path': 'REMAINDER (no row yet, amendment 95): a fixed path or name an operator file or the guest is found by (a config path, a pinned binary, a cmdline key, an environment name). Changing it points the helper at another file the operator did not author, or at none. Not surveyed: no test installs a second file at the mutated path',
    'const_bound': "REMAINDER (no row yet, amendment 95): a numeric bound (a size, a timeout, a depth, an age) read by a refusing function. Survey (amendment 95): mutated to 2^30 (2^40 for an age) with the crate's suite run: killed and ROWED are the custodian line, the policy bound, the evidence age, the guest policy word, the cmdline limit, the contract and recipe limits; SURVIVED are the observer's MAX_REPLY and MAX_PROFILE_MANIFEST, sealed_exec's MAX_BYTES, backend's MAX, git's MAX_REASONS and certcheck's MAX_DEPTH. The other bounds were not surveyed",
    'const_table': "REMAINDER (no row yet, amendment 95): a table a refusing function matches against (profile ids, flag names, config keys, kinds). Survey (amendment 95): an entry replaced by another, the crate's suite run: the recipe's SKIPPED_TOP_LEVEL survived (rowed, M2454), attest's CANONICAL was killed by two tests (not rowed), Cortex's policy files and paths survived; protected_host's KEYS and REFUSED_CALLER_FLAGS and the rest were not surveyed",
    'const_exit': "REMAINDER (no row yet, amendment 95): an exit code the helper reports (Launched, Refused, Unknown). Fabric and the helper's tests read the numbers through this const, so a consistent change is unobservable; a literal copy of 30 or 31 elsewhere would pin it",
    'const_text': 'REMAINDER (no row yet, amendment 95): a fixed string the code compares or matches (a profile name, an id, a reason text, a basis). A consistent change is unobservable where every reader takes the const; a literal copy elsewhere pins it from outside',
    'const_other': 'REMAINDER (no row yet, amendment 95): a structured constant (a backend profile record, a descriptor number, an open-flag set) read by a refusing function; each field is judged where it is compared',
    'materialize_dest': 'DOMINATED (checkable): the `dest.exists() || symlink_metadata(dest).is_ok()` refusal on the lines above (DestinationExists, M2274) has already refused an existing destination, so `create_dir_all` here differs from `create_dir` only by making missing PARENTS, which is the intent (a caller names `<dir>/candidate` under a dir not yet made); a racing creator would have to write the Fabric-private tree',
    'ensure_tree_parent': "ENSURE-EXISTS (checkable): the parent directories of an entry of the store's own manifest (paths validated by check_path at import, and the manifest hashes to its reference), made when absent",
    'ensure_trial': "ENSURE-EXISTS (checkable): the trial cache's `home`, `xdg-cache` and `cargo-target` directories under a per-trial root, made on first use so a build finds its cache again; the root's mode is set to 0700 on the next line (`set_mode(&root, 0o700)`)",
    'nofollow_excl': "DOMINATED (checkable): the open is `create_new(true)` (O_CREAT|O_EXCL), which the kernel refuses on any existing name, a symlink included (EEXIST), so O_NOFOLLOW adds nothing: `a_symlink_at_a_snapshot_destination_is_refused_by_create_new_alone` shows create_new alone refuses it; survey (amendment 95): removing the flag left the whole axon-fabric suite green",
    'unlink_job': "REMAINDER (no row yet, amendment 95): removes the job directory itself (empty by now) from Fabric's inputs after the loop above unlinked each file in it; the secret's departure is the loop's, observed by the_secret_leaves_fabrics_job_dir_before_the_launcher_runs, and a leftover empty directory holds nothing",
    'create_mode_window': "DOMINATED (checkable): the final mode is set by `set_mode(&shown, mode)` after the copy (the umask does not apply: the comment there); the mode given at creation governs only the window while the bytes are written, inside the root-private 0700 staging dir the helper just made (`a_snapshot_never_copies_into_a_directory_that_already_exists` observes the final 0644/0755); survey and row run (amendment 95): `.mode(0o777)` here survived the whole axon-fabric suite and that test",
    'take_equiv': "EQUIVALENT in outcome (checkable): the `take(BOUND + 1)` only limits how much of an oversized or growing file is read into memory; the size is refused by the comparison against the bound next to it (`bytes.len() > MAX_POLICY`, `id.size > MAX_BYTES`), so no input is accepted or refused differently; survey (amendment 95): removing it left the whole axon-fabric suite green",
}


# Amendment 98 (eqgate5): the `.ok_or(..)?` refusals of an absent field were judged one by one, by
# applying the PERMISSIVE default (`unwrap_or(..)`, `unwrap_or_default()`, an empty list) and running
# the owning crate's suite (axon-fabric sharded, on gpumaster; the others locally). Each exemption
# below says what that run found. REMAINDER kinds are counted and NOT claimed covered.
def _observed(names):
    return ("OBSERVED-NOT-ROWED (survey, amendment 98): the permissive edit fails the test(s) " + names
            + "; no row of its own")


def _closed(why):
    return ("REMAINDER (survey, amendment 98, fail-closed): the permissive edit leaves the suite green and "
            "FAILS CLOSED: " + why)


def _unjudged(edit):
    return ("REMAINDER (survey, amendment 98, unjudged): the edit " + edit + " leaves the suite green; "
            "which way the default then fails was not judged")


def _nodefault():
    return ("REMAINDER (survey, amendment 98, no default): the lookup of an absent entity has no neutral "
            "value to default to (the edit would invent the entity or loop); not mutated")


def _offroute(why):
    return "REMAINDER (survey, amendment 98, off the protected route): " + why


def _dominated(fact):
    return "DOMINATED (checkable, survey amendment 98): " + fact + "; the edit leaves the suite green because of it"

EXEMPT += [
    ('crates/axon-attest/src/lib.rs', '    let aop = axon_os_path.ok_or_else(|| MeasureError::ComponentMissing("axon-os".to_string()))?;',
     _offroute("axon-attest's measurement of the host stack (axon-vm), not a route of the protected profile; the edit substitutes the kernel's path for axon-os and the suite stayed green")),
    ('crates/axon-attest/src/lib.rs', 'pub const SOFTWARE_TPM_HW_ROOT: &str = "software-tpm-v1";',
     _A95['const_path']),
    ('crates/axon-attest/src/lib.rs', 'const AXTCB_EXT_PREFIX: &str = "axtcb1-ext:";',
     _A95['const_tag']),
    ('crates/axon-attest/src/lib.rs', '    const CANONICAL: [&str; 4] = ["kernel", "axon-os", "axon-audit", "monitor"];',
     _A95['const_table']),
    ('crates/axon-certcheck/src/synth.rs', 'const MAX_DEPTH: usize = 64;',
     _A95['const_bound']),
    ('crates/axon-core/src/interp/conform.rs', 'const UNDET: &str = "?undetermined";',
     _A95['const_tag']),
    ('crates/axon-core/src/interp/conform.rs', 'const HANDLE: &str = "handle:";',
     _A95['const_tag']),
    ('crates/axon-core/src/interp/conform.rs', 'const MAX_CAST_DEPTH: usize = 1_000_000;',
     _A95['const_bound']),
    ('crates/axon-cortex/src/generate.rs', 'const MAX_GENERATOR_READ: usize = 1 << 20;',
     _CTX),
    ('crates/axon-cortex/src/runner.rs', '        let g = grant.ok_or_else(|| Refusal::UnknownGrant("<none>".into()))?;',
     _offroute('Cortex, the local repair loop, which is not a route of the protected profile; not surveyed')),
    ('crates/axon-cortex/src/runner.rs', '        std::fs::create_dir_all(dst)?;',
     _CTX),
    ('crates/axon-cortex/src/runner.rs', '            .ok_or_else(|| unresolvable("not found on PATH".into()))?',
     _offroute('Cortex, the local repair loop, which is not a route of the protected profile; not surveyed')),
    ('crates/axon-cortex/src/runner.rs', '        .ok_or_else(|| bad("is not check-suite:<id>@<version>#<entry>"))?;',
     _offroute('Cortex, the local repair loop, which is not a route of the protected profile; not surveyed')),
    ('crates/axon-cortex/src/runner.rs', '        .ok_or_else(|| bad("names no version"))?;',
     _offroute('Cortex, the local repair loop, which is not a route of the protected profile; not surveyed')),
    ('crates/axon-cortex/src/runner.rs', '    let (version, entry) = rest.split_once(\'#\').ok_or_else(|| bad("names no entry"))?;',
     _offroute('Cortex, the local repair loop, which is not a route of the protected profile; not surveyed')),
    ('crates/axon-cortex/src/runner.rs', '            .ok_or("check registry has no `executors` array")?',
     _offroute('Cortex, the local repair loop, which is not a route of the protected profile; not surveyed')),
    ('crates/axon-cortex/src/runner.rs', 'const POLICY_FILES: &[&str] = &["axon.lock", ".axon-policy", "gate.sh", "profile.rs"];',
     _CTX),
    ('crates/axon-cortex/src/runner.rs', 'const POLICY_PREFIXES: &[&str] = &["scripts/", "governance/", ".github/"];',
     _CTX),
    ('crates/axon-cortex/src/runner.rs', 'const POLICY_PATHS: &[&str] = &["AXON-COMPLETENESS.json", "AXON-COMPLETENESS.md"];',
     _CTX),
    ('crates/axon-cortex/src/runner.rs', 'pub const LOCAL_AXON_TEST_ID: &str = "axon-test-local";',
     _CTX),
    ('crates/axon-cortex/src/runner.rs', 'pub const FABRIC_SUBMIT_ID: &str = "axon-fabric";',
     _CTX),
    ('crates/axon-fabric/src/backend.rs', '        .ok_or("waiver file has no waivers list")?',
     _closed('an absent waivers list becomes an empty one, and a waiver only RELAXES a verdict (an empty list waives nothing)')),
    ('crates/axon-fabric/src/backend.rs', '        let name = non_empty(&x["assertion"]).ok_or("a waiver names no assertion")?;',
     _closed('a waiver that names no assertion becomes one for the name "", which no assertion has')),
    ('crates/axon-fabric/src/backend.rs', '            .ok_or("evidence record has no profile.manifest_sha256")?',
     _observed('a_record_naming_no_manifest_digest_is_refused_as_that')),
    ('crates/axon-fabric/src/backend.rs', '    if let Err(e) = std::fs::create_dir_all(&lx.out_root) {',
     _A95['ensure_outroot']),
    ('crates/axon-fabric/src/backend.rs', 'pub const LOCAL_INTERPRETER: Profile = Profile {',
     _A95['const_other']),
    ('crates/axon-fabric/src/backend.rs', 'pub const FIRECRACKER_AXON_KERNEL: Profile = Profile {',
     _A95['const_other']),
    ('crates/axon-fabric/src/backend.rs', 'pub const LINUX_GUEST_AXON_ID: &str = "axon-linux-guest";',
     _A95['const_text']),
    ('crates/axon-fabric/src/backend.rs', 'pub const ALL: &[Profile] = &[',
     _A95['const_table']),
    ('crates/axon-fabric/src/backend.rs', 'pub const WAIVER_SCHEMA: &str = "axon-b263-waiver/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/backend.rs', 'pub const X1_GUEST_POLICY_CHANNEL: &str = "x1_guest_policy_channel";',
     _A95['const_text']),
    ('crates/axon-fabric/src/backend.rs', '    const MAX: u64 = 256 << 20;',
     _A95['const_bound']),
    ('crates/axon-fabric/src/backend.rs', 'pub const GUEST_POLICY_SCHEMA: &str = "axon-vm-mmds/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/backend.rs', 'const LAUNCH_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin:/usr/local/bin";',
     _A95['const_path']),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '    std::fs::create_dir_all(&od).unwrap();',
     _A95['hidden_verb']),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '        expected_manifest_sha256: sha.clone(),',
     _A95['hidden_verb']),
    ('crates/axon-fabric/src/bin/axon-fabric.rs', '        drop: None,',
     _A95['hidden_verb']),
    ('crates/axon-fabric/src/branches.rs', '    std::fs::create_dir_all(dir)?;',
     _A95['ensure_store']),
    ('crates/axon-fabric/src/branches.rs', '            best.ok_or_else(|| BranchError::Unknown(format!("branch {exp}/{arm} has no head")))?;',
     _closed('an absent head becomes sequence 0, whose file `head-0.json` the branch never writes: the read that follows fails')),
    ('crates/axon-fabric/src/branches.rs', 'pub const EXPERIMENT_SCHEMA: &str = "axon-fabric-experiment/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/branches.rs', 'pub const HEAD_SCHEMA: &str = "axon-fabric-branch-head/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/custodian.rs', '                let nonce = r.nonce.as_deref().ok_or("check names no nonce")?;',
     _unjudged('`unwrap_or("")`')),
    ('crates/axon-fabric/src/custodian.rs', '                let nonce = r.nonce.as_deref().ok_or("spend names no nonce")?;',
     _unjudged('`unwrap_or("")`')),
    ('crates/axon-fabric/src/custodian.rs', '                    .ok_or("spend names no launch manifest sha256")?;',
     _observed('a_protected_custodian_spends_nothing_for_a_spend_naming_no_manifest')),
    ('crates/axon-fabric/src/custodian.rs', 'pub const CONFIG_PATH: &str = "/etc/axon/custodian.json";',
     _A95['const_path']),
    ('crates/axon-fabric/src/custodian.rs', 'pub const CONFIG_SCHEMA: &str = "axon-custodian/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/custodian.rs', 'pub const REQUEST_SCHEMA: &str = "axon-custodian-request/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/custodian.rs', 'pub const REPLY_SCHEMA: &str = "axon-custodian-reply/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/custodian.rs', 'const IO_TIMEOUT: Duration = Duration::from_secs(30);',
     _A95['const_bound']),
    ('crates/axon-fabric/src/custodian.rs', 'const SCM_PIDFD: libc::c_int = 4;',
     _A95['const_other']),
    ('crates/axon-fabric/src/custodian.rs', 'pub const MAX_OUTSTANDING: usize = 1024;',
     _A95['const_bound']),
    ('crates/axon-fabric/src/git_data.rs', '                .ok_or(format!("{} is not a gitfile", dotgit.display()))?;',
     _unjudged('`unwrap_or("")`')),
    ('crates/axon-fabric/src/git_data.rs', '        .ok_or("the repository config path is not UTF-8")?;',
     _unjudged('`unwrap_or("")`')),
    ('crates/axon-fabric/src/git_data.rs', '        let out = std::io::BufReader::new(child.stdout.take().ok_or("git cat-file: no stdout")?);',
     _A95['okor_piped']),
    ('crates/axon-fabric/src/git_data.rs', '        let stdin = self.child.stdin.as_mut().ok_or("git cat-file: no stdin")?;',
     _A95['okor_piped']),
    ('crates/axon-fabric/src/git_data.rs', '                .ok_or(format!("commit {commit} names no tree"))?,',
     _observed('git_data::tests::a_tree_entry_that_is_not_what_git_writes_is_refused_as_malformed')),
    ('crates/axon-fabric/src/git_data.rs', "                let sp = i + b[i..].iter().position(|&x| x == b' ').ok_or_else(bad)?;",
     _unjudged('`unwrap_or(0)`')),
    ('crates/axon-fabric/src/git_data.rs', '                let nul = sp + b[sp..].iter().position(|&x| x == 0).ok_or_else(bad)?;',
     _unjudged('`unwrap_or(0)`')),
    ('crates/axon-fabric/src/git_data.rs', '            .ok_or_else(|| format!("{} is not in a git working tree", start.display()))?;',
     _nodefault()),
    ('crates/axon-fabric/src/git_data.rs', 'pub const GIT_BIN: &str = "/usr/bin/git";',
     _A95['const_path']),
    ('crates/axon-fabric/src/git_data.rs', 'pub const ALLOWLIST_SCHEMA: &str = "axon-provenance-allowlist/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/grants.rs', '            .ok_or("grant registry has no `grants` array")?',
     _closed('an absent `grants` array becomes an empty registry: every grant lookup then fails')),
    ('crates/axon-fabric/src/grants.rs', '            .ok_or_else(|| format!("grant_ref `{grant_ref}` is not in the grant registry"))?;',
     _nodefault()),
    ('crates/axon-fabric/src/grants.rs', 'pub const GRANT_REGISTRY_SCHEMA: &str = "axon-fabric-grant-registry/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/grants.rs', 'const PLACEHOLDER_PROGRAM: &str = "fabric-request.ax";',
     _A95['const_text']),
    ('crates/axon-fabric/src/journal.rs', '            .ok_or_else(|| JournalError::UnknownScope(Box::new(scope.clone())))?;',
     _nodefault()),
    ('crates/axon-fabric/src/journal.rs', '            .ok_or_else(|| JournalError::UnknownOp(op.clone()))?\n            .clone();',
     _nodefault()),
    ('crates/axon-fabric/src/journal.rs', '            .ok_or_else(|| JournalError::UnknownOp(op.clone()))?;',
     _nodefault()),
    ('crates/axon-fabric/src/branches.rs', '            .ok_or_else(|| BranchError::Unverified("the operation recorded no receipt".into()))?;',
     _nodefault()),
    ('crates/axon-fabric/src/journal.rs', 'pub const JOURNAL_SCHEMA: &str = "axon-fabric-journal/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/observer.rs', '        std::fs::create_dir_all(&self.dir).map_err(|e| format!("nonce store: {e}"))?;',
     _A95['ensure_obs_store']),
    ('crates/axon-fabric/src/observer_service.rs', '            .ok_or("the observer\'s reply carries no signature")?;',
     _unjudged('`unwrap_or_default()`')),
    ('crates/axon-fabric/src/observer_service.rs', '                .ok_or("the helper config names no custodian")?,',
     _dominated('`serde_json::from_value::<CustodianRef>(Value::Null)` is an error, and it is the next statement')),
    ('crates/axon-fabric/src/observer_service.rs', 'pub const CONFIG_PATH: &str = "/etc/axon/observer.json";',
     _A95['const_path']),
    ('crates/axon-fabric/src/observer_service.rs', 'pub const CONFIG_SCHEMA: &str = "axon-observer/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/observer_service.rs', 'pub const REQUEST_SCHEMA: &str = "axon-observer-request/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/observer_service.rs', 'pub const REPLY_SCHEMA: &str = "axon-observer-reply/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/observer_service.rs', 'pub const MAX_REQUEST: u64 = 64 << 10;',
     _observed('observer_service::tests::an_observer_request_line_is_cut_off_at_its_size_bound (the bound raised to 2^30)')),
    ('crates/axon-fabric/src/observer_service.rs', 'pub const MAX_REPLY: u64 = 64 << 10;',
     _A95['const_bound']),
    ('crates/axon-fabric/src/observer_service.rs', 'const IO_TIMEOUT: Duration = Duration::from_secs(30);',
     _A95['const_bound']),
    ('crates/axon-fabric/src/observer_service.rs', 'const MAX_PROFILE_MANIFEST: u64 = 1 << 20;',
     _A95['const_bound']),
    ('crates/axon-fabric/src/privileged_launcher.rs', '    let parent = path.parent().ok_or_else(|| bad("no parent".into()))?;',
     _unjudged('`unwrap_or("/")`')),
    ('crates/axon-fabric/src/privileged_launcher.rs', '    let name = path.file_name().ok_or_else(|| bad("no file name".into()))?;',
     _unjudged('`unwrap_or("")`')),
    ('crates/axon-fabric/src/privileged_launcher.rs', '    let name = p.file_name().ok_or_else(outside)?;',
     _A95['okor_plainname']),
    ('crates/axon-fabric/src/privileged_launcher.rs', '        let parent = p.parent().ok_or_else(outside)?;',
     _A95['okor_parent_leaf']),
    ('crates/axon-fabric/src/privileged_launcher.rs', '        .create(&dir)',
     _A95['staging_random']),
    ('crates/axon-fabric/src/privileged_launcher.rs', '                    .custom_flags(libc::O_NOFOLLOW)',
     _A95['nofollow_excl']),
    ('crates/axon-fabric/src/privileged_launcher.rs', '            unsafe { libc::unlinkat(ifd.as_raw_fd(), c"job".as_ptr(), libc::AT_REMOVEDIR) };',
     _A95['unlink_job']),
    ('crates/axon-fabric/src/privileged_launcher.rs', '        .take(MAX_POLICY as u64 + 1)',
     _A95['take_equiv']),
    ('crates/axon-fabric/src/privileged_launcher.rs', '        .ok_or("this helper\'s config names no observer.service: no observation is relayed")?;',
     _nodefault()),
    ('crates/axon-fabric/src/privileged_launcher.rs', '    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, ppid, 0) } as RawFd;',
     _A95['pidfd']),
    ('crates/axon-fabric/src/privileged_launcher.rs', '    if m.uid() != 0 || m.mode() & libc::S_ISUID == 0 {',
     _A95['setuid_msg']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'pub const CONFIG_PATH: &str = "/etc/axon/protected-launcher.json";',
     _A95['const_path']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'pub const CONFIG_SCHEMA: &str = "axon-protected-launcher/2";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'pub const REQUEST_SCHEMA: &str = "axon-protected-launch-request/3";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'pub const REPORT_SCHEMA: &str = "axon-protected-launch-report/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'pub const OBSERVE_REQUEST_SCHEMA: &str = "axon-protected-observe-request/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'pub const OBSERVE_REPORT_SCHEMA: &str = "axon-protected-observe-report/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'pub const EXIT_LAUNCHED: i32 = 0;',
     _A95['const_exit']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'pub const EXIT_REFUSED: i32 = 30;',
     _A95['const_exit']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'pub const EXIT_UNKNOWN: i32 = 31;',
     _A95['const_exit']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'const PATH_ENV: &str = "/usr/sbin:/usr/bin:/sbin:/bin";',
     _A95['const_path']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'const MAX_REQUEST: u64 = 256 << 10;',
     _A95['const_bound']),
    ('crates/axon-fabric/src/privileged_launcher.rs', 'const DIR_FLAGS: libc::c_int = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW;',
     _A95['const_other']),
    ('crates/axon-fabric/src/protected_host.rs', '            .ok_or_else(|| NO_GRANT_REGISTRY.to_string())?;',
     _nodefault()),
    ('crates/axon-fabric/src/protected_host.rs', '                .ok_or_else(|| bad(format!("{ptr} is not a string")))?;',
     _dominated('the next lines refuse a path that is not absolute (`!p.is_absolute()`), and the edit\'s `""` is not')),
    ('crates/axon-fabric/src/protected_host.rs', '                .ok_or_else(|| bad("signer.issuer_ref is not a string".into()))?',
     _observed('an_observer_section_value_of_the_wrong_kind_is_refused_as_that')),
    ('crates/axon-fabric/src/protected_host.rs', '                .ok_or_else(|| bad("signer.public_key is not a string".into()))?',
     _observed('an_observer_section_value_of_the_wrong_kind_is_refused_as_that')),
    ('crates/axon-fabric/src/protected_host.rs', '                .ok_or_else(|| bad(format!("{} has no parent directory", p.display())))?;',
     _unjudged('`unwrap_or(p)`')),
    ('crates/axon-fabric/src/protected_host.rs', '                    .ok_or_else(|| bad("observer.custodian.uid is not a uid".into()))?;',
     _observed('an_observer_section_value_of_the_wrong_kind_is_refused_as_that')),
    ('crates/axon-fabric/src/protected_host.rs', '            .map(PathBuf::from)\n            .ok_or_else(|| bad(format!("{ptr} is not a string")))?;',
     _dominated("the next lines refuse a path that is not absolute (`!p.is_absolute()`), and the edit's empty path is not")),
    ('crates/axon-fabric/src/protected_host.rs', 'pub const PROTECTED_HOST_CONFIG: &str = "/etc/axon/protected-host.json";',
     _A95['const_path']),
    ('crates/axon-fabric/src/protected_host.rs', 'pub const PROTECTED_HOST_SCHEMA: &str = "axon-protected-host/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/protected_host.rs', 'pub const NO_GRANT_REGISTRY: &str =',
     _A95['const_text']),
    ('crates/axon-fabric/src/protected_host.rs', 'const KEYS: [&str; 9] = [',
     _A95['const_table']),
    ('crates/axon-fabric/src/provenance.rs', '        let mut stdin = child.stdin.take().ok_or("git check-ignore: no stdin")?;',
     _A95['okor_piped']),
    ('crates/axon-fabric/src/psv.rs', '    std::fs::create_dir_all(&lx.out_root).map_err(|e| format!("out_root: {e}"))?;',
     _A95['ensure_outroot']),
    ('crates/axon-fabric/src/psv.rs', 'pub const EVIDENCE_CLASS_PREFIX: &str = "evidence-class:";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/readiness.rs', '            .ok_or_else(|| format!("{} is not one of readiness\'s trust roots", dir.display()))?;',
     _nodefault()),
    ('crates/axon-fabric/src/readiness.rs', '        .ok_or(format!("{component}: no evidence listed"))?;',
     _dominated('an empty evidence list reaches `named(&evidence, .., "trust_preflight_sha256")?` a few lines below, which refuses it (`names no certified evidence file`)')),
    ('crates/axon-fabric/src/readiness.rs', '            .ok_or(format!("{component}: evidence entries are paths"))?;',
     _dominated('an entry read as `""` is the repository directory itself, which `read_once` refuses as not a regular file')),
    ('crates/axon-fabric/src/readiness.rs', 'pub const READINESS_SCHEMA: &str = "axon-fabric-readiness/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/readiness.rs', 'pub const CERT_SCHEMA: &str = "axon-v022-protected-certification/2";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/readiness.rs', 'pub const PROTECTED_PROFILE: &str = "linux-microvm-protected";',
     _A95['const_text']),
    ('crates/axon-fabric/src/readiness.rs', 'const PSV_SPEC: &str = "governance/specs/v022-protected-suite-verdict.md";',
     _A95['const_path']),
    ('crates/axon-fabric/src/readiness.rs', 'const REGISTRY: &str = "governance/cortex_gate_execution_registry.json";',
     _A95['const_path']),
    ('crates/axon-fabric/src/readiness.rs', 'const TRUST_EXPECTATIONS: &str = "governance/status/trust-expectations.json";',
     _A95['const_path']),
    ('crates/axon-fabric/src/readiness.rs', 'pub const COMPONENTS: [(&str, &[&str], &[&str]); 3] = [',
     _A95['const_table']),
    ('crates/axon-fabric/src/readiness.rs', 'pub const RUN_OUTPUT_SCHEMA: &str = "axon-fabric-submit/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/readiness.rs', 'pub const REQUEST_SCHEMA: &str = "acf-compute-request/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/readiness.rs', 'pub const TRUST_PREFLIGHT_SCHEMA: &str = "axon-trust-preflight/1";',
     _A95['const_tag']),
    ('crates/axon-fabric/src/sealed_exec.rs', '        .take(MAX_BYTES + 1)',
     _A95['take_equiv']),
    ('crates/axon-fabric/src/submit.rs', '        std::fs::create_dir_all(&runs).map_err(|e| SubmitError::Workspace(e.to_string()))?;',
     _A95['ensure_runs']),
    ('crates/axon-fabric/src/submit.rs', '        .ok_or_else(|| SubmitError::Unregistered(format!("check suite `{id}` is not registered")))?',
     _nodefault()),
    ('crates/axon-fabric/src/submit.rs', 'pub const PLACEHOLDER_POLICY_DIGEST: &str =',
     _A95['const_text']),
    ('crates/axon-fabric/src/workspace.rs', '    std::fs::create_dir_all(dir)?;',
     _A95['ensure_store']),
    ('crates/axon-fabric/src/workspace.rs', '        std::fs::create_dir_all(root.join("blobs"))?;',
     _A95['ensure_store']),
    ('crates/axon-fabric/src/workspace.rs', '        std::fs::create_dir_all(root.join("versions"))?;',
     _A95['ensure_store']),
    ('crates/axon-fabric/src/workspace.rs', '            .ok_or_else(|| StoreError::Corrupt(format!("manifest of {r} is malformed")))?;',
     _unjudged('`unwrap_or_default()`')),
    ('crates/axon-fabric/src/workspace.rs', '        std::fs::create_dir_all(dest)?;',
     _A95['materialize_dest']),
    ('crates/axon-fabric/src/workspace.rs', '                    std::fs::create_dir_all(parent)?;',
     _A95['ensure_tree_parent']),
    ('crates/axon-fabric/src/workspace.rs', '            .ok_or_else(|| StoreError::HashOnly(p.snapshot_ref.clone()))?;',
     _nodefault()),
    ('crates/axon-fabric/src/workspace.rs', '            std::fs::create_dir_all(root.join(sub))?;',
     _A95['ensure_trial']),
    ('crates/axon-guest-init/src/main.rs', 'const CMDLINE_PATH: &str = "/proc/cmdline";',
     _A95['const_path']),
    ('crates/axon-guest-init/src/main.rs', 'const CMDLINE_POLICY_KEY: &str = "axon.policy=";',
     _A95['const_path']),
    ('crates/axon-guest-init/src/main.rs', 'const POLICY_SCHEMA: &str = "axon-vm-mmds/1";',
     _A95['const_tag']),
    ('crates/axon-loop-contracts/src/attestation.rs', 'pub const ATTESTATION_SCHEMA: &str = "acf-receipt-attestation/2";',
     _A95['const_tag']),
    ('crates/axon-loop-contracts/src/attestation.rs', 'pub const EXECUTION_DOMAIN: &str = "axon.fabric-execution/1";',
     _A95['const_tag']),
    ('crates/axon-loop-contracts/src/lib.rs', 'pub const PROTECTED_PROFILES: &[&str] = &["linux-microvm-protected"];',
     _observed('protected_profiles_is_the_one_profile_the_launch_manifest_pins (a second profile added)')),
    ('crates/axon-loop-contracts/src/operator_trust.rs', '    pub const ALL: [TrustAuthority; 5] = [',
     _A95['const_table']),
    ('crates/axon-loop-contracts/src/operator_trust.rs', 'pub const EVIDENCE_SIGNATURE_SCHEMA: &str = "axon-evidence-signature/2";',
     _A95['const_tag']),
    ('crates/axon-loop-contracts/src/protected_evidence.rs', 'pub const PSV_EVIDENCE_SCHEMA: &str = "axon-psv-evidence/2";',
     _A95['const_tag']),
    ('crates/axon-loop-contracts/src/schema.rs', 'const ANNOTATIONS: &[&str] = &["$schema", "$id", "title", "description"];',
     _A95['const_table']),
    ('crates/axon-loop-contracts/src/schema.rs', 'const SUPPORTED: &[&str] = &[',
     _A95['const_table']),
    ('crates/axon-loop/src/bin/axon-loop.rs', '                .ok_or_else(|| LoopError::Usage(format!("--{k} needs a value")))?;',
     _unjudged('`unwrap_or_default()`')),
    ('crates/axon-loop/src/bin/axon-loop.rs', '                .ok_or_else(|| LoopError::Usage("missing --store DIR".into()))?,',
     _unjudged('`unwrap_or_default()`')),
    ('crates/axon-loop/src/candidates.rs', '        .ok_or_else(|| LoopError::Io(format!("store corrupt: candidate set {r} missing")))?;',
     _dominated('an empty text is not a record: `strict_record("")` is an error, and it is the next statement')),
    ('crates/axon-loop/src/evl.rs', 'pub const CONTEXT_DOMAIN: &str = "axon.closed-loop.context/1";',
     _A95['const_tag']),
    ('crates/axon-loop/src/intake.rs', 'pub const ACK_SCHEMA: &str = "micode.closed-loop.policy-ack/1";',
     _A95['const_tag']),
    ('crates/axon-loop/src/plan.rs', '        .ok_or_else(|| LoopError::Io(format!("store corrupt: plan {r} missing")))?;',
     _observed('g6_freeze_is_permanent')),
    ('crates/axon-loop/src/price.rs', 'pub const PRICE_SCHEDULE_SCHEMA: &str = "axon.loop.price-schedule/1";',
     _A95['const_tag']),
    ('crates/axon-loop/src/rules.rs', 'pub const PPM: u64 = 1_000_000;',
     _A95['const_bound']),
    ('crates/axon-loop/src/safety.rs', 'pub const CLEARANCE_DOMAIN: &str = "axon.loop.trial-safety/1";',
     _A95['const_tag']),
    ('crates/axon-loop/src/store.rs', '            .ok_or_else(|| format!("{who} has no registered {} key", a.dir_name()))?;',
     _nodefault()),
    ('crates/axon-loop/src/store.rs', '        fs::create_dir_all(&root)?;',
     _A95['ensure_loop_root']),
    ('crates/axon-loop/src/store.rs', '            match fs::create_dir(&cur) {',
     _A95['loop_ensure_dir']),
    ('crates/axon-loop/src/store.rs', '            .ok_or_else(|| LoopError::Io(format!("{} has no parent", path.display())))?;',
     _unjudged('`unwrap_or(path)`')),
    ('crates/axon-loop/src/store.rs', '            .ok_or_else(|| LoopError::Io(format!("bad path {}", path.display())))?;',
     _unjudged('`unwrap_or("")`')),
    ('crates/axon-loop/src/store.rs', '            .ok_or_else(|| LoopError::Io("no parent".into()))?;',
     _unjudged('`unwrap_or(path)`')),
    ('crates/axon-loop/src/store.rs', 'pub const LEDGER_KEY_ENV: &str = "AXON_ATTEST_KEY";',
     _A95['const_path']),
    ('crates/axon-loop/src/tasks.rs', '        .ok_or_else(|| LoopError::Io(format!("store corrupt: task manifest {r} missing")))?;',
     _dominated('an empty text is not a record: `strict_record("")` is an error, and it is the next statement')),
    ('crates/axon-loop/src/tel.rs', '        .ok_or_else(|| LoopError::Refused(format!("{what} exceeds 2^53-1")))?;',
     _observed('a_cost_total_beyond_2_53_is_never_summarized')),
    ('crates/axon-loop/src/tel.rs', 'pub const EXECUTION_COST_BASIS: &str =',
     _A95['const_text']),
    ('crates/axon-os/src/cli.rs', '        if let Err(e) = std::fs::create_dir_all(&out) {',
     _OSO),
    ('crates/axon-os/src/cli.rs', '        std::env::set_var("AXON_KILL_FILE", &kf);',
     _OSO),
    ('crates/axon-os/src/cli.rs', '        std::env::set_var("AXON_KILL_FILE", &kill_file);',
     _OSO),
    ('crates/axon-os/src/cli.rs', '        std::env::set_var("AXON_AUDIT_LEDGER", &ledger);',
     _OSO),
    ('crates/axon-os/src/cli.rs', '            std::env::remove_var("AXON_KILL_FILE");',
     _OSO),
    ('crates/axon-os/src/cli.rs', '    if kill_file_path.is_some() {\n        std::env::remove_var("AXON_KILL_FILE");',
     _OSO),
    ('crates/axon-os/src/cli.rs', '        // Clean up env var.\n        std::env::remove_var("AXON_KILL_FILE");',
     _OSO),
    ('crates/axon-os/src/cli.rs', '    if let Err(e) = std::fs::create_dir_all(&out) {\n        eprintln!("axon-os run: cannot create',
     _OSO),
    ('crates/axon-os/src/runtime.rs', '            // what the profile promises it will not do.\n            cmd.env(',
     _OSO),
    ('crates/axon-psv/src/lib.rs', 'pub const LAUNCH_MANIFEST_SCHEMA: &str = "axon-launch-manifest/2";',
     _A95['const_tag']),
    ('crates/axon-psv/src/lib.rs', 'pub const GUEST_VERDICT_SCHEMA: &str = "axon-guest-verdict/2";',
     _A95['const_tag']),
    ('crates/axon-psv/src/lib.rs', 'pub const GUEST_POLICY_SCHEMA: &str = "axon-vm-mmds/1";',
     _A95['const_tag']),
    ('crates/axon-psv/src/lib.rs', 'pub const COMPLETION_SCHEME: &str = "axon-guest-completion/1";',
     _A95['const_tag']),
    ('crates/axon-psv/src/lib.rs', 'pub const PROTECTED_PROFILE: &str = "linux-microvm-protected";',
     _A95['const_text']),
    ('crates/axon-psv/src/lib.rs', 'const MKFS_LOST_FOUND: &str = "lost+found";',
     _A95['const_text']),
    ('crates/axon-psv/src/lib.rs', 'pub const PREFLIGHT_OBSERVATION_SCHEMA: &str = "axon-preflight-observation/1";',
     _A95['const_tag']),
    ('crates/axon-workspace-recipe/src/lib.rs', '            ".." => depth = depth.checked_sub(1).ok_or_else(esc)?,',
     _observed('tests::a_link_with_an_empty_or_absolute_or_escaping_target_is_refused')),
    ('crates/axon-workspace-recipe/src/lib.rs', '                .ok_or_else(|| ImportRefusal::NonUtf8(path.clone()))?',
     _dominated('an empty link target reaches `check_link`, which refuses it (the empty symlink target, M2451)')),
    ('crates/axon-fabric/src/privileged_launcher.rs', '                    .mode(mode)',
     _A95['create_mode_window']),
]

TERM_EXEMPT += [
    ("crates/axon-fabric/src/custodian.rs", "!m.is_file()",
     "UNREACHABLE (checkable): `m` is the metadata of `/proc/<pid>/exe` opened as a File, and the kernel executes only a regular file (execve(2) refuses anything else with EACCES), so the type term cannot fire for a process that is running; survey (amendment 95): replacing it with `false` left the whole axon-fabric suite green"),
    ("crates/axon-fabric/src/privileged_launcher.rs", "!is_dir(&st)",
     "DOMINATED (checkable): `walk_open` opens every component with DIR_FLAGS (O_DIRECTORY), so `st` is the fstat of a directory descriptor and the type term cannot fire; survey (amendment 95): replacing it with `false` left the whole axon-fabric suite green"),
    ("crates/axon-fabric/src/sealed_exec.rs", "id.size < 0",
     "DOMINATED (checkable): the second term `id.size as u64 > MAX_BYTES` refuses every negative size (an i64 cast to u64 wraps above MAX_BYTES), so the sign term refuses nothing the next does not; survey (amendment 95): replacing it with `false` left the whole axon-fabric suite green"),
    ("crates/axon-loop/src/evl.rs", "VerificationResult::Passed",
     "REMAINDER (killed only by incidental tests; survey, amendment 95): replacing this term with `false` fails at least 12 axon-loop tests (the first, `a6_forged_admission_record_is_refused`, panics on an unwrap of the refusal admission then makes, not on an assertion about this term), so it is observed but no test names it as its own attack: not counted killed"),
]


def load_rows():
    spec = importlib.util.spec_from_file_location("mut", os.path.join(ROOT, "scripts/v022_g01_mutations.py"))
    mut = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mut)
    # A retired STALE row names text that is gone by definition (its
    # replacement, an ACTIVE row, mutates the guard's current form).
    stale = set(getattr(mut, "STALE_REFACTORED", {}))
    return [r for r in mut.MUTATIONS if r[0] not in stale]


def _skip_rust_token(t, i):
    """The index after the Rust lexical unit at t[i] when it is a comment, a
    string (incl. raw and byte strings) or a char literal, else None."""
    n = len(t)
    c = t[i]
    if t.startswith("//", i):
        j = t.find("\n", i)
        return n if j < 0 else j
    if t.startswith("/*", i):
        depth, j = 1, i + 2
        while j < n and depth:
            if t.startswith("/*", j):
                depth, j = depth + 1, j + 2
            elif t.startswith("*/", j):
                depth, j = depth - 1, j + 2
            else:
                j += 1
        return j
    m = re.compile(r'(?:b?r(#*)")').match(t, i) if c in "br" else None
    if m:
        end = '"' + m.group(1)
        j = t.find(end, m.end())
        return n if j < 0 else j + len(end)
    if c == '"' or (c == "b" and t.startswith('b"', i)):
        j = i + (2 if c == "b" else 1)
        while j < n and t[j] != '"':
            j += 2 if t[j] == "\\" else 1
        return min(j + 1, n)
    if c == "'":
        if t.startswith("'\\", i):
            j = t.find("'", i + 3)
            return n if j < 0 else j + 1
        if i + 2 < n and t[i + 2] == "'":
            return i + 3
        return i + 1  # a lifetime
    return None


def _cfg_test_extent(t, i):
    """The end offset of the item (or statement) an attribute at t[i] governs:
    its brace extent when it has a body, else its terminating `;`. A real
    matcher: braces inside strings, chars and comments do not count
    (amendment 74)."""
    n, depth, j = len(t), 0, i
    while j < n:
        k = _skip_rust_token(t, j)
        if k is not None:
            j = max(k, j + 1)
            continue
        c = t[j]
        if c in "([{":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif c == "}":
            depth -= 1
            if depth <= 0:
                return j + 1
        elif c == ";" and depth <= 0:
            return j + 1
        j += 1
    return n


# `cfg(all(test, ..))` compiles only in a test build too (amendment 76: the psv
# crate's xattr test module, whose helper calls were read as production);
# `cfg(any(test, feature = ..))` and `cfg(not(test))` are production code.
CFG_TEST = re.compile(r"^[ \t]*#\[cfg\((?:test|all\(test,[^\]\n]*\))\)\][ \t]*$", re.M)


def code_lines(text):
    """The file's lines with every `#[cfg(test)]` item BLANKED (its attribute
    through its brace extent, or its `;` for `mod tests;`): tests are not
    guards, and nothing else is dropped. Line numbers are preserved.
    Amendment 74: this used to return `lines[:i]` at the FIRST cfg(test) line
    followed by `mod tests`, discarding everything after it to the end of the
    file: production code after the test module (axon-os approval.rs's
    `authorize`) and anything placed below an empty `#[cfg(test)] mod tests`
    was invisible."""
    out = text
    pieces = []
    pos = 0
    for m in CFG_TEST.finditer(text):
        if m.start() < pos:
            continue
        end = _cfg_test_extent(text, m.end())
        pieces.append((m.start(), end))
        pos = end
    lines = text.split("\n")
    hidden = set()
    for a, b in pieces:
        first, last = text.count("\n", 0, a), text.count("\n", 0, b)
        hidden.update(range(first, last + 1))
    return ["" if i in hidden else l for i, l in enumerate(lines)]


def cfg_test_only_files(dirs):
    """Source files only a test build compiles: declared `#[cfg(test)] mod X;`
    in a sibling file (the module file is test code, not a protected path)."""
    out = set()
    pat = re.compile(r"#\[cfg\(test\)\]\s*(?:pub(?:\([a-z]+\))?\s+)?mod\s+(\w+)\s*;")
    for d in dirs:
        for f in _rs_under(d):
            base = os.path.dirname(f)
            for name in pat.findall(open(os.path.join(ROOT, f)).read()):
                out.add(os.path.normpath(os.path.join(base, f"{name}.rs")))
                out.add(os.path.normpath(os.path.join(base, name, "mod.rs")))
    return out


def line_of(text, offset):
    return text.count("\n", 0, offset)


# Amendment 58 (extended): a refusal is also a TAIL expression (`Err(…)` as
# an arm's or a block's value, no `return`) and a call of a refusal
# constructor (`refused(`, `fail(`, `shape(` in the loop, the runner's
# `refused(`, the verdict's `unknown(`). A definition of one is not a site.
# Amendment 61: and the interpreter's `panic(` (a Flow refusal: the seal
# edges and conform.rs refuse that way; `panic!(` is not it).
# C9 round 4b, OBSERVER workstream (amendment 68): the observer service, the
# helper's --observe relay and Fabric's relay client. Every decision has a row
# (M1520-M1549, M531 for the shared caller gate); these sites are named below.
OSV = "crates/axon-fabric/src/observer_service.rs"
OBS = "crates/axon-fabric/src/observer.rs"
PHC = "crates/axon-fabric/src/protected_host.rs"
EXEMPT += [
    (OSV, "        if self.schema != CONFIG_SCHEMA {",
     "operator-authored field of the operator-owned /etc/axon/observer.json (read through "
     "read_operator_file, the helper config's walk, M585-M589): a version tag"),
    (OSV, "            if !plain_absolute(p) {",
     "operator-authored fields of the operator-owned /etc/axon/observer.json (M585-M589); on the "
     "protected route the socket must EQUAL the path systemd bound (activated_listener, M703)"),
    (OSV, "        if self.test_paths.is_some() {",
     "operator-authored field of the operator-owned config; a protected observer never reads it "
     "(Sources::operator() is the fixed operator paths, chosen by the binary's protected arm)"),
    (OSV, "        if r.schema != REPLY_SCHEMA {",
     "authored by the pinned axon-observer: every byte of the reply is attributed by the kernel "
     "to a process executing the operator's pin (M1542, M1483/M1489), which writes REPLY_SCHEMA "
     "only; the relayed observation is then verified at Fabric and at the root spend"),
    (OSV, "        if !r.ok {",
     "RE-REPORTED: the pinned observer's own refusal (each of its decisions is its own row), "
     "carried to the relay's report; nothing is relayed"),
    (OSV, "    if !f.metadata().map(|m| m.is_file()).unwrap_or(false) {",
     "fails closed: a measurement that cannot be taken (the operator's file is not a regular "
     "file) signs nothing; no input chooses success (the paths come from operator files)"),
    (OSV, "    if bytes.len() as u64 > max {",
     "fails closed: an operator-owned file (the profile manifest) over the fixed bound is not "
     "measured, so nothing is signed; the path comes from the operator's config, never a request"),
    (OSV, "            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {",
     "NAMED ROW M1540: the decision is create_new on the record (a second observation of one "
     "nonce meets AlreadyExists); removing create_new is M1540, killed by the replay attack"),
    (OSV, '            Err(e) => return Err(format!("observation record: {e}")),',
     "OS error creating the record in the observer's own 0700 store: nothing is signed (fails "
     "closed)"),
    (OSV, "            Err(e) => self.reply(Err(e)),",
     "NOTHING TO ADMIT: SO_PEERCRED failed, so there is no peer uid for decide() to judge"),
    (OBS, "    if r.schema != pl::OBSERVE_REPORT_SCHEMA {",
     "authored by the pinned helper Fabric executed from its verified descriptor (open_verified, "
     "the host config's privileged_launcher pin): a version tag; whatever it relays is verified "
     "next by verify_observation (M192, M193, M195, M329) and again at the root spend"),
    (OBS, '        _ => Err(format!(\n            "the privileged launcher relayed no observation: {}",',
     "RE-REPORTED: the helper's own refusal (each of its decisions is its own row); no "
     "observation, so nothing is launched"),
    (PL, "        if !plain || !is_hex64(&s.sha256) {",
     "operator-authored fields of the operator-owned helper config (M585-M589); a pin that is not "
     "a sha256 matches no program, and every reply is compared with it (M1483, M1542)"),
    (PL, "        Err(why) => (ObserveReport::refused(why), EXIT_REFUSED),",
     "RE-REPORTED: authenticated() or the relay refused (each decision its own row: M531, M1543, "
     "M1544, M1545, the observer's M1520-M1549); nothing is relayed"),
    (PL, "        if libc::fstat(1, &mut st) != 0 {",
     "OS error: the helper's own stdout is not open, so there is no reply channel to judge and "
     "nothing is served (fails closed); no input chooses success"),
    (PL, '            Err(e) => return Err(format!("/proc/{pid}/fd: {e}")),',
     "OS error reading another process's descriptor table (a root helper can read them all): the "
     "holders of the reply pipe cannot be counted, so nothing is served (fails closed); the "
     "decision over what the scan finds is M2050, the scan itself M2053"),
    (PL, "    if raw < 0 {",
     "OS error from pidfd_open (the parent is gone, or the kernel has no pidfds): the running "
     "Fabric cannot be measured, so nothing is relayed (fails closed)"),
    (PL, '    if !still_parent() {\n        return Err(format!(\n            "the helper\'s parent (pid {ppid}) exited: the running',
     "a race that fails closed (the parent exited before the pidfd named it, and this process was "
     "reparented); not deterministically reachable, and a reparented helper's new parent (init or "
     "a subreaper) is refused by the uid rule (M1544) unless it is the Fabric uid's own"),
    (PL, '    if !still_parent() {\n        return Err(format!(\n            "the helper\'s parent (pid {ppid}) exited while',
     "a race that fails closed (the parent exited between naming it and reading its status and "
     "executable); not deterministically reachable; the pidfd pins the process, so a recycled pid "
     "cannot answer"),
    (PHC, '                    Some(_) => {\n                        return Err(bad(\n                            "observer.command:',
     "NAMED ROW M1547: the arm above decides (a TEST-TRUST build alone takes the in-uid program); "
     "this is its other branch, and making that arm match in a production build is M1547, killed "
     "by the production Fabric attack"),
    (BIN, "                serde_json::to_string(&pl::ObserveReport::refused(why)).unwrap_or_default(),",
     "NOT A SITE: the body of the binary's `refuse` constructor in observe mode; each use of "
     "refuse( is its own site"),
]

# ── C9 round 9, EQGATE5 (amendment 98) ──────────────────────────────────────
EXEMPT += [
    ("crates/axon-fabric/src/submit.rs", '            path: PathBuf::from("/usr/bin/axon (guest rootfs)"),',
     "UNUSED (checkable): the guest interpreter's `RegisteredExecutable.path` is a LABEL. `executable_digest(id, &e)` "
     "hashes only the id and `e.sha256`; the only reader of `exe.path` is `host_executor` (`&exe.path`), which runs "
     "only in the `else` of `let local = if is_linux { None } else { .. }`, i.e. never for the protected profile this "
     "line serves. Any other value for the path changes no decision (survey, amendment 98: `/bin/true` here left "
     "the whole axon-fabric suite green)"),
]



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


# Amendment 71: the refusal forms the files the crate rule brings in use. A
# compile refusal (`Diagnostic::error(`: the resolver's sealed-module E0004),
# a process exit with a code that is not the literal 0 (the guest's PID 1 and
# the `axon test` entry refuse that way), and a call of a FILE-LOCAL refusal
# closure: `let NAME = |..| .. Err(..)` on one line makes every later `NAME(`
# in that file a site (conform.rs's `kind_ok`), the definition itself not;
# and so is a call of a file-local DIVERGING refusal constructor: a `fn NAME(..)
# -> !` or `let NAME = |..| -> !` whose first lines exit with a non-zero
# literal code (axon-custodian's `die`, the `axon test` key reader's `fail`).
# Amendment 81 (C9 round 4c, eqgate): a refusal done BY THE KERNEL through an
# open flag constructs no Err and returns no verdict, so no form above saw it:
# the root helper's snapshot of its inputs (`O_NOFOLLOW`), the observer's key
# read ("a symlink is never followed"), `create_new(true)` /
# `RENAME_NOREPLACE` (refuse an existing file), each removable alone with the
# whole root-run suite green. Every USE of such a flag in non-test code is a
# site (a line site, guard block as for `Err(`): the flags are an enumerated
# family because the kernel's atomic-refusal vocabulary is (open(2), renameat2(2),
# mount(2), the `*at` calls), and the sweep that derived this list
# (every in-scope file, every `O_*` / `AT_*` / `MS_*` / `RENAME_*` / `create_new`
# token) is repeated by `an_open_flag_form_is_a_site`'s control: the unedited
# tree holds, so a flag nobody rows or exempts fails the gate.
# One alternative per line: each is the target of its own gate row.
OPEN_FLAG = re.compile(
    r"\bO_NOFOLLOW\b|"
    r"\bO_EXCL\b|"
    r"\bcreate_new\(\s*true\s*\)|"
    r"\bRENAME_NOREPLACE\b|"
    r"\bAT_SYMLINK_NOFOLLOW\b|"
    r"\bO_DIRECTORY\b|"
    r"\bMS_(?:NOSUID|NODEV|NOEXEC|RDONLY|BIND|PRIVATE|SLAVE|REC)\b"
)
# Amendment 87 (C9 round 6, eqgate2): the same blindness one level up. A guard
# expressed as a file or directory PERMISSION MODE (the completion secret's
# 0o400, the trial cache's 0o700), a `prctl` flag (PR_SET_NO_NEW_PRIVS,
# PR_SET_PDEATHSIG, PR_SET_DUMPABLE), a privilege drop, a resource limit, a
# signal disposition or a `pre_exec` hook builds no `Err` either, and each could
# be weakened alone with every suite green (round 6: 0o400 -> 0o644, 0o700 ->
# 0o755, the NO_NEW_PRIVS call under `if false`, the PDEATHSIG call deleted).
# The forms were derived by sweeping every in-scope file for the syscall and
# permission vocabulary (mode literals, set_permissions/from_mode, chmod family,
# mkdirat, umask, chown family, prctl, setrlimit, setsid/setpgid/process_group,
# the setuid family and setgroups, signal/sigaction/sigprocmask/kill/killpg,
# fcntl, pre_exec, namespace and mount calls, capset/seccomp); a use of any of
# them in non-test code is a site. One alternative per line: each is its own row.
PRIV_FORM = re.compile(
    r"\.mode\(\s*[^)\s]|"
    r"\bset_permissions\(|\bPermissions::from_mode\(|(?<!fn )\bset_mode\(|"
    r"\blibc::f?chmod(?:at)?\(|\blibc::[lf]?chown(?:at)?\(|"
    r"\blibc::mkdirat\(|\blibc::umask\(|"
    r"\blibc::prctl\(|"
    r"\blibc::setrlimit\(|"
    r"\blibc::(?:setsid|setpgid|setpgrp)\(|\.process_group\(|"
    r"\blibc::set(?:re|res)?[ug]id\(|\blibc::(?:setgroups|initgroups)\(|"
    r"\blibc::(?:signal|sigaction|sigprocmask|pthread_sigmask|kill|killpg)\(|"
    r"\blibc::fcntl\(|"
    r"\.pre_exec\(|"
    r"\blibc::syscall\(|"
    r"\boom_score_adj\b|"
    r"\blibc::(?:unshare|setns|chroot|pivot_root|mount|umount2?|capset|seccomp)\(|\bSECCOMP_MODE_"
)
# Amendment 91 (C9 round 7, eqgate3): the same blindness for what a child is
# BUILT with and what bounds a read. A child's environment, its stdio, the
# option list handed to git, a size cap and a descriptor-inheritance flag build
# no `Err` either, and each could be removed alone with every suite green
# (round 7: the helper's `quiet` Stdio::null -> inherit, git's GIT_OPTIONAL_LOCKS,
# core.hooksPath and --no-includes, the runner's `n.min(room)` output cap). The
# forms were derived by sweeping every in-scope file for the builder and OS
# boundary vocabulary: Command env/env_clear/envs/env_remove/current_dir/stdin/
# stdout/stderr, the git `-c` option lists and GIT_* variables, process-state
# libc calls (setitimer, chdir, close_range, setpriority, sched_*, personality,
# flock, setsockopt, dup2/dup3, pipe2, socket/socketpair/accept4, unlink/rename/
# link), descriptor flags (O_CLOEXEC, SOCK_CLOEXEC, O_NONBLOCK, FD_CLOEXEC),
# and the cap forms (`.min(<bound>)` slices, `.take(<bound>)`/`.truncate(..)` of a
# MAX_* bound; a comparison `x > MAX_*` is not one: it is followed by its own
# refusal, which the Err forms already see). One alternative per line: each group is its own gate row.
BUILD_FORM = re.compile(
    r"\.(?:env_clear|env_remove|envs)\(|(?<![\w:])(?:cmd|c|command)\.env\(|^\s*\.env\(|\.current_dir\(|\.(?:stdin|stdout|stderr)\(\s*(?:std::process::)?Stdio::(?:null|inherit)\(|"
    r"\"-c\"|\bcore\.(?:fsmonitor|hooksPath|excludesFile|attributesFile|checkStat|trustCtime)\b|\bprotocol\.allow\b|\bsafe\.directory\b|\"--no-includes\"|\"GIT_[A-Z_]+\"|"
    r"\blibc::(?:setitimer|chdir|fchdir|close_range|setpriority|sched_\w+|personality|flock|setsockopt|dup[23]?|pipe2|socketpair|accept4|socket|unlinkat?|renameat2?|linkat?)\(|\bSYS_close_range\b|\b(?:O_CLOEXEC|SOCK_CLOEXEC|FD_CLOEXEC|SOCK_NONBLOCK)\b|"
    r"\.min\(\s*(?:room|cap|limit|bound|max)\w*\s*\)|\.take\(\s*[a-z_]*(?:limit|cap|bound|max)\w*|\.(?:min|take|truncate)\([^)]*\bMAX_[A-Z_]+"
)
# Amendment 95 (C9 round 8, eqgate4): the same blindness for a guard expressed as
# a VALUE or as an atomic refusal. `create_dir` refuses an existing directory
# and `create_dir_all` does not (the RunDir's own comment says an existing dir is
# never reused); a uid/gid CONSTANT or a `drop: Some((UID, GID))` handed to a
# privilege primitive is a decision the primitive's own row never sees (the
# runner's TEST_UID 65534 -> 0 kept the suite green); an `expected_*sha256`
# field is the pin a verifier compares against (`String::new()` matches
# nothing and nothing failed); `env::set_var`/`remove_var` is how the guest
# PID 1 builds the child's policy (AXON_BUDGET_TOKENS, AXON_PRINCIPAL). One
# alternative per line: each is its own gate row. Amendment 98 (eqgate5): a struct-literal
# FIELD whose value is an ABSOLUTE path literal (`secret: PathBuf::from("/in/job/..")`) is a
# decision about WHERE a trusted component looks, handed to a config struct; the runner's
# guest_config left five of them unobserved with the suite green.
VALUE_FORM = re.compile(
    r"\bcreate_dir(?:_all)?\(|"
    r"\bDirBuilder\b|\blibc::mkdir\(|"
    r"\bdrop:\s*(?:Some\(|None\b)|"
    r"\b[A-Z][A-Z0-9_]*(?:UID|GID)\b|"
    r"^(?!\s*pub\b)\s*expected_\w*(?:sha256|digest|hash)\w*\s*:(?!\s*(?:&|String\s*[,)]|Vec<|\[u8|u\d+\b|Option<))|"
    r"\benv::set_var\(|"
    r"\benv::remove_var\(|"
    r"^\s*\w+\s*:\s*(?:std::path::)?(?:PathBuf::from|Path::new)\(\s*\"/"
)
# Amendment 95: a SINGLE-LINE `.ok_or(..)?` / `.ok_or_else(..)?` is a refusal
# too: the absent value is an error the function returns (amendment 76 saw only
# a closure calling refused()/bad()/fail()). 
OKOR_FORM = re.compile(r"\.ok_or(?:_else)?\(.*\)\s*\?")
# A decision expressed as `Some("reason")` / `Some(format!(..))` (evo::propose's
# exclusion chain, submit's `problem = Some(..)`), as a VALUE: a pattern
# (`Some("x") =>`, `== Some("x")`, `matches!(.., Some("x"))`) reads one.
SOME_REASON = re.compile(r"\bSome\(\s*(?:\"|format!\()")
DIAG = re.compile(r"\bDiagnostic::error\(")
EXIT = re.compile(r"\bprocess::exit\((?!\s*0\s*\))")
LOCAL_CTOR_DEF = re.compile(r"\blet\s+(\w+)\s*=\s*(move\s+)?\|[^|]*\|.*\bErr\(")
DIVERGING_DEF = re.compile(r"(?:\bfn\s+(\w+)\s*(?:<[^>]*>)?\s*\([^)]*\)|\blet\s+(\w+)\s*=\s*(?:move\s+)?\|[^|]*\|)\s*->\s*!")
EXIT_NONZERO = re.compile(r"\bexit\(\s*[1-9]")
# Amendment 91: a diverging fn that exits with a COMPUTED code (`exit(code)`,
# axon-fabric's `refuse`) is a refusal constructor as well: the code is the
# refusal's kind, and no caller passes 0.
EXIT_COMPUTED = re.compile(r"\bexit\(\s*[A-Za-z_]")
DIVERGING_BODY = 8


def local_ctors(lines):
    """Names of the file-local refusal constructors (amendment 71): one-line
    `Err` closures, and diverging fns/closures that exit non-zero."""
    out = {m.group(1) for l in lines for m in [LOCAL_CTOR_DEF.search(l)] if m}
    defs = []
    for i, l in enumerate(lines):
        m = DIVERGING_DEF.search(l)
        if m:
            defs.append((m.group(1) or m.group(2), i))
    for name, i in defs:
        if any(EXIT_NONZERO.search(x) or EXIT_COMPUTED.search(x) for x in lines[i:i + DIVERGING_BODY]):
            out.add(name)
    # Amendment 91: a diverging fn or closure whose body DELEGATES to a
    # refusal (`refuse(..)`, `die(..)`, another registered diverging name) is a
    # refusal constructor too, to a fixed point. Round 7: `let bad = |why|
    # -> ! { refuse(..) }` delegates through `refuse`, whose own exit takes a
    # variable code, so the 13 `bad(` calls of the signer loader were invisible.
    changed = True
    while changed:
        changed = False
        for name, i in defs:
            if name in out:
                continue
            body = "\n".join(lines[i:i + DIVERGING_BODY])
            calls = {m.group(0)[:-1] for m in re.finditer(r"(?<![\w.])\w+\(", body)} - {name}
            if any(c in out for c in calls):
                out.add(name)
                changed = True
    return out


def _some_reason_is_value(l):
    """Whether a `Some("..")`/`Some(format!(..))` on `l` builds a value (a
    reason a decision returns) rather than matching or comparing one."""
    for m in SOME_REASON.finditer(l):
        before = l[:m.start()]
        if re.search(r"(==|!=|\bmatches!\(.*|\blet|\|)\s*$", before) or before.rstrip().endswith("|"):
            continue
        depth, j = 0, m.start() + 4
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
        if re.match(r"(=>|\|)", rest):
            continue
        return True
    return False


def is_form(l):
    """Whether `l` is a site by a FORM (amendment 81/87/91/95: an atomic-refusal
    flag, a privilege or permission call, a child build or size cap, a value or
    directory creation, a single-line `ok_or(..)?`, a `Some(reason)` value): the
    decision IS this line, not a condition above it."""
    c = l.split("//")[0]
    return bool(BUILD_FORM.search(c) or PRIV_FORM.search(c) or OPEN_FLAG.search(c) or VALUE_FORM.search(c)
                or OKOR_FORM.search(c) or _some_reason_is_value(c))


def is_own_line_form(l):
    """A form whose decision is THE LINE ITSELF: a row must change this line (a
    row on a neighbouring condition of the same block does not credit it: the
    `create_dir` inside the block of a uid check, the `.take(MAX_*)` in the block
    of an owner test). A `Some(reason)` value is not one: its decision is the
    condition above it."""
    c = l.split("//")[0]
    return bool(BUILD_FORM.search(c) or PRIV_FORM.search(c) or OPEN_FLAG.search(c) or VALUE_FORM.search(c)
                or OKOR_FORM.search(c))


def is_site(l, ctors=()):
    return is_condition_site(l, ctors) or is_form(l)


def is_condition_site(l, ctors=()):
    """A site whose decision is the condition ABOVE it (`return Err(..)`,
    `refuse(..)`, a refusal constructor call, ...), as opposed to a FORM."""
    if SITE.search(l) or DIAG.search(l) or EXIT.search(l):
        return True
    if any(err_is_expression(l, m.start()) for m in ERR.finditer(l)):
        return True
    if (any(re.search(rf"(?<![\w.]){re.escape(c)}\(", l) for c in ctors)
            and not LOCAL_CTOR_DEF.search(l) and not DIVERGING_DEF.search(l)):
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


def anchor_region(lines, text, f, bad):
    """The REGIONS span of `f` as (first, last) line, or None (BAD) when an
    anchor is not in the file exactly once."""
    a, b = REGIONS[f]
    if text.count(a) != 1 or text.count(b) != 1:
        bad.append(f"{f}: a REGIONS anchor is not in the file exactly once: the region scans nothing")
        return None
    return (line_of(text, text.index(a)), line_of(text, text.index(b)))


# Amendment 74 (C9 round 4c, gate): a PREDICATE PRIMITIVE is a decision too.
# Round 4c (EQUIVALENCE) found axon_psv::keyed_outcome -- the one function
# that decides whether a result line carries K's token -- with no row and no
# exemption, invisible to the gate: it refuses by `return None` and
# `.then_some(`, and SITE matched neither. A list of more forms would miss the
# next one (a tail `a == b`, a `.filter(`), so the rule is per FUNCTION: every
# in-scope function whose declared return type is `bool` or `Option<..>`
# DECIDES (its "no" is `false` / `None`, in whatever form) and is one site
# whose guard block is its whole body. It is covered by a row whose edit
# changes a line of that body, or exempt by an anchor in it (by convention its
# head line). A bool -> Option conversion inside any other function
# (`.then_some(` / `.then(`) is a line site of its own. `.ok_or(` /
# `.ok_or_else(` is NOT a site: it converts an absence some other code
# decided (a predicate primitive, scanned by this rule, or a lookup, where the
# absent value is nothing to admit) into the refusal, and decides nothing.
FN_HEAD = re.compile(r"^(\s*)(?:pub(?:\([a-z]+\))?\s+)?(?:const\s+)?(?:unsafe\s+)?(?:extern\s+\"C\"\s+)?fn\s+(\w+)")
# Amendment 81: a function DECIDES by `bool`/`Option<..>` (amendment 74) and
# also by `Result<bool, _>` (`learning_eligible`, `git_data::reaches`,
# schema `type_matches`), by an `i32` status (`sealed_exec::check_unchanged`,
# the `exit_code` mappings) and by `ExitCode` (a CLI's `return
# ExitCode::from(8)` refusals). Swept over the in-scope crates: the other
# integer returns (`u8`, `u32`, `i64`) are data (a discriminant, a port read, a
# uid, a clock), not a status.
PRED_RET = re.compile(r"^(bool|Option\s*<|Result\s*<\s*bool\s*[,>]|i32\b|(?:std::process::)?ExitCode\b)")
PRED_LINE = re.compile(r"\.then_some\(|\.then\(")


def _return_type(sig):
    """The declared return type of a signature (the text after the `->` at
    parenthesis depth 0), or ''."""
    depth = 0
    for k, c in enumerate(sig):
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif c == "-" and depth == 0 and sig[k + 1:k + 2] == ">":
            return sig[k + 2:].strip()
    return ""


def predicate_fns(lines):
    """(head, last) line spans of the functions in `lines` that decide by
    `bool` or `Option<..>` (amendment 74). A declaration without a body (a
    trait item) is not one."""
    out = []
    for i, l in enumerate(lines):
        m = FN_HEAD.match(l)
        if not m:
            continue
        sig, j = "", i
        while j < len(lines) and j < i + 16:
            sig += lines[j] + " "
            if "{" in lines[j] or lines[j].rstrip().endswith(";"):
                break
            j += 1
        if "{" not in sig:
            continue
        if not PRED_RET.match(_return_type(sig.split("{")[0])):
            continue
        body = sig[sig.index("{"):]
        if body.count("{") == body.count("}") and lines[j].rstrip().endswith("}"):
            out.append((i, j))
            continue
        close = m.group(1) + "}"
        for k in range(j + 1, len(lines)):
            if lines[k] == close:
                out.append((i, k))
                break
    return out


# Amendment 76 (C9 round 4c, admit): a VERDICT is a decision too.
# Round 4c integration found the axon-os admission chain (`gate::admit`, the
# supervisor's isolation guard, the grant algebra it intersects) with no row
# and no site: it refuses by RETURNING an enum value (`Admission::Deny {..}`,
# `Verdict::Denied {..}`), and neither `Err(` nor a `bool`/`Option` return
# type is in that. The refusal vocabulary is not a list this script keeps, it
# is DERIVED from the in-scope code: every enum (not an `*Error`, whose
# refusals travel inside `Err(` and are sites already) that has a variant named
# for refusing (STRONG_NEG) or is itself named for deciding (VERDICT_NAME) is a
# VERDICT TYPE, its NEGATIVE variants being the refusal-named ones plus the
# "no verdict" ones (WEAK_NEG: Unknown, TimedOut, ...). Forms:
#
#   * a CONSTRUCTION of a negative variant (`Verdict::Denied {..}`,
#     `return Verdict::Denied..`, a match arm's value, `Self::Deny` inside the
#     enum's own impl) is a line site, guard block as for `Err(`. A pattern is
#     not one (`Admission::Deny { .. } = admit(..)`, a match arm, `matches!`,
#     `==`): that READS a verdict somebody else decided;
#   * a function whose declared return type is a verdict type, or a struct named
#     for deciding (`*Verdict`, `*Decision`, `*Outcome`, `*Admission`), also
#     through one `Result<..>`/`Option<..>`/`Vec<..>`, DECIDES by data flow as
#     well as by constructor (`let status = f(); Verdict { status }`) and is a
#     site of its own, its body the guard block (exempt by an anchor on its
#     head line, like a predicate primitive);
#   * a call of a HELPER CONSTRUCTOR is a site and its definition is not: a
#     function returning a verdict type whose every construction is negative
#     (`fn deny(..) -> Admission { Admission::Deny {..} }`), the verdict
#     analogue of `refused(` / `fail(`.
#
# `?` and `.ok_or(` stay non-sites: they convert an absence or an error some
# other scanned decision already made (this script says why at the `ok_or`
# paragraph above); an INLINE decision before an `ok_or` is a predicate in a
# closure (`.filter(`, `.take_if(`, `.is_some_and(`, `.then(`) and is a site by
# INLINE_PRED below.
STRONG_NEG = re.compile(
    r"^(Den(y|ied)|Refus\w*|Reject\w*|Veto\w*|Blocked|\w*Violation\w*|Invalid\w*|Unauthori[sz]ed|"
    r"Unverified|\w*Mismatch|Tamper\w*|Forbid\w*|Halted|Unsupported|FailClosed|Fail(ed|ure)?|"
    r"Malformed|\w*Exhausted|\w*Bound|Corrupt\w*|Tripped|Flagged)$")
WEAK_NEG = re.compile(r"^(Unknown|OutcomeUnknown|Inconclusive|TimedOut|Cancel(l)?ed|Conflict|Stale\w*|Unresolved)$")
VERDICT_NAME = re.compile(r"Verdict|Decision|Outcome|Admission")
INLINE_PRED = re.compile(r"\.(filter|take_if|is_some_and|is_ok_and|is_none_or)\(")


def blank_non_code(text):
    """`text` with comments and string/char literal CONTENTS blanked (same
    length, newlines kept): what is left is code to match on."""
    out, i, n = [], 0, len(text)
    while i < n:
        j = _skip_rust_token(text, i)
        if j is None:
            out.append(text[i])
            i += 1
            continue
        j = max(j, i + 1)
        seg = text[i:j]
        if seg.startswith("//") or seg.startswith("/*"):
            out.append(re.sub(r"[^\n]", " ", seg))
        elif seg[0] in "\"'br" and len(seg) > 2:
            out.append(seg[0] + re.sub(r"[^\n]", " ", seg[1:-1]) + seg[-1])
        else:
            out.append(seg)
        i = j
    return "".join(out)


def _match_close(t, i):
    """Index just past the group opened at t[i] (one of `(` `[` `{`), or len(t)."""
    depth, j, n = 0, i, len(t)
    while j < n:
        if t[j] in "([{":
            depth += 1
        elif t[j] in ")]}":
            depth -= 1
            if depth == 0:
                return j + 1
        j += 1
    return n


def _enum_variants(clean):
    """(name, [variant names]) for each `enum` item of already-blanked code."""
    for m in re.finditer(r"^[ \t]*(?:pub(?:\([a-z]+\))?\s+)?enum\s+(\w+)[^{;]*\{", clean, re.M):
        end = _match_close(clean, m.end() - 1)
        body, parts, depth, cur = clean[m.end():end - 1], [], 0, ""
        for ch in body:
            if ch in "{([<":
                depth += 1
            elif ch in "})]>":
                depth -= 1
            if depth == 0 and ch == ",":
                parts.append(cur)
                cur = ""
            else:
                cur += ch
        parts.append(cur)
        names = []
        for v in parts:
            v = re.sub(r"#\[[^\]]*\]", "", v)
            mm = re.match(r"\s*(\w+)", v)
            if mm:
                names.append(mm.group(1))
        yield m.group(1), names


_VERDICTS = None


def verdict_types():
    """(enums, structs): verdict enum -> its NEGATIVE variants, and the struct
    names that decide. Derived once from every in-scope file's non-test code."""
    global _VERDICTS
    if _VERDICTS is not None:
        return _VERDICTS
    enums, structs = {}, set()
    for f in in_scope_files():
        clean = blank_non_code("\n".join(code_lines(open(os.path.join(ROOT, f)).read())))
        for name, vs in _enum_variants(clean):
            if name.endswith("Error"):
                continue
            strong = {v for v in vs if STRONG_NEG.match(v)}
            if strong or VERDICT_NAME.search(name):
                enums.setdefault(name, set()).update(strong | {v for v in vs if WEAK_NEG.match(v)})
        for m in re.finditer(r"^[ \t]*(?:pub(?:\([a-z]+\))?\s+)?struct\s+(\w+)", clean, re.M):
            if VERDICT_NAME.search(m.group(1)):
                structs.add(m.group(1))
    enums = {k: v for k, v in enums.items() if v}
    # A `type Judged = (Outcome, String, ..);` alias of a verdict is one (a
    # function returning it decides): resolved to a fixpoint over every file.
    aliases = {}
    for f in in_scope_files():
        clean = blank_non_code("\n".join(code_lines(open(os.path.join(ROOT, f)).read())))
        for m in re.finditer(r"^[ \t]*(?:pub(?:\([a-z]+\))?\s+)?type\s+(\w+)\s*=\s*([^;]+);", clean, re.M):
            aliases[m.group(1)] = m.group(2)
    grew = True
    while grew:
        grew = False
        for name, rhs in aliases.items():
            if name not in structs and _decides_return(rhs, enums, structs):
                structs.add(name)
                grew = True
    _VERDICTS = (enums, structs)
    return _VERDICTS


def _impl_self_map(clean):
    """[(first, last, Name)] character extents of `impl .. Name {`: what `Self` means there."""
    out = []
    for m in re.finditer(r"^[ \t]*impl\b(?:<[^>{]*>)?\s+(?:[\w:<>, ]+?\s+for\s+)?(\w+)[^{;]*\{", clean, re.M):
        out.append((m.start(), _match_close(clean, m.end() - 1), m.group(1)))
    return out


def arm_lhs_ranges(clean):
    """(first, last) character extents of every match ARM's left side (pattern
    and guard): from the arm's separator back to its `=>`. A variant inside one
    is matched, not built, whatever else shares the pattern (a tuple pattern
    `(SafetyState::Violation { .. }, _, _) =>` is one)."""
    out = []
    for m in re.finditer(r"=>", clean):
        j, depth, k = m.start(), 0, m.start() - 1
        while k >= 0 and clean[k] in " \t\n":
            k -= 1
        first_closer = k
        while k >= 0:
            c = clean[k]
            if c in ")]}":
                if c == "}" and depth == 0 and k != first_closer:
                    break
                depth += 1
            elif c in "([{":
                if depth == 0:
                    break
                depth -= 1
            elif c in ",;" and depth == 0:
                break
            k -= 1
        out.append((k + 1, j))
    return out


def _is_pattern(clean, p, q, arms=()):
    """Whether the variant path at clean[p:q] is matched, not built: inside an
    arm's left side, a `let`/`matches!`/alternative pattern, an `if let`
    scrutinee pattern, or a `==` comparison."""
    if any(a <= p < b for a, b in arms):
        return True
    ls = clean.rfind("\n", 0, p) + 1
    before = clean[ls:p]
    if re.search(r"\b(let|ref|mut)\s*$", before) or re.search(r"(==|!=|\|)\s*$", before):
        return True
    k = clean.rfind("matches!(", max(0, p - 400), p)
    if k >= 0 and ";" not in clean[k:p] and _match_close(clean, k + len("matches!")) > p:
        return True
    r = q
    while r < len(clean) and clean[r] in " \t":
        r += 1
    if r < len(clean) and clean[r] in "({":
        r = _match_close(clean, r)
    rest = clean[r:r + 12]
    rest = re.sub(r"^[\s)]+", "", rest)
    return bool(re.match(r"(=>|\||if\b|=(?!=)|==|!=)", rest))


def verdict_constructions(clean, enums):
    """Line numbers (0-based) at which a NEGATIVE variant of a verdict enum is
    built (amendment 76)."""
    selfmap = _impl_self_map(clean)
    arms = arm_lhs_ranges(clean)
    out = []
    for m in re.finditer(r"\b(\w+)::(\w+)\b", clean):
        enum, var = m.group(1), m.group(2)
        if enum == "Self":
            enum = next((n for a, b, n in selfmap if a <= m.start() < b), enum)
        if var not in enums.get(enum, ()):
            continue
        if _is_pattern(clean, m.start(), m.end(), arms):
            continue
        out.append((m.start(), clean.count("\n", 0, m.start())))
    return out


def _decides_return(ret, enums, structs):
    """Whether a declared return type `ret` is a verdict (amendment 76),
    looking through one Result/Option/Vec."""
    t = ret.strip()
    if t.startswith("("):
        parts, depth, cur = [], 0, ""
        for c in t[1:-1]:
            depth += c in "<(["
            depth -= c in ">)]"
            if c == "," and depth == 0:
                parts.append(cur)
                cur = ""
            else:
                cur += c
        return any(_decides_return(x, enums, structs) for x in parts + [cur])
    m = re.match(r"(?:Result|Option|Vec|Box)\s*<(.*)>\s*$", t, re.S)
    if m:
        inner, depth, cut = m.group(1), 0, len(m.group(1))
        for k, c in enumerate(inner):
            depth += c in "<(["
            depth -= c in ">)]"
            if c == "," and depth == 0:
                cut = k
                break
        t = inner[:cut].strip()
    t = re.sub(r"^&\s*(mut\s+)?(\'\w+\s+)?", "", t)
    name = re.match(r"(?:\w+::)*(\w+)", t)
    return bool(name) and (name.group(1) in enums or name.group(1) in structs)


def _guard_start(lines, i):
    """The first line of the guard block of a line site at `i`: the nearest
    opener above it, at most MAX_UP lines up. Amendment 74: a guard block never
    reaches into the function above (it used to: the nearest opener could be
    another fn's)."""
    for j in range(i, max(-1, i - MAX_UP - 1), -1):
        if j < i and FN_HEAD.match(lines[j]):
            break
        if OPENER.search(lines[j]):
            return j
    return i


def _fn_spans(cl):
    """(head, last, name, return type) for each fn with a body in blanked lines."""
    out = []
    for i, l in enumerate(cl):
        m = FN_HEAD.match(l)
        if not m:
            continue
        sig, j = "", i
        while j < len(cl) and j < i + 16:
            sig += cl[j] + " "
            if "{" in cl[j] or cl[j].rstrip().endswith(";"):
                break
            j += 1
        if "{" not in sig:
            continue
        end = j
        if not (sig.count("{") == sig.count("}") and cl[j].rstrip().endswith("}")):
            close = m.group(1) + "}"
            end = next((k for k in range(j + 1, len(cl)) if cl[k] == close), None)
            if end is None:
                continue
        out.append((i, end, m.group(2), _return_type(sig.split("{")[0])))
    return out


def _trait_extents(clean):
    """Character extents of `trait X { .. }`: a default method there is not a
    free helper (its callers are methods)."""
    return [(m.start(), _match_close(clean, m.end() - 1))
            for m in re.finditer(r"^[ \t]*(?:pub(?:\([a-z]+\))?\s+)?trait\s+\w+[^{;]*\{", clean, re.M)]


_HELPERS = None
HELPER_BODY = 8  # a helper constructor is a SHORT fn: a function that decides is a site of its own


def refusal_helpers():
    """{file: (verdict helpers, Err helpers)}: names of the free functions, per
    in-scope file, that only REFUSE: a verdict-typed fn every construction in
    whose body is a negative variant (`fn bad(..) -> Verdict { Verdict::Malformed {..} }`),
    and a short fn whose body builds an `Err(..)` and has no `Ok`/`?`/branch.
    A call of one is a refusal site; its definition is not (the verdict
    analogue of `refused(`/`fail(`, which this script names, and of
    `local_ctors`, which is per file)."""
    global _HELPERS
    if _HELPERS is not None:
        return _HELPERS
    enums, structs = verdict_types()
    out = {}
    for f in in_scope_files():
        vh, eh = set(), set()
        text = "\n".join(code_lines(open(os.path.join(ROOT, f)).read()))
        clean = blank_non_code(text)
        cl = clean.split("\n")
        traits = _trait_extents(clean)
        offs = [0]
        for l in cl:
            offs.append(offs[-1] + len(l) + 1)
        spans = _fn_spans(cl)
        defined = collections.Counter(n for _, _, n, _ in spans)
        for a, b, n, rt in spans:
            # A name defined twice in one file is a platform/feature variant
            # (`#[cfg(not(unix))] fn check_operator_owned` beside the real
            # one): which one a call reaches is not the text's to say, so
            # neither is a helper.
            if any(x <= offs[a] < y for x, y in traits) or defined[n] > 1:
                continue
            body = "\n".join(cl[a:b + 1])
            if b - a > HELPER_BODY:
                continue
            if _decides_return(rt, enums, structs):
                built = [m for m in re.finditer(r"\b(\w+)::(\w+)\b", body) if m.group(1) in enums]
                if built and all(m.group(2) in enums[m.group(1)] for m in built):
                    vh.add(n)
            elif (re.search(r"\bErr\(", body)
                  and not re.search(r"\bOk\(|\?|\bSome\(|\bif\b|\bmatch\b|\bfor\b|\bwhile\b", body[body.index("{"):])):
                eh.add(n)
        if vh or eh:
            out[f] = (vh, eh)
    _HELPERS = out
    return _HELPERS


def inline_predicate_sites(cl):
    """(first, last) lines of a statement that DECIDES in a closure and then
    refuses on the result: `x.filter(|k| k.len() == 32).ok_or(..)?`. `.ok_or(`
    alone converts an absence some scanned decision made; with an inline
    predicate before it, the predicate IS the decision and nothing else scans
    it. The statement runs from the previous `;`/`{`/`}` to the `ok_or`."""
    clean = "\n".join(cl)
    out = []
    for m in re.finditer(r"\.(ok_or|ok_or_else)\(", clean):
        st = max(clean.rfind(";", 0, m.start()), clean.rfind("{", 0, m.start()), clean.rfind("}", 0, m.start()))
        seg = clean[st + 1:m.start()]
        p = INLINE_PRED.search(seg)
        if p:
            out.append((clean.count("\n", 0, st + 1 + p.start()), clean.count("\n", 0, m.start())))
    return out


def verdict_sites(lines, regions=None, f=None):
    """The amendment-76 sites of a file: (guard first, last, reported) lines.
    Constructions of a negative verdict variant, calls of a helper
    constructor and an inline predicate refused through `ok_or` are line
    sites; a function deciding by a verdict return type is a block site (head
    reported, like a predicate primitive)."""
    enums, structs = verdict_types()
    vh, eh = refusal_helpers().get(f, (set(), set()))
    clean = blank_non_code("\n".join(lines))
    cl = clean.split("\n")
    fns = _fn_spans(cl)
    decides = [(a, b, n) for a, b, n, rt in fns if _decides_return(rt, enums, structs)]
    helper_def = [(a, b) for a, b, n, rt in fns if n in vh or n in eh]
    # A LOCAL closure that builds a refusal (`let refused = |why| LinuxRun {
    # outcome: LinuxOutcome::Refused, .. };`) is a helper constructor too: its
    # body is the definition, its calls are the sites.
    closure_names = set()
    for m in re.finditer(r"\blet\s+(\w+)\s*=\s*(?:move\s+)?\|[^|\n]*\|\s*(?:-> \w+\s*)?(?=\w+\s*\{|\{)", clean):
        k = clean.index("{", m.end())
        end = _match_close(clean, k)
        body = clean[k:end]
        if any(x.group(2) in enums.get(x.group(1), ()) for x in re.finditer(r"\b(\w+)::(\w+)\b", body)):
            a, b = clean.count("\n", 0, m.start()), clean.count("\n", 0, end)
            helper_def.append((a, b))
            closure_names.add(m.group(1))
    in_helper = lambda i: any(a <= i <= b for a, b in helper_def)
    in_region = lambda i: regions is None or any(x <= i <= y for x, y in regions)
    out = []
    for _, i in verdict_constructions(clean, enums):
        if not in_helper(i) and in_region(i):
            out.append((_guard_start(lines, i), i, i, "line"))
    # A call is a site in the file that defines the helper (a bare name: another
    # file's `bad` is another function) and, anywhere, through its module path
    # (`backend::check_operator_owned(`).
    calls = []
    if vh | eh | closure_names:
        calls.append(re.compile(r"(?<![\w.:])(" + "|".join(sorted(map(re.escape, vh | eh | closure_names))) + r")\("))
    for g, (v2, e2) in refusal_helpers().items():
        if g != f and v2 | e2:
            stem = os.path.splitext(os.path.basename(g))[0]
            calls.append(re.compile(rf"\b{re.escape(stem)}::(" + "|".join(sorted(map(re.escape, v2 | e2))) + r")\("))
    for i, l in enumerate(cl):
        if (any(c.search(l) for c in calls) and not FN_HEAD.match(l) and not in_helper(i) and in_region(i)
                and not l.strip().startswith("//")):
            out.append((_guard_start(lines, i), i, i, "line"))
    for a, b in inline_predicate_sites(cl):
        if in_region(a) and not in_helper(a):
            out.append((min(_guard_start(lines, b), a), b, b, "line"))
    for a, b, n in decides:
        if n not in vh and in_region(a):
            out.append((a, b, a, "verdict"))
    return out


def sites(text, f=None, bad=None):
    """The refusal sites of `f` as (first, last, reported) lines: `first` to
    `last` is the guard block a row or an exemption must reach, `reported` the
    line the report names."""
    lines = code_lines(text)
    regions = None
    if f in SCOPE_FN_REGIONS or f in REGIONS:
        regions = fn_regions(lines, SCOPE_FN_REGIONS[f]) if f in SCOPE_FN_REGIONS else []
        if f in REGIONS:
            r = anchor_region(lines, text, f, bad if bad is not None else [])
            if r:
                regions.append(r)
    out = []
    ctors = local_ctors(lines)
    for i, l in enumerate(lines):
        s = l.strip()
        # A `use` declaration names TEST_TRUST_BUILD; it reads nothing.
        if s.startswith("//") or s.startswith("use ") or not (is_site(l, ctors) or PRED_LINE.search(l)):
            continue
        if regions is not None and not any(a <= i <= b for a, b in regions):
            continue
        kind = "form" if (is_own_line_form(l) and not is_condition_site(l, ctors) and not PRED_LINE.search(l)) else "line"
        at = i
        if kind == "form" and re.search(r"\bDirBuilder\b", l.split("//")[0]) and not re.search(r"\.create\(", l):
            # A builder chain: the decision is the `.create(..)` that makes the
            # directory, where a row's edit (`.recursive(true)`) lands.
            for j in range(i + 1, min(len(lines), i + 8)):
                if re.search(r"\.create\(", lines[j]):
                    at = j
                    break
        out.append((_guard_start(lines, i), at, at, kind))
    for a, b in predicate_fns(lines):
        if regions is not None and not any(x <= a <= y for x, y in regions):
            continue
        out.append((a, b, a, "predicate"))
    out.extend(verdict_sites(lines, regions, f))
    return out


def changed_lines(text, old, new):
    """The file lines (0-based) a row's edit CHANGES (amendment 74), from a
    line diff of `old` and `new` placed where `old` is in `text`: a replaced or
    deleted old line, and for an insertion the old line it is inserted before
    (the last line when it is appended). Leading and trailing newlines of
    `old` are not lines of the edit: an `old` that ends in a newline used to
    reach the NEXT line, and so covered a site the edit never touched."""
    import difflib
    lead = len(old) - len(old.lstrip("\n"))
    a0 = line_of(text, text.index(old) + lead)
    o = old.strip("\n").split("\n")
    nn = new[lead:] if new[:lead] == "\n" * lead else new
    n = nn.rstrip("\n").split("\n")
    out = set()
    for tag, i1, i2, _, _ in difflib.SequenceMatcher(None, o, n, autojunk=False).get_opcodes():
        if tag == "equal":
            continue
        if i1 == i2:
            out.add(a0 + min(i1, len(o) - 1))
        else:
            out.update(range(a0 + i1, a0 + i2))
    return out


def in_scope_files():
    """The rule's file set (amendment 61), sorted: every .rs under SCOPE_DIRS,
    SCOPE_FILES, and the SCOPE_FN_REGIONS files."""
    found = set(SCOPE_FILES) | set(SCOPE_FN_REGIONS) | set(REGIONS)
    for d in SCOPE_DIRS:
        found.update(_rs_under(d))
    return sorted(found - linked_only_as_library() - cfg_test_only_files(SCOPE_DIRS))


# Amendment 95 (C9 round 8, eqgate4): a COMPOUND guard is judged PER TERM and a
# CONSTANT a guard reads is a site of its own.
#
# TERMS. `if a || b || c { return Err(..) }` was credited whole as soon as one
# row's edit touched the block (the round-8 review: custodian.rs's `!m.is_file()`
# and the uid term, observer_service.rs's `!is_file()`, each replaceable by
# `false` with every suite green because the row on the MODE term credited the
# guard). Now each top-level `||` / `&&` term of the opening condition of a
# row-covered line site must be reached by a row's edit (the edit's changed
# characters overlap the term), or the edit must make the WHOLE condition a
# constant (`if false && (a || b)`: the reducer below evaluates `false`/`true`
# literals through `&&`/`||`, so `false && a || b`, which leaves `b`, is NOT
# constant), or the term must be named on TERM_EXEMPT with a fact. A term
# exemption that matches no uncredited term is stale and refused.
#
# CONSTS. A `const`/`static` whose NAME appears inside the guard block of a
# refusal site (outside strings and comments) is a decision VALUE the site's
# own row never sees (DEFAULT_OBSERVATION_MAX_AGE_S 300 -> 86400000, a table
# entry replaced by a duplicate). Its definition (through the `;`) is a site:
# a row must change it, or an EXEMPT anchor must lie on it. The definition of a
# const read only through a local (`let m = MAX; if x > m`) or in a function
# the rule does not see as a guard is the stated REMAINDER (amendment 95).
CONST_DEF = re.compile(r"^\s*(?:pub(?:\([a-z]+\))?\s+)?(?:const|static)\s+([A-Z][A-Z0-9_]*)\s*:(?!.*(?:Atomic|Mutex|OnceLock|LazyLock|Cell<))")
CONST_FILES_SKIPPED = re.compile(r"^crates/axon-core/src/(?!interp/conform\.rs)")


# A use that only REPORTS or exits (the message, the exit code) reads no
# decision value; a use anywhere else in a function that refuses does.
CONST_MESSAGE_USE = re.compile(r"\bErr\(|format!|println!|eprintln!|\bexit\(|\brefuse\(|panic!|\bexpect\(")
CONST_NOT_VALUE = re.compile(r"Atomic|Mutex|OnceLock|LazyLock|Cell<")


def _const_end(cl, i):
    for j in range(i, min(len(cl), i + 400)):
        if cl[j].rstrip().endswith(";"):
            return j
    return i


def _split_top(s, op):
    parts, depth, cur, i = [], 0, 0, 0
    while i < len(s):
        c = s[i]
        if c in "([{":
            depth += 1
        elif c in ")]}":
            depth -= 1
        elif depth == 0 and s.startswith(op, i):
            parts.append(s[cur:i])
            i += len(op)
            cur = i
            continue
        i += 1
    parts.append(s[cur:])
    return parts


def reduce_condition(s):
    """True/False when the boolean expression `s` is a constant once the
    `true`/`false` literals are propagated through `&&`/`||`/parentheses,
    else None (an atom that is anything else is unknown)."""
    s = s.strip()
    while s.startswith("(") and s.endswith(")"):
        d, whole = 0, True
        for k, c in enumerate(s):
            d += c == "("
            d -= c == ")"
            if d == 0 and k < len(s) - 1:
                whole = False
                break
        if not whole:
            break
        s = s[1:-1].strip()
    ors = _split_top(s, "||")
    if len(ors) > 1:
        v = [reduce_condition(x) for x in ors]
        return True if any(x is True for x in v) else (False if all(x is False for x in v) else None)
    ands = _split_top(s, "&&")
    if len(ands) > 1:
        v = [reduce_condition(x) for x in ands]
        return False if any(x is False for x in v) else (True if all(x is True for x in v) else None)
    return {"true": True, "false": False}.get(s)


def _cond_span(clean, start):
    """(begin, end) offsets of the `if` condition whose line starts at `start`
    in blanked text, or None (not an `if`, or an `if let`)."""
    m = re.compile(r"\s*(?:\}\s*else\s+)?if\s+").match(clean, start)
    if not m or clean.startswith("let", m.end()):
        return None
    i, depth, j = m.end(), 0, m.end()
    while j < len(clean):
        c = clean[j]
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif c == "{" and depth == 0 and not re.search(r"unsafe\s*$", clean[i:j]):
            break
        j += 1
    return i, j


def condition_terms(clean, start):
    """The top-level `||`/`&&` terms of the guard condition at `start` as
    (begin, end) offsets, or [] when it has fewer than two."""
    span = _cond_span(clean, start)
    if not span:
        return []
    i, j = span
    terms, depth, cur, k = [], 0, i, i
    while k < j:
        c = clean[k]
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif depth == 0 and clean.startswith(("||", "&&"), k):
            terms.append((cur, k))
            k += 2
            cur = k
            continue
        k += 1
    terms.append((cur, j))
    if len(terms) < 2:
        return []
    out = []
    for a, b in terms:
        while a < b and clean[a].isspace():
            a += 1
        while b > a and clean[b - 1].isspace():
            b -= 1
        out.append((a, b))
    return out


def edit_extent(text, old, new):
    """The (begin, end) characters of `text` the edit old -> new changes: the
    old text minus its common prefix and suffix with the new."""
    off = text.index(old)
    p = 0
    while p < min(len(old), len(new)) and old[p] == new[p]:
        p += 1
    q = 0
    while q < min(len(old), len(new)) - p and old[-1 - q] == new[-1 - q]:
        q += 1
    return off + p, off + len(old) - q


def uncredited_terms(text, clean, offs, g, by_rows):
    """Terms of the compound guard opening at line `g` that no covering row's
    edit reaches (see the TERMS paragraph). `by_rows` are registry rows."""
    terms = condition_terms(clean, offs[g])
    if not terms:
        return []
    spans = []
    for r in by_rows:
        # An edit that REMOVES the refusal (the `return Err(..)` the guard leads to,
        # turned into a no-op) disables every term at once.
        if (sum(is_condition_site(x) for x in r[3].split("\n"))
                > sum(is_condition_site(x) for x in r[4].split("\n"))):
            return []
        span = _cond_span(blank_non_code(text.replace(r[3], r[4], 1)), offs[g])
        if span and reduce_condition(blank_non_code(text.replace(r[3], r[4], 1))[span[0]:span[1]]) is not None:
            return []
        spans.append(edit_extent(text, r[3], r[4]))
    return [(a, b) for a, b in terms if not any(x <= b and y >= a for x, y in spans)]


_SITES_MEMO = {}


def sites_memo(f, text):
    k = (f, hash(text))
    if k not in _SITES_MEMO:
        _SITES_MEMO[k] = sites(text, f, [])
    return _SITES_MEMO[k]


def const_names_read_by_guards(scope):
    """{(defining file, NAME)} for every const/static of an in-scope file whose
    name appears (outside strings and comments) in the guard block of a refusal
    site of the same file, or, qualified by the file's module or imported by
    name, of another."""
    defs, used = {}, {}
    texts = {f: open(os.path.join(ROOT, f)).read() for f in scope}
    for f, t in texts.items():
        cl = code_lines(t)
        for i, l in enumerate(cl):
            m = CONST_DEF.match(l)
            if m and not CONST_FILES_SKIPPED.match(f):
                defs.setdefault(m.group(1), []).append((f, i))
    for f, t in texts.items():
        clean = blank_non_code(t).split("\n")
        uses = set()
        S = [x for x in sites_memo(f, t) if x[3] == "line"]
        site_lines = {i for _, i, _, _ in S}
        for fa, fb, _, _ in _fn_spans(clean):
            if not any(fa <= i <= fb for i in site_lines):
                continue
            for j in range(fa, fb + 1):
                l = clean[j]
                if j in site_lines or CONST_MESSAGE_USE.search(l):
                    continue
                for m in re.finditer(r"((?:\w+::)*)\b([A-Z][A-Z0-9_]{2,})\b", l):
                    uses.add((m.group(1), m.group(2)))
        used[f] = uses
    out = set()
    for name, where in defs.items():
        for f, i in where:
            stem = os.path.splitext(os.path.basename(f))[0]
            crate = f.split("/")[1].replace("-", "_")
            for g, uses in used.items():
                for q, n in uses:
                    if n != name:
                        continue
                    if g == f or any(x in (stem, crate) for x in q.strip(":").split("::")) or \
                            re.search(r"\buse\b[^;]*\b" + name + r"\b", texts[g]):
                        out.add((f, name))
    return out


def const_sites(f, text, names):
    cl = code_lines(text)
    out = []
    for i, l in enumerate(cl):
        m = CONST_DEF.match(l)
        if m and (f, m.group(1)) in names:
            out.append((i, _const_end(cl, i), i, "const"))
    return out


REMAINDER_SITES = []
OBSERVED_SITES = []


def remainder_category(reason):
    """The category of a REMAINDER exemption: the `_A95` key whose reason it is
    (okor_field, const_tag, ...), else `other` (a hand-written REMAINDER)."""
    for k, v in _A95.items():
        if v == reason:
            return k
    for prefix, k in (("REMAINDER (survey, amendment 98, fail-closed)", "okor_closed"),
                      ("REMAINDER (survey, amendment 98, unjudged)", "okor_unjudged"),
                      ("REMAINDER (survey, amendment 98, no default)", "okor_nodefault"),
                      ("REMAINDER (survey, amendment 98, off the protected route)", "okor_offroute")):
        if reason.startswith(prefix):
            return k
    return "other"


def remainder_summary(sites_):
    """(total, {category: count}) of the REMAINDER sites."""
    cats = {}
    for _, _, c in sites_:
        cats[c] = cats.get(c, 0) + 1
    return len(sites_), dict(sorted(cats.items(), key=lambda kv: (-kv[1], kv[0])))


def judge_file(f, rows, bad, const_names=frozenset()):
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
        spans.append((r[0], changed_lines(text, r[3], r[4])))
    row_by_id = {r[0]: r for r in rows if r[2] == f and text.count(r[3]) == 1}
    clean = blank_non_code(text)
    offs = [0]
    for l in lines:
        offs.append(offs[-1] + len(l) + 1)
    tex = [[a, an, reason, 0] for ef, an, reason in TERM_EXEMPT if ef == f for a in [None]]
    termseen = set()
    ex = []
    for ef, anchor, reason in EXEMPT:
        if ef != f:
            continue
        n = text.count(anchor)
        if n != 1:
            bad.append(f"exemption anchor occurs {n} times in {f}: {anchor!r}")
            continue
        ex.append([line_of(text, text.index(anchor)), anchor, reason, 0, 0, 0])
    covered = exempt = 0
    uncovered = []
    all_sites = list(sites(text, f, bad)) + const_sites(f, text, const_names)
    for g, i, at, kind in all_sites:
        # Amendment 74: a row covers a site only when a line its edit CHANGES
        # lies in the site's guard block (it used to be enough that the row's
        # OLD text overlapped the block, plus the line after it).
        lo = i if kind == "form" else g
        by = [rid for rid, ch in spans if any(lo <= c <= i for c in ch)]
        # A predicate primitive's exemption is anchored on its HEAD line
        # (amendment 74): an anchor in its body belongs to a line site there.
        ex_hit = [e for e in ex if ((g <= e[0] <= i) if kind in ("line", "const", "form") else e[0] == g)]
        for e in ex_hit:
            e[3] += 1
            # Amendment 91: an exemption is STALE only when every site whose
            # block holds it is covered by a row (it used to be dropped as soon
            # as ANY covered site's block held it, which the amendment-87 gate
            # narrowed to its own line, and so stopped flagging the exemption of
            # a guard a row had since taken over: the survey found four).
            e[4 if by else 5] += 1
        if by and kind == "line" and g not in termseen:
            termseen.add(g)
            for a, b in uncredited_terms(text, clean, offs, g, [row_by_id[x] for x in by]):
                hit = [t for t in tex if t[1] in text[a:b]]
                if hit:
                    for t in hit:
                        t[3] += 1
                    continue
                bad.append(f"{f}:{line_of(text, a) + 1}: compound guard term with no row and no exemption "
                           f"(the row on another term of the same `||`/`&&` does not credit it): "
                           f"{' '.join(text[a:b].split())}")
        if by:
            covered += 1
        elif ex_hit:
            exempt += 1
            # Amendment 98: an exemption whose reason says REMAINDER is a guard no test
            # observes; it is COUNTED (by category), never claimed covered.
            for e in ex_hit:
                if "REMAINDER" in e[2]:
                    REMAINDER_SITES.append((f, g + 1, remainder_category(e[2])))
                    break
                if e[2].startswith("OBSERVED-NOT-ROWED"):
                    OBSERVED_SITES.append((f, g + 1, "observed"))
                    break
        else:
            what = {"line": f"{lines[g].strip()} ... {lines[i].strip()}",
                    "const": f"{lines[g].strip()} (a constant a guard reads)",
                    "form": f"{lines[g].strip()} ... {lines[i].strip()}",
                    "predicate": f"{lines[g].strip()} (a predicate primitive: it decides by bool/Option)",
                    "verdict": f"{lines[g].strip()} (a function deciding by a verdict return type)"}[kind]
            uncovered.append(f"{f}:{at + 1}: refusal site with no row and no exemption: {what}")
    for t in tex:
        if t[3] == 0:
            bad.append(f"{f}: term exemption matches no uncredited guard term: {t[1]!r}")
    for line, anchor, _, hits, cov, sole in ex:
        if hits == 0:
            bad.append(f"{f}:{line + 1}: exemption matches no refusal site: {anchor!r}")
        elif cov and not sole:
            bad.append(f"{f}:{line + 1}: exempt ({anchor!r}) yet every site it lies in is covered by a row: "
                       "drop the exemption")
    return covered, exempt, uncovered


# ── Amendment 98 (C9 round 9, eqgate5): Python guards ───────────────────────
# The gate read Rust only, and the build environment (`scripts/guest_build_env.py`,
# on the claim's own list of protected paths) is Python: about half of its guard
# lines had no row, and no test set AXON_GUEST_BUILD_UID to 0 or to the builder's own
# uid, so `int(raw) == 0` could go with every test green. A Python REFUSAL SITE,
# found from the syntax tree, never from text: a call of `fail(`/`die(`/`refuse(`,
# a `sys.exit(<message or usage>)` (not the propagation of a callee's status), a
# `raise <exc>` (not a bare re-raise), and a `return` of a non-empty message (alone
# or last in a tuple: the `*_problem(s)` convention, whose empty string means
# "nothing wrong"). The primitive `fail` itself is not a site. A site's GUARD is the
# nearest enclosing `if`/`elif`/`else`/`while`/`for` in the same function (its first
# line, through the site's last line), or the site's own lines when unconditional.
# It is COVERED when a row's edit CHANGES a line of that block; otherwise it needs an
# EXEMPT anchor (text in the block, exactly once in the file) or is a stated
# REMAINDER. What this does NOT see: a guard that is an expression value
# (`return bool(...)`, `all(...)`), a refusal by a subprocess's exit status, the
# shell scripts.
PY_SCOPE = ["scripts/guest_build_env.py"]
PY_EXEMPT = []   # (file, function, n, fragment, kind, reason): the table below
PY_EXEMPT += [
    ("scripts/guest_build_env.py", 'pinned_channel', 1, 'fail("rust-toolchain.toml names no channel: the guest is bui', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'pinned_channel no rust-toolchain.toml' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'rustup', 1, 'fail(f"no rustup at {p}: the pinned toolchain cannot be reso', 'OBSERVED',
     'OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) "rustup none in the builder\'s home" of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own'),
    ("scripts/guest_build_env.py", 'toolchain', 1, 'fail(f"rustup cannot resolve {tool} of the pinned toolchain ', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'toolchain rustup fails but prints a path' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'ancestors_of', 1, 'return [], f"the build parent {parent} is not an absolute pa', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'ancestors_of relative path' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'ancestors_of', 2, 'return out, f"{p}: {e.strerror}"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'ancestors_of missing directory' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'ancestors_problem', 1, 'return "it records no build parent"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'ancestors_problem parent not a string' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'ancestors_problem', 2, 'return "its recorded build-parent ancestors are not the pare', 'OBSERVED',
     'OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) "ancestors_problem ancestors are not the parent\'s" of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own'),
    ("scripts/guest_build_env.py", 'copy_tracked_tree', 1, 'fail(f"cannot list the tree\'s tracked files: {r.stderr.decod', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'copy_tracked_tree not a git tree' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'copy_tracked_tree', 2, 'fail(f"tracked path {rel} is neither a file nor a symlink")', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'copy_tracked_tree a tracked path that is a FIFO' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'private_toolchain', 1, 'fail("cargo and rustc are not of one toolchain directory")', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'private_toolchain cargo and rustc of two toolchains' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'private_toolchain', 3, 'fail(f"cannot make the private toolchain copy: {r.stderr.dec', 'REMAINDER',
     "REMAINDER (no test observes it): the failure of `cp -a` to make the private toolchain copy (disk full, a read-only target) cannot be provoked without breaking the host; the copy's own bytes are judged by the next line"),
    ("scripts/guest_build_env.py", 'private_toolchain', 4, 'fail("the private toolchain copy is not the pinned toolchain', 'REMAINDER',
     'REMAINDER (no test observes it): the copy is made by `cp -al`/`cp -a` from the same source and the digests are then compared; a stand-in copier that corrupts a file would need to replace /bin/cp, which no test does'),
    ("scripts/guest_build_env.py", 'measure_problem', 1, 'return f"cannot re-measure the build\'s tools: {e}"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'write measure_problem' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'git_clone', 1, 'fail(f"cannot clone the tree: {r.stderr.strip()[-300:]}")', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'git_clone not a git tree' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'git_clone', 2, 'fail("the clone is not at the tree\'s HEAD")', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'git_clone an empty repository has no HEAD to be at' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'reach_problem', 1, 'return (f"{cur} (mode {oct(st.st_mode & 0o7777)}) cannot be ', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'kernel a build parent the build uid cannot traverse | reach_problem a directory closed to other uids' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'reap_build_processes', 1, 'fail(f"processes of the build uid {uid} survived SIGKILL aft', 'REMAINDER',
     'REMAINDER (no test observes it): a process of the build uid that survives SIGKILL (an uninterruptible sleep) cannot be made without a kernel fault'),
    ("scripts/guest_build_env.py", 'committed_file', 1, 'fail(f"cannot read {rel} from the committed tree: {r.stderr.', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'committed_file a path the committed tree does not hold' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", '_begin', 1, 'fail(why)', 'REMAINDER',
     'REMAINDER (no test observes it): inside `_begin` (amendment 99: `begin` now only takes the build-uid lock and calls `_begin`), which needs the real pinned toolchain (rustup), root, and a built private copy; the begin route is exercised end to end by guest_build_env.rs, whose cases name the REFUSALS of begin, not this `why`'),
    ("scripts/guest_build_env.py", '_begin', 2, 'fail("cargo\'s effective configuration for the guest build is', 'REMAINDER',
     "REMAINDER (no test observes it): inside `_begin` (real toolchain): cargo's effective config at begin; judged end to end by guest_build_env.rs's committed-config cases, which refuse through `effective_config`, not through a removal of this line alone"),
    ("scripts/guest_build_env.py", 'build_uid_lock', 2, 'fail(f"cannot open the build-uid lock file in {LOCK_DIR}', 'REMAINDER',
     "REMAINDER (amendment 99, no test observes it alone): turns the OSError of the O_NOFOLLOW open of the lock file (a symlink there) into a refusal with a message; the symlink is planted by the_build_uid_lock_is_root_owned_and_begin_holds_it (LOCK_IS_SYMLINK) and refused, but without this handler the OSError propagates uncaught (a traceback, a non-zero exit, no lock taken), so removing the handler changes the message and the exit path and not the outcome; the refusal itself is the open flag, whose removal would be caught by the symlink case only because /dev/null is not a regular file (the check after the open)"),
    ("scripts/guest_build_env.py", 'first', 1, 'fail(f"{\' \'.join(cmd)} failed: {r.stderr.strip()[-300:]}")', 'REMAINDER',
     'REMAINDER (no test observes it): inside `begin` (real toolchain): a failed `git`/`cargo` child; no test makes the real toolchain fail'),
    ("scripts/guest_build_env.py", 'proof_key', 1, 'return None, "the record names no usable proof id or build p', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'proof_key the parent is not a string' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'proof_key', 3, 'return None, f"the build\'s proof key is not at {path} ({e.st', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'proof_key no key at the pinned place' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'proof_key', 5, 'return None, f"{path} holds no key"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'proof_key a key too short to be one' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'builder_pin', 1, 'return None, f"the builder pin {path} is not the operator\'s:', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'builder_pin a pin that is not an operator file' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'builder_pin', 2, 'return None, f"the builder pin {path} is unreadable: {e}"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'builder_pin an unreadable pin' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'proof_problems', 1, 'return f"the {what} record carries no builder proof (a recor', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'proof_problems a record that is not an object' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'proof_problems', 2, 'return "no operator builder identity to judge the proof agai', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'proof_problems no builder' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'proof_problems', 4, 'return f"the {what} record\'s proof cannot be checked: {why}"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'proof_problems no key at the pinned parent' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'write', 2, 'fail(why)', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'write a record whose key is not usable' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'load', 1, 'fail(f"{path} is not a controlled build record: {why}")', 'OBSERVED',
     'OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) "load a record that is not a controlled build\'s" of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own'),
    ("scripts/guest_build_env.py", 'cargo_step', 2, 'fail(why)', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'cargo_step tools that changed since begin' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'run_cargo', 1, 'fail("usage: cargo RECORD [--rustflags FLAGS] -- CARGO-ARGS.', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'run_cargo without the -- separator' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'host_build', 1, 'fail(f"{outdir} exists: the host build writes a new director', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'host_build an output directory that exists' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'host_build', 2, 'fail(f"the controlled host build failed (cargo exit {rc}); i', 'REMAINDER',
     'REMAINDER (no test observes it): inside `host_build`, after a real cargo build: a failed host build'),
    ("scripts/guest_build_env.py", 'host_build', 3, 'fail(f"the controlled host build produced no {n}")', 'REMAINDER',
     'REMAINDER (no test observes it): inside `host_build`, after a real cargo build: a missing host binary'),
    ("scripts/guest_build_env.py", 'host_record_problems', 1, 'return (f"{outdir} holds no readable {HOST_RECORD} ({e}): th', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'host_record_problems no record in the directory | main check-host-record names why a directory is not a host build' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'host_record_problems', 2, 'return f"{HOST_RECORD} is not a {HOST_SCHEMA} record"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'host_record_problems another schema' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'host_record_problems', 3, 'return f"the host build record is not a controlled build\'s: ', 'OBSERVED',
     'OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) "host_record_problems a record that is not a controlled build\'s" of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own'),
    ("scripts/guest_build_env.py", 'dist_record', 2, 'fail(f"dist/rootfs.sqfs ({got}) is not the controlled assemb', 'OBSERVED',
     'OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) "dist_record a rootfs that is not the controlled assembly\'s output" of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own'),
    ("scripts/guest_build_env.py", 'dist_problems', 1, 'return "the build record names no digest for every dist arti', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'dist_problems a build record naming not every dist artifact' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'rootfs', 2, 'fail(f"busybox --list failed: {lst.stderr.strip()[-300:]}")', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'rootfs a busybox that cannot list its applets' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'rootfs', 3, 'fail(f"no mksquashfs in {TOOL_PATH}")', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'rootfs no mksquashfs in the fixed directories' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'rootfs', 4, 'fail(f"mksquashfs failed ({r.returncode})")', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'rootfs a mksquashfs that fails' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'kernel', 1, 'fail(why)', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'kernel a build parent the build uid cannot traverse' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'kernel', 2, 'fail(f"the kernel build\'s host tools {missing} are not in {T', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'kernel a build without a required host tool' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'kernel', 3, 'fail("the kernel tarball did not extract")', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'kernel a tarball that does not extract' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'kernel', 4, 'fail(f"{\' \'.join(argv)} failed ({r.returncode})")', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'kernel a make step that fails' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'kernel', 5, 'fail("vmlinux not built")', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'line 529' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'entry_problems', 1, 'return "a recorded build is not an object"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'entry_problems an entry that is not an object' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'shape_problems', 1, 'return f"it is not a {SCHEMA} or {HOST_SCHEMA} record of a c', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'shape_problems a record that is not an object' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'shape_problems', 2, 'return "it records no toolchain identity or environment"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'shape_problems a toolchain that is not an object' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'operator_file_problem', 1, 'return f"{cur}: {e.strerror}"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'operator_file_problem missing file' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'operator_file_problem', 2, 'return f"{cur} is not a real {\'file\' if last else \'directory', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'operator_file_problem a symlink as the file' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'toolchain_pin_problems', 1, 'return f"the host-toolchain pin {pin_path} is not the operat', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'toolchain_pin a pin that is not an operator file' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'toolchain_pin_problems', 2, 'return f"the host-toolchain pin {pin_path} is unreadable: {e', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'toolchain_pin an unreadable pin' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'toolchain_pin_problems', 3, 'return f"the host-toolchain pin {pin_path} is not a {TOOLCHA', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'toolchain_pin a pin that is not an object' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'image_problems', 1, 'return "it records no build environment"', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'image_problems no build environment' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'main', 1, 'sys.exit(__doc__)', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'main check-host-record with an unknown option prints the usage' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'main', 4, 'fail(why)', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'main check-host-record with no flags and no operator builder pin' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'main', 5, 'fail(why)', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'main check-host-record names why a directory is not a host build' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'main', 6, 'fail("the host binaries\' build configuration is not the tree', 'REMAINDER',
     "REMAINDER (no test observes it): `check-host-build` runs the real toolchain's `cargo config get` over a clone"),
    ("scripts/guest_build_env.py", 'main', 9, 'sys.exit(__doc__)', 'OBSERVED',
     "OBSERVED-NOT-ROWED (survey, scripts/v022_py_guard_survey.py): removing this guard fails the case(s) 'main an unknown verb prints the usage and fails' of crates/axon-fabric/tests/guest_build_env_guards.rs; no row of its own"),
    ("scripts/guest_build_env.py", 'main', 10, 'fail(why)', 'REMAINDER',
     "REMAINDER (no test observes it): `toolchain-pin` refuses only when the operator's pin exists at its fixed /etc path (absent on every test host, and no test writes under /etc)"),
]


def py_sites(text):
    """[(guard first line, site first line, site last line, function, n)] (lines
    0-based; `n` counts the function's sites from 1 in source order) of the Python
    refusal sites of `text` (see the paragraph above)."""
    import ast

    def is_msg(v):
        if isinstance(v, ast.Constant):
            return isinstance(v.value, str) and v.value != ""
        return isinstance(v, ast.JoinedStr)

    out = []

    class V(ast.NodeVisitor):
        def __init__(self):
            self.fn, self.guards = [], []

        def visit_FunctionDef(self, n):
            self.fn.append(n.name)
            old, self.guards = self.guards, []
            self.generic_visit(n)
            self.guards = old
            self.fn.pop()

        visit_AsyncFunctionDef = visit_FunctionDef

        def _guarded(self, n, body):
            self.guards.append(n)
            for x in body:
                self.visit(x)
            self.guards.pop()

        def visit_If(self, n):
            self.visit(n.test)
            self._guarded(n, n.body + n.orelse)

        def visit_While(self, n):
            self._guarded(n, n.body + n.orelse)

        def visit_For(self, n):
            self._guarded(n, n.body + n.orelse)

        def rec(self, n):
            if self.fn and self.fn[-1] == "fail":
                return
            g = self.guards[-1].lineno if self.guards else n.lineno
            out.append((g - 1, n.lineno - 1, n.end_lineno - 1, self.fn[-1] if self.fn else ""))

        def visit_Call(self, n):
            f = n.func
            if isinstance(f, ast.Name) and f.id in ("fail", "die", "refuse"):
                self.rec(n)
            elif (isinstance(f, ast.Attribute) and f.attr == "exit" and isinstance(f.value, ast.Name)
                  and f.value.id == "sys" and n.args and not isinstance(n.args[0], ast.Call)):
                self.rec(n)
            self.generic_visit(n)

        def visit_Raise(self, n):
            if n.exc is not None:
                self.rec(n)
            self.generic_visit(n)

        def visit_Return(self, n):
            v = n.value
            if v is not None and (is_msg(v) or (isinstance(v, ast.Tuple) and v.elts and is_msg(v.elts[-1]))):
                self.rec(n)
            self.generic_visit(n)

    V().visit(ast.parse(text))
    per, named = {}, []
    for g, a, b, fn in sorted(set(out)):
        per[fn] = per.get(fn, 0) + 1
        named.append((g, a, b, fn, per[fn]))
    return named


def judge_py_file(f, rows, bad):
    """(covered, exempt, uncovered) of one Python file, as `judge_file` does for
    Rust. An exemption is keyed by (function, n-th site of that function, a
    fragment of the site's own text), so a line shift keeps it and a site added
    before it makes the fragment disagree (BAD), never silently re-attaches it.
    A PY_EXEMPT entry whose kind is OBSERVED says a survey removed this guard and
    a named test failed (no row of its own); REMAINDER says none did."""
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
        spans.append((r[0], set(changed_lines(text, r[3], r[4]))))
    ex = {(e[1], e[2]): [e, 0, 0, 0] for e in PY_EXEMPT if e[0] == f}
    covered = exempt = 0
    uncovered = []
    for g, a, b, fn, n in py_sites(text):
        by = [rid for rid, ch in spans if any(g <= c <= b for c in ch)]
        hit = ex.get((fn, n))
        site_text = "\n".join(lines[a:b + 1])
        if hit is not None:
            hit[1] += 1
            hit[2 if by else 3] += 1
            if hit[0][3] not in site_text:
                bad.append(f"{f}:{a + 1}: exemption ({fn}, {n}) names the fragment {hit[0][3]!r}, which is not "
                           "in the site's text: a site was added or moved, re-judge it")
        if by:
            covered += 1
        elif hit is not None:
            exempt += 1
            kind, reason = hit[0][4], hit[0][5]
            if kind == "REMAINDER":
                REMAINDER_SITES.append((f, g + 1, "py_guard"))
            else:
                OBSERVED_SITES.append((f, g + 1, kind))
        else:
            uncovered.append(f"{f}:{a + 1}: refusal site with no row and no exemption: "
                             f"{lines[g].strip()} ... {lines[a].strip()}")
    for key, (e, hits, cov, sole) in ex.items():
        if hits == 0:
            bad.append(f"{f}: exemption {key} matches no refusal site (a function renamed, or fewer sites than it names)")
        elif cov == hits:
            bad.append(f"{f}: exemption {key} ({e[3]!r}) yet the site is covered by a row: drop the exemption")
    return covered, exempt, uncovered


def check(without=(), freeze=False, out=print):
    """Run the gate. Returns the list of problems (empty: it holds). With
    `freeze`, a non-empty NOT_YET_SCANNED is itself a problem."""
    rows = [r for r in load_rows() if r[0] not in set(without)]
    bad = []
    del REMAINDER_SITES[:]
    del OBSERVED_SITES[:]
    scope = in_scope_files()
    for f in sorted(set(OUT_OF_SCOPE) | set(NOT_YET_SCANNED)):
        if f not in scope:
            bad.append(f"{f}: named in OUT_OF_SCOPE/NOT_YET_SCANNED but not in scope by the rule "
                       "(or gone): the table must not outlive its file")
        if f in OUT_OF_SCOPE and f in NOT_YET_SCANNED:
            bad.append(f"{f}: both OUT_OF_SCOPE and NOT_YET_SCANNED")
    cnames = const_names_read_by_guards([f for f in scope if f not in OUT_OF_SCOPE])
    for f in scope:
        if f in OUT_OF_SCOPE:
            out(f"{f}: OUT OF SCOPE ({OUT_OF_SCOPE[f]})")
            continue
        covered, exempt, uncovered = judge_file(f, rows, bad, cnames)
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
    for f in PY_SCOPE:
        covered, exempt, uncovered = judge_py_file(f, rows, bad)
        out(f"{f}: {covered} covered by a row, {exempt} exempt (Python guards)")
        bad.extend(uncovered)
    # Amendment 91: an exemption that names a row must name one that EXISTS.
    # 41 of 343 row-citing exemptions named a retired row (a four-cell record
    # stands behind it) and three named a row that was never there (M1088,
    # M1597, M2151): a reason resting on a row nobody can run is no reason.
    known = {r[0] for r in rows} | {r[0] for r in load_rows()}
    for ef, anchor, reason in EXEMPT:
        gone = sorted({m for m in re.findall(r"\bM\d+\b", reason) if m not in known}, key=lambda x: int(x[1:]))
        if gone:
            bad.append(f"{ef}: exemption {anchor[:50]!r} cites {gone}, which are not registry rows")
    for b in bad:
        out(f"BAD {b}")
    n, cats = remainder_summary(REMAINDER_SITES)
    out(f"OBSERVED-NOT-ROWED: {len(OBSERVED_SITES)} guards a survey removed with a named test failing and no row of "
        "their own (a measurement, not a row)")
    out(f"REMAINDER: {n} guard sites no test observes alone, COUNTED and NOT CLAIMED COVERED (no row, no checkable "
        f"exemption): " + ", ".join(f"{c} {k}" for c, k in ((c, cats[c]) for c in cats)))
    return bad


def main():
    without = set()
    freeze = False
    listing = False
    for a in sys.argv[1:]:
        if a.startswith("--without="):
            without = set(a.split("=", 1)[1].split(","))
        elif a == "--freeze":
            freeze = True
        elif a == "--remainder":
            listing = True
        else:
            sys.exit(__doc__)
    if check(without, freeze):
        sys.exit(1)
    if listing:
        for f, line, cat in sorted(REMAINDER_SITES):
            print(f"REMAINDER {f}:{line} {cat}")
    if NOT_YET_SCANNED:
        print(f"refusal coverage: every refusal site in the scanned files has a row or a reasoned "
              f"exemption; {len(NOT_YET_SCANNED)} in-scope file(s) NOT YET SCANNED (a freeze "
              f"refuses until none is)")
    else:
        print("refusal coverage: every refusal site in every in-scope file has a row, a checkable "
              "exemption, or is on the counted REMAINDER list (guards no test observes alone; REMAINDER "
              "is NOT claimed covered: `python3 scripts/v022_refusal_coverage.py --remainder`)")


if __name__ == "__main__":
    main()
