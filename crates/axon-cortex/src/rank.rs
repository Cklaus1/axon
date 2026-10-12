//! Reordering `cortex repair`'s candidates with a model, without letting the
//! model choose what is eligible.
//!
//! `locate::rank` orders the functions the failing checks reach by spectrum
//! (Ochiai). On the PoC corpus that puts the broken function first in 25 of 60
//! tasks; a cheap model reordering the same top-6 put it first in 60 of 60
//! (`benchmarks/locrank/`), and a decision model in 53 of 57
//! (`crates/axon-reflex/benchmarks/decisions/`). Every wrong candidate ahead of
//! the truth costs the walk at least one proposal, so the order is where the
//! time goes.
//!
//! # What a ranker may do
//!
//! Only PERMUTE the eligible list it is handed — the same top-`--candidates`
//! names the walk would try in spectrum order. [`validate_ranking`] applies the
//! abstain rules of `axon_reflex::shortlist::decide` (nothing outside the set,
//! nothing twice, nothing empty) plus one more: every eligible name must be
//! present, because dropping a candidate here would silently narrow what the
//! walk tries. (`axon-reflex` depends on this crate, so the rules are mirrored
//! here and an agreement test there feeds both the same outputs.)
//!
//! Anything else — no key, no HTTP client in this build, a transport error, a
//! timeout, a non-200, an answer that is not exactly the expected shape — keeps
//! the spectrum order and SAYS so: [`RankReport::fallback_line`] for stderr and
//! [`RankReport::to_json`] for `--json`. A ranker never changes which check
//! adjudicates, the exit code, or the verdict.
//!
//! # What a ranker sees
//!
//! What Cortex itself holds: the file under repair with the hidden check's
//! definition cut out, the visible failing checks, and the eligible names with
//! their bodies. Never the hidden check, and the request is refused (spectrum
//! order, with a reason) if the hidden check's name still appears anywhere in
//! it. The prompt is capped at [`MAX_PROMPT_CHARS`]; over the cap the file is
//! replaced by the candidate functions alone, truncated evenly, before any
//! failing check is shortened.

use crate::openrouter::{model_id, quote, Client};
use serde_json::{json, Value};

/// Ranker selection when `--ranker` is absent.
pub const RANKER_ENV: &str = "CORTEX_RANKER";
/// The model behind the default ranker: 60/60 truth-first on LocRank.
pub const DEFAULT_CHAT_MODEL: &str = "anthropic/claude-haiku-5.5";
/// Upper bound on the characters of one ranking request's text.
pub const MAX_PROMPT_CHARS: usize = 24_000;
/// No candidate is cut below this many characters while a failing check could
/// be shortened instead.
const MIN_CANDIDATE_CHARS: usize = 400;

/// LocRank's system prompt, verbatim: the one measured at 60/60.
pub const CHAT_SYSTEM: &str =
    "You localize a defect. Exactly one function in the Axon source file below \
contains an injected bug that makes the failing test fail. Rank the \
candidate functions from most to least likely to contain the bug.\n\
Reply with ONLY one JSON object and nothing else, of this exact shape:\n\
{\"ranking\": [\"<candidate id>\", \"...\"]}\n\
Use only the candidate ids given, each at most once. No prose, no explanation.";

/// DecisionBench's `loc` question id and instructions, verbatim.
pub const DECISION_QID: &str = "buggy_function";
pub const DECISION_INSTRUCTIONS: &str =
    "Exactly one candidate function in `source` contains an injected bug that makes \
`failing_test` fail. Which candidate contains the bug?";

/// Which ranker a run asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RankerSpec {
    /// `locate::rank`'s order, untouched. No network.
    Spectrum,
    /// `openrouter:MODEL` — a chat completion asked for a JSON ranking.
    OpenRouter(String),
    /// `decision:MODEL` — OpenRouter's decisions API, one `choice` question.
    Decision(String),
}

impl RankerSpec {
    pub fn parse(s: &str) -> Result<Self, String> {
        let model = |m: &str| model_id("ranker", s, m);
        if s == "spectrum" {
            Ok(RankerSpec::Spectrum)
        } else if let Some(m) = s.strip_prefix("openrouter:") {
            Ok(RankerSpec::OpenRouter(model(m)?))
        } else if let Some(m) = s.strip_prefix("decision:") {
            Ok(RankerSpec::Decision(model(m)?))
        } else {
            Err(format!(
                "unknown ranker `{s}` (expected spectrum | openrouter:MODEL | decision:MODEL)"
            ))
        }
    }

