//! The shell's modal surfaces: Preferences, About, Zoom To, and the local
//! keyboard shortcut reference.
//!
//! One host for all four. A dialog is a panel over a backdrop that takes
//! the click that dismisses it, which is the same shape the menus already
//! use; what differs is only the body.
//!
//! The shortcut reference is generated from the keymap in force rather than
//! written out, so a rebound command reads correctly and an unbound one is
//! absent instead of lying. That is what makes it worth shipping as the Help
//! menu's local reference (parity row 112: online help is out of scope, a
//! local reference is not).

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::canvas::ViewAction;
use super::chrome::accessible::{Activation, Element, Rects, Surface};
use super::chrome::{ShellFrame, ThemeTokens};
use crate::keymap::{platform_keystroke, Binding};
use crate::preferences::PreferenceCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum ShellDialog {
    Preferences(PreferenceCategory),
    Export,
    About,
    KeyboardShortcuts,
    ZoomTo,
    Combine(super::chrome::combine_dialog::CombineEntryPoint),
    Split,
    Stamps,
    Summary,
    Properties,
    LayerProperties,
    BookmarkTitle,
    UnsavedChanges,
    Recover,
}

impl ShellDialog {
    pub(in crate::shell) fn title(self) -> &'static str {
        match self {
            Self::Combine(entry_point) => entry_point.title(),
            Self::Split => "Split Document",
            Self::Stamps => "Stamps",
            Self::Summary => "Summarize Comments",
            Self::Properties => "Document Properties",
            Self::LayerProperties => "Layer Properties",
            Self::BookmarkTitle => "Bookmark Title",
            Self::UnsavedChanges => "Unsaved Changes",
            Self::Recover => "Recover Unsaved Changes",
            Self::Preferences(_) => "Preferences",
            Self::Export => "Export",
            Self::About => "About Onionskin",
            Self::KeyboardShortcuts => "Keyboard Shortcuts",
            Self::ZoomTo => "Zoom To",
        }
    }
}

/// The magnifications Acrobat's Zoom To dialog offers, as percentages.
///
/// A chosen magnification goes through the same clamp every other zoom does,
/// so a page too large to rasterize at 3200% lands at the largest scale it
/// can be drawn at rather than being refused.
pub(in crate::shell) const MAGNIFICATIONS: [u32; 12] =
    [25, 50, 75, 100, 125, 150, 200, 400, 800, 1600, 2400, 3200];

/// One row per magnification, as the dialog prints them and as it describes
/// them: the label and the view action are built together so a screen reader
/// cannot be offered a magnification a click would not apply.
fn magnification_rows() -> impl Iterator<Item = (String, ViewAction)> {
    MAGNIFICATIONS.into_iter().map(|percent| {
        (
            format!("{percent}%"),
            ViewAction::ZoomTo(percent as f32 / 100.0),
        )
    })
}

/// What the About panel says. Kept as data so a test can assert the version
/// is the crate's own rather than a string somebody typed.
pub(in crate::shell) fn about_lines() -> Vec<String> {
    vec![
        format!("Onionskin {}", env!("CARGO_PKG_VERSION")),
        "A local, private, non-destructive PDF editor.".to_owned(),
        "Nothing leaves this machine.".to_owned(),
    ]
}

/// One row per keystroke in force: what it runs, and how to type it here.
///
/// `label` comes from the caller because the menus own what a command is
/// called; this only decides the order and the platform spelling.
pub(in crate::shell) fn shortcut_rows(
    bindings: &[Binding],
    label: impl Fn(&str) -> String,
) -> Vec<(String, String)> {
    let macos = cfg!(target_os = "macos");
    bindings
        .iter()
        .map(|binding| {
            (
                label(binding.id),
                platform_keystroke(&binding.keystroke, macos),
            )
        })
        .collect()
}

