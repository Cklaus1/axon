# iOS parity harness (R14 — macOS-host-required remainder)

This directory is the **scaffold** for running the Axon iOS compute-parity oracle
and the `ios_sim_window_clear` Metal journey on a booted iOS Simulator, invoked
by `.github/workflows/ios.yml`.

## Honest status (read this first)

The pieces in this repo that are **host-agnostic and Linux-authored/-verified**:

- the byte-stable Swift wrapper generator (`crates/axon-core/src/mobile.rs`),
- the iOS `AxonRuntime` lifecycle bridge core + its `#[no_mangle] extern "C"`
  entry points (`crates/axon-rt/src/mobile_ios.rs`, `cfg(target_os="ios")`),
- the compute-parity oracle program + its expected output
  (`examples/mobile/ios_compute_parity.ax` → `ios-parity gcd=21 fib=6765
  sum=5050 mix=115`),
- the CI workflow that drives all of the above on Apple hardware.

What **genuinely requires a macOS host with Xcode to author exactly** (and was
NOT created on the Linux dev host, by design — it cannot be):

1. **`Package.swift` / the `.xcodeproj`** wiring the Axon-compiled compute object
   + the generated `Axon.swift` shim + the `AxonRuntime` Swift support library
   into a runnable iOS-Simulator app/test target. The exact SwiftPM platform
   conditionals, the `XCTest` bundle layout, and the signing-free simulator build
   settings depend on the installed Xcode and must be validated against it.
2. **`run_on_sim.sh`** — the concrete `xcrun simctl install` / `xcrun simctl
   launch` (or `xcodebuild test -destination 'platform=iOS Simulator,...'`)
   incantation that runs the harness on the booted simulator and captures its
   stdout for the `$EXPECTED` diff. The exact flags are Xcode-version-specific.
3. The **`AxonRuntime` Swift support library** (the hand-written, versioned bridge
   that conforms to `UIApplicationDelegate`, owns the run loop, and calls the
   `axon_ios_*` C-ABI entry points). Its Rust-callable contract is fully
   specified by `crates/axon-rt/src/mobile_ios.rs`; the Swift side is authored on
   macOS against that contract.

The workflow degrades gracefully: if this harness's `run_on_sim.sh` is absent it
emits a CI `::warning::` and still verifies the oracle + the cross-build + the
wrapper + the bridge symbols — so the macOS job is useful immediately and the
simulator-app harness can be filled in by a macOS contributor.

## The contract the harness binds to (so the macOS author has everything)

The generated `out/ios/Axon.swift` shim declares (via `@_silgen_name`) the app's
exported C-ABI entries (`on_start`/`on_frame`/`on_resume`). The `AxonRuntime`
bridge calls the runtime entry points exported by `libaxon_rt.a`:

| C symbol (`axon-rt`, `cfg(target_os="ios")`) | iOS lifecycle hook |
|---|---|
| `axon_ios_runtime_new(start, frame, resume)` | `application:didFinishLaunching` |
| `axon_ios_start(ctx) -> i64`                 | run `on_start` |
| `axon_ios_frame(ctx) -> i64`                 | `CADisplayLink` tick → `on_frame` |
| `axon_ios_suspend() -> i64`                  | `sceneDidEnterBackground` |
| `axon_ios_resume(ctx) -> i64`                | `sceneWillEnterForeground` → `on_resume` |
| `axon_ios_teardown() -> i64`                 | `applicationWillTerminate` |

A negative return is an invalid-transition status the bridge surfaces as a clean
app-level error (it never panics across the FFI line — invariant I-4).

For the **compute-parity** oracle (no lifecycle), the harness can link the
Axon-compiled `main` object directly and assert its single stdout line equals
`$EXPECTED` — that is the minimal macOS-authored piece needed to close
acceptance §9's iOS compute-parity tier.
