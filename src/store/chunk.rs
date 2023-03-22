//! MediaWiki pages are stored in chunk files, implemented in this module.
//!
//! Currently the chunk files contain about 10 MB of pages serialised as a flatbuffers object.
//!
//! Work is in progress to switch to capnproto instead, because flatbuffers is only safe to use if
//! the object tree is verified before use, and this takes quite a long time (50-100ms per chunk
//! file).

use anyhow::{bail, ensure, format_err};
use crate::{
    dump,
    fbs::wikimedia as wm,
    Result,
    TempDir,
    util::{
        fmt::Bytes,
        IteratorExtSend,
    },
};
use crossbeam_utils::CachePadded;
use flatbuffers::{self as fb, FlatBufferBuilder, WIPOffset};
use serde::Serialize;
use std::{
    cmp,
    fmt::{self, Debug, Display},
    fs,
    io::Write,
    ops::Deref,
    path::PathBuf,
    result::Result as StdResult,
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};
use valuable::Valuable;

pub struct Store {
    next_chunk_id: CachePadded<AtomicU64>,
    opts: Options,
    temp_dir: TempDir,
}

pub struct Options {
    pub path: PathBuf,
    pub max_chunk_len: u64,
}

#[derive(Clone, Copy, Debug)]
pub struct StorePageId {
    chunk_id: ChunkId,
    page_chunk_idx: PageChunkIndex,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Valuable)]
pub struct ChunkId(u64);

#[derive(Clone, Copy, Debug)]
pub struct PageChunkIndex(u64);

#[derive(Debug)]
pub struct MappedChunk {
    id: ChunkId,
    mmap: memmap2::Mmap,
    path: PathBuf,
}

#[derive(Debug)]
pub struct MappedPage {
    chunk: MappedChunk,
    loc: usize,
}

#[derive(Clone, Debug, Serialize, Valuable)]
pub struct ChunkMeta {
    pub bytes_len: Bytes,
    pub id: ChunkId,
    pub pages_len: u64,
    pub path: PathBuf,
}

pub struct Builder<'fbb, 'store> {
    chunk_id: ChunkId,
    fbb: FlatBufferBuilder<'fbb>,
    out_path: PathBuf,
    temp_path: PathBuf,
    page_fbs: Vec::<WIPOffset<wm::Page<'fbb>>>,

    _store: &'store Store,
}

pub const MAX_LEN_DEFAULT: u64 = 10_000_000; // 10 MB.

impl FromStr for ChunkId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        Ok(ChunkId(s.parse::<u64>()?))
    }
}

impl Debug for ChunkId {
    fn fmt(&self,
           f: &mut fmt::Formatter
    ) -> StdResult<(), fmt::Error> {
        let ChunkId(chunk_id) = self;
        write!(f, "ChunkId(dec = {chunk_id}, hex = {chunk_id:#x})")
    }
}

impl Display for ChunkId {
    fn fmt(&self,
           f: &mut fmt::Formatter
    ) -> StdResult<(), fmt::Error> {
        let ChunkId(chunk_id) = self;
        write!(f, "{chunk_id}")
    }
}

impl Display for PageChunkIndex {
    fn fmt(&self,
           f: &mut fmt::Formatter
    ) -> StdResult<(), fmt::Error> {
        let PageChunkIndex(idx) = self;
        write!(f, "{idx}")
    }
}

impl FromStr for StorePageId {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        let segments = s.split('.').map(|s| s.to_string()).collect::<Vec<String>>();
        if segments.len() != 2 {
            bail!("StorePageId::from_str expects 2 integers separated by a '.'");
        }

        Ok(StorePageId {
            chunk_id: ChunkId(segments[0].parse::<u64>()?),
            page_chunk_idx: PageChunkIndex(segments[1].parse::<u64>()?),
        })
    }
}

impl Display for StorePageId {
    fn fmt(&self,
           f: &mut fmt::Formatter
    ) -> StdResult<(), fmt::Error> {
        let StorePageId { chunk_id, page_chunk_idx } = self;
        write!(f, "{chunk_id}.{page_chunk_idx}")
    }
}

impl TryFrom<&[u8]> for StorePageId {
    type Error = anyhow::Error;

