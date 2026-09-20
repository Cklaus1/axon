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
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        let t = l.trim_start();
        if !(t.starts_with("@[test") || t.starts_with("@[forall")) {
            continue;
        }
        // The annotation may sit above other annotations; take the next `fn`.
        for next in lines.iter().skip(i + 1).take(4) {
            let n = next.trim_start();
            if let Some(rest) = n.strip_prefix("fn ") {
                if let Some(name) = rest.split('(').next() {
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
/// Deliberately crude: a line starting with `fn NAME(`. The parser lives in
/// another crate and this needs no more than the names. If that ever stops
/// being enough, the fix is to depend on the parser, not to make the regex
/// cleverer.
fn defined_fns(src: &str) -> Vec<String> {
    src.lines()
        .filter_map(|l| {
            let t = l.trim_start();
            let rest = t.strip_prefix("fn ")?;
            let name = rest.split('(').next()?.trim();
            (!name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_'))
                .then(|| name.to_string())
        })
        .collect()
}

/// The body of `fn NAME(...)`, by brace matching from its opening brace.
fn fn_body<'a>(src: &'a str, name: &str) -> Option<&'a str> {
    let needle = format!("fn {name}(");
    let at = src.find(&needle)?;
    let open = at + src[at..].find('{')?;
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[open + 1..open + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Which of `defined` are called inside `body`.
fn called_in(body: &str, defined: &[String]) -> Vec<String> {
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
            if ok && !out.contains(name) {
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
