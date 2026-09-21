//! The Document Properties dialog as data: its tabs, its choices, and what
//! the fields it holds mean. Tested without a window.

use onionskin_core::metadata::{
    Description, FontEntry, InitialView, OpenFit, PageLayout, PageMode,
};

/// The five tabs, in the order Acrobat's unified UI shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) enum PropertiesTab {
    #[default]
    Description,
    Security,
    Fonts,
    InitialView,
    Custom,
}

impl PropertiesTab {
    pub(in crate::shell) const ALL: [Self; 5] = [
        Self::Description,
        Self::Security,
        Self::Fonts,
        Self::InitialView,
        Self::Custom,
    ];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Description => "Description",
            Self::Security => "Security",
            Self::Fonts => "Fonts",
            Self::InitialView => "Initial View",
            Self::Custom => "Custom",
        }
    }
}

/// How the opening page is fitted, as the Initial View tab offers it. A zoom
/// is whole percent, which is what a user types and what keeps the choice
/// comparable; `OpenFit` holds the file's own number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum FitChoice {
    Default,
    Page,
    Width,
    Height,
    Visible,
    Zoom(u32),
}

/// The magnifications offered, before the one the document has.
const OFFERED_FITS: [FitChoice; 8] = [
    FitChoice::Default,
    FitChoice::Page,
    FitChoice::Width,
    FitChoice::Height,
    FitChoice::Visible,
    FitChoice::Zoom(50),
    FitChoice::Zoom(100),
    FitChoice::Zoom(200),
];

impl FitChoice {
    pub(in crate::shell) fn label(self) -> String {
        match self {
            Self::Default => "Default".to_owned(),
            Self::Page => "Fit Page".to_owned(),
            Self::Width => "Fit Width".to_owned(),
            Self::Height => "Fit Height".to_owned(),
            Self::Visible => "Fit Visible".to_owned(),
            Self::Zoom(percent) => format!("{percent}%"),
        }
    }

    pub(in crate::shell) fn of(fit: OpenFit) -> Self {
        match fit {
            OpenFit::Default => Self::Default,
            OpenFit::Page => Self::Page,
            OpenFit::Width => Self::Width,
            OpenFit::Height => Self::Height,
            OpenFit::Visible => Self::Visible,
            OpenFit::Zoom(zoom) => Self::Zoom((zoom * 100.0).round().max(1.0) as u32),
        }
    }

    pub(in crate::shell) fn fit(self) -> OpenFit {
        match self {
            Self::Default => OpenFit::Default,
            Self::Page => OpenFit::Page,
            Self::Width => OpenFit::Width,
            Self::Height => OpenFit::Height,
            Self::Visible => OpenFit::Visible,
            Self::Zoom(percent) => OpenFit::Zoom(f64::from(percent) / 100.0),
        }
    }
}

/// The magnifications the tab lists: the offered ones, and the document's
/// own when it is none of them, so opening the dialog cannot silently
/// change it.
pub(in crate::shell) fn fit_choices(in_force: FitChoice) -> Vec<FitChoice> {
    let mut choices = OFFERED_FITS.to_vec();
    if !choices.contains(&in_force) {
        choices.push(in_force);
    }
    choices
}

pub(in crate::shell) fn layout_label(layout: Option<PageLayout>) -> &'static str {
    match layout {
        None => "Default",
        Some(PageLayout::SinglePage) => "Single Page",
        Some(PageLayout::OneColumn) => "Single Page Continuous",
        Some(PageLayout::TwoColumnLeft) => "Two-Up Continuous",
        Some(PageLayout::TwoColumnRight) => "Two-Up Continuous (Cover Page)",
        Some(PageLayout::TwoPageLeft) => "Two-Up",
        Some(PageLayout::TwoPageRight) => "Two-Up (Cover Page)",
    }
}

