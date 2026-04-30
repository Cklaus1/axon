//! LSP server for the Axon compiler (`axon lsp`).
//!
//! Implements a JSON-RPC 2.0 server on stdin/stdout following the Language
//! Server Protocol 3.17 specification.  The server runs until stdin closes
//! (client disconnect).
//!
//! ## Capabilities
//! - `textDocumentSync`: full-document sync (mode 1)
//! - `hoverProvider`: function signatures and type names
//! - `definitionProvider`: jump to top-level declarations
//! - `diagnosticProvider`: error/warning squiggles via `publishDiagnostics`
//!
//! ## Analysis strategy
//! On every `didOpen` or `didChange` notification the full pipeline is re-run
//! from scratch on the in-memory document text.  No partial re-parse.  This is
//! fast enough (< 50 ms on files under 1 000 lines) for Phase 4.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};

use serde_json::{json, Value};

use crate::{ast, checker, infer, resolver, span};

// ── Public types ──────────────────────────────────────────────────────────────

/// A diagnostic emitted by the analysis pipeline, ready for LSP serialisation.
#[derive(Debug, Clone)]
pub struct LspDiagnostic {
    pub code: String,
    pub message: String,
    pub span: span::Span,
    /// 1 = error, 2 = warning, 3 = info
    pub severity: u8,
}

// ── Analysis entry point (called from lib.rs) ─────────────────────────────────

