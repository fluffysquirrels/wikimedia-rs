//! To return pages rapidly by MediaWiki ID, page slug, or full text search
//! there are indexes implemented in this module that contain the serialised
//! page's location in a chunk file.

#[allow(dead_code)] // Not used at the moment, soon to be deleted.
mod sled;

use anyhow::{Context, format_err};
use crate::{
    dump,
    Error,
    Result,
    slug,
    store::StorePageId,
};
use rusqlite::{config::DbConfig, Connection, OpenFlags, Row, Transaction, TransactionBehavior};
use sea_query::{BlobSize, ColumnDef, enum_def, Expr, InsertStatement, OnConflict, Query,
                SimpleExpr, SqliteQueryBuilder, Table};
use sea_query_rusqlite::{RusqliteBinder, RusqliteValues};
use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};

#[derive(Debug)]
pub struct Index {
    conn: Mutex<Connection>,
    opts: Options,
}

#[derive(Debug)]
pub struct Options {
    pub max_values_per_batch: usize,
    pub path: PathBuf,
}

pub struct ImportBatchBuilder<'index>
{
    index: &'index Index,
    page_batch: BatchInsert,
    page_categories_batch: BatchInsert,
}

struct BatchInsert {
    built: Vec<(String, RusqliteValues)>,
    curr_num_values: usize,
    init_fn: Box<dyn Fn() -> InsertStatement>,
    max_batch_len: usize,
    statement: InsertStatement,
}

#[derive(Debug)]
#[enum_def]
#[allow(dead_code)] // PageIden (generated from this) is used.
struct Page {
    mediawiki_id: u64,
    store_id: StorePageId,
    slug: String,
}

#[derive(Debug)]
#[enum_def]
#[allow(dead_code)] // PageCategoriesIden (generated from this) is used.
struct PageCategories {
    mediawiki_id: u64,
    category_title: String,
}

impl Options {
    pub fn build(self) -> Result<Index> {

        fs::create_dir_all(&*self.path)?;
        let db_path = self.path.join("index.db");

        let open_flags =
            OpenFlags::SQLITE_OPEN_READ_WRITE |
            OpenFlags::SQLITE_OPEN_CREATE |
            OpenFlags::SQLITE_OPEN_URI |
            OpenFlags::SQLITE_OPEN_NO_MUTEX;
        let conn = Connection::open_with_flags(db_path, open_flags)?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true)?;
        // TODO: conn.trace()
        // TODO: pragmas.
        conn.pragma_update(None, "journal_mode", "WAL")?;

        let schema_sql = [
                Table::create()
                    .table(PageIden::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(PageIden::MediawikiId)
                            .integer()
                            .not_null()
                            .primary_key())
                    .col(ColumnDef::new(PageIden::StoreId)
                            .blob(BlobSize::Blob(Some(16)))
                            .not_null())
                    .col(ColumnDef::new(PageIden::Slug)
                            .text()
                            .not_null())
                    .build(SqliteQueryBuilder),
                sea_query::Index::create()
                    .name("index_page_by_slug")
                    .if_not_exists()
                    .table(PageIden::Table)
                    .col(PageIden::Slug)
                    .unique()
                    .build(SqliteQueryBuilder),
                Table::create()
                    .table(PageCategoriesIden::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(PageCategoriesIden::MediawikiId)
                             .integer()
                             .not_null())
                    .col(ColumnDef::new(PageCategoriesIden::CategoryTitle)
                             .text()
                             .not_null())
                    .primary_key(sea_query::Index::create()
                                     .col(PageCategoriesIden::MediawikiId)
                                     .col(PageCategoriesIden::CategoryTitle)
                                     .unique())
                    .build(SqliteQueryBuilder),
                sea_query::Index::create()
                    .name("index_page_categories_by_category_title")
                    .if_not_exists()
                    .table(PageCategoriesIden::Table)
                    .col(PageCategoriesIden::CategoryTitle)
                    .col(PageCategoriesIden::MediawikiId)
                    .unique()
                    .build(SqliteQueryBuilder),
            ]
            .join("; ");

        conn.execute_batch(&schema_sql)?;

        Ok(Index {
            conn: Mutex::new(conn),

            opts: self,
        })
    }
}

impl Index {
    pub fn clear(&mut self) -> Result<()> {
        let (delete_sql, params) =
                Query::delete()
                    .from_table(PageIden::Table)
                    .build_rusqlite(SqliteQueryBuilder);

        tracing::debug!(sql = delete_sql, "Index::clear() sql");
        self.conn()?.execute(&*delete_sql, &*params.as_params())
            .with_context(
                || "in Index::clear() while deleting rows from sqlite table page")?;
        self.conn()?.execute("VACUUM;", [])
            .with_context(
                || "in Index::clear() while vacuuming the database")?;
        Ok(())
    }

    pub fn optimise(&mut self) -> Result<()> {
        self.conn()?.execute("VACUUM;", [])
            .with_context(
                || "in Index::optimise() while vacuuming the database")?;
        Ok(())
    }

