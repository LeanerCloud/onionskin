//! Stamps and Attach File, through the real gesture lifecycle on a real
//! document.
//!
//! The dynamic stamp is asserted against a clock the test controls. The
//! mutation this must catch is a dynamic stamp that ignores the clock - a
//! stamp that says the same thing at every instant is a static one.

use std::path::{Path, PathBuf};
use std::process::Command;

use onionskin_core::images::{image_document, ImageColor, ImageData, ImagePage};
use onionskin_core::{
    read_annotations, Document, FitMode, Modifiers, PagePoint, ViewSize, Viewport,
};
use onionskin_corpus_testing::{encrypted_fixture, seed};
use onionskin_cos::Object;
use onionskin_plugin_api::{PointerInput, ToolCtx, ToolEnvironment, ToolPlugin};
use onionskin_tools_comment::{AttachFileTool, LibraryError, StampLibrary, StampTool};

// 2026-09-21 14:05 UTC, and a day and some hours later.
const MONDAY: i64 = 1_789_999_500;
const TUESDAY: i64 = MONDAY + 86_400 + 3 * 3600 + 17 * 60;

struct Fixture {
    doc: Document,
    viewport: Viewport,
}

impl Fixture {
    fn blank() -> Self {
        let mut doc = Document::open_bytes(blank_page()).expect("opens");
        let mut viewport = Viewport::new(
            1,
            ViewSize {
                width: 800.0,
                height: 600.0,
            },
            12.0,
        )
        .expect("viewport");
        let geometry = doc.page_geometry(0).expect("measures").clone();
        viewport.measure_page(geometry).expect("measurable");
        viewport.fit(FitMode::Page).expect("fits");
        Fixture { doc, viewport }
    }

    fn click(&mut self, tool: &mut dyn ToolPlugin, (x, y): (f64, f64)) {
        let mut ctx = ToolCtx {
            doc: &mut self.doc,
            viewport: &mut self.viewport,
        };
        let input = PointerInput {
            at: PagePoint { page: 0, x, y },
            pressure: 1.0,
            modifiers: Modifiers::default(),
            clicks: 1,
        };
        tool.on_pointer_down(&mut ctx, input);
        tool.on_pointer_up(&mut ctx, input);
    }

    fn annotations(&mut self) -> Vec<onionskin_core::ReadAnnotation> {
        let count = self.doc.page_count();
        let current = self.doc.structure().expect("the document");
        read_annotations(current, count, &Default::default()).expect("reads")
    }

    /// The last annotation's normal appearance, decoded.
    fn last_appearance(&mut self) -> Vec<u8> {
        let current = self.doc.structure().expect("the document");
        let page = current.page(0).expect("page");
        let Ok(Object::Array(annots)) = current.resolve(page.dict.get(b"Annots").expect("annots"))
        else {
            panic!("an /Annots array");
        };
        let Ok(Object::Dict(annotation)) = current.resolve(annots.last().expect("one")) else {
            panic!("an annotation");
        };
        let Some(Object::Dict(appearances)) = annotation.get(b"AP") else {
            panic!("an /AP");
        };
        let Ok(Object::Stream(stream)) = current.resolve(appearances.get(b"N").expect("/N")) else {
            panic!("an appearance stream");
        };
        current.decode_stream(&stream).expect("decodes")
    }

    fn pixels(&mut self) -> Vec<u8> {
        self.doc
            .render_page_now(0, 1.0)
            .expect("renders")
            .raster
            .rgba()
            .to_vec()
    }
}

fn blank_page() -> Vec<u8> {
    let objects: &[&[u8]] = &[
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>",
    ];
    let mut out: Vec<u8> = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (index, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn tool_at(clock: i64, author: Option<&str>, data_dir: Option<&Path>) -> StampTool {
    let mut tool = StampTool::with_clock(Box::new(move || clock));
    tool.configure(&ToolEnvironment {
        author: author.map(str::to_owned),
        data_dir: data_dir.map(Path::to_path_buf),
    });
    tool
}

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every built-in stamp is the script's output: an SVG or a catalog entry
/// edited by hand no longer matches a fresh generation, and this fails.
#[test]
fn the_committed_stamps_are_what_the_generator_makes() {
    let python = ["python3", "python"]
        .into_iter()
        .find(|candidate| Command::new(candidate).arg("--version").output().is_ok())
        .expect("a Python 3 interpreter");
    let checked = Command::new(python)
        .arg(repository().join("tools/stamps.py"))
        .arg("--check")
        .output()
        .expect("the generator runs");
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stdout)
    );
}

