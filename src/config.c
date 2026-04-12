#include "rudo/config.h"
#include "rudo/log.h"

#include <ctype.h>
#include <errno.h>
#include <math.h>
#include <sys/stat.h>

const char *const rudo_default_ansi_hex[16] = {
    "#000000", "#cc0000", "#00cc00", "#cccc00", "#0000cc", "#cc00cc", "#00cccc", "#cccccc",
    "#555555", "#ff5555", "#55ff55", "#ffff55", "#5555ff", "#ff55ff", "#55ffff", "#ffffff",
};

static void set_string(char **dst, const char *src) { free(*dst); *dst = rudo_strdup(src); }
static float sanitize_f32(float value, float minimum, float def) { return isfinite(value) ? (value < minimum ? minimum : value) : def; }
static uint32_t get_u32(const rudo_toml_table *t, const char *s, const char *k, uint32_t def) { size_t v; return rudo_toml_get_usize(t, s, k, &v) ? (uint32_t)v : def; }
static const char *get_str_def(const rudo_toml_table *t, const char *s, const char *k, const char *def) { const char *v = rudo_toml_get_str(t, s, k); return v ? v : def; }
static bool get_bool_def(const rudo_toml_table *t, const char *s, const char *k, bool def) { bool v; return rudo_toml_get_bool(t, s, k, &v) ? v : def; }
static float get_f32_def(const rudo_toml_table *t, const char *s, const char *k, float def) { float v; return rudo_toml_get_f32(t, s, k, &v) ? v : def; }
static size_t get_usize_def(const rudo_toml_table *t, const char *s, const char *k, size_t def) { size_t v; return rudo_toml_get_usize(t, s, k, &v) ? v : def; }

static void color_defaults(rudo_color_config *c) {
    c->foreground = rudo_strdup(RUDO_DEFAULT_FOREGROUND_HEX);
    c->background = rudo_strdup(RUDO_DEFAULT_BACKGROUND_HEX);
    c->cursor = rudo_strdup(RUDO_DEFAULT_CURSOR_HEX);
    c->selection = rudo_strdup(RUDO_DEFAULT_SELECTION_HEX);
    c->black = rudo_strdup(rudo_default_ansi_hex[0]);
    c->red = rudo_strdup(rudo_default_ansi_hex[1]);
    c->green = rudo_strdup(rudo_default_ansi_hex[2]);
    c->yellow = rudo_strdup(rudo_default_ansi_hex[3]);
    c->blue = rudo_strdup(rudo_default_ansi_hex[4]);
    c->magenta = rudo_strdup(rudo_default_ansi_hex[5]);
    c->cyan = rudo_strdup(rudo_default_ansi_hex[6]);
    c->white = rudo_strdup(rudo_default_ansi_hex[7]);
    c->bright_black = rudo_strdup(rudo_default_ansi_hex[8]);
    c->bright_red = rudo_strdup(rudo_default_ansi_hex[9]);
    c->bright_green = rudo_strdup(rudo_default_ansi_hex[10]);
    c->bright_yellow = rudo_strdup(rudo_default_ansi_hex[11]);
    c->bright_blue = rudo_strdup(rudo_default_ansi_hex[12]);
    c->bright_magenta = rudo_strdup(rudo_default_ansi_hex[13]);
    c->bright_cyan = rudo_strdup(rudo_default_ansi_hex[14]);
    c->bright_white = rudo_strdup(rudo_default_ansi_hex[15]);
}

