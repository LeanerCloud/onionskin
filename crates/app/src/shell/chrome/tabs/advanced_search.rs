//! The frame's half of Edit > Advanced Search: opening the dialog and
//! running a search over the document in front.

use gpui::{Context, Entity, Focusable as _, Window};
use onionskin_core::metadata::PropertyCriterion;
use onionskin_core::{AttachmentSearch, SearchOptions};

use super::ShellFrame;
use crate::shell::chrome::advanced_search::{
    AdvancedAction, AdvancedSearchState, CriteriaOutcome, Outcome, NO_WORDS,
};
use crate::shell::dialog::ShellDialog;
use crate::shell::panes::{NavigationPane, PaneAction};
use crate::shell::Canvas;

impl ShellFrame {
    /// Open the dialog on the find bar's words and options.
    pub(super) fn open_advanced_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_canvas().is_none() {
            return;
        }
        let query = self.find_input.read(cx).query().to_owned();
        let options = self.find.options();
        self.show_dialog(ShellDialog::AdvancedSearch, window, cx);
        let theme = self.shell_view_state.tokens();
        let state = AdvancedSearchState::new(query, options, theme, cx);
        window.focus(&state.query.read(cx).focus_handle(cx));
        self.advanced_search = Some(state);
    }

    pub(in crate::shell) fn advanced_search_dialog(&self) -> Option<&AdvancedSearchState> {
        self.advanced_search.as_ref()
    }

    pub(super) fn run_advanced_action(
        &mut self,
        action: AdvancedAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if action == AdvancedAction::Search {
            self.run_advanced_search(window, cx);
        } else if let Some(state) = self.advanced_search.as_mut() {
            state.apply(action);
        }
        cx.notify();
    }

    /// Criteria, then the pages through the find walk, then attachments.
    fn run_advanced_search(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        let (Some(state), Some(canvas)) = (self.advanced_search.as_ref(), self.active_canvas())
        else {
            return;
        };
        let canvas = canvas.clone();
        let needle = state.query.read(cx).query().to_owned();
        let criterion = state.criterion(cx);
        let form = state.form;
        let outcome = if needle.trim().is_empty() {
            Outcome {
                criteria: CriteriaOutcome::NotUsed,
                attachments: None,
                error: Some(NO_WORDS.to_owned()),
            }
        } else {
            let criteria = check_criterion(&canvas, criterion, cx);
            let searched = matches!(
                criteria,
                CriteriaOutcome::NotUsed | CriteriaOutcome::Matched
            );
            let attachments = (searched && form.include_attachments)
                .then(|| search_attachments(&canvas, &needle, form.options, cx));
            if searched {
                self.search_pages(needle, form.options, cx);
            } else {
                super::cancel_find_on(&canvas, cx);
            }
            Outcome {
                criteria,
                attachments,
                error: None,
            }
        };
        if let Some(state) = self.advanced_search.as_mut() {
            state.outcome = Some(outcome);
        }
    }

    /// Run the words through the find bar's walk with the dialog's options,
    /// and show the Search Results pane that lists what it finds.
    fn search_pages(&mut self, needle: String, options: SearchOptions, cx: &mut Context<Self>) {
        self.find.set_options(options);
        self.find.open();
        self.find_input
            .update(cx, |input, cx| input.set_query(needle, cx));
        self.run_find_query(cx);
        if self.navigation.active() != Some(NavigationPane::SearchResults) {
            self.run_pane_action(PaneAction::Select(NavigationPane::SearchResults), cx);
        }
    }
}

fn check_criterion(
    canvas: &Entity<Canvas>,
    criterion: Option<PropertyCriterion>,
    cx: &mut Context<ShellFrame>,
) -> CriteriaOutcome {
    let Some(criterion) = criterion else {
        return CriteriaOutcome::NotUsed;
    };
    canvas.update(cx, |canvas, _| {
        let document = canvas.model.document_mut();
        let read = document
            .info()
            .and_then(|info| document.xmp().map(|xmp| (info, xmp)));
        match read {
            Ok((info, xmp)) => match criterion.matches(&info, xmp.as_ref()) {
                Ok(true) => CriteriaOutcome::Matched,
                Ok(false) => CriteriaOutcome::NotMatched,
                Err(error) => CriteriaOutcome::Refused(error.to_string()),
            },
            Err(error) => CriteriaOutcome::Refused(format!(
                "The document's properties cannot be read: {error}"
            )),
        }
    })
}

fn search_attachments(
    canvas: &Entity<Canvas>,
    needle: &str,
    options: SearchOptions,
    cx: &mut Context<ShellFrame>,
) -> AttachmentSearch {
    canvas.update(cx, |canvas, _| {
        canvas
            .model
            .document_mut()
            .search_attachments(needle, options)
            .unwrap_or_else(|error| AttachmentSearch {
                skipped: vec![format!("The attachments cannot be listed: {error}")],
                ..AttachmentSearch::default()
            })
    })
}
