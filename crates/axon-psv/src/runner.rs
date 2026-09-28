//! The trusted guest verdict runner (`v022-psv-protocol.md` §4–§5).
//!
//! Order is the security property; nothing is executed before every check
//! passes:
//! 1. read the per-attempt secret `S`. It stays in THIS process. The child
//!    only ever receives the derived key `K`, and candidate code never
//!    receives either;
//! 2. verify the launch manifest: exactly the bytes Fabric named on the kernel
//!    command line, canonical, and for the protected profile;
//! 3. re-digest the candidate and the suite trees, and compare each with the
//!    manifest;
//! 4. verify the test identity: the entry exists in the suite tree, and the
//!    test name is a plain identifier;
//! 5. only then run `axon test` of EXACTLY that entry and test:
//!    * `K` is written to its stdin as one line and the pipe is closed, so the
//!      interpreter consumes it before any program code runs and program code
//!      finds EOF;
//!    * no other file descriptor is inherited;
//!    * the environment is cleared and holds no secret;
//!    * with `drop` set, the child runs as an unprivileged uid with no
//!      supplementary groups and no-new-privs, so it cannot read the secret
//!      file (root, 0400) or ptrace this process;
//! 6. write the verdict. Its `status` is a CLAIM: Fabric re-derives the verdict
//!    from the raw output with the certified parser and its OWN key (M2).

use crate::*;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Where everything is. In the guest these are fixed paths
/// (`src/bin/axon-psv-runner.rs`); tests pass their own.
#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub manifest: PathBuf,
    pub secret: PathBuf,
    pub candidate: PathBuf,
    pub suite: PathBuf,
    pub out: PathBuf,
    pub axon: PathBuf,
    /// The runner's own executable (its sha256 goes in the verdict).
    pub runner_exe: PathBuf,
    /// The manifest digest Fabric named (kernel command line).
    pub expected_manifest_sha256: String,
    /// Run the test as this uid:gid (in the guest: always set).
    pub drop: Option<(u32, u32)>,
    /// The guest policy's effect ceiling, passed through unchanged.
    pub effect_ceiling: Option<String>,
}

fn refused(
    cfg: &RunnerConfig,
    m_sha: &str,
    inputs: InputCheck,
    test: &str,
    why: String,
) -> GuestVerdict {
    GuestVerdict {
        schema: GUEST_VERDICT_SCHEMA.into(),
        launch_manifest_sha256: m_sha.into(),
        inputs,
        test: test.into(),
        status: GuestStatus::Refused,
        refusal: Some(why),
        exit_code: None,
        report: None,
        runner: runner_identity(cfg),
        stdout_sha256: None,
    }
}

fn runner_identity(cfg: &RunnerConfig) -> Runner {
    let sha = |p: &Path| std::fs::read(p).map(|b| sha256_hex(&b)).unwrap_or_default();
    Runner {
        init_sha256: sha(&cfg.runner_exe),
        axon_sha256: sha(&cfg.axon),
    }
}

/// A plain test identifier: what `@[test] fn NAME` can be. Anything else could
/// be an option, a path or a filter pattern.
fn is_identifier(s: &str) -> bool {
    let mut c = s.chars();
    matches!(c.next(), Some(ch) if ch == '_' || ch.is_ascii_alphabetic())
        && c.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

/// The exact-name result lines of `axon test --json` for `test`. `--filter` is
/// a SUBSTRING match, so every other name is ignored, and a name reported
/// twice is not a pass.
pub fn report_for(stdout: &str, test: &str) -> (GuestStatus, GuestReport) {
    let mut report = GuestReport {
        passed: vec![],
        failed: vec![],
        completion: vec![],
    };
    let mut seen = 0;
    for line in stdout.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        if v["name"].as_str() != Some(test) {
            continue;
        }
        seen += 1;
        match v["status"].as_str() {
            Some("ok") => {
                report.passed.push(test.into());
                if let Some(t) = v["completion"].as_str() {
                    report.completion.push((test.into(), t.into()));
                }
            }
            Some("failed") => report.failed.push(test.into()),
            _ => {}
        }
    }
    let status = match (seen, report.passed.len(), report.failed.len()) {
        (1, 1, 0) if report.completion.len() == 1 => GuestStatus::Passed,
        (1, 0, 1) => GuestStatus::Failed,
        _ => GuestStatus::Unknown,
    };
    (status, report)
}

