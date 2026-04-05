use std::{collections::VecDeque, sync::Arc};

use skia_safe::Color4f;
use unicode_width::UnicodeWidthChar;

use crate::terminal::{
    ClipboardRequest, ClipboardRequestKind, ClipboardSelection, Hyperlink,
    cell::{CellWidth, TerminalCell},
    cursor::TerminalCursor,
    input::{KittyKeyboardFlags, TerminalInputSettings, TerminalMouseMode},
    screen::TerminalScreen,
    style::{TerminalColor, TerminalColors, TerminalStyle},
    theme::{TerminalTheme, to_osc_rgb_spec},
};
use crate::ui::{CursorShape, UnderlineStyle};

const DEFAULT_SCROLLBACK_LIMIT: usize = 10_000;

/// Fixed-capacity bitset for tracking dirty rows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirtyRows {
    words: [u64; Self::WORDS],
}

impl DirtyRows {
    const MAX_ROWS: usize = 512;
    const WORDS: usize = Self::MAX_ROWS / 64;

    pub const fn new() -> Self {
        Self { words: [0; Self::WORDS] }
    }

    pub const fn can_represent_row(row: usize) -> bool {
        row < Self::MAX_ROWS
    }

    pub const fn can_represent_range(range: &std::ops::Range<usize>) -> bool {
        range.start <= range.end && range.end <= Self::MAX_ROWS
    }

    pub fn set(&mut self, row: usize) {
        debug_assert!(Self::can_represent_row(row));
        self.words[row / 64] |= 1u64 << (row % 64);
    }

    pub fn set_range(&mut self, range: std::ops::Range<usize>) {
        debug_assert!(Self::can_represent_range(&range));
        let start = range.start;
        let end = range.end;
        if start >= end {
            return;
        }
        let first_word = start / 64;
        let last_word = (end - 1) / 64;
        if first_word == last_word {
            self.words[first_word] |= Self::range_mask(start % 64, end - first_word * 64);
        } else {
            self.words[first_word] |= !0u64 << (start % 64);
            for word in &mut self.words[first_word + 1..last_word] {
                *word = !0u64;
            }
            self.words[last_word] |= Self::range_mask(0, end - last_word * 64);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.words.iter().all(|&w| w == 0)
    }

    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.words.iter().enumerate().flat_map(|(word_idx, &word)| {
            let base = word_idx * 64;
            BitIter(word).map(move |bit| base + bit)
        })
    }

    const fn range_mask(start: usize, end: usize) -> u64 {
        if end >= 64 { !0u64 << start } else { ((!0u64) >> (64 - end)) & (!0u64 << start) }
    }
}

impl Default for DirtyRows {
    fn default() -> Self {
        Self::new()
    }
}

struct BitIter(u64);

