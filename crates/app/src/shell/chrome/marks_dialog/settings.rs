//! What the dialog keeps with a mark so Update opens on it: its choices and
//! every field, one `key=value` line each.
//!
//! Only this dialog writes and reads it. A key it does not know, or a value
//! it cannot read, is skipped, so a mark made by a later version still
//! opens, on what this one understands.

use std::collections::BTreeMap;
use std::path::PathBuf;

use onionskin_tools_edit::marks::{Font, HAlign, VAlign};

use super::{MarkField, MarkForm, Source, COLORS};

/// The form's choices and what is typed, as lines.
pub(in crate::shell) fn save(form: &MarkForm, typed: &BTreeMap<MarkField, String>) -> String {
    let mut lines = vec![
        format!("font={}", form.font.base_font()),
        format!("color={}", form.color),
        format!("source={}", source_key(form.source)),
        format!("horizontal={}", horizontal_key(form.horizontal)),
        format!("vertical={}", vertical_key(form.vertical)),
        format!("behind={}", form.behind),
        format!("position={}", form.position),
        format!("numbers={}", form.numbers_in_names),
    ];
    if let Some(file) = &form.file {
        lines.push(format!("file={}", escape(&file.to_string_lossy())));
    }
    for (field, text) in typed {
        lines.push(format!("{}={}", field.id(), escape(text)));
    }
    lines.join("\n")
}

/// Read `saved` back into `form` and `typed`.
pub(in crate::shell) fn restore(
    saved: &str,
    form: &mut MarkForm,
    typed: &mut BTreeMap<MarkField, String>,
) {
    for line in saved.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = unescape(value);
        match key {
            "font" => {
                if let Some(font) = Font::ALL.into_iter().find(|font| font.base_font() == value) {
                    form.font = font;
                }
            }
            "color" => set(
                &mut form.color,
                value.parse().ok().filter(|at| *at < COLORS.len()),
            ),
            "source" => set(&mut form.source, parse_source(&value)),
            "horizontal" => set(&mut form.horizontal, parse_horizontal(&value)),
            "vertical" => set(&mut form.vertical, parse_vertical(&value)),
            "behind" => set(&mut form.behind, value.parse().ok()),
            "position" => set(&mut form.position, value.parse().ok().filter(|at| *at < 6)),
            "numbers" => set(&mut form.numbers_in_names, value.parse().ok()),
            "file" => form.file = Some(PathBuf::from(value)),
            id => {
                if let Some(field) = MarkField::ALL.into_iter().find(|field| field.id() == id) {
                    typed.insert(field, value);
                }
            }
        }
    }
}

fn set<T>(slot: &mut T, value: Option<T>) {
    if let Some(value) = value {
        *slot = value;
    }
}

fn source_key(source: Source) -> &'static str {
    match source {
        Source::Text => "text",
        Source::Color => "color",
        Source::File => "file",
    }
}

fn parse_source(value: &str) -> Option<Source> {
    [Source::Text, Source::Color, Source::File]
        .into_iter()
        .find(|source| source_key(*source) == value)
}

fn horizontal_key(align: HAlign) -> &'static str {
    match align {
        HAlign::Left => "left",
        HAlign::Center => "center",
        HAlign::Right => "right",
    }
}

fn parse_horizontal(value: &str) -> Option<HAlign> {
    [HAlign::Left, HAlign::Center, HAlign::Right]
        .into_iter()
        .find(|align| horizontal_key(*align) == value)
}

fn vertical_key(align: VAlign) -> &'static str {
    match align {
        VAlign::Top => "top",
        VAlign::Center => "center",
        VAlign::Bottom => "bottom",
    }
}

fn parse_vertical(value: &str) -> Option<VAlign> {
    [VAlign::Top, VAlign::Center, VAlign::Bottom]
        .into_iter()
        .find(|align| vertical_key(*align) == value)
}

/// A value on one line: backslashes and line breaks escaped.
fn escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\n', "\\n")
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use onionskin_core::pages::MarkKind;

    #[test]
    fn what_is_saved_is_what_is_restored() {
        let mut form = MarkForm::new(MarkKind::Watermark, false);
        form.font = Font::TimesBold;
        form.color = 4;
        form.source = Source::File;
        form.file = Some("/art/a=b.pdf".into());
        form.horizontal = HAlign::Right;
        form.vertical = VAlign::Bottom;
        form.behind = true;
        form.position = 2;
        form.numbers_in_names = false;
        let typed: BTreeMap<MarkField, String> = [
            (MarkField::Text, "Two\\nlines\nhere".to_owned()),
            (MarkField::Opacity, "30".to_owned()),
        ]
        .into_iter()
        .collect();

        let saved = save(&form, &typed);
        let mut restored = MarkForm::new(MarkKind::Watermark, true);
        let mut fields = BTreeMap::new();
        restore(&saved, &mut restored, &mut fields);
        assert_eq!(
            MarkForm {
                existing: false,
                ..restored
            },
            form
        );
        assert_eq!(fields, typed);
    }

    #[test]
    fn what_it_cannot_read_leaves_the_form_alone() {
        let mut form = MarkForm::new(MarkKind::HeaderFooter, false);
        let before = form.clone();
        let mut typed = BTreeMap::new();
        restore(
            "font=Comic\ncolor=99\nsource=smoke\nhorizontal=up\nvertical=up\nbehind=maybe\n\
             position=9\nnumbers=perhaps\nno equals sign\nunknown=1\ntrailing=\\",
            &mut form,
            &mut typed,
        );
        assert_eq!(form, before);
        assert!(typed.is_empty());
        assert_eq!(unescape("end\\"), "end\\");
    }
}
