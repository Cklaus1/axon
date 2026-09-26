//! `axon-loop` — drive the v0.22 closed loop over files/JSON.
//!
//! Every verb reads at most ONE JSON document (`--in FILE`, or stdin when
//! `--in` is `-`) and writes ONE JSON document to stdout on success. On
//! failure stdout is empty and stderr carries
//! `{"schema":"axon.loop.error/1","error":<kind>,"exit_code":N,"message":…}`.
//!
//! Exit codes: 0 ok · 2 usage/io/corrupt store (incl. a pointer projection
//! that differs from the ledger, a broken ledger chain, a symlink in the
//! store) · 3 malformed input (strict
//! parse) · 4 refused (rules; nothing written) · 5 CAS conflict (stale epoch /
//! wrong expected policy; nothing written) · 6 paused (no usable active
//! policy) · 7 plan not ready (unset operator fields / not approved / not
//! frozen).
//!
//! ```text
//! axon-loop --store DIR pointer resolve    --tenant T --family F
//! axon-loop --store DIR pointer show       --tenant T --family F
//! axon-loop --store DIR pointer transition --in transition.json
//! axon-loop --store DIR pointer revoke     --tenant T --family F --policy REF --reason REF --issuer ID
//! axon-loop --store DIR pointer baseline   --in baseline.json   (incumbent-of-record; activate from paused)
//! axon-loop --store DIR tasks   put        --in task-manifest.json   (trusted admitter; the task list behind a plan's task_manifest_ref)
//! axon-loop --store DIR candidates put     --in candidate-set.json   (trusted admitter; the list behind a candidate_set_ref)
//! axon-loop --store DIR policy  put        --in policy.json      (shortlist must be within a registered candidate list)
//! axon-loop --store DIR plan    register   --in plan.json
//! axon-loop --store DIR plan    freeze     --experiment ID
//! axon-loop --store DIR plan    show       --experiment ID
//! axon-loop --store DIR evo     propose    --in evo-request.json
//! axon-loop --store DIR evl     evaluate   --in evl-request.json
//! axon-loop --store DIR admit              --in admit-request.json
//! axon-loop             tel     summarize  --in tel-request.json
//!                       (optional `price_schedule: {ref, document}` pins the
//!                       schedule every usage/request must name — G10; optional
//!                       `fabric_attempts: [{request, receipt}]` joins Fabric
//!                       receipts per attempt ref, execution cost unknown — D10)
//! axon-loop --store DIR intake  episode    --in sidecar.json --context FILE|DIR
//!                                          --ack FILE|DIR [--projection FILE] [--source-episode FILE]
//! ```
//!
//! `intake episode`: `--context` may name MiCode's `context/` directory, in
//! which case the file is the one the sidecar's `context_ref` names. `--ack`
//! may name MiCode's `policy-ack/` directory: the acknowledgement is found BY
//! CONTENT (exactly one distinct ack pinning the episode's policy_ref over its
//! candidate_set_ref), because `projection_ref` names a PolicyProjection, not
//! the ack (G6). A non-null `projection_ref` requires `--projection`.

