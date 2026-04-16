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
#include <time.h>
#include <unistd.h>
#include <wayland-client.h>
#include <xkbcommon/xkbcommon.h>

#include "xdg-shell-client-protocol.h"
#include "xdg-decoration-unstable-v1-client-protocol.h"
#include "viewporter-client-protocol.h"
#include "fractional-scale-v1-client-protocol.h"

#define RUDO_WL_BUF_COUNT 3
#define RUDO_WL_MAX_DIM 16384u
#define RUDO_WL_DAMAGE_HISTORY 8u
#define RUDO_WL_MAX_ROW_RANGES 4096u
#define RUDO_WL_PTY_COALESCE_NS 5000000L

typedef struct {
    size_t start_row;
    size_t end_row_inclusive;
} row_range;

typedef struct {
    struct wl_buffer *buffer;
    void *map;
    size_t size;
    uint32_t width, height, stride;
    bool busy;
    bool retired;
    uint32_t age;
    uint64_t content_serial;
} shm_buf;

typedef struct {
    uint64_t serial;
    row_range ranges[RUDO_WL_MAX_ROW_RANGES];
    size_t count;
    bool full;
} damage_snapshot;

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
    struct wl_region *opaque_region;
    rudo_core_app *app;
    rudo_software_renderer *renderer;
    shm_buf bufs[RUDO_WL_BUF_COUNT];
    shm_buf *last_presented;
    output_info *outputs;
    size_t outputs_len, outputs_cap;
    uint32_t *surface_outputs;
    size_t surface_outputs_len, surface_outputs_cap;
    uint32_t width, height;
    uint32_t pending_width, pending_height;
    uint32_t logical_width, logical_height;
    float scale, frac_scale;
    bool have_frac;
    bool configured, running, redraw_pending, pointer_focus, pending_wl_read;
    bool frame_callback_pending, want_frame_callback, frame_callback_dirty, layout_dirty, clear_buffer_age_history;
    bool cursor_state_valid, last_cursor_visible;
    double pointer_x, pointer_y;
    uint32_t serial;
    uint64_t present_serial;
    damage_snapshot damage_history[RUDO_WL_DAMAGE_HISTORY];
    size_t damage_history_len;
    size_t damage_history_head;
    row_range current_ranges[RUDO_WL_MAX_ROW_RANGES];
    size_t current_range_count;
    float last_cursor_corners[8];
    size_t last_cursor_row_start, last_cursor_row_end;
    rudo_wayland_keyboard kb;
};

static int clamp_dim(uint32_t v) { return (int)RUDO_CLAMP(v ? v : 1u, 1u, RUDO_WL_MAX_DIM); }
static uint8_t bg_alpha(float o) { if (!isfinite(o)) o = 1.f; o = RUDO_CLAMP(o, 0.f, 1.f); return (uint8_t)lroundf(o * 255.f); }
static float effective_scale(rudo_wayland_app *wl) { size_t i; int32_t best = 1; if (wl->have_frac && wl->frac_scale >= 1.f) return wl->frac_scale; for (i = 0; i < wl->surface_outputs_len; ++i) { size_t j; for (j = 0; j < wl->outputs_len; ++j) if (wl->outputs[j].name == wl->surface_outputs[i] && wl->outputs[j].scale > best) best = wl->outputs[j].scale; } return (float)(best > 0 ? best : 1); }
static void app_sync_renderer(rudo_wayland_app *wl) { float cw, ch, ox, oy; rudo_software_renderer_cell_size(wl->renderer, &cw, &ch); rudo_software_renderer_grid_offset(wl->renderer, &ox, &oy); rudo_core_app_set_cell_size(wl->app, cw, ch); rudo_core_app_set_grid_offset(wl->app, ox, oy); }
static bool push_u32(uint32_t **v, size_t *len, size_t *cap, uint32_t x) { if (*len == *cap) { size_t nc = *cap ? *cap * 2u : 8u; *v = rudo_realloc(*v, nc * sizeof(**v)); *cap = nc; } (*v)[(*len)++] = x; return true; }
static bool push_output(output_info **v, size_t *len, size_t *cap, output_info x) { if (*len == *cap) { size_t nc = *cap ? *cap * 2u : 8u; *v = rudo_realloc(*v, nc * sizeof(**v)); *cap = nc; } (*v)[(*len)++] = x; return true; }
static output_info *find_output(rudo_wayland_app *wl, struct wl_output *out) { size_t i; for (i = 0; i < wl->outputs_len; ++i) if (wl->outputs[i].output == out) return &wl->outputs[i]; return NULL; }
static shm_buf *acquire_buf(rudo_wayland_app *wl, uint32_t w, uint32_t h);

