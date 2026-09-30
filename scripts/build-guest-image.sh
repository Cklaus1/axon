#!/usr/bin/env bash
# K5: Build the Axon guest image for Firecracker.
#
# Two kernel backends (select with AXON_KERNEL_BACKEND env var):
#
#   axon (default) — builds crates/axon-guest-kernel as a bare-metal ELF.
#                    TCB ~15K LOC, @[pure]/@[verify] syscall gate, <10ms boot.
#                    Requires: rustup component add rust-src + lld.
#
#   linux          — the pinned "protected Linux microVM" profile (B263):
#                    Linux 6.1.188 + Firecracker v1.10.1's CI guest config, and
#                    a read-only squashfs root (static busybox + static axon
#                    + static axon-guest-init, the in-guest policy channel).
#                    Pins: profiles/linux-microvm/kernel.pin (digest-checked).
#                    Outputs: dist/guest-linux/{vmlinux,rootfs.sqfs,manifest.json}.
#                    Requires: gcc make flex bison bc libelf-dev squashfs-tools
#                    busybox-static + rust target x86_64-unknown-linux-musl.
#
# Outputs:
#   dist/guest/vmlinuz          — kernel image (ELF or bzImage)
#   dist/guest/initramfs.cpio.gz — initramfs with axon binary as /usr/bin/axon
#                                  (no axon-guest-init when using axon backend;
#                                   the kernel handles policy and supervision)
#
# Usage:
#   ./scripts/build-guest-image.sh [--kernel-only] [--initrd-only]
#   AXON_KERNEL_BACKEND=linux ./scripts/build-guest-image.sh [--kernel-only|--rootfs-only]

set -euo pipefail
cd "$(dirname "$0")/.."

# The image is evidence about these sources (operator decision E): no compiler
# wrapper may stand between them and the bytes. A cache (sccache is installed
# for DEVELOPMENT evidence runs only) is an input this tree does not contain.
for v in RUSTC_WRAPPER RUSTC_WORKSPACE_WRAPPER CARGO_BUILD_RUSTC_WRAPPER CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER; do
    if [ -n "${!v:-}" ]; then
        echo "refused: $v is set (${!v}); a guest image is never built through a compiler wrapper" >&2
        exit 2
    fi
done
for f in "${CARGO_HOME:-$HOME/.cargo}"/config "${CARGO_HOME:-$HOME/.cargo}"/config.toml .cargo/config .cargo/config.toml; do
    if [ -f "$f" ] && grep -Eq '^[[:space:]]*rustc-(workspace-)?wrapper[[:space:]]*=' "$f"; then
        echo "refused: $f configures a rustc wrapper; a guest image is never built through one" >&2
        exit 2
    fi
done

DIST="dist/guest"
KERNEL_ONLY="${1:-}"
BACKEND="${AXON_KERNEL_BACKEND:-axon}"

mkdir -p "$DIST"

# ── Axon guest kernel (default) ────────────────────────────────────────────────

build_kernel_axon() {
    echo "[build-guest-image] Building axon-guest-kernel (bare-metal, x86_64-axon-metal)..."

    # Build the kernel ELF.  Requires rust-src component and lld.
    # The custom target JSON is at crates/axon-guest-kernel/targets/x86_64-axon-metal.json.
    RUSTFLAGS="-C target-feature=+crt-static" \
    cargo build -p axon-guest-kernel \
        -Z json-target-spec \
        --target "crates/axon-guest-kernel/targets/x86_64-axon-metal.json" \
        --release \
        -Z build-std=core,compiler_builtins \
        -Z build-std-features=compiler-builtins-mem \
        --quiet 2>&1

    local KERNEL_ELF="target/x86_64-axon-metal/release/axon-guest-kernel"
    if [[ ! -f "$KERNEL_ELF" ]]; then
        echo "[build-guest-image] ERROR: kernel ELF not found at $KERNEL_ELF"
        exit 1
    fi

    # Firecracker can boot flat ELF kernels directly (no bzImage wrapping needed
    # when using the --no-vmm-config path; or we strip to a raw binary).
    # Copy the ELF as vmlinuz for use by axon-vm.
    cp "$KERNEL_ELF" "$DIST/vmlinuz"
    echo "[build-guest-image] vmlinuz → $DIST/vmlinuz ($(du -sh "$DIST/vmlinuz" | cut -f1))"
}

