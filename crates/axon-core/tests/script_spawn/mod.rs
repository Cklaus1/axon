#![allow(dead_code)]
//! The ONE way a test in this workspace runs a repository script.
//!
//! A check that execs a built binary must exec the binary built from the tree
//! under test, never a stale one (C9 round 4, EQUIVALENCE (6)). A script run
//! by a test executes EXACTLY ONE of:
//!
//! * the binaries the test NAMES ([`Bins::Named`]), passed in the variable
//!   the script reads (`AXON=env!("CARGO_BIN_EXE_axon")`, ...);
//! * binaries the script BUILDS ITSELF and runs at the path cargo built them
//!   to ([`Bins::BuildsItsOwn`], `scripts/lib/axon_bin.sh`'s `use_built`);
//! * no binary this workspace builds ([`Bins::NoWorkspaceBinary`]).
//!
//! Never a binary that merely sits under `target/` or on PATH: C9 round 4
//! executed a planted `$REPO/target/debug/axon` through clock_parity.sh, and a
//! planted `axon` on PATH through the R17 IR/QEMU gates, which then PASSED.
//!
//! Enforced here, at the one place every test spawn goes through:
//! * every variable through which a caller can name a binary
//!   ([`BINARY_VARS`]) is REMOVED from the inherited environment, so an
//!   ambient `AXON` / `AXON_BIN` / `CORTEX_BIN` from the shell that launched
//!   `cargo test` never chooses what runs; only the test's own `Named` values
//!   reach the script;
//! * the script's text is checked before it runs: a script that picks a binary
//!   by a guessed `target/` path or a PATH lookup is refused
//!   ([`binary_choice_violations`]).
//!
//! `crates/axon-core/tests/harness_binaries.rs` fails the build if any test in
//! ANY crate spawns a script another way ([`spawn_violations`]), if any script
//! picks a binary another way, or if a script reads a binary-naming variable
//! missing from [`BINARY_VARS`].

use std::path::{Path, PathBuf};
use std::process::Command;

/// Every environment variable through which a caller names a binary for a
/// script to execute. Derived, not trusted: `harness_binaries.rs` fails if a
/// script accepts a binary from the caller through a variable not listed.
pub const BINARY_VARS: &[&str] = &[
    "AXON",
    "AXON_BIN",
    "AXON_GUEST_KERNEL",
    "AXON_OS",
    "AXON_VM",
    "AXON_FABRIC_BIN",
    "AXON_PROTECTED_LAUNCHER_BIN",
    "CORTEX_BIN",
    "PSV_DEV",
];

/// Which binaries the spawned script executes.
pub enum Bins<'a> {
    /// The script builds every workspace binary it runs and runs exactly the
    /// file cargo built. `CARGO_TARGET_DIR` is removed so the nested build
    /// never contends with (or reads from) the running test's target dir.
    BuildsItsOwn,
    /// The script builds nothing: it runs exactly these, each named in the
    /// variable the script reads.
    Named(&'a [(&'a str, &'a str)]),
    /// The script runs no binary this workspace builds.
    NoWorkspaceBinary,
}

/// The repository root, from the including crate (every crate is `crates/X`).
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// `interpreter <script>` with the binaries `bins` says, and nothing ambient.
/// Panics (refusing to run it) when the script picks a binary by a guessed
/// `target/` path or a PATH lookup.
pub fn script(interpreter: &str, path: impl AsRef<Path>, bins: Bins<'_>) -> Command {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("cannot read the script {}: {e}", path.display()));
    let bad = binary_choice_violations(&text);
    assert!(
        bad.is_empty(),
        "refused to run {}: it picks a binary its caller did not name and it did not build \
         (scripts/lib/axon_bin.sh):\n  {}",
        path.display(),
        bad.join("\n  ")
    );
    let mut c = Command::new(interpreter);
    c.arg(path);
    for v in BINARY_VARS {
        c.env_remove(v);
    }
    // The nested build (if any) lands in the workspace's own target dir, never
    // the running test's; with the binary chosen above, it cannot matter which
    // stale file sits in either.
    c.env_remove("CARGO_TARGET_DIR");
    match bins {
        Bins::BuildsItsOwn | Bins::NoWorkspaceBinary => {}
        Bins::Named(list) => {
            for (var, bin) in list {
                assert!(
                    BINARY_VARS.contains(var),
                    "{var} is not a binary-naming variable (BINARY_VARS)"
                );
                c.env(var, bin);
            }
        }
    }
    c
}