static void timespec_now(struct timespec *ts) { clock_gettime(CLOCK_MONOTONIC, ts); }
static long timespec_to_ms_ceil(const struct timespec *ts) { if (!ts) return -1; return (long)ts->tv_sec * 1000L + (long)((ts->tv_nsec + 999999L) / 1000000L); }
static int min_timeout(int a, int b) { if (a < 0) return b; if (b < 0) return a; return a < b ? a : b; }
static bool row_range_intersects(size_t a0, size_t a1, size_t b0, size_t b1) { return !(a1 < b0 || b1 < a0); }
static void merge_ranges(row_range *ranges, size_t *count) { size_t i, out; if (!ranges || !count || *count < 2) return; out = 0; for (i = 1; i < *count; ++i) { if (ranges[out].end_row_inclusive + 1u >= ranges[i].start_row) { if (ranges[i].end_row_inclusive > ranges[out].end_row_inclusive) ranges[out].end_row_inclusive = ranges[i].end_row_inclusive; } else ranges[++out] = ranges[i]; } *count = out + 1u; }
static void add_dirty_range(row_range *ranges, size_t *count, size_t cap, size_t start, size_t end, size_t max_rows) { size_t i, ins; if (!ranges || !count || !max_rows) return; if (start >= max_rows) return; end = RUDO_MIN(end, max_rows - 1u); if (start > end) return; for (i = 0; i < *count; ++i) { if (ranges[i].start_row <= end + 1u && start <= ranges[i].end_row_inclusive + 1u) { ranges[i].start_row = RUDO_MIN(ranges[i].start_row, start); ranges[i].end_row_inclusive = RUDO_MAX(ranges[i].end_row_inclusive, end); merge_ranges(ranges, count); return; } }
    if (*count >= cap) { ranges[0].start_row = 0; ranges[0].end_row_inclusive = max_rows - 1u; *count = 1; return; }
    ins = *count;
    while (ins > 0 && ranges[ins - 1u].start_row > start) { ranges[ins] = ranges[ins - 1u]; --ins; }
    ranges[ins].start_row = start;
    ranges[ins].end_row_inclusive = end;
    ++*count;
    merge_ranges(ranges, count);
}
static void clear_damage_history(rudo_wayland_app *wl) { wl->damage_history_len = 0; wl->damage_history_head = 0; }
static void reset_buffer_age_history(rudo_wayland_app *wl) { size_t i; wl->last_presented = NULL; clear_damage_history(wl); for (i = 0; i < RUDO_WL_BUF_COUNT; ++i) wl->bufs[i].age = 0; }
static void note_buffer_dimensions_changed(rudo_wayland_app *wl, uint32_t w, uint32_t h) {
    size_t i;
    bool changed = false;
    for (i = 0; i < RUDO_WL_BUF_COUNT; ++i) {
        shm_buf *b = &wl->bufs[i];
        if (!b->buffer) continue;
        if (b->width == w && b->height == h) continue;
        if (b->busy) b->retired = true;
        changed = true;
    }
    if (changed) reset_buffer_age_history(wl);
}
static void push_damage_history(rudo_wayland_app *wl, uint64_t serial, const row_range *ranges, size_t count, bool full) {
    damage_snapshot *snap;
    if (!wl) return;
    snap = &wl->damage_history[wl->damage_history_head % RUDO_WL_DAMAGE_HISTORY];
    snap->serial = serial;
    snap->full = full;
    snap->count = full ? 0u : RUDO_MIN(count, (size_t)RUDO_WL_MAX_ROW_RANGES);
    if (!full && snap->count) memcpy(snap->ranges, ranges, snap->count * sizeof(*ranges));
    wl->damage_history_head = (wl->damage_history_head + 1u) % RUDO_WL_DAMAGE_HISTORY;
    if (wl->damage_history_len < RUDO_WL_DAMAGE_HISTORY) ++wl->damage_history_len;
}
static bool append_copy_range(row_range *out, size_t *out_count, size_t cap, row_range add) {
    if (*out_count >= cap) return false;
    out[*out_count] = add;
    ++*out_count;
    merge_ranges(out, out_count);
    return true;
}
static bool build_copy_ranges_from_target_age(rudo_wayland_app *wl, uint32_t target_age, size_t rows, const row_range *dirty, size_t dirty_count, row_range *out, size_t *out_count, size_t cap) {
    uint32_t needed_history;
    size_t i;
    RUDO_UNUSED(rows);
    if (out_count) *out_count = 0;
    if (!wl || !out || !out_count || target_age == 0) return false;
    needed_history = target_age - 1u;
    if (needed_history > wl->damage_history_len) return false;
    for (i = 0; i < dirty_count; ++i) if (!append_copy_range(out, out_count, cap, dirty[i])) return false;
    for (i = 0; i < needed_history; ++i) {
        size_t idx = (wl->damage_history_head + RUDO_WL_DAMAGE_HISTORY - 1u - i) % RUDO_WL_DAMAGE_HISTORY;
        const damage_snapshot *snap = &wl->damage_history[idx];
        size_t j;
        if (snap->full) return false;
        for (j = 0; j < snap->count; ++j) if (!append_copy_range(out, out_count, cap, snap->ranges[j])) return false;
    }
    return true;
}
static void copy_row_ranges(rudo_wayland_app *wl, shm_buf *dst, const shm_buf *src, const row_range *ranges, size_t count) {
    size_t i;
    RUDO_UNUSED(wl);
    if (!dst || !src || !ranges || !count || dst->stride != src->stride || dst->height != src->height) return;
    for (i = 0; i < count; ++i) {
        size_t start = ranges[i].start_row, end = ranges[i].end_row_inclusive;
        uint32_t y0 = 0, y1 = 0;
        size_t bytes;
        rudo_software_renderer_pixel_bounds_for_row_range(wl->renderer, start, end, &y0, &y1);
        if (y1 <= y0 || y0 >= dst->height) continue;
        y1 = RUDO_MIN(y1, dst->height);
        bytes = (size_t)(y1 - y0) * dst->stride;
        memcpy((uint8_t *)dst->map + (size_t)y0 * dst->stride, (const uint8_t *)src->map + (size_t)y0 * src->stride, bytes);
    }
}
static void update_window_geometry(rudo_wayland_app *wl) {
    uint32_t logical_w, logical_h;
    if (!wl || !wl->renderer || !wl->surface || !wl->compositor) return;
    logical_w = wl->width ? wl->width : 1u;
    logical_h = wl->height ? wl->height : 1u;
    wl->logical_width = logical_w;
    wl->logical_height = logical_h;
    if (wl->xdg_surface) xdg_surface_set_window_geometry(wl->xdg_surface, 0, 0, (int32_t)logical_w, (int32_t)logical_h);
    if (wl->viewport) wp_viewport_set_destination(wl->viewport, (int32_t)logical_w, (int32_t)logical_h);
    if (wl->have_frac && wl->viewport) wl_surface_set_buffer_scale(wl->surface, 1);
    else wl_surface_set_buffer_scale(wl->surface, (int32_t)RUDO_MAX(1, (int)lroundf(wl->scale)));
    if (wl->opaque_region) wl_region_destroy(wl->opaque_region);
    wl->opaque_region = wl_compositor_create_region(wl->compositor);
    if (wl->opaque_region) {
        const rudo_config *cfg = rudo_core_app_config(wl->app);
        if (bg_alpha(cfg->window.opacity) == 255u) {
            wl_region_add(wl->opaque_region, 0, 0, clamp_dim(logical_w), clamp_dim(logical_h));
            wl_surface_set_opaque_region(wl->surface, wl->opaque_region);
        } else wl_surface_set_opaque_region(wl->surface, NULL);
    }
}
static void relayout_terminal(rudo_wayland_app *wl, bool mark_all) {
    size_t cols, rows;
    uint32_t physical_w, physical_h;
    if (!wl || !wl->renderer) return;
    physical_w = (uint32_t)RUDO_CLAMP(lroundf((float)(wl->width ? wl->width : 1u) * wl->scale), 1.f, (float)RUDO_WL_MAX_DIM);
    physical_h = (uint32_t)RUDO_CLAMP(lroundf((float)(wl->height ? wl->height : 1u) * wl->scale), 1.f, (float)RUDO_WL_MAX_DIM);
    rudo_software_renderer_grid_layout(wl->renderer, physical_w, physical_h, &cols, &rows);
    app_sync_renderer(wl);
    rudo_core_app_handle_resize(wl->app, cols, rows);
    if (mark_all) rudo_damage_mark_all(rudo_core_app_damage_mut(wl->app));
    wl->layout_dirty = false;
}
static void set_scale(rudo_wayland_app *wl) {
    float s = effective_scale(wl);
    if (fabsf(s - wl->scale) < 0.001f) return;
    wl->scale = s;
    rudo_software_renderer_set_scale(wl->renderer, s);
    relayout_terminal(wl, true);
    update_window_geometry(wl);
    reset_buffer_age_history(wl);
    wl->redraw_pending = true;
}

