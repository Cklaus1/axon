//! Finding the symbol to repair, instead of being told it.
//!
//! `cortex repair --symbol double` requires somebody to have already done the
//! interesting half: deciding WHAT is broken. This does that half from the only
//! evidence available without running a model — which checks fail, and which
//! functions those checks exercise.
//!
//! ## Ranked by spectrum, not by call count
//!
//! The first version took every function a failing check calls and refused
//! whenever there was more than one. Measured against the real corpus — a
//! defect injected into each function of `examples/**.ax`, one at a time — that
//! answered 8.8% of cases and refused 91.2%. It was never WRONG, which is the
//! property worth keeping, and it was nearly useless, because a real test calls
//! several helpers where the hand-written fixture called exactly one.
//!
//! What the first version threw away was the PASSING checks. A function
//! exercised by checks that pass is poor evidence for a defect; one exercised
//! only by checks that fail is strong evidence. That is spectrum-based fault
//! localization, and the ranking here is Ochiai, the standard coefficient for
//! it:
//!
//! ```text
//! score(f) = ef / sqrt(F * (ef + ep))
//! ```
//!
//! where `ef`/`ep` are the failing/passing checks that reach `f` and `F` is the
//! total number of failing checks. A function only failing checks reach scores
//! 1.0; one every check reaches scores low. Ties are still refused — the point
//! was never to always produce an answer, it was to stop discarding cases the
//! evidence could actually decide.
//!
//! ## It is syntactic, and says so
//!
//! A failing test names the functions it calls. Cross-referencing those against
//! the functions defined in the file gives a candidate set. It does NOT follow
//! calls transitively, does not know which argument was wrong, and cannot tell
//! a broken implementation from a wrong test. Those are real limits, and the
//! return type states them: the answer is a candidate SET, and a set with more
//! than one member is reported as ambiguous rather than resolved by picking
//! the first.
//!
//! Guessing here is the expensive failure. A wrong localization sends a
//! generator to rewrite a working function, and the hidden check refuses every
//! attempt without ever saying why — the loop would burn its whole budget
//! repairing the wrong thing and report "no progress", which is true and
//! useless.

/// The Axon lexer's view of a source ([`crate::runner::scan_code`]): which
/// bytes are code, and which lie in a string's `{…}` interpolation hole.
/// A hole is parsed as an expression (`ast.rs` `FmtStr`), so a call there is
/// a call; anywhere else in a string or a comment, `NAME(` is text.
struct CodeView {
    code: Vec<bool>,
    holes: Vec<(usize, usize)>,
}

/// `None` when `src` does not lex: no structure is read from text the
/// compiler would refuse (FU-7).
fn code_view(src: &str) -> Option<CodeView> {
    let mut code = vec![false; src.len()];
    crate::runner::scan_code(src, |i, _| {
        code[i] = true;
        None::<()>
    })
    .ok()?;
    // `scan_code` does not say where one string or comment ends and the next
    // begins, so each run of non-code bytes is lexed again here.
    let mut holes = Vec::new();
    let mut at = 0;
    while let Some(p) = code[at..].iter().position(|&c| !c) {
        let start = at + p;
        let end = code[start..]
            .iter()
            .position(|&c| c)
            .map_or(src.len(), |q| start + q);
        hole_spans(src, start, end, &mut holes);
        at = end;
    }
    Some(CodeView { code, holes })
}

/// The interpolation holes in `src[start..end]`, a run of strings and
/// comments that lexed: `"` to the first unescaped `"` is a string, raw when
/// a one-character word `r` precedes it (`runner.rs` `scan_code`); `//` to
/// the newline and `/*` to `*/` are comments. Holes are taken only in
/// non-raw strings, by the parser's rule (`parser.rs` `parse_fmt_str_raw`):
/// `{{` and `}}` are literal braces, a hole's `}` is matched at depth 0.
/// Residual: a string literal inside a hole is read as part of the hole.
fn hole_spans(src: &str, start: usize, end: usize, out: &mut Vec<(usize, usize)>) {
    let b = &src.as_bytes()[..end];
    let is_word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let mut i = start;
    while i < end {
        if b[i] == b'"' {
            let raw = i >= 1 && b[i - 1] == b'r' && (i < 2 || !is_word(b[i - 2]));
            let mut j = i + 1;
            while j < end && b[j] != b'"' {
                j += if b[j] == b'\\' && !raw { 2 } else { 1 };
            }
            if !raw {
                interpolation_holes(&src[i + 1..j.min(end)], i + 1, out);
            }
            i = j + 1;
        } else if b[i..].starts_with(b"/*") {
            let mut j = i + 2;
            while j + 1 < end && b[j..j + 2] != *b"*/" {
                // As in `scan_code`, a `*` takes the next character along.
                j += if b[j] == b'*' { 2 } else { 1 };
            }
            i = j + 2;
        } else {
            // `//` runs to the newline, which is code and ends the run.
            return;
        }
    }
}

