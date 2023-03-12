//! Shared code for making HTTP requests

use anyhow::Context;
use crate::{
    args,
    Result,
};
use sha1::{Digest, Sha1};
use std::{
    path::Path,
    time::Duration,
};
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;
use tracing::Level;

#[derive(Clone, Debug)]
pub struct DownloadFileResult {
    /// SHA1 hash calculated over the downloaded file body, formatted as a lower-case hex string.
    pub sha1: String,

    /// Downloaded file size.
    pub len: u64,
}

pub struct FetchTextResult {
    pub response_body: String,
    pub response_code: reqwest::StatusCode,
}

pub type Client = reqwest_middleware::ClientWithMiddleware;

/// Constructs a `Client` suitable for fetching metadata.
///
/// Currently enables gzip compression, HTTP caching, and request and connection timeouts.
pub fn metadata_client(args: &args::CommonArgs) -> Result<Client> {
    let inner = inner_client_common()?
                    .timeout(Duration::from_secs(15))
                    .gzip(true)
                    .build()?;

    let with_middleware =
        reqwest_middleware::ClientBuilder::new(inner)
            .with(cache(&args)?)
            .build();

    Ok(with_middleware)
}

fn cache(
    args: &args::CommonArgs
) -> Result<http_cache_reqwest::Cache<http_cache_reqwest::CACacheManager>> {
    let cache_path = args.http_cache_path();
    std::fs::create_dir_all(&*cache_path)
        .context("while creating HTTP cache directory")?;
    let cache_path_string = cache_path.to_str().ok_or_else(
                                || anyhow::Error::msg(format!(
                                      "Couldn't convert HTTP cache path '{path}' to a String",
                                      path = args.http_cache_path().display())))?.to_string();

    Ok(http_cache_reqwest::Cache(
           http_cache_reqwest::HttpCache {
               mode: http_cache_reqwest::CacheMode::Default,
               manager: http_cache_reqwest::CACacheManager {
                   path: cache_path_string,
               },
               options: None,
           }))
}

/// Constructs a `Client` suitable for downloading large files.
///
/// Currently disables gzip compression, HTTP caching; enables only a connection timeout.
pub fn download_client(_args: &args::CommonArgs) -> Result<Client> {
    let inner = inner_client_common()?
                    .gzip(false)
                    .build()?;
    let with_middleware =
        reqwest_middleware::ClientBuilder::new(inner)
            .build();

    Ok(with_middleware)
}

fn inner_client_common() -> Result<reqwest::ClientBuilder> {
    Ok(reqwest::ClientBuilder::new()
           .user_agent(concat!(
               env!("CARGO_PKG_NAME"),
               "/",
               env!("CARGO_PKG_VERSION"),))
           .connect_timeout(Duration::from_secs(10))
    )
}

#[tracing::instrument(level = "trace", skip(client), ret)]
pub async fn download_file(
    client: &Client,
    request: reqwest::Request,
    file_path: &Path,
) -> Result<DownloadFileResult> {

    let url = request.url().clone();
    let method = request.method().clone();

    // Closure to add context to errors.
    (async || {

        tracing::info!(url = %url.clone(),
                       method = %method.clone(),
                       "Beginning HTTP download");

        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&*file_path)
            .await
            .with_context(|| "opening output file for writing")?;

        let download_res = client.execute(request).await?;
        let download_res_code = download_res.status();
        let download_res_code_int = download_res_code.as_u16();
        let download_res_code_str = download_res_code.canonical_reason().unwrap_or("");
        tracing::debug!(url = %url.clone(),
                        method = %method.clone(),
                        response_code = download_res_code_int,
                        response_code_str = download_res_code_str,
                        "download_file HTTP status");

        if !download_res_code.is_success() {
            return Err(anyhow::Error::msg(
                format!("HTTP response error code \
                         response_code={download_res_code_int} \
                         response_code_str='{download_res_code_str}'")));
        }

        let mut bytes_stream = download_res.bytes_stream();
        let mut sha1_hasher = Sha1::new();

        while let Some(chunk) = bytes_stream.next().await {
            let chunk = chunk
                .with_context(|| format!("while reading the next chunk"))?;
            sha1_hasher.update(&chunk);
            tokio::io::copy(&mut chunk.as_ref(), &mut file)
                .await
                .with_context(|| "while writing a downloaded chunk to disk")?;
        }

        file.flush().await?;
        file.sync_all().await?;

        let file_len = file.metadata().await?.len();

        drop(file);

        tracing::debug!("download_file download complete");

        let sha1_hash = sha1_hasher.finalize();
        let sha1_hash_string = hex::encode(sha1_hash);

        Ok(DownloadFileResult {
            sha1: sha1_hash_string,
            len: file_len,
        })
    })().await.with_context(|| format!("while downloading HTTP response to file \
                                        url='{url}' \
                                        method={method} \
                                        file_path={file_path}",
                                       file_path = file_path.display()))
}

#[tracing::instrument(
    level = "trace",
    skip(client, request),
    fields(url = %request.url().clone(),
           method = %request.method().clone()))]
pub async fn fetch_text(
    client: &Client,
    request: reqwest::Request,
) -> Result<FetchTextResult> {

    let url = request.url().clone();
    let method = request.method().clone();

    // Closure to add context to errors.
    (async || {
        tracing::info!(url = %url.clone(),
                       method = %method.clone(),
                       "Beginning HTTP fetch_text");

        let response = client.execute(request).await?;

        let res_code = response.status();
        let res_code_int = res_code.as_u16();
        let res_code_str = res_code.canonical_reason().unwrap_or("");
        tracing::debug!(url = %url.clone(),
                        method = %method.clone(),
                        response_code = res_code_int,
                        response_code_str = res_code_str,
                        "HTTP response headers");

        let res_text = response.text().await?;
        if tracing::enabled!(Level::TRACE) {
            tracing::trace!(body_text = res_text.clone(),
                            "HTTP response body");
        }

        if !res_code.is_success() {
            return Err(anyhow::Error::msg(
                format!("HTTP response code error \
                         response_code={res_code_int} \
                         response_code_str={res_code_str}")));
        }

        Ok(FetchTextResult {
            response_body: res_text,
            response_code: res_code,
        })
    })().await.with_context(|| format!("while fetching HTTP response as text \
                                        url='{url}' \
                                        method={method}"))
}
