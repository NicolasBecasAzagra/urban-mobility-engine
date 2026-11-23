//! Errors of the export module

/// Error produced while exporting results
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// File read/write error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Error during serialization to JSON or another format
    #[error("Serialization error: {0}")]
    Serialize(String),
}
