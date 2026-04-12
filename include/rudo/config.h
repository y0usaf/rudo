#ifndef RUDO_CONFIG_H
#define RUDO_CONFIG_H

#include "rudo/defaults.h"
#include "rudo/keybindings.h"
#include "rudo/toml.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    char *family;
    float size;
    float size_adjustment;
    bool bold_is_bright;
} rudo_font_config;

typedef struct {
    char *foreground;
    char *background;
    char *cursor;
    char *selection;
    char *black;
    char *red;
    char *green;
    char *yellow;
    char *blue;
    char *magenta;
    char *cyan;
    char *white;
    char *bright_black;
    char *bright_red;
    char *bright_green;
    char *bright_yellow;
    char *bright_blue;
    char *bright_magenta;
    char *bright_cyan;
    char *bright_white;
} rudo_color_config;

typedef struct {
    char *style;
    float animation_length;
    float short_animation_length;
    float trail_size;
    bool blink;
    float blink_interval;
    char *vfx_mode;
    float vfx_opacity;
    float vfx_particle_lifetime;
    float vfx_particle_highlight_lifetime;
    float vfx_particle_density;
    float vfx_particle_speed;
    float vfx_particle_phase;
    float vfx_particle_curl;
    bool smooth_blink;
    float unfocused_outline_width;
} rudo_cursor_config;

typedef struct {
    uint32_t padding;
    char *title;
    char *app_id;
    uint32_t initial_width;
    uint32_t initial_height;
    float opacity;
} rudo_window_config;

typedef struct {
    size_t cols;
    size_t rows;
    char *term;
    char *colorterm;
    char *shell_fallback;
} rudo_terminal_config;

typedef struct {
    size_t lines;
} rudo_scrollback_config;

typedef struct {
    rudo_font_config font;
    rudo_color_config colors;
    rudo_cursor_config cursor;
    rudo_window_config window;
    rudo_terminal_config terminal;
    rudo_scrollback_config scrollback;
    rudo_keybindings_config keybindings;
} rudo_config;

void rudo_config_init_default(rudo_config *cfg);
void rudo_config_destroy(rudo_config *cfg);
void rudo_config_normalize(rudo_config *cfg);
bool rudo_config_from_toml(rudo_config *cfg, const rudo_toml_table *table);
bool rudo_config_load(rudo_config *cfg);

char *rudo_config_dir(void);
bool rudo_config_paths(char **primary_out, char **legacy_out);
bool rudo_parse_hex_color(const char *hex, uint8_t *r, uint8_t *g, uint8_t *b);

#ifdef __cplusplus
}
#endif

#endif
