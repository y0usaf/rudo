#include "rudo/terminal.h"

#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define CELL_BOLD (1u << 0)
#define CELL_DIM (1u << 1)
#define CELL_ITALIC (1u << 2)
#define CELL_UNDERLINE (1u << 3)
#define CELL_STRIKETHROUGH (1u << 4)
#define CELL_REVERSE (1u << 6)
#define CELL_HIDDEN (1u << 7)
#define CELL_WIDE_SPACER (1u << 9)

static void feed(rudo_terminal_parser *parser, rudo_grid *grid, rudo_damage_tracker *damage, const char *s) {
    rudo_terminal_parser_advance(parser, grid, damage, (const uint8_t *)s, strlen(s));
}

static void test_sgr_reset_preserves_existing_cells(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;
    const rudo_cell *a, *b, *c;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 8, 3, 16);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "\033[31;1mA\033[0mB");

    a = rudo_grid_cell(&grid, 0, 0);
    b = rudo_grid_cell(&grid, 0, 1);
    c = rudo_grid_cell(&grid, 0, 2);

    assert(a->ch == 'A');
    assert((a->flags & CELL_BOLD) != 0);
    assert(!a->fg_default);
    assert(b->ch == 'B');
    assert((b->flags & CELL_BOLD) == 0);
    assert(b->fg_default);
    assert(c->ch == ' ');

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_cursor_movement_and_edits(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 6, 4, 8);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "abcde");
    feed(&parser, &grid, &damage, "\r\033[3C!");
    assert(rudo_grid_cell(&grid, 0, 3)->ch == '!');

    feed(&parser, &grid, &damage, "\r\033[2C\033[P");
    assert(rudo_grid_cell(&grid, 0, 0)->ch == 'a');
    assert(rudo_grid_cell(&grid, 0, 1)->ch == 'b');
    assert(rudo_grid_cell(&grid, 0, 2)->ch == '!');

    feed(&parser, &grid, &damage, "\r\033[1C\033[@");
    assert(rudo_grid_cell(&grid, 0, 1)->ch == ' ');
    assert(rudo_grid_cell(&grid, 0, 2)->ch == 'b');

    feed(&parser, &grid, &damage, "\r\033[3C\033[X");
    assert(rudo_grid_cell(&grid, 0, 3)->ch == ' ');

    feed(&parser, &grid, &damage, "\r\033[2K");
    assert(rudo_grid_cell(&grid, 0, 0)->ch == ' ' || rudo_grid_cell(&grid, 0, 0)->ch == 0);
    assert(rudo_grid_cell(&grid, 0, 5)->ch == ' ');

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_cursor_position_with_explicit_row_and_col(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 6, 4, 8);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "\033[4;3H!");
    assert(rudo_grid_cell(&grid, 3, 2)->ch == '!');
    assert(rudo_grid_cell(&grid, 0, 2)->ch == ' ');

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_grid_alt_screen_and_scrollback_guard(void) {
    rudo_theme theme;
    rudo_grid grid;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 5, 4, 16);

    rudo_grid_put_codepoint(&grid, '1', &theme);
    rudo_grid_linefeed(&grid);
    rudo_grid_put_codepoint(&grid, '2', &theme);
    rudo_grid_linefeed(&grid);
    rudo_grid_put_codepoint(&grid, '3', &theme);
    rudo_grid_linefeed(&grid);
    rudo_grid_put_codepoint(&grid, '4', &theme);
    rudo_grid_linefeed(&grid);
    rudo_grid_put_codepoint(&grid, '5', &theme);

    assert(rudo_grid_scroll_view_up(&grid, 1));
    assert(rudo_grid_is_viewing_scrollback(&grid));

    grid.alternate_active = true;
    assert(!rudo_grid_scroll_view_up(&grid, 1));
    grid.alternate_active = false;

    assert(rudo_grid_cell(&grid, 0, 0)->ch != ' ');
    rudo_grid_destroy(&grid);
}

