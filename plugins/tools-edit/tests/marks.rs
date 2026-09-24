//! Watermarks, backgrounds, headers and footers and Bates numbers through
//! the plugin: what each puts on the page, read back as the text a reader
//! extracts and the pixels the renderer draws, and each one's Update and
//! Remove.

use std::sync::Arc;

use onionskin_core::pages::{Margins, MarkKind};
use onionskin_core::{Document, Error};
use onionskin_corpus_testing::encrypted_fixture;
use onionskin_plugin_api::CommandError;
use onionskin_tools_edit::marks::{
    add_background, add_bates, add_header_footer, add_watermark, marked_pages, page_marks,
    remove_marks, saved_settings, Appearance, Art, Bates, Font, HAlign, HeaderFooter, Numbering,
    TextStyle, VAlign,
};

/// A classic-xref PDF whose object `n` is `objects[n - 1]`.
fn pdf(objects: &[Vec<u8>]) -> Vec<u8> {
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let size = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(format!("trailer\n<< /Size {size} /Root 1 0 R >>\n").as_bytes());
    out.extend_from_slice(format!("startxref\n{xref}\n%%EOF\n").as_bytes());
    out
}

fn stream(data: &str) -> Vec<u8> {
    format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()).into_bytes()
}

/// `count` blank Letter pages; the ones listed in `turned` are turned a
/// quarter clockwise.
fn blank(count: usize, turned: &[usize]) -> Vec<u8> {
    let kids: Vec<String> = (0..count)
        .map(|index| format!("{} 0 R", index + 3))
        .collect();
    let mut objects = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        format!(
            "<< /Type /Pages /Kids [{}] /Count {count} /MediaBox [0 0 612 792] >>",
            kids.join(" ")
        )
        .into_bytes(),
    ];
    for index in 0..count {
        let rotate = if turned.contains(&index) {
            "/Rotate 90"
        } else {
            ""
        };
        objects.push(format!("<< /Type /Page /Parent 2 0 R {rotate} >>").into_bytes());
    }
    pdf(&objects)
}

/// A 100-point blue square, as a one-page PDF to use as art.
fn blue_square() -> Arc<Vec<u8>> {
    Arc::new(pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 100 100] /Contents 4 0 R >>".to_vec(),
        stream("0 0 1 rg 0 0 100 100 re f"),
    ]))
}

fn open(bytes: Vec<u8>) -> Document {
    Document::open_bytes(bytes).expect("opens")
}

/// The page's text, each run with the height of its first glyph's bottom.
fn runs(doc: &mut Document, page: usize) -> Vec<(String, f64)> {
    doc.page_text(page)
        .expect("extracts")
        .runs
        .iter()
        .map(|run| {
            let bottom = run.glyphs[0]
                .quad
                .corners
                .iter()
                .map(|(_, y)| *y)
                .fold(f64::INFINITY, f64::min);
            (run.text.clone(), bottom)
        })
        .collect()
}

fn texts(doc: &mut Document, page: usize) -> Vec<String> {
    runs(doc, page).into_iter().map(|(text, _)| text).collect()
}

/// The pixel at `(x, y)` from the shown top-left, at one pixel a point.
fn pixel(doc: &mut Document, page: usize, (x, y): (u32, u32)) -> [u8; 4] {
    let raster = doc.render_page_now(page, 1.0).expect("renders").raster;
    let at = ((y * raster.width() + x) * 4) as usize;
    raster.rgba()[at..at + 4].try_into().expect("four bytes")
}

/// Whether any pixel in the band of rows is darker than paper.
fn inked_rows(doc: &mut Document, page: usize, rows: std::ops::Range<u32>) -> bool {
    let raster = doc.render_page_now(page, 1.0).expect("renders").raster;
    rows.into_iter().any(|y| {
        (0..raster.width()).any(|x| {
            let at = ((y * raster.width() + x) * 4) as usize;
            raster.rgba()[at] < 128
        })
    })
}

