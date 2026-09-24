//! B274 — the experiment register and frozen plan.
//!
//! A plan is the package's `axon.closed-loop.pilot/1` document, validated
//! against the byte-copied package schema (`schemas/closed-loop-pilot.schema.json`)
//! BEFORE typed serde. `freeze` records its `cl22:` digest; after that the
//! plan can never be replaced, and every reader re-checks that the stored
//! bytes still digest to the frozen ref.
//!
//! The package template leaves the operator fields `null` ON PURPOSE. Nothing
//! here fills them: freeze refuses a plan with any unset operator field, and
//! [`ready`] additionally requires `operator_approved`, `runtime_ready`,
//! distinct corpus manifests and a candidate distinct from the incumbent.

use crate::error::{refused, LoopError, Result};
use crate::store::{
    read_json, strict_record, write_bytes_atomic, write_json_atomic, DirLock, Store,
};
use axon_loop_contracts::{Ref, Refusal, Scope, TaskId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PILOT_SCHEMA_TEXT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/schemas/closed-loop-pilot.schema.json"
));

crate::record_tag!(PilotSchema, "axon.closed-loop.pilot/1");
crate::record_tag!(FrozenSchema, "axon.loop.frozen-plan/1");

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

/// The freeze record: `plans/<id>/frozen.json`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenPlan {
    pub schema: FrozenSchema,
    pub experiment_id: String,
    pub plan_ref: Ref,
}

fn plan_path(store: &Store, id: &str) -> std::path::PathBuf {
    store.plan_dir(id).join("plan.json")
}

fn frozen_path(store: &Store, id: &str) -> std::path::PathBuf {
    store.plan_dir(id).join("frozen.json")
}

/// Register (or, while NOT frozen, replace) a plan. A frozen plan is refused.
pub fn register(store: &Store, plan: &PilotPlan) -> Result<Ref> {
    let dir = store.plan_dir(&plan.experiment_id);
    let _lock = DirLock::acquire(&dir)?;
    if frozen_path(store, &plan.experiment_id).exists() {
        return Err(refused(format!(
            "plan {} is frozen; a frozen plan cannot change",
            plan.experiment_id
        )));
    }
    let bytes = axon_loop_contracts::canonical_json(plan)?;
    write_bytes_atomic(&plan_path(store, &plan.experiment_id), &bytes)?;
    plan.digest()
}

fn load_plan(store: &Store, id: &str) -> Result<PilotPlan> {
    TaskId::new(id).map_err(|_| LoopError::Usage(format!("bad experiment id {id:?}")))?;
    let text = match std::fs::read_to_string(plan_path(store, id)) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(refused(format!("no registered plan {id}")))
        }
        Err(e) => return Err(e.into()),
    };
    let p = PilotPlan::parse(&text)?;
    if p.experiment_id != id {
        return Err(LoopError::Io(format!(
            "plan {id} names {}",
            p.experiment_id
        )));
    }
    Ok(p)
}

/// Freeze: compute the `cl22:` digest and record it. Refuses a plan with any
/// unset operator field (a frozen incomplete plan could never start, and the
/// decision rule must be fixed BEFORE protected outcomes). Idempotent.
pub fn freeze(store: &Store, id: &str) -> Result<Ref> {
    let dir = store.plan_dir(id);
    let _lock = DirLock::acquire(&dir)?;
    let plan = load_plan(store, id)?;
    let r = plan.digest()?;
    if let Some(f) = read_json::<FrozenPlan>(&frozen_path(store, id))? {
        if f.plan_ref != r {
            return Err(LoopError::Io(format!(
                "plan {id} bytes digest to {r} but were frozen as {}",
                f.plan_ref
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
    write_json_atomic(
        &frozen_path(store, id),
        &FrozenPlan {
            schema: FrozenSchema,
            experiment_id: id.to_string(),
            plan_ref: r.clone(),
        },
    )?;
    Ok(r)
}

/// What `plan show` reports.
#[derive(Clone, Debug, Serialize)]
pub struct PlanView {
    pub plan: PilotPlan,
    pub plan_ref: Ref,
    pub frozen_ref: Option<Ref>,
    pub unset_fields: Vec<&'static str>,
    pub start_blockers: Vec<String>,
}

pub fn show(store: &Store, id: &str) -> Result<PlanView> {
    let plan = load_plan(store, id)?;
    let plan_ref = plan.digest()?;
    let frozen = read_json::<FrozenPlan>(&frozen_path(store, id))?;
    if let Some(f) = &frozen {
        if f.plan_ref != plan_ref {
            return Err(LoopError::Io(format!("plan {id} changed after freeze")));
        }
    }
    let mut start_blockers = plan.start_blockers();
    if frozen.is_none() {
        start_blockers.insert(0, "plan is not frozen".into());
    }
    Ok(PlanView {
        unset_fields: plan.unset_fields(),
        frozen_ref: frozen.map(|f| f.plan_ref),
        plan_ref,
        start_blockers,
        plan,
    })
}

/// The frozen, startable plan — or `NotReady` naming every blocker.
pub fn ready(store: &Store, id: &str) -> Result<(PilotPlan, Ref)> {
    let v = show(store, id)?;
    if !v.start_blockers.is_empty() {
        return Err(LoopError::NotReady(format!(
            "plan {id} may not start: {}",
            v.start_blockers.join("; ")
        )));
    }
    Ok((v.plan, v.plan_ref))
}
