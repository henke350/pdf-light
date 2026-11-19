//! Which pages of the document are kept, with undo. Nothing here touches the file.

use crate::error::PdfError;

/// The pages still in the document, as indices into the original file.
pub struct PageList {
    /// Original 0-based indices, ascending.
    kept: Vec<usize>,
    /// Earlier states of `kept`; the last entry is the most recent.
    /// `Vec<Vec<usize>>` works as a stack: `push` to remember, `pop` to undo.
    history: Vec<Vec<usize>>,
}

impl PageList {
    pub fn new(page_count: usize) -> Self {
        Self {
            kept: (0..page_count).collect(),
            history: Vec::new(),
        }
    }

    pub fn kept(&self) -> &[usize] {
        &self.kept
    }

    pub fn len(&self) -> usize {
        self.kept.len()
    }

    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }

    /// Removes the pages at `positions` (indices into `kept()`; out-of-range are ignored).
    /// `&mut self` means this method may change the list; `&self` methods only read it.
    pub fn delete(&mut self, positions: &[usize]) -> Result<(), PdfError> {
        let remaining: Vec<usize> = self
            .kept
            .iter()
            .enumerate()
            .filter(|(pos, _)| !positions.contains(pos))
            .map(|(_, &page)| page)
            .collect();
        if remaining.len() == self.kept.len() {
            return Ok(()); // nothing to delete, so nothing to undo either
        }
        if remaining.is_empty() {
            return Err(PdfError::NoPagesLeft);
        }
        // `replace` swaps in the new list and hands back the old one for the history.
        let previous = std::mem::replace(&mut self.kept, remaining);
        self.history.push(previous);
        Ok(())
    }

    /// Reverts the last delete. Returns `false` when there is nothing to undo.
    pub fn undo(&mut self) -> bool {
        match self.history.pop() {
            Some(previous) => {
                self.kept = previous;
                true
            }
            None => false,
        }
    }

    pub fn is_modified(&self) -> bool {
        !self.history.is_empty()
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
}
