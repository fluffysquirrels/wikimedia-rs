use crate::{
    args::CommonArgs,
    article_dump,
    // fbs::wikimedia as fbs,
    page_store,
    Result,
};
use std::{
    path::PathBuf,
};

/// Import pages from an article dump file into our pages store.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[arg(long)]
    article_dump_file: PathBuf,

    /// Clear existing data in the store before importing.
    #[arg(long, default_value_t = false)]
    clear: bool,

    /// How many pages to import. No limit if omitted.
    #[arg(long)]
    count: Option<usize>,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let pages = article_dump::open_article_dump_file(&*args.article_dump_file)?;

    let mut store = page_store::Options::from_common_args(&args.common).build_store()?;

    if args.clear {
        store.clear()?;
    }

    match args.count {
        None => store.import(pages)?,
        Some(c) => store.import(pages.take(c))?,
    };

    Ok(())
}
