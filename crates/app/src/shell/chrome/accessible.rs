//! What the shell tells a screen reader, and what a screen reader can ask it
//! to do.
//!
//! Every surface builds its accessible description next to its `render`, from
//! the same state and in the same order, so the tree describes what is on
//! screen rather than a second guess at it. The description carries the
//! action the control's click listener runs, so the keyboard path, the
//! screen-reader path and the mouse path cannot diverge.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use accesskit::Rect;
use gpui::{Bounds, Pixels, Window};

use super::global_bar::MenuCommand;
use super::quick_actions::{QuickAction, QuickActionEntry};
use super::rail::RailEntry;
use super::tabs::TabCommand;
use crate::preferences::PreferenceCategory;
use crate::shell::canvas::ViewAction;
use crate::shell::context_menu::CanvasContextCommand;
use crate::shell::find_bar::{FindDirection, FindOption};
use crate::shell::home::HomeView;
use crate::shell::panes::PaneAction;
use crate::shell::preferences_dialog::PreferenceChange;

/// The description of one accessible element, with the shell's own action on
/// it.
pub(in crate::shell) type Element = crate::a11y::Element<Activation>;

/// Which of the shell's three text fields a control focuses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum TextField {
    /// The global bar's tool and command search.
    Search,
    /// The find bar's query.
    Find,
    /// The page number next to the page controls.
    Page,
}

/// What activating an accessible element does.
///
/// One variant per handler `ShellFrame` already exposes to its click
/// listeners. Keeping it an enum rather than a closure makes the dispatch an
/// exhaustive match, so a control cannot be described without also being
/// operable.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) enum Activation {
    ToggleMainMenu,
    MainMenu(MenuCommand),
    ActivateTab(usize),
    TabCommand(TabCommand, usize),
    CanvasContext(CanvasContextCommand),
    DismissNotice(usize),
    OpenRecent(usize),
    ChooseSearchResult(crate::shell::chrome::tool_search::SearchResult),
    Rail(RailEntry),
    ToggleRailExpanded,
    QuickAction(QuickActionEntry),
    ToggleQuickActionCustomization,
    ToggleQuickActionVisibility(QuickAction),
    ToggleSidePanel,
    View(ViewAction),
    SubmitPageEntry,
    Pane(PaneAction),
    StepFind(FindDirection),
    ApplyFindOption(FindOption),
    DismissFindBar,
    SetHomeView(HomeView),
    OpenFromHome,
    ShowPreferences(PreferenceCategory),
    ChangePreference(PreferenceChange),
    CloseDialog,
    Focus(TextField),
    /// Put the shell's focus back on the chrome, which is where the
    /// window-wide page keys are dispatched from.
    FocusDocument,
}

/// The rows whose children the shell measures.
///
/// A surface is a div whose children are built from one ordered list, which
/// is the list its accessible description is built from too. That is what
/// lets `gpui::Div::on_children_prepainted` hand back rectangles the
/// description can take in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(in crate::shell) enum Surface {
    MainMenu,
    Rail,
    PaneStrip,
    QuickActions,
    PageControls,
    FindBar,
    Home,
    Dialog,
}

/// Where the last frame painted each measured surface's children.
///
/// One frame behind by construction: a rectangle is only known after
/// prepaint, and the tree is built during render. Labels, roles and state are
/// current; only the geometry lags, and only by the frame that produced it.
#[derive(Debug, Clone, Default)]
pub(in crate::shell) struct Rects(Rc<RefCell<HashMap<Surface, Vec<Rect>>>>);

impl Rects {
    /// Record a surface's child rectangles, converting gpui's logical points
    /// into the physical pixels AccessKit's bounds are specified in
    /// (accesskit_macos 0.26.3 `util.rs`, `to_ns_rect`).
    pub(in crate::shell) fn record(
        &self,
        surface: Surface,
        bounds: &[Bounds<Pixels>],
        window: &Window,
    ) {
        let factor = f64::from(window.scale_factor());
        let rects = bounds
            .iter()
            .map(|bounds| {
                let x0 = f64::from(f32::from(bounds.origin.x)) * factor;
                let y0 = f64::from(f32::from(bounds.origin.y)) * factor;
                Rect::new(
                    x0,
                    y0,
                    x0 + f64::from(f32::from(bounds.size.width)) * factor,
                    y0 + f64::from(f32::from(bounds.size.height)) * factor,
                )
            })
            .collect();
        self.0.borrow_mut().insert(surface, rects);
    }

