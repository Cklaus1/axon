#!/usr/bin/env bash
# K5: Build the Axon guest image for Firecracker.
#
# Two kernel backends (select with AXON_KERNEL_BACKEND env var):
#
#   axon (default) — builds crates/axon-guest-kernel as a bare-metal ELF.
#                    TCB ~15K LOC, @[pure]/@[verify] syscall gate, <10ms boot.
#                    Requires: rustup component add rust-src + lld.
#
#   linux          — falls back to Linux 6.1 microvm_defconfig (~7 MB bzImage).
#                    TCB ~35M LOC, seccomp enforcement.  Takes ~3 min to build.
#                    Requires: gcc make flex bison bc; AXON_KERNEL_VERSION to pin.
#
# Outputs:
#   dist/guest/vmlinuz          — kernel image (ELF or bzImage)
#   dist/guest/initramfs.cpio.gz — initramfs with axon binary as /usr/bin/axon
#                                  (no axon-guest-init when using axon backend;
#                                   the kernel handles policy and supervision)
#
# Usage:
#   ./scripts/build-guest-image.sh [--kernel-only] [--initrd-only]
#   AXON_KERNEL_BACKEND=linux ./scripts/build-guest-image.sh  # legacy path

set -euo pipefail
cd "$(dirname "$0")/.."

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

build_kernel_linux() {
    local KVER="${AXON_KERNEL_VERSION:-6.1.94}"
    local KSRC="$DIST/linux-$KVER"
    local BZIMAGE="$KSRC/arch/x86/boot/bzImage"

    if [[ -f "$DIST/vmlinuz" ]]; then
        echo "[build-guest-image] vmlinuz exists, skipping Linux kernel build"
        return
    fi

    if [[ ! -d "$KSRC" ]]; then
        echo "[build-guest-image] Downloading Linux $KVER..."
        KMAJOR="${KVER%%.*}"
        wget -q -O "$DIST/linux-$KVER.tar.xz" \
            "https://cdn.kernel.org/pub/linux/kernel/v${KMAJOR}.x/linux-${KVER}.tar.xz"
        tar -xf "$DIST/linux-$KVER.tar.xz" -C "$DIST"
        rm -f "$DIST/linux-$KVER.tar.xz"
    fi

    echo "[build-guest-image] Configuring Linux for Firecracker microVM..."
    pushd "$KSRC" > /dev/null
    make ARCH=x86_64 microvm_defconfig
    scripts/config --enable VIRTIO_NET
    scripts/config --enable HW_RANDOM_VIRTIO
    scripts/config --enable KVM_GUEST
    scripts/config --enable PARAVIRT_CLOCK
    scripts/config --enable VSOCK
    scripts/config --enable VIRTIO_VSOCK
    scripts/config --enable TMPFS
    scripts/config --disable MODULES
    scripts/config --disable DEBUG_KERNEL
    make ARCH=x86_64 -j"$(nproc)" bzImage 2>&1 | tail -3
    popd > /dev/null
    cp "$BZIMAGE" "$DIST/vmlinuz"
    echo "[build-guest-image] vmlinuz → $DIST/vmlinuz ($(du -sh "$DIST/vmlinuz" | cut -f1))"
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

    local AXON_BIN="target/x86_64-unknown-linux-musl/release/axon"
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
        local INIT_BIN="target/x86_64-unknown-linux-musl/release/axon-guest-init"
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
