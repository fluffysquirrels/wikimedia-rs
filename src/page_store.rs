use anyhow::{ensure, format_err};
use crate::{
    article_dump,
    fbs::wikimedia as wm,
    Result,
    slug,
    TempDir,
};
use flatbuffers::{self as fb, FlatBufferBuilder, WIPOffset};
use serde::Serialize;
use std::{
    cmp,
    convert::TryFrom,
    fmt::{self, Debug, Display},
    io::Write,
    ops::Deref,
    path::PathBuf,
    result::Result as StdResult,
    str::FromStr,
    time::{Duration, Instant},
};
use tracing::Level;

macro_rules! try2 {
    ($expr:expr $(,)?) => {
        match $expr {
            std::result::Result::Ok(std::option::Option::Some(val)) => val,
            std::result::Result::Ok(std::option::Option::None) => {
                return std::result::Result::Ok(std::option::Option::None);
            }
            std::result::Result::Err(err) => {
                return std::result::Result::Err(std::convert::From::from(err));
            }
        }
    };
}

pub struct Options {
    pub path: PathBuf,
    pub max_chunk_len: usize,
}

pub struct Store {
    next_chunk_id: ChunkId,
    opts: Options,
    sled_db: sled::Db,
    temp_dir: TempDir,
}

#[derive(Clone, Copy, Debug)]
pub struct StorePageId {
    chunk_id: ChunkId,
    page_chunk_idx: PageChunkIndex,
}

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
pub struct ChunkId(u64);

#[derive(Clone, Copy, Debug)]
struct PageChunkIndex(u64);

pub struct MappedChunk {
    path: PathBuf,
    mmap: memmap2::Mmap,
}

pub struct MappedPage {
    chunk: MappedChunk,
    loc: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChunkMeta {
    bytes_len: u64,
    pages_len: u64,
    path: PathBuf,
}

#[derive(Clone, Debug)]
struct ImportChunkResult {
    chunk_meta: ChunkMeta,
    _duration: Duration,
}

#[derive(Clone, Debug)]
pub struct ImportResult {
    pub bytes_total: u64,
    pub chunks_len: u64,
    pub duration: Duration,
    pub pages_total: u64,
}

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
            return Err(anyhow::Error::msg(
                "StorePageId::from_str expects 2 integers separated by a '.'"));
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
            return Err(anyhow::Error::msg("StorePageId::try_from: input.len() != 16"));
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
    fn to_bytes(&self) -> [u8; 16] {
        let mut out = [0u8; 16];
        out[0..8].copy_from_slice(self.chunk_id.0.to_be_bytes().as_ref());
        out[8..16].copy_from_slice(self.page_chunk_idx.0.to_be_bytes().as_ref());
        out
    }
}

impl Options {
    pub fn from_common_args(common_args: &crate::args::CommonArgs) -> Options {
        Options {
            path: common_args.page_store_path(),
            max_chunk_len: 10_000_000, // 10 MB
        }
    }

    pub fn build_store(self) -> Result<Store> {
        Store::new(self)
    }
}

impl Store {
    pub fn new(opts: Options) -> Result<Store> {
        let sled_db = sled::Config::default()
                                   .path(opts.path.join("sled_db"))
                                   .open()?;

        let max_existing_chunk_id: Result<Option<Result<ChunkId>>> =
            Self::chunk_id_iter_from_opts(&opts)
                 .try_reduce(|id1: Result<ChunkId>, id2: Result<ChunkId>|
                             -> Result<Result<ChunkId>> {
                                 Ok(Ok(cmp::max(id1?, id2?)))
                             });
        let next_chunk_id = match max_existing_chunk_id? {
            Some(res) => ChunkId(res?.0 + 1),
            None => ChunkId(0),
        };

        if tracing::enabled!(Level::DEBUG) {
            let tree_names = sled_db.tree_names()
                                    .into_iter()
                                    .map(|bytes| -> String {
                                         String::from_utf8_lossy(&*bytes).into_owned()
                                    })
                                    .collect::<Vec<String>>();

            tracing::debug!(%next_chunk_id,
                            sled_db.trees = ?tree_names,
                            sled_db.size_on_disk = sled_db.size_on_disk()?,
                            sled_db.was_recovered = sled_db.was_recovered(),
                            "Store::new() loaded");
        }

        Ok(Store {
            next_chunk_id,
            sled_db,
            temp_dir: TempDir::create(&*opts.path, /* keep: */ false)?,

            // This moves opts into Store, so we do that last.
            opts,
        })
    }

