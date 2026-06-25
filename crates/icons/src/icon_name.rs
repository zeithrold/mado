use std::collections::BTreeSet;

pub fn unique_variant_name(stem: &str, used: &mut BTreeSet<String>) -> String {
    let base = variant_name(stem);
    if used.insert(base.clone()) {
        return base;
    }

    for index in 2.. {
        let candidate = format!("{base}{index}");
        if used.insert(candidate.clone()) {
            return candidate;
        }
    }

    unreachable!()
}

pub fn variant_name(stem: &str) -> String {
    let mut output = String::new();

    for part in stem.split(|ch: char| !ch.is_ascii_alphanumeric()) {
        if part.is_empty() {
            continue;
        }

        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            output.extend(first.to_uppercase());
            output.push_str(chars.as_str());
        }
    }

    if output.is_empty() {
        output.push_str("Icon");
    }

    if output.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        output.insert_str(0, "Icon");
    }

    match output.as_str() {
        "Self" | "Super" | "Crate" | "Extern" => output.push_str("Icon"),
        _ => {}
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_pascal_case_variant_names() {
        assert_eq!(
            variant_name("align-center-horizontal"),
            "AlignCenterHorizontal"
        );
        assert_eq!(variant_name("badge_3d"), "Badge3d");
    }

    #[test]
    fn prefixes_numeric_names() {
        assert_eq!(variant_name("360-degrees"), "Icon360Degrees");
    }

    #[test]
    fn handles_empty_or_symbol_only_names() {
        assert_eq!(variant_name(""), "Icon");
        assert_eq!(variant_name("---"), "Icon");
    }

    #[test]
    fn avoids_reserved_keywords() {
        assert_eq!(variant_name("self"), "SelfIcon");
        assert_eq!(variant_name("super"), "SuperIcon");
        assert_eq!(variant_name("crate"), "CrateIcon");
        assert_eq!(variant_name("extern"), "ExternIcon");
    }

    #[test]
    fn disambiguates_duplicate_variant_names() {
        let mut used = BTreeSet::new();
        assert_eq!(unique_variant_name("arrow-left", &mut used), "ArrowLeft");
        assert_eq!(unique_variant_name("arrow_left", &mut used), "ArrowLeft2");
        assert_eq!(unique_variant_name("arrow.left", &mut used), "ArrowLeft3");
    }
}
