//! K3: Syscall enforcement gate.
//!
//! Installs a syscall handler via SYSCALL/SYSRET MSRs (STAR, LSTAR, FMASK).
//! Each syscall is checked against the EffectSet from the MMDS policy before
//! dispatch.  Violations are logged, written to an audit ring buffer, and
//! exit the guest with code 8 (SandboxViolation).

use crate::mmds::{EffectSet, Policy};

// ── MSR indices ───────────────────────────────────────────────────────────────

const IA32_EFER:  u32 = 0xC000_0080;
const IA32_STAR:  u32 = 0xC000_0081;
const IA32_LSTAR: u32 = 0xC000_0082;
const IA32_FMASK: u32 = 0xC000_0084;

// ── Return-value sentinels ────────────────────────────────────────────────────

/// Returned by `syscall_dispatch` to signal a policy violation requiring
/// the guest to be hard-exited (exit code 8).
const VIOLATION: u64 = 0xDEAD_BEEF_DEAD_BEEF;

/// Returned for unrecognised syscalls — Linux ENOSYS (-2 as u64).
const ENOSYS: u64 = 0xFFFF_FFFF_FFFF_FFFE;

// ── Policy state (written once at init, read in the naked handler path) ───────

/// Bitmask of allowed effects copied from the MMDS `Policy` at init time.
/// Default 0xFF matches the open-policy mmds.rs stub so the kernel boots even
/// if K2 hasn't filled in a real policy yet.
static mut ALLOWED_EFFECTS: u64 = 0xFF;

// ── Audit ring buffer ─────────────────────────────────────────────────────────

static mut AUDIT_BUF:  [u8; 4096] = [0u8; 4096];
static mut AUDIT_HEAD: usize      = 0;

fn audit_write_bytes(s: &[u8]) {
    unsafe {
        for &b in s {
            AUDIT_BUF[AUDIT_HEAD % 4096] = b;
            AUDIT_HEAD = AUDIT_HEAD.wrapping_add(1);
        }
    }
}

