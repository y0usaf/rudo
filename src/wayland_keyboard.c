#include "rudo/wayland.h"
#include "rudo/input.h"
#include "rudo/common.h"
#include "wayland_keyboard.h"

#include <ctype.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <fcntl.h>
#include <xkbcommon/xkbcommon.h>

static struct timespec now_mono(void) { struct timespec ts; clock_gettime(CLOCK_MONOTONIC, &ts); return ts; }
static struct timespec add_ms(struct timespec ts, int32_t ms) { ts.tv_sec += ms / 1000; ts.tv_nsec += (long)(ms % 1000) * 1000000L; if (ts.tv_nsec >= 1000000000L) { ts.tv_sec++; ts.tv_nsec -= 1000000000L; } return ts; }
static int cmp_ts(struct timespec a, struct timespec b) { if (a.tv_sec != b.tv_sec) return a.tv_sec < b.tv_sec ? -1 : 1; if (a.tv_nsec != b.tv_nsec) return a.tv_nsec < b.tv_nsec ? -1 : 1; return 0; }

void rudo_wayland_keyboard_destroy(rudo_wayland_keyboard *kb) { if (!kb) return; if (kb->state) xkb_state_unref(kb->state); if (kb->keymap) xkb_keymap_unref(kb->keymap); if (kb->context) xkb_context_unref(kb->context); memset(kb, 0, sizeof(*kb)); }

bool rudo_wayland_keyboard_keymap(rudo_wayland_keyboard *kb, int fd, uint32_t size) {
    char *map; if (!kb) return false; rudo_wayland_keyboard_destroy(kb); kb->context = xkb_context_new(XKB_CONTEXT_NO_FLAGS); if (!kb->context) return false; map = malloc(size + 1u); if (!map) return false; if (pread(fd, map, size, 0) != (ssize_t)size) { free(map); return false; } map[size] = 0; kb->keymap = xkb_keymap_new_from_string(kb->context, map, XKB_KEYMAP_FORMAT_TEXT_V1, XKB_KEYMAP_COMPILE_NO_FLAGS); free(map); if (!kb->keymap) return false; kb->state = xkb_state_new(kb->keymap); return kb->state != NULL; }

rudo_modifiers rudo_wayland_keyboard_modifiers(const rudo_wayland_keyboard *kb) { rudo_modifiers m = {0}; if (!kb || !kb->state) return m; m.shift = xkb_state_mod_name_is_active(kb->state, XKB_MOD_NAME_SHIFT, XKB_STATE_MODS_EFFECTIVE) > 0; m.ctrl = xkb_state_mod_name_is_active(kb->state, XKB_MOD_NAME_CTRL, XKB_STATE_MODS_EFFECTIVE) > 0; m.alt = xkb_state_mod_name_is_active(kb->state, XKB_MOD_NAME_ALT, XKB_STATE_MODS_EFFECTIVE) > 0; return m; }
void rudo_wayland_keyboard_update_modifiers(rudo_wayland_keyboard *kb, uint32_t dep, uint32_t lat, uint32_t lock, uint32_t group) { if (kb && kb->state) xkb_state_update_mask(kb->state, dep, lat, lock, 0, 0, group); }
void rudo_wayland_keyboard_set_repeat_info(rudo_wayland_keyboard *kb, int32_t rate, int32_t delay) { if (!kb) return; kb->repeat_rate = rate; kb->repeat_delay = delay; }
static bool key_repeatable(const rudo_wayland_keyboard *kb, uint32_t key) { return kb && kb->keymap && xkb_keymap_key_repeats(kb->keymap, key + 8); }
void rudo_wayland_keyboard_repeat_start(rudo_wayland_keyboard *kb, uint32_t key) { if (!kb || kb->repeat_rate <= 0 || !key_repeatable(kb, key)) { if (kb) kb->repeating = false; return; } kb->repeat_key = key; kb->repeat_at = add_ms(now_mono(), kb->repeat_delay > 0 ? kb->repeat_delay : 0); kb->repeating = true; }
void rudo_wayland_keyboard_repeat_stop(rudo_wayland_keyboard *kb, uint32_t key) { if (kb && kb->repeating && kb->repeat_key == key) kb->repeating = false; }
int rudo_wayland_keyboard_repeat_timeout_ms(const rudo_wayland_keyboard *kb) { struct timespec n; long long ms; if (!kb || !kb->repeating) return -1; n = now_mono(); if (cmp_ts(kb->repeat_at, n) <= 0) return 0; ms = (long long)(kb->repeat_at.tv_sec - n.tv_sec) * 1000LL + (kb->repeat_at.tv_nsec - n.tv_nsec) / 1000000LL; return ms > 0x7fffffffLL ? 0x7fffffff : (int)ms; }

