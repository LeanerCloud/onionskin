//! Where Onionskin keeps its own files, and how it writes them.
//!
//! One directory on every platform, `~/.config/onionskin`, which is the
//! keymap path the plan names and the one Schist uses. `XDG_CONFIG_HOME`
//! overrides it where it is set.
//!
//! Everything written here is written owner-only, and so is the directory:
//! the recents list is a list of paths to documents the user opened, and the
//! file names alone say which of Onionskin's features someone uses. Neither
//! is something a shared or backed-up home directory should hand to another
//! account.

use std::ffi::OsStr;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

pub const KEYMAP_FILE: &str = "keymap.json";
pub const PREFERENCES_FILE: &str = "preferences.json";
pub const RECENTS_FILE: &str = "recents.json";
/// The folder beside the settings files that tools keep their own files in.
pub const DATA_DIR: &str = "data";
/// Autosave's recovery files, one per open document with unsaved edits.
pub const RECOVERY_DIR: &str = "recovery";

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

/// The user's home directory, whatever this platform calls it.
///
/// Windows sets `USERPROFILE` and normally leaves `HOME` unset, so reading
/// only `HOME` there quietly means "no home", which is how an abbreviation
/// meant to keep an account name off the screen stops happening on one
/// platform.
pub fn home_dir() -> Option<PathBuf> {
    for variable in ["HOME", "USERPROFILE"] {
        if let Some(value) = std::env::var_os(variable).filter(|value| !value.is_empty()) {
            return Some(PathBuf::from(value));
        }
    }
    None
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
    /// Where tools keep files of their own - the custom stamp library - as
    /// opposed to the settings files above.
    pub data: Option<PathBuf>,
    /// Where autosave keeps each open document's unsaved edits. Owner-only;
    /// `core::recovery` refuses a directory that is not.
    pub recovery: Option<PathBuf>,
    /// The user's home directory, resolved once here so the surfaces that
    /// shorten a path for display do not ask the environment per row per
    /// frame.
    pub home: Option<PathBuf>,
}

impl ConfigPaths {
    pub fn resolve() -> Self {
        let home = home_dir();
        match config_dir() {
            Some(dir) => Self {
                home,
                ..Self::in_dir(&dir)
            },
            None => Self {
                home,
                ..Self::default()
            },
        }
    }

    /// The same three files under `dir`. What a test drives, and what keeps
    /// the tests off the developer's own configuration.
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            keymap: Some(dir.join(KEYMAP_FILE)),
            preferences: Some(dir.join(PREFERENCES_FILE)),
            recents: Some(dir.join(RECENTS_FILE)),
            data: Some(dir.join(DATA_DIR)),
            recovery: Some(dir.join(RECOVERY_DIR)),
            home: home_dir(),
        }
    }
}

/// The largest config file this will read.
///
/// Every file here is meant to be edited by hand, and the biggest of them is
/// a recents list of fifty paths. A megabyte is far past anything legitimate
/// and small enough that reading it costs nothing.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// The file's contents, or `None` when it does not exist. A missing config
/// file is the normal first-run state, not a failure.
///
/// Bounded, and not as a formality: a `keymap.json` symlinked to
/// `/dev/urandom` held startup for a minute while memory grew, because
/// nothing about `read_to_string` stops at a file that never ends. Three of
/// these are read before the window opens, so a config file must not be able
/// to hang the app. Anything larger is refused by name, and the caller's
/// defaults stand.
pub fn read(path: &Path) -> io::Result<Option<String>> {
    use std::io::Read as _;

    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    // One byte past the limit, so a file exactly at it still reads and
    // anything longer is caught without reading the rest of it.
    let mut contents = String::new();
    let read = file
        .take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut contents)?;
    if read as u64 > MAX_CONFIG_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} is larger than {MAX_CONFIG_BYTES} bytes, which no configuration file is",
                path.display()
            ),
        ));
    }
    Ok(Some(contents))
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
        create_private_dir(parent)?;
    }
    let temporary = path.with_extension("writing");
    // Removed first and created new, rather than truncated: `mode` applies
    // only to a file this call creates, so a leftover `.writing` from a
    // killed process would keep whatever mode it had and carry it through
    // the rename onto the real file.
    match std::fs::remove_file(&temporary) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
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