impl Iterator for BitIter {
    type Item = usize;
    fn next(&mut self) -> Option<usize> {
        if self.0 == 0 {
            None
        } else {
            let bit = self.0.trailing_zeros() as usize;
            self.0 &= self.0 - 1;
            Some(bit)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalDamage {
    None,
    Full,
    Rows(DirtyRows),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalSnapshot {
    pub cols: usize,
    pub rows: usize,
    pub cursor: TerminalCursor,
    pub title: Option<String>,
    pub using_alternate_screen: bool,
    pub scrollback_len: usize,
    pub input: TerminalInputSettings,
    pub synchronized_updates: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct TerminalPen {
    colors: TerminalColors,
    reverse: bool,
    italic: bool,
    bold: bool,
    strikethrough: bool,
    underline: Option<UnderlineStyle>,
    /// `None` = invalidated, `Some(None)` = default pen, `Some(Some(arc))` = cached style.
    cached_style: Option<Option<Arc<TerminalStyle>>>,
}

impl TerminalPen {
    pub fn style(&mut self) -> Option<Arc<TerminalStyle>> {
        if let Some(ref cached) = self.cached_style {
            return cached.clone();
        }
        let style = TerminalStyle {
            colors: self.colors.clone(),
            reverse: self.reverse,
            italic: self.italic,
            bold: self.bold,
            strikethrough: self.strikethrough,
            underline: self.underline,
        };
        let result = if style.is_default() { None } else { Some(Arc::new(style)) };
        self.cached_style = Some(result.clone());
        result
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn colors(&self) -> &TerminalColors {
        &self.colors
    }
    pub fn reverse(&self) -> bool {
        self.reverse
    }
    pub fn italic(&self) -> bool {
        self.italic
    }
    pub fn bold(&self) -> bool {
        self.bold
    }
    pub fn strikethrough(&self) -> bool {
        self.strikethrough
    }
    pub fn underline(&self) -> Option<UnderlineStyle> {
        self.underline
    }

    fn invalidate_cache(&mut self) {
        self.cached_style = None;
    }
}

const KITTY_KEYBOARD_STACK_LIMIT: usize = 16;

pub struct TerminalState {
    primary_screen: TerminalScreen,
    alternate_screen: Option<TerminalScreen>,
    cursor: TerminalCursor,
    saved_cursor: Option<TerminalCursor>,
    saved_cursor_position: Option<TerminalCursor>,
    pen: TerminalPen,
    title: Option<String>,
    using_alternate_screen: bool,
    scrollback: VecDeque<Vec<TerminalCell>>,
    scrollback_limit: usize,
    damage: TerminalDamage,
    input: TerminalInputSettings,
    kitty_keyboard_stack: Vec<KittyKeyboardFlags>,
    pending_responses: Vec<Vec<u8>>,
    pending_clipboard_requests: Vec<ClipboardRequest>,
    g0_charset: DecCharset,
    g1_charset: DecCharset,
    active_charset: CharsetSlot,
    scroll_region_top: usize,
    scroll_region_bottom: usize,
    left_margin: usize,
    right_margin: usize,
    left_right_margin_mode: bool,
    origin_mode: bool,
    auto_wrap: bool,
    insert_mode: bool,
    wrap_pending: bool,
    synchronized_updates: bool,
    current_hyperlink: Option<Arc<Hyperlink>>,
    base_theme: Box<TerminalTheme>,
    theme: TerminalTheme,
}

impl TerminalState {
    pub fn new(cols: usize, rows: usize) -> Self {
        Self::with_theme(cols, rows, TerminalTheme::default())
    }

    pub fn with_theme(cols: usize, rows: usize, theme: TerminalTheme) -> Self {
        let base_theme = Box::new(theme.clone());
        Self {
            primary_screen: TerminalScreen::new(cols, rows),
            alternate_screen: None,
            cursor: TerminalCursor::default(),
            saved_cursor: None,
            saved_cursor_position: None,
            pen: TerminalPen::default(),
            title: None,
            using_alternate_screen: false,
            scrollback: VecDeque::with_capacity(256),
            scrollback_limit: DEFAULT_SCROLLBACK_LIMIT,
            damage: TerminalDamage::Full,
            input: TerminalInputSettings::default(),
            kitty_keyboard_stack: Vec::new(),
            pending_responses: Vec::new(),
            pending_clipboard_requests: Vec::new(),
            g0_charset: DecCharset::Ascii,
            g1_charset: DecCharset::Ascii,
            active_charset: CharsetSlot::G0,
            scroll_region_top: 0,
            scroll_region_bottom: rows.max(1),
            left_margin: 0,
            right_margin: cols.max(1),
            left_right_margin_mode: false,
            origin_mode: false,
            auto_wrap: true,
            insert_mode: false,
            wrap_pending: false,
            synchronized_updates: false,
            current_hyperlink: None,
            base_theme,
            theme,
        }
    }

    pub fn cols(&self) -> usize {
        self.screen().cols()
    }

    pub fn rows(&self) -> usize {
        self.screen().rows()
    }

    pub fn screen(&self) -> &TerminalScreen {
        if self.using_alternate_screen {
            self.alternate_screen.as_ref().expect("alternate screen must exist when active")
        } else {
            &self.primary_screen
        }
    }

    pub fn screen_mut(&mut self) -> &mut TerminalScreen {
        if self.using_alternate_screen {
            self.alternate_screen.as_mut().expect("alternate screen must exist when active")
        } else {
            &mut self.primary_screen
        }
    }

    pub fn cursor(&self) -> &TerminalCursor {
        &self.cursor
    }

    pub fn pen(&self) -> &TerminalPen {
        &self.pen
    }

    pub fn snapshot(&self) -> TerminalSnapshot {
        TerminalSnapshot {
            cols: self.cols(),
            rows: self.rows(),
            cursor: self.cursor.clone(),
            title: self.title.clone(),
            using_alternate_screen: self.using_alternate_screen,
            scrollback_len: self.scrollback.len(),
            input: self.input,
            synchronized_updates: self.synchronized_updates,
        }
    }

    pub fn input_settings(&self) -> TerminalInputSettings {
        self.input
    }

    pub fn theme(&self) -> &TerminalTheme {
        &self.theme
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn synchronized_updates_active(&self) -> bool {
        self.synchronized_updates
    }

    pub fn current_hyperlink(&self) -> Option<&Arc<Hyperlink>> {
        self.current_hyperlink.as_ref()
    }

    pub fn scroll_region(&self) -> (usize, usize) {
        (self.scroll_region_top, self.scroll_region_bottom)
    }

    pub fn take_pending_clipboard_requests(&mut self) -> Vec<ClipboardRequest> {
        std::mem::take(&mut self.pending_clipboard_requests)
    }

    pub fn set_palette_color(&mut self, index: u8, color: Color4f) {
        self.theme.set_palette_color(index, color);
        self.damage = TerminalDamage::Full;
    }

    pub fn reset_palette_color(&mut self, index: u8) {
        self.theme.set_palette_color(index, self.base_theme.palette_color(index));
        self.damage = TerminalDamage::Full;
    }

    pub fn reset_palette(&mut self) {
        self.theme.copy_palette_from(&self.base_theme);
        self.damage = TerminalDamage::Full;
    }

    pub fn set_default_foreground(&mut self, color: Color4f) {
        self.theme.set_foreground(color);
        self.damage = TerminalDamage::Full;
    }

    pub fn reset_default_foreground(&mut self) {
        self.theme.set_foreground(self.base_theme.foreground);
        self.damage = TerminalDamage::Full;
    }

    pub fn set_default_background(&mut self, color: Color4f) {
        self.theme.set_background(color);
        self.damage = TerminalDamage::Full;
    }

    pub fn reset_default_background(&mut self) {
        self.theme.set_background(self.base_theme.background);
        self.damage = TerminalDamage::Full;
    }

    pub fn set_cursor_color(&mut self, color: Color4f) {
        self.theme.set_cursor(color);
        self.damage = TerminalDamage::Full;
    }

    pub fn reset_cursor_color(&mut self) {
        self.theme.set_cursor(self.base_theme.cursor);
        self.damage = TerminalDamage::Full;
    }

    pub fn queue_osc_color_response(&mut self, code: &str, color: Color4f) {
        self.pending_responses
            .push(format!("\x1b]{code};{}\x1b\\", to_osc_rgb_spec(color)).into_bytes());
    }

    pub fn queue_osc_palette_response(&mut self, index: u8, color: Color4f) {
        self.pending_responses
            .push(format!("\x1b]4;{index};{}\x1b\\", to_osc_rgb_spec(color)).into_bytes());
    }

    pub fn queue_clipboard_set(&mut self, selection: ClipboardSelection, content: String) {
        self.pending_clipboard_requests
            .push(ClipboardRequest { selection, kind: ClipboardRequestKind::Set(content) });
    }

    pub fn queue_clipboard_query(&mut self, selection: ClipboardSelection) {
        self.pending_clipboard_requests
            .push(ClipboardRequest { selection, kind: ClipboardRequestKind::Query });
    }

    pub fn set_current_hyperlink(&mut self, hyperlink: Option<Hyperlink>) {
        self.current_hyperlink = hyperlink.map(Arc::new);
    }

    pub fn take_pending_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.pending_responses)
    }

    pub fn take_damage(&mut self) -> TerminalDamage {
        let damage = std::mem::replace(&mut self.damage, TerminalDamage::None);
        match &damage {
            TerminalDamage::Rows(rows) if rows.is_empty() => TerminalDamage::None,
            _ => damage,
        }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) {
        self.primary_screen.resize(cols, rows);
        if let Some(ref mut alt) = self.alternate_screen {
            alt.resize(cols, rows);
        }
        self.cursor.column = self.cursor.column.min(cols.saturating_sub(1));
        self.cursor.row = self.cursor.row.min(rows.saturating_sub(1));
        self.scroll_region_top = 0;
        self.scroll_region_bottom = rows.max(1);
        self.left_margin = 0;
        self.right_margin = cols.max(1);
        self.left_right_margin_mode = false;
        self.wrap_pending = false;
        self.damage = TerminalDamage::Full;
    }

    pub fn print_char(&mut self, ch: char) {
        let ch = self.map_printable_char(ch);
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);
        if width == 0 {
            self.apply_combining_mark(ch);
            return;
        }

        if self.wrap_pending {
            self.carriage_return();
            self.linefeed();
            self.wrap_pending = false;
        }

        self.wrap_if_needed(width);

        let style = self.pen.style();
        let hyperlink = self.current_hyperlink.clone();
        let col = self.cursor.column;
        let row = self.cursor.row;
        let cols = self.cols();
        let cell_width = if width > 1 { CellWidth::Double } else { CellWidth::Single };

        if self.insert_mode {
            let fill = self.erase_cell();
            self.screen_mut().insert_blank_chars_with(row, col, width.max(1), &fill);
        }

        self.screen_mut().set(
            col,
            row,
            TerminalCell::from_char(ch, style.clone(), hyperlink.clone(), cell_width),
        );
        if width > 1 && col + 1 < cols {
            self.screen_mut().set(col + 1, row, TerminalCell::continuation(style, hyperlink));
        }

        if col + width >= cols {
            self.cursor.column = cols.saturating_sub(1);
            self.wrap_pending = self.auto_wrap && width == 1;
        } else {
            self.cursor.column += width;
            self.wrap_pending = false;
        }
        self.mark_row_dirty(row);
    }

    pub fn carriage_return(&mut self) {
        self.cursor.column = 0;
        self.wrap_pending = false;
    }

    pub fn backspace(&mut self) {
        self.cursor.column = self.cursor.column.saturating_sub(1);
        self.wrap_pending = false;
    }

    pub fn save_cursor_position(&mut self) {
        self.saved_cursor_position = Some(self.cursor.clone());
    }

    fn save_cursor_state(&mut self) {
        self.saved_cursor = Some(self.cursor.clone());
    }

    fn restore_cursor_state(&mut self) {
        if let Some(saved) = self.saved_cursor.as_ref() {
            self.cursor = saved.clone();
            self.wrap_pending = false;
        }
    }

    pub fn restore_cursor_position(&mut self) {
        if let Some(saved) = self.saved_cursor_position.as_ref() {
            self.cursor = saved.clone();
            self.wrap_pending = false;
        }
    }

    pub fn next_line(&mut self, count: usize) {
        let count = count.max(1);
        let max_row = if self.origin_mode {
            self.scroll_region_bottom.saturating_sub(1)
        } else {
            self.rows().saturating_sub(1)
        };
        self.cursor.row = (self.cursor.row + count).min(max_row);
        self.cursor.column = 0;
        self.wrap_pending = false;
    }

    pub fn previous_line(&mut self, count: usize) {
        let count = count.max(1);
        let min_row = if self.origin_mode { self.scroll_region_top } else { 0 };
        self.cursor.row = self.cursor.row.saturating_sub(count).max(min_row);
        self.cursor.column = 0;
        self.wrap_pending = false;
    }

    pub fn linefeed(&mut self) {
        let scroll_region_top = self.scroll_region_top;
        let scroll_region_bottom = self.scroll_region_bottom;
        let bottom_margin = scroll_region_bottom.saturating_sub(1);
        if self.cursor.row == bottom_margin {
            // Cursor is at the bottom of the active scroll region → scroll the region up.
            let fill = self.erase_cell();
            let removed = self.screen_mut().scroll_up_in_region_with(
                scroll_region_top,
                scroll_region_bottom,
                1,
                &fill,
            );
            if !self.using_alternate_screen
                && self.scroll_region_top == 0
                && self.scroll_region_bottom == self.rows()
            {
                for row in removed {
                    self.scrollback.push_back(row);
                }
                let overflow = self.scrollback.len().saturating_sub(self.scrollback_limit);
                if overflow > 0 {
                    self.scrollback.drain(..overflow);
                }
            }
            self.mark_rows_dirty(scroll_region_top..scroll_region_bottom);
        } else if self.cursor.row + 1 < self.rows() {
            // Cursor is not at the bottom margin and there is a row below → move down.
            self.cursor.row += 1;
        }
        // Otherwise cursor is already at the last screen row but outside the active scroll
        // region.  Per VT100/VT220 spec a linefeed only scrolls when the cursor is at the
        // bottom margin; if it is below the bottom margin (e.g. in a status-bar area) the
        // cursor simply stays where it is.  The previous code incorrectly scrolled the entire
        // primary screen in this case, corrupting output from apps like neovim and cmus.
    }

    pub fn reverse_index(&mut self) {
        let scroll_region_top = self.scroll_region_top;
        let scroll_region_bottom = self.scroll_region_bottom;
        if self.cursor.row == scroll_region_top {
            let fill = self.erase_cell();
            self.screen_mut().scroll_down_in_region_with(
                scroll_region_top,
                scroll_region_bottom,
                1,
                &fill,
            );
            self.mark_rows_dirty(scroll_region_top..scroll_region_bottom);
        } else {
            self.cursor.row = self.cursor.row.saturating_sub(1);
        }
    }

    pub fn tab(&mut self) {
        let next_tab_stop = ((self.cursor.column / 8) + 1) * 8;
        self.cursor.column = next_tab_stop.min(self.cols().saturating_sub(1));
    }

    pub fn set_cursor_position(&mut self, row: usize, col: usize) {
        let row = if self.origin_mode {
            let top = self.scroll_region_top;
            let bottom = self.scroll_region_bottom.saturating_sub(1);
            (top + row).min(bottom)
        } else {
            row.min(self.rows().saturating_sub(1))
        };
        self.cursor.row = row;
        self.cursor.column = col.min(self.cols().saturating_sub(1));
        self.wrap_pending = false;
    }

    pub fn set_cursor_column(&mut self, col: usize) {
        self.cursor.column = col.min(self.cols().saturating_sub(1));
        self.wrap_pending = false;
    }

    pub fn set_cursor_row(&mut self, row: usize) {
        self.cursor.row = if self.origin_mode {
            let top = self.scroll_region_top;
            let bottom = self.scroll_region_bottom.saturating_sub(1);
            (top + row).min(bottom)
        } else {
            row.min(self.rows().saturating_sub(1))
        };
        self.wrap_pending = false;
    }

    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let rows = self.rows();
        if top < bottom && bottom <= rows {
            self.scroll_region_top = top;
            self.scroll_region_bottom = bottom;
        } else {
            self.scroll_region_top = 0;
            self.scroll_region_bottom = rows;
        }
        self.set_cursor_position(0, 0);
    }

    pub fn move_cursor(&mut self, rows: isize, cols: isize) {
        let (min_row, max_row) = if self.origin_mode {
            (self.scroll_region_top, self.scroll_region_bottom.saturating_sub(1))
        } else {
            (0, self.rows().saturating_sub(1))
        };
        let new_row = self.cursor.row.saturating_add_signed(rows).clamp(min_row, max_row);
        let new_col =
            self.cursor.column.saturating_add_signed(cols).min(self.cols().saturating_sub(1));
        self.cursor.row = new_row;
        self.cursor.column = new_col;
        self.wrap_pending = false;
    }

    pub fn clear_screen(&mut self) {
        let fill = self.erase_cell();
        self.screen_mut().clear_with(&fill);
        self.damage = TerminalDamage::Full;
    }

    pub fn clear_from_cursor(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        let fill = self.erase_cell();
        self.screen_mut().clear_from_cursor_with(col, row, &fill);
        self.mark_rows_dirty(row..self.rows());
    }

    pub fn clear_to_cursor(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        let fill = self.erase_cell();
        self.screen_mut().clear_to_cursor_with(col, row, &fill);
        self.mark_rows_dirty(0..row.saturating_add(1));
    }

    pub fn clear_line(&mut self) {
        let row = self.cursor.row;
        let fill = self.erase_cell();
        self.screen_mut().clear_line_with(row, &fill);
        self.mark_row_dirty(row);
    }

    pub fn clear_line_from_cursor(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        let fill = self.erase_cell();
        self.screen_mut().clear_line_from_with(row, col, &fill);
        self.mark_row_dirty(row);
    }

    pub fn clear_line_to_cursor(&mut self) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        let fill = self.erase_cell();
        self.screen_mut().clear_line_to_with(row, col, &fill);
        self.mark_row_dirty(row);
    }

    pub fn erase_chars(&mut self, count: usize) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        let fill = self.erase_cell();
        self.screen_mut().erase_chars_with(row, col, count.max(1), &fill);
        self.mark_row_dirty(row);
    }

    pub fn delete_chars(&mut self, count: usize) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        let fill = self.erase_cell();
        self.screen_mut().delete_chars_with(row, col, count.max(1), &fill);
        self.mark_row_dirty(row);
    }

