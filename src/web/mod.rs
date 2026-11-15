//! HTTP routes. Pages are rendered on the server; static assets are embedded in the binary.

mod cook;
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
use axum::routing::{get, post};

use crate::editor::Editor;
use crate::live::{LiveCatalog, Snapshot};
use crate::report;

const PICO_CSS: &str = include_str!("../../assets/pico.min.css");
const MAIN_CSS: &str = include_str!("../../assets/main.css");

/// Query string as ordered pairs, keeping repeated keys.
type Pairs = Query<Vec<(String, String)>>;

/// Whether the app may change the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    ReadWrite,
    /// No route writes, so the app can be exposed beyond the local network. The catalog pages
    /// show the served catalog, and the problems of the files on disk are only logged.
    ReadOnly,
}

#[derive(Clone)]
struct AppState {
    live: Arc<LiveCatalog>,
    editor: Arc<Editor>,
    mode: Mode,
}

impl AppState {
    /// Problems of the catalog files on disk, shown on every page while the previous catalog is
    /// still in use. Not in read-only mode: visitors cannot fix them, and they name server
    /// paths.
    fn catalog_errors(&self, snapshot: &Snapshot) -> Vec<String> {
        match self.mode {
            Mode::ReadWrite => snapshot.problems.iter().map(report::describe).collect(),
            Mode::ReadOnly => Vec::new(),
        }
    }
}

pub fn router(live: Arc<LiveCatalog>, mode: Mode) -> Router {
    let dir = live.dir().to_path_buf();
    let editor = Arc::new(match mode {
        Mode::ReadWrite => Editor::new(dir),
        Mode::ReadOnly => Editor::read_only(dir),
    });
    let pages = Router::new()
        .route("/", get(menu::plan))
        .route("/menu", get(menu::proposal))
        .route("/cook", get(cook::page))
        .route("/menu/accepted", get(shopping::stock_check))
        .route("/shopping-list", get(shopping::shopping_list))
        .route("/shopping-list.txt", get(shopping::shopping_list_text))
        .route("/catalog/recipes", get(editor::recipes))
        .route("/catalog/recipes/{id}", get(editor::edit_recipe))
        .route("/catalog/ingredients", get(editor::ingredients))
        .route("/catalog/ingredients/edit", get(editor::edit_ingredient))
        .route("/catalog/tags", get(editor::tags))
        .route("/catalog/tags/edit", get(editor::edit_tag))
        .route("/assets/pico.min.css", get(|| async { css(PICO_CSS) }))
        .route("/assets/main.css", get(|| async { css(MAIN_CSS) }));
    let routes = match mode {
        Mode::ReadOnly => pages,
        // Same paths as above merge: GET shows, POST edits.
        Mode::ReadWrite => pages
            .route(
                "/catalog/recipes/new",
                get(editor::new_recipe).post(editor::create_recipe),
            )
            .route("/catalog/recipes/{id}", post(editor::update_recipe))
            .route(
                "/catalog/recipes/{id}/delete",
                get(editor::confirm_delete_recipe).post(editor::delete_recipe),
            )
            .route("/catalog/ingredients/new", post(editor::add_ingredient))
            .route("/catalog/ingredients/edit", post(editor::update_ingredient))
            .route(
                "/catalog/ingredients/delete",
                post(editor::delete_ingredient),
            )
            .route(
                "/catalog/import",
                get(editor::import_form).post(editor::import),
            )
            .route("/catalog/tags/new", post(editor::add_tag))
            .route("/catalog/tags/edit", post(editor::update_tag))
            .route("/catalog/tags/delete", post(editor::delete_tag))
            .layer(middleware::from_fn(same_origin_posts)),
    };
    routes.with_state(AppState { live, editor, mode })
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

    use super::Mode;
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
        send_in(live, Mode::ReadWrite, request).await
    }

    pub async fn send_in(live: Arc<LiveCatalog>, mode: Mode, request: Request<Body>) -> Response {
        super::router(live, mode).oneshot(request).await.unwrap()
    }

    pub async fn get_read_only(live: Arc<LiveCatalog>, uri: &str) -> Response {
        let request = Request::get(uri).body(Body::empty()).unwrap();
        send_in(live, Mode::ReadOnly, request).await
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

    use axum::body::Body;
    use axum::http::Request;

    use super::test_support::{body_text, get, get_live, get_read_only, send_in};
    use super::*;
    use crate::catalog::{self, Catalog};
    use crate::i18n::MESSAGES;

    /// The example catalog, served from memory; its directory is the given empty one.
    fn example_in(dir: &TempDir) -> Arc<LiveCatalog> {
        let example = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/catalog");
        Arc::new(LiveCatalog::new(
            dir.path().to_path_buf(),
            catalog::load(&example).unwrap(),
        ))
    }

    #[tokio::test]
    async fn catalog_errors_are_shown_while_the_previous_catalog_is_served() {
        let dir = TempDir::new().unwrap();
        let live = example_in(&dir);

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
    async fn read_only_mode_hides_catalog_errors() {
        let dir = TempDir::new().unwrap();
        let live = example_in(&dir);
        live.reload();
        let body = body_text(get_read_only(live, "/").await).await;
        assert!(!body.contains(&MESSAGES.layout.catalog_errors));
        assert!(body.contains("Recettes au catalogue\u{a0}: 6"));
    }

    #[tokio::test]
    async fn read_only_mode_serves_every_page_but_no_edit() {
        let dir = TempDir::new().unwrap();
        let live = example_in(&dir);
        for uri in [
            "/",
            "/cook?have=jambon",
            "/menu/accepted?meals=1&proposed=croque-monsieur",
            "/shopping-list?meals=1&proposed=croque-monsieur&with_menu=1",
            "/shopping-list.txt?meals=1&proposed=croque-monsieur",
            "/catalog/recipes",
            "/catalog/recipes/croque-monsieur",
            "/catalog/ingredients",
            "/catalog/ingredients/edit?name=jambon",
            "/catalog/tags",
            "/catalog/tags/edit?name=enfants",
        ] {
            let response = get_read_only(Arc::clone(&live), uri).await;
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
        }
        for uri in [
            "/catalog/recipes/new",
            "/catalog/recipes/croque-monsieur/delete",
            "/catalog/import",
        ] {
            let response = get_read_only(Arc::clone(&live), uri).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{uri}");
        }
        for uri in [
            "/catalog/recipes/new",
            "/catalog/recipes/croque-monsieur",
            "/catalog/recipes/croque-monsieur/delete",
            "/catalog/ingredients/new",
            "/catalog/ingredients/edit",
            "/catalog/ingredients/delete",
            "/catalog/tags/new",
            "/catalog/tags/edit",
            "/catalog/tags/delete",
            "/catalog/import",
        ] {
            let request = Request::post(uri)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("sec-fetch-site", "same-origin")
                .body(Body::from(
                    "name=pain&version=x&yaml=ingredients%3A+%5Bpain%5D",
                ))
                .unwrap();
            let status = send_in(Arc::clone(&live), Mode::ReadOnly, request)
                .await
                .status();
            assert!(
                status == StatusCode::METHOD_NOT_ALLOWED || status == StatusCode::NOT_FOUND,
                "{uri}: {status}"
            );
        }
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            0,
            "nothing written"
        );
    }

    #[tokio::test]
    async fn unknown_route_is_not_found() {
        let response = get(Catalog::default(), "/nope").await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
