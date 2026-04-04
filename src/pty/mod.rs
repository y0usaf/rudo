#![allow(dead_code)]

use std::{
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

    #[cfg(unix)]
    #[tokio::test]
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
