//! The Print and Page Setup dialogs, drawn and described from one list of
//! controls.

use accesskit::Role;
use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, Context, Entity, InteractiveElement as _, IntoElement, ParentElement as _,
    StatefulInteractiveElement as _, Styled as _,
};
use onionskin_core::AnnotationFilter;
use onionskin_print::{
    Binding, BookletSides, Duplex, NUp, NUpOrder, Orientation, PaperSize, Sheet, Subset,
};

use super::{
    HandlingChoice, PageSetup, PagesChoice, PrintAction, PrintDialogState, PrintSettings,
    SizingChoice,
};
use crate::a11y::State as A11yState;
use crate::shell::chrome::accessible::{Activation, Element, TextField};
use crate::shell::chrome::combine_dialog::button;
use crate::shell::chrome::{SearchInput, ShellFrame, ThemeTokens};

/// One control.
enum Control {
    /// One of a set.
    Radio {
        label: String,
        selected: bool,
        action: PrintAction,
    },
    /// On or off, with the reason it cannot be changed when it cannot.
    Check {
        label: &'static str,
        checked: bool,
        action: PrintAction,
        locked: Option<&'static str>,
    },
    /// A typed value.
    Field {
        label: &'static str,
        field: TextField,
    },
}

/// A labelled set of controls.
struct Group {
    id: &'static str,
    label: &'static str,
    controls: Vec<Control>,
}

fn radio(label: impl Into<String>, selected: bool, action: PrintAction) -> Control {
    Control::Radio {
        label: label.into(),
        selected,
        action,
    }
}

fn check(label: &'static str, checked: bool, action: PrintAction) -> Control {
    Control::Check {
        label,
        checked,
        action,
        locked: None,
    }
}

const ORIENTATIONS: [(Orientation, &str); 3] = [
    (Orientation::Auto, "Auto portrait/landscape"),
    (Orientation::Portrait, "Portrait"),
    (Orientation::Landscape, "Landscape"),
];

const COMMENTS: [(AnnotationFilter, &str); 4] = [
    (AnnotationFilter::DocumentAndMarkups, "Document and Markups"),
    (AnnotationFilter::DocumentOnly, "Document"),
    (AnnotationFilter::DocumentAndStamps, "Document and Stamps"),
    (AnnotationFilter::FormFieldsOnly, "Form Fields Only"),
];

const DUPLEX: [(Duplex, &str); 3] = [
    (Duplex::Off, "Print on one side"),
    (Duplex::LongEdge, "Both sides, flip on long edge"),
    (Duplex::ShortEdge, "Both sides, flip on short edge"),
];

const ORDERS: [(NUpOrder, &str); 4] = [
    (NUpOrder::Horizontal, "Horizontal"),
    (NUpOrder::HorizontalReversed, "Horizontal Reversed"),
    (NUpOrder::Vertical, "Vertical"),
    (NUpOrder::VerticalReversed, "Vertical Reversed"),
];

const SUBSETS: [(Subset, &str); 3] = [
    (Subset::All, "All pages in range"),
    (Subset::Odd, "Odd pages only"),
    (Subset::Even, "Even pages only"),
];

const HANDLINGS: [(HandlingChoice, &str); 3] = [
    (HandlingChoice::Pages, "Size and Multiple"),
    (HandlingChoice::Booklet, "Booklet"),
    (HandlingChoice::Poster, "Poster"),
];

const BOOKLET_SIDES: [(BookletSides, &str); 3] = [
    (BookletSides::BothSides, "Both sides"),
    (BookletSides::FrontSideOnly, "Front side only"),
    (BookletSides::BackSideOnly, "Back side only"),
];

const BINDINGS: [(Binding, &str); 2] = [(Binding::Left, "Left"), (Binding::Right, "Right")];

const SIZES: [(SizingChoice, &str); 4] = [
    (SizingChoice::Fit, "Fit"),
    (SizingChoice::ActualSize, "Actual size"),
    (SizingChoice::ShrinkOversized, "Shrink oversized pages"),
    (SizingChoice::Custom, "Custom Scale"),
];

/// Page Setup's groups, which the Print dialog shows too.
fn setup_groups(setup: PageSetup) -> Vec<Group> {
    vec![
        Group {
            id: "print-paper",
            label: "Paper Size",
            controls: PaperSize::ALL
                .iter()
                .enumerate()
                .map(|(index, paper)| {
                    radio(paper.name, setup.paper == index, PrintAction::Paper(index))
                })
                .collect(),
        },
        Group {
            id: "print-orientation",
            label: "Orientation",
            controls: ORIENTATIONS
                .iter()
                .map(|(orientation, label)| {
                    radio(
                        *label,
                        setup.orientation == *orientation,
                        PrintAction::Orientation(*orientation),
                    )
                })
                .collect(),
        },
    ]
}

