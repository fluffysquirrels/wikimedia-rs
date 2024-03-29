#![feature(
    async_closure,
    iterator_try_collect,
    iterator_try_reduce,
)]

mod args;
mod commands;

use clap::Parser;
use tracing::Level;
use valuable::Valuable;
use wikimedia::{
    Result,
    util,
};

#[derive(clap::Parser, Clone, Debug)]
#[command(version, about)]
struct Args {
    #[command(subcommand)]
    command: Command,

    /// Set this flag to enable logging to stderr as JSON. Logs are in a text format by default.
    #[arg(long, default_value_t = false, global = true)]
    log_json: bool,
}

#[derive(clap::Subcommand, Clone, Debug)]
enum Command {
    ClearStore(commands::clear_store::Args),
    Completion(commands::completion::Args),
    Download(commands::download::Args),
    GetChunk(commands::get_chunk::Args),
    GetDump(commands::get_dump::Args),
    GetDumpPage(commands::get_dump_page::Args),
    GetFileInfo(commands::get_file_info::Args),
    GetJob(commands::get_job::Args),
    GetStorePage(commands::get_store_page::Args),
    GetVersion(commands::get_version::Args),
    ImportDump(commands::import_dump::Args),
    Web(commands::web::Args),
}

#[derive(Eq, PartialEq)]
enum LogMode {
    Pretty,
    Json,
}

#[tokio::main]
async fn main() -> Result<()> {
    let start_time = std::time::Instant::now();

    let args = Args::parse();

    init_logging(args.log_json)?;

    if tracing::enabled!(Level::DEBUG) {
        tracing::debug!(args = ?args.clone(), "parsed CLI args");
    }

    // Wrap command dispatch in a closure to log errors.
    let res = (|| async {
        match args.command {
            Command::ClearStore(cmd_args)   => commands::clear_store::   main(cmd_args).await?,
            Command::Completion(cmd_args)   => commands::completion::    main(cmd_args).await?,
            Command::Download(cmd_args)     => commands::download::      main(cmd_args).await?,
            Command::GetChunk(cmd_args)     => commands::get_chunk::     main(cmd_args).await?,
            Command::GetDump(cmd_args)      => commands::get_dump::      main(cmd_args).await?,
            Command::GetDumpPage(cmd_args)  => commands::get_dump_page:: main(cmd_args).await?,
            Command::GetFileInfo(cmd_args)  => commands::get_file_info:: main(cmd_args).await?,
            Command::GetJob(cmd_args)       => commands::get_job::       main(cmd_args).await?,
            Command::GetStorePage(cmd_args) => commands::get_store_page::main(cmd_args).await?,
            Command::GetVersion(cmd_args)   => commands::get_version::   main(cmd_args).await?,
            Command::ImportDump(cmd_args)   => commands::import_dump::   main(cmd_args).await?,
            Command::Web(cmd_args)          => commands::web::           main(cmd_args).await?,
        }

        anyhow::Ok(())
    })().await;

    let duration = util::fmt::Duration(start_time.elapsed());

    tracing::info!(duration = duration.as_value(), "wmd::main() returning");

    if let Err(err) = res {
        // Record an error with tracing as this will output properly formatted JSON (if enabled).

        tracing::error!(%err, "Command returned with an error.");

        // Return the error too so Rust can print a pretty stack trace display.
        return Err(err)
    }

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
