use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

use async_trait::async_trait;
use russh::server::{Auth, Msg, Session};
use russh::{Channel, ChannelId};
use tokio::sync::watch;

use crossterm::{cursor, execute};

use crate::accounts::AccountStore;
use crate::ansi_input::AnsiParser;
use crate::channel_writer::ChannelWriter;
use crate::lobby::{self, LobbyResult};
use crate::saves::SaveManager;
use crate::session;

/// Shared server state accessible to all connections.
pub struct ServerState {
    pub data_dir: PathBuf,
    pub accounts: AccountStore,
    pub active_sessions: AtomicUsize,
    pub max_connections: usize,
    pub idle_timeout_secs: u64,
}

impl ServerState {
    pub fn new(data_dir: PathBuf, max_connections: usize, idle_timeout_secs: u64) -> Self {
        let accounts = AccountStore::new(&data_dir);
        Self {
            data_dir,
            accounts,
            active_sessions: AtomicUsize::new(0),
            max_connections,
            idle_timeout_secs,
        }
    }
}

/// Per-connection handler. One instance per SSH connection.
pub struct SshHandler {
    pub server: Arc<ServerState>,
    channels: HashMap<ChannelId, ChannelState>,
}

struct ChannelState {
    input_tx: std::sync::mpsc::Sender<Vec<u8>>,
    size_tx: watch::Sender<(u32, u32)>,
}

impl SshHandler {
    pub fn new(server: Arc<ServerState>) -> Self {
        Self {
            server,
            channels: HashMap::new(),
        }
    }
}

#[async_trait]
impl russh::server::Handler for SshHandler {
    type Error = russh::Error;

    async fn auth_none(&mut self, _user: &str) -> Result<Auth, Self::Error> {
        // Always accept — identity is handled at the application level (lobby).
        Ok(Auth::Accept)
    }

    async fn channel_open_session(
        &mut self,
        channel: Channel<Msg>,
        session: &mut Session,
    ) -> Result<bool, Self::Error> {
        let active = self.server.active_sessions.load(Ordering::Relaxed);
        if active >= self.server.max_connections {
            tracing::warn!(
                active,
                max = self.server.max_connections,
                "Rejecting connection: max connections reached"
            );
            return Ok(false);
        }
        self.server.active_sessions.fetch_add(1, Ordering::Relaxed);

        let (input_tx, input_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let (size_tx, size_rx) = watch::channel((80u32, 24u32));
        let channel_id = channel.id();

        self.channels
            .insert(channel_id, ChannelState { input_tx, size_tx });

        let server = Arc::clone(&self.server);
        let handle = session.handle();

        tokio::task::spawn_blocking(move || {
            let rt_handle = tokio::runtime::Handle::current();
            let mut writer = ChannelWriter::new(handle.clone(), channel_id, rt_handle.clone());
            let _ = execute!(writer, cursor::Hide);
            let mut parser = AnsiParser::new();
            let mut size_rx = size_rx;

            // Wait briefly for the PTY request to arrive with real terminal
            // dimensions. The watch channel is initialized with (80, 24) but
            // the client's pty_request (which carries the actual size) arrives
            // asynchronously after channel_open_session.
            for _ in 0..20 {
                if size_rx.has_changed().unwrap_or(false) {
                    size_rx.borrow_and_update();
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }

            let (width, height) = *size_rx.borrow();
            let idle_timeout = Duration::from_secs(server.idle_timeout_secs);

            // Lobby ↔ session loop: LogOut returns to the lobby.
            loop {
                let active_sessions = server.active_sessions.load(Ordering::Relaxed);

                // Run lobby
                let username = match lobby::run_lobby(
                    &mut writer,
                    &input_rx,
                    &mut parser,
                    &server.accounts,
                    width as i32,
                    height as i32,
                    active_sessions,
                    idle_timeout,
                ) {
                    Ok(LobbyResult::LoggedIn(user)) => user,
                    Ok(LobbyResult::Quit) => {
                        tracing::info!("Client quit from lobby");
                        break;
                    }
                    Err(e) => {
                        tracing::error!("Lobby error: {}", e);
                        break;
                    }
                };

                tracing::info!(username = %username, "User logged in");

                let saves = SaveManager::new(&server.data_dir, &username);

                match session::run_session(
                    &mut writer,
                    &input_rx,
                    &mut size_rx,
                    &mut parser,
                    &saves,
                    &username,
                    idle_timeout,
                ) {
                    Ok(session::SessionResult::LogOut) => {
                        tracing::info!(username = %username, "User logged out");
                        continue; // Back to pre-login lobby
                    }
                    Ok(session::SessionResult::Quit) => {
                        tracing::info!(username = %username, "Session completed normally");
                        break;
                    }
                    Err(e) => {
                        tracing::warn!(username = %username, "Session error: {}", e);
                        break;
                    }
                }
            }

            let _ = execute!(writer, cursor::Show);
            server.active_sessions.fetch_sub(1, Ordering::Relaxed);
            let _ = rt_handle.block_on(async { handle.close(channel_id).await });
        });

        Ok(true)
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(ch) = self.channels.get(&channel) {
            let _ = ch.input_tx.send(data.to_vec());
        }
        Ok(())
    }

    async fn pty_request(
        &mut self,
        channel: ChannelId,
        _term: &str,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _modes: &[(russh::Pty, u32)],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(ch) = self.channels.get(&channel) {
            let _ = ch.size_tx.send((col_width, row_height));
        }
        session.channel_success(channel)?;
        Ok(())
    }

    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        col_width: u32,
        row_height: u32,
        _pix_width: u32,
        _pix_height: u32,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some(ch) = self.channels.get(&channel) {
            let _ = ch.size_tx.send((col_width, row_height));
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.channels.remove(&channel);
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        // Drop the sender to signal disconnection to the game loop.
        self.channels.remove(&channel);
        Ok(())
    }
}
