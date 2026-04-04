use std::ops::Range;

use crate::terminal::cell::TerminalCell;

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalScreen {
    cols: usize,
    rows: usize,
    cells: Vec<TerminalCell>,
}

impl TerminalScreen {
    pub fn new(cols: usize, rows: usize) -> Self {
        let cols = cols.max(1);
        let rows = rows.max(1);
        Self { cols, rows, cells: vec![TerminalCell::default(); cols * rows] }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(1);
        let rows = rows.max(1);
        let mut new_screen = Self::new(cols, rows);
        let copy_rows = rows.min(self.rows);
        let copy_cols = cols.min(self.cols);

        for row in 0..copy_rows {
            let Some(source_row) = self.row(row) else {
                continue;
            };
            let Some(target_row) = new_screen.row_mut(row) else {
                continue;
            };
            target_row[..copy_cols].clone_from_slice(&source_row[..copy_cols]);
        }

        *self = new_screen;
    }

    pub fn get(&self, col: usize, row: usize) -> Option<&TerminalCell> {
        self.index(col, row).and_then(|index| self.cells.get(index))
    }

    pub fn get_mut(&mut self, col: usize, row: usize) -> Option<&mut TerminalCell> {
        let index = self.index(col, row)?;
        self.cells.get_mut(index)
    }

    pub fn row(&self, row: usize) -> Option<&[TerminalCell]> {
        let range = self.row_range(row)?;
        Some(&self.cells[range])
    }

    pub fn row_mut(&mut self, row: usize) -> Option<&mut [TerminalCell]> {
        let range = self.row_range(row)?;
        Some(&mut self.cells[range])
    }

    pub fn set(&mut self, col: usize, row: usize, cell: TerminalCell) {
        if let Some(index) = self.index(col, row) {
            self.cells[index] = cell;
        }
    }

    pub fn clear(&mut self) {
        self.cells.fill(TerminalCell::default());
    }

    pub fn clear_line(&mut self, row: usize) {
        if let Some(line) = self.row_mut(row) {
            line.fill(TerminalCell::default());
        }
    }

    pub fn erase_chars(&mut self, row: usize, start_col: usize, count: usize) {
        let cols = self.cols;
        let end = start_col.saturating_add(count).min(cols);
        if let Some(line) = self.row_mut(row) {
            line[start_col.min(cols)..end].fill(TerminalCell::default());
        }
    }

    pub fn delete_chars(&mut self, row: usize, start_col: usize, count: usize) {
        if row >= self.rows || start_col >= self.cols {
            return;
        }

        let count = count.max(1).min(self.cols - start_col);
        let line = self.row_mut(row).expect("validated row must exist");
        let tail = &mut line[start_col..];
        tail.rotate_left(count);
        let fill_start = tail.len() - count;
        tail[fill_start..].fill(TerminalCell::default());
    }

    pub fn insert_blank_chars(&mut self, row: usize, start_col: usize, count: usize) {
        if row >= self.rows || start_col >= self.cols {
            return;
        }

        let count = count.max(1).min(self.cols - start_col);
        let line = self.row_mut(row).expect("validated row must exist");
        let tail = &mut line[start_col..];
        tail.rotate_right(count);
        tail[..count].fill(TerminalCell::default());
    }

    pub fn insert_lines(&mut self, row: usize, count: usize) {
        self.insert_lines_in_region(row, self.rows, count);
    }

    pub fn insert_lines_in_region(&mut self, row: usize, bottom_exclusive: usize, count: usize) {
        if row >= self.rows || row >= bottom_exclusive || bottom_exclusive > self.rows {
            return;
        }

        let count = count.max(1).min(bottom_exclusive - row);
        let cols = self.cols;
        let region = self.region_range(row, bottom_exclusive);
        self.cells[region].rotate_right(count * cols);
        for clear_row in row..row + count {
            self.clear_line(clear_row);
        }
    }

    pub fn delete_lines(&mut self, row: usize, count: usize) {
        self.delete_lines_in_region(row, self.rows, count);
    }

    pub fn delete_lines_in_region(&mut self, row: usize, bottom_exclusive: usize, count: usize) {
        if row >= self.rows || row >= bottom_exclusive || bottom_exclusive > self.rows {
            return;
        }

        let count = count.max(1).min(bottom_exclusive - row);
        let cols = self.cols;
        let region = self.region_range(row, bottom_exclusive);
        self.cells[region].rotate_left(count * cols);
        for clear_row in bottom_exclusive - count..bottom_exclusive {
            self.clear_line(clear_row);
        }
    }

    pub fn clear_line_from(&mut self, row: usize, start_col: usize) {
        let cols = self.cols;
        if let Some(line) = self.row_mut(row) {
            line[start_col.min(cols)..].fill(TerminalCell::default());
        }
    }

