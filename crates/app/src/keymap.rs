//! Acrobat's keyboard defaults, remappable through `keymap.json`.
//!
//! The file lives at `~/.config/onionskin/keymap.json` and is a flat object
//! of command id to keystroke:
//!
//! ```json
//! { "view.zoom-in": "cmd-shift-=", "file.close": null }
//! ```
//!
//! `null` unbinds a default. Without it, moving a keystroke from one command
//! to another would always collide with the command that had it.
//!
//! Nothing here is fatal. A file that will not parse, an id that no command
//! answers to, a keystroke two commands both claim: each is reported and the
//! rest of the file still applies. Refusing to start because of a text file
//! the user can no longer read would be a worse failure than starting with
//! the defaults and saying so.
//!
//! Keystrokes are written the way GPUI writes them, with `cmd` for the
//! command modifier; [`platform_keystroke`] maps that to `ctrl` off macOS,
//! which is the mapping `plugin_api::Command::keybind` documents.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// A command and the keystroke it carries when the user has not said
/// otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandDefault {
    pub id: &'static str,
    pub keystroke: Option<&'static str>,
}

/// A command and the keystroke it ended up with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Binding {
    pub id: &'static str,
    pub keystroke: String,
}

#[derive(Debug)]
pub enum KeymapError {
    Unreadable {
        path: PathBuf,
        source: io::Error,
    },
    Malformed {
        path: PathBuf,
        message: String,
    },
    UnknownCommand {
        path: PathBuf,
        id: String,
    },
    EmptyKeystroke {
        path: PathBuf,
        id: String,
    },
    /// One binding's value is neither a keystroke nor null. The rest of the
    /// file is fine, which is why this is not [`KeymapError::Malformed`].
    InvalidBinding {
        path: PathBuf,
        id: String,
        value: String,
    },
    /// Two commands claim one keystroke. The earlier command in the built-in
    /// table keeps it, so which one wins does not depend on the order of a
    /// JSON object.
    Duplicate {
        keystroke: String,
        kept: &'static str,
        dropped: &'static str,
    },
}

impl fmt::Display for KeymapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unreadable { path, source } => {
                write!(f, "{} could not be read: {source}", path.display())
            }
            Self::Malformed { path, message } => write!(
                f,
                "{} is not a JSON object of command id to keystroke: {message}",
                path.display()
            ),
            Self::UnknownCommand { path, id } => write!(
                f,
                "{} binds \"{id}\", which is not a command this build has",
                path.display()
            ),
            Self::EmptyKeystroke { path, id } => write!(
                f,
                "{} binds \"{id}\" to an empty keystroke; use null to unbind it",
                path.display()
            ),
            Self::InvalidBinding { path, id, value } => write!(
                f,
                "{} binds \"{id}\" to {value}, which is not a keystroke or null",
                path.display()
            ),
            Self::Duplicate {
                keystroke,
                kept,
                dropped,
            } => write!(
                f,
                "{keystroke} is bound to both \"{kept}\" and \"{dropped}\"; \
                 \"{kept}\" keeps it, so unbind one of them with null"
            ),
        }
    }
}

impl std::error::Error for KeymapError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable { source, .. } => Some(source),
            Self::Malformed { .. }
            | Self::UnknownCommand { .. }
            | Self::EmptyKeystroke { .. }
            | Self::InvalidBinding { .. }
            | Self::Duplicate { .. } => None,
        }
    }
}

/// The keystrokes in force: the built-in table with the user's file applied.
#[derive(Debug, Default)]
pub struct Keymap {
    bindings: Vec<Binding>,
    errors: Vec<KeymapError>,
}