    pub fn insert_blank_chars(&mut self, count: usize) {
        let row = self.cursor.row;
        let col = self.cursor.column;
        let fill = self.erase_cell();
        self.screen_mut().insert_blank_chars_with(row, col, count.max(1), &fill);
        self.mark_row_dirty(row);
    }

    pub fn insert_lines(&mut self, count: usize) {
        let row = self.cursor.row;
        let scroll_region_bottom = self.scroll_region_bottom;
        if row >= self.scroll_region_top && row < scroll_region_bottom {
            let fill = self.erase_cell();
            self.screen_mut().insert_lines_in_region_with(
                row,
                scroll_region_bottom,
                count.max(1),
                &fill,
            );
            self.mark_rows_dirty(row..scroll_region_bottom);
        }
    }

    pub fn delete_lines(&mut self, count: usize) {
        let row = self.cursor.row;
        let scroll_region_bottom = self.scroll_region_bottom;
        if row >= self.scroll_region_top && row < scroll_region_bottom {
            let fill = self.erase_cell();
            self.screen_mut().delete_lines_in_region_with(
                row,
                scroll_region_bottom,
                count.max(1),
                &fill,
            );
            self.mark_rows_dirty(row..scroll_region_bottom);
        }
    }

    pub fn scroll_up_lines(&mut self, count: usize) {
        let scroll_region_top = self.scroll_region_top;
        let scroll_region_bottom = self.scroll_region_bottom;
        let fill = self.erase_cell();
        let removed = self.screen_mut().scroll_up_in_region_with(
            scroll_region_top,
            scroll_region_bottom,
            count.max(1),
            &fill,
        );
        if !self.using_alternate_screen
            && self.scroll_region_top == 0
            && self.scroll_region_bottom == self.rows()
        {
            for row in removed {
                self.scrollback.push_back(row);
            }
            let overflow = self.scrollback.len().saturating_sub(self.scrollback_limit);
            if overflow > 0 {
                self.scrollback.drain(..overflow);
            }
        }
        self.mark_rows_dirty(scroll_region_top..scroll_region_bottom);
    }

