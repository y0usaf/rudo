#include "rudo/common.h"
#include "rudo/core.h"
#include "rudo/input.h"

#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static void sleep_ms(long ms) {
    struct timespec ts;
    ts.tv_sec = ms / 1000;
    ts.tv_nsec = (ms % 1000) * 1000000L;
    nanosleep(&ts, NULL);
}

static void pump_app(rudo_core_app *app, int iterations, long sleep_between_ms) {
    int i;
    for (i = 0; i < iterations; ++i) {
        rudo_core_app_tick(app);
        if (rudo_core_app_pty_exited(app)) break;
        if (sleep_between_ms > 0) sleep_ms(sleep_between_ms);
    }
}

static void test_path_join_helpers(void) {
    char *joined = rudo_path_join3("/tmp", "rudo", "state");
    assert(strcmp(joined, "/tmp/rudo/state") == 0);
    free(joined);
    assert(strcmp(rudo_basename_const("/a/b/c.txt"), "c.txt") == 0);
}

static void test_mouse_sgr_encoding(void) {
    rudo_mouse_state state = { .protocol = RUDO_MOUSE_PROTOCOL_VT200, .encoding = RUDO_MOUSE_ENCODING_SGR };
    char buf[64];
    size_t n = rudo_mouse_encode_press(&state, 0, 4, 9, 2, buf);
    assert(n > 0);
    assert(strcmp(buf, "\033[<4;10;3M") == 0);
    n = rudo_mouse_encode_release(&state, 0, 4, 9, 2, buf);
    assert(strcmp(buf, "\033[<4;10;3m") == 0);
}

static void test_pty_spawn_and_exit_status(void) {
    rudo_pty_spawn_config cfg;
    char *argv[] = { "/bin/sh", "-c", "printf ok; exit 7", NULL };
    rudo_pty *pty;
    char buf[32];
    size_t used = 0;
    int status = 0;
    int tries;

    memset(&cfg, 0, sizeof(cfg));
    cfg.command = argv;
    cfg.command_len = 3;
    cfg.shell_fallback = "/bin/sh";

    pty = rudo_pty_spawn(80, 24, &cfg);
    assert(pty != NULL);

    for (tries = 0; tries < 200 && used < 2; ++tries) {
        ssize_t n = rudo_pty_try_read(pty, buf + used, sizeof(buf) - 1 - used);
        if (n > 0) used += (size_t)n;
        else sleep_ms(10);
    }
    buf[used] = '\0';
    assert(strstr(buf, "ok") != NULL);

    for (tries = 0; tries < 200 && !rudo_pty_take_exit_status(pty, &status); ++tries) sleep_ms(10);
    assert(rudo_pty_take_exit_status(pty, &status));
    assert(WIFEXITED(status));
    assert(WEXITSTATUS(status) == 7);

    rudo_pty_free(pty);
}

static void test_pty_exec_failure_exit_status(void) {
    rudo_pty_spawn_config cfg;
    char *argv[] = { "/definitely/not/a/real/command", NULL };
    rudo_pty *pty;
    int status = 0;
    int tries;

    memset(&cfg, 0, sizeof(cfg));
    cfg.command = argv;
    cfg.command_len = 1;
    cfg.shell_fallback = "/bin/sh";

    pty = rudo_pty_spawn(80, 24, &cfg);
    assert(pty != NULL);

    for (tries = 0; tries < 200 && !rudo_pty_take_exit_status(pty, &status); ++tries) sleep_ms(10);
    assert(rudo_pty_take_exit_status(pty, &status));
    assert(WIFEXITED(status));
    assert(WEXITSTATUS(status) == 127);
    assert(rudo_exit_code_from_wait_status(status) == 127);

    rudo_pty_free(pty);
}

static void test_wait_status_exit_code_mapping(void) {
    assert(rudo_exit_code_from_wait_status(7 << 8) == 7);
    assert(rudo_exit_code_from_wait_status(SIGTERM) == 128 + SIGTERM);
}

static void test_translucent_full_redraw_overwrites_old_buffer_contents(void) {
    rudo_theme_colors theme = {{0xd4, 0xd4, 0xd4}, {0x1e, 0x1e, 0x1e}, {0xff, 0xff, 0xff}, {0x26, 0x4f, 0x78}};
    rudo_render_cell cell = { .col = 0, .row = 0, .width = 1, .flags = 0, .ch = ' ', .fg = 0, .bg = 0, .fg_default = 1, .bg_default = 1 };
    rudo_render_grid grid;
    rudo_render_options options = { .full_redraw = true, .draw_cursor = false, .dirty_rows = NULL, .dirty_row_count = 0 };
    rudo_software_renderer *renderer;
    rudo_framebuffer a, b;
    uint32_t width, height;
    size_t size;

    memset(&grid, 0, sizeof(grid));
    grid.cells = &cell;
    grid.cell_count = 1;
    grid.cols = 1;
    grid.rows = 1;

    renderer = rudo_software_renderer_new(12.0f, NULL, &theme, 0);
    assert(renderer != NULL);
    rudo_software_renderer_set_background_alpha(renderer, 128);
    rudo_software_renderer_window_size_for_grid(renderer, 1, 1, &width, &height);
    rudo_software_renderer_grid_layout(renderer, width, height, NULL, NULL);

    size = (size_t)width * height * 4u;
    a.width = b.width = width;
    a.height = b.height = height;
    a.stride = b.stride = width * 4u;
    a.pixels = malloc(size);
    b.pixels = malloc(size);
    assert(a.pixels != NULL && b.pixels != NULL);
    memset(a.pixels, 0x00, size);
    memset(b.pixels, 0xff, size);

    rudo_software_renderer_render(renderer, &a, &grid, NULL, options);
    rudo_software_renderer_render(renderer, &b, &grid, NULL, options);
    assert(memcmp(a.pixels, b.pixels, size) == 0);

    free(a.pixels);
    free(b.pixels);
    rudo_software_renderer_free(renderer);
}