void rudo_config_init_default(rudo_config *cfg) {
    memset(cfg, 0, sizeof(*cfg));
    cfg->font.family = rudo_strdup(RUDO_DEFAULT_FONT_FAMILY);
    cfg->font.size = RUDO_DEFAULT_FONT_SIZE;
    cfg->font.size_adjustment = RUDO_DEFAULT_FONT_SIZE_ADJUSTMENT;
    cfg->font.bold_is_bright = RUDO_DEFAULT_BOLD_IS_BRIGHT;
    color_defaults(&cfg->colors);
    cfg->cursor.style = rudo_strdup(RUDO_DEFAULT_CURSOR_STYLE);
    cfg->cursor.animation_length = RUDO_DEFAULT_CURSOR_ANIMATION_LENGTH_SECS;
    cfg->cursor.short_animation_length = RUDO_DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS;
    cfg->cursor.trail_size = RUDO_DEFAULT_CURSOR_TRAIL_SIZE;
    cfg->cursor.blink = RUDO_DEFAULT_CURSOR_BLINK_ENABLED;
    cfg->cursor.blink_interval = RUDO_DEFAULT_CURSOR_BLINK_INTERVAL_SECS;
    cfg->cursor.vfx_mode = rudo_strdup(RUDO_DEFAULT_CURSOR_VFX_MODE);
    cfg->cursor.vfx_opacity = RUDO_DEFAULT_CURSOR_VFX_OPACITY;
    cfg->cursor.vfx_particle_lifetime = RUDO_DEFAULT_CURSOR_VFX_PARTICLE_LIFETIME;
    cfg->cursor.vfx_particle_highlight_lifetime = RUDO_DEFAULT_CURSOR_VFX_PARTICLE_HIGHLIGHT_LIFETIME;
    cfg->cursor.vfx_particle_density = RUDO_DEFAULT_CURSOR_VFX_PARTICLE_DENSITY;
    cfg->cursor.vfx_particle_speed = RUDO_DEFAULT_CURSOR_VFX_PARTICLE_SPEED;
    cfg->cursor.vfx_particle_phase = RUDO_DEFAULT_CURSOR_VFX_PARTICLE_PHASE;
    cfg->cursor.vfx_particle_curl = RUDO_DEFAULT_CURSOR_VFX_PARTICLE_CURL;
    cfg->cursor.smooth_blink = RUDO_DEFAULT_CURSOR_SMOOTH_BLINK;
    cfg->cursor.unfocused_outline_width = RUDO_DEFAULT_CURSOR_UNFOCUSED_OUTLINE_WIDTH;
    cfg->window.padding = RUDO_DEFAULT_WINDOW_PADDING_PX;
    cfg->window.title = rudo_strdup(RUDO_APP_NAME);
    cfg->window.app_id = rudo_strdup(RUDO_APP_NAME);
    cfg->window.initial_width = RUDO_DEFAULT_WINDOW_INITIAL_WIDTH;
    cfg->window.initial_height = RUDO_DEFAULT_WINDOW_INITIAL_HEIGHT;
    cfg->window.opacity = RUDO_DEFAULT_WINDOW_OPACITY;
    cfg->terminal.cols = RUDO_DEFAULT_TERMINAL_COLS;
    cfg->terminal.rows = RUDO_DEFAULT_TERMINAL_ROWS;
    cfg->terminal.term = rudo_strdup(RUDO_DEFAULT_TERM);
    cfg->terminal.colorterm = rudo_strdup(RUDO_DEFAULT_COLORTERM);
    cfg->terminal.shell_fallback = rudo_strdup(RUDO_DEFAULT_SHELL_FALLBACK);
    cfg->scrollback.lines = RUDO_DEFAULT_SCROLLBACK_LINES;
    rudo_keybindings_config_init_default(&cfg->keybindings);
}

void rudo_config_destroy(rudo_config *cfg) {
    free(cfg->font.family);
    free(cfg->colors.foreground); free(cfg->colors.background); free(cfg->colors.cursor); free(cfg->colors.selection);
    free(cfg->colors.black); free(cfg->colors.red); free(cfg->colors.green); free(cfg->colors.yellow); free(cfg->colors.blue); free(cfg->colors.magenta); free(cfg->colors.cyan); free(cfg->colors.white);
    free(cfg->colors.bright_black); free(cfg->colors.bright_red); free(cfg->colors.bright_green); free(cfg->colors.bright_yellow); free(cfg->colors.bright_blue); free(cfg->colors.bright_magenta); free(cfg->colors.bright_cyan); free(cfg->colors.bright_white);
    free(cfg->cursor.style); free(cfg->cursor.vfx_mode);
    free(cfg->window.title); free(cfg->window.app_id);
    free(cfg->terminal.term); free(cfg->terminal.colorterm); free(cfg->terminal.shell_fallback);
    rudo_keybindings_config_destroy(&cfg->keybindings);
    memset(cfg, 0, sizeof(*cfg));
}