fn header(center: &str, footer: &str) -> HeaderFooter {
    let mut text: [String; 6] = Default::default();
    text[1] = center.to_owned();
    text[4] = footer.to_owned();
    HeaderFooter {
        text,
        style: TextStyle::default(),
        margins: Margins {
            top: 36.0,
            bottom: 36.0,
            left: 72.0,
            right: 72.0,
        },
        numbering: Numbering {
            start: 1,
            date: "2026-09-24".to_owned(),
        },
    }
}

#[test]
fn a_header_and_footer_number_every_page_at_its_top_and_bottom() {
    let mut doc = open(blank(2, &[]));
    add_header_footer(
        &mut doc,
        &[0, 1],
        &header("Page [page] of [pages]", "[date]"),
        false,
        "",
    )
    .expect("adds");
    for page in 0..2 {
        let found = runs(&mut doc, page);
        let heading = format!("Page {} of 2", page + 1);
        let (_, top) = found
            .iter()
            .find(|(text, _)| *text == heading)
            .expect("the header");
        assert!(*top > 792.0 - 36.0 - 12.0 && *top < 792.0 - 36.0, "{top}");
        let (_, bottom) = found
            .iter()
            .find(|(text, _)| text == "2026-09-24")
            .expect("the footer");
        assert!((*bottom - 36.0).abs() < 4.0, "{bottom}");
    }
    assert_eq!(
        page_marks(&mut doc, 0).expect("reads"),
        [MarkKind::HeaderFooter]
    );
    assert_eq!(
        marked_pages(&mut doc, MarkKind::HeaderFooter).expect("reads"),
        [0, 1]
    );
    assert_eq!(marked_pages(&mut doc, MarkKind::Bates).expect("reads"), []);
    assert!(doc.undo().expect("undoes"));
    assert_eq!(texts(&mut doc, 0), Vec::<String>::new(), "one undo step");
}

#[test]
fn update_replaces_the_header_and_remove_takes_it_away() {
    let mut doc = open(blank(1, &[]));
    add_header_footer(&mut doc, &[0], &header("First", ""), false, "first").expect("adds");
    let mut later = header("Second", "");
    later.numbering.start = 5;
    later.text[2] = "p[page]".to_owned();
    add_header_footer(&mut doc, &[0], &later, true, "second").expect("updates");
    let mut found = texts(&mut doc, 0);
    found.sort();
    assert_eq!(found, ["Second", "p5"]);
    assert_eq!(
        saved_settings(&mut doc, MarkKind::HeaderFooter).expect("reads"),
        Some("second".to_owned()),
        "Update keeps what it was made with"
    );

    assert_eq!(
        remove_marks(&mut doc, MarkKind::HeaderFooter, &[0]).expect("removes"),
        1
    );
    assert_eq!(texts(&mut doc, 0), Vec::<String>::new());
    assert_eq!(page_marks(&mut doc, 0).expect("reads"), []);
    assert_eq!(
        remove_marks(&mut doc, MarkKind::HeaderFooter, &[0]).expect("runs"),
        0
    );
}

/// A turned page's header is at the top of the page as shown.
#[test]
fn a_turned_page_has_its_header_at_the_top_it_shows() {
    let mut doc = open(blank(1, &[0]));
    let mut large = header("HEADER", "");
    large.style.size = 30.0;
    add_header_footer(&mut doc, &[0], &large, false, "").expect("adds");
    // Shown 792 wide by 612 high: the header in the top 36 to 70 points.
    assert!(inked_rows(&mut doc, 0, 36..70));
    assert!(!inked_rows(&mut doc, 0, 100..612));
}

