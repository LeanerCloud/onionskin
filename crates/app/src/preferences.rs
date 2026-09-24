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

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

use onionskin_core::{FitMode, MatchMode, PageLayoutMode, SearchOptions};
pub use onionskin_plugin_api::CommentDefault;

mod redaction;

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
    Commenting,
    Documents,
    Forms,
    General,
    JavaScript,
    PageDisplay,
    Search,
    TrustManager,
}

impl PreferenceCategory {
    /// Acrobat's order, which is alphabetical.
    pub const ALL: [Self; 8] = [
        Self::Commenting,
        Self::Documents,
        Self::Forms,
        Self::General,
        Self::JavaScript,
        Self::PageDisplay,
        Self::Search,
        Self::TrustManager,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Commenting => "Commenting",
            Self::Documents => "Documents",
            Self::Forms => "Forms",
            Self::General => "General",
            Self::JavaScript => "JavaScript",
            Self::PageDisplay => "Page Display",
            Self::Search => "Search",
            Self::TrustManager => "Trust Manager",
        }
    }
}

/// Trust Manager: what following a link to a web page does. A site the user
/// chose to always allow opens under Ask as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WebLinks {
    /// Ask each time, offering to always allow the site.
    #[default]
    Ask,
    /// Open every web link without asking.
    Allow,
    /// Open none.
    Block,
}

impl WebLinks {
    pub const ALL: [Self; 3] = [Self::Ask, Self::Allow, Self::Block];

    pub fn key(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Allow => "allow",
            Self::Block => "block",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "Ask",
            Self::Allow => "Always allow",
            Self::Block => "Never",
        }
    }
}

fn parse_web_links(value: &str) -> Option<WebLinks> {
    WebLinks::ALL.into_iter().find(|links| links.key() == value)
}

