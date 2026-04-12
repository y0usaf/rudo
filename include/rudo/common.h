#ifndef RUDO_COMMON_H
#define RUDO_COMMON_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef __cplusplus
extern "C" {
#endif

#define RUDO_ARRAY_LEN(a) (sizeof(a) / sizeof((a)[0]))
#define RUDO_MIN(a, b) ((a) < (b) ? (a) : (b))
#define RUDO_MAX(a, b) ((a) > (b) ? (a) : (b))
#define RUDO_CLAMP(v, lo, hi) (RUDO_MIN(RUDO_MAX((v), (lo)), (hi)))
#define RUDO_UNUSED(x) ((void)(x))

#if defined(__GNUC__) || defined(__clang__)
# define RUDO_PRINTF(fmt_idx, arg_idx) __attribute__((format(printf, fmt_idx, arg_idx)))
# define RUDO_NORETURN __attribute__((noreturn))
# define RUDO_MALLOC __attribute__((malloc))
#else
# define RUDO_PRINTF(fmt_idx, arg_idx)
# define RUDO_NORETURN
# define RUDO_MALLOC
#endif

typedef struct {
    char *data;
    size_t len;
    size_t cap;
} rudo_str;

void *rudo_malloc(size_t size) RUDO_MALLOC;
void *rudo_calloc(size_t n, size_t size);
void *rudo_realloc(void *ptr, size_t size);
char *rudo_strdup(const char *s);
char *rudo_strndup(const char *s, size_t n);
void rudo_die_oom(void) RUDO_NORETURN;

void rudo_str_init(rudo_str *s);
void rudo_str_free(rudo_str *s);
bool rudo_str_reserve(rudo_str *s, size_t add);
bool rudo_str_append_mem(rudo_str *s, const char *data, size_t len);
bool rudo_str_append_cstr(rudo_str *s, const char *cstr);
bool rudo_str_append_char(rudo_str *s, char ch);
char *rudo_str_take(rudo_str *s);

bool rudo_streq(const char *a, const char *b);
bool rudo_streq_nocase(const char *a, const char *b);
char *rudo_trim(char *s);
char *rudo_path_join2(const char *a, const char *b);
char *rudo_path_join3(const char *a, const char *b, const char *c);
const char *rudo_basename_const(const char *path);

#ifdef __cplusplus
}
#endif

#endif
