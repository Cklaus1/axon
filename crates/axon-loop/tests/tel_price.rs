//! B270 — pinned price schedule (G10), Fabric receipt join, and cohort cost.
//!
//! G10-r22-cohort-denominator: every assigned trial stays in the denominator,
//! and an unknown cost is unresolved, never free — so it can never produce a
//! zero-cost winner. G13-r22-billing-settlement (TEL side): the same attempt
//! reported by two different receipts is refused, the identical receipt is
//! collapsed. G10 schedule identity: the ACF request's `OpaqueRef` must be
//! the pinned schedule's cl22 Ref. D10: execution cost is always unknown.
//!
//! G10-r22-full-task-cost is NOT checked here: it needs an execution price
//! schedule, and none exists (D10).

mod common;

use axon_loop::price::{execution_cost, resolve_opaque, ExecutionCost, PinnedSchedule};
use axon_loop::tel::{self, cohort_cost, compare_per_trial, CohortCost, CostOrder, FabricAttempt};
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

fn pinned() -> PinnedSchedule {
    let doc = schedule_doc(&["model"]);
    PinnedSchedule::pin(&digest_value(&doc).unwrap(), &doc.to_string()).unwrap()
}

fn member<T: Contract>(k: &str) -> T {
    parse(&serde_json::to_string(&bundle()[k]).unwrap()).unwrap()
}

/// The bundle's request, re-pointed at `schedule` (the bundle's own
/// `fixture:synthetic-not-pricing` is exactly the G10 mismatch).
fn request(schedule: &PinnedSchedule, attempt: &str) -> ComputeRequest {
    let mut q: ComputeRequest = member("acf_request");
    q.limits.price_schedule_ref = OpaqueRef::new(schedule.reference().as_str()).unwrap();
    q.attempt_id = AttemptId::new(attempt).unwrap();
    q
}

fn receipt(q: &ComputeRequest) -> ExecutionReceipt {
    let mut rc: ExecutionReceipt = member("acf_receipt");
    rc.attempt_id = q.attempt_id.clone();
    rc
}

fn attempt(s: &PinnedSchedule, id: &str) -> FabricAttempt {
    let request = request(s, id);
    let receipt = receipt(&request);
    FabricAttempt { request, receipt }
}

fn usage(s: &PinnedSchedule, state: UsageState, cost: Option<u64>, refs: Vec<Ref>) -> Usage {
    Usage {
        state,
        cost_micro: cost,
        unresolved_liability_micro: 0,
        currency: s.currency().clone(),
        price_schedule_ref: s.reference().clone(),
        attempt_refs: refs,
    }
}

// ── the schedule is pinned by content ───────────────────────────────────────

#[test]
fn g10_price_schedule_is_pinned_by_content_ref() {
    let doc = schedule_doc(&["model"]);
    let good = digest_value(&doc).unwrap();
    assert!(PinnedSchedule::pin(&good, &doc.to_string()).is_ok());
    // Same Ref, different content: refused.
    let mut other = doc.clone();
    other["rates"]["model"]["claude-haiku"]["input_per_mtok_micro"] = json!(1);
    assert!(PinnedSchedule::pin(&good, &other.to_string()).is_err());
    // Pinned by a non-cl22 Ref: refused even when the hex matches.
    let sha = Ref::new(format!("sha256:{}", good.hex())).unwrap();
    assert!(PinnedSchedule::pin(&sha, &doc.to_string()).is_err());
}

#[test]
fn d10_a_schedule_claiming_execution_coverage_is_refused() {
    for covers in [&["model", "execution"][..], &["execution"][..]] {
        let doc = schedule_doc(covers);
        let e = PinnedSchedule::pin(&digest_value(&doc).unwrap(), &doc.to_string()).unwrap_err();
        assert!(e.to_string().contains("D10"), "{e}");
    }
}

// ── G10: request OpaqueRef vs sidecar Ref ───────────────────────────────────

