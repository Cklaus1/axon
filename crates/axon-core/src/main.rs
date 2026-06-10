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

    /// Statically prove each `@[verify]` bound via Z3 (R9). Requires the `smt`
    /// feature; without it, prints a notice and exits 0.
    Verify {
        #[arg(help = "Path to .ax source file")]
        file: PathBuf,
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
        #[arg(
            long,
            value_name = "N",
            help = "Iterate the goal up to N times, stopping early when converged",
            long_help = "Run the goal up to N times in one command. Run 1 establishes a \
                baseline (continuation off); runs 2..N set AXON_GOAL_CONTINUE=1 so each \
                resumes the optimizer from the best prior input (via the persisted \
                provenance log), letting the best score climb run-over-run. Stops EARLY \
                the first time the best score fails to improve over the previous run \
                (autonomous \"converged\", printed as `# converged after K runs`) — so N \
                is a ceiling, not a fixed count. This is the cross-run self-improvement \
                loop; it is unrelated to the 7-day cap on scheduled `/loop` cron jobs."
        )]
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

        /// Summarize the AI-call audit trail (ai_complete calls: model, mode,
        /// cost) instead of the @[adaptive] score trajectory.
        #[arg(long, help = "Summarize the ai_complete audit trail (model/mode/cost)")]
        ai: bool,
    },

    /// Report the description-length (MDL) complexity of a .ax file.
    ///
    /// A deterministic, format-invariant measure over the typed AST — the bits
    /// it takes to describe the program — per function and whole-program, with a
    /// per-kind cost breakdown. The "measure of simplest program" a compression
    /// loop minimizes (the world-model / `goal { minimize complexity }` pattern).
    /// Exit 0 on success, 2 on a parse error.
    Complexity {
        #[arg(help = "Path to .ax source file")]
        file: PathBuf,

        /// Emit the report as stable JSON (`axon-complexity/1`) for tools/agents.
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

        /// Use AI-driven discovery: an AI selects which pre-verified templates
        /// apply (from the closed registry — it never authors code). Deterministic
        /// under AXON_AI_MOCK=1. Without this flag, runs the static detector.
        #[arg(long, help = "AI-driven discovery (selects templates from the closed registry; mockable)")]
        ai: bool,
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

        /// The candidate pass to verify. `identity` (default) is the baseline;
        /// any name in the closed template registry (`fold-arith-identities`,
        /// `constant-fold`, …) is a discovered optimization.
        #[arg(long, value_name = "PASS", help = "Pass to verify: identity | <registry template, e.g. constant-fold>")]
        pass: Option<String>,
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

        /// Discovery origin to record (audit): `static` (default), `mock`, or
        /// `ai:<model>`. Lets a reviewer see whether an AI proposed this pass.
        #[arg(long = "proposed-by", value_name = "ORIGIN", help = "Discovery origin for the audit trail (e.g. ai:claude-opus-4-8)")]
        proposed_by: Option<String>,
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
        Command::Verify { file } => cmd_verify(file),
        Command::Target { action } => cmd_target(action),
        Command::Test { files, filter, jobs, json } => cmd_test(files, filter, jobs, json),
        Command::Trace { func, path, json, ai } => {
            if ai {
                cmd_trace_ai(func, path, json)
            } else {
                cmd_trace(func, path, json)
            }
        }
        Command::Complexity { file, json } => cmd_complexity(file, json),
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

// ── complexity ──────────────────────────────────────────────────────────────

/// `axon complexity <file> [--json]` — report the AST description-length (MDL)
/// metric per function and for the whole program. Syntactic only (no type-check),
/// so it works on in-progress code. Exit 2 on a parse error.
fn cmd_complexity(file: PathBuf, json: bool) {
    use axon_core::complexity::program_complexity;
    validate_ax_extension(&file);
    let src = read_source(&file);
    let program = match parse_source(&src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    };
    let pc = program_complexity(&program);
    let path = file.display().to_string();

    if json {
        // Hand-built stable JSON (`axon-complexity/1`) — no serde (never link
        // serde+codegen). A future goal/agent loop parses this.
        let esc = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let mut out = String::new();
        out.push_str("{\"schema\":\"axon-complexity/1\",\"file\":\"");
        out.push_str(&esc(&path));
        out.push_str("\",\"total\":");
        out.push_str(&format!(
            "{{\"bits\":{},\"nodes\":{},\"depth\":{}}}",
            pc.total.bits, pc.total.nodes, pc.total.depth
        ));
        out.push_str(",\"functions\":[");
        for (i, (name, c)) in pc.functions.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!(
                "{{\"name\":\"{}\",\"bits\":{},\"nodes\":{},\"depth\":{}}}",
                esc(name), c.bits, c.nodes, c.depth
            ));
        }
        out.push_str("],\"by_kind\":{");
        for (i, (kind, bits)) in pc.by_kind.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push_str(&format!("\"{}\":{}", esc(kind), bits));
        }
        out.push_str("}}");
        println!("{out}");
        return;
    }

    // Human table.
    println!("{path}");
    println!("  {:<28} {:>10} {:>8} {:>6}", "function", "bits", "nodes", "depth");
    println!("  {:-<28} {:->10} {:->8} {:->6}", "", "", "", "");
    for (name, c) in &pc.functions {
        let shown = if name.chars().count() > 28 {
            format!("{}…", &name.chars().take(27).collect::<String>())
        } else {
            name.clone()
        };
        println!("  {:<28} {:>10} {:>8} {:>6}", shown, c.bits, c.nodes, c.depth);
    }
    println!("  {:-<28} {:->10} {:->8} {:->6}", "", "", "", "");
    println!(
        "  {:<28} {:>10} {:>8} {:>6}",
        "TOTAL", pc.total.bits, pc.total.nodes, pc.total.depth
    );
    if !pc.by_kind.is_empty() {
        let top: Vec<String> = pc
            .by_kind
            .iter()
            .take(3)
            .map(|(k, b)| format!("{k} {b}"))
            .collect();
        println!("  top cost by kind: {}", top.join(", "));
    }
}

// ── check ─────────────────────────────────────────────────────────────────────