/// The `{…}` holes of a non-raw string's text, offset by `base`.
fn interpolation_holes(text: &str, base: usize, out: &mut Vec<(usize, usize)>) {
    let b = text.as_bytes();
    let mut i = 0;
    while let Some(p) = b[i..].iter().position(|&c| c == b'{' || c == b'}') {
        let at = i + p;
        // `}` alone or `}}`, and `{{`: literal braces.
        if b[at] == b'}' || b.get(at + 1) == Some(&b'{') {
            i = at + if b.get(at + 1) == Some(&b[at]) { 2 } else { 1 };
            continue;
        }
        let mut depth = 0usize;
        let close = b[at + 1..].iter().position(|&c| match c {
            b'{' => {
                depth += 1;
                false
            }
            b'}' if depth == 0 => true,
            b'}' => {
                depth -= 1;
                false
            }
            _ => false,
        });
        // An unclosed `{` is a parse error: no hole.
        let Some(close) = close else { return };
        out.push((base + at + 1, base + at + 1 + close));
        i = at + 2 + close;
    }
}

/// `src`'s lines with leading blanks trimmed, each with whether its first
/// non-blank byte is code; empty when `src` does not lex.
fn code_lines(src: &str) -> Vec<(&str, bool)> {
    let Some(view) = code_view(src) else {
        return Vec::new();
    };
    let mut at = 0;
    src.split_inclusive('\n')
        .map(|l| {
            let t = l.trim_start();
            let first = at + (l.len() - t.len());
            at += l.len();
            (t, first < src.len() && view.code[first])
        })
        .collect()
}

/// Functions carrying a test attribute — `@[test]`, `@[test(should_fail)]`,
/// `@[forall]`.
///
/// A test is NEVER a repair target. That was previously enforced by removing
/// the checks the spectrum had observed, which left two holes, and the second
/// was severe:
///
/// * the ADJUDICATING check is stripped from both spectra before ranking, so
///   the one name that must never be a candidate was the one name the filter
///   could not see. A run could therefore select the grader, rewrite it to
///   `assert(true)`, leave the actual defect untouched, and exit 0 —
///   measured, not hypothesised;
/// * a check that neither passed nor failed (it errored, it was filtered out,
///   its signature was rejected) is absent from both lists and was a candidate
///   by default.
///
/// Reading the attribute answers the question at its source instead of
/// inferring it from which names happened to appear in a test run.
fn test_fns(src: &str) -> Vec<String> {
    let lines = code_lines(src);
    let mut out = Vec::new();
    for (i, &(t, in_code)) in lines.iter().enumerate() {
        if !in_code || !(t.starts_with("@[test") || t.starts_with("@[forall")) {
            continue;
        }
        // The annotation may sit above other annotations; take the next `fn`.
        for &(n, fn_in_code) in lines.iter().skip(i + 1).take(4) {
            if let Some(rest) = n.strip_prefix("fn ") {
                if let Some(name) = rest.split('(').next().filter(|_| fn_in_code) {
                    out.push(name.trim().to_string());
                }
                break;
            }
            if !n.starts_with('@') {
                break;
            }
        }
    }
    out
}

