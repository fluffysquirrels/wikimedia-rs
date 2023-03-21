//! Read local copies of Wikimedia dump files.

use anyhow::{bail, format_err};
use clap::{
    builder::PossibleValue,
    ValueEnum,
};
use crate::{
    args::{CommonArgs, DumpFileSpecArgs, DumpNameArg, JobNameArg},
    dump::types::*,
    Error,
    Result,
    UserRegex,
    util::{
        fmt::Bytes,
        IteratorExtLocal,
    },
    wikitext,
};
use iterator_ext::IteratorExt;
use quick_xml::events::Event;
use std::{
    borrow::Cow,
    fmt::{self, Display},
    fs::DirEntry,
    io::{BufRead, BufReader, Error as IoError, Seek},
    iter::Iterator,
    path::{Path, PathBuf},
    result::Result as StdResult,
    str::FromStr,
};
use tracing::Level;
use valuable::Valuable;

struct PageIter<R: BufRead> {
    xml_read: quick_xml::reader::Reader<R>,
    buf: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
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
    dump_name: &DumpNameArg,
    version: &Version,
    job_name: &JobNameArg,
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
    dump_name: &DumpNameArg,
    version: &Version,
    job_name: &JobNameArg,
) -> PathBuf {
    out_dir.join(format!("dumps/{dump_name}/{version}/{job_name}",
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
            (Some(_), Some(_)) => bail!("You supplied both --dump-file and --job-dir, \
                                         but should only supply one of these"),
            (Some(file), None) => {
                open_dump_file(file, file_spec.compression, file_spec.seek.clone())?.boxed_local()
            },
            (None, Some(dir)) => {
                open_dump_job_by_dir(dir, file_spec.compression,
                                     file_spec.file_name_regex.value.clone())?
                    .boxed_local()
            }
            (None, None) => {
                match (file_spec.dump_name.as_ref(),
                       file_spec.version.as_ref(),
                       file_spec.job_name.as_ref()) {
                    (Some(dump), Some(version), Some(job)) =>
                        open_dump_job(
                            &*common.out_dir, dump, version,
                            job, file_spec.compression,
                            file_spec.file_name_regex.value.clone()
                        )?.boxed_local(),
                    _ => bail!("You must supply one of these 3 valid argument sets:\n\
                                1. `--dump-file`\n\
                                2. `--job-dir'\n\
                                3. `--dump`, `--version`, and `--job`"),
                }
            },
        };

    let pages = match file_spec.count {
        None => pages,
        Some(count) => pages.take(count).boxed_local(),
    };

    Ok(pages)
}

pub fn open_dump_file(
    path: &Path,
    compression: Compression,
    seek: Option<u64>
) -> Result<Box<dyn Iterator<Item = Result<Page>>>>
{
    tracing::debug!(path = %path.display(),
                    ?compression,
                    ?seek,
                    "dump::open_dump_file");

    let mut file_read = std::fs::File::open(path)?;
    if let Some(offset) = seek {
        let _ = file_read.seek(std::io::SeekFrom::Start(offset))?;
    }

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

    match compression {
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
    user_file_name_regex: Option<UserRegex>,
) -> Result<impl Iterator<Item = Result<Page>>>
{
    let job_path: PathBuf = job_path(out_dir, dump_name, version, job_name);
    open_dump_job_by_dir(&*job_path, compression, user_file_name_regex)
}

pub fn open_dump_job_by_dir(
    job_path: &Path,
    compression: Compression,
    user_file_name_regex: Option<UserRegex>,
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
                macro_rules! file_re_prefix {
                    () => { r#".*pages.*articles(-multistream)?[0-9]+\.xml-p[0-9]+p[0-9]+"# };
                }
                let name_regex = match compression {
                    Compression::Bzip2 => lazy_regex!(file_re_prefix!(), r#"\.bz2$"#),
                    Compression::LZ4 => lazy_regex!(file_re_prefix!(), r#"\.lz4$"#),
                    Compression::None => lazy_regex!(file_re_prefix!(), r#"$"#),
                };
                let name = dir_entry.file_name().to_string_lossy().into_owned();
                if name_regex.is_match(&*name)
                    && user_file_name_regex.as_ref().map_or(true, |re| re.0.is_match(&*name))
                {
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
        tracing::debug!(files_total_len = Bytes(files_total_len).as_value(),
                        file_count = file_paths.len(),
                        file_paths = ?file_paths.iter().map(|p| p.to_string_lossy())
                                                .collect::<Vec<Cow<'_, str>>>(),
                        "dump::open_dump_job() file paths");
    }

    let pages = file_paths.into_iter()
        .map(move |file_path: PathBuf| { // -> Result<impl Iterator<Item = Result<Page>>>
            open_dump_file(&*file_path, compression, None /* seek */)
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
