//! R13 — Native FFI: the curated native-module registry.
//!
//! This is the single source of truth for what `use native::M` may import,
//! shared by EVERY compiler stage (resolver, infer, checker, borrow, effects,
//! codegen, interp). It is the generalization of `BUILTIN_EXTERNS`: instead of
//! one hardcoded table of `__axon_*` builtins, a native module contributes a
//! row of `(axon-name, symbol, params, ret)` signatures plus a capability key
//! and an effect row.
//!
//! There is **no raw `extern "C"` in user Axon** (spec §3, Q1). A native module
//! is a curated Rust shim: each function maps to a `#[no_mangle] extern "C"`
//! symbol for codegen to link, and to an in-process Rust dispatcher for the
//! interpreter to call (`crate::interp::native_dispatch`). One impl, two
//! engines — I-2 by construction modulo the marshalling layer.
//!
//! ## Handles (spec §4/§5)
//!
//! The only new value crossing the boundary is an opaque, **affine** `Handle` =
//! `{i64 tag, i64 payload}` where `payload` indexes a per-module handle table —
//! NEVER a raw pointer, so Axon cannot forge a native pointer (I-4/I-11). A
//! handle is nominal per module/name (`gfx::Window` ≠ `gfx::Surface`), not
//! `Copy` (so the borrow checker treats it as `own`/affine, and use-after-
//! consume is E0601 at compile time, not a runtime liveness check).
//!
//! v1 ships ONE GPU-FREE MOCK module: `native::gfx` (an in-memory frame
//! counter), per spec §8/§11 slice 2. The real `axon-gfx` (wgpu/winit/vello)
//! shim is slice 4 and out of scope here.

use crate::types::Type;

/// One FFI-representable parameter/return type at the native boundary
/// (spec §4 "representable set"). Anything not expressible here is **not**
/// FFI-representable → E1801 at check time.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FfiType {
    I64,
    F64,
    Bool,
    /// Axon `str` — the frozen `{i64 len, ptr data}` ABI (I-6).
    Str,
    /// `[scalar]` — a slice of a scalar element, frozen `{i64 len, ptr}` ABI.
    SliceI64,
    SliceF64,
    /// An opaque nominal handle owned by module `module`, type name `name`.
    /// `resource = true` → affine (`own`, consumed by a consuming fn).
    /// `resource = false` → value handle (`Copy`, no drop obligation).
    Handle {
        module: &'static str,
        name: &'static str,
        resource: bool,
    },
    /// `Unit` return only (a void native call, e.g. `clear`/`present`).
    Unit,
}

impl FfiType {
    /// The semantic `Type` this FFI type maps to in inference / the checker.
    /// A handle becomes `Type::Deferred("native:<module>:<name>:<r|v>")` —
    /// an opaque nominal carrier that:
    ///  * is NOT `Copy` (borrow.rs `is_copy` excludes `Deferred`), so a
    ///    resource handle is affine and use-after-consume is E0601, and
    ///  * unifies only with the identical string, giving cross-module
    ///    nominal distinctness (E1802) for free.
    pub fn to_type(&self) -> Type {
        match self {
            FfiType::I64 => Type::I64,
            FfiType::F64 => Type::F64,
            FfiType::Bool => Type::Bool,
            FfiType::Str => Type::Str,
            FfiType::SliceI64 => Type::Slice(Box::new(Type::I64)),
            FfiType::SliceF64 => Type::Slice(Box::new(Type::F64)),
            FfiType::Handle {
                module,
                name,
                resource,
            } => Type::Deferred(handle_type_key(module, name, *resource)),
            FfiType::Unit => Type::Unit,
        }
    }
}

/// The `Type::Deferred` key string used to carry a native handle through the
/// type system. Encodes module + name + kind so it round-trips and so
/// `is_native_handle_key` can recognise it everywhere.
pub fn handle_type_key(module: &str, name: &str, resource: bool) -> String {
    format!(
        "native:{module}:{name}:{}",
        if resource { "res" } else { "val" }
    )
}

