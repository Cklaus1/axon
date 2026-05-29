//! Axon language toolchain CLI.
//!
//! Subcommands:
//!   parse  — print AST as JSON
//!   check  — type-check and report errors
//!   build  — compile to a native binary
//!   run    — build + execute
//!   test   — run @[test]-tagged functions

#![recursion_limit = "2048"]

use std::io::IsTerminal as _;
use std::path::PathBuf;
use std::process;
use std::time::Instant;

use clap::{Parser, Subcommand};
use axon_core::parse_source;
#[cfg(feature = "codegen")]
use inkwell;

// ── CLI definition ────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "axon",
    about = "The Axon language toolchain",
    version,
    propagate_version = true,
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Parse a .ax file and print the AST as JSON.
    Parse {
        #[arg(help = "Path to .ax source file")]
        file: PathBuf,
    },

    /// Type-check a .ax file and report errors.
    ///
    /// Exit codes: 0 = no errors, 2 = type errors.
    Check {
        #[arg(help = "Path to .ax source file")]
        file: PathBuf,

        /// Emit errors as newline-delimited JSON to stderr (auto-detected when
        /// stderr is not a terminal).
        #[arg(long, help = "Emit errors as JSON")]
        json: bool,
    },

    /// Compile one or more .ax files to a native binary.
    Build {
        /// Path(s) to .ax source files. All files share a single global namespace.
        #[arg(help = "Path(s) to .ax source file(s)", num_args = 1..)]
        files: Vec<PathBuf>,

        /// Output binary path. Defaults to the stem of the first file
        /// (e.g. `main.ax` → `./main`).
        #[arg(long, short, help = "Output binary path")]
        out: Option<PathBuf>,

        /// Enable O2 optimizations (default: O0 / debug).
        #[arg(long, help = "Optimized release build")]
        release: bool,

        /// Cross-compile for the given LLVM target triple
        /// (e.g. `x86_64-unknown-linux-gnu`, `aarch64-apple-darwin`).
        #[arg(long, help = "Target triple for cross-compilation")]
        target: Option<String>,

        /// Disable incremental compilation cache for this invocation.
        #[arg(long, help = "Bypass cache lookup and write")]
        no_cache: bool,

        /// Override the cache directory (default: ~/.cache/axon/).
        #[arg(long, help = "Cache directory path")]
        cache_dir: Option<PathBuf>,
    },

    /// Start the Axon language server (JSON-RPC 2.0 on stdin/stdout).
    ///
    /// Connects to a Language Server Protocol 3.17 client such as VS Code or
    /// Neovim. The server runs until the client closes the connection (stdin EOF).
    Lsp,

    /// Manage the incremental compilation cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },

    /// Compile a structured-prose goal file (`*.md`) to AST and run it.
    ///
    /// The Phase-10 two-track flow in one step: `goal.md` → (axon-surface) →
    /// `.ax` → type-check → interpret.
    Goal {
        #[arg(help = "Path to the goal .md file")]
        file: PathBuf,

        /// Print the generated `.ax` source instead of running it.
        #[arg(long, help = "Emit the compiled .ax to stdout and exit")]
        emit: bool,
    },

    /// Compile a .ax file and execute it, forwarding remaining arguments.
    Run {
        #[arg(help = "Path to .ax source file")]
        file: PathBuf,

        /// Enable O2 optimizations (default: O0 / debug).
        #[arg(long, short = 'r', help = "Optimized release build")]
        release: bool,

        /// Arguments forwarded to the compiled binary.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },

    /// Format one or more .ax files to canonical Axon style.
    Fmt {
        /// Path(s) to .ax source file(s).
        #[arg(help = "Path(s) to .ax source file(s)", num_args = 1..)]
        files: Vec<PathBuf>,

        /// Check only — exit 1 if any file would be reformatted (file unchanged).
        #[arg(long, help = "Check formatting without modifying files")]
        check: bool,
    },

    /// Generate Markdown documentation from `///` doc comments.
    ///
    /// Reads one or more .ax files, extracts `///` doc comments attached to
    /// top-level functions, types, and enums, and writes a Markdown file.
    /// Exit codes: 0 = success, 2 = parse error.
    Doc {
        /// Path(s) to .ax source file(s).
        #[arg(help = "Path(s) to .ax source file(s)", num_args = 1..)]
        files: Vec<PathBuf>,

        /// Output path for the generated Markdown (default: stdout).
        #[arg(long, short, help = "Output Markdown file path")]
        out: Option<PathBuf>,
    },

    /// Run all @[test]-tagged functions in one or more .ax files.
    Test {
        /// Path(s) to .ax source files.
        #[arg(help = "Path(s) to .ax source file(s)", num_args = 1..)]
        files: Vec<PathBuf>,

        /// Only run tests whose names contain this string.
        #[arg(long, help = "Filter tests by name substring")]
        filter: Option<String>,

        /// Number of parallel workers (0 = auto-detect CPU count, default 1).
        #[arg(long, default_value = "1", help = "Parallel worker count (0 = num_cpus)")]
        jobs: usize,

        /// Emit results as newline-delimited JSON (NDJSON).
        #[arg(long, help = "Machine-readable NDJSON output")]
        json: bool,
    },
}

