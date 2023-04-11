//! Data types used in Wikimedia data dumps and their metadata.

use crate::{
    Error,
    Result,
    slug,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt::{self, Display},
    result::Result as StdResult,
    str::FromStr,
};
use valuable::Valuable;

#[derive(Debug, Deserialize, Serialize)]
pub struct DumpVersionStatus {
    pub jobs: BTreeMap<String, JobStatus>,

    #[allow(dead_code)] // Not used currently
    pub version: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct JobStatus {
    pub status: String,

    #[allow(dead_code)] // Not used currently
    pub updated: String,

    #[serde(default)]
    pub files: BTreeMap<String, FileMetadata>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileMetadata {
    /// File length in bytes. Missing for jobs with status "waiting".
    pub size: Option<u64>,

    /// File relative URL under the dumps root. Missing for jobs with status "waiting".
    pub url: Option<String>,

    /// Expected SHA1 hash of the file's data, formatted as a lowercase hex string.
    pub sha1: Option<String>,

    #[allow(dead_code)] // Not used currently
    pub md5: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobOutput {
    pub name: String,

    /// Sum of the sizes of each file.
    pub files_size: u64,

    /// Count of files.
    pub files_count: usize,

    #[serde(flatten)]
    pub status: JobStatus,
}

#[derive(Debug, Serialize)]
pub struct FileInfoOutput {
    pub name: String,

    #[serde(flatten)]
    pub metadata: FileMetadata,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Valuable)]
pub struct DumpName(pub String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Valuable)]
pub struct Version(pub String);

#[derive(Clone, Debug)]
pub enum VersionSpec {
    Latest,
    Version(Version),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Valuable)]
pub struct JobName(pub String);

#[derive(Clone, Debug, Serialize)]
pub struct Page {
    pub ns_id: u64,
    pub id: u64,
    pub title: String,
    pub revision: Option<Revision>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Revision {
    pub id: u64,
    pub text: Option<String>,
    pub categories: Vec<CategoryName>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CategoryName(pub String);

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct CategorySlug(pub String);

impl Display for CategoryName {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "Category:{name}", name = self.0)
    }
}

impl CategoryName {
    pub fn to_slug(&self) -> CategorySlug {
        CategorySlug(slug::title_to_slug(&*self.0))
    }
}

impl FromStr for DumpName {
    type Err = Error;

    fn from_str(s: &str) -> Result<DumpName> {
        Ok(DumpName(s.to_string()))
    }
}

impl FromStr for JobName {
    type Err = Error;

    fn from_str(s: &str) -> Result<JobName> {
        Ok(JobName(s.to_string()))
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

impl Page {
    pub fn revision_text(&self) -> Option<&str> {
        self.revision.as_ref()
            .and_then(|r| r.text.as_ref())
            .map(|t| t.as_str())
    }
}
