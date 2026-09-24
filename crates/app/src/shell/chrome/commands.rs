//! What every shell command is called, and the keystroke Acrobat gives it.
//!
//! One table, read by four surfaces: the keymap that binds the keystrokes,
//! the `keymap.json` loader that resolves the user's file against these
//! defaults, the Keyboard Shortcuts window that lists what is in force, and
//! the tests that check a command cannot be bound but unreachable.
//!
//! Two rules about what carries a default keystroke:
//!
//! - A command that this milestone cannot run carries none. A keystroke
//!   that reports "lands in M3" is worse than no keystroke. None is left;
//!   New Window and Line Weights, the last two, run now and still carry
//!   none because Acrobat's default for either is not settled here.
//! - A command the registry runs (Select All, Deselect All) carries none
//!   here either: its keystroke comes from the plugin that registers it, so
//!   the two cannot disagree and the keymap cannot see the same id twice.
//!
//! Page navigation is deliberately unbound. Acrobat's defaults there are
//! bare keys (Home, End, Page Up, Page Down, the arrows), which belong to
//! the canvas's own key handling; the canvas has none yet, and binding them
//! window-wide would take them away from every text field in the chrome.
//! `keymap.json` can bind them today for anyone who wants them.

use super::global_bar::{ExportTarget, MenuCommand, PageCommand};
use super::quick_actions::QuickAction;
use crate::keymap::CommandDefault;

impl MenuCommand {
    /// Every command the menus can carry, in menu order.
    pub(in crate::shell) fn all() -> Vec<MenuCommand> {
        let mut all = vec![
            MenuCommand::Open,
            MenuCommand::OpenRecent,
            MenuCommand::CloseTab,
            MenuCommand::CloseOtherTabs,
            MenuCommand::CloseAllTabs,
            MenuCommand::Save,
            MenuCommand::SaveAs,
            MenuCommand::Revert,
            MenuCommand::Skins,
            MenuCommand::AttachToEmail,
            MenuCommand::CopyFileToClipboard,
            MenuCommand::CombineFiles,
            MenuCommand::CreateFromFiles,
            MenuCommand::CreateFromFile,
            MenuCommand::CreateFromClipboard,
            MenuCommand::SplitDocument,
        ];
        all.extend(ExportTarget::ALL.map(MenuCommand::Export));
        all.extend([
            MenuCommand::ExportAllImages,
            MenuCommand::SaveAsOther,
            MenuCommand::ReduceFileSize,
            MenuCommand::Properties,
            MenuCommand::PageSetup,
            MenuCommand::Print,
        ]);
        all.extend([MenuCommand::Quit, MenuCommand::Undo, MenuCommand::Redo]);
        all.extend(onionskin_plugin_api::EditVerb::ALL.map(MenuCommand::Edit));
        all.extend([
            MenuCommand::SelectAll,
            MenuCommand::DeselectAll,
            MenuCommand::TakeSnapshot,
            MenuCommand::Find,
            MenuCommand::AdvancedSearch,
        ]);
        all.extend([MenuCommand::OrganizePages, MenuCommand::CropPages]);
        all.extend(PageCommand::ALL.map(MenuCommand::Page));
        all.extend([
            MenuCommand::Stamps,
            MenuCommand::PasteStamp,
            MenuCommand::SummarizeComments,
        ]);
        all.extend([
            MenuCommand::Preferences,
            MenuCommand::PreviousView,
            MenuCommand::NextView,
            MenuCommand::FirstPage,
            MenuCommand::PreviousPage,
            MenuCommand::NextPage,
            MenuCommand::LastPage,
            MenuCommand::RotateClockwise,
            MenuCommand::ActualSize,
            MenuCommand::ZoomOut,
            MenuCommand::ZoomIn,
            MenuCommand::ZoomTo,
            MenuCommand::DynamicZoom,
            MenuCommand::FitPage,
            MenuCommand::FitWidth,
            MenuCommand::FitHeight,
            MenuCommand::FitVisible,
            MenuCommand::SinglePage,
            MenuCommand::SinglePageContinuous,
            MenuCommand::TwoPage,
            MenuCommand::TwoPageContinuous,
            MenuCommand::ToggleCover,
            MenuCommand::AutoScroll,
            MenuCommand::Tools,
            MenuCommand::ManageTools,
            MenuCommand::ToggleNavigationPane,
        ]);
        all.extend(QuickAction::ALL.map(MenuCommand::ToggleQuickAction));
        all.extend([
            MenuCommand::TogglePageControls,
            MenuCommand::LineWeights,
            MenuCommand::ThemeSystem,
            MenuCommand::ThemeLight,
            MenuCommand::ThemeDark,
            MenuCommand::ReadMode,
            MenuCommand::FullScreen,
            MenuCommand::NewWindow,
            MenuCommand::Minimize,
            MenuCommand::ZoomWindow,
            MenuCommand::BringAllToFront,
            MenuCommand::About,
            MenuCommand::KeyboardShortcuts,
        ]);
        all
    }

