// use anyhow::Context;
// use crate::{
//     Result,
// };
// use std::path::{ Path, PathBuf };

#[derive(clap::Args, Clone, Debug)]
pub struct CommonArgs {}

#[derive(clap::Args, Clone, Debug)]
pub struct DumpNameArg {
    /// The name of the dump to download, e.g. `enwiki`.
    ///
    /// If not present tries to read the environment variable `WMD_DUMP`,
    /// finally uses `enwiki` as a default.
    #[arg(id = "dump", long = "dump", default_value = "enwiki", env = "WMD_DUMP")]
    pub value: String,
}

#[derive(clap::Args, Clone, Debug)]
pub struct JobNameArg {
    /// The name of the job to download, e.g. `metacurrentdumprecombine`.
    ///
    /// If not present tries to read the environment variable `WMD_JOB`,
    /// finally uses `metacurrentdumprecombine` as a default.
    #[arg(id = "job", long = "job", default_value = "metacurrentdumprecombine", env = "WMD_JOB")]
    pub value: String,
}
