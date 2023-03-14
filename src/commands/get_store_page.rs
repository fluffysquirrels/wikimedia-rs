use crate::{
    args::CommonArgs,
    page_store,
    Result,
    wikitext,
};
use std::io::Write;

/// Get a page from a chunk.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    /// Which page index in the chunk to get.
    #[arg(long)]
    index: usize,

    /// Choose an output type for the page: HTML or Json. HTML
    /// requires `pandoc` to be installed and on your path.
    #[arg(long, value_enum, default_value_t = OutputType::Json)]
    out: OutputType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum OutputType {
    Json,
    Html,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let mut store = page_store::Options::from_common_args(&args.common).build_store()?;

    let chunk = store.map_chunk()?;
    let page = chunk.get_page(args.index)?;

    match args.out {
        OutputType::Json => {
            serde_json::to_writer_pretty(&std::io::stdout(), &page)?;
            println!();
        },
        OutputType::Html => {
            let html = wikitext::convert_page_to_html(&args.common, &page).await?;
            std::io::stdout().write_all(&*html)?;
        } // end of OutputType::Html
    } // end of match on OutputType

    Ok(())
}