// ── Cache subcommand actions ──────────────────────────────────────────────────

#[derive(Subcommand)]
enum CacheAction {
    /// Remove cache entries.
    ///
    /// Without `--older-than`, removes all entries.
    Clean {
        /// Only remove entries not modified in the last N days.
        #[arg(long, help = "Remove entries older than N days", value_name = "DAYS")]
        older_than: Option<u64>,

        /// Override the cache directory (default: ~/.cache/axon/).
        #[arg(long, help = "Cache directory path")]
        cache_dir: Option<PathBuf>,
    },
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Command::Parse { file } => cmd_parse(file),
        Command::Check { file, json } => cmd_check(file, json),
        Command::Build { files, out, release, target, no_cache, cache_dir } => {
            cmd_build(files, out, release, target, no_cache, cache_dir)
        }
        Command::Goal { file, emit } => cmd_goal(file, emit),
        Command::Run { file, release, args } => cmd_run(file, release, args),
        Command::Fmt { files, check } => cmd_fmt(files, check),
        Command::Doc { files, out } => cmd_doc(files, out),
        Command::Lsp => cmd_lsp(),
        Command::Cache { action } => cmd_cache(action),
        Command::Test { files, filter, jobs, json } => cmd_test(files, filter, jobs, json),
    }
}

// ── parse ─────────────────────────────────────────────────────────────────────

/// `axon parse` emits the AST as JSON, which needs serde — only available when
/// built with the `serde-json` feature.
#[cfg(not(feature = "serde-json"))]
fn cmd_parse(_file: PathBuf) {
    eprintln!("error: `axon parse` (JSON AST output) requires building axon with the `serde-json` feature.");
    process::exit(1);
}

#[cfg(feature = "serde-json")]
fn cmd_parse(file: PathBuf) {
    // Fix 5: validate .ax extension.
    validate_ax_extension(&file);

    let src = read_source(&file);
    match parse_source(&src) {
        // Fix 4: output JSON via the lib function (not serde_json directly in
        // the binary, which would overflow the trait-solver with inkwell).
        Ok(program) => match axon_core::program_to_json(&program) {
            Ok(json) => println!("{json}"),
            Err(e) => {
                eprintln!("error serialising AST to JSON: {e}");
                process::exit(1);
            }
        },
        Err(e) => {
            eprintln!("error: {e}");
            // Exit 2 = compile error (parse error).
            process::exit(2);
        }
    }
}

// ── check ─────────────────────────────────────────────────────────────────────

fn cmd_check(file: PathBuf, json_flag: bool) {
    // Fix 5: validate .ax extension.
    validate_ax_extension(&file);

    let src = read_source(&file);

    // Parse first.
    let mut program = match parse_source(&src) {
        Ok(p) => p,
        Err(e) => {
            // Fix 8: exit 2 for compile errors.
            emit_error(&format!("{e}"), json_flag || !std::io::stderr().is_terminal());
            process::exit(2);
        }
    };

    // Pipe detection: if stderr is not a terminal, switch to JSON automatically.
    let use_json = json_flag || !std::io::stderr().is_terminal();

    // Type-check pipeline.
    let (errors, _infer_ctx) = run_check_pipeline(&mut program, &file);

    if errors.is_empty() {
        // Print nothing on success (Unix convention).
        process::exit(0);
    }

    for err in &errors {
        emit_error(err, use_json);
    }
    // Fix 8: exit 2 for compile errors.
    process::exit(2);
}

