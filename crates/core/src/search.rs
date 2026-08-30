//! Document-level search: the walk `content` does not have, and the state the
//! find bar reads.
//!
//! `content::search` is per page and synchronous. A find over a thousand pages
//! cannot run that way on the UI thread, and it cannot hold every page's text
//! either, so the walk lives on its own worker thread and streams a page at a
//! time back to the session.
//!
//! The worker **constructs its own `cos::Document` inside the spawned
//! closure**, from the same `Arc<Vec<u8>>` the session already holds, exactly
//! as the render worker constructs its `render::Document`. Only the bytes cross
//! the thread boundary, so nothing here needs `cos::Document` to be `Send`.
//!
//! Two properties the tests pin, because both are easy to lose:
//!
//! * **Nothing accumulates.** One page's [`content::PageText`] exists at a time
//!   and is dropped before the next page is read. A [`SearchMatch`] owns its
//!   text and its quads, so keeping a hit does not keep the page it came from.
//! * **Nothing is silently skipped.** A page whose extraction fails is reported
//!   as a failure the UI shows, not dropped from the walk.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};

use onionskin_content as content;
use onionskin_cos::BytesSource;

use crate::{PageIndex, PageQuad, SearchOptions};

/// One hit, in the terms the viewer needs: which page, what was matched, and
/// where it sits on that page.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchMatch {
    pub page: PageIndex,
    pub text: String,
    pub quads: Vec<PageQuad>,
}

impl From<content::Match> for SearchMatch {
    fn from(hit: content::Match) -> Self {
        SearchMatch {
            page: hit.page,
            text: hit.text,
            quads: hit.quads,
        }
    }
}

/// A page the walk could not read. Reported, never skipped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageFailure {
    pub page: PageIndex,
    pub message: String,
}

impl fmt::Display for PageFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "page {}: {}", self.page + 1, self.message)
    }
}

/// What the find bar reads: the query, the hits found so far, which one is
/// current, and whether the walk is still going.
///
/// Hits are keyed by page, so results arriving in walk order (which starts at
/// the page being viewed and wraps) still read back in document order, and the
/// cursor survives a later page's results landing.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SearchState {
    needle: String,
    options: SearchOptions,
    pages: BTreeMap<PageIndex, Vec<SearchMatch>>,
    searched: usize,
    failures: Vec<PageFailure>,
    cursor: Option<(PageIndex, usize)>,
    running: bool,
}

impl SearchState {
    pub fn needle(&self) -> &str {
        &self.needle
    }

    pub fn options(&self) -> SearchOptions {
        self.options
    }

    /// Every hit found so far, in document order.
    pub fn matches(&self) -> impl Iterator<Item = &SearchMatch> {
        self.pages.values().flatten()
    }

    /// The hits on one page, which is what a highlight pass wants.
    pub fn matches_on(&self, page: PageIndex) -> &[SearchMatch] {
        self.pages.get(&page).map_or(&[], Vec::as_slice)
    }

