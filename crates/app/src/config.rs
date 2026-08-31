//! Where Onionskin keeps its own files, and how it writes them.
//!
//! One directory on every platform, `~/.config/onionskin`, which is the
//! keymap path the plan names and the one Schist uses. `XDG_CONFIG_HOME`
//! overrides it where it is set.
//!
//! Everything written here is written owner-only. The recents list is a list
//! of paths to documents the user opened, which is exactly the kind of thing
//! a shared or backed-up home directory should not hand to another account.

use std::ffi::OsStr;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

pub const KEYMAP_FILE: &str = "keymap.json";
pub const PREFERENCES_FILE: &str = "preferences.json";
pub const RECENTS_FILE: &str = "recents.json";

/// The configuration directory, or `None` when the environment names no home
/// to put it in. A caller that gets `None` runs on defaults and cannot
/// persist; it says so rather than inventing a path.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_from(
        std::env::var_os("XDG_CONFIG_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    )
}

/// Split out from [`config_dir`] so the rule is testable without setting
/// process-wide environment variables under a threaded test runner.
fn config_dir_from(xdg_config_home: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    if let Some(xdg) = xdg_config_home.filter(|value| !value.is_empty()) {
        return Some(Path::new(xdg).join("onionskin"));
    }
    let home = home.filter(|value| !value.is_empty())?;
    Some(Path::new(home).join(".config").join("onionskin"))
}

/// The three files, resolved once at startup.
///
/// Every path is optional together: an environment that names no home
/// directory gives an app that runs on defaults and cannot persist, which
/// it says once rather than failing on every write.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConfigPaths {
    pub keymap: Option<PathBuf>,
    pub preferences: Option<PathBuf>,
    pub recents: Option<PathBuf>,
}

impl ConfigPaths {
    pub fn resolve() -> Self {
        match config_dir() {
            Some(dir) => Self::in_dir(&dir),
            None => Self::default(),
        }
    }

    /// The same three files under `dir`. What a test drives, and what keeps
    /// the tests off the developer's own configuration.
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            keymap: Some(dir.join(KEYMAP_FILE)),
            preferences: Some(dir.join(PREFERENCES_FILE)),
            recents: Some(dir.join(RECENTS_FILE)),
        }
    }
}

/// The file's contents, or `None` when it does not exist. A missing config
/// file is the normal first-run state, not a failure.
pub fn read(path: &Path) -> io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(Some(contents)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Write `contents` owner-only, as one step.
///
/// Written to a temporary file beside the destination and renamed over it,
/// so a crash or a full disk leaves the previous file rather than half of
/// the new one. A half-written config reads as malformed on the next start,
/// which is a worse failure than the write that did not happen.
///
/// The rename also means the mode is always the one set here, rather than
/// whatever an existing file happened to carry.
pub fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension("writing");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let write = options
        .open(&temporary)
        .and_then(|mut file| file.write_all(contents.as_bytes()));
    if let Err(error) = write {
        let _ = std::fs::remove_file(&temporary);
        return Err(error);
    }
    std::fs::rename(&temporary, path)
}

/// How many rescue copies of one file this will make before giving up.
/// Past this the user has a directory full of them and is not reading the
/// notice anyway.
const KEPT_COPIES: u32 = 9;

/// Keep a file this build could not read, and say where.
///
/// A user who mistypes their keymap gets one notice; without this the next
/// save would take the rest of the file with it. The note goes on the end
/// of the message that reports the file, so the two cannot be separated.
///
/// Never writes over a file it did not create: a `.bak` may be the user's
/// own, or the rescue copy from the edit before this one, and the whole
/// point is not to lose either. A copy that could not be made is said
/// plainly, because that message is the only warning the user gets that
/// their file is about to be replaced.
pub fn keep_unreadable(path: &Path) -> String {
    let mut kept = path.with_extension("bak");
    // One more turn than there are numbered names, because the first turn
    // tries the unnumbered one.
    for attempt in 1..=KEPT_COPIES + 1 {
        match copy_new(path, &kept) {
            Ok(()) => return format!(" (a copy of it is kept at {})", kept.display()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                kept = path.with_extension(format!("bak.{attempt}"));
            }
            Err(error) => {
                return format!(
                    " (it could not be copied aside: {error}, so the next save replaces it)"
                )
            }
        }
    }
    format!(
        " (it could not be copied aside: {} and its {KEPT_COPIES} numbered copies already \
         exist, so the next save replaces it)",
        path.with_extension("bak").display()
    )
}

/// Copy `from` to `to` only when `to` does not exist yet, owner-only.
///
/// The mode matters as much here as in [`write_private`]: the file most
/// likely to need rescuing is the recents list, and a copy of it is the same
/// list of document paths. `std::fs::copy` would carry the source's mode
/// instead, and this copy is never replaced, so a wide one would stay wide.
fn copy_new(from: &Path, to: &Path) -> io::Result<()> {
    let contents = std::fs::read(from)?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(to)?.write_all(&contents)
}

