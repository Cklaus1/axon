//! Versioned, machine-stable diagnostic JSON (R8).
//!
//! `axon check --json` emits one NDJSON object per diagnostic. Historically that
//! object was an unstructured `{"error": "[E1234] message…"}` — a consumer (an
//! editor integration, or an AI agent) had to regex the prose to recover the
//! code, and a wording change broke it silently. This module emits a **versioned,
//! parse-without-regex** shape instead:
//!
//! ```json
//! {"schema":"axon-diag/1","severity":"error","code":"E1300","message":"…","help":"…"}
//! ```
//!
//! `schema` is a constant version tag: a consumer asserts it and knows the field
//! set; a future breaking change becomes `axon-diag/2` so the bump is detectable.
//! No `serde_json` (it collides with inkwell's trait universe — CLAUDE.md); the
//! JSON is hand-rolled, same discipline as `lockfile.rs` / `manifest.rs`.
//!
//! See `governance/specs/R8-diagnostic-schema.md`.

/// The diagnostic schema version tag. Bumped only on a breaking field change.
pub const DIAG_SCHEMA: &str = "axon-diag/1";

/// Parse a raw diagnostic string into one line of versioned JSON (R8 §4).
///
/// The parse is total and lossless: a string it cannot classify lands in
/// `message` verbatim with `code=""`. Deterministic — a pure function of the
/// input (same input → byte-identical output), with a fixed field order
/// (`schema`, `severity`, `code`, `message`, `help`). `help` is omitted entirely
/// when there is none (the key is absent, never `null`/`""`).
pub fn diagnostic_json(raw: &str) -> String {
    let (code, body) = split_code(raw);
    let severity = severity_for(&code);
    let (message, help) = split_help(body);

    let mut out = String::with_capacity(raw.len() + 48);
    out.push_str("{\"schema\":");
    out.push_str(&json_str(DIAG_SCHEMA));
    out.push_str(",\"severity\":");
    out.push_str(&json_str(severity));
    out.push_str(",\"code\":");
    out.push_str(&json_str(&code));
    out.push_str(",\"message\":");
    out.push_str(&json_str(&message));
    if !help.is_empty() {
        out.push_str(",\"help\":");
        out.push_str(&json_str(&help));
    }
    out.push('}');
    out
}

/// If `raw` begins with `[CODE]` where CODE is `[EWI]` followed by exactly four
/// digits, return `(CODE, rest)` with the `[CODE] ` prefix (and one trailing
/// space if present) stripped. Otherwise `("", raw)`.
fn split_code(raw: &str) -> (String, &str) {
    let bytes = raw.as_bytes();
    if bytes.first() != Some(&b'[') {
        return (String::new(), raw);
    }
    let Some(close) = raw.find(']') else {
        return (String::new(), raw);
    };
    let code = &raw[1..close];
    if is_diag_code(code) {
        let mut rest = &raw[close + 1..];
        rest = rest.strip_prefix(' ').unwrap_or(rest);
        (code.to_string(), rest)
    } else {
        (String::new(), raw)
    }
}

/// `[EWI]` followed by exactly four ASCII digits, e.g. `E1300`, `W0003`, `I0001`.
fn is_diag_code(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 5 && matches!(b[0], b'E' | b'W' | b'I') && b[1..].iter().all(|c| c.is_ascii_digit())
}

/// Severity from a code's leading letter; no code → `error` (the `as_json` path
/// is the error path).
fn severity_for(code: &str) -> &'static str {
    match code.as_bytes().first() {
        Some(b'W') => "warning",
        Some(b'I') => "note",
        _ => "error",
    }
}

/// Split `body` into `(message, help)`. Any line whose trimmed form starts with
/// `help:` or `fix:` contributes its remainder (after the prefix, trimmed) to
/// `help` (multiple joined with a space); the other lines form `message`, with
/// literal newlines re-escaped as `\n` so the result is a single line.
fn split_help(body: &str) -> (String, String) {
    let mut msg_lines: Vec<&str> = Vec::new();
    let mut help_parts: Vec<String> = Vec::new();
    for line in body.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("help:").or_else(|| t.strip_prefix("fix:")) {
            help_parts.push(rest.trim().to_string());
        } else {
            msg_lines.push(line);
        }
    }
    // `lines()` drops a single trailing newline; for a body with no `\n` this is
    // just the whole string. Re-join message lines with literal `\n` so the JSON
    // value is one physical line (json_str escapes the `\n` char to `\\n`).
    let message = msg_lines.join("\n").trim().to_string();
    (message, help_parts.join(" "))
}

