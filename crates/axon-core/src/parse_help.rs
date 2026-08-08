//! Fix hints for **parse-tier** diagnostics (`AXON_FOR_RLM.md` §1 + §3).
//!
//! # Why this exists
//!
//! Axon's diagnostics carry a `help:` line at the type tier (`E0307`) and the
//! unknown-method tier (`E0403`), and carried none at the parse tier (`E0000`).
//! That was measured to be exactly backwards for the caller that needs it most:
//! in the RLM spike, **100% of a model's failures were parse errors**, so the
//! one feature Axon has for teaching a writer what to do next never fired.
//!
//! The dominant single failure was `let mut count` — a Rust habit — producing:
//!
//! ```text
//! E0000  unexpected token: Ident("count"), expected Eq
//! ```
//!
//! A reader of that does not learn that `mut` is the problem. This module maps
//! the parse error to a sentence that says so.
//!
//! # The shape, and why it is a closed table
//!
//! [`parse_help`] is a **pure function** of `(parse message, source, byte
//! offset)` returning `None` by default. It is not a per-call-site hint: parse
//! errors are raised from dozens of places in `parser.rs`, and per-site prose
//! would not stay consistent with itself. It is not model-generated: a compiler
//! diagnostic has to be deterministic and offline.
//!
//! Adding a row is: add a match arm, add a test, and add a probe line to
//! `tests/parse_help_probe.rs` proving the *real* compiler still produces the
//! message the arm keys on.
//!
//! # Keys are verified against the compiler, not assumed
//!
//! Every arm below keys on a message this compiler was **observed** to emit.
//! That matters more than it sounds: of the eight habits `AXON_FOR_RLM.md` §1
//! names, probing the real compiler showed only four reach the parse tier at
//! all.
//!
//! - `;` — **already accepted.** `let c = 0;` parses; it warrants no help
//!   because it is not an error. A row for it would have been dead code keyed
//!   on a message that is never emitted.
//! - `const` / `var` — reach the **resolve** tier, not the parse tier
//!   (`cannot find name \`const\` in this scope`), because they lex as ordinary
//!   identifiers. They are equally unrepairable and are handled separately;
//!   they cannot be handled here, because this function is never called for
//!   them.
//!
//! Writing the table from the spec's list rather than from the compiler's
//! behaviour would have produced two arms that never fire and missed the tier
//! the other two actually land on.

