//! What the VISIBLE side of a workspace said about one attempt, as data (R4).
//!
//! A generator retrying after a rejection used to be shown a prose rendering
//! of the checker's output (`line 6: E0307 return type mismatch …`), cut to a
//! 2 KiB blob: the column, the expected and found types and the help text of
//! an `axon-diag/1` diagnostic were flattened or lost. [`Feedback`] keeps them
//! as fields and is rendered into the prompt as one compact JSON object.
//!
//! Every string in it comes from a run of a file the hidden check was CUT OUT
//! of ([`crate::runner`]'s `visible_run`), passes through the caller's
//! `clean` function (which rewrites the visible copy's name back to the
//! target's and drops anything naming the hidden check), and is bounded: each
//! message or help text to [`MAX_FIELD_BYTES`], at most [`MAX_DIAGNOSTICS`]
//! diagnostics and [`MAX_FAILED_TESTS`] failed tests, the note to
//! [`MAX_NOTE_BYTES`]. What a bound removed is counted in `truncated`, never
//! silently dropped.

use serde::Serialize;

/// The bound on one message, help or other free-text field of a diagnostic or
/// a failed test.
pub const MAX_FIELD_BYTES: usize = 300;
/// Diagnostics kept per attempt.
pub const MAX_DIAGNOSTICS: usize = 8;
/// Failed visible tests kept per attempt.
pub const MAX_FAILED_TESTS: usize = 8;
/// The bound on the note (the visible tests' tally and what they printed).
pub const MAX_NOTE_BYTES: usize = 1024;
/// Lines of printed test output the note keeps.
const MAX_PRINTED_LINES: usize = 40;

/// What the visible side said about one attempt. Empty fields are omitted
/// from the JSON, so an attempt the compiler accepted and no visible test
/// failed on renders as its tally note alone.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Feedback {
    /// `axon check` diagnostics of the patched file (hidden check cut out).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<Diag>,
    /// Visible tests that failed with the attempt applied.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failed_tests: Vec<FailedTest>,
    /// The attempt was refused before it ran because it needed authority the
    /// original code did not hold (R1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
    /// The VISIBLE run stopped at its time or output bound (R2). A bound hit
    /// by the hidden check is never reported here.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub timed_out: bool,
    /// Anything else, as text: the visible tests' tally and what they printed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// How many items or bytes-cut fields the bounds above removed.
    #[serde(skip_serializing_if = "is_zero")]
    pub truncated: u32,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// One `axon-diag/1` diagnostic, with the fields the schema carries.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Diag {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<u64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub code: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub found: Option<String>,
}

/// A visible test that failed, and its failure message when it gave one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct FailedTest {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// The capabilities an attempt needed that the original code did not hold.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Refusal {
    pub capabilities: Vec<String>,
}

impl Feedback {
    /// The compact single-line JSON the prompt carries.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("Feedback serializes")
    }

    /// The diagnostics of an `axon check` transcript: error diagnostics only,
    /// or with `all` every severity. `clean` rewrites a string for the
    /// generator and returns `None` for one that must not be shown, which
    /// drops the whole diagnostic.
    pub fn from_check_output(
        text: &str,
        all: bool,
        clean: &dyn Fn(&str) -> Option<String>,
    ) -> Self {
        let mut fb = Feedback::default();
        for l in text.lines().filter(|l| {
            if all {
                l.contains("\"severity\":\"")
            } else {
                l.contains("\"severity\":\"error\"")
            }
        }) {
            // Field cuts count only for a diagnostic that is kept; one
            // dropped by the item bound counts once.
            let mut cut = 0;
            let Some(d) = parse_diag(l.trim(), clean, &mut cut) else {
                continue;
            };
            if fb.diagnostics.len() == MAX_DIAGNOSTICS {
                fb.truncated += 1;
            } else {
                fb.truncated += cut;
                fb.diagnostics.push(d);
            }
        }
        fb
    }

    /// A visible test run's result: the failed tests with their messages, and
    /// a note with the tally and what the tests printed.
    pub fn from_tests(
        failed: &[(String, Option<String>)],
        passed: usize,
        printed: &[String],
        clean: &dyn Fn(&str) -> Option<String>,
    ) -> Self {
        let mut fb = Feedback::default();
        for (name, message) in failed {
            let Some(name) = clean(name) else { continue };
            let message = match message {
                Some(m) => match clean(m) {
                    Some(m) => Some(m),
                    None => continue,
                },
                None => None,
            };
            if fb.failed_tests.len() == MAX_FAILED_TESTS {
                fb.truncated += 1;
            } else {
                let message = message.map(|m| bound_field(&m, &mut fb.truncated));
                fb.failed_tests.push(FailedTest { name, message });
            }
        }
        let mut note = format!("{} failed, {passed} passed", failed.len());
        let lines: Vec<String> = printed
            .iter()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| clean(l))
            .take(MAX_PRINTED_LINES)
            .collect();
        if !lines.is_empty() {
            note.push_str("\nprinted:\n");
            note.push_str(&lines.join("\n"));
        }
        fb.note = Some(bound(&note, MAX_NOTE_BYTES, &mut fb.truncated));
        fb
    }

    /// A visible run that stopped at a bound: `why` says which.
    pub fn timed_out(why: &str) -> Self {
        let mut fb = Feedback {
            timed_out: true,
            ..Feedback::default()
        };
        fb.note = Some(bound(why, MAX_NOTE_BYTES, &mut fb.truncated));
        fb
    }

    /// An attempt refused for authority the original code did not hold.
    pub fn refused(capabilities: Vec<String>) -> Self {
        Feedback {
            refusal: Some(Refusal { capabilities }),
            ..Feedback::default()
        }
    }
}

