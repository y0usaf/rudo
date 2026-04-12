#define _GNU_SOURCE
#include <ctype.h>
#include <dirent.h>
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

#ifndef PATH_MAX
#define PATH_MAX 4096
#endif

#define ATLAS_WIDTH 1024u
#define ATLAS_HEIGHT 1024u
#define ASCII_RANGE 95u
#define ASCII_CACHE_LEN (ASCII_RANGE * 4u)
#define FREETYPE_FIXED_POINT_SCALE 64.0f
#define FALLBACK_BASELINE_RATIO 0.8f
#define DEFAULT_FONT_FAMILY "monospace"
#define APP_NAME "rudo"
#define FONT_PLAN_CACHE_DIR "font-plans-v1"
#define FONT_SLOT_UNRESOLVED 0
#define FONT_SLOT_READY 1
#define FONT_SLOT_FAILED 2

typedef struct {
    float u0;
    float v0;
    float u1;
    float v1;
    float width;
    float height;
    float offset_x;
    float offset_y;
} RudoGlyphInfo;

static RudoGlyphInfo empty_glyph_info(void) {
    RudoGlyphInfo v;
    memset(&v, 0, sizeof(v));
    return v;
}

static char *dup_string(const char *s) {
    size_t n;
    char *out;
    if (s == NULL) {
        return NULL;
    }
    n = strlen(s);
    out = (char *)malloc(n + 1u);
    if (out == NULL) {
        return NULL;
    }
    memcpy(out, s, n + 1u);
    return out;
}

static char *dup_string_n(const char *s, size_t n) {
    char *out;
    out = (char *)malloc(n + 1u);
    if (out == NULL) {
        return NULL;
    }
    if (n != 0u) {
        memcpy(out, s, n);
    }
    out[n] = '\0';
    return out;
}

static char *trimmed_copy(const char *s) {
    const char *start;
    const char *end;
    if (s == NULL) {
        return dup_string(DEFAULT_FONT_FAMILY);
    }
    start = s;
    while (*start != '\0' && isspace((unsigned char)*start)) {
        start++;
    }
    end = start + strlen(start);
    while (end > start && isspace((unsigned char)end[-1])) {
        end--;
    }
    if (end == start) {
        return dup_string(DEFAULT_FONT_FAMILY);
    }
    return dup_string_n(start, (size_t)(end - start));
}

static int path_exists(const char *path) {
    return path != NULL && access(path, R_OK) == 0;
}

static char *path_join2(const char *a, const char *b) {
    size_t na;
    size_t nb;
    int need_sep;
    char *out;
    if (a == NULL || b == NULL) {
        return NULL;
    }
    na = strlen(a);
    nb = strlen(b);
    need_sep = na != 0u && a[na - 1u] != '/';
    out = (char *)malloc(na + (size_t)need_sep + nb + 1u);
    if (out == NULL) {
        return NULL;
    }
    memcpy(out, a, na);
    if (need_sep) {
        out[na] = '/';
        na += 1u;
    }
    memcpy(out + na, b, nb + 1u);
    return out;
}

static char *path_join3(const char *a, const char *b, const char *c) {
    char *ab;
    char *abc;
    ab = path_join2(a, b);
    if (ab == NULL) {
        return NULL;
    }
    abc = path_join2(ab, c);
    free(ab);
    return abc;
}

static int mkdir_p(const char *path) {
    char tmp[PATH_MAX];
    size_t i;
    size_t n;
    if (path == NULL) {
        return 0;
    }
    n = strlen(path);
    if (n == 0u || n >= sizeof(tmp)) {
        return 0;
    }
    memcpy(tmp, path, n + 1u);
    for (i = 1u; i < n; i++) {
        if (tmp[i] == '/') {
            tmp[i] = '\0';
            if (tmp[0] != '\0' && mkdir(tmp, 0700) != 0 && errno != EEXIST) {
                return 0;
            }
            tmp[i] = '/';
        }
    }
    if (mkdir(tmp, 0700) != 0 && errno != EEXIST) {
        return 0;
    }
    return 1;
}

static char *cache_base_dir(void) {
    const char *xdg;
    const char *home;
    char *tmp;
    xdg = getenv("XDG_CACHE_HOME");
    if (xdg != NULL && xdg[0] != '\0') {
        return dup_string(xdg);
    }
    home = getenv("HOME");
    if (home == NULL || home[0] == '\0') {
        return NULL;
    }
    tmp = path_join2(home, ".cache");
    return tmp;
}

static char *sanitize_cache_key(const char *request) {
    size_t n;
    size_t i;
    size_t out_len;
    int last_sep;
    char *out;
    const char *src;
    src = request != NULL ? request : DEFAULT_FONT_FAMILY;
    n = strlen(src);
    out = (char *)malloc((n != 0u ? n : 1u) + 1u);
    if (out == NULL) {
        return NULL;
    }
    out_len = 0u;
    last_sep = 0;
    for (i = 0u; i < n && out_len < 80u; i++) {
        unsigned char ch = (unsigned char)src[i];
        if (isalnum(ch)) {
            out[out_len++] = (char)tolower(ch);
            last_sep = 0;
        } else if (!last_sep) {
            out[out_len++] = '_';
            last_sep = 1;
        }
    }
    while (out_len != 0u && out[out_len - 1u] == '_') {
        out_len--;
    }
    if (out_len == 0u) {
        memcpy(out, "default", 8u);
        return out;
    }
    out[out_len] = '\0';
    return out;
}

static char *font_plan_cache_path(const char *request) {
    char *base;
    char *app_dir;
    char *cache_dir;
    char *key;
    char *file_name;
    char *path;
    size_t key_len;
    base = cache_base_dir();
    if (base == NULL) {
        return NULL;
    }
    app_dir = path_join2(base, APP_NAME);
    free(base);
    if (app_dir == NULL) {
        return NULL;
    }
    cache_dir = path_join2(app_dir, FONT_PLAN_CACHE_DIR);
    free(app_dir);
    if (cache_dir == NULL) {
        return NULL;
    }
    key = sanitize_cache_key(request);
    if (key == NULL) {
        free(cache_dir);
        return NULL;
    }
    key_len = strlen(key);
    file_name = (char *)malloc(key_len + 5u);
    if (file_name == NULL) {
        free(key);
        free(cache_dir);
        return NULL;
    }
    memcpy(file_name, key, key_len);
    memcpy(file_name + key_len, ".txt", 5u);
    free(key);
    path = path_join2(cache_dir, file_name);
    free(file_name);
    free(cache_dir);
    return path;
}

