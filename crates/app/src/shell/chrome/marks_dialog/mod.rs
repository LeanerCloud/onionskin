//! Watermark, Background, Header & Footer and Bates Numbering (M5): one
//! dialog, its rows chosen by the kind of mark, that adds a mark to the
//! chosen pages, updates the one they have, or removes it.
//!
//! What the form means is plain data ([`MarkForm`], [`request`]), tested
//! without a window; the frame runs the result through `tools-edit`, which
//! this module needs, so the whole dialog is built only with it.

mod settings;
mod view;

use std::collections::BTreeMap;
use std::path::PathBuf;

use gpui::{AppContext as _, Context, Entity};
use onionskin_core::pages::{Margins, MarkKind};
use onionskin_core::PageIndex;
use onionskin_tools_edit::marks::{
    Appearance, Bates, Font, HAlign, HeaderFooter, Naming, Numbering, TextStyle, VAlign, POSITIONS,
};

use super::accessible::TextField;
use super::crop_dialog::CropScope;
use super::{SearchInput, ShellFrame, ThemeTokens};

pub(in crate::shell) use view::{accessible, render};

/// What the Edit menu calls each kind's entry, and the dialog its title.
pub(in crate::shell) fn title(kind: MarkKind) -> &'static str {
    match kind {
        MarkKind::Watermark => "Watermark",
        MarkKind::Background => "Background",
        MarkKind::HeaderFooter => "Header & Footer",
        MarkKind::Bates => "Bates Numbering",
    }
}

/// A typed field of the dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::shell) enum MarkField {
    LeftHeader,
    CenterHeader,
    RightHeader,
    LeftFooter,
    CenterFooter,
    RightFooter,
    Text,
    Size,
    Top,
    Bottom,
    Left,
    Right,
    Start,
    Prefix,
    Suffix,
    Digits,
    Rotation,
    Opacity,
    Scale,
    /// Put after each numbered copy's original name.
    After,
}

impl MarkField {
    pub(in crate::shell) const ALL: [MarkField; 20] = [
        MarkField::LeftHeader,
        MarkField::CenterHeader,
        MarkField::RightHeader,
        MarkField::LeftFooter,
        MarkField::CenterFooter,
        MarkField::RightFooter,
        MarkField::Text,
        MarkField::Size,
        MarkField::Top,
        MarkField::Bottom,
        MarkField::Left,
        MarkField::Right,
        MarkField::Start,
        MarkField::Prefix,
        MarkField::Suffix,
        MarkField::Digits,
        MarkField::Rotation,
        MarkField::Opacity,
        MarkField::Scale,
        MarkField::After,
    ];

    /// The six header and footer lines, in [`POSITIONS`] order.
    const LINES: [MarkField; 6] = [
        MarkField::LeftHeader,
        MarkField::CenterHeader,
        MarkField::RightHeader,
        MarkField::LeftFooter,
        MarkField::CenterFooter,
        MarkField::RightFooter,
    ];

    const MARGINS: [MarkField; 4] = [
        MarkField::Top,
        MarkField::Bottom,
        MarkField::Left,
        MarkField::Right,
    ];

