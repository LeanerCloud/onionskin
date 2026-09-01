use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

#[test]
fn parity_capture_artifacts_stay_private() {
    let workspace = workspace_root();

    for path in [
        "parity/reference/acrobat-reader-25/shell.png",
        "parity/onionskin/579b1d9/shell.png",
        "parity/comparison/579b1d9/shell-diff.png",
        "parity/tmp/capture.png",
        "parity/manifests/local-run.json",
    ] {
        assert!(
            git_check_ignore(workspace, path).success(),
            "{path} is not ignored, so private parity evidence could be committed"
        );
    }

    for path in [
        "parity/reference",
        "parity/onionskin",
        "parity/comparison",
        "parity/tmp",
        "parity/manifests",
    ] {
        assert!(
            !git_ls_files(workspace, path).success(),
            "{path} contains tracked private parity evidence"
        );
    }

    for path in [
        "parity/README.md",
        "parity/.gitignore",
        "docs/evidence/parity-reference.md",
        "crates/app/tests/parity_privacy.rs",
    ] {
        assert!(
            !git_check_ignore(workspace, path).success(),
            "{path} is ignored, so the public parity protocol would not be tracked"
        );
        assert!(
            git_ls_files(workspace, path).success(),
            "{path} is not tracked, so the public parity protocol would be missing"
        );
    }

    let readme = std::fs::read_to_string(workspace.join("parity/README.md"))
        .expect("parity README is readable");
    assert!(
        readme.contains("Do not commit Acrobat"),
        "the parity README does not state the private screenshot commit ban"
    );

    let ledger = std::fs::read_to_string(workspace.join("docs/evidence/parity-reference.md"))
        .expect("parity evidence ledger is readable");
    assert!(
        ledger.contains("REPO-010 remains open"),
        "the parity ledger must not claim the private screenshot blocker is closed"
    );
}

fn git_check_ignore(workspace: &Path, path: &str) -> ExitStatus {
    Command::new("git")
        .arg("check-ignore")
        .arg("--quiet")
        .arg("--no-index")
        .arg(path)
        .current_dir(workspace)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("git check-ignore is runnable")
}

fn git_ls_files(workspace: &Path, path: &str) -> ExitStatus {
    Command::new("git")
        .arg("ls-files")
        .arg("--error-unmatch")
        .arg(path)
        .current_dir(workspace)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("git ls-files is runnable")
}

fn workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("app crate lives under crates/app")
}
