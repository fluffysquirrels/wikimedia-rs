use crate::{
    args::{CommonArgs, DumpNameArg, JobNameArg, VersionArg},
    article_dump,
    store,
    Result,
    util::IteratorExt,
};
use std::{
    path::PathBuf,
};

/// Import pages from an article dump into our store.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(flatten)]
    dump_name: DumpNameArg,

    #[clap(flatten)]
    version: VersionArg,

    #[clap(flatten)]
    job_name: JobNameArg,

    #[arg(long)]
    article_dump_file: Option<PathBuf>,

    /// Clear existing data in the store before importing.
    #[arg(long, default_value_t = false)]
    clear: bool,

    /// Maximum count of pages to import. No limit if omitted.
    #[arg(long)]
    count: Option<usize>,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let pages = match args.article_dump_file {
        Some(path) => article_dump::open_article_dump_file(&*path)?.boxed(),
        None => article_dump::open_article_dump_job(
            &*args.common.out_dir, &args.dump_name, &args.version.value, &args.job_name)?.boxed(),
    };

    let pages = match args.count {
        None => pages,
        Some(count) => pages.take(count).boxed(),
    };

    let mut store = store::Options::from_common_args(&args.common).build_store()?;
    if args.clear {
        store.clear()?;
    }

    store.import(pages)?;

    Ok(())
}