pub(in crate::shell) fn mode_label(mode: Option<PageMode>) -> &'static str {
    match mode {
        None => "Default",
        Some(PageMode::UseNone) => "Page Only",
        Some(PageMode::UseOutlines) => "Bookmarks Panel And Page",
        Some(PageMode::UseThumbs) => "Page Thumbnails Panel And Page",
        Some(PageMode::UseAttachments) => "Attachments Panel And Page",
        Some(PageMode::UseOC) => "Layers Panel And Page",
        Some(PageMode::FullScreen) => "Full Screen",
    }
}

pub(in crate::shell) fn layouts() -> impl Iterator<Item = Option<PageLayout>> {
    std::iter::once(None).chain(PageLayout::ALL.map(Some))
}

pub(in crate::shell) fn modes() -> impl Iterator<Item = Option<PageMode>> {
    std::iter::once(None).chain(PageMode::ALL.map(Some))
}

/// The four Description fields as typed. A blank field is an absent one.
pub(in crate::shell) fn description(fields: [&str; 4]) -> Description {
    let field = |value: &str| {
        let value = value.trim();
        (!value.is_empty()).then(|| value.to_owned())
    };
    Description {
        title: field(fields[0]),
        author: field(fields[1]),
        subject: field(fields[2]),
        keywords: field(fields[3]),
    }
}

/// The open page as typed, from one: blank is no open action.
pub(in crate::shell) fn open_page(text: &str, page_count: usize) -> Result<Option<usize>, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    match text.parse::<usize>() {
        Ok(page) if (1..=page_count).contains(&page) => Ok(Some(page - 1)),
        _ => Err(format!(
            "Open to page must be a page from 1 to {page_count}, got {text:?}"
        )),
    }
}

/// Add a custom property, or say why not. A standard key is `core`'s to
/// refuse, at Apply, with the rule it writes by.
pub(in crate::shell) fn add_custom(
    custom: &mut Vec<(String, String)>,
    key: &str,
    value: &str,
) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() {
        return Err("A custom property needs a name".to_owned());
    }
    if key.chars().any(char::is_whitespace) {
        return Err(format!("{key:?} has spaces; a property name cannot"));
    }
    if custom.iter().any(|(existing, _)| existing == key) {
        return Err(format!("There is already a property named {key:?}"));
    }
    custom.push((key.to_owned(), value.to_owned()));
    Ok(())
}

