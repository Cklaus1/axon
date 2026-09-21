#![recursion_limit = "8192"]

/// Tier → model routing for AI calls (R3 §4.2): cheap/balanced/strong.
pub mod ai_routing;
pub mod ast;
pub mod builtins;
/// R23 mint cert gate — solver-free certificate check of the kernel mint
/// obligations, compiled in EVERY build (Z3 not required).
pub mod cert_gate;
pub mod checker;
pub mod clock;
#[cfg(feature = "codegen")]
pub mod codegen;
/// R21 — exact base-10 fixed-point `Decimal` arithmetic (money-safe, i128-backed).
pub mod decimal;
/// Versioned machine-stable diagnostic JSON (R8): `axon-diag/1` schema.
pub mod diag_schema;
pub mod env_registry;
pub mod error;
pub mod host;
/// Self-improving-compiler pass verification harness (R10): G1 oracle + G2 caps.
pub mod improve;
pub mod improve_templates;
pub mod infer;
pub mod lexer;
/// `axon.lock` content-addressed import lockfile (R6): hash + format.
pub mod lockfile;
/// Graduated-pass manifest (R10): multi-sig graduation gate + format.
pub mod manifest;
/// R14 — Mobile targets (iOS/Android): host-agnostic triple recognition, the
/// deterministic Swift/Kotlin wrapper generator, and the E1710 toolchain probe.
/// Pure logic — compiles on every host; the iOS-specific link/FFI lives behind
/// `cfg(target_os="ios")` in `axon-rt`, not here.
pub mod mobile;
/// R13 native FFI: the curated native-module registry (single source of truth
/// shared by resolver/infer/checker/borrow/effects/codegen/interp).
pub mod native;
pub mod parse_help;
pub mod parser;
pub mod preflight;
pub mod replay;
pub mod resolver;
/// Self-improving-compiler Layer 3 (prototype): AI-authored passes as DATA — a
/// validated, total, capability-free `RewriteSpec` compiled to a verifiable pass.
pub mod rewrite_dsl;
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
pub mod doc;
pub mod effects;
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
    parse_source_in(src, span::SourceId::UNKNOWN)
}

