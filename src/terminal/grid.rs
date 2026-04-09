//! Terminal grid with circular buffer, scrollback, alternate screen, and scroll regions.

use super::cell::Cell;

pub const DEFAULT_TAB_WIDTH: usize = 8;

// ─── Row ─────────────────────────────────────────────────────────────────────

/// A single row of terminal cells.
#[derive(Debug, Clone)]
pub struct Row {
    cells: Vec<Cell>,
    dirty: bool,
}

impl Row {
    pub fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::default(); cols],
            dirty: true,
        }
    }

    pub fn clear(&mut self) {
        self.clear_with(Cell::default());
    }

    pub fn clear_with(&mut self, blank: Cell) {
        let mut blank = blank;
        blank.mark_dirty();
        self.cells.fill(blank);
        self.dirty = true;
    }

    #[inline]
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    #[inline]
    #[allow(dead_code)]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    #[inline]
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
        for cell in &mut self.cells {
            cell.clear_dirty();
        }
    }
}

// ─── CursorState ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub struct CursorState {
    pub col: usize,
    pub row: usize,
    pub visible: bool,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            col: 0,
            row: 0,
            visible: true,
        }
    }
}

// ─── SavedGrid (alternate screen) ────────────────────────────────────────────

#[derive(Debug, Clone)]
#[allow(dead_code)]
struct SavedGrid {
    rows: Vec<Row>,
    num_rows: usize,
    offset: usize,
    cursor: CursorState,
    saved_cursor: CursorState,
    scroll_top: usize,
    scroll_bottom: usize,
    scrollback_lines: usize,
    max_scrollback: usize,
}

// ─── Grid ────────────────────────────────────────────────────────────────────

/// Terminal grid backed by a circular buffer with power-of-2 row count.
#[allow(dead_code)]
pub struct Grid {
    cursor: CursorState,

    /// Circular buffer of rows. Length is always a power of 2.
    rows: Vec<Row>,
    /// Power-of-2 capacity (== rows.len()).
    num_rows: usize,
    /// Start offset into the circular buffer (top of visible screen).
    offset: usize,
    /// Number of visible screen rows.
    screen_rows: usize,
    /// Number of visible screen columns.
    screen_cols: usize,

    /// Lines that have scrolled off the top into scrollback.
    scrollback_lines: usize,
    /// Maximum retained scrollback lines.
    max_scrollback: usize,

    /// Scroll region top (0-based, inclusive).
    scroll_top: usize,
    /// Scroll region bottom (0-based, inclusive).
    scroll_bottom: usize,

    /// Saved cursor for DECSC / DECRC.
    saved_cursor: CursorState,

    /// Saved main-screen grid for alternate screen switching.
    saved_grid: Option<Box<SavedGrid>>,

    /// Whether we are currently on the alternate screen.
    alternate_active: bool,

    /// View offset for scrolling back through history (0 = current, >0 = looking at history).
    view_offset: usize,
}

/// Round up to the next power of 2 (minimum 16).
fn next_power_of_two(n: usize) -> usize {
    let min = 16usize;
    let v = n.max(min);
    v.next_power_of_two()
}

#[inline]
fn copy_row_prefix(dst: &mut Row, src: &Row, copy_cols: usize) {
    dst.cells[..copy_cols].copy_from_slice(&src.cells[..copy_cols]);
    dst.dirty = true;
}

fn mark_rows_dirty(rows: &mut [Row], top: usize, bottom: usize, offset: usize, num_rows: usize) {
    if top > bottom {
        return;
    }

    for r in top..=bottom {
        let idx = (offset + r) & (num_rows - 1);
        rows[idx].dirty = true;
    }
}

#[derive(Clone, Copy)]
struct ResizeRowStoreInput<'a> {
    rows: &'a [Row],
    num_rows: usize,
    offset: usize,
    screen_cols: usize,
    screen_rows: usize,
    scrollback_lines: usize,
    max_scrollback: usize,
    cursor_row: usize,
    new_cols: usize,
    new_rows: usize,
    preserve_scrollback: bool,
}

fn row_capacity(screen_rows: usize, max_scrollback: usize) -> usize {
    next_power_of_two(screen_rows.saturating_add(max_scrollback))
}

fn resize_row_store(input: ResizeRowStoreInput<'_>) -> (Vec<Row>, usize, usize, usize, usize) {
    let new_num_rows = row_capacity(input.new_rows, input.max_scrollback);
    let mut new_buf = vec![Row::new(input.new_cols); new_num_rows];

    let total_history = input.screen_rows.saturating_add(input.scrollback_lines);
    let cursor_row = input.cursor_row.min(input.screen_rows.saturating_sub(1));

    // Total content lines from top of scrollback through the cursor row (inclusive).
    let total_content = input
        .scrollback_lines
        .saturating_add(cursor_row)
        .saturating_add(1);

    let new_anchor = if total_content <= input.new_rows {
        // All content fits in the new screen — place it at the top, no blank padding.
        cursor_row
    } else {
        // Screen is full (or overflowing): keep cursor at the same distance from the bottom.
        let rows_at_and_below = input.screen_rows.saturating_sub(cursor_row);
        input.new_rows.saturating_sub(rows_at_and_below)
    };
    let anchor_logical = input.scrollback_lines + cursor_row;
    let window_start = anchor_logical as isize - new_anchor as isize;
    let blank_top = if window_start < 0 {
        (-window_start) as usize
    } else {
        0
    };
    let visible_src_start = window_start.max(0) as usize;
    let max_scrollback = usize::from(input.preserve_scrollback) * input.max_scrollback;
    let new_scrollback = visible_src_start.min(max_scrollback);
    let scrollback_src_start = visible_src_start.saturating_sub(new_scrollback);
    let copy_cols = input.screen_cols.min(input.new_cols);
    let history_start = input.offset.wrapping_sub(input.scrollback_lines) & (input.num_rows - 1);

    for (i, dst_row) in new_buf.iter_mut().enumerate().take(new_scrollback) {
        let src_logical = scrollback_src_start + i;
        if src_logical >= total_history {
            break;
        }
        let src_idx = (history_start + src_logical) & (input.num_rows - 1);
        copy_row_prefix(dst_row, &input.rows[src_idx], copy_cols);
    }

    let visible_slots = input.new_rows.saturating_sub(blank_top);
    let visible_copy = total_history
        .saturating_sub(visible_src_start)
        .min(visible_slots);
    for i in 0..visible_copy {
        let src_logical = visible_src_start + i;
        let src_idx = (history_start + src_logical) & (input.num_rows - 1);
        let dst_idx = new_scrollback + blank_top + i;
        copy_row_prefix(&mut new_buf[dst_idx], &input.rows[src_idx], copy_cols);
    }

    (
        new_buf,
        new_num_rows,
        new_scrollback,
        new_scrollback,
        new_anchor.min(input.new_rows.saturating_sub(1)),
    )
}

