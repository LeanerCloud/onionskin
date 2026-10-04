//! P18 on a real window: Save, Save As, Undo, Redo, the dirty mark, the
//! question before closing, and recovery. Every keystroke is pressed, not
//! called, because calling the handler is what missed a dead Ctrl+F twice.

use super::*;
use crate::shell::chrome::file_dialogs::FileAction;
#[cfg(feature = "commands-core")]
use crate::shell::chrome::global_bar::PageCommand;

fn seed(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/seeds")
        .join(name)
}

/// A window over copies of `names` in a fresh directory, so saving writes
/// nothing into the corpus, with recovery kept under the same directory.
fn window_over(
    names: &[&str],
    cx: &mut TestAppContext,
) -> (
    tempfile::TempDir,
    gpui::WindowHandle<ShellFrame>,
    Vec<crate::keymap::Binding>,
) {
    let dir = tempfile::tempdir().expect("dir");
    let models = names
        .iter()
        .map(|name| {
            let path = dir.path().join(name);
            std::fs::copy(seed(name), &path).expect("copies");
            let model = CanvasModel::new(
                Document::open_path(&path).expect("opens"),
                crate::build_registry(),
                ViewSize {
                    width: 800.0,
                    height: 600.0,
                },
            )
            .expect("the model builds");
            (path, model)
        })
        .collect();
    let paths = crate::config::ConfigPaths::in_dir(&dir.path().join("config"));
    let (window, bindings) = bound_window_with_models(models, paths, cx);
    (dir, window, bindings)
}

fn press(
    window: gpui::WindowHandle<ShellFrame>,
    bindings: &[crate::keymap::Binding],
    id: &str,
    cx: &mut TestAppContext,
) {
    cx.simulate_keystrokes(window.into(), &keystroke_for(bindings, id));
    cx.run_until_parked();
}

fn page_count(frame: &ShellFrame, cx: &App) -> usize {
    frame
        .tabs
        .active()
        .expect("a tab")
        .canvas
        .read(cx)
        .model
        .view_state()
        .page_count
}

fn dirty(frame: &ShellFrame, cx: &mut Context<ShellFrame>, window: &mut Window) -> bool {
    let tree = frame.accessible(window, cx);
    let tab = tree
        .find(&tab_element_id(
            frame.tabs.active().expect("a tab").canvas.entity_id(),
        ))
        .expect("the tab is described");
    tab.description.as_deref() == Some("Unsaved changes")
}

#[cfg(feature = "commands-core")]
fn delete_page(frame: &mut ShellFrame, window: &mut Window, cx: &mut Context<ShellFrame>) {
    frame.run_activation(
        Activation::MainMenu(MenuCommand::Page(PageCommand::Delete)),
        window,
        cx,
    );
}

/// T1 end to end, by keystroke: delete a page, save, undo, save. The page
/// is back on the canvas and in the file on disk, and the tab is dirty
/// again once undo goes past the saved mark.
// Edits through Delete Page and Rotate, which `commands-core` registers.
#[cfg(feature = "commands-core")]
#[gpui::test]
fn delete_save_undo_save_puts_the_page_back_everywhere(cx: &mut TestAppContext) {
    let (dir, window, bindings) = window_over(&["two-page.pdf"], cx);
    let path = dir.path().join("two-page.pdf");
    window
        .update(cx, |frame, window, cx| {
            assert!(!dirty(frame, cx, window), "clean at open");
            delete_page(frame, window, cx);
            assert_eq!(page_count(frame, cx), 1);
            assert!(dirty(frame, cx, window), "the first edit marks the tab");
        })
        .unwrap();

    press(window, &bindings, "file.save", cx);
    window
        .update(cx, |frame, window, cx| {
            assert!(!dirty(frame, cx, window), "save clears the mark");
        })
        .unwrap();
    assert_eq!(
        Document::open_path(&path).expect("reopens").page_count(),
        1,
        "the save reached the disk"
    );

    press(window, &bindings, "edit.undo", cx);
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(page_count(frame, cx), 2, "undo put the page back");
            assert!(
                dirty(frame, cx, window),
                "undoing past the saved mark makes the document dirty again"
            );
        })
        .unwrap();

    press(window, &bindings, "file.save", cx);
    assert_eq!(
        Document::open_path(&path).expect("reopens").page_count(),
        2,
        "the second save wrote the page back"
    );
    press(window, &bindings, "edit.redo", cx);
    window
        .update(cx, |frame, _window, cx| {
            assert_eq!(page_count(frame, cx), 1, "redo deletes it again");
        })
        .unwrap();
}

