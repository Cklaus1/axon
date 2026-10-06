//! What stood between this crate's sources and its bytes, as cargo shows it to
//! the build script (C9 round 5, FIELD-ORIGIN, amendment 80).
//!
//! The host-side production binaries -- the setuid `axon-protected-launcher`,
//! the verifier, the custodian, the observer -- are built by the operator's own
//! cargo; no controlled environment is constructed for them the way
//! `scripts/guest_build_env.py` constructs the guest's. What the build script
//! CAN see is the state cargo hands it: `RUSTC_WRAPPER` and
//! `RUSTC_WORKSPACE_WRAPPER` (a program run in place of rustc),
//! `CARGO_ENCODED_RUSTFLAGS` (every source of rustflags: `RUSTFLAGS`, config
//! `build.rustflags`, `target.*.rustflags`) and `RUSTC_LINKER` (a configured
//! linker). Any of them non-empty is recorded in the build's identity
//! (`verifier-manifest`'s `build_state`) and, in a PRODUCTION build (release
//! profile, no test-trust feature), REFUSES the build outright.
//!
//! Not seen here, so not judged here: an ancestor `.cargo/config.toml` that
//! sets none of the four, a replaced registry source, a reused `CARGO_HOME`,
//! and the cargo/rustc binaries themselves. The kit's `check-host-build`
//! (scripts/guest_build_env.py) judges the first two at deploy time; the
//! binaries are operator-trusted through the toolchain pin
//! (rust-toolchain.toml), and `--locked` holds every dependency to
//! Cargo.lock's checksums.
//!
//! This file is `#[path]`-included by `build.rs`, which is why it is std-only.

/// The variables cargo gives a build script that name something between the
/// sources and the bytes.
pub const WATCHED: [&str; 4] = [
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTC_LINKER",
];

/// `NAME=value;NAME=value` over the non-empty watched variables, in a fixed
/// order; empty when nothing stands in between.
pub fn state(get: &dyn Fn(&str) -> Option<String>) -> String {
    WATCHED
        .iter()
        .filter_map(|n| get(n).filter(|v| !v.is_empty()).map(|v| format!("{n}={v}")))
        .collect::<Vec<_>>()
        .join(";")
}

/// Why this build must not proceed (None: it may).
pub fn refusal(state: &str, profile: &str, test_trust_feature: bool) -> Option<String> {
    // A production build: the release profile without the test-trust feature
    // (the feature a development `cargo test` unifies in through dev-deps).
    let production = profile == "release" && !test_trust_feature;
    if state.is_empty() || !production {
        return None;
    }
    Some(format!(
        "refusing a production build of axon-fabric under a compiler wrapper, rustflags or \
         linker ({state}): the binaries it makes hold the root helper and every pin, and \
         something other than the pinned toolchain would stand between the sources and them. \
         Build with RUSTC_WRAPPER, RUSTC_WORKSPACE_WRAPPER and RUSTFLAGS unset and no \
         build.rustflags / target linker in any cargo config"
    ))
}
