//! MediaWiki pages are stored in chunk files, implemented in this module.
//!
//! Currently the chunk files contain about 10 MB of pages serialised as a capnproto struct.

use anyhow::{bail, Context, format_err};
use crate::{
    capnp::wikimedia_capnp as wmc,
};
use capnp::{
    message::{HeapAllocator, Reader, ReaderOptions, TypedBuilder,
              TypedReader},
    serialize::BufferSegments,
};
use crossbeam_utils::CachePadded;
use memmap2::Mmap;
use serde::Serialize;
use std::{
    cmp,
    fmt::{self, Debug, Display},
    fs,
    io::{BufWriter, Seek, Write},
    marker::PhantomData,
    path::{Path, PathBuf},
    result::Result as StdResult,
    str::FromStr,
    sync::atomic::{AtomicU64, Ordering},
};
use valuable::Valuable;
use wikimedia::{
    dump::{self, DumpName},
    Error,
    lazy_regex,
    Result,
    TempDir,
    util::{
        fmt::Bytes,
        IteratorExtSend,
    },
    try2,
    wikitext,
};

pub(crate) struct Store {
    lock: fd_lock::RwLock<fs::File>,
    opts: Options,
    temp_dir: TempDir,
}

pub(crate) struct Options {
    pub dump_name: DumpName,
    pub max_chunk_len: u64,
    pub path: PathBuf,
}

pub(crate) struct WriteLockGuard<'lock> {
    _inner: fd_lock::RwLockWriteGuard<'lock, fs::File>,
    max_chunk_len: u64,
    next_chunk_id: CachePadded<AtomicU64>,
    out_dir: PathBuf,
    temp_dir: PathBuf,
}

pub(crate) struct Builder<'lock> {
    capb: TypedBuilder<wmc::chunk::Owned, HeapAllocator>,
    chunk_id: ChunkId,
    curr_bytes_len_estimate: u64,
    max_chunk_len: u64,
    out_path: PathBuf,
    pages: Vec<dump::Page>,
    temp_path: PathBuf,

    phantom_lock: PhantomData<&'lock WriteLockGuard<'lock>>,
}

#[derive(Clone, Copy, Debug)]
pub struct StorePageId {
    pub(crate) chunk_id: ChunkId,
    pub(crate) page_chunk_index: PageChunkIndex,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd, Serialize, Valuable)]
#[serde(transparent)]
pub struct ChunkId(pub(crate) u64);

#[derive(Clone, Copy, Debug)]
pub struct PageChunkIndex(pub(crate) u64);

pub struct MappedChunk {
    dump_name: DumpName,
    id: ChunkId,
    len: u64,
    path: PathBuf,
    reader: TypedReader<BufferSegments<Mmap>, wmc::chunk::Owned>,
}

pub struct MappedPage {
    chunk: MappedChunk,
    dump_name: DumpName,
    store_id: StorePageId,
}

#[derive(Clone, Debug, Serialize, Valuable)]
pub struct ChunkMeta {
    pub bytes_len: Bytes,
    pub id: ChunkId,
    pub pages_len: u64,
    pub path: PathBuf,
}

struct ChunksStats {
    count: usize,
    max_id: Option<ChunkId>,
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
            page_chunk_index: PageChunkIndex(segments[1].parse::<u64>()?),
        })
    }
}

impl Display for StorePageId {
    fn fmt(&self,
           f: &mut fmt::Formatter
    ) -> StdResult<(), fmt::Error> {
        let StorePageId { chunk_id, page_chunk_index } = self;
        write!(f, "{chunk_id}.{page_chunk_index}")
    }
}

impl Options {
    pub fn build(self) -> Result<Store> {
        Store::new(self)
    }
}

impl Store {
    fn new(opts: Options) -> Result<Store> {
        Ok(Store {
            lock: Self::init_lock(&opts)?,
            temp_dir: TempDir::create(&*opts.path, /* keep: */ false)?,

            // This moves opts into Store, so do that last.
            opts,
        })
    }

