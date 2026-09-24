//! Optional content groups, as the layers pane lists and toggles them.
//!
//! A layer's visibility here is the document's own default configuration
//! (`/OCProperties /D`), which is what the renderer starts from. Toggling one
//! sends the whole set to the render worker as an override map, so what the
//! pane shows and what the rasterizer draws come from one value.

use std::collections::HashMap;

use onionskin_cos::{Document as CosDocument, Object};
use onionskin_render::ObjectIdentifier;

use crate::{Error, ObjRef, Result};

/// Enough for every real file; a bound so a crafted `/OCGs` array cannot make
/// the pane grow without end.
const MAX_LAYERS: usize = 10_000;
const MAX_ORDER_DEPTH: usize = 32;

/// One optional content group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    /// The object the group's dictionary lives in, which is how the renderer
    /// names it too.
    pub id: ObjRef,
    pub name: String,
    pub depth: usize,
    pub visible: bool,
    /// Listed in `/OCProperties /D /Locked`. The file is saying the user may
    /// not change this group's visibility, so the pane disables the control
    /// rather than offering a toggle that would be ignored.
    pub locked: bool,
    /// `/Intent`: what the layer is for.
    pub intent: crate::layer_properties::LayerIntent,
}

/// Read `/OCProperties`.
///
/// An absent `/OCProperties` is no layers. A present one that is not a
/// dictionary is an error, because the file claims optional content the
/// reader cannot produce.
pub(crate) fn read(doc: &CosDocument) -> Result<Vec<Layer>> {
    let catalog = doc.catalog()?;
    let Some(properties) = catalog.get(b"OCProperties") else {
        return Ok(Vec::new());
    };
    let properties = doc.resolve(properties)?;
    if matches!(properties, Object::Null) {
        return Ok(Vec::new());
    }
    let properties = properties.as_dict().cloned().ok_or_else(|| {
        Error::Cos(onionskin_cos::Error::Unrecoverable {
            detail: "/OCProperties does not resolve to a dictionary".into(),
        })
    })?;

    let groups = references(doc, properties.get(b"OCGs"))?;
    let config = match properties.get(b"D") {
        Some(config) => doc.resolve(config)?.as_dict().cloned(),
        None => None,
    };

    // `/BaseState` decides what a group not named by `/ON` or `/OFF` does.
    // Its default is `/ON`; `/OFF` inverts the question, which is why the two
    // lists exist at all.
    let base_visible = config
        .as_ref()
        .and_then(|config| config.get(b"BaseState"))
        .and_then(Object::as_name)
        .is_none_or(|state| state.as_bytes() != b"OFF");
    let on = match config.as_ref() {
        Some(config) => references(doc, config.get(b"ON"))?,
        None => Vec::new(),
    };
    let off = match config.as_ref() {
        Some(config) => references(doc, config.get(b"OFF"))?,
        None => Vec::new(),
    };
    let locked = match config.as_ref() {
        Some(config) => references(doc, config.get(b"Locked"))?,
        None => Vec::new(),
    };

    let ordered = match config.as_ref().and_then(|config| config.get(b"Order")) {
        Some(order) => Some(order_references(doc, order)?),
        None => None,
    };
    let listed_groups = groups.clone();
    let entries: Vec<(ObjRef, usize)> = match ordered {
        Some(ordered) => ordered,
        None => groups.into_iter().map(|id| (id, 0)).collect(),
    };

    let mut layers = Vec::new();
    for (id, depth) in entries.into_iter().take(MAX_LAYERS) {
        if !listed_groups.contains(&id) {
            continue;
        }
        if layers.iter().any(|layer: &Layer| layer.id == id) {
            continue;
        }
        // A group the renderer cannot be told about is one the pane must not
        // offer a control for: the toggle would be dropped on its way out.
        if identifier(id).is_none() {
            continue;
        }
        let Some(dict) = doc.get(id.number)?.object.as_dict().cloned() else {
            continue;
        };
        let name = match dict.get(b"Name") {
            Some(Object::String(bytes)) => onionskin_content::pdf_text_string(bytes),
            _ => String::new(),
        };
        // `/OFF` wins over `/ON` for a group named by both: a file that
        // cannot make up its mind gets the safer reading, which is the one
        // that does not show content the document may have meant to hide.
        let visible = if off.contains(&id) {
            false
        } else if on.contains(&id) {
            true
        } else {
            base_visible
        };
        layers.push(Layer {
            id,
            name,
            depth,
            visible,
            locked: locked.contains(&id),
            intent: crate::layer_properties::LayerIntent::of(&dict),
        });
    }
    Ok(layers)
}