    pub fn scroll_down_lines(&mut self, count: usize) {
        let scroll_region_top = self.scroll_region_top;
        let scroll_region_bottom = self.scroll_region_bottom;
        let fill = self.erase_cell();
        self.screen_mut().scroll_down_in_region_with(
            scroll_region_top,
            scroll_region_bottom,
            count.max(1),
            &fill,
        );
        self.mark_rows_dirty(scroll_region_top..scroll_region_bottom);
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        self.title = Some(title.into());
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor.visible = visible;
    }

    pub fn set_cursor_shape(&mut self, shape: CursorShape) {
        self.cursor.shape = shape;
    }

    pub fn enter_alternate_screen(&mut self) {
        self.set_alternate_screen(true, true);
    }

    pub fn exit_alternate_screen(&mut self) {
        self.set_alternate_screen(false, true);
    }

    fn set_alternate_screen(&mut self, enabled: bool, save_restore_cursor: bool) {
        if enabled {
            if self.using_alternate_screen {
                return;
            }
            if save_restore_cursor {
                self.save_cursor_state();
            }
            let cols = self.primary_screen.cols();
            let rows = self.primary_screen.rows();
            match self.alternate_screen {
                Some(ref mut alt) => alt.clear(),
                None => self.alternate_screen = Some(TerminalScreen::new(cols, rows)),
            }
            self.using_alternate_screen = true;
            self.cursor.row = self.cursor.row.min(self.rows().saturating_sub(1));
            self.cursor.column = self.cursor.column.min(self.cols().saturating_sub(1));
            self.wrap_pending = false;
            self.damage = TerminalDamage::Full;
        } else {
            if !self.using_alternate_screen {
                return;
            }
            let alt_cursor = self.cursor.clone();
            self.using_alternate_screen = false;
            self.cursor.row = alt_cursor.row.min(self.rows().saturating_sub(1));
            self.cursor.column = alt_cursor.column.min(self.cols().saturating_sub(1));
            if save_restore_cursor {
                self.restore_cursor_state();
            }
            self.wrap_pending = false;
            self.damage = TerminalDamage::Full;
        }
    }

