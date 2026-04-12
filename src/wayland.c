#define _GNU_SOURCE
#include <linux/input-event-codes.h>
#include "rudo/wayland.h"
#include "rudo/common.h"
#include "rudo/defaults.h"
#include "rudo/log.h"
#include "wayland_keyboard.h"

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <math.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/types.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>

#include "xdg-shell-client-protocol.h"
#include "xdg-decoration-unstable-v1-client-protocol.h"
#include "viewporter-client-protocol.h"
#include "fractional-scale-v1-client-protocol.h"

#define RUDO_WL_BUF_COUNT 3
#define RUDO_WL_MAX_DIM 16384u

typedef struct {
    struct wl_buffer *buffer;
    void *map;
    size_t size;
    uint32_t width, height, stride;
    bool busy;
} shm_buf;

typedef struct { uint32_t name; int32_t scale; struct wl_output *output; } output_info;

static const struct wl_callback_listener frame_listener;

struct rudo_wayland_app {
    struct wl_display *display;
    struct wl_registry *registry;
    struct wl_compositor *compositor;
    struct wl_shm *shm;
    struct wl_seat *seat;
    struct wl_pointer *pointer;
    struct wl_keyboard *keyboard;
    struct xdg_wm_base *wm_base;
    struct wl_surface *surface;
    struct xdg_surface *xdg_surface;
    struct xdg_toplevel *toplevel;
    struct zxdg_decoration_manager_v1 *deco_mgr;
    struct zxdg_toplevel_decoration_v1 *deco;
    struct wp_viewporter *viewporter;
    struct wp_viewport *viewport;
    struct wp_fractional_scale_manager_v1 *frac_mgr;
    struct wp_fractional_scale_v1 *frac;
    struct wl_callback *frame_cb;
    rudo_core_app *app;
    rudo_software_renderer *renderer;
    shm_buf bufs[RUDO_WL_BUF_COUNT];
    output_info *outputs;
    size_t outputs_len, outputs_cap;
    uint32_t *surface_outputs;
    size_t surface_outputs_len, surface_outputs_cap;
    uint32_t width, height;
    uint32_t pending_width, pending_height;
    float scale, frac_scale;
    bool have_frac;
    bool configured, running, frame_ready, pointer_focus, pending_wl_read;
    double pointer_x, pointer_y;
    uint32_t serial;
    rudo_wayland_keyboard kb;
};

static int clamp_dim(uint32_t v) { return (int)RUDO_CLAMP(v ? v : 1u, 1u, RUDO_WL_MAX_DIM); }
static uint8_t bg_alpha(float o) { if (!isfinite(o)) o = 1.f; o = RUDO_CLAMP(o, 0.f, 1.f); return (uint8_t)lroundf(o * 255.f); }
static float effective_scale(rudo_wayland_app *wl) { size_t i; int32_t best = 1; if (wl->have_frac && wl->frac_scale >= 1.f) return wl->frac_scale; for (i = 0; i < wl->surface_outputs_len; ++i) { size_t j; for (j = 0; j < wl->outputs_len; ++j) if (wl->outputs[j].name == wl->surface_outputs[i] && wl->outputs[j].scale > best) best = wl->outputs[j].scale; } return (float)(best > 0 ? best : 1); }
static void set_scale(rudo_wayland_app *wl) { float s = effective_scale(wl); if (fabsf(s - wl->scale) < 0.001f) return; wl->scale = s; rudo_software_renderer_set_scale(wl->renderer, s); wl->frame_ready = true; }
static void app_sync_renderer(rudo_wayland_app *wl) { float cw, ch, ox, oy; rudo_software_renderer_cell_size(wl->renderer, &cw, &ch); rudo_software_renderer_grid_offset(wl->renderer, &ox, &oy); rudo_core_app_set_cell_size(wl->app, cw, ch); rudo_core_app_set_grid_offset(wl->app, ox, oy); }
static bool push_u32(uint32_t **v, size_t *len, size_t *cap, uint32_t x) { if (*len == *cap) { size_t nc = *cap ? *cap * 2u : 8u; *v = rudo_realloc(*v, nc * sizeof(**v)); *cap = nc; } (*v)[(*len)++] = x; return true; }
static bool push_output(output_info **v, size_t *len, size_t *cap, output_info x) { if (*len == *cap) { size_t nc = *cap ? *cap * 2u : 8u; *v = rudo_realloc(*v, nc * sizeof(**v)); *cap = nc; } (*v)[(*len)++] = x; return true; }
static output_info *find_output(rudo_wayland_app *wl, struct wl_output *out) { size_t i; for (i = 0; i < wl->outputs_len; ++i) if (wl->outputs[i].output == out) return &wl->outputs[i]; return NULL; }
static shm_buf *acquire_buf(rudo_wayland_app *wl, uint32_t w, uint32_t h);

