//! Amendment 98 (C9 round 9, eqgate5): the GUARDS of `scripts/guest_build_env.py`
//! that no privileged build is needed to reach.
//!
//! The refusal-site gate read Rust only, and the controlled-build script is on
//! the claim's own list of protected paths: 46 of its guard lines matched no
//! mutation row, and no test set `AXON_GUEST_BUILD_UID` to 0 or to the
//! builder's own uid, so `int(raw) == 0` could be removed with every test green.
//! The gate now reads this file's guards too (`py_sites`); each test below is
//! the observation behind a row.
//!
//! Every test drives the REAL script's functions (`guest_build_env as g`), from a
//! scratch copy of THIS tree's script, inside a private PID namespace (as the
//! existing `guest_build_env.rs` does: the script kills processes of the build
//! uid after a build step, and nothing here runs a step, but no test may run
//! the script where it could see the host's processes). Each case names its own
//! guard (`ATTACK: gbe <case>`) and isolates it: every OTHER guard on the route
//! is satisfied, so the case fails only if THIS guard is gone.

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use script_spawn::repo_root;
#[path = "uid_claim/mod.rs"]
#[allow(dead_code)]
mod uid_claim;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use uid_claim::UidClaim;

/// Where scratch trees live: reachable by the unprivileged uids a case runs as.
const TEST_ROOT: &str = "/var/lib";

fn scratch() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::Builder::new()
        .prefix("axon-gbe-guards-")
        .tempdir_in(TEST_ROOT)
        .unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    d
}

fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

/// A scratch ROOT: `<d>/scripts/guest_build_env.py` (a copy of THIS tree's) and
/// whatever the case adds beside it (`rust-toolchain.toml`, a git repository).
fn tree(d: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let s = d.join("scripts");
    std::fs::create_dir_all(&s).unwrap();
    std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o755)).unwrap();
    let f = s.join("guest_build_env.py");
    std::fs::copy(repo_root().join("scripts/guest_build_env.py"), &f).unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o644)).unwrap();
    d.to_path_buf()
}

/// The Python prelude: `g` is the script, `run(fn, ...)` returns the value or
/// `EXIT:<message>` (a refusal is a `SystemExit`), `ok(x)` marks a clean value.
const PRELUDE: &str = r#"
import json, os, sys, stat, tempfile, subprocess, hashlib, hmac
sys.path.insert(0, sys.argv[1])
import guest_build_env as g
OUT = []
def run(fn, *a, **k):
    try:
        v = fn(*a, **k)
    except SystemExit as e:
        return "EXIT:" + str(e.code)
    return v
def norm(v):
    if isinstance(v, (tuple, list)):
        return [norm(x) for x in v]
    return v
class Eq:
    """An exact string: a plain string `want` is a substring a refusal must carry."""
    def __init__(self, v):
        self.v = v
def chk(name, v, want=""):
    v = norm(v)
    if isinstance(want, Eq):
        OUT.append([name, v == want.v, str(v)[:300]])
        return
    """`want` is "" (the guard says nothing: a clean value), a substring the
    refusal must carry, or any other value the result must equal."""
    if want == "":
        ok = v in ("", None)
    elif isinstance(want, str):
        ok = want in str(v)
    else:
        ok = v == want
    OUT.append([name, bool(ok), str(v)[:300]])
def done():
    print("GBE-CASES " + json.dumps(OUT))
def exec_cases(code):
    """Each top-level statement alone: a statement that RAISES (a guard removed
    lets a malformed value through to a later TypeError, say) is the case
    failing, named by its `chk(..)`, never a driver crash that hides the attack."""
    import ast
    tree = ast.parse(code)
    env = globals()
    for node in tree.body:
        try:
            exec(compile(ast.Module([node], []), "<case>", "exec"), env)
        except BaseException as e:
            name = "line %d" % node.lineno
            v = getattr(node, "value", None)
            if isinstance(v, ast.Call) and getattr(v.func, "id", "") == "chk" and v.args \
                    and isinstance(v.args[0], ast.Constant):
                name = v.args[0].value
            OUT.append([name, False, "raised " + repr(e)[:250]])
os.umask(0o022)
S = os.environ.get("SCR", "")
def mkdir(p, mode=0o755, uid=0):
    os.makedirs(p, exist_ok=True)
    os.chmod(p, mode)
    os.chown(p, uid, uid)
def put(p, text="", mode=0o644, uid=0):
    if not os.path.isdir(os.path.dirname(p)):
        mkdir(os.path.dirname(p), 0o755)
    with open(p, "w") as f:
        f.write(text)
    os.chmod(p, mode)
    os.chown(p, uid, uid)
"#;

