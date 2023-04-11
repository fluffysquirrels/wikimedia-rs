use anyhow::{bail, Context, format_err};
use crate::args::CommonArgs;
use std::{
    fs,
    io::Write,
};
use wikimedia::{
    dump,
    Result,
    slug,
    util::rand::rand_hex,
    wikitext,
};
use wikimedia_store::{
    self as store,
    capnp::wikimedia_capnp as wmc,
    StorePageId,
};

/// Get a page from the store.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    /// The single store page ID to get.
    #[arg(long)]
    store_page_id: Option<StorePageId>,

    /// The chunk ID to get all the pages from.
    #[arg(long)]
    chunk_id: Option<store::ChunkId>,

    /// The mediawiki ID of the page to get.
    #[arg(long)]
    mediawiki_id: Option<u64>,

    /// The slug of the page to get.
    #[arg(long)]
    slug: Option<String>,

    /// Choose an output type for the page
    ///
    /// HTML requires `pandoc` to be installed and on your path.
    #[arg(long, value_enum, default_value_t = OutputType::Json)]
    out: OutputType,

    /// The maximum number of pages to get. No limit if not set.
    #[arg(long)]
    limit: Option<u64>,

    /// Open the output HTML file in your browser. Requires `--out html`.
    #[arg(long, default_value_t = false)]
    open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
enum OutputType {
    /// Output the page's Wikitext markup content as HTML.
    Html,

    /// Output the page as a JSON object, without the body text.
    Json,

    /// Output the page as a JSON object, including the body text.
    JsonWithBody,

    /// Copy the page title and IDs to an in-memory object, then discard it without outputting anything.
    /// Sometimes useful for benchmarking.
    LoadDiscard,

    /// Copy the page IDs to an in-memory object, then discard it without outputting anything.
    /// Sometimes useful for benchmarking.
    LoadIdDiscard,

    /// Output nothing. Sometimes useful for benchmarking.
    None,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    if args.open && args.out != OutputType::Html {
        bail!("If argument `--open` is passed then argument `--out` must equal `html`.");
    }

    let arg_groups_given: Vec<&'static str> = [
            args.store_page_id.as_ref().map(|_| "--store-page-id"),
            args.mediawiki_id.as_ref().map(|_| "--mediawiki-id"),
            args.slug.as_ref().map(|_| "--slug"),
            args.chunk_id.as_ref().map(|_| "--chunk-id"),
        ].into_iter().flatten().collect();

    if arg_groups_given.len() > 1{
        bail!("You passed multiple arguments specifying which pages to get: {opts}.\n\
               You must pass only one of these arguments.",
              opts = arg_groups_given.join(", "));
    }

    let store = args.common.store_options()?.build()?;

    let mut count: u64 = 0;

    match (args.store_page_id, args.mediawiki_id, args.slug.as_ref(), args.chunk_id) {
        (Some(store_page_id), None, None, None) => {
            let page = store.get_page_by_store_id(store_page_id)?
                            .ok_or_else(|| format_err!("page not found by id."))?;
            output_page(&args, page.borrow()?).await?;
            count += 1;
        },
        (None, Some(mediawiki_id), None, None) => {
            let page = store.get_page_by_mediawiki_id(mediawiki_id)?
                            .ok_or_else(|| format_err!("page not found by mediawiki-id."))?;
            output_page(&args, page.borrow()?).await?;
            count += 1;
        },
        (None, None, Some(slug), None) => {
            let page = store.get_page_by_slug(slug)?
                            .ok_or_else(|| format_err!("page not found by slug."))?;
            output_page(&args, page.borrow()?).await?;
            count += 1;
        },
        (None, None, None, Some(chunk_id)) => {
            check_output_type_not_html(args.out)?;
            let chunk = store.map_chunk(chunk_id)?
                             .ok_or_else(|| format_err!("chunk not found by id."))?;
            for (_store_id, page) in chunk.pages_iter()? {
                output_page(&args, page).await?;
                count += 1;

                if args.limit.is_some() && count >= args.limit.unwrap() {
                    break;
                }
            }
        },
        (None, None, None, None) => {
            check_output_type_not_html(args.out)?;
            let mut chunk_ids = store.chunk_id_iter()
                                     .try_collect::<Vec<store::ChunkId>>()?;
            chunk_ids.sort();

            'by_chunk:
            for chunk_id in chunk_ids.into_iter() {
                tracing::debug!(?chunk_id, "Outputting pages from new chunk");
                let chunk = store.map_chunk(chunk_id)?
                                 .ok_or_else(|| format_err!("chunk not found by id."))?;
                '_by_page:
                for (_store_id, page) in chunk.pages_iter()? {
                    output_page(&args, page).await?;
                    count += 1;

                    if args.limit.is_some() && count >= args.limit.unwrap() {
                        break 'by_chunk;
                    }
                }
            }
        },
        _ => unreachable!(),
    } // End of match on input ID variants.

    tracing::info!(page_count = count, "get-store-page complete");

    Ok(())
}

fn check_output_type_not_html(output_type: OutputType) -> Result<()> {
    match output_type {
        OutputType::Html => bail!(
            "Cannot use --out Html if more than one page might be returned."),
        _ => Ok(())
    }
}

async fn output_page(args: &Args, page: wmc::page::Reader<'_>) -> Result<()>
{
    match args.out {
        OutputType::None => {},
        OutputType::LoadDiscard => {
            let page = store::convert_store_page_to_dump_page_without_body(&page)?;
            drop(page);
        }
        OutputType::LoadIdDiscard => {
            let _ = page.get_ns_id();
            let _ = page.get_id();
            let _ = if page.has_revision() {
                let revision = page.get_revision()?;
                let _ = revision.get_id();
            };
        }
        OutputType::Json => {
            let page = store::convert_store_page_to_dump_page_without_body(&page)?;
            serde_json::to_writer_pretty(&std::io::stdout(), &page)?;
            println!();
        },
        OutputType::JsonWithBody => {
            let page = dump::Page::try_from(&page)?;
            serde_json::to_writer_pretty(&std::io::stdout(), &page)?;
            println!();
        },
        OutputType::Html => {
            let page = dump::Page::try_from(&page)?;
            let html = wikitext::convert_page_to_html(&page, &args.common.store_dump_name(),
                                                      &*args.common.out_dir()).await?;

            if args.open {
                // Write page HTML to a temp file.
                let slug = slug::title_to_slug(&*page.title);

                // Add rand a random value to output file names to
                // avoid overwriting data from previous runs.
                let rand = rand_hex(8);

                let path = args.common.out_dir().join(
                    format!("temp/pages/{slug}_{rand}.html"));
                let parent = path.parent().expect("path has parent by construction");

                // Closure to add error context.
                (|| -> Result<()> {
                    println!("\nWrite output HTML to {path} . . .\n", path = path.display());

                    fs::create_dir_all(parent)?;
                    fs::write(&*path, html.as_bytes())?;

                    // Open the html file using the operating system's default method,
                    // should use a web browser.
                    open::that(&*path)
                        .with_context(|| "opening the HTML file in your browser")?;

                    Ok(())
                })().with_context(|| format!("saving HTML to file and opening it in a browser \
                                              file_path={path}",
                                             path = (&*path).display()))?;
            } else {
                // args.open == false
                std::io::stdout().write_all(html.as_bytes())?;
            }
        }
    }

    Ok(())
}
