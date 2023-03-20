use crate::{
    args::{CommonArgs, DumpFileSpecArgs},
    dump,
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
    dump_file_spec: DumpFileSpecArgs,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let pages = dump::local::open_dump_spec(&args.common, &args.dump_file_spec)?;

    let mut store = store::Options::from_common_args(&args.common).build_store()?;
    if args.clear {
        store.clear()?;
    }

    store.import(pages)?;

    Ok(())
}
