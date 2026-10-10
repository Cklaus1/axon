//! B272 — the DEC/RTR shortlist decision interface over an ALREADY-AUTHORIZED
//! eligible candidate set.
//!
//! # Two stages, and which one is authoritative
//!
//! 1. **Deterministic eligibility** is resolved by the caller BEFORE this module
//!    is reached (grants, policy, host/runtime, budget). The result — a
//!    `BTreeSet<CandidateId>` plus the `Ref` naming that view — is the ONLY set
//!    a provider ever sees, and it is authoritative.
//! 2. A **provider** may then rank and/or remove ids WITHIN that set. It can
//!    never add one.
//!
//! Any provider output that names an id outside the eligible set, is empty,
//! repeats an id, carries an out-of-range or non-monotone score, or errors is
//! an [`DecisionResult::Abstain`]. Abstention means "the caller keeps its
//! incumbent" — it is never widened into "everything eligible" and never
//! repaired by dropping the offending entries (a provider that tried to inject
//! an id has told us its output is not trustworthy as a whole).
//!
//! # What the numbers are
//!
//! [`RankedCandidate::ranking_score`] is an ORDINAL ranking score: it says
//! which candidate a provider prefers, not how likely any candidate is to be
//! correct. [`Provenance::calibration`] is `Option<Calibration>` where
//! [`Calibration`] is UNINHABITED — no provider in this build can produce a
//! calibrated correctness claim, and the type makes that unrepresentable
//! rather than merely conventional (G05-r22-decision-semantics).
//!
//! # Providers
//!
//! | id | status |
//! |---|---|
//! | `deterministic-reference` | implemented — the pinned INCUMBENT (arm A) |
//! | `deterministic-frequency` | implemented — a simple CHALLENGER (arm B) |
//! | `clm`, `jev`, `pijev`, `tiny-router` | [`DecisionResult::Unsupported`] — NOT implemented |
//!
//! The research providers are refused explicitly rather than stubbed: a stub
//! returning some ordering would be indistinguishable, in a receipt, from a
//! real one.

use axon_loop_contracts::policy::PolicySchema;
use axon_loop_contracts::{
    check_shortlist, digest_value, CandidateId, PolicyEnvelope, PolicyId, PolicyMode, Ref, Scope,
    MAX_INTEGER,
};
use std::collections::{BTreeMap, BTreeSet};

pub use axon_loop_contracts::Refusal as ContractRefusal;

/// Largest shortlist the policy schema admits.
pub const MAX_SHORTLIST: usize = 256;
/// Largest eligible set a request may carry. Bounded so the input digest and
/// every provider call are bounded too.
pub const MAX_ELIGIBLE: usize = 4096;
/// Task-feature bounds: entries, key bytes, value characters.
pub const MAX_FEATURES: usize = 32;
pub const MAX_FEATURE_KEY: usize = 64;
pub const MAX_FEATURE_VALUE: usize = 256;
/// Label for every score this module emits. Carried in provenance and in the
/// input digest so a consumer cannot mistake it for a probability.
pub const SCORE_KIND: &str = "ranking-score/ordinal";

pub const DETERMINISTIC_REFERENCE: &str = "deterministic-reference";
pub const DETERMINISTIC_FREQUENCY: &str = "deterministic-frequency";
/// Research providers named by the spec and deliberately NOT implemented here.
pub const UNSUPPORTED_PROVIDERS: &[&str] = &["clm", "jev", "pijev", "tiny-router"];

fn shape(msg: impl Into<String>) -> ContractRefusal {
    ContractRefusal::Shape(msg.into())
}

/// A provider identity, using the package id rule.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ProviderId(String);

