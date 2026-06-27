#![no_main]

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use libfuzzer_sys::fuzz_target;
use mado_download::fuzzing;
use mado_download::{
    Checksum, ChecksumAlgorithm, DownloadArtifactKind, DownloadJobId, DownloadJobPolicy,
    DownloadJobSpec, DownloadResumeConfig, DownloadResumeMode, DownloadStorageConfig, DownloadUrl,
    NativeHttpBackendConfig, PARTIAL_DOWNLOAD_METADATA_SCHEMA_VERSION, PartialDownloadMetadata,
    ResumeValidator, ResumeValidatorPolicy,
};

fuzz_target!(|input: &[u8]| {
    let mut reader = FuzzReader::new(input);
    let root = temp_root(input);
    reset_dir(&root);

    exercise_arbitrary_metadata(&root, input, &mut reader);
    exercise_structured_metadata(&root, &mut reader);

    let _ = fs::remove_dir_all(&root);
});

fn exercise_arbitrary_metadata(root: &Path, input: &[u8], reader: &mut FuzzReader<'_>) {
    let job = download_job(root.join("arbitrary").join("artifact.jar"), reader);
    let config = backend_config(reader);
    let partial = byte_vec(reader, 128);

    let decision = fuzzing::resume_decision_for_inputs(&job, &config, root, &partial, input);
    if decision.should_resume {
        assert_eq!(decision.downloaded, partial.len() as u64);
        assert!(decision.downloaded >= config.resume.min_size);
        if config.resume.validator_policy == ResumeValidatorPolicy::RequireMatch {
            assert!(decision.if_range.is_some());
        }
    }
}

fn exercise_structured_metadata(root: &Path, reader: &mut FuzzReader<'_>) {
    let job = download_job(PathBuf::from("structured/artifact.jar"), reader);
    let config = backend_config(reader);
    let partial = byte_vec(reader, 128);
    let downloaded = if reader.bool() {
        partial.len() as u64
    } else {
        reader.next_u64()
    };
    let validator = validator(reader);
    let mut metadata_job = job.clone();
    let metadata_matches_job = !reader.bool();
    if !metadata_matches_job {
        mismatch_metadata_identity(&mut metadata_job, reader);
    }
    let metadata = PartialDownloadMetadata {
        schema_version: PARTIAL_DOWNLOAD_METADATA_SCHEMA_VERSION,
        job_id: metadata_job.id,
        url: metadata_job.url,
        target_path: metadata_job.target_path,
        expected_size: metadata_job.expected_size,
        checksum: metadata_job.checksum,
        downloaded,
        validator: validator.clone(),
    };
    let metadata_bytes =
        serde_json::to_vec(&metadata).expect("structured fuzz metadata should serialize");

    let decision = fuzzing::resume_decision_for_inputs(&job, &config, root, &partial, &metadata_bytes);
    let expected_if_range = validator
        .as_ref()
        .and_then(|value| value.etag.clone().or_else(|| value.last_modified.clone()));
    let should_resume = config.resume.mode == DownloadResumeMode::Enabled
        && metadata_matches_job
        && downloaded == partial.len() as u64
        && downloaded >= config.resume.min_size
        && (expected_if_range.is_some()
            || config.resume.validator_policy == ResumeValidatorPolicy::AllowMissingValidator);

    assert_eq!(decision.should_resume, should_resume);
    if should_resume {
        assert_eq!(decision.downloaded, downloaded);
        assert_eq!(decision.if_range, expected_if_range);
    } else {
        assert_eq!(decision.downloaded, 0);
        assert!(decision.if_range.is_none());
    }
}

fn backend_config(reader: &mut FuzzReader<'_>) -> NativeHttpBackendConfig {
    NativeHttpBackendConfig {
        storage: DownloadStorageConfig {
            temp_suffix: ".part".to_string(),
            metadata_suffix: ".part.json".to_string(),
            fsync_on_complete: false,
            atomic_rename: reader.bool(),
        },
        resume: DownloadResumeConfig {
            mode: if reader.bool() {
                DownloadResumeMode::Enabled
            } else {
                DownloadResumeMode::Disabled
            },
            min_size: reader.bounded_usize(129) as u64,
            validator_policy: if reader.bool() {
                ResumeValidatorPolicy::RequireMatch
            } else {
                ResumeValidatorPolicy::AllowMissingValidator
            },
            ..DownloadResumeConfig::default()
        },
        ..NativeHttpBackendConfig::default()
    }
}

