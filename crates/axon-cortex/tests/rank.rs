//! The ranking step between `locate` and the candidate walk.
//!
//! A ranker may only PERMUTE the eligible names. Every row below that is not
//! the first one is a way of failing — and each must keep the spectrum order
//! and say why, because a silent fallback reads as a choice.
//!
//! Hermetic: a scripted transport records every request and answers from a
//! script; under `--features ai` a loopback server stands in for OpenRouter.
//! Nothing here reaches the network.

mod common;

use axon_cortex::openrouter::Client;
use axon_cortex::rank::{
    self, Advice, RankInput, RankReport, Ranker, RankerSpec, MAX_PROMPT_CHARS,
};
use common::scripted::{Scripted, KEY};
use serde_json::{json, Value};
use std::time::Duration;

const SRC: &str = "\
fn alpha(n: i64) -> i64 { n * 2 }
fn beta(n: i64) -> i64 { n * 2 }
fn gamma(n: i64) -> i64 { n }
@[test]
fn visible_sum() { assert_eq(alpha(1) + beta(1) + gamma(1), 6) }
// the adjudicator: SECRET_COMMENT
@[test]
fn hidden_grader() { assert_eq(gamma(1), 2) assert_eq(SECRET_BODY_MARKER, 1) }
fn main() { println(to_str(alpha(1))) }
";

fn names(xs: &[&str]) -> Vec<String> {
    xs.iter().map(|s| s.to_string()).collect()
}

fn chat_reply(content: &str) -> Result<(u16, String), String> {
    Ok((
        200,
        json!({
            "model": "anthropic/claude-haiku-5.5",
            "choices": [{"message": {"role": "assistant", "content": content}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 900, "completion_tokens": 20, "cost": 0.0007},
        })
        .to_string(),
    ))
}

fn decision_reply(choice: &str, probs: Value) -> Result<(u16, String), String> {
    Ok((
        200,
        json!({
            "model": "inception/mercury-decide-20260930",
            "answers": {"buggy_function": {"type": "choice", "choice": choice, "confidence": 0.5, "probabilities": probs}},
            "usage": {"input_tokens": 2000, "output_tokens": 1, "cost": 0.00004},
        })
        .to_string(),
    ))
}

fn ranker<'t>(spec: RankerSpec, t: &'t dyn axon_cortex::openrouter::Transport) -> Ranker<'t> {
    Ranker {
        spec,
        client: Client {
            key: Some(KEY.to_string()),
            transport: Some(t),
            chat_url: "http://chat.invalid/v1/chat/completions".into(),
            decisions_url: "http://decide.invalid/alpha/decisions".into(),
            timeout: Duration::from_secs(20),
        },
    }
}

fn haiku() -> RankerSpec {
    RankerSpec::OpenRouter("anthropic/claude-haiku-5.5".into())
}

fn mercury() -> RankerSpec {
    RankerSpec::Decision("inception/mercury-decide".into())
}

fn run(r: &Ranker<'_>, src: &str, eligible: &[String]) -> RankReport {
    r.rank(&RankInput {
        file: "three.ax",
        source: src,
        hidden_check: "hidden_grader",
        failing: &names(&["visible_sum"]),
        eligible,
    })
}

fn spectrum() -> Vec<String> {
    names(&["alpha", "beta", "gamma"])
}

fn assert_fell_back(rep: &RankReport, requested: &str, reason_has: &str) {
    assert_eq!(rep.used, "spectrum", "{rep:?}");
    assert_eq!(rep.order, spectrum(), "a fallback keeps the spectrum order");
    let reason = rep.reason.as_deref().expect("a fallback names its reason");
    assert!(
        reason.contains(reason_has),
        "reason `{reason}` should mention `{reason_has}`"
    );
    assert_eq!(
        rep.fallback_line().as_deref(),
        Some(format!("ranker {requested} unavailable: {reason}; using spectrum order").as_str()),
        "a fallback prints exactly one line, in this form"
    );
    assert_eq!(rep.to_json()["reason"], json!(reason));
    assert!(!format!("{rep:?}").contains(KEY), "the key never appears");
}

