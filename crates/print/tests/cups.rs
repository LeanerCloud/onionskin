//! The CUPS backend against stand-in `lp` and `lpstat` scripts: what `lp` is
//! run with, that the PDF on its standard input is the sheets the job
//! imposed, and that a refusal reaches the user in `lp`'s words. No printer
//! is needed, so this runs on every Unix CI runner. One ignored test runs
//! the same job through a real CUPS scheduler.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use onionskin_corpus_testing::seed;
use onionskin_cos::{BytesSource, Document as CosDocument};
use onionskin_print::{
    backend::cups::printers, impose, CupsBackend, CupsPrograms, Duplex, NUp, PrintBackend,
    PrintError, PrintJob,
};

/// An executable script `name` in `dir` running `body`.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).expect("writes");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    path
}

/// An `lp` that records its arguments, one a line, and its input.
fn recording_lp(dir: &Path) -> CupsPrograms {
    let args = dir.join("args");
    let input = dir.join("input.pdf");
    let body = format!(
        "printf '%s\\n' \"$@\" > '{}'\ncat > '{}'\necho 'request id is Office-7 (1 file(s))'",
        args.display(),
        input.display()
    );
    CupsPrograms {
        lp: script(dir, "lp", &body),
        lpstat: dir.join("no-lpstat"),
    }
}

fn backend(programs: CupsPrograms) -> CupsBackend {
    let bytes = std::fs::read(seed("two-page.pdf")).expect("seed");
    CupsBackend::new(Arc::new(bytes), "two-page.pdf", programs).expect("opens")
}

fn print(backend: &mut CupsBackend, job: &PrintJob) -> Result<(), PrintError> {
    let sheets = impose(job, &backend.page_sizes().expect("sizes")).expect("imposes");
    backend.print(job, &sheets)
}

fn two_up_duplex(printer: &str) -> PrintJob {
    PrintJob {
        printer: Some(printer.into()),
        duplex: Duplex::LongEdge,
        n_up: NUp {
            per_sheet: 2,
            ..NUp::default()
        },
        ..PrintJob::default()
    }
}

#[test]
fn a_job_reaches_lp_as_its_options_and_its_sheets() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut backend = backend(recording_lp(dir.path()));
    let job = PrintJob {
        copies: 3,
        duplex: Duplex::ShortEdge,
        ..two_up_duplex("Office")
    };
    print(&mut backend, &job).expect("prints");

    let args = std::fs::read_to_string(dir.path().join("args")).expect("args");
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(args[..6], ["-d", "Office", "-t", "two-page.pdf", "-n", "3"]);
    assert!(args.contains(&"sides=two-sided-short-edge"));
    assert!(args.contains(&"print-scaling=none"));

    let input = std::fs::read(dir.path().join("input.pdf")).expect("input");
    let printed = CosDocument::open(Box::new(BytesSource::new(input))).expect("a PDF");
    assert_eq!(
        printed.page_count().expect("pages"),
        2,
        "two pages two-up are one sheet, and duplex gives it a blank back"
    );
}

#[test]
fn a_refusal_is_reported_in_lps_words() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lp = script(
        dir.path(),
        "lp",
        "cat > /dev/null\necho 'lp: Error - The printer or class does not exist.' >&2\nexit 1",
    );
    let mut backend = backend(CupsPrograms {
        lp,
        ..CupsPrograms::default()
    });
    match print(&mut backend, &PrintJob::default()) {
        Err(PrintError::Platform(reason)) => {
            assert_eq!(reason, "lp: Error - The printer or class does not exist.")
        }
        other => panic!("expected lp's refusal, got {other:?}"),
    }
}

/// An `lp` that gives up without reading or saying why still fails the
/// print, by its exit status.
#[test]
fn a_silent_failure_names_the_exit_status() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lp = script(dir.path(), "lp", "exit 3");
    let mut backend = backend(CupsPrograms {
        lp,
        ..CupsPrograms::default()
    });
    match print(&mut backend, &PrintJob::default()) {
        Err(PrintError::Platform(reason)) => assert!(reason.starts_with("lp failed"), "{reason}"),
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn a_selection_of_nothing_never_runs_lp() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut backend = backend(recording_lp(dir.path()));
    let job = PrintJob {
        selection: onionskin_print::PageSelection {
            ranges: vec![(3, 4)],
            ..Default::default()
        },
        ..PrintJob::default()
    };
    assert!(matches!(
        print(&mut backend, &job),
        Err(PrintError::NothingToPrint)
    ));
    assert!(!dir.path().join("args").exists());
}

