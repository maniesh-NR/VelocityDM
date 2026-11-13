//! VelocityDM daemon + CLI + GUI entry point.

#[cfg(feature = "gui")]
mod gui;

mod cli;
mod daemon;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "velocitydm",
    version,
    about = "VelocityDM - fast, clean-room download manager (HTTP/HLS/BitTorrent)",
    long_about = "VelocityDM accelerates downloads with segmented connections,\nBitTorrent support, scheduling, and browser integration."
)]
struct Cli {
    /// Portable mode: keep settings next to the executable.
    #[arg(long, global = true)]
    portable: bool,
    /// Daemon address (default 127.0.0.1:36990).
    #[arg(long, global = true, default_value = "127.0.0.1:36990")]
    addr: String,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the background daemon (engine + JSON-RPC/SSE server).
    Daemon,
    /// Start the graphical interface (starts the daemon in-process).
    #[cfg(feature = "gui")]
    Gui,
    /// Add an HTTP/HLS download.
    Add {
        url: String,
        /// Output file name.
        #[arg(long)]
        out: Option<String>,
        /// Download directory (defaults to settings).
        #[arg(long)]
        dir: Option<String>,
        /// Connections per download.
        #[arg(long)]
        conn: Option<usize>,
        /// Mirror URL (repeatable).
        #[arg(long = "mirror")]
        mirrors: Vec<String>,
        /// Per-download speed limit (e.g. 2MB, 512K).
        #[arg(long)]
        limit: Option<String>,
        /// Verify checksum: ALGO:HEX (md5|sha1|sha256).
        #[arg(long)]
        checksum: Option<String>,
        /// HLS quality: best|worst|<bandwidth>.
        #[arg(long)]
        quality: Option<String>,
        /// Queue priority (0-255, higher runs first).
        #[arg(long)]
        priority: Option<u8>,
        /// Schedule the download to start at a UNIX timestamp.
        #[arg(long)]
        start_at: Option<u64>,
        /// Ignore any saved progress and restart.
        #[arg(long)]
        fresh: bool,
    },
    /// Add a batch from a URL pattern like https://x/pic[001:120].jpg
    Batch {
        pattern: String,
        #[arg(long)]
        dir: Option<String>,
        #[arg(long)]
        conn: Option<usize>,
        #[arg(long)]
        limit: Option<String>,
    },
    /// List downloads.
    List {
        /// Machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show one download in detail.
    Status {
        id: u64,
    },
    /// Pause a download (or all).
    Pause {
        id: Option<u64>,
    },
    /// Resume a paused download.
    Resume {
        id: u64,
    },
    /// Remove a download from the list.
    Remove {
        id: u64,
        /// Also delete unfinished .part data.
        #[arg(long)]
        delete: bool,
    },
    /// Give a paused/failed download a fresh URL (XDM refresh link).
    Refresh {
        id: u64,
        #[arg(long)]
        url: String,
    },
    /// Add a .torrent download.
    Torrent {
        /// Path to the .torrent file.
        file: String,
        #[arg(long)]
        dir: Option<String>,
        /// Only download files with these indexes (comma list, 0-based).
        #[arg(long, value_delimiter = ',')]
        files: Option<Vec<usize>>,
        #[arg(long)]
        limit: Option<String>,
    },
    /// Add a magnet: download.
    Magnet {
        uri: String,
        #[arg(long)]
        dir: Option<String>,
        #[arg(long, value_delimiter = ',')]
        files: Option<Vec<usize>>,
        #[arg(long)]
        limit: Option<String>,
    },
    /// Create a .torrent from a file or directory.
    TorrentCreate {
        /// Input file or directory.
        input: String,
        #[arg(long)]
        announce: Option<String>,
        /// Piece length override (bytes).
        #[arg(long)]
        piece_len: Option<u64>,
        /// Output .torrent path (default: <input>.torrent).
        #[arg(short, long)]
        output: Option<String>,
    },
    /// Seed a file: start a tracker + seeder and print the magnet link.
    Seed {
        file: String,
        /// Port for the tracker+seeder (0 = ephemeral).
        #[arg(long, default_value = "0")]
        port: u16,
    },
    /// Show or set speed limits.
    Limits {
        /// Global limit for all downloads (e.g. 5MB, 0 = unlimited).
        #[arg(long)]
        global: Option<String>,
    },
    /// Show or change the scheduler configuration.
    Schedule {
        /// Daily pause window start (HH:MM).
        #[arg(long)]
        stop_start: Option<String>,
        /// Daily pause window end (HH:MM).
        #[arg(long)]
        stop_end: Option<String>,
        /// Clear the stop window.
        #[arg(long)]
        clear: bool,
        /// Max simultaneous downloads.
        #[arg(long)]
        max_active: Option<usize>,
        /// Shut the daemon down when the queue drains.
        #[arg(long)]
        shutdown_when_done: bool,
        /// Run a command when the queue drains.
        #[arg(long)]
        on_drained: Option<String>,
    },
    /// Show or change settings.
    Settings {
        #[arg(long)]
        dir: Option<String>,
        #[arg(long)]
        conn: Option<usize>,
        /// Proxy URL: http://host:port or socks5://user:pass@host:port ("none" to clear).
        #[arg(long)]
        proxy: Option<String>,
        #[arg(long)]
        user_agent: Option<String>,
    },
    /// Print version and feature summary.
    /// Ask a running daemon to shut down cleanly.
    Shutdown,
    Version,
}

fn main() {
    let cli = Cli::parse();
    let addr = cli.addr.clone();
    let code = match &cli.command {
        Commands::Daemon => daemon::run(&addr, cli.portable),
        #[cfg(feature = "gui")]
        Commands::Gui => gui::run(&addr, cli.portable),
        other => other.run_remote(&addr),
    };
    std::process::exit(code);
}
