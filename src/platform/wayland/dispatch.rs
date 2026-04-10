//! Wayland protocol dispatch implementations.

use std::os::fd::AsRawFd;

use wayland_client::{
    delegate_noop,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_output, wl_pointer, wl_region,
        wl_registry, wl_seat, wl_shm, wl_shm_pool, wl_surface,
    },
    Connection, Dispatch, QueueHandle, WEnum,
};
use crate::protocols::fractional_scale::{
    wp_fractional_scale_manager_v1, wp_fractional_scale_v1,
};
use crate::protocols::viewporter::{wp_viewport, wp_viewporter};
use crate::protocols::xdg_decoration::{
    zxdg_decoration_manager_v1, zxdg_toplevel_decoration_v1,
};
use crate::protocols::xdg_shell::{xdg_surface, xdg_toplevel, xdg_wm_base};

use crate::info_log;
use crate::input::Key;
use crate::warn_log;

const FRACTIONAL_SCALE_DIVISOR: f32 = 120.0;
const WAYLAND_SCROLL_DISCRETE_FACTOR: f64 = 120.0;

use super::keyboard::{
    fallback_key_event, fallback_key_is_repeatable, map_pointer_button, update_fallback_modifiers,
    XkbContextData,
};
use super::{OutputInfo, WaylandState, ZoomAction};

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => match interface.as_str() {
                "wl_compositor" => {
                    let compositor =
                        registry.bind::<wl_compositor::WlCompositor, _, _>(name, 4, qh, ());
                    state.compositor = Some(compositor);
                    state.init_window(qh);
                }
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(name, 1, qh, ());
                    state.shm = Some(shm);
                    // xdg_surface.configure can race ahead of wl_shm discovery.
                    // If we already consumed the initial frame request while shm
                    // was still missing, request another render now that buffers
                    // can actually be created.
                    if state.surface.is_some() {
                        state.frame_ready = true;
                    }
                }
                "wl_seat" => {
                    registry.bind::<wl_seat::WlSeat, _, _>(name, 5, qh, ());
                }
                "xdg_wm_base" => {
                    let wm_base = registry.bind::<xdg_wm_base::XdgWmBase, _, _>(name, 1, qh, ());
                    state.wm_base = Some(wm_base);
                    state.init_window(qh);
                }
                "zxdg_decoration_manager_v1" => {
                    let manager = registry
                        .bind::<zxdg_decoration_manager_v1::ZxdgDecorationManagerV1, _, _>(
                            name,
                            1,
                            qh,
                            (),
                        );
                    state.decoration_manager = Some(manager);
                }
                "wl_output" => {
                    let ver = version.min(4);
                    let output = registry.bind::<wl_output::WlOutput, _, _>(name, ver, qh, name);
                    state.outputs.push(OutputInfo {
                        output,
                        name,
                        scale: 1,
                    });
                }
                "wp_viewporter" => {
                    let vp = registry.bind::<wp_viewporter::WpViewporter, _, _>(name, 1, qh, ());
                    state.viewporter = Some(vp);
                }
                "wp_fractional_scale_manager_v1" => {
                    let mgr = registry
                        .bind::<wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1, _, _>(
                            name,
                            1,
                            qh,
                            (),
                        );
                    state.fractional_scale_manager = Some(mgr);
                }
                _ => {}
            },
            wl_registry::Event::GlobalRemove { name } => {
                let old_len = state.outputs.len();
                state.outputs.retain(|info| info.name != name);
                state
                    .surface_outputs
                    .retain(|output_name| *output_name != name);
                if state.outputs.len() != old_len && state.update_scale() {
                    state.frame_ready = true;
                }
            }
            _ => {}
        }
    }
}

