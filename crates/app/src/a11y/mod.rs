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

impl Shared {
    /// Record what a screen reader asked for. The platform's action handler
    /// does nothing else, and the tests come through the same door.
    fn record(&self, request: ActionRequest) {
        #[cfg(all(feature = "a11y-probe", target_os = "macos"))]
        probe::record_delivery();
        self.requests.borrow_mut().push(request);
    }
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

    /// An adapter with the id map and the request queue but no platform
    /// behind it.
    ///
    /// GPUI's test platform has no native window to attach to, so a test
    /// about which element a request resolves to cannot go through
    /// [`Adapter::attach`].
    #[cfg(test)]
    pub(crate) fn detached() -> Self {
        Self {
            ids: Ids::default(),
            shared: Rc::new(Shared::default()),
            platform: platform::Adapter::detached(),
        }
    }

    /// Deliver an action request the way the platform's handler delivers one.
    ///
    /// Named by element rather than by node id, because the id is this
    /// adapter's own and a caller has no other way to learn it.
    #[cfg(test)]
    pub(crate) fn deliver(&mut self, key: &ElementId, action: accesskit::Action) {
        let target_node = self.ids.id_for(key);
        self.shared.record(ActionRequest {
            action,
            target_tree: accesskit::TreeId::ROOT,
            target_node,
            data: None,
        });
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
            self.0.record(request);
        }
    }

    impl Adapter {
        #[cfg(test)]
        pub(super) fn detached() -> Self {
            Self { inner: None }
        }

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
        #[cfg(test)]
        pub(super) fn detached() -> Self {
            Self
        }

        pub(super) fn attach(_window: &Window, _shared: Rc<Shared>) -> Self {
            Self
        }

        pub(super) fn update(&mut self, _update: TreeUpdate) {}

        pub(super) fn set_view_focused(&mut self, _focused: bool) {}
    }
}

#[cfg(test)]
mod tests {
    use accesskit::{Action, Role};

    use super::*;

    /// A chrome-shaped tree: two controls a screen reader can press.
    fn tree() -> Element<()> {
        Element::new("window", Role::Window, "Onionskin").with_children(vec![
            Element::new("previous-page", Role::Button, "Previous Page").with_activation(()),
            Element::new("zoom-in", Role::Button, "Zoom In").with_activation(()),
        ])
    }

    /// The whole operate half of the contract: a press has to come back out
    /// naming the element it was aimed at and the thing the shell does about
    /// it. Nothing else in the shell converts an `ActionRequest` into a
    /// `Request`.
    #[test]
    fn a_request_resolves_to_the_element_it_named_and_the_thing_to_do_to_it() {
        let mut adapter = Adapter::detached();
        adapter.publish(&tree(), None);

        adapter.deliver(&"previous-page".into(), Action::Click);
        adapter.deliver(&"zoom-in".into(), Action::Focus);

        assert_eq!(
            adapter.take_requests(),
            vec![
                ("previous-page".into(), Request::Activate),
                ("zoom-in".into(), Request::Focus),
            ]
        );
    }

    /// The queue is drained, not read: a press the shell has already run must
    /// not run again on the next frame.
    #[test]
    fn taking_the_requests_empties_the_queue() {
        let mut adapter = Adapter::detached();
        adapter.publish(&tree(), None);
        adapter.deliver(&"zoom-in".into(), Action::Click);

        assert!(adapter.has_requests());
        assert_eq!(adapter.take_requests().len(), 1);
        assert!(!adapter.has_requests());
        assert_eq!(adapter.take_requests(), Vec::new());
    }

    /// The tree moved on between the query and the press: the node the
    /// request names is not published any more, so there is nothing to run
    /// and running the element that took its place would be worse than
    /// running nothing.
    #[test]
    fn a_request_naming_a_node_the_tree_has_dropped_is_not_run() {
        let mut adapter = Adapter::detached();
        adapter.publish(&tree(), None);
        let stale = adapter.ids.id_for(&"zoom-in".into());

        let smaller: Element<()> = Element::new("window", Role::Window, "Onionskin").child(
            Element::new("previous-page", Role::Button, "Previous Page").with_activation(()),
        );
        adapter.publish(&smaller, None);
        adapter.shared.record(ActionRequest {
            action: Action::Click,
            target_tree: accesskit::TreeId::ROOT,
            target_node: stale,
            data: None,
        });

        assert!(adapter.has_requests());
        assert_eq!(adapter.take_requests(), Vec::new());
    }

    /// AccessKit's action set is much wider than the two the shell publishes.
    /// Anything else is dropped rather than guessed at.
    #[test]
    fn an_action_the_shell_never_offered_is_dropped() {
        let mut adapter = Adapter::detached();
        adapter.publish(&tree(), None);

        adapter.deliver(&"zoom-in".into(), Action::ScrollIntoView);

        assert_eq!(adapter.take_requests(), Vec::new());
    }
}
