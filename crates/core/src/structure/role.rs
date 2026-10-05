//! `/RoleMap` resolution: a producer's own structure types back to the
//! standard ones a consumer understands.
//!
//! The map lives on the `/StructTreeRoot` dictionary. A custom type maps to
//! another type, which may itself be custom, so resolution follows the chain
//! until it lands on a standard type. A chain that cycles, runs past
//! [`MAX_CHAIN`] or ends on a type that is neither mapped nor standard
//! resolves to nothing: the file names a role no consumer can interpret, and
//! inventing one would hide that.
//!
//! What is standard depends on the element's namespace. ISO 32000-1 14.8.4
//! defines `H1` to `H6` and no `Title`, `Em` or `Strong`, so for an element in
//! the default (PDF 1.7) namespace those are custom types that a `/RoleMap` is
//! entitled to define (a PDF/UA-1 file that maps `/Title` to `/P` is
//! conformant, and so is a PDF 2.0 file's element with no `/NS`). ISO 32000-2
//! adds them, `Artifact` and `Hn` for any `n`, in its own namespace.

use std::collections::BTreeMap;

use onionskin_cos::Name;

/// A real map chains two or three deep. This bounds a hostile one: every
/// element resolves its type, so an unbounded chain makes a read quadratic.
const MAX_CHAIN: usize = 32;

/// ISO 32000-1 14.8.4, Tables 333 to 340, without `H1` to `H6`.
const ISO_32000_1: &[&str] = &[
    "Document",
    "Part",
    "Art",
    "Sect",
    "Div",
    "BlockQuote",
    "Caption",
    "TOC",
    "TOCI",
    "Index",
    "NonStruct",
    "Private",
    "P",
    "H",
    "L",
    "LI",
    "Lbl",
    "LBody",
    "Table",
    "TR",
    "TH",
    "TD",
    "THead",
    "TBody",
    "TFoot",
    "Span",
    "Quote",
    "Note",
    "Reference",
    "BibEntry",
    "Code",
    "Link",
    "Annot",
    "Ruby",
    "RB",
    "RT",
    "RP",
    "Warichu",
    "WT",
    "WP",
    "Figure",
    "Formula",
    "Form",
];

/// What ISO 32000-2 14.8.4 adds to the 1.7 set.
const ISO_32000_2_ADDED: &[&str] = &[
    "DocumentFragment",
    "Aside",
    "Title",
    "FENote",
    "Sub",
    "Em",
    "Strong",
    "Artifact",
];

/// Whether `name` is a standard structure type in the PDF 1.7 namespace or, with
/// `iso_32000_2`, the PDF 2.0 one. `H` followed by a number without a leading
/// zero is a heading: 1 to 6 in the first, any in the second.
pub(super) fn is_standard(name: &Name, iso_32000_2: bool) -> bool {
    let bytes = name.as_bytes();
    if let [b'H', digits @ ..] = bytes {
        if !digits.is_empty() && digits.iter().all(u8::is_ascii_digit) && digits[0] != b'0' {
            return iso_32000_2 || (digits.len() == 1 && digits[0] <= b'6');
        }
    }
    let listed = |set: &[&str]| set.iter().any(|standard| standard.as_bytes() == bytes);
    listed(ISO_32000_1) || (iso_32000_2 && listed(ISO_32000_2_ADDED))
}

/// Follow `role_map` from `start` to a standard type.
///
/// A standard type is taken as it stands even when the map also lists it:
/// PDF/UA-1 7.1 forbids remapping a standard type and a consumer reads such a
/// file by its stated type, which also makes an identity mapping terminate.
pub(super) fn resolve(
    role_map: &BTreeMap<Name, Name>,
    start: &Name,
    iso_32000_2: bool,
) -> Option<Name> {
    let mut current = start;
    for _ in 0..=MAX_CHAIN {
        if is_standard(current, iso_32000_2) {
            return Some(current.clone());
        }
        current = role_map.get(current)?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: &[(&str, &str)]) -> BTreeMap<Name, Name> {
        pairs
            .iter()
            .map(|(from, to)| (Name::new(from), Name::new(to)))
            .collect()
    }

    #[test]
    fn a_standard_type_resolves_to_itself() {
        assert_eq!(
            resolve(&BTreeMap::new(), &Name::new("Figure"), false),
            Some(Name::new("Figure"))
        );
    }

    #[test]
    fn a_chain_of_custom_types_ends_on_the_standard_one() {
        let roles = map(&[("Chapter", "Sect"), ("Section", "Chapter"), ("Sect", "Div")]);
        assert_eq!(
            resolve(&roles, &Name::new("Section"), false),
            Some(Name::new("Sect")),
            "the chain stops at the first standard type, not at the end of the map"
        );
    }

    #[test]
    fn a_cycle_resolves_to_nothing() {
        let roles = map(&[("A", "B"), ("B", "A")]);
        assert_eq!(resolve(&roles, &Name::new("A"), false), None);
    }

    #[test]
    fn a_chain_past_the_cap_resolves_to_nothing() {
        let names: Vec<String> = (0..=MAX_CHAIN + 1).map(|n| format!("T{n}")).collect();
        let mut roles: BTreeMap<Name, Name> = names
            .windows(2)
            .map(|pair| (Name::new(&pair[0]), Name::new(&pair[1])))
            .collect();
        roles.insert(Name::new(&names[MAX_CHAIN + 1]), Name::new("P"));
        assert_eq!(resolve(&roles, &Name::new("T0"), false), None);
        assert_eq!(
            resolve(&roles, &Name::new("T2"), false),
            Some(Name::new("P")),
            "a chain inside the cap still resolves"
        );
    }

    #[test]
    fn an_unmapped_custom_type_resolves_to_nothing() {
        assert_eq!(resolve(&map(&[("A", "B")]), &Name::new("A"), false), None);
        assert_eq!(resolve(&BTreeMap::new(), &Name::new("Weird"), true), None);
    }

    #[test]
    fn headings_are_h1_to_h6_in_part_one_and_any_level_in_part_two() {
        for heading in ["H", "H1", "H6"] {
            assert!(is_standard(&Name::new(heading), false), "{heading}");
        }
        for heading in ["H7", "H12"] {
            assert!(!is_standard(&Name::new(heading), false), "{heading}");
            assert!(is_standard(&Name::new(heading), true), "{heading}");
        }
        for not in ["H0", "H01", "Hx", "H1a", "h1"] {
            assert!(!is_standard(&Name::new(not), true), "{not}");
        }
    }

    #[test]
    fn the_types_added_in_part_two_are_custom_outside_its_namespace() {
        assert!(ISO_32000_2_ADDED.contains(&"Artifact"));
        for added in ISO_32000_2_ADDED {
            let name = Name::new(added);
            assert!(!is_standard(&name, false), "{added}");
            assert!(is_standard(&name, true), "{added}");
        }
        let roles = map(&[("Title", "P")]);
        assert_eq!(
            resolve(&roles, &Name::new("Title"), false),
            Some(Name::new("P")),
            "an element outside the 2.0 namespace may define /Title through its role map"
        );
        assert_eq!(
            resolve(&roles, &Name::new("Title"), true),
            Some(Name::new("Title"))
        );
    }
}
