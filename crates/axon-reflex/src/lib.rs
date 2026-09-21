//! CX-35 Phase 1 — the Reflex serving boundary.
//!
//! One client surface over three deployment modes: [`Mode::Embedded`],
//! [`Mode::LocalSidecar`], [`Mode::RemoteService`]. A caller holds a client and
//! a protocol version; it never names a backend and never depends on an
//! inference vendor.
//!
//! # What is invariant, and what is not
//!
//! The naive version of this boundary claims the three modes have "the same
//! semantics". They do not, and asserting it would repeat a defect this
//! repository spent a day fixing: the interpreter and native engine both
//! claimed to honour the same controls, and native silently ignored five of
//! them — an effect ceiling, a replay cache, a record journal, an audit ledger
//! and an RNG seed. What fixed that was not making the engines identical; it
//! was naming, per control per engine, one of `enforced` / `explicitly-refused`
//! / `not-applicable`, and refusing rather than approximating.
//!
//! So this module implements ONE invariant across all three modes, chosen
//! because it is the property most likely to break when work moves from the
//! caller's own address space to a sidecar to a hosted service:
//!
//! > **A decision is made under exactly one principal, and no [`StateHandle`]
//! > is reachable across principals. Cross-principal reuse is a REFUSAL, never
//! > a cache miss.**
//!
//! The refusal-versus-miss distinction is the whole of it, and it is not
//! pedantry. A cache miss invites the caller to re-encode and proceed, so the
//! boundary is silently crossed and nothing records that it happened. A
//! refusal names the violation and cannot be retried into success. Embedded is
//! the mode with the MOST ways to leak this — one address space, one process,
//! shared memory — which is exactly why it is an invariant rather than a
//! mode-dependent row.
//!
//! Latency, cancellation, crash isolation, state residency and failure modes
//! are NOT invariant here. They belong to the mode matrix in
//! `AXON-COMPLETENESS.json` under `axis: mode`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};

/// Wire/protocol version. A client and a backend that disagree must refuse
/// rather than guess at a shape.
pub const PROTOCOL: &str = "axon-reflex/1";

/// Deployment topology. NOT an execution engine — `interpreter` / `native` /
/// `wasm` / `guest` are engines; these are places a backend runs. Conflating
/// them would let a green serving result read as engine support, which the
/// completeness matrix rejects structurally via its `axis` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Embedded,
    LocalSidecar,
    RemoteService,
}

impl Mode {
    pub fn name(&self) -> &'static str {
        match self {
            Mode::Embedded => "embedded",
            Mode::LocalSidecar => "local-sidecar",
            Mode::RemoteService => "remote-service",
        }
    }
}

/// Who a request is decided under. Exactly one per request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrincipalScope {
    pub principal: String,
}

impl PrincipalScope {
    pub fn new(p: impl Into<String>) -> Self {
        Self {
            principal: p.into(),
        }
    }
}

/// Reusable encoded state.
///
/// The owning principal is carried IN the handle, not looked up beside it. A
/// handle that did not name its owner would make cross-principal reuse a
/// lookup question — and a failed lookup is a cache miss, which is precisely
/// the outcome this design refuses to produce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateHandle {
    pub id: String,
    pub principal: String,
}

/// Why a request was not served. Closed, so the set of things a backend may
/// refuse is enumerable rather than open-ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The caller is not the principal the handle belongs to.
    ///
    /// Distinct from `UnknownState` ON PURPOSE. This says "it exists and is
    /// not yours"; that says "there is nothing here". Collapsing them would
    /// turn an authority violation into a retryable miss.
    CrossPrincipal { owner: String, caller: String },
    /// No such handle — a genuine miss, safe to re-encode.
    UnknownState { id: String },
    /// The backend cannot honour a declared control in this mode. Stated at
    /// the boundary rather than degraded quietly.
    Unsupported { control: String, mode: &'static str },
    /// The transport failed. An infrastructure failure is NOT a decision and
    /// NOT an abstention.
    Transport(String),
    /// Protocol mismatch between client and backend.
    Protocol(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::CrossPrincipal { owner, caller } => write!(
                f,
                "cross-principal reuse refused: state belongs to `{owner}`, caller is `{caller}` \
                 — this is a refusal, not a cache miss"
            ),
            Refusal::UnknownState { id } => write!(f, "no such state handle: {id}"),
            Refusal::Unsupported { control, mode } => {
                write!(f, "control `{control}` is not supported in {mode} mode")
            }
            Refusal::Transport(e) => write!(f, "transport failure: {e}"),
            Refusal::Protocol(e) => write!(f, "protocol: {e}"),
        }
    }
}

