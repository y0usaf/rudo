#define _GNU_SOURCE
#include "rudo/core.h"
#include "rudo/common.h"
#include "rudo/defaults.h"
#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/poll.h>
#include <sys/prctl.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <termios.h>
#include <unistd.h>

struct rudo_pty {
    int master_fd;
    pid_t child_pid;
    int exit_status;
    bool child_exited;
    bool child_reaped;
};

static void close_child_fds_from(int first_fd) {
#ifdef SYS_close_range
    if (syscall(SYS_close_range, (unsigned)first_fd, ~0u, 0u) == 0) return;
#endif
    long max_fd = sysconf(_SC_OPEN_MAX);
    int fd;
    if (max_fd < first_fd) max_fd = 1024;
    for (fd = first_fd; fd < max_fd; ++fd) close(fd);
}

static bool rudo_pty_poll_child(rudo_pty *pty) {
    pid_t rc;
    int status;
    if (!pty || pty->child_pid <= 0 || pty->child_reaped) return pty && pty->child_exited;
    do {
        rc = waitpid(pty->child_pid, &status, WNOHANG);
    } while (rc < 0 && errno == EINTR);
    if (rc == pty->child_pid) {
        pty->child_exited = true;
        pty->child_reaped = true;
        pty->exit_status = status;
        pty->child_pid = -1;
        return true;
    }
    if (rc < 0 && errno == ECHILD) {
        pty->child_exited = true;
        pty->child_reaped = true;
        pty->exit_status = 0;
        pty->child_pid = -1;
        return true;
    }
    return pty->child_exited;
}

static void rudo_pty_reap_with_timeout(rudo_pty *pty, int timeout_ms) {
    int waited_ms = 0;
    int status;
    pid_t rc;
    if (!pty || pty->child_pid <= 0 || pty->child_reaped) return;
    while (waited_ms < timeout_ms) {
        do {
            rc = waitpid(pty->child_pid, &status, WNOHANG);
        } while (rc < 0 && errno == EINTR);
        if (rc == pty->child_pid) {
            pty->child_exited = true;
            pty->child_reaped = true;
            pty->exit_status = status;
            pty->child_pid = -1;
            return;
        }
        if (rc < 0 && errno == ECHILD) {
            pty->child_exited = true;
            pty->child_reaped = true;
            pty->exit_status = 0;
            pty->child_pid = -1;
            return;
        }
        if (rc < 0) return;
        usleep(10000);
        waited_ms += 10;
    }
}

rudo_pty *rudo_pty_spawn(uint16_t cols, uint16_t rows, const rudo_pty_spawn_config *config) {
    int master = posix_openpt(O_RDWR | O_NOCTTY | O_CLOEXEC);
    char slave_name[256];
    pid_t pid;
    struct winsize ws;
    rudo_pty *pty;
    char **argv = NULL;
    size_t argc = 0, i;
    if (master < 0 || grantpt(master) != 0 || unlockpt(master) != 0 || ptsname_r(master, slave_name, sizeof(slave_name)) != 0) {
        if (master >= 0) close(master);
        return NULL;
    }
    ws.ws_row = rows;
    ws.ws_col = cols;
    ws.ws_xpixel = 0;
    ws.ws_ypixel = 0;
    argc = config && config->command_len ? config->command_len : 1u;
    argv = rudo_calloc(argc + 1u, sizeof(*argv));
    if (config && config->command_len) {
        for (i = 0; i < argc; ++i) argv[i] = config->command[i];
    } else {
        const char *shell = getenv("SHELL");
        argv[0] = (char *)((shell && *shell) ? shell : (config && config->shell_fallback ? config->shell_fallback : "/bin/sh"));
    }
    pid = fork();
    if (pid == 0) {
        int slave;
        close(master);
        if (setsid() < 0) _exit(1);
        slave = open(slave_name, O_RDWR | O_NOCTTY);
        if (slave < 0) _exit(1);
        if (ioctl(slave, TIOCSCTTY, 0) < 0) _exit(1);
        ioctl(slave, TIOCSWINSZ, &ws);
        if (dup2(slave, 0) < 0 || dup2(slave, 1) < 0 || dup2(slave, 2) < 0) _exit(1);
        if (slave > 2) close(slave);
        close_child_fds_from(3);
        setenv("TERM", config && config->term ? config->term : RUDO_DEFAULT_TERM, 1);
        setenv("COLORTERM", config && config->colorterm ? config->colorterm : RUDO_DEFAULT_COLORTERM, 1);
        prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
        prctl(PR_SET_PDEATHSIG, SIGHUP, 0, 0, 0);
        execvp(argv[0], argv);
        _exit(127);
    }
    free(argv);
    if (pid < 0) {
        close(master);
        return NULL;
    }
    fcntl(master, F_SETFL, fcntl(master, F_GETFL) | O_NONBLOCK);
    pty = rudo_calloc(1, sizeof(*pty));
    pty->master_fd = master;
    pty->child_pid = pid;
    pty->exit_status = 0;
    return pty;
}

