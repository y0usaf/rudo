#include "rudo/keybindings.h"

#include <ctype.h>
#include <stdio.h>

static void set_err(char **err, const char *msg) { if (err) *err = rudo_strdup(msg); }
static void set_errf(char **err, const char *fmt, const char *arg) { if (!err) return; size_t n = snprintf(NULL, 0, fmt, arg); *err = rudo_malloc(n + 1); snprintf(*err, n + 1, fmt, arg); }

static char *lower_dup(const char *s) {
    size_t i, n = strlen(s);
    char *o = rudo_malloc(n + 1);
    for (i = 0; i < n; ++i) o[i] = (char)tolower((unsigned char)s[i]);
    o[n] = 0;
    return o;
}

static void normalize_text_inplace(char *s) {
    if (s[0] && !s[1] && isalpha((unsigned char)s[0])) s[0] = (char)tolower((unsigned char)s[0]);
}

void rudo_keybinding_destroy(rudo_keybinding *binding) { rudo_key_destroy(&binding->key); }
void rudo_keybinding_list_init(rudo_keybinding_list *list) { list->v = NULL; list->len = list->cap = 0; }

void rudo_keybinding_list_destroy(rudo_keybinding_list *list) {
    size_t i;
    for (i = 0; i < list->len; ++i) rudo_keybinding_destroy(&list->v[i]);
    free(list->v);
    list->v = NULL; list->len = list->cap = 0;
}

bool rudo_keybinding_list_push(rudo_keybinding_list *list, const rudo_keybinding *binding) {
    if (list->len == list->cap) {
        size_t cap = list->cap ? list->cap * 2 : 4;
        list->v = rudo_realloc(list->v, cap * sizeof(*list->v));
        list->cap = cap;
    }
    list->v[list->len].modifiers = binding->modifiers;
    rudo_key_copy(&list->v[list->len].key, &binding->key);
    list->len++;
    return true;
}

static bool parse_named_or_text(rudo_key *key, const char *token, char **err) {
    char *lower = lower_dup(token);
    if (rudo_streq(lower, "escape") || rudo_streq(lower, "esc")) rudo_key_init_named(key, RUDO_KEY_ESCAPE);
    else if (rudo_streq(lower, "enter") || rudo_streq(lower, "return")) rudo_key_init_named(key, RUDO_KEY_ENTER);
    else if (rudo_streq(lower, "backspace")) rudo_key_init_named(key, RUDO_KEY_BACKSPACE);
    else if (rudo_streq(lower, "tab")) rudo_key_init_named(key, RUDO_KEY_TAB);
    else if (rudo_streq(lower, "space")) rudo_key_init_named(key, RUDO_KEY_SPACE);
    else if (rudo_streq(lower, "up") || rudo_streq(lower, "arrowup")) rudo_key_init_named(key, RUDO_KEY_ARROW_UP);
    else if (rudo_streq(lower, "down") || rudo_streq(lower, "arrowdown")) rudo_key_init_named(key, RUDO_KEY_ARROW_DOWN);
    else if (rudo_streq(lower, "left") || rudo_streq(lower, "arrowleft")) rudo_key_init_named(key, RUDO_KEY_ARROW_LEFT);
    else if (rudo_streq(lower, "right") || rudo_streq(lower, "arrowright")) rudo_key_init_named(key, RUDO_KEY_ARROW_RIGHT);
    else if (rudo_streq(lower, "home")) rudo_key_init_named(key, RUDO_KEY_HOME);
    else if (rudo_streq(lower, "end")) rudo_key_init_named(key, RUDO_KEY_END);
    else if (rudo_streq(lower, "pageup") || rudo_streq(lower, "pgup")) rudo_key_init_named(key, RUDO_KEY_PAGE_UP);
    else if (rudo_streq(lower, "pagedown") || rudo_streq(lower, "pgdown")) rudo_key_init_named(key, RUDO_KEY_PAGE_DOWN);
    else if (rudo_streq(lower, "delete") || rudo_streq(lower, "del")) rudo_key_init_named(key, RUDO_KEY_DELETE);
    else if (rudo_streq(lower, "insert") || rudo_streq(lower, "ins")) rudo_key_init_named(key, RUDO_KEY_INSERT);
    else if (lower[0] == 'f' && lower[1]) {
        char *end = NULL; long n = strtol(lower + 1, &end, 10);
        if (*end == 0 && n >= 1 && n <= 12) rudo_key_init_function(key, (uint8_t)n); else goto text_fallback;
    } else {
text_fallback:
        {
            const char *text = NULL;
            if (rudo_streq(lower, "plus")) text = "+";
            else if (rudo_streq(lower, "minus")) text = "-";
            else if (rudo_streq(lower, "equal") || rudo_streq(lower, "equals")) text = "=";
            else if (rudo_streq(lower, "comma")) text = ",";
            else if (rudo_streq(lower, "period") || rudo_streq(lower, "dot")) text = ".";
            else if (rudo_streq(lower, "slash")) text = "/";
            else if (rudo_streq(lower, "backslash")) text = "\\";
            else if (rudo_streq(lower, "semicolon")) text = ";";
            else if (rudo_streq(lower, "apostrophe") || rudo_streq(lower, "quote")) text = "'";
            else if (rudo_streq(lower, "grave") || rudo_streq(lower, "backtick")) text = "`";
            else if (rudo_streq(lower, "leftbracket") || rudo_streq(lower, "lbracket")) text = "[";
            else if (rudo_streq(lower, "rightbracket") || rudo_streq(lower, "rbracket")) text = "]";
            else text = token;
            if (strlen(text) != 1) { set_errf(err, "unknown key '%s'", token); free(lower); return false; }
            rudo_key_init_text(key, text);
            normalize_text_inplace(key->text);
        }
    }
    free(lower);
    return true;
}

