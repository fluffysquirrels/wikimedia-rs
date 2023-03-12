use crate::{
    args::{CommonArgs, DumpNameArg, JsonOutputArg},
    http,
    operations,
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

    let mut vers = operations::get_dump_versions(&client, &args.dump_name).await?;
    vers.sort();
    // Rebind as immutable
    let vers = vers;

    if args.json.value {
        for ver in vers {
            println!(r#""{}""#, ver.0);
        }
    } else {
        for ver in vers {
            println!("{}", ver.0);
        }
    }

    Ok(())
}
