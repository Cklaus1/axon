#!/usr/bin/env python3
"""Write dist/guest-linux/manifest.json for the protected Linux microVM profile.

Usage: linux_profile_manifest.py --snapshot <out.json>
       linux_profile_manifest.py [--pre <snapshot.json>] <dist-dir> <profile-dir>

Every artifact the launcher consumes is recorded by sha256. The launcher
(scripts/fc_linux_profile.sh) re-hashes each artifact against THIS manifest
before booting, so an artifact swapped after the build refuses to launch.

The VMM ENGINE (firecracker + jailer) is pinned here too, in `engine`, with the
exact field names axon-fabric's qualification() compares an evidence record's
`engine` block against. It was not pinned at all: the launcher exec'd whatever
sat at /usr/local/bin, so a swapped VMM ran with the same qualification as the
one that was measured. FC_BIN / JAILER_BIN override the paths pinned (the
launcher's defaults are the same two paths).

`source.axon_git_rev_at_build` / `axon_tree_dirty_at_build` gate Fabric's
qualification (RULE:manifest-clean). They come from the SAME Rust code that
stamps the readiness verifier's `source_dirty` (crates/axon-fabric/src/
provenance.rs over git_data.rs, compiled here as the `axon-provenance`
helper): never PATH git under the caller's environment, untracked files
count, and the repository's own git config is refused unless it is inert
(review FIELD-ORIGIN, C9 round 2). "Cannot tell" is DIRTY, never clean.
build-guest-image.sh takes a `--snapshot` BEFORE it builds; the manifest is
clean only if that snapshot and the tree now are both clean and name the same
revision, so the artifacts' source is the tree described.
"""
import hashlib
import json
import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
HELPER = os.path.join(ROOT, "crates", "axon-fabric", "src", "bin", "axon-provenance.rs")


def sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def first_line(cmd):
    try:
        out = subprocess.run(cmd, capture_output=True, text=True, check=False).stdout
        return out.splitlines()[0].strip() if out else "unknown"
    except OSError:
        return "unknown"


def cannot_tell(why):
    return {"revision": "unknown", "dirty": [f"cannot tell: {why}"]}


def provenance():
    """The axon-provenance/1 answer for ROOT's tree, or a DIRTY "cannot tell"."""
    try:
        with tempfile.TemporaryDirectory(prefix="axon-provenance-") as t:
            exe = os.path.join(t, "axon-provenance")
            rustc = os.environ.get("RUSTC", "rustc")
            b = subprocess.run([rustc, "--edition", "2021", "-C", "opt-level=1", "-o", exe, HELPER],
                               capture_output=True, text=True, cwd=ROOT, check=False)
            if b.returncode != 0:
                return cannot_tell(f"the provenance helper did not build: {b.stderr.strip()[-400:]}")
            r = subprocess.run([exe, ROOT], capture_output=True, text=True, check=False,
                               env={"PATH": "/usr/bin:/bin", "LC_ALL": "C"})
            d = json.loads(r.stdout)
    except (OSError, ValueError) as e:
        return cannot_tell(f"the provenance helper failed: {e}")
    if (r.returncode != 0 or not isinstance(d, dict) or d.get("schema") != "axon-provenance/1"
            or not isinstance(d.get("revision"), str) or not isinstance(d.get("dirty"), list)
            or not all(isinstance(x, str) for x in d["dirty"])):
        return cannot_tell("the provenance helper gave no axon-provenance/1 answer")
    return d


def source_state(pre_path):
    """(revision, reasons): the tree now, joined with the pre-build snapshot."""
    now = provenance()
    reasons = list(now["dirty"])
    pre = None
    if pre_path:
        try:
            with open(pre_path) as f:
                pre = json.load(f)
        except (OSError, ValueError):
            pre = None
    if not isinstance(pre, dict) or not isinstance(pre.get("dirty"), list):
        reasons.append("no pre-build provenance snapshot: the tree the artifacts were built from is unknown")
    else:
        reasons.extend(f"before the build: {r}" for r in pre["dirty"])
        if pre.get("revision") != now["revision"]:
            reasons.append(f"the tree moved during the build: {pre.get('revision')} -> {now['revision']}")
    if now["revision"] == "unknown" and not reasons:
        reasons.append("cannot tell: no revision")
    return now["revision"], reasons


