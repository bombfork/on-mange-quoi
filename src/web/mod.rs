//! HTTP routes. Pages are rendered on the server; static assets are embedded in the binary.

mod menu;

use std::sync::Arc;

use askama::Template;
use axum::Router;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use crate::catalog::Catalog;

const PICO_CSS: &str = include_str!("../../assets/pico.min.css");
const MAIN_CSS: &str = include_str!("../../assets/main.css");

#[derive(Clone)]
struct AppState {
    catalog: Arc<Catalog>,
}

pub fn router(catalog: Arc<Catalog>) -> Router {
    Router::new()
        .route("/", get(menu::plan))
        .route("/menu", get(menu::proposal))
        .route("/menu/accepted", get(menu::accepted))
        .route("/assets/pico.min.css", get(|| async { css(PICO_CSS) }))
        .route("/assets/main.css", get(|| async { css(MAIN_CSS) }))
        .with_state(AppState { catalog })
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

    use crate::catalog::Catalog;

    pub async fn get(catalog: Catalog, uri: &str) -> Response {
        super::router(Arc::new(catalog))
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    pub async fn body_text(response: Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::get;
    use super::*;

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
