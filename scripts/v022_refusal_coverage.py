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
}
# (file -> sites with neither a row nor an exemption, as last measured). The
# gate re-measures each count and refuses a stale one, in both directions.
NOT_YET_SCANNED = {
    # Amendment 71 (r4c-fixes part 2): brought in by the crate rule, NOT YET
    # SCANNED (every site measured; neither rowed nor exempted yet). A freeze
    # refuses while any is listed. The libraries the protected crates link:
    "crates/axon-attest/src/lib.rs": 19,
    "crates/axon-audit/src/lib.rs": 12,
    "crates/axon-core/src/main.rs": 3,
    "crates/axon-cortex/src/generate.rs": 6,
    "crates/axon-cortex/src/lib.rs": 9,
    "crates/axon-cortex/src/runner.rs": 35,
    "crates/axon-fabric/src/bin/axon-custodian.rs": 1,
    "crates/axon-os/src/approval.rs": 4,
    "crates/axon-os/src/coalition.rs": 6,
    "crates/axon-os/src/ledger.rs": 4,
    "crates/axon-os/src/manifest.rs": 12,
    "crates/axon-os/src/profile.rs": 4,
    "crates/axon-os/src/record.rs": 5,
    "crates/axon-os/src/replay.rs": 1,
    "crates/axon-os/src/runtime.rs": 7,
    "crates/axon-psv/src/bin/axon-psv-runner.rs": 1,
    "crates/axon-vm/src/admit.rs": 6,
    "crates/axon-vm/src/firecracker.rs": 10,
    # Amendment 74 (C9 round 4c, gate): dependency-crate files the predicate-
    # primitive rule finds decisions in (they had no site before). NOT YET
    # SCANNED like the rest of the dependency crates (another workstream).
    "crates/axon-cortex/src/action.rs": 2,
    "crates/axon-cortex/src/episode.rs": 1,
    "crates/axon-cortex/src/locate.rs": 2,
    "crates/axon-cortex/src/select.rs": 2,
    "crates/axon-os/src/cli.rs": 1,
    "crates/axon-os/src/corrigible.rs": 2,
    "crates/axon-os/src/grant.rs": 10,
    "crates/axon-os/src/killchan.rs": 1,
    "crates/axon-os/src/latch.rs": 1,
    "crates/axon-os/src/monitor.rs": 1,
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
    (PL, "        ok.then_some(())",
     "OS ERROR during the hand-over (fchmod/fchown of the out dir): reported as the helper's error, "
     "fails closed"),
    (PL, "fn is_hex64(s: &str) -> bool {",
     "PREDICATE OF NAMED ROWS / OPERATOR-AUTHORED: its callers judge fields of the operator-owned "
     "helper config (program pins, the observer key: exempt, M585/M586; the custodian pin: M1485), "
     "pins of the operator-owned profile manifest that verify_inputs then hashes against (M590), "
     "and the request's manifest digest (M796, retired with its four-cell record)"),
    (PL, "fn is_dir(st: &libc::stat) -> bool {",
     "UNREACHABLE (checkable): both callers stat a descriptor opened with DIR_FLAGS (O_DIRECTORY): "
     "operator_dir's (walk_open, load_config's parent, new_staging's leaf; the existing exemption) "
     "and open_out_root's (walk_open), so the answer is always true; the rest of open_out_root's "
     "condition is rowed (M537)"),
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
    (LA, "pub(crate) fn other_loop_role(",
     "PREDICATE OF NAMED ROWS: every production caller is a refusal a row removes: admission.rs "
     "(M113), pointer.rs revocation (M121), baseline issue (M102), transition (M1328), baseline "
     "designation (M1335), plan.rs assignment issue (M111), all ACTIVE"),
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
PRED_RET = re.compile(r"^(bool|Option\s*<)")
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
        g = i
        for j in range(i, max(-1, i - MAX_UP - 1), -1):
            # Amendment 74: a guard block never reaches into the function
            # above (it used to: the nearest opener could be another fn's).
            if j < i and FN_HEAD.match(lines[j]):
                break
            if OPENER.search(lines[j]):
                g = j
                break
        out.append((g, i, i))
    for a, b in predicate_fns(lines):
        if regions is not None and not any(x <= a <= y for x, y in regions):
            continue
        out.append((a, b, a))
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
        spans.append((r[0], changed_lines(text, r[3], r[4])))
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
    for g, i, at in sites(text, f, bad):
        # Amendment 74: a row covers a site only when a line its edit CHANGES
        # lies in the site's guard block (it used to be enough that the row's
        # OLD text overlapped the block, plus the line after it).
        by = [rid for rid, ch in spans if any(g <= c <= i for c in ch)]
        # A predicate primitive's exemption is anchored on its HEAD line
        # (amendment 74): an anchor in its body belongs to a line site there.
        ex_hit = [e for e in ex if ((g <= e[0] <= i) if at == i else e[0] == g)]
        for e in ex_hit:
            e[3] += 1
        if by:
            covered += 1
            for e in ex_hit:
                bad.append(f"{f}:{i + 1}: exempt ({e[1]!r}) yet covered by {by}: drop the exemption")
        elif ex_hit:
            exempt += 1
        else:
            what = (f"{lines[g].strip()} ... {lines[i].strip()}" if at == i else
                    f"{lines[g].strip()} (a predicate primitive: it decides by bool/Option)")
            uncovered.append(f"{f}:{at + 1}: refusal site with no row and no exemption: {what}")
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