/// A decision. Carries the principal it was decided under, so an audit reading
/// the response alone can attribute it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub choice: String,
    pub principal: String,
}

/// The backend-neutral, mode-neutral surface.
pub trait ReflexBackend {
    fn mode(&self) -> Mode;
    fn encode_state(&mut self, input: &str, scope: &PrincipalScope)
        -> Result<StateHandle, Refusal>;
    fn decide(
        &mut self,
        handle: &StateHandle,
        question: &str,
        scope: &PrincipalScope,
    ) -> Result<Decision, Refusal>;
    fn release_state(
        &mut self,
        handle: &StateHandle,
        scope: &PrincipalScope,
    ) -> Result<(), Refusal>;
}

// ── the shared decision core ────────────────────────────────────────────────
//
// ONE implementation of the invariant, used by all three modes: embedded calls
// it directly, the sidecar binary calls it, and the remote test server calls
// it. Three copies of an authority check is three places for it to drift, and
// the engine-parity work that motivated this crate is a long record of exactly
// that drift.

/// The authoritative store. Not public: modes reach it through their transport.
#[derive(Default)]
pub struct ReflexCore {
    states: HashMap<String, String>, // id -> owning principal
    next: u64,
}

impl ReflexCore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn encode_state(&mut self, _input: &str, scope: &PrincipalScope) -> StateHandle {
        self.next += 1;
        let id = format!("st-{}", self.next);
        self.states.insert(id.clone(), scope.principal.clone());
        StateHandle {
            id,
            principal: scope.principal.clone(),
        }
    }

    /// The invariant, in one place.
    ///
    /// Note the ORDER: existence first, then ownership. Checking ownership
    /// first would let a caller probe for handles belonging to others by
    /// distinguishing the two refusals — the refusal must not become an oracle
    /// for another principal's state.
    fn authorize(&self, id: &str, caller: &str) -> Result<(), Refusal> {
        match self.states.get(id) {
            None => Err(Refusal::UnknownState { id: id.to_string() }),
            Some(owner) if owner == caller => Ok(()),
            Some(owner) => Err(Refusal::CrossPrincipal {
                owner: owner.clone(),
                caller: caller.to_string(),
            }),
        }
    }

    pub fn decide(
        &mut self,
        id: &str,
        question: &str,
        scope: &PrincipalScope,
    ) -> Result<Decision, Refusal> {
        self.authorize(id, &scope.principal)?;
        // Deterministic by construction: a fixed function of the question.
        // Phase 1 is about the SEAM, not about inference quality, and a
        // stochastic stub would make the cross-mode comparison meaningless.
        Ok(Decision {
            choice: format!("decided({question})"),
            principal: scope.principal.clone(),
        })
    }

    pub fn release_state(&mut self, id: &str, scope: &PrincipalScope) -> Result<(), Refusal> {
        self.authorize(id, &scope.principal)?;
        self.states.remove(id);
        Ok(())
    }
}

// ── Embedded ────────────────────────────────────────────────────────────────

/// In-process. The mode with the most ways to leak state across principals,
/// since there is no address-space boundary doing any of the work.
pub struct EmbeddedBackend {
    core: ReflexCore,
}

impl Default for EmbeddedBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddedBackend {
    pub fn new() -> Self {
        Self {
            core: ReflexCore::new(),
        }
    }
}

impl ReflexBackend for EmbeddedBackend {
    fn mode(&self) -> Mode {
        Mode::Embedded
    }
    fn encode_state(
        &mut self,
        input: &str,
        scope: &PrincipalScope,
    ) -> Result<StateHandle, Refusal> {
        Ok(self.core.encode_state(input, scope))
    }
    fn decide(
        &mut self,
        handle: &StateHandle,
        question: &str,
        scope: &PrincipalScope,
    ) -> Result<Decision, Refusal> {
        self.core.decide(&handle.id, question, scope)
    }
    fn release_state(
        &mut self,
        handle: &StateHandle,
        scope: &PrincipalScope,
    ) -> Result<(), Refusal> {
        self.core.release_state(&handle.id, scope)
    }
}

// ── wire encoding, shared by sidecar and remote ─────────────────────────────

pub fn encode_request(op: &str, id: &str, input: &str, principal: &str) -> String {
    serde_json::json!({
        "protocol": PROTOCOL,
        "op": op,
        "id": id,
        "input": input,
        "principal": principal,
    })
    .to_string()
}

