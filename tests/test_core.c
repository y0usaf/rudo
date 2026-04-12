#include "rudo/common.h"
#include "rudo/core.h"
#include "rudo/input.h"

#include <assert.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

static void test_path_join_helpers(void) {
    char *joined = rudo_path_join3("/tmp", "rudo", "state");
    assert(strcmp(joined, "/tmp/rudo/state") == 0);
    free(joined);
    assert(strcmp(rudo_basename_const("/a/b/c.txt"), "c.txt") == 0);
}

static void test_mouse_sgr_encoding(void) {
    rudo_mouse_state state = { .protocol = RUDO_MOUSE_PROTOCOL_VT200, .encoding = RUDO_MOUSE_ENCODING_SGR };
    char buf[64];
    size_t n = rudo_mouse_encode_press(&state, 0, 4, 9, 2, buf);
    assert(n > 0);
    assert(strcmp(buf, "\033[<4;10;3M") == 0);
    n = rudo_mouse_encode_release(&state, 0, 4, 9, 2, buf);
    assert(strcmp(buf, "\033[<4;10;3m") == 0);
}

static void test_pty_spawn_and_exit_status(void) {
    rudo_pty_spawn_config cfg;
    char *argv[] = { "/bin/sh", "-c", "printf ok; exit 7", NULL };
    rudo_pty *pty;
    char buf[32];
    size_t used = 0;
    int status = 0;
    int tries;

    memset(&cfg, 0, sizeof(cfg));
    cfg.command = argv;
    cfg.command_len = 3;
    cfg.shell_fallback = "/bin/sh";

    pty = rudo_pty_spawn(80, 24, &cfg);
    assert(pty != NULL);

    for (tries = 0; tries < 200 && used < 2; ++tries) {
        ssize_t n = rudo_pty_try_read(pty, buf + used, sizeof(buf) - 1 - used);
        if (n > 0) used += (size_t)n;
        else { struct timespec ts = {0, 10000000}; nanosleep(&ts, NULL); }
    }
    buf[used] = '\0';
    assert(strstr(buf, "ok") != NULL);

    for (tries = 0; tries < 200 && !rudo_pty_take_exit_status(pty, &status); ++tries) { struct timespec ts = {0, 10000000}; nanosleep(&ts, NULL); }
    assert(rudo_pty_take_exit_status(pty, &status));
    assert(WIFEXITED(status));
    assert(WEXITSTATUS(status) == 7);

    rudo_pty_free(pty);
}


static void test_pty_exec_failure_exit_status(void) {
    rudo_pty_spawn_config cfg;
    char *argv[] = { "/definitely/not/a/real/command", NULL };
    rudo_pty *pty;
    int status = 0;
    int tries;

    memset(&cfg, 0, sizeof(cfg));
    cfg.command = argv;
    cfg.command_len = 1;
    cfg.shell_fallback = "/bin/sh";

    pty = rudo_pty_spawn(80, 24, &cfg);
    assert(pty != NULL);

    for (tries = 0; tries < 200 && !rudo_pty_take_exit_status(pty, &status); ++tries) {
        struct timespec ts = {0, 10000000};
        nanosleep(&ts, NULL);
    }
    assert(rudo_pty_take_exit_status(pty, &status));
    assert(WIFEXITED(status));
    assert(WEXITSTATUS(status) == 127);
    assert(rudo_exit_code_from_wait_status(status) == 127);

    rudo_pty_free(pty);
}

static void test_wait_status_exit_code_mapping(void) {
    assert(rudo_exit_code_from_wait_status(7 << 8) == 7);
    assert(rudo_exit_code_from_wait_status(SIGTERM) == 128 + SIGTERM);
}

static void test_runtime_mode_encoding_helpers(void) {
    rudo_mouse_state none = { .protocol = RUDO_MOUSE_PROTOCOL_NONE, .encoding = RUDO_MOUSE_ENCODING_X10 };
    rudo_mouse_state vt200 = { .protocol = RUDO_MOUSE_PROTOCOL_VT200, .encoding = RUDO_MOUSE_ENCODING_X10 };
    rudo_mouse_state button = { .protocol = RUDO_MOUSE_PROTOCOL_BUTTON_EVENT, .encoding = RUDO_MOUSE_ENCODING_SGR };
    rudo_mouse_state any = { .protocol = RUDO_MOUSE_PROTOCOL_ANY_EVENT, .encoding = RUDO_MOUSE_ENCODING_SGR };
    char buf[64];

    assert(!rudo_mouse_state_is_active(&none));
    assert(rudo_mouse_state_is_active(&vt200));
    assert(rudo_mouse_state_is_active(&button));
    assert(rudo_mouse_state_is_active(&any));

    assert(rudo_mouse_encode_press(&none, 0, 0, 1, 1, buf) == 0);
    assert(rudo_mouse_encode_press(&vt200, 0, 0, 1, 1, buf) == 6);
    assert(buf[0] == '\033' && buf[1] == '[' && buf[2] == 'M');
    assert((unsigned char)buf[3] == 32);
    assert((unsigned char)buf[4] == 34);
    assert((unsigned char)buf[5] == 34);
    assert(rudo_mouse_encode_drag(&button, 1, 4, 2, 3, buf) > 0);
    assert(strcmp(buf, "\033[<37;3;4M") == 0);
    assert(rudo_mouse_encode_move(&any, 8, 4, 5, buf) > 0);
    assert(strcmp(buf, "\033[<43;5;6M") == 0);
}

int main(void) {
    test_path_join_helpers();
    test_mouse_sgr_encoding();
    test_pty_spawn_and_exit_status();
    test_pty_exec_failure_exit_status();
    test_wait_status_exit_code_mapping();
    test_runtime_mode_encoding_helpers();
    return 0;
}