// ── build ─────────────────────────────────────────────────────────────────────

/// Native AOT build via the LLVM/inkwell backend. Only available when axon is
/// built with the `codegen` feature. (`axon run`/`check`/`test` work without it
/// via the interpreter.)
#[cfg(not(feature = "codegen"))]
fn cmd_build(
    _files: Vec<PathBuf>,
    _out: Option<PathBuf>,
    _release: bool,
    _target: Option<String>,
    _no_cache: bool,
    _cache_dir: Option<PathBuf>,
) {
    eprintln!("error: `axon build` (native codegen) requires building axon with the `codegen` feature.");
    eprintln!("note: the native codegen build is currently very slow (see BUILD_DIAGNOSIS.md).");
    eprintln!("hint: use `axon run <file.ax>` — it executes via the interpreter, no codegen needed.");
    process::exit(1);
}

#[cfg(feature = "codegen")]
fn cmd_build(
    files: Vec<PathBuf>,
    out: Option<PathBuf>,
    release: bool,
    target: Option<String>,
    no_cache: bool,
    cache_dir: Option<PathBuf>,
) {
    if files.is_empty() {
        eprintln!("error: no source files specified");
        process::exit(1);
    }
    for f in &files {
        validate_ax_extension(f);
    }

    let first = &files[0];
    let output = out.unwrap_or_else(|| {
        let stem = first.file_stem().unwrap_or_default().to_string_lossy();
        PathBuf::from(format!("./{stem}"))
    });

    if files.len() == 1 {
        eprintln!("Compiling {}...", first.display());
    } else {
        eprintln!("Compiling {} files...", files.len());
    }
    let start = Instant::now();

    // Parse all files (in parallel when multiple).
    let file_programs = match axon_core::parse_source_files(&files) {
        Ok(ps) => ps,
        Err(errs) => {
            for e in &errs {
                eprintln!("error: {e}");
            }
            process::exit(2);
        }
    };

    // Merge into a single program, detect duplicate top-level names.
    let (mut program, merge_errors) = axon_core::merge_programs(file_programs);
    if !merge_errors.is_empty() {
        for e in &merge_errors {
            eprintln!("error[{}]: {}", e.code, e.message);
        }
        process::exit(2);
    }

    let opts = BuildOptions {
        release,
        target_triple: target,
        no_cache,
        cache_dir,
    };

    // Warn if cross-compiling without cross.toml configuration.
    if let Some(ref triple) = opts.target_triple {
        let host = inkwell::targets::TargetMachine::get_default_triple()
            .as_str()
            .to_string_lossy()
            .to_string();
        if !host.starts_with(&triple[..triple.find('-').unwrap_or(triple.len())]) {
            // Cross-compiling: check for cross.toml
            let home = std::env::var_os("HOME").unwrap_or_default();
            let cross_toml = std::path::PathBuf::from(home)
                .join(".config").join("axon").join("cross.toml");
            if !cross_toml.exists() {
                eprintln!(
                    "warning[E0905]: cross-compiling to '{}' but ~/.config/axon/cross.toml \
                     is absent — using host linker (may fail)\n  \
                     hint: create [target.{}] in ~/.config/axon/cross.toml with a 'linker' key",
                    triple, triple
                );
            }
        }
    }

    match run_build_pipeline(&mut program, first, &output, &opts) {
        Ok(()) => {
            let elapsed = start.elapsed().as_millis();
            eprintln!("Binary: {} ({elapsed}ms)", output.display());
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    }
}

#[cfg(feature = "codegen")]
struct BuildOptions {
    release: bool,
    target_triple: Option<String>,
    no_cache: bool,
    cache_dir: Option<PathBuf>,
}

// ── goal ──────────────────────────────────────────────────────────────────────

/// `axon goal <file.md>` — the Phase-10 two-track flow in one command:
/// compile structured prose → `.ax` (via `axon-surface`) → type-check → run.
fn cmd_goal(file: PathBuf, emit_only: bool) {
    let md = read_source(&file);

    // Prose → typed-AST `.ax` source via the surface compiler.
    let goal = match axon_surface::parser::GoalFile::parse(&md) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("error: goal file invalid: {e}");
            process::exit(2);
        }
    };
    let ax_src = match axon_surface::compile::emit(&goal) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: goal compilation failed: {e}");
            process::exit(2);
        }
    };

    if emit_only {
        print!("{ax_src}");
        return;
    }

    // Parse the generated `.ax`.
    let mut program = match parse_source(&ax_src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: generated .ax failed to parse: {e}");
            eprintln!("hint: re-run with --emit to inspect the generated source");
            process::exit(1);
        }
    };

    // Type-check before running.
    let (errors, _infer_ctx) = run_check_pipeline(&mut program, &file);
    if !errors.is_empty() {
        let use_json = !std::io::stderr().is_terminal();
        for err in &errors {
            emit_error(err, use_json);
        }
        process::exit(2);
    }

    // Interpret.
    let code = axon_core::interp::run_program(&program);
    process::exit(code);
}

