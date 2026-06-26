//! R22 — domain-interop native modules: `native::modbus` (industrial),
//! `native::fhir` (healthtech), `native::fix` (fintech).
//!
//! Built on the R13 native-FFI machinery (`crates/axon-core/src/native.rs`),
//! these follow the `axon-gfx-mock` template EXACTLY: a shim crate exposing an
//! in-process dispatch, a per-module slab handle table for connections/sessions
//! (unforgeable index, never a raw pointer — I-4/I-11), and registry rows in
//! `native.rs` that wire capability gating (E1004), effect rows, infer, checker
//! and the borrow checker automatically.
//!
//! Unlike `gfx`, these are NOT lowered to native codegen: `modbus`/`fhir` do
//! live network I/O and are E0910-refused at codegen (the `host_await`/native
//! precedent — sound-by-refusal, interp-only). `fix` is a pure codec but is
//! kept interp-only too for a uniform domain-module story. So this crate
//! exposes NO `#[no_mangle] extern "C"` symbols and is not linked into axon-rt.
//!
//! ## The boundary value/arg layer (generalizes `GfxArg`/`GfxValue`)
//!
//! The R13 `gfx` shim only needed Int/Float/Str args and Unit/Int/Handle
//! returns. These modules additionally need **str returns** (FHIR JSON, FIX
//! field values) and **`[i64]` returns** (Modbus register reads), so this crate
//! defines its own [`DomainArg`]/[`DomainValue`] enums spanning the full
//! representable set. The interpreter marshals `Value`→`DomainArg` and
//! `DomainValue`→`Value` at the boundary, exactly as it does for gfx.
//!
//! ## Handles are unforgeable slab indices (I-4/I-11)
//!
//! Every connection/session is a generation-tracked slab slot. A
//! forged/stale/out-of-range index resolves to an absent slot → a graceful
//! [`DomainResult::Err`] (mapped to exit-101 by the interpreter), NEVER a wild
//! deref or host abort — the table lives entirely inside this Rust crate.

pub mod fhir;
pub mod fix;
pub mod modbus;

/// A value a domain native call returns. Engine-independent (the interpreter
/// wraps it in its own `Value`). Spans the full R13 representable RETURN set
/// plus `[i64]` (Modbus register reads) and `Str` (FHIR/FIX).
#[derive(Debug, Clone, PartialEq)]
pub enum DomainValue {
    Unit,
    Int(i64),
    Str(String),
    /// A `[i64]` return (Modbus holding-register / coil reads).
    IntArray(Vec<i64>),
    /// A freshly-allocated handle `(name, payload)`; the caller wraps it.
    Handle {
        name: &'static str,
        payload: i64,
    },
}

/// A marshalled argument crossing into a domain dispatcher. The R13
/// representable set: scalar, str, `[i64]`, and a handle's slab index.
#[derive(Debug, Clone, PartialEq)]
pub enum DomainArg {
    Int(i64),
    Float(f64),
    Str(String),
    IntArray(Vec<i64>),
    /// A handle `(tag, payload)`: nominal tag + slab index (both cross so the
    /// shim can reject a tag/table mismatch beneath the static E1802 check).
    Handle {
        tag: i64,
        payload: i64,
    },
}

impl DomainArg {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            DomainArg::Int(n) => Some(*n),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            DomainArg::Str(s) => Some(s),
            _ => None,
        }
    }
    pub fn as_handle_payload(&self) -> Option<i64> {
        match self {
            DomainArg::Handle { payload, .. } => Some(*payload),
            _ => None,
        }
    }
}

/// Outcome of a domain native call: a value, or a graceful error string (NEVER
/// a host abort — I-4). The interpreter maps `Err` to a graceful exit-101 panic.
pub type DomainResult = Result<DomainValue, String>;

/// Frozen nominal handle tags (the `tag` half of the `{i64 tag, i64 payload}`
/// Handle layout). One per handle type across all domain modules; frozen so
/// they never collide and never get renumbered.
pub fn tag_for(name: &str) -> i64 {
    match name {
        "Conn" => 0x6d6f_6401,     // modbus::Conn
        "FhirConn" => 0x6668_7201, // fhir::FhirConn
        "FixMsg" => 0x6669_7801,   // fix::FixMsg
        _ => -1,
    }
}

/// Map a domain `M::fn` call to its in-process dispatcher. Returns `None` for a
/// non-domain module (the caller falls through to its other native backends).
pub fn dispatch(
    module: &str,
    registry: &Registry,
    fnname: &str,
    args: &[DomainArg],
) -> DomainResult {
    match module {
        "modbus" => registry.modbus.borrow_mut().dispatch(fnname, args),
        "fhir" => registry.fhir.borrow_mut().dispatch(fnname, args),
        "fix" => registry.fix.borrow_mut().dispatch(fnname, args),
        _ => Err(format!("native module `{module}` has no domain backend")),
    }
}

use std::cell::RefCell;

/// The per-`Interp` domain backend state — one slab table per module. Held
/// interior-mutable like the gfx mock.
#[derive(Debug, Default)]
pub struct Registry {
    pub modbus: RefCell<modbus::ModbusBackend>,
    pub fhir: RefCell<fhir::FhirBackend>,
    pub fix: RefCell<fix::FixBackend>,
}

impl Registry {
    pub fn new() -> Self {
        Registry::default()
    }
}

/// A generation-tracked slab slot for a domain handle table. `live = false` ⇒
/// freed/never-allocated; a stale/forged index lands here → graceful error.
#[derive(Debug, Default)]
pub(crate) struct Slot<T> {
    pub(crate) live: bool,
    pub(crate) val: Option<T>,
}

/// A minimal slab: push returns an index, get/get_mut reject dead/forged
/// indices, free marks a slot dead. The unforgeable-handle invariant (I-4).
#[derive(Debug)]
pub(crate) struct Slab<T> {
    slots: Vec<Slot<T>>,
}

// Manual `Default` so `Slab<T>` does not require `T: Default` (the derive would
// add that bound, breaking `Slab<ModbusConn>` / `Slab<FhirConn>`).
impl<T> Default for Slab<T> {
    fn default() -> Self {
        Slab { slots: Vec::new() }
    }
}

impl<T> Slab<T> {
    pub(crate) fn insert(&mut self, val: T) -> i64 {
        let idx = self.slots.len() as i64;
        self.slots.push(Slot {
            live: true,
            val: Some(val),
        });
        idx
    }

    pub(crate) fn get_mut(&mut self, idx: i64) -> Result<&mut T, String> {
        let i = usize::try_from(idx).map_err(|_| bad_handle())?;
        match self.slots.get_mut(i) {
            Some(s) if s.live => s.val.as_mut().ok_or_else(bad_handle),
            _ => Err(bad_handle()),
        }
    }

    pub(crate) fn get(&self, idx: i64) -> Result<&T, String> {
        let i = usize::try_from(idx).map_err(|_| bad_handle())?;
        match self.slots.get(i) {
            Some(s) if s.live => s.val.as_ref().ok_or_else(bad_handle),
            _ => Err(bad_handle()),
        }
    }

    pub(crate) fn free(&mut self, idx: i64) -> Result<T, String> {
        let i = usize::try_from(idx).map_err(|_| bad_handle())?;
        match self.slots.get_mut(i) {
            Some(s) if s.live => {
                s.live = false;
                s.val.take().ok_or_else(bad_handle)
            }
            _ => Err(bad_handle()),
        }
    }
}

pub(crate) fn bad_handle() -> String {
    "native domain module: invalid or consumed handle (forged or stale index)".to_string()
}
