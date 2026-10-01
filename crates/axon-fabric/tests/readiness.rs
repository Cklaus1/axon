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
#[path = "common/git_attacks.rs"]
mod git_attacks;
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

/// C9 round 1 (class: a stat error read as "absent"): a narrowing list that is
/// present but unreadable, here a dangling symlink, must not silently vanish.
#[test]
fn a_narrowing_list_that_cannot_be_read_is_not_read_as_absent() {
    let Some(c) = certified() else { return };
    let exp = c.repo.join("governance/status/trust-expectations.json");
    std::fs::create_dir_all(exp.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&exp);
    std::os::unix::fs::symlink("nowhere.json", &exp).unwrap();
    c.commit("the narrowing list now points nowhere");
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: a narrowing list that cannot be read was read as absent: {v}"
    );
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

    // A byte changed after the operator signed: the micode_sha, the one
    // field only the signature stands behind (every other is also joined to
    // a verified document, amendment 57), so the signature is what refuses.
    let Some(c) = certified() else { return };
    let mut bytes = std::fs::read(c.record()).unwrap();
    let s = String::from_utf8(bytes.clone())
        .unwrap()
        .replace(&"a".repeat(40), &"b".repeat(40));
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
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: certified PASS despite the attack: a trust preflight that is no certified evidence file: {v}"
    );
    c.refused("names no certified evidence file");

    let Some(c) = certified() else { return };
    write(
        &c.repo.join(PREFLIGHT),
        &json!({"schema": TRUST_PREFLIGHT_SCHEMA, "mode": "dev", "verdict": "PASS"}).to_string(),
    );
    let pf = sha(&c.repo.join(PREFLIGHT));
    // The bundle over the record's OWN evidence list (it names the observation
    // and the B263 record too since C9 round 1), so the dev-mode preflight is
    // the ONLY thing wrong and the protected-mode check the only refusal.
    let rec: serde_json::Value =
        serde_json::from_slice(&std::fs::read(c.record()).unwrap()).unwrap();
    let evidence: Vec<String> = rec["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect();
    let refs: Vec<&str> = evidence.iter().map(String::as_str).collect();
    let bundle = bundle_of(&c.repo, &refs);
    resign(&c, &c.operator, |r| {
        r["trust_preflight_sha256"] = json!(pf);
        r["evidence_bundle_sha256"] = json!(bundle);
    });
    c.commit("dev-mode preflight");
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: certified PASS despite the attack: a dev-mode trust preflight: {v}"
    );
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
        // The domain field (M152) and the domain-separated message (M153)
        // each refuse this alone (M152's four-cell record): either reason.
        let v = c.refused_any().to_string();
        assert!(
            v.contains("is for authority") || v.contains("does not verify"),
            "{v}"
        );
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
    // Refused by the replace refusal (M285), the skip-worktree refusal (M287)
    // and the object-store checks, each alone: any refusal is correct (C9 round 1).
    c.refused_any();
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
    // Three independent layers refuse this, each alone: the refs/replace/
    // refusal (M285), git run with replacement objects disabled (M385), and
    // every object re-hashed to its name (M289). Any refusal is correct; only
    // all three removed certifies it (four-cell record, C9 round 1).
    c.refused_any();
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
    // Refused by the info/grafts refusal (M286) AND by the ancestry walk over
    // hash-checked commit objects, which never reads grafts (M581), each
    // alone (four-cell record, C9 round 3): any refusal is correct.
    c.refused_any();
}

/// skip-worktree alone: an uncommitted change git is told not to look at.
#[test]
fn a_skip_worktree_entry_is_not_certified() {
    let Some(c) = certified() else { return };
    write(&c.repo.join(CODE), "// the agent's code\n");
    git(&c.repo, &["update-index", "--skip-worktree", CODE]);
    // Refused by the skip-worktree refusal (M287) AND by the working tree's
    // own bytes re-hashed against the certified tree (M290), each alone
    // (four-cell record, C9 round 1): any refusal is correct.
    c.refused_any();
}

