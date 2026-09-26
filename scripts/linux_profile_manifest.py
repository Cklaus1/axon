#!/usr/bin/env python3
"""Write dist/guest-linux/manifest.json for the protected Linux microVM profile.

Usage: linux_profile_manifest.py <dist-dir> <profile-dir>

Every artifact the launcher consumes is recorded by sha256. The launcher
(scripts/fc_linux_profile.sh) re-hashes each artifact against THIS manifest
before booting, so an artifact swapped after the build refuses to launch.

The VMM ENGINE (firecracker + jailer) is pinned here too, in `engine`, with the
exact field names axon-fabric's qualification() compares an evidence record's
`engine` block against. It was not pinned at all: the launcher exec'd whatever
sat at /usr/local/bin, so a swapped VMM ran with the same qualification as the
one that was measured. FC_BIN / JAILER_BIN override the paths pinned (the
launcher's defaults are the same two paths).
"""
import hashlib
import json
import os
import subprocess
import sys


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


def main():
    dist, prof = sys.argv[1], sys.argv[2]
    pin = {}
    for line in open(os.path.join(prof, "kernel.pin")):
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, v = line.split("=", 1)
            pin[k] = v
    rev = first_line(["git", "rev-parse", "HEAD"])
    dirty = bool(first_line(["git", "status", "--porcelain", "--untracked-files=no"]).strip()
                 not in ("", "unknown"))
    manifest = {
        "schema": "axon-linux-microvm-profile/1",
        "profile": "linux-microvm-protected",
        "guest_truth": "Linux kernel (NOT the axon-guest-kernel)",
        "source": {
            "axon_git_rev_at_build": rev,
            "axon_tree_dirty_at_build": dirty,
            "axon_build": "RUSTFLAGS='-C target-feature=+crt-static' cargo build --locked "
                          "-p axon-core --no-default-features --bin axon --release "
                          "--target x86_64-unknown-linux-musl",
            "axon_guest_init_build": "RUSTFLAGS='-C target-feature=+crt-static' cargo build "
                                     "--locked -p axon-guest-init --release "
                                     "--target x86_64-unknown-linux-musl (default features: "
                                     "no dev-allow-no-policy bypass)",
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
    for name in ("vmlinux", "rootfs.sqfs", "axon", "axon-guest-init"):
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
