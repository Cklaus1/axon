#![recursion_limit = "8192"]

/// Tier → model routing for AI calls (R3 §4.2): cheap/balanced/strong.
pub mod ai_routing;
pub mod ast;
pub mod builtins;
pub mod checker;
#[cfg(feature = "codegen")]
pub mod codegen;
/// Versioned machine-stable diagnostic JSON (R8): `axon-diag/1` schema.
pub mod diag_schema;
pub mod error;
pub mod infer;
pub mod host;
/// Self-improving-compiler pass verification harness (R10): G1 oracle + G2 caps.
pub mod improve;
pub mod improve_templates;
pub mod lexer;
/// `axon.lock` content-addressed import lockfile (R6): hash + format.
pub mod lockfile;
/// Graduated-pass manifest (R10): multi-sig graduation gate + format.
pub mod manifest;
pub mod parser;
pub mod resolver;
/// SMT-backed `@[verify]` static proof (R9, `smt` feature → Z3).
#[cfg(feature = "smt")]
pub mod smt;
pub mod span;
pub mod token;
pub mod types;
// Phase 3
pub mod borrow;
pub mod comptime;
/// Codegen-free tree-walking interpreter (`axon run` without LLVM).
pub mod interp;
/// Phase 7 (R12) kernel runtime services — Slice 1: `principal_authority`
/// (live principal registry with kernel-enforced attenuation). Interp-driven so
/// the codegen build is untouched (R12 §9 Q3).
pub mod kernel;
pub mod mono;
// Phase 4
pub mod audit;
pub mod cache;
pub mod capabilities;
pub mod complexity;
pub mod effects;
pub mod doc;
pub mod fmt;
#[cfg(feature = "serde-json")]
pub mod lsp;
// ASI Layer-2
pub mod verify;

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

/// A token paired with its byte span in the source, as produced by the lexer.
pub type TokenSpan = (token::Token, std::ops::Range<usize>);

/// A parsed file: its path/name plus the resulting AST.
pub type NamedProgram = (String, ast::Program);

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

/// R8: parse source, returning on failure the error message AND the byte offset
/// where the parser stopped, so a caller can resolve it to `line:col` (parse
/// errors are otherwise span-less). On a lexer error the offset is 0 (the lexer
/// reports its own position in the message). `Ok` returns just the program.
pub fn parse_source_located(src: &str) -> Result<ast::Program, (String, usize)> {
    let raw = Lexer::tokenize_with_newlines(src).map_err(|e| (e.to_string(), 0usize))?;
    let mut tokens = Vec::with_capacity(raw.len());
    let mut spans = Vec::with_capacity(raw.len());
    let mut newlines = Vec::with_capacity(raw.len());
    for (tok, range, nl) in raw {
        spans.push(span::Span::new(range.start, range.end));
        tokens.push(tok);
        newlines.push(nl);
    }
    match Parser::with_newlines(tokens, spans, newlines).parse_program_located() {
        Ok(p) => Ok(p),
        Err((e, span)) => Err((e.to_string(), span.start)),
    }
}