# ── Linux fallback kernel ──────────────────────────────────────────────────────

#
# The pinned "protected Linux microVM" profile (B263). This replaces an
# unpinned legacy path (AXON_KERNEL_VERSION default 6.1.94, microvm_defconfig
# + ad-hoc `scripts/config` edits, no digest checks) that had never been built.
#
# Everything consumed here is pinned in profiles/linux-microvm/kernel.pin and
# VERIFIED before use: a tarball, config or busybox whose digest differs fails
# the build rather than producing an image that merely looks like the
# qualified one. Outputs go to dist/guest-linux/ (gitignored); their digests
# are written to dist/guest-linux/manifest.json, copied to the committed
# profiles/linux-microvm/manifest.json.
#
#   vmlinux       uncompressed ELF kernel (Firecracker's x86_64 boot format)
#   rootfs.sqfs   read-only squashfs root: static busybox + static axon + /init
#
# No initramfs and no axon-guest-init on this path: the profile is OFFLINE (no
# NIC, no MMDS), so the MMDS policy fetch axon-guest-init performs has nothing
# to talk to. The job and its result travel on a separate workspace drive —
# see profiles/linux-microvm/README.md.

LDIST="dist/guest-linux"
PROFILE_DIR="profiles/linux-microvm"

require_sha() {  # require_sha <file> <expected> <label>
    local got
    got="$(sha256sum "$1" | cut -d' ' -f1)"
    if [[ "$got" != "$2" ]]; then
        echo "[build-guest-image] ERROR: $3 sha256 mismatch: got $got, pinned $2" >&2
        exit 1
    fi
}

load_pin() {
    # shellcheck source=/dev/null
    source "$PROFILE_DIR/kernel.pin"
    mkdir -p "$LDIST"
}

build_kernel_linux() {
    load_pin
    local KSRC="$LDIST/linux-$KERNEL_VERSION"
    local TARBALL="$LDIST/linux-$KERNEL_VERSION.tar.xz"
    local CONFIG="$PROFILE_DIR/$KERNEL_CONFIG"

    require_sha "$CONFIG" "$KERNEL_CONFIG_SHA256" "kernel config"
    if [[ ! -f "$TARBALL" ]]; then
        echo "[build-guest-image] Downloading Linux $KERNEL_VERSION..."
        curl -fsSL -o "$TARBALL.part" "$KERNEL_URL"
        mv "$TARBALL.part" "$TARBALL"
    fi
    require_sha "$TARBALL" "$KERNEL_TARBALL_SHA256" "kernel tarball"

    # Always re-extract: a pre-existing tree could carry edits the pin cannot see.
    rm -rf "$KSRC"
    tar -xf "$TARBALL" -C "$LDIST"
    require_sha "$PROFILE_DIR/$KERNEL_OVERLAY" "$KERNEL_OVERLAY_SHA256" "kernel config overlay"
    cp "$CONFIG" "$KSRC/.config"
    grep -E '^CONFIG_' "$PROFILE_DIR/$KERNEL_OVERLAY" >> "$KSRC/.config"

    echo "[build-guest-image] Building Linux $KERNEL_VERSION (Firecracker v1.10.1 CI config)..."
    (
        cd "$KSRC"
        make ARCH=x86_64 olddefconfig > /dev/null
        KBUILD_BUILD_TIMESTAMP="1970-01-01" KBUILD_BUILD_USER=axon \
        KBUILD_BUILD_HOST=b263 KBUILD_BUILD_VERSION=1 \
            make ARCH=x86_64 -j"$(nproc)" vmlinux 2>&1 | tail -3
    )
    [[ -f "$KSRC/vmlinux" ]] || { echo "[build-guest-image] ERROR: vmlinux not built" >&2; exit 1; }
    cp "$KSRC/vmlinux" "$LDIST/vmlinux"
    cp "$KSRC/.config" "$LDIST/effective.config"
    echo "[build-guest-image] vmlinux → $LDIST/vmlinux ($(du -sh "$LDIST/vmlinux" | cut -f1))"
}

