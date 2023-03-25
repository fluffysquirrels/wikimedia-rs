//! To return pages rapidly by MediaWiki ID, page slug, or full text search
//! there are indexes implemented in this module that contain the serialised
//! page's location in a chunk file.

#[allow(dead_code)] // Not used at the moment, soon to be deleted.
mod sled;

use anyhow::{Context, format_err};
use crate::{
    dump::{self, CategorySlug},
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
pub(in crate::store) struct Index {
    /// An open connection to the sqlite database. Always `Some(_)`
    /// except for briefly during `Index::clear()`.
    conn: Option<Mutex<Connection>>,
    opts: Options,
}

#[derive(Debug)]
pub(in crate::store) struct Options {
    pub max_values_per_batch: usize,
    pub path: PathBuf,
}

pub(in crate::store) struct ImportBatchBuilder<'index>
{
    index: &'index Index,
    category_batch: BatchInsert,
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

#[derive(Clone, Debug)]
#[enum_def]
pub struct Page {
    pub mediawiki_id: u64,
    pub store_id: StorePageId,
    pub slug: String,
}

#[derive(Clone, Debug)]
#[enum_def]
#[allow(dead_code)] // PageCategoriesIden (generated from this) is used.
struct PageCategories {
    mediawiki_id: u64,
    category_slug: String,
}

#[derive(Debug)]
#[enum_def]
#[allow(dead_code)] // CategoryIden (generated from this) is used.
struct Category {
    slug: String,
}

pub const MAX_LIMIT: u64 = 100;

impl Options {
    pub fn build(self) -> Result<Index> {
        Index::new(self)
    }
}

impl Index {
    fn new(opts: Options) -> Result<Index> {
        let conn = Self::new_conn(&opts)?;

        let mut index = Index {
            conn: Some(Mutex::new(conn)),

            opts: opts,
        };

        index.ensure_schema()?;

        Ok(index)
    }

    fn new_conn(opts: &Options) -> Result<Connection> {
        fs::create_dir_all(&*opts.path)?;
        let db_path = opts.path.join("index.db");

        let open_flags =
            OpenFlags::SQLITE_OPEN_READ_WRITE |
            OpenFlags::SQLITE_OPEN_CREATE |
            OpenFlags::SQLITE_OPEN_URI |
            OpenFlags::SQLITE_OPEN_NO_MUTEX;

        let mut conn = Connection::open_with_flags(db_path, open_flags)?;

        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_DEFENSIVE, true)?;
        conn.set_db_config(DbConfig::SQLITE_DBCONFIG_ENABLE_FKEY, true)?;

        conn.trace(Some(|s: &str| tracing::debug!(sql = s, "Index::conn::trace")));

        // TODO: more safety pragmas.
        conn.pragma_update(None, "journal_mode", "WAL")?;

        Ok(conn)
    }

    fn ensure_schema(&mut self) -> Result<()> {
        let schema_sql = [
                // Table category
                Table::create()
                    .table(CategoryIden::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(CategoryIden::Slug)
                             .text()
                             .not_null()
                             .extra("COLLATE NOCASE".to_string())
                             .primary_key())
                    .build(SqliteQueryBuilder)
                + " WITHOUT ROWID",

                // Table page
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
                            .not_null()
                            .extra("COLLATE NOCASE".to_string()))
                    .build(SqliteQueryBuilder),
                sea_query::Index::create()
                    .name("index_page_by_slug")
                    .if_not_exists()
                    .table(PageIden::Table)
                    .col(PageIden::Slug)
                    .unique()
                    .build(SqliteQueryBuilder),

                // Table page_categories
                Table::create()
                    .table(PageCategoriesIden::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(PageCategoriesIden::MediawikiId)
                             .integer()
                             .not_null())
                    .col(ColumnDef::new(PageCategoriesIden::CategorySlug)
                             .text()
                             .not_null()
                             .extra("COLLATE NOCASE".to_string()))
                    .primary_key(sea_query::Index::create()
                                     .col(PageCategoriesIden::MediawikiId)
                                     .col(PageCategoriesIden::CategorySlug)
                                     .unique())
                    .build(SqliteQueryBuilder),
                sea_query::Index::create()
                    .name("index_page_categories_by_category_slug")
                    .if_not_exists()
                    .table(PageCategoriesIden::Table)
                    .col(PageCategoriesIden::CategorySlug)
                    .col(PageCategoriesIden::MediawikiId)
                    .unique()
                    .build(SqliteQueryBuilder),
            ]
            .join("; ");

        self.conn()?.execute_batch(&schema_sql)?;

        Ok(())
    }

    fn drop_all(&mut self) -> Result<()> {
        let drop_sql = [
                Table::drop()
                    .table(CategoryIden::Table)
                    .if_exists()
                    .build(SqliteQueryBuilder),
                Table::drop()
                    .table(PageCategoriesIden::Table)
                    .if_exists()
                    .build(SqliteQueryBuilder),
                Table::drop()
                    .table(PageIden::Table)
                    .if_exists()
                    .build(SqliteQueryBuilder),
            ]
            .join("; ");

        self.conn()?.execute_batch(&drop_sql)?;

        Ok(())
    }

    pub fn clear(&mut self) -> Result<()> {
        self.drop_all()
            .with_context(
                || "in Index::clear() while dropping all objects")?;
        self.optimise()?;

        // Drop old connection. Closing a sqlite connection seems to
        // help reduce DB size after dropping all the tables.
        if let Some(conn /* : Mutex<Connection> */) = self.conn.take() {
            conn.into_inner()
                .map_err(|_e: std::sync::PoisonError<_>|
                         format_err!("PoisonError locking connection mutex in store::Index"))?
                .close()
                .map_err(|(_conn, err)| err)?;
        }

        // Create new connection.
        let _ = self.conn.insert(Mutex::new(Self::new_conn(&self.opts)?));

        self.ensure_schema()
            .with_context(
                || "in Index::clear() while creating the schame")?;

        Ok(())
    }

    pub fn optimise(&mut self) -> Result<()> {
        self.conn()?.execute("VACUUM;", [])
            .with_context(
                || "in Index::optimise() while vacuuming the database")?;
        Ok(())
    }

    fn conn(&self) -> Result<MutexGuard<Connection>> {
        self.conn.as_ref().ok_or_else(|| format_err!("self.conn is None"))?
            .lock()
            .map_err(|_e: std::sync::PoisonError<_>|
                     format_err!("PoisonError locking connection mutex in store::Index"))
    }

    pub fn import_batch_builder<'index>(&'index self
    ) -> Result<ImportBatchBuilder<'index>> {
        Ok(ImportBatchBuilder::new(self))
    }

    pub fn get_category(&self, slug_lower_bound: Option<&CategorySlug>, limit: Option<u64>
    ) -> Result<Vec<dump::CategorySlug>>
    {
        let limit = limit.unwrap_or(MAX_LIMIT).min(MAX_LIMIT);

        let (sql, params) = Query::select()
            .from(CategoryIden::Table)
            .column(CategoryIden::Slug)
            .limit(limit)
            .and_where_option(slug_lower_bound.map(
                |lower| Expr::col(CategoryIden::Slug).gt(lower.0.as_str())))
            .build_rusqlite(SqliteQueryBuilder);
        let params2 = &*params.as_params();

        let conn = self.conn()?;
        let mut statement = conn.prepare_cached(&*sql)?;
        let mut rows = statement.query(params2)?;

        let mut out = Vec::with_capacity(limit.try_into().expect("u64 to usize"));

        while let Some(row) = rows.next()? {
            let slug = row.get_ref(0)?
                          .as_str()?;
            out.push(dump::CategorySlug(slug.to_string()));
        }

        Ok(out)
    }

    pub fn get_category_pages(
        &self,
        slug: &CategorySlug,
        page_mediawiki_id_lower_bound: Option<u64>,
        limit: Option<u64>,
    ) -> Result<Vec<Page>>
    {
        let limit = limit.unwrap_or(MAX_LIMIT).min(MAX_LIMIT);

        let (sql, params) = Query::select()
            .column((PageIden::Table, PageIden::MediawikiId))
            .column((PageIden::Table, PageIden::StoreId))
            .column((PageIden::Table, PageIden::Slug))
            .from(PageCategoriesIden::Table)
            .inner_join(PageIden::Table,
                        Expr::col((PageCategoriesIden::Table, PageCategoriesIden::MediawikiId))
                            .equals((PageIden::Table, PageIden::MediawikiId)))
            .and_where(Expr::col((PageCategoriesIden::Table, PageCategoriesIden::CategorySlug))
                           .eq(&*slug.0))
            .and_where_option(page_mediawiki_id_lower_bound.map(
                |id|
                Expr::col((PageCategoriesIden::Table, PageCategoriesIden::MediawikiId))
                    .gt(id)))
            .limit(limit)
            .build_rusqlite(SqliteQueryBuilder);
        let params2 = &*params.as_params();

        tracing::debug!(sql, "get_category_pages query");

        let conn = self.conn()?;
        let mut statement = conn.prepare_cached(&*sql)?;
        let mut rows = statement.query(params2)?;

        let mut out = Vec::<Page>::with_capacity(limit.try_into().expect("u64 to usize"));

        while let Some(row) = rows.next()? {
            let page = Page {
                mediawiki_id: row.get(0)?,
                store_id: Self::row_to_store_id(&row, 1)?,
                slug: row.get(2)?,
            };

            out.push(page);
        }

        Ok(out)
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

        let store_id =
            conn.query_row_and_then(
                &*sql, params2,
                |row: &Row|
                Self::row_to_store_id(row, 0))?;

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

        let store_id =
            conn.query_row_and_then(
                &*sql, params2,
                |row: &Row|
                Self::row_to_store_id(row, 0))?;

        Ok(Some(store_id))
    }

    fn row_to_store_id(row: &Row, index: usize) -> Result<StorePageId> {
        let store_id_bytes: [u8; 16] = row.get(index)?;
        let store_id = StorePageId::try_from(store_id_bytes.as_slice())?;
        Ok(store_id)
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
            category_batch: BatchInsert::new(
                || Query::insert()
                       .into_table(CategoryIden::Table)
                       .columns([CategoryIden::Slug])
                       .on_conflict(OnConflict::new().do_nothing().to_owned())
                       .to_owned(),
                index.opts.max_values_per_batch),
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
                                 PageCategoriesIden::CategorySlug])
                       .to_owned(),
                index.opts.max_values_per_batch),
        }
    }

    pub fn push(&mut self, page: &dump::Page, store_page_id: StorePageId) -> Result<()> {
        let store_page_id_bytes = store_page_id.to_bytes();
        let page_slug = slug::title_to_slug(&*page.title);

        self.page_batch.push_values([
            page.id.into(),
            (store_page_id_bytes.as_slice()).into(),
            page_slug.into()
        ])?;

        if let Some(ref rev) = page.revision {
            for category_name in rev.categories.iter() {
                self.category_batch.push_values([
                    category_name.to_slug().0.into(),
                ])?;
                self.page_categories_batch.push_values([
                    page.id.into(),
                    category_name.to_slug().0.into(),
                ])?;
            }
        }

        Ok(())
    }

    pub fn commit(self) -> Result<()> {
        let mut conn = self.index.conn()?;
        let txn = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;

        self.category_batch.execute_all(&txn)?;
        self.page_batch.execute_all(&txn)?;
        self.page_categories_batch.execute_all(&txn)?;

        txn.commit()?;

        Ok(())
    }
}
