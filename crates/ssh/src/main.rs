use std::path::PathBuf;
use std::sync::Arc;

use russh::server::Server as _;
use russh_keys::PrivateKey;

mod accounts;
mod ansi_input;
mod channel_writer;
mod lobby;
mod saves;
mod server;
mod session;
mod ssh_input;

use server::{ServerState, SshHandler};

/// Default port for the SSH server.
const DEFAULT_PORT: u16 = 2222;
/// Default max connections.
const DEFAULT_MAX_CONNECTIONS: usize = 64;
/// Default idle timeout in minutes.
const DEFAULT_IDLE_TIMEOUT_MIN: u64 = 30;

fn default_data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("roguelike-ssh")
}

/// Parse CLI arguments (simple hand-rolled parser to avoid adding clap).
struct Args {
    port: u16,
    data_dir: PathBuf,
    max_connections: usize,
    idle_timeout_secs: u64,
}

impl Args {
    fn parse() -> Self {
        let mut port = std::env::var("ROGUELIKE_SSH_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let mut data_dir = std::env::var("ROGUELIKE_SSH_DATA_DIR")
            .ok()
            .map(PathBuf::from)
            .unwrap_or_else(default_data_dir);
        let mut max_connections = DEFAULT_MAX_CONNECTIONS;
        let mut idle_timeout_secs = DEFAULT_IDLE_TIMEOUT_MIN * 60;

        let args: Vec<String> = std::env::args().collect();
        let mut i = 1;
        while i < args.len() {
            match args[i].as_str() {
                "--port" => {
                    i += 1;
                    if let Some(val) = args.get(i) {
                        port = val.parse().unwrap_or(port);
                    }
                }
                "--data-dir" => {
                    i += 1;
                    if let Some(val) = args.get(i) {
                        data_dir = PathBuf::from(val);
                    }
                }
                "--max-connections" => {
                    i += 1;
                    if let Some(val) = args.get(i) {
                        max_connections = val.parse().unwrap_or(max_connections);
                    }
                }
                "--idle-timeout" => {
                    i += 1;
                    if let Some(val) = args.get(i) {
                        // Parse as minutes
                        idle_timeout_secs =
                            val.parse::<u64>().unwrap_or(DEFAULT_IDLE_TIMEOUT_MIN) * 60;
                    }
                }
                "--help" | "-h" => {
                    eprintln!(
                        "roguelike-ssh - SSH roguelike server\n\n\
                         Usage: roguelike-ssh [OPTIONS]\n\n\
                         Options:\n  \
                           --port <PORT>             Listen port (default: {DEFAULT_PORT})\n  \
                           --data-dir <PATH>         Data directory (default: ~/.local/share/roguelike-ssh/)\n  \
                           --max-connections <N>     Max simultaneous connections (default: {DEFAULT_MAX_CONNECTIONS})\n  \
                           --idle-timeout <MINUTES>  Idle timeout in minutes (default: {DEFAULT_IDLE_TIMEOUT_MIN})\n  \
                           --help                    Show this help\n\n\
                         Environment:\n  \
                           ROGUELIKE_SSH_PORT        Override port\n  \
                           ROGUELIKE_SSH_DATA_DIR    Override data directory"
                    );
                    std::process::exit(0);
                }
                other => {
                    eprintln!("Unknown argument: {other}. Use --help for usage.");
                    std::process::exit(1);
                }
            }
            i += 1;
        }

        Self {
            port,
            data_dir,
            max_connections,
            idle_timeout_secs,
        }
    }
}

/// Load or generate the server's ED25519 host key.
fn load_or_generate_host_key(data_dir: &std::path::Path) -> PrivateKey {
    let key_path = data_dir.join("host_key");

    // Try loading existing key
    if key_path.exists() {
        match russh_keys::load_secret_key(&key_path, None) {
            Ok(key) => {
                tracing::info!("Loaded host key from {}", key_path.display());
                return key;
            }
            Err(e) => {
                tracing::warn!("Failed to load host key: {}. Generating new one.", e);
            }
        }
    }

    // Generate new ED25519 key
    tracing::info!("Generating new ED25519 host key...");
    let key = PrivateKey::random(&mut rand::thread_rng(), russh_keys::Algorithm::Ed25519)
        .expect("Failed to generate host key");

    // Save it
    if let Err(e) = std::fs::create_dir_all(data_dir) {
        tracing::warn!("Failed to create data dir: {}", e);
    }

    // Write the key in PKCS8 PEM format
    let mut key_buf = Vec::new();
    match russh_keys::encode_pkcs8_pem(&key, &mut key_buf) {
        Ok(()) => match std::fs::write(&key_path, &key_buf) {
            Ok(()) => {
                // Set permissions to 600 on Unix
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600));
                }
                tracing::info!("Saved host key to {}", key_path.display());
            }
            Err(e) => {
                tracing::warn!("Failed to save host key: {}", e);
            }
        },
        Err(e) => {
            tracing::warn!("Failed to encode host key: {}", e);
        }
    }

    key
}

struct SshServerImpl {
    state: Arc<ServerState>,
}

impl russh::server::Server for SshServerImpl {
    type Handler = SshHandler;

    fn new_client(&mut self, addr: Option<std::net::SocketAddr>) -> SshHandler {
        tracing::info!(?addr, "New connection");
        SshHandler::new(Arc::clone(&self.state))
    }
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "roguelike_ssh=info".parse().unwrap()),
        )
        .init();

    let args = Args::parse();

    tracing::info!(
        port = args.port,
        data_dir = %args.data_dir.display(),
        max_connections = args.max_connections,
        idle_timeout_secs = args.idle_timeout_secs,
        "Starting roguelike SSH server"
    );

    // Ensure data directory exists
    if let Err(e) = std::fs::create_dir_all(&args.data_dir) {
        tracing::error!("Failed to create data directory: {}", e);
        std::process::exit(1);
    }

    let host_key = load_or_generate_host_key(&args.data_dir);

    let config = russh::server::Config {
        keys: vec![host_key],
        ..Default::default()
    };

    let state = Arc::new(ServerState::new(
        args.data_dir,
        args.max_connections,
        args.idle_timeout_secs,
    ));

    let mut server = SshServerImpl {
        state: Arc::clone(&state),
    };

    let addr = format!("0.0.0.0:{}", args.port);
    tracing::info!("Listening on {}", addr);

    match server.run_on_address(Arc::new(config), &addr).await {
        Ok(()) => tracing::info!("Server shut down gracefully"),
        Err(e) => {
            tracing::error!("Server error: {}", e);
            std::process::exit(1);
        }
    }
}
