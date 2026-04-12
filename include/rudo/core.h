#ifndef RUDO_CORE_H
#define RUDO_CORE_H

#include "rudo/cli.h"
#include "rudo/config.h"
#include "rudo/input.h"
#include "rudo/keybindings.h"
#include "rudo/log.h"
#include "rudo/render.h"
#include "rudo/terminal.h"

#include <time.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rudo_pty rudo_pty;
typedef struct rudo_core_app rudo_core_app;

typedef struct {
    const char *term;
    const char *colorterm;
    const char *shell_fallback;
    char **command;
    size_t command_len;
} rudo_pty_spawn_config;

typedef struct {
    bool redraw_requested;
    bool animating;
} rudo_tick_result;

rudo_pty *rudo_pty_spawn(uint16_t cols, uint16_t rows, const rudo_pty_spawn_config *config);
void rudo_pty_free(rudo_pty *pty);
ssize_t rudo_pty_try_read(rudo_pty *pty, void *buf, size_t len);
ssize_t rudo_pty_write(rudo_pty *pty, const void *buf, size_t len);
bool rudo_pty_resize(rudo_pty *pty, uint16_t cols, uint16_t rows);
int rudo_pty_master_fd(const rudo_pty *pty);
bool rudo_pty_child_exited(rudo_pty *pty);
bool rudo_pty_take_exit_status(rudo_pty *pty, int *status);

bool rudo_clipboard_set(const char *text);
char *rudo_clipboard_get(void);
bool rudo_primary_set(const char *text);
char *rudo_primary_get(void);

rudo_core_app *rudo_core_app_new(const rudo_cli_args *cli);
void rudo_core_app_free(rudo_core_app *app);

const rudo_config *rudo_core_app_config(const rudo_core_app *app);
const char *rudo_core_app_app_id(const rudo_core_app *app);
const char *rudo_core_app_title(const rudo_core_app *app);
const rudo_theme *rudo_core_app_theme(const rudo_core_app *app);
const rudo_grid *rudo_core_app_grid(const rudo_core_app *app);
const rudo_selection *rudo_core_app_selection(const rudo_core_app *app);
const rudo_damage_tracker *rudo_core_app_damage(const rudo_core_app *app);
rudo_damage_tracker *rudo_core_app_damage_mut(rudo_core_app *app);
const rudo_cursor_renderer *rudo_core_app_cursor_renderer(const rudo_core_app *app);
rudo_modifiers rudo_core_app_modifiers(const rudo_core_app *app);
bool rudo_core_app_pty_exited(const rudo_core_app *app);
bool rudo_core_app_poll_pty_exit(rudo_core_app *app);
bool rudo_core_app_take_pty_exit_status(rudo_core_app *app, int *status);
int rudo_core_app_pty_raw_fd(const rudo_core_app *app);
int rudo_exit_code_from_wait_status(int status);

void rudo_core_app_clear_damage(rudo_core_app *app);
void rudo_core_app_set_cell_size(rudo_core_app *app, float cell_w, float cell_h);
void rudo_core_app_set_grid_offset(rudo_core_app *app, float offset_x, float offset_y);
void rudo_core_app_set_modifiers(rudo_core_app *app, rudo_modifiers modifiers);
void rudo_core_app_init_terminal(rudo_core_app *app, size_t cols, size_t rows);
void rudo_core_app_handle_key_event(rudo_core_app *app, const rudo_key_event *event);
bool rudo_core_app_matches_local_keybinding(const rudo_core_app *app, rudo_local_action action, const rudo_key_event *event);
void rudo_core_app_handle_mouse_button(rudo_core_app *app, bool pressed, rudo_mouse_button button);
void rudo_core_app_handle_mouse_move(rudo_core_app *app, double x, double y);
void rudo_core_app_handle_scroll_lines(rudo_core_app *app, double lines);
void rudo_core_app_handle_focus_change(rudo_core_app *app, bool focused);
void rudo_core_app_handle_resize(rudo_core_app *app, size_t cols, size_t rows);
rudo_tick_result rudo_core_app_tick(rudo_core_app *app);
bool rudo_core_app_next_wakeup(const rudo_core_app *app, struct timespec *out_duration);
bool rudo_core_app_cursor_wakeup_due(const rudo_core_app *app);
bool rudo_core_app_take_title_changed(rudo_core_app *app);
bool rudo_core_app_take_theme_changed(rudo_core_app *app);
void rudo_core_app_build_render_grid(const rudo_core_app *app, rudo_render_grid *out, rudo_render_cell *cells, size_t cell_count);

#ifdef __cplusplus
}
#endif

#endif