static char *read_cached_regular_path(const char *request) {
    char *path;
    FILE *fp;
    char *line;
    size_t cap;
    ssize_t nread;
    char *regular;
    path = font_plan_cache_path(request);
    if (path == NULL) {
        return NULL;
    }
    fp = fopen(path, "r");
    free(path);
    if (fp == NULL) {
        return NULL;
    }
    line = NULL;
    cap = 0u;
    regular = NULL;
    while ((nread = getline(&line, &cap, fp)) >= 0) {
        char *eq;
        char *value;
        char *end;
        if (nread == 0) {
            continue;
        }
        eq = strchr(line, '=');
        if (eq == NULL) {
            continue;
        }
        *eq = '\0';
        if (strcmp(line, "regular") != 0) {
            continue;
        }
        value = eq + 1;
        end = value + strlen(value);
        while (end > value && (end[-1] == '\n' || end[-1] == '\r' || isspace((unsigned char)end[-1]))) {
            end--;
        }
        while (*value != '\0' && isspace((unsigned char)*value)) {
            value++;
        }
        if (end > value) {
            regular = dup_string_n(value, (size_t)(end - value));
        }
        break;
    }
    free(line);
    fclose(fp);
    if (!path_exists(regular)) {
        free(regular);
        return NULL;
    }
    return regular;
}

static void write_cached_regular_path(const char *request, const char *regular) {
    char *path;
    char *dir;
    FILE *fp;
    char *slash;
    if (regular == NULL || regular[0] == '\0') {
        return;
    }
    path = font_plan_cache_path(request);
    if (path == NULL) {
        return;
    }
    dir = dup_string(path);
    if (dir == NULL) {
        free(path);
        return;
    }
    slash = strrchr(dir, '/');
    if (slash == NULL) {
        free(dir);
        free(path);
        return;
    }
    *slash = '\0';
    if (!mkdir_p(dir)) {
        free(dir);
        free(path);
        return;
    }
    free(dir);
    fp = fopen(path, "w");
    if (fp != NULL) {
        fprintf(fp, "regular=%s\n", regular);
        fclose(fp);
    }
    free(path);
}

static int grow_array(void **ptr, size_t elem_size, size_t *cap, size_t need) {
    size_t new_cap;
    void *new_ptr;
    if (need <= *cap) {
        return 1;
    }
    new_cap = *cap != 0u ? *cap : 8u;
    while (new_cap < need) {
        if (new_cap > ((size_t)-1) / 2u) {
            return 0;
        }
        new_cap *= 2u;
    }
    new_ptr = realloc(*ptr, elem_size * new_cap);
    if (new_ptr == NULL) {
        return 0;
    }
    *ptr = new_ptr;
    *cap = new_cap;
    return 1;
}

static int string_array_contains(char **items, size_t len, const char *value) {
    size_t i;
    if (value == NULL) {
        return 0;
    }
    for (i = 0u; i < len; i++) {
        if (items[i] != NULL && strcmp(items[i], value) == 0) {
            return 1;
        }
    }
    return 0;
}

static int append_unique_string(char ***items, size_t *len, size_t *cap, const char *value) {
    char *copy;
    if (value == NULL || value[0] == '\0') {
        return 1;
    }
    if (string_array_contains(*items, *len, value)) {
        return 1;
    }
    if (!grow_array((void **)items, sizeof(char *), cap, *len + 1u)) {
        return 0;
    }
    copy = dup_string(value);
    if (copy == NULL) {
        return 0;
    }
    (*items)[*len] = copy;
    *len += 1u;
    return 1;
}

static void free_string_array(char **items, size_t len) {
    size_t i;
    if (items == NULL) {
        return;
    }
    for (i = 0u; i < len; i++) {
        free(items[i]);
    }
    free(items);
}

/* ── fontconfig via dlopen ─────────────────────────────────────────────── */

typedef unsigned char FcChar8;
typedef int FcBool;
typedef int FcResult;
typedef int FcMatchKind;
typedef void FcConfig;
typedef void FcPattern;

typedef struct {
    int nfont;
    int sfont;
    FcPattern **fonts;
} FcFontSet;

typedef struct {
    void *lib;
    FcConfig *(*FcInitLoadConfigAndFonts)(void);
    void (*FcConfigDestroy)(FcConfig *);
    FcPattern *(*FcNameParse)(const FcChar8 *);
    void (*FcPatternDestroy)(FcPattern *);
    FcBool (*FcConfigSubstitute)(FcConfig *, FcPattern *, FcMatchKind);
    void (*FcDefaultSubstitute)(FcPattern *);
    FcPattern *(*FcFontMatch)(FcConfig *, FcPattern *, FcResult *);
    FcFontSet *(*FcFontSort)(FcConfig *, FcPattern *, FcBool, void *, FcResult *);
    void (*FcFontSetDestroy)(FcFontSet *);
    FcResult (*FcPatternGetString)(const FcPattern *, const char *, int, FcChar8 **);
} FcHandle;

static FcHandle g_fc;
static int g_fc_state;

#define FC_TRUE 1
#define FC_MATCH_PATTERN 0
#define FC_RESULT_MATCH 0
#define FC_RESULT_NOMATCH 1

static void *dlopen_any2(const char *a, const char *b) {
    void *lib;
    lib = dlopen(a, RTLD_NOW | RTLD_LOCAL);
    if (lib != NULL) {
        return lib;
    }
    return dlopen(b, RTLD_NOW | RTLD_LOCAL);
}

static void *dlsym_required(void *lib, const char *name) {
    return dlsym(lib, name);
}

