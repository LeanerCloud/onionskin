use gpui::{rgb, rgba, Rgba, WindowAppearance};

/// The display theme is a preference before it is a view state: the View
/// menu and the preferences dialog set the same thing, and it is persisted.
pub(super) use crate::preferences::ThemePreference;

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
    pub(super) tab_bar: bool,
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
    /// View > Show/Hide > Line Weights: the preference as this window last
    /// heard it, for the menu's check mark.
    line_weights: bool,
}

impl ShellViewState {
    pub(in crate::shell) fn new(
        system_appearance: WindowAppearance,
        theme: ThemePreference,
    ) -> Self {
        Self {
            theme,
            system_appearance,
            fullscreen: false,
            read_mode: false,
            navigation_pane_visible: true,
            page_controls_visible: true,
            line_weights: true,
        }
    }

    /// The same state, with Line Weights as the preferences have it.
    pub(in crate::shell) fn with_line_weights(mut self, on: bool) -> Self {
        self.line_weights = on;
        self
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

    pub(super) fn line_weights(self) -> bool {
        self.line_weights
    }

    pub(super) fn set_line_weights(&mut self, on: bool) -> bool {
        if self.line_weights == on {
            return false;
        }
        self.line_weights = on;
        true
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

    /// Which surfaces this state puts on screen.
    ///
    /// Two modes take chrome away, and they take different amounts.
    ///
    /// Full Screen shows the document and nothing around it, which is what
    /// Acrobat's Full Screen mode is for. Read Mode keeps reading controls
    /// and drops everything that is about the application rather than the
    /// page: the top bars, the tool rail, the panes, the quick actions. What
    /// is left is the page controls, which already carry Acrobat's Read Mode
    /// toolbar - page back and forward, the page number, zoom out and in,
    /// and the fit buttons - so the mode needs no second toolbar of its own.
    pub(super) fn visibility(self) -> SurfaceVisibility {
        if self.fullscreen {
            return SurfaceVisibility {
                global_bar: false,
                tab_bar: false,
                rail: false,
                navigation_pane: false,
                quick_actions: false,
                side_panel: false,
                page_controls: false,
            };
        }
        SurfaceVisibility {
            global_bar: !self.read_mode,
            tab_bar: !self.read_mode,
            rail: !self.read_mode,
            navigation_pane: !self.read_mode && self.navigation_pane_visible,
            quick_actions: !self.read_mode,
            side_panel: !self.read_mode,
            page_controls: self.page_controls_visible,
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
    /// The content a Tags or Content pane choice points at, which must not read
    /// as a search hit.
    pub(in crate::shell) structure_highlight: Rgba,
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
                structure_highlight: rgba(0x1e88e555),
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
                structure_highlight: rgba(0x64b5f655),
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
        let mut state = ShellViewState::new(WindowAppearance::Dark, ThemePreference::System);
        assert!(state.set_fullscreen(true));
        assert!(state.apply(ShellViewAction::ToggleReadMode));
        assert!(state.fullscreen());
        assert!(state.read_mode());

        assert!(state.set_fullscreen(false));
        assert!(!state.fullscreen());
        assert!(state.read_mode());
    }

    /// Read Mode leaves the page and the controls that move through it, and
    /// takes everything else. The page controls are its toolbar, so they
    /// stay unless the user turned them off themselves.
    #[test]
    fn read_mode_hides_the_chrome_and_keeps_the_page_controls() {
        let mut state = ShellViewState::new(WindowAppearance::Dark, ThemePreference::System);
        assert!(state.apply(ShellViewAction::ToggleReadMode));

        let visibility = state.visibility();
        assert!(!visibility.global_bar);
        assert!(!visibility.tab_bar);
        assert!(!visibility.rail);
        assert!(!visibility.navigation_pane);
        assert!(!visibility.quick_actions);
        assert!(!visibility.side_panel);
        assert!(visibility.page_controls, "read mode has no toolbar left");

        assert!(state.apply(ShellViewAction::TogglePageControls));
        assert!(
            !state.visibility().page_controls,
            "read mode overrode the user's own show/hide choice"
        );
    }

    /// Full Screen is the document and nothing else, including the toolbar
    /// Read Mode keeps.
    #[test]
    fn full_screen_hides_every_surface() {
        let mut state = ShellViewState::new(WindowAppearance::Dark, ThemePreference::System);
        assert!(state.set_fullscreen(true));

        let visibility = state.visibility();
        assert!(!visibility.global_bar);
        assert!(!visibility.tab_bar);
        assert!(!visibility.rail);
        assert!(!visibility.navigation_pane);
        assert!(!visibility.quick_actions);
        assert!(!visibility.side_panel);
        assert!(!visibility.page_controls);

        assert!(state.set_fullscreen(false));
        assert!(
            state.visibility().global_bar,
            "leaving full screen left the chrome hidden"
        );
    }

    #[test]
    fn visibility_toggles_affect_only_their_target() {
        let mut state = ShellViewState::new(WindowAppearance::Dark, ThemePreference::System);

        let before = state.visibility();
        assert!(before.global_bar && before.tab_bar);
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
