mod dispatch;
mod keyboard;
mod shm;

use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use wayland_client::protocol::{
    wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_region, wl_shm, wl_surface,
};
use wayland_client::{Connection, QueueHandle};
use wayland_protocols::wp::fractional_scale::v1::client::{
    wp_fractional_scale_manager_v1, wp_fractional_scale_v1,
};
use wayland_protocols::wp::viewporter::client::{wp_viewport, wp_viewporter};
use wayland_protocols::xdg::decoration::zv1::client::{
    zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1,
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use crate::cli::CliArgs;
use crate::core_app::CoreApp;
use crate::defaults::{DEFAULT_WINDOW_INITIAL_HEIGHT, DEFAULT_WINDOW_INITIAL_WIDTH};
use crate::info_log;
use crate::input::{KeyEvent, Modifiers};
use crate::keybindings::LocalAction;
use crate::software_renderer::{FrameBuffer, SoftwareRenderer};

use keyboard::{fallback_key_event, RepeatState, XkbContextData};
use shm::ShmBuffer;

const BUFFER_COUNT: usize = 3;

// ─── Zoom ────────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ZoomAction {
    In,
    Out,
    Reset,
}

// ─── Output tracking ─────────────────────────────────────────────────────────

struct OutputInfo {
    output: wl_output::WlOutput,
    /// Registry name (unique id for this global).
    name: u32,
    /// Integer scale factor from wl_output.scale (default 1).
    scale: i32,
}

// ─── Wayland state ───────────────────────────────────────────────────────────

struct WaylandState {
    running: bool,
    configured: bool,
    frame_ready: bool,
    /// Current logical (surface-local) width that we may render/commit.
    width: u32,
    /// Current logical (surface-local) height that we may render/commit.
    height: u32,
    /// Pending logical width from xdg_toplevel.configure.
    pending_width: u32,
    /// Pending logical height from xdg_toplevel.configure.
    pending_height: u32,
    /// Current effective scale factor (≥ 1.0).
    scale: f32,
    app: CoreApp,
    renderer: SoftwareRenderer,
    compositor: Option<wl_compositor::WlCompositor>,
    shm: Option<wl_shm::WlShm>,
    wm_base: Option<xdg_wm_base::XdgWmBase>,
    decoration_manager: Option<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1>,
    surface: Option<wl_surface::WlSurface>,
    xdg_surface: Option<xdg_surface::XdgSurface>,
    toplevel: Option<xdg_toplevel::XdgToplevel>,
    toplevel_decoration: Option<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1>,
    keyboard: Option<wl_keyboard::WlKeyboard>,
    pointer: Option<wl_pointer::WlPointer>,
    pointer_focus: bool,
    /// Buffers eligible for the next attach.
    buffers: Vec<ShmBuffer>,
    /// Old buffers kept alive until the compositor releases them.
    retired_buffers: Vec<ShmBuffer>,
    /// Most recently attached live buffer, used as the copy source for damage-only redraws.
    last_presented_buffer: Option<usize>,
    /// Cursor rows redrawn in the previous frame, so the old cursor image can be erased.
    last_cursor_rows: Option<(usize, usize)>,
    /// Previous cursor quad, used to detect same-row movement where row ranges do not change.
    last_cursor_corners: Option<[(f32, f32); 4]>,
    xkb: Option<XkbContextData>,
    fallback_mods: Modifiers,
    repeat: RepeatState,
    // ── Output / scale tracking ──
    /// All known wl_output globals.
    outputs: Vec<OutputInfo>,
    /// Registry names of outputs the surface is currently on.
    surface_outputs: Vec<u32>,
    // ── Fractional scale + viewporter ──
    viewporter: Option<wp_viewporter::WpViewporter>,
    viewport: Option<wp_viewport::WpViewport>,
    fractional_scale_manager: Option<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1>,
    fractional_scale: Option<wp_fractional_scale_v1::WpFractionalScaleV1>,
    /// Scale value from wp_fractional_scale_v1 (if available).
    fractional_scale_value: Option<f32>,
}

