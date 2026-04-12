#include <wayland-client.h>
#include "xdg-shell-client-protocol.h"
#include "xdg-decoration-unstable-v1-client-protocol.h"
#include "viewporter-client-protocol.h"
#include "fractional-scale-v1-client-protocol.h"
const struct wl_interface xdg_wm_base_interface = { "xdg_wm_base", 1, 0, NULL, 0, NULL };
const struct wl_interface xdg_surface_interface = { "xdg_surface", 1, 0, NULL, 0, NULL };
const struct wl_interface xdg_toplevel_interface = { "xdg_toplevel", 1, 0, NULL, 0, NULL };
const struct wl_interface zxdg_decoration_manager_v1_interface = { "zxdg_decoration_manager_v1", 1, 0, NULL, 0, NULL };
const struct wl_interface zxdg_toplevel_decoration_v1_interface = { "zxdg_toplevel_decoration_v1", 1, 0, NULL, 0, NULL };
const struct wl_interface wp_viewporter_interface = { "wp_viewporter", 1, 0, NULL, 0, NULL };
const struct wl_interface wp_viewport_interface = { "wp_viewport", 1, 0, NULL, 0, NULL };
const struct wl_interface wp_fractional_scale_manager_v1_interface = { "wp_fractional_scale_manager_v1", 1, 0, NULL, 0, NULL };
const struct wl_interface wp_fractional_scale_v1_interface = { "wp_fractional_scale_v1", 1, 0, NULL, 0, NULL };
