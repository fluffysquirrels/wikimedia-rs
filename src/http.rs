//! Shared code for making HTTP requests

use anyhow::{bail, Context, format_err};
use crate::{
    args,
    Result,
    util::fmt::{Bytes, TransferStats},
};
use encoding_rs::{Encoding, UTF_8};
use sha1::{Digest, Sha1};
use std::{
    convert::TryFrom,
    fmt::Debug,
    path::Path,
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;
use tracing::Level;
use valuable::{Fields, NamedField, NamedValues, Structable, StructDef, Valuable, Value, Visit};

#[derive(Clone, Debug, Valuable)]
pub struct DownloadFileResult {
    /// SHA1 hash calculated over the downloaded file body, formatted as a lower-case hex string.
    pub sha1: String,
    pub stats: TransferStats,
    pub response_code: StatusCode,
}

#[derive(Clone, Debug, Valuable)]
pub struct FetchTextResult {
    pub response_body: String,
    pub response_code: StatusCode,
    pub stats: TransferStats,
}

#[derive(Clone, Copy)]
pub struct StatusCode(pub reqwest::StatusCode);

pub type Client = reqwest_middleware::ClientWithMiddleware;

impl Debug for StatusCode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "StatusCode({code_int} = '{code_str}')",
               code_int = self.as_u16(),
               code_str = self.as_str())
    }
}

impl StatusCode {
    const FIELDS: &[NamedField<'static>] = &[
        NamedField::new("int"),
        NamedField::new("str"),
    ];

    pub fn as_u16(&self) -> u16 {
        self.0.as_u16()
    }

    pub fn as_str(&self) -> &'static str {
        self.0.canonical_reason().unwrap_or("")
    }
}

impl Valuable for StatusCode {
    fn as_value(&self) -> Value<'_> {
        Value::Structable(self)
    }

    fn visit(&self, visit: &mut dyn Visit) {
        visit.visit_named_fields(
            &NamedValues::new(
                Self::FIELDS,
                &[Value::U16(self.as_u16()),
                  Value::String(self.as_str())]))
    }
}

impl Structable for StatusCode {
    fn definition(&self) -> StructDef<'_> {
        StructDef::new_static("StatusCode", Fields::Named(Self::FIELDS))
    }
}

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

fn cache(
    args: &args::CommonArgs
) -> Result<http_cache_reqwest::Cache<http_cache_reqwest::CACacheManager>> {
    let cache_path = args.http_cache_path();
    std::fs::create_dir_all(&*cache_path)
        .context("while creating HTTP cache directory")?;
    let cache_path_string = cache_path.to_str().ok_or_else(
                                || format_err!(
                                       "Couldn't convert HTTP cache path '{path}' to a String",
                                       path = args.http_cache_path().display()))?.to_string();

    Ok(http_cache_reqwest::Cache(
           http_cache_reqwest::HttpCache {
               mode: args.http_cache_mode,
               manager: http_cache_reqwest::CACacheManager {
                   path: cache_path_string,
               },
               options: None,
           }))
}

#[tracing::instrument(level = "trace", skip(client), ret)]
pub async fn download_file(
    client: &Client,
    request: reqwest::Request,
    file_path: &Path,
) -> Result<DownloadFileResult> {

    let start_time = Instant::now();

    let url = request.url().clone();
    let method = request.method().clone();

    // Closure to add context to errors.
    (async || {

        tracing::debug!(url = %url.clone(),
                        method = %method.clone(),
                        "http::download_file() beginning");

        let mut file = tokio::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&*file_path)
            .await
            .with_context(|| "opening output file for writing")?;

        let download_res = client.execute(request).await?;
        let download_res_code = StatusCode(download_res.status());
        tracing::debug!(url = %url.clone(),
                        method = %method.clone(),
                        response_code = download_res_code.as_value(),
                        "http::download_file() response HTTP status");

        if !download_res_code.0.is_success() {
            bail!("HTTP response error code \
                   response_code={download_res_code:?}");
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

        let file_len = Bytes(file.metadata().await?.len());

        drop(file);

        let sha1_hash = sha1_hasher.finalize();
        let sha1_hash_string = hex::encode(sha1_hash);

        let duration = start_time.elapsed();

        let res = DownloadFileResult {
            response_code: download_res_code,
            sha1: sha1_hash_string,
            stats: TransferStats::new(file_len, duration),
        };

        tracing::debug!(url = %url.clone(),
                        method = %method.clone(),
                        res = res.as_value(),
                        "http::download_file() done");

        Ok(res)
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

    let start_time = Instant::now();

    let url = request.url().clone();
    let method = request.method().clone();

    // Closure to add context to errors.
    (async || {
        tracing::info!(url = %url.clone(),
                       method = %method.clone(),
                       "http::fetch_text() beginning");

        let response = client.execute(request).await?;

        let res_code = StatusCode(response.status());
        tracing::debug!(url = %url.clone(),
                        method = %method.clone(),
                        response_code = res_code.as_value(),
                        "HTTP response headers");

        // Text decoding copied from reqwest::Response::text(),
        // but tweaked to access the response body length.
        let default_encoding = "utf-8";
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<mime::Mime>().ok());
        let encoding_name = content_type
            .as_ref()
            .and_then(|mime| mime.get_param("charset").map(|charset| charset.as_str()))
            .unwrap_or(default_encoding);
        let encoding = Encoding::for_label(encoding_name.as_bytes()).unwrap_or(UTF_8);

        let response_body_bytes = response.bytes().await?;

        let (text, _, _) = encoding.decode(&response_body_bytes);
        let response_body_string = if let std::borrow::Cow::Owned(s) = text {
            s
        } else {
            unsafe {
                // decoding returned Cow::Borrowed, meaning these bytes
                // are already valid utf8
                String::from_utf8_unchecked(response_body_bytes.to_vec())
            }
        };

        if tracing::enabled!(Level::TRACE) {
            tracing::trace!(body_text = response_body_string.clone(),
                            "HTTP response body");
        }

        if !res_code.0.is_success() {
            bail!("HTTP response code error \
                   response_code={res_code:?}");
        }

        let duration = start_time.elapsed();

        let len = response_body_bytes.len();

        let res = FetchTextResult {
            response_body: response_body_string,
            response_code: res_code,
            stats: TransferStats::new(
                       Bytes(u64::try_from(len).expect("usize to convert to u64")),
                       duration),
        };

        tracing::info!(%url,
                       %method,
                       res.code = res_code.as_value(),
                       res.stats = res.stats.as_value(),
                       "http::fetch_text() complete");

        Ok(res)
    })().await.with_context(|| format!("while fetching HTTP response as text \
                                        url='{url}' \
                                        method={method}"))
}