void rudo_config_normalize(rudo_config *cfg) {
    cfg->font.size = sanitize_f32(cfg->font.size, 1.0f, RUDO_DEFAULT_FONT_SIZE);
    cfg->font.size_adjustment = sanitize_f32(cfg->font.size_adjustment, 0.1f, RUDO_DEFAULT_FONT_SIZE_ADJUSTMENT);
    cfg->cursor.animation_length = sanitize_f32(cfg->cursor.animation_length, 0.0f, RUDO_DEFAULT_CURSOR_ANIMATION_LENGTH_SECS);
    cfg->cursor.short_animation_length = sanitize_f32(cfg->cursor.short_animation_length, 0.0f, RUDO_DEFAULT_CURSOR_SHORT_ANIMATION_LENGTH_SECS);
    cfg->cursor.trail_size = sanitize_f32(cfg->cursor.trail_size, 0.0f, RUDO_DEFAULT_CURSOR_TRAIL_SIZE);
    cfg->cursor.blink_interval = sanitize_f32(cfg->cursor.blink_interval, 0.000001f, RUDO_DEFAULT_CURSOR_BLINK_INTERVAL_SECS);
    cfg->cursor.vfx_opacity = sanitize_f32(cfg->cursor.vfx_opacity, 0.0f, RUDO_DEFAULT_CURSOR_VFX_OPACITY);
    cfg->cursor.vfx_particle_lifetime = sanitize_f32(cfg->cursor.vfx_particle_lifetime, 0.0f, RUDO_DEFAULT_CURSOR_VFX_PARTICLE_LIFETIME);
    cfg->cursor.vfx_particle_highlight_lifetime = sanitize_f32(cfg->cursor.vfx_particle_highlight_lifetime, 0.0f, RUDO_DEFAULT_CURSOR_VFX_PARTICLE_HIGHLIGHT_LIFETIME);
    cfg->cursor.vfx_particle_density = sanitize_f32(cfg->cursor.vfx_particle_density, 0.0f, RUDO_DEFAULT_CURSOR_VFX_PARTICLE_DENSITY);
    cfg->cursor.vfx_particle_speed = sanitize_f32(cfg->cursor.vfx_particle_speed, 0.0f, RUDO_DEFAULT_CURSOR_VFX_PARTICLE_SPEED);
    cfg->cursor.vfx_particle_phase = sanitize_f32(cfg->cursor.vfx_particle_phase, 0.0f, RUDO_DEFAULT_CURSOR_VFX_PARTICLE_PHASE);
    cfg->cursor.vfx_particle_curl = sanitize_f32(cfg->cursor.vfx_particle_curl, 0.0f, RUDO_DEFAULT_CURSOR_VFX_PARTICLE_CURL);
    cfg->cursor.unfocused_outline_width = sanitize_f32(cfg->cursor.unfocused_outline_width, 0.0f, RUDO_DEFAULT_CURSOR_UNFOCUSED_OUTLINE_WIDTH);
    if (cfg->window.initial_width < 1) cfg->window.initial_width = 1;
    if (cfg->window.initial_height < 1) cfg->window.initial_height = 1;
    cfg->window.opacity = isfinite(cfg->window.opacity) ? RUDO_CLAMP(cfg->window.opacity, 0.0f, 1.0f) : RUDO_DEFAULT_WINDOW_OPACITY;
    if (cfg->terminal.cols < 2) cfg->terminal.cols = 2;
    if (cfg->terminal.rows < 2) cfg->terminal.rows = 2;
}

static void load_keybinding_field(const rudo_toml_table *t, const char *key, rudo_keybinding_list *dst, const rudo_keybinding_list *def) {
    bool disabled;
    const char *spec;
    char *err = NULL;
    if (rudo_toml_get_bool(t, "keybindings", key, &disabled) && !disabled) {
        rudo_keybinding_list_destroy(dst);
        rudo_keybinding_list_init(dst);
        return;
    }
    spec = rudo_toml_get_str(t, "keybindings", key);
    if (!spec) return;
    rudo_keybinding_list_destroy(dst);
    if (!rudo_parse_binding_list(dst, spec, &err)) {
        rudo_warn_log("Invalid keybinding for [keybindings].%s: %s, using defaults", key, err ? err : "parse error");
        free(err);
        rudo_keybinding_list_init(dst);
        for (size_t i = 0; i < def->len; ++i) rudo_keybinding_list_push(dst, &def->v[i]);
    }
}

