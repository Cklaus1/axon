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
//! axon-loop --store DIR policy  put        --in policy.json
//! axon-loop --store DIR plan    register   --in plan.json
//! axon-loop --store DIR plan    freeze     --experiment ID
//! axon-loop --store DIR plan    show       --experiment ID
//! axon-loop --store DIR evo     propose    --in evo-request.json
//! axon-loop --store DIR evl     evaluate   --in evl-request.json
//! axon-loop --store DIR admit              --in admit-request.json
//! axon-loop             tel     summarize  --in tel-request.json
//! axon-loop --store DIR intake  episode    --in sidecar.json --context FILE|DIR
//!                                          --ack FILE|DIR [--source-episode FILE]
//! ```
//!
//! `intake episode`: `--context`/`--ack` may name MiCode's `context/` /
//! `policy-ack/` directory, in which case the file is the one the sidecar
//! names (`context_ref` / `projection_ref`); nothing is searched for.

use axon_loop::error::LoopError;
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
        Store::open(
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
            let r = a.store()?.put_cas("policies", &p)?;
            Ok(json!({"schema":"axon.loop.policy-put/1","policy_ref":r}))
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
            let s = tel::summarize(eps.iter().map(|e| (&e.usage, Some(e.status))))?;
            Ok(json!({"schema":"axon.loop.tel-summary/1","summary":s,
                      "token_breakdown":"unavailable: Usage v1 has no token fields"}))
        }
        ["intake", "episode"] => {
            a.only(&["in", "context", "ack", "source-episode"])?;
            let episode = a.input()?;
            // Parse strictly once here only to learn which receipt/ack the
            // sidecar names when a DIRECTORY is given; intake re-parses.
            let named: LoopEpisode = parse(&episode)?;
            let pick = |flag: &str, r: Option<&Ref>| -> Result<Option<String>, LoopError> {
                let Some(p) = a.flags.get(flag) else {
                    return Ok(None);
                };
                let p = std::path::Path::new(p);
                let file = if p.is_dir() {
                    let r = r.ok_or_else(|| {
                        LoopError::Refused(format!(
                            "--{flag} is a directory but the episode names no ref for it"
                        ))
                    })?;
                    p.join(format!("{}.json", r.hex()))
                } else {
                    p.to_path_buf()
                };
                std::fs::read_to_string(&file)
                    .map(Some)
                    .map_err(|e| LoopError::Io(format!("{}: {e}", file.display())))
            };
            let context = pick("context", Some(&named.context_ref))?
                .ok_or_else(|| LoopError::Usage("missing --context".into()))?;
            let ack = pick("ack", named.projection_ref.as_ref())?
                .ok_or_else(|| LoopError::Usage("missing --ack".into()))?;
            let source = match a.flags.get("source-episode") {
                Some(p) => Some(std::fs::read_to_string(p)?),
                None => None,
            };
            let out = intake::intake_episode(
                &a.store()?,
                &intake::IntakeInput {
                    episode: &episode,
                    context: &context,
                    ack: &ack,
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
