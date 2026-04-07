use std::io::Write as _;
use std::os::fd::AsRawFd;
use std::process::{Command, Stdio};

use crate::{
    cli::CliArgs,
    config::Config,
    cursor::CursorRenderer,
    defaults::{DEFAULT_CLIPBOARD_COPY_COMMAND, DEFAULT_CLIPBOARD_PASTE_COMMAND},
    input::{Key, KeyEvent, Modifiers, MouseButton},
    pty::{Pty, PtySpawnConfig},
    terminal::{
        damage::DamageTracker,
        grid::Grid,
        mouse,
        parser::TerminalParser,
        selection::{self, Selection},
        theme::Theme,
    },
};

/// Number of bytes to read from PTY per iteration
const PTY_READ_BUFFER_SIZE: usize = 65536;

/// Lines scrolled per scroll wheel tick  
const SCROLL_MULTIPLIER: usize = 3;

/// Default cell size before font metrics are available
const DEFAULT_CELL_SIZE: (f32, f32) = (9.0, 18.0);

pub struct CoreApp {
    grid: Grid,
    cursor_renderer: CursorRenderer,
    damage: DamageTracker,
    parser: TerminalParser,
    pty: Option<Pty>,
    selection: Selection,
    config: Config,
    theme: Theme,
    last_frame: std::time::Instant,
    modifiers: Modifiers,
    last_title: Option<String>,
    cell_size: (f32, f32),
    grid_offset: (f32, f32),
    title_changed: bool,
    mouse_pressed: bool,
    mouse_button: Option<MouseButton>,
    last_mouse_pos: (f64, f64),
    scroll_accumulator: f64,
    needs_redraw: bool,
    pty_dead: bool,
}

