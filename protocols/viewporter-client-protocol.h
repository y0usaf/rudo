#include <wayland-client-core.h>
#include <wayland-client-protocol.h>
#include <stdint.h>
struct wp_viewporter; struct wp_viewport; struct wl_surface;
extern const struct wl_interface wp_viewporter_interface, wp_viewport_interface;
static inline struct wp_viewport *wp_viewporter_get_viewport(struct wp_viewporter *o, struct wl_surface *s) { return (struct wp_viewport *)wl_proxy_marshal_flags((struct wl_proxy*)o, 1, &wp_viewport_interface, wl_proxy_get_version((struct wl_proxy*)o), 0, NULL, s); }
static inline void wp_viewporter_destroy(struct wp_viewporter *o) { wl_proxy_marshal_flags((struct wl_proxy*)o, 0, NULL, wl_proxy_get_version((struct wl_proxy*)o), WL_MARSHAL_FLAG_DESTROY); }
static inline void wp_viewport_set_destination(struct wp_viewport *o, int32_t w, int32_t h) { wl_proxy_marshal_flags((struct wl_proxy*)o, 3, NULL, wl_proxy_get_version((struct wl_proxy*)o), 0, w, h); }
static inline void wp_viewport_destroy(struct wp_viewport *o) { wl_proxy_marshal_flags((struct wl_proxy*)o, 0, NULL, wl_proxy_get_version((struct wl_proxy*)o), WL_MARSHAL_FLAG_DESTROY); }
