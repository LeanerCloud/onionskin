//! Acrobat's Print dialog and Page Setup, over `crates/print`'s job model.
//!
//! **The dialog owns no printing logic.** Every choice here is an argument
//! to [`onionskin_print::impose`]; the preview is that function's sheets and
//! the print is the same sheets sent to a backend, so the preview cannot
//! disagree with the output.
//!
//! **Page Setup and the Print dialog share one [`PageSetup`]**, kept by the
//! frame, so they cannot disagree about the paper either.
//!
//! The controls are data: [`groups`] lists every choice with its label,
//! state and action, and both the drawing and the accessibility tree are
//! built from that list, so a control cannot be drawn without being
//! operable from the keyboard, nor described without being drawn.

mod view;

pub(in crate::shell) use view::{accessible, accessible_setup, render, render_setup};

use gpui::{AppContext as _, Context, Entity};
use onionskin_core::AnnotationFilter;
use onionskin_print::{
    impose, parse_page_ranges, Duplex, NUp, NUpOrder, Orientation, PageSelection, PageSize,
    PaperSize, PrintJob, Sheet, Sizing, Subset,
};

use super::accessible::TextField;
use super::{SearchInput, ShellFrame, ThemeTokens};

/// Pages to Print.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) enum PagesChoice {
    #[default]
    All,
    Current,
    /// The pages typed in the Pages box.
    Custom,
}

/// Page Sizing & Handling's Size choices, the custom percentage typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) enum SizingChoice {
    #[default]
    Fit,
    ActualSize,
    ShrinkOversized,
    Custom,
}

/// Where the sheets go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) enum Destination {
    /// Print to a PDF file: the file backend.
    SaveAsPdf,
    /// A printer the platform knows, by name.
    Printer(String),
}

impl Destination {
    pub(in crate::shell) fn label(&self) -> String {
        match self {
            Self::SaveAsPdf => "Save as PDF".to_owned(),
            Self::Printer(name) => name.clone(),
        }
    }
}

/// Paper and orientation: Page Setup's two questions, which the Print
/// dialog asks too, from the same place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) struct PageSetup {
    /// An index into [`PaperSize::ALL`].
    pub(in crate::shell) paper: usize,
    pub(in crate::shell) orientation: Orientation,
}

impl PageSetup {
    pub(in crate::shell) fn paper(self) -> PaperSize {
        PaperSize::ALL[self.paper.min(PaperSize::ALL.len() - 1)]
    }
}

/// What a control in either dialog does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum PrintAction {
    Destination(usize),
    Collate,
    Pages(PagesChoice),
    Subset(Subset),
    Reverse,
    Sizing(SizingChoice),
    PerSheet(u8),
    Order(NUpOrder),
    Borders,
    Orientation(Orientation),
    Paper(usize),
    Comments(AnnotationFilter),
    Duplex(Duplex),
    PrintAsImage,
    /// Summarize Comments: the comment summary prints after the document.
    SummarizeComments,
    PreviewPrevious,
    PreviewNext,
    Print,
    Cancel,
}

/// Every choice the dialog holds that is not typed.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct PrintSettings {
    pub(in crate::shell) destination: usize,
    pub(in crate::shell) collate: bool,
    pub(in crate::shell) pages: PagesChoice,
    pub(in crate::shell) subset: Subset,
    pub(in crate::shell) reverse: bool,
    pub(in crate::shell) sizing: SizingChoice,
    pub(in crate::shell) n_up: NUp,
    pub(in crate::shell) comments: AnnotationFilter,
    pub(in crate::shell) duplex: Duplex,
    pub(in crate::shell) print_as_image: bool,
    pub(in crate::shell) summarize_comments: bool,
}

impl Default for PrintSettings {
    fn default() -> Self {
        PrintSettings {
            destination: 0,
            collate: true,
            pages: PagesChoice::All,
            subset: Subset::All,
            reverse: false,
            sizing: SizingChoice::Fit,
            n_up: NUp::default(),
            comments: AnnotationFilter::DocumentAndMarkups,
            duplex: Duplex::Off,
            print_as_image: false,
            summarize_comments: false,
        }
    }
}