/// What a dialog tells a screen reader.
///
/// A dialog is modal, and the shell publishes it as the only reachable
/// subtree, so everything a user needs to hear is in here: nothing outside it
/// will be read while it is up.
pub(in crate::shell) fn accessible(
    frame: &ShellFrame,
    dialog: ShellDialog,
    rects: &Rects,
    cx: &gpui::App,
) -> Element {
    let body = match dialog {
        ShellDialog::Preferences(category) => vec![super::preferences_dialog::accessible(
            frame.preferences(),
            category,
        )],
        ShellDialog::About => {
            row_labels(about_lines().into_iter().map(|line| (line, String::new())))
        }
        ShellDialog::Export => super::chrome::export_dialog::accessible(
            frame.export_dialog().expect("export dialog has state"),
            rects,
            cx,
        ),
        ShellDialog::Combine(_) => super::chrome::combine_dialog::accessible(
            frame.combine_dialog().expect("combine dialog has state"),
            rects,
            cx,
        ),
        ShellDialog::Split => super::chrome::split_dialog::accessible(
            frame.split_dialog().expect("split dialog has state"),
            rects,
            cx,
        ),
        ShellDialog::Stamps => {
            super::chrome::stamps_dialog::accessible(&frame.stamps_rows(cx), rects)
        }
        ShellDialog::Summary => super::chrome::summary_dialog::accessible(
            frame.summary_dialog().expect("summary dialog has state"),
            rects,
        ),
        ShellDialog::Properties => super::chrome::properties_dialog::accessible(
            frame
                .properties_dialog()
                .expect("properties dialog has state"),
            cx,
        ),
        ShellDialog::LayerProperties => row_labels(frame.layer_property_rows()),
        ShellDialog::UnsavedChanges => super::chrome::file_dialogs::accessible_unsaved(
            frame.unsaved_dialog().expect("unsaved dialog has state"),
        ),
        ShellDialog::Recover => super::chrome::file_dialogs::accessible_recover(
            frame.recover_dialog().expect("recover dialog has state"),
        ),
        ShellDialog::BookmarkTitle => super::chrome::bookmark_dialog::accessible(
            frame
                .bookmark_title_dialog()
                .expect("bookmark dialog has state"),
            cx,
        ),
        ShellDialog::KeyboardShortcuts => row_labels(frame.shortcut_rows(cx)),
        ShellDialog::ZoomTo => magnification_rows()
            .enumerate()
            .map(|(index, (label, action))| {
                Element::new(("zoom-to-magnification", index), Role::Button, label)
                    .with_activation(Activation::View(action))
            })
            .collect(),
    };

    let mut close = Element::new("dialog-close", Role::Button, "Close")
        .with_activation(Activation::CloseDialog);
    if dialog == ShellDialog::Export {
        close.bounds = rects.of(Surface::DialogHeader).get(1).copied();
    }
    let mut described = Element::new("dialog", Role::Dialog, dialog.title()).child(close);
    for row in body {
        described = described.child(row);
    }
    described
}

/// One node per printed row, with both columns in the name: a shortcut whose
/// keystroke is announced separately from the command it runs is two facts a
/// screen reader user has to pair up themselves.
fn row_labels(rows: impl IntoIterator<Item = (String, String)>) -> Vec<Element> {
    rows.into_iter()
        .enumerate()
        .map(|(index, (left, right))| {
            let label = if right.is_empty() {
                left
            } else {
                format!("{left}: {right}")
            };
            Element::new(("dialog-row", index), Role::Label, label)
        })
        .collect()
}

