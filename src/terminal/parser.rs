//! VT100/xterm escape sequence parser.
//! Uses the `vte` crate for state machine parsing, with full SGR, CSI, ESC, and OSC support.

use super::cell::{Cell, CellFlags, ColorSource, PackedColor};
use super::damage::DamageTracker;
use super::grid::Grid;
use super::mouse::{MouseMode, MouseState};
use super::theme::Theme;

// ---------------------------------------------------------------------------
// Color helpers
// ---------------------------------------------------------------------------

/// Parse an extended color specification from a flat parameter slice.
///
/// Handles both semicolon-separated (`38;5;idx` / `38;2;r;g;b`) and
/// colon-separated subparameter forms.  Returns `(color, source, number of
/// params consumed)`.
fn parse_extended_color(params: &[u16], theme: &Theme) -> (PackedColor, ColorSource, usize) {
    if params.is_empty() {
        return (PackedColor(0), ColorSource::Default, 0);
    }
    match params[0] {
        // 256-color: ;5;idx
        5 if params.len() >= 2 => {
            let idx = params[1] as u8;
            (theme.palette(idx), ColorSource::Palette, 2)
        }
        // True color: ;2;r;g;b
        2 if params.len() >= 4 => {
            let r = params[1] as u8;
            let g = params[2] as u8;
            let b = params[3] as u8;
            (PackedColor::new(r, g, b), ColorSource::Rgb, 4)
        }
        _ => (PackedColor(0), ColorSource::Default, 0),
    }
}

fn parse_osc_color_component(component: &str) -> Option<u8> {
    if component.is_empty() || !component.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let digits = component.len();
    let value = u32::from_str_radix(component, 16).ok()?;
    let max = (1u32 << (digits * 4)) - 1;
    Some(((value * 255 + max / 2) / max) as u8)
}

fn parse_osc_color(spec: &str) -> Option<PackedColor> {
    let spec = spec.trim();

    if let Some(rgb) = spec.strip_prefix("rgb:") {
        let mut parts = rgb.split('/');
        let r = parse_osc_color_component(parts.next()?)?;
        let g = parse_osc_color_component(parts.next()?)?;
        let b = parse_osc_color_component(parts.next()?)?;
        if parts.next().is_some() {
            return None;
        }
        return Some(PackedColor::new(r, g, b));
    }

    let hex = spec.strip_prefix('#').unwrap_or(spec);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some(PackedColor::new(r, g, b))
}

fn format_osc_color(color: PackedColor) -> String {
    format!(
        "rgb:{0:02x}{0:02x}/{1:02x}{1:02x}/{2:02x}{2:02x}",
        color.r(),
        color.g(),
        color.b()
    )
}

// ---------------------------------------------------------------------------
// Public parser
// ---------------------------------------------------------------------------

/// VT100/xterm terminal parser wrapping the `vte` state machine.
pub struct TerminalParser {
    parser: vte::Parser,
    current_attrs: Cell,
    title: Option<String>,
    responses: Vec<Vec<u8>>,
    theme: Theme,
    base_theme: Theme,
    theme_changed: bool,
    mouse_state: MouseState,
    application_cursor_keys: bool,
    origin_mode: bool,
}

#[allow(dead_code)]
impl TerminalParser {
    pub fn new() -> Self {
        let theme = Theme::default();
        let base_theme = theme.clone();
        let mut default_cell = Cell::default();
        default_cell.fg = theme.foreground;
        default_cell.bg = theme.background;
        Self {
            parser: vte::Parser::new(),
            current_attrs: default_cell,
            title: None,
            responses: Vec::new(),
            theme,
            base_theme,
            theme_changed: false,
            mouse_state: MouseState::new(),
            application_cursor_keys: false,
            origin_mode: false,
        }
    }

    pub fn with_theme(theme: Theme) -> Self {
        let base_theme = theme.clone();
        let mut default_cell = Cell::default();
        default_cell.fg = theme.foreground;
        default_cell.bg = theme.background;
        Self {
            parser: vte::Parser::new(),
            current_attrs: default_cell,
            title: None,
            responses: Vec::new(),
            theme,
            base_theme,
            theme_changed: false,
            mouse_state: MouseState::new(),
            application_cursor_keys: false,
            origin_mode: false,
        }
    }

    /// Feed raw bytes from the PTY into the parser.
    pub fn advance(&mut self, grid: &mut Grid, damage: &mut DamageTracker, bytes: &[u8]) {
        let mut performer = Performer {
            grid,
            damage,
            current_attrs: &mut self.current_attrs,
            title: &mut self.title,
            responses: &mut self.responses,
            theme: &mut self.theme,
            base_theme: &self.base_theme,
            theme_changed: &mut self.theme_changed,
            mouse_state: &mut self.mouse_state,
            application_cursor_keys: &mut self.application_cursor_keys,
            origin_mode: &mut self.origin_mode,
        };
        self.parser.advance(&mut performer, bytes);
    }

    /// Drain and return all pending response bytes (e.g. DSR, DA answers).
    pub fn take_responses(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.responses)
    }

    /// Get the current window title set by OSC sequences.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Get a reference to the current theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    pub fn take_theme_changed(&mut self) -> bool {
        std::mem::take(&mut self.theme_changed)
    }

    /// Get a reference to the mouse tracking state.
    pub fn mouse_state(&self) -> &MouseState {
        &self.mouse_state
    }

    /// Whether application cursor key mode (DECCKM) is active.
    pub fn application_cursor_keys(&self) -> bool {
        self.application_cursor_keys
    }
}

// ---------------------------------------------------------------------------
// Performer – implements vte::Perform
// ---------------------------------------------------------------------------

struct Performer<'a> {
    grid: &'a mut Grid,
    damage: &'a mut DamageTracker,
    current_attrs: &'a mut Cell,
    title: &'a mut Option<String>,
    responses: &'a mut Vec<Vec<u8>>,
    theme: &'a mut Theme,
    base_theme: &'a Theme,
    theme_changed: &'a mut bool,
    mouse_state: &'a mut MouseState,
    application_cursor_keys: &'a mut bool,
    origin_mode: &'a mut bool,
}

impl<'a> Performer<'a> {
    fn sync_default_attrs_with_theme(&mut self) {
        if self.current_attrs.fg_src == ColorSource::Default {
            self.current_attrs.fg = self.theme.foreground;
        }
        if self.current_attrs.bg_src == ColorSource::Default {
            self.current_attrs.bg = self.theme.background;
        }
    }

    fn mark_theme_changed(&mut self) {
        *self.theme_changed = true;
    }

