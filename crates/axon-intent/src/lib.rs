//! `axon-intent` — the Axon Intent→Approve Gateway (R22). The synthesis +
//! approval front-end of the Vision-OS v1 loop: it turns a human's prose
//! **intent** into a runnable, **proven-bounded** job (a `.ax` + `.axjob` +
//! `.approval` triple) that `axon-os` (R21) runs under a bound the human
//! reviewed and signed. R22 does NOT run, enforce, or sandbox anything — that is
//! R21's job. See `governance/specs/R22-intent-approve-gateway.md`.
//!
//! Core logic (`intent`/`grant_infer`/`admit`/`confidence`/`approval`/
//! `pipeline`) is pure; the LLM synthesis call is the only impure seam, behind
//! the `Synthesizer` trait. R22 reuses R21's `Grant`/`DeclaredEffects`/gate
//! verbatim — it does not redefine capability/admission types.

pub mod admit;
pub mod approval;
pub mod cli;
pub mod confidence;
pub mod emit;
pub mod grant_infer;
pub mod intent;
pub mod pipeline;
pub mod policy;
pub mod refusal;
pub mod synth;

// ── R21 (axon-os) types reused verbatim — do NOT redefine these ──────────────
pub use axon_os::gate::{admit as gate_admit, Admission, DeclaredEffects};
pub use axon_os::grant::{Budget, EffectSet, ExecPolicy, Grant, Label};

// ── R22 public API (R22 §5.1) ────────────────────────────────────────────────
pub use admit::prove_admissible;
pub use approval::{
    build_token, render_request, verify_token, ApprovalRequest, ApprovalToken, Mismatch, Risk,
};
pub use confidence::{score, Confidence};
pub use grant_infer::infer as infer_grant;
pub use intent::{parse as parse_intent, CapCeiling, Intent};
pub use pipeline::compile;
pub use pipeline::{resolve, GenMode, Resolved};
pub use refusal::{Decision, Refusal};
pub use synth::{
    MockSynth, MockSynthOrReal, ProgramDraft, RealSynthesizer, SynthErr, SynthMeta, Synthesizer,
};