// ── run ───────────────────────────────────────────────────────────────────────

fn cmd_run(file: PathBuf, _release: bool, args: Vec<String>) {
    // Fix 5: validate .ax extension.
    validate_ax_extension(&file);

    let src = read_source(&file);
    let mut program = match parse_source(&src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            // Fix 8: exit 2 for compile errors.
            process::exit(2);
        }
    };

    // Type-check before running, so type errors are reported up front rather
    // than surfacing as interpreter runtime panics.
    let (errors, _infer_ctx) = run_check_pipeline(&mut program, &file);
    if !errors.is_empty() {
        let use_json = !std::io::stderr().is_terminal();
        for err in &errors {
            emit_error(err, use_json);
        }
        process::exit(2);
    }

    if !args.is_empty() {
        eprintln!("warning: `axon run` does not yet forward arguments to the program");
    }

    // Execute via the tree-walking interpreter (no LLVM codegen needed).
    let code = axon_core::interp::run_program(&program);
    process::exit(code);
}

// ── fmt ───────────────────────────────────────────────────────────────────────

/// Exit codes for `axon fmt`:
///   0 — success (file formatted in-place, or --check and already correct)
///   1 — --check: at least one file would be reformatted
///   2 — parse error in input file (file not touched)
fn cmd_fmt(files: Vec<PathBuf>, check: bool) {
    if files.is_empty() {
        eprintln!("error: no source files specified");
        process::exit(1);
    }
    for f in &files {
        validate_ax_extension(f);
    }

    let mut any_would_change = false;

    for file in &files {
        let src = read_source(file);
        let program = match parse_source(&src) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: {}: {e}", file.display());
                process::exit(2);
            }
        };

        let formatted = axon_core::format_program(&program);

        if check {
            if formatted != src {
                eprintln!("{}: would reformat", file.display());
                any_would_change = true;
            }
        } else if formatted != src {
            std::fs::write(file, &formatted).unwrap_or_else(|e| {
                eprintln!("error writing {}: {e}", file.display());
                process::exit(1);
            });
            eprintln!("formatted: {}", file.display());
        }
    }

    if check && any_would_change {
        process::exit(1);
    }
}

// ── lsp ───────────────────────────────────────────────────────────────────────

/// The language server depends on serde (JSON-RPC) — only available with the
/// `serde-json` feature.
#[cfg(not(feature = "serde-json"))]
fn cmd_lsp() {
    eprintln!("error: `axon lsp` requires building axon with the `serde-json` feature.");
    process::exit(1);
}

#[cfg(feature = "serde-json")]
fn cmd_lsp() {
    axon_core::lsp::run_lsp();
}

// ── cache ─────────────────────────────────────────────────────────────────────