fn download_job(target_path: PathBuf, reader: &mut FuzzReader<'_>) -> DownloadJobSpec {
    let checksum = reader.bool().then(|| Checksum {
        algorithm: if reader.bool() {
            ChecksumAlgorithm::Sha1
        } else {
            ChecksumAlgorithm::Sha256
        },
        value: short_ascii(reader, "checksum", 40),
    });

    DownloadJobSpec {
        id: DownloadJobId::new(format!("job-{}", reader.next_u8()))
            .expect("generated job ids are non-empty"),
        url: DownloadUrl::new(format!("https://example.test/artifact/{}", reader.next_u8()))
            .expect("generated urls are non-empty"),
        host: Some(format!("host-{}.example.test", reader.next_u8())),
        target_path,
        expected_size: reader.bool().then(|| reader.next_u64()),
        checksum,
        kind: DownloadArtifactKind::Asset,
        policy: DownloadJobPolicy {
            resumable: true,
            retryable: true,
        },
    }
}

fn mismatch_metadata_identity(job: &mut DownloadJobSpec, reader: &mut FuzzReader<'_>) {
    match reader.bounded_usize(5) {
        0 => {
            job.id = DownloadJobId::new(format!("other-{}", reader.next_u8()))
                .expect("generated job ids are non-empty");
        }
        1 => {
            job.url = DownloadUrl::new(format!("https://example.test/other/{}", reader.next_u8()))
                .expect("generated urls are non-empty");
        }
        2 => {
            job.target_path = PathBuf::from(format!("other/artifact-{}.jar", reader.next_u8()));
        }
        3 => {
            job.expected_size = Some(job.expected_size.unwrap_or(0).wrapping_add(1));
        }
        4 => {
            job.checksum = Some(Checksum {
                algorithm: ChecksumAlgorithm::Sha1,
                value: short_ascii(reader, "other", 40),
            });
        }
        _ => unreachable!(),
    }
}

fn validator(reader: &mut FuzzReader<'_>) -> Option<ResumeValidator> {
    reader.bool().then(|| ResumeValidator {
        etag: reader.bool().then(|| quoted_token(reader, "etag")),
        last_modified: reader.bool().then(|| short_ascii(reader, "date", 32)),
    })
}

fn byte_vec(reader: &mut FuzzReader<'_>, max_len: usize) -> Vec<u8> {
    let len = reader.bounded_usize(max_len + 1);
    (0..len).map(|_| reader.next_u8()).collect()
}

fn quoted_token(reader: &mut FuzzReader<'_>, prefix: &str) -> String {
    format!("\"{}\"", short_ascii(reader, prefix, 24))
}

fn short_ascii(reader: &mut FuzzReader<'_>, prefix: &str, max_extra: usize) -> String {
    let len = reader.bounded_usize(max_extra + 1);
    let mut value = String::from(prefix);
    for _ in 0..len {
        let byte = b'a' + reader.next_u8().wrapping_rem(26);
        value.push(char::from(byte));
    }
    value
}

fn temp_root(input: &[u8]) -> PathBuf {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in input.iter().take(1024) {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    std::env::temp_dir().join(format!(
        "mado-download-resume-decision-fuzz-{}-{hash:016x}",
        process::id()
    ))
}

fn reset_dir(path: &Path) {
    let _ = fs::remove_dir_all(path);
    fs::create_dir_all(path).expect("temporary fuzz directory should be creatable");
}

#[derive(Debug, Clone, Copy)]
struct FuzzReader<'a> {
    input: &'a [u8],
    cursor: usize,
}

impl<'a> FuzzReader<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { input, cursor: 0 }
    }

    fn next_u8(&mut self) -> u8 {
        let byte = self.input.get(self.cursor).copied().unwrap_or(0);
        self.cursor = self.cursor.saturating_add(1);
        byte
    }

    fn next_u64(&mut self) -> u64 {
        let mut bytes = [0_u8; 8];
        for byte in &mut bytes {
            *byte = self.next_u8();
        }
        u64::from_le_bytes(bytes)
    }

    fn bool(&mut self) -> bool {
        self.next_u8() % 2 == 1
    }

    fn bounded_usize(&mut self, upper_exclusive: usize) -> usize {
        debug_assert!(upper_exclusive > 0);
        usize::from(self.next_u8()) % upper_exclusive
    }
}