build_rootfs_linux() {
    load_pin
    # The guest axon must carry the certified PCI interpreter (the survey found
    # the pinned one predated it): refuse a revision that does not descend from
    # the PCI certification (governance/proofs/v022-pci/CERTIFICATION.md).
    # This early check is a DEVELOPMENT check (a linked worktree may pass it);
    # the manifest binds the protected answer itself.
    #
    # EVIDENCE BUILDS (amendment 44, decisions C and E): the manifest is clean
    # only from a STANDALONE CLONE (a linked worktree's gitfile is dirty), in
    # which every object outside HEAD's tree is dirty unless the operator's
    # root-owned /etc/axon/provenance-allowlist excuses it (`target/` and
    # `dist/` for this build). .gitignore excuses nothing.
    python3 scripts/linux_profile_manifest.py --descends 31413ca7 || {
        echo "[build-guest-image] ERROR: HEAD does not descend from PCI-certified 31413ca7" >&2
        exit 1
    }
    require_sha "$BUSYBOX_SRC" "$BUSYBOX_SHA256" "busybox"
    # The tree the binaries are built FROM, before any build step (the same
    # Rust provenance that stamps the readiness verifier). The manifest is
    # clean only if this and the tree at manifest time are clean and agree.
    mkdir -p "$LDIST"
    python3 scripts/linux_profile_manifest.py --snapshot "$LDIST/provenance.pre.json"

    echo "[build-guest-image] Building axon interpreter (static musl, --locked)..."
    RUSTFLAGS="-C target-feature=+crt-static" \
        cargo build --locked -p axon-core \
            --target x86_64-unknown-linux-musl \
            --no-default-features --bin axon --release --quiet
    local AXON_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-musl/release/axon"
    if ! file "$AXON_BIN" | grep -q 'static'; then
        echo "[build-guest-image] ERROR: $AXON_BIN is not statically linked" >&2
        exit 1
    fi

    # axon-guest-init: the in-guest policy channel (ACF-G25 / x1). guest-init.sh
    # execs the workload under it; it reads `axon.policy=` from /proc/cmdline and
    # refuses to start the workload without a policy that constrains something.
    # DEFAULT FEATURES ONLY: `dev-allow-no-policy` compiles in a runtime
    # no-policy escape, and must never reach an image.
    echo "[build-guest-image] Building axon-guest-init (static musl, --locked, default features)..."
    RUSTFLAGS="-C target-feature=+crt-static" \
        cargo build --locked -p axon-guest-init \
            --target x86_64-unknown-linux-musl --release --quiet
    local INIT_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-musl/release/axon-guest-init"
    if ! file "$INIT_BIN" | grep -q 'static'; then
        echo "[build-guest-image] ERROR: $INIT_BIN is not statically linked" >&2
        exit 1
    fi
    # The artefact-level check that the bypass is absent: the only code that
    # spells the variable's name is compiled out of a default build.
    if grep -qa 'AXON_GUEST_ALLOW_NO_POLICY' "$INIT_BIN"; then
        echo "[build-guest-image] ERROR: $INIT_BIN contains the no-policy bypass" >&2
        exit 1
    fi

    # axon-psv-runner: the trusted suite-verdict runner (v022-psv-protocol.md §4).
    echo "[build-guest-image] Building axon-psv-runner (static musl, --locked)..."
    RUSTFLAGS="-C target-feature=+crt-static" \
        cargo build --locked -p axon-psv --bin axon-psv-runner \
            --target x86_64-unknown-linux-musl --release --quiet
    local RUNNER_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-musl/release/axon-psv-runner"
    if ! file "$RUNNER_BIN" | grep -q 'static'; then
        echo "[build-guest-image] ERROR: $RUNNER_BIN is not statically linked" >&2
        exit 1
    fi

    local STAGE
    STAGE="$(mktemp -d)"
    trap 'rm -rf "${STAGE:-}"' EXIT
    # mktemp -d makes the stage 0700 and it becomes the image's `/`: nothing
    # unprivileged could traverse it (measured: the PSV runner's uid-dropped
    # `axon test` got EACCES at execve). Every workload used to run as root, so
    # it never showed.
    chmod 0755 "$STAGE"
    mkdir -p "$STAGE"/{bin,usr/bin,proc,sys,dev,tmp,work,out,in/candidate,in/suite,in/job}
    cp "$BUSYBOX_SRC" "$STAGE/bin/busybox"
    local applet
    for applet in $("$STAGE/bin/busybox" --list); do
        [[ "$applet" == busybox ]] || ln -s busybox "$STAGE/bin/$applet"
    done
    cp "$AXON_BIN" "$STAGE/usr/bin/axon"
    cp "$INIT_BIN" "$STAGE/usr/bin/axon-guest-init"
    cp "$RUNNER_BIN" "$STAGE/usr/bin/axon-psv-runner"
    cp "$PROFILE_DIR/guest-init.sh" "$STAGE/init"
    chmod 0755 "$STAGE/init" "$STAGE/usr/bin/axon" "$STAGE/usr/bin/axon-guest-init" \
        "$STAGE/usr/bin/axon-psv-runner" "$STAGE/bin/busybox"

    rm -f "$LDIST/rootfs.sqfs"
    # -all-time/-mkfs-time 0 + -all-root: the image is a function of its inputs.
    mksquashfs "$STAGE" "$LDIST/rootfs.sqfs" -noappend -all-root -no-xattrs \
        -mkfs-time 0 -all-time 0 -comp gzip -quiet
    cp "$AXON_BIN" "$LDIST/axon"
    cp "$INIT_BIN" "$LDIST/axon-guest-init"
    cp "$RUNNER_BIN" "$LDIST/axon-psv-runner"
    # The image's root must be traversable by the unprivileged test uid.
    local ROOTMODE
    # `sed -n 1p`, not `head -1`: head closes the pipe early, and under
    # pipefail unsquashfs's SIGPIPE ended one build silently with 141.
    ROOTMODE="$(unsquashfs -lln "$LDIST/rootfs.sqfs" 2>/dev/null | sed -n 1p | cut -c1-10)"
    if [[ "$ROOTMODE" != drwxr-xr-x ]]; then
        echo "[build-guest-image] ERROR: rootfs / is $ROOTMODE, not drwxr-xr-x" >&2
        exit 1
    fi
    echo "[build-guest-image] rootfs → $LDIST/rootfs.sqfs ($(du -sh "$LDIST/rootfs.sqfs" | cut -f1))"
}

