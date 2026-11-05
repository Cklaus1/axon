//! C-1 / C-4 (containment-b4, O-GV-08): the fs path scope RESOLVES.
//!
//! The scope (`AXON_ALLOWED_FS_READ/WRITE`, the ceiling's `fs_*` keys,
//! `sandbox_create_scoped`) tested only the path TEXT, and the host then
//! followed every link: under `./data/` a `./data/link -> /etc/hostname`
//! read the host's file, `/proc/self/root/...` under a `/proc/` grant reached
//! any file, and a `./out/odir -> elsewhere` link wrote outside `./out/`. All
//! exit 0. The fix resolves the path (symlinks followed, `/proc` magic links
//! refused) and requires the result under a grant's root, pinned when the
//! scope is built.
//!
//! C-4: a program sandbox created inside another could name a wider fs/net
//! scope and use it; now a call must be inside every enclosing sandbox's.
//!
//! The C-1 tests are Linux-only (the fixtures use `/proc` and symlinks); the
//! C-4 tests run everywhere except the one symlink sub-case.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const SCOPE_VARS: [&str; 3] = [
    "AXON_ALLOWED_FS_READ",
    "AXON_ALLOWED_FS_WRITE",
    "AXON_ALLOWED_NET",
];

const SECRET: &str = "s3cr3t-outside";

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon_sres_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `axon run` with the effect ceiling UNSET and exactly `vars` of the three
/// scope variables set; the provenance cache is kept inside `dir`.
fn run(dir: &Path, src: &str, vars: &[(&str, &str)]) -> Output {
    run_env(
        dir,
        src,
        vars,
        &[("XDG_CACHE_HOME", &lit(&dir.join("cache")))],
    )
}

