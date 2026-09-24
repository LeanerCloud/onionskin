//! Page boxes: Acrobat's Crop Pages, which is Set Page Boxes under another
//! name. A box is the media box less four margins, and the margins are the
//! ones the user sees: "top" is the edge at the top of the page as it is
//! displayed, whatever `/Rotate` does to it.
//!
//! Acrobat measures every box's margins from the media box, and so does
//! this. The media box itself is not changed here: that is Change Page Size,
//! which moves content rather than framing it.

use onionskin_cos::{Name, Object};

use super::ops::{check_indices, leaves};
use super::rewrite::{dict_at, resolve};
use crate::edit::Transaction;
use crate::{Error, PageIndex, Result};

/// Which box to set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageBox {
    /// What a viewer shows and a printer prints.
    Crop,
    /// Where a printed page is cut, with its bleed.
    Bleed,
    /// The finished page after trimming.
    Trim,
    /// The meaningful content, for placing the page into another document.
    Art,
}

impl PageBox {
    pub const ALL: [PageBox; 4] = [PageBox::Crop, PageBox::Bleed, PageBox::Trim, PageBox::Art];

    fn key(self) -> &'static str {
        match self {
            PageBox::Crop => "CropBox",
            PageBox::Bleed => "BleedBox",
            PageBox::Trim => "TrimBox",
            PageBox::Art => "ArtBox",
        }
    }

    /// The name Acrobat's dialog uses, which is the key.
    pub fn label(self) -> &'static str {
        self.key()
    }
}

/// Distances in from the media box's edges, in points, as the page is
/// displayed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Margins {
    pub top: f64,
    pub bottom: f64,
    pub left: f64,
    pub right: f64,
}

/// The smallest box, in points, a page may be left with: Acrobat refuses a
/// crop that leaves nothing, and a zero-width box is one no reader draws.
pub const MIN_BOX_SIZE: f64 = 1.0;

/// A page with no `/MediaBox` anywhere above it is US Letter, as every
/// reader and `content` take it.
const US_LETTER: [f64; 4] = [0.0, 0.0, 612.0, 792.0];

/// Set `which` on each of `pages` to its media box less `margins`.
///
/// Refuses the whole edit, writing nothing, if a margin is negative or not
/// a number, or if any page would be left narrower or shorter than
/// [`MIN_BOX_SIZE`].
pub fn set_page_box(
    tx: &mut Transaction<'_>,
    pages: &[PageIndex],
    which: PageBox,
    margins: Margins,
) -> Result<()> {
    check_margins(margins)?;
    let leaves = leaves(tx)?;
    let set: std::collections::BTreeSet<usize> = pages.iter().copied().collect();
    check_indices(&set, leaves.len())?;
    // Every box first, so one page that cannot take the margins leaves the
    // others untouched.
    let boxes = set
        .iter()
        .map(|&index| {
            let leaf = &leaves[index];
            let media = media_box(resolve(tx, leaf.inherited.media_box.as_ref())?);
            let rotate = rotation(resolve(tx, leaf.inherited.rotate.as_ref())?);
            boxed(media, rotate, margins)
                .map_err(|(width, height)| Error::PageBoxTooSmall {
                    page: index,
                    width,
                    height,
                })
                .map(|rect| (leaf.objref, rect))
        })
        .collect::<Result<Vec<_>>>()?;
    for (objref, rect) in boxes {
        let mut dict = dict_at(tx, objref)?;
        dict.set(Name::new(which.key()), rect_object(rect));
        tx.put_object(objref.number, objref.generation, Object::Dict(dict))?;
    }
    Ok(())
}

/// Change Page Size: give each of `pages` a media box `width` by `height`
/// points as the page is shown, centred on the one it had, so what the page
/// draws stays in the middle. The crop box becomes the new media box, since
/// a crop kept from the old size would hide the change.
///
/// Refuses the whole edit, writing nothing, for a size smaller than
/// [`MIN_BOX_SIZE`] either way or not a number.
pub fn set_media_size(
    tx: &mut Transaction<'_>,
    pages: &[PageIndex],
    width: f64,
    height: f64,
) -> Result<()> {
    let leaves = leaves(tx)?;
    let set: std::collections::BTreeSet<usize> = pages.iter().copied().collect();
    check_indices(&set, leaves.len())?;
    if !(width >= MIN_BOX_SIZE && height >= MIN_BOX_SIZE) {
        return Err(Error::PageBoxTooSmall {
            page: set.first().copied().unwrap_or_default(),
            width,
            height,
        });
    }
    let mut sized = Vec::with_capacity(set.len());
    for &index in &set {
        let leaf = &leaves[index];
        let media = media_box(resolve(tx, leaf.inherited.media_box.as_ref())?);
        let rotate = rotation(resolve(tx, leaf.inherited.rotate.as_ref())?);
        sized.push((leaf.objref, resized(media, rotate, width, height)));
    }
    for (objref, rect) in sized {
        let mut dict = dict_at(tx, objref)?;
        for key in ["MediaBox", "CropBox"] {
            dict.set(Name::new(key), rect_object(rect));
        }
        tx.put_object(objref.number, objref.generation, Object::Dict(dict))?;
    }
    Ok(())
}

