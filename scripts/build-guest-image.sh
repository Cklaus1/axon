#!/usr/bin/env bash
# Gap 3: Build a minimal Firecracker guest image for Axon programs.
#
# Outputs:
#   ./dist/guest/vmlinuz       — stripped Linux kernel (x86_64, ~7MB)
#   ./dist/guest/initramfs.cpio.gz — initramfs containing axon-guest-init + axon binary
#
# Requirements (install once):
#   apt-get install -y gcc make flex bison bc cpio gzip qemu-system-x86
#   rustup target add x86_64-unknown-linux-musl
#   apt-get install -y musl-tools
#
# Usage:
#   ./scripts/build-guest-image.sh [--kernel-only] [--initrd-only]
#
# Kernel config note:
#   Uses a minimal defconfig + microvm.config (no PCI, no USB, KVM guest clock).
#   The resulting bzImage boots in Firecracker. Expected kernel build time: ~3 min.
#   Snapshot-warm kernels can be pre-built and cached (see AXON_VM.md §Snapshot pool).

set -euo pipefail
cd "$(dirname "$0")/.."

DIST="dist/guest"
KERNEL_ONLY="${1:-}"
INITRD_ONLY="${2:-}"

mkdir -p "$DIST"

# ── Kernel ─────────────────────────────────────────────────────────────────────

build_kernel() {
    local KVER="${AXON_KERNEL_VERSION:-6.1.94}"
    local KSRC="$DIST/linux-$KVER"
    local TARBALL="$DIST/linux-$KVER.tar.xz"
    local BZIMAGE="$KSRC/arch/x86/boot/bzImage"

    if [[ -f "$DIST/vmlinuz" ]]; then
        echo "[build-guest-image] vmlinuz exists, skipping kernel build"
        return
    fi

    # Download kernel source if needed.
    if [[ ! -d "$KSRC" ]]; then
        echo "[build-guest-image] Downloading Linux $KVER..."
        KMAJOR="${KVER%%.*}"
        wget -q -O "$TARBALL" \
            "https://cdn.kernel.org/pub/linux/kernel/v${KMAJOR}.x/linux-${KVER}.tar.xz"
        tar -xf "$TARBALL" -C "$DIST"
        rm -f "$TARBALL"
    fi

    echo "[build-guest-image] Configuring kernel for Firecracker microVM..."
    pushd "$KSRC" > /dev/null

    # Start from the stripped-down microvm defconfig (no PCI, USB, framebuffer).
    make ARCH=x86_64 microvm_defconfig

    # Enable only what Axon guest needs:
    #   - virtio-net (networking)
    #   - virtio-blk (root disk, optional)
    #   - virtio-rng (entropy from host for reproducible seeds)
    #   - KVM guest clock (accurate timing without VDSO)
    #   - AF_VSOCK + virtio-vsock (host_await substrate)
    #   - tmpfs (writable /tmp without a block device)
    scripts/config --enable VIRTIO_NET
    scripts/config --enable VIRTIO_BLK
    scripts/config --enable HW_RANDOM_VIRTIO
    scripts/config --enable KVM_GUEST
    scripts/config --enable PARAVIRT_CLOCK
    scripts/config --enable VSOCK
    scripts/config --enable VIRTIO_VSOCK
    scripts/config --enable TMPFS
    scripts/config --enable OVERLAY_FS

    # Minimize kernel size.
    scripts/config --disable MODULES
    scripts/config --disable DEBUG_KERNEL
    scripts/config --disable EFI
    scripts/config --disable ACPI

    make ARCH=x86_64 -j"$(nproc)" bzImage 2>&1 | tail -5

    popd > /dev/null
    cp "$BZIMAGE" "$DIST/vmlinuz"
    echo "[build-guest-image] vmlinuz → $DIST/vmlinuz ($(du -sh "$DIST/vmlinuz" | cut -f1))"
}

# ── initramfs ──────────────────────────────────────────────────────────────────

build_initramfs() {
    echo "[build-guest-image] Building axon-guest-init (static musl)..."
    RUSTFLAGS="-C target-feature=+crt-static" \
        cargo build -p axon-guest-init \
            --target x86_64-unknown-linux-musl \
            --release \
            --quiet

    echo "[build-guest-image] Building axon interpreter (static musl)..."
    RUSTFLAGS="-C target-feature=+crt-static" \
        cargo build -p axon-core \
            --target x86_64-unknown-linux-musl \
            --no-default-features \
            --bin axon \
            --release \
            --quiet

    local INIT_BIN="target/x86_64-unknown-linux-musl/release/axon-guest-init"
    local AXON_BIN="target/x86_64-unknown-linux-musl/release/axon"

    echo "[build-guest-image] Building initramfs..."
    local INITDIR
    INITDIR="$(mktemp -d)"
    trap 'rm -rf "$INITDIR"' EXIT

    # Minimal /dev nodes for serial + random devices.
    mkdir -p "$INITDIR"/{dev,proc,sys,tmp,axon,usr/bin}

    cp "$INIT_BIN" "$INITDIR/init"
    cp "$AXON_BIN" "$INITDIR/usr/bin/axon"

    chmod +x "$INITDIR/init" "$INITDIR/usr/bin/axon"

    # Strip debug symbols for size.
    strip "$INITDIR/init" "$INITDIR/usr/bin/axon" 2>/dev/null || true

    # Create the CPIO archive.
    pushd "$INITDIR" > /dev/null
    find . | cpio -o -H newc | gzip -9 > "$OLDPWD/$DIST/initramfs.cpio.gz"
    popd > /dev/null

    echo "[build-guest-image] initramfs → $DIST/initramfs.cpio.gz ($(du -sh "$DIST/initramfs.cpio.gz" | cut -f1))"
    echo "[build-guest-image]   init:  $(du -sh "$INIT_BIN" | cut -f1) stripped"
    echo "[build-guest-image]   axon:  $(du -sh "$AXON_BIN" | cut -f1) stripped"
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

echo "[build-guest-image] Done. Guest image:"
echo "  Kernel:    $DIST/vmlinuz"
echo "  Initramfs: $DIST/initramfs.cpio.gz"
echo ""
echo "Boot with Firecracker:"
echo "  axon-vm run --kernel $DIST/vmlinuz --initrd $DIST/initramfs.cpio.gz program.ax"
