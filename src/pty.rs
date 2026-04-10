//! Direct PTY handling using raw libc calls for zero-overhead Linux PTY operations.
//! No portable-pty wrapper - raw syscalls for maximum speed.

use std::ffi::{CStr, CString};
use std::fmt;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

use crate::contracts::CheckInvariant;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const PTY_OPEN_FLAGS: libc::c_int = libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC;
const PTY_NAME_BUFFER_SIZE: usize = 512;

const TERM_ENV_VAR: &str = "TERM";
const COLORTERM_ENV_VAR: &str = "COLORTERM";
const SHELL_ENV_VAR: &str = "SHELL";
const CHILD_STDIO_FD_CEILING: libc::c_int = 3;

#[inline]
unsafe fn close_child_fds_from(first_fd: libc::c_int) {
    // Try close_range(2) syscall first — single syscall to close all fds
    // in [first_fd, u32::MAX]. Available on Linux 5.9+.
    #[cfg(target_os = "linux")]
    {
        let ret = unsafe {
            libc::syscall(
                libc::SYS_close_range,
                first_fd as libc::c_uint,
                libc::c_uint::MAX,
                0 as libc::c_uint,
            )
        };
        if ret == 0 {
            return;
        }
    }

    // Fallback: iterate up to sysconf(_SC_OPEN_MAX).
    let max_fd = match unsafe { libc::sysconf(libc::_SC_OPEN_MAX) } {
        v if v > first_fd as libc::c_long => v as libc::c_int,
        _ => 1024,
    };
    let mut fd = first_fd;
    while fd < max_fd {
        unsafe {
            libc::close(fd);
        }
        fd += 1;
    }
}

#[derive(Debug)]
struct PtyError(String);
impl fmt::Display for PtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PtyError {}

#[derive(Clone, Debug)]
pub struct PtySpawnConfig<'a> {
    pub term: &'a str,
    pub colorterm: &'a str,
    pub shell_fallback: &'a str,
    pub command: &'a [String],
}

/// A PTY master with the child process.
pub struct Pty {
    master: OwnedFd,
    child_pid: i32,
}