pub(in crate::shell) fn render_dialog(
    frame: &ShellFrame,
    dialog: ShellDialog,
    rects: Rects,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let header_rects = rects.clone();
    let focused = frame.dialog_focus();
    let body = match dialog {
        ShellDialog::Preferences(category) => {
            super::preferences_dialog::render_preferences(frame, category, theme, cx)
                .into_any_element()
        }
        ShellDialog::About => rows(
            about_lines().into_iter().map(|line| (line, String::new())),
            rects,
        )
        .text_color(theme.text)
        .into_any_element(),
        ShellDialog::Export => super::chrome::export_dialog::render(
            frame.export_dialog().expect("export dialog has state"),
            rects,
            focused,
            theme,
            cx,
        )
        .into_any_element(),
        ShellDialog::Combine(_) => super::chrome::combine_dialog::render(
            frame.combine_dialog().expect("combine dialog has state"),
            rects,
            focused,
            theme,
            cx,
        )
        .into_any_element(),
        ShellDialog::Split => super::chrome::split_dialog::render(
            frame.split_dialog().expect("split dialog has state"),
            rects,
            focused,
            theme,
            cx,
        )
        .into_any_element(),
        ShellDialog::Summary => super::chrome::summary_dialog::render(
            frame.summary_dialog().expect("summary dialog has state"),
            rects,
            focused,
            theme,
            cx,
        )
        .into_any_element(),
        ShellDialog::Stamps => {
            let rows = frame.stamps_rows(cx);
            super::chrome::stamps_dialog::render(&rows, rects, focused, theme, cx)
                .into_any_element()
        }
        ShellDialog::Properties => super::chrome::properties_dialog::render(
            frame
                .properties_dialog()
                .expect("properties dialog has state"),
            focused,
            theme,
            cx,
        )
        .into_any_element(),
        ShellDialog::UnsavedChanges => super::chrome::file_dialogs::render_unsaved(
            frame.unsaved_dialog().expect("unsaved dialog has state"),
            focused,
            theme,
            cx,
        )
        .into_any_element(),
        ShellDialog::Recover => super::chrome::file_dialogs::render_recover(
            frame.recover_dialog().expect("recover dialog has state"),
            focused,
            theme,
            cx,
        )
        .into_any_element(),
        ShellDialog::BookmarkTitle => super::chrome::bookmark_dialog::render(
            frame
                .bookmark_title_dialog()
                .expect("bookmark dialog has state"),
            focused,
            theme,
            cx,
        )
        .into_any_element(),
        ShellDialog::LayerProperties => rows(frame.layer_property_rows(), rects)
            .text_color(theme.text)
            .into_any_element(),
        ShellDialog::KeyboardShortcuts => rows(frame.shortcut_rows(cx), rects)
            .text_color(theme.text)
            .into_any_element(),
        ShellDialog::ZoomTo => render_magnifications(rects, theme, cx).into_any_element(),
    };

    div()
        .id("dialog-layer")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .occlude()
        .on_click(cx.listener(|frame, _event, window, cx| frame.close_dialog(window, cx)))
        .child(
            div()
                .id("dialog")
                .w(px(560.0))
                .max_h(px(520.0))
                .flex()
                .flex_col()
                .p_4()
                .rounded_md()
                .bg(theme.raised)
                .text_color(theme.text)
                // The panel keeps its own clicks: the backdrop above closes.
                .occlude()
                .child(
                    div()
                        .on_children_prepainted(move |bounds, window, _cx| {
                            if dialog == ShellDialog::Export {
                                header_rects.record(Surface::DialogHeader, &bounds, window);
                            }
                        })
                        .flex()
                        .items_center()
                        .justify_between()
                        .pb_2()
                        .child(div().text_lg().child(dialog.title()))
                        .child(
                            div()
                                .id("dialog-close")
                                .px_2()
                                .cursor_pointer()
                                .rounded_sm()
                                .when(
                                    dialog == ShellDialog::Export
                                        && focused == Some(&"dialog-close".into()),
                                    |button| button.bg(theme.selected),
                                )
                                .hover(move |button| button.bg(theme.subtle_hover))
                                .on_click(cx.listener(|frame, _event, window, cx| {
                                    frame.run_activation(Activation::CloseDialog, window, cx);
                                }))
                                .child("Close"),
                        ),
                )
                .child(
                    div()
                        .id("dialog-body")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(body),
                ),
        )
}

/// The Zoom To body: one row per magnification, each applying it.
fn render_magnifications(
    rects: Rects,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::Div {
    let mut list =
        div()
            .flex()
            .flex_col()
            .gap_1()
            .on_children_prepainted(move |bounds, window, _cx| {
                rects.record(Surface::Dialog, &bounds, window);
            });
    for (index, (label, action)) in magnification_rows().enumerate() {
        list = list.child(
            div()
                .id(("zoom-to-magnification", index))
                .py_1()
                .px_2()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |row| row.bg(theme.selected))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(Activation::View(action), window, cx);
                }))
                .child(label),
        );
    }
    list
}

