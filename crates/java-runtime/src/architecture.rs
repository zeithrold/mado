use std::fmt;

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

impl JavaArchitecture {
    pub(crate) fn from_raw(raw: String) -> Self {
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