    fn osc_terminator(bell_terminated: bool) -> &'static str {
        if bell_terminated {
            "\x07"
        } else {
            "\x1b\\"
        }
    }

    fn respond_with_dynamic_color(&mut self, code: u16, color: PackedColor, bell_terminated: bool) {
        let response = format!(
            "\x1b]{};{}{}",
            code,
            format_osc_color(color),
            Self::osc_terminator(bell_terminated)
        );
        self.responses.push(response.into_bytes());
    }

    fn set_dynamic_color(&mut self, code: u16, color: PackedColor) {
        let changed = match code {
            10 => {
                let changed = self.theme.foreground != color;
                self.theme.foreground = color;
                changed
            }
            11 => {
                let changed = self.theme.background != color;
                self.theme.background = color;
                changed
            }
            12 => {
                let changed = self.theme.cursor != color;
                self.theme.cursor = color;
                changed
            }
            _ => false,
        };

        if changed {
            self.sync_default_attrs_with_theme();
            self.mark_theme_changed();
        }
    }

    fn reset_dynamic_color(&mut self, code: u16) {
        match code {
            110 => self.set_dynamic_color(10, self.base_theme.foreground),
            111 => self.set_dynamic_color(11, self.base_theme.background),
            112 => self.set_dynamic_color(12, self.base_theme.cursor),
            _ => {}
        }
    }

    fn query_dynamic_color(&mut self, code: u16, bell_terminated: bool) {
        let color = match code {
            10 => Some(self.theme.foreground),
            11 => Some(self.theme.background),
            12 => Some(self.theme.cursor),
            _ => None,
        };
        if let Some(color) = color {
            self.respond_with_dynamic_color(code, color, bell_terminated);
        }
    }

    fn set_palette_color(&mut self, index: u8, color: PackedColor) {
        if self.theme.palette(index) != color {
            self.theme.set_palette(index, color);
            self.mark_theme_changed();
        }
    }

    fn reset_palette_color(&mut self, index: u8) {
        self.set_palette_color(index, self.base_theme.palette(index));
    }

    // -----------------------------------------------------------------------
    // SGR attribute handling
    // -----------------------------------------------------------------------

    fn handle_sgr(&mut self, params: &vte::Params) {
        // Collect all params into a flat list so we can index freely.
        // Each "parameter group" from vte may contain subparameters (colon-separated).
        // We flatten them for uniform handling.
        let mut flat = [0u16; 32];
        let mut flat_len = 0usize;
        for group in params.iter() {
            for &val in group {
                if flat_len < flat.len() {
                    flat[flat_len] = val;
                    flat_len += 1;
                }
            }
        }

        if flat_len == 0 {
            // Bare ESC[m == reset.
            self.sgr_reset();
            return;
        }

        let mut i = 0;
        while i < flat_len {
            let code = flat[i];
            i += 1;
            match code {
                0 => self.sgr_reset(),

                // Set attributes
                1 => self.current_attrs.flags.insert(CellFlags::BOLD),
                2 => self.current_attrs.flags.insert(CellFlags::DIM),
                3 => self.current_attrs.flags.insert(CellFlags::ITALIC),
                4 => self.current_attrs.flags.insert(CellFlags::UNDERLINE),
                5 | 6 => self.current_attrs.flags.insert(CellFlags::BLINK),
                7 => self.current_attrs.flags.insert(CellFlags::REVERSE),
                8 => self.current_attrs.flags.insert(CellFlags::HIDDEN),
                9 => self.current_attrs.flags.insert(CellFlags::STRIKETHROUGH),

                // Remove attributes
                21 => self.current_attrs.flags.remove(CellFlags::BOLD), // (or double underline – we treat as unbold)
                22 => {
                    self.current_attrs.flags.remove(CellFlags::BOLD);
                    self.current_attrs.flags.remove(CellFlags::DIM);
                }
                23 => self.current_attrs.flags.remove(CellFlags::ITALIC),
                24 => self.current_attrs.flags.remove(CellFlags::UNDERLINE),
                25 => self.current_attrs.flags.remove(CellFlags::BLINK),
                27 => self.current_attrs.flags.remove(CellFlags::REVERSE),
                28 => self.current_attrs.flags.remove(CellFlags::HIDDEN),
                29 => self.current_attrs.flags.remove(CellFlags::STRIKETHROUGH),

                // Standard foreground 30–37
                30..=37 => {
                    self.current_attrs.fg = self.theme.palette((code - 30) as u8);
                    self.current_attrs.fg_src = ColorSource::Palette;
                }
                // Extended foreground
                38 => {
                    let (color, src, consumed) =
                        parse_extended_color(&flat[i..flat_len], self.theme);
                    if consumed > 0 {
                        self.current_attrs.fg = color;
                        self.current_attrs.fg_src = src;
                        i += consumed;
                    }
                }
                // Default foreground
                39 => {
                    self.current_attrs.fg = self.theme.foreground;
                    self.current_attrs.fg_src = ColorSource::Default;
                }

                // Standard background 40–47
                40..=47 => {
                    self.current_attrs.bg = self.theme.palette((code - 40) as u8);
                    self.current_attrs.bg_src = ColorSource::Palette;
                }
                // Extended background
                48 => {
                    let (color, src, consumed) =
                        parse_extended_color(&flat[i..flat_len], self.theme);
                    if consumed > 0 {
                        self.current_attrs.bg = color;
                        self.current_attrs.bg_src = src;
                        i += consumed;
                    }
                }
                // Default background
                49 => {
                    self.current_attrs.bg = self.theme.background;
                    self.current_attrs.bg_src = ColorSource::Default;
                }

                // Bright foreground 90–97
                90..=97 => {
                    self.current_attrs.fg = self.theme.palette((code - 90 + 8) as u8);
                    self.current_attrs.fg_src = ColorSource::Palette;
                }

                // Bright background 100–107
                100..=107 => {
                    self.current_attrs.bg = self.theme.palette((code - 100 + 8) as u8);
                    self.current_attrs.bg_src = ColorSource::Palette;
                }

                _ => {} // unrecognised – ignore
            }
        }
    }

    fn sgr_reset(&mut self) {
        self.current_attrs.flags = CellFlags::empty();
        self.current_attrs.fg = self.theme.foreground;
        self.current_attrs.bg = self.theme.background;
        self.current_attrs.fg_src = ColorSource::Default;
        self.current_attrs.bg_src = ColorSource::Default;
    }

    // -----------------------------------------------------------------------
    // CSI dispatch helpers
    // -----------------------------------------------------------------------

    /// Extract the first parameter, defaulting to `default` when absent or 0.
    fn param(params: &vte::Params, idx: usize, default: u16) -> u16 {
        params
            .iter()
            .nth(idx)
            .and_then(|p| p.first().copied())
            .map(|v| if v == 0 { default } else { v })
            .unwrap_or(default)
    }

    /// Extract the first parameter, defaulting to `default` when absent (0 IS valid).
    fn param_zero_ok(params: &vte::Params, idx: usize, default: u16) -> u16 {
        params
            .iter()
            .nth(idx)
            .and_then(|p| p.first().copied())
            .unwrap_or(default)
    }

    #[inline]
    fn row_bounds(&self) -> (usize, usize) {
        if *self.origin_mode {
            self.grid.scroll_region()
        } else {
            (0, self.grid.rows().saturating_sub(1))
        }
    }

    #[inline]
    fn absolute_row_param(&self, row: usize) -> usize {
        let (min_row, max_row) = self.row_bounds();
        if *self.origin_mode {
            (min_row + row).min(max_row)
        } else {
            row.min(max_row)
        }
    }

    // -----------------------------------------------------------------------
    // BCE (Background Color Erase) blank cell
    // -----------------------------------------------------------------------

    /// Build a blank cell that inherits the current SGR background color.
    /// Per VT220/xterm spec, erased cells take on the current background.
    fn blank_cell(&self) -> Cell {
        Cell {
            ch: ' ' as u32,
            flags: CellFlags::empty(),
            fg_src: ColorSource::Default,
            bg_src: self.current_attrs.bg_src,
            fg: self.theme.foreground,
            bg: self.current_attrs.bg,
        }
    }

    // -----------------------------------------------------------------------
    // Insert / Delete lines (IL, DL)
    // -----------------------------------------------------------------------

    fn insert_lines(&mut self, count: usize) {
        let blank = self.blank_cell();
        let row = self.grid.cursor_row();
        self.grid.insert_lines_at_with(row, count, blank);
        self.damage.mark_all();
    }

    fn delete_lines(&mut self, count: usize) {
        let blank = self.blank_cell();
        let row = self.grid.cursor_row();
        self.grid.delete_lines_at_with(row, count, blank);
        self.damage.mark_all();
    }

    // -----------------------------------------------------------------------
    // Erase characters (ECH)
    // -----------------------------------------------------------------------

    fn erase_chars(&mut self, count: usize) {
        let blank = self.blank_cell();
        self.grid.erase_chars_with(count, blank);
        self.damage.mark_row(self.grid.cursor_row());
    }
}

