//! A store for MediaWiki pages. Supports search and import from Wikimedia dump job files.

mod chunk;
pub mod index;

pub use chunk::{
    ChunkId, ChunkMeta, convert_store_page_to_dump_page_without_body, MappedChunk, MappedPage,
    StorePageId,
};

use crate::{
    dump::{
        self,
        CategorySlug,
        local::{FileSpec, JobFiles, OpenJobFile},
    },
    Error,
    Result,
    util::fmt::{ByteRate, Bytes, Duration},
};
use rayon::prelude::*;
use std::{
    fmt::Debug,
    io::Write,
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

        let chunk_write_guard = self.chunk_store.try_write_lock()?;

        let files = job_files.open_files_par_iter()?;
        let total_source_bytes = job_files.files_total_len();
        let num_source_files = job_files.file_specs().len();

        tracing::info!(
            total_source_bytes = total_source_bytes.as_value(),
            num_source_files,
            open_spec = job_files.open_spec().as_value(),
            "Starting import");

        enum ImportEnd {
            PageCountMax,
            Err(Error),
        }

        let index = &self.index;

        let chunk_bytes_total = AtomicU64::new(0);
        let chunks_len = AtomicU64::new(0);
        let pages_total = AtomicU64::new(0);
        let total_source_bytes_read = AtomicU64::new(0);

        let end = files.try_for_each(
            |file: Result<OpenJobFile>| -> StdResult<(), ImportEnd> {
                let OpenJobFile {
                    file_spec,
                    pages_iter,
                    source_bytes_read,
                    uncompressed_bytes_read,
                } = match file {
                    Err(e) => return Err(ImportEnd::Err(e)),
                    Ok(file) => file,
                };

                let mut pages = pages_iter.peekable();

                while pages.peek().is_some() {
                    if let Some(max) = job_files.open_spec().max_count.as_ref().copied() {
                        if pages_total.load(Ordering::SeqCst) > max {
                            return Err(ImportEnd::PageCountMax);
                        }
                    }

                    let source_bytes_read_before = source_bytes_read.load(Ordering::SeqCst);

                    let chunk_builder = match chunk_write_guard.chunk_builder() {
                        Ok(b) => b,
                        Err(e) => return Err(ImportEnd::Err(e)),
                    };

                    let index_batch_builder = match index.import_batch_builder() {
                        Ok(b) => b,
                        Err(e) => return Err(ImportEnd::Err(e)),
                    };

                    let res = match Self::import_chunk(&file_spec, &mut pages, chunk_builder,
                                                       index_batch_builder) {
                        Ok(res) => res,
                        Err(e) => {
                            let e = e.context(
                                format!("While importing a chunk from file {file_spec:?} \
                                         source_bytes_read={source_bytes_read:?} \
                                         uncompressed_bytes_read={uncompressed_bytes_read:?}",
                                        source_bytes_read =
                                            Bytes(source_bytes_read.load(Ordering::SeqCst)),
                                        uncompressed_bytes_read =
                                            Bytes(uncompressed_bytes_read.load(
                                                Ordering::SeqCst))));
                            return Err(ImportEnd::Err(e));
                        },
                    };

                    // fetch_add counters.
                    let chunk_bytes_total_curr =
                        chunk_bytes_total.fetch_add(res.chunk_meta.bytes_len.0, Ordering::SeqCst);
                    let pages_total_curr =
                        pages_total.fetch_add(res.chunk_meta.pages_len, Ordering::SeqCst);
                    let chunks_len_curr =
                        chunks_len.fetch_add(1, Ordering::SeqCst);
                    let source_bytes_read_after = source_bytes_read.load(Ordering::SeqCst);
                    let source_bytes_read_diff =
                        source_bytes_read_after - source_bytes_read_before;
                    let total_source_bytes_read_curr =
                        total_source_bytes_read.fetch_add(source_bytes_read_diff,
                                                          Ordering::SeqCst);

                    // Calculate derived stats.
                    let percent_complete =
                        ((total_source_bytes_read_curr as f64) /
                         (total_source_bytes.0 as f64)) * 100.0;

                    let duration_so_far = start.elapsed();
                    let total_source_bytes_remaining =
                        total_source_bytes.0 - total_source_bytes_read_curr;

                    let est_remaining_duration: Option<Duration> =
                        match total_source_bytes_read_curr {
                            0 => None,
                            bytes_done => {
                                let secs: f64 =
                                    (duration_so_far.as_secs_f64() / (bytes_done as f64))
                                        * (total_source_bytes_remaining as f64);

                                let nanos = secs * 1_000_000_000.0;
                                let dur = std::time::Duration::from_nanos(nanos as u64);
                                Some(Duration(dur))
                            }
                        };

                    let now = chrono::Local::now();

                    let eta: Option<String> = est_remaining_duration.and_then(
                        |dur| -> Option<String> {
                            let chrono_time =
                                now
                                    + match chrono::Duration::from_std(dur.0) {
                                        Ok(chrono_dur) => chrono_dur,
                                        Err(_e) => return None,
                                    };
                            let s = chrono_time.to_rfc3339_opts(chrono::SecondsFormat::Secs,
                                                                true /* use_z */)
                                               .replace('T', " ");
                            Some(s)
                        });

                    let percent_complete_str = format!("{percent_complete:02.1}");

                    let res = writeln!(std::io::stderr(),
                           "{now}     Import: {percent_complete_str}%\
                            {remaining_str}\
                            {eta}",
                           now = now.to_rfc3339_opts(chrono::SecondsFormat::Secs,
                                                     true /* use_z */)
                                    .replace('T', " "),
                           remaining_str = match est_remaining_duration {
                               Some(dur) => format!("   remaining: {:.2?}", dur.0),
                               None => "".to_string(),
                           },
                           eta = match eta {
                               Some(ref eta) => format!("   ETA: {eta}"),
                               None => "".to_string(),
                           },
                    );

                    if let Err(e) = res {
                        return Err(ImportEnd::Err(e.into()));
                    }

                    tracing::debug!(
                        // Store current stats
                        chunk_bytes_total = Bytes(chunk_bytes_total_curr).as_value(),
                        pages_total = pages_total_curr,
                        chunks_len = Bytes(chunks_len_curr).as_value(),

                        // Import total raw stats
                        total_source_bytes_read = Bytes(total_source_bytes_read_curr).as_value(),
                        total_source_bytes = total_source_bytes.as_value(),
                        total_source_bytes_remaining =
                            Bytes(total_source_bytes_remaining).as_value(),
                        duration_so_far = Duration(duration_so_far).as_value(),

                        // Import total derived stats
                        percent_complete,
                        percent_complete_str,
                        est_remaining_duration = est_remaining_duration.as_value(),
                        eta,

                        // This file stats
                        input_file = %file_spec.path.display(),
                        source_bytes_read = Bytes(source_bytes_read_diff).as_value(),
                        // WIP: uncompressed_bytes_read = Bytes(uncompressed_bytes_read_diff.get()),
                        "Chunk import done");
                };

                tracing::debug!(input_file = %file_spec.path.display(),
                                "Finished importing from file");

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

        tracing::info!(res = res.as_value(),
                       "Import done");

        if let Err(ImportEnd::Err(e)) = end {
            return Err(e);
        }

        self.index.optimise()?;

        Ok(res)
    }

    fn import_chunk<'lock, 'index>(
        _file_spec: &FileSpec,
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
