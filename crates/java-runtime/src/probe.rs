use std::path::Path;
use std::process::Command;

use crate::JavaRuntimeError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaProbeOutput {
    pub stdout: String,
    pub stderr: String,
}

pub trait JavaProbe {
    fn run(&self, java_executable: &Path) -> Result<JavaProbeOutput, JavaRuntimeError>;
}

pub struct CommandJavaProbe;

impl JavaProbe for CommandJavaProbe {
    fn run(&self, java_executable: &Path) -> Result<JavaProbeOutput, JavaRuntimeError> {
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

        Ok(JavaProbeOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}
