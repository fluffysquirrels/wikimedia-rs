use askama::Template;
use axum::{
    extract::{Path, Query, State},
    headers::ContentType,
    http::{header, status::StatusCode, uri},
    response::{self, IntoResponse, Response},
    Router,
    routing,
    Server,
    TypedHeader,
};
use crate::{
    args::CommonArgs,
    dump::{self, CategorySlug},
    store::{self, index},
    Result,
    wikitext,
};
use futures::future::{self, Either};
use serde::Deserialize;
use std::{
    any::Any,
    fmt::{self, Display},
    future::Future,
    net::SocketAddr,
    result::Result as StdResult,
    sync::{Arc, Mutex},
};
use tower_http::{
    catch_panic::CatchPanicLayer,
    sensitive_headers::SetSensitiveHeadersLayer,
    trace::TraceLayer,
};

/// Run a web server that returns Wikimedia content.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,
}

struct WebState {
    args: Args,
    store: Mutex<store::Store>,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let state = WebState {
        args: args.clone(),
        store: Mutex::new(store::Options::from_common_args(&args.common).build()?),
    };

    let app = Router::new()
        .route("/", routing::get(get_index))
        .route("/:dump_name/category", routing::get(get_categories))
        .route("/:dump_name/category/by-name/:category_slug",
               routing::get(get_category_by_slug))
        .route("/:dump_name/page/by-id/:page_id", routing::get(get_page_by_id))
        .route("/:dump_name/page/by-store-id/:page_store_id", routing::get(get_page_by_store_id))
        .route("/:dump_name/page/by-title/:page_slug", routing::get(get_page_by_slug))
        .route("/test_panic", routing::get(|| async { panic!("Test panic") }))

        // .layer(
        //     tower::ServiceBuilder::new()
        //         .layer(HandleErrorLayer::new(oops))
        // )

        .with_state(Arc::new(state))

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

impl<T> From<std::sync::PoisonError<std::sync::MutexGuard<'_, T>>> for WebError {
    fn from(_e: std::sync::PoisonError<std::sync::MutexGuard<'_, T>>) -> WebError {
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
struct IndexHtml<'a>{
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
) -> StdResult<impl IntoResponse, WebError> {

    let limit = query.limit.unwrap_or(index::MAX_LIMIT).min(index::MAX_LIMIT);

    let categories = state.store.lock()?
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
) -> StdResult<impl IntoResponse, WebError> {

    let limit = query.limit.unwrap_or(index::MAX_LIMIT).min(index::MAX_LIMIT);

    let store = state.store.lock()?;
    let pages: Vec<index::Page> = store.get_category_pages(
        &CategorySlug(category_slug.clone()),
        query.page_mediawiki_id_lower_bound,
        Some(limit),
    )?;

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

#[axum::debug_handler]
async fn get_page_by_id(
    State(state): State<Arc<WebState>>,
    Path((_dump_name, page_id)): Path<(String, u64)>,
) -> StdResult<impl IntoResponse, WebError> {

    let page = state.store.lock()?.get_page_by_mediawiki_id(page_id)?;

    response_from_mapped_page(page, &*state).await
}

async fn get_page_by_store_id(
    State(state): State<Arc<WebState>>,
    Path((_dump_name, page_store_id)): Path<(String, String)>,
) -> StdResult<impl IntoResponse, WebError> {

    let page_store_id = page_store_id.parse::<store::StorePageId>()?;

    let page = state.store.lock()?.get_page_by_store_id(page_store_id)?;

    response_from_mapped_page(page, &*state).await
}

async fn get_page_by_slug(
    State(state): State<Arc<WebState>>,
    Path((_dump_name, page_slug)): Path<(String, String)>,
) -> StdResult<impl IntoResponse, WebError> {

    let page = state.store.lock()?.get_page_by_slug(&*page_slug)?;

    response_from_mapped_page(page, &*state).await
}

fn response_from_mapped_page(page: Option<store::MappedPage>, state: &WebState
) -> impl Future<Output = StdResult<Response, WebError>> + Send {
    let Some(page) = page else {
        return Either::Left(Either::Left(future::ok(_404_response(&"Page not found"))));
    };

    let store_page_id = page.store_id();
    let page_cap = match page.borrow() {
        Ok(p) => p,
        Err(e) => return Either::Left(Either::Right(future::err(e.into()))),
    };
    let page = match dump::Page::try_from(&page_cap) {
        Ok(p) => p,
        Err(e) => return Either::Left(Either::Right(future::err(e.into()))),
    };
    let common_args = state.args.common.clone();

    Either::Right(async move {
        let html = wikitext::convert_page_to_html(&common_args, &page,
                                                  Some(store_page_id)).await?;

        Ok(response::Html(html).into_response())
    })
}
