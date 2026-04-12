#ifndef RUDO_WAYLAND_H
#define RUDO_WAYLAND_H

#include "rudo/cli.h"
#include "rudo/core.h"
#include "rudo/render.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct rudo_wayland_app rudo_wayland_app;

rudo_wayland_app *rudo_wayland_app_new(rudo_core_app *app, rudo_software_renderer *renderer);
void rudo_wayland_app_free(rudo_wayland_app *wl);
int rudo_wayland_app_run(rudo_wayland_app *wl);

#ifdef __cplusplus
}
#endif

#endif
