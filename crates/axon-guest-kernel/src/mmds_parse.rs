// ── Pure MMDS policy parsing (AUDIT T48) ─────────────────────────────────────
//
// Split out of `mmds.rs` and `include!`d back in, for ONE reason: the guest
// kernel is `test = false` (bare-metal `no_std` — it cannot link the std test
// harness), so this parser — the enforcement point of the guest's capability
// boundary — had NEVER had a single test. That is how OSK-P7-C3 shipped: six
// separate paths returned `EffectSet(0xFF)`, granting every effect, and nothing
// executed any of them.
//
// `crates/axon-vm/tests/guest_policy_parse.rs` `include!`s THIS FILE, so the
// tested code is the same text the kernel compiles — not a copy that can drift.
// Keep this file free of statics, `unsafe`, and kernel macros so it stays
// includable from a host test.

// Used by the KERNEL (`mmds.rs` locates `axon.policy=` on the boot cmdline with
// it); the policy parsers below no longer use it, having moved to a structural
// top-level lookup. The host test that `include!`s this file therefore sees it
// as dead, so the allowance is scoped to that build rather than deleting code
// the kernel still calls.
#[cfg_attr(test, allow(dead_code))]
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() { return Some(0); }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn skip_ws(s: &[u8]) -> &[u8] {
    &s[s.iter().position(|&b| !matches!(b, b' '|b'\t'|b'\n'|b'\r')).unwrap_or(s.len())..]
}

/// Decode standard base64 (RFC 4648) in-place. Returns decoded byte count.
fn base64_decode(input: &[u8], out: &mut [u8]) -> usize {
    let val = |c: u8| -> u8 {
        match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,  b'/' => 63,
            _    => 0,  // '=' padding and unknowns → 0
        }
    };
    let mut oi = 0usize;
    let mut i  = 0usize;
    while i + 3 < input.len() {
        let (a, b, c, d) = (val(input[i]), val(input[i+1]), val(input[i+2]), val(input[i+3]));
        if oi < out.len() { out[oi] = (a << 2) | (b >> 4); oi += 1; }
        if input[i+2] != b'=' && oi < out.len() { out[oi] = (b << 4) | (c >> 2); oi += 1; }
        if input[i+3] != b'=' && oi < out.len() { out[oi] = (c << 6) | d;        oi += 1; }
        i += 4;
    }
    oi
}

/// Extract a TOP-LEVEL `"key":"VALUE"` → VALUE bytes.
///
/// Located with `top_level_value` rather than a substring search, for the same
/// reason as the effect grant: a substring match would read the first
/// `"principal":` anywhere in the payload, including one nested inside an
/// unrelated object. A value containing an escape is refused rather than
/// returned half-decoded, since this parser does not unescape.
fn json_str_field<'a>(json: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let rest = top_level_value(json, key)?;
    if rest.is_empty() || rest[0] != b'"' { return None; }
    let inner = &rest[1..];
    let end = inner.iter().position(|&b| b == b'"' || b == b'\\')?;
    if inner[end] != b'"' { return None; }
    Some(&inner[..end])
}

/// Extract a TOP-LEVEL `"key":NUMBER` → u64.
///
/// `budget_tokens` is read through this, and it is a LIMIT: a nested or
/// duplicated `"budget_tokens":` resolved by first textual match would let the
/// wrong number set the cap. Located structurally, and ambiguous (duplicated)
/// keys yield None.
fn json_u64_field(json: &[u8], key: &[u8]) -> Option<u64> {
    let rest = top_level_value(json, key)?;
    if rest.is_empty() || !rest[0].is_ascii_digit() { return None; }
    let mut n: u64 = 0;
    for &b in rest {
        if b.is_ascii_digit() { n = n.saturating_mul(10).saturating_add((b - b'0') as u64); }
        else { break; }
    }
    Some(n)
}