static void test_runtime_mode_encoding_helpers(void) {
    rudo_mouse_state none = { .protocol = RUDO_MOUSE_PROTOCOL_NONE, .encoding = RUDO_MOUSE_ENCODING_X10 };
    rudo_mouse_state vt200 = { .protocol = RUDO_MOUSE_PROTOCOL_VT200, .encoding = RUDO_MOUSE_ENCODING_X10 };
    rudo_mouse_state button = { .protocol = RUDO_MOUSE_PROTOCOL_BUTTON_EVENT, .encoding = RUDO_MOUSE_ENCODING_SGR };
    rudo_mouse_state any = { .protocol = RUDO_MOUSE_PROTOCOL_ANY_EVENT, .encoding = RUDO_MOUSE_ENCODING_SGR };
    char buf[64];

    assert(!rudo_mouse_state_is_active(&none));
    assert(rudo_mouse_state_is_active(&vt200));
    assert(rudo_mouse_state_is_active(&button));
    assert(rudo_mouse_state_is_active(&any));

    assert(rudo_mouse_encode_press(&none, 0, 0, 1, 1, buf) == 0);
    assert(rudo_mouse_encode_press(&vt200, 0, 0, 1, 1, buf) == 6);
    assert(buf[0] == '\033' && buf[1] == '[' && buf[2] == 'M');
    assert((unsigned char)buf[3] == 32);
    assert((unsigned char)buf[4] == 34);
    assert((unsigned char)buf[5] == 34);
    assert(rudo_mouse_encode_drag(&button, 1, 4, 2, 3, buf) > 0);
    assert(strcmp(buf, "\033[<37;3;4M") == 0);
    assert(rudo_mouse_encode_move(&any, 8, 4, 5, buf) > 0);
    assert(strcmp(buf, "\033[<43;5;6M") == 0);
}

static void test_sync_updates_resume_after_end_via_pty(void) {
    rudo_cli_args cli;
    char *argv[] = {
        "/bin/sh", "-c",
        "printf '\033[?2026hSYNC\033[?2026l'; sleep 0.05",
        NULL
    };
    rudo_core_app *app;
    rudo_tick_result tick;
    const rudo_grid *grid;

    memset(&cli, 0, sizeof(cli));
    cli.command = argv;
    cli.command_len = 3;

    app = rudo_core_app_new(&cli);
    rudo_core_app_init_terminal(app, 20, 5);
    pump_app(app, 20, 10);

    tick = rudo_core_app_tick(app);
    assert(tick.redraw_requested);
    grid = rudo_core_app_grid(app);
    assert(rudo_grid_cell(grid, 0, 0)->ch == 'S');
    assert(rudo_grid_cell(grid, 0, 3)->ch == 'C');

    rudo_core_app_free(app);
}

static void test_sync_updates_watchdog_allows_redraw_via_pty(void) {
    rudo_cli_args cli;
    char *argv[] = {
        "/bin/sh", "-c",
        "printf '\033[?2026hHANG'; sleep 0.30",
        NULL
    };
    rudo_core_app *app;
    rudo_tick_result tick;
    bool saw_suppressed = false;
    bool saw_watchdog_redraw = false;
    int i;

    memset(&cli, 0, sizeof(cli));
    cli.command = argv;
    cli.command_len = 3;

    app = rudo_core_app_new(&cli);
    rudo_core_app_init_terminal(app, 20, 5);

    for (i = 0; i < 10; ++i) {
        tick = rudo_core_app_tick(app);
        if (!tick.redraw_requested) saw_suppressed = true;
        sleep_ms(10);
    }

    for (; i < 50; ++i) {
        tick = rudo_core_app_tick(app);
        if (tick.redraw_requested) {
            saw_watchdog_redraw = true;
            break;
        }
        sleep_ms(10);
    }

    assert(saw_suppressed);
    assert(saw_watchdog_redraw);
    rudo_core_app_free(app);
}

int main(void) {
    test_path_join_helpers();
    test_mouse_sgr_encoding();
    test_pty_spawn_and_exit_status();
    test_pty_exec_failure_exit_status();
    test_wait_status_exit_code_mapping();
    test_translucent_full_redraw_overwrites_old_buffer_contents();
    test_runtime_mode_encoding_helpers();
    test_sync_updates_resume_after_end_via_pty();
    test_sync_updates_watchdog_allows_redraw_via_pty();
    return 0;
}
