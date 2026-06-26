use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::DownloadPlanError;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DownloadJobId(String);

impl DownloadJobId {
    pub fn new(value: impl Into<String>) -> Result<Self, DownloadPlanError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DownloadPlanError::EmptyJobId);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadUrl(String);

impl DownloadUrl {
    pub fn new(value: impl Into<String>) -> Result<Self, DownloadPlanError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DownloadPlanError::EmptyUrl);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadPlan {
    pub jobs: Vec<DownloadJobSpec>,
}

impl DownloadPlan {
    pub fn new(jobs: Vec<DownloadJobSpec>) -> Result<Self, DownloadPlanError> {
        let mut ids = BTreeSet::new();
        for job in &jobs {
            if !ids.insert(job.id.clone()) {
                return Err(DownloadPlanError::DuplicateJobId { id: job.id.clone() });
            }
        }
        Ok(Self { jobs })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadJobSpec {
    pub id: DownloadJobId,
    pub url: DownloadUrl,
    pub host: Option<String>,
    pub target_path: PathBuf,
    pub expected_size: Option<u64>,
    pub checksum: Option<Checksum>,
    pub kind: DownloadArtifactKind,
    pub policy: DownloadJobPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checksum {
    pub algorithm: ChecksumAlgorithm,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumAlgorithm {
    Sha1,
    Sha256,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadArtifactKind {
    VersionMetadata,
    ClientJar,
    Library,
    Asset,
    Native,
    JavaRuntime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DownloadJobPolicy {
    pub resumable: bool,
    pub retryable: bool,
}

impl Default for DownloadJobPolicy {
    fn default() -> Self {
        Self {
            resumable: true,
            retryable: true,
        }
    }
}