/// A directory this test process owns, for the modules whose subject is a
/// file on disk.
#[cfg(test)]
pub(crate) fn test_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("onionskin-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("the test can make its own directory");
    dir
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;

    fn dir(xdg: Option<&str>, home: Option<&str>) -> Option<PathBuf> {
        let xdg = xdg.map(OsString::from);
        let home = home.map(OsString::from);
        config_dir_from(xdg.as_deref(), home.as_deref())
    }

    #[test]
    fn the_config_directory_is_xdg_first_then_the_home_default() {
        assert_eq!(
            dir(Some("/tmp/xdg"), Some("/home/user")),
            Some(PathBuf::from("/tmp/xdg/onionskin"))
        );
        assert_eq!(
            dir(None, Some("/home/user")),
            Some(PathBuf::from("/home/user/.config/onionskin"))
        );
    }

    /// An empty variable is not a path. Exported-but-empty is common in
    /// stripped environments, and joining onto it would put the config in
    /// the process's working directory.
    #[test]
    fn an_empty_or_missing_environment_names_no_directory() {
        assert_eq!(
            dir(Some(""), Some("/home/user")),
            Some(PathBuf::from("/home/user/.config/onionskin"))
        );
        assert_eq!(dir(None, Some("")), None);
        assert_eq!(dir(None, None), None);
    }

    /// A file the app already wrote is replaced with the new mode, not left
    /// with whatever it had. The rename is what makes that true, and it is
    /// also what keeps a widened file from staying widened.
    #[cfg(unix)]
    #[test]
    fn rewriting_an_existing_file_leaves_it_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;

        let path = test_dir("config-rewrite").join("recents.json");
        std::fs::write(&path, "{}").expect("the test writes its file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("the test can widen its own file");

        write_private(&path, "{\"documents\":[]}").expect("the file is rewritten");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn a_file_this_build_cannot_read_is_kept_before_it_is_replaced() {
        let dir = test_dir("config-preserve");
        let path = dir.join("keymap.json");
        let kept = path.with_extension("bak");
        let _ = std::fs::remove_file(&kept);
        let _ = std::fs::remove_file(path.with_extension("bak.1"));
        std::fs::write(&path, "not json").expect("the test writes its file");

        let note = keep_unreadable(&path);

        assert!(note.contains(&kept.display().to_string()), "{note}");
        assert_eq!(
            std::fs::read_to_string(&kept).expect("the copy reads"),
            "not json"
        );
    }

    /// A `.bak` may be the user's own, or the rescue copy from the previous
    /// bad edit. Neither is this code's to overwrite, and losing the first
    /// of two bad edits is exactly how a rescue copy becomes useless.
    ///
    /// Run past the last numbered name, so the count in the give-up message
    /// has to be the count of names this really tries.
    #[test]
    fn every_rescue_copy_keeps_its_own_contents_until_the_names_run_out() {
        let dir = test_dir("config-preserve-many");
        let path = dir.join("preferences.json");
        let names: Vec<String> = std::iter::once("bak".to_owned())
            .chain((1..=KEPT_COPIES).map(|index| format!("bak.{index}")))
            .collect();
        for name in &names {
            let _ = std::fs::remove_file(path.with_extension(name));
        }

        for (index, name) in names.iter().enumerate() {
            std::fs::write(&path, format!("bad edit {index}")).expect("the test writes its file");
            let note = keep_unreadable(&path);
            assert!(
                note.contains(&path.with_extension(name).display().to_string()),
                "copy {index} went somewhere else: {note}"
            );
        }
        // Every earlier copy still holds what it held.
        for (index, name) in names.iter().enumerate() {
            assert_eq!(
                std::fs::read_to_string(path.with_extension(name)).expect("the copy reads"),
                format!("bad edit {index}")
            );
        }

        std::fs::write(&path, "one edit too many").expect("the test writes its file");
        let note = keep_unreadable(&path);

        assert!(note.contains("could not be copied aside"), "{note}");
        assert!(
            note.contains(&format!("{KEPT_COPIES} numbered copies")),
            "{note}"
        );
    }

    /// The file most likely to need rescuing is the recents list, and a copy
    /// of it is the same list of document paths.
    #[cfg(unix)]
    #[test]
    fn a_rescue_copy_is_owner_only_like_the_file_it_copies() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = test_dir("config-preserve-mode");
        let path = dir.join("recents.json");
        let kept = path.with_extension("bak");
        let _ = std::fs::remove_file(&kept);
        std::fs::write(&path, "{ not json").expect("the test writes its file");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
            .expect("the test can widen its own file");

        keep_unreadable(&path);

        let mode = std::fs::metadata(&kept).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "the copy of a private list is readable by other accounts"
        );
    }

    /// The note is the only warning that the file is about to be replaced,
    /// so a copy that could not be made has to say so rather than leave the
    /// message reading as though the contents were saved.
    #[test]
    fn a_rescue_copy_that_cannot_be_made_says_so() {
        let path = test_dir("config-preserve-missing").join("gone.json");
        let _ = std::fs::remove_file(&path);

        let note = keep_unreadable(&path);

        assert!(note.contains("could not be copied aside"), "{note}");
        assert!(note.contains("the next save replaces it"), "{note}");
    }

    #[test]
    fn a_missing_file_reads_as_absent_rather_than_as_an_error() {
        let path = test_dir("config-missing").join("nothing.json");

        assert_eq!(read(&path).expect("a missing file is not an error"), None);
    }

    #[test]
    fn a_written_file_round_trips_and_is_owner_only() {
        let path = test_dir("config-write").join("nested").join("recents.json");

        write_private(&path, "{\"documents\":[]}").expect("the file is written");
        assert!(
            !path.with_extension("writing").exists(),
            "the temporary file is renamed away, not left behind"
        );

        assert_eq!(
            read(&path).expect("the file reads back").as_deref(),
            Some("{\"documents\":[]}")
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "a recents list must not be readable by other accounts"
            );
        }
    }
}
