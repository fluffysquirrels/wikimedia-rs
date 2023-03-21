use anyhow::Context;
use crate::{
    args::{CommonArgs, DumpNameArg, FileNameRegexArg, JobNameArg, VersionSpecArg},
    dump::{
        self,
        download::DownloadJobFileResultKind
    },
    http,
    Result,
    TempDir,
    util::fmt::{Bytes, TransferStats},
};
use std::time::Instant;
use valuable::Valuable;

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
    let start_time = Instant::now();

    let dump_name = &*args.dump_name.value;
    let job_name = &*args.job_name.value;

    let metadata_client = http::metadata_client(&args.common)?;

    let (ver, files) = dump::download::get_file_infos(
        &metadata_client,
        &args.dump_name,
        &args.version_spec.value,
        &args.job_name,
        args.file_name_regex.value.as_ref()).await?;

    let temp_dir = TempDir::create(&*args.common.out_dir, args.keep_temp_dir)?;
    let download_client = http::download_client(&args.common)?;

    let mut download_ok: u64 = 0;
    let mut download_len: u64 = 0;
    let mut existing_ok: u64 = 0;
    let mut existing_len: u64 = 0;

    // let progress = indicatif::ProgressBar::new(files.len().try_into()
    //                                                 .expect("convert usize to u64"));
    // progress.set_style(indicatif::ProgressStyle::with_template(
    //     "{bar:40} {pos}/{len} files {elapsed_precise}, eta {eta_precise} {msg}")?);
    // progress.enable_steady_tick(Duration::from_millis(100));

    for (_file_name, file_meta) in files.iter() {
        let res =
            dump::download::download_job_file(&download_client, &args.dump_name, &ver,
                                              &args.job_name, &*args.mirror_url, file_meta,
                                              &*args.common.out_dir, &temp_dir).await
                .with_context(|| format!(
                    "while downloading job file \
                     dump='{dump_name}' \
                     version='{ver}' \
                     job='{job_name}' \
                     file='{file_rel_url:?}'",
                    ver = ver.0,
                    file_rel_url = &file_meta.url))?;
        match res.kind {
            DownloadJobFileResultKind::DownloadOk => {
                download_ok += 1;
                download_len += res.stats.len.0;
            },
            DownloadJobFileResultKind::ExistingOk => {
                existing_ok += 1;
                existing_len += res.stats.len.0;
            },
        };
        // progress.inc(1);
    }

    // progress.abandon_with_message("All done!");

    drop(temp_dir);

    let duration = start_time.elapsed();

    tracing::info!(download_ok,
                   download_stats = TransferStats::new(Bytes(download_len), duration).as_value(),
                   existing_ok,
                   existing_stats = TransferStats::new(Bytes(existing_len), duration).as_value(),
                   ?duration,
                   "download command complete");

    Ok(())
}