/// Decode a response frame into the same `Result` the in-process call returns.
///
/// A refusal must survive serialization AS A REFUSAL. If the wire collapsed
/// `CrossPrincipal` into a generic error, the sidecar and remote modes would
/// report an authority violation as an infrastructure failure, and the
/// invariant would hold only in the mode that needs it least.
pub fn decode_response(line: &str) -> Result<serde_json::Value, Refusal> {
    let v: serde_json::Value = serde_json::from_str(line.trim())
        .map_err(|e| Refusal::Protocol(format!("unparseable response: {e}")))?;
    match v.get("protocol").and_then(|p| p.as_str()) {
        Some(PROTOCOL) => {}
        other => {
            return Err(Refusal::Protocol(format!(
                "expected protocol {PROTOCOL}, got {other:?}"
            )))
        }
    }
    if let Some(r) = v.get("refusal") {
        let kind = r.get("kind").and_then(|k| k.as_str()).unwrap_or("");
        return Err(match kind {
            "cross_principal" => Refusal::CrossPrincipal {
                owner: r
                    .get("owner")
                    .and_then(|o| o.as_str())
                    .unwrap_or_default()
                    .to_string(),
                caller: r
                    .get("caller")
                    .and_then(|c| c.as_str())
                    .unwrap_or_default()
                    .to_string(),
            },
            "unknown_state" => Refusal::UnknownState {
                id: r
                    .get("id")
                    .and_then(|i| i.as_str())
                    .unwrap_or_default()
                    .to_string(),
            },
            other => Refusal::Protocol(format!("unknown refusal kind `{other}`")),
        });
    }
    Ok(v)
}

/// Serialize a `Result` from the core into a response frame.
pub fn encode_response(r: &Result<serde_json::Value, Refusal>) -> String {
    match r {
        Ok(v) => {
            let mut out = v.clone();
            out["protocol"] = serde_json::json!(PROTOCOL);
            out.to_string()
        }
        Err(e) => {
            let refusal = match e {
                Refusal::CrossPrincipal { owner, caller } => serde_json::json!({
                    "kind": "cross_principal", "owner": owner, "caller": caller,
                }),
                Refusal::UnknownState { id } => {
                    serde_json::json!({ "kind": "unknown_state", "id": id })
                }
                other => serde_json::json!({ "kind": "other", "detail": other.to_string() }),
            };
            serde_json::json!({ "protocol": PROTOCOL, "refusal": refusal }).to_string()
        }
    }
}

/// Run one request against a core. Shared by the sidecar binary and the
/// remote test server so the authority check cannot differ between them.
pub fn serve_one(core: &mut ReflexCore, line: &str) -> String {
    let req: serde_json::Value = match serde_json::from_str(line.trim()) {
        Ok(v) => v,
        Err(e) => {
            return encode_response(&Err(Refusal::Protocol(format!("bad request: {e}"))));
        }
    };
    let op = req.get("op").and_then(|o| o.as_str()).unwrap_or("");
    let id = req.get("id").and_then(|i| i.as_str()).unwrap_or("");
    let input = req.get("input").and_then(|i| i.as_str()).unwrap_or("");
    let principal = req
        .get("principal")
        .and_then(|p| p.as_str())
        .unwrap_or("")
        .to_string();
    let scope = PrincipalScope::new(principal);
    let out = match op {
        "encode" => {
            let h = core.encode_state(input, &scope);
            Ok(serde_json::json!({ "id": h.id, "principal": h.principal }))
        }
        "decide" => core
            .decide(id, input, &scope)
            .map(|d| serde_json::json!({ "choice": d.choice, "principal": d.principal })),
        "release" => core
            .release_state(id, &scope)
            .map(|_| serde_json::json!({ "released": true })),
        other => Err(Refusal::Protocol(format!("unknown op `{other}`"))),
    };
    encode_response(&out)
}

// ── LocalSidecar ────────────────────────────────────────────────────────────

/// A separate process on the same host, one JSON frame per line on
/// stdin/stdout. The precedent is `cortex-policy-adapter`, which speaks a typed
/// protocol over the same seam.
pub struct SidecarBackend {
    child: std::process::Child,
    stdin: std::process::ChildStdin,
    stdout: BufReader<std::process::ChildStdout>,
}

