//! `axon-observer` — the preflight observer SERVICE (operator decisions G and
//! G1; `axon_fabric::observer_service`, amendment 68).
//!
//! ```text
//! axon-observer                                   (protected: systemd socket activation)
//! axon-observer --test-config FILE                (test-trust builds ONLY)
//! axon-observer --dev --test-config FILE          (test-trust builds ONLY; replies `mode: dev`)
//! ```
//!
//! Protected: started by `axon-observer.socket` as its own uid (`User=`),
//! with the listening socket as fd 3. It reads its config only from
//! `/etc/axon/observer.json` (operator-owned, walked from `/`), runs as the
//! configured observer uid (neither the Fabric's nor root), loads a key only
//! that uid can read whose public half is in `/etc/axon/trust/observer`
//! alone, keeps one record per observed nonce in its own 0700 store, and
//! answers only uid 0 (the root helper's `--observe` relay). It measures the
//! operator's installed files and signs nothing it did not measure.

use axon_fabric::backend::Clock;
use axon_fabric::custodian::{self as cu, Mode};
use axon_fabric::observer_service::{self as os, ObserverServiceConfig, Server, Sources};
use std::path::PathBuf;

fn die(why: &str) -> ! {
    eprintln!("axon-observer: {why}");
    std::process::exit(2);
}

fn euid() -> u32 {
    // SAFETY: geteuid cannot fail.
    unsafe { libc::geteuid() }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let test_config = |i: usize| -> (ObserverServiceConfig, Sources, PathBuf) {
        let p = PathBuf::from(args.get(i).unwrap_or_else(|| die("--test-config FILE")));
        let bytes = std::fs::read(&p).unwrap_or_else(|e| die(&format!("{}: {e}", p.display())));
        let c: ObserverServiceConfig = serde_json::from_slice(&bytes)
            .unwrap_or_else(|e| die(&format!("{}: {e}", p.display())));
        c.check(false).unwrap_or_else(|e| die(&e));
        let t = c
            .test_paths
            .clone()
            .unwrap_or_else(|| die("a test config names its test_paths"));
        let s = Sources {
            host_config: t.host_config,
            helper_config: t.helper_config,
            authority: None,
        };
        (c, s, t.trust_root)
    };
    // `--dev` (test-trust builds only, below): the same service, replying
    // `mode: dev`, which the root helper never relays.
    let dev = args.first().map(String::as_str) == Some("--dev");
    let rest: Vec<&str> = args
        .iter()
        .skip(usize::from(dev))
        .map(String::as_str)
        .collect();
    let (cfg, sources, root, mode) = match rest[..] {
        [] if !dev => {
            let c = os::load_config(
                std::path::Path::new(os::CONFIG_PATH),
                &axon_fabric::privileged_launcher::Authority::production(),
            )
            .unwrap_or_else(|e| die(&e));
            (
                c,
                Sources::operator(),
                axon_fabric::backend::TrustAuthority::Observer.operator_dir(),
                Mode::Protected,
            )
        }
        ["--test-config", _] if axon_fabric::backend::TEST_TRUST_BUILD => {
            let (c, s, r) = test_config(usize::from(dev) + 1);
            (c, s, r, if dev { Mode::Dev } else { Mode::Test })
        }
        _ => die("usage: axon-observer [--test-config FILE]"),
    };
    // The observer runs as the uid its config names, or not at all.
    if euid() != cfg.observer_uid {
        die(&format!(
            "running as uid {}, not the configured observer uid {}",
            euid(),
            cfg.observer_uid
        ));
    }
    // Protected: WHERE the store sits is the operator's; the store itself is
    // ours (check_store below). The chain only: the parent's entries include
    // the store, which is the observer uid's by design.
    if mode == Mode::Protected {
        let parent = cfg.store.parent().unwrap_or(std::path::Path::new("/"));
        axon_fabric::backend::check_operator_chain(parent).unwrap_or_else(|e| die(&e));
    }
    // Every mode: the key is ours alone and trusted where it must be; the
    // store is ours and private. Checked before a single connection.
    let (key, public_hex) = os::load_key(&cfg.key_path, euid()).unwrap_or_else(|e| die(&e));
    os::key_in_root(&public_hex, &root, mode == Mode::Protected).unwrap_or_else(|e| die(&e));
    cu::check_store(&cfg.store, euid()).unwrap_or_else(|e| die(&e));
    let listener = match mode {
        Mode::Protected => cu::activated_listener(&cfg.socket).unwrap_or_else(|e| die(&e)),
        Mode::Test | Mode::Dev => std::os::unix::net::UnixListener::bind(&cfg.socket)
            .unwrap_or_else(|e| die(&format!("bind {}: {e}", cfg.socket.display()))),
    };
    let key_id = {
        use ring::signature::KeyPair;
        axon_loop_contracts::attestation::key_fingerprint(key.public_key().as_ref())
    };
    let server = Server {
        cfg,
        mode,
        key,
        key_id,
        public_hex,
        sources,
        clock: Clock::System,
        request_deadline: os::REQUEST_DEADLINE,
    };
    server.serve(&listener)
}
