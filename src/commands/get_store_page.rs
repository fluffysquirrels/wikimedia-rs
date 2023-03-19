use anyhow::format_err;
use crate::{
    args::CommonArgs,
    article_dump,
    fbs::wikimedia as wm,
    store::self,
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
    store_page_id: Option<store::StorePageId>,

    #[arg(long)]
    chunk_id: Option<store::ChunkId>,

    /// Choose an output type for the page
    ///
    /// HTML requires `pandoc` to be installed and on your path.
    #[arg(long, value_enum, default_value_t = OutputType::Json)]
    out: OutputType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum OutputType {
    Html,
    Json,
    JsonWithBody,
    None,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let store = store::Options::from_common_args(&args.common).build_store()?;

    match (args.store_page_id, args.chunk_id) {
        (Some(_), Some(_)) => return Err(anyhow::Error::msg(
            "you passed both --store-page-id and --chunk-id arguments, \
             you must use only one of these.")),
        (Some(store_page_id), None) => {
            let page = store.get_page_by_store_id(store_page_id)?
                            .ok_or_else(|| format_err!("page not found by id."))?;
            output_page(&args, page.borrow()).await?;
        },
        (None, Some(chunk_id)) => {
            check_output_type_not_html(args.out)?;
            let chunk = store.get_mapped_chunk_by_chunk_id(chunk_id)?
                             .ok_or_else(|| format_err!("chunk not found by id."))?;
            for page in chunk.pages_iter() {
                output_page(&args, page).await?;
            }
        },
        (None, None) => {
            check_output_type_not_html(args.out)?;
            let mut chunk_ids = store.chunk_id_iter()
                                     .try_collect::<Vec<store::ChunkId>>()?;
            chunk_ids.sort();

            for chunk_id in chunk_ids.into_iter() {
                tracing::debug!(?chunk_id, "Outputting pages from new chunk");
                let chunk = store.get_mapped_chunk_by_chunk_id(chunk_id)?
                                 .ok_or_else(|| format_err!("chunk not found by id."))?;
                for page in chunk.pages_iter() {
                    output_page(&args, page).await?;
                }
            }
        },
    } // End of match on input ID variants.

    Ok(())
}

fn check_output_type_not_html(output_type: OutputType) -> Result<()> {
    match output_type {
        OutputType::Html => Err(anyhow::Error::msg(
            "Cannot use --out Html if more than one page is returned.")),
        _ => Ok(())
    }
}

async fn output_page(args: &Args, page: wm::Page<'_>) -> Result<()> {
    match args.out {
        OutputType::None => {},
        OutputType::Json => {
            let page = store::convert_store_page_to_article_dump_page_without_body(&page)?;
            serde_json::to_writer_pretty(&std::io::stdout(), &page)?;
            println!();
        },
        OutputType::JsonWithBody => {
            let page = article_dump::Page::try_from(&page)?;
            serde_json::to_writer_pretty(&std::io::stdout(), &page)?;
            println!();
        },
        OutputType::Html => {
            let page = article_dump::Page::try_from(&page)?;
            let html = wikitext::convert_page_to_html(&args.common, &page).await?;
            std::io::stdout().write_all(&*html)?;
        }
    }

    Ok(())
}
