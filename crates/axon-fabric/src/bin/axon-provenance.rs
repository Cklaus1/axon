//! `axon-provenance [DIR]`: the build provenance of the tree containing DIR
//! (default `.`), as `axon-provenance/1` JSON on stdout:
//! `{"schema", "revision", "dirty": [reasons]}`. Empty `dirty` means clean.
//!
//! `axon-provenance --descends REV [DIR]`: exit 0 only if HEAD descends from
//! REV, asked of the same hardened git (the guest build's early PCI lineage
//! check). A DEVELOPMENT answer: a linked worktree is accepted.
//!
//! `axon-provenance --lineage REV [DIR]`: the provenance answer plus
//! `"lineage": {"rev", "descends", "why"}`, the PROTECTED lineage answer
//! (decision E: a gitfile or linked worktree is refused, never descends).
//! The guest manifest binds this one, so a development lineage check can
//! never make a manifest clean.
//!
//! This is `src/provenance.rs` over `src/git_data.rs`, the SAME code
//! `build.rs` stamps the readiness verifier with, so the guest-image
//! manifest's `axon_tree_dirty_at_build` (scripts/linux_profile_manifest.py,
//! which compiles this file with `rustc` directly) and the verifier's
//! `source_dirty` have one implementation. It depends on `std` only.

// git_data's own tests use the shared repository attacks.
#[path = "../git_data.rs"]
#[allow(dead_code)]
mod git_data;
#[path = "../provenance.rs"]
#[allow(dead_code)]
mod provenance;
#[cfg(test)]
#[path = "../../tests/common/git_attacks.rs"]
mod test_git_attacks;

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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--descends") {
        let rev = args.get(1).map(String::as_str).unwrap_or("");
        let dir = args.get(2).map(String::as_str).unwrap_or(".");
        if let Err(e) = provenance::descends_from(std::path::Path::new(dir), rev) {
            eprintln!("axon-provenance: {e}");
            std::process::exit(1);
        }
        return;
    }
    let (lineage, rest) = match args.first().map(String::as_str) {
        Some("--lineage") => (
            Some(args.get(1).cloned().unwrap_or_default()),
            args.get(2..).unwrap_or(&[]),
        ),
        _ => (None, &args[..]),
    };
    let dir = rest.first().cloned().unwrap_or_else(|| ".".into());
    let dir = std::path::Path::new(&dir);
    let p = provenance::provenance(dir);
    let dirty: Vec<String> = p.dirty.iter().map(|d| json_str(d)).collect();
    let lineage = lineage.map(|rev| {
        let (descends, why) = match provenance::descends_from_protected(dir, &rev) {
            Ok(()) => (true, String::new()),
            Err(e) => (false, e),
        };
        format!(
            ",\"lineage\":{{\"rev\":{},\"descends\":{descends},\"why\":{}}}",
            json_str(&rev),
            json_str(&why)
        )
    });
    println!(
        "{{\"schema\":\"axon-provenance/1\",\"revision\":{},\"dirty\":[{}]{}}}",
        json_str(&p.revision),
        dirty.join(","),
        lineage.unwrap_or_default()
    );
}