    pub fn set_sgr(&mut self, params: &[i64]) {
        self.set_sgr_iter(params.iter().copied());
    }

    pub(crate) fn set_sgr_iter<I>(&mut self, params: I)
    where
        I: IntoIterator<Item = i64>,
    {
        let params = params.into_iter().collect::<Vec<_>>();
        if params.is_empty() {
            self.pen.reset();
            return;
        }

        let mut index = 0;
        while index < params.len() {
            match params[index] {
                0 => self.pen.reset(),
                1 => self.pen.bold = true,
                3 => self.pen.italic = true,
                4 => self.pen.underline = Some(UnderlineStyle::Underline),
                7 => self.pen.reverse = true,
                9 => self.pen.strikethrough = true,
                22 => self.pen.bold = false,
                23 => self.pen.italic = false,
                24 => self.pen.underline = None,
                27 => self.pen.reverse = false,
                29 => self.pen.strikethrough = false,
                30..=37 => {
                    self.pen.colors.foreground =
                        Some(TerminalColor::Palette((params[index] - 30) as u8))
                }
                39 => self.pen.colors.foreground = None,
                40..=47 => {
                    self.pen.colors.background =
                        Some(TerminalColor::Palette((params[index] - 40) as u8))
                }
                49 => self.pen.colors.background = None,
                90..=97 => {
                    self.pen.colors.foreground =
                        Some(TerminalColor::Palette((params[index] - 90 + 8) as u8))
                }
                100..=107 => {
                    self.pen.colors.background =
                        Some(TerminalColor::Palette((params[index] - 100 + 8) as u8))
                }
                38 | 48 => {
                    if let Some((color, consumed)) = parse_extended_color(&params[index + 1..]) {
                        if params[index] == 38 {
                            self.pen.colors.foreground = Some(color);
                        } else {
                            self.pen.colors.background = Some(color);
                        }
                        index += consumed;
                    }
                }
                _ => {}
            }

            index += 1;
        }
        self.pen.invalidate_cache();
    }

    pub fn report_device_status(&mut self, status: i64) {
        match status {
            5 => self.pending_responses.push(b"\x1b[0n".to_vec()),
            6 => {
                let row = if self.origin_mode {
                    self.cursor.row.saturating_sub(self.scroll_region_top)
                } else {
                    self.cursor.row
                };
                self.pending_responses
                    .push(format!("\x1b[{};{}R", row + 1, self.cursor.column + 1).into_bytes())
            }
            _ => {}
        }
    }

    pub fn report_primary_device_attributes(&mut self) {
        self.pending_responses.push(b"\x1b[?62;c".to_vec());
    }

    pub fn report_secondary_device_attributes(&mut self) {
        self.pending_responses.push(b"\x1b[>1;10;0c".to_vec());
    }

    pub fn report_private_mode(&mut self, mode: i64) {
        let status = self.private_mode_status(mode).unwrap_or(0);
        self.pending_responses.push(format!("\x1b[?{mode};{status}$y").into_bytes());
    }

    pub fn report_selection_or_setting(&mut self, intermediates: &[u8], value: &str) {
        let mut response = Vec::with_capacity(8 + intermediates.len() + value.len());
        response.extend_from_slice(b"\x1bP");
        response.extend_from_slice(intermediates);
        response.extend_from_slice(value.as_bytes());
        response.extend_from_slice(b"\x1b\\");
        self.pending_responses.push(response);
    }

    pub fn report_kitty_keyboard_flags(&mut self) {
        self.pending_responses
            .push(format!("\x1b[?{}u", self.input.kitty_keyboard_flags.bits()).into_bytes());
    }

