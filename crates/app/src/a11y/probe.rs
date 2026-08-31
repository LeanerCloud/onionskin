//! Reads the published accessibility tree back off the window's own NSView.
//!
//! This is M1's `a11y_spike` promoted from a hand-built three-node demo to a
//! probe of the real shell, and from a binary someone reads the output of to
//! something `crates/app/tests/a11y_probe.rs` asserts on.
//!
//! It sends the same `NSAccessibility` messages VoiceOver sends, straight to
//! the view. That needs no accessibility grant, because it never goes through
//! the AX server. It is therefore **narrower than a real VoiceOver session,
//! not stronger**: it proves the adapter serves the tree the shell built, and
//! says nothing about cross-process marshalling, notification delivery or
//! what a user actually hears. See `docs/spikes/m2-voiceover-acceptance.md`.
//!
//! Compiled only under the `a11y-probe` feature, so a normal build of the app
//! carries none of it.

use std::ffi::{c_char, c_void, CStr};
use std::time::{Duration, Instant};

use gpui::{App, Timer};
use objc2::encode::{Encode, Encoding};
use objc2::msg_send;
use objc2::runtime::AnyObject;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// `CGRect`, as `accessibilityFrame` returns it.
///
/// Declared here rather than pulled from `objc2-foundation`: the probe needs
/// one struct layout and nothing else that crate carries.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct Rect {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

// SAFETY: the layout matches `CGRect`, which is four `CGFloat`s, and
// `CGFloat` is `f64` on every platform this builds for.
unsafe impl Encode for Rect {
    const ENCODING: Encoding = Encoding::Struct(
        "CGRect",
        &[
            Encoding::Struct("CGPoint", &[Encoding::Double, Encoding::Double]),
            Encoding::Struct("CGSize", &[Encoding::Double, Encoding::Double]),
        ],
    );
}

/// How often to look again while waiting for the shell to settle.
const POLL: Duration = Duration::from_millis(100);
/// How long to keep looking before reporting whatever there is.
///
/// A deadline rather than a sleep: a machine under load takes longer to open
/// a window and lay a page out, and a fixed wait either flakes there or
/// wastes the time everywhere else. Reaching the deadline is not an error
/// here; it prints the tree as it stands and lets the assertions say what is
/// missing.
const DEADLINE: Duration = Duration::from_secs(20);

/// Read the tree once the shell has settled, print it, and quit.
///
/// Called from `shell::run` after the window is activated, so the probe walks
/// the real boot path rather than a copy of it.
pub(crate) fn arm(cx: &mut App) {
    cx.spawn(async move |cx| {
        let started = Instant::now();
        loop {
            Timer::after(POLL).await;
            // Reading the tree at all is what tells the shell an
            // accessibility client exists, and the shell only pays for the
            // page's text once something is listening. So the first read is
            // also the request, and the text arrives on a later frame.
            let settled = cx
                .update(|cx| {
                    let Some(view) = view_pointer(cx) else {
                        return false;
                    };
                    // Ask for the frame that will carry the text.
                    cx.refresh_windows();
                    unsafe { has_page_text(view) }
                })
                .unwrap_or(false);
            if settled || started.elapsed() >= DEADLINE {
                break;
            }
        }
        let reported = cx.update(|cx| {
            match view_pointer(cx) {
                Some(view) => println!("{}", unsafe { dump(view) }),
                None => println!("{{\"error\":\"the shell opened no window with a native view\"}}"),
            }
            cx.quit();
        });
        if reported.is_err() {
            // Exiting rather than leaving whatever is reading this waiting on
            // a process that will never print anything.
            eprintln!("onionskin: the probe could not reach the app to read its tree");
            std::process::exit(1);
        }
    })
    .detach();
}

/// The identifier the shell gives a run of text on a page.
const PAGE_TEXT: &str = "-text-";

/// Whether the tree carries a page's own words yet.
///
/// The last thing to arrive: a page has to be laid out before its words have
/// anywhere to be, and the text is only extracted once a client has asked.
/// When this is true everything else already is.
///
/// Matched on the identifier rather than on the role, because the chrome's
/// own labels are static text too and would end the wait early.
///
/// # Safety
/// `view` must be a live `NSView` and this must run on the main thread.
unsafe fn has_page_text(view: *mut c_void) -> bool {
    let mut identifiers = Vec::new();
    unsafe { collect_identifiers(view.cast::<AnyObject>(), &mut identifiers) };
    identifiers.iter().any(|id| id.contains(PAGE_TEXT))
}

/// # Safety
/// `element` must be a live accessibility element.
unsafe fn collect_identifiers(element: *mut AnyObject, out: &mut Vec<String>) {
    let children: *mut AnyObject = unsafe { msg_send![element, accessibilityChildren] };
    if children.is_null() {
        return;
    }
    let count: usize = unsafe { msg_send![children, count] };
    for index in 0..count {
        let child: *mut AnyObject = unsafe { msg_send![children, objectAtIndex: index] };
        out.push(unsafe { ns_string(msg_send![child, accessibilityIdentifier]) });
        unsafe { collect_identifiers(child, out) };
    }
}

/// The `NSView` gpui is drawing the shell into.
fn view_pointer(cx: &mut App) -> Option<*mut c_void> {
    let window = cx.windows().into_iter().next()?;
    window
        .update(cx, |_, window, _| {
            // Fully qualified: `gpui::Window` has its own inherent
            // `window_handle`, which shadows the trait method.
            let handle = HasWindowHandle::window_handle(window).ok()?;
            match handle.as_raw() {
                RawWindowHandle::AppKit(appkit) => Some(appkit.ns_view.as_ptr()),
                _ => None,
            }
        })
        .ok()
        .flatten()
}

