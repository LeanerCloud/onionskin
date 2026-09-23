//! The platform backend: present on macOS only, and on macOS it refuses to
//! start anywhere but the main thread rather than hang a print panel off a
//! worker. What reaches the printer is `tests/file_backend.rs`'s output and
//! `backend::native`'s settings, both tested on every platform; the print
//! itself is the manual acceptance run in the P16 evidence.

#[cfg(not(target_os = "macos"))]
#[test]
fn a_build_without_a_platform_backend_says_so() {
    assert_eq!(onionskin_print::native_backend(), None);
}

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::Arc;

    use onionskin_corpus_testing::seed;
    use onionskin_print::{
        impose, native_backend, printers, MacBackend, PrintBackend, PrintError, PrintJob,
    };

    #[test]
    fn macos_has_its_backend() {
        assert_eq!(native_backend(), Some("macOS"));
        // No printer need exist; the list only has to be readable.
        let _ = printers();
    }

    /// Test threads are not the main thread, which is the point: a print
    /// started from one fails with a reason instead of blocking.
    #[test]
    fn a_print_off_the_main_thread_is_refused_with_a_reason() {
        let bytes = std::fs::read(seed("hello.pdf")).expect("seed");
        let mut backend = MacBackend::new(Arc::new(bytes), "hello").expect("opens");
        let job = PrintJob::default();
        let sheets = impose(&job, &backend.page_sizes().expect("sizes")).expect("valid job");
        match backend.print(&job, &sheets) {
            Err(PrintError::Platform(reason)) => assert!(reason.contains("main thread")),
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}
