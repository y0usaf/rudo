#include "rudo/cli.h"
#include "rudo/defaults.h"

static const char usage_text[] =
    "Usage: " RUDO_APP_NAME " [OPTIONS] [--] [command [ARGS...]]\n\n"
    "Options:\n"
    "  -a, --app-id ID     Set the Wayland app-id (default: from config or \"" RUDO_APP_NAME "\")\n"
    "  -t, --title TITLE   Set the initial window title\n"
    "  -e                  Ignored (xterm compat); stops option parsing\n"
    "      --              Stop option parsing; remaining args become the command\n"
    "  -h, --help          Print this help message and exit\n"
    "  -v, --version       Print version and exit\n";

void rudo_cli_args_init(rudo_cli_args *args) { memset(args, 0, sizeof(*args)); }

void rudo_cli_args_destroy(rudo_cli_args *args) {
    size_t i;
    free(args->app_id);
    free(args->title);
    for (i = 0; i < args->command_len; ++i) free(args->command[i]);
    free(args->command);
    memset(args, 0, sizeof(*args));
}

const char *rudo_cli_usage(void) { return usage_text; }

static bool push_command(rudo_cli_args *out, const char *arg) {
    if (out->command_len % 8 == 0) out->command = rudo_realloc(out->command, (out->command_len + 8) * sizeof(*out->command));
    out->command[out->command_len++] = rudo_strdup(arg);
    return true;
}

static const char *expect_inline_value(const char *flag, const char *value) {
    if (!value || !*value) {
        fprintf(stderr, "%s: option '%s' requires a non-empty value\n", RUDO_APP_NAME, flag);
        return NULL;
    }
    return value;
}

bool rudo_cli_parse(rudo_cli_args *out, int argc, char **argv, int *exit_code) {
    int i;
    rudo_cli_args_init(out);
    if (exit_code) *exit_code = -1;
    for (i = 1; i < argc; ++i) {
        const char *arg = argv[i];
        if (rudo_streq(arg, "-h") || rudo_streq(arg, "--help")) {
            fputs(rudo_cli_usage(), stdout);
            if (exit_code) *exit_code = 0;
            return false;
        } else if (rudo_streq(arg, "-v") || rudo_streq(arg, "--version")) {
            printf(RUDO_APP_NAME " " RUDO_VERSION "\n");
            if (exit_code) *exit_code = 0;
            return false;
        } else if (rudo_streq(arg, "-a") || rudo_streq(arg, "--app-id")) {
            if (++i >= argc || !expect_inline_value(arg, argv[i])) goto need_value;
            out->app_id = rudo_strdup(argv[i]);
        } else if (rudo_streq(arg, "-t") || rudo_streq(arg, "--title")) {
            if (++i >= argc || !expect_inline_value(arg, argv[i])) goto need_value;
            out->title = rudo_strdup(argv[i]);
        } else if (!strncmp(arg, "--app-id=", 9)) {
            const char *v = expect_inline_value("--app-id", arg + 9); if (!v) goto need_value; out->app_id = rudo_strdup(v);
        } else if (!strncmp(arg, "--title=", 8)) {
            const char *v = expect_inline_value("--title", arg + 8); if (!v) goto need_value; out->title = rudo_strdup(v);
        } else if (rudo_streq(arg, "--") || rudo_streq(arg, "-e")) {
            for (++i; i < argc; ++i) push_command(out, argv[i]);
            return true;
        } else if (arg[0] == '-') {
            fprintf(stderr, "%s: unknown option '%s'\n%s", RUDO_APP_NAME, arg, rudo_cli_usage());
            if (exit_code) *exit_code = 1;
            return false;
        } else {
            for (; i < argc; ++i) push_command(out, argv[i]);
            return true;
        }
    }
    return true;
need_value:
    fprintf(stderr, "%s: option '%s' requires a value\n", RUDO_APP_NAME, argv[i - 1]);
    if (exit_code) *exit_code = 1;
    return false;
}
