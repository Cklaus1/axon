//! Host-side tests for the guest kernel's MMDS policy parser (AUDIT T48).
//!
//! `axon-guest-kernel` is `test = false` — bare-metal `no_std`, it cannot link
//! the std test harness — so the parser that decides what a confined guest is
//! allowed to do had **never had a single test**. That is how OSK-P7-C3 shipped:
//! six separate paths returned `EffectSet(0xFF)`, granting every effect, and
//! nothing anywhere executed them.
//!
//! This file `include!`s the kernel's own `mmds_parse.rs`, so the code under
//! test is the same text the kernel compiles. A copy would drift, and a drifted
//! copy of a security parser is worse than none.
//!
//! It lives in `axon-vm` because axon-vm is the PRODUCER of this payload, so
//! both halves of the contract are checked in one place.

/// Mirror of the kernel's `EffectSet` — the only type the included file needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectSet(pub u64);

impl EffectSet {
    pub const IO: EffectSet = EffectSet(1 << 0);
    pub const FS: EffectSet = EffectSet(1 << 1);
    pub const NET: EffectSet = EffectSet(1 << 2);
    pub const AI: EffectSet = EffectSet(1 << 3);
    pub const EXEC: EffectSet = EffectSet(1 << 4);
    pub const RANDOM: EffectSet = EffectSet(1 << 5);
    pub fn union(self, other: EffectSet) -> EffectSet {
        EffectSet(self.0 | other.0)
    }
    pub fn contains(self, other: EffectSet) -> bool {
        self.0 & other.0 == other.0
    }
}

include!("../../axon-guest-kernel/src/mmds_parse.rs");

/// The exact JSON shape `axon-vm` serialises (see `MmdsPayload`).
fn payload(effects_field: &str) -> Vec<u8> {
    format!(
        r#"{{"schema":"axon-vm-mmds/1","run_id":"r","principal":"p",{effects_field},"budget_tokens":100}}"#
    )
    .into_bytes()
}