/// Create the configuration directory, owner-only.
///
/// The names of the files in it say which features someone uses, so the
/// directory is 0700 for the same reason the files are 0600. An existing
/// directory keeps its mode: a user who widened it did so deliberately.
fn create_private_dir(path: &Path) -> io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt as _;
        builder.mode(0o700);
    }
    builder.create(path)
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

    /// The directory is as private as the files: its listing says which
    /// features this account uses.
    #[cfg(unix)]
    #[test]
    fn the_configuration_directory_is_owner_only() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = test_dir("config-dir-mode").join("fresh");
        let _ = std::fs::remove_dir_all(&dir);

        write_private(&dir.join("recents.json"), "{}").expect("the file is written");

        let mode = std::fs::metadata(&dir).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o077,
            0,
            "the config directory is readable by others"
        );
    }

    /// A `.writing` file left by a killed process must not lend its mode to
    /// the file that replaces it.
    #[cfg(unix)]
    #[test]
    fn a_stale_temporary_file_does_not_widen_the_file_it_becomes() {
        use std::os::unix::fs::PermissionsExt as _;

        let path = test_dir("config-stale-temp").join("recents.json");
        let temporary = path.with_extension("writing");
        std::fs::write(&temporary, "left behind").expect("the test writes its file");
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o644))
            .expect("the test can widen its own file");

        write_private(&path, "{\"documents\":[]}").expect("the file is written");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    /// Windows sets USERPROFILE and normally leaves HOME unset, so reading
    /// only HOME there means "no home", and the abbreviation that keeps an
    /// account name off the screen silently stops happening on that
    /// platform.
    #[test]
    fn the_home_directory_is_found_under_the_name_this_platform_uses() {
        let named = ["HOME", "USERPROFILE"]
            .iter()
            .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()));

        assert_eq!(
            named,
            home_dir().is_some(),
            "home_dir disagrees with the environment it reads"
        );
    }

    #[test]
    fn a_missing_file_reads_as_absent_rather_than_as_an_error() {
        let path = test_dir("config-missing").join("nothing.json");

        assert_eq!(read(&path).expect("a missing file is not an error"), None);
    }

    /// A config file is read with a bound, because it may be a symlink to
    /// something that never ends. Checked on a real file rather than on the
    /// constant, so the bound is the one `read` applies.
    #[test]
    fn a_file_larger_than_a_configuration_file_is_refused_by_name() {
        let path = test_dir("config-too-large").join("keymap.json");
        let oversized = "x".repeat(MAX_CONFIG_BYTES as usize + 1);
        std::fs::write(&path, &oversized).expect("the test writes its file");

        let error = read(&path).expect_err("an oversized file is refused");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("keymap.json"), "{error}");
        assert!(error.to_string().contains("larger than"), "{error}");
    }

    /// The case the bound exists for: a config file that never ends.
    ///
    /// This is what a `keymap.json` symlinked to `/dev/urandom` does, and
    /// before the bound it held startup for a minute while memory grew. The
    /// test finishes in milliseconds or not at all, which is the assertion.
    #[cfg(unix)]
    #[test]
    fn a_file_that_never_ends_is_refused_rather_than_read_forever() {
        let path = test_dir("config-endless").join("keymap.json");
        let _ = std::fs::remove_file(&path);
        std::os::unix::fs::symlink("/dev/urandom", &path).expect("the test makes its symlink");

        let started = std::time::Instant::now();
        let error = read(&path).expect_err("an endless file is refused");

        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "reading an endless file took {:?}",
            started.elapsed()
        );
    }

    /// And a file at the limit still reads, so the bound refuses only what it
    /// means to.
    #[test]
    fn a_file_at_the_limit_still_reads() {
        let path = test_dir("config-at-limit").join("keymap.json");
        let contents = "x".repeat(MAX_CONFIG_BYTES as usize);
        std::fs::write(&path, &contents).expect("the test writes its file");

        assert_eq!(
            read(&path).expect("a file at the limit reads"),
            Some(contents)
        );
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
