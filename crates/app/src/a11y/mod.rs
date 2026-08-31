//! The accessibility tree the shell publishes, and the platform adapter that
//! serves it.
//!
//! M1's spike (`docs/spikes/m1-shell-accesskit.md`) proved
//! `accesskit_macos::SubclassingAdapter` attaches to gpui's NSView with no
//! fork changes. This is that attach, driven by the real shell instead of a
//! hand-built three-node tree.

pub(crate) mod focus;
#[cfg(all(feature = "a11y-probe", target_os = "macos"))]
pub(crate) mod probe;
pub(crate) mod tree;

use std::cell::RefCell;
use std::rc::Rc;

use accesskit::{ActionRequest, TreeUpdate};
use gpui::{ElementId, Window};

pub(crate) use focus::{Ring, Step};
pub(crate) use tree::{Element, Ids, State};

/// What a screen reader asked the shell to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Request {
    /// VoiceOver pressed a control: run whatever a click would run.
    Activate,
    /// VoiceOver moved its cursor onto a control.
    Focus,
}

/// The shell's half of the accessibility contract.
///
/// Owns the id map, the window-focus tracker, and the platform adapter. The
/// tree it last published is kept so that a screen reader attaching between
/// frames gets the real shell rather than nothing.
pub(crate) struct Adapter {
    ids: Ids,
    shared: Rc<Shared>,
    platform: platform::Adapter,
}

/// State the platform adapter's handlers touch from outside a GPUI update.
///
/// The AccessKit handlers run on the main thread but inside an
/// `NSAccessibility` message, with no `App` in reach, so they can only read
/// and write plain cells. Everything they collect is drained on the next
/// frame.
#[derive(Default)]
struct Shared {
    /// The last tree the shell published, for `request_initial_tree`.
    published: RefCell<Option<TreeUpdate>>,
    requests: RefCell<Vec<ActionRequest>>,
}

impl Adapter {
    /// Attach to the window's native view.
    ///
    /// The spike flags the ordering as a constraint rather than a
    /// coincidence: the adapter has to exist before anything queries the
    /// view, so this is called on the shell's first render, which is the
    /// earliest point a `&Window` is in reach.
    pub(crate) fn attach(window: &Window) -> Self {
        let shared = Rc::new(Shared::default());
        let platform = platform::Adapter::attach(window, Rc::clone(&shared));
        Self {
            ids: Ids::default(),
            shared,
            platform,
        }
    }

    /// Publish a freshly built description.
    pub(crate) fn publish<A>(&mut self, root: &Element<A>, focus: Option<&ElementId>) {
        let update = tree::update(root, focus, &mut self.ids);
        *self.shared.published.borrow_mut() = Some(update.clone());
        self.platform.update(update);
    }

    /// Tell the adapter whether the window is the one the user is in.
    ///
    /// The M1 spike called this with `true` before the window was provably
    /// key, so its focus read was partly its own doing. This passes on what
    /// gpui reports and nothing else. AccessKit raises no event when the
    /// state has not changed, so it is safe to say every frame.
    pub(crate) fn observe_window_active(&mut self, active: bool) {
        self.platform.set_view_focused(active);
    }

    pub(crate) fn has_requests(&self) -> bool {
        !self.shared.requests.borrow().is_empty()
    }

    /// Everything a screen reader asked for since the last frame, resolved
    /// back to the elements it named.
    ///
    /// A request naming a node the shell no longer publishes is dropped: the
    /// tree moved on between the query and the press.
    pub(crate) fn take_requests(&mut self) -> Vec<(ElementId, Request)> {
        let requests = std::mem::take(&mut *self.shared.requests.borrow_mut());
        requests
            .into_iter()
            .filter_map(|request| {
                let key = self.ids.key_for(request.target_node)?.clone();
                let kind = match request.action {
                    accesskit::Action::Click => Request::Activate,
                    accesskit::Action::Focus => Request::Focus,
                    _ => return None,
                };
                Some((key, kind))
            })
            .collect()
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::rc::Rc;

    use accesskit::{ActionHandler, ActionRequest, ActivationHandler, TreeUpdate};
    use accesskit_macos::SubclassingAdapter;
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    use super::Shared;

    pub(super) struct Adapter {
        inner: Option<SubclassingAdapter>,
    }

    struct Activation(Rc<Shared>);

    impl ActivationHandler for Activation {
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            self.0.published.borrow().clone()
        }
    }

    struct Actions(Rc<Shared>);

    impl ActionHandler for Actions {
        fn do_action(&mut self, request: ActionRequest) {
            self.0.requests.borrow_mut().push(request);
        }
    }

    impl Adapter {
        pub(super) fn attach(window: &Window, shared: Rc<Shared>) -> Self {
            // GPUI's test platform has no native window and answers the
            // question by panicking, so a unit-test build attaches nothing.
            // That the attach works is `crates/app/tests/a11y_probe.rs`,
            // which drives the real binary against a real window.
            if cfg!(test) {
                return Self { inner: None };
            }
            // Fully qualified: `gpui::Window` has its own inherent
            // `window_handle` returning an `AnyWindowHandle`, which shadows
            // the trait method.
            let handle = match HasWindowHandle::window_handle(window) {
                Ok(handle) => handle,
                Err(error) => {
                    // Loud, because the alternative is an app that is
                    // silently unusable with a screen reader.
                    eprintln!("onionskin: no accessibility tree, the window exposes no native handle: {error}");
                    return Self { inner: None };
                }
            };
            let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
                eprintln!("onionskin: no accessibility tree, the window is not an AppKit window");
                return Self { inner: None };
            };
            // SAFETY: the pointer comes from gpui's own live NSView, and gpui
            // keeps the window alive for as long as the shell frame that owns
            // this adapter.
            let adapter = unsafe {
                SubclassingAdapter::new(
                    appkit.ns_view.as_ptr(),
                    Activation(Rc::clone(&shared)),
                    Actions(shared),
                )
            };
            Self {
                inner: Some(adapter),
            }
        }

        pub(super) fn update(&mut self, update: TreeUpdate) {
            let Some(adapter) = &mut self.inner else {
                return;
            };
            // The factory only runs when a client is attached, so a build
            // with no screen reader costs one move of an already-built
            // update.
            if let Some(events) = adapter.update_if_active(move || update) {
                events.raise();
            }
        }

        pub(super) fn set_view_focused(&mut self, focused: bool) {
            let Some(adapter) = &mut self.inner else {
                return;
            };
            if let Some(events) = adapter.update_view_focus_state(focused) {
                events.raise();
            }
        }
    }
}

/// Linux and Windows have their own AccessKit adapters; M2 ships the macOS
/// one, which is the platform the shell is developed and accepted on. The
/// tree itself is platform-independent and is built and tested everywhere,
/// so wiring another adapter is this module and nothing else.
#[cfg(not(target_os = "macos"))]
mod platform {
    use std::rc::Rc;

    use accesskit::TreeUpdate;
    use gpui::Window;

    use super::Shared;

    pub(super) struct Adapter;

    impl Adapter {
        pub(super) fn attach(_window: &Window, _shared: Rc<Shared>) -> Self {
            Self
        }

        pub(super) fn update(&mut self, _update: TreeUpdate) {}

        pub(super) fn set_view_focused(&mut self, _focused: bool) {}
    }
}
