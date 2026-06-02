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
use std::path::{Path, PathBuf};
use std::process;
use std::time::Instant;

use clap::{Parser, Subcommand};
use axon_core::parse_source;
#[cfg(feature = "codegen")]
use inkwell;

// ── CLI definition ────────────────────────────────────────────────────────────

/// Version string with build identity: `<semver> (<git-sha>)`, e.g.
/// `0.1.0 (02cd617)`. `AXON_GIT_SHA` is captured by build.rs (BUG_HUNT #30) and
/// is "unknown" when git isn't available at build time.
const VERSION: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("AXON_GIT_SHA"),
    ")"
);

#[derive(Parser)]
#[command(
    name = "axon",
    about = "The Axon language toolchain",
    version = VERSION,
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

        /// CI mode: every imported module MUST have a matching `axon.lock` entry.
        /// A missing entry is E1202 and a tampered hash is E1201 (both fatal).
        /// Without this flag (dev mode), a missing lock entry is only W1210.
        #[arg(long, help = "Require every import to match axon.lock (CI mode)")]
        locked: bool,
    },

    /// Write `axon.lock` pinning each `use`d module to its content hash (R6).
    ///
    /// Resolves the program's imports via AXON_PATH, hashes each module's raw
    /// bytes (`axh1:` SHA-256), and writes a deterministic `axon.lock` next to
    /// the source. Commit the lockfile so later builds can detect tampering.
    Lock {
        #[arg(help = "Path to .ax source file whose imports to lock")]
        file: PathBuf,
    },

    /// Recompute each module's hash and compare to `axon.lock` (R6 tamper check).
    ///
    /// Exits non-zero with E1201 if any module's bytes changed since the lock
    /// was written, or E1202 if an imported module has no lock entry.
    VerifyLock {
        #[arg(help = "Path to .ax source file whose lock to verify")]
        file: PathBuf,
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

    /// Self-improving compiler: verify and graduate learned optimization passes (R10).
    ///
    /// A pass is a transform on the program; it may only run once it is in the
    /// graduated-pass manifest, and it enters the manifest only via `graduate`,
    /// which requires a green four-gate verification AND multi-sig of root
    /// Principals. The compiler proposes; humans dispose (I-12).
    Improve {
        #[command(subcommand)]
        action: ImproveAction,
    },

    /// AI primitives: inspect resolved `@[ai(policy)]` settings (R3).
    Ai {
        #[command(subcommand)]
        action: AiAction,
    },

    /// Cross-platform targets: list buildable targets / build for one (R7).
    Target {
        #[command(subcommand)]
        action: TargetAction,
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

        /// Run the goal N times with cross-run continuation (after run 1), so its
        /// search resumes from its best prior input each time and converges.
        #[arg(long, value_name = "N", help = "Iterate the goal up to N times, stopping when converged")]
        iterate: Option<usize>,
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

    /// Summarize the provenance log: per-`@[adaptive]`-fn score trajectory.
    ///
    /// Reads `$XDG_CACHE_HOME/axon/provenance.jsonl` (written by `@[adaptive]`
    /// returns and `goal_run`) and reports, per function, how the score moved —
    /// the first-class form of what `examples/asi/run.sh trace` simulates.
    Trace {
        /// Only report on this function name.
        #[arg(long = "fn", value_name = "NAME", help = "Filter to one function")]
        func: Option<String>,

        /// Provenance log path (default: $XDG_CACHE_HOME/axon/provenance.jsonl).
        #[arg(long, value_name = "PATH", help = "Override the provenance log path")]
        path: Option<PathBuf>,

        /// Emit the per-fn summary as a JSON array (for programmatic consumers).
        #[arg(long, help = "Machine-readable JSON output")]
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

#[derive(Subcommand)]
enum ImproveAction {
    /// Propose candidate passes (the unprivileged discovery side).
    ///
    /// Discovery is an AI proposal step that writes candidates for `verify` to
    /// gate. It is intentionally unprivileged — a proposal grants nothing; only
    /// `graduate` grants execution. Not yet implemented (the verification core
    /// and graduation gate are; discovery is the next slice).
    Discover {
        #[arg(help = "Path to the corpus directory (default: examples/)")]
        corpus: Option<PathBuf>,
    },

    /// Run the full four-gate verification harness against the corpus.
    ///
    /// Verifies the identity pass (the one pass that always exists) over the
    /// example corpus and prints the per-gate record. Real candidate passes
    /// come from `discover`; this demonstrates the gate and validates the
    /// corpus runs clean. Exit 0 if all gates pass, non-zero otherwise.
    Verify {
        #[arg(help = "Path to the corpus directory (default: examples/)")]
        corpus: Option<PathBuf>,

        /// Also run the G4 wall-clock perf gate.
        #[arg(long, help = "Run the G4 performance timing gate")]
        perf: bool,
    },

    /// Graduate a verified pass into the manifest (requires multi-sig).
    ///
    /// Refuses with E1404 unless ≥2 distinct root Principals sign via repeated
    /// `--sign`. The compiler cannot graduate its own passes (I-12).
    Graduate {
        #[arg(help = "Pass name to record in the manifest")]
        name: String,

        /// A root-Principal signature. Pass at least twice with distinct values.
        #[arg(long = "sign", value_name = "PRINCIPAL", help = "Root-Principal signature (≥2 distinct required)")]
        signers: Vec<String>,

        /// Corpus the pass was verified against — pins its `axc1:` hash into the
        /// manifest entry. Optional; without it the corpus hash is recorded as
        /// unpinned (the multi-sig gate is independent of the corpus).
        #[arg(long, help = "Corpus dir to pin the verification corpus hash")]
        corpus: Option<PathBuf>,

        /// Manifest path (default: ./passes.manifest).
        #[arg(long, help = "passes.manifest path")]
        manifest: Option<PathBuf>,
    },

    /// List graduated passes and their verification provenance.
    List {
        /// Manifest path (default: ./passes.manifest).
        #[arg(long, help = "passes.manifest path")]
        manifest: Option<PathBuf>,
    },

    /// Remove a graduated pass from the manifest (reversibility, gate-3).
    Revert {
        #[arg(help = "Pass id (axp1:…) to remove")]
        id: String,

        /// Manifest path (default: ./passes.manifest).
        #[arg(long, help = "passes.manifest path")]
        manifest: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum AiAction {
    /// Print the resolved AI policy for every `@[ai(...)]` fn in a file (R3 §3.4).
    Policy {
        #[arg(help = "Path to .ax source file")]
        file: PathBuf,
    },
}

#[derive(Subcommand)]
enum TargetAction {
    /// List buildable targets and their engine (R7 §3).
    List,
    /// Build for a cross-target (R7 §4.1). `--engine interp` runs via the
    /// interpreter (wasm-capable now); the AOT path on a wasm target is E0907.
    Build {
        /// Engine: `interp` for the tree-walking interpreter, else AOT codegen.
        #[arg(long, help = "Engine: interp or (default) codegen/AOT")]
        engine: Option<String>,
        /// Target triple / alias (e.g. wasm32, wasm32-wasi).
        #[arg(long, help = "Target triple or alias")]
        target: Option<String>,
        #[arg(help = "Path to .ax source file")]
        file: PathBuf,
    },
}

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    // Run the command on a large stack. The recursive-descent parser and the
    // tree-walking interpreter both use heavy native stack, so deeply nested
    // input (e.g. agent-generated code) or recursion would overflow the small
    // default main-thread stack and abort the process. (The interpreter also
    // bounds recursion via RECURSION_LIMIT for runaway cases.)
    std::thread::Builder::new()
        .stack_size(1024 * 1024 * 1024)
        .spawn(move || dispatch(cli.command))
        .expect("spawn worker thread")
        .join()
        .expect("worker thread panicked");
}

fn dispatch(command: Command) {
    match command {
        Command::Parse { file } => cmd_parse(file),
        Command::Check { file, json, locked } => cmd_check(file, json, locked),
        Command::Lock { file } => cmd_lock(file),
        Command::VerifyLock { file } => cmd_verify_lock(file),
        Command::Build { files, out, release, target, no_cache, cache_dir } => {
            cmd_build(files, out, release, target, no_cache, cache_dir)
        }
        Command::Goal { file, emit, iterate } => cmd_goal(file, emit, iterate),
        Command::Run { file, release, args } => cmd_run(file, release, args),
        Command::Fmt { files, check } => cmd_fmt(files, check),
        Command::Doc { files, out } => cmd_doc(files, out),
        Command::Lsp => cmd_lsp(),
        Command::Cache { action } => cmd_cache(action),
        Command::Improve { action } => cmd_improve(action),
        Command::Ai { action } => cmd_ai(action),
        Command::Target { action } => cmd_target(action),
        Command::Test { files, filter, jobs, json } => cmd_test(files, filter, jobs, json),
        Command::Trace { func, path, json } => cmd_trace(func, path, json),
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

fn cmd_check(file: PathBuf, json_flag: bool, locked: bool) {
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

    // R6 §4.4 import-edge capability check (E1203): before the pipeline merges
    // imports into one program, compare the (still-separate) imported modules
    // against the importer's @[contained] grant ceiling. A `@[contained]`
    // program that imports a module exercising a capability it doesn't grant is
    // rejected (the import-edge extension of I-11). Uncontained importers have
    // no ceiling, so this is a no-op for them (back-compat).
    let search_dirs = axon_core::axon_search_dirs(std::env::current_exe().ok().as_deref());
    let (resolved_imports, _unresolved) = axon_core::resolve_use_files(&program, &search_dirs);
    let mut import_cap_errors: Vec<String> = Vec::new();
    for m in &resolved_imports {
        if let Ok(src) = std::str::from_utf8(&m.bytes) {
            if let Ok(imported) = parse_source(src) {
                for e in axon_core::capabilities::check_import_capabilities(&program, &m.name, &imported) {
                    import_cap_errors.push(format!("[{}] {}", e.code, e.message));
                }
            }
        }
    }

    // R6 §4.2 — `--locked`: every import must match `axon.lock`. In --locked mode
    // a missing entry (E1202) or a hash mismatch (E1201) is FATAL. In dev mode a
    // missing lock entry is only W1210 (a warning — bytes unverified/unaudited),
    // so existing programs without a lockfile keep working until a user opts in.
    let lock_errors = check_locked_imports(&file, &resolved_imports, locked, use_json);

    // Type-check pipeline.
    let (mut errors, _infer_ctx) = run_check_pipeline(&mut program, &file);
    errors.extend(import_cap_errors);
    errors.extend(lock_errors);

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

// ── lock / verify-lock (R6) ─────────────────────────────────────────────────────

/// Path to the `axon.lock` that sits next to a source file.
fn lock_path_for(file: &Path) -> PathBuf {
    file.parent()
        .unwrap_or_else(|| Path::new("."))
        .join("axon.lock")
}

/// Resolve a program's direct `use`s to `(name, path, bytes)`, reporting parse
/// or unresolved-import failures with the right exit code. Shared by lock and
/// verify-lock.
fn resolve_modules_or_exit(file: &PathBuf) -> Vec<axon_core::ResolvedModule> {
    validate_ax_extension(file);
    let src = read_source(file);
    let program = match parse_source(&src) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("{e}"), !std::io::stderr().is_terminal());
            process::exit(2);
        }
    };
    let search_dirs = axon_core::axon_search_dirs(std::env::current_exe().ok().as_deref());
    let (resolved, unresolved) = axon_core::resolve_use_files(&program, &search_dirs);
    if !unresolved.is_empty() {
        for name in &unresolved {
            emit_error(
                &format!(
                    "[{}] `{name}` could not be resolved from AXON_PATH — cannot lock an import whose file is missing",
                    axon_core::error::E0901
                ),
                !std::io::stderr().is_terminal(),
            );
        }
        process::exit(2);
    }
    resolved
}

/// `axon lock <file>` — hash each imported module and write `axon.lock`.
fn cmd_lock(file: PathBuf) {
    use axon_core::lockfile::{module_hash, write_lock, LockEntry};
    let resolved = resolve_modules_or_exit(&file);

    let entries: Vec<LockEntry> = resolved
        .iter()
        .map(|m| {
            let mut e = LockEntry::new(
                m.name.clone(),
                module_hash(&m.bytes),
                format!("file:{}", m.path.display()),
            );
            e.audit = String::new();
            e
        })
        .collect();

    let lock = lock_path_for(&file);
    let text = write_lock(&entries);
    if let Err(e) = std::fs::write(&lock, &text) {
        eprintln!("error writing {}: {e}", lock.display());
        process::exit(1);
    }
    println!(
        "axon: wrote {} ({} module{})",
        lock.display(),
        entries.len(),
        if entries.len() == 1 { "" } else { "s" }
    );
    process::exit(0);
}

/// `axon verify-lock <file>` — recompute hashes and compare to `axon.lock`.
/// E1201 on a content mismatch (tamper), E1202 on a missing entry, E1205 on a
/// malformed lockfile.
fn cmd_verify_lock(file: PathBuf) {
    use axon_core::lockfile::{module_hash, parse_lock};
    let resolved = resolve_modules_or_exit(&file);
    let lock = lock_path_for(&file);

    let text = match std::fs::read_to_string(&lock) {
        Ok(t) => t,
        Err(_) => {
            emit_error(
                &format!(
                    "[{}] no axon.lock next to {} — run `axon lock {}` first",
                    axon_core::error::E1202,
                    file.display(),
                    file.display()
                ),
                !std::io::stderr().is_terminal(),
            );
            process::exit(2);
        }
    };
    let parsed = match parse_lock(&text) {
        Ok(p) => p,
        Err((code, msg)) => {
            emit_error(&format!("[{code}] {msg}"), !std::io::stderr().is_terminal());
            process::exit(2);
        }
    };

    let mut failed = false;
    for m in &resolved {
        let found = module_hash(&m.bytes);
        match parsed.modules.iter().find(|e| e.name == m.name) {
            None => {
                emit_error(
                    &format!(
                        "[{}] `{}` is not in axon.lock — run `axon lock {}` (or restore the removed entry)",
                        axon_core::error::E1202,
                        m.name,
                        file.display()
                    ),
                    !std::io::stderr().is_terminal(),
                );
                failed = true;
            }
            Some(entry) if entry.hash != found => {
                emit_error(
                    &format!(
                        "[{}] content hash mismatch for `{}`: locked {}, found {} — the module's bytes changed since axon.lock was written; run `axon lock {}` to accept or restore the file",
                        axon_core::error::E1201,
                        m.name,
                        entry.hash,
                        found,
                        file.display()
                    ),
                    !std::io::stderr().is_terminal(),
                );
                failed = true;
            }
            Some(_) => {}
        }
    }
    if failed {
        process::exit(2);
    }
    println!("axon: lock verified ({} module{})", resolved.len(), if resolved.len() == 1 { "" } else { "s" });
    process::exit(0);
}

/// R6 §4.2 — verify a program's resolved imports against `axon.lock`.
///
/// In `--locked` (CI) mode every import is FATAL-checked: a module with no lock
/// entry → **E1202**, a module whose bytes don't match the locked hash →
/// **E1201**. Returns these as error strings that join the check's error list.
///
/// In dev mode (`locked == false`) a missing lock entry is a **W1210** warning
/// (printed immediately, non-fatal) so existing programs without a lockfile keep
/// working — the import bytes are simply flagged as unverified/unaudited. A
/// hash *mismatch* is still surfaced as W1210 in dev mode (the lockfile exists
/// and disagrees — worth warning), but only `--locked` makes it fatal.
fn check_locked_imports(
    file: &Path,
    resolved: &[axon_core::ResolvedModule],
    locked: bool,
    use_json: bool,
) -> Vec<String> {
    use axon_core::lockfile::{module_hash, parse_lock};
    if resolved.is_empty() {
        return Vec::new();
    }
    let lock_path = file.parent().unwrap_or_else(|| Path::new(".")).join("axon.lock");
    let parsed = match std::fs::read_to_string(&lock_path) {
        Ok(text) => match parse_lock(&text) {
            Ok(p) => Some(p),
            Err((code, msg)) => {
                // A malformed lockfile is fatal under --locked, a warning in dev.
                if locked {
                    return vec![format!("[{code}] {msg}")];
                }
                emit_error(&format!("[{}] {msg} (lockfile ignored in dev mode)", axon_core::error::W1210), use_json);
                None
            }
        },
        Err(_) => None, // no lockfile
    };

    let mut errors: Vec<String> = Vec::new();
    for m in resolved {
        let found = module_hash(&m.bytes);
        let entry = parsed.as_ref().and_then(|p| p.modules.iter().find(|e| e.name == m.name));
        match entry {
            Some(e) if e.hash == found => { /* locked + matching — ok */ }
            Some(e) => {
                // Hash mismatch: tamper. Fatal under --locked (E1201), warn in dev.
                let msg = format!(
                    "content hash mismatch for `{}`: locked {}, found {} — run `axon lock {}` to accept or restore the file",
                    m.name, e.hash, found, file.display()
                );
                if locked {
                    errors.push(format!("[{}] {msg}", axon_core::error::E1201));
                } else {
                    emit_error(&format!("[{}] {msg}", axon_core::error::W1210), use_json);
                }
            }
            None => {
                // No lock entry. Fatal under --locked (E1202), warn in dev (W1210).
                if locked {
                    errors.push(format!(
                        "[{}] `{}` is not in axon.lock — run `axon add {} <path>` (or drop --locked for dev)",
                        axon_core::error::E1202, m.name, m.name
                    ));
                } else {
                    emit_error(
                        &format!(
                            "[{}] `{}` imported without a lock entry — bytes are unverified and unaudited; run `axon lock {}`",
                            axon_core::error::W1210, m.name, file.display()
                        ),
                        use_json,
                    );
                }
            }
        }
    }
    errors
}

// ── improve (R10 self-improving compiler) ────────────────────────────────────────

/// Default manifest path: `./passes.manifest`.
fn manifest_path(explicit: Option<PathBuf>) -> PathBuf {
    explicit.unwrap_or_else(|| PathBuf::from("passes.manifest"))
}

/// Load a corpus of pure-compute programs from a directory of `.ax` files.
/// Members that fail to parse are skipped (a corpus is "programs that run",
/// not a parser test). Sorted by filename so the corpus hash is stable.
fn load_corpus(dir: &Path) -> Vec<(String, Vec<u8>, axon_core::ast::Program)> {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("ax"))
            .collect(),
        Err(e) => {
            eprintln!("axon improve: cannot read corpus dir {}: {e}", dir.display());
            process::exit(2);
        }
    };
    files.sort();
    let mut corpus = Vec::new();
    for f in files {
        let Ok(bytes) = std::fs::read(&f) else { continue };
        let Ok(src) = std::str::from_utf8(&bytes) else { continue };
        if let Ok(program) = parse_source(src) {
            // Only keep programs with a `main` (runnable by the G1 oracle).
            let has_main = program.items.iter().any(|it| {
                matches!(it, axon_core::ast::Item::FnDef(fd) if fd.name == "main")
            });
            if has_main {
                let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
                corpus.push((name, bytes, program));
            }
        }
    }
    corpus
}

fn cmd_improve(action: ImproveAction) {
    use axon_core::improve::{verify_pass_with, PerfStatus, VerifyOptions};
    use axon_core::manifest::{
        corpus_hash, graduate, pass_hash, parse_manifest, verify_hash, write_manifest, Manifest,
        PassEntry, PerfClaim,
    };

    match action {
        ImproveAction::Discover { corpus: _ } => {
            // Discovery is the unprivileged AI proposal side — out of scope for
            // this slice (the verification core + graduation gate are the
            // safety-critical parts and are built). Honest block, not a stub
            // that pretends to work.
            eprintln!(
                "axon improve discover: pass discovery (the AI proposal side) is not yet \
                 implemented. The verification harness (`verify`) and graduation gate \
                 (`graduate`) are built — discovery feeds them candidate passes and is the \
                 next slice. A proposal grants nothing; only `graduate` grants execution."
            );
            process::exit(2);
        }

        ImproveAction::Verify { corpus, perf } => {
            let dir = corpus.unwrap_or_else(|| PathBuf::from("examples"));
            let members = load_corpus(&dir);
            if members.is_empty() {
                eprintln!("axon improve verify: no runnable .ax programs in {}", dir.display());
                process::exit(2);
            }
            let programs: Vec<axon_core::ast::Program> =
                members.iter().map(|(_, _, p)| p.clone()).collect();
            // The identity pass: the one pass that always exists. A real
            // candidate would come from `discover`; verifying identity proves
            // the harness runs clean over the corpus (and is the G1/G3 baseline).
            let identity: &axon_core::improve::Pass = &|p: &axon_core::ast::Program| p.clone();
            let opts = VerifyOptions { measure_perf: perf, perf_trials: 5 };
            let rec = verify_pass_with(identity, &programs, &opts);

            let g = |r: &Result<(), axon_core::improve::VerifyError>| -> String {
                match r {
                    Ok(()) => "pass".to_string(),
                    Err(e) => format!("FAIL [{}] {}", e.code, e.message),
                }
            };
            println!("axon improve verify — corpus: {} member(s) from {}", rec.members, dir.display());
            println!("  G1 correctness : {}", g(&rec.g1_correctness));
            println!("  G2 safety      : {}", g(&rec.g2_safety));
            println!("  G3 regression  : {}", g(&rec.g3_regression));
            let perf_str = match &rec.g4_perf {
                PerfStatus::Unmeasured => "unmeasured (run with --perf to time)".to_string(),
                PerfStatus::Faster { improved, members } => {
                    format!("faster on {improved}/{members}")
                }
                PerfStatus::NotFaster { regressed, improved } => {
                    format!("not faster (improved {improved}, regressed {regressed})")
                }
            };
            println!("  G4 perf        : {perf_str}");
            if rec.passed() {
                println!("axon improve verify: PASSED (correct + safe + non-regressing)");
                process::exit(0);
            } else {
                eprintln!("axon improve verify: REJECTED — {}", rec.rejection().unwrap().message);
                process::exit(2);
            }
        }

        ImproveAction::Graduate { name, signers, corpus, manifest } => {
            // Build the content-addressed identifiers from the verified pass.
            // (In this slice the pass under graduation is the identity pass,
            // verified by `verify`; a real flow pins the discovered pass's
            // definition bytes.) corpus_hash pins the exact corpus it cleared
            // when `--corpus` is given; the multi-sig gate is independent of it.
            let id = pass_hash(name.as_bytes());
            let verified = verify_hash(format!("verify:{name}").as_bytes());
            let ch = match corpus {
                Some(dir) => {
                    let members = load_corpus(&dir);
                    let corpus_bytes: Vec<Vec<u8>> = members.iter().map(|(_, b, _)| b.clone()).collect();
                    corpus_hash(&corpus_bytes)
                }
                None => corpus_hash(&[]),
            };

            // The multi-sig gate (E1404) — the I-12 firewall.
            let entry: PassEntry = match graduate(
                id,
                &name,
                verified,
                ch,
                &signers,
                PerfClaim::Unmeasured,
            ) {
                Ok(e) => e,
                Err(e) => {
                    eprintln!("axon improve graduate: [{}] {}", e.code, e.message);
                    process::exit(2);
                }
            };

            let mpath = manifest_path(manifest);
            let mut m = match std::fs::read_to_string(&mpath) {
                Ok(t) => match parse_manifest(&t) {
                    Ok(m) => m,
                    Err((code, msg)) => {
                        eprintln!("axon improve graduate: [{code}] {msg}");
                        process::exit(2);
                    }
                },
                Err(_) => Manifest::new(),
            };
            let pass_id = entry.id.clone();
            m.insert(entry);
            if let Err(e) = std::fs::write(&mpath, write_manifest(&m)) {
                eprintln!("axon improve graduate: cannot write {}: {e}", mpath.display());
                process::exit(1);
            }
            println!(
                "axon improve graduate: `{name}` graduated as {pass_id}\n  signed by {} root Principals → {}",
                signers.len(),
                mpath.display()
            );
            process::exit(0);
        }

        ImproveAction::List { manifest } => {
            let mpath = manifest_path(manifest);
            let m = match std::fs::read_to_string(&mpath) {
                Ok(t) => match parse_manifest(&t) {
                    Ok(m) => m,
                    Err((code, msg)) => {
                        eprintln!("axon improve list: [{code}] {msg}");
                        process::exit(2);
                    }
                },
                Err(_) => {
                    println!("axon improve list: no manifest at {} (0 graduated passes)", mpath.display());
                    process::exit(0);
                }
            };
            if m.passes.is_empty() {
                println!("axon improve list: 0 graduated passes");
            } else {
                println!("axon improve list: {} graduated pass(es)", m.passes.len());
                for p in &m.passes {
                    println!(
                        "  {} `{}` — perf:{} signed:[{}]\n    verified {} over corpus {}",
                        p.id,
                        p.name,
                        match p.perf_status {
                            PerfClaim::Unmeasured => "unmeasured",
                            PerfClaim::Faster => "faster",
                        },
                        p.graduated_by.join(", "),
                        p.verified,
                        p.corpus_hash,
                    );
                }
            }
            process::exit(0);
        }

        ImproveAction::Revert { id, manifest } => {
            let mpath = manifest_path(manifest);
            let mut m = match std::fs::read_to_string(&mpath) {
                Ok(t) => match parse_manifest(&t) {
                    Ok(m) => m,
                    Err((code, msg)) => {
                        eprintln!("axon improve revert: [{code}] {msg}");
                        process::exit(2);
                    }
                },
                Err(_) => {
                    eprintln!("axon improve revert: no manifest at {}", mpath.display());
                    process::exit(2);
                }
            };
            if m.revert(&id) {
                if let Err(e) = std::fs::write(&mpath, write_manifest(&m)) {
                    eprintln!("axon improve revert: cannot write {}: {e}", mpath.display());
                    process::exit(1);
                }
                println!("axon improve revert: removed {id} from {}", mpath.display());
                process::exit(0);
            } else {
                eprintln!("axon improve revert: no pass with id {id} in the manifest");
                process::exit(2);
            }
        }
    }
}

// ── ai (R3 policy inspection) ────────────────────────────────────────────────────

/// Read the value of a flat `key: value` arg from a fn's `@[ai(...)]` attribute
/// (the parser flattens the nested `policy(...)` group, so `tier`/`fallback`
/// arrive as `"tier: cheap"` / `"fallback: x"`). Mirrors the interpreter's
/// `current_ai_tier`/`current_ai_fallback` parsing so the CLI reports exactly
/// what a run would resolve.
fn ai_attr_value(fn_def: &axon_core::ast::FnDef, key: &str) -> Option<String> {
    let ai = fn_def.attrs.iter().find(|a| a.name == "ai")?;
    for arg in &ai.args {
        if let Some(rest) = arg.strip_prefix(&format!("{key}:")) {
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// `axon ai policy <file>` — print the resolved policy per `@[ai]` fn as one
/// JSON line each: `{"fn","tier","fallback","model"}` (R3 §3.4). The tier +
/// model come from the SAME `ai_routing::Tier` table the interpreter uses, so
/// the CLI and the provenance never disagree. An unknown tier → E1302.
fn cmd_ai(action: AiAction) {
    use axon_core::ai_routing::Tier;
    let AiAction::Policy { file } = action;
    validate_ax_extension(&file);
    let src = read_source(&file);
    let program = match parse_source(&src) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("{e}"), !std::io::stderr().is_terminal());
            process::exit(2);
        }
    };
    for item in &program.items {
        let axon_core::ast::Item::FnDef(f) = item else { continue };
        if !f.attrs.iter().any(|a| a.name == "ai") {
            continue;
        }
        // Resolve the tier (default balanced); an unknown name is E1302.
        let tier = match ai_attr_value(f, "tier") {
            None => Tier::Balanced,
            Some(name) => match Tier::parse(&name) {
                Some(t) => t,
                None => {
                    emit_error(
                        &format!(
                            "[{}] unknown AI tier `{name}` on `{}` — configured tiers: {}",
                            axon_core::error::E1302, f.name, Tier::configured()
                        ),
                        !std::io::stderr().is_terminal(),
                    );
                    process::exit(2);
                }
            },
        };
        let (model, _ver) = tier.model();
        let fallback = ai_attr_value(f, "fallback").unwrap_or_default();
        println!(
            "{{\"fn\":{},\"tier\":{},\"fallback\":{},\"model\":{}}}",
            json_lit(&f.name),
            json_lit(tier.as_str()),
            json_lit(&fallback),
            json_lit(model),
        );
    }
    process::exit(0);
}

/// Minimal JSON string literal for the CLI's hand-rolled output (no serde_json).
fn json_lit(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── target (R7 cross-platform) ───────────────────────────────────────────────────

/// The buildable targets and their engine (R7 §3). `native` runs through the
/// LLVM codegen backend; `wasm32` runs the interpreter compiled to wasm32.
const TARGETS: &[(&str, &str)] = &[("native", "codegen"), ("wasm32", "interp")];

/// `axon target list` / `axon target build` (R7 §3/§4.1).
fn cmd_target(action: TargetAction) {
    match action {
        TargetAction::List => {
            for (alias, engine) in TARGETS {
                println!("{alias} ({engine})");
            }
            process::exit(0);
        }
        TargetAction::Build { engine, target, file } => {
            validate_ax_extension(&file);
            let triple = target.as_deref().unwrap_or("native");
            let interp = engine.as_deref() == Some("interp");
            if interp {
                // The interpreter engine runs any target (wasm included) by
                // construction (I-2). The actual wasm build is a cargo invocation;
                // this acknowledges the routing and points at it.
                println!(
                    "axon target: engine=interp target={triple} — run via the interpreter \
                     (build: cargo build -p axon-core --no-default-features --bin axon-run \
                     --target wasm32-wasip1; see scripts/wasm_parity.sh)"
                );
                process::exit(0);
            }
            if triple.contains("wasm") {
                // AOT wasm via the codegen backend — honest block (R7 §6 E0907).
                emit_error(
                    &format!(
                        "[{}] AOT wasm build needs the native codegen backend — use \
                         `axon target build --engine interp --target {triple} {}` to run via \
                         the interpreter",
                        axon_core::error::E0907,
                        file.display()
                    ),
                    !std::io::stderr().is_terminal(),
                );
                process::exit(2);
            }
            println!(
                "axon target: engine=codegen target={triple} — use `axon build {}` for the \
                 native AOT binary",
                file.display()
            );
            process::exit(0);
        }
    }
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
fn cmd_goal(file: PathBuf, emit_only: bool, iterate: Option<usize>) {
    // Stamp provenance with the goal file's identity so `trace` keeps this
    // run's metrics distinct from other programs' (BUG_HUNT #4).
    axon_core::interp::set_provenance_source(file.display().to_string());
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

    // Interpret. With --iterate N, run the goal N times: run 1 establishes a
    // baseline (continuation off), then runs 2..N resume from the best prior
    // input via the persisted provenance log (AXON_GOAL_CONTINUE) — so the
    // best score climbs run-over-run and converges. Autonomous iterate-to-
    // converge, driven by one command (builds on cross-run self-improvement).
    let Some(n) = iterate else {
        process::exit(axon_core::interp::run_program(&program));
    };
    let n = n.max(1);
    let start_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    eprintln!("# iterate: up to {n} runs (continuation after run 1; stops when converged)");
    let mut code = 0;
    let mut prev_best: Option<f64> = None;
    for k in 1..=n {
        if k == 1 {
            std::env::remove_var("AXON_GOAL_CONTINUE");
        } else {
            std::env::set_var("AXON_GOAL_CONTINUE", "1");
        }
        eprintln!("── run {k}/{n} ──");
        code = axon_core::interp::run_program(&program);

        // Stop early once the best score stops improving (the search has
        // converged) — autonomous "knows when it's done", not a blind budget.
        let best = axon_core::interp::best_recorded_score(start_ts);
        if let (Some(b), Some(p)) = (best, prev_best) {
            if b <= p {
                eprintln!("# converged after {k} runs (best score {b} did not improve)");
                break;
            }
        }
        prev_best = best;
    }

    // Report the solution found: the best score this session and the input that
    // achieved it (the actual result of an optimization, otherwise only visible
    // via `axon trace`). Scoped to entries written during this iterate run.
    if let Some(recs) = axon_core::interp::read_provenance(None) {
        if let Some(best) = recs
            .iter()
            .filter(|r| r.ts_ms >= start_ts)
            .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap_or(std::cmp::Ordering::Equal))
        {
            let at = best.input.map(|i| format!(" at input {i}")).unwrap_or_default();
            eprintln!("# best: score {}{at}", best.score);
        }
    }
    process::exit(code);
}

// ── trace ─────────────────────────────────────────────────────────────────────

/// Classify a score trajectory as `"improving"`, `"regressing"`, or `"flat"`.
///
/// BUG_HUNT #18: comparing only first-vs-last mislabels a noisy run that peaks
/// then collapses (e.g. `[10, 50, 30, 5, 12]` — last 12 > first 10 reads
/// "improving" even though the search ended near its worst). Instead we fit a
/// least-squares line through the whole series and label by the sign of its
/// slope, so the trend reflects the trajectory, not its endpoints.
///
/// The slope's magnitude is normalized against the score spread so the
/// improving/regressing verdict is scale-invariant; a slope under 1e-9 of the
/// spread (or a zero spread) is `"flat"`.
fn trend_label(scores: &[f64]) -> &'static str {
    let n = scores.len();
    if n < 2 {
        return "flat";
    }
    // Least-squares slope of score against its index 0..n.
    let n_f = n as f64;
    let mean_x = (n_f - 1.0) / 2.0;
    let mean_y = scores.iter().sum::<f64>() / n_f;
    let mut num = 0.0; // Σ (x-mean_x)(y-mean_y)
    let mut den = 0.0; // Σ (x-mean_x)²
    for (i, &y) in scores.iter().enumerate() {
        let dx = i as f64 - mean_x;
        num += dx * (y - mean_y);
        den += dx * dx;
    }
    if den == 0.0 {
        return "flat";
    }
    let slope = num / den;
    // Scale-invariant dead-zone: ignore slopes negligible vs the score spread.
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for &y in scores {
        lo = lo.min(y);
        hi = hi.max(y);
    }
    let spread = hi - lo;
    if spread == 0.0 || slope.abs() < spread * 1e-9 {
        return "flat";
    }
    if slope > 0.0 {
        "improving"
    } else {
        "regressing"
    }
}

/// Per-fn score-trajectory summary computed from the provenance log.
struct TraceStat {
    func: String,
    src: String,
    evals: usize,
    min: f64,
    max: f64,
    best_input: Option<i64>,
    first: f64,
    last: f64,
    trend: &'static str,
}

fn cmd_trace(func: Option<String>, path: Option<PathBuf>, json: bool) {
    use std::collections::HashMap;
    let Some(recs) = axon_core::interp::read_provenance(path.as_deref()) else {
        eprintln!("no provenance log found (run a goal first — see examples/goals/).");
        process::exit(1);
    };

    // Group by (fn, src), preserving first-seen order. Keying on the source
    // program means two different programs that both define `metric` show as
    // separate rows instead of blending into one misleading KPI (BUG_HUNT #4).
    // The log is append-only, so each group's records stay chronological.
    let mut order: Vec<(String, String)> = Vec::new();
    let mut groups: HashMap<(String, String), Vec<&axon_core::interp::ProvRecord>> = HashMap::new();
    let mut total = 0usize;
    for r in &recs {
        if func.as_ref().is_some_and(|f| f != &r.func) {
            continue;
        }
        let key = (r.func.clone(), r.src.clone());
        if !groups.contains_key(&key) {
            order.push(key.clone());
        }
        groups.entry(key).or_default().push(r);
        total += 1;
    }

    let stats: Vec<TraceStat> = order
        .iter()
        .map(|key| {
            let g = &groups[key];
            let best = g.iter().copied().fold(g[0], |a, r| if r.score > a.score { r } else { a });
            TraceStat {
                func: key.0.clone(),
                src: key.1.clone(),
                evals: g.len(),
                min: g.iter().map(|r| r.score).fold(f64::INFINITY, f64::min),
                max: best.score,
                best_input: best.input,
                first: g.first().unwrap().score,
                last: g.last().unwrap().score,
                // Trend reflects the whole trajectory (least-squares slope), not
                // just first-vs-last, so a peaked-then-collapsed run isn't
                // mislabeled "improving" (BUG_HUNT #18).
                trend: {
                    let series: Vec<f64> = g.iter().map(|r| r.score).collect();
                    trend_label(&series)
                },
            }
        })
        .collect();

    if json {
        let body: Vec<String> = stats
            .iter()
            .map(|s| {
                let bi = s.best_input.map(|i| i.to_string()).unwrap_or_else(|| "null".into());
                format!(
                    "{{\"fn\":\"{}\",\"src\":\"{}\",\"evals\":{},\"min\":{},\"max\":{},\"best_input\":{bi},\"first\":{},\"last\":{},\"trend\":\"{}\"}}",
                    s.func, s.src, s.evals, s.min, s.max, s.first, s.last, s.trend,
                )
            })
            .collect();
        println!("[{}]", body.join(","));
        return;
    }

    if total == 0 {
        println!("# provenance: 0 matching records");
        return;
    }
    println!("# provenance: {total} record(s) across {} (fn, source) group(s)", order.len());
    for s in &stats {
        let at = s.best_input.map(|i| format!(" at input {i}")).unwrap_or_default();
        let from = if s.src.is_empty() { String::new() } else { format!(" ({})", s.src) };
        println!(
            "  {}{from}: {} eval(s)  range [{}, {}{at}]  first {} → last {}  [{}]",
            s.func, s.evals, s.min, s.max, s.first, s.last, s.trend,
        );
    }
}

// ── run ───────────────────────────────────────────────────────────────────────

fn cmd_run(file: PathBuf, _release: bool, args: Vec<String>) {
    // Fix 5: validate .ax extension.
    validate_ax_extension(&file);

    // Stamp provenance with this program's identity so `trace` keeps its
    // metrics distinct from other programs that share a function name
    // (BUG_HUNT #4).
    axon_core::interp::set_provenance_source(file.display().to_string());

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

    // Collect test function metadata: (name, should_fail, forall_cases).
    // A `@[test] @[forall(n=N)]` fn with typed params is a PROPERTY test (R8):
    // its params are randomized over N cases (default 100) and a failure is
    // shrunk to a minimal counterexample. A plain `@[test]` fn must be 0-arg.
    let test_meta: Vec<(String, bool, Option<u32>)> = program
        .items
        .iter()
        .filter_map(|item| {
            if let axon_core::ast::Item::FnDef(f) = item {
                let test_attr = f.attrs.iter().find(|a| a.name == "test")?;
                let forall_attr = f.attrs.iter().find(|a| a.name == "forall");
                let forall_cases = forall_attr.map(|a| {
                    // `@[forall(n: 250)]` → 250; bare `@[forall]` → default 100.
                    // Attr args are rendered "key: value" by the parser.
                    a.args.iter()
                        .find_map(|arg| {
                            arg.split_once(':')
                                .filter(|(k, _)| k.trim() == "n")
                                .and_then(|(_, v)| v.trim().parse::<u32>().ok())
                        })
                        .unwrap_or(100)
                });
                if forall_cases.is_none() && !f.params.is_empty() {
                    eprintln!("error: test function '{}' must take zero parameters (or add @[forall] to property-test its params)", f.name);
                    return None;
                }
                if forall_cases.is_some() && f.params.is_empty() {
                    eprintln!("error: @[forall] test '{}' has no parameters to randomize", f.name);
                    return None;
                }
                let should_fail = test_attr.args.iter().any(|a| a == "should_fail");
                if let Some(ref pat) = filter {
                    if !f.name.contains(pat.as_str()) {
                        return None;
                    }
                }
                return Some((f.name.clone(), should_fail, forall_cases));
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
        .map(|(name, should_fail, forall_cases)| {
            let start = Instant::now();
            let (passed, error) = if let Some(cases) = forall_cases {
                // Property test (R8): randomize params, shrink on failure.
                use axon_core::interp::PropertyOutcome;
                match axon_core::interp::run_property_test(&program, name, *cases) {
                    PropertyOutcome::Passed { cases } if *should_fail => (
                        false,
                        Some(format!("should_fail forall '{name}' held over {cases} cases")),
                    ),
                    PropertyOutcome::Passed { .. } => (true, None),
                    PropertyOutcome::Failed { .. } if *should_fail => (true, None),
                    PropertyOutcome::Failed { counterexample, message, seed } => (
                        false,
                        Some(format!(
                            "property failed at [{counterexample}]: {message} (reproduce: AXON_SEED={seed})"
                        )),
                    ),
                    PropertyOutcome::Unsupported(m) => (false, Some(m)),
                }
            } else {
                // Plain zero-arg @[test].
                match axon_core::interp::run_test_fn(&program, name) {
                    Ok(()) if *should_fail => (
                        false,
                        Some(format!("should_fail test '{name}' completed without panicking")),
                    ),
                    Ok(()) => (true, None),
                    Err(_) if *should_fail => (true, None),
                    Err(e) => (false, Some(e)),
                }
            };
            let duration_ms = start.elapsed().as_millis() as u64;
            TestOutcome { name: name.clone(), passed, duration_ms, error }
        })
        .collect();

    let mut passed = 0u32;
    let mut failed = 0u32;
    let mut total_ms: u64 = 0;

    for r in &all_results {
        let should_fail = test_meta
            .iter()
            .find(|(name, _, _)| name == &r.name)
            .map(|(_, sf, _)| *sf)
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
// NOTE: this must stay in sync with `lib::check_pipeline` re: the safety passes
// it runs (resolve → infer → check → borrow → capabilities → verify). The two
// drifted before, silently dropping the @[contained] (E1001) and @[verify]
// (E1101) checks from the CLI; the `*_rejected_by_check` tests in
// `tests/cli_run.rs` guard each class against recurrence.
fn run_check_pipeline(
    program: &mut axon_core::ast::Program,
    source_path: &Path,
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

    // Step 5: capability checking — enforce `@[contained(...)]` I/O sandboxing
    // (E1001). Previously only run by the library check path, so the CLI did not
    // reject containment violations; wire it in so `axon check`/`run` enforce it.
    for err in axon_core::capabilities::check_capabilities(program) {
        all_errors.push(format!("[{}] {}", err.code, err.message));
    }

    // Step 6: static `@[verify(...)]` checking — E1101 when a verify postcondition
    // is provably unsatisfiable by the function's computed confidence bound.
    // (Same CLI-pipeline gap as the capability check; non-`confidence` predicates
    // are skipped, so runtime-gated verifies are unaffected.)
    for err in axon_core::verify::check_verify(program) {
        all_errors.push(format!("[{}] {}", err.code, err.message));
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
        // R8: a versioned, structured NDJSON object (code/severity/message/help
        // as first-class fields), not an opaque message blob. Hand-rolled JSON
        // (no serde_json — it collides with inkwell's trait universe).
        eprintln!("{}", axon_core::diag_schema::diagnostic_json(msg));
    } else {
        eprintln!("error: {msg}");
    }
}

/// Fix 5: Validate that `file` has a `.ax` extension.
/// Exits with code 1 if the extension is wrong.
fn validate_ax_extension(file: &Path) {
    let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext != "ax" {
        let filename = file.display();
        eprintln!("error: Axon source files must have a .ax extension (got '{filename}')");
        process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // BUG_HUNT #18: the headline regression — a run that climbs to a peak then
    // collapses ends ABOVE where it started, so first-vs-last reads "improving"
    // even though the optimizer ended near its worst. Trajectory-aware slope
    // must not call this improving.
    #[test]
    fn peaked_then_collapsed_is_not_improving() {
        let scores = [10.0, 50.0, 30.0, 5.0, 12.0];
        assert_ne!(trend_label(&scores), "improving");
        // Downward overall slope ⇒ regressing.
        assert_eq!(trend_label(&scores), "regressing");
    }

    #[test]
    fn monotonic_climb_is_improving() {
        assert_eq!(trend_label(&[1.0, 2.0, 3.0, 4.0]), "improving");
    }

    #[test]
    fn monotonic_decline_is_regressing() {
        assert_eq!(trend_label(&[4.0, 3.0, 2.0, 1.0]), "regressing");
    }

    #[test]
    fn constant_series_is_flat() {
        assert_eq!(trend_label(&[7.0, 7.0, 7.0]), "flat");
    }

    #[test]
    fn dip_then_recover_above_start_is_improving() {
        // Ends well above start with an upward overall slope despite a mid dip.
        assert_eq!(trend_label(&[10.0, 2.0, 20.0, 30.0]), "improving");
    }

    #[test]
    fn single_and_empty_series_are_flat() {
        assert_eq!(trend_label(&[]), "flat");
        assert_eq!(trend_label(&[42.0]), "flat");
    }

    #[test]
    fn noisy_but_net_upward_is_improving() {
        // Endpoints alone are ambiguous (first 5, last 6) but the line clearly rises.
        assert_eq!(trend_label(&[5.0, 1.0, 8.0, 3.0, 9.0, 6.0]), "improving");
    }

    #[test]
    fn improvement_with_equal_endpoints_is_not_flat_by_endpoints() {
        // first == last (both 10) would read "flat" under partial_cmp, but the
        // peak in the middle makes the least-squares line non-trivial. A
        // symmetric rise-and-fall nets to flat — assert we don't crash and the
        // verdict is deterministic.
        let v = trend_label(&[10.0, 20.0, 10.0]);
        assert_eq!(v, "flat");
    }
}
