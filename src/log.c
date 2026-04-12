#include "rudo/log.h"

#include <stdarg.h>

static bool env_enabled(const char *k) {
    const char *v = getenv(k);
    if (!v) return false;
    return rudo_streq(v, "1") || rudo_streq_nocase(v, "true") || rudo_streq_nocase(v, "yes") || rudo_streq_nocase(v, "on");
}

bool rudo_log_info_enabled(void) {
#ifndef NDEBUG
    return true;
#else
    static int cached = -1;
    if (cached < 0) cached = env_enabled("RUDO_LOG_INFO") || env_enabled("TERMVIDE_LOG_INFO");
    return cached != 0;
#endif
}

void rudo_log_emit(rudo_log_level level, const char *fmt, ...) {
    static const char *const prefix[] = {"[ERROR] ", "[WARN] ", "[INFO] "};
    va_list ap;
    fputs(prefix[level], stderr);
    va_start(ap, fmt);
    vfprintf(stderr, fmt, ap);
    va_end(ap);
    fputc('\n', stderr);
}
