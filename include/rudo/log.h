#ifndef RUDO_LOG_H
#define RUDO_LOG_H

#include "rudo/common.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    RUDO_LOG_ERROR = 0,
    RUDO_LOG_WARN = 1,
    RUDO_LOG_INFO = 2,
} rudo_log_level;

bool rudo_log_info_enabled(void);
void rudo_log_emit(rudo_log_level level, const char *fmt, ...) RUDO_PRINTF(2, 3);

#define rudo_error_log(...) rudo_log_emit(RUDO_LOG_ERROR, __VA_ARGS__)
#define rudo_warn_log(...) rudo_log_emit(RUDO_LOG_WARN, __VA_ARGS__)
#define rudo_info_log(...) do { if (rudo_log_info_enabled()) rudo_log_emit(RUDO_LOG_INFO, __VA_ARGS__); } while (0)

#ifdef __cplusplus
}
#endif

#endif
