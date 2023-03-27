//! A store for MediaWiki pages. Supports search and import from Wikimedia dump job files.

mod chunk;
pub mod index;

pub use chunk::{
    ChunkId, ChunkMeta, convert_store_page_to_dump_page_without_body, MappedChunk, MappedPage,
    StorePageId,
};

use crate::{
    dump::{self, CategorySlug, local::JobFiles},
    Error,
    Result,
    util::fmt::{ByteRate, Bytes, Duration},
};
use rayon::prelude::*;
use std::{
    fmt::Debug,
    path::PathBuf,
    result::Result as StdResult,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};
use valuable::Valuable;

pub struct Options {
    pub path: PathBuf,
    pub max_chunk_len: u64,
}

pub struct Store {
    chunk_store: chunk::Store,
    index: index::Index,
    opts: Options,
}

#[derive(Clone, Debug, Valuable)]
pub struct ImportResult {
    pub chunk_bytes_total: Bytes,
    pub chunk_write_rate: ByteRate,
    pub chunks_len: u64,
    pub duration: Duration,
    pub pages_total: u64,
}

#[derive(Clone, Debug, Valuable)]
pub struct ImportChunkResult {
    pub chunk_meta: chunk::ChunkMeta,
    pub duration: Duration,
}

impl Options {
    pub fn from_common_args(common_args: &crate::args::CommonArgs) -> Options {
        Options {
            path: common_args.store_path(),
            max_chunk_len: chunk::MAX_LEN_DEFAULT,
        }
    }

    /// Open an existing store or create a new one.
    pub fn build(self) -> Result<Store> {
        let index = index::Options {
            max_values_per_batch: 100,
            path: self.path.join("index"),
        }.build()?;

        let chunk_store = chunk::Options {
            max_chunk_len: self.max_chunk_len,
            path: self.path.join("chunks"),
        }.build()?;

        Ok(Store {
            chunk_store,
            index,

            // This moves self into Store, so do that last.
            opts: self,
        })
    }
}

