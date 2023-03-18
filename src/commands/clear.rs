use crate::{
    args::CommonArgs,
    page_store,
    Result,
};

/// Clear the existing pages store.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let mut store = page_store::Options::from_common_args(&args.common).build_store()?;
    store.clear()?;

    Ok(())
}
