use askama::Template;
use axum::{
    extract::{Path, Query, State},
    headers::ContentType,
    http::{header, status::StatusCode, uri},
    response::{IntoResponse, Response},
    Router,
    routing,
    Server,
    TypedHeader,
};
use crate::args::CommonArgs;
use futures::future::{self, Either};
use serde::Deserialize;
use std::{
    any::Any,
    fmt::{self, Display},
    future::Future,
    net::SocketAddr,
    result::Result as StdResult,
    sync::{Arc, MutexGuard},
};
use tower_http::{
    catch_panic::CatchPanicLayer,
    sensitive_headers::SetSensitiveHeadersLayer,
    trace::TraceLayer,
};
use wikimedia::{
    dump::{self, CategorySlug},
    slug,
    Result,
    wikitext,
};
use wikimedia_store::{self as store, index, StorePageId};


/// Run a web server that returns Wikimedia content.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,
}

type WebResult<T> = StdResult<T, WebError>;

mod state {
    use anyhow::{ensure, format_err};
    use std::sync::{Mutex, MutexGuard};
    use super::Args;
    use wikimedia::{dump::DumpName, Result};
    use wikimedia_store::Store;

    pub struct WebState {
        args: Args,
        store: Mutex<Store>,
        store_dump_name: DumpName,
    }

    impl WebState {
        pub fn new(args: Args) -> Result<WebState> {
            let store = args.common.store_options()?.build()?;

            Ok(WebState {
                store: Mutex::new(store),
                store_dump_name: args.common.store_dump_name().clone(),

                // This moves `args`, so do it last.
                args,
            })
        }

        pub fn args(&self) -> &Args {
            &self.args
        }

        pub fn store<'state>(&'state self, dump_name: &str
        ) -> Result<MutexGuard<'state, Store>>
        {
            ensure!(dump_name == &*self.store_dump_name.0,
                    "WebState::store() error: Dump name requested ({dump_name}) \
                     is not the same as the loaded store's dump name ({store_dump_name})",
                    store_dump_name = &*self.store_dump_name.0);

            Ok(self.store.lock()
                   .map_err(|_err| format_err!("PoisonError unlocking Mutex in web module"))?)
        }

        pub fn store_dump_name(&self) -> DumpName {
            self.store_dump_name.clone()
        }
    }
}

use state::WebState;

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let state = Arc::new(WebState::new(args.clone())?);

    let app = Router::new()
        .route("/", routing::get(get_index))
        .route("/:dump_name/category", routing::get(get_categories))
        .route("/:dump_name/category/by-name/:category_slug",
               routing::get(get_category_by_slug))

        .route("/:dump_name/page/by-id/:page_id", routing::get(get_page_by_id))
        .route("/:dump_name/page/by-store-id/:page_store_id", routing::get(get_page_by_store_id))
        .route("/:dump_name/page/by-title/:page_slug", routing::get(get_page_by_slug))

        .route("/page/search", routing::get(get_page_search))

        .route("/test_panic", routing::get(|| async { panic!("Test panic") }))

        // .layer(
        //     tower::ServiceBuilder::new()
        //         .layer(HandleErrorLayer::new(oops))
        // )

        .with_state(state)

        // Lower layers run first.
        .layer(tower::ServiceBuilder::new()
                   .layer(SetSensitiveHeadersLayer::new(vec![header::AUTHORIZATION]))
                   .layer(TraceLayer::new_for_http())
                   .layer(CatchPanicLayer::custom(handle_panic))
                );

    let port: u16 = 8089;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let url = uri::Builder::new()
                           .scheme(uri::Scheme::HTTP)
                           .authority(format!("localhost:{port}"))
                           .path_and_query("/")
                           .build()?;
    tracing::info!(%url,
                   "Listening on http");

    Server::bind(&addr)
           .serve(app.into_make_service_with_connect_info::<SocketAddr>())
           .await?;

    Ok(())
}

struct WebError(Response);

impl WebError {
    fn from_std_error<E>(e: E) -> WebError
        where E: std::error::Error + Send + Sync + 'static
    {
        let anyhow_err = anyhow::Error::new(e);
        WebError::from(anyhow_err)
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        self.0
    }
}

impl From<anyhow::Error> for WebError {
    fn from(e: anyhow::Error) -> WebError {
        WebError(_500_response(&&*format!("Error: {e:#}")))
    }
}

impl From<fmt::Error> for WebError {
    fn from(e: fmt::Error) -> WebError {
        WebError::from_std_error(e)
    }
}

impl<T> From<std::sync::PoisonError<MutexGuard<'_, T>>> for WebError {
    fn from(_e: std::sync::PoisonError<MutexGuard<'_, T>>) -> WebError {
        // PoisonError is from trying to unlock a poisoned mutex. It
        // contains the MutexGuard in case you want to clear the poison and continue.
        // However MutexGuard is not Send, and axum wants errors from handlers to be Send.
        // So we special case this conversion from sync::PoisonError<MutexGuard>
        // to ignore the inner value and make sure we are Send.
        WebError(_500_response(&"PoisonError unlocking Mutex in web module"))
    }
}

