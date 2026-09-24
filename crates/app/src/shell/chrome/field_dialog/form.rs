//! What the Properties dialog's choices mean, as plain data: the tabs a
//! field has, the fields typed on each, what each control does, and the
//! properties the whole comes to. Tested without a window.

use onionskin_core::forms::{ChoiceOption, FieldProperties, FieldScripts, KindOptions};
use onionskin_tools_form::scripts::{Calculate, Format, Op, Validate};

/// The dialog's tabs, in Acrobat's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Tab {
    General,
    Appearance,
    Position,
    Options,
    Format,
    Validate,
    Calculate,
}

impl Tab {
    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Tab::General => "General",
            Tab::Appearance => "Appearance",
            Tab::Position => "Position",
            Tab::Options => "Options",
            Tab::Format => "Format",
            Tab::Validate => "Validate",
            Tab::Calculate => "Calculate",
        }
    }
}

/// A typed field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(in crate::shell) enum FieldInput {
    Name,
    Tooltip,
    Left,
    Bottom,
    Width,
    Height,
    DefaultText,
    MaxLength,
    Export,
    Caption,
    OptionItem,
    OptionExport,
    Currency,
    DatePattern,
    CustomKeystroke,
    CustomFormat,
    RangeMin,
    RangeMax,
    CustomValidate,
    CalculateFields,
    CustomCalculate,
}

impl FieldInput {
    pub(in crate::shell) const ALL: [FieldInput; 21] = [
        FieldInput::Name,
        FieldInput::Tooltip,
        FieldInput::Left,
        FieldInput::Bottom,
        FieldInput::Width,
        FieldInput::Height,
        FieldInput::DefaultText,
        FieldInput::MaxLength,
        FieldInput::Export,
        FieldInput::Caption,
        FieldInput::OptionItem,
        FieldInput::OptionExport,
        FieldInput::Currency,
        FieldInput::DatePattern,
        FieldInput::CustomKeystroke,
        FieldInput::CustomFormat,
        FieldInput::RangeMin,
        FieldInput::RangeMax,
        FieldInput::CustomValidate,
        FieldInput::CalculateFields,
        FieldInput::CustomCalculate,
    ];

    pub(in crate::shell) fn id(self) -> &'static str {
        match self {
            Self::Name => "field-name",
            Self::Tooltip => "field-tooltip",
            Self::Left => "field-left",
            Self::Bottom => "field-bottom",
            Self::Width => "field-width",
            Self::Height => "field-height",
            Self::DefaultText => "field-default",
            Self::MaxLength => "field-max-length",
            Self::Export => "field-export",
            Self::Caption => "field-caption",
            Self::OptionItem => "field-option-item",
            Self::OptionExport => "field-option-export",
            Self::Currency => "field-currency",
            Self::DatePattern => "field-date-pattern",
            Self::CustomKeystroke => "field-custom-keystroke",
            Self::CustomFormat => "field-custom-format",
            Self::RangeMin => "field-range-min",
            Self::RangeMax => "field-range-max",
            Self::CustomValidate => "field-custom-validate",
            Self::CalculateFields => "field-calculate-fields",
            Self::CustomCalculate => "field-custom-calculate",
        }
    }

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::Tooltip => "Tooltip",
            Self::Left => "Left (points)",
            Self::Bottom => "Bottom (points)",
            Self::Width => "Width (points)",
            Self::Height => "Height (points)",
            Self::DefaultText => "Default value",
            Self::MaxLength => "Limit of characters",
            Self::Export => "Export value",
            Self::Caption => "Label",
            Self::OptionItem => "Item",
            Self::OptionExport => "Export value of the item",
            Self::Currency => "Currency symbol",
            Self::DatePattern => "Date format",
            Self::CustomKeystroke => "Custom keystroke script",
            Self::CustomFormat => "Custom format script",
            Self::RangeMin => "Greater than or equal to",
            Self::RangeMax => "Less than or equal to",
            Self::CustomValidate => "Custom validation script",
            Self::CalculateFields => "Fields, separated by commas",
            Self::CustomCalculate => "Custom calculation script",
        }
    }

    pub(in crate::shell) fn numeric(self) -> bool {
        matches!(
            self,
            Self::Left
                | Self::Bottom
                | Self::Width
                | Self::Height
                | Self::MaxLength
                | Self::RangeMin
                | Self::RangeMax
        )
    }
}

