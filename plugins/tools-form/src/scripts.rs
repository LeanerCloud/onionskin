//! What Acrobat's Format, Validate and Calculate tabs write, and reading it
//! back.
//!
//! Each tab's choice becomes the `AF` call Acrobat writes for it, so a form
//! prepared here computes in Acrobat as it does here. A script that is not
//! exactly one of those calls reads back as a custom script, kept as it is.

/// The Format tab.
#[derive(Debug, Clone, PartialEq)]
pub enum Format {
    None,
    Number {
        decimals: u8,
        /// Acrobat's separator style: 0 `1,234.56` to 4 `1'234.56`.
        separator: u8,
        /// 0 minus, 1 red, 2 parentheses, 3 red parentheses.
        negative: u8,
        currency: String,
        /// The currency symbol before the number rather than after.
        prepend: bool,
    },
    Percent {
        decimals: u8,
        separator: u8,
    },
    /// A date in this format, `mm/dd/yyyy`.
    Date(String),
    /// Acrobat's time styles: 0 `HH:MM`, 1 `h:MM tt`, 2 `HH:MM:ss`, 3
    /// `h:MM:ss tt`.
    Time(u8),
    /// 0 zip code, 1 zip+4, 2 phone number, 3 social security number.
    Special(u8),
    Custom {
        keystroke: Option<String>,
        format: Option<String>,
    },
}

/// The Validate tab.
#[derive(Debug, Clone, PartialEq)]
pub enum Validate {
    None,
    Range { min: Option<f64>, max: Option<f64> },
    Custom(String),
}

/// The Calculate tab.
#[derive(Debug, Clone, PartialEq)]
pub enum Calculate {
    None,
    Simple { op: Op, fields: Vec<String> },
    Custom(String),
}

/// What a simple calculation does with its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Sum,
    Product,
    Average,
    Minimum,
    Maximum,
}

impl Op {
    pub const ALL: [Op; 5] = [Op::Sum, Op::Product, Op::Average, Op::Minimum, Op::Maximum];

    fn code(self) -> &'static str {
        match self {
            Op::Sum => "SUM",
            Op::Product => "PRD",
            Op::Average => "AVG",
            Op::Minimum => "MIN",
            Op::Maximum => "MAX",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Op::Sum => "sum (+)",
            Op::Product => "product (x)",
            Op::Average => "average",
            Op::Minimum => "minimum",
            Op::Maximum => "maximum",
        }
    }
}

/// One argument of an `AF` call.
#[derive(Debug, Clone, PartialEq)]
enum Arg {
    Number(f64),
    Text(String),
    Bool(bool),
    List(Vec<String>),
}

/// `name(args);` read into its name and arguments, when a script is
/// exactly one call.
fn call(script: &str) -> Option<(String, Vec<Arg>)> {
    let script = script.trim().trim_end_matches(';').trim();
    let open = script.find('(')?;
    let name = script[..open].trim();
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || name.is_empty() {
        return None;
    }
    let inner = script[open + 1..].strip_suffix(')')?;
    let mut args = Vec::new();
    for piece in split_top(inner)? {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        args.push(arg(piece)?);
    }
    Some((name.to_owned(), args))
}

/// `text` split at commas outside quotes and brackets.
fn split_top(text: &str) -> Option<Vec<&str>> {
    let mut pieces = Vec::new();
    let (mut depth, mut quoted, mut escaped, mut start) = (0i32, false, false, 0);
    for (at, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '(' | '[' if !quoted => depth += 1,
            ')' | ']' if !quoted => depth -= 1,
            ',' if !quoted && depth == 0 => {
                pieces.push(&text[start..at]);
                start = at + 1;
            }
            _ => {}
        }
    }
    (depth == 0 && !quoted).then(|| {
        pieces.push(&text[start..]);
        pieces
    })
}

fn arg(piece: &str) -> Option<Arg> {
    if let Some(text) = piece
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
    {
        return Some(Arg::Text(unescaped(text)));
    }
    match piece {
        "true" => return Some(Arg::Bool(true)),
        "false" => return Some(Arg::Bool(false)),
        _ => {}
    }
    if let Ok(number) = piece.parse::<f64>() {
        return Some(Arg::Number(number));
    }
    let items = piece
        .strip_prefix("new Array")
        .map(str::trim)
        .and_then(|rest| rest.strip_prefix('(')?.strip_suffix(')'))
        .or_else(|| piece.strip_prefix('[')?.strip_suffix(']'))?;
    let names = split_top(items)?
        .into_iter()
        .map(|item| match arg(item.trim())? {
            Arg::Text(name) => Some(name),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Arg::List(names))
}

/// A JavaScript string's text: `\"` and `\\` as the characters.
fn unescaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        match character {
            '\\' => out.extend(characters.next()),
            other => out.push(other),
        }
    }
    out
}