static int make_shm_fd(size_t size) { char name[64]; int fd; snprintf(name, sizeof(name), "%s-%ld", RUDO_APP_NAME, (long)getpid()); fd = memfd_create(name, MFD_CLOEXEC); if (fd < 0) return -1; if (ftruncate(fd, (off_t)size) != 0) { close(fd); return -1; } return fd; }
static void destroy_buf(shm_buf *b) { if (b->buffer) wl_buffer_destroy(b->buffer); if (b->map && b->size) munmap(b->map, b->size); memset(b, 0, sizeof(*b)); }
static void buffer_release(void *data, struct wl_buffer *buffer) { shm_buf *b = data; RUDO_UNUSED(buffer); if (b) b->busy = false; }
static const struct wl_buffer_listener buffer_listener = { buffer_release };
static bool create_buf(rudo_wayland_app *wl, shm_buf *b, uint32_t w, uint32_t h) { int fd; size_t size; struct wl_shm_pool *pool; destroy_buf(b); b->stride = w * 4u; size = (size_t)b->stride * h; fd = make_shm_fd(size); if (fd < 0) return false; b->map = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0); if (b->map == MAP_FAILED) { close(fd); b->map = NULL; return false; } pool = wl_shm_create_pool(wl->shm, fd, (int)size); b->buffer = wl_shm_pool_create_buffer(pool, 0, (int)w, (int)h, (int)b->stride, WL_SHM_FORMAT_ARGB8888); wl_shm_pool_destroy(pool); close(fd); if (!b->buffer) { destroy_buf(b); return false; } wl_buffer_add_listener(b->buffer, &buffer_listener, b); b->width = w; b->height = h; b->size = size; return true; }
static shm_buf *acquire_buf(rudo_wayland_app *wl, uint32_t w, uint32_t h) { size_t i; for (i = 0; i < RUDO_WL_BUF_COUNT; ++i) { shm_buf *b = &wl->bufs[i]; if (b->busy) continue; if (b->width != w || b->height != h) { if (!create_buf(wl, b, w, h)) return NULL; } return b; } return NULL; }

static void redraw(rudo_wayland_app *wl) {
    uint32_t pw, ph; shm_buf *b; rudo_framebuffer fb; const rudo_grid *grid; size_t count; rudo_render_cell *cells; rudo_render_grid rg; rudo_render_options ro = { true, true };
    if (!wl->configured || !wl->surface || !wl->shm) return; pw = (uint32_t)RUDO_CLAMP(lroundf((float)wl->width * wl->scale), 1.f, (float)RUDO_WL_MAX_DIM); ph = (uint32_t)RUDO_CLAMP(lroundf((float)wl->height * wl->scale), 1.f, (float)RUDO_WL_MAX_DIM); b = acquire_buf(wl, pw, ph); if (!b) return;
    grid = rudo_core_app_grid(wl->app); count = grid->cols * grid->rows; cells = count ? rudo_malloc(count * sizeof(*cells)) : NULL; app_sync_renderer(wl); rudo_core_app_build_render_grid(wl->app, &rg, cells, count); fb.width = pw; fb.height = ph; fb.stride = b->stride; fb.pixels = b->map; if (rudo_core_app_take_theme_changed(wl->app)) { const rudo_theme *t = rudo_core_app_theme(wl->app); rudo_theme_colors colors = {{t->foreground[0],t->foreground[1],t->foreground[2]},{t->background[0],t->background[1],t->background[2]},{t->cursor[0],t->cursor[1],t->cursor[2]},{t->selection[0],t->selection[1],t->selection[2]}}; rudo_software_renderer_set_theme(wl->renderer, &colors); }
    rudo_software_renderer_render(wl->renderer, &fb, &rg, rudo_core_app_cursor_renderer(wl->app), ro); free(cells); wl_surface_attach(wl->surface, b->buffer, 0, 0); if (wl->viewport) wp_viewport_set_destination(wl->viewport, (int32_t)wl->width, (int32_t)wl->height); if (wl->scale > 1.f && wl->viewport) wl_surface_set_buffer_scale(wl->surface, 1); else wl_surface_set_buffer_scale(wl->surface, (int32_t)RUDO_MAX(1, (int)lroundf(wl->scale))); wl_surface_damage_buffer(wl->surface, 0, 0, INT32_MAX, INT32_MAX); if (wl->frame_cb) wl_callback_destroy(wl->frame_cb); wl->frame_cb = wl_surface_frame(wl->surface); if (wl->frame_cb) wl_callback_add_listener(wl->frame_cb, &frame_listener, wl); b->busy = true; wl_surface_commit(wl->surface); wl_display_flush(wl->display); rudo_core_app_clear_damage(wl->app);
}

