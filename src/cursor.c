#include "rudo/render.h"
#include <math.h>
#include <string.h>
#include <time.h>

#define NS_PER_SEC 1000000000L
static float timespec_to_seconds(const struct timespec *ts) { if (!ts) return 0.0f; return (float)ts->tv_sec + (float)ts->tv_nsec / 1e9f; }
static void seconds_to_timespec(float seconds, struct timespec *out) { long sec; long nsec; if (!out) return; if (seconds <= 0.0f) { out->tv_sec = 0; out->tv_nsec = 0; return; } sec = (long)seconds; nsec = (long)lroundf((seconds - (float)sec) * (float)NS_PER_SEC); if (nsec >= NS_PER_SEC) { sec += 1; nsec -= NS_PER_SEC; } out->tv_sec = sec; out->tv_nsec = nsec; }

#define CRITICAL_DAMPING_RATIO 1.0f
#define SPRING_DAMPING_FACTOR 4.0f
#define SPRING_SETTLE_THRESHOLD 0.01f
#define POSITION_CHANGE_EPSILON 0.001f
#define DESTINATION_CHANGE_EPSILON 0.001f
#define MIN_CURSOR_HALF_SIZE 0.02f
#define MAX_CURSOR_HALF_SIZE 0.5f
#define SHORT_MOVE_THRESHOLD_COLS 2.001f
#define CURSOR_CELL_CENTER 0.5f
#define INITIAL_PREVIOUS_POSITION -1000.0f
#define CELL_DIMENSION 1.0f
#define BEAM_WIDTH_CELLS 0.12f
#define UNDERLINE_HEIGHT_CELLS 0.16f
#define CURSOR_ANIMATION_FRAME_INTERVAL_SECS (1.0f / 60.0f)

typedef struct { float position, velocity; } spring;
typedef struct { float current_x, current_y, relative_x, relative_y, prev_dest_x, prev_dest_y; spring spring_x, spring_y; float animation_length; } corner;
typedef struct rudo_cursor_vfx_state rudo_cursor_vfx_state;
extern void rudo_cursor_vfx_settings_default(rudo_cursor_vfx_settings *out);

struct rudo_cursor_renderer {
    corner corners[4];
    rudo_cursor_shape shape;
    rudo_cursor_settings settings;
    float prev_col, prev_row;
    bool jumped, blink_on, blink_enabled, animating, smooth_blink, unfocused;
    float blink_timer, unfocused_outline_width;
    rudo_cursor_vfx_state *vfx;
};

