//! The Cortex side of the Policy B boundary.
//!
//! Reads one authorization request as JSON on stdin, asks `axon_cortex`, writes one decision as
//! JSON on stdout. That is the whole program.
//!
//! It exists as a separate executable rather than a library MiCode links so that neither repository
//! depends on the other. MiCode knows the protocol; this knows Cortex. Either can be rebuilt,
//! re-versioned or replaced without touching the other, and the experiment records which version
//! decided.
//!
//! ## What crosses, and what deliberately does not
//!
//! In: `action` (a Cortex catalog name), `principal`, `target_path`, `snapshot_id`.
//! Out: `allow`, or `refuse` with Cortex's own reason.
//!
//! The workspace does not cross. Cortex is a control plane; handing it file contents would make it
//! a second filesystem agent, and the decisions it is good at — is this principal permitted to
//! change this path, under a grant still valid for this state — need identity and scope, not bytes.
//!
//! ## The grant is configured, not inferred
//!
//! `--principal`, `--grant-snapshot`, `--write-prefix` (repeatable). An empty prefix list denies
//! everything, matching Cortex's own convention where `""` denies and `"*"` is unrestricted. The
//! grant's snapshot is supplied separately from the request's, which is the point: when the
//! workspace has moved on, they differ and Cortex refuses `StaleSnapshot` — authority does not
//! survive the state it was granted over.
//!
//! **`--grant-snapshot` is required, and its absence is an infrastructure failure.** It used to
//! default to the snapshot carried by the REQUEST. That made the grant's snapshot and the current
//! snapshot equal by construction, so `StaleSnapshot` could never fire and the property this
//! paragraph describes quietly did not hold. The doc above was already written; the code below
//! disagreed with it.
//!
//! Note the asymmetry that made it easy to miss: an empty `--write-prefix` list DENIES everything,
//! while an empty `--grant-snapshot` PERMITTED everything — opposite fail directions for two
//! fields of one grant, decided a dozen lines apart. Both now fail closed.
//!
//! "No authority basis was supplied" is not "the authority is current". Synthesising the second
//! from the first is what turns an authority check into decoration.

use std::io::Read;

use axon_cortex::action::{CheckRef, CompletionClaim, CortexAction, SymbolRef};
use axon_cortex::runner::{EditGrant, Refusal, Runner};
use axon_cortex::WorkspaceSnapshot;

const PROTOCOL_VERSION: u64 = 1;

fn fail(reason: &str) -> ! {
    // Exit non-zero WITHOUT a decision. The client reads that as an infrastructure failure, which
    // is correct: a malformed request means nothing was decided. Printing a refusal here would
    // report a broken adapter as a strict policy.
    eprintln!("{reason}");
    std::process::exit(2)
}

