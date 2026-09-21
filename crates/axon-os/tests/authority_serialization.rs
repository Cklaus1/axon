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