/// assume-unchanged alone: the same, through the other index flag.
#[test]
fn an_assume_unchanged_entry_is_not_certified() {
    let Some(c) = certified() else { return };
    write(&c.repo.join(CODE), "// the agent's code\n");
    git(&c.repo, &["update-index", "--assume-unchanged", CODE]);
    // Refused by the assume-unchanged refusal (M288) AND by the working
    // tree's bytes re-hashed (M290), each alone (four-cell record, C9 round 1).
    c.refused_any();
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

/// PSV-7 (C9 round 2): the repository's own .git/config makes it a partial
/// clone whose promisor remote is reached through its own core.sshCommand.
/// Any object missing during the walk used to be lazily fetched, running
/// that command as the verifier.
#[test]
fn a_promisor_remote_in_the_repository_config_runs_nothing_as_the_verifier() {
    let Some(c) = certified() else { return };
    let marker = c._d.path().join("ssh-command-ran");
    for (k, v) in [
        ("core.repositoryformatversion", "1".to_string()),
        ("extensions.partialClone", "origin".to_string()),
        (
            "remote.origin.url",
            "ssh://attacker.invalid/r.git".to_string(),
        ),
        ("remote.origin.promisor", "true".to_string()),
        (
            "core.sshCommand",
            format!("touch {}; false", marker.display()),
        ),
    ] {
        git(&c.repo, &["config", k, &v]);
    }
    // The certified revision's root tree goes missing from the object store.
    let cert_tree = git_out(
        &c.repo,
        &["rev-parse", &format!("{}^{{tree}}", c.certified_sha())],
    );
    let loose = c
        .repo
        .join(".git/objects")
        .join(&cert_tree[..2])
        .join(&cert_tree[2..]);
    std::fs::remove_file(&loose).unwrap();
    let v = c.verdict();
    assert!(
        !marker.exists(),
        "ATTACK: readiness lazily fetched a missing object and ran the repository's \
         core.sshCommand as the verifier"
    );
    assert_ne!(v["status"], "PASS", "{v}");
}

// ── Operator decision C (amendment 44, A70): git-ignore has no authority.
// The working tree counts as a FILESYSTEM: an object outside governance/ that
// is not in the certified tree is a change, however git is told to ignore
// it, unless the operator's provenance allowlist excuses it.

fn not_certified(c: &Certified, attack: &str) -> Value {
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: {attack}, and readiness still certified the tree: {v}"
    );
    v
}

/// Write the operator's allowlist (root-owned, 0644) for this fixture.
fn install_allowlist(c: &Certified, entries: &str) -> std::path::PathBuf {
    let p = c.trust.allowlist_path().to_path_buf();
    write(&p, &format!("axon-provenance-allowlist/1\n{entries}"));
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    p
}

#[test]
fn a_gitignored_input_is_not_certified() {
    let Some(c) = certified() else { return };
    // `.env` is ignored by the certified tree's own .gitignore, as in the
    // real repository, and axon-ai reads it as configuration.
    write(
        &c.repo.join(".env"),
        "AXON_AI_BASE_URL=http://attacker.invalid\n",
    );
    let v = not_certified(&c, "a .gitignored .env was ignored as a change");
    assert!(
        v.to_string()
            .contains(".env: a file that is not in the tree"),
        "{v}"
    );
}

#[test]
fn an_info_exclude_hidden_file_is_not_certified() {
    let Some(c) = certified() else { return };
    write(&c.repo.join(".git/info/exclude"), ".cargo/\n");
    write(
        &c.repo.join(".cargo/config.toml"),
        "[build]\nrustflags = [\"--cfg\", \"agent\"]\n",
    );
    let v = not_certified(
        &c,
        "info/exclude hid an untracked .cargo/config.toml from the certification",
    );
    assert!(v.to_string().contains(".cargo/: a directory"), "{v}");
}

#[test]
fn an_untracked_source_is_not_certified() {
    let Some(c) = certified() else { return };
    write(
        &c.repo.join("crates/axon-fabric/build.rs"),
        "fn main() {}\n",
    );
    not_certified(&c, "an untracked build.rs was not a change");
}

