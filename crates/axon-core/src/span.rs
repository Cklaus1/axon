/// Identity of the FILE a [`Span`]'s byte offsets are measured in.
///
/// `Span` used to be `(start, end)` and nothing else. That is enough for a
/// single-file compile and wrong for every multi-file one: `load_use_decls`
/// prepends an imported module's items into the entry program, each item
/// keeping offsets into ITS OWN file, and the diagnostic renderer then resolved
/// every one of them against the entry file's `SourceMap`. Measured: an error
/// truly at `lib.ax:14` was reported as `main.ax:6`, in a 5-line `main.ax`.
///
/// `SourceId(0)` is UNKNOWN and is what `Span::new`/`Span::dummy` produce, so
/// the many synthesized spans (desugaring, tests, tooling) need no change and a
/// caller that cannot name a file is not forced to invent one. UNKNOWN resolves
/// against the ambient entry file, which is the pre-existing behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
#[cfg_attr(feature = "serde-json", derive(serde::Serialize, serde::Deserialize))]
pub struct SourceId(pub u32);

impl SourceId {
    pub const UNKNOWN: SourceId = SourceId(0);

    pub fn is_unknown(&self) -> bool {
        self.0 == 0
    }
}

/// Byte-offset span into a source file, plus the identity of that file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde-json", derive(serde::Serialize, serde::Deserialize))]
pub struct Span {
    pub start: usize,
    pub end: usize,
    /// Which file `start`/`end` index into. `SourceId::UNKNOWN` (the default)
    /// means "not recorded" — serialized output omits it entirely, so
    /// single-file `axon parse` JSON is byte-identical to what it was before
    /// this field existed, and JSON written by an older build still loads.
    #[cfg_attr(
        feature = "serde-json",
        serde(default, skip_serializing_if = "SourceId::is_unknown")
    )]
    pub source: SourceId,
}

impl Span {
    pub fn dummy() -> Self {
        Span {
            start: 0,
            end: 0,
            source: SourceId::UNKNOWN,
        }
    }

    /// A span with no file identity. Kept so the ~76 existing construction
    /// sites compile untouched; prefer [`Span::with_source`] for a span born
    /// from real source bytes.
    pub fn new(start: usize, end: usize) -> Self {
        Span {
            start,
            end,
            source: SourceId::UNKNOWN,
        }
    }

    pub fn with_source(start: usize, end: usize, source: SourceId) -> Self {
        Span { start, end, source }
    }

    /// Union of two spans **in the same file**.
    ///
    /// Taking min/max across two files would manufacture a third span that
    /// indexes neither — the exact failure mode this type exists to prevent —
    /// so a cross-file merge yields `a` unchanged rather than a blend. A dummy
    /// operand is absorbed (merging with "nothing" is the other operand), and
    /// an UNKNOWN operand adopts the known file, since UNKNOWN is an absence of
    /// information, not a distinct file.
    pub fn merge(a: Span, b: Span) -> Self {
        if a.is_dummy() {
            return b;
        }
        if b.is_dummy() {
            return a;
        }
        let source = match (a.source.is_unknown(), b.source.is_unknown()) {
            (true, _) => b.source,
            (_, true) => a.source,
            _ if a.source == b.source => a.source,
            // Different files: not mergeable. Keep the left operand whole.
            _ => return a,
        };
        Span {
            start: a.start.min(b.start),
            end: a.end.max(b.end),
            source,
        }
    }

    pub fn is_dummy(&self) -> bool {
        self.start == 0 && self.end == 0
    }
}

impl From<std::ops::Range<usize>> for Span {
    fn from(r: std::ops::Range<usize>) -> Self {
        Span {
            start: r.start,
            end: r.end,
            source: SourceId::UNKNOWN,
        }
    }
}

/// The process-wide interning table mapping [`SourceId`] → (path, [`SourceMap`]).
///
/// A `Span` is `Copy` and travels through the AST, the resolver, inference, the
/// checker and the borrow checker before anything renders it. Threading a map
/// set through all of that would touch every signature on the way; interning
/// keeps `Span` 24 bytes and lets the renderer — the only consumer that needs
/// the bytes — look the file up at the end. `parser::take_accepted_mut` is the
/// existing precedent for compiler-global state of this shape.
///
/// `parse_source_files` parses in parallel, so the table is behind an `RwLock`.
mod registry {
    use super::SourceMap;
    use std::sync::{Arc, OnceLock, RwLock};

    pub(super) struct Entry {
        pub path: String,
        pub map: Arc<SourceMap>,
    }