    /// The id `keymap.json` binds this command by, and the id a registry
    /// command shares with the menu entry that runs it.
    pub(in crate::shell) fn id(self) -> &'static str {
        match self {
            Self::Open => "file.open",
            Self::OpenRecent => "file.open-recent",
            Self::CloseTab => "file.close",
            Self::CloseOtherTabs => "file.close-others",
            Self::CloseAllTabs => "file.close-all",
            Self::Save => "file.save",
            Self::SaveAs => "file.save-as",
            Self::Revert => "file.revert",
            Self::Skins => "file.skins",
            Self::OrganizePages => "edit.organize-pages",
            Self::AttachToEmail => "file.attach-to-email",
            Self::CopyFileToClipboard => "file.copy-to-clipboard",
            Self::Edit(verb) => match verb {
                onionskin_plugin_api::EditVerb::Cut => "edit.cut",
                onionskin_plugin_api::EditVerb::Copy => "edit.copy",
                onionskin_plugin_api::EditVerb::Paste => "edit.paste",
                onionskin_plugin_api::EditVerb::Delete => "edit.delete",
            },
            Self::CombineFiles => "file.combine",
            Self::CreateFromFiles => "file.create-from-files",
            Self::CreateFromFile => "file.create-from-file",
            Self::CreateFromClipboard => "file.create-from-clipboard",
            Self::Stamps => "comment.stamps",
            Self::PasteStamp => "comment.paste-stamp",
            Self::SummarizeComments => onionskin_plugin_api::command_ids::SUMMARIZE_COMMENTS,
            Self::ExportAllImages => "file.export-all-images",
            Self::SaveAsOther => "file.save-as-other",
            Self::Properties => "file.properties",
            Self::SplitDocument => onionskin_plugin_api::command_ids::SPLIT_DOCUMENT,
            Self::CropPages => onionskin_plugin_api::command_ids::CROP_PAGES,
            Self::ReduceFileSize => onionskin_plugin_api::command_ids::REDUCE_FILE_SIZE,
            Self::PageSetup => "file.page-setup",
            Self::Print => onionskin_plugin_api::command_ids::PRINT,
            Self::Export(ExportTarget::Text) => "file.export-text",
            Self::Export(ExportTarget::Png) => "file.export-png",
            Self::Export(ExportTarget::Svg) => "file.export-svg",
            Self::Export(ExportTarget::Jpeg) => "file.export-jpeg",
            Self::Export(ExportTarget::Tiff) => "file.export-tiff",
            Self::Quit => "file.quit",
            Self::Undo => "edit.undo",
            Self::Redo => "edit.redo",
            Self::SelectAll => onionskin_commands_core_id::SELECT_ALL,
            Self::DeselectAll => onionskin_commands_core_id::DESELECT_ALL,
            Self::TakeSnapshot => "edit.take-snapshot",
            Self::Find => "edit.find",
            Self::AdvancedSearch => "edit.advanced-search",
            Self::Page(page) => page.id(),
            Self::Preferences => "edit.preferences",
            Self::PreviousView => "view.previous-view",
            Self::NextView => "view.next-view",
            Self::FirstPage => "view.first-page",
            Self::PreviousPage => "view.previous-page",
            Self::NextPage => "view.next-page",
            Self::LastPage => "view.last-page",
            Self::RotateClockwise => "view.rotate-clockwise",
            Self::ActualSize => "view.actual-size",
            Self::ZoomOut => "view.zoom-out",
            Self::ZoomIn => "view.zoom-in",
            Self::ZoomTo => "view.zoom-to",
            Self::DynamicZoom => "view.dynamic-zoom",
            Self::FitPage => "view.fit-page",
            Self::FitWidth => "view.fit-width",
            Self::FitHeight => "view.fit-height",
            Self::FitVisible => "view.fit-visible",
            Self::SinglePage => "view.single-page",
            Self::SinglePageContinuous => "view.single-page-continuous",
            Self::TwoPage => "view.two-page",
            Self::TwoPageContinuous => "view.two-page-continuous",
            Self::ToggleCover => "view.show-cover-page",
            Self::Tools => "view.tools",
            Self::ManageTools => "view.manage-tools",
            Self::AutoScroll => "view.automatically-scroll",
            Self::ToggleNavigationPane => "view.show-navigation-panes",
            Self::ToggleQuickAction(QuickAction::Select) => "view.show-quick-action-select",
            Self::ToggleQuickAction(QuickAction::Comment) => "view.show-quick-action-comment",
            Self::ToggleQuickAction(QuickAction::Highlight) => "view.show-quick-action-highlight",
            Self::ToggleQuickAction(QuickAction::Draw) => "view.show-quick-action-draw",
            Self::ToggleQuickAction(QuickAction::FillTextFields) => {
                "view.show-quick-action-fill-text-fields"
            }
            Self::ToggleQuickAction(QuickAction::AddSignature) => {
                "view.show-quick-action-add-signature"
            }
            Self::TogglePageControls => "view.show-page-controls",
            Self::LineWeights => "view.line-weights",
            Self::ThemeSystem => "view.theme-system",
            Self::ThemeLight => "view.theme-light",
            Self::ThemeDark => "view.theme-dark",
            Self::ReadMode => "view.read-mode",
            Self::FullScreen => "view.full-screen",
            Self::NewWindow => "window.new",
            Self::Minimize => "window.minimize",
            Self::ZoomWindow => "window.zoom",
            Self::BringAllToFront => "window.bring-all-to-front",
            Self::About => "help.about",
            Self::KeyboardShortcuts => "help.keyboard-shortcuts",
        }
    }

    /// Acrobat's default keystroke, in GPUI's spelling with `cmd` for the
    /// command modifier. `None` means the command ships unbound.
    pub(in crate::shell) fn default_keystroke(self) -> Option<&'static str> {
        Some(match self {
            Self::Open => "cmd-o",
            Self::CloseTab => "cmd-w",
            Self::CloseAllTabs => "cmd-shift-w",
            Self::Quit => "cmd-q",
            Self::Find => "cmd-f",
            // Acrobat's Ctrl+Shift+F.
            Self::AdvancedSearch => "cmd-shift-f",
            Self::Preferences => "cmd-k",
            Self::Properties => "cmd-d",
            Self::Print => "cmd-p",
            // Acrobat's Shift+Ctrl+P.
            Self::PageSetup => "cmd-shift-p",
            // Acrobat's Ctrl+Shift+H, and free on macOS inside an app.
            Self::AutoScroll => "cmd-shift-h",
            // The platform's own Minimize key.
            Self::Minimize => "cmd-m",
            Self::Save => "cmd-s",
            Self::SaveAs => "cmd-shift-s",
            Self::Undo => "cmd-z",
            Self::Redo => "cmd-shift-z",
            Self::Edit(onionskin_plugin_api::EditVerb::Cut) => "cmd-x",
            Self::Edit(onionskin_plugin_api::EditVerb::Copy) => "cmd-c",
            Self::Edit(onionskin_plugin_api::EditVerb::Paste) => "cmd-v",
            Self::Edit(onionskin_plugin_api::EditVerb::Delete) => "delete",
            Self::PreviousView => "alt-left",
            Self::NextView => "alt-right",
            // Acrobat's Shift+Ctrl+Plus, which is this key with shift held.
            Self::RotateClockwise => "cmd-shift-=",
            Self::ActualSize => "cmd-1",
            Self::ZoomOut => "cmd--",
            Self::ZoomIn => "cmd-=",
            Self::FitPage => "cmd-0",
            Self::FitWidth => "cmd-2",
            Self::FitVisible => "cmd-3",
            Self::ToggleNavigationPane => "f4",
            Self::FullScreen => "cmd-l",
            // Read Mode's Acrobat default is Ctrl+H, and cmd-h is Hide on
            // macOS. Taking Hide from the user is worse than shipping this
            // one unbound; keymap.json can bind it.
            // Acrobat's Zoom To is Ctrl+M, and cmd-m is Minimize on macOS.
            // Same call as Read Mode below: keymap.json can bind it.
            // Acrobat gives Dynamic Zoom no default key either; the tool
            // rail and this entry are how it is reached.
            Self::DynamicZoom
            | Self::ZoomTo
            | Self::ReadMode
            | Self::OpenRecent
            | Self::CloseOtherTabs
            | Self::Revert
            | Self::Skins
            | Self::OrganizePages
            | Self::AttachToEmail
            | Self::CopyFileToClipboard
            | Self::ReduceFileSize
            | Self::CombineFiles
            | Self::CreateFromFiles
            | Self::CreateFromFile
            | Self::CreateFromClipboard
            | Self::Stamps
            | Self::PasteStamp
            | Self::SummarizeComments
            | Self::ExportAllImages
            | Self::SaveAsOther
            | Self::SplitDocument
            | Self::CropPages
            | Self::Export(_)
            | Self::SelectAll
            | Self::DeselectAll
            | Self::TakeSnapshot
            | Self::Page(_)
            | Self::FirstPage
            | Self::PreviousPage
            | Self::NextPage
            | Self::LastPage
            | Self::FitHeight
            | Self::SinglePage
            | Self::SinglePageContinuous
            | Self::TwoPage
            | Self::TwoPageContinuous
            | Self::ToggleCover
            | Self::Tools
            | Self::ManageTools
            | Self::ToggleQuickAction(_)
            | Self::TogglePageControls
            | Self::LineWeights
            | Self::ThemeSystem
            | Self::ThemeLight
            | Self::ThemeDark
            | Self::NewWindow
            | Self::ZoomWindow
            | Self::BringAllToFront
            | Self::About
            | Self::KeyboardShortcuts => return None,
        })
    }

    /// The registry command this entry runs, for the entries that are a
    /// command rather than shell behaviour. The menu never carries the
    /// command's body: a plugin that registers this id makes the entry live,
    /// and a build without that plugin says so.
    pub(in crate::shell) fn registry_command_id(self) -> Option<&'static str> {
        match self {
            Self::SelectAll
            | Self::DeselectAll
            | Self::Page(_)
            | Self::SplitDocument
            | Self::CropPages
            | Self::SummarizeComments
            | Self::ReduceFileSize => Some(self.id()),
            _ => None,
        }
    }
}

