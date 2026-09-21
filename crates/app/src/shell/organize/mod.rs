//! Organize Pages: the document's pages as a grid, to select, reorder and
//! act on together.
//!
//! **The pictures are the thumbnails pane's.** The grid draws from, and
//! asks through, the pane's one cache, so the two surfaces cannot disagree
//! about eviction or about which pictures are stale; a second thumbnail
//! path would bring back the two bugs the pane's was fixed for.
//!
//! **Cells are a fixed size**, so which page is under the pointer, which
//! pages a marquee covers and where a drop lands are arithmetic in
//! [`GridLayout`] rather than something only the layout engine knows.
//!
//! **A drag reorders once, on drop.** Nothing moves while the pointer does,
//! so a drag is one undo step and a drag that is cancelled has changed
//! nothing.

mod view;

pub(in crate::shell) use view::{accessible, render, OrganizeAction};

use std::cell::Cell;
use std::collections::BTreeSet;
use std::ops::Range;
use std::rc::Rc;

use onionskin_core::PageIndex;

/// Why a build cannot change pages from the grid or the thumbnails pane.
#[cfg(not(feature = "tools-organize"))]
pub(in crate::shell) const NO_ORGANIZE: &str = "The Organize Pages plugin is not installed";

/// Why page edits cannot run on a document that refuses edits for
/// `document`, if they cannot: that refusal, or a build without the plugin
/// that makes them.
pub(in crate::shell) fn page_edit_refusal(document: Option<&'static str>) -> Option<&'static str> {
    #[cfg(not(feature = "tools-organize"))]
    {
        let _ = document;
        Some(NO_ORGANIZE)
    }
    #[cfg(feature = "tools-organize")]
    {
        document
    }
}

/// Space between cells, and around the grid.
pub(in crate::shell) const GAP: f32 = 12.0;
/// A pointer that moves less than this between press and release clicked.
const DRAG_THRESHOLD: f32 = 4.0;
/// How wide the grid is assumed to be before it has been drawn once.
const FIRST_WIDTH: f32 = 640.0;

/// Where cells are, for a grid `width` wide of `cell`-sized cells.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(in crate::shell) struct GridLayout {
    pub(in crate::shell) columns: usize,
    pub(in crate::shell) cell: f32,
}

impl GridLayout {
    pub(in crate::shell) fn new(width: f32, cell: f32) -> Self {
        let columns = ((width - GAP) / (cell + GAP)).floor().max(1.0) as usize;
        GridLayout { columns, cell }
    }

    fn pitch(self) -> f32 {
        self.cell + GAP
    }

    /// The top-left corner of `page`'s cell, in grid coordinates.
    pub(in crate::shell) fn origin(self, page: PageIndex) -> (f32, f32) {
        let (column, row) = (page % self.columns, page / self.columns);
        (
            GAP + column as f32 * self.pitch(),
            GAP + row as f32 * self.pitch(),
        )
    }

    /// The page whose cell `point` is inside, if any.
    pub(in crate::shell) fn page_at(
        self,
        (x, y): (f32, f32),
        page_count: usize,
    ) -> Option<PageIndex> {
        if x < GAP || y < GAP {
            return None;
        }
        let column = ((x - GAP) / self.pitch()).floor() as usize;
        let row = ((y - GAP) / self.pitch()).floor() as usize;
        let inside = (x - GAP) % self.pitch() < self.cell && (y - GAP) % self.pitch() < self.cell;
        let page = row * self.columns + column;
        (inside && column < self.columns && page < page_count).then_some(page)
    }

    /// Where a drop at `point` puts the dragged pages: before the page whose
    /// left half it is over, after one whose right half it is over, and at
    /// the end below the last row.
    pub(in crate::shell) fn drop_slot(self, (x, y): (f32, f32), page_count: usize) -> PageIndex {
        let row = ((y - GAP).max(0.0) / self.pitch()).floor() as usize;
        let cells = (x - GAP).max(0.0) / self.pitch();
        let (column, after) = if cells >= self.columns as f32 {
            (self.columns - 1, true)
        } else {
            let column = cells.floor();
            let into = (cells - column) * self.pitch();
            (column as usize, into > self.cell / 2.0)
        };
        (row * self.columns + column + usize::from(after)).min(page_count)
    }

    /// Every page whose cell a marquee from `a` to `b` touches.
    pub(in crate::shell) fn pages_in(
        self,
        a: (f32, f32),
        b: (f32, f32),
        page_count: usize,
    ) -> BTreeSet<PageIndex> {
        let (left, right) = (a.0.min(b.0), a.0.max(b.0));
        let (top, bottom) = (a.1.min(b.1), a.1.max(b.1));
        (0..page_count)
            .filter(|page| {
                let (x, y) = self.origin(*page);
                x < right && x + self.cell > left && y < bottom && y + self.cell > top
            })
            .collect()
    }

