//! Pages of the main flow up to the accepted menu: criteria form, proposal, accepted menu.
//!
//! The flow state travels in the query string. Actions (first proposal, rejection) redirect to
//! the URL of the resulting state, so reloading a page never changes the menu.

use askama::Template;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Redirect, Response};

use super::{AppState, render};
use crate::catalog::Catalog;
use crate::i18n::{MESSAGES, Messages, interpolate};
use crate::menu::{Criteria, MAX_MEALS, Menu, TagFilter};

/// Query key prefix of a tag filter: `tag:<name>=required|excluded`.
const TAG_PREFIX: &str = "tag:";

type Pairs = Query<Vec<(String, String)>>;

/// What a query string carries: the flow state, the requested action and pending notices.
#[derive(Debug, Default)]
struct FlowQuery {
    menu: Menu,
    reject: Option<String>,
    removed: usize,
}

/// Lenient on purpose: invalid values fall back to defaults and unknown tags are ignored.
fn parse(pairs: &[(String, String)], catalog: &Catalog) -> FlowQuery {
    let mut query = FlowQuery::default();
    for (key, value) in pairs {
        match key.as_str() {
            "meals" => {
                if let Ok(meals) = value.trim().parse::<usize>() {
                    query.menu.criteria.meals = meals.clamp(1, MAX_MEALS);
                }
            }
            "proposed" => query.menu.proposed.push(value.clone()),
            "rejected" => query.menu.rejected.push(value.clone()),
            "reject" => query.reject = Some(value.clone()),
            "removed" => query.removed = value.parse().unwrap_or(0),
            _ => {
                let Some(tag) = key.strip_prefix(TAG_PREFIX) else {
                    continue;
                };
                if !catalog.tags.iter().any(|known| known == tag) {
                    continue;
                }
                let tags = &mut query.menu.criteria.tags;
                match value.as_str() {
                    "required" => tags.insert(tag.to_string(), TagFilter::Required),
                    "excluded" => tags.insert(tag.to_string(), TagFilter::Excluded),
                    _ => tags.remove(tag),
                };
            }
        }
    }
    query
}

fn criteria_fields(criteria: &Criteria) -> Vec<(String, String)> {
    let mut fields = vec![("meals".to_string(), criteria.meals.to_string())];
    for (tag, filter) in &criteria.tags {
        let value = match filter {
            TagFilter::Required => "required",
            TagFilter::Excluded => "excluded",
        };
        fields.push((format!("{TAG_PREFIX}{tag}"), value.to_string()));
    }
    fields
}

fn menu_fields(menu: &Menu) -> Vec<(String, String)> {
    let mut fields = criteria_fields(&menu.criteria);
    let ids = |key: &str, ids: &[String]| {
        ids.iter()
            .map(|id| (key.to_string(), id.clone()))
            .collect::<Vec<_>>()
    };
    fields.extend(ids("proposed", &menu.proposed));
    fields.extend(ids("rejected", &menu.rejected));
    fields
}

fn encode(fields: &[(String, String)]) -> String {
    serde_urlencoded::to_string(fields).expect("string pairs always encode")
}

struct RecipeView {
    id: String,
    name: String,
    ingredients: String,
}

fn recipe_views(catalog: &Catalog, ids: &[String]) -> Vec<RecipeView> {
    ids.iter()
        .filter_map(|id| catalog.recipes.iter().find(|recipe| &recipe.id == id))
        .map(|recipe| RecipeView {
            id: recipe.id.clone(),
            name: recipe.name.clone(),
            ingredients: recipe.ingredients.join(", "),
        })
        .collect()
}

struct TagChoice {
    name: String,
    required: bool,
    excluded: bool,
}

#[derive(Template)]
#[template(path = "plan.html")]
struct PlanPage {
    t: &'static Messages,
    catalog_summary: String,
    meals: usize,
    max_meals: usize,
    tags: Vec<TagChoice>,
}

/// Criteria form, prefilled from the query string when coming back from a proposal.
pub(super) async fn plan(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    let catalog = &state.catalog;
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
        catalog_summary: interpolate(
            &MESSAGES.landing.catalog_summary,
            &[("count", &catalog.recipes.len())],
        ),
        meals: criteria.meals,
        max_meals: MAX_MEALS,
        tags,
    })
}

struct Field {
    name: String,
    value: String,
}

#[derive(Template)]
#[template(path = "menu.html")]
struct MenuPage {
    t: &'static Messages,
    recipes: Vec<RecipeView>,
    fields: Vec<Field>,
    notices: Vec<String>,
    criteria_query: String,
}

pub(super) async fn proposal(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    let catalog = &state.catalog;
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
        recipes: recipe_views(catalog, &menu.proposed),
        fields: menu_fields(&menu)
            .into_iter()
            .map(|(name, value)| Field { name, value })
            .collect(),
        notices,
        criteria_query: encode(&criteria_fields(&menu.criteria)),
    })
}

#[derive(Template)]
#[template(path = "accepted.html")]
struct AcceptedPage {
    t: &'static Messages,
    recipes: Vec<RecipeView>,
    menu_query: String,
}

pub(super) async fn accepted(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    let catalog = &state.catalog;
    let query = parse(&pairs, catalog);
    render(&AcceptedPage {
        t: &MESSAGES,
        recipes: recipe_views(catalog, &query.menu.proposed),
        menu_query: encode(&menu_fields(&query.menu)),
    })
}

#[cfg(test)]
mod tests {
    use axum::http::{StatusCode, header};

    use super::*;
    use crate::catalog::Recipe;
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

    #[test]
    fn query_round_trips_and_ignores_unknown_tags() {
        let pairs: Vec<(String, String)> = serde_urlencoded::from_str(
            "meals=99&tag%3Arapide=required&tag%3Aenfants=ignored&tag%3Ainconnu=required\
             &proposed=croque&rejected=omelette&reject=croque",
        )
        .unwrap();
        let query = parse(&pairs, &catalog());
        assert_eq!(query.menu.criteria.meals, MAX_MEALS);
        assert_eq!(
            query.menu.criteria.tags.into_iter().collect::<Vec<_>>(),
            [("rapide".to_string(), TagFilter::Required)]
        );
        assert_eq!(query.menu.proposed, ["croque"]);
        assert_eq!(query.menu.rejected, ["omelette"]);
        assert_eq!(query.reject.as_deref(), Some("croque"));

        let menu = Menu {
            criteria: Criteria {
                meals: 2,
                tags: [("végétarien".to_string(), TagFilter::Excluded)].into(),
            },
            proposed: vec!["croque".into()],
            rejected: vec!["omelette".into()],
        };
        let reparsed: Vec<(String, String)> =
            serde_urlencoded::from_str(&encode(&menu_fields(&menu))).unwrap();
        assert_eq!(parse(&reparsed, &catalog()).menu, menu);
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

    #[tokio::test]
    async fn accepted_menu_lists_recipes_and_links_back() {
        let uri = "/menu/accepted?meals=2&proposed=omelette&proposed=croque&rejected=ratatouille";
        let response = get(catalog(), uri).await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_text(response).await;
        assert!(body.contains("Omelette") && body.contains("Croque-monsieur"));
        assert!(body.contains(
            "/menu?meals=2&#38;proposed=omelette&#38;proposed=croque&#38;rejected=ratatouille"
        ));
    }
}
