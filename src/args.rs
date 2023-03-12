use clap::builder::PossibleValue;
use crate::{
    types::{Version, VersionSpec},
    UserRegex,
};
use http_cache_reqwest::CacheMode as HttpCacheMode;
use once_cell::sync::Lazy;
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    ops::Deref,
    path::PathBuf,
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
    /// https://docs.rs/http-cache/0.10.1/http_cache/enum.CacheMode.html
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
pub struct JobNameArg {
    /// The name of the job to use, e.g. `metacurrentdumprecombine`.
    ///
    /// If not present tries to read the environment variable `WMD_JOB`,
    /// finally uses `metacurrentdumprecombine` as a default.
    #[arg(id = "job", long = "job", default_value = "metacurrentdumprecombine", env = "WMD_JOB")]
    pub value: String,
}

#[derive(clap::Args, Clone, Debug)]
pub struct FileNameRegexArg {
    /// A regex to filter the file names to process from a job.
    ///
    /// The regex syntax used is from the `regex` crate, see their documentation: https://docs.rs/regex/latest/regex/#syntax
    #[arg(id = "file-name-regex", long="file-name-regex")]
    pub value: Option<UserRegex>,
}

#[derive(clap::Args, Clone, Debug)]
pub struct JsonOutputArg {
    /// Print results to stdout as JSON. By default the data will be printed as text.
    #[arg(id = "json", long = "json", default_value_t = false)]
    pub value: bool,
}

#[derive(Clone)]
struct HttpCacheModeParser;

static HTTP_CACHE_MODE_MAP: Lazy<BTreeMap<String, HttpCacheMode>> = Lazy::new(|| {
    let mut map = BTreeMap::new();

    let mut add = |val: HttpCacheMode| {
        map.insert(format!("{:?}", val), val);
    };

    add(HttpCacheMode::Default);
    add(HttpCacheMode::NoStore);
    add(HttpCacheMode::Reload);
    add(HttpCacheMode::NoCache);
    add(HttpCacheMode::ForceCache);
    add(HttpCacheMode::OnlyIfCached);

    drop(add);

    map
});

impl clap::builder::TypedValueParser for HttpCacheModeParser {
    type Value = HttpCacheMode;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value_cow = value.to_string_lossy();
        HTTP_CACHE_MODE_MAP.get(value_cow.deref())
            .ok_or_else(|| clap::Error::raw(
                clap::error::ErrorKind::InvalidValue,
                format!("Argument value was not a valid HttpCache value: '{value_cow}'. \
                         Possible values are: {vals}.\n",
                        vals = HTTP_CACHE_MODE_MAP.keys()
                                                  .map(|s| format!("'{s}'"))
                                                  .collect::<Vec<String>>()
                                                  .join(", ")
                                           )))
            .cloned()
    }

    fn possible_values(
        &self
    ) -> Option<Box<dyn Iterator<Item = PossibleValue>>> {
        Some(Box::new(
            HTTP_CACHE_MODE_MAP.keys()
                               .map(|name: &String| {
                                   let clap_str: clap::builder::Str = name.into();
                                   let possible_value: PossibleValue = clap_str.into();
                                   possible_value
                               })
                               .collect::<Vec<PossibleValue>>()
                               .into_iter()
        ))
    }
}

impl CommonArgs {
    pub fn http_cache_path(&self) -> PathBuf {
        self.out_dir.join("_http_cache")
    }
}

impl FromStr for VersionSpec {
    type Err = clap::Error;

    fn from_str(s: &str) -> std::result::Result<VersionSpec, clap::Error> {
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
