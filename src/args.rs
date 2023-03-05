use crate::types::{Version, VersionSpec};
use regex::Regex;

#[derive(clap::Args, Clone, Debug)]
pub struct CommonArgs {}

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

impl std::str::FromStr for VersionSpec {
    type Err = clap::Error;

    fn from_str(s: &str) -> std::result::Result<VersionSpec, clap::Error> {
        if s == "latest" {
            return Ok(VersionSpec::Latest);
        }

        // TODO: Use lazy_static!
        let version_re = Regex::new(r"^\d{8}$").expect("compile regex");

        if version_re.is_match(s) {
            Ok(VersionSpec::Version(Version(s.to_string())))
        } else {
            Err(clap::error::Error::raw(clap::error::ErrorKind::ValueValidation,
                                        r#"The value must be 8 numerical digits (e.g. "20230301") or the string "latest"."#))
        }
    }
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
