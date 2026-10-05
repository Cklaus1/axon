//! Amendment 64 (C9 round 4b, integrate-D): `price.rs` and `tel.rs`'s refusal
//! sites, each judged on its PRODUCTION route: the `axon-loop tel summarize`
//! verb, the only production caller of `PinnedSchedule::pin`, `tel::join`
//! and (with `evl`/`admission`) the usage summary. Every test is an ATTACK
//! with its CONTROL (`the_honest_request_is_summarized`); where several checks
//! refuse the same attack (a four-cell retirement), any of their reasons is
//! accepted: only the attack getting through is the failure.

mod common;

use axon_loop_contracts::*;
use common::{bundle, r};
use serde_json::{json, Value};

fn schedule_doc(covers: &[&str]) -> Value {
    json!({
        "schema": "axon.loop.price-schedule/1",
        "currency": "USD",
        "covers": covers,
        "rates": {"model": {"claude-haiku": {"input_per_mtok_micro": 1000000}}}
    })
}

/// `{"ref": digest(doc), "document": doc}`.
fn pinned(doc: Value) -> Value {
    json!({"ref": digest_value(&doc).unwrap(), "document": doc})
}

fn sched_ref() -> Ref {
    digest_value(&schedule_doc(&["model"])).unwrap()
}

/// The bundle's request, re-pointed at the honest schedule, as attempt `id`.
fn request(id: &str) -> Value {
    let mut q = bundle()["acf_request"].clone();
    q["limits"]["price_schedule_ref"] = json!(sched_ref());
    q["attempt_id"] = json!(id);
    q
}

fn receipt(q: &Value) -> Value {
    let mut rc = bundle()["acf_receipt"].clone();
    rc["attempt_id"] = q["attempt_id"].clone();
    rc
}

fn attempt(id: &str) -> Value {
    let q = request(id);
    let rc = receipt(&q);
    json!({"request": q, "receipt": rc})
}

fn receipt_ref(att: &Value) -> Ref {
    let rc: ExecutionReceipt = parse(&att["receipt"].to_string()).unwrap();
    digest(&rc).unwrap()
}

/// The bundle's episode, naming the honest schedule and `attempts`.
fn episode(attempts: &[Ref]) -> Value {
    let mut ep = bundle()["episode"].clone();
    ep["usage"]["price_schedule_ref"] = json!(sched_ref());
    ep["usage"]["attempt_refs"] = json!(attempts);
    ep
}

/// The honest request: one episode, the pinned schedule, one Fabric attempt.
fn honest() -> Value {
    let att = attempt("fixture-attempt_id");
    json!({"schema":"axon.loop.tel-request/1",
           "episodes":[episode(&[receipt_ref(&att)])],
           "price_schedule": pinned(schedule_doc(&["model"])),
           "fabric_attempts":[att]})
}

/// Run `axon-loop tel summarize` on `req`: (exit code, stderr).
fn tel(req: &Value) -> (i32, String) {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), req.to_string()).unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_axon-loop"))
        .args(["tel", "summarize", "--in"])
        .arg(tmp.path())
        .output()
        .unwrap();
    (
        o.status.code().unwrap(),
        String::from_utf8_lossy(&o.stderr).into_owned(),
    )
}

/// The summary is refused for one of `why`; ATTACK if it was produced.
fn never_summarized(req: &Value, why: &[&str], attack: &str) {
    let (code, err) = tel(req);
    if code == 0 {
        panic!("ATTACK: {attack}: the cost summary was produced");
    }
    assert!(
        why.iter().any(|y| err.contains(y)),
        "{attack}: refused (exit {code}) but not for one of {why:?}: {err}"
    );
}

#[test]
fn the_honest_request_is_summarized() {
    let (code, err) = tel(&honest());
    assert_eq!(code, 0, "{err}");
    // And without Fabric attempts (the usage-only route).
    let mut q = honest();
    q.as_object_mut().unwrap().remove("fabric_attempts");
    let (code, err) = tel(&q);
    assert_eq!(code, 0, "{err}");
}

/// The verb's own two refusals (amendment 74: the CLI decides them itself, so
/// each has a row): a request of another schema, and Fabric attempts with no
/// pinned schedule (G10: an attempt cannot be priced by a schedule nobody
/// pinned, and a summary that silently priced nothing would read as free).
#[test]
fn a_request_of_another_schema_is_never_summarized() {
    let mut q = honest();
    q["schema"] = json!("axon.loop.tel-request/2");
    never_summarized(
        &q,
        &["axon.loop.tel-request/1"],
        "a telemetry request of another schema was summarized",
    );
}