    pub fn label(&self) -> String {
        match self {
            RankerSpec::Spectrum => "spectrum".into(),
            RankerSpec::OpenRouter(m) => format!("openrouter:{m}"),
            RankerSpec::Decision(m) => format!("decision:{m}"),
        }
    }
}

/// `--ranker` wins, then a non-empty `CORTEX_RANKER`, then the default:
/// `openrouter:` + [`DEFAULT_CHAT_MODEL`]. Without a key that default resolves
/// to the spectrum order at ranking time, with the reason recorded — so a run
/// with no key behaves as before, and says why.
pub fn select(flag: Option<&str>, env: Option<&str>) -> Result<RankerSpec, String> {
    if let Some(f) = flag {
        return RankerSpec::parse(f).map_err(|e| format!("--ranker: {e}"));
    }
    match env.filter(|s| !s.is_empty()) {
        Some(e) => RankerSpec::parse(e).map_err(|e| format!("{RANKER_ENV}: {e}")),
        None => Ok(RankerSpec::OpenRouter(DEFAULT_CHAT_MODEL.into())),
    }
}

/// The ranking rule, shared by every ranker: `proposed` must be a
/// permutation of `eligible`. The first three refusals are
/// `axon_reflex::shortlist::decide`'s (empty, not eligible, duplicate); the
/// last is this caller's, because an omitted candidate would be one the walk
/// never tries.
pub fn validate_ranking(eligible: &[String], proposed: &[String]) -> Result<Vec<String>, String> {
    if proposed.is_empty() {
        return Err("empty ranking".into());
    }
    let mut seen: Vec<&str> = Vec::with_capacity(proposed.len());
    for c in proposed {
        if !eligible.contains(c) {
            return Err(format!("`{c}` is not an eligible candidate"));
        }
        if seen.contains(&c.as_str()) {
            return Err(format!("`{c}` is ranked twice"));
        }
        seen.push(c);
    }
    if proposed.len() != eligible.len() {
        return Err(format!(
            "incomplete ranking: {} of {} candidates",
            proposed.len(),
            eligible.len()
        ));
    }
    Ok(proposed.to_vec())
}

/// What one ranking step did. Serialized under `ranker` in `cortex repair
/// --json`.
#[derive(Clone, Debug, PartialEq)]
pub struct RankReport {
    pub requested: String,
    pub used: String,
    /// `None` exactly when `used == requested`.
    pub reason: Option<String>,
    /// The order the walk will follow.
    pub order: Vec<String>,
    /// The ranking call's wall time, when one was made.
    pub latency_ms: Option<u64>,
    /// As reported by OpenRouter (`usage.cost`); `0` for spectrum; `None` when
    /// unknown.
    pub cost_usd: Option<f64>,
    /// A `decision:` ranker's probabilities, in `order`. `None` for every
    /// other ranker and for a fallback.
    pub probabilities: Option<Vec<f64>>,
    /// The requested ranker was unavailable (as opposed to: there was nothing
    /// to reorder).
    fell_back: bool,
}

impl RankReport {
    pub fn to_json(&self) -> Value {
        json!({
            "requested": self.requested,
            "used": self.used,
            "reason": self.reason,
            "order": self.order,
            "latency_ms": self.latency_ms,
            "cost_usd": self.cost_usd,
        })
    }

    /// The one stderr line a fallback prints. `None` when nothing fell back.
    pub fn fallback_line(&self) -> Option<String> {
        let reason = self.reason.as_deref().filter(|_| self.fell_back)?;
        Some(format!(
            "ranker {} unavailable: {reason}; using spectrum order",
            self.requested
        ))
    }

    pub fn fell_back(&self) -> bool {
        self.fell_back
    }
}

/// Everything a ranking request may be built from. `source` is the file as it
/// is on disk, hidden check included; the view strips it.
pub struct RankInput<'a> {
    pub file: &'a str,
    pub source: &'a str,
    pub hidden_check: &'a str,
    /// The VISIBLE failing checks (`Runner::check_outcomes` already excludes
    /// the hidden one).
    pub failing: &'a [String],
    /// Spectrum order, already cut to `--candidates`.
    pub eligible: &'a [String],
}