/// [`script`] run as the trailing arguments of a WRAPPER that execs them
/// unchanged (`unshare -m ... sh -c '...; exec "$@"' sh ...`, `setpriv ...`):
/// `wrapper[0] wrapper[1..] interpreter <script>`. The script is checked and
/// the environment built exactly as [`script`] does (it IS that command, with
/// the wrapper's argv in front), so a namespace or credential wrapper never
/// becomes a route around this helper. The wrapper itself must not be a
/// repository script.
pub fn script_under(
    wrapper: &[&std::ffi::OsStr],
    interpreter: &str,
    path: impl AsRef<Path>,
    bins: Bins<'_>,
) -> Command {
    let inner = script(interpreter, path, bins);
    let (prog, rest) = wrapper
        .split_first()
        .expect("script_under: an empty wrapper (use script)");
    assert!(
        !rest
            .iter()
            .chain([prog])
            .any(|a| a.to_string_lossy().contains("scripts/")),
        "script_under: the wrapper {wrapper:?} names a repository script; a script runs only \
         as the helper's own script"
    );
    let mut c = Command::new(prog);
    c.args(rest);
    c.arg(inner.get_program());
    c.args(inner.get_args());
    for (k, v) in inner.get_envs() {
        match v {
            Some(v) => c.env(k, v),
            None => c.env_remove(k),
        };
    }
    c
}

// ── what a script may not do ────────────────────────────────────────────────

/// Words that begin a line printing a MESSAGE (a path in prose is not a choice
/// of binary).
const MESSAGE_WORDS: &[&str] = &[
    "echo", "printf", "note", "say", "red", "dim", "bold", "warn", "skip", "bad",
];

/// Leading path segments that are a cargo target triple.
const TRIPLE_PREFIXES: &[&str] = &[
    "wasm32", "x86_64", "aarch64", "i686", "armv", "riscv", "thumb",
];

/// The directory a line assigns to `CARGO_TARGET_DIR`, if any: a script may
/// run what it built into a directory it chose itself.
fn cargo_target_dir_assignment(line: &str) -> Option<String> {
    let i = line.find("CARGO_TARGET_DIR=")?;
    let v = &line[i + "CARGO_TARGET_DIR=".len()..];
    let v = v.trim_start_matches(['"', '\'']);
    let end = v
        .find(|c: char| c == '"' || c == '\'' || c.is_whitespace())
        .unwrap_or(v.len());
    let v = &v[..end];
    (!v.is_empty() && !v.starts_with("${")).then(|| v.to_string())
}

fn path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "_-./${}:+".contains(c)
}

/// The directories whose `debug/` or `release/` profile output `line` names.
fn profile_dirs(line: &str) -> Vec<String> {
    let mut out = vec![];
    for marker in ["/debug/", "/release/"] {
        let mut from = 0;
        while let Some(i) = line[from..].find(marker) {
            let at = from + i;
            from = at + marker.len();
            let start = line[..at]
                .char_indices()
                .rev()
                .find(|&(_, c)| !path_char(c))
                .map(|(j, c)| j + c.len_utf8())
                .unwrap_or(0);
            let mut prefix = &line[start..at];
            // `${VAR:-default/debug/x}`: the default is what the line names.
            if let Some(k) = prefix.rfind(":-") {
                prefix = &prefix[k + 2..];
            }
            let mut dir = prefix.to_string();
            if let Some((head, last)) = dir.rsplit_once('/') {
                if TRIPLE_PREFIXES.iter().any(|t| last.starts_with(t)) {
                    dir = head.to_string();
                }
            }
            if dir.contains("target") || dir.contains("CARGO_TARGET_DIR") {
                out.push(dir);
            }
        }
    }
    out
}