bool rudo_config_from_toml(rudo_config *cfg, const rudo_toml_table *table) {
    rudo_config def;
    rudo_config_init_default(&def);
    set_string(&cfg->font.family, get_str_def(table, "font", "family", def.font.family));
    cfg->font.size = get_f32_def(table, "font", "size", def.font.size);
    cfg->font.size_adjustment = get_f32_def(table, "font", "size_adjustment", def.font.size_adjustment);
    cfg->font.bold_is_bright = get_bool_def(table, "font", "bold_is_bright", def.font.bold_is_bright);
#define SETC(field, sec, key) set_string(&cfg->colors.field, get_str_def(table, sec, key, def.colors.field))
    SETC(foreground, "colors", "foreground"); SETC(background, "colors", "background"); SETC(cursor, "colors", "cursor"); SETC(selection, "colors", "selection");
    SETC(black, "colors", "black"); SETC(red, "colors", "red"); SETC(green, "colors", "green"); SETC(yellow, "colors", "yellow"); SETC(blue, "colors", "blue"); SETC(magenta, "colors", "magenta"); SETC(cyan, "colors", "cyan"); SETC(white, "colors", "white");
    SETC(bright_black, "colors", "bright_black"); SETC(bright_red, "colors", "bright_red"); SETC(bright_green, "colors", "bright_green"); SETC(bright_yellow, "colors", "bright_yellow"); SETC(bright_blue, "colors", "bright_blue"); SETC(bright_magenta, "colors", "bright_magenta"); SETC(bright_cyan, "colors", "bright_cyan"); SETC(bright_white, "colors", "bright_white");
#undef SETC
    set_string(&cfg->cursor.style, get_str_def(table, "cursor", "style", def.cursor.style));
    cfg->cursor.animation_length = get_f32_def(table, "cursor", "animation_length", def.cursor.animation_length);
    cfg->cursor.short_animation_length = get_f32_def(table, "cursor", "short_animation_length", def.cursor.short_animation_length);
    cfg->cursor.trail_size = get_f32_def(table, "cursor", "trail_size", def.cursor.trail_size);
    cfg->cursor.blink = get_bool_def(table, "cursor", "blink", def.cursor.blink);
    cfg->cursor.blink_interval = get_f32_def(table, "cursor", "blink_interval", def.cursor.blink_interval);
    set_string(&cfg->cursor.vfx_mode, get_str_def(table, "cursor", "vfx_mode", def.cursor.vfx_mode));
    cfg->cursor.vfx_opacity = get_f32_def(table, "cursor", "vfx_opacity", def.cursor.vfx_opacity);
    cfg->cursor.vfx_particle_lifetime = get_f32_def(table, "cursor", "vfx_particle_lifetime", def.cursor.vfx_particle_lifetime);
    cfg->cursor.vfx_particle_highlight_lifetime = get_f32_def(table, "cursor", "vfx_particle_highlight_lifetime", def.cursor.vfx_particle_highlight_lifetime);
    cfg->cursor.vfx_particle_density = get_f32_def(table, "cursor", "vfx_particle_density", def.cursor.vfx_particle_density);
    cfg->cursor.vfx_particle_speed = get_f32_def(table, "cursor", "vfx_particle_speed", def.cursor.vfx_particle_speed);
    cfg->cursor.vfx_particle_phase = get_f32_def(table, "cursor", "vfx_particle_phase", def.cursor.vfx_particle_phase);
    cfg->cursor.vfx_particle_curl = get_f32_def(table, "cursor", "vfx_particle_curl", def.cursor.vfx_particle_curl);
    cfg->cursor.smooth_blink = get_bool_def(table, "cursor", "smooth_blink", def.cursor.smooth_blink);
    cfg->cursor.unfocused_outline_width = get_f32_def(table, "cursor", "unfocused_outline_width", def.cursor.unfocused_outline_width);
    cfg->window.padding = get_u32(table, "window", "padding", def.window.padding);
    set_string(&cfg->window.title, get_str_def(table, "window", "title", def.window.title));
    set_string(&cfg->window.app_id, get_str_def(table, "window", "app_id", def.window.app_id));
    cfg->window.initial_width = get_u32(table, "window", "initial_width", def.window.initial_width);
    cfg->window.initial_height = get_u32(table, "window", "initial_height", def.window.initial_height);
    cfg->window.opacity = get_f32_def(table, "window", "opacity", def.window.opacity);
    cfg->terminal.cols = get_usize_def(table, "terminal", "cols", def.terminal.cols);
    cfg->terminal.rows = get_usize_def(table, "terminal", "rows", def.terminal.rows);
    set_string(&cfg->terminal.term, get_str_def(table, "terminal", "term", def.terminal.term));
    set_string(&cfg->terminal.colorterm, get_str_def(table, "terminal", "colorterm", def.terminal.colorterm));
    set_string(&cfg->terminal.shell_fallback, get_str_def(table, "terminal", "shell_fallback", def.terminal.shell_fallback));
    cfg->scrollback.lines = get_usize_def(table, "scrollback", "lines", def.scrollback.lines);
    load_keybinding_field(table, "copy", &cfg->keybindings.copy, &def.keybindings.copy);
    load_keybinding_field(table, "paste", &cfg->keybindings.paste, &def.keybindings.paste);
    load_keybinding_field(table, "zoom_in", &cfg->keybindings.zoom_in, &def.keybindings.zoom_in);
    load_keybinding_field(table, "zoom_out", &cfg->keybindings.zoom_out, &def.keybindings.zoom_out);
    load_keybinding_field(table, "zoom_reset", &cfg->keybindings.zoom_reset, &def.keybindings.zoom_reset);
    rudo_config_normalize(cfg);
    rudo_config_destroy(&def);
    return true;
}

