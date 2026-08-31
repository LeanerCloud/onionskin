//! The preferences Onionskin has a feature behind, and their file.
//!
//! Acrobat's dialog carries roughly thirty categories. This is four. The
//! parity row calls the dialog `partial` for exactly this reason: a category
//! ships with the feature it configures, so Commenting arrives with the
//! comment tools, JavaScript with `scripting`, Signatures with signing. What
//! is here is what M2 can honestly change:
//!
//! - **General**: the display theme, which the View menu also sets.
//! - **Documents**: how many documents the recents list keeps.
//! - **Page Display**: the layout and zoom a document opens at.
//! - **Search**: the options the find bar starts with.
//!
//! The file is `~/.config/onionskin/preferences.json`. Like the keymap, a
//! file that will not parse is reported and replaced by the defaults rather
//! than stopping the app, and a value outside the set its setting allows is
//! named rather than rounded to something plausible.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use onionskin_core::{FitMode, MatchMode, PageLayoutMode, SearchOptions};
use serde::{Deserialize, Serialize};

/// How the shell picks light or dark. `System` follows the window's
/// appearance, which is what makes it the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    pub fn label(self) -> &'static str {
        match self {
            Self::System => "System Theme",
            Self::Light => "Light Theme",
            Self::Dark => "Dark Theme",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}

/// The zoom a document opens at: Acrobat's own Page Display default, which
/// is either a fit or actual size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ZoomPreference {
    ActualSize,
    #[default]
    FitPage,
    FitWidth,
    FitHeight,
}

impl ZoomPreference {
    pub const ALL: [Self; 4] = [
        Self::ActualSize,
        Self::FitPage,
        Self::FitWidth,
        Self::FitHeight,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ActualSize => "Actual Size",
            Self::FitPage => "Fit Page",
            Self::FitWidth => "Fit Width",
            Self::FitHeight => "Fit Height",
        }
    }

    /// The fit this asks the viewport for, or `None` for actual size.
    pub fn fit(self) -> Option<FitMode> {
        match self {
            Self::ActualSize => None,
            Self::FitPage => Some(FitMode::Page),
            Self::FitWidth => Some(FitMode::Width),
            Self::FitHeight => Some(FitMode::Height),
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::ActualSize => "actual-size",
            Self::FitPage => "fit-page",
            Self::FitWidth => "fit-width",
            Self::FitHeight => "fit-height",
        }
    }
}

/// The dialog's category list, in the order Acrobat lists the ones we have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferenceCategory {
    Documents,
    General,
    PageDisplay,
    Search,
}

impl PreferenceCategory {
    pub const ALL: [Self; 4] = [
        Self::Documents,
        Self::General,
        Self::PageDisplay,
        Self::Search,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Documents => "Documents",
            Self::General => "General",
            Self::PageDisplay => "Page Display",
            Self::Search => "Search",
        }
    }
}

/// The largest recents list the Documents category offers. Acrobat's own
/// field tops out in the same range; past this the Home view stops being a
/// list of recent work.
pub const MAX_RECENT_DOCUMENTS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preferences {
    /// General.
    pub theme: ThemePreference,
    /// Documents: "Documents in recently used list".
    pub recent_documents: usize,
    /// Page Display.
    pub layout: PageLayoutMode,
    pub zoom: ZoomPreference,
    /// Search: what the find bar starts with.
    pub search: SearchOptions,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: ThemePreference::default(),
            recent_documents: 10,
            // Acrobat's own default, and the mode the scroll budget is
            // measured in.
            layout: PageLayoutMode::SinglePageContinuous,
            zoom: ZoomPreference::default(),
            search: SearchOptions::default(),
        }
    }
}

