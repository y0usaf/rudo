use std::io::Write as _;
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{
    cli::CliArgs,
    config::Config,
    cursor::{CursorRenderer, CursorTick},
    defaults::{DEFAULT_CLIPBOARD_COPY_COMMAND, DEFAULT_CLIPBOARD_PASTE_COMMAND},
    input::{Key, KeyEvent, Modifiers, MouseButton},
    keybindings::LocalAction,
    pty::{Pty, PtySpawnConfig},
    terminal::{
        damage::DamageTracker,
        grid::Grid,
        mouse,
        parser::TerminalParser,
        selection::{self, GridPoint, Selection, SelectionState},
        theme::Theme,
    },
};

/// Number of bytes to read from PTY per iteration
const PTY_READ_BUFFER_SIZE: usize = 65536;
/// Maximum PTY bytes parsed per frame to keep input/render latency bounded.
const MAX_PTY_BYTES_PER_TICK: usize = PTY_READ_BUFFER_SIZE * 8;

/// Lines scrolled per scroll wheel tick  
const SCROLL_MULTIPLIER: usize = 3;

/// Default cell size before font metrics are available
const DEFAULT_CELL_SIZE: (f32, f32) = (9.0, 18.0);

pub(crate) struct TickResult {
    pub redraw_requested: bool,
    pub animating: bool,
}

pub struct CoreApp {
    grid: Grid,
    cursor_renderer: CursorRenderer,
    damage: DamageTracker,
    parser: TerminalParser,
    pty: Option<Pty>,
    selection: Selection,
    config: Config,
    theme: Theme,
    last_frame: Instant,
    modifiers: Modifiers,
    last_title: Option<String>,
    cell_size: (f32, f32),
    grid_offset: (f32, f32),
    title_changed: bool,
    theme_changed: bool,
    mouse_pressed: bool,
    mouse_button: Option<MouseButton>,
    last_mouse_pos: (f64, f64),
    scroll_accumulator: f64,
    needs_redraw: bool,
    pty_dead: bool,
    command: Vec<String>,
}

impl CoreApp {
    pub fn new(cli: CliArgs) -> Self {
        let CliArgs {
            app_id,
            title,
            command,
        } = cli;
        let mut config = Config::load();
        Self::apply_cli_overrides(&mut config, app_id, title);
        let theme = Theme::load_theme_file().unwrap_or_else(|| Self::theme_from_config(&config));
        let cols = config.terminal.cols.max(2);
        let rows = config.terminal.rows.max(2);
        let cursor_renderer = Self::build_cursor_renderer(&config);
        Self {
            grid: Grid::with_scrollback(cols, rows, config.scrollback.lines),
            cursor_renderer,
            damage: DamageTracker::new(rows),
            parser: TerminalParser::with_theme(theme.clone()),
            pty: None,
            selection: Selection::new(),
            config,
            theme,
            last_frame: Instant::now(),
            modifiers: Modifiers::empty(),
            last_title: None,
            cell_size: DEFAULT_CELL_SIZE,
            grid_offset: (0.0, 0.0),
            title_changed: false,
            theme_changed: false,
            mouse_pressed: false,
            mouse_button: None,
            last_mouse_pos: (0.0, 0.0),
            scroll_accumulator: 0.0,
            needs_redraw: true,
            pty_dead: false,
            command,
        }
    }

    fn apply_cli_overrides(config: &mut Config, app_id: Option<String>, title: Option<String>) {
        if let Some(app_id) = app_id {
            config.window.app_id = app_id;
        }
        if let Some(title) = title {
            config.window.title = title;
        }
    }

    fn theme_from_config(config: &Config) -> Theme {
        use crate::terminal::theme::ThemeColorStrings;

        let c = &config.colors;
        Theme::from_color_strings(&ThemeColorStrings {
            foreground: &c.foreground,
            background: &c.background,
            cursor: &c.cursor,
            selection: &c.selection,
            ansi: [
                &c.black,
                &c.red,
                &c.green,
                &c.yellow,
                &c.blue,
                &c.magenta,
                &c.cyan,
                &c.white,
                &c.bright_black,
                &c.bright_red,
                &c.bright_green,
                &c.bright_yellow,
                &c.bright_blue,
                &c.bright_magenta,
                &c.bright_cyan,
                &c.bright_white,
            ],
        })
    }

