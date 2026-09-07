//! R13 — the GPU-FREE `native::gfx` mock shim. ONE Rust impl, TWO call sites.
//!
//! Per spec §3/§11 a native module's shim is "compiled into BOTH the native
//! link (for codegen) AND the compiler binary (for interp) — same Rust fn, two
//! call sites." This crate is that single impl for the v1 `gfx` mock:
//!
//!  * **Interpreter** (`axon-core::native`) holds a per-`Interp` [`GfxMock`]
//!    instance and calls [`GfxMock::dispatch`] directly with in-process values.
//!  * **Native codegen** links `axon-rt`, which wraps a process-global
//!    [`GfxMock`] behind the `#[no_mangle] extern "C"` `__axon_gfx_*` symbols
//!    (see `axon-rt/src/gfx_mock_ffi.rs`); LLVM-emitted code calls those symbols
//!    across the C ABI.
//!
//! Both paths run the *identical* `dispatch` logic, so I-2 holds modulo the
//! marshalling layer (the one place interp and codegen reach this fn by
//! different routes — exactly where the wasm i64↔i32 saga lived, so it gets the
//! fuzzer + the call-trace oracle).
//!
//! ## Handles are unforgeable slab indices, never raw pointers (I-4/I-11)
//!
//! A handle's `payload` is an index into a per-table `Vec<GfxSlot>`. A
//! forged / stale / out-of-range / `i64::MIN` index resolves to an absent or
//! freed slot → a graceful [`GfxResult::Err`] (mapped to exit-101 by the
//! interpreter, and to a graceful runtime panic by the native shim) — it can
//! **never** dereference a wild pointer or abort the host. This holds across the
//! C ABI because only the i64 index crosses the boundary; the `Vec` lives
//! entirely inside this Rust crate on both engines.
//!
//! ## Call-trace oracle (spec §4)
//!
//! Effectful calls (`clear`/`present`) have no diffable stdout — the effect is
//! pixels. So I-2 is restored for them by a *call-trace oracle*: each dispatch
//! appends a `(module, fn, arg-hash, handle-ids)` line to a trace. When
//! `AXON_NATIVE_TRACE=1` (or [`set_trace_enabled`]) is set the line is printed
//! to **stderr** with a fixed `native-trace: …` prefix; parity = interp and
//! codegen emit the byte-identical trace for the same program.

use std::sync::atomic::{AtomicBool, Ordering};

/// Stable nominal handle tag (the `tag` half of the `{i64 tag, i64 payload}`
/// frozen Handle layout, I-6). Encodes which module/table a payload indexes so
/// the shim can route + reject a tag/table mismatch (defense-in-depth beneath
/// the static E1802 nominal check). These ids are FROZEN — codegen embeds them
/// as constants, so they must never be renumbered.
pub const TAG_GFX_WINDOW: i64 = 0x6766_7801; // "gfx" + Window
pub const TAG_GFX_SURFACE: i64 = 0x6766_7802; // "gfx" + Surface

/// Pack an `(r,g,b,a)` clear color of `[0.0, 1.0]` floats into a single i64 as
/// `0xRRGGBBAA` (each channel an 8-bit `round(c * 255)`, clamped to `[0,255]`).
///
/// This is the FROZEN color encoding shared by EVERY gfx backend (the mock here
/// AND the real wgpu module in `axon-gfx`): the mock computes it directly and
/// the real GPU reads back the rendered pixel and packs it the SAME way, so
/// `read_pixel` is byte-identical across the model and the real render (the I-2
/// parity contract for the value-returning probe). The real renderer therefore
/// uses a NON-sRGB (`Rgba8Unorm`) target so a clear value maps linearly to the
/// stored byte — matching this `round(c*255)` exactly.
pub fn pack_rgba8(r: f64, g: f64, b: f64, a: f64) -> i64 {
    fn ch(c: f64) -> i64 {
        // NaN → 0; clamp to [0,1] then round to nearest byte.
        if c.is_nan() {
            return 0;
        }
        let c = c.clamp(0.0, 1.0);
        (c * 255.0).round() as i64
    }
    (ch(r) << 24) | (ch(g) << 16) | (ch(b) << 8) | ch(a)
}

