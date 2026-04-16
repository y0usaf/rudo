#include "rudo/terminal.h"
#include "rudo/config.h"
#include "rudo/defaults.h"
#include "rudo/input.h"

#include <ctype.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define RUDO_CELL_BOLD (1u << 0)
#define RUDO_CELL_DIM (1u << 1)
#define RUDO_CELL_ITALIC (1u << 2)
#define RUDO_CELL_UNDERLINE (1u << 3)
#define RUDO_CELL_STRIKETHROUGH (1u << 4)
#define RUDO_CELL_BLINK (1u << 5)
#define RUDO_CELL_REVERSE (1u << 6)
#define RUDO_CELL_HIDDEN (1u << 7)
#define RUDO_CELL_WIDE (1u << 8)
#define RUDO_CELL_WIDE_SPACER (1u << 9)
#define RUDO_CELL_DIRTY (1u << 10)
#define RUDO_TAB_WIDTH 8u
#define RUDO_MAX_CSI_PARAMS 32u

typedef enum {
    RUDO_PSTATE_GROUND = 0,
    RUDO_PSTATE_ESC,
    RUDO_PSTATE_CSI,
    RUDO_PSTATE_OSC,
    RUDO_PSTATE_OSC_ESC,
    RUDO_PSTATE_DCS,
    RUDO_PSTATE_DCS_ESC,
} rudo_parser_state;

typedef enum {
    RUDO_CHARSET_ASCII = 0,
    RUDO_CHARSET_DEC_SPECIAL,
} rudo_charset;

static uint32_t pack_rgb(const uint8_t rgb[3]) { return ((uint32_t)rgb[0] << 16) | ((uint32_t)rgb[1] << 8) | rgb[2]; }
static uint8_t color_r(uint32_t color) { return (uint8_t)((color >> 16) & 0xffu); }
static uint8_t color_g(uint32_t color) { return (uint8_t)((color >> 8) & 0xffu); }
static uint8_t color_b(uint32_t color) { return (uint8_t)(color & 0xffu); }

static void cell_reset(rudo_cell *cell, const rudo_theme *theme) {
    cell->ch = ' ';
    cell->fg = pack_rgb(theme->foreground);
    cell->bg = pack_rgb(theme->background);
    cell->flags = 0;
    cell->fg_default = true;
    cell->bg_default = true;
}

static void cell_reset_with_bg(rudo_cell *cell, uint32_t bg, bool bg_default, const rudo_theme *theme) {
    cell->ch = ' ';
    cell->fg = pack_rgb(theme->foreground);
    cell->bg = bg;
    cell->flags = 0;
    cell->fg_default = true;
    cell->bg_default = bg_default;
}

static size_t storage_row_index_for(size_t logical_row, size_t total_rows, size_t capacity_rows, size_t row0) {
    if (!capacity_rows) capacity_rows = total_rows;
    if (!capacity_rows) return 0;
    return (row0 + logical_row) % capacity_rows;
}

static size_t storage_row_index(const rudo_grid *grid, size_t logical_row) {
    return storage_row_index_for(logical_row, grid->total_rows, grid->capacity_rows, grid->row0);
}

static void clear_row(rudo_grid *grid, size_t row, const rudo_theme *theme) {
    size_t i, idx = storage_row_index(grid, row) * grid->cols;
    for (i = 0; i < grid->cols; ++i) cell_reset(&grid->cells[idx + i], theme);
}

static void clear_row_with_bg(rudo_grid *grid, size_t row, uint32_t bg, bool bg_default, const rudo_theme *theme) {
    size_t i, idx = storage_row_index(grid, row) * grid->cols;
    for (i = 0; i < grid->cols; ++i) cell_reset_with_bg(&grid->cells[idx + i], bg, bg_default, theme);
}

static size_t visible_row_index(const rudo_grid *grid, size_t row) {
    size_t base = grid->total_rows > grid->rows ? grid->total_rows - grid->rows : 0;
    if (grid->view_offset > base) return row;
    return base - grid->view_offset + row;
}

static size_t row_base(const rudo_grid *grid, size_t row) { return storage_row_index(grid, visible_row_index(grid, row)) * grid->cols; }
static size_t clamp_col(const rudo_grid *grid, size_t col) { return grid->cols ? RUDO_MIN(col, grid->cols - 1u) : 0; }
static size_t clamp_row(const rudo_grid *grid, size_t row) { return grid->rows ? RUDO_MIN(row, grid->rows - 1u) : 0; }

static void cell_mark_dirty(rudo_cell *cell) { if (cell) cell->flags |= RUDO_CELL_DIRTY; }

static void encode_utf8_append(rudo_str *out, uint32_t ch) {
    char buf[4];
    size_t n = 0;
    if (!out) return;
    if (ch <= 0x7fu) {
        buf[n++] = (char)ch;
    } else if (ch <= 0x7ffu) {
        buf[n++] = (char)(0xc0u | (ch >> 6));
        buf[n++] = (char)(0x80u | (ch & 0x3fu));
    } else if (ch <= 0xffffu) {
        if (ch >= 0xd800u && ch <= 0xdfffu) ch = 0xfffdu;
        buf[n++] = (char)(0xe0u | (ch >> 12));
        buf[n++] = (char)(0x80u | ((ch >> 6) & 0x3fu));
        buf[n++] = (char)(0x80u | (ch & 0x3fu));
    } else if (ch <= 0x10ffffu) {
        buf[n++] = (char)(0xf0u | (ch >> 18));
        buf[n++] = (char)(0x80u | ((ch >> 12) & 0x3fu));
        buf[n++] = (char)(0x80u | ((ch >> 6) & 0x3fu));
        buf[n++] = (char)(0x80u | (ch & 0x3fu));
    } else {
        encode_utf8_append(out, 0xfffdu);
        return;
    }
    rudo_str_append_mem(out, buf, n);
}

static void repair_wide_pair_at(rudo_grid *grid, size_t row, size_t col) {
    rudo_cell *cell;
    if (!grid || row >= grid->rows || col >= grid->cols) return;
    cell = rudo_grid_cell_mut(grid, row, col);
    if ((cell->flags & RUDO_CELL_WIDE_SPACER) && (col == 0 || !(rudo_grid_cell(grid, row, col - 1u)->flags & RUDO_CELL_WIDE))) {
        cell->flags &= ~RUDO_CELL_WIDE_SPACER;
        cell->ch = ' ';
        cell_mark_dirty(cell);
    }
    if ((cell->flags & RUDO_CELL_WIDE) && (col + 1u >= grid->cols || !(rudo_grid_cell(grid, row, col + 1u)->flags & RUDO_CELL_WIDE_SPACER))) {
        cell->flags &= ~RUDO_CELL_WIDE;
        cell_mark_dirty(cell);
    }
}

static void repair_wide_around(rudo_grid *grid, size_t row, size_t start_col, size_t end_col) {
    size_t c0, c1, c;
    if (!grid || row >= grid->rows || !grid->cols) return;
    c0 = start_col > 0 ? start_col - 1u : 0u;
    c1 = RUDO_MIN(end_col + 1u, grid->cols - 1u);
    for (c = c0; c <= c1; ++c) {
        repair_wide_pair_at(grid, row, c);
        if (c == c1) break;
    }
}

static void clear_cell_for_overwrite(rudo_grid *g, size_t row, size_t col) {
    rudo_cell *cell;
    if (!g || row >= g->rows || col >= g->cols) return;
    cell = rudo_grid_cell_mut(g, row, col);
    if (cell->flags & RUDO_CELL_WIDE_SPACER) {
        if (col > 0) {
            rudo_cell *prev = rudo_grid_cell_mut(g, row, col - 1u);
            prev->flags &= ~RUDO_CELL_WIDE;
            prev->ch = ' ';
            cell_mark_dirty(prev);
        }
        cell->flags &= ~RUDO_CELL_WIDE_SPACER;
        cell->ch = ' ';
    }
    if (cell->flags & RUDO_CELL_WIDE) {
        if (col + 1u < g->cols) {
            rudo_cell *next = rudo_grid_cell_mut(g, row, col + 1u);
            next->flags &= ~RUDO_CELL_WIDE_SPACER;
            next->ch = ' ';
            cell_mark_dirty(next);
        }
        cell->flags &= ~RUDO_CELL_WIDE;
        cell->ch = ' ';
    }
    cell_mark_dirty(cell);
}

static bool parse_hex_color_component(const char *s, uint8_t *out) {
    char *end = NULL;
    unsigned long v;
    if (!s || !*s) return false;
    v = strtoul(s, &end, 16);
    if (!end || *end) return false;
    if (v > 0xfffful) return false;
    if (v <= 0xfful) *out = (uint8_t)v;
    else *out = (uint8_t)((v * 255ul + 32767ul) / 65535ul);
    return true;
}

static bool parse_osc_color(const char *spec, uint32_t *out) {
    char tmp[128], *parts[3], *save = NULL, *p;
    uint8_t r, g, b;
    size_t len;
    if (!spec || !out) return false;
    while (*spec == ' ' || *spec == '\t') ++spec;
    if (strncmp(spec, "rgb:", 4) == 0) {
        spec += 4;
        len = strlen(spec);
        if (len >= sizeof(tmp)) len = sizeof(tmp) - 1u;
        memcpy(tmp, spec, len);
        tmp[len] = 0;
        p = tmp;
        parts[0] = strtok_r(p, "/", &save);
        parts[1] = strtok_r(NULL, "/", &save);
        parts[2] = strtok_r(NULL, "/", &save);
        if (!parts[0] || !parts[1] || !parts[2] || strtok_r(NULL, "/", &save)) return false;
        if (!parse_hex_color_component(parts[0], &r) || !parse_hex_color_component(parts[1], &g) || !parse_hex_color_component(parts[2], &b)) return false;
        *out = ((uint32_t)r << 16) | ((uint32_t)g << 8) | b;
        return true;
    }
    return rudo_parse_hex_color(spec, &r, &g, &b) ? ((*out = ((uint32_t)r << 16) | ((uint32_t)g << 8) | b), true) : false;
}

static void format_osc_color(uint32_t color, char out[32]) {
    snprintf(out, 32, "rgb:%02x%02x/%02x%02x/%02x%02x", color_r(color), color_r(color), color_g(color), color_g(color), color_b(color), color_b(color));
}

static uint32_t theme_palette_color(const rudo_theme *theme, unsigned index) {
    unsigned r, g, b;
    if (index < 16u) return theme->ansi[index];
    if (index < 232u) {
        unsigned v = index - 16u;
        unsigned rr = v / 36u;
        unsigned gg = (v / 6u) % 6u;
        unsigned bb = v % 6u;
        r = rr ? 55u + 40u * rr : 0u;
        g = gg ? 55u + 40u * gg : 0u;
        b = bb ? 55u + 40u * bb : 0u;
        return (r << 16) | (g << 8) | b;
    }
    if (index < 256u) {
        unsigned v = 8u + 10u * (index - 232u);
        return (v << 16) | (v << 8) | v;
    }
    return 0;
}

