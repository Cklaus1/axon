//! Every authority-bearing ledger read must reach RBAC.
//!
//! The governing invariant: an authoritative property may be reported as
//! satisfied only if execution REACHED the canonical mechanism that establishes
//! it. The MCP server violated it completely — it opened a raw `Store` and
//! handed it to nine tool handlers, none of which mentioned rbac, so the
//! interface an agent actually talks to returned every principal's records to
//! any caller. REPRODUCED: on one ledger with RBAC active, `--as
//! bob@example.com` through the CLI reported 1 record and the same identity
//! through `tools/call ledger_stats` reported 2.

use std::path::Path;

/// The choke point, asserted across the WHOLE WORKSPACE.
///
/// The first version of this guard read exactly one file —
/// `axon-ledger/src/mcp.rs` — so it could not see another crate, and
/// `Store::open` stayed `pub` and unfiltered. `axon-signal` then read the same
/// ledger through it on every path: its CLI, its OWN MCP server, and an HTTP
/// dashboard bound to 0.0.0.0 with `Access-Control-Allow-Origin: *` that
/// returned every engineer's session goals to an unauthenticated caller.
///
/// That is the exact failure the fix claimed to prevent. The comment said "a
/// handler added later cannot forget"; a whole different CRATE could, and had.
/// A guard scoped to one file proves a property about one file.
///
/// WHAT THIS TEST DOES NOT PROVE. It asks whether `Store::open(` is SPELLED
/// anywhere outside store.rs. That is a naming property, not an authorization
/// one, and the difference is not academic: `open_for_write` — deliberately
/// unfiltered, and invisible to this grep — was used on five of the CLI's
/// eight read paths, so `--as bob diff --json` returned another principal's
/// payload while this test reported 2 passed. A guard that cannot fail on the
/// live bug it is named for is worse than no guard, because it is counted.
///
/// The behavioural counterpart is `read_commands_filter.rs`, which runs every
/// read command as a member against two ledgers and requires the outputs to
/// be identical. Keep BOTH: this one is a cheap structural tripwire for a new
/// crate reaching for a raw handle; that one decides whether reads are
/// actually filtered. Mutation-verified at 10/10 read sites; this grep caught
/// 0 of those 10.
#[test]
fn no_crate_in_the_workspace_opens_an_unfiltered_ledger_store() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf();
    let mut offenders = Vec::new();
    let mut scanned = 0usize;
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                // Skip build output and vendored trees, not source.
                let name = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if name != "target" && !name.starts_with('.') {
                    stack.push(p);
                }
                continue;
            }
            if p.extension().and_then(|x| x.to_str()) != Some("rs") {
                continue;
            }
            let Ok(src) = std::fs::read_to_string(&p) else {
                continue;
            };
            if !src.contains("Store::open") {
                continue;
            }
            // store.rs DEFINES the constructors; `open_as` and `open_for_write`
            // both delegate to `open`, so the defining file necessarily names
            // it. Skipping it is not a loophole: `store_has_exactly_one_reader`
            // guards that file's own invariant.
            if p.ends_with("store.rs") {
                continue;
            }
            // Scan only what is BUILT. `crates/axon-ledger/axon-ledger/` is a
            // git-tracked duplicate of this crate nested inside itself, absent
            // from the workspace members and referenced by nothing — dead code
            // that still ships. It is reported separately rather than silently
            // scanned, because a guard that fails on unbuilt code trains people
            // to ignore it.
            if p.to_string_lossy().contains("axon-ledger/axon-ledger/") {
                continue;
            }
            scanned += 1;
            // Test modules legitimately construct raw stores to seed fixtures:
            // they are not an authority surface and have no caller to filter
            // for. Everything else must name `open_as` (read) or
            // `open_for_write` (ingest/prune), so the intent is auditable.
            // POSITIONAL, not path-based. The first version required a
            // `tests/` path component, which missed `#[cfg(test)] mod` blocks
            // living inside `src/*.rs` — four real sites in axon-signal's
            // rework.rs. A line after the `#[cfg(test)]` marker is test code
            // wherever the file sits.
            let test_from = src.find("#[cfg(test)]").unwrap_or(usize::MAX);
            let mut off = 0usize;
            for (i, l) in src.lines().enumerate() {
                let here = off;
                off += l.len() + 1;
                let t = l.trim_start();
                if t.starts_with("//") || !l.contains("Store::open(") {
                    continue;
                }
                if here > test_from {
                    continue;
                }
                offenders.push(format!("{}:{}: {}", p.display(), i + 1, l.trim()));
            }
        }
    }
    // Guard the guard: if nothing referenced Store::open at all, this test
    // would pass while asserting nothing about anything.
    assert!(
        scanned >= 2,
        "expected several files to reference Store::open; scanned {scanned} — \
         this test's premise is gone, not satisfied"
    );
    assert!(
        offenders.is_empty(),
        "these sites open an UNFILTERED ledger store, bypassing RBAC for every \
         read they perform. Use Store::open_as(dir, caller) to read, or \
         Store::open_for_write(dir) for ingest/prune so the exception is \
         auditable:\n{}",
        offenders.join("\n")
    );
}

/// `Store::all` is the sole file reader, which is what makes filtering there a
/// choke point rather than one more place to remember. If another reader starts
/// opening the events file directly, the choke point silently stops covering it.
#[test]
fn store_has_exactly_one_reader_so_the_filter_covers_every_query() {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store.rs"))
        .expect("read store.rs");
    let readers = src.matches("File::open(&self.events_path)").count();
    assert_eq!(
        readers, 1,
        "expected exactly ONE direct read of the events file (inside Store::all, \
         where the RBAC view is applied); found {readers}. A second reader would \
         bypass the filter for whatever calls it."
    );
}