/// (a) A valid permutation is USED, in the model's order, with its cost and
/// latency recorded and no fallback line.
#[test]
fn rank_a_valid_reorder_is_used() {
    let t = Scripted::new(vec![chat_reply(
        r#"{"ranking": ["gamma", "alpha", "beta"]}"#,
    )]);
    let rep = run(&ranker(haiku(), &t), SRC, &spectrum());
    assert_eq!(rep.used, "openrouter:anthropic/claude-haiku-5.5");
    assert_eq!(rep.requested, rep.used);
    assert_eq!(rep.reason, None);
    assert_eq!(rep.order, names(&["gamma", "alpha", "beta"]));
    assert_eq!(rep.cost_usd, Some(0.0007));
    assert!(rep.latency_ms.is_some());
    assert_eq!(rep.fallback_line(), None);
    let j = rep.to_json();
    assert_eq!(
        j,
        json!({
            "requested": "openrouter:anthropic/claude-haiku-5.5",
            "used": "openrouter:anthropic/claude-haiku-5.5",
            "reason": null,
            "order": ["gamma", "alpha", "beta"],
            "latency_ms": rep.latency_ms,
            "cost_usd": 0.0007,
        })
    );
    // The request was LocRank's: system prompt, model, the endpoint and key.
    let reqs = t.requests();
    assert_eq!(reqs.len(), 1);
    let (url, key, body) = &reqs[0];
    assert_eq!(url, "http://chat.invalid/v1/chat/completions");
    assert_eq!(key, KEY);
    assert_eq!(body["model"], "anthropic/claude-haiku-5.5");
    assert_eq!(body["temperature"], 0);
    assert_eq!(body["messages"][0]["content"], rank::CHAT_SYSTEM);
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(
        user.starts_with("File: three.ax\n\n```axon\nfn alpha("),
        "{user}"
    );
    assert!(user.contains("Failing test: visible_sum\n\n```axon\n@[test]\nfn visible_sum()"));
    assert!(
        user.ends_with("Candidate ids: alpha, beta, gamma\n"),
        "{user}"
    );

    // A reply fenced as a whole is the same answer.
    let t = Scripted::new(vec![chat_reply(
        "```json\n{\"ranking\": [\"beta\", \"gamma\", \"alpha\"]}\n```",
    )]);
    let rep = run(&ranker(haiku(), &t), SRC, &spectrum());
    assert_eq!(rep.order, names(&["beta", "gamma", "alpha"]));
    assert_eq!(rep.reason, None);
}