#[test]
fn bates_numbers_run_on_and_are_removed_apart_from_the_header() {
    let mut doc = open(blank(3, &[]));
    add_header_footer(&mut doc, &[0, 1, 2], &header("Title", ""), false, "").expect("adds");
    let bates = Bates {
        prefix: "ACME".into(),
        suffix: String::new(),
        digits: 6,
        start: 7,
        position: 5,
        style: TextStyle {
            font: Font::Courier,
            ..TextStyle::default()
        },
        margins: Margins {
            top: 36.0,
            bottom: 36.0,
            left: 36.0,
            right: 36.0,
        },
    };
    let (first, last) = add_bates(&mut doc, &[0, 1, 2], &bates, "").expect("numbers");
    assert_eq!(
        (first.as_str(), last.as_str()),
        ("ACME000007", "ACME000009")
    );
    assert!(texts(&mut doc, 1).contains(&"ACME000008".to_owned()));
    // Numbering again replaces, rather than adding a second number.
    add_bates(
        &mut doc,
        &[1],
        &Bates {
            start: 100,
            ..bates
        },
        "",
    )
    .expect("renumbers");
    let mut second = texts(&mut doc, 1);
    second.sort();
    assert_eq!(second, ["ACME000100", "Title"]);

    remove_marks(&mut doc, MarkKind::Bates, &[0, 1, 2]).expect("removes");
    assert_eq!(texts(&mut doc, 0), ["Title"], "the header stays");
}

#[test]
fn a_text_watermark_is_drawn_over_the_middle_at_its_opacity() {
    let mut doc = open(blank(1, &[]));
    let art = Art::Text {
        text: "DRAFT".into(),
        style: TextStyle {
            font: Font::HelveticaBold,
            size: 120.0,
            color: [1.0, 0.0, 0.0],
        },
    };
    let appearance = Appearance {
        opacity: 0.5,
        rotation: 0.0,
        ..Appearance::default()
    };
    add_watermark(&mut doc, &[0], &art, appearance, false, "").expect("adds");
    assert_eq!(texts(&mut doc, 0), ["DRAFT"]);
    let raster = doc.render_page_now(0, 1.0).expect("renders").raster;
    // Half-opaque red on white: pink, somewhere around the middle.
    let pink = (330..460).any(|y| {
        (150..460).any(|x| {
            let at = ((y * raster.width() + x) * 4) as usize;
            let p = &raster.rgba()[at..at + 3];
            p[0] > 240 && (100..160).contains(&p[1]) && (100..160).contains(&p[2])
        })
    });
    assert!(pink, "a half-opaque red letter in the middle");
    assert_eq!(pixel(&mut doc, 0, (10, 10)), [255, 255, 255, 255]);
}

#[test]
fn a_page_watermark_is_placed_scaled_and_can_go_behind() {
    let mut doc = open(blank(1, &[]));
    let art = Art::Page {
        pdf: blue_square(),
        page: 0,
        scale: 0.5,
    };
    let appearance = Appearance {
        horizontal: HAlign::Left,
        vertical: VAlign::Top,
        offset: (10.0, -10.0),
        behind: true,
        ..Appearance::default()
    };
    add_watermark(&mut doc, &[0], &art, appearance, false, "").expect("adds");
    // 50 points square, 10 in from the shown top-left corner.
    let blue = |p: [u8; 4]| p[2] > 200 && p[0] < 60;
    assert!(blue(pixel(&mut doc, 0, (15, 15))));
    assert!(blue(pixel(&mut doc, 0, (55, 55))));
    assert!(!blue(pixel(&mut doc, 0, (65, 65))));
    assert!(!blue(pixel(&mut doc, 0, (5, 5))));
    assert_eq!(
        page_marks(&mut doc, 0).expect("reads"),
        [MarkKind::Watermark]
    );

    let missing = Art::Page {
        pdf: blue_square(),
        page: 4,
        scale: 1.0,
    };
    assert!(matches!(
        add_watermark(&mut doc, &[0], &missing, appearance, true, ""),
        Err(CommandError::Edit {
            label: "Update Watermark",
            source: Error::NoSuchPage { page: 4, .. }
        })
    ));
}

