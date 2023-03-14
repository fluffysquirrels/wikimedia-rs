use crate::{
    args::CommonArgs,
    page_store,
    Result,
};

/// Get a page from a chunk.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    /// Which page index in the chunk to get.
    #[arg(long)]
    index: usize,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let mut store = page_store::Options::from_common_args(&args.common).build_store()?;

    let chunk = store.map_chunk()?;
    let page = chunk.get_page(args.index)?;

    serde_json::to_writer_pretty(&std::io::stdout(), &page)?;
    println!();

    Ok(())
}
