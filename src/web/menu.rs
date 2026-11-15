//! Pages of the main flow up to the proposal: criteria form and proposed menu.
//!
//! Actions (first proposal, rejection) redirect to the URL of the resulting state, so reloading
//! a page never changes the menu.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};

use super::flow::{
    Field, RecipeView, criteria_fields, encode, hidden, menu_fields, parse, recipe_views,
};
use super::{AppState, Pairs, render};
use crate::i18n::{MESSAGES, Messages, interpolate};
use crate::menu::{MAX_MEALS, TagFilter};

struct TagChoice {
    name: String,
    required: bool,
    excluded: bool,
}

#[derive(Template)]
#[template(path = "plan.html")]
struct PlanPage {
    t: &'static Messages,
    catalog_errors: Vec<String>,
    catalog_summary: String,
    meals: usize,
    max_meals: usize,
    tags: Vec<TagChoice>,
}

/// Criteria form, prefilled from the query string when coming back from a proposal.
pub(super) async fn plan(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    let snapshot = state.live.snapshot();
    let catalog = &*snapshot.catalog;
    let criteria = parse(&pairs, catalog).menu.criteria;
    let tags = catalog
        .tags
        .iter()
        .map(|name| {
            let filter = criteria.tags.get(name);
            TagChoice {
                name: name.clone(),
                required: filter == Some(&TagFilter::Required),
                excluded: filter == Some(&TagFilter::Excluded),
            }
        })
        .collect();
    render(&PlanPage {
        t: &MESSAGES,
        catalog_errors: state.catalog_errors(&snapshot),
        catalog_summary: interpolate(
            &MESSAGES.landing.catalog_summary,
            &[("count", &catalog.recipes.len())],
        ),
        meals: criteria.meals,
        max_meals: MAX_MEALS,
        tags,
    })
}

#[derive(Template)]
#[template(path = "menu.html")]
struct MenuPage {
    t: &'static Messages,
    catalog_errors: Vec<String>,
    recipes: Vec<RecipeView>,
    fields: Vec<Field>,
    notices: Vec<String>,
    criteria_query: String,
}

pub(super) async fn proposal(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    let snapshot = state.live.snapshot();
    let catalog = &*snapshot.catalog;
    let query = parse(&pairs, catalog);
    let mut menu = query.menu.clone();
    let removed = menu.update(catalog, query.reject.as_deref(), &mut fastrand::Rng::new());

    if query.reject.is_some() || menu != query.menu {
        let mut fields = menu_fields(&menu);
        if removed > 0 {
            fields.push(("removed".to_string(), removed.to_string()));
        }
        return Redirect::to(&format!("/menu?{}", encode(&fields))).into_response();
    }

    let mut notices = Vec::new();
    if query.removed > 0 {
        notices.push(interpolate(
            &MESSAGES.menu.removed,
            &[("count", &query.removed)],
        ));
    }
    if menu.proposed.is_empty() {
        notices.push(MESSAGES.menu.none.clone());
    } else if menu.missing() > 0 {
        notices.push(interpolate(
            &MESSAGES.menu.missing,
            &[("count", &menu.missing())],
        ));
    }

    render(&MenuPage {
        t: &MESSAGES,
        catalog_errors: state.catalog_errors(&snapshot),
        recipes: recipe_views(catalog, &menu.proposed),
        fields: hidden(menu_fields(&menu)),
        notices,
        criteria_query: encode(&criteria_fields(&menu.criteria)),
    })
}

