use crate::{
    args::{CommonArgs, DumpFileSpecArgs},
    dump,
    Result,
};
use std::{
    io::stdout,
};

/// Get pages from an article dump file.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(flatten)]
    dump_file_spec: DumpFileSpecArgs,

    #[arg(long, value_enum, default_value_t = OutputType::Json)]
    out: OutputType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum OutputType {
    Json,
    JsonWithBody,
    None,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let pages = dump::local::open_dump_spec(&args.common, &args.dump_file_spec)?;

    for page in pages {
        let mut page = page?;
        match args.out {
            OutputType::None => (),
            OutputType::Json => {
                if let Some(ref mut rev) = page.revision {
                    rev.text = None;
                }
                serde_json::to_writer_pretty(&stdout(), &page)?;
                println!();
            },
            OutputType::JsonWithBody => {
                serde_json::to_writer_pretty(&stdout(), &page)?;
                println!();
            },
        }
    }

    Ok(())
}
