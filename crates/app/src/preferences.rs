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

/// What the interface calls a page layout. Here rather than in the chrome
/// because the View menu and the Preferences dialog both say it, and they
/// have to say the same thing.
pub fn layout_label(mode: PageLayoutMode) -> &'static str {
    match mode {
        PageLayoutMode::SinglePage => "Single Page",
        PageLayoutMode::SinglePageContinuous => "Single Page Continuous",
        PageLayoutMode::TwoPage => "Two Page",
        PageLayoutMode::TwoPageContinuous => "Two Page Continuous",
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
    /// A key no setting in this build answers to. Everything else in the
    /// file still applies.
    UnknownSetting {
        path: PathBuf,
        setting: String,
    },
    /// A setting named a value it does not have. The defaults keep that one
    /// setting; everything else in the file still applies.
    UnknownValue {
        path: PathBuf,
        setting: String,
        /// As the file wrote it, so a message can quote it back.
        value: String,
        allowed: &'static str,
    },
    OutOfRange {
        path: PathBuf,
        setting: String,
        value: String,
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
            Self::UnknownSetting { path, setting } => write!(
                f,
                "{} sets {setting}, which is not a setting this build has",
                path.display()
            ),
            Self::UnknownValue {
                path,
                setting,
                value,
                allowed,
            } => write!(
                f,
                "{} sets {setting} to {value}; it takes one of {allowed}",
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
            Self::Malformed { .. }
            | Self::UnknownSetting { .. }
            | Self::UnknownValue { .. }
            | Self::OutOfRange { .. } => None,
        }
    }
}

const THEMES: &str = "\"system\", \"light\", \"dark\"";
const LAYOUTS: &str =
    "\"single-page\", \"single-page-continuous\", \"two-page\", \"two-page-continuous\"";
const ZOOMS: &str = "\"actual-size\", \"fit-page\", \"fit-width\", \"fit-height\"";
const MODES: &str = "\"phrase\", \"any-word\", \"all-words\"";
const FLAGS: &str = "true, false";

impl Preferences {
    /// The defaults with `~/.config/onionskin/preferences.json` applied when
    /// it exists.
    pub fn load(path: Option<&Path>) -> (Self, Vec<PreferencesError>) {
        let Some(path) = path else {
            return (Self::default(), Vec::new());
        };
        match crate::config::read(path) {
            Ok(None) => (Self::default(), Vec::new()),
            Ok(Some(source)) => {
                let (preferences, mut errors) = Self::parse(&source, path);
                for error in &mut errors {
                    if let PreferencesError::Malformed { message, .. } = error {
                        message.push_str(&crate::config::keep_unreadable(path));
                    }
                }
                (preferences, errors)
            }
            Err(source) => (
                Self::default(),
                vec![PreferencesError::Unreadable {
                    path: path.to_path_buf(),
                    source,
                }],
            ),
        }
    }

    /// The defaults with `source` applied over them.
    ///
    /// Read setting by setting rather than into one struct: a value of the
    /// wrong type, a value outside the set its setting takes, and a key this
    /// build does not have are each one setting's problem. Failing the whole
    /// document over any of them would replace everything the user wrote
    /// with the defaults, and then the next save would write those defaults
    /// over their file.
    fn parse(source: &str, path: &Path) -> (Self, Vec<PreferencesError>) {
        let file: serde_json::Map<String, serde_json::Value> = match serde_json::from_str(source) {
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
        for (setting, value) in &file {
            if let Err(error) = apply(&mut preferences, path, setting, value) {
                errors.push(error);
            }
        }
        (preferences, errors)
    }

    /// Write every setting this build has, keeping the keys it does not.
    ///
    /// A key from another version is reported when the file is read, and
    /// that report is not consent to delete it: the notice may have been
    /// dismissed, and the file may be shared with the build that wrote it.
    /// Read back here rather than carried in memory, so a file edited by
    /// hand between the two keeps whatever was added.
    pub fn save(&self, path: &Path) -> Result<(), PreferencesError> {
        let mut file = serde_json::Map::new();
        file.insert("theme".into(), self.theme.key().into());
        file.insert("recent_documents".into(), self.recent_documents.into());
        file.insert("page_layout".into(), layout_key(self.layout).into());
        file.insert("zoom".into(), self.zoom.key().into());
        file.insert(
            "search_case_sensitive".into(),
            self.search.case_sensitive.into(),
        );
        file.insert("search_whole_word".into(), self.search.whole_word.into());
        file.insert("search_mode".into(), mode_key(self.search.mode).into());
        // Whatever is already in the file and is not one of the settings
        // above. `or_insert` keeps this build's value for the keys it wrote.
        if let Ok(Some(source)) = crate::config::read(path) {
            if let Ok(existing) =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&source)
            {
                for (setting, value) in existing {
                    file.entry(setting).or_insert(value);
                }
            }
        }
        let json = serde_json::to_string_pretty(&file)
            .expect("a map of strings, bools and one number serializes");
        crate::config::write_private(path, &json).map_err(|source| PreferencesError::Unwritable {
            path: path.to_path_buf(),
            source,
        })
    }
}

