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
            for col in 0..copy_cols {
                let cell = self.get(col, row).cloned().unwrap_or_default();
                new_screen.set(col, row, cell);
            }
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

    pub fn set(&mut self, col: usize, row: usize, cell: TerminalCell) {
        if let Some(index) = self.index(col, row) {
            self.cells[index] = cell;
        }
    }

    pub fn clear(&mut self) {
        self.cells.fill(TerminalCell::default());
    }

    pub fn clear_line(&mut self, row: usize) {
        for col in 0..self.cols {
            self.set(col, row, TerminalCell::default());
        }
    }

    pub fn erase_chars(&mut self, row: usize, start_col: usize, count: usize) {
        let end = start_col.saturating_add(count).min(self.cols);
        for col in start_col.min(self.cols)..end {
            self.set(col, row, TerminalCell::default());
        }
    }

    pub fn delete_chars(&mut self, row: usize, start_col: usize, count: usize) {
        if row >= self.rows || start_col >= self.cols {
            return;
        }

        let count = count.max(1).min(self.cols - start_col);
        for col in start_col..self.cols {
            let src = col + count;
            let cell = if src < self.cols {
                self.get(src, row).cloned().unwrap_or_default()
            } else {
                TerminalCell::default()
            };
            self.set(col, row, cell);
        }
    }

    pub fn insert_blank_chars(&mut self, row: usize, start_col: usize, count: usize) {
        if row >= self.rows || start_col >= self.cols {
            return;
        }

        let count = count.max(1).min(self.cols - start_col);
        for col in (start_col..self.cols).rev() {
            let src = col.checked_sub(count);
            let cell = if let Some(src) = src {
                if src >= start_col {
                    self.get(src, row).cloned().unwrap_or_default()
                } else {
                    TerminalCell::default()
                }
            } else {
                TerminalCell::default()
            };
            self.set(col, row, cell);
        }
    }

    pub fn insert_lines(&mut self, row: usize, count: usize) {
        self.insert_lines_in_region(row, self.rows, count);
    }

    pub fn insert_lines_in_region(&mut self, row: usize, bottom_exclusive: usize, count: usize) {
        if row >= self.rows || row >= bottom_exclusive || bottom_exclusive > self.rows {
            return;
        }

        let count = count.max(1).min(bottom_exclusive - row);
        for dest_row in (row..bottom_exclusive).rev() {
            let src_row = dest_row.checked_sub(count);
            for col in 0..self.cols {
                let cell = if let Some(src_row) = src_row {
                    if src_row >= row {
                        self.get(col, src_row).cloned().unwrap_or_default()
                    } else {
                        TerminalCell::default()
                    }
                } else {
                    TerminalCell::default()
                };
                self.set(col, dest_row, cell);
            }
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
        for dest_row in row..bottom_exclusive {
            let src_row = dest_row + count;
            for col in 0..self.cols {
                let cell = if src_row < bottom_exclusive {
                    self.get(col, src_row).cloned().unwrap_or_default()
                } else {
                    TerminalCell::default()
                };
                self.set(col, dest_row, cell);
            }
        }
    }

    pub fn clear_line_from(&mut self, row: usize, start_col: usize) {
        for col in start_col.min(self.cols)..self.cols {
            self.set(col, row, TerminalCell::default());
        }
    }

    pub fn clear_line_to(&mut self, row: usize, end_col: usize) {
        if self.cols == 0 {
            return;
        }

        for col in 0..=end_col.min(self.cols.saturating_sub(1)) {
            self.set(col, row, TerminalCell::default());
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
        let mut removed = Vec::with_capacity(lines);

        for _ in 0..lines {
            removed.push(self.row_cells(top));

            for row in top + 1..bottom_exclusive {
                for col in 0..self.cols {
                    let cell = self.get(col, row).cloned().unwrap_or_default();
                    self.set(col, row - 1, cell);
                }
            }

            self.clear_line(bottom_exclusive - 1);
        }

        removed
    }

    pub fn scroll_down_in_region(&mut self, top: usize, bottom_exclusive: usize, lines: usize) {
        if top >= bottom_exclusive || bottom_exclusive > self.rows {
            return;
        }

        let height = bottom_exclusive - top;
        let lines = lines.min(height);

        for _ in 0..lines {
            for row in (top..bottom_exclusive - 1).rev() {
                for col in 0..self.cols {
                    let cell = self.get(col, row).cloned().unwrap_or_default();
                    self.set(col, row + 1, cell);
                }
            }

            self.clear_line(top);
        }
    }

    pub fn row_cells(&self, row: usize) -> Vec<TerminalCell> {
        (0..self.cols).map(|col| self.get(col, row).cloned().unwrap_or_default()).collect()
    }

    pub fn row_text(&self, row: usize) -> String {
        (0..self.cols)
            .filter_map(|col| self.get(col, row))
            .map(|cell| cell.text.clone())
            .collect::<String>()
    }

    fn index(&self, col: usize, row: usize) -> Option<usize> {
        if col < self.cols && row < self.rows { Some(row * self.cols + col) } else { None }
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
}