/// Run the protocol. Always returns a verdict (never panics on hostile input);
/// `run_and_emit` also writes it.
pub fn run(cfg: &RunnerConfig) -> GuestVerdict {
    let none = InputCheck {
        candidate_tree_digest: String::new(),
        suite_tree_digest: String::new(),
        matches: false,
    };
    // 1. The secret: exactly 32 bytes, kept in this process.
    let secret: [u8; 32] = match std::fs::read(&cfg.secret) {
        Ok(b) if b.len() == 32 => b.try_into().expect("32 bytes"),
        Ok(b) => {
            return refused(
                cfg,
                "",
                none,
                "",
                format!("completion secret is {} bytes, not 32", b.len()),
            )
        }
        Err(e) => return refused(cfg, "", none, "", format!("completion secret: {e}")),
    };
    // 2. The manifest, exactly as named.
    let m_bytes = match std::fs::read(&cfg.manifest) {
        Ok(b) => b,
        Err(e) => return refused(cfg, "", none, "", format!("launch manifest: {e}")),
    };
    let m = match LaunchManifest::verify(&m_bytes, &cfg.expected_manifest_sha256) {
        Ok(m) => m,
        Err(e) => return refused(cfg, &sha256_hex(&m_bytes), none, "", e),
    };
    let m_sha = m.digest();
    let test = m.suite.test.clone();
    // 3. Both inputs, each against its own digest.
    let inputs = match check_inputs(&m, &cfg.candidate, &cfg.suite, &Quota::default()) {
        Ok(i) => i,
        Err((found, why)) => return refused(cfg, &m_sha, found, &test, why),
    };
    // 4. The test identity.
    if !is_identifier(&test) {
        return refused(
            cfg,
            &m_sha,
            inputs,
            &test,
            format!("test {test:?} is not an identifier"),
        );
    }
    if axon_workspace_recipe::check_path(&m.suite.entry, &Quota::default()).is_err()
        || !std::fs::symlink_metadata(cfg.suite.join(&m.suite.entry)).is_ok_and(|md| md.is_file())
    {
        return refused(
            cfg,
            &m_sha,
            inputs,
            &test,
            format!(
                "suite entry {:?} is not a file in the suite tree",
                m.suite.entry
            ),
        );
    }
    // 5. Exactly that test, with K and nothing else.
    let key = completion_key(&secret, &m);
    let (exit_code, stdout, stderr) = match exec_axon_test(cfg, &m, &key) {
        Ok(r) => r,
        Err(e) => return refused(cfg, &m_sha, inputs, &test, format!("axon test: {e}")),
    };
    let (mut status, report) = report_for(&stdout, &test);
    if status == GuestStatus::Passed && exit_code != Some(0) {
        status = GuestStatus::Unknown;
    }
    let _ = std::fs::write(cfg.out.join("stdout"), &stdout);
    // Diagnostics only (a refusal's reason, e.g. PCI E0004); never read as a result.
    let _ = std::fs::write(cfg.out.join("stderr"), &stderr);
    GuestVerdict {
        schema: GUEST_VERDICT_SCHEMA.into(),
        launch_manifest_sha256: m_sha,
        inputs,
        test,
        status,
        refusal: None,
        exit_code,
        report: Some(report),
        runner: runner_identity(cfg),
        stdout_sha256: Some(sha256_hex(stdout.as_bytes())),
    }
}

/// Run, write `verdict.json` to `out`, and return the verdict's sha256 (the
/// guest prints it on the serial console as `PSV-VERDICT sha256=<hex>`).
pub fn run_and_emit(cfg: &RunnerConfig) -> std::io::Result<(GuestVerdict, String)> {
    let v = run(cfg);
    let bytes = v.bytes();
    std::fs::write(cfg.out.join("verdict.json"), &bytes)?;
    Ok((v, sha256_hex(&bytes)))
}

fn exec_axon_test(
    cfg: &RunnerConfig,
    m: &LaunchManifest,
    key: &[u8; 32],
) -> Result<(Option<i32>, String, String), String> {
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(&cfg.axon);
    cmd.current_dir(&cfg.suite)
        .arg("test")
        .arg(cfg.suite.join(&m.suite.entry))
        .arg("--json")
        .arg("--filter")
        .arg(&m.suite.test)
        .arg("--completion-key-stdin")
        .arg("--seal")
        .arg(&cfg.candidate)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        // As on the host (`submit::Target::module_path`): the suite's own
        // directory, then the candidate, and nothing ambient.
        .env(
            "AXON_PATH",
            format!("{}:{}", cfg.suite.display(), cfg.candidate.display()),
        )
        .env("AXON_PATH_EXCLUSIVE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(c) = &cfg.effect_ceiling {
        cmd.env("AXON_ALLOWED_EFFECTS", c);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let drop = cfg.drop;
        unsafe {
            cmd.pre_exec(move || {
                // The child cannot gain privilege, and dies with the runner.
                if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
                    || libc::prctl(
                        libc::PR_SET_PDEATHSIG,
                        libc::SIGKILL as libc::c_ulong,
                        0,
                        0,
                        0,
                    ) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                if let Some((uid, gid)) = drop {
                    if libc::setgroups(0, std::ptr::null()) != 0
                        || libc::setgid(gid) != 0
                        || libc::setuid(uid) != 0
                    {
                        return Err(std::io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
    }
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    {
        let mut stdin = child.stdin.take().expect("piped");
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        // One line, then EOF: the interpreter reads it before program code runs.
        writeln!(stdin, "{hex}").map_err(|e| e.to_string())?;
    }
    let cap = m.limits.output_bytes as usize;
    let drain = |mut r: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut kept = Vec::new();
            let mut buf = [0u8; 8192];
            let mut over = false;
            loop {
                match r.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let room = cap.saturating_sub(kept.len());
                        over |= n > room;
                        kept.extend_from_slice(&buf[..n.min(room)]);
                    }
                }
            }
            (kept, over)
        })
    };
    let out_h = drain(Box::new(child.stdout.take().expect("piped")));
    let err_h = drain(Box::new(child.stderr.take().expect("piped")));
    let deadline = Duration::from_millis(m.limits.wall_time_ms);
    let start = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().map_err(|e| e.to_string())? {
            break Some(st);
        }
        if start.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let (o, oo) = out_h.join().unwrap_or_default();
    let (e, eo) = err_h.join().unwrap_or_default();
    let Some(status) = status else {
        return Err(format!(
            "killed at its {} ms wall-clock limit",
            m.limits.wall_time_ms
        ));
    };
    if oo || eo {
        return Err(format!(
            "output exceeded {cap} bytes; a truncated run yields no verdict"
        ));
    }
    Ok((
        status.code(),
        String::from_utf8_lossy(&o).into_owned(),
        String::from_utf8_lossy(&e).into_owned(),
    ))
}
