//! Read local copies of Wikimedia dump files.

use anyhow::format_err;
use chrono::{DateTime, FixedOffset};
use clap::{
    builder::PossibleValue,
    ValueEnum,
};
use crate::{
    dump::types::*,
    Error,
    ProgressReader,
    Result,
    UserRegex,
    util::{
        fmt::{Bytes, Sha1Hash},
        IteratorExtSend,
    },
    wikitext,
};
use iterator_ext::IteratorExt;
use quick_xml::events::Event;
use rayon::{
    prelude::*,
};
use std::{
    borrow::Cow,
    fmt::{self, Display},
    fs::DirEntry,
    io::{BufRead, BufReader, Error as IoError, Seek},
    iter::Iterator,
    path::{Path, PathBuf},
    result::Result as StdResult,
    sync::{
        Arc,
        atomic::AtomicU64,
    },
    str::FromStr,
};
use tracing::Level;
use valuable::Valuable;

struct FilePageIter<R: BufRead> {
    buf: Vec<u8>,
    file_path: PathBuf,
    xml_read: quick_xml::reader::Reader<R>,
}

pub struct JobFiles {
    file_specs: Vec<FileSpec>,
    files_total_len: Bytes,
    open_spec: OpenSpec,
}

pub struct OpenJobFile {
    pub file_spec: FileSpec,
    pub pages_iter: Box<dyn Iterator<Item = Result<Page>> + Send>,
    pub source_bytes_read: Arc<AtomicU64>,
    pub uncompressed_bytes_read: Arc<AtomicU64>,
}

#[derive(Clone, Debug, Valuable)]
pub struct OpenSpec {
    pub source: SourceSpec,
    pub limit: Option<u64>,
    pub compression: Compression,
}

#[derive(Clone, Debug, Valuable)]
pub enum SourceSpec {
    Job(JobSpec),
    Dir(DirSpec),
    File(FileSpec),
}

#[derive(Clone, Debug, Valuable)]
pub struct JobSpec {
    pub out_dir: PathBuf,
    pub dump: DumpName,
    pub version: Version,
    pub job: JobName,
    pub file_name_regex: Option<UserRegex>,
}

#[derive(Clone, Debug, Valuable)]
pub struct DirSpec {
    pub path: PathBuf,
    pub file_name_regex: Option<UserRegex>,
}

#[derive(Clone, Debug, Valuable)]
pub struct FileSpec {
    pub compression: Compression,
    pub path: PathBuf,
    pub seek: Option<u64>,
}

#[derive(Clone, Copy, Debug, Valuable)]
pub enum Compression {
    Bzip2,
    LZ4,
    Zstd,
    None,
}

impl FromStr for Compression {
    type Err = String;

    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        for variant in Self::value_variants() {
            if variant.to_possible_value().unwrap().matches(s, true) {
                return Ok(*variant);
            }
        }
        Err(format!("invalid variant: {s}"))
    }
}

impl Display for Compression {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl clap::ValueEnum for Compression {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Bzip2, Self::LZ4, Self::Zstd, Self::None]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Self::Bzip2 => {
                PossibleValue::new("bzip2")
                              .alias("bz2")
                              .help("Use bzip2 compression. Alias 'bz2'.")
            }
            Self::LZ4 => PossibleValue::new("lz4").help("Use LZ4 compression."),
            Self::Zstd => PossibleValue::new("zstd").help("Use zstd compression."),
            Self::None => PossibleValue::new("none").help("Use no compression."),
        })
    }
}

pub fn job_file_path(
    out_dir: &Path,
    dump_name: &DumpName,
    version: &Version,
    job_name: &JobName,
    file_meta: &FileMetadata
) -> Result<PathBuf> {
    let rel_url = file_meta.url.as_ref().ok_or_else(|| format_err!("file_meta.url was None"))?;
    let name = rel_url.split('/').last()
                      .ok_or_else(|| format_err!("file_meta.url was empty url='{rel_url}'"))?;
    let path = job_path(out_dir, dump_name, version, job_name)
                   .join(name);
    Ok(path)
}

pub fn job_path(
    out_dir: &Path,
    dump: &DumpName,
    version: &Version,
    job: &JobName,
) -> PathBuf {
    out_dir.join(format!("{dump}/{version}/{job}",
                         dump = &*dump.0,
                         version = version.0,
                         job = &*job.0))
}

