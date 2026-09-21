use std::ops::Range;

use gpui::{
    actions, div, fill, point, px, relative, App, Bounds, ClipboardItem, Context, CursorStyle,
    Element, ElementId, ElementInputHandler, Entity, EntityInputHandler, FocusHandle, Focusable,
    GlobalElementId, InteractiveElement as _, IntoElement, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, ParentElement as _, Pixels, Point,
    ShapedLine, SharedString, Style, Styled as _, TextRun, UTF16Selection, UnderlineStyle, Window,
};
use onionskin_plugin_api::PluginRegistry;
use unicode_segmentation::UnicodeSegmentation as _;

use super::accessible::{Activation, TextField};
use super::theme::ThemeTokens;

actions!(
    onionskin_search,
    [
        SearchBackspace,
        SearchDelete,
        SearchLeft,
        SearchRight,
        SearchSelectLeft,
        SearchSelectRight,
        SearchSelectAll,
        SearchHome,
        SearchEnd,
        SearchPaste,
        SearchCut,
        SearchCopy,
    ]
);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) enum SearchResult {
    Tool {
        index: usize,
        id: &'static str,
        name: &'static str,
    },
    Command {
        index: usize,
        id: &'static str,
        title: &'static str,
    },
    DocumentSearch {
        query: String,
    },
    Unavailable {
        label: String,
        reason: &'static str,
    },
}

impl SearchResult {
    pub(super) fn label(&self) -> String {
        match self {
            Self::Tool { name, .. } => (*name).to_owned(),
            Self::Command { title, .. } => (*title).to_owned(),
            Self::DocumentSearch { query } => format!("Search document for \"{query}\""),
            Self::Unavailable { label, .. } => label.clone(),
        }
    }

    pub(super) fn detail(&self) -> String {
        match self {
            Self::Tool { id, .. } => format!("Tool - {id}"),
            Self::Command { id, .. } => format!("Command - {id}"),
            Self::DocumentSearch { .. } => "Document text".to_owned(),
            Self::Unavailable { reason, .. } => (*reason).to_owned(),
        }
    }
}

pub(super) fn search_registry(registry: &PluginRegistry, query: &str) -> Vec<SearchResult> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return Vec::new();
    }

    let mut results: Vec<_> = registry
        .tools()
        .enumerate()
        .filter(|(_, tool)| {
            tool.id().to_lowercase().contains(&query) || tool.name().to_lowercase().contains(&query)
        })
        .map(|(index, tool)| SearchResult::Tool {
            index,
            id: tool.id(),
            name: tool.name(),
        })
        .collect();
    results.extend(
        registry
            .commands()
            .iter()
            .enumerate()
            .filter(|(_, command)| {
                command.id.to_lowercase().contains(&query)
                    || command.title.to_lowercase().contains(&query)
            })
            .map(|(index, command)| SearchResult::Command {
                index,
                id: command.id,
                title: command.title,
            }),
    );
    results
}

pub(super) fn document_search_result(query: &str) -> Option<SearchResult> {
    let query = query.trim();
    (!query.is_empty()).then(|| SearchResult::DocumentSearch {
        query: query.to_owned(),
    })
}

/// Why a result cannot be chosen, or `None` when it can.
///
/// Tools and commands come from the registry; document search routes through
/// the find bar. Commands and document search need an active document, which is
/// a state the panel can report rather than a milestone it is waiting for.
pub(super) fn unavailable_selection(
    result: &SearchResult,
    has_document: bool,
) -> Option<SearchResult> {
    match result {
        SearchResult::DocumentSearch { query } if !has_document => {
            Some(SearchResult::Unavailable {
                label: format!("Search document for \"{query}\""),
                reason: NO_DOCUMENT,
            })
        }
        SearchResult::Tool { .. } | SearchResult::DocumentSearch { .. } => None,
        SearchResult::Command { title, .. } if !has_document => Some(SearchResult::Unavailable {
            label: (*title).to_owned(),
            reason: NO_DOCUMENT,
        }),
        SearchResult::Command { .. } => None,
        SearchResult::Unavailable { .. } => Some(result.clone()),
    }
}