/// Map a `gfx` handle *name* to its frozen nominal tag.
pub fn tag_for(name: &str) -> i64 {
    match name {
        "Window" => TAG_GFX_WINDOW,
        "Surface" => TAG_GFX_SURFACE,
        // An unknown name is not constructible by the front-end (the registry is
        // closed); return a poison tag that matches no table → graceful Err.
        _ => -1,
    }
}

/// One live slot in a handle table. `live = false` ⇒ freed/never-allocated (a
/// stale or forged index lands here → graceful error, never a wild deref).
#[derive(Clone, Copy, Debug, Default)]
struct GfxSlot {
    live: bool,
    /// Bumped on free so a stale index into a reused slot is still detectable
    /// (defense in depth; the affine type system is the primary guarantee).
    generation: u32,
}

/// The mock gfx backend state — a frame counter + two per-table slabs. NO
/// wgpu/winit/GPU. Used per-`Interp` by the interpreter and as a process-global
/// (behind a `Mutex`) by the native shim.
#[derive(Debug, Default)]
pub struct GfxMock {
    windows: Vec<GfxSlot>,
    surfaces: Vec<GfxSlot>,
    /// The last color each surface was cleared to, packed `0xRRGGBBAA`
    /// ([`pack_rgba8`]) — what `read_pixel` returns. Parallel to `surfaces`;
    /// the model of "the framebuffer holds the last clear color". A surface
    /// never cleared reads back `0` (transparent black). This is exactly what
    /// the real wgpu offscreen render produces after a clear (same packing,
    /// non-sRGB target), so `read_pixel` is parity-identical model↔GPU.
    surface_color: Vec<i64>,
    /// How many `present()` calls have happened (what `frame_count` reads).
    frames: i64,
}

/// The outcome of a mock gfx call: a value, or a graceful error string (NEVER a
/// host abort — I-4). Both engines map `Err` to a graceful exit-101 panic.
pub type GfxResult = Result<GfxValue, String>;

/// A value a mock gfx call returns. Kept independent of any engine's value enum.
#[derive(Debug, Clone, PartialEq)]
pub enum GfxValue {
    Unit,
    Int(i64),
    /// A freshly-allocated handle `(name, payload)`. The caller wraps it in its
    /// own handle representation (`Value::Handle` in interp; `{tag,payload}` in
    /// codegen, with `tag = tag_for(name)`).
    Handle {
        name: &'static str,
        payload: i64,
    },
}

/// A marshalled argument crossing into the dispatcher. Only the representable
/// boundary set (spec §4): scalar, str, and a handle's slab index.
#[derive(Debug, Clone, PartialEq)]
pub enum GfxArg {
    Int(i64),
    Float(f64),
    Str(String),
    /// A handle `(tag, payload)` — the nominal tag plus its slab index. Both
    /// halves cross the boundary so the shim can reject a tag/table mismatch.
    Handle {
        tag: i64,
        payload: i64,
    },
}

impl GfxArg {
    /// The handle payload (slab index) if this is a handle, else `None`.
    fn handle_payload(&self) -> Option<i64> {
        match self {
            GfxArg::Handle { payload, .. } => Some(*payload),
            _ => None,
        }
    }
}

impl GfxMock {
    pub fn new() -> Self {
        GfxMock::default()
    }

    fn window_open(&mut self, _w: i64, _h: i64, _title: &str) -> GfxResult {
        let idx = self.windows.len() as i64;
        self.windows.push(GfxSlot {
            live: true,
            generation: 0,
        });
        Ok(GfxValue::Handle {
            name: "Window",
            payload: idx,
        })
    }

