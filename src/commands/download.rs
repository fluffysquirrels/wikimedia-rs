use anyhow::Context;
use crate::{
    args::{CommonArgs, DumpNameArg, JobNameArg},
    http,
    operations,
    Result,
};
use std::path::PathBuf;
use tracing::Level;

/// Download latest dump job files
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    #[clap(flatten)]
    dump_name: DumpNameArg,

    #[clap(flatten)]
    job_name: JobNameArg,

    /// The directory to download to.
    ///
    /// The dump files will be placed in a child directory of this.
    /// With `--out-dir` set to `./out`, dump file paths will be like:
    /// `./out/enwiki/20230301/metacurrentdumprecombine/enwiki-20230301-pages-articles.xml.bz2`
    ///
    /// If not present tries to read the environment variable `WMD_OUT_DIR`.
    #[arg(long, env = "WMD_OUT_DIR")]
    out_dir: PathBuf,

    /// Overwrite existing downloaded files. By default this will fail with an error.
    #[arg(long, default_value_t = false)]
    overwrite: bool,

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

    let client = http::client()?;

    let mut vers = operations::get_dump_versions(&client, &args.dump_name).await?;
    if vers.is_empty() {
        return Err(anyhow::Error::msg(format!("No versions found for dump {dump_name}")));
    }
    vers.sort();
    // Re-bind as immutable.
    let vers = vers;

    let ver = vers.last().expect("vers not empty");

    let ver_status = operations::get_dump_version_status(&client, &args.dump_name, &ver).await?;

    let Some(job_status) = ver_status.jobs.get(job_name) else {
        return Err(anyhow::Error::msg(format!("No status found for job job_name={job_name} version={ver} dump_name={dump_name}",
                                              ver = ver.0)));
    };

    if tracing::enabled!(Level::TRACE) {
        tracing::trace!(job_status = format!("{:#?}", job_status), "Job status");
    }

    if job_status.status != "done" {
        return Err(anyhow::Error::msg(format!("Job status is not 'done' status={status} job={job_name} version={ver} dump={dump_name}",
                                              status = job_status.status,
                                              ver = ver.0)));
    }

    for file_meta in job_status.files.values() {
        operations::download_job_file(&client, &args.dump_name, ver, &args.job_name,
                                      &*args.mirror_url, file_meta, &*args.out_dir,
                                      args.overwrite).await
            .with_context(|| format!("while downloading job file dump={dump_name} version={ver} job={job_name} file={file_rel_url}",
                                     ver = ver.0,
                                     file_rel_url = &*file_meta.url))?;
    }

    Ok(())
}
