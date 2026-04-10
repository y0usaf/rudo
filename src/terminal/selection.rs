//! Mouse-based text selection for the terminal grid.
//! Handles forward and backward selections, normalization, and text extraction.

use super::grid::Grid;

/// A point on the terminal grid (column, row).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridPoint {
    pub col: usize,
    pub row: usize,
}

#[allow(dead_code)]
impl GridPoint {
    pub fn new(col: usize, row: usize) -> Self {
        Self { col, row }
    }
}

/// Current state of the selection.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SelectionState {
    /// No active selection.
    None,
    /// User is currently dragging to select.
    Selecting,
    /// Selection is finalized (mouse button released).
    Selected,
}

/// Represents a rectangular text selection on the terminal grid.
pub struct Selection {
    state: SelectionState,
    start: GridPoint,
    end: GridPoint,
}

#[allow(dead_code)]
impl Selection {
    #[inline]
    fn is_forward(start: GridPoint, end: GridPoint) -> bool {
        start.row < end.row || (start.row == end.row && start.col <= end.col)
    }

    /// Create a new empty selection.
    pub fn new() -> Self {
        Self {
            state: SelectionState::None,
            start: GridPoint::new(0, 0),
            end: GridPoint::new(0, 0),
        }
    }

    /// Get the current selection state.
    pub fn state(&self) -> SelectionState {
        self.state
    }

    /// Return a snapshot of the selection state for change detection.
    pub fn snapshot(&self) -> (SelectionState, GridPoint, GridPoint) {
        (self.state, self.start, self.end)
    }

    /// Begin a new selection at the given grid position (e.g., on mouse press).
    pub fn start_selection(&mut self, col: usize, row: usize) {
        self.state = SelectionState::Selecting;
        self.start = GridPoint::new(col, row);
        self.end = GridPoint::new(col, row);
        ensures!(self.state == SelectionState::Selecting);
    }

    /// Update the end point of the selection (e.g., on mouse drag).
    pub fn update_selection(&mut self, col: usize, row: usize) {
        if self.state == SelectionState::Selecting {
            self.end = GridPoint::new(col, row);
        }
    }

    /// Finalize the selection (e.g., on mouse release).
    pub fn finish_selection(&mut self) {
        if self.state == SelectionState::Selecting {
            // Only transition to Selected if start != end (there's actual content)
            if self.start == self.end {
                self.state = SelectionState::None;
            } else {
                self.state = SelectionState::Selected;
            }
        }
    }

    /// Clear the selection entirely.
    pub fn clear(&mut self) {
        self.state = SelectionState::None;
        self.start = GridPoint::new(0, 0);
        self.end = GridPoint::new(0, 0);
        ensures!(self.state == SelectionState::None);
    }

    /// Returns true if there is an active selection (either selecting or selected).
    pub fn has_selection(&self) -> bool {
        matches!(
            self.state,
            SelectionState::Selecting | SelectionState::Selected
        )
    }

    /// Get the normalized start and end points, ensuring start comes before end
    /// in reading order (top-to-bottom, left-to-right).
    pub fn normalized(&self) -> (GridPoint, GridPoint) {
        let s = self.start;
        let e = self.end;

        if Self::is_forward(s, e) {
            (s, e)
        } else {
            (e, s)
        }
    }

    /// Return the selected column range for a given row, or `None` if the row
    /// has no selection.  Normalizes once so the caller can avoid per-cell work.
    #[inline]
    pub fn row_range(&self, row: usize) -> Option<(usize, usize)> {
        if !self.has_selection() {
            return None;
        }
        let (start, end) = self.normalized();
        if row < start.row || row > end.row {
            return None;
        }
        if start.row == end.row {
            Some((start.col, end.col))
        } else if row == start.row {
            Some((start.col, usize::MAX))
        } else if row == end.row {
            Some((0, end.col))
        } else {
            Some((0, usize::MAX))
        }
    }

    /// Check if a cell at (col, row) falls within the current selection.
    /// Handles both forward and backward selections via normalization.
    pub fn is_selected(&self, col: usize, row: usize) -> bool {
        self.row_range(row)
            .map(|(start, end)| col >= start && col <= end)
            .unwrap_or(false)
    }

