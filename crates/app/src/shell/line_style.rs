//! The line editor's font, size and colour: a short list of each, the
//! first keeping what the line has.
//!
//! The fonts are the standard ones, which every reader draws without the
//! file carrying them. New text from Add Text has no font, size or colour
//! of its own: there the first entries mean Helvetica 12 pt in black.

use onionskin_core::text_edit::TextStyle;

/// Which of the three lists a choice is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum StyleList {
    Font,
    Size,
    Colour,
}

impl StyleList {
    pub(in crate::shell) const ALL: [Self; 3] = [Self::Font, Self::Size, Self::Colour];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Font => "Font",
            Self::Size => "Size",
            Self::Colour => "Colour",
        }
    }

    /// The names of the list's entries, in order.
    pub(in crate::shell) fn names(self) -> Vec<&'static str> {
        match self {
            Self::Font => FONTS.iter().map(|(name, _)| *name).collect(),
            Self::Size => SIZES.iter().map(|(name, _)| *name).collect(),
            Self::Colour => COLOURS.iter().map(|(name, _)| *name).collect(),
        }
    }

    fn index(self) -> usize {
        match self {
            Self::Font => 0,
            Self::Size => 1,
            Self::Colour => 2,
        }
    }
}

/// Entry `index` of list `list` picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) struct StyleChoice {
    pub(in crate::shell) list: StyleList,
    pub(in crate::shell) index: usize,
}

const FONTS: [(&str, Option<&str>); 6] = [
    ("Same font", None),
    ("Helvetica", Some("Helvetica")),
    ("Helvetica Bold", Some("Helvetica-Bold")),
    ("Times", Some("Times-Roman")),
    ("Times Bold", Some("Times-Bold")),
    ("Courier", Some("Courier")),
];

const SIZES: [(&str, Option<f64>); 7] = [
    ("Same size", None),
    ("8", Some(8.0)),
    ("10", Some(10.0)),
    ("12", Some(12.0)),
    ("14", Some(14.0)),
    ("18", Some(18.0)),
    ("24", Some(24.0)),
];

const COLOURS: [(&str, Option<[f64; 3]>); 6] = [
    ("Same colour", None),
    ("Black", Some([0.0, 0.0, 0.0])),
    ("Red", Some([0.8, 0.0, 0.0])),
    ("Blue", Some([0.0, 0.0, 0.8])),
    ("Green", Some([0.0, 0.5, 0.0])),
    ("Grey", Some([0.5, 0.5, 0.5])),
];

/// What the editor has picked in each list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::shell) struct Picked([usize; 3]);

impl Picked {
    pub(in crate::shell) fn pick(&mut self, choice: StyleChoice) {
        if choice.index < choice.list.names().len() {
            self.0[choice.list.index()] = choice.index;
        }
    }

    pub(in crate::shell) fn is_picked(self, choice: StyleChoice) -> bool {
        self.0[choice.list.index()] == choice.index
    }

    /// The style the picks make.
    pub(in crate::shell) fn style(self) -> TextStyle {
        TextStyle {
            face: FONTS[self.0[0]].1,
            size: SIZES[self.0[1]].1,
            fill: COLOURS[self.0[2]].1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_of_each_list_keeps_what_the_line_has() {
        assert_eq!(Picked::default().style(), TextStyle::default());
        let mut picked = Picked::default();
        for (list, index) in [
            (StyleList::Font, 4),
            (StyleList::Size, 6),
            (StyleList::Colour, 2),
        ] {
            let choice = StyleChoice { list, index };
            picked.pick(choice);
            assert!(picked.is_picked(choice));
        }
        assert_eq!(
            picked.style(),
            TextStyle {
                face: Some("Times-Bold"),
                size: Some(24.0),
                fill: Some([0.8, 0.0, 0.0]),
            }
        );
        picked.pick(StyleChoice {
            list: StyleList::Size,
            index: 99,
        });
        assert_eq!(
            picked.style().size,
            Some(24.0),
            "past the list, nothing changes"
        );
        let labels: Vec<_> = StyleList::ALL.map(StyleList::label).to_vec();
        assert_eq!(labels, ["Font", "Size", "Colour"]);
    }
}