/// Every group the Print dialog shows, in tab order.
fn groups(state: &PrintDialogState, setup: PageSetup) -> Vec<Group> {
    let settings = &state.settings;
    let mut groups = vec![
        Group {
            id: "print-printer",
            label: "Printer",
            controls: state
                .destinations
                .iter()
                .enumerate()
                .map(|(index, destination)| {
                    radio(
                        destination.label(),
                        settings.destination == index,
                        PrintAction::Destination(index),
                    )
                })
                .collect(),
        },
        Group {
            id: "print-copies-group",
            label: "Copies",
            controls: vec![
                Control::Field {
                    label: "Copies",
                    field: TextField::PrintCopies,
                },
                check("Collate", settings.collate, PrintAction::Collate),
            ],
        },
        pages_group(state),
        choices(
            "print-handling",
            "Page Sizing & Handling",
            &HANDLINGS,
            settings.handling,
            PrintAction::Handling,
        ),
    ];
    groups.extend(handling_groups(state));
    groups.extend(setup_groups(setup));
    groups.push(choices(
        "print-comments",
        "Comments & Forms",
        &COMMENTS,
        settings.comments,
        PrintAction::Comments,
    ));
    groups.push(choices(
        "print-duplex",
        "Print on Both Sides of Paper",
        &DUPLEX,
        settings.duplex,
        PrintAction::Duplex,
    ));
    groups.push(Group {
        id: "print-advanced",
        label: "Advanced",
        controls: vec![
            Control::Check {
                label: "Print as Image",
                checked: settings.print_as_image || state.printed.image_only.is_some(),
                action: PrintAction::PrintAsImage,
                locked: state.printed.image_only,
            },
            Control::Check {
                label: "Summarize Comments",
                checked: settings.summarize_comments,
                action: PrintAction::SummarizeComments,
                locked: SUMMARY_LOCK,
            },
        ],
    });
    groups
}

/// Why Summarize Comments cannot be chosen in this build, if it cannot.
#[cfg(feature = "tools-comment")]
const SUMMARY_LOCK: Option<&str> = None;
#[cfg(not(feature = "tools-comment"))]
const SUMMARY_LOCK: Option<&str> = Some("The comment tools plugin is not installed");

