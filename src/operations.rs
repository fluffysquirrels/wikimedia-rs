//! Functions that are re-used between commands

use anyhow::Context;
use crate::{
    args::{DumpNameArg, JobNameArg},
    http,
    Result,
    TempDir,
    types::{Dump, DumpVersionStatus, FileMetadata, JobStatus, Version, VersionSpec},
    UserRegex,
};
use sha1::{Sha1, Digest};
use std::{
    path::Path,
};
use tokio_stream::StreamExt;
use tracing::Level;

#[derive(Clone, Debug)]
pub enum ExistingFileStatus {
    NoFile,
    DeletedBecauseIncorrectSize,
    DeletedBecauseIncorrectSha1Hash,
    NoSha1HashToCheck,
    FileOk,
}

const DUMPS_WIKIMEDIA_SERVER: &'static str = "https://dumps.wikimedia.org";

#[tracing::instrument(level = "trace", skip(client))]
pub async fn get_dumps(
    client: &http::Client
) -> Result<Vec<Dump>> {
    let url = format!("{DUMPS_WIKIMEDIA_SERVER}/backup-index-bydb.html");

    let req = client.get(url)
                    .build()?;

    let fetch_res = http::fetch_text(&client, req).await?;

    let doc = scraper::Html::parse_document(&*fetch_res.response_body);
    if !doc.errors.is_empty() {
        tracing::warn!(errors = ?doc.errors,
                       "get_dumps index had HTML parse errors");
    }

    let mut dumps = Vec::<Dump>::new();

    for link in doc.select(&scraper::Selector::parse("a").expect("parse selector")) {
        let href = link.value().attr("href");
        tracing::trace!(href, "dumps index link");

        let Some(href) = href else {
            continue;
        };

        let Some(cap) = lazy_regex!(r"^(?P<dump>[-_a-zA-Z0-9]+)/(?P<date>\d{8})$")
                                   .captures(href) else {
            continue;
        };

        let dump_string = cap.name("dump").expect("regex capture name").as_str().to_string();
        dumps.push(Dump(dump_string));
    }

    tracing::debug!(dumps_count = dumps.len(),
                    "dumps ret count");

    if tracing::enabled!(Level::TRACE) {
        tracing::trace!(dumps = ?dumps,
                       "dumps ret data");
    }

    Ok(dumps)
}

