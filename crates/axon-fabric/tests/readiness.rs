//! Operator direction (2026-09-27): "an agent with full write access to the
//! repo cannot make protected_backend turn PASS by changing any combination of
//! source, scripts, manifests, proof files, or local keys."
//!
//! The verdict is `axon_fabric::readiness` (the operator-installed
//! `axon-fabric verify-readiness`); the repository is only evidence. Each test
//! starts from a GENUINELY certified repository (PASS under a trust root the
//! test controls), applies one attack an agent with repository write access
//! can make, and asserts the component is no longer PASS for THAT reason.
//!
//! The test trust root (`ReadinessTrust::test`) exists only in test-trust
//! builds, which report `build: "test-trust"` and are never accepted by
//! readiness; the production `ReadinessTrust::operator()` is exercised last.

mod common;
mod readiness_fixture;
use common::*;
use readiness_fixture::*;

use axon_fabric::backend::TrustAuthority;
use axon_fabric::readiness::{protected_components, ReadinessTrust, TRUST_PREFLIGHT_SCHEMA};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;

#[test]
fn a_repository_signed_or_planted_key_is_not_authority() {
    let Some(c) = certified() else { return };
    // The agent signs the record with its own key, plants that key in the
    // repository's trusted_issuers AND lists it as expected.
    let agent = Issuer::generate();
    agent.trust_in(&c.repo.join("governance/trusted_issuers"), "agent");
    write(
        &c.repo.join("governance/status/trust-expectations.json"),
        &json!({"qualification_issuers": [agent.key_id()]}).to_string(),
    );
    resign(&c, &agent, |_| {});
    c.commit("agent: my key, my signature");
    c.refused("not a trusted evidence issuer");
}

#[test]
fn the_repository_may_narrow_the_issuers_never_add() {
    let Some(c) = certified() else { return };
    write(
        &c.repo.join("governance/status/trust-expectations.json"),
        &json!({"qualification_issuers": ["ed25519:ffffffffffffffff"]}).to_string(),
    );
    c.commit("narrow away the operator");
    c.refused("not one this repository expects");
}

#[test]
fn any_change_to_source_scripts_or_manifests_invalidates_it() {
    for (file, label) in [
        ("crates/axon-fabric/src/lib.rs", "source"),
        ("scripts/protected_verifier_ready.py", "script"),
        ("profiles/linux-microvm/manifest.json", "manifest"),
        (
            "profiles/linux-microvm/trusted_issuers/agent.pub",
            "local key",
        ),
    ] {
        let Some(c) = certified() else { return };
        write(&c.repo.join(file), "changed by the agent\n");
        // Uncommitted first, then committed: neither counts.
        c.refused("changed since the certified revision");
        c.commit(label);
        c.refused("changed since the certified revision");
    }
}

#[test]
fn a_changed_spec_evidence_or_record_invalidates_it() {
    let Some(c) = certified() else { return };
    write(
        &c.repo
            .join("governance/specs/v022-protected-suite-verdict.md"),
        "# PSV v2\n",
    );
    c.refused("certifies another version");

    let Some(c) = certified() else { return };
    write(
        &c.repo
            .join("governance/proofs/v022-protected/run-evidence.md"),
        "forged\n",
    );
    c.refused("evidence bundle changed");

    let Some(c) = certified() else { return };
    let mut bytes = std::fs::read(c.record()).unwrap();
    let s = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("t_ok", "t_ko");
    bytes = s.into_bytes();
    std::fs::write(c.record(), bytes).unwrap();
    c.refused("does not verify");

    // A re-signed record for ANOTHER component does not count for this one.
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["component"] = json!("pci_on_protected_backend")
    });
    c.refused("not a linux-microvm-protected certification of protected_backend");
}

#[test]
fn gates_and_proofs_without_the_certification_are_not_pass() {
    let Some(c) = certified() else { return };
    std::fs::remove_file(c.record()).unwrap();
    c.refused("no protected-host certification record");
    assert_eq!(c.verdict()["status"], "PARTIAL");
}

/// The production trust: the operator root on this host, fixed, with no flag
/// to choose it. On a development host (no root, or a root the checking
/// process could write) a genuinely signed record still authorizes nothing.
#[test]
fn production_authority_is_the_operator_root_only() {
    let Some(c) = certified() else { return };
    let v = protected_components(&c.repo, &ReadinessTrust::operator());
    assert_eq!(v["trust_root"], "/etc/axon/trust/qualification");
    let pb = &v["components"]["protected_backend"];
    assert_ne!(pb["status"], "PASS", "{pb}");
    // Absent here: the refusal names the missing operator path, and there is
    // no fallback to anything in the repository.
    assert!(pb.to_string().contains("/etc/axon"), "{pb}");
    // This test build carries the test trust constructors, and says so.
    assert_eq!(v["build"], "test-trust");
}

