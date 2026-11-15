//! HTTP routes. Pages are rendered on the server; static assets are embedded in the binary.

use std::sync::Arc;

use askama::Template;
use axum::Router;
use axum::extract::State;
use axum::http::{StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;

use crate::catalog::Catalog;
use crate::i18n::{MESSAGES, Messages, interpolate};

const PICO_CSS: &str = include_str!("../assets/pico.min.css");
const MAIN_CSS: &str = include_str!("../assets/main.css");

#[derive(Clone)]
struct AppState {
    catalog: Arc<Catalog>,
}

pub fn router(catalog: Arc<Catalog>) -> Router {
    Router::new()
        .route("/", get(landing))
        .route("/assets/pico.min.css", get(|| async { css(PICO_CSS) }))
        .route("/assets/main.css", get(|| async { css(MAIN_CSS) }))
        .with_state(AppState { catalog })
}

fn css(body: &'static str) -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], body)
}

#[derive(Template)]
#[template(path = "landing.html")]
struct LandingPage {
    t: &'static Messages,
    catalog_summary: String,
}

async fn landing(State(state): State<AppState>) -> Response {
    let catalog_summary = interpolate(
        &MESSAGES.landing.catalog_summary,
        &[("count", &state.catalog.recipes.len())],
    );
    render(&LandingPage {
        t: &MESSAGES,
        catalog_summary,
    })
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
mod tests {
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;

    async fn get(uri: &str) -> Response {
        router(Arc::new(Catalog::default()))
            .oneshot(Request::get(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn body_text(response: Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn landing_page_shows_title_and_intro() {
        let response = get("/").await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains(&MESSAGES.app.title));
        assert!(body.contains(&MESSAGES.landing.intro));
        assert!(body.contains(&interpolate(
            &MESSAGES.landing.catalog_summary,
            &[("count", &0)]
        )));
    }

    #[tokio::test]
    async fn stylesheets_are_served_as_css() {
        for uri in ["/assets/pico.min.css", "/assets/main.css"] {
            let response = get(uri).await;
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
        assert_eq!(get("/nope").await.status(), StatusCode::NOT_FOUND);
    }
}
