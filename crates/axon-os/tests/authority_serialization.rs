//! Every field that can alter effective authority must be bound by the digests.
//!
//! REPRODUCED before the fix: two manifests differing ONLY by
//! `profile = "hermetic"` vs `"restricted"`, with one hand-minted approval
//! token copied byte-identically to both, BOTH printed
//! "Approval: required and verified (program + grant unedited since sign-off)".
//! The restricted run then loaded and executed operator-ambient code through
//! `AXON_PATH` — a module search path, i.e. a code-injection channel — that the
//! hermetic posture strips.
//!
//! Separately, a manifest differing only by `require_approval = true/false`
//! produced an IDENTICAL `manifest_digest`, so the record and the whole hash
//! chain built on it could not distinguish a job demanding sign-off from one
//! that disarms the gate.

use axon_os::approval::canonical_grant;
use axon_os::grant::{Budget, ExecPolicy, Grant, Label};
use axon_os::manifest::{parse, to_axjob, JobManifest};
use axon_os::record::canonical_manifest;

fn base_grant() -> Grant {
    Grant {
        fs_read: vec!["./in/".into()],
        fs_write: vec!["./out/".into()],
        net: vec![],
        exec: ExecPolicy::None,
        max_label: Label::Internal,
        budget: Budget {
            calls: 1,
            tokens: 2,
            cost_micro: 3,
        },
        reproducible: false,
    }
}

/// Mutate ONE field from a single base. Building the two grants independently
/// would let an unrelated difference produce the inequality, so the test could
/// pass while the field under examination was still unbound.
#[test]
fn the_grant_digest_binds_reproducible() {
    let a = base_grant();
    let mut b = base_grant();
    b.reproducible = true;
    assert_ne!(
        canonical_grant(&a),
        canonical_grant(&b),
        "a hermetic and a non-hermetic grant encoded IDENTICALLY, so an approval \
         token signed over one verifies the other — and `reproducible` gates \
         AXON_PATH, a code-injection channel"
    );
}

fn manifest_with(reproducible: bool, require_approval: bool) -> JobManifest {
    let mut g = base_grant();
    g.reproducible = reproducible;
    JobManifest {
        program: "p.ax".into(),
        intent: "i".into(),
        seed: 7,
        grant: g,
        require_approval,
    }
}

#[test]
fn the_manifest_seal_binds_reproducible_and_require_approval() {
    let base = manifest_with(false, false);
    assert_ne!(
        canonical_manifest(&base),
        canonical_manifest(&manifest_with(true, false)),
        "the record seal ignored `reproducible`"
    );
    assert_ne!(
        canonical_manifest(&base),
        canonical_manifest(&manifest_with(false, true)),
        "the record seal ignored `require_approval`, so a job that disarms the \
         approval gate sealed identically to one that demands it"
    );
}

/// Round-trip totality. Stated over the WHOLE struct rather than a field list,
/// so it catches the NEXT field too — which a hand-written list never would.
#[test]
fn archiving_a_manifest_preserves_every_authority_field() {
    let m = manifest_with(true, true);
    let text = to_axjob(&m);
    let back =
        parse(&text, std::path::Path::new(".")).expect("a manifest we just emitted must parse");
    assert_eq!(
        back.grant.reproducible, m.grant.reproducible,
        "archiving dropped `reproducible`: `axon-os replay` reloads this file, \
         so a hermetic job replayed under the ambient environment"
    );
    assert_eq!(
        back.require_approval, m.require_approval,
        "archiving dropped `require_approval`, so a replayed job never \
         re-checks sign-off"
    );
    assert_eq!(back.seed, m.seed);
    assert_eq!(back.grant.fs_write, m.grant.fs_write);
}

