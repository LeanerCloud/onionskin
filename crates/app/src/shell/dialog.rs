//! The shell's modal surfaces: Preferences, About, and the local keyboard
//! shortcut reference.
//!
//! One host for all three. A dialog is a panel over a backdrop that takes
//! the click that dismisses it, which is the same shape the menus already
//! use; what differs is only the body.
//!
//! The shortcut reference is generated from the keymap in force rather than
//! written out, so a rebound command reads correctly and an unbound one is
//! absent instead of lying. That is what makes it worth shipping as the Help
//! menu's local reference (parity row 112: online help is out of scope, a
//! local reference is not).

use gpui::{
    div, px, Context, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};

use super::chrome::{ShellFrame, ThemeTokens};
use crate::keymap::{platform_keystroke, Binding};
use crate::preferences::PreferenceCategory;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum ShellDialog {
    Preferences(PreferenceCategory),
    About,
    KeyboardShortcuts,
}

impl ShellDialog {
    pub(in crate::shell) fn title(self) -> &'static str {
        match self {
            Self::Preferences(_) => "Preferences",
            Self::About => "About Onionskin",
            Self::KeyboardShortcuts => "Keyboard Shortcuts",
        }
    }
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

pub(in crate::shell) fn render_dialog(
    frame: &ShellFrame,
    dialog: ShellDialog,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let body = match dialog {
        ShellDialog::Preferences(category) => {
            super::preferences_dialog::render_preferences(frame, category, theme, cx)
                .into_any_element()
        }
        ShellDialog::About => rows(about_lines().into_iter().map(|line| (line, String::new())))
            .text_color(theme.text)
            .into_any_element(),
        ShellDialog::KeyboardShortcuts => rows(frame.shortcut_rows(cx))
            .text_color(theme.text)
            .into_any_element(),
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
        .on_click(cx.listener(|frame, _event, _window, cx| frame.close_dialog(cx)))
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
                                .hover(move |button| button.bg(theme.subtle_hover))
                                .on_click(
                                    cx.listener(|frame, _event, _window, cx| {
                                        frame.close_dialog(cx)
                                    }),
                                )
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

fn rows(rows: impl IntoIterator<Item = (String, String)>) -> gpui::Div {
    let mut list = div().flex().flex_col().gap_1();
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
}