    pub fn len(&self) -> usize {
        self.pages.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn current(&self) -> Option<&SearchMatch> {
        let (page, index) = self.cursor?;
        self.pages.get(&page)?.get(index)
    }

    /// The current hit's one-based position among all hits found so far, for
    /// the find bar's "3 of 12".
    pub fn current_ordinal(&self) -> Option<usize> {
        let (page, index) = self.cursor?;
        let before: usize = self
            .pages
            .range(..page)
            .map(|(_, matches)| matches.len())
            .sum();
        Some(before + index + 1)
    }

    pub fn failures(&self) -> &[PageFailure] {
        &self.failures
    }

    /// How many pages the walk has reported on, including pages with no hits
    /// and pages that failed.
    pub fn searched_pages(&self) -> usize {
        self.searched
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Sets the query, discarding results for the previous one. Returns whether
    /// anything changed; an unchanged query keeps its results and its cursor.
    pub fn set_query(&mut self, needle: impl Into<String>, options: SearchOptions) -> bool {
        let needle = needle.into();
        if self.needle == needle && self.options == options {
            return false;
        }
        self.needle = needle;
        self.options = options;
        self.clear_results();
        true
    }

    pub(crate) fn begin(&mut self) {
        self.clear_results();
        self.running = true;
    }

    pub(crate) fn finish(&mut self) {
        self.running = false;
    }

    /// Records one page's hits. Returns whether this call placed the cursor,
    /// which is how the viewer knows to scroll to the first hit.
    pub(crate) fn insert_page(&mut self, page: PageIndex, matches: Vec<SearchMatch>) -> bool {
        self.searched += 1;
        if matches.is_empty() {
            return false;
        }
        self.pages.insert(page, matches);
        if self.cursor.is_some() {
            return false;
        }
        self.cursor = Some((page, 0));
        true
    }

    pub(crate) fn record_failure(&mut self, page: PageIndex, message: String) {
        self.searched += 1;
        self.failures.push(PageFailure { page, message });
    }

    /// Moves to the next hit in document order, wrapping at the end.
    pub fn select_next(&mut self) -> Option<&SearchMatch> {
        self.step(Direction::Forward)
    }

    /// Moves to the previous hit in document order, wrapping at the start.
    pub fn select_previous(&mut self) -> Option<&SearchMatch> {
        self.step(Direction::Backward)
    }

    fn step(&mut self, direction: Direction) -> Option<&SearchMatch> {
        let (page, index) = self.cursor?;
        let hits = self.pages.get(&page)?.len();
        self.cursor = Some(match direction {
            Direction::Forward if index + 1 < hits => (page, index + 1),
            Direction::Forward => {
                let next = self
                    .pages
                    .range(page + 1..)
                    .next()
                    .or_else(|| self.pages.iter().next())?;
                (*next.0, 0)
            }
            Direction::Backward if index > 0 => (page, index - 1),
            Direction::Backward => {
                let previous = self
                    .pages
                    .range(..page)
                    .next_back()
                    .or_else(|| self.pages.iter().next_back())?;
                (*previous.0, previous.1.len().saturating_sub(1))
            }
        });
        self.current()
    }

    fn clear_results(&mut self) {
        self.pages.clear();
        self.failures.clear();
        self.searched = 0;
        self.cursor = None;
        self.running = false;
    }
}

#[derive(Clone, Copy)]
enum Direction {
    Forward,
    Backward,
}

#[derive(Debug)]
pub enum SearchWorkerError {
    Spawn(std::io::Error),
    Open(onionskin_cos::Error),
    Stopped,
}

impl fmt::Display for SearchWorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn(error) => write!(f, "starting search worker: {error}"),
            Self::Open(error) => write!(f, "search worker cannot read the document: {error}"),
            Self::Stopped => write!(f, "search worker stopped before answering"),
        }
    }
}

impl std::error::Error for SearchWorkerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Spawn(error) => Some(error),
            Self::Open(error) => Some(error),
            Self::Stopped => None,
        }
    }
}

/// One page's worth of progress.
pub(crate) enum SearchUpdate {
    Page {
        page: PageIndex,
        matches: Vec<SearchMatch>,
    },
    PageFailed {
        page: PageIndex,
        message: String,
    },
    Finished,
}

struct Job {
    generation: u64,
    needle: String,
    options: SearchOptions,
    start_page: PageIndex,
    page_count: usize,
}

enum Request {
    Search(Box<Job>),
    Shutdown,
}

/// The handle on the search worker. The session owns one, lazily, so a document
/// nobody searches never pays for the second parse.
pub(crate) struct DocumentSearch {
    requests: mpsc::Sender<Request>,
    updates: mpsc::Receiver<(u64, SearchUpdate)>,
    generation: u64,
    thread: Option<JoinHandle<()>>,
}

