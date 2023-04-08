//! A store for MediaWiki pages. Supports search and import from Wikimedia dump job files.

#![feature(
    async_closure,
    iterator_try_collect,
    iterator_try_reduce,
)]

pub mod capnp;

mod chunk;
pub mod index;

pub use chunk::{
    ChunkId, ChunkMeta, convert_store_page_to_dump_page_without_body, MappedChunk, MappedPage,
    StorePageId,
};

use anyhow::Context;
use derive_builder::UninitializedFieldError;
use rayon::prelude::*;
use std::{
    fmt::Debug,
    io::Write,
    path::PathBuf,
    result::Result as StdResult,
    sync::atomic::{AtomicI64, AtomicU64, Ordering},
    time::Instant,
};
use valuable::Valuable;
use wikimedia::{
    dump::{
        self,
        CategorySlug,
        DumpName,
        local::{FileSpec, JobFiles, OpenJobFile},
    },
    Error,
    Result,
    try2,
    util::fmt::{self, ByteRate, Bytes, Duration},
};

#[derive(Clone, Debug, Default)]
pub struct Options {
    dump_name: Option<DumpName>,
    max_chunk_len: Option<u64>,
    path: Option<PathBuf>,
}

struct OptionsBuilt {
    dump_name: DumpName,
    max_chunk_len: u64,
    path: PathBuf,
}

pub struct Store {
    chunk_store: chunk::Store,
    index: index::Index,
    opts: OptionsBuilt,
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

enum ImportEnd {
    PageLimit,
    Err(Error),
}

/// Analagous to the `std::try!(Result<T,E>)` macro but for use in `Store::import`'s
/// `try_for_each` closure, which returns Result<_, ImportEnd>.
macro_rules! try_import {
    ($val:expr) => {
        match $val {
            ::std::result::Result::Err(e) =>
                return ::std::result::Result::Err(ImportEnd::Err(e.into())),
            ::std::result::Result::Ok(v) => v,
        }
    }
}

pub const MAX_QUERY_LIMIT: u64 = 100;

impl Options {
    pub fn dump_name(&mut self, dump_name: DumpName) -> &mut Self {
        self.dump_name = Some(dump_name);
        self
    }

    pub fn path(&mut self, path: impl Into<PathBuf>) -> &mut Self {
        self.path = Some(path.into());
        self
    }

