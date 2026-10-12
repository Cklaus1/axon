//! One error type; every variant maps to one documented CLI exit code.

use axon_loop_contracts::Refusal;

#[derive(Debug)]
pub enum LoopError {
    /// Filesystem failure or a corrupt store record. Exit 2.
    Io(String),
    /// Bad CLI usage. Exit 2.
    Usage(String),
    /// Input failed strict contract parsing. Exit 3.
    Malformed(Refusal),
    /// A shape-valid request the rules refuse (untrusted issuer, unadmitted
    /// target, revoked predecessor, contaminated corpus, frozen plan, …). No
    /// state changed. Exit 4.
    Refused(String),
    /// Lost a compare-and-swap: stale epoch or wrong expected policy. No state
    /// changed; re-resolve and decide again. Exit 5.
    Conflict(String),
    /// The scope has no usable active policy (paused, never activated, or the
    /// active policy is revoked). Exit 6.
    Paused(String),
    /// The plan is not runtime-ready (unset operator fields, not approved, not
    /// frozen). Exit 7.
    NotReady(String),
}

impl LoopError {
    pub fn exit_code(&self) -> i32 {
        match self {
            LoopError::Io(_) | LoopError::Usage(_) => 2,
            LoopError::Malformed(_) => 3,
            LoopError::Refused(_) => 4,
            LoopError::Conflict(_) => 5,
            LoopError::Paused(_) => 6,
            LoopError::NotReady(_) => 7,
        }
    }
    pub fn kind(&self) -> &'static str {
        match self {
            LoopError::Io(_) => "io",
            LoopError::Usage(_) => "usage",
            LoopError::Malformed(_) => "malformed",
            LoopError::Refused(_) => "refused",
            LoopError::Conflict(_) => "conflict",
            LoopError::Paused(_) => "paused",
            LoopError::NotReady(_) => "not_ready",
        }
    }
}

impl std::fmt::Display for LoopError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoopError::Io(s) => write!(f, "io: {s}"),
            LoopError::Usage(s) => write!(f, "usage: {s}"),
            LoopError::Malformed(r) => write!(f, "malformed: {r}"),
            LoopError::Refused(s) => write!(f, "refused: {s}"),
            LoopError::Conflict(s) => write!(f, "conflict: {s}"),
            LoopError::Paused(s) => write!(f, "paused: {s}"),
            LoopError::NotReady(s) => write!(f, "not ready: {s}"),
        }
    }
}

impl std::error::Error for LoopError {}

impl From<std::io::Error> for LoopError {
    fn from(e: std::io::Error) -> Self {
        LoopError::Io(e.to_string())
    }
}

impl From<Refusal> for LoopError {
    fn from(r: Refusal) -> Self {
        match r {
            // A cross-document semantic check failing is a refusal of the
            // request, not a malformed document.
            Refusal::Semantic(s) => LoopError::Refused(s),
            other => LoopError::Malformed(other),
        }
    }
}

pub(crate) fn refused(s: impl Into<String>) -> LoopError {
    LoopError::Refused(s.into())
}

pub type Result<T> = std::result::Result<T, LoopError>;
