#include "rudo/cli.h"
#include "rudo/core.h"
#include "rudo/render.h"
#include "rudo/wayland.h"

#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    rudo_cli_args cli;
    rudo_core_app *app;
    rudo_wayland_app *wl;
    rudo_software_renderer *renderer;
    const rudo_config *cfg;
    const rudo_theme *theme;
    rudo_theme_colors colors;
    int exit_code = 0;

    if (!rudo_cli_parse(&cli, argc, argv, &exit_code)) {
        return exit_code < 0 ? 0 : exit_code;
    }

    app = rudo_core_app_new(&cli);
    rudo_cli_args_destroy(&cli);
    if (!app) {
        return 1;
    }

    cfg = rudo_core_app_config(app);
    theme = rudo_core_app_theme(app);
    colors.foreground[0] = theme->foreground[0]; colors.foreground[1] = theme->foreground[1]; colors.foreground[2] = theme->foreground[2];
    colors.background[0] = theme->background[0]; colors.background[1] = theme->background[1]; colors.background[2] = theme->background[2];
    colors.cursor[0] = theme->cursor[0]; colors.cursor[1] = theme->cursor[1]; colors.cursor[2] = theme->cursor[2];
    colors.selection[0] = theme->selection[0]; colors.selection[1] = theme->selection[1]; colors.selection[2] = theme->selection[2];

    renderer = rudo_software_renderer_new(cfg->font.size, cfg->font.family, &colors, cfg->window.padding);
    if (!renderer) {
        rudo_core_app_free(app);
        return 1;
    }

    wl = rudo_wayland_app_new(app, renderer);
    if (!wl) {
        rudo_software_renderer_free(renderer);
        rudo_core_app_free(app);
        fprintf(stderr, "failed to initialize Wayland frontend\n");
        return 1;
    }

    exit_code = rudo_wayland_app_run(wl);
    rudo_wayland_app_free(wl);
    rudo_software_renderer_free(renderer);
    rudo_core_app_free(app);
    return exit_code;
}