impl ProviderId {
    pub fn new(s: impl Into<String>) -> Result<Self, ContractRefusal> {
        let s = s.into();
        // Same charset/length rule as every other package id.
        CandidateId::new(s.clone()).map_err(|_| shape(format!("ProviderId {s:?}: invalid id")))?;
        Ok(ProviderId(s))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    pub fn reference() -> Self {
        ProviderId(DETERMINISTIC_REFERENCE.into())
    }
    pub fn frequency() -> Self {
        ProviderId(DETERMINISTIC_FREQUENCY.into())
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Opaque, bounded task features. Providers may read them; the decider only
/// bounds them and binds them into the input digest.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct TaskFeatures(BTreeMap<String, String>);

impl TaskFeatures {
    pub fn new(map: BTreeMap<String, String>) -> Result<Self, ContractRefusal> {
        if map.len() > MAX_FEATURES {
            return Err(shape(format!(
                "task features: {} entries > {MAX_FEATURES}",
                map.len()
            )));
        }
        for (k, v) in &map {
            if k.len() > MAX_FEATURE_KEY || CandidateId::new(k.clone()).is_err() {
                return Err(shape(format!("task feature key {k:?} invalid")));
            }
            if v.chars().count() > MAX_FEATURE_VALUE {
                return Err(shape(format!(
                    "task feature {k:?}: value > {MAX_FEATURE_VALUE} chars"
                )));
            }
        }
        Ok(TaskFeatures(map))
    }
    pub fn as_map(&self) -> &BTreeMap<String, String> {
        &self.0
    }
}

/// One decision request. Fields are private so a constructed request always
/// satisfies its bounds: non-empty eligible set (an empty one BLOCKS — there is
/// no "best available" fallback), bounded features, a limit in 1..=256.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DecisionRequest {
    scope: Scope,
    eligible: BTreeSet<CandidateId>,
    candidate_set_ref: Ref,
    controls_ref: Ref,
    features: TaskFeatures,
    provider: ProviderId,
    limit: usize,
}

impl DecisionRequest {
    pub fn new(
        scope: Scope,
        eligible: BTreeSet<CandidateId>,
        candidate_set_ref: Ref,
        controls_ref: Ref,
        features: TaskFeatures,
        provider: ProviderId,
    ) -> Result<Self, ContractRefusal> {
        if eligible.is_empty() {
            return Err(ContractRefusal::Semantic(
                "empty eligible set blocks: no provider is consulted".into(),
            ));
        }
        if eligible.len() > MAX_ELIGIBLE {
            return Err(shape(format!(
                "eligible set: {} > {MAX_ELIGIBLE}",
                eligible.len()
            )));
        }
        Ok(DecisionRequest {
            scope,
            eligible,
            candidate_set_ref,
            controls_ref,
            features,
            provider,
            limit: MAX_SHORTLIST,
        })
    }

    /// Cap the shortlist length (1..=256). Truncation after validation is
    /// subtractive, so it cannot widen anything.
    pub fn with_limit(mut self, limit: usize) -> Result<Self, ContractRefusal> {
        if !(1..=MAX_SHORTLIST).contains(&limit) {
            return Err(shape(format!("limit {limit} outside 1..={MAX_SHORTLIST}")));
        }
        self.limit = limit;
        Ok(self)
    }

    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn eligible(&self) -> &BTreeSet<CandidateId> {
        &self.eligible
    }
    pub fn candidate_set_ref(&self) -> &Ref {
        &self.candidate_set_ref
    }
    pub fn controls_ref(&self) -> &Ref {
        &self.controls_ref
    }
    pub fn features(&self) -> &TaskFeatures {
        &self.features
    }
    pub fn provider(&self) -> &ProviderId {
        &self.provider
    }
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// `cl22:` digest of everything the decision depends on, including the
    /// provider id and the score kind. Deterministic: the eligible set is a
    /// `BTreeSet` and features a `BTreeMap`, so order is fixed.
    pub fn input_digest(&self) -> Result<Ref, ContractRefusal> {
        let v = serde_json::json!({
            "schema": "axon-reflex.shortlist-request/1",
            "scope": self.scope,
            "eligible": self.eligible,
            "candidate_set_ref": self.candidate_set_ref,
            "controls_ref": self.controls_ref,
            "features": self.features.0,
            "provider": self.provider.0,
            "limit": self.limit,
            "score_kind": SCORE_KIND,
        });
        digest_value(&v)
    }
}

/// What a provider is shown: the eligible ids and the task features. Nothing
/// else — no refs, no controls, nothing outside the authorized view.
pub struct ProviderView<'a> {
    pub scope: &'a Scope,
    pub eligible: &'a BTreeSet<CandidateId>,
    pub features: &'a TaskFeatures,
}

/// A provider failure. Transport/internal errors are NOT negative answers:
/// the decider turns every one into an abstention.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ProviderError(pub String);

/// A ranking provider. Returns candidates best-first with ORDINAL ranking
/// scores (non-increasing along the list). It may omit eligible ids (remove)
/// but any id outside `view.eligible` voids the whole output.
pub trait ShortlistProvider {
    fn id(&self) -> ProviderId;
    /// Pinned version string; part of provenance.
    fn version(&self) -> String;
    /// Refs of the discovery evidence the provider consulted (≤256, unique).
    fn evidence_refs(&self) -> Vec<Ref>;
    fn rank(&self, view: &ProviderView<'_>) -> Result<Vec<(CandidateId, i64)>, ProviderError>;
}

/// Uninhabited: no provider in this build produces calibrated correctness, so
/// `Provenance::calibration` is always `None` by construction.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Calibration {}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Provenance {
    pub provider_id: ProviderId,
    pub provider_version: String,
    /// `cl22:` digest of the [`DecisionRequest`].
    pub input_digest: Ref,
    /// Always [`SCORE_KIND`].
    pub score_kind: &'static str,
    pub calibration: Option<Calibration>,
    pub evidence_refs: Vec<Ref>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RankedCandidate {
    pub candidate: CandidateId,
    /// Ordinal ranking score. NOT a probability, NOT calibrated confidence.
    pub ranking_score: i64,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum AbstainReason {
    /// The request named a different provider than the one supplied.
    ProviderMismatch {
        requested: String,
        supplied: String,
    },
    ProviderError(String),
    EmptyOutput,
    /// The provider named an id outside the authorized eligible set.
    NotEligible(CandidateId),
    Duplicate(CandidateId),
    ScoreOutOfRange(i64),
    /// Scores rose along a best-first list: order and score disagree.
    NonMonotoneScores,
    /// Evidence refs duplicated or over the schema bound.
    InvalidEvidence,
    /// The input could not be digested (should not happen for a valid request).
    Digest(String),
}

/// The outcome. Only `Shortlist` carries candidates; on `Abstain` and
/// `Unsupported` the caller keeps its incumbent.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum DecisionResult {
    Shortlist {
        provenance: Provenance,
        ranked: Vec<RankedCandidate>,
    },
    Abstain {
        reason: AbstainReason,
        provider_id: ProviderId,
    },
    /// The named provider is not implemented in this build. Distinct from
    /// abstention: it is not a judgment about the candidates at all.
    Unsupported { provider_id: ProviderId },
}

impl DecisionResult {
    pub fn is_shortlist(&self) -> bool {
        matches!(self, DecisionResult::Shortlist { .. })
    }
    pub fn ids(&self) -> Option<Vec<CandidateId>> {
        match self {
            DecisionResult::Shortlist { ranked, .. } => {
                Some(ranked.iter().map(|r| r.candidate.clone()).collect())
            }
            _ => None,
        }
    }
}

/// Run `provider` over the request's eligible set and validate its output.
/// Eligibility is authoritative: the result is ⊆ `request.eligible()` or it is
/// not a shortlist.
pub fn decide(request: &DecisionRequest, provider: &dyn ShortlistProvider) -> DecisionResult {
    let pid = provider.id();
    let abstain = |reason| DecisionResult::Abstain {
        reason,
        provider_id: pid.clone(),
    };
    if &pid != request.provider() {
        return abstain(AbstainReason::ProviderMismatch {
            requested: request.provider().0.clone(),
            supplied: pid.0.clone(),
        });
    }
    let input_digest = match request.input_digest() {
        Ok(d) => d,
        Err(e) => return abstain(AbstainReason::Digest(e.to_string())),
    };
    let evidence_refs = provider.evidence_refs();
    let unique: BTreeSet<&Ref> = evidence_refs.iter().collect();
    if evidence_refs.len() > MAX_SHORTLIST || unique.len() != evidence_refs.len() {
        return abstain(AbstainReason::InvalidEvidence);
    }
    let view = ProviderView {
        scope: request.scope(),
        eligible: request.eligible(),
        features: request.features(),
    };
    let out = match provider.rank(&view) {
        Ok(o) => o,
        Err(e) => return abstain(AbstainReason::ProviderError(e.0)),
    };
    if out.is_empty() {
        return abstain(AbstainReason::EmptyOutput);
    }
    let mut seen = BTreeSet::new();
    let mut prev: Option<i64> = None;
    for (c, s) in &out {
        if !request.eligible().contains(c) {
            return abstain(AbstainReason::NotEligible(c.clone()));
        }
        if !seen.insert(c) {
            return abstain(AbstainReason::Duplicate(c.clone()));
        }
        if s.unsigned_abs() > MAX_INTEGER {
            return abstain(AbstainReason::ScoreOutOfRange(*s));
        }
        if prev.is_some_and(|p| *s > p) {
            return abstain(AbstainReason::NonMonotoneScores);
        }
        prev = Some(*s);
    }
    let ranked = out
        .into_iter()
        .take(request.limit())
        .map(|(candidate, ranking_score)| RankedCandidate {
            candidate,
            ranking_score,
        })
        .collect();
    DecisionResult::Shortlist {
        provenance: Provenance {
            provider_id: pid,
            provider_version: provider.version(),
            input_digest,
            score_kind: SCORE_KIND,
            calibration: None,
            evidence_refs,
        },
        ranked,
    }
}

/// Run the challenger; if it does not produce a shortlist, run the incumbent
/// on the same eligible set. Returns `(result_used, challenger_result)` so the
/// fallback is recorded rather than hidden. The incumbent's request is the
/// challenger's with only the provider id changed.
pub fn decide_with_fallback(
    request: &DecisionRequest,
    challenger: &dyn ShortlistProvider,
    incumbent: &dyn ShortlistProvider,
) -> (DecisionResult, DecisionResult) {
    let first = decide(request, challenger);
    if first.is_shortlist() {
        return (first.clone(), first);
    }
    let mut inc_req = request.clone();
    inc_req.provider = incumbent.id();
    (decide(&inc_req, incumbent), first)
}

/// Resolve a provider by id. The two deterministic providers are built here;
/// every research provider and every unknown id is `None` — which
/// [`decide_builtin`] reports as [`DecisionResult::Unsupported`], never as a
/// silent fallback to some other provider.
pub fn builtin_provider(
    id: &ProviderId,
    evidence: &[DiscoveryEvidence],
) -> Option<Box<dyn ShortlistProvider>> {
    match id.as_str() {
        DETERMINISTIC_REFERENCE => Some(Box::new(DeterministicReference)),
        DETERMINISTIC_FREQUENCY => Some(Box::new(DeterministicFrequency::new(evidence.to_vec()))),
        _ => None,
    }
}

/// [`builtin_provider`] then [`decide`].
pub fn decide_builtin(request: &DecisionRequest, evidence: &[DiscoveryEvidence]) -> DecisionResult {
    match builtin_provider(request.provider(), evidence) {
        Some(p) => decide(request, p.as_ref()),
        None => DecisionResult::Unsupported {
            provider_id: request.provider().clone(),
        },
    }
}

/// INCUMBENT (arm A): a pinned static ordering — ascending semantic id, never
/// input order. Keeps every eligible id; scores are descending rank positions.
/// Changing the ordering rule MUST bump [`DeterministicReference::VERSION`].
pub struct DeterministicReference;

impl DeterministicReference {
    pub const VERSION: &'static str = "deterministic-reference/1:semantic-id-ascending";
}

impl ShortlistProvider for DeterministicReference {
    fn id(&self) -> ProviderId {
        ProviderId::reference()
    }
    fn version(&self) -> String {
        Self::VERSION.into()
    }
    fn evidence_refs(&self) -> Vec<Ref> {
        Vec::new()
    }
    fn rank(&self, view: &ProviderView<'_>) -> Result<Vec<(CandidateId, i64)>, ProviderError> {
        let n = view.eligible.len() as i64;
        Ok(view
            .eligible
            .iter()
            .enumerate()
            .map(|(i, c)| (c.clone(), n - i as i64))
            .collect())
    }
}

/// Historical usage counts from one piece of discovery evidence.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct DiscoveryEvidence {
    pub evidence_ref: Ref,
    pub usage_counts: BTreeMap<CandidateId, u64>,
}

/// CHALLENGER (arm B): rank eligible ids by summed historical usage count
/// (descending), ties broken by ascending semantic id. Ids with zero usage are
/// kept, after all used ones. Counts for NON-eligible ids are ignored — the
/// provider iterates the eligible set, so evidence cannot inject an id.
pub struct DeterministicFrequency {
    evidence: Vec<DiscoveryEvidence>,
}

impl DeterministicFrequency {
    pub const VERSION: &'static str = "deterministic-frequency/1:count-desc,semantic-id-asc";

