//! Finding the symbol to repair, instead of being told it.
//!
//! `cortex repair --symbol double` requires somebody to have already done the
//! interesting half: deciding WHAT is broken. This does that half from the only
//! evidence available without running a model — which checks fail, and which
//! functions those checks exercise.
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

/// What the evidence supports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Localization {
    /// Exactly one function is implicated by the failing checks.
    Single {
        symbol: String,
        /// The failing checks that named it. Carried so a caller can show its
        /// working rather than asserting a conclusion.
        evidence: Vec<String>,
    },
    /// Several functions are implicated and the evidence does not choose
    /// between them. NOT resolved by picking one: a wrong target is worse than
    /// no target, because the loop cannot tell it is repairing the wrong thing.
    Ambiguous {
        candidates: Vec<String>,
        evidence: Vec<String>,
    },
    /// Every check passed. There is nothing to localize, which is a different
    /// statement from "nothing was found".
    NothingFailing,
    /// Checks failed and named no function defined in this file. Reported as
    /// its own case because the remedy differs from ambiguity: the defect may
    /// be in a callee, in a builtin, or in the check itself.
    NoCandidate { evidence: Vec<String> },
    /// The checks could not be run at all. Not a localization — an absence of
    /// evidence, kept distinct from an absence of candidates.
    Unknown { reason: String },
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

/// Localize from a source file and the names of the checks that FAILED.
///
/// Split from running the checks so the analysis is testable without a
/// compiler, and so the caller decides which checks count as visible. The
/// hidden check must never be passed here: localizing from the grader would
/// make the repair target a function of the answer.
pub fn localize(src: &str, failing: &[String]) -> Localization {
    if failing.is_empty() {
        return Localization::NothingFailing;
    }
    let defined = defined_fns(src);
    // A check is not a candidate for its own repair. Without this every
    // failing test localizes to itself, which is both true and useless.
    let mut candidates: Vec<String> = Vec::new();
    for check in failing {
        let Some(body) = fn_body(src, check) else {
            continue;
        };
        for name in called_in(body, &defined) {
            if !failing.contains(&name) && !candidates.contains(&name) {
                candidates.push(name);
            }
        }
    }
    match candidates.len() {
        0 => Localization::NoCandidate {
            evidence: failing.to_vec(),
        },
        1 => Localization::Single {
            symbol: candidates.remove(0),
            evidence: failing.to_vec(),
        },
        _ => Localization::Ambiguous {
            candidates,
            evidence: failing.to_vec(),
        },
    }
}
