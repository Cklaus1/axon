#!/usr/bin/env bash
# release_package.sh - build, smoke-test and package the static x86_64 Linux interpreter
# (release-contract RC-4; spec/stability.md section 1).
#
# Usage: scripts/release_package.sh <outdir> [--keep-env]
#
# Builds `axon` (interpreter, --no-default-features) for x86_64-unknown-linux-musl with
# +crt-static from this checkout's HEAD, then judges it (each a hard failure, rc 1):
#   - `file` says static;
#   - the version line is `axon <Cargo version> (<git rev-parse --short HEAD>)`: no -dirty,
#     no unknown, no packager override;
#   - `axon card --sha256` and `--card-version` are the last line of card-versions.txt;
#   - examples/hello.ax runs from its directory with rc 0 and stdout `Hello, Axon!`;
#   - scripts/compat_sweep.py with this binary writes compat.tsv.
# Then writes into <outdir>: axon-<ver>-x86_64-unknown-linux-musl.tar.gz (axon, LICENSE,
# card.md, compat.tsv), compat.tsv, and SHA256SUMS over those two.
#
# Refuses before building (rc 1):
#   - AXON_GIT_SHA set (build.rs would embed it verbatim instead of the commit), unless
#     --keep-env, which exists only to show the version judge refusing such a build;
#   - an <outdir> inside the checkout other than under the ignored /dist/ (an untracked file
#     makes build.rs embed `-dirty`).
# The release workflow (.github/workflows/release.yml) never passes --keep-env.
set -euo pipefail

TRIPLE=x86_64-unknown-linux-musl
die() { echo "release_package: FAIL: $*" >&2; exit 1; }

OUT=""
KEEP_ENV=0
for a in "$@"; do
  case "$a" in
    --keep-env) KEEP_ENV=1 ;;
    -*) die "unknown option $a (usage: release_package.sh <outdir> [--keep-env])" ;;
    *) [ -z "$OUT" ] || die "two output directories: $OUT and $a"; OUT="$a" ;;
  esac
done
[ -n "$OUT" ] || die "usage: release_package.sh <outdir> [--keep-env]"

if [ -n "${AXON_GIT_SHA+set}" ] && [ "$KEEP_ENV" = 0 ]; then
  die "AXON_GIT_SHA is set ('$AXON_GIT_SHA'); a release binary names its own commit (unset it)"
fi

REPO="$(cd "$(dirname "$0")/.." && pwd -P)"
cd "$REPO"
OUT="$(realpath -m "$OUT")"
case "$OUT/" in
  "$REPO/dist/"*) ;;
  "$REPO/"*) die "<outdir> $OUT is inside the checkout and not under the ignored $REPO/dist/ (build.rs would see -dirty)" ;;
esac

mkdir -p "$OUT"
VER="$(python3 -c 'import tomllib,sys; print(tomllib.load(open("Cargo.toml","rb"))["workspace"]["package"]["version"])')"
SHA="$(git rev-parse --short HEAD)"
NAME="axon-$VER-$TRIPLE"
echo "release_package: building $NAME at $SHA"

# The binary is the one cargo reports it built (never a guessed target/ path).
BIN="$(RUSTFLAGS="-C target-feature=+crt-static" cargo build --locked --release -p axon-core \
        --no-default-features --bin axon --target "$TRIPLE" --message-format=json-render-diagnostics |
      python3 -c '
import json, sys
exe = None
for line in sys.stdin:
    try:
        m = json.loads(line)
    except ValueError:
        continue
    if m.get("reason") == "compiler-artifact" and m.get("target", {}).get("name") == "axon" and m.get("executable"):
        exe = m["executable"]
print(exe or "")
')" || die "cargo build failed"
[ -n "$BIN" ] && [ -x "$BIN" ] || die "cargo reported no axon executable"

# Smoke: every judge below is a hard failure.
file "$BIN" | grep -q 'static' || die "not statically linked: $(file "$BIN")"
want="axon $VER ($SHA)"
got="$("$BIN" --version)"
[ "$got" = "$want" ] || die "version line: got '$got', want '$want'"
CARD_LAST="$(tail -n 1 crates/axon-core/card-versions.txt)"
want_n="${CARD_LAST%% *}"
want_sha="${CARD_LAST#* }"
got_sha="$("$BIN" card --sha256)"
[ "$got_sha" = "$want_sha" ] || die "card sha256: got $got_sha, card-versions.txt card $want_n is $want_sha"
got_n="$("$BIN" card --card-version)"
[ "$got_n" = "$want_n" ] || die "card version: got $got_n, card-versions.txt ends at $want_n"
hello="$(cd examples && "$BIN" run hello.ax)" || die "examples/hello.ax: rc $?"
[ "$hello" = "Hello, Axon!" ] || die "examples/hello.ax printed '$hello', want 'Hello, Axon!'"

STAGE="$OUT/$NAME"
rm -rf "$STAGE"
mkdir -p "$STAGE"
python3 -B scripts/compat_sweep.py --bin "$BIN" --out "$STAGE/compat.tsv" || die "compat sweep"
cp "$BIN" "$STAGE/axon"
cp LICENSE "$STAGE/LICENSE"
"$BIN" card > "$STAGE/card.md"

# Reproducible archive: sorted names, fixed owner, the commit's own time.
MTIME="$(git log -1 --format=%ct HEAD)"
tar -C "$OUT" --sort=name --owner=0 --group=0 --numeric-owner --mtime="@$MTIME" \
    -cf - "$NAME" | gzip -n > "$OUT/$NAME.tar.gz"
cp "$STAGE/compat.tsv" "$OUT/compat.tsv"
rm -rf "$STAGE"
(cd "$OUT" && sha256sum "$NAME.tar.gz" compat.tsv > SHA256SUMS)
echo "release_package: PASS $got -> $OUT ($NAME.tar.gz, compat.tsv, SHA256SUMS)"
