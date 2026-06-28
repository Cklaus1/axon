//! Bump allocator — kernel heap.
//!
//! Region: 0x21A000 – 0xFFFFFF (~13.8 MiB), placed above the kernel binary.
//!
//! Kernel layout (from linker.ld):
//!   0x100000  .note.Xen + .text32  (32-bit boot stub)
//!   0x200000  .text64_entry + .text + .rodata + .data
//!   0x204000  .bss  (page-table buffers: _pml4/_pdpt/_pd at 0x206/207/208000)
//!   0x209000  _stack_bottom
//!   0x219000  _stack_top  ← heap starts immediately after this
//!
//! Single-threaded; no lock needed during the bare-metal boot phase.

use core::sync::atomic::{AtomicUsize, Ordering};

// Physical start of the heap region — one page above _stack_top (0x219000).
const HEAP_START: usize = 0x21A_000;
const HEAP_END: usize = 0xFF_FFFF;

static BUMP: AtomicUsize = AtomicUsize::new(0);
static INITIALIZED: AtomicUsize = AtomicUsize::new(0);

pub fn init() {
    BUMP.store(HEAP_START, Ordering::Relaxed);
    INITIALIZED.store(1, Ordering::Relaxed);
}

/// Allocate `size` bytes aligned to `align` (must be a power of two).
/// Returns a pointer into the bump region, or panics on exhaustion.
pub fn alloc(size: usize, align: usize) -> *mut u8 {
    debug_assert!(align.is_power_of_two());
    let mut current = BUMP.load(Ordering::Relaxed);
    loop {
        let aligned = (current + align - 1) & !(align - 1);
        let next = aligned + size;
        if next > HEAP_END {
            panic!("kernel heap exhausted");
        }
        match BUMP.compare_exchange(current, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return aligned as *mut u8,
            Err(actual) => current = actual,
        }
    }
}

/// Allocate a zeroed region of `size` bytes aligned to `align`.
pub fn alloc_zeroed(size: usize, align: usize) -> *mut u8 {
    let ptr = alloc(size, align);
    unsafe { core::ptr::write_bytes(ptr, 0, size) };
    ptr
}
