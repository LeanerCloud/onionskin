//! The accessibility tree, read back off the real window's own NSView.
//!
//! M1's spike proved `SubclassingAdapter` attaches by printing a hand-built
//! three-node tree for a human to read. This runs the app, sends the same
//! `NSAccessibility` messages VoiceOver sends, and asserts on what comes
//! back, so a regression fails the build instead of being noticed.
//!
//! What this proves: the adapter attaches to gpui's view, and the platform
//! serves the tree the shell built, with the roles, names and states the
//! shell gave it.
//!
//! What this does not prove: anything past the view. It never goes through
//! the AX server, so cross-process marshalling, notification delivery and
//! what a user actually hears are all untested here. That is the
//! user-gated VoiceOver session in `docs/spikes/m2-voiceover-acceptance.md`.
//!
//! Needs a window server, so it opens a window on the machine running it.
#![cfg(all(feature = "a11y-probe", target_os = "macos"))]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

/// One node as the probe reported it.
type Node = BTreeMap<String, serde_json::Value>;

struct Tree {
    view_class: String,
    /// The gpui view class the probe found, or `None` when the fork no longer
    /// has one by that name.
    fork_view_class: Option<String>,
    fork_defines_accessibility_selectors: bool,
    /// Whether the platform accepted the press this run asked for, or `None`
    /// when it asked for none.
    pressed: Option<bool>,
    nodes: Vec<Node>,
}

impl Tree {
    /// The node the shell gave this element id, by its
    /// `accessibilityIdentifier`. Matching on the id rather than the label
    /// keeps these assertions honest when copy changes.
    fn by_id(&self, id: &str) -> &Node {
        self.nodes
            .iter()
            .find(|node| node["identifier"] == id)
            .unwrap_or_else(|| {
                panic!(
                    "no node identified as {id:?}; the tree carried {:?}",
                    self.ids()
                )
            })
    }

    fn ids(&self) -> Vec<String> {
        self.nodes
            .iter()
            .filter_map(|node| node["identifier"].as_str())
            .filter(|id| !id.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    }

    fn field<'a>(&self, node: &'a Node, key: &str) -> &'a str {
        node[key].as_str().unwrap_or_default()
    }

    /// Whether the platform reports the node as usable.
    fn enabled(&self, node: &Node) -> bool {
        node["enabled"]
            .as_bool()
            .expect("every node reports whether it is enabled")
    }

    /// The node's rectangle on screen: x, y, width, height.
    fn frame(&self, node: &Node) -> [f64; 4] {
        let frame = node["frame"]
            .as_array()
            .expect("every node reports a frame");
        [
            frame[0].as_f64().unwrap(),
            frame[1].as_f64().unwrap(),
            frame[2].as_f64().unwrap(),
            frame[3].as_f64().unwrap(),
        ]
    }
}

fn seed(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../corpus/seeds")
        .join(name)
}

/// Run the app in probe mode over a seed, once per seed for the whole suite.
fn probe(name: &str) -> &'static Tree {
    probed(name, None)
}

/// The same, having first pressed the named control the way VoiceOver
/// presses one.
fn probe_pressing(name: &str, control: &str) -> &'static Tree {
    probed(name, Some(control))
}

/// One run per seed and press, shared by every test that asks for it.
///
/// Once, because every run opens a real window and because the app writes its
/// recents file: a dozen concurrent copies would race each other over it. The
/// lock is held across the run, so the copies are sequential as well as
/// shared.
fn probed(name: &str, press: Option<&str>) -> &'static Tree {
    static PROBED: OnceLock<Mutex<BTreeMap<String, &'static Tree>>> = OnceLock::new();
    let probed = PROBED.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut probed = probed.lock().expect("the probe cache is not poisoned");
    let key = format!("{name} pressing {}", press.unwrap_or("nothing"));
    if let Some(tree) = probed.get(&key) {
        return tree;
    }
    let tree: &'static Tree = Box::leak(Box::new(run_probe(name, press)));
    probed.insert(key, tree);
    tree
}