    pub fn push_kitty_keyboard_flags(&mut self, flags: i64) {
        let flags = KittyKeyboardFlags::new(flags.max(0) as u8);
        if self.kitty_keyboard_stack.len() < KITTY_KEYBOARD_STACK_LIMIT {
            self.kitty_keyboard_stack.push(self.input.kitty_keyboard_flags);
        }
        self.input.kitty_keyboard_flags = flags;
    }

    pub fn pop_kitty_keyboard_flags(&mut self, count: i64) {
        let count = count.max(1) as usize;
        for _ in 0..count {
            let Some(flags) = self.kitty_keyboard_stack.pop() else {
                break;
            };
            self.input.kitty_keyboard_flags = flags;
        }
    }

    pub fn set_kitty_keyboard_flags(&mut self, flags: i64, mode: i64) {
        let flags = KittyKeyboardFlags::new(flags.max(0) as u8);
        match mode {
            1 => self.input.kitty_keyboard_flags = flags,
            2 => {
                let combined = self.input.kitty_keyboard_flags.bits() | flags.bits();
                self.input.kitty_keyboard_flags = KittyKeyboardFlags::new(combined);
            }
            3 => {
                let remaining = self.input.kitty_keyboard_flags.bits() & !flags.bits();
                self.input.kitty_keyboard_flags = KittyKeyboardFlags::new(remaining);
            }
            _ => {}
        }
    }

    pub fn designate_charset(&mut self, slot: CharsetSlot, charset: DecCharset) {
        match slot {
            CharsetSlot::G0 => self.g0_charset = charset,
            CharsetSlot::G1 => self.g1_charset = charset,
        }
    }

    /// DECSTR – Soft Terminal Reset (CSI ! p).  Resets volatile terminal state without
    /// clearing screen content.  Matches the subset of attributes that neovim and other
    /// TUI applications rely on being reset during initialisation.
    pub fn soft_reset(&mut self) {
        self.pen.reset();
        self.cursor.visible = true;
        self.cursor.shape = crate::ui::CursorShape::Block;
        self.auto_wrap = true;
        self.wrap_pending = false;
        self.origin_mode = false;
        self.input.application_cursor = false;
        self.input.application_keypad = false;
        self.insert_mode = false;
        self.left_margin = 0;
        self.right_margin = self.cols();
        self.left_right_margin_mode = false;
        self.g0_charset = DecCharset::Ascii;
        self.g1_charset = DecCharset::Ascii;
        self.active_charset = CharsetSlot::G0;
        let rows = self.rows();
        self.scroll_region_top = 0;
        self.scroll_region_bottom = rows;
        self.damage = TerminalDamage::Full;
    }

    pub fn shift_out(&mut self) {
        self.active_charset = CharsetSlot::G1;
    }

    pub fn shift_in(&mut self) {
        self.active_charset = CharsetSlot::G0;
    }

    pub fn set_application_keypad(&mut self, enabled: bool) {
        self.input.application_keypad = enabled;
    }

    pub fn set_insert_mode(&mut self, enabled: bool) {
        self.insert_mode = enabled;
    }

    pub fn set_left_right_margin_mode(&mut self, enabled: bool) {
        self.left_right_margin_mode = enabled;
        if !enabled {
            self.left_margin = 0;
            self.right_margin = self.cols();
        }
    }

    pub fn left_right_margin_mode_enabled(&self) -> bool {
        self.left_right_margin_mode
    }

    pub fn set_left_right_margins(&mut self, left: usize, right: usize) {
        let cols = self.cols();
        if left < right && right <= cols {
            self.left_margin = left;
            self.right_margin = right;
        } else {
            self.left_margin = 0;
            self.right_margin = cols;
        }
        self.set_cursor_position(0, 0);
    }

    pub fn set_synchronized_updates(&mut self, enabled: bool) {
        self.synchronized_updates = enabled;
    }

    pub fn use_private_mode(&mut self, mode: i64, enabled: bool) {
        match mode {
            1 => self.input.application_cursor = enabled,
            6 => {
                self.origin_mode = enabled;
                self.set_cursor_position(0, 0);
            }
            7 => {
                self.auto_wrap = enabled;
                self.wrap_pending = false;
            }
            25 => self.set_cursor_visible(enabled),
            66 => self.set_application_keypad(enabled),
            69 => self.set_left_right_margin_mode(enabled),
            1000 => {
                if enabled {
                    self.input.mouse_mode = TerminalMouseMode::Click;
                } else if self.input.mouse_mode == TerminalMouseMode::Click {
                    self.input.mouse_mode = TerminalMouseMode::Disabled;
                }
            }
            1002 => {
                if enabled {
                    self.input.mouse_mode = TerminalMouseMode::Drag;
                } else if self.input.mouse_mode == TerminalMouseMode::Drag {
                    self.input.mouse_mode = TerminalMouseMode::Disabled;
                }
            }
            1003 => {
                if enabled {
                    self.input.mouse_mode = TerminalMouseMode::Motion;
                } else if self.input.mouse_mode == TerminalMouseMode::Motion {
                    self.input.mouse_mode = TerminalMouseMode::Disabled;
                }
            }
            1004 => self.input.focus_reporting = enabled,
            1006 => self.input.sgr_mouse = enabled,
            2026 => self.set_synchronized_updates(enabled),
            47 | 1047 => self.set_alternate_screen(enabled, false),
            1048 => {
                if enabled {
                    self.save_cursor_state();
                } else {
                    self.restore_cursor_state();
                }
            }
            2004 => self.input.bracketed_paste = enabled,
            1049 => self.set_alternate_screen(enabled, true),
            _ => {}
        }
    }

    fn private_mode_status(&self, mode: i64) -> Option<i64> {
        let enabled = match mode {
            1 => self.input.application_cursor,
            25 => self.cursor.visible,
            47 | 1047 | 1049 => self.using_alternate_screen,
            66 => self.input.application_keypad,
            69 => self.left_right_margin_mode,
            1000 => self.input.mouse_mode == TerminalMouseMode::Click,
            1002 => self.input.mouse_mode == TerminalMouseMode::Drag,
            1003 => self.input.mouse_mode == TerminalMouseMode::Motion,
            1004 => self.input.focus_reporting,
            1006 => self.input.sgr_mouse,
            2004 => self.input.bracketed_paste,
            2026 => self.synchronized_updates,
            _ => return None,
        };

        Some(if enabled { 1 } else { 2 })
    }

