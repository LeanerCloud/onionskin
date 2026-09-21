//! The document's own Initial View, honoured when it opens.
//!
//! What the Properties dialog's Initial View tab writes - `/PageLayout`,
//! `/PageMode` and an `/OpenAction` destination - is read back here from the
//! session, after the Page Display preferences have been applied, so a
//! document that says how it opens wins over the user's default, as it does
//! in Acrobat. A document that says nothing leaves the preferences in force.

use onionskin_core::metadata::{InitialView, OpenFit, PageLayout, PageMode};
use onionskin_core::{FitMode, PageLayoutMode, MAX_ZOOM, MIN_ZOOM};

use super::canvas::{CanvasError, CanvasModel};
use super::panes::NavigationPane;

/// Apply the document's layout, magnification and opening page to `model`.
///
/// A catalog this cannot read opens at the preferences' view: how a document
/// opens is not a reason for it not to.
pub(in crate::shell) fn apply_initial_view(model: &mut CanvasModel) -> Result<(), CanvasError> {
    let Ok(view) = model.document_mut().initial_view() else {
        return Ok(());
    };
    apply(model, &view)
}

fn apply(model: &mut CanvasModel, view: &InitialView) -> Result<(), CanvasError> {
    if let Some(layout) = view.layout {
        let (mode, cover) = layout_mode(layout);
        model.set_layout_mode(mode)?;
        model.set_show_cover(cover)?;
    }
    if view.page.is_some() {
        match view.fit {
            OpenFit::Default => {}
            OpenFit::Page | OpenFit::Visible => {
                // Fit Visible needs the page's raster to find its content,
                // and nothing is drawn yet at open: the whole page is the
                // nearest view that is not a guess.
                model.fit(FitMode::Page)?;
            }
            OpenFit::Width => {
                model.fit(FitMode::Width)?;
            }
            OpenFit::Height => {
                model.fit(FitMode::Height)?;
            }
            OpenFit::Zoom(zoom) => {
                model.zoom_to((zoom as f32).clamp(MIN_ZOOM, MAX_ZOOM))?;
            }
        }
    }
    if let Some(page) = view
        .page
        .filter(|page| *page < model.view_state().page_count)
    {
        model.go_to_page(page)?;
    }
    Ok(())
}

/// The viewer's layout for a `/PageLayout`, and whether page one stands
/// alone: `Right` puts odd pages on the right, which is a cover page.
pub(in crate::shell) fn layout_mode(layout: PageLayout) -> (PageLayoutMode, bool) {
    match layout {
        PageLayout::SinglePage => (PageLayoutMode::SinglePage, false),
        PageLayout::OneColumn => (PageLayoutMode::SinglePageContinuous, false),
        PageLayout::TwoColumnLeft => (PageLayoutMode::TwoPageContinuous, false),
        PageLayout::TwoColumnRight => (PageLayoutMode::TwoPageContinuous, true),
        PageLayout::TwoPageLeft => (PageLayoutMode::TwoPage, false),
        PageLayout::TwoPageRight => (PageLayoutMode::TwoPage, true),
    }
}

/// The navigation pane a `/PageMode` opens with. Full Screen is not one:
/// it is a window state, and a document does not take the user's screen
/// over on open here.
pub(in crate::shell) fn pane_for(mode: PageMode) -> Option<NavigationPane> {
    match mode {
        PageMode::UseOutlines => Some(NavigationPane::Bookmarks),
        PageMode::UseThumbs => Some(NavigationPane::Thumbnails),
        PageMode::UseAttachments => Some(NavigationPane::Attachments),
        PageMode::UseOC => Some(NavigationPane::Layers),
        PageMode::UseNone | PageMode::FullScreen => None,
    }
}

/// The pane the document asks to open with, if any.
pub(in crate::shell) fn initial_pane(model: &mut CanvasModel) -> Option<NavigationPane> {
    let view = model.document_mut().initial_view().ok()?;
    pane_for(view.mode?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_right_layout_is_a_cover_page_and_a_column_is_continuous() {
        assert_eq!(
            layout_mode(PageLayout::TwoColumnRight),
            (PageLayoutMode::TwoPageContinuous, true)
        );
        assert_eq!(
            layout_mode(PageLayout::TwoPageLeft),
            (PageLayoutMode::TwoPage, false)
        );
        assert_eq!(
            layout_mode(PageLayout::OneColumn),
            (PageLayoutMode::SinglePageContinuous, false)
        );
    }

    #[test]
    fn every_pane_mode_opens_its_pane_and_full_screen_opens_none() {
        assert_eq!(
            pane_for(PageMode::UseOutlines),
            Some(NavigationPane::Bookmarks)
        );
        assert_eq!(
            pane_for(PageMode::UseThumbs),
            Some(NavigationPane::Thumbnails)
        );
        assert_eq!(
            pane_for(PageMode::UseAttachments),
            Some(NavigationPane::Attachments)
        );
        assert_eq!(pane_for(PageMode::UseOC), Some(NavigationPane::Layers));
        assert_eq!(pane_for(PageMode::FullScreen), None);
        assert_eq!(pane_for(PageMode::UseNone), None);
    }
}
