//! The dispatch rule's OFF switch (`DISPATCH_RULE_OFF`, amendment 83) exists so the unit tests of the
//! OTHER seal layers can observe their attacks through a dispatch the rule would refuse first. It must
//! not exist in any build that judges a suite: no `axon` binary, no library user, no env or CLI path.
//! This reads the source (the only place a production build could acquire it) and requires that every
//! mention is compiled under `#[cfg(test)]`, and that nothing outside interp.rs names it.

use std::path::Path;

fn src(rel: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

#[test]
fn the_dispatch_rule_off_switch_is_compiled_only_into_unit_tests() {
    let interp = src("src/interp.rs");
    let lines: Vec<&str> = interp.lines().collect();
    let tests_at = lines
        .iter()
        .position(|l| l.trim() == "mod tests {")
        .expect("interp.rs has its unit-test module");
    assert!(
        lines[tests_at - 1].trim() == "#[cfg(test)]",
        "setup: the unit-test module is cfg(test)"
    );
    let mut seen = 0;
    for (i, l) in lines.iter().enumerate() {
        if !l.contains("DISPATCH_RULE_OFF") || l.trim_start().starts_with("//") {
            continue;
        }
        seen += 1;
        if i > tests_at {
            continue; // inside the cfg(test) module
        }
        // Outside the module: the line (a declaration or a read) must sit directly under #[cfg(test)]
        // (a `static` declaration spans two lines, the second naming the same item).
        let attr_line = if l.trim_start().starts_with("std::sync::atomic") {
            i - 2
        } else {
            i - 1
        };
        assert!(
            lines[attr_line].trim() == "#[cfg(test)]",
            "ATTACK: the dispatch rule's off switch is reachable from a production build (interp.rs:{} is not under #[cfg(test)]): {}",
            i + 1,
            l.trim()
        );
    }
    assert!(
        seen >= 4,
        "setup: the switch's mentions were found ({seen})"
    );
    // No other source file, and no env or CLI reading, names it.
    for f in [
        "src/main.rs",
        "src/lib.rs",
        "src/interp/pin.rs",
        "src/interp/eval.rs",
        "src/interp/conform.rs",
    ] {
        let s = src(f);
        assert!(
            !s.contains("DISPATCH_RULE_OFF"),
            "ATTACK: {f} names the dispatch rule's off switch"
        );
    }
}