/// The three things the dialog has typed into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct Typed {
    pub(in crate::shell) copies: String,
    pub(in crate::shell) pages: String,
    pub(in crate::shell) scale: String,
}

/// What the document being printed is, as far as the dialog needs it.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct Printed {
    pub(in crate::shell) page_sizes: Vec<PageSize>,
    pub(in crate::shell) current_page: usize,
    /// Why this document prints only as images, when it does.
    pub(in crate::shell) image_only: Option<&'static str>,
}

/// Apply a choice to the settings. `false` for the actions that are not
/// settings (preview paging, Print, Cancel), which the frame handles.
pub(in crate::shell) fn apply(settings: &mut PrintSettings, action: PrintAction) -> bool {
    match action {
        PrintAction::Destination(index) => settings.destination = index,
        PrintAction::Collate => settings.collate = !settings.collate,
        PrintAction::Pages(pages) => settings.pages = pages,
        PrintAction::Subset(subset) => settings.subset = subset,
        PrintAction::Reverse => settings.reverse = !settings.reverse,
        PrintAction::Sizing(sizing) => settings.sizing = sizing,
        PrintAction::PerSheet(per_sheet) => settings.n_up.per_sheet = per_sheet,
        PrintAction::Order(order) => settings.n_up.order = order,
        PrintAction::Borders => settings.n_up.borders = !settings.n_up.borders,
        PrintAction::Comments(comments) => settings.comments = comments,
        PrintAction::Duplex(duplex) => settings.duplex = duplex,
        PrintAction::PrintAsImage => settings.print_as_image = !settings.print_as_image,
        PrintAction::SummarizeComments => {
            settings.summarize_comments = !settings.summarize_comments;
        }
        PrintAction::Orientation(_)
        | PrintAction::Paper(_)
        | PrintAction::PreviewPrevious
        | PrintAction::PreviewNext
        | PrintAction::Print
        | PrintAction::Cancel => return false,
    }
    true
}

/// Apply a Page Setup choice. `false` for anything else.
pub(in crate::shell) fn apply_setup(setup: &mut PageSetup, action: PrintAction) -> bool {
    match action {
        PrintAction::Orientation(orientation) => setup.orientation = orientation,
        PrintAction::Paper(paper) => setup.paper = paper.min(PaperSize::ALL.len() - 1),
        _ => return false,
    }
    true
}

/// The job the dialog describes, or what is wrong with what was typed.
pub(in crate::shell) fn job(
    settings: &PrintSettings,
    setup: PageSetup,
    typed: &Typed,
    printed: &Printed,
    destinations: &[Destination],
) -> Result<PrintJob, String> {
    let count = printed.page_sizes.len();
    let ranges = match settings.pages {
        PagesChoice::All => Vec::new(),
        PagesChoice::Current => vec![(printed.current_page, printed.current_page)],
        PagesChoice::Custom => {
            parse_page_ranges(&typed.pages, count).map_err(|error| error.to_string())?
        }
    };
    let sizing = match settings.sizing {
        SizingChoice::Fit => Sizing::Fit,
        SizingChoice::ActualSize => Sizing::ActualSize,
        SizingChoice::ShrinkOversized => Sizing::ShrinkOversized,
        SizingChoice::Custom => Sizing::Custom(percent(&typed.scale)?),
    };
    Ok(PrintJob {
        paper: setup.paper(),
        orientation: setup.orientation,
        selection: PageSelection {
            ranges,
            subset: settings.subset,
            reverse: settings.reverse,
        },
        sizing,
        n_up: settings.n_up,
        duplex: settings.duplex,
        copies: copies(&typed.copies)?,
        collate: settings.collate,
        printer: match destinations.get(settings.destination) {
            Some(Destination::Printer(name)) => Some(name.clone()),
            _ => None,
        },
        comments: settings.comments,
        print_as_image: settings.print_as_image || printed.image_only.is_some(),
        image_dpi: 150.0,
    })
}

