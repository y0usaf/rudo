#ifndef RUDO_TERMINAL_H
#define RUDO_TERMINAL_H

#include "rudo/common.h"
#include "rudo/render.h"
#include "rudo/config.h"
#include "rudo/input.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    size_t col;
    size_t row;
} rudo_grid_point;

typedef enum {
    RUDO_SELECTION_NONE = 0,
    RUDO_SELECTION_SELECTING,
    RUDO_SELECTION_SELECTED,
} rudo_selection_state;

typedef struct {
    uint32_t ch;
    uint32_t fg;
    uint32_t bg;
    uint32_t flags;
    bool fg_default;
    bool bg_default;
} rudo_cell;

typedef struct {
    rudo_cell *cells;
    size_t cols;
    size_t rows;
    size_t total_rows;
    size_t history_rows;
    size_t view_offset;
    size_t scrollback_limit;
    size_t capacity_rows;
    size_t row0;
    size_t cursor_col;
    size_t cursor_row;
    size_t saved_cursor_col;
    size_t saved_cursor_row;
    size_t scroll_top;
    size_t scroll_bottom;
    bool cursor_visible;
    bool alternate_active;
    rudo_cell *alt_cells;
    size_t alt_total_rows;
    size_t alt_history_rows;
    size_t alt_view_offset;
    size_t alt_capacity_rows;
    size_t alt_row0;
    size_t alt_cursor_col;
    size_t alt_cursor_row;
    size_t alt_saved_cursor_col;
    size_t alt_saved_cursor_row;
    size_t alt_scroll_top;
    size_t alt_scroll_bottom;
} rudo_grid;

typedef struct {
    uint64_t *bits;
    size_t rows;
    bool full_damage;
} rudo_damage_tracker;

typedef struct {
    rudo_selection_state state;
    rudo_grid_point start;
    rudo_grid_point end;
} rudo_selection;

typedef struct {
    uint8_t foreground[3];
    uint8_t background[3];
    uint8_t cursor[3];
    uint8_t selection[3];
    uint32_t ansi[16];
} rudo_theme;

typedef enum {
    RUDO_MOUSE_PROTOCOL_NONE = 0,
    RUDO_MOUSE_PROTOCOL_X10,
    RUDO_MOUSE_PROTOCOL_VT200,
    RUDO_MOUSE_PROTOCOL_BUTTON_EVENT,
    RUDO_MOUSE_PROTOCOL_ANY_EVENT,
} rudo_mouse_protocol;

typedef enum {
    RUDO_MOUSE_ENCODING_X10 = 0,
    RUDO_MOUSE_ENCODING_SGR,
} rudo_mouse_encoding;

typedef struct {
    rudo_mouse_protocol protocol;
    rudo_mouse_encoding encoding;
} rudo_mouse_state;

typedef struct {
    char **items;
    size_t len;
    size_t cap;
} rudo_response_list;

typedef struct {
    rudo_theme theme;
    rudo_theme base_theme;
    rudo_mouse_state mouse;
    rudo_response_list responses;
    char *title;
    bool title_set;
    bool bracketed_paste;
    bool application_cursor_keys;
    bool focus_reporting;
    bool synchronized_output;
    bool theme_changed;
    bool origin_mode;
    bool insert_mode;
    bool csi_private;
    bool csi_space;
    bool csi_dollar;
    bool csi_bang;
    bool cursor_visible;
    bool scroll_region_active;
    unsigned state;
    unsigned utf8_need;
    unsigned utf8_have;
    uint32_t utf8_codepoint;
    uint32_t attr_fg;
    uint32_t attr_bg;
    uint32_t attr_flags;
    bool attr_fg_default;
    bool attr_bg_default;
    unsigned g0_charset;
    unsigned g1_charset;
    unsigned active_charset;
    int cursor_shape_request;
    bool cursor_shape_pending;
    uint8_t utf8_bytes[4];
    char esc_buf[256];
    size_t esc_len;
    char osc_buf[4096];
    size_t osc_len;
    char dcs_buf[4096];
    size_t dcs_len;
} rudo_terminal_parser;

void rudo_theme_init_default(rudo_theme *theme);
bool rudo_theme_from_config_colors(rudo_theme *theme, const rudo_color_config *colors);
bool rudo_theme_load(rudo_theme *theme);