fn audit_write_u64(mut n: u64) {
    let mut buf = [0u8; 20];
    let mut pos = buf.len();
    if n == 0 {
        audit_write_bytes(b"0");
        return;
    }
    while n > 0 {
        pos -= 1;
        buf[pos] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    audit_write_bytes(&buf[pos..]);
}

// ── I/O port helpers ──────────────────────────────────────────────────────────

#[inline]
unsafe fn outb(port: u16, val: u8) {
    core::arch::asm!(
        "out dx, al",
        in("dx") port,
        in("al") val,
        options(nomem, nostack),
    );
}

#[inline]
unsafe fn outw(port: u16, val: u16) {
    core::arch::asm!(
        "out dx, ax",
        in("dx") port,
        in("ax") val,
        options(nomem, nostack),
    );
}

// ── MSR helpers ───────────────────────────────────────────────────────────────

#[inline]
unsafe fn rdmsr(ecx: u32) -> u64 {
    let lo: u32;
    let hi: u32;
    core::arch::asm!(
        "rdmsr",
        out("eax") lo,
        out("edx") hi,
        in("ecx") ecx,
        options(nomem, nostack),
    );
    (hi as u64) << 32 | lo as u64
}

#[inline]
unsafe fn wrmsr(ecx: u32, val: u64) {
    let lo = val as u32;
    let hi = (val >> 32) as u32;
    core::arch::asm!(
        "wrmsr",
        in("ecx") ecx,
        in("eax") lo,
        in("edx") hi,
        options(nomem, nostack),
    );
}

// ── Effect-row → syscall mapping ──────────────────────────────────────────────

/// Returns the `EffectSet` bits required by syscall `nr`, or:
///   * `0`        – pure (always allowed, no effect required)
///   * `u64::MAX` – unknown syscall (soft-deny with ENOSYS, not a violation)
#[inline]
fn required_effect(nr: u64) -> u64 {
    match nr {
        // ── Pure: always allowed regardless of policy ────────────────────────
        0    // read       (pre-opened fds including stdin)
        | 1    // write      (pre-opened fds including stdout/stderr)
        | 9    // mmap
        | 10   // mprotect
        | 11   // munmap
        | 12   // brk
        | 14   // rt_sigprocmask
        | 15   // rt_sigreturn
        | 60   // exit
        | 158  // arch_prctl
        | 202  // futex
        | 231  // exit_group
        => 0,

        // ── FS ───────────────────────────────────────────────────────────────
        2    // open
        | 3    // close
        | 4    // stat
        | 5    // fstat
        | 6    // lstat
        | 8    // lseek
        | 79   // getcwd
        | 83   // mkdir
        | 87   // unlink
        | 217  // getdents64
        | 257  // openat
        | 258  // mkdirat
        | 263  // unlinkat
        => EffectSet::FS.0,

        // ── Net ──────────────────────────────────────────────────────────────
        7    // poll
        | 23   // select
        | 41   // socket
        | 42   // connect
        | 43   // accept
        | 44   // sendto
        | 45   // recvfrom
        | 48   // shutdown
        | 49   // bind
        | 50   // listen
        | 54   // setsockopt
        | 55   // getsockopt
        | 288  // accept4
        => EffectSet::NET.0,

        // ── Exec ─────────────────────────────────────────────────────────────
        56   // clone
        | 57   // fork
        | 59   // execve
        | 61   // wait4
        | 62   // kill
        => EffectSet::EXEC.0,

        // ── Random ───────────────────────────────────────────────────────────
        318  // getrandom
        => EffectSet::RANDOM.0,

        // ── Unknown — soft deny, not a violation ─────────────────────────────
        _ => u64::MAX,
    }
}

// ── Syscall dispatch (C-ABI, called from the naked handler) ───────────────────

/// Check whether syscall `nr` is permitted by the current policy.
///
/// Returns:
///   * `0`         – allowed (pure syscall or required effect granted)
///   * `ENOSYS`    – unknown syscall (soft deny, no audit entry)
///   * `VIOLATION` – effect required but not in policy (caller must exit guest)
#[no_mangle]
extern "C" fn syscall_dispatch(nr: u64) -> u64 {
    let req = required_effect(nr);

    if req == u64::MAX {
        // Unknown syscall — quiet soft-deny.
        return ENOSYS;
    }

    if req == 0 {
        // Pure syscall — unconditionally allowed.
        return 0;
    }

    // Effect-gated — compare against the policy installed at init.
    let allowed = unsafe { ALLOWED_EFFECTS };
    if allowed & req == req {
        return 0; // effect granted
    }

    // ── Policy violation ──────────────────────────────────────────────────────
    let ename = effect_name(req);
    kprintln!(
        "[axon-kernel] VIOLATION: syscall {} blocked ({} not in policy)",
        nr,
        ename
    );

    // Append a compact record to the audit ring buffer.
    audit_write_bytes(b"VIOLATION syscall=");
    audit_write_u64(nr);
    audit_write_bytes(b" effect=");
    audit_write_bytes(ename.as_bytes());
    audit_write_bytes(b"\n");

    VIOLATION
}

fn effect_name(bits: u64) -> &'static str {
    if bits == EffectSet::IO.0     { return "IO"; }
    if bits == EffectSet::FS.0     { return "FS"; }
    if bits == EffectSet::NET.0    { return "Net"; }
    if bits == EffectSet::AI.0     { return "AI"; }
    if bits == EffectSet::EXEC.0   { return "Exec"; }
    if bits == EffectSet::RANDOM.0 { return "Random"; }
    "unknown"
}

// ── Violation exit ────────────────────────────────────────────────────────────

/// Hard-exit the guest after a policy violation.
///
/// Writes a sentinel string to COM1 so `axon-vm` can detect the exit code,
/// then triggers ACPI S5 power-off at I/O port 0x604 (the PM1a control
/// register as used by Firecracker and QEMU).  Falls back to the ISA
/// debug-exit device (port 0x501) and finally a `hlt` spin loop.
#[no_mangle]
extern "C" fn violation_exit() -> ! {
    kprintln!("[axon-kernel] HALTING: policy violation — exit code 8");
    // Sentinel line parsed by axon-vm from the serial stream.
    crate::serial::write_str("\x1b[K-VIOLATION8\n");
    unsafe {
        // ACPI S5 power-off: Firecracker/QEMU PM1a_CNT at I/O port 0x604.
        // SLP_EN (bit 13) | SLP_TYP S5 (bits[12:10] = 0b111 → 0x1C00) = 0x2000.
        outw(0x604, 0x2000);
        // Fallback: QEMU/Firecracker ISA debug-exit device at 0x501.
        outb(0x501, 0);
        // Final fallback: spin with HLT.
        loop {
            core::arch::asm!("hlt");
        }
    }
}

// ── Naked syscall entry (installed as the LSTAR target) ───────────────────────