delegate_noop!(WaylandState: ignore wl_compositor::WlCompositor);
delegate_noop!(WaylandState: ignore wl_region::WlRegion);
delegate_noop!(WaylandState: ignore wl_shm::WlShm);
delegate_noop!(WaylandState: ignore wl_shm_pool::WlShmPool);
delegate_noop!(WaylandState: ignore wp_viewporter::WpViewporter);
delegate_noop!(WaylandState: ignore wp_viewport::WpViewport);
delegate_noop!(WaylandState: ignore wp_fractional_scale_manager_v1::WpFractionalScaleManagerV1);
delegate_noop!(WaylandState: ignore zxdg_decoration_manager_v1::ZxdgDecorationManagerV1);

// ─── wl_output: track per-output integer scale ──────────────────────────────────

impl Dispatch<wl_output::WlOutput, u32> for WaylandState {
    fn event(
        state: &mut Self,
        _output: &wl_output::WlOutput,
        event: wl_output::Event,
        name: &u32,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_output::Event::Scale { factor } => {
                if let Some(info) = state.outputs.iter_mut().find(|o| o.name == *name) {
                    info.scale = factor;
                }
            }
            wl_output::Event::Done => {
                // All output properties received; recalculate effective scale.
                if state.update_scale() {
                    state.frame_ready = true;
                }
            }
            _ => {}
        }
    }
}

// ─── wl_surface: track enter/leave for output scale ─────────────────────────────

impl Dispatch<wl_surface::WlSurface, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _surface: &wl_surface::WlSurface,
        event: wl_surface::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        match event {
            wl_surface::Event::Enter { output } => {
                if let Some(info) = state.outputs.iter().find(|o| o.output == output) {
                    let name = info.name;
                    if !state.surface_outputs.contains(&name) {
                        state.surface_outputs.push(name);
                        if state.update_scale() {
                            state.frame_ready = true;
                        }
                    }
                }
            }
            wl_surface::Event::Leave { output } => {
                if let Some(info) = state.outputs.iter().find(|o| o.output == output) {
                    let name = info.name;
                    state.surface_outputs.retain(|n| *n != name);
                    if state.update_scale() {
                        state.frame_ready = true;
                    }
                }
            }
            _ => {}
        }
    }
}

// ─── wp_fractional_scale_v1: precise fractional scale ────────────────────────────

impl Dispatch<wp_fractional_scale_v1::WpFractionalScaleV1, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _proxy: &wp_fractional_scale_v1::WpFractionalScaleV1,
        event: wp_fractional_scale_v1::Event,
        _: &(),
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
    ) {
        if let wp_fractional_scale_v1::Event::PreferredScale { scale } = event {
            // Wire format: scale × 120 (e.g. 180 = 1.5×, 240 = 2.0×)
            let new_scale = scale as f32 / FRACTIONAL_SCALE_DIVISOR;
            state.fractional_scale_value = Some(new_scale);
            if state.update_scale() {
                state.frame_ready = true;
            }
        }
    }
}

impl Dispatch<wl_callback::WlCallback, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _callback: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            state.frame_callback_pending = false;
            state.frame_ready = true;
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, usize> for WaylandState {
    fn event(
        state: &mut Self,
        released_buffer: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        _idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            // Match strictly by wl_buffer object. Buffer slots are recreated on
            // resize/scale changes, and old release events can arrive after a
            // new generation has reused the same logical slot index.
            let mut released = false;

            if let Some(buf) = state
                .buffers
                .iter_mut()
                .find(|buf| buf.buffer == *released_buffer)
            {
                buf.busy = false;
                released = true;
            }

            if !released {
                if let Some(buf) = state
                    .retired_buffers
                    .iter_mut()
                    .find(|buf| buf.buffer == *released_buffer)
                {
                    buf.busy = false;
                    released = true;
                }
            }

            if released {
                state.prune_retired_buffers();
                state.frame_ready = true;
            }
        }
    }
}

impl Dispatch<xdg_wm_base::XdgWmBase, ()> for WaylandState {
    fn event(
        _: &mut Self,
        wm_base: &xdg_wm_base::XdgWmBase,
        event: xdg_wm_base::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_wm_base::Event::Ping { serial } = event {
            wm_base.pong(serial);
        }
    }
}

