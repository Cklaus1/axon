//! Axon runtime library — channel and concurrency primitives.
//!
//! Compiled as a static library and linked into every Axon binary.  All
//! exported symbols use C linkage so the LLVM-emitted code can call them
//! directly by name.
//!
//! Channel implementation: a bounded MPSC/MPMC queue backed by a
//! `Mutex<VecDeque<i64>>` + two `Condvar`s (one for senders, one for receivers).
//! All channel values are `i64`; the codegen is responsible for casting other
//! integer types through `i64`.

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::{Arc, Condvar, Mutex};

pub mod provenance;
pub mod goal;
pub mod adaptive_registry;

// ── Axon str ABI — mirrors codegen's { i64 len, i8* ptr } ──────────────────────

/// Mirrors codegen's Axon `str` layout: LLVM `{ i64 len, i8* ptr }` by value.
///
/// SAFETY/ABI: on the supported targets (x86-64/aarch64 Linux) a 16-byte
/// `{i64, ptr}` aggregate is passed identically by `repr(C)` Rust and by LLVM,
/// so codegen's `str_ty.fn_type(&[str_ty, ...])` matches `fn f(s: AxonStr, ...)`.
/// This ABI match is asserted-by-construction and is RUNTIME-validated only by
/// the native build (R1-gated) — the unit tests below verify LOGIC, not ABI.
#[repr(C)]
pub struct AxonStr {
    pub len: i64,
    pub ptr: *const u8,
}

impl AxonStr {
    /// Reconstruct a `&str`. Caller guarantees ptr/len came from codegen's str ABI.
    pub unsafe fn as_str<'a>(&self) -> &'a str {
        if self.ptr.is_null() || self.len <= 0 {
            return "";
        }
        let bytes = std::slice::from_raw_parts(self.ptr, self.len as usize);
        std::str::from_utf8(bytes).unwrap_or("")
    }
}

// ── Channel ───────────────────────────────────────────────────────────────────

struct Chan {
    queue: Mutex<VecDeque<i64>>,
    not_empty: Condvar,
    not_full: Condvar,
    capacity: usize,
}

impl Chan {
    fn new(capacity: usize) -> Self {
        Chan {
            queue: Mutex::new(VecDeque::with_capacity(capacity)),
            not_empty: Condvar::new(),
            not_full: Condvar::new(),
            capacity,
        }
    }

    fn send(&self, val: i64) {
        let mut q = self.queue.lock().unwrap();
        while q.len() >= self.capacity {
            q = self.not_full.wait(q).unwrap();
        }
        q.push_back(val);
        self.not_empty.notify_one();
    }

    fn recv(&self) -> i64 {
        let mut q = self.queue.lock().unwrap();
        while q.is_empty() {
            q = self.not_empty.wait(q).unwrap();
        }
        let val = q.pop_front().unwrap();
        self.not_full.notify_one();
        val
    }

    fn try_recv(&self) -> Option<i64> {
        let mut q = self.queue.lock().unwrap();
        if q.is_empty() {
            None
        } else {
            let val = q.pop_front().unwrap();
            self.not_full.notify_one();
            Some(val)
        }
    }

    fn has_data(&self) -> bool {
        !self.queue.lock().unwrap().is_empty()
    }
}

/// Create a new channel with the given capacity.
/// Returns an opaque pointer to an `Arc<Chan>`, heap-allocated.
#[no_mangle]
pub extern "C" fn __axon_chan_new(capacity: i64) -> *mut c_void {
    let cap = if capacity <= 0 { 1 } else { capacity as usize };
    let arc = Arc::new(Chan::new(cap));
    let raw = Arc::into_raw(arc) as *mut c_void;
    raw
}

/// Send `val` to the channel.  Blocks if the buffer is full.
#[no_mangle]
pub extern "C" fn __axon_chan_send(chan: *mut c_void, val: i64) {
    assert!(!chan.is_null(), "axon_chan_send: null channel");
    let arc = unsafe { Arc::from_raw(chan as *const Chan) };
    arc.send(val);
    // Keep the Arc alive — don't drop it.
    let _ = Arc::into_raw(arc);
}

/// Receive a value from the channel.  Blocks until one is available.
#[no_mangle]
pub extern "C" fn __axon_chan_recv(chan: *mut c_void) -> i64 {
    assert!(!chan.is_null(), "axon_chan_recv: null channel");
    let arc = unsafe { Arc::from_raw(chan as *const Chan) };
    let val = arc.recv();
    let _ = Arc::into_raw(arc);
    val
}

/// Clone a channel handle — increments the Arc reference count.
/// Both the original and the clone refer to the same underlying channel.
#[no_mangle]
pub extern "C" fn __axon_chan_clone(chan: *mut c_void) -> *mut c_void {
    assert!(!chan.is_null(), "axon_chan_clone: null channel");
    let arc = unsafe { Arc::from_raw(chan as *const Chan) };
    let cloned = Arc::clone(&arc);
    let _ = Arc::into_raw(arc); // restore original
    Arc::into_raw(cloned) as *mut c_void
}

/// Drop the channel (decrease reference count).  Call when done.
#[no_mangle]
pub extern "C" fn __axon_chan_drop(chan: *mut c_void) {
    if !chan.is_null() {
        unsafe { drop(Arc::from_raw(chan as *const Chan)) };
    }
}

/// Select across N channels — returns the index of the first ready one.
///
/// Algorithm: spin-poll each channel in round-robin with a short sleep until
/// one has data available, then return its index.  This is a simple but correct
/// implementation; a production runtime would use platform futexes.
///
/// `chans` is a `*mut *mut c_void` array of `n` channel pointers.
/// The function does NOT consume the channels (reference counts unchanged).
#[no_mangle]
pub extern "C" fn __axon_select(chans: *mut *mut c_void, n: i64) -> i64 {
    use std::thread;
    use std::time::Duration;

    assert!(!chans.is_null() && n > 0, "axon_select: invalid args");
    let count = n as usize;

    // Poll in round-robin until a channel has data, then return its index.
    // The arm body is responsible for calling recv() to actually dequeue the value.
    loop {
        for i in 0..count {
            let ptr = unsafe { *chans.add(i) };
            if ptr.is_null() { continue; }
            let arc = unsafe { Arc::from_raw(ptr as *const Chan) };
            let ready = arc.has_data();
            let _ = Arc::into_raw(arc); // keep alive
            if ready {
                return i as i64;
            }
        }
        thread::sleep(Duration::from_micros(100));
    }
}

// ── Spawn ─────────────────────────────────────────────────────────────────────

/// Spawn a new OS thread.
///
/// `fn_ptr` is a function pointer with signature `fn(*mut c_void)`.
/// `env`    is the closure environment (captured variables), passed as the
///          sole argument.  May be null if the spawned function takes no captures.
#[no_mangle]
pub extern "C" fn __axon_spawn(fn_ptr: *const c_void, env: *mut c_void) {
    assert!(!fn_ptr.is_null(), "axon_spawn: null function pointer");
    let fn_ptr = fn_ptr as usize;  // move into thread
    let env = env as usize;
    std::thread::spawn(move || {
        let f: extern "C" fn(*mut c_void) = unsafe { std::mem::transmute(fn_ptr) };
        f(env as *mut c_void);
    });
}

// ── Builtins ──────────────────────────────────────────────────────────────────