/// The bounded, hidden-check-free material a request is rendered from.
#[derive(Clone, Debug)]
pub struct RankView {
    pub file: String,
    /// The whole file minus the hidden check, or an excerpt of the candidates.
    pub source: String,
    pub source_note: String,
    /// `(name, text)` of each visible failing check.
    pub failing: Vec<(String, String)>,
    /// `(name, signature)` in spectrum order.
    pub candidates: Vec<(String, String)>,
}

/// A configured ranking step. Deliberately not `Debug`: its client holds the
/// key.
pub struct Ranker<'t> {
    pub spec: RankerSpec,
    pub client: Client<'t>,
}

impl Ranker<'static> {
    /// The production configuration ([`Client::from_env`]).
    pub fn from_env(spec: RankerSpec) -> Self {
        Ranker {
            spec,
            client: Client::from_env(),
        }
    }
}

impl Ranker<'_> {
    /// Order `input.eligible`. Never fails: every failure is a report that
    /// keeps the spectrum order and names why.
    pub fn rank(&self, input: &RankInput<'_>) -> RankReport {
        let requested = self.spec.label();
        let spectrum = input.eligible.to_vec();
        let report = |used: &str,
                      reason: Option<String>,
                      order: Vec<String>,
                      latency_ms: Option<u64>,
                      cost_usd: Option<f64>,
                      probabilities: Option<Vec<f64>>,
                      fell_back: bool| RankReport {
            requested: requested.clone(),
            used: used.to_string(),
            reason: reason.map(|r| self.client.redact(&r)),
            order,
            latency_ms,
            cost_usd,
            probabilities,
            fell_back,
        };
        let fallback = |reason: String, latency_ms: Option<u64>, cost_usd: Option<f64>| {
            report(
                "spectrum",
                Some(reason),
                spectrum.clone(),
                latency_ms,
                cost_usd,
                None,
                true,
            )
        };

        if self.spec == RankerSpec::Spectrum {
            return report(
                "spectrum",
                None,
                spectrum.clone(),
                None,
                Some(0.0),
                None,
                false,
            );
        }
        if spectrum.len() < 2 {
            return report(
                "spectrum",
                Some("one eligible candidate: nothing to reorder".into()),
                spectrum.clone(),
                None,
                Some(0.0),
                None,
                false,
            );
        }
        if let Err(e) = self.client.ready() {
            return fallback(e, None, None);
        }
        let view = match build_view(input) {
            Ok(v) => v,
            Err(e) => return fallback(e, None, None),
        };
        let (url, body) = match &self.spec {
            RankerSpec::OpenRouter(m) => {
                (&self.client.chat_url, chat_body(m, &view, input.eligible))
            }
            RankerSpec::Decision(m) => (&self.client.decisions_url, decision_body(m, &view)),
            RankerSpec::Spectrum => unreachable!("handled above"),
        };

        let reply = match self.client.post(url, &body) {
            Ok(r) => r,
            Err(f) => return fallback(f.reason, f.latency_ms, f.cost_usd),
        };
        let (latency, cost) = (Some(reply.latency_ms), reply.cost_usd);
        let proposed = match &self.spec {
            RankerSpec::OpenRouter(_) => parse_chat(&reply.body).map(|o| (o, None)),
            RankerSpec::Decision(_) => parse_decision(&reply.body, input.eligible).map(|ps| {
                let (o, p): (Vec<String>, Vec<f64>) = ps.into_iter().unzip();
                (o, Some(p))
            }),
            RankerSpec::Spectrum => unreachable!("handled above"),
        };
        match proposed
            .and_then(|(p, probs)| validate_ranking(input.eligible, &p).map(|order| (order, probs)))
        {
            Ok((order, probs)) => report(&requested, None, order, latency, cost, probs, false),
            Err(e) => fallback(e, latency, cost),
        }
    }
}

// ---------------------------------------------------------------- escalation

/// What [`escalation`] advises.
///
/// There is no `strong`: the rule below is the only derivation with
/// evidence, and in a walk that follows the ranker it cannot produce one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Advice {
    /// The cheap generator's first proposal is predicted to be verified.
    Cheap,
    /// No model localized anything this run, so the rule has no input.
    Unavailable,
}

