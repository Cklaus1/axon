//! Protected Check Isolation: surfaces probed at the language level, through
//! the real binary, with the module layout Fabric uses (suite directory first,
//! then the candidate, `AXON_PATH_EXCLUSIVE=1`).
//! (`governance/specs/v022-protected-check-isolation.md`.)

use std::process::Command;

/// Run `axon test --json` on `suite/h.ax` with the given suite files and
/// candidate `f.ax`; return the first result line.
fn run(tag: &str, suite: &[(&str, &str)], cand: &str) -> String {
    let d = std::env::temp_dir().join(format!("axon_pci_{}_{tag}", std::process::id()));
    let (s, c) = (d.join("suite"), d.join("cand"));
    std::fs::create_dir_all(&s).unwrap();
    std::fs::create_dir_all(&c).unwrap();
    for (name, src) in suite {
        std::fs::write(s.join(name), src).unwrap();
    }
    std::fs::write(c.join("f.ax"), cand).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .args(["test", "--json"])
        .arg(s.join("h.ax"))
        .env("AXON_PATH", format!("{}:{}", s.display(), c.display()))
        .env("AXON_PATH_EXCLUSIVE", "1")
        .output()
        .unwrap();
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    text.lines().next().unwrap_or("").to_string()
}

/// PCI 10: a named handler is desugared per FILE at parse time, so a candidate
/// defining a handler of the same name cannot rebind the one the suite (or a
/// suite helper) uses — in either merge order. The wrong `double` (n * 3)
/// passes only if the candidate's `Fix` (resume 14) were the one applied.
#[test]
fn a_candidate_cannot_rebind_a_suite_named_handler() {
    let helper = "handler Fix = handler { on Random(p) => resume(21) }\n\
                  fn draw() -> i64 { with Fix { random_i64(0, 100) } }\n";
    let body = "\n\nhandler Fix = handler { on Random(p) => resume(21) }\n\n@[test]\n\
                fn hidden_completion() {\n    assert_eq(double(draw()), 42)\n    \
                let v = with Fix { random_i64(0, 100) }\n    assert_eq(double(v), 42)\n}\n";
    let hijack = "handler Fix = handler { on Random(p) => resume(14) }\n\
                  fn double(n: i64) -> i64 { n * 3 }\n";
    for (tag, head) in [
        ("helper_first", "mod g\nmod f\nuse g.{draw}\nuse f.{double}"),
        (
            "candidate_first",
            "mod f\nmod g\nuse f.{double}\nuse g.{draw}",
        ),
    ] {
        let h = format!("{head}{body}");
        let suite = [("g.ax", helper), ("h.ax", h.as_str())];
        let bad = run(tag, &suite, hijack);
        assert!(bad.contains("\"status\":\"failed\""), "{tag}: {bad}");
        let good = run(
            &format!("{tag}_ok"),
            &suite,
            "fn double(n: i64) -> i64 { n * 2 }\n",
        );
        assert!(good.contains("\"status\":\"ok\""), "{tag} control: {good}");
    }
}

/// PCI 4: with `AXON_PATH_EXCLUSIVE=1` a module resolves ONLY from `AXON_PATH`
/// — never from the ambient `~/.axon/lib` (a trial cache's HOME, which a
/// previous trial could have written). Pinned here, at the resolver, because
/// sealing (21) also refuses such a module's names end to end, so a Fabric
/// verdict alone no longer shows which guard held.
///
/// Mutation: ignore `AXON_PATH_EXCLUSIVE` in `axon_search_dirs` → red.
#[test]
fn an_exclusive_module_path_never_falls_through_to_ambient_dirs() {
    let d = std::env::temp_dir().join(format!("axon_pci_ambient_{}", std::process::id()));
    let (home, src) = (d.join("home"), d.join("src"));
    std::fs::create_dir_all(home.join(".axon/lib")).unwrap();
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(
        home.join(".axon/lib/planted.ax"),
        "fn d2(n: i64) -> i64 { n * 2 }\n",
    )
    .unwrap();
    std::fs::write(
        src.join("main.ax"),
        "mod planted\nuse planted.{d2}\nfn main() -> i64 { d2(21) }\n",
    )
    .unwrap();
    let run = |exclusive: bool| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_axon"));
        c.arg("check")
            .arg(src.join("main.ax"))
            .env("HOME", &home)
            .env("AXON_PATH", &src);
        if exclusive {
            c.env("AXON_PATH_EXCLUSIVE", "1");
        }
        c.output().unwrap()
    };
    // Control: without exclusivity the ambient module IS found.
    assert!(
        run(false).status.success(),
        "control: ambient lib not searched"
    );
    let out = run(true);
    let text =
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "the ambient module resolved: {text}");
    assert!(text.contains("not found"), "{text}");
}