impl WaylandState {
    fn update_window_geometry(&self) {
        let Some(xdg_surface) = &self.xdg_surface else {
            return;
        };

        // Tell the compositor the logical bounds of the toplevel surface.
        // This is especially important for compositors like Niri, which use
        // window geometry when placing focus rings / other scene decorations.
        //
        // Without this, some compositors infer geometry from surface extents or
        // buffer state, which can interact poorly with fractional scale +
        // viewported buffers and make the compositor's active border appear
        // visually overlaid into the window.
        xdg_surface.set_window_geometry(0, 0, self.width as i32, self.height as i32);
    }

    fn update_opaque_region(&self, qh: &QueueHandle<Self>) {
        let Some(surface) = &self.surface else {
            return;
        };
        let Some(compositor) = &self.compositor else {
            return;
        };

        let region: wl_region::WlRegion = compositor.create_region(qh, ());
        // Use i32::MAX so the opaque hint covers the surface regardless of
        // in-flight resizes, matching foot's wl_region_add(0, 0, INT32_MAX, INT32_MAX).
        region.add(0, 0, i32::MAX, i32::MAX);
        surface.set_opaque_region(Some(&region));
        region.destroy();
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
        toplevel.set_app_id(self.app.app_id().into());

        // Request server-side decorations when the compositor supports the
        // xdg-decoration protocol. This matches foot's default preference and
        // can materially affect how compositors like Niri place focus rings /
        // active borders relative to the client surface.
        if let Some(manager) = &self.decoration_manager {
            let decoration = manager.get_toplevel_decoration(&toplevel, qh, ());
            decoration.set_mode(zxdg_toplevel_decoration_v1::Mode::ServerSide);
            self.toplevel_decoration = Some(decoration);
        }

        // Create viewport (for fractional scaling)
        if let Some(viewporter) = &self.viewporter {
            self.viewport = Some(viewporter.get_viewport(&surface, qh, ()));
        }
        // Create fractional scale listener
        if let Some(manager) = &self.fractional_scale_manager {
            self.fractional_scale = Some(manager.get_fractional_scale(&surface, qh, ()));
        }

        self.surface = Some(surface);
        self.xdg_surface = Some(xdg_surface);
        self.toplevel = Some(toplevel);
        self.update_window_geometry();
        self.update_opaque_region(qh);
        self.surface.as_ref().unwrap().commit();
    }

    /// Physical (buffer) dimensions = logical × scale.
    fn physical_size(&self) -> (u32, u32) {
        let w = (self.width as f32 * self.scale).round() as u32;
        let h = (self.height as f32 * self.scale).round() as u32;
        (w.max(1), h.max(1))
    }

    /// Returns true when fractional scaling path should be used
    /// (both viewporter and fractional_scale are available).
    fn use_fractional_scaling(&self) -> bool {
        self.viewport.is_some() && self.fractional_scale.is_some()
    }

    fn sync_terminal_geometry_to_window(&mut self) {
        let (phys_w, phys_h) = self.physical_size();
        let (cols, rows) = self.renderer.grid_size_for_window(phys_w, phys_h);
        let (ox, oy) = self.renderer.grid_offset();
        self.app.set_grid_offset(ox, oy);
        self.app.handle_resize(cols, rows);
    }

    fn prune_retired_buffers(&mut self) {
        self.retired_buffers.retain(|buf| buf.busy);
    }

    fn retire_active_buffers(&mut self) {
        let old_buffers = std::mem::take(&mut self.buffers);
        self.last_presented_buffer = None;
        self.last_cursor_rows = None;
        self.last_cursor_corners = None;
        for buf in old_buffers {
            if buf.busy {
                self.retired_buffers.push(buf);
            }
        }
        self.prune_retired_buffers();
    }

    fn apply_pending_configure(&mut self) {
        let mut changed = false;

        if self.pending_width > 0 && self.pending_width != self.width {
            self.width = self.pending_width;
            changed = true;
        }
        if self.pending_height > 0 && self.pending_height != self.height {
            self.height = self.pending_height;
            changed = true;
        }

        if changed {
            // Match foot's configure handling: only start using the new logical
            // size after xdg_surface.configure has been acked. Also keep any
            // busy old buffers alive until the compositor releases them.
            self.retire_active_buffers();
            self.sync_terminal_geometry_to_window();
        }
    }