#[cfg(any())]
impl<E> From<E> for WebError
    where E: std::error::Error + Send + Sync + 'static
{
    fn from(e: E) -> WebError {
        WebError(_500_response(&*format!("Error: {e}")))
    }
}

#[derive(askama::Template)]
#[template(path = "error.html")]
struct ErrorHtml<'a> {
    title: &'static str,
    message: &'a str,
}

fn _500_response(msg: &dyn Display) -> Response {
    error_response("Error", msg, StatusCode::INTERNAL_SERVER_ERROR)
}

fn _404_response(msg: &dyn Display) -> Response {
    error_response("Not found", msg, StatusCode::NOT_FOUND)
}

fn error_response(title: &'static str, msg: &dyn Display, status: StatusCode) -> Response {
    let msg = msg.to_string();

    let template = ErrorHtml {
        title: title,
        message: &*msg,
    };

    let html = match template.render() {
        Ok(html) => html,
        Err(e) => format!(
            "<html>\
             <head>\
             <title>{title}</title>\
             </head>\
             <body>\
             <h1>{title}</h1>\
             <pre>{msg}</pre>\
             <p>Additional error rendering the error:</p>\
             <pre>{e}</pre>\
             </body>\
             </html>"),
    };

    (
        status,
        TypedHeader(ContentType::html()),
        html,
    ).into_response()
}

fn handle_panic(err: Box<dyn Any + Send + 'static>) -> Response {
    let s = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "Unknown panic message".to_string()
    };

    tracing::error!("panic: {s}");

    _500_response(&format!("panic: {s}"))
}

#[derive(askama::Template)]
#[template(path = "index.html")]
struct IndexHtml<'a> {
    title: &'a str,
}

async fn get_index() -> impl IntoResponse {
    IndexHtml {
        title: "Index",
    }
}

#[derive(Deserialize)]
struct GetCategoryQuery {
    limit: Option<u64>,
    slug_lower_bound: Option<String>,
}

#[derive(askama::Template)]
#[template(path = "categories.html")]
struct CategoriesHtml<'a> {
    title: &'a str,
    dump_name: String,

    categories: Vec<CategorySlug>,
    show_more_href: Option<String>,
}

async fn get_categories(
    State(state): State<Arc<WebState>>,
    Path(dump_name): Path<String>,
    Query(query): Query<GetCategoryQuery>
) -> WebResult<impl IntoResponse> {

    let limit = query.limit.unwrap_or(store::MAX_QUERY_LIMIT).min(store::MAX_QUERY_LIMIT);

    let categories = state.store(&*dump_name)?
        .get_category(
            query.slug_lower_bound.as_ref().map(|s| CategorySlug(s.clone())).as_ref(),
            Some(limit))?;

    let last_slug = categories.last().cloned();
    let len = u64::try_from(categories.len()).expect("u64 from usize");

    let show_more_href =
        if let Some(CategorySlug(slug_lower_bound)) = last_slug {
            if limit == len {
                let limit_pair = match query.limit {
                    Some(limit) => format!("&limit={}", limit),
                    None => "".to_string(),
                };

                Some(format!(
                    "/{dump_name}/category?slug_lower_bound={slug_lower_bound}{limit_pair}"))
            } else { None }
        } else { None };

    Ok(CategoriesHtml {
        title: "Categories",
        dump_name,

        categories,
        show_more_href,
    })
}

#[derive(Deserialize)]
struct GetCategoryBySlugQuery {
    limit: Option<u64>,
    page_mediawiki_id_lower_bound: Option<u64>,
}

#[derive(askama::Template)]
#[template(path = "category.html")]
struct CategoryHtml {
    title: String,
    dump_name: String,

    pages: Vec<index::Page>,
    show_more_href: Option<String>,
}

async fn get_category_by_slug(
    State(state): State<Arc<WebState>>,
    Path((dump_name, category_slug)): Path<(String, String)>,
    Query(query): Query<GetCategoryBySlugQuery>,
) -> WebResult<impl IntoResponse> {

    let limit = query.limit.unwrap_or(store::MAX_QUERY_LIMIT).min(store::MAX_QUERY_LIMIT);

    let store = state.store(&*dump_name)?;
    let pages: Vec<index::Page> = store.get_category_pages(
        &CategorySlug(category_slug.clone()),
        query.page_mediawiki_id_lower_bound,
        Some(limit),
    )?;

    // Drop the MutexGuard.
    drop(store);

    let page_mediawiki_id_lower_bound = pages.last().map(|page| page.mediawiki_id);
    let len = u64::try_from(pages.len()).expect("u64 from usize");

    let show_more_href =
        if let Some(page_mediawiki_id_lower_bound) = page_mediawiki_id_lower_bound {
            if len == limit {
                let limit_pair = match query.limit {
                    Some(limit) => format!("&limit={}", limit),
                    None => "".to_string(),
                };

                Some(format!("/{dump_name}/category/by-name/{category_slug}\
                              ?page_mediawiki_id_lower_bound={page_mediawiki_id_lower_bound}\
                              {limit_pair}"))
            } else { None }
        } else { None };

    Ok(CategoryHtml {
        title: format!("Category:{category_slug}"),
        dump_name,

        pages,
        show_more_href,
    })
}

