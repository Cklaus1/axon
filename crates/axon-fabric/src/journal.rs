//! The durable operation journal (B260, first half).
//!
//! # Storage
//!
//! One append-only file, one JSON record per line (`axon-fabric-journal/1`).
//! Every append is `write_all(line + "\n")` followed by `fsync`; creating the
//! file also fsyncs its parent directory, so the file's existence survives a
//! crash along with its contents. A journal is held open by exactly one
//! process at a time (`flock(LOCK_EX)`), so two writers cannot interleave.
//!
//! # The rule this exists for
//!
//! Intent is durable BEFORE any effect. A caller:
//!
//! 1. [`Journal::begin`] — records the operation's immutable request (id, input
//!    digest, config, authority ref, reservation, expected version). Replaying
//!    `begin` with the same id and the same request is idempotent; the same id
//!    with a different request is [`JournalError::Conflict`].
//! 2. [`Journal::reserve`] — carves the reservation out of the scope's
//!    aggregate budget, atomically. A refusal writes nothing.
//! 3. [`Journal::mark_launched`] — recorded BEFORE dispatching the effect.
//! 4. a terminal record: completed / failed / cancelled.
//!
//! On reopen, [`Journal::open`] RECONCILES: an operation that reached
//! `Launched` with no terminal record may or may not have had its effect, so
//! it becomes `OutcomeUnknown` — never `Completed`, never re-queued, never
//! refunded. Its reservation is kept as an unresolved liability until someone
//! [`Journal::settle`]s it with evidence.
//!
//! # Settlement (G13-r22-billing-settlement)
//!
//! A settlement is an accounting receipt: `(origin, sequence, actual)` for one
//! operation. Replaying the IDENTICAL receipt (same origin, same sequence,
//! same op, same content) is idempotent — nothing is written. Anything else
//! that would settle an op a second time, or reuse one `(origin, sequence)`
//! for a different op or different content, is a CONFLICT: it is refused AND
//! recorded (`settle_conflict`), and from then on the op is DISPUTED — its
//! committed amount is the componentwise maximum of its reservation and every
//! amount any receipt claimed, held as liability. A dispute never lowers
//! what is committed and never turns an unknown cost into a known one.
//!
//! # Accounting
//!
//! Per scope, committed = held (Reserved/Launched) + liability (terminal with
//! unknown cost, or OutcomeUnknown) + charged (terminal with known cost). A new
//! reservation succeeds only if committed + request ≤ ceiling on EVERY
//! dimension. Failed and cancelled-after-launch work is charged (known cost) or
//! held as liability (unknown cost); it is never dropped. The only release is
//! a cancel of an operation that was reserved but never launched — no effect
//! was dispatched, so there is nothing to bill.
//!
//! # Not here yet
//!
//! No epoch/fencing token, no outbox, no launcher integration, and
//! `expected_version` is recorded as the effect's CAS precondition but not
//! compared against anything by this module — `crate::branches` compares it
//! for workspace publication (B271).

use std::collections::{BTreeMap, HashMap};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use axon_loop_contracts::{AttemptId, AuthorityEpoch, OperationId, Ref, Scope, TaskId, TrialId};

pub const JOURNAL_SCHEMA: &str = "axon-fabric-journal/1";

/// The combined budget dimensions an operation reserves against.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceVector {
    /// Model spend, micro-USD (1e-6 USD, the Axon unit).
    pub model_micro_usd: u64,
    pub exec_ms: u64,
    pub verify_ms: u64,
    pub retries: u64,
}

impl ResourceVector {
    /// The dimensions, named, in a fixed order.
    fn dims(self) -> [(&'static str, u64); 4] {
        [
            ("model_micro_usd", self.model_micro_usd),
            ("exec_ms", self.exec_ms),
            ("verify_ms", self.verify_ms),
            ("retries", self.retries),
        ]
    }

    /// Carve `req` on top of `committed` under `ceiling` through axon-os's
    /// ledger algebra (`ResourceLedger::carve`, D-C5) rather than a parallel
    /// comparison. `ResourceLedger` has three fixed axes and the Fabric has
    /// four dimensions, so each dimension is carved on its own single-axis
    /// ledger (`compute`); the refused dimension is reported by name.
    ///
    /// One guard `carve` does not provide: it checks with `saturating_add`
    /// and then adds UNCHECKED, so at a cap of `u64::MAX` an overflowing carve
    /// passes the check and then panics (debug) or wraps (release). An
    /// overflowing sum is refused here first, as the journal always did.
    fn carve_within(
        committed: Self,
        req: Self,
        ceiling: Self,
        lineage: &str,
    ) -> Result<(), &'static str> {
        use axon_os::ledger::{Carve, ResourceLedger};
        for (((name, used), (_, want)), (_, cap)) in committed
            .dims()
            .into_iter()
            .zip(req.dims())
            .zip(ceiling.dims())
        {
            if used.checked_add(want).is_none() {
                return Err(name);
            }
            let mut l = ResourceLedger::new(lineage, cap, 0, 0);
            l.compute_used = used;
            l.carve(Carve {
                compute: want,
                budget: 0,
                persist_bytes: 0,
            })
            .map_err(|_| name)?;
        }
        Ok(())
    }
    fn componentwise_max(self, o: Self) -> Self {
        ResourceVector {
            model_micro_usd: self.model_micro_usd.max(o.model_micro_usd),
            exec_ms: self.exec_ms.max(o.exec_ms),
            verify_ms: self.verify_ms.max(o.verify_ms),
            retries: self.retries.max(o.retries),
        }
    }
    fn saturating_add(self, o: Self) -> Self {
        ResourceVector {
            model_micro_usd: self.model_micro_usd.saturating_add(o.model_micro_usd),
            exec_ms: self.exec_ms.saturating_add(o.exec_ms),
            verify_ms: self.verify_ms.saturating_add(o.verify_ms),
            retries: self.retries.saturating_add(o.retries),
        }
    }
}

