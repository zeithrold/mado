#![no_main]

use std::fs;
use std::path::{Path, PathBuf};
use std::process;

use libfuzzer_sys::fuzz_target;
use mado_download::{
    Checksum, ChecksumAlgorithm, DownloadArtifactKind, DownloadJobId, DownloadJobPolicy,
    DownloadJobSpec, DownloadStorageConfig, DownloadStorageError, DownloadStoragePaths,
    DownloadUrl, PARTIAL_DOWNLOAD_METADATA_SCHEMA_VERSION, PartialDownloadMetadata,
    ResumeValidator,
};

fuzz_target!(|input: &[u8]| {
    let mut reader = FuzzReader::new(input);
    let root = temp_root(input);
    reset_dir(&root);

    parse_arbitrary_metadata(&root, input, &mut reader);
    assert_partial_metadata_roundtrip(&root, &mut reader);

    let _ = fs::remove_dir_all(&root);
});

fn parse_arbitrary_metadata(root: &Path, input: &[u8], reader: &mut FuzzReader<'_>) {
    let config = storage_config(reader);
    let paths = DownloadStoragePaths::for_target(root.join("arbitrary").join("artifact.jar"), &config);
    paths
        .ensure_parent_dirs()
        .expect("temporary metadata directories should be creatable");
    paths
        .write_partial_bytes(b"partial", &config)
        .expect("temporary partial artifact should be writable");
    fs::write(&paths.partial_metadata_path, input)
        .expect("temporary partial metadata should be writable");

    let result = paths.read_partial_metadata();
    match result {
        Ok(metadata) => {
            assert_eq!(
                metadata.schema_version,
                PARTIAL_DOWNLOAD_METADATA_SCHEMA_VERSION
            );
        }
        Err(DownloadStorageError::UnsupportedPartialMetadataVersion { version, .. }) => {
            assert_ne!(version, PARTIAL_DOWNLOAD_METADATA_SCHEMA_VERSION);
            assert!(paths.partial_path.exists());
            assert!(paths.partial_metadata_path.exists());
        }
        Err(DownloadStorageError::ParsePartialMetadata { .. }) => {
            assert!(paths.partial_path.exists());
            assert!(paths.partial_metadata_path.exists());
        }
        Err(DownloadStorageError::ReadPartialMetadata { .. }) => {
            panic!("metadata file was written before reading");
        }
        Err(error) => {
            panic!("unexpected metadata read error: {error}");
        }
    }
}

fn assert_partial_metadata_roundtrip(root: &Path, reader: &mut FuzzReader<'_>) {
    let config = storage_config(reader);
    let target = root.join("roundtrip").join(format!("artifact-{}.jar", reader.next_u8()));
    let paths = DownloadStoragePaths::for_target(&target, &config);
    let job = download_job(target, reader);
    let validator = reader.bool().then(|| ResumeValidator {
        etag: reader.bool().then(|| quoted_token(reader, "etag")),
        last_modified: reader.bool().then(|| short_ascii(reader, "date", 32)),
    });
    let metadata = PartialDownloadMetadata::for_job(&job, reader.next_u64(), validator);

    paths
        .write_partial_metadata(&metadata, &config)
        .expect("temporary partial metadata should be writable");
    let parsed = paths
        .read_partial_metadata()
        .expect("metadata written by mado-download should parse");

    assert_eq!(parsed, metadata);
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
        host: reader
            .bool()
            .then(|| format!("host-{}.example.test", reader.next_u8())),
        target_path,
        expected_size: reader.bool().then(|| reader.next_u64()),
        checksum,
        kind: match reader.bounded_usize(6) {
            0 => DownloadArtifactKind::VersionMetadata,
            1 => DownloadArtifactKind::ClientJar,
            2 => DownloadArtifactKind::Library,
            3 => DownloadArtifactKind::Asset,
            4 => DownloadArtifactKind::Native,
            5 => DownloadArtifactKind::JavaRuntime,
            _ => unreachable!(),
        },
        policy: DownloadJobPolicy {
            resumable: reader.bool(),
            retryable: reader.bool(),
        },
    }
}

fn storage_config(reader: &mut FuzzReader<'_>) -> DownloadStorageConfig {
    DownloadStorageConfig {
        temp_suffix: suffix(reader, ".part"),
        metadata_suffix: suffix(reader, ".part.json"),
        fsync_on_complete: false,
        atomic_rename: reader.bool(),
    }
}

fn suffix(reader: &mut FuzzReader<'_>, fallback: &str) -> String {
    let len = 1 + reader.bounded_usize(16);
    let mut value = String::from(".");
    for _ in 0..len {
        let byte = b'a' + reader.next_u8().wrapping_rem(26);
        value.push(char::from(byte));
    }
    if value == "." {
        fallback.to_string()
    } else {
        value
    }
}

fn quoted_token(reader: &mut FuzzReader<'_>, prefix: &str) -> String {
    format!("\"{}\"", short_ascii(reader, prefix, 24))
}

fn short_ascii(reader: &mut FuzzReader<'_>, prefix: &str, max_extra: usize) -> String {
    let len = reader.bounded_usize(max_extra + 1);
    let mut value = String::from(prefix);
    for _ in 0..len {
        let byte = match reader.bounded_usize(36) {
            value @ 0..=25 => b'a' + value as u8,
            value => b'0' + (value as u8 - 26),
        };
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
        "mado-download-partial-metadata-fuzz-{}-{hash:016x}",
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