fn cmd_check(file: PathBuf, json_flag: bool, locked: bool) {
    // Fix 5: validate .ax extension.
    validate_ax_extension(&file);

    let src = read_source(&file);

    // Parse first. R8: use the LOCATED parse so a parse error resolves to a
    // line:col (previously span-less). The byte offset → (line,col) via the
    // SourceMap, emitted as a structured PipelineDiagnostic JSON.
    let use_json_early = json_flag || !std::io::stderr().is_terminal();
    let mut program = match axon_core::parse_source_located(&src) {
        Ok(p) => p,
        Err((msg, offset)) => {
            let (line, col) = axon_core::span::SourceMap::new(src.clone()).line_col(offset);
            let diag = axon_core::PipelineDiagnostic {
                // Parse errors use the E0000 catch-all (same as lib::check_pipeline).
                code: "E0000".to_string(),
                message: msg,
                file: file.display().to_string(),
                line: line as u32,
                col: col as u32,
                severity: "error".to_string(),
                caret: String::new(),
                expected: None,
                found: None,
                help: None,
            };
            if use_json_early {
                eprintln!("{}", diag.json());
            } else {
                eprintln!("error: {}", diag.display());
            }
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
    // Transitive: the import-edge cap check + --locked verification cover the
    // whole `use` closure, so a deeply-nested import can't slip a capability or
    // a tampered byte past the gate (R6).
    let (resolved_imports, _unresolved) = axon_core::resolve_use_files_transitive(&program, &search_dirs);
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

    // Type-check pipeline. R8 typed end-to-end: use the LOCATED form so the
    // JSON a tool/agent consumes carries file/line/col (resolved from each
    // typed diagnostic's byte-span against the source), not just code+message.
    let (located, _infer_ctx) = run_check_pipeline_located(&mut program, &src, &file);
    // Import-cap (E1203) and lock (E1201/E1202/W1210) errors are file-level
    // strings with no span — they keep the string emit path.
    let mut string_errors = import_cap_errors;
    string_errors.extend(lock_errors);

    if located.is_empty() && string_errors.is_empty() {
        // Print nothing on success (Unix convention).
        process::exit(0);
    }

    for d in &located {
        if use_json {
            eprintln!("{}", d.json());
        } else {
            eprintln!("error: {}", d.display());
        }
    }
    for err in &string_errors {
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
    // Transitive closure: lock/verify-lock must pin EVERY module that joins the
    // program (A uses B uses C → all three), not just the direct edge (R6).
    let (resolved, unresolved) = axon_core::resolve_use_files_transitive(&program, &search_dirs);
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

    // R6 §4.3: audit each module's capability surface at acquisition time and
    // pin the verdict into the lockfile. The verdict is deterministic in the
    // bytes, so it is re-validated cheaply by hash on every build — no AI call
    // at compile time. A `denied` verdict (undeclared net) blocks the build
    // (E1204) unless re-locked with `--accept-flagged` once that lands.
    let entries: Vec<LockEntry> = resolved
        .iter()
        .map(|m| {
            let hash = module_hash(&m.bytes);
            let mut e = LockEntry::new(
                m.name.clone(),
                hash.clone(),
                format!("file:{}", m.path.display()),
            );
            // Audit the parsed module; an unparseable module gets no verdict
            // (its hash still pins the bytes; the parse error surfaces elsewhere).
            let verdict = std::str::from_utf8(&m.bytes)
                .ok()
                .and_then(|s| axon_core::parse_source(s).ok())
                .map(|p| axon_core::audit::audit_module(&p))
                .unwrap_or(axon_core::audit::Verdict::Clear);
            // Pin as `<verdict>:<hash-tail>` so a re-hash that changes the bytes
            // also invalidates the verdict (the verdict is about *these* bytes).
            e.audit = format!("{}:{}", verdict.as_str(), hash);
            e
        })
        .collect();

    // Surface non-clear verdicts to the user at lock time (informational —
    // `denied` becomes fatal only at build via E1204).
    for (m, e) in resolved.iter().zip(entries.iter()) {
        if !e.audit.starts_with("clear:") {
            let verdict = e.audit.split(':').next().unwrap_or("");
            eprintln!("axon: audit `{}` → {verdict}", m.name);
        }
    }

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
            Some(e) if e.hash == found => {
                // R6 §4.3: hash matches; re-validate the pinned audit verdict by
                // hash (no AI call). A `denied:<hash>` verdict whose hash still
                // matches these bytes fails the build with E1204 — un-audited
                // exfiltration surface never executes. Always enforced (not just
                // under --locked): once a module is locked with a denied verdict,
                // running it is a hard error regardless of mode.
                let (verdict, vhash) = e.audit.split_once(':').unwrap_or(("", ""));
                if verdict == "denied" && vhash == found {
                    errors.push(format!(
                        "[{}] import `{}` was denied by capability audit: {} — see `axon audit {}` or re-lock with `--accept-flagged` after review",
                        axon_core::error::E1204,
                        m.name,
                        std::str::from_utf8(&m.bytes)
                            .ok()
                            .and_then(|s| axon_core::parse_source(s).ok())
                            .map(|p| axon_core::audit::verdict_reason(&p, axon_core::audit::Verdict::Denied))
                            .unwrap_or_else(|| "undeclared network surface".to_string()),
                        m.name,
                    ));
                }
            }
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
            // The G1 oracle proves a pass is behavior-preserving by comparing
            // program OUTPUT — which is only meaningful for DETERMINISTIC, pure-
            // compute programs. Exclude any member that does I/O (fs/net/exec: its
            // output depends on external mutable state — e.g. file_roundtrip.ax
            // read/writes a shared /tmp file that RACES under parallel verification,
            // making G1 falsely reject a valid pass) or uses a non-deterministic
            // builtin (clock/RNG/host input). This is the "pure-compute" contract
            // this fn's doc-comment already promises, now enforced.
            let pure = axon_core::capabilities::program_capabilities(&program).is_empty()
                && !["now_ms", "random_i64", "random_f64", "host_await", "read_line"]
                    .iter()
                    .any(|b| src.contains(b));
            if has_main && pure {
                let name = f.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
                corpus.push((name, bytes, program));
            }
        }
    }
    corpus
}

/// R10 AI-driven discovery (the `--ai` path). Builds the AI proposer, runs
/// `discover_with_ai` (which validates every selection against the closed
/// registry — unknown name → E1407), and writes one proposal per accepted
/// template, stamped with its discovery origin (`proposed_by`) for the audit
/// trail. Deterministic under `AXON_AI_MOCK=1`.
fn cmd_improve_discover_ai(programs: &[axon_core::ast::Program], dir: &Path) {
    use axon_core::improve::{discover_with_ai, DiscoveryMode};

    // The proposer seam. The AI's output is validated identically against the
    // closed registry no matter its source, so a live (or compromised) model
    // gains nothing — it can only NAME a template; an unknown name is E1407, and
    // a known one still runs the full four-gate firewall before it can graduate.
    //
    //   - AXON_AI_MOCK   → deterministic stub (reproducible CI/tests).
    //   - live (asi-runtime + ANTHROPIC_API_KEY) → ask the model to pick from the
    //     menu via the ai_discovery_prompt; the model id is recorded for audit.
    type Proposer = Box<dyn Fn(&str) -> String>;
    let mock = axon_core::interp::ai_mock_enabled();
    let (mode, proposer): (DiscoveryMode, Proposer) = if mock {
        (
            DiscoveryMode::Mock,
            // Stub: offer the menu; the static cross-check in discover_with_ai
            // drops a template with no real sites, so this can't fabricate one.
            Box::new(|_prompt: &str| axon_core::improve_templates::template_names().join("\n")),
        )
    } else {
        // Live: route the menu+digest prompt to the model. Its raw reply is
        // parsed for candidate template names and validated against the registry.
        #[cfg(feature = "asi-runtime")]
        {
            let model = axon_core::ai_routing::Tier::Balanced.api_model();
            (
                DiscoveryMode::Ai { model: model.clone() },
                Box::new(move |prompt: &str| {
                    match axon_ai::complete_with_model(prompt, &model) {
                        Ok(reply) => reply,
                        // A failed call proposes nothing (empty reply → no candidates).
                        Err(e) => { eprintln!("axon improve discover --ai: model call failed: {e}"); String::new() }
                    }
                }),
            )
        }
        #[cfg(not(feature = "asi-runtime"))]
        {
            eprintln!(
                "axon improve discover --ai: live AI discovery needs the `asi-runtime` \
                 feature + ANTHROPIC_API_KEY — or run with AXON_AI_MOCK=1 for the \
                 deterministic path"
            );
            process::exit(2);
        }
    };

    let proposals = match discover_with_ai(programs, proposer.as_ref(), mode) {
        Ok(p) => p,
        Err(e) => {
            // E1407: the (mock/live) AI named a template outside the registry.
            eprintln!("axon improve discover --ai: [{}] {}", e.code, e.message);
            process::exit(2);
        }
    };

    if proposals.is_empty() {
        println!(
            "axon improve discover --ai: no applicable templates in {} — nothing to propose",
            dir.display()
        );
        process::exit(0);
    }

    let pdir = PathBuf::from("proposals");
    if let Err(e) = std::fs::create_dir_all(&pdir) {
        eprintln!("axon improve discover --ai: cannot create proposals/: {e}");
        process::exit(1);
    }
    for prop in &proposals {
        // Deterministic filename (no timestamp — red-team advisory): one
        // proposal per template name, overwrite semantics like the static path.
        let ppath = pdir.join(format!("{}.proposal", prop.template));
        let body = format!(
            "# axon improve proposal (UNPRIVILEGED — grants nothing)\n\
             name = {}\n\
             description = {}\n\
             opportunities = {}\n\
             members_affected = {}\n\
             corpus = {}\n\
             proposed_by = {}:{}\n\
             reasoning = {}\n\
             # Next: `axon improve verify --pass {}` runs the four gates; only a\n\
             # multi-sig `axon improve graduate {} --proposed-by {}:{}` adds it.\n",
            prop.template,
            axon_core::improve_templates::get_template(&prop.template)
                .map(|t| t.description).unwrap_or(""),
            prop.opportunities,
            prop.members_affected,
            dir.display(),
            prop.mode.mode_str(), prop.mode.model_str(),
            prop.reasoning,
            prop.template,
            prop.template, prop.mode.mode_str(), prop.mode.model_str(),
        );
        if let Err(e) = std::fs::write(&ppath, &body) {
            eprintln!("axon improve discover --ai: cannot write {}: {e}", ppath.display());
            process::exit(1);
        }
        println!(
            "axon improve discover --ai: proposed `{}` ({} site(s) across {} member(s)) [origin: {}:{}]",
            prop.template, prop.opportunities, prop.members_affected,
            prop.mode.mode_str(), prop.mode.model_str(),
        );
        println!("  wrote {} — a proposal grants nothing; run `axon improve verify --pass {}` next", ppath.display(), prop.template);
    }
    process::exit(0);
}

fn cmd_improve(action: ImproveAction) {
    use axon_core::improve::{verify_pass_with, PerfStatus, VerifyOptions};
    use axon_core::manifest::{
        corpus_hash, graduate, pass_hash, parse_manifest, verify_hash, write_manifest, Manifest,
        PassEntry, PerfClaim,
    };

    match action {
        ImproveAction::Discover { corpus, ai } => {
            // Discovery is the UNPRIVILEGED proposal side (R10 §3/§4): scan the
            // corpus for a candidate optimization and WRITE a proposal. It grants
            // nothing — only `verify` (four gates) then a multi-sig `graduate`
            // can turn a proposal into a runnable pass. This slice detects the
            // canonical safe optimization (arithmetic-identity simplification).
            let dir = corpus.unwrap_or_else(|| PathBuf::from("examples"));
            let members = load_corpus(&dir);
            if members.is_empty() {
                eprintln!("axon improve discover: no runnable .ax programs in {}", dir.display());
                process::exit(2);
            }
            let programs: Vec<axon_core::ast::Program> =
                members.iter().map(|(_, _, p)| p.clone()).collect();

            if ai {
                cmd_improve_discover_ai(&programs, &dir);
                return;
            }

            match axon_core::improve::discover_arith_identities(&programs) {
                Some(prop) => {
                    // Write the proposal (pure data — NOT executable). The
                    // proposals/ dir is the unprivileged staging area `verify`
                    // reads; nothing here is added to the pass manifest.
                    let pdir = PathBuf::from("proposals");
                    if let Err(e) = std::fs::create_dir_all(&pdir) {
                        eprintln!("axon improve discover: cannot create proposals/: {e}");
                        process::exit(1);
                    }
                    let ppath = pdir.join(format!("{}.proposal", prop.name));
                    let body = format!(
                        "# axon improve proposal (UNPRIVILEGED — grants nothing)\n\
                         name = {}\n\
                         description = {}\n\
                         opportunities = {}\n\
                         members_affected = {}\n\
                         corpus = {}\n\
                         # Next: `axon improve verify` runs the four gates; only a\n\
                         # multi-sig `axon improve graduate` adds it to the manifest.\n",
                        prop.name, prop.description, prop.opportunities,
                        prop.members_affected, dir.display(),
                    );
                    if let Err(e) = std::fs::write(&ppath, &body) {
                        eprintln!("axon improve discover: cannot write {}: {e}", ppath.display());
                        process::exit(1);
                    }
                    println!(
                        "axon improve discover: proposed `{}` — {} ({} site(s) across {} member(s))",
                        prop.name, prop.description, prop.opportunities, prop.members_affected
                    );
                    println!("  wrote {} (a proposal grants nothing — run `axon improve verify` next)", ppath.display());
                    process::exit(0);
                }
                None => {
                    println!(
                        "axon improve discover: no arithmetic-identity opportunities in {} — nothing to propose",
                        dir.display()
                    );
                    process::exit(0);
                }
            }
        }

        ImproveAction::Verify { corpus, perf, pass } => {
            // Validate the pass name against the closed registry FIRST — before
            // any corpus I/O. `identity` (default) is the G1/G3 baseline; every
            // other name MUST resolve in `improve_templates::TEMPLATES` (the
            // single source of truth — red-team must-fix #1/#4: pass lookup is
            // by name in TEMPLATES, never dynamic/file-based). An unknown name →
            // E1407, fail-closed, so it never reaches the verifier with an
            // undefined pass (and the error is the same regardless of corpus).
            let pass_name = pass.unwrap_or_else(|| "identity".to_string());
            let identity: &axon_core::improve::Pass = &|p: &axon_core::ast::Program| p.clone();
            let the_pass: &axon_core::improve::Pass = if pass_name == "identity" {
                identity
            } else {
                match axon_core::improve_templates::get_pass_for_template(&pass_name) {
                    Some(p) => p,
                    None => {
                        eprintln!(
                            "axon improve verify: [{}] unknown pass `{pass_name}` — not in the \
                             template registry (known: identity, {:?})",
                            axon_core::error::E1407,
                            axon_core::improve_templates::template_names(),
                        );
                        process::exit(2);
                    }
                }
            };

            let dir = corpus.unwrap_or_else(|| PathBuf::from("examples"));
            let members = load_corpus(&dir);
            if members.is_empty() {
                eprintln!("axon improve verify: no runnable .ax programs in {}", dir.display());
                process::exit(2);
            }
            let programs: Vec<axon_core::ast::Program> =
                members.iter().map(|(_, _, p)| p.clone()).collect();
            println!("axon improve verify — pass: {pass_name}");
            let opts = VerifyOptions { measure_perf: perf, perf_trials: 5 };
            let rec = verify_pass_with(the_pass, &programs, &opts);

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

        ImproveAction::Graduate { name, signers, corpus, manifest, proposed_by } => {
            // BUG_HUNT / red-team must-fix #4: a graduated pass name MUST resolve
            // in the closed template registry (or be the `identity` baseline) —
            // a name absent from the registry is E1408 (manifest tampering /
            // version skew), refused before it can enter the manifest. Pass code
            // is ALWAYS looked up by name in TEMPLATES, never loaded dynamically.
            if name != "identity" && !axon_core::improve_templates::is_known_template(&name) {
                eprintln!(
                    "axon improve graduate: [{}] pass `{name}` is not in the template registry \
                     (known: {:?}) — refusing to graduate a name with no reviewed Rust pass \
                     (possible tampering or version skew)",
                    axon_core::error::E1408,
                    axon_core::improve_templates::template_names(),
                );
                process::exit(2);
            }
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
            let origin = proposed_by.unwrap_or_else(|| "static".to_string());
            let entry: PassEntry = match graduate(
                id,
                &name,
                verified,
                ch,
                &signers,
                PerfClaim::Unmeasured,
                origin,
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

// ── verify (R9 SMT static proof) ─────────────────────────────────────────────────

/// `axon verify <file>` — statically prove each `@[verify]` bound via Z3.
/// With the `smt` feature: encodes each annotated fn and reports proven /
/// counterexample (E1102) / unsupported (W1103). Without it: a clear notice,
/// exit 0 (the binary still works everywhere; the proof is opt-in).
#[cfg(feature = "smt")]
fn cmd_verify(file: PathBuf) {
    use axon_core::smt::{prove_verify_bounds, ProofResult};
    validate_ax_extension(&file);
    let src = read_source(&file);
    let program = match parse_source(&src) {
        Ok(p) => p,
        Err(e) => {
            emit_error(&format!("{e}"), !std::io::stderr().is_terminal());
            process::exit(2);
        }
    };
    let results = prove_verify_bounds(&program);
    // Phase 5 §4: collect refinement predicates so refinement-RETURN obligations
    // are proved too (not just @[verify]).
    let mut refinements: std::collections::HashMap<String, axon_core::ast::Expr> =
        std::collections::HashMap::new();
    for item in &program.items {
        if let axon_core::ast::Item::RefineDef(r) = item {
            refinements.insert(r.name.clone(), (*r.predicate).clone());
        }
    }
    if results.is_empty() && refinements.is_empty() {
        println!("axon verify: no @[verify] functions or refinement returns to prove");
        process::exit(0);
    }
    let mut any_violation = false;
    for r in &results {
        match r {
            ProofResult::Proven { function } => {
                println!("  ✓ proven: `{function}` satisfies its @[verify] bound for all inputs");
            }
            ProofResult::Counterexample { function, inputs, predicate } => {
                any_violation = true;
                let args: Vec<String> = inputs.iter().map(|(n, v)| format!("{n}={v}")).collect();
                emit_error(
                    &format!(
                        "[{}] @[verify] bound `{predicate}` is violated for `{function}` at {} (SMT counterexample)",
                        axon_core::error::E1102,
                        args.join(", ")
                    ),
                    !std::io::stderr().is_terminal(),
                );
            }
            ProofResult::Unsupported { function, reason } => {
                eprintln!(
                    "  warning: [{}] @[verify] on `{function}` not statically provable — {reason}; runtime gate still applies",
                    axon_core::error::W1103
                );
            }
        }
    }

    // Phase 5 §4: also statically prove refinement-RETURN obligations — a fn
    // whose return type is a named refinement returns a value satisfying the
    // predicate for ALL inputs (the non-constant proof the checker defers).
    if !refinements.is_empty() {
        for r in axon_core::smt::prove_refinement_returns(&program, &refinements) {
            match r {
                ProofResult::Proven { function } => {
                    println!("  ✓ proven: `{function}` returns a value satisfying its refinement for all inputs");
                }
                ProofResult::Counterexample { function, inputs, predicate } => {
                    any_violation = true;
                    let args: Vec<String> = inputs.iter().map(|(n, v)| format!("{n}={v}")).collect();
                    emit_error(
                        &format!(
                            "[{}] `{function}`'s {predicate} is violated at {} (SMT counterexample)",
                            axon_core::error::E1102,
                            args.join(", ")
                        ),
                        !std::io::stderr().is_terminal(),
                    );
                }
                ProofResult::Unsupported { function, reason } => {
                    eprintln!(
                        "  warning: [{}] refinement return on `{function}` not statically provable — {reason}",
                        axon_core::error::W1103
                    );
                }
            }
        }

        // Phase 5 §1.5: refinement subtyping under argument forwarding — when a
        // fn forwards its own refinement-typed param as a refinement argument,
        // prove the caller's predicate implies the callee's (the variable-arg
        // obligation the constant checker defers).
        for r in axon_core::smt::prove_refinement_arg_forwarding(&program, &refinements) {
            match r {
                ProofResult::Proven { function } => {
                    println!("  ✓ proven: `{function}` forwards an argument that satisfies the callee's refinement");
                }
                ProofResult::Counterexample { function, inputs, predicate } => {
                    any_violation = true;
                    let args: Vec<String> = inputs.iter().map(|(n, v)| format!("{n}={v}")).collect();
                    emit_error(
                        &format!(
                            "[{}] at `{function}` the {predicate}, but the caller's refinement admits {} (SMT counterexample)",
                            axon_core::error::E1102,
                            args.join(", ")
                        ),
                        !std::io::stderr().is_terminal(),
                    );
                }
                ProofResult::Unsupported { .. } => { /* forwarding outside the fragment: runtime gate applies */ }
            }
        }
    }

    process::exit(if any_violation { 2 } else { 0 });
}

#[cfg(not(feature = "smt"))]
fn cmd_verify(file: PathBuf) {
    validate_ax_extension(&file);
    eprintln!(
        "axon verify: SMT static proof of @[verify] bounds requires the `smt` feature.\n  \
         Build with: cargo build -p axon-core --no-default-features --features smt --bin axon\n  \
         (The runtime @[verify] gate still applies when you `axon run` the program.)"
    );
    process::exit(0);
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
                // R7 Slice B (AOT wasm, object half): emit a real wasm object via
                // the inkwell wasm32 backend. The runnable-.wasm link needs a wasm
                // sysroot + wasm-ld (deferred). When codegen is unavailable this
                // falls back to the E0907 honest block.
                build_wasm_object_cli(&file, triple);
                process::exit(0);
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

// ── R7 Slice B: AOT wasm object emission ─────────────────────────────────────

/// `axon target build --engine codegen --target wasm32 <file>` — without
/// codegen, the honest E0907 block (R7 §6).
#[cfg(not(feature = "codegen"))]
fn build_wasm_object_cli(file: &Path, triple: &str) {
    emit_error(
        &format!(
            "[{}] AOT wasm build needs the native codegen backend (this axon was built \
             without it) — use `axon target build --engine interp --target {triple} {}` \
             to run via the interpreter",
            axon_core::error::E0907,
            file.display()
        ),
        !std::io::stderr().is_terminal(),
    );
    process::exit(2);
}

/// `axon target build --engine codegen --target wasm32 <file>` — emit a real
/// WebAssembly object via the inkwell wasm32 backend (R7 §3.2 Slice B, object
/// half). Parses → checks → monomorphizes → emits a `.wasm` object next to the
/// source, then verifies the wasm magic. The runnable-`.wasm` link (wasm
/// sysroot + wasm-ld) is the documented remaining gap.
#[cfg(feature = "codegen")]
fn build_wasm_object_cli(file: &Path, triple: &str) {
    validate_ax_extension(file);
    // Normalise the alias `wasm32` to a concrete LLVM triple.
    let llvm_triple = if triple == "wasm32" { "wasm32-unknown-unknown" } else { triple };

    let src = read_source(&file.to_path_buf());
    let mut program = match parse_source(&src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    };

    // Type-check before codegen (same gate as native build).
    let (errors, mut infer_ctx) = run_check_pipeline(&mut program, file);
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("error: {e}");
        }
        process::exit(2);
    }

    // Monomorphize, then emit the wasm object.
    let instantiations = infer_ctx.drain_instantiations();
    let mono = axon_core::mono::monomorphise(&program, instantiations);
    let concrete = axon_core::ast::Program {
        items: mono.other_items.into_iter()
            .chain(mono.fns.into_iter().map(axon_core::ast::Item::FnDef))
            .collect(),
    };

    let ctx = inkwell::context::Context::create();
    let module_name = file.file_stem().unwrap_or_default().to_string_lossy();
    let mut cg = axon_core::codegen::Codegen::new(&ctx, &module_name);
    // R7: wasm32 is ILP32 — malloc/free/realloc take an i32 size. Tell codegen
    // before emission so the runtime decls + call sites use the right width
    // (otherwise array allocation traps: `type mismatch: expected i32, found i64`).
    cg.set_target_is_wasm(llvm_triple.starts_with("wasm32"));
    cg.declare_functions(&concrete);
    cg.emit_program(&concrete);

    // Abort if emission recorded hard errors (e.g. a builtin with no native
    // lowering — E0910). Better an honest failure than a wrong wasm object.
    if !cg.codegen_errors().is_empty() {
        for e in cg.codegen_errors() {
            eprintln!("{e}");
        }
        eprintln!("error: {} codegen error(s); wasm build aborted", cg.codegen_errors().len());
        process::exit(2);
    }

    let out = file.with_extension("wasm");
    let out_str = out.to_string_lossy().to_string();
    match cg.compile_to_wasm_object(&out_str, false, llvm_triple) {
        Ok(()) => {
            // Verify the emitted file is genuine wasm (magic `\0asm`).
            let magic_ok = std::fs::read(&out)
                .map(|b| b.len() >= 4 && &b[0..4] == b"\0asm")
                .unwrap_or(false);
            if !magic_ok {
                eprintln!("error: emitted {} is not a valid wasm object (bad magic)", out.display());
                process::exit(2);
            }
            // R7: try to LINK the object into a runnable `.wasm`. After
            // dead-function pruning, a pure-integer program has no `__axon_*`
            // imports, so it links cleanly against the wasi libc in reactor mode
            // (`--no-entry --export=main`) and runs under `wasmtime --invoke main`.
            // Programs that use str/array still pull i64-ABI helpers that clash
            // with wasm32's i32 libc — those report the link error honestly (the
            // i64→i32 ABI retarget is the remaining gap).
            match try_link_wasm(&out, llvm_triple) {
                Some(linked) => eprintln!(
                    "wasm: {} (target {llvm_triple}) — IR→wasm emitted, linked, RUNNABLE.\n  \
                     run:  wasmtime --invoke main {}",
                    linked.display(), linked.display()
                ),
                None => eprintln!(
                    "wasm object: {} (target {llvm_triple}) — IR→wasm emitted + magic-verified. \
                     Link to a runnable .wasm needs `rust-lld` + a wasi libc (pure-int programs \
                     link cleanly; str/array programs await the i64→i32 ABI retarget, R7 §12).",
                    out.display()
                ),
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// R7: link a wasm object into a runnable `.wasm` via `rust-lld -flavor wasm`
/// against the wasi libc, in reactor mode (`--no-entry --export=main`). Returns
/// the linked path on success, or `None` when the toolchain is absent OR the
/// link fails (e.g. an str/array program whose i64-ABI helpers clash with
/// wasm32's i32 libc — the remaining ABI gap). Best-effort: a failed link is
/// not an error, just "object-only".
/// Locate the wasm32 build of `libaxon_rt.a`, the runtime carrying the
/// scalar-ABI bridge for str/array externs. Checks `$AXON_WASM_RT` first (an
/// explicit override), then the conventional cargo target layout. Returns None
/// if the wasm runtime hasn't been built — callers degrade to libc-only linking.
#[cfg(feature = "codegen")]
fn find_wasm_axon_rt(target_subdir: &str) -> Option<PathBuf> {
    // AXON_WASM_RT overrides only for the (default) wasip1 path — a browser link
    // needs the unknown-unknown build specifically, so don't let a wasip1 override
    // leak into it.
    if target_subdir == "wasm32-wasip1" {
        if let Ok(p) = std::env::var("AXON_WASM_RT") {
            let pb = PathBuf::from(p);
            if pb.exists() {
                return Some(pb);
            }
        }
    }
    // Walk up from CWD looking for target/<subdir>/{debug,release}/libaxon_rt.a.
    let mut dir = std::env::current_dir().ok()?;
    loop {
        for profile in ["debug", "release"] {
            let cand = dir
                .join("target")
                .join(target_subdir)
                .join(profile)
                .join("libaxon_rt.a");
            if cand.exists() {
                return Some(cand);
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

#[cfg(feature = "codegen")]
fn try_link_wasm(obj: &Path, triple: &str) -> Option<PathBuf> {
    use std::process::Command;
    // wasm32-unknown-unknown is the BROWSER target (R7c): the result must be
    // wasi-FREE (a browser has no wasi). We link the unknown-unknown axon-rt and
    // NO wasi libc. wasm32-wasip1 is the headless/server target: wasi libc +
    // wasip1 axon-rt, runnable under wasmtime.
    let is_browser = triple.contains("unknown-unknown");
    // Locate rust-lld + the wasi libc under the active rustup toolchain.
    let home = std::env::var("HOME").ok()?;
    let toolchains = PathBuf::from(&home).join(".rustup/toolchains");
    let find = |needle: &str, host_seg: bool| -> Option<PathBuf> {
        // Shallow scan: look for the first matching path under any toolchain.
        for entry in std::fs::read_dir(&toolchains).ok()?.flatten() {
            let base = entry.path();
            let cand = if host_seg {
                // rust-lld lives under lib/rustlib/<host>/bin/
                base.join("lib/rustlib")
            } else {
                base.join("lib/rustlib/wasm32-wasip1/lib/self-contained").join(needle)
            };
            if host_seg {
                // walk the host dir for rust-lld
                if let Ok(rd) = std::fs::read_dir(&cand) {
                    for h in rd.flatten() {
                        let lld = h.path().join("bin/rust-lld");
                        if lld.exists() { return Some(lld); }
                    }
                }
            } else if cand.exists() {
                return Some(cand);
            }
        }
        None
    };
    let rust_lld = find("", true)?;

    // Locate the wasm build of axon-rt (the `__axon_*` runtime, carrying the
    // scalar-ABI bridge under `#[cfg(target_arch="wasm32")]`). The BROWSER target
    // uses the unknown-unknown build (no wasi); the headless target uses wasip1.
    // A program referencing a runtime symbol whose rt isn't built surfaces an
    // unresolved-symbol failure and falls back to object-only (None), honestly.
    let (rt_subdir, link_wasi_libc) = if is_browser {
        ("wasm32-unknown-unknown", false)
    } else {
        ("wasm32-wasip1", true)
    };
    let rt_lib = find_wasm_axon_rt(rt_subdir);

    let linked = obj.with_extension("linked.wasm");
    let mut cmd = Command::new(&rust_lld);
    cmd.args(["-flavor", "wasm", "--no-entry", "--export=main"]);
    if is_browser {
        // The JS/wasm-bindgen glue supplies host imports (`axon_host_write` for
        // println). Allow EXACTLY those to stay undefined → wasm imports. Anything
        // else undefined (e.g. snprintf/malloc for number formatting, not yet
        // shimmed for the browser) stays a hard error → honest object-only.
        // Write the allow-list to a temp path (not next to the object) so the
        // source tree isn't littered. Content is constant, so concurrent builds
        // writing it are harmless.
        let allow = std::env::temp_dir().join("axon-browser-allow-undef.txt");
        if std::fs::write(&allow, "axon_host_write\n").is_ok() {
            cmd.arg(format!("--allow-undefined-file={}", allow.display()));
        }
    }
    if link_wasi_libc {
        // Headless wasip1: the wasi libc provides malloc/memcpy + fd_write etc.
        let libc = find("libc.a", false)?;
        cmd.arg(libc);
    }
    if let Some(rt) = &rt_lib {
        cmd.arg(rt);
    }
    let status = cmd
        .arg(obj)
        .arg("-o")
        .arg(&linked)
        .output()
        .ok()?;
    // rust-lld can succeed-with-warnings; treat a produced + non-empty file as
    // the success signal, but reject if there were signature mismatches (the
    // wasm would trap), so we only claim "runnable" when it genuinely is.
    let stderr = String::from_utf8_lossy(&status.stderr);
    if !status.status.success() || stderr.contains("function signature mismatch") {
        let _ = std::fs::remove_file(&linked);
        return None;
    }
    if linked.exists() && std::fs::metadata(&linked).map(|m| m.len() > 0).unwrap_or(false) {
        Some(linked)
    } else {
        None
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

/// Per-(fn, src) summary of the AI-call audit trail (`axon trace --ai`).
struct AiTraceStat {
    func: String,
    src: String,
    calls: usize,
    cost_usd: f64,
    tier: String,
    model: String,
    goal: String,
    live: usize,
    mock: usize,
    replay: usize,
    fallback: usize,
}

/// `axon trace --ai`: summarize the `ai_complete` audit trail from the provenance
/// log — who called which routed model, in what mode (live/mock/replay/fallback),
/// and the metered cost. The viewing half of the auditability story (the recording
/// half is the provenance log + AXON_AI_REPLAY).
fn cmd_trace_ai(func: Option<String>, path: Option<PathBuf>, json: bool) {
    use std::collections::HashMap;
    let Some(recs) = axon_core::interp::read_ai_calls(path.as_deref()) else {
        eprintln!("no provenance log found (run a program that calls ai_complete first).");
        process::exit(1);
    };
    let mut order: Vec<(String, String)> = Vec::new();
    let mut groups: HashMap<(String, String), AiTraceStat> = HashMap::new();
    let mut total = 0usize;
    let mut total_cost = 0.0f64;
    let (mut t_live, mut t_mock, mut t_replay, mut t_fallback) = (0usize, 0usize, 0usize, 0usize);
    for r in &recs {
        if func.as_ref().is_some_and(|f| f != &r.func) {
            continue;
        }
        let key = (r.func.clone(), r.src.clone());
        if !groups.contains_key(&key) {
            order.push(key.clone());
            groups.insert(
                key.clone(),
                AiTraceStat {
                    func: r.func.clone(),
                    src: r.src.clone(),
                    calls: 0,
                    cost_usd: 0.0,
                    tier: r.tier.clone(),
                    model: r.model.clone(),
                    goal: r.goal.clone(),
                    live: 0,
                    mock: 0,
                    replay: 0,
                    fallback: 0,
                },
            );
        }
        let g = groups.get_mut(&key).unwrap();
        g.calls += 1;
        g.cost_usd += r.cost_usd;
        if !r.tier.is_empty() {
            g.tier = r.tier.clone();
        }
        if !r.model.is_empty() {
            g.model = r.model.clone();
        }
        if !r.goal.is_empty() {
            g.goal = r.goal.clone();
        }
        match r.mode.as_str() {
            "live" => { g.live += 1; t_live += 1; }
            "mock" => { g.mock += 1; t_mock += 1; }
            "replay" => { g.replay += 1; t_replay += 1; }
            "fallback" => { g.fallback += 1; t_fallback += 1; }
            _ => {}
        }
        total += 1;
        total_cost += r.cost_usd;
    }

    if json {
        let body: Vec<String> = order
            .iter()
            .map(|k| {
                let s = &groups[k];
                format!(
                    "{{\"fn\":\"{}\",\"src\":\"{}\",\"calls\":{},\"cost_usd\":{},\"tier\":\"{}\",\"model\":\"{}\",\"goal\":\"{}\",\"live\":{},\"mock\":{},\"replay\":{},\"fallback\":{}}}",
                    s.func, s.src, s.calls, s.cost_usd, s.tier, s.model, s.goal, s.live, s.mock, s.replay, s.fallback,
                )
            })
            .collect();
        println!(
            "{{\"schema\":\"axon-ai-audit/1\",\"calls\":{total},\"cost_usd\":{total_cost},\"modes\":{{\"live\":{t_live},\"mock\":{t_mock},\"replay\":{t_replay},\"fallback\":{t_fallback}}},\"by_fn\":[{}]}}",
            body.join(","),
        );
        return;
    }

    if total == 0 {
        println!("# ai-audit: 0 ai_complete call(s) logged");
        return;
    }
    println!(
        "# ai-audit: {total} ai_complete call(s), ${total_cost:.6} total  (live {t_live}  mock {t_mock}  replay {t_replay}  fallback {t_fallback})",
    );
    for k in &order {
        let s = &groups[k];
        let from = if s.src.is_empty() { String::new() } else { format!(" ({})", s.src) };
        let goal = if s.goal.is_empty() { String::new() } else { format!("  → goal `{}`", s.goal) };
        println!(
            "  {}{from}: {} call(s)  ${:.6}  [{} {}]  live:{} mock:{} replay:{} fallback:{}{goal}",
            s.func, s.calls, s.cost_usd, s.tier, s.model, s.live, s.mock, s.replay, s.fallback,
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

    // Phase 5 §4: when built with the `smt` feature, statically discharge the
    // refinement-return / scalar-`@[verify]` obligations Z3 can prove ∀-inputs,
    // and run with those checks elided. Without the feature this is an empty set
    // (no behaviour change) — the runtime gate enforces everything as before.
    // R15: a program that suspends via `host_await` runs under the stdin/stdout
    // host driver, so `axon run` of an interactive program (a prompt loop) reads
    // user input. (The `contains` check is a cheap gate; a non-host_await program
    // mentioning the name in a comment would just take the equivalent worker-thread
    // path — same result.) Otherwise the normal, deep-stack run path.
    let code = if src.contains("host_await") {
        axon_core::interp::run_suspendable_stdio(&program)
    } else {
        let discharged = compute_discharged(&program);
        axon_core::interp::run_program_with_discharged(&program, discharged)
    };
    process::exit(code);
}

/// Phase 5 §4: assemble the refinement predicate map and ask the SMT backend to
/// discharge every obligation it can prove for all inputs. Behind `#[cfg(smt)]`;
/// the non-smt build returns an empty set so the default pipeline is unchanged.
#[cfg(feature = "smt")]
fn compute_discharged(program: &axon_core::ast::Program) -> axon_core::verify::Discharged {
    let mut refinements: std::collections::HashMap<String, axon_core::ast::Expr> =
        std::collections::HashMap::new();
    for item in &program.items {
        if let axon_core::ast::Item::RefineDef(r) = item {
            refinements.insert(r.name.clone(), (*r.predicate).clone());
        }
    }
    let d = axon_core::smt::discharge(program, &refinements);
    if d.total() > 0 {
        eprintln!(
            "axon: SMT discharged {} runtime obligation(s) statically (checks elided)",
            d.total()
        );
    }
    d
}

#[cfg(not(feature = "smt"))]
fn compute_discharged(_program: &axon_core::ast::Program) -> axon_core::verify::Discharged {
    axon_core::verify::Discharged::default()
}

// ── fmt ───────────────────────────────────────────────────────────────────────

/// Exit codes for `axon fmt`:
///   0 — success (file formatted in-place, or --check and already correct)
///   1 — --check: at least one file would be reformatted
///   2 — parse error in input file (file not touched)
/// Does `src` contain a `//` line comment or `/* */` block comment outside of a
/// string literal? The AST-based formatter discards comments (the lexer skips
/// `//…`), so formatting a file with comments would SILENTLY DELETE them. We
/// detect that up front and refuse, rather than destroy documentation. A simple
/// scanner that tracks whether we're inside a `"…"` string (honoring `\` escapes)
/// — good enough to avoid false positives on `"http://…"` and the like.
fn source_has_comments(src: &str) -> bool {
    let b = src.as_bytes();
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2; // skip the escaped char
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'/' && i + 1 < b.len() && (b[i + 1] == b'/' || b[i + 1] == b'*') {
            return true;
        }
        i += 1;
    }
    false
}

fn cmd_fmt(files: Vec<PathBuf>, check: bool) {
    if files.is_empty() {
        eprintln!("error: no source files specified");
        process::exit(1);
    }
    for f in &files {
        validate_ax_extension(f);
    }

    let mut any_would_change = false;
    let mut any_error = false;

    // Process EVERY file independently — a parse error or comment-refusal on one
    // must not silently skip the files after it (the classic "stopped halfway,
    // user thinks it's done" trap). Errors are collected and reported per file;
    // the command exits non-zero at the end if any occurred.
    for file in &files {
        let src = read_source(file);
        let program = match parse_source(&src) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("error: {}: {e}", file.display());
                any_error = true;
                continue;
            }
        };

        // The formatter is a pure AST pretty-printer — comments aren't in the
        // AST, so reformatting would erase them. Refuse rather than silently
        // delete documentation. (Comment-preserving formatting is future work;
        // see spec/compiler-phase4.md.)
        if source_has_comments(&src) {
            eprintln!(
                "error: {}: refusing to format — the file contains comments, which the \
                 AST-based formatter would delete. (Comment-preserving formatting is not yet \
                 implemented; the file is unchanged.)",
                file.display()
            );
            any_error = true;
            continue;
        }

        let formatted = axon_core::format_program(&program);

        if check {
            if formatted != src {
                eprintln!("{}: would reformat", file.display());
                any_would_change = true;
            }
        } else if formatted != src {
            if let Err(e) = std::fs::write(file, &formatted) {
                eprintln!("error writing {}: {e}", file.display());
                any_error = true;
                continue;
            }
            eprintln!("formatted: {}", file.display());
        }
    }

    // A refusal/parse/write error is the strongest signal (exit 2); otherwise
    // `--check` reports drift with exit 1.
    if any_error {
        process::exit(2);
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
        // Multi-file: document EACH file with its OWN source so the `///` doc
        // comments are extracted (they're read by per-item byte-offset, which
        // is only valid against that file's source). Merging into one program
        // and passing an empty source dropped every doc comment — and with no
        // docs, "*No documented items.*". Concatenate the per-file sections
        // under one H1 instead. (`generate_docs` emits its own H1 per call; we
        // demote those to H2 file headers under a single project H1.)
        let _ = &title; // single-file title is unused on this path
        let mut combined = format!("# API documentation ({} files)\n", files.len());
        for ((filename, program), path) in file_programs.iter().zip(&files) {
            let src = read_source(path);
            let per_file = axon_core::generate_docs(program, &src, filename);
            // Drop the per-file H1 line (`# <filename>`); the project H1 is above.
            // Keep the body, under an H2 file heading.
            let body = per_file.split_once('\n').map(|(_, rest)| rest).unwrap_or("").trim();
            combined.push_str(&format!("\n## {filename}\n\n{body}\n"));
        }
        emit_doc_output(combined, out.as_deref());
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
///
/// Back-compat string view over [`run_check_pipeline_located`]: each located
/// diagnostic is rendered `[CODE] message` (message already folds in any
/// expected/found detail). Callers that only need to count/print strings use
/// this; the `--json` path (R8 typed end-to-end) consumes the located form so
/// `file`/`line`/`col` survive to the consumer.
fn run_check_pipeline(
    program: &mut axon_core::ast::Program,
    source_path: &Path,
) -> (Vec<String>, axon_core::infer::InferCtx) {
    // Source text is only needed to resolve spans → (line,col); the string view
    // doesn't carry locations, so an empty SourceMap (dummy spans → line 0) is
    // fine here. The JSON callers pass the real source via the `_src` variant.
    let (diags, ctx) = run_check_pipeline_located(program, "", source_path);
    let strings = diags
        .iter()
        .map(|d| format!("[{}] {}", d.code, d.message))
        .collect();
    (strings, ctx)
}

/// R8 typed end-to-end: run the pipeline and return **located** diagnostics
/// (`code`/`message`/`file`/`line`/`col`), resolving each typed error's
/// byte-offset span against `src` via [`axon_core::span::SourceMap`]. This is
/// the source of truth; [`run_check_pipeline`] is the flattened string view.
///
// NOTE: this must stay in sync with `lib::check_pipeline` re: the safety passes
// it runs (resolve → infer → check → borrow → capabilities → verify). The two
// drifted before, silently dropping the @[contained] (E1001) and @[verify]
// (E1101) checks from the CLI; the `*_rejected_by_check` tests in
// `tests/cli_run.rs` guard each class against recurrence.
fn run_check_pipeline_located(
    program: &mut axon_core::ast::Program,
    src: &str,
    source_path: &Path,
) -> (Vec<axon_core::PipelineDiagnostic>, axon_core::infer::InferCtx) {
    use axon_core::PipelineDiagnostic;
    let file = source_path.display().to_string();
    let source_map = axon_core::span::SourceMap::new(src.to_string());
    let mut diags: Vec<PipelineDiagnostic> = Vec::new();

    // Resolve a span → (line, col); dummy spans (or an empty source) yield 0.
    let loc = |span: &axon_core::span::Span| -> (u32, u32) {
        if span.is_dummy() {
            (0, 0)
        } else {
            let (l, c) = source_map.line_col(span.start);
            (l as u32, c as u32)
        }
    };
    let push = |diags: &mut Vec<PipelineDiagnostic>,
                code: String,
                message: String,
                severity: &str,
                line: u32,
                col: u32| {
        diags.push(PipelineDiagnostic {
            code,
            message,
            file: file.clone(),
            line,
            col,
            severity: severity.to_string(),
            caret: String::new(),
            expected: None,
            found: None,
            help: None,
        });
    };
    // R8 axon-diag/2: like `push` but carrying the structured type-mismatch +
    // fix fields (the infer/check errors that have them), so the JSON exposes
    // `expected`/`found`/`help` as discrete keys, not folded into `message`.
    let push_typed = |diags: &mut Vec<PipelineDiagnostic>,
                      code: String,
                      message: String,
                      line: u32,
                      col: u32,
                      expected: Option<String>,
                      found: Option<String>,
                      help: Option<String>| {
        diags.push(PipelineDiagnostic {
            code,
            message,
            file: file.clone(),
            line,
            col,
            severity: "error".to_string(),
            caret: String::new(),
            expected,
            found,
            help,
        });
    };

    // The original string body, retained verbatim but rewritten to push located
    // diagnostics instead of formatted strings. (Below this point `file` is the
    // same display string the old code computed.)
    let _ = &file;

    // Step 0: load modules referenced by `use` declarations (AXON_PATH search).
    // MergeErrors carry no span (they're file-level), so line/col stay 0.
    let search_dirs = axon_core::axon_search_dirs(std::env::current_exe().ok().as_deref());
    for e in axon_core::load_use_decls(program, &search_dirs) {
        push(&mut diags, e.code.to_string(), e.message.clone(), "error", 0, 0);
    }

    // Step 1: name resolution
    let resolve_result = axon_core::resolver::resolve_program(program, &file);
    for diag in &resolve_result.errors {
        let (line, col) = loc(&diag.span);
        // The resolver computes a "did you mean `x`?" suggestion (Levenshtein ≤ 3)
        // and stores it in `diag.fix`. Render it through the structured `help`
        // field instead of dropping it — historically `push` discarded `fix`, so
        // infer.rs re-emitted an E0101 "cannot find value … did you mean" purely
        // to resurface the lost hint, double-reporting every undefined name.
        push_typed(&mut diags, diag.code.to_string(), diag.message.clone(),
            line, col, None, None, diag.fix.clone());
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
        let mut msg = err.message.clone();
        if let Some(exp) = &err.expected {
            msg.push_str(&format!(" (expected {exp})"));
        }
        if let Some(fnd) = &err.found {
            msg.push_str(&format!(", found {fnd}"));
        }
        let (line, col) = loc(&err.span);
        // R8: also expose expected/found as discrete fields (InferError has no fix).
        push_typed(&mut diags, err.code.to_string(), msg, line, col,
            err.expected.clone(), err.found.clone(), None);
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
        let mut msg = err.message.clone();
        if let Some(exp) = &err.expected {
            msg.push_str(&format!(" (expected {exp})"));
        }
        if let Some(fnd) = &err.found {
            msg.push_str(&format!(", found {fnd}"));
        }
        // A check-phase WARNING (e.g. W0701 uncertainty-discarded, W0004
        // unreachable-arm) must NOT join the error set — that would fail `check`
        // with exit 2. Print it like the resolver warnings above and move on;
        // only genuine errors accumulate in `diags` (which drives the exit code).
        if matches!(err.severity, axon_core::checker::Severity::Warning) {
            eprintln!("warning: [{}] {msg}", err.code);
            continue;
        }
        // CheckError tracks both a byte-span and legacy line/col; prefer the
        // span (real offset), fall back to the explicit line/col when no span.
        let (line, col) = if !err.span.is_dummy() {
            loc(&err.span)
        } else {
            (err.line, err.col)
        };
        // R8: expose expected/found/fix(help) as discrete structured fields.
        push_typed(&mut diags, err.code.to_string(), msg, line, col,
            err.expected.clone(), err.found.clone(), err.fix.clone());
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
                    let (line, col) = loc(&err.span());
                    let code = match &err {
                        axon_core::borrow::BorrowError::UseAfterMove { .. } => axon_core::error::E0601,
                        axon_core::borrow::BorrowError::MoveBorrowed { .. } => axon_core::error::E0602,
                        axon_core::borrow::BorrowError::BorrowConflict { .. } => axon_core::error::E0603,
                    };
                    push(&mut diags, code.to_string(), err.to_string(), "error", line, col);
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
                        let (line, col) = loc(&err.span());
                        let code = match &err {
                            axon_core::borrow::BorrowError::UseAfterMove { .. } => axon_core::error::E0601,
                            axon_core::borrow::BorrowError::MoveBorrowed { .. } => axon_core::error::E0602,
                            axon_core::borrow::BorrowError::BorrowConflict { .. } => axon_core::error::E0603,
                        };
                        push(&mut diags, code.to_string(), err.to_string(), "error", line, col);
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
        let (line, col) = loc(&err.span);
        push(&mut diags, err.code.to_string(), err.message.clone(), "error", line, col);
    }

    // Step 5b: Phase 6 effect-row subsumption (E1310) — a call performs an
    // effect outside the enclosing fn's declared row. `main` without a clause is
    // the top-level escape hatch, so existing programs are unaffected.
    for err in axon_core::effects::check_effects(program) {
        let (line, col) = loc(&err.span);
        push(&mut diags, err.code.to_string(), err.message.clone(), "error", line, col);
    }

    // Step 6: static `@[verify(...)]` checking — E1101 when a verify postcondition
    // is provably unsatisfiable by the function's computed confidence bound.
    // (Same CLI-pipeline gap as the capability check; non-`confidence` predicates
    // are skipped, so runtime-gated verifies are unaffected.)
    for err in axon_core::verify::check_verify(program) {
        let (line, col) = loc(&err.span);
        push(&mut diags, err.code.to_string(), err.message.clone(), "error", line, col);
    }

    // Collapse byte-identical diagnostics. Some checks fire per-operand: `"a" +
    // "b"` runs check_numeric_operand on BOTH operands, each producing an E0102
    // with the same code/message/line/col — the user sees the SAME line twice
    // with no way to tell them apart. A diagnostic identical in every rendered
    // field to one already emitted is pure noise, so drop it (order-preserving).
    // Genuinely distinct cases survive: `"a" + true` keeps its two lines (the
    // `str` and `bool` messages differ), as does any pair with distinct spans.
    {
        let mut seen = std::collections::HashSet::new();
        diags.retain(|d| {
            seen.insert((
                d.severity.clone(),
                d.code.clone(),
                d.message.clone(),
                d.file.clone(),
                d.line,
                d.col,
            ))
        });
    }

    (diags, infer_ctx)
}

/// Compile the program to a native binary at `output`. Native AOT path —
/// only compiled with the `codegen` feature.
#[cfg(feature = "codegen")]
fn run_build_pipeline(
    program: &mut axon_core::ast::Program,
    source_path: &Path,
    output: &Path,
    opts: &BuildOptions,
) -> Result<(), String> {
    // Check first, fail fast on errors.
    let (errors, mut infer_ctx) = run_check_pipeline(program, source_path);
    if !errors.is_empty() {
        // Print each diagnostic, not just the count — otherwise `axon build` on a
        // program with a type/name error showed only "N error(s); build aborted",
        // forcing the user to re-run `axon check` to see WHAT was wrong.
        for e in &errors {
            eprintln!("error: {e}");
        }
        return Err(format!("{} error(s); build aborted", errors.len()));
    }

    // Cache key MUST include the git SHA, not just the semver: two builds of the
    // compiler at the same 0.1.0 version but different commits emit different IR
    // (e.g. the #36 random_i64 guard), and keying on semver alone served a stale
    // pre-fix binary on rebuild — a silent-wrong-artifact footgun. VERSION is
    // `<semver> (<git-sha>)`, captured by build.rs; a dirty tree appends nothing
    // here, so a `--no-cache` build is still the escape hatch mid-edit.
    let compiler_version = VERSION;
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
    source_path: &std::path::Path,
    output: &std::path::Path,
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
    // R4: stamp the source path into native @[adaptive] provenance (`"src"`).
    cg.set_source_path(source_path.display().to_string());
    // Phase 5 §4: elide the runtime refinement-return / scalar-`@[verify]` checks
    // the SMT prover discharged ∀-inputs (empty set without the `smt` feature, so
    // native output is unchanged). Run on the monomorphized program so the proven
    // fn names match the names codegen emits.
    let discharged = compute_discharged(&concrete_program);
    if discharged.total() > 0 {
        eprintln!(
            "axon: SMT discharged {} runtime obligation(s) statically (native checks elided)",
            discharged.total()
        );
    }
    cg.set_discharged(discharged);
    cg.declare_functions(&concrete_program);
    cg.emit_program(&concrete_program);

    // Abort before linking if emission recorded hard errors (e.g. a known
    // builtin with no native lowering — E0910). Shipping the binary would
    // silently compute a wrong value.
    if !cg.codegen_errors().is_empty() {
        for e in cg.codegen_errors() {
            eprintln!("{e}");
        }
        return Err(format!("{} codegen error(s); build aborted", cg.codegen_errors().len()));
    }

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