/// Print a string to stdout followed by a newline.
#[no_mangle]
pub extern "C" fn __axon_print(ptr: *const u8, len: i64) {
    if ptr.is_null() || len <= 0 {
        println!();
        return;
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
    let s = std::str::from_utf8(slice).unwrap_or("<invalid utf8>");
    println!("{s}");
}

/// Absolute value of an i64.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` to cut IR-generation
/// volume from the native build (R1, `governance/specs/R1-codegen-build-unblock.md`,
/// Batch 1). Behavior matches the interpreter oracle (`interp.rs` `abs_i64`,
/// which is Rust's `i64::abs`): `abs_i64(i64::MIN)` overflows. The interpreter
/// turns that into a panic-exit; a C-ABI extern cannot unwind, so we abort
/// deterministically with the same intent rather than invoke UB or silently
/// wrap (wrapping would reintroduce the interp↔codegen drift class of
/// BUG_HUNT #33/#36/#37). The common case is a branchless `i64::abs`.
#[no_mangle]
pub extern "C" fn __axon_abs_i64(n: i64) -> i64 {
    match n.checked_abs() {
        Some(v) => v,
        None => {
            eprintln!("axon: panic: abs_i64 overflow (i64::MIN has no positive)");
            std::process::abort();
        }
    }
}

/// Minimum of two i64 values.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 1). Matches the
/// interpreter oracle: `a.min(b)`.
#[no_mangle]
pub extern "C" fn __axon_min_i64(a: i64, b: i64) -> i64 {
    a.min(b)
}

// ── String builtins — scalar-return (R1 Batch 2) ───────────────────────────────

/// Check if haystack contains needle.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `a.contains(b)`.
#[no_mangle]
pub extern "C" fn __axon_str_contains(a: AxonStr, b: AxonStr) -> bool {
    let a = unsafe { a.as_str() };
    let b = unsafe { b.as_str() };
    a.contains(b)
}

/// Check if haystack starts with prefix.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `a.starts_with(b)`.
#[no_mangle]
pub extern "C" fn __axon_str_starts_with(a: AxonStr, b: AxonStr) -> bool {
    let a = unsafe { a.as_str() };
    let b = unsafe { b.as_str() };
    a.starts_with(b)
}

/// Check if haystack ends with suffix.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `a.ends_with(b)`.
#[no_mangle]
pub extern "C" fn __axon_str_ends_with(a: AxonStr, b: AxonStr) -> bool {
    let a = unsafe { a.as_str() };
    let b = unsafe { b.as_str() };
    a.ends_with(b)
}

/// Byte index of first occurrence of needle in haystack, or -1 if not found.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `h.find(needle).map(|i| i as i64).unwrap_or(-1)`.
#[no_mangle]
pub extern "C" fn __axon_str_index_of(hay: AxonStr, needle: AxonStr) -> i64 {
    let hay = unsafe { hay.as_str() };
    let needle = unsafe { needle.as_str() };
    hay.find(needle).map(|i| i as i64).unwrap_or(-1)
}

/// Byte length of the string.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `s.len() as i64` (byte length, not char count).
#[no_mangle]
pub extern "C" fn __axon_str_len(s: AxonStr) -> i64 {
    let s = unsafe { s.as_str() };
    s.len() as i64
}

/// Byte value at index i, or -1 if out of bounds.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `s.as_bytes().get(i.max(0) as usize).map(|b| *b as i64).unwrap_or(-1)`.
#[no_mangle]
pub extern "C" fn __axon_char_at(s: AxonStr, i: i64) -> i64 {
    let s = unsafe { s.as_str() };
    let i = i.max(0) as usize;
    s.as_bytes().get(i).map(|b| *b as i64).unwrap_or(-1)
}

/// Maximum of two i64 values.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 1). Matches the
/// interpreter oracle: `a.max(b)`.
#[no_mangle]
pub extern "C" fn __axon_max_i64(a: i64, b: i64) -> i64 {
    a.max(b)
}

/// Sign of an i64: -1 if negative, 0 if zero, 1 if positive.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 1). Matches the
/// interpreter oracle: `n.signum()`.
#[no_mangle]
pub extern "C" fn __axon_sign_i64(n: i64) -> i64 {
    n.signum()
}

/// Clamp i64 `n` to `[lo, hi]` — returns `n.max(lo).min(hi)`.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 1). Matches the
/// interpreter oracle.
#[no_mangle]
pub extern "C" fn __axon_clamp_i64(n: i64, lo: i64, hi: i64) -> i64 {
    n.max(lo).min(hi)
}

/// Clamp f64 `n` to `[lo, hi]` — returns `n.max(lo).min(hi)`.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 1). Matches the
/// interpreter oracle.
#[no_mangle]
pub extern "C" fn __axon_clamp_f64(n: f64, lo: f64, hi: f64) -> f64 {
    n.max(lo).min(hi)
}

/// Absolute value of an i32.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` to cut IR-generation
/// volume from the native build (R1, `governance/specs/R1-codegen-build-unblock.md`,
/// Batch 3). Behavior matches the interpreter oracle (`interp.rs` `abs_i32`,
/// which is Rust's `i64::abs` computed on the i32 value). `abs_i32(i32::MIN)`
/// overflows. The interpreter turns that into a panic-exit; a C-ABI extern
/// cannot unwind, so we abort deterministically with the same intent rather
/// than invoke UB or silently wrap.
#[no_mangle]
pub extern "C" fn __axon_abs_i32(n: i32) -> i32 {
    match n.checked_abs() {
        Some(v) => v,
        None => {
            eprintln!("axon: panic: abs_i32 overflow (i32::MIN has no positive)");
            std::process::abort();
        }
    }
}

/// Minimum of two i32 values.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 3). Matches the
/// interpreter oracle: `a.min(b)`.
#[no_mangle]
pub extern "C" fn __axon_min_i32(a: i32, b: i32) -> i32 {
    a.min(b)
}

/// Maximum of two i32 values.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 3). Matches the
/// interpreter oracle: `a.max(b)`.
#[no_mangle]
pub extern "C" fn __axon_max_i32(a: i32, b: i32) -> i32 {
    a.max(b)
}

/// Absolute value of an f64.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 3). Matches the
/// interpreter oracle: `x.abs()`. No overflow semantics for f64.
#[no_mangle]
pub extern "C" fn __axon_abs_f64(x: f64) -> f64 {
    x.abs()
}

/// Integer power: `base.wrapping_pow(exp as u32)`.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 1). Matches the
/// interpreter oracle. Negative exponent aborts (no C-ABI unwind), matching
/// the interpreter's panic-exit for `pow_i64(base, exp < 0)`.
#[no_mangle]
pub extern "C" fn __axon_pow_i64(base: i64, exp: i64) -> i64 {
    if exp < 0 {
        eprintln!("axon: panic: pow_i64: negative exponent");
        std::process::abort();
    }
    base.wrapping_pow(exp as u32)
}

/// Integer square root (returns i64).
#[no_mangle]
pub extern "C" fn __axon_sqrt(x: f64) -> f64 {
    x.sqrt()
}

/// Integer power.
#[no_mangle]
pub extern "C" fn __axon_pow(base: f64, exp: f64) -> f64 {
    base.powf(exp)
}

/// Floor.
#[no_mangle]
pub extern "C" fn __axon_floor(x: f64) -> f64 {
    x.floor()
}

/// Ceil.
#[no_mangle]
pub extern "C" fn __axon_ceil(x: f64) -> f64 {
    x.ceil()
}

// ── Phase 4: I/O builtins ──────────────────────────────────────────────────────

/// Read one line from stdin.
/// Returns `(len: i64, ptr: *mut u8)` via out-params.
/// The caller owns the buffer and must free it.
#[no_mangle]
pub extern "C" fn __axon_read_line(out_len: *mut i64, out_ptr: *mut *mut u8) {
    use std::io::BufRead;
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin.lock().read_line(&mut line).unwrap_or(0);
    // Strip trailing newline.
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') { line.pop(); }
    }
    let len = line.len();
    let buf = unsafe {
        let p = libc_malloc(len + 1);
        std::ptr::copy_nonoverlapping(line.as_ptr(), p, len);
        *p.add(len) = 0;
        p
    };
    unsafe {
        *out_len = len as i64;
        *out_ptr = buf;
    }
}

