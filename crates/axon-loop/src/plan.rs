//! B274 — the experiment register and frozen plan.
//!
//! A plan is the package's `axon.closed-loop.pilot/1` document, validated
//! against the byte-copied package schema (`schemas/closed-loop-pilot.schema.json`)
//! BEFORE typed serde. It is stored content-addressed (`plans/<hex>.json`)
//! and registered in the store ledger under its experiment id.
//!
//! `freeze` is a LEDGER event, so it is permanent: deleting any file does not
//! undo it (G6), and a frozen experiment id can never be re-registered. A
//! freeze binds the experiment to exactly one (incumbent, candidate) pair and
//! a candidate can be frozen in at most ONE experiment, so a plan cannot be
//! shopped for after outcomes exist (G7). It records the scope's authority
//! epoch and its own ledger sequence; evaluations are only accepted AFTER it.
//!
//! Freeze refuses (exit 7) a plan with any unset operator field — the package
//! template leaves them `null` ON PURPOSE and nothing here fills them — and a
//! plan whose rules the admitter cannot EXECUTE ([`crate::rules`]); a frozen
//! rule that is never read would be a rule in name only (I13, I14). The
//! candidate must be an EVO proposal from this incumbent (O1/O2).

use crate::error::{refused, LoopError, Result};
use crate::ledger::{Event, Tx};
use crate::store::{strict_record, Store};
use axon_loop_contracts::{AuthorityEpoch, PolicyEnvelope, Ref, Refusal, Scope, TaskId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PILOT_SCHEMA_TEXT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/schemas/closed-loop-pilot.schema.json"
));

crate::record_tag!(PilotSchema, "axon.closed-loop.pilot/1");

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutableSurface {
    EligibleToolSkillShortlist,
}

/// `closed-loop-pilot/1`, field for field. Every nullable field is REQUIRED
/// (present, possibly `null`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PilotPlan {
    pub schema: PilotSchema,
    pub experiment_id: String,
    pub scope: Scope,
    pub mutable_surface: MutableSurface,
    pub runtime_ready: bool,
    pub deployment_enabled: bool,
    pub operator_approved: bool,
    pub live_evidence: Vec<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub task_manifest_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub repository_split_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub discovery_manifest_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub confirmation_manifest_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub reporting_manifest_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub controls_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub incumbent_policy_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub candidate_policy_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub analysis_method_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub data_use_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub rollback_policy_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub approval_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub independent_units: Option<u64>,
    #[serde(deserialize_with = "crate::nullable")]
    pub repetitions: Option<u64>,
    #[serde(deserialize_with = "crate::nullable")]
    pub candidate_budget: Option<u64>,
    #[serde(deserialize_with = "crate::nullable")]
    pub independent_unit: Option<String>,
    #[serde(deserialize_with = "crate::nullable")]
    pub quality_margin: Option<String>,
    #[serde(deserialize_with = "crate::nullable")]
    pub economic_threshold: Option<String>,
    #[serde(deserialize_with = "crate::nullable")]
    pub uncertainty_rule: Option<String>,
    #[serde(deserialize_with = "crate::nullable")]
    pub missing_data_rule: Option<String>,
    #[serde(deserialize_with = "crate::nullable")]
    pub multiplicity_rule: Option<String>,
    #[serde(deserialize_with = "crate::nullable")]
    pub order_rule: Option<String>,
    #[serde(deserialize_with = "crate::nullable")]
    pub cache_rule: Option<String>,
    #[serde(deserialize_with = "crate::nullable")]
    pub budget_rule: Option<String>,
}

impl PilotPlan {
    /// Strict parse: profile JSON rules → package schema → typed serde →
    /// canonical round-trip.
    pub fn parse(text: &str) -> Result<PilotPlan> {
        let v = axon_loop_contracts::parse_value(text)?;
        Self::from_value(&v)
    }

