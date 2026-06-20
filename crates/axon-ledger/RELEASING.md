# Release checklist for axon-ledger

## Before first release

1. **Push the repo publicly**
   ```bash
   # On GitHub: create new public repo "axon" under your account
   git remote set-url origin https://github.com/<your-github-username>/axon.git
   git push -u origin main
   ```

2. **Update URLs in these files** (replace `cklaus` with your GitHub username):
   - `crates/axon-ledger/Cargo.toml` → `repository`
   - `crates/axon-ledger/LEDGER.md` → `cargo install --git` URL
   - `crates/axon-ledger/install.sh` → `REPO` variable

3. **Verify the crate packages cleanly**
   ```bash
   cargo package -p axon-ledger --no-verify
   # Should output: Packaged N files, ~86 KiB
   ```

4. **Smoke test the install from git**
   ```bash
   cargo install --git https://github.com/<username>/axon axon-ledger
   axon-ledger --version
   ```

## Publish to crates.io

```bash
# Login once
cargo login  # paste token from crates.io/me

# Publish
cargo publish -p axon-ledger
```

`axon-ledger` has no intra-workspace dependencies — it publishes standalone.

## Tag the release

```bash
git tag axon-ledger-v0.1.0
git push origin axon-ledger-v0.1.0
```

## Build a static Linux binary (optional, for direct download)

```bash
# Install cross (one-time)
cargo install cross

# Build static x86_64 Linux binary
cross build -p axon-ledger --release --target x86_64-unknown-linux-musl
# Output: target/x86_64-unknown-linux-musl/release/axon-ledger

# Build macOS (from macOS only)
cargo build -p axon-ledger --release
# Output: target/release/axon-ledger
```

Upload the binary to the GitHub release page. Then buyers can:
```bash
curl -sSfL https://github.com/<username>/axon/releases/latest/download/axon-ledger-linux-x86_64 \
  -o axon-ledger && chmod +x axon-ledger
```

## Version bump

Edit `crates/axon-ledger/Cargo.toml` → `version = "0.1.1"` etc.
(The ledger crate has its own version, decoupled from the axon workspace.)