#[test]
fn lpstat_lists_the_printers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let lpstat = script(
        dir.path(),
        "lpstat",
        "[ \"$1\" = -e ] || exit 1\necho Office\necho Home_Laser",
    );
    let programs = CupsPrograms {
        lpstat,
        ..CupsPrograms::default()
    };
    assert_eq!(printers(&programs), ["Office", "Home_Laser"]);

    let failing = CupsPrograms {
        lpstat: script(dir.path(), "failing", "echo Office\nexit 1"),
        ..CupsPrograms::default()
    };
    assert!(
        printers(&failing).is_empty(),
        "a failed lpstat lists nothing"
    );
}

/// Through a real CUPS: `ONIONSKIN_CUPS_QUEUE` names a queue whose backend
/// saves the job the filter chain hands it to `ONIONSKIN_CUPS_OUTPUT` and
/// the job's options to that path plus `.options` (the `capture` backend
/// docs/evidence/m4-cups.md sets up). The job goes through the real `lp`,
/// scheduler and filters, and what reaches the backend is read back.
#[test]
#[ignore = "needs a CUPS queue with a capturing backend; see docs/evidence/m4-cups.md"]
fn a_real_cups_queue_receives_the_sheets() {
    let queue = std::env::var("ONIONSKIN_CUPS_QUEUE").expect("ONIONSKIN_CUPS_QUEUE");
    let output =
        PathBuf::from(std::env::var("ONIONSKIN_CUPS_OUTPUT").expect("ONIONSKIN_CUPS_OUTPUT"));
    let options = output.with_extension("pdf.options");
    let _ = std::fs::remove_file(&output);
    let _ = std::fs::remove_file(&options);
    let programs = CupsPrograms::default();
    assert!(printers(&programs).contains(&queue), "lpstat lists {queue}");

    let mut backend = backend(programs);
    print(&mut backend, &two_up_duplex(&queue)).expect("CUPS takes the job");

    let printed = wait_for_pdf(&output);
    assert_eq!(printed.page_count().expect("pages"), 2);
    let options = std::fs::read_to_string(options).expect("the job's options");
    for option in [
        "sides=two-sided-long-edge",
        "print-scaling=none",
        "media=na_letter_8.5x11in",
    ] {
        assert!(
            options.split(' ').any(|given| given == option),
            "{option} in {options}"
        );
    }
}

/// The PDF in `bytes`: a PDF driver may wrap it in a printer language
/// such as PJL, before `%PDF` and after the last `%%EOF`.
fn enveloped_pdf(bytes: &[u8]) -> Option<Vec<u8>> {
    let find = |needle: &[u8]| bytes.windows(needle.len()).position(|at| at == needle);
    let start = find(b"%PDF")?;
    let end = bytes.windows(5).rposition(|at| at == b"%%EOF")? + 5;
    (start < end).then(|| bytes[start..end].to_vec())
}

/// The PDF at `path` once the scheduler, which runs after `lp` returns, has
/// written all of it.
fn wait_for_pdf(path: &Path) -> CosDocument {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    loop {
        let parsed = std::fs::read(path)
            .ok()
            .and_then(|bytes| enveloped_pdf(&bytes))
            .and_then(|pdf| CosDocument::open(Box::new(BytesSource::new(pdf))).ok());
        match parsed {
            Some(document) => return document,
            None if std::time::Instant::now() > deadline => panic!("CUPS wrote no PDF"),
            None => std::thread::sleep(std::time::Duration::from_millis(100)),
        }
    }
}

#[test]
fn a_pdf_is_found_inside_a_printer_language_envelope() {
    let wrapped = b"\x1b%-12345X@PJL JOB\n%PDF-1.7\nbody\n%%EOF\n@PJL EOJ\n";
    assert_eq!(
        enveloped_pdf(wrapped).as_deref(),
        Some(&b"%PDF-1.7\nbody\n%%EOF"[..])
    );
    assert_eq!(enveloped_pdf(b"no pdf here"), None);
}