impl Keymap {
    /// The built-in table with `~/.config/onionskin/keymap.json` applied when
    /// it exists. A missing file is the ordinary case and not an error.
    ///
    /// `macos` decides how the file's keystrokes are compared: off macOS
    /// `cmd` is `ctrl`, so two commands that do not collide on a Mac collide
    /// everywhere else.
    pub fn load(defaults: &[CommandDefault], path: Option<&Path>, macos: bool) -> Self {
        let Some(path) = path else {
            return Self::resolve(defaults, None, Path::new(crate::config::KEYMAP_FILE), macos);
        };
        match crate::config::read(path) {
            Ok(source) => {
                let mut keymap = Self::resolve(defaults, source.as_deref(), path, macos);
                keymap.keep_if_malformed(path);
                keymap
            }
            Err(source) => {
                let mut keymap = Self::resolve(defaults, None, path, macos);
                keymap.errors.push(KeymapError::Unreadable {
                    path: path.to_path_buf(),
                    source,
                });
                keymap
            }
        }
    }

    /// The built-in table with `source` applied, if any. Named separately
    /// from [`Keymap::load`] because everything interesting about a keymap
    /// happens between a file's text and the resulting bindings.
    pub fn resolve(
        defaults: &[CommandDefault],
        source: Option<&str>,
        path: &Path,
        macos: bool,
    ) -> Self {
        let mut resolved: Vec<(&'static str, Option<String>)> = defaults
            .iter()
            .map(|default| (default.id, default.keystroke.map(str::to_owned)))
            .collect();
        let mut errors = Vec::new();

        if let Some(source) = source {
            match serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(source) {
                Ok(overrides) => {
                    for (id, value) in overrides {
                        apply_override(&mut resolved, &mut errors, path, &id, &value);
                    }
                }
                Err(error) => errors.push(KeymapError::Malformed {
                    path: path.to_path_buf(),
                    message: error.to_string(),
                }),
            }
        }

        let mut bindings: Vec<Binding> = Vec::new();
        let mut claimed: Vec<(String, &'static str)> = Vec::new();
        for (id, keystroke) in resolved {
            let Some(keystroke) = keystroke else {
                continue;
            };
            let chord = chord(&keystroke, macos);
            match claimed.iter().find(|(taken, _)| *taken == chord) {
                // Reported in this platform's spelling: off macOS a file
                // that binds ctrl-o collides with a cmd-o default, and
                // naming either spelling alone reads as a mistake.
                Some((_, kept)) => errors.push(KeymapError::Duplicate {
                    keystroke: platform_keystroke(&keystroke, macos),
                    kept,
                    dropped: id,
                }),
                None => {
                    claimed.push((chord, id));
                    bindings.push(Binding { id, keystroke });
                }
            }
        }

        Self { bindings, errors }
    }

    /// Copy the file aside when this build could not parse it at all, so
    /// the next save does not take the user's only copy with it.
    ///
    /// Only for the document-level failure: a file with one bad binding is
    /// still a file the user can read and this build can mostly apply.
    fn keep_if_malformed(&mut self, path: &Path) {
        for error in &mut self.errors {
            if let KeymapError::Malformed { message, .. } = error {
                message.push_str(&crate::config::keep_unreadable(path));
            }
        }
    }

    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }

    pub fn errors(&self) -> &[KeymapError] {
        &self.errors
    }
}

/// One keystroke's identity, for deciding whether two bindings collide.
///
/// Comparing the strings is not enough: `cmd-shift-a` and `shift-cmd-a` are
/// the same chord, and off macOS `cmd-a` and `ctrl-a` are too. Both used to
/// bind, leaving which one wins to GPUI's internal order.
///
/// The key is whatever follows the last separator, which is empty when the
/// key *is* the separator: `cmd--` is the minus key.
fn chord(keystroke: &str, macos: bool) -> String {
    let mapped = platform_keystroke(keystroke, macos);
    let mut parts: Vec<&str> = mapped.split('-').collect();
    let key = parts.pop().unwrap_or_default();
    parts.sort_unstable();
    parts.dedup();
    format!("{}|{key}", parts.join("-"))
}

fn apply_override(
    resolved: &mut [(&'static str, Option<String>)],
    errors: &mut Vec<KeymapError>,
    path: &Path,
    id: &str,
    value: &serde_json::Value,
) {
    let Some(entry) = resolved.iter_mut().find(|(known, _)| *known == id) else {
        errors.push(KeymapError::UnknownCommand {
            path: path.to_path_buf(),
            id: id.to_owned(),
        });
        return;
    };
    match value {
        serde_json::Value::Null => entry.1 = None,
        serde_json::Value::String(keystroke) if !keystroke.trim().is_empty() => {
            entry.1 = Some(keystroke.trim().to_owned());
        }
        serde_json::Value::String(_) => errors.push(KeymapError::EmptyKeystroke {
            path: path.to_path_buf(),
            id: id.to_owned(),
        }),
        other => errors.push(KeymapError::InvalidBinding {
            path: path.to_path_buf(),
            id: id.to_owned(),
            value: other.to_string(),
        }),
    }
}

/// The keystroke as this platform's users type it.
///
/// The tables are written in GPUI's macOS spelling, with `cmd` for the
/// command modifier; Linux and Windows read that as `ctrl`. Only modifier
/// positions are mapped, so a binding whose key is literally `cmd` is left
/// alone.
pub fn platform_keystroke(keystroke: &str, macos: bool) -> String {
    if macos {
        return keystroke.to_owned();
    }
    let parts: Vec<&str> = keystroke.split('-').collect();
    let last = parts.len().saturating_sub(1);
    parts
        .iter()
        .enumerate()
        .map(|(index, part)| {
            if index < last && *part == "cmd" {
                "ctrl"
            } else {
                part
            }
        })
        .collect::<Vec<_>>()
        .join("-")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEFAULTS: [CommandDefault; 4] = [
        CommandDefault {
            id: "file.open",
            keystroke: Some("cmd-o"),
        },
        CommandDefault {
            id: "file.close",
            keystroke: Some("cmd-w"),
        },
        CommandDefault {
            id: "view.zoom-in",
            keystroke: Some("cmd-="),
        },
        CommandDefault {
            id: "help.about",
            keystroke: None,
        },
    ];

    fn resolve(source: &str) -> Keymap {
        Keymap::resolve(&DEFAULTS, Some(source), Path::new("/tmp/keymap.json"), true)
    }

    fn keystroke<'a>(keymap: &'a Keymap, id: &str) -> Option<&'a str> {
        keymap
            .bindings()
            .iter()
            .find(|binding| binding.id == id)
            .map(|binding| binding.keystroke.as_str())
    }

    fn messages(keymap: &Keymap) -> Vec<String> {
        keymap.errors().iter().map(ToString::to_string).collect()
    }

    #[test]
    fn without_a_file_the_defaults_are_the_keymap() {
        let keymap = Keymap::resolve(&DEFAULTS, None, Path::new("/tmp/keymap.json"), true);

        assert!(keymap.errors().is_empty());
        assert_eq!(keystroke(&keymap, "file.open"), Some("cmd-o"));
        assert_eq!(keystroke(&keymap, "help.about"), None);
        assert_eq!(keymap.bindings().len(), 3, "the unbound command has no row");
    }

    #[test]
    fn a_rebound_command_takes_the_users_keystroke_and_leaves_the_rest() {
        let keymap = resolve(r#"{"view.zoom-in": "cmd-shift-="}"#);

        assert!(messages(&keymap).is_empty(), "{:?}", messages(&keymap));
        assert_eq!(keystroke(&keymap, "view.zoom-in"), Some("cmd-shift-="));
        assert_eq!(keystroke(&keymap, "file.open"), Some("cmd-o"));
    }

    /// Binding a command that has no default is how the file reaches the
    /// commands Acrobat ships unbound.
    #[test]
    fn a_command_with_no_default_can_be_given_one() {
        let keymap = resolve(r#"{"help.about": "cmd-i"}"#);

        assert!(messages(&keymap).is_empty());
        assert_eq!(keystroke(&keymap, "help.about"), Some("cmd-i"));
    }

    #[test]
    fn null_unbinds_a_default_so_its_keystroke_can_move() {
        let keymap = resolve(r#"{"file.close": null, "view.zoom-in": "cmd-w"}"#);

        assert!(messages(&keymap).is_empty(), "{:?}", messages(&keymap));
        assert_eq!(keystroke(&keymap, "file.close"), None);
        assert_eq!(keystroke(&keymap, "view.zoom-in"), Some("cmd-w"));
    }

    /// An id nobody answers to is the typo case, and a typo that silently
    /// did nothing would leave the user believing the binding took.
    #[test]
    fn an_unknown_command_id_is_reported_and_the_rest_of_the_file_still_applies() {
        let keymap = resolve(r#"{"file.opne": "cmd-o", "file.close": "cmd-k"}"#);

        assert_eq!(
            messages(&keymap),
            vec![
                "/tmp/keymap.json binds \"file.opne\", which is not a command this build has"
                    .to_owned()
            ]
        );
        assert_eq!(keystroke(&keymap, "file.close"), Some("cmd-k"));
    }

    #[test]
    fn two_commands_claiming_one_keystroke_are_reported_and_the_earlier_one_keeps_it() {
        let keymap = resolve(r#"{"view.zoom-in": "cmd-w"}"#);

        assert_eq!(
            messages(&keymap),
            vec![
                "cmd-w is bound to both \"file.close\" and \"view.zoom-in\"; \
                 \"file.close\" keeps it, so unbind one of them with null"
                    .to_owned()
            ]
        );
        assert_eq!(keystroke(&keymap, "file.close"), Some("cmd-w"));
        assert_eq!(
            keystroke(&keymap, "view.zoom-in"),
            None,
            "the dropped binding is dropped, not silently doubled"
        );
    }

    /// The loud, non-fatal case: the whole file is unusable, every default
    /// survives, and the message names the file and the parse failure.
    #[test]
    fn a_malformed_file_reports_itself_and_leaves_every_default_in_place() {
        let keymap = resolve("{\"file.open\": ");

        let messages = messages(&keymap);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].starts_with("/tmp/keymap.json is not a JSON object"),
            "{messages:?}"
        );
        assert_eq!(keystroke(&keymap, "file.open"), Some("cmd-o"));
        assert_eq!(keystroke(&keymap, "file.close"), Some("cmd-w"));
    }

    #[test]
    fn a_binding_that_is_neither_a_keystroke_nor_null_is_reported() {
        let keymap = resolve(r#"{"file.open": 7, "file.close": ""}"#);

        let messages = messages(&keymap);
        assert!(
            messages.iter().any(|message| {
                message.contains(r#"binds "file.open" to 7, which is not a keystroke or null"#)
            }),
            "{messages:?}"
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("empty keystroke")),
            "{messages:?}"
        );
        assert_eq!(keystroke(&keymap, "file.open"), Some("cmd-o"));
        assert_eq!(keystroke(&keymap, "file.close"), Some("cmd-w"));
    }

    /// A file with one bad binding is not a file at risk: the rest of it
    /// applied, so nothing is copied aside and nothing claims the document
    /// failed to parse.
    #[test]
    fn one_bad_binding_does_not_make_the_file_unreadable() {
        let dir = crate::config::test_dir("keymap-one-bad");
        let path = dir.join("keymap.json");
        let kept = path.with_extension("bak");
        let _ = std::fs::remove_file(&kept);
        std::fs::write(&path, r#"{"file.open": 7, "file.close": "cmd-k"}"#)
            .expect("the test writes its file");

        let keymap = Keymap::load(&DEFAULTS, Some(&path), true);

        assert!(!kept.exists(), "a readable file was copied aside");
        assert_eq!(messages(&keymap).len(), 1, "{:?}", messages(&keymap));
        assert!(
            !messages(&keymap)[0].contains("is not a JSON object"),
            "{:?}",
            messages(&keymap)
        );
        assert_eq!(keystroke(&keymap, "file.close"), Some("cmd-k"));
    }

    /// The message that reports an unreadable file also says where its
    /// contents were kept, because the next save overwrites the original.
    #[test]
    fn an_unreadable_file_is_kept_and_the_message_says_where() {
        let dir = crate::config::test_dir("keymap-kept");
        let path = dir.join("keymap.json");
        let kept = path.with_extension("bak");
        let _ = std::fs::remove_file(&kept);
        std::fs::write(&path, "{ not json").expect("the test writes its file");

        let keymap = Keymap::load(&DEFAULTS, Some(&path), true);

        let messages = messages(&keymap);
        assert_eq!(messages.len(), 1, "{messages:?}");
        assert!(
            messages[0].contains(&kept.display().to_string()),
            "{messages:?}"
        );
        assert_eq!(
            std::fs::read_to_string(&kept).expect("the copy exists"),
            "{ not json"
        );
        assert_eq!(keystroke(&keymap, "file.open"), Some("cmd-o"));
    }

    #[test]
    fn a_file_on_disk_is_read_and_a_missing_one_is_not_an_error() {
        let dir = crate::config::test_dir("keymap-load");
        let path = dir.join("keymap.json");
        let _ = std::fs::remove_file(&path);

        let missing = Keymap::load(&DEFAULTS, Some(&path), true);
        assert!(missing.errors().is_empty());
        assert_eq!(keystroke(&missing, "file.open"), Some("cmd-o"));

        std::fs::write(&path, r#"{"file.open": "cmd-shift-o"}"#).expect("the test writes its file");
        let loaded = Keymap::load(&DEFAULTS, Some(&path), true);

        assert!(loaded.errors().is_empty());
        assert_eq!(keystroke(&loaded, "file.open"), Some("cmd-shift-o"));
    }

    /// One chord written two ways is one chord. Both spellings used to
    /// bind, and which one GPUI honoured was its own business.
    #[test]
    fn a_collision_is_found_however_the_file_spells_it() {
        let reordered = resolve(r#"{"view.zoom-in": "shift-cmd-a", "file.open": "cmd-shift-a"}"#);

        assert_eq!(reordered.errors().len(), 1, "{:?}", messages(&reordered));
        assert!(
            messages(&reordered)[0].contains("bound to both"),
            "{:?}",
            messages(&reordered)
        );
    }

    /// Off macOS the command modifier is ctrl, so a file that binds one of
    /// each collides there and not on a Mac. The report has to follow the
    /// platform it will run on.
    #[test]
    fn cmd_and_ctrl_collide_only_where_they_are_the_same_key() {
        let source = r#"{"view.zoom-in": "ctrl-o"}"#;

        let on_mac = Keymap::resolve(&DEFAULTS, Some(source), Path::new("/tmp/keymap.json"), true);
        let elsewhere = Keymap::resolve(
            &DEFAULTS,
            Some(source),
            Path::new("/tmp/keymap.json"),
            false,
        );

        assert!(on_mac.errors().is_empty(), "{:?}", messages(&on_mac));
        assert_eq!(keystroke(&on_mac, "view.zoom-in"), Some("ctrl-o"));
        assert_eq!(elsewhere.errors().len(), 1, "{:?}", messages(&elsewhere));
        assert!(
            messages(&elsewhere)[0].starts_with("ctrl-o is bound to both"),
            "the message spells the chord the way this platform types it: {:?}",
            messages(&elsewhere)
        );
        assert_eq!(
            keystroke(&elsewhere, "file.open"),
            Some("cmd-o"),
            "the built-in binding is the one that keeps the chord"
        );
    }

    #[test]
    fn the_command_modifier_becomes_ctrl_away_from_macos() {
        assert_eq!(platform_keystroke("cmd-shift-a", true), "cmd-shift-a");
        assert_eq!(platform_keystroke("cmd-shift-a", false), "ctrl-shift-a");
        assert_eq!(platform_keystroke("alt-left", false), "alt-left");
        assert_eq!(
            platform_keystroke("cmd-cmd", false),
            "ctrl-cmd",
            "only modifier positions are mapped"
        );
    }
}
