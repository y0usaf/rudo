#include "rudo/toml.h"

#include <ctype.h>
#include <stdio.h>

static void set_err(char **err, const char *msg) { if (err) *err = rudo_strdup(msg); }
static void set_err_line(char **err, size_t line, const char *msg) { if (!err) return; size_t n = snprintf(NULL, 0, "line %zu: %s", line, msg); *err = rudo_malloc(n + 1); snprintf(*err, n + 1, "line %zu: %s", line, msg); }

void rudo_toml_table_init(rudo_toml_table *table) { table->v = NULL; table->len = table->cap = 0; }

void rudo_toml_table_destroy(rudo_toml_table *table) {
    size_t i;
    for (i = 0; i < table->len; ++i) {
        free(table->v[i].section);
        free(table->v[i].key);
        if (table->v[i].type == RUDO_TOML_STRING) free(table->v[i].as.s);
    }
    free(table->v);
    table->v = NULL; table->len = table->cap = 0;
}

static bool push_entry(rudo_toml_table *table, const rudo_toml_entry *entry) {
    size_t i;
    for (i = 0; i < table->len; ++i) {
        if (rudo_streq(table->v[i].section, entry->section) && rudo_streq(table->v[i].key, entry->key)) {
            if (table->v[i].type == RUDO_TOML_STRING) free(table->v[i].as.s);
            free(table->v[i].section); free(table->v[i].key);
            table->v[i] = *entry;
            return true;
        }
    }
    if (table->len == table->cap) {
        size_t cap = table->cap ? table->cap * 2 : 16;
        table->v = rudo_realloc(table->v, cap * sizeof(*table->v));
        table->cap = cap;
    }
    table->v[table->len++] = *entry;
    return true;
}

static bool parse_value(rudo_toml_entry *entry, const char *raw, char **err) {
    char *tmp = rudo_strdup(raw);
    char *s = rudo_trim(tmp);
    memset(&entry->as, 0, sizeof(entry->as));
    if (*s == '"') {
        char *end = strchr(s + 1, '"');
        if (!end) { free(tmp); set_err(err, "unclosed string"); return false; }
        *end = 0;
        char *trailing = rudo_trim(end + 1);
        if (*trailing && *trailing != '#') { free(tmp); set_err(err, "cannot parse value"); return false; }
        entry->type = RUDO_TOML_STRING;
        entry->as.s = rudo_strdup(s + 1);
        free(tmp);
        return true;
    }
    char *hash = strchr(s, '#');
    if (hash) *hash = 0;
    s = rudo_trim(s);
    if (rudo_streq(s, "true")) { entry->type = RUDO_TOML_BOOLEAN; entry->as.b = true; free(tmp); return true; }
    if (rudo_streq(s, "false")) { entry->type = RUDO_TOML_BOOLEAN; entry->as.b = false; free(tmp); return true; }
    if (strchr(s, '.')) {
        char *end = NULL; double f = strtod(s, &end);
        if (end && *rudo_trim(end) == 0) { entry->type = RUDO_TOML_FLOAT; entry->as.f = f; free(tmp); return true; }
    }
    {
        char *end = NULL; long long i = strtoll(s, &end, 10);
        if (end && *rudo_trim(end) == 0) { entry->type = RUDO_TOML_INTEGER; entry->as.i = (int64_t)i; free(tmp); return true; }
    }
    free(tmp); set_err(err, "cannot parse value"); return false;
}

