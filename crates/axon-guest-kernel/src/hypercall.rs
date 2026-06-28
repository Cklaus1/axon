//! K4: VMCALL hypercall substrate (stub — filled in by K4 agent).
//!
//! Provides the guest side of host_await: the Axon interpreter calls
//! host_await(payload) → kernel issues VMCALL #1 → axon-vm launcher handles
//! it via KVM_EXIT_HYPERCALL → reply written into guest memory → VMCALL #2
//! resumes the interpreter with the reply.

/// Register the VMCALL handler.  Called once at boot.
pub fn init() {
    // TODO (K4 agent): set up VMCALL dispatch table.
    // Stub: nothing to do yet.
}

/// K4: the run trigger. The policy and program are delivered via the boot cmdline, so no
/// host round-trip is needed to START the program — the kernel launches it directly. (The
/// host VMCALL path remains for the interactive `host_await` case, `vmcall_await`.) Hands
/// off to K5 (`enforce::run_program`), which runs the program under the syscall gate and
/// diverges (the program halts, or the gate halts it on a policy violation).
pub fn wait_for_run(policy: &crate::mmds::Policy) -> ! {
    kprintln!("[axon-kernel] K4: run-request (cmdline-delivered program) — launching");
    crate::enforce::run_program(policy)
}

/// Issue VMCALL #1: send a host_await request payload to the host.
/// Returns the reply length written into `reply_buf`, or 0 on EOF.
pub fn vmcall_await(payload: &[u8], reply_buf: &mut [u8]) -> usize {
    // VMCALL ABI (axon-vm specific):
    //   rax = 1 (host_await request)
    //   rdi = payload ptr
    //   rsi = payload len
    //   rdx = reply buf ptr
    //   rcx = reply buf len
    // Return: rax = reply len (0 = EOF / no host)
    let ret: u64;
    unsafe {
        core::arch::asm!(
            "vmcall",
            inout("rax") 1u64 => ret,
            in("rdi") payload.as_ptr() as u64,
            in("rsi") payload.len() as u64,
            in("rdx") reply_buf.as_mut_ptr() as u64,
            in("rcx") reply_buf.len() as u64,
            options(nostack),
        );
    }
    ret as usize
}
