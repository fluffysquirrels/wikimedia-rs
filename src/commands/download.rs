use anyhow::Context;
use crate::{
    args::CommonArgs,
    Result,
};
use regex::Regex;
use serde::Deserialize;
use sha1::{Sha1, Digest};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    time::Duration,
};
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;
use tracing::Level;

/// Download latest dump job files from Wikimedia
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

    /// The name of the dump to download, e.g. `enwiki`.
    ///
    /// If not present tries to read the environment variable `WMD_DUMP`,
    /// finally uses `enwiki` as a default.
    #[arg(long = "dump", default_value = "enwiki", env = "WMD_DUMP")]
    dump_name: String,

    /// The name of the job to download, e.g. `metacurrentdumprecombine`.
    ///
    /// If not present tries to read the environment variable `WMD_JOB`,
    /// finally uses `metacurrentdumprecombine` as a default.
    #[arg(long = "job", default_value = "metacurrentdumprecombine", env = "WMD_JOB")]
    job_name: String,

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
}

#[derive(Debug, Deserialize)]
struct DumpVersionStatus {
    jobs: BTreeMap<String, JobStatus>,

    #[allow(dead_code)] // Not used currently
    version: String,
}

#[derive(Debug, Deserialize)]
struct JobStatus {
    status: String,

    #[allow(dead_code)] // Not used currently
    updated: String,

    #[serde(default)]
    files: BTreeMap<String, FileMetadata>,
}

#[derive(Debug, Deserialize)]
struct FileMetadata {
    size: u64,
    url: String,
    sha1: Option<String>,

    #[allow(dead_code)] // Not used currently
    md5: Option<String>,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let dump_name = &*args.dump_name;
    let job_name = &*args.job_name;

    let client = reqwest::ClientBuilder::new()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
            ))
        // .gzip(true)
        // .timeout(...)
        .build()?;

    let mut vers = get_dump_versions(&client, &*args.dump_name).await?;
    if vers.is_empty() {
        return Err(anyhow::Error::msg(format!("No versions found for dump {dump_name}")));
    }
    vers.sort();
    // Re-bind as immutable.
    let vers = vers;

    let ver = vers.last().expect("vers not empty");

    let ver_status = get_dump_version_status(&client, &*args.dump_name, ver).await?;

    let Some(job_status) = ver_status.jobs.get(&*args.job_name) else {
        return Err(anyhow::Error::msg(format!("No status found for job job_name={job_name} version={ver} dump_name={dump_name}")));
    };

    if tracing::enabled!(Level::TRACE) {
        tracing::trace!(job_status = format!("{:#?}", job_status), "Job status");
    }

    if job_status.status != "done" {
        return Err(anyhow::Error::msg(format!("Job status is not 'done' status={status} job={job_name} version={ver} dump={dump_name}", status = job_status.status)));
    }

    for file_meta in job_status.files.values() {
        download_job_file(&client, &args, ver, file_meta).await
            .with_context(|| format!("while downloading job file dump={dump_name} version={ver} job={job_name} file={file_rel_url}",
                                     file_rel_url = &*file_meta.url))?;
    }

    Ok(())
}

#[tracing::instrument(level = "trace", skip(client))]
async fn get_dump_versions(client: &reqwest::Client, dump_name: &str) -> Result<Vec<String>> {
    let url = format!("https://dumps.wikimedia.org/{dump_name}/");
    let res = client.get(url.clone())
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await?;
    let res_code = res.status();
    let res_code_int = res_code.as_u16();
    let res_code_str = res_code.canonical_reason().unwrap_or("");
    tracing::info!(url = url.clone(),
                   response_code = res_code_int,
                   response_code_str = res_code_str,
                   "GET dump versions");

    let res_text = res.text().await?;
    if tracing::enabled!(Level::TRACE) {
        tracing::trace!(body_text = res_text.clone(),
                       "GET dump versions body");
    }

    if !res_code.is_success() {
        return Err(anyhow::Error::msg(
            format!("HTTP response error fetching dump versions url={url} response_code={res_code_int} response_code_str={res_code_str}")));
    }

    let doc = scraper::Html::parse_document(&*res_text);
    if !doc.errors.is_empty() {
        tracing::warn!(errors = ?doc.errors,
                       "dump versions body had HTML parse errors");
    }

    let mut vers = Vec::<String>::new();

    // TODO: Use lazy_static!
    let date_href_re = Regex::new(r"^(?P<date>\d{8})/$").expect("parse regex");

    for link in doc.select(&scraper::Selector::parse("a").expect("parse selector")) {
        let href = link.value().attr("href");
        tracing::trace!(href = href, "dump versions link");

        let Some(href) = href else {
            continue;
        };

        let Some(cap) = date_href_re.captures(href) else {
            continue;
        };

        vers.push(cap.name("date").expect("regex capture name").as_str().to_string());
    }

    tracing::debug!(versions_count = vers.len(),
                    "dump versions ret count");

    if tracing::enabled!(Level::TRACE) {
        tracing::trace!(versions = ?vers,
                       "dump versions ret data");
    }

    Ok(vers)
}

