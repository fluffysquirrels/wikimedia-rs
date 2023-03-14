use crate::{
    article_dump,
    fbs::wikimedia as wm,
    Result,
};
use flatbuffers::{self as fb, FlatBufferBuilder, ForwardsUOffset, WIPOffset};
use std::{
    ops::Deref,
    io::Write,
    path::PathBuf,
};

pub struct Options {
    pub path: PathBuf,
}

pub struct Store {
    opts: Options,
}

pub struct MappedChunk {
    _path: PathBuf,
    mmap: memmap2::Mmap,
    // bytes: &'a[u8],
    // chunk: wm::Chunk<'a>,
}

impl Options {
    pub fn from_common_args(common_args: &crate::args::CommonArgs) -> Options {
        Options {
            path: common_args.page_store_path(),
        }
    }

    pub fn build_store(self) -> Result<Store> {
        Store::new(self)
    }
}

impl Store {
    pub fn new(opts: Options) -> Result<Store> {
        Ok(Store {
            opts,
        })
    }

    pub fn import(&mut self, mut pages: impl Iterator<Item = Result<article_dump::Page>>
    ) -> Result<()> {
        self.import_inner(&mut pages)
    }

    fn import_inner(&mut self, pages: &mut dyn Iterator<Item = Result<article_dump::Page>>
    ) -> Result<()> {
        let fb_path = self.opts.path.join("articles.fbd");
        std::fs::create_dir_all(fb_path.parent().expect("parent of fb_path"))?;

        let mut page_fbs = Vec::<WIPOffset<wm::Page>>::with_capacity(100);
        let mut fbb = FlatBufferBuilder::with_capacity(1_000_000 /* 1 MB */);

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
        }

        fbb.start_vector::<WIPOffset<ForwardsUOffset<wm::Page>>>(page_fbs.len());
        for page_fb in page_fbs.iter() {
            let _page_off: WIPOffset<ForwardsUOffset<wm::Page>> = fbb.push(page_fb);
        }
        let pages_vec: WIPOffset<fb::Vector<ForwardsUOffset<wm::Page>>> =
            fbb.end_vector::<ForwardsUOffset<wm::Page>>(page_fbs.len());

        let mut chunk_b = wm::ChunkBuilder::new(&mut fbb);
        chunk_b.add_pages(pages_vec);
        let chunk = chunk_b.finish();

        wm::finish_size_prefixed_chunk_buffer(&mut fbb, chunk);
        let fb_out = fbb.finished_data();
        let mut file = std::fs::File::create(&*fb_path)?;
        file.write_all(fb_out)?;
        drop(file);
        drop(fbb);

        Ok(())
    }

    pub fn map_chunk(&mut self) -> Result<MappedChunk> {
        let path = self.opts.path.join("articles.fbd");

        let file = std::fs::File::open(&*path)?;
        let mmap = unsafe {
            memmap2::MmapOptions::new()
                // .populate()
                .map(&file)?
        };
        let bytes: &[u8] = mmap.deref();

        // Runs verifier.
        let _chunk = wm::size_prefixed_root_as_chunk(bytes)?;

        Ok(MappedChunk {
            _path: path,
            mmap,
            // bytes,
            // chunk,
        })
    }
}

impl MappedChunk {
    pub fn get_page(&self, idx: usize) -> Result<article_dump::Page> {
        let chunk = unsafe { self.chunk_unchecked() };
        if idx >= chunk.pages().len() {
            return Err(anyhow::Error::msg("MappedChunk::get_page index out of bounds."));
        }

        let page_fb = chunk.pages().get(idx);
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
