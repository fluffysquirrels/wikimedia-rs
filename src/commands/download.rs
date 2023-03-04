// use anyhow::Context;
use crate::{
    args::CommonArgs,
    Result,
};
use regex::Regex;
use std::{
    path::PathBuf,
    time::Duration,
};
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
    /// With `--out-dir` set to `./out`, dump files will be like:
    /// `./out/enwiki/20230301/enwiki-20230301-pages-articles.xml.bz2`
    ///
    /// If not present tries to read the environment variable `WMD_OUT_DIR`.
    #[arg(long, env = "WMD_OUT_DIR")]
    out_dir: PathBuf,
}

pub async fn main(args: Args) -> Result<()> {
    let client = reqwest::ClientBuilder::new()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION"),
            ))
        // .gzip(true)
        // .timeout(...)
        .build()?;
    let vers = get_dump_versions(&client, &*args.dump_name).await?;
    Ok(())
}

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
        tracing::warn!(errors = format!("{:?}", doc.errors),
                       "dump versions body had HTML parse errors");
    }

    let mut ret = Vec::<String>::new();

    // TODO: Use lazy_static!
    let date_href_re = Regex::new(r"^(?P<date>\d{8})/$").expect("parse regex");

    for link in doc.select(&scraper::Selector::parse("a").expect("parse selector")) {
        let href = link.value().attr("href");
        if tracing::enabled!(Level::TRACE) {
            tracing::trace!(href = href, "dump versions link");
        }

        let Some(href) = href else {
            continue;
        };

        let Some(cap) = date_href_re.captures(href) else {
            continue;
        };

        ret.push(cap.name("date").expect("regex capture name").as_str().to_string());
    }

    if tracing::enabled!(Level::TRACE) {
        tracing::trace!(versions = format!("{:?}", ret),
                       "dump versions ret");
    }

    Ok(ret)
}
