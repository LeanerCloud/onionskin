use gpui::{rgb, rgba, Rgba, WindowAppearance};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ThemePreference {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ResolvedTheme {
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShellViewAction {
    SetTheme(ThemePreference),
    ToggleNavigationPane,
    TogglePageControls,
    ToggleReadMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SurfaceVisibility {
    pub(super) global_bar: bool,
    pub(super) rail: bool,
    pub(super) navigation_pane: bool,
    pub(super) quick_actions: bool,
    pub(super) side_panel: bool,
    pub(super) page_controls: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct ShellViewState {
    theme: ThemePreference,
    system_appearance: WindowAppearance,
    fullscreen: bool,
    read_mode: bool,
    navigation_pane_visible: bool,
    page_controls_visible: bool,
}

impl ShellViewState {
    pub(in crate::shell) fn new(system_appearance: WindowAppearance) -> Self {
        Self {
            theme: ThemePreference::System,
            system_appearance,
            fullscreen: false,
            read_mode: false,
            navigation_pane_visible: true,
            page_controls_visible: true,
        }
    }

    pub(super) fn theme(self) -> ThemePreference {
        self.theme
    }

    pub(super) fn resolved_theme(self) -> ResolvedTheme {
        resolve_theme(self.theme, self.system_appearance)
    }

    pub(in crate::shell) fn tokens(self) -> ThemeTokens {
        ThemeTokens::for_theme(self.resolved_theme())
    }

    pub(super) fn fullscreen(self) -> bool {
        self.fullscreen
    }

    pub(super) fn read_mode(self) -> bool {
        self.read_mode
    }

    pub(super) fn navigation_pane_visible(self) -> bool {
        self.navigation_pane_visible
    }

    pub(super) fn page_controls_visible(self) -> bool {
        self.page_controls_visible
    }

    pub(super) fn set_system_appearance(&mut self, appearance: WindowAppearance) -> bool {
        let before = self.resolved_theme();
        self.system_appearance = appearance;
        before != self.resolved_theme()
    }

    pub(super) fn set_theme(&mut self, theme: ThemePreference) -> bool {
        if self.theme == theme {
            return false;
        }
        self.theme = theme;
        true
    }

    pub(super) fn set_fullscreen(&mut self, fullscreen: bool) -> bool {
        if self.fullscreen == fullscreen {
            return false;
        }
        self.fullscreen = fullscreen;
        true
    }

    pub(super) fn apply(&mut self, action: ShellViewAction) -> bool {
        match action {
            ShellViewAction::SetTheme(theme) => self.set_theme(theme),
            ShellViewAction::ToggleNavigationPane => {
                self.navigation_pane_visible = !self.navigation_pane_visible;
                true
            }
            ShellViewAction::TogglePageControls => {
                self.page_controls_visible = !self.page_controls_visible;
                true
            }
            ShellViewAction::ToggleReadMode => {
                self.read_mode = !self.read_mode;
                true
            }
        }
    }

    pub(super) fn visibility(self) -> SurfaceVisibility {
        SurfaceVisibility {
            global_bar: true,
            rail: !self.read_mode,
            navigation_pane: !self.read_mode && self.navigation_pane_visible,
            quick_actions: !self.read_mode,
            side_panel: !self.read_mode,
            page_controls: !self.read_mode && self.page_controls_visible,
        }
    }
}

pub(super) fn resolve_theme(
    preference: ThemePreference,
    system_appearance: WindowAppearance,
) -> ResolvedTheme {
    match preference {
        ThemePreference::Light => ResolvedTheme::Light,
        ThemePreference::Dark => ResolvedTheme::Dark,
        ThemePreference::System => match system_appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => ResolvedTheme::Light,
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ResolvedTheme::Dark,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) struct ThemeTokens {
    pub(in crate::shell) global_bar: Rgba,
    pub(in crate::shell) surface: Rgba,
    pub(in crate::shell) raised: Rgba,
    pub(in crate::shell) input: Rgba,
    pub(in crate::shell) selected: Rgba,
    pub(in crate::shell) hover: Rgba,
    pub(in crate::shell) subtle_hover: Rgba,
    pub(in crate::shell) canvas: Rgba,
    pub(in crate::shell) text: Rgba,
    pub(in crate::shell) secondary_text: Rgba,
    pub(in crate::shell) muted_text: Rgba,
    pub(in crate::shell) disabled_text: Rgba,
    pub(in crate::shell) feedback_text: Rgba,
    pub(in crate::shell) error_surface: Rgba,
    pub(in crate::shell) canvas_error_surface: Rgba,
    pub(in crate::shell) canvas_error_text: Rgba,
    pub(in crate::shell) error_text: Rgba,
    pub(in crate::shell) selection: Rgba,
    pub(in crate::shell) search_highlight: Rgba,
    pub(in crate::shell) search_highlight_current: Rgba,
    pub(in crate::shell) drag_preview: Rgba,
}

impl ThemeTokens {
    fn for_theme(theme: ResolvedTheme) -> Self {
        match theme {
            ResolvedTheme::Light => Self {
                global_bar: rgb(0xf6f7f8),
                surface: rgb(0xe9ebef),
                raised: rgb(0xffffff),
                input: rgb(0xf2f3f5),
                selected: rgb(0xd8dee8),
                hover: rgb(0xdfe3e8),
                subtle_hover: rgb(0xe7e9ed),
                canvas: rgb(0xcfd2d7),
                text: rgb(0x202124),
                secondary_text: rgb(0x4b5563),
                muted_text: rgb(0x6b7280),
                disabled_text: rgb(0x9ca3af),
                feedback_text: rgb(0x4b5563),
                error_surface: rgb(0xfee2e2),
                canvas_error_surface: rgb(0xb91c1c),
                canvas_error_text: rgb(0xffffff),
                error_text: rgb(0x991b1b),
                selection: rgba(0x4f7cff44),
                search_highlight: rgba(0xffd54f66),
                search_highlight_current: rgba(0xff8f0099),
                drag_preview: rgba(0x777a8066),
            },
            ResolvedTheme::Dark => Self {
                global_bar: rgb(0x17181a),
                surface: rgb(0x202124),
                raised: rgb(0x292a2d),
                input: rgb(0x2d2f33),
                selected: rgb(0x3a3b3f),
                hover: rgb(0x45464b),
                subtle_hover: rgb(0x34363a),
                canvas: rgb(0x2c2c30),
                text: rgb(0xffffff),
                secondary_text: rgb(0xaeb0b5),
                muted_text: rgb(0x85878c),
                disabled_text: rgb(0x696b70),
                feedback_text: rgb(0xc6c8cd),
                error_surface: rgb(0x451a1a),
                canvas_error_surface: rgb(0x7f1d1d),
                canvas_error_text: rgb(0xffffff),
                error_text: rgb(0xfca5a5),
                selection: rgba(0x4f7cff66),
                search_highlight: rgba(0xffd54f55),
                search_highlight_current: rgba(0xffa72688),
                drag_preview: rgba(0x777a80aa),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_theme_resolves_only_to_light_or_dark() {
        assert_eq!(
            resolve_theme(ThemePreference::System, WindowAppearance::Light),
            ResolvedTheme::Light
        );
        assert_eq!(
            resolve_theme(ThemePreference::System, WindowAppearance::VibrantLight),
            ResolvedTheme::Light
        );
        assert_eq!(
            resolve_theme(ThemePreference::System, WindowAppearance::Dark),
            ResolvedTheme::Dark
        );
        assert_eq!(
            resolve_theme(ThemePreference::System, WindowAppearance::VibrantDark),
            ResolvedTheme::Dark
        );
        assert_eq!(
            resolve_theme(ThemePreference::Light, WindowAppearance::Dark),
            ResolvedTheme::Light
        );
        assert_eq!(
            resolve_theme(ThemePreference::Dark, WindowAppearance::Light),
            ResolvedTheme::Dark
        );
    }

    #[test]
    fn fullscreen_and_read_mode_remain_independent() {
        let mut state = ShellViewState::new(WindowAppearance::Dark);
        assert!(state.set_fullscreen(true));
        assert!(state.apply(ShellViewAction::ToggleReadMode));
        assert!(state.fullscreen());
        assert!(state.read_mode());

        assert!(state.set_fullscreen(false));
        assert!(!state.fullscreen());
        assert!(state.read_mode());
    }

    #[test]
    fn read_mode_hides_document_chrome_but_keeps_the_global_bar() {
        let mut state = ShellViewState::new(WindowAppearance::Dark);
        assert!(state.apply(ShellViewAction::ToggleReadMode));

        let visibility = state.visibility();
        assert!(visibility.global_bar);
        assert!(!visibility.rail);
        assert!(!visibility.navigation_pane);
        assert!(!visibility.quick_actions);
        assert!(!visibility.side_panel);
        assert!(!visibility.page_controls);
    }

    #[test]
    fn visibility_toggles_affect_only_their_target() {
        let mut state = ShellViewState::new(WindowAppearance::Dark);

        let before = state.visibility();
        assert!(state.apply(ShellViewAction::ToggleNavigationPane));
        let navigation_hidden = state.visibility();
        assert_ne!(navigation_hidden.navigation_pane, before.navigation_pane);
        assert_eq!(navigation_hidden.quick_actions, before.quick_actions);
        assert_eq!(navigation_hidden.page_controls, before.page_controls);

        assert!(state.apply(ShellViewAction::TogglePageControls));
        let controls_hidden = state.visibility();
        assert_eq!(
            controls_hidden.navigation_pane,
            navigation_hidden.navigation_pane
        );
        assert_eq!(
            controls_hidden.quick_actions,
            navigation_hidden.quick_actions
        );
        assert_ne!(
            controls_hidden.page_controls,
            navigation_hidden.page_controls
        );
    }
}
