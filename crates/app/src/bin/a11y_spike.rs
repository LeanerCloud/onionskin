//! M1 spike (d): can AccessKit be attached to a GPUI window from outside the
//! fork, so VoiceOver reads a focused element?
//!
//! The bet: the fork already implements `raw_window_handle::HasWindowHandle`
//! for `gpui::Window` (src/window.rs), returning an `AppKitWindowHandle` whose
//! `ns_view` is the real NSView. `accesskit_macos::SubclassingAdapter` exists
//! exactly for views the caller did not create, so no fork patch should be
//! needed for the attach itself.
//!
//! Throwaway-permitted spike code. The tree here is hand-built, not derived
//! from any real element tree.

use accesskit::{
    ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Rect, Role, Tree, TreeId,
    TreeUpdate,
};
use accesskit_macos::SubclassingAdapter;
use gpui::{
    div, px, size, App, AppContext as _, Application, Bounds, Context, InteractiveElement as _,
    IntoElement, ParentElement as _, Render, Styled as _, TitlebarOptions, Window, WindowBounds,
    WindowOptions,
};
use objc2::{msg_send, runtime::AnyObject};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::ffi::{c_char, c_void, CStr};

const ROOT: NodeId = NodeId(0);
const PAGE: NodeId = NodeId(1);
const BUTTON: NodeId = NodeId(2);

fn initial_tree() -> TreeUpdate {
    let mut root = Node::new(Role::Window);
    root.set_label("Onionskin");
    root.set_children(vec![PAGE, BUTTON]);

    let mut page = Node::new(Role::Document);
    page.set_label("Page 1 of 1, Onionskin accessibility spike");
    page.set_bounds(Rect::new(0.0, 0.0, 900.0, 640.0));

    let mut button = Node::new(Role::Button);
    button.set_label("Zoom in");
    button.set_bounds(Rect::new(0.0, 640.0, 120.0, 700.0));

    TreeUpdate {
        nodes: vec![(ROOT, root), (PAGE, page), (BUTTON, button)],
        tree: Some(Tree::new(ROOT)),
        tree_id: TreeId::ROOT,
        focus: PAGE,
    }
}

struct Activation;

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        eprintln!("a11y: accessibility client requested the initial tree");
        Some(initial_tree())
    }
}

struct Actions;

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        eprintln!(
            "a11y: action {:?} on {:?}",
            request.action, request.target_node
        );
    }
}

/// Read an `NSString` return value back as a Rust string.
///
/// # Safety
/// `s` must be nil or a valid `NSString`.
unsafe fn ns_string(s: *mut AnyObject) -> String {
    if s.is_null() {
        return "<nil>".into();
    }
    let utf8: *const c_char = unsafe { msg_send![s, UTF8String] };
    if utf8.is_null() {
        return "<nil utf8>".into();
    }
    unsafe { CStr::from_ptr(utf8) }
        .to_string_lossy()
        .into_owned()
}

/// Send the same `NSAccessibility` messages VoiceOver sends, straight to the
/// view. This is the whole verification: it needs no accessibility-permission
/// grant because it never goes through the AX server, and a non-nil labelled
/// child can only come from the adapter's subclass.
///
/// # Safety
/// `view` must be a valid `NSView` pointer, and this must run on the main
/// thread.
unsafe fn dump_accessibility(view: *mut c_void) {
    let view = view as *mut AnyObject;
    let class: *mut AnyObject = unsafe { msg_send![view, class] };
    println!("probe: NSView class after attach = {}", unsafe {
        ns_string(msg_send![class, description])
    });

    let children: *mut AnyObject = unsafe { msg_send![view, accessibilityChildren] };
    if children.is_null() {
        println!("probe: accessibilityChildren = nil (NO TREE)");
        return;
    }
    let count: usize = unsafe { msg_send![children, count] };
    println!("probe: accessibilityChildren count = {count}");
    for i in 0..count {
        let child: *mut AnyObject = unsafe { msg_send![children, objectAtIndex: i] };
        describe(child, "  child");
        let grandkids: *mut AnyObject = unsafe { msg_send![child, accessibilityChildren] };
        if !grandkids.is_null() {
            let n: usize = unsafe { msg_send![grandkids, count] };
            for j in 0..n {
                let g: *mut AnyObject = unsafe { msg_send![grandkids, objectAtIndex: j] };
                describe(g, "    grandchild");
            }
        }
    }

    let focused: *mut AnyObject = unsafe { msg_send![view, accessibilityFocusedUIElement] };
    if focused.is_null() {
        println!("probe: accessibilityFocusedUIElement = nil");
    } else {
        describe(focused, "  focused");
    }
}

/// AccessKit exposes `Node::label` as `accessibilityTitle` on macOS, so print
/// both that and the role description VoiceOver announces alongside it.
fn describe(element: *mut AnyObject, prefix: &str) {
    println!(
        "probe: {prefix} role={} title={} roleDescription={}",
        unsafe { ns_string(msg_send![element, accessibilityRole]) },
        unsafe { ns_string(msg_send![element, accessibilityTitle]) },
        unsafe { ns_string(msg_send![element, accessibilityRoleDescription]) },
    );
}

struct Probe {
    adapter: Option<SubclassingAdapter>,
}

impl Probe {
    /// Attach on the first render, which is the earliest point a `&Window`
    /// (and therefore the NSView) is in reach.
    fn attach(&mut self, window: &Window) {
        if self.adapter.is_some() {
            return;
        }
        // Fully qualified: gpui::Window has its own inherent `window_handle`
        // returning an `AnyWindowHandle`, which shadows the trait method.
        let handle = HasWindowHandle::window_handle(window)
            .expect("gpui window exposes a raw window handle");
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            panic!("expected an AppKit window handle on macOS");
        };
        let ns_view = appkit.ns_view.as_ptr();
        eprintln!("a11y: attaching SubclassingAdapter to NSView {ns_view:?}");
        // SAFETY: `ns_view` comes from gpui's own live NSView and gpui keeps
        // the window alive for the process lifetime of this spike.
        let mut adapter = unsafe { SubclassingAdapter::new(ns_view, Activation, Actions) };
        if let Some(events) = adapter.update_view_focus_state(true) {
            events.raise();
        }
        self.adapter = Some(adapter);
        unsafe { dump_accessibility(ns_view) };
    }
}

impl Render for Probe {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.attach(window);
        div()
            .id("page")
            .size_full()
            .bg(gpui::rgb(0xf4f4f6))
            .child("Onionskin AccessKit probe")
    }
}

fn main() {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(900.0), px(700.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("Onionskin A11y Probe".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |_window, cx| cx.new(|_cx| Probe { adapter: None }),
        )
        .expect("failed to open window");
        cx.activate(true);
    });
}