static void pointer_enter(void *data, struct wl_pointer *p, uint32_t serial, struct wl_surface *surface, wl_fixed_t sx, wl_fixed_t sy) { rudo_wayland_app *wl = data; RUDO_UNUSED(p); RUDO_UNUSED(surface); wl->pointer_focus = true; wl->serial = serial; wl->pointer_x = wl_fixed_to_double(sx); wl->pointer_y = wl_fixed_to_double(sy); rudo_core_app_handle_mouse_move(wl->app, wl->pointer_x * wl->scale, wl->pointer_y * wl->scale); }
static void pointer_leave(void *data, struct wl_pointer *p, uint32_t serial, struct wl_surface *surface) { rudo_wayland_app *wl = data; RUDO_UNUSED(p); RUDO_UNUSED(surface); wl->pointer_focus = false; wl->serial = serial; }
static void pointer_motion(void *data, struct wl_pointer *p, uint32_t time, wl_fixed_t sx, wl_fixed_t sy) { rudo_wayland_app *wl = data; RUDO_UNUSED(p); RUDO_UNUSED(time); wl->pointer_x = wl_fixed_to_double(sx); wl->pointer_y = wl_fixed_to_double(sy); rudo_core_app_handle_mouse_move(wl->app, wl->pointer_x * wl->scale, wl->pointer_y * wl->scale); wl->frame_ready = true; }
static rudo_mouse_button map_btn(uint32_t b) { rudo_mouse_button mb = { RUDO_MOUSE_BUTTON_OTHER, 0 }; if (b == BTN_LEFT) mb.kind = RUDO_MOUSE_BUTTON_LEFT; else if (b == BTN_MIDDLE) mb.kind = RUDO_MOUSE_BUTTON_MIDDLE; else if (b == BTN_RIGHT) mb.kind = RUDO_MOUSE_BUTTON_RIGHT; else { mb.kind = RUDO_MOUSE_BUTTON_OTHER; mb.other = (uint16_t)b; } return mb; }
static void pointer_button(void *data, struct wl_pointer *p, uint32_t serial, uint32_t time, uint32_t button, uint32_t state) { rudo_wayland_app *wl = data; RUDO_UNUSED(p); RUDO_UNUSED(time); wl->serial = serial; rudo_core_app_handle_mouse_button(wl->app, state == WL_POINTER_BUTTON_STATE_PRESSED, map_btn(button)); wl->frame_ready = true; }
static void pointer_axis(void *data, struct wl_pointer *p, uint32_t time, uint32_t axis, wl_fixed_t value) { rudo_wayland_app *wl = data; double lines; RUDO_UNUSED(p); RUDO_UNUSED(time); lines = wl_fixed_to_double(value) / 10.0; if (axis == WL_POINTER_AXIS_VERTICAL_SCROLL) rudo_core_app_handle_scroll_lines(wl->app, lines); wl->frame_ready = true; }
static void pointer_frame(void *data, struct wl_pointer *p) { RUDO_UNUSED(data); RUDO_UNUSED(p); }
static void pointer_axis_source(void *data, struct wl_pointer *p, uint32_t axis_source) { RUDO_UNUSED(data); RUDO_UNUSED(p); RUDO_UNUSED(axis_source); }
static void pointer_axis_stop(void *data, struct wl_pointer *p, uint32_t time, uint32_t axis) { RUDO_UNUSED(data); RUDO_UNUSED(p); RUDO_UNUSED(time); RUDO_UNUSED(axis); }
static void pointer_axis_discrete(void *data, struct wl_pointer *p, uint32_t axis, int32_t discrete) { RUDO_UNUSED(data); RUDO_UNUSED(p); RUDO_UNUSED(axis); RUDO_UNUSED(discrete); }
static void pointer_axis_value120(void *data, struct wl_pointer *p, uint32_t axis, int32_t value120) { RUDO_UNUSED(data); RUDO_UNUSED(p); RUDO_UNUSED(axis); RUDO_UNUSED(value120); }
static void pointer_axis_relative_direction(void *data, struct wl_pointer *p, uint32_t axis, uint32_t direction) { RUDO_UNUSED(data); RUDO_UNUSED(p); RUDO_UNUSED(axis); RUDO_UNUSED(direction); }
static const struct wl_pointer_listener pointer_listener = {
    pointer_enter,
    pointer_leave,
    pointer_motion,
    pointer_button,
    pointer_axis,
    pointer_frame,
    pointer_axis_source,
    pointer_axis_stop,
    pointer_axis_discrete,
    pointer_axis_value120,
    pointer_axis_relative_direction,
};