    fn build_cursor_renderer(config: &Config) -> CursorRenderer {
        let mut cursor_renderer = CursorRenderer::new();
        cursor_renderer.set_animation_length(config.cursor.animation_length);
        cursor_renderer.set_short_animation_length(config.cursor.short_animation_length);
        cursor_renderer.set_trail_size(config.cursor.trail_size);
        cursor_renderer.set_blink_enabled(config.cursor.blink);
        cursor_renderer.set_blink_interval(config.cursor.blink_interval);
        match config.cursor.style.as_str() {
            "beam" | "bar" | "vertical" => {
                cursor_renderer.set_shape(crate::cursor::CursorShape::Beam)
            }
            "underline" | "horizontal" => {
                cursor_renderer.set_shape(crate::cursor::CursorShape::Underline)
            }
            _ => cursor_renderer.set_shape(crate::cursor::CursorShape::Block),
        }
        cursor_renderer
    }

    fn write_pty(&self, bytes: &[u8]) {
        if let Some(pty) = &self.pty {
            let _ = pty.write(bytes);
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn app_id(&self) -> &str {
        &self.config.window.app_id
    }
    pub fn theme(&self) -> &Theme {
        &self.theme
    }
    pub fn grid(&self) -> &Grid {
        &self.grid
    }

    pub fn render_state_mut(&mut self) -> (&mut Grid, &CursorRenderer, &Selection) {
        (&mut self.grid, &self.cursor_renderer, &self.selection)
    }
    pub fn cursor_renderer(&self) -> &CursorRenderer {
        &self.cursor_renderer
    }
    pub fn selection(&self) -> &Selection {
        &self.selection
    }
    pub fn damage(&self) -> &DamageTracker {
        &self.damage
    }

    pub fn clear_damage(&mut self) {
        self.damage.clear();
    }

    pub fn damage_mut(&mut self) -> &mut DamageTracker {
        &mut self.damage
    }

    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    pub fn pty_exited(&self) -> bool {
        self.pty_dead
    }

    pub fn set_cell_size(&mut self, cw: f32, ch: f32) {
        self.cell_size = (cw, ch);
    }

    pub fn set_grid_offset(&mut self, ox: f32, oy: f32) {
        self.grid_offset = (ox, oy);
    }

    pub fn init_terminal(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = clamp_grid_size(cols, rows);
        self.grid = Grid::with_scrollback(cols, rows, self.config.scrollback.lines);
        self.damage = DamageTracker::new(rows);
        let spawn_config = PtySpawnConfig {
            term: &self.config.terminal.term,
            colorterm: &self.config.terminal.colorterm,
            shell_fallback: &self.config.terminal.shell_fallback,
            command: &self.command,
        };
        match Pty::spawn(cols as u16, rows as u16, &spawn_config) {
            Ok(pty) => {
                self.pty = Some(pty);
                self.pty_dead = false;
            }
            Err(e) => crate::error_log!("Failed to spawn PTY: {e}"),
        }
        self.needs_redraw = true;
    }

    pub fn pty_raw_fd(&self) -> Option<i32> {
        self.pty.as_ref().map(|pty| pty.master_fd().as_raw_fd())
    }

    pub fn title(&self) -> &str {
        self.last_title
            .as_deref()
            .unwrap_or(&self.config.window.title)
    }

    pub fn take_title_changed(&mut self) -> bool {
        let changed = self.title_changed;
        self.title_changed = false;
        changed
    }

    pub fn take_theme_changed(&mut self) -> bool {
        let changed = self.theme_changed;
        self.theme_changed = false;
        changed
    }

    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    pub fn handle_key_event(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }
        if self.pty.is_none() {
            return;
        }

        if self
            .config
            .keybindings
            .matches(LocalAction::Copy, event, self.modifiers)
        {
            if self.selection.has_selection() {
                let text = self.selection.selected_text(&self.grid);
                clipboard_set(&text);
            }
            return;
        }

        if self
            .config
            .keybindings
            .matches(LocalAction::Paste, event, self.modifiers)
        {
            if let Some(text) = clipboard_get() {
                self.write_pty(text.as_bytes());
            }
            return;
        }

        if self.modifiers.control_key() {
            if let Key::Text(ref c) = event.key {
                let ch = c.chars().next().unwrap_or('\0');
                if ch.is_ascii_alphabetic() {
                    let code = (ch.to_ascii_lowercase() as u8) - b'a' + 1;
                    self.write_pty(&[code]);
                    return;
                }
            }
        }

        let app_cursor = self.parser.application_cursor_keys();
        let seq: Option<&[u8]> = match &event.key {
            Key::Enter => Some(b"\r"),
            Key::Backspace => Some(b"\x7f"),
            Key::Escape => Some(b"\x1b"),
            Key::Tab => Some(b"\t"),
            Key::Space => Some(b" "),
            Key::ArrowUp => Some(if app_cursor { b"\x1bOA" } else { b"\x1b[A" }),
            Key::ArrowDown => Some(if app_cursor { b"\x1bOB" } else { b"\x1b[B" }),
            Key::ArrowRight => Some(if app_cursor { b"\x1bOC" } else { b"\x1b[C" }),
            Key::ArrowLeft => Some(if app_cursor { b"\x1bOD" } else { b"\x1b[D" }),
            Key::Home => Some(if app_cursor { b"\x1bOH" } else { b"\x1b[H" }),
            Key::End => Some(if app_cursor { b"\x1bOF" } else { b"\x1b[F" }),
            Key::PageUp => Some(b"\x1b[5~"),
            Key::PageDown => Some(b"\x1b[6~"),
            Key::Delete => Some(b"\x1b[3~"),
            Key::Insert => Some(b"\x1b[2~"),
            Key::F(1) => Some(b"\x1bOP"),
            Key::F(2) => Some(b"\x1bOQ"),
            Key::F(3) => Some(b"\x1bOR"),
            Key::F(4) => Some(b"\x1bOS"),
            Key::F(5) => Some(b"\x1b[15~"),
            Key::F(6) => Some(b"\x1b[17~"),
            Key::F(7) => Some(b"\x1b[18~"),
            Key::F(8) => Some(b"\x1b[19~"),
            Key::F(9) => Some(b"\x1b[20~"),
            Key::F(10) => Some(b"\x1b[21~"),
            Key::F(11) => Some(b"\x1b[23~"),
            Key::F(12) => Some(b"\x1b[24~"),
            Key::Text(c) => {
                self.write_pty(c.as_bytes());
                None
            }
            _ => None,
        };

        if let Some(seq) = seq {
            self.write_pty(seq);
        }
    }

    pub fn matches_local_keybinding(&self, action: LocalAction, event: &KeyEvent) -> bool {
        self.config
            .keybindings
            .matches(action, event, self.modifiers)
    }

    pub fn handle_mouse_button(&mut self, pressed: bool, button: MouseButton) {
        let (col, row) = self.pixel_to_grid(self.last_mouse_pos.0, self.last_mouse_pos.1);
        let mouse_state = self.parser.mouse_state();

        if mouse_state.is_active() && !self.modifiers.shift_key() {
            if let Some(btn_code) = mouse::mouse_button_code(button) {
                let mods = mouse::modifier_bits(self.modifiers);
                let seq = if pressed {
                    mouse::encode_mouse_press(mouse_state, btn_code, mods, col as u16, row as u16)
                } else {
                    mouse::encode_mouse_release(mouse_state, btn_code, mods, col as u16, row as u16)
                };
                if let Some(seq) = seq {
                    self.write_pty(&seq);
                }
            }
            if pressed {
                self.mouse_pressed = true;
                self.mouse_button = Some(button);
            } else {
                self.mouse_pressed = false;
                self.mouse_button = None;
            }
        } else if button == MouseButton::Left {
            let before = self.selection.snapshot();
            if pressed {
                self.mouse_pressed = true;
                self.mouse_button = Some(button);
                self.selection.clear();
                self.selection.start_selection(col, row);
            } else {
                self.mouse_pressed = false;
                self.mouse_button = None;
                self.selection.finish_selection();
            }
            let after = self.selection.snapshot();
            self.mark_selection_change(before, after);
            self.needs_redraw = true;
        }
    }

    pub fn handle_mouse_move(&mut self, x: f64, y: f64) {
        self.last_mouse_pos = (x, y);
        let (col, row) = self.pixel_to_grid(x, y);
        let mouse_state = self.parser.mouse_state();

        if mouse_state.is_active() && !self.modifiers.shift_key() {
            let mods = mouse::modifier_bits(self.modifiers);
            let seq = if self.mouse_pressed {
                if let Some(btn) = self.mouse_button {
                    if let Some(btn_code) = mouse::mouse_button_code(btn) {
                        mouse::encode_mouse_drag(
                            mouse_state,
                            btn_code,
                            mods,
                            col as u16,
                            row as u16,
                        )
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                mouse::encode_mouse_move(mouse_state, mods, col as u16, row as u16)
            };
            if let Some(seq) = seq {
                self.write_pty(&seq);
            }
        } else if self.mouse_pressed {
            let before = self.selection.snapshot();
            if self.selection.state() == selection::SelectionState::None {
                self.selection.start_selection(col, row);
            } else {
                let (_, _, end) = before;
                if end.col == col && end.row == row {
                    return;
                }
                self.selection.update_selection(col, row);
            }
            let after = self.selection.snapshot();
            self.mark_selection_change(before, after);
            self.needs_redraw = true;
        }
    }

    pub fn handle_scroll_lines(&mut self, lines: f64) {
        let mouse_state = self.parser.mouse_state();

        if mouse_state.is_active() && !self.modifiers.shift_key() {
            let (col, row) = self.pixel_to_grid(self.last_mouse_pos.0, self.last_mouse_pos.1);
            let mods = mouse::modifier_bits(self.modifiers);
            let up = lines > 0.0;
            let count = lines.abs() as usize;
            for _ in 0..count.max(1) {
                if let Some(seq) =
                    mouse::encode_mouse_scroll(mouse_state, mods, up, col as u16, row as u16)
                {
                    self.write_pty(&seq);
                }
            }
            return;
        }

        if lines.abs() < 1.0 {
            self.scroll_accumulator += lines;
        }
        let total = if self.scroll_accumulator.abs() >= 1.0 {
            let whole = self.scroll_accumulator.trunc();
            self.scroll_accumulator -= whole;
            whole
        } else {
            lines
        };
        if total == 0.0 {
            return;
        }

        let count = (total.abs() as usize).max(1);
        let scroll_lines = count * SCROLL_MULTIPLIER;
        let changed = if total > 0.0 {
            self.grid.scroll_view_up(scroll_lines)
        } else {
            self.grid.scroll_view_down(scroll_lines)
        };
        if changed {
            self.damage.mark_all();
            self.needs_redraw = true;
        }
    }

    pub fn handle_resize(&mut self, cols: usize, rows: usize) {
        let (cols, rows) = clamp_grid_size(cols, rows);
        if cols != self.grid.cols() || rows != self.grid.rows() {
            self.grid.resize(cols, rows);
            self.damage.resize(rows);
            if let Some(pty) = &self.pty {
                let _ = pty.resize(cols as u16, rows as u16);
            }
            self.needs_redraw = true;
        }
    }

    pub fn tick(&mut self) -> TickResult {
        let now = Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
        self.process_pty_output();
        let cursor_pos = self.grid.cursor_position();
        let CursorTick {
            needs_redraw: cursor_redraw,
            animating,
        } = self.cursor_renderer.animate(cursor_pos, dt);
        let redraw_requested =
            self.needs_redraw || self.damage.has_damage() || cursor_redraw || self.theme_changed;
        self.needs_redraw = false;
        TickResult {
            redraw_requested,
            animating,
        }
    }

    pub fn next_wakeup(&self) -> Option<Duration> {
        self.cursor_renderer
            .next_wakeup_in(self.last_frame.elapsed())
    }

    pub fn cursor_wakeup_due(&self) -> bool {
        self.next_wakeup()
            .is_some_and(|duration| duration.is_zero())
    }

    fn process_pty_output(&mut self) -> bool {
        let Some(pty) = &self.pty else { return false };
        let mut buf = [0u8; PTY_READ_BUFFER_SIZE];
        let mut got_output = false;
        let mut bytes_read = 0usize;
        loop {
            match pty.try_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    got_output = true;
                    bytes_read = bytes_read.saturating_add(n);
                    self.parser
                        .advance(&mut self.grid, &mut self.damage, &buf[..n]);
                    if bytes_read >= MAX_PTY_BYTES_PER_TICK {
                        self.needs_redraw = true;
                        break;
                    }
                }
                Err(err) => {
                    crate::warn_log!("PTY read error: {err}");
                    self.pty_dead = true;
                    break;
                }
            }
        }
        if !got_output {
            return false;
        }

        if self.grid.is_viewing_scrollback() {
            self.grid.reset_view();
            self.damage.mark_all();
        }
        for resp in self.parser.take_responses() {
            self.write_pty(&resp);
        }
        if self.parser.take_theme_changed() {
            self.theme = self.parser.theme().clone();
            self.theme_changed = true;
            self.damage.mark_all();
            self.needs_redraw = true;
        }
        let title = self.parser.title();
        if title != self.last_title.as_deref() {
            self.last_title = title.map(str::to_string);
            self.title_changed = true;
        }
        got_output
    }

    fn pixel_to_grid(&self, x: f64, y: f64) -> (usize, usize) {
        let (cw, ch) = sanitized_cell_size(self.cell_size);
        let (ox, oy) = self.grid_offset;
        let col = ((x as f32 - ox) / cw).max(0.0) as usize;
        let row = ((y as f32 - oy) / ch).max(0.0) as usize;
        (
            col.min(self.grid.cols().saturating_sub(1)),
            row.min(self.grid.rows().saturating_sub(1)),
        )
    }

    fn mark_selection_change(
        &mut self,
        before: (SelectionState, GridPoint, GridPoint),
        after: (SelectionState, GridPoint, GridPoint),
    ) {
        self.mark_selection_snapshot(before);
        self.mark_selection_snapshot(after);
    }

    fn mark_selection_snapshot(&mut self, snapshot: (SelectionState, GridPoint, GridPoint)) {
        let (state, start, end) = snapshot;
        if state == SelectionState::None || self.grid.rows() == 0 {
            return;
        }

        let last_row = self.grid.rows().saturating_sub(1);
        let start_row = start.row.min(last_row);
        let end_row = end.row.min(last_row);
        let (start_row, end_row) = if start_row <= end_row {
            (start_row, end_row)
        } else {
            (end_row, start_row)
        };

        self.damage.mark_rows(start_row, end_row);
    }
}

fn clamp_grid_size(cols: usize, rows: usize) -> (usize, usize) {
    (cols.max(2), rows.max(2))
}

fn sanitized_cell_size((cw, ch): (f32, f32)) -> (f32, f32) {
    let cw = if cw.is_finite() && cw > 0.0 {
        cw
    } else {
        DEFAULT_CELL_SIZE.0
    };
    let ch = if ch.is_finite() && ch > 0.0 {
        ch
    } else {
        DEFAULT_CELL_SIZE.1
    };
    (cw, ch)
}

fn clipboard_set(text: &str) {
    if let Ok(mut child) = Command::new(DEFAULT_CLIPBOARD_COPY_COMMAND)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(stdin) = child.stdin.as_mut() {
            let _ = stdin.write_all(text.as_bytes());
        }
    }
}