#[test]
fn only_the_operators_allowlist_excuses_generated_output() {
    let Some(c) = certified() else { return };
    write(&c.repo.join("target/debug/axon-fabric"), "build output\n");
    // No allowlist: nothing is excused, ignored or not.
    let v = not_certified(&c, "an ignored target/ was excused without an allowlist");
    assert!(v.to_string().contains("target/: a directory"), "{v}");
    // The operator's allowlist excuses it (the control)...
    let p = install_allowlist(&c, "target/\ndist/\n");
    assert_eq!(c.verdict()["status"], "PASS", "{}", c.verdict());
    // ...and only it.
    write(&c.repo.join(".env"), "X=1\n");
    not_certified(&c, "an allowlist for target/ also excused .env");
    std::fs::remove_file(c.repo.join(".env")).unwrap();
    // An allowlist the repository's writer could edit excuses nothing.
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o666)).unwrap();
    let v = not_certified(&c, "an other-writable allowlist excused target/");
    assert!(v.to_string().contains("not operator-owned"), "{v}");
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::os::unix::fs::chown(&p, Some(1000), None).unwrap();
    not_certified(&c, "an allowlist owned by another uid excused target/");
    std::os::unix::fs::chown(&p, Some(0), None).unwrap();
    // An entry that covers a source is refused whole.
    install_allowlist(&c, "target/\ncrates/\n");
    let v = not_certified(&c, "an allowlist entry covering crates/ was accepted");
    assert!(v.to_string().contains("covers the tracked path"), "{v}");
}

// ── Operator decision E (amendment 44, A71): a protected answer comes from a
// standalone clone. A linked worktree (a `.git` FILE) is refused.

#[test]
fn a_linked_worktree_is_not_certified() {
    let Some(c) = certified() else { return };
    let wt = c._d.path().join("wt");
    git(
        &c.repo,
        &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
    );
    assert!(std::fs::symlink_metadata(wt.join(".git"))
        .unwrap()
        .is_file());
    let v = protected_components(&wt, &c.trust)["components"]["protected_backend"].clone();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: readiness certified a linked worktree (a gitfile names the repository): {v}"
    );
    // The gitfile refusal (M453) and the common-dir rule (M580) each refuse
    // it alone (four-cell record, C9 round 3): either reason.
    assert!(
        v.to_string().contains("gitfile") || v.to_string().contains("linked worktree"),
        "{v}"
    );
    assert_eq!(
        c.verdict()["status"],
        "PASS",
        "control: the standalone clone"
    );
}

/// Decision E by what git acts on (review PSV-7, C9 round 3; A79): the same
/// linked worktree with its admin directory copied in as a real `.git`
/// (`commondir` naming the clone's `.git`) was certified PASS, because only
/// the KIND of `.git` was checked. Control: the standalone clone.
#[test]
fn a_linked_worktree_disguised_as_a_git_directory_is_not_certified() {
    let Some(c) = certified() else { return };
    let wt = c._d.path().join("wt");
    git_attacks::disguise_worktree(&c.repo, &wt);
    let v = protected_components(&wt, &c.trust)["components"]["protected_backend"].clone();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: readiness certified a linked worktree whose admin dir was placed as .git: {v}"
    );
    assert!(v.to_string().contains("linked worktree"), "{v}");
    assert_eq!(
        c.verdict()["status"],
        "PASS",
        "control: the standalone clone"
    );
}

/// A80 on the readiness route: the certified revision's ancestry is read from
/// hash-checked objects. HEAD is replaced by D (parent C, an orphan with the
/// certified tree); C's loose object is then forged, under its own name, to
/// name the certified commit as its parent. Control: before the forgery
/// readiness refuses for ancestry; after it, still refused.
#[test]
fn a_forged_ancestor_object_does_not_make_the_tree_descend_from_the_certified_revision() {
    let Some(c) = certified() else { return };
    let certified = c.certified_sha();
    let orphan = git_attacks::commit_tree(&c.repo, &[], "C");
    let d = git_attacks::commit_tree(&c.repo, &[&orphan], "D");
    git(&c.repo, &["update-ref", "refs/heads/main", &d]);
    c.refused("is not an ancestor of this tree");
    git_attacks::forge_parent(&c.repo, &orphan, &certified);
    let v = c.refused_any();
    assert!(v.to_string().contains("is not an ancestor"), "{v}");
}

// ── C9 round 4, EQUIVALENCE (rows workstream; M690-M699) ────────────────────
//
// Each readiness rule below was killed only by a unit test calling it
// directly, or its call site had no row. These tests reach it through the
// production decision: the installed `axon-fabric verify-readiness`, whose
// trust is `ReadinessTrust::operator()` (the operator's /etc/axon/trust roots,
// the system clock), run in a private mount namespace with a tmpfs at
// /etc/axon. The host's /etc is never written.