#[tracing::instrument(level = "trace", skip(client), ret)]
async fn get_dump_version_status(
    client: &reqwest::Client,
    dump_name: &str,
    ver: &str
) -> Result<DumpVersionStatus> {

    let url = format!("https://dumps.wikimedia.org/{dump_name}/{ver}/dumpstatus.json");
    let res = client.get(url.clone())
                    .timeout(Duration::from_secs(10))
                    .send()
                    .await?;
    let res_code = res.status();
    let res_code_int = res_code.as_u16();
    let res_code_str = res_code.canonical_reason().unwrap_or("");
    tracing::info!(url = url.clone(),
                   response_code = res_code_int,
                   response_code_str = res_code_str,
                   "GET dump version status");

    let res_text = res.text().await?;
    if tracing::enabled!(Level::TRACE) {
        tracing::trace!(body_text = res_text.clone(),
                       "GET dump version status body");
    }

    if !res_code.is_success() {
        return Err(anyhow::Error::msg(
            format!("HTTP response error fetching dump version status url={url} response_code={res_code_int} response_code_str={res_code_str}")));
    }

    let status: DumpVersionStatus = serde_json::from_str(&*res_text)
        .with_context(|| format!("Getting dump version status url={url}"))?;

    Ok(status)
}

#[tracing::instrument(level = "trace", skip(client))]
async fn download_job_file(
    client: &reqwest::Client,
    args: &Args,
    ver: &str,
    file_meta: &FileMetadata
) -> Result<()> {
    let mut rel_segments = file_meta.url.split('/');
    let Some(first) = rel_segments.next() else {
        tracing::warn!(file_url = file_meta.url,
                       "Bad file meta URL, no segments");
        return Ok(());
    };

    if first.len() > 0 {
        tracing::warn!(file_url = file_meta.url,
                       "Bad file meta URL, missing initial '/'");
        return Ok(());
    }

    // TODO: Use lazy_static!
    let segment_re = Regex::new(r"^[-a-z_0-9A-Z.]+$").expect("parse regex");

    for segment in rel_segments {
        if !segment_re.is_match(segment) {
            tracing::warn!(file_meta.url,
                           file_segment = segment,
                           "Bad file meta URL, segment didn't match regex");
            return Ok(());
        }

        if segment == "." || segment == ".." {
            tracing::warn!(file_meta.url,
                           file_segment = segment,
                           "Bad file meta URL, segment was '.' or '..'");
            return Ok(());
        }
    }

    let url =
        format!("{mirror_url}{file_rel_url}",
                mirror_url = "https://ftp.acc.umu.se/mirror/wikimedia.org/dumps",
                file_rel_url = file_meta.url);

    let file_name = file_meta.url.split('/').last()
        .expect("already verified segments is not empty");
    let file_out_path = args.out_dir.join(format!("{dump_name}/{ver}/{job_name}/{file_name}",
                                                  dump_name = &*args.dump_name,
                                                  job_name = &*args.job_name));
    let file_out_dir_path = file_out_path.parent().expect("file_out_path.parent() not None");
    std::fs::create_dir_all(&*file_out_dir_path)?;

    tracing::info!(
        url,
        out_path = %file_out_path.display(),
        expected_len = file_meta.size,
        "download_job_file starting");

    let mut file_open_options = tokio::fs::OpenOptions::new();
    file_open_options.write(true);
    if args.overwrite {
        file_open_options.create(true)
                         .truncate(true);
    } else {
        file_open_options.create_new(true);
    }
    let mut file = file_open_options.open(file_out_path)
                                    .await?;

    let download_res = client.get(url.clone())
        .send()
        .await?;
    let download_res_code = download_res.status();
    let download_res_code_int = download_res_code.as_u16();
    let download_res_code_str = download_res_code.canonical_reason().unwrap_or("");
    tracing::debug!(url = url.clone(),
                   response_code = download_res_code_int,
                   response_code_str = download_res_code_str,
                   "download job file status");

    if !download_res_code.is_success() {
        return Err(anyhow::Error::msg(
            format!("HTTP response error downloading job file url={url} response_code={download_res_code_int} response_code_str={download_res_code_str}")));
    }

    let mut bytes_stream = download_res.bytes_stream();
    let mut sha1_hasher = Sha1::new();

    while let Some(chunk) = bytes_stream.next().await {
        let chunk = chunk
            .with_context(|| format!("while downloading a job file url={url}"))?;
        tracing::trace!(chunk_len = chunk.len(), "download_job_file chunk");
        sha1_hasher.update(&chunk);
        tokio::io::copy(&mut chunk.as_ref(), &mut file).await?;
    }

    file.flush().await?;
    file.sync_all().await?;

    let finishing_len = file.metadata().await?.len();

    drop(file);

    tracing::debug!("download_job_file download complete");

    if finishing_len != file_meta.size {
        return Err(anyhow::Error::msg(format!("Download job file was the wrong size url={url} expected_len={expected} finishing_len={finishing_len}", expected = file_meta.size)))
    }

    let sha1_hash = sha1_hasher.finalize();
    let sha1_hash_hex = hex::encode(sha1_hash);

    match file_meta.sha1.as_ref() {
        None => tracing::warn!(url, "No SHA1 hash given for job file"),
        Some(expected_sha1) => {
            let expected_sha1 = expected_sha1.to_lowercase();
            if sha1_hash_hex != expected_sha1 {
                return Err(anyhow::Error::msg(
                    format!("Bad SHA1 hash for downloaded job file url={url} expected_sha1={expected_sha1}, computed_sha1={computed_sha1}",
                            computed_sha1 = sha1_hash_hex)));
            }

            tracing::debug!(sha1 = expected_sha1,
                            "Downloaded file SHA1 hash matched the expected value");
        }
    }

    Ok(())
}
