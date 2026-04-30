# Axon Compiler — Phase 4 Spec

**Goal**: Transform Axon from a correct systems language into a complete language ecosystem. Phase 4 adds developer tooling (LSP, formatter, documentation generator), production infrastructure (incremental compilation, cross-compilation), and quality-of-life features (parallel testing, multi-file builds, structured test output) that make Axon usable at scale.  
**Builds on**: Phase 3 (`spec/compiler-phase3.md`)  
**Timeline**: 3-6 months after Phase 3 ships

---

## Phase 4 Scope

### In Phase 4
- LSP server (`axon lsp`) — hover, go-to-definition, live error squiggles
- Formatter (`axon fmt [--check]`) — canonical style from AST
- Documentation generator (`axon doc <file>`) — Markdown from `///` comments
- Incremental compilation / caching — content-hash `.axc` cache files
- Cross-compilation (`axon build --target <triple>`) — multi-target support
- Standard library search path — `AXON_PATH` algorithm
- Multi-file compilation (`axon build src/*.ax`) — global namespace merge
- Parallel test execution (`axon test --jobs N`) — thread-pool test runner
- `axon test --json` — newline-delimited JSON test results

### Explicitly Out of Phase 4
```
Self-modifying compiler passes     → future
Formal verification (@[verify])    → future
Uncertain<T> / Temporal<T>         → future
WASM / JS targets                  → future
Package registry / dependency mgr  → future
```

---

## 1. LSP Server (`axon lsp`)

### Motivation

Without editor integration, developers rely on running `axon check` in a terminal after every save
and mentally mapping error messages to source locations. An LSP server brings diagnostics, type
information, and navigation into the editor in real time, tightening the feedback loop from minutes
to milliseconds. Because Phase 3 already threads spans through the full pipeline and provides a
resolver and type-checker as library code, the LSP is a thin wrapper rather than a second
implementation.

### CLI Interface

```
axon lsp
```

Starts a JSON-RPC 2.0 server on stdin/stdout. No arguments. The server runs until the client
disconnects (stdin closes).

### Capabilities Declared in `initialize` Response

```json
{
  "capabilities": {
    "textDocumentSync": 1,
    "hoverProvider": true,
    "definitionProvider": true,
    "diagnosticProvider": { "interFileDependencies": false, "workspaceDiagnostics": false }
  }
}
```

### Feature: Hover (`textDocument/hover`)

When the cursor rests on an identifier, the LSP returns the type of the expression at that
position.

**Request**:
```json
{
  "method": "textDocument/hover",
  "params": {
    "textDocument": { "uri": "file:///path/to/main.ax" },
    "position": { "line": 4, "character": 12 }
  }
}
```

**Response**:
```json
{
  "contents": { "kind": "markdown", "value": "```axon\nfn greet(name: str) -> str\n```" }
}
```

The LSP resolves the identifier at the given offset by:
1. Running the analysis backend (resolver + type inference + checker) on the current document text.
2. Finding the AST node whose span contains the cursor offset.
3. Looking up the inferred type of that node in the typed AST.
4. Formatting the type (or full function signature when hovering a function name) as Axon source.

### Feature: Go-to-Definition (`textDocument/definition`)

Returns the location of the declaration that the identifier under the cursor refers to.

**Response**:
```json
{
  "uri": "file:///path/to/main.ax",
  "range": { "start": { "line": 1, "character": 3 }, "end": { "line": 1, "character": 8 } }
}
```

Implementation: the resolver already records declaration spans on every symbol table entry
(Phase 3). The LSP looks up the resolved symbol for the hovered identifier and returns its
declaration span converted to LSP `Range` format.

### Feature: Diagnostics (`textDocument/publishDiagnostics`)

On every `textDocument/didChange` or `textDocument/didOpen` notification, the LSP runs the full
analysis pipeline on the new text and sends all diagnostics back:

