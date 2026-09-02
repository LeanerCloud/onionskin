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

use std::cell::{Cell, RefCell};
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

/// What the shell does when a screen reader touches it outside a frame.
///
/// The AccessKit handlers have no `App` in reach, so they cannot run anything
/// themselves. This is how they ask the shell to serve them: it has to reach
/// the main run loop directly, because the shell cannot wait for a frame that
/// a window macOS reports as not visible will never draw.
pub(crate) type Wake = Box<dyn Fn()>;

/// State the platform adapter's handlers touch from outside a GPUI update.
///
/// The AccessKit handlers run on the main thread but inside an
/// `NSAccessibility` message, with no `App` in reach, so they can only read
/// and write plain cells. Everything they collect is drained by the wake they
/// schedule.
struct Shared {
    /// The last tree the shell published, for `request_initial_tree`.
    published: RefCell<Option<TreeUpdate>>,
    requests: RefCell<Vec<ActionRequest>>,
    /// Whether a client has asked for the tree. Until one has, nothing reads
    /// what the shell publishes, and the expensive half of building it is
    /// skipped.
    active: Cell<bool>,
    wake: Option<Wake>,
}

impl Shared {
    fn new(wake: Option<Wake>) -> Self {
        Self {
            published: RefCell::new(None),
            requests: RefCell::new(Vec::new()),
            active: Cell::new(false),
            wake,
        }
    }

    /// Record what a screen reader asked for. The platform's action handler
    /// does nothing else, and the tests come through the same door.
    ///
    /// Woken only when the queue was empty: a burst of requests is one drain,
    /// and the drain takes all of them.
    fn record(&self, request: ActionRequest) {
        #[cfg(all(feature = "a11y-probe", target_os = "macos"))]
        probe::record_delivery();
        let idle = {
            let mut requests = self.requests.borrow_mut();
            let idle = requests.is_empty();
            requests.push(request);
            idle
        };
        if idle {
            self.wake();
        }
    }

    /// A client has attached. The shell has more to say than it published
    /// while nobody was listening, so ask it to say it.
    fn activate(&self) {
        if self.active.replace(true) {
            return;
        }
        self.wake();
    }

    fn wake(&self) {
        if let Some(wake) = &self.wake {
            wake();
        }
    }
}