#[derive(Debug)]
pub enum PreferencesError {
    Unreadable {
        path: PathBuf,
        source: io::Error,
    },
    Malformed {
        path: PathBuf,
        message: String,
    },
    /// A setting named a value it does not have. The defaults keep that one
    /// setting; everything else in the file still applies.
    UnknownValue {
        path: PathBuf,
        setting: &'static str,
        value: String,
        allowed: &'static str,
    },
    OutOfRange {
        path: PathBuf,
        setting: &'static str,
        value: i64,
        max: usize,
    },
    Unwritable {
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for PreferencesError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, source } => {
                write!(f, "{} could not be read: {source}", path.display())
            }
            Self::Unwritable { path, source } => {
                write!(f, "{} could not be written: {source}", path.display())
            }
            Self::Malformed { path, message } => {
                write!(f, "{} is not a preferences file: {message}", path.display())
            }
            Self::UnknownValue {
                path,
                setting,
                value,
                allowed,
            } => write!(
                f,
                "{} sets {setting} to \"{value}\"; it takes one of {allowed}",
                path.display()
            ),
            Self::OutOfRange {
                path,
                setting,
                value,
                max,
            } => write!(
                f,
                "{} sets {setting} to {value}; it takes 0 to {max}",
                path.display()
            ),
        }
    }
}

impl std::error::Error for PreferencesError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { source, .. } | Self::Unwritable { source, .. } => Some(source),
            Self::Malformed { .. } | Self::UnknownValue { .. } | Self::OutOfRange { .. } => None,
        }
    }
}

/// The file's shape, separate from [`Preferences`] so the in-memory type
/// stays typed and every value that is not one of the ones a setting takes
/// is reported rather than deserialized into something else.
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PreferencesFile {
    theme: Option<String>,
    recent_documents: Option<i64>,
    page_layout: Option<String>,
    zoom: Option<String>,
    search_case_sensitive: Option<bool>,
    search_whole_word: Option<bool>,
    search_mode: Option<String>,
}

const THEMES: &str = "\"system\", \"light\", \"dark\"";
const LAYOUTS: &str =
    "\"single-page\", \"single-page-continuous\", \"two-page\", \"two-page-continuous\"";
const ZOOMS: &str = "\"actual-size\", \"fit-page\", \"fit-width\", \"fit-height\"";
const MODES: &str = "\"phrase\", \"any-word\", \"all-words\"";

impl Preferences {
    /// The defaults with `~/.config/onionskin/preferences.json` applied when
    /// it exists.
    pub fn load(path: Option<&Path>) -> (Self, Vec<PreferencesError>) {
        let Some(path) = path else {
            return (Self::default(), Vec::new());
        };
        match crate::config::read(path) {
            Ok(None) => (Self::default(), Vec::new()),
            Ok(Some(source)) => Self::parse(&source, path),
            Err(source) => (
                Self::default(),
                vec![PreferencesError::Unreadable {
                    path: path.to_path_buf(),
                    source,
                }],
            ),
        }
    }

    pub fn parse(source: &str, path: &Path) -> (Self, Vec<PreferencesError>) {
        let file: PreferencesFile = match serde_json::from_str(source) {
            Ok(file) => file,
            Err(error) => {
                return (
                    Self::default(),
                    vec![PreferencesError::Malformed {
                        path: path.to_path_buf(),
                        message: error.to_string(),
                    }],
                )
            }
        };
        let mut preferences = Self::default();
        let mut errors = Vec::new();

        if let Some(theme) = choose(
            &mut errors,
            path,
            "theme",
            file.theme.as_deref(),
            THEMES,
            parse_theme,
        ) {
            preferences.theme = theme;
        }
        if let Some(layout) = choose(
            &mut errors,
            path,
            "page_layout",
            file.page_layout.as_deref(),
            LAYOUTS,
            parse_layout,
        ) {
            preferences.layout = layout;
        }
        if let Some(zoom) = choose(
            &mut errors,
            path,
            "zoom",
            file.zoom.as_deref(),
            ZOOMS,
            parse_zoom,
        ) {
            preferences.zoom = zoom;
        }
        if let Some(mode) = choose(
            &mut errors,
            path,
            "search_mode",
            file.search_mode.as_deref(),
            MODES,
            parse_mode,
        ) {
            preferences.search.mode = mode;
        }
        if let Some(count) = file.recent_documents {
            match usize::try_from(count)
                .ok()
                .filter(|count| *count <= MAX_RECENT_DOCUMENTS)
            {
                Some(count) => preferences.recent_documents = count,
                None => errors.push(PreferencesError::OutOfRange {
                    path: path.to_path_buf(),
                    setting: "recent_documents",
                    value: count,
                    max: MAX_RECENT_DOCUMENTS,
                }),
            }
        }
        if let Some(case_sensitive) = file.search_case_sensitive {
            preferences.search.case_sensitive = case_sensitive;
        }
        if let Some(whole_word) = file.search_whole_word {
            preferences.search.whole_word = whole_word;
        }
        (preferences, errors)
    }

