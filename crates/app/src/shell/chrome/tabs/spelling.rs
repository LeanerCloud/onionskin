//! Check Spelling's half in the frame: reading the document's comments and
//! fields, keeping the dialog on the next word, and making a change.

use std::collections::{BTreeSet, VecDeque};

use gpui::{AppContext as _, Context, Window};
use onionskin_spelling::passages::{correct, misspellings, passages, Misspelling};
use onionskin_spelling::user::UserDictionary;
use onionskin_spelling::Checker;

use super::ShellFrame;
use crate::shell::chrome::spelling_dialog::{SpellingAction, SpellingState, CHANGE_TO_ID};
use crate::shell::chrome::SearchInput;
use crate::shell::dialog::ShellDialog;

impl ShellFrame {
    pub(super) fn open_spelling_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dismiss_menus(cx);
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let added = self
            .user_dictionary()
            .and_then(|dictionary| dictionary.words().ok())
            .unwrap_or_default();
        let checker = Checker::english().with_words(added);
        let read = passages(&mut canvas.read(cx).model.document_mut());
        let found = match read {
            Ok(found) => found,
            Err(error) => {
                self.notices.push(error.to_string());
                cx.notify();
                return;
            }
        };
        let queue = misspellings(&checker, &found).into();
        let theme = self.shell_view_state.tokens();
        let change_to =
            cx.new(|cx| SearchInput::with_placeholder(CHANGE_TO_ID, "Change to", theme, cx));
        self.show_dialog(ShellDialog::Spelling, window, cx);
        self.spelling = Some(SpellingState {
            checker,
            passages: found,
            queue,
            ignored: BTreeSet::new(),
            suggestions: Vec::new(),
            change_to,
            changed: 0,
            error: None,
        });
        self.show_next_word(cx);
    }

    pub(in crate::shell) fn spelling_dialog(&self) -> Option<&SpellingState> {
        self.spelling.as_ref()
    }

    fn user_dictionary(&self) -> Option<UserDictionary> {
        self.settings
            .paths
            .data
            .as_deref()
            .map(UserDictionary::in_dir)
    }

    /// Put the word now first up in the dialog, with its suggestions and
    /// the likeliest in Change To.
    fn show_next_word(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.spelling.as_mut() else {
            return;
        };
        let word = state.queue.front().map(|word| word.word.clone());
        state.suggestions = word
            .as_deref()
            .map(|word| state.checker.suggestions(word))
            .unwrap_or_default();
        let first = state.suggestions.first().cloned().unwrap_or_default();
        state
            .change_to
            .update(cx, |input, cx| input.set_query(first, cx));
        cx.notify();
    }

    pub(in crate::shell) fn run_spelling_action(
        &mut self,
        action: SpellingAction,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.spelling.as_mut() else {
            return;
        };
        state.error = None;
        let Some(word) = state.queue.front().map(|word| word.word.clone()) else {
            return;
        };
        match action {
            SpellingAction::Suggestion(index) => {
                if let Some(chosen) = state.suggestions.get(index).cloned() {
                    state
                        .change_to
                        .update(cx, |input, cx| input.set_query(chosen, cx));
                }
                cx.notify();
                return;
            }
            SpellingAction::Ignore => {
                state.queue.pop_front();
            }
            SpellingAction::IgnoreAll => {
                state.ignored.insert(word.clone());
                state.queue.retain(|found| found.word != word);
            }
            SpellingAction::AddToDictionary => {
                state.checker.add(&word);
                state.queue.retain(|found| found.word != word);
                self.remember_word(&word);
            }
            SpellingAction::Change => self.change_word(cx),
        }
        self.show_next_word(cx);
    }

    /// Keep `word` in the user's dictionary, saying so when it cannot be.
    fn remember_word(&mut self, word: &str) {
        let saved = match self.user_dictionary() {
            Some(dictionary) => dictionary
                .add(word)
                .map_err(|error| format!("{word} is accepted until the dialog closes, but was not saved: {error}")),
            None => Err(format!(
                "{word} is accepted until the dialog closes: there is no folder to keep the dictionary in"
            )),
        };
        if let (Err(error), Some(state)) = (saved, self.spelling.as_mut()) {
            state.error = Some(error);
        }
    }

    /// Put Change To in place of the word on show, and go on from after it.
    fn change_word(&mut self, cx: &mut Context<Self>) {
        let Some(canvas) = self.active_canvas().cloned() else {
            return;
        };
        let Some(state) = self.spelling.as_mut() else {
            return;
        };
        let Some(word) = state.queue.front().cloned() else {
            return;
        };
        let replacement = state.change_to.read(cx).query().to_owned();
        let passage = state.passages[word.passage].clone();
        let outcome = canvas.update(cx, |canvas, cx| {
            let mut text = String::new();
            let outcome = canvas.model.edit_pages(|doc| {
                text = correct(doc, &passage, word.range.clone(), &replacement, now())?;
                Ok(())
            });
            if outcome.is_ok() {
                canvas.handle_change(Ok(true), cx);
            }
            outcome.map(|()| text)
        });
        match outcome {
            Ok(text) => {
                state.passages[word.passage].text = text;
                state.changed += 1;
                let from = (word.passage, word.range.start + replacement.len());
                state.queue = remaining(state, from);
            }
            Err(error) => {
                state.error = Some(crate::shell::chrome::tabs::properties::sentence(
                    &error.to_string(),
                ));
            }
        }
    }
}

/// The words still to show from `from` on, passing over Ignore All's.
fn remaining(state: &SpellingState, from: (usize, usize)) -> VecDeque<Misspelling> {
    misspellings(&state.checker, &state.passages)
        .into_iter()
        .filter(|found| (found.passage, found.range.start) >= from)
        .filter(|found| !state.ignored.contains(&found.word))
        .collect()
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}
