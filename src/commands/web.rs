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
    fmt::{self, Display, Write},
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
        .route("/", routing::get(get_root))
        .route("/:dump_name/category", routing::get(get_category))
        .route("/:dump_name/category/by-name/:category_slug", routing::get(get_category_by_slug))
        .route("/:dump_name/page/by-id/:page_id", routing::get(get_page_by_id))
        .route("/:dump_name/page/by-store-id/:page_store_id", routing::get(get_page_by_store_id))
        .route("/:dump_name/page/by-title/:page_slug", routing::get(get_page_by_title))

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
        WebError(error_response(&&*format!("Error: {e}")))
    }
}

impl From<fmt::Error> for WebError {
    fn from(e: fmt::Error) -> WebError {
        WebError::from_std_error(e)
    }
}

impl<T> From<std::sync::PoisonError<std::sync::MutexGuard<'_, T>>> for WebError {
    fn from(_e: std::sync::PoisonError<std::sync::MutexGuard<'_, T>>) -> WebError {
        WebError(error_response(&"PoisonError unlocking Mutex in web module"))
    }
}

#[cfg(any())]
impl<E> From<E> for WebError
    where E: std::error::Error + Send + Sync + 'static
{
    fn from(e: E) -> WebError {
        WebError(error_response(&*format!("Error: {e}")))
    }
}