    /// The field's element id, which the tree and the focus ring share.
    pub(in crate::shell) fn id(self) -> &'static str {
        match self {
            Self::LeftHeader => "mark-left-header",
            Self::CenterHeader => "mark-center-header",
            Self::RightHeader => "mark-right-header",
            Self::LeftFooter => "mark-left-footer",
            Self::CenterFooter => "mark-center-footer",
            Self::RightFooter => "mark-right-footer",
            Self::Text => "mark-text",
            Self::Size => "mark-size",
            Self::Top => "mark-top",
            Self::Bottom => "mark-bottom",
            Self::Left => "mark-left",
            Self::Right => "mark-right",
            Self::Start => "mark-start",
            Self::Prefix => "mark-prefix",
            Self::Suffix => "mark-suffix",
            Self::Digits => "mark-digits",
            Self::Rotation => "mark-rotation",
            Self::Opacity => "mark-opacity",
            Self::Scale => "mark-scale",
            Self::After => "mark-after",
        }
    }

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::LeftHeader => POSITIONS[0],
            Self::CenterHeader => POSITIONS[1],
            Self::RightHeader => POSITIONS[2],
            Self::LeftFooter => POSITIONS[3],
            Self::CenterFooter => POSITIONS[4],
            Self::RightFooter => POSITIONS[5],
            Self::Text => "Text",
            Self::Size => "Size (pt)",
            Self::Top => "Top margin (pt)",
            Self::Bottom => "Bottom margin (pt)",
            Self::Left => "Left margin (pt)",
            Self::Right => "Right margin (pt)",
            Self::Start => "Start number",
            Self::Prefix => "Prefix",
            Self::Suffix => "Suffix",
            Self::Digits => "Number of digits",
            Self::Rotation => "Rotation (degrees)",
            Self::Opacity => "Opacity (%)",
            Self::Scale => "Scale (%)",
            Self::After => "Add to file names",
        }
    }

    /// Whether the field takes a number, for a screen reader.
    pub(in crate::shell) fn numeric(self) -> bool {
        !matches!(
            self,
            Self::LeftHeader
                | Self::CenterHeader
                | Self::RightHeader
                | Self::LeftFooter
                | Self::CenterFooter
                | Self::RightFooter
                | Self::Text
                | Self::Prefix
                | Self::Suffix
                | Self::After
        )
    }

    /// What the field holds when the dialog opens for `kind`.
    fn initial(self, kind: MarkKind) -> &'static str {
        match (self, kind) {
            (Self::CenterFooter, MarkKind::HeaderFooter) => "Page [page] of [pages]",
            (Self::Text, _) => "DRAFT",
            (Self::Size, MarkKind::Watermark) => "72",
            (Self::Size, _) => "10",
            (Self::Top | Self::Bottom | Self::Left | Self::Right, _) => "36",
            (Self::Start, _) => "1",
            (Self::Digits, _) => "6",
            (Self::Rotation, MarkKind::Watermark) => "45",
            (Self::Rotation, _) => "0",
            (Self::Opacity, MarkKind::Watermark) => "50",
            (Self::Opacity | Self::Scale, _) => "100",
            _ => "",
        }
    }
}

/// The fields `form` shows, in order.
pub(in crate::shell) fn fields(form: &MarkForm) -> Vec<MarkField> {
    use MarkField as F;
    match form.kind {
        MarkKind::HeaderFooter => {
            let mut fields = MarkField::LINES.to_vec();
            fields.extend([F::Size]);
            fields.extend(MarkField::MARGINS);
            fields.push(F::Start);
            fields
        }
        MarkKind::Bates => {
            let mut fields = vec![F::Prefix, F::Suffix, F::Digits, F::Start, F::Size];
            fields.extend(MarkField::MARGINS);
            if !form.other_files.is_empty() {
                fields.push(F::After);
            }
            fields
        }
        MarkKind::Watermark => match form.source {
            Source::File => vec![F::Scale, F::Rotation, F::Opacity],
            _ => vec![F::Text, F::Size, F::Rotation, F::Opacity],
        },
        MarkKind::Background => match form.source {
            Source::File => vec![F::Scale, F::Rotation, F::Opacity],
            _ => vec![F::Opacity],
        },
    }
}

/// What a watermark or background shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Source {
    Text,
    Color,
    File,
}

/// Named colours to choose from, in turn.
pub(in crate::shell) const COLORS: [(&str, [f64; 3]); 6] = [
    ("Black", [0.0, 0.0, 0.0]),
    ("Gray", [0.5, 0.5, 0.5]),
    ("Light Gray", [0.9, 0.9, 0.9]),
    ("Red", [0.8, 0.0, 0.0]),
    ("Blue", [0.0, 0.0, 0.8]),
    ("Green", [0.0, 0.5, 0.0]),
];

