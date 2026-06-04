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
//!
//! ## Lint posture (BUG_HUNT #35)
//!
//! Every `#[no_mangle] pub extern "C"` symbol here is a **codegen-emitted-IR
//! entry point**: the LLVM backend synthesizes the call, and the pointer/length
//! arguments come from codegen's own str/array ABI. The unsafe contract lives at
//! that ABI boundary (asserted-by-construction in the emitter), not at the Rust
//! signature — marking each FFI fn `unsafe fn` would not move the obligation to
//! a real human caller (there is none) and would force `unsafe {}` blocks into
//! the generated-call story where they add no safety. So we `allow`
//! `not_unsafe_ptr_arg_deref` and `missing_safety_doc` crate-wide *with this
//! rationale*, and extend the CI clippy gate to `--workspace` so every OTHER
//! lint in this crate is enforced (previously the gate was `-p axon-core` only,
//! hiding ~80 issues). Non-FFI lints are fixed normally, not allowed.
#![allow(clippy::not_unsafe_ptr_arg_deref)]
#![allow(clippy::missing_safety_doc)]

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

    // Non-blocking sibling of `recv`. Retained for completeness alongside the
    // blocking path even though the current codegen only emits blocking recv;
    // a future `select`/poll lowering will call it. (#35: kept, not deleted —
    // it's part of the channel's intended surface, not dead cruft.)
    #[allow(dead_code)]
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
    Arc::into_raw(arc) as *mut c_void
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

// ── Dict (R1c) ──────────────────────────────────────────────────────────────
//
// A string-keyed, tagged-value hash map — the native realization of the
// interpreter's dynamically-typed `Dict` (`Rc<RefCell<BTreeMap<String,Value>>>`).
// Codegen values are statically typed, so each stored value carries a TAG plus
// an 8-byte payload, exactly mirroring how the interpreter's `Value` enum
// discriminates int/float/str. The dict is an opaque `*mut c_void` handle to an
// `Arc<Mutex<HashMap<…>>>`, the same ownership model as channels above (created,
// mutated, dropped through C-ABI externs the codegen calls by name — never
// inline IR). See `governance/specs/R1c-dict-runtime.md`.
//
// Tag values (must match codegen's call-site dispatch): 0=Int, 1=Float, 2=Str.
// Payload: an i64 for Int, the f64 bit-pattern for Float, or a pointer to a
// heap `String` (boxed) for Str — the dict owns the Str copy.

// BTreeMap (NOT HashMap) so key-iteration order matches the interpreter's Dict
// (`Rc<RefCell<BTreeMap>>`) — dict_keys/values/to_pairs return entries in sorted
// key order, which the parity harnesses depend on.
use std::collections::BTreeMap as StdMap;

#[derive(Clone)]
enum DictVal {
    Int(i64),
    Float(f64),
    Str(String),
}

struct Dict {
    map: Mutex<StdMap<String, DictVal>>,
}

/// Create an empty dict. Opaque handle to an `Arc<Dict>`.
#[no_mangle]
pub extern "C" fn __axon_dict_new() -> *mut c_void {
    let arc = Arc::new(Dict { map: Mutex::new(StdMap::new()) });
    Arc::into_raw(arc) as *mut c_void
}

/// Merge two dicts into a FRESH dict (neither input is mutated), matching the
/// interpreter: start from d1's entries, then overlay d2's (so d2 wins on a key
/// conflict). Returns a new opaque handle. A null input is treated as empty.
#[no_mangle]
pub extern "C" fn __axon_dict_merge(d1: *mut c_void, d2: *mut c_void) -> *mut c_void {
    let mut out: StdMap<String, DictVal> = StdMap::new();
    if !d1.is_null() {
        let a = unsafe { dict_borrow(d1) };
        for (k, v) in a.map.lock().unwrap().iter() {
            out.insert(k.clone(), v.clone());
        }
    }
    if !d2.is_null() {
        let b = unsafe { dict_borrow(d2) };
        for (k, v) in b.map.lock().unwrap().iter() {
            out.insert(k.clone(), v.clone());
        }
    }
    let arc = Arc::new(Dict { map: Mutex::new(out) });
    Arc::into_raw(arc) as *mut c_void
}

