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
        Command::Run { file, release, args } => cmd_run(file, release, args),
        Command::Fmt { files, check } => cmd_fmt(files, check),
        Command::Doc { files, out } => cmd_doc(files, out),
        Command::Lsp => cmd_lsp(),
        Command::Cache { action } => cmd_cache(action),
        Command::Test { files, filter, jobs, json } => cmd_test(files, filter, jobs, json),
    }
}

// ── parse ─────────────────────────────────────────────────────────────────────

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

struct BuildOptions {
    release: bool,
    target_triple: Option<String>,
    no_cache: bool,
    cache_dir: Option<PathBuf>,
}

// ── run ───────────────────────────────────────────────────────────────────────

fn cmd_run(file: PathBuf, release: bool, args: Vec<String>) {
    // Fix 5: validate .ax extension.
    validate_ax_extension(&file);

    // Build to a temp file, then exec it.
    let tmp_dir = std::env::temp_dir();
    let stem = file.file_stem().unwrap_or_default().to_string_lossy();
    let tmp_bin = tmp_dir.join(format!("axon_run_{stem}_{}", process::id()));

    let src = read_source(&file);
    let mut program = match parse_source(&src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            // Fix 8: exit 2 for compile errors.
            process::exit(2);
        }
    };

    if let Err(e) = run_build_pipeline(
        &mut program,
        &file,
        &tmp_bin,
        &BuildOptions { release, target_triple: None, no_cache: true, cache_dir: None },
    ) {
        eprintln!("error: {e}");
        // Fix 8: exit 1 for system/linker errors after type-check passes.
        process::exit(1);
    }

    // Execute and forward the exit code.
    let status = std::process::Command::new(&tmp_bin)
        .args(&args)
        .status()
        .unwrap_or_else(|e| {
            eprintln!("error executing {}: {e}", tmp_bin.display());
            process::exit(1);
        });

    // Clean up temp binary.
    let _ = std::fs::remove_file(&tmp_bin);

    process::exit(status.code().unwrap_or(1));
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
    let effective_jobs = resolve_jobs(jobs);

    if !json {
        if effective_jobs > 1 {
            println!("running {n} test{} with {effective_jobs} workers", if n == 1 { "" } else { "s" });
        } else {
            println!("running {n} test{}", if n == 1 { "" } else { "s" });
        }
    }

    let all_results = run_tests_with_jobs(&program, primary_file, &test_meta, effective_jobs);

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

/// Resolve `--jobs` value: 0 → available parallelism, else use as-is (min 1).
fn resolve_jobs(jobs: usize) -> usize {
    if jobs == 0 {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
    } else {
        jobs
    }
}

/// Run tests sequentially or in parallel based on `jobs`.
fn run_tests_with_jobs(
    program: &axon_core::ast::Program,
    source_path: &PathBuf,
    test_meta: &[(String, bool)],
    jobs: usize,
) -> Vec<axon_core::codegen::TestResult> {
    if jobs <= 1 {
        return run_all_tests_as_subprocesses(program, source_path, test_meta);
    }
    run_tests_parallel(program, source_path, test_meta, jobs)
}

/// Run tests in parallel using `jobs` worker threads.
fn run_tests_parallel(
    program: &axon_core::ast::Program,
    source_path: &PathBuf,
    test_meta: &[(String, bool)],
    jobs: usize,
) -> Vec<axon_core::codegen::TestResult> {
    use std::sync::{Arc, Mutex};
    use std::sync::mpsc;

    if test_meta.is_empty() {
        return vec![];
    }

    let program = Arc::new(program.clone());
    let source_path = Arc::new(source_path.clone());
    let test_meta = Arc::new(test_meta.to_vec());

    // Work queue: indices into test_meta, shared across workers.
    let queue: Arc<Mutex<Vec<usize>>> =
        Arc::new(Mutex::new((0..test_meta.len()).rev().collect()));

    // Result channel.
    let (tx, rx) = mpsc::channel::<(usize, axon_core::codegen::TestResult)>();

    let worker_count = jobs.min(test_meta.len());
    let mut handles = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let prog = Arc::clone(&program);
        let path = Arc::clone(&source_path);
        let meta = Arc::clone(&test_meta);
        let q = Arc::clone(&queue);
        let tx = tx.clone();

        let handle = std::thread::spawn(move || {
            loop {
                let idx = {
                    let mut q = q.lock().unwrap();
                    q.pop()
                };
                let Some(idx) = idx else { break };

                let (name, should_fail) = &meta[idx];
                let start = std::time::Instant::now();
                let result = run_single_test_as_subprocess(&prog, &path, name);
                let duration_ms = start.elapsed().as_millis() as u64;

                let tr = match result {
                    Ok(exit_code) => {
                        let exited_ok = exit_code == 0;
                        let passed = if *should_fail { !exited_ok } else { exited_ok };
                        let error = if passed {
                            None
                        } else if *should_fail {
                            Some(format!(
                                "should_fail test '{name}' exited 0 (expected non-zero)"
                            ))
                        } else {
                            Some(format!("test '{name}' exited with code {exit_code}"))
                        };
                        axon_core::codegen::TestResult { name: name.clone(), passed, duration_ms, error }
                    }
                    Err(e) => axon_core::codegen::TestResult {
                        name: name.clone(),
                        passed: false,
                        duration_ms,
                        error: Some(e),
                    },
                };

                if tx.send((idx, tr)).is_err() {
                    break; // main thread dropped receiver
                }
            }
        });
        handles.push(handle);
    }
    drop(tx); // close sender so rx.iter() terminates

    let mut results: Vec<Option<axon_core::codegen::TestResult>> =
        (0..test_meta.len()).map(|_| None).collect();
    for (idx, tr) in rx {
        results[idx] = Some(tr);
    }

    for h in handles {
        h.join().expect("test worker thread panicked");
    }

    results.into_iter().map(|r| r.unwrap()).collect()
}

