//! M5 links on a real window: the Link tool's requests open Create Link
//! and Link Properties, the Hand tool's click follows a link, the Trust
//! Manager asks before a web page opens, and the Edit menu's link commands
//! run on every page.

use super::*;
use crate::preferences::WebLinks;
use crate::shell::chrome::accessible::Activation;
use crate::shell::chrome::link_dialog::{LinkAction, TargetKind};
use crate::shell::chrome::web_link_dialog::{WebLinkAction, BLOCKED};
use crate::shell::dialog::ShellDialog;
use crate::shell::panes::{AttachmentAction, NavigationPane, PaneAction};
use onionskin_core::links::{LinkLook, LinkTarget};
use onionskin_core::{LinkRequest, PagePoint, PageRect};

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

/// Two small pages; the first says "Visit https://example.com today".
fn document() -> Vec<u8> {
    let content = "BT /F1 10 Tf 10 50 Td (Visit https://example.com today) Tj ET";
    pdf(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 300 100] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << /Font << /F1 6 0 R >> >> >>"
            .to_vec(),
        b"<< /Type /Page /Parent 2 0 R >>".to_vec(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        )
        .into_bytes(),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
    ])
}

fn window(cx: &mut TestAppContext) -> gpui::WindowHandle<ShellFrame> {
    let window = bound_window_from_bytes(vec![("links.pdf", document())], cx).0;
    window
        .update(cx, |frame, _window, cx| {
            frame.run_view_action(crate::shell::canvas::ViewAction::GoToPage(0), cx);
        })
        .unwrap();
    cx.run_until_parked();
    window
}

/// Raise `request` as a tool would, and let the frame take and run it.
fn request(window: gpui::WindowHandle<ShellFrame>, request: LinkRequest, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                canvas.model.document_mut().request_link(request)
            });
            frame.collect_link_request(cx);
            frame.run_pending_link(window, cx);
        })
        .unwrap();
    cx.run_until_parked();
}

fn act(window: gpui::WindowHandle<ShellFrame>, action: Activation, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(action, window, cx)
        })
        .unwrap();
    cx.run_until_parked();
}

fn links(
    window: gpui::WindowHandle<ShellFrame>,
    cx: &mut TestAppContext,
) -> Vec<onionskin_core::links::Link> {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                canvas.model.document_mut().links().expect("reads")
            })
        })
        .unwrap()
}

fn dialog(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Option<ShellDialog> {
    window.update(cx, |frame, _, _| frame.dialog).unwrap()
}

fn current_page(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> usize {
    window
        .update(cx, |frame, _, cx| {
            frame
                .active_canvas()
                .expect("a tab")
                .read(cx)
                .model
                .viewport()
                .current_page()
        })
        .unwrap()
}

fn notices(window: gpui::WindowHandle<ShellFrame>, cx: &mut TestAppContext) -> Vec<String> {
    window
        .update(cx, |frame, _, _| frame.notices.clone())
        .unwrap()
}

const RECT: PageRect = PageRect {
    page: 0,
    x0: 200.0,
    y0: 10.0,
    x1: 280.0,
    y1: 30.0,
};

const INSIDE: PagePoint = PagePoint {
    page: 0,
    x: 240.0,
    y: 20.0,
};

/// Put a link to `target` over [`RECT`], through the document directly.
fn linked(window: gpui::WindowHandle<ShellFrame>, target: LinkTarget, cx: &mut TestAppContext) {
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                onionskin_tools_edit::links::create_link(
                    &mut canvas.model.document_mut(),
                    0,
                    [RECT.x0, RECT.y0, RECT.x1, RECT.y1],
                    &target,
                    LinkLook::default(),
                )
                .expect("creates");
            });
        })
        .unwrap();
}

#[gpui::test]
fn a_drawn_link_goes_to_the_page_typed_and_a_click_follows_it(cx: &mut TestAppContext) {
    let window = window(cx);
    request(window, LinkRequest::Create(RECT), cx);
    assert_eq!(
        dialog(window, cx),
        Some(ShellDialog::Link { editing: false })
    );
    window
        .update(cx, |frame, _, cx| {
            let input = frame.link_dialog.as_ref().expect("open").page.clone();
            assert_eq!(input.read(cx).query(), "1", "the page on screen");
            input.update(cx, |input, cx| input.set_query("2", cx));
        })
        .unwrap();
    act(window, Activation::Link(LinkAction::Submit), cx);
    assert_eq!(dialog(window, cx), None);
    let made = links(window, cx);
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].target, LinkTarget::Page(1));

    request(window, LinkRequest::Follow(INSIDE), cx);
    assert_eq!(current_page(window, cx), 1, "followed to page 2");
    // A click that is not on a link does nothing.
    request(
        window,
        LinkRequest::Follow(PagePoint {
            page: 0,
            x: 5.0,
            y: 5.0,
        }),
        cx,
    );
    assert_eq!(dialog(window, cx), None);
}