    fn surface(&mut self, win: i64) -> GfxResult {
        self.check_window(win)?;
        let idx = self.surfaces.len() as i64;
        self.surfaces.push(GfxSlot {
            live: true,
            generation: 0,
        });
        self.surface_color.push(0);
        Ok(GfxValue::Handle {
            name: "Surface",
            payload: idx,
        })
    }

    fn clear(&mut self, surf: i64, r: f64, g: f64, b: f64, a: f64) -> GfxResult {
        self.check_surface(surf)?;
        // Record the cleared color so `read_pixel` can return it — the model of
        // "the framebuffer now holds this color". The packing is the shared
        // FROZEN encoding the real GPU readback also uses.
        self.surface_color[surf as usize] = pack_rgba8(r, g, b, a);
        Ok(GfxValue::Unit)
    }

    fn read_pixel(&self, surf: i64) -> GfxResult {
        self.check_surface(surf)?;
        Ok(GfxValue::Int(self.surface_color[surf as usize]))
    }

    fn present(&mut self, surf: i64) -> GfxResult {
        self.check_surface(surf)?;
        self.frames += 1;
        Ok(GfxValue::Unit)
    }

    fn frame_count(&self, surf: i64) -> GfxResult {
        self.check_surface(surf)?;
        Ok(GfxValue::Int(self.frames))
    }

    fn surface_close(&mut self, surf: i64) -> GfxResult {
        self.check_surface(surf)?;
        let s = &mut self.surfaces[surf as usize];
        s.live = false;
        s.generation = s.generation.wrapping_add(1);
        Ok(GfxValue::Unit)
    }

    fn window_close(&mut self, win: i64) -> GfxResult {
        self.check_window(win)?;
        let w = &mut self.windows[win as usize];
        w.live = false;
        w.generation = w.generation.wrapping_add(1);
        Ok(GfxValue::Unit)
    }

    fn check_window(&self, win: i64) -> Result<(), String> {
        match self
            .windows
            .get(usize::try_from(win).map_err(|_| bad_handle())?)
        {
            Some(s) if s.live => Ok(()),
            _ => Err(bad_handle()),
        }
    }

    fn check_surface(&self, surf: i64) -> Result<(), String> {
        match self
            .surfaces
            .get(usize::try_from(surf).map_err(|_| bad_handle())?)
        {
            Some(s) if s.live => Ok(()),
            _ => Err(bad_handle()),
        }
    }

    /// Dispatch a resolved `gfx` fn by name. `args` are the marshalled payloads.
    /// A bad handle index here STILL returns a graceful `Err` (I-4 defense in
    /// depth across BOTH engines), never a panic/abort.
    ///
    /// Appends a call-trace line first (the §4 oracle), so interp and codegen
    /// produce the identical trace for the same program.
    pub fn dispatch(&mut self, fnname: &str, args: &[GfxArg]) -> GfxResult {
        emit_trace("gfx", fnname, args);
        match (fnname, args) {
            ("window_open", [GfxArg::Int(w), GfxArg::Int(h), GfxArg::Str(t)]) => {
                self.window_open(*w, *h, t)
            }
            ("surface", [GfxArg::Handle { payload, .. }]) => self.surface(*payload),
            (
                "clear",
                [GfxArg::Handle { payload, .. }, GfxArg::Float(r), GfxArg::Float(g), GfxArg::Float(b), GfxArg::Float(a)],
            ) => self.clear(*payload, *r, *g, *b, *a),
            ("present", [GfxArg::Handle { payload, .. }]) => self.present(*payload),
            ("frame_count", [GfxArg::Handle { payload, .. }]) => self.frame_count(*payload),
            ("read_pixel", [GfxArg::Handle { payload, .. }]) => self.read_pixel(*payload),
            ("surface_close", [GfxArg::Handle { payload, .. }]) => self.surface_close(*payload),
            ("window_close", [GfxArg::Handle { payload, .. }]) => self.window_close(*payload),
            _ => Err(format!(
                "native::gfx: bad call `{fnname}` (wrong argument shape)"
            )),
        }
    }
}

