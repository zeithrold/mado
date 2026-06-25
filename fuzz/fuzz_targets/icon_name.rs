#![no_main]

use std::collections::BTreeSet;

use libfuzzer_sys::fuzz_target;

#[path = "../../crates/icons/src/icon_name.rs"]
mod icon_name;

use icon_name::{unique_variant_name, variant_name};

fuzz_target!(|input: &str| {
    let variant = variant_name(input);
    let mut used = BTreeSet::new();
    let unique = unique_variant_name(input, &mut used);

    assert!(!variant.is_empty());
    assert!(variant.chars().all(|ch| ch.is_ascii_alphanumeric()));
    assert!(
        variant
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
    );
    assert!(!unique.is_empty());
    assert!(used.contains(&unique));
});
