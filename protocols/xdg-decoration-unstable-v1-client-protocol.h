#include <wayland-client-core.h>
#include <wayland-client-protocol.h>
#include <stdint.h>
struct zxdg_decoration_manager_v1; struct zxdg_toplevel_decoration_v1; struct xdg_toplevel;
extern const struct wl_interface zxdg_decoration_manager_v1_interface, zxdg_toplevel_decoration_v1_interface;
enum zxdg_toplevel_decoration_v1_mode { ZXDG_TOPLEVEL_DECORATION_V1_MODE_CLIENT_SIDE = 1, ZXDG_TOPLEVEL_DECORATION_V1_MODE_SERVER_SIDE = 2 };
static inline struct zxdg_toplevel_decoration_v1 *zxdg_decoration_manager_v1_get_toplevel_decoration(struct zxdg_decoration_manager_v1 *o, struct xdg_toplevel *tl) { return (struct zxdg_toplevel_decoration_v1 *)wl_proxy_marshal_flags((struct wl_proxy*)o, 1, &zxdg_toplevel_decoration_v1_interface, wl_proxy_get_version((struct wl_proxy*)o), 0, NULL, tl); }
static inline void zxdg_decoration_manager_v1_destroy(struct zxdg_decoration_manager_v1 *o) { wl_proxy_marshal_flags((struct wl_proxy*)o, 0, NULL, wl_proxy_get_version((struct wl_proxy*)o), WL_MARSHAL_FLAG_DESTROY); }
static inline void zxdg_toplevel_decoration_v1_set_mode(struct zxdg_toplevel_decoration_v1 *o, uint32_t mode) { wl_proxy_marshal_flags((struct wl_proxy*)o, 2, NULL, wl_proxy_get_version((struct wl_proxy*)o), 0, mode); }
static inline void zxdg_toplevel_decoration_v1_destroy(struct zxdg_toplevel_decoration_v1 *o) { wl_proxy_marshal_flags((struct wl_proxy*)o, 0, NULL, wl_proxy_get_version((struct wl_proxy*)o), WL_MARSHAL_FLAG_DESTROY); }
