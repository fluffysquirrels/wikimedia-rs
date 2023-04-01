//! Read local copies of Wikimedia dump files.

use anyhow::format_err;
use clap::{
    builder::PossibleValue,
    ValueEnum,
};
use crate::{
    dump::types::*,
    Error,
    Result,
    UserRegex,
    util::{
        fmt::Bytes,
        IteratorExtSend,
    },
    wikitext,
};
use iterator_ext::IteratorExt;
use progress_streams::ProgressReader;
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
        atomic::{AtomicU64, Ordering},
    },
    str::FromStr,
};
use tracing::Level;
use valuable::Valuable;

struct FilePageIter<R: BufRead> {
    xml_read: quick_xml::reader::Reader<R>,
    buf: Vec<u8>,
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
        &[Self::Bzip2, Self::LZ4, Self::None]
    }

    fn to_possible_value(&self) -> Option<PossibleValue> {
        Some(match self {
            Self::Bzip2 => {
                PossibleValue::new("bzip2")
                              .alias("bz2")
                              .help("Use bzip2 compression. Alias 'bz2'.")
            }
            Self::LZ4 => PossibleValue::new("lz4").help("Use LZ4 compression."),
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

        let source_bytes_read = Arc::new(AtomicU64::new(0));

        let source_bytes_read2 = source_bytes_read.clone();
        let prog_read = ProgressReader::new(
            file_read,
            move |read_len| {
                source_bytes_read2.fetch_add(
                    read_len.try_into().expect("usize as u64"),
                    Ordering::SeqCst);
            });

        let file_bufread = BufReader::with_capacity(64 * 1024, prog_read);

        fn into_page_iter<T>(inner: T
        ) -> Box<dyn Iterator<Item = Result<Page>> + Send>
            where T: BufRead + Send + 'static
        {
            let xml_buf = Vec::<u8>::with_capacity(100_000);
            let xml_read = quick_xml::reader::Reader::from_reader(inner);
            let page_iter = FilePageIter {
                xml_read,
                buf: xml_buf,
            }.boxed_send();
            page_iter
        }

        let (uncompressed_bytes_read, pages_iter) = match self.compression {
            Compression::None => {
                (source_bytes_read.clone(), into_page_iter(file_bufread))
            },
            Compression::Bzip2 => {
                let bzip_decoder = bzip2::bufread::MultiBzDecoder::new(file_bufread);

                let uncompressed_bytes_read = Arc::new(AtomicU64::new(0));
                let uncompressed_bytes_read2 = uncompressed_bytes_read.clone();
                let uncompressed_prog_read = ProgressReader::new(
                    bzip_decoder,
                    move |read_len| {
                        uncompressed_bytes_read2.fetch_add(
                            read_len.try_into().expect("usize as u64"),
                            Ordering::SeqCst);
                    });

                let bzip_bufread = BufReader::with_capacity(64 * 1024, uncompressed_prog_read);
                (uncompressed_bytes_read, into_page_iter(bzip_bufread))
            },
            Compression::LZ4 => {
                let lz4_decoder = lz4_flex::frame::FrameDecoder::new(file_bufread);

                let uncompressed_bytes_read = Arc::new(AtomicU64::new(0));
                let uncompressed_bytes_read2 = uncompressed_bytes_read.clone();
                let uncompressed_prog_read = ProgressReader::new(
                    lz4_decoder,
                    move |read_len| {
                        uncompressed_bytes_read2.fetch_add(
                            read_len.try_into().expect("usize as u64"),
                            Ordering::SeqCst);
                    });

                let lz4_bufread = BufReader::with_capacity(64 * 1024, uncompressed_prog_read);
                (uncompressed_bytes_read, into_page_iter(lz4_bufread))
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
                    r#".*pages.*articles(-multistream)?[0-9]+\.xml-p[0-9]+p[0-9]+"#;

                let name_regex = match compression {
                    Compression::Bzip2 => lazy_regex!(FILE_RE_PREFIX, r#"\.bz2$"#),
                    Compression::LZ4 => lazy_regex!(FILE_RE_PREFIX, r#"\.lz4$"#),
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
            match try_iter!(self.xml_read.read_event_into(&mut self.buf)) {
                Event::Start(b) if b.name().as_ref() == b"page" => {
                    self.buf.clear();
                    let mut page_title: Option<String> = None;
                    let mut page_ns_id: Option<u64> = None;
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
                                                      b"ns")).parse::<u64>()));
                            },
                            Event::Start(b) if b.name().as_ref() == b"id" => {
                                page_id = Some(try_iter!(try_iter!(
                                    take_element_text(&mut self.xml_read,
                                                      &mut self.buf,
                                                      b"id")).parse::<u64>()));
                            },
                            Event::Start(b) if b.name().as_ref() == b"revision" => {
                                let mut revision_text: Option<String> = None;
                                let mut revision_id: Option<u64> = None;
                                loop {
                                    match try_iter!(self.xml_read.read_event_into(&mut self.buf)) {
                                        Event::Start(b) if b.name().as_ref() == b"id" => {
                                            revision_id = Some(
                                                try_iter!(try_iter!(
                                                    take_element_text(&mut self.xml_read,
                                                                      &mut self.buf,
                                                                      b"id")).parse::<u64>()));
                                        },
                                        Event::Start(b) if b.name().as_ref() == b"text" => {
                                            revision_text = Some(
                                                try_iter!(take_element_text(&mut self.xml_read,
                                                                         &mut self.buf,
                                                                         b"text")));
                                        },
                                        Event::End(b) if b.name().as_ref() == b"revision" => break,
                                        _ => {},
                                    }
                                }
                                revision = Some(Revision {
                                    id: try_iter!(revision_id.ok_or(
                                            format_err!("No revision id"))),
                                    categories:
                                        match revision_text {
                                            None => vec![],
                                            Some(ref text) =>
                                                wikitext::parse_categories(text.as_str()),
                                        },
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