    fn refresh_terminal_layout_after_metrics_change(&mut self) {
        // Keep the compositor-provided logical window size unchanged.
        // Font/scale changes should reflow the terminal within the existing
        // container instead of resizing the toplevel itself.
        self.sync_terminal_geometry_to_window();
        self.app.damage_mut().mark_all();
        self.last_presented_buffer = None;
        self.last_cursor_rows = None;
        self.last_cursor_corners = None;
    }

    fn ensure_buffers(&mut self, qh: &QueueHandle<Self>) {
        let (phys_w, phys_h) = self.physical_size();
        if self.buffers.len() == BUFFER_COUNT
            && self
                .buffers
                .iter()
                .all(|b| b.width == phys_w && b.height == phys_h)
        {
            return;
        }
        self.retire_active_buffers();
        for idx in 0..BUFFER_COUNT {
            if let Some(shm) = &self.shm {
                if let Ok(buf) = ShmBuffer::new(shm, phys_w, phys_h, qh, idx) {
                    self.buffers.push(buf);
                }
            }
        }
        self.sync_terminal_geometry_to_window();
        self.prune_retired_buffers();
    }

    fn prepare_target_buffer(&mut self, target_idx: usize) -> bool {
        let Some(source_idx) = self.last_presented_buffer else {
            return false;
        };
        if source_idx >= self.buffers.len() || target_idx >= self.buffers.len() {
            return false;
        }
        if self.buffers[source_idx].width != self.buffers[target_idx].width
            || self.buffers[source_idx].height != self.buffers[target_idx].height
        {
            return false;
        }
        if source_idx == target_idx {
            return true;
        }

        if source_idx < target_idx {
            let (left, right) = self.buffers.split_at_mut(target_idx);
            let src = left[source_idx].pixels();
            right[0].pixels_mut().copy_from_slice(src);
        } else {
            let (left, right) = self.buffers.split_at_mut(source_idx);
            let dst = left[target_idx].pixels_mut();
            dst.copy_from_slice(right[0].pixels());
        }
        true
    }

    fn current_cursor_geometry(&self) -> Option<([(f32, f32); 4], (usize, usize))> {
        let grid = self.app.grid();
        let cursor = self.app.cursor_renderer();
        if !grid.cursor_visible() || !cursor.is_visible() || grid.is_viewing_scrollback() {
            return None;
        }

        let corners = cursor.corner_positions();
        let min_row = corners
            .iter()
            .map(|corner| corner.1)
            .fold(f32::INFINITY, f32::min)
            .floor()
            .max(0.0) as usize;
        let max_row_exclusive = corners
            .iter()
            .map(|corner| corner.1)
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .max(0.0) as usize;
        let last_row = grid.rows().saturating_sub(1);
        let start = min_row.min(last_row);
        let end = max_row_exclusive.saturating_sub(1).min(last_row);
        Some((corners, (start.min(end), end)))
    }

    fn mark_row_range_damage(&mut self, range: Option<(usize, usize)>) {
        let Some((start, end)) = range else {
            return;
        };
        let damage = self.app.damage_mut();
        for row in start..=end {
            damage.mark_row(row);
        }
    }

    fn mark_cursor_damage(
        &mut self,
        current_cursor_rows: Option<(usize, usize)>,
        current_cursor_corners: Option<[(f32, f32); 4]>,
        keep_animating: bool,
    ) {
        let geometry_changed = match (self.last_cursor_corners, current_cursor_corners) {
            (Some(prev), Some(curr)) => prev
                .iter()
                .zip(curr.iter())
                .any(|(a, b)| (a.0 - b.0).abs() > 0.001 || (a.1 - b.1).abs() > 0.001),
            (None, None) => false,
            _ => true,
        };

        if keep_animating || geometry_changed || current_cursor_rows != self.last_cursor_rows {
            self.mark_row_range_damage(self.last_cursor_rows);
            self.mark_row_range_damage(current_cursor_rows);
        }
    }

