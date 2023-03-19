mod http_cache_mode;
use http_cache_mode::HttpCacheModeParser;

use crate::{
    article_dump::Compression,
    types::{Version, VersionSpec},
    UserRegex,
};
use http_cache_reqwest::CacheMode as HttpCacheMode;
use std::{
    path::PathBuf,
    result::Result as StdResult,
    str::FromStr,
};

#[derive(clap::Args, Clone, Debug)]
pub struct CommonArgs {
    /// The directory to save the program's output, including downloaded files and HTTP cache.
    ///
    /// The dump files will be placed in a child directory of this.
    /// With `--out-dir` set to `./out`, dump file paths will be like:
    /// `./out/enwiki/20230301/metacurrentdumprecombine/enwiki-20230301-pages-articles.xml.bz2`
    ///
    /// If not present tries to read the environment variable `WMD_OUT_DIR`.
    #[arg(long, env = "WMD_OUT_DIR")]
    pub out_dir: PathBuf,

    /// HTTP cache mode to use when making requests.
    ///
    /// See the `http-cache` crate documentation for an explanation of each of the options:
    /// <https://docs.rs/http-cache/0.10.1/http_cache/enum.CacheMode.html>
    #[arg(long, default_value = "Default", value_parser = HttpCacheModeParser)]
    pub http_cache_mode: HttpCacheMode,
}

#[derive(clap::Args, Clone, Debug)]
pub struct DumpNameArg {
    /// The name of the dump to use, e.g. `enwiki`.
    ///
    /// If not present tries to read the environment variable `WMD_DUMP`,
    /// finally uses `enwiki` as a default.
    #[arg(id = "dump", long = "dump", default_value = "enwiki", env = "WMD_DUMP")]
    pub value: String,
}

#[derive(clap::Args, Clone, Debug)]
pub struct VersionSpecArg {
    /// The dump version to use. If omitted the latest available version is used.
    ///
    /// The value must be 8 numerical digits (e.g. "20230301") or the string "latest".
    #[arg(id = "version", long = "version", default_value = "latest")]
    pub value: VersionSpec,
}

#[derive(clap::Args, Clone, Debug)]
pub struct VersionArg {
    /// The dump version to use.
    ///
    /// The value must be 8 numerical digits (e.g. "20230301").
    #[arg(id = "version", long = "version")]
    pub value: Version,
}

#[derive(clap::Args, Clone, Debug)]
pub struct JobNameArg {
    /// The name of the job to use, e.g. `articlesdump`.
    ///
    /// If not present tries to read the environment variable `WMD_JOB`,
    /// finally uses `articlesdump` as a default.
    #[arg(id = "job", long = "job", default_value = "articlesdump", env = "WMD_JOB")]
    pub value: String,
}

#[derive(clap::Args, Clone, Debug)]
pub struct DumpFileSpecArgs {
    #[clap(flatten)]
    pub dump_name: DumpNameArg,

    #[clap(flatten)]
    pub version: VersionArg,

    #[clap(flatten)]
    pub job_name: JobNameArg,

    #[arg(long)]
    pub dump_file: Option<PathBuf>,

    #[arg(long)]
    pub job_dir: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = Compression::Bzip2)]
    pub compression: Compression,

    /// Maximum count of pages to import. No limit if omitted.
    #[arg(long)]
    pub count: Option<usize>,
}

#[derive(clap::Args, Clone, Debug)]
pub struct FileNameRegexArg {
    /// A regex to filter the file names to process from a job.
    ///
    /// The regex syntax used is from the `regex` crate, see their documentation: <https://docs.rs/regex/latest/regex/#syntax>
    #[arg(id = "file-name-regex", long="file-name-regex")]
    pub value: Option<UserRegex>,
}

#[derive(clap::Args, Clone, Debug)]
pub struct JsonOutputArg {
    /// Print results to stdout as JSON. By default the data will be printed as text.
    #[arg(id = "json", long = "json", default_value_t = false)]
    pub value: bool,
}

impl CommonArgs {
    pub fn http_cache_path(&self) -> PathBuf {
        self.out_dir.join("_http_cache")
    }

    pub fn page_store_path(&self) -> PathBuf {
        self.out_dir.join("_page_store")
    }
}

impl FromStr for VersionSpec {
    type Err = clap::Error;

    fn from_str(s: &str) -> StdResult<VersionSpec, clap::Error> {
        if s == "latest" {
            return Ok(VersionSpec::Latest);
        }

        if lazy_regex!(r"^\d{8}$").is_match(s) {
            Ok(VersionSpec::Version(Version(s.to_string())))
        } else {
            Err(clap::error::Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "The value must be 8 numerical digits (e.g. \"20230301\") \
                 or the string \"latest\"."))
        }
    }
}

impl FromStr for Version {
    type Err = clap::Error;

    fn from_str(s: &str) -> StdResult<Version, clap::Error> {
        if lazy_regex!(r"^\d{8}$").is_match(s) {
            Ok(Version(s.to_string()))
        } else {
            Err(clap::error::Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "The value must be 8 numerical digits (e.g. \"20230301\")."))
        }
    }
}
