use crate::args::CommonArgs;
use wikimedia::Result;
use wikimedia_store as store;

/// Clear the existing pages store.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let mut store =
        store::Options::default()
            .path(args.common.store_path())
            .build()?;
    store.clear()?;

    Ok(())
}