#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, header};

    use super::*;
    use crate::catalog::{Catalog, Recipe};
    use crate::web::test_support::{body_text, get};

    fn recipe(id: &str, name: &str, tags: &[&str]) -> Recipe {
        Recipe {
            id: id.into(),
            name: name.into(),
            ingredients: vec!["œufs".into(), "sel".into()],
            tags: tags.iter().map(|t| t.to_string()).collect(),
        }
    }

    fn catalog() -> Catalog {
        Catalog {
            choices: Vec::new(),
            ingredient_tags: Vec::new(),
            ingredients: Vec::new(),
            tags: vec!["rapide".into(), "enfants".into(), "végétarien".into()],
            recipes: vec![
                recipe("omelette", "Omelette", &["rapide", "végétarien"]),
                recipe("croque", "Croque-monsieur", &["rapide", "enfants"]),
                recipe("ratatouille", "Ratatouille", &["végétarien"]),
            ],
        }
    }

    /// Query pairs of the redirect target.
    fn redirect_pairs(response: &Response) -> Vec<(String, String)> {
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers()[header::LOCATION].to_str().unwrap();
        let (path, query) = location.split_once('?').unwrap();
        assert_eq!(path, "/menu");
        serde_urlencoded::from_str(query).unwrap()
    }

    fn values<'a>(pairs: &'a [(String, String)], key: &str) -> Vec<&'a str> {
        pairs
            .iter()
            .filter(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    #[tokio::test]
    async fn plan_page_lists_tags_and_restores_criteria() {
        let response = get(
            catalog(),
            "/?meals=3&tag:rapide=required&tag:enfants=excluded",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains(&MESSAGES.landing.intro));
        assert!(body.contains(r#"value="3""#));
        assert!(body.contains(r#"name="tag:rapide" value="required" checked"#));
        assert!(body.contains(r#"name="tag:enfants" value="excluded" checked"#));
        assert!(body.contains(r#"name="tag:végétarien" value="ignored" checked"#));
    }

    #[tokio::test]
    async fn first_proposal_redirects_to_a_stable_menu() {
        let response = get(catalog(), "/menu?meals=2&tag:rapide=required").await;
        let pairs = redirect_pairs(&response);
        let mut proposed = values(&pairs, "proposed");
        proposed.sort();
        assert_eq!(proposed, ["croque", "omelette"]);

        let page = get(catalog(), &format!("/menu?{}", encode(&pairs))).await;
        assert_eq!(page.status(), StatusCode::OK);
        let body = body_text(page).await;
        assert!(body.contains("Omelette") && body.contains("Croque-monsieur"));
        assert!(!body.contains("Ratatouille"));
        assert!(body.contains(r#"name="reject" value="omelette""#));
    }

    #[tokio::test]
    async fn rejecting_replaces_the_recipe_and_remembers_it() {
        let response = get(catalog(), "/menu?meals=1&proposed=omelette&reject=omelette").await;
        let pairs = redirect_pairs(&response);
        assert_eq!(values(&pairs, "rejected"), ["omelette"]);
        let proposed = values(&pairs, "proposed");
        assert_eq!(proposed.len(), 1);
        assert_ne!(proposed[0], "omelette");
    }

    #[tokio::test]
    async fn recipes_that_left_the_catalog_are_reported() {
        let response = get(catalog(), "/menu?meals=1&proposed=disparue").await;
        let pairs = redirect_pairs(&response);
        assert_eq!(values(&pairs, "removed"), ["1"]);
        assert_eq!(values(&pairs, "proposed").len(), 1);

        let body = body_text(get(catalog(), &format!("/menu?{}", encode(&pairs))).await).await;
        assert!(body.contains(&interpolate(&MESSAGES.menu.removed, &[("count", &1)])));
    }

    #[tokio::test]
    async fn missing_recipes_are_reported() {
        let response = get(
            catalog(),
            "/menu?meals=3&tag:enfants=required&proposed=croque",
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains(&interpolate(&MESSAGES.menu.missing, &[("count", &2)])));
    }

    #[tokio::test]
    async fn no_matching_recipe_is_reported() {
        let uri = "/menu?meals=2&tag:rapide=excluded&tag:v%C3%A9g%C3%A9tarien=excluded";
        let response = get(catalog(), uri).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains(&MESSAGES.menu.none));
        assert!(!body.contains(&MESSAGES.menu.accept));
    }
}