    fn try_from(b: &[u8]) -> Result<StorePageId> {
        if b.len() != 16 {
            bail!("StorePageId::try_from: input.len() != 16");
        }

        Ok(StorePageId{
            chunk_id: ChunkId(
                u64::from_be_bytes(b[0..8].try_into()
                                          .expect("already checked b.len()"))),
            page_chunk_idx: PageChunkIndex(
                u64::from_be_bytes(b[8..16].try_into()
                                           .expect("already checked b.len()"))),
        })
    }
}

impl StorePageId {
    pub fn to_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[0..8].copy_from_slice(self.chunk_id.0.to_be_bytes().as_ref());
        out[8..16].copy_from_slice(self.page_chunk_idx.0.to_be_bytes().as_ref());
        out
    }
}

impl Options {
    pub fn build(self) -> Result<Store> {
        let chunk_iter_span = tracing::trace_span!("ChunkStore enumerating existing chunks.",
                                                   chunk_count = tracing::field::Empty,
                                                   max_existing_chunk_id = tracing::field::Empty)
                                      .entered();

        struct ChunkStats {
            count: usize,
            max_id: Option<ChunkId>,
        }

        let chunk_stats: ChunkStats =
            Store::chunk_id_iter_from_opts(&self)
                .try_fold(ChunkStats { count: 0, max_id: None }, // inital state
                          |s: ChunkStats, next: Result<ChunkId>|
                          -> Result<ChunkStats> {
                              let next = next?;
                              Ok(ChunkStats {
                                  count: s.count + 1,
                                  max_id: match s.max_id {
                                      None => Some(next),
                                      Some(prev) => Some(cmp::max(prev, next)),
                                  }
                              })
                          })?;
        chunk_iter_span.record("chunk_count", chunk_stats.count);
        chunk_iter_span.record("max_existing_chunk_id",
                               tracing::field::debug(chunk_stats.max_id));
        let _ = chunk_iter_span.exit();

        let next_chunk_id = match chunk_stats.max_id {
            Some(ChunkId(id)) => ChunkId(id + 1),
            None => ChunkId(0),
        };

        tracing::debug!(%next_chunk_id,
                        "store::chunk::ChunkStore::new() loaded");

        Ok(Store {
            next_chunk_id: CachePadded::new(AtomicU64::new(next_chunk_id.0)),
            temp_dir: TempDir::create(&*self.path, /* keep: */ false)?,

            // This moves self into Store, so do that last.
            opts: self,
        })
    }
}

impl<'fbb, 'store> Builder<'fbb, 'store> {
    pub fn push(&mut self, page: &dump::Page) -> Result<StorePageId> {
        let page_title = self.fbb.create_string(&*page.title);
        let revision_fb = page.revision.as_ref().map(|revision| {
            let revision_text_fb = revision.text.as_ref().map(|text| {
                self.fbb.create_string(&*text)
            });
            let mut revision_b = wm::RevisionBuilder::new(&mut self.fbb);
            revision_b.add_id(revision.id);
            if let Some(fb) = revision_text_fb {
                revision_b.add_text(fb);
            }
            revision_b.finish()
        });
        let mut page_b = wm::PageBuilder::new(&mut self.fbb);
        if let Some(revision_fb) = revision_fb {
            page_b.add_revision(revision_fb);
        }
        page_b.add_ns_id(page.ns_id);
        page_b.add_id(page.id);
        page_b.add_title(page_title);
        let page_fb: WIPOffset<wm::Page> = page_b.finish();
        self.page_fbs.push(page_fb);

        let idx = (self.page_fbs.len() - 1).try_into().expect("usize as u64");

        Ok(StorePageId {
            chunk_id: self.chunk_id,
            page_chunk_idx: PageChunkIndex(idx),
        })
    }

    pub fn write_all(mut self) -> Result<ChunkMeta> {
        let pages_len = self.page_fbs.len();

        let pages_vec = self.fbb.create_vector_from_iter(self.page_fbs.into_iter());

        let mut chunk_fbb = wm::ChunkBuilder::new(&mut self.fbb);
        chunk_fbb.add_pages(pages_vec);
        let chunk_fb = chunk_fbb.finish();

        wm::finish_size_prefixed_chunk_buffer(&mut self.fbb, chunk_fb);
        let fb_out = self.fbb.finished_data();
        let bytes_len = fb_out.len();

        let mut temp_file = fs::File::create(&*self.temp_path)?;
        temp_file.write_all(fb_out)?;
        drop(self.fbb);
        temp_file.flush()?;
        temp_file.sync_all()?;
        drop(temp_file);

        fs::rename(&*self.temp_path, &*self.out_path)?;

        Ok(ChunkMeta {
            bytes_len: Bytes(bytes_len.try_into().expect("Convert usize to u64")),
            id: self.chunk_id,
            pages_len: pages_len.try_into().expect("Convert usize to u64"),
            path: self.out_path,
        })
    }