/// `media` resized to `width` by `height` as shown on a page turned
/// `rotate` degrees, about its centre.
pub fn resized(media: [f64; 4], rotate: i32, width: f64, height: f64) -> [f64; 4] {
    let (width, height) = if rotate % 180 == 0 {
        (width, height)
    } else {
        (height, width)
    };
    let (cx, cy) = ((media[0] + media[2]) / 2.0, (media[1] + media[3]) / 2.0);
    [
        cx - width / 2.0,
        cy - height / 2.0,
        cx + width / 2.0,
        cy + height / 2.0,
    ]
}

fn rect_object(rect: [f64; 4]) -> Object {
    Object::Array(rect.iter().map(|value| Object::Real(*value)).collect())
}

fn check_margins(margins: Margins) -> Result<()> {
    for value in [margins.top, margins.bottom, margins.left, margins.right] {
        if !value.is_finite() || value < 0.0 {
            return Err(Error::InvalidMargin(value));
        }
    }
    Ok(())
}

/// `media` less `margins` seen through a page turned by `rotate` degrees
/// clockwise, or the too-small size it would have.
pub fn boxed(
    media: [f64; 4],
    rotate: i32,
    margins: Margins,
) -> std::result::Result<[f64; 4], (f64, f64)> {
    let unturned = unturn(margins, rotate);
    let rect = [
        media[0] + unturned.left,
        media[1] + unturned.bottom,
        media[2] - unturned.right,
        media[3] - unturned.top,
    ];
    let (width, height) = (rect[2] - rect[0], rect[3] - rect[1]);
    if width < MIN_BOX_SIZE || height < MIN_BOX_SIZE {
        return Err((width, height));
    }
    Ok(rect)
}

/// The margins, as shown on a page turned `rotate` degrees clockwise, that
/// take `media` to `rect`: what [`boxed`] undoes. A dialog opens on these for
/// the box a page already has.
pub fn shown_margins(media: [f64; 4], rect: [f64; 4], rotate: i32) -> Margins {
    let own = Margins {
        left: rect[0] - media[0],
        bottom: rect[1] - media[1],
        right: media[2] - rect[2],
        top: media[3] - rect[3],
    };
    // A turn and its opposite: unturning by the rest of the circle turns.
    unturn(own, (360 - rotate.rem_euclid(360)) % 360)
}

/// The margins on the page's own edges, for margins given as displayed on
/// a page turned `rotate` degrees clockwise. Turned a quarter clockwise, the
/// page's left edge is at the top of the screen and its top at the right.
fn unturn(shown: Margins, rotate: i32) -> Margins {
    match rotate {
        90 => Margins {
            left: shown.top,
            top: shown.right,
            right: shown.bottom,
            bottom: shown.left,
        },
        180 => Margins {
            top: shown.bottom,
            bottom: shown.top,
            left: shown.right,
            right: shown.left,
        },
        270 => Margins {
            top: shown.left,
            left: shown.bottom,
            bottom: shown.right,
            right: shown.top,
        },
        _ => shown,
    }
}

/// The page's media box, lower-left corner first; US Letter if it has none
/// a reader could use.
fn media_box(object: Option<Object>) -> [f64; 4] {
    let Some(Object::Array(values)) = object else {
        return US_LETTER;
    };
    let numbers: Vec<f64> = values.iter().filter_map(number).collect();
    let [x0, y0, x1, y1] = numbers[..] else {
        return US_LETTER;
    };
    [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]
}

/// The page's rotation, one of the four right angles; anything else is a
/// producer bug every reader treats as none.
fn rotation(object: Option<Object>) -> i32 {
    let degrees = object.as_ref().and_then(number).unwrap_or(0.0).round() as i64;
    let turned = degrees.rem_euclid(360) as i32;
    if turned % 90 == 0 {
        turned
    } else {
        0
    }
}