    /// Extract the selected text from the terminal grid.
    /// Iterates rows from start to end in the visible viewport, extracting cell
    /// characters. Trailing spaces on each row are trimmed, and rows are joined
    /// with newlines.
    pub fn selected_text(&self, grid: &Grid) -> String {
        if !self.has_selection() || grid.rows() == 0 || grid.cols() == 0 {
            return String::new();
        }

        let (start, end) = self.normalized();
        let grid_rows = grid.rows();
        let grid_cols = grid.cols();
        if start.row >= grid_rows {
            return String::new();
        }

        let mut result = String::new();
        let mut wrote_row = false;
        let wide_spacer = super::cell::CellFlags::WIDE_SPACER;

        for row in start.row..=end.row.min(grid_rows.saturating_sub(1)) {
            let Some((col_start, col_end)) = self.row_range(row) else {
                continue;
            };
            let col_start = col_start.min(grid_cols);
            let col_end = col_end.min(grid_cols.saturating_sub(1));
            if col_start > col_end {
                continue;
            }

            let row_cells = &grid.view_row_cells(row)[col_start..=col_end];
            let mut trimmed_len = 0usize;
            let mut row_len = 0usize;

            for cell in row_cells {
                if cell.flags.contains(wide_spacer) {
                    continue;
                }

                let ch = cell.character();
                row_len += ch.len_utf8();
                if ch != ' ' {
                    trimmed_len = row_len;
                }
            }

            if wrote_row {
                result.push('\n');
            }
            if trimmed_len != 0 {
                result.reserve(trimmed_len);
                let mut remaining = trimmed_len;

                for cell in row_cells {
                    if cell.flags.contains(wide_spacer) {
                        continue;
                    }

                    let ch = cell.character();
                    let ch_len = ch.len_utf8();
                    if ch_len > remaining {
                        break;
                    }

                    result.push(ch);
                    remaining -= ch_len;
                    if remaining == 0 {
                        break;
                    }
                }
            }
            wrote_row = true;
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_selection() {
        let sel = Selection::new();
        assert_eq!(sel.state, SelectionState::None);
        assert!(!sel.has_selection());
    }

    #[test]
    fn test_selection_lifecycle() {
        let mut sel = Selection::new();

        sel.start_selection(5, 3);
        assert_eq!(sel.state, SelectionState::Selecting);
        assert_eq!(sel.start, GridPoint::new(5, 3));
        assert!(sel.has_selection());

        sel.update_selection(10, 5);
        assert_eq!(sel.end, GridPoint::new(10, 5));

        sel.finish_selection();
        assert_eq!(sel.state, SelectionState::Selected);
        assert!(sel.has_selection());

        sel.clear();
        assert_eq!(sel.state, SelectionState::None);
        assert!(!sel.has_selection());
    }

    #[test]
    fn test_finish_empty_selection_clears() {
        let mut sel = Selection::new();
        sel.start_selection(5, 3);
        // Don't move the endpoint — start == end
        sel.finish_selection();
        assert_eq!(sel.state, SelectionState::None);
    }

    #[test]
    fn test_normalized_forward() {
        let mut sel = Selection::new();
        sel.start_selection(2, 1);
        sel.update_selection(8, 3);

        let (s, e) = sel.normalized();
        assert_eq!(s, GridPoint::new(2, 1));
        assert_eq!(e, GridPoint::new(8, 3));
    }

    #[test]
    fn test_normalized_backward() {
        let mut sel = Selection::new();
        sel.start_selection(8, 3);
        sel.update_selection(2, 1);

        let (s, e) = sel.normalized();
        assert_eq!(s, GridPoint::new(2, 1));
        assert_eq!(e, GridPoint::new(8, 3));
    }

    #[test]
    fn test_normalized_same_row_backward() {
        let mut sel = Selection::new();
        sel.start_selection(10, 5);
        sel.update_selection(3, 5);

        let (s, e) = sel.normalized();
        assert_eq!(s, GridPoint::new(3, 5));
        assert_eq!(e, GridPoint::new(10, 5));
    }

    #[test]
    fn test_is_selected_single_line() {
        let mut sel = Selection::new();
        sel.start_selection(3, 2);
        sel.update_selection(7, 2);

        assert!(!sel.is_selected(2, 2));
        assert!(sel.is_selected(3, 2));
        assert!(sel.is_selected(5, 2));
        assert!(sel.is_selected(7, 2));
        assert!(!sel.is_selected(8, 2));
        assert!(!sel.is_selected(5, 1));
        assert!(!sel.is_selected(5, 3));
    }

    #[test]
    fn test_is_selected_multi_line() {
        let mut sel = Selection::new();
        sel.start_selection(5, 1);
        sel.update_selection(3, 3);

        // Row 1: col >= 5
        assert!(!sel.is_selected(4, 1));
        assert!(sel.is_selected(5, 1));
        assert!(sel.is_selected(79, 1));

        // Row 2: entire row
        assert!(sel.is_selected(0, 2));
        assert!(sel.is_selected(40, 2));
        assert!(sel.is_selected(79, 2));

        // Row 3: col <= 3
        assert!(sel.is_selected(0, 3));
        assert!(sel.is_selected(3, 3));
        assert!(!sel.is_selected(4, 3));

        // Outside rows
        assert!(!sel.is_selected(5, 0));
        assert!(!sel.is_selected(3, 4));
    }

    #[test]
    fn test_is_selected_backward_selection() {
        let mut sel = Selection::new();
        sel.start_selection(3, 3);
        sel.update_selection(5, 1);

        // Should behave identically to forward selection after normalization
        assert!(sel.is_selected(5, 1));
        assert!(sel.is_selected(79, 1));
        assert!(sel.is_selected(0, 2));
        assert!(sel.is_selected(3, 3));
        assert!(!sel.is_selected(4, 3));
    }

    #[test]
    fn test_is_selected_no_selection() {
        let sel = Selection::new();
        assert!(!sel.is_selected(0, 0));
        assert!(!sel.is_selected(5, 5));
    }

    #[test]
    fn test_selected_text_single_row() {
        let mut grid = Grid::new(80, 24);
        // Write "Hello, World!" starting at col 0, row 0
        let text = "Hello, World!";
        for (i, ch) in text.chars().enumerate() {
            let cell = grid.cell_mut(i, 0);
            cell.ch = ch as u32;
        }

        let mut sel = Selection::new();
        sel.start_selection(0, 0);
        sel.update_selection(12, 0);

        let result = sel.selected_text(&grid);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_selected_text_multi_row() {
        let mut grid = Grid::new(80, 24);

        let line0 = "First line";
        for (i, ch) in line0.chars().enumerate() {
            grid.cell_mut(i, 0).ch = ch as u32;
        }

        let line1 = "Second line";
        for (i, ch) in line1.chars().enumerate() {
            grid.cell_mut(i, 1).ch = ch as u32;
        }

        let mut sel = Selection::new();
        sel.start_selection(0, 0);
        sel.update_selection(10, 1);

        let result = sel.selected_text(&grid);
        assert_eq!(result, "First line\nSecond line");
    }

    #[test]
    fn test_update_ignored_when_not_selecting() {
        let mut sel = Selection::new();
        sel.update_selection(5, 5);
        // Should remain at default since state is None
        assert_eq!(sel.end, GridPoint::new(0, 0));
    }

    #[test]
    fn test_selected_text_uses_visible_viewport_rows() {
        let mut grid = Grid::new(5, 3);

        for (col, ch) in "one".chars().enumerate() {
            grid.cell_mut(col, 0).ch = ch as u32;
        }

        grid.set_cursor(0, 2);
        grid.linefeed();
        for (col, ch) in "two".chars().enumerate() {
            grid.cell_mut(col, 2).ch = ch as u32;
        }

        grid.set_cursor(0, 2);
        grid.linefeed();
        for (col, ch) in "tri".chars().enumerate() {
            grid.cell_mut(col, 2).ch = ch as u32;
        }

        grid.set_cursor(0, 2);
        grid.linefeed();
        for (col, ch) in "for".chars().enumerate() {
            grid.cell_mut(col, 2).ch = ch as u32;
        }

        let mut sel = Selection::new();
        sel.start_selection(0, 0);
        sel.update_selection(2, 2);

        let result = sel.selected_text(&grid);
        assert_eq!(result, "two\ntri\nfor");
    }

    #[test]
    fn test_selected_text_skips_wide_spacers_and_trims_trailing_spaces() {
        use crate::terminal::cell::CellFlags;

        let mut grid = Grid::new(6, 1);
        grid.cell_mut(0, 0).ch = '中' as u32;
        grid.cell_mut(1, 0).flags.insert(CellFlags::WIDE_SPACER);
        grid.cell_mut(2, 0).ch = ' ' as u32;
        grid.cell_mut(3, 0).ch = 'x' as u32;
        grid.cell_mut(4, 0).ch = ' ' as u32;
        grid.cell_mut(5, 0).ch = ' ' as u32;

        let mut sel = Selection::new();
        sel.start_selection(0, 0);
        sel.update_selection(5, 0);

        assert_eq!(sel.selected_text(&grid), "中 x");
    }

    #[test]
    fn test_selected_text_ignores_columns_past_visible_width() {
        let mut grid = Grid::new(3, 1);
        for (col, ch) in "abc".chars().enumerate() {
            grid.cell_mut(col, 0).ch = ch as u32;
        }

        let mut sel = Selection::new();
        sel.start_selection(5, 0);
        sel.update_selection(8, 0);

        assert_eq!(sel.selected_text(&grid), "");
    }
}
