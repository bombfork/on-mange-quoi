//! Pages of the main flow after the menu is accepted: stock check and shopping list, also
//! available as plain text to copy or print.

use askama::Template;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};

use super::flow::{
    Field, RecipeView, encode, hidden, menu_fields, parse, recipe_views, stock_fields,
};
use super::{AppState, Pairs, render};
use crate::i18n::{MESSAGES, Messages};
use crate::shopping;

struct StockItem {
    name: String,
    in_stock: bool,
}

#[derive(Template)]
#[template(path = "stock.html")]
struct StockPage {
    t: &'static Messages,
    recipes: Vec<RecipeView>,
    menu_query: String,
    fields: Vec<Field>,
    ingredients: Vec<StockItem>,
    with_menu: bool,
}

/// The accepted menu, with the ingredients to tick when already in stock.
pub(super) async fn stock_check(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    let catalog = &state.catalog;
    let query = parse(&pairs, catalog);
    let ingredients = shopping::ingredients_to_check(catalog, &query.menu.proposed)
        .into_iter()
        .map(|name| StockItem {
            name: name.to_string(),
            in_stock: query.in_stock.iter().any(|stocked| stocked == name),
        })
        .collect();
    let menu_query = encode(&menu_fields(&query.menu));
    render(&StockPage {
        t: &MESSAGES,
        recipes: recipe_views(catalog, &query.menu.proposed),
        menu_query,
        fields: hidden(menu_fields(&query.menu)),
        ingredients,
        with_menu: query.with_menu.unwrap_or(true),
    })
}

/// Shopping list and, when asked for, the menu.
struct ShoppingList {
    items: Vec<String>,
    menu: Vec<String>,
    query: String,
}

fn shopping_list_from(state: &AppState, pairs: &[(String, String)]) -> ShoppingList {
    let catalog = &state.catalog;
    let query = parse(pairs, catalog);
    let with_menu = query.with_menu.unwrap_or(true);
    let items = shopping::shopping_list(catalog, &query.menu.proposed, &query.in_stock)
        .into_iter()
        .map(str::to_string)
        .collect();
    let menu = if with_menu {
        recipe_views(catalog, &query.menu.proposed)
            .into_iter()
            .map(|recipe| recipe.name)
            .collect()
    } else {
        Vec::new()
    };
    ShoppingList {
        items,
        menu,
        query: encode(&stock_fields(&query.menu, &query.in_stock, with_menu)),
    }
}

#[derive(Template)]
#[template(path = "shopping.html")]
struct ShoppingPage {
    t: &'static Messages,
    list: ShoppingList,
}

pub(super) async fn shopping_list(State(state): State<AppState>, Query(pairs): Pairs) -> Response {
    render(&ShoppingPage {
        t: &MESSAGES,
        list: shopping_list_from(&state, &pairs),
    })
}

pub(super) async fn shopping_list_text(
    State(state): State<AppState>,
    Query(pairs): Pairs,
) -> Response {
    let list = shopping_list_from(&state, &pairs);
    let m = &MESSAGES.shopping;
    let mut text = format!("{}\n", m.heading);
    if list.items.is_empty() {
        text.push_str(&format!("{}\n", m.empty));
    }
    for item in &list.items {
        text.push_str(&format!("- {item}\n"));
    }
    if !list.menu.is_empty() {
        text.push_str(&format!("\n{}\n", m.menu));
        for name in &list.menu {
            text.push_str(&format!("- {name}\n"));
        }
    }
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text).into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;

    use super::*;
    use crate::catalog::{Catalog, Ingredient, Recipe};
    use crate::web::test_support::{body_text, get};

    fn catalog() -> Catalog {
        let ingredient = |name: &str, staple| Ingredient {
            name: name.into(),
            staple,
        };
        let recipe = |id: &str, name: &str, ingredients: &[&str]| Recipe {
            id: id.into(),
            name: name.into(),
            ingredients: ingredients.iter().map(|i| i.to_string()).collect(),
            tags: Vec::new(),
        };
        Catalog {
            ingredients: vec![
                ingredient("œufs", false),
                ingredient("jambon", false),
                ingredient("pain de mie", false),
                ingredient("sel", true),
            ],
            tags: Vec::new(),
            recipes: vec![
                recipe(
                    "croque",
                    "Croque-monsieur",
                    &["pain de mie", "jambon", "sel"],
                ),
                recipe("omelette", "Omelette", &["œufs", "jambon", "sel"]),
            ],
        }
    }

    const MENU: &str = "meals=2&proposed=omelette&proposed=croque";

    async fn page(uri: &str) -> String {
        let response = get(catalog(), uri).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        body_text(response).await
    }

    #[tokio::test]
    async fn stock_check_lists_menu_ingredients_without_staples() {
        let body = page(&format!("/menu/accepted?{MENU}")).await;
        assert!(body.contains("Omelette") && body.contains("Croque-monsieur"));
        for name in ["œufs", "jambon", "pain de mie"] {
            assert!(
                body.contains(&format!(r#"name="in_stock" value="{name}" />"#)),
                "{name}"
            );
        }
        assert!(!body.contains(r#"value="sel""#));
        assert!(body.contains(r#"name="with_menu" value="1" checked"#));
    }

    #[tokio::test]
    async fn stock_check_restores_previous_answers() {
        let body = page(&format!(
            "/menu/accepted?{MENU}&in_stock=jambon&with_menu=0"
        ))
        .await;
        assert!(body.contains(r#"name="in_stock" value="jambon" checked"#));
        assert!(body.contains(r#"name="in_stock" value="œufs" />"#));
        assert!(body.contains(r#"name="with_menu" value="1" />"#));
    }

    #[tokio::test]
    async fn shopping_list_keeps_unticked_ingredients_and_the_menu() {
        let body = page(&format!(
            "/shopping-list?{MENU}&in_stock=jambon&with_menu=0&with_menu=1"
        ))
        .await;
        assert!(body.contains("<li>œufs</li>") && body.contains("<li>pain de mie</li>"));
        assert!(!body.contains("<li>jambon</li>"));
        assert!(body.contains("<li>Omelette</li>"));
    }

    #[tokio::test]
    async fn shopping_list_without_menu_when_unticked() {
        let body = page(&format!("/shopping-list?{MENU}&with_menu=0")).await;
        assert!(!body.contains("<li>Omelette</li>"));
    }

    #[tokio::test]
    async fn everything_in_stock_says_so() {
        let body = page(&format!(
            "/shopping-list?{MENU}&in_stock=jambon&in_stock=%C5%93ufs&in_stock=pain+de+mie"
        ))
        .await;
        assert!(body.contains(&MESSAGES.shopping.empty));
    }

    #[tokio::test]
    async fn plain_text_version() {
        let response = get(
            catalog(),
            &format!("/shopping-list.txt?{MENU}&in_stock=jambon&with_menu=1"),
        )
        .await;
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/plain; charset=utf-8"
        );
        let text = body_text(response).await;
        let m = &MESSAGES.shopping;
        assert_eq!(
            text,
            format!(
                "{}\n- œufs\n- pain de mie\n\n{}\n- Omelette\n- Croque-monsieur\n",
                m.heading, m.menu
            )
        );
    }
}