static void init_named(rudo_key *k, rudo_key_kind kind) { rudo_key_init_named(k, kind); }
static void init_text1(rudo_key *k, uint32_t cp) { char buf[8]; int n = 0; if (cp < 0x80) buf[n++] = (char)cp; else if (cp < 0x800) { buf[n++] = (char)(0xc0 | (cp >> 6)); buf[n++] = (char)(0x80 | (cp & 0x3f)); } else if (cp < 0x10000) { buf[n++] = (char)(0xe0 | (cp >> 12)); buf[n++] = (char)(0x80 | ((cp >> 6) & 0x3f)); buf[n++] = (char)(0x80 | (cp & 0x3f)); } else { buf[n++] = (char)(0xf0 | (cp >> 18)); buf[n++] = (char)(0x80 | ((cp >> 12) & 0x3f)); buf[n++] = (char)(0x80 | ((cp >> 6) & 0x3f)); buf[n++] = (char)(0x80 | (cp & 0x3f)); } buf[n] = 0; rudo_key_init_text(k, buf); }

bool rudo_wayland_keyboard_translate(rudo_wayland_keyboard *kb, uint32_t key, bool pressed, rudo_key_event *ev) {
    xkb_keysym_t sym; uint32_t cp; char tmp[64]; int n; if (!ev) return false; memset(ev, 0, sizeof(*ev)); ev->pressed = pressed; if (!kb || !kb->state) { rudo_key_init_named(&ev->key, RUDO_KEY_UNKNOWN); return true; }
    xkb_state_update_key(kb->state, key + 8, pressed ? XKB_KEY_DOWN : XKB_KEY_UP);
    sym = xkb_state_key_get_one_sym(kb->state, key + 8);
    switch (sym) {
        case XKB_KEY_Return: init_named(&ev->key, RUDO_KEY_ENTER); return true;
        case XKB_KEY_BackSpace: init_named(&ev->key, RUDO_KEY_BACKSPACE); return true;
        case XKB_KEY_Escape: init_named(&ev->key, RUDO_KEY_ESCAPE); return true;
        case XKB_KEY_Tab: init_named(&ev->key, RUDO_KEY_TAB); return true;
        case XKB_KEY_Up: init_named(&ev->key, RUDO_KEY_ARROW_UP); return true;
        case XKB_KEY_Down: init_named(&ev->key, RUDO_KEY_ARROW_DOWN); return true;
        case XKB_KEY_Left: init_named(&ev->key, RUDO_KEY_ARROW_LEFT); return true;
        case XKB_KEY_Right: init_named(&ev->key, RUDO_KEY_ARROW_RIGHT); return true;
        case XKB_KEY_Home: init_named(&ev->key, RUDO_KEY_HOME); return true;
        case XKB_KEY_End: init_named(&ev->key, RUDO_KEY_END); return true;
        case XKB_KEY_Page_Up: init_named(&ev->key, RUDO_KEY_PAGE_UP); return true;
        case XKB_KEY_Page_Down: init_named(&ev->key, RUDO_KEY_PAGE_DOWN); return true;
        case XKB_KEY_Delete: init_named(&ev->key, RUDO_KEY_DELETE); return true;
        case XKB_KEY_Insert: init_named(&ev->key, RUDO_KEY_INSERT); return true;
        case XKB_KEY_space: init_named(&ev->key, RUDO_KEY_SPACE); return true;
    }
    if (sym >= XKB_KEY_F1 && sym <= XKB_KEY_F12) { rudo_key_init_function(&ev->key, (uint8_t)(sym - XKB_KEY_F1 + 1)); return true; }
    cp = xkb_state_key_get_utf32(kb->state, key + 8);
    if (cp > 0 && cp != 0x7f && cp != 0x08 && cp != '\r' && cp != '\n' && !iscntrl((int)(cp & 0xff))) { init_text1(&ev->key, cp); return true; }
    n = xkb_state_key_get_utf8(kb->state, key + 8, tmp, sizeof(tmp));
    if (n > 0) { tmp[n] = 0; rudo_key_init_text(&ev->key, tmp); return true; }
    rudo_key_init_named(&ev->key, RUDO_KEY_UNKNOWN); return true;
}

bool rudo_wayland_keyboard_repeat_fire(rudo_wayland_keyboard *kb, rudo_key_event *ev) {
    if (!kb || !kb->repeating || cmp_ts(kb->repeat_at, now_mono()) > 0) return false; if (!rudo_wayland_keyboard_translate(kb, kb->repeat_key, true, ev)) return false; kb->repeat_at = add_ms(now_mono(), kb->repeat_rate > 0 ? 1000 / kb->repeat_rate : 1000); return true;
}
