//! Core logic for corral: managing git worktrees that host parallel
//! Claude Code agent sessions.
//!
//! Both the Tauri desktop app and the `corral` CLI build on this crate.

pub mod agent_session;
pub mod bin;
pub mod caffeinate;
pub mod checks;
pub mod config;
pub mod error;
pub mod files;
pub mod git;
pub mod harness;
pub mod hooks;
pub mod json_store;
pub mod keyed_store;
pub mod log_err;
pub mod merge;
pub mod names;
pub mod natural;
pub mod onboarding;
pub mod opencode;
pub mod paths;
pub mod pi;
pub mod pr;
pub mod process;
pub mod project;
pub mod project_settings;
pub mod settings;
pub mod shell_env;
pub mod watch;
pub mod worktree;
pub mod worktree_notes;
pub mod worktree_settings;

pub use error::{Error, Result};
pub use git::GitStatus;
pub use worktree::Worktree;
