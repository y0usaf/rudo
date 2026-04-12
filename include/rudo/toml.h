#ifndef RUDO_TOML_H
#define RUDO_TOML_H

#include "rudo/common.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    RUDO_TOML_STRING = 0,
    RUDO_TOML_INTEGER,
    RUDO_TOML_FLOAT,
    RUDO_TOML_BOOLEAN,
} rudo_toml_type;

typedef struct {
    char *section;
    char *key;
    rudo_toml_type type;
    union {
        char *s;
        int64_t i;
        double f;
        bool b;
    } as;
} rudo_toml_entry;

typedef struct {
    rudo_toml_entry *v;
    size_t len;
    size_t cap;
} rudo_toml_table;

void rudo_toml_table_init(rudo_toml_table *table);
void rudo_toml_table_destroy(rudo_toml_table *table);
bool rudo_toml_parse(rudo_toml_table *out, const char *input, char **err_msg);
const rudo_toml_entry *rudo_toml_get(const rudo_toml_table *table, const char *section, const char *key);
const char *rudo_toml_get_str(const rudo_toml_table *table, const char *section, const char *key);
bool rudo_toml_get_bool(const rudo_toml_table *table, const char *section, const char *key, bool *out_value);
bool rudo_toml_get_i64(const rudo_toml_table *table, const char *section, const char *key, int64_t *out_value);
bool rudo_toml_get_usize(const rudo_toml_table *table, const char *section, const char *key, size_t *out_value);
bool rudo_toml_get_f64(const rudo_toml_table *table, const char *section, const char *key, double *out_value);
bool rudo_toml_get_f32(const rudo_toml_table *table, const char *section, const char *key, float *out_value);

#ifdef __cplusplus
}
#endif

#endif
