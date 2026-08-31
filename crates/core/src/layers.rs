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

/// One optional content group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    /// The object the group's dictionary lives in, which is how the renderer
    /// names it too.
    pub id: ObjRef,
    pub name: String,
    pub visible: bool,
    /// Listed in `/OCProperties /D /Locked`. The file is saying the user may
    /// not change this group's visibility, so the pane disables the control
    /// rather than offering a toggle that would be ignored.
    pub locked: bool,
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

    let mut layers = Vec::new();
    for id in groups.into_iter().take(MAX_LAYERS) {
        if layers.iter().any(|layer: &Layer| layer.id == id) {
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
            visible,
            locked: locked.contains(&id),
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
        .map(|layer| (identifier(layer.id), layer.visible))
        .collect()
}

/// A layer's id in the renderer's terms. Both sides name an optional content
/// group by the object its dictionary lives in, so this is a rename and not a
/// mapping that could be wrong.
fn identifier(id: ObjRef) -> ObjectIdentifier {
    ObjectIdentifier::new(
        i32::try_from(id.number).unwrap_or(-1),
        i32::from(id.generation),
    )
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
