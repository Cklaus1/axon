#!/usr/bin/env bash
# install.sh — install the `axon` compiler together with its native runtimes.
#
# usage: scripts/install.sh [--prefix DIR]        (default DIR: $HOME/.local)
#
# WHY THIS EXISTS (AX-35).
#
# `axon build` links every native binary against a runtime staticlib
# (`libaxon_rt.a`, or `libaxon_rt_ai.a` for a program that calls an AI builtin).
# A compiler run from its workspace finds that lib in the cargo target dir it
# was built into. A compiler copied anywhere else needs the libs built from the
# SAME sources with the SAME toolchain next to it, and this script produces that
# layout — the one `axon build` searches beside its executable
# (codegen/link.rs, `RuntimeLookup::installed_dir`):
#
#   DIR/bin/axon
#   DIR/lib/axon/runtime/release/libaxon_rt.a      linked by --release builds
#   DIR/lib/axon/runtime/release/libaxon_rt_ai.a
#   DIR/lib/axon/runtime/debug/libaxon_rt.a        linked by debug builds
#   DIR/lib/axon/runtime/debug/libaxon_rt_ai.a
#
# Each `runtime/<profile>` directory has the shape AXON_RUNTIME_DIR takes, and a
# cross runtime goes in `runtime/<profile>/<triple>/` (this script installs the
# host's only). The installed compiler needs neither cargo nor the workspace.
#
# Everything is built with the workspace's pinned toolchain (rust-toolchain.toml):
# the script runs cargo from the workspace root and first clears the variables
# that would redirect it — the list `redirects_runtime_toolchain` in link.rs
# documents and clears for the compiler's own runtime builds.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PREFIX="${HOME:?}/.local"

usage() { sed -n '2,4p' "$0" | sed 's/^# \{0,1\}//'; }
while [ $# -gt 0 ]; do
    case "$1" in
        --prefix) PREFIX="${2:?--prefix needs a directory}"; shift 2 ;;
        --prefix=*) PREFIX="${1#--prefix=}"; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "install.sh: unknown argument '$1'" >&2; usage >&2; exit 2 ;;
    esac
done

unset RUSTUP_TOOLCHAIN RUSTC CARGO_BUILD_RUSTC RUSTC_WRAPPER CARGO_BUILD_RUSTC_WRAPPER \
    RUSTC_WORKSPACE_WRAPPER CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER RUSTFLAGS \
    CARGO_ENCODED_RUSTFLAGS CARGO_BUILD_RUSTFLAGS CARGO_BUILD_TARGET \
    CARGO_BUILD_DEP_INFO_BASEDIR
for var in $(compgen -e); do
    case "$var" in
        CARGO_PROFILE_*|CARGO_TARGET_*_RUSTFLAGS) unset "$var" ;;
    esac
done

cd "$ROOT"
# Where cargo puts artifacts ($CARGO_TARGET_DIR or a .cargo/config target-dir
# included), asked of cargo rather than assumed.
TARGET_DIR="$(cargo metadata --format-version 1 --no-deps \
    | grep -o '"target_directory":"[^"]*"' | cut -d'"' -f4)"
[ -n "$TARGET_DIR" ] || { echo "install.sh: cargo metadata reported no target_directory" >&2; exit 1; }

cargo build --release -p axon-core --bin axon
for profile in release debug; do
    flag=""; [ "$profile" = release ] && flag="--release"
    cargo build $flag -p axon-rt -p axon-rt-ai
done

install -d "$PREFIX/bin"
install -m 755 "$TARGET_DIR/release/axon" "$PREFIX/bin/axon"
for profile in release debug; do
    dest="$PREFIX/lib/axon/runtime/$profile"
    install -d "$dest"
    for lib in libaxon_rt.a libaxon_rt_ai.a; do
        install -m 644 "$TARGET_DIR/$profile/$lib" "$dest/$lib"
    done
done
echo "installed $PREFIX/bin/axon with its runtimes in $PREFIX/lib/axon/runtime/{release,debug}"