/// Run `code` (after the prelude) in a private PID namespace, as root or as
/// `uid`; the cases it prints as one JSON object.
fn py(root: &Path, code: &str, uid: Option<u32>) -> Value {
    // Amendment 111: no case names a uid of its own. `GBE_UID2` is a claimed, process-free uid for any
    // case that starts a process as one (it is visible to every host-namespace observer); the uid a
    // case RUNS as (`uid`) is a claim too, handed to the case as `GBE_UID`.
    let spare = UidClaim::new();
    let mut c = Command::new("/usr/bin/unshare");
    c.args(["--pid", "--fork", "--mount-proc", "--kill-child"]);
    if let Some(u) = uid {
        c.args([
            "/usr/bin/setpriv",
            &format!("--reuid={u}"),
            &format!("--regid={u}"),
            "--clear-groups",
            "--",
        ]);
    }
    c.args([
        "python3",
        "-B",
        "-c",
        &format!("{PRELUDE}\nexec_cases(sys.argv[2])\ndone()\n"),
    ])
    .arg(root.join("scripts"))
    .arg(code)
    .env_clear()
    .env("PATH", "/usr/bin:/bin")
    .env("LC_ALL", "C")
    .env("SCR", root)
    .env("GBE_UID", uid.map(|u| u.to_string()).unwrap_or_default())
    .env("GBE_UID2", spare.uid.to_string());
    let o: Output = c.output().unwrap();
    let t = String::from_utf8_lossy(&o.stdout).to_string();
    let line = t
        .lines()
        .find_map(|l| l.strip_prefix("GBE-CASES "))
        .unwrap_or_else(|| {
            panic!(
                "setup: the driver printed no cases (rc {:?}): {t}{}",
                o.status.code(),
                String::from_utf8_lossy(&o.stderr)
            )
        });
    serde_json::from_str(line).unwrap()
}

/// Every case the driver ran, in order: the first whose guard did not hold is
/// the attack that got through (`ATTACK: gbe <case>`).
#[track_caller]
fn all_hold(cases: &Value) {
    let rows = cases.as_array().expect("setup: the driver's cases");
    assert!(!rows.is_empty(), "setup: the driver ran no case");
    for r in rows {
        let (name, ok, got) = (
            r[0].as_str().unwrap(),
            r[1].as_bool().unwrap(),
            r[2].as_str().unwrap(),
        );
        assert!(ok, "ATTACK: gbe {name}: the guard did not hold: {got}");
    }
}

/// Amendment 111: where a test names a uid LITERALLY for something that runs as it, or hands it to the controlled build as the
/// build uid. `begin` refuses a build uid that "already owns running processes" by reading the HOST /proc, so a literal uid is
/// one any other test, binary or agent on the host can collide with (`the_build_uid_lock_is_root_owned_and_begin_holds_it` failed
/// 3 of 14 whole-binary runs that way). Such a uid comes from `uid_claim` (a flock-claimed, process-free uid per test thread).
fn literal_uid_sites(text: &str) -> Vec<String> {
    let mut out = vec![];
    let digit_after = |rest: &str| {
        rest.trim_start_matches(|c: char| " \"'],=()".contains(c))
            .starts_with(|c: char| c.is_ascii_digit())
    };
    for (i, _) in text.match_indices("--reuid=") {
        if text[i + 8..].starts_with(|c: char| c.is_ascii_digit()) {
            out.push(
                text[i..(i + 24).min(text.len())]
                    .lines()
                    .next()
                    .unwrap()
                    .to_string(),
            );
        }
    }
    for (i, _) in text.match_indices("AXON_GUEST_BUILD_UID") {
        if digit_after(&text[i + 20..]) {
            out.push(
                text[i..(i + 40).min(text.len())]
                    .lines()
                    .next()
                    .unwrap()
                    .to_string(),
            );
        }
    }
    out
}

