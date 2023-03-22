use crate::{
    args::{CommonArgs, OpenSpecArgs},
    dump::local::self,
    store,
    Result,
};

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
    let spec = local::OpenSpec::try_from((args.common.clone(), args.open_spec))?;
    let job_files = local::open_spec(spec)?;

    let mut store = store::Options::from_common_args(&args.common).build()?;

    if args.clear {
        store.clear()?;
    }

    store.import(job_files)?;

    Ok(())
}
