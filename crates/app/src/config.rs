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

pub fn config_path(file: &str) -> Option<PathBuf> {
    Some(config_dir()?.join(file))
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

/// Write `contents`, creating the directory and the file owner-only.
///
/// The mode is set as the file is created; an existing file keeps whatever
/// mode it has, because a user who widened it did so deliberately.
pub fn write_private(path: &Path, contents: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options.open(path)?.write_all(contents.as_bytes())
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

    #[test]
    fn a_missing_file_reads_as_absent_rather_than_as_an_error() {
        let path = test_dir("config-missing").join("nothing.json");

        assert_eq!(read(&path).expect("a missing file is not an error"), None);
    }

    #[test]
    fn a_written_file_round_trips_and_is_owner_only() {
        let path = test_dir("config-write").join("nested").join("recents.json");

        write_private(&path, "{\"documents\":[]}").expect("the file is written");

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
