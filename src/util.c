#include "rudo/common.h"

#include <ctype.h>

void rudo_die_oom(void) {
    fputs("rudo: out of memory\n", stderr);
    abort();
}

void *rudo_malloc(size_t size) {
    void *p = malloc(size ? size : 1);
    if (!p) rudo_die_oom();
    return p;
}

void *rudo_calloc(size_t n, size_t size) {
    void *p = calloc(n ? n : 1, size ? size : 1);
    if (!p) rudo_die_oom();
    return p;
}

void *rudo_realloc(void *ptr, size_t size) {
    void *p = realloc(ptr, size ? size : 1);
    if (!p) rudo_die_oom();
    return p;
}

char *rudo_strdup(const char *s) {
    size_t n = s ? strlen(s) : 0;
    char *out = rudo_malloc(n + 1);
    if (n) memcpy(out, s, n);
    out[n] = '\0';
    return out;
}

char *rudo_strndup(const char *s, size_t n) {
    char *out = rudo_malloc(n + 1);
    if (n) memcpy(out, s, n);
    out[n] = '\0';
    return out;
}

void rudo_str_init(rudo_str *s) { s->data = NULL; s->len = 0; s->cap = 0; }
void rudo_str_free(rudo_str *s) { free(s->data); s->data = NULL; s->len = s->cap = 0; }

bool rudo_str_reserve(rudo_str *s, size_t add) {
    size_t need = s->len + add + 1;
    if (need <= s->cap) return true;
    size_t cap = s->cap ? s->cap : 32;
    while (cap < need) cap *= 2;
    s->data = rudo_realloc(s->data, cap);
    s->cap = cap;
    return true;
}

bool rudo_str_append_mem(rudo_str *s, const char *data, size_t len) {
    if (!rudo_str_reserve(s, len)) return false;
    memcpy(s->data + s->len, data, len);
    s->len += len;
    s->data[s->len] = '\0';
    return true;
}

bool rudo_str_append_cstr(rudo_str *s, const char *cstr) { return rudo_str_append_mem(s, cstr, strlen(cstr)); }
bool rudo_str_append_char(rudo_str *s, char ch) { return rudo_str_append_mem(s, &ch, 1); }
char *rudo_str_take(rudo_str *s) { char *p = s->data; s->data = NULL; s->len = s->cap = 0; return p ? p : rudo_strdup(""); }

bool rudo_streq(const char *a, const char *b) { return strcmp(a, b) == 0; }

bool rudo_streq_nocase(const char *a, const char *b) {
    unsigned char ca, cb;
    while (*a && *b) {
        ca = (unsigned char)tolower((unsigned char)*a++);
        cb = (unsigned char)tolower((unsigned char)*b++);
        if (ca != cb) return false;
    }
    return *a == *b;
}

char *rudo_trim(char *s) {
    char *e;
    while (*s && isspace((unsigned char)*s)) ++s;
    if (!*s) return s;
    e = s + strlen(s) - 1;
    while (e > s && isspace((unsigned char)*e)) *e-- = '\0';
    return s;
}

char *rudo_path_join2(const char *a, const char *b) {
    size_t na = strlen(a), nb = strlen(b);
    bool slash = na && a[na - 1] != '/';
    char *out = rudo_malloc(na + slash + nb + 1);
    memcpy(out, a, na);
    if (slash) out[na++] = '/';
    memcpy(out + na, b, nb);
    out[na + nb] = '\0';
    return out;
}

char *rudo_path_join3(const char *a, const char *b, const char *c) {
    char *ab = rudo_path_join2(a, b);
    char *abc = rudo_path_join2(ab, c);
    free(ab);
    return abc;
}

const char *rudo_basename_const(const char *path) {
    const char *s = strrchr(path, '/');
    return s ? s + 1 : path;
}