fn cmd_cache(action: CacheAction) {
    match action {
        CacheAction::Clean { older_than, cache_dir } => {
            let dir = cache_dir.unwrap_or_else(axon_core::default_cache_dir);
            let older_than_secs = older_than.map(|days| days * 86400);
            let (removed, errors) = axon_core::clean_cache(&dir, older_than_secs);
            eprintln!("removed {removed} cache entr{}", if removed == 1 { "y" } else { "ies" });
            if errors > 0 {
                eprintln!("warning: {errors} entr{} could not be removed", if errors == 1 { "y" } else { "ies" });
                process::exit(1);
            }
        }
    }
}

// ── doc ───────────────────────────────────────────────────────────────────────

/// Exit codes for `axon doc`:
///   0 — success
///   2 — parse error in one of the input files
fn cmd_doc(files: Vec<PathBuf>, out: Option<PathBuf>) {
    if files.is_empty() {
        eprintln!("error: no source files specified");
        process::exit(1);
    }
    for f in &files {
        validate_ax_extension(f);
    }

    // When documenting multiple files, merge into one program first so the
    // output reflects the combined public API.
    let file_programs = match axon_core::parse_source_files(&files) {
        Ok(ps) => ps,
        Err(errs) => {
            for e in &errs { eprintln!("error: {e}"); }
            process::exit(2);
        }
    };

    // For single-file docs, use the filename as the H1 title.
    // For multi-file docs, use the first filename.
    let title = files[0]
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();

    if file_programs.len() == 1 {
        let (filename, program) = &file_programs[0];
        // Re-read the source for `///` comment extraction.
        let src = read_source(&files[0]);
        let markdown = axon_core::generate_docs(program, &src, filename);
        emit_doc_output(markdown, out.as_deref());
    } else {
        // Merge and document the combined namespace.
        let (merged_program, merge_errors) = axon_core::merge_programs(file_programs);
        if !merge_errors.is_empty() {
            for e in &merge_errors {
                eprintln!("error[{}]: {}", e.code, e.message);
            }
            process::exit(2);
        }
        // For multi-file, source text is the concatenation (/// comments are
        // still in the per-file sources, but we use byte-offsets from spans).
        // Pass an empty source — the doc extractor gracefully returns no docs
        // for items whose spans exceed the source length.
        let markdown = axon_core::generate_docs(&merged_program, "", &title);
        emit_doc_output(markdown, out.as_deref());
    }
}

fn emit_doc_output(markdown: String, out: Option<&std::path::Path>) {
    match out {
        Some(path) => {
            std::fs::write(path, &markdown).unwrap_or_else(|e| {
                eprintln!("error writing {}: {e}", path.display());
                process::exit(1);
            });
            eprintln!("docs written to {}", path.display());
        }
        None => print!("{markdown}"),
    }
}

// ── test ──────────────────────────────────────────────────────────────────────

/// Outcome of running one `@[test]` function via the interpreter.
struct TestOutcome {
    name: String,
    passed: bool,
    duration_ms: u64,
    error: Option<String>,
}

