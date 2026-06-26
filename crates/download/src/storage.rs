use std::fs::{self, File};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    Checksum, DownloadJobId, DownloadJobSpec, DownloadStorageConfig, DownloadStorageError,
    DownloadUrl,
};

pub const PARTIAL_DOWNLOAD_METADATA_SCHEMA_VERSION: u32 = 1;

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

    pub fn ensure_parent_dirs(&self) -> Result<(), DownloadStorageError> {
        for path in [
            &self.target_path,
            &self.partial_path,
            &self.partial_metadata_path,
        ] {
            ensure_parent_dir(path)?;
        }
        Ok(())
    }

    pub fn write_partial_bytes(
        &self,
        bytes: &[u8],
        config: &DownloadStorageConfig,
    ) -> Result<(), DownloadStorageError> {
        ensure_parent_dir(&self.partial_path)?;
        fs::write(&self.partial_path, bytes).map_err(|source| {
            DownloadStorageError::WritePartial {
                path: self.partial_path.clone(),
                source,
            }
        })?;
        fsync_path_if_configured(&self.partial_path, config)
    }

    pub fn write_partial_metadata(
        &self,
        metadata: &PartialDownloadMetadata,
        config: &DownloadStorageConfig,
    ) -> Result<(), DownloadStorageError> {
        ensure_parent_dir(&self.partial_metadata_path)?;
        let bytes = serde_json::to_vec_pretty(metadata).map_err(|source| {
            DownloadStorageError::ParsePartialMetadata {
                path: self.partial_metadata_path.clone(),
                source,
            }
        })?;
        fs::write(&self.partial_metadata_path, bytes).map_err(|source| {
            DownloadStorageError::WritePartialMetadata {
                path: self.partial_metadata_path.clone(),
                source,
            }
        })?;
        fsync_path_if_configured(&self.partial_metadata_path, config)
    }

    pub fn read_partial_metadata(&self) -> Result<PartialDownloadMetadata, DownloadStorageError> {
        let bytes = fs::read(&self.partial_metadata_path).map_err(|source| {
            DownloadStorageError::ReadPartialMetadata {
                path: self.partial_metadata_path.clone(),
                source,
            }
        })?;
        let metadata: PartialDownloadMetadata =
            serde_json::from_slice(&bytes).map_err(|source| {
                DownloadStorageError::ParsePartialMetadata {
                    path: self.partial_metadata_path.clone(),
                    source,
                }
            })?;
        if metadata.schema_version != PARTIAL_DOWNLOAD_METADATA_SCHEMA_VERSION {
            return Err(DownloadStorageError::UnsupportedPartialMetadataVersion {
                path: self.partial_metadata_path.clone(),
                version: metadata.schema_version,
            });
        }
        Ok(metadata)
    }

    pub fn remove_partial_metadata_if_exists(&self) -> Result<(), DownloadStorageError> {
        match fs::remove_file(&self.partial_metadata_path) {
            Ok(()) => Ok(()),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(source) => Err(DownloadStorageError::RemovePartialMetadata {
                path: self.partial_metadata_path.clone(),
                source,
            }),
        }
    }

    pub fn promote_partial_to_target(
        &self,
        config: &DownloadStorageConfig,
    ) -> Result<(), DownloadStorageError> {
        ensure_parent_dir(&self.target_path)?;
        if config.atomic_rename {
            fs::rename(&self.partial_path, &self.target_path).map_err(|source| {
                DownloadStorageError::PromotePartial {
                    partial_path: self.partial_path.clone(),
                    target_path: self.target_path.clone(),
                    source,
                }
            })?;
        } else {
            fs::copy(&self.partial_path, &self.target_path).map_err(|source| {
                DownloadStorageError::PromotePartial {
                    partial_path: self.partial_path.clone(),
                    target_path: self.target_path.clone(),
                    source,
                }
            })?;
            fs::remove_file(&self.partial_path).map_err(|source| {
                DownloadStorageError::RemovePromotedPartial {
                    path: self.partial_path.clone(),
                    source,
                }
            })?;
        }
        fsync_path_if_configured(&self.target_path, config)?;
        if config.fsync_on_complete {
            fsync_parent_if_present(&self.target_path)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PartialDownloadMetadata {
    pub schema_version: u32,
    pub job_id: DownloadJobId,
    pub url: DownloadUrl,
    pub target_path: PathBuf,
    pub expected_size: Option<u64>,
    pub checksum: Option<Checksum>,
    pub downloaded: u64,
    pub validator: Option<ResumeValidator>,
}

impl PartialDownloadMetadata {
    pub fn for_job(
        job: &DownloadJobSpec,
        downloaded: u64,
        validator: Option<ResumeValidator>,
    ) -> Self {
        Self {
            schema_version: PARTIAL_DOWNLOAD_METADATA_SCHEMA_VERSION,
            job_id: job.id.clone(),
            url: job.url.clone(),
            target_path: job.target_path.clone(),
            expected_size: job.expected_size,
            checksum: job.checksum.clone(),
            downloaded,
            validator,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResumeValidator {
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

fn ensure_parent_dir(path: &Path) -> Result<(), DownloadStorageError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    fs::create_dir_all(parent).map_err(|source| DownloadStorageError::CreateParentDirectory {
        path: path.to_path_buf(),
        source,
    })
}

fn fsync_path_if_configured(
    path: &Path,
    config: &DownloadStorageConfig,
) -> Result<(), DownloadStorageError> {
    if config.fsync_on_complete {
        let file = File::open(path).map_err(|source| DownloadStorageError::Fsync {
            path: path.to_path_buf(),
            source,
        })?;
        file.sync_all()
            .map_err(|source| DownloadStorageError::Fsync {
                path: path.to_path_buf(),
                source,
            })?;
    }
    Ok(())
}

fn fsync_parent_if_present(path: &Path) -> Result<(), DownloadStorageError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    let directory = File::open(parent).map_err(|source| DownloadStorageError::Fsync {
        path: parent.to_path_buf(),
        source,
    })?;
    directory
        .sync_all()
        .map_err(|source| DownloadStorageError::Fsync {
            path: parent.to_path_buf(),
            source,
        })
}

fn path_with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut value = path.as_os_str().to_os_string();
    value.push(suffix);
    PathBuf::from(value)
}
