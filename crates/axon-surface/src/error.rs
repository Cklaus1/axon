use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// All missing required sections, listed at once (Bug #3) so the author
    /// fixes them in a single pass rather than re-running per missing section.
    #[error("parse: missing required section(s): {}", .0.iter().map(|s| format!("`{s}`")).collect::<Vec<_>>().join(", "))]
    MissingSections(Vec<String>),

    #[error("parse: section `{section}` malformed: {detail}")]
    MalformedSection { section: String, detail: String },

    #[error("parse: could not extract field `{field}` from section `{section}`")]
    ExtractionFailed { field: String, section: String },
}

pub type Result<T> = std::result::Result<T, Error>;
