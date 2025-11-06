#![forbid(unsafe_code)]
// #![deny(unused_imports)]
// #![deny(unused_variables)]
// #![deny(dead_code)]
#![deny(unreachable_code)]
#![deny(clippy::all)]
#![deny(clippy::pedantic)]
#![deny(clippy::nursery)]
#![deny(clippy::panic)]
#![deny(clippy::unimplemented)]
#![deny(clippy::todo)]

mod avahi;
mod github;
mod http;
mod jobs;
mod tui;

use std::sync::Arc;
// use std::sync;

use crate::avahi::AvahiService;
// use crate::avahi::{AvahiService, EntryGroupState};
use crate::github::GitHubArgs;
use crate::http::Server;
use crate::tui::{Status, Throbbing};

use anyhow::Result;
use clap::Parser;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind};
use futures_util::StreamExt;
use tokio::net::TcpListener;
use tokio::sync::Mutex;

// use tracing::{self, Level};
// use tracing_subscriber::{
//     self, 
//     fmt::{self, format::FmtSpan, Subscriber}, 
//     prelude::*, 
//     util::SubscriberInitExt, 
//     EnvFilter,
//     Registry
// };

// use std::{fs::File, io::BufWriter};
// use tracing_appender::{non_blocking, rolling};
use tracing_appender::rolling;

use anyhow::Error;
use tracing::{event, Level};
// use tracing_subscriber::{filter, fmt::time, EnvFilter, prelude::*};
use tracing_subscriber::{filter, fmt::time, prelude::*};
use std::io;
use std::path;
use std::env;
// use tracing_appender::rolling;



#[derive(Parser)]
#[command(name = std::env!("CARGO_PKG_NAME"))]
#[command(about = std::env!("CARGO_PKG_DESCRIPTION"))]
struct Args {
    #[command(flatten)]
    github: GitHubArgs,

    /// Address and port to bind to
    #[arg(short = 'b', long, default_value = "0.0.0.0:8080")]
    bind: String,

    /// Path to offer services on
    #[arg(short = 'p', long, default_value = concat!("/", std::env!("CARGO_PKG_NAME")))]
    path: String,

    /// When set, do not log to stdout/stderr; only use the rolling log file
    #[arg(long)]
    quiet: bool,

    /// Add a random hex suffix to the Avahi service name (e.g., dispatch-a3f2)
    #[arg(long)]
    avahi_random: bool,

    /// Add a custom suffix to the Avahi service name (e.g., dispatch-mytest)
    #[arg(long, conflicts_with = "avahi_random")]
    avahi_suffix: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {

    // Parse arguments
    let args = Args::parse();

    // Setup logging according to --quiet. When quiet is true we only use
    // the rolling file appender; otherwise log to both stderr and rolling file.
    setup_logging_to_stderr_and_rolling_file("beacon", args.quiet).unwrap();
    test_tracing_fn();

    // Ensure we're authenticated with GitHub
    let github = Arc::new(args.github.login().await?);

    // Bind to our server port
    let listener = TcpListener::bind(&args.bind).await?;
    let addr = listener.local_addr()?;
    let path = Arc::new(args.path);

    // Load the github assets
    let assets = github
        .assets()
        .throbbing("Loading GitHub assets...")
        .await?;

    // Show the main UI
    let status = Arc::new(Mutex::new(Status::new(assets, addr, path.clone())));
    status.lock().await.render()?;

    // Create the HTTP server
    let server = Server::new(listener, status.clone(), github, path.clone())?;

    // Build Avahi service name based on command line options
    // Clients browse by service type (_dispatch._tcp), not instance name
    let name = if args.avahi_random {
        let random_suffix: u32 = rand::random();
        format!("{}-{:08x}", std::env!("CARGO_PKG_NAME"), random_suffix)
    } else if let Some(ref suffix) = args.avahi_suffix {
        format!("{}-{}", std::env!("CARGO_PKG_NAME"), suffix)
    } else {
        std::env!("CARGO_PKG_NAME").to_string()
    };
    tracing::debug!(avahi_name = %name, "Generated Avahi service name");
    let txt = [
        ("description", std::env!("CARGO_PKG_DESCRIPTION")),
        ("version", std::env!("CARGO_PKG_VERSION")),
        ("path", &path),
    ];
    tracing::debug!(txt = ?txt);

    // Start the Avahi service discovery.
    let avahi = AvahiService::new().await?;
    avahi.register(name.as_str(), addr.port(), &txt).await?;
    tracing::info!(
        service_name = %name,
        port = addr.port(),
        "Avahi initial registration complete"
    );

    // Create event stream for terminal events
    let mut events = EventStream::new();

    // Periodic Avahi state monitoring (logging only, no recovery)
    let avahi_task = {
        let name_clone = name.clone();
        let port = addr.port();
        async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                match avahi.state().await {
                    Ok(state) => {
                        tracing::debug!(
                            service_name = %name_clone,
                            port = port,
                            avahi_state = ?state,
                            "Avahi health check"
                        );
                    }
                    Err(e) => {
                        tracing::error!(
                            service_name = %name_clone,
                            port = port,
                            error = %e,
                            "Failed to query Avahi state"
                        );
                    }
                }
            }
        }
    };

    // Run the server and wait for quit or terminal events in parallel
    tokio::select! {
        _ = server.serve() => {}
        _ = terminal_events(&mut events, status.clone()) => {}
        _ = avahi_task => {
            tracing::warn!("Avahi health monitoring task ended unexpectedly");
        }
    }

    ratatui::restore();
    Ok(())
}