fn run_env(dir: &Path, src: &str, vars: &[(&str, &str)], env: &[(&str, &str)]) -> Output {
    let prog = dir.join("p.ax");
    std::fs::write(&prog, src).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_axon"));
    cmd.arg("run")
        .arg(&prog)
        .current_dir(dir)
        .env_remove("AXON_ALLOWED_EFFECTS")
        .env_remove("AXON_CEILING");
    for v in SCOPE_VARS {
        cmd.env_remove(v);
    }
    for (k, v) in vars.iter().chain(env) {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string()
}

fn lit(p: &Path) -> String {
    p.display().to_string()
}

fn main_of(body: &str) -> String {
    format!("fn main() {{\n{body}\n}}\n")
}

/// `read_file(p)`, printing `READ:<text>` or `ERR:<why>`.
fn rd(p: &str) -> String {
    format!("match read_file(\"{p}\") {{ Ok(s) => print(\"READ:\" + s)  Err(e) => println(\"ERR:\" + e) }}")
}

/// The reproduction's fixture (`repro_gv08.sh`), with the outside target
/// inside the test's own directory so nothing outside it is read or written.
#[cfg(unix)]
fn fixture(tag: &str) -> PathBuf {
    use std::os::unix::fs::symlink;
    let w = tmp(tag);
    for d in ["data/realdir", "out", "shared", "outside"] {
        std::fs::create_dir_all(w.join(d)).unwrap();
    }
    std::fs::write(w.join("data/real.txt"), "in-prefix text\n").unwrap();
    std::fs::write(w.join("data/realdir/d.txt"), "deep\n").unwrap();
    std::fs::write(w.join("shared/s.txt"), "shared text\n").unwrap();
    std::fs::write(w.join("outside/secret.txt"), format!("{SECRET}\n")).unwrap();
    symlink(w.join("outside/secret.txt"), w.join("data/link")).unwrap();
    symlink(w.join("outside"), w.join("data/etcdir")).unwrap();
    symlink("real.txt", w.join("data/alias")).unwrap();
    symlink("realdir", w.join("data/dirlink")).unwrap();
    symlink("../shared/s.txt", w.join("data/cross")).unwrap();
    symlink(w.join("outside/victim.txt"), w.join("out/wlink")).unwrap();
    symlink(w.join("outside"), w.join("out/odir")).unwrap();
    symlink("newfile.txt", w.join("out/dangle_in")).unwrap();
    symlink(w.join("data"), w.join("datalink")).unwrap();
    w
}

/// rc 8, the resolved-escape reason, and no byte of the outside secret.
fn assert_refused(o: &Output, why: &str, attack: &str) {
    let t = text(o);
    assert!(
        o.status.code() == Some(8) && !t.contains(SECRET) && !t.contains("READ:"),
        "{attack}:\n{t}"
    );
    assert!(
        t.contains(&format!("resolves outside every granted prefix ({why})")),
        "the refusal did not name its reason ({why}):\n{t}"
    );
}

// ── C-1: escapes now refused ────────────────────────────────────────────────

#[cfg(target_os = "linux")]
#[test]
fn a_symlink_under_the_read_prefix_cannot_reach_outside() {
    let w = fixture("rlink");
    let attack = "ATTACK: a read through an in-prefix symlink reached a file outside the grant";
    for body in [
        rd("./data/link"),
        rd("./data/etcdir/secret.txt"),
        "match dir_list(\"./data/etcdir\") { Ok(v) => println(\"READ:\" + to_str(len(v)))  Err(e) => println(\"ERR:\" + e) }".to_string(),
        "println(\"READ:\" + to_str(file_exists(\"./data/etcdir/secret.txt\")))".to_string(),
        "match file_size(\"./data/link\") { Ok(n) => println(\"READ:\" + to_str(n))  Err(e) => println(\"ERR:\" + e) }".to_string(),
    ] {
        let o = run(&w, &main_of(&body), &[("AXON_ALLOWED_FS_READ", "./data/")]);
        assert_refused(&o, "through a symbolic link", &format!("{attack} ({body})"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn a_proc_grant_does_not_reach_the_root_through_a_magic_link() {
    let w = fixture("rproc");
    let attack = "ATTACK: /proc/self/root reached outside the grant";
    let root = format!("/proc/self/root{}", lit(&w.join("outside/secret.txt")));
    let o = run(
        &w,
        &main_of(&rd(&root)),
        &[("AXON_ALLOWED_FS_READ", "./data/,/proc/")],
    );
    assert_refused(&o, "through a /proc magic link", attack);
    let o = run(
        &w,
        &main_of(&rd("/proc/self/cwd/outside/secret.txt")),
        &[("AXON_ALLOWED_FS_READ", "/proc/self/")],
    );
    assert_refused(&o, "through a /proc magic link", attack);
}

#[cfg(target_os = "linux")]
#[test]
fn a_magic_link_into_the_grant_is_still_refused() {
    let w = fixture("rmagic_in");
    let p = format!("/proc/self/root{}", lit(&w.join("data/real.txt")));
    let o = run(
        &w,
        &main_of(&rd(&p)),
        &[("AXON_ALLOWED_FS_READ", "./data/,/proc/")],
    );
    let t = text(&o);
    assert!(
        o.status.code() == Some(8) && !t.contains("READ:"),
        "ATTACK: a /proc magic link was followed under a scope:\n{t}"
    );
    assert!(t.contains("(through a /proc magic link)"), "{t}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_write_through_proc_root_to_a_new_file_is_refused() {
    let w = fixture("wproc");
    let target = w.join("outside/pw.txt");
    let p = format!("/proc/self/root{}", lit(&target));
    let body = format!(
        "match write_file(\"{p}\", \"x\") {{ Ok(_) => println(\"W\")  Err(e) => println(\"ERR:\" + e) }}"
    );
    let o = run(
        &w,
        &main_of(&body),
        &[("AXON_ALLOWED_FS_WRITE", "./out/,/proc/")],
    );
    let t = text(&o);
    assert!(
        o.status.code() == Some(8) && !target.exists(),
        "ATTACK: a write through /proc/self/root created a file outside the grant:\n{t}"
    );
    assert!(t.contains("(through a /proc magic link)"), "{t}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_dangling_symlink_under_the_write_prefix_cannot_create_outside() {
    let w = fixture("wdangle");
    let o = run(
        &w,
        &main_of("match write_file(\"./out/wlink\", \"pwned\") { Ok(_) => println(\"WROTE\")  Err(e) => println(\"ERR:\" + e) }"),
        &[("AXON_ALLOWED_FS_WRITE", "./out/")],
    );
    let t = text(&o);
    assert!(
        o.status.code() == Some(8) && !w.join("outside/victim.txt").exists(),
        "ATTACK: a dangling link created a file outside the grant:\n{t}"
    );
    assert!(t.contains("(through a symbolic link)"), "{t}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_dir_symlink_under_the_write_prefix_cannot_write_outside() {
    let w = fixture("wdir");
    let attack = "ATTACK: a write through an in-prefix symlink landed outside the grant";
    let before = std::fs::read_to_string(w.join("outside/secret.txt")).unwrap();
    for (body, landed) in [
        (
            "match write_file(\"./out/odir/w2.txt\", \"pwned\") { Ok(_) => println(\"WROTE\")  Err(e) => println(\"ERR:\" + e) }",
            "outside/w2.txt",
        ),
        (
            "match dir_create(\"./out/odir/made\") { Ok(_) => println(\"MADE\")  Err(e) => println(\"ERR:\" + e) }",
            "outside/made",
        ),
        (
            "let _ = write_file(\"./out/r.txt\", \"r\")\nmatch file_rename(\"./out/r.txt\", \"./out/odir/r.txt\") { Ok(_) => println(\"RENAMED\")  Err(e) => println(\"ERR:\" + e) }",
            "outside/r.txt",
        ),
    ] {
        let o = run(&w, &main_of(body), &[("AXON_ALLOWED_FS_WRITE", "./out/")]);
        let t = text(&o);
        assert!(
            o.status.code() == Some(8) && !w.join(landed).exists(),
            "{attack} ({landed}):\n{t}"
        );
        assert!(t.contains("(through a symbolic link)"), "{t}");
    }
    let o = run(
        &w,
        &main_of("match append_file(\"./out/odir/secret.txt\", \"+pwned\") { Ok(_) => println(\"APPENDED\")  Err(e) => println(\"ERR:\" + e) }"),
        &[("AXON_ALLOWED_FS_WRITE", "./out/")],
    );
    let after = std::fs::read_to_string(w.join("outside/secret.txt")).unwrap();
    assert!(
        o.status.code() == Some(8) && after == before,
        "{attack} (append):\n{}",
        text(&o)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn file_copy_through_a_read_link_is_refused() {
    let w = fixture("wcopy");
    let o = run(
        &w,
        &main_of("match file_copy(\"./data/link\", \"./out/copied.txt\") { Ok(_) => println(\"COPIED\")  Err(e) => println(\"ERR:\" + e) }"),
        &[
            ("AXON_ALLOWED_FS_READ", "./data/"),
            ("AXON_ALLOWED_FS_WRITE", "./out/"),
        ],
    );
    let t = text(&o);
    assert!(
        o.status.code() == Some(8) && !w.join("out/copied.txt").exists(),
        "ATTACK: file_copy read a file outside the grant through a link:\n{t}"
    );
    assert!(t.contains("(through a symbolic link)"), "{t}");
}

#[cfg(target_os = "linux")]
#[test]
fn a_scoped_sandbox_resolves_symlinks_too() {
    let w = fixture("sb");
    let src = "fn r(n: i64) -> i64 {\n    match read_file(\"./data/link\") {\n        \
               Ok(s) => { print(\"READ:\" + s)\n n }\n        \
               Err(e) => { println(\"ERR:\" + e)\n n + 1 }\n    }\n}\n\
               fn main() {\n    let p = principal_root(\"p\", false, false, false, 100)\n    \
               let s = sandbox_create_scoped(p, \"IO\", \"./data/\", \"\", \"\")\n    \
               println(to_str(sandbox_run(s, \"r\", 1)))\n}\n";
    let o = run(&w, src, &[]);
    assert_refused(
        &o,
        "through a symbolic link",
        "ATTACK: a scoped sandbox read through an in-prefix symlink outside its grant",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_grant_root_swapped_for_a_symlink_mid_run_stays_pinned() {
    let w = fixture("pin");
    std::os::unix::fs::symlink(w.join("outside"), w.join("spare")).unwrap();
    let body = format!(
        "let _ = file_rename(\"./data\", \"./old\")\nlet _ = file_rename(\"./spare\", \"./data\")\n{}",
        rd("./data/secret.txt")
    );
    let o = run(
        &w,
        &main_of(&body),
        &[
            ("AXON_ALLOWED_FS_READ", "./data/"),
            ("AXON_ALLOWED_FS_WRITE", "./"),
        ],
    );
    // Premise: the renames happened, so `./data` now leads outside.
    assert!(w.join("old/real.txt").exists(), "{}", text(&o));
    assert_refused(
        &o,
        "through a symbolic link",
        "ATTACK: a renamed symlink re-pointed the grant",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_symlinked_store_dir_outside_the_write_prefix_is_refused() {
    let w = fixture("store");
    std::fs::create_dir_all(w.join("cache/axon")).unwrap();
    std::os::unix::fs::symlink(w.join("outside"), w.join("cache/axon/stores")).unwrap();
    let cache = lit(&w.join("cache"));
    let grant = format!("{cache}/");
    let o = run_env(
        &w,
        &main_of(
            "let h = dstore_open(\"k\", 0)\nlet _ = dstore_apply(h, 1, 7)\nprintln(\"APPLIED\")",
        ),
        &[("AXON_ALLOWED_FS_WRITE", &grant)],
        &[("XDG_CACHE_HOME", &cache)],
    );
    let t = text(&o);
    assert!(
        o.status.code() == Some(8) && !w.join("outside/k.ndjson").exists(),
        "ATTACK: the durable store wrote through a symlinked dir:\n{t}"
    );
    assert!(t.contains("(through a symbolic link)"), "{t}");
}

#[cfg(target_os = "linux")]
#[test]
fn the_provenance_row_is_suppressed_when_its_log_dir_resolves_outside() {
    let src = "@[adaptive]\nfn metric(x: i64) -> i64 { 10 - (x - 3) * (x - 3) }\n\
               fn main() { let _ = metric(3) }\n";
    let rows = |log: &Path| {
        std::fs::read_to_string(log)
            .map(|t| t.matches("\"event\":\"adaptive_return\"").count())
            .unwrap_or(0)
    };
    // Premise: a real cache dir under the grant gets its row.
    let w = fixture("prov_ok");
    std::fs::create_dir_all(w.join("cache/axon")).unwrap();
    let grant = format!("{}/", lit(&w.join("cache")));
    let o = run(&w, src, &[("AXON_ALLOWED_FS_WRITE", &grant)]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert_eq!(rows(&w.join("cache/axon/provenance.jsonl")), 1, "premise");

    let w = fixture("prov");
    std::fs::create_dir_all(w.join("cache")).unwrap();
    std::os::unix::fs::symlink(w.join("outside"), w.join("cache/axon")).unwrap();
    let grant = format!("{}/", lit(&w.join("cache")));
    let o = run(&w, src, &[("AXON_ALLOWED_FS_WRITE", &grant)]);
    assert_eq!(o.status.code(), Some(0), "{}", text(&o));
    assert_eq!(
        rows(&w.join("outside/provenance.jsonl")),
        0,
        "ATTACK: the provenance row was written through a symlinked log dir"
    );
}

// ── C-1: valid programs that must keep working (V1-V15) ─────────────────────

#[cfg(target_os = "linux")]
fn assert_ok(o: &Output, want: &str, what: &str) {
    assert!(
        o.status.code() == Some(0) && stdout(o) == want,
        "FALSE REFUSAL: {what}:\n{}",
        text(o)
    );
}

#[cfg(target_os = "linux")]
#[test]
fn an_in_prefix_file_symlink_still_reads() {
    let w = fixture("v1");
    let o = run(
        &w,
        &main_of(&rd("./data/alias")),
        &[("AXON_ALLOWED_FS_READ", "./data/")],
    );
    assert_ok(
        &o,
        "READ:in-prefix text\n",
        "an in-prefix file link was refused",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn an_in_prefix_dir_symlink_still_reads() {
    let w = fixture("v2");
    let o = run(
        &w,
        &main_of(&rd("./data/dirlink/d.txt")),
        &[("AXON_ALLOWED_FS_READ", "./data/")],
    );
    assert_ok(&o, "READ:deep\n", "an in-prefix dir link was refused");
}

#[cfg(target_os = "linux")]
#[test]
fn a_symlink_into_another_granted_prefix_still_reads() {
    let w = fixture("v3");
    let o = run(
        &w,
        &main_of(&rd("./data/cross")),
        &[("AXON_ALLOWED_FS_READ", "./data/,./shared/")],
    );
    assert_ok(
        &o,
        "READ:shared text\n",
        "a link between granted prefixes was refused",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_symlinked_grant_prefix_still_reads() {
    let w = fixture("v4");
    let o = run(
        &w,
        &main_of(&rd("./datalink/real.txt")),
        &[("AXON_ALLOWED_FS_READ", "./datalink/")],
    );
    assert_ok(
        &o,
        "READ:in-prefix text\n",
        "a symlinked grant prefix was refused",
    );
    let abs = lit(&w.join("datalink"));
    let o = run(
        &w,
        &main_of(&rd(&format!("{abs}/real.txt"))),
        &[("AXON_ALLOWED_FS_READ", &format!("{abs}/"))],
    );
    assert_ok(
        &o,
        "READ:in-prefix text\n",
        "an absolute symlinked grant prefix was refused",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn writing_through_an_in_prefix_dangling_link_still_works() {
    let w = fixture("v6");
    let o = run(
        &w,
        &main_of("match write_file(\"./out/dangle_in\", \"ok\") { Ok(_) => println(\"WROTE\")  Err(e) => println(\"ERR:\" + e) }"),
        &[("AXON_ALLOWED_FS_WRITE", "./out/")],
    );
    assert_ok(
        &o,
        "WROTE\n",
        "a write through an in-prefix dangling link was refused",
    );
    assert_eq!(
        std::fs::read_to_string(w.join("out/newfile.txt")).unwrap(),
        "ok"
    );
}

#[cfg(target_os = "linux")]
#[test]
fn renaming_a_symlink_inside_the_write_prefix_still_works() {
    let w = fixture("v7");
    let o = run(
        &w,
        &main_of("match file_rename(\"./out/wlink\", \"./out/wlink2\") { Ok(_) => println(\"RENAMED\")  Err(e) => println(\"ERR:\" + e) }"),
        &[("AXON_ALLOWED_FS_WRITE", "./out/")],
    );
    assert_ok(&o, "RENAMED\n", "renaming an in-prefix link was refused");
    assert!(w
        .join("out/wlink2")
        .symlink_metadata()
        .unwrap()
        .file_type()
        .is_symlink());
}

#[cfg(target_os = "linux")]
#[test]
fn a_missing_file_under_the_prefix_is_an_err_not_a_refusal() {
    let w = fixture("v8");
    let body = format!(
        "{}\nprintln(to_str(file_exists(\"./data/missing.txt\")))",
        "match read_file(\"./data/missing.txt\") { Ok(s) => print(\"READ:\" + s)  Err(_) => println(\"ERR\") }"
    );
    let o = run(&w, &main_of(&body), &[("AXON_ALLOWED_FS_READ", "./data/")]);
    assert_ok(&o, "ERR\nfalse\n", "a missing in-prefix file exited 8");
}

#[cfg(target_os = "linux")]
#[test]
fn procfs_files_under_a_proc_grant_still_read() {
    let w = fixture("v9");
    let mut body = String::new();
    for p in ["/proc/self/environ", "/proc/self/cmdline", "/proc/cpuinfo"] {
        body.push_str(&format!(
            "match read_file(\"{p}\") {{ Ok(_) => println(\"OK\")  Err(e) => println(\"ERR:\" + e) }}\n"
        ));
    }
    let o = run(&w, &main_of(&body), &[("AXON_ALLOWED_FS_READ", "/proc/")]);
    assert_ok(
        &o,
        "OK\nOK\nOK\n",
        "a procfs file under a /proc/ grant was refused",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn an_unscoped_run_resolves_nothing() {
    let w = fixture("v10");
    let o = run(
        &w,
        &main_of("match read_file(\"/proc/self/root/etc/hostname\") { Ok(_) => println(\"OK\")  Err(e) => println(\"ERR:\" + e) }"),
        &[],
    );
    assert_ok(
        &o,
        "OK\n",
        "an unscoped run was refused a /proc/self/root read",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_symlink_loop_is_an_err_not_a_refusal() {
    let w = fixture("v11");
    std::os::unix::fs::symlink("loop", w.join("data/loop")).unwrap();
    let o = run(
        &w,
        &main_of("match read_file(\"./data/loop\") { Ok(_) => println(\"READ\")  Err(_) => println(\"ERR\") }"),
        &[("AXON_ALLOWED_FS_READ", "./data/")],
    );
    assert_ok(&o, "ERR\n", "a link loop exited 8 instead of returning Err");
}

#[cfg(target_os = "linux")]
#[test]
fn nested_dir_create_in_prefix_still_works() {
    let w = fixture("v12");
    let o = run(
        &w,
        &main_of("match dir_create(\"./out/a/b/c\") { Ok(_) => println(\"MADE\")  Err(e) => println(\"ERR:\" + e) }\nmatch write_file(\"./out/a/b/c/f.txt\", \"f\") { Ok(_) => println(\"WROTE\")  Err(e) => println(\"ERR:\" + e) }"),
        &[("AXON_ALLOWED_FS_WRITE", "./out/")],
    );
    assert_ok(
        &o,
        "MADE\nWROTE\n",
        "a nested dir_create in the prefix was refused",
    );
    assert!(w.join("out/a/b/c/f.txt").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn a_grant_created_during_the_run_admits_writes() {
    let w = tmp("v13");
    let o = run(
        &w,
        &main_of("match dir_create(\"./out/\") { Ok(_) => println(\"MADE\")  Err(e) => println(\"ERR:\" + e) }\nmatch write_file(\"./out/x\", \"x\") { Ok(_) => println(\"WROTE\")  Err(e) => println(\"ERR:\" + e) }"),
        &[("AXON_ALLOWED_FS_WRITE", "./out/")],
    );
    assert_ok(
        &o,
        "MADE\nWROTE\n",
        "a grant created during the run refused writes",
    );
    assert!(w.join("out/x").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn a_scoped_sandbox_still_reads_in_prefix_links() {
    let w = fixture("v14");
    let src = "fn r(n: i64) -> i64 {\n    match read_file(\"./data/alias\") {\n        \
               Ok(s) => { print(\"READ:\" + s)\n n }\n        \
               Err(e) => { println(\"ERR:\" + e)\n n + 1 }\n    }\n}\n\
               fn main() {\n    let p = principal_root(\"p\", false, false, false, 100)\n    \
               let s = sandbox_create_scoped(p, \"IO\", \"./data/\", \"\", \"\")\n    \
               println(to_str(sandbox_run(s, \"r\", 1)))\n}\n";
    let o = run(&w, src, &[]);
    assert_ok(
        &o,
        "READ:in-prefix text\n1\n",
        "a scoped sandbox refused an in-prefix link",
    );
}

#[cfg(target_os = "linux")]
#[test]
fn a_trailing_slash_grant_root_argument_still_works() {
    let w = tmp("v15");
    std::fs::create_dir_all(w.join("data")).unwrap();
    std::fs::write(w.join("data/a.txt"), "a").unwrap();
    std::fs::write(w.join("data/b.txt"), "b").unwrap();
    let list = |p: &str| {
        format!("match dir_list(\"{p}\") {{ Ok(v) => println(to_str(len(v)))  Err(e) => println(\"ERR:\" + e) }}")
    };
    let what = "the grant root itself was refused";
    let o = run(
        &w,
        &main_of(&list("./data/")),
        &[("AXON_ALLOWED_FS_READ", "./data/")],
    );
    assert_ok(&o, "2\n", what);
    let abs = format!("{}/", lit(&w.join("data")));
    let o = run(&w, &main_of(&list(&abs)), &[("AXON_ALLOWED_FS_READ", &abs)]);
    assert_ok(&o, "2\n", what);
    let o = run(
        &w,
        &main_of("match dir_create(\"./out2/\") { Ok(_) => println(\"MADE\")  Err(e) => println(\"ERR:\" + e) }"),
        &[("AXON_ALLOWED_FS_WRITE", "./out2/")],
    );
    assert_ok(&o, "MADE\n", what);
}
