//! K2: reads axon-vm-mmds/1 policy from the Linux kernel cmdline.
//!
//! axon-vm encodes the policy as `axon.policy=<base64-json>` in the cmdline.
//! We read `cmd_line_ptr` (u32, offset 0x228 in boot_params), copy the cmdline
//! into a static buffer, base64-decode the blob, and scan the JSON with a
//! zero-alloc hand-rolled parser.  No heap — all storage is `static mut`.

/// Parsed boot policy delivered via kernel cmdline.
pub struct Policy<'a> {
    pub principal:       Option<&'a str>,
    pub allowed_effects: EffectSet,
    pub budget_tokens:   Option<u64>,
    pub source_hash:     Option<&'a str>, // unused at kernel level
    pub run_id:          Option<&'a str>, // unused at kernel level
    pub seccomp_hint:    &'a [u8],        // unused at kernel level
}

/// Bitmask of allowed effect rows.
#[derive(Clone, Copy, Default)]
pub struct EffectSet(pub u64);

impl EffectSet {
    pub const IO:     EffectSet = EffectSet(1 << 0);
    pub const FS:     EffectSet = EffectSet(1 << 1);
    pub const NET:    EffectSet = EffectSet(1 << 2);
    pub const AI:     EffectSet = EffectSet(1 << 3);
    pub const EXEC:   EffectSet = EffectSet(1 << 4);
    pub const RANDOM: EffectSet = EffectSet(1 << 5);

    pub fn contains(self, other: EffectSet) -> bool { self.0 & other.0 == other.0 }
    pub fn union(self, other: EffectSet)    -> EffectSet { EffectSet(self.0 | other.0) }
}

// Static buffers — no heap allocation.
static mut CMDLINE_BUF:    [u8; 4096] = [0u8; 4096];
static mut CMDLINE_LEN:    usize      = 0;
static mut JSON_BUF:       [u8; 2048] = [0u8; 2048];
static mut JSON_LEN:       usize      = 0;
static mut PRINCIPAL_BUF:  [u8; 128]  = [0u8; 128];
static mut PRINCIPAL_LEN:  usize      = 0;
static mut ALLOWED_EFFECTS: EffectSet = EffectSet(0xFF); // open by default
static mut BUDGET_TOKENS:  u64        = 0;
static mut HAS_BUDGET:     bool       = false;
static mut POLICY_READY:   bool       = false;

// Byte offset of `cmd_line_ptr` (u32) in the Linux x86 boot_params struct.
const CMD_LINE_PTR_OFF: usize = 0x228;

