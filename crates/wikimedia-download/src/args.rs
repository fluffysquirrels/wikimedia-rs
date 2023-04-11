mod http_cache_mode;
use http_cache_mode::HttpCacheModeParser;

use anyhow::bail;
use clap::CommandFactory;
use http_cache_reqwest::CacheMode as HttpCacheMode;
use std::path::{Path, PathBuf};
use wikimedia::{
    dump::{
        self,
        DumpName, JobName, Version, VersionSpec,
        local::Compression,
    },
    http,
    Result,
    UserRegex,
};
use wikimedia_store as store;

#[derive(clap::Args, Clone, Debug)]
pub struct CommonArgs {
    #[arg(from_global)]
    log_json: bool,

    /// The name of the store dump to use, e.g. `enwiki`.
    ///
    /// If not present tries to read the environment variable `WMD_STORE_DUMP`,
    /// finally uses `enwiki` as a default.
    #[arg(id = "store-dump", long = "store-dump", default_value = "enwiki",
          env = "WMD_STORE_DUMP")]
    store_dump_name: DumpName,

    /// The directory to save the program's output, including downloaded files and HTTP cache.
    ///
    /// If not present tries these alternatives in order:
    ///
    ///   * Value in environment variable `WMD_OUT_DIR`.
    ///   * A subdirectory `wmd` under the platform data directory returned by
    ///     `platform_dirs::AppDirs.data_dir`.
    ///
    ///     For Linux this is: `${XDG_DATA_HOME}` if set or `~/.local/share`
    ///
    ///     For Windows this is `%LOCALAPPDATA%` if set or `C:\Users\%USERNAME%\AppData\Local`
    ///
    ///     For macOS this is `~/Library/Application Support`
    ///
    ///     See the `platform-dirs` documentation:
    ///     <https://github.com/cjbassi/platform-dirs-rs/blob/b7a5a9ad4535aa4fb156bfeb9cf887dd2bd696a4/README.md#appdirs>.
    ///
    /// The dump files downloaded from wikimedia will be placed under the subdirectory `dumps`,
    /// for example:
    /// `dumps/enwiki/20230301/articlesdump/enwiki-20230301-pages-articles1.xml.bz2`
    ///
    /// The data imported from the dumps will be placed under the subdirectory 'store'.
    #[arg(long, env = "WMD_OUT_DIR")]
    out_dir: Option<PathBuf>,

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

    /// A single job file to use.
    #[arg(long)]
    pub job_file: Option<PathBuf>,

    /// Seek to this file offset before reading.
    ///
    /// Can be used with multistream dump files.
    ///
    /// Only used when --job-file is set.
    #[arg(long)]
    pub seek: Option<u64>,

    /// A directory of job files to use.
    #[arg(long)]
    pub job_dir: Option<PathBuf>,

    /// The compression format to use when reading files.
    #[arg(long, value_enum, default_value_t = Compression::Bzip2)]
    pub compression: Compression,

    /// Maximum count of pages operate on. No limit if omitted.
    #[arg(long)]
    pub limit: Option<u64>,

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
    pub fn out_dir(&self) -> PathBuf {
        if let Some(dir) = self.out_dir.as_ref() {
            return dir.clone();
        }

        // self.out_dir == None

        // Fall back to platform-dirs.

        let Some(dirs) = platform_dirs::AppDirs::new(
            Some(env!("CARGO_BIN_NAME")) /* app name */,
            false /* use_xdg_on_macos */) else
        {
            let mut cmd = crate::Args::command();

            let err = cmd.error(
                clap::error::ErrorKind::MissingRequiredArgument,
                "Tried to fall back and get out-dir from platform_dirs, \
                 but AppDirs::None returned None. \
                 Try passing out-dir in environment value `WMD_OUT_DIR` \
                 or with flag `--out-dir`.");
            err.exit(); // Exits the process.
        };

        dirs.data_dir
    }

    pub fn dumps_path(&self) -> PathBuf {
        self.out_dir().join("dumps")
    }

    pub fn http_cache_path(&self) -> PathBuf {
        self.out_dir().join("http_cache")
    }

    pub fn store_path(&self) -> PathBuf {
        self.out_dir().join("stores").join(&*self.store_dump_name.0)
    }

    pub fn http_options(&self) -> Result<http::OptionsBuilder> {
        Ok(http::OptionsBuilder::default()
               .cache_path(self.http_cache_path())
               .cache_mode(self.http_cache_mode)
               .to_owned())
    }

    pub fn store_dump_name(&self) -> DumpName {
        self.store_dump_name.clone()
    }

    pub fn store_options(&self) -> Result<store::Options> {
        Ok(store::Options::default()
               .dump_name(self.store_dump_name.clone())
               .path(self.store_path())
               .to_owned())
    }
}

impl OpenSpecArgs {
    pub fn try_into_open_spec(self, dumps_dir: &Path) -> Result<dump::local::OpenSpec> {
        let source: dump::local::SourceSpec = match (self.job_file, self.job_dir) {
            (Some(_), Some(_)) => bail!("You supplied both --job-file and --job-dir, \
                                         but should only supply one of these"),
            (Some(file), None) => {
                dump::local::SourceSpec::File(dump::local::FileSpec {
                    compression: self.compression,
                    path: file,
                    seek: self.seek,
                })
            },
            (None, Some(dir)) => {
                dump::local::SourceSpec::Dir(dump::local::DirSpec {
                    path: dir,
                    file_name_regex: self.file_name_regex.value,
                })
            }
            (None, None) => {
                match (self.dump_name.as_ref(),
                       self.version.as_ref(),
                       self.job_name.as_ref()) {
                    (Some(dump), Some(version), Some(job)) =>
                        dump::local::SourceSpec::Job(dump::local::JobSpec {
                            out_dir: dumps_dir.to_owned(),
                            dump: dump.value.clone(),
                            version: version.clone(),
                            job: job.value.clone(),
                            file_name_regex: self.file_name_regex.value,
                        }),
                    _ => bail!("You must supply one of these 3 valid argument sets:\n\
                                1. `--dump-file`\n\
                                2. `--job-dir'\n\
                                3. `--dump`, `--version`, and `--job`"),
                }
            },
        }; // end of match on arg choices.

        Ok(dump::local::OpenSpec {
            compression: self.compression,
            source,
            limit: self.limit,
        })
    }
}
