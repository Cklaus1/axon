#!/usr/bin/env python3
"""The controlled environment the protected guest image is built in.

    guest_build_env.py begin  RECORD.json
    guest_build_env.py cargo  RECORD.json [--rustflags FLAGS] -- CARGO-ARGS...
    guest_build_env.py path   RECORD.json TRIPLE PROFILE NAME
    guest_build_env.py finish RECORD.json NAME=PATH...
    guest_build_env.py discard RECORD.json

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
  identity -- `rustc -vV`, `cargo -V`, the sha256 of both binaries -- is
  recorded;
* CARGO_HOME and the target directory are FRESH directories this script
  creates empty, so no config and no artifact from any earlier build is
  reachable;
* before anything builds, cargo's EFFECTIVE configuration from the build's
  working directory (`cargo -Zunstable-options config get --show-origin`,
  which reads every ancestor config) is refused unless every key comes from
  the tree's own committed `.cargo/config.toml` and no environment variable
  other than the two this script sets is in play.

The record (schema axon-guest-build-env/1) goes into the guest manifest
(scripts/linux_profile_manifest.py), and the freeze (scripts/
v022_freeze_manifest.py) refuses a guest manifest whose record is not this
controlled build, or whose artifact digests are not the record's.
"""
import hashlib
import json
import os
import pwd
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCHEMA = "axon-guest-build-env/1"
# Exactly the variables cargo sees. Values are chosen here, never inherited
# (the proxy variables excepted: they choose how crates are FETCHED, and
# `--locked` checks every fetched crate against Cargo.lock's checksum).
ENV_ALLOWLIST = ["CARGO_HOME", "CARGO_TARGET_DIR", "HOME", "LC_ALL", "PATH", "RUSTC"]
PROXY_VARS = ["http_proxy", "https_proxy", "no_proxy", "HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"]


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def fail(why):
    sys.exit(f"refused: {why}")


def pinned_channel():
    """The toolchain channel rust-toolchain.toml pins (the tree's own file)."""
    try:
        for line in open(os.path.join(ROOT, "rust-toolchain.toml")):
            s = line.strip()
            if s.startswith("channel") and "=" in s:
                return s.split("=", 1)[1].strip().strip('"')
    except OSError:
        pass
    fail("rust-toolchain.toml names no channel: the guest is built only by the pinned toolchain")


def rustup():
    """rustup from the invoking user's home, found without PATH."""
    home = pwd.getpwuid(os.geteuid()).pw_dir
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


# The triples a guest build compiles for: the host (build scripts, proc
# macros), the musl guest binaries, and the freestanding kernel.
BUILD_TRIPLES = {"x86_64-unknown-linux-gnu", "x86_64-unknown-linux-musl", "x86_64-axon-metal"}


def key_cannot_reach_the_build(key):
    """A config key that cannot affect a guest build: a setting for a
    `target.<triple>` this build never compiles for (the tree's own
    `target.wasm32-*.rustflags`). Every other key -- build.*, a matching or
    `cfg(...)` target, env, patch, source, paths, profile, unstable -- could."""
    parts = key.split(".")
    if len(parts) < 3 or parts[0] != "target":
        return False
    triple = parts[1].strip('"')
    return not triple.startswith("cfg(") and triple not in BUILD_TRIPLES


def effective_config(cargo, env):
    """(origins, foreign): where every key of cargo's EFFECTIVE config comes
    from, as cargo resolves it from the build's working directory (every
    ancestor .cargo/config.toml, CARGO_HOME's, and the environment), and every
    entry the guest build may not have: any entry that could
    affect this build at all, whatever its origin (a wrapper committed to the
    tree's own config still puts a program outside the tree between the sources
    and the bytes)."""
    r = subprocess.run([cargo, "-Zunstable-options", "config", "get", "--show-origin"],
                       env=env, cwd=ROOT, capture_output=True, text=True)
    if r.returncode != 0:
        return [], [f"cargo config get failed: {r.stderr.strip()[-300:]}"]
    own = os.path.realpath(os.path.join(ROOT, ".cargo", "config.toml"))
    origins, foreign = set(), []
    in_env_note, key = False, None
    for line in (r.stdout + r.stderr).splitlines():
        s = line.strip()
        if not s:
            continue
        if s.startswith("# The following environment variables may affect"):
            in_env_note = True
            continue
        if in_env_note:
            # Only the variables constructed_env sets can be here.
            continue
        if s.startswith("note:"):
            continue
        if s in ("]", "}", "],", "},"):
            continue
        if " = " in s and (s.endswith("= [") or s.endswith("= {")):
            key = s.split(" = ", 1)[0].strip()
            continue
        if " = " in s.split(" # ", 1)[0]:
            key = s.split(" = ", 1)[0].strip()
        if " # " not in s:
            foreign.append(f"a config value of unknown origin: {s}")
            continue
        origin = s.rsplit(" # ", 1)[1].strip()
        origins.add(origin)
        if not key or not key_cannot_reach_the_build(key):
            where = "the tree's own .cargo/config.toml" if os.path.realpath(origin) == own else origin
            foreign.append(f"{key} (from {where}): a setting the guest build would use")
    return sorted(origins), sorted(set(foreign))