fn number(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(*value),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: [f64; 4] = [0.0, 0.0, 612.0, 792.0];

    fn margins(top: f64, bottom: f64, left: f64, right: f64) -> Margins {
        Margins {
            top,
            bottom,
            left,
            right,
        }
    }

    #[test]
    fn an_unturned_page_loses_each_margin_on_its_own_edge() {
        assert_eq!(
            boxed(LETTER, 0, margins(10.0, 20.0, 30.0, 40.0)),
            Ok([30.0, 20.0, 572.0, 782.0])
        );
    }

    /// A quarter turn clockwise puts the page's left edge at the top of the
    /// screen: the margin the user gives for the top comes off its left.
    #[test]
    fn a_turned_page_loses_each_margin_on_the_edge_shown_there() {
        let shown = margins(10.0, 20.0, 30.0, 40.0);
        assert_eq!(boxed(LETTER, 90, shown), Ok([10.0, 30.0, 592.0, 752.0]));
        assert_eq!(boxed(LETTER, 180, shown), Ok([40.0, 10.0, 582.0, 772.0]));
        assert_eq!(boxed(LETTER, 270, shown), Ok([20.0, 40.0, 602.0, 762.0]));
    }

    #[test]
    fn a_crop_that_leaves_nothing_reports_what_it_would_leave() {
        assert_eq!(
            boxed(LETTER, 0, margins(400.0, 400.0, 0.0, 0.0)),
            Err((612.0, -8.0))
        );
        assert!(boxed(LETTER, 0, margins(0.0, 0.0, 305.6, 305.6)).is_err());
    }

    #[test]
    fn a_negative_or_missing_margin_is_refused() {
        assert!(matches!(
            check_margins(margins(-1.0, 0.0, 0.0, 0.0)),
            Err(Error::InvalidMargin(value)) if value == -1.0
        ));
        assert!(check_margins(margins(0.0, f64::NAN, 0.0, 0.0)).is_err());
        assert!(check_margins(Margins::default()).is_ok());
    }

    #[test]
    fn a_media_box_is_read_corner_first_or_taken_as_letter() {
        let array = |values: &[f64]| {
            Some(Object::Array(
                values.iter().map(|v| Object::Real(*v)).collect(),
            ))
        };
        assert_eq!(
            media_box(array(&[100.0, 200.0, 0.0, 50.0])),
            [0.0, 50.0, 100.0, 200.0]
        );
        assert_eq!(media_box(array(&[1.0, 2.0])), US_LETTER);
        assert_eq!(media_box(Some(Object::Integer(3))), US_LETTER);
        assert_eq!(media_box(None), US_LETTER);
    }

    #[test]
    fn a_rotation_is_a_right_angle_or_none() {
        assert_eq!(rotation(Some(Object::Integer(-90))), 270);
        assert_eq!(rotation(Some(Object::Real(450.0))), 90);
        assert_eq!(rotation(Some(Object::Integer(45))), 0);
        assert_eq!(rotation(None), 0);
        assert_eq!(rotation(Some(Object::Null)), 0);
    }

    #[test]
    fn shown_margins_are_what_boxed_takes_back_to_the_same_box() {
        let shown = margins(10.0, 20.0, 30.0, 40.0);
        for rotate in [0, 90, 180, 270] {
            let rect = boxed(LETTER, rotate, shown).expect("fits");
            assert_eq!(shown_margins(LETTER, rect, rotate), shown, "{rotate}");
        }
        assert_eq!(shown_margins(LETTER, LETTER, 90), Margins::default());
    }

    #[test]
    fn a_page_is_resized_about_its_centre_as_it_is_shown() {
        assert_eq!(
            resized(LETTER, 0, 412.0, 592.0),
            [100.0, 100.0, 512.0, 692.0]
        );
        // Turned, the width shown runs along the page's own height.
        assert_eq!(
            resized(LETTER, 90, 592.0, 412.0),
            [100.0, 100.0, 512.0, 692.0]
        );
        assert_eq!(resized(LETTER, 180, 612.0, 792.0), LETTER);
    }

    #[test]
    fn each_box_has_its_key() {
        let keys: Vec<_> = PageBox::ALL.iter().map(|b| b.key()).collect();
        assert_eq!(keys, ["CropBox", "BleedBox", "TrimBox", "ArtBox"]);
        let labels: Vec<_> = PageBox::ALL.iter().map(|b| b.label()).collect();
        assert_eq!(labels, keys, "Acrobat's dialog names each box by its key");
    }
}