    pub fn clear(&mut self) -> Result<()> {
        let opts = &self.opts;
        let _guard = self.lock.try_write()?;

        let chunks_path = &*self.opts.path;
        if chunks_path.try_exists()? {
            for chunk_id in Self::chunk_id_iter_from_opts(opts) {
                let chunk_path = chunk_path(&*opts.path, chunk_id?);
                fs::remove_file(chunk_path)?;
            }
        }

        Ok(())
    }

    pub fn try_write_lock<'store, 'lock>(&'store mut self) -> Result<WriteLockGuard<'lock>>
        where 'store: 'lock
    {
        let inner_guard = self.lock.try_write()?;

        let chunks_stats = Self::get_chunk_stats(&self.opts)?;

        let next_chunk_id = match chunks_stats.max_id {
            Some(ChunkId(id)) => ChunkId(id + 1),
            None => ChunkId(0),
        };

        tracing::debug!(%next_chunk_id,
                        "store::chunk::Store::try_write_lock() succeeded");

        Ok(WriteLockGuard {
            _inner: inner_guard,
            max_chunk_len: self.opts.max_chunk_len,
            next_chunk_id: CachePadded::new(AtomicU64::new(next_chunk_id.0)),
            out_dir: self.opts.path.to_owned(),
            temp_dir: self.temp_dir.path()?.to_owned(),
        })
    }

    fn init_lock(opts: &Options) -> Result<fd_lock::RwLock<fs::File>> {
        let lock_path = opts.path.join("lock");

        // Closure to add context to errors.
        (|| {
            fs::create_dir_all(&*opts.path)?;
            let file = fs::OpenOptions::new()
                           .read(true)
                           .write(true)
                           .create(true)
                           .open(&*lock_path)?;
            let lock = fd_lock::RwLock::new(file);
            anyhow::Ok(lock)
        })().with_context(|| format!("While creating chunk store lock file '{path}'",
                                     path = lock_path.display()))
    }

    pub fn get_page_by_store_id(&self, id: StorePageId) -> Result<Option<MappedPage>> {
        let chunk: MappedChunk = try2!(self.map_chunk(id.chunk_id));
        let page: MappedPage = chunk.get_mapped_page(id.page_chunk_index)?;
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

                let Some(captures) = lazy_regex!("^articles-([0-9a-f]{16}).cap$").captures(&*name)
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

    fn get_chunk_stats(opts: &Options) -> Result<ChunksStats> {
        let chunk_iter_span = tracing::trace_span!("ChunkStore enumerating existing chunks.",
                                                   chunk_count = tracing::field::Empty,
                                                   max_existing_chunk_id = tracing::field::Empty)
                                      .entered();

        let chunk_stats: ChunksStats =
            Store::chunk_id_iter_from_opts(&opts)
                .try_fold(ChunksStats { count: 0, max_id: None }, // inital state
                          |s: ChunksStats, next: Result<ChunkId>|
                          -> Result<ChunksStats> {
                              let next = next?;
                              Ok(ChunksStats {
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

        Ok(chunk_stats)
    }

    pub fn get_chunk_meta_by_chunk_id(&self, chunk_id: ChunkId) -> Result<Option<ChunkMeta>> {
        let chunk = try2!(self.map_chunk(chunk_id));
        Ok(Some(chunk.meta()?))
    }

    pub fn map_chunk(&self, id: ChunkId) -> Result<Option<MappedChunk>> {
        let path = chunk_path(&*self.opts.path, id);

        let file = match fs::File::open(&*path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
            Ok(f) => f,
        };
        let mmap = unsafe {
            memmap2::MmapOptions::new()
                .map(&file)?
        };
        let len = mmap.len().try_into().expect("usize as u64");

        let segments = BufferSegments::new(mmap, ReaderOptions::default())?;
        let reader = Reader::new(segments, ReaderOptions::default());
        let typed_reader = reader.into_typed::<wmc::chunk::Owned>();

        let chunk = MappedChunk {
            dump_name: self.opts.dump_name.clone(),
            id,
            len,
            path: path.clone(),
            reader: typed_reader,
        };

        Ok(Some(chunk))
    }
}

fn chunk_path(dir: &Path, chunk_id: ChunkId) -> PathBuf {
    dir.join(format!("articles-{id:016x}.cap", id = chunk_id.0))
}

impl<'lock> WriteLockGuard<'lock> {
    fn next_chunk_id(&self) -> ChunkId {
        let next = self.next_chunk_id.fetch_add(1, Ordering::SeqCst);
        ChunkId(next)
    }

    pub(crate) fn chunk_builder(&'lock self) -> Result<Builder<'lock>> {
        let chunk_id = self.next_chunk_id();

        let out_path = chunk_path(&*self.out_dir, chunk_id);
        let temp_path = self.temp_dir.join(
            out_path.file_name().expect("Chunk file name"));

        fs::create_dir_all(out_path.parent().expect("parent of out_path"))?;
        fs::create_dir_all(temp_path.parent().expect("parent of temp_path"))?;

        Ok(Builder {
            capb: TypedBuilder::<wmc::chunk::Owned, HeapAllocator>::new_default(),
            chunk_id,
            curr_bytes_len_estimate: 0,
            max_chunk_len: self.max_chunk_len,
            out_path,
            pages: Vec::new(),
            temp_path,

            phantom_lock: PhantomData,
        })
    }
}

impl<'lock> Builder<'lock> {
    pub fn push(&mut self, page: &dump::Page) -> Result<StorePageId> {
        let page = page.clone();
        self.curr_bytes_len_estimate +=
            u64::try_from(page.title.len() +
            match page.revision {
                Some(dump::Revision { text: Some(ref text), .. }) => text.len(),
                _ => 0,
            }).expect("usize as u64");
        self.pages.push(page);
        let idx = self.pages.len() - 1;

        Ok(StorePageId {
            chunk_id: self.chunk_id,
            page_chunk_index: PageChunkIndex(idx.try_into().expect("usize as u64")),
        })
    }

    pub fn write_all(mut self) -> Result<ChunkMeta> {
        let pages_len = self.pages.len();
        let chunk_cap: wmc::chunk::Builder = self.capb.init_root();
        let mut pages_cap = chunk_cap.init_pages(pages_len.try_into()
                                                     .expect("pages.len() usize into u32"));

        let pages = std::mem::take(&mut self.pages);
        for (idx, page) in pages.into_iter().enumerate() {
            let mut page_cap = pages_cap.reborrow().try_get(idx.try_into()
                                    .expect("page chunk index u32 from usize"))
                                    .expect("pages_cap.len() == pages.len()");
            page_cap.set_ns_id(page.ns_id);
            page_cap.set_id(page.id);
            page_cap.set_title(&*page.title);
            if let Some(revision) = page.revision {
                let mut revision_cap = page_cap.init_revision();
                revision_cap.set_id(revision.id);
                if let Some(text) = revision.text {
                    revision_cap.set_text(text.as_str());
                }
            }
        }

        let temp_file = fs::File::create(&*self.temp_path)?;
        let mut buf_writer = BufWriter::with_capacity(16 * 1024, temp_file);
        capnp::serialize::write_message(&mut buf_writer, self.capb.borrow_inner())?;
        drop(self.capb);
        buf_writer.flush()?;
        buf_writer.get_ref().sync_all()?;
        let bytes_len = buf_writer.stream_position()?;
        drop(buf_writer);

        fs::rename(&*self.temp_path, &*self.out_path)?;

        Ok(ChunkMeta {
            bytes_len: Bytes(bytes_len),
            id: self.chunk_id,
            pages_len: pages_len.try_into().expect("Convert usize to u64"),
            path: self.out_path,
        })
    }

    #[allow(dead_code)] // Not used at the moment.
    pub fn curr_bytes_len_estimate(&self) -> u64 {
        self.curr_bytes_len_estimate
    }

    pub fn is_full(&self) -> bool {
        self.curr_bytes_len_estimate > self.max_chunk_len
    }
}

impl MappedChunk {
    fn get_page<'a, 'b>(&'a self, idx: PageChunkIndex
    ) -> Result<wmc::page::Reader<'b>>
        where 'a: 'b
    {
        let chunk: wmc::chunk::Reader<'_> = self.reader.get()?;
        let pages = chunk.get_pages()?;
        let page: wmc::page::Reader<'_> =
            pages.try_get(idx.0.try_into().expect("u64 PageChunkIndex as u32"))
                 .ok_or_else(|| format_err!("MappedPage::borrow page index out of bounds. \
                                             idx={idx} pages_len={len} chunk_id={chunk_id:?}",
                                            len = pages.len(), chunk_id = self.id))?;
        Ok(page)
    }

    fn get_mapped_page(self, idx: PageChunkIndex) -> Result<MappedPage> {
        Ok(MappedPage {
            dump_name: self.dump_name.clone(),
            store_id: StorePageId {
                chunk_id: self.id,
                page_chunk_index: idx
            },

            // This moves `self`, so do it last.
            chunk: self,
        })
    }

    pub fn pages_iter(&self
    ) -> Result<impl Iterator<Item = (StorePageId, wmc::page::Reader<'_>)>>
    {
        let chunk: wmc::chunk::Reader<'_> = self.reader.get()?;
        let pages = chunk.get_pages()?;
        let iter = pages.iter()
                        .enumerate()
                        .map(|(idx, page)|
                             (
                                 StorePageId {
                                     chunk_id: self.id,
                                     page_chunk_index: PageChunkIndex(
                                         idx.try_into().expect("usize as u64")),
                                 },
                                 page
                             ));
        Ok(iter)
    }

    fn meta(&self) -> Result<ChunkMeta> {
        let chunk: wmc::chunk::Reader<'_> = self.reader.get()?;
        let pages = chunk.get_pages()?;

        Ok(ChunkMeta {
            bytes_len: Bytes(self.len),
            id: self.id,
            pages_len: u64::from(pages.len()),
            path: self.path.clone(),
        })
    }
}

impl MappedPage {
    pub fn borrow<'a>(&'a self) -> Result<wmc::page::Reader<'a>> {
        self.chunk.get_page(self.store_id.page_chunk_index)
    }

    pub fn dump_name(&self) -> DumpName {
        self.dump_name.clone()
    }

    pub fn store_id(&self) -> StorePageId {
        self.store_id
    }
}

impl<'a, 'b> TryFrom<&'a wmc::page::Reader<'b>> for dump::Page {
    type Error = Error;

    fn try_from(page_cap: &'a wmc::page::Reader<'b>) -> Result<dump::Page> {
        let mut page = convert_store_page_to_dump_page_without_body(page_cap)?;

        if page_cap.has_revision() {
            let rev_cap = page_cap.get_revision()?;
            if rev_cap.has_text() {
                let text = rev_cap.get_text()?;
                let rev = page.revision.as_mut()
                              .expect("page_cap has revision so page should too");
                rev.text = Some(text.to_string());
                rev.categories = wikitext::parse_categories(text);
            }
        }

        Ok(page)
    }
}

pub fn convert_store_page_to_dump_page_without_body<'a, 'b>(
    page_cap: &'a wmc::page::Reader<'b>
) -> Result<dump::Page> {
    Ok(dump::Page {
        ns_id: page_cap.get_ns_id(),
        id: page_cap.get_id(),
        title: page_cap.get_title()?.to_string(),
        revision: if page_cap.has_revision() {
            let rev_cap = page_cap.get_revision()?;
            let rev_text = if rev_cap.has_text() {
                Some(rev_cap.get_text()?.to_string())
            } else {
                None
            };
            Some(dump::Revision {
                id: rev_cap.get_id(),
                categories: match rev_text {
                    Some(ref text) => wikitext::parse_categories(text.as_str()),
                    None => Vec::with_capacity(0),
                },
                text: rev_text,
            })
        } else {
            None
        },
    })
}
