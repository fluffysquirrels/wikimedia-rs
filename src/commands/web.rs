use axum::{
    extract::{Path, State},
    headers::ContentType,
    http::{status::StatusCode, uri},
    response::{self, IntoResponse, Response},
    Router,
    routing,
    Server,
    TypedHeader,
};
use crate::{
    args::CommonArgs,
    dump,
    store,
    Result,
    wikitext,
};
use std::{
    net::SocketAddr,
    sync::Arc,
};

/// Run a web server that returns Wikimedia content.
#[derive(clap::Args, Clone, Debug)]
pub struct Args {
    #[clap(flatten)]
    common: CommonArgs,
}

struct WebState {
    args: Args,
    page_store: store::Store,
}

#[tracing::instrument(level = "trace")]
pub async fn main(args: Args) -> Result<()> {
    let state = WebState {
        args: args.clone(),
        page_store: store::Options::from_common_args(&args.common).build_store()?,
    };

    let app = Router::new()
        .route("/", routing::get(get_root))
        .route("/:dump_name/page/by-id/:page_id", routing::get(get_page_by_id))
        .route("/:dump_name/page/by-store-id/:page_store_id", routing::get(get_page_by_store_id))
        .route("/:dump_name/page/by-title/:page_slug", routing::get(get_page_by_title))

        // .layer(
        //     tower::ServiceBuilder::new()
        //         .layer(HandleErrorLayer::new(oops))
        // )

        .with_state(Arc::new(state))
        ;

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
           .serve(app.into_make_service())
           .await?;

    Ok(())
}

async fn get_root() -> impl IntoResponse {
    response::Html(format!(
        r#"
               <html>
               <body>
                   <p><a href="/enwiki/page/by-store-id/0.0">enwiki 0.0</a></p>
                   <p><a href="/enwiki/page/by-title/Foster_Air_Force_Base">Foster Air Force Base</a></p>
                   <p><a href="/enwiki/page/by-id/4045403">enwiki 4045403</a></p>
               </body>
               </html>
          "#))
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
        WebError(
            (
                // TODO: Render as HTML
                // TODO: Only show for localhost
                // TypedHeader(ContentType::html()),

                StatusCode::INTERNAL_SERVER_ERROR,
                TypedHeader(ContentType::text_utf8()),
                format!("Error: {e}"),
            ).into_response()
        )
    }
}

#[cfg(any())]
impl<E> From<E> for WebError
    where E: std::error::Error + Send + Sync + 'static
{
    fn from(e: E) -> WebError {
        WebError(
            (
                // TODO: Render as HTML
                // TODO: Only show for localhost
                // TypedHeader(ContentType::html()),

                StatusCode::INTERNAL_SERVER_ERROR,
                TypedHeader(ContentType::text_utf8()),
                format!("Error: {e}"),
            ).into_response()
        )
    }
}

async fn get_page_by_store_id(
    State(state): State<Arc<WebState>>,
    Path((_dump_name, page_store_id)): Path<(String, String)>,
) -> std::result::Result<impl IntoResponse, WebError> {

    let page_store_id = page_store_id.parse::<store::StorePageId>()?;

    let Some(page_fb) = state.page_store.get_page_by_store_id(page_store_id)? else {
        return Ok(
            (
                StatusCode::NOT_FOUND, // 404
                "Page not found by store page ID",
            ).into_response()
        );
    };
    let page = dump::Page::try_from(&page_fb.borrow())?;
    let html = wikitext::convert_page_to_html(&state.args.common, &page).await?;

    Ok(response::Html(html).into_response())
}


async fn get_page_by_id(
    State(state): State<Arc<WebState>>,
    Path((_dump_name, page_id)): Path<(String, String)>,
) -> std::result::Result<impl IntoResponse, WebError> {

    let page_id = page_id.parse::<u64>()
                         .map_err(|e| WebError::from_std_error(e))?;

    let Some(page_fb) = state.page_store.get_page_by_mediawiki_id(page_id)? else {
        return Ok(
            (
                StatusCode::NOT_FOUND, // 404
                "Page not found by page ID",
            ).into_response()
        );
    };
    let page = dump::Page::try_from(&page_fb.borrow())?;
    let html = wikitext::convert_page_to_html(&state.args.common, &page).await?;

    Ok(response::Html(html).into_response())
}

async fn get_page_by_title(
    State(state): State<Arc<WebState>>,
    Path((_dump_name, page_slug)): Path<(String, String)>,
) -> std::result::Result<impl IntoResponse, WebError> {

    let Some(page_fb) = state.page_store.get_page_by_slug(&*page_slug)? else {
        return Ok(
            (
                StatusCode::NOT_FOUND, // 404
                "Page not found by title",
            ).into_response()
        );
    };
    let page = dump::Page::try_from(&page_fb.borrow())?;
    let html = wikitext::convert_page_to_html(&state.args.common, &page).await?;

    Ok(response::Html(html).into_response())
}
