mod http_cache_mode;
use http_cache_mode::HttpCacheModeParser;

use anyhow::bail;
use crate::{
    dump::{
        DumpName, JobName, Version, VersionSpec,
        local::{self, Compression},
    },
    Error,
    Result,
    UserRegex,
};
use http_cache_reqwest::CacheMode as HttpCacheMode;
use std::{
    convert::TryFrom,
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
    /// `./out/dumps/enwiki/20230301/articlesdump/enwiki-20230301-pages-articles.xml.bz2`
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
    pub value: DumpName,
}

#[derive(clap::Args, Clone, Debug)]
pub struct VersionSpecArg {
    /// The dump version to use. If omitted tries to read the
    /// environment variable "WMD_VERSION", then falls back to the
    /// default "latest".
    ///
    /// The value must be 8 numerical digits (e.g. "20230301") or the string "latest".
    #[arg(id = "version", long = "version", default_value = "latest", env = "WMD_VERSION")]
    pub value: VersionSpec,
}

#[derive(clap::Args, Clone, Debug)]
pub struct JobNameArg {
    /// The name of the job to use, e.g. `articlesdump`.
    ///
    /// If not present tries to read the environment variable `WMD_JOB`,
    /// finally uses `articlesdump` as a default.
    #[arg(id = "job", long = "job", default_value = "articlesdump", env = "WMD_JOB")]
    pub value: JobName,
}

#[derive(clap::Args, Clone, Debug)]
pub struct OpenSpecArgs {
    #[clap(flatten)]
    pub dump_name: Option<DumpNameArg>,

    /// The dump version to use.
    ///
    /// The value must be 8 numerical digits (e.g. "20230301").
    ///
    /// If not present tries to read the environment variable `WMD_VERSION`.
    #[arg(long, env = "WMD_VERSION")]
    pub version: Option<Version>,

    #[clap(flatten)]
    pub job_name: Option<JobNameArg>,

    #[arg(long)]
    pub dump_file: Option<PathBuf>,

    /// Seek to this file offset before reading.
    ///
    /// Can be used with multistream dump files.
    ///
    /// Only used when --dump-file is set.
    #[arg(long)]
    pub seek: Option<u64>,

    #[arg(long)]
    pub job_dir: Option<PathBuf>,

    #[arg(long, value_enum, default_value_t = Compression::Bzip2)]
    pub compression: Compression,

    /// Maximum count of pages operate on. No limit if omitted.
    #[arg(long)]
    pub count: Option<u64>,

    #[clap(flatten)]
    pub file_name_regex: FileNameRegexArg,
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
        self.out_dir.join("http_cache")
    }

    pub fn store_path(&self) -> PathBuf {
        self.out_dir.join("store")
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

impl TryFrom<(CommonArgs, OpenSpecArgs)> for local::OpenSpec {
    type Error = Error;

    fn try_from((common, args): (CommonArgs, OpenSpecArgs)) -> Result<local::OpenSpec> {
        let dump_file = args.dump_file;
        let job_dir = args.job_dir;

        let source: local::SourceSpec = match (dump_file, job_dir) {
            (Some(_), Some(_)) => bail!("You supplied both --dump-file and --job-dir, \
                                         but should only supply one of these"),
            (Some(file), None) => {
                local::SourceSpec::File(local::FileSpec {
                    path: file,
                    seek: args.seek,
                })
            },
            (None, Some(dir)) => {
                local::SourceSpec::Dir(local::DirSpec {
                    path: dir,
                    file_name_regex: args.file_name_regex.value,
                })
            }
            (None, None) => {
                match (args.dump_name.as_ref(),
                       args.version.as_ref(),
                       args.job_name.as_ref()) {
                    (Some(dump), Some(version), Some(job)) =>
                        local::SourceSpec::Job(local::JobSpec {
                            out_dir: common.out_dir,
                            dump: dump.value.clone(),
                            version: version.clone(),
                            job: job.value.clone(),
                            file_name_regex: args.file_name_regex.value,
                        }),
                    _ => bail!("You must supply one of these 3 valid argument sets:\n\
                                1. `--dump-file`\n\
                                2. `--job-dir'\n\
                                3. `--dump`, `--version`, and `--job`"),
                }
            },
        }; // end of match on arg choices.

        Ok(local::OpenSpec {
            compression: args.compression,
            source,
            max_count: args.count,
        })
    }
}
