use std::{path::PathBuf, time::Duration};

use anyhow::Result;
use tokio::runtime::{Builder, Runtime};
use winit::event_loop::EventLoopProxy;

use crate::{
    pty::{
        NativePtyProcess, NativePtySystem, PtySize, PtySpawnConfig, PtySystem,
        platform_default_shell, terminal_environment,
    },
    running_tracker::RunningTracker,
    terminal::session::spawn_pty_listener,
    window::{EventPayload, RouteId},
};

#[derive(Clone)]
pub struct TerminalHandle {
    process: NativePtyProcess,
}

impl TerminalHandle {
    pub fn new(process: NativePtyProcess) -> Self {
        Self { process }
    }
}

pub struct TerminalRuntime {
    runtime: Option<Runtime>,
}

impl TerminalRuntime {
    pub fn new() -> std::io::Result<Self> {
        let runtime = Builder::new_multi_thread().enable_all().build()?;
        Ok(Self { runtime: Some(runtime) })
    }

    pub fn launch(
        &mut self,
        route_id: RouteId,
        proxy: EventLoopProxy<EventPayload>,
        running_tracker: RunningTracker,
        size: PtySize,
        shell: Option<String>,
        cwd: Option<PathBuf>,
    ) -> Result<TerminalHandle> {
        let system = NativePtySystem;
        let shell = platform_default_shell(shell.as_deref());
        let process = system.spawn(PtySpawnConfig {
            shell: Some(shell),
            args: Vec::new(),
            cwd,
            env: terminal_environment(),
            size,
        })?;

        let listener_process = process.clone();
        self.runtime().spawn(async move {
            if let Err(error) = spawn_pty_listener(
                listener_process,
                route_id,
                proxy,
                running_tracker,
                size.cols as usize,
                size.rows as usize,
            )
            .await
            {
                log::error!("terminal session listener failed: {error:#}");
            }
        });

        Ok(TerminalHandle::new(process))
    }

    pub fn write(&self, handle: TerminalHandle, bytes: Vec<u8>) {
        self.runtime().spawn(async move {
            let mut process = handle.process;
            if let Err(error) = crate::pty::PtyProcess::write(&mut process, &bytes).await {
                log::error!("terminal PTY write failed: {error:#}");
            }
        });
    }

    pub fn resize(&self, handle: TerminalHandle, size: PtySize) {
        self.runtime().spawn(async move {
            let mut process = handle.process;
            if let Err(error) = crate::pty::PtyProcess::resize(&mut process, size).await {
                log::error!("terminal PTY resize failed: {error:#}");
            }
        });
    }

    pub fn shutdown_process(&self, handle: TerminalHandle) {
        self.runtime().spawn(async move {
            let mut process = handle.process;
            if let Err(error) = crate::pty::PtyProcess::shutdown(&mut process).await {
                log::error!("terminal PTY shutdown failed: {error:#}");
            }
        });
    }

    fn runtime(&self) -> &Runtime {
        self.runtime.as_ref().expect("terminal runtime must be available")
    }

    pub fn shutdown_timeout(&mut self, timeout: Duration) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_timeout(timeout);
        }
    }
}

impl Drop for TerminalRuntime {
    fn drop(&mut self) {
        self.shutdown_timeout(Duration::from_millis(500));
    }
}