/// What a control that is not a typed field does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum MarkAction {
    NextFont,
    NextColor,
    SetSource(Source),
    ChooseFile,
    NextHorizontal,
    NextVertical,
    Behind,
    NextPosition,
    SetScope(CropScope),
    AddFiles,
    NumbersInNames,
    Submit,
    Remove,
}

/// The dialog's choices, apart from what is typed.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct MarkForm {
    pub(in crate::shell) kind: MarkKind,
    pub(in crate::shell) font: Font,
    /// An index into [`COLORS`].
    pub(in crate::shell) color: usize,
    pub(in crate::shell) source: Source,
    pub(in crate::shell) file: Option<PathBuf>,
    pub(in crate::shell) horizontal: HAlign,
    pub(in crate::shell) vertical: VAlign,
    pub(in crate::shell) behind: bool,
    /// An index into [`POSITIONS`], for Bates numbers.
    pub(in crate::shell) position: usize,
    pub(in crate::shell) scope: CropScope,
    /// More files to number after this document, for Bates numbering.
    pub(in crate::shell) other_files: Vec<PathBuf>,
    pub(in crate::shell) numbers_in_names: bool,
    /// Whether the document's pages already carry this kind of mark, which
    /// makes the dialog's button Update and offers Remove.
    pub(in crate::shell) existing: bool,
}

impl MarkForm {
    pub(in crate::shell) fn new(kind: MarkKind, existing: bool) -> Self {
        Self {
            kind,
            font: Font::Helvetica,
            color: match kind {
                MarkKind::Watermark => 3,
                MarkKind::Background => 2,
                _ => 0,
            },
            source: match kind {
                MarkKind::Background => Source::Color,
                _ => Source::Text,
            },
            file: None,
            horizontal: HAlign::Center,
            vertical: VAlign::Center,
            behind: false,
            position: 5,
            scope: CropScope::All,
            other_files: Vec::new(),
            numbers_in_names: true,
            existing,
        }
    }

    /// Apply a control that only changes the form. The file prompts, Add
    /// and Remove are the frame's.
    pub(in crate::shell) fn apply(&mut self, action: MarkAction) {
        match action {
            MarkAction::NextFont => self.font = next(&Font::ALL, self.font),
            MarkAction::NextColor => self.color = (self.color + 1) % COLORS.len(),
            MarkAction::SetSource(source) => self.source = source,
            MarkAction::NextHorizontal => {
                self.horizontal = next(
                    &[HAlign::Left, HAlign::Center, HAlign::Right],
                    self.horizontal,
                )
            }
            MarkAction::NextVertical => {
                self.vertical = next(
                    &[VAlign::Top, VAlign::Center, VAlign::Bottom],
                    self.vertical,
                )
            }
            MarkAction::Behind => self.behind = !self.behind,
            MarkAction::NextPosition => self.position = (self.position + 1) % POSITIONS.len(),
            MarkAction::SetScope(scope) => self.scope = scope,
            MarkAction::NumbersInNames => self.numbers_in_names = !self.numbers_in_names,
            MarkAction::ChooseFile
            | MarkAction::AddFiles
            | MarkAction::Submit
            | MarkAction::Remove => {}
        }
    }

    /// The sources a kind offers.
    pub(in crate::shell) fn sources(&self) -> &'static [Source] {
        match self.kind {
            MarkKind::Watermark => &[Source::Text, Source::File],
            MarkKind::Background => &[Source::Color, Source::File],
            _ => &[],
        }
    }

    /// Whether the kind sets text, which takes a font and a colour.
    pub(in crate::shell) fn sets_text(&self) -> bool {
        match self.kind {
            MarkKind::HeaderFooter | MarkKind::Bates => true,
            MarkKind::Watermark => self.source == Source::Text,
            MarkKind::Background => false,
        }
    }

    /// Whether the art is placed, and so aligned.
    pub(in crate::shell) fn is_placed(&self) -> bool {
        match self.kind {
            MarkKind::Watermark => true,
            MarkKind::Background => self.source == Source::File,
            _ => false,
        }
    }
}

fn next<T: Copy + PartialEq>(all: &[T], current: T) -> T {
    let at = all.iter().position(|each| *each == current).unwrap_or(0);
    all[(at + 1) % all.len()]
}

