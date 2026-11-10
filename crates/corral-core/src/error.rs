use std::path::PathBuf;

use thiserror::Error;

/// Errors produced by corral-core operations.
#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("could not determine home directory")]
    NoHomeDir,

    #[error("could not determine config directory")]
    NoConfigDir,

    #[error("could not determine data directory")]
    NoDataDir,

    #[error("not a git repository: {0}")]
    NotAGitRepo(PathBuf),

    #[error("git failed: `{command}`: {stderr}")]
    Git { command: String, stderr: String },

    #[error("gh failed: `{command}`: {stderr}")]
    Gh { command: String, stderr: String },

    #[error("worktree already exists: {0}")]
    WorktreeExists(PathBuf),

    #[error("no worktree found matching {0:?}")]
    WorktreeNotFound(String),

    #[error("project already registered: {0:?}")]
    ProjectAlreadyRegistered(String),

    #[error("project not registered: {0:?} — add it first")]
    ProjectNotRegistered(String),

    #[error("ambiguous feature {feature:?}; matches projects: {projects}")]
    AmbiguousFeature { feature: String, projects: String },

    #[error("invalid name: {0:?}")]
    InvalidName(String),

    #[error("invalid worktrees root {0:?}: must be an absolute path or start with ~")]
    InvalidWorktreesRoot(String),

    #[error("invalid utf-8 in git output: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