/// Read the entire contents of `path` (null-terminated) into a heap buffer.
/// Returns `(len: i64, ptr: *mut u8)` via out-params.
/// On error, sets len to -1 and writes the error message into ptr.
#[no_mangle]
pub extern "C" fn __axon_read_file(
    path_ptr: *const u8,
    path_len: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let path = unsafe {
        let s = std::slice::from_raw_parts(path_ptr, path_len as usize);
        std::str::from_utf8_unchecked(s)
    };
    match std::fs::read_to_string(path) {
        Ok(content) => {
            let len = content.len();
            let buf = unsafe {
                let p = libc_malloc(len + 1);
                std::ptr::copy_nonoverlapping(content.as_ptr(), p, len);
                *p.add(len) = 0;
                p
            };
            unsafe { *out_len = len as i64; *out_ptr = buf; }
        }
        Err(e) => {
            let msg = e.to_string();
            let len = msg.len();
            let buf = unsafe {
                let p = libc_malloc(len + 1);
                std::ptr::copy_nonoverlapping(msg.as_ptr(), p, len);
                *p.add(len) = 0;
                p
            };
            unsafe { *out_len = -(len as i64); *out_ptr = buf; }
        }
    }
}

/// R6: spawn a process — the native `exec` builtin. `cmd` is the program;
/// `args_ptr` points to `args_count` `AxonStr` argument structs (the `[str]`
/// array's element buffer). Returns the captured stdout on success or the error
/// message, via the same ±len out-param convention as `__axon_read_file`
/// (len ≥ 0 → Ok stdout; len < 0 → Err with `|len|`). Mirrors the interpreter's
/// DefaultHost::exec so native==interp.
#[no_mangle]
pub extern "C" fn __axon_exec(
    cmd: AxonStr,
    args_ptr: *const AxonStr,
    args_count: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let cmd_s = unsafe { cmd.as_str() };
    let args: Vec<String> = if args_ptr.is_null() || args_count <= 0 {
        Vec::new()
    } else {
        let slice = unsafe { std::slice::from_raw_parts(args_ptr, args_count as usize) };
        slice.iter().map(|a| unsafe { a.as_str() }.to_string()).collect()
    };
    let (text, is_err) = match std::process::Command::new(cmd_s).args(&args).output() {
        Ok(output) if output.status.success() => {
            (String::from_utf8_lossy(&output.stdout).into_owned(), false)
        }
        Ok(output) => {
            let code = output.status.code().map(|c| c.to_string()).unwrap_or_else(|| "signal".to_string());
            let stderr = String::from_utf8_lossy(&output.stderr);
            (format!("exec `{cmd_s}` exited {code}: {}", stderr.trim()), true)
        }
        Err(e) => (format!("exec `{cmd_s}` failed: {e}"), true),
    };
    let len = text.len();
    let buf = unsafe {
        let p = libc_malloc(len + 1);
        std::ptr::copy_nonoverlapping(text.as_ptr(), p, len);
        *p.add(len) = 0;
        p
    };
    unsafe {
        *out_len = if is_err { -(len as i64) } else { len as i64 };
        *out_ptr = buf;
    }
}

/// Write `content` to `path`.  Returns 0 on success; on error returns the error
/// message length (positive) and writes the message to `*out_ptr`.
#[no_mangle]
pub extern "C" fn __axon_write_file(
    path_ptr: *const u8,
    path_len: i64,
    content_ptr: *const u8,
    content_len: i64,
    out_err_len: *mut i64,
    out_err_ptr: *mut *mut u8,
) {
    let path = unsafe {
        let s = std::slice::from_raw_parts(path_ptr, path_len as usize);
        std::str::from_utf8_unchecked(s)
    };
    let content = unsafe { std::slice::from_raw_parts(content_ptr, content_len as usize) };
    match std::fs::write(path, content) {
        Ok(()) => unsafe { *out_err_len = 0; *out_err_ptr = std::ptr::null_mut(); },
        Err(e) => {
            let msg = e.to_string();
            let len = msg.len();
            let buf = unsafe {
                let p = libc_malloc(len + 1);
                std::ptr::copy_nonoverlapping(msg.as_ptr(), p, len);
                *p.add(len) = 0;
                p
            };
            unsafe { *out_err_len = len as i64; *out_err_ptr = buf; }
        }
    }
}

// ── Phase 4: Time builtins ─────────────────────────────────────────────────────

/// Suspend the current thread for at least `ms` milliseconds.
#[no_mangle]
pub extern "C" fn __axon_sleep_ms(ms: i64) {
    if ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(ms as u64));
    }
}

/// Return the current wall-clock time as milliseconds since the Unix epoch.
#[no_mangle]
pub extern "C" fn __axon_now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ── Phase 10: i64_to_str_radix ────────────────────────────────────────────────

/// Convert `n` to a string in the given `base` (2–36).
///
/// Negative numbers get a `'-'` prefix.  Bases outside [2, 36] produce an
/// empty string.  The caller owns the returned buffer (heap-allocated with
/// `std::alloc`); it is never freed by the runtime (no GC in Phase 1–10).
///
/// Out-params: `*out_len` receives the byte length; `*out_ptr` receives the
/// pointer to the first byte of a NUL-terminated buffer.
#[no_mangle]
pub extern "C" fn __axon_i64_to_str_radix(
    n: i64,
    base: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    // Validate base.
    if !(2..=36).contains(&base) {
        // Return empty string.
        let buf = unsafe { libc_malloc(1) };
        unsafe { *buf = 0 };
        unsafe { *out_len = 0; *out_ptr = buf; }
        return;
    }
    let base = base as u64;

    // Handle sign.
    let negative = n < 0;
    // Use u64 to avoid overflow on i64::MIN.
    let mut value: u64 = if negative {
        (n as i128).unsigned_abs() as u64
    } else {
        n as u64
    };

    // Build digits in reverse into a fixed-size stack buffer.
    // Max digits: base-2 gives 64 digits; +1 for sign; +1 for NUL = 66.
    let mut tmp = [0u8; 66];
    let mut pos = 66usize;

    // NUL terminator at the very end.
    pos -= 1;
    tmp[pos] = 0;

    // Digits.
    loop {
        pos -= 1;
        let digit = (value % base) as u8;
        tmp[pos] = if digit < 10 { b'0' + digit } else { b'a' + (digit - 10) };
        value /= base;
        if value == 0 { break; }
    }

    // Sign.
    if negative {
        pos -= 1;
        tmp[pos] = b'-';
    }

    let len = 65 - pos; // excludes the NUL at index 65
    let buf = unsafe { libc_malloc(len + 1) };
    unsafe {
        std::ptr::copy_nonoverlapping(tmp.as_ptr().add(pos), buf, len + 1);
        *out_len = len as i64;
        *out_ptr = buf;
    }
}

