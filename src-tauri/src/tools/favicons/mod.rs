//! Favicons: generates a favicon package (favicon.ico, PNG icons, Apple
//! touch icon, web manifest, HTML snippet) from one logo image.
//! Spec: docs/PROMPT_11_SPEC.md

pub mod commands;
pub mod ico;
pub mod package;
pub mod render;
pub mod source;
