//! Direct PTY handling using raw libc calls for zero-overhead Linux PTY operations.
//! No portable-pty wrapper - raw syscalls for maximum speed.

use std::ffi::{CStr, CString};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};

use std::fmt;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const TERM_ENV_VAR: &str = "TERM";
const COLORTERM_ENV_VAR: &str = "COLORTERM";
const SHELL_ENV_VAR: &str = "SHELL";

#[derive(Debug)]
struct PtyError(String);
impl fmt::Display for PtyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for PtyError {}

#[derive(Clone, Copy, Debug)]
pub struct PtySpawnConfig<'a> {
    pub term: &'a str,
    pub colorterm: &'a str,
    pub shell_fallback: &'a str,
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
        // SAFETY: posix_openpt returns a valid fd or -1. We check < 0 before use.
        let master_raw = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY) };
        if master_raw < 0 {
            return Err(Box::new(PtyError("openpt failed".into())));
        }
        // SAFETY: master_raw is a valid fd from posix_openpt (checked above).
        let master = unsafe { OwnedFd::from_raw_fd(master_raw) };

        // SAFETY: master is a valid PTY master fd.
        if unsafe { libc::grantpt(master.as_raw_fd()) } != 0 {
            return Err(Box::new(PtyError("grantpt failed".into())));
        }
        // SAFETY: master is a valid, granted PTY master fd.
        if unsafe { libc::unlockpt(master.as_raw_fd()) } != 0 {
            return Err(Box::new(PtyError("unlockpt failed".into())));
        }

        let slave_name = {
            let mut buf = [0u8; 512];
            // SAFETY: ptsname_r writes into our stack buffer with bounded length,
            // and master_raw is a valid PTY master fd. Using ptsname_r instead of
            // ptsname avoids the thread-safety issue of the static buffer.
            let rc =
                unsafe { libc::ptsname_r(master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if rc != 0 {
                return Err(Box::new(PtyError("ptsname_r failed".into())));
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

        // SAFETY: fork() is safe to call; we handle both parent (pid > 0) and child (pid == 0).
        let pid = unsafe { libc::fork() };
        if pid < 0 {
            return Err(Box::new(PtyError("fork failed".into())));
        }

        if pid == 0 {
            // Child process
            // SAFETY: Child process after fork. We call setsid, open slave PTY,
            // dup2 to stdio fds, set environment, and execvp into the shell.
            // If any step fails, _exit(1) ensures no undefined behavior.
            unsafe {
                libc::setsid();

                let slave_cstr = CString::new(slave_name.clone()).unwrap();
                let slave_fd = libc::open(slave_cstr.as_ptr(), libc::O_RDWR);
                if slave_fd < 0 {
                    libc::_exit(1);
                }

                libc::ioctl(
                    slave_fd,
                    libc::TIOCSWINSZ,
                    &winsize as *const _ as *const libc::c_void,
                );

                libc::dup2(slave_fd, 0);
                libc::dup2(slave_fd, 1);
                libc::dup2(slave_fd, 2);
                if slave_fd > 2 {
                    libc::close(slave_fd);
                }

                let term = CString::new(config.term).unwrap();
                let term_key = CString::new(TERM_ENV_VAR).unwrap();
                libc::setenv(term_key.as_ptr(), term.as_ptr(), 1);

                let colorterm = CString::new(config.colorterm).unwrap();
                let colorterm_key = CString::new(COLORTERM_ENV_VAR).unwrap();
                libc::setenv(colorterm_key.as_ptr(), colorterm.as_ptr(), 1);

                let shell = std::env::var(SHELL_ENV_VAR)
                    .unwrap_or_else(|_| config.shell_fallback.to_string());
                let shell_cstr = CString::new(shell).unwrap();
                libc::execvp(
                    shell_cstr.as_ptr(),
                    [shell_cstr.as_ptr(), std::ptr::null()].as_ptr(),
                );
                libc::_exit(1);
            }
        }

        // Parent - set master to non-blocking
        // SAFETY: master fd is valid; F_GETFL/F_SETFL are standard fcntl operations.
        unsafe {
            let flags = libc::fcntl(master.as_raw_fd(), libc::F_GETFL);
            libc::fcntl(master.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK);
        }

        Ok(Self {
            master,
            child_pid: pid,
        })
    }

    /// Try to read from the PTY (non-blocking).
    pub fn try_read(&self, buf: &mut [u8]) -> Result<usize> {
        // SAFETY: buf is a valid mutable slice; read writes at most buf.len() bytes.
        // The fd is the PTY master owned by self.
        let n = unsafe { libc::read(self.master.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        if n >= 0 {
            Ok(n as usize)
        } else {
            // SAFETY: __errno_location returns a valid pointer to the thread-local errno.
            let errno = unsafe { *libc::__errno_location() };
            if errno == libc::EAGAIN || errno == libc::EWOULDBLOCK {
                Ok(0)
            } else {
                Err(Box::new(PtyError(format!(
                    "read failed: {}",
                    std::io::Error::from_raw_os_error(errno)
                ))))
            }
        }
    }

    /// Write to the PTY.
    pub fn write(&self, buf: &[u8]) -> Result<usize> {
        // SAFETY: buf is a valid slice; write reads at most buf.len() bytes.
        // The fd is the PTY master owned by self.
        let n = unsafe { libc::write(self.master.as_raw_fd(), buf.as_ptr().cast(), buf.len()) };
        if n >= 0 {
            Ok(n as usize)
        } else {
            // SAFETY: __errno_location returns a valid pointer to the thread-local errno.
            let errno = unsafe { *libc::__errno_location() };
            Err(Box::new(PtyError(format!(
                "write failed: {}",
                std::io::Error::from_raw_os_error(errno)
            ))))
        }
    }

    /// Resize the PTY.
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
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
                return Err(Box::new(PtyError("TIOCSWINSZ failed".into())));
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

impl Drop for Pty {
    fn drop(&mut self) {
        // SAFETY: child_pid is valid from fork(). kill(-pid) sends to the
        // entire process group (child called setsid(), so pid == pgid).
        // The SIGCHLD handler installed at startup will reap the child
        // asynchronously – no need to wait here at all.
        unsafe {
            libc::kill(-self.child_pid, libc::SIGHUP);
        }
    }
}