    fn render_frame(&mut self, qh: &QueueHandle<Self>) {
        self.frame_ready = false;
        if self.surface.is_none() || self.shm.is_none() {
            return;
        }
        self.update_window_geometry();
        self.update_opaque_region(qh);
        self.ensure_buffers(qh);

        let Some(target_idx) = self.buffers.iter().position(|b| !b.busy) else {
            return;
        };

        let keep_animating = self.app.tick();
        if self.app.take_title_changed() {
            if let Some(toplevel) = &self.toplevel {
                toplevel.set_title(self.app.title().into());
            }
        }
        if self.app.take_theme_changed() {
            self.renderer.set_theme(self.app.theme().clone());
        }

        let current_cursor_geometry = self.current_cursor_geometry();
        let current_cursor_corners = current_cursor_geometry.map(|(corners, _)| corners);
        let current_cursor_rows = current_cursor_geometry.map(|(_, rows)| rows);
        self.mark_cursor_damage(current_cursor_rows, current_cursor_corners, keep_animating);

        let copied_previous = self.prepare_target_buffer(target_idx);
        let full_redraw = !copied_previous || self.app.damage().is_full_damage();
        let draw_cursor = full_redraw
            || current_cursor_rows
                .map(|(start, end)| (start..=end).any(|row| self.app.damage().is_dirty(row)))
                .unwrap_or(false);

        {
            let buf = &mut self.buffers[target_idx];
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
                full_redraw,
                draw_cursor,
            );
        }

        let damage_ranges = if full_redraw {
            vec![(0, self.app.grid().rows().saturating_sub(1))]
        } else {
            self.app.damage().dirty_row_ranges()
        };
        self.app.clear_damage();
        self.last_cursor_rows = current_cursor_rows;
        self.last_cursor_corners = current_cursor_corners;

        let buf = &mut self.buffers[target_idx];
        let buf_w = buf.width as i32;
        let buf_h = buf.height as i32;

        let surface = self.surface.as_ref().unwrap();
        if full_redraw {
            // damage_buffer uses buffer (physical) coordinates
            surface.damage_buffer(0, 0, buf_w, buf_h);
        } else {
            for (start_row, end_row) in damage_ranges {
                let (y0, y1) = self.renderer.pixel_bounds_for_row_range(start_row, end_row);
                let y1 = y1.min(buf.height);
                if y1 > y0 {
                    surface.damage_buffer(0, y0 as i32, buf_w, (y1 - y0) as i32);
                }
            }
        }
        surface.attach(Some(&buf.buffer), 0, 0);
        buf.busy = true;

        // Apply scaling to the surface
        if self.use_fractional_scaling() {
            // Fractional path: buffer at physical pixels, viewport maps to logical
            surface.set_buffer_scale(1);
            if let Some(viewport) = &self.viewport {
                viewport.set_destination(self.width as i32, self.height as i32);
            }
        } else {
            // Integer path: set_buffer_scale tells compositor the ratio
            surface.set_buffer_scale(self.scale.round().max(1.0) as i32);
        }

