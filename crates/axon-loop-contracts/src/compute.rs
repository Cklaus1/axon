//! Contract C: `acf-compute-request/1`, field for field.
//!
//! Mirrors the byte-preserved ACF reference contract
//! (`schemas/acf-compute-request.schema.json`). It is NEVER extended in place:
//! bridge-only identities (arm, context, policy) travel on the episode
//! sidecar. Validation does not authorize — `registered_executable_ref` and
//! `executable_digest` must be resolved from a trusted registry, never from
//! caller text.

use crate::error::{shape, Refusal};
use crate::ids::{Acf1Ref, AttemptId, Currency, OpaqueRef, OperationId, TaskId, TrialId};
use crate::{check_int, nullable, schema_tag, Contract};
use serde::{Deserialize, Serialize};

schema_tag!(ComputeRequestSchema, "acf-compute-request/1");

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    InterpreterRun,
    RegisteredCheck,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engine {
    AxonInterpreter,
    AxonWasm,
    NativeProcess,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Os {
    Linux,
    None,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Architecture {
    X86_64,
    Aarch64,
    Wasm32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    Deny,
    Brokered,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckpointKind {
    None,
    LogicalWorkspace,
    Filesystem,
    MachineState,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Required {
    pub engine: Engine,
    pub hardware_isolation: bool,
    pub os: Os,
    pub architecture: Architecture,
    pub network_mode: NetworkMode,
    pub checkpoint_kind: CheckpointKind,
}

/// Every limit is a positive JSON-safe integer; there is no "unlimited".
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub cpu_millicores: u64,
    pub memory_bytes: u64,
    pub disk_bytes: u64,
    pub wall_time_ms: u64,
    pub output_bytes: u64,
    pub max_cost_micro: u64,
    pub currency_code: Currency,
    pub price_schedule_ref: OpaqueRef,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ComputeRequest {
    pub schema: ComputeRequestSchema,
    pub operation_id: OperationId,
    pub task_id: TaskId,
    pub trial_id: TrialId,
    pub attempt_id: AttemptId,
    pub principal_ref: OpaqueRef,
    pub grant_ref: OpaqueRef,
    #[serde(deserialize_with = "nullable")]
    pub approval_ref: Option<OpaqueRef>,
    pub job_kind: JobKind,
    pub registered_executable_ref: OpaqueRef,
    pub executable_digest: Acf1Ref,
    pub workspace_version_ref: Acf1Ref,
    #[serde(deserialize_with = "nullable")]
    pub semantic_state_ref: Option<OpaqueRef>,
    pub policy_digest: Acf1Ref,
    pub required: Required,
    pub limits: Limits,
    pub argv: Vec<String>,
    pub result_schema_ref: OpaqueRef,
}

impl Contract for ComputeRequest {
    fn validate(&self) -> Result<(), Refusal> {
        let l = &self.limits;
        for (what, v) in [
            ("limits.cpu_millicores", l.cpu_millicores),
            ("limits.memory_bytes", l.memory_bytes),
            ("limits.disk_bytes", l.disk_bytes),
            ("limits.wall_time_ms", l.wall_time_ms),
            ("limits.output_bytes", l.output_bytes),
            ("limits.max_cost_micro", l.max_cost_micro),
        ] {
            check_int(what, v, 1)?;
        }
        if self.argv.len() > 128 {
            return Err(shape(format!("argv: {} items, max 128", self.argv.len())));
        }
        if self.argv.iter().any(|a| a.chars().count() > 8192) {
            return Err(shape("argv item longer than 8192 characters"));
        }
        Ok(())
    }
}