/// Registering results must not invalidate the certification: a change under
/// `governance/` alone, uncommitted or committed, keeps it (regression: a
/// trimmed `git status` once misread the first dirty path's name).
#[test]
fn a_governance_only_change_keeps_the_certification() {
    let Some(c) = certified() else { return };
    write(
        &c.repo.join("governance/status/v022-pci.json"),
        "{\"state\":1}\n",
    );
    assert_eq!(c.verdict()["status"], "PASS", "{}", c.verdict());
    c.commit("status update");
    assert_eq!(c.verdict()["status"], "PASS", "{}", c.verdict());
}

/// The certification binds THIS repository's history: an operator-signed
/// record naming a revision this tree does not descend from counts for
/// nothing (and a revision it cannot even compare with is never "unchanged").
#[test]
fn a_certification_of_another_history_is_not_this_one() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["axon_sha"] = json!("0123456789abcdef0123456789abcdef01234567")
    });
    c.refused("is not an ancestor of this tree");
}

/// The trust root itself must be operator-owned: a genuinely signed record
/// verifies under a root the checker could not trust as authority.
#[test]
fn a_trust_root_that_is_not_operator_owned_authorizes_nothing() {
    let Some(c) = certified() else { return };
    let root = c.trust.issuers_dir.clone();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
    c.refused("group- or other-writable");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(c.verdict()["status"], "PASS");

    std::os::unix::fs::chown(root.join("operator.pub"), Some(1000), None).unwrap();
    c.refused("not root");
    std::os::unix::fs::chown(root.join("operator.pub"), Some(0), None).unwrap();

    std::os::unix::fs::symlink(root.join("operator.pub"), root.join("alias.pub")).unwrap();
    c.refused("symlink");
}

/// Replacing the installed verifier is a change of authority: a certification
/// made with one verifier binary does not transfer to another.
#[test]
fn another_verifier_binary_does_not_inherit_the_certification() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["readiness_verifier_sha256"] = json!("9".repeat(64))
    });
    c.refused("certified with readiness verifier");
}

/// The trust preflight must be a certified, PROTECTED-mode, passing run: a
/// development run of the same script, or a report outside the bundle, is not.
#[test]
fn a_dev_mode_or_uncertified_trust_preflight_is_refused() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["trust_preflight_sha256"] = json!("8".repeat(64))
    });
    c.refused("names no certified evidence file");

    let Some(c) = certified() else { return };
    write(
        &c.repo.join(PREFLIGHT),
        &json!({"schema": TRUST_PREFLIGHT_SCHEMA, "mode": "dev", "verdict": "PASS"}).to_string(),
    );
    let pf = sha(&c.repo.join(PREFLIGHT));
    use sha2::{Digest, Sha256};
    let ev = sha(&c
        .repo
        .join("governance/proofs/v022-protected/run-evidence.md"));
    let bundle = format!("{:x}", Sha256::digest(format!("{ev}{pf}").as_bytes()));
    resign(&c, &c.operator, |r| {
        r["trust_preflight_sha256"] = json!(pf);
        r["evidence_bundle_sha256"] = json!(bundle);
    });
    c.commit("dev-mode preflight");
    c.refused("not a passing protected-mode");
}

/// The authority domain is part of the signed message: the operator's own
/// key, signing the exact record bytes for ANOTHER authority, does not count
/// as a qualification signature.
#[test]
fn a_signature_for_another_authority_is_not_a_qualification_signature() {
    let Some(c) = certified() else { return };
    let bytes = std::fs::read(c.record()).unwrap();
    for a in [
        TrustAuthority::Observer,
        TrustAuthority::Verifier,
        TrustAuthority::Admission,
    ] {
        let mut sig = c.record().into_os_string();
        sig.push(".sig");
        std::fs::write(&sig, c.operator.sign_for(a, &bytes)).unwrap();
        c.refused("is for authority");
        // Relabelling the domain field without re-signing breaks the signature.
        let mut v: Value = serde_json::from_str(&c.operator.sign_for(a, &bytes)).unwrap();
        v["domain"] = json!("qualification");
        std::fs::write(&sig, v.to_string()).unwrap();
        c.refused("does not verify");
    }
}

// ---------------------------------------------------------------------------
// Repository-local git state that makes git misreport the tree. The verdict
// used to rest on three git answers (is-ancestor, diff, status); each test
// below made all three say "certified, unchanged" while the code differed.
// ---------------------------------------------------------------------------