```json
{
  "method": "textDocument/publishDiagnostics",
  "params": {
    "uri": "file:///path/to/main.ax",
    "diagnostics": [
      {
        "range": { "start": { "line": 11, "character": 4 }, "end": { "line": 11, "character": 7 } },
        "severity": 1,
        "code": "E0301",
        "message": "Option<i32> used as i32 — use unwrap_or or match"
      }
    ]
  }
}
```

Severity mapping: `Error` → 1, `Warning` → 2, `Info` → 3.

### Implementation Approach

The analysis pipeline is exposed as a library function:

```rust
/// Analyse `source` text for the file at `uri`. Returns typed AST and diagnostics.
pub fn analyse(source: &str, uri: &str) -> AnalysisResult;

pub struct AnalysisResult {
    pub program:     Option<TypedProgram>,
    pub diagnostics: Vec<CompileError>,
    pub source_map:  SourceMap,
}
```

The LSP server is a thin event loop:

```
stdin → JSON-RPC dispatcher → handler → analysis backend → JSON-RPC response → stdout
```

Incremental re-parse: on `didChange`, the server re-runs the full pipeline from scratch on the
in-memory document text. Phase 4 does not implement a partial re-parse; the pipeline is fast enough
(< 50 ms for files under 1 000 lines) that full re-analysis is acceptable.

State kept in memory between requests: the last successful `AnalysisResult` per open document URI,
used to answer hover and definition requests without re-running analysis.

### Code Example

```axon
// main.ax — hover over `greet` on line 5 to see: fn greet(name: str) -> str
fn greet(name: str) -> str {
    "hello {name}"
}

fn main() {
    let msg = greet("axon")
    println(msg)
}
```

---

## 2. Formatter (`axon fmt [--check]`)

### Motivation

Inconsistent formatting creates noisy diffs, slows code review, and makes grep-based navigation
unreliable. A canonical formatter removes style debates from code review entirely: there is one
correct way to format every Axon file, enforced automatically. Because Phase 3 adds spans to every
AST node, the formatter can recover the exact source structure and re-emit it in canonical form
without heuristics.

### CLI Interface

```
axon fmt <file.ax>           reformat file in place
axon fmt <file.ax> --check   exit 1 if file is not already formatted, print diff to stderr
axon fmt src/*.ax            reformat all matched files
axon fmt --check src/*.ax    check all matched files
```

**Exit codes**:
```
0    success (file formatted, or --check and file was already correct)
1    --check: file would be reformatted
2    parse error in input file (file not touched)
```

### Formatting Rules

| Rule | Canonical form |
|------|----------------|
| Indent | 4 spaces per level (no tabs) |
| Statements | One statement per line |
| Binary operators | Spaces on both sides (`a + b`, not `a+b`) |
| Trailing whitespace | None |
| Trailing newline | One newline at end of file |
| Opening brace | On the same line as the keyword or signature (`fn f() {`) |
| Closing brace | On its own line at the dedented level |
| Comma lists | Space after each comma, no trailing comma on single-line |
| Blank lines | One blank line between top-level items; none inside function bodies |
| Function signature | `fn name(p1: T1, p2: T2) -> R` — spaces after colons, around `->` |
| Field access | No space around `.` (`p.x`, not `p .x`) |

### Implementation Approach

The formatter is a pretty-printer over the typed AST:

```rust
pub fn format(program: &Program, source: &str) -> String;
```

It is a recursive descent over AST nodes, producing a `String`. Each node emits its canonical
text. Spans from the original source are used only to recover the text of literal values (string
contents, numeric suffixes) — structure comes from the AST, not from the original whitespace.

The `--check` mode computes the formatted output, then diffs it against the original file. If they
differ, it prints the unified diff to stderr and exits 1.

Phase 4 requirement: the formatter must be idempotent — formatting an already-formatted file
produces an identical file.

### Code Example

Input (unformatted):
```axon
fn add(a:i32,b:i32)->i32{a+b}
@[test] fn test_add(){assert_eq(add(2,3),5)}
```

Output (formatted):
```axon
fn add(a: i32, b: i32) -> i32 {
    a + b
}

@[test]
fn test_add() {
    assert_eq(add(2, 3), 5)
}
```

