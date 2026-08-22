use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PathError {
    #[error("path does not exist: {0}")]
    NotFound(PathBuf),

    #[error("path has no parent: {0}")]
    NoParent(PathBuf),

    #[error("path has no usable directory name: {0}")]
    NoUsableDirectoryName(PathBuf),

    #[error("invalid source path: {0}")]
    InvalidSourcePath(PathBuf),
}

#[derive(Debug, Error)]
pub enum FileError {
    #[error("failed to create {path}: {source}")]
    CreateDir {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to write {path}: {source}")]
    Write {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to remove {path}: {source}")]
    Remove {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("refusing to overwrite non-generated file {0}")]
    RefusingOverwrite(PathBuf),

    #[error("failed to walk files: {0}")]
    Walk(String),
}

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("failed to open {label} {path}: {source}")]
    Open {
        label: &'static str,
        path: PathBuf,
        source: rusqlite::Error,
    },

    #[error("invalid {label}: {detail}")]
    InvalidSchema { label: &'static str, detail: String },

    #[error("{0}")]
    Operation(String),
}

#[derive(Debug, Error)]
pub enum JdtlsError {
    #[error("{0}")]
    Operation(String),
}

#[derive(Debug, Error)]
pub enum LlmError {
    #[error("missing ANTHROPIC_API_KEY")]
    MissingApiKey,

    #[error("{0}")]
    Operation(String),
}

#[derive(Debug, Error)]
pub enum CheckpointError {
    #[error("{0}")]
    Operation(String),
}

#[derive(Debug, Error)]
pub enum ParserError {
    #[error("{0}")]
    Operation(String),
}

#[derive(Debug, Error)]
pub enum KnowledgeBaseError {
    #[error("{0}")]
    Load(String),
}

#[derive(Debug, Error)]
pub enum KgError {
    #[error("{0}")]
    Operation(String),
}
