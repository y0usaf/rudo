#ifndef RUDO_CLI_H
#define RUDO_CLI_H

#include "rudo/common.h"

#ifdef __cplusplus
extern "C" {
#endif

typedef struct {
    char *app_id;
    char *title;
    char **command;
    size_t command_len;
} rudo_cli_args;

void rudo_cli_args_init(rudo_cli_args *args);
void rudo_cli_args_destroy(rudo_cli_args *args);
bool rudo_cli_parse(rudo_cli_args *out, int argc, char **argv, int *exit_code);
const char *rudo_cli_usage(void);

#ifdef __cplusplus
}
#endif

#endif