static int ensure_fc(void) {
    void *lib;
    if (g_fc_state != 0) {
        return g_fc_state > 0;
    }
    memset(&g_fc, 0, sizeof(g_fc));
    lib = dlopen_any2("libfontconfig.so.1", "libfontconfig.so");
    if (lib == NULL) {
        g_fc_state = -1;
        return 0;
    }
    g_fc.lib = lib;
    g_fc.FcInitLoadConfigAndFonts = (FcConfig *(*)(void))dlsym_required(lib, "FcInitLoadConfigAndFonts");
    g_fc.FcConfigDestroy = (void (*)(FcConfig *))dlsym_required(lib, "FcConfigDestroy");
    g_fc.FcNameParse = (FcPattern *(*)(const FcChar8 *))dlsym_required(lib, "FcNameParse");
    g_fc.FcPatternDestroy = (void (*)(FcPattern *))dlsym_required(lib, "FcPatternDestroy");
    g_fc.FcConfigSubstitute = (FcBool (*)(FcConfig *, FcPattern *, FcMatchKind))dlsym_required(lib, "FcConfigSubstitute");
    g_fc.FcDefaultSubstitute = (void (*)(FcPattern *))dlsym_required(lib, "FcDefaultSubstitute");
    g_fc.FcFontMatch = (FcPattern *(*)(FcConfig *, FcPattern *, FcResult *))dlsym_required(lib, "FcFontMatch");
    g_fc.FcFontSort = (FcFontSet *(*)(FcConfig *, FcPattern *, FcBool, void *, FcResult *))dlsym_required(lib, "FcFontSort");
    g_fc.FcFontSetDestroy = (void (*)(FcFontSet *))dlsym_required(lib, "FcFontSetDestroy");
    g_fc.FcPatternGetString = (FcResult (*)(const FcPattern *, const char *, int, FcChar8 **))dlsym_required(lib, "FcPatternGetString");
    if (g_fc.FcInitLoadConfigAndFonts == NULL || g_fc.FcConfigDestroy == NULL || g_fc.FcNameParse == NULL ||
        g_fc.FcPatternDestroy == NULL || g_fc.FcConfigSubstitute == NULL || g_fc.FcDefaultSubstitute == NULL ||
        g_fc.FcFontMatch == NULL || g_fc.FcFontSort == NULL || g_fc.FcFontSetDestroy == NULL ||
        g_fc.FcPatternGetString == NULL) {
        dlclose(lib);
        memset(&g_fc, 0, sizeof(g_fc));
        g_fc_state = -1;
        return 0;
    }
    g_fc_state = 1;
    return 1;
}

static FcPattern *build_fontconfig_pattern(FcConfig *config, const char *pattern_str) {
    FcPattern *pattern;
    pattern = g_fc.FcNameParse((const FcChar8 *)pattern_str);
    if (pattern == NULL) {
        return NULL;
    }
    g_fc.FcConfigSubstitute(config, pattern, FC_MATCH_PATTERN);
    g_fc.FcDefaultSubstitute(pattern);
    return pattern;
}

static char *fontconfig_pattern_file(FcPattern *pattern) {
    FcChar8 *raw;
    char *path;
    if (pattern == NULL) {
        return NULL;
    }
    raw = NULL;
    if (g_fc.FcPatternGetString(pattern, "file", 0, &raw) != FC_RESULT_MATCH || raw == NULL) {
        return NULL;
    }
    path = dup_string((const char *)raw);
    if (!path_exists(path)) {
        free(path);
        return NULL;
    }
    return path;
}

static char *fontconfig_match_file(const char *pattern_str) {
    FcConfig *config;
    FcPattern *pattern;
    FcPattern *matched;
    FcResult result;
    char *path;
    if (!ensure_fc()) {
        return NULL;
    }
    config = g_fc.FcInitLoadConfigAndFonts();
    if (config == NULL) {
        return NULL;
    }
    pattern = build_fontconfig_pattern(config, pattern_str);
    if (pattern == NULL) {
        g_fc.FcConfigDestroy(config);
        return NULL;
    }
    result = FC_RESULT_NOMATCH;
    matched = g_fc.FcFontMatch(config, pattern, &result);
    path = matched != NULL ? fontconfig_pattern_file(matched) : NULL;
    if (matched != NULL) {
        g_fc.FcPatternDestroy(matched);
    }
    g_fc.FcPatternDestroy(pattern);
    g_fc.FcConfigDestroy(config);
    return path;
}

static char **fontconfig_sort_files(const char *pattern_str, size_t *out_len) {
    FcConfig *config;
    FcPattern *pattern;
    FcFontSet *set;
    FcResult result;
    char **paths;
    size_t len;
    size_t cap;
    int i;
    *out_len = 0u;
    if (!ensure_fc()) {
        return NULL;
    }
    config = g_fc.FcInitLoadConfigAndFonts();
    if (config == NULL) {
        return NULL;
    }
    pattern = build_fontconfig_pattern(config, pattern_str);
    if (pattern == NULL) {
        g_fc.FcConfigDestroy(config);
        return NULL;
    }
    result = FC_RESULT_NOMATCH;
    set = g_fc.FcFontSort(config, pattern, FC_TRUE, NULL, &result);
    paths = NULL;
    len = 0u;
    cap = 0u;
    if (set != NULL) {
        for (i = 0; i < set->nfont; i++) {
            char *path;
            path = fontconfig_pattern_file(set->fonts[i]);
            if (path == NULL) {
                continue;
            }
            if (!append_unique_string(&paths, &len, &cap, path)) {
                free(path);
                break;
            }
            free(path);
        }
        g_fc.FcFontSetDestroy(set);
    }
    g_fc.FcPatternDestroy(pattern);
    g_fc.FcConfigDestroy(config);
    *out_len = len;
    return paths;
}

/* ── freetype via dlopen ───────────────────────────────────────────────── */

typedef int FT_Error;
typedef void *FT_Library;
typedef struct FT_FaceRec_ *FT_Face;
typedef int32_t FT_Int32;
typedef unsigned int FT_UInt;
typedef unsigned long FT_ULong;
typedef long FT_Long;
typedef long FT_F26Dot6;
typedef long FT_Pos;
typedef long FT_Fixed;

typedef struct {
    void *data;
    void *finalizer;
} FT_Generic;

typedef struct {
    FT_Pos xmin;
    FT_Pos ymin;
    FT_Pos xmax;
    FT_Pos ymax;
} FT_BBox;

typedef struct {
    FT_Pos x;
    FT_Pos y;
} FT_Vector;

typedef struct {
    unsigned int rows;
    unsigned int width;
    int pitch;
    unsigned char *buffer;
    unsigned short num_grays;
    unsigned char pixel_mode;
    unsigned char palette_mode;
    void *palette;
} FT_Bitmap;

typedef struct {
    FT_Pos width;
    FT_Pos height;
    FT_Pos horiBearingX;
    FT_Pos horiBearingY;
    FT_Pos horiAdvance;
    FT_Pos vertBearingX;
    FT_Pos vertBearingY;
    FT_Pos vertAdvance;
} FT_Glyph_Metrics;

typedef struct FT_GlyphSlotRec_ {
    FT_Library library;
    FT_Face face;
    struct FT_GlyphSlotRec_ *next;
    FT_UInt glyph_index;
    FT_Generic generic;
    FT_Glyph_Metrics metrics;
    FT_Fixed linearHoriAdvance;
    FT_Fixed linearVertAdvance;
    FT_Vector advance;
    unsigned int format;
    FT_Bitmap bitmap;
    int bitmap_left;
    int bitmap_top;
} FT_GlyphSlotRec;