/// What a watermark or background shows, before the file is read.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) enum ArtChoice {
    Text { text: String, style: TextStyle },
    File { path: PathBuf, scale: f64 },
    Color([f64; 3]),
}

/// A mark the dialog asks for, checked.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) enum MarkRequest {
    HeaderFooter(HeaderFooter),
    Bates {
        bates: Bates,
        others: Vec<PathBuf>,
        naming: Naming,
    },
    Art {
        art: ArtChoice,
        appearance: Appearance,
    },
}

/// The request, the pages it is for, and the settings kept with the mark.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct Checked {
    pub(in crate::shell) pages: Vec<PageIndex>,
    pub(in crate::shell) request: MarkRequest,
    pub(in crate::shell) settings: String,
}

/// Read the form and what is typed into a mark, or say what is wrong.
/// `date` is what `[date]` becomes.
pub(in crate::shell) fn request(
    form: &MarkForm,
    typed: &BTreeMap<MarkField, String>,
    chosen: &[PageIndex],
    page_count: usize,
    date: &str,
) -> Result<Checked, String> {
    let pages = match form.scope {
        CropScope::Chosen => chosen.to_vec(),
        CropScope::All => (0..page_count).collect(),
    };
    if pages.is_empty() {
        return Err("There are no pages to mark.".to_owned());
    }
    let text = |field: MarkField| typed.get(&field).map_or("", |text| text.as_str());
    let number = |field: MarkField, low: f64, high: f64| -> Result<f64, String> {
        let raw = text(field).trim();
        raw.parse::<f64>()
            .ok()
            .filter(|value| value.is_finite() && (low..=high).contains(value))
            .ok_or_else(|| {
                format!(
                    "{}: {raw:?} is not a number from {low} to {high}",
                    field.label()
                )
            })
    };
    let whole = |field: MarkField, high: f64| number(field, 0.0, high).map(|value| value as usize);
    let style = || -> Result<TextStyle, String> {
        Ok(TextStyle {
            font: form.font,
            size: number(MarkField::Size, 1.0, 1000.0)?,
            color: COLORS[form.color].1,
        })
    };
    let margins = || -> Result<Margins, String> {
        Ok(Margins {
            top: number(MarkField::Top, 0.0, 10_000.0)?,
            bottom: number(MarkField::Bottom, 0.0, 10_000.0)?,
            left: number(MarkField::Left, 0.0, 10_000.0)?,
            right: number(MarkField::Right, 0.0, 10_000.0)?,
        })
    };
    let request = match form.kind {
        MarkKind::HeaderFooter => {
            let lines = MarkField::LINES.map(|field| text(field).to_owned());
            if lines.iter().all(String::is_empty) {
                return Err("Type the text of at least one header or footer.".to_owned());
            }
            MarkRequest::HeaderFooter(HeaderFooter {
                text: lines,
                style: style()?,
                margins: margins()?,
                numbering: Numbering {
                    start: whole(MarkField::Start, 1e9)?,
                    date: date.to_owned(),
                },
            })
        }
        MarkKind::Bates => MarkRequest::Bates {
            bates: Bates {
                prefix: text(MarkField::Prefix).to_owned(),
                suffix: text(MarkField::Suffix).to_owned(),
                digits: whole(MarkField::Digits, 15.0)?,
                start: whole(MarkField::Start, 1e15)? as u64,
                position: form.position,
                style: style()?,
                margins: margins()?,
            },
            others: form.other_files.clone(),
            naming: Naming {
                before: String::new(),
                after: text(MarkField::After).to_owned(),
                numbers: form.numbers_in_names,
                folder: None,
            },
        },
        MarkKind::Watermark | MarkKind::Background => {
            let art = match form.source {
                Source::Text => {
                    let words = text(MarkField::Text);
                    if words.trim().is_empty() {
                        return Err("Type the watermark's text.".to_owned());
                    }
                    ArtChoice::Text {
                        text: words.to_owned(),
                        style: style()?,
                    }
                }
                Source::Color => ArtChoice::Color(COLORS[form.color].1),
                Source::File => ArtChoice::File {
                    path: form.file.clone().ok_or("Choose a PDF file to use.")?,
                    scale: number(MarkField::Scale, 1.0, 1000.0)? / 100.0,
                },
            };
            let rotation = if fields(form).contains(&MarkField::Rotation) {
                number(MarkField::Rotation, -360.0, 360.0)?
            } else {
                0.0
            };
            MarkRequest::Art {
                art,
                appearance: Appearance {
                    rotation,
                    opacity: number(MarkField::Opacity, 0.0, 100.0)? / 100.0,
                    horizontal: form.horizontal,
                    vertical: form.vertical,
                    offset: (0.0, 0.0),
                    behind: form.behind,
                },
            }
        }
    };
    Ok(Checked {
        pages,
        request,
        settings: settings::save(form, typed),
    })
}