static int make_shm_fd(size_t size) { char name[64]; int fd; snprintf(name, sizeof(name), "%s-%ld", RUDO_APP_NAME, (long)getpid()); fd = memfd_create(name, MFD_CLOEXEC); if (fd < 0) return -1; if (ftruncate(fd, (off_t)size) != 0) { close(fd); return -1; } return fd; }
static void destroy_buf(shm_buf *b) { if (b->buffer) wl_buffer_destroy(b->buffer); if (b->map && b->size) munmap(b->map, b->size); memset(b, 0, sizeof(*b)); }
static void buffer_release(void *data, struct wl_buffer *buffer) { shm_buf *b = data; RUDO_UNUSED(buffer); if (!b) return; b->busy = false; if (b->retired) destroy_buf(b); }
static const struct wl_buffer_listener buffer_listener = { buffer_release };
static bool create_buf(rudo_wayland_app *wl, shm_buf *b, uint32_t w, uint32_t h) { int fd; size_t size; struct wl_shm_pool *pool; destroy_buf(b); b->stride = w * 4u; size = (size_t)b->stride * h; fd = make_shm_fd(size); if (fd < 0) return false; b->map = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0); if (b->map == MAP_FAILED) { close(fd); b->map = NULL; return false; } pool = wl_shm_create_pool(wl->shm, fd, (int)size); b->buffer = wl_shm_pool_create_buffer(pool, 0, (int)w, (int)h, (int)b->stride, WL_SHM_FORMAT_ARGB8888); wl_shm_pool_destroy(pool); close(fd); if (!b->buffer) { destroy_buf(b); return false; } wl_buffer_add_listener(b->buffer, &buffer_listener, b); b->width = w; b->height = h; b->size = size; b->retired = false; b->age = 0; b->content_serial = 0; return true; }
static shm_buf *acquire_buf(rudo_wayland_app *wl, uint32_t w, uint32_t h) { size_t i; for (i = 0; i < RUDO_WL_BUF_COUNT; ++i) { shm_buf *b = &wl->bufs[i]; if (b->busy) continue; if (b->retired || b->width != w || b->height != h) { if (!create_buf(wl, b, w, h)) return NULL; } return b; } return NULL; }