static void theme_set_palette_color(rudo_theme *theme, unsigned index, uint32_t color) {
    if (index < 16u) theme->ansi[index] = color;
}

static bool grid_blank_cell(const rudo_terminal_parser *p, rudo_cell *out) {
    if (!p || !out) return false;
    out->ch = ' ';
    out->flags = 0;
    out->fg = pack_rgb(p->theme.foreground);
    out->bg = p->attr_bg;
    out->fg_default = true;
    out->bg_default = p->attr_bg_default;
    return true;
}

void rudo_theme_init_default(rudo_theme *theme) {
    size_t i;
    rudo_parse_hex_color(RUDO_DEFAULT_FOREGROUND_HEX, &theme->foreground[0], &theme->foreground[1], &theme->foreground[2]);
    rudo_parse_hex_color(RUDO_DEFAULT_BACKGROUND_HEX, &theme->background[0], &theme->background[1], &theme->background[2]);
    rudo_parse_hex_color(RUDO_DEFAULT_CURSOR_HEX, &theme->cursor[0], &theme->cursor[1], &theme->cursor[2]);
    rudo_parse_hex_color(RUDO_DEFAULT_SELECTION_HEX, &theme->selection[0], &theme->selection[1], &theme->selection[2]);
    for (i = 0; i < 16; ++i) {
        uint8_t r, g, b;
        rudo_parse_hex_color(rudo_default_ansi_hex[i], &r, &g, &b);
        theme->ansi[i] = ((uint32_t)r << 16) | ((uint32_t)g << 8) | b;
    }
}

bool rudo_theme_from_config_colors(rudo_theme *theme, const rudo_color_config *colors) {
    size_t i; const char *ansi[16]; uint8_t r, g, b;
    ansi[0]=colors->black; ansi[1]=colors->red; ansi[2]=colors->green; ansi[3]=colors->yellow; ansi[4]=colors->blue; ansi[5]=colors->magenta; ansi[6]=colors->cyan; ansi[7]=colors->white;
    ansi[8]=colors->bright_black; ansi[9]=colors->bright_red; ansi[10]=colors->bright_green; ansi[11]=colors->bright_yellow; ansi[12]=colors->bright_blue; ansi[13]=colors->bright_magenta; ansi[14]=colors->bright_cyan; ansi[15]=colors->bright_white;
    if (!rudo_parse_hex_color(colors->foreground, &theme->foreground[0], &theme->foreground[1], &theme->foreground[2])) return false;
    if (!rudo_parse_hex_color(colors->background, &theme->background[0], &theme->background[1], &theme->background[2])) return false;
    if (!rudo_parse_hex_color(colors->cursor, &theme->cursor[0], &theme->cursor[1], &theme->cursor[2])) return false;
    if (!rudo_parse_hex_color(colors->selection, &theme->selection[0], &theme->selection[1], &theme->selection[2])) return false;
    for (i = 0; i < 16; ++i) {
        if (!rudo_parse_hex_color(ansi[i], &r, &g, &b)) return false;
        theme->ansi[i] = ((uint32_t)r << 16) | ((uint32_t)g << 8) | b;
    }
    return true;
}

static bool load_theme_file_path(char **path_out) {
    const char *env = getenv(RUDO_THEME_ENV_VAR);
    if (env && *env == '/') { *path_out = rudo_strdup(env); return true; }
    char *dir = rudo_config_dir();
    if (!dir) return false;
    *path_out = rudo_path_join3(dir, RUDO_APP_NAME, RUDO_THEME_FILE_NAME);
    free(dir);
    return true;
}

bool rudo_theme_load(rudo_theme *theme) {
    char *path = NULL, *contents = NULL, *err = NULL; FILE *f; long n; rudo_toml_table table; rudo_config cfg;
    rudo_theme_init_default(theme);
    if (!load_theme_file_path(&path)) return false;
    f = fopen(path, "rb");
    if (!f) { free(path); return false; }
    if (fseek(f, 0, SEEK_END) != 0) { fclose(f); free(path); return false; }
    n = ftell(f); if (n < 0) { fclose(f); free(path); return false; }
    rewind(f); contents = rudo_malloc((size_t)n + 1u); if (fread(contents, 1, (size_t)n, f) != (size_t)n) { fclose(f); free(path); free(contents); return false; }
    fclose(f); contents[n] = 0;
    if (!rudo_toml_parse(&table, contents, &err)) { free(err); free(contents); free(path); return false; }
    rudo_config_init_default(&cfg); rudo_config_from_toml(&cfg, &table); rudo_theme_from_config_colors(theme, &cfg.colors); rudo_config_destroy(&cfg); rudo_toml_table_destroy(&table); free(contents); free(path); return true;
}

void rudo_grid_init(rudo_grid *grid, size_t cols, size_t rows, size_t scrollback_lines) { memset(grid, 0, sizeof(*grid)); rudo_grid_reset(grid, cols, rows, scrollback_lines); }
void rudo_grid_destroy(rudo_grid *grid) { free(grid->cells); free(grid->alt_cells); memset(grid, 0, sizeof(*grid)); }

void rudo_grid_reset(rudo_grid *grid, size_t cols, size_t rows, size_t scrollback_lines) {
    rudo_theme theme; size_t i;
    free(grid->cells); free(grid->alt_cells); memset(grid, 0, sizeof(*grid));
    grid->cols = cols < 2 ? 2 : cols; grid->rows = rows < 2 ? 2 : rows; grid->scrollback_limit = scrollback_lines; grid->capacity_rows = grid->rows; grid->total_rows = grid->rows; grid->cells = rudo_calloc(grid->cols * grid->capacity_rows, sizeof(*grid->cells)); grid->cursor_visible = true; grid->scroll_top = 0; grid->scroll_bottom = grid->rows ? grid->rows - 1u : 0;
    rudo_theme_init_default(&theme);
    for (i = 0; i < grid->cols * grid->capacity_rows; ++i) cell_reset(&grid->cells[i], &theme);
}

static void copy_row_prefix_cells(rudo_cell *dst, size_t dst_cols, const rudo_cell *src, size_t src_cols, size_t copy_cols) {
    if (!dst || !src || !dst_cols || !src_cols || !copy_cols) return;
    if (copy_cols > dst_cols) copy_cols = dst_cols;
    if (copy_cols > src_cols) copy_cols = src_cols;
    memcpy(dst, src, copy_cols * sizeof(*dst));
}

static void grid_resize_store(rudo_cell **cells_ptr,
                              size_t *capacity_rows_ptr,
                              size_t *row0_ptr,
                              size_t *total_rows_ptr,
                              size_t *history_rows_ptr,
                              size_t old_cols,
                              size_t old_rows,
                              size_t cursor_row,
                              size_t new_cols,
                              size_t new_rows,
                              size_t scrollback_limit,
                              bool preserve_scrollback,
                              size_t *cursor_row_out,
                              size_t *view_offset_out) {
    rudo_theme theme;
    rudo_cell *old_cells = cells_ptr ? *cells_ptr : NULL;
    size_t old_capacity_rows = capacity_rows_ptr ? *capacity_rows_ptr : 0;
    size_t old_row0 = row0_ptr ? *row0_ptr : 0;
    size_t old_history_rows = history_rows_ptr ? *history_rows_ptr : 0;
    size_t total_history, total_content, rows_at_and_below, new_anchor, anchor_logical;
    size_t visible_src_start, blank_top, max_scrollback, new_history_rows, scrollback_src_start;
    size_t visible_slots, visible_copy, copy_cols, src_logical, dst_idx, src_idx, i;
    rudo_cell *new_cells;

    if (!cells_ptr || !capacity_rows_ptr || !row0_ptr || !total_rows_ptr || !history_rows_ptr) return;

    new_cols = new_cols < 2 ? 2 : new_cols;
    new_rows = new_rows < 2 ? 2 : new_rows;
    total_history = old_rows + old_history_rows;
    if (!old_capacity_rows) old_capacity_rows = total_history;
    if (cursor_row >= old_rows) cursor_row = old_rows ? old_rows - 1u : 0u;
    total_content = old_history_rows + cursor_row + 1u;
    if (total_content <= new_rows) new_anchor = cursor_row;
    else {
        rows_at_and_below = old_rows - cursor_row;
        new_anchor = new_rows > rows_at_and_below ? new_rows - rows_at_and_below : 0u;
    }
    anchor_logical = old_history_rows + cursor_row;
    if (anchor_logical >= new_anchor) {
        visible_src_start = anchor_logical - new_anchor;
        blank_top = 0;
    } else {
        visible_src_start = 0;
        blank_top = new_anchor - anchor_logical;
    }
    max_scrollback = preserve_scrollback ? scrollback_limit : 0u;
    new_history_rows = RUDO_MIN(visible_src_start, max_scrollback);
    scrollback_src_start = visible_src_start - new_history_rows;
    visible_slots = new_rows > blank_top ? new_rows - blank_top : 0u;
    visible_copy = total_history > visible_src_start ? RUDO_MIN(total_history - visible_src_start, visible_slots) : 0u;
    copy_cols = RUDO_MIN(old_cols, new_cols);

    rudo_theme_init_default(&theme);
    new_cells = rudo_calloc((new_history_rows + new_rows) * new_cols, sizeof(*new_cells));
    for (i = 0; i < (new_history_rows + new_rows) * new_cols; ++i) cell_reset(&new_cells[i], &theme);

    for (i = 0; i < new_history_rows; ++i) {
        src_logical = scrollback_src_start + i;
        if (src_logical >= total_history) break;
        src_idx = storage_row_index_for(src_logical, total_history, old_capacity_rows, old_row0);
        dst_idx = i;
        copy_row_prefix_cells(new_cells + dst_idx * new_cols, new_cols, old_cells + src_idx * old_cols, old_cols, copy_cols);
    }

    for (i = 0; i < visible_copy; ++i) {
        src_logical = visible_src_start + i;
        if (src_logical >= total_history) break;
        src_idx = storage_row_index_for(src_logical, total_history, old_capacity_rows, old_row0);
        dst_idx = new_history_rows + blank_top + i;
        copy_row_prefix_cells(new_cells + dst_idx * new_cols, new_cols, old_cells + src_idx * old_cols, old_cols, copy_cols);
    }

    free(*cells_ptr);
    *cells_ptr = new_cells;
    *capacity_rows_ptr = new_history_rows + new_rows;
    *row0_ptr = 0;
    *total_rows_ptr = new_history_rows + new_rows;
    *history_rows_ptr = new_history_rows;
    if (cursor_row_out) *cursor_row_out = RUDO_MIN(new_anchor, new_rows - 1u);
    if (view_offset_out) *view_offset_out = 0;
}