    /// Open an existing store or create a new one.
    pub fn build(&self) -> Result<Store> {
        let path = self.path.as_ref().cloned()
                       .ok_or_else(|| UninitializedFieldError::new("path"))?;
        let dump_name = self.dump_name.as_ref().cloned()
                            .ok_or_else(|| UninitializedFieldError::new("dump_name"))?;

        let opts = OptionsBuilt {
            dump_name: dump_name.clone(),
            max_chunk_len: self.max_chunk_len.unwrap_or(chunk::MAX_LEN_DEFAULT),
            path: path.clone(),
        };

        let index = index::Options {
            max_values_per_batch: 100,
            path: path.join("index"),
        }.build()?;

        let chunk_store = chunk::Options {
            dump_name: opts.dump_name.clone(),
            max_chunk_len: opts.max_chunk_len,
            path: path.join("chunks"),
        }.build()?;

        Ok(Store {
            chunk_store,
            index,

            // This moves opts into Store, so do that last.
            opts,
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

        let index = &self.index;

        let chunk_bytes_total = AtomicU64::new(0);
        let chunks_len = AtomicU64::new(0);
        let pages_total = AtomicU64::new(0);
        let total_source_bytes_read = AtomicU64::new(0);

        const PROGRESS_INTERVAL_SECS: i64 = 2;
        assert!(PROGRESS_INTERVAL_SECS > 0);

        let next_progress_ts = AtomicI64::new(
            chrono::Utc::now().timestamp()
             + PROGRESS_INTERVAL_SECS);

        let end = files.try_for_each(
            |file: Result<OpenJobFile>| -> StdResult<(), ImportEnd> {
                let OpenJobFile {
                    file_spec,
                    pages_iter,
                    source_bytes_read,
                    uncompressed_bytes_read,
                } = try_import!(file);

                let mut pages = pages_iter.peekable();

                while pages.peek().is_some() {
                    if let Some(limit) = job_files.open_spec().limit.as_ref().copied() {
                        if pages_total.load(Ordering::SeqCst) > limit {
                            return Err(ImportEnd::PageLimit);
                        }
                    }

                    let source_bytes_read_before = source_bytes_read.load(Ordering::SeqCst);

                    let chunk_builder = try_import!(chunk_write_guard.chunk_builder());
                    let index_batch_builder = try_import!(index.import_batch_builder());

                    let res = try_import!(
                        Self::import_chunk(&file_spec, &mut pages, chunk_builder,
                                           index_batch_builder)
                            .with_context(||
                                format!("While importing a chunk from file {file_spec:?} \
                                         source_bytes_read={source_bytes_read:?} \
                                         uncompressed_bytes_read={uncompressed_bytes_read:?}",
                                        source_bytes_read =
                                            Bytes(source_bytes_read.load(Ordering::SeqCst)),
                                        uncompressed_bytes_read =
                                            Bytes(uncompressed_bytes_read.load(
                                                Ordering::SeqCst)))));

                    // fetch_add counters.
                    let chunk_bytes_total_curr =
                        chunk_bytes_total.fetch_add(res.chunk_meta.bytes_len.0, Ordering::SeqCst);
                    let pages_total_curr = pages_total.fetch_add(res.chunk_meta.pages_len,
                                                                 Ordering::SeqCst);
                    let chunks_len_curr = chunks_len.fetch_add(1, Ordering::SeqCst);
                    let source_bytes_read_after = source_bytes_read.load(Ordering::SeqCst);
                    let source_bytes_read_diff =
                        source_bytes_read_after - source_bytes_read_before;
                    let total_source_bytes_read_curr =
                        total_source_bytes_read.fetch_add(source_bytes_read_diff,
                                                          Ordering::SeqCst);

                    let now = chrono::Utc::now();
                    let now_ts = now.timestamp();
                    let curr_next_progress_ts = next_progress_ts.load(Ordering::SeqCst);

                    if now_ts >= curr_next_progress_ts {
                        // The current time is after next_progress_ts, which is when
                        // we wanted to make the next update.
                        //
                        // So some thread should print an update.
                        // Do a compare exchange on next_progress_ts to determine
                        // if we're the first thread to notice an update is needed,
                        // and if so print a progress update.
                        let candidate_next_progress_ts = now_ts + PROGRESS_INTERVAL_SECS;
                        let cmp_res = next_progress_ts.compare_exchange(
                            curr_next_progress_ts,
                            candidate_next_progress_ts,
                            Ordering::SeqCst /* success */,
                            Ordering::SeqCst /* failure */);

                        if cmp_res.is_ok() {
                            // We succeded in the update, so we are
                            // the thread to print the current
                            // progress.
                            try_import!(Self::print_import_progress(start,
                                                                    &file_spec,
                                                                    chunk_bytes_total_curr,
                                                                    pages_total_curr,
                                                                    chunks_len_curr,
                                                                    total_source_bytes.0,
                                                                    total_source_bytes_read_curr,
                                                                    source_bytes_read_diff));
                        }
                    } // End check whether we should print progress.
                }; // Loop while there are more pages in the import file.

                tracing::debug!(input_file = %file_spec.path.display(),
                                "Finished importing from file");

                Ok(())
            }); // parallel for each over all files.

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

    fn print_import_progress(
        start: Instant,
        file_spec: &FileSpec,
        chunk_bytes_total_curr: u64,
        pages_total_curr: u64,
        chunks_len_curr: u64,
        total_source_bytes: u64,
        total_source_bytes_read_curr: u64,
        source_bytes_read_diff: u64,
     ) -> Result<()> {

        let now = chrono::Local::now();

        // Calculate derived stats.
        let percent_complete =
            ((total_source_bytes_read_curr as f64) /
             (total_source_bytes as f64)) * 100.0;

        let duration_so_far = start.elapsed();
        let total_source_bytes_remaining =
            total_source_bytes - total_source_bytes_read_curr;

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

        let eta: Option<String> = est_remaining_duration.and_then(
            |dur| -> Option<String> {
                let chrono_time =
                    now
                    + match chrono::Duration::from_std(dur.0) {
                        Ok(chrono_dur) => chrono_dur,
                        Err(_e) => return None,
                    };
                let s = fmt::chrono_time(chrono_time);
                Some(s)
            });

        let percent_complete_str = format!("{percent_complete:3.1}%");

        writeln!(std::io::stdout(),
                 "{now}     Import: \
                  {percent_complete_str:>6}\
                  {remaining_str}\
                  {eta}",
                 now = fmt::chrono_time(now),
                 remaining_str = match est_remaining_duration {
                     Some(dur) => format!("   remaining: {dur:>16}"),
                     None => "".to_string(),
                 },
                 eta = match eta {
                     Some(ref eta) => format!("   ETA: {eta}"),
                     None => "".to_string(),
                 })?;

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

            // This chunk stats
            input_file = %file_spec.path.display(),
            source_bytes_read = Bytes(source_bytes_read_diff).as_value(),
            // WIP: uncompressed_bytes_read = Bytes(uncompressed_bytes_read_diff.get()),
            "Chunk import done");

        Ok(())
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