impl DocumentSearch {
    pub(crate) fn spawn(bytes: Arc<Vec<u8>>) -> Result<Self, SearchWorkerError> {
        let (requests, incoming) = mpsc::channel();
        let (outgoing, updates) = mpsc::channel();
        let (ready, opened) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("onionskin-search".into())
            .spawn(move || {
                let document = match onionskin_cos::Document::open_repairing(Box::new(
                    BytesSource::from_shared(bytes),
                )) {
                    Ok((document, _)) => {
                        let _ = ready.send(Ok(()));
                        document
                    }
                    Err(error) => {
                        let _ = ready.send(Err(SearchWorkerError::Open(error)));
                        return;
                    }
                };
                worker_loop(&document, &incoming, &outgoing);
            })
            .map_err(SearchWorkerError::Spawn)?;

        match opened.recv().map_err(|_| SearchWorkerError::Stopped)? {
            Ok(()) => Ok(Self {
                requests,
                updates,
                generation: 0,
                thread: Some(thread),
            }),
            Err(error) => {
                let _ = thread.join();
                Err(error)
            }
        }
    }

    /// Starts a walk, abandoning whatever the worker was doing. Results from
    /// the abandoned walk are discarded by generation on the way back.
    pub(crate) fn start(
        &mut self,
        needle: &str,
        options: SearchOptions,
        start_page: PageIndex,
        page_count: usize,
    ) -> Result<(), SearchWorkerError> {
        self.generation += 1;
        self.requests
            .send(Request::Search(Box::new(Job {
                generation: self.generation,
                needle: needle.to_owned(),
                options,
                start_page,
                page_count,
            })))
            .map_err(|_| SearchWorkerError::Stopped)
    }

    /// Stops caring about the walk in flight. The worker finishes the page it
    /// is on and its results are dropped here.
    pub(crate) fn cancel(&mut self) {
        self.generation += 1;
    }

    pub(crate) fn try_update(&mut self) -> Result<Option<SearchUpdate>, SearchWorkerError> {
        loop {
            match self.updates.try_recv() {
                Ok((generation, update)) if generation == self.generation => {
                    return Ok(Some(update))
                }
                Ok(_) => {}
                Err(mpsc::TryRecvError::Empty) => return Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => return Err(SearchWorkerError::Stopped),
            }
        }
    }
}