bool rudo_keybinding_parse(rudo_keybinding *out, const char *spec, char **err_msg) {
    char *copy, *save = NULL, *part;
    char *segments[16];
    size_t nseg = 0, i;
    if (err_msg) *err_msg = NULL;
    memset(out, 0, sizeof(*out));
    copy = rudo_strdup(spec ? spec : "");
    char *trimmed = rudo_trim(copy);
    if (!*trimmed) { free(copy); set_err(err_msg, "empty keybinding"); return false; }
    for (part = strtok_r(trimmed, "+", &save); part; part = strtok_r(NULL, "+", &save)) segments[nseg++] = rudo_trim(part);
    if (!nseg || !segments[nseg - 1][0]) { free(copy); set_errf(err_msg, "missing key in binding '%s' (use names like 'plus' for '+')", spec); return false; }
    for (i = 0; i + 1 < nseg; ++i) {
        if (!segments[i][0]) { free(copy); set_errf(err_msg, "invalid modifier in binding '%s' (use names like 'plus' for '+')", spec); return false; }
        if (rudo_streq_nocase(segments[i], "ctrl") || rudo_streq_nocase(segments[i], "control")) out->modifiers.ctrl = true;
        else if (rudo_streq_nocase(segments[i], "shift")) out->modifiers.shift = true;
        else if (rudo_streq_nocase(segments[i], "alt")) out->modifiers.alt = true;
        else { free(copy); set_errf(err_msg, "unknown modifier '%s'", segments[i]); return false; }
    }
    if (!parse_named_or_text(&out->key, segments[nseg - 1], err_msg)) { free(copy); return false; }
    free(copy);
    return true;
}

static bool allows_implicit_shift(const rudo_key *key) {
    return key->kind == RUDO_KEY_TEXT && key->text && key->text[0] && !key->text[1] && !isalpha((unsigned char)key->text[0]);
}

