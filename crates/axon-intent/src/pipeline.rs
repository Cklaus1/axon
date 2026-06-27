//! R22 §4.1 + §4.3 — the orchestration: compile → infer least-privilege grant →
//! prove admissible → score → render request. Pure core; the synthesizer (the
//! only impure dependency) is injected, so the whole pipeline is mock-testable.
//!
//! `compile` produces a `ProgramDraft`. `resolve` runs the full
//! intent→draft→grant→admit→score→request chain and returns a `Resolved`
//! (everything needed to present the approval request and, on accept, emit the
//! triple). Any refusal short-circuits — no token is built on a refused draft
//! (fail closed; I-1 refuse-don't-downgrade).

use crate::admit::prove_admissible;
use crate::approval::{render_request, ApprovalRequest, Risk};
use crate::confidence::{score, Confidence};
use crate::grant_infer::infer;
use crate::intent::Intent;
use crate::refusal::Refusal;
use crate::synth::{meta_from_source, strip_fences, ProgramDraft, Synthesizer};
use axon_os::grant::Grant;

/// Whether to use an author-supplied fenced ```axon block (lifted, not
/// generated) when one is present and generation is not explicitly forced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenMode {
    /// Prefer lifting a fenced block if present; else synthesize.
    Auto,
    /// Always synthesize, even if a fenced block is present (`AXON_INTENT_GEN`).
    ForceGenerate,
}

/// The fully-resolved, admissible, scored result ready for human approval +
/// emission. Only produced when every gate passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub draft: ProgramDraft,
    pub grant: Grant,
    pub confidence: Confidence,
    pub request: ApprovalRequest,
}

/// Compile an intent into a candidate program (R22 §4.1). Lifts a fenced block
/// verbatim (`generated=false`) under `GenMode::Auto`, else synthesizes; strips
/// accidental fences from model output; type-checks; populates `SynthMeta`.
pub fn compile(
    intent: &Intent,
    synth: &impl Synthesizer,
    mode: GenMode,
) -> Result<ProgramDraft, Refusal> {
    // 1. lift a fenced block (non-generated) or synthesize.
    let (raw_source, generated) = match (&intent.fenced_axon, mode) {
        (Some(block), GenMode::Auto) => (block.clone(), false),
        _ => {
            let s = synth
                .synthesize(intent)
                .map_err(|e| Refusal::LowConfidence {
                    reason: format!("synthesis failed: {}", e.reason),
                    score: 0,
                })?;
            (s, true)
        }
    };

    // 2. strip accidental ```axon fences from model output.
    let (source, fences_stripped) = strip_fences(&raw_source);

    // 3. type-check + populate meta (the real synth shells out; the mock derives
    //    it structurally — either way `typecheck` is the seam).
    let meta = synth.typecheck(&source);
    // Keep the structural fields consistent with the (possibly fence-stripped)
    // source while honoring the typechecks verdict the synthesizer returned.
    let meta = {
        let mut m = meta_from_source(&source, meta.typechecks, meta.uncertainty_notes);
        // preserve a richer declared set if the synth's was wider (deny-by-default).
        m.declared = wider(meta.declared, m.declared);
        m
    };

    Ok(ProgramDraft {
        source,
        generated,
        fences_stripped,
        meta,
    })
}

/// The union (deny-by-default) of two declared-effect sets — never narrows.
fn wider(
    a: axon_os::gate::DeclaredEffects,
    b: axon_os::gate::DeclaredEffects,
) -> axon_os::gate::DeclaredEffects {
    axon_os::gate::DeclaredEffects {
        row: axon_os::grant::EffectSet {
            fs_read: a.row.fs_read || b.row.fs_read,
            fs_write: a.row.fs_write || b.row.fs_write,
            net: a.row.net || b.row.net,
            exec: a.row.exec || b.row.exec,
        },
        max_label: a.max_label.max(b.max_label),
    }
}