/// CPU entry point for every `SYSCALL` instruction executed in the guest.
///
/// Hardware sets on entry:
/// ```text
///   rax        = syscall number
///   rcx        = saved user RIP   (restored by SYSRET)
///   r11        = saved RFLAGS     (restored by SYSRET)
///   rdi rsi rdx r10 r8 r9 = syscall arguments
/// ```
///
/// Strategy:
///   1. Push rcx/r11 (needed by SYSRET) and all callee-saved registers.
///   2. Stash RSP in rbx; force-align the stack to 16 bytes for the C call.
///   3. Call `syscall_dispatch(nr)` with the syscall number in rdi.
///   4. Restore RSP from rbx (preserved across the call by callee-save rules).
///   5a. If result == VIOLATION sentinel → call `violation_exit` (never returns).
///   5b. Otherwise → pop saved registers, put result in rax, SYSRETQ.
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    core::arch::naked_asm!(
        // ── 1. Save SYSRET-critical registers ────────────────────────────────
        "push rcx",         // user return RIP  (must be in rcx at SYSRET)
        "push r11",         // saved RFLAGS     (must be in r11  at SYSRET)

        // ── 2. Save callee-saved registers (System V AMD64 ABI) ──────────────
        "push rbx",
        "push rbp",
        "push r12",
        "push r13",
        "push r14",
        "push r15",
        // 8 pushes = 64 bytes below entry RSP.

        // ── 3. Save RSP; force 16-byte alignment required by the C ABI ───────
        // rbx is free now (original saved above); use it to hold entry RSP so
        // we can restore the exact frame after the call.
        "mov rbx, rsp",
        "and rsp, -16",

        // ── 4. Dispatch: syscall number (rax) → first argument (rdi) ─────────
        "mov rdi, rax",
        "call {dispatch}",
        // rax = 0 (allowed) | ENOSYS | VIOLATION sentinel

        // ── 5. Restore RSP (rbx is callee-saved, intact across the call) ──────
        "mov rsp, rbx",

        // ── 6. Check for the VIOLATION sentinel ──────────────────────────────
        "mov rbx, {violation_sentinel}",
        "cmp rax, rbx",
        "je 2f",

        // ── 7a. Normal return: restore callee-saved, then SYSRET ─────────────
        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop rbp",
        "pop rbx",          // original rbx restored from stack
        "pop r11",          // RFLAGS for SYSRET
        "pop rcx",          // user RIP for SYSRET
        "sysretq",

        // ── 7b. Violation path: call the hard-exit handler (no return) ────────
        "2:",
        "call {violation}",
        "ud2",              // unreachable; appease the assembler

        dispatch           = sym syscall_dispatch,
        violation          = sym violation_exit,
        violation_sentinel = const 0xDEAD_BEEF_DEAD_BEEFu64,
    );
}

// ── Public init ───────────────────────────────────────────────────────────────

/// Install the syscall gate from the given policy.
///
/// Steps:
/// 1. Copy `policy.allowed_effects` into a static for the (naked) handler.
/// 2. Set SCE (bit 0) in IA32_EFER to enable SYSCALL/SYSRET.
/// 3. Program IA32_STAR with kernel CS (0x18) and user CS base (0x20).
/// 4. Write the handler address to IA32_LSTAR.
/// 5. Set IA32_FMASK to clear IF (bit 9) and DF (bit 10) on syscall entry.
pub fn init(policy: &Policy) {
    // Publish the allowed effects before enabling the gate.
    unsafe {
        ALLOWED_EFFECTS = policy.allowed_effects.0;
    }

    unsafe {
        // ── Enable SYSCALL/SYSRET: set SCE (bit 0) in EFER ───────────────────
        let efer = rdmsr(IA32_EFER);
        wrmsr(IA32_EFER, efer | 1);

        // ── STAR: CS selectors ────────────────────────────────────────────────
        // STAR[47:32] = kernel CS (0x18, GDT index 3×8)
        //   SYSCALL loads: CS = STAR[47:32], SS = STAR[47:32]+8
        // STAR[63:48] = user CS base (0x18+8 = 0x20; no real user mode here)
        //   64-bit SYSRET loads: CS = STAR[63:48]+16, SS = STAR[63:48]+8
        const KERNEL_CS: u64 = 0x18;
        const USER_CS:   u64 = 0x20; // 0x18 + 8
        wrmsr(IA32_STAR, (USER_CS << 48) | (KERNEL_CS << 32));

        // ── LSTAR: handler virtual address ────────────────────────────────────
        wrmsr(IA32_LSTAR, syscall_entry as u64);

        // ── FMASK: bits to clear in RFLAGS on syscall entry ───────────────────
        // Clear IF (bit 9) — disable hardware interrupts during kernel entry.
        // Clear DF (bit 10) — ensure string ops go forward (ABI requirement).
        wrmsr(IA32_FMASK, (1u64 << 9) | (1u64 << 10));
    }

    kprintln!(
        "[axon-kernel] enforce: gate active — {} effect bit(s) allowed ({:#x})",
        policy.allowed_effects.0.count_ones(),
        policy.allowed_effects.0,
    );
}
