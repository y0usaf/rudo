use anyhow::{Context, Result};
use tokio::{sync::{mpsc, watch}, task::JoinHandle};
use winit::event_loop::EventLoopProxy;

use crate::{
    pty::{PtyProcess, PtySize},
    session::bridge::TerminalRenderBridge,
    running_tracker::RunningTracker,
    terminal::{
        ClipboardRequest, ClipboardRequestKind, input::TerminalInputSettings,
        parser::TerminalParser, state::TerminalState, theme::TerminalTheme,
    },
    window::{EventPayload, RouteId, UserEvent, WindowCommand},
};

const DEFAULT_READ_CHUNK_SIZE: usize = 8192;

pub struct TerminalSessionCore {
    parser: TerminalParser,
    state: TerminalState,
    render_bridge: TerminalRenderBridge,
    sync_updates_active: bool,
    pending_full_flush: bool,
}

pub struct TerminalFrame {
    pub commands: Vec<crate::renderer::DrawCommand>,
    pub responses: Vec<Vec<u8>>,
    pub clipboard_requests: Vec<ClipboardRequest>,
}

impl TerminalSessionCore {
    pub fn new(cols: usize, rows: usize, theme: TerminalTheme) -> Self {
        let state = TerminalState::with_theme(cols, rows, theme.clone());
        let sync_updates_active = state.synchronized_updates_active();
        Self {
            parser: TerminalParser::new(),
            state,
            render_bridge: TerminalRenderBridge::with_theme(theme),
            sync_updates_active,
            pending_full_flush: false,
        }
    }

    pub fn bootstrap_draw_commands(&self) -> Vec<crate::renderer::DrawCommand> {
        self.render_bridge.full_draw_commands(&self.state)
    }

    pub fn apply_bytes(&mut self, bytes: &[u8]) -> TerminalFrame {
        self.parser.advance(&mut self.state, bytes);
        let clipboard_requests = self.state.take_pending_clipboard_requests();
        let sync_updates_active = self.state.synchronized_updates_active();

        let commands = if sync_updates_active {
            self.pending_full_flush = true;
            Vec::new()
        } else if self.sync_updates_active && self.pending_full_flush {
            self.pending_full_flush = false;
            let _ = self.state.take_damage();
            self.render_bridge.full_draw_commands(&self.state)
        } else {
            let damage = self.state.take_damage();
            self.render_bridge.draw_commands(&self.state, damage)
        };

        self.sync_updates_active = sync_updates_active;
        let responses = self.state.take_pending_responses();

        TerminalFrame { commands, responses, clipboard_requests }
    }

    pub fn resize(&mut self, cols: usize, rows: usize) -> Vec<crate::renderer::DrawCommand> {
        self.state.resize(cols, rows);
        self.pending_full_flush = false;
        self.render_bridge.full_draw_commands(&self.state)
    }

    pub fn state(&self) -> &TerminalState {
        &self.state
    }
}

pub fn emit_draw_commands(
    proxy: &EventLoopProxy<EventPayload>,
    route_id: RouteId,
    commands: Vec<crate::renderer::DrawCommand>,
) {
    if commands.is_empty() {
        return;
    }
    proxy.send_event(EventPayload::for_route(UserEvent::DrawCommandBatch(commands), route_id)).ok();
}

fn emit_terminal_input(
    proxy: &EventLoopProxy<EventPayload>,
    route_id: RouteId,
    input: TerminalInputSettings,
) {
    proxy
        .send_event(EventPayload::for_route(
            UserEvent::WindowCommand(WindowCommand::TerminalInputChanged(input)),
            route_id,
        ))
        .ok();
}

fn emit_clipboard_requests(
    proxy: &EventLoopProxy<EventPayload>,
    route_id: RouteId,
    requests: Vec<ClipboardRequest>,
) {
    for request in requests {
        let command = match request.kind {
            ClipboardRequestKind::Set(_) => WindowCommand::ClipboardSet(request),
            ClipboardRequestKind::Query => WindowCommand::ClipboardQuery(request.selection),
        };
        proxy.send_event(EventPayload::for_route(UserEvent::WindowCommand(command), route_id)).ok();
    }
}