/// Cheap-vs-strong advice derived from the ranking step, with no model call
/// of its own. Advice only: the generator stays the operator's choice.
#[derive(Clone, Debug, PartialEq)]
pub struct Escalation {
    pub advice: Advice,
    pub basis: String,
    /// `decision:` rankers only: the first candidate's probability and its
    /// margin over the second. Recorded so a later benchmark can calibrate a
    /// `strong` trigger; no threshold is applied to them here.
    pub top_p: Option<f64>,
    pub margin: Option<f64>,
}

impl Escalation {
    pub fn to_json(&self) -> Value {
        json!({
            "advice": match self.advice {
                Advice::Cheap => "cheap",
                Advice::Unavailable => "unavailable",
            },
            "basis": self.basis,
            "top_p": self.top_p,
            "margin": self.margin,
        })
    }

    /// The one stderr line.
    pub fn line(&self) -> String {
        match self.advice {
            Advice::Cheap => format!("escalation: cheap ({})", self.basis),
            Advice::Unavailable => format!("escalation unavailable: {}", self.basis),
        }
    }
}

/// DecisionBench's route derivation (`analyse.py` `route_via_loc`): the cheap
/// model's FIRST proposal is predicted verified iff the model's own
/// localization (its top-1) is the candidate the walk tries first; a task
/// whose localization found ONE function takes the cheap path. Asked
/// directly, decision models scored 0.36-0.76 on the routing question; the
/// same models' loc answers through this rule scored 0.85-0.95 (11 of 14),
/// claude-haiku-5.5 0.92. The rule reads only the top-1, so the chat
/// permutation ranker (no probabilities) is covered: its top-1 is a loc
/// answer, measured 60/60 on LocRank with the very prompt it sends.
///
/// Here the walk follows the ranker's order, so whenever a ranker answered,
/// its top-1 IS the first candidate and the advice is `cheap`. (Comparing to
/// the spectrum's top instead would predict a spectrum-order walk that does
/// not run.) No ranker answer — spectrum by name, a fallback, or a list cut to
/// one name by `--candidates` — leaves the rule without input: `unavailable`.
///
/// `located` is how many functions the spectrum ranked before `--candidates`
/// cut the list.
pub fn escalation(report: &RankReport, located: usize) -> Escalation {
    let rule = "DecisionBench route rule: cheap iff the model's top candidate is the one \
                the walk tries first";
    let none = |basis: String| Escalation {
        advice: Advice::Unavailable,
        basis,
        top_p: None,
        margin: None,
    };
    if located == 1 {
        return Escalation {
            advice: Advice::Cheap,
            basis: format!(
                "localization found one function, `{}`; DecisionBench route rule: \
                 a one-candidate task takes the cheap path",
                report.order.first().map_or("", String::as_str)
            ),
            top_p: None,
            margin: None,
        };
    }
    if report.used == "spectrum" {
        return none(match (&report.reason, report.fell_back) {
            (Some(r), true) => format!(
                "the ranker {} gave no localization ({r}); {rule}",
                report.requested
            ),
            (Some(r), false) => format!("no model ranked the candidates: {r}; {rule}"),
            (None, _) => format!("spectrum order asks no model; {rule}"),
        });
    }
    let top = report.order.first().map_or("", String::as_str);
    let (top_p, margin) = match report.probabilities.as_deref() {
        Some([p1, p2, ..]) => (Some(*p1), Some(p1 - p2)),
        Some([p1]) => (Some(*p1), None),
        _ => (None, None),
    };
    let probs = match (top_p, margin) {
        (Some(p), Some(m)) => format!("; top-1 p={p:.3}, margin to #2 {m:.3}"),
        _ => String::new(),
    };
    Escalation {
        advice: Advice::Cheap,
        basis: format!(
            "{} put `{top}` first and the walk tries it first; {rule}{probs}",
            report.used
        ),
        top_p,
        margin,
    }
}

// ---------------------------------------------------------------- requests