/// Is this `Type::Deferred(_)` key a native handle? Returns `(module, name,
/// resource)` if so.
pub fn parse_handle_key(key: &str) -> Option<(&str, &str, bool)> {
    let rest = key.strip_prefix("native:")?;
    let mut it = rest.splitn(3, ':');
    let module = it.next()?;
    let name = it.next()?;
    let kind = it.next()?;
    let resource = match kind {
        "res" => true,
        "val" => false,
        _ => return None,
    };
    Some((module, name, resource))
}

/// Parameter mode at the native boundary (spec §4/§5). A `Move` of a resource
/// handle CONSUMES it (the borrow checker marks the arg moved → later use is
/// E0601). A `Borrow` (`ref`) leaves the owner valid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamMode {
    /// By-value / consuming. For a resource handle this is the consuming move.
    Move,
    /// Borrow (`ref`) — non-consuming.
    Borrow,
}

/// One native function signature.
pub struct NativeFn {
    /// Axon-source name under the module (e.g. `"window_open"`), called as
    /// `gfx::window_open(...)`.
    pub name: &'static str,
    /// The `#[no_mangle] extern "C"` symbol codegen declares & links.
    pub symbol: &'static str,
    /// Parameter `(ffi-type, mode)` in order.
    pub params: &'static [(FfiType, ParamMode)],
    pub ret: FfiType,
}

/// A curated native module: its name, the `@[contained]`/effect capability key
/// it requires, the effect row it contributes to callers, and its functions.
pub struct NativeModule {
    /// Module name as imported: `use native::<name>`.
    pub name: &'static str,
    /// Capability grant key required in `@[contained(<cap>: …)]` (spec §3).
    pub capability: &'static str,
    /// Effect names contributed to a caller's row (Phase-6 bridge, E1310).
    pub effects: &'static [&'static str],
    pub functions: &'static [NativeFn],
}

impl NativeModule {
    pub fn function(&self, name: &str) -> Option<&NativeFn> {
        self.functions.iter().find(|f| f.name == name)
    }
}

// ── The v1 registry ──────────────────────────────────────────────────────────
//
// `native::gfx` — a GPU-FREE MOCK (spec §8/§11 slice 2). The shim is an
// in-process in-memory frame counter; `window_open` returns a resource handle,
// `clear`/`present` are effectful no-ops bumping a counter, `window_close`
// consumes the handle. No wgpu, no winit, no display — that is slice 4.