void rudo_grid_init(rudo_grid *grid, size_t cols, size_t rows, size_t scrollback_lines);
void rudo_grid_destroy(rudo_grid *grid);
void rudo_grid_reset(rudo_grid *grid, size_t cols, size_t rows, size_t scrollback_lines);
void rudo_grid_resize(rudo_grid *grid, size_t cols, size_t rows);
size_t rudo_grid_cols(const rudo_grid *grid);
size_t rudo_grid_rows(const rudo_grid *grid);
void rudo_grid_linefeed(rudo_grid *grid);
void rudo_grid_put_codepoint(rudo_grid *grid, uint32_t ch, const rudo_theme *theme);
void rudo_grid_set_cursor(rudo_grid *grid, size_t col, size_t row);
void rudo_grid_cursor_position(const rudo_grid *grid, size_t *col, size_t *row);
bool rudo_grid_scroll_view_up(rudo_grid *grid, size_t lines);
bool rudo_grid_scroll_view_down(rudo_grid *grid, size_t lines);
bool rudo_grid_is_viewing_scrollback(const rudo_grid *grid);
void rudo_grid_reset_view(rudo_grid *grid);
void rudo_grid_clear(rudo_grid *grid, const rudo_theme *theme);
const rudo_cell *rudo_grid_cell(const rudo_grid *grid, size_t row, size_t col);
rudo_cell *rudo_grid_cell_mut(rudo_grid *grid, size_t row, size_t col);
void rudo_grid_build_render_cells(const rudo_grid *grid, rudo_render_cell *out);

void rudo_damage_init(rudo_damage_tracker *damage, size_t rows);
void rudo_damage_destroy(rudo_damage_tracker *damage);
void rudo_damage_resize(rudo_damage_tracker *damage, size_t rows);
void rudo_damage_clear(rudo_damage_tracker *damage);
void rudo_damage_mark_all(rudo_damage_tracker *damage);
void rudo_damage_mark_row(rudo_damage_tracker *damage, size_t row);
void rudo_damage_mark_rows(rudo_damage_tracker *damage, size_t start_row, size_t end_row);
bool rudo_damage_has_damage(const rudo_damage_tracker *damage);
bool rudo_damage_is_full(const rudo_damage_tracker *damage);
size_t rudo_damage_collect_row_ranges(const rudo_damage_tracker *damage, rudo_render_row_range *out, size_t cap);

void rudo_selection_init(rudo_selection *sel);
void rudo_selection_clear(rudo_selection *sel);
void rudo_selection_start(rudo_selection *sel, size_t col, size_t row);
void rudo_selection_update(rudo_selection *sel, size_t col, size_t row);
void rudo_selection_finish(rudo_selection *sel);
bool rudo_selection_has_selection(const rudo_selection *sel);
void rudo_selection_snapshot(const rudo_selection *sel, rudo_selection_state *state, rudo_grid_point *start, rudo_grid_point *end);
char *rudo_selection_selected_text(const rudo_selection *sel, const rudo_grid *grid);

void rudo_terminal_parser_init(rudo_terminal_parser *parser, const rudo_theme *theme);
void rudo_terminal_parser_destroy(rudo_terminal_parser *parser);
void rudo_terminal_parser_reset(rudo_terminal_parser *parser, const rudo_theme *theme);
void rudo_terminal_parser_advance(rudo_terminal_parser *parser, rudo_grid *grid, rudo_damage_tracker *damage, const uint8_t *data, size_t len);
const rudo_theme *rudo_terminal_parser_theme(const rudo_terminal_parser *parser);
bool rudo_terminal_parser_take_theme_changed(rudo_terminal_parser *parser);
int rudo_terminal_parser_take_cursor_shape_request(rudo_terminal_parser *parser, bool *has_request);
const char *rudo_terminal_parser_title(const rudo_terminal_parser *parser);
const rudo_mouse_state *rudo_terminal_parser_mouse_state(const rudo_terminal_parser *parser);
bool rudo_terminal_parser_bracketed_paste(const rudo_terminal_parser *parser);
bool rudo_terminal_parser_application_cursor_keys(const rudo_terminal_parser *parser);
bool rudo_terminal_parser_focus_reporting(const rudo_terminal_parser *parser);
bool rudo_terminal_parser_synchronized_output(const rudo_terminal_parser *parser);
size_t rudo_terminal_parser_take_responses(rudo_terminal_parser *parser, char ***responses_out);

bool rudo_mouse_state_is_active(const rudo_mouse_state *state);
int rudo_mouse_button_code(rudo_mouse_button button);
uint8_t rudo_mouse_modifier_bits(rudo_modifiers modifiers);
size_t rudo_mouse_encode_press(const rudo_mouse_state *state, uint8_t button_code, uint8_t modifiers, uint16_t col, uint16_t row, char out[64]);
size_t rudo_mouse_encode_release(const rudo_mouse_state *state, uint8_t button_code, uint8_t modifiers, uint16_t col, uint16_t row, char out[64]);
size_t rudo_mouse_encode_drag(const rudo_mouse_state *state, uint8_t button_code, uint8_t modifiers, uint16_t col, uint16_t row, char out[64]);
size_t rudo_mouse_encode_move(const rudo_mouse_state *state, uint8_t modifiers, uint16_t col, uint16_t row, char out[64]);
size_t rudo_mouse_encode_scroll(const rudo_mouse_state *state, uint8_t modifiers, bool up, uint16_t col, uint16_t row, char out[64]);

#ifdef __cplusplus
}
#endif

#endif