    pub fn new(mut evidence: Vec<DiscoveryEvidence>) -> Self {
        // Evidence order must not change the ranking or the provenance bytes.
        evidence.sort_by(|a, b| a.evidence_ref.cmp(&b.evidence_ref));
        DeterministicFrequency { evidence }
    }
}

impl ShortlistProvider for DeterministicFrequency {
    fn id(&self) -> ProviderId {
        ProviderId::frequency()
    }
    fn version(&self) -> String {
        Self::VERSION.into()
    }
    fn evidence_refs(&self) -> Vec<Ref> {
        self.evidence
            .iter()
            .map(|e| e.evidence_ref.clone())
            .collect()
    }
    fn rank(&self, view: &ProviderView<'_>) -> Result<Vec<(CandidateId, i64)>, ProviderError> {
        let mut scored: Vec<(CandidateId, i64)> = view
            .eligible
            .iter()
            .map(|c| {
                let total = self
                    .evidence
                    .iter()
                    .filter_map(|e| e.usage_counts.get(c))
                    .fold(0u64, |a, n| a.saturating_add(*n))
                    .min(MAX_INTEGER);
                (c.clone(), total as i64)
            })
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        Ok(scored)
    }
}

/// Build a `shortlist_only` [`PolicyEnvelope`] from a shortlist result and
/// verify it with [`check_shortlist`] before returning it. Refuses an
/// abstention/unsupported result — a policy with nothing to offer is not
/// issued (the caller pauses instead).
pub fn to_policy_envelope(
    result: &DecisionResult,
    request: &DecisionRequest,
    policy_id: PolicyId,
    parent_policy_ref: Ref,
) -> Result<PolicyEnvelope, ContractRefusal> {
    let (provenance, ranked) = match result {
        DecisionResult::Shortlist { provenance, ranked } => (provenance, ranked),
        _ => {
            return Err(ContractRefusal::Semantic(
                "no shortlist to issue: abstain/unsupported keeps the incumbent".into(),
            ))
        }
    };
    if &provenance.provider_id != request.provider() {
        return Err(ContractRefusal::Semantic(
            "result provider differs from request provider".into(),
        ));
    }
    if provenance.input_digest != request.input_digest()? {
        return Err(ContractRefusal::Semantic(
            "result was not decided on this request".into(),
        ));
    }
    let env = PolicyEnvelope {
        schema: PolicySchema,
        policy_id,
        parent_policy_ref,
        scope: request.scope().clone(),
        mode: PolicyMode::ShortlistOnly,
        candidate_set_ref: request.candidate_set_ref().clone(),
        controls_ref: request.controls_ref().clone(),
        shortlist: ranked.iter().map(|r| r.candidate.clone()).collect(),
        discovery_evidence_refs: provenance.evidence_refs.clone(),
        authority_expansion: false,
    };
    check_shortlist(
        &env,
        request.eligible(),
        request.candidate_set_ref(),
        request.controls_ref(),
        request.scope(),
    )?;
    Ok(env)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_loop_contracts::{TaskFamily, TenantId};

    fn r(c: char) -> Ref {
        Ref::new(format!("cl22:{}", c.to_string().repeat(64))).unwrap()
    }
    fn cid(s: &str) -> CandidateId {
        CandidateId::new(s).unwrap()
    }
    fn scope() -> Scope {
        Scope {
            tenant_id: TenantId::new("t1").unwrap(),
            task_family: TaskFamily::new("axon-repair").unwrap(),
        }
    }
    fn eligible() -> BTreeSet<CandidateId> {
        ["tool.grep", "tool.read", "skill.fmt", "tool.test"]
            .into_iter()
            .map(cid)
            .collect()
    }
    fn features() -> TaskFeatures {
        let mut m = BTreeMap::new();
        m.insert("lang".into(), "axon".into());
        TaskFeatures::new(m).unwrap()
    }
    fn req(p: ProviderId) -> DecisionRequest {
        DecisionRequest::new(scope(), eligible(), r('a'), r('b'), features(), p).unwrap()
    }
    fn evidence() -> Vec<DiscoveryEvidence> {
        let mut u1 = BTreeMap::new();
        u1.insert(cid("tool.test"), 5);
        u1.insert(cid("tool.read"), 2);
        // A count for an id that is NOT eligible: must never surface.
        u1.insert(cid("tool.shell"), 1000);
        let mut u2 = BTreeMap::new();
        u2.insert(cid("tool.read"), 3);
        u2.insert(cid("skill.fmt"), 1);
        vec![
            DiscoveryEvidence {
                evidence_ref: r('d'),
                usage_counts: u2,
            },
            DiscoveryEvidence {
                evidence_ref: r('c'),
                usage_counts: u1,
            },
        ]
    }

    /// A provider returning whatever the test scripts, for adversarial cases.
    struct Scripted {
        id: ProviderId,
        out: Result<Vec<(CandidateId, i64)>, ProviderError>,
        evidence: Vec<Ref>,
    }
    impl ShortlistProvider for Scripted {
        fn id(&self) -> ProviderId {
            self.id.clone()
        }
        fn version(&self) -> String {
            "scripted/0".into()
        }
        fn evidence_refs(&self) -> Vec<Ref> {
            self.evidence.clone()
        }
        fn rank(&self, _: &ProviderView<'_>) -> Result<Vec<(CandidateId, i64)>, ProviderError> {
            self.out.clone()
        }
    }
    fn scripted(out: Result<Vec<(CandidateId, i64)>, ProviderError>) -> Scripted {
        Scripted {
            id: ProviderId::new("adversary").unwrap(),
            out,
            evidence: vec![],
        }
    }
    fn abstain_reason(d: DecisionResult) -> AbstainReason {
        match d {
            DecisionResult::Abstain { reason, .. } => reason,
            other => panic!("expected abstain, got {other:?}"),
        }
    }

    #[test]
    fn reference_is_semantic_id_order_and_keeps_all() {
        let d = decide_builtin(&req(ProviderId::reference()), &[]);
        let ids: Vec<String> = d.ids().unwrap().iter().map(|c| c.to_string()).collect();
        assert_eq!(ids, ["skill.fmt", "tool.grep", "tool.read", "tool.test"]);
        let DecisionResult::Shortlist { provenance, ranked } = d else {
            unreachable!()
        };
        assert_eq!(provenance.provider_version, DeterministicReference::VERSION);
        assert_eq!(provenance.score_kind, SCORE_KIND);
        assert!(provenance.calibration.is_none());
        assert!(provenance.input_digest.as_str().starts_with("cl22:"));
        assert_eq!(ranked[0].ranking_score, 4);
    }

    #[test]
    fn frequency_ranks_by_count_ignores_ineligible_and_ties_by_id() {
        let d = decide_builtin(&req(ProviderId::frequency()), &evidence());
        let ids: Vec<String> = d.ids().unwrap().iter().map(|c| c.to_string()).collect();
        // read=5, test=5 (tie → id asc), fmt=1, grep=0. tool.shell never appears.
        assert_eq!(ids, ["tool.read", "tool.test", "skill.fmt", "tool.grep"]);
        let DecisionResult::Shortlist { provenance, .. } = d else {
            unreachable!()
        };
        // Evidence refs sorted, so supplied order does not matter.
        assert_eq!(provenance.evidence_refs, vec![r('c'), r('d')]);
    }

    #[test]
    fn deterministic_across_runs_and_evidence_order() {
        let a = decide_builtin(&req(ProviderId::frequency()), &evidence());
        let mut rev = evidence();
        rev.reverse();
        for _ in 0..5 {
            assert_eq!(a, decide_builtin(&req(ProviderId::frequency()), &rev));
        }
        let x = decide_builtin(&req(ProviderId::reference()), &[]);
        assert_eq!(x, decide_builtin(&req(ProviderId::reference()), &[]));
        // The digest binds the provider id: two arms never share an input digest.
        assert_ne!(
            req(ProviderId::reference()).input_digest().unwrap(),
            req(ProviderId::frequency()).input_digest().unwrap()
        );
    }

    #[test]
    fn adversarial_provider_cannot_inject_ids() {
        let p = scripted(Ok(vec![(cid("tool.read"), 9), (cid("tool.shell"), 8)]));
        let d = decide(&req(p.id()), &p);
        assert_eq!(
            abstain_reason(d),
            AbstainReason::NotEligible(cid("tool.shell"))
        );
        // Injection is not "repaired" by dropping the bad entry.
    }

    #[test]
    fn abstention_paths() {
        let cases: Vec<(Scripted, AbstainReason)> = vec![
            (scripted(Ok(vec![])), AbstainReason::EmptyOutput),
            (
                scripted(Ok(vec![(cid("tool.read"), 2), (cid("tool.read"), 1)])),
                AbstainReason::Duplicate(cid("tool.read")),
            ),
            (
                scripted(Err(ProviderError("timeout".into()))),
                AbstainReason::ProviderError("timeout".into()),
            ),
            (
                scripted(Ok(vec![(cid("tool.read"), 1), (cid("tool.test"), 2)])),
                AbstainReason::NonMonotoneScores,
            ),
            (
                scripted(Ok(vec![(cid("tool.read"), i64::MAX)])),
                AbstainReason::ScoreOutOfRange(i64::MAX),
            ),
            (
                Scripted {
                    evidence: vec![r('c'), r('c')],
                    ..scripted(Ok(vec![(cid("tool.read"), 1)]))
                },
                AbstainReason::InvalidEvidence,
            ),
        ];
        for (p, want) in cases {
            assert_eq!(abstain_reason(decide(&req(p.id()), &p)), want);
        }
        // Supplying a provider other than the one requested abstains.
        let d = decide(&req(ProviderId::frequency()), &DeterministicReference);
        assert!(matches!(
            abstain_reason(d),
            AbstainReason::ProviderMismatch { .. }
        ));
    }

    #[test]
    fn empty_eligible_set_blocks_before_any_provider() {
        assert!(DecisionRequest::new(
            scope(),
            BTreeSet::new(),
            r('a'),
            r('b'),
            features(),
            ProviderId::reference()
        )
        .is_err());
    }

    #[test]
    fn features_and_limit_are_bounded() {
        let big: BTreeMap<String, String> = (0..=MAX_FEATURES)
            .map(|i| (format!("k{i}"), "v".into()))
            .collect();
        assert!(TaskFeatures::new(big).is_err());
        let mut long = BTreeMap::new();
        long.insert("k".into(), "x".repeat(MAX_FEATURE_VALUE + 1));
        assert!(TaskFeatures::new(long).is_err());
        let mut badkey = BTreeMap::new();
        badkey.insert("bad key".into(), "v".into());
        assert!(TaskFeatures::new(badkey).is_err());
        assert!(req(ProviderId::reference()).with_limit(0).is_err());
        assert!(req(ProviderId::reference()).with_limit(257).is_err());
        let d = decide_builtin(
            &req(ProviderId::frequency()).with_limit(2).unwrap(),
            &evidence(),
        );
        assert_eq!(d.ids().unwrap().len(), 2);
    }

    #[test]
    fn research_providers_are_unsupported_not_faked() {
        for name in UNSUPPORTED_PROVIDERS {
            let id = ProviderId::new(*name).unwrap();
            let d = decide_builtin(&req(id.clone()), &evidence());
            assert_eq!(d, DecisionResult::Unsupported { provider_id: id });
            assert!(d.ids().is_none());
        }
    }

    #[test]
    fn fallback_to_incumbent_is_recorded() {
        let bad = scripted(Ok(vec![(cid("tool.shell"), 1)]));
        let (used, challenger) =
            decide_with_fallback(&req(bad.id()), &bad, &DeterministicReference);
        assert!(matches!(challenger, DecisionResult::Abstain { .. }));
        assert_eq!(used, decide_builtin(&req(ProviderId::reference()), &[]));

        let good = DeterministicFrequency::new(evidence());
        let (used, challenger) = decide_with_fallback(
            &req(ProviderId::frequency()),
            &good,
            &DeterministicReference,
        );
        assert_eq!(used, challenger);
        assert!(used.is_shortlist());
    }

    #[test]
    fn envelope_passes_check_shortlist_for_both_arms() {
        for (p, ev) in [
            (ProviderId::reference(), vec![]),
            (ProviderId::frequency(), evidence()),
        ] {
            let rq = req(p);
            let d = decide_builtin(&rq, &ev);
            let env = to_policy_envelope(&d, &rq, PolicyId::new("pol-1").unwrap(), r('e')).unwrap();
            let got = check_shortlist(&env, &eligible(), &r('a'), &r('b'), &scope()).unwrap();
            assert_eq!(got.to_vec(), d.ids().unwrap());
            assert!(!env.authority_expansion);
            // Round-trips through strict parse and digests.
            let json =
                String::from_utf8(axon_loop_contracts::canonical_json(&env).unwrap()).unwrap();
            let back: PolicyEnvelope = axon_loop_contracts::parse(&json).unwrap();
            assert_eq!(back, env);
            assert!(env.version().is_ok());
        }
    }

    #[test]
    fn envelope_refuses_abstain_and_foreign_results() {
        let rq = req(ProviderId::reference());
        let pol = || PolicyId::new("pol-1").unwrap();
        let ab = DecisionResult::Abstain {
            reason: AbstainReason::EmptyOutput,
            provider_id: ProviderId::reference(),
        };
        assert!(to_policy_envelope(&ab, &rq, pol(), r('e')).is_err());
        let un = DecisionResult::Unsupported {
            provider_id: ProviderId::new("clm").unwrap(),
        };
        assert!(to_policy_envelope(&un, &rq, pol(), r('e')).is_err());
        // A shortlist decided on a DIFFERENT request (other features) is refused.
        let mut m = BTreeMap::new();
        m.insert("lang".into(), "rust".into());
        let other = DecisionRequest::new(
            scope(),
            eligible(),
            r('a'),
            r('b'),
            TaskFeatures::new(m).unwrap(),
            ProviderId::reference(),
        )
        .unwrap();
        let d = decide_builtin(&other, &[]);
        assert!(to_policy_envelope(&d, &rq, pol(), r('e')).is_err());
    }
}
