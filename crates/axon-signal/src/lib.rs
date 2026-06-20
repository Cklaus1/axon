pub mod ab;
pub mod benchmark;
pub mod dashboard;
pub mod export;
pub mod mcp;
pub mod patterns;
pub mod rework;
pub mod score;
pub mod trends;
pub mod weekly;

/// Unicode-safe, display-clean goal string truncation.
///
/// - Strips stale `# Goal:` / `## ` markdown heading markers
/// - Replaces pipe-table lines (│ / |) with "(structured session)"
/// - Clips at `max_chars` char boundaries and appends `…`
pub fn display_goal(goal: &str, max_chars: usize) -> String {
    let s = goal.trim();
    // System-injected caveat headers are not real user goals
    if s.starts_with('<') {
        return "(system prompt)".to_string();
    }
    // Strip leading markdown heading markers
    let stripped = if s.starts_with('#') {
        let without = s.trim_start_matches('#').trim();
        if let Some(rest) = without.strip_prefix("Goal:") { rest.trim() } else { without }
    } else {
        s
    };
    // Strip literal "\n" sequences that leaked from JSONL storage
    let owned;
    let stripped = if stripped.contains("\\n") {
        owned = stripped.replace("\\n", " ");
        owned.trim()
    } else {
        stripped
    };
    // Find first non-table, non-empty line
    let clean = stripped.lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty() && !l.contains('│') && !l.starts_with('|'))
        .unwrap_or("(structured session)");
    let char_count = clean.chars().count();
    if char_count <= max_chars {
        clean.to_string()
    } else {
        let end = clean.char_indices().nth(max_chars).map(|(i, _)| i).unwrap_or(clean.len());
        format!("{}…", &clean[..end])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_goal_strips_hash_goal_label() {
        assert_eq!(display_goal("# Goal: fix the auth bug\n\nmore context", 80), "fix the auth bug");
    }

    #[test]
    fn test_display_goal_strips_literal_backslash_n() {
        let raw = "# Goal: make CI green\\n\\nThe roadmap work is functional";
        let result = display_goal(raw, 80);
        assert_eq!(result, "make CI green  The roadmap work is functional");
    }

    #[test]
    fn test_display_goal_pipe_table_fallback() {
        assert_eq!(display_goal("│ col │ val │", 80), "(structured session)");
    }

    #[test]
    fn test_display_goal_unicode_safe_truncate() {
        let s = "fix the — em dash bug in auth/jwt.go for all OAuth flows";
        let result = display_goal(s, 20);
        assert!(!result.contains("—\u{0}"), "should not panic or produce invalid UTF-8");
        assert!(result.ends_with('…'));
    }

    #[test]
    fn test_display_goal_plain_passthrough() {
        let s = "analyze this project, what's the status";
        assert_eq!(display_goal(s, 80), s);
    }
}