#[test]
fn a_background_colour_fills_behind_what_the_page_draws() {
    let mut doc = open(pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".to_vec(),
        stream("0 g 0 0 20 20 re f"),
    ]));
    let appearance = Appearance {
        behind: false,
        ..Appearance::default()
    };
    add_background(
        &mut doc,
        &[0],
        &Art::Color([0.0, 1.0, 0.0]),
        appearance,
        false,
        "green",
    )
    .expect("adds");
    assert_eq!(
        &pixel(&mut doc, 0, (100, 100))[..3],
        [0, 255, 0],
        "green all over"
    );
    assert_eq!(
        &pixel(&mut doc, 0, (10, 190))[..3],
        [0, 0, 0],
        "the page's own black on top"
    );
    assert_eq!(
        page_marks(&mut doc, 0).expect("reads"),
        [MarkKind::Background]
    );
    add_background(
        &mut doc,
        &[0],
        &Art::Color([0.0, 0.0, 1.0]),
        appearance,
        true,
        "blue",
    )
    .expect("updates");
    assert_eq!(
        &pixel(&mut doc, 0, (100, 100))[..3],
        [0, 0, 255],
        "replaced"
    );
    assert_eq!(
        saved_settings(&mut doc, MarkKind::Background)
            .expect("reads")
            .as_deref(),
        Some("blue")
    );
    remove_marks(&mut doc, MarkKind::Background, &[0]).expect("removes");
    assert_eq!(&pixel(&mut doc, 0, (100, 100))[..3], [255, 255, 255]);
}

#[test]
fn a_protected_document_refuses_every_mark_by_name() {
    let mut doc = Document::open_path(&encrypted_fixture("r4-aes-128.pdf")).expect("opens");
    let refused = add_header_footer(&mut doc, &[0], &header("x", ""), false, "");
    assert!(matches!(
        refused,
        Err(CommandError::Edit {
            label: "Add Header & Footer",
            source: Error::Protected(_)
        })
    ));
    assert!(matches!(
        remove_marks(&mut doc, MarkKind::Watermark, &[0]),
        Err(CommandError::Edit {
            label: "Remove Watermark",
            ..
        })
    ));
    assert!(matches!(
        page_marks(&mut doc, 0),
        Err(CommandError::Page { page: 0, .. })
    ));
    assert!(matches!(
        marked_pages(&mut doc, MarkKind::Bates),
        Err(CommandError::Failed { .. })
    ));
    assert!(saved_settings(&mut doc, MarkKind::Bates).is_err());
}

#[test]
fn bates_numbers_run_on_across_files_written_beside_them() {
    use onionskin_tools_edit::marks::{number_files, Naming};

    let folder = std::env::temp_dir().join(format!("bates-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&folder);
    std::fs::create_dir_all(&folder).expect("a folder");
    let first = folder.join("one.pdf");
    let second = folder.join("two.pdf");
    std::fs::write(&first, blank(2, &[])).expect("writes");
    std::fs::write(&second, blank(3, &[])).expect("writes");
    let bates = Bates {
        prefix: "X".into(),
        suffix: String::new(),
        digits: 3,
        start: 1,
        position: 5,
        style: TextStyle::default(),
        margins: Margins::default(),
    };
    let naming = Naming {
        after: "-numbered".into(),
        numbers: true,
        ..Naming::default()
    };
    let numbered =
        number_files(&[first.clone(), second.clone()], &bates, &naming).expect("numbers");
    assert_eq!(
        numbered[0].output,
        folder.join("one-numbered_X001-X002.pdf")
    );
    assert_eq!(
        (numbered[1].first.as_str(), numbered[1].last.as_str()),
        ("X003", "X005")
    );
    let mut written = Document::open_path(&numbered[1].output).expect("opens");
    assert_eq!(texts(&mut written, 2), ["X005"]);
    assert_eq!(
        std::fs::read(&first).expect("reads"),
        blank(2, &[]),
        "the original is untouched"
    );

    // Again: the copies exist, so nothing is written and the reason is said.
    let again = number_files(&[first.clone(), second], &bates, &naming);
    assert!(
        matches!(again, Err(CommandError::Failed { ref reason, .. }) if reason.contains("already exists"))
    );
    // Two inputs that would land on one name.
    let twice = number_files(&[first.clone(), first.clone()], &bates, &Naming::default());
    assert!(
        matches!(twice, Err(CommandError::Failed { ref reason, .. }) if reason.contains("already exists") || reason.contains("two files"))
    );
    // A file that will not open is named.
    let missing = number_files(&[folder.join("none.pdf")], &bates, &naming);
    assert!(
        matches!(missing, Err(CommandError::Failed { ref reason, .. }) if reason.contains("none.pdf"))
    );
    let _ = std::fs::remove_dir_all(&folder);
}
