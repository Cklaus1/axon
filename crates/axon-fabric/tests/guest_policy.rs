//! S3-5 (B263 x1, ACF-G25), Fabric side: every Linux-profile launch delivers
//! the ADMITTED grant's effect ceiling to the guest as an `axon-vm-mmds/1`
//! policy file (`--policy FILE`), and a grant that withholds an effect becomes
//! eligible on that profile ONLY when the signed qualification evidence shows
//! `x1_guest_policy_channel` as PASS. Driven through `submit()` with a
//! stand-in launcher that records its argv and the delivered policy bytes.
//! Refusals assert the absence of effects: Unsupported receipt, no journal
//! launch record, the launcher never ran, no policy delivered.

mod common;
use common::*;

use axon_fabric::backend::{self, LinuxProfileConfig};
use axon_fabric::submit;
use axon_loop_contracts::{ComputeRequest, ReceiptStatus};
use serde_json::{json, Value};
use std::path::Path;

const GUEST: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

/// Withholds every axis: its ceiling is `""`, which must reach the guest as
/// deny-all.
const GRANT_DENY: &str = "\
profile = \"restricted\"
[grant]
max_label = \"internal\"
[grant.budget]
cost_micro = 1000
";

/// 512 four-byte characters: the longest `principal_ref` a request may carry,
/// and far more than the guest cmdline can hold once base64-encoded.
fn long_principal() -> String {
    "\u{1D54F}".repeat(512)
}

struct World {
    env: Env,
    issuer: Issuer,
    manifest_sha: String,
}

