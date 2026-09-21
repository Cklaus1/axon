//! A builtin whose IMPLEMENTATION touches the world must be classified.
//!
//! The two existing table guards ask table-vs-table questions: "does a builtin
//! with a Net/Exec effect row get classified" and "does a classified builtin
//! declare a row". Both are blind to a builtin missing from BOTH tables — and
//! `dstore_open`/`dstore_apply`/`dstore_clear` were exactly that, calling
//! `std::fs` directly while every capability mechanism was told they were pure.
//!
//! So this guard does not consult a table for the premise. It reads the
//! interpreter's own source and asks: does this builtin's arm touch the
//! filesystem, spawn a process, or read stdin — and if so, does
//! `capability_of_builtin` know about it?
//!
//! KNOWN LIMIT, stated rather than left to be discovered: attribution is to
//! the ENCLOSING match arm, so a builtin that reaches the world through a
//! helper function defined elsewhere in the file is not seen. This catches the
//! direct case, which is the one that shipped. A guard that cannot see
//! everything is worth having; a guard that implies it can is not.

use axon_core::capabilities::capability_of_builtin;

/// Builtins that reach the world inside their own match arm, by name.
fn worldly_builtins() -> Vec<String> {
    let src = include_str!("../src/interp/builtins.rs");
    const WORLDLY: &[&str] = &["std::fs::", "std::process::Command", "std::io::stdin("];
    let mut current: Vec<String> = Vec::new();
    let mut depth_at_arm: Option<usize> = None;
    let mut depth = 0usize;
    let mut found: Vec<String> = Vec::new();
    for line in src.lines() {
        let code = line.split("//").next().unwrap_or(line);
        if code.contains("=> {") && code.trim_start().starts_with('"') {
            let names: Vec<String> = code
                .split("=>")
                .next()
                .unwrap_or("")
                .split('|')
                .filter_map(|p| {
                    let p = p.trim();
                    p.strip_prefix('"')
                        .and_then(|p| p.strip_suffix('"'))
                        .map(String::from)
                })
                .collect();
            if !names.is_empty() {
                current = names;
                depth_at_arm = Some(depth);
            }
        }
        if let Some(arm_depth) = depth_at_arm {
            if WORLDLY.iter().any(|t| code.contains(t)) {
                found.extend(current.iter().cloned());
            }
            if depth < arm_depth {
                depth_at_arm = None;
                current.clear();
            }
        }
        depth += code.matches('{').count();
        depth = depth.saturating_sub(code.matches('}').count());
    }
    found.sort();
    found.dedup();
    found
}

#[test]
fn every_builtin_whose_arm_touches_the_world_is_capability_classified() {
    let unclassified: Vec<String> = worldly_builtins()
        .into_iter()
        .filter(|n| capability_of_builtin(n).is_none())
        .collect();
    assert!(
        unclassified.is_empty(),
        "these builtins reach the world in their own match arm but \
         `capability_of_builtin` returns None for them, so `@[contained]`, the \
         scoped sandbox and the ambient effect ceiling all treat them as pure: \
         {unclassified:?}"
    );
}

/// The guard must be able to fail — a scanner that matches nothing passes
/// everything.
///
/// The premise that matters is ARM ATTRIBUTION, not a raw line count: the
/// first version of this asserted ">= 5 worldly call sites" and failed at 4,
/// which proved only that I guessed a threshold. The interesting fact is that
/// exactly one builtin family reaches `std::fs` in its own arm — every other
/// builtin goes through the `AxonHost` seam — so the guard is pinned to
/// finding that family by NAME. If a refactor breaks arm detection, this
/// fails instead of the guard above silently passing.
#[test]
fn the_scanner_attributes_worldly_calls_to_the_right_builtin() {
    let found = worldly_builtins();
    assert!(
        found.iter().any(|n| n.starts_with("dstore_")),
        "the scanner no longer attributes the durable store's `std::fs` calls \
         to its match arm, so the guard above cannot see this class at all; \
         it found: {found:?}"
    );
}