const NO_DOCUMENT: &str = "No document is open";

pub(in crate::shell) fn install_keybindings(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", SearchBackspace, Some("OnionskinSearch")),
        KeyBinding::new("delete", SearchDelete, Some("OnionskinSearch")),
        KeyBinding::new("left", SearchLeft, Some("OnionskinSearch")),
        KeyBinding::new("right", SearchRight, Some("OnionskinSearch")),
        KeyBinding::new("shift-left", SearchSelectLeft, Some("OnionskinSearch")),
        KeyBinding::new("shift-right", SearchSelectRight, Some("OnionskinSearch")),
        KeyBinding::new("cmd-a", SearchSelectAll, Some("OnionskinSearch")),
        KeyBinding::new("cmd-v", SearchPaste, Some("OnionskinSearch")),
        KeyBinding::new("cmd-c", SearchCopy, Some("OnionskinSearch")),
        KeyBinding::new("cmd-x", SearchCut, Some("OnionskinSearch")),
        KeyBinding::new("home", SearchHome, Some("OnionskinSearch")),
        KeyBinding::new("end", SearchEnd, Some("OnionskinSearch")),
    ]);
}

#[derive(Debug, Default)]
struct SearchBuffer {
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
}

impl SearchBuffer {
    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize) {
        self.selected_range = offset..offset;
        self.selection_reversed = false;
    }

    fn select_to(&mut self, offset: usize) {
        let anchor = if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        };
        if offset < anchor {
            self.selected_range = offset..anchor;
            self.selection_reversed = true;
        } else {
            self.selected_range = anchor..offset;
            self.selection_reversed = false;
        }
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(index, _)| (index < offset).then_some(index))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(index, _)| (index > offset).then_some(index))
            .unwrap_or(self.content.len())
    }

    fn move_left(&mut self) {
        let offset = if self.selected_range.is_empty() {
            self.previous_boundary(self.cursor_offset())
        } else {
            self.selected_range.start
        };
        self.move_to(offset);
    }

    fn move_right(&mut self) {
        let offset = if self.selected_range.is_empty() {
            self.next_boundary(self.cursor_offset())
        } else {
            self.selected_range.end
        };
        self.move_to(offset);
    }

    fn select_left(&mut self) {
        self.select_to(self.previous_boundary(self.cursor_offset()));
    }

    fn select_right(&mut self) {
        self.select_to(self.next_boundary(self.cursor_offset()));
    }

    fn select_all(&mut self) {
        self.selected_range = 0..self.content.len();
        self.selection_reversed = false;
    }

    fn replace(&mut self, range: Range<usize>, text: &str) {
        self.content.replace_range(range.clone(), text);
        self.move_to(range.start + text.len());
    }

    fn backspace(&mut self) {
        if self.selected_range.is_empty() {
            let cursor = self.cursor_offset();
            self.selected_range = self.previous_boundary(cursor)..cursor;
        }
        self.replace(self.selected_range.clone(), "");
    }

    fn delete(&mut self) {
        if self.selected_range.is_empty() {
            let cursor = self.cursor_offset();
            self.selected_range = cursor..self.next_boundary(cursor);
        }
        self.replace(self.selected_range.clone(), "");
    }
}

pub(in crate::shell) struct SearchInput {
    element_id: &'static str,
    focus_handle: FocusHandle,
    buffer: SearchBuffer,
    placeholder: SharedString,
    marked_range: Option<Range<usize>>,
    last_layout: Option<ShapedLine>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    theme: ThemeTokens,
}

impl SearchInput {
    pub(super) fn new(theme: ThemeTokens, cx: &mut Context<Self>) -> Self {
        Self::with_placeholder("global-search-input", "Search tools or document", theme, cx)
    }