typedef struct {
    unsigned short x_ppem;
    unsigned short y_ppem;
    FT_Fixed x_scale;
    FT_Fixed y_scale;
    FT_Pos ascender;
    FT_Pos descender;
    FT_Pos height;
    FT_Pos max_advance;
} FT_Size_Metrics;

typedef struct FT_SizeRec_ {
    FT_Face face;
    FT_Generic generic;
    FT_Size_Metrics metrics;
} FT_SizeRec;

typedef struct FT_FaceRec_ {
    FT_Long num_faces;
    FT_Long face_index;
    FT_Long face_flags;
    FT_Long style_flags;
    FT_Long num_glyphs;
    char *family_name;
    char *style_name;
    int num_fixed_sizes;
    void *available_sizes;
    int num_charmaps;
    void *charmaps;
    FT_Generic generic;
    FT_BBox bbox;
    unsigned short units_per_em;
    short ascender;
    short descender;
    short height;
    short max_advance_width;
    short max_advance_height;
    short underline_position;
    short underline_thickness;
    FT_GlyphSlotRec *glyph;
    FT_SizeRec *size;
} FT_FaceRec;

typedef struct {
    void *lib;
    FT_Error (*Init_FreeType)(FT_Library *);
    FT_Error (*Done_FreeType)(FT_Library);
    FT_Error (*New_Face)(FT_Library, const char *, FT_Long, FT_Face *);
    FT_Error (*Done_Face)(FT_Face);
    FT_Error (*Set_Pixel_Sizes)(FT_Face, FT_UInt, FT_UInt);
    FT_Error (*Set_Char_Size)(FT_Face, FT_F26Dot6, FT_F26Dot6, FT_UInt, FT_UInt);
    FT_Error (*Load_Char)(FT_Face, FT_ULong, FT_Int32);
    FT_UInt (*Get_Char_Index)(FT_Face, FT_ULong);
} FtHandle;

static FtHandle g_ft;
static FT_Library g_ft_library;
static int g_ft_state;

#define FT_LOAD_DEFAULT 0
#define FT_LOAD_RENDER (1 << 2)

static int ensure_ft(void) {
    void *lib;
    FT_Library library;
    if (g_ft_state != 0) {
        return g_ft_state > 0;
    }
    memset(&g_ft, 0, sizeof(g_ft));
    lib = dlopen_any2("libfreetype.so.6", "libfreetype.so");
    if (lib == NULL) {
        g_ft_state = -1;
        return 0;
    }
    g_ft.lib = lib;
    g_ft.Init_FreeType = (FT_Error (*)(FT_Library *))dlsym_required(lib, "FT_Init_FreeType");
    g_ft.Done_FreeType = (FT_Error (*)(FT_Library))dlsym_required(lib, "FT_Done_FreeType");
    g_ft.New_Face = (FT_Error (*)(FT_Library, const char *, FT_Long, FT_Face *))dlsym_required(lib, "FT_New_Face");
    g_ft.Done_Face = (FT_Error (*)(FT_Face))dlsym_required(lib, "FT_Done_Face");
    g_ft.Set_Pixel_Sizes = (FT_Error (*)(FT_Face, FT_UInt, FT_UInt))dlsym_required(lib, "FT_Set_Pixel_Sizes");
    g_ft.Set_Char_Size = (FT_Error (*)(FT_Face, FT_F26Dot6, FT_F26Dot6, FT_UInt, FT_UInt))dlsym_required(lib, "FT_Set_Char_Size");
    g_ft.Load_Char = (FT_Error (*)(FT_Face, FT_ULong, FT_Int32))dlsym_required(lib, "FT_Load_Char");
    g_ft.Get_Char_Index = (FT_UInt (*)(FT_Face, FT_ULong))dlsym_required(lib, "FT_Get_Char_Index");
    if (g_ft.Init_FreeType == NULL || g_ft.Done_FreeType == NULL || g_ft.New_Face == NULL ||
        g_ft.Done_Face == NULL || g_ft.Set_Pixel_Sizes == NULL || g_ft.Set_Char_Size == NULL ||
        g_ft.Load_Char == NULL || g_ft.Get_Char_Index == NULL) {
        dlclose(lib);
        memset(&g_ft, 0, sizeof(g_ft));
        g_ft_state = -1;
        return 0;
    }
    library = NULL;
    if (g_ft.Init_FreeType(&library) != 0 || library == NULL) {
        dlclose(lib);
        memset(&g_ft, 0, sizeof(g_ft));
        g_ft_state = -1;
        return 0;
    }
    g_ft_library = library;
    g_ft_state = 1;
    return 1;
}

/* ── font discovery ────────────────────────────────────────────────────── */

static int has_font_extension(const char *name) {
    const char *dot;
    if (name == NULL) {
        return 0;
    }
    dot = strrchr(name, '.');
    if (dot == NULL) {
        return 0;
    }
    return strcasecmp(dot, ".ttf") == 0 || strcasecmp(dot, ".otf") == 0;
}

static char *find_first_font_in_dir(const char *dir_path) {
    DIR *dir;
    struct dirent *entry;
    char **subdirs;
    size_t subdir_len;
    size_t subdir_cap;
    char *found;
    dir = opendir(dir_path);
    if (dir == NULL) {
        return NULL;
    }
    subdirs = NULL;
    subdir_len = 0u;
    subdir_cap = 0u;
    found = NULL;
    while ((entry = readdir(dir)) != NULL) {
        char *path;
        struct stat st;
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) {
            continue;
        }
        path = path_join2(dir_path, entry->d_name);
        if (path == NULL) {
            continue;
        }
        if (stat(path, &st) != 0) {
            free(path);
            continue;
        }
        if (S_ISREG(st.st_mode) && has_font_extension(entry->d_name)) {
            found = path;
            break;
        }
        if (S_ISDIR(st.st_mode)) {
            if (!grow_array((void **)&subdirs, sizeof(char *), &subdir_cap, subdir_len + 1u)) {
                free(path);
                continue;
            }
            subdirs[subdir_len++] = path;
        } else {
            free(path);
        }
    }
    closedir(dir);
    if (found != NULL) {
        free_string_array(subdirs, subdir_len);
        return found;
    }
    while (subdir_len != 0u) {
        char *subdir;
        subdir = subdirs[--subdir_len];
        found = find_first_font_in_dir(subdir);
        free(subdir);
        if (found != NULL) {
            free_string_array(subdirs, subdir_len);
            return found;
        }
    }
    free(subdirs);
    return NULL;
}

