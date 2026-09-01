//! R13 slice 5 — the FIRST REAL native module: a headless wgpu offscreen renderer.
//!
//! `axon-gfx` is the real GPU backing for `use native::gfx` when axon-core is
//! built with the `gfx-wgpu` feature. It mirrors the GPU-free `axon-gfx-mock`'s
//! interface EXACTLY — the same [`GfxArg`]/[`GfxValue`]/[`GfxResult`] vocabulary
//! and the same `dispatch(fnname, &[GfxArg]) -> GfxResult` entry point — so the
//! interpreter routes to it as a drop-in backend with no surface change. That is
//! the literal realization of slice 4's design note: "a real module is a drop-in
//! registry row + its own backend — no surface change needed."
//!
//! ## What it renders
//!
//! `window_open(w,h,title)` creates an OFFSCREEN render target sized `w×h` — a
//! real `wgpu::Texture` (RGBA8 UNORM, non-sRGB so a clear value maps linearly to
//! the stored byte). There is **no winit window, no surface, no display**: the
//! target is an offscreen texture, so the whole pipeline is headlessly verifiable
//! on a software Vulkan backend (Mesa lavapipe). `clear(surf, r,g,b,a)` records a
//! render pass that clears the texture to that color and submits it. `present`
//! bumps the frame counter. `read_pixel(surf)` copies the texture into a mappable
//! buffer, maps it, reads the top-left texel, and packs it `0xRRGGBBAA` with the
//! SAME [`axon_gfx_mock::pack_rgba8`] encoding the mock uses — so the value is
//! byte-identical model↔GPU (the I-2 parity contract for the value-returning
//! probe).
//!
//! ## Forge-safety preserved (I-4/I-11) — only an index crosses the boundary
//!
//! A handle's `payload` is a SLAB INDEX into a per-table `Vec`, never a raw GPU
//! pointer or a `wgpu::Texture` address. A forged / stale / out-of-range /
//! `i64::MIN` index resolves to an absent or freed slot → a graceful
//! [`GfxResult::Err`], NEVER a wild deref or host abort. The real wgpu resources
//! live entirely inside this crate, keyed by the index — exactly the slice-4
//! invariant, unchanged.

use axon_gfx_mock::{pack_rgba8, GfxArg, GfxResult, GfxValue};

/// A live offscreen surface: its real wgpu render target + the last color it was
/// cleared to (read back from the GPU). `live = false` ⇒ freed (a stale/forged
/// index lands here → graceful error).
struct SurfaceSlot {
    live: bool,
    texture: wgpu::Texture,
    width: u32,
    height: u32,
    /// The last cleared color packed `0xRRGGBBAA`, read back from the GPU after
    /// the clear render pass. `None` until the first clear.
    last_pixel: Option<i64>,
}

/// A live window slot. The "window" is purely logical here (offscreen, no winit);
/// it just owns the size and gates surface creation, mirroring the mock.
#[derive(Clone, Copy)]
struct WindowSlot {
    live: bool,
    width: u32,
    height: u32,
}

/// The real wgpu gfx backend state. One `Device`/`Queue` (lazily created on the
/// first `window_open`) plus per-table slabs of windows and offscreen surfaces.
///
/// Mirrors `axon_gfx_mock::GfxMock`'s shape so the interpreter can hold one or
/// the other behind the same dispatch call.
pub struct GfxReal {
    device: Option<wgpu::Device>,
    queue: Option<wgpu::Queue>,
    windows: Vec<WindowSlot>,
    surfaces: Vec<SurfaceSlot>,
    frames: i64,
}

impl Default for GfxReal {
    fn default() -> Self {
        Self::new()
    }
}

/// RGBA8 bytes per texel (the offscreen target format).
const BYTES_PER_PIXEL: u32 = 4;
/// wgpu requires the copy buffer row stride be a multiple of this.
const COPY_ALIGN: u32 = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;

impl GfxReal {
    pub fn new() -> Self {
        GfxReal {
            device: None,
            queue: None,
            windows: Vec::new(),
            surfaces: Vec::new(),
            frames: 0,
        }
    }