/// The override map the render worker takes: every layer the pane knows
/// about, at the visibility it is showing.
///
/// The whole set rather than only the toggled ones. hayro merges overrides
/// onto the file's own default configuration, so sending the whole set makes
/// the map say exactly what the pane says, whatever the file's default was.
pub(crate) fn overrides(layers: &[Layer]) -> HashMap<ObjectIdentifier, bool> {
    layers
        .iter()
        .filter_map(|layer| Some((identifier(layer.id)?, layer.visible)))
        .collect()
}

/// A layer's id in the renderer's terms. Both sides name an optional content
/// group by the object its dictionary lives in, so this is a rename and not a
/// mapping that could be wrong.
///
/// `None` for an object number the renderer cannot express. Such an object is
/// beyond what a cross-reference table can address in the first place; what
/// matters is that it is left out rather than folded onto one sentinel, which
/// two of them would have shared, and toggling either would then have moved
/// the other.
fn identifier(id: ObjRef) -> Option<ObjectIdentifier> {
    Some(ObjectIdentifier::new(
        i32::try_from(id.number).ok()?,
        i32::from(id.generation),
    ))
}

/// The object references in an array-valued entry, ignoring anything else it
/// holds. `/OCGs` may carry a null for a group that was removed.
fn references(doc: &CosDocument, value: Option<&Object>) -> Result<Vec<ObjRef>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let mut found = Vec::new();
    // Resolved for the array itself only: the entries have to stay references,
    // because the reference is the group's identity.
    let Some(entries) = doc.resolve(value)?.as_array().map(<[Object]>::to_vec) else {
        return Ok(found);
    };
    for entry in entries {
        if let Some(reference) = entry.as_reference() {
            found.push(reference);
        }
    }
    Ok(found)
}

/// The optional-content groups named by `/D /Order`, with nesting depth.
///
/// If `/Order` is present, it is the pane's list. Groups named only by `/OCGs`
/// are still renderable, but the document did not ask the UI to show them.
fn order_references(doc: &CosDocument, value: &Object) -> Result<Vec<(ObjRef, usize)>> {
    let mut found = Vec::new();
    let Some(entries) = doc.resolve(value)?.as_array().map(<[Object]>::to_vec) else {
        return Ok(found);
    };
    for entry in entries {
        collect_order_entry(entry, 0, &mut found);
        if found.len() >= MAX_LAYERS {
            break;
        }
    }
    Ok(found)
}

fn collect_order_entry(entry: Object, depth: usize, found: &mut Vec<(ObjRef, usize)>) {
    if depth >= MAX_ORDER_DEPTH || found.len() >= MAX_LAYERS {
        return;
    }
    if let Some(reference) = entry.as_reference() {
        found.push((reference, depth));
        return;
    }
    let Some(entries) = entry.as_array().map(<[Object]>::to_vec) else {
        return;
    };
    collect_order_array(entries, depth, found)
}