#[allow(dead_code)]
impl Pty {
    /// Spawn a new PTY with a shell process.
    pub fn spawn(cols: u16, rows: u16, config: &PtySpawnConfig<'_>) -> Result<Self> {
        requires!(cols > 0 && rows > 0);
        // SAFETY: posix_openpt returns a valid fd or -1. We check < 0 before use.
        let master_raw = unsafe { libc::posix_openpt(PTY_OPEN_FLAGS) };
        if master_raw < 0 {
            return Err(Box::new(PtyError(format!(
                "openpt failed: {}",
                std::io::Error::last_os_error()
            ))));
        }
        // SAFETY: master_raw is a valid fd from posix_openpt (checked above).
        let master = unsafe { OwnedFd::from_raw_fd(master_raw) };

        // SAFETY: master is a valid PTY master fd.
        if unsafe { libc::grantpt(master.as_raw_fd()) } != 0 {
            return Err(Box::new(PtyError(format!(
                "grantpt failed: {}",
                std::io::Error::last_os_error()
            ))));
        }
        // SAFETY: master is a valid, granted PTY master fd.
        if unsafe { libc::unlockpt(master.as_raw_fd()) } != 0 {
            return Err(Box::new(PtyError(format!(
                "unlockpt failed: {}",
                std::io::Error::last_os_error()
            ))));
        }

        let slave_name = {
            let mut buf = [0u8; PTY_NAME_BUFFER_SIZE];
            // SAFETY: ptsname_r writes into our stack buffer with bounded length,
            // and master_raw is a valid PTY master fd. Using ptsname_r instead of
            // ptsname avoids the thread-safety issue of the static buffer.
            let rc =
                unsafe { libc::ptsname_r(master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if rc != 0 {
                return Err(Box::new(PtyError(format!(
                    "ptsname_r failed: {}",
                    std::io::Error::from_raw_os_error(rc)
                ))));
            }
            // SAFETY: ptsname_r writes a null-terminated string into buf on success.
            let cstr = unsafe { CStr::from_ptr(buf.as_ptr().cast()) };
            cstr.to_bytes().to_vec()
        };

        let winsize = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let slave_cstr = CString::new(slave_name).map_err(|_| {
            Box::new(PtyError("slave PTY path contained NUL byte".into()))
                as Box<dyn std::error::Error>
        })?;
        let term = CString::new(config.term).map_err(|_| {
            Box::new(PtyError("TERM contained NUL byte".into())) as Box<dyn std::error::Error>
        })?;
        let term_key = CString::new(TERM_ENV_VAR).map_err(|_| {
            Box::new(PtyError("TERM key contained NUL byte".into())) as Box<dyn std::error::Error>
        })?;
        let colorterm = CString::new(config.colorterm).map_err(|_| {
            Box::new(PtyError("COLORTERM contained NUL byte".into())) as Box<dyn std::error::Error>
        })?;
        let colorterm_key = CString::new(COLORTERM_ENV_VAR).map_err(|_| {
            Box::new(PtyError("COLORTERM key contained NUL byte".into()))
                as Box<dyn std::error::Error>
        })?;
        let command_cstrs: Vec<CString> = if config.command.is_empty() {
            let shell =
                std::env::var(SHELL_ENV_VAR).unwrap_or_else(|_| config.shell_fallback.to_string());
            vec![CString::new(shell).map_err(|_| {
                Box::new(PtyError("shell path contained NUL byte".into()))
                    as Box<dyn std::error::Error>
            })?]
        } else {
            config
                .command
                .iter()
                .map(|s| {
                    CString::new(s.as_str()).map_err(|_| {
                        Box::new(PtyError("command contained NUL byte".into()))
                            as Box<dyn std::error::Error>
                    })
                })
                .collect::<Result<Vec<_>>>()?
        };
        let mut command_argv: Vec<*const libc::c_char> =
            command_cstrs.iter().map(|c| c.as_ptr()).collect();
        command_argv.push(std::ptr::null());

        // SAFETY: fork() is safe to call; we handle both parent (pid > 0) and child (pid == 0).
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(Box::new(PtyError(format!(
                "fork failed: {}",
                std::io::Error::last_os_error()
            ))));
        }

        if pid == 0 {
            // Child process
            // SAFETY: Child process after fork. We close the inherited PTY master,
            // create a fresh session, wire the slave PTY to stdio, set the small
            // fixed environment overrides, and exec the already-prepared argv.
            // If any step fails, _exit(1) terminates without running parent-side
            // destructors in the forked child.
            unsafe {
                libc::close(master.as_raw_fd());
                if libc::setsid() < 0 {
                    libc::_exit(1);
                }

                let slave_fd = libc::open(slave_cstr.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
                if slave_fd < 0 {
                    libc::_exit(1);
                }

                if libc::ioctl(slave_fd, libc::TIOCSCTTY, 0) < 0 {
                    libc::_exit(1);
                }

                if libc::ioctl(
                    slave_fd,
                    libc::TIOCSWINSZ,
                    &winsize as *const _ as *const libc::c_void,
                ) < 0
                {
                    libc::_exit(1);
                }

                if libc::dup2(slave_fd, 0) < 0
                    || libc::dup2(slave_fd, 1) < 0
                    || libc::dup2(slave_fd, 2) < 0
                {
                    libc::_exit(1);
                }

                if slave_fd > 2 {
                    libc::close(slave_fd);
                }
                close_child_fds_from(CHILD_STDIO_FD_CEILING);

                if libc::setenv(term_key.as_ptr(), term.as_ptr(), 1) != 0 {
                    libc::_exit(1);
                }
                if libc::setenv(colorterm_key.as_ptr(), colorterm.as_ptr(), 1) != 0 {
                    libc::_exit(1);
                }

                libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGHUP, 0, 0, 0);

                libc::execvp(command_cstrs[0].as_ptr(), command_argv.as_ptr());
                libc::_exit(1);
            }
        }

        let child_pid = pid;

        // Parent - set master to non-blocking
        // SAFETY: master fd is valid; F_GETFL/F_SETFL are standard fcntl operations.
        unsafe {
            let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            if flags < 0 {
                return Err(Box::new(PtyError(format!(
                    "fcntl(F_GETFL) failed: {}",
                    std::io::Error::last_os_error()
                ))));
            }
            if libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                return Err(Box::new(PtyError(format!(
                    "fcntl(F_SETFL) failed: {}",
                    std::io::Error::last_os_error()
                ))));
            }
        }

        let result = Self { master, child_pid };
        debug_check_invariant!(result);
        Ok(result)
    }

