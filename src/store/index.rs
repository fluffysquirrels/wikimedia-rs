//! To return pages rapidly by MediaWiki ID, page slug, or full text search
//! there are indexes implemented in this module that contain the serialised
//! page's location in a chunk file.

#[allow(dead_code)] // Not used at the moment, soon to be deleted.
mod sled;

use anyhow::format_err;
use crate::{
    dump,
    Error,
    Result,
    slug,
    store::StorePageId,
};
use rusqlite::{config::DbConfig, Connection, OpenFlags, Row, TransactionBehavior};
use sea_query::{BlobSize, ColumnDef, enum_def, Expr, InsertStatement, Query,
                SqliteQueryBuilder, Table};
use sea_query_rusqlite::RusqliteBinder;
use std::{
    fs,
    path::PathBuf,
    sync::{Mutex, MutexGuard},
};

#[derive(Debug)]
pub struct Index {
    conn: Mutex<Connection>,

    #[allow(dead_code)] // Not used yet.
    opts: Options,
}

#[derive(Debug)]
pub struct Options {
    pub path: PathBuf,
}

#[derive(Debug)]
pub struct ImportBatchBuilder<'index> {
    index: &'index Index,
    query_builder: InsertStatement,
}

#[derive(Debug)]
#[enum_def]
#[allow(dead_code)] // PageIden (generated from this) is used.
struct Page {
    mediawiki_id: u64,
    store_id: StorePageId,
    slug: String,
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
        self.conn()?.execute(&*delete_sql, &*params.as_params())?;

        Ok(())
    }

    fn conn(&self) -> Result<MutexGuard<Connection>> {
        self.conn.lock()
            .map_err(|_e: std::sync::PoisonError<_>|
                     format_err!("PoisonError locking connection mutex in store::Index"))
    }

    pub fn import_batch_builder<'index>(&'index self) -> Result<ImportBatchBuilder<'index>> {
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

impl<'index> ImportBatchBuilder<'index> {
    fn new(index: &'index Index) -> ImportBatchBuilder<'index> {
        let mut query_builder = Query::insert();
        query_builder
            .into_table(PageIden::Table)
            .columns([PageIden::MediawikiId, PageIden::StoreId, PageIden::Slug]);

        ImportBatchBuilder {
            index,
            query_builder,
        }
    }

    pub fn push(&mut self, page: &dump::Page, store_page_id: StorePageId) -> Result<()> {
        let store_page_id_bytes = store_page_id.to_bytes();
        let page_slug = slug::page_title_to_slug(&*page.title);

        self.query_builder
            .values([page.id.into(), (store_page_id_bytes.as_slice()).into(), page_slug.into()])?;

        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        let (sql, params) = self.query_builder.build_rusqlite(SqliteQueryBuilder);
        let params2 = &*params.as_params();

        let mut conn = self.index.conn()?;
        let txn = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        txn.execute(&*sql, params2)?;
        txn.commit()?;

        Ok(())
    }
}