static void note_cursor_damage(rudo_wayland_app *wl, const rudo_render_grid *grid, bool full_redraw) {
    const rudo_cursor_renderer *cursor;
    size_t rows;
    float corners[8];
    size_t i;
    bool visible;
    size_t row_start = 0, row_end = 0;
    if (!wl || !grid || !grid->rows) return;
    rows = grid->rows;
    cursor = rudo_core_app_cursor_renderer(wl->app);
    visible = !grid->viewing_scrollback && grid->cursor_visible && cursor && rudo_cursor_renderer_is_visible(cursor);
    if (visible) {
        rudo_cursor_renderer_corner_positions(cursor, corners);
        row_start = rows - 1u;
        row_end = 0;
        for (i = 0; i < 4; ++i) {
            float y = corners[i * 2u + 1u];
            size_t row = (size_t)RUDO_CLAMP((long)floorf(y), 0L, (long)(rows - 1u));
            row_start = RUDO_MIN(row_start, row);
            row_end = RUDO_MAX(row_end, row);
        }
    }
    if (full_redraw) {
        wl->cursor_state_valid = true;
        wl->last_cursor_visible = visible;
        wl->last_cursor_row_start = row_start;
        wl->last_cursor_row_end = row_end;
        if (visible) memcpy(wl->last_cursor_corners, corners, sizeof(corners));
        return;
    }
    if (!wl->cursor_state_valid || wl->last_cursor_visible != visible) {
        if (wl->last_cursor_visible) add_dirty_range(wl->current_ranges, &wl->current_range_count, RUDO_WL_MAX_ROW_RANGES, wl->last_cursor_row_start, wl->last_cursor_row_end, rows);
        if (visible) add_dirty_range(wl->current_ranges, &wl->current_range_count, RUDO_WL_MAX_ROW_RANGES, row_start, row_end, rows);
    } else if (visible) {
        bool changed = false;
        for (i = 0; i < 8; ++i) if (fabsf(wl->last_cursor_corners[i] - corners[i]) > 0.001f) { changed = true; break; }
        if (changed) {
            add_dirty_range(wl->current_ranges, &wl->current_range_count, RUDO_WL_MAX_ROW_RANGES, wl->last_cursor_row_start, wl->last_cursor_row_end, rows);
            add_dirty_range(wl->current_ranges, &wl->current_range_count, RUDO_WL_MAX_ROW_RANGES, row_start, row_end, rows);
        }
    }
    wl->cursor_state_valid = true;
    wl->last_cursor_visible = visible;
    wl->last_cursor_row_start = row_start;
    wl->last_cursor_row_end = row_end;
    if (visible) memcpy(wl->last_cursor_corners, corners, sizeof(corners));
}

