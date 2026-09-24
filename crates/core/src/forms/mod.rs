//! Interactive forms: the AcroForm field tree read into fields and their
//! widgets, and a field's value written with the appearance a reader draws.
//!
//! Filling is an ordinary edit, saved incrementally and undone like any
//! other. A value is written on the terminal field, which is where the
//! field tree keeps it, and every widget of the field gets the appearance
//! for it: its `/AS` for a check box or radio button, a new normal
//! appearance for text and choices. Scripts (format, validate, calculate)
//! are the `scripting` crate's; this module only reads what they are.

mod appearance;
mod read;
mod write;

use onionskin_cos::{Dict, ObjRef};

use crate::PageIndex;

pub use read::read_form;
pub use write::{reset_fields, set_field_value};

/// What kind of field, with what its kind carries.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    Text {
        multiline: bool,
        password: bool,
        /// Characters spread evenly over `max_len` cells.
        comb: bool,
        max_len: Option<usize>,
    },
    CheckBox,
    Radio {
        /// Clicking the selected button leaves it selected.
        no_toggle_to_off: bool,
    },
    PushButton,
    Choice {
        /// A dropdown; otherwise a list box.
        combo: bool,
        /// A dropdown that takes typed text too.
        editable: bool,
        multi_select: bool,
        options: Vec<ChoiceOption>,
    },
    Signature,
}

impl FieldKind {
    pub fn label(&self) -> &'static str {
        match self {
            FieldKind::Text { .. } => "Text",
            FieldKind::CheckBox => "Check Box",
            FieldKind::Radio { .. } => "Radio Button",
            FieldKind::PushButton => "Button",
            FieldKind::Choice { combo: true, .. } => "Dropdown",
            FieldKind::Choice { combo: false, .. } => "List Box",
            FieldKind::Signature => "Signature",
        }
    }
}

/// One entry of a choice field: the value exported, and the text shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceOption {
    pub export: String,
    pub display: String,
}

/// A field's value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FieldValue {
    #[default]
    None,
    Text(String),
    /// The chosen entries of a choice field, by export value.
    Chosen(Vec<String>),
    /// A check box's or radio group's on state by name, or off.
    State(Option<String>),
}

impl FieldValue {
    /// The value as a script or a summary reads it.
    pub fn as_text(&self) -> String {
        match self {
            FieldValue::None | FieldValue::State(None) => String::new(),
            FieldValue::Text(text) => text.clone(),
            FieldValue::Chosen(chosen) => chosen.join(", "),
            FieldValue::State(Some(state)) => state.clone(),
        }
    }
}

/// The flags every field kind has: `/Ff` bits 1 to 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FieldFlags {
    pub read_only: bool,
    pub required: bool,
    pub no_export: bool,
}

/// The JavaScript a field runs, from its `/AA`: as a key is typed, to show
/// its value, to accept it, and to compute it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FieldScripts {
    pub keystroke: Option<String>,
    pub format: Option<String>,
    pub validate: Option<String>,
    pub calculate: Option<String>,
}

/// One place a field is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Widget {
    pub objref: ObjRef,
    pub page: Option<PageIndex>,
    pub rect: [f64; 4],
    /// A check box's or radio button's appearance name for on.
    pub on_state: Option<String>,
    /// `/AS` now.
    pub state: Option<String>,
    /// `/F` hidden or no-view.
    pub hidden: bool,
}

impl Widget {
    pub fn contains(&self, (x, y): (f64, f64)) -> bool {
        let [x0, y0, x1, y1] = self.rect;
        (x0..=x1).contains(&x) && (y0..=y1).contains(&y)
    }
}

/// A terminal field: one value, drawn by one or more widgets.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// The dictionary the value lives in.
    pub objref: ObjRef,
    /// The fully qualified name, parents first, joined by `.`.
    pub name: String,
    pub kind: FieldKind,
    pub flags: FieldFlags,
    pub value: FieldValue,
    /// `/DV`, what Clear Form restores.
    pub default: FieldValue,
    pub widgets: Vec<Widget>,
    /// `/TU`, the tooltip; a screen reader's name for the field.
    pub tooltip: Option<String>,
    pub scripts: FieldScripts,
    /// `/Q`: 0 left, 1 centred, 2 right.
    pub align: u8,
    /// `/DA`, inherited from the form when the field has none.
    pub appearance: Option<String>,
}