static char *scan_filesystem_for_any_font(void) {
    static const char *const search_dirs[] = {
        "/usr/share/fonts",
        "/usr/local/share/fonts",
        "/nix/var/nix/profiles/system/sw/share/X11/fonts",
        "/run/current-system/sw/share/X11/fonts",
    };
    size_t i;
    char *path;
    const char *home;
    path = NULL;
    for (i = 0u; i < sizeof(search_dirs) / sizeof(search_dirs[0]); i++) {
        path = find_first_font_in_dir(search_dirs[i]);
        if (path != NULL) {
            return path;
        }
    }
    home = getenv("HOME");
    if (home != NULL && home[0] != '\0') {
        char *dir;
        dir = path_join3(home, ".local", "share/fonts");
        if (dir != NULL) {
            path = find_first_font_in_dir(dir);
            free(dir);
            if (path != NULL) {
                return path;
            }
        }
        dir = path_join2(home, ".fonts");
        if (dir != NULL) {
            path = find_first_font_in_dir(dir);
            free(dir);
            if (path != NULL) {
                return path;
            }
        }
    }
    return NULL;
}

static char *resolve_regular_font_path(const char *request) {
    char *path;
    path = read_cached_regular_path(request);
    if (path != NULL) {
        return path;
    }
    path = fontconfig_match_file(request);
    if (path == NULL && strcmp(request, DEFAULT_FONT_FAMILY) != 0) {
        path = fontconfig_match_file(DEFAULT_FONT_FAMILY);
    }
    if (path == NULL) {
        path = scan_filesystem_for_any_font();
    }
    if (path != NULL) {
        write_cached_regular_path(request, path);
    }
    return path;
}

static char *resolve_style_font_path(const char *request, const char *const *styles, size_t style_count) {
    size_t i;
    for (i = 0u; i < style_count; i++) {
        const char *style = styles[i];
        size_t nr;
        size_t ns;
        char *pattern;
        char *path;
        nr = strlen(request);
        ns = strlen(style);
        pattern = (char *)malloc(nr + 1u + ns + 1u);
        if (pattern == NULL) {
            return NULL;
        }
        memcpy(pattern, request, nr);
        pattern[nr] = ':';
        memcpy(pattern + nr + 1u, style, ns + 1u);
        path = fontconfig_match_file(pattern);
        free(pattern);
        if (path != NULL) {
            return path;
        }
    }
    return NULL;
}

/* ── freetype font wrapper ─────────────────────────────────────────────── */

typedef struct {
    FT_Face face;
    char *path;
} FtFont;

static void ft_font_init(FtFont *font) {
    font->face = NULL;
    font->path = NULL;
}

static void ft_font_drop(FtFont *font) {
    if (font == NULL) {
        return;
    }
    if (font->face != NULL && ensure_ft()) {
        g_ft.Done_Face(font->face);
    }
    font->face = NULL;
    free(font->path);
    font->path = NULL;
}

static void ft_face_set_size(FT_Face face, float size_px) {
    FT_F26Dot6 size_26_6;
    if (face == NULL) {
        return;
    }
    if (size_px < 1.0f) {
        size_px = 1.0f;
    }
    size_26_6 = (FT_F26Dot6)(size_px * FREETYPE_FIXED_POINT_SCALE + 0.5f);
    if (g_ft.Set_Char_Size(face, 0, size_26_6, 72u, 72u) != 0) {
        g_ft.Set_Pixel_Sizes(face, 0u, (FT_UInt)(size_px + 0.5f));
    }
}

static int ft_font_load(FtFont *font, const char *path, float size_px) {
    FT_Face face;
    char *copy;
    if (!ensure_ft() || path == NULL) {
        return 0;
    }
    face = NULL;
    if (g_ft.New_Face(g_ft_library, path, 0, &face) != 0 || face == NULL) {
        return 0;
    }
    copy = dup_string(path);
    if (copy == NULL) {
        g_ft.Done_Face(face);
        return 0;
    }
    ft_face_set_size(face, size_px);
    font->face = face;
    font->path = copy;
    return 1;
}

static int ft_font_has_glyph(const FtFont *font, uint32_t ch) {
    if (font == NULL || font->face == NULL || !ensure_ft()) {
        return 0;
    }
    return g_ft.Get_Char_Index(font->face, (FT_ULong)ch) != 0u;
}

static float ft_font_glyph_advance(FtFont *font, uint32_t ch) {
    FT_GlyphSlotRec *slot;
    if (font == NULL || font->face == NULL || !ensure_ft()) {
        return 0.0f;
    }
    if (g_ft.Load_Char(font->face, (FT_ULong)ch, FT_LOAD_DEFAULT) != 0) {
        return 0.0f;
    }
    slot = font->face->glyph;
    if (slot == NULL) {
        return 0.0f;
    }
    return (float)slot->metrics.horiAdvance / FREETYPE_FIXED_POINT_SCALE;
}

static void ft_font_line_metrics(FtFont *font, float *asc, float *desc, float *height) {
    FT_SizeRec *size;
    if (asc != NULL) {
        *asc = 0.0f;
    }
    if (desc != NULL) {
        *desc = 0.0f;
    }
    if (height != NULL) {
        *height = 0.0f;
    }
    if (font == NULL || font->face == NULL || font->face->size == NULL) {
        return;
    }
    size = font->face->size;
    if (asc != NULL) {
        *asc = (float)size->metrics.ascender / FREETYPE_FIXED_POINT_SCALE;
    }
    if (desc != NULL) {
        *desc = (float)size->metrics.descender / FREETYPE_FIXED_POINT_SCALE;
    }
    if (height != NULL) {
        *height = (float)size->metrics.height / FREETYPE_FIXED_POINT_SCALE;
    }
}

static float compute_cell_width(FtFont *font) {
    static const uint32_t samples[] = { 'M', 'W', '@', '0' };
    float width;
    size_t i;
    width = 0.0f;
    for (i = 0u; i < sizeof(samples) / sizeof(samples[0]); i++) {
        float advance = ft_font_glyph_advance(font, samples[i]);
        if (advance > width) {
            width = advance;
        }
    }
    return width >= 1.0f ? width : 1.0f;
}

static float compute_cell_height(FtFont *font) {
    float asc;
    float desc;
    float line_height;
    float from_metrics;
    FT_GlyphSlotRec *slot;
    ft_font_line_metrics(font, &asc, &desc, &line_height);
    from_metrics = line_height;
    if (asc - desc > from_metrics) {
        from_metrics = asc - desc;
    }
    if (from_metrics > 0.0f) {
        return from_metrics >= 1.0f ? from_metrics : 1.0f;
    }
    if (font != NULL && font->face != NULL && ensure_ft() && g_ft.Load_Char(font->face, (FT_ULong)'M', FT_LOAD_RENDER) == 0) {
        slot = font->face->glyph;
        if (slot != NULL && slot->bitmap.rows > 0u) {
            float h = (float)slot->bitmap.rows;
            return h >= 1.0f ? h : 1.0f;
        }
    }
    return 1.0f;
}

