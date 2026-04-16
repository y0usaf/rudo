#ifndef RUDO_RENDER_H
#define RUDO_RENDER_H

#include "rudo/common.h"
#include "rudo/defaults.h"
#include <time.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    uint8_t r;
    uint8_t g;
    uint8_t b;
} rudo_rgb;

typedef struct {
    uint32_t width;
    uint32_t height;
    uint32_t stride;
    uint8_t *pixels;
} rudo_framebuffer;

typedef struct {
    float u0;
    float v0;
    float u1;
    float v1;
    float width;
    float height;
    float offset_x;
    float offset_y;
} rudo_glyph_info;

typedef struct rudo_font_atlas rudo_font_atlas;

typedef enum {
    RUDO_CURSOR_SHAPE_BLOCK = 0,
    RUDO_CURSOR_SHAPE_BEAM = 1,
    RUDO_CURSOR_SHAPE_UNDERLINE = 2,
} rudo_cursor_shape;

typedef enum {
    RUDO_CURSOR_VFX_DISABLED = 0,
    RUDO_CURSOR_VFX_RAILGUN,
    RUDO_CURSOR_VFX_TORPEDO,
    RUDO_CURSOR_VFX_PIXIE_DUST,
    RUDO_CURSOR_VFX_SONIC_BOOM,
    RUDO_CURSOR_VFX_RIPPLE,
    RUDO_CURSOR_VFX_WIREFRAME,
} rudo_cursor_vfx_mode;

typedef enum {
    RUDO_CURSOR_PARTICLE_FILLED_OVAL = 0,
    RUDO_CURSOR_PARTICLE_STROKED_OVAL,
    RUDO_CURSOR_PARTICLE_FILLED_RECT,
    RUDO_CURSOR_PARTICLE_STROKED_RECT,
} rudo_cursor_particle_shape;

typedef struct {
    float x;
    float y;
    float radius;
    uint8_t alpha;
    rudo_cursor_particle_shape shape;
    float stroke_width;
} rudo_cursor_particle;

typedef struct {
    uint8_t foreground[3];
    uint8_t background[3];
    uint8_t cursor[3];
    uint8_t selection[3];
} rudo_theme_colors;

typedef struct {
    uint32_t col;
    uint32_t row;
    uint32_t width;
    uint32_t flags;
    uint32_t ch;
    uint32_t fg;
    uint32_t bg;
    uint8_t fg_default;
    uint8_t bg_default;
} rudo_render_cell;

typedef struct {
    const rudo_render_cell *cells;
    size_t cell_count;
    size_t cols;
    size_t rows;
    size_t cursor_col;
    size_t cursor_row;
    bool cursor_visible;
    bool viewing_scrollback;
    bool has_selection;
    size_t selection_start_col;
    size_t selection_start_row;
    size_t selection_end_col;
    size_t selection_end_row;
} rudo_render_grid;

enum {
    RUDO_CELL_FLAG_BOLD = 1u << 0,
    RUDO_CELL_FLAG_ITALIC = 1u << 1,
    RUDO_CELL_FLAG_UNDERLINE = 1u << 2,
    RUDO_CELL_FLAG_STRIKETHROUGH = 1u << 3,
    RUDO_CELL_FLAG_REVERSE = 1u << 4,
    RUDO_CELL_FLAG_DIM = 1u << 5,
    RUDO_CELL_FLAG_HIDDEN = 1u << 6,
    RUDO_CELL_FLAG_WIDE_SPACER = 1u << 7,
};

typedef struct {
    size_t start_row;
    size_t end_row_inclusive;
} rudo_render_row_range;

typedef struct {
    bool full_redraw;
    bool draw_cursor;
    const rudo_render_row_range *dirty_rows;
    size_t dirty_row_count;
} rudo_render_options;

typedef struct {
    bool needs_redraw;
    bool animating;
} rudo_cursor_tick;

typedef struct {
    float animation_length;
    float short_animation_length;
    float trail_size;
    float blink_interval;
} rudo_cursor_settings;

typedef struct {
    uint32_t modes[8];
    size_t mode_count;
    float opacity;
    float particle_lifetime;
    float particle_highlight_lifetime;
    float particle_density;
    float particle_speed;
    float particle_phase;
    float particle_curl;
} rudo_cursor_vfx_settings;

typedef struct rudo_cursor_renderer rudo_cursor_renderer;
typedef struct rudo_software_renderer rudo_software_renderer;

rudo_font_atlas *rudo_font_atlas_new(float font_size_px, const char *preferred_family);
void rudo_font_atlas_free(rudo_font_atlas *atlas);
float rudo_font_atlas_cell_width(const rudo_font_atlas *atlas);
float rudo_font_atlas_cell_height(const rudo_font_atlas *atlas);
float rudo_font_atlas_baseline(const rudo_font_atlas *atlas);
rudo_glyph_info rudo_font_atlas_get_glyph(rudo_font_atlas *atlas, uint32_t ch, bool bold, bool italic);
const uint8_t *rudo_font_atlas_data(const rudo_font_atlas *atlas, size_t *len_out);
uint32_t rudo_font_atlas_width(const rudo_font_atlas *atlas);
uint32_t rudo_font_atlas_height(const rudo_font_atlas *atlas);