---

## 3. Documentation Generator (`axon doc <file>`)

### Motivation

Documentation that lives separately from source goes stale. Doc comments attached to functions and
types stay co-located with the code they describe and can be verified by the compiler (the
function signature is always correct because it is extracted from the AST). `axon doc` generates
browsable Markdown from `///` doc comments without requiring an external tool.

### CLI Interface

```
axon doc <file.ax>              print Markdown to stdout
axon doc <file.ax> -o out.md   write Markdown to file
axon doc src/*.ax -o docs/      write one .md per .ax file into docs/
```

### Doc Comment Syntax

```axon
/// Compute the absolute value of a 64-bit integer.
///
/// Returns a non-negative value equal in magnitude to `n`.
/// Behaviour is undefined if `n == i64::MIN` (no 64-bit positive representation exists).
fn abs_i64(n: i64) -> i64 {
    if n < 0 { -n } else { n }
}
```

- `///` lines immediately preceding a top-level `fn_def` or `type_def` or `enum_def` are
  collected as a doc comment block.
- Blank `///` lines become blank lines in the output.
- Non-doc comments (`//`) are not included.
- Doc comments inside function bodies are ignored.

### Output Format

For each documented function:

```markdown
## fn_name(param1: Type1, param2: Type2) -> ReturnType

<doc comment text>

---
```

For each documented type:

```markdown
## type TypeName = { field1: T1, field2: T2 }

<doc comment text>

---
```

For each documented enum:

```markdown
## enum EnumName

<doc comment text>

---
```

A file with no doc comments produces a Markdown file with only the file header:

```markdown
# main.ax

*No documented items.*
```

### Implementation Approach

The doc generator runs the lexer and parser only (no type inference required). During parsing,
the parser tracks a running `Vec<String>` of consecutive `///` lines seen before each top-level
item. When a top-level item is parsed, any accumulated doc lines are attached to that item's AST
node.

The generator then iterates over top-level items in declaration order, skipping items without doc
comments, and emits the Markdown header (built from the item signature) followed by the doc text.

### Code Example

```axon
/// A two-dimensional point in Euclidean space.
type Point = { x: f64, y: f64 }

/// Compute the Euclidean distance between two points.
///
/// Returns the square root of the sum of squared differences.
fn distance(a: Point, b: Point) -> f64 {
    let dx = a.x - b.x
    let dy = a.y - b.y
    sqrt(dx * dx + dy * dy)
}
```

Generated output:

```markdown
# points.ax

## type Point = { x: f64, y: f64 }

A two-dimensional point in Euclidean space.

---

## fn distance(a: Point, b: Point) -> f64

Compute the Euclidean distance between two points.

Returns the square root of the sum of squared differences.

---
```

---

## 4. Incremental Compilation / Caching

### Motivation

Recompiling unchanged files wastes time. For large Axon projects with many modules, full
recompilation after touching a single file is unacceptable. A content-hash cache lets the compiler
skip stages that have not changed, bringing rebuild times close to zero for incremental edits.

### Cache Location and Format

Cache files are stored in `~/.cache/axon/<hash>.axc`.

Each `.axc` file is a binary bundle containing:
- A header with the compiler version string and the source hash.
- The LLVM bitcode (`.bc`) for the compiled module.

```
~/.cache/axon/
  <sha256_of_inputs>.axc    (one per unique (source, compiler version) pair)
```

### Hash Computation

The cache key is a SHA-256 digest over:
1. The exact content of the source file (all bytes).
2. The compiler version string (e.g. `"axon 0.4.0"`).

```rust
use sha2::{Sha256, Digest};

fn cache_key(source: &[u8], compiler_version: &str) -> String {
    let mut h = Sha256::new();
    h.update(source);
    h.update(compiler_version.as_bytes());
    format!("{:x}", h.finalize())
}
```