static const float standard_corners[8] = { -0.5f,-0.5f, 0.5f,-0.5f, 0.5f,0.5f, -0.5f,0.5f };
static bool spring_update(spring *s, float dt, float animation_length) { float omega, a, b, c; if (animation_length <= dt || fabsf(s->position) < SPRING_SETTLE_THRESHOLD) { s->position = s->velocity = 0.0f; return false; } omega = SPRING_DAMPING_FACTOR / (CRITICAL_DAMPING_RATIO * animation_length); a = s->position; b = s->position * omega + s->velocity; c = expf(-omega * dt); s->position = (a + b * dt) * c; s->velocity = c * (-a * omega - b * dt * omega + b); if (fabsf(s->position) < SPRING_SETTLE_THRESHOLD) { s->position = s->velocity = 0.0f; return false; } return true; }
static void corner_init(corner *c, float rx, float ry) { memset(c, 0, sizeof(*c)); c->relative_x = rx; c->relative_y = ry; c->prev_dest_x = c->prev_dest_y = INITIAL_PREVIOUS_POSITION; }
static void corner_set_shape(corner *c, rudo_cursor_shape shape, int idx) { float sx = standard_corners[idx * 2], sy = standard_corners[idx * 2 + 1]; switch (shape) { case RUDO_CURSOR_SHAPE_BLOCK: c->relative_x = sx; c->relative_y = sy; break; case RUDO_CURSOR_SHAPE_BEAM: { float half_width = RUDO_CLAMP(BEAM_WIDTH_CELLS * CURSOR_CELL_CENTER, MIN_CURSOR_HALF_SIZE, MAX_CURSOR_HALF_SIZE); c->relative_x = sx < 0.0f ? -CURSOR_CELL_CENTER : -CURSOR_CELL_CENTER + half_width * 2.0f; c->relative_y = sy; } break; case RUDO_CURSOR_SHAPE_UNDERLINE: { float half_height = RUDO_CLAMP(UNDERLINE_HEIGHT_CELLS * CURSOR_CELL_CENTER, MIN_CURSOR_HALF_SIZE, MAX_CURSOR_HALF_SIZE); c->relative_x = sx; c->relative_y = sy < 0.0f ? CURSOR_CELL_CENTER - half_height * 2.0f : CURSOR_CELL_CENTER; } break; } }
static bool corner_update(corner *c, float center_x, float center_y, float cell_w, float cell_h, float dt) { float dest_x = center_x + c->relative_x * cell_w, dest_y = center_y + c->relative_y * cell_h; bool animating; if (fabsf(dest_x - c->prev_dest_x) > DESTINATION_CHANGE_EPSILON || fabsf(dest_y - c->prev_dest_y) > DESTINATION_CHANGE_EPSILON) { c->spring_x.position = dest_x - c->current_x; c->spring_y.position = dest_y - c->current_y; c->prev_dest_x = dest_x; c->prev_dest_y = dest_y; } animating = spring_update(&c->spring_x, dt, c->animation_length); animating |= spring_update(&c->spring_y, dt, c->animation_length); c->current_x = dest_x - c->spring_x.position; c->current_y = dest_y - c->spring_y.position; return animating; }
static float corner_alignment(const corner *c, float center_x, float center_y, float cell_w, float cell_h) { float dest_x = center_x + c->relative_x * cell_w, dest_y = center_y + c->relative_y * cell_h, dx = dest_x - c->current_x, dy = dest_y - c->current_y, len = sqrtf(dx*dx + dy*dy), rx = c->relative_x, ry = c->relative_y, rlen = sqrtf(rx*rx + ry*ry); if (len < POSITION_CHANGE_EPSILON || rlen < POSITION_CHANGE_EPSILON) return 0.0f; return (dx / len) * (rx / rlen) + (dy / len) * (ry / rlen); }
extern rudo_cursor_vfx_state *rudo_cursor_vfx_new(const rudo_cursor_vfx_settings *settings);
extern void rudo_cursor_vfx_free(rudo_cursor_vfx_state *vfx);
extern void rudo_cursor_vfx_set_settings(rudo_cursor_vfx_state *vfx, const rudo_cursor_vfx_settings *settings);
extern bool rudo_cursor_vfx_update(rudo_cursor_vfx_state *vfx, float cursor_x, float cursor_y, float cell_w, float cell_h, float dt);
extern void rudo_cursor_vfx_cursor_jumped(rudo_cursor_vfx_state *vfx, float x, float y);
extern size_t rudo_cursor_vfx_collect(const rudo_cursor_vfx_state *vfx, rudo_cursor_particle *out, size_t cap);
extern size_t rudo_cursor_vfx_count(const rudo_cursor_vfx_state *vfx);

