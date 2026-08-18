use thiserror::Error;

#[derive(Error, Debug)]
pub enum DomainError {
    #[error("{entity} {id} not found")]
    NotFound { entity: &'static str, id: String },

    #[error("duplicate {entity}: {detail}")]
    Duplicate {
        entity: &'static str,
        detail: String,
    },

    #[error("invalid data: {0}")]
    Invalid(String),

    #[error("external source error: {0}")]
    Source(String),

    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}

#[derive(Error, Debug)]
#[error("{kind}: {detail}")]
pub struct StorageError {
    pub kind: StorageKind,
    pub detail: String,
}

#[derive(Error, Debug)]
pub enum StorageKind {
    #[error("connection")]
    Connection,
    #[error("query")]
    Query,
    #[error("constraint")]
    Constraint,
    #[error("migration")]
    Migration,
}

impl DomainError {
    pub fn not_found(entity: &'static str, id: impl Into<String>) -> Self {
        Self::NotFound {
            entity,
            id: id.into(),
        }
    }

    pub fn invalid(msg: impl Into<String>) -> Self {
        Self::Invalid(msg.into())
    }

    pub fn source(msg: impl Into<String>) -> Self {
        Self::Source(msg.into())
    }
}

impl From<String> for DomainError {
    fn from(s: String) -> Self {
        Self::Source(s)
    }
}