const GFX_FNS: &[NativeFn] = &[
    NativeFn {
        name: "window_open",
        symbol: "__axon_gfx_window_open",
        params: &[
            (FfiType::I64, ParamMode::Move), // width
            (FfiType::I64, ParamMode::Move), // height
            (FfiType::Str, ParamMode::Move), // title
        ],
        ret: FfiType::Handle {
            module: "gfx",
            name: "Window",
            resource: true,
        },
    },
    NativeFn {
        name: "surface",
        symbol: "__axon_gfx_surface",
        // Borrows the window (does not consume it), returns a new Surface handle.
        params: &[(
            FfiType::Handle {
                module: "gfx",
                name: "Window",
                resource: true,
            },
            ParamMode::Borrow,
        )],
        ret: FfiType::Handle {
            module: "gfx",
            name: "Surface",
            resource: true,
        },
    },
    NativeFn {
        name: "clear",
        symbol: "__axon_gfx_clear",
        params: &[
            (
                FfiType::Handle {
                    module: "gfx",
                    name: "Surface",
                    resource: true,
                },
                ParamMode::Borrow, // clear borrows the surface
            ),
            (FfiType::F64, ParamMode::Move), // r
            (FfiType::F64, ParamMode::Move), // g
            (FfiType::F64, ParamMode::Move), // b
            (FfiType::F64, ParamMode::Move), // a
        ],
        ret: FfiType::Unit,
    },
    NativeFn {
        name: "present",
        symbol: "__axon_gfx_present",
        params: &[(
            FfiType::Handle {
                module: "gfx",
                name: "Surface",
                resource: true,
            },
            ParamMode::Borrow,
        )],
        ret: FfiType::Unit,
    },
    NativeFn {
        name: "frame_count",
        symbol: "__axon_gfx_frame_count",
        // A pure value-returning probe (joins the differential parity corpus):
        // how many present() calls have happened. Borrows the surface.
        params: &[(
            FfiType::Handle {
                module: "gfx",
                name: "Surface",
                resource: true,
            },
            ParamMode::Borrow,
        )],
        ret: FfiType::I64,
    },
    NativeFn {
        name: "read_pixel",
        symbol: "__axon_gfx_read_pixel",
        // A pure value-returning probe (joins the differential parity corpus):
        // the surface's last cleared color, packed 0xRRGGBBAA (see
        // `axon_gfx_mock::pack_rgba8`). For the REAL wgpu module this is the
        // pixel read back from the offscreen render target after a clear+
        // present — the headless render-gate's acceptance check. Borrows the
        // surface. Model and real GPU return the IDENTICAL value (same packing,
        // non-sRGB target), so I-2 parity holds for this probe too.
        params: &[(
            FfiType::Handle {
                module: "gfx",
                name: "Surface",
                resource: true,
            },
            ParamMode::Borrow,
        )],
        ret: FfiType::I64,
    },
    NativeFn {
        name: "surface_close",
        symbol: "__axon_gfx_surface_close",
        // Consumes the Surface handle.
        params: &[(
            FfiType::Handle {
                module: "gfx",
                name: "Surface",
                resource: true,
            },
            ParamMode::Move,
        )],
        ret: FfiType::Unit,
    },
    NativeFn {
        name: "window_close",
        symbol: "__axon_gfx_window_close",
        // Consumes the Window handle (the affine drop site).
        params: &[(
            FfiType::Handle {
                module: "gfx",
                name: "Window",
                resource: true,
            },
            ParamMode::Move,
        )],
        ret: FfiType::Unit,
    },
];

const GFX: NativeModule = NativeModule {
    name: "gfx",
    capability: "gfx",
    effects: &["IO"],
    functions: GFX_FNS,
};

// ── R22 — domain-interop native modules ──────────────────────────────────────
//
// `native::modbus` (industrial), `native::fhir` (healthtech), `native::fix`
// (fintech). Built on the SAME registry machinery as `gfx`: a row here wires
// capability gating (E1004), the effect row (E1310), infer, the checker, and
// the borrow checker automatically. The impls live in the `axon-domain` crate
// (interp-only — codegen E0910-refuses the network ones; see `is_codegen_refused`).
//
// `modbus`/`fhir` declare `Net` (they open TCP / make HTTP requests) so an
// un-annotated caller must add `@[contained(modbus: any, net: ["host"])]`;
// `fix` is a pure codec and declares no effect.

const CONN: FfiType = FfiType::Handle {
    module: "modbus",
    name: "Conn",
    resource: true,
};

const MODBUS_FNS: &[NativeFn] = &[
    NativeFn {
        name: "modbus_connect",
        symbol: "__axon_modbus_connect",
        params: &[
            (FfiType::Str, ParamMode::Move), // host
            (FfiType::I64, ParamMode::Move), // port
        ],
        ret: CONN,
    },
    NativeFn {
        name: "modbus_read_holding",
        symbol: "__axon_modbus_read_holding",
        params: &[
            (CONN, ParamMode::Borrow),
            (FfiType::I64, ParamMode::Move), // addr
            (FfiType::I64, ParamMode::Move), // count
        ],
        ret: FfiType::SliceI64,
    },
    NativeFn {
        name: "modbus_write_register",
        symbol: "__axon_modbus_write_register",
        params: &[
            (CONN, ParamMode::Borrow),
            (FfiType::I64, ParamMode::Move), // addr
            (FfiType::I64, ParamMode::Move), // val
        ],
        ret: FfiType::Unit,
    },
    NativeFn {
        name: "modbus_read_coils",
        symbol: "__axon_modbus_read_coils",
        params: &[
            (CONN, ParamMode::Borrow),
            (FfiType::I64, ParamMode::Move),
            (FfiType::I64, ParamMode::Move),
        ],
        ret: FfiType::SliceI64,
    },
    NativeFn {
        name: "modbus_write_coil",
        symbol: "__axon_modbus_write_coil",
        params: &[
            (CONN, ParamMode::Borrow),
            (FfiType::I64, ParamMode::Move),
            (FfiType::I64, ParamMode::Move), // on (0/1)
        ],
        ret: FfiType::Unit,
    },
    NativeFn {
        name: "modbus_close",
        symbol: "__axon_modbus_close",
        params: &[(CONN, ParamMode::Move)], // consumes the connection
        ret: FfiType::Unit,
    },
];