#[test]
fn fabric_attempts_without_a_pinned_schedule_are_never_summarized() {
    let mut q = honest();
    q.as_object_mut().unwrap().remove("price_schedule");
    never_summarized(
        &q,
        &["require a pinned price_schedule"],
        "Fabric attempts with no pinned price schedule were summarized (G10)",
    );
}

// ── PinnedSchedule::pin ─────────────────────────────────────────────────────

#[test]
fn a_schedule_whose_content_is_not_its_ref_never_prices() {
    let mut q = honest();
    q["price_schedule"]["document"]["rates"]["model"]["claude-haiku"]["input_per_mtok_micro"] =
        json!(1);
    never_summarized(
        &q,
        &["not the pinned"],
        "a schedule whose content is not its ref priced the cohort",
    );
}

/// Four-cell (the pin's scheme; the content digest, always cl22): a schedule
/// pinned by a sha256 Ref of its own hex.
#[test]
fn a_schedule_pinned_by_a_non_cl22_ref_never_prices() {
    let sha = Ref::new(format!("sha256:{}", sched_ref().hex())).unwrap();
    let mut q = honest();
    q.as_object_mut().unwrap().remove("fabric_attempts");
    q["price_schedule"]["ref"] = json!(sha);
    q["episodes"][0]["usage"]["price_schedule_ref"] = json!(sha);
    never_summarized(
        &q,
        &["must be pinned by a cl22", "price schedule content is"],
        "a schedule pinned by a sha256 ref priced the cohort",
    );
}

/// Pin `doc` (by its own digest) and name it everywhere the request names
/// the schedule.
fn with_schedule(doc: Value) -> Value {
    let p = pinned(doc);
    let mut q = honest();
    q["price_schedule"] = p.clone();
    q["episodes"][0]["usage"]["price_schedule_ref"] = p["ref"].clone();
    q["fabric_attempts"][0]["request"]["limits"]["price_schedule_ref"] = p["ref"].clone();
    // The receipt is unchanged, so the usage still names it.
    q
}

#[test]
fn a_schedule_of_another_schema_never_prices() {
    let mut doc = schedule_doc(&["model"]);
    doc["schema"] = json!("axon.loop.price-schedule/2");
    never_summarized(
        &with_schedule(doc),
        &["price schedule schema must be"],
        "a schedule of another schema priced the cohort",
    );
}

#[test]
fn a_schedule_covering_nothing_never_prices() {
    never_summarized(
        &with_schedule(schedule_doc(&[])),
        &["covers nothing"],
        "a schedule covering nothing priced the cohort",
    );
}

#[test]
fn a_schedule_repeating_a_coverage_never_prices() {
    never_summarized(
        &with_schedule(schedule_doc(&["model", "model"])),
        &["repeats a coverage"],
        "a schedule repeating a coverage priced the cohort",
    );
}

#[test]
fn a_schedule_claiming_execution_coverage_never_prices() {
    never_summarized(
        &with_schedule(schedule_doc(&["model", "execution"])),
        &["claims execution coverage"],
        "a schedule claiming execution coverage priced the cohort",
    );
}

// ── check_request / resolve_opaque / check_usage ────────────────────────────

#[test]
fn a_request_naming_another_schedule_never_joins() {
    let mut q = honest();
    q["fabric_attempts"][0]["request"]["limits"]["price_schedule_ref"] = json!(r('9'));
    never_summarized(
        &q,
        &["is not the pinned schedule"],
        "a request naming another schedule was joined",
    );
}

/// Four-cell (the request ref's scheme; equality with the pinned cl22 ref):
/// the pinned schedule's hex under sha256.
#[test]
fn a_request_naming_the_schedule_under_another_scheme_never_joins() {
    let mut q = honest();
    q["fabric_attempts"][0]["request"]["limits"]["price_schedule_ref"] =
        json!(format!("sha256:{}", sched_ref().hex()));
    never_summarized(
        &q,
        &["is not a cl22 content Ref", "is not the pinned schedule"],
        "a request naming the schedule under another scheme was joined",
    );
}

#[test]
fn a_request_in_another_currency_never_joins() {
    let mut q = honest();
    q["fabric_attempts"][0]["request"]["limits"]["currency_code"] = json!("EUR");
    never_summarized(
        &q,
        &["is not the pinned schedule's"],
        "a request in another currency was joined",
    );
}

#[test]
fn a_usage_naming_another_schedule_never_prices() {
    let mut q = honest();
    q.as_object_mut().unwrap().remove("fabric_attempts");
    q["episodes"][0]["usage"]["price_schedule_ref"] = json!(r('d'));
    never_summarized(
        &q,
        &["usage.price_schedule_ref"],
        "a usage naming another schedule was priced",
    );
}

