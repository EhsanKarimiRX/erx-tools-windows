use serde::{Serialize, ser::Serializer};

/// Shared error type for all commands callable from the frontend.
///
/// Every variant carries a user-friendly English message; internal details
/// never reach the UI. The `Serialize` impl turns the error into that message
/// string, so the frontend receives a plain `string` when a command fails.
// No command fails yet in the foundation; the first real tool starts
// returning these. Remove this attribute when that happens.
#[allow(dead_code)]
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Unexpected internal failure. Log the detailed cause in Rust before
    /// returning this; the string is shown to the user as-is.
    #[error("{0}")]
    Internal(String),
}

impl Serialize for ToolError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
