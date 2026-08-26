//! Properties every registry entry has to hold, checked against the whole
//! registry at once so a new plugin cannot quietly skip them. A plugin
//! inherits the contract by being registered.

use onionskin_app::build_registry;

#[test]
fn every_plugin_has_a_non_empty_id_and_name() {
    for plugin in build_registry().plugins() {
        assert!(!plugin.id.is_empty(), "{} has an empty id", plugin.name);
        assert!(!plugin.name.is_empty(), "{} has no name", plugin.id);
    }
}

#[test]
fn every_plugin_id_is_unique() {
    let registry = build_registry();
    let mut ids: Vec<&str> = registry.plugins().iter().map(|p| p.id).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(before, ids.len(), "duplicate plugin ids");
}

#[test]
fn every_tool_has_a_name_icon_and_group() {
    for tool in build_registry().tools() {
        assert!(!tool.id().is_empty(), "a tool has an empty id");
        assert!(!tool.name().is_empty(), "{} has no name", tool.id());
        assert!(!tool.icon().is_empty(), "{} has no icon", tool.id());
        assert!(!tool.group().is_empty(), "{} has no group", tool.id());
    }
}

#[test]
fn every_tool_id_is_unique() {
    let registry = build_registry();
    let mut ids: Vec<&str> = registry.tools().map(|t| t.id()).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(before, ids.len(), "duplicate tool ids");
}

#[test]
fn every_command_has_a_title_and_a_namespaced_id() {
    for command in build_registry().commands() {
        assert!(!command.title.is_empty(), "{} has no title", command.id);
        assert!(
            command.id.contains('.'),
            "{} is not namespaced (expected e.g. pages.rotate)",
            command.id
        );
    }
}

#[test]
fn every_command_id_is_unique() {
    let registry = build_registry();
    let mut ids: Vec<&str> = registry.commands().iter().map(|c| c.id).collect();
    let before = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(before, ids.len(), "duplicate command ids");
}

/// Every edit is undoable: applying a tool gesture or command to a
/// document and then undoing it is the identity on the edit graph.
#[test]
#[ignore = "needs core's edit graph and history, M3"]
fn every_edit_is_undoable() {
    unimplemented!("needs Document, the edit graph and undo")
}

/// Every edit serializes: what a tool or command produces saves to an
/// incremental section that a fresh parse of the result accepts.
#[test]
#[ignore = "needs the cos incremental writer, M3"]
fn every_edit_serializes_into_an_acceptable_incremental_section() {
    unimplemented!("needs cos save and re-parse")
}

/// Every edit is deterministic: the same gesture against the same
/// document twice produces the same edit, or previews would disagree with
/// what gets committed.
#[test]
#[ignore = "needs Document and a corpus fixture to replay against, M3"]
fn every_edit_is_deterministic() {
    unimplemented!("needs Document and a fixture to replay")
}
