use crate::{
    article_dump,
    fbs::wikimedia as wm,
    Result,
    TempDir,
};
use flatbuffers::{self as fb, FlatBufferBuilder, ForwardsUOffset, WIPOffset};
use std::{
    io::Write,
    ops::Deref,
    path::PathBuf,
    time::{Duration, Instant},
};

pub struct Options {
    pub path: PathBuf,
    pub max_chunk_len: usize,
}

pub struct Store {
    opts: Options,
    next_chunk_id: u64,
    temp_dir: TempDir,
}

#[derive(Clone, Copy, Debug)]
pub struct StorePageId {
    chunk_id: ChunkId,
    page_chunk_idx: PageChunkIndex,
}

#[derive(Clone, Copy, Debug)]
struct ChunkId(u64);

#[derive(Clone, Copy, Debug)]
struct PageChunkIndex(u64);

struct MappedChunk {
    _path: PathBuf,
    mmap: memmap2::Mmap,
}

#[derive(Clone, Debug)]
struct ChunkMeta {
    bytes_len: u64,
    pages_len: u64,
    _path: PathBuf,
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

impl std::str::FromStr for StorePageId {
    type Err= anyhow::Error;

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
        Ok(Store {
            temp_dir: TempDir::create(&*opts.path, /* keep: */ false)?,
            opts,
            next_chunk_id: 0,
        })
    }

    pub fn clear(&mut self) -> Result<()> {
        let chunks_path = self.chunks_path();
        Ok(std::fs::remove_dir_all(&*chunks_path)?)
    }

    pub fn import(&mut self, pages: impl Iterator<Item = Result<article_dump::Page>> + 'static
    ) -> Result<ImportResult> {
        // import_inner takes a `Box<dyn Iterator>` so we don't have to generate
        // many versions of the whole body.
        self.import_inner(Box::new(pages))
    }

    fn import_inner(&mut self, pages: Box<dyn Iterator<Item = Result<article_dump::Page>>>
    ) -> Result<ImportResult> {
        // TODO: This is dumb but OK for testing, do something smarter later.
        self.clear()?;

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

        for page in pages {
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

            let fbb_curr_len = fbb.unfinished_data().len();
            if fbb_curr_len > self.opts.max_chunk_len {
                break;
            }
        }

        let pages_len = page_fbs.len();

        fbb.start_vector::<WIPOffset<ForwardsUOffset<wm::Page>>>(pages_len);
        for page_fb in page_fbs.into_iter() {
            let _page_off: WIPOffset<ForwardsUOffset<wm::Page>> = fbb.push(page_fb);
        }
        let pages_vec: WIPOffset<fb::Vector<ForwardsUOffset<wm::Page>>> =
            fbb.end_vector::<ForwardsUOffset<wm::Page>>(pages_len);

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

        // TODO: Add chunk to store metadata, including path, ChunkId,
        // count of pages, low page.id, high page.id.

        // TODO: Update index of page.id -> StorePageId.

        let res = ImportChunkResult {
            chunk_meta: ChunkMeta {
                bytes_len: bytes_len.try_into().expect("Convert usize to u64"),
                pages_len: pages_len.try_into().expect("Convert usize to u64"),
                _path: fb_out_path,
            },
            _duration: start.elapsed(),
        };

        tracing::debug!(?res,
                        "Chunk imported");

        Ok(res)
    }

    pub fn get_page_by_store_id(&self, id: StorePageId) -> Result<article_dump::Page> {
        let chunk = self.map_chunk(id.chunk_id)?;
        chunk.get_page(id.page_chunk_idx)
    }

    fn map_chunk(&self, chunk_id: ChunkId) -> Result<MappedChunk> {
        let path = self.chunk_path(chunk_id);

        let file = std::fs::File::open(&*path)?;
        let mmap = unsafe {
            memmap2::MmapOptions::new()
                .map(&file)?
        };
        let bytes: &[u8] = mmap.deref();

        // Runs verifier, subsequent loads will not.
        let _chunk = wm::size_prefixed_root_as_chunk(bytes)?;

        Ok(MappedChunk {
            _path: path,
            mmap,
        })
    }

    fn chunks_path(&self) -> PathBuf {
        self.opts.path.join("chunks")
    }

    fn chunk_path(&self, chunk_id: ChunkId) -> PathBuf {
        self.chunks_path().join(format!("articles-{id:016x}.fbd", id = chunk_id.0))
    }

    fn next_chunk_id(&mut self) -> ChunkId {
        let next = self.next_chunk_id;
        self.next_chunk_id += 1;
        ChunkId(next)
    }
}

impl MappedChunk {
    fn get_page(&self, idx: PageChunkIndex) -> Result<article_dump::Page> {
        let chunk = unsafe { self.chunk_unchecked() };
        if (idx.0 as usize) >= chunk.pages().len() {
            return Err(anyhow::Error::msg("MappedChunk::get_page index out of bounds."));
        }

        let page_fb = chunk.pages().get(idx.0 as usize);
        store_page_to_dump_page(page_fb)
    }

    unsafe fn chunk_unchecked(&self) -> wm::Chunk {
        let bytes = self.mmap.deref();
        wm::size_prefixed_root_as_chunk_unchecked(bytes)
    }
}

fn store_page_to_dump_page(page_fb: wm::Page) -> Result<article_dump::Page> {
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