#[tracing::instrument(level = "trace", skip(client))]
pub async fn get_dump_versions(
    client: &http::Client,
    dump_name: &DumpNameArg
) -> Result<Vec<Version>> {
    let url = format!("{DUMPS_WIKIMEDIA_SERVER}/{dump_name}/", dump_name = dump_name.value);
    let req = client.get(url.clone())
                    .build()?;

    let fetch_res = http::fetch_text(&client, req).await?;

    let doc = scraper::Html::parse_document(&*fetch_res.response_body);
    if !doc.errors.is_empty() {
        tracing::warn!(errors = ?doc.errors,
                       "dump versions body had HTML parse errors");
    }

    let mut vers = Vec::<Version>::new();

    for link in doc.select(&scraper::Selector::parse("a").expect("parse selector")) {
        let href = link.value().attr("href");
        tracing::trace!(href, "dump versions link");

        let Some(href) = href else {
            continue;
        };

        let Some(cap) = lazy_regex!(r"^(?P<date>\d{8})/$").captures(href) else {
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
    client: &http::Client,
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

    let url = format!("{DUMPS_WIKIMEDIA_SERVER}/{dump_name}/{ver}/dumpstatus.json",
                      dump_name = dump_name.value,
                      ver = ver.0);
    let req = client.get(url.clone())
                    .build()?;

    let fetch_res = http::fetch_text(&client, req).await?;

    let status: DumpVersionStatus = serde_json::from_str(&*fetch_res.response_body)
        .with_context(|| format!("Getting dump version status url={url}"))?;

    Ok((ver.clone(), status))
}

#[tracing::instrument(level = "trace", skip(client))]
pub async fn get_job_status(
    client: &http::Client,
    dump_name: &DumpNameArg,
    version_spec: &VersionSpec,
    job_name: &JobNameArg,
) -> Result<(Version, JobStatus)> {
    let (ver, ver_status) = get_dump_version_status(&client, &dump_name, version_spec).await?;

    let Some(job_status) = ver_status.jobs.get(&job_name.value) else {
        return Err(anyhow::Error::msg(format!("No status found for job dump_name={dump_name} version={ver} job_name={job_name}",
                                              dump_name = dump_name.value,
                                              ver = ver.0,
                                              job_name = job_name.value)));
    };

    if tracing::enabled!(Level::TRACE) {
        tracing::trace!(job_status = format!("{:#?}", job_status), "Job status");
    }

    if job_status.status != "done" {
        return Err(anyhow::Error::msg(format!("Job status is not 'done' status={status} dump={dump_name} version={ver} job={job_name}",
                                              status = job_status.status,
                                              dump_name = dump_name.value,
                                              ver = ver.0,
                                              job_name = job_name.value)));
    }

    Ok((ver, job_status.clone()))
}

#[tracing::instrument(level = "trace", skip(client), ret)]
pub async fn get_file_infos(
    client: &http::Client,
    dump_name: &DumpNameArg,
    version_spec: &VersionSpec,
    job_name: &JobNameArg,
    file_name_regex: Option<&UserRegex>,
) -> Result<(Version, Vec<(String, FileMetadata)>)> {
    let (ver, job_status) = get_job_status(&client, dump_name,
                                           version_spec, job_name).await?;

    let files: Vec<(String, FileMetadata)> = match file_name_regex {
        None => job_status.files.into_iter().collect(),
        Some(re) => job_status.files.into_iter().filter(|kv| re.0.is_match(&*kv.0)).collect(),
    };

    Ok((ver, files))
}

#[tracing::instrument(level = "trace", skip(client))]
pub async fn download_job_file(
    client: &http::Client,
    dump_name: &DumpNameArg,
    ver: &Version,
    job_name: &JobNameArg,
    mirror_url: &str,
    file_meta: &FileMetadata,
    out_dir: &Path,
    temp_dir: &TempDir,
) -> Result<()> {
    validate_file_relative_url(&*file_meta.url)?;

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

    match check_existing_file(&*file_out_path, file_meta, &*url).await? {
        ExistingFileStatus::FileOk | ExistingFileStatus::NoSha1HashToCheck => return Ok(()),
        _ => (),
    };

    let file_out_dir_path = file_out_path.parent().expect("file_out_path.parent() not None");

    let temp_file_path = temp_dir.path()?.join(&*file_name);

    std::fs::create_dir_all(&*file_out_dir_path)?;

    tracing::info!(
        url,
        out_path = %file_out_path.display(),
        expected_len = file_meta.size,
        "download_job_file starting download");

    let download_request = client.get(url.clone())
                                 .build()?;
    let download_result = http::download_file(&client, download_request, &*temp_file_path).await?;

    if download_result.len != file_meta.size {
        return Err(anyhow::Error::msg(format!(
            "Download job file was the wrong size \
             url='{url}' \
             expected_len={expected_len} \
             file_len={file_len}",
            expected_len = file_meta.size,
            file_len = download_result.len)));
    }

    match file_meta.sha1.as_ref() {
        None => tracing::warn!(url, "No expected SHA1 hash given for job file"),
        Some(expected_sha1) => {
            let expected_sha1 = expected_sha1.to_lowercase();
            if download_result.sha1 != expected_sha1 {
                return Err(anyhow::Error::msg(
                    format!("Bad SHA1 hash for downloaded job file url='{url}' \
                             expected_sha1={expected_sha1}, computed_sha1={computed_sha1}",
                            computed_sha1 = download_result.sha1)));
            }

            tracing::debug!(sha1 = expected_sha1,
                            "Downloaded file OK: SHA1 hash matched the expected value");
        }
    }

    tokio::fs::rename(&*temp_file_path, &*file_out_path)
        .await
        .with_context(|| format!("While moving a downloaded file from its temporary download \
                                  directory to its target directory \
                                  temp_path='{temp_file_path}' \
                                  target_path='{file_out_path}'",
                                 temp_file_path = temp_file_path.display(),
                                 file_out_path = file_out_path.display()))?;

    tracing::debug!(temp_file_path = %temp_file_path.display(),
                    file_out_path = %file_out_path.display(),
                    "Moved downloaded file from temp directory to output directory");

    tracing::info!(duration = ?download_result.duration,
                   download_rate = %download_result.download_rate(),
                   url,
                   out_path = %file_out_path.display(),
                   len = file_meta.size,
                   "download_job_file download complete, file OK");

    Ok(())
}

fn validate_file_relative_url(url: &str) -> Result<()> {
    // Wrap everyting in a closure to add context with anyhow.
    (|| -> Result<()> {
        if url == "" {
            return Err(anyhow::Error::msg("URL was the empty string"));
        }

        let mut rel_segments = url.split('/');
        let first = rel_segments.next().expect("split always returns at least one segment");

        if first.len() > 0 {
            return Err(anyhow::Error::msg("Path missing initial '/'"));
        }

        for segment in rel_segments {
            // Wrap segment validation in a closure to add context with anyhow.
            (|| -> Result<()> {
                if !lazy_regex!(r"^[-a-z_0-9A-Z.]+$").is_match(segment) {
                    return Err(anyhow::Error::msg("Path segment didn't match regex"));
                }

                if segment == "." || segment == ".." {
                    return Err(anyhow::Error::msg("Path segment was '.' or '..'"));
                }

                Ok(())
            })().with_context(|| format!("Bad path segment \
                                        segment='{segment}'"))?;
        }

        Ok(())
    })().with_context(|| format!("Bad file metadata relative URL \
                                file_url='{url}'"))
}

#[tracing::instrument(level = "trace", ret)]
async fn check_existing_file(
    path: &Path,
    file_meta: &FileMetadata,
    url: &str,
) -> Result<ExistingFileStatus> {
    // Wrapped in a closure to add context on errors.
    (async || -> Result<ExistingFileStatus> {

        // Look for an existing file at the output path.
        let existing_meta = match path.metadata() {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File not found error, go ahead and download to the output path.
                return Ok(ExistingFileStatus::NoFile);
            },

            // Report other errors.
            Err(e) => {
                return Err(anyhow::Error::new(e).context(format!(
                    "while checking for an existing file at the output path \
                     file_out_path='{path}' \
                     file_url='{url}'",
                    path = path.display())));
            }

            Ok(meta) => meta,
        };

        // Check existing file's metdata
        if !existing_meta.is_file() {
            return Err(anyhow::Error::msg(format!(
                 "Found an item that's not a file. \
                  metadata.file_type()={file_type:?}",
                file_type = existing_meta.file_type())));
        }

        // Check existing file length
        let expected_len = file_meta.size;
        let existing_len = existing_meta.len();
        if expected_len != existing_len {
            // Existing file length does not match expected.
            tracing::warn!(path = %path.display(),
                           existing_len,
                           expected_len,
                           url,
                           "Deleting existing file that was the wrong size");

            std::fs::remove_file(&*path)
                .with_context(
                    || format!("while deleting existing file that was the wrong size \
                                existing_len={existing_len} \
                                expected_len={expected_len}"))?;

            return Ok(ExistingFileStatus::DeletedBecauseIncorrectSize);
        }

        // Check existing file SHA1 hash
        let expected_sha1 = match file_meta.sha1.as_ref() {
            // No SHA1 hash in metadata, warn and return OK assuming the download
            // succeeded.
            None => {
                tracing::warn!(file_path = %path.display(),
                               ?file_meta,
                               url,
                               "Existing file is the right size, but there's no SHA1 \
                                hash to check in the dump status file metadata.");
                return Ok(ExistingFileStatus::NoSha1HashToCheck);
            },

            Some(sha1) => sha1,
        };

        // SHA1 hash in metadata, check it matches the existing file's hash.
        let expected_sha1 = expected_sha1.to_lowercase();

        let existing_sha1 = calculate_file_sha1(&*path).await?;

        if expected_sha1 == existing_sha1 {
            // Existing file's SHA1 hash was correct, return Ok.
            tracing::debug!(file_len = expected_len,
                            file_path = %path.display(),
                            sha1 = expected_sha1,
                            url,
                            "Existing file OK: SHA1 hash and file size are \
                             correct.");
            return Ok(ExistingFileStatus::FileOk);
        } else {
            // Existing file's SHA1 hash was incorrect, delete it.
            tracing::warn!(file_len = expected_len,
                           file_path = %path.display(),
                           existing_sha1,
                           expected_sha1,
                           url,
                           "Existing file bad: file size correct but SHA1 hash \
                            was wrong. Deleting existing file.");
            std::fs::remove_file(&*path)
                .with_context(
                    || format!("while deleting existing file that had the correct size \
                                but wrong SHA1 hash \
                                existing_sha1={existing_sha1} \
                                expected_sha1={expected_sha1}"))?;
            return Ok(ExistingFileStatus::DeletedBecauseIncorrectSha1Hash);
        }

        // Not reached.
    })().await.with_context(|| format!(
        "Checking existing file at target path \
         path='{path}' \
         file_metadata={file_meta:?} \
         download_url='{url}'",
        path = path.display()))
}

/// Calculate SHA1 hash for data in a file, formatted as a lower-case hex string.
async fn calculate_file_sha1(
    path: &Path,
) -> Result<String> {
    (async || -> Result<String> {
        let file = tokio::fs::File::open(&*path)
                       .await
                       .with_context(|| "while opening the file")?;
        let mut sha1_hasher = Sha1::new();
        let mut bytes_stream = tokio_util::io::ReaderStream::new(file);

        while let Some(chunk) = bytes_stream.next().await {
            let chunk = chunk.with_context(|| "while reading a chunk of bytes from the file")?;
            sha1_hasher.update(&chunk);
        }

        let sha1_bytes = sha1_hasher.finalize();
        let sha1_string = hex::encode(sha1_bytes);
        Ok(sha1_string)
    })().await.with_context(|| format!("while calculating the SHA1 hash for a file \
                                        path={path}",
                                       path = path.display()))
}

#[cfg(test)]
mod tests {
    use super::validate_file_relative_url;

    #[test]
    fn test_validate_file_relative_url() {
        let cases: &[(&str, Result<(), ()>)] = &[
            ("/enwiki/20230301/enwiki-20230301-abstract17.xml.gz", Ok(())),
            ("", Err(())),
            ("/", Err(())),
            ("a", Err(())),
            ("a/", Err(())),
            ("/a", Ok(())),
            ("/abc123ABC.-_", Ok(())),
            ("//", Err(())),
            ("//a", Err(())),
            ("/abc/123", Ok(())),
            ("/abc/123/", Err(())),
            ("/abc/123/..", Err(())),
            ("/abc/123/.", Err(())),
            ("/abc/../123", Err(())),
            ("/abc/./123", Err(())),
        ];

        let mut failures: usize = 0;

        for (input, expected) in cases.iter() {
            let output = validate_file_relative_url(input);
            println!(r#"case input="{input}" expected={expected:?} output={output:?}"#);
            if expected.is_ok() != output.is_ok() {
                println!("  Case failed!\n");
                failures += 1;
            } else {
                println!("  Case OK!\n");
            }
        }

         assert!(failures == 0);
    }
}
