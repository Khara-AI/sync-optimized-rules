//! sync-optimized-rules: keep the Claude Code rules of many repositories in
//! sync, by groups and scopes, locally and without overwriting anything.
//!
//! The binary (`sor`), the Claude Code plugin and editor extensions are thin
//! clients of this library; machines talk to it through `sor --json`.

pub mod config;
pub mod discover;
pub mod error;
pub mod paths;

#[cfg(test)]
mod testutil;
