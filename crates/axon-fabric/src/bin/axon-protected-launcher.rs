//! `axon-protected-launcher` — the privileged launcher helper (operator
//! decision A; `axon_fabric::privileged_launcher`).
//!
//! ```text
//! axon-protected-launcher            < request.json   (as the Fabric uid)
//! axon-protected-launcher --probe                     (trust preflight)
//! axon-protected-launcher --test-config FILE < request.json   (test-trust builds ONLY)
//! ```
//!
//! Installed root-owned, mode 04750, group = the Fabric service's group, on a
//! filesystem mounted without `nosuid`. It reads its configuration only from
//! `/etc/axon/protected-launcher.json`, authenticates the caller by its real
//! uid, and writes one `axon-protected-launch-report/1` to stdout. Exit 0: it
//! launched; 30: it refused and nothing was launched; 31: something failed
//! after the launch began; 2: usage.

use axon_fabric::privileged_launcher as pl;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

fn main() {
    // SAFETY: getuid/geteuid cannot fail.
    let (ruid, euid) = unsafe { (libc::getuid(), libc::geteuid()) };
    // Read the arguments, then reset everything the caller could have set.
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    pl::harden();
    let (config, authority) = match args.as_slice() {
        [] => (PathBuf::from(pl::CONFIG_PATH), pl::Authority::production()),
        [p] if p == "--probe" => {
            println!("{}", pl::probe(ruid, euid));
            std::process::exit(0);
        }
        [f, p] if f == "--test-config" && axon_fabric::backend::TEST_TRUST_BUILD => {
            let p = PathBuf::from(p);
            let base = p.parent().unwrap_or(Path::new("/")).to_path_buf();
            (
                p,
                pl::Authority {
                    // The operator of a test is whoever the helper runs as.
                    operator_uid: euid,
                    walk_base: base,
                    test: true,
                },
            )
        }
        _ => {
            eprintln!("usage: axon-protected-launcher [--probe] < request.json");
            std::process::exit(2);
        }
    };
    let finish = |r: pl::LaunchReport, code: i32| -> ! {
        let mut out = std::io::stdout().lock();
        let _ = writeln!(out, "{}", serde_json::to_string(&r).unwrap_or_default());
        let _ = out.flush();
        std::process::exit(code);
    };
    let refuse = |why: String| -> ! {
        finish(
            pl::LaunchReport {
                schema: pl::REPORT_SCHEMA.into(),
                build: pl::build_name().into(),
                launched: false,
                error: Some(why),
                launcher_sha256: None,
                interpreter_sha256: None,
                launcher_exit: None,
                verify_exit: None,
                unchanged: false,
            },
            pl::EXIT_REFUSED,
        )
    };
    if euid != 0 && !authority.test {
        refuse(format!(
            "effective uid is {euid}, not 0: the helper is not installed setuid-root (or its \
             filesystem is mounted nosuid)"
        ));
    }
    // The request, bounded, before any id change.
    let mut request = Vec::new();
    if let Err(e) = std::io::stdin()
        .lock()
        .take(256 << 10)
        .read_to_end(&mut request)
    {
        refuse(format!("request: {e}"));
    }
    // Authenticate first (inside serve: the real uid against the operator's
    // fabric_uid), and only then act as root in every id. The config is read
    // with the effective uid the kernel granted.
    let (report, code) = pl::serve_as(&config, &authority, ruid, &request);
    finish(report, code)
}