/// (b) Out of set, repeated, incomplete, malformed: every one keeps the
/// spectrum order and says which.
#[test]
fn rank_an_answer_that_is_not_a_permutation_falls_back_with_its_reason() {
    let cases: &[(&str, &str)] = &[
        (
            r#"{"ranking": ["gamma", "delta", "alpha"]}"#,
            "`delta` is not an eligible candidate",
        ),
        (
            r#"{"ranking": ["gamma", "gamma", "alpha"]}"#,
            "`gamma` is ranked twice",
        ),
        (
            r#"{"ranking": ["gamma", "alpha"]}"#,
            "incomplete ranking: 2 of 3",
        ),
        (r#"{"ranking": []}"#, "empty ranking"),
        (
            r#"{"ranking": ["gamma"], "why": "x"}"#,
            "reply is not {\"ranking\"",
        ),
        (r#"{"ranking": "gamma"}"#, "reply is not {\"ranking\""),
        (
            r#"{"ranking": ["gamma", 3, "beta"]}"#,
            "reply is not {\"ranking\"",
        ),
        (
            r#"{"ranking": ["gamma"], "ranking": ["alpha", "beta", "gamma"]}"#,
            "not one JSON object",
        ),
        ("gamma is the culprit, then alpha", "not one JSON object"),
    ];
    for (content, why) in cases {
        let t = Scripted::new(vec![chat_reply(content)]);
        let rep = run(&ranker(haiku(), &t), SRC, &spectrum());
        assert_fell_back(&rep, "openrouter:anthropic/claude-haiku-5.5", why);
        assert_eq!(rep.cost_usd, Some(0.0007), "a paid call's cost is kept");
    }
    // No message content at all.
    let t = Scripted::new(vec![Ok((200, json!({"choices": []}).to_string()))]);
    let rep = run(&ranker(haiku(), &t), SRC, &spectrum());
    assert_fell_back(
        &rep,
        "openrouter:anthropic/claude-haiku-5.5",
        "no message content",
    );
    // A 200 carrying an error object.
    let t = Scripted::new(vec![Ok((
        200,
        json!({"error": {"code": 402, "message": "no credit"}}).to_string(),
    ))]);
    let rep = run(&ranker(haiku(), &t), SRC, &spectrum());
    assert_fell_back(&rep, "openrouter:anthropic/claude-haiku-5.5", "no credit");
    // A body that is not JSON.
    let t = Scripted::new(vec![Ok((200, "<html>gateway</html>".into()))]);
    let rep = run(&ranker(haiku(), &t), SRC, &spectrum());
    assert_fell_back(
        &rep,
        "openrouter:anthropic/claude-haiku-5.5",
        "response is not JSON",
    );
}

/// (c) An HTTP error or a transport failure (timeout) falls back; an error
/// body that echoes the key does not carry it into the reason.
#[test]
fn rank_http_errors_and_timeouts_fall_back() {
    let t = Scripted::new(vec![Ok((500, format!("upstream exploded; key was {KEY}")))]);
    let rep = run(&ranker(haiku(), &t), SRC, &spectrum());
    assert_fell_back(
        &rep,
        "openrouter:anthropic/claude-haiku-5.5",
        "HTTP 500: upstream exploded",
    );
    assert!(rep.reason.as_deref().unwrap().contains("<redacted>"));

    let t = Scripted::new(vec![Ok((429, "rate limited".into()))]);
    let rep = run(&ranker(mercury(), &t), SRC, &spectrum());
    assert_fell_back(
        &rep,
        "decision:inception/mercury-decide",
        "HTTP 429: rate limited",
    );

    let t = Scripted::new(vec![Err("timed out after 20 s".into())]);
    let rep = run(&ranker(haiku(), &t), SRC, &spectrum());
    assert_fell_back(
        &rep,
        "openrouter:anthropic/claude-haiku-5.5",
        "timed out after 20 s",
    );
    assert!(
        rep.latency_ms.is_some(),
        "the time the attempt took is recorded"
    );
    assert_eq!(rep.cost_usd, None);
}

/// (d) The default ranker without a key resolves to spectrum, and says so.
#[test]
fn rank_without_a_key_the_default_selects_spectrum_with_a_reason() {
    assert_eq!(
        rank::select(None, None).unwrap(),
        haiku(),
        "the default is openrouter:anthropic/claude-haiku-5.5"
    );
    assert_eq!(
        rank::select(None, Some("")).unwrap(),
        haiku(),
        "empty env = unset"
    );
    assert_eq!(
        rank::select(Some("spectrum"), Some("decision:x")).unwrap(),
        RankerSpec::Spectrum,
        "the flag wins over the env"
    );
    assert_eq!(
        rank::select(None, Some("decision:inception/mercury-decide")).unwrap(),
        mercury()
    );
    assert!(rank::select(Some("openrouter:"), None).is_err());
    assert!(rank::select(Some("haiku"), None)
        .unwrap_err()
        .contains("unknown ranker"));
    assert!(rank::select(None, Some("bogus"))
        .unwrap_err()
        .starts_with("CORTEX_RANKER:"));

    let t = Scripted::new(vec![]);
    let mut r = ranker(rank::select(None, None).unwrap(), &t);
    r.client.key = None;
    let rep = run(&r, SRC, &spectrum());
    assert_fell_back(
        &rep,
        "openrouter:anthropic/claude-haiku-5.5",
        "OPENROUTER_API_KEY not set",
    );
    assert_eq!(rep.reason.as_deref(), Some("OPENROUTER_API_KEY not set"));
    assert!(t.requests().is_empty(), "nothing is sent without a key");

    // A build without an HTTP client says that, rather than pretending.
    let mut r = ranker(haiku(), &t);
    r.client.transport = None;
    let rep = run(&r, SRC, &spectrum());
    assert_fell_back(
        &rep,
        "openrouter:anthropic/claude-haiku-5.5",
        "--features ai",
    );

    // Spectrum itself is not a fallback: no reason, no line, no cost.
    let rep = run(&ranker(RankerSpec::Spectrum, &t), SRC, &spectrum());
    assert_eq!(
        (rep.used.as_str(), rep.reason.as_deref()),
        ("spectrum", None)
    );
    assert_eq!(rep.fallback_line(), None);
    assert_eq!(rep.cost_usd, Some(0.0));
    // Nor is one candidate: there is nothing to reorder and nothing is sent.
    let rep = run(&ranker(haiku(), &t), SRC, &names(&["gamma"]));
    assert_eq!(rep.used, "spectrum");
    assert_eq!(rep.fallback_line(), None);
    assert!(t.requests().is_empty());
}

/// (e) What the model is shown never contains the hidden check — not its
/// name, not its body, not the comment above it — for either ranker. A file
/// that mentions the hidden check elsewhere is not sent at all.
#[test]
fn rank_the_request_never_carries_the_hidden_check() {
    let t = Scripted::new(vec![
        chat_reply(r#"{"ranking": ["gamma", "alpha", "beta"]}"#),
        decision_reply("gamma", json!({"alpha": 0.1, "beta": 0.1, "gamma": 0.8})),
    ]);
    run(&ranker(haiku(), &t), SRC, &spectrum());
    run(&ranker(mercury(), &t), SRC, &spectrum());
    let reqs = t.requests();
    assert_eq!(reqs.len(), 2);
    for (_, _, body) in &reqs {
        let wire = body.to_string();
        for secret in ["hidden_grader", "SECRET_BODY_MARKER", "SECRET_COMMENT"] {
            assert!(!wire.contains(secret), "`{secret}` leaked into {wire}");
        }
        assert!(wire.contains("visible_sum"), "the visible check IS shown");
        for c in ["fn alpha(", "fn beta(", "fn gamma("] {
            assert!(wire.contains(c), "candidate `{c}` is shown");
        }
    }

    // The hidden check named anywhere else: refused before anything is sent.
    let leaky = SRC.replace(
        "fn beta(n: i64) -> i64 { n * 2 }",
        "// see hidden_grader\nfn beta(n: i64) -> i64 { n * 2 }",
    );
    let t = Scripted::new(vec![]);
    let rep = run(&ranker(haiku(), &t), &leaky, &spectrum());
    assert_fell_back(
        &rep,
        "openrouter:anthropic/claude-haiku-5.5",
        "hidden check's name",
    );
    assert!(
        t.requests().is_empty(),
        "nothing is sent when it cannot be hidden"
    );
}

/// (f) `decision:` orders by probability, ties broken by spectrum order, and
/// refuses probabilities that do not describe exactly the eligible set.
#[test]
fn rank_decision_orders_by_probability_with_spectrum_ties() {
    let t = Scripted::new(vec![decision_reply(
        "beta",
        json!({"alpha": 0.2, "beta": 0.4, "gamma": 0.4}),
    )]);
    let rep = run(&ranker(mercury(), &t), SRC, &spectrum());
    assert_eq!(rep.reason, None, "{rep:?}");
    assert_eq!(rep.used, "decision:inception/mercury-decide");
    assert_eq!(
        rep.order,
        names(&["beta", "gamma", "alpha"]),
        "beta and gamma tie at 0.4: spectrum order between them"
    );
    assert_eq!(rep.cost_usd, Some(0.00004));
    let (url, _, body) = &t.requests()[0];
    assert_eq!(url, "http://decide.invalid/alpha/decisions");
    assert_eq!(body["model"], "inception/mercury-decide");
    assert_eq!(body["state"]["file"], "three.ax");
    assert_eq!(
        body["state"]["source_note"],
        "complete file (hidden check removed)"
    );
    assert_eq!(body["state"]["failing_test"]["name"], "visible_sum");
    let q = &body["questions"]["buggy_function"];
    assert_eq!(q["type"], "choice");
    assert_eq!(q["instructions"], rank::DECISION_INSTRUCTIONS);
    assert_eq!(
        q["criteria"],
        json!({"alpha": "fn alpha(n: i64) -> i64", "beta": "fn beta(n: i64) -> i64", "gamma": "fn gamma(n: i64) -> i64"})
    );

    let bad: &[(&str, Value, &str)] = &[
        (
            "delta",
            json!({"alpha": 0.2, "beta": 0.4, "gamma": 0.4}),
            "`delta` is not an eligible",
        ),
        (
            "beta",
            json!({"alpha": 0.2, "beta": 0.4, "delta": 0.4}),
            "`delta` is not an eligible",
        ),
        (
            "beta",
            json!({"alpha": 0.5, "beta": 0.5}),
            "probabilities omit `gamma`",
        ),
        (
            "beta",
            json!({"alpha": 0.2, "beta": 0.2, "gamma": 0.2}),
            "sum to 0.600",
        ),
        (
            "beta",
            json!({"alpha": -0.1, "beta": 0.6, "gamma": 0.5}),
            "not in [0, 1]",
        ),
        ("beta", json!(null), "no probabilities"),
    ];
    for (choice, probs, why) in bad {
        let t = Scripted::new(vec![decision_reply(choice, probs.clone())]);
        let rep = run(&ranker(mercury(), &t), SRC, &spectrum());
        assert_fell_back(&rep, "decision:inception/mercury-decide", why);
    }
    let t = Scripted::new(vec![Ok((200, json!({"answers": {}}).to_string()))]);
    let rep = run(&ranker(mercury(), &t), SRC, &spectrum());
    assert_fell_back(
        &rep,
        "decision:inception/mercury-decide",
        "no answer for `buggy_function`",
    );
}

/// (g) Escalation advice is derived from the ranking alone: `cheap` whenever
/// a ranker answered (its top-1 is the walk's first candidate), `cheap` when
/// localization found one function, `unavailable` when no model localized
/// anything. A `decision:` ranker's top-1 probability and margin ride along
/// as data.
#[test]
fn escalation_follows_the_route_rule() {
    // Chat ranker: no probabilities, and none are needed.
    let t = Scripted::new(vec![chat_reply(
        r#"{"ranking": ["gamma", "alpha", "beta"]}"#,
    )]);
    let rep = run(&ranker(haiku(), &t), SRC, &spectrum());
    let e = rank::escalation(&rep, 3);
    assert_eq!(e.advice, Advice::Cheap);
    assert!(e.basis.contains("put `gamma` first"), "{}", e.basis);
    assert_eq!((e.top_p, e.margin), (None, None));
    assert_eq!(
        e.to_json(),
        json!({"advice": "cheap", "basis": e.basis, "top_p": null, "margin": null})
    );
    assert!(e.line().starts_with("escalation: cheap ("), "{}", e.line());

    // Decision ranker: top-1 p and the margin to #2, no threshold applied.
    let t = Scripted::new(vec![decision_reply(
        "beta",
        json!({"alpha": 0.1, "beta": 0.6, "gamma": 0.3}),
    )]);
    let rep = run(&ranker(mercury(), &t), SRC, &spectrum());
    assert_eq!(rep.probabilities, Some(vec![0.6, 0.3, 0.1]));
    let e = rank::escalation(&rep, 3);
    assert_eq!(e.advice, Advice::Cheap);
    assert_eq!(e.top_p, Some(0.6));
    assert!((e.margin.unwrap() - 0.3).abs() < 1e-9);
    assert!(
        e.basis.contains("top-1 p=0.600, margin to #2 0.300"),
        "{}",
        e.basis
    );

    // A fallback: no model answer, so no advice.
    let t = Scripted::new(vec![Ok((500, "{}".into()))]);
    let rep = run(&ranker(haiku(), &t), SRC, &spectrum());
    let e = rank::escalation(&rep, 3);
    assert_eq!(e.advice, Advice::Unavailable);
    assert!(e.basis.contains("HTTP 500"), "{}", e.basis);
    assert!(e.line().starts_with("escalation unavailable: "));
    assert_eq!(e.to_json()["advice"], "unavailable");

    // Spectrum by name: no model asked.
    let rep = run(&ranker(RankerSpec::Spectrum, &t), SRC, &spectrum());
    let e = rank::escalation(&rep, 3);
    assert_eq!(e.advice, Advice::Unavailable);
    assert!(e.basis.contains("asks no model"), "{}", e.basis);

    // `--candidates 1` over three located functions: nothing was reordered.
    let rep = run(&ranker(haiku(), &t), SRC, &names(&["alpha"]));
    let e = rank::escalation(&rep, 3);
    assert_eq!(e.advice, Advice::Unavailable);
    assert!(e.basis.contains("one eligible candidate"), "{}", e.basis);

    // Localization found exactly one function: cheap, no model needed.
    let e = rank::escalation(&rep, 1);
    assert_eq!(e.advice, Advice::Cheap);
    assert!(
        e.basis.contains("found one function, `alpha`"),
        "{}",
        e.basis
    );
    assert!(t.requests().len() == 1, "only the HTTP 500 call was made");
}

/// The prompt is capped; over the cap the file gives way to the candidates,
/// shortened, and every candidate is still named.
#[test]
fn rank_the_prompt_is_capped_by_truncating_candidates_first() {
    let filler: String = (0..4000).map(|i| format!("    let x{i} = {i}\n")).collect();
    let big = SRC
        .replace(
            "fn alpha(n: i64) -> i64 { n * 2 }",
            &format!("fn alpha(n: i64) -> i64 {{\n{filler}    n * 2\n}}"),
        )
        .replace(
            "fn beta(n: i64) -> i64 { n * 2 }",
            &format!("fn beta(n: i64) -> i64 {{\n{filler}    n * 2\n}}"),
        );
    assert!(big.len() > 2 * MAX_PROMPT_CHARS);
    let t = Scripted::new(vec![chat_reply(
        r#"{"ranking": ["gamma", "alpha", "beta"]}"#,
    )]);
    let rep = run(&ranker(haiku(), &t), &big, &spectrum());
    assert_eq!(rep.reason, None, "{rep:?}");
    let body = &t.requests()[0].2;
    let sys = body["messages"][0]["content"].as_str().unwrap();
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(
        sys.len() + user.len() <= MAX_PROMPT_CHARS,
        "{}",
        sys.len() + user.len()
    );
    assert!(user.contains("EXCERPT of a"), "the excerpt is labelled");
    assert!(user.contains("// … truncated"));
    assert!(
        user.contains("fn visible_sum() { assert_eq(alpha(1) + beta(1) + gamma(1), 6) }"),
        "the failing check is shown whole"
    );
    assert!(
        user.contains("fn gamma(n: i64) -> i64 { n }"),
        "short candidates stay whole"
    );
    assert!(!user.contains("hidden_grader"));
}

/// The same failures over a real socket and the workspace's HTTP client.
#[cfg(feature = "ai")]
mod over_http {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// One-request loopback server: returns its URL and a handle yielding the
    /// raw request it received.
    fn serve(
        status: u16,
        body: String,
        delay: Duration,
    ) -> (String, std::thread::JoinHandle<String>) {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/api/v1/chat/completions", l.local_addr().unwrap());
        let h = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = s.read(&mut chunk).unwrap_or(0);
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&buf);
                if let Some(end) = text.find("\r\n\r\n") {
                    let len = text[..end]
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    if buf.len() >= end + 4 + len {
                        break;
                    }
                }
            }
            std::thread::sleep(delay);
            let _ = write!(
                s,
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            String::from_utf8_lossy(&buf).to_string()
        });
        (url, h)
    }

    fn http_ranker(url: &str, timeout: Duration) -> Ranker<'static> {
        Ranker {
            spec: haiku(),
            client: Client {
                key: Some(KEY.to_string()),
                transport: Some(&axon_cortex::openrouter::HttpTransport),
                chat_url: url.to_string(),
                decisions_url: url.to_string(),
                timeout,
            },
        }
    }

    #[test]
    fn rank_over_http_reorders_and_sends_no_hidden_check() {
        let (_, reply) = chat_reply(r#"{"ranking": ["gamma", "beta", "alpha"]}"#).unwrap();
        let (url, h) = serve(200, reply, Duration::ZERO);
        let rep = run(
            &http_ranker(&url, Duration::from_secs(20)),
            SRC,
            &spectrum(),
        );
        assert_eq!(rep.reason, None, "{rep:?}");
        assert_eq!(rep.order, names(&["gamma", "beta", "alpha"]));
        let raw = h.join().unwrap();
        assert!(raw.to_ascii_lowercase().contains(&format!(
            "authorization: bearer {}",
            KEY.to_ascii_lowercase()
        )));
        assert!(!raw.contains("hidden_grader") && !raw.contains("SECRET_BODY_MARKER"));
    }

    #[test]
    fn rank_over_http_a_500_and_a_timeout_fall_back() {
        let (url, h) = serve(500, r#"{"error":"boom"}"#.into(), Duration::ZERO);
        let rep = run(
            &http_ranker(&url, Duration::from_secs(20)),
            SRC,
            &spectrum(),
        );
        assert_fell_back(&rep, "openrouter:anthropic/claude-haiku-5.5", "HTTP 500");
        h.join().unwrap();

        let (_, reply) = chat_reply(r#"{"ranking": ["gamma", "beta", "alpha"]}"#).unwrap();
        let (url, h) = serve(200, reply, Duration::from_millis(1500));
        let rep = run(
            &http_ranker(&url, Duration::from_millis(300)),
            SRC,
            &spectrum(),
        );
        assert_fell_back(&rep, "openrouter:anthropic/claude-haiku-5.5", "timed out");
        assert!(rep.latency_ms.unwrap() < 1400, "the bound held: {rep:?}");
        h.join().unwrap();
    }
}