/// Today as `[date]` shows it: `YYYY-MM-DD`, from a PDF date string.
pub(in crate::shell) fn iso_date(pdf_date: &str) -> String {
    let digits = pdf_date.trim_start_matches("D:");
    match digits.get(0..8) {
        Some(date) if date.bytes().all(|byte| byte.is_ascii_digit()) => {
            format!("{}-{}-{}", &date[0..4], &date[4..6], &date[6..8])
        }
        _ => String::new(),
    }
}

/// The dialog, open.
pub(in crate::shell) struct MarksDialogState {
    pub(in crate::shell) form: MarkForm,
    pub(in crate::shell) chosen: Vec<PageIndex>,
    pub(in crate::shell) page_count: usize,
    pub(in crate::shell) inputs: BTreeMap<MarkField, Entity<SearchInput>>,
    pub(in crate::shell) error: Option<String>,
}

impl MarksDialogState {
    pub(in crate::shell) fn new(
        form: MarkForm,
        chosen: Vec<PageIndex>,
        page_count: usize,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let kind = form.kind;
        let inputs = MarkField::ALL
            .into_iter()
            .map(|field| {
                let input = cx.new(|cx| {
                    let mut input =
                        SearchInput::with_placeholder(field.id(), field.label(), theme, cx);
                    input.set_query(field.initial(kind), cx);
                    input
                });
                (field, input)
            })
            .collect();
        Self {
            form,
            chosen,
            page_count,
            inputs,
            error: None,
        }
    }

    /// Open on the settings a mark was made with.
    pub(in crate::shell) fn restore(&mut self, saved: &str, cx: &mut Context<ShellFrame>) {
        let mut typed = BTreeMap::new();
        let existing = self.form.existing;
        settings::restore(saved, &mut self.form, &mut typed);
        self.form.existing = existing;
        for (field, text) in typed {
            if let Some(input) = self.inputs.get(&field) {
                input.update(cx, |input, cx| input.set_query(text, cx));
            }
        }
    }

    /// A shown field's input.
    pub(in crate::shell) fn text_field(&self, field: MarkField) -> Option<&Entity<SearchInput>> {
        fields(&self.form)
            .contains(&field)
            .then(|| self.inputs.get(&field))
            .flatten()
    }

    /// The request as the dialog stands.
    pub(in crate::shell) fn request(&self, date: &str, cx: &gpui::App) -> Result<Checked, String> {
        let typed = self
            .inputs
            .iter()
            .map(|(field, input)| (*field, input.read(cx).query().to_owned()))
            .collect();
        request(&self.form, &typed, &self.chosen, self.page_count, date)
    }

    /// What the chosen-pages choice says.
    pub(in crate::shell) fn chosen_label(&self) -> String {
        match self.chosen.as_slice() {
            [page] => format!("Page {}", page + 1),
            pages => format!("The {} chosen pages", pages.len()),
        }
    }
}

/// The dialog's text fields, for the focus ring.
pub(in crate::shell) fn text_fields() -> impl Iterator<Item = TextField> {
    MarkField::ALL.into_iter().map(TextField::Mark)
}

#[cfg(test)]
mod tests;