fn choices<T: Copy + PartialEq>(
    id: &'static str,
    label: &'static str,
    options: &[(T, &'static str)],
    chosen: T,
    action: fn(T) -> PrintAction,
) -> Group {
    Group {
        id,
        label,
        controls: options
            .iter()
            .map(|(value, text)| radio(*text, *value == chosen, action(*value)))
            .collect(),
    }
}

fn pages_group(state: &PrintDialogState) -> Group {
    let settings = &state.settings;
    let current = state.printed.current_page + 1;
    let mut controls = vec![
        radio(
            "All",
            settings.pages == PagesChoice::All,
            PrintAction::Pages(PagesChoice::All),
        ),
        radio(
            format!("Current page ({current})"),
            settings.pages == PagesChoice::Current,
            PrintAction::Pages(PagesChoice::Current),
        ),
        radio(
            "Pages",
            settings.pages == PagesChoice::Custom,
            PrintAction::Pages(PagesChoice::Custom),
        ),
    ];
    if settings.pages == PagesChoice::Custom {
        controls.push(Control::Field {
            label: "Pages",
            field: TextField::PrintPages,
        });
    }
    controls.extend(SUBSETS.iter().map(|(subset, label)| {
        radio(
            *label,
            settings.subset == *subset,
            PrintAction::Subset(*subset),
        )
    }));
    controls.push(check(
        "Reverse pages",
        settings.reverse,
        PrintAction::Reverse,
    ));
    Group {
        id: "print-pages-group",
        label: "Pages to Print",
        controls,
    }
}

fn sizing_group(state: &PrintDialogState) -> Group {
    let chosen = state.settings.sizing;
    let mut controls: Vec<Control> = SIZES
        .iter()
        .map(|(sizing, label)| radio(*label, chosen == *sizing, PrintAction::Sizing(*sizing)))
        .collect();
    if chosen == SizingChoice::Custom {
        controls.push(Control::Field {
            label: "Custom Scale (%)",
            field: TextField::PrintScale,
        });
    }
    Group {
        id: "print-sizing",
        label: "Page Sizing & Handling",
        controls,
    }
}

/// The groups the chosen handling asks for: size and pages per sheet, or
/// the booklet's sides and binding, or the poster's tiles.
fn handling_groups(state: &PrintDialogState) -> Vec<Group> {
    let settings: &PrintSettings = &state.settings;
    match settings.handling {
        HandlingChoice::Pages => vec![sizing_group(state), n_up_group(settings.n_up)],
        HandlingChoice::Booklet => vec![
            choices(
                "print-booklet-sides",
                "Booklet subset",
                &BOOKLET_SIDES,
                settings.booklet.sides,
                PrintAction::BookletSides,
            ),
            Group {
                id: "print-booklet-sheets",
                label: "Sheets",
                controls: vec![
                    Control::Field {
                        label: "Sheets from",
                        field: TextField::PrintBookletFrom,
                    },
                    Control::Field {
                        label: "To",
                        field: TextField::PrintBookletTo,
                    },
                ],
            },
            choices(
                "print-binding",
                "Binding",
                &BINDINGS,
                settings.booklet.binding,
                PrintAction::Binding,
            ),
        ],
        HandlingChoice::Poster => {
            vec![
                Group {
                    id: "print-poster",
                    label: "Poster",
                    controls: vec![
                        Control::Field {
                            label: "Tile Scale (%)",
                            field: TextField::PrintPosterScale,
                        },
                        Control::Field {
                            label: "Overlap (in)",
                            field: TextField::PrintPosterOverlap,
                        },
                    ],
                },
                Group {
                    id: "print-poster-marks",
                    label: "Marks",
                    controls: vec![check(
                        "Cut marks",
                        settings.poster.cut_marks,
                        PrintAction::CutMarks,
                    )],
                },
            ]
        }
    }
}

fn n_up_group(n_up: NUp) -> Group {
    let mut controls: Vec<Control> = NUp::CHOICES
        .iter()
        .map(|per_sheet| {
            radio(
                format!("{per_sheet} per sheet"),
                n_up.per_sheet == *per_sheet,
                PrintAction::PerSheet(*per_sheet),
            )
        })
        .collect();
    // Order and borders mean something only when pages share a sheet.
    if n_up.per_sheet > 1 {
        controls.extend(
            ORDERS.iter().map(|(order, label)| {
                radio(*label, n_up.order == *order, PrintAction::Order(*order))
            }),
        );
        controls.push(check(
            "Print page border",
            n_up.borders,
            PrintAction::Borders,
        ));
    }
    Group {
        id: "print-n-up",
        label: "Multiple Pages per Sheet",
        controls,
    }
}

// ----- the accessibility tree --------------------------------------------

fn activation(action: PrintAction) -> Activation {
    Activation::Print(action)
}

fn describe(group: &Group, fields: &dyn Fn(TextField) -> Option<Element>) -> Element {
    let fields_only = group
        .controls
        .iter()
        .all(|control| matches!(control, Control::Field { .. }));
    let children = group
        .controls
        .iter()
        .enumerate()
        .filter_map(|(index, control)| match control {
            Control::Radio {
                label,
                selected,
                action,
            } => Some(
                Element::new((group.id, index), Role::RadioButton, label.clone())
                    .with_state(A11yState::selected(*selected))
                    .with_activation(activation(*action)),
            ),
            Control::Check {
                label,
                checked,
                action,
                locked,
            } => {
                let element =
                    Element::new((group.id, index), Role::CheckBox, *label).with_state(A11yState {
                        toggled: Some(*checked),
                        disabled: locked.is_some(),
                        ..A11yState::default()
                    });
                Some(match locked {
                    Some(reason) => element.with_description(*reason),
                    None => element.with_activation(activation(*action)),
                })
            }
            Control::Field { field, .. } => fields(*field).map(|element| {
                if fields_only {
                    Element::new((group.id, index), Role::Group, "").with_children(vec![element])
                } else {
                    element
                }
            }),
        })
        .collect();
    Element::new(group.id, Role::Group, group.label).with_children(children)
}

/// What the preview says: how many sheets, and what is on the one shown.
pub(super) fn preview_label(sheets: &[Sheet], shown: usize) -> String {
    match sheets.get(shown) {
        None => "Nothing to print".to_owned(),
        Some(sheet) => {
            let pages: Vec<String> = sheet
                .placements
                .iter()
                .map(|placement| (placement.source + 1).to_string())
                .collect();
            let content = match pages.as_slice() {
                [] => "blank".to_owned(),
                [one] => format!("page {one}"),
                many => format!("pages {}", many.join(", ")),
            };
            format!(
                "Preview: sheet {} of {}, {}, {}",
                shown + 1,
                sheets.len(),
                if sheet.is_landscape() {
                    "landscape"
                } else {
                    "portrait"
                },
                content
            )
        }
    }
}

/// The Print dialog, for a screen reader.
pub(in crate::shell) fn accessible(
    state: &PrintDialogState,
    setup: PageSetup,
    cx: &gpui::App,
) -> Vec<Element> {
    let fields = |field: TextField| {
        let (input, label) = match field {
            TextField::PrintCopies => (&state.copies, "Copies"),
            TextField::PrintPages => (&state.pages, "Pages"),
            TextField::PrintScale => (&state.scale, "Custom Scale (%)"),
            TextField::PrintPosterScale => (&state.poster_scale, "Tile Scale (%)"),
            TextField::PrintPosterOverlap => (&state.poster_overlap, "Overlap (in)"),
            TextField::PrintBookletFrom => (&state.booklet_from, "Sheets from"),
            TextField::PrintBookletTo => (&state.booklet_to, "To"),
            _ => unreachable!("non-print text field in print dialog"),
        };
        Some(input.read(cx).accessible(label, field))
    };
    let mut body: Vec<Element> = groups(state, setup)
        .iter()
        .map(|group| describe(group, &fields))
        .collect();
    let preview = state.preview(setup, cx);
    let sheet_count = preview.as_ref().map_or(0, Vec::len);
    let shown = state.preview_index(sheet_count);
    body.push(Element::new(
        "print-preview",
        Role::Label,
        match &preview {
            Ok(sheets) => preview_label(sheets, shown),
            Err(_) => "No preview until the settings are fixed".to_owned(),
        },
    ));
    body.push(
        Element::new("print-preview-previous", Role::Button, "Previous Sheet")
            .with_state(A11yState::enabled(shown > 0))
            .with_activation(activation(PrintAction::PreviewPrevious)),
    );
    body.push(
        Element::new("print-preview-next", Role::Button, "Next Sheet")
            .with_state(A11yState::enabled(shown + 1 < sheet_count))
            .with_activation(activation(PrintAction::PreviewNext)),
    );
    if let Some(error) = state.error.clone().or(preview.err()) {
        body.push(Element::new("print-error", Role::Alert, error));
    }
    body.push(
        Element::new("print-submit", Role::Button, "Print")
            .with_activation(activation(PrintAction::Print)),
    );
    body.push(
        Element::new("print-cancel", Role::Button, "Cancel")
            .with_activation(activation(PrintAction::Cancel)),
    );
    body
}

/// Page Setup, for a screen reader.
pub(in crate::shell) fn accessible_setup(setup: PageSetup) -> Vec<Element> {
    let mut body: Vec<Element> = setup_groups(setup)
        .iter()
        .map(|group| describe(group, &|_| None))
        .collect();
    body.push(
        Element::new("page-setup-ok", Role::Button, "OK").with_activation(Activation::CloseDialog),
    );
    body
}

// ----- drawn ---------------------------------------------------------------

fn draw_control(
    group: &'static str,
    index: usize,
    control: &Control,
    inputs: &dyn Fn(TextField) -> Option<Entity<SearchInput>>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let clickable =
        |label: String, marker: &str, action: PrintAction, cx: &mut Context<ShellFrame>| {
            div()
                .id((group, index))
                .px_1()
                .rounded_sm()
                .cursor_pointer()
                .hover(move |row| row.bg(theme.subtle_hover))
                .on_click(cx.listener(move |frame, _event, window, cx| {
                    frame.run_activation(activation(action), window, cx);
                }))
                .child(format!("{marker} {label}"))
                .into_any_element()
        };
    match control {
        Control::Radio {
            label,
            selected,
            action,
        } => clickable(
            label.clone(),
            if *selected { "(•)" } else { "( )" },
            *action,
            cx,
        ),
        Control::Check {
            label,
            checked,
            action,
            locked,
        } => {
            let marker = if *checked { "[x]" } else { "[ ]" };
            match locked {
                Some(reason) => div()
                    .id((group, index))
                    .px_1()
                    .text_color(theme.disabled_text)
                    .child(format!("{marker} {label} ({reason})"))
                    .into_any_element(),
                None => clickable((*label).to_owned(), marker, *action, cx),
            }
        }
        Control::Field { label, field } => div()
            .flex()
            .gap_1()
            .items_center()
            .child(div().text_color(theme.secondary_text).child(*label))
            .when_some(inputs(*field), |row, input| {
                row.child(
                    div()
                        .w(px(120.0))
                        .px_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(theme.selected)
                        .child(input),
                )
            })
            .into_any_element(),
    }
}

fn draw_group(
    group: &Group,
    inputs: &dyn Fn(TextField) -> Option<Entity<SearchInput>>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> gpui::AnyElement {
    let mut row = div().flex().flex_wrap().gap_x_2();
    for (index, control) in group.controls.iter().enumerate() {
        row = row.child(draw_control(group.id, index, control, inputs, theme, cx));
    }
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .text_xs()
                .text_color(theme.secondary_text)
                .child(group.label),
        )
        .child(row)
        .into_any_element()
}

/// The preview: the shown sheet as an outline, each placed page as a box
/// numbered with its page, where imposition put it.
fn draw_preview(
    sheets: &[Sheet],
    shown: usize,
    state: &PrintDialogState,
    theme: ThemeTokens,
) -> gpui::AnyElement {
    const SIDE: f32 = 160.0;
    let Some(sheet) = sheets.get(shown) else {
        return div().child("Nothing to print").into_any_element();
    };
    let scale = SIDE / sheet.width.max(sheet.height) as f32;
    let (width, height) = (sheet.width as f32 * scale, sheet.height as f32 * scale);
    let mut paper = div()
        .relative()
        .w(px(width))
        .h(px(height))
        .bg(gpui::white())
        .border_1()
        .border_color(theme.hover);
    for placement in &sheet.placements {
        let (page_width, page_height) = state.printed.page_sizes[placement.source];
        let [x0, y0, x1, y1] = placement.visible(page_width, page_height);
        paper = paper.child(
            div()
                .absolute()
                .left(px(x0 as f32 * scale))
                .top(px((sheet.height - y1) as f32 * scale))
                .w(px(((x1 - x0) as f32 * scale).max(1.0)))
                .h(px(((y1 - y0) as f32 * scale).max(1.0)))
                .border_1()
                .border_color(gpui::black())
                .flex()
                .items_center()
                .justify_center()
                .text_xs()
                .text_color(gpui::black())
                .child((placement.source + 1).to_string()),
        );
    }
    div()
        .flex()
        .flex_col()
        .gap_1()
        .child(paper)
        .child(
            div()
                .text_xs()
                .text_color(theme.secondary_text)
                .child(preview_label(sheets, shown)),
        )
        .into_any_element()
}

/// The Print dialog, drawn.
pub(in crate::shell) fn render(
    state: &PrintDialogState,
    setup: PageSetup,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let inputs = |field: TextField| state.text_field(field).cloned();
    let mut settings = div().flex().flex_col().gap_2().w(px(420.0));
    for group in groups(state, setup) {
        settings = settings.child(draw_group(&group, &inputs, theme, cx));
    }
    let preview = state.preview(setup, cx);
    let sheets = preview.as_deref().unwrap_or_default();
    let sheet_count = sheets.len();
    let shown = state.preview_index(sheet_count);
    let error = state.error.clone().or(preview.as_ref().err().cloned());
    let side = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(draw_preview(sheets, shown, state, theme))
        .child(
            div()
                .flex()
                .gap_1()
                .child(button(
                    "print-preview-previous",
                    "Previous Sheet",
                    shown > 0,
                    theme,
                    focused,
                    cx,
                    activation(PrintAction::PreviewPrevious),
                ))
                .child(button(
                    "print-preview-next",
                    "Next Sheet",
                    shown + 1 < sheet_count,
                    theme,
                    focused,
                    cx,
                    activation(PrintAction::PreviewNext),
                )),
        );
    div()
        .flex()
        .flex_col()
        .gap_3()
        .child(div().flex().gap_4().child(settings).child(side))
        .when_some(error, |body, error| {
            body.child(
                div()
                    .id("print-error")
                    .text_color(theme.error_text)
                    .child(error),
            )
        })
        .child(
            div()
                .flex()
                .gap_2()
                .child(button(
                    "print-submit",
                    "Print",
                    true,
                    theme,
                    focused,
                    cx,
                    activation(PrintAction::Print),
                ))
                .child(button(
                    "print-cancel",
                    "Cancel",
                    true,
                    theme,
                    focused,
                    cx,
                    activation(PrintAction::Cancel),
                )),
        )
}

/// Page Setup, drawn.
pub(in crate::shell) fn render_setup(
    setup: PageSetup,
    focused: Option<&gpui::ElementId>,
    theme: ThemeTokens,
    cx: &mut Context<ShellFrame>,
) -> impl IntoElement {
    let mut body = div().flex().flex_col().gap_2();
    for group in setup_groups(setup) {
        body = body.child(draw_group(&group, &|_| None, theme, cx));
    }
    body.child(button(
        "page-setup-ok",
        "OK",
        true,
        theme,
        focused,
        cx,
        Activation::CloseDialog,
    ))
}