/// The sheets the job prints: the preview, and exactly what the backend
/// is given.
pub(in crate::shell) fn sheets(job: &PrintJob, printed: &Printed) -> Vec<Sheet> {
    impose(job, &printed.page_sizes)
}

/// Custom Scale, 1 to 999 percent.
fn percent(text: &str) -> Result<u16, String> {
    let text = text.trim().trim_end_matches('%').trim();
    text.parse::<u16>()
        .ok()
        .filter(|percent| (1..=999).contains(percent))
        .ok_or_else(|| {
            format!("Custom Scale must be a whole percentage from 1 to 999, not {text:?}")
        })
}

/// Copies, 1 to 999.
fn copies(text: &str) -> Result<u16, String> {
    let text = text.trim();
    text.parse::<u16>()
        .ok()
        .filter(|copies| (1..=999).contains(copies))
        .ok_or_else(|| format!("Copies must be a whole number from 1 to 999, not {text:?}"))
}

/// The dialog, open.
pub(in crate::shell) struct PrintDialogState {
    pub(in crate::shell) settings: PrintSettings,
    pub(in crate::shell) printed: Printed,
    pub(in crate::shell) destinations: Vec<Destination>,
    pub(in crate::shell) copies: Entity<SearchInput>,
    pub(in crate::shell) pages: Entity<SearchInput>,
    pub(in crate::shell) scale: Entity<SearchInput>,
    /// Which sheet the preview shows.
    pub(in crate::shell) preview_sheet: usize,
    pub(in crate::shell) error: Option<String>,
}

/// The fields' ids, which the tree and the focus ring share.
pub(in crate::shell) const TEXT_FIELDS: [TextField; 3] = [
    TextField::PrintCopies,
    TextField::PrintPages,
    TextField::PrintScale,
];

impl PrintDialogState {
    pub(in crate::shell) fn new(
        printed: Printed,
        destinations: Vec<Destination>,
        theme: ThemeTokens,
        cx: &mut Context<ShellFrame>,
    ) -> Self {
        let mut field = |id: &'static str, placeholder: &'static str, value: String| {
            cx.new(|cx| {
                let mut input = SearchInput::with_placeholder(id, placeholder, theme, cx);
                input.set_query(value, cx);
                input
            })
        };
        let pages = format!("1-{}", printed.page_sizes.len().max(1));
        let settings = PrintSettings {
            print_as_image: printed.image_only.is_some(),
            ..PrintSettings::default()
        };
        Self {
            copies: field("print-copies", "Copies", "1".to_owned()),
            pages: field("print-pages", "Pages, like 2-4, 7", pages),
            scale: field("print-scale", "Custom Scale (%)", "100".to_owned()),
            settings,
            printed,
            destinations,
            preview_sheet: 0,
            error: None,
        }
    }

    pub(in crate::shell) fn typed(&self, cx: &gpui::App) -> Typed {
        Typed {
            copies: self.copies.read(cx).query().to_owned(),
            pages: self.pages.read(cx).query().to_owned(),
            scale: self.scale.read(cx).query().to_owned(),
        }
    }

    pub(in crate::shell) fn job(
        &self,
        setup: PageSetup,
        cx: &gpui::App,
    ) -> Result<PrintJob, String> {
        job(
            &self.settings,
            setup,
            &self.typed(cx),
            &self.printed,
            &self.destinations,
        )
    }

    /// The preview's sheets, or why there are none.
    pub(in crate::shell) fn preview(
        &self,
        setup: PageSetup,
        cx: &gpui::App,
    ) -> Result<Vec<Sheet>, String> {
        self.job(setup, cx).map(|job| sheets(&job, &self.printed))
    }

    pub(in crate::shell) fn text_field(&self, field: TextField) -> Option<&Entity<SearchInput>> {
        match field {
            TextField::PrintCopies => Some(&self.copies),
            TextField::PrintPages => Some(&self.pages),
            TextField::PrintScale => Some(&self.scale),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests;
