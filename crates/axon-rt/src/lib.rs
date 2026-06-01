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
