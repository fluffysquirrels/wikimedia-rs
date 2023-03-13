use anyhow::Context;
use crate::{
    args::{CommonArgs, DumpNameArg, FileNameRegexArg, JobNameArg, VersionSpecArg},
    http,
    operations::{self, DownloadJobFileResult},
    Result,
    TempDir,
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
    ///   * https://dumps.wikimedia.org
    ///   * https://ftp.acc.umu.se/mirror/wikimedia.org/dumps
    ///
    /// Note that only job files are downloaded from this mirror, metadata files are downloaded from https://dumps.wikimedia.org to ensure we get the freshest data.
    ///
    /// To find a mirror, see https://meta.wikimedia.org/wiki/Mirroring_Wikimedia_project_XML_dumps#Current_mirrors
    #[arg(long, env = "WMD_MIRROR_URL")]
    mirror_url: String,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let dump_name = &*args.dump_name.value;
    let job_name = &*args.job_name.value;

    let metadata_client = http::metadata_client(&args.common)?;

    let (ver, files) = operations::get_file_infos(
        &metadata_client,
        &args.dump_name,
        &args.version_spec.value,
        &args.job_name,
        args.file_name_regex.value.as_ref()).await?;

    let temp_dir = TempDir::create(&*args.common.out_dir, args.keep_temp_dir)?;
    let download_client = http::download_client(&args.common)?;

    let mut download_ok: u64 = 0;
    let mut existing_ok: u64 = 0;

    for (_file_name, file_meta) in files.iter() {
        let res =
            operations::download_job_file(&download_client, &args.dump_name, &ver, &args.job_name,
                                          &*args.mirror_url, file_meta, &*args.common.out_dir,
                                          &temp_dir).await
                .with_context(|| format!(
                    "while downloading job file \
                     dump='{dump_name}' \
                     version='{ver}' \
                     job='{job_name}' \
                     file='{file_rel_url}'",
                    ver = ver.0,
                    file_rel_url = &*file_meta.url))?;
        match res {
            DownloadJobFileResult::DownloadOk => { download_ok += 1 },
            DownloadJobFileResult::ExistingOk => { existing_ok += 1 },
        }
    }

    drop(temp_dir);

    tracing::info!(download_ok,
                   existing_ok,
                   "download command complete");

    Ok(())
}
