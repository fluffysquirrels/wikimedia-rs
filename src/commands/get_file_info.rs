use crate::{
    args::{CommonArgs, DumpNameArg, FileNameRegexArg, JobNameArg, JsonOutputArg, VersionSpecArg},
    http,
    operations,
    Result,
    types::FileInfoOutput,
};

/// Get metadata about files available for download from a job.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(flatten)]
    dump_name: DumpNameArg,

    #[clap(flatten)]
    version_spec: VersionSpecArg,

    #[clap(flatten)]
    job_name: JobNameArg,

    #[clap(flatten)]
    file_name_regex: FileNameRegexArg,

    #[clap(flatten)]
    json: JsonOutputArg,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let client = http::client()?;

    let (_ver, files) = operations::get_file_infos(
        &client,
        &args.dump_name,
        &args.version_spec.value,
        &args.job_name,
        args.file_name_regex.value.as_ref()).await?;

    if args.json.value {
        for (file_name, file_meta) in files.iter() {
            let file = FileInfoOutput {
                name: file_name.clone(),
                metadata: file_meta.clone(),
            };
            serde_json::to_writer_pretty(&std::io::stdout(), &file)?;
            println!();
        }
    } else {
        // json == false, so print file names only
        for (file_name, _file_meta) in files.iter() {
            println!("{}", file_name);
        }
    }

    Ok(())
}