    pub(super) fn with_placeholder(
        element_id: &'static str,
        placeholder: impl Into<SharedString>,
        theme: ThemeTokens,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            element_id,
            focus_handle: cx.focus_handle(),
            buffer: SearchBuffer::default(),
            placeholder: placeholder.into(),
            marked_range: None,
            last_layout: None,
            last_bounds: None,
            is_selecting: false,
            theme,
        }
    }

    pub(super) fn set_theme(&mut self, theme: ThemeTokens, cx: &mut Context<Self>) {
        if self.theme == theme {
            return;
        }
        self.theme = theme;
        cx.notify();
    }

    pub(super) fn query(&self) -> &str {
        &self.buffer.content
    }

    /// The key the field's own node is published under, so a caller matching
    /// GPUI's focus against the tree names the field the same way the tree
    /// does.
    pub(in crate::shell) fn element_id(&self) -> &'static str {
        self.element_id
    }

    /// What the field tells a screen reader.
    ///
    /// The name comes from the caller, because a text field is drawn with no
    /// label of its own: the placeholder is all a sighted user gets, and it
    /// disappears the moment anything is typed.
    pub(super) fn accessible(
        &self,
        label: &'static str,
        field: TextField,
    ) -> super::accessible::Element {
        field_node(
            self.element_id,
            label,
            &self.buffer.content,
            &self.placeholder,
            field,
        )
    }

    /// What the field has selected, for the test that checks a window-wide
    /// binding does not take a keystroke away from a focused text field.
    /// That test needs a window, hence the feature.
    #[cfg(all(test, feature = "shell-test-support"))]
    pub(super) fn selected_range(&self) -> std::ops::Range<usize> {
        self.buffer.selected_range.clone()
    }

    pub(super) fn set_query(&mut self, query: impl Into<String>, cx: &mut Context<Self>) {
        let query = query.into();
        if self.buffer.content == query {
            return;
        }
        self.buffer.content = query;
        self.buffer.move_to(self.buffer.content.len());
        self.marked_range = None;
        cx.notify();
    }

    fn left(&mut self, _: &SearchLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_left();
        cx.notify();
    }

    fn right(&mut self, _: &SearchRight, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_right();
        cx.notify();
    }

    fn select_left(&mut self, _: &SearchSelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_left();
        cx.notify();
    }

    fn select_right(&mut self, _: &SearchSelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_right();
        cx.notify();
    }

    fn select_all(&mut self, _: &SearchSelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.select_all();
        cx.notify();
    }

    fn home(&mut self, _: &SearchHome, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_to(0);
        cx.notify();
    }

    fn end(&mut self, _: &SearchEnd, _: &mut Window, cx: &mut Context<Self>) {
        self.buffer.move_to(self.buffer.content.len());
        cx.notify();
    }

    fn backspace(&mut self, _: &SearchBackspace, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        self.buffer.backspace();
        cx.notify();
    }

    fn delete(&mut self, _: &SearchDelete, _: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        self.buffer.delete();
        cx.notify();
    }

    fn paste(&mut self, _: &SearchPaste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &text, window, cx);
        }
    }

    fn copy(&mut self, _: &SearchCopy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.buffer.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.buffer.content[self.buffer.selected_range.clone()].to_owned(),
            ));
        }
    }

    fn cut(&mut self, _: &SearchCut, window: &mut Window, cx: &mut Context<Self>) {
        self.copy(&SearchCopy, window, cx);
        if !self.buffer.selected_range.is_empty() {
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle);
        self.is_selecting = true;
        let offset = self.index_for_mouse_position(event.position);
        if event.modifiers.shift {
            self.buffer.select_to(offset);
        } else {
            self.buffer.move_to(offset);
        }
        cx.notify();
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            let offset = self.index_for_mouse_position(event.position);
            self.buffer.select_to(offset);
            cx.notify();
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.buffer.content.is_empty() {
            return 0;
        }
        let (Some(bounds), Some(line)) = (self.last_bounds.as_ref(), self.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.buffer.content.len();
        }
        line.closest_index_for_x(position.x - bounds.left())
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        utf8_offset(&self.buffer.content, offset)
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        utf16_offset(&self.buffer.content, offset)
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range.start)..self.offset_from_utf16(range.end)
    }
}