The source content of any `use`-d modules is **not** included in the hash in Phase 4. Cross-module
cache invalidation is deferred; if a depended-upon module changes, recompilation of dependents must
be forced with `--no-cache`.

### Cache Lookup and Miss

On every `axon build`:

1. Compute the cache key for each source file.
2. Check whether `~/.cache/axon/<key>.axc` exists and is readable.
3. **Cache hit**: extract the LLVM bitcode from the `.axc` file; skip the lexer through IR
   emission stages for this file; proceed directly to LLVM optimisation and linking.
4. **Cache miss**: run the full pipeline; after successful IR emission, write the bitcode and
   header to `~/.cache/axon/<key>.axc`.

### CLI Flags

```
axon build <file.ax>            use cache (default)
axon build <file.ax> --no-cache disable cache for this invocation
axon build <file.ax> --cache-dir <path>  use a different cache directory
```

### Cache Eviction

No automatic eviction in Phase 4. Users may clear the cache with:

```
axon cache clean        remove all entries from ~/.cache/axon/
axon cache clean --older-than 30d   remove entries not accessed in 30 days
```

### Code Example (Axon source — cache is transparent to user)

```axon
// lib.ax — first build: full compilation; subsequent builds: cache hit
fn fib(n: i64) -> i64 {
    if n <= 1 { n }
    else { fib(n - 1) + fib(n - 2) }
}
```

```
$ axon build lib.ax          # cache miss — compiles and writes .axc
$ axon build lib.ax          # cache hit  — skips IR emission (~5ms vs ~120ms)
$ touch lib.ax               # file unchanged — mtime irrelevant, hash determines hit
$ axon build lib.ax          # cache hit (content hash unchanged)
$ echo "// comment" >> lib.ax
$ axon build lib.ax          # cache miss (hash changed)
```

---

## 5. Cross-Compilation (`axon build --target <triple>`)

### Motivation

Axon programs must run on servers (Linux x86-64), developer machines (macOS ARM), and Windows
build systems. Cross-compilation from a single host eliminates the need for separate build
machines for each target. LLVM already supports all required targets; Phase 4 exposes the target
triple as a CLI flag and wires it through the `TargetMachine` configuration.

### CLI Interface

```
axon build <file.ax> --target <triple>
axon build <file.ax> --target x86_64-unknown-linux-gnu
axon build <file.ax> --target aarch64-apple-darwin
axon build <file.ax> --target x86_64-pc-windows-msvc
```

### Supported Target Triples (Phase 4)

| Triple | OS | Architecture | Notes |
|--------|----|-------------|-------|
| `x86_64-unknown-linux-gnu` | Linux | x86-64 | Default when no `--target` given on Linux hosts |
| `aarch64-apple-darwin` | macOS | ARM64 | Default on Apple Silicon hosts |
| `x86_64-pc-windows-msvc` | Windows | x86-64 | Requires MSVC linker or LLD |
| `x86_64-apple-darwin` | macOS | x86-64 | For Intel Mac hosts |
| `aarch64-unknown-linux-gnu` | Linux | ARM64 | Embedded / server ARM |

### Implementation Approach

The LLVM `TargetMachine` is constructed with the target triple from `--target`. inkwell exposes:

```rust
let target = Target::from_triple(&TargetTriple::create(triple))?;
let machine = target.create_target_machine(
    &TargetTriple::create(triple),
    "generic",         // CPU
    "",                // features
    OptimizationLevel::Default,
    RelocMode::PIC,
    CodeModel::Default,
)?;
```

The rest of the pipeline (IR emission, optimisation) is target-independent. The target machine is
used only at the final object-file / bitcode emission step.

### Sysroot and Linker Configuration

Cross-compilation requires matching headers and a cross-linker. Phase 4 reads configuration from
`~/.config/axon/cross.toml`:

```toml
[target.aarch64-apple-darwin]
sysroot = "/path/to/macos-sdk"
linker  = "aarch64-apple-darwin-ld"

[target.x86_64-pc-windows-msvc]
sysroot = "/path/to/windows-sdk"
linker  = "lld-link"
```