/// The ids `commands-core` registers, named here so the Edit menu asks for
/// the same strings the plugin publishes. Kept as a module rather than as a
/// dependency on the plugin crate: the plugin is optional, and the menu
/// entry has to exist in a build that compiled it out in order to say it is
/// missing.
mod onionskin_commands_core_id {
    pub(super) const SELECT_ALL: &str = "edit.select-all";
    pub(super) const DESELECT_ALL: &str = "edit.deselect-all";
}

/// What the keymap starts from: every shell command that can run, plus the
/// commands the registry contributes with their own keystrokes.
pub(in crate::shell) fn command_defaults(
    registry: impl Iterator<Item = (&'static str, Option<&'static str>)>,
) -> Vec<CommandDefault> {
    MenuCommand::all()
        .into_iter()
        .filter(|command| command.registry_command_id().is_none())
        .map(|command| CommandDefault {
            id: command.id(),
            keystroke: command.default_keystroke(),
        })
        .chain(registry.map(|(id, keystroke)| CommandDefault { id, keystroke }))
        .collect()
}

/// The command an id names, for the keystroke route and for the palette.
pub(in crate::shell) fn command_for_id(id: &str) -> Option<MenuCommand> {
    MenuCommand::all()
        .into_iter()
        .find(|command| command.id() == id)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use gpui::Keystroke;

    use super::*;

    /// A duplicated id would make `keymap.json` bind two commands at once
    /// and make `command_for_id` answer with whichever came first.
    #[test]
    fn every_command_has_its_own_id() {
        let all = MenuCommand::all();
        let ids: BTreeSet<_> = all.iter().map(|command| command.id()).collect();

        assert_eq!(ids.len(), all.len());
        assert!(all.iter().all(|command| !command.id().is_empty()));
    }

    /// The table is the enum's own list: a variant added without a row here
    /// would be unreachable from the keymap and absent from the shortcuts
    /// window, and nothing else would notice.
    #[test]
    fn the_table_covers_every_command_the_menus_carry() {
        for command in MenuCommand::all() {
            assert!(
                command_for_id(command.id()) == Some(command),
                "{} does not round-trip through its id",
                command.id()
            );
        }
    }

    /// Every default has to be a keystroke GPUI can bind. A typo here would
    /// otherwise surface as a shortcut that silently never fires.
    #[test]
    fn every_default_keystroke_parses_on_both_platforms() {
        for command in MenuCommand::all() {
            let Some(keystroke) = command.default_keystroke() else {
                continue;
            };
            for macos in [true, false] {
                let mapped = crate::keymap::platform_keystroke(keystroke, macos);
                assert!(
                    Keystroke::parse(&mapped).is_ok(),
                    "{} binds {mapped}, which GPUI cannot parse",
                    command.id()
                );
            }
        }
    }

    /// Two commands sharing a default keystroke is the collision the keymap
    /// reports at runtime; in the set this build ships it is a mistake to
    /// catch here, where it is still free to fix.
    ///
    /// Over the defaults the app really resolves, which is the built-in
    /// table plus whatever the installed plugins registered, and on both
    /// platforms: off macOS `cmd` and `ctrl` are one key, so a plugin
    /// binding `ctrl-o` collides there and not on a Mac.
    #[test]
    fn no_two_defaults_claim_one_keystroke() {
        let defaults = command_defaults(
            crate::build_registry()
                .commands()
                .iter()
                .map(|command| (command.id, command.keybind)),
        );
        let bound = defaults
            .iter()
            .filter(|default| default.keystroke.is_some())
            .count();
        assert!(bound > 1, "there is nothing to collide");

        for macos in [true, false] {
            let keymap = crate::keymap::Keymap::resolve(
                &defaults,
                None,
                std::path::Path::new("built-in"),
                macos,
            );
            let reported: Vec<_> = keymap.errors().iter().map(ToString::to_string).collect();
            assert!(
                reported.is_empty(),
                "the built-in defaults collide on macos={macos}: {reported:?}"
            );
            assert_eq!(keymap.bindings().len(), bound);
        }
    }

    /// A command the app cannot run yet must not hold a keystroke: pressing
    /// it would report a milestone rather than do anything.
    #[test]
    fn commands_that_wait_for_a_later_milestone_ship_unbound() {
        assert_eq!(MenuCommand::LineWeights.default_keystroke(), None);
    }

    /// The registry owns the keystroke of the commands it registers, so the
    /// menu contributes no default for them and the keymap sees each id once.
    #[test]
    fn a_registry_backed_command_takes_its_keystroke_from_the_registry() {
        let defaults = command_defaults([("edit.select-all", Some("cmd-a"))].into_iter());

        let select_all: Vec<_> = defaults
            .iter()
            .filter(|default| default.id == "edit.select-all")
            .collect();
        assert_eq!(select_all.len(), 1, "{select_all:?}");
        assert_eq!(select_all[0].keystroke, Some("cmd-a"));
        assert_eq!(MenuCommand::SelectAll.default_keystroke(), None);
        assert_eq!(
            MenuCommand::SelectAll.registry_command_id(),
            Some("edit.select-all")
        );
    }

    /// The defaults the app actually starts from: every id appears once, so
    /// no built-in table entry shadows a plugin's command.
    #[cfg(feature = "commands-core")]
    #[test]
    fn the_installed_registry_and_the_menu_table_share_no_id_twice() {
        let registry = crate::build_registry();
        let defaults = command_defaults(
            registry
                .commands()
                .iter()
                .map(|command| (command.id, command.keybind)),
        );

        let ids: BTreeSet<_> = defaults.iter().map(|default| default.id).collect();
        assert_eq!(ids.len(), defaults.len());
        assert!(ids.contains("edit.select-all"));
    }
}
