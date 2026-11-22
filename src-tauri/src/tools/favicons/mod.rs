//! Favicons: generates a favicon package (favicon.ico, PNG icons, Apple
//! touch icon, web manifest, HTML snippet) from one logo image.
//! Spec: docs/PROMPT_11_SPEC.md (removed in v0.4.2, see commit 1fce0df)

pub mod commands;
pub mod ico;
pub mod package;
pub mod render;
pub mod source;