/// The Format tab's categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum FormatKind {
    None,
    Number,
    Percent,
    Date,
    Time,
    Special,
    Custom,
}

impl FormatKind {
    pub(in crate::shell) const ALL: [FormatKind; 7] = [
        FormatKind::None,
        FormatKind::Number,
        FormatKind::Percent,
        FormatKind::Date,
        FormatKind::Time,
        FormatKind::Special,
        FormatKind::Custom,
    ];

    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Number => "Number",
            Self::Percent => "Percentage",
            Self::Date => "Date",
            Self::Time => "Time",
            Self::Special => "Special",
            Self::Custom => "Custom",
        }
    }
}

/// The Validate and Calculate tabs' choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum RuleKind {
    None,
    /// A range to validate, or a simple calculation.
    Simple,
    Custom,
}

/// A yes-or-no control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Flag {
    Hidden,
    ReadOnly,
    Required,
    Multiline,
    Password,
    Comb,
    OnByDefault,
    NoToggleToOff,
    Editable,
    MultiSelect,
    CurrencyFirst,
}

/// What a control that is not typed into does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum FieldAction {
    Tab(Tab),
    Toggle(Flag),
    NextBorder,
    NextFill,
    NextFontSize,
    NextTextColor,
    NextAlign,
    AddOption,
    RemoveOption,
    OptionUp,
    OptionDown,
    SelectOption(usize),
    DefaultOption,
    SetFormat(FormatKind),
    NextDecimals,
    NextSeparator,
    NextNegative,
    NextTimeStyle,
    NextSpecial,
    SetValidate(RuleKind),
    SetCalculate(RuleKind),
    NextOp,
    Submit,
    Delete,
}

pub(in crate::shell) const BORDERS: [(&str, Option<[f64; 3]>); 5] = [
    ("None", None),
    ("Black", Some([0.0, 0.0, 0.0])),
    ("Gray", Some([0.5, 0.5, 0.5])),
    ("Blue", Some([0.0, 0.0, 1.0])),
    ("Red", Some([1.0, 0.0, 0.0])),
];

pub(in crate::shell) const FILLS: [(&str, Option<[f64; 3]>); 5] = [
    ("None", None),
    ("White", Some([1.0, 1.0, 1.0])),
    ("Light Gray", Some([0.9, 0.9, 0.9])),
    ("Light Yellow", Some([1.0, 1.0, 0.8])),
    ("Light Blue", Some([0.85, 0.9, 1.0])),
];

pub(in crate::shell) const TEXT_COLORS: [(&str, [f64; 3]); 5] = [
    ("Black", [0.0, 0.0, 0.0]),
    ("Blue", [0.0, 0.0, 1.0]),
    ("Red", [1.0, 0.0, 0.0]),
    ("Green", [0.0, 0.5, 0.0]),
    ("Gray", [0.5, 0.5, 0.5]),
];

/// Font sizes offered; 0 is Auto.
pub(in crate::shell) const FONT_SIZES: [f64; 9] =
    [0.0, 6.0, 8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 18.0];

pub(in crate::shell) const SEPARATORS: [&str; 5] =
    ["1,234.56", "1234.56", "1.234,56", "1234,56", "1'234.56"];
pub(in crate::shell) const NEGATIVES: [&str; 4] = [
    "-1,234.01",
    "1,234.01 in red",
    "(1,234.01)",
    "(1,234.01) in red",
];
pub(in crate::shell) const TIME_STYLES: [&str; 4] = ["HH:MM", "h:MM tt", "HH:MM:ss", "h:MM:ss tt"];
pub(in crate::shell) const SPECIALS: [&str; 4] = [
    "Zip Code",
    "Zip Code + 4",
    "Phone Number",
    "Social Security Number",
];
pub(in crate::shell) const ALIGNS: [&str; 3] = ["Left", "Center", "Right"];

