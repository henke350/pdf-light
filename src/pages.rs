//! Which pages of the document are kept and how each is turned, with undo.
//! Nothing here touches the file.

use crate::error::PdfError;
use crate::rotation::Rotation;

/// One page as the owner sees it: which page of the original file it is,
/// and the turn the owner added to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeptPage {
    /// 0-based index in the original file.
    pub index: usize,
    pub rotation: Rotation,
}

impl KeptPage {
    /// Page `index` of the original file, not turned.
    pub fn upright(index: usize) -> Self {
        Self {
            index,
            rotation: Rotation::NONE,
        }
    }
}

/// Everything undo can bring back.
#[derive(Debug, Clone, PartialEq)]
struct State {
    /// Original 0-based indices, ascending.
    kept: Vec<usize>,
    /// The owner's turn for every page of the original file (also deleted ones,
    /// so undoing a delete brings a page back as it was).
    rotations: Vec<Rotation>,
}

/// The pages still in the document, as indices into the original file, and
/// how the owner has turned them.
pub struct PageList {
    state: State,
    /// Earlier states; the last entry is the most recent.
    /// `Vec<State>` works as a stack: `push` to remember, `pop` to undo.
    history: Vec<State>,
}

impl PageList {
    pub fn new(page_count: usize) -> Self {
        Self {
            state: State {
                kept: (0..page_count).collect(),
                rotations: vec![Rotation::NONE; page_count],
            },
            history: Vec::new(),
        }
    }

    pub fn kept(&self) -> &[usize] {
        &self.state.kept
    }

    /// The kept pages in order, each with its turn.
    pub fn kept_pages(&self) -> Vec<KeptPage> {
        self.state
            .kept
            .iter()
            .map(|&index| KeptPage {
                index,
                rotation: self.rotation(index),
            })
            .collect()
    }

    /// The turn of each kept page, in the order shown.
    pub fn kept_rotations(&self) -> Vec<Rotation> {
        self.state
            .kept
            .iter()
            .map(|&index| self.rotation(index))
            .collect()
    }

    /// The owner's turn of page `index` of the original file.
    pub fn rotation(&self, index: usize) -> Rotation {
        self.state.rotations.get(index).copied().unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.state.kept.len()
    }

    pub fn is_empty(&self) -> bool {
        self.state.kept.is_empty()
    }

    /// Removes the pages at `positions` (indices into `kept()`; out-of-range are ignored).
    /// `&mut self` means this method may change the list; `&self` methods only read it.
    pub fn delete(&mut self, positions: &[usize]) -> Result<(), PdfError> {
        let remaining: Vec<usize> = self
            .state
            .kept
            .iter()
            .enumerate()
            .filter(|(pos, _)| !positions.contains(pos))
            .map(|(_, &page)| page)
            .collect();
        if remaining.len() == self.state.kept.len() {
            return Ok(()); // nothing to delete, so nothing to undo either
        }
        if remaining.is_empty() {
            return Err(PdfError::NoPagesLeft);
        }
        self.remember();
        self.state.kept = remaining;
        Ok(())
    }

    /// Turns the pages at `positions` (indices into `kept()`; out-of-range are
    /// ignored) a quarter turn, clockwise or not. Returns `false` when there
    /// was no such page, so nothing changed.
    pub fn rotate(&mut self, positions: &[usize], clockwise: bool) -> bool {
        let pages: Vec<usize> = positions
            .iter()
            .filter_map(|&position| self.state.kept.get(position).copied())
            .collect();
        if pages.is_empty() {
            return false;
        }
        self.remember();
        for page in pages {
            if let Some(rotation) = self.state.rotations.get_mut(page) {
                *rotation = if clockwise {
                    rotation.clockwise()
                } else {
                    rotation.anticlockwise()
                };
            }
        }
        true
    }

    /// Reverts the last delete or turn. Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        match self.history.pop() {
            Some(previous) => {
                self.state = previous;
                true
            }
            None => false,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// Whether the pages differ from the file: some deleted or turned. Turning
    /// a page right and then left again leaves nothing to save.
    pub fn is_modified(&self) -> bool {
        self.state.kept.len() != self.state.rotations.len()
            || self.state.rotations.iter().any(|&r| r != Rotation::NONE)
    }

    /// Saves the current state for undo, before it changes.
    fn remember(&mut self) {
        self.history.push(self.state.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delete_should_remove_positions() {
        let mut list = PageList::new(5);
        list.delete(&[1, 3]).expect("delete works");
        assert_eq!(list.kept(), [0, 2, 4]);
    }

    #[test]
    fn delete_should_refuse_to_remove_every_page() {
        let mut list = PageList::new(2);
        assert!(matches!(list.delete(&[0, 1]), Err(PdfError::NoPagesLeft)));
        assert_eq!(list.kept(), [0, 1]);
    }

    #[test]
    fn undo_should_restore_previous_state_step_by_step() {
        let mut list = PageList::new(5);
        list.delete(&[0]).expect("first delete");
        list.delete(&[1]).expect("second delete");
        assert_eq!(list.kept(), [1, 3, 4]);
        assert!(list.undo());
        assert_eq!(list.kept(), [1, 2, 3, 4]);
        assert!(list.undo());
        assert_eq!(list.kept(), [0, 1, 2, 3, 4]);
        assert!(!list.undo());
    }

    #[test]
    fn is_modified_should_track_deletions() {
        let mut list = PageList::new(3);
        assert!(!list.is_modified());
        list.delete(&[0]).expect("delete works");
        assert!(list.is_modified());
        assert!(list.undo());
        assert!(!list.is_modified());
    }

    #[test]
    fn rotate_should_turn_the_pages_at_positions() {
        let mut list = PageList::new(4);
        list.delete(&[0]).expect("delete works");
        // Positions 0 and 2 are now original pages 1 and 3.
        assert!(list.rotate(&[0, 2], true));
        assert_eq!(
            list.kept_rotations(),
            [
                Rotation::quarter_turns(1),
                Rotation::NONE,
                Rotation::quarter_turns(1)
            ]
        );
        assert_eq!(list.kept_pages()[2].index, 3);
    }

    #[test]
    fn rotate_should_add_up_turns_both_ways() {
        let mut list = PageList::new(1);
        list.rotate(&[0], false);
        list.rotate(&[0], false);
        list.rotate(&[0], false);
        assert_eq!(list.rotation(0), Rotation::quarter_turns(1));
    }

    #[test]
    fn rotate_should_ignore_unknown_positions() {
        let mut list = PageList::new(2);
        assert!(!list.rotate(&[5], true));
        assert!(!list.can_undo());
    }

    #[test]
    fn undo_should_take_back_a_turn() {
        let mut list = PageList::new(2);
        list.rotate(&[1], true);
        assert!(list.is_modified());
        assert!(list.undo());
        assert_eq!(list.kept_rotations(), [Rotation::NONE; 2]);
        assert!(!list.is_modified());
    }

    #[test]
    fn undo_should_bring_a_deleted_page_back_turned() {
        let mut list = PageList::new(3);
        list.rotate(&[1], true);
        list.delete(&[1]).expect("delete works");
        assert!(list.undo());
        assert_eq!(list.kept_pages()[1].rotation, Rotation::quarter_turns(1));
    }

    #[test]
    fn is_modified_should_clear_when_a_page_is_turned_back() {
        let mut list = PageList::new(2);
        list.rotate(&[0], true);
        list.rotate(&[0], false);
        assert_eq!((list.is_modified(), list.can_undo()), (false, true));
    }
}
