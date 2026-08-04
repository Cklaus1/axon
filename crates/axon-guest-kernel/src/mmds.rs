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
// AUDIT T48 (finding OSK-P7-C3; R36 §2 site 3). This was `EffectSet(0xFF)` —
// "open by default" — in the enforcement point of a security boundary. Every
// ambiguity now denies. See `set_closed_policy`.
static mut ALLOWED_EFFECTS: EffectSet = EffectSet(0);
static mut BUDGET_TOKENS:  u64        = 0;
static mut HAS_BUDGET:     bool       = false;
static mut POLICY_READY:   bool       = false;

// Byte offset of `cmd_line_ptr` (u32) in the Linux x86 boot_params struct.
const CMD_LINE_PTR_OFF: usize = 0x228;

/// Parse the boot policy from the kernel cmdline.  Call once after
/// `serial::init()`, before `read_policy()`.
pub fn init(boot_params_phys: u64) {
    // If no boot_params (PVH / multiboot without Linux params), use open policy.
    if boot_params_phys == 0 {
        kprintln!("[axon-kernel] K2: no boot_params — DENY-ALL policy (T48)");
        return set_closed_policy();
    }

    // SAFETY: boot_params_phys is the identity-mapped physical address provided
    // by Firecracker via the Linux boot protocol; the field is 4-byte aligned.
    let cmdline_phys = unsafe {
        core::ptr::read_unaligned(
            (boot_params_phys as usize + CMD_LINE_PTR_OFF) as *const u32,
        ) as u64
    };

    if cmdline_phys == 0 {
        kprintln!("[axon-kernel] K2: no cmdline ptr — DENY-ALL policy (T48)");
        return set_closed_policy();
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
                kprintln!("[axon-kernel] K2: axon.policy= absent — DENY-ALL policy (T48)");
                return set_closed_policy();
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
        let json_slice: &mut [u8] = &mut *(&raw mut JSON_BUF);
        let n = base64_decode(b64, json_slice);
        JSON_LEN = n;
        n
    };

    kprintln!("[axon-kernel] K2: policy {} json bytes", json_len);

    // T48: a base64 value that decodes to nothing is an unreadable policy, not
    // an empty one. It used to fall through and leave ALLOWED_EFFECTS at the
    // static default, which was 0xFF. Refuse explicitly and say so, rather than
    // relying on the static happening to be right.
    if json_len == 0 {
        kprintln!("[axon-kernel] K2: policy failed to base64-decode — DENY-ALL policy (T48)");
        return set_closed_policy();
    }

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
            // T48 (OSK-P7-C3, R36 §2 site 4): was EffectSet(0xFF). `read_policy`
            // before `init` — or after an init path that bailed — must not be
            // the most permissive answer in the system.
            return Policy {
                principal: None, allowed_effects: EffectSet(0),
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

/// Install the DENY-EVERYTHING policy (AUDIT T48, finding OSK-P7-C3).
///
/// This used to be `set_open_policy`, granting `EffectSet(0xFF)` — every effect —
/// on three separate boot paths: no boot_params, no cmdline pointer, and an
/// absent `axon.policy=` tag. Combined with `json_array_effects` returning 0xFF
/// for an absent/unparseable field and `read_policy` returning 0xFF when
/// `POLICY_READY` was false, the sequence *sidecar missing → no principal → no
/// `axon.policy=`* booted a guest with every effect bit set and a green-looking
/// run. R36 §2 names these as the four fail-open policy-provenance sites and
/// calls closing them S0 work; §9 clause (f) gates each negatively.
///
/// The guest still BOOTS (so the failure is observable on the serial log and the
/// launcher gets its sentinel) but holds no authority: the syscall gate refuses
/// the program's first effectful syscall with the ordinary VIOLATION path, exit
/// 8. That is deliberately louder than halting at boot — a guest that never
/// starts is easily mistaken for infrastructure flakiness, while a policy
/// violation is a recorded, attributable refusal.
fn set_closed_policy() {
    // SAFETY: called only from init() on the single boot path.
    unsafe { ALLOWED_EFFECTS = EffectSet(0); HAS_BUDGET = false; PRINCIPAL_LEN = 0; POLICY_READY = true; }
}

// The pure parsing helpers live in `mmds_parse.rs` and are included here so the
// kernel compiles them exactly as before. They are a separate file only so a
// host-side test can `include!` the SAME TEXT — see that file's header (T48).
include!("mmds_parse.rs");