#[test]
fn absent_allowed_effects_denies_everything_osk_p7_c3() {
    // THE SHIPPED DEFAULT PATH. `axon-vm` emits `allowed_effects: null` for any
    // program with no `.axmeta` manifest and no `--principal`, and the parser
    // returned EffectSet(0xFF) — IO+FS+Net+AI+Exec+Random — when the key was
    // absent or its value was not an array. So the default run granted
    // everything, in the enforcement point of a security boundary.
    let no_key = payload(r#""other":1"#);
    assert_eq!(
        json_array_effects(&no_key, b"allowed_effects"),
        EffectSet(0),
        "an ABSENT allowed_effects must grant nothing"
    );

    let null_value = payload(r#""allowed_effects":null"#);
    assert_eq!(
        json_array_effects(&null_value, b"allowed_effects"),
        EffectSet(0),
        "`allowed_effects: null` — what axon-vm emits by default — must grant nothing"
    );

    for malformed in [
        r#""allowed_effects":"IO""#,    // a string, not an array
        r#""allowed_effects":7"#,       // a number
        r#""allowed_effects":{"a":1}"#, // an object
        r#""allowed_effects":true"#,
    ] {
        let p = payload(malformed);
        assert_eq!(
            json_array_effects(&p, b"allowed_effects"),
            EffectSet(0),
            "a non-array allowed_effects must grant nothing: {malformed}"
        );
    }
}

#[test]
fn a_well_formed_grant_is_parsed_exactly_t48() {
    // NEGATIVE CONTROL. Denying on ambiguity is only correct if a real grant
    // still parses — otherwise the fix is "deny everything" and the test above
    // passes for the wrong reason.
    let p = payload(r#""allowed_effects":["IO","Net"]"#);
    let got = json_array_effects(&p, b"allowed_effects");
    assert!(got.contains(EffectSet::IO), "IO must be granted");
    assert!(got.contains(EffectSet::NET), "Net must be granted");
    assert!(!got.contains(EffectSet::EXEC), "Exec was NOT granted");
    assert!(!got.contains(EffectSet::FS), "FS was NOT granted");
    assert_eq!(
        got,
        EffectSet::IO.union(EffectSet::NET),
        "exactly the requested effects, nothing more"
    );

    // An explicitly EMPTY grant is a real, meaningful policy: no effects.
    let empty = payload(r#""allowed_effects":[]"#);
    assert_eq!(json_array_effects(&empty, b"allowed_effects"), EffectSet(0));

    // An unknown effect name contributes nothing rather than widening.
    let unknown = payload(r#""allowed_effects":["IO","Telepathy"]"#);
    assert_eq!(
        json_array_effects(&unknown, b"allowed_effects"),
        EffectSet::IO,
        "an unrecognised effect name must not widen the grant"
    );
}

#[test]
fn scalar_fields_round_trip_and_missing_ones_are_none_t48() {
    let p = payload(r#""allowed_effects":["IO"]"#);
    assert_eq!(json_u64_field(&p, b"budget_tokens"), Some(100));
    assert_eq!(json_str_field(&p, b"principal"), Some(&b"p"[..]));
    assert_eq!(
        json_u64_field(&p, b"no_such_field"),
        None,
        "a missing numeric field must be None, not a default"
    );
    assert_eq!(json_str_field(&p, b"no_such_field"), None);
}

#[test]
fn base64_decode_round_trips_the_real_payload_t48() {
    // The policy arrives base64-encoded on the kernel cmdline; a decode that
    // silently produced nothing used to leave the static default in place,
    // which was 0xFF.
    let json = payload(r#""allowed_effects":["IO","Net"]"#);
    let b64 = {
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = Vec::new();
        for c in json.chunks(3) {
            let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
            let n = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
            out.push(T[(n >> 18) as usize & 63]);
            out.push(T[(n >> 12) as usize & 63]);
            out.push(if c.len() > 1 {
                T[(n >> 6) as usize & 63]
            } else {
                b'='
            });
            out.push(if c.len() > 2 {
                T[n as usize & 63]
            } else {
                b'='
            });
        }
        out
    };
    let mut buf = vec![0u8; json.len() + 8];
    let n = base64_decode(&b64, &mut buf);
    assert_eq!(&buf[..n], &json[..], "base64 round-trip must be exact");
    assert_eq!(
        json_array_effects(&buf[..n], b"allowed_effects"),
        EffectSet::IO.union(EffectSet::NET)
    );
}

/// Every SEMANTICALLY EMPTY policy must deny everything.
///
/// A policy that is syntactically valid JSON but says nothing about effects is
/// the case most likely to be mistaken for "no restriction": `{}` parses, has
/// no error to report, and a permissive default would slide straight through.
/// This is the analogue of the guest-init/MMDS parser boundary already fixed.
#[test]
fn every_semantically_empty_policy_denies_everything() {
    for (label, json) in [
        ("empty object", r#"{}"#),
        ("absent key", r#"{"schema":"axon-vm-mmds/1","run_id":"r"}"#),
        (
            "all-null object",
            r#"{"principal":null,"allowed_effects":null,"budget_tokens":null}"#,
        ),
        (
            "unknown fields only",
            r#"{"foo":1,"bar":["IO","FS","Net"]}"#,
        ),
        ("explicitly empty grant", r#"{"allowed_effects":[]}"#),
    ] {
        assert_eq!(
            json_array_effects(json.as_bytes(), b"allowed_effects"),
            EffectSet(0),
            "{label} must grant nothing: {json}"
        );
    }
    // POSITIVE CONTROLS, so the denials above cannot pass because the parser
    // denies everything unconditionally.
    assert_eq!(
        json_array_effects(br#"{"allowed_effects":["IO"]}"#, b"allowed_effects"),
        EffectSet::IO
    );
    assert_eq!(
        json_array_effects(br#"{"allowed_effects":["Net"]}"#, b"allowed_effects"),
        EffectSet::NET
    );
}

/// The key must be matched at the TOP LEVEL, once, in a well-formed array.
///
/// The parser is a substring scanner rather than a JSON parser, and three
/// structures that are not what they appear to be were measured WIDENING the
/// grant:
///
///   nested object carrying the key, empty top level   -> FS+Net+Exec
///   duplicate key, broad value first                   -> FS+Exec
///   unterminated array                                 -> IO+Exec
///
/// None is reachable today: the only producer, axon-vm's `MmdsPayload`, is a
/// flat struct that serde emits with each key once and every string escaped.
/// That is exactly why it matters — the first time the payload gains a nested
/// object (for instance one echoing a program's own declared effects), the
/// first textual match wins and the grant is whatever that object says. A
/// capability parser should not depend on its producer never growing a field.
#[test]
fn only_a_single_well_formed_top_level_grant_counts() {
    assert_eq!(
        json_array_effects(
            br#"{"meta":{"allowed_effects":["Exec","FS","Net"]},"allowed_effects":[]}"#,
            b"allowed_effects"
        ),
        EffectSet(0),
        "a key inside a NESTED object must not be read as the grant"
    );
    assert_eq!(
        json_array_effects(
            br#"{"meta":{"allowed_effects":["Exec"]}}"#,
            b"allowed_effects"
        ),
        EffectSet(0),
        "a nested key with NO top-level key must not be read as the grant"
    );
    assert_eq!(
        json_array_effects(
            br#"{"allowed_effects":["Exec","FS"],"allowed_effects":[]}"#,
            b"allowed_effects"
        ),
        EffectSet(0),
        "a DUPLICATED key is ambiguous, and an ambiguous policy grants nothing"
    );
    assert_eq!(
        json_array_effects(br#"{"allowed_effects":["IO","Exec""#, b"allowed_effects"),
        EffectSet(0),
        "an UNTERMINATED array is not a grant"
    );
    assert_eq!(
        json_array_effects(
            br#"{"principal":"x\"allowed_effects\":[\"Exec\"]"}"#,
            b"allowed_effects"
        ),
        EffectSet(0),
        "the key text inside a STRING value is not the key"
    );
    // POSITIVE CONTROL: a single top-level grant still parses, even with a
    // nested object and escaped strings elsewhere in the payload.
    assert_eq!(
        json_array_effects(
            br#"{"meta":{"k":"v\"}"},"principal":"a\\b","allowed_effects":["IO","Net"]}"#,
            b"allowed_effects"
        ),
        EffectSet::IO.union(EffectSet::NET),
        "a real top-level grant must survive surrounding structure"
    );
}

/// The SCALAR fields are located structurally too.
///
/// `budget_tokens` is a cap and `principal` is attribution. Read by first
/// textual match, a nested or duplicated occurrence could set either.
#[test]
fn scalar_fields_are_read_only_from_the_top_level() {
    assert_eq!(
        json_u64_field(
            br#"{"meta":{"budget_tokens":999999},"budget_tokens":10}"#,
            b"budget_tokens"
        ),
        Some(10),
        "a nested budget_tokens must not set the cap"
    );
    assert_eq!(
        json_u64_field(
            br#"{"budget_tokens":999999,"budget_tokens":10}"#,
            b"budget_tokens"
        ),
        None,
        "a DUPLICATED budget_tokens is ambiguous and must not set the cap"
    );
    assert_eq!(
        json_str_field(
            br#"{"meta":{"principal":"root"},"principal":"alice"}"#,
            b"principal"
        ),
        Some(&b"alice"[..]),
        "a nested principal must not be read as the principal"
    );
    assert_eq!(
        json_str_field(br#"{"principal":"al\"ice"}"#, b"principal"),
        None,
        "an escaped value is refused rather than returned half-decoded"
    );
}