void rudo_grid_resize(rudo_grid *grid, size_t cols, size_t rows) {
    size_t new_cols, new_rows, new_cursor_row, new_view_offset;
    size_t saved_cursor_row = 0, saved_view_offset = 0;
    if (!grid) return;
    new_cols = cols < 2 ? 2 : cols;
    new_rows = rows < 2 ? 2 : rows;
    if (new_cols == grid->cols && new_rows == grid->rows) return;

    grid_resize_store(&grid->cells,
                      &grid->capacity_rows,
                      &grid->row0,
                      &grid->total_rows,
                      &grid->history_rows,
                      grid->cols,
                      grid->rows,
                      grid->cursor_row,
                      new_cols,
                      new_rows,
                      grid->scrollback_limit,
                      !grid->alternate_active,
                      &new_cursor_row,
                      &new_view_offset);

    if (grid->alt_cells) {
        grid_resize_store(&grid->alt_cells,
                          &grid->alt_capacity_rows,
                          &grid->alt_row0,
                          &grid->alt_total_rows,
                          &grid->alt_history_rows,
                          grid->cols,
                          grid->rows,
                          grid->alt_cursor_row,
                          new_cols,
                          new_rows,
                          grid->scrollback_limit,
                          true,
                          &saved_cursor_row,
                          &saved_view_offset);
        grid->alt_cursor_row = saved_cursor_row;
        grid->alt_cursor_col = RUDO_MIN(grid->alt_cursor_col, new_cols);
        grid->alt_saved_cursor_row = RUDO_MIN(grid->alt_saved_cursor_row, new_rows - 1u);
        grid->alt_saved_cursor_col = RUDO_MIN(grid->alt_saved_cursor_col, new_cols);
        grid->alt_view_offset = saved_view_offset;
        grid->alt_scroll_top = 0;
        grid->alt_scroll_bottom = new_rows - 1u;
    }

    grid->cols = new_cols;
    grid->rows = new_rows;
    grid->cursor_row = new_cursor_row;
    grid->cursor_col = RUDO_MIN(grid->cursor_col, new_cols);
    grid->saved_cursor_row = RUDO_MIN(grid->saved_cursor_row, new_rows - 1u);
    grid->saved_cursor_col = RUDO_MIN(grid->saved_cursor_col, new_cols);
    grid->view_offset = new_view_offset;
    grid->scroll_top = 0;
    grid->scroll_bottom = new_rows - 1u;
}

size_t rudo_grid_cols(const rudo_grid *grid) { return grid->cols; }
size_t rudo_grid_rows(const rudo_grid *grid) { return grid->rows; }

static void grid_scroll_up_region(rudo_grid *grid, size_t top, size_t bottom, size_t lines, const rudo_cell *blank) {
    size_t r, copy_rows;
    if (!grid->rows || top > bottom || bottom >= grid->rows) return;
    lines = RUDO_MIN(lines, bottom - top + 1u);
    if (!lines) return;
    copy_rows = bottom - top + 1u - lines;
    if (top == 0 && bottom + 1u == grid->rows && !grid->alternate_active) {
        size_t row_size = grid->cols * sizeof(*grid->cells);
        size_t max_total = grid->rows + grid->scrollback_limit;
        size_t desired_total = grid->total_rows + lines;
        if (desired_total > grid->capacity_rows && grid->capacity_rows < max_total) {
            size_t old_capacity = grid->capacity_rows;
            size_t new_capacity = grid->capacity_rows ? grid->capacity_rows : grid->rows;
            if (new_capacity < grid->rows) new_capacity = grid->rows;
            while (new_capacity < desired_total && new_capacity < max_total) {
                size_t grown = new_capacity * 2u;
                if (grown <= new_capacity) { new_capacity = max_total; break; }
                new_capacity = RUDO_MIN(grown, max_total);
            }
            if (new_capacity < desired_total) new_capacity = RUDO_MIN(max_total, desired_total);
            grid->cells = rudo_realloc(grid->cells, new_capacity * row_size);
            for (r = old_capacity; r < new_capacity; ++r) {
                size_t base = r * grid->cols, c;
                for (c = 0; c < grid->cols; ++c) grid->cells[base + c] = *blank;
            }
            grid->capacity_rows = new_capacity;
        }
        if (desired_total > max_total && grid->capacity_rows) grid->row0 = (grid->row0 + desired_total - max_total) % grid->capacity_rows;
        grid->total_rows = RUDO_MIN(desired_total, max_total);
        grid->history_rows = grid->total_rows > grid->rows ? grid->total_rows - grid->rows : 0u;
        for (r = grid->total_rows - lines; r < grid->total_rows; ++r) {
            size_t base = storage_row_index(grid, r) * grid->cols, c;
            for (c = 0; c < grid->cols; ++c) grid->cells[base + c] = *blank;
        }
        if (grid->view_offset > grid->history_rows) grid->view_offset = grid->history_rows;
        return;
    }
    for (r = top; r + lines <= bottom; ++r) {
        memmove(&grid->cells[row_base(grid, r)], &grid->cells[row_base(grid, r + lines)], grid->cols * sizeof(*grid->cells));
    }
    for (r = bottom + 1u - lines; r <= bottom; ++r) {
        size_t base = row_base(grid, r), c;
        for (c = 0; c < grid->cols; ++c) grid->cells[base + c] = *blank;
        if (r == bottom) break;
    }
    RUDO_UNUSED(copy_rows);
}

static void grid_scroll_down_region(rudo_grid *grid, size_t top, size_t bottom, size_t lines, const rudo_cell *blank) {
    size_t r;
    if (!grid->rows || top > bottom || bottom >= grid->rows) return;
    lines = RUDO_MIN(lines, bottom - top + 1u);
    if (!lines) return;
    for (r = bottom + 1u - lines; r-- > top;) {
        memmove(&grid->cells[row_base(grid, r + lines)], &grid->cells[row_base(grid, r)], grid->cols * sizeof(*grid->cells));
    }
    for (r = top; r < top + lines; ++r) {
        size_t base = row_base(grid, r), c;
        for (c = 0; c < grid->cols; ++c) grid->cells[base + c] = *blank;
    }
}

void rudo_grid_linefeed(rudo_grid *grid) {
    rudo_theme theme; rudo_cell blank;
    rudo_theme_init_default(&theme); cell_reset(&blank, &theme);
    if (grid->cursor_row == grid->scroll_bottom) grid_scroll_up_region(grid, grid->scroll_top, grid->scroll_bottom, 1u, &blank);
    else if (grid->cursor_row + 1u < grid->rows) grid->cursor_row++;
}

void rudo_grid_put_codepoint(rudo_grid *grid, uint32_t ch, const rudo_theme *theme) {
    size_t idx;
    if (ch == '\n') { rudo_grid_linefeed(grid); return; }
    if (ch == '\r') { grid->cursor_col = 0; return; }
    if (ch == '\b') { if (grid->cursor_col) grid->cursor_col--; return; }
    if (grid->cursor_col >= grid->cols) { grid->cursor_col = 0; rudo_grid_linefeed(grid); }
    idx = row_base(grid, grid->cursor_row) + grid->cursor_col;
    grid->cells[idx].ch = ch ? ch : ' '; grid->cells[idx].fg = pack_rgb(theme->foreground); grid->cells[idx].bg = pack_rgb(theme->background); grid->cells[idx].flags = 0; grid->cells[idx].fg_default = true; grid->cells[idx].bg_default = true;
    if (++grid->cursor_col > grid->cols) grid->cursor_col = grid->cols;
}

void rudo_grid_set_cursor(rudo_grid *grid, size_t col, size_t row) { if (!grid->cols || !grid->rows) return; grid->cursor_col = RUDO_MIN(col, grid->cols); grid->cursor_row = RUDO_MIN(row, grid->rows - 1u); }
void rudo_grid_cursor_position(const rudo_grid *grid, size_t *col, size_t *row) { if (col) *col = clamp_col(grid, grid->cursor_col); if (row) *row = clamp_row(grid, grid->cursor_row); }
bool rudo_grid_scroll_view_up(rudo_grid *grid, size_t lines) { size_t max_off = grid->total_rows > grid->rows ? grid->total_rows - grid->rows : 0, old = grid->view_offset; if (grid->alternate_active) return false; grid->view_offset = RUDO_MIN(grid->view_offset + lines, max_off); return grid->view_offset != old; }
bool rudo_grid_scroll_view_down(rudo_grid *grid, size_t lines) { size_t old = grid->view_offset; grid->view_offset = lines > grid->view_offset ? 0 : grid->view_offset - lines; return grid->view_offset != old; }
bool rudo_grid_is_viewing_scrollback(const rudo_grid *grid) { return grid->view_offset != 0; }
void rudo_grid_reset_view(rudo_grid *grid) { grid->view_offset = 0; }
void rudo_grid_clear(rudo_grid *grid, const rudo_theme *theme) { size_t i, n = grid->capacity_rows ? grid->capacity_rows : grid->total_rows; for (i = 0; i < grid->cols * n; ++i) cell_reset(&grid->cells[i], theme); grid->cursor_col = grid->cursor_row = 0; grid->scroll_top = 0; grid->scroll_bottom = grid->rows ? grid->rows - 1u : 0; }
const rudo_cell *rudo_grid_cell(const rudo_grid *grid, size_t row, size_t col) { static const rudo_cell fallback = { .ch = " "[0], .fg = 0, .bg = 0, .flags = 0, .fg_default = true, .bg_default = true }; size_t idx, n; if (!grid || !grid->cells || row >= grid->rows || col >= grid->cols) return &fallback; idx = row_base(grid, row) + col; n = grid->capacity_rows ? grid->capacity_rows : grid->total_rows; if (idx >= grid->cols * n) return &fallback; return &grid->cells[idx]; }
rudo_cell *rudo_grid_cell_mut(rudo_grid *grid, size_t row, size_t col) { static rudo_cell fallback = { .ch = " "[0], .fg = 0, .bg = 0, .flags = 0, .fg_default = true, .bg_default = true }; size_t idx, n; if (!grid || !grid->cells || row >= grid->rows || col >= grid->cols) return &fallback; idx = row_base(grid, row) + col; n = grid->capacity_rows ? grid->capacity_rows : grid->total_rows; if (idx >= grid->cols * n) return &fallback; return &grid->cells[idx]; }
void rudo_grid_build_render_cells(const rudo_grid *grid, rudo_render_cell *out) { size_t row, col; for (row = 0; row < grid->rows; ++row) for (col = 0; col < grid->cols; ++col) { const rudo_cell *src = rudo_grid_cell(grid, row, col); rudo_render_cell *dst = &out[row * grid->cols + col]; dst->col = (uint32_t)col; dst->row = (uint32_t)row; dst->width = (src->flags & RUDO_CELL_WIDE) ? 2u : 1u; dst->flags = 0; if (src->flags & RUDO_CELL_BOLD) dst->flags |= RUDO_CELL_FLAG_BOLD; if (src->flags & RUDO_CELL_ITALIC) dst->flags |= RUDO_CELL_FLAG_ITALIC; if (src->flags & RUDO_CELL_UNDERLINE) dst->flags |= RUDO_CELL_FLAG_UNDERLINE; if (src->flags & RUDO_CELL_STRIKETHROUGH) dst->flags |= RUDO_CELL_FLAG_STRIKETHROUGH; if (src->flags & RUDO_CELL_REVERSE) dst->flags |= RUDO_CELL_FLAG_REVERSE; if (src->flags & RUDO_CELL_DIM) dst->flags |= RUDO_CELL_FLAG_DIM; if (src->flags & RUDO_CELL_HIDDEN) dst->flags |= RUDO_CELL_FLAG_HIDDEN; if (src->flags & RUDO_CELL_WIDE_SPACER) dst->flags |= RUDO_CELL_FLAG_WIDE_SPACER; dst->ch = src->ch; dst->fg = src->fg; dst->bg = src->bg; dst->fg_default = src->fg_default; dst->bg_default = src->bg_default; } }