    /// How tall all the rows are.
    pub(in crate::shell) fn height(self, page_count: usize) -> f32 {
        let rows = page_count.div_ceil(self.columns);
        GAP + rows as f32 * self.pitch()
    }

    /// The pages a viewport `height` tall shows when scrolled `scroll` down:
    /// whole rows, one row of slack either side, never past the document.
    pub(in crate::shell) fn visible(
        self,
        scroll: f32,
        height: f32,
        page_count: usize,
    ) -> Range<usize> {
        if page_count == 0 || height <= 0.0 {
            return 0..0;
        }
        let first_row = ((scroll - GAP).max(0.0) / self.pitch()).floor() as usize;
        let rows = (height / self.pitch()).ceil() as usize + 2;
        let first = first_row.saturating_sub(1) * self.columns;
        let end = (first_row + rows) * self.columns;
        first.min(page_count)..end.min(page_count)
    }
}

/// Where the grid was last drawn, in window coordinates: left, top, width,
/// height. Shared with the frame that measures it.
pub(in crate::shell) type GridBounds = Rc<Cell<Option<(f32, f32, f32, f32)>>>;

/// Which keys were held with a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(in crate::shell) enum Held {
    #[default]
    Nothing,
    /// Shift: extend from the anchor.
    Extend,
    /// Cmd or Ctrl: add or remove one.
    Toggle,
}

impl Held {
    pub(in crate::shell) fn of(modifiers: &gpui::Modifiers) -> Self {
        if modifiers.shift {
            Held::Extend
        } else if modifiers.platform || modifiers.control {
            Held::Toggle
        } else {
            Held::Nothing
        }
    }
}

/// A press that has not been released yet.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::shell) enum Gesture {
    /// Pressed on a selected page: a drag of the selection, or a click.
    Drag {
        pages: Vec<PageIndex>,
        start: (f32, f32),
        now: (f32, f32),
        /// A plain press on a page already in a larger selection: released
        /// without moving, it narrows the selection to that page.
        narrow_to: Option<PageIndex>,
    },
    /// Pressed between pages: a marquee, adding to `base`.
    Marquee {
        base: BTreeSet<PageIndex>,
        start: (f32, f32),
        now: (f32, f32),
    },
}

/// What a release asks the document for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::shell) struct Reorder {
    pub(in crate::shell) pages: Vec<PageIndex>,
    pub(in crate::shell) before: PageIndex,
}

impl Reorder {
    /// Whether moving `pages` before `before` would leave every page where
    /// it is, which is a drop that should cost no undo step.
    fn changes_nothing(&self) -> bool {
        let first = self.pages[0];
        let contiguous = self.pages.windows(2).all(|pair| pair[1] == pair[0] + 1);
        let last = *self.pages.last().expect("a reorder moves something");
        contiguous && (first..=last + 1).contains(&self.before)
    }
}

/// The grid, open on one document.
pub(in crate::shell) struct OrganizeState {
    pub(in crate::shell) canvas: gpui::EntityId,
    pub(in crate::shell) scroll: f32,
    pub(in crate::shell) selection: BTreeSet<PageIndex>,
    /// Where a Shift-click extends from.
    pub(in crate::shell) anchor: Option<PageIndex>,
    pub(in crate::shell) gesture: Option<Gesture>,
    /// The grid's size the last time it was drawn, which the arithmetic
    /// between frames uses.
    pub(in crate::shell) bounds: GridBounds,
    pub(in crate::shell) error: Option<String>,
}

impl OrganizeState {
    pub(in crate::shell) fn new(canvas: gpui::EntityId, current: PageIndex) -> Self {
        OrganizeState {
            canvas,
            scroll: 0.0,
            selection: BTreeSet::from([current]),
            anchor: Some(current),
            gesture: None,
            bounds: Rc::new(Cell::new(None)),
            error: None,
        }
    }

    /// The grid's width and height as last drawn, or a guess before then.
    pub(in crate::shell) fn size(&self) -> (f32, f32) {
        self.bounds
            .get()
            .map_or((FIRST_WIDTH, 480.0), |(_, _, width, height)| {
                (width, height)
            })
    }

    /// A point in window coordinates, in the grid's own: its top-left is
    /// (0, 0) and it scrolls.
    pub(in crate::shell) fn to_grid(&self, (x, y): (f32, f32)) -> (f32, f32) {
        let (left, top) = self
            .bounds
            .get()
            .map_or((0.0, 0.0), |(left, top, _, _)| (left, top));
        (x - left, y - top + self.scroll)
    }

    /// The selection, in page order.
    pub(in crate::shell) fn selected(&self) -> Vec<PageIndex> {
        self.selection.iter().copied().collect()
    }

