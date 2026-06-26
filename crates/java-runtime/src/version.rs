use crate::{JavaRuntimeError, metadata::validate_metadata_value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaVersion {
    pub raw: String,
    pub major: u16,
}

impl JavaVersion {
    pub(crate) fn parse(raw: &str) -> Result<Self, JavaRuntimeError> {
        let raw = raw.trim().trim_matches('"').to_string();
        let raw = validate_metadata_value("version", raw)?;

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
