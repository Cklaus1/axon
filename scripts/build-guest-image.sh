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

# The image is evidence about these sources (operator decision E): nothing may
# stand between them and the bytes -- no compiler wrapper (sccache is for
# DEVELOPMENT evidence runs only), no substituted rustc, no injected rustflags
# or linker, no artifact from an earlier build. That used to be a LIST of four
# wrapper variables and four config files; cargo resolves far more (ancestor
# configs, dotted keys, RUSTC, RUSTFLAGS, linkers, RUSTUP_TOOLCHAIN, a reused
# target dir), and two C9 round-4 reviewers built through each of them. So
# Round 7 (amendment 90): the controlled steps RUN AS ROOT and start every build
# process (cargo, build scripts, make) as an unprivileged uid; the build parent
# (AXON_GUEST_BUILD_PARENT, default /var/lib/axon-guest-build) must be traversable
# by that uid, and the pinned toolchain must be root-owned.
# every cargo run here goes through scripts/guest_build_env.py, which builds in
# an environment it CONSTRUCTS (caller env dropped; the pinned toolchain; a
# fresh CARGO_HOME and target dir; a private copy of the tracked tree under a
# directory only root or the builder can write) and refuses unless cargo's
# EFFECTIVE config is only the tree's own, before and after EVERY invocation,
# and unless the invocation is one of its table's exactly (round 4b). The
# kernel and the rootfs are made there too (`kernel`, `rootfs`). Its records
# go into the guest manifest, and the freeze refuses an image any of whose
# components was made any other way.
BUILD_ENV=""
gcargo_begin() {  # gcargo_begin <dir>: a fresh controlled build, recorded in <dir>/build-env.json
    BUILD_ENV="$1/build-env.json"
    rm -f "$BUILD_ENV"
    python3 scripts/guest_build_env.py begin "$BUILD_ENV" || exit 2
}
gcargo() {  # gcargo [--rustflags FLAGS] -- <cargo args>: cargo in the controlled environment
    python3 scripts/guest_build_env.py cargo "$BUILD_ENV" "$@"
}
gpath() {  # gpath <triple> <profile> <name>: where the controlled build put <name>
    python3 scripts/guest_build_env.py path "$BUILD_ENV" "$@"
}
if [[ "${1:-}" == "--build-env-only" ]]; then
    # The controlled environment alone, then stop: its refusal is exercised
    # through this production route by crates/axon-fabric/tests/guest_build_env.rs.
    mkdir -p "${2:?--build-env-only needs a directory}"
    gcargo_begin "$2"
    exit 0
fi

DIST="dist/guest"
KERNEL_ONLY="${1:-}"
BACKEND="${AXON_KERNEL_BACKEND:-axon}"

mkdir -p "$DIST"

# ── Axon guest kernel (default) ────────────────────────────────────────────────

