use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("parse: missing required section `{0}`")]
    MissingSection(String),

    #[error("parse: section `{section}` malformed: {detail}")]
    MalformedSection { section: String, detail: String },

    #[error("parse: could not extract field `{field}` from section `{section}`")]
    ExtractionFailed { field: String, section: String },
}

pub type Result<T> = std::result::Result<T, Error>;
