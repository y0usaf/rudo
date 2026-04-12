#include "rudo/terminal.h"

#include <assert.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define CELL_BOLD (1u << 0)

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
    test_grid_alt_screen_and_scrollback_guard();
    test_unicode_selection_extraction();
    test_multiline_selection_extracts_newline();
    test_parser_defaults();
    test_parser_runtime_mode_toggling_via_escape_sequences();
    test_alt_screen_preserves_and_restores_primary_state();
    test_mouse_encoding_edge_cases();
    return 0;
}