bool rudo_toml_parse(rudo_toml_table *out, const char *input, char **err_msg) {
    const char *p = input, *line;
    size_t line_no = 0;
    char *section = rudo_strdup("");
    rudo_toml_entry entry;
    rudo_toml_table_init(out);
    if (err_msg) *err_msg = NULL;
    while (*p) {
        const char *nl = strchr(p, '\n');
        size_t len = nl ? (size_t)(nl - p) : strlen(p);
        char *buf = rudo_strndup(p, len);
        char *s;
        line_no++;
        if (nl) p = nl + 1; else p += len;
        if (len && buf[len - 1] == '\r') buf[len - 1] = 0;
        s = rudo_trim(buf);
        if (!*s || *s == '#') { free(buf); continue; }
        if (*s == '[') {
            char *end = strchr(s + 1, ']');
            if (!end) { free(buf); free(section); set_err_line(err_msg, line_no, "unclosed section bracket"); rudo_toml_table_destroy(out); return false; }
            *end = 0;
            char *name = rudo_trim(s + 1);
            char *trail = rudo_trim(end + 1);
            if (!*name) { free(buf); free(section); set_err_line(err_msg, line_no, "empty section name"); rudo_toml_table_destroy(out); return false; }
            if (*trail && *trail != '#') { free(buf); free(section); set_err_line(err_msg, line_no, "trailing content after section header"); rudo_toml_table_destroy(out); return false; }
            free(section);
            section = rudo_strdup(name);
            free(buf);
            continue;
        }
        char *eq = strchr(s, '=');
        if (!eq) { free(buf); free(section); set_err_line(err_msg, line_no, "expected key = value"); rudo_toml_table_destroy(out); return false; }
        *eq = 0;
        char *key = rudo_trim(s);
        char *val = rudo_trim(eq + 1);
        if (!*key) { free(buf); free(section); set_err_line(err_msg, line_no, "empty key"); rudo_toml_table_destroy(out); return false; }
        entry.section = rudo_strdup(section);
        entry.key = rudo_strdup(key);
        if (!parse_value(&entry, val, err_msg)) { free(entry.section); free(entry.key); free(buf); free(section); if (err_msg && *err_msg) { char *old = *err_msg; size_t n = snprintf(NULL, 0, "line %zu: %s", line_no, old); *err_msg = rudo_malloc(n + 1); snprintf(*err_msg, n + 1, "line %zu: %s", line_no, old); free(old); } rudo_toml_table_destroy(out); return false; }
        push_entry(out, &entry);
        free(buf);
    }
    free(section);
    RUDO_UNUSED(line);
    return true;
}

const rudo_toml_entry *rudo_toml_get(const rudo_toml_table *table, const char *section, const char *key) {
    size_t i;
    for (i = 0; i < table->len; ++i) if (rudo_streq(table->v[i].section, section) && rudo_streq(table->v[i].key, key)) return &table->v[i];
    return NULL;
}

const char *rudo_toml_get_str(const rudo_toml_table *table, const char *section, const char *key) {
    const rudo_toml_entry *e = rudo_toml_get(table, section, key);
    return e && e->type == RUDO_TOML_STRING ? e->as.s : NULL;
}

bool rudo_toml_get_bool(const rudo_toml_table *table, const char *section, const char *key, bool *out_value) {
    const rudo_toml_entry *e = rudo_toml_get(table, section, key);
    if (!e || e->type != RUDO_TOML_BOOLEAN) return false;
    *out_value = e->as.b; return true;
}

bool rudo_toml_get_i64(const rudo_toml_table *table, const char *section, const char *key, int64_t *out_value) {
    const rudo_toml_entry *e = rudo_toml_get(table, section, key);
    if (!e || e->type != RUDO_TOML_INTEGER) return false;
    *out_value = e->as.i; return true;
}

bool rudo_toml_get_usize(const rudo_toml_table *table, const char *section, const char *key, size_t *out_value) {
    int64_t v; if (!rudo_toml_get_i64(table, section, key, &v) || v < 0) return false; *out_value = (size_t)v; return true;
}

bool rudo_toml_get_f64(const rudo_toml_table *table, const char *section, const char *key, double *out_value) {
    const rudo_toml_entry *e = rudo_toml_get(table, section, key);
    if (!e) return false;
    if (e->type == RUDO_TOML_FLOAT) { *out_value = e->as.f; return true; }
    if (e->type == RUDO_TOML_INTEGER) { *out_value = (double)e->as.i; return true; }
    return false;
}

bool rudo_toml_get_f32(const rudo_toml_table *table, const char *section, const char *key, float *out_value) {
    double v; if (!rudo_toml_get_f64(table, section, key, &v)) return false; *out_value = (float)v; return true;
}