/// Lines of a script that pick a binary this workspace builds by any route
/// other than `named_bin` / `use_built` / `built_bin` (scripts/lib/axon_bin.sh)
/// or a directory the script itself assigned to `CARGO_TARGET_DIR` and built
/// into. Empty: the script is clean.
pub fn binary_choice_violations(text: &str) -> Vec<String> {
    let own: Vec<String> = text
        .lines()
        .filter_map(cargo_target_dir_assignment)
        .collect();
    let resolves_via_cargo = text.contains("target_directory");
    let mut out = vec![];
    for (n, raw) in text.lines().enumerate() {
        let l = raw.trim_start();
        if l.starts_with('#') || l.starts_with("//") {
            continue;
        }
        let first = l.split(|c: char| c.is_whitespace()).next().unwrap_or("");
        if MESSAGE_WORDS.contains(&first) {
            continue;
        }
        for dir in profile_dirs(l) {
            if !own.contains(&dir) {
                out.push(format!(
                    "line {}: names a binary under {dir}/ that the script did not build into a \
                     directory it chose: {}",
                    n + 1,
                    raw.trim()
                ));
            }
        }
        for w in [
            "command -v axon",
            "command -v cortex",
            "which axon",
            "which cortex",
            "shutil.which(\"axon",
            "shutil.which('axon",
        ] {
            if l.contains(w) {
                out.push(format!(
                    "line {}: finds a binary on PATH ({w}): {}",
                    n + 1,
                    raw.trim()
                ));
            }
        }
        // A directory the script ASSIGNS to CARGO_TARGET_DIR is one it chose.
        let assigns_target_dir =
            l.contains("\"CARGO_TARGET_DIR\":") || l.contains("CARGO_TARGET_DIR=");
        if !resolves_via_cargo
            && !assigns_target_dir
            && (l.contains("\"target\"") || l.contains("'target'"))
            && (l.contains("os.path.join") || l.contains("/ \"target\""))
        {
            out.push(format!(
                "line {}: guesses the target dir (ask cargo: `cargo metadata` target_directory): {}",
                n + 1,
                raw.trim()
            ));
        }
    }
    out
}