static void redraw(rudo_wayland_app *wl) {
    uint32_t pw, ph;
    shm_buf *b;
    rudo_framebuffer fb;
    const rudo_grid *grid;
    size_t count;
    rudo_render_cell *cells;
    rudo_render_grid rg;
    rudo_render_options ro;
    bool theme_changed;
    size_t i;
    if (!wl->configured || !wl->surface || !wl->shm) return;
    if (wl->layout_dirty) relayout_terminal(wl, true);
    update_window_geometry(wl);
    pw = (uint32_t)RUDO_CLAMP(lroundf((float)wl->logical_width * wl->scale), 1.f, (float)RUDO_WL_MAX_DIM);
    ph = (uint32_t)RUDO_CLAMP(lroundf((float)wl->logical_height * wl->scale), 1.f, (float)RUDO_WL_MAX_DIM);
    note_buffer_dimensions_changed(wl, pw, ph);
    b = acquire_buf(wl, pw, ph);
    if (!b) return;
    if (wl->clear_buffer_age_history) { reset_buffer_age_history(wl); wl->clear_buffer_age_history = false; }
    grid = rudo_core_app_grid(wl->app);
    theme_changed = rudo_core_app_take_theme_changed(wl->app);
    wl->current_range_count = 0;
    if (theme_changed) {
        const rudo_theme *t = rudo_core_app_theme(wl->app);
        rudo_theme_colors colors = {{t->foreground[0],t->foreground[1],t->foreground[2]},{t->background[0],t->background[1],t->background[2]},{t->cursor[0],t->cursor[1],t->cursor[2]},{t->selection[0],t->selection[1],t->selection[2]}};
        rudo_software_renderer_set_theme(wl->renderer, &colors);
    }
    app_sync_renderer(wl);
    count = grid->cols * grid->rows;
    cells = count ? rudo_malloc(count * sizeof(*cells)) : NULL;
    rudo_core_app_build_render_grid(wl->app, &rg, cells, count);
    /* Correctness first: keep the Wayland path on full redraws until the
     * age/copy partial-present path is revalidated against interactive TUIs. */
    note_cursor_damage(wl, &rg, true);
    fb.width = pw;
    fb.height = ph;
    fb.stride = b->stride;
    fb.pixels = b->map;
    ro.full_redraw = true;
    ro.draw_cursor = true;
    ro.dirty_rows = NULL;
    ro.dirty_row_count = 0;
    rudo_software_renderer_render(wl->renderer, &fb, &rg, rudo_core_app_cursor_renderer(wl->app), ro);
    wl_surface_attach(wl->surface, b->buffer, 0, 0);
    wl_surface_damage_buffer(wl->surface, 0, 0, INT32_MAX, INT32_MAX);
    if (!wl->frame_callback_pending && wl->want_frame_callback) {
        wl->frame_cb = wl_surface_frame(wl->surface);
        if (wl->frame_cb) {
            wl_callback_add_listener(wl->frame_cb, &frame_listener, wl);
            wl->frame_callback_pending = true;
        }
    }
    wl->frame_callback_dirty = false;
    b->busy = true;
    ++wl->present_serial;
    for (i = 0; i < RUDO_WL_BUF_COUNT; ++i) {
        shm_buf *it = &wl->bufs[i];
        if (!it->buffer || it == b || it->retired || it->width != pw || it->height != ph || it->age == 0) continue;
        ++it->age;
    }
    b->age = 1;
    b->content_serial = wl->present_serial;
    wl->last_presented = b;
    push_damage_history(wl, b->content_serial, wl->current_ranges, wl->current_range_count, true);
    wl_surface_commit(wl->surface);
    wl_display_flush(wl->display);
    rudo_core_app_clear_damage(wl->app);
    wl->redraw_pending = false;
    free(cells);
}

static void maybe_handle_zoom(rudo_wayland_app *wl, const rudo_key_event *ev, bool *handled) {
    const rudo_config *cfg;
    float delta;
    *handled = false;
    if (!wl || !ev) return;
    cfg = rudo_core_app_config(wl->app);
    delta = cfg->font.size_adjustment;
    if (rudo_core_app_matches_local_keybinding(wl->app, RUDO_LOCAL_ACTION_ZOOM_IN, ev)) {
        rudo_software_renderer_increase_font_size(wl->renderer, delta);
        *handled = true;
    } else if (rudo_core_app_matches_local_keybinding(wl->app, RUDO_LOCAL_ACTION_ZOOM_OUT, ev)) {
        rudo_software_renderer_decrease_font_size(wl->renderer, delta);
        *handled = true;
    } else if (rudo_core_app_matches_local_keybinding(wl->app, RUDO_LOCAL_ACTION_ZOOM_RESET, ev)) {
        rudo_software_renderer_reset_font_size(wl->renderer);
        *handled = true;
    }
    if (*handled) {
        relayout_terminal(wl, true);
        reset_buffer_age_history(wl);
        wl->redraw_pending = true;
    }
}