    #[tracing::instrument(level = "debug", name = "Store::clear()", skip_all,
                          fields(self.path = %self.opts.path.display()))]
    pub fn clear(&mut self) -> Result<()> {
        let chunks_path = self.chunks_path();
        if chunks_path.try_exists()? {
            std::fs::remove_dir_all(&*chunks_path)?;
        }

        let default_tree_name = (*self.sled_db).name();

        for tree_name in self.sled_db.tree_names().into_iter() {
            if tree_name == default_tree_name {
                continue;
            }

            tracing::debug!(tree_name = &*String::from_utf8_lossy(&*tree_name),
                            "Dropping sled_db tree");
            self.sled_db.drop_tree(tree_name)?;
        }

        Ok(())
    }

    pub fn import(&mut self, pages: impl Iterator<Item = Result<article_dump::Page>> + 'static
    ) -> Result<ImportResult> {
        // import_inner takes a `Box<dyn Iterator>` so we don't have to generate
        // many versions of the whole body.
        self.import_inner(Box::new(pages))
    }

    fn import_inner(&mut self, pages: Box<dyn Iterator<Item = Result<article_dump::Page>>>
    ) -> Result<ImportResult> {

        let start = Instant::now();

        let mut pages = pages.peekable();

        let mut bytes_total: u64 = 0;
        let mut pages_total: u64 = 0;
        let mut chunks_len: u64 = 0;

        while pages.peek().is_some() {
            let res = self.import_chunk(&mut pages)?;
            bytes_total += res.chunk_meta.bytes_len;
            pages_total += res.chunk_meta.pages_len;
            chunks_len += 1;
        };

        let res = ImportResult {
            bytes_total,
            chunks_len,
            duration: start.elapsed(),
            pages_total,
        };

        tracing::debug!(?res,
                        "Import done");

        Ok(res)
    }

    fn import_chunk(&mut self, pages: &mut dyn Iterator<Item = Result<article_dump::Page>>
    ) -> Result<ImportChunkResult> {
        let start = Instant::now();

        let chunk_id = self.next_chunk_id();

        let fb_out_path = self.chunk_path(chunk_id);
        let fb_tmp_path = self.temp_dir.path()?.join(
            fb_out_path.file_name().expect("Chunk file name"));

        std::fs::create_dir_all(fb_out_path.parent().expect("parent of fb_out_path"))?;
        std::fs::create_dir_all(fb_tmp_path.parent().expect("parent of fb_tmp_path"))?;

        let mut page_fbs = Vec::<WIPOffset<wm::Page>>::with_capacity(
            self.opts.max_chunk_len / 1_000);
        let mut fbb = FlatBufferBuilder::with_capacity(
            self.opts.max_chunk_len + (self.opts.max_chunk_len / 10) + 1_000_000);

        let mut by_slug_batch = sled::Batch::default();
        let mut by_id_batch = sled::Batch::default();

        for (page_chunk_idx, page) in pages.enumerate() {
            let page = page?;
            let page_title = fbb.create_string(&*page.title);
            let revision_fb = page.revision.as_ref().map(|revision| {
                let revision_text_fb = revision.text.as_ref().map(|text| {
                    fbb.create_string(&*text)
                });
                let mut revision_b = wm::RevisionBuilder::new(&mut fbb);
                revision_b.add_id(revision.id);
                if let Some(fb) = revision_text_fb {
                    revision_b.add_text(fb);
                }
                revision_b.finish()
            });
            let mut page_b = wm::PageBuilder::new(&mut fbb);
            if let Some(revision_fb) = revision_fb {
                page_b.add_revision(revision_fb);
            }
            page_b.add_ns_id(page.ns_id);
            page_b.add_id(page.id);
            page_b.add_title(page_title);
            let page_fb: WIPOffset<wm::Page> = page_b.finish();
            page_fbs.push(page_fb);

            let store_page_id = StorePageId {
                chunk_id,
                page_chunk_idx: PageChunkIndex(page_chunk_idx.try_into().expect("usize as u64")),
            };
            let store_page_id_bytes = store_page_id.to_bytes();

            let page_slug = slug::page_title_to_slug(&*page.title);
            by_slug_batch.insert(&*page_slug, &store_page_id_bytes);

            by_id_batch.insert(&page.id.to_be_bytes(), &store_page_id_bytes);

            let fbb_curr_len = fbb.unfinished_data().len();
            if fbb_curr_len > self.opts.max_chunk_len {
                break;
            }
        }

        let pages_len = page_fbs.len();

        let pages_vec = fbb.create_vector_from_iter(page_fbs.into_iter());

        let mut chunk_b = wm::ChunkBuilder::new(&mut fbb);
        chunk_b.add_pages(pages_vec);
        let chunk = chunk_b.finish();

        wm::finish_size_prefixed_chunk_buffer(&mut fbb, chunk);
        let fb_out = fbb.finished_data();
        let bytes_len = fb_out.len();

        let mut tmp_file = std::fs::File::create(&*fb_tmp_path)?;
        tmp_file.write_all(fb_out)?;
        drop(fbb);
        tmp_file.flush()?;
        tmp_file.sync_all()?;
        drop(tmp_file);

        std::fs::rename(&*fb_tmp_path, &*fb_out_path)?;

        let by_slug = self.get_tree_store_page_id_by_slug()?;
        by_slug.apply_batch(by_slug_batch)?;
        by_slug.flush()?;

        let by_id = self.get_tree_store_page_id_by_mediawiki_id()?;
        by_id.apply_batch(by_id_batch)?;
        by_id.flush()?;

        // TODO: Add chunk to store metadata, including path, ChunkId,
        // count of pages, low page.id, high page.id.

        // TODO: Update index of page.id -> StorePageId.

        let res = ImportChunkResult {
            chunk_meta: ChunkMeta {
                bytes_len: bytes_len.try_into().expect("Convert usize to u64"),
                pages_len: pages_len.try_into().expect("Convert usize to u64"),
                path: fb_out_path,
            },
            _duration: start.elapsed(),
        };

        tracing::debug!(?res,
                        "Chunk imported");

        Ok(res)
    }

    pub fn get_page_by_store_id(&self, id: StorePageId) -> Result<Option<MappedPage>> {
        let chunk: MappedChunk = try2!(self.map_chunk(id.chunk_id));
        let page: MappedPage = chunk.get_mapped_page(id.page_chunk_idx)?;
        Ok(Some(page))
    }

    pub fn get_page_by_slug(&self, slug: &str) -> Result<Option<MappedPage>> {
        let id = try2!(self.get_page_store_id_by_slug(slug));
        self.get_page_by_store_id(id)
    }

    pub fn get_page_by_mediawiki_id(&self, id: u64) -> Result<Option<MappedPage>> {
        let by_id = self.get_tree_store_page_id_by_mediawiki_id()?;
        let store_page_id = try2!(by_id.get(&id.to_be_bytes()));
        let store_page_id = StorePageId::try_from(&*store_page_id)?;
        self.get_page_by_store_id(store_page_id)
    }

    pub fn get_page_store_id_by_slug(&self, slug: &str) -> Result<Option<StorePageId>> {
        let by_slug = self.get_tree_store_page_id_by_slug()?;
        let id = try2!(by_slug.get(slug));
        let store_page_id = StorePageId::try_from(&*id)?;
        Ok(Some(store_page_id))
    }

    pub fn chunk_id_iter(&self) -> impl Iterator<Item = Result<ChunkId>> {
        Self::chunk_id_iter_from_opts(&self.opts)
    }

    fn chunk_id_iter_from_opts(opts: &Options) -> impl Iterator<Item = Result<ChunkId>> {
        // TODO: Use chunks metadata in sled.

        // This closure is to specify the return type explicitly.
        // Without this the return type is inferred from the first return
        // and doesn't include the `dyn`, so the subsequent ones fail to type check.
        (|| -> Box<dyn Iterator<Item = Result<ChunkId>>> {
            let chunks_path = Self::chunks_path_from_opts(opts);
            let read_dir = match std::fs::read_dir(chunks_path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound =>
                    return Box::new(std::iter::empty()),
                Err(e) => return Box::new(std::iter::once(Err(e.into()))),
                Ok(d) => d,
            };
            Box::new(read_dir.flat_map::<Option<Result<ChunkId>>, _>(|item| {
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
            }))
        })()
    }

    pub fn get_chunk_meta_by_chunk_id(&self, chunk_id: ChunkId) -> Result<Option<ChunkMeta>> {
        let chunk = try2!(self.map_chunk(chunk_id));
        Ok(Some(chunk.meta()))
    }

    pub fn get_mapped_chunk_by_chunk_id(
        &self, chunk_id: ChunkId
    ) -> Result<Option<MappedChunk>> {
        self.map_chunk(chunk_id)
    }

    fn map_chunk(&self, chunk_id: ChunkId) -> Result<Option<MappedChunk>> {
        let path = self.chunk_path(chunk_id);

        let file = match std::fs::File::open(&*path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
            Ok(f) => f,
        };
        let mmap = unsafe {
            memmap2::MmapOptions::new()
                .map(&file)?
        };

        let chunk = MappedChunk {
            path: path.clone(),
            mmap,
        };

        let bytes = chunk.bytes();

        // This load runs the flatbuffers verifier, subsequent loads will not.
        let _chunk_fb =
            tracing::trace_span!("Store::map_chunk() running verifier.",
                                 chunk_id = ?chunk_id,
                                 path = %path.display())
                  .in_scope(|| {
                    wm::size_prefixed_root_as_chunk(bytes)
                })?;

        Ok(Some(chunk))
    }

    fn chunks_path(&self) -> PathBuf {
        Self::chunks_path_from_opts(&self.opts)
    }

    fn chunks_path_from_opts(opts: &Options) -> PathBuf {
        opts.path.join("chunks")
    }

    fn chunk_path(&self, chunk_id: ChunkId) -> PathBuf {
        self.chunks_path().join(format!("articles-{id:016x}.fbd", id = chunk_id.0))
    }

    fn next_chunk_id(&mut self) -> ChunkId {
        let next = self.next_chunk_id;
        self.next_chunk_id.0 += 1;
        next
    }

    fn get_tree_store_page_id_by_slug(&self) -> Result<sled::Tree> {
        Ok(self.sled_db.open_tree(b"store_page_id_by_slug")?)
    }

    fn get_tree_store_page_id_by_mediawiki_id(&self) -> Result<sled::Tree> {
        Ok(self.sled_db.open_tree(b"store_page_id_by_mediawiki_id")?)
    }
}

