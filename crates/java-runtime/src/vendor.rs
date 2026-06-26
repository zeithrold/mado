use std::fmt;

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

impl JavaVendor {
    pub(crate) fn from_raw(raw: String) -> Self {
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
