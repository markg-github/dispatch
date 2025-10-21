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
use std::sync;

use crate::avahi::AvahiService;
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
use tracing_appender::{non_blocking, rolling};

use anyhow::Error;
use tracing::{event, Level};
use tracing_subscriber::{filter, fmt::time, EnvFilter, prelude::*};
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
}

#[tokio::main]
async fn main() -> Result<()> {

    // setup_tracing();
    setup_logging_to_stderr_and_rolling_file("beacon").unwrap();
    test_tracing_fn();

    // return Ok(());



    // Parse arguments
    let args = Args::parse();

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

    // Create TXT records
    let name = std::env!("CARGO_PKG_NAME");
    let txt = [
        ("description", std::env!("CARGO_PKG_DESCRIPTION")),
        ("version", std::env!("CARGO_PKG_VERSION")),
        ("path", &path),
    ];

    // Start the Avahi service discovery.
    let avahi = AvahiService::new().await?;
    avahi.register(name, addr.port(), &txt).await?;

    // Create event stream for terminal events
    let mut events = EventStream::new();

    // Run the server and wait for quit or terminal events in parallel
    tokio::select! {
        _ = server.serve() => {}
        _ = terminal_events(&mut events, status.clone()) => {}
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
fn setup_tracing() -> Result<()> {

    let file_appender = rolling::daily("logs", "beacon.log");
    let (non_blocking_writer, _guard) = non_blocking(file_appender);
    let subscriber = tracing_subscriber::fmt()
        .with_writer(non_blocking_writer)
        .finish();

    // Create a file to write logs to.
    //    Ensure the directory exists or handle potential errors.
    // let file = File::create("my_application.log")
    //     .expect("Failed to create log file");

    // Create a non-blocking writer for the file.
    //    The `_guard` must be kept alive for the duration of the program,
    //    as it manages the background thread for non-blocking writes.
    // let (non_blocking_writer, _guard) = non_blocking(file);



    




// Log to stdout
    // let stdout_layer = fmt::layer().with_writer(std::io::stdout);

    // // Log to file 1
    // let file1 = File::create("trace_log1.log")?;
    // let file1_writer = BufWriter::new(file1);
    // // let file1_layer = fmt::layer().with_writer(move || file1_writer);
    // let file1_layer = fmt::layer().with_writer(file1_writer.make_writer());




    // let file1_writer = Arc::new(sync::Mutex::new(file1));

    // let file1_layer = fmt::layer().with_writer({
    //     let writer = file1_writer.clone();
    //     move || writer.clone()
    // });




    // Log to file 2
    // let file2 = File::create("trace_log2.log")?;
    // let file2_writer = BufWriter::new(file2);
    // let file2_layer = fmt::layer().with_writer(move || file2_writer);





    // Define the default logging level using EnvFilter.
    let filter = EnvFilter::builder()
        .with_default_directive(Level::INFO.into()) // Default level is INFO
        .from_env_lossy();

    // Configure the subscriber registry.
    // Combine layers into a single subscriber
    // tracing_subscriber::registry()
    // // let subscriber = Registry::default()
    //     // Add the filter layer
    //     .with(filter)
    //     // Add the formatter layer for console output
    //     .with(
    //         fmt::layer()
    //             // .with_writer(non_blocking_writer)
    //             // Use a compact format suitable for CLIs
    //             .compact()
    //             // Display the span target (module path) and line number
    //             .with_target(true)
    //             .with_line_number(true)
    //             .with_span_events(FmtSpan::ENTER | FmtSpan::CLOSE)
    //     )
    //     // .with(stdout_layer)
    //     // .with(file1_layer)
    //     // .with(file2_layer);

    //     // Set the initialized subscriber as the global default.
    //     .init();



        // Set the combined subscriber as the global default
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to set global subscriber");

    // Example usage of tracing macros
    let span = tracing::span!(Level::TRACE, "my_span");
    let _enter = span.enter();
    tracing::info!("This is an informational message");

    Ok(())


    
}


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
    // stderr_log_level: filter::LevelFilter,
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

    tracing_subscriber::registry()
        // .with(
        //     stderr_layer
        //         .with_timer(time::ChronoLocal::rfc_3339())
        //         .with_filter(stderr_log_level),
        // )
        .with(
            file_layer
                .with_timer(time::ChronoLocal::rfc_3339())
                .with_ansi(false)
                .with_filter(filter::LevelFilter::DEBUG),
        )
        .try_init()?;

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

