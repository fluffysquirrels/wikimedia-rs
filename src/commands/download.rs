use crate::{
    args::{CommonArgs, DumpNameArg, FileNameRegexArg, JobNameArg, VersionSpecArg},
    dump::{
        self,
    },
    Result,
};

/// Download latest dump job files
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

    /// Keep the temporary directory where files are initially downloaded. By default this is deleted after use.
    #[arg(long, default_value_t = false)]
    keep_temp_dir: bool,

    /// Specify the URL of a mirror to download job files from. Only supports http: and https: URLs.
    ///
    /// If not present tries to read the environment variable `WMD_MIRROR_URL`.
    ///
    /// Examples:
    ///   * <https://dumps.wikimedia.org>
    ///   * <https://ftp.acc.umu.se/mirror/wikimedia.org/dumps>
    ///
    /// Note that only job files are downloaded from this mirror, metadata files are downloaded from <https://dumps.wikimedia.org> to ensure we get the freshest data.
    ///
    /// To find a mirror, see <https://meta.wikimedia.org/wiki/Mirroring_Wikimedia_project_XML_dumps#Current_mirrors>
    #[arg(long, env = "WMD_MIRROR_URL")]
    mirror_url: String,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let dump_name = &args.dump_name.value;
    let version_spec = &args.version_spec.value;
    let job_name = &args.job_name.value;

    let _ = dump::download::download_job(
        dump_name,
        version_spec,
        job_name,
        args.file_name_regex.value.as_ref(),
        args.keep_temp_dir,
        &args.common,
        &*args.mirror_url,
    ).await?;

    Ok(())
}
