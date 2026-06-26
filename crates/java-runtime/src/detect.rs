use std::path::{Path, PathBuf};

use crate::{
    JavaArchitecture, JavaRuntimeError, JavaRuntimeInfo, JavaVendor, JavaVersion,
    metadata::JavaMetadata, probe::CommandJavaProbe, probe::JavaProbe,
};

const MAX_RELEASE_FILE_BYTES: u64 = 64 * 1024;

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
    detect_resolved_runtime_with_probe(java_home, java_executable, &CommandJavaProbe)
}

pub fn detect_resolved_runtime_with_probe(
    java_home: PathBuf,
    java_executable: PathBuf,
    probe: &impl JavaProbe,
) -> Result<JavaRuntimeInfo, JavaRuntimeError> {
    if !java_executable.exists() {
        return Err(JavaRuntimeError::ExecutableMissing {
            path: java_executable,
        });
    }

    let release_path = java_home.join("release");
    let metadata = if release_path.exists() {
        let size = std::fs::metadata(&release_path)
            .map_err(|source| JavaRuntimeError::ReadRelease {
                path: release_path.clone(),
                source,
            })?
            .len();
        if size > MAX_RELEASE_FILE_BYTES {
            return Err(JavaRuntimeError::ReleaseFileTooLarge {
                path: release_path,
                size,
                max_size: MAX_RELEASE_FILE_BYTES,
            });
        }

        let content = std::fs::read_to_string(&release_path).map_err(|source| {
            JavaRuntimeError::ReadRelease {
                path: release_path,
                source,
            }
        })?;
        match JavaMetadata::from_release_file(&content) {
            Ok(metadata) => metadata,
            Err(_) => JavaMetadata::from_probe_with(&java_executable, probe)?,
        }
    } else {
        JavaMetadata::from_probe_with(&java_executable, probe)?
    };

    Ok(JavaRuntimeInfo {
        java_home,
        java_executable,
        version: JavaVersion::parse(&metadata.version)?,
        vendor: JavaVendor::from_raw(metadata.vendor),
        architecture: JavaArchitecture::from_raw(metadata.architecture),
    })
}

pub fn java_executable_for_home(java_home: &Path) -> PathBuf {
    let binary = if cfg!(windows) { "java.exe" } else { "java" };
    java_home.join("bin").join(binary)
}

#[cfg(test)]
pub const fn max_release_file_bytes() -> u64 {
    MAX_RELEASE_FILE_BYTES
}