const MODBUS: NativeModule = NativeModule {
    name: "modbus",
    capability: "modbus",
    effects: &["Net"],
    functions: MODBUS_FNS,
};

const FHIR_CONN: FfiType = FfiType::Handle {
    module: "fhir",
    name: "FhirConn",
    resource: true,
};

const FHIR_FNS: &[NativeFn] = &[
    NativeFn {
        name: "fhir_connect",
        symbol: "__axon_fhir_connect",
        params: &[(FfiType::Str, ParamMode::Move)], // base_url
        ret: FHIR_CONN,
    },
    NativeFn {
        name: "fhir_read",
        symbol: "__axon_fhir_read",
        params: &[
            (FHIR_CONN, ParamMode::Borrow),
            (FfiType::Str, ParamMode::Move), // resource type
            (FfiType::Str, ParamMode::Move), // id
        ],
        ret: FfiType::Str, // the JSON body (PHI — pair with the Secret lattice)
    },
    NativeFn {
        name: "fhir_search",
        symbol: "__axon_fhir_search",
        params: &[
            (FHIR_CONN, ParamMode::Borrow),
            (FfiType::Str, ParamMode::Move), // resource type
            (FfiType::Str, ParamMode::Move), // query string
        ],
        ret: FfiType::Str, // the Bundle JSON
    },
    NativeFn {
        name: "fhir_json_get",
        symbol: "__axon_fhir_json_get",
        // Pure dotted-path extract over a JSON string — no network, no handle.
        params: &[
            (FfiType::Str, ParamMode::Move), // json
            (FfiType::Str, ParamMode::Move), // path
        ],
        ret: FfiType::Str,
    },
    NativeFn {
        name: "fhir_close",
        symbol: "__axon_fhir_close",
        params: &[(FHIR_CONN, ParamMode::Move)],
        ret: FfiType::Unit,
    },
];

const FHIR: NativeModule = NativeModule {
    name: "fhir",
    capability: "fhir",
    effects: &["Net"],
    functions: FHIR_FNS,
};

const FIX_MSG: FfiType = FfiType::Handle {
    module: "fix",
    name: "FixMsg",
    resource: true,
};

const FIX_FNS: &[NativeFn] = &[
    NativeFn {
        name: "fix_new_order_single",
        symbol: "__axon_fix_new_order_single",
        params: &[
            (FfiType::Str, ParamMode::Move), // sender comp id
            (FfiType::Str, ParamMode::Move), // target comp id
            (FfiType::Str, ParamMode::Move), // clordid
            (FfiType::Str, ParamMode::Move), // symbol
            (FfiType::I64, ParamMode::Move), // side (1=Buy 2=Sell)
            (FfiType::I64, ParamMode::Move), // qty
            (FfiType::I64, ParamMode::Move), // price
        ],
        ret: FfiType::Str, // the SOH-delimited FIX 4.4 message
    },
    NativeFn {
        name: "fix_parse",
        symbol: "__axon_fix_parse",
        params: &[(FfiType::Str, ParamMode::Move)],
        ret: FIX_MSG,
    },
    NativeFn {
        name: "fix_get",
        symbol: "__axon_fix_get",
        params: &[
            (FIX_MSG, ParamMode::Borrow),
            (FfiType::I64, ParamMode::Move), // tag
        ],
        ret: FfiType::Str,
    },
    NativeFn {
        name: "fix_valid",
        symbol: "__axon_fix_valid",
        // Pure: 1 if BodyLength+CheckSum check out, else 0. No handle.
        params: &[(FfiType::Str, ParamMode::Move)],
        ret: FfiType::I64,
    },
    NativeFn {
        name: "fix_close",
        symbol: "__axon_fix_close",
        params: &[(FIX_MSG, ParamMode::Move)],
        ret: FfiType::Unit,
    },
];

