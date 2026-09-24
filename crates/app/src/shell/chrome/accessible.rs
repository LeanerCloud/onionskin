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
    /// The find bar's Replace With.
    Replace,
    /// The page number next to the page controls.
    Page,
    /// Zoom To's magnification, in percent.
    ZoomPercent,
    /// Layer Properties' layer name.
    LayerName,
    ExportFirst,
    ExportLast,
    ExportDpi,
    /// The export dialog's JPEG quality.
    ExportQuality,
    /// The Combine dialog's page selection for the file selected in its list.
    CombinePages,
    /// The Split dialog's number of pages or size.
    SplitValue,
    /// The Properties dialog's Description fields.
    PropertiesTitle,
    PropertiesAuthor,
    PropertiesSubject,
    PropertiesKeywords,
    /// The Properties dialog's new custom property.
    PropertiesCustomKey,
    PropertiesCustomValue,
    /// The Initial View tab's page the document opens at.
    PropertiesOpenPage,
    /// The title New Bookmark and Rename Bookmark ask for.
    BookmarkTitle,
    /// The Comments pane's field: a comment's text, or a reply.
    CommentDraft,
    /// Commenting preferences' author name.
    CommentingAuthor,
    /// The properties inspector's author and subject.
    InspectorAuthor,
    InspectorSubject,
    /// The Print dialog's copies, pages, custom scale and booklet interval.
    PrintCopies,
    PrintPages,
    PrintScale,
    PrintPosterScale,
    PrintPosterOverlap,
    PrintBookletFrom,
    PrintBookletTo,
    /// Advanced Search's words, and its criterion's value.
    AdvancedQuery,
    AdvancedValue,
    /// Crop Pages' four margins, and the page size it can change to.
    CropTop,
    CropBottom,
    CropLeft,
    CropRight,
    CropWidth,
    CropHeight,
    /// The page-marks dialog's fields.
    #[cfg(feature = "tools-edit")]
    Mark(crate::shell::chrome::marks_dialog::MarkField),
    /// The link dialog's page number and web address.
    #[cfg(feature = "tools-edit")]
    Link(crate::shell::chrome::link_dialog::LinkField),
    /// Add Signature's typed name.
    #[cfg(feature = "tools-fill-sign")]
    Signature(crate::shell::chrome::signature_dialog::SignatureField),
    /// The redaction dialog's fields.
    #[cfg(feature = "redact")]
    Redact(crate::shell::chrome::redact_dialog::RedactField),
    /// The text box over a form field being filled on the canvas.
    #[cfg(feature = "tools-form")]
    FormField,
    /// A form field's Properties dialog's fields.
    #[cfg(feature = "tools-form")]
    Field(crate::shell::chrome::field_dialog::FieldInput),
    /// The text box over a line of text being edited on the canvas.
    #[cfg(feature = "tools-edit")]
    LineText,
    /// Check Spelling's Change To.
    #[cfg(feature = "spelling")]
    SpellingChangeTo,
    /// The password an encrypted document asks for as it opens.
    DocumentPassword,
    /// Protect Using Password's Document Open Password.
    OpenPassword,
    /// Protect Using Password's Change Permissions Password.
    PermissionsPassword,
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
    /// The global bar's Convert button, which opens its own panel.
    ToggleConvertMenu,
    /// The find bar's Replace, or with `all`, Replace All.
    ReplaceText {
        all: bool,
    },
    MainMenu(MenuCommand),
    ActivateTab(usize),
    TabCommand(TabCommand, usize),
    CanvasContext(CanvasContextCommand),
    DismissNotice(usize),
    OpenRecent(usize),
    /// Home's Starred section: open the starred document at this index.
    OpenStarred(usize),
    /// Star or unstar a document on Home.
    ToggleStar(std::path::PathBuf),
    ChooseSearchResult(crate::shell::chrome::tool_search::SearchResult),
    Rail(RailEntry),
    ToggleRailExpanded,
    QuickAction(QuickActionEntry),
    ToggleQuickActionCustomization,
    ToggleQuickActionVisibility(QuickAction),
    /// Copy or move the grid's pages to the open document at this index.
    SendPages(usize),
    /// A control in the Advanced Search dialog.
    AdvancedSearch(crate::shell::chrome::advanced_search::AdvancedAction),
    /// Manage Tools: show or hide this tool's rail button.
    ToggleToolShown(&'static str),
    ToggleSidePanel,
    View(ViewAction),
    SubmitPageEntry,
    /// Zoom To: apply the magnification typed in its field.
    SubmitZoomPercent,
    Pane(PaneAction),
    StepFind(FindDirection),
    ApplyFindOption(FindOption),
    DismissFindBar,
    SetHomeView(HomeView),
    OpenFromHome,
    ShowPreferences(PreferenceCategory),
    ChangePreference(PreferenceChange),
    /// Save the name typed in Commenting preferences.
    SaveCommentingAuthor,
    /// The comment properties inspector in the side panel.
    Inspector(super::inspector::InspectorAction),
    CloseDialog,
    SubmitExport,
    CancelExport,
    Combine(crate::shell::chrome::combine_dialog::CombineAction),
    Split(crate::shell::chrome::split_dialog::SplitAction),
    /// The Crop Pages dialog.
    Crop(crate::shell::chrome::crop_dialog::CropAction),
    /// The Watermark, Background, Header & Footer and Bates dialog.
    #[cfg(feature = "tools-edit")]
    Marks(crate::shell::chrome::marks_dialog::MarkAction),
    /// Create Link and Link Properties.
    #[cfg(feature = "tools-edit")]
    Link(crate::shell::chrome::link_dialog::LinkAction),
    /// Add Signature and Add Initials.
    #[cfg(feature = "tools-fill-sign")]
    Signature(crate::shell::chrome::signature_dialog::SignatureAction),
    /// The redaction dialog.
    #[cfg(feature = "redact")]
    Redact(crate::shell::chrome::redact_dialog::RedactAction),
    /// Pick this option of the dropdown being filled on the canvas.
    #[cfg(feature = "tools-form")]
    FormOption(usize),
    /// Put this Auto-Complete suggestion in the field being filled.
    #[cfg(feature = "tools-form")]
    FormSuggestion(usize),
    /// A form field's Properties dialog.
    #[cfg(feature = "tools-form")]
    Field(crate::shell::chrome::field_dialog::FieldAction),
    /// Check Spelling.
    #[cfg(feature = "spelling")]
    Spelling(crate::shell::chrome::spelling_dialog::SpellingAction),
    /// A font, size or colour picked in the line editor on the canvas.
    #[cfg(feature = "tools-edit")]
    LineStyle(crate::shell::line_style::StyleChoice),
    /// One of the active tool's settings, chosen or turned the other way.
    ToolSetting(String),
    /// The password prompt's Open or Cancel.
    Password(crate::shell::chrome::password_dialog::PasswordAction),
    /// A control in Protect Using Password.
    Protect(crate::shell::chrome::protect_dialog::ProtectAction),
    /// The Trust Manager's Open Web Link prompt.
    WebLink(crate::shell::chrome::web_link_dialog::WebLinkAction),
    /// A control in Layer Properties.
    LayerProperties(crate::shell::chrome::layer_properties_dialog::LayerPropertiesAction),
    /// The Security Settings pane: Document Properties on its Security tab.
    ShowPermissionDetails,
    /// The Signatures pane: open this row's Signature Properties.
    ShowSignatureProperties(usize),
    /// A button in Signature Properties.
    SignatureProperties(crate::shell::chrome::signature_properties::SignaturePropertiesAction),
    Stamps(crate::shell::chrome::stamps_dialog::StampAction),
    Summary(crate::shell::chrome::summary_dialog::SummaryAction),
    Properties(crate::shell::chrome::properties_dialog::PropertiesAction),
    BookmarkTitle(crate::shell::chrome::bookmark_dialog::BookmarkTitleAction),
    File(crate::shell::chrome::file_dialogs::FileAction),
    /// The Print and Page Setup dialogs.
    Print(crate::shell::chrome::print_dialog::PrintAction),
    /// The skins panel and its roll back confirmation.
    Skins(crate::shell::skins::SkinsAction),
    /// The Organize Pages grid's toolbar and pages.
    Organize(crate::shell::organize::OrganizeAction),
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
    NavigationPanes,
    PaneStrip,
    QuickActions,
    PageControls,
    FindBar,
    Home,
    Dialog,
    ExportDialog,
    CombineDialog,
    SplitDialog,
    StampsDialog,
    SummaryDialog,
    DialogHeader,
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
        /// Tab: the next group of controls, entered at its first control.
        FocusNext,
        /// Shift-Tab: the previous group.
        FocusPrevious,
        /// Right or Down: the next control inside the group focus is in.
        FocusNextInGroup,
        /// Left or Up: the previous one.
        FocusPreviousInGroup,
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
        // Both axes move inside the group, because a group is a list either
        // way and a screen-reader user reaching for an arrow does not know
        // which way the surface happens to be drawn. A focused text field
        // keeps Left and Right for its caret by binding them in its own
        // context, which GPUI resolves ahead of this one; Up and Down are
        // what carries the ring out of the field.
        gpui::KeyBinding::new("right", FocusNextInGroup, context),
        gpui::KeyBinding::new("down", FocusNextInGroup, context),
        gpui::KeyBinding::new("left", FocusPreviousInGroup, context),
        gpui::KeyBinding::new("up", FocusPreviousInGroup, context),
        gpui::KeyBinding::new("enter", ActivateFocused, context),
        gpui::KeyBinding::new("space", ActivateFocused, context),
    ]);
}