/// The compile-time guarantee, guarded at the source level.
///
/// The three canonicalizers destructure their struct exhaustively, so adding a
/// field is a BUILD ERROR until it is bound. That property is invisible to a
/// behavioural test — it holds only while no `..` is introduced, and a `..`
/// would silently restore the original defect for every future field.
#[test]
fn the_canonicalizers_bind_every_field_exhaustively() {
    let cases = [
        ("src/approval.rs", "canonical_grant"),
        ("src/record.rs", "canonical_manifest"),
        ("src/manifest.rs", "to_axjob"),
    ];
    for (file, func) in cases {
        let src =
            std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(file))
                .unwrap_or_else(|e| panic!("read {file}: {e}"));
        let at = src
            .find(&format!("fn {func}"))
            .unwrap_or_else(|| panic!("{func} not found in {file} — this test's premise is gone"));
        // The whole function body. A fixed 700-byte window missed the
        // destructure in `canonical_manifest`, because the explanation of WHY
        // it destructures sits between the signature and the pattern — the
        // guard failed on correct code, which is the failure mode that gets a
        // guard deleted.
        let rest = &src[at..];
        let end = rest[1..]
            .find("\npub fn ")
            .or_else(|| rest[1..].find("\nfn "))
            .map(|i| i + 1)
            .unwrap_or(rest.len());
        let window = &rest[..end];
        assert!(
            window.contains("let Grant {") || window.contains("let JobManifest {"),
            "{func} no longer destructures its struct; a new field would be \
             silently omitted from the digest"
        );
        assert!(
            !window.contains(".. }") && !window.contains("..\n"),
            "{func} uses `..` in its destructure, which defeats the whole \
             guarantee: a field added later is silently dropped from the \
             authority encoding"
        );
    }
}

// ── A profile and an explicit `reproducible` must not contradict silently ────

fn manifest_src_with(profile: Option<&str>, reproducible: Option<bool>) -> String {
    let p = profile
        .map(|p| format!("profile = \"{p}\"\n"))
        .unwrap_or_default();
    let r = reproducible
        .map(|r| format!("reproducible = {r}\n"))
        .unwrap_or_default();
    format!(
        "program = \"p.ax\"\nintent = \"t\"\nseed = 1\n{p}\
         [grant]\nfs_read = []\nfs_write = []\nnet = []\nexec = \"none\"\n\
         max_label = \"internal\"\n{r}\
         [grant.budget]\ncalls = 1\ntokens = 1\ncost_micro = 0\n"
    )
}

/// `profile = "hermetic"` with `reproducible = false` parsed, explained and
/// ran, silently resolved in favour of the weaker value — so a job carrying
/// the one profile whose entire purpose is reproducibility was not
/// reproducible, and nothing in the output said so.
#[test]
fn a_reproducible_profile_cannot_be_silently_disclaimed() {
    let err = axon_os::manifest::parse(
        &manifest_src_with(Some("hermetic"), Some(false)),
        std::path::Path::new("."),
    )
    .expect_err("hermetic + reproducible = false must not parse");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("contradict"),
        "the refusal must say what is wrong: {msg}"
    );
}

/// Only the WEAKENING direction is refused. Asking for reproducibility under a
/// profile that does not promise it is coherent — `Grant::intersect` ORs the
/// bit precisely so either side may demand it — and refusing it would make
/// this a papercut rather than a safety property.
#[test]
fn asking_for_more_reproducibility_than_the_profile_promises_is_allowed() {
    let m = axon_os::manifest::parse(
        &manifest_src_with(Some("developer"), Some(true)),
        std::path::Path::new("."),
    )
    .expect("developer + reproducible = true is a strengthening, not a conflict");
    assert!(m.grant.reproducible);
}

/// The archive round-trip must survive the new check: `to_axjob` writes
/// `grant.reproducible` and NO profile line, so the two never co-occur in a
/// generated manifest. If that ever changes, every archived hermetic job
/// stops parsing — which is exactly the kind of coupling worth pinning.
#[test]
fn the_archived_form_of_a_hermetic_job_still_parses() {
    let m = axon_os::manifest::parse(
        &manifest_src_with(Some("hermetic"), None),
        std::path::Path::new("."),
    )
    .expect("hermetic parses");
    assert!(m.grant.reproducible, "premise: hermetic is reproducible");
    let archived = axon_os::manifest::to_axjob(&m);
    let back = axon_os::manifest::parse(&archived, std::path::Path::new("."))
        .expect("the archived form must parse");
    assert!(
        back.grant.reproducible,
        "the round-trip lost reproducibility, so a replayed hermetic job would \
         run under the developer profile"
    );
}

