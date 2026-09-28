//! Dev review round wf_bf757240-925 (FIELD-ORIGIN): the pinned launcher runs
//! as root, and its inline Python must never import from the caller's working
//! directory (`sys.path[0]` is the cwd for `python3 -c` / `python3 -`). Every
//! call is isolated (`python3 -I`). Control: the same launcher with `-I`
//! stripped does import the planted module.

use std::path::Path;
use std::process::Command;

fn run_from_hostile_cwd(launcher: &Path) -> bool {
    let d = tempfile::tempdir().unwrap();
    let cwd = d.path().join("cwd");
    let vd = d.path().join("vd");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&vd).unwrap();
    let marker = d.path().join("imported-from-cwd");
    std::fs::write(
        cwd.join("json.py"),
        format!("open({:?}, 'w').write('x')\n", marker.display().to_string()),
    )
    .unwrap();
    for f in ["workspace.img", "serial.log", "result.json"] {
        std::fs::write(vd.join(f), "{}").unwrap();
    }
    let _ = Command::new("bash")
        .arg(launcher)
        .arg("--verify-result")
        .arg(&vd)
        .current_dir(&cwd)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .output()
        .unwrap();
    marker.exists()
}

#[test]
fn the_launchers_python_never_imports_from_the_callers_cwd() {
    let launcher = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/fc_linux_profile.sh");
    assert!(
        !run_from_hostile_cwd(&launcher),
        "the launcher imported a module from the caller's working directory"
    );
    // Control: without -I the planted module IS imported.
    let d = tempfile::tempdir().unwrap();
    let weak = d.path().join("launcher-no-isolation.sh");
    std::fs::write(
        &weak,
        std::fs::read_to_string(&launcher)
            .unwrap()
            .replace("python3 -I -", "python3 -"),
    )
    .unwrap();
    assert!(
        run_from_hostile_cwd(&weak),
        "control: the planted module is imported without -I"
    );
}
