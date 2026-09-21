//! The model that would draw an annotation, read back from its dictionary.
//!
//! An edit that changes how a comment looks, its colour, its opacity or a
//! free text's words, has to draw its appearance again, and the appearance is
//! drawn from an [`Annotation`]. This reads one back from what the file
//! says, for every subtype this crate draws. A subtype it does not draw, a
//! stamp or an attachment's paperclip whose art is not in the dictionary,
//! answers `None`, and its appearance is left as it is.

use onionskin_cos::{Dict, Object};

use super::model::{
    Annotation, BaseFont, BorderEffect, Color, Intent, LineEnding, Rect, Subtype, TextStyle,
};
use super::read::{as_number, color, ink_list, numbers, quads, rect, text};

/// The model for `dict`, or `None` when this crate does not draw its
/// subtype from the dictionary alone.
pub(crate) fn model_from_dict(dict: &Dict) -> Option<Annotation> {
    let subtype = dict
        .get(b"Subtype")
        .and_then(Object::as_name)
        .and_then(Subtype::from_name)?;
    let rect = rect(dict.get(b"Rect")).unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0));
    let mut model = match subtype {
        Subtype::Stamp | Subtype::FileAttachment => return None,
        Subtype::FreeText => free_text(dict, rect),
        Subtype::Ink => {
            let mut model = Annotation::new(Subtype::Ink, rect);
            model.ink = ink_list(dict.get(b"InkList"));
            model
        }
        Subtype::Polygon | Subtype::PolyLine => {
            let mut model = Annotation::new(subtype, rect);
            model.vertices = pairs(dict.get(b"Vertices"));
            model
        }
        _ => Annotation::new(subtype, rect),
    };
    model.quads = quads(dict.get(b"QuadPoints"));
    model.line = match numbers(dict.get(b"L"))[..] {
        [x0, y0, x1, y1] => Some(((x0, y0), (x1, y1))),
        _ => None,
    };
    model.endings = endings(dict.get(b"LE"));
    model.border_effect = border_effect(dict.get(b"BE"));
    model.color = color(dict.get(b"C"));
    model.interior_color = color(dict.get(b"IC"));
    model.opacity = dict.get(b"CA").and_then(as_number);
    model.border_width = dict
        .get(b"BS")
        .and_then(Object::as_dict)
        .and_then(|border| border.get(b"W"))
        .and_then(as_number)
        .unwrap_or(1.0);
    model.icon = dict
        .get(b"Name")
        .and_then(Object::as_name)
        .map(|name| String::from_utf8_lossy(name.as_bytes()).into_owned());
    model.contents = text(dict.get(b"Contents"));
    Some(model)
}

fn pairs(object: Option<&Object>) -> Vec<(f64, f64)> {
    numbers(object)
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect()
}

fn free_text(dict: &Dict, rect: Rect) -> Annotation {
    let intent = match dict.get(b"IT").and_then(Object::as_name) {
        Some(name) if name.as_bytes() == b"FreeTextTypewriter" => Some(Intent::FreeTextTypewriter),
        Some(name) if name.as_bytes() == b"FreeTextCallout" => Some(Intent::FreeTextCallout),
        _ => None,
    };
    let style = dict
        .get(b"DA")
        .and_then(|da| match da {
            Object::String(bytes) => parse_default_appearance(&String::from_utf8_lossy(bytes)),
            _ => None,
        })
        .unwrap_or(TextStyle::new(BaseFont::Helvetica, 12.0, Color::BLACK));
    let mut model = Annotation::free_text(rect, style, intent);
    model.callout = pairs(dict.get(b"CL"));
    model
}

fn endings(object: Option<&Object>) -> Option<(LineEnding, LineEnding)> {
    let Some(Object::Array(items)) = object else {
        return None;
    };
    let ending = |item: &Object| match item.as_name().map(|name| name.as_bytes()) {
        Some(b"OpenArrow") => LineEnding::OpenArrow,
        Some(b"ClosedArrow") => LineEnding::ClosedArrow,
        _ => LineEnding::None,
    };
    match &items[..] {
        [first, last] => Some((ending(first), ending(last))),
        _ => None,
    }
}

fn border_effect(object: Option<&Object>) -> Option<BorderEffect> {
    let effect = object?.as_dict()?;
    let cloudy = effect
        .get(b"S")
        .and_then(Object::as_name)
        .is_some_and(|name| name.as_bytes() == b"C");
    cloudy.then(|| BorderEffect::Cloudy {
        intensity: effect.get(b"I").and_then(as_number).unwrap_or(0.0),
    })
}

