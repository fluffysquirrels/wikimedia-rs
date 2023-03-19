//! Read a Wikimedia article dump archive.

use anyhow::format_err;
use crate::{
    args::{CommonArgs, DumpFileSpecArgs, DumpNameArg, JobNameArg},
    Error,
    Result,
    types::{FileMetadata, Version},
    util::{IteratorExtLocal}
};
use iterator_ext::IteratorExt;
use quick_xml::events::Event;
use serde::Serialize;
use std::{
    borrow::Cow,
    fs::DirEntry,
    io::{BufRead, BufReader, Error as IoError},
    iter::Iterator,
    path::{Path, PathBuf},
};
use tracing::Level;

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
}

struct PageIter<R: BufRead> {
    xml_read: quick_xml::reader::Reader<R>,
    buf: Vec<u8>,
}

pub struct OpenFileOptions {
    pub path: PathBuf,
    pub compression: Compression,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum Compression {
    Bzip2,
    LZ4,
    None,
}

/// Used to exit early on Err in an Iterator<Item = Result<T>>::next() method.
macro_rules! try_iter {
    ($expr:expr $(,)?) => {
        match $expr {
            Ok(val) => val,
            Err(err) => {
                return Some(Err(err.into()));
            }
        }
    };
}

pub fn job_file_path(
    out_dir: &Path,
    dump_name: &DumpNameArg,
    version: &Version,
    job_name: &JobNameArg,
    file_meta: &FileMetadata
) -> Result<PathBuf> {
    let file_name = file_meta.url.split('/').last()
                                 .ok_or_else(|| format_err!("file_meta.url was empty url='{url}'",
                                                            url = file_meta.url))?;
    let path = job_path(out_dir, dump_name, version, job_name)
                   .join(file_name);
    Ok(path)
}

pub fn job_path(
    out_dir: &Path,
    dump_name: &DumpNameArg,
    version: &Version,
    job_name: &JobNameArg,
) -> PathBuf {
    out_dir.join(format!("{dump_name}/{version}/{job_name}",
                         dump_name = &*dump_name.value,
                         version = version.0,
                         job_name = &*job_name.value))
}

pub fn open_dump_spec(
    common: &CommonArgs,
    file_spec: &DumpFileSpecArgs,
) -> Result<Box<dyn Iterator<Item = Result<Page>>>>
{
    let dump_file = file_spec.dump_file.as_ref().map(|p: &PathBuf| p.as_path());
    let job_dir = file_spec.job_dir.as_ref().map(|p: &PathBuf| p.as_path());
    let pages: Box<dyn Iterator<Item = Result<Page>>> =
        match (dump_file, job_dir) {
            (Some(_), Some(_)) => return Err(
                Error::msg("You supplied both --dump-file and --job-dir, but should only \
                            supply one of these")),
            (Some(file), None) => {
                let open_options = OpenFileOptions {
                    path: file.to_path_buf(),
                    compression: file_spec.compression,
                };
                open_dump_file(open_options)?.boxed_local()
            },
            (None, Some(dir)) => {
                open_dump_job_by_dir(dir, file_spec.compression)?
                    .boxed_local()
            }
            (None, None) => {
                open_dump_job(
                    &*common.out_dir, &file_spec.dump_name, &file_spec.version.value,
                    &file_spec.job_name, file_spec.compression
                )?.boxed_local()
            },
        };

    let pages = match file_spec.count {
        None => pages,
        Some(count) => pages.take(count).boxed_local(),
    };

    Ok(pages)
}

pub fn open_dump_file(
    options: OpenFileOptions,
) -> Result<Box<dyn Iterator<Item = Result<Page>>>>
{
    let file_read = std::fs::File::open(options.path)?;
    let file_bufread = BufReader::with_capacity(64 * 1024, file_read);

    fn into_page_iter<T>(inner: T) -> Result<Box<dyn Iterator<Item = Result<Page>>>>
        where T: BufRead + 'static
    {
        let xml_buf = Vec::<u8>::with_capacity(100_000);
        let xml_read = quick_xml::reader::Reader::from_reader(inner);
        let page_iter = PageIter {
            xml_read,
            buf: xml_buf,
        }.boxed_local();
        Ok(page_iter)
    }

    match options.compression {
        Compression::None => into_page_iter(file_bufread),
        Compression::Bzip2 => {
            let bzip_decoder = bzip2::bufread::MultiBzDecoder::new(file_bufread);
            let bzip_bufread = BufReader::with_capacity(64 * 1024, bzip_decoder);
            into_page_iter(bzip_bufread)
        },
        Compression::LZ4 => {
            let lz4_decoder = lz4_flex::frame::FrameDecoder::new(file_bufread);
            let lz4_bufread = BufReader::with_capacity(64 * 1024, lz4_decoder);
            into_page_iter(lz4_bufread)
        }
    }
}

pub fn open_dump_job(
    out_dir: &Path,
    dump_name: &DumpNameArg,
    version: &Version,
    job_name: &JobNameArg,
    compression: Compression,
) -> Result<impl Iterator<Item = Result<Page>>>
{
    let job_path: PathBuf = job_path(out_dir, dump_name, version, job_name);
    open_dump_job_by_dir(&*job_path, compression)
}

pub fn open_dump_job_by_dir(
    job_path: &Path,
    compression: Compression,
) -> Result<impl Iterator<Item = Result<Page>>>
{
    let mut file_paths =
        std::fs::read_dir(job_path)?
            .map_err(|e: IoError| -> Error {
                     e.into()
            })
            .try_filter_map(|dir_entry: DirEntry| -> Result<Option<PathBuf>> {
                if !dir_entry.file_type()?.is_file() {
                    return Ok(None);
                }
                let name_regex = match compression {
                    Compression::Bzip2 => lazy_regex!("pages.*articles.*xml.*\\.bz2$"),
                    Compression::LZ4 => lazy_regex!("pages.*articles.*xml.*\\.lz4$"),
                    Compression::None => lazy_regex!("pages.*articles.*xml.*$"),
                };
                let name = dir_entry.file_name().to_string_lossy().into_owned();
                if name_regex.is_match(&*name) {
                    Ok(Some(dir_entry.path()))
                } else {
                    Ok(None)
                }
            }).try_collect::<Vec<PathBuf>>()?;
    file_paths.sort_by(|a, b| natord::compare(&*a.to_string_lossy(),
                                              &*b.to_string_lossy()));

    let files_total_len: u64 =
        file_paths.iter().map(|path| path.metadata()
                                         .map_err(|e: std::io::Error| -> Error { e.into() })
                                         .map(|meta| meta.len()))
                         .try_fold(0_u64, |curr, len| -> Result<u64>
                                          { Ok(curr + len?) })?;

    if tracing::enabled!(Level::DEBUG) {
        tracing::debug!(files_total_len,
                        file_count = file_paths.len(),
                        file_paths = ?file_paths.iter().map(|p| p.to_string_lossy())
                                                .collect::<Vec<Cow<'_, str>>>(),
                        "article_dump::open_article_dump_job() file paths");
    }

    let pages = file_paths.into_iter()
        .map(move |file_path: PathBuf| { // -> Result<impl Iterator<Item = Result<Page>>>
            let options = OpenFileOptions {
                path: file_path,
                compression: compression,
            };
            open_dump_file(options)
        })
        .try_flatten_results(); // : impl Iterator<Item = Result<Page>>

    Ok(pages)
}

impl<R: BufRead> Iterator for PageIter<R> {
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
                                        anyhow::Error::msg("No revision id"))),
                                    text: revision_text,
                                });
                            },
                            Event::End(b) if b.name().as_ref() == b"page" => {
                                let page = Page {
                                    title: try_iter!(page_title.ok_or(
                                        anyhow::Error::msg("No page title"))),
                                    id: try_iter!(page_id.ok_or(
                                        anyhow::Error::msg("No page id"))),
                                    ns_id: try_iter!(page_ns_id.ok_or(
                                        anyhow::Error::msg("No page ns"))),
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
} // end of impl Iterator for PageIter

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