#[test]
fn g10_price_schedule_ref_mismatch_is_refused() {
    let s = pinned();
    // The bundle's own request names a non-content opaque string.
    let bundle_req: ComputeRequest = member("acf_request");
    assert!(s.check_request(&bundle_req).is_err());
    // A different cl22 schedule.
    let mut q = request(&s, "a-1");
    q.limits.price_schedule_ref = OpaqueRef::new(r('9').as_str()).unwrap();
    assert!(s.check_request(&q).is_err());
    // The same hex under another scheme is not the same schedule.
    q.limits.price_schedule_ref =
        OpaqueRef::new(format!("sha256:{}", s.reference().hex())).unwrap();
    assert!(s.check_request(&q).is_err());
    assert!(resolve_opaque(&q.limits.price_schedule_ref).is_err());
    // Right schedule, wrong currency.
    let mut q = request(&s, "a-1");
    q.limits.currency_code = Currency::new("EUR").unwrap();
    assert!(s.check_request(&q).is_err());
    // The pinned Ref itself is accepted.
    assert!(s.check_request(&request(&s, "a-1")).is_ok());

    // Sidecar side.
    let mut u = usage(&s, UsageState::Final, Some(1), vec![r('1')]);
    assert!(s.check_usage(&u).is_ok());
    u.price_schedule_ref = r('d');
    assert!(s.check_usage(&u).is_err());

    // End to end: a join refuses either side naming another schedule.
    assert!(tel::join(&s, [(&u, None)], &[], 0).is_err());
    let bad = FabricAttempt {
        receipt: receipt(&bundle_req),
        request: bundle_req,
    };
    assert!(tel::join(&s, std::iter::empty(), &[bad], 0).is_err());
}

// ── D10: execution cost ─────────────────────────────────────────────────────

#[test]
fn d10_execution_cost_is_unknown_and_keeps_the_reservation() {
    let s = pinned();
    let fa = attempt(&s, "a-1");
    // The bundle receipt SAYS settled/25µ; there is no execution schedule to
    // price it under, so it is unknown and holds the 100000µ reservation.
    assert_eq!(fa.receipt.cost_micro, Some(25));
    assert_eq!(
        execution_cost(&fa.request, &fa.receipt),
        ExecutionCost::Unknown {
            reserved_liability_micro: 100_000
        }
    );
    // A receipt reporting MORE than the reservation raises what is held.
    let mut big = fa.clone();
    big.receipt.cost_micro = Some(250_000);
    assert_eq!(
        execution_cost(&big.request, &big.receipt),
        ExecutionCost::Unknown {
            reserved_liability_micro: 250_000
        }
    );

    // Joined with a FINAL model usage, the whole task is still unresolved.
    let rref = digest(&fa.receipt).unwrap();
    let u = usage(&s, UsageState::Final, Some(40), vec![rref]);
    let j = tel::join(&s, [(&u, Some(EpisodeStatus::Completed))], &[fa], 0).unwrap();
    assert_eq!(j.fabric_attempts, 1);
    assert_eq!(j.unjoined_attempt_refs, 0);
    assert_eq!(j.unreferenced_receipts, 0);
    assert_eq!(
        j.summary.single_total().unwrap(),
        &tel::Total::Unresolved {
            known_sum_micro: 40,
            unknown_count: 1,
            unresolved_liability_micro: 100_000
        }
    );
}

// ── join: duplicates and double counts ──────────────────────────────────────

#[test]
fn join_collapses_an_identical_receipt_and_refuses_a_different_one() {
    let s = pinned();
    let fa = attempt(&s, "a-1");
    let j = tel::join(&s, std::iter::empty(), &[fa.clone(), fa.clone()], 0).unwrap();
    assert_eq!(j.fabric_attempts, 1);
    assert_eq!(j.identical_duplicates, 1);
    assert_eq!(
        j.summary.records, 1,
        "the identical receipt is counted once"
    );
    // Unreferenced receipts still count (a failed/retried attempt costs).
    assert_eq!(j.unreferenced_receipts, 1);

    // Same (operation_id, attempt_id), different content: double count.
    let mut other = fa.clone();
    other.receipt.unresolved_liability_micro = 7;
    let e = tel::join(&s, std::iter::empty(), &[fa.clone(), other], 0).unwrap_err();
    assert!(e.to_string().contains("two different receipts"), "{e}");

    // Same receipt paired with a different request.
    let mut req2 = fa.clone();
    req2.request.limits.wall_time_ms += 1;
    assert!(tel::join(&s, std::iter::empty(), &[fa.clone(), req2], 0).is_err());

    // Two usages naming the same receipt: double count.
    let rref = digest(&fa.receipt).unwrap();
    let u1 = usage(&s, UsageState::Final, Some(1), vec![rref.clone()]);
    let u2 = usage(&s, UsageState::Final, Some(1), vec![rref]);
    assert!(tel::join(&s, [(&u1, None), (&u2, None)], &[fa], 0).is_err());
}