static void test_unicode_selection_extraction(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;
    rudo_selection sel;
    char *text;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 6, 2, 4);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);
    rudo_selection_init(&sel);

    feed(&parser, &grid, &damage, "A\xE4\xB8\xAD" "B");
    rudo_selection_start(&sel, 0, 0);
    rudo_selection_update(&sel, 3, 0);
    rudo_selection_finish(&sel);
    text = rudo_selection_selected_text(&sel, &grid);
    assert(strcmp(text, "A中B") == 0);
    free(text);

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_multiline_selection_extracts_newline(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_selection sel;
    char *text;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 6, 3, 4);

    rudo_grid_put_codepoint(&grid, 'A', &theme);
    rudo_grid_put_codepoint(&grid, 'B', &theme);
    rudo_grid_put_codepoint(&grid, 'C', &theme);
    rudo_grid_set_cursor(&grid, 0, 1);
    rudo_grid_put_codepoint(&grid, 'D', &theme);
    rudo_grid_put_codepoint(&grid, 'E', &theme);

    rudo_selection_init(&sel);
    rudo_selection_start(&sel, 0, 0);
    rudo_selection_update(&sel, 1, 1);
    rudo_selection_finish(&sel);
    text = rudo_selection_selected_text(&sel, &grid);
    assert(strcmp(text, "ABC   \nDE") == 0);
    free(text);

    rudo_grid_destroy(&grid);
}

static void test_render_cells_translate_terminal_flags(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_render_cell cells[2];
    rudo_cell *cell;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 2, 1, 0);

    cell = rudo_grid_cell_mut(&grid, 0, 0);
    cell->ch = 'A';
    cell->flags = CELL_BOLD | CELL_DIM | CELL_ITALIC | CELL_UNDERLINE | CELL_STRIKETHROUGH | CELL_REVERSE | CELL_HIDDEN;
    cell = rudo_grid_cell_mut(&grid, 0, 1);
    cell->flags = CELL_WIDE_SPACER;

    rudo_grid_build_render_cells(&grid, cells);

    assert((cells[0].flags & RUDO_CELL_FLAG_BOLD) != 0);
    assert((cells[0].flags & RUDO_CELL_FLAG_DIM) != 0);
    assert((cells[0].flags & RUDO_CELL_FLAG_ITALIC) != 0);
    assert((cells[0].flags & RUDO_CELL_FLAG_UNDERLINE) != 0);
    assert((cells[0].flags & RUDO_CELL_FLAG_STRIKETHROUGH) != 0);
    assert((cells[0].flags & RUDO_CELL_FLAG_REVERSE) != 0);
    assert((cells[0].flags & RUDO_CELL_FLAG_HIDDEN) != 0);
    assert((cells[0].flags & RUDO_CELL_FLAG_WIDE_SPACER) == 0);
    assert((cells[1].flags & RUDO_CELL_FLAG_WIDE_SPACER) != 0);

    rudo_grid_destroy(&grid);
}

static void test_damage_full_and_row_range_collection(void) {
    rudo_damage_tracker damage;
    rudo_render_row_range ranges[4];
    size_t count;

    rudo_damage_init(&damage, 130);
    assert(rudo_damage_is_full(&damage));
    assert(rudo_damage_has_damage(&damage));

    count = rudo_damage_collect_row_ranges(&damage, ranges, 4);
    assert(count == 1);
    assert(ranges[0].start_row == 0);
    assert(ranges[0].end_row_inclusive == 129);

    rudo_damage_clear(&damage);
    assert(!rudo_damage_is_full(&damage));
    assert(!rudo_damage_has_damage(&damage));
    assert(rudo_damage_collect_row_ranges(&damage, ranges, 4) == 0);

    rudo_damage_mark_row(&damage, 1);
    rudo_damage_mark_rows(&damage, 63, 66);
    rudo_damage_mark_rows(&damage, 68, 129);
    assert(!rudo_damage_is_full(&damage));
    assert(rudo_damage_has_damage(&damage));

    count = rudo_damage_collect_row_ranges(&damage, ranges, 4);
    assert(count == 3);
    assert(ranges[0].start_row == 1 && ranges[0].end_row_inclusive == 1);
    assert(ranges[1].start_row == 63 && ranges[1].end_row_inclusive == 66);
    assert(ranges[2].start_row == 68 && ranges[2].end_row_inclusive == 129);

    count = rudo_damage_collect_row_ranges(&damage, ranges, 2);
    assert(count == 3);
    assert(ranges[0].start_row == 1 && ranges[0].end_row_inclusive == 1);
    assert(ranges[1].start_row == 63 && ranges[1].end_row_inclusive == 66);

    rudo_damage_mark_all(&damage);
    assert(rudo_damage_is_full(&damage));
    count = rudo_damage_collect_row_ranges(&damage, ranges, 1);
    assert(count == 1);
    assert(ranges[0].start_row == 0 && ranges[0].end_row_inclusive == 129);

    rudo_damage_destroy(&damage);
}

