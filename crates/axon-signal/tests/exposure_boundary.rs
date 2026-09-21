//! The dashboard's network exposure, asserted at the source.
//!
//! Two separate questions, and fixing one does not fix the other:
//!   1. ledger/RBAC FILTERING — what records a read returns;
//!   2. network EXPOSURE — who can reach the endpoint at all.
//!
//! The dashboard bound `0.0.0.0` with `Access-Control-Allow-Origin: *` and
//! served every engineer's session goals to a caller supplying no identity.
//! Binding to loopback closes the network half; the wildcard CORS header is the
//! worse half ON a loopback server, because it hands the endpoint to every page
//! the operator's own browser visits.

use std::path::Path;

fn dashboard_src() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/dashboard.rs"))
        .expect("read dashboard.rs")
}

#[test]
fn the_dashboard_binds_loopback_not_every_interface() {
    let src = dashboard_src();
    assert!(
        src.contains("format!(\"127.0.0.1:{port}\")"),
        "the dashboard must bind loopback; axon-web does, and this served every \
         engineer's data to the network when it did not"
    );
    let bad: Vec<_> = src
        .lines()
        .enumerate()
        .filter(|(_, l)| l.contains("0.0.0.0") && !l.trim_start().starts_with("//"))
        .map(|(i, l)| format!("dashboard.rs:{}: {}", i + 1, l.trim()))
        .collect();
    assert!(bad.is_empty(), "binds every interface:\n{}", bad.join("\n"));
}

#[test]
fn the_dashboard_sets_no_wildcard_cors_header() {
    let src = dashboard_src();
    let bad: Vec<_> = src
        .lines()
        .enumerate()
        .filter(|(_, l)| {
            l.contains("Access-Control-Allow-Origin") && !l.trim_start().starts_with("//")
        })
        .map(|(i, l)| format!("dashboard.rs:{}: {}", i + 1, l.trim()))
        .collect();
    assert!(
        bad.is_empty(),
        "a wildcard CORS header on a loopback server lets any page the operator \
         visits read this endpoint cross-origin. The dashboard serves its own \
         HTML from this origin and needs no CORS header:\n{}",
        bad.join("\n")
    );
}