/// Run the full analysis pipeline on `source` text.
///
/// Returns the typed AST (if parsing succeeded), the infer context, and a flat
/// list of diagnostics for LSP `publishDiagnostics`.
pub fn analyse_source(source: &str, uri: &str) -> crate::AnalysisResult {
    // ── Parse ──────────────────────────────────────────────────────────────
    let mut program = match crate::parse_source(source) {
        Ok(p) => p,
        Err(e) => {
            return crate::AnalysisResult {
                program: None,
                infer_ctx: None,
                diagnostics: vec![LspDiagnostic {
                    code: "E0000".to_string(),
                    message: e.to_string(),
                    span: span::Span::dummy(),
                    severity: 1,
                }],
            };
        }
    };

    // ── AXON_PATH module loading ──────────────────────────────────────────
    let search_dirs = crate::axon_search_dirs(None);
    let mut diagnostics: Vec<LspDiagnostic> = crate::load_use_decls(&mut program, &search_dirs)
        .into_iter()
        .map(|e| LspDiagnostic {
            code: e.code.to_string(),
            message: e.message,
            span: span::Span::dummy(),
            severity: 1,
        })
        .collect();

    // ── Resolve ───────────────────────────────────────────────────────────
    let resolve_result = resolver::resolve_program(&mut program, uri);
    resolver::fill_captures(&mut program);

    for d in &resolve_result.errors {
        diagnostics.push(LspDiagnostic {
            code: d.code.to_string(),
            message: d.message.clone(),
            span: d.span,
            severity: 1,
        });
    }
    for d in &resolve_result.warnings {
        diagnostics.push(LspDiagnostic {
            code: d.code.to_string(),
            message: d.message.clone(),
            span: d.span,
            severity: 2,
        });
    }

    // ── Type inference ────────────────────────────────────────────────────
    let mut infer_ctx = infer::InferCtx::new(uri);
    infer_ctx.infer_program(&mut program);

    for e in &infer_ctx.errors {
        let mut msg = e.message.clone();
        if let Some(exp) = &e.expected {
            msg.push_str(&format!(" (expected {exp})"));
        }
        if let Some(fnd) = &e.found {
            msg.push_str(&format!(", found {fnd}"));
        }
        diagnostics.push(LspDiagnostic {
            code: e.code.to_string(),
            message: msg,
            span: e.span,
            severity: 1,
        });
    }

    // ── Type checking ─────────────────────────────────────────────────────
    let fn_sigs: HashMap<String, checker::FnSig> = infer_ctx
        .fn_sigs
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                checker::FnSig {
                    params: v.params.clone(),
                    ret: v.ret.clone(),
                },
            )
        })
        .collect();
    let mut check_ctx = checker::CheckCtx::new(uri, fn_sigs, infer_ctx.struct_fields.clone());
    let check_errors = check_ctx.check_program(&program, HashMap::new());

    for e in &check_errors {
        let mut msg = e.message.clone();
        if let Some(exp) = &e.expected {
            msg.push_str(&format!(" (expected {exp})"));
        }
        if let Some(fnd) = &e.found {
            msg.push_str(&format!(", found {fnd}"));
        }
        diagnostics.push(LspDiagnostic {
            code: e.code.to_string(),
            message: msg,
            span: e.span,
            severity: 1,
        });
    }

    // ── Borrow checking ───────────────────────────────────────────────────
    for item in &program.items {
        match item {
            ast::Item::FnDef(fndef) => {
                let param_types: HashMap<String, crate::types::Type> =
                    if let Some(sig) = infer_ctx.fn_sigs.get(&fndef.name) {
                        fndef
                            .params
                            .iter()
                            .zip(sig.params.iter())
                            .map(|(p, t)| (p.name.clone(), t.clone()))
                            .collect()
                    } else {
                        HashMap::new()
                    };
                for err in crate::borrow::check_fn(fndef, param_types) {
                    diagnostics.push(LspDiagnostic {
                        code: "E0800".to_string(),
                        message: err.to_string(),
                        span: span::Span::dummy(),
                        severity: 1,
                    });
                }
            }
            ast::Item::ImplBlock(blk) => {
                let type_name = match &blk.for_type {
                    ast::AxonType::Named(n) => n.clone(),
                    ast::AxonType::Generic { base, .. } => base.clone(),
                    _ => "Unknown".into(),
                };
                for method in &blk.methods {
                    let key = format!("{type_name}__{}", method.name);
                    let param_types: HashMap<String, crate::types::Type> =
                        if let Some(sig) = infer_ctx.fn_sigs.get(&key) {
                            method
                                .params
                                .iter()
                                .zip(sig.params.iter())
                                .map(|(p, t)| (p.name.clone(), t.clone()))
                                .collect()
                        } else {
                            HashMap::new()
                        };
                    for err in crate::borrow::check_fn(method, param_types) {
                        diagnostics.push(LspDiagnostic {
                            code: "E0800".to_string(),
                            message: err.to_string(),
                            span: span::Span::dummy(),
                            severity: 1,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    crate::AnalysisResult {
        program: Some(program),
        infer_ctx: Some(infer_ctx),
        diagnostics,
    }
}

// ── LSP server event loop ─────────────────────────────────────────────────────

/// Run the LSP server, reading JSON-RPC 2.0 messages from stdin and writing
/// responses to stdout.  Blocks until stdin is closed.
pub fn run_lsp() {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut out = stdout.lock();

    // In-memory document store: URI → source text + last analysis result.
    let mut documents: HashMap<String, String> = HashMap::new();
    let mut last_result: HashMap<String, crate::AnalysisResult> = HashMap::new();

    loop {
        // ── Read Content-Length header ────────────────────────────────────
        let content_length = match read_content_length(&mut reader) {
            Some(n) => n,
            None => break, // stdin closed
        };

        // ── Read JSON body ────────────────────────────────────────────────
        let mut body = vec![0u8; content_length];
        use std::io::Read as _;
        if reader.read_exact(&mut body).is_err() {
            break;
        }

        let msg: Value = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let id = msg.get("id").cloned();
        let method = msg["method"].as_str().unwrap_or("").to_string();
        let params = msg.get("params").cloned().unwrap_or(Value::Null);

        match method.as_str() {
            // ── Lifecycle ─────────────────────────────────────────────────
            "initialize" => {
                let response = json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": {
                        "capabilities": {
                            "textDocumentSync": 1,
                            "hoverProvider": true,
                            "definitionProvider": true,
                            "diagnosticProvider": {
                                "interFileDependencies": false,
                                "workspaceDiagnostics": false
                            }
                        },
                        "serverInfo": {
                            "name": "axon-lsp",
                            "version": env!("CARGO_PKG_VERSION")
                        }
                    }
                });
                send_message(&mut out, &response);
            }

            "initialized" => { /* no-op notification */ }

            "shutdown" => {
                send_message(
                    &mut out,
                    &json!({"jsonrpc":"2.0","id":id,"result":null}),
                );
            }

            "exit" => break,

            // ── Document sync ─────────────────────────────────────────────
            "textDocument/didOpen" => {
                if let Some(text_doc) = params.get("textDocument") {
                    let uri = text_doc["uri"].as_str().unwrap_or("").to_string();
                    let text = text_doc["text"].as_str().unwrap_or("").to_string();
                    let result = crate::analyse(&text, &uri);
                    push_diagnostics(&mut out, &uri, &result.diagnostics, &text);
                    last_result.insert(uri.clone(), result);
                    documents.insert(uri, text);
                }
            }

            "textDocument/didChange" => {
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                // Full-document sync: take the last content change.
                if let Some(changes) = params["contentChanges"].as_array() {
                    if let Some(last) = changes.last() {
                        let text = last["text"].as_str().unwrap_or("").to_string();
                        let result = crate::analyse(&text, &uri);
                        push_diagnostics(&mut out, &uri, &result.diagnostics, &text);
                        last_result.insert(uri.clone(), result);
                        documents.insert(uri, text);
                    }
                }
            }

            "textDocument/didClose" => {
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                documents.remove(&uri);
                last_result.remove(&uri);
                // Clear diagnostics for closed document.
                push_diagnostics(&mut out, &uri, &[], "");
            }

            // ── Hover ─────────────────────────────────────────────────────
            "textDocument/hover" => {
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as u32;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as u32;

                let hover_content = if let (Some(src), Some(res)) =
                    (documents.get(&uri), last_result.get(&uri))
                {
                    compute_hover(res, src, line, character)
                } else {
                    None
                };

                let result_val = match hover_content {
                    Some(md) => json!({
                        "contents": { "kind": "markdown", "value": md }
                    }),
                    None => Value::Null,
                };
                send_message(
                    &mut out,
                    &json!({"jsonrpc":"2.0","id":id,"result":result_val}),
                );
            }

            // ── Go-to-definition ──────────────────────────────────────────
            "textDocument/definition" => {
                let uri = params["textDocument"]["uri"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let line = params["position"]["line"].as_u64().unwrap_or(0) as u32;
                let character = params["position"]["character"].as_u64().unwrap_or(0) as u32;

                let def_location = if let (Some(src), Some(res)) =
                    (documents.get(&uri), last_result.get(&uri))
                {
                    compute_definition(res, &uri, src, line, character)
                } else {
                    None
                };

                let result_val = def_location.unwrap_or(Value::Null);
                send_message(
                    &mut out,
                    &json!({"jsonrpc":"2.0","id":id,"result":result_val}),
                );
            }

            // ── Unknown / notification ─────────────────────────────────────
            _ => {
                // Only respond to requests (which have an id), not notifications.
                if id.is_some() {
                    send_message(
                        &mut out,
                        &json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": {
                                "code": -32601,
                                "message": format!("Method not found: {method}")
                            }
                        }),
                    );
                }
            }
        }
    }
}

// ── Hover ─────────────────────────────────────────────────────────────────────

fn compute_hover(
    result: &crate::AnalysisResult,
    source: &str,
    line: u32,
    character: u32,
) -> Option<String> {
    let byte_offset = lsp_pos_to_byte_offset(source, line, character)?;
    let word = word_at_offset(source, byte_offset)?;

    // Function signature hover.
    if let Some(infer_ctx) = &result.infer_ctx {
        if let Some(sig) = infer_ctx.fn_sigs.get(&word) {
            return Some(format_fn_hover(&word, sig));
        }

        // Struct type hover.
        if let Some(fields) = infer_ctx.struct_fields.get(&word) {
            let field_strs: Vec<String> = fields
                .iter()
                .map(|(n, t)| format!("{n}: {}", format_type(t)))
                .collect();
            return Some(format!(
                "```axon\ntype {} = {{ {} }}\n```",
                word,
                field_strs.join(", ")
            ));
        }
    }

    // Enum or trait declaration hover from AST.
    if let Some(program) = &result.program {
        for item in &program.items {
            match item {
                ast::Item::EnumDef(e) if e.name == word => {
                    return Some(format!("```axon\nenum {}\n```", e.name));
                }
                ast::Item::TraitDef(t) if t.name == word => {
                    return Some(format!("```axon\ntrait {}\n```", t.name));
                }
                _ => {}
            }
        }
    }

    None
}

fn format_fn_hover(name: &str, sig: &infer::FnSig) -> String {
    let params: Vec<String> = sig
        .params
        .iter()
        .map(|t| format_type(t))
        .collect();
    let ret = format_type(&sig.ret);
    format!(
        "```axon\nfn {}({}) -> {}\n```",
        name,
        params.join(", "),
        ret
    )
}

fn format_type(ty: &crate::types::Type) -> String {
    use crate::types::Type;
    match ty {
        Type::I8 => "i8".into(),
        Type::I16 => "i16".into(),
        Type::I32 => "i32".into(),
        Type::I64 => "i64".into(),
        Type::U8 => "u8".into(),
        Type::U16 => "u16".into(),
        Type::U32 => "u32".into(),
        Type::U64 => "u64".into(),
        Type::F32 => "f32".into(),
        Type::F64 => "f64".into(),
        Type::Bool => "bool".into(),
        Type::Str => "str".into(),
        Type::Unit => "()".into(),
        Type::Struct(n) | Type::Enum(n) => n.clone(),
        Type::TypeParam(n) => n.clone(),
        Type::DynTrait(n) => format!("dyn {n}"),
        Type::Option(inner) => format!("Option<{}>", format_type(inner)),
        Type::Result(ok, err) => format!("Result<{}, {}>", format_type(ok), format_type(err)),
        Type::Chan(inner) => format!("Chan<{}>", format_type(inner)),
        Type::Slice(inner) => format!("[{}]", format_type(inner)),
        Type::Tuple(elems) => {
            let es: Vec<_> = elems.iter().map(format_type).collect();
            format!("({})", es.join(", "))
        }
        Type::Fn(params, ret) => {
            let ps: Vec<_> = params.iter().map(format_type).collect();
            format!("fn({}) -> {}", ps.join(", "), format_type(ret))
        }
        Type::Var(n) => format!("?{n}"),
        Type::Unknown => "?".into(),
        Type::Deferred(n) => n.clone(),
    }
}

// ── Go-to-definition ──────────────────────────────────────────────────────────

fn compute_definition(
    result: &crate::AnalysisResult,
    uri: &str,
    source: &str,
    line: u32,
    character: u32,
) -> Option<Value> {
    let byte_offset = lsp_pos_to_byte_offset(source, line, character)?;
    let word = word_at_offset(source, byte_offset)?;

    let program = result.program.as_ref()?;

    for item in &program.items {
        let decl_span = match item {
            ast::Item::FnDef(f) if f.name == word => f.span,
            ast::Item::TypeDef(t) if t.name == word => t.span,
            ast::Item::EnumDef(e) if e.name == word => e.span,
            ast::Item::TraitDef(t) if t.name == word => t.span,
            _ => continue,
        };

        if decl_span.start == 0 && decl_span.end == 0 {
            continue; // dummy span — no location info
        }

        let range = span_to_lsp_range(source, decl_span);
        return Some(json!({
            "uri": uri,
            "range": range
        }));
    }

    None
}

// ── Diagnostics push ──────────────────────────────────────────────────────────

fn push_diagnostics(
    out: &mut impl Write,
    uri: &str,
    diagnostics: &[LspDiagnostic],
    source: &str,
) {
    let diags: Vec<Value> = diagnostics
        .iter()
        .map(|d| {
            let range = if d.span.start == 0 && d.span.end == 0 {
                json!({"start":{"line":0,"character":0},"end":{"line":0,"character":0}})
            } else {
                span_to_lsp_range(source, d.span)
            };
            json!({
                "range": range,
                "severity": d.severity,
                "code": d.code,
                "source": "axon",
                "message": d.message
            })
        })
        .collect();

    let notification = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": {
            "uri": uri,
            "diagnostics": diags
        }
    });
    send_message(out, &notification);
}