    /// Lazily acquire a headless wgpu adapter/device/queue. Uses the env-pinned
    /// backend (the gate sets `WGPU_BACKEND=vulkan` + the lavapipe ICD), with no
    /// surface compatibility requirement (offscreen). Any failure is a graceful
    /// `Err` — NEVER a panic/abort (I-4): a host with no usable Vulkan refuses
    /// cleanly, exactly like any absent native capability (I-9).
    fn ensure_device(&mut self) -> Result<(), String> {
        if self.device.is_some() {
            return Ok(());
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            // Honor WGPU_BACKEND (the gate pins it to `vulkan` for lavapipe);
            // default to Vulkan for headless software rendering on Linux.
            backends: wgpu::util::backend_bits_from_env().unwrap_or(wgpu::Backends::VULKAN),
            ..Default::default()
        });
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::None,
            force_fallback_adapter: false,
            compatible_surface: None, // headless: no surface
        }))
        .ok_or_else(|| {
            "native::gfx: no usable wgpu adapter (headless GPU unavailable). \
             Set WGPU_BACKEND=vulkan + VK_ICD_FILENAMES to a software Vulkan ICD \
             (e.g. Mesa lavapipe) for headless rendering."
                .to_string()
        })?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("axon-gfx headless device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_defaults(),
                memory_hints: wgpu::MemoryHints::default(),
            },
            None,
        ))
        .map_err(|e| format!("native::gfx: wgpu device request failed: {e}"))?;
        // Announce the real adapter once (proves the render ran on an actual GPU
        // backend — e.g. `llvmpipe (Cpu, Vulkan)` under lavapipe). The render
        // gate greps this line so a vacuous pass (no GPU touched) is impossible.
        let info = adapter.get_info();
        eprintln!(
            "axon-gfx: wgpu adapter = {} ({:?}, {:?})",
            info.name, info.device_type, info.backend
        );
        self.device = Some(device);
        self.queue = Some(queue);
        Ok(())
    }

    fn window_open(&mut self, w: i64, h: i64, _title: &str) -> GfxResult {
        // Clamp the requested size into a sane offscreen extent (>=1).
        let width = w.clamp(1, 16384) as u32;
        let height = h.clamp(1, 16384) as u32;
        self.ensure_device()?;
        let idx = self.windows.len() as i64;
        self.windows.push(WindowSlot {
            live: true,
            width,
            height,
        });
        Ok(GfxValue::Handle {
            name: "Window",
            payload: idx,
        })
    }

    fn surface(&mut self, win: i64) -> GfxResult {
        let w = self.check_window(win)?;
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| "native::gfx: device not initialized".to_string())?;
        // The offscreen render target: a real GPU texture (non-sRGB so a clear
        // value maps linearly to the stored byte — matching pack_rgba8).
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("axon-gfx offscreen target"),
            size: wgpu::Extent3d {
                width: w.width,
                height: w.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let idx = self.surfaces.len() as i64;
        self.surfaces.push(SurfaceSlot {
            live: true,
            texture,
            width: w.width,
            height: w.height,
            last_pixel: None,
        });
        Ok(GfxValue::Handle {
            name: "Surface",
            payload: idx,
        })
    }

    fn clear(&mut self, surf: i64, r: f64, g: f64, b: f64, a: f64) -> GfxResult {
        self.check_surface(surf)?;
        // Borrow the disjoint fields directly (device/queue vs surfaces) so the
        // borrow checker is happy without cloning the (non-`Clone`) Device/Queue.
        let device = self
            .device
            .as_ref()
            .ok_or_else(|| "native::gfx: device not initialized".to_string())?;
        let queue = self
            .queue
            .as_ref()
            .ok_or_else(|| "native::gfx: queue not initialized".to_string())?;
        let slot = &self.surfaces[surf as usize];
        let view = slot
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("axon-gfx clear encoder"),
        });
        {
            // A render pass whose only op is the clear (LoadOp::Clear + Store).
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("axon-gfx clear pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // Non-sRGB target → stored byte = round(c*255), matching
                        // pack_rgba8 — that is what keeps read_pixel parity-exact.
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: r.clamp(0.0, 1.0),
                            g: g.clamp(0.0, 1.0),
                            b: b.clamp(0.0, 1.0),
                            a: a.clamp(0.0, 1.0),
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        queue.submit(std::iter::once(encoder.finish()));
        // Read back the top-left pixel from the GPU (proves the clear landed,
        // and feeds read_pixel with the SAME packing the mock uses → parity).
        let packed = self.read_back_top_left(surf)?;
        self.surfaces[surf as usize].last_pixel = Some(packed);
        Ok(GfxValue::Unit)
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

    fn read_pixel(&self, surf: i64) -> GfxResult {
        self.check_surface(surf)?;
        // The color last read back from the GPU after a clear; 0 if never cleared.
        Ok(GfxValue::Int(
            self.surfaces[surf as usize].last_pixel.unwrap_or(0),
        ))
    }

    /// Copy the surface's offscreen texture into a mappable buffer, map it, and
    /// pack the top-left texel `0xRRGGBBAA`. This is the REAL GPU readback — the
    /// headless render gate's acceptance check. Any wgpu failure is a graceful
    /// `Err` (I-4), never a panic.
    fn read_back_top_left(&self, surf: i64) -> Result<i64, String> {
        let device = self.device.as_ref().expect("device");
        let queue = self.queue.as_ref().expect("queue");
        let slot = &self.surfaces[surf as usize];
        // wgpu requires bytes-per-row aligned to COPY_BYTES_PER_ROW_ALIGNMENT.
        let unpadded = slot.width * BYTES_PER_PIXEL;
        let padded = unpadded.div_ceil(COPY_ALIGN) * COPY_ALIGN;
        let buf_size = (padded * slot.height) as u64;
        let read_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("axon-gfx readback"),
            size: buf_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("axon-gfx readback encoder"),
        });
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &slot.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &read_buf,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(slot.height),
                },
            },
            wgpu::Extent3d {
                width: slot.width,
                height: slot.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(std::iter::once(encoder.finish()));

        // Map the buffer and wait for the GPU. Block via poll loop.
        let slice = read_buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res);
        });
        // Drive the device until the map callback fires.
        device.poll(wgpu::Maintain::Wait);
        match rx.recv() {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(format!("native::gfx: buffer map failed: {e}")),
            Err(_) => return Err("native::gfx: buffer map channel closed".to_string()),
        }
        let data = slice.get_mapped_range();
        if data.len() < 4 {
            return Err("native::gfx: readback shorter than one texel".to_string());
        }
        // Top-left texel = bytes [0..4] = R,G,B,A (Rgba8Unorm byte order).
        let r = data[0] as f64 / 255.0;
        let g = data[1] as f64 / 255.0;
        let b = data[2] as f64 / 255.0;
        let a = data[3] as f64 / 255.0;
        drop(data);
        read_buf.unmap();
        Ok(pack_rgba8(r, g, b, a))
    }

    fn surface_close(&mut self, surf: i64) -> GfxResult {
        self.check_surface(surf)?;
        let slot = &mut self.surfaces[surf as usize];
        slot.live = false;
        // Drop the GPU texture by replacing the slot's liveness; the Texture is
        // freed when the slot is overwritten on a future run. We keep the slot
        // (index stability) but mark it dead so reuse is a graceful Err.
        Ok(GfxValue::Unit)
    }

    fn window_close(&mut self, win: i64) -> GfxResult {
        self.check_window(win)?;
        self.windows[win as usize].live = false;
        Ok(GfxValue::Unit)
    }

    fn check_window(&self, win: i64) -> Result<WindowSlot, String> {
        match self
            .windows
            .get(usize::try_from(win).map_err(|_| bad_handle())?)
        {
            Some(s) if s.live => Ok(*s),
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

    /// Dispatch a resolved `gfx` fn by name — the SAME signature as
    /// `axon_gfx_mock::GfxMock::dispatch`, so the interpreter routes to either
    /// backend interchangeably. A bad handle index here is STILL a graceful
    /// `Err` (I-4 defense in depth), never a panic/abort.
    pub fn dispatch(&mut self, fnname: &str, args: &[GfxArg]) -> GfxResult {
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
                "native::gfx (wgpu): bad call `{fnname}` (wrong argument shape)"
            )),
        }
    }
}

