use std::{collections::HashMap, sync::Arc, time::Instant};

use winit::{
    event::WindowEvent,
    event::{DeviceId, ElementState, MouseButton, MouseScrollDelta},
    window::Window,
};

use glamour::{Contains, Point2};

use crate::{
    renderer::{MessageSelection, Renderer, WindowDrawDetails},
    settings::Settings,
    terminal::input::{
        TerminalInputSettings, encode_mouse_drag, encode_mouse_move, encode_mouse_press,
        encode_mouse_release, encode_mouse_scroll,
    },
    units::{GridPos, GridScale, GridSize, PixelPos, PixelRect},
    window::{WindowSettings, keyboard_manager::KeyboardManager},
};

fn mouse_button_to_button_text(mouse_button: MouseButton) -> Option<String> {
    match mouse_button {
        MouseButton::Left => Some("left".to_owned()),
        MouseButton::Right => Some("right".to_owned()),
        MouseButton::Middle => Some("middle".to_owned()),
        MouseButton::Back => Some("x1".to_owned()),
        MouseButton::Forward => Some("x2".to_owned()),
        _ => None,
    }
}

struct DragDetails {
    draw_details: WindowDrawDetails,
    button: MouseButton,
}

#[derive(Clone, Debug)]
struct MessageSelectionState {
    draw_details: WindowDrawDetails,
    start: GridPos<u32>,
    end: GridPos<u32>,
}

#[derive(Clone, Debug)]
pub enum MessageSelectionEvent {
    Outside,
    Update(MessageSelection),
    Finish(MessageSelection),
    Clear,
}

#[derive(Clone, Debug, Default)]
pub enum OverlayEvent {
    #[default]
    Unchanged,
    MessageSelection(MessageSelectionEvent),
}

pub struct PointerTransitionResult {
    pub overlay_event: OverlayEvent,
}

pub struct MouseEventResult {
    pub overlay_event: OverlayEvent,
}

pub struct TerminalMouseEventResult {
    pub overlay_event: OverlayEvent,
    pub bytes: Vec<u8>,
    pub open_hyperlink: Option<String>,
}

pub struct EditorState<'a> {
    pub grid_scale: &'a GridScale,
    pub window_regions: &'a Vec<WindowDrawDetails>,
    pub full_region: WindowDrawDetails,
    pub window: &'a Window,
    pub keyboard_manager: &'a KeyboardManager,
}

#[derive(Debug)]
struct TouchTrace {
    start_time: Instant,
    start: PixelPos<f32>,
    last: PixelPos<f32>,
    left_deadzone_once: bool,
}

pub struct MouseManager {
    drag_details: Option<DragDetails>,
    grid_position: GridPos<u32>,

    has_moved: bool,
    pub window_position: PixelPos<f32>,

    scroll_position: GridPos<f32>,

    // the tuple allows to keep track of different fingers per device
    touch_position: HashMap<(DeviceId, u64), TouchTrace>,

    mouse_hidden: bool,
    // tracks whether we need to force a cursor visibility resync once focus returns.
    // on macos, alt-tabbing while hidden keeps appkit in a cursor hidden mode that ignores
    // redundant show requests https://github.com/rust-windowing/winit/issues/1295 so we
    // remember to toggle visibility once we regain focus.
    cursor_resync_needed: bool,
    pub enabled: bool,

    settings: Arc<Settings>,
    message_selection: Option<MessageSelectionState>,
}

impl MouseManager {
    pub fn new(settings: Arc<Settings>) -> MouseManager {
        MouseManager {
            drag_details: None,
            has_moved: false,
            window_position: PixelPos::default(),
            grid_position: GridPos::default(),
            scroll_position: GridPos::default(),
            touch_position: HashMap::new(),
            mouse_hidden: false,
            cursor_resync_needed: false,
            enabled: true,
            settings,
            message_selection: None,
        }
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
        if self.settings.get::<WindowSettings>().has_mouse_grid_detection {
            Some(&editor_state.full_region)
        } else {
            // the rendered window regions are sorted by draw order, so the earlier windows in the
            // list are drawn under the later ones
            editor_state.window_regions.iter().rfind(|details| details.region.contains(&position))
        }
    }

    fn get_window_details_under_mouse_raw<'b>(
        &self,
        editor_state: &'b EditorState<'b>,
    ) -> Option<&'b WindowDrawDetails> {
        let position = self.window_position;
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

    pub fn clear_message_selection(&mut self) -> bool {
        let had_selection = self.message_selection.take().is_some();
        if had_selection {
            self.drag_details = None;
            self.has_moved = false;
        }

        had_selection
    }

    pub fn handle_terminal_event(
        &mut self,
        event: &WindowEvent,
        keyboard_manager: &KeyboardManager,
        renderer: &Renderer,
        window: &Window,
        input: TerminalInputSettings,
    ) -> TerminalMouseEventResult {
        let full_region = WindowDrawDetails {
            id: 0,
            region: renderer.window_regions.first().map_or(PixelRect::ZERO, |v| v.region),
            grid_size: renderer.window_regions.first().map_or(GridSize::ZERO, |v| v.grid_size),
            window_type: crate::ui::WindowType::Editor,
        };
        let editor_state = EditorState {
            grid_scale: &renderer.grid_renderer.grid_scale,
            window_regions: &renderer.window_regions,
            full_region,
            window,
            keyboard_manager,
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
                    if *state == ElementState::Pressed {
                        if let Some(encoded) =
                            encode_mouse_press(input, *button, modifiers, position.x, position.y)
                        {
                            bytes.extend(encoded);
                        }
                        self.drag_details =
                            Some(DragDetails { button: *button, draw_details: details.clone() });
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