#[test]
fn every_builtin_stamp_places_and_draws_where_it_was_clicked() {
    let mut fixture = Fixture::blank();
    let blank = fixture.pixels();
    let mut tool = tool_at(MONDAY, Some("Ana"), None);
    let choices = tool.choices();
    assert_eq!(
        choices.len(),
        22,
        "twelve business, five sign here, five dynamic"
    );
    for (index, choice) in choices.iter().enumerate() {
        assert!(tool.choose(&choice.id), "{}", choice.id);
        let y = 760.0 - 34.0 * index as f64;
        fixture.click(&mut tool, (306.0, y));
    }
    let annotations = fixture.annotations();
    assert_eq!(annotations.len(), choices.len());
    for (annotation, choice) in annotations.iter().zip(&choices) {
        assert_eq!(annotation.raw_subtype, "Stamp");
        assert_eq!(annotation.contents.as_deref(), Some(choice.label.as_str()));
        assert_eq!(annotation.author.as_deref(), Some("Ana"));
        let centre = (
            (annotation.rect.x0 + annotation.rect.x1) / 2.0,
            (annotation.rect.y0 + annotation.rect.y1) / 2.0,
        );
        assert!((centre.0 - 306.0).abs() < 1e-6, "centred on the click");
    }
    assert_ne!(fixture.pixels(), blank, "the stamps are on the page");
    assert_eq!(fixture.doc.edit().history().reach(), choices.len());
}

#[test]
fn a_dynamic_stamp_says_who_and_the_injected_clocks_time() {
    let mut on_monday = Fixture::blank();
    let mut tool = tool_at(MONDAY, Some("José"), None);
    assert!(tool.choose("dynamic-approved"));
    on_monday.click(&mut tool, (300.0, 400.0));
    let monday = on_monday.last_appearance();
    let text = String::from_utf8_lossy(&monday);
    assert!(
        text.contains("(Jos\\351, 2026-09-21 14:05 UTC)"),
        "the name, in WinAnsi, and the clock's date: {text}"
    );

    let mut on_tuesday = Fixture::blank();
    let mut tool = tool_at(TUESDAY, Some("José"), None);
    assert!(tool.choose("dynamic-approved"));
    on_tuesday.click(&mut tool, (300.0, 400.0));
    let tuesday = String::from_utf8_lossy(&on_tuesday.last_appearance()).into_owned();
    assert!(tuesday.contains("2026-09-22 17:22 UTC"), "{tuesday}");
    assert_ne!(
        on_monday.pixels(),
        on_tuesday.pixels(),
        "a different instant draws a different stamp"
    );
}

#[test]
fn with_no_name_chosen_a_dynamic_stamp_carries_only_the_time_and_no_author() {
    let mut fixture = Fixture::blank();
    let mut tool = tool_at(MONDAY, None, None);
    assert!(tool.choose("dynamic-reviewed"));
    fixture.click(&mut tool, (300.0, 400.0));
    let text = String::from_utf8_lossy(&fixture.last_appearance()).into_owned();
    assert!(text.contains("(2026-09-21 14:05 UTC)"), "{text}");
    assert_eq!(fixture.annotations()[0].author, None);
}

fn green_page() -> Vec<u8> {
    image_document(&ImagePage {
        width: 60,
        height: 30,
        dpi: (72.0, 72.0),
        color: ImageColor::Rgb,
        data: ImageData::Samples([20u8, 180, 40].repeat(60 * 30)),
        alpha: None,
        inverted_cmyk: false,
        icc: None,
    })
    .expect("writes")
}