// ---------------------------------------------------------------------------
// vte::Perform implementation
// ---------------------------------------------------------------------------

/// Mask to strip positional flags when copying text attributes to cells.
const PRINT_FLAGS_MASK: CellFlags = CellFlags::from_bits_truncate(
    !(CellFlags::WIDE.bits() | CellFlags::WIDE_SPACER.bits() | CellFlags::DIRTY.bits()),
);

impl<'a> vte::Perform for Performer<'a> {
    // ------ Print a visible character ------------------------------------
    #[inline]
    fn print(&mut self, c: char) {
        let col = self.grid.cursor_col();
        let _row = self.grid.cursor_row();
        let cols = self.grid.cols();

        // Determine character width
        let width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);

        // Handle autowrap: if we're past the last column, or if a wide character
        // would start in the final column, wrap before printing.
        if col >= cols || (width == 2 && col + 1 >= cols) {
            self.grid.carriage_return();
            let blank = self.blank_cell();
            self.grid.linefeed_with(blank);
        }

        let col = self.grid.cursor_col();
        let row = self.grid.cursor_row();

        if col >= cols {
            return; // safety
        }

        // Write the cell
        let cell = self.grid.cell_mut(col, row);
        cell.ch = c as u32;
        cell.flags = self.current_attrs.flags & PRINT_FLAGS_MASK;
        cell.fg = self.current_attrs.fg;
        cell.bg = self.current_attrs.bg;
        cell.fg_src = self.current_attrs.fg_src;
        cell.bg_src = self.current_attrs.bg_src;
        cell.mark_dirty();

        if width == 2 {
            cell.flags.insert(CellFlags::WIDE);
            // Place spacer in next column
            if col + 1 < cols {
                let spacer = self.grid.cell_mut(col + 1, row);
                *spacer = Cell::default();
                spacer.flags.insert(CellFlags::WIDE_SPACER);
                spacer.mark_dirty();
                self.grid.set_cursor_col(col + 2);
            } else {
                self.grid.set_cursor_col(col + 1);
            }
        } else {
            self.grid.set_cursor_col(col + 1);
        }