fn main() {
    let mut args = std::env::args().skip(1);
    // No default. The module doc two screens up enumerates this exact class of
    // bug for the other two grant fields and concludes "Both now fail closed";
    // the THIRD field of the same grant still defaulted to `agent`, so a write
    // could be authorised in the name of a principal no operator ever named.
    // `Refusal::WrongPrincipal` was unreachable for any client that guessed
    // the default.
    let mut principal = String::new();
    let mut grant_snapshot = String::new();
    let mut write_prefixes: Vec<String> = Vec::new();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--principal" => {
                principal = args
                    .next()
                    .unwrap_or_else(|| fail("--principal needs a value"))
            }
            "--grant-snapshot" => {
                grant_snapshot = args
                    .next()
                    .unwrap_or_else(|| fail("--grant-snapshot needs a value"));
            }
            "--write-prefix" => {
                write_prefixes.push(
                    args.next()
                        .unwrap_or_else(|| fail("--write-prefix needs a value")),
                );
            }
            other => fail(&format!("unknown argument {other}")),
        }
    }

    // Required. Absence is an infrastructure failure, NOT a permissive default: without it nobody
    // has said which state this authority was issued over, and the only way to proceed would be to
    // assume it is the current one — the assumption that made StaleSnapshot unreachable.
    //
    // `fail` exits non-zero with no decision on stdout, so the client records an infrastructure
    // failure rather than an allow or a refuse. A refusal here would misreport a misconfigured
    // adapter as a strict policy.
    if principal.trim().is_empty() {
        fail(
            "--principal is required: a grant belongs to somebody, and defaulting \
             that to a well-known name makes the wrong-principal refusal \
             unreachable for anyone who guesses it.",
        );
    }

    if grant_snapshot.trim().is_empty() {
        fail(
            "--grant-snapshot is required: the state the grant was issued over must be supplied \
             independently of the request, or staleness cannot be evaluated. Refusing to infer it \
             from the request's snapshot, which would make StaleSnapshot unreachable.",
        );
    }

    // BOUNDED. An authority request is a handful of short fields; reading an
    // arbitrarily large one buffers it whole, and the failure mode is an
    // allocation abort — a non-2 exit with no decision and no message, which
    // is the one outcome this program is built to never produce.
    const MAX_REQUEST: u64 = 1 << 20;
    let mut body = String::new();
    if std::io::stdin()
        .take(MAX_REQUEST + 1)
        .read_to_string(&mut body)
        .is_err()
    {
        fail("could not read the request");
    }
    if body.len() as u64 > MAX_REQUEST {
        fail(&format!(
            "request is larger than {MAX_REQUEST} bytes; an authority request \
             is a handful of short fields and nothing was decided"
        ));
    }
    // STRICTLY. This is the only place untrusted JSON enters the system, and
    // `parse_strict` exists for exactly it — yet it had no production caller
    // and this line used `serde_json` directly.
    //
    // `serde_json` resolves a duplicate key by last-wins, so a request can be
    // read one way by a human and decided another way by the machine.
    // Measured, before this change:
    //
    //   {"principal":"intruder","principal":"agent",
    //    "target_path":"../../etc/shadow","target_path":"ok.ax", …}
    //     → {"decision":"allow"}
    //
    // A reviewer scanning that request sees `intruder` and a traversal path.
    // The decision was made on `agent` and `ok.ax`. Ambiguous input must be
    // refused, not silently resolved — and refused as an INFRASTRUCTURE
    // failure (exit 2, no decision), because nothing was decided.
    let req: serde_json::Value = match axon_cortex::parse_strict(body.trim()) {
        Ok(v) => v,
        Err(e) => fail(&format!("malformed request: {e}")),
    };

    match req["protocol_version"].as_u64() {
        Some(v) if v == PROTOCOL_VERSION => {}
        other => fail(&format!(
            "protocol version mismatch: adapter {PROTOCOL_VERSION}, client {other:?}"
        )),
    }

    let action = req["action"].as_str().unwrap_or_else(|| fail("no action"));
    let target = req["target_path"]
        .as_str()
        .unwrap_or_else(|| fail("no target_path"));
    let req_principal = req["principal"]
        .as_str()
        .unwrap_or_else(|| fail("no principal"));
    let snapshot_id = req["snapshot_id"]
        .as_str()
        .unwrap_or_else(|| fail("no snapshot_id"));

    // The state the decision is made against, named by the request. Only the identity is needed:
    // Cortex compares it with the grant's, and the difference is the whole `StaleSnapshot` check.
    let current = WorkspaceSnapshot {
        snapshot_id: snapshot_id.to_string(),
        parent_snapshot_id: None,
        files: Vec::new(),
        observation_scope: vec![target.to_string()],
    };
    let grant = EditGrant {
        grant_id: "policy-b".to_string(),
        principal: principal.clone(),
        // NEVER synthesised from the request. See the module docs: deriving the grant's basis from
        // the thing it is supposed to be checked against is a tautology, not a default.
        snapshot_id: grant_snapshot,
        write_prefixes,
    };

    let mut runner = Runner::new(
        std::path::PathBuf::from("axon"),
        std::env::current_dir().unwrap_or_default(),
    );
    // The STRING EDGE. A request arrives as JSON from another process, so the
    // action is a string here and nowhere deeper: it is parsed into a typed
    // `CortexAction` before any authority question is asked. That is the only
    // place an unknown action can appear, and the only place `NotInCatalog`
    // still fires — past this point the type is the catalog.
    //
    // `patch_symbol_body` REQUIRES a symbol. The protocol did not carry one, so
    // the adapter was authorising an edit without knowing what it edited: the
    // grant check could say the path was in range while the request named no
    // symbol at all. Refusing that is not new strictness, it is the question
    // finally being answerable.
    let symbol = req["symbol"].as_str();
    let typed = match action {
        // A read needs no symbol to be coherent, and that asymmetry with
        // `patch_symbol_body` is deliberate: an EDIT that does not name what
        // it edits cannot be authorised, while inspecting or checking a path
        // is a complete request on its own. A review suggested requiring one
        // everywhere; the suite said otherwise, in a test whose comment states
        // the contrast in as many words. The tests were right.
        "inspect" => Ok(CortexAction::Inspect {
            target: SymbolRef {
                path: target.to_string(),
                symbol: symbol.unwrap_or("").to_string(),
            },
        }),
        "run_check" => Ok(CortexAction::RunCheck {
            check: CheckRef {
                name: symbol.unwrap_or("").to_string(),
                path: target.to_string(),
            },
        }),
        "claim_done" => Ok(CortexAction::ClaimDone {
            claim: CompletionClaim {
                done: true,
                // The claim's rationale is evidence, not an input to the
                // verdict. An absent one is recorded as absent rather than
                // invented.
                rationale: req["rationale"]
                    .as_str()
                    .unwrap_or("<none given>")
                    .to_string(),
            },
        }),
        "patch_symbol_body" => match symbol {
            Some(s) if !s.is_empty() => Ok(CortexAction::PatchSymbolBody {
                symbol: SymbolRef {
                    path: target.to_string(),
                    symbol: s.to_string(),
                },
                // The body does not enter the authority decision — Cortex
                // checks the principal, the grant's state and the path, none
                // of which depend on what is being written. Refusing for its
                // absence would answer a question nobody asked. What the
                // decision DOES cover is stated in `basis`.
                proposed_body: req["proposed_body"].as_str().unwrap_or("").to_string(),
            }),
            _ => Err(Refusal::NotInCatalog(
                "patch_symbol_body without a `symbol`: an edit that does not \
                 name what it edits cannot be authorised"
                    .to_string(),
            )),
        },
        other => Err(Refusal::NotInCatalog(other.to_string())),
    };

    // `typed` is bound OUTSIDE the match so the action outlives the decision.
    // That is the Authorized witness doing its job rather than an inconvenience:
    // it borrows the action, so the thing authorized cannot be dropped while a
    // proof of its authorization is still held, and cannot be swapped for a
    // copy that drifted.
    let decision = match &typed {
        Ok(a) => runner
            .authorize_action(a, Some(&grant), req_principal, &current)
            .map(|_authorized| ()),
        Err(refusal) => Err(refusal.clone()),
    };

    let out = match decision {
        // Mapped to () above: this adapter DECIDES, it does not execute. That
        // boundary is the point — Cortex answers "may I?", the caller acts.
        // WHAT WAS CHECKED, not merely the verdict.
        //
        // Cortex answers Ok immediately for any action that needs no write
        // authority, BEFORE the principal, staleness, traversal and
        // policy-file checks. So `inspect` with a principal the grant does not
        // belong to, a snapshot that never existed, and a `..` path returned
        // the same four bytes as a granted write — measured. "The grant
        // authorised this" and "no authority question was asked" were the same
        // token, which is this crate's own absent-vs-passed collapse sitting
        // in its security boundary.
        Ok(()) => serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "decision": "allow",
            "basis": if typed.as_ref().map(|a| a.requires_write_authority()).unwrap_or(false) {
                "granted: the principal, the grant's state and the path were all checked"
            } else {
                "no write authority required: this action cannot modify the \
                 workspace, so no grant was consulted and the principal, \
                 snapshot and path in this request were NOT evaluated"
            },
            "action": typed.as_ref().map(|a| a.name()).unwrap_or("<unparsed>"),
        }),
        Err(refusal) => serde_json::json!({
            "protocol_version": PROTOCOL_VERSION,
            "decision": "refuse",
            // Cortex's own words, not a re-description. A refusal that loses its reason cannot be
            // distinguished from a crash by anything downstream.
            "reason": refusal.to_string(),
        }),
    };
    println!("{out}");
}