    pub fn clear_line_to(&mut self, row: usize, end_col: usize) {
        let cols = self.cols;
        if cols == 0 {
            return;
        }

        if let Some(line) = self.row_mut(row) {
            line[..=end_col.min(cols.saturating_sub(1))].fill(TerminalCell::default());
        }
    }

    pub fn clear_from_cursor(&mut self, col: usize, row: usize) {
        self.clear_line_from(row, col);
        for next_row in row.saturating_add(1)..self.rows {
            self.clear_line(next_row);
        }
    }

    pub fn clear_to_cursor(&mut self, col: usize, row: usize) {
        for prior_row in 0..row {
            self.clear_line(prior_row);
        }
        self.clear_line_to(row, col);
    }

    pub fn scroll_up(&mut self, lines: usize) -> Vec<Vec<TerminalCell>> {
        self.scroll_up_in_region(0, self.rows, lines)
    }

    pub fn scroll_up_in_region(
        &mut self,
        top: usize,
        bottom_exclusive: usize,
        lines: usize,
    ) -> Vec<Vec<TerminalCell>> {
        if top >= bottom_exclusive || bottom_exclusive > self.rows {
            return Vec::new();
        }

        let height = bottom_exclusive - top;
        let lines = lines.min(height);
        let removed = (top..top + lines).map(|row| self.row_cells(row)).collect();

        let cols = self.cols;
        let region = self.region_range(top, bottom_exclusive);
        self.cells[region].rotate_left(lines * cols);
        for clear_row in bottom_exclusive - lines..bottom_exclusive {
            self.clear_line(clear_row);
        }

        removed
    }

    pub fn scroll_down_in_region(&mut self, top: usize, bottom_exclusive: usize, lines: usize) {
        if top >= bottom_exclusive || bottom_exclusive > self.rows {
            return;
        }

        let height = bottom_exclusive - top;
        let lines = lines.min(height);
        let cols = self.cols;
        let region = self.region_range(top, bottom_exclusive);
        self.cells[region].rotate_right(lines * cols);
        for clear_row in top..top + lines {
            self.clear_line(clear_row);
        }
    }

    pub fn row_cells(&self, row: usize) -> Vec<TerminalCell> {
        self.row(row).map_or_else(Vec::new, ToOwned::to_owned)
    }

    pub fn row_text(&self, row: usize) -> String {
        self.row(row).into_iter().flatten().map(|cell| cell.text.as_str()).collect::<String>()
    }

    fn index(&self, col: usize, row: usize) -> Option<usize> {
        if col < self.cols && row < self.rows { Some(row * self.cols + col) } else { None }
    }

    fn row_range(&self, row: usize) -> Option<Range<usize>> {
        if row >= self.rows {
            return None;
        }

        let start = row * self.cols;
        Some(start..start + self.cols)
    }

    fn region_range(&self, top: usize, bottom_exclusive: usize) -> Range<usize> {
        (top * self.cols)..(bottom_exclusive * self.cols)
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalScreen;
    use crate::terminal::cell::{CellWidth, TerminalCell};

    #[test]
    fn scroll_up_returns_removed_rows() {
        let mut screen = TerminalScreen::new(3, 2);
        screen.set(0, 0, TerminalCell::occupied("a", None, CellWidth::Single));
        screen.set(0, 1, TerminalCell::occupied("b", None, CellWidth::Single));

        let removed = screen.scroll_up(1);

        assert_eq!(removed[0][0].text, "a");
        assert_eq!(screen.get(0, 0).unwrap().text, "b");
        assert_eq!(screen.get(0, 1).unwrap().text, " ");
    }

    #[test]
    fn delete_chars_shifts_tail_once() {
        let mut screen = TerminalScreen::new(6, 1);
        for (col, ch) in "abcdef".chars().enumerate() {
            screen.set(col, 0, TerminalCell::occupied(ch.to_string(), None, CellWidth::Single));
        }

        screen.delete_chars(0, 1, 2);

        assert_eq!(screen.row_text(0), "adef  ");
    }

    #[test]
    fn insert_lines_rotates_region() {
        let mut screen = TerminalScreen::new(2, 4);
        for row in 0..4 {
            let text = char::from_u32(b'a' as u32 + row as u32).unwrap().to_string();
            screen.set(0, row, TerminalCell::occupied(text, None, CellWidth::Single));
        }

        screen.insert_lines_in_region(1, 4, 2);

        assert_eq!(screen.row_text(0), "a ");
        assert_eq!(screen.row_text(1), "  ");
        assert_eq!(screen.row_text(2), "  ");
        assert_eq!(screen.row_text(3), "b ");
    }
}