def constructed_env(base, cargo, rustc, proxies):
    """The WHOLE environment cargo runs in: built here from the fresh base dir
    and the pinned toolchain, never taken from the caller (the proxy variables
    excepted, which choose how crates are fetched, not what is built)."""
    env = {"CARGO_HOME": os.path.join(base, "cargo-home"),
           "CARGO_TARGET_DIR": os.path.join(base, "target"), "HOME": base, "LC_ALL": "C",
           "PATH": f"{os.path.dirname(cargo)}:/usr/bin:/bin", "RUSTC": rustc}
    env.update(proxies)
    return env


def begin(record_path):
    chan, cargo, rustc = toolchain()
    base = tempfile.mkdtemp(prefix="axon-guest-build-", dir="/var/tmp")
    proxies = {k: os.environ[k] for k in PROXY_VARS if os.environ.get(k)}
    env = constructed_env(base, cargo, rustc, proxies)
    cargo_home, target = env["CARGO_HOME"], env["CARGO_TARGET_DIR"]
    os.mkdir(cargo_home)
    os.mkdir(target)
    origins, foreign = effective_config(cargo, env)
    if foreign:
        subprocess.run(["rm", "-rf", "--", base], check=False)
        fail("cargo's effective configuration for the guest build is not the tree's own "
             "(a wrapper, rustc, rustflags or linker could stand between these sources and the "
             "bytes):\n  " + "\n  ".join(foreign))

    def first(cmd):
        r = subprocess.run(cmd, env=env, cwd=ROOT, capture_output=True, text=True)
        if r.returncode != 0:
            subprocess.run(["rm", "-rf", "--", base], check=False)
            fail(f"{' '.join(cmd)} failed: {r.stderr.strip()[-300:]}")
        return r.stdout.strip()
    rec = {
        "schema": SCHEMA,
        "controlled": True,
        "toolchain": {"channel": chan, "cargo": cargo, "cargo_sha256": sha256(cargo),
                      "cargo_version": first([cargo, "-V"]), "rustc": rustc,
                      "rustc_sha256": sha256(rustc), "rustc_vV": first([rustc, "-vV"])},
        "env": env,
        "env_allowlist": ENV_ALLOWLIST,
        "proxy_vars": sorted(proxies),
        "cargo_home": cargo_home,
        "cargo_home_created_empty": True,
        "target_dir": target,
        "target_dir_created_empty": True,
        "effective_config": {"origins": origins, "foreign": foreign,
                             "own_config": ".cargo/config.toml"},
        "builds": [],
        "artifacts": {},
    }
    write(record_path, rec)
    print(f"[guest-build-env] controlled: toolchain {chan} "
          f"({rec['toolchain']['rustc_vV'].splitlines()[0]}), fresh CARGO_HOME and target dir "
          f"under {base}, effective config only from {origins or ['(none)']}")


def write(path, rec):
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


def run_cargo(record_path, argv):
    rustflags = None
    if argv[:1] == ["--rustflags"]:
        rustflags, argv = argv[1], argv[2:]
    if argv[:1] != ["--"]:
        fail("usage: cargo RECORD [--rustflags FLAGS] -- CARGO-ARGS...")
    args = argv[1:]
    rec = load(record_path)
    env = controlled_env(rec, rustflags)
    rec["builds"].append({"args": args, "rustflags": rustflags})
    write(record_path, rec)
    r = subprocess.run([rec["toolchain"]["cargo"], *args], env=env, cwd=ROOT)
    sys.exit(r.returncode)


def finish(record_path, pairs):
    rec = load(record_path)
    for p in pairs:
        name, path = p.split("=", 1)
        rec["artifacts"][name] = sha256(path)
    write(record_path, rec)


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
    return ""


def discard(record_path):
    """Remove the fresh CARGO_HOME and target dir once the artifacts are copied
    out (their digests are in the record)."""
    rec = load(record_path)
    base = os.path.dirname(rec["target_dir"])
    if os.path.basename(base).startswith("axon-guest-build-") and os.path.dirname(base) == "/var/tmp":
        subprocess.run(["rm", "-rf", "--", base], check=False)


def main():
    a = sys.argv[1:]
    if a[:1] == ["begin"] and len(a) == 2:
        begin(a[1])
    elif a[:1] == ["cargo"] and len(a) >= 3:
        run_cargo(a[1], a[2:])
    elif a[:1] == ["path"] and len(a) == 5:
        rec = load(a[1])
        triple, profile, name = a[2], a[3], a[4]
        print(os.path.join(rec["target_dir"], triple, profile, name))
    elif a[:1] == ["finish"] and len(a) >= 3:
        finish(a[1], a[2:])
    elif a[:1] == ["discard"] and len(a) == 2:
        discard(a[1])
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