write_manifest_linux() {
    python3 scripts/linux_profile_manifest.py --pre "$LDIST/provenance.pre.json" "$LDIST" "$PROFILE_DIR"
    cp "$LDIST/manifest.json" "$PROFILE_DIR/manifest.json"
    echo "[build-guest-image] manifest → $LDIST/manifest.json (+ $PROFILE_DIR/manifest.json)"
}

build_kernel() {
    if [[ "$BACKEND" == "linux" ]]; then
        build_kernel_linux
    else
        build_kernel_axon
    fi
}

# ── initramfs ──────────────────────────────────────────────────────────────────

build_initramfs() {
    echo "[build-guest-image] Building axon interpreter (static musl)..."
    RUSTFLAGS="-C target-feature=+crt-static" \
        cargo build -p axon-core \
            --target x86_64-unknown-linux-musl \
            --no-default-features \
            --bin axon \
            --release \
            --quiet

    local AXON_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-musl/release/axon"
    local INITDIR
    INITDIR="$(mktemp -d)"
    # AUDIT T12: INITDIR is `local` to this function, but an EXIT trap runs in
    # global scope AFTER the function has returned — where the name is unbound.
    # Under `set -u` that made the script exit 1 at the very end, after all the
    # real work had succeeded. Default the expansion so cleanup is best-effort
    # rather than fatal. (Latent until now: the script previously died earlier,
    # on the json-target-spec error, so the trap never ran.)
    trap 'rm -rf "${INITDIR:-}"' EXIT

    mkdir -p "$INITDIR"/{dev,proc,sys,tmp,axon,usr/bin}
    cp "$AXON_BIN" "$INITDIR/usr/bin/axon"
    chmod +x "$INITDIR/usr/bin/axon"
    strip "$INITDIR/usr/bin/axon" 2>/dev/null || true

    if [[ "$BACKEND" == "linux" ]]; then
        # Linux backend: include axon-guest-init as /init (PID-1 supervisor).
        echo "[build-guest-image] Building axon-guest-init (static musl)..."
        RUSTFLAGS="-C target-feature=+crt-static" \
            cargo build -p axon-guest-init \
                --target x86_64-unknown-linux-musl \
                --release \
                --quiet
        local INIT_BIN="${CARGO_TARGET_DIR:-target}/x86_64-unknown-linux-musl/release/axon-guest-init"
        cp "$INIT_BIN" "$INITDIR/init"
        chmod +x "$INITDIR/init"
        strip "$INITDIR/init" 2>/dev/null || true
        echo "[build-guest-image]   init:  $(du -sh "$INIT_BIN" | cut -f1)"
    else
        # Axon-kernel backend: no /init needed — kernel execs /usr/bin/axon directly.
        # Write a minimal stub so the CPIO isn't empty.
        printf '#!/bin/sh\nexec /usr/bin/axon run /axon/program.ax\n' > "$INITDIR/init"
        chmod +x "$INITDIR/init"
    fi

    pushd "$INITDIR" > /dev/null
    find . | cpio -o -H newc | gzip -9 > "$OLDPWD/$DIST/initramfs.cpio.gz"
    popd > /dev/null

    echo "[build-guest-image] initramfs → $DIST/initramfs.cpio.gz ($(du -sh "$DIST/initramfs.cpio.gz" | cut -f1))"
    echo "[build-guest-image]   axon:  $(du -sh "$AXON_BIN" | cut -f1)"
}

# ── Main ───────────────────────────────────────────────────────────────────────

if [[ "$BACKEND" == "linux" ]]; then
    case "${KERNEL_ONLY:-}" in
        --kernel-only) build_kernel_linux ;;
        --rootfs-only) build_rootfs_linux ;;
        *) build_kernel_linux; build_rootfs_linux ;;
    esac
    write_manifest_linux
    echo ""
    echo "[build-guest-image] Done (backend=linux, profile=linux-microvm-protected)."
    echo "  Kernel: $LDIST/vmlinux   Rootfs: $LDIST/rootfs.sqfs"
    echo "  Launch: scripts/fc_linux_profile.sh --program prog.ax --out DIR"
    exit 0
fi

case "${KERNEL_ONLY:-}" in
    --kernel-only) build_kernel ;;
    --initrd-only) build_initramfs ;;
    *)
        build_kernel
        build_initramfs
        ;;
esac

echo ""
echo "[build-guest-image] Done (backend=$BACKEND)."
echo "  Kernel:    $DIST/vmlinuz"
echo "  Initramfs: $DIST/initramfs.cpio.gz"
echo ""
echo "Boot:"
echo "  axon-vm run --kernel $DIST/vmlinuz --initrd $DIST/initramfs.cpio.gz program.ax"