    pub fn from_value(v: &Value) -> Result<PilotPlan> {
        let schema = serde_json::from_str::<Value>(PILOT_SCHEMA_TEXT)
            .map_err(|e| LoopError::Io(format!("pilot schema unreadable: {e}")))?;
        axon_loop_contracts::schema::validate_against(&schema, v)?;
        let text = serde_json::to_string(v).map_err(|e| LoopError::Io(e.to_string()))?;
        let p: PilotPlan = strict_record(&text)?;
        TaskId::new(p.experiment_id.as_str())
            .map_err(|_| LoopError::Malformed(Refusal::Shape("experiment_id".into())))?;
        Ok(p)
    }

    pub fn digest(&self) -> Result<Ref> {
        Ok(axon_loop_contracts::digest(self)?)
    }

    /// The operator fields still `null`, in schema order. Empty ⇒ complete.
    pub fn unset_fields(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        macro_rules! chk {
            ($($f:ident),*) => { $( if self.$f.is_none() { out.push(stringify!($f)); } )* };
        }
        chk!(
            task_manifest_ref,
            repository_split_ref,
            discovery_manifest_ref,
            confirmation_manifest_ref,
            reporting_manifest_ref,
            controls_ref,
            incumbent_policy_ref,
            candidate_policy_ref,
            analysis_method_ref,
            data_use_ref,
            rollback_policy_ref,
            approval_ref,
            independent_units,
            repetitions,
            candidate_budget,
            independent_unit,
            quality_margin,
            economic_threshold,
            uncertainty_rule,
            missing_data_rule,
            multiplicity_rule,
            order_rule,
            cache_rule,
            budget_rule
        );
        out
    }

    /// Every reason this plan may not START (empty ⇒ may start once frozen).
    /// Mirrors the reference `pilot_ready`.
    pub fn start_blockers(&self) -> Vec<String> {
        let mut b = Vec::new();
        if !self.operator_approved {
            b.push("operator_approved is false".to_string());
        }
        if !self.runtime_ready {
            b.push("runtime_ready is false".to_string());
        }
        for f in self.unset_fields() {
            b.push(format!("unset operator field: {f}"));
        }
        if let (Some(d), Some(c), Some(r)) = (
            &self.discovery_manifest_ref,
            &self.confirmation_manifest_ref,
            &self.reporting_manifest_ref,
        ) {
            if d == c || d == r || c == r {
                b.push("corpus role aliasing: discovery/confirmation/reporting manifests must be distinct".into());
            }
        }
        if self.incumbent_policy_ref.is_some()
            && self.incumbent_policy_ref == self.candidate_policy_ref
        {
            b.push("candidate equals incumbent".into());
        }
        b
    }
}

/// Register (or, while NOT frozen, replace) a plan. A frozen id is refused.
pub fn register(store: &Store, plan: &PilotPlan) -> Result<Ref> {
    store.plan_dir(&plan.experiment_id)?; // id validated before any fs call
    let mut tx = Tx::begin(store)?;
    if tx.freeze_of(&plan.experiment_id).is_some() {
        return Err(refused(format!(
            "plan {} is frozen; a frozen plan cannot change",
            plan.experiment_id
        )));
    }
    let r = plan.digest()?;
    if let Some(Event::PlanRegistered { plan_ref, .. }) =
        tx.latest_registration(&plan.experiment_id)
    {
        if plan_ref == &r {
            return Ok(r);
        }
    }
    store.put_cas("plans", plan)?;
    tx.append(Event::PlanRegistered {
        experiment_id: plan.experiment_id.clone(),
        scope: plan.scope.clone(),
        plan_ref: r.clone(),
    })?;
    Ok(r)
}

fn load_registered(tx: &Tx, id: &str) -> Result<(PilotPlan, Ref)> {
    let r = match tx.latest_registration(id) {
        Some(Event::PlanRegistered { plan_ref, .. }) => plan_ref.clone(),
        _ => return Err(refused(format!("no registered plan {id}"))),
    };
    Ok((load_by_ref(tx.store, &r)?, r))
}

/// Load a plan by digest, re-checking bytes against the name.
pub fn load_by_ref(store: &Store, r: &Ref) -> Result<PilotPlan> {
    let text = store
        .read_text(&store.cas_path("plans", r)?)?
        .ok_or_else(|| LoopError::Io(format!("store corrupt: plan {r} missing")))?;
    let p = PilotPlan::parse(&text)?;
    if &p.digest()? != r {
        return Err(LoopError::Io(format!("store corrupt: plan {r} changed")));
    }
    Ok(p)
}