    /// A rectangle the shell computed itself, rather than one prepaint
    /// measured.
    ///
    /// The document's pages are laid out by the viewport, in the canvas's own
    /// coordinates, so they need the canvas origin added and the same
    /// conversion into physical pixels that [`Rects::record`] applies.
    pub(in crate::shell) fn view_rect(
        rect: onionskin_core::ViewRect,
        origin: onionskin_core::ViewPoint,
        scale: f32,
    ) -> Rect {
        let factor = f64::from(scale);
        let x0 = f64::from(origin.x + rect.origin.x) * factor;
        let y0 = f64::from(origin.y + rect.origin.y) * factor;
        Rect::new(
            x0,
            y0,
            x0 + f64::from(rect.size.width) * factor,
            y0 + f64::from(rect.size.height) * factor,
        )
    }

    pub(in crate::shell) fn of(&self, surface: Surface) -> Vec<Rect> {
        self.0.borrow().get(&surface).cloned().unwrap_or_default()
    }

    /// Give a surface's described children the rectangles the last frame
    /// painted them at.
    pub(in crate::shell) fn place(&self, surface: Surface, element: &mut Element) {
        element.place_children(&self.of(surface));
    }
}

gpui::actions!(
    onionskin_a11y,
    [
        /// Tab: the next control in reading order.
        FocusNext,
        /// Shift-Tab: the previous one.
        FocusPrevious,
        /// Enter or Space on the focused control.
        ActivateFocused,
    ]
);

/// The key context the shell's own navigation keys are bound in.
///
/// Named so that a text field, which binds its own context, keeps the keys it
/// needs: GPUI prefers the more specific context on the dispatch path.
pub(in crate::shell) const SHELL_KEY_CONTEXT: &str = "OnionskinShell";

pub(in crate::shell) fn install_keybindings(cx: &mut gpui::App) {
    let context = Some(SHELL_KEY_CONTEXT);
    cx.bind_keys([
        gpui::KeyBinding::new("tab", FocusNext, context),
        gpui::KeyBinding::new("shift-tab", FocusPrevious, context),
        gpui::KeyBinding::new("enter", ActivateFocused, context),
        gpui::KeyBinding::new("space", ActivateFocused, context),
    ]);
}

/// The shell's accessibility state: the platform adapter, the tab order, the
/// GPUI focus handle the chrome's keys are dispatched through, and the
/// rectangles the last frame measured.
pub(in crate::shell) struct ShellAccessibility {
    adapter: Option<crate::a11y::Adapter>,
    pub(in crate::shell) rects: Rects,
    ring: crate::a11y::Ring,
    /// What each tab stop in the last published tree does. Read back rather
    /// than recomputed, so a keystroke runs what the user was told is there.
    actions: HashMap<gpui::ElementId, Activation>,
    focus: gpui::FocusHandle,
}

impl ShellAccessibility {
    pub(in crate::shell) fn new(cx: &mut gpui::App) -> Self {
        Self {
            adapter: None,
            rects: Rects::default(),
            ring: crate::a11y::Ring::default(),
            actions: HashMap::new(),
            focus: cx.focus_handle(),
        }
    }

    /// What activating the named element runs, as of the last published tree.
    pub(in crate::shell) fn activation_for(&self, key: &gpui::ElementId) -> Option<Activation> {
        self.actions.get(key).cloned()
    }

    /// What activating the focused element runs.
    pub(in crate::shell) fn focused_activation(&self) -> Option<Activation> {
        self.activation_for(self.ring.focused()?)
    }

    pub(in crate::shell) fn focus_handle(&self) -> &gpui::FocusHandle {
        &self.focus
    }

    #[cfg(all(test, feature = "shell-test-support"))]
    pub(in crate::shell) fn focused(&self) -> Option<&gpui::ElementId> {
        self.ring.focused()
    }

