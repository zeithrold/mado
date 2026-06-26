use std::path::{Path, PathBuf};

use crate::{Checksum, DownloadJobId, DownloadJobSpec, DownloadStorageConfig, DownloadUrl};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadStoragePaths {
    pub target_path: PathBuf,
    pub partial_path: PathBuf,
    pub partial_metadata_path: PathBuf,
}

impl DownloadStoragePaths {
    pub fn for_job(job: &DownloadJobSpec, config: &DownloadStorageConfig) -> Self {
        Self::for_target(&job.target_path, config)
    }

    pub fn for_target(target_path: impl AsRef<Path>, config: &DownloadStorageConfig) -> Self {
        let target_path = target_path.as_ref().to_path_buf();
        let partial_path = path_with_suffix(&target_path, &config.temp_suffix);
        let partial_metadata_path = path_with_suffix(&target_path, &config.metadata_suffix);
        Self {
            target_path,
            partial_path,
            partial_metadata_path,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialDownloadMetadata {
    pub job_id: DownloadJobId,
    pub url: DownloadUrl,
    pub target_path: PathBuf,
    pub expected_size: Option<u64>,
    pub checksum: Option<Checksum>,
    pub downloaded: u64,
    pub validator: Option<ResumeValidator>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResumeValidator {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}
