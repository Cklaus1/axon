/// Byte-offset span into the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde-json", derive(serde::Serialize, serde::Deserialize))]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn dummy() -> Self {
        Span { start: 0, end: 0 }
    }

    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }

    pub fn merge(a: Span, b: Span) -> Self {
        Span {
            start: a.start.min(b.start),
            end: a.end.max(b.end),
        }
    }

    pub fn is_dummy(&self) -> bool {
        self.start == 0 && self.end == 0
    }
}

impl From<std::ops::Range<usize>> for Span {
    fn from(r: std::ops::Range<usize>) -> Self {
        Span { start: r.start, end: r.end }
    }
}

/// Maps byte offsets to (line, col) pairs for diagnostic rendering.
pub struct SourceMap {
    line_starts: Vec<usize>,
    pub source: String,
}

impl SourceMap {
    pub fn new(source: String) -> Self {
        let mut line_starts = vec![0usize];
        for (i, c) in source.char_indices() {
            if c == '\n' {
                line_starts.push(i + 1);
            }
        }
        SourceMap { line_starts, source }
    }

    /// Convert byte offset to 1-indexed (line, col).
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let line = self.line_starts.partition_point(|&s| s <= offset) - 1;
        let col = offset - self.line_starts[line];
        (line + 1, col + 1)
    }

    /// Extract the text of the given line (0-indexed).
    pub fn line_text(&self, line_idx: usize) -> &str {
        let start = self.line_starts[line_idx];
        let end = self.line_starts.get(line_idx + 1).copied().unwrap_or(self.source.len());
        self.source[start..end].trim_end_matches('\n').trim_end_matches('\r')
    }

    /// Render a diagnostic caret block like rustc.
    pub fn render_caret(&self, span: Span) -> String {
        if span.is_dummy() {
            return String::new();
        }
        let (line, col) = self.line_col(span.start);
        let line_text = self.line_text(line - 1);
        let caret_len = (span.end - span.start).max(1);
        let padding = " ".repeat(col - 1);
        let carets = "^".repeat(caret_len.min(line_text.len().saturating_sub(col - 1) + 1));
        format!("{line:4} │ {line_text}\n     │ {padding}{carets}")
    }
}
