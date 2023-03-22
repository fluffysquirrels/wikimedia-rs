use crate::{
    args::CommonArgs,
    store,
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
    let mut store = store::Options::from_common_args(&args.common).build()?;
    store.clear()?;

    Ok(())
}