static void kb_keymap(void *data, struct wl_keyboard *k, uint32_t format, int fd, uint32_t size) { rudo_wayland_app *wl = data; RUDO_UNUSED(k); if (format == WL_KEYBOARD_KEYMAP_FORMAT_XKB_V1) rudo_wayland_keyboard_keymap(&wl->kb, fd, size); close(fd); }
static void kb_enter(void *data, struct wl_keyboard *k, uint32_t serial, struct wl_surface *surface, struct wl_array *keys) { rudo_wayland_app *wl = data; RUDO_UNUSED(k); RUDO_UNUSED(surface); RUDO_UNUSED(keys); wl->serial = serial; rudo_core_app_handle_focus_change(wl->app, true); wl->frame_ready = true; }
static void kb_leave(void *data, struct wl_keyboard *k, uint32_t serial, struct wl_surface *surface) { rudo_wayland_app *wl = data; RUDO_UNUSED(k); RUDO_UNUSED(surface); wl->serial = serial; rudo_core_app_handle_focus_change(wl->app, false); wl->kb.repeating = false; wl->frame_ready = true; }
static void kb_key(void *data, struct wl_keyboard *k, uint32_t serial, uint32_t time, uint32_t key, uint32_t state) { rudo_wayland_app *wl = data; rudo_key_event ev; RUDO_UNUSED(k); RUDO_UNUSED(time); wl->serial = serial; if (rudo_wayland_keyboard_translate(&wl->kb, key, state == WL_KEYBOARD_KEY_STATE_PRESSED, &ev)) { rudo_core_app_set_modifiers(wl->app, rudo_wayland_keyboard_modifiers(&wl->kb)); if (state == WL_KEYBOARD_KEY_STATE_PRESSED) rudo_wayland_keyboard_repeat_start(&wl->kb, key); else rudo_wayland_keyboard_repeat_stop(&wl->kb, key); rudo_core_app_handle_key_event(wl->app, &ev); rudo_key_event_destroy(&ev); wl->frame_ready = true; } }
static void kb_modifiers(void *data, struct wl_keyboard *k, uint32_t serial, uint32_t dep, uint32_t lat, uint32_t lock, uint32_t group) { rudo_wayland_app *wl = data; RUDO_UNUSED(k); wl->serial = serial; rudo_wayland_keyboard_update_modifiers(&wl->kb, dep, lat, lock, group); rudo_core_app_set_modifiers(wl->app, rudo_wayland_keyboard_modifiers(&wl->kb)); }
static void kb_repeat_info(void *data, struct wl_keyboard *k, int32_t rate, int32_t delay) { RUDO_UNUSED(k); rudo_wayland_app *wl = data; rudo_wayland_keyboard_set_repeat_info(&wl->kb, rate, delay); }
static const struct wl_keyboard_listener keyboard_listener = { kb_keymap, kb_enter, kb_leave, kb_key, kb_modifiers, kb_repeat_info };