impl OpenSpec {
    pub fn open(self) -> Result<JobFiles> {
        let file_specs: Vec<FileSpec> = match &self.source {
            SourceSpec::File(file_spec) => vec![file_spec.clone()],
            SourceSpec::Dir(dir_spec) =>
                file_specs_from_job_dir(&*dir_spec.path, self.compression,
                                        dir_spec.file_name_regex.as_ref())?,
            SourceSpec::Job(job_spec) => {
                let job_path: PathBuf = job_path(&*job_spec.out_dir, &job_spec.dump,
                                                 &job_spec.version, &job_spec.job);
                file_specs_from_job_dir(&*job_path, self.compression,
                                        job_spec.file_name_regex.as_ref())?
            },
        };

        let files_total_len: u64 =
            file_specs.iter().map(|spec| spec.path.metadata()
                                  .map_err(|e: std::io::Error| -> Error { e.into() })
                                  .map(|meta| meta.len()))
            .try_fold(0_u64, |curr, len| -> Result<u64>
                      { Ok(curr + len?) })?;
        let files_total_len = Bytes(files_total_len);

        tracing::debug!(files_total_len = files_total_len.as_value(),
                        file_count = file_specs.len(),
                        "job dir files opened");

        if tracing::enabled!(Level::TRACE) {
            tracing::trace!(file_specs = ?file_specs.iter().map(|s| s.path.to_string_lossy())
                            .collect::<Vec<Cow<'_, str>>>(),
                            "job dir files");
        }

        Ok(JobFiles {
            file_specs: file_specs,
            files_total_len,
            open_spec: self,
        })
    }
}

impl JobFiles {
    #[allow(dead_code)] // Not used yet.
    pub fn file_specs(&self) -> &[FileSpec] {
        &*self.file_specs
    }

    #[allow(dead_code)] // Not used yet.
    pub fn open_spec(&self) -> &OpenSpec {
        &self.open_spec
    }

    pub fn files_total_len(&self) -> Bytes {
        self.files_total_len
    }

    pub fn open_pages_iter(&self) -> Result<Box<dyn Iterator<Item = Result<Page>> + Send>> {
        let file_specs = self.file_specs.clone();

        let pages = file_specs.into_iter()
            .map(move |spec: FileSpec| { // -> Result<OpenJobFile>
                spec.open()
            })
            .try_flat_map_results(|file: OpenJobFile| {
                Ok(file.pages_iter)
            })
            .boxed_send();

        let pages = match self.open_spec.limit {
            None => pages,
            Some(limit) => pages.take(usize::try_from(limit)?).boxed_send(),
        };

        Ok(pages)
    }

    pub fn open_files_par_iter(&self)
    -> Result<impl ParallelIterator<Item = Result<OpenJobFile>>>
    {
        let file_specs: Vec<FileSpec> = self.file_specs.clone();

        let open_files = file_specs.into_par_iter()
            .with_max_len(1) // Each thread processes one file at a time
            .map(|spec: FileSpec| spec.open());
        Ok(open_files)
    }
}

impl FileSpec {
    pub fn open(&self) -> Result<OpenJobFile> {
        tracing::debug!(path = %self.path.display(),
                        ?self.compression,
                        ?self.seek,
                        "dump::local::FileSpec::open_pages_iter()");

        let mut file_read = std::fs::File::open(&*self.path)?;
        if let Some(offset) = self.seek {
            let _ = file_read.seek(std::io::SeekFrom::Start(offset))?;
        }

        let (prog_read, source_bytes_read) = ProgressReader::new(file_read);
        let file_bufread = BufReader::with_capacity(128 * 1024, prog_read);

        fn into_page_iter<T>(file_path: &Path, inner: T
        ) -> Box<dyn Iterator<Item = Result<Page>> + Send>
            where T: BufRead + Send + 'static
        {
            let xml_buf = Vec::<u8>::with_capacity(100_000);
            let xml_read = quick_xml::reader::Reader::from_reader(inner);
            let page_iter = FilePageIter {
                buf: xml_buf,
                file_path: file_path.to_path_buf(),
                xml_read,
            }.boxed_send();
            page_iter
        }

        let (uncompressed_bytes_read, pages_iter) = match self.compression {
            Compression::None => {
                (source_bytes_read.clone(), into_page_iter(&*self.path, file_bufread))
            },
            Compression::Bzip2 => {
                let bzip_decoder = bzip2::bufread::MultiBzDecoder::new(file_bufread);

                let (uncompressed_prog_read, uncompressed_bytes_read) =
                    ProgressReader::new(bzip_decoder);

                let bzip_bufread = BufReader::with_capacity(64 * 1024, uncompressed_prog_read);
                (uncompressed_bytes_read, into_page_iter(&*self.path, bzip_bufread))
            },
            Compression::LZ4 => {
                let lz4_decoder = lz4_flex::frame::FrameDecoder::new(file_bufread);

                let (uncompressed_prog_read, uncompressed_bytes_read) =
                    ProgressReader::new(lz4_decoder);

                let lz4_bufread = BufReader::with_capacity(64 * 1024, uncompressed_prog_read);
                (uncompressed_bytes_read, into_page_iter(&*self.path, lz4_bufread))
            }
            Compression::Zstd => {
                let zstd_decoder = zstd::stream::read::Decoder::with_buffer(file_bufread)?;

                let (uncompressed_prog_read, uncompressed_bytes_read) =
                    ProgressReader::new(zstd_decoder);

                let capacity = zstd::stream::read::Decoder::<'_, std::io::Empty>
                                   ::recommended_output_size();
                let zstd_bufread = BufReader::with_capacity(capacity, uncompressed_prog_read);
                (uncompressed_bytes_read, into_page_iter(&*self.path, zstd_bufread))
            }
        };

        Ok(OpenJobFile {
            file_spec: self.clone(),
            pages_iter,
            source_bytes_read,
            uncompressed_bytes_read,
        })
    }
}