fn cmd_test(files: Vec<PathBuf>, filter: Option<String>, jobs: usize, json: bool) {
    if files.is_empty() {
        eprintln!("error: no source files specified");
        process::exit(1);
    }
    for f in &files {
        validate_ax_extension(f);
    }

    // Parse and merge all source files.
    let file_programs = match axon_core::parse_source_files(&files) {
        Ok(ps) => ps,
        Err(errs) => {
            for e in &errs { eprintln!("error: {e}"); }
            process::exit(2);
        }
    };
    let (mut program, merge_errors) = axon_core::merge_programs(file_programs);
    if !merge_errors.is_empty() {
        for e in &merge_errors { eprintln!("error[{}]: {}", e.code, e.message); }
        process::exit(2);
    }

    // Abort on type errors before running any tests.
    let primary_file = &files[0];
    let (type_errors, _infer_ctx) = run_check_pipeline(&mut program, primary_file);
    if !type_errors.is_empty() {
        for err in &type_errors { eprintln!("error: {err}"); }
        eprintln!("error: {} type error(s); tests aborted", type_errors.len());
        process::exit(2);
    }

    // Collect test function metadata: (name, should_fail).
    let test_meta: Vec<(String, bool)> = program
        .items
        .iter()
        .filter_map(|item| {
            if let axon_core::ast::Item::FnDef(f) = item {
                let test_attr = f.attrs.iter().find(|a| a.name == "test");
                if let Some(attr) = test_attr {
                    if !f.params.is_empty() {
                        eprintln!(
                            "error: test function '{}' must take zero parameters",
                            f.name
                        );
                        return None;
                    }
                    let should_fail = attr.args.iter().any(|a| a == "should_fail");
                    if let Some(ref pat) = filter {
                        if f.name.contains(pat.as_str()) {
                            return Some((f.name.clone(), should_fail));
                        }
                        return None;
                    }
                    return Some((f.name.clone(), should_fail));
                }
            }
            None
        })
        .collect();

    let n = test_meta.len();
    let _ = jobs; // tests run in-process via the interpreter; --jobs is currently a no-op

    if !json {
        println!("running {n} test{}", if n == 1 { "" } else { "s" });
    }

    // Run each @[test] in-process via the interpreter (no per-test compile).
    // A test passes if it completes without panicking; a should_fail test
    // passes iff it panics.
    let all_results: Vec<TestOutcome> = test_meta
        .iter()
        .map(|(name, should_fail)| {
            let start = Instant::now();
            let outcome = axon_core::interp::run_test_fn(&program, name);
            let duration_ms = start.elapsed().as_millis() as u64;
            let (passed, error) = match outcome {
                Ok(()) if *should_fail => (
                    false,
                    Some(format!("should_fail test '{name}' completed without panicking")),
                ),
                Ok(()) => (true, None),
                Err(_) if *should_fail => (true, None),
                Err(e) => (false, Some(e)),
            };
            TestOutcome { name: name.clone(), passed, duration_ms, error }
        })
        .collect();

    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut total_ms: u64 = 0;

    for r in &all_results {
        let should_fail = test_meta
            .iter()
            .find(|(name, _)| name == &r.name)
            .map(|(_, sf)| *sf)
            .unwrap_or(false);

        total_ms += r.duration_ms;
        if r.passed { passed += 1; } else { failed += 1; }

        if json {
            if r.passed {
                println!(
                    "{{\"name\":{:?},\"status\":\"ok\",\"duration_ms\":{}}}",
                    r.name, r.duration_ms
                );
            } else {
                let msg = r.error.as_deref().unwrap_or("non-zero exit");
                let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");
                println!(
                    "{{\"name\":{:?},\"status\":\"failed\",\"duration_ms\":{},\"message\":\"{}\"}}",
                    r.name, r.duration_ms, escaped
                );
            }
        } else if r.passed {
            println!("test {} ... ok ({:.1}ms)", r.name, r.duration_ms as f64);
        } else {
            let err = r.error.as_deref().unwrap_or("non-zero exit");
            if should_fail {
                println!("test {} [should_fail] ... FAILED\n  {err}", r.name);
            } else {
                println!("test {} ... FAILED\n  {err}", r.name);
            }
        }
    }

    if json {
        println!(
            "{{\"type\":\"summary\",\"total\":{n},\"passed\":{passed},\"failed\":{failed},\"skipped\":0,\"duration_ms\":{total_ms}}}"
        );
    } else {
        let outcome = if failed == 0 { "ok" } else { "FAILED" };
        println!(
            "\ntest result: {outcome}. {passed} passed, {failed} failed ({total_ms}ms total)"
        );
    }

    process::exit(if failed == 0 { 0 } else { 3 });
}

// ── Pipeline ──────────────────────────────────────────────────────────────────