static size_t damage_word_count(size_t rows) { return (rows + 63u) / 64u; }

void rudo_damage_init(rudo_damage_tracker *damage, size_t rows) { memset(damage, 0, sizeof(*damage)); rudo_damage_resize(damage, rows); }
void rudo_damage_destroy(rudo_damage_tracker *damage) { free(damage->bits); memset(damage, 0, sizeof(*damage)); }
void rudo_damage_resize(rudo_damage_tracker *damage, size_t rows) {
    size_t words = damage_word_count(rows);
    if (words) {
        damage->bits = rudo_realloc(damage->bits, words * sizeof(uint64_t));
        memset(damage->bits, 0, words * sizeof(uint64_t));
    } else {
        free(damage->bits);
        damage->bits = NULL;
    }
    damage->rows = rows;
    damage->full_damage = rows != 0;
}
void rudo_damage_clear(rudo_damage_tracker *damage) {
    size_t words = damage_word_count(damage->rows);
    if (words && damage->bits) memset(damage->bits, 0, words * sizeof(uint64_t));
    damage->full_damage = false;
}
void rudo_damage_mark_all(rudo_damage_tracker *damage) {
    size_t words = damage_word_count(damage->rows);
    if (words && damage->bits) memset(damage->bits, 0, words * sizeof(uint64_t));
    damage->full_damage = damage->rows != 0;
}
void rudo_damage_mark_row(rudo_damage_tracker *damage, size_t row) {
    if (!damage || damage->full_damage || row >= damage->rows || !damage->bits) return;
    damage->bits[row >> 6] |= 1ull << (row & 63u);
}
void rudo_damage_mark_rows(rudo_damage_tracker *damage, size_t start_row, size_t end_row) {
    size_t start_word, end_word, word;
    uint64_t start_mask, end_mask;
    if (!damage || damage->full_damage || !damage->rows || start_row >= damage->rows || !damage->bits) return;
    if (end_row >= damage->rows) end_row = damage->rows - 1u;
    if (start_row > end_row) return;
    start_word = start_row >> 6;
    end_word = end_row >> 6;
    start_mask = ~0ull << (start_row & 63u);
    end_mask = ~0ull >> (63u - (end_row & 63u));
    if (start_word == end_word) {
        damage->bits[start_word] |= start_mask & end_mask;
        return;
    }
    damage->bits[start_word] |= start_mask;
    for (word = start_word + 1u; word < end_word; ++word) damage->bits[word] = ~0ull;
    damage->bits[end_word] |= end_mask;
}
bool rudo_damage_has_damage(const rudo_damage_tracker *damage) {
    size_t i, n;
    if (!damage) return false;
    if (damage->full_damage) return damage->rows != 0;
    n = damage_word_count(damage->rows);
    for (i = 0; i < n; ++i) if (damage->bits[i]) return true;
    return false;
}
bool rudo_damage_is_full(const rudo_damage_tracker *damage) { return damage && damage->full_damage; }
size_t rudo_damage_collect_row_ranges(const rudo_damage_tracker *damage, rudo_render_row_range *out, size_t cap) {
    size_t count = 0, row = 0;
    if (!damage || !damage->rows) return 0;
    if (damage->full_damage) {
        if (out && cap) {
            out[0].start_row = 0;
            out[0].end_row_inclusive = damage->rows - 1u;
        }
        return 1;
    }
    while (row < damage->rows) {
        bool dirty = (damage->bits[row >> 6] & (1ull << (row & 63u))) != 0;
        if (!dirty) {
            ++row;
            continue;
        }
        {
            size_t start = row;
            do {
                ++row;
            } while (row < damage->rows && (damage->bits[row >> 6] & (1ull << (row & 63u))) != 0);
            if (out && count < cap) {
                out[count].start_row = start;
                out[count].end_row_inclusive = row - 1u;
            }
            ++count;
        }
    }
    return count;
}

void rudo_selection_init(rudo_selection *sel) { memset(sel, 0, sizeof(*sel)); sel->state = RUDO_SELECTION_NONE; }
void rudo_selection_clear(rudo_selection *sel) { sel->state = RUDO_SELECTION_NONE; sel->start.col = sel->start.row = sel->end.col = sel->end.row = 0; }
void rudo_selection_start(rudo_selection *sel, size_t col, size_t row) { sel->state = RUDO_SELECTION_SELECTING; sel->start.col = sel->end.col = col; sel->start.row = sel->end.row = row; }
void rudo_selection_update(rudo_selection *sel, size_t col, size_t row) { sel->end.col = col; sel->end.row = row; if (sel->state == RUDO_SELECTION_NONE) sel->state = RUDO_SELECTION_SELECTING; }
void rudo_selection_finish(rudo_selection *sel) { if (sel->state == RUDO_SELECTION_SELECTING) sel->state = (sel->start.col == sel->end.col && sel->start.row == sel->end.row) ? RUDO_SELECTION_NONE : RUDO_SELECTION_SELECTED; }
bool rudo_selection_has_selection(const rudo_selection *sel) { return sel->state != RUDO_SELECTION_NONE && !(sel->start.col == sel->end.col && sel->start.row == sel->end.row); }
void rudo_selection_snapshot(const rudo_selection *sel, rudo_selection_state *state, rudo_grid_point *start, rudo_grid_point *end) { if (state) *state = sel->state; if (start) *start = sel->start; if (end) *end = sel->end; }
char *rudo_selection_selected_text(const rudo_selection *sel, const rudo_grid *grid) {
    rudo_str out;
    size_t row0, row1, row;
    if (!rudo_selection_has_selection(sel) || !grid || !grid->cols || !grid->rows) return rudo_strdup("");
    rudo_str_init(&out);
    row0 = RUDO_MIN(sel->start.row, sel->end.row);
    row1 = RUDO_MIN(RUDO_MAX(sel->start.row, sel->end.row), grid->rows - 1u);
    for (row = row0; row <= row1; ++row) {
        size_t c0 = row == sel->start.row ? sel->start.col : 0, c1 = row == sel->end.row ? sel->end.col : grid->cols - 1u, c;
        if (row == sel->end.row && sel->end.row < sel->start.row) c0 = sel->end.col;
        if (row == sel->start.row && sel->start.row < sel->end.row) c1 = grid->cols - 1u;
        if (c0 > c1) { size_t t = c0; c0 = c1; c1 = t; }
        c1 = RUDO_MIN(c1, grid->cols - 1u);
        for (c = c0; c <= c1; ++c) {
            const rudo_cell *cell = rudo_grid_cell(grid, row, c);
            if (cell->flags & RUDO_CELL_WIDE_SPACER) { if (c == c1) break; continue; }
            encode_utf8_append(&out, cell->ch ? cell->ch : (uint32_t)' ');
            if (c == c1) break;
        }
        if (row != row1) rudo_str_append_char(&out, '\n');
        if (row == row1) break;
    }
    return rudo_str_take(&out);
}

static void responses_push(rudo_response_list *list, const char *s) { if (list->len == list->cap) { list->cap = list->cap ? list->cap * 2u : 8u; list->items = rudo_realloc(list->items, list->cap * sizeof(*list->items)); } list->items[list->len++] = rudo_strdup(s); }
static void parser_set_title(rudo_terminal_parser *parser, const char *title) { free(parser->title); parser->title = rudo_strdup(title ? title : ""); parser->title_set = true; }

static void parser_reset_attrs(rudo_terminal_parser *p) {
    p->attr_flags = 0;
    p->attr_fg = pack_rgb(p->theme.foreground);
    p->attr_bg = pack_rgb(p->theme.background);
    p->attr_fg_default = true;
    p->attr_bg_default = true;
}

void rudo_terminal_parser_init(rudo_terminal_parser *parser, const rudo_theme *theme) {
    memset(parser, 0, sizeof(*parser)); parser->cursor_shape_request = -1; if (theme) { parser->theme = *theme; parser->base_theme = *theme; } else { rudo_theme_init_default(&parser->theme); parser->base_theme = parser->theme; } parser->mouse.encoding = RUDO_MOUSE_ENCODING_X10; parser->cursor_visible = true; parser->scroll_region_active = false; parser->g0_charset = RUDO_CHARSET_ASCII; parser->g1_charset = RUDO_CHARSET_ASCII; parser->active_charset = 0; parser->state = RUDO_PSTATE_GROUND; parser_reset_attrs(parser);
}

void rudo_terminal_parser_destroy(rudo_terminal_parser *parser) { size_t i; for (i = 0; i < parser->responses.len; ++i) free(parser->responses.items[i]); free(parser->responses.items); free(parser->title); memset(parser, 0, sizeof(*parser)); }
void rudo_terminal_parser_reset(rudo_terminal_parser *parser, const rudo_theme *theme) { rudo_terminal_parser_destroy(parser); rudo_terminal_parser_init(parser, theme); }

static void parser_sync_default_attrs(rudo_terminal_parser *p) {
    if (p->attr_fg_default) p->attr_fg = pack_rgb(p->theme.foreground);
    if (p->attr_bg_default) p->attr_bg = pack_rgb(p->theme.background);
}

static uint32_t map_dec_special(uint32_t ch) {
    switch ((char)ch) {
        case '`': return 0x25c6u;
        case 'a': return 0x2592u;
        case 'f': return 0x00b0u;
        case 'g': return 0x00b1u;
        case 'j': return 0x2518u;
        case 'k': return 0x2510u;
        case 'l': return 0x250cu;
        case 'm': return 0x2514u;
        case 'n': return 0x253cu;
        case 'q': return 0x2500u;
        case 't': return 0x251cu;
        case 'u': return 0x2524u;
        case 'v': return 0x2534u;
        case 'w': return 0x252cu;
        case 'x': return 0x2502u;
        case 'y': return 0x2264u;
        case 'z': return 0x2265u;
        case '{': return 0x03c0u;
        case '|': return 0x2260u;
        case '}': return 0x00a3u;
        case '~': return 0x00b7u;
        default: return ch;
    }
}

static uint32_t parser_map_char(rudo_terminal_parser *p, uint32_t ch) {
    if (ch < 0x80u) {
        rudo_charset cs = p->active_charset ? p->g1_charset : p->g0_charset;
        if (cs == RUDO_CHARSET_DEC_SPECIAL) return map_dec_special(ch);
    }
    return ch;
}

