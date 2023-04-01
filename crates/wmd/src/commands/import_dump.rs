use crate::args::{CommonArgs, OpenSpecArgs};
use wikimedia::Result;

/// Import pages from an article dump into our store.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    /// Clear existing data in the store before importing.
    #[arg(long, default_value_t = false)]
    clear: bool,

    #[clap(flatten)]
    open_spec: OpenSpecArgs,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let job_files = args.open_spec.try_into_open_spec(&*args.common.dumps_path())?
                        .open()?;

    let mut store = args.common.store_options()?.build()?;

    if args.clear {
        store.clear()?;
    }

    store.import(job_files)?;

    Ok(())
}