impl Adapter {
    /// Attach to the window's native view.
    ///
    /// The spike flags the ordering as a constraint rather than a
    /// coincidence: the adapter has to exist before anything queries the
    /// view, so this is called on the shell's first render, which is the
    /// earliest point a `&Window` is in reach.
    pub(crate) fn attach(window: &Window, wake: Wake) -> Self {
        let shared = Rc::new(Shared::new(Some(wake)));
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
            shared: Rc::new(Shared::new(None)),
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
        // A push the platform accepted means a client took it, which is the
        // other door into the active state: AccessKit only asks for an
        // initial tree once, and a client that arrived while the shell had
        // published nothing comes back through this one.
        if self.platform.update(update) {
            self.shared.activate();
        }
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

    /// Only the drain's own test asks: the shell is woken when there is
    /// something to take, rather than looking for itself.
    #[cfg(test)]
    pub(crate) fn has_requests(&self) -> bool {
        !self.shared.requests.borrow().is_empty()
    }

    /// Whether anything is reading what the shell publishes.
    ///
    /// False until a client asks for the tree, which is how the shell knows
    /// not to pay for the parts of the description nobody would hear.
    pub(crate) fn is_active(&self) -> bool {
        self.shared.active.get()
    }

    /// Attach a client the way the platform attaches one, for a test that
    /// has no screen reader to do it.
    #[cfg(test)]
    pub(crate) fn activate(&self) {
        self.shared.activate();
    }

    /// A node out of the tree the shell last published, as a client reading
    /// the tree would find it.
    ///
    /// The published tree, not the description the shell built: a shell that
    /// runs what a screen reader asked for and then publishes nothing leaves
    /// the reader announcing the state before the press.
    #[cfg(test)]
    pub(crate) fn published_node(&self, key: &ElementId) -> Option<accesskit::Node> {
        let id = self.ids.assigned(key)?;
        self.shared
            .published
            .borrow()
            .as_ref()?
            .nodes
            .iter()
            .find(|(node, _)| *node == id)
            .map(|(_, node)| node.clone())
    }

    /// The element the published tree names as focused, which is what moves a
    /// screen reader's cursor. The ring's own answer is not the same thing:
    /// only this one leaves the process.
    #[cfg(test)]
    pub(crate) fn published_focus(&self) -> Option<ElementId> {
        let focus = self.shared.published.borrow().as_ref()?.focus;
        self.ids.key_for(focus).cloned()
    }

    /// Everything a screen reader has asked for and not been given, resolved
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
        /// AccessKit calls this the first time a client queries the view, and
        /// never again: the adapter is Active from here on
        /// (accesskit_macos 0.26.3 `adapter.rs`, `get_or_init_context`). It
        /// is therefore the shell's only notice that anything is listening.
        fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
            self.0.activate();
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

        /// Push the tree, and answer whether a client took it.
        pub(super) fn update(&mut self, update: TreeUpdate) -> bool {
            let Some(adapter) = &mut self.inner else {
                return false;
            };
            // The factory only runs when a client is attached, so a build
            // with no screen reader costs one move of an already-built
            // update.
            let Some(events) = adapter.update_if_active(move || update) else {
                return false;
            };
            events.raise();
            true
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
///
/// Two things it owes [`Shared`] when it is wired: call `activate` when a
/// client first asks for the tree, or the shell keeps skipping the page text
/// nobody has asked for, and answer `update` with whether the push landed.
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

        pub(super) fn update(&mut self, _update: TreeUpdate) -> bool {
            false
        }

        pub(super) fn set_view_focused(&mut self, _focused: bool) {}
    }
}

#[cfg(test)]
mod tests {
    use accesskit::{Action, NodeId, Role};

    use super::*;

    fn request(target_node: NodeId, action: Action) -> ActionRequest {
        ActionRequest {
            action,
            target_tree: accesskit::TreeId::ROOT,
            target_node,
            data: None,
        }
    }

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

    /// The queue is drained by a wake the handler schedules, not by the next
    /// frame: a window macOS reports as not visible draws no frame, and a
    /// request that waits for one never runs. One wake per burst, and a fresh
    /// one once the queue has been emptied.
    #[test]
    fn a_request_wakes_the_shell_once_per_burst() {
        let woken = Rc::new(Cell::new(0_usize));
        let counter = Rc::clone(&woken);
        let shared = Shared::new(Some(Box::new(move || {
            counter.set(counter.get() + 1);
        })));

        shared.record(request(NodeId(1), accesskit::Action::Click));
        shared.record(request(NodeId(2), accesskit::Action::Click));
        assert_eq!(woken.get(), 1, "a burst of presses woke the shell twice");

        shared.requests.borrow_mut().clear();
        shared.record(request(NodeId(3), accesskit::Action::Click));
        assert_eq!(woken.get(), 2, "a press after the drain did not wake the shell");
    }

    /// The shell has no other notice that a screen reader has arrived, and it
    /// publishes less while nobody is listening, so the attach has to wake it
    /// to say the rest.
    #[test]
    fn a_client_asking_for_the_tree_makes_the_adapter_active_and_wakes_the_shell() {
        let woken = Rc::new(Cell::new(0_usize));
        let counter = Rc::clone(&woken);
        let shared = Shared::new(Some(Box::new(move || {
            counter.set(counter.get() + 1);
        })));

        assert!(!shared.active.get());
        shared.activate();
        assert!(shared.active.get());
        assert_eq!(woken.get(), 1);

        shared.activate();
        assert_eq!(woken.get(), 1, "a second client woke the shell again");
    }

    #[test]
    fn an_adapter_nothing_has_asked_is_not_active() {
        let mut adapter = Adapter::detached();
        adapter.publish(&tree(), None);

        assert!(!adapter.is_active());

        adapter.activate();
        assert!(adapter.is_active());
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
