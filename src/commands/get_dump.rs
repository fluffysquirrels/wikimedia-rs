use crate::{
    args::{CommonArgs, JsonOutputArg},
    dump,
    http,
    Result,
};

/// Get data about what dumps are available.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(flatten)]
    json: JsonOutputArg,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let client = http::metadata_client(&args.common)?;

    let mut dumps = dump::download::get_dumps(&client).await?;
    dumps.sort();
    // Rebind as immutable
    let dumps = dumps;

    if args.json.value {
        for dump in dumps {
            println!(r#""{}""#, dump.0);
        }
    } else {
        for dump in dumps {
            println!("{}", dump.0);
        }
    }

    Ok(())
}