/// What an operation actually cost. `Unknown` is a first-class answer: it
/// keeps the whole reservation as unresolved liability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Billing {
    Known(ResourceVector),
    Unknown,
}

/// The immutable request an operation id is bound to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intent {
    pub op: OperationId,
    pub task_id: TaskId,
    pub trial_id: TrialId,
    pub attempt_id: AttemptId,
    /// `cl22:` digest of the operation's immutable input (for a submitted
    /// `ComputeRequest`, the whole request). Same op + different digest is a
    /// CONFLICT.
    pub input_digest: Ref,
    /// Opaque execution configuration (profile, limits …) as supplied.
    pub config: serde_json::Value,
    /// Reference to the authority (grant / approval) this runs under.
    pub authority_ref: String,
    /// The authority epoch the intent was admitted under.
    pub authority_epoch: AuthorityEpoch,
    pub scope: Scope,
    pub reservation: ResourceVector,
    /// The version of the target the effect expects to find (CAS precondition).
    pub expected_version: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpState {
    Intended,
    Reserved,
    Launched,
    Completed,
    Failed,
    Cancelled,
    OutcomeUnknown,
}

impl OpState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            OpState::Completed | OpState::Failed | OpState::Cancelled | OpState::OutcomeUnknown
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpView {
    pub intent: Intent,
    pub state: OpState,
    /// Whether `Launched` was ever recorded (i.e. an effect may have happened).
    pub launched: bool,
    /// For a terminal op: how it is billed. `None` while in flight, and for a
    /// released (never-launched) cancel.
    pub billing: Option<Billing>,
    pub reason: Option<String>,
    /// The terminal outcome document (e.g. an `acf-execution-receipt/1`)
    /// recorded for this op, once. A duplicate submit returns THIS rather than
    /// re-executing.
    pub outcome: Option<serde_json::Value>,
    /// The accepted settlement receipt, if the op's unknown cost was settled.
    pub settlement: Option<SettlementReceipt>,
    /// Conflicting settlement receipts that were refused and recorded. Non-empty
    /// means DISPUTED: the op is held at the conservative maximum as liability.
    pub disputes: Vec<SettlementReceipt>,
}

impl OpView {
    pub fn disputed(&self) -> bool {
        !self.disputes.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScopeUsage {
    pub ceiling: ResourceVector,
    /// Reserved or launched, not yet terminal.
    pub held: ResourceVector,
    /// Terminal (or OutcomeUnknown) with unknown cost: the reservation is kept.
    pub liability: ResourceVector,
    /// Terminal with known cost.
    pub charged: ResourceVector,
}

impl ScopeUsage {
    pub fn committed(&self) -> ResourceVector {
        self.held
            .saturating_add(self.liability)
            .saturating_add(self.charged)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Begin {
    /// A new intent was recorded and fsynced.
    Recorded,
    /// The same id with the byte-identical request was already recorded; no
    /// record written. The caller resumes from `view.state` — it does NOT
    /// re-run an effect that `view` says may already have happened.
    AlreadyRecorded(Box<OpView>),
}

/// What reopening found and did.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecoveryReport {
    pub records_replayed: u64,
    /// Bytes of an incomplete final line (a write torn by a crash) that were
    /// truncated. The caller of that write never received `Ok`.
    pub torn_tail_bytes: u64,
    /// Operations that were `Launched` with no terminal record, now
    /// `OutcomeUnknown` (liability kept).
    pub reconciled_unknown: Vec<OperationId>,
    /// Operations left `Intended` (no effect, no reservation).
    pub pending_intended: Vec<OperationId>,
    /// Operations left `Reserved` (budget held, never launched).
    pub pending_reserved: Vec<OperationId>,
}

/// Settlement of an unresolved liability with evidence of the actual cost:
/// an accounting receipt identified by `(origin, sequence)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    /// Who issued the accounting receipt (e.g. a metering source). Non-empty.
    pub origin: String,
    /// The receipt's sequence number within `origin`.
    pub sequence: u64,
    pub actual: ResourceVector,
}

/// A settlement receipt as recorded against an op.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettlementReceipt {
    pub origin: String,
    pub sequence: u64,
    pub actual: ResourceVector,
}

/// What [`Journal::settle`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Settle {
    /// A new settlement was recorded and fsynced.
    Recorded,
    /// The identical receipt (origin, sequence, op, content) was already
    /// recorded; nothing was written.
    AlreadySettled,
}

