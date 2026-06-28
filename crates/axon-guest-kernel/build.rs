use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let boot_s = "src/boot.s";
    let boot_o = out_dir.join("boot.o");

    // Compile boot.s with GNU assembler — LLVM IAS has quirks with .code32 +
    // AT&T syntax under certain nightly versions; GAS compiles it cleanly.
    let status = Command::new("as")
        .args([
            "-64",                        // x86_64 output
            "-o", boot_o.to_str().unwrap(),
            boot_s,
        ])
        .status()
        .expect("failed to run `as` (GNU assembler) — install binutils");

    if !status.success() {
        panic!("GNU assembler failed on {}", boot_s);
    }

    // Pass the object file to the Rust linker.
    println!("cargo:rustc-link-arg={}", boot_o.display());

    // Re-run this build script if boot.s or the linker script change.
    println!("cargo:rerun-if-changed=src/boot.s");
    println!("cargo:rerun-if-changed=linker.ld");
}
