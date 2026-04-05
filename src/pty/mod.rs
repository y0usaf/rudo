#![allow(dead_code)]

use std::{
    env,
    ffi::OsString,
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use parking_lot::Mutex;
use portable_pty::{CommandBuilder, native_pty_system};
use tokio::task;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl PtySize {
    pub const fn new(cols: u16, rows: u16) -> Self {
        Self { cols, rows, pixel_width: 0, pixel_height: 0 }
    }
}

impl Default for PtySize {
    fn default() -> Self {
        Self::new(80, 24)
    }
}

impl From<PtySize> for portable_pty::PtySize {
    fn from(value: PtySize) -> Self {
        Self {
            rows: value.rows,
            cols: value.cols,
            pixel_width: value.pixel_width,
            pixel_height: value.pixel_height,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PtySpawnConfig {
    pub shell: Option<OsString>,
    pub args: Vec<OsString>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
    pub size: PtySize,
}

impl PtySpawnConfig {
    pub fn new(size: PtySize) -> Self {
        Self { shell: None, args: Vec::new(), cwd: None, env: Vec::new(), size }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyExitStatus {
    pub success: bool,
    pub code: u32,
    pub signal: Option<String>,
}

#[async_trait]
pub trait PtyProcess: Send {
    async fn resize(&mut self, size: PtySize) -> Result<()>;
    async fn read(&mut self, max_bytes: usize) -> Result<Vec<u8>>;
    async fn write(&mut self, bytes: &[u8]) -> Result<()>;
    async fn shutdown(&mut self) -> Result<()>;
    async fn wait(&mut self) -> Result<PtyExitStatus>;
    fn process_id(&self) -> Option<u32>;
}

pub trait PtySystem {
    type Process: PtyProcess;

    fn spawn(&self, config: PtySpawnConfig) -> Result<Self::Process>;
}

pub fn platform_default_shell(configured_shell: Option<&str>) -> OsString {
    if let Some(shell) = configured_shell {
        return OsString::from(shell);
    }

    platform_default_shell_impl()
}

pub const TERMVIDE_TERM_ENV_VAR: &str = "TERMVIDE_TERM";
const DEFAULT_TERM: &str = "xterm-256color";
const TERMVIDE_TERM: &str = "termvide";

fn configured_term() -> OsString {
    match env::var_os(TERMVIDE_TERM_ENV_VAR) {
        Some(value) if !value.is_empty() => value,
        _ if bundled_terminfo_is_available() => OsString::from(TERMVIDE_TERM),
        _ => OsString::from(DEFAULT_TERM),
    }
}

fn bundled_terminfo_is_available() -> bool {
    terminfo_entry_exists(TERMVIDE_TERM)
}

fn terminfo_entry_exists(term: &str) -> bool {
    if term.is_empty() {
        return false;
    }

    for directory in candidate_terminfo_dirs() {
        if terminfo_entry_exists_in_dir(&directory, term) {
            return true;
        }
    }

    false
}

fn candidate_terminfo_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(terminfo) = env::var_os("TERMINFO") {
        push_non_empty_path_list_entry(&mut dirs, PathBuf::from(terminfo));
    }

    if let Some(term_info_dirs) = env::var_os("TERMINFO_DIRS") {
        dirs.extend(env::split_paths(&term_info_dirs).filter(|path| !path.as_os_str().is_empty()));
    }

    if let Some(home) = env::var_os("HOME") {
        let mut home_terminfo = PathBuf::from(home);
        home_terminfo.push(".terminfo");
        dirs.push(home_terminfo);
    }

    #[cfg(unix)]
    dirs.extend([
        PathBuf::from("/etc/terminfo"),
        PathBuf::from("/lib/terminfo"),
        PathBuf::from("/usr/share/terminfo"),
    ]);

    dirs
}

fn push_non_empty_path_list_entry(dirs: &mut Vec<PathBuf>, path: PathBuf) {
    if !path.as_os_str().is_empty() {
        dirs.push(path);
    }
}

fn terminfo_entry_exists_in_dir(base_dir: &std::path::Path, term: &str) -> bool {
    for candidate in terminfo_entry_paths(base_dir, term) {
        if candidate.is_file() {
            return true;
        }
    }

    false
}

fn terminfo_entry_paths(base_dir: &std::path::Path, term: &str) -> [PathBuf; 2] {
    let first = term.as_bytes()[0] as char;
    [
        base_dir.join(first.to_string()).join(term),
        base_dir.join(format!("{:x}", term.as_bytes()[0])).join(term),
    ]
}

pub fn terminal_environment() -> Vec<(OsString, OsString)> {
    vec![
        (OsString::from("TERM"), configured_term()),
        (OsString::from("COLORTERM"), OsString::from("truecolor")),
        (OsString::from("TERM_PROGRAM"), OsString::from("termvide")),
    ]
}

#[cfg(unix)]
fn platform_default_shell_impl() -> OsString {
    unix::platform_default_shell_impl()
}

#[cfg(windows)]
fn platform_default_shell_impl() -> OsString {
    windows::platform_default_shell_impl()
}

#[cfg(not(any(unix, windows)))]
fn platform_default_shell_impl() -> OsString {
    OsString::from("sh")
}

pub struct NativePtySystem;

#[derive(Clone)]
pub struct NativePtyProcess {
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    reader: Arc<Mutex<Box<dyn Read + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn portable_pty::Child + Send + Sync>>>,
}

#[async_trait]
impl PtyProcess for NativePtyProcess {
    async fn resize(&mut self, size: PtySize) -> Result<()> {
        task::block_in_place(|| self.master.lock().resize(size.into())).context("PTY resize failed")
    }

    async fn read(&mut self, max_bytes: usize) -> Result<Vec<u8>> {
        task::block_in_place(|| {
            let mut buffer = vec![0; max_bytes.max(1)];
            let bytes_read = self.reader.lock().read(&mut buffer)?;
            buffer.truncate(bytes_read);
            Ok::<_, std::io::Error>(buffer)
        })
        .context("PTY read failed")
    }

    async fn write(&mut self, bytes: &[u8]) -> Result<()> {
        task::block_in_place(|| {
            let mut writer = self.writer.lock();
            writer.write_all(bytes)?;
            writer.flush()
        })
        .context("PTY write failed")
    }

    async fn shutdown(&mut self) -> Result<()> {
        task::block_in_place(|| self.child.lock().kill()).context("PTY child termination failed")
    }

    async fn wait(&mut self) -> Result<PtyExitStatus> {
        task::block_in_place(|| {
            let status = self.child.lock().wait()?;
            Ok::<_, std::io::Error>(PtyExitStatus {
                success: status.success(),
                code: status.exit_code(),
                signal: status.signal().map(str::to_string),
            })
        })
        .context("PTY wait failed")
    }

    fn process_id(&self) -> Option<u32> {
        self.child.lock().process_id()
    }
}

impl PtySystem for NativePtySystem {
    type Process = NativePtyProcess;

    fn spawn(&self, config: PtySpawnConfig) -> Result<Self::Process> {
        let shell = config.shell.unwrap_or_else(|| platform_default_shell(None));
        let mut command = CommandBuilder::new(shell.as_os_str());
        command.args(config.args.iter().map(OsString::as_os_str));

        if let Some(cwd) = &config.cwd {
            command.cwd(cwd.as_os_str());
        }

        for (key, value) in &config.env {
            command.env(key, value);
        }

        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(config.size.into())
            .with_context(|| format!("failed to allocate PTY for shell {:?}", shell))?;

        let child = pair
            .slave
            .spawn_command(command)
            .with_context(|| format!("failed to spawn shell {:?} in PTY", shell))?;

        let reader = pair.master.try_clone_reader().context("failed to clone PTY reader")?;
        let writer = pair.master.take_writer().context("failed to take PTY writer")?;

        Ok(NativePtyProcess {
            master: Arc::new(Mutex::new(pair.master)),
            reader: Arc::new(Mutex::new(reader)),
            writer: Arc::new(Mutex::new(writer)),
            child: Arc::new(Mutex::new(child)),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_shell_takes_precedence() {
        assert_eq!(platform_default_shell(Some("/usr/bin/fish")), OsString::from("/usr/bin/fish"));
    }

    #[test]
    fn default_shell_never_returns_empty() {
        assert!(!platform_default_shell(None).is_empty());
    }

    #[test]
    fn terminal_environment_sets_expected_term() {
        with_env_vars_cleared(&[TERMVIDE_TERM_ENV_VAR, "TERMINFO", "TERMINFO_DIRS"], || {
            let env = terminal_environment();
            assert!(env.iter().any(|(key, value)| {
                key == &OsString::from("TERM") && value == &OsString::from(DEFAULT_TERM)
            }));
        });
    }

    #[test]
    fn terminal_environment_prefers_bundled_term_when_available() {
        with_env_vars_cleared(&[TERMVIDE_TERM_ENV_VAR, "TERMINFO", "TERMINFO_DIRS"], || {
            let temp_dir = temp_test_dir("bundled-term");
            let entry_path = temp_dir.join("74").join(TERMVIDE_TERM);
            std::fs::create_dir_all(entry_path.parent().unwrap()).unwrap();
            std::fs::write(&entry_path, b"compiled-terminfo").unwrap();

            std::env::set_var("TERMINFO", &temp_dir);
            let env = terminal_environment();

            assert!(env.iter().any(|(key, value)| {
                key == &OsString::from("TERM") && value == &OsString::from(TERMVIDE_TERM)
            }));
        });
    }

    #[test]
    fn terminal_environment_allows_term_override() {
        with_env_vars_cleared(&[TERMVIDE_TERM_ENV_VAR, "TERMINFO", "TERMINFO_DIRS"], || {
            std::env::set_var(TERMVIDE_TERM_ENV_VAR, "termvide");
            let env = terminal_environment();

            assert!(env.iter().any(|(key, value)| {
                key == &OsString::from("TERM") && value == &OsString::from("termvide")
            }));
        });
    }

    #[test]
    fn terminal_environment_override_wins_even_without_bundled_terminfo() {
        with_env_vars_cleared(&[TERMVIDE_TERM_ENV_VAR, "TERMINFO", "TERMINFO_DIRS"], || {
            std::env::set_var(TERMVIDE_TERM_ENV_VAR, "screen-256color");
            let env = terminal_environment();

            assert!(env.iter().any(|(key, value)| {
                key == &OsString::from("TERM") && value == &OsString::from("screen-256color")
            }));
        });
    }

    #[test]
    fn terminal_environment_ignores_empty_term_override() {
        with_env_vars_cleared(&[TERMVIDE_TERM_ENV_VAR, "TERMINFO", "TERMINFO_DIRS"], || {
            std::env::set_var(TERMVIDE_TERM_ENV_VAR, "");
            let env = terminal_environment();

            assert!(env.iter().any(|(key, value)| {
                key == &OsString::from("TERM") && value == &OsString::from(DEFAULT_TERM)
            }));
        });
    }

    #[test]
    fn terminfo_entry_exists_supports_hashed_and_letter_layouts() {
        let hashed_dir = temp_test_dir("hashed-layout");
        let hashed_entry = hashed_dir.join("74").join(TERMVIDE_TERM);
        std::fs::create_dir_all(hashed_entry.parent().unwrap()).unwrap();
        std::fs::write(&hashed_entry, b"compiled-terminfo").unwrap();
        assert!(terminfo_entry_exists_in_dir(&hashed_dir, TERMVIDE_TERM));

        let letter_dir = temp_test_dir("letter-layout");
        let letter_entry = letter_dir.join("t").join(TERMVIDE_TERM);
        std::fs::create_dir_all(letter_entry.parent().unwrap()).unwrap();
        std::fs::write(&letter_entry, b"compiled-terminfo").unwrap();
        assert!(terminfo_entry_exists_in_dir(&letter_dir, TERMVIDE_TERM));
    }

    fn with_env_vars_cleared<T>(keys: &[&str], f: impl FnOnce() -> T) -> T {
        let saved: Vec<_> =
            keys.iter().map(|key| ((*key).to_string(), std::env::var_os(key))).collect();

        for key in keys {
            std::env::remove_var(key);
        }

        let result = f();

        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(&key, value),
                None => std::env::remove_var(&key),
            }
        }

        result
    }

    fn temp_test_dir(label: &str) -> PathBuf {
        let unique = format!(
            "termvide-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
        );
        let path = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn native_pty_can_spawn_and_capture_output() -> Result<()> {
        let system = NativePtySystem;
        let mut process = system.spawn(PtySpawnConfig {
            shell: Some(OsString::from("/bin/sh")),
            args: vec![OsString::from("-c"), OsString::from("printf 'termvide-pty-test'")],
            cwd: None,
            env: Vec::new(),
            size: PtySize::new(80, 24),
        })?;

        let status = process.wait().await?;
        let mut output = Vec::new();
        for _ in 0..4 {
            let chunk = process.read(4096).await?;
            if chunk.is_empty() {
                break;
            }
            output.extend_from_slice(&chunk);
        }

        assert!(status.success);
        assert_eq!(status.code, 0);
        assert!(String::from_utf8_lossy(&output).contains("termvide-pty-test"));
        Ok(())
    }

    #[cfg(unix)]
    #[ignore = "diagnostic only"]
    #[tokio::test]
    async fn diagnostic_nushell_interactive_startup() -> Result<()> {
        let nu = OsString::from("/run/current-system/sw/bin/nu");
        if !std::path::Path::new(&nu).exists() {
            return Ok(());
        }

        let system = NativePtySystem;
        let mut process = system.spawn(PtySpawnConfig {
            shell: Some(nu),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            size: PtySize::new(80, 24),
        })?;

        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let chunk = process.read(4096).await?;
        eprintln!("NU_STARTUP_BYTES={:?}", String::from_utf8_lossy(&chunk));
        let _ = process.write(b"exit\r").await;
        let _ = process.wait().await?;
        Ok(())
    }
}