// ── JSON-RPC framing ──────────────────────────────────────────────────────────

fn read_content_length(reader: &mut impl BufRead) -> Option<usize> {
    let mut content_length: Option<usize> = None;

    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return None, // EOF
            Err(_) => return None,
            _ => {}
        }

        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            // Blank line signals end of headers.
            return content_length;
        }

        if let Some(rest) = trimmed.strip_prefix("Content-Length: ") {
            content_length = rest.trim().parse().ok();
        }
        // Other headers (e.g. Content-Type) are silently ignored.
    }
}

fn send_message(out: &mut impl Write, value: &Value) {
    let body = serde_json::to_string(value).unwrap_or_default();
    let _ = write!(out, "Content-Length: {}\r\n\r\n{}", body.len(), body);
    let _ = out.flush();
}

// ── Position helpers ──────────────────────────────────────────────────────────

/// Convert an LSP (line, character) position (both 0-based) to a byte offset
/// in `source`.  Returns `None` if the position is out of range.
fn lsp_pos_to_byte_offset(source: &str, line: u32, character: u32) -> Option<usize> {
    let mut cur_line = 0u32;
    let mut offset = 0usize;

    for ch in source.chars() {
        if cur_line == line {
            break;
        }
        if ch == '\n' {
            cur_line += 1;
        }
        offset += ch.len_utf8();
    }

    if cur_line < line {
        return None; // line is beyond end of file
    }

    // Advance by `character` UTF-16 code units (LSP uses UTF-16 offsets).
    // For ASCII-only code (the common case) UTF-16 == UTF-8 lengths.
    let remaining = source.get(offset..)?;
    let mut col_utf16 = 0u32;
    for ch in remaining.chars() {
        if col_utf16 >= character {
            break;
        }
        col_utf16 += ch.len_utf16() as u32;
        offset += ch.len_utf8();
    }

    Some(offset)
}