/// Run the type-checking pipeline and return a list of error messages.
fn run_check_pipeline(
    program: &mut axon_core::ast::Program,
    source_path: &PathBuf,
) -> (Vec<String>, axon_core::infer::InferCtx) {
    let file = source_path.display().to_string();
    let mut all_errors: Vec<String> = Vec::new();

    // Step 0: load modules referenced by `use` declarations (AXON_PATH search).
    let search_dirs = axon_core::axon_search_dirs(std::env::current_exe().ok().as_deref());
    for e in axon_core::load_use_decls(program, &search_dirs) {
        all_errors.push(format!("[{}] {}", e.code, e.message));
    }

    // Step 1: name resolution
    let resolve_result = axon_core::resolver::resolve_program(program, &file);
    for diag in &resolve_result.errors {
        all_errors.push(format!("[{}] {}", diag.code, diag.message));
    }
    for warn in &resolve_result.warnings {
        eprintln!("warning: [{}] {}", warn.code, warn.message);
    }

    // Step 1b: fill lambda capture lists (post-resolution pass)
    axon_core::resolver::fill_captures(program);

    // Step 2: type inference
    let mut infer_ctx = axon_core::infer::InferCtx::new(&file);
    let _subst = infer_ctx.infer_program(program);
    for err in &infer_ctx.errors {
        let mut msg = format!("[{}] {}", err.code, err.message);
        if let Some(exp) = &err.expected {
            msg.push_str(&format!(" (expected {exp})"));
        }
        if let Some(fnd) = &err.found {
            msg.push_str(&format!(", found {fnd}"));
        }
        all_errors.push(msg);
    }

    // Step 3: type checking (uses infer results)
    let fn_sigs: std::collections::HashMap<String, axon_core::checker::FnSig> =
        infer_ctx.fn_sigs.iter()
            .map(|(k, v)| (k.clone(), axon_core::checker::FnSig {
                params: v.params.clone(),
                ret: v.ret.clone(),
            }))
            .collect();
    let mut check_ctx = axon_core::checker::CheckCtx::new(
        &file,
        fn_sigs,
        infer_ctx.struct_fields.clone(),
    );
    let check_errors = check_ctx.check_program(program, std::collections::HashMap::new());
    for err in &check_errors {
        let mut msg = format!("[{}] {}", err.code, err.message);
        if let Some(exp) = &err.expected {
            msg.push_str(&format!(" (expected {exp})"));
        }
        if let Some(fnd) = &err.found {
            msg.push_str(&format!(", found {fnd}"));
        }
        all_errors.push(msg);
    }

    // Step 4: borrow checking — enforce move semantics within function bodies.
    for item in &program.items {
        match item {
            axon_core::ast::Item::FnDef(fndef) => {
                let param_types: std::collections::HashMap<String, axon_core::types::Type> =
                    if let Some(sig) = infer_ctx.fn_sigs.get(&fndef.name) {
                        fndef.params.iter()
                            .zip(sig.params.iter())
                            .map(|(p, t)| (p.name.clone(), t.clone()))
                            .collect()
                    } else {
                        std::collections::HashMap::new()
                    };
                for err in axon_core::borrow::check_fn(fndef, param_types) {
                    all_errors.push(err.to_string());
                }
            }
            axon_core::ast::Item::ImplBlock(blk) => {
                let type_name = match &blk.for_type {
                    axon_core::ast::AxonType::Named(n) => n.clone(),
                    axon_core::ast::AxonType::Generic { base, .. } => base.clone(),
                    _ => "Unknown".into(),
                };
                for method in &blk.methods {
                    let key = format!("{type_name}__{}", method.name);
                    let param_types: std::collections::HashMap<String, axon_core::types::Type> =
                        if let Some(sig) = infer_ctx.fn_sigs.get(&key) {
                            method.params.iter()
                                .zip(sig.params.iter())
                                .map(|(p, t)| (p.name.clone(), t.clone()))
                                .collect()
                        } else {
                            std::collections::HashMap::new()
                        };
                    for err in axon_core::borrow::check_fn(method, param_types) {
                        all_errors.push(err.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    (all_errors, infer_ctx)
}

/// Compile the program to a native binary at `output`. Native AOT path —
/// only compiled with the `codegen` feature.
#[cfg(feature = "codegen")]
fn run_build_pipeline(
    program: &mut axon_core::ast::Program,
    source_path: &PathBuf,
    output: &PathBuf,
    opts: &BuildOptions,
) -> Result<(), String> {
    // Check first, fail fast on errors.
    let (errors, mut infer_ctx) = run_check_pipeline(program, source_path);
    if !errors.is_empty() {
        return Err(format!("{} error(s); build aborted", errors.len()));
    }

    let compiler_version = env!("CARGO_PKG_VERSION");
    let cache_dir = opts
        .cache_dir
        .clone()
        .unwrap_or_else(axon_core::default_cache_dir);
    let target_triple = opts.target_triple.as_deref();

    // ── Cache lookup ──────────────────────────────────────────────────────
    if !opts.no_cache {
        // Hash all source files to form the cache key.
        let mut hasher_input = Vec::new();
        // Include the source path stem as a namespace separator.
        hasher_input.extend_from_slice(
            source_path.to_string_lossy().as_bytes(),
        );
        if let Ok(bytes) = std::fs::read(source_path) {
            hasher_input.extend_from_slice(&bytes);
        }
        // Also include target triple in the key so cross-compiled artifacts
        // are cached separately from native ones.
        if let Some(triple) = target_triple {
            hasher_input.extend_from_slice(triple.as_bytes());
        }

        let key = axon_core::cache_key(&hasher_input, compiler_version);
        let cache_path = axon_core::cache_path(&key, &cache_dir);

        if let Some(bitcode) = axon_core::read_axc(&cache_path, compiler_version) {
            // Cache hit — skip IR emission, link from stored bitcode.
            return axon_core::compile_bitcode_to_binary(
                &bitcode,
                &output.to_string_lossy(),
                opts.release,
                target_triple,
            );
        }

        // Cache miss — full compilation then write.
        let result = build_ir_and_link(
            program,
            source_path,
            output,
            opts.release,
            target_triple,
            &mut infer_ctx,
            Some((&key, &cache_path, compiler_version)),
        );
        return result;
    }

    // --no-cache: full compilation, no read or write.
    build_ir_and_link(
        program,
        source_path,
        output,
        opts.release,
        target_triple,
        &mut infer_ctx,
        None,
    )
}

/// Emit LLVM IR, optionally write bitcode to cache, then link.
#[cfg(feature = "codegen")]
fn build_ir_and_link(
    program: &mut axon_core::ast::Program,
    source_path: &PathBuf,
    output: &PathBuf,
    release: bool,
    target_triple: Option<&str>,
    infer_ctx: &mut axon_core::infer::InferCtx,
    cache_write: Option<(&str, &std::path::Path, &str)>, // (key, path, version)
) -> Result<(), String> {
    // Collect generic instantiations recorded during inference.
    let instantiations = infer_ctx.drain_instantiations();

    // Monomorphize: expand generic functions into concrete instances.
    let mono = axon_core::mono::monomorphise(program, instantiations);
    let concrete_program = axon_core::ast::Program {
        items: mono.other_items.into_iter()
            .chain(mono.fns.into_iter().map(axon_core::ast::Item::FnDef))
            .collect(),
    };

    let ctx = inkwell::context::Context::create();
    let module_name = source_path.file_stem()
        .unwrap_or_default().to_string_lossy();
    let mut cg = axon_core::codegen::Codegen::new(&ctx, &module_name);
    cg.declare_functions(&concrete_program);
    cg.emit_program(&concrete_program);

    // Write bitcode to cache before linking (so a link failure doesn't
    // prevent future cache hits for successfully compiled IR).
    if let Some((_key, cache_path, compiler_version)) = cache_write {
        let bitcode = cg.emit_bitcode();
        let _ = axon_core::write_axc(cache_path, &bitcode, compiler_version);
    }

    cg.compile_to_binary_target(&output.to_string_lossy(), release, target_triple)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn read_source(file: &PathBuf) -> String {
    std::fs::read_to_string(file).unwrap_or_else(|e| {
        eprintln!("error reading {}: {e}", file.display());
        // Fix 8: exit 1 for I/O errors.
        process::exit(1);
    })
}

fn emit_error(msg: &str, as_json: bool) {
    if as_json {
        // Newline-delimited JSON — manually escape the message to avoid
        // pulling serde_json into the binary (would cause trait-solver
        // explosion combined with inkwell's type universe).
        let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");
        eprintln!("{{\"error\": \"{escaped}\"}}");
    } else {
        eprintln!("error: {msg}");
    }
}

/// Fix 5: Validate that `file` has a `.ax` extension.
/// Exits with code 1 if the extension is wrong.
fn validate_ax_extension(file: &PathBuf) {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "ax" {
        let filename = file.display();
        eprintln!("error: Axon source files must have a .ax extension (got '{filename}')");
        process::exit(1);
    }
}