#[allow(dead_code)]
impl Grid {
    // ── Construction ─────────────────────────────────────────────────────

    pub fn new(cols: usize, rows: usize) -> Self {
        let screen_rows = rows.max(1);
        Self::with_scrollback(cols, screen_rows, screen_rows.saturating_mul(3))
    }

    pub fn with_scrollback(cols: usize, rows: usize, max_scrollback: usize) -> Self {
        let screen_rows = rows.max(1);
        let screen_cols = cols.max(1);
        let num_rows = row_capacity(screen_rows, max_scrollback);
        let row_buf = vec![Row::new(screen_cols); num_rows];
        Self {
            cursor: CursorState::default(),
            rows: row_buf,
            num_rows,
            offset: 0,
            screen_rows,
            screen_cols,
            scrollback_lines: 0,
            max_scrollback,
            scroll_top: 0,
            scroll_bottom: screen_rows.saturating_sub(1),
            saved_cursor: CursorState::default(),
            saved_grid: None,
            alternate_active: false,
            view_offset: 0,
        }
    }

    // ── Dimensions ───────────────────────────────────────────────────────

    #[inline]
    pub fn cols(&self) -> usize {
        self.screen_cols
    }

    #[inline]
    pub fn rows(&self) -> usize {
        self.screen_rows
    }

    #[inline]
    pub fn scroll_region(&self) -> (usize, usize) {
        (self.scroll_top, self.scroll_bottom)
    }

    // ── Internal helpers ─────────────────────────────────────────────────

    /// Map a visible-screen row index to the circular buffer index.
    #[inline]
    fn abs_row(&self, row: usize) -> usize {
        (self.offset + row) & (self.num_rows - 1)
    }

    /// Clamp a column value to the valid range.
    #[inline]
    fn clamp_col(&self, col: usize) -> usize {
        col.min(self.screen_cols.saturating_sub(1))
    }

    #[inline]
    fn view_abs_row(&self, row: usize) -> usize {
        (self.offset.wrapping_sub(self.view_offset) + row) & (self.num_rows - 1)
    }

    /// Clamp a row value to the valid range.
    #[inline]
    fn clamp_row(&self, row: usize) -> usize {
        row.min(self.screen_rows.saturating_sub(1))
    }

    // ── Row / Cell access ────────────────────────────────────────────────

    /// Get an immutable reference to a cell.
    #[inline]
    pub fn cell(&self, col: usize, row: usize) -> &Cell {
        let idx = self.abs_row(row);
        debug_assert!(idx < self.rows.len());
        debug_assert!(col < self.rows[idx].cells.len());
        &self.rows[idx].cells[col]
    }

    /// Get a mutable reference to a cell.
    #[inline]
    pub fn cell_mut(&mut self, col: usize, row: usize) -> &mut Cell {
        let idx = self.abs_row(row);
        debug_assert!(idx < self.rows.len());
        debug_assert!(col < self.rows[idx].cells.len());
        let cell = &mut self.rows[idx].cells[col];
        cell.mark_dirty();
        cell
    }

    // ── Cursor ───────────────────────────────────────────────────────────

    #[inline]
    pub fn cursor_col(&self) -> usize {
        self.cursor.col
    }

    #[inline]
    pub fn cursor_row(&self) -> usize {
        self.cursor.row
    }

    #[inline]
    pub fn cursor_visible(&self) -> bool {
        self.cursor.visible
    }

    #[inline]
    pub fn set_cursor_col(&mut self, col: usize) {
        self.cursor.col = col;
    }

    #[inline]
    pub fn set_cursor_row(&mut self, row: usize) {
        self.cursor.row = row;
    }

