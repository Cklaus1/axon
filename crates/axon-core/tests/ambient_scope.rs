//! R0b — ambient PATH/HOST scope: `AXON_ALLOWED_FS_READ`,
//! `AXON_ALLOWED_FS_WRITE`, `AXON_ALLOWED_NET`.
//!
//! `AXON_ALLOWED_EFFECTS` can only say "IO" or "not IO", and `IO` is
//! `println` as much as `write_file`. A repair candidate's checks need console
//! output and must not touch the disk, so there was no ceiling an operator
//! could set for a candidate run: a model-written repair wrote
//! `/var/tmp/r51-tmp/PWNED` during its checks and the episode exited 0.
//!
//! These three variables populate the ambient sandbox's SCOPE, the same
//! per-argument table `sandbox_create_scoped` and `@[contained]` use. Unset is
//! unrestricted; set-but-empty denies every path/host of that kind; and they are
//! independent of `AXON_ALLOWED_EFFECTS` — with it unset every effect stays
//! allowed and only paths/hosts are restricted.
//!
//! Red before the fix: the interpreter read none of the three, so every
//! refused case below ran to exit 0 and created its file.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SCOPE_VARS: [&str; 3] = [
    "AXON_ALLOWED_FS_READ",
    "AXON_ALLOWED_FS_WRITE",
    "AXON_ALLOWED_NET",
];

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon_ascope_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `axon run` with the effect ceiling UNSET and exactly `vars` of the three
/// scope variables set; the provenance cache is kept inside `dir`.
fn run(dir: &Path, src: &str, vars: &[(&str, &str)]) -> Output {
    let prog = dir.join("p.ax");
    std::fs::write(&prog, src).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_axon"));
    cmd.arg("run")
        .arg(&prog)
        .current_dir(dir)
        .env_remove("AXON_ALLOWED_EFFECTS")
        .env("XDG_CACHE_HOME", dir.join("cache"));
    for v in SCOPE_VARS {
        cmd.env_remove(v);
    }
    for (k, v) in vars {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
}

fn lit(p: &Path) -> String {
    p.display().to_string()
}

#[test]
fn fs_write_empty_keeps_the_console_and_refuses_every_write() {
    let d = tmp("wdeny");
    std::fs::write(d.join("src.txt"), "s").unwrap();
    let src_txt = lit(&d.join("src.txt"));

    // The console is IO too, and must survive a filesystem-write deny-all.
    let o = run(
        &d,
        "fn main() { println(\"still talking\") }\n",
        &[("AXON_ALLOWED_FS_WRITE", "")],
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(text(&o).contains("still talking"));

    let cases: Vec<(&str, String, PathBuf)> = vec![
        (
            "write_file",
            format!("write_file(\"{}\", \"x\")", lit(&d.join("w.txt"))),
            d.join("w.txt"),
        ),
        (
            "append_file",
            format!("append_file(\"{}\", \"x\")", lit(&d.join("a.txt"))),
            d.join("a.txt"),
        ),
        (
            "file_copy",
            format!("file_copy(\"{src_txt}\", \"{}\")", lit(&d.join("c.txt"))),
            d.join("c.txt"),
        ),
        (
            "file_rename",
            format!("file_rename(\"{src_txt}\", \"{}\")", lit(&d.join("r.txt"))),
            d.join("r.txt"),
        ),
        (
            "dir_create",
            format!("dir_create(\"{}\")", lit(&d.join("newdir"))),
            d.join("newdir"),
        ),
    ];
    for (name, call, target) in cases {
        let o = run(
            &d,
            &format!("fn main() {{\n    println(\"before\")\n    let _ = {call}\n}}\n"),
            &[("AXON_ALLOWED_FS_WRITE", "")],
        );
        let t = text(&o);
        assert_eq!(
            o.status.code(),
            Some(8),
            "ATTACK: {name} ran under an empty AXON_ALLOWED_FS_WRITE:\n{t}"
        );
        assert!(t.contains("before"), "{name}: console output lost:\n{t}");
        assert!(t.contains("write path"), "{name}: wrong refusal:\n{t}");
        assert!(!target.exists(), "{name} created {}", target.display());
        assert!(d.join("src.txt").exists(), "{name} moved the source away");
    }
}

#[test]
fn an_fs_write_prefix_admits_only_paths_under_it() {
    let d = tmp("wprefix");
    std::fs::create_dir_all(d.join("out")).unwrap();
    let prefix = format!("{}/", lit(&d.join("out")));
    let inside = d.join("out").join("ok.txt");
    let outside = d.join("escape.txt");

    let o = run(
        &d,
        &format!(
            "fn main() {{ let _ = write_file(\"{}\", \"x\") }}\n",
            lit(&inside)
        ),
        &[("AXON_ALLOWED_FS_WRITE", &prefix)],
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(inside.exists());

    let o = run(
        &d,
        &format!(
            "fn main() {{ let _ = write_file(\"{}\", \"x\") }}\n",
            lit(&outside)
        ),
        &[("AXON_ALLOWED_FS_WRITE", &prefix)],
    );
    assert_eq!(o.status.code(), Some(8), "{}", text(&o));
    assert!(!outside.exists());

    // `..` cannot climb out of the prefix.
    let climb = format!("{prefix}../climb.txt");
    let o = run(
        &d,
        &format!("fn main() {{ let _ = write_file(\"{climb}\", \"x\") }}\n"),
        &[("AXON_ALLOWED_FS_WRITE", &prefix)],
    );
    assert_eq!(o.status.code(), Some(8), "{}", text(&o));
    assert!(!d.join("climb.txt").exists());
}

#[test]
fn fs_read_empty_refuses_reads_and_existence_probes() {
    let d = tmp("rdeny");
    std::fs::write(d.join("secret.txt"), "s3cr3t").unwrap();
    let secret = lit(&d.join("secret.txt"));
    for call in [
        format!("match read_file(\"{secret}\") {{ Ok(s) => s  Err(_) => \"none\" }}"),
        format!("if file_exists(\"{secret}\") {{ \"yes\" }} else {{ \"no\" }}"),
    ] {
        let o = run(
            &d,
            &format!("fn main() {{\n    let v = {call}\n    println(v)\n}}\n"),
            &[("AXON_ALLOWED_FS_READ", "")],
        );
        let t = text(&o);
        assert_eq!(o.status.code(), Some(8), "{call}:\n{t}");
        assert!(t.contains("read path"), "{call}:\n{t}");
        assert!(!t.contains("s3cr3t"), "{call} leaked the file:\n{t}");
    }
    // Writes are a separate kind: a read deny-all does not refuse them.
    let w = d.join("w.txt");
    let o = run(
        &d,
        &format!(
            "fn main() {{ let _ = write_file(\"{}\", \"x\") }}\n",
            lit(&w)
        ),
        &[("AXON_ALLOWED_FS_READ", "")],
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(w.exists());
}

/// Offline-safe: the refusal is a scope check before any socket is opened, so
/// the message is the sandbox's, not a connection error.
#[test]
fn net_empty_refuses_an_http_builtin_before_any_connect() {
    let d = tmp("ndeny");
    let o = run(
        &d,
        "fn main() {\n    let _ = http_get(\"http://127.0.0.1:9/x\", \"\")\n    println(\"got\")\n}\n",
        &[("AXON_ALLOWED_NET", "")],
    );
    let t = text(&o);
    assert_eq!(o.status.code(), Some(8), "{t}");
    assert!(t.contains("reach host `127.0.0.1`"), "{t}");
}

/// The scope is a CEILING: a sandbox the program creates cannot widen it, even
/// one that names `*` (unrestricted) for every kind.
#[test]
fn an_inner_sandbox_cannot_widen_the_ambient_scope() {
    let d = tmp("inner");
    let target = d.join("inner.txt");
    let src = format!(
        "fn w(n: i64) -> i64 {{\n    let _ = write_file(\"{}\", \"x\")\n    n\n}}\n\
         fn main() {{\n    \
         let p = principal_root(\"p\", false, false, false, 100)\n    \
         let s = sandbox_create_scoped(p, \"IO\", \"*\", \"*\", \"*\")\n    \
         let _ = sandbox_run(s, \"w\", 1)\n}}\n",
        lit(&target)
    );
    let o = run(&d, &src, &[("AXON_ALLOWED_FS_WRITE", "")]);
    let t = text(&o);
    assert_eq!(
        o.status.code(),
        Some(8),
        "ATTACK: an inner sandbox scoped `*` widened the ambient AXON_ALLOWED_FS_WRITE= scope:\n{t}"
    );
    assert!(
        !target.exists(),
        "the inner sandbox widened the ambient scope"
    );
}

/// The ambient entry built from scope variables alone allows every EFFECT, so
/// it is the widest sandbox there is. Entering it by handle from inside a
/// narrower sandbox (`sandbox_run(0, …)`) must be refused like entering any
/// wider sandbox — it is not a way back out to "every effect".
#[test]
fn entering_the_ambient_handle_from_a_narrower_sandbox_is_refused() {
    let d = tmp("reenter");
    let marker = d.join("spawned");
    let src = format!(
        "fn do_spawn(n: i64) -> i64 {{\n    let _ = exec(\"touch\", [\"{}\"])\n    n\n}}\n\
         fn inner(n: i64) -> i64 {{ sandbox_run(0, \"do_spawn\", n) }}\n\
         fn main() {{\n    \
         let p = principal_root(\"p\", false, false, false, 100)\n    \
         let s = sandbox_create(p, \"IO\")\n    \
         let _ = sandbox_run(s, \"inner\", 1)\n}}\n",
        lit(&marker)
    );
    let o = run(&d, &src, &[("AXON_ALLOWED_FS_WRITE", "")]);
    let t = text(&o);
    assert_eq!(
        o.status.code(),
        Some(8),
        "ATTACK: sandbox_run(0, …) re-entered the all-effects ambient handle from an IO-only sandbox:\n{t}"
    );
    assert!(t.contains("never widen"), "{t}");
    assert!(
        !marker.exists(),
        "the program re-entered the ambient sandbox and spawned"
    );
}

/// Independent of the effect ceiling in both directions: an effect ceiling
/// that grants `IO` does not lift a scope, and a scope does not grant an effect
/// the ceiling withholds.
#[test]
fn the_scope_and_the_effect_ceiling_compose() {
    let d = tmp("compose");
    let target = d.join("c.txt");
    let prog = format!(
        "fn main() {{\n    println(\"hi\")\n    let _ = write_file(\"{}\", \"x\")\n}}\n",
        lit(&target)
    );
    std::fs::write(d.join("p.ax"), &prog).unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_axon"))
        .arg("run")
        .arg(d.join("p.ax"))
        .env("XDG_CACHE_HOME", d.join("cache"))
        .env("AXON_ALLOWED_EFFECTS", "IO")
        .env("AXON_ALLOWED_FS_WRITE", "")
        .env_remove("AXON_ALLOWED_FS_READ")
        .env_remove("AXON_ALLOWED_NET")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(8), "{}", text(&o));
    assert!(!target.exists());

    let o = Command::new(env!("CARGO_BIN_EXE_axon"))
        .arg("run")
        .arg(d.join("p.ax"))
        .env("XDG_CACHE_HOME", d.join("cache"))
        .env("AXON_ALLOWED_EFFECTS", "")
        .env("AXON_ALLOWED_FS_WRITE", lit(&d))
        .env_remove("AXON_ALLOWED_FS_READ")
        .env_remove("AXON_ALLOWED_NET")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(8), "{}", text(&o));
    assert!(!text(&o).contains("hi\n"), "{}", text(&o));
    assert!(!target.exists());
}

/// The runtime's `@[adaptive]` provenance row is a program-derived file write
/// (the program chooses its score, input and payload): a run whose scope denies
/// every write must not persist one, as the effect ceiling already does not.
/// The CLI's own `run_start` stamp is not program data and, as under
/// `AXON_ALLOWED_EFFECTS=Pure`, is not what this counts.
#[test]
fn fs_write_empty_suppresses_the_adaptive_provenance_row() {
    let d = tmp("prov");
    let src = "@[adaptive]\nfn metric(x: i64) -> i64 { 10 - (x - 3) * (x - 3) }\n\
               fn main() { let _ = metric(3) }\n";
    let log = d.join("cache").join("axon").join("provenance.jsonl");
    let rows = || {
        std::fs::read_to_string(&log)
            .map(|t| t.matches("\"event\":\"adaptive_return\"").count())
            .unwrap_or(0)
    };

    // PREMISE: unrestricted, the row is written.
    let o = run(&d, src, &[]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert_eq!(rows(), 1, "premise: an unrestricted run writes provenance");

    let o = run(&d, src, &[("AXON_ALLOWED_FS_WRITE", "")]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert_eq!(rows(), 1, "an adaptive row was written under FS_WRITE=");

    // A prefix that covers the cache dir admits it.
    let cache = format!("{}/", lit(&d.join("cache")));
    let o = run(&d, src, &[("AXON_ALLOWED_FS_WRITE", &cache)]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert_eq!(rows(), 2, "a scope covering the cache refused provenance");
}

/// Unset is today's behaviour exactly: nothing is restricted.
#[test]
fn unset_scope_vars_restrict_nothing() {
    let d = tmp("unset");
    let target = d.join("u.txt");
    let o = run(
        &d,
        &format!(
            "fn main() {{ let _ = write_file(\"{}\", \"x\") }}\n",
            lit(&target)
        ),
        &[],
    );
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert!(target.exists());
}

/// A compiled binary cannot enforce the scope (codegen and `axon-rt` read none
/// of the three variables), so it must refuse rather than silently drop it —
/// the treatment `AXON_ALLOWED_EFFECTS` already gets: `axon build` refuses to
/// emit while one is set (E0910), and a binary built earlier refuses to start
/// under one (`__axon_rt_refuse_interp_only_env`, exit 2).
#[test]
fn a_compiled_binary_refuses_the_scope_rather_than_dropping_it() {
    let d = tmp("native");
    let target = d.join("native.txt");
    let src = d.join("n.ax");
    std::fs::write(
        &src,
        format!(
            "fn main() -> i64 {{\n    let _ = write_file(\"{}\", \"x\")\n    0\n}}\n",
            lit(&target)
        ),
    )
    .unwrap();
    let bin = d.join("nbin");
    let mut build = Command::new(env!("CARGO_BIN_EXE_axon"));
    build
        .args(["build", src.to_str().unwrap(), "-o", bin.to_str().unwrap()])
        .env_remove("AXON_ALLOWED_EFFECTS")
        .env_remove("AXON_BUDGET_TOKENS");
    for v in SCOPE_VARS {
        build.env_remove(v);
    }
    let built = build.output().unwrap();
    if !built.status.success() {
        eprintln!(
            "codegen unavailable (control build with no scope failed) — skipped: {}",
            String::from_utf8_lossy(&built.stderr)
        );
        return;
    }
    for var in SCOPE_VARS {
        // Run time: a binary built before the policy existed.
        let mut run = Command::new(&bin);
        for v in SCOPE_VARS {
            run.env_remove(v);
        }
        let ran = run.env(var, "").output().unwrap();
        let err = String::from_utf8_lossy(&ran.stderr);
        assert_eq!(
            ran.status.code(),
            Some(2),
            "ATTACK: a compiled binary ran under {var}= instead of refusing it: {err}"
        );
        assert!(err.contains(var), "{var}: {err}");
        assert!(!target.exists(), "{var}: the binary wrote the file");

        // Build time: refuse to emit while the scope is set.
        let out = d.join(format!("refused_{var}"));
        let mut b = Command::new(env!("CARGO_BIN_EXE_axon"));
        b.args(["build", src.to_str().unwrap(), "-o", out.to_str().unwrap()])
            .env_remove("AXON_ALLOWED_EFFECTS")
            .env_remove("AXON_BUDGET_TOKENS");
        for v in SCOPE_VARS {
            b.env_remove(v);
        }
        let o = b.env(var, "").output().unwrap();
        let err = String::from_utf8_lossy(&o.stderr);
        assert_eq!(
            o.status.code(),
            Some(2),
            "ATTACK: `axon build` emitted a binary while {var} was set: {err}"
        );
        assert!(err.contains("E0910") && err.contains(var), "{var}: {err}");
        assert!(!out.exists(), "{var}: a binary was emitted");
    }
    // CONTROL: unconstrained, the binary still runs.
    let mut free = Command::new(&bin);
    for v in SCOPE_VARS {
        free.env_remove(v);
    }
    let f = free.output().unwrap();
    assert_eq!(
        f.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&f.stderr)
    );
    assert!(target.exists());
}
