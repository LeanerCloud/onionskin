use std::collections::VecDeque;
use std::num::NonZeroUsize;

use crate::viewport::ZoomPolicy;
use crate::{PageIndex, PageLayoutMode, ViewPoint, ViewRotation};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewState {
    pub current_page: PageIndex,
    pub offset: ViewPoint,
    pub zoom: f32,
    pub zoom_policy: ZoomPolicy,
    pub mode: PageLayoutMode,
    pub show_cover: bool,
    pub rotation: ViewRotation,
}

#[derive(Clone, Debug)]
pub struct ViewHistory {
    capacity: NonZeroUsize,
    previous: VecDeque<ViewState>,
    next: VecDeque<ViewState>,
}

impl ViewHistory {
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            capacity,
            previous: VecDeque::new(),
            next: VecDeque::new(),
        }
    }

    pub fn can_previous(&self) -> bool {
        !self.previous.is_empty()
    }

    pub fn can_next(&self) -> bool {
        !self.next.is_empty()
    }

    pub fn clear(&mut self) {
        self.previous.clear();
        self.next.clear();
    }

    pub fn record(&mut self, state: ViewState) {
        if self.previous.back().copied() == Some(state) {
            return;
        }
        self.previous.push_back(state);
        trim_to_capacity(&mut self.previous, self.capacity);
        self.next.clear();
    }

    pub fn previous(&mut self, current: ViewState) -> Option<ViewState> {
        let target = self.previous.pop_back()?;
        self.next.push_back(current);
        trim_to_capacity(&mut self.next, self.capacity);
        Some(target)
    }

    pub fn next(&mut self, current: ViewState) -> Option<ViewState> {
        let target = self.next.pop_back()?;
        self.previous.push_back(current);
        trim_to_capacity(&mut self.previous, self.capacity);
        Some(target)
    }
}

fn trim_to_capacity(stack: &mut VecDeque<ViewState>, capacity: NonZeroUsize) {
    while stack.len() > capacity.get() {
        stack.pop_front();
    }
}