/// The largest recents list the Documents category offers. Acrobat's own
/// field tops out in the same range; past this the Home view stops being a
/// list of recent work.
pub const MAX_RECENT_DOCUMENTS: usize = 50;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preferences {
    /// General.
    pub theme: ThemePreference,
    /// Documents: "Documents in recently used list".
    pub recent_documents: usize,
    /// Page Display.
    pub layout: PageLayoutMode,
    pub zoom: ZoomPreference,
    /// Page Display: "Use line weights". Off draws every stroke one pixel
    /// wide on screen; printing and export keep the page's own widths. The
    /// View menu's Line Weights sets the same thing.
    pub line_weights: bool,
    /// Search: what the find bar starts with.
    pub search: SearchOptions,
    /// Commenting: the name comments are signed with. `None` until the user
    /// chooses one; the operating system's account name is never used.
    pub commenting_author: Option<String>,
    /// Commenting: each kind of comment's default look, set with "Make
    /// Current Properties Default", keyed by `/Subtype`.
    pub comment_defaults: BTreeMap<String, CommentDefault>,
    /// Manage Tools: the tools the user took out of the rail, by id. A
    /// hidden tool still runs from its menu entry, its shortcut and Tool
    /// Search; only its rail button goes.
    pub hidden_tools: BTreeSet<String>,
    /// Trust Manager: whether a web link opens.
    pub web_links: WebLinks,
    /// Trust Manager: the sites whose links open without asking, by host.
    pub trusted_sites: BTreeSet<String>,
    /// Redaction Properties: the look new marks take. `None` for the
    /// redaction tool's own.
    pub redaction: Option<onionskin_plugin_api::RedactionDefault>,
    /// JavaScript: "Enable Acrobat JavaScript", which here is a form's
    /// calculation, validation and format scripts.
    pub javascript: bool,
    /// Forms: Auto-Complete, Basic when on: what was typed into text
    /// fields is offered again.
    pub autocomplete: bool,
    /// Forms: "Remember numerical data". Off by default, so a number
    /// typed into a form, which may be an account's, is not kept.
    pub autocomplete_numbers: bool,
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
            line_weights: true,
            search: SearchOptions::default(),
            commenting_author: None,
            comment_defaults: BTreeMap::new(),
            hidden_tools: BTreeSet::new(),
            web_links: WebLinks::default(),
            trusted_sites: BTreeSet::new(),
            redaction: None,
            javascript: true,
            autocomplete: true,
            autocomplete_numbers: false,
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
const WEB_LINKS: &str = "\"ask\", \"allow\", \"block\"";
const SITES: &str = "a list of host names in quotes";

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
        file.insert("line_weights".into(), self.line_weights.into());
        file.insert("javascript".into(), self.javascript.into());
        file.insert("autocomplete".into(), self.autocomplete.into());
        file.insert(
            "autocomplete_numbers".into(),
            self.autocomplete_numbers.into(),
        );
        file.insert(
            "search_case_sensitive".into(),
            self.search.case_sensitive.into(),
        );
        file.insert("search_whole_word".into(), self.search.whole_word.into());
        file.insert("search_mode".into(), mode_key(self.search.mode).into());
        if let Some(author) = &self.commenting_author {
            file.insert("commenting_author".into(), author.clone().into());
        }
        if !self.comment_defaults.is_empty() {
            file.insert(
                "comment_defaults".into(),
                defaults_json(&self.comment_defaults),
            );
        }
        if !self.hidden_tools.is_empty() {
            file.insert(
                "hidden_tools".into(),
                self.hidden_tools.iter().cloned().collect(),
            );
        }
        file.insert("web_links".into(), self.web_links.key().into());
        if !self.trusted_sites.is_empty() {
            file.insert(
                "trusted_sites".into(),
                self.trusted_sites.iter().cloned().collect(),
            );
        }
        if let Some(redaction) = &self.redaction {
            file.insert("redaction".into(), redaction::json(redaction));
        }
        carry_forward(path, &mut file);
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
        "line_weights" => preferences.line_weights = flag(path, setting, value)?,
        "javascript" => preferences.javascript = flag(path, setting, value)?,
        "autocomplete" => preferences.autocomplete = flag(path, setting, value)?,
        "autocomplete_numbers" => {
            preferences.autocomplete_numbers = flag(path, setting, value)?;
        }
        "recent_documents" => preferences.recent_documents = count(path, setting, value)?,
        "commenting_author" => preferences.commenting_author = author(path, setting, value)?,
        "redaction" => {
            preferences.redaction = Some(
                redaction::parse(value)
                    .ok_or_else(|| unknown_value(path, setting, value, redaction::REDACTION))?,
            );
        }
        "comment_defaults" => {
            preferences.comment_defaults = comment_defaults(value)
                .ok_or_else(|| unknown_value(path, setting, value, DEFAULTS))?;
        }
        "hidden_tools" => {
            preferences.hidden_tools =
                tool_ids(value).ok_or_else(|| unknown_value(path, setting, value, TOOL_IDS))?;
        }
        "web_links" => {
            preferences.web_links = named(path, setting, value, WEB_LINKS, parse_web_links)?;
        }
        "trusted_sites" => {
            preferences.trusted_sites =
                tool_ids(value).ok_or_else(|| unknown_value(path, setting, value, SITES))?;
        }
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

const DEFAULTS: &str =
    "an object of comment kinds, each {\"color\": \"#rrggbb\" or null, \"opacity\": 0 to 100}";

/// `comment_defaults` as the file writes it: `{"Square": {"color":
/// "#ff0000", "opacity": 50}}`. One malformed entry refuses the whole
/// setting, so a half-read table never overwrites what the user wrote.
fn comment_defaults(value: &serde_json::Value) -> Option<BTreeMap<String, CommentDefault>> {
    value
        .as_object()?
        .iter()
        .map(|(kind, entry)| {
            let color = match entry.get("color") {
                None | Some(serde_json::Value::Null) => None,
                Some(color) => Some(parse_hex(color.as_str()?)?),
            };
            let opacity_percent = u8::try_from(entry.get("opacity")?.as_u64()?)
                .ok()
                .filter(|percent| *percent <= 100)?;
            Some((
                kind.clone(),
                CommentDefault {
                    color,
                    opacity_percent,
                },
            ))
        })
        .collect()
}

fn parse_hex(hex: &str) -> Option<[u8; 3]> {
    let digits = hex.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

fn defaults_json(defaults: &BTreeMap<String, CommentDefault>) -> serde_json::Value {
    defaults
        .iter()
        .map(|(kind, default)| {
            let color = default.color.map_or(serde_json::Value::Null, |[r, g, b]| {
                format!("#{r:02x}{g:02x}{b:02x}").into()
            });
            (
                kind.clone(),
                serde_json::json!({ "color": color, "opacity": default.opacity_percent }),
            )
        })
        .collect::<serde_json::Map<_, _>>()
        .into()
}

const TOOL_IDS: &str = "a list of tool ids in quotes";

/// `hidden_tools` as the file writes it: `["tool.highlight"]`. One entry that
/// is not a string refuses the whole list, like `comment_defaults`. An id no
/// tool in this build has is kept: it may be a plugin's that is not
/// installed today.
fn tool_ids(value: &serde_json::Value) -> Option<BTreeSet<String>> {
    value
        .as_array()?
        .iter()
        .map(|id| id.as_str().map(str::to_owned))
        .collect()
}

/// A name to sign comments with. An empty or blank one is no name.
fn author(
    path: &Path,
    setting: &str,
    value: &serde_json::Value,
) -> Result<Option<String>, PreferencesError> {
    value
        .as_str()
        .map(|name| Some(name.trim().to_owned()).filter(|name| !name.is_empty()))
        .ok_or_else(|| unknown_value(path, setting, value, "a name in quotes"))
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

/// How many keys this build does not know it will carry forward.
///
/// Enough for another version's whole category list, few enough that a file
/// filled with junk sheds it rather than being re-serialised on every click.
const MAX_CARRIED_SETTINGS: usize = 64;

/// Add the keys already in the file that `save` did not write.
///
/// A key from another version was reported when the file was read, and that
/// report is not consent to delete it. Read back from disk rather than
/// carried in memory, so a file edited by hand between load and save keeps
/// what was added.
///
/// Read-modify-write against an atomic writer: two processes saving at once
/// can lose the second one's unknown keys. Not worth a lock file for a
/// preference dialog, and the known settings are unaffected either way.
fn carry_forward(path: &Path, file: &mut serde_json::Map<String, serde_json::Value>) {
    let Ok(Some(source)) = crate::config::read(path) else {
        // No file, or one that cannot be read at all. Nothing to carry, and
        // the load that reported it already copied it aside.
        return;
    };
    let Ok(existing) = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&source)
    else {
        // The file became unparseable since it was loaded, so this save is
        // about to replace something the user cannot get back otherwise.
        crate::config::keep_unreadable(path);
        return;
    };
    let mut carried = 0;
    for (setting, value) in existing {
        if file.contains_key(&setting) {
            continue;
        }
        if carried >= MAX_CARRIED_SETTINGS {
            break;
        }
        file.insert(setting, value);
        carried += 1;
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

    /// A default table with one bad entry is refused whole, and says what
    /// the setting takes; the rest of the file still applies.
    #[test]
    fn a_malformed_comment_default_refuses_the_table_and_names_the_shape() {
        let (preferences, errors) = parse(
            r##"{"theme": "dark", "comment_defaults": {"Square": {"color": "#ff0000", "opacity": 50}, "Ink": {"color": "red", "opacity": 50}}}"##,
        );
        assert!(preferences.comment_defaults.is_empty());
        assert_eq!(preferences.theme, ThemePreference::Dark);
        assert!(errors[0].contains("#rrggbb"), "{errors:?}");
        let (preferences, _) =
            parse(r#"{"comment_defaults": {"Ink": {"color": null, "opacity": 100}}}"#);
        assert_eq!(
            preferences.comment_defaults["Ink"],
            CommentDefault {
                color: None,
                opacity_percent: 100
            }
        );
        assert!(parse(r#"{"comment_defaults": {"Ink": {"opacity": 101}}}"#)
            .0
            .comment_defaults
            .is_empty());
    }

    /// A hidden-tools list with anything but ids in it is refused whole and
    /// says what it takes; an id no tool has survives, as a plugin's may.
    #[test]
    fn hidden_tools_are_a_list_of_ids_and_a_bad_list_is_named() {
        let (preferences, errors) = parse(r#"{"hidden_tools": ["tool.note", "plugin.gone"]}"#);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            preferences.hidden_tools,
            BTreeSet::from(["plugin.gone".to_owned(), "tool.note".to_owned()])
        );
        let (preferences, errors) = parse(r#"{"hidden_tools": ["tool.note", 3]}"#);
        assert!(preferences.hidden_tools.is_empty());
        assert!(errors[0].contains(TOOL_IDS), "{errors:?}");
        assert!(parse(r#"{"hidden_tools": "tool.note"}"#).1.len() == 1);
    }

    /// A blank name is no name: comments are then signed by nobody rather
    /// than by an empty string a reader would show as a blank author.
    #[test]
    fn the_commenting_author_is_trimmed_and_a_blank_one_is_none() {
        assert_eq!(
            parse(r#"{"commenting_author": "  Ana  "}"#)
                .0
                .commenting_author
                .as_deref(),
            Some("Ana")
        );
        assert_eq!(
            parse(r#"{"commenting_author": "   "}"#).0.commenting_author,
            None
        );
        let (preferences, errors) = parse(r#"{"commenting_author": 7}"#);
        assert_eq!(preferences.commenting_author, None);
        assert!(errors[0].contains("a name in quotes"), "{errors:?}");
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
            line_weights: false,
            search: SearchOptions {
                case_sensitive: true,
                whole_word: true,
                mode: MatchMode::AllWords,
                include_comments: false,
            },
            commenting_author: Some("Ana Pop".into()),
            comment_defaults: BTreeMap::from([(
                "Square".to_owned(),
                CommentDefault {
                    color: Some([255, 0, 16]),
                    opacity_percent: 50,
                },
            )]),
            hidden_tools: BTreeSet::from(["tool.highlight".to_owned(), "tool.ink".to_owned()]),
            web_links: WebLinks::Block,
            trusted_sites: BTreeSet::from(["example.com".to_owned()]),
            redaction: Some(onionskin_plugin_api::RedactionDefault {
                fill: Some([0, 0, 0]),
                outline: [255, 0, 0],
                overlay: None,
            }),
            javascript: false,
            autocomplete: false,
            autocomplete_numbers: true,
        };

        written.save(&path).expect("preferences save");
        let (read, errors) = Preferences::load(Some(&path));

        assert!(errors.is_empty());
        assert_eq!(read, written);
    }

    #[test]
    fn a_malformed_redaction_default_is_named_and_ignored() {
        let (preferences, errors) = parse(r#"{"redaction": {"fill": "black"}}"#);
        assert_eq!(preferences.redaction, None);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("\"outline\""), "{}", errors[0]);
    }

    #[test]
    fn web_links_name_their_three_choices() {
        let (preferences, errors) =
            parse(r#"{"web_links": "allow", "trusted_sites": ["a.example"]}"#);
        assert!(errors.is_empty());
        assert_eq!(preferences.web_links, WebLinks::Allow);
        assert!(preferences.trusted_sites.contains("a.example"));
        let (_, errors) = parse(r#"{"web_links": "sometimes", "trusted_sites": [3]}"#);
        assert_eq!(errors.len(), 2);
        assert!(
            errors[0].contains("\"ask\", \"allow\", \"block\""),
            "{errors:?}"
        );
        let labels: Vec<_> = WebLinks::ALL.iter().map(|links| links.label()).collect();
        assert_eq!(labels, ["Ask", "Always allow", "Never"]);
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
        std::fs::write(&path, r#"{"future_setting": "me", "theme": "dark"}"#)
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
            written.contains("future_setting"),
            "the save dropped a setting this build does not have: {written}"
        );
        assert!(written.contains("\"light\""), "{written}");
    }

    #[test]
    fn exactly_sixty_four_unknown_settings_survive_regardless_of_key_order() {
        fn exercise(prefix: &str) {
            let path = crate::config::test_dir(&format!("preferences-unknown-cap-{prefix}"))
                .join("preferences.json");
            let mut entries = vec![r#""theme": "dark""#.to_owned()];
            for index in 0..80 {
                entries.push(format!(r#""{prefix}_unknown_{index:02}": {index}"#));
            }
            std::fs::write(&path, format!("{{{}}}", entries.join(",")))
                .expect("the test writes its file");

            let (preferences, errors) = Preferences::load(Some(&path));
            assert_eq!(errors.len(), 80, "{errors:?}");

            preferences.save(&path).expect("preferences save");

            let written = std::fs::read_to_string(&path).expect("the file reads back");
            let saved =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&written)
                    .expect("saved preferences parse");
            assert_eq!(
                saved.keys().filter(|key| key.contains("_unknown_")).count(),
                MAX_CARRIED_SETTINGS,
                "wrong unknown carry-forward count for {prefix}: {written}"
            );
        }

        exercise("aaa");
        exercise("zzz");
    }

    /// A file that became unparseable between load and save is about to be
    /// replaced by this save, so it is copied aside first. Without this the
    /// only rescue copy was made at load time, and a hand edit mid-session
    /// never saw one.
    #[test]
    fn a_file_that_broke_since_it_was_loaded_is_kept_before_the_save_replaces_it() {
        let dir = crate::config::test_dir("preferences-broke-later");
        let path = dir.join("preferences.json");
        let kept = path.with_extension("bak");
        let _ = std::fs::remove_file(&kept);
        std::fs::write(&path, r#"{"theme": "dark"}"#).expect("the test writes its file");
        let (preferences, errors) = Preferences::load(Some(&path));
        assert!(errors.is_empty(), "{errors:?}");
        // Someone edits the file badly while the app is running.
        std::fs::write(&path, "{ broken by hand").expect("the test breaks its file");

        preferences.save(&path).expect("preferences save");

        assert_eq!(
            std::fs::read_to_string(&kept).expect("the copy exists"),
            "{ broken by hand"
        );
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
            parse(r#"{"future_setting": "me", "theme": "dark", "recent_documents": 3}"#);

        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("future_setting"), "{errors:?}");
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
    fn the_dialog_lists_the_categories_that_change_something() {
        assert_eq!(
            PreferenceCategory::ALL.map(PreferenceCategory::label),
            [
                "Commenting",
                "Documents",
                "Forms",
                "General",
                "JavaScript",
                "Page Display",
                "Search",
                "Trust Manager"
            ]
        );
    }
}
