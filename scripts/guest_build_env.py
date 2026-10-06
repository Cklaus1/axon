#!/usr/bin/env python3
"""The controlled environment EVERY byte of the protected guest image is built in.

    guest_build_env.py begin   RECORD.json
    guest_build_env.py cargo   RECORD.json [--rustflags FLAGS] -- CARGO-ARGS...
    guest_build_env.py path    RECORD.json TRIPLE PROFILE NAME
    guest_build_env.py finish  RECORD.json NAME=PATH...
    guest_build_env.py rootfs  RECORD.json OUT.sqfs
    guest_build_env.py dist    RECORD.json DIST-DIR   (digest of every artifact in dist,
                       refused unless it is what the controlled steps produced)
    guest_build_env.py check-host-build CLONE [--cargo PATH]   (round 5: the AMBIENT
                       build of the host binaries, judged by the guest's classifier)
    guest_build_env.py discard RECORD.json
    guest_build_env.py kernel  KERNEL-RECORD.json DIST-DIR PROFILE-DIR
    guest_build_env.py toolchain-pin MANIFEST.json   (amendment 65: the image's
                       recorded host tools against the operator's pin; a
                       missing pin is a WARNING here and a refusal at the freeze)

WHY (C9 round 4, FIELD-ORIGIN / PSV-2 / EQUIVALENCE, executed by two
reviewers): build-guest-image.sh refused a compiler wrapper by LISTING four
environment variables and a wrapper key in four config files. Cargo resolves
far more than that list: an ANCESTOR directory's .cargo/config.toml, the dotted
key `build.rustc-wrapper`, RUSTC / CARGO_BUILD_RUSTC, RUSTFLAGS /
CARGO_ENCODED_RUSTFLAGS, linker settings, RUSTUP_TOOLCHAIN -- and a reused
CARGO_TARGET_DIR served a wrapped build's artifacts to a clean-looking one
(nothing recompiled in 0.20 s). The runner built into this image holds the
completion secret, so a runner built through a wrapper can forge a keyed pass.

So the build no longer asks where a wrapper MIGHT be configured. It runs cargo
in an environment it CONSTRUCTS:

* the caller's environment is dropped: cargo sees exactly ENV_ALLOWLIST, whose
  values this script chooses (PATH is the pinned toolchain's bin dir and
  /usr/bin:/bin; RUSTC is that toolchain's own rustc; RUSTFLAGS only what the
  build step passes explicitly, and recorded);
* the toolchain is the one rust-toolchain.toml pins, resolved by rustup under
  that same cleared environment (never RUSTUP_TOOLCHAIN or PATH), and its
  identity -- `rustc -vV`, `cargo -V`, the sha256 of both binaries and of the
  host linker -- is recorded;
* CARGO_HOME and the target directory are FRESH directories this script
  creates empty, so no config and no artifact from any earlier build is
  reachable;
* before anything builds, cargo's EFFECTIVE configuration from the build's
  working directory (`cargo -Zunstable-options config get --show-origin`,
  which reads every ancestor config) is refused unless every key comes from
  the tree's own committed `.cargo/config.toml` and no environment variable
  other than the two this script sets is in play.

C9 round 4b (FIELD-ORIGIN, three major-adjacent findings; amendment 63):

1. The effective config was checked ONCE, at `begin`, while cargo re-reads it
   on every invocation from its working directory upward. Cargo ran in the
   clone, under /var/tmp (drwxrwxrwt): an ancestor config planted after begin
   wrapped every rustc of the runner build, and the record still said
   foreign=[]. Now the build is IMMUTABLE for its duration: `begin` copies the
   tree's tracked files into a private directory under a BUILD PARENT whose
   every ancestor only root or the builder can write (refused otherwise), and
   cargo runs THERE, never in the clone -- so no other uid can place a config
   anywhere cargo looks. The effective config is also checked before AND
   after every cargo invocation against begin's; each check is recorded in the
   build's entry, and the record's judge requires both.
2. The judge ignored the recorded cargo args and RUSTFLAGS. Now `cargo` runs
   only an invocation in INVOCATIONS (args and RUSTFLAGS exactly), refusing
   anything else before cargo starts; the judge requires every recorded build
   to be its table entry, the protected image to be exactly PROTECTED_BUILDS
   in order, and the toolchain to be the pinned channel's.
3. vmlinux and rootfs.sqfs were made outside it (the caller's gcc, KCFLAGS,
   PATH; the caller's mksquashfs). Now `kernel` builds the kernel from the
   pinned tarball, config and overlay (each copied privately, THEN verified)
   with make under a constructed environment, recording the host toolchain
   (path, sha256, version of gcc, cc1, as, ld, make and the other tools);
   `rootfs` assembles the root filesystem from the controlled build's own
   artifacts (re-verified), the pinned busybox and the tree's guest-init.sh,
   with /usr/bin/mksquashfs under a constructed environment. `image_problems`
   is the judge the freeze applies to the WHOLE image: every artifact the
   manifest pins must be the bytes one of these records produced.

C9 round 5 (FIELD-ORIGIN, amendment 80):

1. The effective config is judged by its STRUCTURED key path (`cargo config get
   --format json`) against COMMITTED_KEYS, never by splitting the text cargo
   prints (a single-quoted `cfg(all(..="..."))` target read as harmless and a
   committed linker linked the guest binaries). Everything else is refused.
2. `check-host-build` applies the same classifier to the ambient build of the
   host binaries (the setuid launcher, verifier, custodian, observer).
3. Each record carries a PROOF: an HMAC under a per-build key stored only in
   the builder-private parent (`<parent>/keys`), re-signed by every write of the
   runner, covering every field. `dist` records every dist artifact's digest;
   the manifest and the freeze read digests from the record and refuse a
   record whose proof does not hold. Residual trust: the builder account; the
   key files must travel if the freeze runs elsewhere.

What is recorded, not independently verified: the identity of the host tools
(and of the toolchain) is their sha256 at build time; nothing pins the
expected digests (operator item, as for rustc in round 4b's FUTURE finding),
and shared libraries those tools load are not hashed.

The records go into the guest manifest (scripts/linux_profile_manifest.py),
and the freeze (scripts/v022_freeze_manifest.py) refuses a guest manifest any
of whose components is not this controlled build's.
"""
import hashlib
import hmac
import json
import os
import pwd
import re
import stat
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCHEMA = "axon-guest-build-env/2"
KERNEL_SCHEMA = "axon-guest-kernel-build/1"
# Exactly the variables cargo sees. Values are chosen here, never inherited
# (the proxy variables excepted: they choose how crates are FETCHED, and
# `--locked` checks every fetched crate against Cargo.lock's checksum).
ENV_ALLOWLIST = ["CARGO_HOME", "CARGO_TARGET_DIR", "HOME", "LC_ALL", "PATH", "RUSTC"]
PROXY_VARS = ["http_proxy", "https_proxy", "no_proxy", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"]
# The only directories a host tool (linker, gcc, make, mksquashfs) is taken
# from, and the PATH every non-cargo step runs under.
TOOL_PATH = "/usr/bin:/bin"
GIT = "/usr/bin/git"
GIT_ENV = {"PATH": TOOL_PATH, "LC_ALL": "C", "GIT_CONFIG_NOSYSTEM": "1",
           "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_NO_REPLACE_OBJECTS": "1",
           "GIT_TERMINAL_PROMPT": "0"}

CRT = "-C target-feature=+crt-static"
MUSL = "x86_64-unknown-linux-musl"
# Every cargo invocation the guest build may make: (name, args, RUSTFLAGS).
# `cargo` refuses anything else before cargo starts (round 4b, finding 2).
PROTECTED_BUILDS = [
    ("axon", ["build", "--locked", "-p", "axon-core", "--target", MUSL,
              "--no-default-features", "--bin", "axon", "--release", "--quiet"], CRT),
    ("axon-guest-init", ["build", "--locked", "-p", "axon-guest-init", "--target", MUSL,
                         "--release", "--quiet"], CRT),
    ("axon-psv-runner", ["build", "--locked", "-p", "axon-psv", "--bin", "axon-psv-runner",
                         "--target", MUSL, "--release", "--quiet"], CRT),
]
# The development backends' builds (axon-guest-kernel, the initramfs). They
# run in the same environment; the freeze binds only PROTECTED_BUILDS.
DEV_BUILDS = [
    ("dev:axon-guest-kernel", ["build", "-p", "axon-guest-kernel", "-Z", "json-target-spec",
                               "--target", "crates/axon-guest-kernel/targets/x86_64-axon-metal.json",
                               "--release", "-Z", "build-std=core,compiler_builtins",
                               "-Z", "build-std-features=compiler-builtins-mem", "--quiet"], CRT),
    ("dev:initramfs-axon", ["build", "-p", "axon-core", "--target", MUSL, "--no-default-features",
                            "--bin", "axon", "--release", "--quiet"], CRT),
    ("dev:initramfs-guest-init", ["build", "-p", "axon-guest-init", "--target", MUSL,
                                  "--release", "--quiet"], CRT),
]
INVOCATIONS = PROTECTED_BUILDS + DEV_BUILDS
# The rootfs: where each controlled artifact is installed, and mksquashfs's
# exact flags (-all-time/-mkfs-time 0 + -all-root: a function of its inputs).
ROOTFS_BINARIES = {"axon": "usr/bin/axon", "axon-guest-init": "usr/bin/axon-guest-init",
                   "axon-psv-runner": "usr/bin/axon-psv-runner"}
MKSQUASHFS_FLAGS = ["-noappend", "-all-root", "-no-xattrs", "-mkfs-time", "0", "-all-time", "0",
                    "-comp", "gzip", "-quiet"]
# Host tools whose identity a kernel build records; the first five must resolve.
KERNEL_TOOLS_REQUIRED = ["make", "gcc", "cc1", "as", "ld"]
KERNEL_TOOLS = KERNEL_TOOLS_REQUIRED + ["cc", "ar", "nm", "objcopy", "objdump", "strip", "flex",
                                        "bison", "bc", "perl", "sh", "bash", "awk", "sed", "tar",
                                        "xz", "openssl", "pahole", "python3"]
# The linker cargo's musl builds invoke.
CARGO_HOST_TOOLS = ["cc", "ld"]
# Amendment 65: the operator's host-toolchain pin (written by the deployment
# kit, scripts/operator_deploy_protected_host.sh, from the deployed image's own
# build records). A fixed path: a caller never chooses which pin judges it.
TOOLCHAIN_PIN = "/etc/axon/host-toolchain-pin.json"
TOOLCHAIN_PIN_SCHEMA = "axon-host-toolchain-pin/1"


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def fail(why):
    sys.exit(f"refused: {why}")


def pinned_channel_or_none():
    """The toolchain channel rust-toolchain.toml pins (the tree's own file)."""
    try:
        for line in open(os.path.join(ROOT, "rust-toolchain.toml")):
            s = line.strip()
            if s.startswith("channel") and "=" in s:
                return s.split("=", 1)[1].strip().strip('"')
    except OSError:
        pass
    return None


def pinned_channel():
    c = pinned_channel_or_none()
    if c is None:
        fail("rust-toolchain.toml names no channel: the guest is built only by the pinned toolchain")
    return c


def builder_home():
    return pwd.getpwuid(os.geteuid()).pw_dir


def rustup():
    """rustup from the invoking user's home, found without PATH."""
    home = builder_home()
    p = os.path.join(home, ".cargo", "bin", "rustup")
    if not os.path.isfile(p):
        fail(f"no rustup at {p}: the pinned toolchain cannot be resolved without PATH")
    return p, home


def toolchain():
    """Absolute paths of the pinned toolchain's cargo and rustc, resolved by
    rustup under a cleared environment (so RUSTUP_TOOLCHAIN, RUSTUP_HOME and
    PATH from the caller choose nothing)."""
    ru, home = rustup()
    chan = pinned_channel()
    env = {"HOME": home, "PATH": "/usr/bin:/bin", "LC_ALL": "C"}
    out = {}
    for tool in ("cargo", "rustc"):
        r = subprocess.run([ru, "which", "--toolchain", chan, tool], env=env, cwd=ROOT,
                           capture_output=True, text=True)
        p = r.stdout.strip()
        if r.returncode != 0 or not os.path.isabs(p) or not os.path.isfile(p):
            fail(f"rustup cannot resolve {tool} of the pinned toolchain {chan}: {r.stderr.strip()[-300:]}")
        out[tool] = p
    return chan, out["cargo"], out["rustc"]


def controlled_env(rec, rustflags=None):
    env = dict(rec["env"])
    if rustflags is not None:
        env["RUSTFLAGS"] = rustflags
    return env


# The triples a guest build compiles for besides the HOST (build scripts, proc
# macros -- taken from `rustc -vV`, never hard-coded): the musl guest binaries
# and the freestanding kernel.
GUEST_TRIPLES = {MUSL, "x86_64-axon-metal"}
# The ONLY keys of cargo's effective configuration a guest build tolerates:
# the tree's own committed `.cargo/config.toml` (R7: a larger wasm stack),
# each a `target.<triple>.rustflags` for a triple this build never compiles for.
# Everything else -- whatever its spelling, table or origin -- is refused: a
# key is judged by its STRUCTURED path (`cargo config get --format json`),
# never by splitting the text cargo prints (round 5, FIELD-ORIGIN: cargo prints
# a `cfg(...)` target whose expression holds double quotes with SINGLE quotes,
# the split left a stray quote, the table read as a harmless triple, and a
# committed `linker` under it linked the guest binaries).
COMMITTED_KEYS = {("target", "wasm32-wasip1", "rustflags"),
                  ("target", "wasm32-unknown-unknown", "rustflags")}


def host_triple(rustc):
    """The triple rustc itself says it runs on (`rustc -vV`), or None."""
    try:
        r = subprocess.run([rustc, "-vV"], env={"PATH": TOOL_PATH, "LC_ALL": "C"},
                           stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        return None
    for line in r.stdout.splitlines():
        if line.startswith("host: "):
            return line[len("host: "):].strip() or None
    return None


def config_leaves(tree, prefix=()):
    """Every leaf key PATH of a parsed config (a tuple of the exact keys, never
    a dotted string): scalars, arrays, and an empty table (a `[net]` header
    with nothing under it is still a table somebody wrote)."""
    if isinstance(tree, dict) and tree:
        for k, v in tree.items():
            yield from config_leaves(v, prefix + (k,))
    else:
        yield prefix


def key_problem(path, triples):
    """Why config key `path` could reach a guest build, or None: only the
    exact COMMITTED_KEYS pass, and only for a triple that is not one the build
    compiles for and is not a `cfg(...)` table."""
    if path not in COMMITTED_KEYS:
        return "a setting the guest build would use"
    if path[1].startswith("cfg(") or path[1] in triples:
        return f"a setting for {path[1]}, which this build compiles for"
    return None


def key_text(path):
    return " > ".join(repr(k) for k in path)


def effective_config(cargo, env, cwd, env_ok=None):
    """(origins, foreign): where every key of cargo's EFFECTIVE config comes
    from, as cargo resolves it from `cwd` (every ancestor .cargo/config.toml,
    CARGO_HOME's, and the environment), and every entry the guest build may
    not have: any entry that could affect this build at all, whatever its
    origin (a wrapper committed to the tree's own config still puts a program
    outside the tree between the sources and the bytes).

    Classification is on the STRUCTURED config (`--format json`: a nested
    object keyed by the exact key names) against COMMITTED_KEYS, whatever file
    a key came from. The text form (`--show-origin`) is read only to record
    ORIGINS. `env_ok(name)` says which variables of `env` may be present (default: the
    constructed ENV_ALLOWLIST and the proxies); any other is refused, since
    cargo's own list of environment variables that may affect the config does
    not name RUSTFLAGS, RUSTC_WRAPPER and the like."""
    if env_ok is None:
        env_ok = lambda n: n in ENV_ALLOWLIST or n in PROXY_VARS  # noqa: E731
    foreign = [f"environment variable {n} is set: it could stand between the sources and the bytes"
               for n in sorted(env) if not env_ok(n)]
    rustc = env.get("RUSTC") or os.path.join(os.path.dirname(cargo), "rustc")
    host = host_triple(rustc)
    if host is None:
        foreign.append(f"cannot learn the host triple from `{rustc} -vV`")
    triples = GUEST_TRIPLES | ({host} if host else set())
    argv = [cargo, "-Zunstable-options", "config", "get"]
    j = subprocess.run(argv + ["--format", "json"], env=env, cwd=cwd, capture_output=True, text=True)
    try:
        tree = json.loads(j.stdout) if j.returncode == 0 else None
    except ValueError:
        tree = None
    if not isinstance(tree, dict):
        return [], sorted(set(foreign + [f"cargo config get --format json failed: {j.stderr.strip()[-300:]}"]))
    for path in config_leaves(tree):
        why = key_problem(path, triples)
        if why:
            foreign.append(f"{key_text(path)}: {why}")
    # The text form is read for ORIGINS only (recorded: where each value came
    # from). It decides nothing: the structured config above is complete.
    r = subprocess.run(argv + ["--show-origin"], env=env, cwd=cwd, capture_output=True, text=True)
    origins = set()
    for line in r.stdout.splitlines():
        s = line.strip()
        if " # " in s and not s.startswith("#"):
            origins.add(s.rsplit(" # ", 1)[1].strip())
    return sorted(origins), sorted(set(foreign))


# What an AMBIENT (host-side) build may inherit. The host binaries (the setuid
# launcher, the verifier, the custodian, the observer) are built by the
# operator's own cargo; nothing constructs that environment, so what can be
# done is to refuse the ambient state that could steer it: any variable cargo or
# rustc reads for the compiler, wrapper, flags, linker, registry or profile, and
# the C toolchain variables the `cc` crate honours (ring compiles C).
HOST_ENV_OK = {"CARGO_HOME", "CARGO_TARGET_DIR"}
HOST_ENV_PREFIXES = ("CARGO_", "RUSTC", "RUSTFLAGS", "RUSTDOC", "RUSTUP_TOOLCHAIN", "SCCACHE",
                     "CC_", "CXX_", "CFLAGS", "CXXFLAGS", "CPPFLAGS", "LDFLAGS", "TARGET_", "HOST_",
                     "LD_", "BINDGEN_", "PKG_CONFIG")
HOST_ENV_NAMES = {"CC", "CXX", "AR", "CPP", "LD", "RANLIB", "NM", "STRIP", "OBJCOPY"}


def host_env_ok(name):
    """May this variable be present in an ambient host build's environment?"""
    if name in HOST_ENV_OK:
        return True
    return not (name in HOST_ENV_NAMES or name.startswith(HOST_ENV_PREFIXES))


def host_build_problems(clone, cargo=None, env=None):
    """Why an ambient host-binary build of `clone` is not one that only the
    tree's own sources and committed config steer (empty: it is). The same
    classifier the guest build applies (effective_config), over the caller's
    OWN environment, from the clone, reading every ancestor config and
    CARGO_HOME's. A check made at deploy time: it cannot see what existed when
    the binaries were built -- the build's own record of that is
    crates/axon-fabric/src/build_state.rs."""
    env = dict(os.environ if env is None else env)
    if cargo is None:
        _chan, cargo, _rustc = toolchain()
    return effective_config(cargo, env, clone, env_ok=host_env_ok)[1]


def constructed_env(base, cargo, rustc, proxies):
    """The WHOLE environment cargo runs in: built here from the fresh base dir
    and the pinned toolchain, never taken from the caller (the proxy variables
    excepted, which choose how crates are fetched, not what is built)."""
    env = {"CARGO_HOME": os.path.join(base, "cargo-home"),
           "CARGO_TARGET_DIR": os.path.join(base, "target"), "HOME": base, "LC_ALL": "C",
           "PATH": f"{os.path.dirname(cargo)}:/usr/bin:/bin", "RUSTC": rustc}
    env.update(proxies)
    return env


def kernel_env(base):
    """The WHOLE environment the kernel's make runs in: no KCFLAGS, KCPPFLAGS,
    CROSS_COMPILE, LLVM, CC, HOSTCC, MAKEFLAGS or PATH of the caller's."""
    return {"HOME": base, "LC_ALL": "C", "PATH": TOOL_PATH,
            "KBUILD_BUILD_TIMESTAMP": "1970-01-01", "KBUILD_BUILD_USER": "axon",
            "KBUILD_BUILD_HOST": "b263", "KBUILD_BUILD_VERSION": "1"}


def prefixes(path):
    """`/`, `/a`, `/a/b`, ... `path`."""
    out, cur = ["/"], "/"
    for part in [p for p in path.split("/") if p]:
        cur = os.path.join(cur, part)
        out.append(cur)
    return out


def ancestors_of(parent, builder_uid=None):
    """(ancestors, problem): every directory from / to the build parent, with
    its owner and mode, or why another uid could write one of them. A
    directory a third uid can write is one it can plant a `.cargo/config.toml`
    (or anything else a build reads) in -- a sticky /var/tmp included."""
    if not os.path.isabs(parent) or os.path.realpath(parent) != os.path.normpath(parent):
        return [], f"the build parent {parent} is not an absolute path free of symlinks"
    me, out = os.geteuid() if builder_uid is None else builder_uid, []
    for p in prefixes(os.path.normpath(parent)):
        try:
            st = os.lstat(p)
        except OSError as e:
            return out, f"{p}: {e.strerror}"
        out.append({"path": p, "uid": st.st_uid, "mode": oct(st.st_mode & 0o7777)})
        if st.st_uid not in (0, me) or st.st_mode & 0o022:
            return out, (f"{p} (uid {st.st_uid}, mode {oct(st.st_mode & 0o7777)}) can be written by "
                         "a uid other than root or the builder: a config planted there reaches the build")
    return out, ""


def ancestors_problem(parent, ancestors, builder_uid):
    """Why a RECORDED build parent was not one only root or the builder could
    write (the judge's side of ancestors_of)."""
    if not isinstance(parent, str) or not isinstance(ancestors, list):
        return "it records no build parent"
    if [a.get("path") if isinstance(a, dict) else None for a in ancestors] != prefixes(os.path.normpath(parent)):
        return "its recorded build-parent ancestors are not the parent's"
    for a in ancestors:
        mode = a.get("mode")
        if (a.get("uid") not in (0, builder_uid) or not isinstance(mode, str)
                or not mode.startswith("0o") or not mode[2:] or set(mode[2:]) - set("01234567")
                or int(mode, 8) & 0o022):
            return f"its build parent's ancestor {a.get('path')} was writable by another uid"
    return ""


def build_parent():
    """The private directory every guest build's workspace is created under:
    `<builder's home>/.cache/axon-guest-build` (AXON_GUEST_BUILD_PARENT names
    another), refused unless only root or the builder can write any ancestor."""
    parent = os.environ.get("AXON_GUEST_BUILD_PARENT") or os.path.join(
        builder_home(), ".cache", "axon-guest-build")
    os.makedirs(parent, mode=0o700, exist_ok=True)
    ancestors, why = ancestors_of(parent)
    if why:
        fail(f"the guest build's parent directory is not private to the builder: {why}")
    return parent, ancestors


def private_base(prefix):
    parent, ancestors = build_parent()
    base = tempfile.mkdtemp(prefix=prefix, dir=parent)
    return parent, ancestors, base


def copy_tracked_tree(dst):
    """Copy the tree's TRACKED files (working-tree content) into `dst`: the
    build runs on this private copy, never on the clone (whose ancestors are
    not the build's to control). Untracked files are not sources."""
    r = subprocess.run([GIT, "--no-replace-objects", "-c", "safe.directory=*", "-c", "core.fsmonitor=",
                        "-C", ROOT, "ls-files", "-z", "--cached"],
                       env=GIT_ENV, capture_output=True)
    if r.returncode != 0:
        fail(f"cannot list the tree's tracked files: {r.stderr.decode(errors='replace').strip()[-300:]}")
    n = 0
    for rel in [p for p in r.stdout.decode().split("\0") if p]:
        src, out = os.path.join(ROOT, rel), os.path.join(dst, rel)
        if not os.path.lexists(src):
            continue
        os.makedirs(os.path.dirname(out), exist_ok=True)
        if os.path.islink(src):
            os.symlink(os.readlink(src), out)
        elif os.path.isfile(src):
            shutil.copy2(src, out)
        else:
            fail(f"tracked path {rel} is neither a file nor a symlink")
        n += 1
    return n


def tool_identity(path, version_flag="--version"):
    try:
        v = subprocess.run([path, version_flag], env={"PATH": TOOL_PATH, "LC_ALL": "C"},
                           stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=30)
        version = ((v.stdout or v.stderr).strip().splitlines() or ["unknown"])[0]
    except (OSError, subprocess.TimeoutExpired):
        version = "unknown"
    return {"path": path, "realpath": os.path.realpath(path), "sha256": sha256(path),
            "version": version}


def host_tool(name):
    """A host tool by name, from TOOL_PATH only (never the caller's PATH)."""
    for d in TOOL_PATH.split(":"):
        p = os.path.join(d, name)
        if os.path.isfile(p) and os.access(p, os.X_OK):
            return p
    return None


def host_tool_path(name):
    """Where a judge expects host tool `name`: TOOL_PATH's first directory."""
    return os.path.join(TOOL_PATH.split(":")[0], name)


def host_tools(names):
    out = {}
    for n in names:
        if n == "cc1":
            r = subprocess.run([host_tool("gcc") or host_tool_path("gcc"), "-print-prog-name=cc1"],
                               env={"PATH": TOOL_PATH, "LC_ALL": "C"}, stdin=subprocess.DEVNULL,
                               capture_output=True, text=True)
            p = r.stdout.strip() if r.returncode == 0 else ""
            p = p if os.path.isabs(p) and os.path.isfile(p) else None
        else:
            p = host_tool(n)
        if p:
            out[n] = tool_identity(p)
    return out


def begin(record_path):
    chan, cargo, rustc = toolchain()
    parent, ancestors, base = private_base("axon-guest-build-")
    proxies = {k: os.environ[k] for k in PROXY_VARS if os.environ.get(k)}
    env = constructed_env(base, cargo, rustc, proxies)
    cargo_home, target = env["CARGO_HOME"], env["CARGO_TARGET_DIR"]
    os.mkdir(cargo_home)
    os.mkdir(target)
    src = os.path.join(base, "src")
    os.mkdir(src)
    files = copy_tracked_tree(src)
    origins, foreign = effective_config(cargo, env, src)
    if foreign:
        subprocess.run(["rm", "-rf", "--", base], check=False)
        fail("cargo's effective configuration for the guest build is not the tree's own "
             "(a wrapper, rustc, rustflags or linker could stand between these sources and the "
             "bytes):\n  " + "\n  ".join(foreign))

    def first(cmd):
        r = subprocess.run(cmd, env=env, cwd=src, capture_output=True, text=True)
        if r.returncode != 0:
            subprocess.run(["rm", "-rf", "--", base], check=False)
            fail(f"{' '.join(cmd)} failed: {r.stderr.strip()[-300:]}")
        return r.stdout.strip()
    rec = {
        "schema": SCHEMA,
        "controlled": True,
        "toolchain": {"channel": chan, "cargo": cargo, "cargo_sha256": sha256(cargo),
                      "cargo_version": first([cargo, "-V"]), "rustc": rustc,
                      "rustc_sha256": sha256(rustc), "rustc_vV": first([rustc, "-vV"]),
                      "host_tools": host_tools(CARGO_HOST_TOOLS)},
        "env": env,
        "env_allowlist": ENV_ALLOWLIST,
        "proxy_vars": sorted(proxies),
        "builder_uid": os.geteuid(),
        "build_parent": parent,
        "build_parent_ancestors": ancestors,
        "src_dir": src,
        "src_files": files,
        "cargo_home": cargo_home,
        "cargo_home_created_empty": True,
        "target_dir": target,
        "target_dir_created_empty": True,
        "effective_config": {"origins": origins, "foreign": foreign,
                             "own_config": ".cargo/config.toml"},
        "builds": [],
        "artifacts": {},
        "proof": new_proof(parent, base),
    }
    write(record_path, rec)
    print(f"[guest-build-env] controlled: toolchain {chan} "
          f"({rec['toolchain']['rustc_vV'].splitlines()[0]}), a private copy of the tree's "
          f"{files} tracked files, fresh CARGO_HOME and target dir under {base}, effective "
          f"config only from {origins or ['(none)']}")


PROOF_SCHEMA = "axon-guest-build-proof/1"
PROOF_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,100}")


def proof_payload(rec):
    """The bytes a record's proof covers: the whole record, the proof's own
    schema and id included, its hmac excluded."""
    body = {k: v for k, v in rec.items() if k != "proof"}
    pr = rec.get("proof") if isinstance(rec.get("proof"), dict) else {}
    body["proof"] = {"schema": pr.get("schema"), "id": pr.get("id")}
    return json.dumps(body, sort_keys=True, separators=(",", ":")).encode()


def proof_key(parent, pid, builder_uid, judging=True):
    """(key bytes, why-not): the per-build key `new_proof` stored under the
    builder-private parent. Only a key that is a real file (no symlink), owned
    by the builder, closed to everyone else, in a directory likewise, in a
    parent only root or the builder can write, is a key at all."""
    if not isinstance(parent, str) or not isinstance(pid, str) or not PROOF_ID.fullmatch(pid):
        return None, "the record names no usable proof id or build parent"
    # The builder signing needs only its key; whoever JUDGES a record also needs
    # the key's parent to be one nobody else could have written a key into.
    _anc, why = ancestors_of(parent, builder_uid) if judging else ([], "")
    if why:
        return None, f"the proof's build parent is not private: {why}"
    kd = os.path.join(parent, "keys")
    path = os.path.join(kd, pid + ".key")
    try:
        dst = os.lstat(kd)
        fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    except OSError as e:
        return None, f"the build's proof key is not at {path} ({e.strerror}): a record is judged only where its builder's key is"
    try:
        st = os.fstat(fd)
        if (not stat.S_ISDIR(dst.st_mode) or dst.st_uid != builder_uid or dst.st_mode & 0o077
                or not stat.S_ISREG(st.st_mode) or st.st_uid != builder_uid or st.st_mode & 0o177):
            return None, f"{path} is not a file only the builder (uid {builder_uid}) can read, in a directory likewise"
        key = os.read(fd, 4096)
    finally:
        os.close(fd)
    if len(key) < 32:
        return None, f"{path} holds no key"
    return key, ""


def new_proof(parent, base):
    """Create this build's key (random, 0400, in `<parent>/keys`, never
    removed with the build directory: the freeze re-reads it) and return the
    record's `proof` stub; `write` fills in the hmac."""
    kd = os.path.join(parent, "keys")
    os.makedirs(kd, mode=0o700, exist_ok=True)
    os.chmod(kd, 0o700)
    pid = os.path.basename(base)
    fd = os.open(os.path.join(kd, pid + ".key"), os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o400)
    try:
        os.write(fd, os.urandom(32).hex().encode())
    finally:
        os.close(fd)
    return {"schema": PROOF_SCHEMA, "id": pid, "hmac": ""}


def proof_problems(rec, what):
    """Why `rec` (a controlled-build record, `what` names it) is not the one
    this build's runner wrote: no proof, a proof under another key, or any
    field edited after the runner signed it (empty: it holds). A hand-written
    record, or the record of a build made elsewhere, cannot carry it: the key
    never leaves the builder-private parent. Residual: whoever holds the
    builder account can sign anything."""
    pr = rec.get("proof") if isinstance(rec, dict) else None
    if (not isinstance(pr, dict) or pr.get("schema") != PROOF_SCHEMA
            or not isinstance(pr.get("hmac"), str) or not re.fullmatch(r"[0-9a-f]{64}", pr["hmac"])):
        return f"the {what} record carries no builder proof (a record the controlled runner did not write)"
    key, why = proof_key(rec.get("build_parent"), pr.get("id"), rec.get("builder_uid"))
    if why:
        return f"the {what} record's proof cannot be checked: {why}"
    if not hmac.compare_digest(hmac.new(key, proof_payload(rec), "sha256").hexdigest(), pr["hmac"]):
        return f"the {what} record's proof does not hold: it was edited after its builder signed it, or signed under another key"
    return ""


def write(path, rec):
    if isinstance(rec.get("proof"), dict):
        key, why = proof_key(rec.get("build_parent"), rec["proof"].get("id"), rec.get("builder_uid"),
                             judging=False)
        if why:
            fail(why)
        rec["proof"]["hmac"] = hmac.new(key, proof_payload(rec), "sha256").hexdigest()
    with open(path, "w") as f:
        json.dump(rec, f, indent=2)
        f.write("\n")


def load(path):
    with open(path) as f:
        rec = json.load(f)
    why = shape_problems(rec)
    if why:
        fail(f"{path} is not a controlled build record: {why}")
    return rec


def invocation(args, rustflags):
    """The INVOCATIONS entry these exact args and RUSTFLAGS are, or None."""
    for name, a, rf in INVOCATIONS:
        if a == args and rf == rustflags:
            return name
    return None


def config_now(rec):
    o, f = effective_config(rec["toolchain"]["cargo"], rec["env"], rec["src_dir"])
    return {"origins": o, "foreign": f}


def config_at_begin(rec):
    ec = rec.get("effective_config") or {}
    return {"origins": ec.get("origins"), "foreign": ec.get("foreign")}


def run_cargo(record_path, argv):
    rustflags = None
    if argv[:1] == ["--rustflags"]:
        rustflags, argv = argv[1], argv[2:]
    if argv[:1] != ["--"]:
        fail("usage: cargo RECORD [--rustflags FLAGS] -- CARGO-ARGS...")
    args = argv[1:]
    name = invocation(args, rustflags)
    if name is None:
        fail(f"cargo {args} with RUSTFLAGS {rustflags!r} is not a controlled guest build "
             "invocation (scripts/guest_build_env.py INVOCATIONS): extra arguments or flags "
             "could put a wrapper, linker or config between the sources and the bytes")
    rec = load(record_path)
    env = controlled_env(rec, rustflags)
    # Cargo re-reads its config on EVERY invocation: hold it to begin's before
    # this one starts and after it ends (round 4b, finding 1).
    before = config_now(rec)
    if before != config_at_begin(rec):
        fail(f"cargo's effective configuration changed since begin, before `{name}`: {before}")
    entry = {"name": name, "args": args, "rustflags": rustflags, "config_before": before}
    rec["builds"].append(entry)
    write(record_path, rec)
    r = subprocess.run([rec["toolchain"]["cargo"], *args], env=env, cwd=rec["src_dir"])
    entry["config_after"] = after = config_now(rec)
    write(record_path, rec)
    if after != config_at_begin(rec):
        fail(f"cargo's effective configuration changed DURING `{name}`: {after}")
    sys.exit(r.returncode)


def built_path(rec, triple, profile, name):
    return os.path.join(rec["target_dir"], triple, profile, name)


def finish(record_path, pairs):
    rec = load(record_path)
    for p in pairs:
        name, path = p.split("=", 1)
        rec["artifacts"][name] = sha256(path)
    write(record_path, rec)


DIST_BINARIES = ("axon", "axon-guest-init", "axon-psv-runner")


def dist_record(record_path, dist):
    """The end of the build: every artifact the runner produced is in `dist`,
    byte for byte what the record says it produced. Records the digest of each
    dist file (the three binaries and rootfs.sqfs) and refuses a copy that
    differs from the controlled step's output."""
    rec = load(record_path)
    out = {}
    for name in DIST_BINARIES:
        got, want = sha256(os.path.join(dist, name)), rec["artifacts"].get(name)
        if got != want:
            fail(f"dist/{name} ({got}) is not the bytes the controlled build produced ({want})")
        out[name] = got
    got = sha256(os.path.join(dist, "rootfs.sqfs"))
    if got != (rec.get("rootfs") or {}).get("sha256"):
        fail(f"dist/rootfs.sqfs ({got}) is not the controlled assembly's output")
    out["rootfs.sqfs"] = got
    rec["dist"] = out
    write(record_path, rec)


def dist_problems(dist, benv, kbuild):
    """Why a dist directory is not what the build records say their runner
    produced (empty: it is): each file's digest from the RECORD, never from
    the file, and both records' proofs. `kbuild` may be None (a rootfs-only
    build has no kernel record: the freeze refuses it later)."""
    why = proof_problems(benv, "build")
    if why:
        return why
    want = dict(benv.get("dist") or {})
    if sorted(want) != sorted(DIST_BINARIES + ("rootfs.sqfs",)):
        return "the build record names no digest for every dist artifact (`guest_build_env.py dist` was not run)"
    pairs = list(want.items())
    if kbuild is not None:
        why = proof_problems(kbuild, "kernel build")
        if why:
            return why
        pairs += [("vmlinux", kbuild.get("vmlinux_sha256")),
                  ("effective.config", kbuild.get("effective_config_sha256"))]
    for name, digest in pairs:
        p = os.path.join(dist, name)
        if not os.path.isfile(p) or sha256(p) != digest:
            return f"dist/{name} is not the bytes its build record says the controlled runner produced ({digest})"
    return ""


def read_pin(profile_dir):
    pin = {}
    for line in open(os.path.join(profile_dir, "kernel.pin")):
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, v = line.split("=", 1)
            pin[k] = v
    return pin


def pinned_copy(src, dst, want, label):
    """Copy `src` to `dst`, THEN verify the copy is the pinned bytes: what is
    used is what was checked, whatever happens to `src` afterwards."""
    shutil.copyfile(src, dst)
    got = sha256(dst)
    if got != want:
        fail(f"{label} sha256 mismatch: got {got}, pinned {want}")
    return got


def rootfs(record_path, out):
    """Assemble rootfs.sqfs from the controlled build's own artifacts, the
    pinned busybox and the tree's guest-init.sh, with /usr/bin/mksquashfs
    under a constructed environment; record every input and the output."""
    rec = load(record_path)
    base = os.path.dirname(rec["target_dir"])
    prof = os.path.join(rec["src_dir"], "profiles", "linux-microvm")
    pin = read_pin(prof)
    stage = tempfile.mkdtemp(prefix="rootfs-", dir=base)
    # The stage becomes the image's `/`: the unprivileged test uid must
    # traverse it (mkdtemp makes it 0700).
    os.chmod(stage, 0o755)
    for d in ["bin", "usr", "usr/bin", "proc", "sys", "dev", "tmp", "work", "out", "in",
              "in/candidate", "in/suite", "in/job"]:
        os.mkdir(os.path.join(stage, d))
        os.chmod(os.path.join(stage, d), 0o755)
    inputs = {}
    for name, rel in ROOTFS_BINARIES.items():
        dst = os.path.join(stage, rel)
        shutil.copyfile(built_path(rec, MUSL, "release", name), dst)
        got = sha256(dst)
        if got != rec["artifacts"].get(name):
            fail(f"the rootfs's {name} ({got}) is not the bytes the controlled build produced "
                 f"({rec['artifacts'].get(name)})")
        inputs[name] = got
    bb = os.path.join(stage, "bin", "busybox")
    inputs["busybox"] = pinned_copy(pin["BUSYBOX_SRC"], bb, pin["BUSYBOX_SHA256"], "busybox")
    shutil.copyfile(os.path.join(prof, "guest-init.sh"), os.path.join(stage, "init"))
    inputs["guest-init.sh"] = sha256(os.path.join(stage, "init"))
    for f in ["init", "bin/busybox", *ROOTFS_BINARIES.values()]:
        os.chmod(os.path.join(stage, f), 0o755)
    renv = {"HOME": base, "LC_ALL": "C", "PATH": TOOL_PATH}
    lst = subprocess.run([bb, "--list"], env=renv, capture_output=True, text=True)
    if lst.returncode != 0:
        fail(f"busybox --list failed: {lst.stderr.strip()[-300:]}")
    for applet in lst.stdout.split():
        if applet != "busybox":
            os.symlink("busybox", os.path.join(stage, "bin", applet))
    mks = host_tool("mksquashfs")
    if mks is None:
        fail(f"no mksquashfs in {TOOL_PATH}")
    if os.path.lexists(out):
        os.unlink(out)
    argv = [mks, stage, os.path.abspath(out), *MKSQUASHFS_FLAGS]
    r = subprocess.run(argv, env=renv, cwd=base)
    if r.returncode != 0:
        fail(f"mksquashfs failed ({r.returncode})")
    rec["rootfs"] = {"tool": tool_identity(mks, "-version"), "argv": argv, "env": renv,
                     "inputs": inputs, "sha256": sha256(out)}
    write(record_path, rec)
    subprocess.run(["rm", "-rf", "--", stage], check=False)


def kernel(record_path, dist, profile_dir):
    """Build vmlinux from the pinned tarball, config and overlay, with make
    under a constructed environment in a private directory; record the host
    toolchain that produced it. Writes vmlinux and effective.config to `dist`."""
    pin = read_pin(profile_dir)
    parent, ancestors, base = private_base("axon-kernel-build-")
    try:
        ver = pin["KERNEL_VERSION"]
        tarball = os.path.join(base, f"linux-{ver}.tar.xz")
        pins = {"version": ver,
                "tarball_sha256": pinned_copy(os.path.join(dist, f"linux-{ver}.tar.xz"), tarball,
                                              pin["KERNEL_TARBALL_SHA256"], "kernel tarball"),
                "config_sha256": pinned_copy(os.path.join(profile_dir, pin["KERNEL_CONFIG"]),
                                             os.path.join(base, "base.config"),
                                             pin["KERNEL_CONFIG_SHA256"], "kernel config"),
                "overlay_sha256": pinned_copy(os.path.join(profile_dir, pin["KERNEL_OVERLAY"]),
                                              os.path.join(base, "overlay.config"),
                                              pin["KERNEL_OVERLAY_SHA256"], "kernel config overlay")}
        kenv = kernel_env(base)
        tools = host_tools(KERNEL_TOOLS)
        missing = [t for t in KERNEL_TOOLS_REQUIRED + ["tar"] if t not in tools]
        if missing:
            fail(f"the kernel build's host tools {missing} are not in {TOOL_PATH}")
        r = subprocess.run([tools["tar"]["path"], "-xf", tarball, "-C", base], env=kenv)
        if r.returncode != 0:
            fail("the kernel tarball did not extract")
        ksrc = os.path.join(base, f"linux-{ver}")
        with open(os.path.join(ksrc, ".config"), "wb") as f:
            f.write(open(os.path.join(base, "base.config"), "rb").read())
            for line in open(os.path.join(base, "overlay.config"), "rb"):
                if line.startswith(b"CONFIG_"):
                    f.write(line)
        make = tools["make"]["path"]
        steps = [[make, "ARCH=x86_64", "olddefconfig"],
                 [make, "ARCH=x86_64", f"-j{os.cpu_count() or 1}", "vmlinux"]]
        for argv in steps:
            r = subprocess.run(argv, env=kenv, cwd=ksrc, stdout=subprocess.DEVNULL)
            if r.returncode != 0:
                fail(f"{' '.join(argv)} failed ({r.returncode})")
        vml = os.path.join(ksrc, "vmlinux")
        if not os.path.isfile(vml):
            fail("vmlinux not built")
        for name, src in (("vmlinux", vml), ("effective.config", os.path.join(ksrc, ".config"))):
            dst = os.path.join(dist, name)
            if os.path.lexists(dst):
                os.unlink(dst)
            shutil.copyfile(src, dst)
        rec = {"schema": KERNEL_SCHEMA, "controlled": True, "builder_uid": os.geteuid(),
               "build_parent": parent, "build_parent_ancestors": ancestors, "base": base,
               "pin": pins, "env": kenv, "make": steps, "tools": tools,
               "effective_config_sha256": sha256(os.path.join(dist, "effective.config")),
               "vmlinux_sha256": sha256(os.path.join(dist, "vmlinux")),
               "proof": new_proof(parent, base)}
        write(record_path, rec)
    finally:
        subprocess.run(["rm", "-rf", "--", base], check=False)


def entry_problems(rec, b):
    """Why one recorded cargo invocation is not a controlled one."""
    if not isinstance(b, dict):
        return "a recorded build is not an object"
    if b.get("name") is None or invocation(b.get("args"), b.get("rustflags")) != b.get("name"):
        return (f"recorded build {b.get('name')!r} ran cargo {b.get('args')} with RUSTFLAGS "
                f"{b.get('rustflags')!r}, not its controlled invocation")
    if b.get("config_before") != config_at_begin(rec) or b.get("config_after") != config_at_begin(rec):
        return (f"recorded build {b.get('name')!r} has no effective-config check equal to begin's "
                f"before AND after it ran (before {b.get('config_before')}, after {b.get('config_after')})")
    return ""


def shape_problems(rec):
    """Why `rec` is not a controlled-build record (empty: it is one). The
    judgement the freeze applies; the manifest embeds the record verbatim."""
    if not isinstance(rec, dict) or rec.get("schema") != SCHEMA or rec.get("controlled") is not True:
        return f"it is not a {SCHEMA} record of a controlled build"
    tc = rec.get("toolchain")
    env = rec.get("env")
    if (not isinstance(tc, dict) or not isinstance(env, dict) or not tc.get("rustc_vV")
            or not tc.get("rustc_sha256") or not tc.get("cargo") or not tc.get("rustc")):
        return "it records no toolchain identity or environment"
    chan = pinned_channel_or_none()
    tdir = os.path.dirname(str(tc["cargo"]))
    if (chan is None or tc.get("channel") != chan or os.path.dirname(str(tc["rustc"])) != tdir
            or not os.path.basename(os.path.dirname(tdir)).startswith(chan + "-")
            or not all(((tc.get("host_tools") or {}).get(t) or {}).get("sha256") for t in CARGO_HOST_TOOLS)):
        return (f"its toolchain is not the pinned channel {chan}'s (channel {tc.get('channel')}, "
                f"cargo {tc.get('cargo')}, rustc {tc.get('rustc')}) or records no linker identity")
    base = os.path.dirname(str(rec.get("target_dir")))
    proxies = {k: env.get(k) for k in rec.get("proxy_vars") or [] if k in PROXY_VARS}
    want = constructed_env(base, tc["cargo"], tc["rustc"], proxies)
    if env != want or rec.get("cargo_home") != want["CARGO_HOME"] or rec.get("target_dir") != want["CARGO_TARGET_DIR"]:
        return ("cargo did not run in the environment the build constructs "
                f"(extra {sorted(set(env) - set(want))}, differing "
                f"{sorted(k for k in set(env) & set(want) if env[k] != want[k])}, "
                f"missing {sorted(set(want) - set(env))})")
    if rec.get("target_dir_created_empty") is not True or rec.get("cargo_home_created_empty") is not True:
        return "its target dir or CARGO_HOME is not a fresh one the build created"
    if (rec.get("effective_config") or {}).get("foreign") != []:
        return f"cargo's effective config held settings the build would use: {(rec.get('effective_config') or {}).get('foreign')}"
    why = ancestors_problem(rec.get("build_parent"), rec.get("build_parent_ancestors"), rec.get("builder_uid"))
    if rec.get("src_dir") != os.path.join(base, "src") or os.path.dirname(base) != rec.get("build_parent") or why:
        return ("cargo did not run on a private copy of the tree in a directory only root or the "
                "builder could write: " + (why or "its working directory was not the build's own copy"))
    for b in rec.get("builds") or []:
        why = entry_problems(rec, b)
        if why:
            return why
    return ""


def kernel_problems(k, man):
    """Why the manifest's vmlinux is not the bytes a controlled kernel build
    (`kernel` above) produced from the manifest's own pins."""
    mk = man.get("kernel") or {}
    vml = ((man.get("artifacts") or {}).get("vmlinux") or {}).get("sha256")
    want = {"version": mk.get("version"), "tarball_sha256": mk.get("tarball_sha256"),
            "config_sha256": mk.get("config_sha256"), "overlay_sha256": mk.get("overlay_sha256")}
    if (not isinstance(k, dict) or k.get("schema") != KERNEL_SCHEMA or k.get("controlled") is not True
            or not vml or k.get("vmlinux_sha256") != vml
            or k.get("effective_config_sha256") != mk.get("effective_config_sha256")
            or k.get("pin") != want):
        return ("its vmlinux is not the bytes a controlled kernel build produced from the "
                f"manifest's pinned sources {want} (kernel build record: "
                f"{'absent' if not isinstance(k, dict) else k.get('pin')})")
    k = k if isinstance(k, dict) else {}
    base = k.get("base")
    tools = k.get("tools") or {}
    make = (tools.get("make") or {}).get("path")
    steps = k.get("make") or []
    if (not isinstance(base, str) or os.path.dirname(base) != k.get("build_parent")
            or ancestors_problem(k.get("build_parent"), k.get("build_parent_ancestors"), k.get("builder_uid"))
            or k.get("env") != kernel_env(base) or make != host_tool_path("make") or len(steps) != 2
            or steps[0] != [make, "ARCH=x86_64", "olddefconfig"]
            or len(steps[1]) != 4 or steps[1][:2] != [make, "ARCH=x86_64"]
            or not str(steps[1][2]).startswith("-j") or not str(steps[1][2])[2:].isdigit()
            or steps[1][3] != "vmlinux"
            or not all((tools.get(t) or {}).get("sha256") for t in KERNEL_TOOLS_REQUIRED)):
        return (f"its kernel's make did not run in a private directory, in the constructed "
                f"environment, with the recorded host toolchain (env {k.get('env')}, make {steps})")
    return ""


def rootfs_problems(rec, man):
    """Why the manifest's rootfs.sqfs is not the bytes `rootfs` above made
    from this record's artifacts, the pinned busybox and guest-init.sh."""
    r = rec.get("rootfs")
    sq = ((man.get("artifacts") or {}).get("rootfs.sqfs") or {}).get("sha256")
    want = {n: (rec.get("artifacts") or {}).get(n) for n in ROOTFS_BINARIES}
    want["busybox"] = (man.get("busybox") or {}).get("sha256")
    want["guest-init.sh"] = (man.get("guest_init") or {}).get("sha256")
    if (not isinstance(r, dict) or not sq or r.get("sha256") != sq
            or r.get("inputs") != want or not all(want.values())):
        return ("its rootfs.sqfs is not the bytes the controlled rootfs assembly produced from the "
                f"controlled artifacts and pinned inputs {want}")
    r = r if isinstance(r, dict) else {}
    base = os.path.dirname(str(rec.get("target_dir")))
    tool = r.get("tool") or {}
    argv = r.get("argv") or []
    if (tool.get("path") != host_tool_path("mksquashfs") or not tool.get("sha256")
            or argv[:1] != [tool.get("path")] or argv[3:] != MKSQUASHFS_FLAGS
            or r.get("env") != {"HOME": base, "LC_ALL": "C", "PATH": TOOL_PATH}):
        return f"its rootfs was not made by {TOOL_PATH}'s mksquashfs in the constructed environment ({argv})"
    return ""


def recorded_host_tools(man):
    """Every host tool the image's build records name, by name: the kernel
    build's tools, cargo's host tools (linker), the rootfs's mksquashfs, and
    the toolchain's rustc and cargo. The same extraction the deployment kit
    pins from (operator_deploy_protected_host.sh, step `toolchain`)."""
    src = (man.get("source") or {}).get("build_environment") or {}
    tc = src.get("toolchain") or {}
    tools = {}
    for name, t in sorted((((man.get("kernel") or {}).get("build_environment") or {}).get("tools") or {}).items()):
        tools[name] = t
    for name, t in sorted((tc.get("host_tools") or {}).items()):
        tools.setdefault(name, t)
    rt = (src.get("rootfs") or {}).get("tool")
    if rt:
        tools["mksquashfs"] = rt
    tools["rustc"] = {"path": tc.get("rustc"), "sha256": tc.get("rustc_sha256")}
    tools["cargo"] = {"path": tc.get("cargo"), "sha256": tc.get("cargo_sha256")}
    return {n: {"path": (t or {}).get("path"), "sha256": (t or {}).get("sha256")} for n, t in tools.items()}


def operator_file_problem(path):
    """Why `path` is not an operator file: the file and every directory above
    it real (no symlink), root-owned and not group/other-writable."""
    import stat as st_
    parts = os.path.abspath(path).split("/")[1:]
    cur = "/"
    for i, part in enumerate([""] + parts):
        cur = os.path.join(cur, part) if part else "/"
        try:
            st = os.lstat(cur)
        except OSError as e:
            return f"{cur}: {e.strerror}"
        last = i == len(parts)
        if st_.S_ISLNK(st.st_mode) or (last and not st_.S_ISREG(st.st_mode)) or (not last and not st_.S_ISDIR(st.st_mode)):
            return f"{cur} is not a real {'file' if last else 'directory'} (a symlink is never followed)"
        if st.st_uid != 0 or st.st_mode & 0o022:
            return (f"{cur} (uid {st.st_uid}, mode {oct(st.st_mode & 0o7777)}) is not root-owned and "
                    "closed to group/other writes: whoever writes it chooses the pin")
    return ""


def toolchain_pin_problems(man, required, pin_path=TOOLCHAIN_PIN):
    """Why the image's recorded host tools are not the operator's pin (empty:
    they are, or there is no pin and none is `required`). A pin that exists
    is judged whether or not it is required: owner and mode first, then every
    recorded tool must be a pinned one, at the pinned path, with the pinned
    digest, and every pinned tool must be recorded."""
    if not os.path.lexists(pin_path):
        return (f"there is no operator host-toolchain pin at {pin_path} (the deployment kit writes it; "
                "a freeze binds only a build made with the operator's pinned tools)") if required else ""
    why = operator_file_problem(pin_path)
    if why:
        return f"the host-toolchain pin {pin_path} is not the operator's: {why}"
    try:
        with open(pin_path, "rb") as f:
            pin = json.loads(f.read(1 << 20))
    except (OSError, ValueError) as e:
        return f"the host-toolchain pin {pin_path} is unreadable: {e}"
    want = pin.get("tools") if isinstance(pin, dict) and pin.get("schema") == TOOLCHAIN_PIN_SCHEMA else None
    if not isinstance(want, dict) or not want:
        return f"the host-toolchain pin {pin_path} is not a {TOOLCHAIN_PIN_SCHEMA} naming tools"
    got = recorded_host_tools(man)
    for name in sorted(set(got) | set(want)):
        g, w = got.get(name), want.get(name)
        if not isinstance(w, dict):
            return f"the build recorded host tool {name} {g}, which the operator's pin does not name"
        if g is None or g.get("path") != w.get("path") or not g.get("sha256") or g.get("sha256") != w.get("sha256"):
            return (f"host tool {name}: the build recorded {g}, not the operator's pin "
                    f"{ {'path': w.get('path'), 'sha256': w.get('sha256')} }")
    return ""


def image_problems(man, pin_required=False):
    """Why a guest manifest has a component produced outside the controlled
    build (empty: none). The judge the freeze applies to the WHOLE image,
    after the record's own judge (shape_problems) and its binding of the three
    binaries; it does not repeat those, so each refusal has one owner.
    Amendment 65: last, the recorded host tools against the operator's
    toolchain pin (`pin_required`: the freeze; absent pin otherwise passes
    here and is a warning from `toolchain-pin`)."""
    rec = (man.get("source") or {}).get("build_environment")
    if not isinstance(rec, dict):
        return "it records no build environment"
    names = [b.get("name") for b in rec.get("builds") or []]
    if names != [n for n, _, _ in PROTECTED_BUILDS]:
        return f"its binaries were built by {names}, not exactly the protected builds in order"
    why = rootfs_problems(rec, man)
    if why:
        return why
    why = kernel_problems((man.get("kernel") or {}).get("build_environment"), man)
    if why:
        return why
    why = toolchain_pin_problems(man, pin_required)
    if why:
        return why
    # Last: the records are the runner's own. A structure that holds is not
    # authorship -- a hand-written record naming a foreign binary's digests has
    # every field the judges above read.
    return (proof_problems(rec, "build")
            or proof_problems((man.get("kernel") or {}).get("build_environment"), "kernel build"))


def discard(record_path):
    """Remove the private build directory once the artifacts are copied out
    (their digests are in the record)."""
    rec = load(record_path)
    base = os.path.dirname(rec["target_dir"])
    if os.path.basename(base).startswith("axon-guest-build-") and os.path.dirname(base) == rec["build_parent"]:
        subprocess.run(["rm", "-rf", "--", base], check=False)


def main():
    a = sys.argv[1:]
    if a[:1] == ["begin"] and len(a) == 2:
        begin(a[1])
    elif a[:1] == ["cargo"] and len(a) >= 3:
        run_cargo(a[1], a[2:])
    elif a[:1] == ["path"] and len(a) == 5:
        rec = load(a[1])
        print(built_path(rec, a[2], a[3], a[4]))
    elif a[:1] == ["finish"] and len(a) >= 3:
        finish(a[1], a[2:])
    elif a[:1] == ["rootfs"] and len(a) == 3:
        rootfs(a[1], a[2])
    elif a[:1] == ["kernel"] and len(a) == 4:
        kernel(a[1], a[2], a[3])
    elif a[:1] == ["dist"] and len(a) == 3:
        dist_record(a[1], a[2])
    elif a[:1] == ["check-host-build"] and len(a) in (2, 4) and (len(a) == 2 or a[2] == "--cargo"):
        why = host_build_problems(os.path.realpath(a[1]), a[3] if len(a) == 4 else None)
        if why:
            fail("the host binaries' build environment is not the tree's own (a wrapper, rustc, "
                 "rustflags, linker, replaced source or ambient config could stand between the "
                 "sources and the bytes):\n  " + "\n  ".join(why))
        print("[guest-build-env] host build environment: cargo's effective config for the clone is "
              "only its own committed config, and no compiler, wrapper, flag or linker variable is set")
    elif a[:1] == ["discard"] and len(a) == 2:
        discard(a[1])
    elif a[:1] == ["toolchain-pin"] and len(a) == 2:
        with open(a[1]) as f:
            man = json.load(f)
        why = toolchain_pin_problems(man, False)
        if why:
            fail(why)
        if not os.path.lexists(TOOLCHAIN_PIN):
            print(f"WARNING: no operator host-toolchain pin at {TOOLCHAIN_PIN}: this build's tools "
                  "are judged by nothing but its own record (development); a freeze refuses it",
                  file=sys.stderr)
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