/* ── atlas + cache ─────────────────────────────────────────────────────── */

typedef struct {
    uint32_t ch;
    unsigned char style;
    unsigned char used;
    RudoGlyphInfo glyph;
} GlyphCacheEntry;

typedef struct {
    FtFont regular;
    FtFont bold;
    FtFont italic;
    FtFont bold_italic;
    int bold_state;
    int italic_state;
    int bold_italic_state;
    FtFont *fallback_fonts;
    size_t fallback_font_len;
    size_t fallback_font_cap;
    char **fallback_paths;
    size_t fallback_path_len;
    size_t fallback_path_cap;
    size_t next_fallback_path;
    int fallback_paths_resolved;
    char *request;
    float font_size;
    float cell_width;
    float cell_height;
    float baseline;
    unsigned char *atlas_data;
    GlyphCacheEntry *cache;
    size_t cache_len;
    size_t cache_cap;
    unsigned int current_x;
    unsigned int current_y;
    unsigned int row_height;
    RudoGlyphInfo ascii_cache[ASCII_CACHE_LEN];
    unsigned char ascii_populated[ASCII_CACHE_LEN];
} RudoFontAtlas;

static unsigned int ascii_cache_idx(unsigned char ch, int bold, int italic) {
    unsigned int style = (unsigned int)((bold != 0) * 2 + (italic != 0));
    return style * ASCII_RANGE + (unsigned int)(ch - 32u);
}

static uint32_t glyph_hash(uint32_t ch, unsigned char style) {
    uint32_t x;
    x = ch * 0x9e3779b1u;
    x ^= (uint32_t)style * 0x85ebca6bu;
    x ^= x >> 16;
    x *= 0x7feb352du;
    x ^= x >> 15;
    return x;
}

static void glyph_cache_clear(RudoFontAtlas *atlas) {
    if (atlas->cache != NULL && atlas->cache_cap != 0u) {
        memset(atlas->cache, 0, atlas->cache_cap * sizeof(atlas->cache[0]));
    }
    atlas->cache_len = 0u;
}

static int glyph_cache_reserve(RudoFontAtlas *atlas, size_t need) {
    GlyphCacheEntry *old_entries;
    size_t old_cap;
    GlyphCacheEntry *new_entries;
    size_t new_cap;
    size_t i;
    if (atlas->cache_cap >= need && atlas->cache_cap != 0u) {
        return 1;
    }
    new_cap = atlas->cache_cap != 0u ? atlas->cache_cap : 256u;
    while (new_cap < need * 2u) {
        if (new_cap > ((size_t)-1) / 2u) {
            return 0;
        }
        new_cap *= 2u;
    }
    new_entries = (GlyphCacheEntry *)calloc(new_cap, sizeof(GlyphCacheEntry));
    if (new_entries == NULL) {
        return 0;
    }
    old_entries = atlas->cache;
    old_cap = atlas->cache_cap;
    atlas->cache = new_entries;
    atlas->cache_cap = new_cap;
    atlas->cache_len = 0u;
    for (i = 0u; i < old_cap; i++) {
        size_t idx;
        if (!old_entries[i].used) {
            continue;
        }
        idx = (size_t)glyph_hash(old_entries[i].ch, old_entries[i].style) & (atlas->cache_cap - 1u);
        while (atlas->cache[idx].used) {
            idx = (idx + 1u) & (atlas->cache_cap - 1u);
        }
        atlas->cache[idx] = old_entries[i];
        atlas->cache_len += 1u;
    }
    free(old_entries);
    return 1;
}

static int glyph_cache_lookup(RudoFontAtlas *atlas, uint32_t ch, unsigned char style, RudoGlyphInfo *out) {
    size_t idx;
    if (atlas->cache_cap == 0u || atlas->cache == NULL) {
        return 0;
    }
    idx = (size_t)glyph_hash(ch, style) & (atlas->cache_cap - 1u);
    for (;;) {
        GlyphCacheEntry *entry = &atlas->cache[idx];
        if (!entry->used) {
            return 0;
        }
        if (entry->style == style && entry->ch == ch) {
            *out = entry->glyph;
            return 1;
        }
        idx = (idx + 1u) & (atlas->cache_cap - 1u);
    }
}

static void glyph_cache_insert(RudoFontAtlas *atlas, uint32_t ch, unsigned char style, RudoGlyphInfo glyph) {
    size_t idx;
    if ((atlas->cache_len + 1u) * 10u >= atlas->cache_cap * 7u) {
        if (!glyph_cache_reserve(atlas, atlas->cache_len + 1u)) {
            return;
        }
    } else if (atlas->cache_cap == 0u) {
        if (!glyph_cache_reserve(atlas, 1u)) {
            return;
        }
    }
    idx = (size_t)glyph_hash(ch, style) & (atlas->cache_cap - 1u);
    while (atlas->cache[idx].used) {
        if (atlas->cache[idx].style == style && atlas->cache[idx].ch == ch) {
            atlas->cache[idx].glyph = glyph;
            return;
        }
        idx = (idx + 1u) & (atlas->cache_cap - 1u);
    }
    atlas->cache[idx].used = 1u;
    atlas->cache[idx].style = style;
    atlas->cache[idx].ch = ch;
    atlas->cache[idx].glyph = glyph;
    atlas->cache_len += 1u;
}

static void reset_atlas(RudoFontAtlas *atlas) {
    memset(atlas->ascii_populated, 0, sizeof(atlas->ascii_populated));
    glyph_cache_clear(atlas);
    atlas->current_x = 0u;
    atlas->current_y = 0u;
    atlas->row_height = 0u;
}

static int ensure_fallback_font_capacity(RudoFontAtlas *atlas, size_t need) {
    return grow_array((void **)&atlas->fallback_fonts, sizeof(FtFont), &atlas->fallback_font_cap, need);
}

static void ensure_fallback_paths(RudoFontAtlas *atlas) {
    char **paths;
    size_t len;
    size_t i;
    if (atlas->fallback_paths_resolved) {
        return;
    }
    atlas->fallback_paths_resolved = 1;
    paths = fontconfig_sort_files(atlas->request, &len);
    for (i = 0u; i < len; i++) {
        if (paths[i] == NULL) {
            continue;
        }
        if (atlas->regular.path != NULL && strcmp(paths[i], atlas->regular.path) == 0) {
            continue;
        }
        append_unique_string(&atlas->fallback_paths, &atlas->fallback_path_len, &atlas->fallback_path_cap, paths[i]);
    }
    free_string_array(paths, len);
}