static void test_parser_defaults(void) {
    rudo_theme theme;
    rudo_terminal_parser parser;
    char out[64];

    rudo_theme_init_default(&theme);
    rudo_terminal_parser_init(&parser, &theme);

    assert(!rudo_terminal_parser_bracketed_paste(&parser));
    assert(!rudo_terminal_parser_application_cursor_keys(&parser));
    assert(!rudo_terminal_parser_focus_reporting(&parser));
    assert(!rudo_terminal_parser_synchronized_output(&parser));
    assert(!rudo_mouse_state_is_active(rudo_terminal_parser_mouse_state(&parser)));
    assert(rudo_terminal_parser_mouse_state(&parser)->protocol == RUDO_MOUSE_PROTOCOL_NONE);
    assert(rudo_terminal_parser_mouse_state(&parser)->encoding == RUDO_MOUSE_ENCODING_X10);
    assert(rudo_mouse_encode_press(rudo_terminal_parser_mouse_state(&parser), 0, 0, 0, 0, out) == 0);

    rudo_terminal_parser_destroy(&parser);
}

static void test_osc_104_selective_reset_does_not_reset_all(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;
    const rudo_theme *current;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 4, 2, 4);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "\033]4;1;rgb:11/22/33;2;rgb:44/55/66\007");
    current = rudo_terminal_parser_theme(&parser);
    assert(current->ansi[1] != theme.ansi[1]);
    assert(current->ansi[2] != theme.ansi[2]);

    feed(&parser, &grid, &damage, "\033]104;1\007");
    current = rudo_terminal_parser_theme(&parser);
    assert(current->ansi[1] == theme.ansi[1]);
    assert(current->ansi[2] != theme.ansi[2]);

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_osc_110_111_112_ignore_parameterized_forms(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;
    const rudo_theme *current;
    uint8_t foreground[3], background[3], cursor[3];

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 4, 2, 4);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "\033]10;rgb:11/22/33\007\033]11;rgb:44/55/66\007\033]12;rgb:77/88/99\007");
    current = rudo_terminal_parser_theme(&parser);
    memcpy(foreground, current->foreground, sizeof(foreground));
    memcpy(background, current->background, sizeof(background));
    memcpy(cursor, current->cursor, sizeof(cursor));

    feed(&parser, &grid, &damage, "\033]110;?\007\033]111;rgb:aa/bb/cc\007\033]112;bogus\007");
    current = rudo_terminal_parser_theme(&parser);
    assert(memcmp(current->foreground, foreground, sizeof(foreground)) == 0);
    assert(memcmp(current->background, background, sizeof(background)) == 0);
    assert(memcmp(current->cursor, cursor, sizeof(cursor)) == 0);

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_parser_runtime_mode_toggling_via_escape_sequences(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;
    char out[64];

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 8, 4, 8);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "\033[?2004h\033[?1h\033[?1004h\033[?2026h\033[?1003h\033[?1006h");
    assert(rudo_terminal_parser_bracketed_paste(&parser));
    assert(rudo_terminal_parser_application_cursor_keys(&parser));
    assert(rudo_terminal_parser_focus_reporting(&parser));
    assert(rudo_terminal_parser_synchronized_output(&parser));
    assert(rudo_terminal_parser_mouse_state(&parser)->protocol == RUDO_MOUSE_PROTOCOL_ANY_EVENT);
    assert(rudo_terminal_parser_mouse_state(&parser)->encoding == RUDO_MOUSE_ENCODING_SGR);
    assert(rudo_mouse_encode_move(rudo_terminal_parser_mouse_state(&parser), 8, 4, 5, out) > 0);
    assert(strcmp(out, "\033[<43;5;6M") == 0);

    feed(&parser, &grid, &damage, "\033[?1006l\033[?1003l\033[?2026l\033[?1004l\033[?1l\033[?2004l");
    assert(!rudo_terminal_parser_bracketed_paste(&parser));
    assert(!rudo_terminal_parser_application_cursor_keys(&parser));
    assert(!rudo_terminal_parser_focus_reporting(&parser));
    assert(!rudo_terminal_parser_synchronized_output(&parser));
    assert(rudo_terminal_parser_mouse_state(&parser)->protocol == RUDO_MOUSE_PROTOCOL_NONE);
    assert(rudo_terminal_parser_mouse_state(&parser)->encoding == RUDO_MOUSE_ENCODING_X10);
    assert(rudo_mouse_encode_press(rudo_terminal_parser_mouse_state(&parser), 0, 0, 1, 1, out) == 0);

    feed(&parser, &grid, &damage, "\033[?2004h\033[?1h\033[?1004h\033[?2026h\033[?1003h\033[?1006h");
    feed(&parser, &grid, &damage, "\033[!p");
    assert(!rudo_terminal_parser_bracketed_paste(&parser));
    assert(!rudo_terminal_parser_application_cursor_keys(&parser));
    assert(!rudo_terminal_parser_focus_reporting(&parser));
    assert(!rudo_terminal_parser_synchronized_output(&parser));
    assert(rudo_terminal_parser_mouse_state(&parser)->protocol == RUDO_MOUSE_PROTOCOL_NONE);
    assert(rudo_terminal_parser_mouse_state(&parser)->encoding == RUDO_MOUSE_ENCODING_X10);

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_alt_screen_preserves_and_restores_primary_state(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 6, 3, 16);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "MAIN");
    assert(rudo_grid_cell(&grid, 0, 0)->ch == 'M');
    assert(rudo_grid_cell(&grid, 0, 3)->ch == 'N');
    assert(grid.cursor_row == 0);
    assert(grid.cursor_col == 4);

    feed(&parser, &grid, &damage, "\033[?1049h");
    assert(grid.alternate_active);
    assert(rudo_grid_cell(&grid, 0, 0)->ch == ' ');
    assert(grid.cursor_row == 0);
    assert(grid.cursor_col == 0);

    feed(&parser, &grid, &damage, "ALT");
    assert(rudo_grid_cell(&grid, 0, 0)->ch == 'A');
    assert(rudo_grid_cell(&grid, 0, 2)->ch == 'T');

    feed(&parser, &grid, &damage, "\033[?1049l");
    assert(!grid.alternate_active);
    assert(rudo_grid_cell(&grid, 0, 0)->ch == 'M');
    assert(rudo_grid_cell(&grid, 0, 3)->ch == 'N');
    assert(grid.cursor_row == 0);
    assert(grid.cursor_col == 4);

    feed(&parser, &grid, &damage, "Z");
    assert(rudo_grid_cell(&grid, 0, 4)->ch == 'Z');

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_ed_3_clears_scrollback_only(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 4, 3, 16);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "1111\r\n2222\r\n3333\r\n4444\r\n5555");
    assert(grid.history_rows > 0);
    assert(rudo_grid_cell(&grid, 0, 0)->ch == '3');
    assert(rudo_grid_cell(&grid, 1, 0)->ch == '4');
    assert(rudo_grid_cell(&grid, 2, 0)->ch == '5');

    feed(&parser, &grid, &damage, "\033[3J");
    assert(grid.history_rows == 0);
    assert(grid.total_rows == grid.rows);
    assert(grid.view_offset == 0);
    assert(rudo_grid_cell(&grid, 0, 0)->ch == '3');
    assert(rudo_grid_cell(&grid, 1, 0)->ch == '4');
    assert(rudo_grid_cell(&grid, 2, 0)->ch == '5');
    assert(!rudo_grid_scroll_view_up(&grid, 1));

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_scrollback_retains_recent_history_at_limit(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 4, 3, 2);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "1\r\n2\r\n3\r\n4\r\n5\r\n6");
    assert(grid.history_rows == 2);
    assert(grid.total_rows == 5);
    assert(rudo_grid_cell(&grid, 0, 0)->ch == '4');
    assert(rudo_grid_cell(&grid, 1, 0)->ch == '5');
    assert(rudo_grid_cell(&grid, 2, 0)->ch == '6');
    assert(rudo_grid_scroll_view_up(&grid, 1));
    assert(rudo_grid_cell(&grid, 0, 0)->ch == '3');
    assert(rudo_grid_cell(&grid, 1, 0)->ch == '4');
    assert(rudo_grid_cell(&grid, 2, 0)->ch == '5');
    assert(rudo_grid_scroll_view_up(&grid, 1));
    assert(rudo_grid_cell(&grid, 0, 0)->ch == '2');
    assert(rudo_grid_cell(&grid, 1, 0)->ch == '3');
    assert(rudo_grid_cell(&grid, 2, 0)->ch == '4');
    assert(!rudo_grid_scroll_view_up(&grid, 1));
    rudo_grid_reset_view(&grid);
    assert(rudo_grid_cell(&grid, 0, 0)->ch == '4');
    assert(rudo_grid_cell(&grid, 1, 0)->ch == '5');
    assert(rudo_grid_cell(&grid, 2, 0)->ch == '6');

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_dcs_synchronized_update_toggling(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 8, 4, 8);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "\033P=1s\033\\");
    assert(rudo_terminal_parser_synchronized_output(&parser));

    feed(&parser, &grid, &damage, "\033P=2s\033\\");
    assert(!rudo_terminal_parser_synchronized_output(&parser));

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_prompt_repaint_inside_synchronized_update(void) {
    rudo_theme theme;
    rudo_grid grid;
    rudo_damage_tracker damage;
    rudo_terminal_parser parser;

    rudo_theme_init_default(&theme);
    rudo_grid_init(&grid, 12, 3, 8);
    rudo_damage_init(&damage, grid.rows);
    rudo_terminal_parser_init(&parser, &theme);

    feed(&parser, &grid, &damage, "OLD> ");
    feed(&parser, &grid, &damage, "\033[?2026h\r\033[2KNEW> \033[?2026l");

    assert(!rudo_terminal_parser_synchronized_output(&parser));
    assert(rudo_grid_cell(&grid, 0, 0)->ch == 'N');
    assert(rudo_grid_cell(&grid, 0, 1)->ch == 'E');
    assert(rudo_grid_cell(&grid, 0, 2)->ch == 'W');
    assert(rudo_grid_cell(&grid, 0, 3)->ch == '>');
    assert(rudo_grid_cell(&grid, 0, 4)->ch == ' ');

    rudo_terminal_parser_destroy(&parser);
    rudo_damage_destroy(&damage);
    rudo_grid_destroy(&grid);
}

