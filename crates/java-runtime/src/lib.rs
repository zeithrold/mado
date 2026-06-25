use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRuntimeInfo {
    pub java_home: PathBuf,
    pub java_executable: PathBuf,
    pub version: JavaVersion,
    pub vendor: JavaVendor,
    pub architecture: JavaArchitecture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaVersion {
    pub raw: String,
    pub major: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaVendor {
    pub raw: String,
    pub kind: JavaVendorKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaVendorKind {
    Temurin,
    Zulu,
    Liberica,
    Oracle,
    OpenJdk,
    Microsoft,
    Corretto,
    Unknown,
}

impl fmt::Display for JavaVendorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Temurin => f.write_str("temurin"),
            Self::Zulu => f.write_str("zulu"),
            Self::Liberica => f.write_str("liberica"),
            Self::Oracle => f.write_str("oracle"),
            Self::OpenJdk => f.write_str("openjdk"),
            Self::Microsoft => f.write_str("microsoft"),
            Self::Corretto => f.write_str("corretto"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaArchitecture {
    pub raw: String,
    pub kind: JavaArchitectureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JavaArchitectureKind {
    X86_64,
    Aarch64,
    X86,
    Arm,
    Unknown,
}

impl fmt::Display for JavaArchitectureKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::X86_64 => f.write_str("x86_64"),
            Self::Aarch64 => f.write_str("aarch64"),
            Self::X86 => f.write_str("x86"),
            Self::Arm => f.write_str("arm"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

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
    #[error("Java metadata is missing required field: {field}")]
    MissingMetadata { field: &'static str },
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

pub fn detect_java_home(path: impl AsRef<Path>) -> Result<JavaRuntimeInfo, JavaRuntimeError> {
    let java_home = path.as_ref();
    if !java_home.exists() {
        return Err(JavaRuntimeError::HomeMissing {
            path: java_home.to_path_buf(),
        });
    }

    let java_executable = java_executable_for_home(java_home);
    detect_resolved_runtime(java_home.to_path_buf(), java_executable)
}

pub fn detect_java_executable(path: impl AsRef<Path>) -> Result<JavaRuntimeInfo, JavaRuntimeError> {
    let java_executable = path.as_ref();
    if !java_executable.exists() {
        return Err(JavaRuntimeError::ExecutableMissing {
            path: java_executable.to_path_buf(),
        });
    }

    let bin_dir =
        java_executable
            .parent()
            .ok_or_else(|| JavaRuntimeError::ExecutableWithoutParent {
                path: java_executable.to_path_buf(),
            })?;
    let java_home = bin_dir
        .parent()
        .ok_or_else(|| JavaRuntimeError::ExecutableWithoutParent {
            path: java_executable.to_path_buf(),
        })?;

    detect_resolved_runtime(java_home.to_path_buf(), java_executable.to_path_buf())
}

fn detect_resolved_runtime(
    java_home: PathBuf,
    java_executable: PathBuf,
) -> Result<JavaRuntimeInfo, JavaRuntimeError> {
    if !java_executable.exists() {
        return Err(JavaRuntimeError::ExecutableMissing {
            path: java_executable,
        });
    }

    let release_path = java_home.join("release");
    let metadata = if release_path.exists() {
        let content = std::fs::read_to_string(&release_path).map_err(|source| {
            JavaRuntimeError::ReadRelease {
                path: release_path,
                source,
            }
        })?;
        match JavaMetadata::from_release_file(&content) {
            Ok(metadata) if metadata.is_complete() => metadata,
            Ok(_) | Err(_) => JavaMetadata::from_probe(&java_executable)?,
        }
    } else {
        JavaMetadata::from_probe(&java_executable)?
    };

    Ok(JavaRuntimeInfo {
        java_home,
        java_executable,
        version: JavaVersion::parse(&metadata.version)?,
        vendor: JavaVendor::from_raw(metadata.vendor),
        architecture: JavaArchitecture::from_raw(metadata.architecture),
    })
}

fn java_executable_for_home(java_home: &Path) -> PathBuf {
    let binary = if cfg!(windows) { "java.exe" } else { "java" };
    java_home.join("bin").join(binary)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct JavaMetadata {
    version: String,
    vendor: String,
    architecture: String,
}

impl JavaMetadata {
    fn from_release_file(content: &str) -> Result<Self, JavaRuntimeError> {
        let values = parse_key_value_lines(content);
        let version = required_value(&values, "JAVA_VERSION")?;
        let vendor = optional_value(&values, "IMPLEMENTOR")
            .or_else(|| optional_value(&values, "JAVA_VENDOR"))
            .unwrap_or_else(|| "Unknown".to_string());
        let architecture = optional_value(&values, "OS_ARCH")
            .or_else(|| optional_value(&values, "SUN_ARCH_ABI"))
            .unwrap_or_else(|| "unknown".to_string());

        Ok(Self {
            version,
            vendor,
            architecture,
        })
    }

    fn from_probe(java_executable: &Path) -> Result<Self, JavaRuntimeError> {
        let output = Command::new(java_executable)
            .args(["-XshowSettings:properties", "-version"])
            .output()
            .map_err(|source| JavaRuntimeError::ProbeStart {
                executable: java_executable.to_path_buf(),
                source,
            })?;

        if !output.status.success() {
            return Err(JavaRuntimeError::ProbeFailed {
                executable: java_executable.to_path_buf(),
                status: output.status.to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let combined = format!("{stdout}\n{stderr}");
        let values = parse_probe_properties(&combined);

        Ok(Self {
            version: required_value(&values, "java.version")?,
            vendor: optional_value(&values, "java.vendor").unwrap_or_else(|| "Unknown".to_string()),
            architecture: optional_value(&values, "os.arch")
                .unwrap_or_else(|| "unknown".to_string()),
        })
    }

    fn is_complete(&self) -> bool {
        !self.version.trim().is_empty()
            && !self.vendor.trim().is_empty()
            && !self.architecture.trim().is_empty()
    }
}

impl JavaVersion {
    fn parse(raw: &str) -> Result<Self, JavaRuntimeError> {
        let raw = raw.trim().trim_matches('"').to_string();
        let major_text = raw.strip_prefix("1.").map_or_else(
            || raw.split(['.', '_', '-']).next(),
            |rest| rest.split(['.', '_', '-']).next(),
        );

        let major = major_text
            .filter(|part| !part.is_empty())
            .and_then(|part| part.parse::<u16>().ok())
            .ok_or_else(|| JavaRuntimeError::InvalidVersion { raw: raw.clone() })?;

        Ok(Self { raw, major })
    }
}

impl JavaVendor {
    fn from_raw(raw: String) -> Self {
        let normalized = raw.to_ascii_lowercase();
        let kind = if normalized.contains("temurin")
            || normalized.contains("adoptium")
            || normalized.contains("eclipse foundation")
        {
            JavaVendorKind::Temurin
        } else if normalized.contains("zulu") || normalized.contains("azul") {
            JavaVendorKind::Zulu
        } else if normalized.contains("liberica") || normalized.contains("bellsoft") {
            JavaVendorKind::Liberica
        } else if normalized.contains("oracle") {
            JavaVendorKind::Oracle
        } else if normalized.contains("microsoft") {
            JavaVendorKind::Microsoft
        } else if normalized.contains("corretto") || normalized.contains("amazon") {
            JavaVendorKind::Corretto
        } else if normalized.contains("openjdk") {
            JavaVendorKind::OpenJdk
        } else {
            JavaVendorKind::Unknown
        };

        Self { raw, kind }
    }
}

impl JavaArchitecture {
    fn from_raw(raw: String) -> Self {
        let normalized = raw.to_ascii_lowercase();
        let kind = match normalized.as_str() {
            "x86_64" | "amd64" => JavaArchitectureKind::X86_64,
            "aarch64" | "arm64" => JavaArchitectureKind::Aarch64,
            "x86" | "i386" | "i486" | "i586" | "i686" => JavaArchitectureKind::X86,
            "arm" | "arm32" => JavaArchitectureKind::Arm,
            _ => JavaArchitectureKind::Unknown,
        };

        Self { raw, kind }
    }
}

fn parse_key_value_lines(content: &str) -> BTreeMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }

            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), unquote(value.trim())))
        })
        .collect()
}

fn parse_probe_properties(content: &str) -> BTreeMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let (key, value) = trimmed.split_once('=')?;
            Some((key.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn required_value(
    values: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<String, JavaRuntimeError> {
    values
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or(JavaRuntimeError::MissingMetadata { field: key })
}

fn optional_value(values: &BTreeMap<String, String>, key: &str) -> Option<String> {
    values
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
}

fn unquote(value: &str) -> String {
    value.trim_matches('"').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
}
