use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    pub size: u64,
    pub url: String,
    pub sha1: Option<String>,

    #[allow(dead_code)] // Not used currently
    pub md5: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct JobOutput {
    pub name: String,

    #[serde(flatten)]
    pub status: JobStatus,
}

#[derive(Debug, Serialize)]
pub struct FileInfoOutput {
    pub name: String,

    #[serde(flatten)]
    pub metadata: FileMetadata,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct Version(pub String);

#[derive(Clone, Debug)]
pub enum VersionSpec {
    Latest,
    Version(Version),
}