    pub fn curr_bytes_len(&self) -> u64 {
        u64::try_from(self.fbb.unfinished_data().len()).expect("usize to u64")
    }
}

impl Store {
    pub fn clear(&mut self) -> Result<()> {
        let chunks_path = &*self.opts.path;
        if chunks_path.try_exists()? {
            std::fs::remove_dir_all(&*chunks_path)?;
        }

        *self.next_chunk_id.get_mut() = 0;
        Ok(())
    }

    pub fn chunk_builder<'fbb, 'store>(&'store self) -> Result<Builder<'fbb, 'store>> {
        let chunk_id = self.next_chunk_id();

        let out_path = self.chunk_path(chunk_id);
        let temp_path = self.temp_dir.path()?.join(
            out_path.file_name().expect("Chunk file name"));

        fs::create_dir_all(out_path.parent().expect("parent of out_path"))?;
        fs::create_dir_all(temp_path.parent().expect("parent of temp_path"))?;

        Ok(Builder {
            chunk_id: self.next_chunk_id(),
            fbb: FlatBufferBuilder::with_capacity(
                usize::try_from(
                    self.opts.max_chunk_len + (self.opts.max_chunk_len / 10) + 1_000_000)
                    .expect("usize from u64")),
            out_path,
            temp_path,
            page_fbs: Vec::<WIPOffset<wm::Page>>::with_capacity(
                usize::try_from(self.opts.max_chunk_len / 1_000)
                    .expect("usize from u64")),

            _store: self,
        })
    }

    pub fn get_page_by_store_id(&self, id: StorePageId) -> Result<Option<MappedPage>> {
        let chunk: MappedChunk = try2!(self.map_chunk(id.chunk_id));
        let page: MappedPage = chunk.get_mapped_page(id.page_chunk_idx)?;
        Ok(Some(page))
    }

    pub fn chunk_id_vec(&self) -> Result<Vec<ChunkId>> {
        let mut vec: Vec<ChunkId> = self.chunk_id_iter().try_collect()?;
        vec.sort();
        Ok(vec)
    }

    pub fn chunk_id_iter(&self) -> impl Iterator<Item = Result<ChunkId>> {
        Self::chunk_id_iter_from_opts(&self.opts)
    }

    fn chunk_id_iter_from_opts(opts: &Options) -> impl Iterator<Item = Result<ChunkId>> + Send {
        // This closure is to specify the return type explicitly.
        // Without this the return type is inferred from the first return
        // and doesn't include the `dyn`, so the subsequent ones fail to type check.
        (|| -> Box<dyn Iterator<Item = Result<ChunkId>> + Send> {
            let read_dir = match fs::read_dir(&*opts.path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound =>
                    return std::iter::empty().boxed_send(),
                Err(e) => return std::iter::once(Err(e.into())).boxed_send(),
                Ok(d) => d,
            };
            read_dir.flat_map(|item: StdResult<fs::DirEntry, _>| -> Option<Result<ChunkId>>{
                let item = match item {
                    Ok(item) => item,
                    Err(e) => return Some(Err(e.into())),
                };
                let name = match item.file_name().into_string() {
                    Ok(name) => name,
                    Err(oss) => return Some(Err(
                        format_err!("Cannot convert item name into String: '{oss}'",
                                    oss = oss.to_string_lossy().to_string()))),
                };

                let Some(captures) = lazy_regex!("^articles-([0-9a-f]{16}).fbd$").captures(&*name)
                else {
                    return None;
                };

                let id_hex = captures.get(1).expect("regex capture 1 is None").as_str();
                let id = u64::from_str_radix(id_hex, 16)
                             .expect("parse u64 from prevalidated hex String");
                Some(Ok(ChunkId(id)))
            }).boxed_send()
        })()
    }

