use std::collections::BTreeMap;
use std::path::Path;

use crate::{JavaRuntimeError, probe::JavaProbe};

pub const MAX_METADATA_VALUE_BYTES: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaMetadata {
    pub version: String,
    pub vendor: String,
    pub architecture: String,
}

impl JavaMetadata {
    pub fn from_release_file(content: &str) -> Result<Self, JavaRuntimeError> {
        let values = parse_key_value_lines(content);
        let version = required_value(&values, "JAVA_VERSION")?;
        let vendor = match optional_value(&values, "IMPLEMENTOR")? {
            Some(vendor) => vendor,
            None => {
                optional_value(&values, "JAVA_VENDOR")?.unwrap_or_else(|| "Unknown".to_string())
            }
        };
        let architecture = match optional_value(&values, "OS_ARCH")? {
            Some(architecture) => architecture,
            None => {
                optional_value(&values, "SUN_ARCH_ABI")?.unwrap_or_else(|| "unknown".to_string())
            }
        };

        Ok(Self {
            version,
            vendor,
            architecture,
        })
    }

    pub fn from_probe_with(
        java_executable: &Path,
        probe: &impl JavaProbe,
    ) -> Result<Self, JavaRuntimeError> {
        let output = probe.run(java_executable)?;

        Self::from_probe_output(&output.stdout, &output.stderr)
    }

    pub fn from_probe_output(stdout: &str, stderr: &str) -> Result<Self, JavaRuntimeError> {
        let combined = format!("{stdout}\n{stderr}");
        let values = parse_probe_properties(&combined);

        Ok(Self {
            version: required_value(&values, "java.version")?,
            vendor: optional_value(&values, "java.vendor")?
                .unwrap_or_else(|| "Unknown".to_string()),
            architecture: optional_value(&values, "os.arch")?
                .unwrap_or_else(|| "unknown".to_string()),
        })
    }
}

pub fn parse_key_value_lines(content: &str) -> BTreeMap<String, String> {
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
    let value = values
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
        .ok_or(JavaRuntimeError::MissingMetadata { field: key })?;
    validate_metadata_value(key, value)
}

fn optional_value(
    values: &BTreeMap<String, String>,
    key: &'static str,
) -> Result<Option<String>, JavaRuntimeError> {
    let Some(value) = values
        .get(key)
        .filter(|value| !value.trim().is_empty())
        .cloned()
    else {
        return Ok(None);
    };

    validate_metadata_value(key, value).map(Some)
}

pub fn validate_metadata_value(
    key: &'static str,
    value: String,
) -> Result<String, JavaRuntimeError> {
    if value.len() > MAX_METADATA_VALUE_BYTES {
        return Err(JavaRuntimeError::MetadataValueTooLarge {
            field: key,
            max_bytes: MAX_METADATA_VALUE_BYTES,
        });
    }

    if value.chars().any(char::is_control) {
        return Err(JavaRuntimeError::MetadataValueContainsControl { field: key });
    }

    Ok(value)
}

fn unquote(value: &str) -> String {
    value.trim_matches('"').to_string()
}