        let _ = self.grid.row_mut(row);
        self.damage.mark_row(row);
    }

    // ------ Execute a C0 control byte ------------------------------------
    fn execute(&mut self, byte: u8) {
        match byte {
            // BEL
            0x07 => { /* bell – ignore for now */ }
            // BS – backspace
            0x08 => {
                if self.grid.cursor_col() > 0 {
                    self.grid.set_cursor_col(self.grid.cursor_col() - 1);
                }
            }
            // HT – horizontal tab
            0x09 => {
                let col = self.grid.cursor_col();
                let cols = self.grid.cols();
                // Advance to next tab stop (every DEFAULT_TAB_WIDTH columns).
                let next =
                    ((col / super::grid::DEFAULT_TAB_WIDTH) + 1) * super::grid::DEFAULT_TAB_WIDTH;
                self.grid.set_cursor_col(next.min(cols.saturating_sub(1)));
            }
            // LF, VT, FF
            0x0A | 0x0B | 0x0C => {
                let blank = self.blank_cell();
                self.grid.linefeed_with(blank);
                self.damage.mark_all();
            }
            // CR
            0x0D => {
                self.grid.carriage_return();
            }
            _ => {}
        }
    }

    // ------ CSI dispatch -------------------------------------------------
    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let has_question = intermediates.contains(&b'?');

        match action {
            // -- Cursor movement ------------------------------------------
            // CUU – Cursor Up
            'A' => {
                let n = Self::param(params, 0, 1) as usize;
                let (min_row, _) = self.row_bounds();
                let row = self.grid.cursor_row();
                self.grid.set_cursor_row(row.saturating_sub(n).max(min_row));
            }
            // CUD – Cursor Down
            'B' => {
                let n = Self::param(params, 0, 1) as usize;
                let (_, max_row) = self.row_bounds();
                let row = self.grid.cursor_row();
                self.grid.set_cursor_row((row + n).min(max_row));
            }
            // CUF – Cursor Forward
            'C' => {
                let n = Self::param(params, 0, 1) as usize;
                let col = self.grid.cursor_col();
                let max = self.grid.cols().saturating_sub(1);
                self.grid.set_cursor_col((col + n).min(max));
            }
            // CUB – Cursor Back
            'D' => {
                let n = Self::param(params, 0, 1) as usize;
                let col = self.grid.cursor_col();
                self.grid.set_cursor_col(col.saturating_sub(n));
            }
            // CNL – Cursor Next Line
            'E' => {
                let n = Self::param(params, 0, 1) as usize;
                let (_, max_row) = self.row_bounds();
                let row = self.grid.cursor_row();
                self.grid.set_cursor_row((row + n).min(max_row));
                self.grid.set_cursor_col(0);
            }
            // CPL – Cursor Previous Line
            'F' => {
                let n = Self::param(params, 0, 1) as usize;
                let (min_row, _) = self.row_bounds();
                let row = self.grid.cursor_row();
                self.grid.set_cursor_row(row.saturating_sub(n).max(min_row));
                self.grid.set_cursor_col(0);
            }
            // CHA – Cursor Horizontal Absolute
            'G' => {
                let n = Self::param(params, 0, 1) as usize;
                let max = self.grid.cols().saturating_sub(1);
                self.grid.set_cursor_col((n.saturating_sub(1)).min(max));
            }
            // CUP / HVP – Cursor Position
            'H' | 'f' => {
                let row = Self::param(params, 0, 1) as usize;
                let col = Self::param(params, 1, 1) as usize;
                self.grid.set_cursor(
                    col.saturating_sub(1),
                    self.absolute_row_param(row.saturating_sub(1)),
                );
            }
            // ED – Erase Display
            'J' => {
                let blank = self.blank_cell();
                let mode = Self::param_zero_ok(params, 0, 0);
                match mode {
                    0 => self.grid.erase_below_with(blank),
                    1 => self.grid.erase_above_with(blank),
                    2 | 3 => self.grid.erase_all_with(blank),
                    _ => {}
                }
                self.damage.mark_all();
            }
            // EL – Erase in Line
            'K' => {
                let blank = self.blank_cell();
                let mode = Self::param_zero_ok(params, 0, 0);
                match mode {
                    0 => self.grid.erase_to_end_of_line_with(blank),
                    1 => self.grid.erase_to_start_of_line_with(blank),
                    2 => self.grid.erase_line_with(blank),
                    _ => {}
                }
                self.damage.mark_row(self.grid.cursor_row());
            }
            // IL – Insert Lines
            'L' => {
                let n = Self::param(params, 0, 1) as usize;
                self.insert_lines(n);
            }
            // DL – Delete Lines
            'M' => {
                let n = Self::param(params, 0, 1) as usize;
                self.delete_lines(n);
            }
            // DCH – Delete Characters
            'P' => {
                let n = Self::param(params, 0, 1) as usize;
                let blank = self.blank_cell();
                self.grid.delete_chars_with(n, blank);
                self.damage.mark_row(self.grid.cursor_row());
            }
            // ICH – Insert Characters
            '@' => {
                let n = Self::param(params, 0, 1) as usize;
                let blank = self.blank_cell();
                self.grid.insert_chars_with(n, blank);
                self.damage.mark_row(self.grid.cursor_row());
            }
            // ECH – Erase Characters
            'X' => {
                let n = Self::param(params, 0, 1) as usize;
                self.erase_chars(n);
            }
            // SU – Scroll Up
            'S' => {
                let n = Self::param(params, 0, 1) as usize;
                let blank = self.blank_cell();
                self.grid.scroll_up_with(n, blank);
                self.damage.mark_all();
            }
            // SD – Scroll Down
            'T' => {
                let n = Self::param(params, 0, 1) as usize;
                let blank = self.blank_cell();
                self.grid.scroll_down_with(n, blank);
                self.damage.mark_all();
            }
            // VPA – Vertical Position Absolute
            'd' => {
                let n = Self::param(params, 0, 1) as usize;
                self.grid
                    .set_cursor_row(self.absolute_row_param(n.saturating_sub(1)));
            }
            // SGR – Select Graphic Rendition
            'm' => {
                self.handle_sgr(params);
            }
            // DSR – Device Status Report
            'n' => {
                let mode = Self::param_zero_ok(params, 0, 0);
                if mode == 6 {
                    // Respond with cursor position (1-based). In origin mode, the row is
                    // reported relative to the active scroll region.
                    let row = if *self.origin_mode {
                        let (top, _) = self.grid.scroll_region();
                        self.grid.cursor_row().saturating_sub(top) + 1
                    } else {
                        self.grid.cursor_row() + 1
                    };
                    let col = self.grid.cursor_col() + 1;
                    let response = format!("\x1b[{};{}R", row, col);
                    self.responses.push(response.into_bytes());
                }
            }
            // DECSTBM – Set Scrolling Region
            'r' => {
                if !has_question {
                    let top = Self::param(params, 0, 1) as usize;
                    let bottom = Self::param(params, 1, self.grid.rows() as u16) as usize;
                    self.grid
                        .set_scroll_region(top.saturating_sub(1), bottom.saturating_sub(1));
                    // Move cursor to home after setting scroll region.
                    self.grid.set_cursor(0, 0);
                }
            }
            // Save / Restore cursor
            's' => {
                if !has_question && intermediates.is_empty() {
                    self.grid.save_cursor();
                }
            }
            'u' => {
                if !has_question && intermediates.is_empty() {
                    self.grid.restore_cursor();
                }
            }
            // DA – Device Attributes
            'c' => {
                if !has_question {
                    let mode = Self::param_zero_ok(params, 0, 0);
                    if mode == 0 {
                        // Respond as VT220.
                        self.responses.push(b"\x1b[?62;c".to_vec());
                    }
                }
            }
            // Window manipulation
            't' => {
                let mode = Self::param_zero_ok(params, 0, 0);
                match mode {
                    22 => {
                        // Push title – ignore (we don't maintain a title stack)
                    }
                    23 => {
                        // Pop title – ignore
                    }
                    _ => {
                        // Ignore other window manipulations
                    }
                }
            }
            // DECSET / DECRST – Private mode set / reset
            'h' => {
                if has_question {
                    for group in params.iter() {
                        if let Some(&mode) = group.first() {
                            match mode {
                                // DECCKM – application cursor keys
                                1 => *self.application_cursor_keys = true,
                                // DECOM – origin mode
                                6 => {
                                    *self.origin_mode = true;
                                    let (top, _) = self.grid.scroll_region();
                                    self.grid.set_cursor(0, top);
                                }
                                // DECTCEM – show cursor
                                25 => self.grid.set_cursor_visible(true),
                                // Alternate screen buffers
                                47 | 1047 => {
                                    self.grid.enter_alternate_screen();
                                    self.damage.mark_all();
                                }
                                // Save cursor + alternate screen
                                1049 => {
                                    self.grid.save_cursor();
                                    self.grid.enter_alternate_screen();
                                    self.damage.mark_all();
                                }
                                // Save cursor
                                1048 => self.grid.save_cursor(),
                                // Autowrap mode (parsing left as current default behaviour)
                                7 => {}
                                // Mouse tracking modes
                                1000 => self.mouse_state.mode = MouseMode::Click,
                                1002 => self.mouse_state.mode = MouseMode::Drag,
                                1003 => self.mouse_state.mode = MouseMode::Motion,
                                1006 => self.mouse_state.sgr = true,
                                _ => {}
                            }
                        }
                    }
                }
            }
            'l' => {
                if has_question {
                    for group in params.iter() {
                        if let Some(&mode) = group.first() {
                            match mode {
                                1 => *self.application_cursor_keys = false,
                                6 => {
                                    *self.origin_mode = false;
                                    self.grid.set_cursor(0, 0);
                                }
                                25 => self.grid.set_cursor_visible(false),
                                47 | 1047 => {
                                    self.grid.leave_alternate_screen();
                                    self.damage.mark_all();
                                }
                                1049 => {
                                    self.grid.leave_alternate_screen();
                                    self.grid.restore_cursor();
                                    self.damage.mark_all();
                                }
                                1048 => self.grid.restore_cursor(),
                                7 => {}
                                // Mouse tracking modes
                                1000 | 1002 | 1003 => self.mouse_state.mode = MouseMode::None,
                                1006 => self.mouse_state.sgr = false,
                                _ => {}
                            }
                        }
                    }
                }
            }

            _ => {
                // Unknown CSI – ignore
            }
        }
    }

    // ------ ESC dispatch -------------------------------------------------
    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        match (byte, intermediates) {
            // DECSC – Save Cursor
            (b'7', []) => {
                self.grid.save_cursor();
            }
            // DECRC – Restore Cursor
            (b'8', []) => {
                self.grid.restore_cursor();
            }
            // RI – Reverse Index (scroll down if cursor is at the top of scroll region)
            (b'M', []) => {
                let blank = self.blank_cell();
                self.grid.reverse_index_with(blank);
                self.damage.mark_all();
            }
            // IND – Index (like linefeed)
            (b'D', []) => {
                let blank = self.blank_cell();
                self.grid.linefeed_with(blank);
                self.damage.mark_all();
            }
            _ => {
                // Unhandled escape – ignore
            }
        }
    }

    // ------ OSC dispatch -------------------------------------------------
    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        if params.is_empty() {
            return;
        }

        // First param is the OSC command number as ASCII digits.
        let cmd = std::str::from_utf8(params[0])
            .ok()
            .and_then(|s| s.parse::<u16>().ok());

        match cmd {
            // 0: Set icon name + title, 1: Set icon name, 2: Set title
            Some(0) | Some(1) | Some(2) => {
                if params.len() >= 2 {
                    let title = params[1..]
                        .iter()
                        .filter_map(|param| std::str::from_utf8(param).ok())
                        .collect::<Vec<_>>()
                        .join(";");
                    *self.title = Some(title);
                }
            }
            // 4: Set/query palette color indexes.
            Some(4) => {
                for chunk in params[1..].chunks(2) {
                    let Some(index_bytes) = chunk.first() else {
                        continue;
                    };
                    let Some(index) = std::str::from_utf8(index_bytes)
                        .ok()
                        .and_then(|s| s.parse::<u8>().ok())
                    else {
                        continue;
                    };

                    let Some(color_bytes) = chunk.get(1) else {
                        continue;
                    };
                    let Some(color_spec) = std::str::from_utf8(color_bytes).ok() else {
                        continue;
                    };

                    if color_spec == "?" {
                        let response = format!(
                            "\x1b]4;{};{}{}",
                            index,
                            format_osc_color(self.theme.palette(index)),
                            Self::osc_terminator(bell_terminated)
                        );
                        self.responses.push(response.into_bytes());
                    } else if let Some(color) = parse_osc_color(color_spec) {
                        self.set_palette_color(index, color);
                    }
                }
            }
            // 10/11/12: foreground/background/cursor dynamic colors.
            Some(10) | Some(11) | Some(12) => {
                let mut dynamic_code = cmd.unwrap();
                for param in &params[1..] {
                    let Some(spec) = std::str::from_utf8(param).ok() else {
                        dynamic_code += 1;
                        continue;
                    };
                    if spec == "?" {
                        self.query_dynamic_color(dynamic_code, bell_terminated);
                    } else if let Some(color) = parse_osc_color(spec) {
                        self.set_dynamic_color(dynamic_code, color);
                    }
                    dynamic_code += 1;
                }
            }
            // 104: reset palette indexes.
            Some(104) => {
                if params.len() == 1 {
                    for index in 0u8..=255 {
                        self.reset_palette_color(index);
                    }
                } else {
                    for param in &params[1..] {
                        let Some(index) = std::str::from_utf8(param)
                            .ok()
                            .and_then(|s| s.parse::<u8>().ok())
                        else {
                            continue;
                        };
                        self.reset_palette_color(index);
                    }
                }
            }
            // 110/111/112: reset dynamic foreground/background/cursor.
            Some(110) | Some(111) | Some(112) => self.reset_dynamic_color(cmd.unwrap()),
            _ => {
                // Unhandled OSC – ignore
            }
        }
    }

    // Unused hooks – provide defaults
    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
    }
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup(cols: usize, rows: usize) -> (TerminalParser, Grid, DamageTracker) {
        (
            TerminalParser::new(),
            Grid::new(cols, rows),
            DamageTracker::new(rows),
        )
    }

    // === Original tests ===

    #[test]
    fn parses_alternate_screen_toggle() {
        let (mut p, mut g, mut d) = setup(4, 2);
        g.cell_mut(0, 0).ch = 'M' as u32;
        p.advance(&mut g, &mut d, b"\x1b[?1049h");
        assert_eq!(g.cell(0, 0).ch, ' ' as u32);
        p.advance(&mut g, &mut d, b"\x1b[?1049l");
        assert_eq!(g.cell(0, 0).ch, 'M' as u32);
    }

    #[test]
    fn decset_1049_saves_cursor_on_enter_and_restores_on_leave() {
        let (mut p, mut g, mut d) = setup(6, 4);
        g.set_cursor(3, 2);
        p.advance(&mut g, &mut d, b"\x1b[?1049h");
        assert_eq!((g.cursor_col(), g.cursor_row()), (0, 0));

        g.set_cursor(1, 1);
        p.advance(&mut g, &mut d, b"\x1b[?1049l");
        assert_eq!((g.cursor_col(), g.cursor_row()), (3, 2));
    }

    #[test]
    fn decset_1049_does_not_restore_cursor_modified_only_in_alt_screen() {
        let (mut p, mut g, mut d) = setup(6, 4);
        g.set_cursor(4, 1);
        p.advance(&mut g, &mut d, b"\x1b[?1049h\x1b[?1048h");
        g.set_cursor(0, 3);

        p.advance(&mut g, &mut d, b"\x1b[?1049l");
        assert_eq!((g.cursor_col(), g.cursor_row()), (4, 1));
    }

    #[test]
    fn cpr_is_relative_to_scroll_region_in_origin_mode() {
        let (mut p, mut g, mut d) = setup(8, 6);
        p.advance(&mut g, &mut d, b"\x1b[2;5r\x1b[?6h\x1b[1;1H\x1b[6n");
        assert_eq!(p.take_responses(), vec![b"\x1b[1;1R".to_vec()]);
    }

    // === SGR Attribute Tests ===

    #[test]
    fn sgr_bold() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[1mA");
        assert!(g.cell(0, 0).flags.contains(CellFlags::BOLD));
        assert_eq!(g.cell(0, 0).ch, 'A' as u32);
    }

    #[test]
    fn sgr_italic() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[3mB");
        assert!(g.cell(0, 0).flags.contains(CellFlags::ITALIC));
    }

    #[test]
    fn sgr_underline() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[4mC");
        assert!(g.cell(0, 0).flags.contains(CellFlags::UNDERLINE));
    }

    #[test]
    fn sgr_reverse() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[7mD");
        assert!(g.cell(0, 0).flags.contains(CellFlags::REVERSE));
    }

    #[test]
    fn sgr_strikethrough() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[9mE");
        assert!(g.cell(0, 0).flags.contains(CellFlags::STRIKETHROUGH));
    }

    #[test]
    fn sgr_hidden() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[8mF");
        assert!(g.cell(0, 0).flags.contains(CellFlags::HIDDEN));
    }

    #[test]
    fn sgr_dim() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[2mG");
        assert!(g.cell(0, 0).flags.contains(CellFlags::DIM));
    }

    #[test]
    fn sgr_reset_clears_all() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[1;3;4;7mX\x1b[0mY");
        let x = g.cell(0, 0);
        assert!(x.flags.contains(CellFlags::BOLD));
        assert!(x.flags.contains(CellFlags::ITALIC));
        let y = g.cell(1, 0);
        assert!(!y.flags.contains(CellFlags::BOLD));
        assert!(!y.flags.contains(CellFlags::ITALIC));
        assert!(!y.flags.contains(CellFlags::UNDERLINE));
        assert!(!y.flags.contains(CellFlags::REVERSE));
    }

    #[test]
    fn sgr_remove_bold_dim() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[1;2mA\x1b[22mB");
        assert!(g.cell(0, 0).flags.contains(CellFlags::BOLD));
        assert!(g.cell(0, 0).flags.contains(CellFlags::DIM));
        assert!(!g.cell(1, 0).flags.contains(CellFlags::BOLD));
        assert!(!g.cell(1, 0).flags.contains(CellFlags::DIM));
    }

    #[test]
    fn sgr_multiple_params() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[1;3;9mZ");
        let c = g.cell(0, 0);
        assert!(c.flags.contains(CellFlags::BOLD));
        assert!(c.flags.contains(CellFlags::ITALIC));
        assert!(c.flags.contains(CellFlags::STRIKETHROUGH));
    }

    #[test]
    fn sgr_standard_fg_color() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[31mR");
        assert_eq!(g.cell(0, 0).fg, p.theme().palette(1));
    }

    #[test]
    fn sgr_standard_bg_color() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[42mG");
        assert_eq!(g.cell(0, 0).bg, p.theme().palette(2));
    }

    #[test]
    fn sgr_bright_fg_color() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[91mR");
        assert_eq!(g.cell(0, 0).fg, p.theme().palette(9));
    }

    #[test]
    fn sgr_bright_bg_color() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[104mB");
        assert_eq!(g.cell(0, 0).bg, p.theme().palette(12));
    }

    #[test]
    fn sgr_256_color_fg() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[38;5;196mX");
        assert_eq!(g.cell(0, 0).fg, p.theme().palette(196));
    }

    #[test]
    fn sgr_truecolor_fg() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[38;2;255;128;0mX");
        assert_eq!(g.cell(0, 0).fg, PackedColor::new(255, 128, 0));
    }

    #[test]
    fn sgr_default_fg_reset() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[31mA\x1b[39mB");
        assert_eq!(g.cell(1, 0).fg, p.theme().foreground);
    }

    #[test]
    fn sgr_default_bg_reset() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[41mA\x1b[49mB");
        assert_eq!(g.cell(1, 0).bg, p.theme().background);
    }

    // === CSI Cursor Movement Tests ===

    #[test]
    fn csi_cup_cursor_position() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[5;10H");
        assert_eq!(g.cursor_row(), 4);
        assert_eq!(g.cursor_col(), 9);
    }

    #[test]
    fn csi_cuu_cursor_up() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[6;1H\x1b[3A");
        assert_eq!(g.cursor_row(), 2);
    }

    #[test]
    fn csi_cud_cursor_down() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[1;1H\x1b[4B");
        assert_eq!(g.cursor_row(), 4);
    }

    #[test]
    fn csi_cuf_cursor_forward() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[1;1H\x1b[5C");
        assert_eq!(g.cursor_col(), 5);
    }

    #[test]
    fn csi_cub_cursor_back() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[1;10H\x1b[3D");
        assert_eq!(g.cursor_col(), 6);
    }

    #[test]
    fn csi_cha_cursor_horizontal_absolute() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[8G");
        assert_eq!(g.cursor_col(), 7);
    }

    #[test]
    fn csi_vpa_vertical_position() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[6d");
        assert_eq!(g.cursor_row(), 5);
    }

    #[test]
    fn csi_cnl_cursor_next_line() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[1;5H\x1b[2E");
        assert_eq!(g.cursor_row(), 2);
        assert_eq!(g.cursor_col(), 0);
    }

    #[test]
    fn csi_cpl_cursor_previous_line() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[5;5H\x1b[2F");
        assert_eq!(g.cursor_row(), 2);
        assert_eq!(g.cursor_col(), 0);
    }

    // === CSI Erase Tests ===

    #[test]
    fn csi_ed_erase_below() {
        let (mut p, mut g, mut d) = setup(5, 3);
        p.advance(&mut g, &mut d, b"ABCDE12345vwxyz");
        p.advance(&mut g, &mut d, b"\x1b[2;3H\x1b[J");
        assert_eq!(g.cell(0, 1).ch, '1' as u32);
        assert_eq!(g.cell(1, 1).ch, '2' as u32);
        assert_eq!(g.cell(2, 1).ch, ' ' as u32);
        assert_eq!(g.cell(0, 2).ch, ' ' as u32);
    }

    #[test]
    fn csi_ed_erase_all() {
        let (mut p, mut g, mut d) = setup(5, 3);
        p.advance(&mut g, &mut d, b"Hello");
        p.advance(&mut g, &mut d, b"\x1b[2J");
        assert_eq!(g.cell(0, 0).ch, ' ' as u32);
    }

    #[test]
    fn csi_el_erase_to_end() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"ABCDEFGHIJ");
        p.advance(&mut g, &mut d, b"\x1b[1;4H\x1b[K");
        assert_eq!(g.cell(2, 0).ch, 'C' as u32);
        assert_eq!(g.cell(3, 0).ch, ' ' as u32);
        assert_eq!(g.cell(9, 0).ch, ' ' as u32);
    }

    #[test]
    fn csi_el_erase_entire_line() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"ABCDEFGHIJ");
        p.advance(&mut g, &mut d, b"\x1b[1;5H\x1b[2K");
        for col in 0..10 {
            assert_eq!(g.cell(col, 0).ch, ' ' as u32);
        }
    }

    #[test]
    fn csi_ech_erase_characters() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"ABCDEFGHIJ");
        p.advance(&mut g, &mut d, b"\x1b[1;3H\x1b[3X");
        assert_eq!(g.cell(1, 0).ch, 'B' as u32);
        assert_eq!(g.cell(2, 0).ch, ' ' as u32);
        assert_eq!(g.cell(3, 0).ch, ' ' as u32);
        assert_eq!(g.cell(4, 0).ch, ' ' as u32);
        assert_eq!(g.cell(5, 0).ch, 'F' as u32);
    }

    // === CSI Insert/Delete ===

    #[test]
    fn csi_dch_delete_characters() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"ABCDE");
        p.advance(&mut g, &mut d, b"\x1b[1;2H\x1b[2P");
        assert_eq!(g.cell(0, 0).ch, 'A' as u32);
        assert_eq!(g.cell(1, 0).ch, 'D' as u32);
        assert_eq!(g.cell(2, 0).ch, 'E' as u32);
    }

    #[test]
    fn csi_ich_insert_characters() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"ABCDE");
        p.advance(&mut g, &mut d, b"\x1b[1;2H\x1b[1@");
        assert_eq!(g.cell(0, 0).ch, 'A' as u32);
        assert_eq!(g.cell(1, 0).ch, ' ' as u32);
        assert_eq!(g.cell(2, 0).ch, 'B' as u32);
    }

    #[test]
    fn csi_scroll_up() {
        let (mut p, mut g, mut d) = setup(3, 3);
        p.advance(&mut g, &mut d, b"AAA\r\nBBB\r\nCCC");
        p.advance(&mut g, &mut d, b"\x1b[1S");
        assert_eq!(g.cell(0, 0).ch, 'B' as u32);
        assert_eq!(g.cell(0, 1).ch, 'C' as u32);
        assert_eq!(g.cell(0, 2).ch, ' ' as u32);
    }

    #[test]
    fn csi_scroll_down() {
        let (mut p, mut g, mut d) = setup(3, 3);
        p.advance(&mut g, &mut d, b"AAA\r\nBBB\r\nCCC");
        p.advance(&mut g, &mut d, b"\x1b[1T");
        assert_eq!(g.cell(0, 0).ch, ' ' as u32);
        assert_eq!(g.cell(0, 1).ch, 'A' as u32);
        assert_eq!(g.cell(0, 2).ch, 'B' as u32);
    }

    // === Scroll Region ===

    #[test]
    fn csi_decstbm_scroll_region() {
        let (mut p, mut g, mut d) = setup(10, 10);
        p.advance(&mut g, &mut d, b"\x1b[3;7r");
        assert_eq!(g.scroll_region(), (2, 6));
        assert_eq!(g.cursor_col(), 0);
        assert_eq!(g.cursor_row(), 0);
    }

    // === C0 Controls ===

    #[test]
    fn c0_tab() {
        let (mut p, mut g, mut d) = setup(20, 2);
        p.advance(&mut g, &mut d, b"A\tB");
        assert_eq!(g.cell(0, 0).ch, 'A' as u32);
        assert_eq!(g.cell(8, 0).ch, 'B' as u32);
    }

    #[test]
    fn c0_backspace() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"AB\x08C");
        assert_eq!(g.cell(0, 0).ch, 'A' as u32);
        assert_eq!(g.cell(1, 0).ch, 'C' as u32);
    }

    #[test]
    fn wide_char_in_last_column_wraps_before_printing() {
        let (mut p, mut g, mut d) = setup(4, 3);
        p.advance(&mut g, &mut d, b"abc");
        p.advance(&mut g, &mut d, "好".as_bytes());

        assert_eq!(g.cell(0, 0).ch, 'a' as u32);
        assert_eq!(g.cell(1, 0).ch, 'b' as u32);
        assert_eq!(g.cell(2, 0).ch, 'c' as u32);
        assert_eq!(g.cell(3, 0).ch, ' ' as u32);

        assert_eq!(g.cell(0, 1).ch, '好' as u32);
        assert!(g.cell(0, 1).flags.contains(CellFlags::WIDE));
        assert!(g.cell(1, 1).flags.contains(CellFlags::WIDE_SPACER));
        assert_eq!((g.cursor_col(), g.cursor_row()), (2, 1));
    }

    #[test]
    fn c0_cr_lf() {
        let (mut p, mut g, mut d) = setup(10, 3);
        p.advance(&mut g, &mut d, b"AB\r\nCD");
        assert_eq!(g.cell(0, 0).ch, 'A' as u32);
        assert_eq!(g.cell(1, 0).ch, 'B' as u32);
        assert_eq!(g.cell(0, 1).ch, 'C' as u32);
        assert_eq!(g.cell(1, 1).ch, 'D' as u32);
    }

    // === ESC Sequences ===

    #[test]
    fn esc_reverse_index() {
        let (mut p, mut g, mut d) = setup(5, 3);
        p.advance(&mut g, &mut d, b"AAAAA\r\nBBBBB\r\nCCCCC");
        p.advance(&mut g, &mut d, b"\x1b[1;1H\x1bM");
        assert_eq!(g.cell(0, 0).ch, ' ' as u32);
        assert_eq!(g.cell(0, 1).ch, 'A' as u32);
    }

    #[test]
    fn esc_save_restore_cursor() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[5;8H\x1b7\x1b[1;1H\x1b8");
        assert_eq!(g.cursor_row(), 4);
        assert_eq!(g.cursor_col(), 7);
    }

    // === OSC ===

    #[test]
    fn osc_set_title() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b]0;MyTitle\x07");
        assert_eq!(p.title(), Some("MyTitle"));
    }

    #[test]
    fn osc_set_title_osc2() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b]2;AnotherTitle\x07");
        assert_eq!(p.title(), Some("AnotherTitle"));
    }

    #[test]
    fn osc_10_query_foreground() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b]10;?\x07");
        assert_eq!(
            p.take_responses(),
            vec![b"\x1b]10;rgb:d4d4/d4d4/d4d4\x07".to_vec()]
        );
    }

    #[test]
    fn osc_11_query_background() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b]11;?\x07");
        assert_eq!(
            p.take_responses(),
            vec![b"\x1b]11;rgb:1e1e/1e1e/1e1e\x07".to_vec()]
        );
    }

    #[test]
    fn osc_4_query_palette_color() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b]4;1;?\x07");
        assert_eq!(
            p.take_responses(),
            vec![b"\x1b]4;1;rgb:cccc/0000/0000\x07".to_vec()]
        );
    }

    #[test]
    fn osc_10_set_and_reset_foreground() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b]10;#112233\x07");
        assert_eq!(p.theme().foreground, PackedColor::new(0x11, 0x22, 0x33));
        assert!(p.take_theme_changed());

        p.advance(&mut g, &mut d, b"\x1b]110\x07");
        assert_eq!(p.theme().foreground, Theme::default().foreground);
        assert!(p.take_theme_changed());
    }

    #[test]
    fn osc_11_set_background_from_rgb_spec() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b]11;rgb:11/22/33\x07");
        assert_eq!(p.theme().background, PackedColor::new(0x11, 0x22, 0x33));
        assert!(p.take_theme_changed());
    }

    #[test]
    fn osc_4_set_palette_color() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b]4;1;#123456\x07");
        assert_eq!(p.theme().palette(1), PackedColor::new(0x12, 0x34, 0x56));
        assert!(p.take_theme_changed());
    }

    // === DECSET / DECRST ===

    #[test]
    fn decset_hide_show_cursor() {
        let (mut p, mut g, mut d) = setup(10, 2);
        assert!(g.cursor_visible());
        p.advance(&mut g, &mut d, b"\x1b[?25l");
        assert!(!g.cursor_visible());
        p.advance(&mut g, &mut d, b"\x1b[?25h");
        assert!(g.cursor_visible());
    }

    #[test]
    fn decset_mouse_click_mode() {
        let (mut p, mut g, mut d) = setup(10, 2);
        assert_eq!(p.mouse_state().mode, MouseMode::None);
        p.advance(&mut g, &mut d, b"\x1b[?1000h");
        assert_eq!(p.mouse_state().mode, MouseMode::Click);
        p.advance(&mut g, &mut d, b"\x1b[?1000l");
        assert_eq!(p.mouse_state().mode, MouseMode::None);
    }

    #[test]
    fn decset_mouse_sgr_mode() {
        let (mut p, mut g, mut d) = setup(10, 2);
        assert!(!p.mouse_state().sgr);
        p.advance(&mut g, &mut d, b"\x1b[?1006h");
        assert!(p.mouse_state().sgr);
        p.advance(&mut g, &mut d, b"\x1b[?1006l");
        assert!(!p.mouse_state().sgr);
    }

    // === Device Attributes ===

    #[test]
    fn da_response() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[c");
        let responses = p.take_responses();
        assert_eq!(responses, vec![b"\x1b[?62;c".to_vec()]);
    }

    // === Print / Autowrap ===

    #[test]
    fn print_basic_text() {
        let (mut p, mut g, mut d) = setup(5, 2);
        p.advance(&mut g, &mut d, b"Hello");
        assert_eq!(g.cell(0, 0).ch, 'H' as u32);
        assert_eq!(g.cell(4, 0).ch, 'o' as u32);
    }

    #[test]
    fn print_autowrap() {
        let (mut p, mut g, mut d) = setup(3, 2);
        p.advance(&mut g, &mut d, b"ABCDE");
        assert_eq!(g.cell(0, 0).ch, 'A' as u32);
        assert_eq!(g.cell(2, 0).ch, 'C' as u32);
        assert_eq!(g.cell(0, 1).ch, 'D' as u32);
        assert_eq!(g.cell(1, 1).ch, 'E' as u32);
    }

    // === Save/Restore Cursor via CSI ===

    #[test]
    fn csi_save_restore_cursor() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[4;6H\x1b[s\x1b[1;1H\x1b[u");
        assert_eq!(g.cursor_row(), 3);
        assert_eq!(g.cursor_col(), 5);
    }

    // === DSR ===

    #[test]
    fn dsr_cursor_position_report() {
        let (mut p, mut g, mut d) = setup(20, 10);
        p.advance(&mut g, &mut d, b"\x1b[3;7H\x1b[6n");
        let responses = p.take_responses();
        assert_eq!(responses, vec![b"\x1b[3;7R".to_vec()]);
    }

    // === Insert/Delete Lines ===

    #[test]
    fn csi_il_insert_lines() {
        let (mut p, mut g, mut d) = setup(3, 4);
        p.advance(&mut g, &mut d, b"AAA\r\nBBB\r\nCCC\r\nDDD");
        p.advance(&mut g, &mut d, b"\x1b[2;1H\x1b[1L");
        assert_eq!(g.cell(0, 0).ch, 'A' as u32);
        assert_eq!(g.cell(0, 1).ch, ' ' as u32);
        assert_eq!(g.cell(0, 2).ch, 'B' as u32);
        assert_eq!(g.cell(0, 3).ch, 'C' as u32);
    }

    #[test]
    fn csi_dl_delete_lines() {
        let (mut p, mut g, mut d) = setup(3, 4);
        p.advance(&mut g, &mut d, b"AAA\r\nBBB\r\nCCC\r\nDDD");
        p.advance(&mut g, &mut d, b"\x1b[2;1H\x1b[1M");
        assert_eq!(g.cell(0, 0).ch, 'A' as u32);
        assert_eq!(g.cell(0, 1).ch, 'C' as u32);
        assert_eq!(g.cell(0, 2).ch, 'D' as u32);
        assert_eq!(g.cell(0, 3).ch, ' ' as u32);
    }

    // === Bare ESC[m is reset ===

    #[test]
    fn bare_sgr_is_reset() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[1mA\x1b[mB");
        assert!(g.cell(0, 0).flags.contains(CellFlags::BOLD));
        assert!(!g.cell(1, 0).flags.contains(CellFlags::BOLD));
    }

    // === Origin mode ===

    #[test]
    fn origin_mode_homes_cursor() {
        let (mut p, mut g, mut d) = setup(10, 10);
        p.advance(&mut g, &mut d, b"\x1b[3;7r");
        p.advance(&mut g, &mut d, b"\x1b[?6h");
        assert_eq!(g.cursor_row(), 2);
        assert_eq!(g.cursor_col(), 0);
    }

    // === ESC D (Index) ===

    #[test]
    fn esc_index_linefeed() {
        let (mut p, mut g, mut d) = setup(5, 3);
        p.advance(&mut g, &mut d, b"\x1b[1;1H\x1bD");
        assert_eq!(g.cursor_row(), 1);
    }

    // === Compound sequences ===

    #[test]
    fn compound_sgr_changes_mid_text() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"A\x1b[1mB\x1b[0mC");
        assert!(!g.cell(0, 0).flags.contains(CellFlags::BOLD));
        assert!(g.cell(1, 0).flags.contains(CellFlags::BOLD));
        assert!(!g.cell(2, 0).flags.contains(CellFlags::BOLD));
    }

    #[test]
    fn incremental_byte_feeding() {
        let (mut p, mut g, mut d) = setup(10, 2);
        for &byte in b"\x1b[1;31mHi".iter() {
            p.advance(&mut g, &mut d, &[byte]);
        }
        assert_eq!(g.cell(0, 0).ch, 'H' as u32);
        assert!(g.cell(0, 0).flags.contains(CellFlags::BOLD));
        assert_eq!(g.cell(0, 0).fg, p.theme().palette(1));
    }

    #[test]
    fn mouse_drag_mode() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[?1002h");
        assert_eq!(p.mouse_state().mode, MouseMode::Drag);
    }

    #[test]
    fn mouse_motion_mode() {
        let (mut p, mut g, mut d) = setup(10, 2);
        p.advance(&mut g, &mut d, b"\x1b[?1003h");
        assert_eq!(p.mouse_state().mode, MouseMode::Motion);
    }
}