    fn current_charset(&self) -> DecCharset {
        match self.active_charset {
            CharsetSlot::G0 => self.g0_charset,
            CharsetSlot::G1 => self.g1_charset,
        }
    }

    fn map_printable_char(&self, ch: char) -> char {
        match self.current_charset() {
            DecCharset::Ascii => ch,
            DecCharset::DecSpecialGraphics => map_dec_special_graphics(ch),
        }
    }

    fn erase_cell(&mut self) -> TerminalCell {
        TerminalCell::blank_plain(self.pen.style())
    }

    fn mark_row_dirty(&mut self, row: usize) {
        self.mark_rows_dirty(row..row.saturating_add(1));
    }

    fn mark_rows_dirty(&mut self, rows: std::ops::Range<usize>) {
        if rows.start >= rows.end {
            return;
        }

        let rows_end = rows.end.min(self.rows());
        if rows.start >= rows_end {
            return;
        }

        let rows = rows.start..rows_end;
        if self.rows() > DirtyRows::MAX_ROWS || !DirtyRows::can_represent_range(&rows) {
            self.damage = TerminalDamage::Full;
            return;
        }

        match &mut self.damage {
            TerminalDamage::Full => {}
            TerminalDamage::None => {
                let mut dirty = DirtyRows::new();
                dirty.set_range(rows);
                self.damage = TerminalDamage::Rows(dirty);
            }
            TerminalDamage::Rows(dirty) => {
                dirty.set_range(rows);
            }
        }
    }

    fn wrap_if_needed(&mut self, width: usize) {
        if self.cursor.column + width > self.cols() {
            if self.auto_wrap {
                self.carriage_return();
                self.linefeed();
            } else {
                self.cursor.column = self.cols().saturating_sub(width.max(1));
                self.wrap_pending = false;
            }
        }
    }