def main():
    args = sys.argv[1:]
    if args[:1] == ["--snapshot"]:
        with open(args[1], "w") as f:
            json.dump(provenance(), f)
            f.write("\n")
        return
    pre_path = None
    if args[:1] == ["--pre"]:
        pre_path, args = args[1], args[2:]
    dist, prof = args[0], args[1]
    pin = {}
    for line in open(os.path.join(prof, "kernel.pin")):
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, v = line.split("=", 1)
            pin[k] = v
    rev, reasons = source_state(pre_path)
    dirty = bool(reasons)
    manifest = {
        "schema": "axon-linux-microvm-profile/1",
        "profile": "linux-microvm-protected",
        "guest_truth": "Linux kernel (NOT the axon-guest-kernel)",
        "source": {
            "axon_git_rev_at_build": rev,
            "axon_tree_dirty_at_build": dirty,
            "axon_tree_dirty_reasons": reasons,
            "axon_build": "RUSTFLAGS='-C target-feature=+crt-static' cargo build --locked "
                          "-p axon-core --no-default-features --bin axon --release "
                          "--target x86_64-unknown-linux-musl",
            "axon_guest_init_build": "RUSTFLAGS='-C target-feature=+crt-static' cargo build "
                                     "--locked -p axon-guest-init --release "
                                     "--target x86_64-unknown-linux-musl (default features: "
                                     "no dev-allow-no-policy bypass)",
            "axon_psv_runner_build": "RUSTFLAGS='-C target-feature=+crt-static' cargo build "
                                     "--locked -p axon-psv --bin axon-psv-runner --release "
                                     "--target x86_64-unknown-linux-musl",
            "pci_lineage": {"certified_revision": "31413ca7",
                            "rule": "axon_git_rev_at_build descends from it (git merge-base --is-ancestor)"},
            "rustc": first_line(["rustc", "--version"]),
        },
        "kernel": {
            "version": pin["KERNEL_VERSION"],
            "url": pin["KERNEL_URL"],
            "tarball_sha256": pin["KERNEL_TARBALL_SHA256"],
            "config_file": os.path.join(prof, pin["KERNEL_CONFIG"]),
            "config_sha256": pin["KERNEL_CONFIG_SHA256"],
            "config_origin": "firecracker v1.10.1 resources/guest_configs/"
                             "microvm-kernel-ci-x86_64-6.1.config (unmodified)",
            "overlay_file": os.path.join(prof, pin["KERNEL_OVERLAY"]),
            "overlay_sha256": pin["KERNEL_OVERLAY_SHA256"],
            "effective_config_sha256": sha(os.path.join(dist, "effective.config")),
            "cc": first_line(["gcc", "--version"]),
        },
        "busybox": {"package": pin["BUSYBOX_PKG"], "sha256": pin["BUSYBOX_SHA256"]},
        "guest_init": {"path": os.path.join(prof, "guest-init.sh"),
                       "sha256": sha(os.path.join(prof, "guest-init.sh"))},
        "artifacts": {},
    }
    fc_bin = os.environ.get("FC_BIN", "/usr/local/bin/firecracker")
    jailer_bin = os.environ.get("JAILER_BIN", "/usr/local/bin/jailer")
    manifest["engine"] = {
        "firecracker_path": fc_bin,
        "firecracker_version": first_line([fc_bin, "--version"]),
        "firecracker_sha256": sha(fc_bin),
        "jailer_path": jailer_bin,
        "jailer_version": first_line([jailer_bin, "--version"]),
        "jailer_sha256": sha(jailer_bin),
    }
    # axon-guest-init is pinned like axon: it is the in-guest policy channel
    # (reads `axon.policy=` from the kernel cmdline), so a swapped binary is a
    # swapped policy enforcer.
    # axon-psv-runner is the trusted suite-verdict runner (v022-psv-protocol.md
    # §4): it holds the per-attempt secret, so it is pinned like the others.
    for name in ("vmlinux", "rootfs.sqfs", "axon", "axon-guest-init", "axon-psv-runner"):
        p = os.path.join(dist, name)
        manifest["artifacts"][name] = {
            "path": os.path.join("dist/guest-linux", name),
            "sha256": sha(p),
            "bytes": os.path.getsize(p),
        }
    with open(os.path.join(dist, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")
    print(json.dumps({"artifacts": manifest["artifacts"], "engine": manifest["engine"]}, indent=2))


if __name__ == "__main__":
    main()