rudo_cursor_renderer *rudo_cursor_renderer_new(void) { rudo_cursor_renderer *c = (rudo_cursor_renderer *)calloc(1, sizeof(*c)); rudo_cursor_vfx_settings vs; int i; if (!c) return NULL; for (i = 0; i < 4; ++i) corner_init(&c->corners[i], standard_corners[i * 2], standard_corners[i * 2 + 1]); c->shape = RUDO_CURSOR_SHAPE_BLOCK; c->settings.animation_length = RUDO_DEFAULT_CURSOR_ANIMATION_LENGTH_SECS; c->settings.short_animation_length = RUDO_DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS; c->settings.trail_size = RUDO_DEFAULT_CURSOR_TRAIL_SIZE; c->settings.blink_interval = RUDO_DEFAULT_CURSOR_BLINK_INTERVAL_SECS; c->prev_col = c->prev_row = -1.0f; c->blink_on = true; c->unfocused_outline_width = RUDO_DEFAULT_CURSOR_UNFOCUSED_OUTLINE_WIDTH; rudo_cursor_vfx_settings_default(&vs); c->vfx = rudo_cursor_vfx_new(&vs); return c; }
void rudo_cursor_renderer_free(rudo_cursor_renderer *c) { if (!c) return; rudo_cursor_vfx_free(c->vfx); free(c); }
void rudo_cursor_renderer_set_shape(rudo_cursor_renderer *c, rudo_cursor_shape shape) { int i; if (!c) return; for (i = 0; i < 4; ++i) corner_set_shape(&c->corners[i], shape, i); c->shape = shape; }
void rudo_cursor_renderer_set_animation_length(rudo_cursor_renderer *c, float s) { if (c) c->settings.animation_length = s > 0.0f ? s : 0.0f; }
void rudo_cursor_renderer_set_short_animation_length(rudo_cursor_renderer *c, float s) { if (c) c->settings.short_animation_length = s > 0.0f ? s : 0.0f; }
void rudo_cursor_renderer_set_trail_size(rudo_cursor_renderer *c, float t) { if (c) c->settings.trail_size = t; }
void rudo_cursor_renderer_set_blink_enabled(rudo_cursor_renderer *c, bool enabled) { if (!c) return; c->blink_enabled = enabled; c->blink_on = true; c->blink_timer = 0.0f; }
void rudo_cursor_renderer_set_blink_interval(rudo_cursor_renderer *c, float s) { if (!c) return; c->settings.blink_interval = s > 0.0f ? s : 0.0f; c->blink_timer = 0.0f; }
void rudo_cursor_renderer_set_smooth_blink(rudo_cursor_renderer *c, bool enabled) { if (c) c->smooth_blink = enabled; }
void rudo_cursor_renderer_set_unfocused(rudo_cursor_renderer *c, bool unfocused) { if (c) c->unfocused = unfocused; }
void rudo_cursor_renderer_set_unfocused_outline_width(rudo_cursor_renderer *c, float width) { if (c) c->unfocused_outline_width = width > 0.0f ? width : 0.0f; }
void rudo_cursor_renderer_set_vfx_settings(rudo_cursor_renderer *c, const rudo_cursor_vfx_settings *settings) { if (c && c->vfx && settings) rudo_cursor_vfx_set_settings(c->vfx, settings); }
bool rudo_cursor_renderer_is_visible(const rudo_cursor_renderer *c) { if (!c) return false; if (c->smooth_blink) return true; return !c->blink_enabled || c->blink_on; }
float rudo_cursor_renderer_blink_opacity(const rudo_cursor_renderer *c) { if (!c) return 0.0f; if (!c->blink_enabled || c->settings.blink_interval <= 0.0f) return 1.0f; if (c->smooth_blink) { float progress = c->blink_timer / c->settings.blink_interval; float phase = c->blink_on ? progress : 1.0f + progress; return cosf((float)M_PI * phase) * 0.5f + 0.5f; } return c->blink_on ? 1.0f : 0.0f; }
bool rudo_cursor_renderer_is_unfocused(const rudo_cursor_renderer *c) { return c && c->unfocused; }
float rudo_cursor_renderer_unfocused_outline_width(const rudo_cursor_renderer *c) { return c ? c->unfocused_outline_width : 0.0f; }
rudo_cursor_tick rudo_cursor_renderer_tick(rudo_cursor_renderer *c, float col, float row, float dt) { bool moved, blink_changed = false, animating = false, vfx_anim; float center_x, center_y; int i; rudo_cursor_tick tick = { false, false }; if (!c) return tick; moved = fabsf(col - c->prev_col) > POSITION_CHANGE_EPSILON || fabsf(row - c->prev_row) > POSITION_CHANGE_EPSILON; if (moved) { c->jumped = true; c->blink_on = true; c->blink_timer = 0.0f; if (c->vfx) rudo_cursor_vfx_cursor_jumped(c->vfx, col, row); }
    if (c->blink_enabled && c->settings.blink_interval > 0.0f) { c->blink_timer += dt > 0.0f ? dt : 0.0f; while (c->blink_timer >= c->settings.blink_interval) { c->blink_timer -= c->settings.blink_interval; c->blink_on = !c->blink_on; blink_changed = true; } } else { blink_changed = !c->blink_on; c->blink_on = true; c->blink_timer = 0.0f; }
    center_x = col + CURSOR_CELL_CENTER; center_y = row + CURSOR_CELL_CENTER; if (c->jumped) { struct align_pair { int idx; float a; } alignments[4], tmp; int ranks[4]; bool is_short = fabsf(col - c->prev_col) <= SHORT_MOVE_THRESHOLD_COLS && fabsf(row - c->prev_row) < POSITION_CHANGE_EPSILON; float short_length = RUDO_MIN(c->settings.animation_length, c->settings.short_animation_length); float leading = c->settings.animation_length * RUDO_CLAMP(1.0f - c->settings.trail_size, 0.0f, 1.0f); float trailing = c->settings.animation_length; float middle = (leading + trailing) * 0.5f; int a, b; for (i = 0; i < 4; ++i) { alignments[i].idx = i; alignments[i].a = corner_alignment(&c->corners[i], center_x, center_y, CELL_DIMENSION, CELL_DIMENSION); } for (a = 0; a < 4; ++a) for (b = a + 1; b < 4; ++b) if (alignments[a].a > alignments[b].a) { tmp = alignments[a]; alignments[a] = alignments[b]; alignments[b] = tmp; } for (i = 0; i < 4; ++i) ranks[alignments[i].idx] = i; for (i = 0; i < 4; ++i) c->corners[i].animation_length = is_short ? short_length : (ranks[i] >= 2 ? leading : ranks[i] == 1 ? middle : trailing); }
    c->prev_col = col; c->prev_row = row; for (i = 0; i < 4; ++i) animating |= corner_update(&c->corners[i], center_x, center_y, CELL_DIMENSION, CELL_DIMENSION, dt); c->jumped = false; vfx_anim = c->vfx ? rudo_cursor_vfx_update(c->vfx, col, row, CELL_DIMENSION, CELL_DIMENSION, dt) : false; animating |= vfx_anim; c->animating = animating; tick.needs_redraw = moved || blink_changed || animating; tick.animating = animating; return tick; }