/// The fixture's B263 record re-dated to the system clock (the production
/// decision reads the real time), re-signed, and the record re-bound to it
/// and to `verifier` (the verify-readiness binary that will decide). Commits.
fn bind_to_system_clock_and(c: &Certified, verifier: &std::path::Path) {
    let utc = |ago_s: u64| {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            - ago_s;
        let o = std::process::Command::new("date")
            .args(["-u", "-d", &format!("@{t}"), "+%Y-%m-%dT%H:%M:%SZ"])
            .output()
            .unwrap();
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    // B263 issued, then the run observed, then certified, all in the past
    // of the system clock (the record is current at both `observed_at` and
    // now, A89): the run is launched again under the re-dated record, so the
    // launch manifest names its digest (M745).
    let mut b = b263_record(&c.operator);
    b["start"] = json!(utc(4 * 3600));
    b["end"] = json!(utc(3 * 3600));
    write_signed_for(
        &c.operator,
        TrustAuthority::Qualification,
        &c.repo.join(B263),
        &b,
    );
    let observed_at = utc(2 * 3600);
    relaunch(
        c,
        keep(),
        Box::new(move |o: &mut Value| o["observed_at"] = json!(observed_at)),
        keep(),
    );
    let v_sha = sha(verifier);
    rebundle(c, |r| {
        r["readiness_verifier_sha256"] = json!(v_sha);
        r["certified_at"] = json!(utc(3600));
    });
    c.commit("certification re-dated for the production decision (governance only)");
}

/// `verify-readiness --repo` for the certified repository, decided with the
/// operator's trust (`ReadinessTrust::operator()`): the fixture's three roots
/// installed at /etc/axon/trust/ (root-owned, 0755/0644) in a private mount
/// namespace. Returns (the verdict as root, the verdict as uid 4242).
fn production_verdicts(c: &Certified) -> (Value, Value) {
    production_verdicts_by(c, std::path::Path::new(env!("CARGO_BIN_EXE_axon-fabric")))
}

/// [`production_verdicts`], decided by the `axon-fabric` build at `verifier`.
fn production_verdicts_by(c: &Certified, verifier: &std::path::Path) -> (Value, Value) {
    let d = c._d.path();
    let bin = d.join("axon-fabric");
    std::fs::copy(verifier, &bin).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    bind_to_system_clock_and(c, &bin);
    let script = "set -e\n\
         mount -t tmpfs -o mode=0755 tmpfs /etc/axon\n\
         cp -a \"$1/trust\" /etc/axon/trust\n\
         chown -R 0:0 /etc/axon\n\
         chmod -R go-w /etc/axon\n\
         set +e\n\
         \"$1/axon-fabric\" verify-readiness --repo \"$2\" > \"$1/as-root.json\"\n\
         setpriv --reuid=4242 --regid=4242 --clear-groups -- \
         \"$1/axon-fabric\" verify-readiness --repo \"$2\" > \"$1/as-4242.json\"\n\
         exit 0\n";
    let st = std::process::Command::new("unshare")
        .args(["-m", "--propagation", "private", "sh", "-c", script, "sh"])
        .arg(d)
        .arg(&c.repo)
        .status()
        .unwrap();
    assert!(st.success(), "setup: the namespace script failed");
    let read = |n: &str| -> Value {
        let s = std::fs::read_to_string(d.join(n)).unwrap_or_default();
        serde_json::from_str::<Value>(&s)
            .unwrap_or_else(|e| panic!("setup: {n} is not a verdict ({e}): {s}"))["components"]
            ["protected_backend"]
            .clone()
    };
    (read("as-root.json"), read("as-4242.json"))
}

fn can_mount_etc_axon() -> bool {
    if !std::path::Path::new("/etc/axon").is_dir() {
        eprintln!("skipped: no /etc/axon mount point (this test never creates one)");
        return false;
    }
    true
}

/// Readiness (M690; M490, M491 and M492 re-anchored here), ROOT ONLY: the
/// production decision refuses a trust root the process deciding can WRITE.
/// Root can write every root on the host, so an operator (or an agent that
/// became root) running `verify-readiness` as root decides with authority it
/// could have planted itself. The production trust requires unwritable roots
/// (M492), `check()` enforces it (M490) through `writable_by_me` (M491), and
/// the decision calls `check()` (M690): with any one gone, the root run
/// certifies. Control: the same decision, by a uid that cannot write the
/// roots, certifies, so the fixture reaches PASS through the production
/// trust and only the writability refuses.
#[test]
fn a_readiness_run_that_can_write_its_trust_roots_certifies_nothing() {
    let Some(c) = certified() else { return };
    if !can_mount_etc_axon() {
        return;
    }
    let (as_root, as_other) = production_verdicts(&c);
    assert_eq!(
        as_other["status"], "PASS",
        "control: a verifier uid that cannot write the operator's roots certifies: {as_other}"
    );
    assert_ne!(
        as_root["status"], "PASS",
        "ATTACK: a readiness run that can write its operator trust roots certified PASS: \
         {as_root}"
    );
    assert!(
        as_root
            .to_string()
            .contains("writable by the process running this check"),
        "{as_root}"
    );
}

/// The verdict must not be PASS: the attack is the record (or tree) `attack`
/// describes getting certified. Control first: the untouched fixture PASSes.
fn not_pass_under(c: &Certified, attack: &str) -> Value {
    let v = c.verdict();
    assert_ne!(v["status"], "PASS", "ATTACK: {attack}: {v}");
    v
}

/// Each binding of the operator's record to THIS repository and component
/// had no row (C9 round 4, EQUIVALENCE). One attack per binding, each on the
/// route where that binding is the only check that refuses it.
///
/// M691: an evidence file changed after certification. `run-evidence.md` is
/// bound by nothing but the bundle digest (the observation and B263 record
/// are also bound one by one), and a change under governance/ is allowed.
#[test]
fn an_evidence_file_changed_after_certification_is_not_certified() {
    let Some(c) = certified() else { return };
    write(
        &c.repo
            .join("governance/proofs/v022-protected/run-evidence.md"),
        "the agent's evidence\n",
    );
    c.commit("governance: other evidence");
    let v = not_pass_under(
        &c,
        "an evidence file changed after certification and readiness still certified the bundle",
    );
    assert!(v.to_string().contains("evidence bundle changed"), "{v}");
}

/// M692-M694: a genuine operator record for another component, or of another
/// host or qualification profile, certifies nothing here. Each is re-signed
/// by the operator (so the signature holds) and differs in one field only.
#[test]
fn a_record_for_another_component_or_profile_is_not_certified() {
    for (field, value, attack) in [
        (
            "component",
            "pci_on_protected_backend",
            "a record certifying another component certified protected_backend",
        ),
        (
            "host_profile",
            "linux-microvm-dev",
            "a record of another host profile certified the protected profile",
        ),
        (
            "qualification_profile",
            "linux-microvm-dev",
            "a record of another qualification profile certified the protected profile",
        ),
    ] {
        let Some(c) = certified() else { return };
        resign(&c, &c.operator, |r| r[field] = json!(value));
        let v = not_pass_under(&c, attack);
        assert!(
            v.to_string()
                .contains("not a linux-microvm-protected certification of protected_backend"),
            "{field}: {v}"
        );
    }
}

/// M695: the PSV spec the record certifies is the one in this tree. The spec
/// sits under governance/, where a change is otherwise allowed, and it is not
/// part of the evidence bundle: the spec digest is its only binding.
#[test]
fn a_changed_psv_spec_is_not_certified() {
    let Some(c) = certified() else { return };
    write(
        &c.repo
            .join("governance/specs/v022-protected-suite-verdict.md"),
        "# PSV, as the agent would have it\n",
    );
    c.commit("governance: another PSV");
    let v = not_pass_under(
        &c,
        "the PSV spec changed after certification and readiness still certified",
    );
    assert!(v.to_string().contains("certifies another version"), "{v}");
}

/// M696: HEAD is an orphan history (C, then D) holding the certified tree
/// byte for byte. Every tree comparison finds no change; only the ancestry
/// of the certified revision refuses it.
#[test]
fn a_history_not_descending_from_the_certified_revision_is_not_certified() {
    let Some(c) = certified() else { return };
    let orphan = git_attacks::commit_tree(&c.repo, &[], "C");
    let d = git_attacks::commit_tree(&c.repo, &[&orphan], "D");
    git(&c.repo, &["update-ref", "refs/heads/main", &d]);
    let v = not_pass_under(
        &c,
        "an orphan history holding the certified tree was certified as the certified revision's \
         descendant",
    );
    assert!(v.to_string().contains("is not an ancestor"), "{v}");
}

/// M697: a committed change to code outside governance/. The history still
/// descends from the certified revision, and the record and evidence are
/// untouched: the outside-governance comparison is the only refusal.
#[test]
fn a_committed_code_change_is_not_certified() {
    let Some(c) = certified() else { return };
    write(&c.repo.join(CODE), "// the agent's code\n");
    c.commit("agent: code");
    let v = not_pass_under(
        &c,
        "code changed outside governance/ after certification and readiness still certified",
    );
    assert!(
        v.to_string()
            .contains("changed since the certified revision"),
        "{v}"
    );
}

/// M698: a record of another schema is not this certification, even with
/// every field present and the operator's signature over it.
#[test]
fn a_record_of_another_schema_is_not_certified() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["schema"] = json!("axon-v022-protected-certification/1")
    });
    let v = not_pass_under(
        &c,
        "a record of another schema was read as a protected certification",
    );
    assert!(
        v.to_string()
            .contains("not a axon-v022-protected-certification/2"),
        "{v}"
    );
}

