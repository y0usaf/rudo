#ifndef RUDO_WAYLAND_KEYBOARD_H
#define RUDO_WAYLAND_KEYBOARD_H

#include "rudo/input.h"

#include <stdbool.h>
#include <stdint.h>
#include <time.h>
#include <xkbcommon/xkbcommon.h>

typedef struct rudo_wayland_keyboard {
    struct xkb_context *context;
    struct xkb_keymap *keymap;
    struct xkb_state *state;
    uint32_t repeat_key;
    int32_t repeat_rate;
    int32_t repeat_delay;
    struct timespec repeat_at;
    bool repeating;
} rudo_wayland_keyboard;

void rudo_wayland_keyboard_destroy(rudo_wayland_keyboard *kb);
bool rudo_wayland_keyboard_keymap(rudo_wayland_keyboard *kb, int fd, uint32_t size);
rudo_modifiers rudo_wayland_keyboard_modifiers(const rudo_wayland_keyboard *kb);
void rudo_wayland_keyboard_update_modifiers(rudo_wayland_keyboard *kb, uint32_t dep, uint32_t lat, uint32_t lock, uint32_t group);
void rudo_wayland_keyboard_set_repeat_info(rudo_wayland_keyboard *kb, int32_t rate, int32_t delay);
void rudo_wayland_keyboard_repeat_start(rudo_wayland_keyboard *kb, uint32_t key);
void rudo_wayland_keyboard_repeat_stop(rudo_wayland_keyboard *kb, uint32_t key);
int rudo_wayland_keyboard_repeat_timeout_ms(const rudo_wayland_keyboard *kb);
bool rudo_wayland_keyboard_translate(rudo_wayland_keyboard *kb, uint32_t key, bool pressed, rudo_key_event *ev);
bool rudo_wayland_keyboard_repeat_fire(rudo_wayland_keyboard *kb, rudo_key_event *ev);

#endif