const FIX: NativeModule = NativeModule {
    name: "fix",
    capability: "fix",
    effects: &[], // pure codec — no effect row
    functions: FIX_FNS,
};

/// R22: is this native module INTERP-ONLY (codegen must E0910-refuse a call to
/// it)? The domain modules do live network I/O (`modbus`/`fhir`) or are kept
/// interp-only for a uniform domain story (`fix`) — the `host_await`/native
/// precedent (sound-by-refusal). `gfx` is the only codegen-lowered module.
pub fn is_codegen_refused(module: &str) -> bool {
    matches!(module, "modbus" | "fhir" | "fix")
}

// ── `native::platform` — the R14 mobile platform shim (HEADLESS STUB) ─────────
//
// Spec §3/§7: the `std.platform.*` surface (lifecycle / permissions / haptics /
// notifications / biometrics) is delivered as a `native::platform` native module.
// On a device the iOS half calls UIKit/AVFoundation/LocalAuthentication; the
// interpreter (a dev laptop, no device) returns CLEAN REFUSALS / stubs — exactly
// like R13's headless gfx handling (I-9: clean refusal, not silent success). So
// `axon run app.ax` on Linux type-checks and runs the non-device logic, stubbing
// the device calls. The REAL UIKit-backed impl is iOS-only and lives behind
// `cfg(target_os="ios")` in the device build, NOT in this interp shim.
//
// v1 surfaces three representative entries:
//   * request_permission(name) -> i64   (a Result-shaped status; headless = DENIED)
//   * haptic(style)            -> Unit   (no-op off-device)
//   * is_device()             -> i64    (0 = headless/no device, 1 = on device)

const PLATFORM_FNS: &[NativeFn] = &[
    NativeFn {
        name: "request_permission",
        symbol: "__axon_platform_request_permission",
        // permission name (e.g. "Camera"); returns a status code.
        params: &[(FfiType::Str, ParamMode::Move)],
        // 0 = denied, 1 = granted, 2 = pending. Headless always returns 0.
        ret: FfiType::I64,
    },
    NativeFn {
        name: "haptic",
        symbol: "__axon_platform_haptic",
        // haptic style (e.g. "Light"); a no-op off-device.
        params: &[(FfiType::Str, ParamMode::Move)],
        ret: FfiType::Unit,
    },
    NativeFn {
        name: "is_device",
        symbol: "__axon_platform_is_device",
        // 0 = headless (interp / no device), 1 = on a real device.
        params: &[],
        ret: FfiType::I64,
    },
];

const PLATFORM: NativeModule = NativeModule {
    name: "platform",
    capability: "platform",
    // Lifecycle/permission calls are IO-effected (Phase-6 row → caller's row).
    effects: &["IO"],
    functions: PLATFORM_FNS,
};

/// Permission status returned by the headless `platform::request_permission`
/// stub: 0 = denied (the off-device default — a graceful Result, never a crash;
/// spec §7 "permission denied → graceful Result").
pub const PERMISSION_DENIED: i64 = 0;

/// Headless dispatch for `native::platform` (interp side, no device). Mirrors
/// `GfxMock::dispatch` but is stateless: every device-touching call returns its
/// clean off-device stub (I-9). The on-device behaviour is the iOS build's job.
pub fn platform_dispatch_headless(fnname: &str, args: &[GfxArg]) -> GfxResult {
    match (fnname, args) {
        // Off-device: every permission request is DENIED (a graceful status,
        // never a host abort). The user's `@[contained(platform:...)]` grant is a
        // SEPARATE compile-time gate (E1004); this is the runtime OS-grant layer.
        ("request_permission", [GfxArg::Str(_name)]) => Ok(GfxValue::Int(PERMISSION_DENIED)),
        // Haptics are a no-op with no hardware.
        ("haptic", [GfxArg::Str(_style)]) => Ok(GfxValue::Unit),
        // No device under the interpreter.
        ("is_device", []) => Ok(GfxValue::Int(0)),
        _ => Err(format!(
            "native::platform: bad call `{fnname}` (wrong argument shape)"
        )),
    }
}