fn bad_handle() -> String {
    "native::gfx: invalid or consumed handle (forged or stale index)".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The headless render round-trip — REQUIRES a usable (software) Vulkan
    /// backend (lavapipe). Skips gracefully if none is available so the default
    /// `cargo test` never fails on a GPU-less host. The render-gate script is
    /// the authoritative end-to-end check; this is the crate-level smoke test.
    #[test]
    fn headless_clear_readback() {
        let mut g = GfxReal::new();
        let win = match g.dispatch(
            "window_open",
            &[GfxArg::Int(64), GfxArg::Int(64), GfxArg::Str("t".into())],
        ) {
            Ok(GfxValue::Handle { payload, .. }) => payload,
            Ok(_) => unreachable!(),
            Err(e) => {
                eprintln!("SKIP headless_clear_readback: no GPU adapter ({e})");
                return;
            }
        };
        let wh = GfxArg::Handle {
            tag: axon_gfx_mock::tag_for("Window"),
            payload: win,
        };
        let surf = match g.dispatch("surface", std::slice::from_ref(&wh)).unwrap() {
            GfxValue::Handle { payload, .. } => payload,
            _ => unreachable!(),
        };
        let sh = GfxArg::Handle {
            tag: axon_gfx_mock::tag_for("Surface"),
            payload: surf,
        };
        // Clear to r=0, g=128/255, b=1, a=1 → 0x0080FFFF.
        g.dispatch(
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
        g.dispatch("present", std::slice::from_ref(&sh)).unwrap();
        let px = match g.dispatch("read_pixel", std::slice::from_ref(&sh)).unwrap() {
            GfxValue::Int(n) => n,
            _ => unreachable!(),
        };
        assert_eq!(
            px, 0x0080_FFFF,
            "real GPU readback must match the clear color"
        );
    }

    #[test]
    fn forged_handle_is_graceful_err() {
        // No device needed: the index check fires before any GPU work.
        let g = GfxReal::new();
        for bad in [9999i64, -1, i64::MIN, i64::MAX, 0] {
            assert!(g.check_surface(bad).is_err(), "forged index {bad} must Err");
            assert!(g.check_window(bad).is_err(), "forged index {bad} must Err");
        }
    }
}
