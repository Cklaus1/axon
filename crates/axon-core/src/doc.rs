//! Documentation generator for the Axon language (`axon doc`).
//!
//! Extracts `///` doc comments from source text, matches them to the AST item
//! that follows them, and emits Markdown documentation.
//!
//! No type inference is required — only the lexed+parsed AST and the raw source
//! text are needed.

use crate::ast::*;

/// Generate Markdown documentation for `program`.
///
/// `source` is the original source text (used to find `///` doc comments by
/// scanning backward from each item's span). `filename` appears as the H1 header.
pub fn generate_docs(program: &Program, source: &str, filename: &str) -> String {
    let mut out = String::new();
    out.push_str("# ");
    out.push_str(filename);
    out.push_str("\n\n");

    let mut has_docs = false;

    for item in &program.items {
        let (sig, byte_start) = match item {
            Item::FnDef(f)   => (fn_signature(f),   f.span.start),
            Item::TypeDef(t) => (type_signature(t), t.span.start),
            Item::EnumDef(e) => (enum_signature(e), e.span.start),
            _ => continue,
        };

        let doc = doc_comment_before(source, byte_start);
        if doc.is_empty() { continue; }

        has_docs = true;
        out.push_str("## ");
        out.push_str(&sig);
        out.push_str("\n\n");
        out.push_str(&doc);
        if !doc.ends_with('\n') { out.push('\n'); }
        out.push('\n');
        out.push_str("---\n\n");
    }

    if !has_docs {
        out.push_str("*No documented items.*\n");
    }

    out
}

// ── Signature renderers ───────────────────────────────────────────────────────

fn fn_signature(f: &FnDef) -> String {
    let mut s = String::from("fn ");
    s.push_str(&f.name);
    if !f.generic_params.is_empty() {
        s.push('<');
        s.push_str(&f.generic_params.join(", "));
        s.push('>');
    }
    s.push('(');
    for (i, p) in f.params.iter().enumerate() {
        if i > 0 { s.push_str(", "); }
        s.push_str(&p.name);
        s.push_str(": ");
        s.push_str(&render_type(&p.ty));
    }
    s.push(')');
    if let Some(ret) = &f.return_type {
        s.push_str(" -> ");
        s.push_str(&render_type(ret));
    }
    s
}

fn type_signature(t: &TypeDef) -> String {
    let mut s = String::from("type ");
    s.push_str(&t.name);
    if !t.generic_params.is_empty() {
        s.push('<');
        s.push_str(&t.generic_params.join(", "));
        s.push('>');
    }
    s.push_str(" = { ");
    for (i, field) in t.fields.iter().enumerate() {
        if i > 0 { s.push_str(", "); }
        s.push_str(&field.name);
        s.push_str(": ");
        s.push_str(&render_type(&field.ty));
    }
    s.push_str(" }");
    s
}

fn enum_signature(e: &EnumDef) -> String {
    let mut s = String::from("enum ");
    s.push_str(&e.name);
    if !e.generic_params.is_empty() {
        s.push('<');
        s.push_str(&e.generic_params.join(", "));
        s.push('>');
    }
    s
}

fn render_type(ty: &AxonType) -> String {
    match ty {
        AxonType::Named(n) | AxonType::TypeParam(n) => n.clone(),
        AxonType::DynTrait(n) => format!("dyn {n}"),
        AxonType::Option(inner) => format!("Option<{}>", render_type(inner)),
        AxonType::Result { ok, err } => format!("Result<{}, {}>", render_type(ok), render_type(err)),
        AxonType::Chan(inner) => format!("Chan<{}>", render_type(inner)),
        AxonType::Slice(inner) => format!("[{}]", render_type(inner)),
        AxonType::Generic { base, args } => {
            let args_str = args.iter().map(render_type).collect::<Vec<_>>().join(", ");
            format!("{base}<{args_str}>")
        }
        AxonType::Fn { params, ret } => {
            let ps = params.iter().map(render_type).collect::<Vec<_>>().join(", ");
            format!("fn({ps}) -> {}", render_type(ret))
        }
        AxonType::Ref(inner) => format!("&{}", render_type(inner)),
        AxonType::Tuple(elems) => {
            let parts: Vec<_> = elems.iter().map(render_type).collect();
            format!("({})", parts.join(", "))
        }
    }
}

// ── Doc comment extraction ────────────────────────────────────────────────────

/// Extract `///` doc comment lines that appear immediately before `byte_offset`
/// in `source`. Returns the doc text (with `///` prefix stripped), or an empty
/// string if there are no doc comments.
///
/// Stops scanning backward at the first line that is not a `///` comment and
/// is not blank (blank `///` lines inside a doc comment are preserved as
/// paragraph breaks).
fn doc_comment_before(source: &str, byte_offset: usize) -> String {
    let prefix = if byte_offset >= source.len() {
        source
    } else {
        &source[..byte_offset]
    };

    // Collect lines in forward order up to (but not including) the item.
    let lines: Vec<&str> = prefix.lines().collect();

    // Scan backward to find the `///` block immediately before the item.
    let mut doc_lines: Vec<String> = Vec::new();
    let mut collecting = false;

    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("///") {
            collecting = true;
            doc_lines.push(rest.trim_start_matches(' ').to_string());
        } else if trimmed.is_empty() && !collecting {
            // Skip leading blank lines between item and potential doc.
            continue;
        } else {
            break;
        }
    }

    if doc_lines.is_empty() {
        return String::new();
    }

    doc_lines.reverse();
    doc_lines.join("\n")
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    #[test]
    fn doc_basic_fn() {
        let src = "/// Add two numbers.\nfn add(a: i64, b: i64) -> i64 { a + b }";
        let prog = parse_source(src).expect("parse");
        let out = generate_docs(&prog, src, "test.ax");
        assert!(out.contains("## fn add(a: i64, b: i64) -> i64"), "sig: {out}");
        assert!(out.contains("Add two numbers."), "doc: {out}");
    }

    #[test]
    fn doc_no_comments_produces_placeholder() {
        let src = "fn add(a: i64, b: i64) -> i64 { a + b }";
        let prog = parse_source(src).expect("parse");
        let out = generate_docs(&prog, src, "test.ax");
        assert!(out.contains("*No documented items.*"), "placeholder: {out}");
    }

    #[test]
    fn doc_type_alias() {
        let src = "/// A 2D point.\ntype Point = { x: f64, y: f64 }";
        let prog = parse_source(src).expect("parse");
        let out = generate_docs(&prog, src, "test.ax");
        assert!(out.contains("## type Point = { x: f64, y: f64 }"), "type sig: {out}");
        assert!(out.contains("A 2D point."), "doc text: {out}");
    }

    #[test]
    fn doc_paragraph_break_in_comment() {
        let src = "/// First line.\n///\n/// Second paragraph.\nfn f() {}";
        let prog = parse_source(src).expect("parse");
        let out = generate_docs(&prog, src, "test.ax");
        assert!(out.contains("First line.\n\nSecond paragraph."), "paragraph: {out}");
    }
}