/// Undo leaves what the user sees exactly as it was before the edit: the
/// page rendered after edit-then-undo is the page rendered before it.
// Edits through Delete Page and Rotate, which `commands-core` registers.
#[cfg(feature = "commands-core")]
#[gpui::test]
fn undo_restores_the_rendered_page(cx: &mut TestAppContext) {
    let (_dir, window, bindings) = window_over(&["two-page.pdf"], cx);
    let render = |frame: &ShellFrame, cx: &mut Context<ShellFrame>| {
        let canvas = frame.tabs.active().expect("a tab").canvas.clone();
        canvas.update(cx, |canvas, _| {
            let page = canvas.model.view_state().current_page;
            let raster = canvas
                .model
                .document_mut()
                .render_page_now(page, 1.0)
                .expect("renders")
                .raster;
            (raster.width(), raster.height(), raster.rgba().to_vec())
        })
    };
    let before = window
        .update(cx, |frame, _window, cx| render(frame, cx))
        .unwrap();
    window
        .update(cx, |frame, window, cx| {
            frame.run_activation(
                Activation::MainMenu(MenuCommand::Page(PageCommand::RotateClockwise)),
                window,
                cx,
            );
            assert!(render(frame, cx) != before, "the rotation changed the page");
        })
        .unwrap();
    press(window, &bindings, "edit.undo", cx);
    window
        .update(cx, |frame, _window, cx| {
            assert!(render(frame, cx) == before, "pixel for pixel");
        })
        .unwrap();
}

/// Undo and Redo are in the tree while there is nothing to do, disabled
/// with the reason, rather than absent.
// Edits through Delete Page and Rotate, which `commands-core` registers.
#[cfg(feature = "commands-core")]
#[gpui::test]
fn undo_and_redo_are_disabled_with_a_reason_when_there_is_nothing_to_do(cx: &mut TestAppContext) {
    let (_dir, window, _bindings) = window_over(&["two-page.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            let tree = frame.accessible(window, cx);
            for (id, reason) in [
                ("global-undo", "Nothing to undo"),
                ("global-redo", "Nothing to redo"),
                ("global-save", "No unsaved changes"),
            ] {
                let button = tree.find(&id.into()).expect("the button is described");
                assert!(button.state.disabled, "{id}");
                assert_eq!(button.description.as_deref(), Some(reason), "{id}");
            }
            delete_page(frame, window, cx);
            let tree = frame.accessible(window, cx);
            assert!(!tree.find(&"global-undo".into()).unwrap().state.disabled);
            assert!(!tree.find(&"global-save".into()).unwrap().state.disabled);
        })
        .unwrap();
}

/// Save As writes where the user chose and the tab follows the document
/// there; the original file is untouched.
// Edits through Delete Page and Rotate, which `commands-core` registers.
#[cfg(feature = "commands-core")]
#[gpui::test]
fn save_as_writes_the_chosen_file_and_the_tab_follows_it(cx: &mut TestAppContext) {
    let (dir, window, bindings) = window_over(&["two-page.pdf"], cx);
    let original = std::fs::read(dir.path().join("two-page.pdf")).expect("reads");
    let copy = dir.path().join("copy.pdf");
    window.update(cx, delete_page).unwrap();
    press(window, &bindings, "file.save-as", cx);
    let rejected = dir.path().join("missing").join("rejected.pdf");
    cx.simulate_new_path_selection(move |_| Some(rejected));
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            let tab = frame.tabs.active().expect("active");
            assert_eq!(tab.title(), "two-page.pdf");
            assert_eq!(
                tab.canvas.read(cx).model.path(),
                Some(dir.path().join("two-page.pdf"))
            );
            assert!(dirty(frame, cx, window));
        })
        .unwrap();
    press(window, &bindings, "file.save-as", cx);
    assert!(cx.did_prompt_for_new_path());
    let chosen = copy.clone();
    cx.simulate_new_path_selection(move |_| Some(chosen));
    cx.run_until_parked();

    assert_eq!(
        Document::open_path(&copy)
            .expect("the copy opens")
            .page_count(),
        1
    );
    assert_eq!(
        std::fs::read(dir.path().join("two-page.pdf")).expect("reads"),
        original,
        "the original is untouched"
    );
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.tabs.active().unwrap().title(), "copy.pdf");
            assert!(!dirty(frame, cx, window));
        })
        .unwrap();
}

