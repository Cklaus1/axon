//! The store's ONE source of authority: a hash-chained, append-only ledger.
//!
//! `ledger.jsonl` holds every authority-relevant event: plan registration and
//! freeze, EVO hypotheses, incumbent baselines, evaluations, admissions,
//! pointer transitions and revocations. Each line carries `seq` (1, 2, …) and `prev`, the `cl22:`
//! digest of the previous line's entry (the null sentinel for the first).
//! `ledger.head` records `{seq, entry_ref}` of the last acknowledged entry.
//!
//! Everything else is either content-addressed data (`policies/`,
//! `admissions/`, `evaluations/`, `plans/`, `candidate-sets/`,
//! `task-manifests/`, …, each re-checked against the digest the ledger
//! names) or a PROJECTION of the ledger (`scopes/<t>/<f>/pointer.json`).
//!
//! # What the chain does and does not detect (the threat model, precisely)
//!
//! The chain is `cl22:` sha256 over each entry, including `prev`. It detects
//! ACCIDENTAL damage and edits by a writer who does not rewrite the chain
//! consistently:
//!
//! * a projection that differs from the replay of the ledger (edited
//!   `pointer.json`, injected history) is corruption, exit 2 (A7b, F3b);
//! * an edited, reordered or duplicated ledger line breaks the chain, exit 2;
//! * truncating the ledger while the head is LEFT UNTOUCHED is detected
//!   (the head names a later entry); deleting the head alone is detected;
//! * deleting ledger AND head while any ledger-dependent directory remains,
//!   or while `ledger.anchor` remains, is detected (A7c, R3 partial) — see
//!   below;
//! * a freeze cannot be undone by deleting a file (G6).
//!
//! ## Keyed mode (D-015): the operator key closes F1, F2 and R2
//!
//! When the operator sets `AXON_ATTEST_KEY` (hex, >= 16 bytes; the same key
//! and key rule `axon-vm` attests under), the ledger is authenticated the way
//! `axon-audit`'s keyed chain is, with the same primitive
//! (`axon_attest::hmac_sha256`) and the same two-part shape:
//!
//! * every entry carries `mac` = HMAC(k, "axon-loop entry v1" ‖ the `cl22:`
//!   digest of the entry without its `mac`) — `axon-audit`'s keyed
//!   `entry_hash`. The per-entry MAC is not redundant with the head: an
//!   appended entry the head does not yet name is ROLLED FORWARD as a crash
//!   tail, so without it one forged line would be adopted and re-headed;
//! * `ledger.head` carries `mac` = HMAC(k, "axon-loop head v1" ‖ seq ‖
//!   entry_ref) — `axon-audit`'s `compute_tip` over (count, last hash). A
//!   prefix of a valid keyed chain is itself valid, so truncation is only
//!   detectable because the COUNT is authenticated.
//!
//! `k` is derived as HMAC(operator key, "axon-loop ledger key v1"), so these
//! MACs are never interchangeable with any other use of the operator key.
//! The KEY decides the verification, never the files: a keyed opener refuses
//! an entry or head without a valid `mac`, and an unkeyed opener refuses a
//! ledger that has one (exit 2) rather than silently verifying less. Under a
//! key the following are DETECTED (exit 2), and pinned by tests:
//!
//! * F1 — a well-chained forged entry appended, head recomputed;
//! * F2 — a hand-written evaluation record plus its journalled ledger line;
//! * R2 — truncation plus a head rewritten to the new last entry;
//! * a forged line appended with the head left alone (the roll-forward path).
//!
//! What stays OUT OF MODEL, precisely:
//!
//! * **No key configured.** There is deliberately no ephemeral key: a
//!   per-process key cannot verify what the previous process wrote, and would
//!   only look like authentication. Unkeyed, F1/F2/R2 are undetectable, as
//!   before; the store says what it is.
//! * **Whoever holds the key**, or can read where the operator keeps it (the
//!   environment of an `axon-loop` process), can mint any ledger. The key
//!   authenticates "written by a key holder", not which key holder.
//! * **R1 — restoring an older, fully consistent keyed store** (ledger, head
//!   and projections copied from an earlier moment), or truncating to an
//!   earlier point and restoring the genuine head file that was current then.
//!   Every byte is genuine; only freshness is wrong, and freshness needs a
//!   MONOTONIC witness outside the store. Candidate: append each head to an
//!   external `axon-audit` keyed ledger and refuse a store head older than the
//!   last witnessed one. Not done: `axon-audit`'s `Ledger` takes no
//!   inter-process lock, so concurrent writers would corrupt the witness
//!   chain. **R1 is OPEN.**
//! * **R3/NS6c** — deleting ledger, head, anchor and every dependent
//!   directory is indistinguishable from a fresh store (same witness gap).
//! * Replay re-checks linkage and authenticity; it does not re-validate event
//!   SEMANTICS. A key holder's well-formed but wrong event is served.
//!
//! //! `ledger.anchor` (R3, best effort): written once with the first entry's
//! ref and never rewritten. Its presence with no ledger is corruption, so a
//! scope's fence cannot be silently reissued by deleting the ledger and the
//! dependent directories; deleting the anchor as well is still undetectable
//! (in-store, unkeyed — same limit as above).
//!
//! Every [`Tx`] holds the store's single exclusive lock for its whole life,
//! verifies the chain, and rolls a crash forward: an entry appended before the
//! head was updated was decided and made durable, so its projection and head
//! are completed.