/// A PDF date as the Description tab prints it: `D:20260921140500Z` is
/// `2026-09-21 14:05:00 UTC`. A date that is not one is shown as written,
/// so a malformed value is still visible rather than blank.
pub(in crate::shell) fn date_label(date: &str) -> String {
    let digits = date.strip_prefix("D:").unwrap_or(date);
    let field = |range: std::ops::Range<usize>| {
        digits
            .get(range)
            .filter(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
    };
    let Some(year) = field(0..4) else {
        return date.to_owned();
    };
    let part = |range, default| field(range).unwrap_or(default);
    let zone = match digits.get(14..) {
        Some(rest) if rest.starts_with('Z') => " UTC".to_owned(),
        Some(rest) if rest.starts_with('+') || rest.starts_with('-') => {
            format!(" {}", rest.replace('\'', ":").trim_end_matches(':'))
        }
        _ => String::new(),
    };
    format!(
        "{year}-{}-{} {}:{}:{}{zone}",
        part(4..6, "01"),
        part(6..8, "01"),
        part(8..10, "00"),
        part(10..12, "00"),
        part(12..14, "00"),
    )
}

/// The file facts the Description tab shows beside the fields.
pub(in crate::shell) fn size_label(bytes: u64) -> String {
    const KIB: f64 = 1024.0;
    let bytes_f = bytes as f64;
    if bytes_f < KIB {
        format!("{bytes} bytes")
    } else if bytes_f < KIB * KIB {
        format!("{:.1} KB ({bytes} bytes)", bytes_f / KIB)
    } else {
        format!("{:.2} MB ({bytes} bytes)", bytes_f / (KIB * KIB))
    }
}

/// One font as the Fonts tab lists it.
pub(in crate::shell) fn font_label(font: &FontEntry) -> String {
    let embedding = match (font.embedded, font.subset) {
        (true, true) => "Embedded Subset",
        (true, false) => "Embedded",
        (false, _) => "Not embedded",
    };
    format!("{} ({}, {embedding})", font.name, font.kind)
}

/// The view the Initial View tab describes.
pub(in crate::shell) fn initial_view(
    layout: Option<PageLayout>,
    mode: Option<PageMode>,
    page: Option<usize>,
    fit: FitChoice,
) -> InitialView {
    InitialView {
        layout,
        mode,
        page,
        fit: fit.fit(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zoom_the_tab_does_not_offer_is_listed_so_it_stays_selected() {
        let odd = FitChoice::of(OpenFit::Zoom(1.25));
        assert_eq!(odd, FitChoice::Zoom(125));
        assert!(fit_choices(odd).contains(&odd));
        assert_eq!(
            fit_choices(FitChoice::Width).len(),
            OFFERED_FITS.len(),
            "an offered fit is not listed twice"
        );
    }

    #[test]
    fn every_fit_choice_round_trips_through_the_file_form() {
        for choice in fit_choices(FitChoice::Zoom(125)) {
            assert_eq!(FitChoice::of(choice.fit()), choice, "{}", choice.label());
        }
    }

    #[test]
    fn the_open_page_is_typed_from_one_and_blank_is_none() {
        assert_eq!(open_page(" 5 ", 9), Ok(Some(4)));
        assert_eq!(open_page("", 9), Ok(None));
        assert!(open_page("0", 9).is_err());
        assert!(open_page("10", 9).is_err());
        assert!(open_page("two", 9).is_err());
    }

    #[test]
    fn a_blank_description_field_is_absent() {
        let read = description(["Report", "  ", "", "a, b"]);
        assert_eq!(read.title.as_deref(), Some("Report"));
        assert_eq!(read.author, None);
        assert_eq!(read.subject, None);
        assert_eq!(read.keywords.as_deref(), Some("a, b"));
    }

    #[test]
    fn a_custom_property_needs_a_new_name_without_spaces() {
        let mut custom = vec![("Department".to_owned(), "Finance".to_owned())];
        assert!(add_custom(&mut custom, "", "x").is_err());
        assert!(add_custom(&mut custom, "Two words", "x").is_err());
        assert!(add_custom(&mut custom, "Department", "x").is_err());
        assert_eq!(add_custom(&mut custom, " Owner ", "Ana"), Ok(()));
        assert_eq!(custom[1], ("Owner".to_owned(), "Ana".to_owned()));
    }

    #[test]
    fn every_layout_and_mode_has_its_own_label() {
        let layouts: std::collections::HashSet<_> = layouts().map(layout_label).collect();
        assert_eq!(layouts.len(), PageLayout::ALL.len() + 1);
        let modes: std::collections::HashSet<_> = modes().map(mode_label).collect();
        assert_eq!(modes.len(), PageMode::ALL.len() + 1);
    }

    #[test]
    fn dates_read_as_a_person_writes_them() {
        assert_eq!(
            date_label("D:20260921140500Z00'00'"),
            "2026-09-21 14:05:00 UTC"
        );
        assert_eq!(
            date_label("D:20260921140500+02'00'"),
            "2026-09-21 14:05:00 +02:00"
        );
        assert_eq!(date_label("D:2026"), "2026-01-01 00:00:00");
        assert_eq!(date_label("yesterday"), "yesterday");
    }

    #[test]
    fn sizes_and_fonts_read_as_the_tab_prints_them() {
        assert_eq!(size_label(512), "512 bytes");
        assert_eq!(size_label(2048), "2.0 KB (2048 bytes)");
        assert_eq!(size_label(3 * 1024 * 1024), "3.00 MB (3145728 bytes)");
        let font = FontEntry {
            name: "Garamond".into(),
            kind: "TrueType".into(),
            embedded: true,
            subset: true,
        };
        assert_eq!(font_label(&font), "Garamond (TrueType, Embedded Subset)");
    }
}
