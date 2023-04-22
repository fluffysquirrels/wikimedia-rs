use crate::args::{CommonArgs, OpenSpecArgs};
use std::io::stdout;
use wikimedia::Result;

/// Get pages from an article dump file.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(flatten)]
    open_spec: OpenSpecArgs,

    /// How to format the data fetched.
    #[arg(long, value_enum, default_value_t = OutputType::Json)]
    out: OutputType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum OutputType {
    /// Format the pages as JSON, with no body text. Does include the revision's  categories list.
    Json,

    /// Format the pages as JSON, including the body text.
    JsonWithBody,

    /// Do not print output; sometimes useful for benchmarking and testing.
    None,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let job_files = args.open_spec.try_into_open_spec(&*args.common.dumps_path())?
                        .open()?;

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