/// The variables a script accepts a binary through from its CALLER: those it
/// passes to `named_bin`, or to `use_built` without first clearing them, or
/// defaults with `${VAR:-...}` to a built path.
pub fn caller_binary_vars(text: &str) -> Vec<String> {
    let mut out = vec![];
    for raw in text.lines() {
        let l = raw.trim_start();
        if l.starts_with('#') {
            continue;
        }
        for verb in ["named_bin ", "use_built "] {
            let mut from = 0;
            while let Some(i) = l[from..].find(verb) {
                let at = from + i;
                from = at + verb.len();
                // A function definition or a mention inside a word is not a call.
                if at > 0 && !" ;&|(".contains(&l[at - 1..at]) {
                    continue;
                }
                let var: String = l[from..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if var.is_empty() {
                    continue;
                }
                let cleared =
                    l.contains(&format!("{var}=\"\";")) || l.contains(&format!("{var}=\"\" "));
                if verb == "named_bin " || !cleared {
                    out.push(var);
                }
            }
        }
        // `AXON="${AXON:-$WORK/.../debug/axon}"`
        if let Some(i) = l.find("=\"${") {
            let var = &l[..i];
            let rest = &l[i + 4..];
            if !var.is_empty()
                && var.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                && rest.starts_with(&format!("{var}:-"))
                && (rest.contains("/debug/") || rest.contains("/release/"))
            {
                out.push(var.to_string());
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

// ── what a test may not do ──────────────────────────────────────────────────

/// Programs that run a script file given as their argument.
const INTERPRETERS: &[&str] = &[
    "bash", "sh", "dash", "zsh", "python", "python3", "perl", "env", "node",
];

/// Byte offset of the `)` matching the `(` at `open`, skipping string and
/// char literals.
fn matching_close(s: &str, open: usize) -> Option<usize> {
    let b = s.as_bytes();
    let (mut depth, mut i) = (0i32, open);
    while i < b.len() {
        match b[i] {
            b'"' => {
                // A raw string r#"..."# or r"...": find its terminator.
                let raw_hashes = {
                    let mut k = i;
                    let mut h = 0;
                    while k > 0 && b[k - 1] == b'#' {
                        h += 1;
                        k -= 1;
                    }
                    if k > 0 && b[k - 1] == b'r' {
                        Some(h)
                    } else {
                        None
                    }
                };
                i += 1;
                match raw_hashes {
                    Some(h) => {
                        let term = format!("\"{}", "#".repeat(h));
                        i += s[i..].find(&term)? + term.len();
                        continue;
                    }
                    None => {
                        while i < b.len() && b[i] != b'"' {
                            if b[i] == b'\\' {
                                i += 1;
                            }
                            i += 1;
                        }
                    }
                }
            }
            b'\'' if i + 2 < b.len() && (b[i + 2] == b'\'' || b[i + 1] == b'\\') => {
                // A char literal ('(' or '\n'), not a lifetime.
                i += s[i + 1..].find('\'').map(|k| k + 1)?;
            }
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// The text of the argument list opened at `open` (exclusive of the parens).
fn call_args(s: &str, open: usize) -> Option<&str> {
    matching_close(s, open).map(|c| &s[open + 1..c])
}

/// The builder chain after a call ending at `close`: up to the end of the
/// statement.
fn chain_after(s: &str, close: usize) -> &str {
    let rest = &s[close + 1..];
    let end = rest.find(';').unwrap_or(rest.len());
    &rest[..end]
}

/// Whether `ident` is bound (`let [mut] ident = ...;`) to something naming
/// `scripts/`.
fn bound_to_a_script(src: &str, ident: &str) -> bool {
    for pat in [format!("let {ident} "), format!("let mut {ident} ")] {
        let mut from = 0;
        while let Some(i) = src[from..].find(&pat) {
            let at = from + i;
            from = at + pat.len();
            let Some(eq) = src[at..].find('=') else {
                continue;
            };
            // To the end of the statement: the first `;` outside any bracket.
            let body = &src[at + eq + 1..];
            let (mut depth, mut end) = (0i32, body.len());
            for (k, c) in body.char_indices() {
                match c {
                    '(' | '[' | '{' => depth += 1,
                    ')' | ']' | '}' => depth -= 1,
                    ';' if depth <= 0 => {
                        end = k;
                        break;
                    }
                    _ => {}
                }
            }
            if body[..end].contains("scripts/") {
                return true;
            }
        }
    }
    false
}

fn names_a_script(src: &str, arg: &str) -> bool {
    let a = arg.trim().trim_start_matches('&');
    if a.contains("scripts/") {
        return true;
    }
    let ident: String = a
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    !ident.is_empty() && bound_to_a_script(src, &ident)
}

/// Spawns in a test source file that run a repository script around
/// [`script`]: an interpreter (bash, sh, python3, ...) given a FILE rather
/// than an inline `-c` program, a script executed directly, or a script path
/// handed to some command's `.arg(...)`. Empty: every script spawn in `src`
/// goes through the helper.
pub fn spawn_violations(src: &str) -> Vec<String> {
    let mut out = vec![];
    let line_of = |at: usize| src[..at].matches('\n').count() + 1;
    let in_comment = |at: usize| {
        let ls = src[..at].rfind('\n').map(|k| k + 1).unwrap_or(0);
        src[ls..at].trim_start().starts_with("//")
    };
    let mut from = 0;
    let needle = "Command::new(";
    while let Some(i) = src[from..].find(needle) {
        let at = from + i;
        from = at + needle.len();
        if in_comment(at) {
            continue;
        }
        let open = at + needle.len() - 1;
        let (Some(arg), Some(close)) = (call_args(src, open), matching_close(src, open)) else {
            continue;
        };
        let a = arg.trim();
        let lit = a.strip_prefix('"').and_then(|x| x.strip_suffix('"'));
        let interp = match lit {
            Some(p) => INTERPRETERS.contains(&p.rsplit('/').next().unwrap_or(p)),
            None => a.starts_with("concat!("),
        };
        if interp {
            let chain = chain_after(src, close);
            // An inline program (`sh -c "..."`, `env -i ... sh -c`, `python3 -c`)
            // is not a script FILE.
            let inline = chain.contains("\"-c\"");
            if !inline {
                out.push(format!(
                    "line {}: an interpreter ({a}) is spawned on a FILE outside the script helper",
                    line_of(at)
                ));
            }
        } else if names_a_script(src, a) {
            out.push(format!(
                "line {}: a script is executed directly outside the script helper ({a})",
                line_of(at)
            ));
        }
    }
    for needle in [".arg(", ".args("] {
        let mut from = 0;
        while let Some(i) = src[from..].find(needle) {
            let at = from + i;
            from = at + needle.len();
            if in_comment(at) {
                continue;
            }
            let Some(arg) = call_args(src, at + needle.len() - 1) else {
                continue;
            };
            if names_a_script(src, arg) {
                out.push(format!(
                    "line {}: a script path is handed to a command outside the script helper ({})",
                    line_of(at),
                    arg.trim()
                ));
            }
        }
    }
    out
}

// ── the workspace binaries a test executes directly ────────────────────────

/// A workspace binary that a test EXECUTES but whose crate this test's cargo
/// run does not build (the `axon` interpreter from axon-psv, axon-fabric,
/// axon-cortex, axon-os and axon-intent tests; the `cortex` CLI; `axon-os`),
/// as cargo has made it current for THIS tree.
///
/// It used to be whatever `$AXON_BIN` or `<target>/debug/<name>` held:
/// `cargo test -p axon-psv` never rebuilds axon-core, so after a merge the
/// PSV-1 attack test ran an interpreter that predated the fix and PASSED THE
/// ATTACK (C9 round 4, integration). Now:
/// * a binary named in `var` (the mutation and paired-disable harnesses name
///   the interpreter they just built) is used only if it is at least as new as
///   every source file cargo would rebuild it from (the package and its
///   workspace path dependencies, and Cargo.lock); a stale one is REFUSED;
/// * otherwise the test BUILDS it: `cargo <build>` into
///   `<this test's target dir>/workspace-bins`, and runs exactly the file that
///   build left (cargo rebuilds whatever changed), never one that merely sits
///   in a target dir.
pub fn workspace_bin(var: &str, build: &[&str], name: &str) -> PathBuf {
    use std::collections::HashMap;
    use std::sync::Mutex;
    let pkg = build
        .iter()
        .position(|a| *a == "-p")
        .and_then(|i| build.get(i + 1))
        .copied()
        .unwrap_or_else(|| panic!("workspace_bin: {build:?} names no -p package"));
    if let Some(p) = std::env::var_os(var) {
        let p = PathBuf::from(p);
        assert!(p.is_file(), "{var}={} is not a file", p.display());
        let stale = stale_against_sources(&p, pkg);
        assert!(
            stale.is_none(),
            "refused: {var}={} is older than {} -- it was not built from this tree \
             (rebuild it: cargo {})",
            p.display(),
            stale.unwrap_or_default(),
            build.join(" ")
        );
        return p;
    }
    static BUILT: Mutex<Option<HashMap<String, PathBuf>>> = Mutex::new(None);
    let key = format!("{} => {name}", build.join(" "));
    let mut built = BUILT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(p) = built.get_or_insert_with(HashMap::new).get(&key) {
        return p.clone();
    }
    // <target>/<profile>/deps/<this test> -> <target>
    let exe = std::env::current_exe().unwrap();
    let target = exe
        .ancestors()
        .nth(3)
        .expect("this test's target dir")
        .join("workspace-bins");
    // Test PROCESSES running at once (a sharded suite, concurrent harness
    // cells) share `target`, and cargo REPLACES an uplifted binary whenever it
    // relinks it -- in a git worktree on EVERY build of the interpreter, since
    // axon-core's build script watches `.git/HEAD` and `.git/index` paths that
    // do not exist there (`.git` is a file), and a missing rerun-if path
    // reruns it. One process's build then removed the `debug/axon` another
    // had just been handed ("No such file or directory"). So the build runs
    // under an exclusive lock on `target`, and each process gets its OWN copy
    // of what cargo just built (copied by a child `cp`, so this process holds
    // no write descriptor to it), which no later build touches.
    std::fs::create_dir_all(&target).unwrap();
    let lock = std::fs::File::create(target.join(".workspace-bins.lock")).unwrap();
    lock.lock().unwrap();
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let o = Command::new(&cargo)
        .args(build)
        .arg("-q")
        .current_dir(repo_root())
        .env("CARGO_TARGET_DIR", &target)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "could not build {name} from this tree (cargo {}):\n{}",
        build.join(" "),
        String::from_utf8_lossy(&o.stderr)
    );
    let profile = target.join("debug");
    let p = if build.contains(&"--example") {
        profile.join("examples").join(name)
    } else {
        profile.join(name)
    };
    assert!(
        p.is_file(),
        "cargo {} left no {}",
        build.join(" "),
        p.display()
    );
    let mine = per_process_copy(&target, &p, name);
    drop(lock);
    built.as_mut().unwrap().insert(key, mine.clone());
    mine
}

/// `built` copied to `<target>/per-process/<this pid>/<name>` (mode kept), by
/// a child `cp`. Copies of processes that have exited are removed first.
/// Called holding the `target` lock.
fn per_process_copy(target: &Path, built: &Path, name: &str) -> PathBuf {
    let base = target.join("per-process");
    if let Ok(entries) = std::fs::read_dir(&base) {
        for e in entries.flatten() {
            let pid = e.file_name().to_string_lossy().into_owned();
            if !Path::new("/proc").join(&pid).exists() {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    let dir = base.join(std::process::id().to_string());
    std::fs::create_dir_all(&dir).unwrap();
    let mine = dir.join(name);
    let st = Command::new("cp")
        .arg("--preserve=mode,timestamps")
        .arg("--")
        .arg(built)
        .arg(&mine)
        .status()
        .unwrap();
    assert!(
        st.success(),
        "could not copy {} to {}",
        built.display(),
        mine.display()
    );
    mine
}

/// The newest source file cargo would rebuild `pkg`'s binaries from, when it
/// is newer than `bin` (None: `bin` is current).
fn stale_against_sources(bin: &Path, pkg: &str) -> Option<String> {
    let built = std::fs::metadata(bin).ok()?.modified().ok()?;
    let root = repo_root();
    let mut crates = vec![root.join("crates").join(pkg)];
    let mut seen = std::collections::HashSet::new();
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    let mut consider = |p: &Path| {
        if let Ok(t) = std::fs::metadata(p).and_then(|m| m.modified()) {
            if newest.as_ref().is_none_or(|(n, _)| t > *n) {
                newest = Some((t, p.to_path_buf()));
            }
        }
    };
    consider(&root.join("Cargo.lock"));
    while let Some(dir) = crates.pop() {
        let Ok(dir) = dir.canonicalize() else {
            continue;
        };
        if !seen.insert(dir.clone()) {
            continue;
        }
        // Workspace path dependencies: `path = "../x"` in the manifest.
        if let Ok(m) = std::fs::read_to_string(dir.join("Cargo.toml")) {
            for line in m.lines() {
                if let Some(i) = line.find("path = \"") {
                    let rest = &line[i + 8..];
                    if let Some(end) = rest.find('"') {
                        let dep = dir.join(&rest[..end]);
                        if dep.join("Cargo.toml").is_file() {
                            crates.push(dep);
                        }
                    }
                }
            }
        }
        // The crate's files git calls part of the tree (tracked, or untracked
        // and not ignored). A git-ignored file is an OUTPUT, not a source:
        // axon-core's own suite writes `crates/axon-core/out/` (gitignored
        // `out/`), and counting it made the interpreter a harness had just
        // built read as stale, so every consumer suite that ran after it in
        // one cell refused AXON_BIN (paired-disable M58, C9 round 4;
        // amendment 59). Without git (a source tarball), every file counts.
        if let Some(files) = tree_files(&root, &dir) {
            for p in files {
                let rel = p.strip_prefix(&dir).unwrap_or(&p);
                if !rel.components().any(|c| {
                    ["target", "tests", "benches"]
                        .iter()
                        .any(|x| c.as_os_str() == *x)
                }) {
                    consider(&p);
                }
            }
            continue;
        }
        let mut stack = vec![dir];
        while let Some(d) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&d) else {
                continue;
            };
            for e in rd.flatten() {
                let p = e.path();
                let n = e.file_name();
                if n == "target" || n == "tests" || n == "benches" {
                    continue;
                }
                if p.is_dir() {
                    stack.push(p);
                } else {
                    consider(&p);
                }
            }
        }
    }
    newest
        .filter(|(t, _)| *t > built)
        .map(|(_, p)| p.display().to_string())
}

/// The files under `dir` that git counts as the working tree: tracked, or
/// untracked and not ignored. None when git cannot say.
fn tree_files(root: &Path, dir: &Path) -> Option<Vec<PathBuf>> {
    let o = Command::new("git")
        .arg("-C")
        .arg(root)
        .args([
            "ls-files",
            "-z",
            "--cached",
            "--others",
            "--exclude-standard",
            "--",
        ])
        .arg(dir)
        .output()
        .ok()?;
    if !o.status.success() {
        return None;
    }
    Some(
        o.stdout
            .split(|b| *b == 0)
            .filter(|f| !f.is_empty())
            .map(|f| root.join(String::from_utf8_lossy(f).as_ref()))
            .collect(),
    )
}

/// Lines of a test source that pick a workspace binary to EXECUTE around
/// [`workspace_bin`]: reading a binary-naming variable directly, or naming a
/// `<profile>/axon*` / `<profile>/cortex*` file in a target dir. (A line that
/// WRITES such a path, as a fixture's build output, is not a choice of binary.)
pub fn binary_resolution_violations(src: &str) -> Vec<String> {
    // A test that runs its own `cargo build --target-dir DIR` and execs what
    // that build left chose its directory (as a script assigning
    // CARGO_TARGET_DIR does).
    let builds_into_its_own_dir = src.contains("\"--target-dir\"");
    let mut out = vec![];
    for (n, raw) in src.lines().enumerate() {
        let l = raw.trim_start();
        if l.starts_with("//") || l.contains("write(") || l.contains("write_executable(") {
            continue;
        }
        // (A kernel image is named explicitly or not at all: AXON_GUEST_KERNEL.)
        let reads_var = BINARY_VARS
            .iter()
            .filter(|v| **v != "AXON_GUEST_KERNEL")
            .any(|v| {
                l.contains(&format!("var_os(\"{v}\")")) || l.contains(&format!("env::var(\"{v}\")"))
            });
        let names_profile_bin = !builds_into_its_own_dir
            && l.contains("join(\"")
            && ["debug/", "release/"].iter().any(|p| {
                ["axon", "cortex"]
                    .iter()
                    .any(|b| l.contains(&format!("{p}{b}")))
            });
        if reads_var || names_profile_bin {
            out.push(format!(
                "line {}: picks a workspace binary around script_spawn::workspace_bin: {}",
                n + 1,
                raw.trim()
            ));
        }
    }
    out
}