fn run_probe(name: &str, press: Option<&str>) -> Tree {
    // The probe is a build of the real app, so it reads and writes the same
    // three config files. Point it at a directory of its own so it neither
    // reads the developer's settings nor records these seeds in their
    // recents list.
    let config = std::env::temp_dir().join("onionskin-a11y-probe");
    std::fs::create_dir_all(&config).expect("the probe's config directory is writable");
    let mut command = Command::new(env!("CARGO_BIN_EXE_onionskin"));
    command.arg(seed(name)).env("XDG_CONFIG_HOME", &config);
    if let Some(control) = press {
        command.env("ONIONSKIN_A11Y_PRESS", control);
    }
    let output = command.output().expect("the probe build of the app runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let start = stdout.find('{').unwrap_or_else(|| {
        panic!(
            "the probe printed no tree.\nstdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    let parsed: serde_json::Value =
        serde_json::from_str(&stdout[start..]).expect("the probe prints one JSON object");
    assert!(
        output.status.success(),
        "the probe exited with {}",
        output.status
    );
    Tree {
        view_class: parsed["viewClass"].as_str().unwrap_or_default().to_owned(),
        fork_view_class: parsed["forkViewClass"].as_str().map(ToOwned::to_owned),
        fork_defines_accessibility_selectors: parsed["forkDefinesAccessibilitySelectors"]
            .as_bool()
            .expect("the probe reports the fork check"),
        pressed: parsed["pressed"].as_bool(),
        nodes: parsed["nodes"]
            .as_array()
            .expect("the probe reports a node list")
            .iter()
            .map(|node| {
                node.as_object()
                    .expect("each node is an object")
                    .clone()
                    .into_iter()
                    .collect()
            })
            .collect(),
    }
}

/// The whole bet: AccessKit subclasses gpui's view from application code,
/// with no fork changes.
#[test]
fn the_adapter_subclasses_the_shell_window_s_own_view() {
    let tree = probe("hello.pdf");

    assert_eq!(tree.view_class, "AccessKitSubclassOfGPUIView");
    assert!(!tree.nodes.is_empty());
}

/// The spike's standing caveat, as a test rather than a note: a fork rebase
/// that adds accessibility selectors to gpui's view class would collide with
/// the runtime subclass. This fails on the bump that introduces one.
///
/// The class has to be found first, or a fork that merely renamed it would
/// make this pass by inspecting nothing.
#[test]
fn the_pinned_fork_still_defines_no_accessibility_selectors_of_its_own() {
    let tree = probe("hello.pdf");

    assert_eq!(
        tree.fork_view_class.as_deref(),
        Some("GPUIView"),
        "the fork no longer has a view class by that name, so the selector check inspected nothing"
    );
    assert!(!tree.fork_defines_accessibility_selectors);
}

/// The rectangles, which is the half of this most likely to break silently:
/// they come from a prepaint callback whose order has to match the order the
/// description is built in, and they need the window's scale factor applied.
/// A mismatch gives a node the wrong rectangle, which no unit test can see.
#[test]
fn the_page_controls_report_the_rectangles_they_were_painted_at() {
    let tree = probe("two-page.pdf");

    let row: Vec<[f64; 4]> = ["first-page", "previous-page", "next-page", "last-page"]
        .into_iter()
        .map(|id| tree.frame(tree.by_id(id)))
        .collect();

    for (id, frame) in ["first-page", "previous-page", "next-page", "last-page"]
        .into_iter()
        .zip(&row)
    {
        assert!(
            frame[2] > 0.0 && frame[3] > 0.0,
            "{id} reports an empty rectangle: {frame:?}"
        );
    }
    // Left to right, in the order the row draws them.
    for pair in row.windows(2) {
        assert!(
            pair[1][0] > pair[0][0],
            "the page controls report rectangles out of order: {row:?}"
        );
    }
    // All on one row.
    assert!(row
        .windows(2)
        .all(|pair| (pair[1][1] - pair[0][1]).abs() < 1.0));
}

/// The document's own rectangle comes from the viewport rather than from a
/// prepaint callback, so it is the other half of the bounds path.
#[test]
fn a_page_reports_the_rectangle_it_is_drawn_at() {
    let tree = probe("hello.pdf");

    let page = tree.frame(tree.by_id("page-0"));

    assert!(
        page[2] > 0.0 && page[3] > 0.0,
        "the page reports an empty rectangle: {page:?}"
    );
    let text = tree
        .nodes
        .iter()
        .find(|node| tree.field(node, "identifier").contains("-text-"))
        .expect("the page published no text node");
    let text = tree.frame(text);
    assert!(text[2] > 0.0 && text[3] > 0.0);
    // The words sit inside the page they are on, within a pixel of rounding.
    assert!(
        text[0] >= page[0] - 1.0,
        "text {text:?} is left of page {page:?}"
    );
    assert!(
        text[0] + text[2] <= page[0] + page[2] + 1.0,
        "text {text:?} is right of page {page:?}"
    );
}

/// Every one of these renders as a glyph. Before P12 a screen reader read
/// them as punctuation or skipped them.
#[test]
fn the_glyph_only_page_controls_are_announced_by_name() {
    let tree = probe("two-page.pdf");

    for (id, name) in [
        ("first-page", "First Page"),
        ("previous-page", "Previous Page"),
        ("next-page", "Next Page"),
        ("last-page", "Last Page"),
        ("rotate-clockwise", "Rotate Clockwise"),
        ("zoom-in", "Zoom In"),
        ("zoom-out", "Zoom Out"),
    ] {
        let node = tree.by_id(id);
        assert_eq!(tree.field(node, "title"), name, "{id}");
        assert_eq!(tree.field(node, "role"), "AXButton", "{id}");
    }
}

/// Labels reach macOS as `accessibilityTitle` and never as
/// `accessibilityLabel`. Anything reading them back has to use the former,
/// which is a spike finding worth pinning rather than rediscovering.
#[test]
fn a_label_arrives_as_the_title_and_not_as_the_accessibility_label() {
    let tree = probe("hello.pdf");
    let node = tree.by_id("first-page");

    assert_eq!(tree.field(node, "title"), "First Page");
    assert!(tree.field(node, "label").is_empty());
}

/// The checked toolbar toggle. AccessKit maps a button carrying a toggled
/// state onto `AXCheckBox` with the toggle subrole, which is how macOS says
/// "this is on" rather than the tick that used to be inside the label.
#[test]
fn a_checked_control_reaches_the_platform_as_state_and_not_as_a_tick() {
    let tree = probe("hello.pdf");
    let node = tree.by_id("actual-size");

    assert_eq!(tree.field(node, "title"), "Actual Size");
    assert!(!tree.field(node, "title").contains('✓'));
    assert_eq!(tree.field(node, "role"), "AXCheckBox");
    assert_eq!(tree.field(node, "subrole"), "AXToggle");
}

/// The `Role::Document` fix.
///
/// AccessKit maps `Role::Document` onto `NSAccessibilityGroupRole` and onto
/// the `AXDocument` subrole (accesskit_macos 0.26.3, `node.rs`). AppKit has
/// no AXDocument role, so group plus that subrole is the correct encoding,
/// and the spike's "it flattens to AXGroup" was a reading of the role alone.
/// What was genuinely missing is the role description: without one AppKit
/// answers "group", which is what a user would have heard.
#[test]
fn a_page_is_announced_as_a_document_and_not_as_a_group() {
    let tree = probe("hello.pdf");
    let document = tree.by_id("document");

    assert_eq!(tree.field(document, "subrole"), "AXDocument");
    assert_eq!(tree.field(document, "roleDescription"), "document");
    assert_eq!(tree.field(document, "title"), "hello.pdf");
}

#[test]
fn each_page_is_its_own_node_and_says_which_page_it_is() {
    let tree = probe("two-page.pdf");
    let page = tree.by_id("page-0");

    assert_eq!(tree.field(page, "title"), "Page 1 of 2");
    assert_eq!(tree.field(page, "roleDescription"), "page");
}

/// The page's words, reachable and structured. A page handed over as one
/// string is a page a screen reader cannot navigate, so the text arrives as
/// one static-text node per run.
#[test]
fn the_page_text_is_reachable_as_static_text_under_its_page() {
    let tree = probe("hello.pdf");

    let text: Vec<&str> = tree
        .nodes
        .iter()
        .filter(|node| tree.field(node, "role") == "AXStaticText")
        .map(|node| tree.field(node, "title"))
        .collect();

    assert!(
        text.iter().any(|run| run.contains("Hello Onionskin")),
        "the page's own words were not in the tree; static text was {text:?}"
    );
}

/// A disabled control is on the tree, with the reason it is off, rather than
/// being left out of it.
#[test]
fn a_control_that_cannot_be_used_says_why_instead_of_disappearing() {
    let tree = probe("hello.pdf");
    let node = tree.by_id("previous-page");

    assert_eq!(tree.field(node, "title"), "Previous Page");
    assert_eq!(tree.field(node, "help"), "This is the first page");
    // The state, not just the words about it: a control announced as dimmed
    // but reported usable is one a screen reader will let a user press.
    assert!(
        !tree.enabled(node),
        "a disabled control reached the platform enabled"
    );
}

/// The press: how a VoiceOver user operates a control, and the one path with
/// no keyboard and no mouse in it. It leaves through AccessKit's action
/// handler rather than coming back through a getter, so nothing else in this
/// file can see it.
///
/// Actual Size rather than a page turn, because what it does is visible in the
/// tree whatever size the window opens at: the zoom reads 100 percent
/// afterwards and something else before.
#[test]
fn a_press_through_the_platform_runs_the_control_it_landed_on() {
    let before = probe("hello.pdf");
    let after = probe_pressing("hello.pdf", "actual-size");

    assert_eq!(
        after.pressed,
        Some(true),
        "the platform did not accept a press on Actual Size"
    );
    assert_ne!(
        before.field(before.by_id("zoom-level"), "title"),
        "Zoom 100 percent",
        "the view was already at actual size, so this press could not show anything"
    );
    assert_eq!(
        after.field(after.by_id("zoom-level"), "title"),
        "Zoom 100 percent",
        "a press through the platform did not run the control"
    );
}

/// The rail draws each tool's `icon()` string as visible text, which is an
/// id-like token. A screen reader has to hear the tool's name instead.
#[test]
fn a_rail_entry_is_announced_by_its_tool_name_and_not_by_its_icon_string() {
    let tree = probe("hello.pdf");

    let rail: Vec<&Node> = tree
        .nodes
        .iter()
        .filter(|node| {
            tree.field(node, "identifier")
                .starts_with("tool-rail-entry")
        })
        .collect();

    assert!(!rail.is_empty(), "the rail published no entries");
    // `tools-basic`'s hand tool: its `icon()` is a glyph and its name is
    // words, so this fails the moment the rail announces the icon.
    let titles: Vec<&str> = rail.iter().map(|node| tree.field(node, "title")).collect();
    assert!(
        titles.contains(&"Hand"),
        "the rail announced {titles:?}, none of which is a tool's name"
    );
}

/// The document node names the file.
///
/// Not the root: AccessKit deliberately drops the label of a root node whose
/// role is `Window` (accesskit_macos 0.26.3, `node.rs`, "if the group element
/// that we expose for the top-level window includes a title, VoiceOver
/// behavior is broken"), so the window's own name is AppKit's, not ours.
#[test]
fn the_document_node_names_the_open_file() {
    let tree = probe("hello.pdf");

    assert_eq!(tree.field(tree.by_id("document"), "title"), "hello.pdf");
}