/// Extract the identifier word (alphanumeric + `_`) surrounding `byte_offset`.
fn word_at_offset(source: &str, byte_offset: usize) -> Option<String> {
    if byte_offset > source.len() {
        return None;
    }

    let bytes = source.as_bytes();

    // Walk backward to the start of the word.
    let mut start = byte_offset;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }

    // Walk forward to the end of the word.
    let mut end = byte_offset;
    while end < bytes.len() && is_ident_byte(bytes[end]) {
        end += 1;
    }

    if start == end {
        return None;
    }

    let word = &source[start..end];
    if word.is_empty() || word.chars().next()?.is_ascii_digit() {
        return None; // pure number — not an identifier
    }

    Some(word.to_string())
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// Convert a `Span` (byte offsets) to an LSP `Range` (0-based line/character).
fn span_to_lsp_range(source: &str, sp: span::Span) -> Value {
    let start = byte_offset_to_lsp_pos(source, sp.start);
    let end = byte_offset_to_lsp_pos(source, sp.end.min(source.len()));
    json!({
        "start": start,
        "end": end
    })
}

fn byte_offset_to_lsp_pos(source: &str, byte_offset: usize) -> Value {
    let clamped = byte_offset.min(source.len());
    let prefix = &source[..clamped];
    let line = prefix.chars().filter(|&c| c == '\n').count() as u32;
    let last_newline = prefix.rfind('\n').map(|i| i + 1).unwrap_or(0);
    let character = prefix[last_newline..].encode_utf16().count() as u32;
    json!({ "line": line, "character": character })
}