/// Closing a document with unsaved changes asks first. The answer applies
/// to the document the question was asked about even when another tab has
/// become active meanwhile (B4.2), and Cancel keeps it open.
// Edits through Delete Page and Rotate, which `commands-core` registers.
#[cfg(feature = "commands-core")]
#[gpui::test]
fn closing_an_unsaved_tab_asks_and_the_answer_lands_on_that_tab(cx: &mut TestAppContext) {
    let (_dir, window, bindings) = window_over(&["hello.pdf", "two-page.pdf"], cx);
    window
        .update(cx, |frame, window, cx| {
            frame.activate(1, cx);
            delete_page(frame, window, cx);
        })
        .unwrap();

    press(window, &bindings, "file.close", cx);
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::UnsavedChanges));
            assert_eq!(frame.tabs.tabs().len(), 2, "nothing closed yet");
            frame.run_activation(Activation::File(FileAction::CancelClose), window, cx);
            assert_eq!(frame.dialog, None);
            assert_eq!(frame.tabs.tabs().len(), 2, "Cancel keeps it open");
        })
        .unwrap();

    press(window, &bindings, "file.close", cx);
    window
        .update(cx, |frame, window, cx| {
            // The user switches tabs while the question is up.
            frame.activate(0, cx);
            frame.run_activation(Activation::File(FileAction::DiscardAndClose), window, cx);
            let titles: Vec<_> = frame
                .tabs
                .tabs()
                .iter()
                .map(|tab| tab.title().to_owned())
                .collect();
            assert_eq!(
                titles,
                ["hello.pdf"],
                "the edited document closed, not the active one"
            );
        })
        .unwrap();
}

/// A clean document closes without a question.
#[gpui::test]
fn closing_a_clean_tab_does_not_ask(cx: &mut TestAppContext) {
    let (_dir, window, bindings) = window_over(&["hello.pdf", "two-page.pdf"], cx);
    press(window, &bindings, "file.close", cx);
    window
        .update(cx, |frame, _window, _cx| {
            assert_eq!(frame.dialog, None);
            assert_eq!(frame.tabs.tabs().len(), 1);
        })
        .unwrap();
}

/// A recovery written for document A is offered when A opens again, and
/// not when B does; accepting it makes A dirty with the recovered edit.
#[gpui::test]
fn a_recovery_is_offered_for_its_own_document_only(cx: &mut TestAppContext) {
    let dir = tempfile::tempdir().expect("dir");
    let a = dir.path().join("a.pdf");
    let b = dir.path().join("b.pdf");
    std::fs::copy(seed("two-page.pdf"), &a).expect("copies");
    std::fs::copy(seed("two-page.pdf"), &b).expect("copies");
    let paths = crate::config::ConfigPaths::in_dir(&dir.path().join("config"));
    let store = onionskin_core::RecoveryStore::open(paths.recovery.as_ref().unwrap())
        .expect("the store opens");

    // A session that edited A, autosaved, and never closed.
    let mut file = onionskin_core::DocumentFile::open(&a).expect("opens");
    file.set_recovery(store);
    file.document_mut()
        .edit_pages("Delete Page", |tx, structure| {
            onionskin_core::pages::delete_pages(tx, structure, &[1])
        })
        .expect("deletes");
    file.autosave().expect("autosaves");
    drop(file);

    let (window, _) = bound_window_with_models(Vec::new(), paths, cx);
    window
        .update(cx, |frame, _window, cx| {
            frame.open_documents(std::slice::from_ref(&b), cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, _window, _cx| {
            assert_eq!(frame.dialog, None, "B has no recovery");
        })
        .unwrap();

    window
        .update(cx, |frame, _window, cx| {
            frame.open_documents(std::slice::from_ref(&a), cx)
        })
        .unwrap();
    cx.run_until_parked();
    window
        .update(cx, |frame, window, cx| {
            assert_eq!(frame.dialog, Some(ShellDialog::Recover), "A's is offered");
            frame.run_activation(Activation::File(FileAction::Recover), window, cx);
            assert_eq!(frame.dialog, None);
            assert_eq!(page_count(frame, cx), 1, "the recovered deletion is back");
            assert!(dirty(frame, cx, window), "and unsaved");
        })
        .unwrap();
}