/// Parse source and return both the AST and the raw token+span list.
/// Used by the LSP server and formatter (Phase 4) which need source positions.
pub fn parse_source_with_spans(
    src: &str,
) -> Result<(ast::Program, Vec<TokenSpan>), AxonError> {
    let raw = Lexer::tokenize_with_newlines(src)?;
    let mut tokens = Vec::with_capacity(raw.len());
    let mut spans_ast = Vec::with_capacity(raw.len());
    let mut newlines = Vec::with_capacity(raw.len());
    let mut token_spans: Vec<TokenSpan> = Vec::with_capacity(raw.len());
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
#[derive(Debug, Clone, Default)]
pub struct PipelineDiagnostic {
    pub code: String,
    pub message: String,
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub severity: String,
    pub caret: String,
    /// R8 axon-diag/2: structured `expected` type, when the diagnostic is a type
    /// mismatch that carries one (else `None`, omitted from JSON).
    pub expected: Option<String>,
    /// R8 axon-diag/2: structured `found` type, paired with `expected`.
    pub found: Option<String>,
    /// R8 axon-diag/2: structured fix hint (`help`), when the error carries one.
    pub help: Option<String>,
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

    /// R8 typed end-to-end: emit one line of the versioned `axon-diag/1` schema
    /// **with source location** as first-class fields. Unlike
    /// [`diag_schema::diagnostic_json`] — which regex-recovers a code from a
    /// flattened string and has no location — this serialises a *typed*
    /// diagnostic, so `file`/`line`/`col` survive to the consumer (an editor or
    /// agent can jump to the offending span without re-parsing the source).
    ///
    /// Field order is fixed (`schema`, `severity`, `code`, `file`, `line`,
    /// `col`, `message`); `line`/`col` are omitted when 0 (a file-level
    /// diagnostic with no span — e.g. a missing-module error), never faked.
    /// Hand-rolled JSON, no `serde_json` (CLAUDE.md: it collides with inkwell).
    pub fn json(&self) -> String {
        fn q(s: &str) -> String {
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
        let mut s = String::with_capacity(self.message.len() + 96);
        s.push_str("{\"schema\":");
        s.push_str(&q(diag_schema::DIAG_SCHEMA));
        s.push_str(",\"severity\":");
        s.push_str(&q(&self.severity));
        s.push_str(",\"code\":");
        s.push_str(&q(&self.code));
        if !self.file.is_empty() {
            s.push_str(",\"file\":");
            s.push_str(&q(&self.file));
        }
        if self.line > 0 {
            s.push_str(&format!(",\"line\":{}", self.line));
            s.push_str(&format!(",\"col\":{}", self.col));
        }
        s.push_str(",\"message\":");
        s.push_str(&q(&self.message));
        // R8: structured type-mismatch + fix fields, each omitted when absent
        // (additive — consumers ignore unknown keys, so the schema stays
        // axon-diag/1; this is not a breaking change).
        if let Some(exp) = &self.expected {
            s.push_str(",\"expected\":");
            s.push_str(&q(exp));
        }
        if let Some(found) = &self.found {
            s.push_str(",\"found\":");
            s.push_str(&q(found));
        }
        if let Some(help) = &self.help {
            s.push_str(",\"help\":");
            s.push_str(&q(help));
        }
        s.push('}');
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
        ast::Item::RefineDef(r) => Some(r.name.clone()),
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
) -> Result<Vec<NamedProgram>, Vec<String>> {
    use std::sync::{Arc, Mutex};

    let errors: Arc<Mutex<Vec<(usize, String)>>> = Arc::new(Mutex::new(Vec::new()));
    let results: Arc<Mutex<Vec<Option<NamedProgram>>>> =
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

/// A module referenced by a `use`, resolved to its file on disk and its bytes.
/// Used by `axon lock` / `verify-lock` to content-hash each import (R6).
pub struct ResolvedModule {
    /// The `use` path joined with `::`, e.g. `scorelib::metric` (the lockfile name).
    pub name: String,
    /// Absolute or search-relative path to the resolved `.ax` file.
    pub path: std::path::PathBuf,
    /// The raw source bytes (what gets content-hashed).
    pub bytes: Vec<u8>,
}

/// Resolve a program's **direct** `use` declarations to the module files on
/// disk, returning each one's name + path + raw bytes. Mirrors
/// `load_use_decls`' search (`a::b::c` → `a/b/c.ax`, first match in
/// `search_dirs` wins) but does NOT merge or parse — it just locates and reads
/// the bytes, the input the content hash is computed over.
///
/// Returns `(resolved, unresolved_names)`: modules found, and `use` names for
/// which no file existed in any search dir (so the caller can report them).
/// Transitive `use`s inside modules are a follow-on slice; this covers the
/// direct edge the lockfile pins.
pub fn resolve_use_files(
    program: &ast::Program,
    search_dirs: &[std::path::PathBuf],
) -> (Vec<ResolvedModule>, Vec<String>) {
    let mut resolved = Vec::new();
    let mut unresolved = Vec::new();

    for item in &program.items {
        let ast::Item::UseDecl(u) = item else { continue };
        if u.path.is_empty() {
            continue;
        }
        let name = u.path.join("::");
        // `a::b::c` → `a/b/c.ax`.
        let mut rel = std::path::PathBuf::new();
        for segment in &u.path {
            rel.push(segment);
        }
        rel.set_extension("ax");

        let mut found = false;
        for dir in search_dirs {
            let candidate = dir.join(&rel);
            if let Ok(bytes) = std::fs::read(&candidate) {
                resolved.push(ResolvedModule { name: name.clone(), path: candidate, bytes });
                found = true;
                break;
            }
        }
        if !found {
            unresolved.push(name);
        }
    }
    (resolved, unresolved)
}

/// Resolve a program's `use` declarations to module files **transitively** —
/// the full import closure, not just the direct edge. When module A `use`s B
/// and B `use`s C, all three are returned (R6: `axon lock`/`verify-lock`/E1203
/// must pin/check every byte that joins the program, not only the first hop).
///
/// A worklist BFS: resolve the entry program's direct `use`s, parse each
/// resolved module, enqueue *its* `use`s, and repeat — deduplicating by the
/// `::`-joined name so a diamond (two modules importing the same third) is
/// resolved once and a cycle terminates. Modules whose file is missing, or that
/// fail to parse (so their transitive `use`s can't be read), are reported in
/// `unresolved`. Resolution order is deterministic (BFS over sorted-encounter
/// order), so the resulting list — and any hash computed from it — is stable.
pub fn resolve_use_files_transitive(
    program: &ast::Program,
    search_dirs: &[std::path::PathBuf],
) -> (Vec<ResolvedModule>, Vec<String>) {
    use std::collections::HashSet;

    // Extract the `use` names from a program as `a::b::c` strings.
    fn use_names(p: &ast::Program) -> Vec<Vec<String>> {
        p.items
            .iter()
            .filter_map(|it| match it {
                ast::Item::UseDecl(u) if !u.path.is_empty() => Some(u.path.clone()),
                _ => None,
            })
            .collect()
    }

    let mut resolved: Vec<ResolvedModule> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // Worklist seeded with the entry program's direct uses.
    let mut queue: std::collections::VecDeque<Vec<String>> = use_names(program).into_iter().collect();

    while let Some(use_path) = queue.pop_front() {
        let name = use_path.join("::");
        if !seen.insert(name.clone()) {
            continue; // already resolved (diamond / cycle)
        }
        // `a::b::c` → `a/b/c.ax`.
        let mut rel = std::path::PathBuf::new();
        for segment in &use_path {
            rel.push(segment);
        }
        rel.set_extension("ax");

        let mut found = false;
        for dir in search_dirs {
            let candidate = dir.join(&rel);
            let Ok(bytes) = std::fs::read(&candidate) else { continue };
            found = true;
            // Enqueue this module's own `use`s (the transitive step). A parse
            // failure means we can't see its imports — report it as unresolved
            // (its bytes are still pinned via the entry below).
            if let Ok(src) = std::str::from_utf8(&bytes) {
                match parse_source(src) {
                    Ok(modp) => {
                        for nested in use_names(&modp) {
                            if !seen.contains(&nested.join("::")) {
                                queue.push_back(nested);
                            }
                        }
                    }
                    Err(_) => unresolved.push(format!("{name} (unparseable — transitive uses not followed)")),
                }
            }
            resolved.push(ResolvedModule { name: name.clone(), path: candidate, bytes });
            break;
        }
        if !found {
            unresolved.push(name);
        }
    }
    (resolved, unresolved)
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
                if let Some(s) = searched.last_mut() {
                    s.push_str(&format!(" (read error: {e})"));
                }
            }
        }
    }

    if !found {
        let detail = searched
            .iter()
            .map(|s| format!("    {s} (not found)"))
            .collect::<Vec<_>>()
            .join("\n");
        // BUG_HUNT #34: a multi-segment `use a::b` is loaded as the NESTED
        // module file `a/b.ax`. A common mistake is writing `use a::b` (or
        // `use a.b`) meaning "import item `b` from the flat module `a`" — the
        // `mod a` + `use a.{b}` idiom every example uses. When `a/b.ax` isn't
        // found but the flat `a.ax` IS, say so explicitly instead of leaving the
        // user staring at a bare not-found for a path they didn't think they
        // wrote. (Checked before the AXON_PATH hint since it's more specific.)
        let item_import_hint = if use_path.len() >= 2 {
            let root = &use_path[0];
            let root_rel = std::path::PathBuf::from(format!("{root}.ax"));
            let flat_exists = search_dirs.iter().any(|d| d.join(&root_rel).exists());
            if flat_exists {
                let items = use_path[1..].join(", ");
                Some(format!(
                    "\n  hint: module `{root}` exists ({root}.ax), but `{path_str}` looks for a \
                     nested file `{}`. To import item(s) from `{root}`, write `use {root}.{{{items}}}` \
                     (the dot-brace form), not `use {}`.",
                    rel.display(),
                    use_path.join("::"),
                ))
            } else {
                None
            }
        } else {
            None
        };
        // Bug #10: the search dirs are install locations the user never
        // created; nearly every in-repo demo is run via AXON_PATH. When it's
        // unset, point the user at the lever they're actually missing.
        let hint = if let Some(h) = item_import_hint {
            h
        } else if std::env::var_os("AXON_PATH").is_none() {
            let modfile = rel.display();
            format!(
                "\n  hint: AXON_PATH is unset — set it to the directory containing `{modfile}` \
                 (e.g. `AXON_PATH=examples/stdlib axon run ...`)"
            )
        } else {
            String::new()
        };
        errors.push(MergeError {
            code: error::E0901,
            message: format!("module `{path_str}` not found\n  searched:\n{detail}{hint}"),
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
                expected: None,
                found: None,
                help: None,
            });
            return out;
        }
    };

    let resolve_result = resolver::resolve_program(&program, file);
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
            expected: None,
            found: None,
            help: None,
        });
    }

    resolver::fill_captures(&mut program);
    let mut infer_ctx = infer::InferCtx::new(file);
    let _subst = infer_ctx.infer_program(&program);
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
            expected: None,
            found: None,
            help: None,
        });
    }

    let fn_sigs: std::collections::HashMap<String, checker::FnSig> = infer_ctx.fn_sigs
        .iter()
        .map(|(k, v)| (k.clone(), checker::FnSig { params: v.params.clone(), ret: v.ret.clone() }))
        .collect();
    let mut check_ctx = checker::CheckCtx::new(file, fn_sigs, infer_ctx.struct_fields);
    let check_errors = check_ctx.check_program(&program, std::collections::HashMap::new());
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
            expected: None,
            found: None,
            help: None,
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
                    expected: None,
                    found: None,
                    help: None,
                });
            }
        }
    }

    // Capability checking (@[contained])
    for err in capabilities::check_capabilities(&program) {
        let (line, col) = if !err.span.is_dummy() {
            let (l, c) = source_map.line_col(err.span.start);
            (l as u32, c as u32)
        } else {
            (0, 0)
        };
        let caret = if !err.span.is_dummy() {
            source_map.render_caret(err.span)
        } else {
            String::new()
        };
        out.push(PipelineDiagnostic {
            code: err.code.to_string(),
            message: err.message.clone(),
            file: file.to_string(),
            line,
            col,
            severity: "error".into(),
            caret,
            expected: None,
            found: None,
            help: None,
        });
    }

    // Phase 6 effect-row checking (§2 subsumption E02/E05 → E1310)
    for err in effects::check_effects(&program) {
        let (line, col) = if !err.span.is_dummy() {
            let (l, c) = source_map.line_col(err.span.start);
            (l as u32, c as u32)
        } else {
            (0, 0)
        };
        let caret = if !err.span.is_dummy() {
            source_map.render_caret(err.span)
        } else {
            String::new()
        };
        out.push(PipelineDiagnostic {
            code: err.code.to_string(),
            message: err.message.clone(),
            file: file.to_string(),
            line,
            col,
            severity: "error".into(),
            caret,
            expected: None,
            found: None,
            help: None,
        });
    }

    // Verify checking (@[verify(...)])
    for err in verify::check_verify(&program) {
        let (line, col) = if !err.span.is_dummy() {
            let (l, c) = source_map.line_col(err.span.start);
            (l as u32, c as u32)
        } else {
            (0, 0)
        };
        let caret = if !err.span.is_dummy() {
            source_map.render_caret(err.span)
        } else {
            String::new()
        };
        out.push(PipelineDiagnostic {
            code: err.code.to_string(),
            message: err.message.clone(),
            file: file.to_string(),
            line,
            col,
            severity: "error".into(),
            caret,
            expected: None,
            found: None,
            help: None,
        });
    }

    out
}