/// All registered native modules: the GPU-free `gfx` mock, the R14 headless
/// mobile `platform` shim, plus the R22 domain-interop modules
/// (`modbus`/`fhir`/`fix`).
pub const MODULES: &[&NativeModule] = &[&GFX, &PLATFORM, &MODBUS, &FHIR, &FIX];

/// Look up a registered native module by name (the `M` in `use native::M`).
pub fn module(name: &str) -> Option<&'static NativeModule> {
    MODULES.iter().copied().find(|m| m.name == name)
}

/// Resolve a `M::fn` call name (e.g. `"gfx::window_open"`) to its module + fn.
pub fn resolve_call(qualified: &str) -> Option<(&'static NativeModule, &'static NativeFn)> {
    let (modname, fnname) = qualified.split_once("::")?;
    let m = module(modname)?;
    let f = m.function(fnname)?;
    Some((m, f))
}

/// Is `qualified` (a `::`-joined StructLit callee name) a native-module call?
pub fn is_native_call(qualified: &str) -> bool {
    resolve_call(qualified).is_some()
}

/// The FFI-representable predicate (spec §4/§5): the type-system half of E1801.
/// Returns `true` only for the allowed boundary set — scalars (i64/i32/f64/bool),
/// `str`, `[scalar]`, a native `Handle`, and `Unit` (return position only). A
/// user struct, `Option<T>`, `Result<T,E>`, a closure, a generic `T`, a channel,
/// a tuple, etc. is NOT representable → E1801 at check time, never reaching
/// codegen (mirrors the E0910 out-of-subset refusal).
pub fn is_ffi_repr(ty: &Type) -> bool {
    match ty {
        // Scalars.
        Type::I8
        | Type::I16
        | Type::I32
        | Type::I64
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::F32
        | Type::F64
        | Type::Bool
        | Type::Str
        | Type::Unit => true,
        // [scalar] only — a slice of a representable scalar element.
        Type::Slice(elem) => is_scalar(elem),
        // A native handle is carried as Deferred("native:…").
        Type::Deferred(key) => parse_handle_key(key).is_some(),
        _ => false,
    }
}

fn is_scalar(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
            | Type::Bool
            | Type::Str
    )
}

/// True if `ty` is a native handle carrier (`Type::Deferred("native:…")`).
pub fn is_native_handle(ty: &Type) -> bool {
    matches!(ty, Type::Deferred(k) if parse_handle_key(k).is_some())
}

/// True if `ty` is a native *resource* handle (affine — consumed by a consuming
/// call, use-after-consume is E0601). A value handle (`resource=false`) is Copy.
pub fn is_resource_handle(ty: &Type) -> bool {
    matches!(ty, Type::Deferred(k) if matches!(parse_handle_key(k), Some((_, _, true))))
}

// ── The `native::gfx` GPU-FREE MOCK shim (shared, one impl two engines) ──────
//
// The mock impl lives in the `axon-gfx-mock` crate (spec §3/§11: "the SAME crate
// is compiled into BOTH the native link and the compiler binary"). The
// interpreter drives a per-`Interp` `GfxMock` instance from that crate; the
// native link gets the matching `#[no_mangle] extern "C"` `__axon_gfx_*`
// symbols from `axon-rt::gfx_mock_ffi`, which wrap a process-global instance of
// the SAME `GfxMock`. So the value-returning `frame_count` is byte-identical
// across engines (the differential-parity corpus member) and the effectful
// calls share the call-trace oracle (spec §4).
//
// SAFETY / I-4: a handle's `payload` is a SLAB INDEX into a table of live
// slots, never a raw pointer. A consumed/forged/out-of-range index resolves to
// an absent slot → a graceful `Err`/refusal, NEVER a host abort. This holds
// across the C ABI because only the i64 index crosses the boundary; the table
// lives entirely inside the shared Rust crate on both engines.