/// Apply one setting from the file.
///
/// Every arm names the values it takes, so the message a user gets says what
/// to write instead rather than that something was wrong.
fn apply(
    preferences: &mut Preferences,
    path: &Path,
    setting: &str,
    value: &serde_json::Value,
) -> Result<(), PreferencesError> {
    match setting {
        "theme" => preferences.theme = named(path, setting, value, THEMES, parse_theme)?,
        "page_layout" => preferences.layout = named(path, setting, value, LAYOUTS, parse_layout)?,
        "zoom" => preferences.zoom = named(path, setting, value, ZOOMS, parse_zoom)?,
        "search_mode" => preferences.search.mode = named(path, setting, value, MODES, parse_mode)?,
        "search_case_sensitive" => preferences.search.case_sensitive = flag(path, setting, value)?,
        "search_whole_word" => preferences.search.whole_word = flag(path, setting, value)?,
        "recent_documents" => preferences.recent_documents = count(path, setting, value)?,
        _ => {
            return Err(PreferencesError::UnknownSetting {
                path: path.to_path_buf(),
                setting: setting.to_owned(),
            })
        }
    }
    Ok(())
}

fn named<T>(
    path: &Path,
    setting: &str,
    value: &serde_json::Value,
    allowed: &'static str,
    parse: fn(&str) -> Option<T>,
) -> Result<T, PreferencesError> {
    value
        .as_str()
        .and_then(parse)
        .ok_or_else(|| unknown_value(path, setting, value, allowed))
}

fn flag(path: &Path, setting: &str, value: &serde_json::Value) -> Result<bool, PreferencesError> {
    value
        .as_bool()
        .ok_or_else(|| unknown_value(path, setting, value, FLAGS))
}

fn count(path: &Path, setting: &str, value: &serde_json::Value) -> Result<usize, PreferencesError> {
    value
        .as_u64()
        .and_then(|count| usize::try_from(count).ok())
        .filter(|count| *count <= MAX_RECENT_DOCUMENTS)
        .ok_or_else(|| PreferencesError::OutOfRange {
            path: path.to_path_buf(),
            setting: setting.to_owned(),
            value: value.to_string(),
            max: MAX_RECENT_DOCUMENTS,
        })
}

fn unknown_value(
    path: &Path,
    setting: &str,
    value: &serde_json::Value,
    allowed: &'static str,
) -> PreferencesError {
    PreferencesError::UnknownValue {
        path: path.to_path_buf(),
        setting: setting.to_owned(),
        value: value.to_string(),
        allowed,
    }
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
    Some(match value {
        "single-page" => PageLayoutMode::SinglePage,
        "single-page-continuous" => PageLayoutMode::SinglePageContinuous,
        "two-page" => PageLayoutMode::TwoPage,
        "two-page-continuous" => PageLayoutMode::TwoPageContinuous,
        _ => return None,
    })
}

fn parse_mode(value: &str) -> Option<MatchMode> {
    Some(match value {
        "phrase" => MatchMode::Phrase,
        "any-word" => MatchMode::AnyWord,
        "all-words" => MatchMode::AllWords,
        _ => return None,
    })
}