/// Parse `src`, stamping every span with `sid` — the identity of the file the
/// bytes came from.
///
/// This is one of the three places in the compiler where a real span is BORN
/// (the other two are `parse_source_located_in` and `parse_source_with_spans`);
/// everywhere else a span is copied or re-derived from one of these. Stamping
/// here is therefore enough to give the whole AST file identity, and is why
/// `load_use_decls` can merge a module's items into the entry program without
/// their offsets becoming unattributable.
pub fn parse_source_in(src: &str, sid: span::SourceId) -> Result<ast::Program, AxonError> {
    let raw = Lexer::tokenize_with_newlines(src)?;
    let mut tokens = Vec::with_capacity(raw.len());
    let mut spans = Vec::with_capacity(raw.len());
    let mut newlines = Vec::with_capacity(raw.len());
    for (tok, range, nl) in raw {
        spans.push(span::Span::with_source(range.start, range.end, sid));
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
/// Recover the byte offset from a lexer error message of the form
/// `… at 60..61`. Returns `None` when the message has no such span, in which
/// case the caller falls back to 0 — the previous behaviour for every message.
fn lex_error_offset(msg: &str) -> Option<usize> {
    let at = msg.rfind(" at ")? + 4;
    let rest = &msg[at..];
    let end = rest.find("..")?;
    rest[..end].trim().parse::<usize>().ok()
}

pub fn parse_source_located(src: &str) -> Result<ast::Program, (String, usize)> {
    parse_source_located_in(src, span::SourceId::UNKNOWN)
}

/// `parse_source_located`, stamping each span with the file it came from.
pub fn parse_source_located_in(
    src: &str,
    sid: span::SourceId,
) -> Result<ast::Program, (String, usize)> {
    // A lex error's offset used to be discarded (`0usize`), so every
    // lexer-tier diagnostic reported line 1 column 1 no matter where the bad
    // character was — the same "a hint that cannot say where is half a repair"
    // defect AXON_FOR_RLM §2 fixed at the parse tier, one tier lower. The
    // message already carries the span as `… at 60..61`, so the offset is
    // recoverable without changing the lexer's error type.
    let raw = Lexer::tokenize_with_newlines(src).map_err(|e| {
        let msg = e.to_string();
        let offset = lex_error_offset(&msg).unwrap_or(0);
        (msg, offset)
    })?;
    let mut tokens = Vec::with_capacity(raw.len());
    let mut spans = Vec::with_capacity(raw.len());
    let mut newlines = Vec::with_capacity(raw.len());
    for (tok, range, nl) in raw {
        spans.push(span::Span::with_source(range.start, range.end, sid));
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
pub fn parse_source_with_spans(src: &str) -> Result<(ast::Program, Vec<TokenSpan>), AxonError> {
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
        let mut s = format!(
            "{}: {}[{}]: {}",
            loc, self.severity, self.code, self.message
        );
        if !self.caret.is_empty() {
            s.push('\n');
            s.push_str(&self.caret);
        }
        // The repair hint, which this renderer dropped entirely.
        //
        // `help` was serialised into `axon-diag/1` and nowhere else, so every
        // hint in the compiler — the spelling suggestion on E0001, the
        // foreign-keyword hints at the parse tier, "match it: `match x { Ok(v)
        // => … }`" — reached JSON consumers and was invisible to anyone reading
        // the terminal. AXON_FOR_RLM §2 is "stop `axon run` stripping help";
        // this is the same defect one layer down, where the stripping is
        // unconditional.
        //
        // Named `help:` to match the field and the JSON key rather than
        // `AxonError::display`'s `fix:`, so a reader grepping either surface for
        // the same hint finds it under the same word.
        if let Some(help) = &self.help {
            s.push_str("\n       help: ");
            s.push_str(help);
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
pub fn parse_source_files(paths: &[std::path::PathBuf]) -> Result<Vec<NamedProgram>, Vec<String>> {
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
                        errors
                            .lock()
                            .unwrap()
                            .push((idx, format!("cannot read {file}: {e}")));
                        return;
                    }
                };
                // Each file gets its own id: `axon test a.ax b.ax` merges
                // these programs and then renders every diagnostic against
                // `files[0]`, so without identity a diagnostic from `b.ax` is
                // labelled `a.ax`.
                let sid = span::intern_source(&file, &src);
                match parse_source_in(&src, sid) {
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

pub use cache::{cache_key, cache_path, clean_cache, default_cache_dir, read_axc, write_axc};

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
        let ast::Item::UseDecl(u) = item else {
            continue;
        };
        if u.path.is_empty() {
            continue;
        }
        // R13: `use native::M` is a curated native-module import, not a `.ax`
        // file — it is resolved by the in-compiler registry (and gated by
        // E1800/E1004), never loaded from AXON_PATH. Skip it here.
        if u.path.first().map(String::as_str) == Some("native") {
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
                resolved.push(ResolvedModule {
                    name: name.clone(),
                    path: candidate,
                    bytes,
                });
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
    let mut queue: std::collections::VecDeque<Vec<String>> =
        use_names(program).into_iter().collect();

    while let Some(use_path) = queue.pop_front() {
        // R13: `use native::M` is a registry import, not a `.ax` file — skip.
        if use_path.first().map(String::as_str) == Some("native") {
            continue;
        }
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
            let Ok(bytes) = std::fs::read(&candidate) else {
                continue;
            };
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
                    Err(_) => unresolved.push(format!(
                        "{name} (unparseable — transitive uses not followed)"
                    )),
                }
            }
            resolved.push(ResolvedModule {
                name: name.clone(),
                path: candidate,
                bytes,
            });
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
    // R13: `use native::M` is a curated registry import, not a `.ax` file — it
    // is never loaded/merged from disk (the registry + E1800/E1004 own it).
    if use_path.first().map(String::as_str) == Some("native") {
        return;
    }

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
            // Parse the module with ITS OWN source id. Without this, the items
            // prepended into the entry program below carry offsets into this
            // file wearing no identity at all, and the renderer resolves them
            // against the ENTRY file's SourceMap — which is how an error truly
            // at `lib.ax:14` came out as `main.ax:6`.
            Ok(src) => match parse_source_in(
                &src,
                span::intern_source(&candidate.display().to_string(), &src),
            ) {
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
                        message: format!("module `{path_str}` at {}: {e}", candidate.display()),
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
pub fn check_pipeline(source: &str, file: &str) -> Vec<PipelineDiagnostic> {
    let source_map = span::SourceMap::new(source.to_string());
    let mut out: Vec<PipelineDiagnostic> = Vec::new();

    // AXON_FOR_RLM §1/§2: parse with the LOCATED variant so a parse error
    // resolves to a line:col and can carry a fix hint. The unlocated
    // `parse_source` was why this diagnostic reported line 0 with no help — the
    // offset it needs for both was thrown away one call earlier.
    // M5: the parser records where it accepted a foreign `mut`. Clear first, so
    // a previous parse's notes cannot leak into this one's diagnostics.
    parser::clear_accepted_mut();
    let mut program = match parse_source_located(source) {
        Ok(p) => p,
        Err((msg, offset)) => {
            let (line, col) = source_map.line_col(offset);
            let help = parse_help::parse_help(&msg, source, offset);
            out.push(PipelineDiagnostic {
                code: "E0000".into(),
                message: msg,
                file: file.to_string(),
                line: line as u32,
                col: col as u32,
                severity: "error".into(),
                caret: String::new(),
                expected: None,
                found: None,
                help,
            });
            return out;
        }
    };

    // M5: surface each accepted `mut` as an INFO. The parser no longer refuses
    // it — accepting asserts nothing false, since Axon locals are already
    // reassignable — but a human reading the code should still learn that the
    // keyword did nothing, so the note is emitted rather than the program
    // silently compiling as if `mut` had never been written.
    for offset in parser::take_accepted_mut() {
        let (line, col) = source_map.line_col(offset);
        out.push(PipelineDiagnostic {
            code: error::I0002.to_string(),
            message: "`mut` is not an Axon keyword and was ignored — bindings are \
                      already reassignable"
                .to_string(),
            file: file.to_string(),
            line: line as u32,
            col: col as u32,
            severity: "note".into(),
            caret: String::new(),
            expected: None,
            found: None,
            help: Some("drop it: `let x = …`, then assign with `x = …`".to_string()),
        });
    }

    // `x = 7 // 2` — Python floor division. Axon reads `//` as a line comment,
    // so the expression silently becomes `7` and the program compiles clean:
    //
    //     let x = 7 // 2      Python: 3.   Axon: 7, no diagnostic at any tier.
    //
    // That is the worst outcome an error message can have, which is to say no
    // error message: every other foreign habit in this file at least FAILS, so
    // the reader knows to look. This one produces a wrong answer quietly.
    for offset in empty_block_binding_offsets(source) {
        let (line, col) = source_map.line_col(offset);
        out.push(PipelineDiagnostic {
            code: error::W0008.to_string(),
            message: "`{}` here is an empty BLOCK, whose value is `()` — Axon has \
                      no dict literal"
                .to_string(),
            file: file.to_string(),
            line: line as u32,
            col: col as u32,
            severity: "warning".into(),
            caret: String::new(),
            expected: None,
            found: None,
            help: Some(
                "start the map empty and fill it: `let d = dict_new()` then \
                 `dict_set(d, \"a\", 1)`; or build it from pairs with \
                 `dict_from_pairs([(\"a\", 1)])`"
                    .to_string(),
            ),
        });
    }

    for offset in floor_division_comment_offsets(source) {
        let (line, col) = source_map.line_col(offset);
        out.push(PipelineDiagnostic {
            code: error::W0007.to_string(),
            message: "`//` starts a comment in Axon — the rest of this line was \
                      discarded, not divided"
                .to_string(),
            file: file.to_string(),
            line: line as u32,
            col: col as u32,
            severity: "warning".into(),
            caret: String::new(),
            expected: None,
            found: None,
            help: Some(
                "if you meant Python's floor division, write `/` (integer `/` \
                 already truncates in Axon); if you meant a comment, move it to \
                 its own line"
                    .to_string(),
            ),
        });
    }

    let resolve_result = resolver::resolve_program(&program, file);
    // `.errors` AND `.warnings`. Reading only `.errors` silently dropped every
    // resolver warning from this pipeline — and this pipeline is what the
    // browser playground (`axon-wasm`) shows, so the playground reported fewer
    // problems than `axon check` on the same source. Measured over 19 probe
    // programs: W0006 (unused binding), W0002 (shadowing), W0003 (user fn
    // shadows a builtin) and W2001 (vague `@[goal]`) appeared in the CLI and
    // nowhere here. The CLI reads both; the note at `run_check_pipeline_located`
    // says the two must stay in sync, and on warnings they were not.
    for d in resolve_result.errors.iter().chain(&resolve_result.warnings) {
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
            // A resolver diagnostic's `fix` is its help text — the "did you
            // mean" suggestion and (since the foreign-keyword table) the
            // `const`/`var` hints. This dropped it, so every library consumer of
            // `check_pipeline` saw resolver diagnostics with no help while the
            // CLI's `run_check_pipeline_located` carried it. The two are
            // documented as needing to stay in sync and had drifted, which is
            // the same class of divergence this whole spec exists to close.
            help: d.fix.clone(),
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

    let fn_sigs: std::collections::HashMap<String, checker::FnSig> = infer_ctx
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
            // O-RLM-12, second drift. A checker `Diagnostic` carries
            // `expected`/`found`/`fix`; all three were dropped here while the
            // CLI's `run_check_pipeline_located` carried them, so E0307 reached
            // a library consumer with no help and no typed fields and reached a
            // CLI user with both. Found by making the pipeline-agreement test
            // bidirectional — the one-way version missed it, because it only
            // asked whether the CLI had what the library had.
            expected: e.expected.clone(),
            found: e.found.clone(),
            help: e.fix.clone(),
        });
    }

    // Borrow checking
    for item in &program.items {
        if let ast::Item::FnDef(fndef) = item {
            let param_types: std::collections::HashMap<String, types::Type> =
                if let Some(sig) = infer_ctx.fn_sigs.get(&fndef.name) {
                    fndef
                        .params
                        .iter()
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

    collapse_refined_type_errors(&mut out);
    collapse_unresolved_duplicates(&mut out);
    out
}

/// Drop an E0102 that a hint-bearing checker diagnostic already accounts for at
/// the same span.
///
/// E0102 is INFER's code for "unification failed here". The checker's E03xx
/// block is the refined restatement of the same failure — E0301 (an unhandled
/// `Option`), E0303 (`?` outside a `Result` fn), E0306 (a wrong argument),
/// E0307 (a return mismatch). Each of those carries the repair `help`; the
/// E0102 beside it carries none. One fact, reported twice, with the useless
/// copy FIRST.
///
/// Order is what makes this more than cosmetic. A consumer that reads one error
/// per failure — the RLM harness does, deliberately, so that an advisory
/// warning cannot be misreported as the cause — always got the bare one.
/// Measured across eight benchmark sweeps: 37 of 48 errors (77%) reached the
/// model as a hint-free E0102 with its hint-bearing twin discarded a line
/// later. The model had nothing to repair toward.
///
/// Keyed on the SPAN alone, not the type pair. The two describe one failure but
/// not always in the same words: E0303 reports `expected Result<T, E>` where
/// the E0102 for the identical span says `Result<?3, str>`, an internal
/// inference variable that means nothing to a reader. Requiring the pair to
/// match would skip exactly the cases whose wording is worst.
///
/// Only a HINT-BEARING partner suppresses, so an E0102 that is the sole account
/// of a failure always survives rather than vanishing.
///
/// E0001 (`cannot find name X`) suppresses too, and unlike the E03xx partners it
/// does so with or without `help`. An unresolved name is a COMPLETE account of
/// the failure — the expression has no type because the name has no binding — so
/// the E0102 beside it is not a second finding but the first one restated in
/// terms of an inference variable the reader cannot act on. Measured, the
/// cascade is narrow: of four shapes that fail name resolution (call site, let
/// binding, two independent unresolved names, tail expression) only the tail
/// expression emits the pair, and there the two spans are identical. `mut x` in
/// an RLM cell hits exactly that shape.
///
/// This lives here, not in the CLI, because `check_pipeline` and the CLI must
/// agree diagnostic-for-diagnostic — `parse_help_probe` asserts it in both
/// directions, and it is what caught this when the collapse was CLI-only.
pub fn collapse_refined_type_errors(diags: &mut Vec<PipelineDiagnostic>) {
    let refined: std::collections::HashSet<(String, u32, u32)> = diags
        .iter()
        // `line == 0` is the serializer's sentinel for "no location", not line
        // zero. Several checker sites emit `.at(&file, 0, 0)` with no span, so an
        // unlocated E0306 and an unlocated E0102 describing SEPARATE failures
        // collide at the same key and the E0102 disappears — the one failure
        // mode this filter must not have. An unknown location cannot prove it
        // accounts for anything, so it never suppresses.
        .filter(|d| {
            d.severity == "error"
                && d.line > 0
                && (d.code == "E0001" || (d.code.starts_with("E03") && d.help.is_some()))
        })
        .map(|d| (d.file.clone(), d.line, d.col))
        .collect();
    if refined.is_empty() {
        return;
    }
    diags.retain(|d| d.code != "E0102" || !refined.contains(&(d.file.clone(), d.line, d.col)));
}

/// Drop the LESS-RESOLVED of two identical "cannot be used directly" errors at
/// one span.
///
/// `if found != None` reports twice at the same line and column — once for the
/// `Option<i64>` on the left, once for the `None` on the right, whose inner type
/// never resolves:
///
///   E0301 value of type `Option<i64>` cannot be used directly
///   E0301 value of type `Option<<unknown>>` cannot be used directly
///
/// One mistake, one fix, two errors, and the second names a type the reader
/// cannot act on — `<unknown>` is the printer saying it does not know, and
/// `Option<<unknown>>` reads like a malformed type rather than a missing one.
/// Comparing an `Option` against `None` is an ordinary habit (there is no
/// `is_some` builtin here — matching really is the answer), so this is a
/// message a reader meets while doing something reasonable.
///
/// Keyed on (file, line, col, code) so it only ever collapses genuine
/// duplicates of the SAME diagnostic at the SAME place, and keeps the one whose
/// type is fully known.
pub fn collapse_unresolved_duplicates(diags: &mut Vec<PipelineDiagnostic>) {
    let has_resolved: std::collections::HashSet<(String, u32, u32, String)> = diags
        .iter()
        .filter(|d| d.line > 0 && !d.message.contains("<unknown>"))
        .map(|d| (d.file.clone(), d.line, d.col, d.code.clone()))
        .collect();
    if has_resolved.is_empty() {
        return;
    }
    diags.retain(|d| {
        !d.message.contains("<unknown>")
            || d.line == 0
            || !has_resolved.contains(&(d.file.clone(), d.line, d.col, d.code.clone()))
    });
}

#[cfg(test)]
mod collapse_tests {
    use super::{collapse_refined_type_errors, PipelineDiagnostic};

    fn diag(code: &str, line: u32, col: u32, help: Option<&str>) -> PipelineDiagnostic {
        PipelineDiagnostic {
            code: code.to_string(),
            message: format!("{code} message"),
            file: "t.ax".to_string(),
            line,
            col,
            severity: "error".to_string(),
            caret: String::new(),
            expected: None,
            found: None,
            help: help.map(|h| h.to_string()),
        }
    }

    /// The guard that makes the collapse safe, tested where it lives.
    ///
    /// `line == 0` is the serializer's "no location" sentinel, not line zero.
    /// Two UNLOCATED diagnostics describing separate failures therefore share a
    /// key, and suppressing on it would drop a real error nothing else accounts
    /// for — the one failure mode a diagnostic filter must not have.
    ///
    /// This lived only in a CLI fixture that happened to produce two unlocated
    /// errors. When those two sites gained spans the fixture stopped exercising
    /// the guard, and would have gone on passing while testing nothing.
    #[test]
    fn an_unlocated_partner_never_suppresses() {
        let mut ds = vec![
            diag("E0306", 0, 0, Some("a hint")),
            diag("E0102", 0, 0, None),
        ];
        collapse_refined_type_errors(&mut ds);
        assert_eq!(
            ds.len(),
            2,
            "no location cannot prove it accounts for anything"
        );
    }

    #[test]
    fn a_located_hint_bearing_partner_suppresses_at_the_same_span() {
        let mut ds = vec![
            diag("E0306", 7, 3, Some("a hint")),
            diag("E0102", 7, 3, None),
        ];
        collapse_refined_type_errors(&mut ds);
        assert_eq!(ds.len(), 1, "one failure, one diagnostic");
        assert_eq!(ds[0].code, "E0306", "the survivor must carry the hint");
    }

    #[test]
    fn a_different_span_does_not_suppress() {
        let mut ds = vec![
            diag("E0306", 4, 3, Some("a hint")),
            diag("E0102", 5, 3, None),
        ];
        collapse_refined_type_errors(&mut ds);
        assert_eq!(ds.len(), 2, "different lines are different failures");
    }

    /// Only a HINT-BEARING partner suppresses, so an E0102 that is the sole
    /// account of a failure always survives.
    #[test]
    fn a_hintless_partner_never_suppresses() {
        let mut ds = vec![diag("E0306", 7, 3, None), diag("E0102", 7, 3, None)];
        collapse_refined_type_errors(&mut ds);
        assert_eq!(ds.len(), 2, "a partner with no hint accounts for nothing");
    }
}

#[cfg(test)]
mod display_tests {
    use super::PipelineDiagnostic;

    fn diag(help: Option<&str>) -> PipelineDiagnostic {
        PipelineDiagnostic {
            code: "E0001".to_string(),
            message: "cannot find name `nope` in this scope".to_string(),
            file: "t.ax".to_string(),
            line: 3,
            col: 3,
            severity: "error".to_string(),
            caret: String::new(),
            expected: None,
            found: None,
            help: help.map(str::to_string),
        }
    }

    /// The terminal renderer dropped `help` entirely.
    ///
    /// Every hint in the compiler reached `axon-diag/1` and no human. Tested
    /// here rather than through the CLI because stderr is not a tty under
    /// `cargo test`, so the CLI emits JSON — an end-to-end assertion on `axon
    /// check` output passes against the unfixed renderer.
    #[test]
    fn display_renders_the_repair_hint() {
        let s = diag(Some("introduce `nope` with `let nope = …`")).display();
        assert!(s.contains("E0001"), "{s}");
        assert!(
            s.contains("help:") && s.contains("introduce `nope`"),
            "the rendered diagnostic must carry its repair: {s}"
        );
        // On its own line, so the message stays greppable as one line.
        assert_eq!(s.lines().count(), 2, "{s}");
    }

    #[test]
    fn display_adds_nothing_when_there_is_no_hint() {
        let s = diag(None).display();
        assert!(!s.contains("help:"), "{s}");
        assert_eq!(s.lines().count(), 1, "{s}");
    }
}

/// Byte offsets of a `{}` bound as a value — `let d = {}`.
///
/// Axon has no dict literal, and `{}` is a perfectly legal EMPTY BLOCK, so this
/// parses, type-checks and yields `()`. Nothing complains at the binding; the
/// reader finds out downstream, from messages about a type they never wrote:
/// "cannot index a value of type ()", "argument 0 of `println` ... found ()".
///
/// The sibling rule for `{"a": 1}` catches the form WITH pairs, because that one
/// is a parse error (an unexpected `:`). The empty form has no colon and no
/// error, so it needed its own check — the silent half of the same habit.
///
/// Zero occurrences of this shape across the repo's 327 `.ax` files, so the
/// only programs it can fire on are the ones making the mistake.
pub fn empty_block_binding_offsets(source: &str) -> Vec<usize> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    let mut in_str = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if in_str => i += 2,
            b'"' => {
                in_str = !in_str;
                i += 1;
            }
            b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                i = source[i..].find('\n').map_or(source.len(), |n| i + n);
            }
            b'{' if !in_str => {
                // `= {}` (any spacing), and nothing but whitespace inside.
                let before = source[..i].trim_end();
                let close = source[i + 1..]
                    .find('}')
                    .map(|n| i + 1 + n)
                    .unwrap_or(source.len());
                let inner_blank =
                    close <= source.len() && source[i + 1..close].chars().all(char::is_whitespace);
                if before.ends_with('=') && !before.ends_with("==") && inner_blank {
                    out.push(i);
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// Byte offsets of a `//` that is almost certainly Python floor division
/// rather than a comment: it trails an expression on the line, and everything
/// after it is a bare numeric literal.
///
/// The "bare numeric literal" test is what keeps this quiet. A real trailing
/// comment is prose; `// 2` is not prose, and a comment whose entire body is a
/// number is vanishingly rare next to the habit it catches. Anything richer —
/// `// 2 items`, `// see 3` — is left alone, so the check under-reports rather
/// than crying wolf on ordinary code.
///
/// String literals are skipped, so a `"//"` inside text never matches, and a
/// line's FIRST `//` wins (the rest of the line is already comment text).
pub fn floor_division_comment_offsets(source: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0usize;
    let mut line_start = 0usize;
    let mut in_str = false;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' if !in_str => {
                line_start = i + 1;
                i += 1;
            }
            b'\\' if in_str => i += 2, // escape: never ends the string
            b'"' => {
                in_str = !in_str;
                i += 1;
            }
            b'/' if !in_str && i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                let before = source[line_start..i].trim_end();
                // A standalone comment line has nothing before it; a trailing
                // comment after an expression does. Only the latter can be a
                // misread operator.
                // Measured against the repo's own 327 `.ax` files, "trails an
                // expression + numeric body" fired 13 times and was WRONG all
                // 13: `println(to_str(gcd(48, 18)))    // 6` is the house style
                // for annotating expected output. Two discriminators separate
                // that from the habit, and both are needed:
                //
                //   1. a closing `)` or `]` before the `//`. Every corpus hit
                //      ended with one; `a // b` does not. This costs the
                //      `f() // 2` shape, which is the rarer half of the habit —
                //      the right trade against crying wolf on real code.
                //   2. exactly one space each side. An expected-output comment
                //      is aligned away from the code; a misread operator sits
                //      where an operator would sit.
                let last = before.chars().last();
                let trails_an_expression =
                    last.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
                let rest_end = source[i..].find('\n').map_or(source.len(), |n| i + n);
                let body = source[i + 2..rest_end].trim();
                // An OPERAND, not prose: one whitespace-free token. `2`,
                // `len(scores)` and `n` qualify; `count of items` does not.
                //
                // The first version required a bare NUMBER, tuned on `7 // 2`.
                // That missed the shape real code actually takes —
                // `total // len(scores)` — which is exactly what an end-to-end
                // run produced, checking clean and computing `total`. Widening
                // to "no internal whitespace" keeps prose comments out while
                // catching the operand case; verified against the corpus, not
                // assumed.
                let is_operand = !body.is_empty()
                    && !body.chars().any(char::is_whitespace)
                    && body
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._()[]&".contains(c));
                let spaced_like_an_operator = source[line_start..i].ends_with(' ')
                    && !source[line_start..i].ends_with("  ")
                    && source[i + 2..rest_end].starts_with(' ')
                    && !source[i + 2..rest_end].starts_with("  ");
                if trails_an_expression && is_operand && spaced_like_an_operator {
                    out.push(i);
                }
                // Skip to end of line: the remainder is comment text.
                i = rest_end;
            }
            _ => i += 1,
        }
    }
    out
}

#[cfg(test)]
mod floor_division_scan_tests {
    use super::floor_division_comment_offsets as scan;

    /// The habit: `//` is Python floor division and an Axon comment, so
    /// `let x = 7 // 2` compiles clean and evaluates to 7 where Python gives 3.
    #[test]
    fn the_floor_division_shape_is_caught() {
        assert_eq!(scan("let x = 7 // 2\n").len(), 1);
        assert_eq!(scan("let x = a // 2\n").len(), 1);
        // The shape real code takes. The first rule required a bare NUMBER and
        // missed this — an end-to-end run of a model-shaped file produced
        // `let avg = total // len(scores)`, which checked CLEAN and computed
        // `total`. An operand is any whitespace-free token, borrow included.
        assert_eq!(scan("let avg = total // len(scores)\n").len(), 1);
        assert_eq!(scan("let avg = total // len(&xs)\n").len(), 1);
        assert_eq!(scan("let x = a // b\n").len(), 1);
    }

    /// The discriminators, each measured against the repo's own corpus rather
    /// than guessed. "Trails an expression + numeric body" alone fired on 13 of
    /// 327 real files and was wrong all 13 times — `// 6` after a `println` is
    /// this project's house style for annotating expected output.
    #[test]
    fn real_expected_output_comments_stay_quiet() {
        // Closing paren before the `//` — every corpus false positive.
        assert_eq!(scan("println(to_str(gcd(48, 18)))    // 6\n").len(), 0);
        assert_eq!(scan("println(to_str(p.0 + p.1))   // 7\n").len(), 0);
        // Aligned away from the code, which is what an annotation looks like.
        assert_eq!(scan("let answer = 2 + 2 * 20    // 42\n").len(), 0);
        // Prose, a standalone line, and a body that is not purely numeric.
        assert_eq!(scan("let x = 7 // count of items\n").len(), 0);
        assert_eq!(scan("// 2\n").len(), 0);
        assert_eq!(scan("let x = 7 // 2 items\n").len(), 0);
        // Prose is what the operand test excludes: more than one token.
        assert_eq!(scan("let avg = total // number of scores\n").len(), 0);
    }

    /// A `//` inside a string is text, not a comment, and must never be read
    /// as either one.
    #[test]
    fn string_literals_are_not_scanned() {
        assert_eq!(scan("println(\"a // 2\")\n").len(), 0);
        assert_eq!(scan("let u = \"http://h/2\"\n").len(), 0);
        assert_eq!(scan("let s = \"esc \\\" // 2\"\n").len(), 0);
    }
}
