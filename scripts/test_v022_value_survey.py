#!/usr/bin/env python3
"""The value survey's own rules (amendment 110), with no cargo: the mutation rules for DEFAULT values, and that an
entry naming two test binaries runs BOTH.

    python3 scripts/test_v022_value_survey.py
"""
import os
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__))))
import v022_value_survey as vs  # noqa: E402


def main():
    bad = 0

    def eq(what, got, want):
        nonlocal bad
        if got != want:
            print(f"FAIL {what}: got {got!r}, want {want!r}")
            bad += 1

    # the default flips to the OTHER value (the fail-open direction), not to an arbitrary one
    eq("Mode::Dev flips to Protected", vs.mutate("val_default", "Mode::Dev", ""), "Mode::Protected")
    eq("a qualified variant keeps its path", vs.mutate("val_default", "crate::psv::EvidenceClass::GuestUnobserved", ""),
       "crate::psv::EvidenceClass::Protected")
    eq("a count moves", vs.mutate("val_default", "0", ""), "1")
    eq("a bool flips", vs.mutate("val_default", "true", ""), "false")
    eq("an empty string is no longer empty", vs.mutate("val_default", '""', ""), '"x"')
    eq("a path fallback moves", vs.mutate("val_default", 'Path::new(".")', ""), 'Path::new(".x")')
    eq("i64::MIN / 2 moves to the other end", vs.mutate("val_default", "i64::MIN / 2", ""), "i64::MAX / 2")
    eq("unwrap_or_default becomes a panic when taken", vs.mutate("val_default", ".unwrap_or_default()", ""),
       '.into_iter().next().unwrap_or_else(|| panic!("eq8 default"))')
    eq("Default::default becomes a panic when taken", vs.mutate("val_default", "Default::default()", ""),
       '{ panic!("eq8 default") }')
    eq("an unknown enum variant has no rule (MANUAL, never a silent pass)", vs.mutate("val_default", "Foo::Bar", ""), None)
    # amendment 115: the absent arm of a combinator answers the other way; matches! stops matching the absent case
    eq("is_none_or becomes is_some_and", vs.mutate("val_comb", ".is_none_or(|t| now >= t)", ""), ".is_some_and(|t| now >= t)")
    eq("is_some_and becomes is_none_or", vs.mutate("val_comb", ".is_some_and(|s| s != pid)", ""), ".is_none_or(|s| s != pid)")
    eq("is_ok_and gives Err the true answer", vs.mutate("val_comb", ".is_ok_and(|m| m.is_dir())", ""), ".map_or(true, |m| m.is_dir())")
    eq("is_err_and gives Ok the true answer", vs.mutate("val_comb", ".is_err_and(|e| e.is_empty())", ""),
       ".map_or_else(|e| e.is_empty(), |_| true)")
    eq("matches! drops its absent alternative", vs.mutate("val_comb", "matches!(v.exit_code, Some(0) | None)", ""),
       "matches!(v.exit_code, Some(0))")
    eq("matches! over a lone None stops matching it", vs.mutate("val_comb", "matches!(x, None)", ""), "matches!(x, Some(_))")
    eq("an or_else literal moves", vs.mutate("val_comb", "9191", ""), "9192")
    eq("a match-arm variant flips to the fail-open one", vs.mutate("val_arm", "EvidenceClass::GuestUnobserved", ""),
       "EvidenceClass::Protected")
    eq("a let-else literal flips", vs.mutate("val_arm", "false", ""), "true")
    eq("a combinator with no rule is MANUAL, never a silent pass", vs.mutate("val_comb", "matches!(x)", ""), None)
    # signing inputs: a string moves, a const is replaced
    eq("a signed string moves", vs.mutate("flow_sign", 'b"axon-loop ledger key v1"', ""), 'b"axon-loop ledger key v1x"')
    eq("a signed const is replaced", vs.mutate("flow_sign", "CLEARANCE_DOMAIN", ""), '"/tmp"')

    # an entry that names two test binaries runs BOTH: the first survives, the second kills
    ran = []

    def runner(cmd):
        ran.append(cmd)
        return {"t_second"} if cmd == "second" else set()
    failing, results = vs.run_commands(["first", "second", "third"], runner)
    eq("both named binaries ran", ran, ["first", "second"])
    eq("the second one's kill is the entry's kill", failing, {"t_second"})
    ran.clear()
    failing, _ = vs.run_commands(["first", "second"], lambda c: ran.append(c) or set())
    eq("a survivor ran every command", ran, ["first", "second"])
    eq("... and survives", failing, set())
    print("test_v022_value_survey:", "FAIL" if bad else "PASS")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
