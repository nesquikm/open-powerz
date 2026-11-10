//! The error every binding call returns: a plain `{ kind, message, hint }` for JS.

use wasm_bindgen::prelude::*;

/// A binding error. `kind` is a stable machine-readable tag (`invalid`,
/// `forbidden`, …); `message` is the human-readable text.
#[wasm_bindgen(getter_with_clone)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindError {
    /// Stable tag: `invalid` for bad caller input, `forbidden` for a request
    /// outside the safe set.
    pub kind: String,
    /// Human-readable description.
    pub message: String,
    /// What the user can do about it, when there is something to say.
    pub hint: Option<String>,
}

impl BindError {
    /// An error of `kind` with `message`.
    pub fn new(kind: &str, message: impl Into<String>) -> BindError {
        BindError {
            kind: kind.to_owned(),
            message: message.into(),
            hint: None,
        }
    }

    /// An error of `kind` with `message` and `hint`.
    pub fn with_hint(kind: &str, message: impl Into<String>, hint: impl Into<String>) -> BindError {
        BindError {
            hint: Some(hint.into()),
            ..BindError::new(kind, message)
        }
    }

    /// Bad caller input.
    pub fn invalid(message: impl Into<String>) -> BindError {
        BindError::new("invalid", message)
    }
}
