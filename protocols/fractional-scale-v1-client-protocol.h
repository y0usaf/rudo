#include <wayland-client-core.h>
#include <wayland-client-protocol.h>
#include <stdint.h>
struct wp_fractional_scale_manager_v1; struct wp_fractional_scale_v1; struct wl_surface;
extern const struct wl_interface wp_fractional_scale_manager_v1_interface, wp_fractional_scale_v1_interface;
struct wp_fractional_scale_v1_listener { void (*preferred_scale)(void *, struct wp_fractional_scale_v1 *, uint32_t); };
static inline struct wp_fractional_scale_v1 *wp_fractional_scale_manager_v1_get_fractional_scale(struct wp_fractional_scale_manager_v1 *o, struct wl_surface *s) { return (struct wp_fractional_scale_v1 *)wl_proxy_marshal_flags((struct wl_proxy*)o, 1, &wp_fractional_scale_v1_interface, wl_proxy_get_version((struct wl_proxy*)o), 0, NULL, s); }
static inline void wp_fractional_scale_manager_v1_destroy(struct wp_fractional_scale_manager_v1 *o) { wl_proxy_marshal_flags((struct wl_proxy*)o, 0, NULL, wl_proxy_get_version((struct wl_proxy*)o), WL_MARSHAL_FLAG_DESTROY); }
static inline int wp_fractional_scale_v1_add_listener(struct wp_fractional_scale_v1 *o, const struct wp_fractional_scale_v1_listener *l, void *d) { return wl_proxy_add_listener((struct wl_proxy*)o, (void (**)(void))l, d); }
static inline void wp_fractional_scale_v1_destroy(struct wp_fractional_scale_v1 *o) { wl_proxy_marshal_flags((struct wl_proxy*)o, 0, NULL, wl_proxy_get_version((struct wl_proxy*)o), WL_MARSHAL_FLAG_DESTROY); }