use crate::error::{LoopError, Result};
use crate::evo::Hypothesis;
use crate::pointer::{PointerFile, PointerRecord, Revocation};
use crate::store::{LedgerKey, Lock, Store, LEDGER_KEY_ENV};
use axon_loop_contracts::{AuthorityEpoch, PolicyTransition, Ref, Scope};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::PathBuf;

crate::record_tag!(EntrySchema, "axon.loop.ledger/1");
crate::record_tag!(HeadSchema, "axon.loop.ledger-head/1");

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    PlanRegistered {
        experiment_id: String,
        scope: Scope,
        plan_ref: Ref,
    },
    Freeze {
        experiment_id: String,
        scope: Scope,
        plan_ref: Ref,
        incumbent_policy_ref: Ref,
        candidate_policy_ref: Ref,
        authority_epoch: AuthorityEpoch,
        /// ADR-001 D3: fixed AT FREEZE from whether the operator lists the
        /// scope as protected, so it cannot be chosen after outcomes. The
        /// pilot plan document (a vendored v0.22 contract) is not changed.
        /// Absent for a development freeze, so older entries are unchanged.
        #[serde(
            default,
            skip_serializing_if = "crate::plan::EvaluationClass::is_development"
        )]
        evaluation_class: crate::plan::EvaluationClass,
    },
    Hypothesis {
        scope: Scope,
        hypothesis: Box<Hypothesis>,
    },
    Baseline {
        scope: Scope,
        baseline_ref: Ref,
        policy_ref: Ref,
    },
    Evaluation {
        scope: Scope,
        experiment_id: String,
        evaluation_ref: Ref,
        freeze_seq: u64,
        authority_epoch: AuthorityEpoch,
    },
    /// An admission decision AND its hypothesis verdict, in ONE entry: a
    /// crash leaves the store before or after the decision, never with a
    /// decision whose verdict is missing from hypothesis history (CW1).
    Admission {
        scope: Scope,
        experiment_id: String,
        admission_ref: Ref,
        target_policy_ref: Ref,
        decision: crate::admission::Decision,
        mechanism_test: bool,
    },
    Transition {
        transition_ref: Ref,
        transition: Box<PolicyTransition>,
        prior: Box<PointerRecord>,
        result: Box<PointerRecord>,
    },
    Revocation {
        scope: Scope,
        revocation: Revocation,
    },
    /// A MiCode closed-loop episode sidecar joined to this store's policy and
    /// the task's context receipt (`crate::intake`). Records evidence; moves
    /// no pointer and grants nothing.
    EpisodeIntake {
        scope: Scope,
        intake: Box<crate::intake::IntakeRecord>,
    },
    /// ADR-001 §5: an accepted safety finding about one intaken trial
    /// attempt (`crate::safety`). Evidence only; evaluation reads it.
    SafetyReport {
        scope: Scope,
        report: Box<crate::safety::SafetyReport>,
        /// A clearance: the monitor key its signature verified under, so a
        /// re-derivation can require that key to still be the monitor's.
        /// Absent on violations and on older entries (bytes unchanged).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        key_id: Option<String>,
    },
    /// A trusted admitter registered the eligible candidate LIST behind a
    /// `candidate_set_ref` (bytes in `candidate-sets/`; see `crate::candidates`).
    CandidateSet {
        scope: Scope,
        candidate_set_ref: Ref,
    },
    /// A trusted admitter registered an experiment's task manifest (bytes in
    /// `task-manifests/`; see `crate::tasks`).
    TaskManifest { scope: Scope, manifest_ref: Ref },
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub schema: EntrySchema,
    pub seq: u64,
    pub prev: Ref,
    pub recorded_ms: u64,
    pub event: Event,
    /// Keyed mode only (D-015): hex HMAC over the digest of this entry
    /// without `mac`. Absent in an unkeyed store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Head {
    pub schema: HeadSchema,
    pub seq: u64,
    pub entry_ref: Ref,
    /// Keyed mode only (D-015): hex HMAC over `(seq, entry_ref)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mac: Option<String>,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Length-checked, data-independent comparison of two MAC strings.