bool rudo_keybinding_matches(const rudo_keybinding *binding, const rudo_key_event *event, rudo_modifiers modifiers) {
    if (!event->pressed) return false;
    if (!rudo_key_equal(&binding->key, &event->key)) {
        if (binding->key.kind == RUDO_KEY_TEXT && event->key.kind == RUDO_KEY_TEXT && binding->key.text && event->key.text) {
            char *a = rudo_strdup(binding->key.text), *b = rudo_strdup(event->key.text);
            normalize_text_inplace(a); normalize_text_inplace(b);
            bool ok = rudo_streq(a, b);
            free(a); free(b);
            if (!ok) return false;
        } else return false;
    }
    if (binding->modifiers.ctrl != modifiers.ctrl) return false;
    if (binding->modifiers.alt != modifiers.alt) return false;
    if (binding->modifiers.shift == modifiers.shift) return true;
    return allows_implicit_shift(&binding->key) && !binding->modifiers.shift && modifiers.shift;
}

bool rudo_parse_binding_list(rudo_keybinding_list *out, const char *spec, char **err_msg) {
    char *copy, *save = NULL, *part;
    bool any = false;
    rudo_keybinding tmp;
    rudo_keybinding_list_init(out);
    if (err_msg) *err_msg = NULL;
    copy = rudo_strdup(spec ? spec : "");
    for (part = strtok_r(copy, ",", &save); part; part = strtok_r(NULL, ",", &save)) {
        char *trimmed = rudo_trim(part);
        char *err = NULL;
        if (!*trimmed) continue;
        any = true;
        if (!rudo_keybinding_parse(&tmp, trimmed, &err)) { if (err_msg) *err_msg = err; else free(err); free(copy); rudo_keybinding_list_destroy(out); return false; }
        rudo_keybinding_list_push(out, &tmp);
        rudo_keybinding_destroy(&tmp);
    }
    free(copy);
    if (!any) { set_err(err_msg, "expected at least one keybinding"); return false; }
    return true;
}

static void init_action_defaults(rudo_keybinding_list *list, const char *spec) {
    char *err = NULL;
    if (!rudo_parse_binding_list(list, spec, &err)) { free(err); abort(); }
}

void rudo_keybindings_config_init_default(rudo_keybindings_config *cfg) {
    init_action_defaults(&cfg->copy, "ctrl+shift+c");
    init_action_defaults(&cfg->paste, "ctrl+shift+v");
    init_action_defaults(&cfg->zoom_in, "ctrl+equal, ctrl+plus");
    init_action_defaults(&cfg->zoom_out, "ctrl+minus");
    init_action_defaults(&cfg->zoom_reset, "ctrl+0");
}

void rudo_keybindings_config_destroy(rudo_keybindings_config *cfg) {
    rudo_keybinding_list_destroy(&cfg->copy);
    rudo_keybinding_list_destroy(&cfg->paste);
    rudo_keybinding_list_destroy(&cfg->zoom_in);
    rudo_keybinding_list_destroy(&cfg->zoom_out);
    rudo_keybinding_list_destroy(&cfg->zoom_reset);
}

rudo_keybinding_list *rudo_keybindings_action_list(rudo_keybindings_config *cfg, rudo_local_action action) {
    switch (action) {
        case RUDO_LOCAL_ACTION_COPY: return &cfg->copy;
        case RUDO_LOCAL_ACTION_PASTE: return &cfg->paste;
        case RUDO_LOCAL_ACTION_ZOOM_IN: return &cfg->zoom_in;
        case RUDO_LOCAL_ACTION_ZOOM_OUT: return &cfg->zoom_out;
        case RUDO_LOCAL_ACTION_ZOOM_RESET: return &cfg->zoom_reset;
        default: return NULL;
    }
}

const rudo_keybinding_list *rudo_keybindings_action_list_const(const rudo_keybindings_config *cfg, rudo_local_action action) {
    return (const rudo_keybinding_list *)rudo_keybindings_action_list((rudo_keybindings_config *)cfg, action);
}

bool rudo_keybindings_matches(const rudo_keybindings_config *cfg, rudo_local_action action, const rudo_key_event *event, rudo_modifiers modifiers) {
    size_t i;
    const rudo_keybinding_list *list = rudo_keybindings_action_list_const(cfg, action);
    if (!list) return false;
    for (i = 0; i < list->len; ++i) if (rudo_keybinding_matches(&list->v[i], event, modifiers)) return true;
    return false;
}
