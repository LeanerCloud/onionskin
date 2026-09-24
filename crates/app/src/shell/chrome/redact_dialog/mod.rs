//! The redaction dialogs (M5): Redaction Properties with its code sets,
//! Find Text & Redact, Mark Pages for Redaction, and the confirmation
//! before Apply Redactions or Remove Hidden Information writes a new file.
//!
//! One dialog with four panels, so the shell wires one state, one action
//! and one kind of field. What each panel's form means is plain data,
//! tested without a window; the frame runs it through the `redact` plugin.

mod view;

use gpui::{AppContext as _, Context, Entity};
use onionskin_core::redactions::{Align, Overlay, RedactionLook};
use onionskin_core::{ObjRef, PageIndex, SearchOptions};
use onionskin_redact::codes::CodeSet;
use onionskin_redact::find::{Found, Pattern, Query};

use super::accessible::TextField;
use super::{SearchInput, ShellFrame, ThemeTokens};

pub(in crate::shell) use view::{accessible, render};

/// Which panel the dialog shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Panel {
    /// The look new marks take, or with a mark, that mark's.
    Properties {
        mark: Option<(ObjRef, PageIndex)>,
    },
    Find,
    Pages,
    /// Apply Redactions, or with `sanitize`, Remove Hidden Information.
    Apply {
        sanitize: bool,
    },
}

impl Panel {
    pub(in crate::shell) fn title(self) -> &'static str {
        match self {
            Self::Properties { .. } => "Redaction Properties",
            Self::Find => "Find Text & Redact",
            Self::Pages => "Mark Pages for Redaction",
            Self::Apply { sanitize: false } => "Apply Redactions",
            Self::Apply { sanitize: true } => "Remove Hidden Information",
        }
    }
}

/// The colours an area can be filled with; `None` leaves it empty.
pub(in crate::shell) const FILLS: [(&str, Option<[f64; 3]>); 5] = [
    ("Black", Some([0.0, 0.0, 0.0])),
    ("White", Some([1.0, 1.0, 1.0])),
    ("Red", Some([1.0, 0.0, 0.0])),
    ("Gray", Some([0.5, 0.5, 0.5])),
    ("No Fill", None),
];

/// The colours a mark's outline and its overlay text take.
pub(in crate::shell) const COLORS: [(&str, [f64; 3]); 5] = [
    ("Red", [1.0, 0.0, 0.0]),
    ("Black", [0.0, 0.0, 0.0]),
    ("White", [1.0, 1.0, 1.0]),
    ("Blue", [0.0, 0.0, 1.0]),
    ("Yellow", [1.0, 1.0, 0.0]),
];

/// A typed field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::shell) enum RedactField {
    OverlayText,
    FontSize,
    SetName,
    Codes,
    FindText,
    Pages,
}

impl RedactField {
    pub(in crate::shell) const ALL: [RedactField; 6] = [
        RedactField::OverlayText,
        RedactField::FontSize,
        RedactField::SetName,
        RedactField::Codes,
        RedactField::FindText,
        RedactField::Pages,
    ];

    pub(in crate::shell) fn id(self) -> &'static str {
        match self {
            Self::OverlayText => "redact-overlay-text",
            Self::FontSize => "redact-font-size",
            Self::SetName => "redact-set-name",
            Self::Codes => "redact-codes",
            Self::FindText => "redact-find-text",
            Self::Pages => "redact-pages",
        }
    }

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::OverlayText => "Overlay text",
            Self::FontSize => "Font size (empty fits)",
            Self::SetName => "Code set name",
            Self::Codes => "Codes, comma separated",
            Self::FindText => "Words or phrase",
            Self::Pages => "Pages",
        }
    }

    pub(in crate::shell) fn numeric(self) -> bool {
        self == Self::FontSize
    }
}

/// What a control that is not typed into does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum RedactAction {
    NextFill,
    NextOutline,
    Overlay,
    NextTextColor,
    NextAlign,
    Repeat,
    NextSet,
    NextCode,
    UseCode,
    SaveSet,
    RenameSet,
    RemoveSet,
    ImportSet,
    ExportSet,
    Save,
    RemoveMark,
    Patterns(bool),
    NextPattern,
    WholeWord,
    MatchCase,
    Find,
    Toggle(usize),
    MarkChecked,
    MarkPages,
    HiddenInformation,
    Apply,
}

fn next<T: Copy + PartialEq>(all: &[(&str, T)], current: T) -> T {
    let at = all.iter().position(|(_, each)| *each == current);
    all[at.map_or(0, |at| (at + 1) % all.len())].1
}