// ── `explain` must connect the profile name to the grant ────────────────────

/// A profile supplies defaults for OMITTED dimensions; an explicit value
/// overrides it. That is the documented design, not a bug — but `explain`
/// showed a job labelled `profile = "hermetic"` granting `read *`, `write *`,
/// `reach *` and `spawn processes`, said "It may NOT: (no restrictions)", and
/// exited 0, with nothing connecting the label to the grant. A reader who
/// trusts the name learns the opposite of the truth from the output whose
/// whole job is to say what a run may do.
///
/// This reports; it does not refuse. Making a profile a CEILING rather than a
/// default set is a policy change that the documented semantics contradict.
#[test]
fn explain_says_when_a_grant_diverges_from_the_profile_it_names() {
    use std::process::Command;

    let d = std::env::temp_dir().join(format!("axon_os_prof_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let prog = d.join("p.ax");
    std::fs::write(&prog, "fn main() { let _ = 1 + 1 }\n").unwrap();

    let write_job = |name: &str, body: &str| -> std::path::PathBuf {
        let p = d.join(name);
        std::fs::write(
            &p,
            format!(
                "program = \"{}\"\nintent = \"t\"\nseed = 1\nprofile = \"hermetic\"\n\
                 [grant]\n{body}max_label = \"internal\"\n\
                 [grant.budget]\ncalls = 1\ntokens = 1\ncost_micro = 0\n",
                prog.display()
            ),
        )
        .unwrap();
        p
    };

    let explain = |p: &std::path::Path| -> String {
        let out = Command::new(env!("CARGO_BIN_EXE_axon-os"))
            .arg("explain")
            .arg(p)
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).to_string()
    };

    let wide = write_job(
        "wide.axjob",
        "fs_read = [\"*\"]\nfs_write = [\"*\"]\nnet = [\"*\"]\nexec = \"any\"\n",
    );
    let txt = explain(&wide);
    assert!(
        txt.contains("read *"),
        "premise: the grant really is wide open:\n{txt}"
    );
    assert!(
        txt.contains("do NOT match this profile's defaults"),
        "a job labelled hermetic granted everything and explain said nothing \
         about the divergence:\n{txt}"
    );
    for dim in ["fs_read", "fs_write", "net", "exec"] {
        assert!(txt.contains(dim), "the note must name `{dim}`:\n{txt}");
    }

    // CONTROL: a grant that MATCHES the profile must not be reported as
    // diverging, or the note is noise everyone learns to skip.
    let matching = write_job("match.axjob", "");
    let txt = explain(&matching);
    assert!(
        txt.contains("grant matches its defaults"),
        "a conforming job must be reported as conforming:\n{txt}"
    );
    assert!(!txt.contains("do NOT match"), "false divergence:\n{txt}");

    let _ = std::fs::remove_dir_all(&d);
}

/// A duplicate `profile` key is ambiguous and worse than ambiguous: the
/// parser is last-wins, but `explain`'s profile-divergence note reads the
/// FIRST `profile =` line straight from the file text (it does not go
/// through this parser). Measured before this check existed —
/// `profile = "hermetic"` then `profile = "developer"` (no `reproducible`
/// line): the parser produced a wide-open `developer` grant, and `explain`
/// printed "Profile: hermetic — but ... do NOT match this profile's
/// defaults", naming the WRONG profile for the grant it just showed.
#[test]
fn a_duplicate_profile_key_is_refused() {
    let src = "program = \"p.ax\"\nintent = \"t\"\nseed = 1\n\
               profile = \"hermetic\"\nprofile = \"developer\"\n\
               [grant]\nmax_label = \"internal\"\n\
               [grant.budget]\ncalls = 1\ntokens = 1\ncost_micro = 0\n";
    let err =
        parse(src, std::path::Path::new(".")).expect_err("a duplicate profile key must be refused");
    let msg = format!("{err:?}");
    assert!(
        msg.contains("duplicate") && msg.to_lowercase().contains("profile"),
        "the refusal must name the problem: {msg}"
    );
}