/// Hand-rolled JSON string literal (the escapes a diagnostic can contain). Same
/// shape as `manifest.rs`/`lockfile.rs`.
fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_json_is_versioned_and_structured() {
        let j = diagnostic_json("[E1300] no model reachable");
        assert!(j.contains("\"schema\":\"axon-diag/1\""), "schema tag: {j}");
        assert!(j.contains("\"severity\":\"error\""), "severity: {j}");
        assert!(j.contains("\"code\":\"E1300\""), "code field: {j}");
        assert!(
            j.contains("\"message\":\"no model reachable\""),
            "message: {j}"
        );
        // One physical line.
        assert!(!j.contains('\n'), "must be single-line: {j}");
    }

    #[test]
    fn severity_maps_from_code_letter() {
        assert!(diagnostic_json("[W0003] shadows builtin").contains("\"severity\":\"warning\""));
        assert!(diagnostic_json("[I0001] deferred attr").contains("\"severity\":\"note\""));
        assert!(diagnostic_json("[E0001] undefined").contains("\"severity\":\"error\""));
    }

    #[test]
    fn diagnostic_json_handles_help_and_no_code() {
        // Help line split into its own field; message is the non-help remainder.
        let j = diagnostic_json("[W0003] fn shadows builtin\n  help: rename it");
        assert!(j.contains("\"code\":\"W0003\""), "{j}");
        assert!(
            j.contains("\"message\":\"fn shadows builtin\""),
            "message excludes help: {j}"
        );
        assert!(j.contains("\"help\":\"rename it\""), "help field: {j}");

        // `fix:` is treated like `help:`.
        let jf = diagnostic_json("[E0001] undefined\n  fix: did you mean foo?");
        assert!(
            jf.contains("\"help\":\"did you mean foo?\""),
            "fix→help: {jf}"
        );

        // No code → code="" and severity defaults to error; whole string is the message.
        let jn = diagnostic_json("plain message, no code");
        assert!(jn.contains("\"code\":\"\""), "empty code: {jn}");
        assert!(
            jn.contains("\"severity\":\"error\""),
            "default severity: {jn}"
        );
        assert!(
            jn.contains("\"message\":\"plain message, no code\""),
            "{jn}"
        );

        // No help → the `help` key is omitted entirely (not null/empty).
        assert!(
            !diagnostic_json("[E1300] x").contains("\"help\""),
            "help key absent when no help"
        );
    }

    #[test]
    fn diagnostic_json_escapes_and_stays_single_line() {
        // Quotes and backslashes escaped; embedded newline (a multi-line message
        // with no help) becomes a literal \n; result is one physical line.
        let j = diagnostic_json("[E0102] type mismatch: expected \"i64\"\n  found a \\ here");
        assert!(!j.contains('\n'), "single physical line: {j}");
        assert!(j.contains("\\\""), "escapes the inner quote: {j}");
        assert!(j.contains("\\\\"), "escapes the backslash: {j}");
        assert!(j.contains("\\n"), "embedded newline → literal \\n: {j}");
    }

    #[test]
    fn diagnostic_json_is_deterministic() {
        let inputs = [
            "[E1300] msg",
            "[W0003] a\n  help: b",
            "no code here",
            "[I0001] note\nline two",
        ];
        for i in inputs {
            assert_eq!(
                diagnostic_json(i),
                diagnostic_json(i),
                "deterministic for {i:?}"
            );
        }
    }

    #[test]
    fn non_code_bracket_prefix_is_not_mistaken_for_a_code() {
        // `[hint]` is not a diagnostic code — the whole string stays the message.
        let j = diagnostic_json("[hint] try again");
        assert!(j.contains("\"code\":\"\""), "not a code: {j}");
        assert!(
            j.contains("\"message\":\"[hint] try again\""),
            "message verbatim: {j}"
        );
    }
}