    fn apply_combining_mark(&mut self, ch: char) {
        let target_col = self.cursor.column.saturating_sub(1);
        let row = self.cursor.row;
        if let Some(cell) = self.screen_mut().get_mut(target_col, row) {
            cell.text_mut().push(ch);
        }
        self.mark_row_dirty(row);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharsetSlot {
    G0,
    G1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecCharset {
    Ascii,
    DecSpecialGraphics,
}

fn map_dec_special_graphics(ch: char) -> char {
    match ch {
        '`' => '◆',
        'a' => '▒',
        'f' => '°',
        'g' => '±',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => ch,
    }
}

fn parse_extended_color(params: &[i64]) -> Option<(TerminalColor, usize)> {
    match params {
        [5, index, ..] => Some((TerminalColor::Palette(*index as u8), 2)),
        [2, r, g, b, ..] => Some((TerminalColor::Rgb(rgb(*r as u8, *g as u8, *b as u8)), 4)),
        _ => None,
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color4f {
    Color4f::new(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::{CharsetSlot, DecCharset, DirtyRows, TerminalDamage, TerminalState};
    use crate::terminal::input::KittyKeyboardFlags;
    use crate::terminal::{
        ClipboardRequestKind, ClipboardSelection, Hyperlink, style::TerminalColor,
    };

    #[test]
    fn linefeed_pushes_primary_screen_into_scrollback() {
        let mut state = TerminalState::new(4, 2);
        state.print_char('a');
        state.carriage_return();
        state.linefeed();
        state.print_char('b');
        state.carriage_return();
        state.linefeed();

        let snapshot = state.snapshot();

        assert_eq!(snapshot.scrollback_len, 1);
        assert_eq!(state.screen().row_text(0).trim(), "b");
    }

    #[test]
    fn alternate_screen_does_not_consume_primary_scrollback() {
        let mut state = TerminalState::new(3, 2);
        state.enter_alternate_screen();
        state.print_char('x');
        state.linefeed();
        state.linefeed();

        let snapshot = state.snapshot();

        assert!(snapshot.using_alternate_screen);
        assert_eq!(snapshot.scrollback_len, 0);
    }

    #[test]
    fn sgr_rgb_sets_foreground() {
        let mut state = TerminalState::new(2, 2);
        state.set_sgr(&[38, 2, 10, 20, 30]);

        match state.pen().colors().foreground.as_ref().unwrap() {
            TerminalColor::Rgb(color) => {
                assert!(color.r > 0.03 && color.r < 0.05);
                assert!(color.g > 0.07 && color.g < 0.09);
                assert!(color.b > 0.11 && color.b < 0.13);
            }
            color => panic!("expected rgb color, got {color:?}"),
        }
    }

    #[test]
    fn delayed_wrap_moves_next_print_to_following_line() {
        let mut state = TerminalState::new(2, 2);
        state.print_char('a');
        state.print_char('b');
        assert_eq!(state.cursor().row, 0);
        assert_eq!(state.cursor().column, 1);

        state.print_char('c');
        assert_eq!(state.screen().row_text(0), "ab");
        assert_eq!(state.screen().row_text(1).trim(), "c");
    }

    #[test]
    fn linefeed_outside_scroll_region_at_last_row_does_not_scroll_screen() {
        let mut state = TerminalState::new(4, 4);
        for (row, ch) in ['a', 'b', 'c', 'd'].into_iter().enumerate() {
            state.set_cursor_position(row, 0);
            state.print_char(ch);
        }

        state.set_scroll_region(0, 3);
        state.set_cursor_position(3, 0);
        state.linefeed();

        assert_eq!(state.screen().row_text(0), "a   ");
        assert_eq!(state.screen().row_text(1), "b   ");
        assert_eq!(state.screen().row_text(2), "c   ");
        assert_eq!(state.screen().row_text(3), "d   ");
        assert_eq!(state.cursor().row, 3);
    }

    #[test]
    fn insert_and_delete_lines_outside_scroll_region_are_ignored() {
        let mut state = TerminalState::new(3, 4);
        for row in 0..4 {
            state.set_cursor_position(row, 0);
            state.print_char(char::from_u32(b'a' as u32 + row as u32).unwrap());
        }

        state.set_scroll_region(0, 3);
        state.set_cursor_position(3, 0);
        state.insert_lines(1);
        state.delete_lines(1);

        assert_eq!(state.screen().row_text(0), "a  ");
        assert_eq!(state.screen().row_text(1), "b  ");
        assert_eq!(state.screen().row_text(2), "c  ");
        assert_eq!(state.screen().row_text(3), "d  ");
    }

    #[test]
    fn erase_uses_current_background_style() {
        let mut state = TerminalState::new(4, 1);
        state.set_sgr(&[44]);
        state.clear_line();

        let cell = state.screen().get(0, 0).unwrap();
        let style = cell.style.as_ref().expect("styled blank after erase");
        assert!(matches!(
            style.colors.background,
            Some(crate::terminal::style::TerminalColor::Palette(4))
        ));
    }

    #[test]
    fn cpr_is_relative_to_scroll_region_in_origin_mode() {
        let mut state = TerminalState::new(4, 5);
        state.set_scroll_region(1, 4);
        state.use_private_mode(6, true);
        state.set_cursor_position(1, 2);
        state.report_device_status(6);

        let response = state.take_pending_responses().pop().unwrap();
        assert_eq!(response, b"\x1b[2;3R");
    }

    #[test]
    fn dec_special_graphics_maps_line_drawing_chars() {
        let mut state = TerminalState::new(2, 1);
        state.designate_charset(CharsetSlot::G0, DecCharset::DecSpecialGraphics);
        state.print_char('q');
        state.print_char('x');

        assert_eq!(state.screen().row_text(0), "─│");
    }

    #[test]
    fn decrqm_reports_known_private_modes() {
        let mut state = TerminalState::new(2, 1);
        state.use_private_mode(1, true);
        state.use_private_mode(2004, true);
        state.use_private_mode(2026, true);
        state.report_private_mode(1);
        state.report_private_mode(25);
        state.report_private_mode(2004);
        state.report_private_mode(2026);

        let responses = state.take_pending_responses();
        assert_eq!(responses[0], b"\x1b[?1;1$y");
        assert_eq!(responses[1], b"\x1b[?25;1$y");
        assert_eq!(responses[2], b"\x1b[?2004;1$y");
        assert_eq!(responses[3], b"\x1b[?2026;1$y");
    }

    #[test]
    fn tracks_synchronized_updates_state() {
        let mut state = TerminalState::new(2, 1);
        assert!(!state.synchronized_updates_active());

        state.set_synchronized_updates(true);
        assert!(state.synchronized_updates_active());
        assert!(state.snapshot().synchronized_updates);

        state.set_synchronized_updates(false);
        assert!(!state.synchronized_updates_active());
    }

    #[test]
    fn tracks_current_hyperlink_and_clipboard_queue() {
        let mut state = TerminalState::new(4, 1);
        state.set_current_hyperlink(Some(Hyperlink {
            id: Some("id-1".into()),
            uri: "https://example.com".into(),
        }));
        state.print_char('x');
        let cell = state.screen().get(0, 0).unwrap();
        assert_eq!(cell.hyperlink.as_ref().unwrap().uri, "https://example.com");

        state.queue_clipboard_set(ClipboardSelection::Clipboard, "hello".into());
        let requests = state.take_pending_clipboard_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].kind, ClipboardRequestKind::Set("hello".into()));

        state.queue_clipboard_query(ClipboardSelection::Primary);
        let requests = state.take_pending_clipboard_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].selection, ClipboardSelection::Primary);
        assert_eq!(requests[0].kind, ClipboardRequestKind::Query);
    }

    #[test]
    fn kitty_keyboard_state_tracks_current_flags_and_stack() {
        let mut state = TerminalState::new(2, 1);
        assert_eq!(state.input_settings().kitty_keyboard_flags.bits(), 0);

        state.push_kitty_keyboard_flags(1);
        assert_eq!(
            state.input_settings().kitty_keyboard_flags.bits(),
            KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES
        );

        state.set_kitty_keyboard_flags(1, 3);
        assert_eq!(state.input_settings().kitty_keyboard_flags.bits(), 0);

        state.pop_kitty_keyboard_flags(1);
        assert_eq!(state.input_settings().kitty_keyboard_flags.bits(), 0);
    }

    #[test]
    fn kitty_keyboard_stack_pop_is_bounded_and_masked() {
        let mut state = TerminalState::new(2, 1);

        state.push_kitty_keyboard_flags(255);
        assert_eq!(
            state.input_settings().kitty_keyboard_flags.bits(),
            KittyKeyboardFlags::DISAMBIGUATE_ESCAPE_CODES
        );

        state.pop_kitty_keyboard_flags(2);
        assert_eq!(state.input_settings().kitty_keyboard_flags.bits(), 0);
    }

    #[test]
    fn dirty_row_tracking_degrades_to_full_for_tall_terminals() {
        let mut state = TerminalState::new(2, DirtyRows::MAX_ROWS + 1);
        let _ = state.take_damage();

        state.set_cursor_position(DirtyRows::MAX_ROWS, 0);
        state.print_char('x');

        assert_eq!(state.take_damage(), TerminalDamage::Full);
    }

    #[test]
    fn dirty_row_tracking_degrades_to_full_for_ranges_past_capacity() {
        let mut state = TerminalState::new(2, DirtyRows::MAX_ROWS);
        let _ = state.take_damage();

        state.set_cursor_position(DirtyRows::MAX_ROWS - 1, 0);
        state.clear_from_cursor();

        assert_eq!(state.take_damage(), TerminalDamage::Full);
    }
}