/// The printed rows of About and the shortcut reference.
///
/// The list is what the description reads as the dialog's body, so it is what
/// reports the rectangles: the panel around it holds the title and the close
/// button too, which are described separately.
fn rows(rows: impl IntoIterator<Item = (String, String)>, rects: Rects) -> gpui::Div {
    let mut list =
        div()
            .flex()
            .flex_col()
            .gap_1()
            .on_children_prepainted(move |bounds, window, _cx| {
                rects.record(Surface::Dialog, &bounds, window);
            });
    for (left, right) in rows {
        list = list.child(
            div()
                .flex()
                .justify_between()
                .gap_4()
                .py_1()
                .child(div().child(left))
                .child(div().flex_none().child(right)),
        );
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The printed label and the applied zoom come from one place, so a
    /// magnification cannot say one thing and do another, and every one of
    /// them is a zoom the viewport will take.
    #[test]
    fn every_magnification_row_applies_the_zoom_it_prints() {
        let rows: Vec<_> = magnification_rows().collect();

        assert_eq!(rows.len(), MAGNIFICATIONS.len());
        for (percent, (label, action)) in MAGNIFICATIONS.into_iter().zip(rows) {
            assert_eq!(label, format!("{percent}%"));
            let ViewAction::ZoomTo(zoom) = action else {
                panic!("{label} is not a magnification: {action:?}");
            };
            assert!((zoom * 100.0 - percent as f32).abs() < 1e-3, "{label}");
            assert!(
                (onionskin_core::MIN_ZOOM..=onionskin_core::MAX_ZOOM).contains(&zoom),
                "{label} is outside the zoom range the viewport offers"
            );
        }
        assert!(rows_contain(1.0), "actual size is one of the choices");
    }

    fn rows_contain(zoom: f32) -> bool {
        magnification_rows().any(|(_, action)| action == ViewAction::ZoomTo(zoom))
    }

    #[test]
    fn about_names_the_build_rather_than_a_typed_version() {
        let lines = about_lines();

        assert_eq!(lines[0], format!("Onionskin {}", env!("CARGO_PKG_VERSION")));
        assert!(lines.iter().any(|line| line.contains("local")));
    }

    /// The reference is the keymap, so a rebound command reads as the user
    /// rebound it. A written-out list would still say cmd-o here.
    #[test]
    fn the_shortcut_reference_reads_the_keymap_in_force() {
        let bindings = vec![Binding {
            id: "file.open",
            keystroke: "cmd-shift-o".to_owned(),
        }];

        let rows = shortcut_rows(&bindings, |id| format!("<{id}>"));

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "<file.open>");
        assert_eq!(
            rows[0].1,
            platform_keystroke("cmd-shift-o", cfg!(target_os = "macos"))
        );
    }

    /// A shortcut is two printed columns. Announcing them as two nodes leaves
    /// the reader to pair a command with the keystroke beside it, so the row
    /// carries both, and a row with nothing in its right column carries one.
    #[test]
    fn a_described_row_announces_both_of_its_printed_columns() {
        let shortcuts = row_labels(vec![
            ("Open".to_owned(), "cmd-o".to_owned()),
            ("Close".to_owned(), "cmd-w".to_owned()),
        ]);
        let about = row_labels(about_lines().into_iter().map(|line| (line, String::new())));

        assert_eq!(shortcuts[0].label, "Open: cmd-o");
        assert_eq!(shortcuts[1].label, "Close: cmd-w");
        assert_eq!(shortcuts[0].role, Role::Label);
        assert_eq!(shortcuts[1].key, ("dialog-row", 1usize).into());
        assert_eq!(
            about[0].label,
            format!("Onionskin {}", env!("CARGO_PKG_VERSION"))
        );
        assert!(!about[0].label.ends_with(':'));
    }
}