// Borrow the Arc<Dict> from a raw handle WITHOUT consuming it (the caller still
// owns the handle). Returns a cloned Arc whose drop is balanced by into_raw.
#[inline]
unsafe fn dict_borrow(d: *mut c_void) -> Arc<Dict> {
    let arc = Arc::from_raw(d as *const Dict);
    let clone = Arc::clone(&arc);
    let _ = Arc::into_raw(arc); // keep the caller's handle alive
    clone
}

#[cfg(not(target_arch = "wasm32"))]
fn dict_key_of(key: AxonStr) -> String {
    unsafe { key.as_str() }.to_string()
}

fn dict_val_of(tag: i64, payload: i64, payload_str: *const u8, payload_str_len: i64) -> DictVal {
    match tag {
        1 => DictVal::Float(f64::from_bits(payload as u64)),
        2 => {
            // Str payload: reconstruct from (ptr,len) and own a copy.
            if payload_str.is_null() || payload_str_len < 0 {
                DictVal::Str(String::new())
            } else {
                let s = unsafe { std::slice::from_raw_parts(payload_str, payload_str_len as usize) };
                DictVal::Str(String::from_utf8_lossy(s).into_owned())
            }
        }
        _ => DictVal::Int(payload),
    }
}

/// Set `key` → tagged value. Str payloads arrive as (ptr,len); int/float in
/// `payload`. The dict owns its key + Str-value copies.
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn __axon_dict_set(
    d: *mut c_void,
    key: AxonStr,
    tag: i64,
    payload: i64,
    payload_str: *const u8,
    payload_str_len: i64,
) {
    if d.is_null() { return; }
    let dict = unsafe { dict_borrow(d) };
    let k = dict_key_of(key);
    let v = dict_val_of(tag, payload, payload_str, payload_str_len);
    dict.map.lock().unwrap().insert(k, v);
}

/// Look up `key`. Returns 1 if found (writing the tag + payload out-params), 0
/// if absent. For a Str value, `*out_payload` is set to a freshly-malloc'd,
/// null-terminated copy's pointer and `*out_str_len` to its byte length.
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn __axon_dict_get(
    d: *mut c_void,
    key: AxonStr,
    out_tag: *mut i64,
    out_payload: *mut i64,
    out_str_len: *mut i64,
) -> bool {
    if d.is_null() { return false; }
    let dict = unsafe { dict_borrow(d) };
    let k = dict_key_of(key);
    let guard = dict.map.lock().unwrap();
    match guard.get(&k) {
        None => false,
        Some(v) => {
            unsafe {
                match v {
                    DictVal::Int(n) => { *out_tag = 0; *out_payload = *n; }
                    DictVal::Float(f) => { *out_tag = 1; *out_payload = f.to_bits() as i64; }
                    DictVal::Str(s) => {
                        *out_tag = 2;
                        let len = s.len();
                        let p = libc_malloc(len + 1);
                        std::ptr::copy_nonoverlapping(s.as_ptr(), p, len);
                        *p.add(len) = 0;
                        *out_payload = p as i64;
                        if !out_str_len.is_null() { *out_str_len = len as i64; }
                    }
                }
            }
            true
        }
    }
}

/// Whether `key` is present.
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn __axon_dict_has(d: *mut c_void, key: AxonStr) -> bool {
    if d.is_null() { return false; }
    let dict = unsafe { dict_borrow(d) };
    let k = dict_key_of(key);
    let has = dict.map.lock().unwrap().contains_key(&k);
    has
}

/// Number of entries.
#[no_mangle]
pub extern "C" fn __axon_dict_len(d: *mut c_void) -> i64 {
    if d.is_null() { return 0; }
    let dict = unsafe { dict_borrow(d) };
    let n = dict.map.lock().unwrap().len() as i64;
    n
}