        if keep_animating || self.repeat.key.is_some() {
            surface.frame(qh, ());
        }
        surface.commit();
        self.last_presented_buffer = Some(target_idx);
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
        self.refresh_terminal_layout_after_metrics_change();
        self.frame_ready = true;
    }

    /// Recalculate the effective scale from available sources.
    /// Returns true if the scale actually changed.
    fn update_scale(&mut self) -> bool {
        let new_scale = if let Some(frac) = self.fractional_scale_value {
            // Best: fractional scale protocol gives exact value
            frac
        } else if !self.surface_outputs.is_empty() {
            // Integer scale from the output(s) the surface is on
            self.surface_outputs
                .iter()
                .filter_map(|name| self.outputs.iter().find(|o| o.name == *name))
                .map(|o| o.scale)
                .max()
                .unwrap_or(1) as f32
        } else {
            // Fallback: highest scale of any known output, or 1.0
            self.outputs.iter().map(|o| o.scale).max().unwrap_or(1) as f32
        };

        let new_scale = new_scale.max(1.0);

        if (self.scale - new_scale).abs() < 0.001 {
            return false;
        }

        info_log!("Scale changed: {:.3} -> {:.3}", self.scale, new_scale);
        self.scale = new_scale;
        self.renderer.set_scale(new_scale);
        let (cw, ch) = self.renderer.cell_size();
        self.app.set_cell_size(cw, ch);
        self.refresh_terminal_layout_after_metrics_change();
        true
    }

    fn local_key_action_for(&self, event: &KeyEvent) -> Option<ZoomAction> {
        if self
            .app
            .matches_local_keybinding(LocalAction::ZoomIn, event)
        {
            Some(ZoomAction::In)
        } else if self
            .app
            .matches_local_keybinding(LocalAction::ZoomOut, event)
        {
            Some(ZoomAction::Out)
        } else if self
            .app
            .matches_local_keybinding(LocalAction::ZoomReset, event)
        {
            Some(ZoomAction::Reset)
        } else {
            None
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

pub fn run(cli: CliArgs) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let mut event_queue = conn.new_event_queue();
    let qh = event_queue.handle();
    conn.display().get_registry(&qh, ());

    let mut app = CoreApp::new(cli);
    let padding = app.config().window.padding;
    let configured_width = app.config().window.initial_width.max(1);
    let configured_height = app.config().window.initial_height.max(1);
    let configured_cols = app.config().terminal.cols.max(2) as u32;
    let configured_rows = app.config().terminal.rows.max(2) as u32;
    let mut renderer = SoftwareRenderer::new(
        app.config().font.size,
        app.config().font.family.clone(),
        app.theme().clone(),
        padding,
    );
    let (cw, ch) = renderer.cell_size();
    let (fit_width, fit_height) =
        renderer.window_size_for_grid(configured_cols as usize, configured_rows as usize);
    let initial_width = if configured_width == DEFAULT_WINDOW_INITIAL_WIDTH
        && configured_height == DEFAULT_WINDOW_INITIAL_HEIGHT
    {
        fit_width
    } else {
        configured_width
    };
    let initial_height = if configured_width == DEFAULT_WINDOW_INITIAL_WIDTH
        && configured_height == DEFAULT_WINDOW_INITIAL_HEIGHT
    {
        fit_height
    } else {
        configured_height
    };
    app.set_cell_size(cw, ch);
    let (cols, rows) = renderer.grid_size_for_window(initial_width, initial_height);
    let (ox, oy) = renderer.grid_offset();
    app.set_grid_offset(ox, oy);
    app.init_terminal(cols, rows);

    let mut state = WaylandState {
        running: true,
        configured: false,
        frame_ready: false,
        width: initial_width,
        height: initial_height,
        pending_width: initial_width,
        pending_height: initial_height,
        scale: 1.0,
        app,
        renderer,
        compositor: None,
        shm: None,
        wm_base: None,
        decoration_manager: None,
        surface: None,
        xdg_surface: None,
        toplevel: None,
        toplevel_decoration: None,
        keyboard: None,
        pointer: None,
        pointer_focus: false,
        buffers: Vec::new(),
        retired_buffers: Vec::new(),
        last_presented_buffer: None,
        last_cursor_rows: None,
        last_cursor_corners: None,
        xkb: None,
        fallback_mods: Modifiers::empty(),
        repeat: RepeatState::default(),
        outputs: Vec::new(),
        surface_outputs: Vec::new(),
        viewporter: None,
        viewport: None,
        fractional_scale_manager: None,
        fractional_scale: None,
        fractional_scale_value: None,
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

        match event_queue.flush() {
            Ok(()) => {}
            Err(wayland_client::backend::WaylandError::Io(err))
                if err.kind() == ErrorKind::WouldBlock => {}
            Err(err) => return Err(Box::new(err)),
        }

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
            if rc < 0 {
                let errno = unsafe { *libc::__errno_location() };
                if errno == libc::EINTR {
                    continue;
                }
                return Err(Box::new(std::io::Error::from_raw_os_error(errno)));
            }
            if rc > 0 {
                if (pfds[0].revents & libc::POLLIN) != 0 {
                    match guard.read() {
                        Ok(_) => {}
                        Err(wayland_client::backend::WaylandError::Io(err))
                            if err.kind() == ErrorKind::WouldBlock => {}
                        Err(err) => return Err(Box::new(err)),
                    }
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
