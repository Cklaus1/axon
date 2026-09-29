//! `axon-provenance [DIR]`: the build provenance of the tree containing DIR
//! (default `.`), as `axon-provenance/1` JSON on stdout:
//! `{"schema", "revision", "dirty": [reasons]}`. Empty `dirty` means clean.
//!
//! This is `src/provenance.rs` over `src/git_data.rs`, the SAME code
//! `build.rs` stamps the readiness verifier with, so the guest-image
//! manifest's `axon_tree_dirty_at_build` (scripts/linux_profile_manifest.py,
//! which compiles this file with `rustc` directly) and the verifier's
//! `source_dirty` have one implementation. It depends on `std` only.

#[path = "../git_data.rs"]
#[allow(dead_code)]
mod git_data;
#[path = "../provenance.rs"]
#[allow(dead_code)]
mod provenance;

fn json_str(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn main() {
    let dir = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let p = provenance::provenance(std::path::Path::new(&dir));
    let dirty: Vec<String> = p.dirty.iter().map(|d| json_str(d)).collect();
    println!(
        "{{\"schema\":\"axon-provenance/1\",\"revision\":{},\"dirty\":[{}]}}",
        json_str(&p.revision),
        dirty.join(",")
    );
}