static void ensure_bold_font(RudoFontAtlas *atlas) {
    static const char *const styles[] = { "style=Bold" };
    char *path;
    if (atlas->bold_state != FONT_SLOT_UNRESOLVED) {
        return;
    }
    path = resolve_style_font_path(atlas->request, styles, sizeof(styles) / sizeof(styles[0]));
    if (path == NULL || (atlas->regular.path != NULL && strcmp(path, atlas->regular.path) == 0) ||
        !ft_font_load(&atlas->bold, path, atlas->font_size)) {
        atlas->bold_state = FONT_SLOT_FAILED;
        free(path);
        return;
    }
    atlas->bold_state = FONT_SLOT_READY;
    free(path);
}

static void ensure_italic_font(RudoFontAtlas *atlas) {
    static const char *const styles[] = { "style=Italic", "style=Oblique" };
    char *path;
    if (atlas->italic_state != FONT_SLOT_UNRESOLVED) {
        return;
    }
    path = resolve_style_font_path(atlas->request, styles, sizeof(styles) / sizeof(styles[0]));
    if (path == NULL || (atlas->regular.path != NULL && strcmp(path, atlas->regular.path) == 0) ||
        !ft_font_load(&atlas->italic, path, atlas->font_size)) {
        atlas->italic_state = FONT_SLOT_FAILED;
        free(path);
        return;
    }
    atlas->italic_state = FONT_SLOT_READY;
    free(path);
}

static void ensure_bold_italic_font(RudoFontAtlas *atlas) {
    static const char *const styles[] = { "style=Bold Italic", "style=Bold Oblique", "style=Italic Bold" };
    char *path;
    if (atlas->bold_italic_state != FONT_SLOT_UNRESOLVED) {
        return;
    }
    path = resolve_style_font_path(atlas->request, styles, sizeof(styles) / sizeof(styles[0]));
    if (path == NULL || (atlas->regular.path != NULL && strcmp(path, atlas->regular.path) == 0) ||
        !ft_font_load(&atlas->bold_italic, path, atlas->font_size)) {
        atlas->bold_italic_state = FONT_SLOT_FAILED;
        free(path);
        return;
    }
    atlas->bold_italic_state = FONT_SLOT_READY;
    free(path);
}

static FtFont *pick_styled_font(RudoFontAtlas *atlas, int bold, int italic) {
    if (bold && italic) {
        ensure_bold_italic_font(atlas);
        if (atlas->bold_italic_state == FONT_SLOT_READY) {
            return &atlas->bold_italic;
        }
        ensure_bold_font(atlas);
        if (atlas->bold_state == FONT_SLOT_READY) {
            return &atlas->bold;
        }
        ensure_italic_font(atlas);
        if (atlas->italic_state == FONT_SLOT_READY) {
            return &atlas->italic;
        }
        return &atlas->regular;
    }
    if (bold) {
        ensure_bold_font(atlas);
        if (atlas->bold_state == FONT_SLOT_READY) {
            return &atlas->bold;
        }
        return &atlas->regular;
    }
    if (italic) {
        ensure_italic_font(atlas);
        if (atlas->italic_state == FONT_SLOT_READY) {
            return &atlas->italic;
        }
        return &atlas->regular;
    }
    return &atlas->regular;
}

static FtFont *pick_font_with_fallback(RudoFontAtlas *atlas, uint32_t ch, int bold, int italic) {
    FtFont *styled;
    size_t i;
    styled = pick_styled_font(atlas, bold, italic);
    if (ch < 128u) {
        return styled;
    }
    if (ft_font_has_glyph(styled, ch)) {
        return styled;
    }
    if (ft_font_has_glyph(&atlas->regular, ch)) {
        return &atlas->regular;
    }
    for (i = 0u; i < atlas->fallback_font_len; i++) {
        if (ft_font_has_glyph(&atlas->fallback_fonts[i], ch)) {
            return &atlas->fallback_fonts[i];
        }
    }
    ensure_fallback_paths(atlas);
    while (atlas->next_fallback_path < atlas->fallback_path_len) {
        FtFont font;
        const char *path;
        path = atlas->fallback_paths[atlas->next_fallback_path++];
        ft_font_init(&font);
        if (!ft_font_load(&font, path, atlas->font_size)) {
            continue;
        }
        if (!ensure_fallback_font_capacity(atlas, atlas->fallback_font_len + 1u)) {
            ft_font_drop(&font);
            break;
        }
        atlas->fallback_fonts[atlas->fallback_font_len++] = font;
        if (ft_font_has_glyph(&atlas->fallback_fonts[atlas->fallback_font_len - 1u], ch)) {
            return &atlas->fallback_fonts[atlas->fallback_font_len - 1u];
        }
    }
    return styled;
}

static void copy_bitmap_rows(unsigned char *atlas_data, unsigned int atlas_x, unsigned int atlas_y, const FT_Bitmap *bitmap) {
    unsigned int row;
    if (bitmap == NULL || bitmap->buffer == NULL || bitmap->width == 0u || bitmap->rows == 0u) {
        return;
    }
    for (row = 0u; row < bitmap->rows; row++) {
        const unsigned char *src;
        unsigned char *dst;
        if (bitmap->pitch < 0) {
            src = bitmap->buffer + (ptrdiff_t)row * (ptrdiff_t)bitmap->pitch;
        } else {
            src = bitmap->buffer + (size_t)row * (size_t)bitmap->pitch;
        }
        dst = atlas_data + ((size_t)(atlas_y + row) * (size_t)ATLAS_WIDTH + (size_t)atlas_x);
        memcpy(dst, src, bitmap->width);
    }
}

