use std::io;
use std::path::PathBuf;

use thiserror::Error;

use crate::{ChecksumAlgorithm, DownloadJobId};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DownloadPlanError {
    #[error("download job id must not be empty")]
    EmptyJobId,
    #[error("download URL must not be empty")]
    EmptyUrl,
    #[error("duplicate download job id: {id:?}")]
    DuplicateJobId { id: DownloadJobId },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DownloadConfigError {
    #[error("global download concurrency must be greater than zero")]
    ZeroGlobalConcurrency,
    #[error("per-host download concurrency must be greater than zero")]
    ZeroPerHostConcurrency,
    #[error("per-host download concurrency must not exceed global concurrency")]
    PerHostExceedsGlobal,
    #[error("download queue capacity must be greater than zero")]
    ZeroQueueCapacity,
    #[error("retry attempts must be greater than zero")]
    ZeroRetryAttempts,
    #[error("download event buffer must be greater than zero")]
    ZeroEventBuffer,
    #[error("partial download suffix must not be empty")]
    EmptyTempSuffix,
    #[error("partial metadata suffix must not be empty")]
    EmptyMetadataSuffix,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DownloadManagerError {
    #[error(transparent)]
    InvalidConfig(#[from] DownloadConfigError),
    #[error("unknown download job: {id:?}")]
    UnknownJob { id: DownloadJobId },
}

#[derive(Debug, Error)]
pub enum ArtifactVerifyError {
    #[error("failed to read metadata for artifact {path}: {source}")]
    ReadMetadata { path: PathBuf, source: io::Error },
    #[error("failed to open artifact {path}: {source}")]
    Open { path: PathBuf, source: io::Error },
    #[error("failed to read artifact {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("read {read} bytes from {path}, beyond buffer capacity {capacity}")]
    ReadBufferOutOfBounds {
        path: PathBuf,
        read: usize,
        capacity: usize,
    },
    #[error("artifact size mismatch for {path}: expected {expected} bytes, got {actual} bytes")]
    SizeMismatch {
        path: PathBuf,
        expected: u64,
        actual: u64,
    },
    #[error(
        "artifact checksum mismatch for {path}: expected {algorithm:?} {expected}, got {actual}"
    )]
    ChecksumMismatch {
        path: PathBuf,
        algorithm: ChecksumAlgorithm,
        expected: String,
        actual: String,
    },
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DownloadBackendError {
    #[error("backend failed to start job {id:?}: {message}")]
    StartJob { id: DownloadJobId, message: String },
    #[error("backend failed to stop job {id:?}: {message}")]
    StopWorker { id: DownloadJobId, message: String },
}

#[derive(Debug, Error)]
pub enum DownloadServiceError {
    #[error(transparent)]
    Manager(#[from] DownloadManagerError),
    #[error(transparent)]
    Backend(#[from] DownloadBackendError),
    #[error("download event stream receiver is closed")]
    EventStreamClosed,
}
