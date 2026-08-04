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

/// Build `"key":` search pattern in a 64-byte stack buffer; return length.
fn make_key_pat(key: &[u8], buf: &mut [u8; 64]) -> usize {
    let mut n = 0usize;
    buf[n] = b'"'; n += 1;
    for &b in key { if n < 62 { buf[n] = b; n += 1; } }
    buf[n] = b'"'; n += 1;
    buf[n] = b':'; n += 1;
    n
}

/// Extract `"key":"VALUE"` → VALUE bytes (no escape handling needed).
fn json_str_field<'a>(json: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut pat = [0u8; 64];
    let plen = make_key_pat(key, &mut pat);
    let rest = skip_ws(&json[find_subslice(json, &pat[..plen])? + plen..]);
    if rest.is_empty() || rest[0] != b'"' { return None; }
    let inner = &rest[1..];
    Some(&inner[..inner.iter().position(|&b| b == b'"')?])
}

/// Extract `"key":NUMBER` → u64.
fn json_u64_field(json: &[u8], key: &[u8]) -> Option<u64> {
    let mut pat = [0u8; 64];
    let plen = make_key_pat(key, &mut pat);
    let p = find_subslice(json, &pat[..plen])?;
    let rest = skip_ws(&json[p + plen..]);
    if rest.is_empty() || !rest[0].is_ascii_digit() { return None; }
    let mut n: u64 = 0;
    for &b in rest {
        if b.is_ascii_digit() { n = n.saturating_mul(10).saturating_add((b - b'0') as u64); }
        else { break; }
    }
    Some(n)
}

/// Parse `"key":["V1","V2"]` → EffectSet.
///
/// AUDIT T48 (OSK-P7-C3). Both early returns were `EffectSet(0xFF)` — ALL EIGHT
/// effects — when the key was absent or its value was not an array. The absent
/// case is not exotic: `axon-vm` serialises `allowed_effects: null` for any
/// program with no `.axmeta` manifest and no `--principal`, which is the DEFAULT
/// run. So the shipped default path granted IO+FS+Net+AI+Exec+Random.
/// A policy we cannot read is a policy we do not have; both now deny.
fn json_array_effects(json: &[u8], key: &[u8]) -> EffectSet {
    let mut pat = [0u8; 64];
    let plen = make_key_pat(key, &mut pat);
    let after = match find_subslice(json, &pat[..plen]) {
        Some(p) => p + plen,
        None    => return EffectSet(0),
    };
    let rest = skip_ws(&json[after..]);
    if rest.is_empty() || rest[0] != b'[' { return EffectSet(0); }
    let inner = &rest[1..];
    let end   = inner.iter().position(|&b| b == b']').unwrap_or(inner.len());
    let inner = &inner[..end];

    let mut effects = EffectSet(0);
    let mut i = 0usize;
    while i < inner.len() {
        if inner[i] == b'"' {
            i += 1;
            let start = i;
            while i < inner.len() && inner[i] != b'"' { i += 1; }
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
