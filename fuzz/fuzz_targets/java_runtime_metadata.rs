#![no_main]

use libfuzzer_sys::fuzz_target;
use mado_java_runtime::fuzzing::{
    ParsedJavaMetadata, classify_architecture, classify_vendor, max_metadata_value_bytes,
    parse_java_version, parse_probe_metadata, parse_release_metadata,
};

fuzz_target!(|input: &[u8]| {
    let content = String::from_utf8_lossy(input);
    let (stdout, stderr) = split_on_char_boundary(&content);

    assert_deterministic(|| parse_release_metadata(&content));
    assert_deterministic(|| parse_probe_metadata(stdout, stderr));
    assert_deterministic(|| parse_java_version(&content));

    if let Ok(metadata) = parse_release_metadata(&content) {
        assert_metadata_invariants(&metadata);
    }

    if let Ok(metadata) = parse_probe_metadata(stdout, stderr) {
        assert_metadata_invariants(&metadata);
    }

    if let Ok(version) = parse_java_version(&content) {
        assert!(!version.raw.is_empty());
        assert_safe_metadata_value(&version.raw);
    }
});

fn split_on_char_boundary(content: &str) -> (&str, &str) {
    let char_count = content.chars().count();
    let midpoint = char_count / 2;
    let split_at = content
        .char_indices()
        .nth(midpoint)
        .map_or(content.len(), |(index, _)| index);

    content.split_at(split_at)
}

fn assert_metadata_invariants(metadata: &ParsedJavaMetadata) {
    assert!(!metadata.version.trim().is_empty());
    assert!(!metadata.vendor.trim().is_empty());
    assert!(!metadata.architecture.trim().is_empty());
    assert_safe_metadata_value(&metadata.version);
    assert_safe_metadata_value(&metadata.vendor);
    assert_safe_metadata_value(&metadata.architecture);

    let vendor_kind = classify_vendor(&metadata.vendor);
    let architecture_kind = classify_architecture(&metadata.architecture);

    assert!(!vendor_kind.to_string().is_empty());
    assert!(!architecture_kind.to_string().is_empty());
}

fn assert_safe_metadata_value(value: &str) {
    assert!(value.len() <= max_metadata_value_bytes());
    assert!(!value.chars().any(char::is_control));
}

fn assert_deterministic<T: std::fmt::Debug>(mut parse: impl FnMut() -> T) {
    let first = format!("{:?}", parse());
    let second = format!("{:?}", parse());

    assert_eq!(first, second);
}