/// Run ALL test functions as subprocesses (both normal and should_fail).
///
/// Fix 1: This avoids in-process JIT execution where a test calling exit(1)
/// would kill the entire test runner, silently dropping remaining tests.
///
/// Strategy: compile one temporary binary containing all test functions plus a
/// `main` dispatcher.  Then, for each test function, invoke the binary with
/// `--run-test <name>`.  Normal tests pass if exit code is 0; should_fail
/// tests pass if exit code is non-zero.
fn run_all_tests_as_subprocesses(
    program: &axon_core::ast::Program,
    source_path: &PathBuf,
    test_meta: &[(String, bool)],
) -> Vec<axon_core::codegen::TestResult> {
    if test_meta.is_empty() {
        return vec![];
    }

    // Build one binary with a dispatcher main that accepts `--run-test <name>`.
    let tmp_dir = std::env::temp_dir();
    let stem = source_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let tmp_bin = tmp_dir.join(format!(
        "axon_test_{stem}_{pid}",
        pid = process::id()
    ));

    // We use the per-test subprocess approach (one binary per test) because
    // inserting a runtime dispatcher into the Axon IR is complex and fragile.
    // This matches what `run_single_should_fail_test` already does and is
    // correct: each test gets its own clean process.
    let results: Vec<axon_core::codegen::TestResult> = test_meta
        .iter()
        .map(|(name, should_fail)| {
            let start = std::time::Instant::now();
            let result = run_single_test_as_subprocess(program, source_path, name);
            let duration_ms = start.elapsed().as_millis() as u64;

            match result {
                Ok(exit_code) => {
                    let exited_ok = exit_code == 0;
                    let passed = if *should_fail { !exited_ok } else { exited_ok };
                    let error = if passed {
                        None
                    } else if *should_fail {
                        Some(format!(
                            "should_fail test '{name}' exited 0 (expected non-zero / panic)"
                        ))
                    } else {
                        Some(format!("test '{name}' exited with code {exit_code}"))
                    };
                    axon_core::codegen::TestResult {
                        name: name.clone(),
                        passed,
                        duration_ms,
                        error,
                    }
                }
                Err(e) => axon_core::codegen::TestResult {
                    name: name.clone(),
                    passed: false,
                    duration_ms,
                    error: Some(e),
                },
            }
        })
        .collect();

    // Clean up temp binary if it somehow exists (shouldn't, per-test binaries
    // are cleaned up inside run_single_test_as_subprocess).
    let _ = std::fs::remove_file(&tmp_bin);

    results
}

/// Compile a synthetic program where `test_name` is called from `main`,
/// execute it as a subprocess, and return the exit code.
///
/// Returns `Ok(exit_code)` on successful execution, `Err(msg)` on
/// compilation or spawn failure.
fn run_single_test_as_subprocess(
    program: &axon_core::ast::Program,
    source_path: &PathBuf,
    test_name: &str,
) -> Result<i32, String> {
    use axon_core::ast::{FnDef, Expr, Stmt, Item};

    // Clone the program and add a synthetic `main` that calls `test_name()`.
    // Remove any existing `main` to avoid duplicate symbols.
    let mut items: Vec<Item> = program
        .items
        .iter()
        .filter(|item| {
            if let Item::FnDef(f) = item {
                f.name != "main"
            } else {
                true
            }
        })
        .cloned()
        .collect();

    // Add: fn main() { <test_name>() }
    let call_test = Expr::Call {
        callee: Box::new(Expr::Ident(test_name.to_string())),
        args: vec![],
    };
    let main_fn = FnDef {
        public: false,
        name: "main".to_string(),
        generic_params: vec![],
        generic_bounds: vec![],
        params: vec![],
        return_type: None,
        body: Expr::Block(vec![Stmt::simple(call_test)]),
        attrs: vec![],
        contained: None,
        span: axon_core::span::Span::dummy(),
    };
    items.push(Item::FnDef(main_fn));

    let synthetic = axon_core::ast::Program { items };

    // Compile to a temp binary.
    let tmp_dir = std::env::temp_dir();
    let tmp_bin = tmp_dir.join(format!(
        "axon_test_{stem}_{name}_{pid}",
        stem = source_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy(),
        name = test_name,
        pid = process::id(),
    ));

    let ctx = inkwell::context::Context::create();
    let module_name = format!("test_{test_name}");
    let mut cg = axon_core::codegen::Codegen::new(&ctx, &module_name);
    cg.declare_functions(&synthetic);
    cg.emit_program(&synthetic);
    cg.compile_to_binary(&tmp_bin.to_string_lossy(), /*release=*/ false)?;

    // Execute as a subprocess.
    let status = std::process::Command::new(&tmp_bin)
        .status()
        .map_err(|e| format!("spawn {}: {e}", tmp_bin.display()))?;

    let _ = std::fs::remove_file(&tmp_bin);

    Ok(status.code().unwrap_or(1))
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

/// Compile the program to a native binary at `output`.
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
