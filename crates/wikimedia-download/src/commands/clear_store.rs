use crate::args::CommonArgs;
use wikimedia::Result;

/// Clear the existing pages store.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let mut store = args.common.store_options()?.build()?;

    store.clear()?;

    Ok(())
}