fn small(arg: Option<&Arg>) -> Option<u8> {
    match arg? {
        Arg::Number(number) if number.fract() == 0.0 && (0.0..=255.0).contains(number) => {
            Some(*number as u8)
        }
        _ => None,
    }
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

impl Format {
    /// The Format tab's choice for a field's keystroke and format scripts.
    pub fn of(keystroke: Option<&str>, format: Option<&str>) -> Format {
        let custom = || Format::Custom {
            keystroke: keystroke.map(str::to_owned),
            format: format.map(str::to_owned),
        };
        let (Some(keystroke_script), Some(format_script)) = (keystroke, format) else {
            return if keystroke.is_none() && format.is_none() {
                Format::None
            } else {
                custom()
            };
        };
        let parsed = Self::parse(format_script);
        match parsed {
            Some(chosen)
                if chosen.scripts().0.as_deref().and_then(call) == call(keystroke_script) =>
            {
                chosen
            }
            _ => custom(),
        }
    }

    fn parse(format: &str) -> Option<Format> {
        let (name, args) = call(format)?;
        Some(match name.as_str() {
            "AFNumber_Format" => Format::Number {
                decimals: small(args.first())?,
                separator: small(args.get(1))?,
                negative: small(args.get(2))?,
                currency: match args.get(4)? {
                    Arg::Text(text) => text.clone(),
                    _ => return None,
                },
                prepend: matches!(args.get(5)?, Arg::Bool(true)),
            },
            "AFPercent_Format" if args.len() == 2 => Format::Percent {
                decimals: small(args.first())?,
                separator: small(args.get(1))?,
            },
            "AFDate_FormatEx" => match args.first()? {
                Arg::Text(text) => Format::Date(text.clone()),
                _ => return None,
            },
            "AFTime_Format" => Format::Time(small(args.first())?.min(3)),
            "AFSpecial_Format" => Format::Special(small(args.first())?.min(3)),
            _ => return None,
        })
    }

    /// The keystroke and format scripts the choice writes.
    pub fn scripts(&self) -> (Option<String>, Option<String>) {
        let pair = |keystroke: String, format: String| (Some(keystroke), Some(format));
        match self {
            Format::None => (None, None),
            Format::Number {
                decimals,
                separator,
                negative,
                currency,
                prepend,
            } => {
                let args = format!(
                    "{decimals}, {separator}, {negative}, 0, {}, {prepend}",
                    quoted(currency)
                );
                pair(
                    format!("AFNumber_Keystroke({args});"),
                    format!("AFNumber_Format({args});"),
                )
            }
            Format::Percent {
                decimals,
                separator,
            } => pair(
                format!("AFPercent_Keystroke({decimals}, {separator});"),
                format!("AFPercent_Format({decimals}, {separator});"),
            ),
            Format::Date(pattern) => pair(
                format!("AFDate_KeystrokeEx({});", quoted(pattern)),
                format!("AFDate_FormatEx({});", quoted(pattern)),
            ),
            Format::Time(style) => pair(
                format!("AFTime_Keystroke({style});"),
                format!("AFTime_Format({style});"),
            ),
            Format::Special(kind) => pair(
                format!("AFSpecial_Keystroke({kind});"),
                format!("AFSpecial_Format({kind});"),
            ),
            Format::Custom { keystroke, format } => (keystroke.clone(), format.clone()),
        }
    }
}

impl Validate {
    pub fn of(script: Option<&str>) -> Validate {
        let Some(script) = script else {
            return Validate::None;
        };
        let range = call(script).and_then(|(name, args)| {
            if name != "AFRange_Validate" || args.len() != 4 {
                return None;
            }
            let bound = |on: &Arg, value: &Arg| match (on, value) {
                (Arg::Bool(true), Arg::Number(value)) => Some(Some(*value)),
                (Arg::Bool(false), _) => Some(None),
                _ => None,
            };
            Some(Validate::Range {
                min: bound(&args[0], &args[1])?,
                max: bound(&args[2], &args[3])?,
            })
        });
        range.unwrap_or_else(|| Validate::Custom(script.to_owned()))
    }

    pub fn script(&self) -> Option<String> {
        match self {
            Validate::None => None,
            Validate::Range { min, max } => Some(format!(
                "AFRange_Validate({}, {}, {}, {});",
                min.is_some(),
                min.unwrap_or(0.0),
                max.is_some(),
                max.unwrap_or(0.0)
            )),
            Validate::Custom(script) => Some(script.clone()),
        }
    }
}

impl Calculate {
    pub fn of(script: Option<&str>) -> Calculate {
        let Some(script) = script else {
            return Calculate::None;
        };
        let simple = call(script).and_then(|(name, args)| {
            let [Arg::Text(code), fields] = args.as_slice() else {
                return None;
            };
            if name != "AFSimple_Calculate" {
                return None;
            }
            let op = Op::ALL.into_iter().find(|op| op.code() == code)?;
            let fields = match fields {
                Arg::List(names) => names.clone(),
                Arg::Text(names) => names
                    .split(',')
                    .map(|name| name.trim().to_owned())
                    .filter(|name| !name.is_empty())
                    .collect(),
                _ => return None,
            };
            Some(Calculate::Simple { op, fields })
        });
        simple.unwrap_or_else(|| Calculate::Custom(script.to_owned()))
    }

    pub fn script(&self) -> Option<String> {
        match self {
            Calculate::None => None,
            Calculate::Simple { op, fields } => Some(format!(
                "AFSimple_Calculate({}, new Array ({}));",
                quoted(op.code()),
                fields
                    .iter()
                    .map(|field| quoted(field))
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
            Calculate::Custom(script) => Some(script.clone()),
        }
    }
}