    /// A click on `page` with `held`.
    pub(in crate::shell) fn click(&mut self, page: PageIndex, held: Held) {
        match (held, self.anchor) {
            (Held::Extend, Some(anchor)) => {
                self.selection = (anchor.min(page)..=anchor.max(page)).collect();
            }
            (Held::Toggle, _) => {
                if !self.selection.remove(&page) {
                    self.selection.insert(page);
                }
                self.anchor = Some(page);
            }
            _ => {
                self.selection = BTreeSet::from([page]);
                self.anchor = Some(page);
            }
        }
    }

    /// Select every page.
    pub(in crate::shell) fn select_all(&mut self, page_count: usize) {
        self.selection = (0..page_count).collect();
    }

    /// Keep the selection to pages that exist. After an undo that takes
    /// pages away, what pointed past the end is dropped, and the grid says
    /// so rather than acting on pages that are gone.
    pub(in crate::shell) fn clamp(&mut self, page_count: usize) {
        let before = self.selection.len();
        self.selection.retain(|page| *page < page_count);
        if self.selection.len() < before {
            self.error = Some(format!(
                "{} selected {} no longer in the document",
                before - self.selection.len(),
                if before - self.selection.len() == 1 {
                    "page is"
                } else {
                    "pages are"
                }
            ));
        }
        if self.selection.is_empty() && page_count > 0 {
            self.selection.insert(page_count - 1);
        }
        self.anchor = self
            .anchor
            .map(|anchor| anchor.min(page_count.saturating_sub(1)));
        self.gesture = None;
    }

    /// A press at `point` (grid coordinates).
    pub(in crate::shell) fn press(
        &mut self,
        point: (f32, f32),
        held: Held,
        layout: GridLayout,
        page_count: usize,
    ) {
        self.error = None;
        match layout.page_at(point, page_count) {
            Some(page) => {
                let already = self.selection.contains(&page);
                let narrow_to =
                    (held == Held::Nothing && already && self.selection.len() > 1).then_some(page);
                if held != Held::Nothing || !already {
                    self.click(page, held);
                }
                self.gesture = self.selection.contains(&page).then(|| Gesture::Drag {
                    pages: self.selected(),
                    start: point,
                    now: point,
                    narrow_to,
                });
            }
            None => {
                let base = if held == Held::Nothing {
                    BTreeSet::new()
                } else {
                    self.selection.clone()
                };
                self.selection = base.clone();
                self.gesture = Some(Gesture::Marquee {
                    base,
                    start: point,
                    now: point,
                });
            }
        }
    }

    /// The pointer moved to `point` with the button down.
    pub(in crate::shell) fn drag_to(
        &mut self,
        point: (f32, f32),
        layout: GridLayout,
        page_count: usize,
    ) {
        match &mut self.gesture {
            Some(Gesture::Drag { now, .. }) => *now = point,
            Some(Gesture::Marquee { base, start, now }) => {
                *now = point;
                let mut selection = base.clone();
                selection.extend(layout.pages_in(*start, point, page_count));
                self.selection = selection;
            }
            None => {}
        }
    }

    /// The button came up at `point`: the reorder a drag asks for, if it
    /// asks for one. A click on a page in a multi-selection, without a
    /// drag, narrows the selection to it, as Acrobat's grid does.
    pub(in crate::shell) fn release(
        &mut self,
        point: (f32, f32),
        layout: GridLayout,
        page_count: usize,
    ) -> Option<Reorder> {
        let gesture = self.gesture.take()?;
        let Gesture::Drag {
            pages,
            start,
            narrow_to,
            ..
        } = gesture
        else {
            return None;
        };
        let moved = (point.0 - start.0).hypot(point.1 - start.1) >= DRAG_THRESHOLD;
        if !moved {
            if let Some(page) = narrow_to {
                self.click(page, Held::Nothing);
            }
            return None;
        }
        let reorder = Reorder {
            pages,
            before: layout.drop_slot(point, page_count),
        };
        (!reorder.changes_nothing()).then_some(reorder)
    }

    /// Escape, or the pointer left: the gesture ends having done nothing.
    pub(in crate::shell) fn cancel(&mut self) {
        if let Some(Gesture::Marquee { base, .. }) = self.gesture.take() {
            self.selection = base;
        }
    }

    /// After `reorder` has been applied: the moved pages stay selected at
    /// their new places.
    pub(in crate::shell) fn follow(&mut self, reorder: &Reorder) {
        let staying_before = reorder.before
            - reorder
                .pages
                .iter()
                .filter(|p| **p < reorder.before)
                .count();
        self.selection = (staying_before..staying_before + reorder.pages.len()).collect();
        self.anchor = Some(staying_before);
    }

    pub(in crate::shell) fn scroll_by(
        &mut self,
        delta: f32,
        layout: GridLayout,
        page_count: usize,
    ) {
        let (_, height) = self.size();
        let limit = (layout.height(page_count) - height).max(0.0);
        self.scroll = (self.scroll - delta).clamp(0.0, limit);
    }
}

#[cfg(test)]
mod tests;