/// Remove `key`, returning whether it was present (writing the prior value's
/// tag + payload to the out-params, same convention as `__axon_dict_get`). For
/// a Str value, `*out_payload` is a freshly-malloc'd null-terminated copy.
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn __axon_dict_remove(
    d: *mut c_void,
    key: AxonStr,
    out_tag: *mut i64,
    out_payload: *mut i64,
    out_str_len: *mut i64,
) -> bool {
    if d.is_null() { return false; }
    let dict = unsafe { dict_borrow(d) };
    let k = dict_key_of(key);
    let removed = dict.map.lock().unwrap().remove(&k);
    match removed {
        None => false,
        Some(v) => {
            unsafe {
                match v {
                    DictVal::Int(n) => { *out_tag = 0; *out_payload = n; }
                    DictVal::Float(f) => { *out_tag = 1; *out_payload = f.to_bits() as i64; }
                    DictVal::Str(s) => {
                        *out_tag = 2;
                        let len = s.len();
                        let p = libc_malloc(len + 1);
                        std::ptr::copy_nonoverlapping(s.as_ptr(), p, len);
                        *p.add(len) = 0;
                        *out_payload = p as i64;
                        if !out_str_len.is_null() { *out_str_len = len as i64; }
                    }
                }
            }
            true
        }
    }
}

/// Atomically bump an i64 counter at `key`: initialize to 1 if absent (or if
/// the existing value is non-Int — matching the interpreter's get-or-0 + 1),
/// else previous+1. Returns the new value. The common frequency-table primitive.
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn __axon_dict_inc(d: *mut c_void, key: AxonStr) -> i64 {
    if d.is_null() { return 0; }
    let dict = unsafe { dict_borrow(d) };
    let k = dict_key_of(key);
    let mut guard = dict.map.lock().unwrap();
    let cur = match guard.get(&k) {
        Some(DictVal::Int(n)) => *n,
        _ => 0,
    };
    let next = cur + 1;
    guard.insert(k, DictVal::Int(next));
    next
}

/// Collect the keys into a fresh `[str]` slice (BTreeMap order). Writes the
/// slice's `{len, data}` via out-params: `*out_len` = #keys, `*out_data` = a
/// malloc'd array of `AxonStr` structs (each key's bytes also malloc'd +
/// null-terminated). The codegen assembles the `{i64,ptr}` slice from these.
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn __axon_dict_keys(
    d: *mut c_void,
    out_len: *mut i64,
    out_data: *mut *mut u8,
) {
    if d.is_null() {
        unsafe { *out_len = 0; *out_data = std::ptr::null_mut(); }
        return;
    }
    let dict = unsafe { dict_borrow(d) };
    let guard = dict.map.lock().unwrap();
    let n = guard.len();
    if n == 0 {
        unsafe { *out_len = 0; *out_data = std::ptr::null_mut(); }
        return;
    }
    // Array of AxonStr ({i64,ptr} = 16 bytes each).
    let elem_size = std::mem::size_of::<AxonStr>();
    let arr = unsafe { libc_malloc(elem_size * n) } as *mut AxonStr;
    for (i, k) in guard.keys().enumerate() {
        let len = k.len();
        let buf = unsafe {
            let p = libc_malloc(len + 1);
            std::ptr::copy_nonoverlapping(k.as_ptr(), p, len);
            *p.add(len) = 0;
            p
        };
        unsafe { *arr.add(i) = AxonStr { len: len as i64, ptr: buf }; }
    }
    unsafe {
        *out_len = n as i64;
        *out_data = arr as *mut u8;
    }
}

/// Collect the values into a fresh `[i64]` slice (BTreeMap key order). Writes
/// the slice's `{len, data}` via out-params: `*out_len` = #values, `*out_data`
/// = a malloc'd array of `i64`. v1 reinterprets each value as `i64` with the
/// SAME convention as `__axon_dict_get` (Int→n, Float→bits, Str→ptr) — the
/// int-valued case is the supported surface, mirroring `dict_get`. The codegen
/// assembles the `{i64,ptr}` slice from these out-params.
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn __axon_dict_values(
    d: *mut c_void,
    out_len: *mut i64,
    out_data: *mut *mut u8,
) {
    if d.is_null() {
        unsafe { *out_len = 0; *out_data = std::ptr::null_mut(); }
        return;
    }
    let dict = unsafe { dict_borrow(d) };
    let guard = dict.map.lock().unwrap();
    let n = guard.len();
    if n == 0 {
        unsafe { *out_len = 0; *out_data = std::ptr::null_mut(); }
        return;
    }
    let arr = unsafe { libc_malloc(std::mem::size_of::<i64>() * n) } as *mut i64;
    for (i, v) in guard.values().enumerate() {
        let payload: i64 = match v {
            DictVal::Int(x) => *x,
            DictVal::Float(f) => f.to_bits() as i64,
            DictVal::Str(s) => s.as_ptr() as i64,
        };
        unsafe { *arr.add(i) = payload; }
    }
    unsafe {
        *out_len = n as i64;
        *out_data = arr as *mut u8;
    }
}