    #[inline]
    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor.visible = visible;
    }

    pub fn set_cursor(&mut self, col: usize, row: usize) {
        self.cursor.col = self.clamp_col(col);
        self.cursor.row = self.clamp_row(row);
    }

    /// Returns cursor position as `(col_f32, row_f32)` suitable for rendering.
    ///
    /// The terminal parser may temporarily place the cursor one cell past the
    /// right edge to represent a wrap-pending state after printing in the last
    /// column. Rendering must clamp that back onto the visible grid, otherwise
    /// cursor drawing can index past the end of the row and panic.
    pub fn cursor_position(&self) -> (f32, f32) {
        (
            self.clamp_col(self.cursor.col) as f32,
            self.clamp_row(self.cursor.row) as f32,
        )
    }

    /// DECSC — save cursor position.
    pub fn save_cursor(&mut self) {
        self.saved_cursor = self.cursor;
    }

    /// DECRC — restore cursor position.
    pub fn restore_cursor(&mut self) {
        self.cursor = self.saved_cursor;
        self.cursor.col = self.clamp_col(self.cursor.col);
        self.cursor.row = self.clamp_row(self.cursor.row);
    }

    // ── Line operations ──────────────────────────────────────────────────

    /// Move cursor to column 0.
    pub fn carriage_return(&mut self) {
        self.cursor.col = 0;
    }

    /// Linefeed: move cursor down one row, scrolling if at the bottom of the scroll region.
    pub fn linefeed_with(&mut self, blank: Cell) {
        if self.cursor.row == self.scroll_bottom {
            self.scroll_up_with(1, blank);
        } else if self.cursor.row < self.screen_rows - 1 {
            self.cursor.row += 1;
        }
    }

    pub fn linefeed(&mut self) {
        self.linefeed_with(Cell::default());
    }

    /// Reverse index (RI / ESC M): move cursor up one row, scrolling down if at the
    /// top of the scroll region.
    pub fn reverse_index_with(&mut self, blank: Cell) {
        if self.cursor.row == self.scroll_top {
            self.scroll_down_with(1, blank);
        } else if self.cursor.row > 0 {
            self.cursor.row -= 1;
        }
    }

    pub fn reverse_index(&mut self) {
        self.reverse_index_with(Cell::default());
    }

    // ── Scrolling ────────────────────────────────────────────────────────

    /// Scroll the scroll region up by `lines` lines. New blank lines appear at the bottom
    /// of the scroll region.
    pub fn scroll_up_with(&mut self, lines: usize, blank: Cell) {
        let top = self.scroll_top;
        let bot = self.scroll_bottom;
        let region_size = bot - top + 1;
        let lines = lines.min(region_size);
        if lines == 0 {
            return;
        }

        if top == 0 && bot == self.screen_rows - 1 && !self.alternate_active {
            // Full-screen scroll: advance offset (old top rows become scrollback).
            for _ in 0..lines {
                self.offset = (self.offset + 1) & (self.num_rows - 1);
                self.scrollback_lines = (self.scrollback_lines + 1).min(self.max_scrollback);
                // Clear the new bottom row.
                let bottom_abs = self.abs_row(self.screen_rows - 1);
                self.rows[bottom_abs].clear_with(blank);
            }
        } else {
            // Scroll region: use rotate_left on the physical slice if contiguous,
            // otherwise fall back to swap-based rotation.
            let abs_top = self.abs_row(top);
            let abs_bot = self.abs_row(bot);

            if abs_top <= abs_bot {
                // Region is contiguous in the circular buffer — use slice rotate.
                self.rows[abs_top..=abs_bot].rotate_left(lines);
            } else {
                // Region wraps around the circular buffer — swap one-by-one per line.
                for _ in 0..lines {
                    for r in top..bot {
                        let src = self.abs_row(r + 1);
                        let dst = self.abs_row(r);
                        if src != dst {
                            self.rows.swap(src, dst);
                        }
                    }
                }
            }
            // Clear the new bottom `lines` rows of the region.
            for i in 0..lines {
                let r = bot - (lines - 1) + i;
                let idx = self.abs_row(r);
                self.rows[idx].clear_with(blank);
            }
        }
        mark_rows_dirty(&mut self.rows, top, bot, self.offset, self.num_rows);
    }

    /// Scroll the scroll region down by `lines` lines. New blank lines appear at the top
    /// of the scroll region.
    pub fn scroll_down_with(&mut self, lines: usize, blank: Cell) {
        let top = self.scroll_top;
        let bot = self.scroll_bottom;
        let region_size = bot - top + 1;
        let lines = lines.min(region_size);
        if lines == 0 {
            return;
        }

        let abs_top = self.abs_row(top);
        let abs_bot = self.abs_row(bot);

        if abs_top <= abs_bot {
            // Region is contiguous — use slice rotate.
            self.rows[abs_top..=abs_bot].rotate_right(lines);
        } else {
            // Region wraps — swap one-by-one per line.
            for _ in 0..lines {
                for r in (top..bot).rev() {
                    let src = self.abs_row(r);
                    let dst = self.abs_row(r + 1);
                    if src != dst {
                        self.rows.swap(src, dst);
                    }
                }
            }
        }
        // Clear the new top `lines` rows of the region.
        for i in 0..lines {
            let idx = self.abs_row(top + i);
            self.rows[idx].clear_with(blank);
        }
        mark_rows_dirty(&mut self.rows, top, bot, self.offset, self.num_rows);
    }

    /// Insert blank lines starting at `row`, shifting the rest of the active scroll region down.
    pub fn insert_lines_at_with(&mut self, row: usize, count: usize, blank: Cell) {
        if row < self.scroll_top || row > self.scroll_bottom {
            return;
        }

        let top = row;
        let bot = self.scroll_bottom;
        let region_size = bot - top + 1;
        let lines = count.min(region_size);
        if lines == 0 {
            return;
        }

        let abs_top = self.abs_row(top);
        let abs_bot = self.abs_row(bot);

        if abs_top <= abs_bot {
            self.rows[abs_top..=abs_bot].rotate_right(lines);
        } else {
            for _ in 0..lines {
                for r in (top..bot).rev() {
                    let src = self.abs_row(r);
                    let dst = self.abs_row(r + 1);
                    if src != dst {
                        self.rows.swap(src, dst);
                    }
                }
            }
        }

        for i in 0..lines {
            let idx = self.abs_row(top + i);
            self.rows[idx].clear_with(blank);
        }
        mark_rows_dirty(&mut self.rows, top, bot, self.offset, self.num_rows);
    }

    pub fn insert_lines_at(&mut self, row: usize, count: usize) {
        self.insert_lines_at_with(row, count, Cell::default());
    }

    /// Delete lines starting at `row`, shifting the rest of the active scroll region up.
    pub fn delete_lines_at_with(&mut self, row: usize, count: usize, blank: Cell) {
        if row < self.scroll_top || row > self.scroll_bottom {
            return;
        }

        let top = row;
        let bot = self.scroll_bottom;
        let region_size = bot - top + 1;
        let lines = count.min(region_size);
        if lines == 0 {
            return;
        }

        let abs_top = self.abs_row(top);
        let abs_bot = self.abs_row(bot);

        if abs_top <= abs_bot {
            self.rows[abs_top..=abs_bot].rotate_left(lines);
        } else {
            for _ in 0..lines {
                for r in top..bot {
                    let src = self.abs_row(r + 1);
                    let dst = self.abs_row(r);
                    if src != dst {
                        self.rows.swap(src, dst);
                    }
                }
            }
        }

        for i in 0..lines {
            let idx = self.abs_row(bot - (lines - 1) + i);
            self.rows[idx].clear_with(blank);
        }
        mark_rows_dirty(&mut self.rows, top, bot, self.offset, self.num_rows);
    }

    pub fn delete_lines_at(&mut self, row: usize, count: usize) {
        self.delete_lines_at_with(row, count, Cell::default());
    }

    /// Set the scroll region (0-based, inclusive top and bottom).
    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let t = top.min(self.screen_rows.saturating_sub(1));
        let b = bottom.min(self.screen_rows.saturating_sub(1));
        if t < b {
            self.scroll_top = t;
            self.scroll_bottom = b;
        }
        // Per DEC spec: setting scroll region moves cursor to home.
        self.cursor.col = 0;
        self.cursor.row = 0;
    }

    // ── Erase operations ─────────────────────────────────────────────────

    /// EL 0 — erase from cursor to end of line (inclusive).
    pub fn erase_to_end_of_line_with(&mut self, blank: Cell) {
        let r = self.cursor.row;
        let c = self.cursor.col;
        let idx = self.abs_row(r);
        let cols = self.screen_cols;
        self.rows[idx].cells[c..cols].fill(blank);
        self.rows[idx].dirty = true;
    }

    /// EL 1 — erase from start of line to cursor (inclusive).
    pub fn erase_to_start_of_line_with(&mut self, blank: Cell) {
        let r = self.cursor.row;
        let c = self.cursor.col;
        let idx = self.abs_row(r);
        let end = c.min(self.screen_cols - 1) + 1;
        self.rows[idx].cells[..end].fill(blank);
        self.rows[idx].dirty = true;
    }

    /// EL 2 — erase entire line.
    pub fn erase_line_with(&mut self, blank: Cell) {
        let r = self.cursor.row;
        let idx = self.abs_row(r);
        self.rows[idx].clear_with(blank);
    }

    /// ED 0 — erase from cursor to end of screen.
    pub fn erase_below_with(&mut self, blank: Cell) {
        // Erase from cursor to end of current line.
        self.erase_to_end_of_line_with(blank);
        // Erase all lines below.
        for r in (self.cursor.row + 1)..self.screen_rows {
            let idx = self.abs_row(r);
            self.rows[idx].clear_with(blank);
        }
    }

    /// ED 1 — erase from start of screen to cursor.
    pub fn erase_above_with(&mut self, blank: Cell) {
        // Erase from start of current line to cursor.
        self.erase_to_start_of_line_with(blank);
        // Erase all lines above.
        for r in 0..self.cursor.row {
            let idx = self.abs_row(r);
            self.rows[idx].clear_with(blank);
        }
    }

    /// ED 2 — erase entire screen.
    pub fn erase_all_with(&mut self, blank: Cell) {
        for r in 0..self.screen_rows {
            let idx = self.abs_row(r);
            self.rows[idx].clear_with(blank);
        }
    }

    /// ECH — erase `count` characters from cursor position (overwrite with blank, no shift).
    pub fn erase_chars_with(&mut self, count: usize, blank: Cell) {
        let r = self.cursor.row;
        let c = self.cursor.col;
        let idx = self.abs_row(r);
        let end = (c + count).min(self.screen_cols);
        self.rows[idx].cells[c..end].fill(blank);
        self.rows[idx].dirty = true;
    }

    // ── Convenience wrappers (use default blank cell) ────────────────────

    pub fn erase_to_end_of_line(&mut self) {
        self.erase_to_end_of_line_with(Cell::default());
    }

    pub fn erase_to_start_of_line(&mut self) {
        self.erase_to_start_of_line_with(Cell::default());
    }

    pub fn erase_all(&mut self) {
        self.erase_all_with(Cell::default());
    }

    pub fn erase_chars(&mut self, count: usize) {
        self.erase_chars_with(count, Cell::default());
    }

    // ── Character operations ─────────────────────────────────────────────

    /// DCH — delete `count` characters at cursor, shifting remaining left.
    pub fn delete_chars_with(&mut self, count: usize, blank: Cell) {
        let r = self.cursor.row;
        let c = self.cursor.col;
        let idx = self.abs_row(r);
        let cols = self.screen_cols;
        let n = count.min(cols.saturating_sub(c));

        if n == 0 {
            return;
        }

        self.rows[idx].cells.copy_within((c + n)..cols, c);
        self.rows[idx].cells[(cols - n)..cols].fill(blank);
        self.rows[idx].dirty = true;
    }

    pub fn delete_chars(&mut self, count: usize) {
        self.delete_chars_with(count, Cell::default());
    }

    /// ICH — insert `count` blank characters at cursor, shifting existing right.
    pub fn insert_chars_with(&mut self, count: usize, blank: Cell) {
        let r = self.cursor.row;
        let c = self.cursor.col;
        let idx = self.abs_row(r);
        let cols = self.screen_cols;
        let n = count.min(cols.saturating_sub(c));

        if n == 0 {
            return;
        }

        self.rows[idx].cells.copy_within(c..(cols - n), c + n);
        self.rows[idx].cells[c..(c + n)].fill(blank);
        self.rows[idx].dirty = true;
    }

    pub fn insert_chars(&mut self, count: usize) {
        self.insert_chars_with(count, Cell::default());
    }

    // ── Alternate screen ─────────────────────────────────────────────────

    /// Enter the alternate screen buffer (e.g. for vim, less).
    /// Saves the current main screen and creates a fresh grid.
    pub fn enter_alternate_screen(&mut self) {
        if self.alternate_active {
            return;
        }

        // Save current state.
        let saved = SavedGrid {
            rows: self.rows.clone(),
            num_rows: self.num_rows,
            offset: self.offset,
            cursor: self.cursor,
            saved_cursor: self.saved_cursor,
            scroll_top: self.scroll_top,
            scroll_bottom: self.scroll_bottom,
            scrollback_lines: self.scrollback_lines,
            max_scrollback: self.max_scrollback,
        };
        self.saved_grid = Some(Box::new(saved));
        self.alternate_active = true;

        // Reset to a fresh screen (same buffer size, but cleared visible area).
        // Re-use the existing buffer to avoid allocation; just clear visible rows and reset offset.
        self.offset = 0;
        self.scrollback_lines = 0;
        self.scroll_top = 0;
        self.scroll_bottom = self.screen_rows.saturating_sub(1);
        self.cursor = CursorState::default();
        self.saved_cursor = CursorState::default();
        self.view_offset = 0;

        // Clear all rows in the buffer for alternate screen (no scrollback).
        for row in &mut self.rows {
            row.clear();
        }
    }

    /// Leave the alternate screen buffer, restoring the main screen.
    pub fn leave_alternate_screen(&mut self) {
        if !self.alternate_active {
            return;
        }

        if let Some(saved) = self.saved_grid.take() {
            self.rows = saved.rows;
            self.num_rows = saved.num_rows;
            self.offset = saved.offset;
            self.cursor = saved.cursor;
            self.saved_cursor = saved.saved_cursor;
            self.scroll_top = saved.scroll_top;
            self.scroll_bottom = saved.scroll_bottom;
            self.scrollback_lines = saved.scrollback_lines;
            self.max_scrollback = saved.max_scrollback;
        }
        self.alternate_active = false;
        self.view_offset = 0;

        // Mark all visible rows dirty so they get redrawn.
        mark_rows_dirty(
            &mut self.rows,
            0,
            self.screen_rows.saturating_sub(1),
            self.offset,
            self.num_rows,
        );
    }

    // ── Scrollback ───────────────────────────────────────────────────────

    /// Number of lines currently in scrollback.
    pub fn scrollback_count(&self) -> usize {
        if self.alternate_active {
            0
        } else {
            self.scrollback_lines
        }
    }

    /// Scroll the view backward into history (toward older content).
    /// Returns true if the view actually changed.
    pub fn scroll_view_up(&mut self, lines: usize) -> bool {
        if self.alternate_active {
            return false;
        }
        let max = self.scrollback_lines;
        let old = self.view_offset;
        self.view_offset = (self.view_offset + lines).min(max);
        self.view_offset != old
    }

    /// Scroll the view forward toward current content.
    /// Returns true if the view actually changed.
    pub fn scroll_view_down(&mut self, lines: usize) -> bool {
        let old = self.view_offset;
        self.view_offset = self.view_offset.saturating_sub(lines);
        self.view_offset != old
    }

    /// Reset view to the current (bottom) position.
    pub fn reset_view(&mut self) {
        self.view_offset = 0;
    }

    /// Whether we are currently looking at scrollback history.
    pub fn is_viewing_scrollback(&self) -> bool {
        self.view_offset > 0
    }

    #[inline]
    pub fn view_row_cells(&self, row: usize) -> &[Cell] {
        let abs = self.view_abs_row(row);
        &self.rows[abs].cells
    }

    #[inline]
    pub fn clear_view_row_dirty(&mut self, row: usize) {
        let abs = self.view_abs_row(row);
        self.rows[abs].clear_dirty();
    }

    /// Get an immutable reference to a row, accounting for view offset.
    #[inline]
    pub fn view_row(&self, row: usize) -> &Row {
        let abs = self.view_abs_row(row);
        &self.rows[abs]
    }

    /// Get a mutable reference to a row, accounting for view offset.
    #[inline]
    pub fn view_row_mut(&mut self, row: usize) -> &mut Row {
        let abs = self.view_abs_row(row);
        &mut self.rows[abs]
    }

    // ── Resize ───────────────────────────────────────────────────────────

    /// Resize the grid, preserving content around the cursor row.
    pub fn resize(&mut self, cols: usize, rows: usize) {
        let new_cols = cols.max(1);
        let new_rows = rows.max(1);

        if new_cols == self.screen_cols && new_rows == self.screen_rows {
            return;
        }

        let old_cols = self.screen_cols;
        let old_rows = self.screen_rows;

        let (new_rows_buf, new_num_rows, new_offset, new_scrollback_lines, new_cursor_row) =
            resize_row_store(ResizeRowStoreInput {
                rows: &self.rows,
                num_rows: self.num_rows,
                offset: self.offset,
                screen_cols: old_cols,
                screen_rows: old_rows,
                scrollback_lines: if self.alternate_active {
                    0
                } else {
                    self.scrollback_lines
                },
                max_scrollback: self.max_scrollback,
                cursor_row: self.cursor.row,
                new_cols,
                new_rows,
                preserve_scrollback: !self.alternate_active,
            });

        self.rows = new_rows_buf;
        self.num_rows = new_num_rows;
        self.offset = new_offset;
        self.scrollback_lines = if self.alternate_active {
            0
        } else {
            new_scrollback_lines
        };
        self.cursor.row = new_cursor_row;

        if let Some(saved) = self.saved_grid.as_mut() {
            let (saved_rows, saved_num_rows, saved_offset, saved_scrollback, saved_cursor_row) =
                resize_row_store(ResizeRowStoreInput {
                    rows: &saved.rows,
                    num_rows: saved.num_rows,
                    offset: saved.offset,
                    screen_cols: old_cols,
                    screen_rows: old_rows,
                    scrollback_lines: saved.scrollback_lines,
                    max_scrollback: saved.max_scrollback,
                    cursor_row: saved.cursor.row,
                    new_cols,
                    new_rows,
                    preserve_scrollback: true,
                });
            saved.rows = saved_rows;
            saved.num_rows = saved_num_rows;
            saved.offset = saved_offset;
            saved.scrollback_lines = saved_scrollback;
            saved.cursor.row = saved_cursor_row;
            saved.cursor.col = saved.cursor.col.min(new_cols.saturating_sub(1));
            saved.saved_cursor.col = saved.saved_cursor.col.min(new_cols.saturating_sub(1));
            saved.saved_cursor.row = saved.saved_cursor.row.min(new_rows.saturating_sub(1));
            saved.scroll_top = 0;
            saved.scroll_bottom = new_rows.saturating_sub(1);
        }

        self.screen_cols = new_cols;
        self.screen_rows = new_rows;

        // Resize resets the scroll region to the full screen.
        self.scroll_top = 0;
        self.scroll_bottom = new_rows.saturating_sub(1);

        // Clamp cursor.
        self.cursor.col = self.cursor.col.min(new_cols.saturating_sub(1));
        self.cursor.row = self.cursor.row.min(new_rows.saturating_sub(1));
        self.saved_cursor.col = self.saved_cursor.col.min(new_cols.saturating_sub(1));
        self.saved_cursor.row = self.saved_cursor.row.min(new_rows.saturating_sub(1));

        self.view_offset = 0;

        // Mark all visible rows dirty.
        mark_rows_dirty(
            &mut self.rows,
            0,
            self.screen_rows.saturating_sub(1),
            self.offset,
            self.num_rows,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_grid_dimensions() {
        let g = Grid::new(80, 24);
        assert_eq!(g.cols(), 80);
        assert_eq!(g.rows(), 24);
    }

    #[test]
    fn test_circular_buffer_power_of_two() {
        let g = Grid::new(80, 24);
        assert!(g.num_rows.is_power_of_two());
        assert!(g.num_rows >= 24 * 4);
    }

    #[test]
    fn test_cell_default() {
        let g = Grid::new(80, 24);
        let c = g.cell(0, 0);
        assert_eq!(c.ch, ' ' as u32);
    }

    #[test]
    fn test_cursor_movement() {
        let mut g = Grid::new(80, 24);
        g.set_cursor(10, 5);
        assert_eq!(g.cursor.col, 10);
        assert_eq!(g.cursor.row, 5);
    }

    #[test]
    fn test_cursor_clamping() {
        let mut g = Grid::new(80, 24);
        g.set_cursor(200, 200);
        assert_eq!(g.cursor.col, 79);
        assert_eq!(g.cursor.row, 23);
    }

    #[test]
    fn test_linefeed_scrolls() {
        let mut g = Grid::new(80, 24);
        g.set_cursor(0, 23);
        g.cell_mut(0, 0).ch = 'A' as u32;
        g.linefeed();
        // After scroll, cursor should still be at bottom.
        assert_eq!(g.cursor.row, 23);
        assert_eq!(g.scrollback_count(), 1);
    }

    #[test]
    fn test_scroll_region() {
        let mut g = Grid::new(80, 24);
        g.set_scroll_region(5, 15);
        assert_eq!(g.scroll_top, 5);
        assert_eq!(g.scroll_bottom, 15);
        // Cursor moves home.
        assert_eq!(g.cursor.col, 0);
        assert_eq!(g.cursor.row, 0);
    }

    #[test]
    fn test_reverse_index_at_top() {
        let mut g = Grid::new(80, 24);
        g.set_cursor(0, 0);
        g.reverse_index();
        // Should have scrolled down, cursor stays at 0.
        assert_eq!(g.cursor.row, 0);
    }

    #[test]
    fn test_reverse_index_not_at_top() {
        let mut g = Grid::new(80, 24);
        g.set_cursor(0, 5);
        g.reverse_index();
        assert_eq!(g.cursor.row, 4);
    }

    #[test]
    fn test_erase_chars() {
        let mut g = Grid::new(80, 24);
        g.cell_mut(5, 0).ch = 'X' as u32;
        g.cell_mut(6, 0).ch = 'Y' as u32;
        g.set_cursor(5, 0);
        g.erase_chars(1);
        assert_eq!(g.cell(5, 0).ch, ' ' as u32);
        assert_eq!(g.cell(6, 0).ch, 'Y' as u32);
    }

    #[test]
    fn test_delete_chars() {
        let mut g = Grid::new(80, 24);
        g.cell_mut(0, 0).ch = 'A' as u32;
        g.cell_mut(1, 0).ch = 'B' as u32;
        g.cell_mut(2, 0).ch = 'C' as u32;
        g.set_cursor(0, 0);
        g.delete_chars(1);
        assert_eq!(g.cell(0, 0).ch, 'B' as u32);
        assert_eq!(g.cell(1, 0).ch, 'C' as u32);
    }

    #[test]
    fn test_insert_chars() {
        let mut g = Grid::new(80, 24);
        g.cell_mut(0, 0).ch = 'A' as u32;
        g.cell_mut(1, 0).ch = 'B' as u32;
        g.set_cursor(0, 0);
        g.insert_chars(1);
        assert_eq!(g.cell(0, 0).ch, ' ' as u32);
        assert_eq!(g.cell(1, 0).ch, 'A' as u32);
        assert_eq!(g.cell(2, 0).ch, 'B' as u32);
    }

    #[test]
    fn test_alternate_screen() {
        let mut g = Grid::new(80, 24);
        g.cell_mut(0, 0).ch = 'M' as u32;
        g.set_cursor(5, 5);

        g.enter_alternate_screen();
        assert_eq!(g.cell(0, 0).ch, ' ' as u32);
        assert_eq!(g.cursor.col, 0);
        assert_eq!(g.cursor.row, 0);
        assert_eq!(g.scrollback_count(), 0);

        g.leave_alternate_screen();
        assert_eq!(g.cell(0, 0).ch, 'M' as u32);
        assert_eq!(g.cursor.col, 5);
        assert_eq!(g.cursor.row, 5);
    }

    #[test]
    fn test_resize_cols() {
        let mut g = Grid::new(80, 24);
        g.cell_mut(0, 0).ch = 'A' as u32;
        g.resize(120, 24);
        assert_eq!(g.cols(), 120);
        assert_eq!(g.rows(), 24);
        assert_eq!(g.cell(0, 0).ch, 'A' as u32);
    }

    #[test]
    fn test_resize_rows_shrink() {
        let mut g = Grid::new(80, 24);
        g.cell_mut(0, 23).ch = 'Z' as u32;
        g.resize(80, 20);
        assert_eq!(g.rows(), 20);
    }

    #[test]
    fn test_scrollback_cap() {
        let mut g = Grid::new(80, 24);
        let max = g.max_scrollback;
        for _ in 0..(max + 10) {
            g.set_cursor(0, 23);
            g.linefeed();
        }
        assert_eq!(g.scrollback_count(), max);
    }

    #[test]
    fn test_custom_scrollback_cap() {
        let mut g = Grid::with_scrollback(80, 24, 5);
        for _ in 0..32 {
            g.set_cursor(0, 23);
            g.linefeed();
        }
        assert_eq!(g.scrollback_count(), 5);
    }

    #[test]
    fn test_carriage_return() {
        let mut g = Grid::new(80, 24);
        g.set_cursor(40, 10);
        g.carriage_return();
        assert_eq!(g.cursor.col, 0);
        assert_eq!(g.cursor.row, 10);
    }

    #[test]
    fn test_erase_to_end_of_line() {
        let mut g = Grid::new(80, 24);
        for i in 0..80 {
            g.cell_mut(i, 0).ch = 'X' as u32;
        }
        g.set_cursor(40, 0);
        g.erase_to_end_of_line();
        assert_eq!(g.cell(39, 0).ch, 'X' as u32);
        assert_eq!(g.cell(40, 0).ch, ' ' as u32);
        assert_eq!(g.cell(79, 0).ch, ' ' as u32);
    }

    #[test]
    fn test_erase_to_start_of_line() {
        let mut g = Grid::new(80, 24);
        for i in 0..80 {
            g.cell_mut(i, 0).ch = 'X' as u32;
        }
        g.set_cursor(40, 0);
        g.erase_to_start_of_line();
        assert_eq!(g.cell(0, 0).ch, ' ' as u32);
        assert_eq!(g.cell(40, 0).ch, ' ' as u32);
        assert_eq!(g.cell(41, 0).ch, 'X' as u32);
    }

    #[test]
    fn test_erase_all() {
        let mut g = Grid::new(80, 24);
        g.cell_mut(10, 10).ch = 'Q' as u32;
        g.erase_all();
        assert_eq!(g.cell(10, 10).ch, ' ' as u32);
    }

    #[test]
    fn test_save_restore_cursor() {
        let mut g = Grid::new(80, 24);
        g.set_cursor(15, 7);
        g.save_cursor();
        g.set_cursor(0, 0);
        g.restore_cursor();
        assert_eq!(g.cursor.col, 15);
        assert_eq!(g.cursor.row, 7);
    }

    #[test]
    fn test_scroll_region_linefeed() {
        let mut g = Grid::new(80, 24);
        g.set_scroll_region(5, 10);
        g.set_cursor(0, 10);
        g.cell_mut(0, 5).ch = 'T' as u32;
        g.cell_mut(0, 6).ch = 'U' as u32;
        g.linefeed();
        // Row 5 content ('T') should have scrolled off (region scroll up).
        // Row 6 content ('U') should now be at row 5.
        assert_eq!(g.cell(0, 5).ch, 'U' as u32);
        // No scrollback from region scrolls.
        assert_eq!(g.scrollback_count(), 0);
    }

    #[test]
    fn test_cursor_position() {
        let mut g = Grid::new(80, 24);
        g.set_cursor(10, 5);
        let (cx, cy) = g.cursor_position();
        assert!((cx - 10.0).abs() < 0.001);
        assert!((cy - 5.0).abs() < 0.001);
    }

    #[test]
    fn test_cursor_position_clamps_wrap_pending_column_for_rendering() {
        let mut g = Grid::new(4, 2);
        g.set_cursor_col(4);
        g.set_cursor_row(1);

        let (cx, cy) = g.cursor_position();
        assert!((cx - 3.0).abs() < 0.001);
        assert!((cy - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_linefeed_below_scroll_region_moves_cursor() {
        let mut g = Grid::new(8, 6);
        g.set_scroll_region(1, 3);
        g.set_cursor(0, 4);
        g.linefeed();
        assert_eq!(g.cursor.row, 5); // moves down, clamped to last row
    }

    #[test]
    fn test_linefeed_below_scroll_region_clamps_at_bottom() {
        let mut g = Grid::new(8, 6);
        g.set_scroll_region(1, 3);
        g.set_cursor(0, 5); // already at last row
        g.linefeed();
        assert_eq!(g.cursor.row, 5); // stays at last row
    }

    #[test]
    fn test_reverse_index_above_scroll_region_moves_cursor() {
        let mut g = Grid::new(8, 6);
        g.set_scroll_region(2, 4);
        g.set_cursor(0, 1);
        g.reverse_index();
        assert_eq!(g.cursor.row, 0); // moves up
    }

    #[test]
    fn test_reverse_index_above_scroll_region_clamps_at_top() {
        let mut g = Grid::new(8, 6);
        g.set_scroll_region(2, 4);
        g.set_cursor(0, 0); // already at top
        g.reverse_index();
        assert_eq!(g.cursor.row, 0); // stays at top
    }

    #[test]
    fn test_insert_lines_at_respects_scroll_region() {
        let mut g = Grid::new(1, 5);
        g.set_scroll_region(1, 3);
        g.cell_mut(0, 1).ch = 'A' as u32;
        g.cell_mut(0, 2).ch = 'B' as u32;
        g.cell_mut(0, 3).ch = 'C' as u32;
        g.cell_mut(0, 4).ch = 'Z' as u32;

        g.insert_lines_at(2, 1);

        assert_eq!(g.cell(0, 1).ch, 'A' as u32);
        assert_eq!(g.cell(0, 2).ch, ' ' as u32);
        assert_eq!(g.cell(0, 3).ch, 'B' as u32);
        assert_eq!(g.cell(0, 4).ch, 'Z' as u32);
    }

    #[test]
    fn test_delete_lines_at_respects_scroll_region() {
        let mut g = Grid::new(1, 5);
        g.set_scroll_region(1, 3);
        g.cell_mut(0, 1).ch = 'A' as u32;
        g.cell_mut(0, 2).ch = 'B' as u32;
        g.cell_mut(0, 3).ch = 'C' as u32;
        g.cell_mut(0, 4).ch = 'Z' as u32;

        g.delete_lines_at(2, 1);

        assert_eq!(g.cell(0, 1).ch, 'A' as u32);
        assert_eq!(g.cell(0, 2).ch, 'C' as u32);
        assert_eq!(g.cell(0, 3).ch, ' ' as u32);
        assert_eq!(g.cell(0, 4).ch, 'Z' as u32);
    }

    #[test]
    fn test_resize_shrink_rows_preserves_cursor_line() {
        let mut g = Grid::new(1, 4);
        for (row, ch) in ['a', 'b', 'c', 'd'].into_iter().enumerate() {
            g.cell_mut(0, row).ch = ch as u32;
        }
        g.set_cursor(0, 3);

        g.resize(1, 2);

        assert_eq!(g.cell(0, g.cursor.row).ch, 'd' as u32);
        assert_eq!(g.scrollback_count(), 2);
    }

    #[test]
    fn test_resize_resets_scroll_region() {
        let mut g = Grid::new(4, 4);
        g.set_scroll_region(1, 2);
        g.resize(4, 6);
        assert_eq!(g.scroll_region(), (0, 5));
    }

    #[test]
    fn test_resize_grow_taller_empty_grid_prompt_stays_at_top() {
        // Simulates the startup bug: grid created at one size, then compositor
        // resizes the window taller. With cursor at row 0 and no scrollback,
        // the prompt must stay at row 0, not get pushed to the middle.
        let mut g = Grid::new(80, 24);
        g.cell_mut(0, 0).ch = '$' as u32; // shell prompt
        g.set_cursor(2, 0);

        g.resize(80, 40); // compositor gives a taller window

        assert_eq!(g.cursor.row, 0, "cursor should stay at row 0");
        assert_eq!(
            g.cell(0, 0).ch,
            '$' as u32,
            "prompt should be on the first row"
        );
        assert_eq!(g.scrollback_count(), 0, "no scrollback expected");
    }

    #[test]
    fn test_resize_grow_taller_with_content_keeps_cursor_position() {
        // When the screen is full and grows taller, cursor row shouldn't jump.
        let mut g = Grid::new(1, 4);
        for (row, ch) in ['a', 'b', 'c', 'd'].into_iter().enumerate() {
            g.cell_mut(0, row).ch = ch as u32;
        }
        g.set_cursor(0, 3); // cursor at bottom

        g.resize(1, 6); // grow taller

        // Cursor line ('d') should still be visible and the content above it preserved.
        assert_eq!(g.cell(0, g.cursor.row).ch, 'd' as u32);
    }
}