#[gpui::test]
fn a_clicked_link_is_changed_to_a_web_page_that_asks_before_it_opens(cx: &mut TestAppContext) {
    let window = window(cx);
    linked(window, LinkTarget::Page(1), cx);
    let link = links(window, cx)[0].objref;
    request(window, LinkRequest::Edit(link), cx);
    assert_eq!(
        dialog(window, cx),
        Some(ShellDialog::Link { editing: true })
    );
    act(
        window,
        Activation::Link(LinkAction::SetKind(TargetKind::Web)),
        cx,
    );
    window
        .update(cx, |frame, _, cx| {
            let input = frame.link_dialog.as_ref().expect("open").url.clone();
            input.update(cx, |input, cx| input.set_query("example.com/docs", cx));
        })
        .unwrap();
    act(window, Activation::Link(LinkAction::Visible), cx);
    act(window, Activation::Link(LinkAction::Submit), cx);
    let changed = &links(window, cx)[0];
    assert_eq!(
        changed.target,
        LinkTarget::Web("http://example.com/docs".into())
    );
    assert!(changed.look.visible);

    request(window, LinkRequest::Follow(INSIDE), cx);
    assert_eq!(dialog(window, cx), Some(ShellDialog::WebLink));
    act(window, Activation::WebLink(WebLinkAction::Cancel), cx);
    assert_eq!(cx.opened_url(), None, "cancelled");

    request(window, LinkRequest::Follow(INSIDE), cx);
    act(window, Activation::WebLink(WebLinkAction::AlwaysAllow), cx);
    assert_eq!(cx.opened_url().as_deref(), Some("http://example.com/docs"));
    let trusted = window
        .update(cx, |frame, _, _| {
            frame.settings.preferences.trusted_sites.clone()
        })
        .unwrap();
    assert!(trusted.contains("example.com"));
    // Now it opens without asking.
    request(window, LinkRequest::Follow(INSIDE), cx);
    assert_eq!(dialog(window, cx), None);
    assert!(notices(window, cx).contains(&"Opened http://example.com/docs".to_owned()));

    // Trust Manager lists the site, and forgets it.
    act(
        window,
        Activation::ChangePreference(
            crate::shell::preferences_dialog::PreferenceChange::KeepSite(0),
        ),
        cx,
    );
    act(
        window,
        Activation::ChangePreference(
            crate::shell::preferences_dialog::PreferenceChange::ForgetSite(0),
        ),
        cx,
    );
    let trusted = window
        .update(cx, |frame, _, _| {
            frame.settings.preferences.trusted_sites.clone()
        })
        .unwrap();
    assert!(trusted.is_empty());
    act(
        window,
        Activation::ChangePreference(
            crate::shell::preferences_dialog::PreferenceChange::WebLinks(WebLinks::Allow),
        ),
        cx,
    );

    // Never means never, trusted or not.
    window
        .update(cx, |frame, _, _| {
            frame.settings.preferences.web_links = WebLinks::Block
        })
        .unwrap();
    request(window, LinkRequest::Follow(INSIDE), cx);
    assert!(notices(window, cx).contains(&BLOCKED.to_owned()));

    // And the link can be deleted from its properties.
    request(window, LinkRequest::Edit(link), cx);
    act(window, Activation::Link(LinkAction::Delete), cx);
    assert!(links(window, cx).is_empty());
}