static void pointer_enter(void *data, struct wl_pointer *p, uint32_t serial, struct wl_surface *surface, wl_fixed_t sx, wl_fixed_t sy) { rudo_wayland_app *wl = data; RUDO_UNUSED(p); RUDO_UNUSED(surface); wl->pointer_focus = true; wl->serial = serial; wl->pointer_x = wl_fixed_to_double(sx); wl->pointer_y = wl_fixed_to_double(sy); rudo_core_app_handle_mouse_move(wl->app, wl->pointer_x * wl->scale, wl->pointer_y * wl->scale); }
static void pointer_leave(void *data, struct wl_pointer *p, uint32_t serial, struct wl_surface *surface) { rudo_wayland_app *wl = data; RUDO_UNUSED(p); RUDO_UNUSED(surface); wl->pointer_focus = false; wl->serial = serial; }
static void pointer_motion(void *data, struct wl_pointer *p, uint32_t time, wl_fixed_t sx, wl_fixed_t sy) { rudo_wayland_app *wl = data; RUDO_UNUSED(p); RUDO_UNUSED(time); wl->pointer_x = wl_fixed_to_double(sx); wl->pointer_y = wl_fixed_to_double(sy); rudo_core_app_handle_mouse_move(wl->app, wl->pointer_x * wl->scale, wl->pointer_y * wl->scale); wl->redraw_pending = true; }
static rudo_mouse_button map_btn(uint32_t b) { rudo_mouse_button mb = { RUDO_MOUSE_BUTTON_OTHER, 0 }; if (b == BTN_LEFT) mb.kind = RUDO_MOUSE_BUTTON_LEFT; else if (b == BTN_MIDDLE) mb.kind = RUDO_MOUSE_BUTTON_MIDDLE; else if (b == BTN_RIGHT) mb.kind = RUDO_MOUSE_BUTTON_RIGHT; else { mb.kind = RUDO_MOUSE_BUTTON_OTHER; mb.other = (uint16_t)b; } return mb; }
static void pointer_button(void *data, struct wl_pointer *p, uint32_t serial, uint32_t time, uint32_t button, uint32_t state) { rudo_wayland_app *wl = data; RUDO_UNUSED(p); RUDO_UNUSED(time); wl->serial = serial; rudo_core_app_handle_mouse_button(wl->app, state == WL_POINTER_BUTTON_STATE_PRESSED, map_btn(button)); wl->redraw_pending = true; }
static void pointer_axis(void *data, struct wl_pointer *p, uint32_t time, uint32_t axis, wl_fixed_t value) { rudo_wayland_app *wl = data; double lines; RUDO_UNUSED(p); RUDO_UNUSED(time); lines = wl_fixed_to_double(value) / 10.0; if (axis == WL_POINTER_AXIS_VERTICAL_SCROLL) rudo_core_app_handle_scroll_lines(wl->app, lines); wl->redraw_pending = true; }
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
static void kb_enter(void *data, struct wl_keyboard *k, uint32_t serial, struct wl_surface *surface, struct wl_array *keys) { rudo_wayland_app *wl = data; RUDO_UNUSED(k); RUDO_UNUSED(surface); RUDO_UNUSED(keys); wl->serial = serial; rudo_core_app_handle_focus_change(wl->app, true); wl->redraw_pending = true; }
static void kb_leave(void *data, struct wl_keyboard *k, uint32_t serial, struct wl_surface *surface) { rudo_wayland_app *wl = data; RUDO_UNUSED(k); RUDO_UNUSED(surface); wl->serial = serial; rudo_core_app_handle_focus_change(wl->app, false); wl->kb.repeating = false; wl->redraw_pending = true; }
static void kb_key(void *data, struct wl_keyboard *k, uint32_t serial, uint32_t time, uint32_t key, uint32_t state) { rudo_wayland_app *wl = data; rudo_key_event ev; bool handled = false; RUDO_UNUSED(k); RUDO_UNUSED(time); wl->serial = serial; if (rudo_wayland_keyboard_translate(&wl->kb, key, state == WL_KEYBOARD_KEY_STATE_PRESSED, &ev)) { rudo_core_app_set_modifiers(wl->app, rudo_wayland_keyboard_modifiers(&wl->kb)); if (state == WL_KEYBOARD_KEY_STATE_PRESSED) rudo_wayland_keyboard_repeat_start(&wl->kb, key); else rudo_wayland_keyboard_repeat_stop(&wl->kb, key); if (state == WL_KEYBOARD_KEY_STATE_PRESSED) maybe_handle_zoom(wl, &ev, &handled); if (!handled) rudo_core_app_handle_key_event(wl->app, &ev); rudo_key_event_destroy(&ev); wl->redraw_pending = true; } }
static void kb_modifiers(void *data, struct wl_keyboard *k, uint32_t serial, uint32_t dep, uint32_t lat, uint32_t lock, uint32_t group) { rudo_wayland_app *wl = data; RUDO_UNUSED(k); wl->serial = serial; rudo_wayland_keyboard_update_modifiers(&wl->kb, dep, lat, lock, group); rudo_core_app_set_modifiers(wl->app, rudo_wayland_keyboard_modifiers(&wl->kb)); }
static void kb_repeat_info(void *data, struct wl_keyboard *k, int32_t rate, int32_t delay) { RUDO_UNUSED(k); rudo_wayland_app *wl = data; rudo_wayland_keyboard_set_repeat_info(&wl->kb, rate, delay); }
static const struct wl_keyboard_listener keyboard_listener = { kb_keymap, kb_enter, kb_leave, kb_key, kb_modifiers, kb_repeat_info };