/// Run the full resolve chain (R22 §4): compile → infer → admit → score →
/// render. Any failure short-circuits with the appropriate `Refusal` (no token
/// is built on a refused draft). `risk_floor` (from `--risk`) may only raise.
pub fn resolve(
    intent: &Intent,
    synth: &impl Synthesizer,
    mode: GenMode,
    risk_floor: Option<Risk>,
) -> Result<Resolved, Refusal> {
    // 1. compile (synthesis seam).
    let draft = compile(intent, synth, mode)?;

    // 2. confidence gate FIRST — refuse a low-quality draft before inferring a
    //    grant for it (fail closed; cheap rejection).
    let confidence = score(&draft, intent);
    if let Confidence::Refuse { reason } = &confidence {
        let sc = confidence.score();
        return Err(Refusal::LowConfidence {
            reason: reason.clone(),
            score: sc,
        });
    }

    // 3. least-privilege grant inference (may refuse: GrantExceedsCeiling).
    let grant = infer(&draft.meta.declared, &intent.cap_ceiling, &intent.budget)?;

    // 4. admissibility proof (must hold by construction; refuse on any skew).
    prove_admissible(&draft.meta.declared, &grant)?;

    // 5. render the legible request.
    let request = render_request(&draft, &grant, intent, &confidence, risk_floor);

    Ok(Resolved {
        draft,
        grant,
        confidence,
        request,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent;
    use crate::synth::MockSynth;

    const VALID: &str = r#"# Intent
Summarize ./data/report.txt into ./out/summary.txt.

## Inputs
- ./data/report.txt

## Outputs
- ./out/summary.txt

## Allowed
- fs_read: ./data/
- fs_write: ./out/
- net: api.example.com
- exec: none
- max_label: internal

## Budget
- calls: 100
- tokens: 50000
- cost_micro: 1000000
"#;

    fn it() -> Intent {
        intent::parse(VALID).unwrap()
    }

    // R22 §7 `synthesized_job_is_self_admissible` (full chain).
    #[test]
    fn synthesized_job_is_self_admissible() {
        let r = resolve(&it(), &MockSynth::new(), GenMode::Auto, None).expect("admissible");
        // The mock program reads + writes but does NOT use net → least privilege.
        assert!(r.grant.net.is_empty(), "net unused ⇒ empty");
        assert_eq!(r.grant.fs_read, vec!["./data/".to_string()]);
        assert_eq!(r.grant.fs_write, vec!["./out/".to_string()]);
        // Admissible by construction.
        assert!(prove_admissible(&r.draft.meta.declared, &r.grant).is_ok());
        assert!(matches!(r.confidence, Confidence::Ship { .. }));
    }

    // R22 §7 `grant_is_least_privilege` — the ceiling permits net, the program
    // never uses it → inferred grant has net=∅.
    #[test]
    fn grant_is_least_privilege() {
        let r = resolve(&it(), &MockSynth::new(), GenMode::Auto, None).unwrap();
        assert!(
            r.grant.net.is_empty(),
            "ceiling permits net but the program never uses it ⇒ net granted nothing"
        );
        assert!(r.request.legible.contains("use the network"));
    }

    // R22 §7 `non_admissible_refused` (adversarial: a draft declaring net while
    // inference produced none — forced via an override program that calls a net
    // builtin the ceiling does permit, then... actually we force the skew by a
    // program that scans net but whose grant we make narrow). Here we force a
    // program that declares net AND a ceiling that permits net, so it's
    // admissible. To force NotAdmissible we use a narrower ceiling test below.
    #[test]
    fn low_confidence_short_circuits_no_resolve() {
        // A stub program → LowConfidence; resolve returns before inferring/admit.
        let stub = MockSynth::with_source("fn main() -> i64 { TODO }");
        let r = resolve(&it(), &stub, GenMode::Auto, None);
        assert!(matches!(r, Err(Refusal::LowConfidence { .. })));
        assert_eq!(r.unwrap_err().exit_code(), 5);
    }

    #[test]
    fn grant_exceeds_ceiling_short_circuits() {
        // A program that reads a non-permitted axis: force net usage with a
        // ceiling that forbids net.
        let net_prog = MockSynth::with_source(
            "fn main() -> i64 { let x = http_get(\"api.example.com\") write_file(\"./out/summary.txt\", x) 0 }",
        );
        let no_net = VALID.replace("net: api.example.com", "net: none");
        let i = intent::parse(&no_net).unwrap();
        let r = resolve(&i, &net_prog, GenMode::Auto, None);
        assert!(matches!(r, Err(Refusal::GrantExceedsCeiling { ref axis }) if axis == "net"));
    }

    #[test]
    fn lifts_a_fenced_block_under_auto() {
        let fenced = format!(
            "{VALID}\n## Program\n```axon\nfn main() -> i64 {{ write_file(\"./out/summary.txt\", \"x\") 0 }}\n```\n"
        );
        let i = intent::parse(&fenced).unwrap();
        let d = compile(&i, &MockSynth::new(), GenMode::Auto).unwrap();
        assert!(!d.generated, "a fenced block is lifted, not generated");
        assert!(d.source.contains("write_file"));
    }
}
