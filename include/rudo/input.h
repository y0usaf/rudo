#ifndef RUDO_INPUT_H
#define RUDO_INPUT_H

#include "rudo/common.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    bool shift;
    bool ctrl;
    bool alt;
} rudo_modifiers;

typedef enum {
    RUDO_KEY_TEXT = 0,
    RUDO_KEY_ENTER,
    RUDO_KEY_BACKSPACE,
    RUDO_KEY_ESCAPE,
    RUDO_KEY_TAB,
    RUDO_KEY_SPACE,
    RUDO_KEY_ARROW_UP,
    RUDO_KEY_ARROW_DOWN,
    RUDO_KEY_ARROW_RIGHT,
    RUDO_KEY_ARROW_LEFT,
    RUDO_KEY_HOME,
    RUDO_KEY_END,
    RUDO_KEY_PAGE_UP,
    RUDO_KEY_PAGE_DOWN,
    RUDO_KEY_DELETE,
    RUDO_KEY_INSERT,
    RUDO_KEY_FUNCTION,
    RUDO_KEY_UNKNOWN,
} rudo_key_kind;

typedef struct {
    rudo_key_kind kind;
    char *text;
    uint8_t function;
} rudo_key;

typedef struct {
    bool pressed;
    rudo_key key;
} rudo_key_event;

typedef enum {
    RUDO_MOUSE_BUTTON_LEFT = 0,
    RUDO_MOUSE_BUTTON_MIDDLE,
    RUDO_MOUSE_BUTTON_RIGHT,
    RUDO_MOUSE_BUTTON_OTHER,
} rudo_mouse_button_kind;

typedef struct {
    rudo_mouse_button_kind kind;
    uint16_t other;
} rudo_mouse_button;

rudo_modifiers rudo_modifiers_empty(void);
bool rudo_modifiers_shift_key(rudo_modifiers mods);
bool rudo_modifiers_control_key(rudo_modifiers mods);
bool rudo_modifiers_alt_key(rudo_modifiers mods);

void rudo_key_init_text(rudo_key *key, const char *text);
void rudo_key_init_named(rudo_key *key, rudo_key_kind kind);
void rudo_key_init_function(rudo_key *key, uint8_t function);
void rudo_key_destroy(rudo_key *key);
void rudo_key_copy(rudo_key *dst, const rudo_key *src);
bool rudo_key_equal(const rudo_key *a, const rudo_key *b);

void rudo_key_event_destroy(rudo_key_event *ev);
void rudo_key_event_copy(rudo_key_event *dst, const rudo_key_event *src);

#ifdef __cplusplus
}
#endif

#endif
