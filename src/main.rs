mod args;
mod commands;

use clap::Parser;
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

    /// Set this flag to enable logging to stdout as JSON. Logs are in a a text format by default.
    #[arg(long, default_value_t = false)]
    log_json: bool,
}

#[derive(clap::Subcommand, Clone, Debug)]
enum Command {
    Download(commands::download::Args),
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
        .with_target(true);

    if args.log_json {
        sub_builder.json().init();
    } else {
        sub_builder.pretty().init();
    }

    if tracing::enabled!(Level::DEBUG) {
        tracing::debug!(args = tracing::field::debug(args.clone()), "parsed CLI args");
    }

    match args.command {
        Command::Download(cmd_args) => commands::download::main(cmd_args).await?,
    };

    Ok(())
}