static size_t utf8_width(uint32_t ch) {
    if (ch == 0 || ch < 0x20u) return 0;
    if (ch < 0x1100u) return 1;
    if ((ch >= 0x1100u && ch <= 0x115fu) || (ch >= 0x2329u && ch <= 0x232au) || (ch >= 0x2e80u && ch <= 0xa4cfu && ch != 0x303fu) || (ch >= 0xac00u && ch <= 0xd7a3u) || (ch >= 0xf900u && ch <= 0xfaffu) || (ch >= 0xfe10u && ch <= 0xfe19u) || (ch >= 0xfe30u && ch <= 0xfe6fu) || (ch >= 0xff00u && ch <= 0xff60u) || (ch >= 0xffe0u && ch <= 0xffe6u) || (ch >= 0x1f300u && ch <= 0x1f64fu) || (ch >= 0x1f900u && ch <= 0x1f9ffu) || (ch >= 0x20000u && ch <= 0x3fffdu)) return 2;
    return 1;
}

static void parser_mark_scroll_damage(rudo_damage_tracker *d, size_t top, size_t bottom) { rudo_damage_mark_rows(d, top, bottom); }

static void parser_write_cell(rudo_terminal_parser *p, rudo_grid *g, rudo_damage_tracker *d, uint32_t ch) {
    size_t width = utf8_width(ch), row, col, idx;
    if (!width) return;
    ch = parser_map_char(p, ch);
    if (g->cursor_col >= g->cols || (width == 2u && g->cursor_col + 1u >= g->cols)) {
        g->cursor_col = 0;
        if (g->cursor_row == g->scroll_bottom) {
            rudo_cell blank; grid_blank_cell(p, &blank); grid_scroll_up_region(g, g->scroll_top, g->scroll_bottom, 1u, &blank); parser_mark_scroll_damage(d, g->scroll_top, g->scroll_bottom);
        } else if (g->cursor_row + 1u < g->rows) g->cursor_row++;
    }
    row = g->cursor_row; col = g->cursor_col;
    if (p->insert_mode) {
        size_t c;
        rudo_cell blank;
        grid_blank_cell(p, &blank);
        for (c = g->cols - width; c-- > col;) {
            *rudo_grid_cell_mut(g, row, c + width) = *rudo_grid_cell(g, row, c);
        }
        for (c = 0; c < width && col + c < g->cols; ++c) *rudo_grid_cell_mut(g, row, col + c) = blank;
    }
    clear_cell_for_overwrite(g, row, col);
    if (width == 2u && col + 1u < g->cols) clear_cell_for_overwrite(g, row, col + 1u);
    idx = row_base(g, row) + col;
    g->cells[idx].ch = ch ? ch : ' ';
    g->cells[idx].fg = p->attr_fg;
    g->cells[idx].bg = p->attr_bg;
    g->cells[idx].flags = p->attr_flags | RUDO_CELL_DIRTY | (width == 2u ? RUDO_CELL_WIDE : 0u);
    g->cells[idx].fg_default = p->attr_fg_default;
    g->cells[idx].bg_default = p->attr_bg_default;
    if (width == 2u && col + 1u < g->cols) {
        idx = row_base(g, row) + col + 1u;
        g->cells[idx].ch = ' ';
        g->cells[idx].fg = p->attr_fg;
        g->cells[idx].bg = p->attr_bg;
        g->cells[idx].flags = RUDO_CELL_WIDE_SPACER | RUDO_CELL_DIRTY;
        g->cells[idx].fg_default = p->attr_fg_default;
        g->cells[idx].bg_default = p->attr_bg_default;
    }
    g->cursor_col += width;
    repair_wide_around(g, row, col, RUDO_MIN(g->cols - 1u, col + width));
    rudo_damage_mark_row(d, row);
}

static unsigned parse_uint_param(const char *s, unsigned def, bool zero_means_default) {
    unsigned long v;
    char *end = NULL;
    if (!s) return def;
    while (*s && !isdigit((unsigned char)*s)) s++;
    if (!*s) return def;
    v = strtoul(s, &end, 10);
    if (!end || *end) return def;
    if (zero_means_default && v == 0ul) return def;
    return (unsigned)v;
}

static size_t parser_row_min(const rudo_terminal_parser *p, const rudo_grid *g) { return p->origin_mode ? g->scroll_top : 0u; }
static size_t parser_row_max(const rudo_terminal_parser *p, const rudo_grid *g) { return p->origin_mode ? g->scroll_bottom : (g->rows ? g->rows - 1u : 0u); }
static size_t parser_absolute_row(const rudo_terminal_parser *p, const rudo_grid *g, size_t row) { return p->origin_mode ? RUDO_MIN(g->scroll_top + row, g->scroll_bottom) : clamp_row(g, row); }

static void grid_erase_line_range(rudo_grid *g, size_t row, size_t start_col, size_t end_col, const rudo_cell *blank) {
    size_t c;
    if (!g->rows || row >= g->rows || start_col >= g->cols) return;
    if (end_col >= g->cols) end_col = g->cols - 1u;
    for (c = start_col; c <= end_col; ++c) { clear_cell_for_overwrite(g, row, c); *rudo_grid_cell_mut(g, row, c) = *blank; }
    repair_wide_around(g, row, start_col, end_col);
}

static void grid_delete_chars(rudo_grid *g, size_t count, const rudo_cell *blank) {
    size_t row = g->cursor_row, col = g->cursor_col, c;
    if (col >= g->cols) return;
    count = RUDO_MIN(count, g->cols - col);
    for (c = col; c < g->cols; ++c) clear_cell_for_overwrite(g, row, c);
    for (c = col; c + count < g->cols; ++c) *rudo_grid_cell_mut(g, row, c) = *rudo_grid_cell(g, row, c + count);
    for (; c < g->cols; ++c) *rudo_grid_cell_mut(g, row, c) = *blank;
    repair_wide_around(g, row, col, g->cols - 1u);
}

static void grid_insert_chars(rudo_grid *g, size_t count, const rudo_cell *blank) {
    size_t row = g->cursor_row, col = g->cursor_col, c;
    if (col >= g->cols) return;
    count = RUDO_MIN(count, g->cols - col);
    for (c = col; c < g->cols; ++c) clear_cell_for_overwrite(g, row, c);
    for (c = g->cols - count; c-- > col;) *rudo_grid_cell_mut(g, row, c + count) = *rudo_grid_cell(g, row, c);
    for (c = 0; c < count; ++c) *rudo_grid_cell_mut(g, row, col + c) = *blank;
    repair_wide_around(g, row, col, g->cols - 1u);
}

static void grid_erase_chars(rudo_grid *g, size_t count, const rudo_cell *blank) {
    size_t row = g->cursor_row, col = g->cursor_col, c, end = RUDO_MIN(g->cols, col + count);
    for (c = col; c < end; ++c) { clear_cell_for_overwrite(g, row, c); *rudo_grid_cell_mut(g, row, c) = *blank; }
    if (end > col) repair_wide_around(g, row, col, end - 1u);
}

static void grid_insert_lines(rudo_grid *g, size_t count, const rudo_cell *blank) {
    if (g->cursor_row < g->scroll_top || g->cursor_row > g->scroll_bottom) return;
    grid_scroll_down_region(g, g->cursor_row, g->scroll_bottom, count, blank);
}

static void grid_delete_lines(rudo_grid *g, size_t count, const rudo_cell *blank) {
    if (g->cursor_row < g->scroll_top || g->cursor_row > g->scroll_bottom) return;
    grid_scroll_up_region(g, g->cursor_row, g->scroll_bottom, count, blank);
}

static void grid_reverse_index(rudo_grid *g, const rudo_cell *blank) {
    if (g->cursor_row == g->scroll_top) grid_scroll_down_region(g, g->scroll_top, g->scroll_bottom, 1u, blank);
    else if (g->cursor_row > 0) g->cursor_row--;
}

static void grid_set_scroll_region(rudo_grid *g, size_t top, size_t bottom) {
    top = clamp_row(g, top); bottom = clamp_row(g, bottom);
    if (top < bottom) { g->scroll_top = top; g->scroll_bottom = bottom; }
    else { g->scroll_top = 0; g->scroll_bottom = g->rows ? g->rows - 1u : 0; }
    g->cursor_col = 0; g->cursor_row = 0;
}

static void grid_save_cursor(rudo_grid *g) { g->saved_cursor_col = g->cursor_col; g->saved_cursor_row = g->cursor_row; }
static void grid_restore_cursor(rudo_grid *g) { g->cursor_col = RUDO_MIN(g->saved_cursor_col, g->cols); g->cursor_row = clamp_row(g, g->saved_cursor_row); }

static void grid_enter_alt(rudo_grid *g, const rudo_theme *theme) {
    size_t i;
    if (g->alternate_active) return;
    free(g->alt_cells);
    g->alt_cells = g->cells;
    g->alt_total_rows = g->total_rows;
    g->alt_history_rows = g->history_rows;
    g->alt_view_offset = g->view_offset;
    g->alt_capacity_rows = g->capacity_rows;
    g->alt_row0 = g->row0;
    g->alt_cursor_col = g->cursor_col;
    g->alt_cursor_row = g->cursor_row;
    g->alt_saved_cursor_col = g->saved_cursor_col;
    g->alt_saved_cursor_row = g->saved_cursor_row;
    g->alt_scroll_top = g->scroll_top;
    g->alt_scroll_bottom = g->scroll_bottom;
    g->cells = rudo_calloc(g->cols * g->rows, sizeof(*g->cells));
    g->capacity_rows = g->rows;
    g->row0 = 0;
    g->total_rows = g->rows;
    g->history_rows = 0;
    g->view_offset = 0;
    g->cursor_col = 0;
    g->cursor_row = 0;
    g->saved_cursor_col = 0;
    g->saved_cursor_row = 0;
    g->scroll_top = 0;
    g->scroll_bottom = g->rows ? g->rows - 1u : 0;
    g->alternate_active = true;
    for (i = 0; i < g->cols * g->rows; ++i) cell_reset(&g->cells[i], theme);
}

static void grid_leave_alt(rudo_grid *g) {
    if (!g->alternate_active || !g->alt_cells) return;
    free(g->cells);
    g->cells = g->alt_cells;
    g->total_rows = g->alt_total_rows;
    g->history_rows = g->alt_history_rows;
    g->view_offset = g->alt_view_offset;
    g->capacity_rows = g->alt_capacity_rows;
    g->row0 = g->alt_row0;
    g->cursor_col = g->alt_cursor_col;
    g->cursor_row = g->alt_cursor_row;
    g->saved_cursor_col = g->alt_saved_cursor_col;
    g->saved_cursor_row = g->alt_saved_cursor_row;
    g->scroll_top = g->alt_scroll_top;
    g->scroll_bottom = g->alt_scroll_bottom;
    g->alt_cells = NULL;
    g->alt_total_rows = g->alt_history_rows = g->alt_view_offset = 0;
    g->alt_capacity_rows = g->alt_row0 = 0;
    g->alternate_active = false;
}