    pub fn get_chunk_meta_by_chunk_id(&self, chunk_id: ChunkId) -> Result<Option<ChunkMeta>> {
        let chunk = try2!(self.map_chunk(chunk_id));
        Ok(Some(chunk.meta()))
    }

    pub fn map_chunk(&self, id: ChunkId) -> Result<Option<MappedChunk>> {
        let path = self.chunk_path(id);

        let file = match fs::File::open(&*path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
            Ok(f) => f,
        };
        let mmap = unsafe {
            memmap2::MmapOptions::new()
                .map(&file)?
        };

        let chunk = MappedChunk {
            id,
            mmap,
            path: path.clone(),
        };

        let bytes = chunk.bytes();

        // This load runs the flatbuffers verifier, subsequent loads will not.
        let _chunk_fb =
            tracing::trace_span!("chunk::Store::map_chunk() running verifier.",
                                 chunk_id = ?id,
                                 path = %path.display())
                .in_scope(|| {
                    wm::size_prefixed_root_as_chunk(bytes)
                })?;

        Ok(Some(chunk))
    }

    fn chunk_path(&self, chunk_id: ChunkId) -> PathBuf {
        self.opts.path.join(format!("articles-{id:016x}.fbd", id = chunk_id.0))
    }

    fn next_chunk_id(&self) -> ChunkId {
        let next = self.next_chunk_id.fetch_add(1, Ordering::SeqCst);
        ChunkId(next)
    }
}


impl MappedPage {
    pub fn borrow<'a>(&'a self) -> wm::Page<'a> {
        let bytes = self.chunk.bytes();

        unsafe {
            let table = fb::Table::new(bytes, self.loc);
            wm::Page::init_from_table(table)
        }
    }
}

impl MappedChunk {
    fn get_page<'a, 'b>(&'a self, idx: PageChunkIndex
    ) -> Result<wm::Page<'b>>
        where 'a: 'b
    {
        let chunk_fb = unsafe { self.chunk_fb_unchecked() };
        let idx = idx.0 as usize;
        ensure!(idx < chunk_fb.pages().len(),
                "MappedChunk::get_page index out of bounds.");

        let page_fb = chunk_fb.pages().get(idx);
        Ok(page_fb)
    }

    fn get_mapped_page(self, idx: PageChunkIndex) -> Result<MappedPage> {
        let page_fb = self.get_page(idx)?;
        let loc = page_fb._tab.loc();
        drop(page_fb);

        Ok(MappedPage {
            chunk: self,
            loc,
        })
    }

    pub fn pages_iter(&self) -> impl Iterator<Item = wm::Page<'_>> {
        let chunk_fb = unsafe { self.chunk_fb_unchecked() };
        chunk_fb.pages().iter()
    }

    fn meta(&self) -> ChunkMeta {
        let chunk_fb = unsafe { self.chunk_fb_unchecked() };

        ChunkMeta {
            bytes_len: Bytes(self.mmap.len().try_into().expect("usize as u64")),
            id: self.id,
            pages_len: chunk_fb.pages().len().try_into().expect("usize as u64"),
            path: self.path.clone(),
        }
    }

    unsafe fn chunk_fb_unchecked(&self) -> wm::Chunk {
        let bytes = self.bytes();
        wm::size_prefixed_root_as_chunk_unchecked(bytes)
    }

    fn bytes(&self) -> &[u8] {
        self.mmap.deref()
    }
}

impl<'a, 'b> TryFrom<&'a wm::Page<'b>> for dump::Page {
    type Error = anyhow::Error;

    fn try_from(page_fb: &'a wm::Page<'b>) -> Result<dump::Page> {
        let mut page = convert_store_page_to_dump_page_without_body(page_fb)?;

        if let dump::Page {
            revision: Some(dump::Revision { ref mut text, .. }),
            ..
        } = page {
            *text = page_fb.revision()
                           .expect("just converted from this")
                           .text()
                           .map(|s| s.to_string());
        }
        Ok(page)
    }
}

pub fn convert_store_page_to_dump_page_without_body<'a, 'b>(
    page_fb: &'a wm::Page<'b>
) -> Result<dump::Page> {
    Ok(dump::Page {
        ns_id: page_fb.ns_id(),
        id: page_fb.id(),
        title: page_fb.title().to_string(),
        revision: page_fb.revision().as_ref().map(|revision| dump::Revision {
            id: revision.id(),
            text: None,
            categories: vec![],
        }),
    })
}