/// Parse the boot policy from the kernel cmdline.  Call once after
/// `serial::init()`, before `read_policy()`.
pub fn init(boot_params_phys: u64) {
    // SAFETY: boot_params_phys is the identity-mapped physical address provided
    // by Firecracker via the Linux boot protocol; the field is 4-byte aligned.
    let cmdline_phys = unsafe {
        core::ptr::read_unaligned(
            (boot_params_phys as usize + CMD_LINE_PTR_OFF) as *const u32,
        ) as u64
    };

    if cmdline_phys == 0 {
        kprintln!("[axon-kernel] K2: no cmdline ptr — open policy");
        return set_open_policy();
    }

    // Copy null-terminated cmdline into CMDLINE_BUF.
    // SAFETY: cmdline_phys points to the ASCII cmdline Firecracker wrote into
    // guest memory before the kernel entry point.
    let cmdline_len = unsafe {
        let src = cmdline_phys as *const u8;
        let mut i = 0usize;
        while i < 4095 {
            let b = core::ptr::read_volatile(src.add(i));
            if b == 0 { break; }
            CMDLINE_BUF[i] = b;
            i += 1;
        }
        CMDLINE_LEN = i;
        i
    };

    // Locate "axon.policy=" and capture the value's byte range.
    const TAG: &[u8] = b"axon.policy=";
    let (val_start, val_end) = {
        // SAFETY: reading CMDLINE_BUF written immediately above.
        let cmdline = unsafe {
            core::slice::from_raw_parts(
                core::ptr::addr_of!(CMDLINE_BUF) as *const u8, cmdline_len,
            )
        };
        match find_subslice(cmdline, TAG) {
            Some(p) => {
                let vs = p + TAG.len();
                let ve = cmdline[vs..].iter()
                    .position(|&b| b == b' ')
                    .map(|q| vs + q)
                    .unwrap_or(cmdline_len);
                (vs, ve)
            }
            None => {
                kprintln!("[axon-kernel] K2: axon.policy= absent — open policy");
                return set_open_policy();
            }
        }
    };

    // Base64-decode the value from CMDLINE_BUF into JSON_BUF.
    // SAFETY: CMDLINE_BUF and JSON_BUF are distinct statics; single-threaded boot.
    let json_len = unsafe {
        let b64 = core::slice::from_raw_parts(
            (core::ptr::addr_of!(CMDLINE_BUF) as *const u8).add(val_start),
            val_end - val_start,
        );
        let n = base64_decode(b64, &mut JSON_BUF);
        JSON_LEN = n;
        n
    };

    kprintln!("[axon-kernel] K2: policy {} json bytes", json_len);

    // Parse JSON fields.
    // SAFETY: JSON_BUF holds the decoded policy; PRINCIPAL_BUF is distinct.
    unsafe {
        let json = core::slice::from_raw_parts(
            core::ptr::addr_of!(JSON_BUF) as *const u8, json_len,
        );

        if let Some(s) = json_str_field(json, b"principal") {
            let n = s.len().min(127);
            core::ptr::copy_nonoverlapping(
                s.as_ptr(),
                core::ptr::addr_of_mut!(PRINCIPAL_BUF) as *mut u8,
                n,
            );
            PRINCIPAL_LEN = n;
        }

        if let Some(budget) = json_u64_field(json, b"budget_tokens") {
            BUDGET_TOKENS = budget;
            HAS_BUDGET    = true;
        }

        ALLOWED_EFFECTS = json_array_effects(json, b"allowed_effects");
        POLICY_READY    = true;
    }
}

/// Return the parsed policy, borrowing from static storage.
/// `init()` must be called first.
pub fn read_policy() -> Policy<'static> {
    // SAFETY: statics written only by init(), which runs once before this.
    unsafe {
        if !POLICY_READY {
            return Policy {
                principal: None, allowed_effects: EffectSet(0xFF),
                budget_tokens: None, source_hash: None, run_id: None,
                seccomp_hint: &[],
            };
        }
        let principal = if PRINCIPAL_LEN > 0 {
            core::str::from_utf8(core::slice::from_raw_parts(
                core::ptr::addr_of!(PRINCIPAL_BUF) as *const u8, PRINCIPAL_LEN,
            )).ok()
        } else {
            None
        };
        Policy {
            principal,
            allowed_effects: ALLOWED_EFFECTS,
            budget_tokens:   if HAS_BUDGET { Some(BUDGET_TOKENS) } else { None },
            source_hash: None, run_id: None, seccomp_hint: &[],
        }
    }
}

// ── Internal helpers ───────────────────────────────────────────────────────────

fn set_open_policy() {
    // SAFETY: called only from init() on the single boot path.
    unsafe { ALLOWED_EFFECTS = EffectSet(0xFF); HAS_BUDGET = false; PRINCIPAL_LEN = 0; POLICY_READY = true; }
}

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
    let rest = skip_ws(&json[find_subslice(json, &pat[..plen])? + plen..]);
    if rest.is_empty() || !rest[0].is_ascii_digit() { return None; }
    let mut n: u64 = 0;
    for &b in rest {
        if b.is_ascii_digit() { n = n.saturating_mul(10).saturating_add((b - b'0') as u64); }
        else { break; }
    }
    Some(n)
}

/// Parse `"key":["V1","V2"]` → EffectSet.  Returns 0xFF (open) if field absent.
fn json_array_effects(json: &[u8], key: &[u8]) -> EffectSet {
    let mut pat = [0u8; 64];
    let plen = make_key_pat(key, &mut pat);
    let after = match find_subslice(json, &pat[..plen]) {
        Some(p) => p + plen,
        None    => return EffectSet(0xFF),
    };
    let rest = skip_ws(&json[after..]);
    if rest.is_empty() || rest[0] != b'[' { return EffectSet(0xFF); }
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