void rudo_pty_free(rudo_pty *pty) {
    if (!pty) return;
    if (pty->child_pid > 0) kill(-pty->child_pid, SIGHUP);
    if (pty->master_fd >= 0) {
        close(pty->master_fd);
        pty->master_fd = -1;
    }
    rudo_pty_reap_with_timeout(pty, 200);
    if (pty->child_pid > 0 && !pty->child_reaped) {
        kill(-pty->child_pid, SIGTERM);
        rudo_pty_reap_with_timeout(pty, 300);
    }
    if (pty->child_pid > 0 && !pty->child_reaped) {
        kill(-pty->child_pid, SIGKILL);
        rudo_pty_reap_with_timeout(pty, 500);
    }
    free(pty);
}

ssize_t rudo_pty_try_read(rudo_pty *pty, void *buf, size_t len) {
    ssize_t n;
    if (!pty || pty->master_fd < 0 || !buf || !len) return -1;
    do n = read(pty->master_fd, buf, len); while (n < 0 && errno == EINTR);
    if (n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
        rudo_pty_poll_child(pty);
        return 0;
    }
    if (n == 0) {
        rudo_pty_poll_child(pty);
        return -1;
    }
    if (n < 0) {
        if (errno == EIO) {
            rudo_pty_poll_child(pty);
        }
        return -1;
    }
    return n;
}

ssize_t rudo_pty_write(rudo_pty *pty, const void *buf, size_t len) {
    size_t off = 0;
    if (!pty || pty->master_fd < 0 || (!buf && len)) return -1;
    while (off < len) {
        ssize_t n = write(pty->master_fd, (const char *)buf + off, len - off);
        if (n > 0) {
            off += (size_t)n;
            continue;
        }
        if (n < 0 && errno == EINTR) continue;
        if (n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
            struct pollfd pfd = { .fd = pty->master_fd, .events = POLLOUT, .revents = 0 };
            if (poll(&pfd, 1, -1) < 0 && errno != EINTR) return -1;
            continue;
        }
        if (n < 0 && (errno == EPIPE || errno == EIO)) rudo_pty_poll_child(pty);
        return -1;
    }
    return (ssize_t)off;
}

bool rudo_pty_resize(rudo_pty *pty, uint16_t cols, uint16_t rows) {
    struct winsize ws;
    if (!pty || pty->master_fd < 0) return false;
    ws.ws_row = rows;
    ws.ws_col = cols;
    ws.ws_xpixel = 0;
    ws.ws_ypixel = 0;
    return ioctl(pty->master_fd, TIOCSWINSZ, &ws) == 0;
}

int rudo_pty_master_fd(const rudo_pty *pty) { return pty ? pty->master_fd : -1; }
bool rudo_pty_child_exited(rudo_pty *pty) { return rudo_pty_poll_child(pty); }
bool rudo_pty_take_exit_status(rudo_pty *pty, int *status) { if (!pty) return false; rudo_pty_poll_child(pty); if (!pty->child_exited) return false; if (status) *status = pty->exit_status; return true; }