#[derive(Debug)]
pub enum JournalError {
    Io(std::io::Error),
    /// Another process holds the journal.
    Locked(PathBuf),
    /// A line other than the last is unparseable or violates the state
    /// machine. The journal is refused rather than partially trusted.
    Corrupt {
        line: u64,
        reason: String,
    },
    /// Same operation id, different immutable request.
    Conflict {
        op: OperationId,
        recorded: Box<Intent>,
        requested: Box<Intent>,
    },
    UnknownOp(OperationId),
    /// A settlement receipt that is not an identical duplicate of the one on
    /// record: refused, and RECORDED as a dispute (see the module docs).
    SettlementConflict {
        op: OperationId,
        reason: String,
    },
    /// A settlement receipt with no usable identity (empty origin).
    InvalidSettlement(String),
    UnknownScope(Box<Scope>),
    /// A budget scope was redeclared with a different ceiling.
    ScopeConflict {
        scope: Box<Scope>,
        recorded: ResourceVector,
        requested: ResourceVector,
    },
    InvalidTransition {
        op: OperationId,
        from: OpState,
        to: &'static str,
    },
    /// The reservation does not fit. Nothing was written.
    BudgetExceeded {
        scope: Box<Scope>,
        /// `[requested, committed, ceiling]`, boxed to keep the error small.
        vectors: Box<[ResourceVector; 3]>,
        /// The first dimension that did not fit, as `ResourceLedger::carve`
        /// reported it.
        dimension: &'static str,
    },
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JournalError::Io(e) => write!(f, "journal I/O: {e}"),
            JournalError::Locked(p) => {
                write!(f, "journal {} is held by another process", p.display())
            }
            JournalError::Corrupt { line, reason } => {
                write!(f, "journal corrupt at line {line}: {reason}")
            }
            JournalError::Conflict { op, .. } => write!(
                f,
                "operation {op} is already recorded with a different request; refusing"
            ),
            JournalError::UnknownOp(op) => write!(f, "unknown operation {op}"),
            JournalError::SettlementConflict { op, reason } => write!(
                f,
                "operation {op}: conflicting settlement receipt refused and recorded: {reason}"
            ),
            JournalError::InvalidSettlement(why) => write!(f, "invalid settlement: {why}"),
            JournalError::UnknownScope(s) => write!(f, "unknown budget scope {s:?}"),
            JournalError::ScopeConflict { scope, .. } => {
                write!(
                    f,
                    "budget scope {scope:?} already declared with a different ceiling"
                )
            }
            JournalError::InvalidTransition { op, from, to } => {
                write!(f, "operation {op}: cannot go from {from:?} to {to}")
            }
            JournalError::BudgetExceeded {
                scope, dimension, ..
            } => {
                write!(
                    f,
                    "reservation exceeds the remaining {dimension} budget of scope {scope:?}"
                )
            }
        }
    }
}

impl std::error::Error for JournalError {}

impl From<std::io::Error> for JournalError {
    fn from(e: std::io::Error) -> Self {
        JournalError::Io(e)
    }
}

