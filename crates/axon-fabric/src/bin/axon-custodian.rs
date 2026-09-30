//! `axon-custodian` — the observation-nonce custodian (operator decision D6;
//! `axon_fabric::custodian`, amendment 50).
//!
//! ```text
//! axon-custodian                                  (protected: systemd socket activation)
//! axon-custodian --dev --socket P --store D [--fabric-uid U] [--launcher-uid U] [--max-age-s N]
//! axon-custodian --test-config FILE               (test-trust builds ONLY)
//! ```
//!
//! Protected: started by `axon-custodian.socket` as its own uid (`User=`),
//! with the listening socket as fd 3. It reads its config only from
//! `/etc/axon/custodian.json` (operator-owned, walked from `/`), runs as the
//! configured custodian uid, which is neither the Fabric's nor root, serves
//! from its own 0700 store, and refuses an activation on any other socket.
//! Every reply says `"mode":"protected"`.
//!
//! `--dev`: a MANUAL launch that binds the socket itself. Every reply says
//! `"mode":"dev"`, and the privileged launcher refuses a dev spend: a dev
//! custodian never yields a protected launch.

use axon_fabric::backend::Clock;
use axon_fabric::custodian::{self as cu, CustodianConfig, Mode, Server};
use axon_fabric::observer::NonceStore;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

fn die(why: &str) -> ! {
    eprintln!("axon-custodian: {why}");
    std::process::exit(2);
}

fn euid() -> u32 {
    // SAFETY: geteuid cannot fail.
    unsafe { libc::geteuid() }
}

/// The custodian runs as the uid its config names, or not at all.
fn must_run_as(c: &CustodianConfig) {
    if euid() != c.custodian_uid {
        die(&format!(
            "running as uid {}, not the configured custodian uid {}",
            euid(),
            c.custodian_uid
        ));
    }
}

fn bind(socket: &std::path::Path) -> UnixListener {
    UnixListener::bind(socket).unwrap_or_else(|e| {
        die(&format!(
            "bind {}: {e} (it must not exist)",
            socket.display()
        ))
    })
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (cfg, mode) = match args.first().map(String::as_str) {
        None => {
            let c = cu::load_config(
                std::path::Path::new(cu::CONFIG_PATH),
                &axon_fabric::privileged_launcher::Authority::production(),
            )
            .unwrap_or_else(|e| die(&e));
            must_run_as(&c);
            // WHERE the store sits is the operator's; the store itself is ours.
            let parent = c.store.parent().unwrap_or(std::path::Path::new("/"));
            // The chain only: the parent's entries include the store, which
            // is the custodian's uid's by design (check_store below).
            axon_fabric::backend::check_operator_chain(parent).unwrap_or_else(|e| die(&e));
            (c, Mode::Protected)
        }
        Some("--test-config") if axon_fabric::backend::TEST_TRUST_BUILD => {
            let p = PathBuf::from(args.get(1).unwrap_or_else(|| die("--test-config FILE")));
            let bytes = std::fs::read(&p).unwrap_or_else(|e| die(&format!("{}: {e}", p.display())));
            let c: CustodianConfig = serde_json::from_slice(&bytes)
                .unwrap_or_else(|e| die(&format!("{}: {e}", p.display())));
            c.check(false).unwrap_or_else(|e| die(&e));
            must_run_as(&c);
            (c, Mode::Test)
        }
        Some("--dev") => {
            let opt = |k: &str| -> Option<String> {
                args.iter()
                    .position(|a| a == k)
                    .and_then(|i| args.get(i + 1).cloned())
            };
            let uid = |v: Option<String>| -> u32 {
                v.map(|s| s.parse().unwrap_or_else(|_| die("a uid is a number")))
                    .unwrap_or_else(euid)
            };
            let socket =
                PathBuf::from(opt("--socket").unwrap_or_else(|| die("--dev needs --socket P")));
            let store =
                PathBuf::from(opt("--store").unwrap_or_else(|| die("--dev needs --store D")));
            let c = CustodianConfig {
                schema: cu::CONFIG_SCHEMA.into(),
                custodian_uid: euid(),
                fabric_uid: uid(opt("--fabric-uid")),
                launcher_uid: uid(opt("--launcher-uid")),
                socket,
                store,
                max_age_s: opt("--max-age-s")
                    .map(|s| s.parse().unwrap_or_else(|_| die("--max-age-s is a number")))
                    .unwrap_or(300),
            };
            c.check(false).unwrap_or_else(|e| die(&e));
            use std::os::unix::fs::DirBuilderExt;
            let _ = std::fs::DirBuilder::new()
                .mode(0o700)
                .recursive(true)
                .create(&c.store);
            eprintln!(
                "axon-custodian: DEV custodian on {} (never protected)",
                c.socket.display()
            );
            (c, Mode::Dev)
        }
        _ => die("usage: axon-custodian [--dev --socket P --store D | --test-config FILE]"),
    };
    // Every mode: the store is ours, and private — checked before a single
    // connection is accepted.
    cu::check_store(&cfg.store, euid()).unwrap_or_else(|e| die(&e));
    let listener = match mode {
        Mode::Protected => cu::activated_listener(&cfg.socket).unwrap_or_else(|e| die(&e)),
        Mode::Test | Mode::Dev => bind(&cfg.socket),
    };
    let server = Server {
        store: NonceStore {
            dir: cfg.store.clone(),
        },
        cfg,
        mode,
        clock: Clock::System,
    };
    server.serve(&listener)
}