/// The tree, as JSON, plus the fork check that has to run on every bump.
///
/// # Safety
/// `view` must be a live `NSView` and this must run on the main thread.
unsafe fn dump(view: *mut c_void) -> String {
    let view = view.cast::<AnyObject>();
    let class: *mut AnyObject = unsafe { msg_send![view, class] };
    let mut out = String::from("{\n");
    out.push_str(&format!(
        "  \"viewClass\": {},\n",
        json_string(&unsafe { ns_string(msg_send![class, description]) })
    ));
    // The class is reported separately from the answer: a fork that renamed
    // its view class would otherwise make the selector check pass by looking
    // at nothing, which is the one way this check can rot.
    let fork_class = objc2::runtime::AnyClass::get(FORK_VIEW_CLASS);
    out.push_str(&format!(
        "  \"forkViewClass\": {},\n",
        match fork_class {
            Some(_) => json_string(FORK_VIEW_CLASS),
            None => "null".to_owned(),
        }
    ));
    out.push_str(&format!(
        "  \"forkDefinesAccessibilitySelectors\": {},\n",
        fork_class.is_some_and(defines_accessibility_selectors)
    ));
    out.push_str("  \"nodes\": [\n");
    let mut nodes = Vec::new();
    unsafe { collect(view, 0, &mut nodes) };
    out.push_str(&nodes.join(",\n"));
    out.push_str("\n  ],\n");
    let focused: *mut AnyObject = unsafe { msg_send![view, accessibilityFocusedUIElement] };
    out.push_str(&format!(
        "  \"focused\": {}\n",
        if focused.is_null() {
            "null".to_owned()
        } else {
            unsafe { describe(focused, 0) }
        }
    ));
    out.push_str("}");
    out
}

/// What the pinned gpui fork calls the view it draws into
/// (`src/platform/mac/window.rs`, `build_classes`).
const FORK_VIEW_CLASS: &str = "GPUIView";

/// Whether gpui's own view class defines any accessibility selector.
///
/// The spike's standing caveat: a fork rebase that adds accessibility
/// selectors to gpui's view class would collide with AccessKit's runtime
/// subclass, silently shadowing one of them. This asks only about the methods
/// that class defines itself, and not about `NSView`, which conforms to
/// `NSAccessibility` and would answer yes to `respondsToSelector:` for
/// everything.
fn defines_accessibility_selectors(class: &objc2::runtime::AnyClass) -> bool {
    class
        .instance_methods()
        .iter()
        .any(|method| method.name().name().starts_with("accessibility"))
}

/// # Safety
/// `element` must be a live accessibility element.
unsafe fn collect(element: *mut AnyObject, depth: usize, out: &mut Vec<String>) {
    let children: *mut AnyObject = unsafe { msg_send![element, accessibilityChildren] };
    if children.is_null() {
        return;
    }
    let count: usize = unsafe { msg_send![children, count] };
    for index in 0..count {
        let child: *mut AnyObject = unsafe { msg_send![children, objectAtIndex: index] };
        out.push(unsafe { describe(child, depth + 1) });
        unsafe { collect(child, depth + 1, out) };
    }
}

/// One node, as JSON.
///
/// AccessKit maps `Node::label` to `accessibilityTitle` and never sets
/// `accessibilityLabel`, so anything reading a label back has to use the
/// former. `accessibilityIdentifier` carries the element's own key, which is
/// how a test names a control without matching on copy.
///
/// # Safety
/// `element` must be a live accessibility element.
unsafe fn describe(element: *mut AnyObject, depth: usize) -> String {
    let value: *mut AnyObject = unsafe { msg_send![element, accessibilityValue] };
    let enabled: bool = unsafe { msg_send![element, isAccessibilityEnabled] };
    // In screen coordinates, which is what AccessKit converts a node's own
    // bounds into. A node the shell never measured reports a zero rectangle,
    // which is how a missing rectangle is told apart from a real one.
    let frame: Rect = unsafe { msg_send![element, accessibilityFrame] };
    format!(
        "    {{\"depth\": {depth}, \"role\": {}, \"subrole\": {}, \"roleDescription\": {}, \"title\": {}, \"label\": {}, \"identifier\": {}, \"help\": {}, \"value\": {}, \"enabled\": {}, \"frame\": [{}, {}, {}, {}]}}",
        json_string(&unsafe { ns_string(msg_send![element, accessibilityRole]) }),
        json_string(&unsafe { ns_string(msg_send![element, accessibilitySubrole]) }),
        json_string(&unsafe { ns_string(msg_send![element, accessibilityRoleDescription]) }),
        json_string(&unsafe { ns_string(msg_send![element, accessibilityTitle]) }),
        json_string(&unsafe { ns_string(msg_send![element, accessibilityLabel]) }),
        json_string(&unsafe { ns_string(msg_send![element, accessibilityIdentifier]) }),
        json_string(&unsafe { ns_string(msg_send![element, accessibilityHelp]) }),
        if value.is_null() {
            "null".to_owned()
        } else {
            // A value is not always a string: a checkbox's is a boolean
            // object, so it is read through `description`, which every
            // Objective-C object answers.
            json_string(&unsafe { ns_string(msg_send![value, description]) })
        },
        enabled,
        frame.x,
        frame.y,
        frame.width,
        frame.height,
    )
}


/// # Safety
/// `s` must be nil or a valid `NSString`.
unsafe fn ns_string(s: *mut AnyObject) -> String {
    if s.is_null() {
        return String::new();
    }
    let utf8: *const c_char = unsafe { msg_send![s, UTF8String] };
    if utf8.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(utf8) }
        .to_string_lossy()
        .into_owned()
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for character in value.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