#[test]
fn an_attempt_ref_without_a_receipt_is_unresolved_not_free() {
    let s = pinned();
    // A FINAL model usage whose attempt has no Fabric receipt: the execution
    // component is unknown, so the total cannot be the model cost alone.
    let u = usage(&s, UsageState::Final, Some(40), vec![r('1')]);
    let j = tel::join(&s, [(&u, Some(EpisodeStatus::Completed))], &[], 0).unwrap();
    assert_eq!(j.unjoined_attempt_refs, 1);
    assert!(matches!(
        j.summary.single_total().unwrap(),
        tel::Total::Unresolved { .. }
    ));
}

// ── cohort denominator and comparison ───────────────────────────────────────

#[test]
fn g10_cohort_denominator_counts_unknown_as_unresolved_not_free() {
    let s = pinned();
    let known = usage(&s, UsageState::Final, Some(10), vec![r('1')]);
    let unknown = usage(&s, UsageState::Unknown, None, vec![r('2')]);
    let sum = tel::summarize([
        (&known, Some(EpisodeStatus::Completed)),
        (&unknown, Some(EpisodeStatus::Failed)),
    ])
    .unwrap();
    assert_eq!(
        cohort_cost(&sum, 2).unwrap(),
        CohortCost::Unresolved {
            known_sum_micro: 10,
            unresolved_records: 1,
            unresolved_liability_micro: 0,
            assigned: 2
        }
    );
    // A missing trial stays in the denominator as an unresolved record.
    let sum = tel::summarize_with_missing([(&known, Some(EpisodeStatus::Completed))], 1).unwrap();
    assert_eq!(
        cohort_cost(&sum, 2).unwrap(),
        CohortCost::Unresolved {
            known_sum_micro: 10,
            unresolved_records: 1,
            unresolved_liability_micro: 0,
            assigned: 2
        }
    );
    // No denominator, or fewer assigned than missing: refused.
    assert!(cohort_cost(&sum, 0).is_err());
    let sum2 = tel::summarize_with_missing(std::iter::empty(), 3).unwrap();
    assert!(cohort_cost(&sum2, 2).is_err());
    assert!(matches!(
        cohort_cost(&sum2, 3).unwrap(),
        CohortCost::Unresolved {
            unresolved_records: 3,
            ..
        }
    ));
}

#[test]
fn g13_an_unknown_cost_never_produces_a_zero_cost_winner() {
    let s = pinned();
    // Candidate: one trial of UNKNOWN cost. Incumbent: one trial at 10µ.
    let cu = usage(&s, UsageState::Unknown, None, vec![r('1')]);
    let iu = usage(&s, UsageState::Final, Some(10), vec![r('2')]);
    let cand = cohort_cost(&tel::summarize([(&cu, None)]).unwrap(), 1).unwrap();
    let inc = cohort_cost(&tel::summarize([(&iu, None)]).unwrap(), 1).unwrap();
    assert!(matches!(cand, CohortCost::Unresolved { .. }));
    assert_eq!(compare_per_trial(&cand, &inc), CostOrder::Unresolved);
    assert_eq!(compare_per_trial(&inc, &cand), CostOrder::Unresolved);

    // Mixed: known 5 + unknown against known 10 + 10 — still no winner.
    let c1 = usage(&s, UsageState::Final, Some(5), vec![r('3')]);
    let c2 = usage(&s, UsageState::Unknown, None, vec![r('4')]);
    let i1 = usage(&s, UsageState::Final, Some(10), vec![r('5')]);
    let i2 = usage(&s, UsageState::Final, Some(10), vec![r('6')]);
    let cand = cohort_cost(&tel::summarize([(&c1, None), (&c2, None)]).unwrap(), 2).unwrap();
    let inc = cohort_cost(&tel::summarize([(&i1, None), (&i2, None)]).unwrap(), 2).unwrap();
    assert_eq!(compare_per_trial(&cand, &inc), CostOrder::Unresolved);

    // Two known cohorts ARE ordered, per ASSIGNED trial (failures included):
    // 40µ over 2 trials beats 25µ over 1.
    let f = usage(&s, UsageState::Final, Some(30), vec![r('7')]);
    let ok = usage(&s, UsageState::Final, Some(10), vec![r('8')]);
    let cand = cohort_cost(
        &tel::summarize([
            (&ok, Some(EpisodeStatus::Completed)),
            (&f, Some(EpisodeStatus::Failed)),
        ])
        .unwrap(),
        2,
    )
    .unwrap();
    let one = usage(&s, UsageState::Final, Some(25), vec![r('a')]);
    let inc = cohort_cost(&tel::summarize([(&one, None)]).unwrap(), 1).unwrap();
    assert_eq!(compare_per_trial(&cand, &inc), CostOrder::Lower);
    assert_eq!(compare_per_trial(&inc, &cand), CostOrder::Higher);

    // Through the Fabric join: a candidate whose model cost is FINAL 0 still
    // has an unknown execution component, so it cannot win on cost either.
    let fa = attempt(&s, "a-1");
    let zero = usage(
        &s,
        UsageState::Final,
        Some(0),
        vec![digest(&fa.receipt).unwrap()],
    );
    let j = tel::join(&s, [(&zero, Some(EpisodeStatus::Completed))], &[fa], 0).unwrap();
    let cand = cohort_cost(&j.summary, 1).unwrap();
    assert_eq!(compare_per_trial(&cand, &inc), CostOrder::Unresolved);
}