fn mac_eq(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

fn entry_mac(k: &LedgerKey, e: &Entry) -> Result<String> {
    let bare = Entry {
        mac: None,
        ..e.clone()
    };
    let d = axon_loop_contracts::digest(&bare)?;
    let mut data = b"axon-loop entry v1\0".to_vec();
    data.extend_from_slice(d.as_str().as_bytes());
    Ok(hex(&k.mac(&data)))
}

fn head_mac(k: &LedgerKey, seq: u64, entry_ref: &Ref) -> String {
    let mut data = b"axon-loop head v1\0".to_vec();
    data.extend_from_slice(&seq.to_le_bytes());
    data.extend_from_slice(entry_ref.as_str().as_bytes());
    hex(&k.mac(&data))
}

/// The key decides; the file's claim never does.
fn check_entry_mac(k: Option<&LedgerKey>, e: &Entry) -> Result<()> {
    match (k, &e.mac) {
        (Some(k), Some(m)) if mac_eq(m, &entry_mac(k, e)?) => Ok(()),
        (Some(_), _) => Err(corrupt(format!(
            "ledger entry seq {} is not authenticated under the operator key \
             (forged, or written without {LEDGER_KEY_ENV})",
            e.seq
        ))),
        (None, Some(_)) => Err(corrupt(format!(
            "ledger is keyed but no key is configured: set {LEDGER_KEY_ENV}"
        ))),
        (None, None) => Ok(()),
    }
}

fn check_head_mac(k: Option<&LedgerKey>, h: &Head) -> Result<()> {
    match (k, &h.mac) {
        (Some(k), Some(m)) if mac_eq(m, &head_mac(k, h.seq, &h.entry_ref)) => Ok(()),
        (Some(_), _) => Err(corrupt(
            "ledger head is not authenticated under the operator key \
             (rewritten, or written without the key)",
        )),
        (None, Some(_)) => Err(corrupt(format!(
            "ledger head is keyed but no key is configured: set {LEDGER_KEY_ENV}"
        ))),
        (None, None) => Ok(()),
    }
}

/// A ledger transaction: the store lock, plus the verified ledger.
/// Test seam (B280, G13-r22-restart-matrix): called inside [`Tx::append`] at
/// `"after_ledger_append"` (the entry is durable; nothing projected) and
/// `"after_projection"` (projected; the head not yet written), so a crash
/// child can die in exactly those windows and a real restart be tested from
/// what it leaves. Never set in production; settable once per process.
static FAULT_HOOK: std::sync::OnceLock<fn(&'static str)> = std::sync::OnceLock::new();

/// Install the [`FAULT_HOOK`] (tests only). Returns false if one was set.
pub fn set_fault_hook(h: fn(&'static str)) -> bool {
    FAULT_HOOK.set(h).is_ok()
}

pub struct Tx<'s> {
    pub store: &'s Store,
    _lock: Lock,
    entries: Vec<Entry>,
    refs: Vec<Ref>,
}

fn ledger_path(s: &Store) -> PathBuf {
    s.root().join("ledger.jsonl")
}
fn head_path(s: &Store) -> PathBuf {
    s.root().join("ledger.head")
}
fn anchor_path(s: &Store) -> PathBuf {
    s.root().join("ledger.anchor")
}

crate::record_tag!(AnchorSchema, "axon.loop.ledger-anchor/1");

/// Written once, with the first ledger entry; never rewritten (see module docs).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Anchor {
    pub schema: AnchorSchema,
    pub first_entry_ref: Ref,
}

fn corrupt(msg: impl Into<String>) -> LoopError {
    LoopError::Io(format!("store corrupt: {}", msg.into()))
}

/// Directories that only ever hold ledger-dependent state.
const DEPENDENT: &[&str] = &[
    "plans",
    "admissions",
    "evaluations",
    "baselines",
    "scopes",
    "episodes",
    "contexts",
    "candidate-sets",
    "task-manifests",
];

impl<'s> Tx<'s> {
    pub fn begin(store: &'s Store) -> Result<Tx<'s>> {
        let lock = store.lock_root(false)?;
        let entries: Vec<Entry> = store.read_jsonl(&ledger_path(store))?;
        let head: Option<Head> = store.read_json(&head_path(store))?;

        let key = store.ledger_key();
        let mut refs = Vec::with_capacity(entries.len());
        let mut prev = crate::null_policy_ref();
        for (i, e) in entries.iter().enumerate() {
            check_entry_mac(key, e)?;
            if e.seq != i as u64 + 1 {
                return Err(corrupt(format!("ledger seq {} at line {}", e.seq, i + 1)));
            }
            if e.prev != prev {
                return Err(corrupt(format!("ledger chain broken at seq {}", e.seq)));
            }
            prev = axon_loop_contracts::digest(e)?;
            refs.push(prev.clone());
        }
        let mut tx = Tx {
            store,
            _lock: lock,
            entries,
            refs,
        };
        let n = tx.entries.len() as u64;
        match head {
            Some(h) => {
                check_head_mac(key, &h)?;
                if h.seq > n {
                    return Err(corrupt(format!(
                        "ledger truncated: head at seq {}, ledger has {n}",
                        h.seq
                    )));
                }
                if h.seq == 0 || tx.refs[h.seq as usize - 1] != h.entry_ref {
                    return Err(corrupt("ledger head does not match its entry"));
                }
                if n > h.seq + 1 {
                    return Err(corrupt("ledger more than one entry ahead of its head"));
                }
                if n == h.seq + 1 {
                    tx.roll_forward()?;
                }
            }
            None if n == 1 => tx.roll_forward()?,
            None if n > 1 => return Err(corrupt("ledger head missing")),
            None => {
                if std::fs::symlink_metadata(anchor_path(store)).is_ok() {
                    return Err(corrupt(
                        "ledger missing but ledger.anchor exists: this store had a ledger; \
                         a fence is never reissued",
                    ));
                }
                for d in DEPENDENT {
                    let p = store.root().join(d);
                    if std::fs::symlink_metadata(&p).is_ok() {
                        return Err(corrupt(format!(
                            "ledger missing but {d}/ exists: a fence is never reissued"
                        )));
                    }
                }
            }
        }
        if let Some(a) = store.read_json::<Anchor>(&anchor_path(store))? {
            if tx.refs.first() != Some(&a.first_entry_ref) {
                return Err(corrupt(
                    "ledger.anchor names a different first entry: the ledger was replaced",
                ));
            }
        }
        tx.check_projections()?;
        Ok(tx)
    }

    /// Complete an appended-but-unacknowledged entry: projection FIRST, head
    /// LAST. The head is the commit acknowledgement, so any crash leaves
    /// either `ledger == head + 1` (this runs again, idempotently) or a fully
    /// committed entry — never a head whose projection is stale.
    fn roll_forward(&mut self) -> Result<()> {
        let last = self.entries.last().expect("nonempty").clone();
        self.project(&last)?;
        self.write_head()
    }

    fn write_head(&self) -> Result<()> {
        // The anchor is written BEFORE the first head (so a committed ledger
        // always has one) and never rewritten.
        if std::fs::symlink_metadata(anchor_path(self.store)).is_err() {
            self.store.write_json(
                &anchor_path(self.store),
                &Anchor {
                    schema: AnchorSchema,
                    first_entry_ref: self.refs[0].clone(),
                },
            )?;
        }
        let seq = self.entries.len() as u64;
        let entry_ref = self.refs.last().expect("nonempty").clone();
        let h = Head {
            schema: HeadSchema,
            seq,
            mac: self
                .store
                .ledger_key()
                .map(|k| head_mac(k, seq, &entry_ref)),
            entry_ref,
        };
        self.store.write_json(&head_path(self.store), &h)
    }

    fn project(&self, e: &Entry) -> Result<()> {
        if let Event::Transition {
            transition, result, ..
        } = &e.event
        {
            let f = PointerFile {
                pointer: (**result).clone(),
                ledger_seq: e.seq,
                entry_ref: self.refs[e.seq as usize - 1].clone(),
            };
            self.store.write_json(
                &crate::pointer::pointer_path(self.store, &transition.scope),
                &f,
            )?;
        }
        Ok(())
    }

    /// Every `pointer.json` must be exactly the projection of the ledger.
    fn check_projections(&self) -> Result<()> {
        let mut scopes: BTreeSet<(String, String)> = BTreeSet::new();
        let root = self.store.root().join("scopes");
        if let Ok(rd) = std::fs::read_dir(&root) {
            for t in rd {
                let t = t?;
                if t.file_type()?.is_symlink() {
                    return Err(corrupt(format!("symlink {}", t.path().display())));
                }
                for f in std::fs::read_dir(t.path())? {
                    let f = f?;
                    if f.file_type()?.is_symlink() {
                        return Err(corrupt(format!("symlink {}", f.path().display())));
                    }
                    scopes.insert((
                        t.file_name().to_string_lossy().into_owned(),
                        f.file_name().to_string_lossy().into_owned(),
                    ));
                }
            }
        }
        for e in &self.entries {
            if let Event::Transition { transition, .. } = &e.event {
                scopes.insert((
                    transition.scope.tenant_id.to_string(),
                    transition.scope.task_family.to_string(),
                ));
            }
        }
        for (t, f) in scopes {
            let scope = Scope {
                tenant_id: axon_loop_contracts::TenantId::new(t.as_str())
                    .map_err(|_| corrupt(format!("bad scope dir {t}")))?,
                task_family: axon_loop_contracts::TaskFamily::new(f.as_str())
                    .map_err(|_| corrupt(format!("bad scope dir {f}")))?,
            };
            let on_disk: Option<PointerFile> = self
                .store
                .read_json(&crate::pointer::pointer_path(self.store, &scope))?;
            let expected = self.last_transition(&scope);
            match (on_disk, expected) {
                (None, None) => {}
                (Some(_), None) => {
                    return Err(corrupt(format!(
                        "pointer.json for {t}/{f} has no transition in the ledger"
                    )))
                }
                (None, Some(_)) => {
                    return Err(corrupt(format!("pointer.json for {t}/{f} is missing")))
                }
                (Some(d), Some((seq, rec))) => {
                    if d.pointer != rec
                        || d.ledger_seq != seq
                        || d.entry_ref != self.refs[seq as usize - 1]
                    {
                        return Err(corrupt(format!(
                            "pointer.json for {t}/{f} is not the ledger's projection"
                        )));
                    }
                }
            }
        }
        Ok(())
    }

    fn last_transition(&self, scope: &Scope) -> Option<(u64, PointerRecord)> {
        self.entries.iter().rev().find_map(|e| match &e.event {
            Event::Transition {
                transition, result, ..
            } if &transition.scope == scope => Some((e.seq, (**result).clone())),
            _ => None,
        })
    }

    /// Append an event: ledger line (fsync), projection, then head.
    pub fn append(&mut self, event: Event) -> Result<u64> {
        let fault = |stage: &'static str| {
            if let Some(h) = FAULT_HOOK.get() {
                h(stage)
            }
        };
        let seq = self.entries.len() as u64 + 1;
        let mut e = Entry {
            schema: EntrySchema,
            seq,
            prev: self
                .refs
                .last()
                .cloned()
                .unwrap_or_else(crate::null_policy_ref),
            recorded_ms: crate::now_ms(),
            event,
            mac: None,
        };
        if let Some(k) = self.store.ledger_key() {
            e.mac = Some(entry_mac(k, &e)?);
        }
        let r = axon_loop_contracts::digest(&e)?;
        self.store.append_jsonl(&ledger_path(self.store), &e)?;
        fault("after_ledger_append");
        self.entries.push(e.clone());
        self.refs.push(r);
        self.project(&e)?;
        fault("after_projection");
        self.write_head()?;
        Ok(seq)
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// The seq the next appended event will get.
    pub fn next_seq(&self) -> u64 {
        self.entries.len() as u64 + 1
    }

    /// The scope's current pointer (replay of the ledger).
    pub fn pointer(&self, scope: &Scope) -> PointerRecord {
        self.last_transition(scope)
            .map(|(_, r)| r)
            .unwrap_or_else(|| PointerRecord::initial(scope))
    }

    pub fn is_revoked(&self, scope: &Scope, policy: &Ref) -> bool {
        self.revocations(scope)
            .iter()
            .any(|r| &r.policy_ref == policy)
    }

    pub fn revocations(&self, scope: &Scope) -> Vec<Revocation> {
        self.entries
            .iter()
            .filter_map(|e| match &e.event {
                Event::Revocation {
                    scope: s,
                    revocation,
                } if s == scope => Some(revocation.clone()),
                _ => None,
            })
            .collect()
    }

    /// `(seq, event)` of the experiment's freeze, if any.
    pub fn freeze_of(&self, experiment_id: &str) -> Option<(u64, &Event)> {
        self.entries.iter().find_map(|e| match &e.event {
            Event::Freeze {
                experiment_id: x, ..
            } if x == experiment_id => Some((e.seq, &e.event)),
            _ => None,
        })
    }

    pub fn latest_registration(&self, experiment_id: &str) -> Option<&Event> {
        self.entries.iter().rev().find_map(|e| match &e.event {
            Event::PlanRegistered {
                experiment_id: x, ..
            } if x == experiment_id => Some(&e.event),
            _ => None,
        })
    }
}

impl Tx<'_> {
    /// `(seq, event)` of the ledger entry that recorded `admission_ref`.
    pub fn admission_event(&self, admission_ref: &Ref) -> Option<(u64, &Event)> {
        self.entries.iter().find_map(|e| match &e.event {
            Event::Admission {
                admission_ref: a, ..
            } if a == admission_ref => Some((e.seq, &e.event)),
            _ => None,
        })
    }

    pub fn evaluation_event(&self, evaluation_ref: &Ref) -> Option<(u64, &Event)> {
        self.entries.iter().find_map(|e| match &e.event {
            Event::Evaluation {
                evaluation_ref: a, ..
            } if a == evaluation_ref => Some((e.seq, &e.event)),
            _ => None,
        })
    }

    pub fn baseline_of(&self, scope: &Scope) -> Option<(Ref, Ref)> {
        self.entries.iter().find_map(|e| match &e.event {
            Event::Baseline {
                scope: s,
                baseline_ref,
                policy_ref,
            } if s == scope => Some((baseline_ref.clone(), policy_ref.clone())),
            _ => None,
        })
    }

    pub fn recorded_ms(&self, seq: u64) -> u64 {
        self.entries[seq as usize - 1].recorded_ms
    }

    /// The scope's hypothesis history, optionally only entries before `upto`.
    /// The scope's hypothesis history, optionally only entries before `upto`.
    /// Verdicts are read from `admission` entries (one entry per decision).
    pub fn hypotheses(&self, scope: &Scope, upto: Option<u64>) -> Vec<Hypothesis> {
        self.entries
            .iter()
            .filter(|e| upto.is_none_or(|u| e.seq < u))
            .filter_map(|e| match &e.event {
                Event::Hypothesis {
                    scope: s,
                    hypothesis,
                } if s == scope => Some((**hypothesis).clone()),
                Event::Admission {
                    scope: s,
                    admission_ref,
                    target_policy_ref,
                    decision,
                    mechanism_test,
                    ..
                } if s == scope => Some(Hypothesis::Verdict {
                    schema: crate::evo::HypothesisSchema,
                    candidate_policy_ref: target_policy_ref.clone(),
                    verdict: (*decision).into(),
                    admission_ref: admission_ref.clone(),
                    mechanism_test: *mechanism_test,
                    decided_ms: e.recorded_ms,
                }),
                _ => None,
            })
            .collect()
    }
}

impl Tx<'_> {
    pub fn candidate_set_event(&self, scope: &Scope, r: &Ref) -> bool {
        self.entries.iter().any(|e| {
            matches!(&e.event, Event::CandidateSet { scope: s, candidate_set_ref } if s == scope && candidate_set_ref == r)
        })
    }
}