static void grid_erase_scrollback(rudo_grid *g) {
    rudo_cell *cells;
    size_t r, c;
    if (!g) return;
    g->view_offset = 0;
    if (g->alternate_active || !g->history_rows) return;
    cells = rudo_malloc(g->cols * g->rows * sizeof(*cells));
    for (r = 0; r < g->rows; ++r) {
        for (c = 0; c < g->cols; ++c) cells[r * g->cols + c] = *rudo_grid_cell(g, r, c);
    }
    free(g->cells);
    g->cells = cells;
    g->capacity_rows = g->rows;
    g->row0 = 0;
    g->total_rows = g->rows;
    g->history_rows = 0;
}

static void parser_handle_sgr(rudo_terminal_parser *p, const char *params) {
    char buf[256], *save = NULL, *tok;
    unsigned vals[RUDO_MAX_CSI_PARAMS];
    size_t count = 0, i = 0;
    if (!params || !*params) { parser_reset_attrs(p); return; }
    strncpy(buf, params, sizeof(buf) - 1u); buf[sizeof(buf) - 1u] = 0;
    for (tok = strtok_r(buf, ";:", &save); tok && count < RUDO_MAX_CSI_PARAMS; tok = strtok_r(NULL, ";:", &save)) vals[count++] = parse_uint_param(tok, 0u, false);
    if (!count) { parser_reset_attrs(p); return; }
    while (i < count) {
        unsigned code = vals[i++];
        switch (code) {
            case 0: parser_reset_attrs(p); break;
            case 1: p->attr_flags |= RUDO_CELL_BOLD; break;
            case 2: p->attr_flags |= RUDO_CELL_DIM; break;
            case 3: p->attr_flags |= RUDO_CELL_ITALIC; break;
            case 4: p->attr_flags |= RUDO_CELL_UNDERLINE; break;
            case 5: case 6: p->attr_flags |= RUDO_CELL_BLINK; break;
            case 7: p->attr_flags |= RUDO_CELL_REVERSE; break;
            case 8: p->attr_flags |= RUDO_CELL_HIDDEN; break;
            case 9: p->attr_flags |= RUDO_CELL_STRIKETHROUGH; break;
            case 21: p->attr_flags &= ~RUDO_CELL_BOLD; break;
            case 22: p->attr_flags &= ~(RUDO_CELL_BOLD | RUDO_CELL_DIM); break;
            case 23: p->attr_flags &= ~RUDO_CELL_ITALIC; break;
            case 24: p->attr_flags &= ~RUDO_CELL_UNDERLINE; break;
            case 25: p->attr_flags &= ~RUDO_CELL_BLINK; break;
            case 27: p->attr_flags &= ~RUDO_CELL_REVERSE; break;
            case 28: p->attr_flags &= ~RUDO_CELL_HIDDEN; break;
            case 29: p->attr_flags &= ~RUDO_CELL_STRIKETHROUGH; break;
            case 30 ... 37: p->attr_fg = theme_palette_color(&p->theme, code - 30u); p->attr_fg_default = false; break;
            case 38:
                if (i + 1u < count && vals[i] == 5u) { p->attr_fg = theme_palette_color(&p->theme, vals[i + 1u]); p->attr_fg_default = false; i += 2u; }
                else if (i + 3u < count && vals[i] == 2u) { p->attr_fg = ((vals[i + 1u] & 255u) << 16) | ((vals[i + 2u] & 255u) << 8) | (vals[i + 3u] & 255u); p->attr_fg_default = false; i += 4u; }
                break;
            case 39: p->attr_fg = pack_rgb(p->theme.foreground); p->attr_fg_default = true; break;
            case 40 ... 47: p->attr_bg = theme_palette_color(&p->theme, code - 40u); p->attr_bg_default = false; break;
            case 48:
                if (i + 1u < count && vals[i] == 5u) { p->attr_bg = theme_palette_color(&p->theme, vals[i + 1u]); p->attr_bg_default = false; i += 2u; }
                else if (i + 3u < count && vals[i] == 2u) { p->attr_bg = ((vals[i + 1u] & 255u) << 16) | ((vals[i + 2u] & 255u) << 8) | (vals[i + 3u] & 255u); p->attr_bg_default = false; i += 4u; }
                break;
            case 49: p->attr_bg = pack_rgb(p->theme.background); p->attr_bg_default = true; break;
            case 90 ... 97: p->attr_fg = theme_palette_color(&p->theme, code - 90u + 8u); p->attr_fg_default = false; break;
            case 100 ... 107: p->attr_bg = theme_palette_color(&p->theme, code - 100u + 8u); p->attr_bg_default = false; break;
            default: break;
        }
    }
}

static void parser_handle_osc(rudo_terminal_parser *p, bool bell_terminated) {
    char buf[4096], *save = NULL, *cmd, *param;
    char reply[256], colorbuf[32];
    strncpy(buf, p->osc_buf, sizeof(buf) - 1u); buf[sizeof(buf) - 1u] = 0;
    cmd = strtok_r(buf, ";", &save);
    if (!cmd) return;
    if (!strcmp(cmd, "0") || !strcmp(cmd, "1") || !strcmp(cmd, "2")) {
        param = save ? save : "";
        parser_set_title(p, param);
        return;
    }
    if (!strcmp(cmd, "4")) {
        while ((param = strtok_r(NULL, ";", &save)) != NULL) {
            char *color = strtok_r(NULL, ";", &save);
            unsigned idx; uint32_t packed;
            if (!color) break;
            idx = parse_uint_param(param, 0u, false);
            if (!strcmp(color, "?")) {
                format_osc_color(theme_palette_color(&p->theme, idx), colorbuf);
                snprintf(reply, sizeof(reply), "\033]4;%u;%s%s", idx, colorbuf, bell_terminated ? "\a" : "\033\\");
                responses_push(&p->responses, reply);
            } else if (idx < 16u && parse_osc_color(color, &packed)) {
                theme_set_palette_color(&p->theme, idx, packed);
                p->theme_changed = true;
            }
        }
        parser_sync_default_attrs(p);
        return;
    }
    if (!strcmp(cmd, "10") || !strcmp(cmd, "11") || !strcmp(cmd, "12")) {
        unsigned code = (unsigned)parse_uint_param(cmd, 10u, false);
        while ((param = strtok_r(NULL, ";", &save)) != NULL) {
            uint32_t packed;
            if (!strcmp(param, "?")) {
                uint32_t color = code == 10u ? pack_rgb(p->theme.foreground) : (code == 11u ? pack_rgb(p->theme.background) : pack_rgb(p->theme.cursor));
                format_osc_color(color, colorbuf);
                snprintf(reply, sizeof(reply), "\033]%u;%s%s", code, colorbuf, bell_terminated ? "\a" : "\033\\");
                responses_push(&p->responses, reply);
            } else if (parse_osc_color(param, &packed)) {
                if (code == 10u) { p->theme.foreground[0] = color_r(packed); p->theme.foreground[1] = color_g(packed); p->theme.foreground[2] = color_b(packed); }
                else if (code == 11u) { p->theme.background[0] = color_r(packed); p->theme.background[1] = color_g(packed); p->theme.background[2] = color_b(packed); }
                else if (code == 12u) { p->theme.cursor[0] = color_r(packed); p->theme.cursor[1] = color_g(packed); p->theme.cursor[2] = color_b(packed); }
                p->theme_changed = true;
                parser_sync_default_attrs(p);
            }
            ++code;
        }
        return;
    }
    if (!strcmp(cmd, "104")) {
        bool any = false, had_param = false;
        while ((param = strtok_r(NULL, ";", &save)) != NULL) {
            unsigned idx = parse_uint_param(param, 0u, false);
            had_param = true;
            if (idx < 16u) { p->theme.ansi[idx] = p->base_theme.ansi[idx]; any = true; }
        }
        if (!had_param) {
            size_t i; for (i = 0; i < 16u; ++i) p->theme.ansi[i] = p->base_theme.ansi[i]; any = true;
        }
        if (any) p->theme_changed = true;
        return;
    }
    if (!strcmp(cmd, "110")) { if (save && *save) return; memcpy(p->theme.foreground, p->base_theme.foreground, sizeof(p->theme.foreground)); p->theme_changed = true; parser_sync_default_attrs(p); return; }
    if (!strcmp(cmd, "111")) { if (save && *save) return; memcpy(p->theme.background, p->base_theme.background, sizeof(p->theme.background)); p->theme_changed = true; parser_sync_default_attrs(p); return; }
    if (!strcmp(cmd, "112")) { if (save && *save) return; memcpy(p->theme.cursor, p->base_theme.cursor, sizeof(p->theme.cursor)); p->theme_changed = true; return; }
}

static void parser_handle_dcs(rudo_terminal_parser *p) {
    if (!p || !p->dcs_buf[0]) return;
    if (strcmp(p->dcs_buf, "=1s") == 0) p->synchronized_output = true;
    else if (strcmp(p->dcs_buf, "=2s") == 0) p->synchronized_output = false;
}