/// What kind of field the dialog is about, for which tabs it shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::shell) enum Shape {
    Text,
    CheckBox,
    Radio,
    ListBox,
    Dropdown,
    Button,
    Signature,
}

impl Shape {
    pub(in crate::shell) fn label(self) -> &'static str {
        match self {
            Shape::Text => "Text Field",
            Shape::CheckBox => "Check Box",
            Shape::Radio => "Radio Button",
            Shape::ListBox => "List Box",
            Shape::Dropdown => "Dropdown",
            Shape::Button => "Button",
            Shape::Signature => "Signature Field",
        }
    }

    /// The dialog's title, as Acrobat titles it.
    pub(in crate::shell) fn title(self) -> &'static str {
        match self {
            Shape::Text => "Text Field Properties",
            Shape::CheckBox => "Check Box Properties",
            Shape::Radio => "Radio Button Properties",
            Shape::ListBox => "List Box Properties",
            Shape::Dropdown => "Dropdown Properties",
            Shape::Button => "Button Properties",
            Shape::Signature => "Digital Signature Properties",
        }
    }

    pub(in crate::shell) fn tabs(self) -> Vec<Tab> {
        let mut tabs = vec![Tab::General, Tab::Appearance, Tab::Position];
        if self != Shape::Signature {
            tabs.push(Tab::Options);
        }
        if matches!(self, Shape::Text | Shape::Dropdown) {
            tabs.extend([Tab::Format, Tab::Validate, Tab::Calculate]);
        }
        tabs
    }
}

/// The dialog's choices, apart from what is typed.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) struct FieldForm {
    pub(in crate::shell) shape: Shape,
    pub(in crate::shell) tab: Tab,
    /// The field as it was, and the non-typed choices made since.
    pub(in crate::shell) properties: FieldProperties,
    /// A choice's items.
    pub(in crate::shell) items: Vec<ChoiceOption>,
    pub(in crate::shell) selected_item: Option<usize>,
    pub(in crate::shell) default_item: Option<String>,
    pub(in crate::shell) format: FormatKind,
    pub(in crate::shell) decimals: u8,
    pub(in crate::shell) separator: u8,
    pub(in crate::shell) negative: u8,
    pub(in crate::shell) currency_first: bool,
    pub(in crate::shell) time_style: u8,
    pub(in crate::shell) special: u8,
    pub(in crate::shell) validate: RuleKind,
    pub(in crate::shell) calculate: RuleKind,
    pub(in crate::shell) op: Op,
}

/// What each typed field starts with.
pub(in crate::shell) type Typed = Vec<(FieldInput, String)>;