#[test]
fn custom_stamps_come_from_a_pdf_page_or_an_image_and_can_be_removed() {
    let data = tempfile::tempdir().expect("dir");
    let library = StampLibrary::new(data.path().join("stamps"));
    let from_image = library
        .add("Mine", "Checked", &green_page(), 0, false)
        .expect("adds");
    let two_page = std::fs::read(seed("two-page.pdf")).expect("reads");
    let from_page = library
        .add("Mine", "Second Page", &two_page, 1, false)
        .expect("adds");
    assert!(matches!(
        library.add("Mine", "Checked", &green_page(), 0, false),
        Err(LibraryError::Exists(_))
    ));

    let mut tool = tool_at(MONDAY, None, Some(data.path()));
    let ids: Vec<String> = tool.choices().into_iter().map(|choice| choice.id).collect();
    assert!(ids.contains(&from_image.id()) && ids.contains(&from_page.id()));

    let mut fixture = Fixture::blank();
    assert!(tool.choose(&from_image.id()));
    fixture.click(&mut tool, (300.0, 400.0));
    let stamp = &fixture.annotations()[0];
    assert_eq!(stamp.contents.as_deref(), Some("Checked"));
    assert!(
        (stamp.rect.x1 - stamp.rect.x0 - 60.0).abs() < 1e-6,
        "placed at its own size"
    );
    let render = fixture.doc.render_page_now(0, 1.0).expect("renders");
    let (width, height) = (
        render.raster.width() as usize,
        render.raster.height() as usize,
    );
    // The stamp's centre, (300, 400) in page space.
    let at = ((height - 400) * width + 300) * 4;
    let pixel = &render.raster.rgba()[at..at + 3];
    assert!(
        pixel[1] > 150 && pixel[0] < 60,
        "the image's green: {pixel:?}"
    );

    assert!(tool.choose(&from_page.id()));
    fixture.click(&mut tool, (300.0, 200.0));
    assert_eq!(fixture.annotations().len(), 2);

    library.remove(&from_image).expect("removes");
    assert!(
        !tool.choose(&from_image.id()),
        "a removed stamp is no longer a choice"
    );
    library.remove(&from_page).expect("removes");
    assert!(
        !data.path().join("stamps/Mine").exists(),
        "the empty category goes too"
    );
    assert_eq!(
        tool.choices().len(),
        22,
        "and every built-in is still there"
    );
}

#[test]
fn a_custom_stamp_from_an_encrypted_pdf_is_refused() {
    let data = tempfile::tempdir().expect("dir");
    let library = StampLibrary::new(data.path());
    let encrypted = std::fs::read(encrypted_fixture("r4-aes-128.pdf")).expect("reads");
    assert!(matches!(
        library.add("Mine", "Secret", &encrypted, 0, false),
        Err(LibraryError::Source(onionskin_core::Error::Protected(_)))
    ));
    assert!(library.list().is_empty());
}

#[test]
fn a_file_chosen_for_attach_goes_into_the_document_where_it_was_clicked() {
    let data = tempfile::tempdir().expect("dir");
    let file = data.path().join("minutes.txt");
    let bytes: Vec<u8> = b"minutes of the meeting\n".repeat(50);
    std::fs::write(&file, &bytes).expect("writes");

    let mut tool = AttachFileTool::new();
    tool.configure(&ToolEnvironment {
        author: Some("Ana".into()),
        data_dir: None,
    });
    let mut fixture = Fixture::blank();
    fixture.click(&mut tool, (100.0, 700.0));
    assert!(
        fixture.annotations().is_empty(),
        "no file chosen, nothing attached"
    );

    assert!(tool.choose(file.to_str().expect("utf-8")));
    fixture.click(&mut tool, (100.0, 700.0));
    let annotations = fixture.annotations();
    assert_eq!(annotations.len(), 1);
    assert_eq!(annotations[0].raw_subtype, "FileAttachment");
    assert_eq!(annotations[0].author.as_deref(), Some("Ana"));
    assert_eq!(annotations[0].rect.x0, 100.0);
    assert_eq!(annotations[0].rect.y1, 700.0, "hanging from the click");

    let listed = fixture.doc.attachments().expect("lists").to_vec();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "minutes.txt");
    assert_eq!(listed[0].mime.as_deref(), Some("text/plain"));
    assert_eq!(listed[0].page, Some(0));
    assert_eq!(fixture.doc.attachment_bytes(0).expect("reads"), bytes);
}