static void parser_handle_csi(rudo_terminal_parser *p, rudo_grid *g, rudo_damage_tracker *d) {
    char final = p->esc_len ? p->esc_buf[p->esc_len - 1u] : 0;
    char content[256], *params;
    rudo_cell blank;
    if (!final) return;
    memcpy(content, p->esc_buf, p->esc_len - 1u); content[p->esc_len - 1u] = 0;
    params = content;
    grid_blank_cell(p, &blank);
    switch (final) {
        case 'A': { size_t n = parse_uint_param(params, 1u, true); size_t min = parser_row_min(p, g); g->cursor_row = g->cursor_row > n ? g->cursor_row - n : min; if (g->cursor_row < min) g->cursor_row = min; } break;
        case 'B': { size_t n = parse_uint_param(params, 1u, true); size_t max = parser_row_max(p, g); g->cursor_row = RUDO_MIN(g->cursor_row + n, max); } break;
        case 'C': { size_t n = parse_uint_param(params, 1u, true); g->cursor_col = RUDO_MIN(g->cursor_col + n, g->cols ? g->cols - 1u : 0u); } break;
        case 'D': { size_t n = parse_uint_param(params, 1u, true); g->cursor_col = g->cursor_col > n ? g->cursor_col - n : 0u; } break;
        case 'E': { size_t n = parse_uint_param(params, 1u, true); size_t max = parser_row_max(p, g); g->cursor_row = RUDO_MIN(g->cursor_row + n, max); g->cursor_col = 0; } break;
        case 'F': { size_t n = parse_uint_param(params, 1u, true); size_t min = parser_row_min(p, g); g->cursor_row = g->cursor_row > n ? g->cursor_row - n : min; if (g->cursor_row < min) g->cursor_row = min; g->cursor_col = 0; } break;
        case 'G': { size_t n = parse_uint_param(params, 1u, true); g->cursor_col = RUDO_MIN(n ? n - 1u : 0u, g->cols ? g->cols - 1u : 0u); } break;
        case 'H': case 'f': {
            char *semi = strchr(params, ';');
            size_t row, col = 1u;
            if (semi) *semi++ = 0;
            row = parse_uint_param(params, 1u, true);
            if (semi) col = parse_uint_param(semi, 1u, true);
            g->cursor_row = parser_absolute_row(p, g, row ? row - 1u : 0u);
            g->cursor_col = RUDO_MIN(col ? col - 1u : 0u, g->cols ? g->cols - 1u : 0u);
        } break;
        case 'J': { unsigned mode = parse_uint_param(params, 0u, false); size_t r; if (mode == 0u) { grid_erase_line_range(g, g->cursor_row, g->cursor_col, g->cols ? g->cols - 1u : 0u, &blank); for (r = g->cursor_row + 1u; r < g->rows; ++r) clear_row_with_bg(g, visible_row_index(g, r), blank.bg, blank.bg_default, &p->theme); } else if (mode == 1u) { for (r = 0; r < g->cursor_row; ++r) clear_row_with_bg(g, visible_row_index(g, r), blank.bg, blank.bg_default, &p->theme); grid_erase_line_range(g, g->cursor_row, 0, g->cursor_col, &blank); } else if (mode == 2u) { for (r = 0; r < g->rows; ++r) clear_row_with_bg(g, visible_row_index(g, r), blank.bg, blank.bg_default, &p->theme); } else if (mode == 3u) grid_erase_scrollback(g); rudo_damage_mark_all(d); } break;
        case 'K': { unsigned mode = parse_uint_param(params, 0u, false); if (mode == 0u) grid_erase_line_range(g, g->cursor_row, g->cursor_col, g->cols ? g->cols - 1u : 0u, &blank); else if (mode == 1u) grid_erase_line_range(g, g->cursor_row, 0, g->cursor_col, &blank); else if (mode == 2u) grid_erase_line_range(g, g->cursor_row, 0, g->cols ? g->cols - 1u : 0u, &blank); rudo_damage_mark_row(d, g->cursor_row); } break;
        case 'L': { size_t n = parse_uint_param(params, 1u, true); grid_insert_lines(g, n, &blank); parser_mark_scroll_damage(d, g->cursor_row, g->scroll_bottom); } break;
        case 'M': { size_t n = parse_uint_param(params, 1u, true); grid_delete_lines(g, n, &blank); parser_mark_scroll_damage(d, g->cursor_row, g->scroll_bottom); } break;
        case 'P': { size_t n = parse_uint_param(params, 1u, true); grid_delete_chars(g, n, &blank); rudo_damage_mark_row(d, g->cursor_row); } break;
        case '@': { size_t n = parse_uint_param(params, 1u, true); grid_insert_chars(g, n, &blank); rudo_damage_mark_row(d, g->cursor_row); } break;
        case 'X': { size_t n = parse_uint_param(params, 1u, true); grid_erase_chars(g, n, &blank); rudo_damage_mark_row(d, g->cursor_row); } break;
        case 'S': { size_t n = parse_uint_param(params, 1u, true); grid_scroll_up_region(g, g->scroll_top, g->scroll_bottom, n, &blank); parser_mark_scroll_damage(d, g->scroll_top, g->scroll_bottom); } break;
        case 'T': { size_t n = parse_uint_param(params, 1u, true); grid_scroll_down_region(g, g->scroll_top, g->scroll_bottom, n, &blank); parser_mark_scroll_damage(d, g->scroll_top, g->scroll_bottom); } break;
        case 'd': { size_t n = parse_uint_param(params, 1u, true); g->cursor_row = parser_absolute_row(p, g, n ? n - 1u : 0u); } break;
        case 'm': parser_handle_sgr(p, params); break;
        case 'n': { unsigned mode = parse_uint_param(params, 0u, false); char reply[64]; if (mode == 5u) responses_push(&p->responses, "\033[0n"); else if (mode == 6u) { size_t row = g->cursor_row + 1u, col = clamp_col(g, g->cursor_col) + 1u; if (p->origin_mode) row = g->cursor_row - g->scroll_top + 1u; snprintf(reply, sizeof(reply), "\033[%zu;%zuR", row, col); responses_push(&p->responses, reply); } } break;
        case 'r': if (!p->csi_private) { char *semi = strchr(params, ';'); size_t top, bottom = g->rows; if (semi) *semi++ = 0; top = parse_uint_param(params, 1u, true); if (semi) bottom = parse_uint_param(semi, (unsigned)g->rows, true); grid_set_scroll_region(g, top ? top - 1u : 0u, bottom ? bottom - 1u : 0u); } break;
        case 's': if (!p->csi_private) grid_save_cursor(g); break;
        case 'u': if (!p->csi_private) grid_restore_cursor(g); break;
        case 'c': if (!p->csi_private && parse_uint_param(params, 0u, false) == 0u) responses_push(&p->responses, "\033[?62;c"); break;
        case 't': break;
        case 'q': if (p->csi_space) { p->cursor_shape_request = (int)parse_uint_param(params, 0u, false); p->cursor_shape_pending = true; } break;
        case 'p':
            if (p->csi_private && p->csi_dollar) {
                unsigned mode = parse_uint_param(params, 0u, false), status = 0u; char reply[64];
                switch (mode) {
                    case 1: status = p->application_cursor_keys ? 1u : 2u; break;
                    case 6: status = p->origin_mode ? 1u : 2u; break;
                    case 25: status = g->cursor_visible ? 1u : 2u; break;
                    case 47: case 1047: case 1049: status = g->alternate_active ? 1u : 2u; break;
                    case 1000: status = p->mouse.protocol == RUDO_MOUSE_PROTOCOL_VT200 ? 1u : 2u; break;
                    case 1002: status = p->mouse.protocol == RUDO_MOUSE_PROTOCOL_BUTTON_EVENT ? 1u : 2u; break;
                    case 1003: status = p->mouse.protocol == RUDO_MOUSE_PROTOCOL_ANY_EVENT ? 1u : 2u; break;
                    case 1004: status = p->focus_reporting ? 1u : 2u; break;
                    case 1006: status = p->mouse.encoding == RUDO_MOUSE_ENCODING_SGR ? 1u : 2u; break;
                    case 2004: status = p->bracketed_paste ? 1u : 2u; break;
                    case 2026: status = p->synchronized_output ? 1u : 2u; break;
                    default: status = 0u; break;
                }
                snprintf(reply, sizeof(reply), "\033[?%u;%u$y", mode, status); responses_push(&p->responses, reply);
            } else if (p->csi_bang) {
                p->application_cursor_keys = false; p->origin_mode = false; p->bracketed_paste = false; p->focus_reporting = false; p->synchronized_output = false; p->insert_mode = false; p->mouse.protocol = RUDO_MOUSE_PROTOCOL_NONE; p->mouse.encoding = RUDO_MOUSE_ENCODING_X10; g->cursor_visible = true; g->scroll_top = 0; g->scroll_bottom = g->rows ? g->rows - 1u : 0; p->g0_charset = p->g1_charset = RUDO_CHARSET_ASCII; p->active_charset = 0; parser_reset_attrs(p);
            }
            break;
        case 'h': case 'l': {
            bool enable = final == 'h'; char listbuf[256], *save = NULL, *tok;
            strncpy(listbuf, params, sizeof(listbuf) - 1u); listbuf[sizeof(listbuf) - 1u] = 0;
            for (tok = strtok_r(listbuf, ";", &save); tok; tok = strtok_r(NULL, ";", &save)) {
                unsigned mode = parse_uint_param(tok, 0u, false);
                if (p->csi_private) {
                    switch (mode) {
                        case 1: p->application_cursor_keys = enable; break;
                        case 6: p->origin_mode = enable; g->cursor_col = 0; g->cursor_row = enable ? g->scroll_top : 0; break;
                        case 25: g->cursor_visible = enable; break;
                        case 47: case 1047: if (enable) grid_enter_alt(g, &p->theme); else grid_leave_alt(g); rudo_damage_mark_all(d); break;
                        case 1049: if (enable) { grid_save_cursor(g); grid_enter_alt(g, &p->theme); } else { grid_leave_alt(g); grid_restore_cursor(g); } rudo_damage_mark_all(d); break;
                        case 1048: if (enable) grid_save_cursor(g); else grid_restore_cursor(g); break;
                        case 1000: p->mouse.protocol = enable ? RUDO_MOUSE_PROTOCOL_VT200 : RUDO_MOUSE_PROTOCOL_NONE; break;
                        case 1002: p->mouse.protocol = enable ? RUDO_MOUSE_PROTOCOL_BUTTON_EVENT : RUDO_MOUSE_PROTOCOL_NONE; break;
                        case 1003: p->mouse.protocol = enable ? RUDO_MOUSE_PROTOCOL_ANY_EVENT : RUDO_MOUSE_PROTOCOL_NONE; break;
                        case 1004: p->focus_reporting = enable; break;
                        case 1006: p->mouse.encoding = enable ? RUDO_MOUSE_ENCODING_SGR : RUDO_MOUSE_ENCODING_X10; break;
                        case 2004: p->bracketed_paste = enable; break;
                        case 2026: p->synchronized_output = enable; break;
                        default: break;
                    }
                } else if (mode == 4u) p->insert_mode = enable;
            }
        } break;
        default: break;
    }
}

static void parser_execute(rudo_terminal_parser *p, rudo_grid *g, rudo_damage_tracker *d, uint8_t ch) {
    switch (ch) {
        case 0x07: break;
        case 0x08: if (g->cursor_col) g->cursor_col--; break;
        case 0x09: { size_t next = ((g->cursor_col / RUDO_TAB_WIDTH) + 1u) * RUDO_TAB_WIDTH; g->cursor_col = RUDO_MIN(next, g->cols ? g->cols - 1u : 0u); } break;
        case 0x0a: case 0x0b: case 0x0c: { size_t old = g->cursor_row; rudo_cell blank; grid_blank_cell(p, &blank); if (g->cursor_row == g->scroll_bottom) { grid_scroll_up_region(g, g->scroll_top, g->scroll_bottom, 1u, &blank); parser_mark_scroll_damage(d, g->scroll_top, g->scroll_bottom); } else if (g->cursor_row + 1u < g->rows) g->cursor_row++; if (old != g->cursor_row) rudo_damage_mark_row(d, g->cursor_row); } break;
        case 0x0d: g->cursor_col = 0; break;
        case 0x0e: p->active_charset = 1; break;
        case 0x0f: p->active_charset = 0; break;
        default: break;
    }
}