    pub(super) fn table() -> &'static RwLock<Vec<Entry>> {
        static TABLE: OnceLock<RwLock<Vec<Entry>>> = OnceLock::new();
        TABLE.get_or_init(|| RwLock::new(Vec::new()))
    }
}

/// Intern `path`'s bytes and return the id spans into it should carry.
///
/// Re-interning the same (path, source) pair returns the existing id, so
/// re-parsing a file — `axon session` re-checks the whole accumulated program
/// every cell — does not grow the table without bound. The same path with
/// DIFFERENT bytes gets a fresh id on purpose: two ids naming one path is
/// harmless, whereas serving new spans from a stale map is the bug.
pub fn intern_source(path: &str, source: &str) -> SourceId {
    {
        let t = registry::table().read().unwrap();
        for (i, e) in t.iter().enumerate() {
            if e.path == path && e.map.source == source {
                return SourceId(i as u32 + 1);
            }
        }
    }
    let mut t = registry::table().write().unwrap();
    // Re-check under the write lock: two parse threads can race to here.
    for (i, e) in t.iter().enumerate() {
        if e.path == path && e.map.source == source {
            return SourceId(i as u32 + 1);
        }
    }
    t.push(registry::Entry {
        path: path.to_string(),
        map: std::sync::Arc::new(SourceMap::new(source.to_string())),
    });
    SourceId(t.len() as u32)
}

/// The `SourceMap` for an interned id, or `None` for UNKNOWN / an id from a
/// different process (a deserialized AST).
pub fn source_map_of(id: SourceId) -> Option<std::sync::Arc<SourceMap>> {
    if id.is_unknown() {
        return None;
    }
    let t = registry::table().read().unwrap();
    t.get(id.0 as usize - 1)
        .map(|e| std::sync::Arc::clone(&e.map))
}

/// The path an interned id names.
pub fn source_path_of(id: SourceId) -> Option<String> {
    if id.is_unknown() {
        return None;
    }
    let t = registry::table().read().unwrap();
    t.get(id.0 as usize - 1).map(|e| e.path.clone())
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
        SourceMap {
            line_starts,
            source,
        }
    }

    /// Convert byte offset to 1-indexed (line, col), or `None` when `offset`
    /// does not lie in THIS source.
    ///
    /// The refusal is the point. `partition_point` clamps: an offset past EOF
    /// used to land on the last line with an enormous column, so a span
    /// belonging to another file produced a confident, wrong, *impossible*
    /// location instead of admitting it could not say. Measured before this
    /// check existed: `lib.ax:14` rendered as `main.ax:6:112` in a 5-line
    /// `main.ax`. An out-of-range offset is a bug somewhere upstream; saying so
    /// is strictly better than inventing a plausible answer for it.
    pub fn try_line_col(&self, offset: usize) -> Option<(usize, usize)> {
        if offset > self.source.len() {
            return None;
        }
        let line = self.line_starts.partition_point(|&s| s <= offset) - 1;
        let col = offset - self.line_starts[line];
        Some((line + 1, col + 1))
    }

    /// Convert byte offset to 1-indexed (line, col); `(0, 0)` when the offset
    /// is not in this source. Line 0 is the codebase's existing "no location"
    /// value — `PipelineDiagnostic::display` already prints the bare file name
    /// for it — so an out-of-range span degrades to "no location" rather than
    /// to a wrong one.
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        self.try_line_col(offset).unwrap_or((0, 0))
    }

    /// Extract the text of the given line (0-indexed).
    pub fn line_text(&self, line_idx: usize) -> &str {
        let start = self.line_starts[line_idx];
        let end = self
            .line_starts
            .get(line_idx + 1)
            .copied()
            .unwrap_or(self.source.len());
        self.source[start..end]
            .trim_end_matches('\n')
            .trim_end_matches('\r')
    }

    /// Render a diagnostic caret block like rustc.
    pub fn render_caret(&self, span: Span) -> String {
        if span.is_dummy() {
            return String::new();
        }
        // An offset this map cannot place has no caret to draw. Before
        // `try_line_col` refused, `line` here was clamped to the last line and
        // the caret was painted over unrelated source.
        let Some((line, col)) = self.try_line_col(span.start) else {
            return String::new();
        };
        let line_text = self.line_text(line - 1);
        // Defensive: a malformed span (end < start) or a one-past-EOF span must
        // not panic the diagnostic renderer (saturating, not wrapping).
        let caret_len = span.end.saturating_sub(span.start).max(1);
        let padding = " ".repeat(col - 1);
        let carets = "^".repeat(caret_len.min(line_text.len().saturating_sub(col - 1) + 1));
        format!("{line:4} │ {line_text}\n     │ {padding}{carets}")
    }
}
