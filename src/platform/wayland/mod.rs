mod dispatch;
mod keyboard;
mod shm;

use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_compositor, wl_keyboard, wl_pointer, wl_region, wl_shm, wl_surface,
};
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use crate::core_app::CoreApp;
use crate::input::{KeyEvent, Modifiers};
use crate::software_renderer::{FrameBuffer, SoftwareRenderer};

use keyboard::{fallback_key_event, RepeatState, XkbContextData};
use shm::ShmBuffer;

const INITIAL_WIDTH: u32 = 800;
const INITIAL_HEIGHT: u32 = 600;
const BUFFER_COUNT: usize = 3;

// ─── Zoom ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZoomAction {
    In,
    Out,
    Reset,
}

// ─── Wayland state ───────────────────────────────────────────────────────────

struct WaylandState {
    running: bool,
    configured: bool,
    frame_ready: bool,
    width: u32,
    height: u32,
    app: CoreApp,
    renderer: SoftwareRenderer,
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    surface: Option<wl_surface::WlSurface>,
    xdg_surface: Option<xdg_surface::XdgSurface>,
    toplevel: Option<xdg_toplevel::XdgToplevel>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    pointer_focus: bool,
    buffers: Vec<ShmBuffer>,
    xkb: Option<XkbContextData>,
    fallback_mods: Modifiers,
    repeat: RepeatState,
}

impl WaylandState {
    fn update_opaque_region(&self, qh: &QueueHandle<Self>) {
        let Some(surface) = &self.surface else {
            return;
        };

        if self.app.config().window.opacity >= 1.0 {
            let Some(compositor) = &self.compositor else {
                return;
            };
            let region: wl_region::WlRegion = compositor.create_region(qh, ());
            region.add(0, 0, self.width as i32, self.height as i32);
            surface.set_opaque_region(Some(&region));
            region.destroy();
        } else {
            surface.set_opaque_region(None);
        }
    }

    fn init_window(&mut self, qh: &QueueHandle<Self>) {
        if self.surface.is_some() || self.compositor.is_none() || self.wm_base.is_none() {
            return;
        }
        let surface = self.compositor.as_ref().unwrap().create_surface(qh, ());
        let xdg_surface = self
            .wm_base
            .as_ref()
            .unwrap()
            .get_xdg_surface(&surface, qh, ());
        let toplevel = xdg_surface.get_toplevel(qh, ());
        toplevel.set_title(self.app.title().into());
        surface.commit();
        self.surface = Some(surface);
        self.xdg_surface = Some(xdg_surface);
        self.toplevel = Some(toplevel);
        self.update_opaque_region(qh);
    }

    fn ensure_buffers(&mut self, qh: &QueueHandle<Self>) {
        if self.buffers.len() == BUFFER_COUNT
            && self
                .buffers
                .iter()
                .all(|b| b.width == self.width && b.height == self.height)
        {
            return;
        }
        self.buffers.clear();
        for idx in 0..BUFFER_COUNT {
            if let Some(shm) = &self.shm {
                if let Ok(buf) = ShmBuffer::new(shm, self.width, self.height, qh, idx) {
                    self.buffers.push(buf);
                }
            }
        }
        let (cols, rows) = self.renderer.grid_size_for_window(self.width, self.height);
        let (ox, oy) = self.renderer.grid_offset();
        self.app.set_grid_offset(ox, oy);
        self.app.handle_resize(cols, rows);
    }

    fn render_frame(&mut self, qh: &QueueHandle<Self>) {
        self.frame_ready = false;
        if self.surface.is_none() || self.shm.is_none() {
            return;
        }
        self.update_opaque_region(qh);
        self.ensure_buffers(qh);
        let Some(buf) = self.buffers.iter_mut().find(|b| !b.busy) else {
            return;
        };

        let keep_animating = self.app.tick();
        if self.app.take_title_changed() {
            if let Some(toplevel) = &self.toplevel {
                toplevel.set_title(self.app.title().into());
            }
        }

        let mut fb = FrameBuffer {
            width: buf.width,
            height: buf.height,
            stride: buf.stride,
            pixels: buf.pixels_mut(),
        };
        self.renderer.render(
            &mut fb,
            self.app.grid(),
            self.app.cursor_renderer(),
            self.app.selection(),
            self.app.damage(),
        );
        self.app.clear_damage();

        let surface = self.surface.as_ref().unwrap();
        surface.damage_buffer(0, 0, self.width as i32, self.height as i32);
        surface.attach(Some(&buf.buffer), 0, 0);
        if keep_animating || self.repeat.key.is_some() {
            surface.frame(qh, ());
        }
        surface.commit();
        buf.busy = true;
    }