static void test_mouse_encoding_edge_cases(void) {
    rudo_mouse_state sgr = { .protocol = RUDO_MOUSE_PROTOCOL_ANY_EVENT, .encoding = RUDO_MOUSE_ENCODING_SGR };
    rudo_mouse_state x10 = { .protocol = RUDO_MOUSE_PROTOCOL_VT200, .encoding = RUDO_MOUSE_ENCODING_X10 };
    char out[64];
    size_t n;

    n = rudo_mouse_encode_move(&sgr, 0, 299, 399, out);
    assert(n > 0);
    assert(strcmp(out, "\033[<35;300;400M") == 0);

    n = rudo_mouse_encode_press(&x10, 0, 0, 223, 0, out);
    assert(n == 0);

    n = rudo_mouse_encode_release(&x10, 0, 0, 10, 5, out);
    assert(n == 6);
    assert(out[0] == '\033' && out[1] == '[' && out[2] == 'M');
}

int main(void) {
    test_sgr_reset_preserves_existing_cells();
    test_cursor_movement_and_edits();
    test_cursor_position_with_explicit_row_and_col();
    test_grid_alt_screen_and_scrollback_guard();
    test_unicode_selection_extraction();
    test_multiline_selection_extracts_newline();
    test_render_cells_translate_terminal_flags();
    test_damage_full_and_row_range_collection();
    test_parser_defaults();
    test_osc_104_selective_reset_does_not_reset_all();
    test_osc_110_111_112_ignore_parameterized_forms();
    test_parser_runtime_mode_toggling_via_escape_sequences();
    test_alt_screen_preserves_and_restores_primary_state();
    test_ed_3_clears_scrollback_only();
    test_scrollback_retains_recent_history_at_limit();
    test_dcs_synchronized_update_toggling();
    test_prompt_repaint_inside_synchronized_update();
    test_mouse_encoding_edge_cases();
    return 0;
}