// ── on-disk records ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Rec {
    Header {
        schema: String,
    },
    Budget {
        scope: Scope,
        ceiling: ResourceVector,
    },
    Intent {
        intent: Intent,
    },
    Reserved {
        op: OperationId,
    },
    Launched {
        op: OperationId,
    },
    Completed {
        op: OperationId,
        billing: Billing,
    },
    Failed {
        op: OperationId,
        reason: String,
        billing: Billing,
    },
    Cancelled {
        op: OperationId,
        reason: String,
        /// Required iff the op was launched.
        billing: Option<Billing>,
    },
    OutcomeUnknown {
        op: OperationId,
        reason: String,
    },
    Settled {
        op: OperationId,
        origin: String,
        sequence: u64,
        actual: ResourceVector,
    },
    /// A refused, conflicting settlement receipt, kept as evidence.
    SettleConflict {
        op: OperationId,
        origin: String,
        sequence: u64,
        actual: ResourceVector,
        reason: String,
    },
    /// The outcome document for a terminal op. At most one per op.
    Outcome {
        op: OperationId,
        outcome: serde_json::Value,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct Line {
    seq: u64,
    #[serde(flatten)]
    rec: Rec,
}

// ── state ───────────────────────────────────────────────────────────────────

#[derive(Default)]
struct State {
    scopes: HashMap<Scope, ResourceVector>,
    ops: BTreeMap<OperationId, OpView>,
}

enum Change {
    None,
    /// An identical duplicate of what is already recorded: the live path writes
    /// nothing, and a journal containing one is not one it could have written.
    Duplicate,
    Scope(Scope, ResourceVector),
    Op(Box<OpView>),
}

impl State {
    /// Validate `rec` against the current state and return the change it
    /// makes. Used identically for live appends and for replay, so a journal
    /// that replays is one the live path could have written.
    fn transition(&self, rec: &Rec) -> Result<Change, JournalError> {
        let get = |op: &OperationId| {
            self.ops
                .get(op)
                .cloned()
                .ok_or_else(|| JournalError::UnknownOp(op.clone()))
        };
        let bad = |v: &OpView, to: &'static str| JournalError::InvalidTransition {
            op: v.intent.op.clone(),
            from: v.state,
            to,
        };
        Ok(match rec {
            Rec::Header { .. } => Change::None,
            Rec::Budget { scope, ceiling } => match self.scopes.get(scope) {
                Some(c) if c == ceiling => Change::None,
                Some(c) => {
                    return Err(JournalError::ScopeConflict {
                        scope: Box::new(scope.clone()),
                        recorded: *c,
                        requested: *ceiling,
                    })
                }
                None => Change::Scope(scope.clone(), *ceiling),
            },
            Rec::Intent { intent } => {
                if !self.scopes.contains_key(&intent.scope) {
                    return Err(JournalError::UnknownScope(Box::new(intent.scope.clone())));
                }
                if let Some(v) = self.ops.get(&intent.op) {
                    if v.intent != *intent {
                        return Err(JournalError::Conflict {
                            op: intent.op.clone(),
                            recorded: Box::new(v.intent.clone()),
                            requested: Box::new(intent.clone()),
                        });
                    }
                    return Ok(Change::None);
                }
                Change::Op(Box::new(OpView {
                    intent: intent.clone(),
                    state: OpState::Intended,
                    launched: false,
                    billing: None,
                    reason: None,
                    outcome: None,
                    settlement: None,
                    disputes: Vec::new(),
                }))
            }
            Rec::Reserved { op } => {
                let mut v = get(op)?;
                if v.state != OpState::Intended {
                    return Err(bad(&v, "reserved"));
                }
                let usage = self.usage(&v.intent.scope)?;
                if let Err(dimension) = ResourceVector::carve_within(
                    usage.committed(),
                    v.intent.reservation,
                    usage.ceiling,
                    v.intent.op.as_str(),
                ) {
                    return Err(JournalError::BudgetExceeded {
                        scope: Box::new(v.intent.scope.clone()),
                        vectors: Box::new([v.intent.reservation, usage.committed(), usage.ceiling]),
                        dimension,
                    });
                }
                v.state = OpState::Reserved;
                Change::Op(Box::new(v))
            }
            Rec::Launched { op } => {
                let mut v = get(op)?;
                if v.state != OpState::Reserved {
                    return Err(bad(&v, "launched"));
                }
                v.state = OpState::Launched;
                v.launched = true;
                Change::Op(Box::new(v))
            }
            Rec::Completed { op, billing } => {
                let mut v = get(op)?;
                if v.state != OpState::Launched {
                    return Err(bad(&v, "completed"));
                }
                v.state = OpState::Completed;
                v.billing = Some(*billing);
                Change::Op(Box::new(v))
            }
            Rec::Failed {
                op,
                reason,
                billing,
            } => {
                let mut v = get(op)?;
                // A failure before launch is still a recorded failure; it had
                // no effect, but a Known(0)/Unknown billing is the caller's
                // statement and is kept as-is.
                if !matches!(
                    v.state,
                    OpState::Intended | OpState::Reserved | OpState::Launched
                ) {
                    return Err(bad(&v, "failed"));
                }
                if v.state == OpState::Intended {
                    // Nothing was reserved, so there is nothing to hold.
                    v.billing = Some(Billing::Known(ResourceVector::default()));
                } else {
                    v.billing = Some(*billing);
                }
                v.state = OpState::Failed;
                v.reason = Some(reason.clone());
                Change::Op(Box::new(v))
            }
            Rec::Cancelled {
                op,
                reason,
                billing,
            } => {
                let mut v = get(op)?;
                match (v.state, billing) {
                    // Never launched: no effect was dispatched → released.
                    (OpState::Intended | OpState::Reserved, None) => v.billing = None,
                    // Launched: the cancel acknowledges a request to stop, not
                    // that nothing happened. Billing is mandatory.
                    (OpState::Launched, Some(b)) => v.billing = Some(*b),
                    _ => return Err(bad(&v, "cancelled")),
                }
                v.state = OpState::Cancelled;
                v.reason = Some(reason.clone());
                Change::Op(Box::new(v))
            }
            Rec::OutcomeUnknown { op, reason } => {
                let mut v = get(op)?;
                if v.state != OpState::Launched {
                    return Err(bad(&v, "outcome_unknown"));
                }
                v.state = OpState::OutcomeUnknown;
                v.billing = Some(Billing::Unknown);
                v.reason = Some(reason.clone());
                Change::Op(Box::new(v))
            }
            Rec::Settled {
                op,
                origin,
                sequence,
                actual,
            } => {
                let r = SettlementReceipt {
                    origin: origin.clone(),
                    sequence: *sequence,
                    actual: *actual,
                };
                match self.settle_check(op, &r)? {
                    SettleCheck::Duplicate => Change::Duplicate,
                    SettleCheck::Conflict(reason) => {
                        return Err(JournalError::SettlementConflict {
                            op: op.clone(),
                            reason,
                        })
                    }
                    SettleCheck::New => {
                        let mut v = get(op)?;
                        // A disputed op stays disputed; its billing stays
                        // unknown (committed at the conservative maximum).
                        if v.disputes.is_empty() {
                            v.billing = Some(Billing::Known(*actual));
                        }
                        v.settlement = Some(r);
                        Change::Op(Box::new(v))
                    }
                }
            }
            Rec::SettleConflict {
                op,
                origin,
                sequence,
                actual,
                reason: _,
            } => {
                let r = SettlementReceipt {
                    origin: origin.clone(),
                    sequence: *sequence,
                    actual: *actual,
                };
                // Only a receipt that IS a conflict may be recorded as one.
                match self.settle_check(op, &r)? {
                    SettleCheck::Conflict(_) => {
                        let mut v = get(op)?;
                        v.billing = Some(Billing::Unknown);
                        v.disputes.push(r);
                        Change::Op(Box::new(v))
                    }
                    _ => return Err(bad(&get(op)?, "settle_conflict")),
                }
            }
            Rec::Outcome { op, outcome } => {
                let mut v = get(op)?;
                if !v.state.is_terminal() || v.outcome.is_some() {
                    return Err(bad(&v, "outcome"));
                }
                v.outcome = Some(outcome.clone());
                Change::Op(Box::new(v))
            }
        })
    }

    /// Classify a settlement receipt against the recorded state.
    fn settle_check(
        &self,
        op: &OperationId,
        r: &SettlementReceipt,
    ) -> Result<SettleCheck, JournalError> {
        if r.origin.is_empty() {
            return Err(JournalError::InvalidSettlement(
                "settlement origin must be non-empty".into(),
            ));
        }
        let v = self
            .ops
            .get(op)
            .ok_or_else(|| JournalError::UnknownOp(op.clone()))?;
        if let Some(prev) = &v.settlement {
            if prev == r {
                return Ok(SettleCheck::Duplicate);
            }
            return Ok(SettleCheck::Conflict(format!(
                "already settled by {}#{} for {:?}; {}#{} claims {:?}",
                prev.origin, prev.sequence, prev.actual, r.origin, r.sequence, r.actual
            )));
        }
        // Only an op whose cost is unknown can be settled (a disputed op's
        // billing is unknown too).
        if !(v.state.is_terminal() && v.billing == Some(Billing::Unknown)) {
            return Err(JournalError::InvalidTransition {
                op: op.clone(),
                from: v.state,
                to: "settled",
            });
        }
        // One receipt settles at most one op.
        if let Some(other) = self.ops.values().find(|o| {
            o.intent.op != *op
                && o.settlement
                    .as_ref()
                    .is_some_and(|s| s.origin == r.origin && s.sequence == r.sequence)
        }) {
            return Ok(SettleCheck::Conflict(format!(
                "receipt {}#{} already settled operation {}",
                r.origin, r.sequence, other.intent.op
            )));
        }
        Ok(SettleCheck::New)
    }

    fn commit(&mut self, c: Change) {
        match c {
            Change::None | Change::Duplicate => {}
            Change::Scope(s, c) => {
                self.scopes.insert(s, c);
            }
            Change::Op(v) => {
                self.ops.insert(v.intent.op.clone(), *v);
            }
        }
    }

    fn usage(&self, scope: &Scope) -> Result<ScopeUsage, JournalError> {
        self.usage_where(scope, |_| true)
    }

    /// [`Self::usage`] over the scope's ops whose intent satisfies `member`
    /// (B271: one logical branch's ops). `ceiling` is the SCOPE's.
    fn usage_where(
        &self,
        scope: &Scope,
        member: impl Fn(&Intent) -> bool,
    ) -> Result<ScopeUsage, JournalError> {
        let ceiling = *self
            .scopes
            .get(scope)
            .ok_or_else(|| JournalError::UnknownScope(Box::new(scope.clone())))?;
        let mut u = ScopeUsage {
            ceiling,
            ..Default::default()
        };
        for v in self
            .ops
            .values()
            .filter(|v| &v.intent.scope == scope && member(&v.intent))
        {
            let r = v.intent.reservation;
            match (v.state, v.billing) {
                (OpState::Intended, _) => {}
                (OpState::Reserved | OpState::Launched, _) => u.held = u.held.saturating_add(r),
                // Disputed: the componentwise maximum of the reservation and
                // every amount any receipt claimed, held as liability.
                _ if v.disputed() => {
                    let m = v
                        .settlement
                        .iter()
                        .chain(&v.disputes)
                        .fold(r, |m, s| m.componentwise_max(s.actual));
                    u.liability = u.liability.saturating_add(m)
                }
                (_, Some(Billing::Known(a))) => u.charged = u.charged.saturating_add(a),
                (_, Some(Billing::Unknown)) => u.liability = u.liability.saturating_add(r),
                // A released cancel (never launched).
                (_, None) => {}
            }
        }
        Ok(u)
    }
}

