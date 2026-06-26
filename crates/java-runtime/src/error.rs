use std::path::PathBuf;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum JavaRuntimeError {
    #[error("Java home does not exist: {path}")]
    HomeMissing { path: PathBuf },
    #[error("Java executable does not exist: {path}")]
    ExecutableMissing { path: PathBuf },
    #[error("Java executable path has no parent: {path}")]
    ExecutableWithoutParent { path: PathBuf },
    #[error("failed to read Java release file at {path}: {source}")]
    ReadRelease {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Java release file is too large at {path}: {size} bytes exceeds {max_size} bytes")]
    ReleaseFileTooLarge {
        path: PathBuf,
        size: u64,
        max_size: u64,
    },
    #[error("Java metadata is missing required field: {field}")]
    MissingMetadata { field: &'static str },
    #[error("Java metadata field is too large: {field} exceeds {max_bytes} bytes")]
    MetadataValueTooLarge {
        field: &'static str,
        max_bytes: usize,
    },
    #[error("Java metadata field contains a control character: {field}")]
    MetadataValueContainsControl { field: &'static str },
    #[error("Java version is invalid: {raw}")]
    InvalidVersion { raw: String },
    #[error("failed to run Java probe command {executable}: {source}")]
    ProbeStart {
        executable: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Java probe command failed for {executable} with status {status}: {stderr}")]
    ProbeFailed {
        executable: PathBuf,
        status: String,
        stderr: String,
    },
}
