use crate::{
    args::{CommonArgs, OpenSpecArgs},
    dump::local::{self, OpenSpec},
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
    open_spec: OpenSpecArgs,

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
    let spec = OpenSpec::try_from((args.common, args.open_spec))?;
    let job_files = local::open_spec(spec)?;

    for page in job_files.open_pages_iter()? {
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
