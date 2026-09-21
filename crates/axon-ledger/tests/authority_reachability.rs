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

/// The choke point, asserted structurally rather than behaviourally.
///
/// Patching the nine existing handlers would have fixed nine and left the tenth
/// to be written without a check. The filter lives inside `Store::all` — the
/// only reader — so a handler added later cannot forget it, PROVIDED the MCP
/// layer never opens an unfiltered handle. That proviso is what this test
/// guards, and it is the part a behavioural test cannot cover: a behavioural
/// test only sees the handlers that exist today.
#[test]
fn the_mcp_layer_cannot_open_an_unfiltered_store() {
    let src = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/mcp.rs"))
        .expect("read mcp.rs");
    // Guard the guard: if the file stopped containing any Store open at all,
    // this test would pass while asserting nothing.
    assert!(
        src.contains("Store::open_as("),
        "mcp.rs no longer opens a filtered store — this test's premise is gone, not satisfied"
    );
    let unfiltered: Vec<_> = src
        .lines()
        .enumerate()
        // `open_for_write` is the AUDITABLE exception: ingest and refresh must
        // see every record. It is a distinct name precisely so this guard can
        // tell a deliberate unfiltered read from a forgotten authorization —
        // an exception that looks identical to the bug is not an exception.
        .filter(|(_, l)| l.contains("Store::open(") && !l.trim_start().starts_with("//"))
        .map(|(i, l)| format!("mcp.rs:{}: {}", i + 1, l.trim()))
        .collect();
    assert!(
        unfiltered.is_empty(),
        "the MCP layer opened an UNFILTERED store, which bypasses RBAC for every \
         handler using it. Use Store::open_as(dir, caller).\n{}",
        unfiltered.join("\n")
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
