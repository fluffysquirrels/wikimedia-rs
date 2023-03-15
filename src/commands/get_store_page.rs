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

    /// The store page ID to get.
    #[arg(long)]
    id: page_store::StorePageId,

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
    let store = page_store::Options::from_common_args(&args.common).build_store()?;

    let page = store.get_page_by_store_id(args.id)?;

    match args.out {
        OutputType::Json => {
            serde_json::to_writer_pretty(&std::io::stdout(), &page)?;
            println!();
        },
        OutputType::Html => {
            let html = wikitext::convert_page_to_html(&args.common, &page).await?;
            std::io::stdout().write_all(&*html)?;
        }
    }

    Ok(())
}