static void parser_feed_byte(rudo_terminal_parser *p, rudo_grid *g, rudo_damage_tracker *d, uint8_t ch) {
    switch (p->state) {
        case RUDO_PSTATE_GROUND:
            if (ch == 0x1b) { p->state = RUDO_PSTATE_ESC; p->esc_len = 0; return; }
            if (ch < 0x20u || ch == 0x7fu) { parser_execute(p, g, d, ch); return; }
            if (p->utf8_need) {
                if ((ch & 0xc0u) == 0x80u) {
                    p->utf8_codepoint = (p->utf8_codepoint << 6) | (ch & 0x3fu);
                    if (++p->utf8_have == p->utf8_need) {
                        uint32_t cp = p->utf8_codepoint;
                        p->utf8_need = p->utf8_have = 0; p->utf8_codepoint = 0;
                        parser_write_cell(p, g, d, cp);
                    }
                    return;
                }
                p->utf8_need = p->utf8_have = 0; p->utf8_codepoint = 0;
            }
            if (ch < 0x80u) parser_write_cell(p, g, d, ch);
            else if ((ch & 0xe0u) == 0xc0u) { p->utf8_need = 1; p->utf8_have = 0; p->utf8_codepoint = ch & 0x1fu; }
            else if ((ch & 0xf0u) == 0xe0u) { p->utf8_need = 2; p->utf8_have = 0; p->utf8_codepoint = ch & 0x0fu; }
            else if ((ch & 0xf8u) == 0xf0u) { p->utf8_need = 3; p->utf8_have = 0; p->utf8_codepoint = ch & 0x07u; }
            else parser_write_cell(p, g, d, 0xfffdu);
            return;
        case RUDO_PSTATE_ESC:
            if (p->esc_len == 1 && (p->esc_buf[0] == '(' || p->esc_buf[0] == ')')) {
                if (p->esc_buf[0] == '(') p->g0_charset = ch == '0' ? RUDO_CHARSET_DEC_SPECIAL : RUDO_CHARSET_ASCII;
                else p->g1_charset = ch == '0' ? RUDO_CHARSET_DEC_SPECIAL : RUDO_CHARSET_ASCII;
                p->state = RUDO_PSTATE_GROUND; p->esc_len = 0; return;
            }
            if (ch == '[') { p->state = RUDO_PSTATE_CSI; p->esc_len = 0; p->csi_private = p->csi_space = p->csi_dollar = p->csi_bang = false; return; }
            if (ch == ']') { p->state = RUDO_PSTATE_OSC; p->osc_len = 0; return; }
            if (ch == 'P') { p->state = RUDO_PSTATE_DCS; p->dcs_len = 0; return; }
            if (ch == '(' || ch == ')') { p->esc_buf[0] = (char)ch; p->esc_len = 1; return; }
            if (ch == '7') grid_save_cursor(g);
            else if (ch == '8') grid_restore_cursor(g);
            else if (ch == 'M') { rudo_cell blank; grid_blank_cell(p, &blank); if (g->cursor_row == g->scroll_top) { grid_reverse_index(g, &blank); parser_mark_scroll_damage(d, g->scroll_top, g->scroll_bottom); } else grid_reverse_index(g, &blank); }
            else if (ch == 'D') { rudo_cell blank; grid_blank_cell(p, &blank); if (g->cursor_row == g->scroll_bottom) { grid_scroll_up_region(g, g->scroll_top, g->scroll_bottom, 1u, &blank); parser_mark_scroll_damage(d, g->scroll_top, g->scroll_bottom); } else if (g->cursor_row + 1u < g->rows) g->cursor_row++; }
            p->state = RUDO_PSTATE_GROUND; p->esc_len = 0; return;
        case RUDO_PSTATE_CSI:
            if (ch >= 0x40u && ch <= 0x7eu) {
                if (p->esc_len + 1u < sizeof(p->esc_buf)) p->esc_buf[p->esc_len++] = (char)ch;
                parser_handle_csi(p, g, d);
                p->state = RUDO_PSTATE_GROUND; p->esc_len = 0; p->csi_private = p->csi_space = p->csi_dollar = p->csi_bang = false;
                return;
            }
            if (ch == '?') p->csi_private = true;
            else if (ch == ' ') p->csi_space = true;
            else if (ch == '$') p->csi_dollar = true;
            else if (ch == '!') p->csi_bang = true;
            if (p->esc_len + 1u < sizeof(p->esc_buf)) p->esc_buf[p->esc_len++] = (char)ch;
            return;
        case RUDO_PSTATE_OSC:
            if (ch == 0x07u) { p->osc_buf[p->osc_len] = 0; parser_handle_osc(p, true); p->osc_len = 0; p->state = RUDO_PSTATE_GROUND; return; }
            if (ch == 0x1bu) { p->state = RUDO_PSTATE_OSC_ESC; return; }
            if (p->osc_len + 1u < sizeof(p->osc_buf)) p->osc_buf[p->osc_len++] = (char)ch;
            return;
        case RUDO_PSTATE_OSC_ESC:
            if (ch == '\\') { p->osc_buf[p->osc_len] = 0; parser_handle_osc(p, false); p->osc_len = 0; p->state = RUDO_PSTATE_GROUND; }
            else { if (p->osc_len + 2u < sizeof(p->osc_buf)) { p->osc_buf[p->osc_len++] = 0x1b; p->osc_buf[p->osc_len++] = (char)ch; } p->state = RUDO_PSTATE_OSC; }
            return;
        case RUDO_PSTATE_DCS:
            if (ch == 0x1bu) { p->state = RUDO_PSTATE_DCS_ESC; return; }
            if (p->dcs_len + 1u < sizeof(p->dcs_buf)) p->dcs_buf[p->dcs_len++] = (char)ch;
            return;
        case RUDO_PSTATE_DCS_ESC:
            if (ch == '\\') { p->dcs_buf[p->dcs_len] = 0; parser_handle_dcs(p); p->dcs_len = 0; p->state = RUDO_PSTATE_GROUND; }
            else { if (p->dcs_len + 2u < sizeof(p->dcs_buf)) { p->dcs_buf[p->dcs_len++] = 0x1b; p->dcs_buf[p->dcs_len++] = (char)ch; } p->state = RUDO_PSTATE_DCS; }
            return;
    }
}

void rudo_terminal_parser_advance(rudo_terminal_parser *p, rudo_grid *g, rudo_damage_tracker *d, const uint8_t *data, size_t len) { size_t i; for (i = 0; i < len; ++i) parser_feed_byte(p, g, d, data[i]); }
const rudo_theme *rudo_terminal_parser_theme(const rudo_terminal_parser *parser) { return &parser->theme; }
bool rudo_terminal_parser_take_theme_changed(rudo_terminal_parser *parser) { bool changed = parser->theme_changed; parser->theme_changed = false; return changed; }
int rudo_terminal_parser_take_cursor_shape_request(rudo_terminal_parser *parser, bool *has_request) { int v = parser->cursor_shape_request; if (has_request) *has_request = parser->cursor_shape_pending; parser->cursor_shape_pending = false; parser->cursor_shape_request = -1; return v; }
const char *rudo_terminal_parser_title(const rudo_terminal_parser *parser) { return parser->title_set ? parser->title : NULL; }
const rudo_mouse_state *rudo_terminal_parser_mouse_state(const rudo_terminal_parser *parser) { return &parser->mouse; }
bool rudo_terminal_parser_bracketed_paste(const rudo_terminal_parser *parser) { return parser->bracketed_paste; }
bool rudo_terminal_parser_application_cursor_keys(const rudo_terminal_parser *parser) { return parser->application_cursor_keys; }
bool rudo_terminal_parser_focus_reporting(const rudo_terminal_parser *parser) { return parser->focus_reporting; }
bool rudo_terminal_parser_synchronized_output(const rudo_terminal_parser *parser) { return parser->synchronized_output; }
size_t rudo_terminal_parser_take_responses(rudo_terminal_parser *parser, char ***responses_out) { size_t n = parser->responses.len; if (responses_out) *responses_out = parser->responses.items; parser->responses.items = NULL; parser->responses.len = parser->responses.cap = 0; return n; }
bool rudo_mouse_state_is_active(const rudo_mouse_state *state) { return state && state->protocol != RUDO_MOUSE_PROTOCOL_NONE; }
int rudo_mouse_button_code(rudo_mouse_button button) { switch (button.kind) { case RUDO_MOUSE_BUTTON_LEFT: return 0; case RUDO_MOUSE_BUTTON_MIDDLE: return 1; case RUDO_MOUSE_BUTTON_RIGHT: return 2; default: return -1; } }
uint8_t rudo_mouse_modifier_bits(rudo_modifiers modifiers) { return (modifiers.shift ? 4u : 0u) | (modifiers.alt ? 8u : 0u) | (modifiers.ctrl ? 16u : 0u); }
static size_t mouse_encode_common(const rudo_mouse_state *state, uint8_t code, uint16_t col, uint16_t row, char out[64], bool release) { if (!state || state->protocol == RUDO_MOUSE_PROTOCOL_NONE) return 0; if (state->encoding == RUDO_MOUSE_ENCODING_SGR) return (size_t)snprintf(out, 64, "\033[<%u;%u;%u%c", code, (unsigned)(col + 1u), (unsigned)(row + 1u), release ? 'm' : 'M'); if (col > 222 || row > 222) return 0; out[0]='\033'; out[1]='['; out[2]='M'; out[3]=(char)(32 + code); out[4]=(char)(33 + col); out[5]=(char)(33 + row); return 6; }
size_t rudo_mouse_encode_press(const rudo_mouse_state *state, uint8_t button_code, uint8_t modifiers, uint16_t col, uint16_t row, char out[64]) { return mouse_encode_common(state, (uint8_t)(button_code | modifiers), col, row, out, false); }
size_t rudo_mouse_encode_release(const rudo_mouse_state *state, uint8_t button_code, uint8_t modifiers, uint16_t col, uint16_t row, char out[64]) { if (!state || state->protocol == RUDO_MOUSE_PROTOCOL_NONE) return 0; if (state->encoding == RUDO_MOUSE_ENCODING_SGR) return mouse_encode_common(state, (uint8_t)(button_code | modifiers), col, row, out, true); return mouse_encode_common(state, (uint8_t)(3u | modifiers), col, row, out, false); }
size_t rudo_mouse_encode_drag(const rudo_mouse_state *state, uint8_t button_code, uint8_t modifiers, uint16_t col, uint16_t row, char out[64]) { if (!state || (state->protocol != RUDO_MOUSE_PROTOCOL_BUTTON_EVENT && state->protocol != RUDO_MOUSE_PROTOCOL_ANY_EVENT)) return 0; return mouse_encode_common(state, (uint8_t)(32u | button_code | modifiers), col, row, out, false); }
size_t rudo_mouse_encode_move(const rudo_mouse_state *state, uint8_t modifiers, uint16_t col, uint16_t row, char out[64]) { if (!state || state->protocol != RUDO_MOUSE_PROTOCOL_ANY_EVENT) return 0; return mouse_encode_common(state, (uint8_t)(35u | modifiers), col, row, out, false); }
size_t rudo_mouse_encode_scroll(const rudo_mouse_state *state, uint8_t modifiers, bool up, uint16_t col, uint16_t row, char out[64]) { return mouse_encode_common(state, (uint8_t)((up ? 64u : 65u) | modifiers), col, row, out, false); }