    /// Try to read from the PTY (non-blocking).
    #[inline]
    pub fn try_read(&self, buf: &mut [u8]) -> Result<usize> {
        requires!(!buf.is_empty());
        loop {
            // SAFETY: buf is a valid mutable slice; read writes at most buf.len() bytes.
            // The fd is the PTY master owned by self.
            let n =
                unsafe { libc::read(self.master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n >= 0 {
                return Ok(n as usize);
            }

            // SAFETY: __errno_location returns a valid pointer to the thread-local errno.
            let errno = unsafe { *libc::__errno_location() };
            if errno == libc::EINTR {
                continue;
            }
            if errno == libc::EAGAIN || errno == libc::EWOULDBLOCK {
                return Ok(0);
            }
            return Err(Box::new(PtyError(format!(
                "read failed: {}",
                std::io::Error::from_raw_os_error(errno)
            ))));
        }
    }

    /// Write the full buffer to the PTY or return an error.
    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        let mut written = 0;

        while written < buf.len() {
            invariant!(written <= buf.len());
            // SAFETY: buf[written..] is a valid slice; write reads at most the remaining bytes.
            // The fd is the PTY master owned by self.
            let n = unsafe {
                libc::write(
                    self.master.as_raw_fd(),
                    buf[written..].as_ptr().cast(),
                    buf.len() - written,
                )
            };
            if n > 0 {
                written += n as usize;
                continue;
            }
            if n == 0 {
                return Err(Box::new(PtyError(format!(
                    "write failed after writing {written} of {} bytes: write returned 0",
                    buf.len()
                ))));
            }

            // SAFETY: __errno_location returns a valid pointer to the thread-local errno.
            let errno = unsafe { *libc::__errno_location() };
            if errno == libc::EINTR {
                continue;
            }
            if errno == libc::EAGAIN || errno == libc::EWOULDBLOCK {
                let mut pollfd = libc::pollfd {
                    fd: self.master.as_raw_fd(),
                    events: libc::POLLOUT,
                    revents: 0,
                };

                loop {
                    let rc = unsafe { libc::poll(&mut pollfd, 1, -1) };
                    if rc > 0 {
                        if (pollfd.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL)) != 0
                        {
                            return Err(Box::new(PtyError(format!(
                                "write failed after writing {written} of {} bytes: poll returned revents=0x{:x}",
                                buf.len(),
                                pollfd.revents
                            ))));
                        }
                        break;
                    }
                    if rc == 0 {
                        continue;
                    }

                    let poll_errno = unsafe { *libc::__errno_location() };
                    if poll_errno == libc::EINTR {
                        continue;
                    }
                    return Err(Box::new(PtyError(format!(
                        "write failed after writing {written} of {} bytes: {}",
                        buf.len(),
                        std::io::Error::from_raw_os_error(poll_errno)
                    ))));
                }
                continue;
            }
            return Err(Box::new(PtyError(format!(
                "write failed after writing {written} of {} bytes: {}",
                buf.len(),
                std::io::Error::from_raw_os_error(errno)
            ))));
        }

        Ok(written)
    }

    /// Resize the PTY.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        requires!(cols > 0 && rows > 0);
        let winsize = libc::winsize {
            ws_row: rows,
            ws_col: cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        unsafe {
            // SAFETY: master fd is valid; winsize is a stack-allocated struct
            // passed by const pointer. TIOCSWINSZ is a standard PTY ioctl.
            if libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &winsize) < 0 {
                return Err(Box::new(PtyError(format!(
                    "TIOCSWINSZ failed: {}",
                    std::io::Error::last_os_error()
                ))));
            }
        }
        Ok(())
    }

    pub fn master_fd(&self) -> BorrowedFd<'_> {
        self.master.as_fd()
    }
}

/// Install a process-wide SIGCHLD handler that automatically reaps all child
/// processes.  This prevents zombies without ever blocking on waitpid in Drop.
/// Call once at startup (idempotent – uses an atomic flag).
pub fn install_sigchld_reaper() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    unsafe {
        // SAFETY: sa is fully zero-initialised then we set the handler and
        // SA_RESTART | SA_NOCLDSTOP flags. sigaction is async-signal-safe.
        let mut sa: libc::sigaction = std::mem::zeroed();
        sa.sa_sigaction = sigchld_handler as *const () as usize;
        sa.sa_flags = libc::SA_RESTART | libc::SA_NOCLDSTOP;
        libc::sigaction(libc::SIGCHLD, &sa, std::ptr::null_mut());
    }
}

extern "C" fn sigchld_handler(_sig: libc::c_int) {
    // Reap all finished children. Loop because multiple children may have
    // exited before the signal was delivered.
    // SAFETY: waitpid with WNOHANG is async-signal-safe per POSIX.
    unsafe {
        loop {
            let ret = libc::waitpid(-1, std::ptr::null_mut(), libc::WNOHANG);
            if ret <= 0 {
                break;
            }
        }
    }
}

