use crate::{
    args::{CommonArgs, DumpNameArg, JsonOutputArg, VersionSpecArg},
    // operations,
    Result,
};
use quick_xml::events::Event;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Get pages from an article dump.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,

//    #[clap(flatten)]
//    dump_name: DumpNameArg,
//
//    #[clap(flatten)]
//    version: VersionSpecArg,
//
//    /// The specific job name to get. By default information is returned about all jobs in the dump version.
//    #[arg(long = "job")]
//    job_name: Option<String>,

    #[clap(flatten)]
    json: JsonOutputArg,

    #[arg(long)]
    article_dump_file: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
pub struct Page {
    pub title: String,
    pub ns_id: u64,
    pub id: u64,
    pub revision: Option<Revision>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Revision {
    pub id: u64,
    pub text: Option<String>,
}

use std::{
    io::{BufRead, Read},
    iter::Iterator,
};

pub struct PageIter<R: BufRead> {
    xml_read: quick_xml::reader::Reader<R>,
    buf: Vec<u8>,
}

pub fn open_article_dump_file(file: &Path) -> Result<PageIter<impl BufRead>> {
    let file_read = std::fs::File::open(file)?;
    let file_bufread = std::io::BufReader::new(file_read);
    let bzip_decoder = bzip2::bufread::BzDecoder::new(file_bufread);
    let bzip_bufread = std::io::BufReader::new(bzip_decoder);
    let xml_read = quick_xml::reader::Reader::from_reader(bzip_bufread);

    let buf = Vec::<u8>::with_capacity(100_000);

    Ok(PageIter {
        xml_read,
        buf,
    })
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

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {

    let pages = open_article_dump_file(&*args.article_dump_file)?;
    for page in pages {
        serde_json::to_writer_pretty(&std::io::stdout(), &(page?))?;
        println!();
    }

    Ok(())
}

fn take_element_text<R: std::io::BufRead>(
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
