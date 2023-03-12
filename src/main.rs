#![feature(async_closure)]

// This module is first to import its macro.
#[macro_use]
mod lazy_regex;

mod args;
mod commands;
mod http;
mod operations;
mod temp_dir;
mod types;
mod user_regex;

use clap::Parser;
use crate::{
    temp_dir::TempDir,
    user_regex::UserRegex,
};
use tracing::Level;

type Result<T> = std::result::Result<T, anyhow::Error>;

#[derive(clap::Parser, Clone, Debug)]
struct Args {
    #[command(subcommand)]
    command: Command,

    /// Set this flag to enable logging to stderr as JSON. Logs are in a text format by default.
    #[arg(long, default_value_t = false)]
    log_json: bool,
}

#[derive(clap::Subcommand, Clone, Debug)]
enum Command {
    Completion(commands::completion::Args),
    Download(commands::download::Args),
    GetDump(commands::get_dump::Args),
    GetFileInfo(commands::get_file_info::Args),
    GetJob(commands::get_job::Args),
    GetVersion(commands::get_version::Args),
}

#[derive(Eq, PartialEq)]
enum LogMode {
    Pretty,
    Json,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    init_logging(args.log_json)?;

    if tracing::enabled!(Level::DEBUG) {
        tracing::debug!(args = ?args.clone(), "parsed CLI args");
    }

    match args.command {
        Command::Completion(cmd_args) => commands::completion::main(cmd_args).await?,
        Command::Download(cmd_args) => commands::download::main(cmd_args).await?,
        Command::GetDump(cmd_args) => commands::get_dump::main(cmd_args).await?,
        Command::GetFileInfo(cmd_args) => commands::get_file_info::main(cmd_args).await?,
        Command::GetJob(cmd_args) => commands::get_job::main(cmd_args).await?,
        Command::GetVersion(cmd_args) => commands::get_version::main(cmd_args).await?,
    };

    Ok(())
}

fn init_logging(log_json: bool) -> Result<()> {
    use tracing_bunyan_formatter::{
        BunyanFormattingLayer,
        JsonStorageLayer,
    };
    use tracing_subscriber::{
        EnvFilter,
        filter::LevelFilter,
        fmt,
        prelude::*,
    };

    let log_mode = if log_json { LogMode::Json } else { LogMode::Pretty };

    tracing_subscriber::Registry::default()
        .with(if log_mode == LogMode::Pretty {
                  Some(fmt::Layer::new()
                           .event_format(fmt::format()
                                             .pretty()
                                             .with_timer(fmt::time::UtcTime::<_>::
                                                             rfc_3339())
                                             .with_target(true)
                                             .with_source_location(true)
                                             .with_thread_ids(true))
                           .with_writer(std::io::stderr)
                           .with_span_events(fmt::format::FmtSpan::NEW
                                             | fmt::format::FmtSpan::CLOSE))
              } else {
                  None
              })
        .with(if log_mode == LogMode::Json {
                  Some(JsonStorageLayer
                           .and_then(BunyanFormattingLayer::new(
                               env!("CARGO_CRATE_NAME").to_string(),
                               std::io::stderr)))
              } else {
                  None
              })
        // Global filter
        .with(EnvFilter::builder()
                  .with_default_directive(LevelFilter::INFO.into())
                  .parse(std::env::var("RUST_LOG")
                             .unwrap_or(format!("warn,{crate_}=info",
                                                crate_ = env!("CARGO_CRATE_NAME"))))?)
        .try_init()?;

    Ok(())
}