void rudo_cursor_vfx_settings_default(rudo_cursor_vfx_settings *out);
rudo_cursor_vfx_mode rudo_cursor_vfx_mode_parse(const char *s);
size_t rudo_cursor_vfx_modes_parse(const char *s, uint32_t *out_modes, size_t out_cap);

rudo_cursor_renderer *rudo_cursor_renderer_new(void);
void rudo_cursor_renderer_free(rudo_cursor_renderer *cursor);
void rudo_cursor_renderer_set_shape(rudo_cursor_renderer *cursor, rudo_cursor_shape shape);
void rudo_cursor_renderer_set_animation_length(rudo_cursor_renderer *cursor, float seconds);
void rudo_cursor_renderer_set_short_animation_length(rudo_cursor_renderer *cursor, float seconds);
void rudo_cursor_renderer_set_trail_size(rudo_cursor_renderer *cursor, float trail_size);
void rudo_cursor_renderer_set_blink_enabled(rudo_cursor_renderer *cursor, bool enabled);
void rudo_cursor_renderer_set_blink_interval(rudo_cursor_renderer *cursor, float seconds);
void rudo_cursor_renderer_set_smooth_blink(rudo_cursor_renderer *cursor, bool enabled);
void rudo_cursor_renderer_set_unfocused(rudo_cursor_renderer *cursor, bool unfocused);
void rudo_cursor_renderer_set_unfocused_outline_width(rudo_cursor_renderer *cursor, float width);
void rudo_cursor_renderer_set_vfx_settings(rudo_cursor_renderer *cursor, const rudo_cursor_vfx_settings *settings);
rudo_cursor_tick rudo_cursor_renderer_tick(rudo_cursor_renderer *cursor, float col, float row, float dt_seconds);
bool rudo_cursor_renderer_is_visible(const rudo_cursor_renderer *cursor);
float rudo_cursor_renderer_blink_opacity(const rudo_cursor_renderer *cursor);
bool rudo_cursor_renderer_is_unfocused(const rudo_cursor_renderer *cursor);
float rudo_cursor_renderer_unfocused_outline_width(const rudo_cursor_renderer *cursor);
void rudo_cursor_renderer_corner_positions(const rudo_cursor_renderer *cursor, float out_xy4[8]);
size_t rudo_cursor_renderer_particle_count(const rudo_cursor_renderer *cursor);
size_t rudo_cursor_renderer_particles(const rudo_cursor_renderer *cursor, rudo_cursor_particle *out, size_t cap);
bool rudo_cursor_renderer_next_wakeup(const rudo_cursor_renderer *cursor, const struct timespec *elapsed_since_frame, struct timespec *out_duration);

rudo_software_renderer *rudo_software_renderer_new(float font_size_points, const char *font_family, const rudo_theme_colors *theme, uint32_t padding_px);
void rudo_software_renderer_free(rudo_software_renderer *renderer);
void rudo_software_renderer_set_theme(rudo_software_renderer *renderer, const rudo_theme_colors *theme);
void rudo_software_renderer_set_scale(rudo_software_renderer *renderer, float scale);
void rudo_software_renderer_set_font_size(rudo_software_renderer *renderer, float font_size_points);
void rudo_software_renderer_increase_font_size(rudo_software_renderer *renderer, float delta_points);
void rudo_software_renderer_decrease_font_size(rudo_software_renderer *renderer, float delta_points);
void rudo_software_renderer_reset_font_size(rudo_software_renderer *renderer);
void rudo_software_renderer_set_background_alpha(rudo_software_renderer *renderer, uint8_t alpha);
void rudo_software_renderer_cell_size(const rudo_software_renderer *renderer, float *cell_width, float *cell_height);
void rudo_software_renderer_grid_offset(const rudo_software_renderer *renderer, float *offset_x, float *offset_y);
void rudo_software_renderer_grid_layout(rudo_software_renderer *renderer, uint32_t width, uint32_t height, size_t *cols, size_t *rows);
void rudo_software_renderer_window_size_for_grid(const rudo_software_renderer *renderer, size_t cols, size_t rows, uint32_t *width, uint32_t *height);
void rudo_software_renderer_pixel_bounds_for_row_range(const rudo_software_renderer *renderer, size_t start_row, size_t end_row_inclusive, uint32_t *y0, uint32_t *y1);
void rudo_software_renderer_render(rudo_software_renderer *renderer, rudo_framebuffer *fb, const rudo_render_grid *grid, const rudo_cursor_renderer *cursor, rudo_render_options options);

#ifdef __cplusplus
}
#endif

#endif
