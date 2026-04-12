#include "rudo/input.h"

rudo_modifiers rudo_modifiers_empty(void) { rudo_modifiers m = {0}; return m; }
bool rudo_modifiers_shift_key(rudo_modifiers mods) { return mods.shift; }
bool rudo_modifiers_control_key(rudo_modifiers mods) { return mods.ctrl; }
bool rudo_modifiers_alt_key(rudo_modifiers mods) { return mods.alt; }

void rudo_key_init_text(rudo_key *key, const char *text) {
    key->kind = RUDO_KEY_TEXT;
    key->text = rudo_strdup(text ? text : "");
    key->function = 0;
}

void rudo_key_init_named(rudo_key *key, rudo_key_kind kind) {
    key->kind = kind;
    key->text = NULL;
    key->function = 0;
}

void rudo_key_init_function(rudo_key *key, uint8_t function) {
    key->kind = RUDO_KEY_FUNCTION;
    key->text = NULL;
    key->function = function;
}

void rudo_key_destroy(rudo_key *key) {
    free(key->text);
    key->text = NULL;
    key->kind = RUDO_KEY_UNKNOWN;
    key->function = 0;
}

void rudo_key_copy(rudo_key *dst, const rudo_key *src) {
    dst->kind = src->kind;
    dst->function = src->function;
    dst->text = src->text ? rudo_strdup(src->text) : NULL;
}

bool rudo_key_equal(const rudo_key *a, const rudo_key *b) {
    if (a->kind != b->kind) return false;
    if (a->kind == RUDO_KEY_TEXT) return rudo_streq(a->text ? a->text : "", b->text ? b->text : "");
    if (a->kind == RUDO_KEY_FUNCTION) return a->function == b->function;
    return true;
}

void rudo_key_event_destroy(rudo_key_event *ev) { rudo_key_destroy(&ev->key); }
void rudo_key_event_copy(rudo_key_event *dst, const rudo_key_event *src) { dst->pressed = src->pressed; rudo_key_copy(&dst->key, &src->key); }