/// Names of functions DEFINED in the source, in order.
///
/// Deliberately crude: a line starting with `fn NAME(`, where that line
/// starts in code (not inside a string or a comment, by the Axon lexer's
/// view; nothing for a source that does not lex). The parser lives in
/// another crate and this needs no more than the names. If that ever stops
/// being enough, the fix is to depend on the parser, not to make the regex
/// cleverer.
fn defined_fns(src: &str) -> Vec<String> {
    code_lines(src)
        .into_iter()
        .filter(|&(_, in_code)| in_code)
        .filter_map(|(t, _)| {
            let rest = t.strip_prefix("fn ")?;
            let name = rest.split('(').next()?.trim();
            (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
                .then(|| name.to_string())
        })
        .collect()
}

/// The body of `fn NAME(...)`: the same reading `runner` repairs and splices
/// by ([`crate::runner::symbol_body`], the Axon lexer's view), so a `"}"` in
/// a string or a `fn NAME(` in a comment is not structure. A source that
/// does not lex — anywhere, not only before the body closes — has no body
/// here, hence no reach (FU-7, O-S1-11): counting raw braces in it would be
/// a guess at what the compiler sees.
fn fn_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    crate::runner::scan_code(src, |_, _| None::<()>).ok()?;
    crate::runner::symbol_body(src, name).map(|(a, b)| &src[a..b])
}

/// Which of `defined` are called inside `body`: a `NAME(` in code or in an
/// interpolation hole, never in other string text or a comment.
fn called_in(body: &str, defined: &[String]) -> Vec<String> {
    let Some(view) = code_view(body) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for name in defined {
        let needle = format!("{name}(");
        // Require a non-identifier character before the name so `double(` does
        // not match inside `redouble(`. Without it the candidate set silently
        // gains every function whose name ends with another's.
        let mut from = 0;
        while let Some(i) = body[from..].find(&needle) {
            let abs = from + i;
            let ok = abs == 0
                || !body[..abs]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_alphanumeric() || c == '_');
            let in_hole = view.holes.iter().any(|&(a, b)| a <= abs && abs < b);
            let in_code = view.code[abs] || in_hole;
            if ok && in_code && !out.contains(name) {
                out.push(name.clone());
                break;
            }
            from = abs + needle.len();
        }
    }
    out
}

