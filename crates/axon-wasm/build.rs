// build.rs — browser-target link config for the axon-wasm interpreter cdylib.
//
// The browser substrate (wasm32-unknown-unknown) reaches the JS host through one
// imported symbol, `axon_host_await` (R15 §13 B3 Asyncify binding). That symbol is
// resolved by the JS host at instantiation, so it MUST be left undefined at link
// time. rust-lld (wasm-ld) defaults to erroring on undefined symbols, which broke
// `cargo build -p axon-wasm --target wasm32-unknown-unknown` (and skipped the
// wasm_browser_interp_parity gate). `--allow-undefined` tells wasm-ld to emit the
// undefined symbol as a wasm import instead of failing — the same allowance the
// passing browser harnesses already make via `axon target build`.
//
// Scoped to the browser wasm target only; native/wasip1 builds are untouched.
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target == "wasm32-unknown-unknown" {
        println!("cargo:rustc-link-arg-cdylib=--allow-undefined");
    }
}