impl CheckInvariant for Pty {
    fn check_invariant(&self) {
        invariant!(
            self.master.as_raw_fd() >= 0,
            "Pty master fd must be non-negative, got {}",
            self.master.as_raw_fd()
        );
        invariant!(
            self.child_pid > 0,
            "Pty child_pid must be positive, got {}",
            self.child_pid
        );
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        // SAFETY: child_pid is valid from fork(). kill(-pid) sends to the
        // entire process group (child called setsid(), so pid == pgid).
        // The SIGCHLD handler installed at startup will reap the child
        // asynchronously – no need to wait here at all.
        if self.child_pid > 0 {
            unsafe {
                libc::kill(-self.child_pid, libc::SIGHUP);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::os::fd::{FromRawFd, IntoRawFd};
    use std::os::unix::net::UnixStream;
    use std::time::{Duration, Instant};

    fn test_spawn_config<'a>(command: &'a [String]) -> PtySpawnConfig<'a> {
        PtySpawnConfig {
            term: "xterm-256color",
            colorterm: "truecolor",
            shell_fallback: "/bin/sh",
            command,
        }
    }

    fn read_until_contains(pty: &Pty, needle: &[u8], timeout: Duration) -> Vec<u8> {
        let deadline = Instant::now() + timeout;
        let mut buf = [0u8; 4096];
        let mut out = Vec::new();

        while Instant::now() < deadline {
            match pty.try_read(&mut buf).unwrap() {
                0 => std::thread::sleep(Duration::from_millis(10)),
                n => {
                    out.extend_from_slice(&buf[..n]);
                    if out.windows(needle.len()).any(|window| window == needle) {
                        return out;
                    }
                }
            }
        }

        out
    }

    #[test]
    fn spawn_runs_explicit_command_and_reads_output() {
        install_sigchld_reaper();

        let command = vec![
            "/bin/sh".to_string(),
            "-lc".to_string(),
            "printf READY".to_string(),
        ];
        let pty = Pty::spawn(80, 24, &test_spawn_config(&command)).unwrap();
        let out = read_until_contains(&pty, b"READY", Duration::from_secs(2));

        assert!(
            out.windows(5).any(|window| window == b"READY"),
            "pty output missing READY: {:?}",
            String::from_utf8_lossy(&out)
        );
    }

    #[test]
    fn shell_mode_accepts_input_and_produces_output() {
        install_sigchld_reaper();

        let previous_shell = std::env::var_os(SHELL_ENV_VAR);
        unsafe {
            std::env::set_var(SHELL_ENV_VAR, "/bin/sh");
        }

        let pty = Pty::spawn(80, 24, &test_spawn_config(&[])).unwrap();
        pty.write(b"printf READY\\nexit\\n").unwrap();
        let out = read_until_contains(&pty, b"READY", Duration::from_secs(3));

        match previous_shell {
            Some(value) => unsafe { std::env::set_var(SHELL_ENV_VAR, value) },
            None => unsafe { std::env::remove_var(SHELL_ENV_VAR) },
        }

        assert!(
            out.windows(5).any(|window| window == b"READY"),
            "interactive shell output missing READY: {:?}",
            String::from_utf8_lossy(&out)
        );
    }

    #[test]
    fn write_retries_after_partial_write_until_full_buffer_written() {
        use std::thread;

        let (reader, peer) = UnixStream::pair().unwrap();
        let write_end = unsafe { OwnedFd::from_raw_fd(peer.into_raw_fd()) };
        unsafe {
            let flags = libc::fcntl(write_end.as_raw_fd(), libc::F_GETFL);
            assert!(flags >= 0);
            assert_eq!(
                libc::fcntl(
                    write_end.as_raw_fd(),
                    libc::F_SETFL,
                    flags | libc::O_NONBLOCK
                ),
                0
            );
        }

        let fill = vec![b'y'; 64 * 1024];
        let mut prefilled = 0usize;
        loop {
            let n = unsafe { libc::write(write_end.as_raw_fd(), fill.as_ptr().cast(), fill.len()) };
            if n > 0 {
                prefilled += n as usize;
                continue;
            }

            let errno = unsafe { *libc::__errno_location() };
            assert!(errno == libc::EAGAIN || errno == libc::EWOULDBLOCK);
            break;
        }

        let requested = 1 << 20;
        let total_expected = prefilled + requested;
        let reader_thread = thread::spawn(move || {
            let mut reader = reader;
            let mut drain = vec![0u8; total_expected];
            let mut read_total = 0;

            while read_total < total_expected {
                let n = reader.read(&mut drain[read_total..]).unwrap();
                assert!(n > 0);
                read_total += n;
                if read_total < total_expected {
                    thread::sleep(Duration::from_millis(1));
                }
            }

            drain
        });

        let pty = Pty {
            master: write_end,
            child_pid: 0,
        };
        let buf = vec![b'x'; requested];

        let written = pty.write(&buf).unwrap();
        assert_eq!(written, requested);

        let drain = reader_thread.join().unwrap();
        assert_eq!(drain.len(), total_expected);
        assert!(drain[..prefilled].iter().all(|&byte| byte == b'y'));
        assert_eq!(&drain[prefilled..], &buf);
    }
}