#[derive(Deserialize)]
struct SinglePageQuery {
    debug: Option<bool>,
}

async fn get_page_by_id(
    State(state): State<Arc<WebState>>,
    Path((dump_name, page_id)): Path<(String, u64)>,
    Query(query): Query<SinglePageQuery>,
) -> WebResult<impl IntoResponse> {

    let page = state.store(&*dump_name)?.get_page_by_mediawiki_id(page_id)?;

    response_from_mapped_page(page, &*state, query).await
}

async fn get_page_by_store_id(
    State(state): State<Arc<WebState>>,
    Path((dump_name, page_store_id)): Path<(String, String)>,
    Query(query): Query<SinglePageQuery>,
) -> WebResult<impl IntoResponse> {

    let page_store_id = page_store_id.parse::<store::StorePageId>()?;

    let page = state.store(&*dump_name)?.get_page_by_store_id(page_store_id)?;

    response_from_mapped_page(page, &*state, query).await
}

async fn get_page_by_slug(
    State(state): State<Arc<WebState>>,
    Path((dump_name, page_slug)): Path<(String, String)>,
    Query(query): Query<SinglePageQuery>,
) -> WebResult<impl IntoResponse> {

    let page = state.store(&*dump_name)?.get_page_by_slug(&*page_slug)?;

    response_from_mapped_page(page, &*state, query).await
}

#[derive(askama::Template)]
#[template(path = "page.html")]
struct PageHtml {
    title: String,

    slug: String,
    wikitext_html: String,

    dump_name: String,
    wikimedia_url_base: Option<String>,
}

#[derive(askama::Template)]
#[template(path = "page_debug.html")]
struct PageDebugHtml {
    title: String,

    ns_id: u64,
    mediawiki_id: u64,
    slug: String,
    store_page_id: StorePageId,

    wikitext: String,

    dump_name: String,
    wikimedia_url_base: Option<String>,
}

fn response_from_mapped_page(
    page: Option<store::MappedPage>,
    state: &WebState,
    query: SinglePageQuery,
) -> impl Future<Output = WebResult<Response>> + Send {
    let Some(page) = page else {
        return Either::Left(Either::Left(future::ok(_404_response(&"Page not found"))));
    };

    let store_page_id = page.store_id();
    let page_cap = match page.borrow() {
        Ok(p) => p,
        Err(e) => return Either::Left(Either::Right(future::err(e.into()))),
    };
    let page_dump = match dump::Page::try_from(&page_cap) {
        Ok(p) => p,
        Err(e) => return Either::Left(Either::Right(future::err(e.into()))),
    };

    let common_args = state.args().common.clone();
    let dump_name = page.dump_name();
    let wikimedia_url_base = dump::dump_name_to_wikimedia_url_base(&dump_name);

    if query.debug.unwrap_or(false) {
        let wikitext = page_dump.revision_text().unwrap_or("").to_string();
        let slug = slug::title_to_slug(&*page_dump.title);

        Either::Right(Either::Left({
            let html = PageDebugHtml {
                title: page_dump.title,

                ns_id: page_dump.ns_id,
                mediawiki_id: page_dump.id,
                slug,
                store_page_id,
                wikitext,

                wikimedia_url_base,

                // This moves dump_name, do it last.
                dump_name: dump_name.0,
            };
            future::ok(html.into_response())
        }))
    } else {
        Either::Right(Either::Right(async move {
            let wikitext_html = wikitext::convert_page_to_html(&page_dump,
                                                               &dump_name,
                                                               &*common_args.out_dir()).await?;
            let slug = slug::title_to_slug(&*page_dump.title);
            let html = PageHtml {
                title: page_dump.title,

                slug,
                wikitext_html,

                wikimedia_url_base,

                // This moves dump_name, do it last.
                dump_name: dump_name.0,
            };
            Ok(html.into_response())
        }))
    }
}



#[derive(Deserialize)]
struct PageSearchQuery {
    query: Option<String>,
}

#[derive(askama::Template)]
#[template(path = "page_search.html")]
struct PageSearchHtml {
    title: String,
    dump_name: String,

    query: Option<String>,

    pages: Vec<index::Page>,
    show_more_href: Option<String>,
}

async fn get_page_search(
    State(state): State<Arc<WebState>>,
    Query(query): Query<PageSearchQuery>,
) -> WebResult<impl IntoResponse> {

    let dump_name = state.store_dump_name();
    let Some(query_string) = query.query else {
        return Ok(PageSearchHtml {
                title: "Page search".to_string(),
                dump_name: dump_name.0,
                query: None,
                pages: Vec::with_capacity(0),
                show_more_href: None,
            });
    };

    let store = state.store(&*dump_name.0)?;

    let pages = store.page_search(&*query_string, None /* limit, TODO */)?;

    Ok(PageSearchHtml {
        title: "Page search".to_string(),
        dump_name: dump_name.0,
        query: Some(query_string),
        pages,
        show_more_href: None, // TODO
    })
}
