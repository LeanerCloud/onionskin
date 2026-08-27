//! Feeds arbitrary bytes to the whole open path.
//!
//! The contract is not that anything opens. It is that `cos` either returns a
//! typed `Error` or hands back a document whose objects can all be reached,
//! and that it never panics, never overflows and never hangs. The `/Index`
//! overflow that review finding F1 caught is exactly the shape of bug this
//! target exists to find.
//!
//! Run with nightly:
//!
//! ```text
//! cargo +nightly fuzz run open -- -max_len=65536
//! ```

#![no_main]

use libfuzzer_sys::fuzz_target;
use onionskin_cos::{BytesSource, Document};

/// Enough to exercise the growth loop and the repair scan without letting the
/// fuzzer spend its budget on one enormous input.
const MAX_INPUT: usize = 1 << 20;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }

    // `open` must refuse anything that needed repair rather than opening it.
    if let Ok(document) = Document::open(Box::new(BytesSource::new(data.to_vec()))) {
        assert!(
            document.provenance().is_clean(),
            "open returned a document that was not clean"
        );
        exercise(&document);
    }

    if let Ok((document, provenance)) =
        Document::open_repairing(Box::new(BytesSource::new(data.to_vec())))
    {
        assert_eq!(
            document.provenance(),
            &provenance,
            "the document and the returned provenance disagree"
        );
        exercise(&document);
    }
});

/// Walks everything an ordinary caller would touch, so a bad parse surfaces
/// here rather than in a later milestone.
fn exercise(document: &Document) {
    let _ = document.catalog();
    let _ = document.page_count();
    let _ = document.first_page();
    for (number, _) in document.xref().iter().take(512) {
        if let Ok(parsed) = document.get(number) {
            // A span the document reports must be one the source can hold.
            if let Some(span) = parsed.origin.file_span() {
                assert!(
                    span.end >= span.start && span.end <= document.original_len(),
                    "object {number} reports a span outside the file"
                );
            }
        }
    }
    // Assembling the section must not panic either; a save is where a bad
    // offset turns into a corrupt file.
    let _ = document.incremental_section();
}
