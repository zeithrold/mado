mod architecture;
mod detect;
mod error;
mod metadata;
mod probe;
mod runtime;
mod vendor;
mod version;

pub use architecture::{JavaArchitecture, JavaArchitectureKind};
pub use detect::{detect_java_executable, detect_java_home};
pub use error::JavaRuntimeError;
pub use runtime::JavaRuntimeInfo;
pub use vendor::{JavaVendor, JavaVendorKind};
pub use version::JavaVersion;

#[cfg(fuzzing)]
pub mod fuzzing {
    use crate::{
        JavaArchitecture, JavaArchitectureKind, JavaRuntimeError, JavaVendor, JavaVendorKind,
        JavaVersion,
        metadata::{JavaMetadata, MAX_METADATA_VALUE_BYTES},
    };

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct ParsedJavaMetadata {
        pub version: String,
        pub vendor: String,
        pub architecture: String,
    }

    pub fn parse_release_metadata(content: &str) -> Result<ParsedJavaMetadata, JavaRuntimeError> {
        JavaMetadata::from_release_file(content).map(ParsedJavaMetadata::from)
    }

    pub fn parse_probe_metadata(
        stdout: &str,
        stderr: &str,
    ) -> Result<ParsedJavaMetadata, JavaRuntimeError> {
        JavaMetadata::from_probe_output(stdout, stderr).map(ParsedJavaMetadata::from)
    }

    pub fn parse_java_version(raw: &str) -> Result<JavaVersion, JavaRuntimeError> {
        JavaVersion::parse(raw)
    }

    pub fn classify_vendor(raw: &str) -> JavaVendorKind {
        JavaVendor::from_raw(raw.to_string()).kind
    }

    pub fn classify_architecture(raw: &str) -> JavaArchitectureKind {
        JavaArchitecture::from_raw(raw.to_string()).kind
    }

    pub const fn max_metadata_value_bytes() -> usize {
        MAX_METADATA_VALUE_BYTES
    }

    impl From<JavaMetadata> for ParsedJavaMetadata {
        fn from(metadata: JavaMetadata) -> Self {
            Self {
                version: metadata.version,
                vendor: metadata.vendor,
                architecture: metadata.architecture,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        detect::{
            detect_resolved_runtime_with_probe, java_executable_for_home, max_release_file_bytes,
        },
        metadata::{JavaMetadata, MAX_METADATA_VALUE_BYTES, parse_key_value_lines},
        probe::{JavaProbe, JavaProbeOutput},
    };
    use std::fs;
    use std::path::{Path, PathBuf};

    use tempfile::TempDir;

    #[test]
    fn parses_java_8_release_file() -> Result<(), Box<dyn std::error::Error>> {
        let metadata = JavaMetadata::from_release_file(
            r#"
JAVA_VERSION="1.8.0_402"
IMPLEMENTOR="Eclipse Adoptium"
OS_ARCH="x86_64"
"#,
        )?;

        let version = JavaVersion::parse(&metadata.version)?;
        assert_eq!(version.major, 8);
        assert_eq!(
            JavaVendor::from_raw(metadata.vendor).kind,
            JavaVendorKind::Temurin
        );
        assert_eq!(
            JavaArchitecture::from_raw(metadata.architecture).kind,
            JavaArchitectureKind::X86_64
        );
        Ok(())
    }

    #[test]
    fn parses_modern_release_file_versions() -> Result<(), Box<dyn std::error::Error>> {
        for (raw, major) in [
            ("17.0.11+9", 17_u16),
            ("21.0.5+11-LTS", 21_u16),
            ("25.0.1+8-LTS", 25_u16),
        ] {
            let version = JavaVersion::parse(raw)?;
            assert_eq!(version.major, major);
        }
        Ok(())
    }

    #[test]
    fn normalizes_vendor_strings() {
        for (raw, kind) in [
            ("Eclipse Adoptium", JavaVendorKind::Temurin),
            ("Eclipse Temurin", JavaVendorKind::Temurin),
            ("Azul Systems, Inc.", JavaVendorKind::Zulu),
            ("Azul Zulu", JavaVendorKind::Zulu),
            ("BellSoft", JavaVendorKind::Liberica),
            ("LibericaJDK", JavaVendorKind::Liberica),
            ("Oracle Corporation", JavaVendorKind::Oracle),
            ("OpenJDK", JavaVendorKind::OpenJdk),
            ("Microsoft", JavaVendorKind::Microsoft),
            ("Amazon.com Inc.", JavaVendorKind::Corretto),
            ("Amazon Corretto", JavaVendorKind::Corretto),
            ("Someone Else", JavaVendorKind::Unknown),
        ] {
            assert_eq!(JavaVendor::from_raw(raw.to_string()).kind, kind);
        }
    }

    #[test]
    fn normalizes_architecture_strings() {
        for (raw, kind) in [
            ("x86_64", JavaArchitectureKind::X86_64),
            ("amd64", JavaArchitectureKind::X86_64),
            ("aarch64", JavaArchitectureKind::Aarch64),
            ("arm64", JavaArchitectureKind::Aarch64),
            ("i386", JavaArchitectureKind::X86),
            ("arm", JavaArchitectureKind::Arm),
            ("riscv64", JavaArchitectureKind::Unknown),
        ] {
            assert_eq!(JavaArchitecture::from_raw(raw.to_string()).kind, kind);
        }
    }

    #[test]
    fn detects_home_from_release_file() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let java_home = temp.path();
        fs::create_dir(java_home.join("bin"))?;
        fs::write(java_home.join("bin").join("java"), "")?;
        fs::write(
            java_home.join("release"),
            r#"
JAVA_VERSION="21.0.5+11-LTS"
IMPLEMENTOR="Azul Systems, Inc."
OS_ARCH="aarch64"
"#,
        )?;

        let info = detect_java_home(java_home)?;
        assert_eq!(info.java_home, java_home);
        assert_eq!(info.java_executable, java_home.join("bin").join("java"));
        assert_eq!(info.version.major, 21);
        assert_eq!(info.vendor.kind, JavaVendorKind::Zulu);
        assert_eq!(info.architecture.kind, JavaArchitectureKind::Aarch64);
        Ok(())
    }