pub fn spawn_pty_listener<P>(
    mut process: P,
    route_id: RouteId,
    proxy: EventLoopProxy<EventPayload>,
    running_tracker: RunningTracker,
    cols: usize,
    rows: usize,
    mut resize_rx: watch::Receiver<PtySize>,
    theme: TerminalTheme,
) -> JoinHandle<Result<()>>
where
    P: PtyProcess + Clone + 'static,
{
    tokio::spawn(async move {
        let mut reader_process = process.clone();
        let (read_tx, mut read_rx) = mpsc::unbounded_channel();
        let reader_thread = std::thread::spawn(move || {
            loop {
                let result = reader_process.read_blocking(DEFAULT_READ_CHUNK_SIZE);
                let should_stop = match &result {
                    Ok(bytes) => bytes.is_empty(),
                    Err(_) => true,
                };

                if read_tx.send(result).is_err() {
                    break;
                }

                if should_stop {
                    break;
                }
            }
        });

        let mut session = TerminalSessionCore::new(cols, rows, theme);
        let mut last_title = session.state().title().map(str::to_owned);
        let mut last_input = session.state().input_settings();
        emit_terminal_input(&proxy, route_id, last_input);
        emit_draw_commands(&proxy, route_id, session.bootstrap_draw_commands());

        // Helper: drain all pending resize signals, applying only the latest.
        // Returns `true` if at least one resize was applied.
        let apply_pending_resizes = |session: &mut TerminalSessionCore,
                                     resize_rx: &mut watch::Receiver<PtySize>,
                                     proxy: &EventLoopProxy<EventPayload>,
                                     route_id: RouteId| -> bool {
            if resize_rx.has_changed().unwrap_or(false) {
                let new_size = *resize_rx.borrow_and_update();
                let resize_commands =
                    session.resize(new_size.cols as usize, new_size.rows as usize);
                emit_draw_commands(proxy, route_id, resize_commands);
                true
            } else {
                false
            }
        };

        let mut reader_thread = Some(reader_thread);
        let mut reader_exited = false;

        let result: Result<()> = async {
            loop {
                tokio::select! {
                    biased;

                    _ = resize_rx.changed() => {
                        apply_pending_resizes(
                            &mut session, &mut resize_rx, &proxy, route_id,
                        );
                        continue;
                    }

                    read_result = read_rx.recv() => {
                        let Some(read_result) = read_result else {
                            reader_exited = true;
                            break;
                        };
                        let bytes = read_result?;
                        if bytes.is_empty() {
                            break;
                        }

                        apply_pending_resizes(
                            &mut session, &mut resize_rx, &proxy, route_id,
                        );

                        let frame = session.apply_bytes(&bytes);
                        for response in frame.responses {
                            process.write(&response).await?;
                        }
                        emit_clipboard_requests(&proxy, route_id, frame.clipboard_requests);
                        let commands = frame.commands;
                        let input = session.state().input_settings();
                        if input != last_input {
                            emit_terminal_input(&proxy, route_id, input);
                            last_input = input;
                        }
                        let title = session.state().title();
                        if title != last_title.as_deref() {
                            if let Some(title) = title {
                                proxy
                                    .send_event(EventPayload::for_route(
                                        UserEvent::WindowCommand(WindowCommand::TitleChanged(title.to_owned())),
                                        route_id,
                                    ))
                                    .ok();
                            }
                            last_title = title.map(str::to_owned);
                        }
                        emit_draw_commands(&proxy, route_id, commands);
                    }
                }
            }
            Ok(())
        }
        .await;

        if reader_exited {
            match reader_thread.take().unwrap().join() {
                Ok(()) => {}
                Err(panic) => std::panic::resume_unwind(panic),
            }
        } else {
            // The blocking PTY read may never wake during shutdown, so avoid
            // joining unless the channel closure proves the reader thread exited.
            drop(reader_thread.take());
        }

        result?;

        let status = process.wait().await.context("failed waiting for PTY child exit")?;
        let reason = match status.signal.as_deref() {
            Some(signal) => format!("terminal process exited due to signal {signal}"),
            None => format!("terminal process exited with code {}", status.code),
        };
        if !status.success {
            running_tracker.quit_with_code(1, &reason);
        }
        log::info!("{reason}");
        proxy.send_event(EventPayload::for_route(UserEvent::ProcessExited, route_id)).ok();
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use async_trait::async_trait;

    use super::TerminalSessionCore;
    use crate::pty::{PtyExitStatus, PtyProcess, PtySize};
    use crate::terminal::ClipboardRequestKind;
    use crate::terminal::theme::TerminalTheme;

    #[test]
    fn session_core_bootstraps_and_applies_output() {
        let mut session = TerminalSessionCore::new(4, 2, TerminalTheme::default());

        let bootstrap = session.bootstrap_draw_commands();
        let frame = session.apply_bytes(b"ab\r\ncd");

        assert!(!bootstrap.is_empty());
        assert!(frame.commands.iter().any(|command| matches!(
            command,
            crate::renderer::DrawCommand::Window {
                command: crate::renderer::WindowDrawCommand::DrawLine { .. },
                ..
            }
        )));
        assert!(
            frame
                .commands
                .iter()
                .any(|command| matches!(command, crate::renderer::DrawCommand::UpdateCursor(_)))
        );
        assert!(frame.responses.is_empty());
        assert!(frame.clipboard_requests.is_empty());
        assert_eq!(session.state().screen().row_text(0).trim(), "ab");
        assert_eq!(session.state().screen().row_text(1).trim(), "cd");
    }

    #[test]
    fn session_core_buffers_during_synchronized_updates_and_flushes_full_frame_after() {
        let mut session = TerminalSessionCore::new(4, 2, TerminalTheme::default());

        let frame = session.apply_bytes(b"\x1bP=1s\x1b\\ab");
        assert!(frame.commands.is_empty());

        let frame = session.apply_bytes(b"\x1bP=2s\x1b\\");
        assert!(frame.commands.iter().any(|command| matches!(
            command,
            crate::renderer::DrawCommand::Window {
                command: crate::renderer::WindowDrawCommand::Clear,
                ..
            }
        )));
        assert!(frame.commands.iter().any(|command| matches!(
            command,
            crate::renderer::DrawCommand::Window {
                command: crate::renderer::WindowDrawCommand::DrawLine { row: 0, line },
                ..
            } if line.text.starts_with("ab")
        )));
    }

    #[test]
    fn session_core_drains_clipboard_requests() {
        let mut session = TerminalSessionCore::new(10, 5, TerminalTheme::default());
        let frame = session.apply_bytes(b"\x1b]52;c;aGVsbG8=\x07");

        assert_eq!(frame.clipboard_requests.len(), 1);
        assert_eq!(frame.clipboard_requests[0].kind, ClipboardRequestKind::Set("hello".into()));
    }

    #[test]
    fn session_core_drains_clipboard_queries() {
        let mut session = TerminalSessionCore::new(10, 5, TerminalTheme::default());
        let frame = session.apply_bytes(b"\x1b]52;c;?\x07");

        assert_eq!(frame.clipboard_requests.len(), 1);
        assert_eq!(frame.clipboard_requests[0].kind, ClipboardRequestKind::Query);
    }

    #[derive(Clone)]
    struct FakePtyProcess {
        chunks: std::sync::Arc<std::sync::Mutex<Vec<Vec<u8>>>>,
        status: PtyExitStatus,
    }

    #[async_trait]
    impl PtyProcess for FakePtyProcess {
        async fn resize(&mut self, _size: PtySize) -> Result<()> {
            Ok(())
        }

        fn read_blocking(&mut self, _max_bytes: usize) -> Result<Vec<u8>> {
            let mut chunks = self.chunks.lock().unwrap();
            if chunks.is_empty() { Ok(Vec::new()) } else { Ok(chunks.remove(0)) }
        }

        async fn write(&mut self, _bytes: &[u8]) -> Result<()> {
            Ok(())
        }

        async fn shutdown(&mut self) -> Result<()> {
            Ok(())
        }

        async fn wait(&mut self) -> Result<PtyExitStatus> {
            Ok(self.status.clone())
        }

        fn process_id(&self) -> Option<u32> {
            Some(1)
        }
    }

    #[tokio::test]
    async fn fake_process_can_be_read_to_completion() -> Result<()> {
        let mut process = FakePtyProcess {
            chunks: std::sync::Arc::new(std::sync::Mutex::new(vec![b"hello".to_vec(), Vec::new()])),
            status: PtyExitStatus { success: true, code: 0, signal: None },
        };

        assert_eq!(process.read(16).await?, b"hello");
        assert!(process.read(16).await?.is_empty());
        assert_eq!(process.wait().await?.code, 0);
        Ok(())
    }

    #[test]
    fn session_core_generates_terminal_query_responses() {
        let mut session = TerminalSessionCore::new(10, 5, TerminalTheme::default());
        let frame = session.apply_bytes(b"\x1b[6n\x1b[c\x1b[>c");
        let responses = frame.responses;
        assert!(responses.iter().any(|resp| resp == b"\x1b[1;1R"));
        assert!(responses.iter().any(|resp| resp == b"\x1b[?62;c"));
        assert!(responses.iter().any(|resp| resp == b"\x1b[>1;10;0c"));
    }
}
