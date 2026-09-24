//! Layer Properties: a layer's name, its intent, and whether it is on when
//! the document opens, written back to the file.
//!
//! The name and intent live in the group's own dictionary. The default
//! state lives in the default configuration, `/OCProperties /D`: a group is
//! put in `/ON` or `/OFF` and taken out of the other, which says the same
//! whatever the configuration's `/BaseState` is.

use onionskin_cos::{Dict, Name, Object};

use crate::annots::author::text_string;
use crate::{Error, ObjRef, Result, Transaction};

/// What a layer is for, as ISO 32000-2 8.11.2.1 names the two intents.
/// A group whose intent is Design is one a viewer does not consider when
/// deciding what to show.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayerIntent {
    #[default]
    View,
    Design,
}

impl LayerIntent {
    pub fn label(self) -> &'static str {
        match self {
            Self::View => "View",
            Self::Design => "Design",
        }
    }

    /// `/Intent` as a group states it: a name or an array of names. View
    /// unless it names Design and not View.
    pub(crate) fn of(group: &Dict) -> Self {
        let names: Vec<&[u8]> = match group.get(b"Intent") {
            Some(Object::Name(name)) => vec![name.as_bytes()],
            Some(Object::Array(items)) => items
                .iter()
                .filter_map(Object::as_name)
                .map(Name::as_bytes)
                .collect(),
            _ => Vec::new(),
        };
        if names.contains(&b"Design".as_slice()) && !names.contains(&b"View".as_slice()) {
            Self::Design
        } else {
            Self::View
        }
    }
}

/// What Layer Properties sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LayerProperties {
    pub name: String,
    pub intent: LayerIntent,
    /// On when the document opens.
    pub default_on: bool,
}

/// Write `properties` to the group `layer`.
pub fn set_layer_properties(
    tx: &mut Transaction<'_>,
    layer: ObjRef,
    properties: &LayerProperties,
) -> Result<()> {
    let mut group = dict_at(tx, layer)?;
    group.set(Name::new("Name"), text_string(&properties.name));
    group.set(Name::new("Intent"), Object::name(properties.intent.label()));
    tx.put_object(layer.number, layer.generation, Object::Dict(group))?;
    set_default_state(tx, layer, properties.default_on)
}

/// Put `layer` in `/D /ON` or `/D /OFF`, and out of the other.
fn set_default_state(tx: &mut Transaction<'_>, layer: ObjRef, on: bool) -> Result<()> {
    let root = tx
        .trailer_value(b"Root")
        .and_then(|root| root.as_reference())
        .ok_or(Error::NoCatalog)?;
    let mut catalog = dict_at(tx, root)?;
    let (mut properties, properties_at) = nested(tx, catalog.get(b"OCProperties"))?;
    let (mut config, config_at) = nested(tx, properties.get(b"D"))?;
    let (add, remove) = if on { ("ON", "OFF") } else { ("OFF", "ON") };
    let without = |list: Option<&Object>| -> Vec<Object> {
        list.and_then(Object::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter(|item| item.as_reference() != Some(layer))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut adding = without(resolved(tx, config.get(add.as_bytes()))?.as_ref());
    adding.push(Object::Ref(layer));
    let removing = without(resolved(tx, config.get(remove.as_bytes()))?.as_ref());
    config.set(Name::new(add), Object::Array(adding));
    config.set(Name::new(remove), Object::Array(removing));

    store(tx, &mut properties, "D", config, config_at)?;
    store(tx, &mut catalog, "OCProperties", properties, properties_at)?;
    tx.put_object(root.number, root.generation, Object::Dict(catalog))
}

/// A dictionary-valued entry and where it lives: its own object, or inline
/// (`None`). An absent entry is an empty inline dictionary.
fn nested(tx: &Transaction<'_>, value: Option<&Object>) -> Result<(Dict, Option<ObjRef>)> {
    match value {
        Some(Object::Ref(at)) => Ok((dict_at(tx, *at)?, Some(*at))),
        Some(Object::Dict(dict)) => Ok((dict.clone(), None)),
        _ => Ok((Dict::new(), None)),
    }
}

/// Put `dict` back where it came from: its own object, or `parent`'s `key`.
fn store(
    tx: &mut Transaction<'_>,
    parent: &mut Dict,
    key: &str,
    dict: Dict,
    at: Option<ObjRef>,
) -> Result<()> {
    match at {
        Some(at) => tx.put_object(at.number, at.generation, Object::Dict(dict)),
        None => {
            parent.set(Name::new(key), Object::Dict(dict));
            Ok(())
        }
    }
}

/// An entry with one level of reference followed, as `/ON` may be written.
fn resolved(tx: &Transaction<'_>, value: Option<&Object>) -> Result<Option<Object>> {
    Ok(match value {
        Some(Object::Ref(at)) => tx.object(at.number)?.map(|state| state.object),
        other => other.cloned(),
    })
}

fn dict_at(tx: &Transaction<'_>, at: ObjRef) -> Result<Dict> {
    match tx.object(at.number)?.map(|state| state.object) {
        Some(Object::Dict(dict)) => Ok(dict),
        _ => Err(Error::NotADictionary { number: at.number }),
    }
}