static void seat_caps(void *data, struct wl_seat *seat, uint32_t caps) { rudo_wayland_app *wl = data; if ((caps & WL_SEAT_CAPABILITY_POINTER) && !wl->pointer) { wl->pointer = wl_seat_get_pointer(seat); wl_pointer_add_listener(wl->pointer, &pointer_listener, wl); } else if (!(caps & WL_SEAT_CAPABILITY_POINTER) && wl->pointer) { wl_pointer_destroy(wl->pointer); wl->pointer = NULL; } if ((caps & WL_SEAT_CAPABILITY_KEYBOARD) && !wl->keyboard) { wl->keyboard = wl_seat_get_keyboard(seat); wl_keyboard_add_listener(wl->keyboard, &keyboard_listener, wl); } else if (!(caps & WL_SEAT_CAPABILITY_KEYBOARD) && wl->keyboard) { wl_keyboard_destroy(wl->keyboard); wl->keyboard = NULL; } }
static void seat_name(void *data, struct wl_seat *seat, const char *name) { RUDO_UNUSED(data); RUDO_UNUSED(seat); RUDO_UNUSED(name); }
static const struct wl_seat_listener seat_listener = { seat_caps, seat_name };

static void wm_ping(void *data, struct xdg_wm_base *wm, uint32_t serial) { RUDO_UNUSED(data); xdg_wm_base_pong(wm, serial); }
static const struct xdg_wm_base_listener wm_listener = { wm_ping };
static void xdg_surface_configure(void *data, struct xdg_surface *surf, uint32_t serial) { rudo_wayland_app *wl = data; xdg_surface_ack_configure(surf, serial); if (wl->pending_width) wl->width = wl->pending_width; if (wl->pending_height) wl->height = wl->pending_height; wl->configured = true; wl->frame_ready = true; }
static const struct xdg_surface_listener xdg_surface_listener = { xdg_surface_configure };
static void toplevel_configure(void *data, struct xdg_toplevel *tl, int32_t w, int32_t h, struct wl_array *states) { rudo_wayland_app *wl = data; size_t cols, rows; RUDO_UNUSED(tl); RUDO_UNUSED(states); if (w > 0) wl->pending_width = (uint32_t)w; if (h > 0) wl->pending_height = (uint32_t)h; rudo_software_renderer_grid_layout(wl->renderer, (uint32_t)RUDO_MAX(wl->pending_width,1u), (uint32_t)RUDO_MAX(wl->pending_height,1u), &cols, &rows); app_sync_renderer(wl); rudo_core_app_handle_resize(wl->app, cols, rows); }
static void toplevel_close(void *data, struct xdg_toplevel *tl) { rudo_wayland_app *wl = data; RUDO_UNUSED(tl); wl->running = false; }
static const struct xdg_toplevel_listener toplevel_listener = { toplevel_configure, toplevel_close, NULL, NULL };
static void frac_preferred_scale(void *data, struct wp_fractional_scale_v1 *f, uint32_t scale) { rudo_wayland_app *wl = data; RUDO_UNUSED(f); wl->have_frac = true; wl->frac_scale = (float)scale / 120.f; set_scale(wl); }
static const struct wp_fractional_scale_v1_listener frac_listener = { frac_preferred_scale };
static void frame_done(void *data, struct wl_callback *cb, uint32_t time) { rudo_wayland_app *wl = data; RUDO_UNUSED(time); wl_callback_destroy(cb); if (wl->frame_cb == cb) wl->frame_cb = NULL; wl->frame_ready = true; }
static const struct wl_callback_listener frame_listener = { frame_done };
static void output_geometry(void *data, struct wl_output *o, int32_t x, int32_t y, int32_t pw, int32_t ph, int32_t sub, const char *make, const char *model, int32_t transform) { RUDO_UNUSED(data); RUDO_UNUSED(o); RUDO_UNUSED(x); RUDO_UNUSED(y); RUDO_UNUSED(pw); RUDO_UNUSED(ph); RUDO_UNUSED(sub); RUDO_UNUSED(make); RUDO_UNUSED(model); RUDO_UNUSED(transform); }
static void output_mode(void *data, struct wl_output *o, uint32_t flags, int32_t w, int32_t h, int32_t refresh) { RUDO_UNUSED(data); RUDO_UNUSED(o); RUDO_UNUSED(flags); RUDO_UNUSED(w); RUDO_UNUSED(h); RUDO_UNUSED(refresh); }
static void output_done(void *data, struct wl_output *o) { RUDO_UNUSED(o); set_scale(data); }
static void output_scale(void *data, struct wl_output *o, int32_t factor) { output_info *out = find_output(data, o); if (out) out->scale = factor; }
static const struct wl_output_listener output_listener = { output_geometry, output_mode, output_done, output_scale };
static void surface_enter(void *data, struct wl_surface *s, struct wl_output *o) { rudo_wayland_app *wl = data; output_info *info = find_output(wl, o); size_t i; RUDO_UNUSED(s); if (!info) return; for (i = 0; i < wl->surface_outputs_len; ++i) if (wl->surface_outputs[i] == info->name) return; push_u32(&wl->surface_outputs, &wl->surface_outputs_len, &wl->surface_outputs_cap, info->name); set_scale(wl); }
static void surface_leave(void *data, struct wl_surface *s, struct wl_output *o) { rudo_wayland_app *wl = data; output_info *info = find_output(wl, o); size_t i; RUDO_UNUSED(s); if (!info) return; for (i = 0; i < wl->surface_outputs_len; ++i) if (wl->surface_outputs[i] == info->name) { memmove(&wl->surface_outputs[i], &wl->surface_outputs[i+1], (wl->surface_outputs_len - i - 1u) * sizeof(*wl->surface_outputs)); wl->surface_outputs_len--; break; } set_scale(wl); }
static const struct wl_surface_listener surface_listener = { surface_enter, surface_leave, NULL, NULL };

