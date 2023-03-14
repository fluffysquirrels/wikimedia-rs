use crate::{
    args::CommonArgs,
    article_dump,
    Result,
};
use std::{
    io::stdout,
    path::PathBuf,
};

/// Get pages from an article dump file.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[arg(long)]
    article_dump_file: PathBuf,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let pages = article_dump::open_article_dump_file(&*args.article_dump_file)?;

    for page in pages {
        serde_json::to_writer_pretty(&stdout(), &page?)?;
        println!();
    }

    Ok(())
}