impl World {
    fn new() -> World {
        let env = Env::new();
        let m = env.dir.path().join("manifest.json");
        std::fs::write(&m, lx_manifest(GUEST)).unwrap();
        let manifest_sha = sha256_file(&m);
        write_grant_registry(
            &env.grant_registry,
            &[
                ("grant:test", PRINCIPAL, GRANT_FS),
                ("grant:open", PRINCIPAL, GRANT_OPEN),
                ("grant:deny", PRINCIPAL, GRANT_DENY),
                ("grant:long", &long_principal(), GRANT_FS),
            ],
        );
        World {
            env,
            issuer: Issuer::generate(),
            manifest_sha,
        }
    }
    fn dir(&self) -> &Path {
        self.env.dir.path()
    }
    /// A complete, fresh PASS record that ALSO shows x1 as PASS.
    fn evidence_x1_pass(&self) -> Value {
        let mut ev = good_evidence(&self.manifest_sha);
        ev["assertions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": backend::X1_GUEST_POLICY_CHANNEL, "status": "PASS"}));
        ev["counts"] = json!({"total": 4, "PASS": 4, "FAIL": 0, "BLOCKED": 0});
        ev
    }
    /// x1 BLOCKED — today's real state — under an issuer-signed waiver, so
    /// the record still QUALIFIES the profile: the only thing standing between
    /// a restricted grant and a launch is the x1 rule itself.
    fn cfg_x1_blocked_but_waived(&self) -> LinuxProfileConfig {
        let mut ev = good_evidence(&self.manifest_sha);
        ev["assertions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": backend::X1_GUEST_POLICY_CHANNEL, "status": "BLOCKED"}));
        ev["counts"] = json!({"total": 4, "PASS": 3, "FAIL": 0, "BLOCKED": 1});
        ev["result"] = json!("PASS_WITH_BLOCKED");
        let mut lx = self.cfg(&ev);
        let p = self.dir().join("waivers.json");
        self.issuer.write_signed(
            &p,
            &json!({"schema": "axon-b263-waiver/1",
                    "evidence_sha256": sha256_file(&lx.evidence),
                    "waivers": [{"assertion": backend::X1_GUEST_POLICY_CHANNEL,
                                 "reason": "test: accepted without a policy channel",
                                 "expires": "2026-12-31T00:00:00Z"}]}),
        );
        lx.waivers = Some(p);
        lx
    }
    fn cfg(&self, evidence: &Value) -> LinuxProfileConfig {
        let mut lx = qualified_linux_cfg(self.dir(), &self.issuer, evidence);
        set_launcher(&mut lx, stand_in_launcher(&self.env, 0, true, true, 0));
        std::fs::create_dir_all(&lx.out_root).unwrap();
        lx
    }
    fn request(&self, op: &str, grant: &str, principal: &str) -> Value {
        let mut r = request(&self.env, op, "t_ok");
        r["required"]["hardware_isolation"] = json!(true);
        r["required"]["os"] = json!("linux");
        r["job_kind"] = json!("interpreter_run");
        r["argv"] = json!(["f.ax"]);
        r["registered_executable_ref"] = json!(backend::LINUX_GUEST_AXON_ID);
        r["grant_ref"] = json!(grant);
        r["principal_ref"] = json!(principal);
        r["executable_digest"] = json!(axon_cortex::runner::fabric_executable_digest(
            backend::LINUX_GUEST_AXON_ID,
            GUEST
        ));
        r
    }
    fn submit(
        &self,
        lx: &LinuxProfileConfig,
        op: &str,
        grant: &str,
        principal: &str,
    ) -> axon_fabric::Submission {
        let mut cfg = self.env.cfg(0);
        cfg.linux = Some(lx.clone());
        submit(&self.request(op, grant, principal).to_string(), &cfg).unwrap()
    }
    fn launches(&self, lx: &LinuxProfileConfig) -> usize {
        std::fs::read_to_string(lx.out_root.join("launches"))
            .map(|s| s.lines().count())
            .unwrap_or(0)
    }
    fn delivered(&self, lx: &LinuxProfileConfig) -> Option<(String, Value)> {
        let raw = std::fs::read_to_string(lx.out_root.join("delivered-policy.json")).ok()?;
        let v = serde_json::from_str(&raw).unwrap();
        Some((raw, v))
    }
    fn argv(&self, lx: &LinuxProfileConfig) -> String {
        std::fs::read_to_string(lx.out_root.join("argv")).unwrap_or_default()
    }

    /// Launched once, and the launcher was handed `--policy` whose contents
    /// are an `axon-vm-mmds/1` payload carrying exactly `effects`.
    fn assert_launched_with(
        &self,
        lx: &LinuxProfileConfig,
        s: &axon_fabric::Submission,
        effects: Value,
    ) -> Value {
        assert_eq!(s.receipt.status, ReceiptStatus::Completed, "{:?}", s.reason);
        assert_eq!(s.backend, Some("linux-microvm-protected"));
        assert_eq!(self.env.launch_records(), 1);
        assert_eq!(self.launches(lx), 1);
        let argv = self.argv(lx);
        assert!(
            argv.contains("--policy "),
            "no --policy in launcher argv: {argv}"
        );
        let (raw, p) = self
            .delivered(lx)
            .expect("the launcher was handed a --policy file");
        assert_eq!(p["schema"], "axon-vm-mmds/1");
        assert_eq!(p["allowed_effects"], effects, "{raw}");
        assert_eq!(p["run_id"], s.receipt.operation_id.as_str());
        // The receipt names the exact policy bytes that were delivered.
        let want = format!("guest-policy-sha256:{}", {
            use sha2::{Digest, Sha256};
            format!("{:x}", Sha256::digest(raw.as_bytes()))
        });
        assert!(
            s.receipt.evidence_refs.iter().any(|e| e.as_str() == want),
            "{:?}",
            s.receipt.evidence_refs
        );
        p
    }

    fn assert_refused_unlaunched(
        &self,
        lx: &LinuxProfileConfig,
        s: &axon_fabric::Submission,
        why: &str,
    ) {
        assert_eq!(
            s.receipt.status,
            ReceiptStatus::Unsupported,
            "{:?}",
            s.reason
        );
        assert!(
            s.reason.as_deref().unwrap_or("").contains(why),
            "expected {why:?} in {:?}",
            s.reason
        );
        assert_eq!(s.backend, None);
        assert_eq!(self.env.launch_records(), 0, "no journal launch record");
        assert_eq!(self.launches(lx), 0, "the launcher never ran");
        assert!(self.delivered(lx).is_none(), "no policy was delivered");
        assert_eq!(spawn_count(&self.env.spawns), 0, "nothing was spawned");
        let leftover: Vec<_> = std::fs::read_dir(&lx.out_root)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(leftover.is_empty(), "no policy file written: {leftover:?}");
    }
}

// ── the payload is the admitted grant's ceiling ──────────────────────────────

#[test]
fn the_policy_handed_to_the_launcher_is_the_admitted_grants_ceiling() {
    let w = World::new();
    let lx = w.cfg(&w.evidence_x1_pass());
    let s = w.submit(&lx, "op-fs", "grant:test", PRINCIPAL);
    // GRANT_FS induces AXON_ALLOWED_EFFECTS=IO on the host executor.
    let p = w.assert_launched_with(&lx, &s, json!(["IO"]));
    assert_eq!(p["principal"], PRINCIPAL);
}

#[test]
fn an_open_grant_is_delivered_as_its_full_ceiling_even_without_x1() {
    // A grant that withholds nothing needs no guest enforcement to be honest,
    // so it stays eligible on today's evidence (no x1) — and its policy is
    // still delivered, spelled out, never left implicit.
    let w = World::new();
    let lx = w.cfg(&good_evidence(&w.manifest_sha));
    let s = w.submit(&lx, "op-open", "grant:open", PRINCIPAL);
    w.assert_launched_with(&lx, &s, json!(["Net", "AI", "IO", "Exec"]));
}

#[test]
fn an_empty_ceiling_is_delivered_as_deny_all_never_unrestricted() {
    let w = World::new();
    let lx = w.cfg(&w.evidence_x1_pass());
    let s = w.submit(&lx, "op-deny", "grant:deny", PRINCIPAL);
    let p = w.assert_launched_with(&lx, &s, json!([]));
    // Present and empty: an absent key would read as "no ceiling" in the guest.
    assert!(p.as_object().unwrap().contains_key("allowed_effects"));
}

