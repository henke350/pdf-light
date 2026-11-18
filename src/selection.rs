//! Which pages are selected in the pages panel: click, Ctrl+click, Shift+click.
//!
//! Positions are indices into the current document (after deletions), not the original file.

use std::collections::BTreeSet;

/// The selected pages plus the anchor that Shift+click measures a range from.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Selection {
    /// A `BTreeSet` keeps its items sorted and without duplicates, so the
    /// positions come out ascending for free.
    selected: BTreeSet<usize>,
    /// The page last clicked without Shift. `Option` means "maybe a value":
    /// `None` until the first click (or after `clear`).
    anchor: Option<usize>,
}

impl Selection {
    /// Clicks the page at `position`, the way file lists in Windows do:
    /// plain click selects only it, Ctrl+click adds or removes it, Shift+click
    /// selects the range from the anchor, Ctrl+Shift+click adds that range.
    pub fn click(&mut self, position: usize, ctrl: bool, shift: bool) {
        if shift {
            // With no anchor yet, the range is just this page.
            let anchor = self.anchor.unwrap_or(position);
            let range = anchor.min(position)..=anchor.max(position);
            if !ctrl {
                self.selected.clear();
            }
            self.selected.extend(range);
            self.anchor = Some(anchor);
            return;
        }
        if ctrl {
            // `insert` returns `false` when the page was already there: then remove it.
            if !self.selected.insert(position) {
                self.selected.remove(&position);
            }
        } else {
            self.selected.clear();
            self.selected.insert(position);
        }
        self.anchor = Some(position);
    }

    /// The selected positions, ascending.
    pub fn positions(&self) -> Vec<usize> {
        self.selected.iter().copied().collect()
    }

    pub fn contains(&self, position: usize) -> bool {
        self.selected.contains(&position)
    }

    /// How many pages are selected.
    pub fn len(&self) -> usize {
        self.selected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// Selects nothing and forgets the anchor.
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    /// Selects every page of a document with `count` pages.
    pub fn select_all(&mut self, count: usize) {
        self.selected = (0..count).collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn click_should_select_only_that_page() {
        let mut selection = Selection::default();
        selection.click(2, false, false);
        selection.click(4, false, false);
        assert_eq!(selection.positions(), [4]);
    }

    #[test]
    fn ctrl_click_should_toggle() {
        let mut selection = Selection::default();
        selection.click(1, false, false);
        selection.click(3, true, false);
        selection.click(1, true, false);
        assert_eq!(selection.positions(), [3]);
    }

    #[test]
    fn shift_click_should_select_range_from_anchor() {
        let mut selection = Selection::default();
        selection.click(5, false, false);
        selection.click(2, false, true);
        assert_eq!(selection.positions(), [2, 3, 4, 5]);
    }

    #[test]
    fn shift_click_should_keep_anchor_for_the_next_range() {
        let mut selection = Selection::default();
        selection.click(3, false, false);
        selection.click(5, false, true);
        selection.click(1, false, true);
        assert_eq!(selection.positions(), [1, 2, 3]);
    }

    #[test]
    fn ctrl_shift_click_should_add_range_to_selection() {
        let mut selection = Selection::default();
        selection.click(0, false, false);
        selection.click(4, true, false);
        selection.click(6, true, true);
        assert_eq!(selection.positions(), [0, 4, 5, 6]);
    }

    #[test]
    fn select_all_should_select_every_position() {
        let mut selection = Selection::default();
        selection.click(1, false, false);
        selection.select_all(3);
        assert_eq!(selection.positions(), [0, 1, 2]);
    }

    #[test]
    fn clear_should_forget_selection_and_anchor() {
        let mut selection = Selection::default();
        selection.click(4, false, false);
        selection.clear();
        selection.click(2, false, true);
        assert_eq!(selection.positions(), [2]);
    }
}