/// The chat request LocRank measured: temperature 0, LocRank's system prompt,
/// the file, the failing check, the candidate ids.
pub fn chat_body(model: &str, view: &RankView, eligible: &[String]) -> Value {
    json!({
        "model": model,
        "temperature": 0,
        "max_tokens": 4096,
        "messages": [
            {"role": "system", "content": CHAT_SYSTEM},
            {"role": "user", "content": chat_prompt(view, eligible)},
        ],
        "usage": {"include": true},
    })
}

/// LocRank's user prompt. With one failing check it is byte-for-byte the
/// measured one.
pub fn chat_prompt(view: &RankView, eligible: &[String]) -> String {
    prompt_with(
        &view.file,
        &view.source_note,
        &view.source,
        &view.failing,
        eligible,
    )
}

fn prompt_with(
    file: &str,
    note: &str,
    source: &str,
    failing: &[(String, String)],
    eligible: &[String],
) -> String {
    let header = if note == FULL_NOTE {
        format!("File: {file}")
    } else {
        format!("File: {file} ({note})")
    };
    let names: Vec<&str> = failing.iter().map(|(n, _)| n.as_str()).collect();
    let label = if failing.len() == 1 {
        "Failing test"
    } else {
        "Failing tests"
    };
    let bodies: Vec<&str> = failing.iter().map(|(_, b)| b.trim_end()).collect();
    format!(
        "{header}\n\n```axon\n{source}\n```\n\n{label}: {}\n\n```axon\n{}\n```\n\nCandidate ids: {}\n",
        names.join(", "),
        bodies.join("\n\n"),
        eligible.join(", ")
    )
}

/// DecisionBench's `loc` request: `{model, state, questions}` with one
/// `choice` question whose options are the eligible names.
pub fn decision_body(model: &str, view: &RankView) -> Value {
    let mut state = json!({
        "file": view.file,
        "source_note": view.source_note,
        "source": view.source,
    });
    if let Some((name, code)) = view.failing.first() {
        state["failing_test"] = json!({"name": name, "code": code.trim_end()});
    }
    if view.failing.len() > 1 {
        state["also_failing"] = Value::Array(
            view.failing[1..]
                .iter()
                .map(|(n, c)| json!({"name": n, "code": c.trim_end()}))
                .collect(),
        );
    }
    let criteria: serde_json::Map<String, Value> = view
        .candidates
        .iter()
        .map(|(n, sig)| (n.clone(), Value::String(sig.clone())))
        .collect();
    json!({
        "model": model,
        "state": state,
        "questions": {
            DECISION_QID: {
                "type": "choice",
                "instructions": DECISION_INSTRUCTIONS,
                "criteria": criteria,
            }
        },
    })
}

// ---------------------------------------------------------------- answers

/// `{"ranking": [ids]}`, bare or as the WHOLE reply inside one fence — the
/// shape LocRank accepted. Anything else is malformed.
pub fn parse_chat(resp: &Value) -> Result<Vec<String>, String> {
    let content = resp["choices"][0]["message"]["content"]
        .as_str()
        .ok_or("no message content in the reply")?;
    let mut t = content.trim();
    if let Some(rest) = t.strip_prefix("```") {
        if let (Some((_, inner)), true) = (rest.split_once('\n'), rest.ends_with("```")) {
            t = inner[..inner.len() - 3].trim();
        }
    }
    let v: Value = crate::parse_strict(t)
        .map_err(|_| format!("reply is not one JSON object: {}", quote(content)))?;
    let shape = || format!("reply is not {{\"ranking\": [names]}}: {}", quote(content));
    let obj = v.as_object().ok_or_else(shape)?;
    if obj.len() != 1 {
        return Err(shape());
    }
    let list = obj
        .get("ranking")
        .and_then(Value::as_array)
        .ok_or_else(shape)?;
    list.iter()
        .map(|x| x.as_str().map(str::to_string).ok_or_else(shape))
        .collect()
}