#[gpui::test]
fn file_and_other_links_say_what_they_cannot_do(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let retained_dir = dir.keep();
    let path = retained_dir.join("links.pdf");
    let expected_path = path.clone();
    std::fs::write(&path, document()).expect("writes");
    let model = CanvasModel::new(
        Document::open_path(&path).expect("opens"),
        crate::build_registry(),
        ViewSize {
            width: 800.0,
            height: 600.0,
        },
    )
    .expect("builds");
    let window = bound_window_with_models(
        vec![(path, model)],
        crate::config::ConfigPaths::default(),
        cx,
    )
    .0;
    window
        .update(cx, |frame, _window, cx| {
            assert_eq!(
                frame.active_canvas().expect("a tab").read(cx).model.path(),
                Some(expected_path)
            );
        })
        .unwrap();
    linked(window, LinkTarget::File("notes.txt".into()), cx);
    request(window, LinkRequest::Follow(INSIDE), cx);
    assert!(notices(window, cx)
        .iter()
        .any(|n| n.contains("PDF files only")));
    let link = links(window, cx)[0].objref;
    window
        .update(cx, |frame, _, cx| {
            let canvas = frame.active_canvas().expect("a tab").clone();
            canvas.update(cx, |canvas, _| {
                onionskin_tools_edit::links::edit_link(
                    &mut canvas.model.document_mut(),
                    link,
                    &LinkTarget::File("missing.pdf".into()),
                    LinkLook::default(),
                )
                .expect("edits");
            });
        })
        .unwrap();
    request(window, LinkRequest::Follow(INSIDE), cx);
    assert!(notices(window, cx)
        .iter()
        .any(|n| n.contains("which is not there")));

    // A bad page number is said in the dialog.
    request(window, LinkRequest::Create(RECT), cx);
    window
        .update(cx, |frame, _, cx| {
            let input = frame.link_dialog.as_ref().expect("open").page.clone();
            input.update(cx, |input, cx| input.set_query("9", cx));
        })
        .unwrap();
    act(window, Activation::Link(LinkAction::Submit), cx);
    let error = window
        .update(cx, |frame, _, _| {
            frame.link_dialog_state().and_then(|s| s.error.clone())
        })
        .unwrap();
    assert!(error.expect("said").contains("not a page"));
}

#[gpui::test]
fn pathless_attachment_refuses_relative_links_but_opens_absolute_pdfs(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let absolute = dir.path().join("linked.pdf");
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/seeds/hello.pdf"),
        &absolute,
    )
    .expect("copies");
    let window = bound_window_from_bytes(
        vec![("parent.pdf", crate::shell::fixtures::attached_pdf_pdf())],
        cx,
    )
    .0;
    window
        .update(cx, |frame, window, cx| {
            frame.run_pane_action(PaneAction::Select(NavigationPane::Attachments), cx);
            frame.run_activation(
                Activation::Pane(PaneAction::Attachment(AttachmentAction::Open(0))),
                window,
                cx,
            );
            let before = frame.tabs.tabs().len();
            frame.follow(LinkTarget::File("relative.pdf".into()), window, cx);
            assert_eq!(frame.tabs.tabs().len(), before);
            assert!(frame
                .notices
                .last()
                .is_some_and(|notice| notice.contains("Save this document")));
            frame.follow(
                LinkTarget::File(absolute.to_string_lossy().into_owned()),
                window,
                cx,
            );
        })
        .unwrap();
    let count = window
        .update(cx, |frame, _, _| frame.tabs.tabs().len())
        .unwrap();
    assert_eq!(count, 3);
}

#[gpui::test]
fn the_edit_menu_makes_links_from_urls_and_removes_them(cx: &mut TestAppContext) {
    let window = window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::WebLinks { remove: false }, window, cx)
                .expect("runs");
        })
        .unwrap();
    assert_eq!(
        links(window, cx)[0].target,
        LinkTarget::Web("https://example.com".into())
    );
    assert!(notices(window, cx).contains(&"Created 1 web link.".to_owned()));
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::WebLinks { remove: true }, window, cx)
                .expect("runs");
        })
        .unwrap();
    assert!(links(window, cx).is_empty());
    assert!(notices(window, cx).contains(&"Removed 1 web link.".to_owned()));
}

#[gpui::test]
fn create_link_from_the_context_menu_links_the_selected_text(cx: &mut TestAppContext) {
    let window = window(cx);
    window
        .update(cx, |frame, window, cx| {
            frame
                .run_main_menu_command(MenuCommand::SelectAll, window, cx)
                .expect("selects");
            frame.run_canvas_context_command(
                crate::shell::context_menu::CanvasContextCommand::CreateLink,
                window,
                cx,
            );
            assert_eq!(frame.dialog, Some(ShellDialog::Link { editing: false }));
            let mode = frame.link_dialog.as_ref().expect("open").mode;
            let crate::shell::chrome::link_dialog::LinkMode::Create(rect) = mode else {
                panic!("a new link");
            };
            assert!(
                rect.x0 >= 9.0 && rect.x1 > 120.0 && rect.y0 > 40.0,
                "{rect:?}"
            );
        })
        .unwrap();
}