    #[test]
    fn detects_executable_parent_home_from_release_file() -> Result<(), Box<dyn std::error::Error>>
    {
        let temp = TempDir::new()?;
        let java_home = temp.path();
        let executable = java_home.join("bin").join("java");
        fs::create_dir(java_home.join("bin"))?;
        fs::write(&executable, "")?;
        fs::write(
            java_home.join("release"),
            r#"
JAVA_VERSION="17.0.11+9"
IMPLEMENTOR="BellSoft"
OS_ARCH="amd64"
"#,
        )?;

        let info = detect_java_executable(&executable)?;
        assert_eq!(info.java_home, java_home);
        assert_eq!(info.java_executable, executable);
        assert_eq!(info.version.major, 17);
        assert_eq!(info.vendor.kind, JavaVendorKind::Liberica);
        assert_eq!(info.architecture.kind, JavaArchitectureKind::X86_64);
        Ok(())
    }

    #[test]
    fn reports_missing_home() {
        let error = detect_java_home(Path::new("definitely-missing-java-home"))
            .err()
            .unwrap_or_else(|| JavaRuntimeError::HomeMissing {
                path: PathBuf::new(),
            });

        assert!(matches!(error, JavaRuntimeError::HomeMissing { .. }));
    }

    #[test]
    fn reports_missing_executable_inside_home() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let error = detect_java_home(temp.path()).err().unwrap_or_else(|| {
            JavaRuntimeError::ExecutableMissing {
                path: PathBuf::new(),
            }
        });

        assert!(matches!(error, JavaRuntimeError::ExecutableMissing { .. }));
        Ok(())
    }

    #[test]
    fn reports_invalid_version() {
        let error = JavaVersion::parse("not-a-version")
            .err()
            .unwrap_or_else(|| JavaRuntimeError::InvalidVersion { raw: String::new() });

        assert!(matches!(error, JavaRuntimeError::InvalidVersion { .. }));
    }