/// `answers.buggy_function` → every option with its probability, highest
/// first, ties kept in spectrum order. The probabilities must cover exactly
/// the options, each in [0, 1], summing to 1 ± 0.05 (DecisionBench's
/// validation).
pub fn parse_decision(resp: &Value, eligible: &[String]) -> Result<Vec<(String, f64)>, String> {
    let ans = resp["answers"]
        .get(DECISION_QID)
        .and_then(Value::as_object)
        .ok_or_else(|| format!("no answer for `{DECISION_QID}`"))?;
    match ans.get("type") {
        None | Some(Value::Null) => {}
        Some(Value::String(t)) if t == "choice" => {}
        Some(t) => return Err(format!("answer type {t} is not `choice`")),
    }
    let choice = ans
        .get("choice")
        .and_then(Value::as_str)
        .ok_or("answer has no choice")?;
    if !eligible.iter().any(|e| e == choice) {
        return Err(format!("`{choice}` is not an eligible candidate"));
    }
    let probs = ans
        .get("probabilities")
        .and_then(Value::as_object)
        .ok_or("answer has no probabilities")?;
    let mut sum = 0.0;
    for (k, v) in probs {
        if !eligible.contains(k) {
            return Err(format!("`{k}` is not an eligible candidate"));
        }
        match v.as_f64() {
            Some(p) if p.is_finite() && (0.0..=1.0).contains(&p) => sum += p,
            _ => return Err(format!("probability for `{k}` is not in [0, 1]: {v}")),
        }
    }
    if let Some(missing) = eligible.iter().find(|e| !probs.contains_key(e.as_str())) {
        return Err(format!("probabilities omit `{missing}`"));
    }
    if (sum - 1.0).abs() > 0.05 {
        return Err(format!("probabilities sum to {sum:.3}"));
    }
    let mut order: Vec<(String, f64)> = eligible
        .iter()
        .map(|e| (e.clone(), probs[e.as_str()].as_f64().unwrap_or(0.0)))
        .collect();
    // Stable: equal probabilities keep spectrum order.
    order.sort_by(|a, b| b.1.total_cmp(&a.1));
    Ok(order)
}

// ---------------------------------------------------------------- the view

const FULL_NOTE: &str = "complete file (hidden check removed)";

/// Build the request material from `input`, or say why it cannot be shown.
pub fn build_view(input: &RankInput<'_>) -> Result<RankView, String> {
    let src = without_check(input.source, input.hidden_check);
    let mut failing = Vec::new();
    for name in input.failing {
        if name == input.hidden_check {
            continue;
        }
        if let Some((a, b)) = fn_span(&src, name) {
            failing.push((name.clone(), src[a..b].trim_end().to_string()));
        }
    }
    if failing.is_empty() {
        return Err(format!(
            "no visible failing check is defined in {}",
            input.file
        ));
    }
    let mut candidates = Vec::new();
    let mut texts = Vec::new();
    for name in input.eligible {
        let (a, b) = fn_span(&src, name)
            .ok_or_else(|| format!("candidate `{name}` is not defined in {}", input.file))?;
        candidates.push((name.clone(), signature(&src[a..b], name)));
        texts.push(src[a..b].trim_end().to_string());
    }

    // Over the cap: the candidates alone, then shortened evenly, then — only
    // if they are already at the floor — the failing checks too.
    let fits = |source: &str, note: &str, failing: &[(String, String)]| {
        CHAT_SYSTEM.len() + prompt_with(input.file, note, source, failing, input.eligible).len()
            <= MAX_PROMPT_CHARS
    };
    let (source, note) = if fits(&src, FULL_NOTE, &failing) {
        (src.clone(), FULL_NOTE.to_string())
    } else {
        let note = format!(
            "EXCERPT of a {}-character file: only the candidate functions are shown",
            src.len()
        );
        let joined = texts.join("\n\n");
        if fits(&joined, &note, &failing) {
            (joined, note)
        } else {
            let overhead = CHAT_SYSTEM.len()
                + prompt_with(input.file, &note, "", &failing, input.eligible).len();
            let share =
                (MAX_PROMPT_CHARS.saturating_sub(overhead) / texts.len()).max(MIN_CANDIDATE_CHARS);
            let cut: Vec<String> = texts.iter().map(|t| truncate(t, share)).collect();
            let joined = cut.join("\n\n");
            if !fits(&joined, &note, &failing) {
                let room = MAX_PROMPT_CHARS.saturating_sub(
                    CHAT_SYSTEM.len()
                        + prompt_with(input.file, &note, &joined, &[], input.eligible).len(),
                );
                let each = room / failing.len();
                for f in &mut failing {
                    f.1 = truncate(&f.1, each);
                }
            }
            (joined, note)
        }
    };

    let view = RankView {
        file: input.file.to_string(),
        source,
        source_note: note,
        failing,
        candidates,
    };
    let shown = [
        view.source.as_str(),
        &view
            .failing
            .iter()
            .map(|(n, b)| format!("{n}\n{b}"))
            .collect::<String>(),
        &view
            .candidates
            .iter()
            .map(|(n, s)| format!("{n}\n{s}"))
            .collect::<String>(),
    ];
    if shown.iter().any(|t| mentions(t, input.hidden_check)) {
        return Err(
            "the hidden check's name appears outside its own definition, so the file \
             cannot be shown without it"
                .into(),
        );
    }
    Ok(view)
}