/// Locate the value of `key` among the TOP-LEVEL members of a JSON object.
///
/// Returns the slice starting at the value, or `None` if the key is absent or
/// the payload is not a single well-formed object — and ALSO `None` when the
/// key occurs more than once, since a duplicated capability field is ambiguous
/// and an ambiguous grant must grant nothing.
///
/// This replaces a substring search for `"key":`, which matched the first
/// occurrence ANYWHERE in the text. Measured against that search:
///
///   {"meta":{"allowed_effects":["Exec","FS","Net"]},"allowed_effects":[]}
///                                                  -> granted FS+Net+Exec
///   {"allowed_effects":["Exec","FS"],"allowed_effects":[]} -> FS+Exec
///
/// Neither is reachable from today's producer (axon-vm's flat, serde-escaped
/// `MmdsPayload`). That is not a reason to keep it: the first time the payload
/// gains a nested object, the grant would be whatever that object said.
///
/// Walks bytes tracking nesting depth and whether it is inside a string
/// (honouring `\` escapes), so a key inside a nested object or a string value
/// is never mistaken for the top-level member. No allocation — this runs in
/// the bare-metal guest kernel.
fn top_level_value<'a>(json: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let s = skip_ws(json);
    if s.first() != Some(&b'{') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    let mut str_start = 0usize;
    let mut found: Option<usize> = None;
    let mut i = 0usize;
    while i < s.len() {
        let b = s[i];
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
                // A string that closes at depth 1 and is followed by `:` is a
                // top-level member NAME. Compare it to the key byte-for-byte;
                // key names here never contain escapes.
                if depth == 1 {
                    let name = &s[str_start..i];
                    let after = skip_ws(&s[i + 1..]);
                    if after.first() == Some(&b':') && name == key {
                        if found.is_some() {
                            return None; // duplicate key: ambiguous
                        }
                        let colon = s.len() - after.len();
                        found = Some(colon + 1);
                    }
                }
            }
        } else {
            match b {
                b'"' => {
                    in_str = true;
                    str_start = i + 1;
                }
                b'{' | b'[' => depth += 1,
                b'}' | b']' => {
                    if depth == 0 {
                        return None;
                    }
                    depth -= 1;
                    if depth == 0 {
                        // End of the top-level object. Anything after it other
                        // than whitespace means this was not one object.
                        if !skip_ws(&s[i + 1..]).is_empty() {
                            return None;
                        }
                        return found.map(|at| skip_ws(&s[at..]));
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    // Ran off the end: unterminated object or string. Not a policy.
    None
}

/// Parse `"key":["V1","V2"]` → EffectSet.
///
/// AUDIT T48 (OSK-P7-C3). Both early returns were `EffectSet(0xFF)` — ALL EIGHT
/// effects — when the key was absent or its value was not an array. The absent
/// case is not exotic: `axon-vm` serialises `allowed_effects: null` for any
/// program with no `.axmeta` manifest and no `--principal`, which is the DEFAULT
/// run. So the shipped default path granted IO+FS+Net+AI+Exec+Random.
/// A policy we cannot read is a policy we do not have; both now deny.
///
/// The key is located with `top_level_value`, not a substring search, and the
/// array must be properly terminated: an unterminated `["IO","Exec"` used to
/// run to the end of the buffer and grant both.
fn json_array_effects(json: &[u8], key: &[u8]) -> EffectSet {
    let rest = match top_level_value(json, key) {
        Some(v) => v,
        None => return EffectSet(0),
    };
    if rest.is_empty() || rest[0] != b'[' {
        return EffectSet(0);
    }
    let inner = &rest[1..];
    // Find the closing `]` OUTSIDE any string element. A `]` inside a quoted
    // name is text, not the end of the array.
    let mut in_str = false;
    let mut esc = false;
    let mut end: Option<usize> = None;
    for (k, &b) in inner.iter().enumerate() {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
        } else if b == b'"' {
            in_str = true;
        } else if b == b']' {
            end = Some(k);
            break;
        }
    }
    let inner = match end {
        Some(e) => &inner[..e],
        None => return EffectSet(0), // unterminated array: not a grant
    };

    let mut effects = EffectSet(0);
    let mut i = 0usize;
    while i < inner.len() {
        if inner[i] == b'"' {
            i += 1;
            let start = i;
            let mut esc = false;
            while i < inner.len() {
                if esc {
                    esc = false;
                } else if inner[i] == b'\\' {
                    esc = true;
                } else if inner[i] == b'"' {
                    break;
                }
                i += 1;
            }
            // An element containing an escape can never equal a valid effect
            // name, so `effect_from_name` maps it to nothing.
            effects = effects.union(effect_from_name(&inner[start..i]));
        }
        i += 1;
    }
    effects
}

fn effect_from_name(s: &[u8]) -> EffectSet {
    match s {
        b"IO"                      => EffectSet::IO,
        b"FS"                      => EffectSet::FS,
        b"Net" | b"NET"            => EffectSet::NET,
        b"AI"                      => EffectSet::AI,
        b"Exec" | b"EXEC"          => EffectSet::EXEC,
        b"Random" | b"RANDOM"      => EffectSet::RANDOM,
        _                          => EffectSet(0),
    }
}