/// The freeze facts the rest of the loop binds to.
/// ADR-001 D3: how an experiment's evidence may be used. `Development` (the
/// default) is recorded and reportable, never protected evidence; `Protected`
/// counts a trial only when every receipt it rests on came from a
/// [`axon_loop_contracts::PROTECTED_PROFILES`] backend. Fixed at freeze.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationClass {
    #[default]
    Development,
    Protected,
}

impl EvaluationClass {
    pub fn is_development(&self) -> bool {
        *self == EvaluationClass::Development
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Frozen {
    pub evaluation_class: EvaluationClass,
    pub plan: PilotPlan,
    pub plan_ref: Ref,
    pub freeze_seq: u64,
    pub freeze_ms: u64,
    pub authority_epoch: AuthorityEpoch,
}

pub(crate) fn frozen_in(tx: &Tx, id: &str) -> Result<Option<Frozen>> {
    let Some((
        seq,
        Event::Freeze {
            plan_ref,
            authority_epoch,
            evaluation_class,
            ..
        },
    )) = tx.freeze_of(id)
    else {
        return Ok(None);
    };
    Ok(Some(Frozen {
        evaluation_class: *evaluation_class,
        plan: load_by_ref(tx.store, plan_ref)?,
        plan_ref: plan_ref.clone(),
        freeze_seq: seq,
        freeze_ms: tx.recorded_ms(seq),
        authority_epoch: *authority_epoch,
    }))
}

/// Freeze: permanent, journalled, one candidate per experiment. Idempotent.
pub fn freeze(store: &Store, id: &str) -> Result<Ref> {
    store.plan_dir(id)?;
    let mut tx = Tx::begin(store)?;
    let (plan, r) = load_registered(&tx, id)?;
    if let Some(f) = frozen_in(&tx, id)? {
        if f.plan_ref != r {
            return Err(LoopError::Io(format!(
                "store corrupt: plan {id} re-registered after its freeze"
            )));
        }
        return Ok(r);
    }
    let unset = plan.unset_fields();
    if !unset.is_empty() {
        return Err(LoopError::NotReady(format!(
            "cannot freeze {id}: operator fields unset: {}",
            unset.join(", ")
        )));
    }
    crate::rules::Rules::parse(&plan)
        .map_err(|e| LoopError::NotReady(format!("cannot freeze {id}: {e}")))?;
    let inc = plan.incumbent_policy_ref.clone().expect("set");
    let cand = plan.candidate_policy_ref.clone().expect("set");
    if inc == cand {
        return Err(LoopError::NotReady("candidate equals incumbent".into()));
    }
    for e in tx.entries() {
        if let Event::Freeze {
            experiment_id,
            candidate_policy_ref,
            ..
        } = &e.event
        {
            if candidate_policy_ref == &cand {
                return Err(refused(format!(
                    "candidate {cand} is already frozen in experiment {experiment_id}: one experiment per candidate (no plan shopping)"
                )));
            }
        }
    }
    check_candidate(&tx, &plan, &inc, &cand)?;
    // AB9/AB10: the assigned task set is a REGISTERED manifest, frozen with
    // the plan, and it must hold at least the planned independent units.
    let manifest = crate::tasks::resolve(
        &tx,
        &plan.scope,
        plan.task_manifest_ref.as_ref().expect("set"),
    )?;
    let units = plan.independent_units.expect("set");
    if (manifest.tasks.len() as u64) < units {
        return Err(LoopError::NotReady(format!(
            "cannot freeze {id}: the task manifest has {} task(s) < independent_units {units}",
            manifest.tasks.len()
        )));
    }
    let epoch = tx.pointer(&plan.scope).epoch;
    let evaluation_class = if store.config()?.protected_scopes.contains(&plan.scope) {
        EvaluationClass::Protected
    } else {
        EvaluationClass::Development
    };
    tx.append(Event::Freeze {
        experiment_id: id.to_string(),
        scope: plan.scope.clone(),
        plan_ref: r.clone(),
        incumbent_policy_ref: inc,
        candidate_policy_ref: cand,
        authority_epoch: epoch,
        evaluation_class,
    })?;
    Ok(r)
}

/// The candidate must be a bounded EVO mutation of this incumbent: proposed
/// (proposer on record), parent = incumbent, same scope/mode/candidate set/
/// controls, shortlist ⊆ the incumbent's, no authority expansion.
pub(crate) fn check_candidate(tx: &Tx, plan: &PilotPlan, inc: &Ref, cand: &Ref) -> Result<()> {
    let ie: PolicyEnvelope = tx.store.get_contract("policies", inc)?;
    let ce: PolicyEnvelope = tx.store.get_contract("policies", cand)?;
    // G2: both shortlists within the registered candidate list.
    crate::candidates::require_shortlist(tx, &ie)?;
    crate::candidates::require_shortlist(tx, &ce)?;
    if crate::evo::proposer_in(tx, &plan.scope, cand).is_none() {
        return Err(refused(format!(
            "candidate {cand} was not produced by EVO in this scope (no proposer on record)"
        )));
    }
    if &ce.parent_policy_ref != inc {
        return Err(refused("candidate's parent is not the plan's incumbent"));
    }
    if ce.scope != plan.scope || ie.scope != plan.scope {
        return Err(refused("candidate/incumbent scope differs from the plan"));
    }
    if Some(&ce.controls_ref) != plan.controls_ref.as_ref() || ce.controls_ref != ie.controls_ref {
        return Err(refused(
            "candidate controls differ from the plan's frozen controls",
        ));
    }
    if ce.candidate_set_ref != ie.candidate_set_ref || ce.mode != ie.mode {
        return Err(refused(
            "candidate changes the eligible candidate view or mode",
        ));
    }
    if let Some(c) = ce.shortlist.iter().find(|c| !ie.shortlist.contains(c)) {
        return Err(refused(format!(
            "candidate adds {c}: a shortlist mutation may only reorder/remove"
        )));
    }
    if ce.authority_expansion {
        return Err(refused("authority_expansion"));
    }
    Ok(())
}

/// What `plan show` reports.
#[derive(Clone, Debug, Serialize)]
pub struct PlanView {
    pub plan: PilotPlan,
    pub plan_ref: Ref,
    pub frozen_ref: Option<Ref>,
    pub freeze_seq: Option<u64>,
    pub unset_fields: Vec<&'static str>,
    pub start_blockers: Vec<String>,
}

pub fn show(store: &Store, id: &str) -> Result<PlanView> {
    store.plan_dir(id)?;
    let tx = Tx::begin(store)?;
    let (plan, plan_ref) = load_registered(&tx, id)?;
    let frozen = frozen_in(&tx, id)?;
    let mut start_blockers = plan.start_blockers();
    if frozen.is_none() {
        start_blockers.insert(0, "plan is not frozen".into());
    }
    Ok(PlanView {
        unset_fields: plan.unset_fields(),
        frozen_ref: frozen.as_ref().map(|f| f.plan_ref.clone()),
        freeze_seq: frozen.as_ref().map(|f| f.freeze_seq),
        plan_ref,
        start_blockers,
        plan,
    })
}

/// The frozen, startable plan — or `NotReady` naming every blocker.
pub(crate) fn ready_in(tx: &Tx, id: &str) -> Result<Frozen> {
    check_segment_id(id)?;
    let Some(f) = frozen_in(tx, id)? else {
        return Err(LoopError::NotReady(format!("plan {id} is not frozen")));
    };
    let b = f.plan.start_blockers();
    if !b.is_empty() {
        return Err(LoopError::NotReady(format!(
            "plan {id} may not start: {}",
            b.join("; ")
        )));
    }
    Ok(f)
}

pub fn ready(store: &Store, id: &str) -> Result<Frozen> {
    check_segment_id(id)?;
    ready_in(&Tx::begin(store)?, id)
}

fn check_segment_id(id: &str) -> Result<()> {
    crate::store::check_segment("experiment id", id)
}