    pub fn save(&self, path: &Path) -> Result<(), PreferencesError> {
        let file = PreferencesFile {
            theme: Some(self.theme.key().to_owned()),
            recent_documents: Some(self.recent_documents as i64),
            page_layout: Some(layout_key(self.layout).to_owned()),
            zoom: Some(self.zoom.key().to_owned()),
            search_case_sensitive: Some(self.search.case_sensitive),
            search_whole_word: Some(self.search.whole_word),
            search_mode: Some(mode_key(self.search.mode).to_owned()),
        };
        let json = serde_json::to_string_pretty(&file)
            .expect("a preferences file of strings, bools and one number serializes");
        crate::config::write_private(path, &json).map_err(|source| PreferencesError::Unwritable {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// The value a setting names, or `None` with the reason recorded. Generic
/// over the setting's type so every one of them reports the same way.
fn choose<T>(
    errors: &mut Vec<PreferencesError>,
    path: &Path,
    setting: &'static str,
    value: Option<&str>,
    allowed: &'static str,
    parse: fn(&str) -> Option<T>,
) -> Option<T> {
    let value = value?;
    let parsed = parse(value);
    if parsed.is_none() {
        errors.push(PreferencesError::UnknownValue {
            path: path.to_path_buf(),
            setting,
            value: value.to_owned(),
            allowed,
        });
    }
    parsed
}

fn parse_theme(value: &str) -> Option<ThemePreference> {
    ThemePreference::ALL
        .into_iter()
        .find(|theme| theme.key() == value)
}

fn parse_zoom(value: &str) -> Option<ZoomPreference> {
    ZoomPreference::ALL
        .into_iter()
        .find(|zoom| zoom.key() == value)
}

fn parse_layout(value: &str) -> Option<PageLayoutMode> {
    LAYOUT_MODES
        .into_iter()
        .find(|(key, _)| *key == value)
        .map(|(_, mode)| mode)
}

fn parse_mode(value: &str) -> Option<MatchMode> {
    MATCH_MODES
        .into_iter()
        .find(|(key, _)| *key == value)
        .map(|(_, mode)| mode)
}

fn layout_key(mode: PageLayoutMode) -> &'static str {
    LAYOUT_MODES
        .into_iter()
        .find(|(_, known)| *known == mode)
        .map(|(key, _)| key)
        .expect("every layout mode has a key")
}

fn mode_key(mode: MatchMode) -> &'static str {
    MATCH_MODES
        .into_iter()
        .find(|(_, known)| *known == mode)
        .map(|(key, _)| key)
        .expect("every match mode has a key")
}

/// The file's spelling of `core`'s layout modes. One table for both
/// directions, so a mode cannot be readable and unwritable.
const LAYOUT_MODES: [(&str, PageLayoutMode); 4] = [
    ("single-page", PageLayoutMode::SinglePage),
    (
        "single-page-continuous",
        PageLayoutMode::SinglePageContinuous,
    ),
    ("two-page", PageLayoutMode::TwoPage),
    ("two-page-continuous", PageLayoutMode::TwoPageContinuous),
];

const MATCH_MODES: [(&str, MatchMode); 3] = [
    ("phrase", MatchMode::Phrase),
    ("any-word", MatchMode::AnyWord),
    ("all-words", MatchMode::AllWords),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> (Preferences, Vec<String>) {
        let (preferences, errors) = Preferences::parse(source, Path::new("/tmp/preferences.json"));
        (
            preferences,
            errors.iter().map(ToString::to_string).collect(),
        )
    }

    #[test]
    fn an_empty_file_is_the_defaults() {
        let (preferences, errors) = parse("{}");

        assert!(errors.is_empty());
        assert_eq!(preferences, Preferences::default());
        assert_eq!(preferences.zoom.fit(), Some(FitMode::Page));
    }

    #[test]
    fn every_setting_round_trips_through_the_file() {
        let path = crate::config::test_dir("preferences-round-trip").join("preferences.json");
        let written = Preferences {
            theme: ThemePreference::Dark,
            recent_documents: 3,
            layout: PageLayoutMode::TwoPage,
            zoom: ZoomPreference::ActualSize,
            search: SearchOptions {
                case_sensitive: true,
                whole_word: true,
                mode: MatchMode::AllWords,
            },
        };

        written.save(&path).expect("preferences save");
        let (read, errors) = Preferences::load(Some(&path));

        assert!(errors.is_empty());
        assert_eq!(read, written);
    }

    /// Every value a setting names is reported by name, and the rest of the
    /// file still applies: one typo does not cost the user their whole file.
    #[test]
    fn a_value_a_setting_does_not_have_is_named_and_the_rest_survives() {
        let (preferences, errors) = parse(r#"{"theme": "sepia", "page_layout": "two-page"}"#);

        assert_eq!(
            errors,
            vec![
                "/tmp/preferences.json sets theme to \"sepia\"; it takes one of \
                 \"system\", \"light\", \"dark\""
                    .to_owned()
            ]
        );
        assert_eq!(preferences.theme, ThemePreference::System);
        assert_eq!(preferences.layout, PageLayoutMode::TwoPage);
    }

    #[test]
    fn a_recents_length_outside_the_range_is_reported_rather_than_clamped() {
        let (preferences, errors) = parse(r#"{"recent_documents": 900}"#);

        assert_eq!(
            errors,
            vec!["/tmp/preferences.json sets recent_documents to 900; it takes 0 to 50".to_owned()]
        );
        assert_eq!(
            preferences.recent_documents,
            Preferences::default().recent_documents
        );

        let (negative, errors) = parse(r#"{"recent_documents": -1}"#);
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(
            negative.recent_documents,
            Preferences::default().recent_documents
        );
    }

    /// Turning the list off is a setting, not an error: Acrobat's field
    /// accepts zero and it means "keep no recents".
    #[test]
    fn zero_recent_documents_is_allowed() {
        let (preferences, errors) = parse(r#"{"recent_documents": 0}"#);

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(preferences.recent_documents, 0);
    }

    #[test]
    fn a_malformed_file_is_reported_and_every_default_survives() {
        let (preferences, errors) = parse("{\"theme\":");

        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].starts_with("/tmp/preferences.json is not a preferences file"),
            "{errors:?}"
        );
        assert_eq!(preferences, Preferences::default());
    }

    /// A key nobody reads is a setting the user believes is in force. Say so
    /// rather than accepting the file silently.
    #[test]
    fn a_setting_this_build_does_not_have_is_reported() {
        let (preferences, errors) = parse(r#"{"commenting_author": "me"}"#);

        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("commenting_author"), "{errors:?}");
        assert_eq!(preferences, Preferences::default());
    }

    /// The dialog carries only the categories with a setting behind them.
    /// The parity row is `partial` for that reason, and a category that
    /// arrives with its feature adds a row here at the same time.
    #[test]
    fn the_dialog_lists_the_four_categories_this_milestone_can_change() {
        assert_eq!(
            PreferenceCategory::ALL.map(PreferenceCategory::label),
            ["Documents", "General", "Page Display", "Search"]
        );
    }
}
