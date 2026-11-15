//! HTTP routes. Pages are rendered on the server; static assets are embedded in the binary.

mod editor;
mod flow;
mod menu;
mod shopping;

use std::sync::Arc;

use askama::Template;
use axum::Router;
use axum::extract::{Query, Request};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use crate::editor::Editor;
use crate::live::{LiveCatalog, Snapshot};
use crate::report;

const PICO_CSS: &str = include_str!("../../assets/pico.min.css");
const MAIN_CSS: &str = include_str!("../../assets/main.css");

/// Query string as ordered pairs, keeping repeated keys.
type Pairs = Query<Vec<(String, String)>>;

#[derive(Clone)]
struct AppState {
    live: Arc<LiveCatalog>,
    editor: Arc<Editor>,
}

pub fn router(live: Arc<LiveCatalog>) -> Router {
    let editor = Arc::new(Editor::new(live.dir().to_path_buf()));
    Router::new()
        .route("/", get(menu::plan))
        .route("/menu", get(menu::proposal))
        .route("/menu/accepted", get(shopping::stock_check))
        .route("/shopping-list", get(shopping::shopping_list))
        .route("/shopping-list.txt", get(shopping::shopping_list_text))
        .route("/catalog/recipes", get(editor::recipes))
        .route(
            "/catalog/recipes/new",
            get(editor::new_recipe).post(editor::create_recipe),
        )
        .route(
            "/catalog/recipes/{id}",
            get(editor::edit_recipe).post(editor::update_recipe),
        )
        .route(
            "/catalog/recipes/{id}/delete",
            get(editor::confirm_delete_recipe).post(editor::delete_recipe),
        )
        .route("/catalog/ingredients", get(editor::ingredients))
        .route(
            "/catalog/ingredients/new",
            axum::routing::post(editor::add_ingredient),
        )
        .route(
            "/catalog/ingredients/edit",
            get(editor::edit_ingredient).post(editor::update_ingredient),
        )
        .route(
            "/catalog/ingredients/delete",
            axum::routing::post(editor::delete_ingredient),
        )
        .route("/catalog/tags", get(editor::tags))
        .route("/catalog/tags/new", axum::routing::post(editor::add_tag))
        .route(
            "/catalog/tags/edit",
            get(editor::edit_tag).post(editor::update_tag),
        )
        .route(
            "/catalog/tags/delete",
            axum::routing::post(editor::delete_tag),
        )
        .route("/assets/pico.min.css", get(|| async { css(PICO_CSS) }))
        .route("/assets/main.css", get(|| async { css(MAIN_CSS) }))
        .layer(middleware::from_fn(same_origin_posts))
        .with_state(AppState { live, editor })
}

/// Rejects POST requests sent by other sites: without authentication, any page opened on the
/// local network could otherwise submit edits to the catalog.
async fn same_origin_posts(request: Request, next: Next) -> Response {
    if request.method() == Method::POST && !is_same_origin(request.headers()) {
        return StatusCode::FORBIDDEN.into_response();
    }
    next.run(request).await
}

/// `Sec-Fetch-Site`, set by the browser, wins: unlike `Host`, a reverse proxy does not rewrite
/// it. Browsers without it are checked by comparing `Origin` with `Host`.
fn is_same_origin(headers: &HeaderMap) -> bool {
    if let Some(site) = headers.get("sec-fetch-site") {
        return site == "same-origin" || site == "none";
    }
    match (headers.get(header::ORIGIN), headers.get(header::HOST)) {
        (Some(origin), Some(host)) => origin
            .to_str()
            .ok()
            .and_then(|origin| origin.split_once("://"))
            .is_some_and(|(_, authority)| authority.as_bytes() == host.as_bytes()),
        _ => true,
    }
}

fn css(body: &'static str) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], body)
}

/// Problems of the catalog files on disk, shown on every page while the previous catalog is
/// still in use.
fn catalog_errors(snapshot: &Snapshot) -> Vec<String> {
    snapshot.problems.iter().map(report::describe).collect()
}

fn render(page: &impl Template) -> Response {
    match page.render() {
        Ok(html) => Html(html).into_response(),
        Err(error) => {
            eprintln!("{error}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

#[cfg(test)]
mod test_support {
    use std::sync::Arc;

    use axum::body::Body;
    use axum::http::Request;
    use axum::response::Response;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use crate::catalog::Catalog;
    use crate::live::LiveCatalog;

    /// Serves `catalog` from memory; its directory is an empty temporary one.
    pub async fn get(catalog: Catalog, uri: &str) -> Response {
        let dir = tempfile::TempDir::new().unwrap();
        let live = LiveCatalog::new(dir.path().to_path_buf(), catalog);
        get_live(Arc::new(live), uri).await
    }

    pub async fn get_live(live: Arc<LiveCatalog>, uri: &str) -> Response {
        send(live, Request::get(uri).body(Body::empty()).unwrap()).await
    }

    pub async fn send(live: Arc<LiveCatalog>, request: Request<Body>) -> Response {
        super::router(live).oneshot(request).await.unwrap()
    }

    /// A same-origin form submission.
    pub async fn post_form(live: Arc<LiveCatalog>, uri: &str, body: &str) -> Response {
        let request = Request::post(uri)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(Body::from(body.to_string()))
            .unwrap();
        send(live, request).await
    }

    pub async fn body_text(response: Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::TempDir;

    use super::test_support::{body_text, get, get_live};
    use super::*;
    use crate::catalog::{self, Catalog};
    use crate::i18n::MESSAGES;

    #[tokio::test]
    async fn catalog_errors_are_shown_while_the_previous_catalog_is_served() {
        let dir = TempDir::new().unwrap();
        let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/catalog");
        let live = Arc::new(LiveCatalog::new(
            dir.path().to_path_buf(),
            catalog::load(&example).unwrap(),
        ));

        // The temporary directory is empty: reloading it fails.
        live.reload();
        let body = body_text(get_live(Arc::clone(&live), "/").await).await;
        assert!(body.contains(&MESSAGES.layout.catalog_errors));
        assert!(body.contains(&MESSAGES.catalog.missing));
        assert!(body.contains("Recettes au catalogue\u{a0}: 6"));

        fs::write(dir.path().join("ingredients.yml"), "ingredients: [pain]\n").unwrap();
        fs::write(dir.path().join("tags.yml"), "tags: []\n").unwrap();
        fs::create_dir(dir.path().join("recipes")).unwrap();
        live.reload();
        let body = body_text(get_live(live, "/").await).await;
        assert!(!body.contains(&MESSAGES.layout.catalog_errors));
        assert!(body.contains("Recettes au catalogue\u{a0}: 0"));
    }

    #[tokio::test]
    async fn stylesheets_are_served_as_css() {
        for uri in ["/assets/pico.min.css", "/assets/main.css"] {
            let response = get(Catalog::default(), uri).await;
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/css; charset=utf-8",
                "{uri}"
            );
        }
    }

    #[tokio::test]
    async fn unknown_route_is_not_found() {
        let response = get(Catalog::default(), "/nope").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