#[test]
fn a_usage_in_another_currency_never_prices() {
    let mut q = honest();
    q.as_object_mut().unwrap().remove("fabric_attempts");
    q["episodes"][0]["usage"]["currency"] = json!("EUR");
    never_summarized(
        &q,
        &["usage currency EUR"],
        "a usage in another currency was priced",
    );
}

// ── tel::join / summarize ───────────────────────────────────────────────────

#[test]
fn an_attempt_named_by_two_usages_is_never_counted_twice() {
    let mut q = honest();
    q.as_object_mut().unwrap().remove("fabric_attempts");
    q.as_object_mut().unwrap().remove("price_schedule");
    let ep = q["episodes"][0].clone();
    q["episodes"] = json!([ep.clone(), ep]);
    never_summarized(
        &q,
        &["is accounted twice"],
        "an attempt named by two usages was counted twice",
    );
}

#[test]
fn a_receipt_of_another_attempt_never_joins_its_request() {
    let mut q = honest();
    q["fabric_attempts"][0]["receipt"]["attempt_id"] = json!("other-attempt");
    never_summarized(
        &q,
        &["receipt identity does not match its request"],
        "a receipt of another attempt was joined to the request",
    );
}

#[test]
fn a_receipt_paired_with_two_requests_never_joins() {
    let mut q = honest();
    let mut other = q["fabric_attempts"][0].clone();
    let w = other["request"]["limits"]["wall_time_ms"].as_u64().unwrap();
    other["request"]["limits"]["wall_time_ms"] = json!(w + 1);
    q["fabric_attempts"].as_array_mut().unwrap().push(other);
    never_summarized(
        &q,
        &["is paired with two different requests"],
        "a receipt paired with two different requests was joined",
    );
}

#[test]
fn an_attempt_reported_by_two_receipts_is_never_counted_twice() {
    let mut q = honest();
    let mut other = q["fabric_attempts"][0].clone();
    other["receipt"]["unresolved_liability_micro"] = json!(7);
    q["fabric_attempts"].as_array_mut().unwrap().push(other);
    never_summarized(
        &q,
        &["is reported by two different receipts"],
        "an attempt reported by two different receipts was counted twice",
    );
}

/// Four-cell (join's re-validation of the receipt; the parse's schema and
/// typed validation of the same document): a passed verification on a failed
/// run.
#[test]
fn a_receipt_breaking_its_contract_never_joins() {
    let mut q = honest();
    q["fabric_attempts"][0]["receipt"]["verification"] = json!("passed");
    q["fabric_attempts"][0]["receipt"]["status"] = json!("failed");
    never_summarized(
        &q,
        &["fabric_attempts[0].receipt"],
        "a receipt breaking its contract was joined",
    );
}

/// Four-cell (join's re-validation of the request; the parse's schema and
/// typed validation of the same document): 129 argv items.
#[test]
fn a_request_breaking_its_contract_never_joins() {
    let mut q = honest();
    q["fabric_attempts"][0]["request"]["argv"] = json!(vec!["x"; 129]);
    never_summarized(
        &q,
        &["fabric_attempts[0].request"],
        "a request breaking its contract was joined",
    );
}

/// The parse's schema layer (amendment 64): an enum value in serde's
/// map form (`{"completed": null}`), which typed serde alone accepts and the
/// checked-in schema refuses (red-team D2).
#[test]
fn an_episode_in_a_non_schema_shape_is_never_summarized() {
    let mut q = honest();
    q.as_object_mut().unwrap().remove("fabric_attempts");
    q.as_object_mut().unwrap().remove("price_schedule");
    q["episodes"][0]["status"] = json!({"completed": null});
    never_summarized(
        &q,
        &["episodes[0]"],
        "an episode in a shape its schema refuses was summarized",
    );
}

/// `resolve_opaque` is a public primitive (its one production caller,
/// check_request, then compares with the pinned cl22 ref, M1351): on its own
/// it never reads a ref of another scheme as the schedule's content ref.
/// Control: the cl22 ref resolves to itself.
#[test]
fn resolve_opaque_never_reads_another_scheme_as_a_content_ref() {
    use axon_loop::price::resolve_opaque;
    let cl22 = sched_ref();
    assert_eq!(
        resolve_opaque(&OpaqueRef::new(cl22.as_str()).unwrap()).unwrap(),
        cl22
    );
    let sha = OpaqueRef::new(format!("sha256:{}", cl22.hex())).unwrap();
    if let Ok(r) = resolve_opaque(&sha) {
        panic!("ATTACK: resolve_opaque read a sha256 ref as the content ref {r}");
    }
}