/// The node for a text field: its name, and what it holds.
///
/// Split from [`SearchInput::accessible`] so what a field says can be checked
/// without a window to build one in.
fn field_node(
    id: &'static str,
    label: &'static str,
    query: &str,
    placeholder: &str,
    field: TextField,
) -> super::accessible::Element {
    let role = match field {
        TextField::Page
        | TextField::ExportFirst
        | TextField::ExportLast
        | TextField::ExportDpi
        | TextField::ExportQuality
        | TextField::SplitValue => accesskit::Role::NumberInput,
        TextField::CombinePages => accesskit::Role::TextInput,
        TextField::Search | TextField::Find => accesskit::Role::SearchInput,
    };
    let mut node = super::accessible::Element::new(id, role, label)
        .with_value(query)
        .with_activation(Activation::Focus(field));
    if query.is_empty() {
        node = node.with_description(placeholder);
    }
    node
}

fn single_line(text: &str) -> String {
    text.chars()
        .map(|character| match character {
            '\n' | '\r' => ' ',
            other => other,
        })
        .collect()
}

fn utf8_offset(text: &str, utf16_offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for character in text.chars() {
        if utf16 >= utf16_offset {
            break;
        }
        utf8 += character.len_utf8();
        utf16 += character.len_utf16();
    }
    utf8
}

fn utf16_offset(text: &str, utf8_offset: usize) -> usize {
    let mut utf8 = 0;
    let mut utf16 = 0;
    for character in text.chars() {
        if utf8 >= utf8_offset {
            break;
        }
        utf8 += character.len_utf8();
        utf16 += character.len_utf16();
    }
    utf16
}

impl EntityInputHandler for SearchInput {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.buffer.content[range].to_owned())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.buffer.selected_range),
            reversed: self.buffer.selection_reversed,
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _: &mut Window, _: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.buffer.selected_range.clone());
        self.buffer.replace(range, &single_line(new_text));
        self.marked_range = None;
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.buffer.selected_range.clone());
        let replacement = single_line(new_text);
        let start = range.start;
        self.buffer.replace(range, &replacement);
        self.marked_range = (!replacement.is_empty()).then_some(start..start + replacement.len());
        if let Some(selection) = new_selected_range_utf16 {
            let selection = utf8_offset(&replacement, selection.start)
                ..utf8_offset(&replacement, selection.end);
            self.buffer.selected_range = start + selection.start..start + selection.end;
            self.buffer.selection_reversed = false;
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let line = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        Some(Bounds::from_corners(
            point(bounds.left() + line.x_for_index(range.start), bounds.top()),
            point(bounds.left() + line.x_for_index(range.end), bounds.bottom()),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let bounds = self.last_bounds?;
        let line_point = bounds.localize(&point)?;
        let line = self.last_layout.as_ref()?;
        let index = line.index_for_x(line_point.x)?;
        Some(self.offset_to_utf16(index))
    }
}

struct SearchTextElement {
    input: Entity<SearchInput>,
}

struct SearchPrepaint {
    line: Option<ShapedLine>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for SearchTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SearchTextElement {
    type RequestLayoutState = ();
    type PrepaintState = SearchPrepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = window.line_height().into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.buffer.content.clone();
        let selection = input.buffer.selected_range.clone();
        let cursor = input.buffer.cursor_offset();
        let style = window.text_style();
        let (display_text, color) = if content.is_empty() {
            (input.placeholder.clone(), input.theme.muted_text.into())
        } else {
            (SharedString::from(content), style.color)
        };
        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked.end - marked.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len() - marked.end,
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display_text, font_size, &runs, None);
        let cursor_x = line.x_for_index(cursor);
        let (selection, cursor) = if selection.is_empty() {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_x, bounds.top()),
                        gpui::size(px(1.0), bounds.size.height),
                    ),
                    input.theme.text,
                )),
            )
        } else {
            (
                Some(fill(
                    Bounds::from_corners(
                        point(
                            bounds.left() + line.x_for_index(selection.start),
                            bounds.top(),
                        ),
                        point(
                            bounds.left() + line.x_for_index(selection.end),
                            bounds.bottom(),
                        ),
                    ),
                    input.theme.selection,
                )),
                None,
            )
        };
        SearchPrepaint {
            line: Some(line),
            cursor,
            selection,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }
        let line = prepaint.line.take().expect("search line is prepainted");
        line.paint(bounds.origin, window.line_height(), window, cx)
            .expect("search line paint succeeds");
        if focus_handle.is_focused(window) {
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
        }
        self.input.update(cx, |input, _| {
            input.last_layout = Some(line);
            input.last_bounds = Some(bounds);
        });
    }
}