If `cross.toml` is absent and a non-host target is requested, the compiler emits a warning and
attempts to use the host linker (which will fail for truly cross-compiled targets). The error
message includes the path to create `cross.toml`.

### Code Example

```axon
// hello.ax
fn main() {
    println("hello from any platform")
}
```

```
# Build for Linux from a macOS host:
$ axon build hello.ax --target x86_64-unknown-linux-gnu -o hello-linux

# Build for Windows:
$ axon build hello.ax --target x86_64-pc-windows-msvc -o hello.exe

# Verify target:
$ file hello-linux
hello-linux: ELF 64-bit LSB executable, x86-64
```

---

## 6. Standard Library Search Path

### Motivation

Without a defined search algorithm, every Axon installation places the standard library in a
different location and `use std::io` fails with opaque "module not found" errors. A well-specified
search algorithm makes installations reproducible and lets users override the stdlib for testing or
custom environments.

### Search Algorithm

When the compiler encounters `use std::io`, it searches for the file `std/io.ax` in each directory
from the following ordered list, stopping at the first hit:

1. Each directory in `AXON_PATH` (colon-separated on Unix, semicolon-separated on Windows),
   searched in order.
2. `~/.axon/lib/` — the user's local stdlib installation.
3. The toolchain-relative path: `<dir of axon binary>/../lib/axon/` — the stdlib bundled with
   the compiler.

If none of the directories contain the file, the compiler emits:

```
error[E0003]: module 'std::io' not found
  searched:
    $AXON_PATH entries (none set)
    /home/user/.axon/lib/std/io.ax  (not found)
    /usr/local/lib/axon/std/io.ax   (not found)
  help: install the Axon standard library or set AXON_PATH
```

### Path Segment Mapping

`use a::b::c` maps to the file `a/b/c.ax` relative to each search directory. The `::` separator
in `use` declarations is a path separator; each segment is a directory component except the last,
which is the filename stem.

```
use std::io          → std/io.ax
use std::collections → std/collections.ax
use mylib::utils     → mylib/utils.ax
```

### `AXON_PATH` Example

```bash
export AXON_PATH=/home/user/axon-libs:/opt/axon/vendor

# use std::io resolves to /home/user/axon-libs/std/io.ax (first hit)
```

### Code Example

```axon
use std::io

fn main() {
    let line = io::read_line()
    println("you typed: {line}")
}
```

```
$ AXON_PATH=/home/user/my-axon-libs axon build main.ax
# searches /home/user/my-axon-libs/std/io.ax first
```

---

## 7. Multi-File Compilation (`axon build src/*.ax`)

### Motivation

Real programs span multiple files. Compiling one file at a time prevents cross-file type checking
and forces manual link steps. Multi-file compilation lets the compiler see all source files at
once, merge their namespaces, detect cross-file errors, and produce a single linked binary.

### CLI Interface

```
axon build src/main.ax src/lib.ax src/utils.ax
axon build src/*.ax
axon build src/*.ax -o myapp
```

All `.ax` files listed are compiled together as a single program. The entry point is the `fn main`
found in any of the files (exactly one must exist; zero or more than one is an error).

### Namespace Merge Rules

All top-level items from all source files are merged into a single global namespace before
name resolution:

1. Each file's top-level items (functions, types, enums) are collected in declaration order per
   file; files are processed in the order given on the command line.
2. Duplicate names across files are an error:
   ```
   error[E0002]: 'greet' already defined (first: src/lib.ax:3, redefined: src/utils.ax:7)
   ```
3. Items from all files are visible to all other files without any `use` declaration. (Explicit
   `use` is still required for stdlib modules.)
4. Circular imports (file A uses module B, file B uses module A) are detected during the file
   loading phase and reported as an error before any compilation begins:
   ```
   error: circular import detected: main.ax → lib.ax → main.ax
   ```

### Pipeline Changes

Multi-file compilation runs the pipeline as follows:

```
[file_1.ax, file_2.ax, ...]
  │
  ▼  (parallel)
[Lex + Parse each file]  → [Program_1, Program_2, ...]
  │
  ▼
[Merge top-level items into global Program]
  │
  ▼
[Name Resolution → Type Inference → Checker → Borrow → Monomorphize → Codegen]
  │
  ▼
[Single LLVM module → Optimise → Link → Binary]
```

Lexing and parsing of individual files are done in parallel (one thread per file). The merge and
subsequent stages are single-threaded.

### Code Example

```axon
// math.ax
fn square(n: i64) -> i64 { n * n }

// main.ax
fn main() {
    println(to_str(square(7)))   // 49
}
```

```
$ axon build main.ax math.ax   # or: axon build src/*.ax
```

---

## 8. Parallel Test Execution (`axon test --jobs N`)

### Motivation

Projects with hundreds of tests wait unnecessarily when tests run sequentially. Each `@[test]`
function is independent (no shared mutable state between tests by the borrow checker's guarantees),
so parallel execution is safe and can reduce test suite runtime by `N`x on an `N`-core machine.

### CLI Interface

```
axon test <file.ax>              run tests sequentially (default: --jobs 1)
axon test <file.ax> --jobs 4     run up to 4 tests concurrently
axon test <file.ax> --jobs 0     use num_cpus (auto-detect)
axon test src/*.ax --jobs 0      parallel test across all files
axon test <file.ax> --filter=foo run only tests whose names contain "foo"
```

### Implementation Approach

The test runner collects all `@[test]` function names from the compiled binary, then distributes
them across a thread pool of N workers. Each worker runs a single test in a subprocess:

```
subprocess: axon-test-runner <binary> <test_name>
  → exits 0 on pass, exits 1 on fail/panic
```

A coordinator thread collects results as subprocesses complete and maintains a live progress
display. Results are printed as each test finishes (not buffered to the end).

```
running 8 tests with 4 workers
test test_add              ... ok    (2ms)
test test_sub              ... ok    (1ms)
test test_mul              ... ok    (1ms)
test test_parse_err        ... FAILED (3ms)
  assertion failed: expected Err, got Ok
test test_div              ... ok    (1ms)
...
test result: FAILED. 7 passed, 1 failed, 0 skipped (total 12ms, 4 workers)
```

### `num_cpus` Detection

When `--jobs 0`, the number of workers is determined by:

```rust
fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
}
```

### Code Example

```axon
@[test] fn test_a() { assert_eq(1 + 1, 2) }
@[test] fn test_b() { assert_eq(2 * 3, 6) }
@[test] fn test_c() { assert_eq(10 / 2, 5) }
@[test] fn test_d() { assert_eq(7 % 3, 1) }
```

```
$ axon test math.ax --jobs 0
running 4 tests with 8 workers
test test_a   ... ok  (1ms)
test test_b   ... ok  (1ms)
test test_c   ... ok  (1ms)
test test_d   ... ok  (1ms)
test result: ok. 4 passed, 0 failed (4ms total, 8 workers)
```

---

## 9. JSON Test Output (`axon test --json`)

### Motivation

CI systems, test dashboards, and IDE integrations need machine-readable test results. A
human-readable progress display cannot be reliably parsed. `--json` emits one JSON object per
line (newline-delimited JSON / NDJSON) so consumers can stream results as they arrive.

### CLI Interface

```
axon test <file.ax> --json
axon test <file.ax> --json --jobs 4
```

`--json` and human-readable output are mutually exclusive; `--json` suppresses all other output.

### Output Format

One JSON object per line. Each test result:

```json
{"name":"test_add","status":"ok","duration_ms":2}
{"name":"test_bad","status":"failed","duration_ms":5,"message":"assertion failed: 3 != 4"}
{"name":"test_skip","status":"skipped","duration_ms":0}
```

Field definitions:

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Test function name (without `@[test]` marker) |
| `status` | `"ok"` \| `"failed"` \| `"skipped"` | Test outcome |
| `duration_ms` | integer | Wall-clock time in milliseconds |
| `message` | string (optional) | Failure message; present only when `status == "failed"` |

Final summary line (always last):

```json
{"type":"summary","total":4,"passed":3,"failed":1,"skipped":0,"duration_ms":12}
```

### Integration Example

```bash
# Run tests and feed results to a CI reporter:
axon test src/*.ax --json --jobs 0 | tee results.ndjson | jq 'select(.status=="failed")'
```

### Code Example

```axon
@[test] fn test_ok()     { assert_eq(1 + 1, 2) }
@[test] fn test_fail()   { assert_eq(1 + 1, 3) }
```

```
$ axon test suite.ax --json
{"name":"test_ok","status":"ok","duration_ms":1}
{"name":"test_fail","status":"failed","duration_ms":2,"message":"assertion failed: values not equal"}
{"type":"summary","total":2,"passed":1,"failed":1,"skipped":0,"duration_ms":3}
```

---

## New Error Codes (Phase 4)

```
E0901  multiple 'fn main' found: {files}
E0902  circular import: {cycle}
E0903  duplicate top-level name '{name}' (first: {file1}:{line}, redefined: {file2}:{line})
E0904  --target '{triple}' not supported by this LLVM build
E0905  cross-compilation target '{triple}' requires sysroot — add [target.{triple}] to ~/.config/axon/cross.toml
E0906  cache entry is corrupt or was written by a different compiler version — ignoring
```

---

## CLI Summary (Phase 4 additions)

```
axon lsp                               start LSP server on stdin/stdout
axon fmt <file>                        reformat file in place
axon fmt <file> --check                exit 1 if formatting would change file
axon doc <file>                        print Markdown docs to stdout
axon doc <file> -o <out>               write Markdown docs to file
axon build <file...>                   compile multiple source files together
axon build <file> --target <triple>    cross-compile for specified target
axon build <file> --no-cache           skip cache lookup and write
axon cache clean                       remove all cache entries
axon cache clean --older-than <days>   remove stale cache entries
axon test <file> --jobs <N>            run tests with N parallel workers
axon test <file> --jobs 0              auto-detect worker count (num_cpus)
axon test <file> --json                emit NDJSON test results
```

---

## Dependencies (Phase 4 Additions)

```toml
# Cargo.toml additions for Phase 4
sha2        = "0.10"    # SHA-256 for cache keys
lsp-types   = "0.95"   # JSON-RPC and LSP type definitions
serde       = { version = "1", features = ["derive"] }
serde_json  = "1"       # JSON serialization for LSP + test output
num_cpus    = "1.16"    # auto-detect parallelism for --jobs 0
```

---

## Verification Checklist

Phase 4 is done when:

- [ ] `axon lsp` starts, accepts `initialize`, responds with capabilities
- [ ] Hover on a function call returns the function signature
- [ ] Hover on a local variable returns its inferred type
- [ ] Go-to-definition returns the correct declaration span
- [ ] `textDocument/didChange` re-runs analysis and pushes diagnostics within 100 ms for files < 500 lines
- [ ] `axon fmt` produces idempotent output (formatting a formatted file is a no-op)
- [ ] `axon fmt --check` exits 1 on unformatted input and 0 on formatted input
- [ ] `axon doc` generates valid Markdown for all documented items in the Phase 1 sample file
- [ ] Second `axon build` on unchanged source uses cache (measured < 20 ms for hello.ax)
- [ ] `--no-cache` forces full recompilation regardless of cache state
- [ ] `axon build src/*.ax` compiles multi-file programs; duplicate names across files are rejected
- [ ] Circular imports between files produce E0902
- [ ] `axon build --target aarch64-apple-darwin` produces an ARM ELF / Mach-O binary when cross-toolchain is configured
- [ ] `axon test --jobs 4` runs tests in parallel; total wall time is less than sequential time for suites with ≥ 8 tests
- [ ] `axon test --json` output is valid NDJSON parseable by `jq`; summary line is always last
- [ ] `axon test --json` includes `"message"` field on failed tests
