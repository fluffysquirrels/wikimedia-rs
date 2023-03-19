//! Read a Wikimedia article dump archive.

use anyhow::format_err;
use crate::{
    args::{DumpNameArg, JobNameArg},
    Error,
    Result,
    types::{FileMetadata, Version},
};
use iterator_ext::IteratorExt;
use quick_xml::events::Event;
use serde::Serialize;
use std::{
    borrow::Cow,
    fs::DirEntry,
    io::{BufRead, Error as IoError},
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

/// Used to exit early on Err in an Iterator::next() method.
macro_rules! early {
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

pub fn open_article_dump_file(file_path: &Path
) -> Result<impl Iterator<Item = Result<Page>>>
{
    let file_read = std::fs::File::open(file_path)?;
    let file_bufread = std::io::BufReader::new(file_read);
    let bzip_decoder = bzip2::bufread::MultiBzDecoder::new(file_bufread);
    let bzip_bufread = std::io::BufReader::new(bzip_decoder);
    let xml_read = quick_xml::reader::Reader::from_reader(bzip_bufread);

    let buf = Vec::<u8>::with_capacity(1_000_000);

    Ok(PageIter {
        xml_read,
        buf,
    })
}

pub fn open_article_dump_job(
    out_dir: &Path,
    dump_name: &DumpNameArg,
    version: &Version,
    job_name: &JobNameArg
) -> Result<impl Iterator<Item = Result<Page>>>
{
    let job_path: PathBuf = job_path(out_dir, dump_name, version, job_name);
    let mut file_paths =
        std::fs::read_dir(&*job_path)?
            .map_err(|e: IoError| -> Error {
                     e.into()
            })
            .try_filter_map(|dir_entry: DirEntry| -> Result<Option<PathBuf>> {
                if !dir_entry.file_type()?.is_file() {
                    return Ok(None);
                }
                let name = dir_entry.file_name().to_string_lossy().into_owned();
                if lazy_regex!("pages.*articles.*xml.*\\.bz2$").is_match(&*name) {
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
        .map(|file_path: PathBuf| // -> Result<impl Iterator<Item = Result<Page>>>
             open_article_dump_file(&*file_path))
        .try_flatten_results(); // : impl Iterator<Item = Result<Page>>
    Ok(pages)
}

impl<R: BufRead> Iterator for PageIter<R> {
    type Item = Result<Page>;

    fn next(&mut self) -> Option<Result<Page>> {
        loop {
            match early!(self.xml_read.read_event_into(&mut self.buf)) {
                Event::Start(b) if b.name().as_ref() == b"page" => {
                    self.buf.clear();
                    let mut page_title: Option<String> = None;
                    let mut page_ns_id: Option<u64> = None;
                    let mut page_id: Option<u64> = None;
                    let mut revision: Option<Revision> = None;
                    loop {
                        match early!(self.xml_read.read_event_into(&mut self.buf)) {
                            Event::Start(b) if b.name().as_ref() == b"title" => {
                                page_title = Some(early!(take_element_text(&mut self.xml_read,
                                                                           &mut self.buf,
                                                                           b"title")));
                            },
                            Event::Start(b) if b.name().as_ref() == b"ns" => {
                                page_ns_id = Some(early!(early!(
                                    take_element_text(&mut self.xml_read,
                                                      &mut self.buf,
                                                      b"ns")).parse::<u64>()));
                            },
                            Event::Start(b) if b.name().as_ref() == b"id" => {
                                page_id = Some(early!(early!(
                                    take_element_text(&mut self.xml_read,
                                                      &mut self.buf,
                                                      b"id")).parse::<u64>()));
                            },
                            Event::Start(b) if b.name().as_ref() == b"revision" => {
                                let mut revision_text: Option<String> = None;
                                let mut revision_id: Option<u64> = None;
                                loop {
                                    match early!(self.xml_read.read_event_into(&mut self.buf)) {
                                        Event::Start(b) if b.name().as_ref() == b"id" => {
                                            revision_id = Some(
                                                early!(early!(
                                                    take_element_text(&mut self.xml_read,
                                                                      &mut self.buf,
                                                                      b"id")).parse::<u64>()));
                                        },
                                        Event::Start(b) if b.name().as_ref() == b"text" => {
                                            revision_text = Some(
                                                early!(take_element_text(&mut self.xml_read,
                                                                         &mut self.buf,
                                                                         b"text")));
                                        },
                                        Event::End(b) if b.name().as_ref() == b"revision" => break,
                                        _ => {},
                                    }
                                }
                                revision = Some(Revision {
                                    id: early!(revision_id.ok_or(
                                        anyhow::Error::msg("No revision id"))),
                                    text: revision_text,
                                });
                            },
                            Event::End(b) if b.name().as_ref() == b"page" => {
                                let page = Page {
                                    title: early!(page_title.ok_or(
                                        anyhow::Error::msg("No page title"))),
                                    id: early!(page_id.ok_or(
                                        anyhow::Error::msg("No page id"))),
                                    ns_id: early!(page_ns_id.ok_or(
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
