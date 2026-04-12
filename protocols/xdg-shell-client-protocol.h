#include <wayland-client-core.h>
#include <wayland-client-protocol.h>
#include <stdint.h>
struct xdg_wm_base; struct xdg_surface; struct xdg_toplevel;
extern const struct wl_interface xdg_wm_base_interface, xdg_surface_interface, xdg_toplevel_interface;
struct xdg_wm_base_listener { void (*ping)(void *, struct xdg_wm_base *, uint32_t); };
struct xdg_surface_listener { void (*configure)(void *, struct xdg_surface *, uint32_t); };
struct xdg_toplevel_listener { void (*configure)(void *, struct xdg_toplevel *, int32_t, int32_t, struct wl_array *); void (*close)(void *, struct xdg_toplevel *); void (*configure_bounds)(void *, struct xdg_toplevel *, int32_t, int32_t); void (*wm_capabilities)(void *, struct xdg_toplevel *, struct wl_array *); };
#define XDG_TOPLEVEL_STATE_MAXIMIZED 1
#define XDG_TOPLEVEL_STATE_FULLSCREEN 2
#define XDG_TOPLEVEL_STATE_RESIZING 3
#define XDG_TOPLEVEL_STATE_ACTIVATED 4
static inline struct xdg_surface *xdg_wm_base_get_xdg_surface(struct xdg_wm_base *o, struct wl_surface *s) { return (struct xdg_surface *)wl_proxy_marshal_flags((struct wl_proxy*)o, 2, &xdg_surface_interface, wl_proxy_get_version((struct wl_proxy*)o), 0, NULL, s); }
static inline void xdg_wm_base_pong(struct xdg_wm_base *o, uint32_t serial) { wl_proxy_marshal_flags((struct wl_proxy*)o, 3, NULL, wl_proxy_get_version((struct wl_proxy*)o), 0, serial); }
static inline int xdg_wm_base_add_listener(struct xdg_wm_base *o, const struct xdg_wm_base_listener *l, void *d) { return wl_proxy_add_listener((struct wl_proxy*)o, (void (**)(void))l, d); }
static inline void xdg_wm_base_destroy(struct xdg_wm_base *o) { wl_proxy_marshal_flags((struct wl_proxy*)o, 0, NULL, wl_proxy_get_version((struct wl_proxy*)o), WL_MARSHAL_FLAG_DESTROY); }
static inline struct xdg_toplevel *xdg_surface_get_toplevel(struct xdg_surface *o) { return (struct xdg_toplevel *)wl_proxy_marshal_flags((struct wl_proxy*)o, 1, &xdg_toplevel_interface, wl_proxy_get_version((struct wl_proxy*)o), 0, NULL); }
static inline void xdg_surface_ack_configure(struct xdg_surface *o, uint32_t serial) { wl_proxy_marshal_flags((struct wl_proxy*)o, 4, NULL, wl_proxy_get_version((struct wl_proxy*)o), 0, serial); }
static inline int xdg_surface_add_listener(struct xdg_surface *o, const struct xdg_surface_listener *l, void *d) { return wl_proxy_add_listener((struct wl_proxy*)o, (void (**)(void))l, d); }
static inline void xdg_surface_destroy(struct xdg_surface *o) { wl_proxy_marshal_flags((struct wl_proxy*)o, 0, NULL, wl_proxy_get_version((struct wl_proxy*)o), WL_MARSHAL_FLAG_DESTROY); }
static inline void xdg_toplevel_set_title(struct xdg_toplevel *o, const char *s) { wl_proxy_marshal_flags((struct wl_proxy*)o, 2, NULL, wl_proxy_get_version((struct wl_proxy*)o), 0, s); }
static inline void xdg_toplevel_set_app_id(struct xdg_toplevel *o, const char *s) { wl_proxy_marshal_flags((struct wl_proxy*)o, 3, NULL, wl_proxy_get_version((struct wl_proxy*)o), 0, s); }
static inline int xdg_toplevel_add_listener(struct xdg_toplevel *o, const struct xdg_toplevel_listener *l, void *d) { return wl_proxy_add_listener((struct wl_proxy*)o, (void (**)(void))l, d); }
static inline void xdg_toplevel_destroy(struct xdg_toplevel *o) { wl_proxy_marshal_flags((struct wl_proxy*)o, 0, NULL, wl_proxy_get_version((struct wl_proxy*)o), WL_MARSHAL_FLAG_DESTROY); }