    /// Ask the shell for something as a screen reader asks for it, through
    /// the same queue the platform's action handler writes to.
    #[cfg(all(test, feature = "shell-test-support"))]
    pub(in crate::shell) fn deliver(&mut self, key: &gpui::ElementId, action: accesskit::Action) {
        self.adapter
            .as_mut()
            .expect("the adapter is attached on the first frame")
            .deliver(key, action);
    }

    pub(in crate::shell) fn step(&mut self, step: crate::a11y::Step) -> bool {
        self.ring.step(step)
    }

    pub(in crate::shell) fn focus_key(&mut self, key: &gpui::ElementId) -> bool {
        self.ring.focus(key)
    }

    /// Attach the adapter if this is the first frame, take the new tab order,
    /// and publish the tree.
    ///
    /// The attach happens here because the spike records it as an ordering
    /// constraint: the adapter has to exist before anything queries the view,
    /// and the first render is the earliest point a `&Window` is in reach.
    pub(in crate::shell) fn publish(&mut self, root: &Element, window: &Window) {
        self.ring.rebuild(root);
        self.actions = root
            .walk()
            .filter_map(|element| Some((element.key.clone(), element.activation.clone()?)))
            .collect();
        let adapter = self
            .adapter
            .get_or_insert_with(|| crate::a11y::Adapter::attach(window));
        adapter.observe_window_active(window.is_window_active());
        adapter.publish(root, self.ring.focused());
    }

    /// Whether a screen reader asked for anything since the last frame.
    pub(in crate::shell) fn has_requests(&self) -> bool {
        self.adapter
            .as_ref()
            .is_some_and(crate::a11y::Adapter::has_requests)
    }

    /// What a screen reader asked the shell to do since the last frame.
    pub(in crate::shell) fn take_requests(
        &mut self,
    ) -> Vec<(gpui::ElementId, crate::a11y::Request)> {
        self.adapter
            .as_mut()
            .map(crate::a11y::Adapter::take_requests)
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use accesskit::Role;
    use onionskin_core::{ViewPoint, ViewRect, ViewSize};

    /// The only transform that puts a page, and every run of words on it, on
    /// screen. A rectangle arrives in the canvas's own coordinates and has to
    /// leave with the canvas origin added and the zoom applied, or a screen
    /// reader draws its cursor somewhere the document is not.
    ///
    /// A non-zero origin and a zoom other than 1, because both are identity
    /// at the defaults and would hide a transform that dropped either.
    #[test]
    fn a_page_rectangle_takes_the_canvas_origin_and_the_zoom() {
        let placed = Rects::view_rect(
            ViewRect {
                origin: ViewPoint { x: 10.0, y: 20.0 },
                size: ViewSize {
                    width: 30.0,
                    height: 40.0,
                },
            },
            ViewPoint { x: 5.0, y: 7.0 },
            2.0,
        );

        assert_eq!(placed, Rect::new(30.0, 54.0, 90.0, 134.0));
    }

    /// Two rectangles that differ in the canvas have to differ on screen.
    /// A transform that answers the same thing for everything satisfies any
    /// single-rectangle assertion.
    #[test]
    fn two_rectangles_that_differ_in_the_canvas_differ_on_screen() {
        let page = ViewRect {
            origin: ViewPoint { x: 0.0, y: 0.0 },
            size: ViewSize {
                width: 200.0,
                height: 100.0,
            },
        };
        let word = ViewRect {
            origin: ViewPoint { x: 20.0, y: 36.0 },
            size: ViewSize {
                width: 130.0,
                height: 18.0,
            },
        };
        let origin = ViewPoint { x: 12.0, y: 34.0 };

        let page = Rects::view_rect(page, origin, 1.5);
        let word = Rects::view_rect(word, origin, 1.5);

        assert!(word.x0 > page.x0 && word.y0 > page.y0);
        assert!(word.x1 < page.x1 && word.y1 < page.y1);
    }

    #[test]
    fn a_surface_with_no_recorded_frame_leaves_its_children_unplaced() {
        let rects = Rects::default();
        let mut row =
            Element::new("row", Role::Group, "Row").child(Element::new("a", Role::Button, "A"));

        rects.place(Surface::PageControls, &mut row);

        assert_eq!(row.children[0].bounds, None);
    }
}