static void seat_caps(void *data, struct wl_seat *seat, uint32_t caps) { rudo_wayland_app *wl = data; if ((caps & WL_SEAT_CAPABILITY_POINTER) && !wl->pointer) { wl->pointer = wl_seat_get_pointer(seat); wl_pointer_add_listener(wl->pointer, &pointer_listener, wl); } else if (!(caps & WL_SEAT_CAPABILITY_POINTER) && wl->pointer) { wl_pointer_destroy(wl->pointer); wl->pointer = NULL; } if ((caps & WL_SEAT_CAPABILITY_KEYBOARD) && !wl->keyboard) { wl->keyboard = wl_seat_get_keyboard(seat); wl_keyboard_add_listener(wl->keyboard, &keyboard_listener, wl); } else if (!(caps & WL_SEAT_CAPABILITY_KEYBOARD) && wl->keyboard) { wl_keyboard_destroy(wl->keyboard); wl->keyboard = NULL; } }
static void seat_name(void *data, struct wl_seat *seat, const char *name) { RUDO_UNUSED(data); RUDO_UNUSED(seat); RUDO_UNUSED(name); }
static const struct wl_seat_listener seat_listener = { seat_caps, seat_name };

static void wm_ping(void *data, struct xdg_wm_base *wm, uint32_t serial) { RUDO_UNUSED(data); xdg_wm_base_pong(wm, serial); }
static const struct xdg_wm_base_listener wm_listener = { wm_ping };
static void xdg_surface_configure(void *data, struct xdg_surface *surf, uint32_t serial) { rudo_wayland_app *wl = data; xdg_surface_ack_configure(surf, serial); if (wl->pending_width) wl->width = wl->pending_width; if (wl->pending_height) wl->height = wl->pending_height; wl->configured = true; wl->layout_dirty = true; update_window_geometry(wl); wl->redraw_pending = true; }
static const struct xdg_surface_listener xdg_surface_listener = { xdg_surface_configure };
static void toplevel_configure(void *data, struct xdg_toplevel *tl, int32_t w, int32_t h, struct wl_array *states) { rudo_wayland_app *wl = data; RUDO_UNUSED(tl); RUDO_UNUSED(states); if (w > 0) wl->pending_width = (uint32_t)w; if (h > 0) wl->pending_height = (uint32_t)h; wl->layout_dirty = true; }
static void toplevel_close(void *data, struct xdg_toplevel *tl) { rudo_wayland_app *wl = data; RUDO_UNUSED(tl); wl->running = false; }
static const struct xdg_toplevel_listener toplevel_listener = { toplevel_configure, toplevel_close, NULL, NULL };
static void frac_preferred_scale(void *data, struct wp_fractional_scale_v1 *f, uint32_t scale) { rudo_wayland_app *wl = data; RUDO_UNUSED(f); wl->have_frac = true; wl->frac_scale = (float)scale / 120.f; set_scale(wl); }
static const struct wp_fractional_scale_v1_listener frac_listener = { frac_preferred_scale };
static void frame_done(void *data, struct wl_callback *cb, uint32_t time) { rudo_wayland_app *wl = data; RUDO_UNUSED(time); wl_callback_destroy(cb); if (wl->frame_cb == cb) wl->frame_cb = NULL; wl->frame_callback_pending = false; if (wl->want_frame_callback || wl->frame_callback_dirty) wl->redraw_pending = true; }
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
    wl->app = app; wl->renderer = renderer; wl->running = true; wl->scale = 1.f; wl->frac_scale = 1.f; wl->clear_buffer_age_history = true;
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
    wl->logical_width = wl->width;
    wl->logical_height = wl->height;
    rudo_software_renderer_grid_layout(renderer, (uint32_t)RUDO_CLAMP(lroundf((float)wl->width * wl->scale), 1.f, (float)RUDO_WL_MAX_DIM), (uint32_t)RUDO_CLAMP(lroundf((float)wl->height * wl->scale), 1.f, (float)RUDO_WL_MAX_DIM), &cols, &rows);
    app_sync_renderer(wl);
    rudo_core_app_init_terminal(app, cols, rows);
    wl->pending_width = wl->width;
    wl->pending_height = wl->height;
    update_window_geometry(wl);
    wl_surface_commit(wl->surface);
    wl_display_roundtrip(wl->display);
    wl_display_roundtrip(wl->display);
    wl->redraw_pending = true;
    return wl;
}