fn parse_diag(
    line: &str,
    clean: &dyn Fn(&str) -> Option<String>,
    truncated: &mut u32,
) -> Option<Diag> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        // A severity-bearing line that is not JSON: shown as its text.
        let message = clean(line)?;
        return Some(Diag {
            message: bound_field(&message, truncated),
            ..Diag::default()
        });
    };
    let text = |k: &str| -> Option<Option<String>> {
        match v.get(k).and_then(|x| x.as_str()) {
            None => Some(None),
            Some(s) => clean(s).map(Some),
        }
    };
    let num = |k: &str| v.get(k).and_then(|x| x.as_u64());
    let message = text("message")?.unwrap_or_default();
    let help = text("help")?;
    Some(Diag {
        file: text("file")?,
        line: num("line"),
        col: num("col"),
        code: text("code")?.unwrap_or_default(),
        severity: text("severity")?.unwrap_or_default(),
        message: bound_field(&message, truncated),
        help: help.map(|h| bound_field(&h, truncated)),
        expected: text("expected")?.map(|s| bound_field(&s, truncated)),
        found: text("found")?.map(|s| bound_field(&s, truncated)),
    })
}

fn bound_field(s: &str, truncated: &mut u32) -> String {
    bound(s, MAX_FIELD_BYTES, truncated)
}

/// `s` cut to at most `max` bytes on a char boundary, ending in `…` when cut
/// (which counts once in `truncated`).
fn bound(s: &str, max: usize, truncated: &mut u32) -> String {
    const MARK: &str = "…";
    if s.len() <= max {
        return s.to_string();
    }
    *truncated += 1;
    let mut end = max - MARK.len();
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{MARK}", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keep(s: &str) -> Option<String> {
        Some(s.replace("/ws/.cortex-visible-9-f.ax", "f.ax"))
    }

    #[test]
    fn a_diagnostic_keeps_its_fields_and_its_file_is_rewritten() {
        let line = r#"{"schema":"axon-diag/1","severity":"error","code":"E0307","file":"/ws/.cortex-visible-9-f.ax","line":6,"col":5,"message":"return type mismatch","expected":"i64","found":"str","help":"adjust it"}"#;
        let fb = Feedback::from_check_output(line, false, &keep);
        assert_eq!(
            fb.diagnostics,
            vec![Diag {
                file: Some("f.ax".into()),
                line: Some(6),
                col: Some(5),
                code: "E0307".into(),
                severity: "error".into(),
                message: "return type mismatch".into(),
                help: Some("adjust it".into()),
                expected: Some("i64".into()),
                found: Some("str".into()),
            }]
        );
        assert_eq!(fb.truncated, 0);
        assert!(fb
            .to_json()
            .starts_with(r#"{"diagnostics":[{"file":"f.ax","line":6,"col":5"#));
    }

    #[test]
    fn bounds_cut_and_count() {
        let long = "m".repeat(MAX_FIELD_BYTES * 2);
        let line = format!(r#"{{"severity":"error","code":"E1","message":"{long}"}}"#);
        let text = vec![line; MAX_DIAGNOSTICS + 3].join("\n");
        let fb = Feedback::from_check_output(&text, false, &keep);
        assert_eq!(fb.diagnostics.len(), MAX_DIAGNOSTICS);
        assert!(fb.diagnostics[0].message.len() <= MAX_FIELD_BYTES);
        // 8 cut messages + 3 dropped diagnostics.
        assert_eq!(fb.truncated, MAX_DIAGNOSTICS as u32 + 3);
    }

    #[test]
    fn anything_clean_refuses_is_dropped_whole() {
        let hide = |s: &str| (!s.contains("secret")).then(|| s.to_string());
        let text = "{\"severity\":\"error\",\"message\":\"calls secret\"}\n\
                    {\"severity\":\"error\",\"message\":\"ok\"}";
        let fb = Feedback::from_check_output(text, false, &hide);
        assert_eq!(fb.diagnostics.len(), 1);
        assert_eq!(fb.diagnostics[0].message, "ok");
        let fb = Feedback::from_tests(
            &[
                ("t_secret".into(), None),
                ("t".into(), Some("secret said".into())),
                ("u".into(), Some("1 != 2".into())),
            ],
            2,
            &["secret line".into(), "seen".into()],
            &hide,
        );
        assert_eq!(fb.failed_tests.len(), 1);
        assert_eq!(fb.failed_tests[0].name, "u");
        assert_eq!(
            fb.note.as_deref(),
            Some("3 failed, 2 passed\nprinted:\nseen")
        );
    }

    #[test]
    fn warnings_only_with_all() {
        let text = "{\"severity\":\"warning\",\"code\":\"W1\",\"message\":\"w\"}";
        assert!(Feedback::from_check_output(text, false, &keep)
            .diagnostics
            .is_empty());
        assert_eq!(
            Feedback::from_check_output(text, true, &keep)
                .diagnostics
                .len(),
            1
        );
    }
}