/// M699: the record's attribution is checked at the decision. Here it names
/// an observer key the operator's observer root does not hold (re-signed by
/// the operator, so the signature holds): only the attribution refuses it.
#[test]
fn a_record_attributed_to_an_untrusted_observer_is_not_certified() {
    let Some(c) = certified() else { return };
    let stranger = Issuer::generate();
    resign(&c, &c.operator, |r| {
        r["observer_key_id"] = json!(stranger.key_id())
    });
    let v = not_pass_under(
        &c,
        "a record naming an observer key outside the operator's observer root was certified",
    );
    assert!(
        v.to_string()
            .contains("is not a key in the operator's observer root"),
        "{v}"
    );
}

// ── C9 round 4 fix wave, ROWS2 (EQUIVALENCE (4); rows M760-M819) ────────────
//
// Refusal sites of readiness.rs that had neither a row nor an exemption
// (`scripts/v022_refusal_coverage.py`, now scanning this file). Each is
// attacked on the production decision.

/// PRODUCTION builds of `axon-fabric` (no test-trust-root), once per test
/// process: `(clean, dirty)`. Both are built from one copy of this
/// workspace's tracked sources, as they are in the working tree, made a
/// standalone clone of its own: `clean` with the copy committed (it reports
/// `source_dirty: false`), `dirty` with one uncommitted file added
/// (`source_dirty: true`). The copy keeps the sources' timestamps and the
/// commit is deterministic, so a later process rebuilds only what changed.
fn production_verifiers() -> &'static (std::path::PathBuf, std::path::PathBuf) {
    use std::path::{Path, PathBuf};
    use std::process::Command;
    static V: std::sync::OnceLock<(PathBuf, PathBuf)> = std::sync::OnceLock::new();
    V.get_or_init(|| {
        let exe = PathBuf::from(env!("CARGO_BIN_EXE_axon-fabric"));
        let base = exe
            .parent()
            .and_then(Path::parent)
            .unwrap()
            .join("rows2-production-verifier");
        let (src, target) = (base.join("src"), base.join("target"));
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(&src).unwrap();
        let ws = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let st = Command::new("sh")
            .args([
                "-c",
                "git -C \"$1\" ls-files -z -- Cargo.toml Cargo.lock rust-toolchain.toml .cargo \
                 crates | tar -C \"$1\" --null -T - -cf - | tar -xpf - -C \"$2\"",
                "sh",
            ])
            .arg(&ws)
            .arg(&src)
            .status()
            .unwrap();
        assert!(st.success(), "setup: copying the workspace sources failed");
        let git = |args: &[&str]| {
            let st = Command::new("git")
                .current_dir(&src)
                .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
                .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
                .args(["-c", "user.name=rows2", "-c", "user.email=rows2@invalid"])
                .args(args)
                .status()
                .unwrap();
            assert!(st.success(), "setup: git {args:?} failed");
        };
        git(&["init", "-q", "-b", "main"]);
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "copy"]);
        let build = |name: &str, dirty: bool| -> PathBuf {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let out = Command::new(cargo)
                .current_dir(&src)
                .args([
                    "build",
                    "--offline",
                    "-j",
                    "4",
                    "-p",
                    "axon-fabric",
                    "--bin",
                    "axon-fabric",
                    "--target-dir",
                ])
                .arg(&target)
                .env_remove("CARGO_TARGET_DIR")
                .env_remove("CARGO_BUILD_TARGET_DIR")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "setup: the production build failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let bin = base.join(name);
            std::fs::copy(target.join("debug/axon-fabric"), &bin).unwrap();
            let m = Command::new(&bin)
                .arg("verifier-manifest")
                .output()
                .unwrap();
            let v: Value = serde_json::from_slice(&m.stdout).unwrap();
            assert!(
                v["build"] == "production" && v["source_dirty"] == dirty,
                "setup: not a {} production build: {v}",
                if dirty { "dirty" } else { "clean" }
            );
            bin
        };
        let clean = build("clean-axon-fabric", false);
        write(
            &src.join("crates/axon-fabric/src/UNCOMMITTED.txt"),
            "a change nobody committed\n",
        );
        let dirty = build("dirty-axon-fabric", true);
        (clean, dirty)
    })
}