/// A `/DA` string back into the style that wrote it: `/Font size Tf r g b rg`.
/// A font this crate does not name falls back to Helvetica; a missing colour
/// is black.
pub fn parse_default_appearance(da: &str) -> Option<TextStyle> {
    let tokens: Vec<&str> = da.split_whitespace().collect();
    let tf = tokens.iter().position(|token| *token == "Tf")?;
    let size: f64 = tokens.get(tf.checked_sub(1)?)?.parse().ok()?;
    let font_name = tokens.get(tf.checked_sub(2)?)?.trim_start_matches('/');
    let font = [
        BaseFont::Helvetica,
        BaseFont::HelveticaBold,
        BaseFont::TimesRoman,
        BaseFont::Courier,
    ]
    .into_iter()
    .find(|font| font.resource_name() == font_name)
    .unwrap_or(BaseFont::Helvetica);
    let colour = tokens
        .iter()
        .position(|token| *token == "rg")
        .and_then(|rg| {
            let part = |back: usize| tokens.get(rg.checked_sub(back)?)?.parse::<f64>().ok();
            Some(Color::new(part(3)?, part(2)?, part(1)?))
        })
        .unwrap_or(Color::BLACK);
    Some(TextStyle::new(font, size, colour))
}

#[cfg(test)]
mod tests {
    use onionskin_cos::Name;

    use super::*;

    #[test]
    fn a_default_appearance_reads_back_as_the_style_that_wrote_it() {
        let style = TextStyle::new(BaseFont::Courier, 14.0, Color::new(0.5, 0.25, 1.0));
        assert_eq!(
            parse_default_appearance(&style.default_appearance()),
            Some(style)
        );
        assert_eq!(
            parse_default_appearance("/Unknown 9 Tf"),
            Some(TextStyle::new(BaseFont::Helvetica, 9.0, Color::BLACK))
        );
        assert_eq!(parse_default_appearance("0 g"), None);
    }

    fn array(values: &[f64]) -> Object {
        Object::Array(values.iter().map(|value| Object::Real(*value)).collect())
    }

    #[test]
    fn an_arrow_reads_back_with_its_line_endings_colour_and_opacity() {
        let mut dict = Dict::new();
        dict.set(Name::new("Subtype"), Object::name("Line"));
        dict.set(Name::new("Rect"), array(&[0.0, 0.0, 10.0, 10.0]));
        dict.set(Name::new("L"), array(&[1.0, 2.0, 3.0, 4.0]));
        dict.set(
            Name::new("LE"),
            Object::Array(vec![Object::name("None"), Object::name("ClosedArrow")]),
        );
        dict.set(Name::new("C"), array(&[1.0, 0.0, 0.0]));
        dict.set(Name::new("CA"), Object::Real(0.5));
        let model = model_from_dict(&dict).expect("a line is drawn");
        assert_eq!(model.line, Some(((1.0, 2.0), (3.0, 4.0))));
        assert_eq!(
            model.endings,
            Some((LineEnding::None, LineEnding::ClosedArrow))
        );
        assert_eq!(model.color, Some(Color::new(1.0, 0.0, 0.0)));
        assert_eq!(model.opacity, Some(0.5));
    }

    #[test]
    fn a_cloud_keeps_its_vertices_and_its_intensity() {
        let mut dict = Dict::new();
        dict.set(Name::new("Subtype"), Object::name("Polygon"));
        dict.set(
            Name::new("Vertices"),
            array(&[0.0, 0.0, 5.0, 0.0, 5.0, 5.0]),
        );
        let mut effect = Dict::new();
        effect.set(Name::new("S"), Object::name("C"));
        effect.set(Name::new("I"), Object::Real(2.0));
        dict.set(Name::new("BE"), Object::Dict(effect));
        let model = model_from_dict(&dict).expect("a polygon is drawn");
        assert_eq!(model.vertices, [(0.0, 0.0), (5.0, 0.0), (5.0, 5.0)]);
        assert_eq!(
            model.border_effect,
            Some(BorderEffect::Cloudy { intensity: 2.0 })
        );
    }

    #[test]
    fn a_stamp_is_not_redrawn_from_its_dictionary() {
        let mut dict = Dict::new();
        dict.set(Name::new("Subtype"), Object::name("Stamp"));
        assert!(model_from_dict(&dict).is_none());
    }
}