void rudo_cursor_renderer_corner_positions(const rudo_cursor_renderer *c, float out_xy4[8]) { int i; if (!c || !out_xy4) return; for (i = 0; i < 4; ++i) { out_xy4[i * 2] = c->corners[i].current_x; out_xy4[i * 2 + 1] = c->corners[i].current_y; } }
size_t rudo_cursor_renderer_particle_count(const rudo_cursor_renderer *c) { return (c && c->vfx) ? rudo_cursor_vfx_count(c->vfx) : 0; }
size_t rudo_cursor_renderer_particles(const rudo_cursor_renderer *c, rudo_cursor_particle *out, size_t cap) { return (c && c->vfx) ? rudo_cursor_vfx_collect(c->vfx, out, cap) : 0; }
bool rudo_cursor_renderer_next_wakeup(const rudo_cursor_renderer *c, const struct timespec *elapsed_since_frame, struct timespec *out_duration) { float elapsed, remaining = 0.0f, blink_remaining; bool have = false; if (!c || !out_duration) return false; elapsed = timespec_to_seconds(elapsed_since_frame); if (c->animating) { remaining = CURSOR_ANIMATION_FRAME_INTERVAL_SECS - elapsed; if (remaining < 0.0f) remaining = 0.0f; have = true; }
    if (c->blink_enabled && c->settings.blink_interval > 0.0f) { blink_remaining = c->settings.blink_interval - c->blink_timer - elapsed; if (blink_remaining < 0.0f) blink_remaining = 0.0f; if (!have || blink_remaining < remaining) { remaining = blink_remaining; have = true; } }
    if (!have) return false;
    seconds_to_timespec(remaining, out_duration);
    return true; }