fn number(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

impl FieldForm {
    /// The dialog for a field of `shape` with `properties`, and what its
    /// typed fields start with.
    pub(in crate::shell) fn of(shape: Shape, properties: FieldProperties) -> (FieldForm, Typed) {
        let scripts = &properties.scripts;
        let format = Format::of(scripts.keystroke.as_deref(), scripts.format.as_deref());
        let validate = Validate::of(scripts.validate.as_deref());
        let calculate = Calculate::of(scripts.calculate.as_deref());
        let [x0, y0, x1, y1] = properties.rect;
        let mut typed: Typed = vec![
            (FieldInput::Name, properties.name.clone()),
            (FieldInput::Tooltip, properties.tooltip.clone()),
            (FieldInput::Left, number(x0)),
            (FieldInput::Bottom, number(y0)),
            (FieldInput::Width, number(x1 - x0)),
            (FieldInput::Height, number(y1 - y0)),
            (FieldInput::DatePattern, "mm/dd/yyyy".to_owned()),
        ];
        let (items, default_item) = match &properties.options {
            KindOptions::Text {
                default, max_len, ..
            } => {
                typed.push((FieldInput::DefaultText, default.clone()));
                typed.push((
                    FieldInput::MaxLength,
                    max_len.map(|most| most.to_string()).unwrap_or_default(),
                ));
                (Vec::new(), None)
            }
            KindOptions::Button { export, .. } => {
                typed.push((FieldInput::Export, export.clone()));
                (Vec::new(), None)
            }
            KindOptions::Choice {
                options, default, ..
            } => (options.clone(), default.clone()),
            KindOptions::PushButton { caption } => {
                typed.push((FieldInput::Caption, caption.clone()));
                (Vec::new(), None)
            }
            KindOptions::Signature => (Vec::new(), None),
        };
        let mut form = FieldForm {
            shape,
            tab: Tab::General,
            properties,
            items,
            selected_item: None,
            default_item,
            format: FormatKind::None,
            decimals: 2,
            separator: 0,
            negative: 0,
            currency_first: true,
            time_style: 0,
            special: 0,
            validate: RuleKind::None,
            calculate: RuleKind::None,
            op: Op::Sum,
        };
        form.format = match format {
            Format::None => FormatKind::None,
            Format::Number {
                decimals,
                separator,
                negative,
                currency,
                prepend,
            } => {
                (form.decimals, form.separator, form.negative) = (decimals, separator, negative);
                form.currency_first = prepend;
                typed.push((FieldInput::Currency, currency));
                FormatKind::Number
            }
            Format::Percent {
                decimals,
                separator,
            } => {
                (form.decimals, form.separator) = (decimals, separator);
                FormatKind::Percent
            }
            Format::Date(pattern) => {
                typed.retain(|(input, _)| *input != FieldInput::DatePattern);
                typed.push((FieldInput::DatePattern, pattern));
                FormatKind::Date
            }
            Format::Time(style) => {
                form.time_style = style;
                FormatKind::Time
            }
            Format::Special(kind) => {
                form.special = kind;
                FormatKind::Special
            }
            Format::Custom { keystroke, format } => {
                typed.push((FieldInput::CustomKeystroke, keystroke.unwrap_or_default()));
                typed.push((FieldInput::CustomFormat, format.unwrap_or_default()));
                FormatKind::Custom
            }
        };
        form.validate = match validate {
            Validate::None => RuleKind::None,
            Validate::Range { min, max } => {
                typed.push((FieldInput::RangeMin, min.map(number).unwrap_or_default()));
                typed.push((FieldInput::RangeMax, max.map(number).unwrap_or_default()));
                RuleKind::Simple
            }
            Validate::Custom(script) => {
                typed.push((FieldInput::CustomValidate, script));
                RuleKind::Custom
            }
        };
        form.calculate = match calculate {
            Calculate::None => RuleKind::None,
            Calculate::Simple { op, fields } => {
                form.op = op;
                typed.push((FieldInput::CalculateFields, fields.join(", ")));
                RuleKind::Simple
            }
            Calculate::Custom(script) => {
                typed.push((FieldInput::CustomCalculate, script));
                RuleKind::Custom
            }
        };
        (form, typed)
    }

    /// The typed fields on the tab shown.
    pub(in crate::shell) fn inputs(&self) -> Vec<FieldInput> {
        use FieldInput as I;
        match self.tab {
            Tab::General => vec![I::Name, I::Tooltip],
            Tab::Appearance => Vec::new(),
            Tab::Position => vec![I::Left, I::Bottom, I::Width, I::Height],
            Tab::Options => match self.shape {
                Shape::Text => vec![I::DefaultText, I::MaxLength],
                Shape::CheckBox | Shape::Radio => vec![I::Export],
                Shape::ListBox | Shape::Dropdown => vec![I::OptionItem, I::OptionExport],
                Shape::Button => vec![I::Caption],
                Shape::Signature => Vec::new(),
            },
            Tab::Format => match self.format {
                FormatKind::Number => vec![I::Currency],
                FormatKind::Date => vec![I::DatePattern],
                FormatKind::Custom => vec![I::CustomKeystroke, I::CustomFormat],
                _ => Vec::new(),
            },
            Tab::Validate => match self.validate {
                RuleKind::None => Vec::new(),
                RuleKind::Simple => vec![I::RangeMin, I::RangeMax],
                RuleKind::Custom => vec![I::CustomValidate],
            },
            Tab::Calculate => match self.calculate {
                RuleKind::None => Vec::new(),
                RuleKind::Simple => vec![I::CalculateFields],
                RuleKind::Custom => vec![I::CustomCalculate],
            },
        }
    }

    pub(in crate::shell) fn flag(&self, flag: Flag) -> bool {
        let properties = &self.properties;
        match (flag, &properties.options) {
            (Flag::Hidden, _) => properties.hidden,
            (Flag::ReadOnly, _) => properties.read_only,
            (Flag::Required, _) => properties.required,
            (Flag::Multiline, KindOptions::Text { multiline, .. }) => *multiline,
            (Flag::Password, KindOptions::Text { password, .. }) => *password,
            (Flag::Comb, KindOptions::Text { comb, .. }) => *comb,
            (Flag::OnByDefault, KindOptions::Button { on_by_default, .. }) => *on_by_default,
            (
                Flag::NoToggleToOff,
                KindOptions::Button {
                    no_toggle_to_off, ..
                },
            ) => no_toggle_to_off.unwrap_or(false),
            (Flag::Editable, KindOptions::Choice { editable, .. }) => *editable,
            (Flag::MultiSelect, KindOptions::Choice { multi_select, .. }) => *multi_select,
            (Flag::CurrencyFirst, _) => self.currency_first,
            _ => false,
        }
    }

    fn toggle(&mut self, flag: Flag) {
        let properties = &mut self.properties;
        match (flag, &mut properties.options) {
            (Flag::Hidden, _) => properties.hidden = !properties.hidden,
            (Flag::ReadOnly, _) => properties.read_only = !properties.read_only,
            (Flag::Required, _) => properties.required = !properties.required,
            (Flag::Multiline, KindOptions::Text { multiline, .. }) => *multiline = !*multiline,
            (Flag::Password, KindOptions::Text { password, .. }) => *password = !*password,
            (Flag::Comb, KindOptions::Text { comb, .. }) => *comb = !*comb,
            (Flag::OnByDefault, KindOptions::Button { on_by_default, .. }) => {
                *on_by_default = !*on_by_default;
            }
            (
                Flag::NoToggleToOff,
                KindOptions::Button {
                    no_toggle_to_off: Some(hold),
                    ..
                },
            ) => *hold = !*hold,
            (Flag::Editable, KindOptions::Choice { editable, .. }) => *editable = !*editable,
            (Flag::MultiSelect, KindOptions::Choice { multi_select, .. }) => {
                *multi_select = !*multi_select;
            }
            (Flag::CurrencyFirst, _) => self.currency_first = !self.currency_first,
            _ => {}
        }
    }

    /// Run a control. `typed` reads a typed field, for Add.
    pub(in crate::shell) fn apply(
        &mut self,
        action: FieldAction,
        typed: &dyn Fn(FieldInput) -> String,
    ) {
        let properties = &mut self.properties;
        match action {
            FieldAction::Tab(tab) => self.tab = tab,
            FieldAction::Toggle(flag) => self.toggle(flag),
            FieldAction::NextBorder => properties.border = next_of(&BORDERS, properties.border),
            FieldAction::NextFill => properties.fill = next_of(&FILLS, properties.fill),
            FieldAction::NextTextColor => {
                properties.text_color = next_of(&TEXT_COLORS, properties.text_color);
            }
            FieldAction::NextFontSize => {
                let at = FONT_SIZES
                    .iter()
                    .position(|size| *size == properties.font_size);
                properties.font_size = FONT_SIZES[at.map_or(0, |at| (at + 1) % FONT_SIZES.len())];
            }
            FieldAction::NextAlign => {
                if let KindOptions::Text { align, .. } = &mut properties.options {
                    *align = (*align + 1) % 3;
                }
            }
            FieldAction::AddOption => {
                let item = typed(FieldInput::OptionItem).trim().to_owned();
                if !item.is_empty() {
                    let export = typed(FieldInput::OptionExport).trim().to_owned();
                    self.items.push(ChoiceOption {
                        export: if export.is_empty() {
                            item.clone()
                        } else {
                            export
                        },
                        display: item,
                    });
                    self.selected_item = Some(self.items.len() - 1);
                }
            }
            FieldAction::RemoveOption => {
                if let Some(at) = self.selected_item.filter(|at| *at < self.items.len()) {
                    let removed = self.items.remove(at);
                    if self.default_item.as_ref() == Some(&removed.export) {
                        self.default_item = None;
                    }
                    self.selected_item = None;
                }
            }
            FieldAction::OptionUp | FieldAction::OptionDown => {
                if let Some(at) = self.selected_item {
                    let to = if action == FieldAction::OptionUp {
                        at.checked_sub(1)
                    } else {
                        Some(at + 1).filter(|to| *to < self.items.len())
                    };
                    if let Some(to) = to {
                        self.items.swap(at, to);
                        self.selected_item = Some(to);
                    }
                }
            }
            FieldAction::SelectOption(at) => {
                self.selected_item = (at < self.items.len()).then_some(at);
            }
            FieldAction::DefaultOption => {
                let chosen = self.selected_item.and_then(|at| self.items.get(at));
                let export = chosen.map(|option| option.export.clone());
                self.default_item = if self.default_item == export {
                    None
                } else {
                    export
                };
            }
            FieldAction::SetFormat(kind) => self.format = kind,
            FieldAction::NextDecimals => self.decimals = (self.decimals + 1) % 5,
            FieldAction::NextSeparator => {
                self.separator = (self.separator + 1) % SEPARATORS.len() as u8;
            }
            FieldAction::NextNegative => {
                self.negative = (self.negative + 1) % NEGATIVES.len() as u8;
            }
            FieldAction::NextTimeStyle => {
                self.time_style = (self.time_style + 1) % TIME_STYLES.len() as u8;
            }
            FieldAction::NextSpecial => self.special = (self.special + 1) % SPECIALS.len() as u8,
            FieldAction::SetValidate(kind) => self.validate = kind,
            FieldAction::SetCalculate(kind) => self.calculate = kind,
            FieldAction::NextOp => {
                let at = Op::ALL.iter().position(|op| *op == self.op).unwrap_or(0);
                self.op = Op::ALL[(at + 1) % Op::ALL.len()];
            }
            FieldAction::Submit | FieldAction::Delete => {}
        }
    }

    /// The properties the dialog comes to, with `typed` read for what was
    /// typed, or what is wrong with them.
    pub(in crate::shell) fn request(
        &self,
        typed: &dyn Fn(FieldInput) -> String,
    ) -> Result<FieldProperties, String> {
        let mut properties = self.properties.clone();
        properties.name = typed(FieldInput::Name).trim().to_owned();
        properties.tooltip = typed(FieldInput::Tooltip).trim().to_owned();
        let measure = |input: FieldInput| -> Result<f64, String> {
            let text = typed(input);
            text.trim()
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .ok_or_else(|| format!("{} must be a number, not {:?}", input.label(), text.trim()))
        };
        let (left, bottom) = (measure(FieldInput::Left)?, measure(FieldInput::Bottom)?);
        let (width, height) = (measure(FieldInput::Width)?, measure(FieldInput::Height)?);
        properties.rect = [left, bottom, left + width, bottom + height];
        match &mut properties.options {
            KindOptions::Text {
                default, max_len, ..
            } => {
                *default = typed(FieldInput::DefaultText);
                let limit = typed(FieldInput::MaxLength);
                *max_len = match limit.trim() {
                    "" => None,
                    text => Some(
                        text.parse::<usize>()
                            .ok()
                            .filter(|most| *most > 0)
                            .ok_or_else(|| {
                                format!(
                                    "The limit of characters must be a whole number, not {text:?}"
                                )
                            })?,
                    ),
                };
            }
            KindOptions::Button { export, .. } => {
                *export = typed(FieldInput::Export).trim().to_owned()
            }
            KindOptions::Choice {
                options, default, ..
            } => {
                options.clone_from(&self.items);
                default.clone_from(&self.default_item);
            }
            KindOptions::PushButton { caption } => *caption = typed(FieldInput::Caption),
            KindOptions::Signature => {}
        }
        if matches!(self.shape, Shape::Text | Shape::Dropdown) {
            properties.scripts = self.scripts(typed)?;
        }
        Ok(properties)
    }

    fn scripts(&self, typed: &dyn Fn(FieldInput) -> String) -> Result<FieldScripts, String> {
        let optional = |input: FieldInput| {
            let text = typed(input);
            (!text.trim().is_empty()).then(|| text.trim().to_owned())
        };
        let format = match self.format {
            FormatKind::None => Format::None,
            FormatKind::Number => Format::Number {
                decimals: self.decimals,
                separator: self.separator,
                negative: self.negative,
                currency: typed(FieldInput::Currency),
                prepend: self.currency_first,
            },
            FormatKind::Percent => Format::Percent {
                decimals: self.decimals,
                separator: self.separator,
            },
            FormatKind::Date => Format::Date(
                optional(FieldInput::DatePattern).ok_or("Type the date format, as mm/dd/yyyy.")?,
            ),
            FormatKind::Time => Format::Time(self.time_style),
            FormatKind::Special => Format::Special(self.special),
            FormatKind::Custom => Format::Custom {
                keystroke: optional(FieldInput::CustomKeystroke),
                format: optional(FieldInput::CustomFormat),
            },
        };
        let bound = |input: FieldInput| -> Result<Option<f64>, String> {
            match optional(input) {
                None => Ok(None),
                Some(text) => text
                    .parse::<f64>()
                    .map(Some)
                    .map_err(|_| format!("{} must be a number, not {text:?}", input.label())),
            }
        };
        let validate = match self.validate {
            RuleKind::None => Validate::None,
            RuleKind::Simple => Validate::Range {
                min: bound(FieldInput::RangeMin)?,
                max: bound(FieldInput::RangeMax)?,
            },
            RuleKind::Custom => {
                optional(FieldInput::CustomValidate).map_or(Validate::None, Validate::Custom)
            }
        };
        let calculate = match self.calculate {
            RuleKind::None => Calculate::None,
            RuleKind::Simple => {
                let fields: Vec<String> = typed(FieldInput::CalculateFields)
                    .split(',')
                    .map(|name| name.trim().to_owned())
                    .filter(|name| !name.is_empty())
                    .collect();
                if fields.is_empty() {
                    return Err("Name the fields to calculate from.".to_owned());
                }
                Calculate::Simple {
                    op: self.op,
                    fields,
                }
            }
            RuleKind::Custom => {
                optional(FieldInput::CustomCalculate).map_or(Calculate::None, Calculate::Custom)
            }
        };
        let (keystroke, format_script) = format.scripts();
        Ok(FieldScripts {
            keystroke,
            format: format_script,
            validate: validate.script(),
            calculate: calculate.script(),
        })
    }
}

/// The entry after `current` in `all`, going round; the first when
/// `current` is not one of them.
fn next_of<T: Copy + PartialEq>(all: &[(&str, T)], current: T) -> T {
    let at = all.iter().position(|(_, each)| *each == current);
    all[at.map_or(0, |at| (at + 1) % all.len())].1
}

/// The name of `current` in `all`, or Custom.
pub(in crate::shell) fn name_of<T: PartialEq>(
    all: &[(&'static str, T)],
    current: &T,
) -> &'static str {
    all.iter()
        .find(|(_, each)| each == current)
        .map_or("Custom", |(name, _)| name)
}