char *rudo_config_dir(void) {
    const char *xdg = getenv("XDG_CONFIG_HOME");
    if (xdg && *xdg == '/') return rudo_strdup(xdg);
    const char *home = getenv("HOME");
    if (home && *home == '/') return rudo_path_join2(home, ".config");
    return NULL;
}

bool rudo_config_paths(char **primary_out, char **legacy_out) {
    char *dir = rudo_config_dir();
    if (!dir) return false;
    *primary_out = rudo_path_join3(dir, RUDO_APP_NAME, RUDO_CONFIG_FILE_NAME);
    *legacy_out = rudo_path_join3(dir, RUDO_LEGACY_CONFIG_DIR_NAME, RUDO_CONFIG_FILE_NAME);
    free(dir);
    return true;
}

static bool read_file(const char *path, char **out) {
    FILE *f = fopen(path, "rb");
    long n;
    if (!f) return false;
    if (fseek(f, 0, SEEK_END) != 0) { fclose(f); return false; }
    n = ftell(f);
    if (n < 0) { fclose(f); return false; }
    rewind(f);
    *out = rudo_malloc((size_t)n + 1);
    if (fread(*out, 1, (size_t)n, f) != (size_t)n) { fclose(f); free(*out); *out = NULL; return false; }
    (*out)[n] = 0;
    fclose(f);
    return true;
}

bool rudo_config_load(rudo_config *cfg) {
    char *primary = NULL, *legacy = NULL, *path = NULL, *contents = NULL, *err = NULL;
    rudo_toml_table table;
    if (!rudo_config_paths(&primary, &legacy)) { rudo_info_log("No config directory found, using defaults"); rudo_config_init_default(cfg); return true; }
    struct stat st;
    if (stat(primary, &st) == 0) path = primary;
    else if (stat(legacy, &st) == 0) path = legacy;
    else { rudo_info_log("Config file not found at %s, using defaults", primary); rudo_config_init_default(cfg); free(primary); free(legacy); return true; }
    if (path == legacy) rudo_info_log("Loaded legacy config from %s, consider moving it to %s", legacy, primary);
    if (!read_file(path, &contents)) { rudo_warn_log("Failed to read config at %s: %s, using defaults", path, strerror(errno)); rudo_config_init_default(cfg); free(primary); free(legacy); return true; }
    if (!rudo_toml_parse(&table, contents, &err)) { rudo_warn_log("Failed to parse config at %s: %s, using defaults", path, err ? err : "parse error"); free(err); free(contents); free(primary); free(legacy); rudo_config_init_default(cfg); return true; }
    rudo_config_init_default(cfg);
    rudo_info_log("Loaded config from %s", path);
    rudo_config_from_toml(cfg, &table);
    rudo_toml_table_destroy(&table);
    free(contents); free(primary); free(legacy);
    return true;
}

bool rudo_parse_hex_color(const char *hex, uint8_t *r, uint8_t *g, uint8_t *b) {
    if (*hex == '#') ++hex;
    if (strlen(hex) != 6) return false;
    char buf[3] = {0,0,0};
    buf[0] = hex[0]; buf[1] = hex[1]; *r = (uint8_t)strtoul(buf, NULL, 16);
    buf[0] = hex[2]; buf[1] = hex[3]; *g = (uint8_t)strtoul(buf, NULL, 16);
    buf[0] = hex[4]; buf[1] = hex[5]; *b = (uint8_t)strtoul(buf, NULL, 16);
    for (int i = 0; i < 6; ++i) if (!isxdigit((unsigned char)hex[i])) return false;
    return true;
}
