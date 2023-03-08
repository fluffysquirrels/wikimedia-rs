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
use tracing_subscriber::{
    EnvFilter,
    filter::LevelFilter,
};

type Result<T> = std::result::Result<T, anyhow::Error>;

#[derive(clap::Parser, Clone, Debug)]
struct Args {
    #[command(subcommand)]
    command: Command,

    /// Set this flag to enable logging to stdout as JSON. Logs are in a text format by default.
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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let sub_builder = tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .parse(std::env::var("RUST_LOG")
                       .unwrap_or("warn,wikimedia_downloader=info".to_string()))?)
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::FULL)
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .with_writer(std::io::stderr);

    if args.log_json {
        sub_builder.json().init();
    } else {
        sub_builder.pretty().init();
    }

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
