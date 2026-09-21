//! File > Attach to Email and Copy File to Clipboard: handing the open file
//! to the operating system, never to a web service.
//!
//! **No shell, ever.** The mail client is started with the file's path as
//! one argument of an argument vector. A file named `a; rm -rf ~` or
//! `$(touch x).pdf` is a file name here, never a command, because nothing
//! parses the vector for one.

use std::ffi::OsString;
use std::path::Path;

/// A program and its arguments, run directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct MailCommand {
    pub(in crate::shell) program: &'static str,
    pub(in crate::shell) args: Vec<OsString>,
}

/// How this platform asks its mail client for a new message carrying
/// `path`, or `None` where there is no such request to make.
pub(in crate::shell) fn mail_command(path: &Path) -> Option<MailCommand> {
    let path = path.as_os_str().to_owned();
    if cfg!(target_os = "macos") {
        // Mail opens a new message with the file attached, the way Finder's
        // Share > Mail does.
        Some(MailCommand {
            program: "open",
            args: vec!["-a".into(), "Mail".into(), path],
        })
    } else if cfg!(unix) {
        // The freedesktop request for "compose a message with this file".
        Some(MailCommand {
            program: "xdg-email",
            args: vec!["--attach".into(), path],
        })
    } else {
        None
    }
}

/// Start the mail client. Returns what could not be done, for the notice.
pub(in crate::shell) fn attach_to_email(path: &Path) -> Result<(), String> {
    let command = mail_command(path).ok_or("No mail client can be asked on this platform")?;
    run(&command).map_err(|error| format!("The mail client did not start: {error}"))
}

/// Run `command` directly, with no shell between it and the process.
pub(in crate::shell) fn run(command: &MailCommand) -> std::io::Result<()> {
    std::process::Command::new(command.program)
        .args(&command.args)
        .spawn()
        .map(drop)
}

/// A `file://` URI for `path`, with every byte outside the unreserved set
/// percent-encoded, so a name with spaces, quotes or a newline is one URI.
/// This is what Copy File to Clipboard puts on the clipboard, and what a
/// file manager or a mail client resolves back to the file.
pub(in crate::shell) fn file_uri(path: &Path) -> String {
    let mut uri = String::from("file://");
    for byte in path.to_string_lossy().bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(byte as char);
        } else {
            uri.push_str(&format!("%{byte:02X}"));
        }
    }
    uri
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOSTILE: &str = "a; rm -rf ~ $(touch pwned) `id` \"q\" 'q'\nline.pdf";

    #[test]
    fn the_path_is_one_argument_and_no_shell_is_involved() {
        let path = Path::new("/tmp").join(HOSTILE);
        let Some(command) = mail_command(&path) else {
            return;
        };
        assert!(
            !["sh", "bash", "zsh", "cmd", "cmd.exe", "powershell"].contains(&command.program),
            "{} is a shell",
            command.program
        );
        assert_eq!(
            command.args.last(),
            Some(&path.as_os_str().to_owned()),
            "the path is passed whole, as the last argument"
        );
    }

    /// The runner, given a path whose name would create a marker file if
    /// any shell ever saw it: nothing is created. Passing the name through
    /// `sh -c` makes this fail.
    #[cfg(unix)]
    #[test]
    fn a_crafted_file_name_runs_nothing() {
        let dir = tempfile::tempdir().expect("dir");
        let marker = dir.path().join("pwned");
        let hostile = format!(
            "/tmp/report; touch {m} $(touch {m}) `touch {m}` \"q\"\n.pdf",
            m = marker.display()
        );
        run(&MailCommand {
            program: "true",
            args: vec![hostile.into()],
        })
        .expect("the program starts");
        std::thread::sleep(std::time::Duration::from_millis(200));
        assert!(!marker.exists(), "a shell interpreted the file name");
    }

    #[test]
    fn a_file_uri_encodes_everything_a_name_can_hide() {
        let uri = file_uri(Path::new("/docs/My File \"v2\".pdf"));
        assert_eq!(uri, "file:///docs/My%20File%20%22v2%22.pdf");
        assert!(!file_uri(&Path::new("/tmp").join(HOSTILE)).contains('\n'));
    }
}