void rudo_wayland_app_free(rudo_wayland_app *wl) {
    size_t i; if (!wl) return; for (i = 0; i < RUDO_WL_BUF_COUNT; ++i) destroy_buf(&wl->bufs[i]); rudo_wayland_keyboard_destroy(&wl->kb); if (wl->frame_cb) wl_callback_destroy(wl->frame_cb); if (wl->opaque_region) wl_region_destroy(wl->opaque_region); if (wl->frac) wp_fractional_scale_v1_destroy(wl->frac); if (wl->viewport) wp_viewport_destroy(wl->viewport); if (wl->deco) zxdg_toplevel_decoration_v1_destroy(wl->deco); if (wl->pointer) wl_pointer_destroy(wl->pointer); if (wl->keyboard) wl_keyboard_destroy(wl->keyboard); if (wl->seat) wl_seat_destroy(wl->seat); if (wl->toplevel) xdg_toplevel_destroy(wl->toplevel); if (wl->xdg_surface) xdg_surface_destroy(wl->xdg_surface); if (wl->surface) wl_surface_destroy(wl->surface); if (wl->frac_mgr) wp_fractional_scale_manager_v1_destroy(wl->frac_mgr); if (wl->viewporter) wp_viewporter_destroy(wl->viewporter); if (wl->deco_mgr) zxdg_decoration_manager_v1_destroy(wl->deco_mgr); if (wl->wm_base) xdg_wm_base_destroy(wl->wm_base); for (i = 0; i < wl->outputs_len; ++i) if (wl->outputs[i].output) wl_output_destroy(wl->outputs[i].output); if (wl->shm) wl_shm_destroy(wl->shm); if (wl->compositor) wl_compositor_destroy(wl->compositor); if (wl->registry) wl_registry_destroy(wl->registry); if (wl->display) wl_display_disconnect(wl->display); free(wl->outputs); free(wl->surface_outputs); free(wl);
}

int rudo_wayland_app_run(rudo_wayland_app *wl) {
    int status = 0;
    struct timespec pty_coalesce = { 0, 0 };
    bool pty_waiting = false;
    while (wl && wl->running) {
        struct pollfd pfd[2];
        int timeout = -1;
        int nfds;
        rudo_tick_result tick;
        rudo_key_event rep;
        struct timespec cursor_wakeup;

        while (rudo_wayland_keyboard_repeat_fire(&wl->kb, &rep)) {
            bool handled = false;
            maybe_handle_zoom(wl, &rep, &handled);
            if (!handled) rudo_core_app_handle_key_event(wl->app, &rep);
            rudo_key_event_destroy(&rep);
            wl->redraw_pending = true;
        }

        tick = rudo_core_app_tick(wl->app);
        if (rudo_core_app_take_title_changed(wl->app) && wl->toplevel) {
            xdg_toplevel_set_title(wl->toplevel, rudo_core_app_title(wl->app));
            wl_surface_commit(wl->surface);
        }
        wl->want_frame_callback = tick.animating;
        if (!wl->want_frame_callback) wl->frame_callback_dirty = false;
        else if (tick.redraw_requested) wl->frame_callback_dirty = true;
        if (tick.redraw_requested) wl->redraw_pending = true;
        if (rudo_core_app_poll_pty_exit(wl->app) || rudo_core_app_pty_exited(wl->app)) break;

        if (wl->redraw_pending) redraw(wl);

        pfd[0].fd = wl_display_get_fd(wl->display);
        pfd[0].events = POLLIN;
        pfd[0].revents = 0;
        pfd[1].fd = rudo_core_app_pty_raw_fd(wl->app);
        pfd[1].events = POLLIN | POLLHUP | POLLERR;
        pfd[1].revents = 0;
        nfds = pfd[1].fd >= 0 ? 2 : 1;

        timeout = rudo_wayland_keyboard_repeat_timeout_ms(&wl->kb);
        if (rudo_core_app_next_wakeup(wl->app, &cursor_wakeup)) timeout = min_timeout(timeout, (int)timespec_to_ms_ceil(&cursor_wakeup));
        if (pty_waiting) {
            struct timespec now;
            long remain_ns;
            timespec_now(&now);
            remain_ns = (pty_coalesce.tv_sec - now.tv_sec) * 1000000000L + (pty_coalesce.tv_nsec - now.tv_nsec);
            timeout = min_timeout(timeout, remain_ns <= 0 ? 0 : (int)((remain_ns + 999999L) / 1000000L));
        }
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
        if (pfd[1].fd >= 0 && (pfd[1].revents & (POLLIN | POLLHUP | POLLERR))) {
            struct timespec now;
            timespec_now(&now);
            pty_coalesce = now;
            pty_coalesce.tv_nsec += RUDO_WL_PTY_COALESCE_NS;
            if (pty_coalesce.tv_nsec >= 1000000000L) { ++pty_coalesce.tv_sec; pty_coalesce.tv_nsec -= 1000000000L; }
            pty_waiting = true;
        }
        if (pty_waiting) {
            struct timespec now;
            long remain_ns;
            timespec_now(&now);
            remain_ns = (pty_coalesce.tv_sec - now.tv_sec) * 1000000000L + (pty_coalesce.tv_nsec - now.tv_nsec);
            if (remain_ns <= 0) {
                wl->redraw_pending = true;
                pty_waiting = false;
            }
        }
    }
    if (wl && rudo_core_app_take_pty_exit_status(wl->app, &status)) return rudo_exit_code_from_wait_status(status);
    return 0;
}