static void registry_global(void *data, struct wl_registry *reg, uint32_t name, const char *iface, uint32_t version) {
    rudo_wayland_app *wl = data;
    if (strcmp(iface, wl_compositor_interface.name) == 0) wl->compositor = wl_registry_bind(reg, name, &wl_compositor_interface, 4);
    else if (strcmp(iface, wl_shm_interface.name) == 0) wl->shm = wl_registry_bind(reg, name, &wl_shm_interface, 1);
    else if (strcmp(iface, wl_seat_interface.name) == 0) { wl->seat = wl_registry_bind(reg, name, &wl_seat_interface, version > 5 ? 5 : version); wl_seat_add_listener(wl->seat, &seat_listener, wl); }
    else if (strcmp(iface, xdg_wm_base_interface.name) == 0) { wl->wm_base = wl_registry_bind(reg, name, &xdg_wm_base_interface, 1); xdg_wm_base_add_listener(wl->wm_base, &wm_listener, wl); }
    else if (strcmp(iface, zxdg_decoration_manager_v1_interface.name) == 0) wl->deco_mgr = wl_registry_bind(reg, name, &zxdg_decoration_manager_v1_interface, 1);
    else if (strcmp(iface, wp_viewporter_interface.name) == 0) wl->viewporter = wl_registry_bind(reg, name, &wp_viewporter_interface, 1);
    else if (strcmp(iface, wp_fractional_scale_manager_v1_interface.name) == 0) wl->frac_mgr = wl_registry_bind(reg, name, &wp_fractional_scale_manager_v1_interface, 1);
    else if (strcmp(iface, wl_output_interface.name) == 0) { output_info out; out.name = name; out.scale = 1; out.output = wl_registry_bind(reg, name, &wl_output_interface, version > 2 ? 2 : version); wl_output_add_listener(out.output, &output_listener, wl); push_output(&wl->outputs, &wl->outputs_len, &wl->outputs_cap, out); }
}
static void registry_remove(void *data, struct wl_registry *reg, uint32_t name) { rudo_wayland_app *wl = data; size_t i; RUDO_UNUSED(reg); for (i = 0; i < wl->outputs_len; ++i) if (wl->outputs[i].name == name) { if (wl->outputs[i].output) wl_output_destroy(wl->outputs[i].output); memmove(&wl->outputs[i], &wl->outputs[i+1], (wl->outputs_len - i - 1u) * sizeof(*wl->outputs)); wl->outputs_len--; break; } set_scale(wl); }
static const struct wl_registry_listener registry_listener = { registry_global, registry_remove };

