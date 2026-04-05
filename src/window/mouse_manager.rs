use std::sync::Arc;
use std::time::Instant;

use winit::{
    event::WindowEvent,
    event::{ElementState, MouseButton, MouseScrollDelta},
    keyboard::ModifiersState,
    window::Window,
};

use glamour::{Contains, Point2};

use crate::{
    renderer::{MessageSelection, Renderer, WindowDrawDetails},
    settings::Settings,
    terminal::input::{
        TerminalInputSettings, TerminalMouseMode, encode_mouse_drag, encode_mouse_move,
        encode_mouse_press, encode_mouse_release, encode_mouse_scroll,
    },
    units::{GridPos, GridScale, PixelPos},
    window::{WindowSettings, keyboard_manager::KeyboardManager},
};

struct DragDetails {
    button: MouseButton,
}

/// Multi-click timeout for distinguishing single/double/triple clicks.
const MULTI_CLICK_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    CharWise,
    WordWise,
    LineWise,
    Block,
}

#[derive(Clone, Debug)]
pub struct TerminalSelection {
    pub grid_id: u64,
    pub kind: SelectionKind,
    pub start: GridPos<u32>,
    pub end: GridPos<u32>,
    pub ongoing: bool,
    /// Pivot range for word/line wise selections (allows extending in both directions)
    pub pivot_start: GridPos<u32>,
    pub pivot_end: GridPos<u32>,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
pub enum MessageSelectionEvent {
    Outside,
    Update(MessageSelection),
    Finish(MessageSelection),
    Clear,
}

#[derive(Clone, Debug)]
pub enum TerminalSelectionEvent {
    Update(TerminalSelection),
    Finish(TerminalSelection),
    Clear,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Default)]
pub enum OverlayEvent {
    #[default]
    Unchanged,
    MessageSelection(MessageSelectionEvent),
    TerminalSelection(TerminalSelectionEvent),
}

pub struct TerminalMouseEventResult {
    pub overlay_event: OverlayEvent,
    pub bytes: Vec<u8>,
    pub open_hyperlink: Option<String>,
}

pub struct EditorState<'a> {
    pub grid_scale: &'a GridScale,
    pub window_regions: &'a Vec<WindowDrawDetails>,
}

pub struct MouseManager {
    drag_details: Option<DragDetails>,
    grid_position: GridPos<u32>,

    pub window_position: PixelPos<f32>,

    mouse_hidden: bool,
    // tracks whether we need to force a cursor visibility resync once focus returns.
    // on macos, alt-tabbing while hidden keeps appkit in a cursor hidden mode that ignores
    // redundant show requests https://github.com/rust-windowing/winit/issues/1295 so we
    // remember to toggle visibility once we regain focus.
    cursor_resync_needed: bool,
    pub enabled: bool,

    terminal_selection: Option<TerminalSelection>,
    last_click_time: Option<Instant>,
    click_count: u32,

    settings: Arc<Settings>,
}

impl MouseManager {
    pub fn new(settings: Arc<Settings>) -> MouseManager {
        MouseManager {
            drag_details: None,
            window_position: PixelPos::default(),
            grid_position: GridPos::default(),
            mouse_hidden: false,
            cursor_resync_needed: false,
            enabled: true,
            terminal_selection: None,
            last_click_time: None,
            click_count: 0,
            settings,
        }
    }

    fn should_intercept_for_selection(
        input: TerminalInputSettings,
        modifiers: ModifiersState,
    ) -> bool {
        input.mouse_mode == TerminalMouseMode::Disabled || modifiers.shift_key()
    }

    pub fn clear_selection(&mut self) {
        self.terminal_selection = None;
    }

    pub fn current_selection(&self) -> Option<&TerminalSelection> {
        self.terminal_selection.as_ref()
    }

    fn request_cursor_visible(&mut self, window: &Window) {
        window.set_cursor_visible(true);
        self.mouse_hidden = false;
        // remember to reapply on focus regain if we changed visibility while unfocused.
        self.cursor_resync_needed = !window.has_focus();
    }

    fn force_cursor_visible(&mut self, window: &Window) {
        #[cfg(target_os = "macos")]
        {
            // winit short-circuits duplicate visibility requests and AppKit won't repaint when the
            // window is unfocused https://github.com/rust-windowing/winit/issues/1295 so flip to
            // false first to ensure the following true call is seen.
            // TODO: move this workaround into winit so visibility resyncs automatically.
            window.set_cursor_visible(false);
        }
        window.set_cursor_visible(true);
        self.mouse_hidden = false;
        self.cursor_resync_needed = false;
    }

    fn hide_cursor(&mut self, window: &Window) {
        window.set_cursor_visible(false);
        self.mouse_hidden = true;
        self.cursor_resync_needed = false;
    }

    fn handle_focus_gain(&mut self, window: &Window) {
        if self.cursor_resync_needed {
            self.force_cursor_visible(window);
        }
    }