impl SidecarBackend {
    pub fn spawn(exe: &str) -> Result<Self, Refusal> {
        let mut child = std::process::Command::new(exe)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| Refusal::Transport(format!("spawn {exe}: {e}")))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| Refusal::Transport("no stdin".into()))?;
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| Refusal::Transport("no stdout".into()))?,
        );
        Ok(Self {
            child,
            stdin,
            stdout,
        })
    }

    fn roundtrip(&mut self, req: &str) -> Result<serde_json::Value, Refusal> {
        writeln!(self.stdin, "{req}").map_err(|e| Refusal::Transport(e.to_string()))?;
        self.stdin
            .flush()
            .map_err(|e| Refusal::Transport(e.to_string()))?;
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .map_err(|e| Refusal::Transport(e.to_string()))?;
        if line.is_empty() {
            return Err(Refusal::Transport("sidecar closed the stream".into()));
        }
        decode_response(&line)
    }
}

impl Drop for SidecarBackend {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl ReflexBackend for SidecarBackend {
    fn mode(&self) -> Mode {
        Mode::LocalSidecar
    }
    fn encode_state(
        &mut self,
        input: &str,
        scope: &PrincipalScope,
    ) -> Result<StateHandle, Refusal> {
        let v = self.roundtrip(&encode_request("encode", "", input, &scope.principal))?;
        Ok(StateHandle {
            id: v
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or_default()
                .to_string(),
            principal: scope.principal.clone(),
        })
    }
    fn decide(
        &mut self,
        handle: &StateHandle,
        question: &str,
        scope: &PrincipalScope,
    ) -> Result<Decision, Refusal> {
        let v = self.roundtrip(&encode_request(
            "decide",
            &handle.id,
            question,
            &scope.principal,
        ))?;
        Ok(Decision {
            choice: v
                .get("choice")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string(),
            principal: scope.principal.clone(),
        })
    }
    fn release_state(
        &mut self,
        handle: &StateHandle,
        scope: &PrincipalScope,
    ) -> Result<(), Refusal> {
        self.roundtrip(&encode_request("release", &handle.id, "", &scope.principal))?;
        Ok(())
    }
}

// ── RemoteService ───────────────────────────────────────────────────────────

/// A hosted service over HTTP/1.1.
///
/// Written directly on `TcpStream` rather than pulling in an HTTP client.
/// `reqwest` IS compiled into the default build (via `axon-domain`'s default
/// features — verified with `cargo tree -i reqwest`), so this is a choice, not
/// a necessity: its blocking API needs an extra feature and its async API would
/// drag a runtime into a gate that does not otherwise have one. Phase 1 needs
/// one request/response shape, and one-shot HTTP/1.1 with `Connection: close`
/// is a dozen lines with no ambiguity about what is on the wire.
pub struct RemoteBackend {
    addr: String,
}

impl RemoteBackend {
    pub fn new(addr: impl Into<String>) -> Self {
        Self { addr: addr.into() }
    }

    fn roundtrip(&mut self, req: &str) -> Result<serde_json::Value, Refusal> {
        use std::net::TcpStream;
        let mut s =
            TcpStream::connect(&self.addr).map_err(|e| Refusal::Transport(e.to_string()))?;
        let http = format!(
            "POST /reflex HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
            self.addr,
            req.len(),
            req
        );
        s.write_all(http.as_bytes())
            .map_err(|e| Refusal::Transport(e.to_string()))?;
        let mut raw = String::new();
        s.read_to_string(&mut raw)
            .map_err(|e| Refusal::Transport(e.to_string()))?;
        let body = raw
            .split_once("\r\n\r\n")
            .map(|(_, b)| b)
            .ok_or_else(|| Refusal::Transport("malformed HTTP response".into()))?;
        decode_response(body)
    }
}

impl ReflexBackend for RemoteBackend {
    fn mode(&self) -> Mode {
        Mode::RemoteService
    }
    fn encode_state(
        &mut self,
        input: &str,
        scope: &PrincipalScope,
    ) -> Result<StateHandle, Refusal> {
        let v = self.roundtrip(&encode_request("encode", "", input, &scope.principal))?;
        Ok(StateHandle {
            id: v
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or_default()
                .to_string(),
            principal: scope.principal.clone(),
        })
    }
    fn decide(
        &mut self,
        handle: &StateHandle,
        question: &str,
        scope: &PrincipalScope,
    ) -> Result<Decision, Refusal> {
        let v = self.roundtrip(&encode_request(
            "decide",
            &handle.id,
            question,
            &scope.principal,
        ))?;
        Ok(Decision {
            choice: v
                .get("choice")
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_string(),
            principal: scope.principal.clone(),
        })
    }
    fn release_state(
        &mut self,
        handle: &StateHandle,
        scope: &PrincipalScope,
    ) -> Result<(), Refusal> {
        self.roundtrip(&encode_request("release", &handle.id, "", &scope.principal))?;
        Ok(())
    }
}
