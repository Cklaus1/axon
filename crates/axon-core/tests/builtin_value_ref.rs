//! Aliasing a capability-bearing builtin must be refused exactly as calling it is.
//!
//! `let f = write_file` inside `@[contained(fs: [])]` was E1001, while
//! `let f = file_copy` passed with exit 0 — which is the laundering route the
//! check exists to close, left open in the one builtin that reads one path and
//! writes another. Two separate causes, reproduced together:
//!
//!   * `check_builtin_value_ref` asked `capability_of_builtin`, which yields
//!     ONE kind per call and has no arm for `file_copy`/`file_rename`, so it
//!     returned early for both;
//!   * its match on the capability LABEL ended in `_ => false`, silently
//!     permitting every label it did not name. `env_var` — whose direct call
//!     is an unconditional E1001, because the environment is an ungrantable
//!     ambient secret channel — could therefore be aliased and called, and so
//!     could the durable store.

use std::process::Command;

fn check(src: &str) -> (i32, String) {
    // A unique file per call. Sharing one path across tests in the same
    // process let them overwrite each other's source: two tests failed
    // reporting that `write_file` aliasing "was permitted" with EMPTY
    // diagnostic output, which is what a check of somebody else's program
    // looks like.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("axon_vref_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join(format!("t{}.ax", N.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&f, src).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .arg("check")
        .arg(&f)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr),
    )
}

fn contained(body: &str) -> String {
    format!("@[contained(fs: [], net: [], exec: none)]\nfn f() -> i64 {{\n{body}\n    0\n}}\nfn main() {{ let _ = f() }}\n")
}

#[test]
fn aliasing_an_ungranted_builtin_is_refused_for_every_capability_kind() {
    for (name, body) in [
        ("write_file", "    let g = write_file"),
        ("file_copy", "    let g = file_copy"),
        ("file_rename", "    let g = file_rename"),
        ("env_var", "    let g = env_var"),
        ("dstore_open", "    let g = dstore_open"),
    ] {
        let (code, out) = check(&contained(body));
        assert_eq!(code, 2, "aliasing `{name}` was permitted:\n{out}");
        assert!(
            out.contains("E1001"),
            "wrong diagnostic for `{name}`:\n{out}"
        );
        assert!(
            out.contains(name),
            "the diagnostic does not name `{name}`:\n{out}"
        );
    }
}

/// The only refusal a builtin alias may draw when its capability IS granted:
/// AX-25's E0306 (a builtin is not a first-class value — the interpreter has no
/// value for it and panicked "undefined identifier" at run time). Any E1001 /
/// E1004 here would be the capability guard over-refusing.
fn assert_refused_only_as_a_non_value(out: &str) {
    let errors: Vec<&str> = out
        .lines()
        .filter(|l| l.contains("\"severity\":\"error\""))
        .collect();
    assert!(
        !errors.is_empty() && errors.iter().all(|l| l.contains("\"code\":\"E0306\"")),
        "a granted alias must draw only the not-a-value E0306, no capability error:\n{out}"
    );
}

/// CONTROL: a grant covering every capability the alias confers must permit
/// it, or the fix is "refuse all aliasing". Since AX-25 the program is still
/// refused — with E0306, because a builtin is not a value — but the capability
/// guard itself must stay silent.
#[test]
fn a_fully_granted_alias_draws_no_capability_error() {
    let src = "@[contained(fs: [read(\"./\"), write(\"./out/\")], net: [], exec: none)]\n\
               fn f() -> i64 {\n    let g = file_copy\n    let _ = g(\"./a.txt\", \"./out/b.txt\")\n    0\n}\n\
               fn main() { let _ = f() }\n";
    let (code, out) = check(src);
    assert_eq!(code, 2, "a builtin is not a first-class value (E0306):\n{out}");
    assert_refused_only_as_a_non_value(&out);
}

/// CONTROL, and the sharper one: `file_copy` confers a READ and a WRITE, so a
/// read-only grant must still refuse it — naming the missing half. A guard
/// that accepted this would be checking only the first argument.
#[test]
fn a_read_only_grant_does_not_permit_aliasing_a_copy() {
    let src = "@[contained(fs: [read(\"./\")], net: [], exec: none)]\n\
               fn f() -> i64 {\n    let g = file_copy\n    let _ = g(\"./a.txt\", \"./out/b.txt\")\n    0\n}\n\
               fn main() { let _ = f() }\n";
    let (code, out) = check(src);
    assert_eq!(
        code, 2,
        "a read-only grant permitted an aliased copy:\n{out}"
    );
    assert!(
        out.contains("fs:write"),
        "the diagnostic must name the missing half:\n{out}"
    );
}

