//! Contract D (second role): the preflight ExecutionContextReceipt.
//!
//! An OBSERVATION of context, never a process fact and never upgraded into an
//! [`crate::ExecutionReceipt`] (or vice versa). An `observed_issuer_ref` in
//! JSON authenticates nothing.

use crate::error::Refusal;
use crate::ids::{
    Acf1Ref, AuthorityEpoch, BoundedText, ContextId, OpaqueRef, Ref, RepoId, Scope, TrialIdentity,
    WorktreeId,
};
use crate::{check_array, check_int, schema_tag, Contract};
use serde::{Deserialize, Serialize};

schema_tag!(ContextSchema, "axon.closed-loop.context/1");

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Implementation,
    Critic,
    Verifier,
    Documentation,
    Experiment,
}

/// The facts a preflight compares. A superset of MiCode's
/// EXECUTION_CONTEXT_RECEIPT_SPEC (adds `workspace_ref`, `namespace_ref`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ContextFacts {
    pub repo_id: RepoId,
    pub base_commit: BoundedText,
    pub workspace_ref: Acf1Ref,
    pub branch: BoundedText,
    pub worktree_id: WorktreeId,
    pub working_directory: BoundedText,
    pub build_namespace: BoundedText,
    pub model_ref: Ref,
    pub role: Role,
    pub namespace_ref: Ref,
    pub read_paths: Vec<BoundedText>,
    pub write_paths: Vec<BoundedText>,
    pub is_primary_worktree: bool,
}

impl ContextFacts {
    fn validate(&self, which: &str) -> Result<(), Refusal> {
        check_array(
            &format!("{which}.read_paths"),
            &self.read_paths,
            0,
            1024,
            true,
        )?;
        check_array(
            &format!("{which}.write_paths"),
            &self.write_paths,
            0,
            1024,
            true,
        )
    }
}

/// `closed-loop-context`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionContextReceipt {
    pub schema: ContextSchema,
    pub context_id: ContextId,
    pub identity: TrialIdentity,
    pub scope: Scope,
    pub expected: ContextFacts,
    pub observed: ContextFacts,
    pub expected_issuer_ref: OpaqueRef,
    pub observed_issuer_ref: OpaqueRef,
    pub observed_evidence_ref: Ref,
    pub created_ms: u64,
    pub expires_ms: u64,
    pub authority_epoch: AuthorityEpoch,
}

impl Contract for ExecutionContextReceipt {
    const SCHEMA: &'static str = crate::schema::schema_text!("closed-loop-context.schema.json");
    fn validate(&self) -> Result<(), Refusal> {
        check_int("created_ms", self.created_ms, 0)?;
        check_int("expires_ms", self.expires_ms, 1)?;
        // An empty window (expires <= created) is schema-valid and is refused
        // by `check_context_current`, which is where the reference refuses it;
        // refusing it here would make this type stricter than the schema.
        self.expected.validate("expected")?;
        self.observed.validate("observed")
    }
}