    fn apply_zoom_action(&mut self, action: ZoomAction) {
        let step = self.app.config().font.size_adjustment.max(0.1);
        match action {
            ZoomAction::In => self.renderer.increase_font_size(step),
            ZoomAction::Out => self.renderer.decrease_font_size(step),
            ZoomAction::Reset => self.renderer.reset_font_size(),
        }
        let (cw, ch) = self.renderer.cell_size();
        self.app.set_cell_size(cw, ch);
        let (cols, rows) = self.renderer.grid_size_for_window(self.width, self.height);
        let (ox, oy) = self.renderer.grid_offset();
        self.app.set_grid_offset(ox, oy);
        self.app.handle_resize(cols, rows);
        self.frame_ready = true;
    }

    fn local_key_action_for(&self, event: &KeyEvent) -> Option<ZoomAction> {
        if !event.pressed || !self.app.modifiers().control_key() {
            return None;
        }

        match &event.key {
            crate::input::Key::Text(text) => match text.as_str() {
                "+" | "=" => Some(ZoomAction::In),
                "-" => Some(ZoomAction::Out),
                "0" => Some(ZoomAction::Reset),
                _ => None,
            },
            _ => None,
        }
    }

    fn handle_local_key_event(&mut self, event: &KeyEvent) -> bool {
        if let Some(action) = self.local_key_action_for(event) {
            self.apply_zoom_action(action);
            true
        } else {
            false
        }
    }

    fn fire_repeat(&mut self) {
        let Some(key) = self.repeat.key else {
            return;
        };
        let Some(next) = self.repeat.next_fire else {
            return;
        };
        if Instant::now() < next {
            return;
        }

        let ev = if let Some(xkb) = &mut self.xkb {
            let ev = xkb.repeat_key_event(key);
            self.app.set_modifiers(xkb.modifiers());
            if matches!(ev.key, crate::input::Key::Unknown) {
                fallback_key_event(key, true, self.fallback_mods)
            } else {
                ev
            }
        } else {
            self.app.set_modifiers(self.fallback_mods);
            fallback_key_event(key, true, self.fallback_mods)
        };

        if !self.handle_local_key_event(&ev) {
            self.app.handle_key_event(&ev);
        }
        self.repeat.reschedule();
    }
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    conn.display().get_registry(&qh, ());

    let mut app = CoreApp::new();
    let padding = app.config().window.padding;
    let mut renderer = SoftwareRenderer::new(
        app.config().font.size,
        app.theme().clone(),
        padding,
        app.config().window.opacity,
    );
    let (cw, ch) = renderer.cell_size();
    app.set_cell_size(cw, ch);
    let (cols, rows) = renderer.grid_size_for_window(INITIAL_WIDTH, INITIAL_HEIGHT);
    let (ox, oy) = renderer.grid_offset();
    app.set_grid_offset(ox, oy);
    app.init_terminal(cols, rows);

    let mut state = WaylandState {
        running: true,
        configured: false,
        frame_ready: false,
        width: INITIAL_WIDTH,
        height: INITIAL_HEIGHT,
        app,
        renderer,
        compositor: None,
        shm: None,
        wm_base: None,
        surface: None,
        xdg_surface: None,
        toplevel: None,
        keyboard: None,
        pointer: None,
        pointer_focus: false,
        buffers: Vec::new(),
        xkb: None,
        fallback_mods: Modifiers::empty(),
        repeat: RepeatState::default(),
    };

    while state.running {
        let _ = event_queue.dispatch_pending(&mut state)?;
        state.fire_repeat();
        if state.configured && state.frame_ready {
            state.render_frame(&qh);
        }
        if state.app.pty_exited() || !state.running {
            break;
        }

        event_queue.flush()?;

        let timeout_ms = state.repeat.timeout_ms();
        let pty_fd = state.app.pty_raw_fd();
        if let Some(guard) = event_queue.prepare_read() {
            let mut pfds = [
                libc::pollfd {
                    fd: guard.connection_fd().as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                },
                libc::pollfd {
                    fd: pty_fd.unwrap_or(-1),
                    events: libc::POLLIN,
                    revents: 0,
                },
            ];
            let nfds = if pty_fd.is_some() { 2 } else { 1 };
            // SAFETY: pfds is a stack-allocated array of valid pollfd structs.
            // nfds correctly reflects how many entries are valid (1 or 2).
            // timeout_ms is a valid poll timeout (-1 = infinite, 0 = immediate, >0 = ms).
            let rc = unsafe { libc::poll(pfds.as_mut_ptr(), nfds, timeout_ms) };
            if rc > 0 {
                if (pfds[0].revents & libc::POLLIN) != 0 {
                    let _ = guard.read()?;
                }
                if pty_fd.is_some()
                    && (pfds[1].revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR)) != 0
                {
                    state.frame_ready = true;
                }
            }
        } else if timeout_ms > 0 {
            std::thread::sleep(Duration::from_millis(timeout_ms as u64));
        }
    }

    Ok(())
}