impl Field {
    /// The first widget's page, where a field is said to be.
    pub fn page(&self) -> Option<PageIndex> {
        self.widgets.iter().find_map(|widget| widget.page)
    }

    /// The list box option drawn at `point` on `widget`, by its index.
    /// `None` for a point past the last option, or a field that is not a
    /// list box.
    pub fn list_row(&self, widget: &Widget, (_, y): (f64, f64)) -> Option<usize> {
        let FieldKind::Choice {
            combo: false,
            options,
            ..
        } = &self.kind
        else {
            return None;
        };
        let da = appearance::parse_da(self.appearance.as_deref());
        let top = widget.rect[1].max(widget.rect[3]);
        appearance::list_row(&da, top, y).filter(|row| *row < options.len())
    }
}

/// The form: every terminal field, and what the form dictionary says.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Form {
    pub fields: Vec<Field>,
    /// `/CO`: the fields whose calculations run, in order.
    pub calculation_order: Vec<ObjRef>,
    pub need_appearances: bool,
    /// The form carries XFA, which is shown read-only as its AcroForm.
    pub xfa: bool,
    /// `/DR`, the default resources.
    pub resources: Option<Dict>,
}

impl Form {
    pub fn field(&self, name: &str) -> Option<&Field> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub fn field_by_ref(&self, objref: ObjRef) -> Option<&Field> {
        self.fields.iter().find(|field| field.objref == objref)
    }

    /// The field and widget under `point` on `page`, the last drawn first.
    pub fn field_at(&self, page: PageIndex, point: (f64, f64)) -> Option<(&Field, &Widget)> {
        self.fields
            .iter()
            .rev()
            .flat_map(|field| {
                field
                    .widgets
                    .iter()
                    .rev()
                    .map(move |widget| (field, widget))
            })
            .find(|(_, widget)| {
                widget.page == Some(page) && !widget.hidden && widget.contains(point)
            })
    }

    /// What to tell the user about the form when the document opens: an
    /// XFA form, which is not run (legal posture rule 5), filled as its
    /// standard fields when it has them and not at all when it has none.
    pub fn notice(&self) -> Option<&'static str> {
        match (self.xfa, self.fields.is_empty()) {
            (false, _) => None,
            (true, true) => Some(
                "This is an XFA form, which Onionskin does not show or fill: the pages are shown as they are drawn, read-only",
            ),
            (true, false) => Some(
                "This form also has an XFA version, which Onionskin does not run: its standard fields are the ones filled",
            ),
        }
    }

    /// Every visible widget in tab order: page by page, and on a page top to
    /// bottom then left to right, as Acrobat orders a page without `/Tabs`.
    pub fn tab_order(&self) -> Vec<(usize, usize)> {
        let mut order: Vec<(usize, usize)> = self
            .fields
            .iter()
            .enumerate()
            .flat_map(|(field_index, field)| {
                field
                    .widgets
                    .iter()
                    .enumerate()
                    .filter(|(_, widget)| !widget.hidden && widget.page.is_some())
                    .map(move |(widget_index, _)| (field_index, widget_index))
            })
            .collect();
        let key = |&(field, widget): &(usize, usize)| {
            let widget = &self.fields[field].widgets[widget];
            (
                widget.page.unwrap_or(usize::MAX),
                -widget.rect[3],
                widget.rect[0],
            )
        };
        order.sort_by(|a, b| {
            let (pa, ya, xa) = key(a);
            let (pb, yb, xb) = key(b);
            pa.cmp(&pb)
                .then(ya.partial_cmp(&yb).unwrap_or(std::cmp::Ordering::Equal))
                .then(xa.partial_cmp(&xb).unwrap_or(std::cmp::Ordering::Equal))
        });
        order
    }
}