impl Store {
    #[tracing::instrument(level = "debug", name = "Store::clear()", skip_all,
                          fields(self.path = %self.opts.path.display()))]
    pub fn clear(&mut self) -> Result<()> {
        self.chunk_store.clear()?;
        self.index.clear()?;

        Ok(())
    }

    pub fn import(&mut self, job_files: JobFiles) -> Result<ImportResult> {
        let start = Instant::now();

        let files = job_files.open_files_par_iter()?;
        let chunk_write_guard = self.chunk_store.try_write_lock()?;
        let index = &self.index;

        let chunk_bytes_total = AtomicU64::new(0);
        let pages_total = AtomicU64::new(0);
        let chunks_len = AtomicU64::new(0);

        enum ImportEnd {
            PageCountMax,
            Err(Error),
        }

        let end = files.try_for_each(
            |file_pages /* : impl Iterator<Item = Result<Page>> */| -> StdResult<(), ImportEnd> {
                let mut pages = file_pages.peekable();

                while pages.peek().is_some() {
                    if let Some(max) = job_files.open_spec().max_count.as_ref().copied() {
                        if pages_total.load(Ordering::SeqCst) > max {
                            return Err(ImportEnd::PageCountMax);
                        }
                    }

                    let chunk_builder = match chunk_write_guard.chunk_builder() {
                        Ok(b) => b,
                        Err(e) => return Err(ImportEnd::Err(e)),
                    };

                    let index_batch_builder = match index.import_batch_builder() {
                        Ok(b) => b,
                        Err(e) => return Err(ImportEnd::Err(e)),
                    };

                    let res = match Self::import_chunk(&mut pages, chunk_builder,
                                                       index_batch_builder) {
                        Ok(res) => res,
                        Err(e) => return Err(ImportEnd::Err(e)),
                    };

                    chunk_bytes_total.fetch_add(res.chunk_meta.bytes_len.0, Ordering::SeqCst);
                    pages_total.fetch_add(res.chunk_meta.pages_len, Ordering::SeqCst);
                    chunks_len.fetch_add(1, Ordering::SeqCst);

                    tracing::debug!(
                        chunk_bytes_total = Bytes(chunk_bytes_total.load(Ordering::SeqCst))
                                               .as_value(),
                        pages_total = pages_total.load(Ordering::SeqCst),
                        chunks_len = chunks_len.load(Ordering::SeqCst),
                        duration_so_far = Duration(start.elapsed()).as_value(),
                        "Chunk import done");
                };

                Ok(())
            });

        // Log stats before checking `end` for an Error.
        let chunk_bytes_total = Bytes(chunk_bytes_total.into_inner());
        let duration = Duration(start.elapsed());

        let res = ImportResult {
            chunk_bytes_total,
            chunk_write_rate: ByteRate::new(chunk_bytes_total, duration.0),
            chunks_len: chunks_len.into_inner(),
            duration,
            pages_total: pages_total.into_inner(),
        };

        if let Err(ImportEnd::Err(e)) = end {
            return Err(e);
        }

        tracing::info!(res = res.as_value(),
                       "Import done");

        self.index.optimise()?;

        Ok(res)
    }

    fn import_chunk<'lock, 'index>(
        pages: &mut dyn Iterator<Item = Result<dump::Page>>,
        mut chunk_builder: chunk::Builder<'lock>,
        mut index_batch_builder: index::ImportBatchBuilder<'index>,
    ) -> Result<ImportChunkResult> {
        let start = Instant::now();

        for page in pages {
            let page: dump::Page = page?;

            let store_page_id = chunk_builder.push(&page)?;
            index_batch_builder.push(&page, store_page_id)?;

            if chunk_builder.is_full() {
                break;
            }
        }

        let chunk_meta = chunk_builder.write_all()?;
        index_batch_builder.commit()?;

        let res = ImportChunkResult {
            chunk_meta,
            duration: Duration(start.elapsed()),
        };

        Ok(res)
    }

    pub fn get_category(&self, slug_lower_bound: Option<&CategorySlug>, limit: Option<u64>
    ) -> Result<Vec<dump::CategorySlug>>
    {
        self.index.get_category(slug_lower_bound, limit)
    }

    pub fn get_category_pages(
        &self,
        slug: &CategorySlug,
        page_mediawiki_id_lower_bound: Option<u64>,
        limit: Option<u64>,
    ) -> Result<Vec<index::Page>>
    {
        self.index.get_category_pages(slug, page_mediawiki_id_lower_bound, limit)
    }

    pub fn get_page_by_store_id(&self, id: StorePageId) -> Result<Option<MappedPage>> {
        self.chunk_store.get_page_by_store_id(id)
    }

    pub fn get_page_by_slug(&self, slug: &str) -> Result<Option<MappedPage>> {
        let id = try2!(self.index.get_store_page_id_by_slug(slug));
        self.get_page_by_store_id(id)
    }

    pub fn get_page_by_mediawiki_id(&self, id: u64) -> Result<Option<MappedPage>> {
        let store_page_id = try2!(self.index.get_store_page_id_by_mediawiki_id(id));
        self.get_page_by_store_id(store_page_id)
    }

    pub fn chunk_id_vec(&self) -> Result<Vec<ChunkId>> {
        self.chunk_store.chunk_id_vec()
    }

    pub fn chunk_id_iter(&self) -> impl Iterator<Item = Result<ChunkId>> {
        self.chunk_store.chunk_id_iter()
    }

    pub fn get_chunk_meta_by_chunk_id(&self, chunk_id: ChunkId) -> Result<Option<ChunkMeta>> {
        self.chunk_store.get_chunk_meta_by_chunk_id(chunk_id)
    }

    pub fn map_chunk(&self, chunk_id: ChunkId) -> Result<Option<MappedChunk>> {
        self.chunk_store.map_chunk(chunk_id)
    }
}
