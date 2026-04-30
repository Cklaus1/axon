#![recursion_limit = "8192"]

pub mod ast;
pub mod builtins;
pub mod checker;
#[cfg(feature = "codegen")]
pub mod codegen;
pub mod error;
pub mod infer;
pub mod lexer;
pub mod parser;
pub mod resolver;
pub mod span;
pub mod token;
pub mod types;
// Phase 3
pub mod borrow;
pub mod comptime;
pub mod mono;
// Phase 4
pub mod cache;
pub mod doc;
pub mod fmt;
#[cfg(feature = "serde-json")]
pub mod lsp;

use std::collections::HashMap;

use lexer::{LexError, Lexer};
use parser::{ParseError, Parser};

#[derive(Debug, thiserror::Error)]
pub enum AxonError {
    #[error("lex error: {0}")]
    Lex(#[from] LexError),
    #[error("parse error: {0}")]
    Parse(#[from] ParseError),
}

pub fn parse_source(src: &str) -> Result<ast::Program, AxonError> {
    let raw = Lexer::tokenize_with_newlines(src)?;
    let mut tokens = Vec::with_capacity(raw.len());
    let mut spans = Vec::with_capacity(raw.len());
    let mut newlines = Vec::with_capacity(raw.len());
    for (tok, range, nl) in raw {
        spans.push(span::Span::new(range.start, range.end));
        tokens.push(tok);
        newlines.push(nl);
    }
    let program = Parser::with_newlines(tokens, spans, newlines).parse_program()?;
    Ok(program)
}

/// Parse source and return both the AST and the raw token+span list.
/// Used by the LSP server and formatter (Phase 4) which need source positions.
pub fn parse_source_with_spans(
    src: &str,
) -> Result<(ast::Program, Vec<(token::Token, std::ops::Range<usize>)>), AxonError> {
    let raw = Lexer::tokenize_with_newlines(src)?;
    let mut tokens = Vec::with_capacity(raw.len());
    let mut spans_ast = Vec::with_capacity(raw.len());
    let mut newlines = Vec::with_capacity(raw.len());
    let mut token_spans: Vec<(token::Token, std::ops::Range<usize>)> = Vec::with_capacity(raw.len());
    for (tok, range, nl) in raw {
        spans_ast.push(span::Span::new(range.start, range.end));
        newlines.push(nl);
        token_spans.push((tok.clone(), range));
        tokens.push(tok);
    }
    let program = Parser::with_newlines(tokens, spans_ast, newlines).parse_program()?;
    Ok((program, token_spans))
}

/// Serialize a `Program` to pretty-printed JSON.
///
/// This function lives in the lib (not the binary) to avoid serde_json pulling
/// in trait impls that overflow the compiler's recursion limit when combined
/// with inkwell's large type universe in the binary crate.
#[cfg(feature = "serde-json")]
pub fn program_to_json(program: &ast::Program) -> Result<String, String> {
    serde_json::to_string_pretty(program).map_err(|e| e.to_string())
}

/// A single structured diagnostic from any pipeline stage.
#[derive(Debug, Clone)]
pub struct PipelineDiagnostic {
    pub code: String,
    pub message: String,
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub severity: String,
    pub caret: String,
}

impl PipelineDiagnostic {
    pub fn display(&self) -> String {
        let loc = if self.line > 0 {
            format!("{}:{}:{}", self.file, self.line, self.col)
        } else {
            self.file.clone()
        };
        let mut s = format!("{}: {}[{}]: {}", loc, self.severity, self.code, self.message);
        if !self.caret.is_empty() {
            s.push('\n');
            s.push_str(&self.caret);
        }
        s
    }
}

/// An error detected while merging multiple source files.
#[derive(Debug, Clone)]
pub struct MergeError {
    pub code: &'static str,
    pub message: String,
    pub file: String,
}

/// Merge multiple parsed programs into a single global namespace.
///
/// Files are processed in the order given (command-line order). Items from all
/// files are merged into one `Program` so the subsequent pipeline stages see a
/// single global scope. Duplicate top-level names across files produce
/// [`error::E0903`] errors; the second definition is dropped from the merged
/// output so later passes still have a consistent (if incomplete) AST.
///
/// Items without names (`UseDecl`, `ImplBlock`) are always included.
pub fn merge_programs(
    file_programs: Vec<(String, ast::Program)>,
) -> (ast::Program, Vec<MergeError>) {
    let mut merged: Vec<ast::Item> = Vec::new();
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut errors: Vec<MergeError> = Vec::new();

    for (file, program) in file_programs {
        for item in program.items {
            let name = top_level_name(&item);
            if let Some(name) = name {
                if let Some(first_file) = seen.get(&name) {
                    errors.push(MergeError {
                        code: error::E0903,
                        message: format!(
                            "'{name}' already defined (first: {first_file}; redefined: {file})"
                        ),
                        file: file.clone(),
                    });
                    // Drop the duplicate; keep the first definition.
                } else {
                    seen.insert(name, file.clone());
                    merged.push(item);
                }
            } else {
                merged.push(item);
            }
        }
    }

    (ast::Program { items: merged }, errors)
}

/// Extract the declared name from a top-level item, if it has one.
fn top_level_name(item: &ast::Item) -> Option<String> {
    match item {
        ast::Item::FnDef(f) => Some(f.name.clone()),
        ast::Item::TypeDef(t) => Some(t.name.clone()),
        ast::Item::EnumDef(e) => Some(e.name.clone()),
        ast::Item::TraitDef(t) => Some(t.name.clone()),
        ast::Item::ModDecl(m) => Some(m.name.clone()),
        ast::Item::LetDef { name, .. } => Some(name.clone()),
        ast::Item::ImplBlock(_) | ast::Item::UseDecl(_) => None,
    }
}

/// Pretty-print an Axon program to canonical source.
///
/// The output is idempotent: formatting an already-formatted file produces
/// identical output. See `spec/compiler-phase4.md §2` for formatting rules.
pub fn format_program(program: &ast::Program) -> String {
    fmt::format_program(program)
}

pub fn generate_docs(program: &ast::Program, source: &str, filename: &str) -> String {
    doc::generate_docs(program, source, filename)
}

#[cfg(feature = "codegen")]
pub fn compile_bitcode_to_binary(
    bitcode: &[u8],
    output_path: &str,
    release: bool,
    target_triple: Option<&str>,
) -> Result<(), String> {
    codegen::compile_bitcode_to_binary(bitcode, output_path, release, target_triple)
}

/// Result of running the full analysis pipeline on a source text.
/// Used by the LSP server.
#[cfg(feature = "serde-json")]
pub struct AnalysisResult {
    pub program: Option<ast::Program>,
    pub infer_ctx: Option<infer::InferCtx>,
    pub diagnostics: Vec<lsp::LspDiagnostic>,
}

/// Run the full analysis pipeline (parse → resolve → infer → check → borrow)
/// on `source` text and return results suitable for the LSP server.
#[cfg(feature = "serde-json")]
pub fn analyse(source: &str, uri: &str) -> AnalysisResult {
    lsp::analyse_source(source, uri)
}

/// Parse multiple source files in parallel.
///
/// Returns a vec of `(filename, Program)` pairs in the same order as `paths`,
/// or a vec of error messages if any file fails to read or parse.
pub fn parse_source_files(
    paths: &[std::path::PathBuf],
) -> Result<Vec<(String, ast::Program)>, Vec<String>> {
    use std::sync::{Arc, Mutex};

    let errors: Arc<Mutex<Vec<(usize, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let results: Arc<Mutex<Vec<Option<(String, ast::Program)>>>> =
        Arc::new(Mutex::new(vec![None; paths.len()]));

    let handles: Vec<_> = paths
        .iter()
        .enumerate()
        .map(|(idx, path)| {
            let path = path.clone();
            let errors = Arc::clone(&errors);
            let results = Arc::clone(&results);
            std::thread::spawn(move || {
                let file = path.display().to_string();
                let src = match std::fs::read_to_string(&path) {
                    Ok(s) => s,
                    Err(e) => {
                        errors.lock().unwrap().push((idx, format!("cannot read {file}: {e}")));
                        return;
                    }
                };
                match parse_source(&src) {
                    Ok(program) => {
                        results.lock().unwrap()[idx] = Some((file, program));
                    }
                    Err(e) => {
                        errors.lock().unwrap().push((idx, format!("{file}: {e}")));
                    }
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("parse thread panicked");
    }

    let errs = Arc::try_unwrap(errors).unwrap().into_inner().unwrap();
    if !errs.is_empty() {
        let mut msgs: Vec<_> = errs;
        msgs.sort_by_key(|(i, _)| *i);
        return Err(msgs.into_iter().map(|(_, m)| m).collect());
    }

    let parsed = Arc::try_unwrap(results).unwrap().into_inner().unwrap();
    Ok(parsed.into_iter().map(|opt| opt.unwrap()).collect())
}

// ── Cache re-exports (Phase 4 §4) ────────────────────────────────────────────

pub use cache::{
    cache_key, cache_path, clean_cache, default_cache_dir, read_axc, write_axc,
};

// ── AXON_PATH module loading (Phase 4 §6) ────────────────────────────────────

/// Build the ordered list of directories to search for Axon modules.
///
/// Search order (spec §6):
/// 1. Each entry in `AXON_PATH` (colon-separated on Unix, semicolon on Windows).
/// 2. `~/.axon/lib/`
/// 3. `<dir of axon binary>/../lib/axon/`
///
/// Pass `binary_path` as `std::env::current_exe().ok()` for option 3.
pub fn axon_search_dirs(binary_path: Option<&std::path::Path>) -> Vec<std::path::PathBuf> {
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();

    // 1. AXON_PATH env var.
    if let Ok(axon_path) = std::env::var("AXON_PATH") {
        let sep = if cfg!(windows) { ';' } else { ':' };
        for entry in axon_path.split(sep) {
            if !entry.is_empty() {
                dirs.push(std::path::PathBuf::from(entry));
            }
        }
    }

    // 2. ~/.axon/lib/
    let home_key = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    if let Ok(home) = std::env::var(home_key) {
        dirs.push(std::path::PathBuf::from(home).join(".axon").join("lib"));
    }

    // 3. <binary dir>/../lib/axon/
    if let Some(bin) = binary_path {
        if let Some(bin_dir) = bin.parent() {
            dirs.push(bin_dir.join("..").join("lib").join("axon"));
        }
    }

    dirs
}

/// Load source files referenced by `use` declarations in `program` and merge
/// their items into `program`.
///
/// For each `use a::b::c` declaration, the compiler searches for the file
/// `a/b/c.ax` in each directory in `search_dirs` (in order). The first match
/// wins. Items from found modules are prepended to `program.items` so they are
/// visible to the main program during name resolution.
///
/// Returns E0901 errors for any modules that could not be found and E0902 errors
/// for circular imports. Parse errors inside found module files are also returned
/// as E0901 errors. Already-loaded module paths are skipped (no double-loading).
pub fn load_use_decls(
    program: &mut ast::Program,
    search_dirs: &[std::path::PathBuf],
) -> Vec<MergeError> {
    let use_paths: Vec<Vec<String>> = program
        .items
        .iter()
        .filter_map(|item| {
            if let ast::Item::UseDecl(u) = item {
                if !u.path.is_empty() {
                    return Some(u.path.clone());
                }
            }
            None
        })
        .collect();

    if use_paths.is_empty() || search_dirs.is_empty() {
        return Vec::new();
    }

    let mut errors: Vec<MergeError> = Vec::new();
    let mut loaded_items: Vec<ast::Item> = Vec::new();
    // `already_loaded` prevents double-loading; `loading_stack` detects cycles.
    let mut already_loaded: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut loading_stack: Vec<String> = Vec::new();

    for use_path in use_paths {
        load_module_recursive(
            &use_path,
            search_dirs,
            &mut already_loaded,
            &mut loading_stack,
            &mut loaded_items,
            &mut errors,
        );
    }

    // Prepend loaded module items so they are visible to the main program.
    if !loaded_items.is_empty() {
        let mut new_items = loaded_items;
        new_items.append(&mut program.items);
        program.items = new_items;
    }

    errors
}

/// Recursively load a module and all its transitive `use` dependencies.
///
/// `loading_stack` tracks the chain of modules currently being loaded.  If a
/// module is found in the stack, that is a cycle (E0902).  `already_loaded`
/// prevents any module from being loaded more than once.
fn load_module_recursive(
    use_path: &[String],
    search_dirs: &[std::path::PathBuf],
    already_loaded: &mut std::collections::HashSet<String>,
    loading_stack: &mut Vec<String>,
    loaded_items: &mut Vec<ast::Item>,
    errors: &mut Vec<MergeError>,
) {
    let path_str = use_path.join("::");

    // Already fully loaded — nothing to do.
    if already_loaded.contains(&path_str) {
        return;
    }

    // Currently loading — circular import detected.
    if loading_stack.contains(&path_str) {
        let cycle: Vec<String> = loading_stack
            .iter()
            .skip_while(|s| *s != &path_str)
            .cloned()
            .collect();
        let cycle_str = if cycle.is_empty() {
            format!("{path_str} → {path_str}")
        } else {
            format!("{} → {path_str}", cycle.join(" → "))
        };
        errors.push(MergeError {
            code: error::E0902,
            message: format!("circular import detected: {cycle_str}"),
            file: String::new(),
        });
        return;
    }

    // Build relative file path: `a::b::c` → `a/b/c.ax`.
    let mut rel = std::path::PathBuf::new();
    for segment in use_path {
        rel.push(segment);
    }
    rel.set_extension("ax");

    let mut found = false;
    let mut searched: Vec<String> = Vec::new();

    for dir in search_dirs {
        let candidate = dir.join(&rel);
        searched.push(candidate.display().to_string());
        if !candidate.exists() {
            continue;
        }

        match std::fs::read_to_string(&candidate) {
            Ok(src) => match parse_source(&src) {
                Ok(mod_prog) => {
                    // Mark as in-progress before recursing to detect cycles.
                    loading_stack.push(path_str.clone());

                    // Collect transitive `use` declarations from this module
                    // and load them first (depth-first).
                    let nested_uses: Vec<Vec<String>> = mod_prog
                        .items
                        .iter()
                        .filter_map(|item| {
                            if let ast::Item::UseDecl(u) = item {
                                if !u.path.is_empty() {
                                    return Some(u.path.clone());
                                }
                            }
                            None
                        })
                        .collect();

                    for nested in nested_uses {
                        load_module_recursive(
                            &nested,
                            search_dirs,
                            already_loaded,
                            loading_stack,
                            loaded_items,
                            errors,
                        );
                    }

                    // Add this module's items after its dependencies.
                    loaded_items.extend(mod_prog.items);

                    loading_stack.pop();
                    already_loaded.insert(path_str.clone());
                    found = true;
                    break;
                }
                Err(e) => {
                    errors.push(MergeError {
                        code: error::E0901,
                        message: format!(
                            "module `{path_str}` at {}: {e}",
                            candidate.display()
                        ),
                        file: candidate.display().to_string(),
                    });
                    found = true; // file found but broken — don't also report not-found
                    break;
                }
            },
            Err(e) => {
                // I/O error on this candidate — try next directory.
                searched
                    .last_mut()
                    .map(|s| s.push_str(&format!(" (read error: {e})")));
            }
        }
    }

    if !found {
        let detail = searched
            .iter()
            .map(|s| format!("    {s} (not found)"))
            .collect::<Vec<_>>()
            .join("\n");
        errors.push(MergeError {
            code: error::E0901,
            message: format!("module `{path_str}` not found\n  searched:\n{detail}"),
            file: String::new(),
        });
    }
}

/// Run the full check pipeline (parse → resolve → infer → check → borrow)
/// and return all diagnostics with source locations.
pub fn check_pipeline(
    source: &str,
    file: &str,
) -> Vec<PipelineDiagnostic> {
    let source_map = span::SourceMap::new(source.to_string());
    let mut out: Vec<PipelineDiagnostic> = Vec::new();

    let mut program = match parse_source(source) {
        Ok(p) => p,
        Err(e) => {
            out.push(PipelineDiagnostic {
                code: "E0000".into(),
                message: e.to_string(),
                file: file.to_string(),
                line: 0,
                col: 0,
                severity: "error".into(),
                caret: String::new(),
            });
            return out;
        }
    };

    let resolve_result = resolver::resolve_program(&mut program, file);
    for d in &resolve_result.errors {
        let (line, col) = if !d.span.is_dummy() {
            let (l, c) = source_map.line_col(d.span.start);
            (l as u32, c as u32)
        } else {
            (d.line, d.col)
        };
        let caret = if !d.span.is_dummy() {
            source_map.render_caret(d.span)
        } else {
            String::new()
        };
        let severity = match d.severity {
            resolver::Severity::Error => "error",
            resolver::Severity::Warning => "warning",
            resolver::Severity::Info => "note",
        };
        out.push(PipelineDiagnostic {
            code: d.code.to_string(),
            message: d.message.clone(),
            file: file.to_string(),
            line,
            col,
            severity: severity.into(),
            caret,
        });
    }

    resolver::fill_captures(&mut program);
    let mut infer_ctx = infer::InferCtx::new(file);
    let _subst = infer_ctx.infer_program(&mut program);
    for e in &infer_ctx.errors {
        let (line, col) = if !e.span.is_dummy() {
            let (l, c) = source_map.line_col(e.span.start);
            (l as u32, c as u32)
        } else {
            (0, 0)
        };
        let caret = if !e.span.is_dummy() {
            source_map.render_caret(e.span)
        } else {
            String::new()
        };
        out.push(PipelineDiagnostic {
            code: e.code.to_string(),
            message: e.message.clone(),
            file: file.to_string(),
            line,
            col,
            severity: "error".into(),
            caret,
        });
    }

    let fn_sigs: std::collections::HashMap<String, checker::FnSig> = infer_ctx.fn_sigs
        .iter()
        .map(|(k, v)| (k.clone(), checker::FnSig { params: v.params.clone(), ret: v.ret.clone() }))
        .collect();
    let mut check_ctx = checker::CheckCtx::new(file, fn_sigs, infer_ctx.struct_fields);
    let check_errors = check_ctx.check_program(&mut program, std::collections::HashMap::new());
    for e in &check_errors {
        let (line, col) = if !e.span.is_dummy() {
            let (l, c) = source_map.line_col(e.span.start);
            (l as u32, c as u32)
        } else {
            (e.line, e.col)
        };
        let caret = if !e.span.is_dummy() {
            source_map.render_caret(e.span)
        } else {
            String::new()
        };
        let severity = match e.severity {
            checker::Severity::Error => "error",
            checker::Severity::Warning => "warning",
            checker::Severity::Info => "note",
        };
        out.push(PipelineDiagnostic {
            code: e.code.to_string(),
            message: e.message.clone(),
            file: file.to_string(),
            line,
            col,
            severity: severity.into(),
            caret,
        });
    }

    // Borrow checking
    for item in &program.items {
        if let ast::Item::FnDef(fndef) = item {
            let param_types: std::collections::HashMap<String, types::Type> =
                if let Some(sig) = infer_ctx.fn_sigs.get(&fndef.name) {
                    fndef.params.iter()
                        .zip(sig.params.iter())
                        .map(|(p, t)| (p.name.clone(), t.clone()))
                        .collect()
                } else {
                    std::collections::HashMap::new()
                };
            for err in borrow::check_fn(fndef, param_types) {
                let span = err.span();
                let (line, col) = if !span.is_dummy() {
                    let (l, c) = source_map.line_col(span.start);
                    (l as u32, c as u32)
                } else {
                    (0, 0)
                };
                let caret = if !span.is_dummy() {
                    source_map.render_caret(span)
                } else {
                    String::new()
                };
                let code = match &err {
                    borrow::BorrowError::UseAfterMove { .. } => error::E0601,
                    borrow::BorrowError::MoveBorrowed { .. } => error::E0602,
                    borrow::BorrowError::BorrowConflict { .. } => error::E0603,
                };
                out.push(PipelineDiagnostic {
                    code: code.to_string(),
                    message: err.to_string(),
                    file: file.to_string(),
                    line,
                    col,
                    severity: "error".into(),
                    caret,
                });
            }
        }
    }

    out
}