/// Write a str result via out-params: malloc a NUL-terminated buffer, copy the
/// string bytes, set *out_len and *out_ptr.  Caller owns the returned buffer.
#[inline(never)]
unsafe fn write_str_out(
    s: &str,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let len = s.len();
    let buf = libc_malloc(len + 1);
    unsafe {
        std::ptr::copy_nonoverlapping(s.as_ptr(), buf, len);
        *buf.add(len) = 0; // NUL-terminate
        *out_len = len as i64;
        *out_ptr = buf;
    }
}

// ── R1 Batch 2b: str_repeat ───────────────────────────────────────────────────
/// `str_repeat(s, n)` — repeats string `s` `n` times (n clamped to max(0,n)).
/// Uses the out-param convention: the caller allocates slots; this function
/// mallocs the result and writes {byte_length, buffer_ptr}.
#[no_mangle]
pub extern "C" fn __axon_str_repeat(
    s: AxonStr,
    n: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let src = unsafe { s.as_str() };
    let count = n.max(0) as usize;
    let result = src.repeat(count);
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

// ── R1 Batch 2b: str_slice ────────────────────────────────────────────────────
/// `str_slice(s, start, end)` — byte-indexed slice of `s`.
/// Clamps start to [0, len], end to [start, len]; returns "" if byte range
/// crosses a UTF-8 boundary (s.get returns None).
#[no_mangle]
pub extern "C" fn __axon_str_slice(
    s: AxonStr,
    start: i64,
    end: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let src = unsafe { s.as_str() };
    let start = (start.max(0) as usize).min(src.len());
    let end = (end.max(0) as usize).min(src.len());
    let start = start.min(end);
    let slice = src.get(start..end).unwrap_or("");
    unsafe { write_str_out(slice, out_len, out_ptr) }
}

// ── BUG_HUNT #37: parse_int Err message (echoes the input, like the interp) ──
/// Build the `parse_int` error message for a failed input, matching the
/// interpreter's I-2-canonical form `` could not parse `<input>` as a base-10
/// integer ``. Codegen calls this from the Err branch (it has the input str)
/// instead of emitting a static message, so native==interp on the message too.
#[no_mangle]
pub extern "C" fn __axon_parse_int_err(
    input: AxonStr,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let src = unsafe { input.as_str() };
    // Mirror the interpreter (interp.rs): a radix-prefixed input gets a hint.
    let lower = src.to_ascii_lowercase();
    let hint = if lower.starts_with("0x") || lower.starts_with("0o") || lower.starts_with("0b") {
        " (parse_int is base-10 only; strip the radix prefix)"
    } else {
        ""
    };
    let msg = format!("could not parse `{src}` as a base-10 integer{hint}");
    unsafe { write_str_out(&msg, out_len, out_ptr) }
}

// ── BUG_HUNT #38: str_reverse (char-correct, not byte-reverse) ────────────────
/// `str_reverse(s)` — reverse by Unicode scalar (char), matching the
/// interpreter (`chars().rev()`). The old inline codegen reversed BYTES, which
/// mangles any multibyte UTF-8 (`str_reverse("héllo")` → invalid bytes); this
/// is the I-2-canonical implementation codegen now calls instead.
#[no_mangle]
pub extern "C" fn __axon_str_reverse(
    s: AxonStr,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let src = unsafe { s.as_str() };
    let result: String = src.chars().rev().collect();
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

// ── BUG_HUNT #39: str_replace (matches Rust str::replace) ─────────────────────
/// `str_replace(s, from, to)` — replace every occurrence of `from` with `to`,
/// matching the interpreter (Rust `str::replace`). In particular an empty
/// `from` interleaves `to` between every char (and at both ends), e.g.
/// `str_replace("abc", "", "X")` → `"XaXbXcX"`. The old inline codegen skipped
/// the empty-`from` case (returned `s` unchanged) — a silent divergence.
#[no_mangle]
pub extern "C" fn __axon_str_replace(
    s: AxonStr,
    from: AxonStr,
    to: AxonStr,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let src = unsafe { s.as_str() };
    let from_s = unsafe { from.as_str() };
    let to_s = unsafe { to.as_str() };
    let result = src.replace(from_s, to_s);
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

// ── ASI Layer-3: @[verify] runtime enforcement ────────────────────────────────

/// Runtime panic for `@[verify(confidence OP K)]` violations.
///
/// Codegen injects a call to this symbol at every return site of an
/// `@[verify]`-annotated function whose `Uncertain<T>` return value has a
/// runtime confidence that fails the predicate.  The static `verify::check_verify`
/// pass catches *definite* violations at compile time; this runtime hook
/// catches violations whose source is unknown to the static lattice
/// (e.g. confidence flowing in from `ai_extract_uncertain_*`).
///
/// Behaviour: writes a one-line error message to stderr and exits with
/// [`VERIFY_FAILED_EXIT_CODE`] (3). A `@[verify]` violation is a *policy*
/// rejection — the artifact didn't meet its declared bound — distinct from a
/// bug-crash (SIGABRT / 101), so CI can branch on it. This mirrors the
/// interpreter, which exits 3 on the same condition (BUG_HUNT #26). Previously
/// this called `abort()`, conflating policy rejection with a programmer error.
///
/// Parameters:
/// * `fn_name_ptr` / `fn_name_len` — pointer + byte length of the offending
///   function's name (Axon `str` ABI).
/// * `op_ptr` / `op_len` — the source-level operator string (`">="`, `">"`,
///   `"<="`, `"<"`, `"=="`, `"!="`).  Used only for the message; the runtime
///   does no semantic interpretation.
/// * `bound` — the literal `f64` from the predicate.
/// * `actual` — the runtime confidence extracted from the `Uncertain<T>`
///   value at the return site.
/// Exit code for an `@[verify]` / deploy-gate rejection. Must match the
/// interpreter's `interp::VERIFY_FAILED_EXIT_CODE` (axon-rt has no dependency
/// on axon-core, so the value is duplicated, not imported) — BUG_HUNT #26.
pub const VERIFY_FAILED_EXIT_CODE: i32 = 3;

#[no_mangle]
pub extern "C" fn __axon_verify_panic(
    fn_name_ptr: *const u8,
    fn_name_len: i64,
    op_ptr: *const u8,
    op_len: i64,
    bound: f64,
    actual: f64,
) -> ! {
    let msg = format_verify_panic(fn_name_ptr, fn_name_len, op_ptr, op_len, bound, actual);
    eprintln!("{msg}");
    std::process::exit(VERIFY_FAILED_EXIT_CODE);
}

/// Produce the verify-panic message without aborting.  Factored out so unit
/// tests can assert on the formatted text without taking the process down.
fn format_verify_panic(
    fn_name_ptr: *const u8,
    fn_name_len: i64,
    op_ptr: *const u8,
    op_len: i64,
    bound: f64,
    actual: f64,
) -> String {
    let fn_name = verify_slice_to_str(fn_name_ptr, fn_name_len);
    let op = verify_slice_to_str(op_ptr, op_len);
    format!(
        "axon: verify violation in {}: confidence {op} {bound} failed (actual={actual})",
        verify_fn_label(fn_name)
    )
}

/// Founder-facing label for a `@[verify]`-armed function. Mirrors
/// `interp::verify_fn_label` so the native and interpreted paths agree
/// (BUG_HUNT #25): the generated deploy-gate symbol `assert_deployable` is an
/// impl detail and must not leak; author-named gates keep their own name.
fn verify_fn_label(fn_name: &str) -> String {
    if fn_name == "assert_deployable" {
        "the deploy gate".to_string()
    } else {
        format!("`{fn_name}`")
    }
}

fn verify_slice_to_str<'a>(ptr: *const u8, len: i64) -> &'a str {
    if ptr.is_null() || len <= 0 {
        return "<unknown>";
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        std::str::from_utf8(bytes).unwrap_or("<invalid utf8>")
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────────

unsafe fn libc_malloc(size: usize) -> *mut u8 {
    let layout = std::alloc::Layout::from_size_align(size, 1).unwrap();
    std::alloc::alloc(layout)
}

// Note: libc_free is intentionally omitted — our libc_malloc uses
// std::alloc::alloc which requires the exact Layout for dealloc.
// Tests leak intentionally; the runtime pattern (no GC) means callers
// own the buffer but the test harness doesn't bother freeing.

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod verify_panic_tests {
    use super::*;

    #[test]
    fn message_contains_fn_name_op_bound_actual() {
        let fn_name = b"safe_extract";
        let op = b">=";
        let msg = format_verify_panic(
            fn_name.as_ptr(),
            fn_name.len() as i64,
            op.as_ptr(),
            op.len() as i64,
            0.8,
            0.42,
        );
        assert!(msg.contains("safe_extract"), "msg: {msg}");
        assert!(msg.contains(">="),           "msg: {msg}");
        assert!(msg.contains("0.8"),          "msg: {msg}");
        assert!(msg.contains("0.42"),         "msg: {msg}");
        assert!(msg.contains("verify violation"), "msg: {msg}");
    }

    #[test]
    fn message_hides_generated_deploy_gate_symbol() {
        // BUG_HUNT #25: the generated `assert_deployable` gate must read as
        // "the deploy gate" to founders, not leak the internal symbol —
        // matching the interpreter path.
        let fn_name = b"assert_deployable";
        let op = b">=";
        let msg = format_verify_panic(
            fn_name.as_ptr(),
            fn_name.len() as i64,
            op.as_ptr(),
            op.len() as i64,
            0.9,
            0.6,
        );
        assert!(msg.contains("the deploy gate"), "msg: {msg}");
        assert!(!msg.contains("assert_deployable"), "internal symbol leaked: {msg}");
    }

    #[test]
    fn message_handles_null_ptrs_gracefully() {
        let msg = format_verify_panic(
            std::ptr::null(), 0,
            std::ptr::null(), 0,
            0.5, 0.1,
        );
        // Should not panic; should contain placeholder text.
        assert!(msg.contains("<unknown>"), "msg: {msg}");
    }
}

// ── R1 Batch 1: migrated-builtin parity (R1-codegen-build-unblock.md §8) ──────
#[cfg(test)]
mod migrated_builtin_tests {
    use super::*;

    /// The interpreter oracle for `abs_i64` is Rust's `i64::abs` (interp.rs:
    /// `as_int(..).abs()`). The migrated extern must agree across a value sweep
    /// for every input where the oracle is defined (i.e. not i64::MIN, which
    /// overflows in both — covered separately).
    #[test]
    fn migrated_abs_i64_matches_interpreter() {
        let oracle = |n: i64| n.abs(); // exactly interp.rs's abs_i64
        let sweep = [
            0i64, 1, -1, 42, -42, 7, -7,
            1000, -1000, i64::MAX, i64::MAX - 1, i64::MIN + 1,
        ];
        for &n in &sweep {
            assert_eq!(
                __axon_abs_i64(n),
                oracle(n),
                "abs_i64({n}) diverges from the interpreter oracle"
            );
        }
    }

    #[test]
    fn migrated_abs_i64_common_cases() {
        assert_eq!(__axon_abs_i64(-5), 5);
        assert_eq!(__axon_abs_i64(5), 5);
        assert_eq!(__axon_abs_i64(0), 0);
        assert_eq!(__axon_abs_i64(i64::MIN + 1), i64::MAX);
    }

    // Note: __axon_abs_i64(i64::MIN) aborts (matching the interpreter's
    // panic-exit on negate-overflow). Not unit-tested here because
    // process::abort would kill the test runner — the behavior is asserted by
    // the interpreter-side test that abs_i64(i64::MIN) exits 101.

    // ── min_i64: matches interp.rs a.min(b) ──────────────────────────
    #[test]
    fn migrated_min_i64_matches_interpreter() {
        let oracle = |a: i64, b: i64| a.min(b);
        let vals = [
            0i64, 1, -1, 42, -42, 7, -7,
            1000, -1000, i64::MAX, i64::MIN,
        ];
        for &a in &vals {
            for &b in &vals {
                assert_eq!(
                    __axon_min_i64(a, b),
                    oracle(a, b),
                    "min_i64({a}, {b}) diverges"
                );
            }
        }
    }

    #[test]
    fn migrated_min_i64_common_cases() {
        assert_eq!(__axon_min_i64(3, 7), 3);
        assert_eq!(__axon_min_i64(7, 3), 3);
        assert_eq!(__axon_min_i64(0, 0), 0);
        assert_eq!(__axon_min_i64(i64::MIN, i64::MAX), i64::MIN);
    }

    // ── max_i64: matches interp.rs a.max(b) ──────────────────────────
    #[test]
    fn migrated_max_i64_matches_interpreter() {
        let oracle = |a: i64, b: i64| a.max(b);
        let vals = [
            0i64, 1, -1, 42, -42, 7, -7,
            1000, -1000, i64::MAX, i64::MIN,
        ];
        for &a in &vals {
            for &b in &vals {
                assert_eq!(
                    __axon_max_i64(a, b),
                    oracle(a, b),
                    "max_i64({a}, {b}) diverges"
                );
            }
        }
    }

    #[test]
    fn migrated_max_i64_common_cases() {
        assert_eq!(__axon_max_i64(3, 7), 7);
        assert_eq!(__axon_max_i64(7, 3), 7);
        assert_eq!(__axon_max_i64(0, 0), 0);
        assert_eq!(__axon_max_i64(i64::MAX, i64::MIN), i64::MAX);
    }

    // ── sign_i64: matches interp.rs n.signum() ───────────────────────
    #[test]
    fn migrated_sign_i64_matches_interpreter() {
        let oracle = |n: i64| n.signum();
        for &n in &[0i64, 1, -1, 42, -42, i64::MAX, i64::MIN, i64::MAX - 1, i64::MIN + 1] {
            assert_eq!(
                __axon_sign_i64(n),
                oracle(n),
                "sign_i64({n}) diverges from interpreter"
            );
        }
    }

    #[test]
    fn migrated_sign_i64_common_cases() {
        assert_eq!(__axon_sign_i64(5), 1);
        assert_eq!(__axon_sign_i64(-3), -1);
        assert_eq!(__axon_sign_i64(0), 0);
    }

    // ── clamp_i64: matches interp.rs n.max(lo).min(hi) ───────────────
    #[test]
    fn migrated_clamp_i64_matches_interpreter() {
        let oracle = |n: i64, lo: i64, hi: i64| n.max(lo).min(hi);
        let vals = [0i64, 1, -1, 42, -42, 100, i64::MAX, i64::MIN];
        for &n in &vals {
            for &lo in &vals {
                for &hi in &vals {
                    // Skip cases where lo > hi (clamp has undefined semantics for invalid range).
                    if lo > hi { continue; }
                    assert_eq!(
                        __axon_clamp_i64(n, lo, hi),
                        oracle(n, lo, hi),
                        "clamp_i64({n}, {lo}, {hi}) diverges"
                    );
                }
            }
        }
    }

    #[test]
    fn migrated_clamp_i64_common_cases() {
        assert_eq!(__axon_clamp_i64(5, 0, 10), 5);
        assert_eq!(__axon_clamp_i64(-5, 0, 10), 0);
        assert_eq!(__axon_clamp_i64(15, 0, 10), 10);
        assert_eq!(__axon_clamp_i64(0, 0, 0), 0);
    }

    // ── clamp_f64: matches interp.rs n.max(lo).min(hi) ───────────────
    #[test]
    fn migrated_clamp_f64_matches_interpreter() {
        let oracle = |n: f64, lo: f64, hi: f64| n.max(lo).min(hi);
        let vals: [f64; 8] = [0.0, 1.0, -1.0, 42.0, -42.0, 100.0, f64::INFINITY, f64::NEG_INFINITY];
        for &n in &vals {
            for &lo in &vals {
                for &hi in &vals {
                    if lo > hi { continue; }
                    // NaN propagates naturally in both Rust and the LLVM IR version;
                    // assert_eq handles it since NaN != NaN so both sides diverge equally.
                    let result = __axon_clamp_f64(n, lo, hi);
                    let expected = oracle(n, lo, hi);
                    if n.is_nan() || lo.is_nan() || hi.is_nan() {
                        assert!(result.is_nan(), "clamp_f64({n}, {lo}, {hi}) expected NaN");
                    } else {
                        assert_eq!(
                            result, expected,
                            "clamp_f64({n}, {lo}, {hi}) diverges"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn migrated_clamp_f64_common_cases() {
        assert_eq!(__axon_clamp_f64(5.0, 0.0, 10.0), 5.0);
        assert_eq!(__axon_clamp_f64(-5.0, 0.0, 10.0), 0.0);
        assert_eq!(__axon_clamp_f64(15.0, 0.0, 10.0), 10.0);
        assert_eq!(__axon_clamp_f64(0.0, 0.0, 0.0), 0.0);
    }

    // ── pow_i64: matches interp.rs base.wrapping_pow(exp as u32), exp>=0 ─
    #[test]
    fn migrated_pow_i64_matches_interpreter() {
        // The interpreter oracle: base.wrapping_pow(exp as u32).
        // Negative exponent is an abort (not testable here).
        let oracle = |base: i64, exp: i32| base.wrapping_pow(exp as u32);
        let bases = [0i64, 1, -1, 2, -2, 3, 10, -10];
        let exps = [0i32, 1, 2, 3, 4, 10, 31];
        for &base in &bases {
            for &exp in &exps {
                assert_eq!(
                    __axon_pow_i64(base, exp as i64),
                    oracle(base, exp),
                    "pow_i64({base}, {exp}) diverges"
                );
            }
        }
    }

    #[test]
    fn migrated_pow_i64_common_cases() {
        assert_eq!(__axon_pow_i64(2, 0), 1);
        assert_eq!(__axon_pow_i64(2, 10), 1024);
        assert_eq!(__axon_pow_i64(0, 5), 0);
        assert_eq!(__axon_pow_i64(1, 99), 1);
        assert_eq!(__axon_pow_i64(-2, 3), -8);
    }

    // Note: __axon_pow_i64(base, neg_exp) aborts (matching the interpreter's
    // panic-exit for negative exponent). Not unit-tested here because
    // process::abort would kill the test runner.

    // ── Helper: build an AxonStr from a Rust &str ─────────────────────
    fn s(x: &str) -> AxonStr {
        AxonStr { len: x.len() as i64, ptr: x.as_ptr() }
    }

    // ── str_contains: matches interp.rs a.contains(b) ─────────────────
    #[test]
    fn migrated_str_contains_matches_interpreter() {
        let oracle = |a: &str, b: &str| a.contains(b);
        for &a in &["hello world", "", "a", "hello", "abcdef"] {
            for &b in &["world", "hello", "", "o", "xyz", "lo wo"] {
                assert_eq!(
                    __axon_str_contains(s(a), s(b)),
                    oracle(a, b),
                    "str_contains({a:?}, {b:?}) diverges"
                );
            }
        }
    }

    #[test]
    fn migrated_str_contains_common_cases() {
        assert_eq!(__axon_str_contains(s("hello world"), s("world")), true);
        assert_eq!(__axon_str_contains(s("hello world"), s("xyz")), false);
        assert_eq!(__axon_str_contains(s("hello"), s("")), true);      // empty needle always matches
        assert_eq!(__axon_str_contains(s(""), s("")), true);             // empty haystack, empty needle
        assert_eq!(__axon_str_contains(s(""), s("a")), false);           // empty haystack
        // UTF-8 multibyte: "héllo" contains "él"
        assert_eq!(__axon_str_contains(s("héllo"), s("él")), true);
        assert_eq!(__axon_str_contains(s("héllo"), s("xyz")), false);
    }

    // ── str_starts_with: matches interp.rs a.starts_with(b) ───────────
    #[test]
    fn migrated_str_starts_with_matches_interpreter() {
        let oracle = |a: &str, b: &str| a.starts_with(b);
        for &a in &["hello world", "", "a", "hello"] {
            for &b in &["hello", "world", "hell", "xyz", "o wo", ""] {
                assert_eq!(
                    __axon_str_starts_with(s(a), s(b)),
                    oracle(a, b),
                    "str_starts_with({a:?}, {b:?}) diverges"
                );
            }
        }
    }

    #[test]
    fn migrated_str_starts_with_common_cases() {
        assert_eq!(__axon_str_starts_with(s("hello world"), s("hello")), true);
        assert_eq!(__axon_str_starts_with(s("hello world"), s("world")), false);
        assert_eq!(__axon_str_starts_with(s("hello"), s("")), true);     // empty prefix always matches
        // UTF-8: "héllo" starts with "hé"
        assert_eq!(__axon_str_starts_with(s("héllo"), s("hé")), true);
    }

    // ── str_ends_with: matches interp.rs a.ends_with(b) ───────────────
    #[test]
    fn migrated_str_ends_with_matches_interpreter() {
        let oracle = |a: &str, b: &str| a.ends_with(b);
        for &a in &["hello world", "", "a", "hello"] {
            for &b in &["world", "hello", "ld", "xyz", "helo", ""] {
                assert_eq!(
                    __axon_str_ends_with(s(a), s(b)),
                    oracle(a, b),
                    "str_ends_with({a:?}, {b:?}) diverges"
                );
            }
        }
    }

    #[test]
    fn migrated_str_ends_with_common_cases() {
        assert_eq!(__axon_str_ends_with(s("hello world"), s("world")), true);
        assert_eq!(__axon_str_ends_with(s("hello world"), s("hello")), false);
        assert_eq!(__axon_str_ends_with(s("hello"), s("")), true);       // empty suffix always matches
        // UTF-8: "héllo" ends with "llo"
        assert_eq!(__axon_str_ends_with(s("héllo"), s("llo")), true);
    }

    // ── str_index_of: matches interp.rs h.find(needle) ────────────────
    #[test]
    fn migrated_str_index_of_matches_interpreter() {
        let oracle = |h: &str, n: &str| h.find(n).map(|i| i as i64).unwrap_or(-1);
        for &h in &["hello world", "", "a", "ababab", "abcdef"] {
            for &n in &["world", "ab", "", "xyz", "a", "b", "abc", "bcd"] {
                assert_eq!(
                    __axon_str_index_of(s(h), s(n)),
                    oracle(h, n),
                    "str_index_of({h:?}, {n:?}) diverges"
                );
            }
        }
    }

    #[test]
    fn migrated_str_index_of_common_cases() {
        assert_eq!(__axon_str_index_of(s("hello world"), s("world")), 6);
        assert_eq!(__axon_str_index_of(s("hello world"), s("xyz")), -1);
        assert_eq!(__axon_str_index_of(s("hello"), s("")), 0);           // empty needle → 0
        // UTF-8: "héllo" — "l" first at byte 3 (h=0, é=1..2, l=3)
        assert_eq!(__axon_str_index_of(s("héllo"), s("l")), 3);
        assert_eq!(__axon_str_index_of(s("héllo"), s("xyz")), -1);
    }

    // ── str_len: matches interp.rs s.len() ───────────────────────────
    #[test]
    fn migrated_str_len_matches_interpreter() {
        for s_val in &["hello", "", "a", "abc", "hello world", "héllo"] {
            assert_eq!(
                __axon_str_len(s(s_val)),
                s_val.len() as i64,
                "str_len({s_val:?}) diverges"
            );
        }
    }

    #[test]
    fn migrated_str_len_common_cases() {
        assert_eq!(__axon_str_len(s("")), 0);
        assert_eq!(__axon_str_len(s("a")), 1);
        assert_eq!(__axon_str_len(s("hello world")), 11);
        // UTF-8: "héllo" is 6 bytes (é = 2 bytes)
        assert_eq!(__axon_str_len(s("héllo")), 6);
        // Emoji: "🦀" is 4 bytes
        assert_eq!(__axon_str_len(s("🦀")), 4);
    }

    // ── char_at: matches interp.rs s.as_bytes().get(i).unwrap_or(-1) ──
    #[test]
    fn migrated_char_at_matches_interpreter() {
        for s_val in &["hello", "a", "", "héllo", "🦀"] {
            // Test all byte positions and some OOB
            for i in -3i64..(s_val.len() as i64 + 2) {
                let expected = {
                    let bytes = s_val.as_bytes();
                    let i_clamped = i.max(0) as usize;
                    bytes.get(i_clamped).map(|b| *b as i64).unwrap_or(-1)
                };
                assert_eq!(
                    __axon_char_at(s(s_val), i),
                    expected,
                    "char_at({s_val:?}, {i}) diverges"
                );
            }
        }
    }

    #[test]
    fn migrated_char_at_common_cases() {
        // Note: the oracle uses i.max(0) as usize — negative indices clamp to 0
        // (matching interp.rs). Codegen's separate LLVM path does OOB for negative;
        // we match the interpreter as specified in R1 Batch 2.
        assert_eq!(__axon_char_at(s("hello world"), 0), b'h' as i64);   // 104
        assert_eq!(__axon_char_at(s("hello world"), 6), b'w' as i64);   // 119
        assert_eq!(__axon_char_at(s("hello world"), 100), -1);          // OOB
        assert_eq!(__axon_char_at(s("hello world"), -1), b'h' as i64); // -1 clamped to 0 → 'h'=104
        assert_eq!(__axon_char_at(s(""), 0), -1);                       // empty → -1
        // UTF-8: first byte of 'é' is 0xc3 (195) at position 1
        assert_eq!(__axon_char_at(s("héllo"), 1), 195);
        // Emoji: first byte of '🦀' is 0xf0 (240)
        assert_eq!(__axon_char_at(s("🦀"), 0), 240);
    }

    // ── R1 Batch 2b: str_repeat ──────────────────────────────────────────────

    /// Helper to call an out-param str-returning builtin and get the malloc'd
    /// result back as a Rust String.
    fn call_str_ret(f: impl FnOnce(*mut i64, *mut *mut u8)) -> String {
        let mut len: i64 = 0;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        f(&mut len, &mut ptr);
        assert!(!ptr.is_null(), "result pointer must not be null");
        let bytes = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
        let s = String::from_utf8_lossy(bytes).into_owned();
        // Intentionally leak — test harness doesn't free.
        // Runtime callers own the buffer but the interpreter never frees.
        s
    }

    /// str_repeat(s, n): out-param malloc + write roundtrip.
    #[test]
    fn migrated_str_repeat_outparam_roundtrip() {
        // Basic repeat
        let got = call_str_ret(|l, p| __axon_str_repeat(s("ab"), 3, l, p));
        assert_eq!(got, "ababab");

        // Zero repeat → empty
        let got = call_str_ret(|l, p| __axon_str_repeat(s("ab"), 0, l, p));
        assert_eq!(got, "");

        // Negative repeat → empty (interp clamps to 0)
        let got = call_str_ret(|l, p| __axon_str_repeat(s("x"), -5, l, p));
        assert_eq!(got, "");

        // Empty input → empty output
        let got = call_str_ret(|l, p| __axon_str_repeat(s(""), 5, l, p));
        assert_eq!(got, "");

        // Single-char repeat
        let got = call_str_ret(|l, p| __axon_str_repeat(s("x"), 4, l, p));
        assert_eq!(got, "xxxx");
    }

    /// str_repeat: compare against the interpreter oracle.
    #[test]
    fn migrated_str_repeat_matches_interpreter() {
        let cases: Vec<(i64, &str)> = vec![
            (0, "hello"),
            (1, "hello"),
            (2, "hello"),
            (3, "ab"),
            (5, "x"),
            (-1, "hello"),
            (-100, "anything"),
            (0, ""),
            (1, ""),
            (10, "a"),
        ];
        for (n, src) in cases {
            let oracle = src.repeat(n.max(0) as usize);
            let got = call_str_ret(|l, p| __axon_str_repeat(s(src), n, l, p));
            assert_eq!(
                got, oracle,
                "str_repeat({src:?}, {n}) — interpreter says {oracle:?}, rt gave {got:?}"
            );
        }
    }

    // ── R1 Batch 2b: str_slice ──────────────────────────────────────────────

    #[test]
    fn migrated_str_slice_outparam_roundtrip() {
        let got = call_str_ret(|l, p| __axon_str_slice(s("hello"), 1, 4, l, p));
        assert_eq!(got, "ell");

        // end > len → clamp to len
        let got = call_str_ret(|l, p| __axon_str_slice(s("hello"), 0, 100, l, p));
        assert_eq!(got, "hello");

        // start > end → empty
        let got = call_str_ret(|l, p| __axon_str_slice(s("hello"), 3, 1, l, p));
        assert_eq!(got, "");

        // Negative indices clamped to 0
        let got = call_str_ret(|l, p| __axon_str_slice(s("hello"), -5, 2, l, p));
        assert_eq!(got, "he");

        // Zero range
        let got = call_str_ret(|l, p| __axon_str_slice(s("hello"), 2, 2, l, p));
        assert_eq!(got, "");
    }

    #[test]
    fn migrated_str_slice_matches_interpreter() {
        // Unicode: byte-indexed, s.get() returns None if mid-codepoint
        // "héllo" bytes: h(0) é(1,2) l(3) l(4) o(5)
        // 0..2 = "h" + first byte of é = mid-codepoint → None → ""
        let s_val = "héllo";
        let start: i64 = 0;
        let end: i64 = 2;
        let oracle = s_val.get(start.max(0) as usize..end.max(0) as usize).unwrap_or("");
        let got = call_str_ret(|l, p| __axon_str_slice(s(s_val), start, end, l, p));
        assert_eq!(got, oracle, "str_slice unicode mid-codepoint must match");

        // Normal cases — match the interp oracle exactly
        let cases = [
            ("hello", 0i64, 5i64, "hello"),
            ("hello", 0i64, 0i64, ""),
            ("hello", -1i64, 3i64, "hel"),  // negative clamped to 0
            ("hello", 2i64, 100i64, "llo"),  // end clamped to len
        ];
        for (src, start, end, expected) in cases {
            // Interp oracle: end clamped to s.len(), start clamped to 0 then min(end)
            let s_end = (end.max(0) as usize).min(src.len());
            let s_start = (start.max(0) as usize).min(src.len()).min(s_end);
            let s_clamped = src.get(s_start..s_end).unwrap_or("");
            let got = call_str_ret(|l, p| __axon_str_slice(s(src), start, end, l, p));
            assert_eq!(got, expected, "str_slice({src:?}, {start}, {end})");
            assert_eq!(got, s_clamped, "must also match oracle");
        }
    }

    // ── BUG_HUNT #38: str_reverse (char-correct, matches chars().rev()) ──
    #[test]
    fn str_reverse_matches_interpreter_chars_rev() {
        let oracle = |x: &str| -> String { x.chars().rev().collect() };
        for &x in &["", "a", "hello", "héllo", "🦀ab", "naïve", "日本語"] {
            let got = call_str_ret(|l, p| __axon_str_reverse(s(x), l, p));
            assert_eq!(got, oracle(x), "str_reverse({x:?}) must reverse by char, not byte");
            // The result must be valid UTF-8 (the #38 bug produced invalid bytes).
            assert!(got.chars().count() == x.chars().count(), "char count preserved for {x:?}");
        }
    }

    // ── BUG_HUNT #39: str_replace (matches Rust str::replace, incl. empty from) ──
    #[test]
    fn str_replace_matches_interpreter_incl_empty_from() {
        let oracle = |x: &str, f: &str, t: &str| -> String { x.replace(f, t) };
        let cases = [
            ("abc", "", "X"),     // the #39 case: empty from interleaves → "XaXbXcX"
            ("abc", "b", "ZZ"),
            ("aaa", "a", ""),
            ("hello world", "o", "0"),
            ("", "x", "y"),
            ("héllo", "l", "L"),
        ];
        for (x, f, t) in cases {
            let got = call_str_ret(|l, p| __axon_str_replace(s(x), s(f), s(t), l, p));
            assert_eq!(got, oracle(x, f, t), "str_replace({x:?}, {f:?}, {t:?})");
        }
    }

    // ── R1 Batch 3: scalar builtins (abs_i32, min_i32, max_i32, abs_f64) ──

    // ── abs_i32: matches interp.rs i32::abs — computed in i64 then returned ─
    #[test]
    fn migrated_abs_i32_matches_interpreter() {
        // The interpreter oracle: as_int(..).abs() — for i32 values this is
        // (n as i64).abs() which is safe because the input is i32-range.
        let oracle = |n: i32| (n as i64).abs() as i32;
        let sweep = [
            0i32, 1, -1, 42, -42, 7, -7,
            1000, -1000, i32::MAX, i32::MAX - 1, i32::MIN + 1,
        ];
        for &n in &sweep {
            assert_eq!(
                __axon_abs_i32(n),
                oracle(n),
                "abs_i32({n}) diverges from the interpreter oracle"
            );
        }
    }

    #[test]
    fn migrated_abs_i32_common_cases() {
        assert_eq!(__axon_abs_i32(-5), 5);
        assert_eq!(__axon_abs_i32(5), 5);
        assert_eq!(__axon_abs_i32(0), 0);
        assert_eq!(__axon_abs_i32(i32::MIN + 1), i32::MAX);
        // Note: __axon_abs_i32(i32::MIN) aborts (matching the interpreter's
        // panic-exit on negate-overflow). Not unit-tested here because
        // process::abort would kill the test runner — the behavior is asserted
        // by the interpreter-side test that abs_i32(i32::MIN) exits 101.
    }

    // ── min_i32: matches interp.rs a.min(b) ──────────────────────────
    #[test]
    fn migrated_min_i32_matches_interpreter() {
        let oracle = |a: i32, b: i32| a.min(b);
        let vals = [
            0i32, 1, -1, 42, -42, 7, -7,
            1000, -1000, i32::MAX, i32::MIN,
        ];
        for &a in &vals {
            for &b in &vals {
                assert_eq!(
                    __axon_min_i32(a, b),
                    oracle(a, b),
                    "min_i32({a}, {b}) diverges"
                );
            }
        }
    }

    #[test]
    fn migrated_min_i32_common_cases() {
        assert_eq!(__axon_min_i32(3, 7), 3);
        assert_eq!(__axon_min_i32(7, 3), 3);
        assert_eq!(__axon_min_i32(0, 0), 0);
        assert_eq!(__axon_min_i32(i32::MIN, i32::MAX), i32::MIN);
    }

    // ── max_i32: matches interp.rs a.max(b) ──────────────────────────
    #[test]
    fn migrated_max_i32_matches_interpreter() {
        let oracle = |a: i32, b: i32| a.max(b);
        let vals = [
            0i32, 1, -1, 42, -42, 7, -7,
            1000, -1000, i32::MAX, i32::MIN,
        ];
        for &a in &vals {
            for &b in &vals {
                assert_eq!(
                    __axon_max_i32(a, b),
                    oracle(a, b),
                    "max_i32({a}, {b}) diverges"
                );
            }
        }
    }

    #[test]
    fn migrated_max_i32_common_cases() {
        assert_eq!(__axon_max_i32(3, 7), 7);
        assert_eq!(__axon_max_i32(7, 3), 7);
        assert_eq!(__axon_max_i32(0, 0), 0);
        assert_eq!(__axon_max_i32(i32::MAX, i32::MIN), i32::MAX);
    }

    // ── abs_f64: matches interp.rs x.abs() ───────────────────────────
    #[test]
    fn migrated_abs_f64_matches_interpreter() {
        let oracle = |x: f64| x.abs();
        let vals: [f64; 12] = [
            0.0, -0.0, 1.0, -1.0, 3.5, -3.5,
            100.0, -100.0, f64::INFINITY, f64::NEG_INFINITY,
            f64::MAX, f64::MIN,
        ];
        for &x in &vals {
            assert_eq!(
                __axon_abs_f64(x),
                oracle(x),
                "abs_f64({x}) diverges from the interpreter oracle"
            );
        }
    }

    #[test]
    fn migrated_abs_f64_common_cases() {
        assert_eq!(__axon_abs_f64(-3.5), 3.5);
        assert_eq!(__axon_abs_f64(3.5), 3.5);
        assert_eq!(__axon_abs_f64(0.0), 0.0);
        // abs(-0.0) == 0.0 (sign is lost, as expected)
        assert!(__axon_abs_f64(-0.0).is_sign_positive());
    }
}
