//! Index implementation using sled. To replace.

use crate::{
    dump,
    Result,
    slug,
    store::StorePageId,
    util::fmt::Bytes,
};
use std::{
    path::PathBuf,
};
use tracing::Level;
use valuable::Valuable;

pub struct Index {
    #[allow(dead_code)] // Not used yet.
    opts: Options,
    sled_db: sled::Db,
}

pub struct Options {
    pub path: PathBuf,
}

pub struct ImportBatchBuilder<'index> {
    by_id_batch: sled::Batch,
    by_slug_batch: sled::Batch,
    index: &'index Index,
}

impl Options {
    pub fn build(self) -> Result<Index> {
        let sled_db =
            tracing::debug_span!("index::Options::build() opening sled_db",
                                 sled_path = %self.path.display())
                .in_scope(||
                          sled::Config::default()
                              .path(&*self.path)
                              .open())?;

        if tracing::enabled!(Level::DEBUG) {
            let tree_names = sled_db.tree_names()
                                    .into_iter()
                                    .map(|bytes| -> String {
                                         String::from_utf8_lossy(&*bytes).into_owned()
                                    })
                                    .collect::<Vec<String>>();

            tracing::debug!(sled_db.trees = ?tree_names,
                            sled_db.size_on_disk = Bytes(sled_db.size_on_disk()?).as_value(),
                            sled_db.was_recovered = sled_db.was_recovered(),
                            "Store::new() loaded");
        }

        Ok(Index {
            sled_db,

            opts: self,
        })
    }
}

impl Index {
    pub fn clear(&self) -> Result<()> {
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

    pub fn import_batch_builder<'index>(&'index self) -> Result<ImportBatchBuilder<'index>> {
        Ok(ImportBatchBuilder {
            by_id_batch: sled::Batch::default(),
            by_slug_batch: sled::Batch::default(),
            index: self,
        })
    }

    pub fn get_store_page_id_by_mediawiki_id(&self, id: u64) -> Result<Option<StorePageId>> {
        let by_id = self.get_tree_store_page_id_by_mediawiki_id()?;
        let store_page_id = try2!(by_id.get(&id.to_be_bytes()));
        let store_page_id = StorePageId::try_from(&*store_page_id)?;
        Ok(Some(store_page_id))
    }

    pub fn get_store_page_id_by_slug(&self, slug: &str) -> Result<Option<StorePageId>> {
        let by_slug = self.get_tree_store_page_id_by_slug()?;
        let store_page_id = try2!(by_slug.get(slug));
        let store_page_id = StorePageId::try_from(&*store_page_id)?;
        Ok(Some(store_page_id))
    }

    fn get_tree_store_page_id_by_slug(&self) -> Result<sled::Tree> {
        Ok(self.sled_db.open_tree(b"store_page_id_by_slug")?)
    }

    fn get_tree_store_page_id_by_mediawiki_id(&self) -> Result<sled::Tree> {
        Ok(self.sled_db.open_tree(b"store_page_id_by_mediawiki_id")?)
    }
}

impl<'index> ImportBatchBuilder<'index> {
    pub fn push(&mut self, page: &dump::Page, store_page_id: StorePageId) -> Result<()> {
        let store_page_id_bytes = store_page_id.to_bytes();
        let page_slug = slug::page_title_to_slug(&*page.title);
        self.by_slug_batch.insert(&*page_slug, &store_page_id_bytes);

        self.by_id_batch.insert(&page.id.to_be_bytes(), &store_page_id_bytes);

        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        let by_slug = self.index.get_tree_store_page_id_by_slug()?;
        by_slug.apply_batch(self.by_slug_batch)?;
        by_slug.flush()?;

        let by_id = self.index.get_tree_store_page_id_by_mediawiki_id()?;
        by_id.apply_batch(self.by_id_batch)?;
        by_id.flush()?;

        Ok(())
    }
}