    fn conn(&self) -> Result<MutexGuard<Connection>> {
        self.conn.lock()
            .map_err(|_e: std::sync::PoisonError<_>|
                     format_err!("PoisonError locking connection mutex in store::Index"))
    }

    pub fn import_batch_builder<'index>(&'index self
    ) -> Result<ImportBatchBuilder<'index>> {
        Ok(ImportBatchBuilder::new(self))
    }

    #[tracing::instrument(level = "trace", skip(self), ret)]
    pub fn get_store_page_id_by_mediawiki_id(&self, id: u64) -> Result<Option<StorePageId>> {
        let (sql, params) = Query::select()
            .from(PageIden::Table)
            .column(PageIden::StoreId)
            .and_where(Expr::col(PageIden::MediawikiId).eq(id))
            .build_rusqlite(SqliteQueryBuilder);
        let params2 = &*params.as_params();

        let conn = self.conn()?;

        let store_id_bytes =
            match conn.query_row(&*sql, params2, |row: &Row| -> rusqlite::Result::<[u8; 16]>
                                                 { row.get(0) }) {
                Ok(x) => x,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
                Err(e) => return Err(Error::from(e)),
            };
        let store_id = StorePageId::try_from(store_id_bytes.as_slice())?;

        Ok(Some(store_id))
    }

    #[tracing::instrument(level = "trace", skip(self), ret)]
    pub fn get_store_page_id_by_slug(&self, slug: &str) -> Result<Option<StorePageId>> {
        let (sql, params) = Query::select()
            .from(PageIden::Table)
            .column(PageIden::StoreId)
            .and_where(Expr::col(PageIden::Slug).eq(slug))
            .build_rusqlite(SqliteQueryBuilder);
        let params2 = &*params.as_params();

        let conn = self.conn()?;

        let store_id_bytes =
            match conn.query_row(&*sql, params2, |row: &Row| -> rusqlite::Result::<[u8; 16]> {
                                                     row.get(0)
                                                 }) {
                Ok(x) => x,
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
                Err(e) => return Err(Error::from(e)),
            };

        let store_id = StorePageId::try_from(store_id_bytes.as_slice())?;

        Ok(Some(store_id))
    }
}

impl BatchInsert {
    fn new(init_fn: impl Fn() -> InsertStatement + 'static, max_batch_len: usize) -> BatchInsert {
        BatchInsert {
            built: Vec::new(),
            curr_num_values: 0,
            max_batch_len,
            statement: init_fn(),
            init_fn: Box::new(init_fn),
        }
    }

    fn push_values<I>(&mut self, values: I) -> Result<()>
        where I: IntoIterator<Item = SimpleExpr>
    {
        self.statement.values(values)?;

        self.curr_num_values += 1;

        if self.curr_num_values >= self.max_batch_len {
            let built_query = self.statement.build_rusqlite(SqliteQueryBuilder);
            self.built.push(built_query);
            self.curr_num_values = 0;
            let _old = std::mem::replace(&mut self.statement, (self.init_fn)());
        }

        Ok(())
    }

    fn execute_all(mut self, txn: &Transaction) -> Result<()> {
        if self.curr_num_values > 0 {
            let built_final = self.statement.build_rusqlite(SqliteQueryBuilder);
            self.built.push(built_final);
        }

        for (sql, params) in self.built.into_iter() {
            let params2 = params.as_params();
            txn.execute(&*sql, &*params2)?;
        }

        Ok(())
    }
}

impl<'index> ImportBatchBuilder<'index> {
    fn new(index: &'index Index) -> ImportBatchBuilder<'index> {
        ImportBatchBuilder {
            index,
            page_batch: BatchInsert::new(
                || Query::insert()
                       .into_table(PageIden::Table)
                       .columns([PageIden::MediawikiId, PageIden::StoreId, PageIden::Slug])
                       .on_conflict(OnConflict::new().do_nothing().to_owned())
                       .to_owned(),
                index.opts.max_values_per_batch),
            page_categories_batch: BatchInsert::new(
                || Query::insert()
                       .into_table(PageCategoriesIden::Table)
                       .columns([PageCategoriesIden::MediawikiId,
                                 PageCategoriesIden::CategoryTitle])
                       .to_owned(),
                index.opts.max_values_per_batch),
        }
    }

    pub fn push(&mut self, page: &dump::Page, store_page_id: StorePageId) -> Result<()> {
        let store_page_id_bytes = store_page_id.to_bytes();
        let page_slug = slug::page_title_to_slug(&*page.title);

        self.page_batch.push_values([
            page.id.into(),
            (store_page_id_bytes.as_slice()).into(),
            page_slug.into()
        ])?;

        if let Some(ref rev) = page.revision {
            for category in rev.categories.iter() {
                self.page_categories_batch.push_values([
                    page.id.into(),
                    category.0.clone().into(),
                ])?;
            }
        }

        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        let mut conn = self.index.conn()?;
        let txn = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        self.page_batch.execute_all(&txn)?;
        self.page_categories_batch.execute_all(&txn)?;

        txn.commit()?;

        Ok(())
    }
}
