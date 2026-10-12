//! The eligible candidate LIST behind a `candidate_set_ref` (interop gap G2).
//!
//! A policy envelope names its candidate view only by digest. Without the
//! list, Axon cannot check `shortlist ⊆ candidates` and would store a
//! tool-ADDING policy, leaving MiCode as the sole enforcement point. So the
//! list is registered here, by a trusted admitter, as a ledger event:
//!
//! * the document is `axon.loop.candidate-set/1` `{scope, candidates,
//!   issuer_ref}`; `candidates` is a non-empty, sorted, duplicate-free list of
//!   [`CandidateId`]s (sorted so one set has one spelling);
//! * its `candidate_set_ref` is `cl22:` over the candidate ARRAY — the same
//!   rule MiCode's `PolicyScope::candidate_set_ref` and Axon's DEC driver use,
//!   so the ref a policy carries is the ref registered here;
//! * the list is stored under `candidate-sets/<tenant>/<family>/<hex>.json`
//!   (scope-keyed: the same list registered for two scopes is two files, so
//!   one registration can never overwrite another's, NS3) and re-checked
//!   against its name AND scope on every read.
//!
//! [`require_shortlist`] is called by `policy put`, `evo propose`, plan freeze,
//! `admit` and `activate`: an unregistered `candidate_set_ref` is REFUSED
//! (never "unknown ⇒ allow"), and every shortlist entry must be in the list.
//! Registration does not grant a tool: the list is the view MiCode's own
//! permission layer already authorised, recorded so Axon can check against it.

use crate::error::{refused, LoopError, Result};
use crate::ledger::{Event, Tx};
use crate::store::{strict_record, Store};
use axon_loop_contracts::{
    check_shortlist, digest_value, CandidateId, OpaqueRef, PolicyEnvelope, Ref, Refusal, Scope,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

crate::record_tag!(CandidateSetSchema, "axon.loop.candidate-set/1");

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateSet {
    pub schema: CandidateSetSchema,
    pub scope: Scope,
    pub candidates: Vec<CandidateId>,
    pub issuer_ref: OpaqueRef,
}

impl CandidateSet {
    pub fn parse(text: &str) -> Result<CandidateSet> {
        let c: CandidateSet = strict_record(text)?;
        c.validate()?;
        Ok(c)
    }

    fn validate(&self) -> Result<()> {
        if self.candidates.is_empty() || self.candidates.len() > 4096 {
            return Err(LoopError::Malformed(Refusal::Shape(
                "candidates: 1..=4096 items".into(),
            )));
        }
        if !self.candidates.windows(2).all(|w| w[0] < w[1]) {
            return Err(LoopError::Malformed(Refusal::Shape(
                "candidates must be sorted and duplicate-free (one set, one spelling)".into(),
            )));
        }
        Ok(())
    }

    /// `cl22:` over the candidate array — the policy's `candidate_set_ref`.
    pub fn candidate_set_ref(&self) -> Result<Ref> {
        let names: Vec<&str> = self.candidates.iter().map(CandidateId::as_str).collect();
        Ok(digest_value(&serde_json::json!(names))?)
    }

    pub fn eligible(&self) -> BTreeSet<CandidateId> {
        self.candidates.iter().cloned().collect()
    }
}

/// `candidate-sets/<tenant>/<family>/<hex>.json` (NS3: scope-keyed, so the same list
/// registered for another scope is a second file, never an overwrite).
fn path(store: &Store, scope: &Scope, r: &Ref) -> Result<std::path::PathBuf> {
    store.scoped_cas_path("candidate-sets", scope, r)
}

/// Read the record for `(scope, r)`. A store written before NS3 kept it at
/// the flat `candidate-sets/<hex>.json`; that file is accepted only when its bytes
/// name THIS scope (checked by the caller), so it is read, never trusted.
fn read(tx: &Tx, scope: &Scope, r: &Ref) -> Result<Option<String>> {
    match tx.store.read_text(&path(tx.store, scope, r)?)? {
        Some(t) => Ok(Some(t)),
        None => tx.store.read_text(&tx.store.cas_path("candidate-sets", r)?),
    }
}

/// Register a candidate list. Idempotent. Returns its `candidate_set_ref`.
pub fn put(store: &Store, c: &CandidateSet) -> Result<Ref> {
    c.validate()?;
    let mut tx = Tx::begin(store)?;
    if !store.config()?.admitters().contains(&c.issuer_ref) {
        return Err(refused(format!(
            "issuer {} is not a trusted admitter",
            c.issuer_ref
        )));
    }
    let r = c.candidate_set_ref()?;
    let registered = tx.candidate_set_event(&c.scope, &r);
    if registered && resolve(&tx, &c.scope, &r).is_ok() {
        return Ok(r);
    }
    // The path is keyed by (scope, list), so this can only ever replace a
    // file for the SAME scope and list: never another scope's (NS3). When the
    // event already exists this is a repair: it restores the record of a
    // store whose flat pre-NS3 file another scope overwrote.
    let bytes = axon_loop_contracts::canonical_json(c)?;
    store.write_atomic(&path(store, &c.scope, &r)?, &bytes)?;
    if registered {
        return Ok(r);
    }
    tx.append(Event::CandidateSet {
        scope: c.scope.clone(),
        candidate_set_ref: r.clone(),
    })?;
    Ok(r)
}

/// The registered list for `(scope, candidate_set_ref)`, or a refusal.
pub fn resolve(tx: &Tx, scope: &Scope, r: &Ref) -> Result<CandidateSet> {
    if !tx.candidate_set_event(scope, r) {
        return Err(refused(format!(
            "candidate_set_ref {r} is not a registered candidate list for {}/{}: \
             register it with `candidates put` first (an unknown view is never allowed)",
            scope.tenant_id, scope.task_family
        )));
    }
    let text = read(tx, scope, r)?
        .ok_or_else(|| LoopError::Io(format!("store corrupt: candidate set {r} missing")))?;
    let c: CandidateSet = strict_record(&text)
        .map_err(|e| LoopError::Io(format!("store corrupt: candidate set {r}: {e}")))?;
    if &c.candidate_set_ref()? != r || &c.scope != scope {
        return Err(LoopError::Io(format!(
            "store corrupt: candidate set {r} does not match its name"
        )));
    }
    Ok(c)
}

/// `check_shortlist` against the REGISTERED list: the policy's view must be
/// registered for its scope, and every shortlist entry must be in it.
pub fn require_shortlist(tx: &Tx, p: &PolicyEnvelope) -> Result<()> {
    let set = resolve(tx, &p.scope, &p.candidate_set_ref)?;
    check_shortlist(
        p,
        &set.eligible(),
        &p.candidate_set_ref,
        &p.controls_ref,
        &p.scope,
    )?;
    Ok(())
}

/// `policy put`: store a policy only if its shortlist is within a registered
/// candidate list. Returns its `cl22:` ref.
pub fn put_policy(store: &Store, p: &PolicyEnvelope) -> Result<Ref> {
    let tx = Tx::begin(store)?;
    require_shortlist(&tx, p)?;
    store.put_cas("policies", p)
}