#[test]
fn no_test_gives_a_process_or_the_build_uid_a_literal_uid() {
    // control: the shapes this refuses are seen
    for planted in [
        "setpriv --reuid=4242 --regid=4242",
        "os.environ[\"AXON_GUEST_BUILD_UID\"] = \"4242\"",
        ".env(\"AXON_GUEST_BUILD_UID\", \"4242\")",
    ] {
        assert!(
            !literal_uid_sites(planted).is_empty(),
            "setup: the literal-uid scan missed {planted:?}"
        );
    }
    for fine in [
        "setpriv --reuid=$BUID",
        "\"--reuid=%d\" % X",
        ".env(\"AXON_GUEST_BUILD_UID\", test_uid().to_string())",
        "os.environ.pop(\"AXON_GUEST_BUILD_UID\", None)",
        "os.environ[\"AXON_GUEST_BUILD_UID\"] = \"{uid}\"",
    ] {
        assert!(
            literal_uid_sites(fine).is_empty(),
            "setup: the literal-uid scan refused {fine:?}"
        );
    }
    let t = repo_root().join("crates/axon-fabric/tests");
    let mut files = vec![
        t.join("guest_build_env.rs"),
        t.join("guest_build_env_guards.rs"),
    ];
    for e in std::fs::read_dir(t.join("guest_build_env_guards")).unwrap() {
        let p = e.unwrap().path();
        // ids_root.py compares an ARGV VALUE (the command as_build_uid would run); it starts nothing
        if p.extension().is_some_and(|x| x == "py") && !p.ends_with("ids_root.py") {
            files.push(p);
        }
    }
    let mut bad = vec![];
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        // this test's own planted strings are not sites
        let text = text
            .split("fn no_test_gives_a_process_or_the_build_uid_a_literal_uid")
            .next()
            .unwrap();
        for site in literal_uid_sites(text) {
            bad.push(format!("{}: {site}", f.display()));
        }
    }
    assert!(
        bad.is_empty(),
        "ATTACK: a test names a literal uid for a process or the build uid (use uid_claim): {bad:?}"
    );
}

// ── the cases ───────────────────────────────────────────────────────────────

macro_rules! cases {
    ($($f:literal),+) => {
        concat!($(include_str!(concat!("guest_build_env_guards/", $f)), "\n"),+)
    };
}

fn run_cases(code: &str, uid: Option<u32>) {
    if !is_root() {
        eprintln!("skipped: needs root (a PID namespace, setpriv, chown)");
        return;
    }
    let d = scratch();
    let r = tree(d.path());
    all_hold(&py(&r, code, uid));
}

/// `build_ids()`, `require_runner()`: who the build runs as (as a claimed uid).
#[test]
fn the_build_uid_is_unprivileged_and_not_the_builders_own() {
    let me = UidClaim::new();
    run_cases(cases!("ids_user.py"), Some(me.uid));
}

#[test]
fn the_controlled_build_runs_only_as_root_as_an_unprivileged_uid() {
    run_cases(cases!("ids_root.py"), None);
}

/// `ancestors_of`, `ancestors_problem`, `build_parent`, `reach_problem`,
/// `operator_file_problem`: where the build lives and who may write it.
#[test]
fn the_build_parent_and_the_operator_files_are_judged_by_who_can_write_them() {
    run_cases(cases!("paths.py"), None);
}

/// The builder pin, the per-build proof key, the proof over a record, the
/// record's shape, the host build's record, the dist and rootfs judges.
#[test]
fn a_record_is_judged_by_its_pin_its_proof_its_shape_and_its_bytes() {
    run_cases(
        cases!("pins.py", "shape.py", "hostrec.py", "build.py", "cargo.py"),
        None,
    );
}

/// `pinned_channel`, `rustup`, `toolchain`: the pinned toolchain is resolved by
/// rustup under a cleared environment, and nothing else.
#[test]
fn the_pinned_toolchain_is_resolved_through_rustup_and_nothing_else() {
    run_cases(cases!("env.py"), None);
}

/// The tree copy, the clone, the committed file, the private toolchain, the
/// command line and the operator's host-toolchain pin.
#[test]
fn the_tree_copy_the_clone_the_toolchain_pin_and_the_command_line_refuse() {
    run_cases(cases!("misc.py"), None);
}

/// The constructed environments and the host tools' identities (VALUES: what
/// cargo, make and mksquashfs may see, and where a tool is taken from).
#[test]
fn the_constructed_environments_and_the_host_tool_identities_are_the_documented_ones() {
    run_cases(cases!("values.py"), None);
}

/// Amendment 101: the build uid's dedication is judged against EVERY shape a service's identity takes (plural
/// and string uids, any letter case, subdirectories and non-.json names under /etc/axon, quoted and drop-in
/// `User=`, units not named axon-* that run an axon binary, `Group=` and `*_gid` for the build GID), a file that
/// cannot be read or parsed REFUSES instead of being skipped, `DynamicUser` refuses, and a build uid that already
/// owns running processes is refused. Amendment 109 adds the keys in any spelling (`ownerUid`, `owner-uid`,
/// `runAsUser`, `username`), TOML inline tables and multi-line arrays, YAML flow maps and a value on the next
/// line, a uid-named key of no known class holding a number, a strict key with no readable value, and an
/// unreadable drop-in directory (a clean refusal, not a traceback): it reads the LISTED shapes and fails
/// closed on the rest, not "every shape".
#[test]
fn the_service_accounts_are_found_in_every_shape_and_an_unreadable_file_refuses() {
    run_cases(cases!("service_ids.py"), None);
}