fn error_response(msg: &dyn Display) -> Response {
    (
        // TODO: Render as HTML
        // TODO: Only show for localhost
        // TypedHeader(ContentType::html()),

        StatusCode::INTERNAL_SERVER_ERROR,
        TypedHeader(ContentType::text_utf8()),
        msg.to_string()
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

    error_response(&format!("panic: {s}"))
}

async fn get_root() -> impl IntoResponse {
    response::Html(format!(
        r#"
               <html>
               <body>
                   <p><a href="/enwiki/page/by-store-id/0.0">enwiki 0.0</a></p>
                   <p><a href="/enwiki/category">enwiki categories</a></p>
                   <p><a href="/enwiki/page/by-title/The_Matrix">The Matrix</a></p>
                   <p><a href="/enwiki/page/by-id/30007">enwiki 30007</a></p>
               </body>
               </html>
          "#))
}

#[derive(Deserialize)]
struct GetCategoryQuery {
    limit: Option<u64>,
    slug_lower_bound: Option<String>,
}

async fn get_category(
    State(state): State<Arc<WebState>>,
    Path(dump_name): Path<String>,
    Query(query): Query<GetCategoryQuery>
) -> StdResult<impl IntoResponse, WebError> {

    let limit = query.limit.unwrap_or(index::MAX_LIMIT).min(index::MAX_LIMIT);

    let categories = state.store.lock()?
        .get_category(
            query.slug_lower_bound.as_ref().map(|s| CategorySlug(s.clone())).as_ref(),
            Some(limit))?;

    let mut html: String = r#"
        <html>
        <head>
          <title>Categories | wmd</title>
        </head>
        <body>
          <h1>Categories</h1>
    "#.to_string();

    let last_slug = categories.last().cloned();
    let len = u64::try_from(categories.len()).expect("u64 from usize");

    for category in categories.into_iter() {
        write!(html, r#"
                        <p><a href="/{dump_name}/category/by-name/{slug}">{slug}</a></p>"#,
               slug = html_escape::encode_safe(&*category.0))?;
    }

    if let Some(CategorySlug(slug)) = last_slug {
        if limit == len {
            let limit_pair = match query.limit {
                Some(limit) => format!("&limit={}", limit),
                None => "".to_string(),
            };

            write!(html, r#"<p><a href="/{dump_name}/category?slug_lower_bound={slug_lower_bound}{limit}">
                               More</a>
                        </p>"#,
                   slug_lower_bound = html_escape::encode_safe(&*slug),
                   limit = limit_pair)?;
        }
    }

    write!(html, r#"
                    </body>
                    </html>"#)?;

    Ok(response::Html(html))
}

#[derive(Deserialize)]
struct GetCategoryByNameQuery {
    limit: Option<u64>,
    page_mediawiki_id_lower_bound: Option<u64>,
}

async fn get_category_by_slug(
    State(state): State<Arc<WebState>>,
    Path((dump_name, category_slug)): Path<(String, String)>,
    Query(query): Query<GetCategoryByNameQuery>,
) -> StdResult<impl IntoResponse, WebError> {

    let limit = query.limit.unwrap_or(index::MAX_LIMIT).min(index::MAX_LIMIT);

    let store = state.store.lock()?;
    let pages: Vec<index::Page> = store.get_category_pages(
        &CategorySlug(category_slug.clone()),
        query.page_mediawiki_id_lower_bound,
        Some(limit),
    )?;

    let mut html: String = format!(r#"
        <html>
        <head>
          <title>Category:{category_slug} | wmd</title>
        </head>
        <body>
          <h1>Category:{category_slug}</h1>
    "#,
        category_slug = html_escape::encode_safe(&*category_slug));

    let page_mediawiki_id_lower_bound = pages.last().map(|page| page.mediawiki_id);
    let len = u64::try_from(pages.len()).expect("u64 from usize");

    for page in pages.into_iter() {
        write!(html, r#"
                        <p><a href="/{dump_name}/page/by-title/{slug}">{slug}</a></p>"#,
               slug = html_escape::encode_safe(&*page.slug))?;
    }

    if let Some(page_mediawiki_id_lower_bound) = page_mediawiki_id_lower_bound {
        if len == limit {
            let limit_pair = match query.limit {
                Some(limit) => format!("&limit={}", limit),
                None => "".to_string(),
            };

            write!(html, r#"<p><a href="/{dump_name}/category/by-name/{category_slug}?page_mediawiki_id_lower_bound={page_mediawiki_id_lower_bound}{limit}">
                               More</a>
                        </p>"#,
                   category_slug = html_escape::encode_safe(&*category_slug),
                   limit = limit_pair)?;
        }
    }

    write!(html, "\n</body>\n</html>")?;

    Ok(response::Html(html))
}

#[axum::debug_handler]
async fn get_page_by_id(
    State(state): State<Arc<WebState>>,
    Path((_dump_name, page_id)): Path<(String, u64)>,
) -> StdResult<impl IntoResponse, WebError> {

    let Some(page_cap) = state.store.lock()?.get_page_by_mediawiki_id(page_id)? else {
        return Ok(
            (
                StatusCode::NOT_FOUND, // 404
                "Page not found by page ID",
            ).into_response()
        );
    };

    response_from_mapped_page(page_cap, &*state).await
}

async fn get_page_by_store_id(
    State(state): State<Arc<WebState>>,
    Path((_dump_name, page_store_id)): Path<(String, String)>,
) -> StdResult<impl IntoResponse, WebError> {

    let page_store_id = page_store_id.parse::<store::StorePageId>()?;

    let Some(page_cap) = state.store.lock()?.get_page_by_store_id(page_store_id)? else {
        return Ok(
            (
                StatusCode::NOT_FOUND, // 404
                "Page not found by store page ID",
            ).into_response()
        );
    };

    response_from_mapped_page(page_cap, &*state).await
}

async fn get_page_by_title(
    State(state): State<Arc<WebState>>,
    Path((_dump_name, page_slug)): Path<(String, String)>,
) -> StdResult<impl IntoResponse, WebError> {

    let Some(page_cap) = state.store.lock()?.get_page_by_slug(&*page_slug)? else {
        return Ok(
            (
                StatusCode::NOT_FOUND, // 404
                "Page not found by title",
            ).into_response()
        );
    };

    response_from_mapped_page(page_cap, &*state).await
}

fn response_from_mapped_page(mapped_page: store::MappedPage, state: &WebState
) -> impl Future<Output = StdResult<Response, WebError>> + Send {
    let store_page_id = mapped_page.store_id();
    let page_cap = match mapped_page.borrow() {
        Ok(p) => p,
        Err(e) => return Either::Left(Either::Left(future::err(e.into()))),
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