/// How the platform's AccessKit handlers reach the shell.
///
/// They run on the main thread inside an `NSAccessibility` message with no
/// `App` to run anything in, so all they can do is ask for the shell to be
/// served. This dispatches that onto the main queue
/// (`ForegroundExecutor`, gpui `platform/mac/dispatcher.rs`), which macOS
/// runs whether or not the window is visible.
///
/// Asking for a frame instead would not do: gpui runs a window's display link
/// only while macOS reports the window visible (`platform/mac/window.rs`,
/// `window_did_change_occlusion_state`) and `App::refresh_windows` only marks
/// a window dirty, so a press on a window with something in front of it would
/// wait for the user to bring the window forward.
fn wake(window: &Window, cx: &mut gpui::Context<super::tabs::ShellFrame>) -> crate::a11y::Wake {
    let frame = cx.entity().downgrade();
    let window = window.to_async(cx);
    Box::new(move || {
        let frame = frame.clone();
        window
            .spawn(async move |cx| {
                let _ = cx.update(|window, app| {
                    let _ = frame.update(app, |frame, cx| frame.serve_accessibility(window, cx));
                });
            })
            .detach();
    })
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

    /// Tab: to the next group of controls.
    pub(in crate::shell) fn step(&mut self, step: crate::a11y::Step) -> bool {
        self.ring.step(step)
    }

    /// An arrow key: to the next control inside the group focus is in.
    pub(in crate::shell) fn step_within(&mut self, step: crate::a11y::Step) -> bool {
        self.ring.step_within(step)
    }

    /// Whether the shell should pay to describe a page's own words.
    ///
    /// Extracting them parses a content stream, on the thread that draws, on
    /// the first frame each page is visible for. That is a scroll's worth of
    /// work for something nobody is listening to until a screen reader
    /// attaches, so it waits until one has.
    pub(in crate::shell) fn wants_page_text(&self) -> bool {
        self.adapter
            .as_ref()
            .is_some_and(crate::a11y::Adapter::is_active)
    }

    /// A node out of the tree the shell last published, and the element that
    /// tree names as focused. Both read the update that left for the
    /// platform, not the description the shell built from.
    #[cfg(all(test, feature = "shell-test-support"))]
    pub(in crate::shell) fn published_node(
        &self,
        key: &gpui::ElementId,
    ) -> Option<accesskit::Node> {
        self.adapter.as_ref()?.published_node(key)
    }

    #[cfg(all(test, feature = "shell-test-support"))]
    pub(in crate::shell) fn published_focus(&self) -> Option<gpui::ElementId> {
        self.adapter.as_ref()?.published_focus()
    }

    /// How many stops each group holds, for the sweep that walks every one
    /// of them with the keyboard. Only used to bound the walk: a wrong
    /// answer makes the sweep fail, never pass.
    #[cfg(all(test, feature = "shell-test-support"))]
    pub(in crate::shell) fn group_sizes(&self) -> Vec<usize> {
        self.ring.groups().iter().map(Vec::len).collect()
    }

    /// Attach a client the way a screen reader attaches one, for a test with
    /// no screen reader to do it.
    #[cfg(all(test, feature = "shell-test-support"))]
    pub(in crate::shell) fn attach_client(&self) {
        self.adapter
            .as_ref()
            .expect("the adapter is attached on the first frame")
            .activate();
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
    /// `focused_field` is the text field GPUI's focus is in, if it is in
    /// one. It wins over the ring's own cursor, because that is where the
    /// keys are actually going: clicking into the find field has to move a
    /// screen reader's cursor there too. It is applied after the rebuild,
    /// since a field that has only just appeared is not in the old order.
    pub(in crate::shell) fn publish(
        &mut self,
        root: &Element,
        focused_field: Option<gpui::ElementId>,
        window: &Window,
        cx: &mut gpui::Context<super::tabs::ShellFrame>,
    ) {
        self.ring.rebuild(root);
        if let Some(key) = focused_field {
            self.ring.focus(&key);
        }
        self.actions = root
            .walk()
            .filter_map(|element| Some((element.key.clone(), element.activation.clone()?)))
            .collect();
        let adapter = self
            .adapter
            .get_or_insert_with(|| crate::a11y::Adapter::attach(window, wake(window, cx)));
        adapter.observe_window_active(window.is_window_active());
        adapter.publish(root, self.ring.focused());
    }

    /// What a screen reader has asked the shell to do and not been given.
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