enum SettleCheck {
    New,
    Duplicate,
    Conflict(String),
}

// ── the journal ─────────────────────────────────────────────────────────────

struct Inner {
    file: File,
    seq: u64,
    state: State,
}

/// A durable operation journal. `Send + Sync`; share it with `Arc`.
pub struct Journal {
    path: PathBuf,
    inner: Mutex<Inner>,
}

#[cfg(unix)]
fn lock_exclusive(f: &File, path: &Path) -> Result<(), JournalError> {
    use std::os::unix::io::AsRawFd;
    // A flock belongs to the open file DESCRIPTION, and a fork shares it with
    // the child until that child execs (the fd is O_CLOEXEC). So after a
    // same-process drop, any thread's concurrent fork can keep the lock
    // apparently held for the fork→exec window. Measured in this crate's own
    // tests. Retry briefly so `Locked` means a holder that persists.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
    loop {
        // SAFETY: flock on a valid, owned fd; no memory is shared with the kernel.
        let rc = unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc == 0 {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline {
            return Err(JournalError::Locked(path.to_path_buf()));
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
#[cfg(not(unix))]
fn lock_exclusive(_f: &File, _path: &Path) -> Result<(), JournalError> {
    Ok(())
}

fn fsync_dir(path: &Path) -> std::io::Result<()> {
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    File::open(dir)?.sync_all()
}

impl Journal {
    /// Open (creating if absent) and reconcile. See the module docs.
    pub fn open(path: impl AsRef<Path>) -> Result<(Journal, RecoveryReport), JournalError> {
        let path = path.as_ref().to_path_buf();
        let existed = path.exists();
        let file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)?;
        lock_exclusive(&file, &path)?;
        if !existed {
            file.sync_all()?;
            fsync_dir(&path)?;
        }

        let mut report = RecoveryReport::default();
        let mut state = State::default();
        let mut seq = 0u64;

        // Read whole lines; an unterminated final segment is a torn write.
        let mut reader = BufReader::new(&file);
        reader.seek(SeekFrom::Start(0))?;
        let mut good_len: u64 = 0;
        let mut buf = Vec::new();
        let mut lineno = 0u64;
        loop {
            buf.clear();
            let n = reader.read_until(b'\n', &mut buf)?;
            if n == 0 {
                break;
            }
            lineno += 1;
            if buf.last() != Some(&b'\n') {
                report.torn_tail_bytes = n as u64;
                break;
            }
            let line: Line =
                serde_json::from_slice(&buf[..n - 1]).map_err(|e| JournalError::Corrupt {
                    line: lineno,
                    reason: e.to_string(),
                })?;
            if line.seq != seq + 1 {
                return Err(JournalError::Corrupt {
                    line: lineno,
                    reason: format!("sequence {} after {}", line.seq, seq),
                });
            }
            if lineno == 1 {
                match &line.rec {
                    Rec::Header { schema } if schema == JOURNAL_SCHEMA => {}
                    _ => {
                        return Err(JournalError::Corrupt {
                            line: 1,
                            reason: format!("first record must be a {JOURNAL_SCHEMA} header"),
                        })
                    }
                }
            }
            let change = state
                .transition(&line.rec)
                .map_err(|e| JournalError::Corrupt {
                    line: lineno,
                    reason: format!("record does not replay: {e}"),
                })?;
            if matches!(change, Change::Duplicate) {
                return Err(JournalError::Corrupt {
                    line: lineno,
                    reason: "duplicate settlement record (the live path never writes one)".into(),
                });
            }
            state.commit(change);
            seq = line.seq;
            good_len += n as u64;
            report.records_replayed += 1;
        }
        drop(reader);
        if report.torn_tail_bytes > 0 {
            file.set_len(good_len)?;
            file.sync_all()?;
        }

        let journal = Journal {
            path,
            inner: Mutex::new(Inner { file, seq, state }),
        };
        if seq == 0 {
            journal.append(Rec::Header {
                schema: JOURNAL_SCHEMA.to_string(),
            })?;
        }

        // RECONCILE: a launched op with no terminal record may have had its
        // effect. It becomes OutcomeUnknown, liability kept — never Completed
        // and never re-run.
        let (launched, intended, reserved) = {
            let g = journal.lock();
            let pick = |s: OpState| {
                g.state
                    .ops
                    .values()
                    .filter(|v| v.state == s)
                    .map(|v| v.intent.op.clone())
                    .collect::<Vec<_>>()
            };
            (
                pick(OpState::Launched),
                pick(OpState::Intended),
                pick(OpState::Reserved),
            )
        };
        for op in launched {
            journal.append(Rec::OutcomeUnknown {
                op: op.clone(),
                reason: "journal reopened with the operation launched and no terminal \
                         record; the effect may or may not have happened"
                    .to_string(),
            })?;
            report.reconciled_unknown.push(op);
        }
        report.pending_intended = intended;
        report.pending_reserved = reserved;
        Ok((journal, report))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Validate, append + fsync, then apply — all under the lock, so a
    /// concurrent caller sees either none or all of it. A validation failure
    /// writes nothing.
    fn append(&self, rec: Rec) -> Result<(), JournalError> {
        self.append_inner(rec).map(|_| ())
    }

    /// [`Self::append`], reporting whether a record was written (`false` for
    /// an identical duplicate, which writes nothing).
    fn append_inner(&self, rec: Rec) -> Result<bool, JournalError> {
        let mut g = self.lock();
        let change = g.state.transition(&rec)?;
        if matches!(change, Change::Duplicate) {
            return Ok(false);
        }
        let line = Line {
            seq: g.seq + 1,
            rec,
        };
        let mut bytes = serde_json::to_vec(&line).map_err(std::io::Error::other)?;
        bytes.push(b'\n');
        g.file.write_all(&bytes)?;
        g.file.sync_data()?;
        g.seq += 1;
        g.state.commit(change);
        Ok(true)
    }

    pub fn declare_budget(
        &self,
        scope: &Scope,
        ceiling: ResourceVector,
    ) -> Result<(), JournalError> {
        if self.lock().state.scopes.get(scope) == Some(&ceiling) {
            return Ok(());
        }
        self.append(Rec::Budget {
            scope: scope.clone(),
            ceiling,
        })
    }

    /// Record intent BEFORE any effect. Idempotent for an identical request;
    /// [`JournalError::Conflict`] for the same id with a different one.
    pub fn begin(&self, intent: Intent) -> Result<Begin, JournalError> {
        {
            let g = self.lock();
            if let Some(v) = g.state.ops.get(&intent.op) {
                if v.intent == intent {
                    return Ok(Begin::AlreadyRecorded(Box::new(v.clone())));
                }
                return Err(JournalError::Conflict {
                    op: intent.op.clone(),
                    recorded: Box::new(v.intent.clone()),
                    requested: Box::new(intent),
                });
            }
        }
        match self.append(Rec::Intent {
            intent: intent.clone(),
        }) {
            Ok(()) => Ok(Begin::Recorded),
            Err(e) => Err(e),
        }
    }

    /// Carve the operation's reservation from its scope, atomically.
    pub fn reserve(&self, op: &OperationId) -> Result<(), JournalError> {
        self.append(Rec::Reserved { op: op.clone() })
    }

    /// [`Self::reserve`], and ALSO within `sub_ceiling` over the scope's ops
    /// that satisfy `member` (B271: a logical branch's declared regime). Both
    /// carves go through `ResourceVector::carve_within`, and the sub-ceiling
    /// check and the append happen under ONE lock. A refusal writes nothing.
    /// The sub-ceiling is not a journal record, so reopening re-checks only
    /// the scope ceiling (which a prefix of accepted carves always satisfies).
    pub fn reserve_within(
        &self,
        op: &OperationId,
        sub_ceiling: ResourceVector,
        member: impl Fn(&Intent) -> bool,
    ) -> Result<(), JournalError> {
        let mut g = self.lock();
        let v = g
            .state
            .ops
            .get(op)
            .ok_or_else(|| JournalError::UnknownOp(op.clone()))?
            .clone();
        let sub = g.state.usage_where(&v.intent.scope, member)?;
        if let Err(dimension) = ResourceVector::carve_within(
            sub.committed(),
            v.intent.reservation,
            sub_ceiling,
            op.as_str(),
        ) {
            return Err(JournalError::BudgetExceeded {
                scope: Box::new(v.intent.scope.clone()),
                vectors: Box::new([v.intent.reservation, sub.committed(), sub_ceiling]),
                dimension,
            });
        }
        let rec = Rec::Reserved { op: op.clone() };
        let change = g.state.transition(&rec)?;
        let line = Line {
            seq: g.seq + 1,
            rec,
        };
        let mut bytes = serde_json::to_vec(&line).map_err(std::io::Error::other)?;
        bytes.push(b'\n');
        g.file.write_all(&bytes)?;
        g.file.sync_data()?;
        g.seq += 1;
        g.state.commit(change);
        Ok(())
    }

    /// Usage of the scope's ops whose intent satisfies `member`.
    pub fn usage_where(
        &self,
        scope: &Scope,
        member: impl Fn(&Intent) -> bool,
    ) -> Result<ScopeUsage, JournalError> {
        self.lock().state.usage_where(scope, member)
    }

    /// Every op whose intent satisfies `member`, in no particular order.
    pub fn views_where(&self, member: impl Fn(&Intent) -> bool) -> Vec<OpView> {
        self.lock()
            .state
            .ops
            .values()
            .filter(|v| member(&v.intent))
            .cloned()
            .collect()
    }

    /// Record that the effect is about to be dispatched. Call BEFORE dispatch.
    pub fn mark_launched(&self, op: &OperationId) -> Result<(), JournalError> {
        self.append(Rec::Launched { op: op.clone() })
    }

    pub fn complete(&self, op: &OperationId, billing: Billing) -> Result<(), JournalError> {
        self.append(Rec::Completed {
            op: op.clone(),
            billing,
        })
    }

    pub fn fail(
        &self,
        op: &OperationId,
        reason: &str,
        billing: Billing,
    ) -> Result<(), JournalError> {
        self.append(Rec::Failed {
            op: op.clone(),
            reason: reason.to_string(),
            billing,
        })
    }

    /// Cancel. `billing` must be `None` for a never-launched op (released) and
    /// `Some` for a launched one (charged or held as liability).
    pub fn cancel(
        &self,
        op: &OperationId,
        reason: &str,
        billing: Option<Billing>,
    ) -> Result<(), JournalError> {
        self.append(Rec::Cancelled {
            op: op.clone(),
            reason: reason.to_string(),
            billing,
        })
    }

    /// Resolve an unknown-cost liability with an accounting receipt. The
    /// operation's STATE does not change: an OutcomeUnknown op stays unknown.
    ///
    /// Idempotent ONLY for the identical receipt (origin, sequence, op and
    /// content). Any other second settlement of the op, or any reuse of the
    /// receipt's `(origin, sequence)` for another op, is refused with
    /// [`JournalError::SettlementConflict`] and the refused receipt is
    /// RECORDED, leaving the op disputed (see the module docs).
    pub fn settle(&self, op: &OperationId, s: Settlement) -> Result<Settle, JournalError> {
        let Settlement {
            origin,
            sequence,
            actual,
        } = s;
        match self.append_inner(Rec::Settled {
            op: op.clone(),
            origin: origin.clone(),
            sequence,
            actual,
        }) {
            Ok(true) => Ok(Settle::Recorded),
            Ok(false) => Ok(Settle::AlreadySettled),
            Err(JournalError::SettlementConflict { op, reason }) => {
                self.append(Rec::SettleConflict {
                    op: op.clone(),
                    origin,
                    sequence,
                    actual,
                    reason: reason.clone(),
                })?;
                Err(JournalError::SettlementConflict { op, reason })
            }
            Err(e) => Err(e),
        }
    }

    /// Attach the terminal outcome document (once) to a terminal op.
    pub fn record_outcome(
        &self,
        op: &OperationId,
        outcome: serde_json::Value,
    ) -> Result<(), JournalError> {
        self.append(Rec::Outcome {
            op: op.clone(),
            outcome,
        })
    }

    pub fn view(&self, op: &OperationId) -> Option<OpView> {
        self.lock().state.ops.get(op).cloned()
    }

    pub fn scope_usage(&self, scope: &Scope) -> Result<ScopeUsage, JournalError> {
        self.lock().state.usage(scope)
    }

    /// Number of records on disk (including the header).
    pub fn len(&self) -> u64 {
        self.lock().seq
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
