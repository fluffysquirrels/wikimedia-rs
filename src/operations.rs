//! Functions that are re-used between commands

use anyhow::Context;
use crate::{
    args::{DumpNameArg, JobNameArg},
    Result,
    types::{DumpVersionStatus, FileMetadata, Version, VersionSpec},
};
use regex::Regex;
use sha1::{Sha1, Digest};
use std::{
    path::Path,
    time::Duration,
};
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;
use tracing::Level;

#[tracing::instrument(level = "trace", skip(client))]
pub async fn get_dump_versions(
    client: &reqwest::Client,
    dump_name: &DumpNameArg
) -> Result<Vec<Version>> {
    let url = format!("https://dumps.wikimedia.org/{dump_name}/", dump_name = dump_name.value);
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

    let mut vers = Vec::<Version>::new();

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

        let ver_string = cap.name("date").expect("regex capture name").as_str().to_string();
        vers.push(Version(ver_string));
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
pub async fn get_dump_version_status(
    client: &reqwest::Client,
    dump_name: &DumpNameArg,
    version_spec: &VersionSpec,
) -> Result<(Version, DumpVersionStatus)> {

    let ver = match version_spec {
        VersionSpec::Version(ver) => ver.clone(),
        VersionSpec::Latest => {
            let mut vers = get_dump_versions(&client, dump_name).await?;
            if vers.is_empty() {
                return Err(anyhow::Error::msg(format!("No versions found for dump {dump_name}",
                                                      dump_name = dump_name.value)));
            }
            vers.sort();
            // Re-bind as immutable.
            let vers = vers;

            let ver = vers.last().expect("vers not empty");
            ver.clone()
        },
    };

    let url = format!("https://dumps.wikimedia.org/{dump_name}/{ver}/dumpstatus.json",
                      dump_name = dump_name.value,
                      ver = ver.0);
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

    Ok((ver.clone(), status))
}

#[tracing::instrument(level = "trace", skip(client))]
pub async fn download_job_file(
    client: &reqwest::Client,
    dump_name: &DumpNameArg,
    ver: &Version,
    job_name: &JobNameArg,
    mirror_url: &str,
    file_meta: &FileMetadata,
    out_dir: &Path,
    overwrite: bool,
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
                mirror_url = mirror_url,
                file_rel_url = file_meta.url);

    let file_name = file_meta.url.split('/').last()
        .expect("already verified segments is not empty");
    let file_out_path = out_dir.join(format!("{dump_name}/{ver}/{job_name}/{file_name}",
                                             dump_name = &*dump_name.value,
                                             ver = ver.0,
                                             job_name = &*job_name.value));
    let file_out_dir_path = file_out_path.parent().expect("file_out_path.parent() not None");
    std::fs::create_dir_all(&*file_out_dir_path)?;

    tracing::info!(
        url,
        out_path = %file_out_path.display(),
        expected_len = file_meta.size,
        "download_job_file starting");

    let mut file_open_options = tokio::fs::OpenOptions::new();
    file_open_options.write(true);
    if overwrite {
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
