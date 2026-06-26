use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use sha1::{Digest, Sha1};
use sha2::Sha256;

use crate::{ArtifactVerifyError, ChecksumAlgorithm, DownloadIntegrityConfig, DownloadJobSpec};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVerification {
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtifactVerifier {
    integrity: DownloadIntegrityConfig,
}

impl ArtifactVerifier {
    pub const fn new(integrity: DownloadIntegrityConfig) -> Self {
        Self { integrity }
    }

    pub fn verify_job_target(
        &self,
        job: &DownloadJobSpec,
    ) -> Result<ArtifactVerification, ArtifactVerifyError> {
        self.verify_path(job, &job.target_path)
    }

    pub fn verify_path(
        &self,
        job: &DownloadJobSpec,
        path: impl AsRef<Path>,
    ) -> Result<ArtifactVerification, ArtifactVerifyError> {
        let path = path.as_ref();
        let metadata = path
            .metadata()
            .map_err(|source| ArtifactVerifyError::ReadMetadata {
                path: path.to_path_buf(),
                source,
            })?;
        let size = metadata.len();

        if self.integrity.require_size_when_available
            && let Some(expected) = job.expected_size
            && size != expected
        {
            return Err(ArtifactVerifyError::SizeMismatch {
                path: path.to_path_buf(),
                expected,
                actual: size,
            });
        }

        if self.integrity.require_checksum_when_available
            && let Some(checksum) = &job.checksum
        {
            let actual = compute_checksum(path, checksum.algorithm)?;
            if !checksum_matches(&actual, &checksum.value) {
                return Err(ArtifactVerifyError::ChecksumMismatch {
                    path: path.to_path_buf(),
                    algorithm: checksum.algorithm,
                    expected: checksum.value.clone(),
                    actual,
                });
            }
        }

        Ok(ArtifactVerification {
            path: path.to_path_buf(),
            size,
        })
    }
}

fn checksum_matches(actual: &str, expected: &str) -> bool {
    actual.eq_ignore_ascii_case(expected.trim())
}

fn compute_checksum(
    path: &Path,
    algorithm: ChecksumAlgorithm,
) -> Result<String, ArtifactVerifyError> {
    let file = File::open(path).map_err(|source| ArtifactVerifyError::Open {
        path: path.to_path_buf(),
        source,
    })?;
    let mut reader = BufReader::new(file);
    match algorithm {
        ChecksumAlgorithm::Sha1 => {
            let mut hasher = Sha1::new();
            copy_to_digest(&mut reader, &mut hasher, path)?;
            Ok(hex::encode(hasher.finalize()))
        }
        ChecksumAlgorithm::Sha256 => {
            let mut hasher = Sha256::new();
            copy_to_digest(&mut reader, &mut hasher, path)?;
            Ok(hex::encode(hasher.finalize()))
        }
    }
}

fn copy_to_digest<D>(
    reader: &mut impl Read,
    digest: &mut D,
    path: &Path,
) -> Result<(), ArtifactVerifyError>
where
    D: Digest,
{
    let mut buffer = [0; 8 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|source| ArtifactVerifyError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            return Ok(());
        }
        let Some(chunk) = buffer.get(..read) else {
            return Err(ArtifactVerifyError::ReadBufferOutOfBounds {
                path: path.to_path_buf(),
                read,
                capacity: buffer.len(),
            });
        };
        digest.update(chunk);
    }
}
