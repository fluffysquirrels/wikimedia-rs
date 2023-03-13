use crate::{
    args::{CommonArgs, DumpNameArg, JsonOutputArg, VersionSpecArg},
    // operations,
    Result,
};
use quick_xml::events::Event;
use serde::Serialize;
use std::path::PathBuf;

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
    pub text: String,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let file_read = std::fs::File::open(&*args.article_dump_file)?;
    let file_bufread = std::io::BufReader::new(file_read);
    let bzip_decoder = bzip2::bufread::BzDecoder::new(file_bufread);
    let bzip_bufread = std::io::BufReader::new(bzip_decoder);
    let mut xml_read = quick_xml::reader::Reader::from_reader(bzip_bufread);

    let mut buf = Vec::<u8>::with_capacity(100_000);

    loop {
        match xml_read.read_event_into(&mut buf)? {
            Event::Start(b) if b.name().as_ref() == b"page" => {
                buf.clear();
                let mut page_title: Option<String> = None;
                let mut page_ns_id: Option<u64> = None;
                let mut page_id: Option<u64> = None;
                let mut revision: Option<Revision> = None;
                loop {
                    match xml_read.read_event_into(&mut buf)? {
                        Event::Start(b) if b.name().as_ref() == b"title" => {
                            page_title = Some(take_element_text(&mut xml_read, &mut buf,
                                                                b"title")?);
                        },
                        Event::Start(b) if b.name().as_ref() == b"ns" => {
                            page_ns_id = Some(take_element_text(&mut xml_read, &mut buf,
                                                                b"ns")?.parse::<u64>()?);
                        },
                        Event::Start(b) if b.name().as_ref() == b"id" => {
                            page_id = Some(take_element_text(&mut xml_read, &mut buf,
                                                             b"id")?.parse::<u64>()?);
                        },
                        Event::Start(b) if b.name().as_ref() == b"revision" => {
                            let mut revision_text: Option<String> = None;
                            let mut revision_id: Option<u64> = None;
                            loop {
                                match xml_read.read_event_into(&mut buf)? {
                                    Event::Start(b) if b.name().as_ref() == b"id" => {
                                        revision_id = Some(
                                            take_element_text(&mut xml_read,
                                                              &mut buf,
                                                              b"id")?.parse::<u64>()?);
                                    },
                                    Event::Start(b) if b.name().as_ref() == b"text" => {
                                        revision_text = Some(
                                            take_element_text(&mut xml_read,
                                                              &mut buf,
                                                              b"text")?);
                                    },
                                    Event::End(b) if b.name().as_ref() == b"revision" => break,
                                    _ => {},
                                }
                            }
                            revision = Some(Revision {
                                text: revision_text.ok_or(anyhow::Error::msg("No revision text"))?,
                                id: revision_id.ok_or(anyhow::Error::msg("No revision id"))?,
                            });
                        },
                        Event::End(b) if b.name().as_ref() == b"page" => {
                            let page = Page {
                                title: page_title.ok_or(anyhow::Error::msg("No page title"))?,
                                id: page_id.ok_or(anyhow::Error::msg("No page id"))?,
                                ns_id: page_ns_id.ok_or(anyhow::Error::msg("No page ns"))?,
                                revision: revision,
                            };
                            handle_page(&page)?;
                            break;
                        },
                        _ => {},
                    }
                }
            },
            Event::Eof => break,
            _ => {},
        }

        buf.clear();
    }

    Ok(())
}

fn handle_page(page: &Page) -> Result<()> {
    serde_json::to_writer_pretty(&std::io::stdout(), &page)?;
    println!();
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
            Event::Text(b) => text = String::from_utf8_lossy(&*b).into_owned(),
            Event::End(b) if b.name().as_ref() == name => break,
            _ => {},
        }
    }
    Ok(text)
}