/// Written as a match rather than a lookup in the table below, so a new
/// variant in `core` fails the build here instead of panicking on the first
/// save that meets it.
fn layout_key(mode: PageLayoutMode) -> &'static str {
    match mode {
        PageLayoutMode::SinglePage => "single-page",
        PageLayoutMode::SinglePageContinuous => "single-page-continuous",
        PageLayoutMode::TwoPage => "two-page",
        PageLayoutMode::TwoPageContinuous => "two-page-continuous",
    }
}

fn mode_key(mode: MatchMode) -> &'static str {
    match mode {
        MatchMode::Phrase => "phrase",
        MatchMode::AnyWord => "any-word",
        MatchMode::AllWords => "all-words",
    }
}

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
    /// A value of the wrong type is one setting's problem too: the file's
    /// other settings still apply and the file is not replaced.
    #[test]
    fn a_value_of_the_wrong_type_costs_only_its_own_setting() {
        let (preferences, errors) = parse(
            r#"{"theme": 7, "search_whole_word": "yes", "recent_documents": "ten", "zoom": "fit-width"}"#,
        );

        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(
            errors.iter().any(|error| error.contains("sets theme to 7")),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains(r#"sets search_whole_word to "yes""#)),
            "{errors:?}"
        );
        assert!(
            errors
                .iter()
                .any(|error| error.contains(r#"sets recent_documents to "ten""#)),
            "{errors:?}"
        );
        assert_eq!(preferences.zoom, ZoomPreference::FitWidth);
        assert_eq!(preferences.theme, ThemePreference::System);
        assert!(!preferences.search.whole_word);
    }

    #[test]
    fn zero_recent_documents_is_allowed() {
        let (preferences, errors) = parse(r#"{"recent_documents": 0}"#);

        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(preferences.recent_documents, 0);
    }

    /// The read table and the write match have to agree, or a preference
    /// saves as something the next start cannot read back.
    #[test]
    fn every_key_written_is_a_key_that_reads_back() {
        for layout in [
            PageLayoutMode::SinglePage,
            PageLayoutMode::SinglePageContinuous,
            PageLayoutMode::TwoPage,
            PageLayoutMode::TwoPageContinuous,
        ] {
            assert_eq!(parse_layout(layout_key(layout)), Some(layout));
        }
        for mode in [MatchMode::Phrase, MatchMode::AnyWord, MatchMode::AllWords] {
            assert_eq!(parse_mode(mode_key(mode)), Some(mode));
        }
        for theme in ThemePreference::ALL {
            assert_eq!(parse_theme(theme.key()), Some(theme));
        }
        for zoom in ZoomPreference::ALL {
            assert_eq!(parse_zoom(zoom.key()), Some(zoom));
        }
    }

    /// A setting from another version survives a save. Reporting it is not
    /// permission to delete it, and the first click in the dialog saves.
    #[test]
    fn a_setting_this_build_does_not_have_survives_a_save() {
        let path = crate::config::test_dir("preferences-unknown").join("preferences.json");
        std::fs::write(&path, r#"{"commenting_author": "me", "theme": "dark"}"#)
            .expect("the test writes its file");
        let (preferences, errors) = Preferences::load(Some(&path));
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert_eq!(preferences.theme, ThemePreference::Dark);

        Preferences {
            theme: ThemePreference::Light,
            ..preferences
        }
        .save(&path)
        .expect("preferences save");

        let written = std::fs::read_to_string(&path).expect("the file reads back");
        assert!(
            written.contains("commenting_author"),
            "the save dropped a setting this build does not have: {written}"
        );
        assert!(written.contains("\"light\""), "{written}");
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

    /// A key nobody reads is a setting the user believes is in force, so it
    /// is reported. It is also one key: a preference file written by a
    /// version that has one more category must not cost the user the
    /// settings this build does understand.
    #[test]
    fn a_setting_this_build_does_not_have_is_reported_and_costs_nothing_else() {
        let (preferences, errors) =
            parse(r#"{"commenting_author": "me", "theme": "dark", "recent_documents": 3}"#);

        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("commenting_author"), "{errors:?}");
        assert!(
            errors[0].contains("not a setting this build has"),
            "{errors:?}"
        );
        assert_eq!(preferences.theme, ThemePreference::Dark);
        assert_eq!(preferences.recent_documents, 3);
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