impl RankView {
    /// The whole file (hidden check removed) is shown, not an excerpt.
    pub fn is_complete(&self) -> bool {
        self.source_note == FULL_NOTE
    }
}

/// `src` with every definition of `fn HIDDEN` cut out, together with the
/// comment and attribute lines directly above it ([`fn_span`]). The one
/// mechanism by which the ranker's view, the audit's view, the generator's
/// view and the generator's visible-check runs drop the adjudicating check.
/// What remains may still MENTION the name; every caller refuses on that
/// ([`mentions`]) rather than show it.
pub(crate) fn without_check(src: &str, hidden: &str) -> String {
    let mut s = src.to_string();
    // A name defined twice is cut twice; four is far past any real file.
    for _ in 0..4 {
        match fn_span(&s, hidden) {
            Some((a, b)) => s.replace_range(a..b, ""),
            None => break,
        }
    }
    s
}

/// `s` cut to at most `n` characters (on a char boundary), marked as cut.
pub(crate) fn truncate(s: &str, n: usize) -> String {
    const MARK: &str = "\n    // … truncated";
    if s.chars().count() <= n {
        return s.to_string();
    }
    let keep = n.saturating_sub(MARK.chars().count());
    let mut out: String = s.chars().take(keep).collect();
    out.push_str(MARK);
    out
}

/// Does `text` contain `name` as a whole identifier?
pub(crate) fn mentions(text: &str, name: &str) -> bool {
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut from = 0;
    while let Some(i) = text[from..].find(name) {
        let at = from + i;
        let end = at + name.len();
        let before_ok = !text[..at].chars().next_back().is_some_and(ident);
        let after_ok = !text[end..].chars().next().is_some_and(ident);
        if before_ok && after_ok {
            return true;
        }
        from = end;
    }
    false
}

/// Byte offset of the line defining `fn NAME(` (optionally `pub`).
fn def_line(src: &str, name: &str) -> Option<usize> {
    let mut off = 0;
    for line in src.split_inclusive('\n') {
        let t = line.trim_start();
        let t = t.strip_prefix("pub ").map(str::trim_start).unwrap_or(t);
        if let Some(rest) = t.strip_prefix("fn ").and_then(|r| r.strip_prefix(name)) {
            if rest.trim_start().starts_with(['(', '<', '[']) {
                return Some(off);
            }
        }
        off += line.len();
    }
    None
}

/// `[start, end)` of `fn NAME`, from the comment and attribute lines directly
/// above it to its closing brace and the newline after it.
pub(crate) fn fn_span(src: &str, name: &str) -> Option<(usize, usize)> {
    let def = def_line(src, name)?;
    let mut start = def;
    while start > 0 {
        let prev = src[..start - 1].rfind('\n').map_or(0, |i| i + 1);
        let t = src[prev..start].trim();
        if t.starts_with("@[") || t.starts_with("//") {
            start = prev;
        } else {
            break;
        }
    }
    let open = def + src[def..].find('{')?;
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    let mut end = open + i + 1;
                    if src[end..].starts_with('\n') {
                        end += 1;
                    }
                    return Some((start, end));
                }
            }
            _ => {}
        }
    }
    None
}

/// `fn NAME(params) -> T`, whitespace collapsed (DecisionBench's criteria).
fn signature(text: &str, name: &str) -> String {
    let Some(at) = def_line(text, name) else {
        return format!("fn {name}");
    };
    let head = &text[at..];
    let head = &head[..head.find('{').unwrap_or(head.len())];
    head.split_whitespace().collect::<Vec<_>>().join(" ")
}
