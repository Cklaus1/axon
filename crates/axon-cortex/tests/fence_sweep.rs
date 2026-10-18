//! R1f: the offline fence sweep (benchmarks/fence_sweep.py) passes only when
//! it swept something. A corpus with no task, or one that does not load,
//! proves nothing and must exit 2 — never 0 (a pass) and never 1 (which the
//! gate reads as a flip).

use std::path::{Path, PathBuf};
use std::process::Command;

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/fence_sweep.py")
}

/// Run the sweep on one corpus file: (exit code, stdout, stderr).
fn sweep(corpus: &Path) -> (i32, String, String) {
    // Never reached on these corpora; named so nothing resolves a default.
    let bins = [
        ("AXON_BIN", "/nonexistent/axon"),
        ("CORTEX_BIN", "/nonexistent/cortex"),
    ];
    let out = script_spawn::script("python3", script(), script_spawn::Bins::Named(&bins))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg("--corpus")
        .arg(corpus)
        .arg("--json")
        .output()
        .expect("run python3");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

fn corpus_file(name: &str, text: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("fence_sweep_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join("corpus.json");
    std::fs::write(&p, text).unwrap();
    p
}

#[test]
fn an_empty_corpus_is_not_a_pass() {
    let p = corpus_file("empty", r#"{"meta": {"sources": {}}, "tasks": []}"#);
    let (code, out, err) = sweep(&p);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("holds no task"), "{err}");
    let _ = std::fs::remove_dir_all(p.parent().unwrap());
}

#[test]
fn a_corpus_that_does_not_load_is_exit_2_not_a_flip() {
    let p = corpus_file("broken", "{ not json");
    let (code, out, err) = sweep(&p);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("does not load"), "{err}");
    let _ = std::fs::remove_dir_all(p.parent().unwrap());
}

/// Every task errors (no cortex to derive a grant): zero swept, exit 2.
#[test]
fn a_sweep_where_every_task_errored_is_exit_2() {
    // A corpus task whose derived texts are consistent, so it loads; the
    // sweep then fails to run `cortex grant` on it.
    let src = "fn double(n: i64) -> i64 {\n    n * 2\n}\n\n@[test]\nfn t() {\n    \
               assert_eq(double(2), 4)\n}\n";
    let gen = Command::new("python3")
        .arg("-B")
        .arg("-c")
        .arg(
            "import json, sys, hashlib\n\
             sys.path.insert(0, sys.argv[1])\n\
             import corpus\n\
             src = sys.argv[2]\n\
             t = {'id': 't1', 'file': 'f.ax', 'symbol': 'double', 'source_file': 'f.ax',\n\
                  'mutant_body': '\\n    n + 2\\n', 'hidden': 't'}\n\
             d = {'meta': {'sources': {'f.ax': src}}, 'tasks': [t]}\n\
             a, b = corpus.fn_span(src, 'double')\n\
             t2 = dict(t, pristine=src, oracle_body=src[a:b],\n\
                       broken=src[:a] + t['mutant_body'] + src[b:])\n\
             t2['given_a'], t2['hidden_block'] = corpus.cut_test(t2['broken'], 't')\n\
             t['sha256'] = {k: hashlib.sha256(t2[k].encode()).hexdigest() for k in corpus.DERIVED}\n\
             print(json.dumps(d))\n",
        )
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/ab"))
        .arg(src)
        .output()
        .expect("build corpus");
    assert!(
        gen.status.success(),
        "{}",
        String::from_utf8_lossy(&gen.stderr)
    );
    let p = corpus_file("errored", &String::from_utf8_lossy(&gen.stdout));
    let (code, out, err) = sweep(&p);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("ERROR"), "{err}");
    assert!(out.contains("\"pass\": false"), "{out}");
    let _ = std::fs::remove_dir_all(p.parent().unwrap());
}