impl CoreApp {
    pub fn new(cli: CliArgs) -> Self {
        let mut config = Config::load();
        if let Some(app_id) = cli.app_id {
            config.window.app_id = app_id;
        }
        if let Some(title) = cli.title {
            config.window.title = title;
        }
        let theme = Theme::load_theme_file().unwrap_or_else(|| {
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
        });
        let cols = config.terminal.cols.max(2);
        let rows = config.terminal.rows.max(2);
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
        Self {
            grid: Grid::new(cols, rows),
            cursor_renderer,
            damage: DamageTracker::new(rows),
            parser: TerminalParser::with_theme(theme.clone()),
            pty: None,
            selection: Selection::new(),
            config,
            theme,
            last_frame: std::time::Instant::now(),
            modifiers: Modifiers::empty(),
            last_title: None,
            cell_size: DEFAULT_CELL_SIZE,
            grid_offset: (0.0, 0.0),
            title_changed: false,
            mouse_pressed: false,
            mouse_button: None,
            last_mouse_pos: (0.0, 0.0),
            scroll_accumulator: 0.0,
            needs_redraw: true,
            pty_dead: false,
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
        let cols = cols.max(2);
        let rows = rows.max(2);
        self.grid = Grid::new(cols, rows);
        self.damage = DamageTracker::new(rows);
        let spawn_config = PtySpawnConfig {
            term: &self.config.terminal.term,
            colorterm: &self.config.terminal.colorterm,
            shell_fallback: &self.config.terminal.shell_fallback,
        };
        match Pty::spawn(cols as u16, rows as u16, &spawn_config) {
            Ok(pty) => self.pty = Some(pty),
            Err(e) => eprintln!("[ERROR] Failed to spawn PTY: {e}"),
        }
        self.needs_redraw = true;
    }

    pub fn pty_raw_fd(&self) -> Option<i32> {
        self.pty.as_ref().map(|pty| pty.master_fd().as_raw_fd())
    }

    pub fn title(&self) -> &str {
        self.parser.title().unwrap_or(&self.config.window.title)
    }

    pub fn take_title_changed(&mut self) -> bool {
        let changed = self.title_changed;
        self.title_changed = false;
        changed
    }

    pub fn set_modifiers(&mut self, modifiers: Modifiers) {
        self.modifiers = modifiers;
    }

    pub fn handle_key_event(&mut self, event: &KeyEvent) {
        if !event.pressed {
            return;
        }
        let Some(pty) = &self.pty else { return };
        let ctrl = self.modifiers.control_key();
        let shift = self.modifiers.shift_key();

        if ctrl && shift {
            if let Key::Text(ref c) = event.key {
                match c.as_str() {
                    "C" | "c" => {
                        if self.selection.has_selection() {
                            let text = self.selection.selected_text(&self.grid);
                            clipboard_set(&text);
                        }
                        return;
                    }
                    "V" | "v" => {
                        if let Some(text) = clipboard_get() {
                            let _ = pty.write(text.as_bytes());
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }

        if ctrl {
            if let Key::Text(ref c) = event.key {
                let ch = c.chars().next().unwrap_or('\0');
                if ch.is_ascii_alphabetic() {
                    let code = (ch.to_ascii_lowercase() as u8) - b'a' + 1;
                    let _ = pty.write(&[code]);
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
                let _ = pty.write(c.as_bytes());
                None
            }
            _ => None,
        };

        if let Some(seq) = seq {
            let _ = pty.write(seq);
        }
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
                if let (Some(seq), Some(pty)) = (seq, &self.pty) {
                    let _ = pty.write(&seq);
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
            if pressed {
                self.mouse_pressed = true;
                self.mouse_button = Some(button);
                self.selection.clear();
            } else {
                self.mouse_pressed = false;
                self.mouse_button = None;
                self.selection.finish_selection();
            }
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
            if let (Some(seq), Some(pty)) = (seq, &self.pty) {
                let _ = pty.write(&seq);
            }
        } else if self.mouse_pressed {
            if self.selection.state() == selection::SelectionState::None {
                self.selection.start_selection(col, row);
            } else {
                self.selection.update_selection(col, row);
            }
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
                    if let Some(pty) = &self.pty {
                        let _ = pty.write(&seq);
                    }
                }
            }
        } else {
            if lines.abs() < 1.0 {
                self.scroll_accumulator += lines;
            }
            let total = if self.scroll_accumulator.abs() >= 1.0 {
                let v = self.scroll_accumulator.trunc();
                self.scroll_accumulator -= v;
                v
            } else {
                lines
            };
            let count = (total.abs() as usize).max(1);
            let scroll_lines = count * SCROLL_MULTIPLIER;
            if total > 0.0 {
                self.grid.scroll_view_up(scroll_lines);
                self.needs_redraw = true;
            } else if total < 0.0 {
                self.grid.scroll_view_down(scroll_lines);
                self.needs_redraw = true;
            }
        }
    }

    pub fn handle_resize(&mut self, cols: usize, rows: usize) {
        let cols = cols.max(2);
        let rows = rows.max(2);
        if cols != self.grid.cols() || rows != self.grid.rows() {
            self.grid.resize(cols, rows);
            self.damage.resize(rows);
            if let Some(pty) = &self.pty {
                let _ = pty.resize(cols as u16, rows as u16);
            }
            self.needs_redraw = true;
        }
    }

    pub fn tick(&mut self) -> bool {
        let now = std::time::Instant::now();
        let dt = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
        let got_output = self.process_pty_output();
        let cursor_pos = self.grid.cursor_position();
        let animating = self.cursor_renderer.animate(cursor_pos, dt);
        let redraw = self.needs_redraw || got_output || animating || self.title_changed;
        self.needs_redraw = false;
        redraw
    }

    fn process_pty_output(&mut self) -> bool {
        let Some(pty) = &self.pty else { return false };
        let mut buf = [0u8; PTY_READ_BUFFER_SIZE];
        let mut got_output = false;
        loop {
            match pty.try_read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    got_output = true;
                    self.parser
                        .advance(&mut self.grid, &mut self.damage, &buf[..n]);
                }
                Err(_) => {
                    self.pty_dead = true;
                    break;
                }
            }
        }
        if got_output && self.grid.is_viewing_scrollback() {
            self.grid.reset_view();
        }
        let responses = self.parser.take_responses();
        if let Some(pty) = &self.pty {
            for resp in responses {
                let _ = pty.write(&resp);
            }
        }
        if self.parser.title() != self.last_title.as_deref() {
            self.last_title = self.parser.title().map(str::to_string);
            self.title_changed = true;
        }
        got_output
    }

    fn pixel_to_grid(&self, x: f64, y: f64) -> (usize, usize) {
        let (cw, ch) = self.cell_size;
        let (ox, oy) = self.grid_offset;
        let col = ((x as f32 - ox) / cw).max(0.0) as usize;
        let row = ((y as f32 - oy) / ch).max(0.0) as usize;
        (
            col.min(self.grid.cols().saturating_sub(1)),
            row.min(self.grid.rows().saturating_sub(1)),
        )
    }
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
        let _ = child.wait();
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