impl Drop for DocumentSearch {
    fn drop(&mut self) {
        let _ = self.requests.send(Request::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn worker_loop(
    document: &onionskin_cos::Document,
    incoming: &mpsc::Receiver<Request>,
    outgoing: &mpsc::Sender<(u64, SearchUpdate)>,
) {
    let mut queued: Option<Box<Job>> = None;
    loop {
        let job = match queued.take() {
            Some(job) => job,
            None => match incoming.recv() {
                Ok(Request::Search(job)) => job,
                Ok(Request::Shutdown) | Err(_) => return,
            },
        };
        let mut abandoned = false;
        for offset in 0..job.page_count {
            match drain(incoming, &mut queued) {
                Drained::Empty => {}
                Drained::Superseded => {
                    abandoned = true;
                    break;
                }
                Drained::Stop => return,
            }
            // The page's text lives exactly as long as this iteration: a
            // thousand-page walk holds one page, not a thousand.
            let page = (job.start_page + offset) % job.page_count;
            let update = match content::extract_page(document, page) {
                Ok(text) => SearchUpdate::Page {
                    page,
                    matches: content::search(&text, &job.needle, job.options)
                        .into_iter()
                        .map(SearchMatch::from)
                        .collect(),
                },
                Err(error) => SearchUpdate::PageFailed {
                    page,
                    message: error.to_string(),
                },
            };
            if outgoing.send((job.generation, update)).is_err() {
                return;
            }
        }
        if !abandoned
            && outgoing
                .send((job.generation, SearchUpdate::Finished))
                .is_err()
        {
            return;
        }
    }
}

enum Drained {
    Empty,
    Superseded,
    Stop,
}

fn drain(incoming: &mpsc::Receiver<Request>, queued: &mut Option<Box<Job>>) -> Drained {
    let mut superseded = false;
    loop {
        match incoming.try_recv() {
            Ok(Request::Search(job)) => {
                *queued = Some(job);
                superseded = true;
            }
            Ok(Request::Shutdown) | Err(mpsc::TryRecvError::Disconnected) => return Drained::Stop,
            Err(mpsc::TryRecvError::Empty) if superseded => return Drained::Superseded,
            Err(mpsc::TryRecvError::Empty) => return Drained::Empty,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(page: PageIndex, text: &str) -> SearchMatch {
        SearchMatch {
            page,
            text: text.to_owned(),
            quads: Vec::new(),
        }
    }

    fn state_with(pages: &[(PageIndex, usize)]) -> SearchState {
        let mut state = SearchState::default();
        state.set_query("a", SearchOptions::default());
        state.begin();
        for (page, count) in pages {
            state.insert_page(*page, (0..*count).map(|_| hit(*page, "a")).collect());
        }
        state
    }

    #[test]
    fn results_read_back_in_document_order_whatever_order_they_arrive_in() {
        let state = state_with(&[(7, 1), (9, 2), (0, 1)]);

        assert_eq!(
            state.matches().map(|hit| hit.page).collect::<Vec<_>>(),
            vec![0, 7, 9, 9]
        );
        assert_eq!(state.len(), 4);
        assert_eq!(state.searched_pages(), 3);
    }

    #[test]
    fn the_cursor_lands_on_the_first_page_that_reports_a_hit() {
        let mut state = SearchState::default();
        state.begin();

        assert!(!state.insert_page(3, Vec::new()));
        assert!(state.current().is_none());
        assert!(state.insert_page(7, vec![hit(7, "a")]));
        assert_eq!(state.current().map(|hit| hit.page), Some(7));
        // A page found later does not move the cursor off the hit the user is
        // looking at, even though it sorts before it.
        assert!(!state.insert_page(2, vec![hit(2, "a")]));
        assert_eq!(state.current().map(|hit| hit.page), Some(7));
        assert_eq!(state.current_ordinal(), Some(2));
    }

    #[test]
    fn next_and_previous_wrap_at_both_ends() {
        let mut state = state_with(&[(4, 2), (0, 1)]);
        // The walk started on page 4, so the cursor is on its first hit.
        assert_eq!(state.current_ordinal(), Some(2));

        assert_eq!(state.select_next().map(|hit| hit.page), Some(4));
        assert_eq!(state.current_ordinal(), Some(3));
        assert_eq!(state.select_next().map(|hit| hit.page), Some(0));
        assert_eq!(state.current_ordinal(), Some(1));
        assert_eq!(state.select_previous().map(|hit| hit.page), Some(4));
        assert_eq!(state.current_ordinal(), Some(3));
        assert_eq!(state.select_previous().map(|hit| hit.page), Some(4));
        assert_eq!(state.select_previous().map(|hit| hit.page), Some(0));
        assert_eq!(state.current_ordinal(), Some(1));
    }

    #[test]
    fn navigation_over_one_hit_stays_on_it() {
        let mut state = state_with(&[(2, 1)]);

        assert_eq!(state.select_next().map(|hit| hit.page), Some(2));
        assert_eq!(state.select_previous().map(|hit| hit.page), Some(2));
        assert_eq!(state.current_ordinal(), Some(1));
    }

    #[test]
    fn navigation_without_results_reports_nothing_rather_than_panicking() {
        let mut state = SearchState::default();

        assert!(state.select_next().is_none());
        assert!(state.select_previous().is_none());
        assert!(state.current_ordinal().is_none());
    }

    #[test]
    fn changing_the_query_drops_results_failures_and_the_cursor() {
        let mut state = state_with(&[(1, 1)]);
        state.record_failure(2, "broken".into());
        state.finish();

        assert!(state.set_query("other", SearchOptions::default()));
        assert_eq!(state.needle(), "other");
        assert_eq!(state.len(), 0);
        assert!(state.failures().is_empty());
        assert_eq!(state.searched_pages(), 0);
        assert!(state.current().is_none());
        assert!(!state.set_query("other", SearchOptions::default()));
    }

    #[test]
    fn a_failed_page_counts_as_searched_and_is_reported() {
        let mut state = state_with(&[(0, 1)]);
        state.record_failure(1, "content stream is not readable".into());
        state.finish();

        assert_eq!(state.searched_pages(), 2);
        assert_eq!(state.failures().len(), 1);
        assert_eq!(state.failures()[0].page, 1);
        assert_eq!(
            state.failures()[0].to_string(),
            "page 2: content stream is not readable"
        );
        assert!(!state.is_running());
    }
}