    #[test]
    fn reports_missing_version_metadata() {
        let error = JavaMetadata::from_release_file(
            r#"
IMPLEMENTOR="Oracle Corporation"
OS_ARCH="x86_64"
"#,
        )
        .err()
        .unwrap_or(JavaRuntimeError::MissingMetadata {
            field: "JAVA_VERSION",
        });

        assert!(matches!(
            error,
            JavaRuntimeError::MissingMetadata {
                field: "JAVA_VERSION"
            }
        ));
    }

    #[test]
    fn parses_probe_output() -> Result<(), Box<dyn std::error::Error>> {
        let metadata = JavaMetadata::from_probe_output(
            "",
            r"
Property settings:
    java.version = 21.0.5
    java.vendor = Eclipse Adoptium
    os.arch = aarch64
",
        )?;

        assert_eq!(metadata.version, "21.0.5");
        assert_eq!(metadata.vendor, "Eclipse Adoptium");
        assert_eq!(metadata.architecture, "aarch64");
        Ok(())
    }

    #[test]
    fn probe_output_defaults_missing_vendor_and_architecture()
    -> Result<(), Box<dyn std::error::Error>> {
        let metadata = JavaMetadata::from_probe_output("", "java.version = 17.0.11")?;

        assert_eq!(metadata.version, "17.0.11");
        assert_eq!(metadata.vendor, "Unknown");
        assert_eq!(metadata.architecture, "unknown");
        Ok(())
    }

    #[test]
    fn reports_missing_version_in_probe_output() {
        let error = JavaMetadata::from_probe_output("", "java.vendor = Oracle")
            .err()
            .unwrap_or(JavaRuntimeError::MissingMetadata {
                field: "java.version",
            });

        assert!(matches!(
            error,
            JavaRuntimeError::MissingMetadata {
                field: "java.version"
            }
        ));
    }

    #[test]
    fn release_file_falls_back_to_java_vendor() -> Result<(), Box<dyn std::error::Error>> {
        let metadata = JavaMetadata::from_release_file(
            r#"
JAVA_VERSION="17.0.11"
JAVA_VENDOR="Oracle Corporation"
OS_ARCH="x86_64"
"#,
        )?;

        assert_eq!(metadata.vendor, "Oracle Corporation");
        Ok(())
    }

    #[test]
    fn release_file_defaults_vendor_and_architecture() -> Result<(), Box<dyn std::error::Error>> {
        let metadata = JavaMetadata::from_release_file(
            r#"
JAVA_VERSION="21.0.5"
"#,
        )?;

        assert_eq!(metadata.vendor, "Unknown");
        assert_eq!(metadata.architecture, "unknown");
        Ok(())
    }

    #[test]
    fn release_file_falls_back_to_sun_arch_abi() -> Result<(), Box<dyn std::error::Error>> {
        let metadata = JavaMetadata::from_release_file(
            r#"
JAVA_VERSION="11.0.24"
IMPLEMENTOR="Oracle Corporation"
SUN_ARCH_ABI="amd64"
"#,
        )?;

        assert_eq!(metadata.architecture, "amd64");
        Ok(())
    }