fn bad_handle() -> String {
    "native::gfx: invalid or consumed handle (forged or stale index)".to_string()
}

// ── Call-trace oracle (spec §4) ──────────────────────────────────────────────

static TRACE_ENABLED: AtomicBool = AtomicBool::new(false);

/// Force-enable the call-trace oracle (in addition to the `AXON_NATIVE_TRACE`
/// env var). Used by tests that want the trace without mutating the env.
pub fn set_trace_enabled(on: bool) {
    TRACE_ENABLED.store(on, Ordering::Relaxed);
}

fn trace_on() -> bool {
    TRACE_ENABLED.load(Ordering::Relaxed)
        || std::env::var("AXON_NATIVE_TRACE")
            .map(|v| v != "0" && !v.is_empty())
            .unwrap_or(false)
}

/// A stable, engine-independent hash of the marshalled args (the "marshalled-
/// arg-hash" of the §4 trace tuple). FNV-1a over a canonical byte encoding so
/// the same args produce the same hash on interp and codegen regardless of how
/// the value reached the dispatcher.
pub fn arg_hash(args: &[GfxArg]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |b: u8| {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for a in args {
        match a {
            GfxArg::Int(n) => {
                mix(b'i');
                for b in n.to_le_bytes() {
                    mix(b);
                }
            }
            GfxArg::Float(f) => {
                mix(b'f');
                // Canonicalize -0.0/+0.0 and NaN so the hash is engine-stable.
                let bits = if f.is_nan() {
                    f64::NAN.to_bits()
                } else if *f == 0.0 {
                    0.0f64.to_bits()
                } else {
                    f.to_bits()
                };
                for b in bits.to_le_bytes() {
                    mix(b);
                }
            }
            GfxArg::Str(s) => {
                mix(b's');
                for b in s.as_bytes() {
                    mix(*b);
                }
                mix(0);
            }
            GfxArg::Handle { tag, .. } => {
                // Hash the NOMINAL tag, not the slab index — the index is an
                // internal allocation detail that must match across engines but
                // is not part of the program-observable arg identity. The
                // handle ids are reported separately in the trace line.
                mix(b'h');
                for b in tag.to_le_bytes() {
                    mix(b);
                }
            }
        }
    }
    h
}

fn emit_trace(module: &str, fnname: &str, args: &[GfxArg]) {
    if !trace_on() {
        return;
    }
    let hash = arg_hash(args);
    // Handle ids = the slab indices of every handle arg, in order.
    let ids: Vec<String> = args
        .iter()
        .filter_map(|a| a.handle_payload())
        .map(|p| p.to_string())
        .collect();
    eprintln!(
        "native-trace: {module}::{fnname} arg-hash={hash:016x} handles=[{}]",
        ids.join(",")
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
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
            _ => panic!("handle"),
        };
        let wh = GfxArg::Handle {
            tag: TAG_GFX_WINDOW,
            payload: win,
        };
        let surf = match m.dispatch("surface", std::slice::from_ref(&wh)).unwrap() {
            GfxValue::Handle { payload, .. } => payload,
            _ => panic!("handle"),
        };
        let sh = GfxArg::Handle {
            tag: TAG_GFX_SURFACE,
            payload: surf,
        };
        m.dispatch(
            "clear",
            &[
                sh.clone(),
                GfxArg::Float(0.1),
                GfxArg::Float(0.1),
                GfxArg::Float(0.1),
                GfxArg::Float(1.0),
            ],
        )
        .unwrap();
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
    fn bad_handle_is_graceful_err_not_panic() {
        let mut m = GfxMock::new();
        for bad in [9999i64, -1, i64::MIN, i64::MAX] {
            let h = GfxArg::Handle {
                tag: TAG_GFX_SURFACE,
                payload: bad,
            };
            assert!(
                m.dispatch("frame_count", std::slice::from_ref(&h)).is_err(),
                "forged index {bad} must be a graceful Err"
            );
        }
    }

    #[test]
    fn use_after_consume_is_graceful_err() {
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
        let wh = GfxArg::Handle {
            tag: TAG_GFX_WINDOW,
            payload: win,
        };
        m.dispatch("window_close", std::slice::from_ref(&wh))
            .unwrap();
        assert!(m.dispatch("surface", std::slice::from_ref(&wh)).is_err());
        assert!(m
            .dispatch("window_close", std::slice::from_ref(&wh))
            .is_err());
    }

    #[test]
    fn pack_rgba8_frozen_encoding() {
        // Opaque black / white.
        assert_eq!(pack_rgba8(0.0, 0.0, 0.0, 1.0), 0x0000_00FF);
        assert_eq!(pack_rgba8(1.0, 1.0, 1.0, 1.0), 0xFFFF_FFFF);
        // The gate's known color r=0.0, g=128/255≈0.502, b=1.0, a=1.0 packs to
        // 0x0080FFFF (g rounds back to 128).
        assert_eq!(pack_rgba8(0.0, 128.0 / 255.0, 1.0, 1.0), 0x0080_FFFF);
        // Clamp + NaN.
        assert_eq!(pack_rgba8(2.0, -1.0, f64::NAN, 1.0), 0xFF00_00FF);
    }

    #[test]
    fn read_pixel_returns_last_cleared_color() {
        let mut m = GfxMock::new();
        let win = match m
            .dispatch(
                "window_open",
                &[GfxArg::Int(4), GfxArg::Int(4), GfxArg::Str("x".into())],
            )
            .unwrap()
        {
            GfxValue::Handle { payload, .. } => payload,
            _ => unreachable!(),
        };
        let wh = GfxArg::Handle {
            tag: TAG_GFX_WINDOW,
            payload: win,
        };
        let surf = match m.dispatch("surface", std::slice::from_ref(&wh)).unwrap() {
            GfxValue::Handle { payload, .. } => payload,
            _ => unreachable!(),
        };
        let sh = GfxArg::Handle {
            tag: TAG_GFX_SURFACE,
            payload: surf,
        };
        // Before any clear: transparent black.
        assert_eq!(
            m.dispatch("read_pixel", std::slice::from_ref(&sh)).unwrap(),
            GfxValue::Int(0)
        );
        // Clear to the gate color and read it back.
        m.dispatch(
            "clear",
            &[
                sh.clone(),
                GfxArg::Float(0.0),
                GfxArg::Float(128.0 / 255.0),
                GfxArg::Float(1.0),
                GfxArg::Float(1.0),
            ],
        )
        .unwrap();
        assert_eq!(
            m.dispatch("read_pixel", std::slice::from_ref(&sh)).unwrap(),
            GfxValue::Int(0x0080_FFFF)
        );
    }

    #[test]
    fn arg_hash_is_stable_and_index_independent() {
        // Same nominal tag, different slab index → SAME hash (index excluded).
        let a = [GfxArg::Handle {
            tag: TAG_GFX_SURFACE,
            payload: 0,
        }];
        let b = [GfxArg::Handle {
            tag: TAG_GFX_SURFACE,
            payload: 7,
        }];
        assert_eq!(arg_hash(&a), arg_hash(&b));
        // Different tag → different hash.
        let c = [GfxArg::Handle {
            tag: TAG_GFX_WINDOW,
            payload: 0,
        }];
        assert_ne!(arg_hash(&a), arg_hash(&c));
        // Scalars contribute.
        assert_ne!(arg_hash(&[GfxArg::Int(1)]), arg_hash(&[GfxArg::Int(2)]));
    }
}