/// PSV-7 (M769, M772, M773), ROOT ONLY, PRODUCTION BUILD: a production
/// readiness verifier built from a DIRTY tree certifies nothing, even when
/// the operator's record names exactly its bytes: which source it is cannot
/// be told from its revision. And each build names itself in its verdict (the
/// report's `build`, and the verifier's own identity, which the relay
/// `scripts/protected_verifier_ready.py` requires to be `production`): a
/// test-trust build never reports itself as a production one. Control: the
/// production verifier built from the SAME sources committed certifies,
/// through the same decision, so only the dirty build refuses.
#[test]
fn a_production_verifier_built_from_a_dirty_tree_certifies_nothing() {
    if !can_mount_etc_axon() {
        return;
    }
    let report = |bin: &std::path::Path| -> Value {
        let o = std::process::Command::new(bin)
            .args(["verify-readiness", "--repo", "/nonexistent"])
            .output()
            .unwrap();
        serde_json::from_slice(&o.stdout).unwrap()
    };
    let r = report(std::path::Path::new(env!("CARGO_BIN_EXE_axon-fabric")));
    assert_eq!(
        r["verifier"]["build"], "test-trust",
        "ATTACK: a test-trust verifier's identity names the production build: {r}"
    );
    assert_eq!(
        r["build"], "test-trust",
        "ATTACK: a test-trust verifier's report names the production build: {r}"
    );
    let (clean, dirty) = production_verifiers();
    let r = report(clean);
    assert!(
        r["build"] == "production" && r["verifier"]["build"] == "production",
        "control: a production verifier names itself so: {r}"
    );
    let Some(c) = certified() else { return };
    let (_, v) = production_verdicts_by(&c, dirty);
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: a production verifier built from a dirty tree certified PASS: {v}"
    );
    assert!(v.to_string().contains("built from a dirty tree"), "{v}");
    let Some(c) = certified() else { return };
    let (_, v) = production_verdicts_by(&c, clean);
    assert_eq!(
        v["status"], "PASS",
        "control: the production verifier built from the same sources committed certifies: {v}"
    );
}