fn file_specs_from_job_dir(
    job_path: &Path,
    compression: Compression,
    user_file_name_regex: Option<&UserRegex>,
) -> Result<Vec<FileSpec>>
{
    let mut file_specs =
        std::fs::read_dir(job_path)?
            .map_err(|e: IoError| -> Error {
                     e.into()
            })
            .try_filter_map(|dir_entry: DirEntry| -> Result<Option<FileSpec>> {
                if !dir_entry.file_type()?.is_file() {
                    return Ok(None);
                }

                const FILE_RE_PREFIX: &'static str =
                    r#".*pages.*articles(-multistream)?.*\.xml.*"#;

                let name_regex = match compression {
                    Compression::Bzip2 => lazy_regex!(FILE_RE_PREFIX, r#"\.bz2$"#),
                    Compression::LZ4 => lazy_regex!(FILE_RE_PREFIX, r#"\.lz4$"#),
                    Compression::Zstd => lazy_regex!(FILE_RE_PREFIX, r#"\.zstd$"#),
                    Compression::None => lazy_regex!(FILE_RE_PREFIX, r#"$"#),
                };
                let name = dir_entry.file_name().to_string_lossy().into_owned();
                if name_regex.is_match(&*name)
                    && user_file_name_regex.as_ref().map_or(true, |re| re.0.is_match(&*name))
                {
                    Ok(Some(FileSpec {
                        compression,
                        path: dir_entry.path(),
                        seek: None,
                    }))
                } else {
                    Ok(None)
                }
            }).try_collect::<Vec<FileSpec>>()?;

    file_specs.sort_by(|a, b| natord::compare(&*a.path.to_string_lossy(),
                                              &*b.path.to_string_lossy()));

    Ok(file_specs)
}

impl<R: BufRead> Iterator for FilePageIter<R> {
    type Item = Result<Page>;

    fn next(&mut self) -> Option<Result<Page>> {
        loop {
            let pos = self.xml_read.buffer_position();
            match try_iter!(self.xml_read.read_event_into(&mut self.buf)) {
                Event::Start(b) if b.name().as_ref() == b"page" => {
                    let page_start_pos = pos;
                    self.buf.clear();
                    let mut page_title: Option<String> = None;
                    let mut page_ns_id: Option<i64> = None;
                    let mut page_id: Option<u64> = None;
                    let mut revision: Option<Revision> = None;
                    loop {
                        match try_iter!(self.xml_read.read_event_into(&mut self.buf)) {
                            Event::Start(b) if b.name().as_ref() == b"title" => {
                                page_title = Some(try_iter!(take_element_text(&mut self.xml_read,
                                                                           &mut self.buf,
                                                                           b"title")));
                            },
                            Event::Start(b) if b.name().as_ref() == b"ns" => {
                                page_ns_id = Some(try_iter!(try_iter!(
                                    take_element_text(&mut self.xml_read,
                                                      &mut self.buf,
                                                      b"ns")).parse::<i64>()));
                            },
                            Event::Start(b) if b.name().as_ref() == b"id" => {
                                page_id = Some(try_iter!(try_iter!(
                                    take_element_text(&mut self.xml_read,
                                                      &mut self.buf,
                                                      b"id")).parse::<u64>()));
                            },
                            Event::Start(b) if b.name().as_ref() == b"revision" => {
                                let mut revision_id: Option<u64> = None;
                                let mut revision_parent_id: Option<u64> = None;
                                let mut revision_timestamp: Option<DateTime<FixedOffset>> = None;
                                let mut revision_text: Option<String> = None;
                                let mut revision_sha1: Option<Sha1Hash> = None;
                                loop {
                                    match try_iter!(self.xml_read.read_event_into(&mut self.buf)) {
                                        // Skip <id> if revision_id is already Some(_).
                                        // This ignores <contributor><id>_</id></contributor>
                                        // in a hacky way without actually handling
                                        // an element stack properly.
                                        Event::Start(b) if b.name().as_ref() == b"id"
                                                           && revision_id.is_none() => {
                                            revision_id = Some(
                                                try_iter!(try_iter!(
                                                    take_element_text(&mut self.xml_read,
                                                                      &mut self.buf,
                                                                      b"id")).parse::<u64>()));
                                        },
                                        Event::Start(b) if b.name().as_ref() == b"parentid"
                                                           && revision_parent_id.is_none() => {
                                            revision_parent_id = Some(
                                                try_iter!(try_iter!(
                                                    take_element_text(&mut self.xml_read,
                                                                      &mut self.buf,
                                                                      b"parentid"))
                                                          .parse::<u64>()));
                                        },
                                        Event::Start(b) if b.name().as_ref() == b"timestamp"
                                                           && revision_timestamp.is_none() => {
                                            let s =
                                                try_iter!(
                                                    take_element_text(&mut self.xml_read,
                                                                      &mut self.buf,
                                                                      b"timestamp"));
                                            let dt = try_iter!(
                                                DateTime::<FixedOffset>::parse_from_rfc3339(&*s));
                                            revision_timestamp = Some(dt);
                                        },
                                        Event::Start(b) if b.name().as_ref() == b"text" => {
                                            revision_text = Some(
                                                try_iter!(take_element_text(&mut self.xml_read,
                                                                         &mut self.buf,
                                                                         b"text")));
                                        },
                                        Event::Start(b) if b.name().as_ref() == b"sha1" => {
                                            let s = try_iter!(take_element_text(&mut self.xml_read,
                                                                         &mut self.buf,
                                                                         b"sha1"));
                                            revision_sha1 = Some(
                                                try_iter!(Sha1Hash::from_base36_str(&*s)));
                                        },
                                        Event::End(b) if b.name().as_ref() == b"revision" => break,
                                        _ => {},
                                    }
                                } // end of loop over child nodes of <revision />

                                let revision_id = try_iter!(
                                    revision_id.ok_or(format_err!("No revision id \
                                                                   page_id={page_id:?} \
                                                                   page_title={page_title:?}")));
                                match (revision_text.as_ref(), revision_sha1.as_ref()) {
                                    (Some(text), Some(expected_sha1)) => {
                                        let calculated_sha1 =
                                            Sha1Hash::calculate_from_bytes(text.as_bytes());
                                        if *expected_sha1 != calculated_sha1 {
                                            tracing::warn!(
                                                %expected_sha1,
                                                %calculated_sha1,
                                                %revision_id,
                                                ?page_title,
                                                ?page_id,
                                                page_start_pos,
                                                file_path = %self.file_path.display(),
                                                "Dump page revision text SHA1 hash did not \
                                                 match expected.");
                                        }
                                    },
                                    (_, _) => {},
                                }
                                revision = Some(Revision {
                                    id: revision_id,
                                    parent_id: revision_parent_id,
                                    timestamp: revision_timestamp,
                                    categories:
                                        match revision_text {
                                            None => vec![],
                                            Some(ref text) =>
                                                wikitext::parse_categories(text.as_str()),
                                        },
                                    sha1: revision_sha1,
                                    // This moves revision_text, so do it last.
                                    text: revision_text,
                                });
                            },
                            Event::End(b) if b.name().as_ref() == b"page" => {
                                let page = Page {
                                    title: try_iter!(page_title.ok_or(
                                        format_err!("No page title"))),
                                    id: try_iter!(page_id.ok_or(
                                        format_err!("No page id"))),
                                    ns_id: try_iter!(page_ns_id.ok_or(
                                        format_err!("No page ns"))),
                                    revision: revision,
                                };
                                return Some(Ok(page));
                            },
                            _ => {},
                        } // match on Event in <page>
                    } // loop on Events in <page>
                }, // Handle <page>
                Event::Eof => return None,
                _ => {},
            } // match on Event at top level

            self.buf.clear();
        } // loop on Event at top level
    } // end of fn next
} // end of impl Iterator for FilePageIter

fn take_element_text<R: BufRead>(
    xml_read: &mut quick_xml::reader::Reader<R>,
    buf: &mut Vec<u8>,
    name: &[u8],
) -> Result<String> {
    let mut text = "".to_string();
    loop {
        match xml_read.read_event_into(buf)? {
            Event::Text(b) => text = b.unescape()?.into_owned(),
            Event::End(b) if b.name().as_ref() == name => break,
            _ => {},
        }
    }
    Ok(text)
}