fn collect_order_array(entries: Vec<Object>, depth: usize, found: &mut Vec<(ObjRef, usize)>) {
    let mut entries = entries.into_iter();
    let Some(first) = entries.next() else {
        return;
    };
    if let Some(reference) = first.as_reference() {
        found.push((reference, depth));
    } else if !matches!(first, Object::String(_) | Object::Name(_)) {
        collect_order_entry(first, depth + 1, found);
    }
    for entry in entries {
        collect_order_entry(entry, depth + 1, found);
        if found.len() >= MAX_LAYERS {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testpdf::{dict, pages, pdf};

    fn open(bytes: Vec<u8>) -> CosDocument {
        onionskin_cos::Document::open_repairing(Box::new(onionskin_cos::BytesSource::new(bytes)))
            .expect("the fixture opens")
            .0
    }

    /// Objects 1 and 2 are the catalog and the page tree root, 3 is the one
    /// page, and the rest are written by the test.
    fn document(catalog: &str, tail: &[&str]) -> Vec<u8> {
        let (tree, page_bodies) = pages(3, 1);
        let mut objects = vec![dict(catalog), tree];
        objects.extend(page_bodies);
        objects.extend(tail.iter().map(|body| dict(body)));
        pdf(&objects)
    }

    /// Objects 4, 5 and 6 are the three groups; 7 is the default config.
    fn three_groups(config: &str) -> Vec<u8> {
        document(
            "<< /Type /Catalog /Pages 2 0 R /OCProperties \
             << /OCGs [4 0 R 5 0 R 6 0 R] /D 7 0 R >> >>",
            &[
                "<< /Type /OCG /Name (Background) >>",
                "<< /Type /OCG /Name (Annotations) >>",
                "<< /Type /OCG /Name (Watermark) >>",
                config,
            ],
        )
    }

    /// Objects 4 through 7 are the four groups; 8 is the default config.
    fn four_groups(config: &str) -> Vec<u8> {
        document(
            "<< /Type /Catalog /Pages 2 0 R /OCProperties \
             << /OCGs [4 0 R 5 0 R 6 0 R 7 0 R] /D 8 0 R >> >>",
            &[
                "<< /Type /OCG /Name (Parent) >>",
                "<< /Type /OCG /Name (Child) >>",
                "<< /Type /OCG /Name (Sibling) >>",
                "<< /Type /OCG /Name (Omitted) >>",
                config,
            ],
        )
    }

    #[test]
    fn the_default_configuration_decides_which_groups_start_visible() {
        let doc = open(three_groups("<< /OFF [6 0 R] >>"));

        let layers = read(&doc).expect("the layers read");

        assert_eq!(
            layers
                .iter()
                .map(|layer| (layer.name.as_str(), layer.visible, layer.locked))
                .collect::<Vec<_>>(),
            [
                ("Background", true, false),
                ("Annotations", true, false),
                ("Watermark", false, false),
            ]
        );
    }

    /// `/BaseState /OFF` inverts the default, so `/ON` is the list that
    /// matters. A reader that only looked at `/OFF` would show every layer.
    #[test]
    fn base_state_off_hides_every_group_the_on_list_does_not_name() {
        let doc = open(three_groups("<< /BaseState /OFF /ON [5 0 R] >>"));

        let layers = read(&doc).expect("the layers read");

        assert_eq!(
            layers.iter().map(|layer| layer.visible).collect::<Vec<_>>(),
            [false, true, false]
        );
    }

    #[test]
    fn a_locked_group_is_reported_as_locked_and_keeps_its_visibility() {
        let doc = open(three_groups("<< /OFF [4 0 R] /Locked [4 0 R 5 0 R] >>"));

        let layers = read(&doc).expect("the layers read");

        assert!(layers[0].locked && !layers[0].visible);
        assert!(layers[1].locked && layers[1].visible);
        assert!(!layers[2].locked);
    }

    #[test]
    fn a_group_named_by_both_lists_stays_hidden() {
        let doc = open(three_groups("<< /ON [6 0 R] /OFF [6 0 R] >>"));

        assert!(!read(&doc).expect("the layers read")[2].visible);
    }

    #[test]
    fn default_order_lists_nested_groups_and_hides_omitted_groups() {
        let doc = open(four_groups("<< /Order [6 0 R [4 0 R 5 0 R]] >>"));

        let layers = read(&doc).expect("the layers read");

        assert_eq!(
            layers
                .iter()
                .map(|layer| (layer.name.as_str(), layer.depth))
                .collect::<Vec<_>>(),
            [("Sibling", 0), ("Parent", 0), ("Child", 1)]
        );
    }

    #[test]
    fn default_order_entries_outside_the_ocg_list_are_ignored() {
        let doc = open(four_groups("<< /Order [8 0 R 6 0 R [4 0 R 5 0 R]] >>"));

        let layers = read(&doc).expect("the layers read");

        assert_eq!(
            layers
                .iter()
                .map(|layer| layer.name.as_str())
                .collect::<Vec<_>>(),
            ["Sibling", "Parent", "Child"]
        );
    }

    #[test]
    fn default_order_keeps_visibility_and_locked_state() {
        let doc = open(four_groups(
            "<< /BaseState /OFF /ON [5 0 R] /Locked [5 0 R] \
             /Order [6 0 R [4 0 R 5 0 R]] >>",
        ));

        let layers = read(&doc).expect("the layers read");

        assert_eq!(
            layers
                .iter()
                .map(|layer| (layer.name.as_str(), layer.visible, layer.locked))
                .collect::<Vec<_>>(),
            [
                ("Sibling", false, false),
                ("Parent", false, false),
                ("Child", true, true),
            ]
        );
    }

    #[test]
    fn order_tree_depth_is_bounded() {
        let mut order = "4 0 R".to_owned();
        for _ in 0..=MAX_ORDER_DEPTH {
            order = format!("[{order}]");
        }
        let config = format!("<< /Order [{order}] >>");
        let doc = open(four_groups(&config));

        assert!(read(&doc).expect("the layers read").is_empty());
    }

    /// Without `/D` there is no default configuration to read, so every group
    /// takes the base state's own default rather than disappearing.
    #[test]
    fn groups_without_a_default_configuration_are_all_visible() {
        let doc = open(document(
            "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R] >> >>",
            &["<< /Type /OCG /Name (Only) >>"],
        ));

        let layers = read(&doc).expect("the layers read");

        assert_eq!(layers.len(), 1);
        assert!(layers[0].visible);
    }

    #[test]
    fn a_document_without_optional_content_lists_nothing() {
        let doc = open(document("<< /Type /Catalog /Pages 2 0 R >>", &[]));

        assert!(read(&doc).expect("the layers read").is_empty());
    }

    /// An object number the renderer has no room for is left out of the map
    /// rather than shortened to fit. Two of them shortened the same way
    /// would be one key, and toggling either would have moved the other.
    #[test]
    fn a_layer_the_renderer_cannot_name_is_left_out_of_the_map() {
        let unnameable = |number| Layer {
            id: ObjRef::new(number, 0),
            name: "beyond the table".to_owned(),
            depth: 0,
            visible: true,
            locked: false,
            intent: Default::default(),
        };

        assert_eq!(identifier(ObjRef::new(u32::MAX, 0)), None);
        assert_eq!(
            identifier(ObjRef::new(7, 0)),
            Some(ObjectIdentifier::new(7, 0))
        );

        let map = overrides(&[unnameable(u32::MAX), unnameable(u32::MAX - 1)]);

        assert!(map.is_empty(), "neither can be named, so neither is sent");
    }

    /// The override map is what the renderer is told. It has to name every
    /// layer, including the ones nobody toggled, because the file's own
    /// default is what it is being overridden against.
    #[test]
    fn the_override_map_names_every_layer_at_the_visibility_the_pane_shows() {
        let doc = open(three_groups("<< /OFF [6 0 R] >>"));
        let mut layers = read(&doc).expect("the layers read");
        layers[0].visible = false;

        let map = overrides(&layers);

        assert_eq!(map.len(), 3);
        assert_eq!(map.get(&ObjectIdentifier::new(4, 0)), Some(&false));
        assert_eq!(map.get(&ObjectIdentifier::new(5, 0)), Some(&true));
        assert_eq!(map.get(&ObjectIdentifier::new(6, 0)), Some(&false));
    }
}
