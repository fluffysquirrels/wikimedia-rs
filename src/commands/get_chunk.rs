use anyhow::format_err;
use crate::{
    args::CommonArgs,
    store,
    Result,
};

/// Get information about a page store chunk.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[arg(long)]
    chunk_id: Option<store::ChunkId>,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let store = store::Options::from_common_args(&args.common).build()?;

    let chunk_ids: Vec<store::ChunkId> =
        match args.chunk_id {
            Some(chunk_id) => vec![chunk_id],
            None => store.chunk_id_vec()?,
        };

    for chunk_id in chunk_ids.into_iter() {
        let chunk_meta = store.get_chunk_meta_by_chunk_id(chunk_id)?
                              .ok_or_else(|| format_err!("ChunkMeta not found by ChunkId"))?;

        serde_json::to_writer_pretty(&std::io::stdout(), &chunk_meta)?;
        println!();
    }

    Ok(())
}