rudo_wayland_app *rudo_wayland_app_new(rudo_core_app *app, rudo_software_renderer *renderer) {
    rudo_wayland_app *wl; const rudo_config *cfg; rudo_theme_colors colors; size_t cols, rows; uint32_t w, h; uint32_t init_w, init_h;
    if (!app || !renderer) return NULL;
    wl = rudo_calloc(1, sizeof(*wl));
    wl->app = app; wl->renderer = renderer; wl->running = true; wl->scale = 1.f; wl->frac_scale = 1.f;
    cfg = rudo_core_app_config(app);
    colors.foreground[0] = rudo_core_app_theme(app)->foreground[0]; colors.foreground[1] = rudo_core_app_theme(app)->foreground[1]; colors.foreground[2] = rudo_core_app_theme(app)->foreground[2];
    colors.background[0] = rudo_core_app_theme(app)->background[0]; colors.background[1] = rudo_core_app_theme(app)->background[1]; colors.background[2] = rudo_core_app_theme(app)->background[2];
    colors.cursor[0] = rudo_core_app_theme(app)->cursor[0]; colors.cursor[1] = rudo_core_app_theme(app)->cursor[1]; colors.cursor[2] = rudo_core_app_theme(app)->cursor[2];
    colors.selection[0] = rudo_core_app_theme(app)->selection[0]; colors.selection[1] = rudo_core_app_theme(app)->selection[1]; colors.selection[2] = rudo_core_app_theme(app)->selection[2];
    rudo_software_renderer_set_theme(renderer, &colors);
    rudo_software_renderer_set_background_alpha(renderer, bg_alpha(cfg->window.opacity));
    wl->display = wl_display_connect(NULL);
    if (!wl->display) { free(wl); return NULL; }
    wl->registry = wl_display_get_registry(wl->display);
    wl_registry_add_listener(wl->registry, &registry_listener, wl);
    wl_display_roundtrip(wl->display);
    if (!wl->compositor || !wl->wm_base) { rudo_wayland_app_free(wl); return NULL; }

    wl->surface = wl_compositor_create_surface(wl->compositor);
    wl_surface_add_listener(wl->surface, &surface_listener, wl);
    wl->xdg_surface = xdg_wm_base_get_xdg_surface(wl->wm_base, wl->surface);
    xdg_surface_add_listener(wl->xdg_surface, &xdg_surface_listener, wl);
    wl->toplevel = xdg_surface_get_toplevel(wl->xdg_surface);
    xdg_toplevel_add_listener(wl->toplevel, &toplevel_listener, wl);
    xdg_toplevel_set_title(wl->toplevel, rudo_core_app_title(app));
    xdg_toplevel_set_app_id(wl->toplevel, rudo_core_app_app_id(app));
    if (wl->deco_mgr) {
        wl->deco = zxdg_decoration_manager_v1_get_toplevel_decoration(wl->deco_mgr, wl->toplevel);
        zxdg_toplevel_decoration_v1_set_mode(wl->deco, ZXDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE);
    }
    if (wl->viewporter) wl->viewport = wp_viewporter_get_viewport(wl->viewporter, wl->surface);
    if (wl->frac_mgr) {
        wl->frac = wp_fractional_scale_manager_v1_get_fractional_scale(wl->frac_mgr, wl->surface);
        wp_fractional_scale_v1_add_listener(wl->frac, &frac_listener, wl);
    }

    init_w = cfg->window.initial_width;
    init_h = cfg->window.initial_height;
    if (init_w == RUDO_DEFAULT_WINDOW_INITIAL_WIDTH && init_h == RUDO_DEFAULT_WINDOW_INITIAL_HEIGHT) {
        rudo_software_renderer_window_size_for_grid(renderer, RUDO_MAX(cfg->terminal.cols, 2u), RUDO_MAX(cfg->terminal.rows, 2u), &w, &h);
        wl->width = w;
        wl->height = h;
    } else {
        wl->width = init_w ? init_w : RUDO_DEFAULT_WINDOW_INITIAL_WIDTH;
        wl->height = init_h ? init_h : RUDO_DEFAULT_WINDOW_INITIAL_HEIGHT;
    }
    rudo_software_renderer_grid_layout(renderer, wl->width, wl->height, &cols, &rows);
    app_sync_renderer(wl);
    rudo_core_app_init_terminal(app, cols, rows);
    wl->pending_width = wl->width;
    wl->pending_height = wl->height;
    wl_surface_commit(wl->surface);
    wl_display_roundtrip(wl->display);
    wl_display_roundtrip(wl->display);
    return wl;
}

