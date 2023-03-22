use crate::{
    args::{CommonArgs, OpenSpecArgs},
    dump::local::{self, OpenSpec},
    Result,
};
use rayon::prelude::*;
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

    #[arg(long, default_value_t = false, action = clap::ArgAction::Set)]
    parallel: bool,
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

    if args.parallel {
        let par_iter = job_files.open_pages_par_iter()?;
        par_iter
            .try_for_each_init(
                || 0_u64,
                |state: &mut u64, _page| -> Result<()> {
                    *state += 1;
                    if *state % 100 == 0 {
                        println!("state = {state} @ thread {id:?}",
                                 id = rayon::current_thread_index());
                    }
                    Ok(())
                })?;
    } else {
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
    }

    Ok(())
}