static RudoGlyphInfo rasterize_glyph(RudoFontAtlas *atlas, uint32_t ch, int bold, int italic) {
    FtFont *font;
    FT_GlyphSlotRec *slot;
    FT_Bitmap *bitmap;
    unsigned int gw;
    unsigned int gh;
    unsigned int ax;
    unsigned int ay;
    RudoGlyphInfo info;
    info = empty_glyph_info();
    font = pick_font_with_fallback(atlas, ch, bold, italic);
    if (font == NULL || font->face == NULL || !ensure_ft()) {
        return info;
    }
    if (g_ft.Load_Char(font->face, (FT_ULong)ch, FT_LOAD_RENDER) != 0) {
        return info;
    }
    slot = font->face->glyph;
    if (slot == NULL) {
        return info;
    }
    bitmap = &slot->bitmap;
    gw = bitmap->width;
    gh = bitmap->rows;
    ax = 0u;
    ay = 0u;
    if (gw != 0u && gh != 0u) {
        if (atlas->current_x + gw > ATLAS_WIDTH) {
            atlas->current_y += atlas->row_height;
            atlas->current_x = 0u;
            atlas->row_height = 0u;
        }
        if (atlas->current_y + gh > ATLAS_HEIGHT) {
            reset_atlas(atlas);
        }
        if (atlas->current_x + gw > ATLAS_WIDTH) {
            atlas->current_y += atlas->row_height;
            atlas->current_x = 0u;
            atlas->row_height = 0u;
        }
        if (atlas->current_y + gh > ATLAS_HEIGHT) {
            return info;
        }
        ax = atlas->current_x;
        ay = atlas->current_y;
        copy_bitmap_rows(atlas->atlas_data, ax, ay, bitmap);
        atlas->current_x += gw + 1u;
        if (gh + 1u > atlas->row_height) {
            atlas->row_height = gh + 1u;
        }
    }
    info.u0 = (float)ax / (float)ATLAS_WIDTH;
    info.v0 = (float)ay / (float)ATLAS_HEIGHT;
    info.u1 = (float)(ax + gw) / (float)ATLAS_WIDTH;
    info.v1 = (float)(ay + gh) / (float)ATLAS_HEIGHT;
    info.width = (float)gw;
    info.height = (float)gh;
    info.offset_x = (float)slot->bitmap_left;
    info.offset_y = (float)(slot->bitmap_top - (int)gh);
    return info;
}

/* ── exported API ──────────────────────────────────────────────────────── */

void *rudo_font_atlas_new(float font_size, const char *preferred_family) {
    RudoFontAtlas *atlas;
    char *request;
    char *regular_path;
    if (font_size < 1.0f) {
        font_size = 1.0f;
    }
    atlas = (RudoFontAtlas *)calloc(1u, sizeof(RudoFontAtlas));
    if (atlas == NULL) {
        return NULL;
    }
    ft_font_init(&atlas->regular);
    ft_font_init(&atlas->bold);
    ft_font_init(&atlas->italic);
    ft_font_init(&atlas->bold_italic);
    request = trimmed_copy(preferred_family);
    if (request == NULL) {
        free(atlas);
        return NULL;
    }
    regular_path = resolve_regular_font_path(request);
    if (regular_path == NULL || !ft_font_load(&atlas->regular, regular_path, font_size)) {
        free(regular_path);
        free(request);
        free(atlas);
        fprintf(stderr, "[ERROR] No usable font found for '%s'\n", preferred_family != NULL ? preferred_family : DEFAULT_FONT_FAMILY);
        return NULL;
    }
    atlas->atlas_data = (unsigned char *)calloc((size_t)ATLAS_WIDTH * (size_t)ATLAS_HEIGHT, 1u);
    if (atlas->atlas_data == NULL) {
        free(regular_path);
        ft_font_drop(&atlas->regular);
        free(request);
        free(atlas);
        return NULL;
    }
    atlas->request = request;
    atlas->font_size = font_size;
    atlas->cell_width = compute_cell_width(&atlas->regular);
    atlas->cell_height = compute_cell_height(&atlas->regular);
    {
        float asc;
        float desc;
        float height;
        ft_font_line_metrics(&atlas->regular, &asc, &desc, &height);
        atlas->baseline = asc > 0.0f ? asc : atlas->cell_height * FALLBACK_BASELINE_RATIO;
    }
    free(regular_path);
    return atlas;
}

void rudo_font_atlas_free(void *ptr) {
    RudoFontAtlas *atlas;
    size_t i;
    if (ptr == NULL) {
        return;
    }
    atlas = (RudoFontAtlas *)ptr;
    ft_font_drop(&atlas->regular);
    ft_font_drop(&atlas->bold);
    ft_font_drop(&atlas->italic);
    ft_font_drop(&atlas->bold_italic);
    for (i = 0u; i < atlas->fallback_font_len; i++) {
        ft_font_drop(&atlas->fallback_fonts[i]);
    }
    free(atlas->fallback_fonts);
    free_string_array(atlas->fallback_paths, atlas->fallback_path_len);
    free(atlas->atlas_data);
    free(atlas->cache);
    free(atlas->request);
    free(atlas);
}

float rudo_font_atlas_cell_width(const void *ptr) {
    const RudoFontAtlas *atlas = (const RudoFontAtlas *)ptr;
    return atlas != NULL ? atlas->cell_width : 0.0f;
}

float rudo_font_atlas_cell_height(const void *ptr) {
    const RudoFontAtlas *atlas = (const RudoFontAtlas *)ptr;
    return atlas != NULL ? atlas->cell_height : 0.0f;
}

float rudo_font_atlas_baseline(const void *ptr) {
    const RudoFontAtlas *atlas = (const RudoFontAtlas *)ptr;
    return atlas != NULL ? atlas->baseline : 0.0f;
}

RudoGlyphInfo rudo_font_atlas_get_glyph(void *ptr, uint32_t ch, int bold, int italic) {
    RudoFontAtlas *atlas;
    RudoGlyphInfo glyph;
    unsigned char style;
    atlas = (RudoFontAtlas *)ptr;
    if (atlas == NULL) {
        return empty_glyph_info();
    }
    if (ch >= 32u && ch <= 126u) {
        unsigned int idx = ascii_cache_idx((unsigned char)ch, bold, italic);
        if (!atlas->ascii_populated[idx]) {
            atlas->ascii_cache[idx] = rasterize_glyph(atlas, ch, bold, italic);
            atlas->ascii_populated[idx] = 1u;
        }
        return atlas->ascii_cache[idx];
    }
    style = (unsigned char)(((bold != 0) << 1) | (italic != 0));
    if (glyph_cache_lookup(atlas, ch, style, &glyph)) {
        return glyph;
    }
    glyph = rasterize_glyph(atlas, ch, bold, italic);
    glyph_cache_insert(atlas, ch, style, glyph);
    return glyph;
}

const unsigned char *rudo_font_atlas_data(const void *ptr, size_t *len_out) {
    const RudoFontAtlas *atlas = (const RudoFontAtlas *)ptr;
    if (len_out != NULL) {
        *len_out = atlas != NULL ? (size_t)ATLAS_WIDTH * (size_t)ATLAS_HEIGHT : 0u;
    }
    return atlas != NULL ? atlas->atlas_data : NULL;
}

uint32_t rudo_font_atlas_width(const void *ptr) {
    (void)ptr;
    return ATLAS_WIDTH;
}

uint32_t rudo_font_atlas_height(const void *ptr) {
    (void)ptr;
    return ATLAS_HEIGHT;
}
