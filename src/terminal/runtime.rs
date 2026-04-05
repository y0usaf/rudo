use std::{path::PathBuf, sync::Arc, thread, time::Duration};

use anyhow::Result;
use tokio::{runtime::{Builder, Runtime}, sync::watch};
use winit::event_loop::EventLoopProxy;

use crate::{
    pty::{
        NativePtyProcess, NativePtySystem, PtySize, PtySpawnConfig, PtySystem,
        platform_default_shell, terminal_environment,
    },
    running_tracker::RunningTracker,
    terminal::{session::spawn_pty_listener, theme::TerminalTheme},
    window::{EventPayload, RouteId},
};

/// Handle to a running terminal process.  Cloning is cheap – both fields are
/// backed by reference-counted handles.
#[derive(Clone)]
pub struct TerminalHandle {
    process: NativePtyProcess,
    /// Sender half of a watch channel used to signal PTY dimension changes to
    /// the session listener loop.  The sender is wrapped in an `Arc` so that
    /// multiple clones of `TerminalHandle` all share the same channel.
    resize_tx: Arc<watch::Sender<PtySize>>,
}

impl TerminalHandle {
    pub fn new(process: NativePtyProcess, resize_tx: Arc<watch::Sender<PtySize>>) -> Self {
        Self { process, resize_tx }
    }
}

pub struct TerminalRuntime {
    runtime: Option<Runtime>,
}

impl TerminalRuntime {
    pub fn new() -> std::io::Result<Self> {
        let available = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let workers = available.min(4);
        let runtime = Builder::new_multi_thread()
            .worker_threads(workers)
            .enable_all()
            .build()?;
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

        // Create a watch channel so that window resize events can be forwarded to
        // the session listener loop without requiring it to share a mutex with the
        // render thread.
        let (resize_tx, resize_rx) = watch::channel(size);
        let resize_tx = Arc::new(resize_tx);

        let theme = TerminalTheme::load();

        let listener_process = process.clone();
        self.runtime().spawn(async move {
            if let Err(error) = spawn_pty_listener(
                listener_process,
                route_id,
                proxy,
                running_tracker,
                size.cols as usize,
                size.rows as usize,
                resize_rx,
                theme,
            )
            .await
            {
                log::error!("terminal session listener failed: {error:#}");
            }
        });

        Ok(TerminalHandle::new(process, resize_tx))
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
        // Notify the session listener loop first so the terminal state is updated
        // before the child process starts its redraw in response to SIGWINCH.
        let _ = handle.resize_tx.send(size);
        self.runtime().spawn(async move {
            // Yield once so the session listener has a chance to observe the
            // watch-channel update and resize its terminal state *before* we
            // deliver SIGWINCH to the child (which triggers its redraw).
            tokio::task::yield_now().await;
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