use axon_loop::error::LoopError;
use axon_loop::price::PinnedSchedule;
use axon_loop::store::{contract_from_value, strict_record, Store};
use axon_loop::{admission, evl, evo, intake, plan, pointer, tel};
use axon_loop_contracts::{
    parse, LoopEpisode, OpaqueRef, PolicyEnvelope, PolicyTransition, Ref, Refusal, Scope,
    TaskFamily, TenantId,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::Read;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TelRequest {
    schema: String,
    episodes: Vec<Value>,
    /// The schedule every usage and request must name, pinned by content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    price_schedule: Option<TelSchedule>,
    /// Fabric attempts to join per attempt ref. Requires `price_schedule`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fabric_attempts: Option<Vec<TelAttempt>>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TelSchedule {
    #[serde(rename = "ref")]
    reference: Ref,
    document: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TelAttempt {
    request: Value,
    receipt: Value,
}

struct Args {
    store: Option<String>,
    words: Vec<String>,
    flags: BTreeMap<String, String>,
}

fn parse_args() -> Result<Args, LoopError> {
    let mut it = std::env::args().skip(1);
    let mut a = Args {
        store: None,
        words: Vec::new(),
        flags: BTreeMap::new(),
    };
    while let Some(x) = it.next() {
        if let Some(k) = x.strip_prefix("--") {
            let v = it
                .next()
                .ok_or_else(|| LoopError::Usage(format!("--{k} needs a value")))?;
            if k == "store" {
                a.store = Some(v);
            } else if a.flags.insert(k.to_string(), v).is_some() {
                return Err(LoopError::Usage(format!("--{k} given twice")));
            }
        } else {
            a.words.push(x);
        }
    }
    Ok(a)
}

impl Args {
    fn flag(&self, k: &str) -> Result<&str, LoopError> {
        self.flags
            .get(k)
            .map(String::as_str)
            .ok_or_else(|| LoopError::Usage(format!("missing --{k}")))
    }
    fn store(&self) -> Result<Store, LoopError> {
        Store::open_dir(
            self.store
                .clone()
                .ok_or_else(|| LoopError::Usage("missing --store DIR".into()))?,
        )
    }
    fn input(&self) -> Result<String, LoopError> {
        match self.flag("in")? {
            "-" => {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                Ok(s)
            }
            p => Ok(std::fs::read_to_string(p)?),
        }
    }
    fn scope(&self) -> Result<Scope, LoopError> {
        let bad = |w: &str| LoopError::Usage(format!("bad --{w}"));
        Ok(Scope {
            tenant_id: TenantId::new(self.flag("tenant")?).map_err(|_| bad("tenant"))?,
            task_family: TaskFamily::new(self.flag("family")?).map_err(|_| bad("family"))?,
        })
    }
    fn only(&self, allowed: &[&str]) -> Result<(), LoopError> {
        match self.flags.keys().find(|k| !allowed.contains(&k.as_str())) {
            Some(k) => Err(LoopError::Usage(format!("unexpected --{k}"))),
            None => Ok(()),
        }
    }
    fn fref(&self, k: &str) -> Result<Ref, LoopError> {
        Ref::new(self.flag(k)?).map_err(|_| LoopError::Usage(format!("bad --{k}")))
    }
}

fn val<T: Serialize>(x: &T) -> Result<Value, LoopError> {
    serde_json::to_value(x).map_err(|e| LoopError::Io(e.to_string()))
}

fn run(a: &Args) -> Result<Value, LoopError> {
    let w: Vec<&str> = a.words.iter().map(String::as_str).collect();
    match w.as_slice() {
        ["pointer", "resolve"] => {
            a.only(&["tenant", "family"])?;
            let r = pointer::resolve(&a.store()?, &a.scope()?)?;
            Ok(
                json!({"schema":"axon.loop.resolve/1","pin":r.pin,"policy":r.policy,
                      "mechanism_test":r.mechanism_test,"admission_ref":r.admission_ref}),
            )
        }
        ["pointer", "show"] => {
            a.only(&["tenant", "family"])?;
            let st = a.store()?;
            let s = a.scope()?;
            let p = pointer::load(&st, &s)?;
            let r = pointer::revocations(&st, &s)?;
            let n = pointer::log(&st, &s)?.len();
            let r = json!({"schema":"axon.loop.revocations/1","revoked":r});
            Ok(
                json!({"schema":"axon.loop.pointer-view/1","pointer":p,"revocations":r,"transitions":n}),
            )
        }
        ["pointer", "transition"] => {
            a.only(&["in"])?;
            let t: PolicyTransition = parse(&a.input()?)?;
            let p = pointer::transition(&a.store()?, &t)?;
            Ok(json!({"schema":"axon.loop.transition-result/1","pointer":p}))
        }
        ["pointer", "revoke"] => {
            a.only(&["tenant", "family", "policy", "reason", "issuer"])?;
            let issuer = OpaqueRef::new(a.flag("issuer")?)
                .map_err(|_| LoopError::Usage("bad --issuer".into()))?;
            let revoked = pointer::revoke(
                &a.store()?,
                &a.scope()?,
                &a.fref("policy")?,
                &a.fref("reason")?,
                &issuer,
            )?;
            Ok(json!({"schema":"axon.loop.revocations/1","revoked":revoked}))
        }
        ["pointer", "baseline"] => {
            a.only(&["in"])?;
            let b = pointer::parse_baseline(&a.input()?)?;
            let r = pointer::designate_baseline(&a.store()?, &b)?;
            Ok(json!({"schema":"axon.loop.baseline-result/1","baseline_ref":r}))
        }
        ["policy", "put"] => {
            a.only(&["in"])?;
            let p: PolicyEnvelope = parse(&a.input()?)?;
            let r = axon_loop::candidates::put_policy(&a.store()?, &p)?;
            Ok(json!({"schema":"axon.loop.policy-put/1","policy_ref":r}))
        }
        ["tasks", "put"] => {
            a.only(&["in"])?;
            let m = axon_loop::tasks::TaskManifest::parse(&a.input()?)?;
            let r = axon_loop::tasks::put(&a.store()?, &m)?;
            Ok(json!({"schema":"axon.loop.tasks-put/1","task_manifest_ref":r}))
        }
        ["candidates", "put"] => {
            a.only(&["in"])?;
            let c = axon_loop::candidates::CandidateSet::parse(&a.input()?)?;
            let r = axon_loop::candidates::put(&a.store()?, &c)?;
            Ok(json!({"schema":"axon.loop.candidates-put/1","candidate_set_ref":r}))
        }
        ["plan", "register"] => {
            a.only(&["in"])?;
            let p = plan::PilotPlan::parse(&a.input()?)?;
            let r = plan::register(&a.store()?, &p)?;
            Ok(
                json!({"schema":"axon.loop.plan-register/1","experiment_id":p.experiment_id,
                      "plan_ref":r,"unset_fields":p.unset_fields()}),
            )
        }
        ["plan", "freeze"] => {
            a.only(&["experiment"])?;
            let r = plan::freeze(&a.store()?, a.flag("experiment")?)?;
            Ok(json!({"schema":"axon.loop.plan-freeze/1","plan_ref":r}))
        }
        ["plan", "show"] => {
            a.only(&["experiment"])?;
            val(&plan::show(&a.store()?, a.flag("experiment")?)?)
        }
        ["evo", "propose"] => {
            a.only(&["in"])?;
            let req = evo::parse_request(&a.input()?)?;
            val(&evo::propose(&a.store()?, &req)?)
        }
        ["evl", "evaluate"] => {
            a.only(&["in"])?;
            let req = evl::parse_request(&a.input()?)?;
            let (rec, r) = evl::evaluate(&a.store()?, &req)?;
            Ok(json!({"schema":"axon.loop.evl-result/1","evaluation_ref":r,"evaluation":rec}))
        }
        ["admit"] => {
            a.only(&["in"])?;
            let req = admission::parse_request(&a.input()?)?;
            let (rec, r) = admission::admit(&a.store()?, &req)?;
            Ok(json!({"schema":"axon.loop.admit-result/1","admission_ref":r,"admission":rec}))
        }
        ["tel", "summarize"] => {
            a.only(&["in"])?;
            let req: TelRequest = strict_record(&a.input()?)?;
            if req.schema != "axon.loop.tel-request/1" {
                return Err(LoopError::Malformed(Refusal::Shape(
                    "schema must be axon.loop.tel-request/1".into(),
                )));
            }
            let mut eps = Vec::new();
            for (i, e) in req.episodes.iter().enumerate() {
                let ep: LoopEpisode = contract_from_value(&format!("episodes[{i}]"), e)?;
                eps.push(ep);
            }
            let items = || eps.iter().map(|e| (&e.usage, Some(e.status)));
            let schedule = match &req.price_schedule {
                Some(p) => Some(PinnedSchedule::pin(&p.reference, &p.document.to_string())?),
                None => None,
            };
            match (schedule, &req.fabric_attempts) {
                (None, Some(_)) => Err(LoopError::Refused(
                    "fabric_attempts require a pinned price_schedule (G10)".into(),
                )),
                (Some(sched), Some(atts)) => {
                    let mut attempts = Vec::new();
                    for (i, t) in atts.iter().enumerate() {
                        attempts.push(tel::FabricAttempt {
                            request: contract_from_value(
                                &format!("fabric_attempts[{i}].request"),
                                &t.request,
                            )?,
                            receipt: contract_from_value(
                                &format!("fabric_attempts[{i}].receipt"),
                                &t.receipt,
                            )?,
                        });
                    }
                    let j = tel::join(&sched, items(), &attempts, 0)?;
                    Ok(
                        json!({"schema":"axon.loop.tel-summary/1","summary":j.summary,
                              "fabric_join":{
                                  "price_schedule_ref":j.price_schedule_ref,
                                  "fabric_attempts":j.fabric_attempts,
                                  "identical_duplicates":j.identical_duplicates,
                                  "unjoined_attempt_refs":j.unjoined_attempt_refs,
                                  "unreferenced_receipts":j.unreferenced_receipts,
                                  "execution_cost_basis":j.execution_cost_basis},
                              "token_breakdown":"unavailable: Usage v1 has no token fields"}),
                    )
                }
                (schedule, None) => {
                    if let Some(sched) = &schedule {
                        for e in &eps {
                            sched.check_usage(&e.usage)?;
                        }
                    }
                    let s = tel::summarize(items())?;
                    Ok(json!({"schema":"axon.loop.tel-summary/1","summary":s,
                              "token_breakdown":"unavailable: Usage v1 has no token fields"}))
                }
            }
        }
        ["intake", "episode"] => {
            a.only(&["in", "context", "ack", "projection", "source-episode"])?;
            let episode = a.input()?;
            // Parse strictly once here only to learn which receipt the
            // sidecar names when a DIRECTORY is given; intake re-parses.
            let named: LoopEpisode = parse(&episode)?;
            let read = |file: &std::path::Path| {
                std::fs::read_to_string(file)
                    .map_err(|e| LoopError::Io(format!("{}: {e}", file.display())))
            };
            let context = {
                let p = std::path::Path::new(a.flag("context")?);
                if p.is_dir() {
                    read(&p.join(format!("{}.json", named.context_ref.hex())))?
                } else {
                    read(p)?
                }
            };
            // G6: the ack is found BY CONTENT, so a directory hands over all
            // of its *.json files and the intake selects exactly one.
            let acks: Vec<String> = {
                let p = std::path::Path::new(a.flag("ack")?);
                if p.is_dir() {
                    let mut files: Vec<_> = std::fs::read_dir(p)?
                        .filter_map(|e| e.ok().map(|e| e.path()))
                        .filter(|f| f.extension().is_some_and(|x| x == "json") && f.is_file())
                        .collect();
                    files.sort();
                    files.iter().map(|f| read(f)).collect::<Result<_, _>>()?
                } else {
                    vec![read(p)?]
                }
            };
            let projection = match a.flags.get("projection") {
                Some(p) => Some(read(std::path::Path::new(p))?),
                None => None,
            };
            let source = match a.flags.get("source-episode") {
                Some(p) => Some(std::fs::read_to_string(p)?),
                None => None,
            };
            let out = intake::intake_episode(
                &a.store()?,
                &intake::IntakeInput {
                    episode: &episode,
                    context: &context,
                    acks: &acks,
                    projection: projection.as_deref(),
                    source_episode: source.as_deref(),
                },
            )?;
            Ok(
                json!({"schema":"axon.loop.intake-result/1","episode_ref":out.record.episode_ref,
                      "ledger_seq":out.ledger_seq,"recorded_now":out.recorded_now,
                      "record":out.record}),
            )
        }
        _ => Err(LoopError::Usage(format!(
            "unknown verb {:?}; see `axon-loop` module docs for the verb list",
            a.words.join(" ")
        ))),
    }
}

fn main() {
    match parse_args().and_then(|a| run(&a)) {
        Ok(v) => println!("{}", serde_json::to_string_pretty(&v).expect("json")),
        Err(e) => {
            eprintln!(
                "{}",
                json!({"schema":"axon.loop.error/1","error":e.kind(),
                       "exit_code":e.exit_code(),"message":e.to_string()})
            );
            std::process::exit(e.exit_code());
        }
    }
}
