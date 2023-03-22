use crate::{
    args::{CommonArgs, DumpNameArg, JsonOutputArg},
    dump,
    http,
    Result,
};

/// Get data about what versions are available for a dump.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(flatten)]
    dump_name: DumpNameArg,

    #[clap(flatten)]
    json: JsonOutputArg,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let client = http::metadata_client(&args.common)?;

    let versions = dump::download::get_dump_versions(&client, &args.dump_name.value).await?;

    if args.json.value {
        for version in versions {
            println!(r#""{}""#, version.0);
        }
    } else {
        for version in versions {
            println!("{}", version.0);
        }
    }

    Ok(())
}