pub(in crate::shell) fn name_of<T: PartialEq>(
    all: &[(&'static str, T)],
    value: &T,
) -> &'static str {
    all.iter()
        .find(|(_, each)| each == value)
        .map_or("Custom", |(name, _)| name)
}

/// Redaction Properties' choices, apart from what is typed.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct PropertiesForm {
    pub(in crate::shell) fill: Option<[f64; 3]>,
    pub(in crate::shell) outline: [f64; 3],
    pub(in crate::shell) overlay: bool,
    pub(in crate::shell) text_color: [f64; 3],
    pub(in crate::shell) align: Align,
    pub(in crate::shell) repeat: bool,
    pub(in crate::shell) sets: Vec<CodeSet>,
    pub(in crate::shell) set: usize,
    pub(in crate::shell) code: usize,
}

impl PropertiesForm {
    /// The form for `look`, with the overlay text and size to type.
    pub(in crate::shell) fn of(look: &RedactionLook, sets: Vec<CodeSet>) -> (Self, String, String) {
        let overlay = look.overlay.clone().unwrap_or_default();
        let size = if overlay.size > 0.0 {
            format!("{}", overlay.size)
        } else {
            String::new()
        };
        let form = Self {
            fill: look.fill,
            outline: look.outline,
            overlay: look.overlay.is_some(),
            text_color: overlay.color,
            align: overlay.align,
            repeat: overlay.repeat,
            sets,
            set: 0,
            code: 0,
        };
        (form, overlay.text, size)
    }

    pub(in crate::shell) fn apply(&mut self, action: RedactAction) {
        match action {
            RedactAction::NextFill => self.fill = next(&FILLS, self.fill),
            RedactAction::NextOutline => self.outline = next(&COLORS, self.outline),
            RedactAction::Overlay => self.overlay = !self.overlay,
            RedactAction::NextTextColor => self.text_color = next(&COLORS, self.text_color),
            RedactAction::NextAlign => {
                let at = Align::ALL
                    .iter()
                    .position(|align| *align == self.align)
                    .unwrap_or(0);
                self.align = Align::ALL[(at + 1) % Align::ALL.len()];
            }
            RedactAction::Repeat => self.repeat = !self.repeat,
            RedactAction::NextSet => {
                self.set = (self.set + 1) % self.sets.len().max(1);
                self.code = 0;
            }
            RedactAction::NextCode => {
                let count = self.current_set().map_or(0, |set| set.codes.len());
                self.code = (self.code + 1) % count.max(1);
            }
            _ => {}
        }
    }

    pub(in crate::shell) fn current_set(&self) -> Option<&CodeSet> {
        self.sets.get(self.set)
    }

    pub(in crate::shell) fn current_code(&self) -> Option<&str> {
        self.current_set()?.codes.get(self.code).map(String::as_str)
    }

    /// Choose the set named `name`, as after saving or importing it.
    pub(in crate::shell) fn choose_set(&mut self, sets: Vec<CodeSet>, name: &str) {
        self.set = sets.iter().position(|set| set.name == name).unwrap_or(0);
        self.sets = sets;
        self.code = 0;
    }

    /// The fields shown: the overlay's when there is one, and the code set
    /// editor's.
    pub(in crate::shell) fn fields(&self) -> Vec<RedactField> {
        let mut fields = Vec::new();
        if self.overlay {
            fields.extend([RedactField::OverlayText, RedactField::FontSize]);
        }
        fields.extend([RedactField::SetName, RedactField::Codes]);
        fields
    }
}

/// The look the form describes with `text` and `size` typed.
pub(in crate::shell) fn look(
    form: &PropertiesForm,
    text: &str,
    size: &str,
) -> Result<RedactionLook, String> {
    let overlay = if form.overlay {
        let text = text.trim();
        if text.is_empty() {
            return Err("Type the overlay text, or turn the overlay off.".to_owned());
        }
        let size = match size.trim() {
            "" => 0.0,
            typed => typed
                .parse::<f64>()
                .ok()
                .filter(|size| *size > 0.0 && size.is_finite())
                .ok_or_else(|| format!("{typed:?} is not a font size in points"))?,
        };
        Some(Overlay {
            text: text.to_owned(),
            size,
            color: form.text_color,
            align: form.align,
            repeat: form.repeat,
        })
    } else {
        None
    };
    Ok(RedactionLook {
        fill: form.fill,
        outline: form.outline,
        overlay,
    })
}