// Re-export the shared mock types so the rest of the compiler (interp dispatch,
// the codegen lowering's tag helper) refers to one definition.
pub use axon_gfx_mock::{tag_for, GfxArg, GfxMock, GfxResult, GfxValue};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gfx_registered() {
        assert!(module("gfx").is_some());
        assert!(module("nope").is_none());
    }

    #[test]
    fn platform_registered() {
        // R14: the `native::platform` mobile shim is a registered native module.
        let m = module("platform").expect("platform registered");
        assert_eq!(m.capability, "platform");
        assert!(m.function("request_permission").is_some());
        assert!(m.function("haptic").is_some());
        assert!(m.function("is_device").is_some());
        let (m2, f) = resolve_call("platform::is_device").expect("resolves");
        assert_eq!(m2.name, "platform");
        assert_eq!(f.ret, FfiType::I64);
    }

    #[test]
    fn platform_headless_stub_is_clean_refusal() {
        // Spec §7: off-device, permission is DENIED, haptic is a no-op, no device.
        assert_eq!(
            platform_dispatch_headless("request_permission", &[GfxArg::Str("Camera".into())]),
            Ok(GfxValue::Int(PERMISSION_DENIED))
        );
        assert_eq!(
            platform_dispatch_headless("haptic", &[GfxArg::Str("Light".into())]),
            Ok(GfxValue::Unit)
        );
        assert_eq!(
            platform_dispatch_headless("is_device", &[]),
            Ok(GfxValue::Int(0))
        );
        // A wrong-shape call is a graceful Err, never a panic (I-4).
        assert!(platform_dispatch_headless("request_permission", &[]).is_err());
        assert!(platform_dispatch_headless("nope", &[]).is_err());
    }

    #[test]
    fn resolve_known_call() {
        let (m, f) = resolve_call("gfx::window_open").expect("known");
        assert_eq!(m.name, "gfx");
        assert_eq!(f.symbol, "__axon_gfx_window_open");
        assert!(matches!(f.ret, FfiType::Handle { resource: true, .. }));
    }

    #[test]
    fn unknown_call_not_native() {
        assert!(!is_native_call("gfx::nope"));
        assert!(!is_native_call("nope::x"));
        assert!(!is_native_call("plain"));
    }

    #[test]
    fn handle_key_roundtrip() {
        let k = handle_type_key("gfx", "Window", true);
        assert_eq!(parse_handle_key(&k), Some(("gfx", "Window", true)));
        let v = handle_type_key("gfx", "Color", false);
        assert_eq!(parse_handle_key(&v), Some(("gfx", "Color", false)));
        assert_eq!(parse_handle_key("Dict"), None);
    }

    #[test]
    fn ffi_repr_accepts_allowed_set() {
        assert!(is_ffi_repr(&Type::I64));
        assert!(is_ffi_repr(&Type::F64));
        assert!(is_ffi_repr(&Type::Bool));
        assert!(is_ffi_repr(&Type::Str));
        assert!(is_ffi_repr(&Type::Unit));
        assert!(is_ffi_repr(&Type::Slice(Box::new(Type::I64))));
        assert!(is_ffi_repr(&Type::Slice(Box::new(Type::Str))));
        assert!(is_ffi_repr(
            &FfiType::Handle {
                module: "gfx",
                name: "Window",
                resource: true
            }
            .to_type()
        ));
    }

    #[test]
    fn ffi_repr_rejects_disallowed() {
        // user struct, Option, Result, closure, generic, tuple, channel,
        // and a slice of a non-scalar — none are FFI-representable.
        assert!(!is_ffi_repr(&Type::Struct("Big".into())));
        assert!(!is_ffi_repr(&Type::Option(Box::new(Type::I64))));
        assert!(!is_ffi_repr(&Type::Result(
            Box::new(Type::I64),
            Box::new(Type::Str)
        )));
        assert!(!is_ffi_repr(&Type::Fn(vec![], Box::new(Type::I64))));
        assert!(!is_ffi_repr(&Type::TypeParam("T".into())));
        assert!(!is_ffi_repr(&Type::Tuple(vec![Type::I64, Type::I64])));
        assert!(!is_ffi_repr(&Type::Chan(Box::new(Type::I64))));
        assert!(!is_ffi_repr(&Type::Slice(Box::new(Type::Struct(
            "Big".into()
        )))));
        // Non-handle Deferred (e.g. the Dict opaque) is not representable.
        assert!(!is_ffi_repr(&Type::Deferred("Dict".into())));
    }

    // Helper: a handle arg with the right nominal tag for the given name.
    fn h(name: &str, payload: i64) -> GfxArg {
        GfxArg::Handle {
            tag: tag_for(name),
            payload,
        }
    }

    #[test]
    fn gfx_mock_roundtrip() {
        let mut m = GfxMock::new();
        let win = match m
            .dispatch(
                "window_open",
                &[
                    GfxArg::Int(800),
                    GfxArg::Int(600),
                    GfxArg::Str("axon".into()),
                ],
            )
            .unwrap()
        {
            GfxValue::Handle { payload, .. } => payload,
            _ => panic!("expected handle"),
        };
        let wh = h("Window", win);
        let surf = match m.dispatch("surface", std::slice::from_ref(&wh)).unwrap() {
            GfxValue::Handle { payload, .. } => payload,
            _ => panic!("expected handle"),
        };
        let sh = h("Surface", surf);
        assert_eq!(
            m.dispatch(
                "clear",
                &[
                    sh.clone(),
                    GfxArg::Float(0.1),
                    GfxArg::Float(0.1),
                    GfxArg::Float(0.1),
                    GfxArg::Float(1.0)
                ]
            )
            .unwrap(),
            GfxValue::Unit
        );
        m.dispatch("present", std::slice::from_ref(&sh)).unwrap();
        m.dispatch("present", std::slice::from_ref(&sh)).unwrap();
        assert_eq!(
            m.dispatch("frame_count", std::slice::from_ref(&sh))
                .unwrap(),
            GfxValue::Int(2)
        );
        m.dispatch("surface_close", std::slice::from_ref(&sh))
            .unwrap();
        m.dispatch("window_close", std::slice::from_ref(&wh))
            .unwrap();
    }

    #[test]
    fn gfx_mock_bad_handle_is_graceful_err_not_panic() {
        let mut m = GfxMock::new();
        // A forged/out-of-range index → Err, NEVER a panic/abort (I-4).
        for bad in [9999i64, -1, i64::MIN, i64::MAX, 0] {
            assert!(
                m.dispatch("frame_count", &[h("Surface", bad)]).is_err(),
                "forged index {bad} must be graceful Err"
            );
        }
    }

    #[test]
    fn gfx_mock_use_after_consume_is_graceful_err() {
        // (The PRIMARY guarantee is the COMPILE-TIME affine borrow check; this
        // is the runtime defense-in-depth backstop.)
        let mut m = GfxMock::new();
        let win = match m
            .dispatch(
                "window_open",
                &[GfxArg::Int(1), GfxArg::Int(1), GfxArg::Str("x".into())],
            )
            .unwrap()
        {
            GfxValue::Handle { payload, .. } => payload,
            _ => unreachable!(),
        };
        let wh = h("Window", win);
        m.dispatch("window_close", std::slice::from_ref(&wh))
            .unwrap();
        // Reusing the freed window index → graceful Err.
        assert!(m.dispatch("surface", std::slice::from_ref(&wh)).is_err());
        assert!(m
            .dispatch("window_close", std::slice::from_ref(&wh))
            .is_err());
    }

    #[test]
    fn resource_handle_is_not_copy() {
        // The handle type maps to Deferred, which borrow.rs::is_copy excludes —
        // so a resource handle is affine. (Asserted structurally here; the
        // borrow-checker behaviour is tested in borrow.rs / integration.)
        let t = FfiType::Handle {
            module: "gfx",
            name: "Window",
            resource: true,
        }
        .to_type();
        assert!(matches!(t, Type::Deferred(_)));
    }
}