trait ResultOptionExt<T, E> {
    fn map2<F, U>(self, f: F) -> StdResult<Option<U>, E>
        where F: Fn(T) -> U;
}

impl<T, E> ResultOptionExt<T, E> for StdResult<Option<T>, E> {

    fn map2<F, U>(self, f: F) -> StdResult<Option<U>, E>
        where F: Fn(T) -> U
    {
        self.map(|opt: Option<T>|
                 opt.map(|x: T|
                         f(x)))
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
            bytes_len: self.mmap.len().try_into().expect("usize as u64"),
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

impl<'a, 'b> TryFrom<&'a wm::Page<'b>> for article_dump::Page {
    type Error = anyhow::Error;

    fn try_from(page_fb: &'a wm::Page<'b>) -> Result<article_dump::Page> {
        Ok(article_dump::Page {
            ns_id: page_fb.ns_id(),
            id: page_fb.id(),
            title: page_fb.title().to_string(),
            revision: page_fb.revision().as_ref().map(|revision| article_dump::Revision {
                id: revision.id(),
                text: revision.text().map(|text| text.to_string()),
            }),
        })
    }
}

pub fn convert_store_page_to_article_dump_page_without_body<'a, 'b>(page_fb: &'a wm::Page<'b>
) -> Result<article_dump::Page> {
    Ok(article_dump::Page {
        ns_id: page_fb.ns_id(),
        id: page_fb.id(),
        title: page_fb.title().to_string(),
        revision: page_fb.revision().as_ref().map(|revision| article_dump::Revision {
            id: revision.id(),
            text: None,
        }),
    })
}
