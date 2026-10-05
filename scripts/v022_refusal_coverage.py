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
    "crates/axon-loop/src/bin/axon-loop.rs":
        "the CLI front end: argument and request parsing; every verb hands its one document to a "
        "library function in a scanned file, which decides; its refusals are usage errors before "
        "any decision",
}
# (file -> sites with neither a row nor an exemption, as last measured). The
# gate re-measures each count and refuses a stale one, in both directions.
NOT_YET_SCANNED = {
    # Amendment 71 (r4c-fixes part 2): brought in by the crate rule, NOT YET
    # SCANNED (every site measured; neither rowed nor exempted yet). A freeze
    # refuses while any is listed. The libraries the protected crates link:
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
    (ST, '                    return Err(LoopError::Io(format!(\n                        "non-normal store path {}",',
     "UNREACHABLE: every store path is built by the store from its root joined with segments "
     "validated by check_segment or the ids.rs newtypes (charset [A-Za-z0-9._:-], first character "
     "alphanumeric) or with digest hex, so no component is `..`, `.` or a root"),
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
     "answer and fails: the four-cell run of this site (former M1088) read set_off=ATTACK_REFUSED. "
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
    (FW, '                return Err(ImportRefusal::QuotaBytes { limit: quota.bytes });', "RESOURCE BOUND: as above"),
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
    (FW, '            return Err(StoreError::DestinationExists(dest.to_path_buf()));',
     "UNREACHABLE BY CONSTRUCTION (checkable): every materialize caller passes a destination "
     "inside a directory it created NEW just before: psv.rs materialize_inputs (DirBuilder, not "
     "recursive) and submit's RunDir::new (create_dir, made non-recursive in C9 round 4b, so a "
     "leftover run dir is refused, never reused); the destination names inside it are fixed and "
     "each used once"),
    (FW, '                    return Err(StoreError::Io(format!("no symlinks here: {target}")));',
     "NON-UNIX: compiled only under cfg(not(unix))"),
    # journal.rs
    (FJ, '                return Err(name);', _JNV),
    (FJ, '                    return Err(JournalError::ScopeConflict {', _JNV),
    (FJ, '                    return Err(JournalError::UnknownScope(Box::new(intent.scope.clone())));', _JNV),
    (FJ, '                        return Err(JournalError::Conflict {\n', _JNV),
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
    (CI, 'return Err(shape(format!("authority epoch {v} exceeds 2^53-1")));',
     "UNREACHABLE BY CONSTRUCTION (FLAGGED: no four-cell can be built): a pointer epoch starts at "
     "0 and an applied transition raises it by exactly one (pointer.rs CAS, M1266/M1330), so "
     "next() never leaves the range; every epoch READ from a document is refused past 2^53-1 by "
     "parse_value first (M1204) and then by the schema maximum (M1219), and even with all three "
     "removed it is joined to the scope's current epoch (intake's epoch join, the pointer's "
     "expected_epoch CAS), which a value past 2^53-1 never equals"),
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
    (CS, '        return Err(fail(path, "non-integer number"));',
     "UNREACHABLE (FLAGGED: no four-cell can be built): a non-integer Number is refused by "
     "parse_value before any walk (M1206); every schema node carrying minimum/maximum declares "
     "a type, and no type name admits a float (asserted by redteam.rs every_checked_in_schema_is_within_the_supported_subset), "
     "whose check refuses it first (M1210); and with both removed the typed layer still refuses a "
     "float in an integer field (serde, no mutable site), so no input reaches this arm"),
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
    (CU, "    if r != 0 {\n        return Err(format!(\n            \"SO_PASSPIDFD:",
     "OS error from setsockopt: a kernel that cannot name a reply's sender refuses the call (fails "
     "closed); no sender is assumed"),
    (CU, "            if e.kind() == std::io::ErrorKind::Interrupted {",
     "OS error from recvmsg: the call is refused; EINTR is retried"),
    (CU, "        if msg.msg_flags & libc::MSG_CTRUNC != 0 || other {",
     "defence in depth, dominated by the per-message pin check (M1483): every descriptor received "
     "is owned and closed, and a truncated control buffer has no SCM_PIDFD, which the next line "
     "refuses (the kernel named no sender)"),
    (CU, "        if text.len() as u64 > MAX_MESSAGE {",
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
    (BIN, '            eprintln!("usage: axon-protected-launcher [--probe] < request.json");', _USE),
    (BIN, "        let _ = out.flush();\n        std::process::exit(code);",
     "RELAY: the report writer exits with the code serve_as decided; each refusal there is a "
     "site of its own"),
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
_AUD = ("NOT ON THE PROTECTED ROUTE (checkable): no protected crate names axon_audit; it is linked "
        "through axon-os, whose one caller is cli.rs (axon_audit::Ledger::open, the `axon-os` "
        "binary's ledger verbs), and Fabric uses only axon_os::{supervise_requiring, "
        "parse_manifest, canonical_grant, runtime::scan_effects, ledger::{Carve, ResourceLedger}} and "
        "its types (submit.rs, grants.rs, journal.rs, backend.rs), none of which reaches cli.rs")
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
_COA = ("NOT ON THE PROTECTED ROUTE (checkable): coalition.rs's Coalition is constructed by no "
        "non-test code in the workspace (grep `Coalition::new|carve_for_member|propose_vote`: "
        "coalition.rs's own tests only); corrigible.rs uses only the CoalitionBound type")
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
    (_SU, '        if on_disk < expected {',
     _AUD + "; Ledger::verify_against_file"),
    (_SU, '        if on_disk > expected {',
     _AUD + "; Ledger::verify_against_file"),
    (_SU, '        if entry.seq != i as u64 {',
     _AUD + "; verify_chain_keyed"),
    (_SU, '        if entry.prev_hash != expected_prev {',
     _AUD + "; verify_chain_keyed"),
    (_SU, '        if entry.entry_hash != expected_hash {',
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
     _LED + "; the compute axis, the one carve_within carves (its refusal is the journal's BudgetExceeded, exempt above as NOT A VERDICT PROPERTY)"),
    (_SOL, '        if self.budget_used.saturating_add(c.budget) > self.budget_cap {',
     _LED + "; and SELECTS NOTHING there: carve_within builds every ledger with this cap 0 and carves 0 on it (`ResourceLedger::new(lineage, cap, 0, 0)`, `budget: 0, persist_bytes: 0`), and 0 + 0 > 0 is false"),
    (_SOL, '        if self.persist_bytes_used.saturating_add(c.persist_bytes) > self.persist_bytes_cap {',
     _LED + "; and SELECTS NOTHING there: carve_within builds every ledger with this cap 0 and carves 0 on it (`ResourceLedger::new(lineage, cap, 0, 0)`, `budget: 0, persist_bytes: 0`), and 0 + 0 > 0 is false"),
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


# Amendment 71: the refusal forms the files the crate rule brings in use. A
# compile refusal (`Diagnostic::error(`: the resolver's sealed-module E0004),
# a process exit with a code that is not the literal 0 (the guest's PID 1 and
# the `axon test` entry refuse that way), and a call of a FILE-LOCAL refusal
# closure: `let NAME = |..| .. Err(..)` on one line makes every later `NAME(`
# in that file a site (conform.rs's `kind_ok`), the definition itself not;
# and so is a call of a file-local DIVERGING refusal constructor: a `fn NAME(..)
# -> !` or `let NAME = |..| -> !` whose first lines exit with a non-zero
# literal code (axon-custodian's `die`, the `axon test` key reader's `fail`).
DIAG = re.compile(r"\bDiagnostic::error\(")
EXIT = re.compile(r"\bprocess::exit\((?!\s*0\s*\))")
LOCAL_CTOR_DEF = re.compile(r"\blet\s+(\w+)\s*=\s*(move\s+)?\|[^|]*\|.*\bErr\(")
DIVERGING_DEF = re.compile(r"(?:\bfn\s+(\w+)\s*(?:<[^>]*>)?\s*\([^)]*\)|\blet\s+(\w+)\s*=\s*(?:move\s+)?\|[^|]*\|)\s*->\s*!")
EXIT_NONZERO = re.compile(r"\bexit\(\s*[1-9]")
DIVERGING_BODY = 8


def local_ctors(lines):
    """Names of the file-local refusal constructors (amendment 71): one-line
    `Err` closures, and diverging fns/closures that exit non-zero."""
    out = {m.group(1) for l in lines for m in [LOCAL_CTOR_DEF.search(l)] if m}
    for i, l in enumerate(lines):
        m = DIVERGING_DEF.search(l)
        if m and any(EXIT_NONZERO.search(x) for x in lines[i:i + DIVERGING_BODY]):
            out.add(m.group(1) or m.group(2))
    return out


def is_site(l, ctors=()):
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


def sites(text, f=None, bad=None):
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
        if s.startswith("//") or s.startswith("use ") or not is_site(l, ctors):
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
    found = set(SCOPE_FILES) | set(SCOPE_FN_REGIONS) | set(REGIONS)
    for d in SCOPE_DIRS:
        found.update(_rs_under(d))
    return sorted(found - linked_only_as_library())


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
    for g, i in sites(text, f, bad):
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