/// C9 round 1 class, production decision (M770), ROOT ONLY: a narrowing
/// list the verifier cannot STAT is not read as absent. The repository names
/// only a stranger as its qualification issuer; its directory is 0700 root,
/// so the production verifier, running as uid 4242, gets EACCES on it. Read
/// as absent, the narrowing would vanish and the operator's issuer be
/// accepted. Control: the same list, readable, refuses the operator's issuer
/// (the list is honoured), so only the stat error stands between the two.
#[test]
fn a_narrowing_list_the_verifier_cannot_stat_is_not_read_as_absent() {
    if !can_mount_etc_axon() {
        return;
    }
    let narrow = |c: &Certified| {
        write(
            &c.repo.join("governance/status/trust-expectations.json"),
            &json!({"qualification_issuers": ["ed25519:ffffffffffffffff"]}).to_string(),
        );
        c.commit("narrow to a stranger");
    };
    let Some(c) = certified() else { return };
    narrow(&c);
    std::fs::set_permissions(
        c.repo.join("governance/status"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let (_, v) = production_verdicts(&c);
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: a narrowing list the verifier could not stat was read as absent: {v}"
    );
    assert!(v.to_string().contains("Permission denied"), "{v}");
    let Some(c) = certified() else { return };
    narrow(&c);
    let (_, v) = production_verdicts(&c);
    assert!(
        v.to_string().contains("not one this repository expects"),
        "control: the readable list is honoured: {v}"
    );
}