#[tracing::instrument(level = tracing::Level::INFO, skip(status))]
async fn terminal_events(events: &mut EventStream, status: Arc<Mutex<Status>>) -> Result<()> {
    loop {
        if let Some(event) = events.next().await {
            match event? {
                // Quit on 'q' key press
                Event::Key(KeyEvent {
                    code: KeyCode::Char('q'),
                    kind: KeyEventKind::Press,
                    ..
                }) => break,

                // Re-render the status on terminal resize
                Event::Resize(_, _) => status.lock().await.render()?,

                // Ignore other events
                _ => {}
            }
        }
    }

    Ok(())
}

// --- Instrumentation Setup ---
/// Initializes the global tracing subscriber.
/// The logging level is controlled by the RUST_LOG environment variable, 
/// defaulting to 'info' if not set.


#[tracing::instrument(level = tracing::Level::INFO)]
fn test_tracing_fn() {
    tracing::trace!("This is a trace message");
    tracing::debug!("This is a debug message");
    tracing::info!("This is an info message");
    tracing::warn!("This is a warning message");
    tracing::error!("This is an error message");
}



pub fn setup_logging_to_stderr_and_rolling_file(
    filename_prefix: &str,
    quiet: bool,
) -> Result<(), Error> {
    let stderr_log_level = filter::LevelFilter::INFO;
    // let stderr_layer = tracing_subscriber::fmt::layer()
    //     .pretty()
    //     .with_writer(io::stderr);

    let tmp_dir = get_tmp_dir();

    let file_layer = tracing_subscriber::fmt::layer().pretty().with_writer(
        rolling::RollingFileAppender::builder()
            .rotation(rolling::Rotation::DAILY)
            .filename_prefix(filename_prefix)
            .filename_suffix("log")
            .build(&tmp_dir)?,
    );

    // Build the registry conditionally including the stderr layer.
    // Build a stderr layer that is either disabled (quiet) or writes to stderr.
    let stderr_layer = if quiet {
        // disabled layer with OFF filter
        tracing_subscriber::fmt::layer()
            .pretty()
            .with_writer(io::stderr)
            .with_timer(time::ChronoLocal::rfc_3339())
            .with_filter(filter::LevelFilter::OFF)
    } else {
        tracing_subscriber::fmt::layer()
            .pretty()
            .with_writer(io::stderr)
            .with_timer(time::ChronoLocal::rfc_3339())
            .with_filter(stderr_log_level)
    };

    // Attach timer and filtering to the file layer and compose the subscriber.
    let registry = tracing_subscriber::registry()
        .with(stderr_layer)
        .with(
            file_layer
                .with_timer(time::ChronoLocal::rfc_3339())
                .with_ansi(false)
                .with_filter(filter::LevelFilter::DEBUG),
        );

    registry.try_init()?;

    let log_dir_abs_path = match path::Path::new(&tmp_dir).canonicalize() {
        Ok(v) => v,
        Err(_) => path::PathBuf::from(tmp_dir),
    };

    event!(Level::INFO, "log dir = {}", log_dir_abs_path.display());

    Ok(())
}

fn get_tmp_dir() -> String {
    match env::var("TMPDIR").or_else(|_| env::var("TEMP")) {
        Ok(v) => v,
        Err(_) => "log".into(),
    }
}