/// Split `s` on the separator `sep` into a fresh `[str]` slice, matching the
/// interpreter's `str_split` exactly: an empty `sep` yields the single element
/// `[s]`, otherwise Rust's `str::split` (so "a,,b".split(",") = ["a","","b"]).
/// Writes the slice's `{len, data}` via out-params: `*out_len` = #parts,
/// `*out_data` = a malloc'd array of `AxonStr` (each part's bytes malloc'd +
/// null-terminated). Same shape as `__axon_dict_keys`; codegen assembles the
/// `{i64,ptr}` slice from these.
#[cfg(not(target_arch = "wasm32"))]
#[no_mangle]
pub extern "C" fn __axon_str_split(
    s: AxonStr,
    sep: AxonStr,
    out_len: *mut i64,
    out_data: *mut *mut u8,
) {
    let s = unsafe { s.as_str() };
    let sep = unsafe { sep.as_str() };
    let parts: Vec<&str> = if sep.is_empty() { vec![s] } else { s.split(sep).collect() };
    let n = parts.len();
    if n == 0 {
        unsafe { *out_len = 0; *out_data = std::ptr::null_mut(); }
        return;
    }
    let elem_size = std::mem::size_of::<AxonStr>();
    let arr = unsafe { libc_malloc(elem_size * n) } as *mut AxonStr;
    for (i, part) in parts.iter().enumerate() {
        let len = part.len();
        let buf = unsafe {
            let p = libc_malloc(len + 1);
            std::ptr::copy_nonoverlapping(part.as_ptr(), p, len);
            *p.add(len) = 0;
            p
        };
        unsafe { *arr.add(i) = AxonStr { len: len as i64, ptr: buf }; }
    }
    unsafe {
        *out_len = n as i64;
        *out_data = arr as *mut u8;
    }
}

