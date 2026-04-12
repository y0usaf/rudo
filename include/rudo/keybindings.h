#ifndef RUDO_KEYBINDINGS_H
#define RUDO_KEYBINDINGS_H

#include "rudo/input.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    RUDO_LOCAL_ACTION_COPY = 0,
    RUDO_LOCAL_ACTION_PASTE,
    RUDO_LOCAL_ACTION_ZOOM_IN,
    RUDO_LOCAL_ACTION_ZOOM_OUT,
    RUDO_LOCAL_ACTION_ZOOM_RESET,
    RUDO_LOCAL_ACTION_COUNT,
} rudo_local_action;

typedef struct {
    bool ctrl;
    bool shift;
    bool alt;
} rudo_binding_modifiers;

typedef struct {
    rudo_binding_modifiers modifiers;
    rudo_key key;
} rudo_keybinding;

typedef struct {
    rudo_keybinding *v;
    size_t len;
    size_t cap;
} rudo_keybinding_list;

typedef struct {
    rudo_keybinding_list copy;
    rudo_keybinding_list paste;
    rudo_keybinding_list zoom_in;
    rudo_keybinding_list zoom_out;
    rudo_keybinding_list zoom_reset;
} rudo_keybindings_config;

void rudo_keybinding_destroy(rudo_keybinding *binding);
void rudo_keybinding_list_init(rudo_keybinding_list *list);
void rudo_keybinding_list_destroy(rudo_keybinding_list *list);
bool rudo_keybinding_list_push(rudo_keybinding_list *list, const rudo_keybinding *binding);

bool rudo_keybinding_parse(rudo_keybinding *out, const char *spec, char **err_msg);
bool rudo_keybinding_matches(const rudo_keybinding *binding, const rudo_key_event *event, rudo_modifiers modifiers);
bool rudo_parse_binding_list(rudo_keybinding_list *out, const char *spec, char **err_msg);

void rudo_keybindings_config_init_default(rudo_keybindings_config *cfg);
void rudo_keybindings_config_destroy(rudo_keybindings_config *cfg);
bool rudo_keybindings_matches(const rudo_keybindings_config *cfg, rudo_local_action action, const rudo_key_event *event, rudo_modifiers modifiers);
rudo_keybinding_list *rudo_keybindings_action_list(rudo_keybindings_config *cfg, rudo_local_action action);
const rudo_keybinding_list *rudo_keybindings_action_list_const(const rudo_keybindings_config *cfg, rudo_local_action action);

#ifdef __cplusplus
}
#endif

#endif
