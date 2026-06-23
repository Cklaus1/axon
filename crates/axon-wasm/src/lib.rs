//! `axon-wasm` — the Axon interpreter as a `wasm32-unknown-unknown` cdylib, driven
//! from JavaScript by raw C-ABI exports (no wasm-bindgen, so the module has zero
//! JS-glue imports and loads with a bare `WebAssembly.instantiate`).
//!
//! It is the in-browser PLAYGROUND/REPL: run arbitrary `.ax` source dynamically
//! via the tree-walking interpreter — distinct from the codegen browser path,
//! which AOT-compiles each program separately and can't `eval` source. It is also
//! the entry-point foundation for the R15 browser `host_await` binding (R7c): a
//! follow-on slice adds an imported `axon_host_await` + Asyncify so a suspending
//! program can be driven from the page; this slice runs the non-suspending case.
//!
//! ABI (all lengths are byte counts; pointers index the module's linear memory):
//!   axon_alloc(len) -> ptr        — reserve `len` bytes; JS writes the source there
//!   axon_eval(ptr, len) -> i32    — parse+run the source; returns the exit code;
//!                                   captures the program's stdout for read-back
//!   axon_output_ptr() -> ptr      — start of the captured output (valid until the
//!   axon_output_len() -> len        next axon_eval)
//!
//! Typical JS:
//!   const p = inst.exports.axon_alloc(bytes.length);
//!   new Uint8Array(mem.buffer, p, bytes.length).set(bytes);
//!   const code = inst.exports.axon_eval(p, bytes.length);
//!   const out  = new TextDecoder().decode(new Uint8Array(mem.buffer,
//!                  inst.exports.axon_output_ptr(), inst.exports.axon_output_len()));

use std::cell::RefCell;

thread_local! {
    /// The captured stdout of the most recent `axon_eval`, held so JS can read it
    /// back via `axon_output_ptr`/`axon_output_len` after the call returns.
    static LAST_OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

/// Reserve `len` bytes of linear memory and hand JS the pointer. JS fills it with
/// the `.ax` source, then passes the same (ptr, len) to `axon_eval`, which
/// reclaims the buffer. Returns a null pointer for a zero-length request.
#[no_mangle]
pub extern "C" fn axon_alloc(len: usize) -> *mut u8 {
    if len == 0 {
        return std::ptr::null_mut();
    }
    let mut buf = Vec::<u8>::with_capacity(len);
    let ptr = buf.as_mut_ptr();
    std::mem::forget(buf); // ownership passes to JS; axon_eval reclaims it
    ptr
}

/// Heuristic: a prose goal file (markdown) opens with a `# ` heading; `.ax`
/// source never does (`#` is not Axon syntax). So a leading `# ` routes the
/// input through the surface compiler; anything else is treated as `.ax`.
#[allow(dead_code)]
fn is_prose_goal(src: &str) -> bool {
    src.lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| l.trim_start().starts_with("# "))
        .unwrap_or(false)
}

/// Compile a prose goal file to `.ax` via the surface compiler — the browser-side
/// `axon goal`. Returns the generated source, or a formatted error (so an invalid
/// or uncompilable goal file refuses with a clear message, like the CLI).
#[allow(dead_code)]
fn compile_prose(src: &str) -> Result<String, String> {
    let goal = axon_surface::parser::GoalFile::parse(src)
        .map_err(|e| format!("axon: goal file invalid: {e}\n"))?;
    axon_surface::compile::emit(&goal).map_err(|e| format!("axon: goal compilation failed: {e}\n"))
}

/// Format one check-pipeline diagnostic for the playground console, in the
/// human-readable shape the CLI uses (`severity[CODE] file:line:col: message`,
/// the source caret, and a `help:` line when present).
fn render_diag(d: &axon_core::PipelineDiagnostic) -> String {
    let mut s = format!(
        "{}[{}] {}:{}:{}: {}\n",
        d.severity, d.code, d.file, d.line, d.col, d.message
    );
    if !d.caret.is_empty() {
        s.push_str(&d.caret);
        if !d.caret.ends_with('\n') {
            s.push('\n');
        }
    }
    if let Some(h) = &d.help {
        s.push_str(&format!("  help: {h}\n"));
    }
    s
}

/// Parse, **check**, and run the `.ax` source at `(ptr, len)` via the interpreter,
/// capturing output for read-back. Like the `axon run` CLI flow (and unlike the
/// old eval-without-check), it runs the full static check pipeline FIRST — so
/// capability (`@[contained]`, E1001), type, and effect diagnostics surface in the
/// browser playground exactly as they do at the CLI. If the check finds any
/// `error`-severity diagnostic the program is REFUSED (the diagnostics are the
/// captured output, exit 2) and never runs; warnings don't block. A clean program
/// runs as before.
///
/// Returns the exit code: 0 ok; 1 a parse error; 2 a refused check (incl. a
/// sandbox/capability violation); the interpreter's runtime-flow codes — 3 verify
/// / 4 corrigible / 5 ai-policy / 6 refine / 7 goal-budget / 101 panic — otherwise.
/// Reclaims the source buffer that `axon_alloc` handed out.
///
/// # Safety
/// `ptr`/`len` must be a buffer previously returned by `axon_alloc(len)` that JS
/// filled with exactly `len` bytes of UTF-8 source. Calling otherwise is UB.
#[no_mangle]
pub unsafe extern "C" fn axon_eval(ptr: *mut u8, len: usize) -> i32 {
    // Reclaim the source buffer (alloc'd by axon_alloc with capacity == len).
    let src_bytes = if ptr.is_null() || len == 0 {
        Vec::new()
    } else {
        unsafe { Vec::from_raw_parts(ptr, len, len) }
    };
    let src = String::from_utf8_lossy(&src_bytes).into_owned();

    // Static check FIRST — the wedge's capability diagnostics must be visible in
    // the playground, and the browser must refuse what the CLI refuses.
    let diags = axon_core::check_pipeline(&src, "playground.ax");
    let errors: Vec<&axon_core::PipelineDiagnostic> =
        diags.iter().filter(|d| d.severity == "error").collect();
    if !errors.is_empty() {
        let mut out = String::new();
        for d in &errors {
            out.push_str(&render_diag(d));
        }
        out.push_str(&format!(
            "\naxon: {} error(s); refused before running.\n",
            errors.len()
        ));
        LAST_OUTPUT.with(|o| *o.borrow_mut() = out.into_bytes());
        return 2;
    }

    let (code, output) = match axon_core::parse_source(&src) {
        Ok(program) => axon_core::interp::run_program_capturing(&program),
        Err(e) => (1, format!("axon: parse error:\n{e}\n")),
    };
    LAST_OUTPUT.with(|o| *o.borrow_mut() = output.into_bytes());
    code
}

/// Pointer to the captured output of the last `axon_eval` (valid until the next).
#[no_mangle]
pub extern "C" fn axon_output_ptr() -> *const u8 {
    LAST_OUTPUT.with(|o| o.borrow().as_ptr())
}

/// Byte length of the captured output of the last `axon_eval`.
#[no_mangle]
pub extern "C" fn axon_output_len() -> usize {
    LAST_OUTPUT.with(|o| o.borrow().len())
}