impl Dispatch<xdg_surface::XdgSurface, ()> for WaylandState {
    fn event(
        state: &mut Self,
        xdg_surface: &xdg_surface::XdgSurface,
        event: xdg_surface::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let xdg_surface::Event::Configure { serial } = event {
            // Like foot, only apply the pending toplevel configure after we've
            // acked the matching xdg_surface.configure. Rendering/attaching a
            // buffer for the new logical size before that can trip protocol
            // errors during fast fullscreen/resize transitions.
            xdg_surface.ack_configure(serial);
            state.configured = true;
            state.apply_pending_configure();
            state.frame_ready = true;
        }
    }
}

impl Dispatch<xdg_toplevel::XdgToplevel, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &xdg_toplevel::XdgToplevel,
        event: xdg_toplevel::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            xdg_toplevel::Event::Close => state.running = false,
            xdg_toplevel::Event::Configure { width, height, .. } => {
                if width > 0 {
                    state.pending_width = width as u32;
                }
                if height > 0 {
                    state.pending_height = height as u32;
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1, ()> for WaylandState {
    fn event(
        _state: &mut Self,
        _decoration: &zxdg_toplevel_decoration_v1::ZxdgToplevelDecorationV1,
        event: zxdg_toplevel_decoration_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let zxdg_toplevel_decoration_v1::Event::Configure { mode: _mode } = event {}
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for WaylandState {
    fn event(
        state: &mut Self,
        seat: &wl_seat::WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(capabilities),
        } = event
        {
            let has_keyboard = capabilities.contains(wl_seat::Capability::Keyboard);
            if has_keyboard {
                if state.keyboard.is_none() {
                    state.keyboard = Some(seat.get_keyboard(qh, ()));
                }
            } else {
                state.keyboard = None;
            }

            let has_pointer = capabilities.contains(wl_seat::Capability::Pointer);
            if has_pointer {
                if state.pointer.is_none() {
                    state.pointer = Some(seat.get_pointer(qh, ()));
                }
            } else {
                state.pointer = None;
                state.pointer_focus = false;
            }
        }
    }
}

impl Dispatch<wl_pointer::WlPointer, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &wl_pointer::WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_pointer::Event::Enter {
                surface_x,
                surface_y,
                ..
            } => {
                state.pointer_focus = true;
                // Convert surface-local (logical) coords to physical pixels
                let s = state.scale as f64;
                state.app.handle_mouse_move(surface_x * s, surface_y * s);
                state.frame_ready = true;
            }
            wl_pointer::Event::Leave { .. } => {
                state.pointer_focus = false;
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } => {
                if state.pointer_focus {
                    let s = state.scale as f64;
                    let before = state.app.selection().snapshot();
                    state.app.handle_mouse_move(surface_x * s, surface_y * s);
                    let after = state.app.selection().snapshot();
                    if before != after {
                        state.frame_ready = true;
                    }
                }
            }
            wl_pointer::Event::Button {
                button,
                state: button_state,
                ..
            } => {
                if state.pointer_focus {
                    let pressed =
                        matches!(button_state, WEnum::Value(wl_pointer::ButtonState::Pressed));
                    state
                        .app
                        .handle_mouse_button(pressed, map_pointer_button(button));
                    state.frame_ready = true;
                }
            }
            wl_pointer::Event::Axis {
                axis: WEnum::Value(wl_pointer::Axis::VerticalScroll),
                value,
                ..
            } => {
                if state.pointer_focus && value != 0.0 {
                    if state.app.modifiers().control_key() {
                        let action = if value < 0.0 {
                            ZoomAction::In
                        } else {
                            ZoomAction::Out
                        };
                        state.apply_zoom_action(action);
                    } else {
                        state
                            .app
                            .handle_scroll_lines(-value / WAYLAND_SCROLL_DISCRETE_FACTOR);
                        state.frame_ready = true;
                    }
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<wl_keyboard::WlKeyboard, ()> for WaylandState {
    fn event(
        state: &mut Self,
        _: &wl_keyboard::WlKeyboard,
        event: wl_keyboard::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            wl_keyboard::Event::Keymap {
                format: WEnum::Value(wl_keyboard::KeymapFormat::XkbV1),
                fd,
                size,
            } => {
                let map_len = size as usize;
                if map_len == 0 {
                    warn_log!("Wayland keymap was empty, using fallback keymap");
                } else {
                    // Match foot here: wl_keyboard.keymap hands us a shared
                    // file descriptor intended for mmap(). Some compositors /
                    // proxy layers are unhappy when clients try to stream-read
                    // the whole fd, which left us stuck on the fallback US
                    // keymap and broke synthetic input (e.g. wtype/Handy).
                    let map_ptr = unsafe {
                        libc::mmap(
                            std::ptr::null_mut(),
                            map_len,
                            libc::PROT_READ,
                            libc::MAP_PRIVATE,
                            fd.as_raw_fd(),
                            0,
                        )
                    };
                    if map_ptr == libc::MAP_FAILED {
                        warn_log!(
                            "Wayland keymap mmap failed ({}), using fallback keymap",
                            std::io::Error::last_os_error()
                        );
                    } else {
                        let keymap_buf =
                            unsafe { std::slice::from_raw_parts(map_ptr.cast::<u8>(), map_len) };
                        let trimmed_len = keymap_buf
                            .iter()
                            .rposition(|&byte| byte != 0)
                            .map(|idx| idx + 1)
                            .unwrap_or(0);
                        state.xkb = XkbContextData::from_keymap_string(&keymap_buf[..trimmed_len]);
                        unsafe {
                            libc::munmap(map_ptr, map_len);
                        }
                        if let Some(xkb) = &mut state.xkb {
                            info_log!("Wayland xkb keymap loaded");
                            state.app.set_modifiers(xkb.modifiers());
                        } else {
                            warn_log!("Wayland xkb keymap parse failed, using fallback keymap");
                        }
                    }
                }
            }
            wl_keyboard::Event::Modifiers {
                mods_depressed,
                mods_latched,
                mods_locked,
                group,
                ..
            } => {
                if let Some(xkb) = &mut state.xkb {
                    xkb.update_modifiers(mods_depressed, mods_latched, mods_locked, group);
                    state.app.set_modifiers(xkb.modifiers());
                }
            }
            wl_keyboard::Event::RepeatInfo { rate, delay } => {
                state.repeat.rate = rate;
                state.repeat.delay = delay;
            }
            wl_keyboard::Event::Key {
                key,
                state: key_state,
                ..
            } => {
                let pressed = matches!(key_state, WEnum::Value(wl_keyboard::KeyState::Pressed));
                update_fallback_modifiers(&mut state.fallback_mods, key, pressed);
                let should_repeat = if let Some(xkb) = &mut state.xkb {
                    xkb.key_repeats(key)
                } else {
                    fallback_key_is_repeatable(key)
                };

                let ev = if let Some(xkb) = &mut state.xkb {
                    let ev = xkb.key_event(key, pressed);
                    state.app.set_modifiers(xkb.modifiers());
                    if matches!(ev.key, Key::Unknown) {
                        fallback_key_event(key, pressed, state.fallback_mods)
                    } else {
                        ev
                    }
                } else {
                    state.app.set_modifiers(state.fallback_mods);
                    fallback_key_event(key, pressed, state.fallback_mods)
                };

                if !state.handle_local_key_event(&ev) {
                    state.app.handle_key_event(&ev);
                }

                if pressed && should_repeat {
                    state.repeat.start(key);
                } else {
                    state.repeat.stop(Some(key));
                }
            }
            wl_keyboard::Event::Leave { .. } => {
                state.app.handle_focus_change(false);
                state.repeat.stop(None);
                state.fallback_mods = crate::input::Modifiers::empty();
                if let Some(xkb) = &mut state.xkb {
                    xkb.reset_state();
                    state.app.set_modifiers(xkb.modifiers());
                } else {
                    state.app.set_modifiers(state.fallback_mods);
                }
            }
            wl_keyboard::Event::Enter { .. } => {
                state.app.handle_focus_change(true);
            }
            _ => {}
        }
    }
}