/// The audit ledger must record `file_copy`/`file_rename` as FS.
///
/// `audit_effect_kind` preferred `capability_of_builtin`, which returns None
/// for both, so they fell back to the coarse `IO` of their effect row. A
/// reviewer filtering the ledger for filesystem activity did not see the one
/// builtin that reads one path and writes another.
#[test]
fn a_copy_and_a_rename_audit_as_filesystem_effects() {
    let d = std::env::temp_dir().join(format!("axon_vref_audit_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("src.txt"), "hello\n").unwrap();
    let prog = d.join("a.ax");
    std::fs::write(
        &prog,
        format!(
            "fn main() {{\n    let _ = file_copy(\"{0}/src.txt\", \"{0}/dst.txt\")\n\
                 let _ = file_rename(\"{0}/dst.txt\", \"{0}/moved.txt\")\n}}\n",
            d.display()
        ),
    )
    .unwrap();
    let ledger = d.join("ledger.jsonl");
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .arg("run")
        .arg(&prog)
        .env("AXON_AUDIT_LEDGER", &ledger)
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "premise: the program must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = std::fs::read_to_string(&ledger).expect("ledger written");
    for op in ["file_copy", "file_rename"] {
        let row = text
            .lines()
            .find(|l| l.contains(op))
            .unwrap_or_else(|| panic!("no audit row for `{op}`:\n{text}"));
        // Asserted on the row's own text rather than a guessed field name: a
        // wrong key would make this pass by finding nothing.
        assert!(
            row.contains("\"effect\":\"FS\"") || row.contains("\"effect\": \"FS\""),
            "`{op}` did not audit as FS: {row}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// A `never:` clause is a HARD deny that overrides the allowlist, and the
/// value-ref check consulted only the allowlists.
///
/// Under `never: [exec, net("*")]` with both otherwise granted, the direct
/// calls produced two E1004s and the aliases produced exit 0. An alias has no
/// call site left at which to path/host-check, so if anything must refuse it,
/// a hard deny must.
#[test]
fn a_never_clause_forbids_aliasing_the_capability_it_denies() {
    let spec = "@[contained(fs: [read(\"./\")], net: [\"api.example.com\"], exec: any, \
                never: [exec, net(\"*\")])]";

    // Premise: the DIRECT calls are refused, or the never clause is not in
    // force and the alias result below would mean nothing.
    let (code, out) = check(&format!(
        "{spec}\nfn f() -> i64 {{\n    let _ = exec(\"ls\", [])\n    \
         let _ = http_get(\"api.example.com/x\")\n    0\n}}\nfn main() {{ let _ = f() }}\n"
    ));
    assert_eq!(
        code, 2,
        "premise: direct calls must hit the never clause:\n{out}"
    );
    assert!(out.contains("E1004"), "premise: expected E1004:\n{out}");

    let (code, out) = check(&format!(
        "{spec}\nfn f() -> i64 {{\n    let g = exec\n    let h = http_get\n    0\n}}\n\
         fn main() {{ let _ = f() }}\n"
    ));
    assert_eq!(
        code, 2,
        "aliasing a never-denied capability was permitted:\n{out}"
    );
}

/// CONTROL: a `never:` clause naming a PATH must not forbid the alias. That
/// clause denies a path, and the whole reason to refuse an alias is that no
/// path is knowable — so only a clause denying a WHOLE capability can decide
/// a question with no argument in it. (The alias itself is E0306 since AX-25;
/// the capability guard must add nothing.)
#[test]
fn a_path_scoped_never_clause_adds_no_capability_error_to_an_alias() {
    let (code, out) = check(
        "@[contained(fs: [read(\"./\")], net: [], exec: none, never: [read(\"/etc/\")])]\n\
         fn f() -> i64 {\n    let g = read_file\n    0\n}\nfn main() { let _ = f() }\n",
    );
    assert_eq!(code, 2, "a builtin is not a first-class value (E0306):\n{out}");
    assert_refused_only_as_a_non_value(&out);
}