void rudo_wayland_app_free(rudo_wayland_app *wl) {
    size_t i; if (!wl) return; for (i = 0; i < RUDO_WL_BUF_COUNT; ++i) destroy_buf(&wl->bufs[i]); rudo_wayland_keyboard_destroy(&wl->kb); if (wl->frame_cb) wl_callback_destroy(wl->frame_cb); if (wl->frac) wp_fractional_scale_v1_destroy(wl->frac); if (wl->viewport) wp_viewport_destroy(wl->viewport); if (wl->deco) zxdg_toplevel_decoration_v1_destroy(wl->deco); if (wl->pointer) wl_pointer_destroy(wl->pointer); if (wl->keyboard) wl_keyboard_destroy(wl->keyboard); if (wl->seat) wl_seat_destroy(wl->seat); if (wl->toplevel) xdg_toplevel_destroy(wl->toplevel); if (wl->xdg_surface) xdg_surface_destroy(wl->xdg_surface); if (wl->surface) wl_surface_destroy(wl->surface); if (wl->frac_mgr) wp_fractional_scale_manager_v1_destroy(wl->frac_mgr); if (wl->viewporter) wp_viewporter_destroy(wl->viewporter); if (wl->deco_mgr) zxdg_decoration_manager_v1_destroy(wl->deco_mgr); if (wl->wm_base) xdg_wm_base_destroy(wl->wm_base); for (i = 0; i < wl->outputs_len; ++i) if (wl->outputs[i].output) wl_output_destroy(wl->outputs[i].output); if (wl->shm) wl_shm_destroy(wl->shm); if (wl->compositor) wl_compositor_destroy(wl->compositor); if (wl->registry) wl_registry_destroy(wl->registry); if (wl->display) wl_display_disconnect(wl->display); free(wl->outputs); free(wl->surface_outputs); free(wl);
}

int rudo_wayland_app_run(rudo_wayland_app *wl) {
    int status = 0;
    while (wl && wl->running) {
        struct pollfd pfd[2];
        int timeout = -1;
        int nfds;
        rudo_tick_result tick;
        rudo_key_event rep;

        while (rudo_wayland_keyboard_repeat_fire(&wl->kb, &rep)) {
            rudo_core_app_handle_key_event(wl->app, &rep);
            rudo_key_event_destroy(&rep);
            wl->frame_ready = true;
        }

        tick = rudo_core_app_tick(wl->app);
        if (rudo_core_app_take_title_changed(wl->app) && wl->toplevel) {
            xdg_toplevel_set_title(wl->toplevel, rudo_core_app_title(wl->app));
            wl_surface_commit(wl->surface);
        }
        if (tick.redraw_requested || wl->frame_ready) {
            redraw(wl);
            wl->frame_ready = false;
        }
        if (rudo_core_app_poll_pty_exit(wl->app) || rudo_core_app_pty_exited(wl->app)) break;

        pfd[0].fd = wl_display_get_fd(wl->display);
        pfd[0].events = POLLIN;
        pfd[0].revents = 0;
        pfd[1].fd = rudo_core_app_pty_raw_fd(wl->app);
        pfd[1].events = POLLIN | POLLHUP | POLLERR;
        pfd[1].revents = 0;
        nfds = pfd[1].fd >= 0 ? 2 : 1;

        timeout = rudo_wayland_keyboard_repeat_timeout_ms(&wl->kb);
        if (timeout < 0 && tick.animating) timeout = 16;
        if (timeout < 0) timeout = 50;

        wl_display_dispatch_pending(wl->display);
        if (!wl->pending_wl_read) {
            if (wl_display_prepare_read(wl->display) == 0) {
                wl->pending_wl_read = true;
                wl_display_flush(wl->display);
            } else {
                wl_display_dispatch_pending(wl->display);
                continue;
            }
        }

        if (poll(pfd, nfds, timeout) < 0) {
            if (wl->pending_wl_read) {
                wl_display_cancel_read(wl->display);
                wl->pending_wl_read = false;
            }
            if (errno == EINTR) continue;
            break;
        }

        if (wl->pending_wl_read) {
            if (pfd[0].revents & (POLLIN | POLLERR | POLLHUP)) {
                if (wl_display_read_events(wl->display) < 0) {
                    wl->pending_wl_read = false;
                    break;
                }
            } else {
                wl_display_cancel_read(wl->display);
            }
            wl->pending_wl_read = false;
        }
        if (wl_display_dispatch_pending(wl->display) < 0) break;
        if (pfd[0].revents & (POLLERR | POLLHUP)) break;
        if (pfd[1].fd >= 0 && (pfd[1].revents & (POLLIN | POLLHUP | POLLERR))) wl->frame_ready = true;
    }
    if (wl && rudo_core_app_take_pty_exit_status(wl->app, &status)) return rudo_exit_code_from_wait_status(status);
    return 0;
}