/// Codes typed as a comma separated list.
pub(in crate::shell) fn codes(typed: &str) -> Vec<String> {
    typed
        .split(',')
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Find Text & Redact's choices and what it found.
#[derive(Debug, Clone, Default, PartialEq)]
pub(in crate::shell) struct FindForm {
    pub(in crate::shell) patterns: bool,
    pub(in crate::shell) pattern: usize,
    pub(in crate::shell) whole_word: bool,
    pub(in crate::shell) match_case: bool,
    pub(in crate::shell) found: Vec<Found>,
    pub(in crate::shell) checked: Vec<bool>,
    pub(in crate::shell) searched: bool,
}

impl FindForm {
    pub(in crate::shell) fn apply(&mut self, action: RedactAction) {
        match action {
            RedactAction::Patterns(patterns) => self.patterns = patterns,
            RedactAction::NextPattern => self.pattern = (self.pattern + 1) % Pattern::ALL.len(),
            RedactAction::WholeWord => self.whole_word = !self.whole_word,
            RedactAction::MatchCase => self.match_case = !self.match_case,
            RedactAction::Toggle(at) => {
                if let Some(checked) = self.checked.get_mut(at) {
                    *checked = !*checked;
                }
            }
            _ => {}
        }
    }

    pub(in crate::shell) fn current_pattern(&self) -> Pattern {
        Pattern::ALL[self.pattern % Pattern::ALL.len()]
    }

    /// What was found, every hit checked.
    pub(in crate::shell) fn show(&mut self, found: Vec<Found>) {
        self.checked = vec![true; found.len()];
        self.found = found;
        self.searched = true;
    }

    pub(in crate::shell) fn chosen(&self) -> Vec<Found> {
        self.found
            .iter()
            .zip(&self.checked)
            .filter(|(_, checked)| **checked)
            .map(|(found, _)| found.clone())
            .collect()
    }

    pub(in crate::shell) fn fields(&self) -> Vec<RedactField> {
        if self.patterns {
            Vec::new()
        } else {
            vec![RedactField::FindText]
        }
    }
}

/// What Find Text & Redact looks for.
pub(in crate::shell) fn query(form: &FindForm, text: &str) -> Result<Query, String> {
    if form.patterns {
        return Ok(Query::Pattern(form.current_pattern()));
    }
    let text = text.trim();
    if text.is_empty() {
        return Err("Type the words or phrase to find.".to_owned());
    }
    Ok(Query::Text(
        text.to_owned(),
        SearchOptions {
            case_sensitive: form.match_case,
            whole_word: form.whole_word,
            ..SearchOptions::default()
        },
    ))
}

/// The dialog, open.
pub(in crate::shell) struct RedactDialogState {
    pub(in crate::shell) panel: Panel,
    pub(in crate::shell) properties: PropertiesForm,
    pub(in crate::shell) find: FindForm,
    /// Apply Redactions' "also remove hidden information".
    pub(in crate::shell) hidden_information: bool,
    pub(in crate::shell) page_count: usize,
    pub(in crate::shell) inputs: Vec<(RedactField, Entity<SearchInput>)>,
    pub(in crate::shell) error: Option<String>,
}

impl RedactDialogState {
    /// The dialog on `panel`, its typed fields holding `texts`.
    pub(in crate::shell) fn new(
        panel: Panel,
        properties: PropertiesForm,
        page_count: usize,
        texts: &[(RedactField, String)],
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let inputs = RedactField::ALL
            .into_iter()
            .map(|field| {
                let text = texts
                    .iter()
                    .find(|(each, _)| *each == field)
                    .map(|(_, text)| text.clone())
                    .unwrap_or_default();
                let input = cx.new(|cx| {
                    let mut input =
                        SearchInput::with_placeholder(field.id(), field.label(), theme, cx);
                    input.set_query(text, cx);
                    input
                });
                (field, input)
            })
            .collect();
        Self {
            panel,
            properties,
            find: FindForm::default(),
            hidden_information: false,
            page_count,
            inputs,
            error: None,
        }
    }

    /// The fields the panel shows.
    pub(in crate::shell) fn fields(&self) -> Vec<RedactField> {
        match self.panel {
            Panel::Properties { .. } => self.properties.fields(),
            Panel::Find => self.find.fields(),
            Panel::Pages => vec![RedactField::Pages],
            Panel::Apply { .. } => Vec::new(),
        }
    }

    pub(in crate::shell) fn text_field(&self, field: RedactField) -> Option<&Entity<SearchInput>> {
        if !self.fields().contains(&field) {
            return None;
        }
        self.input(field)
    }

    fn input(&self, field: RedactField) -> Option<&Entity<SearchInput>> {
        self.inputs
            .iter()
            .find(|(each, _)| *each == field)
            .map(|(_, input)| input)
    }

    /// What is typed in `field`.
    pub(in crate::shell) fn text(&self, field: RedactField, cx: &gpui::App) -> String {
        self.input(field)
            .map(|input| input.read(cx).query().to_owned())
            .unwrap_or_default()
    }
}

/// The dialog's text fields, for the focus ring.
pub(in crate::shell) fn text_fields() -> impl Iterator<Item = TextField> {
    RedactField::ALL.into_iter().map(TextField::Redact)
}

#[cfg(test)]
mod tests;