/// Functions reachable from `start`'s body, following calls transitively.
///
/// Direct calls alone are not enough on real code: a check calls a public
/// helper which calls the function that is actually broken, and a direct-only
/// scan reports NoCandidate for a defect sitting two lines away. Depth is
/// bounded because the graph may be cyclic and because evidence this indirect
/// stops discriminating anyway.
fn reachable(src: &str, start: &str, defined: &[String]) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut frontier = vec![start.to_string()];
    for _ in 0..MAX_CALL_DEPTH {
        let mut next = Vec::new();
        for f in frontier.drain(..) {
            let Some(body) = fn_body(src, &f) else {
                continue;
            };
            for callee in called_in(body, defined) {
                if !seen.contains(&callee) {
                    seen.push(callee.clone());
                    next.push(callee);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }
    seen
}

/// How far a call chain is followed from a check.
///
/// Four, and the honest reason is narrower than the first version of this
/// comment claimed.
///
/// Ablated on the corpus in `benchmarks/` — one defect injected per function
/// across `examples/**.ax`, five defect classes, 111 trials per depth:
///
/// | depth | top-1 (sole candidate) | top-3 | truth absent from the ranking |
/// |---|---|---|---|
/// | 1 (direct calls only) | — | 80.6% | 2.8% |
/// | 2 | — | 90.1% | 0.9% |
/// | 4 | 31.5% | 89.2% | **0.0%** |
///
/// (The depth-1 row is from a 36-trial subset; the others are the full corpus.)
///
/// Following calls at all is worth about ten points of top-3 recall, and that
/// much the evidence states plainly. Beyond that it does NOT: depth 2 and
/// depth 4 differ by a single trial in each direction, which is noise, and an
/// earlier version of this table read those differences as a result. It also
/// quoted a measurement the benchmark README had since withdrawn.
///
/// Four is kept for the one thing that does separate them: it is the smallest
/// depth at which the broken function was never ABSENT from the ranking. A
/// candidate that is never listed cannot be tried; one ranked slightly lower
/// still can.
const MAX_CALL_DEPTH: usize = 4;

/// (see [`rank`])
///
/// Split from running the checks so the analysis is testable without a
/// compiler, and so the caller decides which checks count as visible. The
/// hidden check must never be passed here: localizing from the grader would
/// make the repair target a function of the answer.
/// Every candidate with its suspiciousness, most suspicious first.
///
/// Exposed separately from [`localize`] because the ranking answers a question
/// the verdict cannot: when the top candidate is wrong, was the right one
/// second? That is measurable, and it decides whether a loop should try
/// candidates in order or stop at one. A verdict that collapses the list to a
/// single name makes its own quality unmeasurable.
pub fn rank(src: &str, failing: &[String], passing: &[String]) -> Vec<(String, f64)> {
    if failing.is_empty() {
        return Vec::new();
    }
    let defined = defined_fns(src);
    // Anything carrying a test attribute, plus anything the spectrum observed.
    // The attribute is the authority: the adjudicating check is removed from
    // both spectra before this runs, so a name-based filter alone cannot see
    // the one name that matters most.
    let annotated = test_fns(src);
    let is_check = |n: &String| annotated.contains(n) || failing.contains(n) || passing.contains(n);

    // ef / ep per candidate. A check is never a candidate for its own repair:
    // without that, every failing test localizes to itself — true, and no help.
    let mut stats: Vec<(String, f64, f64)> = Vec::new();
    let bump = |name: &str, failed: bool, stats: &mut Vec<(String, f64, f64)>| match stats
        .iter_mut()
        .find(|(n, _, _)| n == name)
    {
        Some(e) => {
            if failed {
                e.1 += 1.0
            } else {
                e.2 += 1.0
            }
        }
        None => stats.push((
            name.to_string(),
            if failed { 1.0 } else { 0.0 },
            if failed { 0.0 } else { 1.0 },
        )),
    };
    for (checks, failed) in [(failing, true), (passing, false)] {
        for check in checks {
            for name in reachable(src, check, &defined) {
                if !is_check(&name) {
                    bump(&name, failed, &mut stats);
                }
            }
        }
    }

    let total_failing = failing.len() as f64;
    let mut ranked: Vec<(String, f64)> = stats
        .into_iter()
        .filter(|(_, ef, _)| *ef > 0.0)
        .map(|(n, ef, ep)| (n, ef / (total_failing * (ef + ep)).sqrt()))
        .collect();
    // Descending by score, then by name so a tie is reported in a stable order
    // rather than in whatever order the source happened to define things.
    ranked.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    ranked
}

/// The non-test functions that EVERY failing check reaches, in the order the
/// source defines them.
///
/// A check can only change its verdict if the body that was edited runs under
/// it. So when this set is empty, no single function body is reached by every
/// failing check, and one body edit cannot make them all pass: that is what
/// `cortex repair`'s early stop reads. The reach is the same syntactic one
/// [`rank`] uses (calls found in the source, followed to [`MAX_CALL_DEPTH`]),
/// so it says what the evidence shows, not what a run would execute: a call
/// made through a function value is not seen.
///
/// Like [`rank`] it takes only visible evidence. The hidden check must not be
/// in `failing`.
pub fn reached_by_all(src: &str, failing: &[String], passing: &[String]) -> Vec<String> {
    if failing.is_empty() {
        return Vec::new();
    }
    let defined = defined_fns(src);
    let annotated = test_fns(src);
    let is_check = |n: &String| annotated.contains(n) || failing.contains(n) || passing.contains(n);
    let mut common: Option<Vec<String>> = None;
    for check in failing {
        let reach: Vec<String> = reachable(src, check, &defined)
            .into_iter()
            .filter(|n| !is_check(n))
            .collect();
        common = Some(match common {
            None => reach,
            Some(c) => c.into_iter().filter(|n| reach.contains(n)).collect(),
        });
    }
    let common = common.unwrap_or_default();
    defined.into_iter().filter(|n| common.contains(n)).collect()
}

#[cfg(test)]
mod fn_body_tests {
    use super::*;

    fn reach(src: &str, from: &str) -> Vec<String> {
        reachable(src, from, &defined_fns(src))
    }

    /// FU-7 (O-S1-11): a `"}"` in a body is not structure. Counted as a
    /// brace, it ended `caller`'s body inside the string, and the call after
    /// it was never seen.
    #[test]
    fn a_brace_in_a_string_does_not_end_the_body() {
        let src = "fn helper(n: i64) -> i64 {\n    n\n}\n\
                   fn caller() -> i64 {\n    let s = \"}\";\n    helper(1)\n}\n";
        assert!(
            reach(src, "caller").contains(&"helper".to_string()),
            "ATTACK: a brace in a string ended the body and hid the call after it"
        );
    }

    /// FU-7: a `fn caller(` in a comment is not the function. Taken as one,
    /// the body read was the next function's (`helper`'s, empty here), and
    /// `caller`'s real call went unseen.
    #[test]
    fn a_definition_in_a_comment_is_not_the_function() {
        let src = "// fn caller( was renamed\nfn helper() {}\nfn caller() {\n    helper()\n}\n";
        assert!(
            reach(src, "caller").contains(&"helper".to_string()),
            "ATTACK: a `fn NAME(` in a comment was read as the function"
        );
    }

    /// FU-7 behaviour change: a source that does not lex has no body to read
    /// and so no reach — never a reach guessed from raw braces. `defined` is
    /// passed in: [`defined_fns`] is empty for such a source too, which would
    /// keep this green with the body check gone (SF-2).
    #[test]
    fn a_source_that_does_not_lex_has_no_reach() {
        let src = "fn helper() {}\nfn caller() {\n    helper()\n}\nfn open() { \"never closed }\n";
        assert!(
            reachable(src, "caller", &["helper".to_string()]).is_empty(),
            "ATTACK: a body was read from a source that does not lex"
        );
    }

    /// SF-2 (O-FU-02): `NAME(` in a comment is not a call. Counted as one,
    /// `unrelated` joined the candidates of every check reaching `helper`.
    #[test]
    fn a_call_in_a_comment_is_not_a_call() {
        let src = "fn unrelated(n: i64) -> i64 {\n    n\n}\n\
                   fn helper(n: i64) -> i64 {\n    // was: unrelated(n)\n    n\n}\n";
        assert!(
            !reach(src, "helper").contains(&"unrelated".to_string()),
            "ATTACK: a call in a comment was counted as a call"
        );
    }

    /// SF-2: `NAME(` in a string literal is not a call.
    #[test]
    fn a_call_in_a_string_is_not_a_call() {
        let src = "fn unrelated(n: i64) -> i64 {\n    n\n}\n\
                   fn helper(n: i64) -> i64 {\n    let s = \"unrelated(\";\n    n\n}\n";
        assert!(
            !reach(src, "helper").contains(&"unrelated".to_string()),
            "ATTACK: a call in a string literal was counted as a call"
        );
    }

    /// SF-2: a `fn NAME(` line inside a block comment defines nothing, so a
    /// call to that name reaches no candidate.
    #[test]
    fn a_definition_in_a_block_comment_is_not_defined() {
        let src = "/*\nfn ghost(n: i64) -> i64 { n }\n*/\nfn helper(n: i64) -> i64 {\n    \
                   ghost(n) + double(n)\n}\nfn double(n: i64) -> i64 { n * 2 }\n";
        assert!(
            !reach(src, "helper").contains(&"ghost".to_string()),
            "ATTACK: a function defined only inside a block comment was a candidate"
        );
    }

    /// SF-2: an `@[test` line inside a block comment does not make the next
    /// function a check. Taken as one, the defect itself left the ranking.
    #[test]
    fn a_test_attribute_in_a_block_comment_does_not_make_a_check() {
        let src = "/*\n@[test] */\nfn double(n: i64) -> i64 {\n    n + 2\n}\n\
                   fn helper(n: i64) -> i64 {\n    double(n)\n}\n\
                   @[test]\nfn visible_repro() {\n    assert_eq(helper(5), 10)\n}\n";
        let ranked = rank(src, &["visible_repro".to_string()], &[]);
        assert!(
            ranked.iter().any(|(n, _)| n == "double"),
            "ATTACK: a test attribute in a block comment dropped the defect from the ranking: {ranked:?}"
        );
    }

    /// SF-2 guard: a call inside a string's `{…}` interpolation hole is code,
    /// and is the real callee.
    #[test]
    fn a_call_in_an_interpolation_hole_is_a_call() {
        let src = "fn double(n: i64) -> i64 {\n    n + 2\n}\n\
                   fn helper(n: i64) -> str {\n    \"{{lit}} {to_str(double(n))}\"\n}\n";
        assert!(
            reach(src, "helper").contains(&"double".to_string()),
            "ATTACK: a call in an interpolation hole was not counted as a call"
        );
    }
}
