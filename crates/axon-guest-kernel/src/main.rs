//! axon-guest-kernel — Axon ASI supervisor kernel for Firecracker microVMs.
//!
//! Boot sequence (see boot.s for stages 0→1):
//!   _start32  (boot.s)   Linux boot-protocol entry, long-mode setup
//!   _start64  (boot.s)   64-bit init, calls kernel_main
//!   kernel_main (here)   K1: serial, K2: MMDS policy, K3: syscall gate,
//!                        K4: hypercall substrate, K5: run Axon program

#![no_std]
#![no_main]
// asm_const and naked_functions are stable since Rust 1.82/1.88 — no feature gate needed.

#[macro_use]
mod serial;
mod bump;
mod enforce;
mod hypercall;
mod mmds;

use core::panic::PanicInfo;

// ── Kernel entry point ────────────────────────────────────────────────────────

/// Called from _start64 in boot.s with `boot_params_phys: u64` pointing at the
/// Linux boot_params structure that Firecracker built for us.
#[no_mangle]
pub extern "C" fn kernel_main(boot_params_phys: u64) -> ! {
    // K1: serial port for boot logging.
    serial::init();
    kprintln!("[axon-kernel] boot ok  params={:#x}", boot_params_phys);

    // K1: initialize the bump allocator (kernel heap: 14 MiB at 0x200000).
    bump::init();
    kprintln!("[axon-kernel] heap ok");

    // K2: read policy from kernel cmdline (`axon.policy=<base64-json>`).
    mmds::init(boot_params_phys);
    let policy = mmds::read_policy();
    kprintln!("[axon-kernel] policy ok  principal={}", policy.principal.unwrap_or("root"));

    // K3: install syscall gate (SYSCALL MSR).
    enforce::init(&policy);
    kprintln!("[axon-kernel] syscall gate active");

    // K4: register VMCALL handler for host_await.
    hypercall::init();
    kprintln!("[axon-kernel] hypercall substrate active");

    // Run the Axon program.  The binary is loaded at a well-known guest address
    // (passed via MMDS or a fixed convention).  For now, spin waiting for the
    // interpreter to be invoked via hypercall from the host.
    kprintln!("[axon-kernel] ready — waiting for interpreter launch via hypercall");
    hypercall::wait_for_run();
}

// ── Panic handler ─────────────────────────────────────────────────────────────

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("[axon-kernel] PANIC: {}", info);
    // Write sentinel so axon-vm can detect kernel panic vs clean exit.
    serial::write_str("\x1b[K-PANIC\n");
    loop {
        unsafe { core::arch::asm!("hlt") };
    }
}

// ── Global allocator (none — bare metal, bump only) ──────────────────────────

// No #[global_allocator]: we use the bump allocator directly via bump::alloc().
// Rust's Box/Vec/etc. are not available in this crate by design.

// Include the boot assembly.
core::arch::global_asm!(include_str!("boot.s"));