// ── the CLI surface (`tel summarize` with price_schedule / fabric_attempts) ─

fn tel_cli(req: &Value) -> (i32, Value) {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), req.to_string()).unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_axon-loop"))
        .args(["tel", "summarize", "--in"])
        .arg(tmp.path())
        .output()
        .unwrap();
    let code = o.status.code().unwrap();
    let out = String::from_utf8(o.stdout).unwrap();
    if code != 0 {
        assert!(out.is_empty(), "stdout must be empty on failure");
        return (code, Value::Null);
    }
    (code, serde_json::from_str(&out).unwrap())
}

#[test]
fn cli_tel_pins_the_schedule_and_joins_fabric_receipts() {
    let s = pinned();
    let doc = schedule_doc(&["model"]);
    let fa = attempt(&s, "fixture-attempt_id");
    let mut ep = bundle()["episode"].clone();
    ep["usage"]["price_schedule_ref"] = json!(s.reference());
    ep["usage"]["attempt_refs"] = json!([digest(&fa.receipt).unwrap()]);
    let att = json!({"request": fa.request, "receipt": fa.receipt});
    let sched = json!({"ref": s.reference(), "document": doc});

    let (c, v) = tel_cli(&json!({"schema":"axon.loop.tel-request/1","episodes":[ep],
        "price_schedule": sched, "fabric_attempts": [att, att]}));
    assert_eq!(c, 0);
    assert_eq!(v["fabric_join"]["fabric_attempts"], json!(1));
    assert_eq!(v["fabric_join"]["identical_duplicates"], json!(1));
    assert_eq!(
        v["summary"]["by_currency"][0]["total"],
        json!({"state":"unresolved","known_sum_micro":25,"unknown_count":1,
               "unresolved_liability_micro":100000})
    );

    // A usage naming another schedule: refused (exit 4), nothing printed.
    let mut bad = ep.clone();
    bad["usage"]["price_schedule_ref"] = json!(r('d'));
    let (c, _) = tel_cli(&json!({"schema":"axon.loop.tel-request/1","episodes":[bad],
        "price_schedule": sched}));
    assert_eq!(c, 4);
    // A request naming another schedule: refused.
    let mut bad_att = att.clone();
    bad_att["request"]["limits"]["price_schedule_ref"] = json!("fixture:synthetic-not-pricing");
    let (c, _) = tel_cli(&json!({"schema":"axon.loop.tel-request/1","episodes":[ep],
        "price_schedule": sched, "fabric_attempts": [bad_att]}));
    assert_eq!(c, 4);
    // Schedule content that does not match its Ref: refused.
    let mut wrong = sched.clone();
    wrong["document"]["currency"] = json!("EUR");
    let (c, _) = tel_cli(&json!({"schema":"axon.loop.tel-request/1","episodes":[ep],
        "price_schedule": wrong}));
    assert_eq!(c, 4);
    // Fabric attempts without a pinned schedule: refused.
    let (c, _) = tel_cli(&json!({"schema":"axon.loop.tel-request/1","episodes":[ep],
        "fabric_attempts": [att]}));
    assert_eq!(c, 4);
}
