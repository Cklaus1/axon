//! The guest kernel's two `ALLOWED_EFFECTS` statics must both default to DENY-ALL.
//!
//! `axon-guest-kernel` is `test = false` (bare-metal `no_std` — it cannot link the
//! std test harness), so nothing in the workspace executes its code. That is how
//! OSK-P7-C3 shipped six fail-OPEN policy paths with zero coverage, and it is why
//! `crates/axon-vm/tests/guest_policy_parse.rs` exists at all.
//!
//! ONE concept — "what is this confined guest allowed to do?" — is stored in TWO
//! independent statics, in two modules, with two different types:
//!
//!   * `mmds.rs`    `static mut ALLOWED_EFFECTS: EffectSet = EffectSet(0)`
//!   * `enforce.rs` `static mut ALLOWED_EFFECTS: u64 = 0`
//!
//! AUDIT T48 closed the mmds half and left the enforce half at `0xFF` — every
//! effect — so for the intervening period the two halves of one concept disagreed
//! about which way to fail. Nothing detected that, because nothing links either.
//!
//! There is no shared constant and no type in common, so they CAN drift apart
//! again. This test is the only thing that stops it: it reads the kernel's own
//! source (the same text the kernel compiles, not a copy) and asserts each static
//! is initialised to a grant of nothing.
//!
//! Why source text rather than behaviour: the value under test is a *static
//! initialiser*, and on every real path `enforce::init` overwrites it before the
//! syscall gate is armed. A behavioural test would therefore observe the policy,
//! never the default. The default only matters if that coupling breaks — which is
//! exactly the case no runtime test can set up.

use std::path::PathBuf;

fn kernel_src(file: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../axon-guest-kernel/src")
        .join(file);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read guest-kernel source {}: {e}", path.display()))
}

/// Return the initialiser text of the single `static mut ALLOWED_EFFECTS` in `src`.
///
/// Fails loudly on zero matches or more than one: a test that silently matches
/// nothing reports "fine" on code it never looked at.
fn allowed_effects_initialiser(file: &str) -> String {
    let src = kernel_src(file);
    let decls: Vec<&str> = src
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("static mut ALLOWED_EFFECTS"))
        .collect();

    assert_eq!(
        decls.len(),
        1,
        "expected exactly one `static mut ALLOWED_EFFECTS` declaration in {file}, \
         found {}: {decls:?} — this test's anchor has moved and it is no longer \
         checking what it claims to",
        decls.len()
    );

    let decl = decls[0];
    let (_, rhs) = decl
        .split_once('=')
        .unwrap_or_else(|| panic!("no `=` initialiser in {file}: {decl}"));
    rhs.trim().trim_end_matches(';').trim().to_string()
}

/// Accepts the deny-all spellings each module actually uses: a bare `0` (u64) or
/// a newtype wrapping zero, e.g. `EffectSet(0)`. Rejects anything else, including
/// `0xFF`, so a new "convenient" default has to argue with this test.
fn grants_nothing(init: &str) -> bool {
    let inner = match init.split_once('(') {
        Some((_, rest)) => rest.trim_end_matches(')').trim(),
        None => init,
    };
    matches!(inner, "0" | "0x0" | "0u64" | "0b0")
}

#[test]
fn enforce_allowed_effects_defaults_to_deny_all() {
    // The syscall gate's copy. This was `0xFF` — the `syscall_dispatch` fast path
    // reads it with no readiness flag to consult, so if it is ever read before
    // `init` writes the real policy, 0xFF grants IO+FS+Net+AI+Exec+Random to an
    // unconfigured guest.
    let init = allowed_effects_initialiser("enforce.rs");
    assert!(
        grants_nothing(&init),
        "enforce.rs `ALLOWED_EFFECTS` defaults to `{init}`, which is not deny-all. \
         The static initialiser of a security gate must grant nothing; the policy \
         is installed by `enforce::init`, not by hoping the default is right."
    );
}

#[test]
fn mmds_allowed_effects_defaults_to_deny_all() {
    // The policy-parser's copy (AUDIT T48 / OSK-P7-C3). Guarded here so the pair
    // is checked by one test file and neither half can regress alone.
    let init = allowed_effects_initialiser("mmds.rs");
    assert!(
        grants_nothing(&init),
        "mmds.rs `ALLOWED_EFFECTS` defaults to `{init}`, which is not deny-all — \
         this is the exact regression AUDIT T48 fixed."
    );
}

#[test]
fn the_two_allowed_effects_statics_fail_the_same_way() {
    // The drift guard proper. Two statics for one concept, no shared constant and
    // no common type, in a crate nothing links. Asserting each is deny-all catches
    // a half-fix — the state the kernel was actually in between T48 and now.
    let enforce = allowed_effects_initialiser("enforce.rs");
    let mmds = allowed_effects_initialiser("mmds.rs");
    assert!(
        grants_nothing(&enforce) && grants_nothing(&mmds),
        "the guest kernel's two ALLOWED_EFFECTS statics disagree about which way \
         to fail: enforce.rs = `{enforce}`, mmds.rs = `{mmds}`. One concept, two \
         storage sites — they must both deny."
    );
}

#[test]
fn enforce_init_publishes_the_policy_before_arming_the_gate() {
    // Deny-all is only free because `init` overwrites it BEFORE setting SCE in
    // IA32_EFER. If that order ever inverts, there is a window in which the gate
    // is live and the policy is not — with deny-all that window refuses
    // everything (loud), and with the old 0xFF it permitted everything (silent).
    // Either way the order is load-bearing, so pin it.
    let src = kernel_src("enforce.rs");
    let publish = src
        .find("ALLOWED_EFFECTS = policy.allowed_effects.0")
        .expect("enforce::init must publish the policy into ALLOWED_EFFECTS");
    let arm = src
        .find("wrmsr(IA32_EFER, efer | 1)")
        .expect("enforce::init must set SCE in IA32_EFER to arm the gate");
    assert!(
        publish < arm,
        "enforce::init arms the syscall gate (SCE in IA32_EFER) at byte {arm} \
         before publishing the policy at byte {publish} — the gate would run \
         against the static default"
    );
}
