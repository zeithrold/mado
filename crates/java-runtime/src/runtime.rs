use std::path::PathBuf;

use crate::{JavaArchitecture, JavaVendor, JavaVersion};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JavaRuntimeInfo {
    pub java_home: PathBuf,
    pub java_executable: PathBuf,
    pub version: JavaVersion,
    pub vendor: JavaVendor,
    pub architecture: JavaArchitecture,
}
