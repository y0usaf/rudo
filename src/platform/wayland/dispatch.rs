//! Wayland protocol dispatch implementations.

use std::fs::File;
use std::io::Read;

use wayland_client::{
    delegate_noop,
    protocol::{
        wl_buffer, wl_callback, wl_compositor, wl_keyboard, wl_pointer, wl_region, wl_registry,
        wl_seat, wl_shm, wl_shm_pool, wl_surface,
    },
    Connection, Dispatch, QueueHandle, WEnum,
};
use wayland_protocols::xdg::shell::client::{xdg_surface, xdg_toplevel, xdg_wm_base};

use crate::input::Key;

use super::keyboard::{
    fallback_key_event, fallback_key_is_repeatable, map_pointer_button, update_fallback_modifiers,
    XkbContextData,
};
use super::WaylandState;
use super::ZoomAction;

impl Dispatch<wl_registry::WlRegistry, ()> for WaylandState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name, interface, ..
        } = event
        {
            match interface.as_str() {
                "wl_compositor" => {
                    let compositor =
                        registry.bind::<wl_compositor::WlCompositor, _, _>(name, 4, qh, ());
                    state.compositor = Some(compositor);
                    state.init_window(qh);
                }
                "wl_shm" => {
                    let shm = registry.bind::<wl_shm::WlShm, _, _>(name, 1, qh, ());
                    state.shm = Some(shm);
                }
                "wl_seat" => {
                    registry.bind::<wl_seat::WlSeat, _, _>(name, 5, qh, ());
                }
                "xdg_wm_base" => {
                    let wm_base = registry.bind::<xdg_wm_base::XdgWmBase, _, _>(name, 1, qh, ());
                    state.wm_base = Some(wm_base);
                    state.init_window(qh);
                }
                _ => {}
            }
        }
    }
}

delegate_noop!(WaylandState: ignore wl_compositor::WlCompositor);
delegate_noop!(WaylandState: ignore wl_surface::WlSurface);
delegate_noop!(WaylandState: ignore wl_region::WlRegion);
delegate_noop!(WaylandState: ignore wl_shm::WlShm);
delegate_noop!(WaylandState: ignore wl_shm_pool::WlShmPool);

impl Dispatch<wl_callback::WlCallback, ()> for WaylandState {
    fn event(
        state: &mut Self,
        callback: &wl_callback::WlCallback,
        event: wl_callback::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_callback::Event::Done { .. } = event {
            let _ = callback;
            state.frame_ready = true;
        }
    }
}

impl Dispatch<wl_buffer::WlBuffer, usize> for WaylandState {
    fn event(
        state: &mut Self,
        _: &wl_buffer::WlBuffer,
        event: wl_buffer::Event,
        idx: &usize,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let wl_buffer::Event::Release = event {
            if let Some(buf) = state.buffers.get_mut(*idx) {
                buf.busy = false;
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
            xdg_surface.ack_configure(serial);
            state.configured = true;
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
                    state.width = width as u32;
                }
                if height > 0 {
                    state.height = height as u32;
                }
            }
            _ => {}
        }
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
            if capabilities.contains(wl_seat::Capability::Keyboard) && state.keyboard.is_none() {
                state.keyboard = Some(seat.get_keyboard(qh, ()));
            }
            if capabilities.contains(wl_seat::Capability::Pointer) && state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(qh, ()));
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
                state.app.handle_mouse_move(surface_x, surface_y);
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
                    let selection_before = state.app.selection().state;
                    let selection_start_before = state.app.selection().start;
                    let selection_end_before = state.app.selection().end;
                    state.app.handle_mouse_move(surface_x, surface_y);
                    let selection_after = state.app.selection().state;
                    let selection_start_after = state.app.selection().start;
                    let selection_end_after = state.app.selection().end;
                    if selection_before != selection_after
                        || selection_start_before != selection_start_after
                        || selection_end_before != selection_end_after
                    {
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
                        state.app.handle_scroll_lines(-value / 120.0);
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
                let mut file = File::from(fd);
                let mut buf = vec![0u8; size as usize];
                if file.read_exact(&mut buf).is_ok() {
                    state.xkb = XkbContextData::from_keymap_string(&buf);
                    if let Some(xkb) = &mut state.xkb {
                        eprintln!("[INFO] Wayland xkb keymap loaded");
                        state.app.set_modifiers(xkb.modifiers());
                    } else {
                        eprintln!("[WARN] Wayland xkb keymap parse failed, using fallback keymap");
                    }
                } else {
                    eprintln!("[WARN] Wayland keymap read failed, using fallback keymap");
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
                state.repeat.stop(None);
            }
            _ => {}
        }
    }
}