impl Tx<'_> {
    pub fn task_manifest_event(&self, scope: &Scope, r: &Ref) -> bool {
        self.entries.iter().any(|e| {
            matches!(&e.event, Event::TaskManifest { scope: s, manifest_ref } if s == scope && manifest_ref == r)
        })
    }

    /// `(seq, evaluation_ref)` of every evaluation journalled for `experiment_id`.
    pub fn evaluations_of(&self, experiment_id: &str) -> Vec<(u64, Ref)> {
        self.entries
            .iter()
            .filter_map(|e| match &e.event {
                Event::Evaluation {
                    experiment_id: x,
                    evaluation_ref,
                    ..
                } if x == experiment_id => Some((e.seq, evaluation_ref.clone())),
                _ => None,
            })
            .collect()
    }

    /// Admission refs journalled for `experiment_id`.
    pub fn admissions_of(&self, experiment_id: &str) -> Vec<Ref> {
        self.entries
            .iter()
            .filter_map(|e| match &e.event {
                Event::Admission {
                    experiment_id: x,
                    admission_ref,
                    ..
                } if x == experiment_id => Some(admission_ref.clone()),
                _ => None,
            })
            .collect()
    }

    /// Every journalled evaluation of `scope`, by ref.
    pub fn evaluations_in(&self, scope: &Scope) -> Vec<Ref> {
        self.entries
            .iter()
            .filter_map(|e| match &e.event {
                Event::Evaluation {
                    scope: s,
                    evaluation_ref,
                    ..
                } if s == scope => Some(evaluation_ref.clone()),
                _ => None,
            })
            .collect()
    }
}