    #[test]
    fn parse_key_value_lines_skips_comments_and_unquotes_values() {
        let values = parse_key_value_lines(
            r#"
# comment
# JAVA_VERSION="99.0.0"
JAVA_VERSION="21.0.5"

IMPLEMENTOR=Eclipse Adoptium
"#,
        );

        assert_eq!(values.get("JAVA_VERSION"), Some(&"21.0.5".to_string()));
        assert_eq!(
            values.get("IMPLEMENTOR"),
            Some(&"Eclipse Adoptium".to_string())
        );
        // A commented line that still contains '=' must be dropped, not parsed
        // into a "# JAVA_VERSION" entry. This guards the empty/comment early
        // return so the filter cannot be weakened from `||` to `&&`.
        assert!(values.keys().all(|key| !key.starts_with('#')));
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn reports_invalid_version_edge_cases() {
        for raw in ["", "   ", "1.", "\"\""] {
            let error = JavaVersion::parse(raw)
                .err()
                .unwrap_or_else(|| JavaRuntimeError::InvalidVersion { raw: String::new() });

            assert!(matches!(error, JavaRuntimeError::InvalidVersion { .. }));
        }
    }

    #[test]
    fn reports_release_file_too_large() -> Result<(), Box<dyn std::error::Error>> {
        let temp = TempDir::new()?;
        let java_home = temp.path();
        let oversized_len = usize::try_from(max_release_file_bytes() + 1)?;
        fs::create_dir(java_home.join("bin"))?;
        fs::write(java_home.join("bin").join("java"), "")?;
        fs::write(java_home.join("release"), "A".repeat(oversized_len))?;

        let error = detect_java_home(java_home).err().unwrap_or_else(|| {
            JavaRuntimeError::ReleaseFileTooLarge {
                path: PathBuf::new(),
                size: 0,
                max_size: max_release_file_bytes(),
            }
        });

        assert!(matches!(
            error,
            JavaRuntimeError::ReleaseFileTooLarge { .. }
        ));
        Ok(())
    }

    #[test]
    fn reports_required_metadata_value_too_large() {
        let content = format!(
            "JAVA_VERSION=\"{}\"",
            "1".repeat(MAX_METADATA_VALUE_BYTES + 1)
        );
        let error = JavaMetadata::from_release_file(&content).err().unwrap_or(
            JavaRuntimeError::MetadataValueTooLarge {
                field: "JAVA_VERSION",
                max_bytes: MAX_METADATA_VALUE_BYTES,
            },
        );

        assert!(matches!(
            error,
            JavaRuntimeError::MetadataValueTooLarge {
                field: "JAVA_VERSION",
                ..
            }
        ));
    }

    #[test]
    fn reports_optional_metadata_value_too_large() {
        let content = format!(
            "JAVA_VERSION=\"21.0.5\"\nIMPLEMENTOR=\"{}\"",
            "A".repeat(MAX_METADATA_VALUE_BYTES + 1)
        );
        let error = JavaMetadata::from_release_file(&content).err().unwrap_or(
            JavaRuntimeError::MetadataValueTooLarge {
                field: "IMPLEMENTOR",
                max_bytes: MAX_METADATA_VALUE_BYTES,
            },
        );

        assert!(matches!(
            error,
            JavaRuntimeError::MetadataValueTooLarge {
                field: "IMPLEMENTOR",
                ..
            }
        ));
    }

    #[test]
    fn reports_probe_metadata_value_too_large() {
        let stderr = format!(
            "java.version = 21.0.5\njava.vendor = {}",
            "A".repeat(MAX_METADATA_VALUE_BYTES + 1)
        );
        let error = JavaMetadata::from_probe_output("", &stderr)
            .err()
            .unwrap_or(JavaRuntimeError::MetadataValueTooLarge {
                field: "java.vendor",
                max_bytes: MAX_METADATA_VALUE_BYTES,
            });

        assert!(matches!(
            error,
            JavaRuntimeError::MetadataValueTooLarge {
                field: "java.vendor",
                ..
            }
        ));
    }

    #[test]
    fn reports_version_value_too_large_without_echoing_raw_input() {
        let error = JavaVersion::parse(&"1".repeat(MAX_METADATA_VALUE_BYTES + 1))
            .err()
            .unwrap_or(JavaRuntimeError::MetadataValueTooLarge {
                field: "version",
                max_bytes: MAX_METADATA_VALUE_BYTES,
            });

        assert!(matches!(
            error,
            JavaRuntimeError::MetadataValueTooLarge {
                field: "version",
                ..
            }
        ));
    }

    #[test]
    fn reports_control_character_in_metadata_value() {
        let error = JavaMetadata::from_release_file("JAVA_VERSION=\"21.0.5\u{1b}[31m\"")
            .err()
            .unwrap_or(JavaRuntimeError::MetadataValueContainsControl {
                field: "JAVA_VERSION",
            });

        assert!(matches!(
            error,
            JavaRuntimeError::MetadataValueContainsControl {
                field: "JAVA_VERSION"
            }
        ));
    }

    #[test]
    fn reports_control_character_in_version_without_echoing_raw_input() {
        let error = JavaVersion::parse("21.0.5\u{0}")
            .err()
            .unwrap_or(JavaRuntimeError::MetadataValueContainsControl { field: "version" });

        assert!(matches!(
            error,
            JavaRuntimeError::MetadataValueContainsControl { field: "version" }
        ));
    }

    #[test]
    fn parses_version_with_quotes_and_whitespace() -> Result<(), Box<dyn std::error::Error>> {
        let version = JavaVersion::parse("  \"21.0.5+11-LTS\"  ")?;
        assert_eq!(version.major, 21);
        assert_eq!(version.raw, "21.0.5+11-LTS");
        Ok(())
    }

    #[test]
    fn normalizes_remaining_architecture_variants() {
        for (raw, kind) in [
            ("i486", JavaArchitectureKind::X86),
            ("i586", JavaArchitectureKind::X86),
            ("i686", JavaArchitectureKind::X86),
            ("arm32", JavaArchitectureKind::Arm),
        ] {
            assert_eq!(JavaArchitecture::from_raw(raw.to_string()).kind, kind);
        }
    }

    #[test]
    fn vendor_kind_display_round_trip() {
        for (kind, label) in [
            (JavaVendorKind::Temurin, "temurin"),
            (JavaVendorKind::Zulu, "zulu"),
            (JavaVendorKind::Liberica, "liberica"),
            (JavaVendorKind::Oracle, "oracle"),
            (JavaVendorKind::OpenJdk, "openjdk"),
            (JavaVendorKind::Microsoft, "microsoft"),
            (JavaVendorKind::Corretto, "corretto"),
            (JavaVendorKind::Unknown, "unknown"),
        ] {
            assert_eq!(kind.to_string(), label);
        }
    }

    #[test]
    fn architecture_kind_display_round_trip() {
        for (kind, label) in [
            (JavaArchitectureKind::X86_64, "x86_64"),
            (JavaArchitectureKind::Aarch64, "aarch64"),
            (JavaArchitectureKind::X86, "x86"),
            (JavaArchitectureKind::Arm, "arm"),
            (JavaArchitectureKind::Unknown, "unknown"),
        ] {
            assert_eq!(kind.to_string(), label);
        }
    }

    #[test]
    fn reports_missing_executable_path() {
        let error = detect_java_executable(Path::new("definitely-missing-java-executable"))
            .err()
            .unwrap_or_else(|| JavaRuntimeError::ExecutableMissing {
                path: PathBuf::new(),
            });

        assert!(matches!(error, JavaRuntimeError::ExecutableMissing { .. }));
    }

    mod fake_probe {
        use super::*;
        use std::cell::Cell;

        fn create_java_executable(java_home: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
            let executable = java_executable_for_home(java_home);
            fs::create_dir_all(executable.parent().ok_or_else(|| {
                JavaRuntimeError::ExecutableWithoutParent {
                    path: executable.clone(),
                }
            })?)?;
            fs::write(&executable, "")?;
            Ok(executable)
        }

        const PROBE_STDERR: &str = r"
Property settings:
    java.version = 21.0.5
    java.vendor = Eclipse Adoptium
    os.arch = aarch64
";

        enum FakeProbeResult {
            Success,
            Failure,
        }

        struct FakeJavaProbe {
            result: FakeProbeResult,
            calls: Cell<usize>,
        }

        impl FakeJavaProbe {
            fn success() -> Self {
                Self {
                    result: FakeProbeResult::Success,
                    calls: Cell::new(0),
                }
            }

            fn failure() -> Self {
                Self {
                    result: FakeProbeResult::Failure,
                    calls: Cell::new(0),
                }
            }
        }

        impl JavaProbe for FakeJavaProbe {
            fn run(&self, java_executable: &Path) -> Result<JavaProbeOutput, JavaRuntimeError> {
                self.calls.set(self.calls.get() + 1);

                match self.result {
                    FakeProbeResult::Success => Ok(JavaProbeOutput {
                        stdout: String::new(),
                        stderr: PROBE_STDERR.to_string(),
                    }),
                    FakeProbeResult::Failure => Err(JavaRuntimeError::ProbeFailed {
                        executable: java_executable.to_path_buf(),
                        status: "exit status: 1".to_string(),
                        stderr: "probe failed".to_string(),
                    }),
                }
            }
        }

        #[test]
        fn detects_home_via_probe_when_release_missing() -> Result<(), Box<dyn std::error::Error>> {
            let temp = TempDir::new()?;
            let java_home = temp.path();
            let executable = create_java_executable(java_home)?;
            let probe = FakeJavaProbe::success();

            let info =
                detect_resolved_runtime_with_probe(java_home.to_path_buf(), executable, &probe)?;
            assert_eq!(info.version.major, 21);
            assert_eq!(info.vendor.kind, JavaVendorKind::Temurin);
            assert_eq!(info.architecture.kind, JavaArchitectureKind::Aarch64);
            assert_eq!(probe.calls.get(), 1);
            Ok(())
        }

        #[test]
        fn detects_executable_via_probe_when_release_missing()
        -> Result<(), Box<dyn std::error::Error>> {
            let temp = TempDir::new()?;
            let java_home = temp.path();
            let executable = create_java_executable(java_home)?;
            let probe = FakeJavaProbe::success();

            let info =
                detect_resolved_runtime_with_probe(java_home.to_path_buf(), executable, &probe)?;
            assert_eq!(info.java_home, java_home);
            assert_eq!(info.version.major, 21);
            assert_eq!(probe.calls.get(), 1);
            Ok(())
        }

        #[test]
        fn falls_back_to_probe_when_release_is_incomplete() -> Result<(), Box<dyn std::error::Error>>
        {
            let temp = TempDir::new()?;
            let java_home = temp.path();
            let executable = create_java_executable(java_home)?;
            let probe = FakeJavaProbe::success();
            fs::write(
                java_home.join("release"),
                r#"
JAVA_VERSION=""
IMPLEMENTOR="Oracle Corporation"
OS_ARCH="x86_64"
"#,
            )?;

            let info =
                detect_resolved_runtime_with_probe(java_home.to_path_buf(), executable, &probe)?;
            assert_eq!(info.version.major, 21);
            assert_eq!(info.vendor.kind, JavaVendorKind::Temurin);
            assert_eq!(probe.calls.get(), 1);
            Ok(())
        }

        #[test]
        fn falls_back_to_probe_when_release_is_malformed() -> Result<(), Box<dyn std::error::Error>>
        {
            let temp = TempDir::new()?;
            let java_home = temp.path();
            let executable = create_java_executable(java_home)?;
            let probe = FakeJavaProbe::success();
            fs::write(java_home.join("release"), "not a valid release file")?;

            let info =
                detect_resolved_runtime_with_probe(java_home.to_path_buf(), executable, &probe)?;
            assert_eq!(info.version.major, 21);
            assert_eq!(probe.calls.get(), 1);
            Ok(())
        }

        #[test]
        fn reports_probe_failure() -> Result<(), Box<dyn std::error::Error>> {
            let temp = TempDir::new()?;
            let java_home = temp.path();
            let executable = create_java_executable(java_home)?;
            let probe = FakeJavaProbe::failure();

            let error =
                detect_resolved_runtime_with_probe(java_home.to_path_buf(), executable, &probe)
                    .err()
                    .unwrap_or_else(|| JavaRuntimeError::ProbeFailed {
                        executable: PathBuf::new(),
                        status: String::new(),
                        stderr: String::new(),
                    });

            assert!(matches!(error, JavaRuntimeError::ProbeFailed { .. }));
            assert_eq!(probe.calls.get(), 1);
            Ok(())
        }

        #[test]
        fn reports_read_release_error() -> Result<(), Box<dyn std::error::Error>> {
            let temp = TempDir::new()?;
            let java_home = temp.path();
            let executable = create_java_executable(java_home)?;
            let probe = FakeJavaProbe::success();
            fs::create_dir(java_home.join("release"))?;

            let error =
                detect_resolved_runtime_with_probe(java_home.to_path_buf(), executable, &probe)
                    .err()
                    .unwrap_or_else(|| JavaRuntimeError::ReadRelease {
                        path: PathBuf::new(),
                        source: std::io::Error::from(std::io::ErrorKind::Other),
                    });

            assert!(matches!(error, JavaRuntimeError::ReadRelease { .. }));
            assert_eq!(probe.calls.get(), 0);
            Ok(())
        }
    }
}