// ── x1 is lifted by the evidence, not by code ────────────────────────────────

#[test]
fn x1_blocked_evidence_keeps_the_refusal_even_when_waived() {
    let w = World::new();
    let lx = w.cfg_x1_blocked_but_waived();
    let q = lx
        .qualification()
        .expect("the waived record still qualifies");
    assert!(!q.guest_policy_channel, "a waived BLOCKED x1 is not a PASS");
    for grant in ["grant:test", "grant:deny"] {
        let s = w.submit(&lx, &format!("op-{grant}"), grant, PRINCIPAL);
        w.assert_refused_unlaunched(&lx, &s, "x1_guest_policy_channel as PASS");
    }
}

#[test]
fn evidence_without_an_x1_assertion_keeps_the_refusal() {
    let w = World::new();
    let lx = w.cfg(&good_evidence(&w.manifest_sha));
    assert!(!lx.qualification().unwrap().guest_policy_channel);
    let s = w.submit(&lx, "op-no-x1", "grant:test", PRINCIPAL);
    w.assert_refused_unlaunched(&lx, &s, "x1_guest_policy_channel as PASS");
}

#[test]
fn x1_pass_signed_evidence_lifts_the_refusal() {
    let w = World::new();
    let lx = w.cfg(&w.evidence_x1_pass());
    assert!(lx.qualification().unwrap().guest_policy_channel);
    let req: ComputeRequest =
        axon_loop_contracts::parse(&w.request("op-sel", "grant:test", PRINCIPAL).to_string())
            .unwrap();
    let needs = backend::AuthorityNeeds {
        guest_policy_channel: true,
        ..Default::default()
    };
    assert_eq!(
        backend::select(&req, Some(&lx), needs).unwrap().id,
        "linux-microvm-protected"
    );
    // x2 is a different assertion: a path-scoped grant is still refused.
    let scoped = backend::AuthorityNeeds {
        guest_policy_channel: true,
        path_scoped_grant: true,
        ..Default::default()
    };
    assert!(backend::select(&req, Some(&lx), scoped).is_err());
}

// ── a policy the guest cmdline cannot hold is refused before launch ─────────

#[test]
fn an_oversize_policy_is_refused_with_no_launch() {
    let w = World::new();
    let lx = w.cfg(&w.evidence_x1_pass());
    let s = w.submit(&lx, "op-big", "grant:long", &long_principal());
    w.assert_refused_unlaunched(&lx, &s, "the kernel would truncate it");
    // Journalled as unsupported, so a retry returns the same answer.
    let again = w.submit(&lx, "op-big", "grant:long", &long_principal());
    assert!(again.replayed);
    assert_eq!(again.receipt.status, ReceiptStatus::Unsupported);
    assert_eq!(w.launches(&lx), 0);
}

#[test]
fn the_size_rule_is_measured_on_the_encoded_cmdline_word() {
    let w = World::new();
    let req = |p: &str| -> ComputeRequest {
        axon_loop_contracts::parse(&w.request("op-sz", "grant:test", p).to_string()).unwrap()
    };
    let ok = backend::GuestPolicy::for_grant(&req(PRINCIPAL), "").unwrap();
    let v: Value = serde_json::from_str(ok.json()).unwrap();
    assert_eq!(v["allowed_effects"], json!([]));
    // ASCII principal of exactly 512 chars: ~800 bytes of base64 — fits.
    assert!(backend::GuestPolicy::for_grant(&req(&"p".repeat(512)), "Net,AI,IO,Exec").is_ok());
    assert!(backend::GuestPolicy::for_grant(&req(&long_principal()), "").is_err());
}

/// The reserve is only honest while the launcher's own boot args fit in it
/// with room left for what Firecracker appends (one `virtio_mmio.device=` word
/// per device). Read from the script the Fabric launches, not restated.
#[test]
fn the_launchers_boot_args_fit_the_reserved_cmdline_budget() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/fc_linux_profile.sh");
    let text = std::fs::read_to_string(&script).unwrap();
    let line = text
        .lines()
        .find(|l| l.starts_with("BOOT_ARGS=\""))
        .expect("fc_linux_profile.sh defines BOOT_ARGS");
    let args = line
        .trim_start_matches("BOOT_ARGS=\"")
        .trim_end_matches('"');
    assert!(!args.is_empty());
    assert!(
        args.len() <= backend::LAUNCHER_CMDLINE_RESERVE / 2,
        "BOOT_ARGS is {} bytes; LAUNCHER_CMDLINE_RESERVE {} no longer leaves half for Firecracker",
        args.len(),
        backend::LAUNCHER_CMDLINE_RESERVE
    );
    assert_eq!(
        backend::GUEST_POLICY_WORD_MAX + backend::LAUNCHER_CMDLINE_RESERVE,
        backend::GUEST_CMDLINE_MAX_SAFE
    );
}