impl gpui::Render for SearchInput {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        div()
            .id(self.element_id)
            .key_context("OnionskinSearch")
            .track_focus(&self.focus_handle(cx))
            .h(px(28.0))
            .w_full()
            .flex()
            .items_center()
            .overflow_hidden()
            .px_2()
            .rounded_md()
            .bg(theme.input)
            .text_color(theme.text)
            .text_size(px(13.0))
            .line_height(px(18.0))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .child(SearchTextElement { input: cx.entity() })
    }
}

impl Focusable for SearchInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use onionskin_plugin_api::{
        Command, CommandCtx, CommandPlugin, PointerInput, ToolCtx, ToolPlugin,
    };

    use super::*;

    struct FakeTool {
        id: &'static str,
        name: &'static str,
    }

    impl ToolPlugin for FakeTool {
        fn id(&self) -> &'static str {
            self.id
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn icon(&self) -> &'static str {
            "fake"
        }

        fn on_pointer_down(&mut self, _: &mut ToolCtx, _: PointerInput) {}
        fn on_pointer_move(&mut self, _: &mut ToolCtx, _: PointerInput) {}
        fn on_pointer_up(&mut self, _: &mut ToolCtx, _: PointerInput) {}
    }

    struct FakeCommands;

    impl CommandPlugin for FakeCommands {
        fn commands(&self) -> Vec<Command> {
            vec![
                Command {
                    id: "pages.rotate",
                    title: "Rotate Clockwise",
                    keybind: None,
                    effect: onionskin_plugin_api::CommandEffect::Reads,
                    run: Box::new(|_: &mut CommandCtx| Ok(())),
                },
                Command {
                    id: "document.properties",
                    title: "Document Properties",
                    keybind: None,
                    effect: onionskin_plugin_api::CommandEffect::Reads,
                    run: Box::new(|_: &mut CommandCtx| Ok(())),
                },
            ]
        }
    }

    fn registry() -> PluginRegistry {
        let mut registry = PluginRegistry::new();
        registry.register_tool(Box::new(FakeTool {
            id: "view.select",
            name: "Select Tool",
        }));
        registry.register_tool(Box::new(FakeTool {
            id: "annotate.marker",
            name: "Highlight",
        }));
        registry.register_commands(&FakeCommands);
        registry
    }

    #[test]
    fn search_matches_tool_names_and_ids_case_insensitively() {
        let registry = registry();

        assert!(matches!(
            search_registry(&registry, "HIGHL").as_slice(),
            [SearchResult::Tool { index: 1, .. }]
        ));
        assert!(matches!(
            search_registry(&registry, "MARKER").as_slice(),
            [SearchResult::Tool { index: 1, .. }]
        ));
    }

    #[test]
    fn search_matches_command_titles_and_ids_case_insensitively() {
        let registry = registry();

        assert!(matches!(
            search_registry(&registry, "CLOCKWISE").as_slice(),
            [SearchResult::Command { index: 0, .. }]
        ));
        assert!(matches!(
            search_registry(&registry, "DOCUMENT.PROPERTIES").as_slice(),
            [SearchResult::Command { index: 1, .. }]
        ));
    }

    #[test]
    fn search_preserves_registry_order_with_tools_before_commands() {
        let results = search_registry(&registry(), "t");

        assert!(matches!(results[0], SearchResult::Tool { index: 0, .. }));
        assert!(matches!(results[1], SearchResult::Tool { index: 1, .. }));
        assert!(matches!(results[2], SearchResult::Command { index: 0, .. }));
        assert!(matches!(results[3], SearchResult::Command { index: 1, .. }));
    }

    #[test]
    fn no_registry_match_does_not_fall_through_to_document_search() {
        let registry = registry();

        assert!(search_registry(&registry, "needle only in the pdf").is_empty());
        let explicit = document_search_result("needle only in the pdf").unwrap();
        assert!(matches!(explicit, SearchResult::DocumentSearch { .. }));
        // Live since P9: the frame opens the find bar on it rather than
        // reporting a milestone it is waiting for.
        assert_eq!(unavailable_selection(&explicit, true), None);
    }

    #[test]
    fn every_route_the_panel_offers_is_live_with_a_document_open() {
        assert_eq!(
            unavailable_selection(
                &SearchResult::Tool {
                    index: 0,
                    id: "view.select",
                    name: "Select Tool",
                },
                true
            ),
            None
        );
        assert_eq!(
            unavailable_selection(
                &SearchResult::Command {
                    index: 0,
                    id: "pages.rotate",
                    title: "Rotate Clockwise",
                },
                true
            ),
            None
        );
        assert_eq!(
            unavailable_selection(
                &SearchResult::DocumentSearch {
                    query: "needle".to_owned(),
                },
                true
            ),
            None
        );
        let unavailable = SearchResult::Unavailable {
            label: "Already unavailable".to_owned(),
            reason: "Pinned reason",
        };
        assert_eq!(unavailable_selection(&unavailable, true), Some(unavailable));
    }

    /// A command runs against a document, so with none open the panel says
    /// that, rather than naming a milestone that has landed.
    #[test]
    fn a_command_hit_waits_for_a_document_rather_than_for_a_milestone() {
        let registry = registry();
        let command = search_registry(&registry, "rotate").remove(0);

        assert!(matches!(
            unavailable_selection(&command, false),
            Some(SearchResult::Unavailable { label, reason })
                if label == "Rotate Clockwise" && reason == "No document is open"
        ));
        assert_eq!(unavailable_selection(&command, true), None);
        let select = search_registry(&registry, "select").remove(0);
        assert_eq!(unavailable_selection(&select, true), None);
    }

    #[test]
    fn document_search_waits_for_a_document_rather_than_no_oping() {
        let result = document_search_result("needle").unwrap();

        assert!(matches!(
            unavailable_selection(&result, false),
            Some(SearchResult::Unavailable { label, reason })
                if label == "Search document for \"needle\"" && reason == NO_DOCUMENT
        ));
        assert_eq!(unavailable_selection(&result, true), None);
    }

    #[test]
    fn editing_and_deletion_stop_at_grapheme_boundaries() {
        let mut buffer = SearchBuffer {
            content: "a👨‍👩‍👧‍👦e\u{301}".to_owned(),
            ..Default::default()
        };
        buffer.move_to(buffer.content.len());

        buffer.backspace();
        assert_eq!(buffer.content, "a👨‍👩‍👧‍👦");
        buffer.backspace();
        assert_eq!(buffer.content, "a");
        buffer.move_to(0);
        buffer.delete();
        assert!(buffer.content.is_empty());
    }

    #[test]
    fn cursor_and_selection_transitions_follow_grapheme_boundaries() {
        let family = "👨‍👩‍👧‍👦";
        let accent = "e\u{301}";
        let mut buffer = SearchBuffer {
            content: format!("a{family}{accent}"),
            ..Default::default()
        };

        buffer.move_to(1);
        buffer.move_right();
        assert_eq!(buffer.cursor_offset(), 1 + family.len());
        buffer.select_right();
        assert_eq!(
            buffer.selected_range,
            1 + family.len()..1 + family.len() + accent.len()
        );
        assert!(!buffer.selection_reversed);
        buffer.select_left();
        assert!(buffer.selected_range.is_empty());
        buffer.select_left();
        assert_eq!(buffer.selected_range, 1..1 + family.len());
        assert!(buffer.selection_reversed);
        buffer.delete();
        assert_eq!(buffer.content, format!("a{accent}"));
        assert_eq!(buffer.cursor_offset(), 1);
    }

    #[test]
    fn utf16_offsets_round_trip_through_multibyte_text() {
        let text = "a😀e\u{301}";

        assert_eq!(utf16_offset(text, 0), 0);
        assert_eq!(utf16_offset(text, "a😀".len()), 3);
        assert_eq!(utf8_offset(text, 3), "a😀".len());
        assert_eq!(utf16_offset(text, text.len()), 5);
        assert_eq!(utf8_offset(text, 5), text.len());
    }

    #[test]
    fn pasted_text_remains_single_line() {
        assert_eq!(single_line("one\r\ntwo\nthree"), "one  two three");
    }

    /// The placeholder is the only thing a sighted user sees in an empty
    /// field, and it is gone the moment anything is typed, so the field says
    /// whichever of the two is on screen and keeps its name either way.
    #[test]
    fn a_text_field_reads_what_was_typed_and_its_placeholder_until_then() {
        let empty = field_node(
            "global-search-input",
            "Search",
            "",
            "Search tools or document",
            TextField::Search,
        );
        let typed = field_node(
            "global-search-input",
            "Search",
            "rotate",
            "Search tools or document",
            TextField::Search,
        );

        assert_eq!(empty.role, accesskit::Role::SearchInput);
        assert_eq!(empty.label, "Search");
        assert_eq!(
            empty.description.as_deref(),
            Some("Search tools or document")
        );
        assert_eq!(typed.label, "Search");
        assert_eq!(empty.value.as_deref(), Some(""));
        assert_eq!(typed.value.as_deref(), Some("rotate"));
        assert_eq!(typed.description, None);
        assert_eq!(typed.activation, Some(Activation::Focus(TextField::Search)));
        assert_eq!(typed.key, ElementId::from("global-search-input"));
    }

    /// The same thing over a live field rather than over the helper, so the
    /// wiring between the two is covered as well as the shape it produces.
    #[cfg(feature = "shell-test-support")]
    #[gpui::test]
    fn a_live_field_announces_its_own_id_and_what_is_in_it(cx: &mut gpui::TestAppContext) {
        let theme = crate::shell::chrome::ShellViewState::new(
            gpui::WindowAppearance::Dark,
            crate::preferences::ThemePreference::System,
        )
        .tokens();
        let (input, cx) = cx.add_window_view(|_window, cx| {
            SearchInput::with_placeholder("page-entry-input", "Page", theme, cx)
        });
        cx.run_until_parked();

        let empty = input.update(cx, |input, _cx| {
            input.accessible("Page Number", TextField::Page)
        });
        assert_eq!(empty.key, ElementId::from("page-entry-input"));
        assert_eq!(empty.role, accesskit::Role::NumberInput);
        assert_eq!(empty.label, "Page Number");
        assert_eq!(empty.description.as_deref(), Some("Page"));

        let typed = input.update(cx, |input, cx| {
            input.set_query("7", cx);
            input.accessible("Page Number", TextField::Page)
        });
        assert_eq!(empty.value.as_deref(), Some(""));
        assert_eq!(typed.value.as_deref(), Some("7"));
        assert_eq!(typed.description, None);
        assert_eq!(typed.activation, Some(Activation::Focus(TextField::Page)));
    }
}