build_kernel_axon() {
    echo "[build-guest-image] Building axon-guest-kernel (bare-metal, x86_64-axon-metal)..."

    # Build the kernel ELF.  Requires rust-src component and lld.
    # The custom target JSON is at crates/axon-guest-kernel/targets/x86_64-axon-metal.json.
    gcargo_begin "$DIST"
    gcargo --rustflags "-C target-feature=+crt-static" -- build -p axon-guest-kernel \
        -Z json-target-spec \
        --target "crates/axon-guest-kernel/targets/x86_64-axon-metal.json" \
        --release \
        -Z build-std=core,compiler_builtins \
        -Z build-std-features=compiler-builtins-mem \
        --quiet 2>&1

    local KERNEL_ELF
    KERNEL_ELF="$(gpath x86_64-axon-metal release axon-guest-kernel)"
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
    local TARBALL="$LDIST/linux-$KERNEL_VERSION.tar.xz"
    if [[ ! -f "$TARBALL" ]]; then
        echo "[build-guest-image] Downloading Linux $KERNEL_VERSION..."
        curl -fsSL -o "$TARBALL.part" "$KERNEL_URL"
        mv "$TARBALL.part" "$TARBALL"
    fi
    # C9 round 4b (FIELD-ORIGIN, amendment 63): the kernel is built in the
    # controlled environment too. guest_build_env.py copies the tarball, config
    # and overlay into a private directory, verifies each COPY against the pin,
    # and runs make there with a constructed environment (no caller KCFLAGS,
    # CROSS_COMPILE, CC, LLVM, MAKEFLAGS or PATH), recording the host toolchain
    # (gcc, cc1, as, ld, make, ...) in kernel-build.json, which the manifest
    # carries and the freeze judges.
    echo "[build-guest-image] Building Linux $KERNEL_VERSION (Firecracker v1.10.1 CI config, controlled)..."
    python3 scripts/guest_build_env.py kernel "$LDIST/kernel-build.json" "$LDIST" "$PROFILE_DIR" || exit 1
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
    # Taken BEFORE begin copies the tracked tree into the build's private
    # directory, so the copy lies between two clean observations of the tree.
    mkdir -p "$LDIST"
    python3 scripts/linux_profile_manifest.py --snapshot "$LDIST/provenance.pre.json"
    gcargo_begin "$LDIST"

    echo "[build-guest-image] Building axon interpreter (static musl, --locked)..."
    gcargo --rustflags "-C target-feature=+crt-static" -- build --locked -p axon-core \
            --target x86_64-unknown-linux-musl \
            --no-default-features --bin axon --release --quiet || exit 1
    local AXON_BIN
    AXON_BIN="$(gpath x86_64-unknown-linux-musl release axon)"
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
    gcargo --rustflags "-C target-feature=+crt-static" -- build --locked -p axon-guest-init \
            --target x86_64-unknown-linux-musl --release --quiet || exit 1
    local INIT_BIN
    INIT_BIN="$(gpath x86_64-unknown-linux-musl release axon-guest-init)"
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
    gcargo --rustflags "-C target-feature=+crt-static" -- build --locked -p axon-psv --bin axon-psv-runner \
            --target x86_64-unknown-linux-musl --release --quiet || exit 1
    local RUNNER_BIN
    RUNNER_BIN="$(gpath x86_64-unknown-linux-musl release axon-psv-runner)"
    # The digests of exactly what this controlled build produced; the freeze
    # requires the manifest's artifacts to be these.
    python3 scripts/guest_build_env.py finish "$BUILD_ENV" \
        "axon=$AXON_BIN" "axon-guest-init=$INIT_BIN" "axon-psv-runner=$RUNNER_BIN" || exit 1
    if ! file "$RUNNER_BIN" | grep -q 'static'; then
        echo "[build-guest-image] ERROR: $RUNNER_BIN is not statically linked" >&2
        exit 1
    fi

    # The root filesystem is assembled in the controlled environment
    # (round 4b, amendment 63): the record's own artifacts (re-verified against
    # their recorded digests), the pinned busybox (its COPY verified), the
    # tree's guest-init.sh as /init, and /usr/bin/mksquashfs under a
    # constructed environment; inputs and output are recorded.
    python3 scripts/guest_build_env.py rootfs "$BUILD_ENV" "$LDIST/rootfs.sqfs" || exit 1
    cp "$AXON_BIN" "$LDIST/axon"
    cp "$INIT_BIN" "$LDIST/axon-guest-init"
    cp "$RUNNER_BIN" "$LDIST/axon-psv-runner"
    # Round 5 (amendment 80): what sits in dist/ is byte for byte what the
    # controlled steps produced, and the record carries each digest (the manifest
    # and the freeze read artifact digests from the RECORD, not from the files).
    python3 scripts/guest_build_env.py dist "$BUILD_ENV" "$LDIST" || exit 1
    python3 scripts/guest_build_env.py discard "$BUILD_ENV"
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
    [[ -n "$BUILD_ENV" ]] || gcargo_begin "$DIST"
    gcargo --rustflags "-C target-feature=+crt-static" -- build -p axon-core \
            --target x86_64-unknown-linux-musl \
            --no-default-features \
            --bin axon \
            --release \
            --quiet || exit 1

    local AXON_BIN
    AXON_BIN="$(gpath x86_64-unknown-linux-musl release axon)"
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
        gcargo --rustflags "-C target-feature=+crt-static" -- build -p axon-guest-init \
                --target x86_64-unknown-linux-musl \
                --release \
                --quiet || exit 1
        local INIT_BIN
        INIT_BIN="$(gpath x86_64-unknown-linux-musl release axon-guest-init)"
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