fn clipboard_get() -> Option<String> {
    if let Ok(out) = Command::new(DEFAULT_CLIPBOARD_PASTE_COMMAND)
        .arg("--no-newline")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
    {
        if out.status.success() {
            return String::from_utf8(out.stdout).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::MouseButton;

    fn test_app() -> CoreApp {
        let mut app = CoreApp::new(CliArgs {
            app_id: None,
            title: None,
            command: Vec::new(),
        });
        app.set_cell_size(10.0, 20.0);
        app.set_grid_offset(0.0, 0.0);
        app
    }

    #[test]
    fn local_selection_starts_on_left_press_at_current_cell() {
        let mut app = test_app();
        app.handle_mouse_move(25.0, 45.0);

        app.handle_mouse_button(true, MouseButton::Left);

        assert_eq!(app.selection().state(), SelectionState::Selecting);
        assert_eq!(
            app.selection().snapshot(),
            (
                SelectionState::Selecting,
                GridPoint::new(2, 2),
                GridPoint::new(2, 2),
            )
        );
    }

    #[test]
    fn local_selection_drag_keeps_press_cell_as_anchor() {
        let mut app = test_app();
        app.handle_mouse_move(25.0, 45.0);
        app.handle_mouse_button(true, MouseButton::Left);

        app.handle_mouse_move(65.0, 85.0);

        assert_eq!(
            app.selection().snapshot(),
            (
                SelectionState::Selecting,
                GridPoint::new(2, 2),
                GridPoint::new(6, 4),
            )
        );
    }
}
