//! M4 on a real window, off macOS: the Print dialog lists CUPS's printers,
//! and printing to one runs `lp` with the job's options and the printed
//! sheets. Stand-in `lp` and `lpstat` scripts take the printer's place.

use std::os::unix::fs::PermissionsExt as _;

use super::print::{act, window_over};
use super::*;
use crate::shell::chrome::print_dialog::{Destination, PrintAction};
use crate::shell::chrome::tabs::print::cups;

/// An executable script `name` in `dir` running `body`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("writes");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

/// CUPS with one printer, Office, whose `lp` writes each job's arguments to
/// `job-N.args` and its PDF to `job-N.pdf` in `spool`; or, when `refuse`,
/// says the printer is gone and fails, in real `lp`'s words.
fn office_printer(spool: &Path, refuse: bool) {
    let lp = if refuse {
        "cat > /dev/null\necho 'lp: Error - The printer or class does not exist.' >&2\nexit 1"
            .to_owned()
    } else {
        let at = spool.display();
        format!(
            "n=$(ls '{at}' | grep -c '\\.args$')\nprintf '%s\\n' \"$@\" > '{at}/job-'$n.args\ncat > '{at}/job-'$n.pdf"
        )
    };
    let programs = onionskin_print::CupsPrograms {
        lp: script(spool, "lp", &lp),
        lpstat: script(spool, "lpstat", "echo Office"),
    };
    cups::stand_in(programs);
}

/// Open the Print dialog on `window` and choose Office.
fn choose_office(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::Print, window, cx)
                .expect("opens");
            let state = frame.print_dialog().expect("a dialog");
            assert_eq!(
                state.destinations,
                [
                    Destination::SaveAsPdf,
                    Destination::Printer("Office".into())
                ]
            );
        })
        .unwrap();
    act(window, PrintAction::Destination(1), cx);
}

/// The arguments of job `n`, one a line.
fn arguments(spool: &Path, n: usize) -> Vec<String> {
    std::fs::read_to_string(spool.join(format!("job-{n}.args")))
        .expect("the job ran lp")
        .lines()
        .map(str::to_owned)
        .collect()
}

fn sheets(spool: &Path, n: usize) -> usize {
    onionskin_cos::Document::open_path(&spool.join(format!("job-{n}.pdf")))
        .expect("lp got a PDF")
        .page_count()
        .expect("pages") as usize
}

#[gpui::test]
fn printing_to_a_cups_printer_runs_lp_with_the_sheets(cx: &mut TestAppContext) {
    let spool = tempfile::tempdir().expect("spool");
    office_printer(spool.path(), false);
    let (_dir, window, _bindings) = window_over("two-page.pdf", cx);
    choose_office(window, cx);
    act(window, PrintAction::Print, cx);
    cx.run_until_parked();

    window
        .update(cx, |frame, _, _| {
            assert!(frame.print_dialog().is_none(), "a print that went closes");
            assert_eq!(
                frame.notices.last().map(String::as_str),
                Some("Sent to Office")
            );
        })
        .unwrap();
    let args = arguments(spool.path(), 0);
    assert_eq!(args[..4], ["-d", "Office", "-t", "two-page.pdf"]);
    assert!(
        args.contains(&"media=na_letter_8.5x11in".to_owned()),
        "{args:?}"
    );
    assert_eq!(sheets(spool.path(), 0), 2);
    assert!(!spool.path().join("job-1.args").exists(), "one job");
}

#[gpui::test]
fn a_printer_that_refuses_keeps_the_dialog_open_saying_why(cx: &mut TestAppContext) {
    let spool = tempfile::tempdir().expect("spool");
    office_printer(spool.path(), true);
    let (_dir, window, _bindings) = window_over("two-page.pdf", cx);
    choose_office(window, cx);
    act(window, PrintAction::Print, cx);
    cx.run_until_parked();

    let error = window
        .update(cx, |frame, _, _| {
            frame.print_dialog().and_then(|state| state.error.clone())
        })
        .unwrap();
    assert_eq!(
        error.as_deref(),
        Some("Could not print: lp: Error - The printer or class does not exist.")
    );
}

/// Summarize Comments to a printer is two jobs: the document, then its
/// comment summary under its own title.
#[cfg(feature = "tools-comment")]
#[gpui::test]
fn summarize_comments_sends_the_summary_as_a_second_job(cx: &mut TestAppContext) {
    let spool = tempfile::tempdir().expect("spool");
    office_printer(spool.path(), false);
    let (_dir, window, _bindings) = window_over("two-page.pdf", cx);
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().unwrap().clone();
            canvas.update(cx, |canvas, cx| {
                {
                    let mut document = canvas.model.document_mut();
                    let page = document.structure().unwrap().page(0).unwrap().objref;
                    document
                        .edit_annotations("Sticky Note", |tx, structure| {
                            let mut note = onionskin_core::Annotation::new(
                                onionskin_core::Subtype::Text,
                                onionskin_core::Rect::new(50.0, 50.0, 70.0, 70.0),
                            );
                            note.contents = Some("Check the totals".to_owned());
                            onionskin_core::add_annotation(tx, structure, page, &note, 0)
                        })
                        .expect("adds a note");
                }
                canvas.handle_change(Ok(true), cx);
            });
        })
        .unwrap();
    choose_office(window, cx);
    act(window, PrintAction::SummarizeComments, cx);
    act(window, PrintAction::Print, cx);
    cx.run_until_parked();

    assert_eq!(sheets(spool.path(), 0), 2, "the document first");
    let summary = arguments(spool.path(), 1);
    assert_eq!(
        summary[..4],
        ["-d", "Office", "-t", "two-page.pdf - Comments"]
    );
    assert!(sheets(spool.path(), 1) >= 1, "then its summary");
}