/// Drop the dict handle (Arc decref).
#[no_mangle]
pub extern "C" fn __axon_dict_drop(d: *mut c_void) {
    if !d.is_null() {
        unsafe { drop(Arc::from_raw(d as *const Dict)) };
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
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_str_contains(a: AxonStr, b: AxonStr) -> bool {
    let a = unsafe { a.as_str() };
    let b = unsafe { b.as_str() };
    a.contains(b)
}
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_str_contains(a_len: i64, a_ptr: *const u8, b_len: i64, b_ptr: *const u8) -> bool {
    let a = unsafe { AxonStr { len: a_len, ptr: a_ptr }.as_str() };
    let b = unsafe { AxonStr { len: b_len, ptr: b_ptr }.as_str() };
    a.contains(b)
}

/// Check if haystack starts with prefix.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `a.starts_with(b)`.
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_str_starts_with(a: AxonStr, b: AxonStr) -> bool {
    let a = unsafe { a.as_str() };
    let b = unsafe { b.as_str() };
    a.starts_with(b)
}
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_str_starts_with(a_len: i64, a_ptr: *const u8, b_len: i64, b_ptr: *const u8) -> bool {
    let a = unsafe { AxonStr { len: a_len, ptr: a_ptr }.as_str() };
    let b = unsafe { AxonStr { len: b_len, ptr: b_ptr }.as_str() };
    a.starts_with(b)
}

/// Check if haystack ends with suffix.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `a.ends_with(b)`.
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_str_ends_with(a: AxonStr, b: AxonStr) -> bool {
    let a = unsafe { a.as_str() };
    let b = unsafe { b.as_str() };
    a.ends_with(b)
}
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_str_ends_with(a_len: i64, a_ptr: *const u8, b_len: i64, b_ptr: *const u8) -> bool {
    let a = unsafe { AxonStr { len: a_len, ptr: a_ptr }.as_str() };
    let b = unsafe { AxonStr { len: b_len, ptr: b_ptr }.as_str() };
    a.ends_with(b)
}

/// Byte index of first occurrence of needle in haystack, or -1 if not found.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `h.find(needle).map(|i| i as i64).unwrap_or(-1)`.
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_str_index_of(hay: AxonStr, needle: AxonStr) -> i64 {
    let hay = unsafe { hay.as_str() };
    let needle = unsafe { needle.as_str() };
    hay.find(needle).map(|i| i as i64).unwrap_or(-1)
}
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_str_index_of(hay_len: i64, hay_ptr: *const u8, needle_len: i64, needle_ptr: *const u8) -> i64 {
    let hay = unsafe { AxonStr { len: hay_len, ptr: hay_ptr }.as_str() };
    let needle = unsafe { AxonStr { len: needle_len, ptr: needle_ptr }.as_str() };
    hay.find(needle).map(|i| i as i64).unwrap_or(-1)
}

/// Byte length of the string.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `s.len() as i64` (byte length, not char count).
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_str_len(s: AxonStr) -> i64 {
    let s = unsafe { s.as_str() };
    s.len() as i64
}
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_str_len(s_len: i64, s_ptr: *const u8) -> i64 {
    let s = unsafe { AxonStr { len: s_len, ptr: s_ptr }.as_str() };
    s.len() as i64
}

/// Byte value at index i, or -1 if out of bounds.
///
/// Migrated from inline LLVM IR in `codegen/builtins.rs` (R1,
/// `governance/specs/R1-codegen-build-unblock.md`, Batch 2). Matches the
/// interpreter oracle: `s.as_bytes().get(i.max(0) as usize).map(|b| *b as i64).unwrap_or(-1)`.
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_char_at(s: AxonStr, i: i64) -> i64 {
    let s = unsafe { s.as_str() };
    let i = i.max(0) as usize;
    s.as_bytes().get(i).map(|b| *b as i64).unwrap_or(-1)
}
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_char_at(s_len: i64, s_ptr: *const u8, i: i64) -> i64 {
    let s = unsafe { AxonStr { len: s_len, ptr: s_ptr }.as_str() };
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
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_exec(
    cmd_len: i64,
    cmd_ptr: *const u8,
    args_ptr: *const AxonStr,
    args_count: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    __axon_exec_impl(unsafe { AxonStr { len: cmd_len, ptr: cmd_ptr }.as_str() }, args_ptr, args_count, out_len, out_ptr)
}
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_exec(
    cmd: AxonStr,
    args_ptr: *const AxonStr,
    args_count: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    __axon_exec_impl(unsafe { cmd.as_str() }, args_ptr, args_count, out_len, out_ptr)
}
fn __axon_exec_impl(
    cmd_s: &str,
    args_ptr: *const AxonStr,
    args_count: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
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
#[cfg(not(target_arch = "wasm32"))]
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
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_str_repeat(
    s_len: i64,
    s_ptr: *const u8,
    n: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let src = unsafe { AxonStr { len: s_len, ptr: s_ptr }.as_str() };
    let count = n.max(0) as usize;
    let result = src.repeat(count);
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

// ── R1 Batch 2b: str_slice ────────────────────────────────────────────────────
/// `str_slice(s, start, end)` — byte-indexed slice of `s`.
/// Clamps start to [0, len], end to [start, len]; returns "" if byte range
/// crosses a UTF-8 boundary (s.get returns None).
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
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
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_str_slice(
    s_len: i64,
    s_ptr: *const u8,
    start: i64,
    end: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let src = unsafe { AxonStr { len: s_len, ptr: s_ptr }.as_str() };
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
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_parse_int_err(
    input_len: i64,
    input_ptr: *const u8,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    __axon_parse_int_err_impl(unsafe { AxonStr { len: input_len, ptr: input_ptr }.as_str() }, out_len, out_ptr)
}
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_parse_int_err(
    input: AxonStr,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    __axon_parse_int_err_impl(unsafe { input.as_str() }, out_len, out_ptr)
}
fn __axon_parse_int_err_impl(
    src: &str,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
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

// ── BUG_HUNT #22: parse_int_radix (radix-aware integer parse) ─────────────────
/// `parse_int_radix(s, base) -> Result<i64, str>`. The whole parse lives here
/// (the delegate-to-rt pattern, like #37/#38/#39) so codegen only assembles the
/// Result struct. On success writes `*out_ok = 1` and `*out_val = n`. On failure
/// writes `*out_ok = 0` and the error message via `out_len`/`out_ptr` (the same
/// ±/buffer convention `write_str_out` uses). Semantics are byte-identical to
/// the interpreter (`interp.rs` `parse_int_radix`): a matching radix prefix
/// (`0x`/`0o`/`0b`) is stripped, a leading `-` is preserved, an out-of-range
/// base or bad digit is a recoverable Err — never a panic.
#[no_mangle]
#[cfg(target_arch = "wasm32")]
pub extern "C" fn __axon_parse_int_radix(
    s_len: i64,
    s_ptr: *const u8,
    base: i64,
    out_ok: *mut i64,
    out_val: *mut i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    __axon_parse_int_radix_impl(unsafe { AxonStr { len: s_len, ptr: s_ptr }.as_str() }, base, out_ok, out_val, out_len, out_ptr)
}
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_parse_int_radix(
    s: AxonStr,
    base: i64,
    out_ok: *mut i64,
    out_val: *mut i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    __axon_parse_int_radix_impl(unsafe { s.as_str() }, base, out_ok, out_val, out_len, out_ptr)
}
#[allow(clippy::too_many_arguments)]
fn __axon_parse_int_radix_impl(
    src: &str,
    base: i64,
    out_ok: *mut i64,
    out_val: *mut i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let set_ok = |n: i64| unsafe {
        *out_ok = 1;
        *out_val = n;
    };
    let set_err = |msg: &str| unsafe {
        *out_ok = 0;
        *out_val = 0;
        write_str_out(msg, out_len, out_ptr);
    };

    if !(2..=36).contains(&base) {
        set_err(&format!("parse_int_radix: base must be 2..=36, got {base}"));
        return;
    }
    let t = src.trim();
    let (sign, rest) = match t.strip_prefix('-') {
        Some(r) => ("-", r),
        None => ("", t),
    };
    let lower = rest.to_ascii_lowercase();
    let digits = match base {
        16 if lower.starts_with("0x") => &rest[2..],
        8 if lower.starts_with("0o") => &rest[2..],
        2 if lower.starts_with("0b") => &rest[2..],
        _ => rest,
    };
    let normalized = format!("{sign}{digits}");
    match i64::from_str_radix(&normalized, base as u32) {
        Ok(n) => set_ok(n),
        Err(_) => set_err(&format!("could not parse `{src}` as a base-{base} integer")),
    }
}

// ── BUG_HUNT #38: str_reverse (char-correct, not byte-reverse) ────────────────
/// `str_reverse(s)` — reverse by Unicode scalar (char), matching the
/// interpreter (`chars().rev()`). The old inline codegen reversed BYTES, which
/// mangles any multibyte UTF-8 (`str_reverse("héllo")` → invalid bytes); this
/// is the I-2-canonical implementation codegen now calls instead.
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_str_reverse(
    s: AxonStr,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let src = unsafe { s.as_str() };
    let result: String = src.chars().rev().collect();
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

// wasm32 ABI bridge (R7): LLVM (codegen) expands a by-value `{i64 len, ptr}`
// struct arg into SCALARS — so codegen calls `__axon_str_reverse(i64 len,
// i32 ptr, …)`. rustc, by contrast, would pass `AxonStr` INDIRECTLY on wasm32
// (a single i32 pointer), so the by-value signature above mismatches the
// codegen call (`function signature mismatch` → trap). On wasm we therefore
// declare the EXPANDED scalar form that matches codegen exactly, and rebuild
// the AxonStr inside. (Native keeps the by-value form, which agrees there.)
#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn __axon_str_reverse(
    s_len: i64,
    s_ptr: *const u8,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let s = AxonStr { len: s_len, ptr: s_ptr };
    let src = unsafe { s.as_str() };
    let result: String = src.chars().rev().collect();
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

/// `str_digits_only(s)` — keep only the ASCII digit chars, matching the
/// interpreter (`chars().filter(is_ascii_digit)`). Same str→str out-param ABI as
/// str_reverse; native takes AxonStr by value, wasm32 takes the expanded scalar
/// form (see the str_reverse ABI note above).
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_str_digits_only(
    s: AxonStr,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let src = unsafe { s.as_str() };
    let result: String = src.chars().filter(|c| c.is_ascii_digit()).collect();
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn __axon_str_digits_only(
    s_len: i64,
    s_ptr: *const u8,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let s = AxonStr { len: s_len, ptr: s_ptr };
    let src = unsafe { s.as_str() };
    let result: String = src.chars().filter(|c| c.is_ascii_digit()).collect();
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

/// `str_join(parts, sep)` — join a `[str]` slice with `sep`, matching the
/// interpreter (Rust `slice::join`). The slice is passed as two SCALARS
/// (`slice_len`, `slice_data` → an array of `AxonStr`, the same element layout
/// str_split produces) rather than a by-value struct, sidestepping the
/// slice-struct ABI; `sep` keeps the str by-value (native) / scalar (wasm) form.
/// `slice_data` may be null when `slice_len == 0` (empty slice → "").
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
pub extern "C" fn __axon_str_join(
    slice_len: i64,
    slice_data: *const AxonStr,
    sep: AxonStr,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let sep = unsafe { sep.as_str() };
    let result = unsafe { join_axonstr_slice(slice_len, slice_data, sep) };
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

#[cfg(target_arch = "wasm32")]
#[no_mangle]
pub extern "C" fn __axon_str_join(
    slice_len: i64,
    slice_data: *const AxonStr,
    sep_len: i64,
    sep_ptr: *const u8,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let sep = unsafe { AxonStr { len: sep_len, ptr: sep_ptr }.as_str() };
    let result = unsafe { join_axonstr_slice(slice_len, slice_data, sep) };
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

/// Shared join body: read `slice_len` `AxonStr` elements from `slice_data` and
/// join them with `sep`. Extracted so the native + wasm entry points can't drift.
unsafe fn join_axonstr_slice(slice_len: i64, slice_data: *const AxonStr, sep: &str) -> String {
    if slice_len <= 0 || slice_data.is_null() {
        return String::new();
    }
    let n = slice_len as usize;
    let mut parts: Vec<&str> = Vec::with_capacity(n);
    for i in 0..n {
        let elem = &*slice_data.add(i);
        parts.push(elem.as_str());
    }
    parts.join(sep)
}

// ── BUG_HUNT #39: str_replace (matches Rust str::replace) ─────────────────────
/// `str_replace(s, from, to)` — replace every occurrence of `from` with `to`,
/// matching the interpreter (Rust `str::replace`). In particular an empty
/// `from` interleaves `to` between every char (and at both ends), e.g.
/// `str_replace("abc", "", "X")` → `"XaXbXcX"`. The old inline codegen skipped
/// the empty-`from` case (returned `s` unchanged) — a silent divergence.
#[no_mangle]
#[cfg(not(target_arch = "wasm32"))]
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
#[no_mangle]
#[cfg(target_arch = "wasm32")]
#[allow(clippy::too_many_arguments)]
pub extern "C" fn __axon_str_replace(
    s_len: i64,
    s_ptr: *const u8,
    from_len: i64,
    from_ptr: *const u8,
    to_len: i64,
    to_ptr: *const u8,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    let src = unsafe { AxonStr { len: s_len, ptr: s_ptr }.as_str() };
    let from_s = unsafe { AxonStr { len: from_len, ptr: from_ptr }.as_str() };
    let to_s = unsafe { AxonStr { len: to_len, ptr: to_ptr }.as_str() };
    let result = src.replace(from_s, to_s);
    unsafe { write_str_out(&result, out_len, out_ptr) }
}

// ── ASI Layer-3: @[verify] runtime enforcement ────────────────────────────────

/// Exit code for an `@[verify]` / deploy-gate rejection. Must match the
/// interpreter's `interp::VERIFY_FAILED_EXIT_CODE` (axon-rt has no dependency
/// on axon-core, so the value is duplicated, not imported) — BUG_HUNT #26.
pub const VERIFY_FAILED_EXIT_CODE: i32 = 3;

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

    // ── parse_int_radix (BUG_HUNT #22): matches interp.rs semantics ───
    /// Call __axon_parse_int_radix and decode the out-params into a
    /// `Result<i64, String>` mirroring the Axon `Result<i64, str>`.
    fn parse_radix(input: &str, base: i64) -> Result<i64, String> {
        let mut ok: i64 = -1;
        let mut val: i64 = 0;
        let mut len: i64 = 0;
        let mut ptr: *mut u8 = std::ptr::null_mut();
        __axon_parse_int_radix(s(input), base, &mut ok, &mut val, &mut len, &mut ptr);
        if ok == 1 {
            Ok(val)
        } else {
            let msg = unsafe {
                String::from_utf8_lossy(std::slice::from_raw_parts(ptr, len as usize)).into_owned()
            };
            Err(msg)
        }
    }

    #[test]
    fn parse_int_radix_matches_interpreter_semantics() {
        // Prefix-stripping per base.
        assert_eq!(parse_radix("0x1F", 16), Ok(31));
        assert_eq!(parse_radix("1F", 16), Ok(31));
        assert_eq!(parse_radix("0b1010", 2), Ok(10));
        assert_eq!(parse_radix("0o17", 8), Ok(15));
        // Sign preserved.
        assert_eq!(parse_radix("-2A", 16), Ok(-42));
        // Whitespace trimmed.
        assert_eq!(parse_radix("  ff  ", 16), Ok(255));
        // Base 10 rejects hex digits.
        assert!(parse_radix("ff", 10).is_err());
        // Bad digit for the base → Err naming the input, not a panic.
        let e = parse_radix("zzz", 16).unwrap_err();
        assert!(e.contains("zzz") && e.contains("base-16"), "got {e}");
        // Out-of-range base → Err, not a panic.
        let e = parse_radix("10", 99).unwrap_err();
        assert!(e.contains("base must be 2..=36"), "got {e}");
        let e = parse_radix("10", 1).unwrap_err();
        assert!(e.contains("base must be 2..=36"), "got {e}");
        // The 0x prefix is NOT stripped for a non-16 base (it's just bad digits).
        assert!(parse_radix("0x1F", 10).is_err());
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
        assert!(__axon_str_contains(s("hello world"), s("world")));
        assert!(!(__axon_str_contains(s("hello world"), s("xyz"))));
        assert!(__axon_str_contains(s("hello"), s("")));      // empty needle always matches
        assert!(__axon_str_contains(s(""), s("")));             // empty haystack, empty needle
        assert!(!(__axon_str_contains(s(""), s("a"))));           // empty haystack
        // UTF-8 multibyte: "héllo" contains "él"
        assert!(__axon_str_contains(s("héllo"), s("él")));
        assert!(!(__axon_str_contains(s("héllo"), s("xyz"))));
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
        assert!(__axon_str_starts_with(s("hello world"), s("hello")));
        assert!(!(__axon_str_starts_with(s("hello world"), s("world"))));
        assert!(__axon_str_starts_with(s("hello"), s("")));     // empty prefix always matches
        // UTF-8: "héllo" starts with "hé"
        assert!(__axon_str_starts_with(s("héllo"), s("hé")));
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
        assert!(__axon_str_ends_with(s("hello world"), s("world")));
        assert!(!(__axon_str_ends_with(s("hello world"), s("hello"))));
        assert!(__axon_str_ends_with(s("hello"), s("")));       // empty suffix always matches
        // UTF-8: "héllo" ends with "llo"
        assert!(__axon_str_ends_with(s("héllo"), s("llo")));
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