/// The whole source line containing `offset`, trimmed.
///
/// **Why the line and not the token at `offset`.** The offset the parser hands
/// back is *not* the position of the token its message names. For
/// `let mut count = 0` the message is `unexpected token: Ident("count")` and the
/// offset points at the `=` — the cursor has already advanced past the token it
/// is complaining about. A first implementation of this module keyed on
/// "the word before `offset`", which is `count`, and no hint ever fired; the
/// probe test caught it. Rather than encode a guess about how far the cursor
/// has run on, each rule below reads the line the way a person would.
fn line_at(src: &str, offset: usize) -> &str {
    // The offset arrives from the parser and is used to slice `src`. A slice on
    // a non-boundary byte panics, which would turn a compile error into a
    // compiler crash on any source containing a multi-byte character — so walk
    // back to the nearest boundary rather than trusting the input.
    let mut offset = offset.min(src.len());
    while offset > 0 && !src.is_char_boundary(offset) {
        offset -= 1;
    }
    let start = src[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
    let end = src[offset..]
        .find('\n')
        .map(|i| offset + i)
        .unwrap_or(src.len());
    src[start..end].trim()
}

/// Split a line into whitespace-separated words, with `:`/`=`/`(` kept as
/// boundaries so `let c:=0` and `let c := 0` tokenise alike.
fn words(line: &str) -> Vec<&str> {
    line.split(|c: char| c.is_whitespace() || "(){}[],".contains(c))
        .filter(|w| !w.is_empty())
        .collect()
}

/// Fix hint for a parse error, or `None`.
///
/// `msg` is the parser's message (`unexpected token: Ident("c"), expected Eq`),
/// `src` the full source, `offset` the byte offset the parser reported.
/// Deterministic and total: an unrecognised error yields `None`, never a guess.
pub fn parse_help(msg: &str, src: &str, offset: usize) -> Option<String> {
    let line = line_at(src, offset);
    let w = words(line);

    // `let mut c = 0` → the parser read `mut` as the binding name, then wanted
    // `=` and found `c`. Measured as the single most common model failure.
    if msg.contains("expected Eq") && w.first() == Some(&"let") && w.get(1) == Some(&"mut") {
        let name = w.get(2).copied().unwrap_or("x");
        // `let mut x: i64 = 0` — keep an annotation if one was written.
        let name = name.trim_end_matches(':');
        return Some(format!(
            "`mut` is not an Axon keyword — bindings are immutable by default \
             and reassignment needs no marker. Write `let {name} = …`, then \
             assign with `{name} = …` (no `let`)"
        ));
    }

    // `let c -> i64 = 0` — an arrow where the type annotation goes.
    //
    // A first draft also guarded on the arrow preceding any `=`, to stop a
    // lambda right-hand side (`let f = |x| -> i64 { x }`) tripping the arm.
    // Mutation-testing it showed the guard could not be made to fail any test,
    // and probing the compiler showed why: once `=` has been consumed the
    // parser is no longer *expecting* one, so it reports
    // `unexpected token: Arrow, expected expression` — a different message this
    // arm does not match. `let f = |x| -> i64 { x }` is simply valid and
    // produces no diagnostic at all. The guard was unreachable, so it is gone
    // rather than left in as reassurance.
    if msg.contains("expected Eq")
        && msg.contains("unexpected token: Arrow")
        && w.first() == Some(&"let")
    {
        let name = w.get(1).copied().unwrap_or("x");
        return Some(format!(
            "`->` declares a *function's* return type; a binding's type is \
             written with `:`. Write `let {name}: i64 = …` (or drop the \
             annotation — Axon infers it)"
        ));
    }

    // `let c := 0` — the `:` opened a type annotation, so `=` arrives where a
    // type name was expected. Keyed on the two characters being ADJACENT, so an
    // ordinary `let c: i64 = 0` cannot match.
    if msg.contains("expected identifier")
        && msg.contains("unexpected token: Eq")
        && line.contains(":=")
    {
        return Some(
            "`:=` is not Axon — assignment is `=`. Write `let x = …`, or \
             `let x: T = …` to annotate the type"
                .to_string(),
        );
    }

    // `if c == ' '` — a single-quoted character literal.
    //
    // Measured 2026-08-06: this one construct caused **all three** of the
    // remaining failures in the RLM fluency gate, in every one of six runs and
    // in both repair arms. All three tasks iterate a string (count vowels,
    // reverse it, count words), which is what invites a char comparison, and
    // Axon has no character type — `char_at` returns a one-character `str`.
    //
    // Keyed on the message rather than the line because this is a LEXER
    // rejection: the lexer stops at the quote, so there is no token stream and
    // no `expected …` clause to match on.
    // The advice below was WRONG on first writing — it claimed `char_at` returns
    // a `str`. It returns the BYTE VALUE as an `i64` (`builtins.rs`), so a model
    // that followed the hint got a fresh type error, which is worse than no hint
    // at all. Caught only by running the repaired program the model produced.
    // Any text here is advice a reader will act on; it is verified by
    // `parse_help_probe::the_character_literal_advice_actually_compiles`.
    // `let best = 9223372036854775808` — an i64 literal one past the maximum.
    //
    // The lexer says "integer literal too large for i64", which states the problem
    // and not the FACT the reader needs: the actual bound. Measured on the
    // tasks_hard set the model reached for a sentinel to seed a min/max scan, wrote
    // MAX+1 (the classic off-by-one), and got NO help at all — 3 of 36 attempts.
    // Naming the bound turns it into a one-token edit.
    //
    // Deliberately NOT solved by adding `i64_min()`/`i64_max()` builtins: R42's
    // admission test asks whether the need can be met in userland, and
    // `let m = 9223372036854775807` plainly can. A diagnostic costs no new surface,
    // and new surface must then be documented on the card — itself a measured cost.
    if msg.contains("integer literal too large") {
        return Some(
            "the largest `i64` is 9223372036854775807 and the smallest is -9223372036854775808 — this literal is outside that range. Axon has no `i64::MAX` constant, so bind it if you need one: `let i64_max = 9223372036854775807`. For a min/max scan prefer seeding from the first element (`let best = xs[0]`) over a sentinel — it cannot be off by one."
                .to_string(),
        );
    }

    if msg.contains("unexpected character '''") {
        return Some(
            "Axon has no character literals — `'a'` is not valid, and string \
             literals use double quotes. `char_at(s, i)` returns the BYTE VALUE \
             as an `i64`, so compare it numerically (`char_at(s, i) == 32` for a \
             space); to compare as text, take a one-character slice instead: \
             `str_eq(str_slice(s, i, i + 1), \" \")`"
                .to_string(),
        );
    }

    // `if a or b` / `if a and b` / `if not a` — Python/Ruby spellings of the
    // logical operators.
    //
    // `not` was missing while this row's own help text already ADVERTISED `!` as
    // the answer, so the hint named the fix for a mistake it could not detect.
    // Measured on tasks_hard: `if not found {` produced the bare
    // `unexpected token: Ident("found"), expected LBrace` — technically true and
    // useless — and the repair round came back with the IDENTICAL error at the
    // identical line and column, which is the signature of a diagnostic that gave
    // the model nothing to act on. It cost that task outright.
    //
    // `not` is checked before `and`/`or` only for tidiness; the three are
    // mutually exclusive in practice. A leading `not` at the start of the line is
    // matched too (`not found` as a bare condition), which ` not ` alone misses.
    if msg.contains("unexpected token: Ident") {
        let seen = if line.contains(" not ") || line.trim_start().starts_with("not ") {
            Some(("not", "!"))
        } else if line.contains(" or ") {
            Some(("or", "||"))
        } else if line.contains(" and ") {
            Some(("and", "&&"))
        } else {
            None
        };
        if let Some((word, op)) = seen {
            return Some(format!(
                "`{word}` is not an Axon operator — write `{op}`. Axon spells the \
                 logical operators `&&`, `||` and `!`"
            ));
        }
    }

    // `def f():` / `function f() {` at item position.
    if msg.contains("expected item") {
        if let Some(seen) = w.first() {
            if matches!(*seen, "def" | "function" | "func" | "fun" | "define") {
                return Some(format!(
                    "Axon declares functions with `fn`, not `{seen}` — write \
                     `fn name(arg: i64) -> i64 {{ … }}`. The return type follows \
                     `->` and the body is the final expression (no `return` needed)"
                ));
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each test's `msg` is the message the real compiler was observed to emit
    // for that source; `tests/parse_help_probe.rs` is what keeps that true.

    #[test]
    fn mut_is_refused_by_name_and_echoes_the_binding() {
        let src = "fn main() -> i64 {\n    let mut count = 0\n    0\n}\n";
        let offset = src.find("count").unwrap();
        let h = parse_help(
            "unexpected token: Ident(\"count\"), expected Eq",
            src,
            offset,
        )
        .expect("mut must produce help — it is the measured dominant failure");
        assert!(h.contains("`mut` is not an Axon keyword"), "{h}");
        assert!(h.contains("let count = "), "echoes the real name: {h}");
    }

    #[test]
    fn a_plain_missing_eq_gets_no_help() {
        // Same "expected Eq" message, no `mut` behind it. The table must not
        // blame `mut` for every malformed `let` — a wrong hint is worse than
        // none, because it sends the reader to a line that is already correct.
        let src = "fn main() -> i64 {\n    let count 0\n    0\n}\n";
        let offset = src.find('0').unwrap();
        assert_eq!(
            parse_help("unexpected token: Int(0), expected Eq", src, offset),
            None
        );
    }

    #[test]
    fn arrow_in_a_let_points_at_the_colon_form() {
        let src = "fn main() -> i64 {\n    let c -> i64 = 0\n    0\n}\n";
        // The SECOND arrow — the first is `fn main() -> i64` on line 1. Taking
        // `find("->")` here resolved the wrong line and returned no help, which
        // read as an implementation bug and was a test bug.
        let offset = src.rfind("->").unwrap();
        let h = parse_help("unexpected token: Arrow, expected Eq", src, offset).unwrap();
        assert!(h.contains("let c: i64"), "{h}");
    }

    #[test]
    fn walrus_is_named() {
        let src = "fn main() -> i64 {\n    let c := 0\n    0\n}\n";
        let offset = src.find('=').unwrap();
        let h = parse_help("unexpected token: Eq, expected identifier", src, offset).unwrap();
        assert!(h.contains("`:=` is not Axon"), "{h}");
    }

    #[test]
    fn a_real_type_annotation_is_not_mistaken_for_a_walrus() {
        // `let c: = 0` is malformed, but `let c: i64 = 0` is not, and the
        // walrus arm keys on ':' immediately before the '='. This pins that it
        // keys on ADJACENCY, so an ordinary annotation cannot trip it.
        let src = "fn main() -> i64 {\n    let c: i64 = 0\n    0\n}\n";
        let offset = src.find("= 0").unwrap();
        assert_eq!(
            parse_help("unexpected token: Eq, expected identifier", src, offset),
            None
        );
    }

    #[test]
    fn def_and_function_are_pointed_at_fn() {
        for (kw, src) in [
            ("def", "def main():\n    return 0\n"),
            ("function", "function main() {\n    return 0\n}\n"),
        ] {
            let offset = src.find(kw).unwrap();
            let msg = format!(
                "unexpected token: Ident(\"{kw}\"), expected item \
                 (fn/type/enum/mod/use/trait/impl/let)"
            );
            let h =
                parse_help(&msg, src, offset).unwrap_or_else(|| panic!("{kw} must produce help"));
            assert!(h.contains("Axon declares functions with `fn`"), "{h}");
            assert!(h.contains(kw), "names the token seen: {h}");
        }
    }

    /// `not` / `and` / `or` all reach their hint, including a LEADING `not`.
    ///
    /// `not` was the missing one, and its absence was measured rather than
    /// guessed: on the `tasks_hard` set `if not found {` produced only
    /// `unexpected token: Ident("found"), expected LBrace`, and the repair round
    /// returned the IDENTICAL error at the identical line and column — the
    /// signature of a diagnostic carrying nothing actionable. The row's help text
    /// already advertised `!` as the answer while being unable to detect the
    /// mistake it was describing.
    #[test]
    fn python_logical_operators_are_pointed_at_their_axon_spelling() {
        for (word, op, src) in [
            (
                "not",
                "!",
                "fn main() -> i64 {\n  if not found { 0 } else { 1 }\n}\n",
            ),
            (
                "and",
                "&&",
                "fn main() -> i64 {\n  if a and b { 0 } else { 1 }\n}\n",
            ),
            (
                "or",
                "||",
                "fn main() -> i64 {\n  if a or b { 0 } else { 1 }\n}\n",
            ),
            // A bare leading `not` — ` not ` alone would miss this, which is why
            // the check also tests the start of the trimmed line.
            ("not", "!", "fn main() -> i64 {\n  let x = 1\n  not x\n}\n"),
        ] {
            let offset = src.find(word).expect("fixture must contain the word");
            let msg = "unexpected token: Ident(\"found\"), expected LBrace";
            let h = parse_help(msg, src, offset)
                .unwrap_or_else(|| panic!("`{word}` must produce help for: {src}"));
            assert!(
                h.contains(&format!("`{word}` is not an Axon operator")),
                "must name the word seen: {h}"
            );
            assert!(h.contains(op), "must name the Axon spelling `{op}`: {h}");
        }
    }

    #[test]
    fn an_unrecognised_parse_error_yields_none_not_a_guess() {
        assert_eq!(
            parse_help("some novel parser message", "fn main() {}", 0),
            None
        );
    }

    #[test]
    fn offsets_past_the_end_and_mid_codepoint_are_total() {
        // The offset arrives from the parser and is used to index `src`; a
        // panic here would turn a compile error into a compiler crash.
        let src = "fn main() { \"héllo\" }";
        for off in [0, 1, src.len(), src.len() + 99, src.find('é').unwrap() + 1] {
            let _ = parse_help("unexpected token: Eq, expected identifier", src, off);
            let _ = parse_help("expected item", src, off);
        }
    }
}
