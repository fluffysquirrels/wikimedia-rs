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
}

#[derive(clap::Subcommand, Clone, Debug)]
enum Command {
    Download(commands::download::Args),
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .parse(std::env::var("RUST_LOG")
                       .unwrap_or("warn,wikimedia_downloader=info".to_string()))?)
        .compact()
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::FULL)
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .init();

    let args = Args::parse();

    if tracing::enabled!(Level::DEBUG) {
        tracing::debug!(args = tracing::field::debug(args.clone()), "parsed CLI args");
    }

    match args.command {
        Command::Download(cmd_args) => commands::download::main(cmd_args).await?,
    };

    Ok(())
}