/// The reviewer's composite spoof: the agent's commit is `git replace`d by a
/// commit carrying the certified tree, the index is reset to it, and the
/// changed file is marked skip-worktree. The worktree AND the real HEAD hold
/// the agent's code.
#[test]
fn a_replaced_head_with_a_skip_worktree_index_is_not_certified() {
    let Some(c) = certified() else { return };
    let (before, agent) = c.commit_changed_code();
    let fake = git_out(
        &c.repo,
        &[
            "commit-tree",
            &format!("{before}^{{tree}}"),
            "-p",
            &before,
            "-m",
            "fake",
        ],
    );
    git(&c.repo, &["replace", &agent, &fake]);
    git(&c.repo, &["read-tree", "HEAD"]);
    git(&c.repo, &["update-index", "--skip-worktree", CODE]);
    assert_eq!(c.worktree_code(), "// the agent's code\n");
    c.refused("refs/replace/ object replacements");
}

/// `git replace` alone: the working tree is reset to the certified bytes, but
/// the commit HEAD really names (the one that would be built or shipped from
/// this history) holds the agent's code.
#[test]
fn a_replaced_head_commit_is_not_certified() {
    let Some(c) = certified() else { return };
    let (before, agent) = c.commit_changed_code();
    let fake = git_out(
        &c.repo,
        &[
            "commit-tree",
            &format!("{before}^{{tree}}"),
            "-p",
            &before,
            "-m",
            "fake",
        ],
    );
    git(&c.repo, &["replace", &agent, &fake]);
    git(&c.repo, &["read-tree", "-u", "--reset", "HEAD"]);
    assert_eq!(c.worktree_code(), "// code\n");
    assert_eq!(head(&c.repo), agent, "HEAD really is the agent's commit");
    c.refused("refs/replace/ object replacements");
}

/// info/grafts rewrites ancestry: a history that does not descend from the
/// certified revision reads as one that does.
#[test]
fn grafted_ancestry_is_not_certified() {
    let Some(c) = certified() else { return };
    let orphan = git_out(
        &c.repo,
        &["commit-tree", "HEAD^{tree}", "-m", "unrelated history"],
    );
    git(&c.repo, &["update-ref", "refs/heads/main", &orphan]);
    c.refused("is not an ancestor of this tree");
    let grafts = c.repo.join(".git/info/grafts");
    write(&grafts, &format!("{orphan} {}\n", c.certified_sha()));
    c.refused("info/grafts file");
}

/// skip-worktree alone: an uncommitted change git is told not to look at.
#[test]
fn a_skip_worktree_entry_is_not_certified() {
    let Some(c) = certified() else { return };
    write(&c.repo.join(CODE), "// the agent's code\n");
    git(&c.repo, &["update-index", "--skip-worktree", CODE]);
    c.refused("is marked skip-worktree");
}

/// assume-unchanged alone: the same, through the other index flag.
#[test]
fn an_assume_unchanged_entry_is_not_certified() {
    let Some(c) = certified() else { return };
    write(&c.repo.join(CODE), "// the agent's code\n");
    git(&c.repo, &["update-index", "--assume-unchanged", CODE]);
    c.refused("is marked assume-unchanged");
}

/// A zlib stream of `data` in one stored (uncompressed) deflate block.
fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let n = u16::try_from(data.len()).unwrap();
    let mut o = vec![0x78, 0x01, 0x01];
    o.extend(n.to_le_bytes());
    o.extend((!n).to_le_bytes());
    o.extend(data);
    let (mut a, mut b) = (1u32, 0u32);
    for &x in data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    o.extend(((b << 16) | a).to_be_bytes());
    o
}

/// A forged loose object: the certified revision's root tree object is
/// overwritten with the agent's tree. Git verifies a commit's hash when it
/// parses one, but not a tree it reads for `diff`, so the certified name now
/// reads as the agent's code.
#[test]
fn a_forged_object_under_the_certified_name_is_not_certified() {
    let Some(c) = certified() else { return };
    let cert_tree = git_out(
        &c.repo,
        &["rev-parse", &format!("{}^{{tree}}", c.certified_sha())],
    );
    c.commit_changed_code();
    let agent_tree = git_raw(&c.repo, &["cat-file", "tree", "HEAD^{tree}"]);
    let mut obj = format!("tree {}\0", agent_tree.len()).into_bytes();
    obj.extend(&agent_tree);
    let loose = c
        .repo
        .join(".git/objects")
        .join(&cert_tree[..2])
        .join(&cert_tree[2..]);
    assert!(loose.exists(), "the fixture's objects are loose");
    std::fs::write(&loose, zlib_stored(&obj)).unwrap();
    c.refused("does not hash to its name");
}

/// A rename into governance/: `git diff --name-only` with rename detection
/// names only the destination, so the removed code looked governance-only.
#[test]
fn moving_code_into_governance_is_a_change() {
    let Some(c) = certified() else { return };
    git(&c.repo, &["mv", CODE, "governance/lib.rs"]);
    c.commit("move code under governance/");
    c.refused("changed since the certified revision");
}