    fn handle_focus_loss(&mut self, window: &Window) {
        if self.mouse_hidden {
            self.request_cursor_visible(window);
            self.cursor_resync_needed = true;
        }
    }

    fn handle_focus_change(&mut self, window: &Window, focused: bool) {
        if focused {
            self.handle_focus_gain(window);
        } else {
            self.handle_focus_loss(window);
        }
    }

    pub fn get_window_details_under_mouse<'b>(
        &self,
        editor_state: &'b EditorState<'b>,
    ) -> Option<&'b WindowDrawDetails> {
        let position = self.window_position;
        // the rendered window regions are sorted by draw order, so the earlier windows in the
        // list are drawn under the later ones
        editor_state.window_regions.iter().rfind(|details| details.region.contains(&position))
    }

    fn get_relative_position_at(
        window_position: PixelPos<f32>,
        window_details: &WindowDrawDetails,
        editor_state: &EditorState,
    ) -> GridPos<u32> {
        let relative_position = (window_position - window_details.region.min).to_point();
        (relative_position / *editor_state.grid_scale)
            .floor()
            .max((0.0, 0.0).into())
            .try_cast()
            .unwrap()
            .min(Point2::new(
                window_details.grid_size.width.max(1) - 1,
                window_details.grid_size.height.max(1) - 1,
            ))
    }

    pub fn get_relative_position(
        &self,
        window_details: &WindowDrawDetails,
        editor_state: &EditorState,
    ) -> GridPos<u32> {
        Self::get_relative_position_at(self.window_position, window_details, editor_state)
    }

    pub fn handle_terminal_event(
        &mut self,
        event: &WindowEvent,
        keyboard_manager: &KeyboardManager,
        renderer: &Renderer,
        window: &Window,
        input: TerminalInputSettings,
    ) -> TerminalMouseEventResult {
        let editor_state = EditorState {
            grid_scale: &renderer.grid_renderer.grid_scale,
            window_regions: &renderer.window_regions,
        };
        let hide_mouse_when_typing = self.settings.get::<WindowSettings>().hide_mouse_when_typing;
        let mut bytes = Vec::new();
        let mut open_hyperlink = None;

        match event {
            WindowEvent::CursorMoved { position, .. } => {
                self.window_position = (position.x as f32, position.y as f32).into();
                if let Some(details) = self.get_window_details_under_mouse(&editor_state) {
                    self.grid_position = self.get_relative_position(details, &editor_state);
                    let modifiers = keyboard_manager.current_modifiers().state();

                    // Handle selection drag updates
                    if let Some(ref mut selection) = self.terminal_selection {
                        if selection.ongoing {
                            selection.end = self.grid_position;
                            let updated = selection.clone();
                            return TerminalMouseEventResult {
                                overlay_event: OverlayEvent::TerminalSelection(
                                    TerminalSelectionEvent::Update(updated),
                                ),
                                bytes,
                                open_hyperlink,
                            };
                        }
                    }

                    if let Some(drag) = &self.drag_details {
                        if let Some(encoded) = encode_mouse_drag(
                            input,
                            drag.button,
                            modifiers,
                            self.grid_position.x,
                            self.grid_position.y,
                        ) {
                            bytes.extend(encoded);
                        }
                    } else if let Some(encoded) =
                        encode_mouse_move(input, modifiers, self.grid_position.x, self.grid_position.y)
                    {
                        bytes.extend(encoded);
                    }
                }
                if self.mouse_hidden && window.has_focus() {
                    self.request_cursor_visible(window);
                } else if self.cursor_resync_needed && window.has_focus() {
                    self.force_cursor_visible(window);
                }
            }
            WindowEvent::CursorEntered { .. } => {
                if self.mouse_hidden {
                    self.request_cursor_visible(window);
                } else if self.cursor_resync_needed && window.has_focus() {
                    self.force_cursor_visible(window);
                }
            }
            WindowEvent::MouseWheel { delta: MouseScrollDelta::LineDelta(_, y), .. } => {
                if let Some(details) = self.get_window_details_under_mouse(&editor_state) {
                    let position = self.get_relative_position(details, &editor_state);
                    let steps = y.abs().max(1.0) as usize;
                    for _ in 0..steps {
                        if let Some(encoded) = encode_mouse_scroll(
                            input,
                            keyboard_manager.current_modifiers().state(),
                            *y > 0.0,
                            position.x,
                            position.y,
                        ) {
                            bytes.extend(encoded);
                        }
                    }
                }
            }
            WindowEvent::MouseWheel { delta: MouseScrollDelta::PixelDelta(delta), .. } => {
                if delta.y != 0.0 {
                    if let Some(details) = self.get_window_details_under_mouse(&editor_state) {
                        let position = self.get_relative_position(details, &editor_state);
                        if let Some(encoded) = encode_mouse_scroll(
                            input,
                            keyboard_manager.current_modifiers().state(),
                            delta.y > 0.0,
                            position.x,
                            position.y,
                        ) {
                            bytes.extend(encoded);
                        }
                    }
                }
            }
            WindowEvent::MouseInput { button, state, .. } => {
                if let Some(details) = self.get_window_details_under_mouse(&editor_state) {
                    let position = self.get_relative_position(details, &editor_state);
                    self.grid_position = position;
                    let modifiers = keyboard_manager.current_modifiers().state();
                    let hyperlink_modifier = if cfg!(target_os = "macos") {
                        modifiers.super_key()
                    } else {
                        modifiers.control_key()
                    };
                    if *state == ElementState::Pressed
                        && *button == MouseButton::Left
                        && hyperlink_modifier
                    {
                        if let Some(rendered_window) = renderer.rendered_windows.get(&details.id) {
                            if let Some(link) =
                                rendered_window.hyperlink_at_cell(position.y, position.x)
                            {
                                open_hyperlink = Some(link.uri.clone());
                                self.drag_details = None;
                                return TerminalMouseEventResult {
                                    overlay_event: OverlayEvent::default(),
                                    bytes,
                                    open_hyperlink,
                                };
                            }
                        }
                    }

                    // Selection interception: when the terminal has no mouse mode or
                    // shift is held, use left-button events for text selection instead
                    // of forwarding them to the PTY.
                    if Self::should_intercept_for_selection(input, modifiers) {
                        if *button == MouseButton::Left {
                            if *state == ElementState::Pressed {
                                // Determine click count via multi-click timing
                                let now = Instant::now();
                                let click_count = match self.last_click_time {
                                    Some(prev)
                                        if now.duration_since(prev) < MULTI_CLICK_TIMEOUT =>
                                    {
                                        self.click_count + 1
                                    }
                                    _ => 1,
                                };
                                self.last_click_time = Some(now);
                                self.click_count = click_count;

                                let kind = match click_count {
                                    1 => SelectionKind::CharWise,
                                    2 => SelectionKind::WordWise,
                                    _ => {
                                        // Reset count so the next click cycle restarts
                                        self.click_count = 3;
                                        SelectionKind::LineWise
                                    }
                                };

                                let selection = TerminalSelection {
                                    grid_id: details.id,
                                    kind,
                                    start: position,
                                    end: position,
                                    ongoing: true,
                                    pivot_start: position,
                                    pivot_end: position,
                                };
                                self.terminal_selection = Some(selection.clone());
                                return TerminalMouseEventResult {
                                    overlay_event: OverlayEvent::TerminalSelection(
                                        TerminalSelectionEvent::Update(selection),
                                    ),
                                    bytes,
                                    open_hyperlink,
                                };
                            } else {
                                // Left button released – finalize ongoing selection
                                if let Some(ref mut selection) = self.terminal_selection {
                                    if selection.ongoing {
                                        selection.ongoing = false;
                                        let finished = selection.clone();
                                        return TerminalMouseEventResult {
                                            overlay_event: OverlayEvent::TerminalSelection(
                                                TerminalSelectionEvent::Finish(finished),
                                            ),
                                            bytes,
                                            open_hyperlink,
                                        };
                                    }
                                }
                            }
                        } else if *state == ElementState::Pressed
                            && self.terminal_selection.is_some()
                        {
                            // Any other button press while a selection exists clears it
                            self.terminal_selection = None;
                            return TerminalMouseEventResult {
                                overlay_event: OverlayEvent::TerminalSelection(
                                    TerminalSelectionEvent::Clear,
                                ),
                                bytes,
                                open_hyperlink,
                            };
                        }
                    }

                    if *state == ElementState::Pressed {
                        if let Some(encoded) =
                            encode_mouse_press(input, *button, modifiers, position.x, position.y)
                        {
                            bytes.extend(encoded);
                        }
                        self.drag_details = Some(DragDetails { button: *button });
                    } else {
                        if let Some(encoded) =
                            encode_mouse_release(input, *button, modifiers, position.x, position.y)
                        {
                            bytes.extend(encoded);
                        }
                        self.drag_details = None;
                    }
                } else if *state == ElementState::Released {
                    self.drag_details = None;
                }
            }
            WindowEvent::KeyboardInput { event: key_event, .. }
                if hide_mouse_when_typing
                    && key_event.state == ElementState::Pressed
                    && !self.mouse_hidden
                    && window.has_focus() =>
            {
                self.hide_cursor(window);
            }
            WindowEvent::Focused(focused_event) if hide_mouse_when_typing => {
                self.handle_focus_change(window, *focused_event);
            }
            _ => {}
        }

        TerminalMouseEventResult { overlay_event: OverlayEvent::default(), bytes, open_hyperlink }
    }
}
